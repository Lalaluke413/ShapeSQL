# ShapeSQL Draft Specification

This directory contains the draft specification for ShapeSQL. It defines the
portable contract shared by ShapeSQL front ends, planners, interpreters, and
execution targets.

## Status

The current target is **ShapeSQL 0.1**. It is an unstable draft: requirements
may change until the version is explicitly declared stable.

The documents use normative terms defined in
[Conventions and conformance](00-conventions.md). A document or section marked
non-normative provides context only.

## Current documents

1. [Conventions and conformance](00-conventions.md) defines requirement
   language, terminology, and the meaning of conformance.
2. [Language scope](01-language-scope.md) defines the v0.1 feature boundary and
   its exclusions.
3. [Data model](02-data-model.md) defines schemas, flat rows, scalar values,
   `NULL`, bags, and ordering.
4. [Core query semantics](03-query-semantics.md) defines how the principal
   query constructs transform relations.

## Planned documents

The following documents should be added as the design becomes concrete:

- **Lexical structure and grammar** — tokens, identifiers, literals, and a
  machine-readable grammar.
- **Type system and expressions** — type checking, coercion, scalar operators,
  and error behavior.
- **Shape IR** — typed relational nodes and semantics-preserving rewrites.
- **Execution IR** — streams, ports, routing, buffering, keyed state, and
  completion.
- **SQL lowering** — translation from every accepted syntax form to Shape IR.
- **Diagnostics** — parse, bind, type, planning, and execution errors.
- **Conformance examples** — executable input relations, queries, results, and
  expected errors.

The Keyed State Unit is expected to be specified as an Execution IR mechanism,
not as a SQL operator or a relational Shape IR node.

## Specification boundary

The specification defines:

- accepted ShapeSQL source programs;
- their meaning over valid finite inputs;
- static rejection conditions;
- observable results and errors; and
- portable contracts for intermediate representations once those contracts
  are added.

The specification does not prescribe:

- parser, optimizer, or executor implementation techniques;
- a storage format, index structure, or join algorithm;
- cost models or plan-selection policy;
- transaction, authorization, catalog, or persistence behavior; or
- the internal layout of a software or hardware execution unit.

## Editing guidance

A normative statement should be specific enough to produce a test that
distinguishes a conforming implementation from a non-conforming one.

Place exploratory alternatives, performance arguments, and rejected designs
under `docs/design`. Moving an idea into this directory means that it has
become part of the proposed portable contract.
