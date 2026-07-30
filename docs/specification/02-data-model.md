# Data Model

## 1. Overview

ShapeSQL operates on finite, flat, typed relations. Its logical data model is
independent of physical row layout, column layout, storage format, batching,
compression, and transport.

## 2. Schemas and fields

A schema is an ordered sequence of zero or more fields. Each field has:

- an identity unique within the schema;
- a display name, which MAY be empty;
- one scalar type; and
- a nullability property.

Field identity, rather than display name or ordinal position, is used inside
typed IR. This permits a planner to distinguish same-named fields introduced
by a join.

SQL source resolves fields by name according to
[binding rules](07-binding.md). An unqualified reference that matches more
than one visible field is a binding error. A result schema MAY contain
duplicate display names.

The order of fields in a schema is observable and MUST be preserved unless a
query construct explicitly produces a different schema.

## 3. Core scalar types

The portable ShapeSQL 0.1 core contains:

| Type | Values |
| --- | --- |
| `BOOLEAN` | `TRUE` or `FALSE` |
| `INT64` | signed integers from −2^63 through 2^63−1 |
| `TEXT` | finite Unicode strings encoded as UTF-8 |

`NULL` is not a scalar type. A nullable field of type `T` contains either a
value of `T` or `NULL`.

Future versions may add exact decimal, binary, date, timestamp, interval, and
other scalar types without changing the flat relational model. Such types are
not portable ShapeSQL 0.1 until specified here.

### 3.1 Text comparison

Portable `TEXT` equality and ordering compare the unsigned bytes of the
canonical UTF-8 encoding lexicographically. ShapeSQL does not perform Unicode
normalization or locale-sensitive collation.

An implementation MAY provide other collations in extension mode, but MUST NOT
use one implicitly for a portable ShapeSQL 0.1 program.

## 4. Rows

A row conforms to exactly one schema and contains one scalar value or `NULL`
for each field in that schema.

A non-`NULL` value MUST have the field's scalar type. `NULL` is valid only when
the field is nullable.

Rows have no observable identity independent of their values. Two rows are
duplicates when every corresponding field compares as not distinct under
Section 8.

## 5. Relations use bag semantics

A relation is a finite bag of rows conforming to one schema. Duplicate rows are
preserved unless an operation explicitly removes or combines them.

Consequently:

- projection preserves the multiplicity of each input row;
- filtering either preserves or removes each input occurrence;
- `UNION ALL` adds multiplicities; and
- `DISTINCT` reduces each duplicate class to one row.

No relation has an inherent row order.

## 6. Ordered results

Ordering is metadata attached to the outermost query result, not a property of
the logical relation itself.

`ORDER BY` produces a sequence of the rows in the result bag. Every executor
MUST emit that sequence in the specified order. If two rows compare equally on
all ordering expressions, their relative order is unspecified.

Operators and IR nodes MUST NOT rely on input order unless their contract
explicitly accepts an ordered input.

## 7. `NULL` and three-valued logic

`NULL` represents a missing scalar value. Ordinary comparisons with `NULL`
produce the logical result `UNKNOWN`, including `NULL = NULL`.

Logical expressions use these truth tables:

### 7.1 `NOT`

| `p` | `NOT p` |
| --- | --- |
| `TRUE` | `FALSE` |
| `FALSE` | `TRUE` |
| `UNKNOWN` | `UNKNOWN` |

### 7.2 `AND`

| `p AND q` | `TRUE` | `FALSE` | `UNKNOWN` |
| --- | --- | --- | --- |
| `TRUE` | `TRUE` | `FALSE` | `UNKNOWN` |
| `FALSE` | `FALSE` | `FALSE` | `FALSE` |
| `UNKNOWN` | `UNKNOWN` | `FALSE` | `UNKNOWN` |

### 7.3 `OR`

| `p OR q` | `TRUE` | `FALSE` | `UNKNOWN` |
| --- | --- | --- | --- |
| `TRUE` | `TRUE` | `TRUE` | `TRUE` |
| `FALSE` | `TRUE` | `FALSE` | `UNKNOWN` |
| `UNKNOWN` | `TRUE` | `UNKNOWN` | `UNKNOWN` |

`IS NULL` and `IS NOT NULL` always produce `TRUE` or `FALSE`.

An expression whose declared type is `BOOLEAN` may evaluate to `UNKNOWN` only
through `NULL` propagation. `UNKNOWN` is the logical interpretation of a null
boolean result; it is not a fourth stored value.

## 8. Equality for grouping and duplicate elimination

Grouping, `DISTINCT`, and set operations use **not-distinct equality**:

- two non-`NULL` values are not distinct when ordinary equality is `TRUE`;
- two `NULL` values are not distinct; and
- one `NULL` and one non-`NULL` value are distinct.

Rows are not distinct when every pair of corresponding values is not distinct.

Join predicates and ordinary comparison expressions use three-valued logic,
not not-distinct equality.

## 9. Expression evaluation

Unless an operator is explicitly defined otherwise:

- a strict scalar operator returns `NULL` when any operand is `NULL`;
- a non-null result has the statically inferred result type; and
- evaluation failure produces an evaluation error for the query.

Boolean connectives, `CASE`, `IS NULL`, `IS NOT NULL`, and aggregate functions
have explicit non-strict behavior and are not governed solely by the strict
operator rule.

Except where an operator's contract explicitly defines conditional
evaluation, evaluating an expression requires evaluating every operand
expression. An implementation MAY choose the physical operand evaluation
order, but MUST NOT skip an operand when doing so would suppress a required
evaluation error.

In particular, `AND` and `OR` are not conditional-evaluation operators. Both
operands are required even when one operand determines the truth-table result.
The strict-operator null rule likewise does not permit an implementation to
skip another operand that would produce an evaluation error.

`CASE` has an explicit conditional-evaluation contract in
[Core query semantics](03-query-semantics.md#7-conditional-expressions).

## 10. Finite execution

Every input relation MUST be finite. Every successful relational operation
defined by ShapeSQL over finite inputs produces a finite result.

An implementation MAY reject a query before or during evaluation when a
documented resource limit is exceeded. Resource exhaustion is an evaluation
failure, not a successful partial result.
