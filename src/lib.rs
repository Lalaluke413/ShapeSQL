//! Reference implementation of the ShapeSQL language.

mod lexer;

pub use lexer::{LexError, LexErrorKind, Span, Token, TokenKind, lex};
