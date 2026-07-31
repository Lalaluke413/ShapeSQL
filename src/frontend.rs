//! Owned parsing boundary for later semantic phases.

use std::sync::Arc;

use crate::ast::{Identifier, IdentifierKind, IntegerLiteral, Program, TextLiteral};
use crate::shape_ir::Graph;
use crate::{BindError, Catalog, Name, ParseError, TypeError, bind, lower, parse, type_check};

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

/// A successfully parsed program that failed semantic analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    Binding(BindError),
    Typing(TypeError),
}

impl std::fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Binding(error) => std::fmt::Display::fmt(error, formatter),
            Self::Typing(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for AnalysisError {}

/// A lexical, syntactic, binding, or typing failure during compilation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompileError {
    Parsing(ParseError),
    Binding(BindError),
    Typing(TypeError),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parsing(error) => std::fmt::Display::fmt(error, formatter),
            Self::Binding(error) => std::fmt::Display::fmt(error, formatter),
            Self::Typing(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for CompileError {}

/// Runs the separate binding and type-checking phases.
pub fn analyze(
    parsed: &ParsedProgram,
    catalog: &Catalog,
) -> Result<crate::hir::TypedProgram, AnalysisError> {
    let bound = bind(parsed, catalog).map_err(AnalysisError::Binding)?;
    type_check(bound).map_err(AnalysisError::Typing)
}

/// Compiles one ShapeSQL source program into valid Shape IR 0.1.
pub fn compile(source: &[u8], catalog: &Catalog) -> Result<Graph, CompileError> {
    let parsed = parse_owned(source).map_err(CompileError::Parsing)?;
    let bound = bind(&parsed, catalog).map_err(CompileError::Binding)?;
    let typed = type_check(bound).map_err(CompileError::Typing)?;
    Ok(lower(typed))
}
