# Shape IR

## 1. Overview

Shape IR 0.1 is the portable, statically typed relational representation of a
ShapeSQL query. It records the query's logical operations after lexical,
syntactic, binding, and typing analysis have succeeded.

Shape IR is deliberately higher-level than a physical execution plan. It
defines what a query computes, including multiplicity, null, ordering, and
error behavior. It does not select a join algorithm, storage layout, vector
width, partitioning scheme, or buffering policy.

A conforming ShapeSQL 0.1 front end MUST produce a valid Shape IR graph with
the result schema and observable behavior required by the source
specification. A conforming Shape IR 0.1 evaluator MUST accept every valid
Shape IR 0.1 graph and evaluate it according to this document.

ShapeSQL and Shape IR versions are independent. This document defines
**Shape IR 0.1**.

## 2. Design requirements

A Shape IR graph has the following properties:

- it is finite and acyclic;
- it has exactly one result root;
- every node and expression is statically typed;
- every field reference uses a field identity rather than a source name;
- every relational dependency is explicit;
- every stateful relational operation is represented by a distinct logical
  node;
- bag multiplicity, nullability, and required ordering are explicit; and
- evaluation errors remain observable under the rules in
  [Conventions and conformance](00-conventions.md#7-semantics-preserving-rewrites).

Shape IR MUST NOT contain:

- unresolved relation, qualifier, field, or alias names;
- wildcards;
- common-table-expression names;
- implicit casts;
- recursive edges;
- procedural control flow;
- physical algorithms or device-specific operations; or
- implementation-specific catalog, storage, or authorization actions.

Source locations and diagnostic annotations MAY be attached as non-semantic
metadata. Removing or changing that metadata MUST NOT change graph validity or
evaluation.

## 3. Graph model

### 3.1 Nodes, roots, and reachability

Every node has an identity unique within its graph. The graph identifies
exactly one node as its result root.

An edge exists when:

- a relational node consumes another node's output; or
- a scalar expression contains an `exists` or `in_query` operation that
  refers to another node.

The graph formed by both kinds of edge MUST be acyclic. Every node MUST be
reachable from the result root through those edges. An implementation MUST
reject a graph containing a cycle, a missing node reference, or an unreachable
node.

Reachability is structural, not a requirement for unconditional evaluation.
In particular, a node referenced only by an unselected `case` result remains
reachable and is validated, but its evaluation is not required for that
unselected result.

Graph sharing has no materialization meaning. Two consumers MAY refer to the
same node, but that does not require an evaluator to cache, recompute, or
physically store its output in any particular way.

### 3.2 Ordinary and demand-evaluated dependencies

An ordinary relational input edge is **strict**. When a node is evaluated, all
of its ordinary inputs are required, even when one input's contents would be
enough to determine the node's result bag. An implementation MUST NOT skip an
ordinary input when doing so would replace a required evaluation error with
success.

A node reference inside `exists` or `in_query` is **demand-evaluated**. The
referenced node is evaluated only when evaluation of that scalar expression is
required. Once demanded, the complete referenced result is required; finding
one row or one matching value MUST NOT suppress an evaluation error elsewhere
in that result.

These rules are semantic requirements, not required implementation
strategies. An implementation MAY evaluate, cache, or schedule work in any
way that preserves the same success-or-error outcome and successful result.

### 3.3 Schemas and field identities

Every node has one ordered output schema. A Shape IR field descriptor contains:

- a field identity;
- a display name, which MAY be empty;
- one scalar type; and
- a nullability property.

The scalar types and nullability properties are those defined by
[Types and type checking](08-types-and-type-checking.md). A node that defines a
new field MUST use an identity not defined by any other field in the graph.
A node that passes through a field MUST retain its identity, display name, and
scalar type.

Null extension by an outer join MAY widen a passed-through field from `T` to
`T?` while retaining its identity. No other node may change the descriptor of
a passed-through field.

Within one schema, every field identity MUST be unique. The two inputs of a
`join` MUST have disjoint field-identity sets. A front end that uses the same
subgraph for two relation occurrences can satisfy this requirement by placing
a `project` with fresh computed fields at each occurrence boundary.

Display names are result metadata only. They MUST NOT be used to resolve an
expression or establish graph validity.

### 3.4 Collection kinds

A node produces either:

- a **bag**, whose row order is not observable; or
- an **ordered collection**, whose row occurrences form a sequence.

An ordered collection retains bag multiplicities. Ordering changes sequence,
not membership or multiplicity.

`input` and `empty` produce bags. `filter` and `project` preserve their input
collection kind. `order` produces an ordered collection. `slice` consumes and
produces an ordered collection. `forget_order` converts an ordered collection
to a bag. Every other node defined by this document produces a bag, regardless
of its input collection kinds.

An order-insensitive node MAY consume an ordered collection, but it MUST NOT
depend on that sequence and its output order is discarded.

### 3.5 Value keys

A **value key** of a node is a set of output field identities with this
property:

> If two output row occurrences are not distinct on every field in the value
> key, they are not distinct across the complete output schema.

A value key does not imply occurrence uniqueness. Duplicate occurrences of
one complete row value are permitted.

The complete output field set is always a value key, including for a
zero-field schema. The grouping-key output fields of an `aggregate` are also a
value key. The empty grouping-key set is a value key for a global aggregate
because that node produces exactly one row.

`filter`, `order`, `slice`, and `forget_order` preserve their input value keys.
A `project` preserves an input value key when it keeps every field in that key.
Other value-key inference is optional in Shape IR 0.1. An implementation MUST
NOT rely on an inferred value key unless it can establish the defining
property above for every valid input.

Value keys are used only to prove that `row_number` and `slice` cannot choose
among rows having distinguishable values. They do not imply a catalog
constraint or a physical index.

## 4. Scalar expressions

### 4.1 Expression forms

Shape IR 0.1 contains the following scalar expression forms:

| Form | Contents |
| --- | --- |
| `literal` | A non-null scalar value, or a typed `NULL`. |
| `field` | One field identity from the expression's environment. |
| `unary` | Unary `+`, unary `-`, or `not`. |
| `binary` | Distinct left and right operands and one binary operation. |
| `is_null` | An operand and a Boolean `negated` property. |
| `case` | Ordered predicate-result arms and an explicit fallback. |
| `cast` | An operand and target scalar type. |
| `in_list` | A value and one or more scalar candidates. |
| `exists` | A referenced relational node. |
| `in_query` | A value, a one-field relational node, and its result field. |

The `binary` operations are:

- `add`, `subtract`, `multiply`, `divide`, and `remainder`;
- `concatenate`;
- `equal`, `not_equal`, `less`, `less_or_equal`, `greater`, and
  `greater_or_equal`; and
- `and` and `or`.

`NOT EXISTS` and both forms of `NOT IN` are represented by `not` applied to the
corresponding positive expression. Source parentheses, operator spelling, and
aliases do not appear in scalar IR.

Aggregate and ranking invocations are not scalar expression forms. They occur
only in `aggregate` and `window` node definitions. This separation prevents a
scalar expression from hiding cross-row state.

### 4.2 Expression descriptors and validation

Every scalar expression has exactly one scalar type and nullability
descriptor. A validator MUST derive and verify that descriptor using the
signatures and nullability rules in
[Types and type checking](08-types-and-type-checking.md).

A `NULL` literal in Shape IR is already resolved and MUST identify its scalar
type. Shape IR has no contextual or untyped null.

A `field` expression MUST refer to a field in the expression environment
defined by its containing node. Its descriptor is the descriptor of that field
in that environment.

The `case` fallback is always explicit in Shape IR. A source `CASE` without
`ELSE` lowers to a typed `NULL` fallback. A `case` MUST contain at least one
predicate-result arm. The predicates and result expressions follow the source
type and conditional-evaluation rules.

An `exists` expression returns non-nullable `BOOLEAN`. An `in_query` reference
MUST name the sole field in the referenced node's result schema and MUST have
the same scalar type as its left value. Its result nullability is determined
by the source type rules.

### 4.3 Expression evaluation

Scalar expressions use the value, null, error, and conditional-evaluation
semantics in the data model and core query semantics.

In particular:

- `and` and `or` require their left operand first and require their right
  operand only under the ordered conditional-evaluation rules;
- a strict operator still requires all operands when one operand is `NULL`;
- every `in_list` candidate is required;
- a demanded `exists` or `in_query` relation is evaluated completely; and
- `case` predicates are evaluated in order and only the selected result or
  fallback is evaluated.

An implementation MAY share a scalar expression representation. Sharing MUST
NOT cause an expression in an unselected `case` result or an unrequired `and`
or `or` right operand to be evaluated unconditionally.

## 5. Common node rules

Every node records:

- its node identity;
- its node kind and kind-specific properties;
- its ordinary input node identities;
- its ordered output schema; and
- its output collection kind.

The output schema and collection kind are not unchecked annotations. A
validator MUST derive the required values from the node kind and reject a
mismatch.

Unless a node rule says otherwise:

- every expression is evaluated in the ordered schema of that node's input;
- every required expression is evaluated for every applicable input row
  occurrence;
- an expression producing an evaluation error makes evaluation of the graph
  fail;
- input row multiplicities are preserved when the node emits a corresponding
  row; and
- all output fields introduced by the node use fresh field identities.

## 6. Relational nodes

### 6.1 `input`

An `input` node identifies one host-supplied relation and declares its expected
schema. Its relation binding is an opaque host identity, not a ShapeSQL source
name.

The host MUST supply a finite bag conforming exactly to the declared schema.
Supplying the snapshot, validating the host binding, and handling an
unavailable relation are host responsibilities outside Shape IR evaluation.

Two `input` nodes MAY use the same host relation binding. They MUST instantiate
fresh field identities so that their schemas remain distinguishable if they
are joined.

An `input` produces a bag.

### 6.2 `empty`

An `empty` node declares a schema and produces the empty bag of that schema.
Its field identities are fresh.

ShapeSQL 0.1 has no direct empty-relation syntax. The node exists so that
rewrites can represent a statically empty result without inventing a host
input. Introducing `empty` is semantics-preserving only when required
evaluation errors in the replaced subgraph cannot be suppressed.

### 6.3 `project`

A `project` has one input and an ordered list of output entries. Each entry is
one of:

- `keep(f)`, which passes through input field `f` with the same descriptor; or
- `compute(g, e)`, which evaluates expression `e` and defines fresh output
  field `g`.

An input field may appear in at most one `keep` entry. Producing two result
fields from one input field requires at least one fresh `compute` field.

Every `compute` expression uses the complete input schema as its environment.
It cannot refer to another field computed by the same `project`. All compute
expressions are required for every input row occurrence. The output schema is
the entry list in order.

A `project` preserves its input collection kind and the relative order of an
ordered input. For an ordered input, a kept field remains peer-constant when
the corresponding input field was peer-constant. A computed field is
peer-constant when every input field referenced by its expression was
peer-constant.

Source `SELECT` fields use `compute`, including a direct selected column,
because binding assigns fresh result identities. `keep` supports internal
field pruning and temporary fields without changing semantic identity.

### 6.4 `filter`

A `filter` has one input and one predicate evaluated in the input schema. The
predicate MUST have scalar type `BOOLEAN` and MAY be nullable.

The node preserves a row occurrence exactly when the predicate is `TRUE`.
`FALSE` and `UNKNOWN` remove it. The output schema is identical to the input
schema.

A `filter` preserves its input collection kind, relative sequence when
ordered, value keys, and peer-constant fields.

`WHERE` and `HAVING` use the same Shape IR node.

### 6.5 `join`

A `join` has left and right inputs and one of these kinds:

- `cross`;
- `inner`;
- `left`;
- `right`; or
- `full`.

A `cross` join has no condition. Every other kind has exactly one condition,
which MUST have scalar type `BOOLEAN`. The condition environment is the left
schema followed by the right schema.

The input field-identity sets MUST be disjoint. The output schema is the left
schema followed by the right schema, with these nullability changes:

- `left` makes every right field nullable;
- `right` makes every left field nullable; and
- `full` makes every field nullable.

The join emits rows and multiplicities according to
[Core query semantics](03-query-semantics.md#4-joins). Both ordinary inputs
are strict. The candidate-pair definition is mathematical and does not require
materializing a cross product or physically testing the condition once per
pair. An implementation MAY use any join method that preserves all matching
occurrences and required evaluation errors. For a qualified join, the
condition is semantically applied to every mathematical candidate pair, with
conditional demand within that condition determined by the scalar expression
rules. Finding one match does not permit remaining candidate pairs to be
discarded.

A `join` produces a bag and discards input ordering.

### 6.6 `aggregate`

An `aggregate` has:

- one input;
- an ordered list of grouping-key definitions; and
- an ordered list of aggregate definitions.

Each grouping-key definition contains a fresh output field and one scalar
expression over the input schema. Each aggregate definition contains a fresh
output field, an aggregate function, and its argument when required.

The aggregate functions are:

- `count_all`;
- `count`;
- `sum`;
- `min`;
- `max`;
- `bool_and`; and
- `bool_or`.

`count_all` has no argument. Every other function has exactly one argument
over the input schema. Functions, argument descriptors, result descriptors,
nullability, overflow, null treatment, and empty-input behavior are defined by
[Types and type checking](08-types-and-type-checking.md#114-aggregate-signatures)
and [Core query semantics](03-query-semantics.md#9-aggregates).

Grouping expressions and aggregate arguments are required for every input row
occurrence. Aggregate arguments are independent: one aggregate result becoming
known does not permit another argument evaluation to be skipped.

The output schema contains the grouping-key fields followed by the aggregate
fields. A later `project` arranges source result fields and computes scalar
expressions over these outputs.

With one or more grouping keys, the node emits one row for each not-distinct
key tuple and emits no rows for empty input. With no grouping keys, it emits
exactly one global-aggregate row, including for empty input.

The grouping-key output fields form a value key. An `aggregate` produces a bag
and discards input ordering.

### 6.7 `window`

A `window` has one input and a nonempty ordered list of window definitions. It
passes through every input field and appends one fresh result field for each
definition.

Each definition is one of:

- a partitioned `count_all`, `count`, `sum`, `min`, `max`, `bool_and`, or
  `bool_or`;
- `row_number`;
- `rank`; or
- `dense_rank`.

Every definition has an ordered list of partition expressions over the input
schema. A partitioned aggregate has its required argument, if any, and no
ordering. A ranking definition has a nonempty list of ordering items. Each
ordering item contains:

- one scalar expression over the input schema;
- `ascending` or `descending`; and
- `first`, `last`, or `not_applicable` null placement.

`not_applicable` is valid only for a non-nullable ordering expression. A
nullable ordering expression requires `first` or `last`.

Window definitions cannot refer to a result field introduced by the same
`window`. Every argument, partition expression, and ordering expression is
required for every input row occurrence to which its definition applies.

Partitioned aggregate values are defined by
[Core query semantics](03-query-semantics.md#13-partitioned-aggregates), and
ranking values by
[Core query semantics](03-query-semantics.md#14-ranking). For `row_number`,
the directly referenced fields among its ordering items MUST contain one value
key of the input. Parentheses, direction, and null placement do not affect
whether an item is a direct field reference. This permits peers only when
their complete input rows are not distinct.

A `window` produces a bag. Its internal partition ordering does not order the
node's output.

### 6.8 `distinct`

A `distinct` has one input. It emits one occurrence from every class of rows
that are not distinct across the complete input schema.

Its output schema is identical to its input schema. It produces a bag and does
not preserve input ordering.

### 6.9 `set`

A `set` has:

- left and right inputs;
- an operation: `union`, `intersect`, or `except`; and
- a quantifier: `all` or `distinct`.

The input schemas MUST have equal arity. Corresponding input fields MUST have
the same scalar type. Each output field has:

- a fresh identity;
- the left input field's display name;
- the common scalar type; and
- nullable status when either input field is nullable.

Input and output fields correspond by ordinal. Result multiplicities use the
applicable rule in
[Core query semantics](03-query-semantics.md#11-set-operations).

Both inputs are strict. A `set` produces a bag and discards input ordering.

### 6.10 `order`

An `order` has one input and a nonempty ordered list of ordering items. An item
has the expression, direction, and null-placement properties defined for a
window ordering item in Section 6.7.

Every ordering expression is required for every input row occurrence.
Ordering is lexicographic. Two rows are peers when every ordering expression
compares equally, including two `NULL` values at the same ordering position.
The relative sequence of peers is unspecified.

An `order` has the same output schema as its input and produces an ordered
collection. Its **peer-constant set** contains every input field used as a
direct `field` ordering expression. Other expressions do not establish
peer-constant input fields in Shape IR 0.1.

Direction and null placement affect sequence but do not affect the
peer-constant set.

### 6.11 `slice`

A `slice` has:

- one ordered input;
- a non-negative `INT64` offset, defaulting to zero; and
- an optional non-negative `INT64` limit.

The input's peer-constant set MUST contain at least one value key of the input.
This requirement ensures that rows within an unspecified peer sequence are
not distinct across the complete schema. Cutting through such a peer group can
change only which indistinguishable occurrences remain, not the result values
or multiplicities.

`filter` and `project` nodes between `order` and `slice` propagate
peer-constant fields as defined by their node rules. If no propagated value
key remains, the graph is invalid.

The node removes the first `offset` occurrences and then retains at most
`limit` occurrences when a limit is present. It has the same output schema as
its input and produces an ordered collection.

The complete ordinary input is semantically required. A physical executor
MUST NOT use the bound to suppress an upstream evaluation error.

### 6.12 `forget_order`

A `forget_order` has one ordered input and produces the same rows, schema, and
multiplicities as a bag. It discards only sequence metadata.

This node makes the boundary explicit when an ordered nested query is consumed
as an unordered relation source. Evaluating `forget_order` still requires its
ordered input, including every ordering expression and any resulting
evaluation error.

## 7. Result root

The result root's ordered schema is the graph's result schema.

The root produces an ordered collection exactly when the query result has an
observable `ORDER BY`. Otherwise it produces a bag. A graph whose source query
has no outermost `ORDER BY` MUST NOT expose an incidental internal ordering.

For an ordered result, conforming evaluators MUST emit the required sequence.
The relative sequence of peers remains unspecified, but a valid `slice`
ensures that peer selection cannot change the successful result values or
multiplicities.

## 8. Source-to-IR boundary

This document does not prescribe one lowering algorithm, but a conforming
front end must erase or make explicit every source-only construct.

In particular:

- relation and field names are replaced by resolved host and field identities;
- wildcards are expanded;
- source aliases and redundant parentheses are removed;
- `WHERE` and `HAVING` become `filter` nodes at their semantic stages;
- grouping invocations become `aggregate` definitions;
- partitioned and ranking invocations become `window` definitions;
- selected expressions become fresh `project` fields;
- `DISTINCT` and set operators become their corresponding nodes;
- ordering expressions not retained in the result MAY be represented by
  temporary computed fields that are removed by a later `project`;
- ordered nested queries use `forget_order` when their sequence is not
  consumed; and
- common table expressions and derived queries become ordinary subgraphs.

A common-table-expression name does not require a Shape IR node. A front end
MAY share or duplicate its subgraph, provided relation occurrences receive the
required fresh field identities and evaluation errors remain observable.

The exact translation of every accepted syntax form will be defined by the
separate source-to-IR lowering specification. That document may select a
canonical graph shape but MUST NOT change the valid node semantics in this
document.

## 9. Evaluation errors and rewrites

Shape IR evaluation either:

- succeeds with the root schema, bag, and required sequence; or
- fails with an evaluation error.

It does not produce a partial successful relation.

Arithmetic, casts, counts, aggregate sums, and all other error-capable
operations retain the evaluation behavior defined by the source
specification. A graph rewrite is valid only under the success-and-error
equivalence rule in
[Conventions and conformance](00-conventions.md#7-semantics-preserving-rewrites).

Consequently, an optimizer MUST NOT, without a sufficient proof:

- replace an error-capable expression with an unused value;
- remove an ordered nested query whose ordering expressions may fail;
- stop an aggregate, membership test, or relational predicate early;
- discard join candidate pairs or matching occurrences merely because another
  match was found, or bypass a join condition in a way that suppresses a
  required evaluation error;
- reorder, reassociate, or unconditionally evaluate `and` or `or` operands
  when doing so changes which error-capable operands are required;
- evaluate an unselected `case` result;
- evaluate a demand-evaluated subquery when its containing expression is not
  required; or
- use `slice` to avoid errors in rows outside its retained range.

Shape IR does not require a physical evaluation order when several required
operations fail. Diagnostic text and the choice among multiple simultaneously
applicable evaluation errors are not portable. This freedom does not override
the conditional-demand rules for `case`, `and`, or `or`. Returning success
when any required operation fails is never permitted.

## 10. Validation

Shape IR validation occurs before evaluation and requires no input row values.
A Shape IR 0.1 validator MUST reject at least:

- an unsupported Shape IR version;
- a missing, duplicate, unreachable, or cyclic node;
- a missing result root;
- a duplicate field definition or duplicate field identity within one schema;
- a field reference outside its expression environment;
- a join whose input field identities overlap;
- an incorrect output schema, field descriptor, or collection kind;
- an expression or node operation outside the Shape IR 0.1 closed set;
- an expression whose declared descriptor does not match its derived
  descriptor;
- an invalid aggregate or window signature;
- a scalar relational predicate with invalid result arity or field type;
- a nullable ordering expression without defined null placement;
- a `row_number` ordering that does not cover an input value key;
- a `slice` whose peer-constant fields do not cover an input value key; or
- any other violation of a node-specific invariant in this document.

Validation MUST inspect every reachable node and expression, including a
demand-evaluated subgraph or unselected `case` result.

A front end MUST classify source errors before Shape IR exists. It MUST NOT
emit an invalid graph and report a `shape-ir-validation` error for invalid
ShapeSQL source.

## 11. Worked example

This section is non-normative. The notation is illustrative and is not a
Shape IR serialization.

Assume `sales.department` is non-nullable `TEXT`, `sales.amount` is nullable
`INT64`, and `sales.active` is nullable `BOOLEAN`. Consider:

```sql
SELECT
    department,
    SUM(amount) AS total
FROM sales
WHERE active
GROUP BY department
HAVING SUM(amount) > 0
ORDER BY department ASC, total DESC NULLS LAST
LIMIT 10;
```

One valid graph shape is:

| Node | Logical contents |
| --- | --- |
| `n0 input` | Bind `sales`; define fresh source fields. |
| `n1 filter` | Input `n0`; predicate `field(active)`. |
| `n2 aggregate` | Input `n1`; key `department`; measure `sum(amount)`. |
| `n3 filter` | Input `n2`; predicate `sum_result > 0`. |
| `n4 project` | Define fresh result fields `department` and `total`. |
| `n5 order` | Order both result fields as written by the query. |
| `n6 slice` | Offset zero, limit ten; graph result root. |

The grouping key on `n2` is a value key of the aggregate output. The projection
on `n4` gives the query its bound result identities and display names. Because
`n5` directly orders both fields of `n4`, its peer-constant set contains the
complete result value key and `n6` is valid.

The graph says nothing about whether aggregation uses hashing or sorting,
whether `n1` and `n4` are fused into expression pipelines, or whether rows are
stored or streamed between nodes.

## 12. Physical lowering boundary

This section is non-normative.

Shape IR intentionally exposes the logical boundaries that a later execution
representation must preserve:

- `project` and `filter` describe row-local expression pipelines;
- `join`, `aggregate`, `distinct`, and `set` describe keyed or multiplicity
  state;
- `window` describes partitioned state and ranking;
- `order` describes global or partitioned ordering work; and
- `slice` and `forget_order` make sequence consumption explicit.

The lower representation must preserve:

- field scalar types and validity information;
- bag multiplicities;
- not-distinct key behavior where required;
- join-predicate three-valued logic;
- ordered peer semantics;
- conditional demand; and
- the evaluation-error channel.

Keeping those requirements explicit allows all
implementations to share one semantic query-plan contract without requiring
their physical plans to have the same nodes.

## 13. Versioning and interchange

Every portable graph MUST identify its Shape IR version. A consumer MUST reject
an unknown node, expression, scalar type, or semantic property in portable
mode rather than silently reinterpret it.

Shape IR 0.1 defines an abstract data model, not a textual or binary
serialization. An implementation may expose the model through native data
structures while claiming semantic conformance. A future interchange
specification will define stable encoding, identifier representation, and
direct Shape IR conformance fixtures.
