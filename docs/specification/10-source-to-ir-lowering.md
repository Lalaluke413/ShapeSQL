# Source-to-IR Lowering

## 1. Overview

This document defines the reference lowering from a successfully bound and
typed ShapeSQL 0.1 program to a valid Shape IR 0.1 graph.

Lowering receives a program for which:

- lexical, syntactic, binding, and typing analysis have succeeded;
- every relation occurrence and field reference has a resolved identity;
- every wildcard has been expanded;
- every expression has a scalar type and nullability descriptor; and
- every query expression has an ordered typed result schema.

Lowering MUST succeed for every such program. It MUST produce a graph whose
result schema and observable behavior are those of the source program. A
failure to lower an otherwise valid program is an implementation failure, not
a new source error phase.

This document specifies a reference graph shape so that every source construct
has one explicit interpretation. A conforming front end MAY emit a different
valid Shape IR graph only when the difference is a semantics-preserving
rewrite under
[Conventions and conformance](00-conventions.md#7-semantics-preserving-rewrites).

## 2. Reference-lowering model

### 2.1 Canonical structure and identity renaming

The reference lowering is canonical up to:

- renaming node identities;
- renaming fresh temporary field identities;
- the choice of opaque host relation-binding identities; and
- omission of a node that this document explicitly marks as optional.

Bound result-field identities and bound relation-occurrence field identities
MAY be reused as the corresponding Shape IR identities. If a front end uses a
different identity domain, it MUST preserve a one-to-one mapping for every
identity that remains live at that point in lowering.

Node identity allocation order has no semantic meaning. Temporary fields have
an empty display name unless this document says otherwise.

### 2.2 No optimization requirement

The reference lowering makes source stages explicit. It does not:

- choose physical algorithms;
- emit `empty`, which has no ShapeSQL 0.1 source form;
- push filters through other nodes;
- reorder joins;
- fold constants;
- prune fields except where this document requires a public schema boundary;
- decorrelate relational predicates; or
- infer that error-capable work is unnecessary from input values.

An implementation may perform such work as a later Shape IR rewrite when the
rewrite satisfies the success-and-error equivalence requirement.

### 2.3 Lowering result

Lowering one program produces:

- a finite set of Shape IR nodes;
- exactly one result root;
- the Shape IR version `0.1`; and
- no node that is unreachable from the result root through ordinary or scalar
  relational edges.

The root schema uses the program's bound result-field order, identities,
display names, scalar types, and nullability. The root collection kind is
determined by the ordering rules in Section 11.

## 3. Environments and helper operations

### 3.1 Field environment

A lowering environment maps each field identity visible in a bound expression
to the Shape IR field identity that currently carries its value.

The mapping is usually the identity mapping. It changes when:

- a common table expression or derived query is instantiated as a relation
  occurrence;
- a grouping expression is replaced by an `aggregate` grouping-key field;
- a grouping aggregate is replaced by an `aggregate` result field;
- a partitioned or ranking invocation is replaced by a `window` result field;
  or
- a set operation defines its fresh result fields.

Every lowered `field` expression MUST name a field in the schema that is the
expression environment of its containing Shape IR node.

### 3.2 Public and temporary fields

A **public field** is a field in the bound result schema of the query
expression currently being lowered.

A **temporary field** exists only to carry a value between logical stages.
Temporary fields include:

- grouping-key and aggregate-result fields before final projection;
- window-result fields before final projection;
- source fields carried through projection for a non-`DISTINCT` `ORDER BY`;
  and
- materialized ordering-key fields.

Temporary fields MUST be removed before the result of that query expression is
exposed to a containing source, set operation, relational predicate, or the
program result.

### 3.3 Consuming an ordered query as a bag

The helper operation **consume as bag** takes a lowered query result:

- if the result is a bag, it returns that node unchanged; and
- if the result is an ordered collection, it emits `forget_order` and returns
  the resulting bag.

The helper does not remove `order` or `slice`. It therefore preserves:

- evaluation errors from ordering expressions;
- the membership and multiplicity effects of a row bound; and
- every ordinary dependency of the nested query.

The reference lowering uses consume as bag whenever a query result is used as:

- a common-table-expression or derived relation source;
- an operand of a set operation; or
- the query of `EXISTS`, `IN`, or `NOT IN`.

### 3.4 Re-identifying a relation occurrence

The helper operation **instantiate occurrence** takes a bag-valued query result
and the ordered field list assigned by binding to one relation occurrence. It
emits a `project` containing one `compute` entry per source result field:

```text
compute(occurrence_field_i, field(source_result_field_i))
```

Fields correspond by ordinal. Each occurrence field is fresh, uses the
display name assigned to the source result field, and has the same type
descriptor. The project output contains only the occurrence fields.

This project is required even when the query result and occurrence schemas
otherwise appear identical. It ensures that two occurrences of one common
table expression or derived query have disjoint field identities.

## 4. Scalar expression lowering

### 4.1 Ordinary forms

After stage-owned aggregate and window invocations have been replaced as
defined in Sections 9 and 10, scalar syntax lowers as follows:

| Bound source form | Shape IR form |
| --- | --- |
| A non-null literal | `literal` containing its typed value. |
| A resolved `NULL` | `literal` containing typed `NULL`. |
| A column or result reference | `field` using the current field environment. |
| Parenthesized expression | Its lowered contained expression. |
| Unary `+` or `-` | `unary` with the corresponding operation. |
| `NOT e` | `unary(not, e)`. |
| Arithmetic, concatenation, or comparison | The corresponding `binary` operation. |
| `p AND q` | `binary(and, p, q)`. |
| `p OR q` | `binary(or, p, q)`. |
| `e IS NULL` | `is_null(e, negated = false)`. |
| `e IS NOT NULL` | `is_null(e, negated = true)`. |
| Searched `CASE` | `case` with ordered arms and an explicit fallback. |
| `CAST(e AS T)` | `cast(e, T)`. |
| `x IN (c1, ... cn)` | `in_list(x, [c1, ... cn])`. |
| `x NOT IN (c1, ... cn)` | `unary(not, in_list(x, [c1, ... cn]))`. |

The lowerer MUST copy the scalar type and nullability derived during typing
and MUST produce the expression form whose descriptor Shape IR validation
derives. It MUST NOT insert a cast that was not explicit in the source.

The successfully typed token sequence `-9223372036854775808` lowers to one
`INT64` literal containing the minimum value. It does not lower to unary
negation of a positive literal, because that positive magnitude is not an
`INT64` value and the source type rules define the complete sequence as the
special minimum-value case. Every other unary sign lowers normally.

A source `CASE` without `ELSE` receives a `literal` typed `NULL` fallback.
Source parentheses, aliases, token spelling, and an omitted `AS` do not appear
in Shape IR.

### 4.2 Tree shape and conditional demand

Scalar lowering MUST preserve:

- the left-associative tree produced for `AND` and `OR`;
- the distinct left and right operands of every binary operation;
- the source order of `CASE` arms;
- the separation between every `CASE` predicate and result; and
- the source order and multiplicity of `IN` list candidates.

In particular, lowering MUST NOT commute, reassociate, flatten, or reorder
`AND` or `OR`. Their right operands remain conditionally required according to
[Data model](02-data-model.md#9-expression-evaluation).

Lowering a scalar expression does not evaluate it. Constant operands do not
permit the lowerer to remove an error-capable operand or relational
dependency.

### 4.3 Relational predicates

For `EXISTS (Q)`, the lowerer:

1. lowers `Q` as an independent query expression with the common table
   expressions visible at that source position;
2. consumes its result as a bag; and
3. emits `exists` referring to that bag node.

`NOT EXISTS (Q)` emits `unary(not, exists(Q))`.

For `x IN (Q)`, the lowerer:

1. lowers `x` in the containing scalar environment;
2. lowers `Q` independently;
3. consumes `Q` as a bag; and
4. emits `in_query` referring to the sole result field of `Q`.

`x NOT IN (Q)` wraps that `in_query` in `unary(not, ...)`.

The query node remains connected through a demand-evaluated scalar edge.
The lowerer MUST NOT turn that edge into an ordinary input, pre-evaluate the
query, or replace `EXISTS` or `IN` with a join as part of reference lowering.

### 4.4 Stage-owned invocations

A grouping aggregate, partitioned aggregate, or ranking invocation does not
lower directly as a scalar form.

The owning query block extracts it into an `aggregate` or `window` definition
and replaces the scalar occurrence with a `field` expression. The replacement
field has the invocation's typed result descriptor.

An invocation is extracted even when it occurs inside:

- an unselected `CASE` result;
- a later `CASE` predicate;
- the conditionally skipped right operand of `AND` or `OR`; or
- another ordinary scalar operator.

Its cross-row work belongs to an earlier logical stage. Relational predicates
within the same conditional scalar regions are not extracted and retain their
demand-evaluated behavior.

## 5. Relation-source lowering

### 5.1 Catalog relation

Each bound occurrence of a catalog relation emits one `input` node.

The node:

- uses the catalog entry's opaque host relation binding;
- defines the fresh field identities assigned to that occurrence;
- preserves catalog field order and display names;
- uses the type descriptors visible for the relation before any containing
  outer join; and
- produces a bag.

A source alias changes only binding and does not appear in the node.
Two occurrences of the same catalog relation emit two `input` nodes in the
reference lowering.

### 5.2 Common table expression

The query of a referenced common table expression is lowered once for the
declaration. A source occurrence that resolves to the declaration:

1. obtains that lowered query result;
2. consumes it as a bag; and
3. instantiates the occurrence as defined in Section 3.4.

The reference lowering shares that declaration subgraph among its occurrences,
but each occurrence has its own re-identifying `project`. Graph sharing has the
non-materializing meaning defined by Shape IR. A conforming alternative may
duplicate the declaration subgraph only under the rewrite rule in Section 1.

### 5.3 Derived source

A derived source is lowered in the same manner as a common-table-expression
occurrence:

1. lower its contained query expression;
2. consume the result as a bag; and
3. instantiate the bound derived-source occurrence.

The required source alias has already affected binding. It does not appear in
Shape IR.

### 5.4 Explicit and comma joins

A `joined_table` is lowered from left to right.

For each explicit join:

1. lower the current left operand;
2. lower the right `table_primary`;
3. lower the `ON` expression, when present, in the left schema followed by
   the right schema; and
4. emit the corresponding `join` node.

`CROSS JOIN` emits `cross` without a condition. An omitted join type emits
`inner`. `LEFT`, `RIGHT`, and `FULL` emit their corresponding join kinds.
`OUTER` has no separate representation.

After each outer join, the output descriptors are widened exactly as required
by Shape IR. The field identities themselves are retained, so later bound
references continue to identify the same fields with their now-visible
nullable descriptors.

The completed `joined_table` operands in a comma-separated `FROM` clause are
combined from left to right with `cross` join nodes. The final node's schema is
therefore every source occurrence in left-to-right `FROM` order.

The lowerer MUST NOT reorder joins or replace a qualified join by another
operation during reference lowering.

## 6. Common table expressions and reachability

Every common table expression has already been bound and typed, including an
unreferenced declaration. Lowering does not repeat or weaken that static
analysis.

The reference lowering recursively lowers a declaration only when a source
occurrence reachable from the program result refers to it. A declaration may
refer to a later declaration because binding has established an acyclic
dependency graph.

An unreferenced declaration emits no Shape IR node. Emitting its subgraph
would make those nodes unreachable, while attaching it through an ordinary
edge would incorrectly make its evaluation errors observable.

When a declaration is referenced, every node in its query that is structurally
reachable from the declaration result is included. This includes a relational
predicate inside an unselected `CASE` result even though that predicate remains
demand-evaluated.

## 7. Query bodies and set operations

### 7.1 Query primary

A `select_query` is lowered according to Sections 8 through 10.

Parentheses around a `query_expression` do not by themselves emit a node. The
contained query is lowered with its own `WITH`, `ORDER BY`, and row-bound
clauses.

If a parenthesized query is the only primary in its containing query body, its
collection kind passes through unchanged. This permits, for example, a
top-level program consisting of parentheses around an ordered query to retain
that observable order.

### 7.2 Set-operation tree

The parsed precedence and associativity have already produced a binary
set-operation tree. The lowerer visits that tree without reassociation.

For each binary operation:

1. lower the left query body;
2. consume the left result as a bag;
3. lower the right query body;
4. consume the right result as a bag; and
5. emit one `set` node with the bound output fields.

The operation and quantifier map as follows:

| Source operator | Shape IR operation | Quantifier |
| --- | --- | --- |
| `UNION ALL` | `union` | `all` |
| `UNION` | `union` | `distinct` |
| `INTERSECT ALL` | `intersect` | `all` |
| `INTERSECT` | `intersect` | `distinct` |
| `EXCEPT ALL` | `except` | `all` |
| `EXCEPT` | `except` | `distinct` |

Each set node defines the fresh result-field identities assigned by binding.
Fields correspond by ordinal; display names come from the left input, and
typed descriptors are those derived for the set result.

Set inputs remain strict. An empty or result-determining operand does not
permit the other operand or an internal ordering operation to be removed.

## 8. Select-query lowering

### 8.1 Stage sequence

One bound `select_query` is lowered in this sequence:

| Source stage | Reference Shape IR |
| --- | --- |
| `FROM` | `input`, occurrence `project`, and `join` nodes. |
| `WHERE` | Optional `filter`. |
| Grouping and grouping aggregates | Optional `aggregate`. |
| `HAVING` | Optional `filter`. |
| Partitioned and ranking operations | Optional `window`. |
| `SELECT` | `project`. |
| `DISTINCT` | Optional `distinct`. |

The outer `ORDER BY` and row bound belong to the containing
`query_expression` and are lowered afterward as defined in Sections 10 and
11. For a direct, non-`DISTINCT` select query, lowering of that outer ordering
is coupled to projection so that permitted source values remain available.

### 8.2 `FROM` and `WHERE`

The `FROM` clause is lowered according to Section 5. Its output schema is the
bound source environment of the query block.

When `WHERE` is present, its expression is lowered in that environment and a
`filter` is emitted. When `WHERE` is absent, no node is emitted.

The current schema remains the complete `FROM` schema. The lowerer MUST NOT
prune an unselected source or source field before work that can observe its
evaluation errors has been preserved.

### 8.3 Aggregate-query classification

The lowerer uses the aggregate-query classification already established by
typing. It MUST NOT reclassify the query after constant folding or another
rewrite.

For a non-aggregate query, no `aggregate` node is emitted and the post-`WHERE`
schema becomes the pre-window schema.

For an aggregate query, the lowerer emits exactly one reference `aggregate`
node as defined in Section 9. Its input is the post-`WHERE` node.

### 8.4 `HAVING`

When `HAVING` is present:

1. replace grouping expressions and grouping aggregates with their
   `aggregate` output fields;
2. lower the resulting scalar expression in the aggregate output schema; and
3. emit `filter`.

Typing guarantees that `HAVING` is present only when the query has an
`aggregate` node. The filter preserves the aggregate node's value keys.

### 8.5 Window stage

After `HAVING`, the lowerer extracts every partitioned aggregate and ranking
invocation owned by the query block as defined in Section 10.

If at least one invocation is present, one `window` node containing all
reference definitions is emitted. Otherwise no window node is emitted.

### 8.6 Projection and `DISTINCT`

After aggregate and window substitutions, each expanded or explicit select
item lowers to:

```text
compute(bound_result_field, lowered_select_expression)
```

Entries appear in bound result-field order. A wildcard-expanded column is a
direct `field` compute, not a `keep`, because the result field has the fresh
identity assigned by binding.

If wildcard expansion produces a zero-field result schema, this `project` has
an empty entry list. It still preserves one zero-field output occurrence for
each input occurrence and preserves multiplicity.

Without the source-support case in Section 11.2, the `project` output is
exactly the public result schema.

When `DISTINCT` is present, `distinct` is emitted immediately after that
public projection. No source-support or ordering-key field may be present in
the input to `distinct`, because duplicate elimination is defined only over
the selected result fields.

## 9. Grouping and aggregate extraction

### 9.1 Canonical grouping keys

Grouping expressions are considered using the structural-equality rule from
type checking. Redundant parentheses do not distinguish expressions.

The reference lowering creates one grouping-key definition for the first
occurrence of each structurally distinct `GROUP BY` expression, in source
order. A later structurally equal grouping expression maps to the same output
field.

Coalescing duplicate grouping expressions is required by the reference
lowering because:

- duplicate expressions cannot refine the not-distinct grouping partition;
- every occurrence has the same value and success-or-error outcome for every
  input row; and
- the unique grouping-key fields form the value key needed by a lowered
  grouped `ROW_NUMBER`.

This coalescing is specific to structurally equal expressions. The lowerer
MUST NOT use algebraic equivalence, catalog constraints, or input statistics
to merge other grouping expressions.

Each canonical grouping-key definition:

- lowers its expression in the post-`WHERE` source schema;
- defines a fresh field with that expression's descriptor; and
- appears before every aggregate-result field in the output schema.

A global aggregate has no grouping-key definitions.

### 9.2 Grouping aggregate collection

The lowerer collects every grouping aggregate invocation in:

1. select items from left to right; then
2. `HAVING`, when present.

Within one scalar expression, collection uses source-order preorder traversal.
Binary operands are visited left then right. A `CASE` visits each predicate
and result in arm order, then its fallback. An `IN` list visits its left value
and then each candidate. The traversal visits every `CASE` arm and both
operands of `AND` and `OR` without applying scalar conditional demand.
It does not enter the query of `EXISTS` or query-form `IN`; the nested query
owns and lowers its own aggregate invocations.

Each syntactic invocation creates one aggregate definition, even when another
invocation is structurally equal. Definitions retain encounter order.

The mapping is:

| Source invocation | Shape IR function |
| --- | --- |
| `COUNT(*)` | `count_all` with no argument |
| `COUNT(e)` | `count` with lowered argument `e` |
| `SUM(e)` | `sum` with lowered argument `e` |
| `MIN(e)` | `min` with lowered argument `e` |
| `MAX(e)` | `max` with lowered argument `e` |
| `BOOL_AND(e)` | `bool_and` with lowered argument `e` |
| `BOOL_OR(e)` | `bool_or` with lowered argument `e` |

Arguments are lowered in the post-`WHERE` source schema. Each definition
introduces one fresh field with the invocation's typed result descriptor.

The aggregate node output contains the canonical grouping-key fields followed
by the aggregate-result fields.

### 9.3 Post-group substitution

Every expression evaluated after grouping is rewritten recursively before
ordinary scalar lowering:

1. if the complete current subtree is a grouping aggregate occurrence,
   replace it with that occurrence's aggregate-result field;
2. otherwise, if the complete current subtree is structurally equal to a
   `GROUP BY` expression, replace it with the corresponding canonical
   grouping-key field;
3. otherwise, recursively rewrite its scalar children.

The whole-subtree grouping match occurs before descending. For example,
grouping by `a + b` permits the selected expression `(a + b) * 2`; the
`a + b` subtree becomes one grouping-key field before multiplication is
lowered.

After this substitution, no source field may remain in a post-group
expression except inside an independently lowered relational subquery.
Successful typing guarantees this invariant.

### 9.4 Earlier-stage demand

All aggregate definitions are ordinary parts of one strict `aggregate` node.
Consequently, an aggregate syntactically located in an unselected `CASE`
result or a skipped Boolean right operand is still computed at the grouping
stage.

Conditional demand inside an aggregate argument remains intact. For example,
the right operand of `p AND q` within `SUM(CASE ...)` or another aggregate
argument is lowered without reassociation and is required per input row only
under the scalar `AND` rules.

## 10. Partitioned and ranking extraction

### 10.1 Invocation sites

The lowerer collects partitioned aggregate and ranking invocations from:

1. select items from left to right; and
2. the containing outer `ORDER BY` items from left to right when that ordering
   is attached directly to this unparenthesized, non-`DISTINCT`
   `select_query`.

A window invocation introduced only in the outer `ORDER BY` of
`SELECT DISTINCT` is a typing error. A window result may instead be selected,
participate in duplicate elimination, and then be referenced through its
result field by `ORDER BY`.

Collection within an expression uses the source-order preorder traversal from
Section 9.2 and visits conditionally unselected scalar regions. Each
syntactic invocation creates one window definition.
It likewise does not enter an independently nested query.

### 10.2 Definition lowering

For every definition, the lowerer first applies post-group substitution when
the owning query is an aggregate query. It then lowers:

- the aggregate argument, except for `COUNT(*)`;
- partition expressions in source order; and
- ranking ordering expressions in source order.

All of those expressions use the post-`HAVING` schema as their Shape IR
environment.

Partitioned aggregate functions map to `count_all`, `count`, `sum`, `min`,
`max`, `bool_and`, or `bool_or`. `ROW_NUMBER`, `RANK`, and `DENSE_RANK` map to
`row_number`, `rank`, and `dense_rank`.

Source defaults are normalized as follows:

- omitted direction becomes `ascending`;
- `ASC` becomes `ascending`;
- `DESC` becomes `descending`;
- omitted null placement on a non-nullable expression becomes
  `not_applicable`; and
- explicit `NULLS FIRST` or `NULLS LAST` becomes `first` or `last`.

Each definition introduces one fresh result field. The single reference
`window` node passes through its complete input schema and appends definitions
in encounter order.

### 10.3 Scalar replacement

After the node is formed, each invocation occurrence in a select or outer
ordering expression is replaced by `field(window_result)`. The lowerer does
not descend into the invocation again during ordinary scalar lowering.

An invocation inside an unselected `CASE` result or skipped Boolean right
operand is therefore computed by the earlier `window` node. Relational
predicates in those scalar regions remain demand-evaluated.

### 10.4 `ROW_NUMBER` validity

For a non-aggregate query, the direct fields required by source typing are the
complete visible `FROM` schema, which is a value key of the window input.

For a grouped query, post-group substitution turns every required grouping
expression in the window ordering into a direct canonical grouping-key field.
Those fields form a value key of the aggregate output and remain a value key
through `HAVING`.

For a global aggregate, the aggregate node has the empty value key.

The resulting `row_number` definition therefore satisfies Shape IR
validation without catalog key inference.

## 11. Result ordering and row bounds

### 11.1 Ordering input

The outer `ORDER BY` is lowered after its query body.

When the body is not one direct, unparenthesized, non-`DISTINCT`
`select_query`, every bound ordering reference denotes a public result field.
The body is first consumed as a bag if it is already ordered by a nested query.

For a direct `SELECT DISTINCT`, ordering also begins from the public output of
`distinct`. Every ordering expression is over public result fields.

For a direct, non-`DISTINCT` `select_query`, an ordering expression may combine:

- public result-field references;
- fields from the query's post-window schema; and
- window results introduced by that ordering expression.

For an aggregate query, grouping expressions in the ordering expression have
already undergone post-group substitution. Every window invocation has
likewise been replaced by its window-result field before projection support is
computed.

The source-support projection below preserves the association between each
public result occurrence and the pre-projection values that produced it.

### 11.2 Source-support projection

For the direct non-`DISTINCT` case, Section 10 first extracts and replaces
every select-list or ordering-only window invocation. The lowerer then lowers
the select and ordering expressions in their respective field environments.

The select projection emits, in this order:

1. one `compute` entry for every public result field; and
2. one `keep` entry for every post-window field referenced by a lowered
   ordering expression and not already represented by a public result
   identity.

Support fields appear in current input-schema order and each is kept at most
once. They are temporary. A result-field reference in an ordering expression
continues to use the fresh public result field, so a selected expression is
not recomputed merely because its alias or ordinal is ordered.

If no ordering expression needs a post-window field, the select projection
contains only public results.

### 11.3 Ordering-key materialization

Each bound order item is normalized independently.

Binding has already replaced an ordering ordinal with its public result-field
identity and resolved every alias or source reference. Redundant expression
parentheses are erased during scalar lowering.

If its lowered expression is exactly `field(public_result)`, the corresponding
Shape IR order item uses that public field directly.

Otherwise, the lowerer:

1. emits a fresh hidden field computing the complete ordering expression; and
2. makes the Shape IR order item a direct reference to that hidden field.

All hidden ordering fields are computed together by a `project` whose output
contains:

1. every public result field as `keep`, in result order; then
2. one hidden `compute` field for each non-direct order item, in order-item
   order.

That project drops every source-support field. If there are no source-support
or hidden ordering fields, it MAY be omitted.

Materializing a complete order expression does not change its conditional
scalar tree. It makes every order item a direct field reference while ensuring
that all ordering expressions are required for every applicable row before
sorting.

### 11.4 `order`, cleanup, and `slice`

The lowerer emits one `order` node using the direct public or hidden key field
for each source order item. Direction and null placement are copied from the
normalized item.

Immediately after `order`, a `project` removes every hidden ordering field and
keeps the public result fields in bound result order. This cleanup project MAY
be omitted when the order input already has exactly the public result schema.
Because `project` preserves collection kind, the cleaned result remains
ordered.

When `LIMIT` or `OFFSET` is present, `slice` is emitted after cleanup:

- omitted `OFFSET` becomes zero;
- the typed `OFFSET` literal becomes the offset;
- an omitted `LIMIT` becomes no limit; and
- the typed `LIMIT` literal becomes the limit.

Source clause order does not change this mapping. `OFFSET` always applies
before `LIMIT`.

Typing requires every public result field to occur as a direct outer order
item when a row bound is present. Every hidden field is also a direct Shape IR
order item by construction. Before cleanup, the complete order-input schema is
therefore peer-constant; after cleanup, the complete public schema remains
peer-constant. The `slice` input consequently contains its complete-schema
value key in its peer-constant set.

### 11.5 No outer ordering

When no outer `ORDER BY` is present, no ordering support or key fields are
created.

A select or set query produces a bag. A singleton parenthesized query body may
pass through an ordered contained query as described in Section 7.1. At the
program boundary, that contained ordering remains observable because no
relation consumer has discarded it.

## 12. Complete query-expression algorithm

The reference lowering of one `query_expression` is:

1. establish its bound `WITH` declaration environment;
2. lower its query body using the parsed set-operation tree;
3. when the body is one direct, unparenthesized, non-`DISTINCT` select query,
   allow its select lowerer to collect ordering-only windows and carry
   source-support fields;
4. if an outer `ORDER BY` is present, lower it and any row bound according to
   Section 11;
5. otherwise expose the body result without temporary fields; and
6. return the resulting node, public schema, and collection kind to the
   consumer.

When the consumer requires a bag, it applies consume as bag after the complete
query expression has been lowered. It does not discard ordering before an
inner `slice` has selected the query's rows.

## 13. Required evaluation and error preservation

Reference lowering MUST preserve the distinction between ordinary and
demand-evaluated work.

In particular, it MUST:

- include every reachable `FROM` source even when projection uses none of its
  fields;
- keep both set operands and both join inputs as ordinary dependencies;
- retain an inner `order` even when a containing operation ignores its
  sequence;
- place every grouping aggregate before `HAVING`, windows, and projection;
- place every partitioned or ranking operation before projection and
  `DISTINCT`;
- preserve the scalar tree of `CASE`, `AND`, and `OR`;
- leave `exists` and `in_query` as demand-evaluated scalar edges;
- evaluate every `IN` list candidate when the `in_list` expression is
  demanded;
- compute every outer ordering expression for every row presented to
  `order`; and
- place `slice` after the complete ordered input.

Reference lowering MUST NOT:

- remove work because `LIMIT 0`, a constant predicate, an empty relation, or
  one set operand appears to determine the successful result;
- move an aggregate or window invocation back into scalar IR;
- hoist a relational predicate out of a conditional scalar region;
- use a selected `CASE` arm to decide which aggregate or window definitions
  exist;
- stop at the first join match or membership match; or
- make an unreferenced common table expression reachable solely to evaluate
  it.

These requirements constrain observable success and error, not a physical
schedule.

## 14. Worked examples

This section is non-normative. Field and node notation is illustrative.

### 14.1 Hidden source ordering

Assume `items.a` and `items.b` are non-nullable `INT64`:

```sql
SELECT a + 1 AS value
FROM items
ORDER BY value ASC, items.b DESC
LIMIT 3;
```

One reference graph shape is:

| Node | Logical contents |
| --- | --- |
| `n0 input` | Bind `items`; define occurrence fields `a0`, `b0`. |
| `n1 project` | Compute public `value1 = a0 + 1`; keep support `b0`. |
| `n2 project` | Keep `value1`; compute hidden `key2 = b0`. |
| `n3 order` | Order by `value1 ASC`, then `key2 DESC`. |
| `n4 project` | Keep only public `value1`. |
| `n5 slice` | Offset zero, limit three; result root. |

The selected expression is computed once. The qualified source field remains
associated with its occurrence until its ordering key is materialized. The
hidden key does not enter the result schema.

### 14.2 Conditional scalar demand across relational stages

Assume `sales.department` is non-nullable `TEXT`, `sales.amount` is nullable
`INT64`, and `audit.audit_id` is non-nullable `INT64`:

```sql
SELECT
    department,
    CASE
        WHEN FALSE AND EXISTS (SELECT audit_id FROM audit)
        THEN SUM(amount)
        ELSE 0
    END AS total,
    ROW_NUMBER() OVER (ORDER BY department) AS position
FROM sales
GROUP BY department;
```

The important reference stages are:

| Stage | Lowering consequence |
| --- | --- |
| Grouping | Define canonical `department` and `sum(amount)` fields. |
| Window | Compute `row_number` ordered by the grouping-key field. |
| Projection | Evaluate the `CASE` using aggregate and window result fields. |
| Scalar demand | `FALSE AND ...` does not demand the `EXISTS` query. |

`SUM(amount)` is still computed because it belongs to the earlier grouping
stage even though its `CASE` result is not selected. The `EXISTS` subgraph is
present and validated, but its demand-evaluated edge is skipped by the
left-to-right `AND` rule.

### 14.3 Common-table-expression occurrences

```sql
WITH active AS (
    SELECT id
    FROM users
    WHERE enabled
)
SELECT l.id, r.id
FROM active AS l
JOIN active AS r ON l.id = r.id;
```

The `active` query is one shared subgraph in the reference lowering. The `l`
and `r` occurrences each receive a separate re-identifying `project`, so their
output field sets are disjoint and the join condition identifies one field
from each occurrence.

## 15. Lowering completion checks

Before returning a graph, a front end MUST establish that:

- the graph contains exactly the nodes reachable from its result root;
- all source names, aliases, wildcards, ordinals, and parentheses have been
  erased or resolved;
- every field reference is valid in its containing expression environment;
- every relation occurrence has its bound fresh field identities;
- no aggregate or window invocation remains in scalar IR;
- no temporary field appears in an exposed query result schema;
- every nested ordering is either observable or explicitly consumed as a bag;
- the root collection kind matches the source query's observable ordering;
- the root schema is the bound typed program result schema; and
- ordinary versus demand-evaluated dependencies preserve the source rules.

The resulting graph MUST pass Shape IR 0.1 validation. A front end MUST NOT
report a `shape-ir-validation` error for the source program if its own emitted
graph violates one of these requirements.

## 16. Version boundary

This document defines only the mapping from ShapeSQL 0.1 to Shape IR 0.1.
A future source or IR version may add syntax, node forms, or a different
canonical normalization. A front end MUST identify both versions and MUST NOT
silently lower a construct using rules from another version.
