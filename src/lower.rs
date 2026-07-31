//! Reference lowering from typed HIR to Shape IR 0.1.

use std::collections::{HashMap, HashSet};

use crate::FieldId as HirFieldId;
use crate::ast::{
    AggregateFunction as SourceAggregateFunction, BinaryOperator, JoinKind, NullPlacement,
    OrderDirection, RankingFunction as SourceRankingFunction, SetOperator, UnaryOperator,
};
use crate::hir::{
    self, AggregateArgument as HirAggregateArgument, ExpressionKind as H,
    QueryBody as HirQueryBody, SelectQuery as HirSelectQuery, TablePrimary as HirTablePrimary,
    TypedExpression, TypedField, TypedProgram, TypedQueryExpression,
};
use crate::shape_ir::{
    AggregateDefinition, AggregateFunction, BinaryOperation, CaseArm, CollectionKind, Direction,
    Expression, ExpressionKind, Field, FieldId, Graph, GroupingKey, JoinType, Node, NodeId,
    NodeKind, NullPlacement as IrNullPlacement, OrderingItem, ProjectEntry, RankingFunction,
    SetOperation, SetQuantifier, UnaryOperation, WindowDefinition,
};
use crate::{CteId, TypeDescriptor};

/// Lowers a successfully typed program to a valid Shape IR 0.1 graph.
///
/// Static source errors cannot reach this function. Any failure of the final
/// internal validation is therefore an implementation defect and is asserted
/// rather than exposed as another source error phase.
pub fn lower(program: TypedProgram) -> Graph {
    let mut lowerer = Lowerer::new();
    let root = lowerer.lower_query_expression(program.query);
    let graph = Graph::new(root.node, lowerer.nodes);
    graph
        .validate()
        .expect("reference lowering must always produce valid Shape IR");
    graph
}

#[derive(Clone)]
struct Plan {
    node: NodeId,
    schema: Vec<Field>,
    collection: CollectionKind,
    environment: FieldEnvironment,
}

type FieldEnvironment = HashMap<HirFieldId, FieldId>;
type InvocationKey = (usize, usize);

#[derive(Clone, Default)]
struct ScalarContext {
    fields: FieldEnvironment,
    grouping_keys: Vec<(hir::TypedExpression, FieldId)>,
    aggregates: HashMap<InvocationKey, FieldId>,
    windows: HashMap<InvocationKey, FieldId>,
}

struct SelectResult {
    plan: Plan,
    lowered_order: Option<Vec<Expression>>,
}

struct Lowerer {
    nodes: Vec<Node>,
    next_node: u32,
    next_temporary: u32,
    cte_definitions: HashMap<CteId, TypedQueryExpression>,
    lowered_ctes: HashMap<CteId, Plan>,
}

