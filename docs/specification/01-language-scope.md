# ShapeSQL 0.1 Language Scope

## 1. Purpose

ShapeSQL 0.1 is a pure query language for reshaping finite, flat relations. A
program describes the result relation to derive from its inputs; it does not
modify those inputs or perform externally visible actions.

Every valid ShapeSQL 0.1 program MUST be representable as a finite, statically
typed, acyclic relational graph.

## 2. Design boundary

ShapeSQL includes operations that select, combine, extend, reduce, partition,
and order tabular data. It excludes constructs whose purpose is general
control flow, recursion, external interaction, or mutation.

The following properties are REQUIRED:

- **Query-only:** evaluation reads input relations and produces one result.
- **Finite:** every input relation and successful result is finite.
- **Flat:** every field contains a scalar value or `NULL`, not another
  relation, collection, record, or document.
- **Non-recursive:** the dependency graph between query expressions is
  acyclic.
- **Statically typed:** every expression and result field has a type before
  execution begins.
- **Side-effect free:** query evaluation does not modify host state or invoke
  external effects.

These restrictions make ShapeSQL computationally incomplete by design. They do
not limit the optimizer's ability to rewrite a query or the executor's ability
to use loops internally.

## 3. Included query forms

The following constructs are part of ShapeSQL 0.1.

### 3.1 Relation sources

- named input relations;
- aliased relation sources;
- derived tables produced by a nested query; and
- non-recursive common table expressions introduced by `WITH`.

A common table expression MUST NOT refer to itself, directly or indirectly.

### 3.2 Selection and projection

- `SELECT` expressions and aliases;
- `SELECT *` and qualified wildcard projection;
- `DISTINCT`;
- `WHERE`; and
- `CASE` expressions.

The initial scalar expression set is defined in Section 7.

### 3.3 Joins

- `CROSS JOIN`;
- `INNER JOIN`;
- `LEFT OUTER JOIN`;
- `RIGHT OUTER JOIN`;
- `FULL OUTER JOIN`; and
- comma-separated relation sources, with the semantics of a cross join.

Qualified joins use `ON`. `NATURAL JOIN` and `USING` are excluded from v0.1 so
that output schemas do not depend on implicit name matching or column merging.

### 3.4 Relational predicates

- `EXISTS` and `NOT EXISTS`;
- `IN` and `NOT IN` with a finite literal list; and
- `IN` and `NOT IN` with a single-column query.

Subqueries in these predicates MAY refer to fields from an enclosing query.
Scalar-valued and row-valued subquery expressions are excluded.

### 3.5 Grouping

- `GROUP BY` over scalar expressions;
- `HAVING`; and
- `COUNT`, `SUM`, `MIN`, and `MAX`.

`GROUPING SETS`, `ROLLUP`, `CUBE`, ordered-set aggregates, hypothetical-set
aggregates, and user-defined aggregates are excluded.

### 3.6 Set operations

- `UNION` and `UNION ALL`;
- `INTERSECT` and `INTERSECT ALL`; and
- `EXCEPT` and `EXCEPT ALL`.

Inputs to a set operation MUST have the same number of fields and compatible
field types. Exact compatibility and coercion rules will be defined by the
type-system document.

### 3.7 Ordering and bounds

- `ORDER BY`;
- explicit `ASC` and `DESC`;
- explicit `NULLS FIRST` and `NULLS LAST`;
- `LIMIT` with a non-negative integer literal; and
- `OFFSET` with a non-negative integer literal.

An `ORDER BY` item MUST be a result field, its ordinal position, or an
expression accepted by the query's projection environment. Default null
placement is not defined: v0.1 programs that sort a nullable expression MUST
state `NULLS FIRST` or `NULLS LAST`.

### 3.8 Partitioned operations

ShapeSQL 0.1 includes only:

- `COUNT`, `SUM`, `MIN`, and `MAX` over an entire partition;
- `ROW_NUMBER`;
- `RANK`; and
- `DENSE_RANK`.

Partitioned aggregates MAY use `PARTITION BY` but MUST NOT use a window
`ORDER BY` or frame clause. Ranking functions MUST use a window `ORDER BY` and
MAY use `PARTITION BY`.

The window `ORDER BY` for `ROW_NUMBER` MUST contain a direct reference to every
field visible at the window-evaluation stage. This guarantees a complete order
of distinct row values without relying on runtime uniqueness. `RANK` and
`DENSE_RANK` MAY intentionally contain peer groups.

Named window specifications, offset functions such as `LAG` and `LEAD`,
value-selection functions, and explicit window frames are excluded.

## 4. Excluded language categories

ShapeSQL 0.1 does not include:

- recursive common table expressions or any other recursion;
- procedural blocks, variables, loops, branching statements, or exception
  handlers;
- data definition, data modification, transactions, or session control;
- prepared-statement syntax or dynamic SQL;
- user-defined scalar, aggregate, or table functions;
- stored procedures;
- volatile, nondeterministic, side-effecting, or external functions;
- `LATERAL` relation sources or general `APPLY` operators;
- scalar or row-valued subquery expressions;
- arrays, maps, records, ranges, JSON, XML, or nested relations;
- graph, pattern-recognition, or model clauses;
- implementation-specific hints; or
- catalog, authorization, and storage-management statements.

These categories are outside the language, not merely optional
implementation features. Accepting one requires an extension mode as described
in [Conventions and conformance](00-conventions.md).

## 5. Determinism

Given the same input schemas and bags, a valid ShapeSQL program MUST produce
the same result schema and result bag, unless evaluation raises a specified
error.

Row sequence is observable only when the outermost query contains `ORDER BY`.
Without it, implementations MAY emit rows in any order and MAY choose a
different order between executions.

An `ORDER BY` that does not uniquely order the result does not define the order
among peers. A query that requires a fully repeatable sequence MUST include
enough ordering expressions to distinguish every result row.

## 6. Host boundary

The host environment, not ShapeSQL, is responsible for:

- choosing the input snapshot;
- resolving authorized relation names;
- supplying input schemas and data;
- transaction isolation and concurrency;
- persistence and recovery; and
- delivering the completed result or execution error.

ShapeSQL evaluation MUST behave as if every input relation were immutable for
the duration of one query. The means used to provide that view are outside this
specification.

## 7. Initial scalar expression set

ShapeSQL 0.1 includes:

- column references and scalar literals;
- parentheses;
- unary integer `+` and `-`;
- integer `+`, `-`, `*`, `/`, and `%`;
- `=`, `<>`, `<`, `<=`, `>`, and `>=`;
- `AND`, `OR`, and `NOT`;
- `IS NULL` and `IS NOT NULL`;
- searched `CASE`;
- `CAST` among conversions explicitly permitted by the type system; and
- text concatenation with `||`.

No implicit conversion is permitted unless the type-system document explicitly
defines it. Division by zero and integer overflow are execution errors.

Functions not explicitly listed by this specification are excluded from the
portable language.

## 8. Relational completeness

“Relationally complete” is a project design goal: ShapeSQL should express the
operations of relational algebra over finite flat relations. It is not yet a
conformance claim. A future rationale document will map the final language and
Shape IR to the required relational operations.
