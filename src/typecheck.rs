//! Static typing, nullability derivation, and semantic placement checks.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ast::{
    AggregateFunction, BinaryOperator, JoinKind, NullPlacement, RankingFunction, UnaryOperator,
};
use crate::hir::{
    self, AggregateArgument, AggregateWindow, BoundExpression, BoundFieldAnnotation, BoundProgram,
    BoundQueryExpression, CommonTableExpression, Expression, ExpressionKind, Field, JoinedTable,
    LiteralValue, OrderItem, QueryBody, QueryExpression, RelationOccurrence, RowBound, SelectField,
    SelectQuery, TablePrimary, TypedExpression, TypedField, TypedProgram, WhenClause,
    structurally_equal,
};
use crate::{CteId, FieldId, ScalarType, Span, TypeDescriptor};

/// A violation of ShapeSQL static typing rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeError {
    pub kind: TypeErrorKind,
    pub span: Span,
}

/// Typing failure categories. Exact diagnostic text is not normative.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeErrorKind {
    UnconstrainedNull,
    IntegerOutOfRange(String),
    TypeMismatch {
        expected: ScalarType,
        actual: ScalarType,
    },
    InvalidPredicate(ScalarType),
    UnsupportedCast {
        source: ScalarType,
        target: ScalarType,
    },
    InQueryArity(usize),
    SetArity {
        left: usize,
        right: usize,
    },
    InvalidAggregateArgument,
    InvalidCrossRowPlacement,
    HavingWithoutAggregate,
    UngroupedColumn,
    NullableOrderingWithoutPlacement,
    IncompleteRowNumberOrdering,
    IncompleteRowBoundOrdering,
    MissingFieldDescriptor(FieldId),
}

impl fmt::Display for TypeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use TypeErrorKind as K;
        match &self.kind {
            K::UnconstrainedNull => formatter.write_str("NULL has no scalar type context"),
            K::IntegerOutOfRange(value) => write!(formatter, "integer `{value}` is out of range"),
            K::TypeMismatch { expected, actual } => {
                write!(formatter, "expected {expected:?}, found {actual:?}")
            }
            K::InvalidPredicate(actual) => {
                write!(formatter, "predicate has type {actual:?}, not BOOLEAN")
            }
            K::UnsupportedCast { source, target } => {
                write!(formatter, "cannot cast {source:?} to {target:?}")
            }
            K::InQueryArity(arity) => {
                write!(
                    formatter,
                    "IN query has {arity} result fields, expected one"
                )
            }
            K::SetArity { left, right } => {
                write!(formatter, "set operands have arities {left} and {right}")
            }
            K::InvalidAggregateArgument => formatter.write_str("invalid aggregate argument"),
            K::InvalidCrossRowPlacement => {
                formatter.write_str("aggregate or ranking expression is not permitted here")
            }
            K::HavingWithoutAggregate => formatter.write_str("HAVING requires an aggregate query"),
            K::UngroupedColumn => formatter.write_str("expression is not group-valid"),
            K::NullableOrderingWithoutPlacement => {
                formatter.write_str("nullable ordering expression requires NULLS FIRST or LAST")
            }
            K::IncompleteRowNumberOrdering => {
                formatter.write_str("ROW_NUMBER ordering does not cover a complete row value")
            }
            K::IncompleteRowBoundOrdering => {
                formatter.write_str("row-bound ordering does not reference every result field")
            }
            K::MissingFieldDescriptor(field) => {
                write!(
                    formatter,
                    "field {:?} has no type descriptor",
                    field.index()
                )
            }
        }
    }
}

impl std::error::Error for TypeError {}

/// Type checks a wholly bound program and constructs a wholly typed program.
pub fn type_check(program: BoundProgram) -> Result<TypedProgram, TypeError> {
    TypeChecker::new().type_program(program)
}

struct TypeChecker {
    ctes: HashMap<CteId, CteEntry>,
}

#[derive(Clone)]
struct CteEntry {
    declaration: CommonTableExpression<(), BoundFieldAnnotation>,
    state: CteTypeState,
}

#[derive(Clone)]
enum CteTypeState {
    Unvisited,
    Typing,
    Typed(Box<QueryExpression<TypeDescriptor, TypeDescriptor>>),
}

type BoundOccurrence = RelationOccurrence<BoundFieldAnnotation>;
type TypedOccurrence = RelationOccurrence<TypeDescriptor>;
type TypedJoinedTable = JoinedTable<TypeDescriptor, TypeDescriptor>;
type TypedFrom = (Vec<TypedJoinedTable>, Vec<TypedField>);
type ScalarExpectations = Vec<Option<ScalarType>>;
type FieldEnvironment = HashMap<FieldId, TypeDescriptor>;

#[derive(Clone, Copy)]
struct Placement {
    grouping: bool,
    window: bool,
}

impl Placement {
    const NONE: Self = Self {
        grouping: false,
        window: false,
    };
    const SELECT: Self = Self {
        grouping: true,
        window: true,
    };
    const HAVING: Self = Self {
        grouping: true,
        window: false,
    };
}

impl TypeChecker {
    fn new() -> Self {
        Self {
            ctes: HashMap::new(),
        }
    }

    fn type_program(&mut self, program: BoundProgram) -> Result<TypedProgram, TypeError> {
        Ok(hir::Program {
            query: self.type_query_expression(program.query, None)?,
            span: program.span,
        })
    }

    fn register_ctes(&mut self, declarations: &[CommonTableExpression<(), BoundFieldAnnotation>]) {
        for declaration in declarations {
            self.ctes.entry(declaration.id).or_insert_with(|| CteEntry {
                declaration: declaration.clone(),
                state: CteTypeState::Unvisited,
            });
        }
    }

    fn type_cte(&mut self, id: CteId) -> Result<(), TypeError> {
        let (state, query, span) = {
            let entry = self.ctes.get(&id).expect("binding produced known CTE id");
            (
                entry.state.clone(),
                entry.declaration.query.as_ref().clone(),
                entry.declaration.span,
            )
        };
        match state {
            CteTypeState::Typed(_) => return Ok(()),
            CteTypeState::Typing => {
                // Binding has already rejected every dependency cycle.
                return Err(TypeError {
                    kind: TypeErrorKind::MissingFieldDescriptor(FieldId::from_index(id.index())),
                    span,
                });
            }
            CteTypeState::Unvisited => {}
        }

        self.ctes.get_mut(&id).expect("known CTE").state = CteTypeState::Typing;
        let query = self.type_query_expression(query, None)?;
        self.ctes.get_mut(&id).expect("known CTE").state = CteTypeState::Typed(Box::new(query));
        Ok(())
    }

