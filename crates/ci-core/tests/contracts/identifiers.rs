use tsc_ci_core::{
    ApplicationNamespaceV1, CanonicalInputRefV1, ImplementationIdV1, InputDigestV1, ObjectDigestV1,
    ProtocolDomainV1, SchemaIdV1,
};

#[test]
fn fixed_identifiers_round_trip_without_cross_type_aliases() {
    let bytes = [0x2a; 16];
    let domain = ProtocolDomainV1::from_bytes(bytes);
    let namespace = ApplicationNamespaceV1::from_bytes(bytes);
    let schema = SchemaIdV1::from_bytes(bytes);
    let implementation = ImplementationIdV1::from_bytes(bytes);

    assert_eq!(domain.as_bytes(), &bytes);
    assert_eq!(namespace.as_bytes(), &bytes);
    assert_eq!(schema.as_bytes(), &bytes);
    assert_eq!(implementation.as_bytes(), &bytes);
    assert_eq!(domain, ProtocolDomainV1::from_bytes(bytes));
}

#[test]
fn purpose_specific_digests_and_input_reference_are_orderable() {
    let first = InputDigestV1::from_bytes([1; 32]);
    let second = InputDigestV1::from_bytes([2; 32]);
    assert!(first < second);
    assert_eq!(ObjectDigestV1::from_bytes([3; 32]).as_bytes(), &[3; 32]);

    let input = CanonicalInputRefV1 {
        namespace: ApplicationNamespaceV1::from_bytes([4; 16]),
        schema: SchemaIdV1::from_bytes([5; 16]),
        implementation: ImplementationIdV1::from_bytes([6; 16]),
        payload: first,
    };
    assert_eq!(input.payload, first);
}
