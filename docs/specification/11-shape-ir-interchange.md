# Shape IR Interchange

## 1. Overview

This document defines **Shape IR Interchange 0.1**, the portable JSON encoding
of one Shape IR 0.1 graph.

The interchange format is a control-plane representation. It permits a front
end, optimizer, validator, evaluator, or diagnostic tool to exchange the
logical graph defined by [Shape IR](09-shape-ir.md). It is not:

- a ShapeSQL source format;
- a catalog or host-relation manifest;
- an encoding of input or result rows;
- a physical execution plan;
- a hardware command stream; or
- an in-memory API requirement.

The abstract graph remains normative for relational meaning. This document
defines only how that graph is represented as bytes and reconstructed by a
consumer.

Shape IR Interchange and Shape IR have independent versions. This document
defines interchange version `0.1` for Shape IR version `0.1`.

## 2. Conformance and processing model

A conforming Shape IR Interchange 0.1 producer MUST:

- emit a document satisfying the JSON and structural requirements in this
  document;
- identify interchange version `0.1` and Shape IR version `0.1`;
- encode every semantic property of the represented graph; and
- emit a graph that passes Shape IR 0.1 validation when it claims to produce
  valid Shape IR.

A conforming Shape IR Interchange 0.1 consumer MUST:

- decode every structurally valid interchange document using its supported
  versions;
- reject a document that violates the JSON or structural mapping;
- reconstruct the represented abstract graph without assigning meaning to
  JSON object-member order or node-array order;
- validate the complete reconstructed graph before evaluation; and
- reject unknown semantic content rather than silently ignore it.

Processing has three distinct stages:

1. Decode one JSON text under Section 3.
2. Map the decoded JSON value to the interchange records in Sections 4
   through 10.
