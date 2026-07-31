use std::fs;
use std::path::Path;

use shapesql::{
    Catalog, CatalogField, CatalogRelation, ScalarType, TypeDescriptor, bind, parse_owned,
    type_check,
};

fn catalog() -> Catalog {
    Catalog::new(vec![
        CatalogRelation::new(
            "outer_rows",
            "outer_rows",
            vec![
                CatalogField::new("id", TypeDescriptor::non_nullable(ScalarType::Int64)),
                CatalogField::new("group_id", TypeDescriptor::non_nullable(ScalarType::Int64)),
            ],
        ),
        CatalogRelation::new(
            "inner_rows",
            "inner_rows",
            vec![
                CatalogField::new("id", TypeDescriptor::non_nullable(ScalarType::Int64)),
                CatalogField::new("group_id", TypeDescriptor::non_nullable(ScalarType::Int64)),
                CatalogField::new("flag", TypeDescriptor::nullable(ScalarType::Boolean)),
            ],
        ),
    ])
}

fn type_case(relative: &str) -> Result<shapesql::hir::TypedProgram, shapesql::TypeError> {
    let path = Path::new("tests/corpus").join(relative);
    let source = fs::read(path).unwrap();
    let parsed = parse_owned(&source).unwrap();
    let bound = bind(&parsed, &catalog()).unwrap();
    type_check(bound)
}

#[test]
fn type_checks_every_accepted_corpus_query() {
    for case in [
        "accepted/uncorrelated-exists.sql",
        "accepted/uncorrelated-in.sql",
        "accepted/grammar-core.sql",
        "accepted/set-precedence.sql",
        "accepted/grouped-boolean-aggregates.sql",
        "accepted/grouped-row-number.sql",
        "accepted/partitioned-boolean-aggregates.sql",
        "accepted/binding-alias-wildcard.sql",
        "accepted/order-by-ordinal.sql",
        "accepted/forward-cte-reference.sql",
        "accepted/typed-expressions.sql",
    ] {
        type_case(case).unwrap_or_else(|error| panic!("{case}: {error}"));
    }
}

#[test]
fn rejects_every_typing_corpus_case() {
    for case in [
        "rejected/type-arithmetic-text.sql",
        "rejected/type-unconstrained-null.sql",
        "rejected/type-non-boolean-predicate.sql",
        "rejected/type-incompatible-case.sql",
        "rejected/type-incompatible-set-fields.sql",
        "rejected/type-in-query-arity.sql",
        "rejected/type-boolean-sum.sql",
        "rejected/type-nullable-order-without-placement.sql",
        "rejected/type-incomplete-row-number-order.sql",
        "rejected/type-incomplete-row-bound-order.sql",
        "rejected/type-aggregate-in-where.sql",
        "rejected/type-out-of-range-integer.sql",
        "rejected/type-unsupported-cast.sql",
        "rejected/type-nonaggregate-having.sql",
        "rejected/type-ungrouped-column.sql",
        "rejected/type-window-in-distinct-order.sql",
    ] {
        assert!(type_case(case).is_err(), "{case} unexpectedly type checked");
    }
}

#[test]
fn resolves_contextual_nulls_and_the_minimum_int64_spelling() {
    let program = type_case("accepted/typed-expressions.sql").unwrap();
    let expected = [
        TypeDescriptor::non_nullable(ScalarType::Int64),
        TypeDescriptor::nullable(ScalarType::Text),
        TypeDescriptor::non_nullable(ScalarType::Int64),
        TypeDescriptor::non_nullable(ScalarType::Boolean),
        TypeDescriptor::nullable(ScalarType::Int64),
        TypeDescriptor::nullable(ScalarType::Boolean),
        TypeDescriptor::non_nullable(ScalarType::Int64),
    ];
    let actual = program
        .query
        .result_fields
        .iter()
        .map(|field| field.annotation)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn contextual_null_constraints_flow_both_directions_across_sets() {
    for source in [
        "SELECT NULL FROM outer_rows UNION SELECT id FROM inner_rows",
        "SELECT id FROM outer_rows UNION SELECT NULL FROM inner_rows",
        "SELECT NULL, id FROM outer_rows UNION SELECT id, NULL FROM inner_rows",
    ] {
        let parsed = parse_owned(source.as_bytes()).unwrap();
        let bound = bind(&parsed, &catalog()).unwrap();
        let typed = type_check(bound).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert!(
            typed
                .query
                .result_fields
                .iter()
                .all(|field| field.annotation.scalar == ScalarType::Int64)
        );
    }
}

#[test]
fn a_source_use_does_not_retroactively_type_its_producer() {
    let source = b"WITH unresolved AS (SELECT NULL FROM inner_rows) SELECT id FROM outer_rows WHERE id IN (SELECT * FROM unresolved)";
    let parsed = parse_owned(source).unwrap();
    let bound = bind(&parsed, &catalog()).unwrap();

    assert!(type_check(bound).is_err());
}

#[test]
fn contextual_null_constraints_flow_both_directions_through_in_query() {
    for source in [
        "SELECT NULL IN (SELECT id FROM inner_rows) FROM outer_rows",
        "SELECT id IN (SELECT NULL FROM inner_rows) FROM outer_rows",
    ] {
        let parsed = parse_owned(source.as_bytes()).unwrap();
        let bound = bind(&parsed, &catalog()).unwrap();
        let typed = type_check(bound).unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(
            typed.query.result_fields[0].annotation.scalar,
            ScalarType::Boolean
        );
    }
}

#[test]
fn outer_join_widens_only_the_null_extended_side() {
    let source = b"SELECT o.id, i.id FROM outer_rows AS o LEFT JOIN inner_rows AS i ON o.id = i.id";
    let parsed = parse_owned(source).unwrap();
    let bound = bind(&parsed, &catalog()).unwrap();
    let typed = type_check(bound).unwrap();

    assert!(!typed.query.result_fields[0].annotation.nullable);
    assert!(typed.query.result_fields[1].annotation.nullable);
}
