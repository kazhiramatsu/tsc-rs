use tsc_ci_core::{
    compare_graphs, digest_graph, validate_graph_transition, ActionGraph, ApplicationNamespaceV1,
    AuthorityReceiptDigestV1, BoundedBytesSink, CanonicalEncode, CanonicalValue, GraphTransitionV1,
    ImplementationIdV1, NodeClass, NodeRecord, ObjectDigestV1, TransitionApprovalV1,
    TransitionChangeV1, TransitionDecisionV1, TransitionError, TrustBindingV1, TrustRootV1,
};

fn value(text: &str) -> CanonicalValue {
    CanonicalValue::String(text.to_owned())
}

fn node(
    id: &str,
    kind: &str,
    dependencies: Vec<&str>,
) -> NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue> {
    NodeRecord::new(
        value(id),
        NodeClass::Derived,
        value(kind),
        value("spec"),
        dependencies.into_iter().map(value).collect(),
    )
}

fn graph(
    nodes: Vec<NodeRecord<CanonicalValue, CanonicalValue, CanonicalValue>>,
) -> ActionGraph<CanonicalValue, CanonicalValue, CanonicalValue> {
    ActionGraph::try_from_sorted(nodes).expect("sorted graph")
}

fn render<T: CanonicalEncode>(value: &T) -> Vec<u8> {
    let mut sink = BoundedBytesSink::new(16 * 1024);
    value.encode_canonical(&mut sink).expect("transition fits");
    sink.into_bytes()
}

fn trust_root() -> TrustRootV1 {
    TrustRootV1::try_new(
        ApplicationNamespaceV1::from_bytes([1; 16]),
        ImplementationIdV1::from_bytes([2; 16]),
        ObjectDigestV1::from_bytes([3; 32]),
        vec![TrustBindingV1::new(
            ImplementationIdV1::from_bytes([4; 16]),
            ImplementationIdV1::from_bytes([5; 16]),
        )],
        ObjectDigestV1::from_bytes([6; 32]),
        ImplementationIdV1::from_bytes([7; 16]),
        ImplementationIdV1::from_bytes([8; 16]),
    )
    .expect("trust root")
}

#[test]
fn paired_graphs_include_both_reverse_reaches_and_removed_nodes() {
    let prior = graph(vec![
        node("consumer", "old", vec!["source"]),
        node("source", "input", vec![]),
    ]);
    let current = graph(vec![
        node("consumer", "new", vec!["source"]),
        node("source", "input", vec![]),
    ]);
    let plan = compare_graphs(&prior, &current).expect("paired graph impact");
    assert_eq!(plan.changed_prior(), &[value("consumer")]);
    assert_eq!(plan.changed_current(), &[value("consumer")]);
    assert_eq!(plan.impacted(), &[value("consumer")]);

    let removed = graph(vec![node("consumer", "old", vec![])]);
    let removed_plan = compare_graphs(&prior, &removed).expect("removed graph impact");
    assert_eq!(
        removed_plan.changed_prior(),
        &[value("consumer"), value("source")]
    );
    assert_eq!(removed_plan.changed_current(), &[value("consumer")]);
    assert_eq!(removed_plan.impacted(), &[value("consumer")]);
}

#[test]
fn dependency_change_reaches_shared_consumers_and_closure_digest_is_bound() {
    let prior = graph(vec![
        node("consumer-a", "leaf", vec!["shared"]),
        node("consumer-b", "leaf", vec!["shared"]),
        node("shared", "input", vec![]),
    ]);
    let current = graph(vec![
        node("consumer-a", "leaf", vec!["shared"]),
        node("consumer-b", "changed", vec!["shared"]),
        node("shared", "input", vec![]),
    ]);
    let plan = compare_graphs(&prior, &current).expect("shared impact");
    assert_eq!(plan.changed_current(), &[value("consumer-b")]);
    assert_eq!(
        plan.impacted(),
        &[value("consumer-b")],
        "unrelated shared sibling is not over-impacted"
    );
    assert_ne!(
        digest_graph(&prior).expect("prior digest"),
        digest_graph(&current).expect("current digest")
    );
}

