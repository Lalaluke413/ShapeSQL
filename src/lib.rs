//! Reference implementation of the ShapeSQL language.

pub mod ast;
mod bind;
mod catalog;
mod evaluation;
mod frontend;
pub mod hir;
mod identity;
pub mod interchange;
mod lexer;
mod lower;
mod name;
mod parser;
pub mod shape_ir;
mod typecheck;
mod types;

pub use bind::{BindError, BindErrorKind, bind};
pub use catalog::{Catalog, CatalogField, CatalogRelation, RelationBinding};
pub use evaluation::{
    EvaluateError, EvaluationError, EvaluationErrorKind, EvaluationResult, InputError,
    InputErrorKind, InputField, InputRelation, Row, Snapshot, SnapshotError, Value, evaluate,
};
pub use frontend::{AnalysisError, CompileError, ParsedProgram, analyze, compile, parse_owned};
pub use identity::{CteId, FieldId, RelationOccurrenceId};
pub use lexer::{LexError, LexErrorKind, Span, Token, TokenKind, lex};
pub use lower::lower;
pub use name::Name;
pub use parser::{ParseError, SyntaxError, parse};
pub use typecheck::{TypeError, TypeErrorKind, type_check};
pub use types::{ScalarType, TypeDescriptor};
