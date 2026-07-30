use std::fmt;

/// A half-open byte range in the UTF-8 source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

/// A lexical token recognized by ShapeSQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

/// The token classes and reserved keywords defined by ShapeSQL 0.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Identifier,
    DelimitedIdentifier,
    IntegerLiteral,
    TextLiteral,

    All,
    And,
    As,
    Asc,
    Boolean,
    BoolAnd,
    BoolOr,
    By,
    Case,
    Cast,
    Count,
    Cross,
    DenseRank,
    Desc,
    Distinct,
    Else,
    End,
    Except,
    Exists,
    False,
    First,
    From,
    Full,
    Group,
    Having,
    In,
    Inner,
    Int64,
    Intersect,
    Is,
    Join,
    Last,
    Left,
    Limit,
    Max,
    Min,
    Not,
    Null,
    Nulls,
    Offset,
    On,
    Or,
    Order,
    Outer,
    Over,
    Partition,
    Rank,
    Right,
    RowNumber,
    Select,
    Sum,
    Text,
    Then,
    True,
    Union,
    When,
    Where,
    With,

    LeftParenthesis,
    RightParenthesis,
    Comma,
    Dot,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Concatenate,
}

impl TokenKind {
    fn from_keyword(word: &str) -> Option<Self> {
        macro_rules! keyword {
            ($($spelling:literal => $kind:ident),* $(,)?) => {
                $(
                    if word.eq_ignore_ascii_case($spelling) {
                        return Some(Self::$kind);
                    }
                )*
            };
        }

        keyword! {
            "ALL" => All,
            "AND" => And,
            "AS" => As,
            "ASC" => Asc,
            "BOOLEAN" => Boolean,
            "BOOL_AND" => BoolAnd,
            "BOOL_OR" => BoolOr,
            "BY" => By,
            "CASE" => Case,
            "CAST" => Cast,
            "COUNT" => Count,
            "CROSS" => Cross,
            "DENSE_RANK" => DenseRank,
            "DESC" => Desc,
            "DISTINCT" => Distinct,
            "ELSE" => Else,
            "END" => End,
            "EXCEPT" => Except,
            "EXISTS" => Exists,
            "FALSE" => False,
            "FIRST" => First,
            "FROM" => From,
            "FULL" => Full,
            "GROUP" => Group,
            "HAVING" => Having,
            "IN" => In,
            "INNER" => Inner,
            "INT64" => Int64,
            "INTERSECT" => Intersect,
            "IS" => Is,
            "JOIN" => Join,
            "LAST" => Last,
            "LEFT" => Left,
            "LIMIT" => Limit,
            "MAX" => Max,
            "MIN" => Min,
            "NOT" => Not,
            "NULL" => Null,
            "NULLS" => Nulls,
            "OFFSET" => Offset,
            "ON" => On,
            "OR" => Or,
            "ORDER" => Order,
            "OUTER" => Outer,
            "OVER" => Over,
            "PARTITION" => Partition,
            "RANK" => Rank,
            "RIGHT" => Right,
            "ROW_NUMBER" => RowNumber,
            "SELECT" => Select,
            "SUM" => Sum,
            "TEXT" => Text,
            "THEN" => Then,
            "TRUE" => True,
            "UNION" => Union,
            "WHEN" => When,
            "WHERE" => Where,
            "WITH" => With,
        }

        None
    }
}

/// A reason that source text cannot be divided into ShapeSQL tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexErrorKind {
    InvalidUtf8,
    NullCharacter,
    UnexpectedCharacter(char),
    UnterminatedBlockComment,
    UnterminatedDelimitedIdentifier,
    EmptyDelimitedIdentifier,
    UnterminatedTextLiteral,
}

/// A lexical error and the byte range that caused it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LexError {
    pub kind: LexErrorKind,
    pub span: Span,
}

impl LexError {
    const fn new(kind: LexErrorKind, start: usize, end: usize) -> Self {
        Self {
            kind,
            span: Span::new(start, end),
        }
    }
}

