use tsc_ci_core::{
    ActionGraph, AdapterIdV1, AdapterInstanceRefV1, ApplicationNamespaceV1, BoundedBytesSink,
    CanonicalEncode, CanonicalError, CanonicalValue, CompositeProfileV1, GraphSchemaError,
    InstanceIdV1, NodeClass, NodeRecord, SchemaIdV1,
};

fn value(text: &str) -> CanonicalValue {
    CanonicalValue::String(text.to_owned())
}

fn node(
    id: &str,
    class: NodeClass,
    dependencies: Vec<&str>,
) -> NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue> {
    NodeRecord::new(
        value(id),
        class,
        value("kind"),
        value("spec"),
        dependencies.into_iter().map(value).collect(),
    )
}

fn render<I, K, S>(graph: &ActionGraph<I, K, S>) -> Vec<u8>
where
    I: CanonicalEncode,
    K: CanonicalEncode,
    S: CanonicalEncode,
{
    let mut sink = BoundedBytesSink::new(4096);
    graph
        .encode_canonical(&mut sink)
        .expect("graph fixture fits");
    sink.into_bytes()
}

#[test]
fn h2_shaped_and_flat_graphs_share_one_canonical_shape() {
    let executable = node("a", NodeClass::Executable, vec!["input"]);
    let input = node("input", NodeClass::Input, vec![]);
    let h2 = ActionGraph::try_from_sorted(vec![executable.clone(), input.clone()])
        .expect("sorted H2-shaped graph");
    let flat = ActionGraph::try_from_sorted(vec![
        node("input", NodeClass::Input, vec![]),
        node("a", NodeClass::Derived, vec!["input"]),
    ]);
    assert!(flat.is_err(), "flat graph must still obey id ordering");
    let flat = ActionGraph::try_from_sorted(vec![
        node("a", NodeClass::Derived, vec!["input"]),
        node("input", NodeClass::Input, vec![]),
    ])
    .expect("sorted flat graph");
    assert_ne!(render(&h2), render(&flat));
    assert!(render(&h2).starts_with(br#"{"nodes":[{"class":"executable""#));
}

#[test]
fn graph_rejects_duplicate_or_unsorted_ids_before_rendering() {
    let first = node("a", NodeClass::Input, vec![]);
    let second = node("a", NodeClass::Derived, vec![]);
    assert_eq!(
        ActionGraph::try_from_sorted(vec![first.clone(), second]),
        Err(GraphSchemaError::Unsorted { index: 1 })
    );
    assert_eq!(
        ActionGraph::try_from_sorted(vec![first.clone(), node("0", NodeClass::Input, vec![])]),
        Err(GraphSchemaError::Unsorted { index: 1 })
    );
}

#[test]
fn composite_profile_is_strictly_ordered_and_canonical() {
    let first = AdapterInstanceRefV1::new(
        InstanceIdV1::from_bytes([1; 16]),
        AdapterIdV1::from_bytes([2; 16]),
        SchemaIdV1::from_bytes([3; 16]),
    );
    let second = AdapterInstanceRefV1::new(
        InstanceIdV1::from_bytes([4; 16]),
        AdapterIdV1::from_bytes([5; 16]),
        SchemaIdV1::from_bytes([6; 16]),
    );
    let profile =
        CompositeProfileV1::try_from_sorted(vec![first, second]).expect("strict profile order");
    let mut sink = BoundedBytesSink::new(4096);
    profile
        .encode_canonical(&mut sink)
        .expect("profile fixture fits");
    assert_eq!(
        sink.bytes(),
        br#"{"instances":[{"adapter":"02020202020202020202020202020202","instance":"01010101010101010101010101010101","schema":"03030303030303030303030303030303"},{"adapter":"05050505050505050505050505050505","instance":"04040404040404040404040404040404","schema":"06060606060606060606060606060606"}]}"#
    );

    let unsorted = CompositeProfileV1::new(vec![second, first]);
    let mut sink = BoundedBytesSink::new(4096);
    assert_eq!(
        unsorted.encode_canonical(&mut sink),
        Err(CanonicalError::InvalidKeyOrder)
    );
}

#[test]
fn generic_graph_source_has_no_repository_literals_or_callbacks() {
    let source = include_str!("../../src/graph_schema.rs");
    for forbidden in ["tsc-rs", "H2", "Cargo", "compiler", "Callback"] {
        assert!(!source.contains(forbidden), "generic literal: {forbidden}");
    }
    let _ = ApplicationNamespaceV1::from_bytes([9; 16]);
}
