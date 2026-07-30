# Types and Type Checking

## 1. Overview

This document defines static typing for ShapeSQL 0.1. Type checking operates
on a successfully bound program and determines:

- one scalar type and one nullability property for every scalar expression;
- the ordered typed schema of every query expression;
- whether operator, aggregate, set-operation, and ordering requirements are
  satisfied; and
- whether aggregate and partitioned expressions occur in permitted contexts.

Successful type checking produces a fully typed program suitable for lowering
to Shape IR. A violation of a rule in this document is a `typing` error unless
the rule explicitly assigns another phase.

## 2. Type descriptors

A scalar expression has a type descriptor `T` or `T?`, where `T` is one of:

- `BOOLEAN`;
- `INT64`; or
- `TEXT`.

`T` is non-nullable. `T?` is nullable and may produce either a value of `T` or
`NULL`.

Nullability is part of the static schema but not a separate scalar type.
Except for the contextual `NULL` rule in Section 4, two expressions have the
same scalar type only when their scalar type names are equal. Nullability does
not affect scalar type equality.

Type checking MUST assign a type descriptor to every expression, including
expressions whose value is unused by the final projection.

## 3. No implicit conversion

ShapeSQL 0.1 performs no implicit conversion between distinct scalar types.
An operator that requires two operands of the same scalar type rejects
`INT64` and `TEXT`, for example, even when the text value could be parsed as an
integer.

The following do not perform implicit conversion:

- arithmetic, comparison, logical, and concatenation operators;
- `CASE` result expressions;
- `IN` operands;
- aggregate arguments;
- set-operation fields; and
- ordering expressions.

An author MUST use `CAST` when Section 8 permits the intended conversion.

## 4. Literals

### 4.1 Boolean and text literals

`TRUE` and `FALSE` have type `BOOLEAN`.

A text literal has type `TEXT`. Its value is the decoded Unicode string
defined by the lexical structure.

These literals are non-nullable.

### 4.2 Integer literals

An integer literal denotes an `INT64` value and is non-nullable. Its magnitude
MUST be no greater than `2^63 - 1`, except for the special case below.

The token sequence `-9223372036854775808` denotes the minimum `INT64` value.
This exception applies only when the integer token `9223372036854775808` is
the immediate operand of unary `-`. The token by itself, its use with unary
`+`, and any greater magnitude are typing errors.

Range checking an integer literal is static. Arithmetic overflow after
successful typing remains an evaluation error, including overflow caused by
negating the minimum `INT64` value.

### 4.3 `NULL`

`NULL` is a contextual literal, not a fourth scalar type. It is nullable and
receives its scalar type from a context that requires one particular scalar
type.

Contexts that can determine the type of `NULL` include:

- the other operand of an operator that requires equal scalar types;
- the other result expressions of a `CASE`;
- the left operand or other candidates of `IN`;
- the corresponding field of a set operation; and
- the target type of `CAST`.

Constraints from the complete bound expression or query are considered
together. Every `NULL` literal MUST resolve to exactly one scalar type.
An unconstrained `NULL`, including an uncast result expression consisting only
of `NULL`, is a typing error.

Type constraints may flow between corresponding operands of one set operation
and between the left operand and query result of one `IN` predicate. A use of
a common table expression or derived-source field does not retroactively
constrain the query that produced that field. Each such source MUST therefore
have a complete typed result schema before its containing query is checked.

Examples:

```sql
SELECT CASE WHEN flag THEN 1 ELSE NULL END
FROM items;
```

The `NULL` has type `INT64?`.

```sql
SELECT CAST(NULL AS BOOLEAN)
FROM items;
```

The `NULL` and the cast result have type `BOOLEAN?`.

```sql
SELECT NULL
FROM items;
```

This program has a typing error because no scalar type constrains `NULL`.

## 5. Column references and parentheses

A bound column reference has the scalar type and nullability of its referenced
field as visible at that point in the query.

Parentheses do not change an expression's type descriptor.

