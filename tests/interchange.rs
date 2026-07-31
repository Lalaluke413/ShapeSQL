use std::fs;
use std::path::Path;

use shapesql::interchange::{EncodeError, decode, encode};
use shapesql::shape_ir::{CollectionKind, Graph, Node, NodeId, NodeKind};

const ACCEPTED: &[&str] = &[
    "accepted/minimal-input.shapeir.json",
    "accepted/int64-boundaries.shapeir.json",
    "accepted/demand-subquery.shapeir.json",
    "accepted/forward-shared-subgraph.shapeir.json",
    "accepted/scalar-expression-forms.shapeir.json",
    "accepted/relational-node-forms.shapeir.json",
];

fn fixture(relative: &str) -> Vec<u8> {
    fs::read(Path::new("tests/corpus/ir").join(relative)).unwrap()
}

#[test]
fn accepted_direct_ir_fixtures_decode_validate_and_round_trip() {
    for case in ACCEPTED {
        let graph = decode(&fixture(case)).unwrap_or_else(|error| panic!("{case}: {error}"));
        graph
            .validate()
            .unwrap_or_else(|error| panic!("{case}: {error}"));

        let encoded = encode(&graph).unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(encoded.ends_with('\n'));
        assert_eq!(decode(encoded.as_bytes()).unwrap(), graph, "{case}");
    }
}

#[test]
fn interchange_corpus_failures_stop_before_graph_validation() {
    for case in [
        "rejected/duplicate-member.shapeir.json",
        "rejected/unknown-semantic-member.shapeir.json",
        "rejected/int64-json-number.shapeir.json",
        "rejected/unsupported-version.shapeir.json",
    ] {
        assert!(decode(&fixture(case)).is_err(), "{case}");
    }
}

#[test]
fn semantic_corpus_failures_reconstruct_a_graph_before_validation() {
    for case in [
        "rejected/negative-slice-offset.shapeir.json",
        "rejected/unreachable-node.shapeir.json",
        "rejected/incorrect-output-schema.shapeir.json",
    ] {
        let graph = decode(&fixture(case)).unwrap_or_else(|error| panic!("{case}: {error}"));
        assert!(graph.validate().is_err(), "{case}");
    }
}

#[test]
fn duplicate_members_are_rejected_even_inside_metadata() {
    let document = br#"{
        "interchange_version":"0.1",
        "shape_ir_version":"0.1",
        "root":"n0",
        "nodes":[{"id":"n0","kind":"empty","output_schema":[],"collection":"bag"}],
        "metadata":{"tool":{"value":1,"value":2}}
    }"#;
    assert!(decode(document).is_err());
}

#[test]
fn metadata_accepts_json_numbers_outside_machine_ranges() {
    let document = br#"{
        "interchange_version":"0.1",
        "shape_ir_version":"0.1",
        "root":"n0",
        "nodes":[{"id":"n0","kind":"empty","output_schema":[],"collection":"bag"}],
        "metadata":{"tool.measurement":1.234567890123456789e9999}
    }"#;
    decode(document).unwrap().validate().unwrap();
}

#[test]
fn document_framing_and_unicode_errors_are_rejected() {
    let minimal = br#"{"interchange_version":"0.1","shape_ir_version":"0.1","root":"n0","nodes":[{"id":"n0","kind":"empty","output_schema":[],"collection":"bag"}]}"#;
    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend_from_slice(minimal);
    assert!(decode(&bom).is_err());

    let mut trailing = minimal.to_vec();
    trailing.extend_from_slice(b" {}");
    assert!(decode(&trailing).is_err());

    assert!(decode(br#"{"bad":"\uD800"}"#).is_err());
}

#[test]
fn canonical_int64_spelling_is_enforced_during_mapping() {
    for spelling in ["+1", "01", "-0", "9223372036854775808"] {
        let document = format!(
            r#"{{
                "interchange_version":"0.1",
                "shape_ir_version":"0.1",
                "root":"n0",
                "nodes":[{{
                    "id":"n0",
                    "kind":"input",
                    "binding":"rows",
                    "output_schema":[{{"id":"f0","name":"x","type":{{"scalar":"int64","nullable":false}}}}],
                    "collection":"bag",
                    "metadata":{{"ignored":"{spelling}"}}
                }}]
            }}"#
        );
        let graph = decode(document.as_bytes()).unwrap();
        graph.validate().unwrap();

        let literal_document = format!(
            r#"{{
                "interchange_version":"0.1",
                "shape_ir_version":"0.1",
                "root":"n0",
                "nodes":[{{
                    "id":"n0",
                    "kind":"filter",
                    "input":"n1",
                    "predicate":{{"kind":"literal","type":{{"scalar":"int64","nullable":false}},"value":"{spelling}"}},
                    "output_schema":[],
                    "collection":"bag"
                }},{{"id":"n1","kind":"empty","output_schema":[],"collection":"bag"}}]
            }}"#
        );
        assert!(decode(literal_document.as_bytes()).is_err(), "{spelling}");
    }
}

#[test]
fn encoder_rejects_invalid_graphs_and_nonportable_identities() {
    let invalid = decode(&fixture("rejected/negative-slice-offset.shapeir.json")).unwrap();
    assert!(matches!(encode(&invalid), Err(EncodeError::Validation(_))));

    let graph = Graph::new(
        NodeId::new("bad id"),
        vec![Node {
            id: NodeId::new("bad id"),
            kind: NodeKind::Empty,
            output_schema: vec![],
            collection: CollectionKind::Bag,
        }],
    );
    graph.validate().unwrap();
    assert!(matches!(
        encode(&graph),
        Err(EncodeError::InvalidIdentifier(identifier)) if identifier == "bad id"
    ));
}
