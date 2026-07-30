# Binding

## 1. Overview

Binding resolves the names in a syntactically valid ShapeSQL program and
assigns stable identities to relation occurrences and fields. It operates
after parsing and before static typing.

Binding receives:

- a syntax tree accepted by the ShapeSQL grammar; and
- a host-supplied catalog containing the names and schemas of available input
  relations.

Successful binding produces a bound program in which:

- every named relation source resolves to one common table expression or
  catalog relation;
- every column reference resolves to one field identity;
- every wildcard is expanded;
- every query result has an ordered output field list with identities and
  display names;
- every common-table-expression dependency is acyclic; and
- no nested query refers to a field from an enclosing query block.

A violation of a rule in this document is a `binding` error. Binding does not
determine operator types, expression result types, nullability, or
set-operation type compatibility; those are typing concerns.

## 2. Identifier comparison

Names are compared using the identifier values produced by lexical analysis.
ASCII letters in a regular identifier are folded to lowercase. A delimited
identifier is decoded and compared exactly, without case folding.

Consequently, the regular identifiers `item`, `Item`, and `ITEM` compare equal
to each other and to the delimited identifier `"item"`. They do not compare
equal to `"Item"`.

Names introduced by ShapeSQL source use the decoded value of their identifier
token. Catalog relation names and catalog field display names are already name
values and are not case-folded by binding. A regular source identifier
therefore matches a lowercase catalog name, while a differently cased catalog
name requires an exactly matching delimited identifier.

These comparison rules apply uniformly to:

- catalog relation names;
- common table expression names;
- relation-source aliases;
- field names; and
- result-field aliases.

## 3. Namespaces

Binding uses three namespaces:

- the **relation namespace** contains common table expressions and catalog
  relations that may be used as named sources;
- the **source namespace** contains relation occurrences visible within one
  query block and their qualifiers; and
- the **result namespace** contains the fields produced by a query expression.

The namespaces are distinct. For example, a select-item alias does not create
a relation name or a source qualifier.

Duplicate names are permitted in a result namespace. They are not permitted
where this document requires a name to identify exactly one declaration.

## 4. Relation-name binding

### 4.1 Catalog relations

The host catalog MUST make each available portable ShapeSQL relation
addressable by one identifier and MUST provide its ordered field schema.
Authorization is a host concern: a relation unavailable to the submitted
program is treated as absent.

A named source that does not resolve to a visible common table expression
MUST resolve to exactly one catalog relation. No match is an unresolved
relation error. More than one match is an ambiguous relation error.

### 4.2 Common table expressions

The names declared by one `WITH` clause form one relation namespace. Duplicate
common table expression names in that clause are a binding error.

Every name declared by the clause is visible:

- within every common table expression query in that clause; and
- within the query body governed by the clause.

A name in the innermost applicable `WITH` clause takes precedence over an
equal name from an enclosing `WITH` clause or the host catalog. This rule also
applies within the defining query of that common table expression. A
self-reference therefore resolves to the current declaration and forms a
dependency cycle; it does not fall through to an outer declaration or catalog
relation.

Common table expression queries MAY refer to declarations written later in
the same `WITH` clause. After relation names are resolved, the dependency
graph among declarations in one clause MUST be acyclic. A direct or indirect
cycle is a binding error.

A common table expression exposes the ordered result field list of its query.
ShapeSQL 0.1 has no common-table-expression column-name list.

## 5. Query blocks and source visibility

A `select_query` creates one query block. Binding its `FROM` clause creates
one relation occurrence for every named or derived source.

A relation occurrence has:

- one source qualifier; and
- one ordered sequence of visible fields instantiated from the source's
  schema.

For a named source without an alias, its qualifier is the identifier written
as the relation name. For any source with an alias, its qualifier is the alias.
The alias replaces the original qualifier; it does not add a second way to
qualify the source. A derived source always uses its required alias.

Every source qualifier in one query block MUST be unique. This rule applies
even when no qualified column reference uses the duplicate name.

A derived source is not lateral. Its query is bound without the source
namespace of the containing query block. The derived source exposes only its
result fields; qualifiers used inside it are not visible outside it.

### 5.1 Join visibility

An `ON` expression may refer to:

- every source occurrence in the join's completed left operand; and
- the source occurrence introduced by that join's right operand.

It MUST NOT refer to a source introduced by a later join or by a later
comma-separated `FROM` item.

After the complete `FROM` clause has been bound, all its source occurrences
are visible to `WHERE`, `GROUP BY`, `HAVING`, window expressions, and the
`SELECT` list.

### 5.2 Nested query boundaries

A nested query may use common table expression names visible through its
relation namespace, but it MUST NOT bind a column reference to a field from an
enclosing query block.

This prohibition applies to:

- common table expression queries;
- derived sources; and
- queries used by `EXISTS`, `IN`, or `NOT IN`.

