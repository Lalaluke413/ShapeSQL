# Lexical Structure

## 1. Overview

This document defines how a ShapeSQL source text is divided into tokens.
The grammar defines how those tokens form a program. An implementation may use
any parsing technique that accepts exactly the programs defined by the lexical
and grammatical rules.

## 2. Source encoding

ShapeSQL source MUST be valid UTF-8. An invalid UTF-8 byte sequence is a lexical
error.

The Unicode byte-order mark U+FEFF MAY appear once at the beginning of a source
text and is ignored there. U+FEFF anywhere else is not whitespace.

Source text MUST NOT contain U+0000.

## 3. Separators

The following characters are whitespace:

- U+0009 CHARACTER TABULATION;
- U+000A LINE FEED;
- U+000C FORM FEED;
- U+000D CARRIAGE RETURN; and
- U+0020 SPACE.

Whitespace separates tokens but is otherwise insignificant. It may appear
before the first token, after the last token, and between any two tokens. It
does not separate characters within a token.

Comments have the same separating effect as whitespace:

- A line comment begins with `--` and extends up to the next U+000A LINE FEED,
  U+000D CARRIAGE RETURN, or the end of the source text.
- A block comment begins with `/*` and extends through the next `*/`. Block
  comments do not nest.

An unterminated block comment is a lexical error.

At least one separator is REQUIRED between adjacent tokens when their
characters would otherwise form one longer token. For example, `SELECTa` is
one identifier, not the keyword `SELECT` followed by the identifier `a`.

## 4. Token classes

After separators are removed, every source character MUST belong to exactly
one token. ShapeSQL has these token classes:

- keywords;
- regular and delimited identifiers;
- integer, text, Boolean, and `NULL` literals;
- operators; and
- punctuation.

When more than one token could begin at the same position, the lexer MUST
consume the longest valid token. Keyword recognition is applied to a complete
regular identifier token, not to a prefix of one.

An unrecognized source character is a lexical error.

## 5. Keywords

Keywords are ASCII words defined by the grammar. Keyword matching is
case-insensitive: `select`, `SELECT`, and `SeLeCt` denote the same keyword.

The grammar identifies which keywords are reserved. A reserved keyword MUST
NOT be used as a regular identifier. It MAY be used as a delimited identifier.

## 6. Identifiers

### 6.1 Regular identifiers

A regular identifier:

1. begins with an ASCII letter (`A` through `Z` or `a` through `z`) or
   underscore (`_`); and
2. continues with zero or more ASCII letters, decimal digits (`0` through
   `9`), or underscores.

Regular identifiers are case-insensitive. For name comparison, every ASCII
letter in a regular identifier is folded to lowercase. The identifiers
`employee`, `Employee`, and `EMPLOYEE` therefore denote the same name.

### 6.2 Delimited identifiers

A delimited identifier begins and ends with a double quote (`"`). Its content
MAY contain any Unicode scalar value permitted in source text except an
unescaped double quote. Two consecutive double quotes inside the identifier
represent one double quote in its value.

Delimited identifiers are case-sensitive and are not case-folded. A delimited
identifier MUST contain at least one Unicode scalar value after doubled quotes
are decoded.

An unterminated delimited identifier is a lexical error.

## 7. Literals

### 7.1 Integer literals

An integer literal is one or more ASCII decimal digits. A leading `+` or `-`
is a separate unary operator, not part of the literal token.

[Types and type checking](08-types-and-type-checking.md#42-integer-literals)
defines range checking, including the treatment of the minimum `INT64` value.

### 7.2 Text literals

A text literal begins and ends with a single quote (`'`). Its content MAY
contain any Unicode scalar value permitted in source text except an unescaped
single quote. Two consecutive single quotes inside the literal represent one
single quote in its value.

Backslash has no special meaning. Adjacent text literals are separate tokens
and are not implicitly concatenated.

An unterminated text literal is a lexical error.

### 7.3 Boolean and null literals

`TRUE`, `FALSE`, and `NULL` are case-insensitive keyword tokens with their
corresponding literal meanings.

## 8. Operators and punctuation

ShapeSQL 0.1 uses the following symbolic tokens:

| Token | Role |
| --- | --- |
| `(`, `)` | grouping and syntactic delimiters |
| `,` | list separator |
| `.` | qualification |
| `;` | program terminator |
| `+`, `-`, `*`, `/`, `%` | arithmetic or wildcard |
| `=` | equality |
| `<>`, `<`, `<=`, `>`, `>=` | comparison |
| `\|\|` | text concatenation |

The grammar determines where each token is valid. `!=` is not a ShapeSQL 0.1
token.

## 9. Lexical errors

The following are lexical errors:

- invalid UTF-8;
- U+0000 in source text;
- an unrecognized source character;
- an unterminated block comment;
- an unterminated delimited identifier; or
- an unterminated text literal.

An implementation MAY implement lexical and syntactic analysis as one
operation. It MUST nevertheless classify an error in this list as lexical
rather than syntactic.
