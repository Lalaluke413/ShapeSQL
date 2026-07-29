# Core Query Semantics

## 1. Overview

This document defines the relational meaning of the principal ShapeSQL 0.1
query constructs. Surface grammar and exact type-checking rules will be defined
separately.

## 2. Logical processing

For semantic description, a query block is evaluated in this order:

1. relation sources and joins in `FROM`;
2. row selection by `WHERE`;
3. grouping and aggregate evaluation;
4. group selection by `HAVING`;
5. partitioned operation evaluation;
6. projection by `SELECT`;
7. duplicate elimination by `DISTINCT`;
8. set operations;
9. result ordering by `ORDER BY`; and
10. row removal and bounding by `OFFSET` and `LIMIT`.

An implementation need not execute these steps in this physical order. Any
plan is conforming when it preserves the specified observable behavior and
errors.

## 3. Relation sources

A named relation source contributes every row occurrence from the input
relation and makes its fields visible in the query block's binding environment.

A derived table or common table expression is evaluated according to its query
and contributes the resulting relation. Its internal ordering is discarded
unless a surrounding language rule explicitly consumes an ordered result.

## 4. Joins

### 4.1 Cross join

For each occurrence of row `l` in left input `L` and each occurrence of row
`r` in right input `R`, `L CROSS JOIN R` contains one concatenated row
`(l, r)`.

The multiplicity of `(l, r)` is the product of the input multiplicities.

### 4.2 Inner join

An inner join evaluates its `ON` condition for every candidate pair from the
cross join. It emits the concatenated row exactly when the condition is
`TRUE`. It emits no row when the condition is `FALSE` or `UNKNOWN`.

### 4.3 Left outer join

A left outer join first emits every matching pair according to inner-join
semantics. For each occurrence of a left row with no matching right
occurrence, it emits one row consisting of the left row followed by `NULL` for
every right field.

The right-side fields in the output schema are nullable.

### 4.4 Right and full outer joins

A right outer join is the symmetric form of a left outer join.

A full outer join emits every matching pair, one null-extended row for each
unmatched left occurrence, and one null-extended row for each unmatched right
occurrence. Fields from both inputs are nullable in the output schema.

## 5. Filtering

`WHERE p` evaluates `p` once for each input row occurrence:

- if `p` is `TRUE`, the occurrence is preserved;
- if `p` is `FALSE` or `UNKNOWN`, the occurrence is removed.

`HAVING` uses the same rule once per group after aggregate evaluation.

Filtering does not otherwise change row values, schemas, or multiplicities.

## 6. Projection

Projection evaluates each `SELECT` expression in source order for every input
row or group. It emits one result row containing those values in the same
order.

Projection preserves input multiplicity. Two input rows that produce equal
result rows remain duplicate occurrences unless `DISTINCT` applies.

The nullability of a projected field MUST include every case in which its
expression can evaluate to `NULL`.

## 7. Conditional expressions

A searched `CASE` expression evaluates `WHEN` predicates in source order. The
result is the expression associated with the first predicate that is `TRUE`.
Predicates that are `FALSE` or `UNKNOWN` do not match.

If no predicate matches:

- the `ELSE` expression is the result when present; or
- `NULL` is the result when `ELSE` is absent.

Only the selected result expression is evaluated. Errors in unselected result
expressions are not observable.

## 8. Grouping

`GROUP BY e1, ... en` evaluates its grouping expressions for every input row
and partitions the input bag using not-distinct equality.

Each distinct tuple of grouping values identifies one group. `NULL` values
therefore belong to one group for a given grouping position.

Without `GROUP BY`, a query containing an aggregate has one implicit group:

- a non-empty input produces one group containing all rows; and
- an empty input produces one empty group.

With one or more grouping expressions, an empty input produces no groups.

A selected expression in a grouped query MUST be:

- a grouping expression;
- an aggregate expression; or
- composed only from grouping expressions and aggregate expressions.

Otherwise the program has a static error.

## 9. Aggregates

An aggregate consumes the bag of values contributed by one group.

- `COUNT(*)` returns the number of row occurrences in the group.
- `COUNT(e)` returns the number of occurrences for which `e` is not `NULL`.
- `SUM(e)` returns the sum of non-`NULL` values.
- `MIN(e)` returns the least non-`NULL` value.
- `MAX(e)` returns the greatest non-`NULL` value.

`SUM`, `MIN`, and `MAX` ignore `NULL`. They return `NULL` when no non-`NULL`
value is present. `COUNT` returns `0` for an empty input.

