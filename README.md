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

That boundary serves a second goal. A ShapeSQL query should compile to an
explicit, statically typed, acyclic dataflow graph that can first be interpreted
in software and later lowered to a hardware-oriented execution model.

## Proposed compilation model

ShapeSQL separates the meaning of a query from the mechanism used to execute
it:

1. **ShapeSQL source** expresses a query over flat relations.
2. **Shape IR** represents its typed relational meaning.
3. **Optimized Shape IR** applies semantics-preserving rewrites.
4. **Execution IR** expresses lower-level streaming, routing, and keyed-state
   operations.
5. A **software interpreter** or future **CGRA implementation** executes the
   resulting plan.

Relational operators such as joins and grouping belong to Shape IR. Physical
mechanisms such as partitioning, buffering, and the Keyed State Unit belong to
Execution IR. This keeps the language independent of any one software
algorithm or hardware design.

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
  specification/       Normative language and execution contracts
  design/              Non-normative proposals and design rationale
src/                    Future reference implementation
tests/                  Future executable conformance examples
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
2. the typed relational Shape IR;
3. a reference software interpreter;
4. a lower, hardware-first Execution IR; and
5. a software executor for that IR before attempting an FPGA or CGRA design.

## License

ShapeSQL is licensed under the [GNU General Public License v3.0](LICENSE).