3. Validate the resulting abstract graph under
   [Shape IR](09-shape-ir.md#10-validation).

A failure in stage 1 or 2 is an `interchange` error. A failure in stage 3 is a
`shape-ir-validation` error. A consumer MUST NOT begin graph evaluation after
either failure.

This processing model does not require three implementation passes. A
consumer MAY combine them internally, provided it reports the normative error
phase and does not use unvalidated graph contents for execution.

## 3. JSON encoding

### 3.1 Document bytes

An interchange document MUST contain exactly one JSON text as defined by
[RFC 8259](https://www.rfc-editor.org/rfc/rfc8259). The text:

- MUST be encoded as UTF-8;
- MUST NOT begin with a byte-order mark;
- MUST have one JSON object as its top-level value; and
- MUST NOT be followed by another JSON value.

JSON whitespace before or after the object and between tokens is permitted.
A producer SHOULD terminate a file with one line feed, but the presence of
that line feed has no semantic meaning.

Every decoded JSON string MUST be a sequence of Unicode scalar values.
An isolated UTF-16 surrogate escape is therefore an `interchange` error.
Different valid JSON escape spellings that decode to the same scalar sequence
represent the same string.

### 3.2 Object members

Object-member names are case-sensitive. Their order has no meaning.

Every JSON object in the document, including inside `metadata`, MUST have
unique member names. A consumer MUST detect and reject duplicate members
rather than select one occurrence.

Each semantic object is closed. It contains exactly:

- the required members specified for that object;
- any conditionally present members specified for its variant; and
- the optional `metadata` member when that object class permits metadata.

An absent optional member is distinct from a member whose value is JSON
`null`. JSON `null` is permitted only where this document explicitly permits
it.

An unknown member outside `metadata` is an `interchange` error. This rule
prevents an older consumer from ignoring a new property that could change
query meaning or error behavior.

### 3.3 Arrays

JSON arrays represent ordered sequences unless this document explicitly says
otherwise. Their element order and multiplicity MUST be preserved.

In particular, order is significant for:

- schemas;
- project entries;
- grouping keys and aggregate definitions;
- window definitions;
- partition expressions;
- ordering items;
- `case` arms; and
- `in_list` candidates.

The `nodes` array is a table, not a relational sequence. Its element order has
no semantic meaning.

### 3.4 JSON numbers

No semantic interchange member in version 0.1 uses a JSON number. `INT64`
values use the decimal-string encoding in Section 4.3.

JSON numbers MAY occur inside `metadata`, where they have no Shape IR meaning.
A consumer that preserves metadata SHOULD preserve their mathematical JSON
value, but metadata preservation is not required.

## 4. Common value encodings

### 4.1 Identifiers

Node and field identities are JSON strings using this ASCII grammar:

```text
identifier =
    ( ASCII letter | "_" )
    { ASCII letter | ASCII digit | "_" | "-" | "." | ":" }
```

An identity is nonempty and case-sensitive. Consumers compare identities by
their exact ASCII bytes.

Node identities form one graph-wide namespace. Field identities form a
separate graph-wide namespace. The same spelling MAY occur once in each
namespace because the reference position determines the namespace.

Renaming identities consistently does not change graph semantics. A consumer
MUST NOT infer allocation order, node kind, field position, or host meaning
from an identity's spelling.

### 4.2 Host bindings and display names

An `input` relation binding is a nonempty JSON string. Its decoded Unicode
scalar sequence is the opaque host identity defined by Shape IR. Two bindings
are equal only when those sequences are equal. No Unicode normalization,
case conversion, URI interpretation, path interpretation, or ShapeSQL name
resolution is applied.

A field display name is any JSON string, including the empty string. It is
result metadata and is not an identity.

### 4.3 `INT64` values

Every Shape IR `INT64` value outside a scalar `TEXT` literal is encoded as a
JSON string containing its canonical base-ten spelling.

The spelling:

- uses only ASCII digits with an optional leading `-`;
- has no leading zero unless the complete spelling is `"0"`;
- does not use a leading `+`;
- does not use `"-0"`;
- contains no whitespace, separator, decimal point, or exponent; and
- denotes a value from `-9223372036854775808` through
  `9223372036854775807`.

The strings `"-9223372036854775808"`, `"0"`, and
`"9223372036854775807"` are valid. The JSON number `0`, the string `"+1"`,
and the string `"01"` are not valid `INT64` encodings.

### 4.4 Enum values

Every enum value defined by this document is a lowercase ASCII JSON string.
Enum spellings are exact. A consumer MUST NOT apply case conversion or accept
an alias.

An enum value outside the closed set for its claimed versions is an
`interchange` error.

### 4.5 Type descriptors

A type descriptor is an object with exactly these members:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `scalar` | string | `boolean`, `int64`, or `text`. |
| `nullable` | Boolean | Whether the value is permitted to be `NULL`. |

For example:

```json
{
  "scalar": "int64",
  "nullable": true
}
```

Type descriptors do not permit `metadata`. Their lowercase scalar spellings
map directly to `BOOLEAN`, `INT64`, and `TEXT` in the abstract graph.

### 4.6 Field descriptors

A field descriptor contains:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `id` | identifier | The field identity. |
| `name` | string | The display name. |
| `type` | object | The field type descriptor. |
| `metadata` | object | Optional non-semantic metadata. |

The object contains no other members.

An output schema is an array of field descriptors in schema order. The same
passed-through field identity appears in the output schemas of several nodes;
those repeated descriptors do not define the field again. Definition and
pass-through behavior is determined by the node rules in Shape IR.

### 4.7 Metadata

The document, every node, every field descriptor, and every scalar expression
MAY contain one `metadata` member. Its value MUST be a JSON object. The object
MAY contain arbitrary JSON values subject to the duplicate-member and Unicode
rules in Section 3.

Metadata is non-semantic:

- a consumer MUST NOT use it to make an otherwise invalid graph valid;
- changing or removing it MUST NOT change evaluation, ordering, errors, or
  host binding;
- a consumer MUST accept unrecognized metadata members;
- a consumer MAY discard all metadata; and
- a producer MUST NOT place a property required for correct interpretation
  only in metadata.

An object inside `metadata` does not become a node, expression, descriptor, or
reference merely because it uses the same member names as a semantic record.

Metadata authors SHOULD namespace member names when collision with other tools
is possible.

Helper records such as ordering items, project entries, `case` arms, and
aggregate definitions do not independently permit `metadata` in version 0.1.
Their containing node or expression can carry any needed annotation.

## 5. Document envelope

The top-level object contains:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `interchange_version` | string | Required and exactly `"0.1"`. |
| `shape_ir_version` | string | Required and exactly `"0.1"`. |
| `root` | node identifier | Required result-root reference. |
| `nodes` | array | Required nonempty node table. |
| `metadata` | object | Optional non-semantic metadata. |

It contains no other members.

The document does not contain a ShapeSQL language version. A graph can be
created or rewritten without having one source program. It also does not
contain input rows, catalog schemas, credentials, storage locations, or
physical-plan properties.

Every `nodes` element MUST be one node object under Sections 8 through 10.
Node identities MUST be unique. `root` and every node reference are resolved
after the complete table is indexed; forward references are permitted.

For example, this is the envelope of a one-node graph:

```json
{
  "interchange_version": "0.1",
  "shape_ir_version": "0.1",
  "root": "n0",
  "nodes": [
    {
      "id": "n0",
      "kind": "empty",
      "output_schema": [],
      "collection": "bag"
    }
  ]
}
```

## 6. Scalar expressions

### 6.1 Common expression members

Every scalar expression is an object containing:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `kind` | string | One expression-kind tag from Section 6.2. |
| `type` | object | Required type descriptor for this expression. |
| `metadata` | object | Optional non-semantic metadata. |

It also contains the kind-specific members defined below and no others.

Expressions are nested values rather than identity-bearing table entries.
Repeating the same expression object in two locations does not require
shared evaluation. The exact nested tree preserves conditional demand for
`and`, `or`, and `case`.

### 6.2 Expression-kind summary

The expression kinds and their additional members are:

| `kind` | Additional members |
| --- | --- |
| `literal` | `value` |
| `field` | `field` |
| `unary` | `operation`, `operand` |
| `binary` | `operation`, `left`, `right` |
| `is_null` | `operand`, `negated` |
| `case` | `arms`, `fallback` |
| `cast` | `operand`, `target` |
| `in_list` | `value`, `candidates` |
| `exists` | `query` |
| `in_query` | `value`, `query`, `field` |

### 6.3 `literal`

The `value` member encodes the literal according to its `type.scalar`:

| Scalar type | Non-null JSON value |
| --- | --- |
| `boolean` | JSON `true` or `false`. |
| `int64` | A decimal string under Section 4.3. |
| `text` | A JSON string containing the text value. |

JSON `null` represents typed `NULL`.

A non-null literal MUST have `nullable` set to `false`. A null literal MUST
have `nullable` set to `true`. Shape IR validation verifies these descriptors.
The string `"1"` is an `INT64` value when the scalar type is `int64` and a
one-character text value when the scalar type is `text`.

Examples:

```json
{
  "kind": "literal",
  "type": {
    "scalar": "int64",
    "nullable": false
  },
  "value": "-9223372036854775808"
}
```

```json
{
  "kind": "literal",
  "type": {
    "scalar": "boolean",
    "nullable": true
  },
  "value": null
}
```

### 6.4 `field`

The `field` member is one field identity. It refers to the expression
environment of the containing node. For `in_query.field`, the separate rules
in Section 6.10 apply.

### 6.5 `unary` and `binary`

A `unary.operation` is one of:

- `positive`;
- `negative`; or
- `not`.

Its `operand` is one scalar expression.

A `binary.operation` is one of:

- `add`, `subtract`, `multiply`, `divide`, or `remainder`;
- `concatenate`;
- `equal`, `not_equal`, `less`, `less_or_equal`, `greater`, or
  `greater_or_equal`;
- `and`; or
- `or`.

Its `left` and `right` members are distinct operand positions containing
scalar expressions. Their object order is irrelevant, but their member names
preserve the ordered operand roles. A producer MUST NOT flatten `and` or `or`
into an operand array.

### 6.6 `is_null`

`operand` is one scalar expression. `negated` is a JSON Boolean:

- `false` represents `IS NULL`; and
- `true` represents `IS NOT NULL`.

### 6.7 `case`

`arms` is a nonempty array. Each arm is an object containing exactly:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `when` | expression | The arm predicate. |
| `then` | expression | The arm result. |

Arm order is semantic and MUST be preserved. `fallback` is one scalar
expression and is always present, including when it is a typed null introduced
for a source `CASE` without `ELSE`.

### 6.8 `cast`

`operand` is one scalar expression. `target` is one of `boolean`, `int64`, or
`text`.

`target` records only the target scalar type. The containing expression's
`type.nullable` records the result nullability derived from the operand.

### 6.9 `in_list`

`value` is the left scalar expression. `candidates` is a nonempty array of
scalar expressions. Candidate order and duplicate candidates are preserved.

### 6.10 `exists` and `in_query`

For `exists`, `query` is the identity of the referenced relational node.

For `in_query`:

- `value` is the left scalar expression;
- `query` is the identity of the referenced relational node; and
- `field` is the identity of that node's sole result field.

These references create the demand-evaluated graph edges defined by Shape IR.
They are not ordinary node inputs. The referenced node MAY occur before or
after the containing node in the `nodes` array.

## 7. Helper records

### 7.1 Ordering items

An ordering item contains exactly:

| Member | JSON kind | Allowed value |
| --- | --- | --- |
| `expression` | expression | The scalar ordering expression. |
| `direction` | string | `ascending` or `descending`. |
| `null_placement` | string | `first`, `last`, or `not_applicable`. |

The items in one ordering array appear in lexicographic comparison order.
Shape IR validation checks descriptor and null-placement requirements.

### 7.2 Project entries

A project entry is one of these closed objects:

```json
{
  "kind": "keep",
  "field": "f0"
}
```

```json
{
  "kind": "compute",
  "output": "f1",
  "expression": {
    "kind": "field",
    "type": {
      "scalar": "int64",
      "nullable": false
    },
    "field": "f0"
  }
}
```

A `keep` entry contains exactly `kind` and `field`. A `compute` entry contains
exactly `kind`, `output`, and `expression`. `output` is the identity of the
fresh computed field at the corresponding output-schema position.

### 7.3 Grouping-key definitions

A grouping-key definition contains exactly:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `output` | field identifier | Fresh grouping-key output field. |
| `expression` | expression | Key expression over the input schema. |

### 7.4 Aggregate definitions

An aggregate definition contains:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `output` | field identifier | Required fresh result field. |
| `function` | string | Required aggregate function. |
| `argument` | expression | Conditional as described below. |

`function` is one of `count_all`, `count`, `sum`, `min`, `max`, `bool_and`,
or `bool_or`.

For `count_all`, `argument` MUST be absent. For every other function,
`argument` MUST be present. The object contains no other members.

### 7.5 Window definitions

A partitioned-aggregate window definition contains:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `kind` | string | Exactly `partitioned_aggregate`. |
| `output` | field identifier | Fresh result field. |
| `function` | string | One aggregate function from Section 7.4. |
| `argument` | expression | Absent for `count_all`; otherwise required. |
| `partition_by` | array | Ordered scalar partition expressions. |

It does not contain `order_by`.

A ranking window definition contains:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `kind` | string | Exactly `ranking`. |
| `output` | field identifier | Fresh result field. |
| `function` | string | `row_number`, `rank`, or `dense_rank`. |
| `partition_by` | array | Ordered scalar partition expressions. |
| `order_by` | array | Nonempty ordered list of ordering items. |

It does not contain `argument`.

Both `partition_by` arrays MAY be empty. Window-definition order determines
the order of appended result fields.

## 8. Common node encoding

Every node object contains:

| Member | JSON kind | Requirement |
| --- | --- | --- |
| `id` | node identifier | Required graph-wide identity. |
| `kind` | string | One node-kind tag listed below. |
| `output_schema` | array | Required ordered field descriptors. |
| `collection` | string | `bag` or `ordered`. |
| `metadata` | object | Optional non-semantic metadata. |

It also contains the kind-specific members in Sections 9 and 10 and no others.

The node kinds are:

- `input`;
- `empty`;
- `project`;
- `filter`;
- `join`;
- `aggregate`;
- `window`;
- `distinct`;
- `set`;
- `order`;
- `slice`; and
- `forget_order`.

Ordinary relational inputs use kind-specific `input`, `left`, and `right`
members. There is no redundant common input array.

`output_schema` and `collection` are required checked descriptors. A consumer
MUST derive them from the node kind and its inputs during Shape IR validation
and reject any mismatch. Their serialization does not make them trusted host
assertions.

Value keys and peer-constant sets are not serialized. A consumer derives them
from the graph under Shape IR rules. Producer-specific proofs, catalog
constraints, or physical uniqueness metadata MUST NOT be required to validate
a portable interchange graph.

## 9. Source, row-local, and join nodes

### 9.1 `input`

An `input` node adds exactly:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `binding` | nonempty string | Opaque host relation binding. |

It has no ordinary input member. Its `collection` MUST be `bag`.

The document declares only the expected schema and binding slot. The host's
mapping from the slot to a finite relation is not embedded in the document.

### 9.2 `empty`

An `empty` node has no kind-specific member and no ordinary input. Its
`collection` MUST be `bag`.

### 9.3 `project`

A `project` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary input node. |
| `entries` | array | Ordered project entries from Section 7.2. |

`entries` MAY be empty for a zero-field projection. Entry order MUST correspond
one-for-one with `output_schema` order. Shape IR validation checks keep and
compute field identities, descriptors, environments, and collection
preservation.

### 9.4 `filter`

A `filter` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary input node. |
| `predicate` | expression | Predicate over the input schema. |

### 9.5 `join`

A `join` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `left` | node identifier | Left ordinary input. |
| `right` | node identifier | Right ordinary input. |
| `join_type` | string | `cross`, `inner`, `left`, `right`, or `full`. |
| `condition` | expression | Conditional as described below. |

For `cross`, `condition` MUST be absent. For every other `join_type`,
`condition` MUST be present. Its left and right input roles are semantic and
are not affected by JSON object-member order.

## 10. Stateful and ordering nodes

### 10.1 `aggregate`

An `aggregate` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary input node. |
| `grouping_keys` | array | Ordered definitions from Section 7.3. |
| `aggregates` | array | Ordered definitions from Section 7.4. |

Either array MAY be empty. The grouping-key outputs followed by aggregate
outputs MUST correspond one-for-one with `output_schema`.

### 10.2 `window`

A `window` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary input node. |
| `definitions` | array | Nonempty window definitions from Section 7.5. |

Its `output_schema` begins with every input field and appends definition
outputs in array order.

### 10.3 `distinct`

A `distinct` node adds only:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary input node. |

### 10.4 `set`

A `set` node adds:

| Member | JSON kind | Allowed value |
| --- | --- | --- |
| `left` | node identifier | Left ordinary input. |
| `right` | node identifier | Right ordinary input. |
| `operation` | string | `union`, `intersect`, or `except`. |
| `quantifier` | string | `all` or `distinct`. |

Left and right roles and output-field ordinal correspondence are semantic.

### 10.5 `order`

An `order` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary input node. |
| `items` | array | Nonempty ordered list of ordering items. |

Its `collection` MUST be `ordered`.

### 10.6 `slice`

A `slice` node adds:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary ordered input. |
| `offset` | decimal string | `INT64` offset. |
| `limit` | decimal string or null | Optional `INT64` limit. |

Both members are always present. JSON `null` for `limit` represents no limit.
It is not a typed scalar `NULL`.

`offset` and a non-null `limit` use Section 4.3. A negative canonical value
maps successfully, then fails the non-negative Shape IR node invariant. The
node's `collection` MUST be `ordered`.

### 10.7 `forget_order`

A `forget_order` node adds only:

| Member | JSON kind | Meaning |
| --- | --- | --- |
| `input` | node identifier | Ordinary ordered input. |

Its `collection` MUST be `bag`.

## 11. Mapping and graph validation

### 11.1 Interchange mapping

After JSON decoding, a consumer MUST establish all of the following before an
abstract graph has been reconstructed:

- the top-level and every nested value has the required JSON kind;
- every required member is present exactly once;
- no prohibited or unknown member is present;
- every discriminated object uses a recognized `kind`;
- every enum uses a recognized exact spelling;
- every identity and decimal string is well formed;
- every type descriptor uses a recognized scalar type; and
- each conditionally present member matches its object variant.

A violation is an `interchange` error. Mapping does not yet establish that a
reference resolves or that a declared descriptor is semantically correct.

### 11.2 Shape IR validation

After mapping, the consumer MUST validate the complete abstract graph under
Shape IR 0.1. This includes:

- node and field identity uniqueness;
- reference resolution, reachability, and acyclicity;
- ordinary and demand-evaluated edge rules;
- schema and collection derivation;
- expression environments and descriptors;
- aggregate and window signatures;
- field definition and pass-through rules;
- value-key and peer-constant requirements; and
- every node-specific invariant.

These failures are `shape-ir-validation` errors. For example:

- `"offset": 0` is an `interchange` error because a JSON number cannot map to
  the decimal-string member;
- `"offset": "-1"` is a `shape-ir-validation` error because it maps to an
  `INT64` but violates the non-negative `slice` invariant;
- an unknown node `kind` is an `interchange` error because no node can be
  reconstructed; and
- an incorrect `output_schema` is a `shape-ir-validation` error because the
  checked annotation maps successfully but disagrees with node semantics.

A producer and consumer MAY validate more than one condition in one pass.
When a document has independent failures in multiple phases, this
specification does not select diagnostic text or require one particular
failure to be reported first.

### 11.3 Host input validation

Host binding occurs after graph validation. The host supplies one finite bag
for each required binding and establishes that it conforms to every
corresponding `input.output_schema`.

The interchange document does not define how bindings are located, secured,
snapshotted, transported, or authorized. A missing binding is a host failure,
not an `interchange` error.

### 11.4 JSON Schema

An implementation MAY provide a JSON Schema for editor support or partial
stage-2 validation. Such a schema is informative and MUST NOT replace this
document or Shape IR validation.

In particular, a general JSON Schema cannot by itself establish graph
reachability, acyclicity, field environments, derived output schemas, value
keys, or conditional descriptor rules. Passing an informative schema
therefore does not establish that a document represents valid Shape IR.

## 12. Versioning and compatibility

`interchange_version` and `shape_ir_version` are opaque version identifiers,
not decimal numbers and not ranges. Version `0.1` consumers MUST require exact
matches for both members. A different or malformed value cannot select the
version `0.1` mapping and is therefore an `interchange` error at this
interface.

A consumer MUST NOT:

- infer compatibility from a shared major or minor component;
- interpret a document under a different Shape IR version;
- ignore an unknown member, kind, operation, scalar type, or enum; or
- substitute a locally similar operation for an unknown one.

Unknown metadata is the only extension content that a version `0.1` consumer
accepts without a new specification version.

A future specification MAY:

- define another encoding of the same abstract interchange information;
- add a new Shape IR mapping;
- publish an explicit compatibility relation; or
- reserve semantic extension points.

Such a specification does not change version `0.1` reader behavior.

## 13. Equivalence and canonicalization

Shape IR Interchange 0.1 defines no canonical byte representation.

The following differences do not change the represented graph:

- permitted JSON whitespace;
- object-member order;
- valid escape spellings of the same string;
- node-array order; and
- consistent renaming of opaque node or field identities.

Metadata differences also do not change graph semantics.

This document does not require two semantically equivalent but structurally
different Shape IR graphs to have the same document. In particular, graph
sharing, optional reference-lowering nodes, identity allocation, and
semantics-preserving rewrites MAY differ.

Producers SHOULD provide a stable formatter for diagnostics, fixtures, and
version-control review. Stable formatting is not a conformance requirement,
and byte equality MUST NOT be used as a general test of graph equivalence.

Plan signing, content-addressed identities, and semantic graph hashing are
outside Shape IR Interchange 0.1. A future canonical profile would need both a
canonical JSON encoding and Shape-IR-specific rules for identities and node
ordering.

## 14. Worked example

This section is non-normative, but the JSON document is valid interchange
syntax.

Assume a host binding named `sales_input` supplies one non-nullable `INT64`
field. The graph below computes a fresh result field equal to that input plus
one:

```json
{
  "interchange_version": "0.1",
  "shape_ir_version": "0.1",
  "root": "n1",
  "nodes": [
    {
      "id": "n0",
      "kind": "input",
      "binding": "sales_input",
      "output_schema": [
        {
          "id": "f_amount",
          "name": "amount",
          "type": {
            "scalar": "int64",
            "nullable": false
          }
        }
      ],
      "collection": "bag"
    },
    {
      "id": "n1",
      "kind": "project",
      "input": "n0",
      "entries": [
        {
          "kind": "compute",
          "output": "f_result",
          "expression": {
            "kind": "binary",
            "type": {
              "scalar": "int64",
              "nullable": false
            },
            "operation": "add",
            "left": {
              "kind": "field",
              "type": {
                "scalar": "int64",
                "nullable": false
              },
              "field": "f_amount"
            },
            "right": {
              "kind": "literal",
              "type": {
                "scalar": "int64",
                "nullable": false
              },
              "value": "1"
            }
          }
        }
      ],
      "output_schema": [
        {
          "id": "f_result",
          "name": "amount",
          "type": {
            "scalar": "int64",
            "nullable": false
          }
        }
      ],
      "collection": "bag"
    }
  ]
}
```

The array happens to place the dependency first, but a consumer MUST also
accept the two node objects in the opposite order. The host-binding spelling
does not imply a table name or storage location.

## 15. Files, transport, and bundles

A standalone interchange file SHOULD use the suffix `.shapeir.json`. The
suffix is advisory; consumers establish format and version from the document
contents.

The same one-document encoding MAY be transported over a pipe, message, or
component API. Framing multiple documents in one stream is outside version
0.1 and MUST be supplied by the containing transport.

A production interchange document contains only the graph. A conformance or
evaluation bundle MAY separately associate it with:

- host-binding schemas and finite input rows;
- an expected result schema, bag, and ordering; or
- an expected `interchange`, `shape-ir-validation`, or `evaluation` error.

Those bundle members are corpus infrastructure and are not part of the graph
document.

## 16. Direct conformance fixtures

Direct Shape IR corpus cases use the `ir` member in place of a ShapeSQL
`source` member. Their expected outcomes distinguish:

- accepted interchange that reconstructs valid Shape IR;
- `interchange` errors in the JSON or structural mapping;
- `shape-ir-validation` errors in a reconstructed graph; and
- evaluation outcomes when finite host inputs are supplied separately.

The portable corpus SHOULD cover:

- every node kind, expression kind, and helper-record variant;
- forward references and graph sharing;
- demand-evaluated `exists` and `in_query` edges;
- minimum and maximum `INT64` values and typed null literals;
- empty and nonempty schemas and definition arrays;
- arbitrary accepted metadata;
- duplicate members, unknown semantic members, and malformed encodings;
- unsupported versions and enum values;
- missing, duplicate, cyclic, and unreachable references;
- invalid field environments and descriptors;
- incorrect schemas and collection kinds; and
- invalid value-key, peer-constant, aggregate, and window invariants.

Fixtures instantiate this document and Shape IR; they do not add semantics.
