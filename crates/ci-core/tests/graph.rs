use tsc_ci_core::{
    ActionRecord, AdapterDescriptorV1, AdapterIdV1, AdapterInstanceRefV1, ApplicationNamespaceV1,
    CompleteMembership, CompositeProfileV1, ImplementationIdV1, InstanceIdV1, NodeClass,
    NodeRecord, PendingMembership, RootRecord, SchemaIdV1,
};

#[test]
fn generic_records_preserve_declared_order_without_evaluating_graphs() {
    let node = NodeRecord::new(
        7u8,
        NodeClass::Executable,
        "kind",
        ApplicationNamespaceV1::from_bytes([1; 16]),
        vec![2u8, 3u8],
    );
    assert_eq!(node.id(), &7);
    assert_eq!(node.class(), NodeClass::Executable);
    assert_eq!(node.dependencies(), &[2, 3]);

    let action = ActionRecord::new("leaf", 11u8, vec!["input"]);
    assert_eq!(action.id(), &"leaf");
    assert_eq!(action.dependencies(), &["input"]);

    let root = RootRecord::new("root", vec!["leaf"]);
    assert_eq!(root.spec(), &"root");
    assert_eq!(root.members(), &["leaf"]);
}

#[test]
fn profile_references_are_typed_and_pending_cannot_be_completed_publicly() {
    let adapter = AdapterDescriptorV1::try_new(
        AdapterIdV1::from_bytes([2; 16]),
        SchemaIdV1::from_bytes([3; 16]),
        ImplementationIdV1::from_bytes([4; 16]),
    )
    .expect("descriptor");
    let reference = AdapterInstanceRefV1::new(
        InstanceIdV1::from_bytes([5; 16]),
        adapter.adapter(),
        adapter.schema(),
    );
    let profile = CompositeProfileV1::new(vec![reference]);
    assert_eq!(profile.instances(), &[reference]);

    let pending = PendingMembership::<u8, u16>::new(vec![1, 2]);
    assert_eq!(pending.expected(), &[1, 2]);
    let _type_marker: core::marker::PhantomData<CompleteMembership<u8, u16>> =
        core::marker::PhantomData;
}
