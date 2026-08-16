use tsc_ci_core::{
    hash_action_key, hash_application_namespace, hash_input, hash_object,
    validate_namespace_lineage, validate_rename, ActionKeyV1, ApplicationNamespaceDigestV1,
    ApplicationNamespaceV1, NamespaceError, NamespaceLineageV1, ProtocolDomainTagV1,
};

#[test]
fn domain_registry_is_unique_and_fixed_width() {
    let tags = ProtocolDomainTagV1::all();
    for (index, tag) in tags.iter().enumerate() {
        assert_eq!(tag.as_bytes().len(), 16);
        assert_eq!(tag.domain().as_bytes(), tag.as_bytes());
        assert!(tags[index + 1..]
            .iter()
            .all(|other| tag.as_bytes() != other.as_bytes()));
    }
}

#[test]
fn purpose_specific_hashes_are_domain_separated() {
    let namespace = ApplicationNamespaceV1::from_bytes([7; 16]);
    let other_namespace = ApplicationNamespaceV1::from_bytes([8; 16]);
    let input = b"same canonical bytes";
    let first = hash_input(input);
    let object = hash_object(input);
    let action = hash_action_key(namespace, input);
    let other_action = hash_action_key(other_namespace, input);

    assert_ne!(first.as_bytes(), object.as_bytes());
    assert_ne!(action.as_bytes(), other_action.as_bytes());
    assert_ne!(action.as_bytes(), first.as_bytes());
    let _: ActionKeyV1 = action;
    let _: ApplicationNamespaceDigestV1 = hash_application_namespace(namespace);
}

#[test]
fn canonical_input_hash_uses_versioned_length_framing() {
    assert_eq!(
        hash_input(b"").as_bytes(),
        &[
            0xf4, 0x45, 0xe6, 0x78, 0xd0, 0xdd, 0xe3, 0xfb, 0x05, 0x3d, 0x2f, 0xd1, 0x62, 0x32,
            0x69, 0xcc, 0x6d, 0xb2, 0xeb, 0xd3, 0x1e, 0x93, 0x92, 0x48, 0x88, 0x93, 0x6e, 0xd7,
            0x33, 0x1a, 0x14, 0xc5,
        ]
    );
}

#[test]
fn namespace_lineage_rejects_empty_or_self_fork_and_preserves_rename_identity() {
    let empty = ApplicationNamespaceV1::from_bytes([0; 16]);
    let namespace = ApplicationNamespaceV1::from_bytes([1; 16]);
    assert_eq!(
        validate_namespace_lineage(empty, NamespaceLineageV1::Original),
        Err(NamespaceError::EmptyNamespace)
    );
    assert_eq!(
        validate_namespace_lineage(namespace, NamespaceLineageV1::Fork { parent: empty }),
        Err(NamespaceError::EmptyParent)
    );
    assert_eq!(
        validate_namespace_lineage(namespace, NamespaceLineageV1::Fork { parent: namespace }),
        Err(NamespaceError::SelfFork)
    );
    assert!(validate_rename(namespace, namespace).is_ok());
    assert_eq!(
        validate_rename(namespace, ApplicationNamespaceV1::from_bytes([2; 16])),
        Err(NamespaceError::RenameChangesIdentity)
    );
}
