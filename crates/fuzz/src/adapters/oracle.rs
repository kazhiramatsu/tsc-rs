//! Trusted M9 Node-oracle projection and response validation.
//!
//! Artifact process policy is declarative identity only. This adapter
//! validates it against one closed launch shape, then constructs the command
//! from trusted constants rather than executing artifact-provided strings.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::model::{
    EngineResult, ProducerFailure, ProducerFailureKind, ProducerFailureSource, TerminalBoundaryId,
    TerminalKind, TerminalOutcome,
};
use crate::process_session::{
    run_one_case, ProcessSessionLimits, SessionFailure, SessionFailureKind, SessionOutcome,
};
use crate::schema::{CaseSpec, NodeProcessPolicy};
use crate::worker_protocol::{
    ExecuteCaseRequest, ProtocolViolation, ResponseValidator, ValidatedWorkerResult, WorkerHello,
    WorkerPhase, WorkerRejection,
};
use crate::{FoundationError, FoundationResult};

pub const TRUSTED_NODE_EXECUTABLE_ID: &str = "node-pinned";
pub const TRUSTED_NODE_PROGRAM: &str = "node";
pub const TRUSTED_NODE_ARGUMENT: &str = "--single-threaded";
pub const TRUSTED_NODE_VERSION: &str = "25.2.1";
pub const ORACLE_IMPLEMENTATION_ID: &str = "oracle-node";

const NODE_VERSION_FILE: &str = include_str!("../../../../.node-version");

pub type OracleExecutionResult = Result<EngineResult, ProducerFailure>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedOraclePolicy {
    deadline: Duration,
    rollover_cases: u64,
}

impl ValidatedOraclePolicy {
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    pub const fn rollover_cases(self) -> u64 {
        self.rollover_cases
    }
}

/// Tighten the general CaseSpec process-policy validation to the one trusted
/// M9 oracle launch shape.
pub fn validate_trusted_policy(
    policy: &NodeProcessPolicy,
) -> FoundationResult<ValidatedOraclePolicy> {
    if policy.executable_id != TRUSTED_NODE_EXECUTABLE_ID {
        return Err(FoundationError::new(format!(
            "oracle executable_id must be {TRUSTED_NODE_EXECUTABLE_ID:?}"
        )));
    }
    if !policy.single_threaded
        || policy.arguments.len() != 1
        || policy.arguments[0].ordinal != 0
        || policy.arguments[0].value != TRUSTED_NODE_ARGUMENT
    {
        return Err(FoundationError::new(
            "oracle policy must contain exactly ordinal-0 --single-threaded and no other argument",
        ));
    }
    if policy.deadline_ms.get() == 0 || policy.rollover_cases.get() == 0 {
        return Err(FoundationError::new(
            "oracle deadline and rollover_cases must be positive",
        ));
    }
    Ok(ValidatedOraclePolicy {
        deadline: Duration::from_millis(policy.deadline_ms.get()),
        rollover_cases: policy.rollover_cases.get(),
    })
}

pub fn oracle_driver_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../oracle/m9-driver.mjs")
}

/// Construct the trusted command. No CaseSpec executable or argument string
/// reaches `Command`.
pub fn trusted_node_command() -> Command {
    let mut command = Command::new(TRUSTED_NODE_PROGRAM);
    command.arg(TRUSTED_NODE_ARGUMENT).arg(oracle_driver_path());
    command
}

pub fn execute_request(id: u64, case: &CaseSpec) -> FoundationResult<ExecuteCaseRequest> {
    case.validate()?;
    validate_trusted_policy(&case.process_policy.oracle_node)?;
    ExecuteCaseRequest::from_case(id, case)
}