impl Lowerer {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            next_node: 0,
            next_temporary: 0,
            cte_definitions: HashMap::new(),
            lowered_ctes: HashMap::new(),
        }
    }

    fn lower_query_expression(&mut self, query: TypedQueryExpression) -> Plan {
        for declaration in &query.common_table_expressions {
            self.cte_definitions
                .entry(declaration.id)
                .or_insert_with(|| declaration.query.as_ref().clone());
        }

        let public_schema = lower_schema(&query.result_fields);
        let order_by = query.order_by;
        let row_bound = query.row_bound;
        let span_body = query.body;

        let SelectResult {
            mut plan,
            lowered_order,
        } = match span_body {
            HirQueryBody::Select(select) => {
                let coupled_order =
                    (!select.distinct && !order_by.is_empty()).then_some(order_by.as_slice());
                self.lower_select(*select, coupled_order)
            }
            body => SelectResult {
                plan: self.lower_query_body(body),
                lowered_order: None,
            },
        };

        if !order_by.is_empty() {
            if lowered_order.is_none() && plan.collection == CollectionKind::Ordered {
                plan = self.consume_as_bag(plan);
            }
            let expressions = lowered_order.unwrap_or_else(|| {
                let context = ScalarContext {
                    fields: plan.environment.clone(),
                    ..ScalarContext::default()
                };
                order_by
                    .iter()
                    .map(|item| self.lower_scalar(&item.expression, &context))
                    .collect()
            });
            plan = self.lower_order_and_bound(
                plan,
                &public_schema,
                &order_by,
                expressions,
                row_bound.as_ref(),
            );
        }

        plan.environment = environment_from_hir_schema(&query.result_fields);
        debug_assert_eq!(plan.schema, public_schema);
        plan
    }

    fn lower_query_body(&mut self, body: HirQueryBody<TypeDescriptor, TypeDescriptor>) -> Plan {
        match body {
            HirQueryBody::Select(select) => self.lower_select(*select, None).plan,
            HirQueryBody::Parenthesized { query, .. } => self.lower_query_expression(*query),
            HirQueryBody::SetOperation {
                left,
                operator,
                all,
                right,
                result_fields,
                ..
            } => {
                let left = self.lower_query_body(*left);
                let left = self.consume_as_bag(left);
                let right = self.lower_query_body(*right);
                let right = self.consume_as_bag(right);
                let schema = lower_schema(&result_fields);
                let node = self.push_node(
                    NodeKind::Set {
                        left: left.node,
                        right: right.node,
                        operation: match operator {
                            SetOperator::Union => SetOperation::Union,
                            SetOperator::Intersect => SetOperation::Intersect,
                            SetOperator::Except => SetOperation::Except,
                        },
                        quantifier: if all {
                            SetQuantifier::All
                        } else {
                            SetQuantifier::Distinct
                        },
                    },
                    schema.clone(),
                    CollectionKind::Bag,
                );
                Plan {
                    node,
                    schema,
                    collection: CollectionKind::Bag,
                    environment: environment_from_hir_schema(&result_fields),
                }
            }
        }
    }

    fn lower_select(
        &mut self,
        select: HirSelectQuery<TypeDescriptor, TypeDescriptor>,
        outer_order: Option<&[hir::OrderItem<TypeDescriptor, TypeDescriptor>]>,
    ) -> SelectResult {
        let mut plan = self.lower_from(select.from);
        let source_context = ScalarContext {
            fields: plan.environment.clone(),
            ..ScalarContext::default()
        };

        if let Some(predicate) = &select.where_clause {
            let predicate = self.lower_scalar(predicate, &source_context);
            let node = self.push_node(
                NodeKind::Filter {
                    input: plan.node,
                    predicate,
                },
                plan.schema.clone(),
                plan.collection,
            );
            plan.node = node;
        }

        let mut context = source_context;
        if select.is_aggregate {
            let (aggregate_plan, aggregate_context) = self.lower_grouping_stage(
                plan,
                &select.group_by,
                &select.select_list,
                select.having.as_ref(),
                &context,
            );
            plan = aggregate_plan;
            context = aggregate_context;
        }

        if let Some(having) = &select.having {
            let predicate = self.lower_scalar(having, &context);
            let node = self.push_node(
                NodeKind::Filter {
                    input: plan.node,
                    predicate,
                },
                plan.schema.clone(),
                plan.collection,
            );
            plan.node = node;
        }

        let (window_plan, window_context) = self.lower_window_stage(
            plan,
            &select.select_list,
            outer_order.unwrap_or(&[]),
            context,
        );
        plan = window_plan;
        context = window_context;

        let public_schema = lower_schema(&select.result_fields);
        let public_ids = public_schema
            .iter()
            .map(|field| field.id.clone())
            .collect::<HashSet<_>>();
        let mut lowered_order = outer_order.map(|items| {
            let mut ordering_context = context.clone();
            ordering_context
                .fields
                .extend(environment_from_hir_schema(&select.result_fields));
            items
                .iter()
                .map(|item| self.lower_scalar(&item.expression, &ordering_context))
                .collect::<Vec<_>>()
        });

        let support_ids = lowered_order
            .as_ref()
            .map(|expressions| {
                let available = plan
                    .schema
                    .iter()
                    .map(|field| field.id.clone())
                    .collect::<HashSet<_>>();
                expressions
                    .iter()
                    .flat_map(referenced_fields)
                    .filter(|field| available.contains(field) && !public_ids.contains(field))
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();

        let mut entries = Vec::new();
        for item in &select.select_list {
            entries.push(ProjectEntry::Compute {
                output: lower_field_id(item.output.id),
                expression: self.lower_scalar(&item.expression, &context),
            });
        }
        let mut projection_schema = public_schema.clone();
        for field in &plan.schema {
            if support_ids.contains(&field.id) {
                entries.push(ProjectEntry::Keep(field.id.clone()));
                projection_schema.push(field.clone());
            }
        }
        let node = self.push_node(
            NodeKind::Project {
                input: plan.node,
                entries,
            },
            projection_schema.clone(),
            plan.collection,
        );
        plan = Plan {
            node,
            schema: projection_schema,
            collection: plan.collection,
            environment: environment_from_hir_schema(&select.result_fields),
        };

        if select.distinct {
            let node = self.push_node(
                NodeKind::Distinct { input: plan.node },
                public_schema.clone(),
                CollectionKind::Bag,
            );
            plan = Plan {
                node,
                schema: public_schema,
                collection: CollectionKind::Bag,
                environment: environment_from_hir_schema(&select.result_fields),
            };
            lowered_order = None;
        }

        SelectResult {
            plan,
            lowered_order,
        }
    }

    fn lower_from(
        &mut self,
        joined_tables: Vec<hir::JoinedTable<TypeDescriptor, TypeDescriptor>>,
    ) -> Plan {
        let mut complete: Option<Plan> = None;
        for joined in joined_tables {
            let mut current = self.lower_table_primary(joined.left);
            for join in joined.joins {
                let right = self.lower_table_primary(join.right);
                let mut condition_context = ScalarContext {
                    fields: current.environment.clone(),
                    ..ScalarContext::default()
                };
                condition_context.fields.extend(right.environment.clone());
                let condition = join
                    .condition
                    .as_ref()
                    .map(|condition| self.lower_scalar(condition, &condition_context));
                current = self.lower_join(current, right, join.kind, condition);
            }
            complete = Some(match complete {
                Some(left) => self.lower_join(left, current, JoinKind::Cross, None),
                None => current,
            });
        }
        complete.expect("ShapeSQL SELECT always has at least one FROM source")
    }

    fn lower_table_primary(
        &mut self,
        primary: HirTablePrimary<TypeDescriptor, TypeDescriptor>,
    ) -> Plan {
        match primary {
            HirTablePrimary::Catalog {
                binding,
                occurrence,
                ..
            } => {
                let schema = lower_schema(&occurrence.fields);
                let node = self.push_node(
                    NodeKind::Input { binding },
                    schema.clone(),
                    CollectionKind::Bag,
                );
                Plan {
                    node,
                    schema,
                    collection: CollectionKind::Bag,
                    environment: environment_from_hir_schema(&occurrence.fields),
                }
            }
            HirTablePrimary::CommonTableExpression {
                declaration,
                occurrence,
                ..
            } => {
                let source = self.lower_cte(declaration);
                self.instantiate_occurrence(source, &occurrence.fields)
            }
            HirTablePrimary::Derived {
                query, occurrence, ..
            } => {
                let source = self.lower_query_expression(*query);
                let source = self.consume_as_bag(source);
                self.instantiate_occurrence(source, &occurrence.fields)
            }
        }
    }

    fn lower_cte(&mut self, id: CteId) -> Plan {
        if let Some(plan) = self.lowered_ctes.get(&id) {
            return plan.clone();
        }
        let query = self
            .cte_definitions
            .get(&id)
            .cloned()
            .expect("typed CTE reference has a declaration");
        let plan = self.lower_query_expression(query);
        let plan = self.consume_as_bag(plan);
        self.lowered_ctes.insert(id, plan.clone());
        plan
    }

    fn instantiate_occurrence(&mut self, source: Plan, fields: &[TypedField]) -> Plan {
        let schema = lower_schema(fields);
        let entries = source
            .schema
            .iter()
            .zip(&schema)
            .map(|(source, output)| ProjectEntry::Compute {
                output: output.id.clone(),
                expression: Expression::new(
                    ExpressionKind::Field(source.id.clone()),
                    source.descriptor,
                ),
            })
            .collect();
        let node = self.push_node(
            NodeKind::Project {
                input: source.node,
                entries,
            },
            schema.clone(),
            CollectionKind::Bag,
        );
        Plan {
            node,
            schema,
            collection: CollectionKind::Bag,
            environment: environment_from_hir_schema(fields),
        }
    }

    fn lower_join(
        &mut self,
        left: Plan,
        right: Plan,
        kind: JoinKind,
        condition: Option<Expression>,
    ) -> Plan {
        let mut schema = match kind {
            JoinKind::Cross | JoinKind::Inner => left.schema.clone(),
            JoinKind::Left => left.schema.clone(),
            JoinKind::Right | JoinKind::Full => nullable_schema(&left.schema),
        };
        schema.extend(match kind {
            JoinKind::Left | JoinKind::Full => nullable_schema(&right.schema),
            JoinKind::Cross | JoinKind::Inner | JoinKind::Right => right.schema.clone(),
        });
        let mut environment = left.environment;
        environment.extend(right.environment);
        let node = self.push_node(
            NodeKind::Join {
                left: left.node,
                right: right.node,
                join_type: match kind {
                    JoinKind::Cross => JoinType::Cross,
                    JoinKind::Inner => JoinType::Inner,
                    JoinKind::Left => JoinType::Left,
                    JoinKind::Right => JoinType::Right,
                    JoinKind::Full => JoinType::Full,
                },
                condition,
            },
            schema.clone(),
            CollectionKind::Bag,
        );
        Plan {
            node,
            schema,
            collection: CollectionKind::Bag,
            environment,
        }
    }

    fn lower_grouping_stage(
        &mut self,
        input: Plan,
        group_by: &[TypedExpression],
        select_list: &[hir::SelectField<TypeDescriptor, TypeDescriptor>],
        having: Option<&TypedExpression>,
        source_context: &ScalarContext,
    ) -> (Plan, ScalarContext) {
        let mut canonical_groups: Vec<(TypedExpression, FieldId)> = Vec::new();
        let mut grouping_keys = Vec::new();
        let mut schema = Vec::new();
        for expression in group_by {
            if canonical_groups
                .iter()
                .any(|(existing, _)| hir::structurally_equal(existing, expression))
            {
                continue;
            }
            let output = self.temporary_field();
            grouping_keys.push(GroupingKey {
                output: output.clone(),
                expression: self.lower_scalar(expression, source_context),
            });
            schema.push(Field::new(output.clone(), "", expression.annotation));
            canonical_groups.push((expression.clone(), output));
        }

        let mut aggregate_expressions = Vec::new();
        for item in select_list {
            collect_grouping_aggregates(&item.expression, &mut aggregate_expressions);
        }
        if let Some(having) = having {
            collect_grouping_aggregates(having, &mut aggregate_expressions);
        }

        let mut aggregates = Vec::new();
        let mut aggregate_map = HashMap::new();
        for expression in aggregate_expressions {
            let H::Aggregate {
                function,
                argument,
                window: None,
            } = &expression.kind
            else {
                unreachable!("collector returns grouping aggregates")
            };
            let output = self.temporary_field();
            let (function, argument) =
                self.lower_aggregate_invocation(*function, argument, source_context);
            aggregates.push(AggregateDefinition {
                output: output.clone(),
                function,
                argument,
            });
            schema.push(Field::new(output.clone(), "", expression.annotation));
            aggregate_map.insert(invocation_key(expression), output);
        }

        let node = self.push_node(
            NodeKind::Aggregate {
                input: input.node,
                grouping_keys,
                aggregates,
            },
            schema.clone(),
            CollectionKind::Bag,
        );
        (
            Plan {
                node,
                schema,
                collection: CollectionKind::Bag,
                environment: HashMap::new(),
            },
            ScalarContext {
                fields: HashMap::new(),
                grouping_keys: canonical_groups,
                aggregates: aggregate_map,
                windows: HashMap::new(),
            },
        )
    }

    fn lower_window_stage(
        &mut self,
        input: Plan,
        select_list: &[hir::SelectField<TypeDescriptor, TypeDescriptor>],
        outer_order: &[hir::OrderItem<TypeDescriptor, TypeDescriptor>],
        mut context: ScalarContext,
    ) -> (Plan, ScalarContext) {
        let mut expressions = Vec::new();
        for item in select_list {
            collect_windows(&item.expression, &mut expressions);
        }
        for item in outer_order {
            collect_windows(&item.expression, &mut expressions);
        }
        if expressions.is_empty() {
            return (input, context);
        }

        let mut definitions = Vec::with_capacity(expressions.len());
        let mut schema = input.schema.clone();
        let mut window_map = HashMap::new();
        for expression in expressions {
            let output = self.temporary_field();
            let definition = match &expression.kind {
                H::Aggregate {
                    function,
                    argument,
                    window: Some(window),
                } => {
                    let (function, argument) =
                        self.lower_aggregate_invocation(*function, argument, &context);
                    WindowDefinition::PartitionedAggregate {
                        output: output.clone(),
                        function,
                        argument,
                        partition_by: window
                            .partition_by
                            .iter()
                            .map(|expression| self.lower_scalar(expression, &context))
                            .collect(),
                    }
                }
                H::Ranking {
                    function,
                    partition_by,
                    order_by,
                } => WindowDefinition::Ranking {
                    output: output.clone(),
                    function: lower_ranking_function(*function),
                    partition_by: partition_by
                        .iter()
                        .map(|expression| self.lower_scalar(expression, &context))
                        .collect(),
                    order_by: order_by
                        .iter()
                        .map(|item| self.lower_ordering_item(item, &context))
                        .collect(),
                },
                _ => unreachable!("collector returns window expressions"),
            };
            definitions.push(definition);
            schema.push(Field::new(output.clone(), "", expression.annotation));
            window_map.insert(invocation_key(expression), output);
        }
        let node = self.push_node(
            NodeKind::Window {
                input: input.node,
                definitions,
            },
            schema.clone(),
            CollectionKind::Bag,
        );
        context.windows = window_map;
        (
            Plan {
                node,
                schema,
                collection: CollectionKind::Bag,
                environment: input.environment,
            },
            context,
        )
    }

    fn lower_aggregate_invocation(
        &mut self,
        function: SourceAggregateFunction,
        argument: &HirAggregateArgument<TypeDescriptor, TypeDescriptor>,
        context: &ScalarContext,
    ) -> (AggregateFunction, Option<Expression>) {
        let argument = match argument {
            HirAggregateArgument::Star(_) => None,
            HirAggregateArgument::Expression(expression) => {
                Some(self.lower_scalar(expression, context))
            }
        };
        let function = match (function, argument.is_none()) {
            (SourceAggregateFunction::Count, true) => AggregateFunction::CountAll,
            (SourceAggregateFunction::Count, false) => AggregateFunction::Count,
            (SourceAggregateFunction::Sum, _) => AggregateFunction::Sum,
            (SourceAggregateFunction::Min, _) => AggregateFunction::Min,
            (SourceAggregateFunction::Max, _) => AggregateFunction::Max,
            (SourceAggregateFunction::BoolAnd, _) => AggregateFunction::BoolAnd,
            (SourceAggregateFunction::BoolOr, _) => AggregateFunction::BoolOr,
        };
        (function, argument)
    }

    fn lower_order_and_bound(
        &mut self,
        mut input: Plan,
        public_schema: &[Field],
        source_items: &[hir::OrderItem<TypeDescriptor, TypeDescriptor>],
        expressions: Vec<Expression>,
        row_bound: Option<&hir::RowBound>,
    ) -> Plan {
        let public_ids = public_schema
            .iter()
            .map(|field| field.id.clone())
            .collect::<HashSet<_>>();
        let input_map = input
            .schema
            .iter()
            .map(|field| (field.id.clone(), field.clone()))
            .collect::<HashMap<_, _>>();
        let mut entries = public_schema
            .iter()
            .map(|field| ProjectEntry::Keep(field.id.clone()))
            .collect::<Vec<_>>();
        let mut order_schema = public_schema.to_vec();
        let mut keys = Vec::with_capacity(expressions.len());
        let mut hidden = false;

        for (source, expression) in source_items.iter().zip(expressions) {
            let (key, descriptor) = match &expression.kind {
                ExpressionKind::Field(field) if public_ids.contains(field) => {
                    (field.clone(), expression.descriptor)
                }
                _ => {
                    hidden = true;
                    let output = self.temporary_field();
                    let descriptor = expression.descriptor;
                    entries.push(ProjectEntry::Compute {
                        output: output.clone(),
                        expression,
                    });
                    order_schema.push(Field::new(output.clone(), "", descriptor));
                    (output, descriptor)
                }
            };
            keys.push(OrderingItem {
                expression: Expression::new(ExpressionKind::Field(key), descriptor),
                direction: lower_direction(source.direction),
                null_placement: lower_null_placement(source.null_placement),
            });
        }

        let needs_key_project = hidden || input.schema != public_schema;
        if needs_key_project {
            // Every public field must be present in the ordering input.
            debug_assert!(
                public_schema
                    .iter()
                    .all(|field| input_map.contains_key(&field.id))
            );
            let node = self.push_node(
                NodeKind::Project {
                    input: input.node,
                    entries,
                },
                order_schema.clone(),
                input.collection,
            );
            input = Plan {
                node,
                schema: order_schema.clone(),
                collection: input.collection,
                environment: input.environment,
            };
        }

        let node = self.push_node(
            NodeKind::Order {
                input: input.node,
                items: keys,
            },
            input.schema.clone(),
            CollectionKind::Ordered,
        );
        input.node = node;
        input.collection = CollectionKind::Ordered;

        if hidden {
            let node = self.push_node(
                NodeKind::Project {
                    input: input.node,
                    entries: public_schema
                        .iter()
                        .map(|field| ProjectEntry::Keep(field.id.clone()))
                        .collect(),
                },
                public_schema.to_vec(),
                CollectionKind::Ordered,
            );
            input = Plan {
                node,
                schema: public_schema.to_vec(),
                collection: CollectionKind::Ordered,
                environment: input.environment,
            };
        }

        if let Some(bound) = row_bound {
            let offset = bound
                .offset
                .as_ref()
                .map(|value| value.spelling.parse::<i64>().expect("typed row bound"))
                .unwrap_or(0);
            let limit = bound
                .limit
                .as_ref()
                .map(|value| value.spelling.parse::<i64>().expect("typed row bound"));
            let node = self.push_node(
                NodeKind::Slice {
                    input: input.node,
                    offset,
                    limit,
                },
                public_schema.to_vec(),
                CollectionKind::Ordered,
            );
            input.node = node;
        }
        input.schema = public_schema.to_vec();
        input
    }

    fn lower_ordering_item(
        &mut self,
        item: &hir::OrderItem<TypeDescriptor, TypeDescriptor>,
        context: &ScalarContext,
    ) -> OrderingItem {
        OrderingItem {
            expression: self.lower_scalar(&item.expression, context),
            direction: lower_direction(item.direction),
            null_placement: lower_null_placement(item.null_placement),
        }
    }

    fn lower_scalar(
        &mut self,
        expression: &TypedExpression,
        context: &ScalarContext,
    ) -> Expression {
        if let Some(field) = context.windows.get(&invocation_key(expression)) {
            return Expression::new(ExpressionKind::Field(field.clone()), expression.annotation);
        }
        if let Some(field) = context.aggregates.get(&invocation_key(expression)) {
            return Expression::new(ExpressionKind::Field(field.clone()), expression.annotation);
        }
        if let Some((_, field)) = context
            .grouping_keys
            .iter()
            .find(|(group, _)| hir::structurally_equal(group, expression))
        {
            return Expression::new(ExpressionKind::Field(field.clone()), expression.annotation);
        }

        let descriptor = expression.annotation;
        let kind = match &expression.kind {
            H::Literal(value) => ExpressionKind::Literal(match value {
                hir::LiteralValue::Integer(value) => crate::shape_ir::LiteralValue::Int64(
                    value.parse::<i64>().expect("typed integer literal"),
                ),
                hir::LiteralValue::Text(value) => {
                    crate::shape_ir::LiteralValue::Text(value.clone())
                }
                hir::LiteralValue::Boolean(value) => crate::shape_ir::LiteralValue::Boolean(*value),
                hir::LiteralValue::Null => crate::shape_ir::LiteralValue::Null,
            }),
            H::Field(field) => ExpressionKind::Field(
                context
                    .fields
                    .get(field)
                    .cloned()
                    .expect("typed field is live at its lowering stage"),
            ),
            H::Parenthesized(inner) => return self.lower_scalar(inner, context),
            H::Unary {
                operator: UnaryOperator::Negative,
                expression: inner,
            } if matches!(
                &inner.kind,
                H::Literal(hir::LiteralValue::Integer(value)) if value == "9223372036854775808"
            ) =>
            {
                ExpressionKind::Literal(crate::shape_ir::LiteralValue::Int64(i64::MIN))
            }
            H::Unary {
                operator,
                expression: inner,
            } => ExpressionKind::Unary {
                operation: match operator {
                    UnaryOperator::Positive => UnaryOperation::Positive,
                    UnaryOperator::Negative => UnaryOperation::Negative,
                    UnaryOperator::Not => UnaryOperation::Not,
                },
                operand: Box::new(self.lower_scalar(inner, context)),
            },
            H::Binary {
                left,
                operator,
                right,
            } => ExpressionKind::Binary {
                operation: lower_binary(*operator),
                left: Box::new(self.lower_scalar(left, context)),
                right: Box::new(self.lower_scalar(right, context)),
            },
            H::IsNull {
                expression: inner,
                negated,
            } => ExpressionKind::IsNull {
                operand: Box::new(self.lower_scalar(inner, context)),
                negated: *negated,
            },
            H::InList {
                expression: value,
                negated,
                values,
            } => {
                let positive = Expression::new(
                    ExpressionKind::InList {
                        value: Box::new(self.lower_scalar(value, context)),
                        candidates: values
                            .iter()
                            .map(|value| self.lower_scalar(value, context))
                            .collect(),
                    },
                    descriptor,
                );
                if *negated {
                    return Expression::new(
                        ExpressionKind::Unary {
                            operation: UnaryOperation::Not,
                            operand: Box::new(positive),
                        },
                        descriptor,
                    );
                }
                return positive;
            }
            H::InQuery {
                expression: value,
                negated,
                query,
            } => {
                let query = self.lower_query_expression(query.as_ref().clone());
                let query = self.consume_as_bag(query);
                let positive = Expression::new(
                    ExpressionKind::InQuery {
                        value: Box::new(self.lower_scalar(value, context)),
                        query: query.node,
                        field: query.schema[0].id.clone(),
                    },
                    descriptor,
                );
                if *negated {
                    return Expression::new(
                        ExpressionKind::Unary {
                            operation: UnaryOperation::Not,
                            operand: Box::new(positive),
                        },
                        descriptor,
                    );
                }
                return positive;
            }
            H::Case {
                branches,
                else_expression,
            } => ExpressionKind::Case {
                arms: branches
                    .iter()
                    .map(|branch| CaseArm {
                        when: self.lower_scalar(&branch.condition, context),
                        then: self.lower_scalar(&branch.result, context),
                    })
                    .collect(),
                fallback: Box::new(
                    else_expression
                        .as_deref()
                        .map(|fallback| self.lower_scalar(fallback, context))
                        .unwrap_or_else(|| {
                            Expression::new(
                                ExpressionKind::Literal(crate::shape_ir::LiteralValue::Null),
                                TypeDescriptor::nullable(descriptor.scalar),
                            )
                        }),
                ),
            },
            H::Cast {
                expression: inner,
                scalar_type,
            } => ExpressionKind::Cast {
                operand: Box::new(self.lower_scalar(inner, context)),
                target: *scalar_type,
            },
            H::Exists { query } => {
                let query = self.lower_query_expression(query.as_ref().clone());
                let query = self.consume_as_bag(query);
                ExpressionKind::Exists { query: query.node }
            }
            H::Aggregate { .. } | H::Ranking { .. } => {
                panic!("cross-row invocation was not extracted before scalar lowering")
            }
        };
        Expression::new(kind, descriptor)
    }

    fn consume_as_bag(&mut self, plan: Plan) -> Plan {
        if plan.collection == CollectionKind::Bag {
            return plan;
        }
        let node = self.push_node(
            NodeKind::ForgetOrder { input: plan.node },
            plan.schema.clone(),
            CollectionKind::Bag,
        );
        Plan {
            node,
            collection: CollectionKind::Bag,
            ..plan
        }
    }

    fn push_node(
        &mut self,
        kind: NodeKind,
        output_schema: Vec<Field>,
        collection: CollectionKind,
    ) -> NodeId {
        let id = NodeId::new(format!("n{}", self.next_node));
        self.next_node = self.next_node.checked_add(1).expect("node identity space");
        self.nodes.push(Node {
            id: id.clone(),
            kind,
            output_schema,
            collection,
        });
        id
    }

    fn temporary_field(&mut self) -> FieldId {
        let id = FieldId::new(format!("t{}", self.next_temporary));
        self.next_temporary = self
            .next_temporary
            .checked_add(1)
            .expect("temporary field identity space");
        id
    }
}

fn lower_schema(fields: &[TypedField]) -> Vec<Field> {
    fields
        .iter()
        .map(|field| {
            Field::new(
                lower_field_id(field.id),
                field.name.as_str(),
                field.annotation,
            )
        })
        .collect()
}

fn lower_field_id(field: HirFieldId) -> FieldId {
    FieldId::new(format!("f{}", field.index()))
}

fn environment_from_hir_schema(fields: &[TypedField]) -> FieldEnvironment {
    fields
        .iter()
        .map(|field| (field.id, lower_field_id(field.id)))
        .collect()
}

fn nullable_schema(schema: &[Field]) -> Vec<Field> {
    schema
        .iter()
        .cloned()
        .map(|field| Field {
            descriptor: field.descriptor.with_nullable(true),
            ..field
        })
        .collect()
}

fn invocation_key(expression: &TypedExpression) -> InvocationKey {
    (expression.span.start, expression.span.end)
}

fn collect_grouping_aggregates<'a>(
    expression: &'a TypedExpression,
    output: &mut Vec<&'a TypedExpression>,
) {
    if matches!(expression.kind, H::Aggregate { window: None, .. }) {
        output.push(expression);
        return;
    }
    walk_scalar_children(expression, |child| {
        collect_grouping_aggregates(child, output)
    });
}

