//! True replay executor.
//!
//! Saved evidence is rederived before any launch, artifact process strings
//! are validated as declarative policy only, and both engines are executed
//! once through trusted commands before the saved comparison/class is
//! checked against the fresh observation.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::adapters::oracle;
use crate::evaluate::EvaluatedCase;
use crate::model::{
    CaseExecution, EngineResult, ProducerFailure, ProducerFailureKind, ProducerFailureSource,
    TerminalBoundaryId, TerminalKind, TerminalOutcome, TerminalPhase,
};
use crate::process_session::{
    run_one_case, ProcessSessionLimits, SessionFailure, SessionFailureKind, SessionOutcome,
};
use crate::replay::ReplayArtifact;
use crate::rust_worker::{TSRS_WORKER_IMPLEMENTATION, TSRS_WORKER_VERSION};
use crate::schema::{CaseSpec, ProcessPolicy};
use crate::worker_protocol::{
    ExecuteCaseRequest, ValidatedWorkerResult, WorkerPhase, WorkerRejection,
};
use crate::{FoundationError, FoundationResult};

pub const REPLAY_REQUEST_ID: u64 = 0;
pub const TRUSTED_NODE_DEADLINE_MS: u64 = 30_000;
pub const TRUSTED_NODE_ROLLOVER_CASES: u64 = 500;
pub const TRUSTED_TSRS_WORKER_CAP: u32 = 2;
pub const TRUSTED_TSRS_DEADLINE_MS: u64 = 30_000;
pub const TRUSTED_TSRS_ROLLOVER_CASES: u64 = 500;
pub const TRUSTED_CHILD_POLICY_ID: &str = "bounded-serial-v1";
pub const TRUSTED_CHILD_CASES: u64 = 500;
pub const TSRS_WORKER_ARGUMENT: &str = "__worker";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplayExecution {
    execution: CaseExecution,
    evaluated: EvaluatedCase,
}

impl ReplayExecution {
    pub const fn execution(&self) -> &CaseExecution {
        &self.execution
    }

    pub const fn evaluated(&self) -> &EvaluatedCase {
        &self.evaluated
    }
}

/// Execute true replay using a trusted producer executable as the Rust
/// worker host. The path is runtime configuration supplied by the producer,
/// never a field read from the artifact.
pub fn replay_artifact(
    artifact: &ReplayArtifact,
    trusted_worker_executable: &Path,
) -> FoundationResult<ReplayExecution> {
    artifact.verify_saved()?;
    validate_trusted_process_policy(&artifact.case.process_policy)?;

    let oracle = oracle::execute_oracle_case(REPLAY_REQUEST_ID, &artifact.case)?
        .map_err(|failure| producer_failure("oracle", failure))?;
    let tsrs = execute_tsrs_case(REPLAY_REQUEST_ID, &artifact.case, trusted_worker_executable)
        .map_err(|failure| producer_failure("tsrs", failure))?;
    let execution = CaseExecution::Compared { oracle, tsrs };
    let evaluated = artifact.verify_replayed_execution(&execution)?;
    Ok(ReplayExecution {
        execution,
        evaluated,
    })
}

/// Reject any artifact-controlled launch shape before either engine starts.
pub fn validate_trusted_process_policy(policy: &ProcessPolicy) -> FoundationResult<()> {
    let oracle = oracle::validate_trusted_policy(&policy.oracle_node)?;
    if oracle.deadline() != Duration::from_millis(TRUSTED_NODE_DEADLINE_MS)
        || oracle.rollover_cases() != TRUSTED_NODE_ROLLOVER_CASES
    {
        return Err(FoundationError::new(format!(
            "oracle process policy must use deadline_ms={TRUSTED_NODE_DEADLINE_MS} and rollover_cases={TRUSTED_NODE_ROLLOVER_CASES}"
        )));
    }
    if policy.tsrs.worker_cap != TRUSTED_TSRS_WORKER_CAP
        || policy.tsrs.deadline_ms.get() != TRUSTED_TSRS_DEADLINE_MS
        || policy.tsrs.rollover_cases.get() != TRUSTED_TSRS_ROLLOVER_CASES
    {
        return Err(FoundationError::new(format!(
            "Rust process policy must use worker_cap={TRUSTED_TSRS_WORKER_CAP}, deadline_ms={TRUSTED_TSRS_DEADLINE_MS}, and rollover_cases={TRUSTED_TSRS_ROLLOVER_CASES}"
        )));
    }
    if policy.child.policy_id != TRUSTED_CHILD_POLICY_ID
        || policy.child.cases_per_child.get() != TRUSTED_CHILD_CASES
    {
        return Err(FoundationError::new(format!(
            "child process policy must use policy_id={TRUSTED_CHILD_POLICY_ID:?} and cases_per_child={TRUSTED_CHILD_CASES}"
        )));
    }
    Ok(())
}

fn execute_tsrs_case(
    id: u64,
    case: &CaseSpec,
    trusted_worker_executable: &Path,
) -> Result<EngineResult, ProducerFailure> {
    let request = ExecuteCaseRequest::from_case(id, case)
        .map_err(|error| malformed_tsrs(format!("cannot build Rust worker request: {error}")))?;
    let mut command = trusted_tsrs_command(trusted_worker_executable);
    let limits = ProcessSessionLimits {
        deadline: Duration::from_millis(case.process_policy.tsrs.deadline_ms.get()),
        ..ProcessSessionLimits::default()
    };
    let expected_hello = crate::rust_worker::trusted_hello_line()
        .map_err(|error| malformed_tsrs(format!("cannot build trusted Rust hello: {error}")))?;
    finish_tsrs_session(
        case,
        run_one_case(&mut command, &request, limits, &expected_hello),
    )
}