/// Execute one case in one fresh trusted Node process.
///
/// The artifact policy is validated but never executed. The process session
/// owns one absolute deadline and never retries or resends the request.
pub fn execute_oracle_case(id: u64, case: &CaseSpec) -> FoundationResult<OracleExecutionResult> {
    let request = execute_request(id, case)?;
    let policy = validate_trusted_policy(&case.process_policy.oracle_node)?;
    if !pinned_node_version_file_is_exact() {
        return Err(FoundationError::new(format!(
            ".node-version must contain exactly {TRUSTED_NODE_VERSION:?}"
        )));
    }

    let mut command = trusted_node_command();
    let limits = ProcessSessionLimits {
        deadline: policy.deadline(),
        ..ProcessSessionLimits::default()
    };
    let expected_hello = trusted_hello_line()?;
    Ok(finish_session(
        case,
        run_one_case(&mut command, &request, limits, &expected_hello),
    ))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum HelloFrameKind {
    #[serde(rename = "hello")]
    Hello,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct OracleHello {
    schema: u32,
    frame: HelloFrameKind,
    implementation: String,
    version: String,
}

fn trusted_hello_line() -> FoundationResult<Vec<u8>> {
    serde_json::to_vec(&OracleHello {
        schema: 1,
        frame: HelloFrameKind::Hello,
        implementation: ORACLE_IMPLEMENTATION_ID.to_owned(),
        version: format!("v{TRUSTED_NODE_VERSION}"),
    })
    .map_err(|error| FoundationError::new(format!("cannot encode trusted oracle hello: {error}")))
}

/// Validate the first line emitted by the launched process.
///
/// The caller supplies bytes without the JSONL delimiter. Exact re-encoding
/// prevents whitespace, key-order, duplicate-key, and alternate-escape forms
/// from becoming a second hello representation.
pub fn validate_hello_line(line: &[u8]) -> Result<(), ProducerFailure> {
    let hello: OracleHello = serde_json::from_slice(line)
        .map_err(|error| malformed(format!("invalid oracle hello JSON: {error}")))?;
    let canonical = serde_json::to_vec(&hello)
        .map_err(|error| malformed(format!("cannot reserialize oracle hello: {error}")))?;
    if canonical != line {
        return Err(malformed(
            "oracle hello must use canonical compact schema-1 JSON bytes",
        ));
    }
    if hello.schema != 1 {
        return Err(malformed(format!(
            "oracle hello schema mismatch: expected 1, found {}",
            hello.schema
        )));
    }
    validate_worker_hello(&WorkerHello {
        implementation: hello.implementation,
        version: hello.version,
    })
}

pub fn validate_worker_hello(hello: &WorkerHello) -> Result<(), ProducerFailure> {
    if hello.implementation != ORACLE_IMPLEMENTATION_ID
        || hello.version != format!("v{TRUSTED_NODE_VERSION}")
    {
        return Err(malformed(format!(
            "oracle hello mismatch: implementation={:?} version={:?}",
            hello.implementation, hello.version
        )));
    }
    Ok(())
}

pub struct OracleResponseDecoder<'case> {
    case: &'case CaseSpec,
    validator: ResponseValidator,
}

impl<'case> OracleResponseDecoder<'case> {
    pub fn new(case: &'case CaseSpec, request: &ExecuteCaseRequest) -> Self {
        Self {
            case,
            validator: ResponseValidator::new(request),
        }
    }

    pub fn last_phase(&self) -> Option<WorkerPhase> {
        self.validator.last_phase()
    }

    pub fn accept_line(&mut self, line: &[u8]) -> Result<Option<EngineResult>, ProducerFailure> {
        let could_be_hello = self.validator.hello().is_none();
        let Some(result) = self.validator.accept_line(line).map_err(protocol_failure)? else {
            if could_be_hello && self.validator.hello().is_some() {
                validate_hello_line(line)?;
            }
            return Ok(None);
        };
        match result {
            ValidatedWorkerResult::Engine(result) => {
                result
                    .validate_for_case(self.case, "oracle")
                    .map_err(|error| {
                        malformed(format!(
                            "oracle result is invalid for its bound case: {error}"
                        ))
                    })?;
                Ok(Some(result))
            }
            ValidatedWorkerResult::Rejected(rejection) => Err(rejection_failure(rejection)),
        }
    }

    pub fn deadline_result(&self) -> OracleExecutionResult {
        match self.last_phase() {
            Some(phase) => Ok(terminal_result(
                phase,
                TerminalKind::Timeout,
                TerminalBoundaryId::Deadline,
                "oracle Node deadline expired",
            )),
            None => Err(worker_interruption(
                "oracle Node deadline expired before the first phase",
            )),
        }
    }

    pub fn process_exit_result(&self, detail: impl Into<String>) -> OracleExecutionResult {
        let detail = detail.into();
        match self.last_phase() {
            Some(phase) => Ok(terminal_result(
                phase,
                TerminalKind::Crash,
                TerminalBoundaryId::ProcessSignal,
                detail,
            )),
            None => Err(worker_interruption(detail)),
        }
    }
}

fn finish_session(
    case: &CaseSpec,
    session: Result<SessionOutcome, SessionFailure>,
) -> OracleExecutionResult {
    match session {
        Ok(outcome) => {
            validate_worker_hello(&outcome.hello)?;
            match outcome.result {
                ValidatedWorkerResult::Engine(result) => {
                    result.validate_for_case(case, "oracle").map_err(|error| {
                        malformed(format!(
                            "oracle result is invalid for its bound case: {error}"
                        ))
                    })?;
                    Ok(result)
                }
                ValidatedWorkerResult::Rejected(rejection) => Err(rejection_failure(rejection)),
            }
        }
        Err(failure) => map_session_failure(failure),
    }
}

fn map_session_failure(failure: SessionFailure) -> OracleExecutionResult {
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
        (SessionFailureKind::Deadline | SessionFailureKind::UnexpectedEof, None)
        | (
            SessionFailureKind::Spawn
            | SessionFailureKind::MissingPipe
            | SessionFailureKind::Thread
            | SessionFailureKind::Write
            | SessionFailureKind::Read,
            _,
        ) => Err(worker_interruption(detail)),
        _ => Err(malformed(detail)),
    }
}

