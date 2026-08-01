use std::fs;
use std::path::Path;

use shapesql::interchange::decode;
use shapesql::shape_ir::{CollectionKind, ExpressionKind, LiteralValue, NodeKind};
use shapesql::{
    Catalog, CatalogField, CatalogRelation, EvaluateError, EvaluationErrorKind, InputErrorKind,
    InputField, InputRelation, Row, ScalarType, Snapshot, TypeDescriptor, Value, compile, evaluate,
};

fn descriptor(scalar: ScalarType, nullable: bool) -> TypeDescriptor {
    TypeDescriptor::new(scalar, nullable)
}

fn catalog_field(name: &str, scalar: ScalarType, nullable: bool) -> CatalogField {
    CatalogField::new(name, descriptor(scalar, nullable))
}

fn input_field(name: &str, scalar: ScalarType, nullable: bool) -> InputField {
    InputField::new(name, descriptor(scalar, nullable))
}

fn relation(schema: Vec<InputField>, rows: Vec<Row>) -> InputRelation {
    InputRelation::new(schema, rows)
}

fn row(values: Vec<Value>) -> Row {
    Row::new(values)
}

fn int(value: i64) -> Value {
    Value::Int64(value)
}

fn boolean(value: bool) -> Value {
    Value::Boolean(value)
}

fn text(value: &str) -> Value {
    Value::Text(value.into())
}

fn snapshot<I, B>(relations: I) -> Snapshot
where
    I: IntoIterator<Item = (B, InputRelation)>,
    B: Into<shapesql::RelationBinding>,
{
    Snapshot::from_relations(relations).unwrap()
}

fn evaluate_sql(
    source: &str,
    catalog: &Catalog,
    snapshot: &Snapshot,
) -> shapesql::EvaluationResult {
    let graph = compile(source.as_bytes(), catalog).unwrap();
    evaluate(&graph, snapshot).unwrap()
}

fn fixture(relative: &str) -> shapesql::shape_ir::Graph {
    let bytes = fs::read(Path::new("tests/corpus/ir").join(relative)).unwrap();
    decode(&bytes).unwrap()
}

fn assert_bag_eq(actual: &[Row], expected: Vec<Row>) {
    let mut remaining = actual.to_vec();
    for expected_row in &expected {
        let Some(index) = remaining.iter().position(|row| row == expected_row) else {
            panic!("missing row {expected_row:?}; actual rows: {actual:?}");
        };
        remaining.remove(index);
    }
    assert!(
        remaining.is_empty(),
        "unexpected rows {remaining:?}; expected rows: {expected:?}"
    );
}

#[test]
fn evaluates_direct_scalar_expression_fixture() {
    let graph = fixture("accepted/scalar-expression-forms.shapeir.json");
    let snapshot = snapshot([
        (
            "main_rows",
            relation(
                vec![
                    input_field("number", ScalarType::Int64, false),
                    input_field("flag", ScalarType::Boolean, true),
                    input_field("label", ScalarType::Text, false),
                ],
                vec![
                    row(vec![int(2), Value::Null, text("x")]),
                    row(vec![int(3), boolean(false), text("é")]),
                ],
            ),
        ),
        (
            "existence_rows",
            relation(
                vec![input_field("id", ScalarType::Int64, false)],
                vec![row(vec![int(9)])],
            ),
        ),
        (
            "membership_rows",
            relation(
                vec![input_field("id", ScalarType::Int64, true)],
                vec![row(vec![int(2)]), row(vec![Value::Null])],
            ),
        ),
    ]);

    let result = evaluate(&graph, &snapshot).unwrap();
    assert_eq!(result.collection, CollectionKind::Bag);
    assert_eq!(
        result.rows,
        vec![
            row(vec![
                int(2),
                int(-2),
                text("x!"),
                boolean(true),
                int(0),
                text("2"),
                boolean(true),
                boolean(true),
                boolean(true),
                Value::Null,
            ]),
            row(vec![
                int(3),
                int(-3),
                text("é!"),
                boolean(false),
                int(3),
                text("3"),
                boolean(false),
                boolean(true),
                Value::Null,
                boolean(true),
            ]),
        ]
    );
}

