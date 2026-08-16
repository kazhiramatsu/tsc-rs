use tsc_ci_core::{
    validate_declared_closures, validate_global_id_sets, validate_graph, ActionGraph,
    CanonicalEncode, CanonicalValue, ClosureRecord, GraphValidationError, NodeClass, NodeRecord,
};

fn value(text: &str) -> CanonicalValue {
    CanonicalValue::String(text.to_owned())
}

fn node(
    id: &str,
    dependencies: Vec<&str>,
) -> NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue> {
    NodeRecord::new(
        value(id),
        NodeClass::Derived,
        value("kind"),
        value("spec"),
        dependencies.into_iter().map(value).collect(),
    )
}

fn graph(
    nodes: Vec<NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue>>,
) -> ActionGraph<CanonicalValue, CanonicalValue, CanonicalValue> {
    ActionGraph::try_from_sorted(nodes).expect("sorted graph fixture")
}

#[test]
fn validation_derives_stable_topological_order_and_transitive_closures() {
    let graph = graph(vec![
        node("a", vec!["b", "c"]),
        node("b", vec!["c"]),
        node("c", vec![]),
    ]);
    let validated = validate_graph(&graph).expect("acyclic graph");
    let order = validated.plan().order();
    assert_eq!(order, &[value("c"), value("b"), value("a")]);
    let closure = validated
        .closures()
        .iter()
        .find(|closure| closure.node() == &value("a"))
        .expect("closure for a");
    assert_eq!(closure.members(), &[value("a"), value("b"), value("c")]);

    let mut declared = validated.closures().to_vec();
    validate_declared_closures(&graph, &declared).expect("fresh closure records");
    declared[0] = ClosureRecord::new(
        declared[0].node().clone(),
        vec![value("stale")],
        declared[0].digest(),
    );
    assert!(matches!(
        validate_declared_closures(&graph, &declared),
        Err(GraphValidationError::StaleClosure { .. })
    ));
}

#[test]
fn invalid_edges_and_cycles_fail_closed() {
    let missing = graph(vec![node("a", vec!["missing"])]);
    assert_eq!(
        validate_graph(&missing),
        Err(GraphValidationError::MissingDependency {
            node_index: 0,
            dependency_index: 0,
        })
    );

    let duplicate = graph(vec![node("a", vec!["b", "b"]), node("b", vec![])]);
    assert_eq!(
        validate_graph(&duplicate),
        Err(GraphValidationError::DuplicateDependency {
            node_index: 0,
            dependency_index: 1,
        })
    );

    let cycle = graph(vec![node("a", vec!["b"]), node("b", vec!["a"])]);
    assert_eq!(validate_graph(&cycle), Err(GraphValidationError::Cycle));
}

#[test]
fn global_id_sets_reject_cross_shape_collisions() {
    let first = [value("a")];
    let second = [value("b")];
    assert!(validate_global_id_sets(&[&first, &second]).is_ok());
    assert_eq!(
        validate_global_id_sets(&[&first, &first]),
        Err(GraphValidationError::GlobalIdCollision {
            set_index: 1,
            item_index: 0,
        })
    );
}

#[test]
fn canonical_id_bound_is_available_for_closure_digest() {
    let mut sink = tsc_ci_core::BoundedBytesSink::new(64);
    value("id").encode_canonical(&mut sink).expect("small id");
    assert_eq!(sink.bytes(), br#""id""#);
}
