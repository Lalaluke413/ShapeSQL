# Grammar

## 1. Overview

This document defines the concrete grammar of a ShapeSQL 0.1 program after
tokenization according to [Lexical structure](04-lexical-structure.md).

A token sequence is syntactically valid only if the entire sequence matches
the `program` production. A token sequence that does not match is a syntactic
error.

## 2. Notation

The grammar uses the following extended Backus-Naur form:

| Form | Meaning |
| --- | --- |
| `"SELECT"` | The keyword or symbolic token between the quotes. |
| `name` | A production or lexical token class. |
| `a, b` | `a` followed by `b`. |
| `a \| b` | Either `a` or `b`. |
| `[a]` | Zero or one occurrence of `a`. |
| `{a}` | Zero or more occurrences of `a`. |
| `(a)` | Grouping within the grammar. |

Keyword terminals are written in uppercase for readability, but keyword
matching is case-insensitive. `identifier`, `integer_literal`, and
`text_literal` are token classes defined by the lexical structure.
`end_of_input` is the position after the final token.

## 3. Complete programs

```ebnf
program =
    query_expression, [ ";" ], end_of_input ;

query_expression =
    [ with_clause ],
    query_body,
    [ order_by_clause, [ row_bound ] ] ;

with_clause =
    "WITH", common_table_expression,
    { ",", common_table_expression } ;

common_table_expression =
    identifier, "AS", "(", query_expression, ")" ;
```

A source text contains exactly one query. The terminating semicolon is
optional. An empty source text, a second query, or more than one terminating
semicolon is a syntactic error.

`WITH` applies to the complete `query_body` that follows it. Common table
expression column-name lists are not part of ShapeSQL 0.1. Whether a common
table expression refers to itself or another common table expression in a
cycle is determined during binding, not parsing.

## 4. Query expressions and set operations

```ebnf
query_body =
    union_or_except_expression ;

union_or_except_expression =
    intersect_expression,
    {
        ( "UNION" | "EXCEPT" ), [ "ALL" ],
        intersect_expression
    } ;

intersect_expression =
    query_primary,
    {
        "INTERSECT", [ "ALL" ],
        query_primary
    } ;

query_primary =
      select_query
    | "(", query_expression, ")" ;
```

`INTERSECT` has higher precedence than `UNION` and `EXCEPT`. `UNION` and
`EXCEPT` have equal precedence. Repeated operators at the same precedence
associate from left to right. Parentheses override these rules.

The absence of `ALL` requests duplicate elimination. ShapeSQL 0.1 does not
accept an explicit `DISTINCT` after a set operator.

An `ORDER BY` and any row bound attach to the complete `query_body` immediately
before them. Ordering or bounding one operand of a set operation therefore
requires parentheses.

## 5. Select queries

```ebnf
select_query =
    "SELECT", [ "DISTINCT" ], select_list,
    "FROM", from_clause,
    [ "WHERE", expression ],
    [ "GROUP", "BY", expression_list ],
    [ "HAVING", expression ] ;

select_list =
    select_item, { ",", select_item } ;

select_item =
      "*"
    | identifier, ".", "*"
    | expression, [ select_alias ] ;

select_alias =
    [ "AS" ], identifier ;

expression_list =
    expression, { ",", expression } ;
```

`FROM` is required. ShapeSQL 0.1 has no implicit one-row relation, so
`SELECT 1` is not a program.

An unqualified wildcard selects every field visible to the query block. A
qualified wildcard names a relation source or source alias. Wildcard expansion
and alias visibility are binding rules.

`AS` is optional for a select-item alias. Because every ShapeSQL keyword is
reserved, omitting `AS` does not permit a clause keyword to be consumed as an
alias.

## 6. Relation sources and joins

