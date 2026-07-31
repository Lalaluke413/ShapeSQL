use std::fs;
use std::path::Path;

use shapesql::{
    Catalog, CatalogField, CatalogRelation, ScalarType, TypeDescriptor, bind, parse_owned,
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

fn bind_case(relative: &str) -> Result<shapesql::hir::BoundProgram, shapesql::BindError> {
    let path = Path::new("tests/corpus").join(relative);
    let source = fs::read(path).unwrap();
    let parsed = parse_owned(&source).unwrap();
    bind(&parsed, &catalog())
}

#[test]
fn binds_every_accepted_corpus_query() {
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
        bind_case(case).unwrap_or_else(|error| panic!("{case}: {error}"));
    }
}

#[test]
fn rejects_every_binding_corpus_case() {
    for case in [
        "rejected/correlated-exists.sql",
        "rejected/correlated-in.sql",
        "rejected/ambiguous-column.sql",
        "rejected/cte-cycle.sql",
        "rejected/hidden-source-qualifier.sql",
        "rejected/distinct-order-by-source.sql",
        "rejected/select-alias-in-where.sql",
        "rejected/unknown-wildcard-qualifier.sql",
        "rejected/ambiguous-order-alias.sql",
        "rejected/order-by-ordinal-out-of-range.sql",
        "rejected/duplicate-source-qualifier.sql",
        "rejected/unknown-relation.sql",
    ] {
        assert!(bind_case(case).is_err(), "{case} unexpectedly bound");
    }
}

#[test]
fn typing_corpus_cases_reach_the_type_checker() {
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
        bind_case(case).unwrap_or_else(|error| panic!("{case}: {error}"));
    }
}

#[test]
fn expands_wildcards_and_assigns_fresh_result_fields() {
    let program = bind_case("accepted/binding-alias-wildcard.sql").unwrap();
    let shapesql::hir::QueryBody::Select(select) = &program.query.body else {
        panic!("expected select query")
    };

    let names = select
        .result_fields
        .iter()
        .map(|field| field.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(names, ["item_id", "flag", "outer_group"]);
    assert!(select.result_fields.iter().all(|result| {
        select
            .source_fields
            .iter()
            .all(|source| result.id != source.id)
    }));
}

#[test]
fn binds_forward_ctes_and_retains_unreferenced_declarations() {
    let source = b"WITH later AS (SELECT id FROM inner_rows), unused AS (SELECT flag FROM inner_rows) SELECT id FROM later";
    let parsed = parse_owned(source).unwrap();
    let program = bind(&parsed, &catalog()).unwrap();

    assert_eq!(program.query.common_table_expressions.len(), 2);
    assert_eq!(
        program.query.common_table_expressions[1].name.as_str(),
        "unused"
    );
}

#[test]
fn statically_analyzes_unreferenced_ctes() {
    let source = b"WITH unused AS (SELECT id FROM missing_rows) SELECT id FROM outer_rows";
    let parsed = parse_owned(source).unwrap();

    assert!(bind(&parsed, &catalog()).is_err());
}