    fn typed_cte_query(
        &mut self,
        id: CteId,
    ) -> Result<QueryExpression<TypeDescriptor, TypeDescriptor>, TypeError> {
        self.type_cte(id)?;
        let CteTypeState::Typed(query) = &self.ctes.get(&id).expect("known CTE").state else {
            unreachable!("CTE was just typed")
        };
        Ok(query.as_ref().clone())
    }

    fn type_query_expression(
        &mut self,
        query: BoundQueryExpression,
        expected: Option<&[Option<ScalarType>]>,
    ) -> Result<QueryExpression<TypeDescriptor, TypeDescriptor>, TypeError> {
        self.register_ctes(&query.common_table_expressions);

        let body = self.type_query_body(query.body, expected)?;
        let result_fields = body.result_fields().to_vec();

        let direct_select = match &body {
            QueryBody::Select(select) => Some(select.as_ref()),
            QueryBody::Parenthesized { .. } | QueryBody::SetOperation { .. } => None,
        };
        let allow_order_window = direct_select.is_some_and(|select| !select.distinct);
        let order_placement = Placement {
            grouping: false,
            window: allow_order_window,
        };

        let mut order_environment = environment(&result_fields);
        if let Some(select) = direct_select.filter(|select| !select.distinct) {
            extend_environment(&mut order_environment, &select.source_fields);
        }

        if let Some(select) = direct_select.filter(|select| select.is_aggregate) {
            let allowed_results = result_fields
                .iter()
                .map(|field| field.id)
                .collect::<HashSet<_>>();
            for item in &query.order_by {
                if !group_valid(&item.expression, &select.group_by, &allowed_results) {
                    return Err(TypeError {
                        kind: TypeErrorKind::UngroupedColumn,
                        span: item.expression.span,
                    });
                }
            }
        }

        let mut order_by = Vec::with_capacity(query.order_by.len());
        for item in query.order_by {
            validate_placement(&item.expression, order_placement)?;
            let expression = self.type_expression(item.expression, &order_environment, None)?;
            validate_order_nullability(&expression, item.null_placement)?;
            order_by.push(OrderItem {
                expression,
                direction: item.direction,
                null_placement: item.null_placement,
                span: item.span,
            });
        }

        if let Some(select) = direct_select {
            for item in &order_by {
                validate_row_number_expressions(&item.expression, select)?;
            }
        }

        let row_bound = query
            .row_bound
            .map(|bound| self.type_row_bound(bound, &order_by, &result_fields))
            .transpose()?;

        let mut common_table_expressions = Vec::with_capacity(query.common_table_expressions.len());
        for declaration in query.common_table_expressions {
            let typed_query = self.typed_cte_query(declaration.id)?;
            common_table_expressions.push(CommonTableExpression {
                id: declaration.id,
                name: declaration.name,
                query: Box::new(typed_query),
                span: declaration.span,
            });
        }

        Ok(QueryExpression {
            common_table_expressions,
            body,
            order_by,
            row_bound,
            result_fields,
            span: query.span,
        })
    }

    fn type_row_bound(
        &self,
        bound: RowBound,
        order_by: &[OrderItem<TypeDescriptor, TypeDescriptor>],
        result_fields: &[TypedField],
    ) -> Result<RowBound, TypeError> {
        for value in bound.limit.iter().chain(bound.offset.iter()) {
            if value.spelling.parse::<i64>().is_err() {
                return Err(TypeError {
                    kind: TypeErrorKind::IntegerOutOfRange(value.spelling.clone()),
                    span: value.span,
                });
            }
        }

        let directly_ordered = order_by
            .iter()
            .filter_map(|item| direct_field(&item.expression))
            .collect::<HashSet<_>>();
        if result_fields
            .iter()
            .any(|field| !directly_ordered.contains(&field.id))
        {
            return Err(TypeError {
                kind: TypeErrorKind::IncompleteRowBoundOrdering,
                span: bound.span,
            });
        }
        Ok(bound)
    }

    fn type_query_body(
        &mut self,
        body: QueryBody<(), BoundFieldAnnotation>,
        expected: Option<&[Option<ScalarType>]>,
    ) -> Result<QueryBody<TypeDescriptor, TypeDescriptor>, TypeError> {
        match body {
            QueryBody::Select(select) => Ok(QueryBody::Select(Box::new(
                self.type_select_query(*select, expected)?,
            ))),
            QueryBody::Parenthesized { query, span } => Ok(QueryBody::Parenthesized {
                query: Box::new(self.type_query_expression(*query, expected)?),
                span,
            }),
            QueryBody::SetOperation {
                left,
                operator,
                all,
                right,
                result_fields,
                span,
            } => {
                let left_arity = left.result_fields().len();
                let right_arity = right.result_fields().len();
                if left_arity != right_arity {
                    return Err(TypeError {
                        kind: TypeErrorKind::SetArity {
                            left: left_arity,
                            right: right_arity,
                        },
                        span,
                    });
                }

                let left_hints = self.body_hints(&left)?;
                let right_hints = self.body_hints(&right)?;
                let mut common = Vec::with_capacity(left_arity);
                for index in 0..left_arity {
                    let external = expected
                        .and_then(|expected| expected.get(index))
                        .copied()
                        .flatten();
                    common.push(common_hint(
                        external,
                        left_hints[index],
                        right_hints[index],
                        span,
                    )?);
                }

                let left = self.type_query_body(*left, Some(&common))?;
                let right = self.type_query_body(*right, Some(&common))?;
                let mut typed_results = Vec::with_capacity(result_fields.len());
                for ((output, left), right) in result_fields
                    .into_iter()
                    .zip(left.result_fields())
                    .zip(right.result_fields())
                {
                    ensure_scalar(left.annotation.scalar, right.annotation.scalar, span)?;
                    typed_results.push(Field {
                        id: output.id,
                        name: output.name,
                        annotation: TypeDescriptor::new(
                            left.annotation.scalar,
                            left.annotation.nullable || right.annotation.nullable,
                        ),
                    });
                }

                Ok(QueryBody::SetOperation {
                    left: Box::new(left),
                    operator,
                    all,
                    right: Box::new(right),
                    result_fields: typed_results,
                    span,
                })
            }
        }
    }

