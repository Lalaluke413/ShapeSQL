//! Reference implementation of the ShapeSQL language.

pub mod ast;
mod bind;
mod catalog;
mod frontend;
pub mod hir;
mod identity;
mod lexer;
mod name;
mod parser;
mod typecheck;
mod types;

pub use bind::{BindError, BindErrorKind, bind};
pub use catalog::{Catalog, CatalogField, CatalogRelation, RelationBinding};
pub use frontend::{ParsedProgram, parse_owned};
pub use identity::{CteId, FieldId, RelationOccurrenceId};
pub use lexer::{LexError, LexErrorKind, Span, Token, TokenKind, lex};
pub use name::Name;
pub use parser::{ParseError, SyntaxError, parse};
pub use typecheck::{TypeError, TypeErrorKind, type_check};
pub use types::{ScalarType, TypeDescriptor};
