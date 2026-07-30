use shapesql::ast::{BinaryOperator, Expression, QueryBody, SelectItem, SetOperator};
use shapesql::{ParseError, Span, TokenKind, parse};

fn parse_source(source: &str) -> shapesql::ast::Program {
    parse(source.as_bytes()).expect("source should parse successfully")
}

fn first_select_expression(program: &shapesql::ast::Program) -> &Expression {
    let select = match &program.query.body {
        QueryBody::Select(select) => select,
        _ => panic!("query body should be a SELECT"),
    };
    match &select.select_list[0] {
        SelectItem::Expression { expression, .. } => expression,
        _ => panic!("first select item should be an expression"),
    }
}

#[test]
fn parses_every_accepted_corpus_case() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/accepted");

    for entry in std::fs::read_dir(directory).expect("corpus directory should exist") {
        let path = entry.expect("corpus entry should be readable").path();

        if path.extension().and_then(|extension| extension.to_str()) == Some("sql") {
            let source = std::fs::read(&path).expect("corpus case should be readable");

            parse(&source)
                .unwrap_or_else(|error| panic!("{} should parse: {error}", path.display()));
        }
    }
}

#[test]
fn accepts_cases_assigned_to_later_static_phases() {
    let cases = [
        "ambiguous-column.sql",
        "ambiguous-order-alias.sql",
        "correlated-exists.sql",
        "correlated-in.sql",
        "cte-cycle.sql",
        "duplicate-source-qualifier.sql",
        "hidden-source-qualifier.sql",
        "order-by-ordinal-out-of-range.sql",
        "select-alias-in-where.sql",
        "type-aggregate-in-where.sql",
        "type-arithmetic-text.sql",
        "type-boolean-sum.sql",
        "type-in-query-arity.sql",
        "type-incompatible-case.sql",
        "type-incompatible-set-fields.sql",
        "type-incomplete-row-number-order.sql",
        "type-nonaggregate-having.sql",
        "type-non-boolean-predicate.sql",
        "type-nullable-order-without-placement.sql",
        "type-out-of-range-integer.sql",
        "type-unconstrained-null.sql",
        "type-ungrouped-column.sql",
        "type-unsupported-cast.sql",
        "unknown-relation.sql",
        "unknown-wildcard-qualifier.sql",
    ];
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/rejected");

    for case in cases {
        let path = directory.join(case);
        let source = std::fs::read(&path).expect("corpus case should be readable");

        parse(&source).unwrap_or_else(|error| {
            panic!(
                "{} is syntactically valid and should reach a later phase: {error}",
                path.display()
            )
        });
    }
}

#[test]
fn rejects_every_syntactic_corpus_case() {
    let cases = [
        "derived-source-without-alias.sql",
        "empty-in-list.sql",
        "limit-without-order.sql",
        "multiple-programs.sql",
        "select-without-from.sql",
        "window-aggregate-order.sql",
    ];
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/rejected");

    for case in cases {
        let path = directory.join(case);
        let source = std::fs::read(&path).expect("corpus case should be readable");
        let error = match parse(&source) {
            Ok(_) => panic!("{} should be rejected", path.display()),
            Err(error) => error,
        };

        assert!(
            matches!(error, ParseError::Syntactic(_)),
            "{} should be a syntactic error, got {error}",
            path.display()
        );
    }
}

#[test]
fn builds_set_operations_with_specified_precedence_and_associativity() {
    let program = parse_source(
        "\
        SELECT a FROM first_table \
        UNION SELECT b FROM second_table \
        INTERSECT ALL SELECT c FROM third_table \
        EXCEPT SELECT d FROM fourth_table",
    );

    let (left, right) = match &program.query.body {
        QueryBody::SetOperation {
            left,
            operator: SetOperator::Except,
            right,
            ..
        } => (left, right),
        _ => panic!("EXCEPT should be the root operation"),
    };
    assert!(matches!(right.as_ref(), QueryBody::Select(_)));

    let (union_left, union_right) = match left.as_ref() {
        QueryBody::SetOperation {
            left,
            operator: SetOperator::Union,
            right,
            ..
        } => (left, right),
        _ => panic!("UNION should associate to the left of EXCEPT"),
    };
    assert!(matches!(union_left.as_ref(), QueryBody::Select(_)));
    assert!(matches!(
        union_right.as_ref(),
        QueryBody::SetOperation {
            operator: SetOperator::Intersect,
            all: true,
            ..
        }
    ));
}

