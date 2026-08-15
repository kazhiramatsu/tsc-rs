use tsc_ci_core::{
    AdapterDescriptorError, AdapterDescriptorSetV1, AdapterDescriptorV1, AdapterIdV1,
    ImplementationIdV1, SchemaIdV1,
};

fn descriptor(value: u8) -> AdapterDescriptorV1 {
    AdapterDescriptorV1::try_new(
        AdapterIdV1::from_bytes([value; 16]),
        SchemaIdV1::from_bytes([value + 1; 16]),
        ImplementationIdV1::from_bytes([value + 2; 16]),
    )
    .expect("non-empty descriptor identities")
}

#[test]
fn descriptor_rejects_empty_identity() {
    let error = AdapterDescriptorV1::try_new(
        AdapterIdV1::from_bytes([0; 16]),
        SchemaIdV1::from_bytes([1; 16]),
        ImplementationIdV1::from_bytes([2; 16]),
    )
    .expect_err("zero adapter identity must be rejected");
    assert_eq!(error, AdapterDescriptorError::EmptyIdentity);
}

#[test]
fn descriptor_set_requires_strict_order_and_rejects_duplicates() {
    let first = descriptor(1);
    let second = descriptor(4);
    let set = AdapterDescriptorSetV1::try_from_sorted(vec![first, second])
        .expect("strictly ordered descriptors");
    assert_eq!(set.as_slice(), &[first, second]);

    let duplicate = AdapterDescriptorSetV1::try_from_sorted(vec![first, first])
        .expect_err("duplicate descriptors must be rejected");
    assert_eq!(duplicate, AdapterDescriptorError::Unsorted { index: 1 });
}
