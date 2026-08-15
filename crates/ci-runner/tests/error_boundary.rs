use std::io;

use tsc_ci_runner::{EffectPhase, InfraError, InfraErrorFamily, IoKind, RunCancellation};

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn every_closed_family_keeps_its_effect_phase() {
    let errors = [
        InfraError::Io {
            phase: EffectPhase::Read,
            kind: IoKind::InvalidData,
        },
        InfraError::Transport {
            phase: EffectPhase::Acquire,
        },
        InfraError::Spawn {
            phase: EffectPhase::Spawn,
        },
        InfraError::Signal {
            phase: EffectPhase::Execute,
        },
        InfraError::Timeout {
            phase: EffectPhase::Join,
        },
        InfraError::Cancelled {
            phase: EffectPhase::Execute,
            reason: RunCancellation::DeadlineExpired,
        },
        InfraError::OutOfMemory {
            phase: EffectPhase::Execute,
        },
        InfraError::Panic {
            phase: EffectPhase::Join,
        },
        InfraError::Quota {
            phase: EffectPhase::Read,
        },
        InfraError::Guard {
            phase: EffectPhase::Acquire,
        },
        InfraError::Race {
            phase: EffectPhase::Commit,
        },
        InfraError::Durability {
            phase: EffectPhase::Commit,
        },
    ];

    assert_eq!(errors[0].family(), InfraErrorFamily::Io);
    assert_eq!(errors[0].phase(), EffectPhase::Read);
    assert!(errors[5].is_cancelled());
    assert_eq!(errors[5].phase(), EffectPhase::Execute);
    assert_eq!(errors[11].family(), InfraErrorFamily::Durability);
    assert_send_sync::<InfraError>();
}

#[test]
fn conversion_discards_nonsemantic_io_and_panic_payloads() {
    let io_error = InfraError::from_io(
        EffectPhase::Read,
        io::Error::new(io::ErrorKind::InvalidData, "secret semantic-looking text"),
    );
    assert_eq!(
        io_error,
        InfraError::Io {
            phase: EffectPhase::Read,
            kind: IoKind::InvalidData,
        }
    );
    assert!(!io_error.to_string().contains("secret"));

    let panic = std::panic::catch_unwind(|| panic!("semantic rejection"));
    assert!(panic.is_err());
    let panic_error = InfraError::from_panic(EffectPhase::Execute);
    assert_eq!(panic_error.family(), InfraErrorFamily::Panic);
    assert!(!panic_error.to_string().contains("semantic"));
}

#[test]
fn cancellation_is_explicit_and_never_a_miss_value() {
    let reasons = [
        RunCancellation::UserRequested,
        RunCancellation::ProviderRequested,
        RunCancellation::DeadlineExpired,
    ];
    assert_ne!(reasons[0], reasons[1]);
    assert_ne!(reasons[1], reasons[2]);
    for reason in reasons {
        let error = InfraError::Cancelled {
            phase: EffectPhase::Acquire,
            reason,
        };
        assert!(error.is_cancelled());
        assert_eq!(error.family(), InfraErrorFamily::Cancelled);
    }
}

#[test]
fn public_surface_is_blocking_and_has_no_future_effect_placeholder() {
    let source = include_str!("../src/lib.rs");
    assert!(!source.contains("async"));
    assert!(!source.contains("Future"));
    for forbidden in [
        "RunContext",
        "SourceSnapshotProvider",
        "Sandbox",
        "CasBackend",
        "ExactCacheBackend",
        "Publication",
        "Worker",
    ] {
        assert!(
            !source.contains(forbidden),
            "future placeholder: {forbidden}"
        );
    }
}