    fn body_hints(
        &mut self,
        body: &QueryBody<(), BoundFieldAnnotation>,
    ) -> Result<ScalarExpectations, TypeError> {
        match body {
            QueryBody::Select(select) => {
                let source_fields = self.source_schema_hint(&select.from)?;
                let environment = environment(&source_fields);
                select
                    .select_list
                    .iter()
                    .map(|item| scalar_hint(&item.expression, &environment))
                    .collect()
            }
            QueryBody::Parenthesized { query, .. } => self.query_hints(query),
            QueryBody::SetOperation {
                left, right, span, ..
            } => {
                let left = self.body_hints(left)?;
                let right = self.body_hints(right)?;
                if left.len() != right.len() {
                    return Ok(left);
                }
                left.into_iter()
                    .zip(right)
                    .map(|(left, right)| common_hint(None, left, right, *span))
                    .collect()
            }
        }
    }

    fn query_hints(
        &mut self,
        query: &BoundQueryExpression,
    ) -> Result<ScalarExpectations, TypeError> {
        self.register_ctes(&query.common_table_expressions);
        self.body_hints(&query.body)
    }

    fn type_select_query(
        &mut self,
        select: SelectQuery<(), BoundFieldAnnotation>,
        expected: Option<&[Option<ScalarType>]>,
    ) -> Result<SelectQuery<TypeDescriptor, TypeDescriptor>, TypeError> {
        if let Some(having) = &select.having
            && !select.is_aggregate
        {
            return Err(TypeError {
                kind: TypeErrorKind::HavingWithoutAggregate,
                span: having.span,
            });
        }

        validate_select_placements(&select)?;
        if select.is_aggregate {
            let no_result_fields = HashSet::new();
            for item in &select.select_list {
                if !group_valid(&item.expression, &select.group_by, &no_result_fields) {
                    return Err(TypeError {
                        kind: TypeErrorKind::UngroupedColumn,
                        span: item.expression.span,
                    });
                }
            }
            if let Some(having) = &select.having
                && !group_valid(having, &select.group_by, &no_result_fields)
            {
                return Err(TypeError {
                    kind: TypeErrorKind::UngroupedColumn,
                    span: having.span,
                });
            }
        }

        let (from, source_fields) = self.type_from(select.from)?;
        let source_environment = environment(&source_fields);

        let where_clause = select
            .where_clause
            .map(|expression| self.type_predicate(expression, &source_environment))
            .transpose()?;
        let group_by = select
            .group_by
            .into_iter()
            .map(|expression| self.type_expression(expression, &source_environment, None))
            .collect::<Result<Vec<_>, _>>()?;
        let having = select
            .having
            .map(|expression| self.type_predicate(expression, &source_environment))
            .transpose()?;

        let mut select_list = Vec::with_capacity(select.select_list.len());
        let mut result_fields = Vec::with_capacity(select.result_fields.len());
        for (index, item) in select.select_list.into_iter().enumerate() {
            let expected_scalar = expected
                .and_then(|expected| expected.get(index))
                .copied()
                .flatten();
            let expression =
                self.type_expression(item.expression, &source_environment, expected_scalar)?;
            let output = Field {
                id: item.output.id,
                name: item.output.name,
                annotation: expression.annotation,
            };
            result_fields.push(output.clone());
            select_list.push(SelectField {
                output,
                expression,
                span: item.span,
            });
        }

        let typed = SelectQuery {
            distinct: select.distinct,
            select_list,
            from,
            source_fields,
            where_clause,
            group_by,
            having,
            result_fields,
            is_aggregate: select.is_aggregate,
            span: select.span,
        };
        for item in &typed.select_list {
            validate_row_number_expressions(&item.expression, &typed)?;
        }
        Ok(typed)
    }

    fn type_from(
        &mut self,
        joined_tables: Vec<JoinedTable<(), BoundFieldAnnotation>>,
    ) -> Result<TypedFrom, TypeError> {
        let mut typed_tables = Vec::with_capacity(joined_tables.len());
        let mut complete_schema = Vec::new();

        for joined in joined_tables {
            let (left, mut joined_schema) = self.type_table_primary(joined.left)?;
            let mut typed_joins = Vec::with_capacity(joined.joins.len());
            for join in joined.joins {
                let (right, right_schema) = self.type_table_primary(join.right)?;
                let mut condition_schema = joined_schema.clone();
                condition_schema.extend(right_schema.iter().cloned());
                let condition_environment = environment(&condition_schema);
                let condition = join
                    .condition
                    .map(|condition| {
                        validate_placement(&condition, Placement::NONE)?;
                        self.type_predicate(condition, &condition_environment)
                    })
                    .transpose()?;

                match join.kind {
                    JoinKind::Cross | JoinKind::Inner => {
                        joined_schema.extend(right_schema);
                    }
                    JoinKind::Left => {
                        joined_schema.extend(nullable_schema(right_schema));
                    }
                    JoinKind::Right => {
                        joined_schema = nullable_schema(joined_schema);
                        joined_schema.extend(right_schema);
                    }
                    JoinKind::Full => {
                        joined_schema = nullable_schema(joined_schema);
                        joined_schema.extend(nullable_schema(right_schema));
                    }
                }
                typed_joins.push(hir::Join {
                    kind: join.kind,
                    right,
                    condition,
                    span: join.span,
                });
            }
            complete_schema.extend(joined_schema);
            typed_tables.push(JoinedTable {
                left,
                joins: typed_joins,
                span: joined.span,
            });
        }
        Ok((typed_tables, complete_schema))
    }

