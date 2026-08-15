use tsc_ci_core::{
    BuildComponentSetV1, BuildComponentV1, DisclosureEntryV1, DisclosureError, DisclosureHistoryV1,
    EvidenceAudienceV1, ExecutionPlatformV1, FilesystemAccessV1, IdentityError, ImplementationIdV1,
    InvocationIdV1, InvocationIdentityV1, NetworkAccessV1, ObjectDigestV1, PlatformTokenV1,
    ProcessObservationStatusV1, ProcessObservationV1, PublicEnvironmentEntryV1, ReuseScopeV1,
    SandboxCapabilitiesV1, SchemaIdV1, SecretFreeEnvironmentV1, ToolIdV1, ToolRefV1, ToolRoleV1,
    ToolchainSetV1,
};

fn platform(value: u8) -> ExecutionPlatformV1 {
    let token = PlatformTokenV1::from_bytes([value; 16]);
    ExecutionPlatformV1::new(
        token, token, token, token, token, token, token, token, false,
    )
}

fn tool(value: u8) -> ToolRefV1 {
    ToolRefV1::new(
        ToolIdV1::from_bytes([value; 16]),
        ToolRoleV1::from_bytes([value + 1; 16]),
        ObjectDigestV1::from_bytes([value + 2; 32]),
        platform(value),
    )
}

#[test]
fn tool_and_build_sets_require_strict_order() {
    let first = tool(1);
    let second = tool(3);
    assert!(ToolchainSetV1::try_from_sorted(vec![first, second]).is_ok());
    assert_eq!(
        ToolchainSetV1::try_from_sorted(vec![second, first]),
        Err(IdentityError::Unsorted { index: 1 })
    );

    let first = BuildComponentV1::new(
        ToolIdV1::from_bytes([1; 16]),
        SchemaIdV1::from_bytes([2; 16]),
        ObjectDigestV1::from_bytes([3; 32]),
    );
    let second = BuildComponentV1::new(
        ToolIdV1::from_bytes([4; 16]),
        SchemaIdV1::from_bytes([5; 16]),
        ObjectDigestV1::from_bytes([6; 32]),
    );
    assert!(BuildComponentSetV1::try_from_sorted(vec![first, second]).is_ok());
}

#[test]
fn secret_free_environment_is_explicit_and_ordered() {
    let public = PublicEnvironmentEntryV1::try_new(b"LANG".to_vec(), b"C".to_vec())
        .expect("non-empty public key");
    let secret = PublicEnvironmentEntryV1::try_new(b"TOKEN".to_vec(), b"redacted".to_vec())
        .expect("non-empty key");
    assert!(SecretFreeEnvironmentV1::try_from_sorted(vec![public.clone()], &[b"TOKEN"]).is_ok());
    assert_eq!(
        SecretFreeEnvironmentV1::try_from_sorted(vec![secret], &[b"TOKEN"]),
        Err(IdentityError::SecretEnvironmentKey)
    );
    assert_eq!(
        SecretFreeEnvironmentV1::try_from_sorted(vec![public.clone(), public], &[]),
        Err(IdentityError::Unsorted { index: 1 })
    );
}