fn collect_windows<'a>(expression: &'a TypedExpression, output: &mut Vec<&'a TypedExpression>) {
    if matches!(
        expression.kind,
        H::Aggregate {
            window: Some(_),
            ..
        } | H::Ranking { .. }
    ) {
        output.push(expression);
        return;
    }
    walk_scalar_children(expression, |child| collect_windows(child, output));
}

fn walk_scalar_children<'a>(
    expression: &'a TypedExpression,
    mut visit: impl FnMut(&'a TypedExpression),
) {
    match &expression.kind {
        H::Parenthesized(inner)
        | H::Unary {
            expression: inner, ..
        }
        | H::IsNull {
            expression: inner, ..
        }
        | H::Cast {
            expression: inner, ..
        } => visit(inner),
        H::Binary { left, right, .. } => {
            visit(left);
            visit(right);
        }
        H::InList {
            expression, values, ..
        } => {
            visit(expression);
            for value in values {
                visit(value);
            }
        }
        H::InQuery { expression, .. } => visit(expression),
        H::Case {
            branches,
            else_expression,
        } => {
            for branch in branches {
                visit(&branch.condition);
                visit(&branch.result);
            }
            if let Some(else_expression) = else_expression {
                visit(else_expression);
            }
        }
        H::Aggregate {
            argument, window, ..
        } => {
            if let HirAggregateArgument::Expression(argument) = argument {
                visit(argument);
            }
            if let Some(window) = window {
                for expression in &window.partition_by {
                    visit(expression);
                }
            }
        }
        H::Ranking {
            partition_by,
            order_by,
            ..
        } => {
            for expression in partition_by {
                visit(expression);
            }
            for item in order_by {
                visit(&item.expression);
            }
        }
        H::Literal(_) | H::Field(_) | H::Exists { .. } => {}
    }
}