    fn type_table_primary(
        &mut self,
        primary: TablePrimary<(), BoundFieldAnnotation>,
    ) -> Result<
        (
            TablePrimary<TypeDescriptor, TypeDescriptor>,
            Vec<TypedField>,
        ),
        TypeError,
    > {
        match primary {
            TablePrimary::Catalog {
                binding,
                occurrence,
                span,
            } => {
                let occurrence = type_catalog_occurrence(occurrence, span)?;
                let schema = occurrence.fields.clone();
                Ok((
                    TablePrimary::Catalog {
                        binding,
                        occurrence,
                        span,
                    },
                    schema,
                ))
            }
            TablePrimary::CommonTableExpression {
                declaration,
                occurrence,
                span,
            } => {
                let query = self.typed_cte_query(declaration)?;
                let occurrence = type_derived_occurrence(occurrence, &query.result_fields, span)?;
                let schema = occurrence.fields.clone();
                Ok((
                    TablePrimary::CommonTableExpression {
                        declaration,
                        occurrence,
                        span,
                    },
                    schema,
                ))
            }
            TablePrimary::Derived {
                query,
                occurrence,
                span,
            } => {
                let query = self.type_query_expression(*query, None)?;
                let occurrence = type_derived_occurrence(occurrence, &query.result_fields, span)?;
                let schema = occurrence.fields.clone();
                Ok((
                    TablePrimary::Derived {
                        query: Box::new(query),
                        occurrence,
                        span,
                    },
                    schema,
                ))
            }
        }
    }

    fn source_schema_hint(
        &mut self,
        joined_tables: &[JoinedTable<(), BoundFieldAnnotation>],
    ) -> Result<Vec<TypedField>, TypeError> {
        let mut complete = Vec::new();
        for joined in joined_tables {
            let mut schema = self.primary_schema_hint(&joined.left)?;
            for join in &joined.joins {
                let right = self.primary_schema_hint(&join.right)?;
                match join.kind {
                    JoinKind::Cross | JoinKind::Inner => schema.extend(right),
                    JoinKind::Left => schema.extend(nullable_schema(right)),
                    JoinKind::Right => {
                        schema = nullable_schema(schema);
                        schema.extend(right);
                    }
                    JoinKind::Full => {
                        schema = nullable_schema(schema);
                        schema.extend(nullable_schema(right));
                    }
                }
            }
            complete.extend(schema);
        }
        Ok(complete)
    }

    fn primary_schema_hint(
        &mut self,
        primary: &TablePrimary<(), BoundFieldAnnotation>,
    ) -> Result<Vec<TypedField>, TypeError> {
        match primary {
            TablePrimary::Catalog {
                occurrence, span, ..
            } => Ok(type_catalog_occurrence(occurrence.clone(), *span)?.fields),
            TablePrimary::CommonTableExpression {
                declaration,
                occurrence,
                span,
            } => {
                let query = self.typed_cte_query(*declaration)?;
                Ok(
                    type_derived_occurrence(occurrence.clone(), &query.result_fields, *span)?
                        .fields,
                )
            }
            TablePrimary::Derived {
                query,
                occurrence,
                span,
            } => {
                let query = self.type_query_expression(query.as_ref().clone(), None)?;
                Ok(
                    type_derived_occurrence(occurrence.clone(), &query.result_fields, *span)?
                        .fields,
                )
            }
        }
    }

    fn type_predicate(
        &mut self,
        expression: BoundExpression,
        environment: &FieldEnvironment,
    ) -> Result<TypedExpression, TypeError> {
        let span = expression.span;
        let expression =
            self.type_expression(expression, environment, Some(ScalarType::Boolean))?;
        if expression.annotation.scalar != ScalarType::Boolean {
            return Err(TypeError {
                kind: TypeErrorKind::InvalidPredicate(expression.annotation.scalar),
                span,
            });
        }
        Ok(expression)
    }

    fn type_expression(
        &mut self,
        expression: BoundExpression,
        environment: &FieldEnvironment,
        expected: Option<ScalarType>,
    ) -> Result<TypedExpression, TypeError> {
        self.type_expression_inner(expression, environment, expected, false)
    }

