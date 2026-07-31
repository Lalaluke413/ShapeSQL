//! Bound and typed high-level semantic representations.
//!
//! The syntax tree preserves source spelling and unresolved names. The HIR
//! instead contains resolved field and relation identities, expanded select
//! lists, owned literal values, and explicit result schemas. Its generic
//! annotations let type checking consume a wholly untyped expression tree and
//! construct a wholly typed tree without partially initialized descriptors.

use crate::ast::{
    AggregateFunction, BinaryOperator, JoinKind, NullPlacement, OrderDirection, RankingFunction,
    SetOperator, UnaryOperator,
};
use crate::{CteId, FieldId, Name, RelationBinding, RelationOccurrenceId, Span, TypeDescriptor};

/// Field annotation used before type checking.
///
/// Catalog occurrence fields carry their declared descriptor. Fields produced
/// by source queries remain unresolved until the type checker constructs typed
/// HIR.
pub type BoundFieldAnnotation = Option<TypeDescriptor>;

pub type BoundProgram = Program<(), BoundFieldAnnotation>;
pub type TypedProgram = Program<TypeDescriptor, TypeDescriptor>;
pub type BoundQueryExpression = QueryExpression<(), BoundFieldAnnotation>;
pub type TypedQueryExpression = QueryExpression<TypeDescriptor, TypeDescriptor>;
pub type BoundExpression = Expression<(), BoundFieldAnnotation>;
pub type TypedExpression = Expression<TypeDescriptor, TypeDescriptor>;
pub type BoundField = Field<BoundFieldAnnotation>;
pub type TypedField = Field<TypeDescriptor>;

/// One complete semantic program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program<E, F> {
    pub query: QueryExpression<E, F>,
    pub span: Span,
}

/// A resolved schema field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field<F> {
    pub id: FieldId,
    pub name: Name,
    pub annotation: F,
}

impl<F> Field<F> {
    pub fn map_annotation<G>(self, map: impl FnOnce(F) -> G) -> Field<G> {
        Field {
            id: self.id,
            name: self.name,
            annotation: map(self.annotation),
        }
    }
}