fn lower_binary(operator: BinaryOperator) -> BinaryOperation {
    match operator {
        BinaryOperator::Add => BinaryOperation::Add,
        BinaryOperator::Subtract => BinaryOperation::Subtract,
        BinaryOperator::Multiply => BinaryOperation::Multiply,
        BinaryOperator::Divide => BinaryOperation::Divide,
        BinaryOperator::Remainder => BinaryOperation::Remainder,
        BinaryOperator::Concatenate => BinaryOperation::Concatenate,
        BinaryOperator::Equal => BinaryOperation::Equal,
        BinaryOperator::NotEqual => BinaryOperation::NotEqual,
        BinaryOperator::Less => BinaryOperation::Less,
        BinaryOperator::LessEqual => BinaryOperation::LessOrEqual,
        BinaryOperator::Greater => BinaryOperation::Greater,
        BinaryOperator::GreaterEqual => BinaryOperation::GreaterOrEqual,
        BinaryOperator::And => BinaryOperation::And,
        BinaryOperator::Or => BinaryOperation::Or,
    }
}

fn lower_ranking_function(function: SourceRankingFunction) -> RankingFunction {
    match function {
        SourceRankingFunction::RowNumber => RankingFunction::RowNumber,
        SourceRankingFunction::Rank => RankingFunction::Rank,
        SourceRankingFunction::DenseRank => RankingFunction::DenseRank,
    }
}

