//! ShapeSQL name binding and wildcard expansion.

use std::fmt;

use crate::ast;
use crate::hir::{
    self, AggregateArgument, AggregateWindow, BoundExpression, BoundField, BoundFieldAnnotation,
    BoundProgram, BoundQueryExpression, CommonTableExpression, Expression, ExpressionKind, Field,
    IntegerValue, JoinedTable, LiteralValue, OrderItem, QueryBody, QueryExpression,
    RelationOccurrence, RowBound, SelectField, SelectQuery, TablePrimary, WhenClause,
};
use crate::{
    Catalog, CatalogRelation, CteId, FieldId, Name, ParsedProgram, RelationOccurrenceId, Span,
};

/// A name-resolution failure in syntactically valid ShapeSQL source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindError {
    pub kind: BindErrorKind,
    pub span: Span,
}

/// Binding failure categories. Diagnostic wording is intentionally not part
/// of the language contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindErrorKind {
    UnknownRelation(Name),
    AmbiguousRelation(Name),
    DuplicateCommonTableExpression(Name),
    CommonTableExpressionCycle(Name),
    DuplicateSourceQualifier(Name),
    UnknownQualifier(Name),
    UnknownColumn(Name),
    AmbiguousColumn(Name),
    OrderByOrdinalOutOfRange(String),
}

impl fmt::Display for BindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        use BindErrorKind as K;
        match &self.kind {
            K::UnknownRelation(name) => write!(formatter, "unknown relation `{name}`"),
            K::AmbiguousRelation(name) => write!(formatter, "ambiguous relation `{name}`"),
            K::DuplicateCommonTableExpression(name) => {
                write!(formatter, "duplicate common table expression `{name}`")
            }
            K::CommonTableExpressionCycle(name) => {
                write!(formatter, "common table expression cycle through `{name}`")
            }
            K::DuplicateSourceQualifier(name) => {
                write!(formatter, "duplicate source qualifier `{name}`")
            }
            K::UnknownQualifier(name) => write!(formatter, "unknown source qualifier `{name}`"),
            K::UnknownColumn(name) => write!(formatter, "unknown column `{name}`"),
            K::AmbiguousColumn(name) => write!(formatter, "ambiguous column `{name}`"),
            K::OrderByOrdinalOutOfRange(ordinal) => {
                write!(formatter, "ORDER BY ordinal `{ordinal}` is out of range")
            }
        }
    }
}

impl std::error::Error for BindError {}

/// Resolves a parsed program against one immutable catalog snapshot.
pub fn bind(parsed: &ParsedProgram, catalog: &Catalog) -> Result<BoundProgram, BindError> {
    Binder::new(parsed, catalog).bind_program()
}

struct Binder<'a> {
    parsed: &'a ParsedProgram,
    catalog: &'a Catalog,
    next_field: u32,
    next_occurrence: u32,
    next_cte: u32,
    cte_scopes: Vec<CteScope>,
}

type BoundOccurrence = RelationOccurrence<BoundFieldAnnotation>;
type BoundJoinedTable = JoinedTable<(), BoundFieldAnnotation>;

#[derive(Clone)]
struct CteScope {
    lexical_chain: Vec<usize>,
    declarations: Vec<CteDeclaration>,
}

#[derive(Clone)]
struct CteDeclaration {
    id: CteId,
    name: Name,
    syntax: ast::CommonTableExpression,
    state: CteState,
}

#[derive(Clone)]
enum CteState {
    Unvisited,
    Binding,
    Bound(Box<BoundQueryExpression>),
}

#[derive(Clone, Copy)]
struct CteReference {
    scope: usize,
    declaration: usize,
}

impl<'a> Binder<'a> {
    fn new(parsed: &'a ParsedProgram, catalog: &'a Catalog) -> Self {
        Self {
            parsed,
            catalog,
            next_field: 0,
            next_occurrence: 0,
            next_cte: 0,
            cte_scopes: Vec::new(),
        }
    }

    fn bind_program(mut self) -> Result<BoundProgram, BindError> {
        let syntax = self.parsed.syntax();
        let query = self.bind_query_expression(&syntax.query, &[])?;
        Ok(hir::Program {
            query,
            span: syntax.span,
        })
    }