impl fmt::Display for LexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            LexErrorKind::InvalidUtf8 => write!(formatter, "source is not valid UTF-8"),
            LexErrorKind::NullCharacter => write!(formatter, "source contains U+0000"),
            LexErrorKind::UnexpectedCharacter(character) => {
                write!(formatter, "unexpected character {character:?}")
            }
            LexErrorKind::UnterminatedBlockComment => {
                write!(formatter, "unterminated block comment")
            }
            LexErrorKind::UnterminatedDelimitedIdentifier => {
                write!(formatter, "unterminated delimited identifier")
            }
            LexErrorKind::EmptyDelimitedIdentifier => {
                write!(formatter, "delimited identifier is empty")
            }
            LexErrorKind::UnterminatedTextLiteral => {
                write!(formatter, "unterminated text literal")
            }
        }
    }
}

impl std::error::Error for LexError {}

/// Divides a complete ShapeSQL source text into tokens.
///
/// Token spans use half-open UTF-8 byte offsets. Whitespace, comments, and an
/// optional initial byte-order mark do not produce tokens.
pub fn lex(source: &[u8]) -> Result<Vec<Token>, LexError> {
    let source = std::str::from_utf8(source).map_err(invalid_utf8_error)?;

    if let Some(offset) = source.bytes().position(|byte| byte == 0) {
        return Err(LexError::new(
            LexErrorKind::NullCharacter,
            offset,
            offset + 1,
        ));
    }

    Scanner::new(source).scan()
}

fn invalid_utf8_error(error: std::str::Utf8Error) -> LexError {
    let start = error.valid_up_to();
    let end = error
        .error_len()
        .map_or(start.saturating_add(1), |length| start + length);

    LexError::new(LexErrorKind::InvalidUtf8, start, end)
}

struct Scanner<'source> {
    source: &'source str,
    bytes: &'source [u8],
    offset: usize,
}

impl<'source> Scanner<'source> {
    fn new(source: &'source str) -> Self {
        let offset = if source.starts_with('\u{feff}') {
            '\u{feff}'.len_utf8()
        } else {
            0
        };

        Self {
            source,
            bytes: source.as_bytes(),
            offset,
        }
    }

    fn scan(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();

        while self.offset < self.bytes.len() {
            self.skip_separators()?;

            if self.offset < self.bytes.len() {
                tokens.push(self.scan_token()?);
            }
        }

        Ok(tokens)
    }

    fn skip_separators(&mut self) -> Result<(), LexError> {
        loop {
            while self.current_byte().is_some_and(is_whitespace) {
                self.offset += 1;
            }

            if self.starts_with(b"--") {
                self.skip_line_comment();
            } else if self.starts_with(b"/*") {
                self.skip_block_comment()?;
            } else {
                return Ok(());
            }
        }
    }

    fn skip_line_comment(&mut self) {
        self.offset += 2;

        while let Some(byte) = self.current_byte() {
            if matches!(byte, b'\n' | b'\r') {
                break;
            }

            self.offset += 1;
        }
    }

    fn skip_block_comment(&mut self) -> Result<(), LexError> {
        let start = self.offset;
        self.offset += 2;

        while self.offset < self.bytes.len() {
            if self.starts_with(b"*/") {
                self.offset += 2;
                return Ok(());
            }

            self.offset += 1;
        }

        Err(LexError::new(
            LexErrorKind::UnterminatedBlockComment,
            start,
            self.offset,
        ))
    }

    fn scan_token(&mut self) -> Result<Token, LexError> {
        let start = self.offset;
        let first = self.bytes[start];

        if is_identifier_start(first) {
            return Ok(self.scan_identifier(start));
        }

        if first.is_ascii_digit() {
            return Ok(self.scan_integer(start));
        }

        match first {
            b'"' => self.scan_delimited_identifier(start),
            b'\'' => self.scan_text_literal(start),
            b'(' => Ok(self.single_byte_token(start, TokenKind::LeftParenthesis)),
            b')' => Ok(self.single_byte_token(start, TokenKind::RightParenthesis)),
            b',' => Ok(self.single_byte_token(start, TokenKind::Comma)),
            b'.' => Ok(self.single_byte_token(start, TokenKind::Dot)),
            b';' => Ok(self.single_byte_token(start, TokenKind::Semicolon)),
            b'+' => Ok(self.single_byte_token(start, TokenKind::Plus)),
            b'-' => Ok(self.single_byte_token(start, TokenKind::Minus)),
            b'*' => Ok(self.single_byte_token(start, TokenKind::Star)),
            b'/' => Ok(self.single_byte_token(start, TokenKind::Slash)),
            b'%' => Ok(self.single_byte_token(start, TokenKind::Percent)),
            b'=' => Ok(self.single_byte_token(start, TokenKind::Equal)),
            b'<' => Ok(self.scan_less_operator(start)),
            b'>' => Ok(self.scan_greater_operator(start)),
            b'|' if self.starts_with(b"||") => {
                Ok(self.double_byte_token(start, TokenKind::Concatenate))
            }
            _ => Err(self.unexpected_character(start)),
        }
    }

