use shapesql::{LexErrorKind, Span, TokenKind, lex};

fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source.as_bytes())
        .expect("source should lex successfully")
        .into_iter()
        .map(|token| token.kind)
        .collect()
}

#[test]
fn accepts_empty_and_separator_only_source_as_empty_token_streams() {
    assert!(kinds("").is_empty());
    assert!(kinds(" \t\r\n\x0c-- comment").is_empty());
}

#[test]
fn lexes_a_complete_query() {
    assert_eq!(
        kinds("SELECT employee_id, \"Display Name\" FROM employees WHERE active = TRUE;"),
        [
            TokenKind::Select,
            TokenKind::Identifier,
            TokenKind::Comma,
            TokenKind::DelimitedIdentifier,
            TokenKind::From,
            TokenKind::Identifier,
            TokenKind::Where,
            TokenKind::Identifier,
            TokenKind::Equal,
            TokenKind::True,
            TokenKind::Semicolon,
        ]
    );
}

#[test]
fn recognizes_every_reserved_keyword_case_insensitively() {
    let source = "\
        all and as asc boolean bool_and bool_or by case cast count cross \
        dense_rank desc distinct else end except exists false first from full \
        group having in inner int64 intersect is join last left limit max min \
        not null nulls offset on or order outer over partition rank right \
        row_number select sum text then true union when where with";

    let tokens = lex(source.as_bytes()).expect("keywords should lex successfully");

    assert_eq!(tokens.len(), 58);
    assert!(
        tokens
            .iter()
            .all(|token| token.kind != TokenKind::Identifier)
    );
}

#[test]
fn recognizes_keywords_only_after_scanning_the_complete_identifier() {
    assert_eq!(
        kinds("SELECT selecta _FROM FROM_"),
        [
            TokenKind::Select,
            TokenKind::Identifier,
            TokenKind::Identifier,
            TokenKind::Identifier,
        ]
    );
}

#[test]
fn skips_the_initial_bom_whitespace_and_comments() {
    assert_eq!(
        kinds("\u{feff}\t-- first comment\r\n/* second comment */ SELECT"),
        [TokenKind::Select]
    );
}

#[test]
fn block_comments_do_not_nest() {
    assert_eq!(kinds("/* outer /* inner */ SELECT"), [TokenKind::Select]);
}

#[test]
fn lexes_integer_text_and_delimited_identifier_tokens() {
    let source = "123 'it''s \\\\ literal' \"a\"\"b\" \"雪\"";
    let tokens = lex(source.as_bytes()).expect("literals and identifiers should lex");

    assert_eq!(
        tokens.iter().map(|token| token.kind).collect::<Vec<_>>(),
        [
            TokenKind::IntegerLiteral,
            TokenKind::TextLiteral,
            TokenKind::DelimitedIdentifier,
            TokenKind::DelimitedIdentifier,
        ]
    );
    assert_eq!(
        &source[tokens[1].span.start..tokens[1].span.end],
        "'it''s \\\\ literal'"
    );
    assert_eq!(&source[tokens[3].span.start..tokens[3].span.end], "\"雪\"");
}

#[test]
fn lexes_every_symbolic_token_using_longest_match() {
    assert_eq!(
        kinds("( ) , . ; + - * / % = <> < <= > >= ||"),
        [
            TokenKind::LeftParenthesis,
            TokenKind::RightParenthesis,
            TokenKind::Comma,
            TokenKind::Dot,
            TokenKind::Semicolon,
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::Equal,
            TokenKind::NotEqual,
            TokenKind::Less,
            TokenKind::LessEqual,
            TokenKind::Greater,
            TokenKind::GreaterEqual,
            TokenKind::Concatenate,
        ]
    );
}

#[test]
fn spans_are_half_open_utf8_byte_ranges() {
    let tokens = lex("\"雪\" x".as_bytes()).expect("source should lex successfully");

    assert_eq!(tokens[0].span, Span { start: 0, end: 5 });
    assert_eq!(tokens[1].span, Span { start: 6, end: 7 });
}