    fn type_expression_inner(
        &mut self,
        expression: BoundExpression,
        environment: &FieldEnvironment,
        expected: Option<ScalarType>,
        allow_min_magnitude: bool,
    ) -> Result<TypedExpression, TypeError> {
        use ExpressionKind as K;
        let span = expression.span;
        let (kind, descriptor) = match expression.kind {
            K::Literal(value) => {
                let descriptor = match &value {
                    LiteralValue::Integer(value) => {
                        if value.parse::<i64>().is_err()
                            && !(allow_min_magnitude && value == "9223372036854775808")
                        {
                            return Err(TypeError {
                                kind: TypeErrorKind::IntegerOutOfRange(value.clone()),
                                span,
                            });
                        }
                        TypeDescriptor::non_nullable(ScalarType::Int64)
                    }
                    LiteralValue::Text(_) => TypeDescriptor::non_nullable(ScalarType::Text),
                    LiteralValue::Boolean(_) => TypeDescriptor::non_nullable(ScalarType::Boolean),
                    LiteralValue::Null => TypeDescriptor::nullable(expected.ok_or(TypeError {
                        kind: TypeErrorKind::UnconstrainedNull,
                        span,
                    })?),
                };
                (K::Literal(value), descriptor)
            }
            K::Field(field) => {
                let descriptor = environment.get(&field).copied().ok_or(TypeError {
                    kind: TypeErrorKind::MissingFieldDescriptor(field),
                    span,
                })?;
                (K::Field(field), descriptor)
            }
            K::Parenthesized(inner) => {
                let inner = self.type_expression_inner(*inner, environment, expected, false)?;
                let descriptor = inner.annotation;
                (K::Parenthesized(Box::new(inner)), descriptor)
            }
            K::Unary {
                operator,
                expression: inner,
            } => {
                let required = match operator {
                    UnaryOperator::Positive | UnaryOperator::Negative => ScalarType::Int64,
                    UnaryOperator::Not => ScalarType::Boolean,
                };
                let special_min = operator == UnaryOperator::Negative
                    && matches!(
                        &inner.kind,
                        K::Literal(LiteralValue::Integer(value)) if value == "9223372036854775808"
                    );
                let inner =
                    self.type_expression_inner(*inner, environment, Some(required), special_min)?;
                ensure_scalar(required, inner.annotation.scalar, span)?;
                let descriptor = TypeDescriptor::new(required, inner.annotation.nullable);
                (
                    K::Unary {
                        operator,
                        expression: Box::new(inner),
                    },
                    descriptor,
                )
            }
            K::Binary {
                left,
                operator,
                right,
            } => {
                let operand_type = match operator {
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Remainder => ScalarType::Int64,
                    BinaryOperator::Concatenate => ScalarType::Text,
                    BinaryOperator::And | BinaryOperator::Or => ScalarType::Boolean,
                    BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual => {
                        common_expression_scalar(&left, &right, environment, span)?
                    }
                };
                let left = self.type_expression(*left, environment, Some(operand_type))?;
                let right = self.type_expression(*right, environment, Some(operand_type))?;
                ensure_scalar(operand_type, left.annotation.scalar, span)?;
                ensure_scalar(operand_type, right.annotation.scalar, span)?;
                let result_type = match operator {
                    BinaryOperator::Equal
                    | BinaryOperator::NotEqual
                    | BinaryOperator::Less
                    | BinaryOperator::LessEqual
                    | BinaryOperator::Greater
                    | BinaryOperator::GreaterEqual
                    | BinaryOperator::And
                    | BinaryOperator::Or => ScalarType::Boolean,
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Remainder => ScalarType::Int64,
                    BinaryOperator::Concatenate => ScalarType::Text,
                };
                let descriptor = TypeDescriptor::new(
                    result_type,
                    left.annotation.nullable || right.annotation.nullable,
                );
                (
                    K::Binary {
                        left: Box::new(left),
                        operator,
                        right: Box::new(right),
                    },
                    descriptor,
                )
            }
            K::IsNull {
                expression: inner,
                negated,
            } => {
                let hint = scalar_hint(&inner, environment)?;
                let inner = self.type_expression(*inner, environment, hint)?;
                (
                    K::IsNull {
                        expression: Box::new(inner),
                        negated,
                    },
                    TypeDescriptor::non_nullable(ScalarType::Boolean),
                )
            }
            K::InList {
                expression: value,
                negated,
                values,
            } => {
                let mut common = scalar_hint(&value, environment)?;
                for candidate in &values {
                    common = common_hint(common, scalar_hint(candidate, environment)?, None, span)?;
                }
                let common = common.ok_or(TypeError {
                    kind: TypeErrorKind::UnconstrainedNull,
                    span,
                })?;
                let value = self.type_expression(*value, environment, Some(common))?;
                let values = values
                    .into_iter()
                    .map(|candidate| self.type_expression(candidate, environment, Some(common)))
                    .collect::<Result<Vec<_>, _>>()?;
                let nullable = value.annotation.nullable
                    || values.iter().any(|value| value.annotation.nullable);
                (
                    K::InList {
                        expression: Box::new(value),
                        negated,
                        values,
                    },
                    TypeDescriptor::new(ScalarType::Boolean, nullable),
                )
            }
            K::InQuery {
                expression: value,
                negated,
                query,
            } => {
                if query.result_fields.len() != 1 {
                    return Err(TypeError {
                        kind: TypeErrorKind::InQueryArity(query.result_fields.len()),
                        span: query.span,
                    });
                }
                let query_hints = self.query_hints(&query)?;
                let common = common_hint(
                    None,
                    scalar_hint(&value, environment)?,
                    query_hints.first().copied().flatten(),
                    span,
                )?
                .ok_or(TypeError {
                    kind: TypeErrorKind::UnconstrainedNull,
                    span,
                })?;
                let value = self.type_expression(*value, environment, Some(common))?;
                let query = self.type_query_expression(*query, Some(&[Some(common)]))?;
                let query_field = &query.result_fields[0];
                ensure_scalar(common, query_field.annotation.scalar, span)?;
                let nullable = value.annotation.nullable || query_field.annotation.nullable;
                (
                    K::InQuery {
                        expression: Box::new(value),
                        negated,
                        query: Box::new(query),
                    },
                    TypeDescriptor::new(ScalarType::Boolean, nullable),
                )
            }
            K::Case {
                branches,
                else_expression,
            } => {
                let mut result_scalar = expected;
                for branch in &branches {
                    result_scalar = common_hint(
                        result_scalar,
                        scalar_hint(&branch.result, environment)?,
                        None,
                        branch.result.span,
                    )?;
                }
                if let Some(else_expression) = &else_expression {
                    result_scalar = common_hint(
                        result_scalar,
                        scalar_hint(else_expression, environment)?,
                        None,
                        else_expression.span,
                    )?;
                }
                let result_scalar = result_scalar.ok_or(TypeError {
                    kind: TypeErrorKind::UnconstrainedNull,
                    span,
                })?;

                let mut typed_branches = Vec::with_capacity(branches.len());
                for branch in branches {
                    let condition = self.type_predicate(branch.condition, environment)?;
                    let result =
                        self.type_expression(branch.result, environment, Some(result_scalar))?;
                    typed_branches.push(WhenClause {
                        condition,
                        result,
                        span: branch.span,
                    });
                }
                let typed_else = else_expression
                    .map(|expression| {
                        self.type_expression(*expression, environment, Some(result_scalar))
                            .map(Box::new)
                    })
                    .transpose()?;
                let nullable = typed_branches
                    .iter()
                    .any(|branch| branch.result.annotation.nullable)
                    || typed_else
                        .as_deref()
                        .is_none_or(|expression| expression.annotation.nullable);
                (
                    K::Case {
                        branches: typed_branches,
                        else_expression: typed_else,
                    },
                    TypeDescriptor::new(result_scalar, nullable),
                )
            }
            K::Cast {
                expression: inner,
                scalar_type,
            } => {
                let source_hint = scalar_hint(&inner, environment)?;
                let inner =
                    self.type_expression(*inner, environment, source_hint.or(Some(scalar_type)))?;
                if !cast_permitted(inner.annotation.scalar, scalar_type) {
                    return Err(TypeError {
                        kind: TypeErrorKind::UnsupportedCast {
                            source: inner.annotation.scalar,
                            target: scalar_type,
                        },
                        span,
                    });
                }
                let descriptor = TypeDescriptor::new(scalar_type, inner.annotation.nullable);
                (
                    K::Cast {
                        expression: Box::new(inner),
                        scalar_type,
                    },
                    descriptor,
                )
            }
            K::Exists { query } => {
                let query = self.type_query_expression(*query, None)?;
                (
                    K::Exists {
                        query: Box::new(query),
                    },
                    TypeDescriptor::non_nullable(ScalarType::Boolean),
                )
            }
            K::Aggregate {
                function,
                argument,
                window,
            } => {
                let argument_expected = match function {
                    AggregateFunction::Sum => Some(ScalarType::Int64),
                    AggregateFunction::BoolAnd | AggregateFunction::BoolOr => {
                        Some(ScalarType::Boolean)
                    }
                    AggregateFunction::Min | AggregateFunction::Max => expected,
                    AggregateFunction::Count => None,
                };
                let argument = match argument {
                    AggregateArgument::Star(span) => AggregateArgument::Star(span),
                    AggregateArgument::Expression(expression) => {
                        AggregateArgument::Expression(Box::new(self.type_expression(
                            *expression,
                            environment,
                            argument_expected,
                        )?))
                    }
                };
                validate_aggregate_signature(function, &argument, span)?;
                let window = window
                    .map(|window| {
                        Ok(AggregateWindow {
                            partition_by: window
                                .partition_by
                                .into_iter()
                                .map(|expression| {
                                    self.type_expression(expression, environment, None)
                                })
                                .collect::<Result<_, _>>()?,
                            span: window.span,
                        })
                    })
                    .transpose()?;
                let descriptor = aggregate_descriptor(function, &argument);
                (
                    K::Aggregate {
                        function,
                        argument,
                        window,
                    },
                    descriptor,
                )
            }
            K::Ranking {
                function,
                partition_by,
                order_by,
            } => {
                let partition_by = partition_by
                    .into_iter()
                    .map(|expression| self.type_expression(expression, environment, None))
                    .collect::<Result<_, _>>()?;
                let mut typed_order = Vec::with_capacity(order_by.len());
                for item in order_by {
                    let expression = self.type_expression(item.expression, environment, None)?;
                    validate_order_nullability(&expression, item.null_placement)?;
                    typed_order.push(OrderItem {
                        expression,
                        direction: item.direction,
                        null_placement: item.null_placement,
                        span: item.span,
                    });
                }
                (
                    K::Ranking {
                        function,
                        partition_by,
                        order_by: typed_order,
                    },
                    TypeDescriptor::non_nullable(ScalarType::Int64),
                )
            }
        };

        if let Some(expected) = expected {
            ensure_scalar(expected, descriptor.scalar, span)?;
        }
        Ok(Expression::new(kind, descriptor, span))
    }
}

