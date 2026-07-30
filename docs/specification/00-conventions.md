# Conventions and Conformance

## 1. Status

This document is part of the draft ShapeSQL 0.1 specification.

## 2. Normative language

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHOULD**, **SHOULD NOT**,
and **MAY** describe requirement levels.

- **MUST**, **MUST NOT**, and **REQUIRED** state conformance requirements.
- **SHOULD** and **SHOULD NOT** state recommendations. An implementation may
  depart from one only when it understands and accepts the consequences.
- **MAY** states a permitted choice.

Text is normative unless it is explicitly marked non-normative, an example,
an editor's note, or an open issue.

Examples illustrate requirements but do not add requirements. If an example
conflicts with normative text, the normative text governs.

## 3. Terms

This specification uses the following terms:

- **ShapeSQL program**: one ShapeSQL query submitted for compilation.
- **input relation**: a finite relation supplied to a query by its host
  environment.
- **result relation**: the finite relation produced by a successful query.
- **schema**: an ordered sequence of fields.
- **field**: a typed position in a schema.
- **row**: one value for each field in a schema.
- **bag**: an unordered collection that may contain duplicate rows.
- **expression**: a computation that produces one scalar value from literals
  and values visible in its evaluation environment.
- **implementation**: a front end, Shape IR consumer, or combined system that
  claims conformance to some part of this specification.
- **host environment**: the system that supplies input relations and consumes
  results. It may also provide catalog, storage, transaction, and authorization
  services outside ShapeSQL.
- **Shape IR**: the portable, statically typed relational representation of a
  ShapeSQL query.
- **observable behavior**: a result schema, result bag, required result order,
  or specified error outcome visible outside the implementation.

“Query” refers to the ShapeSQL program or its relational meaning as determined
by context. “Column” is used for SQL-facing names; “field” is used for typed
schema positions.

## 4. Conformance classes

ShapeSQL defines separate conformance classes so that components can be tested
independently.

### 4.1 Front-end conformance

A conforming front end:

- MUST accept every valid program in the claimed ShapeSQL version;
- MUST reject every program outside the claimed version;
- MUST produce Shape IR with the specified schema and relational meaning; and
- MUST report an error in the phase required by the specification when that
  phase is defined.

### 4.2 Shape IR evaluation conformance

A conforming Shape IR evaluator:

- MUST accept every valid graph in the claimed Shape IR version;
- MUST produce the specified observable behavior for every valid finite input;
  and
- MUST report specified evaluation errors rather than silently produce a
  different result.

### 4.3 End-to-end conformance

A conforming end-to-end implementation MUST satisfy both front-end and
Shape IR evaluation conformance for the same language and Shape IR versions.

Extensions MUST NOT change the meaning of valid ShapeSQL programs. An
implementation that accepts extensions MUST provide a mode in which programs
outside the claimed ShapeSQL version are rejected.

## 5. Mathematical notation

The following notation is used where prose would be ambiguous:

- `S = [f1, f2, ... fn]` denotes an ordered schema.
- `r.f` denotes the value of field `f` in row `r`.
- `|R|` denotes the number of rows in bag `R`, including duplicates.
- `R ⊎ T` denotes bag union: multiplicities are added.
- `TRUE`, `FALSE`, and `UNKNOWN` denote logical truth values.
- `NULL` denotes the absence of a scalar value. It is not the same object as
  the logical value `UNKNOWN`.

## 6. Errors

A **static error** occurs during lexical, syntactic, binding, or typing
analysis of ShapeSQL source and prevents Shape IR evaluation.

A **Shape IR validation error** occurs when a Shape IR graph violates the
structural, binding, or typing invariants of its claimed Shape IR version.

An **evaluation error** occurs while evaluating valid Shape IR over otherwise
valid inputs.

An implementation MUST NOT replace a required error with an ordinary scalar
value, an empty relation, or partial success. It also MUST NOT replace a
successful result with an error.

The error phase is normative when the specification or conformance corpus
defines it. Error text is not normative. Implementations MAY detect an error
earlier than a literal phase-by-phase implementation would, but MUST classify
it according to the phase whose rule was violated.

The phases and their boundaries are defined in
[Diagnostics](05-diagnostics.md).

## 7. Semantics-preserving rewrites

A rewrite is semantics-preserving only when, for every valid input:

- evaluation succeeds before the rewrite if and only if it succeeds after the
  rewrite; and
- when evaluation succeeds, the result schema, result bag, and any required
  result order are unchanged.

Therefore, equivalence over successful scalar values or result bags alone is
insufficient. A rewrite MUST NOT suppress an evaluation error that the
unrewritten Shape IR would require, and MUST NOT introduce an evaluation error
for an input on which the unrewritten Shape IR would succeed.

Static errors are determined before valid Shape IR exists. Rewriting or
constant folding MUST NOT make otherwise invalid ShapeSQL source acceptable.

## 8. Versioning

Language and Shape IR versions are independent. Supporting ShapeSQL 0.1 does
not imply support for every future Shape IR version.

Before ShapeSQL 0.1 is declared stable, this draft may introduce incompatible
changes. After stabilization, an implementation MUST identify the language and
Shape IR versions to which it claims conformance.
