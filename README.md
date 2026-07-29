# ShapeSQL

ShapeSQL is a structured query language for flat, non-recursive data
reshaping. It is intended to be relationally complete while remaining
intentionally computationally incomplete.

> [!IMPORTANT]
> ShapeSQL is an early design project. The specification is a draft and no
> conforming implementation exists yet.

## Motivation

SQL is exceptionally good at describing the shape of an answer:

- filtering resizes a relation;
- projection changes its axes;
- joining expands it with another dimension;
- grouping reduces a dimension;
- ordering aligns the result along an axis; and
- partitioned operations compute within a dimension without removing it.

Modern SQL dialects also include recursion, procedural logic, dynamic
execution, and extension mechanisms for arbitrary computation. ShapeSQL draws
a deliberate boundary: a query describes how finite tabular input is reshaped
into finite tabular output. Application logic determines which question to
ask.

That boundary serves a second goal. A ShapeSQL query should have an explicit,
statically typed relational meaning represented by a finite, acyclic Shape IR
graph.

## Language and IR model

ShapeSQL separates a query's source spelling from its relational meaning:

1. **ShapeSQL source** expresses a query over flat relations.
2. **Shape IR** represents its typed relational meaning.
3. Implementations may apply semantics-preserving rewrites while remaining in
   Shape IR. Such rewrites must preserve specified errors as well as successful
   results.

Relational operators such as joins and grouping belong to Shape IR.
Parsing algorithms, optimization strategy, and evaluation mechanisms are
implementation concerns outside the specification.

## Draft v0.1 scope

The first language version is query-only. Its intended features include:

- `SELECT`, `FROM`, `WHERE`, `GROUP BY`, `HAVING`, and `DISTINCT`;
- inner, outer, and cross joins;
- non-recursive common table expressions and derived tables;
- relational predicates such as `EXISTS`, `IN`, and their negations;
- grouping and a small, explicit aggregate set;
- `UNION`, `INTERSECT`, and `EXCEPT`;
- `ORDER BY`, `LIMIT`, and `OFFSET`; and
- a deliberately limited set of partitioned aggregate and ranking operations.

The first version excludes recursive queries, procedural SQL, dynamic SQL,
user-defined functions, side-effecting or volatile functions, nested data,
transactions, data definition, and data modification.

See [Language scope](docs/specification/01-language-scope.md) for the normative
feature boundary.

## Repository layout

```text
docs/
  specification/       Normative ShapeSQL source and Shape IR contracts
  design/              Non-normative proposals and design rationale
src/                    Future reference implementation
tests/
  corpus/               Portable conformance cases and expected outcomes
```

Only `docs/specification` defines conformance. Design notes and implementation
choices may explain a decision, but they do not change the language contract.

## Specification

The draft specification begins at
[docs/specification/README.md](docs/specification/README.md).

The documents are intentionally being written in implementation order. Each
normative rule should eventually correspond to a conformance test, and each
language feature should be representable without relying on recursion or
unbounded computation.

## Project status

The current work is to establish:

1. the exact ShapeSQL language subset and observable query behavior;
2. the lexical structure and grammar of ShapeSQL source;
3. its type rules, null semantics, and multiplicity semantics;
4. the typed relational Shape IR; and
5. a corpus of accepted and rejected queries with expected results or errors.

## License

ShapeSQL is licensed under the [GNU General Public License v3.0](LICENSE).