#[test]
fn reports_invalid_utf8() {
    let error = lex(&[b'S', b'E', 0xff]).expect_err("invalid UTF-8 should fail");

    assert_eq!(error.kind, LexErrorKind::InvalidUtf8);
    assert_eq!(error.span, Span { start: 2, end: 3 });
}

#[test]
fn reports_a_null_character_anywhere_in_source() {
    let error = lex(b"-- hidden\0null").expect_err("U+0000 should fail");

    assert_eq!(error.kind, LexErrorKind::NullCharacter);
    assert_eq!(error.span, Span { start: 9, end: 10 });
}

#[test]
fn reports_unrecognized_characters_with_complete_utf8_spans() {
    let error = lex("SELECT ☃".as_bytes()).expect_err("snowman should not be a token");

    assert_eq!(error.kind, LexErrorKind::UnexpectedCharacter('☃'));
    assert_eq!(error.span, Span { start: 7, end: 10 });
}

#[test]
fn does_not_treat_other_ascii_control_characters_as_whitespace() {
    let error = lex(b"SELECT\x0bvalue").expect_err("vertical tab should not be whitespace");

    assert_eq!(error.kind, LexErrorKind::UnexpectedCharacter('\u{000b}'));
    assert_eq!(error.span, Span { start: 6, end: 7 });
}

#[test]
fn ignores_a_bom_only_at_the_beginning() {
    let error = lex("SELECT \u{feff}".as_bytes()).expect_err("later BOM should not be ignored");

    assert_eq!(error.kind, LexErrorKind::UnexpectedCharacter('\u{feff}'));
    assert_eq!(error.span, Span { start: 7, end: 10 });
}

#[test]
fn reports_an_unterminated_block_comment() {
    let error = lex(b"SELECT /* no end").expect_err("unterminated comment should fail");

    assert_eq!(error.kind, LexErrorKind::UnterminatedBlockComment);
    assert_eq!(error.span, Span { start: 7, end: 16 });
}

#[test]
fn reports_an_unterminated_delimited_identifier() {
    let error = lex("SELECT \"name".as_bytes()).expect_err("unterminated identifier should fail");

    assert_eq!(error.kind, LexErrorKind::UnterminatedDelimitedIdentifier);
    assert_eq!(error.span, Span { start: 7, end: 12 });
}

#[test]
fn reports_an_empty_delimited_identifier() {
    let error = lex(b"SELECT \"\"").expect_err("empty identifier should fail");

    assert_eq!(error.kind, LexErrorKind::EmptyDelimitedIdentifier);
    assert_eq!(error.span, Span { start: 7, end: 9 });
}

#[test]
fn reports_an_unterminated_text_literal() {
    let error = lex(b"SELECT 'value").expect_err("unterminated literal should fail");

    assert_eq!(error.kind, LexErrorKind::UnterminatedTextLiteral);
    assert_eq!(error.span, Span { start: 7, end: 13 });
}

#[test]
fn does_not_accept_bang_equal_as_an_operator() {
    let error = lex(b"a != b").expect_err("!= should not be recognized");

    assert_eq!(error.kind, LexErrorKind::UnexpectedCharacter('!'));
    assert_eq!(error.span, Span { start: 2, end: 3 });
}

#[test]
fn every_existing_source_corpus_case_is_lexically_valid() {
    let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");

    for category in ["accepted", "rejected"] {
        for entry in
            std::fs::read_dir(corpus.join(category)).expect("corpus directory should exist")
        {
            let path = entry.expect("corpus entry should be readable").path();

            if path.extension().and_then(|extension| extension.to_str()) == Some("sql") {
                let source = std::fs::read(&path).expect("corpus file should be readable");

                lex(&source).unwrap_or_else(|error| {
                    panic!("{} should be lexically valid: {error}", path.display())
                });
            }
        }
    }
}