#[test]
fn disclosure_history_can_only_grow_and_preserves_first_events() {
    let audience_a = EvidenceAudienceV1::from_bytes([1; 16]);
    let audience_b = EvidenceAudienceV1::from_bytes([2; 16]);
    let first = DisclosureEntryV1::new(
        audience_a,
        tsc_ci_core::PublicationEventDigestV1::from_bytes([3; 32]),
    );
    let second = DisclosureEntryV1::new(
        audience_b,
        tsc_ci_core::PublicationEventDigestV1::from_bytes([4; 32]),
    );
    let prior = DisclosureHistoryV1::try_from_sorted(vec![first]).expect("sorted prior history");
    let replacement =
        DisclosureHistoryV1::try_from_sorted(vec![first, second]).expect("expanded history");
    let merged =
        DisclosureHistoryV1::merge_monotonic(&prior, &replacement).expect("monotonic expansion");
    assert_eq!(merged.as_slice(), &[first, second]);

    let shrunk = DisclosureHistoryV1::try_from_sorted(Vec::new()).expect("empty replacement");
    assert_eq!(
        DisclosureHistoryV1::merge_monotonic(&prior, &shrunk),
        Err(DisclosureError::Shrunk { index: 0 })
    );
    let changed = DisclosureHistoryV1::try_from_sorted(vec![DisclosureEntryV1::new(
        audience_a,
        tsc_ci_core::PublicationEventDigestV1::from_bytes([9; 32]),
    )])
    .expect("sorted changed history");
    assert_eq!(
        DisclosureHistoryV1::merge_monotonic(&prior, &changed),
        Err(DisclosureError::ChangedFirstEvent { index: 0 })
    );
}

#[test]
fn sandbox_identity_has_no_effect_constructor_and_observation_is_typed() {
    let abi = SchemaIdV1::from_bytes([7; 16]);
    assert_eq!(
        SandboxCapabilitiesV1::new(
            abi,
            NetworkAccessV1::Disabled,
            FilesystemAccessV1::ReadOnly,
            0
        ),
        Err(IdentityError::InvalidLimit)
    );
    let capabilities = SandboxCapabilitiesV1::new(
        abi,
        NetworkAccessV1::Disabled,
        FilesystemAccessV1::ReadOnly,
        1024,
    )
    .expect("positive output limit");
    assert_eq!(capabilities.network(), NetworkAccessV1::Disabled);
    let observation = ProcessObservationV1::new(
        ProcessObservationStatusV1::Exited { code: 0 },
        ObjectDigestV1::from_bytes([1; 32]),
        ObjectDigestV1::from_bytes([2; 32]),
        false,
    );
    assert_eq!(
        observation.status(),
        ProcessObservationStatusV1::Exited { code: 0 }
    );
}

#[test]
fn invocation_identity_binds_platform_toolchain_and_capture_versions() {
    let environment = SecretFreeEnvironmentV1::try_from_sorted(Vec::new(), &[])
        .expect("empty public environment");
    let toolchain = ToolchainSetV1::try_from_sorted(vec![tool(1)]).expect("one tool");
    let sandbox = SandboxCapabilitiesV1::new(
        SchemaIdV1::from_bytes([4; 16]),
        NetworkAccessV1::Disabled,
        FilesystemAccessV1::ReadOnly,
        2048,
    )
    .expect("positive output limit");
    let invocation = InvocationIdentityV1::new(
        InvocationIdV1::from_bytes([1; 16]),
        tsc_ci_core::ApplicationNamespaceV1::from_bytes([2; 16]),
        SchemaIdV1::from_bytes([3; 16]),
        ImplementationIdV1::from_bytes([4; 16]),
        tsc_ci_core::ActionKeyV1::from_bytes([5; 32]),
        vec![b"--stable".to_vec()],
        b"/work".to_vec(),
        environment,
        platform(1),
        toolchain,
        sandbox,
        ImplementationIdV1::from_bytes([6; 16]),
        ImplementationIdV1::from_bytes([7; 16]),
        ImplementationIdV1::from_bytes([8; 16]),
    );
    assert_eq!(invocation.argv()[0].as_ref(), b"--stable");
    assert_eq!(invocation.working_directory(), b"/work");
    assert_eq!(invocation.sandbox().max_output_bytes(), 2048);
}

#[test]
fn generic_identity_source_contains_no_repository_tool_literals() {
    let source = include_str!("../../src/identity.rs");
    for forbidden in ["tsc-rs", "TypeScript", "Cargo", "H2", "compiler"] {
        assert!(
            !source.contains(forbidden),
            "repository literal: {forbidden}"
        );
    }
    let _ = ReuseScopeV1::NonReusable;
}