fn type_catalog_occurrence(
    occurrence: BoundOccurrence,
    span: Span,
) -> Result<TypedOccurrence, TypeError> {
    let fields = occurrence
        .fields
        .into_iter()
        .map(|field| {
            Ok(Field {
                id: field.id,
                name: field.name,
                annotation: field.annotation.ok_or(TypeError {
                    kind: TypeErrorKind::MissingFieldDescriptor(field.id),
                    span,
                })?,
            })
        })
        .collect::<Result<_, TypeError>>()?;
    Ok(RelationOccurrence {
        id: occurrence.id,
        qualifier: occurrence.qualifier,
        fields,
    })
}

fn type_derived_occurrence(
    occurrence: BoundOccurrence,
    source_fields: &[TypedField],
    span: Span,
) -> Result<TypedOccurrence, TypeError> {
    if occurrence.fields.len() != source_fields.len() {
        return Err(TypeError {
            kind: TypeErrorKind::SetArity {
                left: occurrence.fields.len(),
                right: source_fields.len(),
            },
            span,
        });
    }
    let fields = occurrence
        .fields
        .into_iter()
        .zip(source_fields)
        .map(|(field, source)| Field {
            id: field.id,
            name: field.name,
            annotation: source.annotation,
        })
        .collect();
    Ok(RelationOccurrence {
        id: occurrence.id,
        qualifier: occurrence.qualifier,
        fields,
    })
}

fn environment(fields: &[TypedField]) -> FieldEnvironment {
    fields
        .iter()
        .map(|field| (field.id, field.annotation))
        .collect()
}

fn extend_environment(environment: &mut FieldEnvironment, fields: &[TypedField]) {
    environment.extend(fields.iter().map(|field| (field.id, field.annotation)));
}

fn nullable_schema(fields: Vec<TypedField>) -> Vec<TypedField> {
    fields
        .into_iter()
        .map(|field| Field {
            annotation: field.annotation.with_nullable(true),
            ..field
        })
        .collect()
}

fn scalar_hint(
    expression: &BoundExpression,
    environment: &FieldEnvironment,
) -> Result<Option<ScalarType>, TypeError> {
    use ExpressionKind as K;
    match &expression.kind {
        K::Literal(LiteralValue::Null) => Ok(None),
        K::Literal(LiteralValue::Integer(_)) => Ok(Some(ScalarType::Int64)),
        K::Literal(LiteralValue::Text(_)) => Ok(Some(ScalarType::Text)),
        K::Literal(LiteralValue::Boolean(_)) => Ok(Some(ScalarType::Boolean)),
        K::Field(field) => environment
            .get(field)
            .map(|descriptor| Some(descriptor.scalar))
            .ok_or(TypeError {
                kind: TypeErrorKind::MissingFieldDescriptor(*field),
                span: expression.span,
            }),
        K::Parenthesized(expression) => scalar_hint(expression, environment),
        K::Unary { operator, .. } => Ok(Some(match operator {
            UnaryOperator::Positive | UnaryOperator::Negative => ScalarType::Int64,
            UnaryOperator::Not => ScalarType::Boolean,
        })),
        K::Binary { operator, .. } => Ok(Some(match operator {
            BinaryOperator::Add
            | BinaryOperator::Subtract
            | BinaryOperator::Multiply
            | BinaryOperator::Divide
            | BinaryOperator::Remainder => ScalarType::Int64,
            BinaryOperator::Concatenate => ScalarType::Text,
            BinaryOperator::Equal
            | BinaryOperator::NotEqual
            | BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
            | BinaryOperator::And
            | BinaryOperator::Or => ScalarType::Boolean,
        })),
        K::IsNull { .. } | K::InList { .. } | K::InQuery { .. } | K::Exists { .. } => {
            Ok(Some(ScalarType::Boolean))
        }
        K::Case {
            branches,
            else_expression,
        } => {
            let mut common = None;
            for branch in branches {
                common = common_hint(
                    common,
                    scalar_hint(&branch.result, environment)?,
                    None,
                    branch.result.span,
                )?;
            }
            if let Some(else_expression) = else_expression {
                common = common_hint(
                    common,
                    scalar_hint(else_expression, environment)?,
                    None,
                    else_expression.span,
                )?;
            }
            Ok(common)
        }
        K::Cast { scalar_type, .. } => Ok(Some(*scalar_type)),
        K::Aggregate {
            function, argument, ..
        } => Ok(match function {
            AggregateFunction::Count | AggregateFunction::Sum => Some(ScalarType::Int64),
            AggregateFunction::BoolAnd | AggregateFunction::BoolOr => Some(ScalarType::Boolean),
            AggregateFunction::Min | AggregateFunction::Max => match argument {
                AggregateArgument::Star(_) => None,
                AggregateArgument::Expression(expression) => scalar_hint(expression, environment)?,
            },
        }),
        K::Ranking { .. } => Ok(Some(ScalarType::Int64)),
    }
}