#[test]
fn evaluates_every_relational_node_in_the_direct_fixture() {
    let graph = fixture("accepted/relational-node-forms.shapeir.json");
    let snapshot = snapshot([
        (
            "left_rows",
            relation(
                vec![
                    input_field("group_id", ScalarType::Int64, false),
                    input_field("keep", ScalarType::Boolean, true),
                ],
                vec![
                    row(vec![int(1), boolean(true)]),
                    row(vec![int(1), boolean(true)]),
                    row(vec![int(2), boolean(false)]),
                    row(vec![int(3), Value::Null]),
                ],
            ),
        ),
        (
            "right_rows",
            relation(
                vec![input_field("group_id", ScalarType::Int64, false)],
                vec![
                    row(vec![int(1)]),
                    row(vec![int(1)]),
                    row(vec![int(2)]),
                    row(vec![int(3)]),
                ],
            ),
        ),
    ]);

    let result = evaluate(&graph, &snapshot).unwrap();
    assert_eq!(result.collection, CollectionKind::Bag);
    assert_eq!(result.rows, vec![row(vec![int(1), int(4), int(1), int(1)])]);
}

#[test]
fn conditional_demand_does_not_require_a_skipped_host_binding() {
    let mut graph = fixture("accepted/demand-subquery.shapeir.json");
    let snapshot = snapshot([(
        "main_rows",
        relation(
            vec![input_field("id", ScalarType::Int64, false)],
            vec![row(vec![int(1)])],
        ),
    )]);

    let result = evaluate(&graph, &snapshot).unwrap();
    assert!(result.rows.is_empty());

    let filter = graph
        .nodes
        .iter_mut()
        .find(|node| node.id.as_str() == "n2")
        .unwrap();
    let NodeKind::Filter { predicate, .. } = &mut filter.kind else {
        panic!("fixture root must be a filter");
    };
    let ExpressionKind::Binary { left, .. } = &mut predicate.kind else {
        panic!("fixture predicate must be binary");
    };
    left.kind = ExpressionKind::Literal(LiteralValue::Null);
    left.descriptor = descriptor(ScalarType::Boolean, true);
    predicate.descriptor = descriptor(ScalarType::Boolean, true);

    let error = evaluate(&graph, &snapshot).unwrap_err();
    assert!(matches!(
        error,
        EvaluateError::Input(shapesql::InputError {
            kind: InputErrorKind::MissingBinding,
            ..
        })
    ));
}

#[test]
fn evaluates_lowered_grouping_outer_join_order_and_slice() {
    let catalog = Catalog::new(vec![
        CatalogRelation::new(
            "outer_rows",
            "outer_rows",
            vec![
                catalog_field("id", ScalarType::Int64, false),
                catalog_field("group_id", ScalarType::Int64, false),
            ],
        ),
        CatalogRelation::new(
            "inner_rows",
            "inner_rows",
            vec![
                catalog_field("id", ScalarType::Int64, false),
                catalog_field("group_id", ScalarType::Int64, false),
                catalog_field("flag", ScalarType::Boolean, true),
            ],
        ),
    ]);
    let snapshot = snapshot([
        (
            "outer_rows",
            relation(
                vec![
                    input_field("id", ScalarType::Int64, false),
                    input_field("group_id", ScalarType::Int64, false),
                ],
                vec![
                    row(vec![int(1), int(10)]),
                    row(vec![int(2), int(20)]),
                    row(vec![int(3), int(30)]),
                    row(vec![int(0), int(40)]),
                ],
            ),
        ),
        (
            "inner_rows",
            relation(
                vec![
                    input_field("id", ScalarType::Int64, false),
                    input_field("group_id", ScalarType::Int64, false),
                    input_field("flag", ScalarType::Boolean, true),
                ],
                vec![
                    row(vec![int(5), int(10), boolean(true)]),
                    row(vec![int(7), int(10), Value::Null]),
                    row(vec![int(3), int(20), boolean(false)]),
                ],
            ),
        ),
    ]);
    let source = fs::read_to_string("tests/corpus/accepted/grammar-core.sql").unwrap();

    let result = evaluate_sql(&source, &catalog, &snapshot);
    assert_eq!(result.collection, CollectionKind::Ordered);
    assert_eq!(
        result.rows,
        vec![
            row(vec![int(10), int(12)]),
            row(vec![int(20), int(3)]),
            row(vec![int(30), Value::Null]),
        ]
    );
}

