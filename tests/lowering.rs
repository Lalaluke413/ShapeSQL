use std::fs;
use std::path::Path;

use shapesql::shape_ir::{ExpressionKind, LiteralValue, NodeKind};
use shapesql::{Catalog, CatalogField, CatalogRelation, ScalarType, TypeDescriptor, compile};

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

fn compile_case(relative: &str) -> shapesql::shape_ir::Graph {
    let source = fs::read(Path::new("tests/corpus").join(relative)).unwrap();
    compile(&source, &catalog()).unwrap()
}

#[test]
fn lowers_every_accepted_source_fixture_to_valid_shape_ir() {
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
        let graph = compile_case(case);
        graph
            .validate()
            .unwrap_or_else(|error| panic!("{case}: {error}"));
    }
}

#[test]
fn lowers_grouping_windows_ordering_and_bounds_to_owned_nodes() {
    let graph = compile_case("accepted/grammar-core.sql");
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Aggregate { .. }))
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Order { .. }))
    );
    assert!(
        graph
            .nodes
            .iter()
            .any(|node| matches!(node.kind, NodeKind::Slice { .. }))
    );
}

#[test]
fn lowers_the_special_minimum_int64_as_one_ir_literal() {
    let graph = compile_case("accepted/typed-expressions.sql");
    let has_minimum = graph.nodes.iter().any(|node| {
        let NodeKind::Project { entries, .. } = &node.kind else {
            return false;
        };
        entries.iter().any(|entry| {
            let shapesql::shape_ir::ProjectEntry::Compute { expression, .. } = entry else {
                return false;
            };
            expression_contains_minimum(expression)
        })
    });
    assert!(has_minimum);
}

fn expression_contains_minimum(expression: &shapesql::shape_ir::Expression) -> bool {
    match &expression.kind {
        ExpressionKind::Literal(LiteralValue::Int64(value)) => *value == i64::MIN,
        ExpressionKind::Unary { operand, .. }
        | ExpressionKind::IsNull { operand, .. }
        | ExpressionKind::Cast { operand, .. } => expression_contains_minimum(operand),
        ExpressionKind::Binary { left, right, .. } => {
            expression_contains_minimum(left) || expression_contains_minimum(right)
        }
        ExpressionKind::Case { arms, fallback } => {
            arms.iter().any(|arm| {
                expression_contains_minimum(&arm.when) || expression_contains_minimum(&arm.then)
            }) || expression_contains_minimum(fallback)
        }
        ExpressionKind::InList { value, candidates } => {
            expression_contains_minimum(value) || candidates.iter().any(expression_contains_minimum)
        }
        ExpressionKind::InQuery { value, .. } => expression_contains_minimum(value),
        ExpressionKind::Literal(_) | ExpressionKind::Field(_) | ExpressionKind::Exists { .. } => {
            false
        }
    }
}

#[test]
fn shares_a_cte_subgraph_but_reidentifies_each_occurrence() {
    let source = b"WITH rows AS (SELECT id FROM inner_rows) SELECT l.id, r.id FROM rows AS l CROSS JOIN rows AS r";
    let graph = compile(source, &catalog()).unwrap();
    let inner_inputs = graph
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                &node.kind,
                NodeKind::Input { binding } if binding.as_str() == "inner_rows"
            )
        })
        .count();
    let occurrence_projects = graph
        .nodes
        .iter()
        .filter(|node| matches!(&node.kind, NodeKind::Project { .. }))
        .count();

    assert_eq!(inner_inputs, 1);
    assert!(occurrence_projects >= 3);
}

#[test]
fn omits_unreferenced_ctes_from_the_graph() {
    let source = b"WITH unused AS (SELECT id FROM inner_rows) SELECT id FROM outer_rows";
    let graph = compile(source, &catalog()).unwrap();

    assert!(graph.nodes.iter().all(|node| {
        !matches!(
            &node.kind,
            NodeKind::Input { binding } if binding.as_str() == "inner_rows"
        )
    }));
}
