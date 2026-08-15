use tsc_ci_core::{
    shortest_reason_paths, validate_budget, ActionGraph, BoundedBytesSink, BudgetError,
    BudgetFieldV1, CanonicalEncode, CanonicalValue, MissDifferenceV1, MissFieldV1, NodeClass,
    NodeRecord, PlanSets, PlanningBudgetV1, PlanningObservationV1, WhyMiss,
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

fn graph() -> ActionGraph<CanonicalValue, CanonicalValue, CanonicalValue> {
    ActionGraph::try_from_sorted(vec![
        node("consumer-a", vec!["source"]),
        node("consumer-b", vec!["source"]),
        node("source", vec![]),
    ])
    .expect("sorted graph")
}

fn render<T: CanonicalEncode>(value: &T) -> Vec<u8> {
    let mut sink = BoundedBytesSink::new(16 * 1024);
    value.encode_canonical(&mut sink).expect("explanation fits");
    sink.into_bytes()
}

#[test]
fn shortest_reason_paths_use_stable_lexicographic_ties() {
    let paths = shortest_reason_paths(&graph(), &[value("consumer-a"), value("consumer-b")])
        .expect("reason paths");
    assert_eq!(paths[0].path(), &[value("source"), value("consumer-a")]);
    assert_eq!(paths[1].path(), &[value("source"), value("consumer-b")]);
    assert!(shortest_reason_paths(&graph(), &[value("missing")]).is_err());
}

#[test]
fn plan_sets_keep_explicit_execution_classes_disjoint_and_canonical() {
    let sets = PlanSets::try_new(
        vec![value("changed")],
        vec![value("changed")],
        vec![value("carry")],
        vec![value("reuse")],
        vec![value("execute")],
        vec![value("revalidate")],
        vec![value("repack")],
        vec![value("rebuild")],
    )
    .expect("plan sets");
    assert_eq!(
        render(&sets),
        br#"{"cache_reuse":["reuse"],"carry_forward":["carry"],"changed":["changed"],"execute":["execute"],"impacted":["changed"],"rebuild":["rebuild"],"repack":["repack"],"revalidate":["revalidate"]}"#
    );
    assert!(PlanSets::try_new(
        vec![],
        vec![],
        vec![value("same")],
        vec![value("same")],
        vec![],
        vec![],
        vec![],
        vec![],
    )
    .is_err());
}

#[test]
fn why_miss_binds_first_field_and_reason_target() {
    let paths = shortest_reason_paths(&graph(), &[value("consumer-a")]).expect("path");
    let why = WhyMiss::try_new(
        value("consumer-a"),
        MissDifferenceV1::new(
            MissFieldV1::Input,
            tsc_ci_core::ObjectDigestV1::from_bytes([1; 32]),
            Some(tsc_ci_core::ObjectDigestV1::from_bytes([2; 32])),
        ),
        paths.into_vec().remove(0),
    )
    .expect("why miss");
    assert_eq!(why.difference().field(), MissFieldV1::Input);
    assert!(render(&why).starts_with(br#"{"action":"consumer-a","difference":{"available":"0202"#));
}

#[test]
fn budgets_reject_zero_and_over_limit_observations() {
    let budget = PlanningBudgetV1::try_new(1000, 1024, 512, 512, 512, 512, 512, 1).expect("budget");
    let exact = PlanningObservationV1::new(1000, 1024, 512, 512, 512, 512, 512, 1);
    assert!(validate_budget(&budget, &exact).is_ok());
    let over = PlanningObservationV1::new(1001, 1024, 512, 512, 512, 512, 512, 1);
    assert_eq!(
        validate_budget(&budget, &over),
        Err(BudgetError::Exceeded(BudgetFieldV1::ControlCpuMillis))
    );
    assert_eq!(
        PlanningBudgetV1::try_new(0, 1, 1, 1, 1, 1, 1, 1),
        Err(BudgetError::ZeroCeiling(BudgetFieldV1::ControlCpuMillis))
    );
}

#[test]
fn explanation_fixture_and_source_are_pure() {
    let fixture =
        include_str!("../../../../docs/design/greenfield/slices/explanation-cases.v1.json");
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
    let source = include_str!("../../src/explain.rs");
    for forbidden in [
        "std::process",
        "SystemTime",
        "Instant",
        "env::",
        "tsc-rs",
        "H2",
    ] {
        assert!(
            !source.contains(forbidden),
            "pure explanation literal: {forbidden}"
        );
    }
}