Outer joins adjust the visible field nullability as defined by
[Core query semantics](03-query-semantics.md#4-joins). A reference to a field
on a null-extended side therefore uses the nullable form of its scalar type
even when the source field was non-nullable.

## 6. Operator signatures

### 6.1 Arithmetic

Unary and binary arithmetic have these signatures:

| Expression | Operand types | Result |
| --- | --- | --- |
| `+x`, `-x` | `INT64` | `INT64` |
| `x + y`, `x - y`, `x * y` | `INT64`, `INT64` | `INT64` |
| `x / y`, `x % y` | `INT64`, `INT64` | `INT64` |

The result is nullable if any operand is nullable. These operators are strict.

Division by zero, remainder by zero, and any result outside the `INT64` range
are evaluation errors. These remain evaluation errors when constant operands
make the failure statically provable.

`INT64` division truncates the exact quotient toward zero. For a nonzero
divisor, remainder is defined by:

```text
x = (x / y) * y + (x % y)
```

and has the same sign as `x` when it is nonzero.

### 6.2 Text concatenation

`x || y` requires two `TEXT` operands and returns `TEXT`. Its result is
nullable if either operand is nullable. Concatenation is strict and joins the
two strings without a separator.

### 6.3 Comparison

`=`, `<>`, `<`, `<=`, `>`, and `>=` require both operands to have the same
scalar type. All three ShapeSQL 0.1 scalar types support all six comparison
operators.

Comparison returns `BOOLEAN`. It is nullable if either operand is nullable;
a null comparison is interpreted as `UNKNOWN` as defined by the data model.

Non-null values use these total orders:

- `BOOLEAN`: `FALSE` precedes `TRUE`;
- `INT64`: signed numeric order; and
- `TEXT`: the bytewise order defined by
  [Data model](02-data-model.md#31-text-comparison).

`<>`, `<=`, `>`, and `>=` have their ordinary meanings derived from equality
and the applicable total order.

### 6.4 Logical operators

`NOT`, `AND`, and `OR` require `BOOLEAN` operands and return `BOOLEAN`.
`NOT p` is nullable exactly when `p` is nullable. `p AND q` and `p OR q` are
nullable when either operand is nullable.

Their values follow the three-valued truth tables in the data model. Their
typing and nullability do not imply conditional evaluation.

### 6.5 Null tests

`x IS NULL` and `x IS NOT NULL` accept an operand of any scalar type and
return non-nullable `BOOLEAN`.

A `NULL` literal used only as the operand of a null test remains
unconstrained. An author must provide a scalar type, for example:

```sql
CAST(NULL AS INT64) IS NULL
```

## 7. Conditional expressions

Every `WHEN` predicate of a searched `CASE` MUST have type `BOOLEAN`.

Every `THEN` expression and the `ELSE` expression, when present, MUST have the
same scalar type after contextual `NULL` resolution. No conversion is
performed to find a common type.

The scalar type of the `CASE` result is that shared type. Its result is
nullable when:

- any `THEN` result is nullable;
- the `ELSE` result is nullable; or
- no `ELSE` clause is present.

The predicates' nullability does not by itself make the result nullable.
Conditional evaluation behavior is defined by
[Core query semantics](03-query-semantics.md#7-conditional-expressions).

## 8. Casts

`CAST(x AS T)` has scalar type `T`. It is nullable exactly when `x` is
nullable. A null input produces `NULL` without applying a non-null conversion.

Portable ShapeSQL 0.1 permits:

| Source | Target | Non-null conversion |
| --- | --- | --- |
| `BOOLEAN` | `BOOLEAN` | Identity. |
| `INT64` | `INT64` | Identity. |
| `TEXT` | `TEXT` | Identity. |
| `INT64` | `TEXT` | Base-ten spelling with a leading `-` only for negative values. |
| `TEXT` | `INT64` | Parse the complete string using Section 8.1. |
| `BOOLEAN` | `TEXT` | `TRUE` or `FALSE`. |
| `TEXT` | `BOOLEAN` | Parse the complete string using Section 8.2. |

Every other source-target pair is a typing error.

### 8.1 `TEXT` to `INT64`

The input MUST consist of an optional leading `+` or `-` followed by one or
more ASCII decimal digits. No whitespace or other character is permitted.
The represented value MUST be within the `INT64` range.

An invalid spelling or out-of-range value is an evaluation error.

### 8.2 `TEXT` to `BOOLEAN`

The input MUST equal `TRUE` or `FALSE` under ASCII case-insensitive
comparison. No whitespace or other character is permitted.

An invalid spelling is an evaluation error.

## 9. Predicate contexts

The expressions in these positions MUST have type `BOOLEAN`:

- `ON`;
- `WHERE`;
- `HAVING`; and
- each searched `CASE` `WHEN` clause.

They MAY be nullable. `FALSE` and `UNKNOWN` have the filtering behavior
defined by the applicable query construct.

`EXISTS (Q)` and `NOT EXISTS (Q)` have type non-nullable `BOOLEAN`.
The result arity and field types of `Q` do not otherwise affect the type of
`EXISTS`.

## 10. Membership predicates

For `x IN (c1, ... cn)` and `x NOT IN (c1, ... cn)`, `x` and every candidate
`ci` MUST have the same scalar type after contextual `NULL` resolution.

For `x IN (Q)` and `x NOT IN (Q)`:

- `Q` MUST have exactly one result field; and
- `x` and that field MUST have the same scalar type after contextual `NULL`
  resolution.

The one-field requirement is a typing rule. The prohibition on correlation is
a binding rule.

An `IN` or `NOT IN` result has type `BOOLEAN`. It is nullable when the left
operand is nullable or any list candidate or query result field is nullable.
Otherwise it is non-nullable.

## 11. Grouping and aggregate expressions

### 11.1 Aggregate query classification

A `select_query` is an **aggregate query** when it has a `GROUP BY` clause or
contains a grouping aggregate in its `SELECT` list or `HAVING` expression.
A grouping aggregate is an `aggregate_expression` without `OVER`.

An aggregate query without `GROUP BY` has the one implicit group defined by
the query semantics.

A `HAVING` clause is permitted only in an aggregate query. `HAVING` without
`GROUP BY` or a grouping aggregate is a typing error.

### 11.2 Group-valid expressions

Within an aggregate query, each column reference outside a grouping aggregate
MUST occur as part of an expression that is structurally equal to a
`GROUP BY` expression. This requirement applies to:

- the `SELECT` list;
- `HAVING`; and
- partitioned expressions evaluated by the query; and
- an outer `ORDER BY` reference that resolves against the aggregate query's
  source namespace rather than its result namespace.

Structural equality ignores redundant parentheses but does not use algebraic
equivalence. For example, grouping by `a + b` permits `a + b` but does not
permit `b + a`, `a`, or `b`.

An expression composed from group-valid expressions and grouping aggregates
is group-valid. An expression containing no column reference and no prohibited
aggregate or ranking expression is group-valid. A violation is a typing error.

Each `GROUP BY` expression MUST be scalar and MUST NOT contain a grouping
aggregate, partitioned aggregate, or ranking expression.

### 11.3 Placement and nesting

A grouping aggregate MAY occur only in the `SELECT` list or `HAVING`.

A partitioned aggregate or ranking expression MAY occur only in the `SELECT`
list or in an `ORDER BY` attached directly to one unparenthesized
`select_query`.

An aggregate or ranking expression MUST NOT occur within:

- another aggregate or ranking expression;
- `ON`;
- `WHERE`;
- `GROUP BY`; or
- a scalar expression inside an `aggregate_window` or ranking `OVER` clause.

`HAVING` MUST NOT contain a partitioned aggregate or ranking expression.

These restrictions apply through parentheses, `CASE`, `CAST`, and every
scalar operator. An aggregate in a nested query belongs to that nested query,
not to the expression context containing the query.

### 11.4 Aggregate signatures

Grouping and partitioned aggregates have the same signatures:

| Invocation | Argument requirement | Result |
| --- | --- | --- |
| `COUNT(*)` | None | non-nullable `INT64` |
| `COUNT(e)` | any scalar type | non-nullable `INT64` |
| `SUM(e)` | `INT64` | nullable `INT64` |
| `MIN(e)` | any scalar type | nullable argument scalar type |
| `MAX(e)` | any scalar type | nullable argument scalar type |
| `BOOL_AND(e)` | `BOOLEAN` | nullable `BOOLEAN` |
| `BOOL_OR(e)` | `BOOLEAN` | nullable `BOOLEAN` |

The nullable declarations for aggregates other than `COUNT` are required even
when a particular query could prove that every group or partition contains a
non-null argument.

`COUNT` and `SUM` overflow are evaluation errors as defined by the query
semantics.

### 11.5 Aggregate windows

Every expression in `PARTITION BY` MUST have a resolved scalar type.
All scalar types are permitted. A nullable partition expression is permitted
and uses not-distinct equality.

The aggregate result type is not affected by the number or types of partition
expressions.

## 12. Ranking expressions

`ROW_NUMBER()`, `RANK()`, and `DENSE_RANK()` return non-nullable `INT64`.

Every `PARTITION BY` and window `ORDER BY` expression MUST have a resolved
scalar type. All scalar types are permitted. A nullable window ordering
expression MUST specify `NULLS FIRST` or `NULLS LAST`.

The additional completeness requirement for `ROW_NUMBER` ordering is a typing
rule. After binding, its window `ORDER BY` list MUST contain a direct column
reference to every field visible at the window-evaluation stage. Surrounding
parentheses and ordering direction do not prevent a direct reference from
satisfying this requirement. A reference inside another scalar expression
does not satisfy it.

Each required field identity must occur at least once. Additional ordering
expressions are permitted.

## 13. Projection and query schemas

Each explicit select expression produces a field with that expression's scalar
type and nullability. Wildcard-expanded fields use the type descriptors of
their bound source fields.

Display names and field identities are determined by binding. Type checking
completes the result schema by attaching the type descriptor of each field.

`DISTINCT` does not change field types or nullability. Every scalar type
supports not-distinct equality and is therefore valid in a `DISTINCT` result.

## 14. Set operations

The left and right inputs of `UNION`, `INTERSECT`, or `EXCEPT` MUST have the
same number of fields.

Corresponding fields MUST have the same scalar type. Nullability need not
match. The output field has:

- the scalar type shared by the corresponding inputs; and
- nullable status when either input field is nullable.

No set operation performs implicit conversion. Arity or scalar-type mismatch
is a typing error.

The output field identities and display names come from binding as defined by
[Binding](07-binding.md#9-result-schemas-and-display-names).

## 15. Ordering and row bounds

Every scalar type is orderable using the total orders in Section 6.3.

An `ORDER BY` expression MUST have a resolved scalar type. If it is nullable,
its order item MUST specify `NULLS FIRST` or `NULLS LAST`. An explicit null
placement is permitted for a non-nullable expression and has no effect unless
the expression produces `NULL`, which a conforming evaluation cannot do.

A `LIMIT` or `OFFSET` integer literal MUST be no greater than `2^63 - 1`.
The grammar already excludes a sign. A larger bound is a typing error.

## 16. Nullability summary

Unless a more specific rule in this document applies:

- a strict operator is nullable when any operand is nullable;
- a non-strict expression uses its explicit nullability rule;
- outer-join null extension makes every field on the extended side nullable;
- projection uses expression nullability;
- a set-operation field is nullable when either input field is nullable; and
- `DISTINCT`, filtering, grouping keys, ordering, `LIMIT`, and `OFFSET` do not
  change result-field nullability.

Type checking MUST use these rules exactly. It MUST NOT declare a required
nullable result field non-nullable based on data statistics, constraints not
represented by the portable input schema, constant folding, or a
query-specific proof.

## 17. Static and evaluation errors

Typing errors include:

- an unresolved or ambiguous contextual `NULL` type;
- an out-of-range integer literal or row bound;
- an operator applied to an unsupported operand type;
- distinct scalar types where a rule requires the same type;
- a predicate context whose expression is not `BOOLEAN`;
- an unsupported cast source-target pair;
- incompatible `CASE` result types;
- an `IN` query with arity other than one;
- an aggregate with an unsupported argument type;
- an aggregate, partitioned aggregate, or ranking expression in a prohibited
  position or nesting;
- a `HAVING` clause on a non-aggregate query;
- an expression that is not valid for its aggregate query;
- incompatible set-operation arity or field types;
- a nullable ordering expression without explicit null placement; or
- incomplete `ROW_NUMBER` ordering.

Evaluation errors include:

- arithmetic overflow;
- division or remainder by zero;
- aggregate result overflow; and
- failure of an otherwise permitted text cast.

Constant operands do not move an evaluation error into the typing phase.
Implementations MAY diagnose such an error before executing the relational
plan, but MUST classify it as `evaluation`.