    fn bind_query_expression(
        &mut self,
        syntax: &ast::QueryExpression,
        enclosing_scopes: &[usize],
    ) -> Result<BoundQueryExpression, BindError> {
        let (scope_chain, local_scope) =
            self.establish_cte_scope(&syntax.common_table_expressions, enclosing_scopes)?;

        let body = self.bind_query_body(&syntax.body, &scope_chain)?;
        let result_fields = body.result_fields().to_vec();

        let (fallback_sources, window_sources) = match &body {
            QueryBody::Select(select) => {
                let sources = select_occurrences(select);
                let fallback = (!select.distinct).then(|| sources.clone());
                (fallback, Some(sources))
            }
            QueryBody::Parenthesized { .. } | QueryBody::SetOperation { .. } => (None, None),
        };

        let order_by = syntax
            .order_by
            .iter()
            .map(|item| {
                self.bind_order_item(
                    item,
                    &result_fields,
                    fallback_sources.as_deref(),
                    window_sources.as_deref(),
                    &scope_chain,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        let row_bound = syntax.row_bound.as_ref().map(|bound| RowBound {
            limit: bound.limit.map(|literal| IntegerValue {
                spelling: self.parsed.integer_spelling(literal).into(),
                span: literal.span,
            }),
            offset: bound.offset.map(|literal| IntegerValue {
                spelling: self.parsed.integer_spelling(literal).into(),
                span: literal.span,
            }),
            span: bound.span,
        });

        let common_table_expressions = match local_scope {
            Some(scope) => self.finish_cte_scope(scope)?,
            None => Vec::new(),
        };

        Ok(QueryExpression {
            common_table_expressions,
            body,
            order_by,
            row_bound,
            result_fields,
            span: syntax.span,
        })
    }

    fn establish_cte_scope(
        &mut self,
        declarations: &[ast::CommonTableExpression],
        enclosing_scopes: &[usize],
    ) -> Result<(Vec<usize>, Option<usize>), BindError> {
        if declarations.is_empty() {
            return Ok((enclosing_scopes.to_vec(), None));
        }

        let mut names = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let name = self.parsed.identifier_name(declaration.name);
            if names.contains(&name) {
                return Err(BindError {
                    kind: BindErrorKind::DuplicateCommonTableExpression(name),
                    span: declaration.name.span,
                });
            }
            names.push(name);
        }

        let scope_id = self.cte_scopes.len();
        let mut lexical_chain = enclosing_scopes.to_vec();
        lexical_chain.push(scope_id);
        let declarations = declarations
            .iter()
            .cloned()
            .zip(names)
            .map(|(syntax, name)| CteDeclaration {
                id: self.fresh_cte(),
                name,
                syntax,
                state: CteState::Unvisited,
            })
            .collect();
        self.cte_scopes.push(CteScope {
            lexical_chain: lexical_chain.clone(),
            declarations,
        });

        Ok((lexical_chain, Some(scope_id)))
    }

    fn finish_cte_scope(
        &mut self,
        scope: usize,
    ) -> Result<Vec<CommonTableExpression<(), BoundFieldAnnotation>>, BindError> {
        let count = self.cte_scopes[scope].declarations.len();
        for declaration in 0..count {
            self.bind_cte(CteReference { scope, declaration })?;
        }

        Ok(self.cte_scopes[scope]
            .declarations
            .iter()
            .map(|declaration| {
                let CteState::Bound(query) = &declaration.state else {
                    unreachable!("all CTE declarations were bound")
                };
                CommonTableExpression {
                    id: declaration.id,
                    name: declaration.name.clone(),
                    query: query.clone(),
                    span: declaration.syntax.span,
                }
            })
            .collect())
    }

    fn bind_cte(&mut self, reference: CteReference) -> Result<(), BindError> {
        let (state, name, span, syntax, lexical_chain) = {
            let scope = &self.cte_scopes[reference.scope];
            let declaration = &scope.declarations[reference.declaration];
            (
                declaration.state.clone(),
                declaration.name.clone(),
                declaration.syntax.name.span,
                declaration.syntax.query.as_ref().clone(),
                scope.lexical_chain.clone(),
            )
        };

        match state {
            CteState::Bound(_) => return Ok(()),
            CteState::Binding => {
                return Err(BindError {
                    kind: BindErrorKind::CommonTableExpressionCycle(name),
                    span,
                });
            }
            CteState::Unvisited => {}
        }

        self.cte_scopes[reference.scope].declarations[reference.declaration].state =
            CteState::Binding;
        let query = self.bind_query_expression(&syntax, &lexical_chain)?;
        self.cte_scopes[reference.scope].declarations[reference.declaration].state =
            CteState::Bound(Box::new(query));
        Ok(())
    }

    fn bind_query_body(
        &mut self,
        syntax: &ast::QueryBody,
        cte_scopes: &[usize],
    ) -> Result<QueryBody<(), BoundFieldAnnotation>, BindError> {
        match syntax {
            ast::QueryBody::Select(select) => Ok(QueryBody::Select(Box::new(
                self.bind_select_query(select, cte_scopes)?,
            ))),
            ast::QueryBody::Parenthesized { query, span } => Ok(QueryBody::Parenthesized {
                query: Box::new(self.bind_query_expression(query, cte_scopes)?),
                span: *span,
            }),
            ast::QueryBody::SetOperation {
                left,
                operator,
                all,
                right,
                span,
            } => {
                let left = self.bind_query_body(left, cte_scopes)?;
                let right = self.bind_query_body(right, cte_scopes)?;
                let result_fields = left
                    .result_fields()
                    .iter()
                    .map(|field| self.fresh_result_field(field.name.clone()))
                    .collect();
                Ok(QueryBody::SetOperation {
                    left: Box::new(left),
                    operator: *operator,
                    all: *all,
                    right: Box::new(right),
                    result_fields,
                    span: *span,
                })
            }
        }
    }

    fn bind_select_query(
        &mut self,
        syntax: &ast::SelectQuery,
        cte_scopes: &[usize],
    ) -> Result<SelectQuery<(), BoundFieldAnnotation>, BindError> {
        let mut qualifiers = Vec::new();
        let mut source_occurrences = Vec::new();
        let mut from = Vec::with_capacity(syntax.from.len());

        for joined in &syntax.from {
            let (bound, occurrences) =
                self.bind_joined_table(joined, cte_scopes, &mut qualifiers)?;
            source_occurrences.extend(occurrences);
            from.push(bound);
        }

        let source_fields = source_occurrences
            .iter()
            .flat_map(|occurrence| occurrence.fields.iter().cloned())
            .collect();
        let where_clause = syntax
            .where_clause
            .as_ref()
            .map(|expression| self.bind_expression(expression, &source_occurrences, cte_scopes))
            .transpose()?;
        let group_by = syntax
            .group_by
            .iter()
            .map(|expression| self.bind_expression(expression, &source_occurrences, cte_scopes))
            .collect::<Result<Vec<_>, _>>()?;
        let having = syntax
            .having
            .as_ref()
            .map(|expression| self.bind_expression(expression, &source_occurrences, cte_scopes))
            .transpose()?;

        let mut select_list = Vec::new();
        for item in &syntax.select_list {
            match item {
                ast::SelectItem::Wildcard { qualifier, span } => {
                    let expanded = match qualifier {
                        Some(qualifier) => {
                            let qualifier_name = self.parsed.identifier_name(*qualifier);
                            let matches = source_occurrences
                                .iter()
                                .filter(|source| source.qualifier == qualifier_name)
                                .collect::<Vec<_>>();
                            if matches.len() != 1 {
                                return Err(BindError {
                                    kind: BindErrorKind::UnknownQualifier(qualifier_name),
                                    span: qualifier.span,
                                });
                            }
                            matches[0].fields.iter().collect::<Vec<_>>()
                        }
                        None => source_occurrences
                            .iter()
                            .flat_map(|source| source.fields.iter())
                            .collect(),
                    };

                    for source_field in expanded {
                        let output = self.fresh_result_field(source_field.name.clone());
                        select_list.push(SelectField {
                            output,
                            expression: Expression::new(
                                ExpressionKind::Field(source_field.id),
                                (),
                                *span,
                            ),
                            span: *span,
                        });
                    }
                }
                ast::SelectItem::Expression {
                    expression,
                    alias,
                    span,
                } => {
                    let expression =
                        self.bind_expression(expression, &source_occurrences, cte_scopes)?;
                    let name = match alias {
                        Some(alias) => self.parsed.identifier_name(*alias),
                        None => expression_field(&expression)
                            .and_then(|field| find_field_name(&source_occurrences, field))
                            .unwrap_or_default(),
                    };
                    select_list.push(SelectField {
                        output: self.fresh_result_field(name),
                        expression,
                        span: *span,
                    });
                }
            }
        }

        let result_fields = select_list.iter().map(|item| item.output.clone()).collect();
        let is_aggregate = !group_by.is_empty()
            || select_list
                .iter()
                .any(|item| contains_grouping_aggregate(&item.expression))
            || having.as_ref().is_some_and(contains_grouping_aggregate);

        Ok(SelectQuery {
            distinct: syntax.distinct,
            select_list,
            from,
            source_fields,
            where_clause,
            group_by,
            having,
            result_fields,
            is_aggregate,
            span: syntax.span,
        })
    }

    fn bind_joined_table(
        &mut self,
        syntax: &ast::JoinedTable,
        cte_scopes: &[usize],
        block_qualifiers: &mut Vec<Name>,
    ) -> Result<(BoundJoinedTable, Vec<BoundOccurrence>), BindError> {
        let left = self.bind_table_primary(&syntax.left, cte_scopes)?;
        self.register_qualifier(left.occurrence(), block_qualifiers, syntax.left.span())?;
        let mut occurrences = vec![left.occurrence().clone()];
        let mut joins = Vec::with_capacity(syntax.joins.len());

        for join in &syntax.joins {
            let right = self.bind_table_primary(&join.right, cte_scopes)?;
            self.register_qualifier(right.occurrence(), block_qualifiers, join.right.span())?;

            let mut condition_sources = occurrences.clone();
            condition_sources.push(right.occurrence().clone());
            let condition = join
                .condition
                .as_ref()
                .map(|condition| self.bind_expression(condition, &condition_sources, cte_scopes))
                .transpose()?;

            occurrences.push(right.occurrence().clone());
            joins.push(hir::Join {
                kind: join.kind,
                right,
                condition,
                span: join.span,
            });
        }

        Ok((
            JoinedTable {
                left,
                joins,
                span: syntax.span,
            },
            occurrences,
        ))
    }

    fn register_qualifier(
        &self,
        occurrence: &RelationOccurrence<BoundFieldAnnotation>,
        qualifiers: &mut Vec<Name>,
        span: Span,
    ) -> Result<(), BindError> {
        if qualifiers.contains(&occurrence.qualifier) {
            return Err(BindError {
                kind: BindErrorKind::DuplicateSourceQualifier(occurrence.qualifier.clone()),
                span,
            });
        }
        qualifiers.push(occurrence.qualifier.clone());
        Ok(())
    }

    fn bind_table_primary(
        &mut self,
        syntax: &ast::TablePrimary,
        cte_scopes: &[usize],
    ) -> Result<TablePrimary<(), BoundFieldAnnotation>, BindError> {
        match syntax {
            ast::TablePrimary::Named { name, alias, span } => {
                let source_name = self.parsed.identifier_name(*name);
                let qualifier = alias
                    .map(|alias| self.parsed.identifier_name(alias))
                    .unwrap_or_else(|| source_name.clone());

                if let Some(reference) = self.resolve_cte(&source_name, cte_scopes) {
                    self.bind_cte(reference)?;
                    let (declaration, source_fields) = {
                        let declaration =
                            &self.cte_scopes[reference.scope].declarations[reference.declaration];
                        let CteState::Bound(query) = &declaration.state else {
                            unreachable!("resolved CTE is bound")
                        };
                        (declaration.id, query.result_fields.clone())
                    };
                    let occurrence = self.instantiate_occurrence(qualifier, &source_fields, false);
                    return Ok(TablePrimary::CommonTableExpression {
                        declaration,
                        occurrence,
                        span: *span,
                    });
                }

                let matches = self
                    .catalog
                    .relations()
                    .iter()
                    .filter(|relation| relation.name == source_name)
                    .collect::<Vec<_>>();
                let relation = match matches.as_slice() {
                    [] => {
                        return Err(BindError {
                            kind: BindErrorKind::UnknownRelation(source_name),
                            span: name.span,
                        });
                    }
                    [relation] => *relation,
                    _ => {
                        return Err(BindError {
                            kind: BindErrorKind::AmbiguousRelation(source_name),
                            span: name.span,
                        });
                    }
                };
                Ok(self.bind_catalog_occurrence(relation, qualifier, *span))
            }
            ast::TablePrimary::Derived { query, alias, span } => {
                let query = self.bind_query_expression(query, cte_scopes)?;
                let qualifier = self.parsed.identifier_name(*alias);
                let occurrence =
                    self.instantiate_occurrence(qualifier, &query.result_fields, false);
                Ok(TablePrimary::Derived {
                    query: Box::new(query),
                    occurrence,
                    span: *span,
                })
            }
        }
    }

    fn resolve_cte(&self, name: &Name, cte_scopes: &[usize]) -> Option<CteReference> {
        for scope_id in cte_scopes.iter().rev() {
            if let Some(declaration) = self.cte_scopes[*scope_id]
                .declarations
                .iter()
                .position(|declaration| declaration.name == *name)
            {
                return Some(CteReference {
                    scope: *scope_id,
                    declaration,
                });
            }
        }
        None
    }

    fn bind_catalog_occurrence(
        &mut self,
        relation: &CatalogRelation,
        qualifier: Name,
        span: Span,
    ) -> TablePrimary<(), BoundFieldAnnotation> {
        let fields = relation
            .fields
            .iter()
            .map(|field| Field {
                id: self.fresh_field(),
                name: field.name.clone(),
                annotation: Some(field.descriptor),
            })
            .collect();
        TablePrimary::Catalog {
            binding: relation.binding.clone(),
            occurrence: RelationOccurrence {
                id: self.fresh_occurrence(),
                qualifier,
                fields,
            },
            span,
        }
    }

    fn instantiate_occurrence(
        &mut self,
        qualifier: Name,
        source_fields: &[BoundField],
        preserve_annotations: bool,
    ) -> RelationOccurrence<BoundFieldAnnotation> {
        let fields = source_fields
            .iter()
            .map(|field| Field {
                id: self.fresh_field(),
                name: field.name.clone(),
                annotation: preserve_annotations.then_some(field.annotation).flatten(),
            })
            .collect();
        RelationOccurrence {
            id: self.fresh_occurrence(),
            qualifier,
            fields,
        }
    }

    fn bind_expression(
        &mut self,
        syntax: &ast::Expression,
        sources: &[RelationOccurrence<BoundFieldAnnotation>],
        cte_scopes: &[usize],
    ) -> Result<BoundExpression, BindError> {
        self.bind_expression_with(syntax, sources, None, Some(sources), cte_scopes, false)
    }

    fn bind_order_item(
        &mut self,
        syntax: &ast::OrderItem,
        results: &[BoundField],
        fallback_sources: Option<&[RelationOccurrence<BoundFieldAnnotation>]>,
        window_sources: Option<&[RelationOccurrence<BoundFieldAnnotation>]>,
        cte_scopes: &[usize],
    ) -> Result<OrderItem<(), BoundFieldAnnotation>, BindError> {
        let expression =
            if let ast::Expression::Literal(ast::Literal::Integer(literal)) = &syntax.expression {
                let spelling = self.parsed.integer_spelling(*literal);
                let ordinal = spelling.parse::<usize>().ok();
                let Some(field) = ordinal
                    .filter(|ordinal| *ordinal > 0)
                    .and_then(|ordinal| results.get(ordinal - 1))
                else {
                    return Err(BindError {
                        kind: BindErrorKind::OrderByOrdinalOutOfRange(spelling.into()),
                        span: literal.span,
                    });
                };
                Expression::new(
                    ExpressionKind::Field(field.id),
                    (),
                    syntax.expression.span(),
                )
            } else {
                self.bind_expression_with(
                    &syntax.expression,
                    fallback_sources.unwrap_or(&[]),
                    Some(results),
                    window_sources,
                    cte_scopes,
                    false,
                )?
            };

        Ok(OrderItem {
            expression,
            direction: syntax.direction,
            null_placement: syntax.null_placement,
            span: syntax.span,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn bind_expression_with(
        &mut self,
        syntax: &ast::Expression,
        sources: &[RelationOccurrence<BoundFieldAnnotation>],
        results: Option<&[BoundField]>,
        window_sources: Option<&[RelationOccurrence<BoundFieldAnnotation>]>,
        cte_scopes: &[usize],
        inside_window: bool,
    ) -> Result<BoundExpression, BindError> {
        use ast::Expression as A;

        let span = syntax.span();
        let recurse = |binder: &mut Self, expression: &ast::Expression| {
            binder.bind_expression_with(
                expression,
                sources,
                results,
                window_sources,
                cte_scopes,
                inside_window,
            )
        };

        let kind = match syntax {
            A::Literal(literal) => ExpressionKind::Literal(match literal {
                ast::Literal::Integer(literal) => {
                    LiteralValue::Integer(self.parsed.integer_spelling(*literal).into())
                }
                ast::Literal::Text(literal) => LiteralValue::Text(self.parsed.text_value(*literal)),
                ast::Literal::Boolean { value, .. } => LiteralValue::Boolean(*value),
                ast::Literal::Null { .. } => LiteralValue::Null,
            }),
            A::ColumnReference {
                qualifier, name, ..
            } => {
                let name_value = self.parsed.identifier_name(*name);
                let field = if inside_window {
                    resolve_source_field(self.parsed, qualifier.as_ref(), name, sources)?
                } else if qualifier.is_none() {
                    match results {
                        Some(results) => {
                            let matches = results
                                .iter()
                                .filter(|field| field.name == name_value)
                                .collect::<Vec<_>>();
                            match matches.as_slice() {
                                [field] => field.id,
                                [] => resolve_source_field(
                                    self.parsed,
                                    qualifier.as_ref(),
                                    name,
                                    sources,
                                )?,
                                _ => {
                                    return Err(BindError {
                                        kind: BindErrorKind::AmbiguousColumn(name_value),
                                        span: name.span,
                                    });
                                }
                            }
                        }
                        None => {
                            resolve_source_field(self.parsed, qualifier.as_ref(), name, sources)?
                        }
                    }
                } else {
                    resolve_source_field(self.parsed, qualifier.as_ref(), name, sources)?
                };
                ExpressionKind::Field(field)
            }
            A::Parenthesized { expression, .. } => {
                ExpressionKind::Parenthesized(Box::new(recurse(self, expression)?))
            }
            A::Unary {
                operator,
                expression,
                ..
            } => ExpressionKind::Unary {
                operator: *operator,
                expression: Box::new(recurse(self, expression)?),
            },
            A::Binary {
                left,
                operator,
                right,
                ..
            } => ExpressionKind::Binary {
                left: Box::new(recurse(self, left)?),
                operator: *operator,
                right: Box::new(recurse(self, right)?),
            },
            A::IsNull {
                expression,
                negated,
                ..
            } => ExpressionKind::IsNull {
                expression: Box::new(recurse(self, expression)?),
                negated: *negated,
            },
            A::InList {
                expression,
                negated,
                values,
                ..
            } => ExpressionKind::InList {
                expression: Box::new(recurse(self, expression)?),
                negated: *negated,
                values: values
                    .iter()
                    .map(|value| recurse(self, value))
                    .collect::<Result<_, _>>()?,
            },
            A::InQuery {
                expression,
                negated,
                query,
                ..
            } => ExpressionKind::InQuery {
                expression: Box::new(recurse(self, expression)?),
                negated: *negated,
                query: Box::new(self.bind_query_expression(query, cte_scopes)?),
            },
            A::Case {
                branches,
                else_expression,
                ..
            } => ExpressionKind::Case {
                branches: branches
                    .iter()
                    .map(|branch| {
                        Ok(WhenClause {
                            condition: recurse(self, &branch.condition)?,
                            result: recurse(self, &branch.result)?,
                            span: branch.span,
                        })
                    })
                    .collect::<Result<_, BindError>>()?,
                else_expression: else_expression
                    .as_ref()
                    .map(|expression| recurse(self, expression).map(Box::new))
                    .transpose()?,
            },
            A::Cast {
                expression,
                scalar_type,
                ..
            } => ExpressionKind::Cast {
                expression: Box::new(recurse(self, expression)?),
                scalar_type: *scalar_type,
            },
            A::Exists { query, .. } => ExpressionKind::Exists {
                query: Box::new(self.bind_query_expression(query, cte_scopes)?),
            },
            A::Aggregate {
                function,
                argument,
                window,
                ..
            } => {
                let is_window = window.is_some();
                let bind_window_child = |binder: &mut Self, expression: &ast::Expression| {
                    binder.bind_expression_with(
                        expression,
                        window_sources.unwrap_or(&[]),
                        None,
                        window_sources,
                        cte_scopes,
                        true,
                    )
                };
                let argument = match argument {
                    ast::AggregateArgument::Star(span) => AggregateArgument::Star(*span),
                    ast::AggregateArgument::Expression(expression) => {
                        let expression = if is_window {
                            bind_window_child(self, expression)?
                        } else {
                            recurse(self, expression)?
                        };
                        AggregateArgument::Expression(Box::new(expression))
                    }
                };
                let window = window
                    .as_ref()
                    .map(|window| {
                        Ok(AggregateWindow {
                            partition_by: window
                                .partition_by
                                .iter()
                                .map(|expression| bind_window_child(self, expression))
                                .collect::<Result<_, _>>()?,
                            span: window.span,
                        })
                    })
                    .transpose()?;
                ExpressionKind::Aggregate {
                    function: *function,
                    argument,
                    window,
                }
            }
            A::Ranking {
                function,
                partition_by,
                order_by,
                ..
            } => {
                let bind_window_child = |binder: &mut Self, expression: &ast::Expression| {
                    binder.bind_expression_with(
                        expression,
                        window_sources.unwrap_or(&[]),
                        None,
                        window_sources,
                        cte_scopes,
                        true,
                    )
                };
                ExpressionKind::Ranking {
                    function: *function,
                    partition_by: partition_by
                        .iter()
                        .map(|expression| bind_window_child(self, expression))
                        .collect::<Result<_, _>>()?,
                    order_by: order_by
                        .iter()
                        .map(|item| {
                            Ok(OrderItem {
                                expression: bind_window_child(self, &item.expression)?,
                                direction: item.direction,
                                null_placement: item.null_placement,
                                span: item.span,
                            })
                        })
                        .collect::<Result<_, BindError>>()?,
                }
            }
        };

        Ok(Expression::new(kind, (), span))
    }

    fn fresh_field(&mut self) -> FieldId {
        let id = FieldId::from_index(self.next_field);
        self.next_field = self
            .next_field
            .checked_add(1)
            .expect("field identity space");
        id
    }

    fn fresh_occurrence(&mut self) -> RelationOccurrenceId {
        let id = RelationOccurrenceId::from_index(self.next_occurrence);
        self.next_occurrence = self
            .next_occurrence
            .checked_add(1)
            .expect("relation occurrence identity space");
        id
    }

    fn fresh_cte(&mut self) -> CteId {
        let id = CteId::from_index(self.next_cte);
        self.next_cte = self.next_cte.checked_add(1).expect("CTE identity space");
        id
    }

    fn fresh_result_field(&mut self, name: Name) -> BoundField {
        Field {
            id: self.fresh_field(),
            name,
            annotation: None,
        }
    }
}

fn resolve_source_field(
    parsed: &ParsedProgram,
    qualifier: Option<&ast::Identifier>,
    name: &ast::Identifier,
    sources: &[RelationOccurrence<BoundFieldAnnotation>],
) -> Result<FieldId, BindError> {
    let name_value = parsed.identifier_name(*name);
    if let Some(qualifier) = qualifier {
        let qualifier_value = parsed.identifier_name(*qualifier);
        let matching_sources = sources
            .iter()
            .filter(|source| source.qualifier == qualifier_value)
            .collect::<Vec<_>>();
        let source = match matching_sources.as_slice() {
            [source] => *source,
            _ => {
                return Err(BindError {
                    kind: BindErrorKind::UnknownQualifier(qualifier_value),
                    span: qualifier.span,
                });
            }
        };
        let matching_fields = source
            .fields
            .iter()
            .filter(|field| field.name == name_value)
            .collect::<Vec<_>>();
        return match matching_fields.as_slice() {
            [field] => Ok(field.id),
            [] => Err(BindError {
                kind: BindErrorKind::UnknownColumn(name_value),
                span: name.span,
            }),
            _ => Err(BindError {
                kind: BindErrorKind::AmbiguousColumn(name_value),
                span: name.span,
            }),
        };
    }

    let matching_fields = sources
        .iter()
        .flat_map(|source| source.fields.iter())
        .filter(|field| field.name == name_value)
        .collect::<Vec<_>>();
    match matching_fields.as_slice() {
        [field] => Ok(field.id),
        [] => Err(BindError {
            kind: BindErrorKind::UnknownColumn(name_value),
            span: name.span,
        }),
        _ => Err(BindError {
            kind: BindErrorKind::AmbiguousColumn(name_value),
            span: name.span,
        }),
    }
}

fn expression_field(expression: &BoundExpression) -> Option<FieldId> {
    match &expression.kind {
        ExpressionKind::Field(field) => Some(*field),
        ExpressionKind::Parenthesized(expression) => expression_field(expression),
        _ => None,
    }
}

fn find_field_name(
    sources: &[RelationOccurrence<BoundFieldAnnotation>],
    id: FieldId,
) -> Option<Name> {
    sources
        .iter()
        .flat_map(|source| &source.fields)
        .find(|field| field.id == id)
        .map(|field| field.name.clone())
}

fn select_occurrences(
    select: &SelectQuery<(), BoundFieldAnnotation>,
) -> Vec<RelationOccurrence<BoundFieldAnnotation>> {
    let mut occurrences = Vec::new();
    for joined in &select.from {
        occurrences.push(joined.left.occurrence().clone());
        occurrences.extend(
            joined
                .joins
                .iter()
                .map(|join| join.right.occurrence().clone()),
        );
    }
    occurrences
}

fn contains_grouping_aggregate(expression: &BoundExpression) -> bool {
    use ExpressionKind as K;
    match &expression.kind {
        K::Aggregate { window: None, .. } => true,
        K::Parenthesized(expression)
        | K::Unary { expression, .. }
        | K::IsNull { expression, .. }
        | K::Cast { expression, .. } => contains_grouping_aggregate(expression),
        K::Binary { left, right, .. } => {
            contains_grouping_aggregate(left) || contains_grouping_aggregate(right)
        }
        K::InList {
            expression, values, ..
        } => {
            contains_grouping_aggregate(expression)
                || values.iter().any(contains_grouping_aggregate)
        }
        K::Case {
            branches,
            else_expression,
        } => {
            branches.iter().any(|branch| {
                contains_grouping_aggregate(&branch.condition)
                    || contains_grouping_aggregate(&branch.result)
            }) || else_expression
                .as_deref()
                .is_some_and(contains_grouping_aggregate)
        }
        K::Aggregate {
            argument, window, ..
        } => {
            matches!(argument, AggregateArgument::Expression(expression) if contains_grouping_aggregate(expression))
                || window.as_ref().is_some_and(|window| {
                    window.partition_by.iter().any(contains_grouping_aggregate)
                })
        }
        K::Ranking {
            partition_by,
            order_by,
            ..
        } => {
            partition_by.iter().any(contains_grouping_aggregate)
                || order_by
                    .iter()
                    .any(|item| contains_grouping_aggregate(&item.expression))
        }
        // Nested queries classify themselves independently.
        K::InQuery { expression, .. } => contains_grouping_aggregate(expression),
        K::Literal(_) | K::Field(_) | K::Exists { .. } => false,
    }
}
