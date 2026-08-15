use tsc_ci_core::{
    ActionKeyV1, BoundedBytesSink, BuildSystemOwnershipV1, CanonicalEncode, CollisionKindV1,
    GeneratedOwnershipV1, GlobalDispositionV1, ImplementationIdV1, InventoryEntryV1,
    InventoryError, NegativeLookupV1, NormalizedPathV1, ObjectDigestV1, PathCollisionV1,
    SchemaIdV1, UnknownInputPolicyV1, WorkspaceInventorySpecV1,
};

fn path(value: &str) -> NormalizedPathV1 {
    NormalizedPathV1::try_new(value.as_bytes().to_vec()).expect("normalized path")
}

fn render<T: CanonicalEncode>(value: &T) -> Vec<u8> {
    let mut sink = BoundedBytesSink::new(16 * 1024);
    value.encode_canonical(&mut sink).expect("inventory fits");
    sink.into_bytes()
}

#[test]
fn inventory_spec_keeps_global_dispositions_and_ownership_sorted() {
    let entries = vec![
        InventoryEntryV1::new(
            path("a.txt"),
            GlobalDispositionV1::Present,
            Some(ObjectDigestV1::from_bytes([1; 32])),
        ),
        InventoryEntryV1::new(path("generated/out"), GlobalDispositionV1::Generated, None),
    ];
    let negatives = vec![NegativeLookupV1::try_new(
        path("missing.txt"),
        SchemaIdV1::from_bytes([2; 16]),
        vec![path("roots")],
        ObjectDigestV1::from_bytes([3; 32]),
    )
    .expect("sorted negative lookup")];
    let generated = vec![GeneratedOwnershipV1::new(
        path("generated/out"),
        ActionKeyV1::from_bytes([4; 32]),
        ImplementationIdV1::from_bytes([5; 16]),
    )];
    let build = vec![BuildSystemOwnershipV1::try_new(
        ImplementationIdV1::from_bytes([6; 16]),
        vec![path("src/input")],
        vec![path("generated/out")],
        false,
    )
    .expect("transparent generated ownership")];
    let spec = WorkspaceInventorySpecV1::try_new(
        entries,
        negatives,
        generated,
        build,
        UnknownInputPolicyV1::FailClosed,
    )
    .expect("complete inventory spec");
    assert_eq!(spec.unknown_policy(), UnknownInputPolicyV1::FailClosed);
    assert_eq!(
        spec.entries()[1].disposition(),
        GlobalDispositionV1::Generated
    );
}

#[test]
fn inventory_rejects_path_traversal_collisions_and_unsorted_lists() {
    assert!(NormalizedPathV1::try_new(b"../escape".to_vec()).is_err());
    assert!(NormalizedPathV1::try_new(b"a//b".to_vec()).is_err());
    let first = path("A");
    let second = path("a");
    let collision =
        PathCollisionV1::try_new(first.clone(), second.clone(), CollisionKindV1::CaseFolded)
            .expect("ordered collision record");
    assert_eq!(collision.kind(), CollisionKindV1::CaseFolded);
    assert_eq!(
        PathCollisionV1::try_new(second, first, CollisionKindV1::Exact),
        Err(InventoryError::Collision { index: 1 })
    );
    let exact = PathCollisionV1::try_new(path("same"), path("same"), CollisionKindV1::Exact)
        .expect("exact duplicate path collision");
    assert_eq!(exact.first(), exact.second());

    let entries = vec![
        InventoryEntryV1::new(path("b"), GlobalDispositionV1::Present, None),
        InventoryEntryV1::new(path("a"), GlobalDispositionV1::Deleted, None),
    ];
    assert_eq!(
        WorkspaceInventorySpecV1::try_new(
            entries,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            UnknownInputPolicyV1::ImpactAll,
        ),
        Err(InventoryError::Unsorted { index: 1 })
    );
}

#[test]
fn inventory_canonical_bytes_are_stable_and_key_sorted() {
    let spec = WorkspaceInventorySpecV1::try_new(
        vec![InventoryEntryV1::new(
            path("a\".txt"),
            GlobalDispositionV1::Present,
            Some(ObjectDigestV1::from_bytes([1; 32])),
        )],
        vec![NegativeLookupV1::try_new(
            path("missing"),
            SchemaIdV1::from_bytes([2; 16]),
            vec![path("root")],
            ObjectDigestV1::from_bytes([3; 32]),
        )
        .expect("negative lookup")],
        vec![GeneratedOwnershipV1::new(
            path("out"),
            ActionKeyV1::from_bytes([4; 32]),
            ImplementationIdV1::from_bytes([5; 16]),
        )],
        vec![BuildSystemOwnershipV1::try_new(
            ImplementationIdV1::from_bytes([6; 16]),
            vec![path("in")],
            vec![path("out")],
            true,
        )
        .expect("build ownership")],
        UnknownInputPolicyV1::FailClosed,
    )
    .expect("inventory spec");
    assert_eq!(
        render(&spec),
        br#"{"build_systems":[{"inputs":["in"],"opaque":true,"outputs":["out"],"producer":"06060606060606060606060606060606"}],"entries":[{"content":"0101010101010101010101010101010101010101010101010101010101010101","disposition":"present","path":"a\".txt"}],"generated":[{"generator":"0404040404040404040404040404040404040404040404040404040404040404","implementation":"05050505050505050505050505050505","output":"out"}],"negatives":[{"algorithm":"02020202020202020202020202020202","listing_digest":"0303030303030303030303030303030303030303030303030303030303030303","requested":"missing","roots":["root"]}],"unknown_policy":"fail_closed"}"#
    );
}

#[test]
fn unknown_policy_and_opaque_build_ownership_are_explicit() {
    let build = BuildSystemOwnershipV1::try_new(
        ImplementationIdV1::from_bytes([1; 16]),
        vec![path("input")],
        vec![path("out")],
        true,
    )
    .expect("opaque producer record");
    assert!(build.opaque());
    assert_eq!(
        UnknownInputPolicyV1::ImpactAll,
        UnknownInputPolicyV1::ImpactAll
    );
}

#[test]
fn inventory_source_has_no_repository_or_tool_specific_branch() {
    let source = include_str!("../src/inventory.rs");
    for forbidden in ["tsc-rs", "Cargo", "TypeScript", "compiler", "Git", "H2"] {
        assert!(
            !source.contains(forbidden),
            "generic inventory literal: {forbidden}"
        );
    }
}
