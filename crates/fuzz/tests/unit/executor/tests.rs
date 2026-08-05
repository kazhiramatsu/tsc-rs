use super::*;
use crate::process_session::CapturedStderr;
use crate::schema::{
    CanonicalU64, ChildProcessPolicy, NodeProcessPolicy, OrderedArgument, RustProcessPolicy,
};

fn process_policy() -> ProcessPolicy {
    ProcessPolicy {
        schema: 1,
        oracle_node: NodeProcessPolicy {
            executable_id: oracle::TRUSTED_NODE_EXECUTABLE_ID.to_owned(),
            arguments: vec![OrderedArgument {
                ordinal: 0,
                value: oracle::TRUSTED_NODE_ARGUMENT.to_owned(),
            }],
            single_threaded: true,
            deadline_ms: CanonicalU64::new(TRUSTED_NODE_DEADLINE_MS),
            rollover_cases: CanonicalU64::new(TRUSTED_NODE_ROLLOVER_CASES),
        },
        tsrs: RustProcessPolicy {
            worker_cap: TRUSTED_TSRS_WORKER_CAP,
            deadline_ms: CanonicalU64::new(TRUSTED_TSRS_DEADLINE_MS),
            rollover_cases: CanonicalU64::new(TRUSTED_TSRS_ROLLOVER_CASES),
        },
        child: ChildProcessPolicy {
            policy_id: TRUSTED_CHILD_POLICY_ID.to_owned(),
            cases_per_child: CanonicalU64::new(TRUSTED_CHILD_CASES),
        },
    }
}

#[test]
fn trusted_policy_is_exact_and_rejects_artifact_executable() {
    validate_trusted_process_policy(&process_policy()).unwrap();

    let mut malicious = process_policy();
    malicious.oracle_node.executable_id = "/tmp/artifact-owned-worker".to_owned();
    assert!(validate_trusted_process_policy(&malicious).is_err());

    let mut widened = process_policy();
    widened.tsrs.worker_cap += 1;
    assert!(validate_trusted_process_policy(&widened).is_err());

    let mut child_drift = process_policy();
    child_drift.child.cases_per_child = CanonicalU64::new(TRUSTED_CHILD_CASES + 1);
    assert!(validate_trusted_process_policy(&child_drift).is_err());
}

fn session_failure(kind: SessionFailureKind, last_phase: Option<WorkerPhase>) -> SessionFailure {
    SessionFailure {
        kind,
        detail: "canary".to_owned(),
        last_phase,
        stderr: CapturedStderr::default(),
        process_status: None,
    }
}

#[test]
fn rust_stop_is_terminal_only_after_a_phase_boundary() {
    let before_parse =
        map_tsrs_session_failure(session_failure(SessionFailureKind::Deadline, None)).unwrap_err();
    assert_eq!(before_parse.source, ProducerFailureSource::Worker);
    assert_eq!(before_parse.kind, ProducerFailureKind::WorkerInterruption);

    let after_check = map_tsrs_session_failure(session_failure(
        SessionFailureKind::Deadline,
        Some(WorkerPhase::Check),
    ))
    .unwrap();
    let EngineResult::Terminal { outcome } = after_check else {
        panic!("expected terminal");
    };
    assert_eq!(outcome.phase, TerminalPhase::Check);
    assert_eq!(outcome.kind, TerminalKind::Timeout);
    assert_eq!(outcome.boundary_id, TerminalBoundaryId::Deadline);

    let read_interruption = map_tsrs_session_failure(session_failure(
        SessionFailureKind::Read,
        Some(WorkerPhase::Check),
    ))
    .unwrap_err();
    assert_eq!(read_interruption.source, ProducerFailureSource::Worker);
    assert_eq!(
        read_interruption.kind,
        ProducerFailureKind::WorkerInterruption
    );

    let malformed = map_tsrs_session_failure(session_failure(
        SessionFailureKind::ResponseLineTooLong,
        None,
    ))
    .unwrap_err();
    assert_eq!(malformed.source, ProducerFailureSource::TsrsAdapter);
    assert_eq!(malformed.kind, ProducerFailureKind::MalformedResponse);
}
