use std::fmt;

use crate::ast::{
    AggregateArgument, AggregateFunction, AggregateWindow, BinaryOperator, CommonTableExpression,
    Expression, Identifier, IdentifierKind, IntegerLiteral, Join, JoinKind, JoinedTable, Literal,
    NullPlacement, OrderDirection, OrderItem, Program, QueryBody, QueryExpression, RankingFunction,
    RowBound, ScalarType, SelectItem, SelectQuery, SetOperator, TablePrimary, TextLiteral,
    UnaryOperator, WhenClause,
};
use crate::{LexError, Span, Token, TokenKind, lex};

/// A lexical or syntactic failure encountered while parsing a source program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Lexical(LexError),
    Syntactic(SyntaxError),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lexical(error) => fmt::Display::fmt(error, formatter),
            Self::Syntactic(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ParseError {}

/// A token mismatch between the input and the ShapeSQL grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub expected: &'static str,
    pub found: Option<TokenKind>,
    pub span: Span,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.found {
            Some(found) => write!(formatter, "expected {}, found {found:?}", self.expected),
            None => write!(formatter, "expected {}, found end of input", self.expected),
        }
    }
}

impl std::error::Error for SyntaxError {}

/// Lexes and parses one complete ShapeSQL source program.
pub fn parse(source: &[u8]) -> Result<Program, ParseError> {
    let tokens = lex(source).map_err(ParseError::Lexical)?;

    Parser::new(&tokens, source.len())
        .parse_program()
        .map_err(ParseError::Syntactic)
}

struct Parser<'tokens> {
    tokens: &'tokens [Token],
    position: usize,
    end_offset: usize,
}

impl<'tokens> Parser<'tokens> {
    fn new(tokens: &'tokens [Token], end_offset: usize) -> Self {
        Self {
            tokens,
            position: 0,
            end_offset,
        }
    }

    fn parse_program(mut self) -> Result<Program, SyntaxError> {
        let query = self.parse_query_expression()?;
        let semicolon = self.eat(TokenKind::Semicolon);

        if self.current().is_some() {
            return Err(self.error("end of input"));
        }

        let span = Span::new(
            query.span.start,
            semicolon.map_or(query.span.end, |token| token.span.end),
        );

        Ok(Program { query, span })
    }

    fn parse_query_expression(&mut self) -> Result<QueryExpression, SyntaxError> {
        let start = self.mark();
        let mut common_table_expressions = Vec::new();

        if self.eat(TokenKind::With).is_some() {
            common_table_expressions.push(self.parse_common_table_expression()?);

            while self.eat(TokenKind::Comma).is_some() {
                common_table_expressions.push(self.parse_common_table_expression()?);
            }
        }

        let body = self.parse_union_or_except_expression()?;
        let mut order_by = Vec::new();
        let mut row_bound = None;

        if self.eat(TokenKind::Order).is_some() {
            self.expect(TokenKind::By, "`BY`")?;
            order_by.push(self.parse_order_item()?);

            while self.eat(TokenKind::Comma).is_some() {
                order_by.push(self.parse_order_item()?);
            }

            if self.at(TokenKind::Limit) || self.at(TokenKind::Offset) {
                row_bound = Some(self.parse_row_bound()?);
            }
        }

        Ok(QueryExpression {
            common_table_expressions,
            body,
            order_by,
            row_bound,
            span: self.span_since(start),
        })
    }

    fn parse_common_table_expression(&mut self) -> Result<CommonTableExpression, SyntaxError> {
        let start = self.mark();
        let name = self.parse_identifier()?;
        self.expect(TokenKind::As, "`AS`")?;
        self.expect(TokenKind::LeftParenthesis, "`(`")?;
        let query = self.parse_query_expression()?;
        self.expect(TokenKind::RightParenthesis, "`)`")?;

        Ok(CommonTableExpression {
            name,
            query: Box::new(query),
            span: self.span_since(start),
        })
    }