```ebnf
from_clause =
    joined_table, { ",", joined_table } ;

joined_table =
    table_primary, { join_tail } ;

join_tail =
      "CROSS", "JOIN", table_primary
    | [ "INNER" ], "JOIN", table_primary, "ON", expression
    | "LEFT", [ "OUTER" ], "JOIN",
      table_primary, "ON", expression
    | "RIGHT", [ "OUTER" ], "JOIN",
      table_primary, "ON", expression
    | "FULL", [ "OUTER" ], "JOIN",
      table_primary, "ON", expression ;

table_primary =
      named_source
    | derived_source ;

named_source =
    identifier, [ source_alias ] ;

derived_source =
    "(", query_expression, ")", source_alias ;

source_alias =
    [ "AS" ], identifier ;
```

Explicit joins associate from left to right. They bind more tightly than the
comma in `from_clause`; each comma therefore combines two completed
`joined_table` operands as a cross join.

`JOIN` without a preceding join type means `INNER JOIN`. `CROSS JOIN` does not
accept an `ON` clause. Every other join form requires one.

A derived source requires an alias. A named source may have an alias. `AS` is
optional in both forms.

ShapeSQL relation names contain one identifier. Catalog, schema, and other
multi-part relation names are host concerns and are not part of portable
ShapeSQL 0.1.

## 7. Result ordering and bounds

```ebnf
order_by_clause =
    "ORDER", "BY", order_item, { ",", order_item } ;

order_item =
    expression,
    [ "ASC" | "DESC" ],
    [ "NULLS", ( "FIRST" | "LAST" ) ] ;

row_bound =
      limit_clause, [ offset_clause ]
    | offset_clause, [ limit_clause ] ;

limit_clause =
    "LIMIT", integer_literal ;

offset_clause =
    "OFFSET", integer_literal ;
```

`LIMIT` and `OFFSET` may appear in either order, but each may appear at most
once. They are syntactically permitted only after `ORDER BY`. Because a sign is
a separate token, these productions accept only unsigned integer literals.
Range checking occurs during typing.

Whether an ordering expression is nullable, whether it therefore requires an
explicit null placement, and how an integer literal is interpreted as an
ordinal are static semantic rules rather than grammar rules.

## 8. Scalar expressions

The expression grammar, from lowest to highest precedence, is:

```ebnf
expression =
    or_expression ;

or_expression =
    and_expression, { "OR", and_expression } ;

and_expression =
    not_expression, { "AND", not_expression } ;

not_expression =
      "NOT", not_expression
    | predicate_expression ;

predicate_expression =
    concatenation_expression,
    [
          comparison_operator, concatenation_expression
        | "IS", [ "NOT" ], "NULL"
        | [ "NOT" ], "IN", "(", in_contents, ")"
    ] ;

comparison_operator =
      "=" | "<>" | "<" | "<=" | ">" | ">=" ;

in_contents =
      query_expression
    | expression_list ;

concatenation_expression =
    additive_expression, { "||", additive_expression } ;

additive_expression =
    multiplicative_expression,
    { ( "+" | "-" ), multiplicative_expression } ;

multiplicative_expression =
    unary_expression,
    { ( "*" | "/" | "%" ), unary_expression } ;

unary_expression =
      ( "+" | "-" ), unary_expression
    | primary_expression ;
```

`OR`, `AND`, concatenation, addition, subtraction, multiplication, division,
and remainder associate from left to right. Unary operators and `NOT`
associate from right to left.

A comparison, null test, or membership test may occur at most once without
parenthesized subexpressions. For example, `a < b < c` is a syntactic error,
while `(a < b) = (b < c)` is syntactically valid and is checked by the type
system.

An `IN` list is nonempty. The query form is distinguished from the expression
list form by parsing the contents as a `query_expression`. The query must
produce exactly one field, and it must be uncorrelated; those are typing and
binding requirements, respectively.

## 9. Primary expressions

