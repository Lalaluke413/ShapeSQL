# Reference Implementation Corpus

This directory contains source-front-end and direct Shape IR regression cases
for the Rust reference implementation. The canonical, language-neutral
end-to-end suite is in [`conformance`](../../conformance/README.md).

These cases still instantiate requirements from `docs/specification`; they do
not create new language requirements. If a case conflicts with the normative
documents, the normative documents govern.

`manifest.json` lists each case. A case has:

- a stable `id`;
- a source file or, for direct Shape IR tests, an IR fixture;
- any catalog schemas and finite input relations required by the case; and
- an expected outcome.

An erroneous case records one of these phases:

- `lexical`;
- `syntactic`;
- `binding`;
- `typing`;
- `interchange`;
- `shape-ir-validation`; or
- `evaluation`.

Error codes and diagnostic text are intentionally absent. An accepted
front-end case may assert only that source is accepted and valid Shape IR is
produced. Portable catalog, snapshot, typed result, comparison, and process
interop formats are defined only by the canonical conformance suite.

The initial cases establish that:

- uncorrelated relational predicates are accepted;
- correlated `EXISTS` and `IN` subqueries are binding errors;
- grouped and partitioned `BOOL_AND` and `BOOL_OR` expressions are accepted;
- relation, source, field, alias, wildcard, and ordering names follow the
  binding rules;
- representative literals, casts, operators, predicates, aggregates, set
  fields, ordering expressions, and ranking expressions follow the type rules;
- `SELECT DISTINCT` ordering uses only result fields and does not introduce an
  ordering-only partitioned or ranking invocation, and row bounds require an
  ordering complete over result values; and
- direct Shape IR documents distinguish valid interchange, interchange
  decoding failures, and decoded graph-validation failures; and
- representative program, query, expression, and window forms have the
  syntactic outcomes required by the grammar.
