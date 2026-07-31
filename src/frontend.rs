//! Owned parsing boundary for later semantic phases.

use std::sync::Arc;

use crate::ast::{Identifier, IdentifierKind, IntegerLiteral, Program, TextLiteral};
use crate::{Name, ParseError, parse};

/// A syntax tree paired inseparably with the UTF-8 source that its spans index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedProgram {
    source: Arc<str>,
    syntax: Program,
}

impl ParsedProgram {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn syntax(&self) -> &Program {
        &self.syntax
    }

    pub fn into_syntax(self) -> Program {
        self.syntax
    }

    /// Decodes and normalizes an identifier from this program's syntax tree.
    pub fn identifier_name(&self, identifier: Identifier) -> Name {
        let spelling = &self.source[identifier.span.start..identifier.span.end];
        match identifier.kind {
            IdentifierKind::Regular => Name::new(spelling.to_ascii_lowercase()),
            IdentifierKind::Delimited => {
                let inner = &spelling[1..spelling.len() - 1];
                Name::new(inner.replace("\"\"", "\""))
            }
        }
    }

    /// Returns the exact unsigned decimal spelling of an integer token.
    pub fn integer_spelling(&self, literal: IntegerLiteral) -> &str {
        &self.source[literal.span.start..literal.span.end]
    }

    /// Decodes a text literal from this program's syntax tree.
    pub fn text_value(&self, literal: TextLiteral) -> String {
        let spelling = &self.source[literal.span.start..literal.span.end];
        spelling[1..spelling.len() - 1].replace("''", "'")
    }
}

/// Parses one complete program and retains an owned copy of its source text.
pub fn parse_owned(source: &[u8]) -> Result<ParsedProgram, ParseError> {
    let syntax = parse(source)?;
    // Successful lexical analysis has already established valid UTF-8.
    let source = std::str::from_utf8(source)
        .expect("successful ShapeSQL lexing must establish UTF-8")
        .into();

    Ok(ParsedProgram { source, syntax })
}