#[test]
fn protected_transition_rejects_self_approval_and_preserves_conservative_fallback() {
    let trust = trust_root();
    let current = GraphTransitionV1::try_new(
        Some(tsc_ci_core::GraphDigestV1::from_bytes([1; 32])),
        tsc_ci_core::GraphDigestV1::from_bytes([2; 32]),
        vec![TransitionChangeV1::OwnerNarrowing(value("owner"))],
        None,
    )
    .expect("unapproved narrowing");
    assert_eq!(
        validate_graph_transition(&current, &trust, ImplementationIdV1::from_bytes([9; 16])),
        Ok(TransitionDecisionV1::ConservativeSuperset)
    );

    let candidate = ImplementationIdV1::from_bytes([9; 16]);
    let self_approved = GraphTransitionV1::try_new(
        Some(tsc_ci_core::GraphDigestV1::from_bytes([1; 32])),
        tsc_ci_core::GraphDigestV1::from_bytes([2; 32]),
        vec![TransitionChangeV1::OwnerNarrowing(value("owner"))],
        Some(TransitionApprovalV1::new(
            candidate,
            AuthorityReceiptDigestV1::from_bytes([3; 32]),
        )),
    )
    .expect("candidate approval record");
    assert_eq!(
        validate_graph_transition(&self_approved, &trust, candidate),
        Err(TransitionError::CandidateSelfApproval)
    );

    let approved = GraphTransitionV1::try_new(
        Some(tsc_ci_core::GraphDigestV1::from_bytes([1; 32])),
        tsc_ci_core::GraphDigestV1::from_bytes([2; 32]),
        vec![TransitionChangeV1::OwnerNarrowing(value("owner"))],
        Some(TransitionApprovalV1::new(
            trust.transition_authority(),
            AuthorityReceiptDigestV1::from_bytes([4; 32]),
        )),
    )
    .expect("protected approval record");
    assert_eq!(
        validate_graph_transition(&approved, &trust, candidate),
        Ok(TransitionDecisionV1::Approved)
    );
}

#[test]
fn genesis_and_transition_canonical_bytes_are_stable() {
    let transition = GraphTransitionV1::try_new(
        None,
        tsc_ci_core::GraphDigestV1::from_bytes([2; 32]),
        vec![TransitionChangeV1::NodeAdded(value("root"))],
        None,
    )
    .expect("genesis transition");
    assert_eq!(
        render(&transition),
        br#"{"approval":null,"changes":[{"kind":"node_added","node":"root"}],"current":"0202020202020202020202020202020202020202020202020202020202020202","prior":null}"#
    );
    assert!(GraphTransitionV1::try_new(
        None,
        tsc_ci_core::GraphDigestV1::from_bytes([2; 32]),
        vec![TransitionChangeV1::NodeRemoved(value("root"))],
        None,
    )
    .is_err());
}

#[test]
fn impact_fixture_is_synthetic_and_source_has_no_repository_branch() {
    let fixture = include_str!("../../../../docs/design/greenfield/slices/impact-cases.v1.json");
    for forbidden in [
        "tsc-rs",
        "Cargo",
        "TypeScript",
        "compiler",
        "Git",
        "provider",
    ] {
        assert!(
            !fixture.contains(forbidden),
            "synthetic fixture literal: {forbidden}"
        );
    }
    let source = include_str!("../../src/impact.rs");
    for forbidden in [
        "tsc-rs",
        "Cargo",
        "TypeScript",
        "compiler",
        "Git",
        "Callback",
        "H2",
    ] {
        assert!(
            !source.contains(forbidden),
            "generic impact literal: {forbidden}"
        );
    }
}
