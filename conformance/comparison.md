# Conformance Comparison Rules

## 1. Purpose

This document defines how a conformance runner compares an observed candidate
outcome with a checked-in expected outcome. ShapeSQL semantics remain defined
by `docs/specification`.

Before testing a candidate, a runner MUST validate the suite and verify every
expected outcome against the pinned reference implementation. A candidate is
then compared with the same checked-in outcome. An evaluator regression
therefore cannot silently redefine the corpus, and a stale fixture cannot be
used to judge another implementation.

## 2. Outcome kind

`success` matches only `success`. `error` matches only `error`. A process,
protocol, timeout, or fixture failure is not an observed ShapeSQL outcome and
must be reported separately.

## 3. Successful results

### 3.1 Schema

Schemas compare positionally. They match only when they have the same number
of fields and every corresponding field has the same:

- exact display name;
- scalar type; and
- nullability.

Graph-local field identities, source qualifiers, catalog bindings, metadata,
storage types, and physical representations are not compared.

### 3.2 Values and rows

Every observed row must conform to the observed schema before comparison.
Values compare according to their ShapeSQL scalar values, not their host
representation. `INT64` is compared numerically after canonical decoding,
`TEXT` by exact Unicode scalar sequence, `BOOLEAN` by truth value, and `NULL`
as the null value of its field's scalar type.

Rows compare positionally across fields. Two rows are equal for conformance
comparison exactly when all corresponding values are not distinct under the
ShapeSQL data model.

### 3.3 Bags

For `collection: "bag"`, row sequence is ignored. The expected and observed
results match only when every distinct row has exactly the same multiplicity
in both collections.

Sorting rows by an implementation-specific serialization is not itself a
semantic comparison rule. A runner may use sorting, hashing, or counting as an
implementation technique only when it preserves typed not-distinct equality.

### 3.4 Ordered results

For `collection: "ordered"`, sequence and multiplicity are compared
positionally. `bag` and `ordered` never match, even if their rows happen to be
listed in the same sequence.

The canonical suite admits an ordered case only when every ordering peer group
contains identical result rows or the query uniquely orders its distinct
result rows. Consequently, position-by-position comparison does not select an
otherwise unspecified order among distinguishable peers.

## 4. Errors

Error outcomes match only when their phases are identical. The phases are
`lexical`, `syntactic`, `binding`, `typing`, and `evaluation`.

Diagnostic wording, codes, source spans, the number of diagnostics, the
specific evaluation-error variant, and the point at which an implementation
physically discovers the violation are not compared.

## 5. Protocol correlation

Before outcome comparison, a runner MUST establish that a response is a valid
protocol message with the requested `case_id` and supported protocol version.
A missing, duplicate, out-of-order, malformed, or mismatched response is a
protocol failure, not a conformance mismatch in ShapeSQL semantics.

## 6. Reporting

Report formatting is not standardized. A useful report should distinguish:

- invalid suite data;
- a reference-versus-expected mismatch;
- candidate process or protocol failure;
- candidate schema mismatch;
- candidate collection-kind mismatch;
- candidate row or multiplicity mismatch; and
- candidate error-phase mismatch.
