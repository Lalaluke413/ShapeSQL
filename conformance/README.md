# ShapeSQL Conformance Suite

This directory contains the canonical end-to-end conformance suite for the
ShapeSQL language version declared by `manifest.json`.

The suite tests the public language boundary:

```text
ShapeSQL source + catalog fixture + finite relation snapshot
    -> typed result bag / ordered result / error phase
```

It does not require an implementation to expose, consume, or internally use
Shape IR. Direct Shape IR interchange and evaluator tests belong to the
reference implementation's own test suite.

## Status and authority

The documents in `docs/specification` define ShapeSQL semantics. The cases in
this directory instantiate those requirements but do not create new language
requirements. If a case or conformance document conflicts with the
specification, the specification governs.

The fixture and comparison documents are a versioned, language-neutral test
contract. `protocol.md` is normative only for an adapter claiming compatibility
with the adapter protocol version declared by the manifest. Neither the
protocol nor a Rust implementation API is part of the ShapeSQL language.

The reference implementation is an executable semantic oracle, not a second
source of normative rules. A corpus verifier must compare every checked-in
expected outcome with the reference stack before using the corpus to judge
another implementation.

## Contents

- `manifest.json` declares all relevant versions and inventories every case.
- `fixtures.md` defines catalogs, snapshots, expected outcomes, and typed JSON
  values.
- `comparison.md` defines schema, bag, ordering, and error comparison.
- `protocol.md` defines the JSON Lines adapter protocol.
- `schemas/` contains JSON Schema validation aids for the manifest, fixture
  documents, and protocol messages.
- `cases/` contains self-contained end-to-end cases grouped by expected phase
  or successful evaluation.

Each manifest entry explicitly names its source, catalog, snapshot, and
expected-outcome file. Consumers must not infer compatibility or file roles
from directory names.

## Versioning

Language, fixture-format, adapter-protocol, and manifest-format versions are
explicit fields in `manifest.json`. They are not inferred from this directory's
path.

An immutable ShapeSQL release tag preserves the exact specification, corpus,
and protocol for that release. A maintenance branch may publish compatible
errata or additional cases for an older language version. A later language
version may reorganize or replace this directory without changing a tagged
historical release.

## Intended tooling flow

A conformance tool should provide three operations:

1. validate every manifest entry and fixture document;
2. run every case through the pinned reference stack and compare it with the
   checked-in expected outcome; and
3. send the same source, catalog, and snapshot to an external adapter and
   compare its response with that expected outcome.

The candidate never receives `expected.json`.
