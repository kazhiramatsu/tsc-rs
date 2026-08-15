use tsc_ci_adapter_protocol::{
    ActionInvocationV1, CaseIdV1, FixedPlanV1, ObservationEnvelopeV1, ProtocolDecodeError,
    ProtocolError, RootReceiptV1, ShardRangeV1, ShardSpecV1,
};
use tsc_ci_core::{
    ActionKeyV1, ApplicationNamespaceV1, BoundedBytesSink, CanonicalEncode, GraphDigestV1,
    ImplementationIdV1, InputDigestV1, InvocationIdV1, ObjectDigestV1, SchemaIdV1,
};

fn digest(byte: u8) -> ObjectDigestV1 {
    ObjectDigestV1::from_bytes([byte; 32])
}

fn id(byte: u8) -> ImplementationIdV1 {
    ImplementationIdV1::from_bytes([byte; 16])
}

fn render<T: CanonicalEncode>(value: &T) -> Vec<u8> {
    let mut sink = BoundedBytesSink::new(16 * 1024);
    value.encode_canonical(&mut sink).expect("protocol fits");
    sink.into_bytes()
}

#[test]
fn invocation_and_observation_are_typed_and_bounded() {
    let invocation = ActionInvocationV1::try_new(
        ActionKeyV1::from_bytes([1; 32]),
        SchemaIdV1::from_bytes([2; 16]),
        id(3),
        InputDigestV1::from_bytes([4; 32]),
        InvocationIdV1::from_bytes([5; 16]),
        digest(6),
        0,
        0,
        32,
    )
    .expect("invocation");
    assert!(render(&invocation).starts_with(br#"{"action":"0101"#));
    assert_eq!(
        ObservationEnvelopeV1::try_new(
            id_action(),
            SchemaIdV1::from_bytes([2; 16]),
            id(3),
            0,
            vec![1, 2],
            1
        ),
        Err(ProtocolError::InvalidLimit)
    );
    let observation = ObservationEnvelopeV1::try_new(
        id_action(),
        SchemaIdV1::from_bytes([2; 16]),
        id(3),
        1,
        vec![1, 2],
        2,
    )
    .expect("bounded observation");
    assert_eq!(observation.bytes(), &[1, 2]);
}

#[test]
fn canonical_wire_round_trips_through_strict_typed_decoders() {
    let invocation = ActionInvocationV1::try_new(
        ActionKeyV1::from_bytes([1; 32]),
        SchemaIdV1::from_bytes([2; 16]),
        id(3),
        InputDigestV1::from_bytes([4; 32]),
        InvocationIdV1::from_bytes([5; 16]),
        digest(6),
        1,
        2,
        128,
    )
    .expect("invocation");
    let invocation_bytes = render(&invocation);
    let decoded = ActionInvocationV1::decode_canonical(&invocation_bytes, 1024)
        .expect("strict invocation decode");
    assert_eq!(decoded, invocation);

    let observation = ObservationEnvelopeV1::try_new(
        ActionKeyV1::from_bytes([7; 32]),
        SchemaIdV1::from_bytes([8; 16]),
        id(9),
        0,
        b"probe".to_vec(),
        128,
    )
    .expect("observation");
    let observation_bytes = render(&observation);
    let decoded = ObservationEnvelopeV1::decode_canonical(&observation_bytes, 1024)
        .expect("strict observation decode");
    assert_eq!(decoded, observation);
}

#[test]
fn typed_decoders_reject_noncanonical_or_malformed_wire() {
    let invocation = ActionInvocationV1::try_new(
        ActionKeyV1::from_bytes([1; 32]),
        SchemaIdV1::from_bytes([2; 16]),
        id(3),
        InputDigestV1::from_bytes([4; 32]),
        InvocationIdV1::from_bytes([5; 16]),
        digest(6),
        0,
        0,
        32,
    )
    .expect("invocation");
    let bytes = render(&invocation);
    assert!(matches!(
        ActionInvocationV1::decode_canonical(&bytes[..bytes.len() - 1], 1024),
        Err(ProtocolDecodeError::Canonical(_))
    ));
    let mut unknown = bytes;
    unknown[1] = b'X';
    assert!(ActionInvocationV1::decode_canonical(&unknown, 1024).is_err());
}

fn id_action() -> ActionKeyV1 {
    ActionKeyV1::from_bytes([7; 32])
}

#[test]
fn root_receipt_keeps_graph_profile_outcome_and_membership_separate() {
    let receipt = RootReceiptV1::new(
        GraphDigestV1::from_bytes([1; 32]),
        digest(2),
        digest(3),
        digest(4),
        digest(5),
    );
    assert_ne!(receipt.graph().as_bytes(), receipt.outcome().as_bytes());
    assert!(render(&receipt).starts_with(br#"{"graph":"0101"#));
    let _ = ApplicationNamespaceV1::from_bytes([9; 16]);
}

#[test]
fn fixed_plan_rejects_empty_ranges_gaps_and_unordered_membership() {
    let schema = SchemaIdV1::from_bytes([9; 16]);
    let first = ShardSpecV1::try_new(
        CaseIdV1::try_new("first".to_owned()).expect("case id"),
        ShardRangeV1::try_new(0, 2, 4).expect("range"),
        digest(1),
    )
    .expect("shard");
    let second = ShardSpecV1::try_new(
        CaseIdV1::try_new("second".to_owned()).expect("case id"),
        ShardRangeV1::try_new(2, 4, 4).expect("range"),
        digest(2),
    )
    .expect("shard");
    let plan = FixedPlanV1::try_new(
        CaseIdV1::try_new("profile".to_owned()).expect("profile"),
        CaseIdV1::try_new("suite".to_owned()).expect("suite"),
        schema,
        4,
        vec![first, second],
        digest(3),
        digest(4),
        vec![CaseIdV1::try_new("policy".to_owned()).expect("policy")],
    )
    .expect("fixed plan");
    assert_eq!(plan.denominator(), 4);
    assert_eq!(
        ShardRangeV1::try_new(2, 2, 4),
        Err(ProtocolError::InvalidRange)
    );
    assert!(FixedPlanV1::try_new(
        CaseIdV1::try_new("profile".to_owned()).expect("profile"),
        CaseIdV1::try_new("suite".to_owned()).expect("suite"),
        schema,
        4,
        vec![ShardSpecV1::try_new(
            CaseIdV1::try_new("first".to_owned()).expect("case id"),
            ShardRangeV1::try_new(1, 4, 4).expect("range"),
            digest(1),
        )
        .expect("shard")],
        digest(3),
        digest(4),
        vec![CaseIdV1::try_new("policy".to_owned()).expect("policy")],
    )
    .is_err());
}