#[test]
fn validates_only_demanded_host_relations_and_checks_them_strictly() {
    let graph = fixture("accepted/minimal-input.shapeir.json");
    let missing = evaluate(&graph, &Snapshot::new()).unwrap_err();
    assert!(matches!(
        missing,
        EvaluateError::Input(shapesql::InputError {
            kind: InputErrorKind::MissingBinding,
            ..
        })
    ));

    let wrong_schema = snapshot([(
        "rows",
        relation(
            vec![input_field("other", ScalarType::Int64, false)],
            vec![row(vec![int(1)])],
        ),
    )]);
    assert!(matches!(
        evaluate(&graph, &wrong_schema),
        Err(EvaluateError::Input(shapesql::InputError {
            kind: InputErrorKind::FieldName { .. },
            ..
        }))
    ));

    let invalid_value = snapshot([(
        "rows",
        relation(
            vec![input_field("value", ScalarType::Int64, false)],
            vec![row(vec![Value::Null])],
        ),
    )]);
    assert!(matches!(
        evaluate(&graph, &invalid_value),
        Err(EvaluateError::Input(shapesql::InputError {
            kind: InputErrorKind::InvalidValue { .. },
            ..
        }))
    ));

    let invalid_graph = fixture("rejected/negative-slice-offset.shapeir.json");
    assert!(matches!(
        evaluate(&invalid_graph, &Snapshot::new()),
        Err(EvaluateError::Validation(_))
    ));
}

#[test]
fn preserves_conditional_and_strict_scalar_error_demand() {
    let catalog = Catalog::new(vec![CatalogRelation::new(
        "numbers",
        "numbers",
        vec![catalog_field("id", ScalarType::Int64, false)],
    )]);
    let one = snapshot([(
        "numbers",
        relation(
            vec![input_field("id", ScalarType::Int64, false)],
            vec![row(vec![int(1)])],
        ),
    )]);

    let success = evaluate_sql(
        "SELECT CASE WHEN TRUE THEN 7 ELSE 1 / 0 END, FALSE AND (1 / 0 = 0), TRUE OR (1 / 0 = 0) FROM numbers",
        &catalog,
        &one,
    );
    assert_eq!(
        success.rows,
        vec![row(vec![int(7), boolean(false), boolean(true)])]
    );

    for source in [
        "SELECT CAST(NULL AS INT64) + (1 / 0) FROM numbers",
        "SELECT 1 IN (1, 1 / 0) FROM numbers",
    ] {
        let graph = compile(source.as_bytes(), &catalog).unwrap();
        assert!(matches!(
            evaluate(&graph, &one),
            Err(EvaluateError::Evaluation(shapesql::EvaluationError {
                kind: EvaluationErrorKind::DivisionByZero,
                ..
            }))
        ));
    }
}

