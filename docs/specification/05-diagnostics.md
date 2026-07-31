# Diagnostics

## 1. Overview

This document classifies errors by the language or Shape IR rule that is
violated. The classification makes rejection cases testable without
standardizing diagnostic prose or prescribing an implementation pipeline.

## 2. Error phases

ShapeSQL defines the following phases:

| Phase | Input | Violation |
| --- | --- | --- |
| `lexical` | Source | Invalid tokenization. |
| `syntactic` | Source | Tokens do not match the grammar. |
| `binding` | Source | Invalid name resolution, visibility, or CTE dependency. |
| `typing` | Source | Invalid type, nullability, or arity. |
| `interchange` | Interchange document | Invalid JSON or record mapping. |
| `shape-ir-validation` | Shape IR | Invalid graph invariant. |
| `evaluation` | Shape IR and inputs | Required evaluation failure. |

Lexical, syntactic, binding, and typing errors are collectively **static
errors**. They prevent a source program from producing valid Shape IR.

Interchange decoding and Shape IR validation are separate interface
boundaries. A purported interchange document that cannot reconstruct an
abstract graph has an `interchange` error. A reconstructed graph that violates
a graph invariant has a `shape-ir-validation` error. The exact boundary is
defined by
[Shape IR interchange](11-shape-ir-interchange.md#11-mapping-and-graph-validation).

A conforming front end MUST NOT produce invalid Shape IR from any source
program. A Shape IR validation corpus case therefore tests a Shape IR consumer
directly; it does not describe a permitted user error from otherwise valid
ShapeSQL source.

Evaluation is also distinct from static rejection. Division by zero and
integer overflow, for example, remain evaluation errors even when an
implementation can prove before evaluation that they will occur.

## 3. Phase assignment

An error belongs to the earliest phase in the table whose rules require the
information needed to establish the violation.

Token spelling and source encoding are lexical concerns. Token arrangement is
a syntactic concern. Resolving relation, field, alias, and common-table
expression names according to [Binding](07-binding.md) is a binding concern.
Applying operator signatures and other static semantic rules to a bound
program is a typing concern.

JSON decoding, duplicate object members, and mapping recognized interchange
records are interchange concerns. Reference resolution, acyclicity, schema
derivation, and expression descriptor verification are Shape IR validation
concerns.

Whether a subquery reference is correlated depends on which field the
reference resolves to. A prohibited correlated reference in `EXISTS`, `IN`, or
`NOT IN` is therefore a binding error, not a syntactic error.

An implementation MAY combine phases internally or discover a violation
earlier than its phase name suggests. Those choices do not change the
normative classification.

## 4. Diagnostic content

Conformance requires the correct phase classification. ShapeSQL 0.1 does not
standardize:

- error codes;
- diagnostic text;
- source spans;
- hints;
- the number of diagnostics emitted; or
- which diagnostic is selected when a program independently violates multiple
  rules in the same phase.

Implementations SHOULD identify the relevant source or Shape IR location and
provide enough detail for an author to correct the violation.

## 5. Conformance corpus

Each erroneous corpus case MUST identify exactly one expected phase from
Section 2. It MUST NOT require an exact error message.

A case expected to succeed MUST instead specify its result schema, result bag,
and required result order, when any. A front-end-only case MAY assert
acceptance and valid Shape IR without prescribing evaluation output.

Portable corpus cases MUST NOT depend on implementation-defined resource
limits.