/// A query together with its local CTE declarations, ordering, and row bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryExpression<E, F> {
    pub common_table_expressions: Vec<CommonTableExpression<E, F>>,
    pub body: QueryBody<E, F>,
    pub order_by: Vec<OrderItem<E, F>>,
    pub row_bound: Option<RowBound>,
    pub result_fields: Vec<Field<F>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonTableExpression<E, F> {
    pub id: CteId,
    pub name: Name,
    pub query: Box<QueryExpression<E, F>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryBody<E, F> {
    Select(Box<SelectQuery<E, F>>),
    Parenthesized {
        query: Box<QueryExpression<E, F>>,
        span: Span,
    },
    SetOperation {
        left: Box<QueryBody<E, F>>,
        operator: SetOperator,
        all: bool,
        right: Box<QueryBody<E, F>>,
        result_fields: Vec<Field<F>>,
        span: Span,
    },
}

impl<E, F> QueryBody<E, F> {
    pub fn result_fields(&self) -> &[Field<F>] {
        match self {
            Self::Select(select) => &select.result_fields,
            Self::Parenthesized { query, .. } => &query.result_fields,
            Self::SetOperation { result_fields, .. } => result_fields,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Self::Select(select) => select.span,
            Self::Parenthesized { span, .. } | Self::SetOperation { span, .. } => *span,
        }
    }
}

/// One bound query block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectQuery<E, F> {
    pub distinct: bool,
    pub select_list: Vec<SelectField<E, F>>,
    pub from: Vec<JoinedTable<E, F>>,
    pub source_fields: Vec<Field<F>>,
    pub where_clause: Option<Expression<E, F>>,
    pub group_by: Vec<Expression<E, F>>,
    pub having: Option<Expression<E, F>>,
    pub result_fields: Vec<Field<F>>,
    pub is_aggregate: bool,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectField<E, F> {
    pub output: Field<F>,
    pub expression: Expression<E, F>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedTable<E, F> {
    pub left: TablePrimary<E, F>,
    pub joins: Vec<Join<E, F>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TablePrimary<E, F> {
    Catalog {
        binding: RelationBinding,
        occurrence: RelationOccurrence<F>,
        span: Span,
    },
    CommonTableExpression {
        declaration: CteId,
        occurrence: RelationOccurrence<F>,
        span: Span,
    },
    Derived {
        query: Box<QueryExpression<E, F>>,
        occurrence: RelationOccurrence<F>,
        span: Span,
    },
}

impl<E, F> TablePrimary<E, F> {
    pub fn occurrence(&self) -> &RelationOccurrence<F> {
        match self {
            Self::Catalog { occurrence, .. }
            | Self::CommonTableExpression { occurrence, .. }
            | Self::Derived { occurrence, .. } => occurrence,
        }
    }

    pub fn span(&self) -> Span {
        match self {
            Self::Catalog { span, .. }
            | Self::CommonTableExpression { span, .. }
            | Self::Derived { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationOccurrence<F> {
    pub id: RelationOccurrenceId,
    pub qualifier: Name,
    pub fields: Vec<Field<F>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Join<E, F> {
    pub kind: JoinKind,
    pub right: TablePrimary<E, F>,
    pub condition: Option<Expression<E, F>>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderItem<E, F> {
    pub expression: Expression<E, F>,
    pub direction: Option<OrderDirection>,
    pub null_placement: Option<NullPlacement>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowBound {
    pub limit: Option<IntegerValue>,
    pub offset: Option<IntegerValue>,
    pub span: Span,
}

/// An unsigned integer spelling retained until static range checking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegerValue {
    pub spelling: String,
    pub span: Span,
}

/// A scalar expression annotated by its analysis phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expression<E, F> {
    pub kind: ExpressionKind<E, F>,
    pub annotation: E,
    pub span: Span,
}

impl<E, F> Expression<E, F> {
    pub fn new(kind: ExpressionKind<E, F>, annotation: E, span: Span) -> Self {
        Self {
            kind,
            annotation,
            span,
        }
    }
}

impl<F> Expression<TypeDescriptor, F> {
    pub fn descriptor(&self) -> TypeDescriptor {
        self.annotation
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExpressionKind<E, F> {
    Literal(LiteralValue),
    Field(FieldId),
    Parenthesized(Box<Expression<E, F>>),
    Unary {
        operator: UnaryOperator,
        expression: Box<Expression<E, F>>,
    },
    Binary {
        left: Box<Expression<E, F>>,
        operator: BinaryOperator,
        right: Box<Expression<E, F>>,
    },
    IsNull {
        expression: Box<Expression<E, F>>,
        negated: bool,
    },
    InList {
        expression: Box<Expression<E, F>>,
        negated: bool,
        values: Vec<Expression<E, F>>,
    },
    InQuery {
        expression: Box<Expression<E, F>>,
        negated: bool,
        query: Box<QueryExpression<E, F>>,
    },
    Case {
        branches: Vec<WhenClause<E, F>>,
        else_expression: Option<Box<Expression<E, F>>>,
    },
    Cast {
        expression: Box<Expression<E, F>>,
        scalar_type: crate::ScalarType,
    },
    Exists {
        query: Box<QueryExpression<E, F>>,
    },
    Aggregate {
        function: AggregateFunction,
        argument: AggregateArgument<E, F>,
        window: Option<AggregateWindow<E, F>>,
    },
    Ranking {
        function: RankingFunction,
        partition_by: Vec<Expression<E, F>>,
        order_by: Vec<OrderItem<E, F>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiteralValue {
    Integer(String),
    Text(String),
    Boolean(bool),
    Null,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhenClause<E, F> {
    pub condition: Expression<E, F>,
    pub result: Expression<E, F>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AggregateArgument<E, F> {
    Star(Span),
    Expression(Box<Expression<E, F>>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateWindow<E, F> {
    pub partition_by: Vec<Expression<E, F>>,
    pub span: Span,
}

/// Tests structural equality under the source grouping rule.
///
/// Redundant parentheses and phase annotations are ignored. Bound field
/// identities, literal values, operator roles, and invocation structure remain
/// significant. Query-bearing predicates are compared using their bound HIR;
/// their independently allocated relation identities consequently remain
/// distinct unless they refer to the same semantic query object.
pub fn structurally_equal<E1, F1, E2, F2>(
    left: &Expression<E1, F1>,
    right: &Expression<E2, F2>,
) -> bool {
    use ExpressionKind as K;

    let left = without_parentheses(left);
    let right = without_parentheses(right);

    match (&left.kind, &right.kind) {
        (K::Literal(left), K::Literal(right)) => left == right,
        (K::Field(left), K::Field(right)) => left == right,
        (
            K::Unary {
                operator: left_operator,
                expression: left_expression,
            },
            K::Unary {
                operator: right_operator,
                expression: right_expression,
            },
        ) => {
            left_operator == right_operator && structurally_equal(left_expression, right_expression)
        }
        (
            K::Binary {
                left: left_left,
                operator: left_operator,
                right: left_right,
            },
            K::Binary {
                left: right_left,
                operator: right_operator,
                right: right_right,
            },
        ) => {
            left_operator == right_operator
                && structurally_equal(left_left, right_left)
                && structurally_equal(left_right, right_right)
        }
        (
            K::IsNull {
                expression: left_expression,
                negated: left_negated,
            },
            K::IsNull {
                expression: right_expression,
                negated: right_negated,
            },
        ) => left_negated == right_negated && structurally_equal(left_expression, right_expression),
        (
            K::InList {
                expression: left_expression,
                negated: left_negated,
                values: left_values,
            },
            K::InList {
                expression: right_expression,
                negated: right_negated,
                values: right_values,
            },
        ) => {
            left_negated == right_negated
                && structurally_equal(left_expression, right_expression)
                && expression_slices_equal(left_values, right_values)
        }
        (
            K::Case {
                branches: left_branches,
                else_expression: left_else,
            },
            K::Case {
                branches: right_branches,
                else_expression: right_else,
            },
        ) => {
            left_branches.len() == right_branches.len()
                && left_branches
                    .iter()
                    .zip(right_branches)
                    .all(|(left, right)| {
                        structurally_equal(&left.condition, &right.condition)
                            && structurally_equal(&left.result, &right.result)
                    })
                && optional_expressions_equal(left_else.as_deref(), right_else.as_deref())
        }
        (
            K::Cast {
                expression: left_expression,
                scalar_type: left_type,
            },
            K::Cast {
                expression: right_expression,
                scalar_type: right_type,
            },
        ) => left_type == right_type && structurally_equal(left_expression, right_expression),
        (
            K::Aggregate {
                function: left_function,
                argument: left_argument,
                window: left_window,
            },
            K::Aggregate {
                function: right_function,
                argument: right_argument,
                window: right_window,
            },
        ) => {
            left_function == right_function
                && aggregate_arguments_equal(left_argument, right_argument)
                && aggregate_windows_equal(left_window.as_ref(), right_window.as_ref())
        }
        (
            K::Ranking {
                function: left_function,
                partition_by: left_partition,
                order_by: left_order,
            },
            K::Ranking {
                function: right_function,
                partition_by: right_partition,
                order_by: right_order,
            },
        ) => {
            left_function == right_function
                && expression_slices_equal(left_partition, right_partition)
                && order_items_equal(left_order, right_order)
        }
        // Query-bearing expressions are never legal grouping expressions in
        // the current reference HIR comparison. They remain scalar and typed,
        // but do not establish group-valid source fields by textual repetition.
        (K::InQuery { .. } | K::Exists { .. }, K::InQuery { .. } | K::Exists { .. }) => false,
        _ => false,
    }
}

fn without_parentheses<E, F>(mut expression: &Expression<E, F>) -> &Expression<E, F> {
    while let ExpressionKind::Parenthesized(inner) = &expression.kind {
        expression = inner;
    }
    expression
}

fn expression_slices_equal<E1, F1, E2, F2>(
    left: &[Expression<E1, F1>],
    right: &[Expression<E2, F2>],
) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| structurally_equal(left, right))
}

fn optional_expressions_equal<E1, F1, E2, F2>(
    left: Option<&Expression<E1, F1>>,
    right: Option<&Expression<E2, F2>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => structurally_equal(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn aggregate_arguments_equal<E1, F1, E2, F2>(
    left: &AggregateArgument<E1, F1>,
    right: &AggregateArgument<E2, F2>,
) -> bool {
    match (left, right) {
        (AggregateArgument::Star(_), AggregateArgument::Star(_)) => true,
        (AggregateArgument::Expression(left), AggregateArgument::Expression(right)) => {
            structurally_equal(left, right)
        }
        _ => false,
    }
}

fn aggregate_windows_equal<E1, F1, E2, F2>(
    left: Option<&AggregateWindow<E1, F1>>,
    right: Option<&AggregateWindow<E2, F2>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            expression_slices_equal(&left.partition_by, &right.partition_by)
        }
        (None, None) => true,
        _ => false,
    }
}

fn order_items_equal<E1, F1, E2, F2>(
    left: &[OrderItem<E1, F1>],
    right: &[OrderItem<E2, F2>],
) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.direction == right.direction
                && left.null_placement == right.null_placement
                && structurally_equal(&left.expression, &right.expression)
        })
}

#[cfg(test)]
mod tests {
    use super::{Expression, ExpressionKind, LiteralValue, structurally_equal};
    use crate::Span;
    use crate::ast::BinaryOperator;

    fn span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn integer(value: &str) -> Expression<(), ()> {
        Expression::new(
            ExpressionKind::Literal(LiteralValue::Integer(value.into())),
            (),
            span(),
        )
    }

    #[test]
    fn structural_equality_ignores_redundant_parentheses() {
        let plain = integer("1");
        let parenthesized = Expression::new(
            ExpressionKind::Parenthesized(Box::new(integer("1"))),
            (),
            span(),
        );

        assert!(structurally_equal(&plain, &parenthesized));
    }

    #[test]
    fn structural_equality_preserves_binary_operand_roles() {
        let left = Expression::new(
            ExpressionKind::Binary {
                left: Box::new(integer("1")),
                operator: BinaryOperator::Add,
                right: Box::new(integer("2")),
            },
            (),
            span(),
        );
        let right = Expression::new(
            ExpressionKind::Binary {
                left: Box::new(integer("2")),
                operator: BinaryOperator::Add,
                right: Box::new(integer("1")),
            },
            (),
            span(),
        );

        assert!(!structurally_equal(&left, &right));
    }
}