`COUNT` returns `INT64`. A count greater than the maximum `INT64` value is an
execution error. `SUM` over `INT64` is an execution error when its exact result
is outside the `INT64` range.

## 10. Duplicate elimination

`DISTINCT` partitions the input bag using row not-distinct equality and emits
one occurrence from each partition.

Duplicate elimination does not imply an output order.

## 11. Set operations

For a row `r`, let `mR(r)` and `mT(r)` be its multiplicities in inputs `R` and
`T`, using row not-distinct equality.

| Operation | Result multiplicity of `r` |
| --- | --- |
| `R UNION ALL T` | `mR(r) + mT(r)` |
| `R UNION T` | `1` if either multiplicity is nonzero, otherwise `0` |
| `R INTERSECT ALL T` | `min(mR(r), mT(r))` |
| `R INTERSECT T` | `1` if both multiplicities are nonzero, otherwise `0` |
| `R EXCEPT ALL T` | `max(mR(r) - mT(r), 0)` |
| `R EXCEPT T` | `1` if `mR(r)` is nonzero and `mT(r)` is zero, otherwise `0` |

Set operations compare fields by ordinal position. Output field names come
from the left input. Output field types and nullability are the common types
determined during static typing.

## 12. Relational predicates

### 12.1 `EXISTS`

`EXISTS (Q)` is `TRUE` when `Q` produces at least one row and `FALSE` when it
produces no rows. It never produces `UNKNOWN`.

`NOT EXISTS (Q)` is the boolean negation of `EXISTS (Q)`.

### 12.2 `IN`

For scalar value `x` and finite candidate bag `C`, `x IN C` is:

1. `TRUE` if `x = c` is `TRUE` for any candidate `c`;
2. otherwise `UNKNOWN` if `x = c` is `UNKNOWN` for any candidate `c`;
3. otherwise `FALSE`.

For an empty candidate bag, `IN` is `FALSE`, including when `x` is `NULL`.

`x NOT IN C` is `NOT (x IN C)` under three-valued logic.

A query used as the right operand of `IN` MUST have exactly one result field.

## 13. Partitioned aggregates

`PARTITION BY` divides rows using not-distinct equality of the partition
expression tuple. With no partition expressions, all rows form one partition.

A partitioned aggregate computes the corresponding aggregate over the entire
partition and appends or projects that value for every row occurrence in the
partition. It does not reduce the number of rows.

For example:

```sql
SELECT
    department,
    employee,
    MAX(salary) OVER (PARTITION BY department) AS department_max
FROM employees;
```

Each employee row is retained and receives the maximum non-`NULL` salary in
its department.

## 14. Ranking

Ranking functions operate independently within each partition after sorting by
their window ordering expressions.

- `ROW_NUMBER()` assigns consecutive `INT64` values starting at `1`.
- `RANK()` assigns peers the position of the first peer and leaves gaps after
  peer groups.
- `DENSE_RANK()` assigns peers the same value and increments by one for the
  next peer group.

Rows are peers when all window ordering expressions compare equally according
to the ordering rules.

For `ROW_NUMBER`, the ordering expressions include every visible input field.
Peer occurrences are therefore permitted only when their complete input rows
are not distinct; assigning consecutive numbers among such indistinguishable
occurrences does not change the result bag.

`RANK` and `DENSE_RANK` MAY contain peers with otherwise different field
values. Every row in a peer group receives the same rank, so their result does
not depend on their internal sequence.

## 15. Ordering, offset, and limit

`ORDER BY` sorts result rows lexicographically by its ordering items. Each item
specifies ascending or descending direction and, for nullable expressions,
whether `NULL` sorts before or after non-`NULL` values.

`OFFSET n` removes the first `n` rows from the ordered result. If fewer than
`n` rows exist, the result is empty.

`LIMIT n` retains at most the first `n` remaining rows.

When both are present, `OFFSET` applies before `LIMIT`.

Using `LIMIT` or `OFFSET` without an outermost `ORDER BY` is a static error
because a bag has no first row.

## 16. Worked semantic example

Given:

| `users.id` | `users.name` | `users.active` |
| ---: | --- | --- |
| `1` | `Ada` | `TRUE` |
| `2` | `Grace` | `FALSE` |
| `3` | `Linus` | `NULL` |

the query:

```sql
SELECT name
FROM users
WHERE active = TRUE;
```

filters out the rows whose predicate is `FALSE` or `UNKNOWN`, then projects the
`name` field. Its result bag contains exactly:

| `name` |
| --- |
| `Ada` |

No row order is specified because the query has no `ORDER BY`.