Because ShapeSQL 0.1 has no correlated query form, a column reference that
would require an enclosing query block is unresolved and is a binding error.

## 6. Column-reference binding

An unqualified column reference is matched against the visible fields of
every source occurrence in its source namespace:

- exactly one matching field resolves the reference;
- no matching field is an unresolved column error; and
- more than one matching field is an ambiguous column error.

A qualified column reference is resolved in two steps:

1. its qualifier MUST match exactly one visible source occurrence; and
2. its field name MUST match exactly one field of that occurrence.

No qualifier match is an unresolved qualifier error. Multiple qualifier
matches are prevented by the source-qualifier uniqueness rule. Within the
selected occurrence, no field match is an unresolved column error and
multiple field matches are an ambiguous column error.

Duplicate display names in a source schema are therefore permitted, but a
column reference cannot select one of those fields by name. A wildcard may
still select all of them.

## 7. Select-item aliases

A select-item alias determines the display name of that result field. It does
not create a name visible to:

- another expression in the same `SELECT` list;
- `ON`;
- `WHERE`;
- `GROUP BY`;
- `HAVING`; or
- a window `PARTITION BY` or window `ORDER BY`.

Select-item aliases are visible when binding the query expression's outer
`ORDER BY` as defined in Section 10.

`GROUP BY` does not interpret an integer literal as an ordinal and does not
resolve select-item aliases. Every grouping expression is bound against the
query block's source namespace using the ordinary expression rules.

## 8. Wildcard expansion

Wildcard expansion occurs during binding before the result schema is
finalized.

An unqualified `*` expands to every field of every source occurrence in
left-to-right `FROM` order. Within an occurrence, fields retain source-schema
order.

A qualified wildcard expands to every field of the exactly matching source
occurrence in source-schema order. An unresolved qualifier is a binding error.

Expansion occurs at the wildcard's position in the `SELECT` list. It preserves
duplicate field names. Expanding a source with zero fields contributes zero
result fields and is not itself an error.

## 9. Result schemas and display names

After wildcard expansion, each select item contributes one result field for
each expanded or explicit expression.

The display name of a result field is:

- the select-item alias, when present;
- the referenced field's display name for an unaliased expression consisting
  only of a column reference and any surrounding parentheses;
- the source field's display name for a wildcard-expanded field; or
- the empty string for any other unaliased expression.

The empty display name is portable and observable, but it cannot be named by a
ShapeSQL identifier. Authors SHOULD provide an alias when a computed result
field will be referenced by an enclosing query.

`DISTINCT` does not change the result schema.

A set operation's bound result field list has the arity and display names of
its left input. Binding does not require its right input to have the same
arity. Equal arity, common types, and result nullability are required during
typing.

## 10. `ORDER BY` binding

`ORDER BY` is bound after the ordered query body's result namespace is known.

An order item consisting solely of an unparenthesized integer literal denotes
a one-based result-field ordinal. The ordinal MUST be between `1` and the
number of result fields, inclusive. Otherwise the program has a binding error.
An integer literal elsewhere in an ordering expression is an ordinary scalar
literal.

Each unqualified column reference in any other ordering expression is resolved
as follows:

1. if exactly one result field has the requested display name, the reference
   resolves to that result field;
2. if multiple result fields have that display name, the reference is
   ambiguous and binding fails; or
3. if no result field matches and the ordered query body is one direct,
   unparenthesized `select_query`, the reference is resolved against that
   query block's source namespace.

If no result field matches and no source namespace is available, the reference
is unresolved.

A qualified reference in an ordering expression is resolved only against the
source namespace of a direct, unparenthesized `select_query`. Result fields
have no source qualifiers. An ordered set operation or parenthesized query
therefore permits only references resolvable through its result namespace.

Result fields take precedence over same-named source fields. This makes a
select-item alias usable both as a complete order item and within a larger
ordering expression.

## 11. Field identity

Names are used only to select declarations during binding. The bound program
and Shape IR refer to fields by identity.

Each relation occurrence receives fresh visible field identities, in source
schema order. Two occurrences of the same catalog relation or common table
expression therefore have distinct field identities.

Each result field produced by projection or a set operation also receives a
fresh identity. A bound expression records the identities of its input fields;
renaming a source or result field does not change which input identity an
expression denotes.

Wildcard expansion records the corresponding source field identity for each
expanded expression, then assigns a fresh identity to each produced result
field.

## 12. Binding error summary

The following are binding errors:

- an unresolved or ambiguous relation name;
- duplicate common table expression names in one `WITH` clause;
- a direct or indirect common table expression dependency cycle;
- duplicate source qualifiers in one query block;
- an unresolved or ambiguous qualifier or column reference;
- a column reference that would require correlation;
- an unresolved qualified wildcard;
- an ambiguous result-field reference in `ORDER BY`; or
- an `ORDER BY` ordinal outside the result schema.

The diagnostic phase is normative. Diagnostic codes and text are not.
