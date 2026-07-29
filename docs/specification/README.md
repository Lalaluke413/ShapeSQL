# ShapeSQL Draft Specification

This directory contains the draft specification for ShapeSQL. It defines the
portable contract for ShapeSQL source and its typed relational representation,
Shape IR.

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
5. [Lexical structure](04-lexical-structure.md) defines source encoding,
   whitespace, comments, identifiers, literals, and tokenization.

## Planned documents

The following documents should be added as the language becomes concrete:

- **Grammar** — a machine-readable grammar for complete ShapeSQL programs.
- **Type system and expressions** — type checking, coercion, scalar operators,
  and error behavior.
- **Shape IR** — typed relational nodes and semantics-preserving rewrites.
- **Source-to-IR lowering** — translation from every accepted syntax form to
  Shape IR.
- **Diagnostics** — parse, bind, type, planning, and execution errors.
- **Conformance examples** — executable input relations, queries, results, and
  expected errors.

## Specification boundary

The specification defines:

- accepted ShapeSQL source programs;
- their meaning over valid finite inputs;
- static rejection conditions;
- observable results and errors; and
- the portable Shape IR contract.

The specification does not prescribe:

- parser, optimizer, or evaluator implementation techniques;
- a storage format, index structure, or join algorithm;
- cost models or plan-selection policy;
- transaction, authorization, catalog, or persistence behavior; or
- any lower-level or physical execution representation.

## Editing guidance

A normative statement should be specific enough to produce a test that
distinguishes a conforming implementation from a non-conforming one.

Place exploratory alternatives, performance arguments, and rejected designs
under `docs/design`. Moving an idea into this directory means that it has
become part of the proposed portable contract.
