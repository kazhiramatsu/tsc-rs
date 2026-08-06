//! Closed JSON-lines protocol shared by the M9 engine workers.
//!
//! A request is bound to one canonical case hash and one controller-owned
//! request id. Every response frame must repeat that binding, phase frames
//! must arrive in the frozen order, and the terminal result is converted into
//! the existing engine model only after its renderer joins are revalidated.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::model::{
    AssembledDiagnostic, CompletedOutcome, EngineResult, RenderSegment, RendererObservation,
    TerminalBoundaryId, TerminalKind, TerminalOutcome, TerminalPhase,
};
use crate::schema::{CanonicalU64, CaseSpec, EncodedFile, OrderedSetting};
use crate::{FoundationError, FoundationResult};

pub const WORKER_WIRE_SCHEMA: u32 = 1;
pub const EXECUTE_CASE_OPERATION: &str = "execute-case";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum WorkerOperation {
    #[serde(rename = "execute-case")]
    ExecuteCase,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireProgram {
    pub cwd: String,
    pub options: Vec<OrderedSetting>,
    pub libs: Vec<EncodedFile>,
    pub files: Vec<EncodedFile>,
}

impl From<&CaseSpec> for WireProgram {
    fn from(case: &CaseSpec) -> Self {
        Self {
            cwd: case.cwd.clone(),
            options: case.options.clone(),
            libs: case.libs.clone(),
            files: case.files.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteCaseRequest {
    schema: u32,
    id: CanonicalU64,
    op: WorkerOperation,
    case_sha256: String,
    program: WireProgram,
}

impl ExecuteCaseRequest {
    pub fn from_case(id: u64, case: &CaseSpec) -> FoundationResult<Self> {
        let request = Self {
            schema: WORKER_WIRE_SCHEMA,
            id: CanonicalU64::new(id),
            op: WorkerOperation::ExecuteCase,
            case_sha256: case.canonical_sha256()?,
            program: WireProgram::from(case),
        };
        request.validate_binding()?;
        Ok(request)
    }

    pub const fn id(&self) -> CanonicalU64 {
        self.id
    }

    pub fn case_sha256(&self) -> &str {
        &self.case_sha256
    }

    pub const fn program(&self) -> &WireProgram {
        &self.program
    }

    pub fn validate_binding(&self) -> FoundationResult<()> {
        if self.schema != WORKER_WIRE_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported worker request schema {}; expected {WORKER_WIRE_SCHEMA}",
                self.schema
            )));
        }
        if !is_lower_sha256(&self.case_sha256) {
            return Err(FoundationError::new(
                "worker request case_sha256 must be exactly 64 lowercase hexadecimal characters",
            ));
        }
        Ok(())
    }

    /// Serialize one compact JSON payload and append its JSONL delimiter.
    ///
    /// `max_payload_bytes` bounds the bytes before the final line feed.
    pub fn canonical_line(&self, max_payload_bytes: usize) -> FoundationResult<Vec<u8>> {
        self.validate_binding()?;
        let mut bytes = serde_json::to_vec(self).map_err(|error| {
            FoundationError::new(format!("cannot serialize worker request: {error}"))
        })?;
        if bytes.len() > max_payload_bytes {
            return Err(FoundationError::new(format!(
                "worker request line is {} bytes; limit is {max_payload_bytes}",
                bytes.len()
            )));
        }
        bytes.push(b'\n');
        Ok(bytes)
    }

    #[cfg(test)]
    fn fixture(id: u64) -> Self {
        Self {
            schema: WORKER_WIRE_SCHEMA,
            id: CanonicalU64::new(id),
            op: WorkerOperation::ExecuteCase,
            case_sha256: "a".repeat(64),
            program: WireProgram {
                cwd: "/work".to_owned(),
                options: Vec::new(),
                libs: Vec::new(),
                files: vec![EncodedFile {
                    ordinal: 0,
                    name: "main.ts".to_owned(),
                    text_base64: String::new(),
                }],
            },
        }
    }
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerPhase {
    Parse,
    Bind,
    Check,
    Format,
}

impl WorkerPhase {
    pub const ORDERED: [Self; 4] = [Self::Parse, Self::Bind, Self::Check, Self::Format];
}

impl From<WorkerPhase> for TerminalPhase {
    fn from(phase: WorkerPhase) -> Self {
        match phase {
            WorkerPhase::Parse => Self::Parse,
            WorkerPhase::Bind => Self::Bind,
            WorkerPhase::Check => Self::Check,
            WorkerPhase::Format => Self::Format,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WireRenderSegment {
    pub assembled_index: u32,
    pub raw_text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerRejectionKind {
    MalformedRequest,
    MalformedObservation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerHello {
    pub implementation: String,
    pub version: String,
}

impl WorkerHello {
    fn validate(&self) -> FoundationResult<()> {
        for (name, value) in [
            ("implementation", self.implementation.as_str()),
            ("version", self.version.as_str()),
        ] {
            if value.is_empty() || value.chars().any(char::is_control) {
                return Err(FoundationError::new(format!(
                    "worker hello {name} must not be empty or contain control characters"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorkerResult {
    Completed {
        assembled: Vec<AssembledDiagnostic>,
        deduped_indices: Vec<u32>,
        segments: Vec<WireRenderSegment>,
        aggregate_text: String,
    },
    Terminal {
        phase: WorkerPhase,
        kind: TerminalKind,
        boundary_id: TerminalBoundaryId,
        detail: String,
    },
    Rejected {
        kind: WorkerRejectionKind,
        detail: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerRejection {
    pub kind: WorkerRejectionKind,
    pub detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidatedWorkerResult {
    Engine(EngineResult),
    Rejected(WorkerRejection),
}

impl WorkerResult {
    fn into_validated(self) -> FoundationResult<ValidatedWorkerResult> {
        match self {
            Self::Completed {
                assembled,
                deduped_indices,
                segments,
                aggregate_text,
            } => {
                if deduped_indices.len() != segments.len() {
                    return Err(FoundationError::new(
                        "worker completed result segments length must equal deduped_indices length",
                    ));
                }

                let mut deduped = Vec::with_capacity(deduped_indices.len());
                let mut rendered = Vec::with_capacity(segments.len());
                let mut joined_text = String::new();
                for (index, (assembled_index, segment)) in
                    deduped_indices.iter().copied().zip(segments).enumerate()
                {
                    if segment.assembled_index != assembled_index {
                        return Err(FoundationError::new(format!(
                            "worker completed result segments[{index}].assembled_index does not match deduped_indices[{index}]"
                        )));
                    }
                    let assembled_index = usize::try_from(assembled_index).map_err(|_| {
                        FoundationError::new(format!(
                            "worker completed result deduped_indices[{index}] does not fit usize"
                        ))
                    })?;
                    let diagnostic = assembled.get(assembled_index).ok_or_else(|| {
                        FoundationError::new(format!(
                            "worker completed result deduped_indices[{index}] is outside assembled"
                        ))
                    })?;
                    deduped.push(diagnostic.clone());
                    joined_text.push_str(&segment.raw_text);
                    rendered.push(RenderSegment {
                        diagnostic: diagnostic.clone(),
                        raw_text: segment.raw_text,
                    });
                }
                if joined_text != aggregate_text {
                    return Err(FoundationError::new(
                        "worker completed result aggregate_text must equal the exact concatenation of segment raw_text boundaries",
                    ));
                }

                let diagnostics = assembled
                    .iter()
                    .map(|entry| entry.diagnostic.clone())
                    .collect();
                let outcome = CompletedOutcome {
                    diagnostics,
                    renderer: RendererObservation {
                        assembled,
                        deduped,
                        segments: rendered,
                        aggregate_text,
                    },
                };
                outcome.validate("worker completed result")?;
                Ok(ValidatedWorkerResult::Engine(EngineResult::Completed {
                    outcome,
                }))
            }
            Self::Terminal {
                phase,
                kind,
                boundary_id,
                detail,
            } => {
                let result = EngineResult::Terminal {
                    outcome: TerminalOutcome {
                        phase: phase.into(),
                        kind,
                        boundary_id,
                        detail,
                    },
                };
                result.validate("worker terminal result")?;
                Ok(ValidatedWorkerResult::Engine(result))
            }
            Self::Rejected { kind, detail } => {
                if detail.is_empty() || detail.contains('\0') {
                    return Err(FoundationError::new(
                        "worker rejected result detail must not be empty or contain NUL",
                    ));
                }
                Ok(ValidatedWorkerResult::Rejected(WorkerRejection {
                    kind,
                    detail,
                }))
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "frame", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorkerFrame {
    Hello {
        schema: u32,
        implementation: String,
        version: String,
    },
    Phase {
        schema: u32,
        id: CanonicalU64,
        case_sha256: String,
        phase: WorkerPhase,
    },
    Result {
        schema: u32,
        id: CanonicalU64,
        case_sha256: String,
        result: WorkerResult,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolViolationKind {
    MalformedFrame,
    Schema,
    Handshake,
    Binding,
    PhaseOrder,
    ResultShape,
    FrameAfterResult,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtocolViolation {
    kind: ProtocolViolationKind,
    detail: String,
}

impl ProtocolViolation {
    fn new(kind: ProtocolViolationKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> ProtocolViolationKind {
        self.kind
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ProtocolViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for ProtocolViolation {}

#[derive(Clone, Debug)]
pub struct ResponseValidator {
    expected_id: CanonicalU64,
    expected_case_sha256: String,
    hello: Option<WorkerHello>,
    phases: Vec<WorkerPhase>,
    finished: bool,
}

impl ResponseValidator {
    pub fn new(request: &ExecuteCaseRequest) -> Self {
        Self {
            expected_id: request.id,
            expected_case_sha256: request.case_sha256.clone(),
            hello: None,
            phases: Vec::with_capacity(WorkerPhase::ORDERED.len()),
            finished: false,
        }
    }

    pub fn phases(&self) -> &[WorkerPhase] {
        &self.phases
    }

    pub const fn hello(&self) -> Option<&WorkerHello> {
        self.hello.as_ref()
    }

    pub fn last_phase(&self) -> Option<WorkerPhase> {
        self.phases.last().copied()
    }

    pub fn accept_line(
        &mut self,
        line: &[u8],
    ) -> Result<Option<ValidatedWorkerResult>, ProtocolViolation> {
        let frame: WorkerFrame = serde_json::from_slice(line).map_err(|error| {
            ProtocolViolation::new(
                ProtocolViolationKind::MalformedFrame,
                format!("invalid worker response frame JSON: {error}"),
            )
        })?;
        self.accept_frame(frame)
    }

    pub fn accept_frame(
        &mut self,
        frame: WorkerFrame,
    ) -> Result<Option<ValidatedWorkerResult>, ProtocolViolation> {
        if self.finished {
            return Err(ProtocolViolation::new(
                ProtocolViolationKind::FrameAfterResult,
                "worker emitted a frame after its result",
            ));
        }

        match frame {
            WorkerFrame::Hello {
                schema,
                implementation,
                version,
            } => {
                self.validate_schema(schema)?;
                if self.hello.is_some() || !self.phases.is_empty() {
                    return Err(ProtocolViolation::new(
                        ProtocolViolationKind::Handshake,
                        "worker hello must be emitted exactly once before every phase",
                    ));
                }
                let hello = WorkerHello {
                    implementation,
                    version,
                };
                hello.validate().map_err(|error| {
                    ProtocolViolation::new(ProtocolViolationKind::Handshake, error.to_string())
                })?;
                self.hello = Some(hello);
                Ok(None)
            }
            WorkerFrame::Phase {
                schema,
                id,
                case_sha256,
                phase,
            } => {
                self.require_hello()?;
                self.validate_envelope(schema, id, &case_sha256)?;
                let expected = WorkerPhase::ORDERED.get(self.phases.len()).copied();
                if expected != Some(phase) {
                    return Err(ProtocolViolation::new(
                        ProtocolViolationKind::PhaseOrder,
                        format!(
                            "worker phase order mismatch: expected {expected:?}, found {phase:?}"
                        ),
                    ));
                }
                self.phases.push(phase);
                Ok(None)
            }
            WorkerFrame::Result {
                schema,
                id,
                case_sha256,
                result,
            } => {
                self.require_hello()?;
                self.validate_envelope(schema, id, &case_sha256)?;
                self.validate_result_phase(&result)?;
                let result = result.into_validated().map_err(|error| {
                    ProtocolViolation::new(ProtocolViolationKind::ResultShape, error.to_string())
                })?;
                self.finished = true;
                Ok(Some(result))
            }
        }
    }

    fn validate_envelope(
        &self,
        schema: u32,
        id: CanonicalU64,
        case_sha256: &str,
    ) -> Result<(), ProtocolViolation> {
        self.validate_schema(schema)?;
        if id != self.expected_id || case_sha256 != self.expected_case_sha256 {
            return Err(ProtocolViolation::new(
                ProtocolViolationKind::Binding,
                "worker response id/case_sha256 binding does not match the request",
            ));
        }
        Ok(())
    }

    fn validate_schema(&self, schema: u32) -> Result<(), ProtocolViolation> {
        if schema != WORKER_WIRE_SCHEMA {
            return Err(ProtocolViolation::new(
                ProtocolViolationKind::Schema,
                format!(
                    "unsupported worker response schema {schema}; expected {WORKER_WIRE_SCHEMA}"
                ),
            ));
        }
        Ok(())
    }

    fn require_hello(&self) -> Result<(), ProtocolViolation> {
        if self.hello.is_none() {
            return Err(ProtocolViolation::new(
                ProtocolViolationKind::Handshake,
                "worker must emit hello before every phase or result",
            ));
        }
        Ok(())
    }

    fn validate_result_phase(&self, result: &WorkerResult) -> Result<(), ProtocolViolation> {
        match result {
            WorkerResult::Completed { .. } => {
                if self.phases.as_slice() != WorkerPhase::ORDERED {
                    return Err(ProtocolViolation::new(
                        ProtocolViolationKind::PhaseOrder,
                        "worker completed result requires parse, bind, check, format phases",
                    ));
                }
            }
            WorkerResult::Terminal { phase, .. } => {
                if self.last_phase() != Some(*phase) {
                    return Err(ProtocolViolation::new(
                        ProtocolViolationKind::PhaseOrder,
                        "worker terminal result phase must equal the last emitted phase",
                    ));
                }
            }
            WorkerResult::Rejected { kind, .. } => {
                let valid = match kind {
                    WorkerRejectionKind::MalformedRequest => self.phases.is_empty(),
                    WorkerRejectionKind::MalformedObservation => matches!(
                        self.last_phase(),
                        Some(WorkerPhase::Check | WorkerPhase::Format)
                    ),
                };
                if !valid {
                    return Err(ProtocolViolation::new(
                        ProtocolViolationKind::PhaseOrder,
                        "worker rejected result is not valid at the current phase boundary",
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/worker_protocol/tests.rs"]
mod tests;