#[test]
fn relational_predicates_ordering_and_slice_require_complete_inputs() {
    let catalog = Catalog::new(vec![
        CatalogRelation::new(
            "outer_rows",
            "outer_rows",
            vec![catalog_field("id", ScalarType::Int64, false)],
        ),
        CatalogRelation::new(
            "inner_rows",
            "inner_rows",
            vec![catalog_field("id", ScalarType::Int64, false)],
        ),
    ]);
    let snapshot = snapshot([
        (
            "outer_rows",
            relation(
                vec![input_field("id", ScalarType::Int64, false)],
                vec![row(vec![int(1)])],
            ),
        ),
        (
            "inner_rows",
            relation(
                vec![input_field("id", ScalarType::Int64, false)],
                vec![row(vec![int(1)]), row(vec![int(0)])],
            ),
        ),
    ]);

    for source in [
        "SELECT o.id FROM outer_rows AS o WHERE EXISTS (SELECT 1 / i.id FROM inner_rows AS i)",
        "SELECT o.id FROM outer_rows AS o WHERE o.id IN (SELECT CASE WHEN i.id = 0 THEN 1 / 0 ELSE 1 END FROM inner_rows AS i)",
        "SELECT i.id FROM inner_rows AS i ORDER BY 1 DESC, 1 / i.id ASC LIMIT 1",
        "SELECT o.id FROM outer_rows AS o WHERE FALSE INTERSECT SELECT 1 / i.id FROM inner_rows AS i",
    ] {
        let graph = compile(source.as_bytes(), &catalog).unwrap();
        assert!(matches!(
            evaluate(&graph, &snapshot),
            Err(EvaluateError::Evaluation(shapesql::EvaluationError {
                kind: EvaluationErrorKind::DivisionByZero,
                ..
            }))
        ));
    }
}

#[test]
fn full_outer_join_preserves_candidate_pair_multiplicity() {
    let catalog = Catalog::new(vec![
        CatalogRelation::new(
            "left_rows",
            "left_rows",
            vec![catalog_field("id", ScalarType::Int64, false)],
        ),
        CatalogRelation::new(
            "right_rows",
            "right_rows",
            vec![catalog_field("id", ScalarType::Int64, false)],
        ),
    ]);
    let snapshot = snapshot([
        (
            "left_rows",
            relation(
                vec![input_field("id", ScalarType::Int64, false)],
                vec![row(vec![int(1)]), row(vec![int(1)]), row(vec![int(2)])],
            ),
        ),
        (
            "right_rows",
            relation(
                vec![input_field("id", ScalarType::Int64, false)],
                vec![row(vec![int(1)]), row(vec![int(1)]), row(vec![int(3)])],
            ),
        ),
    ]);
    let result = evaluate_sql(
        "SELECT l.id, r.id FROM left_rows AS l FULL JOIN right_rows AS r ON l.id = r.id",
        &catalog,
        &snapshot,
    );

    assert_bag_eq(
        &result.rows,
        vec![
            row(vec![int(1), int(1)]),
            row(vec![int(1), int(1)]),
            row(vec![int(1), int(1)]),
            row(vec![int(1), int(1)]),
            row(vec![int(2), Value::Null]),
            row(vec![Value::Null, int(3)]),
        ],
    );

    let cross = evaluate_sql(
        "SELECT l.id, r.id FROM left_rows AS l CROSS JOIN right_rows AS r",
        &catalog,
        &snapshot,
    );
    assert_eq!(cross.rows.len(), 9);
}

