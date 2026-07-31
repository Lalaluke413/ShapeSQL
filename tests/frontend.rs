use shapesql::{
    Catalog, CatalogField, CatalogRelation, Name, RelationBinding, ScalarType, TypeDescriptor,
    parse_owned,
};

#[test]
fn parsed_program_owns_the_source_used_by_its_spans() {
    let parsed = parse_owned(b"SELECT 'value' FROM relation").unwrap();
    assert_eq!(parsed.source(), "SELECT 'value' FROM relation");
    assert_eq!(parsed.syntax().span.end, parsed.source().len());
}

#[test]
fn catalog_snapshot_keeps_source_names_separate_from_host_bindings() {
    let relation = CatalogRelation::new(
        Name::new("orders"),
        RelationBinding::new("production.orders"),
        vec![CatalogField::new(
            "id",
            TypeDescriptor::non_nullable(ScalarType::Int64),
        )],
    );
    let catalog = Catalog::new(vec![relation]);

    assert_eq!(catalog.relations()[0].name.as_str(), "orders");
    assert_eq!(catalog.relations()[0].binding.as_str(), "production.orders");
}