```ebnf
primary_expression =
      literal
    | column_reference
    | "(", expression, ")"
    | case_expression
    | cast_expression
    | exists_expression
    | aggregate_expression
    | ranking_expression ;

literal =
      integer_literal
    | text_literal
    | "TRUE"
    | "FALSE"
    | "NULL" ;

column_reference =
    identifier, [ ".", identifier ] ;

case_expression =
    "CASE",
    when_clause, { when_clause },
    [ "ELSE", expression ],
    "END" ;

when_clause =
    "WHEN", expression, "THEN", expression ;

cast_expression =
    "CAST", "(", expression, "AS", scalar_type, ")" ;

scalar_type =
      "BOOLEAN"
    | "INT64"
    | "TEXT" ;

exists_expression =
    "EXISTS", "(", query_expression, ")" ;
```

Only searched `CASE` is accepted. A `CASE` contains at least one `WHEN` clause.
Simple `CASE x WHEN ...`, scalar subqueries, and row constructors do not match
the grammar.

A qualified column reference contains exactly a source name or alias and a
field name. The meaning of unqualified and qualified references is determined
during binding.

## 10. Aggregate and partitioned expressions

```ebnf
aggregate_expression =
    aggregate_invocation, [ aggregate_window ] ;

aggregate_invocation =
      "COUNT", "(", ( "*" | expression ), ")"
    | "SUM", "(", expression, ")"
    | "MIN", "(", expression, ")"
    | "MAX", "(", expression, ")" ;

aggregate_window =
    "OVER", "(",
    [ "PARTITION", "BY", expression_list ],
    ")" ;

ranking_expression =
    ranking_function, "(", ")",
    "OVER", "(",
    [ "PARTITION", "BY", expression_list ],
    window_order_by_clause,
    ")" ;

ranking_function =
      "ROW_NUMBER"
    | "RANK"
    | "DENSE_RANK" ;

window_order_by_clause =
    "ORDER", "BY",
    order_item, { ",", order_item } ;
```

An aggregate without `OVER` is a grouping aggregate. An aggregate with `OVER`
is evaluated over its entire partition. The grammar does not admit an ordering
or frame in an aggregate window.

A ranking expression always has `OVER` and a nonempty window `ORDER BY`.
Named windows, window frames, and a window `ORDER BY` on a partitioned
aggregate do not match the grammar.

The placement and nesting of aggregate and partitioned expressions, the
required completeness of `ROW_NUMBER` ordering, and their operand types are
static semantic rules.

## 11. Reserved keywords

Every keyword used by the grammar is reserved:

```text
ALL AND AS ASC
BOOLEAN BY
CASE CAST COUNT CROSS
DENSE_RANK DESC DISTINCT
ELSE END EXCEPT EXISTS
FALSE FIRST FROM FULL
GROUP
HAVING
IN INNER INT64 INTERSECT IS
JOIN
LAST LEFT LIMIT
MAX MIN
NOT NULL NULLS
OFFSET ON OR ORDER OUTER OVER
PARTITION
RANK RIGHT ROW_NUMBER
SELECT SUM
TEXT THEN TRUE
UNION
WHEN WHERE WITH
```

A reserved keyword cannot be a regular identifier. It may be used as a
delimited identifier.

Words associated only with excluded features, such as `INSERT`, `VALUES`,
`UPDATE`, `DELETE`, `AVG`, `BOOL_AND`, and `BOOL_OR`, are not ShapeSQL 0.1
keywords. Their use does not introduce those features; a token sequence must
still match the complete grammar above.

## 12. Grammar and later analysis

The grammar determines only whether tokens have a permitted structure. In
particular, it does not determine:

- whether relation, common-table-expression, alias, or field names resolve;
- whether a subquery is correlated;
- whether a common-table-expression dependency graph is acyclic;
- whether an operator's operands have permitted types;
- whether set-operation fields have compatible arity and types;
- whether grouping, aggregate, or partitioned expressions appear in permitted
  environments;
- whether an `IN` query has exactly one result field; or
- whether ordering and bounds satisfy their static semantic requirements.

Violations of those rules are binding or typing errors as assigned by their
normative documents, not syntactic errors.
