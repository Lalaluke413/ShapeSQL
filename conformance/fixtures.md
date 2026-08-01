# Conformance Fixture Format

## 1. Status and scope

This document defines the language-neutral fixture format used by the
ShapeSQL conformance suite. It is non-normative with respect to ShapeSQL
semantics. A document claiming fixture-format version `0.1` must satisfy this
contract.

A complete case consists of four artifacts named explicitly by the manifest:

- a byte-for-byte ShapeSQL source file;
- a catalog document;
- a finite relation snapshot document; and
- an expected-outcome document.

The fixture model is deliberately smaller than a database host. It has no
files, persistence, indexes, transactions, authorization, statistics,
streaming, or resource controls.

## 2. Common JSON requirements

Catalog, snapshot, expected-outcome, manifest, request, and response documents
are JSON encoded as UTF-8 without a byte-order mark.

A conforming consumer:

- MUST reject duplicate object member names;
- MUST reject unknown members in semantic objects;
- MUST require every member marked as required by this document;
- MUST compare member names and string values exactly by Unicode scalar value;
- MUST NOT assign meaning to object member order; and
- MUST reject a version other than the exact version it supports.

Arrays are ordered. Unless a section states otherwise, their order is part of
the represented value. JSON Schema files in `schemas/` validate structural
requirements but do not replace this prose where contextual rules cannot be
expressed in JSON Schema.

## 3. Type descriptors and fields

A type descriptor has exactly these members:

```json
{
  "scalar": "int64",
  "nullable": false
}
```

`scalar` is one of `boolean`, `int64`, or `text`. `nullable` is a JSON boolean.

A portable field has exactly a `name` and `type`:

```json
{
  "name": "amount",
  "type": {
    "scalar": "int64",
    "nullable": false
  }
}
```

Field order is observable. Field names may be empty or duplicated where the
ShapeSQL result-schema rules permit it.

## 4. Catalog documents

A catalog declares the complete source-visible relation namespace supplied to
one case:

```json
{
  "format_version": "0.1",
  "relations": [
    {
      "name": "orders",
      "binding": "fixture.orders",
      "fields": [
        {
          "name": "amount",
          "type": {
            "scalar": "int64",
            "nullable": false
          }
        }
      ]
    }
  ]
}
```

`name` is the catalog name after ShapeSQL identifier decoding and
normalization. A regular source identifier is ASCII-case-folded as specified
by the binding rules; a delimited source identifier denotes its decoded exact
name. `binding` is an opaque host identity and is compared exactly, without
identifier normalization.

Catalog order is not semantically significant. Duplicate or otherwise
ambiguous source-visible names are permitted only when a case intentionally
tests the resulting binding behavior.

More than one catalog relation may use the same binding only when all such
relations have identical ordered field names and type descriptors. This lets
the snapshot schema be derived unambiguously without repeating it.

## 5. Snapshot documents

A snapshot supplies one finite input bag for every distinct catalog binding:

```json
{
  "format_version": "0.1",
  "relations": [
    {
      "binding": "fixture.orders",
      "rows": [
        ["10"],
        ["10"],
        ["0"]
      ]
    }
  ]
}
```

Snapshot relation order and row order are not semantically significant.
Repeated rows represent distinct occurrences and therefore bag multiplicity.
A zero-field row is encoded as `[]`.

Each distinct catalog binding MUST occur exactly once in the snapshot, and the
snapshot MUST contain no other binding. Every row MUST have the arity of the
catalog schema associated with that binding. A snapshot is an isolated,
immutable database for one case.

### 5.1 Typed cell encoding

The catalog field at the same ordinal determines how a JSON cell is decoded:

| ShapeSQL value | JSON encoding |
| --- | --- |
| `NULL` | `null` |
| `BOOLEAN` | JSON `true` or `false` |
| `INT64` | canonical base-ten JSON string |
| `TEXT` | JSON string |

An `INT64` string is `0`, or an optional `-` followed by a nonzero ASCII digit
and zero or more ASCII digits. Leading `+`, leading zeroes, `-0`, whitespace,
and non-ASCII digits are invalid. Its mathematical value MUST be in the
inclusive range -9223372036854775808 through 9223372036854775807.

A `null` cell is valid only for a nullable field. A non-null cell must use the
encoding required by the field's scalar type. The schema, not the spelling,
distinguishes an `INT64` string such as `"42"` from the `TEXT` value `"42"`.

## 6. Expected-outcome documents

An expected outcome is either success or a ShapeSQL error phase.

### 6.1 Success

```json
{
  "format_version": "0.1",
  "outcome": {
    "kind": "success",
    "collection": "bag",
    "schema": [
      {
        "name": "amount",
        "type": {
          "scalar": "int64",
          "nullable": false
        }
      }
    ],
    "rows": [
      ["10"],
      ["10"]
    ]
  }
}
```

`collection` is `bag` when the outermost result is unordered and `ordered`
when the outermost query has an observable order. Rows use the typed cell
encoding from Section 5.1 and MUST conform to `schema`.

For `bag`, row sequence in the document is non-semantic. For `ordered`, row
sequence is the required result sequence. Because ShapeSQL leaves the order of
ordering peers unspecified, canonical `ordered` cases MUST be constructed so
that every peer group either contains only not-distinct result rows or is
otherwise uniquely ordered by the query. The format does not make an
arbitrary reference-evaluator tie break normative.

### 6.2 Error

```json
{
  "format_version": "0.1",
  "outcome": {
    "kind": "error",
    "phase": "evaluation"
  }
}
```

The phase is exactly one of `lexical`, `syntactic`, `binding`, `typing`, or
`evaluation`. Diagnostic codes, messages, spans, node identities, and
implementation error types are not fixture data.

Host setup failures, malformed fixture data, unsupported protocol versions,
timeouts, process failures, I/O errors, cancellation, and resource exhaustion
are not ShapeSQL outcome phases in this format.

## 7. Manifest and case identity

`manifest.json` explicitly declares:

- its manifest format version;
- the ShapeSQL language name and version;
- the fixture-format version;
- the adapter-protocol version; and
- the stable identifier, four artifact paths, and tags of every case.

Paths are relative to the directory containing the manifest. They MUST use
forward slashes, MUST NOT be absolute, and MUST NOT contain `.` or `..` path
segments. Every path MUST resolve to a regular file inside the conformance
directory. A file may be named by more than one case, although the canonical
suite favors self-contained case directories.

Case identifiers are opaque stable strings. A runner reports and exchanges
the identifier from the manifest; it must not derive an identifier from a
path. Case and tag order in the manifest is non-semantic.

## 8. Fixture validation boundary

A runner MUST validate a complete case before invoking either the reference
stack or a candidate adapter. Invalid JSON, version mismatch, duplicate
members, path violations, missing bindings, extra bindings, schema mismatch,
row arity mismatch, or invalid cell encoding makes the case invalid. It is a
suite-authoring or runner failure, not the expected ShapeSQL error for that
case.