fn trusted_tsrs_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    command.arg(TSRS_WORKER_ARGUMENT);
    command
}

fn finish_tsrs_session(
    case: &CaseSpec,
    session: Result<SessionOutcome, SessionFailure>,
) -> Result<EngineResult, ProducerFailure> {
    match session {
        Ok(outcome) => {
            if outcome.hello.implementation != TSRS_WORKER_IMPLEMENTATION
                || outcome.hello.version != TSRS_WORKER_VERSION
            {
                return Err(malformed_tsrs(format!(
                    "Rust worker hello mismatch: implementation={:?} version={:?}",
                    outcome.hello.implementation, outcome.hello.version
                )));
            }
            match outcome.result {
                ValidatedWorkerResult::Engine(result) => {
                    result.validate_for_case(case, "tsrs").map_err(|error| {
                        malformed_tsrs(format!(
                            "Rust worker result is invalid for its bound case: {error}"
                        ))
                    })?;
                    Ok(result)
                }
                ValidatedWorkerResult::Rejected(rejection) => Err(rejected_tsrs(rejection)),
            }
        }
        Err(failure) => map_tsrs_session_failure(failure),
    }
}

fn map_tsrs_session_failure(failure: SessionFailure) -> Result<EngineResult, ProducerFailure> {
    let detail = session_failure_detail(&failure);
    match (failure.kind, failure.last_phase) {
        (SessionFailureKind::Deadline, Some(phase)) => Ok(terminal_result(
            phase,
            TerminalKind::Timeout,
            TerminalBoundaryId::Deadline,
            detail,
        )),
        (SessionFailureKind::UnexpectedEof, Some(phase)) => Ok(terminal_result(
            phase,
            TerminalKind::Crash,
            TerminalBoundaryId::ProcessSignal,
            detail,
        )),
        (
            SessionFailureKind::Spawn
            | SessionFailureKind::MissingPipe
            | SessionFailureKind::Thread
            | SessionFailureKind::Write
            | SessionFailureKind::Read,
            _,
        )
        | (SessionFailureKind::Deadline | SessionFailureKind::UnexpectedEof, None) => {
            Err(worker_interruption(detail))
        }
        _ => Err(malformed_tsrs(detail)),
    }
}

fn terminal_result(
    phase: WorkerPhase,
    kind: TerminalKind,
    boundary_id: TerminalBoundaryId,
    detail: impl Into<String>,
) -> EngineResult {
    EngineResult::Terminal {
        outcome: TerminalOutcome {
            phase: TerminalPhase::from(phase),
            kind,
            boundary_id,
            detail: safe_detail(detail.into(), "Rust worker terminated without detail"),
        },
    }
}

fn rejected_tsrs(rejection: WorkerRejection) -> ProducerFailure {
    malformed_tsrs(format!(
        "Rust worker rejected {:?}: {}",
        rejection.kind, rejection.detail
    ))
}

fn malformed_tsrs(detail: impl Into<String>) -> ProducerFailure {
    ProducerFailure {
        source: ProducerFailureSource::TsrsAdapter,
        kind: ProducerFailureKind::MalformedResponse,
        detail: safe_detail(
            detail.into(),
            "Rust adapter received an empty failure detail",
        ),
    }
}

fn worker_interruption(detail: impl Into<String>) -> ProducerFailure {
    ProducerFailure {
        source: ProducerFailureSource::Worker,
        kind: ProducerFailureKind::WorkerInterruption,
        detail: safe_detail(detail.into(), "Rust worker was interrupted before parse"),
    }
}

fn producer_failure(engine: &str, failure: ProducerFailure) -> FoundationError {
    FoundationError::new(format!(
        "{engine} producer failure {:?}/{:?}: {}",
        failure.source, failure.kind, failure.detail
    ))
}

fn session_failure_detail(failure: &SessionFailure) -> String {
    let mut detail = format!("Rust worker session {:?}: {}", failure.kind, failure.detail);
    if let Some(status) = &failure.process_status {
        detail.push_str("; process status: ");
        detail.push_str(status);
    }
    if !failure.stderr.bytes.is_empty() {
        detail.push_str("; stderr: ");
        detail.push_str(&failure.stderr.to_string_lossy());
        if failure.stderr.truncated {
            detail.push_str(" [truncated]");
        }
    }
    if let Some(read_error) = &failure.stderr.read_error {
        detail.push_str("; stderr read error: ");
        detail.push_str(read_error);
    }
    safe_detail(detail, "Rust worker session failed without detail")
}

fn safe_detail(detail: String, fallback: &str) -> String {
    if detail.is_empty() {
        fallback.to_owned()
    } else {
        detail.replace('\0', "\\0")
    }
}

#[cfg(test)]
mod tests {
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

    fn session_failure(
        kind: SessionFailureKind,
        last_phase: Option<WorkerPhase>,
    ) -> SessionFailure {
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
            map_tsrs_session_failure(session_failure(SessionFailureKind::Deadline, None))
                .unwrap_err();
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
}
