use crate::Span;

/// A complete ShapeSQL source program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub query: QueryExpression,
    pub span: Span,
}

/// A query, including any common table expressions, ordering, and row bound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryExpression {
    pub common_table_expressions: Vec<CommonTableExpression>,
    pub body: QueryBody,
    pub order_by: Vec<OrderItem>,
    pub row_bound: Option<RowBound>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonTableExpression {
    pub name: Identifier,
    pub query: Box<QueryExpression>,
    pub span: Span,
}

/// The relational body of a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryBody {
    Select(Box<SelectQuery>),
    Parenthesized {
        query: Box<QueryExpression>,
        span: Span,
    },
    SetOperation {
        left: Box<QueryBody>,
        operator: SetOperator,
        all: bool,
        right: Box<QueryBody>,
        span: Span,
    },
}

impl QueryBody {
    pub fn span(&self) -> Span {
        match self {
            Self::Select(query) => query.span,
            Self::Parenthesized { span, .. } | Self::SetOperation { span, .. } => *span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOperator {
    Union,
    Except,
    Intersect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectQuery {
    pub distinct: bool,
    pub select_list: Vec<SelectItem>,
    pub from: Vec<JoinedTable>,
    pub where_clause: Option<Expression>,
    pub group_by: Vec<Expression>,
    pub having: Option<Expression>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectItem {
    Wildcard {
        qualifier: Option<Identifier>,
        span: Span,
    },
    Expression {
        expression: Expression,
        alias: Option<Identifier>,
        span: Span,
    },
}

impl SelectItem {
    pub fn span(&self) -> Span {
        match self {
            Self::Wildcard { span, .. } | Self::Expression { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JoinedTable {
    pub left: TablePrimary,
    pub joins: Vec<Join>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TablePrimary {
    Named {
        name: Identifier,
        alias: Option<Identifier>,
        span: Span,
    },
    Derived {
        query: Box<QueryExpression>,
        alias: Identifier,
        span: Span,
    },
}

impl TablePrimary {
    pub fn span(&self) -> Span {
        match self {
            Self::Named { span, .. } | Self::Derived { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Join {
    pub kind: JoinKind,
    pub right: TablePrimary,
    pub condition: Option<Expression>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    Cross,
    Inner,
    Left,
    Right,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderItem {
    pub expression: Expression,
    pub direction: Option<OrderDirection>,
    pub null_placement: Option<NullPlacement>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderDirection {
    Ascending,
    Descending,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NullPlacement {
    First,
    Last,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowBound {
    pub limit: Option<IntegerLiteral>,
    pub offset: Option<IntegerLiteral>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expression {
    Literal(Literal),
    ColumnReference {
        qualifier: Option<Identifier>,
        name: Identifier,
        span: Span,
    },
    Parenthesized {
        expression: Box<Expression>,
        span: Span,
    },
    Unary {
        operator: UnaryOperator,
        expression: Box<Expression>,
        span: Span,
    },
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
        span: Span,
    },
    IsNull {
        expression: Box<Expression>,
        negated: bool,
        span: Span,
    },
    InList {
        expression: Box<Expression>,
        negated: bool,
        values: Vec<Expression>,
        span: Span,
    },
    InQuery {
        expression: Box<Expression>,
        negated: bool,
        query: Box<QueryExpression>,
        span: Span,
    },
    Case {
        branches: Vec<WhenClause>,
        else_expression: Option<Box<Expression>>,
        span: Span,
    },
    Cast {
        expression: Box<Expression>,
        scalar_type: ScalarType,
        span: Span,
    },
    Exists {
        query: Box<QueryExpression>,
        span: Span,
    },
    Aggregate {
        function: AggregateFunction,
        argument: AggregateArgument,
        window: Option<AggregateWindow>,
        span: Span,
    },
    Ranking {
        function: RankingFunction,
        partition_by: Vec<Expression>,
        order_by: Vec<OrderItem>,
        span: Span,
    },
}

impl Expression {
    pub fn span(&self) -> Span {
        match self {
            Self::Literal(literal) => literal.span(),
            Self::ColumnReference { span, .. }
            | Self::Parenthesized { span, .. }
            | Self::Unary { span, .. }
            | Self::Binary { span, .. }
            | Self::IsNull { span, .. }
            | Self::InList { span, .. }
            | Self::InQuery { span, .. }
            | Self::Case { span, .. }
            | Self::Cast { span, .. }
            | Self::Exists { span, .. }
            | Self::Aggregate { span, .. }
            | Self::Ranking { span, .. } => *span,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Literal {
    Integer(IntegerLiteral),
    Text(TextLiteral),
    Boolean { value: bool, span: Span },
    Null { span: Span },
}

impl Literal {
    pub fn span(&self) -> Span {
        match self {
            Self::Integer(literal) => literal.span,
            Self::Text(literal) => literal.span,
            Self::Boolean { span, .. } | Self::Null { span } => *span,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntegerLiteral {
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextLiteral {
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identifier {
    pub kind: IdentifierKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdentifierKind {
    Regular,
    Delimited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOperator {
    Positive,
    Negative,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    Or,
    And,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Concatenate,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WhenClause {
    pub condition: Expression,
    pub result: Expression,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    Boolean,
    Int64,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregateFunction {
    Count,
    Sum,
    Min,
    Max,
    BoolAnd,
    BoolOr,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AggregateArgument {
    Star(Span),
    Expression(Box<Expression>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateWindow {
    pub partition_by: Vec<Expression>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankingFunction {
    RowNumber,
    Rank,
    DenseRank,
}