fn common_expression_scalar(
    left: &BoundExpression,
    right: &BoundExpression,
    environment: &FieldEnvironment,
    span: Span,
) -> Result<ScalarType, TypeError> {
    common_hint(
        None,
        scalar_hint(left, environment)?,
        scalar_hint(right, environment)?,
        span,
    )?
    .ok_or(TypeError {
        kind: TypeErrorKind::UnconstrainedNull,
        span,
    })
}

fn common_hint(
    first: Option<ScalarType>,
    second: Option<ScalarType>,
    third: Option<ScalarType>,
    span: Span,
) -> Result<Option<ScalarType>, TypeError> {
    let mut common = first;
    for candidate in [second, third].into_iter().flatten() {
        if let Some(existing) = common {
            ensure_scalar(existing, candidate, span)?;
        } else {
            common = Some(candidate);
        }
    }
    Ok(common)
}

fn ensure_scalar(expected: ScalarType, actual: ScalarType, span: Span) -> Result<(), TypeError> {
    if expected == actual {
        Ok(())
    } else {
        Err(TypeError {
            kind: TypeErrorKind::TypeMismatch { expected, actual },
            span,
        })
    }
}

fn cast_permitted(source: ScalarType, target: ScalarType) -> bool {
    source == target
        || matches!(
            (source, target),
            (ScalarType::Int64, ScalarType::Text)
                | (ScalarType::Text, ScalarType::Int64)
                | (ScalarType::Boolean, ScalarType::Text)
                | (ScalarType::Text, ScalarType::Boolean)
        )
}

fn validate_aggregate_signature(
    function: AggregateFunction,
    argument: &AggregateArgument<TypeDescriptor, TypeDescriptor>,
    span: Span,
) -> Result<(), TypeError> {
    let descriptor = match argument {
        AggregateArgument::Star(_) => {
            if function == AggregateFunction::Count {
                return Ok(());
            }
            return Err(TypeError {
                kind: TypeErrorKind::InvalidAggregateArgument,
                span,
            });
        }
        AggregateArgument::Expression(expression) => expression.annotation,
    };
    let valid = match function {
        AggregateFunction::Count | AggregateFunction::Min | AggregateFunction::Max => true,
        AggregateFunction::Sum => descriptor.scalar == ScalarType::Int64,
        AggregateFunction::BoolAnd | AggregateFunction::BoolOr => {
            descriptor.scalar == ScalarType::Boolean
        }
    };
    if valid {
        Ok(())
    } else {
        Err(TypeError {
            kind: TypeErrorKind::InvalidAggregateArgument,
            span,
        })
    }
}

fn aggregate_descriptor(
    function: AggregateFunction,
    argument: &AggregateArgument<TypeDescriptor, TypeDescriptor>,
) -> TypeDescriptor {
    match function {
        AggregateFunction::Count => TypeDescriptor::non_nullable(ScalarType::Int64),
        AggregateFunction::Sum => TypeDescriptor::nullable(ScalarType::Int64),
        AggregateFunction::BoolAnd | AggregateFunction::BoolOr => {
            TypeDescriptor::nullable(ScalarType::Boolean)
        }
        AggregateFunction::Min | AggregateFunction::Max => {
            let AggregateArgument::Expression(expression) = argument else {
                unreachable!("signature validation rejects star")
            };
            TypeDescriptor::nullable(expression.annotation.scalar)
        }
    }
}

fn validate_order_nullability(
    expression: &TypedExpression,
    placement: Option<NullPlacement>,
) -> Result<(), TypeError> {
    if expression.annotation.nullable && placement.is_none() {
        Err(TypeError {
            kind: TypeErrorKind::NullableOrderingWithoutPlacement,
            span: expression.span,
        })
    } else {
        Ok(())
    }
}

fn validate_select_placements(
    select: &SelectQuery<(), BoundFieldAnnotation>,
) -> Result<(), TypeError> {
    for joined in &select.from {
        for join in &joined.joins {
            if let Some(condition) = &join.condition {
                validate_placement(condition, Placement::NONE)?;
            }
        }
    }
    if let Some(where_clause) = &select.where_clause {
        validate_placement(where_clause, Placement::NONE)?;
    }
    for group in &select.group_by {
        validate_placement(group, Placement::NONE)?;
    }
    if let Some(having) = &select.having {
        validate_placement(having, Placement::HAVING)?;
    }
    for item in &select.select_list {
        validate_placement(&item.expression, Placement::SELECT)?;
    }
    Ok(())
}