fn lower_direction(direction: Option<OrderDirection>) -> Direction {
    match direction {
        None | Some(OrderDirection::Ascending) => Direction::Ascending,
        Some(OrderDirection::Descending) => Direction::Descending,
    }
}

fn lower_null_placement(placement: Option<NullPlacement>) -> IrNullPlacement {
    match placement {
        None => IrNullPlacement::NotApplicable,
        Some(NullPlacement::First) => IrNullPlacement::First,
        Some(NullPlacement::Last) => IrNullPlacement::Last,
    }
}

fn referenced_fields(expression: &Expression) -> HashSet<FieldId> {
    let mut fields = HashSet::new();
    collect_ir_fields(expression, &mut fields);
    fields
}

fn collect_ir_fields(expression: &Expression, output: &mut HashSet<FieldId>) {
    match &expression.kind {
        ExpressionKind::Field(field) => {
            output.insert(field.clone());
        }
        ExpressionKind::Unary { operand, .. }
        | ExpressionKind::IsNull { operand, .. }
        | ExpressionKind::Cast { operand, .. } => collect_ir_fields(operand, output),
        ExpressionKind::Binary { left, right, .. } => {
            collect_ir_fields(left, output);
            collect_ir_fields(right, output);
        }
        ExpressionKind::Case { arms, fallback } => {
            for arm in arms {
                collect_ir_fields(&arm.when, output);
                collect_ir_fields(&arm.then, output);
            }
            collect_ir_fields(fallback, output);
        }
        ExpressionKind::InList { value, candidates } => {
            collect_ir_fields(value, output);
            for candidate in candidates {
                collect_ir_fields(candidate, output);
            }
        }
        ExpressionKind::InQuery { value, .. } => collect_ir_fields(value, output),
        ExpressionKind::Literal(_) | ExpressionKind::Exists { .. } => {}
    }
}