    fn parse_union_or_except_expression(&mut self) -> Result<QueryBody, SyntaxError> {
        let mut left = self.parse_intersect_expression()?;

        loop {
            let operator = if self.eat(TokenKind::Union).is_some() {
                SetOperator::Union
            } else if self.eat(TokenKind::Except).is_some() {
                SetOperator::Except
            } else {
                break;
            };
            let all = self.eat(TokenKind::All).is_some();
            let right = self.parse_intersect_expression()?;
            let span = Span::new(left.span().start, right.span().end);

            left = QueryBody::SetOperation {
                left: Box::new(left),
                operator,
                all,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_intersect_expression(&mut self) -> Result<QueryBody, SyntaxError> {
        let mut left = self.parse_query_primary()?;

        while self.eat(TokenKind::Intersect).is_some() {
            let all = self.eat(TokenKind::All).is_some();
            let right = self.parse_query_primary()?;
            let span = Span::new(left.span().start, right.span().end);

            left = QueryBody::SetOperation {
                left: Box::new(left),
                operator: SetOperator::Intersect,
                all,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_query_primary(&mut self) -> Result<QueryBody, SyntaxError> {
        if self.at(TokenKind::Select) {
            return self
                .parse_select_query()
                .map(|query| QueryBody::Select(Box::new(query)));
        }

        if let Some(left_parenthesis) = self.eat(TokenKind::LeftParenthesis) {
            let query = self.parse_query_expression()?;
            let right_parenthesis = self.expect(TokenKind::RightParenthesis, "`)`")?;

            return Ok(QueryBody::Parenthesized {
                query: Box::new(query),
                span: Span::new(left_parenthesis.span.start, right_parenthesis.span.end),
            });
        }

        Err(self.error("a `SELECT` query or parenthesized query"))
    }

    fn parse_select_query(&mut self) -> Result<SelectQuery, SyntaxError> {
        let start = self.mark();
        self.expect(TokenKind::Select, "`SELECT`")?;
        let distinct = self.eat(TokenKind::Distinct).is_some();
        let mut select_list = vec![self.parse_select_item()?];

        while self.eat(TokenKind::Comma).is_some() {
            select_list.push(self.parse_select_item()?);
        }

        self.expect(TokenKind::From, "`FROM`")?;
        let mut from = vec![self.parse_joined_table()?];

        while self.eat(TokenKind::Comma).is_some() {
            from.push(self.parse_joined_table()?);
        }

        let where_clause = if self.eat(TokenKind::Where).is_some() {
            Some(self.parse_expression()?)
        } else {
            None
        };

        let mut group_by = Vec::new();
        if self.eat(TokenKind::Group).is_some() {
            self.expect(TokenKind::By, "`BY`")?;
            group_by = self.parse_expression_list()?;
        }

        let having = if self.eat(TokenKind::Having).is_some() {
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(SelectQuery {
            distinct,
            select_list,
            from,
            where_clause,
            group_by,
            having,
            span: self.span_since(start),
        })
    }

    fn parse_select_item(&mut self) -> Result<SelectItem, SyntaxError> {
        let start = self.mark();

        if self.eat(TokenKind::Star).is_some() {
            return Ok(SelectItem::Wildcard {
                qualifier: None,
                span: self.span_since(start),
            });
        }

        if self.at_identifier()
            && self.peek(1) == Some(TokenKind::Dot)
            && self.peek(2) == Some(TokenKind::Star)
        {
            let qualifier = self.parse_identifier()?;
            self.expect(TokenKind::Dot, "`.`")?;
            self.expect(TokenKind::Star, "`*`")?;

            return Ok(SelectItem::Wildcard {
                qualifier: Some(qualifier),
                span: self.span_since(start),
            });
        }

        let expression = self.parse_expression()?;
        let alias = self.parse_optional_alias()?;

        Ok(SelectItem::Expression {
            expression,
            alias,
            span: self.span_since(start),
        })
    }

    fn parse_joined_table(&mut self) -> Result<JoinedTable, SyntaxError> {
        let start = self.mark();
        let left = self.parse_table_primary()?;
        let mut joins = Vec::new();

        while matches!(
            self.current_kind(),
            Some(
                TokenKind::Cross
                    | TokenKind::Inner
                    | TokenKind::Join
                    | TokenKind::Left
                    | TokenKind::Right
                    | TokenKind::Full
            )
        ) {
            joins.push(self.parse_join()?);
        }

        Ok(JoinedTable {
            left,
            joins,
            span: self.span_since(start),
        })
    }

    fn parse_join(&mut self) -> Result<Join, SyntaxError> {
        let start = self.mark();
        let (kind, needs_condition) = match self.current_kind() {
            Some(TokenKind::Cross) => {
                self.bump();
                self.expect(TokenKind::Join, "`JOIN`")?;
                (JoinKind::Cross, false)
            }
            Some(TokenKind::Inner) => {
                self.bump();
                self.expect(TokenKind::Join, "`JOIN`")?;
                (JoinKind::Inner, true)
            }
            Some(TokenKind::Join) => {
                self.bump();
                (JoinKind::Inner, true)
            }
            Some(TokenKind::Left) => {
                self.bump();
                self.eat(TokenKind::Outer);
                self.expect(TokenKind::Join, "`JOIN`")?;
                (JoinKind::Left, true)
            }
            Some(TokenKind::Right) => {
                self.bump();
                self.eat(TokenKind::Outer);
                self.expect(TokenKind::Join, "`JOIN`")?;
                (JoinKind::Right, true)
            }
            Some(TokenKind::Full) => {
                self.bump();
                self.eat(TokenKind::Outer);
                self.expect(TokenKind::Join, "`JOIN`")?;
                (JoinKind::Full, true)
            }
            _ => return Err(self.error("a join")),
        };

        let right = self.parse_table_primary()?;
        let condition = if needs_condition {
            self.expect(TokenKind::On, "`ON`")?;
            Some(self.parse_expression()?)
        } else {
            None
        };

        Ok(Join {
            kind,
            right,
            condition,
            span: self.span_since(start),
        })
    }

    fn parse_table_primary(&mut self) -> Result<TablePrimary, SyntaxError> {
        let start = self.mark();

        if self.eat(TokenKind::LeftParenthesis).is_some() {
            let query = self.parse_query_expression()?;
            self.expect(TokenKind::RightParenthesis, "`)`")?;
            let alias = self.parse_required_alias()?;

            return Ok(TablePrimary::Derived {
                query: Box::new(query),
                alias,
                span: self.span_since(start),
            });
        }

        let name = self.parse_identifier()?;
        let alias = self.parse_optional_alias()?;

        Ok(TablePrimary::Named {
            name,
            alias,
            span: self.span_since(start),
        })
    }

    fn parse_optional_alias(&mut self) -> Result<Option<Identifier>, SyntaxError> {
        if self.eat(TokenKind::As).is_some() {
            return self.parse_identifier().map(Some);
        }

        if self.at_identifier() {
            return self.parse_identifier().map(Some);
        }

        Ok(None)
    }

    fn parse_required_alias(&mut self) -> Result<Identifier, SyntaxError> {
        self.eat(TokenKind::As);
        self.parse_identifier()
    }

    fn parse_order_item(&mut self) -> Result<OrderItem, SyntaxError> {
        let start = self.mark();
        let expression = self.parse_expression()?;
        let direction = if self.eat(TokenKind::Asc).is_some() {
            Some(OrderDirection::Ascending)
        } else if self.eat(TokenKind::Desc).is_some() {
            Some(OrderDirection::Descending)
        } else {
            None
        };
        let null_placement = if self.eat(TokenKind::Nulls).is_some() {
            if self.eat(TokenKind::First).is_some() {
                Some(NullPlacement::First)
            } else if self.eat(TokenKind::Last).is_some() {
                Some(NullPlacement::Last)
            } else {
                return Err(self.error("`FIRST` or `LAST`"));
            }
        } else {
            None
        };

        Ok(OrderItem {
            expression,
            direction,
            null_placement,
            span: self.span_since(start),
        })
    }

    fn parse_row_bound(&mut self) -> Result<RowBound, SyntaxError> {
        let start = self.mark();
        let (limit, offset) = if self.eat(TokenKind::Limit).is_some() {
            let limit = Some(self.parse_integer_literal()?);
            let offset = if self.eat(TokenKind::Offset).is_some() {
                Some(self.parse_integer_literal()?)
            } else {
                None
            };
            (limit, offset)
        } else {
            self.expect(TokenKind::Offset, "`OFFSET`")?;
            let offset = Some(self.parse_integer_literal()?);
            let limit = if self.eat(TokenKind::Limit).is_some() {
                Some(self.parse_integer_literal()?)
            } else {
                None
            };
            (limit, offset)
        };

        Ok(RowBound {
            limit,
            offset,
            span: self.span_since(start),
        })
    }

    fn parse_expression_list(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        let mut expressions = vec![self.parse_expression()?];

        while self.eat(TokenKind::Comma).is_some() {
            expressions.push(self.parse_expression()?);
        }

        Ok(expressions)
    }

    fn parse_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.parse_or_expression()
    }

    fn parse_or_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut left = self.parse_and_expression()?;

        while self.eat(TokenKind::Or).is_some() {
            let right = self.parse_and_expression()?;
            left = binary_expression(left, BinaryOperator::Or, right);
        }

        Ok(left)
    }

    fn parse_and_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut left = self.parse_not_expression()?;

        while self.eat(TokenKind::And).is_some() {
            let right = self.parse_not_expression()?;
            left = binary_expression(left, BinaryOperator::And, right);
        }

        Ok(left)
    }

    fn parse_not_expression(&mut self) -> Result<Expression, SyntaxError> {
        if let Some(operator) = self.eat(TokenKind::Not) {
            let expression = self.parse_not_expression()?;
            let span = Span::new(operator.span.start, expression.span().end);

            return Ok(Expression::Unary {
                operator: UnaryOperator::Not,
                expression: Box::new(expression),
                span,
            });
        }

        self.parse_predicate_expression()
    }

    fn parse_predicate_expression(&mut self) -> Result<Expression, SyntaxError> {
        let left = self.parse_concatenation_expression()?;

        let comparison = match self.current_kind() {
            Some(TokenKind::Equal) => Some(BinaryOperator::Equal),
            Some(TokenKind::NotEqual) => Some(BinaryOperator::NotEqual),
            Some(TokenKind::Less) => Some(BinaryOperator::Less),
            Some(TokenKind::LessEqual) => Some(BinaryOperator::LessEqual),
            Some(TokenKind::Greater) => Some(BinaryOperator::Greater),
            Some(TokenKind::GreaterEqual) => Some(BinaryOperator::GreaterEqual),
            _ => None,
        };

        if let Some(operator) = comparison {
            self.bump();
            let right = self.parse_concatenation_expression()?;
            return Ok(binary_expression(left, operator, right));
        }

        if self.eat(TokenKind::Is).is_some() {
            let negated = self.eat(TokenKind::Not).is_some();
            let null = self.expect(TokenKind::Null, "`NULL`")?;
            let span = Span::new(left.span().start, null.span.end);

            return Ok(Expression::IsNull {
                expression: Box::new(left),
                negated,
                span,
            });
        }

        let negated = self.at(TokenKind::Not) && self.peek(1) == Some(TokenKind::In);
        if negated {
            self.bump();
        }

        if negated || self.at(TokenKind::In) {
            return self.parse_in_predicate(left, negated);
        }

        Ok(left)
    }

    fn parse_in_predicate(
        &mut self,
        left: Expression,
        negated: bool,
    ) -> Result<Expression, SyntaxError> {
        self.expect(TokenKind::In, "`IN`")?;
        self.expect(TokenKind::LeftParenthesis, "`(`")?;

        if self.in_contents_is_query() {
            let query = self.parse_query_expression()?;
            let right_parenthesis = self.expect(TokenKind::RightParenthesis, "`)`")?;
            let span = Span::new(left.span().start, right_parenthesis.span.end);

            return Ok(Expression::InQuery {
                expression: Box::new(left),
                negated,
                query: Box::new(query),
                span,
            });
        }

        let values = self.parse_expression_list()?;
        let right_parenthesis = self.expect(TokenKind::RightParenthesis, "`)`")?;
        let span = Span::new(left.span().start, right_parenthesis.span.end);

        Ok(Expression::InList {
            expression: Box::new(left),
            negated,
            values,
            span,
        })
    }

    fn in_contents_is_query(&self) -> bool {
        // The query and expression-list alternatives can both begin with `(`.
        // After any query parentheses, however, a query must begin with
        // `SELECT` or `WITH`, neither of which can begin a scalar expression.
        let mut lookahead = 0;

        while self.peek(lookahead) == Some(TokenKind::LeftParenthesis) {
            lookahead += 1;
        }

        matches!(
            self.peek(lookahead),
            Some(TokenKind::Select | TokenKind::With)
        )
    }

    fn parse_concatenation_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut left = self.parse_additive_expression()?;

        while self.eat(TokenKind::Concatenate).is_some() {
            let right = self.parse_additive_expression()?;
            left = binary_expression(left, BinaryOperator::Concatenate, right);
        }

        Ok(left)
    }

    fn parse_additive_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut left = self.parse_multiplicative_expression()?;

        loop {
            let operator = if self.eat(TokenKind::Plus).is_some() {
                BinaryOperator::Add
            } else if self.eat(TokenKind::Minus).is_some() {
                BinaryOperator::Subtract
            } else {
                break;
            };
            let right = self.parse_multiplicative_expression()?;
            left = binary_expression(left, operator, right);
        }

        Ok(left)
    }

    fn parse_multiplicative_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut left = self.parse_unary_expression()?;

        loop {
            let operator = if self.eat(TokenKind::Star).is_some() {
                BinaryOperator::Multiply
            } else if self.eat(TokenKind::Slash).is_some() {
                BinaryOperator::Divide
            } else if self.eat(TokenKind::Percent).is_some() {
                BinaryOperator::Remainder
            } else {
                break;
            };
            let right = self.parse_unary_expression()?;
            left = binary_expression(left, operator, right);
        }

        Ok(left)
    }

    fn parse_unary_expression(&mut self) -> Result<Expression, SyntaxError> {
        let (token, operator) = if let Some(token) = self.eat(TokenKind::Plus) {
            (token, UnaryOperator::Positive)
        } else if let Some(token) = self.eat(TokenKind::Minus) {
            (token, UnaryOperator::Negative)
        } else {
            return self.parse_primary_expression();
        };
        let expression = self.parse_unary_expression()?;
        let span = Span::new(token.span.start, expression.span().end);

        Ok(Expression::Unary {
            operator,
            expression: Box::new(expression),
            span,
        })
    }

    fn parse_primary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match self.current_kind() {
            Some(TokenKind::IntegerLiteral) => {
                let literal = self.parse_integer_literal()?;
                Ok(Expression::Literal(Literal::Integer(literal)))
            }
            Some(TokenKind::TextLiteral) => {
                let token = self.bump().expect("the current token exists");
                Ok(Expression::Literal(Literal::Text(TextLiteral {
                    span: token.span,
                })))
            }
            Some(TokenKind::True | TokenKind::False) => {
                let token = self.bump().expect("the current token exists");
                Ok(Expression::Literal(Literal::Boolean {
                    value: token.kind == TokenKind::True,
                    span: token.span,
                }))
            }
            Some(TokenKind::Null) => {
                let token = self.bump().expect("the current token exists");
                Ok(Expression::Literal(Literal::Null { span: token.span }))
            }
            Some(TokenKind::Identifier | TokenKind::DelimitedIdentifier) => {
                self.parse_column_reference()
            }
            Some(TokenKind::LeftParenthesis) => self.parse_parenthesized_expression(),
            Some(TokenKind::Case) => self.parse_case_expression(),
            Some(TokenKind::Cast) => self.parse_cast_expression(),
            Some(TokenKind::Exists) => self.parse_exists_expression(),
            Some(
                TokenKind::Count
                | TokenKind::Sum
                | TokenKind::Min
                | TokenKind::Max
                | TokenKind::BoolAnd
                | TokenKind::BoolOr,
            ) => self.parse_aggregate_expression(),
            Some(TokenKind::RowNumber | TokenKind::Rank | TokenKind::DenseRank) => {
                self.parse_ranking_expression()
            }
            _ => Err(self.error("an expression")),
        }
    }