fn session_failure_detail(failure: &SessionFailure) -> String {
    let mut detail = format!("oracle session {:?}: {}", failure.kind, failure.detail);
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
    detail
}

fn protocol_failure(error: ProtocolViolation) -> ProducerFailure {
    malformed(format!("oracle response protocol violation: {error}"))
}

fn rejection_failure(rejection: WorkerRejection) -> ProducerFailure {
    malformed(format!(
        "oracle driver rejected {:?}: {}",
        rejection.kind, rejection.detail
    ))
}

fn malformed(detail: impl Into<String>) -> ProducerFailure {
    let detail = detail.into();
    ProducerFailure {
        source: ProducerFailureSource::OracleAdapter,
        kind: ProducerFailureKind::MalformedResponse,
        detail: if detail.is_empty() {
            "oracle adapter received an empty failure detail".to_owned()
        } else {
            detail.replace('\0', "\\0")
        },
    }
}

fn worker_interruption(detail: impl Into<String>) -> ProducerFailure {
    let detail = detail.into();
    ProducerFailure {
        source: ProducerFailureSource::Worker,
        kind: ProducerFailureKind::WorkerInterruption,
        detail: if detail.is_empty() {
            "oracle worker was interrupted without detail".to_owned()
        } else {
            detail.replace('\0', "\\0")
        },
    }
}

fn terminal_result(
    phase: WorkerPhase,
    kind: TerminalKind,
    boundary_id: TerminalBoundaryId,
    detail: impl Into<String>,
) -> EngineResult {
    let detail = detail.into();
    EngineResult::Terminal {
        outcome: TerminalOutcome {
            phase: phase.into(),
            kind,
            boundary_id,
            detail: if detail.is_empty() {
                "oracle process exited without detail".to_owned()
            } else {
                detail.replace('\0', "\\0")
            },
        },
    }
}

pub fn pinned_node_version_file_is_exact() -> bool {
    NODE_VERSION_FILE == format!("{TRUSTED_NODE_VERSION}\n")
        || NODE_VERSION_FILE == TRUSTED_NODE_VERSION
}

#[cfg(test)]
#[path = "../../tests/unit/adapters/oracle/tests.rs"]
mod tests;