    fn scan_identifier(&mut self, start: usize) -> Token {
        self.offset += 1;

        while self.current_byte().is_some_and(is_identifier_continue) {
            self.offset += 1;
        }

        let word = &self.source[start..self.offset];
        let kind = TokenKind::from_keyword(word).unwrap_or(TokenKind::Identifier);

        Token {
            kind,
            span: Span::new(start, self.offset),
        }
    }

    fn scan_integer(&mut self, start: usize) -> Token {
        self.offset += 1;

        while self
            .current_byte()
            .is_some_and(|byte| byte.is_ascii_digit())
        {
            self.offset += 1;
        }

        Token {
            kind: TokenKind::IntegerLiteral,
            span: Span::new(start, self.offset),
        }
    }

    fn scan_delimited_identifier(&mut self, start: usize) -> Result<Token, LexError> {
        self.offset += 1;
        let mut has_value = false;

        while self.offset < self.bytes.len() {
            if self.current_byte() == Some(b'"') {
                if self.byte_at(self.offset + 1) == Some(b'"') {
                    has_value = true;
                    self.offset += 2;
                } else {
                    self.offset += 1;

                    if !has_value {
                        return Err(LexError::new(
                            LexErrorKind::EmptyDelimitedIdentifier,
                            start,
                            self.offset,
                        ));
                    }

                    return Ok(Token {
                        kind: TokenKind::DelimitedIdentifier,
                        span: Span::new(start, self.offset),
                    });
                }
            } else {
                has_value = true;
                self.offset += 1;
            }
        }

        Err(LexError::new(
            LexErrorKind::UnterminatedDelimitedIdentifier,
            start,
            self.offset,
        ))
    }

    fn scan_text_literal(&mut self, start: usize) -> Result<Token, LexError> {
        self.offset += 1;

        while self.offset < self.bytes.len() {
            if self.current_byte() == Some(b'\'') {
                if self.byte_at(self.offset + 1) == Some(b'\'') {
                    self.offset += 2;
                } else {
                    self.offset += 1;

                    return Ok(Token {
                        kind: TokenKind::TextLiteral,
                        span: Span::new(start, self.offset),
                    });
                }
            } else {
                self.offset += 1;
            }
        }

        Err(LexError::new(
            LexErrorKind::UnterminatedTextLiteral,
            start,
            self.offset,
        ))
    }

    fn scan_less_operator(&mut self, start: usize) -> Token {
        let kind = match self.byte_at(start + 1) {
            Some(b'>') => TokenKind::NotEqual,
            Some(b'=') => TokenKind::LessEqual,
            _ => return self.single_byte_token(start, TokenKind::Less),
        };

        self.double_byte_token(start, kind)
    }

    fn scan_greater_operator(&mut self, start: usize) -> Token {
        if self.byte_at(start + 1) == Some(b'=') {
            self.double_byte_token(start, TokenKind::GreaterEqual)
        } else {
            self.single_byte_token(start, TokenKind::Greater)
        }
    }

    fn single_byte_token(&mut self, start: usize, kind: TokenKind) -> Token {
        self.offset += 1;

        Token {
            kind,
            span: Span::new(start, self.offset),
        }
    }

    fn double_byte_token(&mut self, start: usize, kind: TokenKind) -> Token {
        self.offset += 2;

        Token {
            kind,
            span: Span::new(start, self.offset),
        }
    }

    fn unexpected_character(&self, start: usize) -> LexError {
        let character = self.source[start..]
            .chars()
            .next()
            .expect("a token starts before the end of source");

        LexError::new(
            LexErrorKind::UnexpectedCharacter(character),
            start,
            start + character.len_utf8(),
        )
    }

    fn current_byte(&self) -> Option<u8> {
        self.byte_at(self.offset)
    }

    fn byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(offset).copied()
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.bytes[self.offset..].starts_with(prefix)
    }
}

fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}
