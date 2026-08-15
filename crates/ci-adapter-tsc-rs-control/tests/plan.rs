use tsc_ci_adapter_control::{verify_plan, PlanError};
use tsc_ci_adapter_protocol::{CaseIdV1, FixedPlanV1, ShardRangeV1, ShardSpecV1};
use tsc_ci_core::{BoundedBytesSink, CanonicalSink, ObjectDigestV1, SchemaIdV1};

fn digest(byte: u8) -> ObjectDigestV1 {
    ObjectDigestV1::from_bytes([byte; 32])
}

#[test]
fn control_seals_only_an_exact_fixed_plan() {
    let plan = FixedPlanV1::try_new(
        CaseIdV1::try_new("profile".to_owned()).expect("profile"),
        CaseIdV1::try_new("suite".to_owned()).expect("suite"),
        SchemaIdV1::from_bytes([1; 16]),
        4,
        vec![
            ShardSpecV1::try_new(
                CaseIdV1::try_new("a".to_owned()).expect("shard"),
                ShardRangeV1::try_new(0, 2, 4).expect("range"),
                digest(2),
            )
            .expect("shard"),
            ShardSpecV1::try_new(
                CaseIdV1::try_new("b".to_owned()).expect("shard"),
                ShardRangeV1::try_new(2, 4, 4).expect("range"),
                digest(3),
            )
            .expect("shard"),
        ],
        digest(4),
        digest(5),
        vec![CaseIdV1::try_new("policy".to_owned()).expect("policy")],
    )
    .expect("plan");
    let verified = verify_plan(plan).expect("verified plan");
    assert_ne!(verified.digest().as_bytes(), &[0; 32]);
}

#[test]
fn exact_h2_source_and_plan_are_bound_without_copying_case_rows() {
    let plan = include_str!("../../../.github/ci/plans/h2-5g.v1.json");
    assert!(plan.contains("\"denominator\": 9027"));
    assert!(plan.contains("ratchets/h2-5g-qualification.v1.json"));
    assert!(plan.contains("\"membership_digest\": \"71929a845db200173c63c99b8adc9654e8e902179e91b3ea88bfe76ee4e1d395\""));
    assert_eq!(plan.matches("\"id\": \"h2-5g-").count(), 4);
}

#[test]
fn control_has_no_production_or_candidate_dependency_literal() {
    let source = include_str!("../src/lib.rs");
    for forbidden in [
        "compiler",
        "oracle",
        "harness",
        "xtask",
        "ActionModel",
        "AdapterCodec",
    ] {
        assert!(!source.contains(forbidden), "control literal: {forbidden}");
    }
    let mut sink = BoundedBytesSink::new(4096);
    let _ = sink.write(b"control-only");
    let _ = PlanError::EmptyPolicy;
}

#[test]
fn plan_types_have_no_callback_or_downcast_branch() {
    let source = include_str!("../../ci-adapter-tsc-rs-protocol/src/lib.rs");
    for forbidden in [
        "Any", "downcast", "Callback", "compiler", "oracle", "harness",
    ] {
        assert!(!source.contains(forbidden), "protocol literal: {forbidden}");
    }
}