#[test]
fn builds_arithmetic_expressions_with_specified_precedence() {
    let program = parse_source("SELECT a - b - c + d * -e FROM values_table");
    let expression = first_select_expression(&program);

    let (left, right) = match expression {
        Expression::Binary {
            left,
            operator: BinaryOperator::Add,
            right,
            ..
        } => (left, right),
        _ => panic!("addition should be the root operator"),
    };
    assert!(matches!(
        left.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: nested_left,
            ..
        } if matches!(
            nested_left.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                ..
            }
        )
    ));
    assert!(matches!(
        right.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            ..
        }
    ));
}

#[test]
fn distinguishes_parenthesized_in_queries_from_parenthesized_lists() {
    let query = parse_source("SELECT a FROM t WHERE a IN (((SELECT b FROM u)))");
    let list = parse_source("SELECT a FROM t WHERE a IN (((b)))");

    let query_select = match &query.query.body {
        QueryBody::Select(select) => select,
        _ => panic!("query should contain a SELECT"),
    };
    let list_select = match &list.query.body {
        QueryBody::Select(select) => select,
        _ => panic!("list should contain a SELECT"),
    };

    assert!(matches!(
        &query_select.where_clause,
        Some(Expression::InQuery { .. })
    ));
    assert!(matches!(
        &list_select.where_clause,
        Some(Expression::InList { .. })
    ));
}

#[test]
fn parses_all_major_grammar_families_together() {
    parse_source(
        "\
        WITH source_rows AS (
            SELECT id, group_id, flag FROM source_table
        )
        SELECT DISTINCT
            CASE WHEN NOT l.id IS NULL
                THEN CAST(+l.id AS INT64)
                ELSE -1
            END AS computed,
            COUNT(*) OVER (PARTITION BY l.group_id) AS group_count,
            ROW_NUMBER() OVER (
                PARTITION BY l.group_id
                ORDER BY l.id DESC NULLS FIRST
            ) AS position
        FROM source_rows AS l
        CROSS JOIN lookup r
        LEFT OUTER JOIN (
            SELECT id FROM details
        ) AS d ON d.id = l.id
        RIGHT JOIN other o ON o.id = l.id
        FULL OUTER JOIN final_table f ON f.id = l.id
        WHERE l.id NOT IN (1, 2, 3)
            AND EXISTS (SELECT x.id FROM extra x)
        GROUP BY l.id, l.group_id
        HAVING BOOL_AND(l.flag)
        ORDER BY computed ASC NULLS LAST
        OFFSET 1 LIMIT 10;",
    );
}

#[test]
fn permits_one_optional_terminating_semicolon() {
    let without = parse_source("SELECT a FROM t");
    let with = parse_source("SELECT a FROM t;");

    assert_eq!(without.span, Span { start: 0, end: 15 });
    assert_eq!(with.span, Span { start: 0, end: 16 });

    let error = parse(b"SELECT a FROM t;;").expect_err("a second semicolon should fail");
    assert!(matches!(
        error,
        ParseError::Syntactic(error)
            if error.found == Some(TokenKind::Semicolon)
                && error.span == (Span { start: 16, end: 17 })
    ));
}

#[test]
fn reports_lexical_and_syntactic_failures_separately() {
    let lexical = parse(b"SELECT ! FROM t").expect_err("invalid token should fail");
    let syntactic = parse(b"SELECT FROM t").expect_err("invalid grammar should fail");

    assert!(matches!(lexical, ParseError::Lexical(_)));
    assert!(matches!(
        syntactic,
        ParseError::Syntactic(error)
            if error.found == Some(TokenKind::From)
                && error.span == (Span { start: 7, end: 11 })
    ));
}

#[test]
fn rejects_chained_predicates_without_parentheses() {
    let error =
        parse(b"SELECT a FROM t WHERE a < b < c").expect_err("chained comparison should fail");

    assert!(matches!(
        error,
        ParseError::Syntactic(error) if error.found == Some(TokenKind::Less)
    ));
}