#[test]
fn grouping_aggregates_use_null_and_exact_final_sum_semantics() {
    let catalog = Catalog::new(vec![CatalogRelation::new(
        "rows",
        "rows",
        vec![
            catalog_field("group_id", ScalarType::Int64, false),
            catalog_field("value", ScalarType::Int64, true),
            catalog_field("flag", ScalarType::Boolean, true),
        ],
    )]);
    let grouped = snapshot([(
        "rows",
        relation(
            vec![
                input_field("group_id", ScalarType::Int64, false),
                input_field("value", ScalarType::Int64, true),
                input_field("flag", ScalarType::Boolean, true),
            ],
            vec![
                row(vec![int(1), int(5), boolean(true)]),
                row(vec![int(1), Value::Null, Value::Null]),
                row(vec![int(1), int(-2), boolean(false)]),
                row(vec![int(2), Value::Null, Value::Null]),
            ],
        ),
    )]);
    let result = evaluate_sql(
        "SELECT group_id, COUNT(*), COUNT(value), SUM(value), MIN(value), MAX(value), BOOL_AND(flag), BOOL_OR(flag) FROM rows GROUP BY group_id",
        &catalog,
        &grouped,
    );
    assert_bag_eq(
        &result.rows,
        vec![
            row(vec![
                int(1),
                int(3),
                int(2),
                int(3),
                int(-2),
                int(5),
                boolean(false),
                boolean(true),
            ]),
            row(vec![
                int(2),
                int(1),
                int(0),
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
                Value::Null,
            ]),
        ],
    );

    let exact = snapshot([(
        "rows",
        relation(
            vec![
                input_field("group_id", ScalarType::Int64, false),
                input_field("value", ScalarType::Int64, true),
                input_field("flag", ScalarType::Boolean, true),
            ],
            vec![
                row(vec![int(1), int(i64::MAX), boolean(true)]),
                row(vec![int(1), int(1), boolean(true)]),
                row(vec![int(1), int(-1), boolean(true)]),
            ],
        ),
    )]);
    let result = evaluate_sql("SELECT SUM(value) FROM rows", &catalog, &exact);
    assert_eq!(result.rows, vec![row(vec![int(i64::MAX)])]);

    let empty = snapshot([(
        "rows",
        relation(
            vec![
                input_field("group_id", ScalarType::Int64, false),
                input_field("value", ScalarType::Int64, true),
                input_field("flag", ScalarType::Boolean, true),
            ],
            vec![],
        ),
    )]);
    let global = evaluate_sql(
        "SELECT COUNT(*), COUNT(value), SUM(value), MIN(value), MAX(value), BOOL_AND(flag), BOOL_OR(flag) FROM rows",
        &catalog,
        &empty,
    );
    assert_eq!(
        global.rows,
        vec![row(vec![
            int(0),
            int(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        ])]
    );
    let grouped_empty = evaluate_sql(
        "SELECT group_id, COUNT(*) FROM rows GROUP BY group_id",
        &catalog,
        &empty,
    );
    assert!(grouped_empty.rows.is_empty());
}

#[test]
fn boolean_aggregates_do_not_suppress_later_argument_errors() {
    let catalog = Catalog::new(vec![CatalogRelation::new(
        "rows",
        "rows",
        vec![catalog_field("id", ScalarType::Int64, false)],
    )]);
    let snapshot = snapshot([(
        "rows",
        relation(
            vec![input_field("id", ScalarType::Int64, false)],
            vec![row(vec![int(1)]), row(vec![int(0)])],
        ),
    )]);
    let graph = compile("SELECT BOOL_OR(1 / id > 0) FROM rows".as_bytes(), &catalog).unwrap();
    assert!(matches!(
        evaluate(&graph, &snapshot),
        Err(EvaluateError::Evaluation(shapesql::EvaluationError {
            kind: EvaluationErrorKind::DivisionByZero,
            ..
        }))
    ));
}

#[test]
fn evaluates_partitioned_aggregates_and_all_ranking_functions() {
    let catalog = Catalog::new(vec![CatalogRelation::new(
        "rows",
        "rows",
        vec![
            catalog_field("group_id", ScalarType::Int64, false),
            catalog_field("id", ScalarType::Int64, false),
            catalog_field("value", ScalarType::Int64, false),
        ],
    )]);
    let snapshot = snapshot([(
        "rows",
        relation(
            vec![
                input_field("group_id", ScalarType::Int64, false),
                input_field("id", ScalarType::Int64, false),
                input_field("value", ScalarType::Int64, false),
            ],
            vec![
                row(vec![int(1), int(1), int(10)]),
                row(vec![int(1), int(2), int(10)]),
                row(vec![int(1), int(3), int(5)]),
                row(vec![int(2), int(4), int(7)]),
            ],
        ),
    )]);
    let source = "
        SELECT id,
               COUNT(*) OVER (PARTITION BY group_id),
               SUM(value) OVER (PARTITION BY group_id),
               ROW_NUMBER() OVER (
                   PARTITION BY group_id
                   ORDER BY group_id ASC, id ASC, value ASC
               ),
               RANK() OVER (PARTITION BY group_id ORDER BY value DESC),
               DENSE_RANK() OVER (PARTITION BY group_id ORDER BY value DESC)
        FROM rows
    ";
    let result = evaluate_sql(source, &catalog, &snapshot);
    assert_bag_eq(
        &result.rows,
        vec![
            row(vec![int(1), int(3), int(25), int(1), int(1), int(1)]),
            row(vec![int(2), int(3), int(25), int(2), int(1), int(1)]),
            row(vec![int(3), int(3), int(25), int(3), int(3), int(2)]),
            row(vec![int(4), int(1), int(7), int(1), int(1), int(1)]),
        ],
    );
}

#[test]
fn implements_all_set_multiplicity_rules() {
    let catalog = Catalog::new(vec![
        CatalogRelation::new(
            "left_rows",
            "left_rows",
            vec![catalog_field("value", ScalarType::Int64, false)],
        ),
        CatalogRelation::new(
            "right_rows",
            "right_rows",
            vec![catalog_field("value", ScalarType::Int64, false)],
        ),
    ]);
    let snapshot = snapshot([
        (
            "left_rows",
            relation(
                vec![input_field("value", ScalarType::Int64, false)],
                vec![
                    row(vec![int(1)]),
                    row(vec![int(1)]),
                    row(vec![int(2)]),
                    row(vec![int(3)]),
                ],
            ),
        ),
        (
            "right_rows",
            relation(
                vec![input_field("value", ScalarType::Int64, false)],
                vec![
                    row(vec![int(1)]),
                    row(vec![int(2)]),
                    row(vec![int(2)]),
                    row(vec![int(4)]),
                ],
            ),
        ),
    ]);

    let cases = [
        (
            "UNION ALL",
            vec![
                row(vec![int(1)]),
                row(vec![int(1)]),
                row(vec![int(1)]),
                row(vec![int(2)]),
                row(vec![int(2)]),
                row(vec![int(2)]),
                row(vec![int(3)]),
                row(vec![int(4)]),
            ],
        ),
        (
            "UNION",
            vec![
                row(vec![int(1)]),
                row(vec![int(2)]),
                row(vec![int(3)]),
                row(vec![int(4)]),
            ],
        ),
        ("INTERSECT ALL", vec![row(vec![int(1)]), row(vec![int(2)])]),
        ("INTERSECT", vec![row(vec![int(1)]), row(vec![int(2)])]),
        ("EXCEPT ALL", vec![row(vec![int(1)]), row(vec![int(3)])]),
        ("EXCEPT", vec![row(vec![int(3)])]),
    ];

    for (operation, expected) in cases {
        let source =
            format!("SELECT value FROM left_rows {operation} SELECT value FROM right_rows");
        let result = evaluate_sql(&source, &catalog, &snapshot);
        assert_bag_eq(&result.rows, expected);
    }
}

#[test]
fn null_placement_is_independent_of_direction_and_slice_is_ordered() {
    let catalog = Catalog::new(vec![CatalogRelation::new(
        "rows",
        "rows",
        vec![
            catalog_field("id", ScalarType::Int64, false),
            catalog_field("value", ScalarType::Int64, true),
        ],
    )]);
    let snapshot = snapshot([(
        "rows",
        relation(
            vec![
                input_field("id", ScalarType::Int64, false),
                input_field("value", ScalarType::Int64, true),
            ],
            vec![
                row(vec![int(1), int(1)]),
                row(vec![int(2), Value::Null]),
                row(vec![int(3), int(2)]),
            ],
        ),
    )]);

    let first = evaluate_sql(
        "SELECT id, value FROM rows ORDER BY value DESC NULLS FIRST, id ASC",
        &catalog,
        &snapshot,
    );
    assert_eq!(
        first.rows,
        vec![
            row(vec![int(2), Value::Null]),
            row(vec![int(3), int(2)]),
            row(vec![int(1), int(1)]),
        ]
    );

    let bounded = evaluate_sql(
        "SELECT id, value FROM rows ORDER BY value DESC NULLS LAST, id ASC LIMIT 1 OFFSET 1",
        &catalog,
        &snapshot,
    );
    assert_eq!(bounded.rows, vec![row(vec![int(1), int(1)])]);
}

#[test]
fn arithmetic_and_cast_boundaries_follow_portable_rules() {
    let catalog = Catalog::new(vec![CatalogRelation::new("unit", "unit", vec![])]);
    let snapshot = snapshot([("unit", relation(vec![], vec![row(vec![])]))]);
    let result = evaluate_sql(
        "SELECT -9223372036854775808 % -1, CAST('+001' AS INT64), CAST('fAlSe' AS BOOLEAN) FROM unit",
        &catalog,
        &snapshot,
    );
    assert_eq!(result.rows, vec![row(vec![int(0), int(1), boolean(false)])]);

    for (source, expected) in [
        (
            "SELECT 9223372036854775807 + 1 FROM unit",
            EvaluationErrorKind::IntegerOverflow,
        ),
        (
            "SELECT CAST(' 1' AS INT64) FROM unit",
            EvaluationErrorKind::InvalidTextCast {
                target: ScalarType::Int64,
            },
        ),
        (
            "SELECT CAST('yes' AS BOOLEAN) FROM unit",
            EvaluationErrorKind::InvalidTextCast {
                target: ScalarType::Boolean,
            },
        ),
    ] {
        let graph = compile(source.as_bytes(), &catalog).unwrap();
        assert!(matches!(
            evaluate(&graph, &snapshot),
            Err(EvaluateError::Evaluation(shapesql::EvaluationError { kind, .. })) if kind == expected
        ));
    }
}

#[test]
fn evaluates_every_accepted_source_corpus_case() {
    let catalog = Catalog::new(vec![
        CatalogRelation::new(
            "outer_rows",
            "outer_rows",
            vec![
                catalog_field("id", ScalarType::Int64, false),
                catalog_field("group_id", ScalarType::Int64, false),
            ],
        ),
        CatalogRelation::new(
            "inner_rows",
            "inner_rows",
            vec![
                catalog_field("id", ScalarType::Int64, false),
                catalog_field("group_id", ScalarType::Int64, false),
                catalog_field("flag", ScalarType::Boolean, true),
            ],
        ),
    ]);
    let snapshot = snapshot([
        (
            "outer_rows",
            relation(
                vec![
                    input_field("id", ScalarType::Int64, false),
                    input_field("group_id", ScalarType::Int64, false),
                ],
                vec![row(vec![int(1), int(1)]), row(vec![int(2), int(2)])],
            ),
        ),
        (
            "inner_rows",
            relation(
                vec![
                    input_field("id", ScalarType::Int64, false),
                    input_field("group_id", ScalarType::Int64, false),
                    input_field("flag", ScalarType::Boolean, true),
                ],
                vec![
                    row(vec![int(1), int(1), boolean(true)]),
                    row(vec![int(2), int(1), boolean(false)]),
                    row(vec![int(3), int(2), Value::Null]),
                ],
            ),
        ),
    ]);

    for case in [
        "uncorrelated-exists.sql",
        "uncorrelated-in.sql",
        "grammar-core.sql",
        "set-precedence.sql",
        "grouped-boolean-aggregates.sql",
        "grouped-row-number.sql",
        "partitioned-boolean-aggregates.sql",
        "binding-alias-wildcard.sql",
        "order-by-ordinal.sql",
        "forward-cte-reference.sql",
        "typed-expressions.sql",
    ] {
        let source = fs::read(Path::new("tests/corpus/accepted").join(case)).unwrap();
        let graph = compile(&source, &catalog).unwrap_or_else(|error| panic!("{case}: {error}"));
        evaluate(&graph, &snapshot).unwrap_or_else(|error| panic!("{case}: {error}"));
    }
}