fn validate_placement(expression: &BoundExpression, placement: Placement) -> Result<(), TypeError> {
    use ExpressionKind as K;
    match &expression.kind {
        K::Aggregate {
            argument, window, ..
        } => {
            let permitted = if window.is_some() {
                placement.window
            } else {
                placement.grouping
            };
            if !permitted {
                return Err(TypeError {
                    kind: TypeErrorKind::InvalidCrossRowPlacement,
                    span: expression.span,
                });
            }
            if let AggregateArgument::Expression(argument) = argument {
                validate_placement(argument, Placement::NONE)?;
            }
            if let Some(window) = window {
                for partition in &window.partition_by {
                    validate_placement(partition, Placement::NONE)?;
                }
            }
        }
        K::Ranking {
            partition_by,
            order_by,
            ..
        } => {
            if !placement.window {
                return Err(TypeError {
                    kind: TypeErrorKind::InvalidCrossRowPlacement,
                    span: expression.span,
                });
            }
            for partition in partition_by {
                validate_placement(partition, Placement::NONE)?;
            }
            for item in order_by {
                validate_placement(&item.expression, Placement::NONE)?;
            }
        }
        K::Parenthesized(inner)
        | K::Unary {
            expression: inner, ..
        }
        | K::IsNull {
            expression: inner, ..
        }
        | K::Cast {
            expression: inner, ..
        } => validate_placement(inner, placement)?,
        K::Binary { left, right, .. } => {
            validate_placement(left, placement)?;
            validate_placement(right, placement)?;
        }
        K::InList {
            expression, values, ..
        } => {
            validate_placement(expression, placement)?;
            for value in values {
                validate_placement(value, placement)?;
            }
        }
        K::InQuery { expression, .. } => validate_placement(expression, placement)?,
        K::Case {
            branches,
            else_expression,
        } => {
            for branch in branches {
                validate_placement(&branch.condition, placement)?;
                validate_placement(&branch.result, placement)?;
            }
            if let Some(else_expression) = else_expression {
                validate_placement(else_expression, placement)?;
            }
        }
        K::Literal(_) | K::Field(_) | K::Exists { .. } => {}
    }
    Ok(())
}

fn group_valid<E1, F1, E2, F2>(
    expression: &Expression<E1, F1>,
    group_by: &[Expression<E2, F2>],
    allowed_results: &HashSet<FieldId>,
) -> bool {
    use ExpressionKind as K;
    if group_by
        .iter()
        .any(|group| structurally_equal(expression, group))
    {
        return true;
    }
    match &expression.kind {
        K::Literal(_) | K::Exists { .. } => true,
        K::Field(field) => allowed_results.contains(field),
        K::Parenthesized(inner)
        | K::Unary {
            expression: inner, ..
        }
        | K::IsNull {
            expression: inner, ..
        }
        | K::Cast {
            expression: inner, ..
        } => group_valid(inner, group_by, allowed_results),
        K::Binary { left, right, .. } => {
            group_valid(left, group_by, allowed_results)
                && group_valid(right, group_by, allowed_results)
        }
        K::InList {
            expression, values, ..
        } => {
            group_valid(expression, group_by, allowed_results)
                && values
                    .iter()
                    .all(|value| group_valid(value, group_by, allowed_results))
        }
        K::InQuery { expression, .. } => group_valid(expression, group_by, allowed_results),
        K::Case {
            branches,
            else_expression,
        } => {
            branches.iter().all(|branch| {
                group_valid(&branch.condition, group_by, allowed_results)
                    && group_valid(&branch.result, group_by, allowed_results)
            }) && else_expression
                .as_deref()
                .is_none_or(|expression| group_valid(expression, group_by, allowed_results))
        }
        K::Aggregate {
            argument, window, ..
        } => {
            if window.is_none() {
                true
            } else {
                matches!(
                    argument,
                    AggregateArgument::Star(_) | AggregateArgument::Expression(_)
                ) && match argument {
                    AggregateArgument::Star(_) => true,
                    AggregateArgument::Expression(expression) => {
                        group_valid(expression, group_by, allowed_results)
                    }
                } && window.as_ref().is_none_or(|window| {
                    window
                        .partition_by
                        .iter()
                        .all(|expression| group_valid(expression, group_by, allowed_results))
                })
            }
        }
        K::Ranking {
            partition_by,
            order_by,
            ..
        } => {
            partition_by
                .iter()
                .all(|expression| group_valid(expression, group_by, allowed_results))
                && order_by
                    .iter()
                    .all(|item| group_valid(&item.expression, group_by, allowed_results))
        }
    }
}

fn validate_row_number_expressions(
    expression: &TypedExpression,
    select: &SelectQuery<TypeDescriptor, TypeDescriptor>,
) -> Result<(), TypeError> {
    use ExpressionKind as K;
    match &expression.kind {
        K::Ranking {
            function: RankingFunction::RowNumber,
            order_by,
            ..
        } => {
            let complete = if !select.is_aggregate {
                let direct = order_by
                    .iter()
                    .filter_map(|item| direct_field(&item.expression))
                    .collect::<HashSet<_>>();
                select
                    .source_fields
                    .iter()
                    .all(|field| direct.contains(&field.id))
            } else if !select.group_by.is_empty() {
                select.group_by.iter().all(|group| {
                    order_by
                        .iter()
                        .any(|item| structurally_equal(group, &item.expression))
                })
            } else {
                true
            };
            if !complete {
                return Err(TypeError {
                    kind: TypeErrorKind::IncompleteRowNumberOrdering,
                    span: expression.span,
                });
            }
        }
        K::Parenthesized(inner)
        | K::Unary {
            expression: inner, ..
        }
        | K::IsNull {
            expression: inner, ..
        }
        | K::Cast {
            expression: inner, ..
        } => validate_row_number_expressions(inner, select)?,
        K::Binary { left, right, .. } => {
            validate_row_number_expressions(left, select)?;
            validate_row_number_expressions(right, select)?;
        }
        K::InList {
            expression, values, ..
        } => {
            validate_row_number_expressions(expression, select)?;
            for value in values {
                validate_row_number_expressions(value, select)?;
            }
        }
        K::InQuery { expression, .. } => validate_row_number_expressions(expression, select)?,
        K::Case {
            branches,
            else_expression,
        } => {
            for branch in branches {
                validate_row_number_expressions(&branch.condition, select)?;
                validate_row_number_expressions(&branch.result, select)?;
            }
            if let Some(else_expression) = else_expression {
                validate_row_number_expressions(else_expression, select)?;
            }
        }
        K::Aggregate {
            argument, window, ..
        } => {
            if let AggregateArgument::Expression(argument) = argument {
                validate_row_number_expressions(argument, select)?;
            }
            if let Some(window) = window {
                for expression in &window.partition_by {
                    validate_row_number_expressions(expression, select)?;
                }
            }
        }
        K::Ranking { .. } | K::Literal(_) | K::Field(_) | K::Exists { .. } => {}
    }
    Ok(())
}

fn direct_field<E, F>(expression: &Expression<E, F>) -> Option<FieldId> {
    match &expression.kind {
        ExpressionKind::Field(field) => Some(*field),
        ExpressionKind::Parenthesized(inner) => direct_field(inner),
        _ => None,
    }
}