    fn parse_column_reference(&mut self) -> Result<Expression, SyntaxError> {
        let first = self.parse_identifier()?;

        if self.eat(TokenKind::Dot).is_some() {
            let name = self.parse_identifier()?;
            let span = Span::new(first.span.start, name.span.end);

            Ok(Expression::ColumnReference {
                qualifier: Some(first),
                name,
                span,
            })
        } else {
            Ok(Expression::ColumnReference {
                qualifier: None,
                name: first,
                span: first.span,
            })
        }
    }

    fn parse_parenthesized_expression(&mut self) -> Result<Expression, SyntaxError> {
        let left_parenthesis = self.expect(TokenKind::LeftParenthesis, "`(`")?;
        let expression = self.parse_expression()?;
        let right_parenthesis = self.expect(TokenKind::RightParenthesis, "`)`")?;

        Ok(Expression::Parenthesized {
            expression: Box::new(expression),
            span: Span::new(left_parenthesis.span.start, right_parenthesis.span.end),
        })
    }

    fn parse_case_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.mark();
        self.expect(TokenKind::Case, "`CASE`")?;
        let mut branches = vec![self.parse_when_clause()?];

        while self.at(TokenKind::When) {
            branches.push(self.parse_when_clause()?);
        }

        let else_expression = if self.eat(TokenKind::Else).is_some() {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };

        self.expect(TokenKind::End, "`END`")?;

        Ok(Expression::Case {
            branches,
            else_expression,
            span: self.span_since(start),
        })
    }

    fn parse_when_clause(&mut self) -> Result<WhenClause, SyntaxError> {
        let start = self.mark();
        self.expect(TokenKind::When, "`WHEN`")?;
        let condition = self.parse_expression()?;
        self.expect(TokenKind::Then, "`THEN`")?;
        let result = self.parse_expression()?;

        Ok(WhenClause {
            condition,
            result,
            span: self.span_since(start),
        })
    }

    fn parse_cast_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.mark();
        self.expect(TokenKind::Cast, "`CAST`")?;
        self.expect(TokenKind::LeftParenthesis, "`(`")?;
        let expression = self.parse_expression()?;
        self.expect(TokenKind::As, "`AS`")?;
        let scalar_type = match self.current_kind() {
            Some(TokenKind::Boolean) => {
                self.bump();
                ScalarType::Boolean
            }
            Some(TokenKind::Int64) => {
                self.bump();
                ScalarType::Int64
            }
            Some(TokenKind::Text) => {
                self.bump();
                ScalarType::Text
            }
            _ => return Err(self.error("`BOOLEAN`, `INT64`, or `TEXT`")),
        };
        self.expect(TokenKind::RightParenthesis, "`)`")?;

        Ok(Expression::Cast {
            expression: Box::new(expression),
            scalar_type,
            span: self.span_since(start),
        })
    }

    fn parse_exists_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.mark();
        self.expect(TokenKind::Exists, "`EXISTS`")?;
        self.expect(TokenKind::LeftParenthesis, "`(`")?;
        let query = self.parse_query_expression()?;
        self.expect(TokenKind::RightParenthesis, "`)`")?;

        Ok(Expression::Exists {
            query: Box::new(query),
            span: self.span_since(start),
        })
    }

    fn parse_aggregate_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.mark();
        let function = match self.current_kind() {
            Some(TokenKind::Count) => AggregateFunction::Count,
            Some(TokenKind::Sum) => AggregateFunction::Sum,
            Some(TokenKind::Min) => AggregateFunction::Min,
            Some(TokenKind::Max) => AggregateFunction::Max,
            Some(TokenKind::BoolAnd) => AggregateFunction::BoolAnd,
            Some(TokenKind::BoolOr) => AggregateFunction::BoolOr,
            _ => return Err(self.error("an aggregate function")),
        };
        self.bump();
        self.expect(TokenKind::LeftParenthesis, "`(`")?;
        let argument = if function == AggregateFunction::Count && self.at(TokenKind::Star) {
            let star = self.bump().expect("the current token exists");
            AggregateArgument::Star(star.span)
        } else {
            AggregateArgument::Expression(Box::new(self.parse_expression()?))
        };
        self.expect(TokenKind::RightParenthesis, "`)`")?;

        let window = if self.eat(TokenKind::Over).is_some() {
            let window_start = self.mark() - 1;
            self.expect(TokenKind::LeftParenthesis, "`(`")?;
            let partition_by = if self.eat(TokenKind::Partition).is_some() {
                self.expect(TokenKind::By, "`BY`")?;
                self.parse_expression_list()?
            } else {
                Vec::new()
            };
            self.expect(TokenKind::RightParenthesis, "`)`")?;

            Some(AggregateWindow {
                partition_by,
                span: self.span_since(window_start),
            })
        } else {
            None
        };

        Ok(Expression::Aggregate {
            function,
            argument,
            window,
            span: self.span_since(start),
        })
    }

    fn parse_ranking_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.mark();
        let function = match self.current_kind() {
            Some(TokenKind::RowNumber) => RankingFunction::RowNumber,
            Some(TokenKind::Rank) => RankingFunction::Rank,
            Some(TokenKind::DenseRank) => RankingFunction::DenseRank,
            _ => return Err(self.error("a ranking function")),
        };
        self.bump();
        self.expect(TokenKind::LeftParenthesis, "`(`")?;
        self.expect(TokenKind::RightParenthesis, "`)`")?;
        self.expect(TokenKind::Over, "`OVER`")?;
        self.expect(TokenKind::LeftParenthesis, "`(`")?;

        let partition_by = if self.eat(TokenKind::Partition).is_some() {
            self.expect(TokenKind::By, "`BY`")?;
            self.parse_expression_list()?
        } else {
            Vec::new()
        };

        self.expect(TokenKind::Order, "`ORDER`")?;
        self.expect(TokenKind::By, "`BY`")?;
        let mut order_by = vec![self.parse_order_item()?];

        while self.eat(TokenKind::Comma).is_some() {
            order_by.push(self.parse_order_item()?);
        }

        self.expect(TokenKind::RightParenthesis, "`)`")?;

        Ok(Expression::Ranking {
            function,
            partition_by,
            order_by,
            span: self.span_since(start),
        })
    }

    fn parse_identifier(&mut self) -> Result<Identifier, SyntaxError> {
        let token = match self.current_kind() {
            Some(TokenKind::Identifier | TokenKind::DelimitedIdentifier) => {
                self.bump().expect("the current token exists")
            }
            _ => return Err(self.error("an identifier")),
        };
        let kind = if token.kind == TokenKind::Identifier {
            IdentifierKind::Regular
        } else {
            IdentifierKind::Delimited
        };

        Ok(Identifier {
            kind,
            span: token.span,
        })
    }

    fn parse_integer_literal(&mut self) -> Result<IntegerLiteral, SyntaxError> {
        let token = self.expect(TokenKind::IntegerLiteral, "an integer literal")?;
        Ok(IntegerLiteral { span: token.span })
    }

    fn mark(&self) -> usize {
        self.position
    }

    fn span_since(&self, start: usize) -> Span {
        let first = self
            .tokens
            .get(start)
            .expect("a parsed production consumes at least one token");
        let last = self
            .tokens
            .get(self.position - 1)
            .expect("a parsed production consumes at least one token");

        Span::new(first.span.start, last.span.end)
    }

    fn current(&self) -> Option<Token> {
        self.tokens.get(self.position).copied()
    }

    fn current_kind(&self) -> Option<TokenKind> {
        self.current().map(|token| token.kind)
    }

    fn peek(&self, lookahead: usize) -> Option<TokenKind> {
        self.tokens
            .get(self.position + lookahead)
            .map(|token| token.kind)
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current_kind() == Some(kind)
    }

    fn at_identifier(&self) -> bool {
        matches!(
            self.current_kind(),
            Some(TokenKind::Identifier | TokenKind::DelimitedIdentifier)
        )
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.current()?;
        self.position += 1;
        Some(token)
    }

    fn eat(&mut self, kind: TokenKind) -> Option<Token> {
        if self.at(kind) { self.bump() } else { None }
    }

    fn expect(&mut self, kind: TokenKind, expected: &'static str) -> Result<Token, SyntaxError> {
        self.eat(kind).ok_or_else(|| self.error(expected))
    }

    fn error(&self, expected: &'static str) -> SyntaxError {
        let token = self.current();
        let (found, span) = match token {
            Some(token) => (Some(token.kind), token.span),
            None => (None, Span::new(self.end_offset, self.end_offset)),
        };

        SyntaxError {
            expected,
            found,
            span,
        }
    }
}

fn binary_expression(left: Expression, operator: BinaryOperator, right: Expression) -> Expression {
    let span = Span::new(left.span().start, right.span().end);

    Expression::Binary {
        left: Box::new(left),
        operator,
        right: Box::new(right),
        span,
    }
}
