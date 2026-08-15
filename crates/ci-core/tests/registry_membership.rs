use tsc_ci_core::{
    complete_adapter_input, complete_composite_input, AdapterCodec, AdapterDecodeError,
    AdapterDescriptorSetV1, AdapterDescriptorV1, AdapterIdV1, AdapterRegistration,
    BoundedBytesSink, CanonicalEncode, CanonicalValue, ImplementationIdV1, MembershipError,
    PendingMembership, RegistryError, SchemaIdV1,
};

struct FirstCodec;

impl AdapterCodec for FirstCodec {
    type RawObservation = CanonicalValue;

    fn descriptor() -> AdapterDescriptorV1 {
        AdapterDescriptorV1::try_new(
            AdapterIdV1::from_bytes([1; 16]),
            SchemaIdV1::from_bytes([2; 16]),
            ImplementationIdV1::from_bytes([3; 16]),
        )
        .expect("non-empty descriptor")
    }

    fn decode(bytes: &[u8]) -> Result<Self::RawObservation, AdapterDecodeError> {
        tsc_ci_core::decode_canonical(bytes, 1024, 8).map_err(|error| match error {
            tsc_ci_core::DecodeError::NonCanonical => AdapterDecodeError::NonCanonical,
            _ => AdapterDecodeError::Malformed,
        })
    }
}

struct SecondCodec;

impl AdapterCodec for SecondCodec {
    type RawObservation = CanonicalValue;

    fn descriptor() -> AdapterDescriptorV1 {
        AdapterDescriptorV1::try_new(
            AdapterIdV1::from_bytes([4; 16]),
            SchemaIdV1::from_bytes([5; 16]),
            ImplementationIdV1::from_bytes([6; 16]),
        )
        .expect("non-empty descriptor")
    }

    fn decode(bytes: &[u8]) -> Result<Self::RawObservation, AdapterDecodeError> {
        FirstCodec::decode(bytes)
    }
}

fn canonical(value: &CanonicalValue) -> Vec<u8> {
    let mut sink = BoundedBytesSink::new(1024);
    value.encode_canonical(&mut sink).expect("small value");
    sink.into_bytes()
}

#[test]
fn registry_seal_freezes_descriptors_and_keeps_typed_decode_private() {
    let first = AdapterRegistration::of::<FirstCodec>();
    let second = AdapterRegistration::of::<SecondCodec>();
    let expected =
        AdapterDescriptorSetV1::try_from_sorted(vec![first.descriptor(), second.descriptor()])
            .expect("strict descriptor set");
    let mut builder = tsc_ci_core::AdapterRegistryBuilder::new();
    builder.register(second).expect("second registration");
    builder.register(first).expect("first registration");
    let registry = builder.seal(&expected).expect("sealed registry");
    assert!(registry.digest().as_bytes().iter().any(|byte| *byte != 0));
    assert_eq!(
        registry
            .decode_reencode(first.descriptor(), br#"{"a":1}"#)
            .expect("canonical decode/re-encode"),
        br#"{"a":1}"#
    );
    assert!(matches!(
        registry.decode_reencode(first.descriptor(), br#"{"a":01}"#),
        Err(RegistryError::Decode(AdapterDecodeError::NonCanonical))
    ));
}

#[test]
fn registry_rejects_duplicate_missing_and_unexpected_registration() {
    let first = AdapterRegistration::of::<FirstCodec>();
    let mut duplicate = tsc_ci_core::AdapterRegistryBuilder::new();
    duplicate.register(first).expect("first registration");
    assert_eq!(duplicate.register(first), Err(RegistryError::Duplicate));
    let empty = AdapterDescriptorSetV1::try_from_sorted(Vec::new()).expect("empty expected set");
    let mut unexpected = tsc_ci_core::AdapterRegistryBuilder::new();
    unexpected
        .register(AdapterRegistration::of::<FirstCodec>())
        .expect("registration");
    assert!(matches!(
        unexpected.seal(&empty),
        Err(RegistryError::Unexpected)
    ));
}

#[test]
fn membership_transitions_only_after_exact_ordered_values() {
    let pending = PendingMembership::<CanonicalValue, CanonicalValue>::new(vec![
        CanonicalValue::String("a".to_owned()),
        CanonicalValue::String("b".to_owned()),
    ]);
    let values = vec![
        (CanonicalValue::String("a".to_owned()), CanonicalValue::Null),
        (
            CanonicalValue::String("b".to_owned()),
            CanonicalValue::Bool(true),
        ),
    ];
    let complete = complete_adapter_input(&pending, values.clone()).expect("exact membership");
    assert_eq!(complete.values(), values.as_slice());
    let composite = complete_composite_input(&pending, values).expect("composite membership");
    assert_eq!(composite.values().len(), 2);

    let missing = vec![(CanonicalValue::String("a".to_owned()), CanonicalValue::Null)];
    assert_eq!(
        complete_adapter_input(&pending, missing),
        Err(MembershipError::Missing { index: 1 })
    );
    let duplicate = vec![
        (CanonicalValue::String("a".to_owned()), CanonicalValue::Null),
        (CanonicalValue::String("a".to_owned()), CanonicalValue::Null),
    ];
    assert_eq!(
        complete_adapter_input(&pending, duplicate),
        Err(MembershipError::Duplicate { index: 1 })
    );
}

#[test]
fn generic_registry_and_membership_source_has_no_repository_branch() {
    for source in [
        include_str!("../src/registry.rs"),
        include_str!("../src/membership.rs"),
    ] {
        for forbidden in ["tsc-rs", "H2", "Cargo", "compiler", "downcast", "Any"] {
            assert!(!source.contains(forbidden), "generic literal: {forbidden}");
        }
    }
    assert_eq!(canonical(&CanonicalValue::Null), b"null");
}
