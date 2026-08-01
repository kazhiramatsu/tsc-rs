//! One-case child-process session with bounded JSONL transport.
//!
//! The controller owns the absolute deadline. Blocking stdin/stdout/stderr
//! work runs on dedicated threads, every retained buffer is bounded, and all
//! paths after a successful spawn terminate and wait for the child exactly
//! once. A failed case is returned to the caller without an implicit retry.

use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, SyncSender};
use std::thread;
use std::time::{Duration, Instant};

use crate::worker_protocol::{
    ExecuteCaseRequest, ProtocolViolationKind, ResponseValidator, ValidatedWorkerResult,
    WorkerHello, WorkerPhase,
};
use crate::{FoundationError, FoundationResult};

const EVENT_QUEUE_CAPACITY: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessSessionLimits {
    /// Maximum compact request JSON bytes before its final line feed.
    pub max_request_line_bytes: usize,
    /// Maximum response bytes before each line feed.
    pub max_response_line_bytes: usize,
    /// Maximum stderr bytes retained. Excess bytes are still drained.
    pub max_stderr_bytes: usize,
    /// One absolute budget covering spawn, transport, and response validation.
    pub deadline: Duration,
}

impl Default for ProcessSessionLimits {
    fn default() -> Self {
        Self {
            max_request_line_bytes: 1024 * 1024,
            max_response_line_bytes: 16 * 1024 * 1024,
            max_stderr_bytes: 256 * 1024,
            deadline: Duration::from_secs(30),
        }
    }
}

impl ProcessSessionLimits {
    pub fn validate(self) -> FoundationResult<Self> {
        if self.max_request_line_bytes == 0 {
            return Err(FoundationError::new(
                "process session request line limit must be positive",
            ));
        }
        if self.max_response_line_bytes == 0 {
            return Err(FoundationError::new(
                "process session response line limit must be positive",
            ));
        }
        if self.deadline.is_zero() {
            return Err(FoundationError::new(
                "process session deadline must be positive",
            ));
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapturedStderr {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub read_error: Option<String>,
}

impl CapturedStderr {
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionFailureKind {
    InvalidLimits,
    InvalidRequest,
    Spawn,
    MissingPipe,
    Thread,
    Write,
    Read,
    ResponseLineTooLong,
    UnterminatedResponseLine,
    MalformedFrame,
    SchemaMismatch,
    Handshake,
    BindingMismatch,
    PhaseOrder,
    ResultShape,
    FrameAfterResult,
    UnexpectedEof,
    Deadline,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionFailure {
    pub kind: SessionFailureKind,
    pub detail: String,
    pub last_phase: Option<WorkerPhase>,
    pub stderr: CapturedStderr,
    pub process_status: Option<String>,
}

impl SessionFailure {
    fn bare(kind: SessionFailureKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
            last_phase: None,
            stderr: CapturedStderr::default(),
            process_status: None,
        }
    }
}

impl fmt::Display for SessionFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for SessionFailure {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionOutcome {
    pub hello: WorkerHello,
    pub result: ValidatedWorkerResult,
    pub phases: Vec<WorkerPhase>,
    pub stderr: CapturedStderr,
    pub process_status: Option<String>,
}

enum SessionEvent {
    Line(Vec<u8>),
    Eof,
    UnterminatedLine { bytes: usize },
    LineTooLong { limit: usize },
    StdoutError(String),
    WriterFinished(Result<(), String>),
}

enum SessionDecision {
    Success {
        hello: WorkerHello,
        result: ValidatedWorkerResult,
        phases: Vec<WorkerPhase>,
    },
    Failure(SessionFailure),
}

enum BoundedLine {
    Line(Vec<u8>),
    Eof,
    Unterminated { bytes: usize },
    TooLong,
}

/// Execute exactly one request in a fresh, caller-configured child command.
///
/// The command is spawned once. No failure path retries or recursively
/// launches another worker.
pub fn run_one_case(
    command: &mut Command,
    request: &ExecuteCaseRequest,
    limits: ProcessSessionLimits,
    expected_hello_line: &[u8],
) -> Result<SessionOutcome, SessionFailure> {
    let limits = limits.validate().map_err(|error| {
        SessionFailure::bare(SessionFailureKind::InvalidLimits, error.to_string())
    })?;
    if expected_hello_line.is_empty()
        || expected_hello_line.len() > limits.max_response_line_bytes
        || expected_hello_line
            .iter()
            .any(|byte| matches!(byte, b'\r' | b'\n'))
    {
        return Err(SessionFailure::bare(
            SessionFailureKind::InvalidLimits,
            "expected worker hello must be one non-empty bounded JSONL payload",
        ));
    }
    let request_line = request
        .canonical_line(limits.max_request_line_bytes)
        .map_err(|error| {
            SessionFailure::bare(SessionFailureKind::InvalidRequest, error.to_string())
        })?;

    let started = Instant::now();
    let deadline = started.checked_add(limits.deadline).ok_or_else(|| {
        SessionFailure::bare(
            SessionFailureKind::InvalidLimits,
            "process session deadline overflows Instant",
        )
    })?;

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            SessionFailure::bare(
                SessionFailureKind::Spawn,
                format!("cannot spawn worker process: {error}"),
            )
        })?;

    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => {
            let process_status = terminate_and_wait(&mut child);
            let mut failure = SessionFailure::bare(
                SessionFailureKind::MissingPipe,
                "spawned worker has no piped stdin",
            );
            failure.process_status = process_status;
            return Err(failure);
        }
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            drop(stdin);
            let process_status = terminate_and_wait(&mut child);
            let mut failure = SessionFailure::bare(
                SessionFailureKind::MissingPipe,
                "spawned worker has no piped stdout",
            );
            failure.process_status = process_status;
            return Err(failure);
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            drop(stdin);
            drop(stdout);
            let process_status = terminate_and_wait(&mut child);
            let mut failure = SessionFailure::bare(
                SessionFailureKind::MissingPipe,
                "spawned worker has no piped stderr",
            );
            failure.process_status = process_status;
            return Err(failure);
        }
    };

    let (event_sender, event_receiver) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
    let stdout_sender = event_sender.clone();
    let stdout_handle = match thread::Builder::new()
        .name("tsc-rs-worker-stdout".to_owned())
        .spawn(move || read_stdout(stdout, limits.max_response_line_bytes, stdout_sender))
    {
        Ok(handle) => handle,
        Err(error) => {
            drop(stdin);
            drop(stderr);
            drop(event_receiver);
            drop(event_sender);
            let process_status = terminate_and_wait(&mut child);
            let mut failure = SessionFailure::bare(
                SessionFailureKind::Thread,
                format!("cannot spawn worker stdout reader thread: {error}"),
            );
            failure.process_status = process_status;
            return Err(failure);
        }
    };

    let stderr_handle = match thread::Builder::new()
        .name("tsc-rs-worker-stderr".to_owned())
        .spawn(move || drain_stderr(stderr, limits.max_stderr_bytes))
    {
        Ok(handle) => handle,
        Err(error) => {
            drop(stdin);
            drop(event_receiver);
            drop(event_sender);
            let process_status = terminate_and_wait(&mut child);
            let _ = stdout_handle.join();
            let mut failure = SessionFailure::bare(
                SessionFailureKind::Thread,
                format!("cannot spawn worker stderr reader thread: {error}"),
            );
            failure.process_status = process_status;
            return Err(failure);
        }
    };

    let writer_sender = event_sender.clone();
    let writer_handle = match thread::Builder::new()
        .name("tsc-rs-worker-stdin".to_owned())
        .spawn(move || write_request(stdin, request_line, writer_sender))
    {
        Ok(handle) => handle,
        Err(error) => {
            drop(event_receiver);
            drop(event_sender);
            let process_status = terminate_and_wait(&mut child);
            let _ = stdout_handle.join();
            let stderr = stderr_handle.join().unwrap_or_default();
            let mut failure = SessionFailure::bare(
                SessionFailureKind::Thread,
                format!("cannot spawn worker stdin writer thread: {error}"),
            );
            failure.stderr = stderr;
            failure.process_status = process_status;
            return Err(failure);
        }
    };
    drop(event_sender);

    let mut validator = ResponseValidator::new(request);
    let mut completed_result = None;
    let decision = loop {
        let now = Instant::now();
        if now >= deadline {
            let (kind, detail) = if completed_result.is_some() {
                (
                    SessionFailureKind::FrameAfterResult,
                    "worker did not close stdout after its result frame".to_owned(),
                )
            } else {
                (
                    SessionFailureKind::Deadline,
                    format!(
                        "worker did not produce a valid result within {} ms",
                        limits.deadline.as_millis()
                    ),
                )
            };
            break SessionDecision::Failure(SessionFailure {
                kind,
                detail,
                last_phase: validator.last_phase(),
                stderr: CapturedStderr::default(),
                process_status: None,
            });
        }

        let remaining = deadline.saturating_duration_since(now);
        match event_receiver.recv_timeout(remaining) {
            Ok(SessionEvent::Line(line)) => {
                if validator.hello().is_none() && line != expected_hello_line {
                    break SessionDecision::Failure(SessionFailure {
                        kind: SessionFailureKind::Handshake,
                        detail: "worker hello bytes do not match the trusted canonical hello"
                            .to_owned(),
                        last_phase: None,
                        stderr: CapturedStderr::default(),
                        process_status: None,
                    });
                }
                match validator.accept_line(&line) {
                    Ok(Some(result)) => {
                        completed_result = Some(result);
                    }
                    Ok(None) => {}
                    Err(error) => {
                        break SessionDecision::Failure(SessionFailure {
                            kind: session_kind_for_protocol(error.kind()),
                            detail: error.detail().to_owned(),
                            last_phase: validator.last_phase(),
                            stderr: CapturedStderr::default(),
                            process_status: None,
                        });
                    }
                }
            }
            Ok(SessionEvent::Eof) => {
                if let Some(result) = completed_result.take() {
                    break SessionDecision::Success {
                        hello: validator
                            .hello()
                            .cloned()
                            .expect("validated result requires worker hello"),
                        result,
                        phases: validator.phases().to_vec(),
                    };
                }
                break SessionDecision::Failure(SessionFailure {
                    kind: SessionFailureKind::UnexpectedEof,
                    detail: "worker stdout reached EOF before a result frame".to_owned(),
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
            Ok(SessionEvent::UnterminatedLine { bytes }) => {
                break SessionDecision::Failure(SessionFailure {
                    kind: SessionFailureKind::UnterminatedResponseLine,
                    detail: format!(
                        "worker stdout ended with an unterminated {bytes}-byte response line"
                    ),
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
            Ok(SessionEvent::LineTooLong { limit }) => {
                break SessionDecision::Failure(SessionFailure {
                    kind: SessionFailureKind::ResponseLineTooLong,
                    detail: format!("worker response line exceeds {limit} bytes"),
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
            Ok(SessionEvent::StdoutError(detail)) => {
                break SessionDecision::Failure(SessionFailure {
                    kind: SessionFailureKind::Read,
                    detail,
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
            Ok(SessionEvent::WriterFinished(Ok(()))) => {}
            Ok(SessionEvent::WriterFinished(Err(detail))) => {
                break SessionDecision::Failure(SessionFailure {
                    kind: SessionFailureKind::Write,
                    detail,
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let (kind, detail) = if completed_result.is_some() {
                    (
                        SessionFailureKind::FrameAfterResult,
                        "worker did not close stdout after its result frame".to_owned(),
                    )
                } else {
                    (
                        SessionFailureKind::Deadline,
                        format!(
                            "worker did not produce a valid result within {} ms",
                            limits.deadline.as_millis()
                        ),
                    )
                };
                break SessionDecision::Failure(SessionFailure {
                    kind,
                    detail,
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(result) = completed_result.take() {
                    break SessionDecision::Success {
                        hello: validator
                            .hello()
                            .cloned()
                            .expect("validated result requires worker hello"),
                        result,
                        phases: validator.phases().to_vec(),
                    };
                }
                break SessionDecision::Failure(SessionFailure {
                    kind: SessionFailureKind::UnexpectedEof,
                    detail: "worker transport threads stopped before a result frame".to_owned(),
                    last_phase: validator.last_phase(),
                    stderr: CapturedStderr::default(),
                    process_status: None,
                });
            }
        }
    };

    drop(event_receiver);
    let process_status = terminate_and_wait(&mut child);
    let stdout_joined = stdout_handle.join().is_ok();
    let writer_joined = writer_handle.join().is_ok();
    let stderr = match stderr_handle.join() {
        Ok(stderr) => stderr,
        Err(_) => {
            return Err(SessionFailure {
                kind: SessionFailureKind::Thread,
                detail: "worker stderr reader thread panicked".to_owned(),
                last_phase: match &decision {
                    SessionDecision::Success { phases, .. } => phases.last().copied(),
                    SessionDecision::Failure(failure) => failure.last_phase,
                },
                stderr: CapturedStderr::default(),
                process_status,
            });
        }
    };

    if !stdout_joined || !writer_joined {
        return Err(SessionFailure {
            kind: SessionFailureKind::Thread,
            detail: "worker transport thread panicked".to_owned(),
            last_phase: match &decision {
                SessionDecision::Success { phases, .. } => phases.last().copied(),
                SessionDecision::Failure(failure) => failure.last_phase,
            },
            stderr,
            process_status,
        });
    }

    match decision {
        SessionDecision::Success {
            hello,
            result,
            phases,
        } => {
            if let Some(read_error) = &stderr.read_error {
                return Err(SessionFailure {
                    kind: SessionFailureKind::Read,
                    detail: format!("cannot drain worker stderr: {read_error}"),
                    last_phase: phases.last().copied(),
                    stderr,
                    process_status,
                });
            }
            Ok(SessionOutcome {
                hello,
                result,
                phases,
                stderr,
                process_status,
            })
        }
        SessionDecision::Failure(mut failure) => {
            failure.stderr = stderr;
            failure.process_status = process_status;
            Err(failure)
        }
    }
}

fn write_request(mut stdin: ChildStdin, request_line: Vec<u8>, sender: SyncSender<SessionEvent>) {
    let result = stdin
        .write_all(&request_line)
        .and_then(|()| stdin.flush())
        .map_err(|error| format!("cannot write worker request: {error}"));
    drop(stdin);
    let _ = sender.send(SessionEvent::WriterFinished(result));
}

fn read_stdout(stdout: ChildStdout, max_line_bytes: usize, sender: SyncSender<SessionEvent>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let event = match read_bounded_line(&mut reader, max_line_bytes) {
            Ok(BoundedLine::Line(line)) => SessionEvent::Line(line),
            Ok(BoundedLine::Eof) => SessionEvent::Eof,
            Ok(BoundedLine::Unterminated { bytes }) => SessionEvent::UnterminatedLine { bytes },
            Ok(BoundedLine::TooLong) => SessionEvent::LineTooLong {
                limit: max_line_bytes,
            },
            Err(error) => SessionEvent::StdoutError(format!("cannot read worker stdout: {error}")),
        };
        let terminal = !matches!(event, SessionEvent::Line(_));
        if sender.send(event).is_err() || terminal {
            break;
        }
    }
}

fn read_bounded_line(reader: &mut impl BufRead, max_line_bytes: usize) -> io::Result<BoundedLine> {
    let mut line = Vec::with_capacity(max_line_bytes.min(8 * 1024));
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                BoundedLine::Eof
            } else {
                BoundedLine::Unterminated { bytes: line.len() }
            });
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let payload_bytes = newline.unwrap_or(available.len());
        if payload_bytes > max_line_bytes.saturating_sub(line.len()) {
            return Ok(BoundedLine::TooLong);
        }
        line.extend_from_slice(&available[..payload_bytes]);
        reader.consume(payload_bytes + usize::from(newline.is_some()));
        if newline.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(BoundedLine::Line(line));
        }
    }
}

fn drain_stderr(mut stderr: ChildStderr, max_bytes: usize) -> CapturedStderr {
    let mut capture = CapturedStderr {
        bytes: Vec::with_capacity(max_bytes.min(8 * 1024)),
        truncated: false,
        read_error: None,
    };
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        match stderr.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                let retained = max_bytes.saturating_sub(capture.bytes.len()).min(read);
                capture.bytes.extend_from_slice(&chunk[..retained]);
                if retained != read {
                    capture.truncated = true;
                }
            }
            Err(error) => {
                capture.read_error = Some(error.to_string());
                break;
            }
        }
    }
    capture
}

fn terminate_and_wait(child: &mut Child) -> Option<String> {
    match child.try_wait() {
        Ok(Some(status)) => child.wait().ok().or(Some(status)),
        Ok(None) | Err(_) => {
            let _ = child.kill();
            child.wait().ok()
        }
    }
    .map(|status| status.to_string())
}

fn session_kind_for_protocol(kind: ProtocolViolationKind) -> SessionFailureKind {
    match kind {
        ProtocolViolationKind::MalformedFrame => SessionFailureKind::MalformedFrame,
        ProtocolViolationKind::Schema => SessionFailureKind::SchemaMismatch,
        ProtocolViolationKind::Handshake => SessionFailureKind::Handshake,
        ProtocolViolationKind::Binding => SessionFailureKind::BindingMismatch,
        ProtocolViolationKind::PhaseOrder => SessionFailureKind::PhaseOrder,
        ProtocolViolationKind::ResultShape => SessionFailureKind::ResultShape,
        ProtocolViolationKind::FrameAfterResult => SessionFailureKind::FrameAfterResult,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::json;

    use super::*;
    use crate::model::EngineResult;

    const WORKER_FIXTURE: &str = r#"
const fs = require("node:fs");
const emit = (value) => process.stdout.write(JSON.stringify(value) + "\n");
emit({
  schema: 1,
  frame: "hello",
  implementation: process.env.TSC_RS_SESSION_MODE === "wrong-hello"
    ? "wrong-worker"
    : "test-worker",
  version: "1.0.0",
});
const request = JSON.parse(fs.readFileSync(0, "utf8"));
const mode = process.env.TSC_RS_SESSION_MODE;
const bound = (frame, payload) => ({
  schema: 1,
  id: request.id,
  case_sha256: request.case_sha256,
  frame,
  ...payload,
});
const phase = (value) => emit(bound("phase", { phase: value }));
const completed = () => emit(bound("result", {
  result: {
    status: "completed",
    assembled: [],
    deduped_indices: [],
    segments: [],
    aggregate_text: "",
  },
}));
if (process.env.TSC_RS_SESSION_MARKER) {
  fs.appendFileSync(process.env.TSC_RS_SESSION_MARKER, "x");
}
if (mode === "oversized") {
  process.stdout.write("x".repeat(1024) + "\n");
} else if (mode === "malformed") {
  process.stdout.write("{}\n");
} else if (mode === "timeout") {
  phase("parse");
  phase("bind");
  phase("check");
  setInterval(() => {}, 1000);
} else {
  if (mode === "stderr") process.stderr.write("z".repeat(4096));
  phase("parse");
  phase("bind");
  phase("check");
  phase("format");
  completed();
  if (mode === "trailing-result") completed();
}
"#;

    static NEXT_MARKER: AtomicU64 = AtomicU64::new(0);

    fn request() -> ExecuteCaseRequest {
        serde_json::from_value(json!({
            "schema": 1,
            "id": "1",
            "op": "execute-case",
            "case_sha256": "a".repeat(64),
            "program": {
                "cwd": "/work",
                "options": [],
                "libs": [],
                "files": [{
                    "ordinal": 0,
                    "name": "main.ts",
                    "text_base64": ""
                }]
            }
        }))
        .unwrap()
    }

    fn worker(mode: &str) -> Command {
        let mut command = Command::new("node");
        command
            .arg("-e")
            .arg(WORKER_FIXTURE)
            .env("TSC_RS_SESSION_MODE", mode);
        command
    }

    fn limits() -> ProcessSessionLimits {
        ProcessSessionLimits {
            max_request_line_bytes: 4 * 1024,
            max_response_line_bytes: 4 * 1024,
            max_stderr_bytes: 32,
            deadline: Duration::from_secs(3),
        }
    }

    fn expected_hello_line() -> &'static [u8] {
        br#"{"schema":1,"frame":"hello","implementation":"test-worker","version":"1.0.0"}"#
    }

    #[test]
    fn successful_session_is_bound_ordered_and_stderr_bounded() {
        let mut command = worker("stderr");
        let outcome =
            run_one_case(&mut command, &request(), limits(), expected_hello_line()).unwrap();
        assert_eq!(outcome.phases, WorkerPhase::ORDERED);
        assert_eq!(outcome.hello.implementation, "test-worker");
        assert_eq!(outcome.hello.version, "1.0.0");
        assert!(matches!(
            outcome.result,
            ValidatedWorkerResult::Engine(EngineResult::Completed { .. })
        ));
        assert_eq!(outcome.stderr.bytes, vec![b'z'; 32]);
        assert!(outcome.stderr.truncated);
    }

    #[test]
    fn absolute_deadline_preserves_last_phase_and_kills_worker() {
        let mut command = worker("timeout");
        let mut bounded = limits();
        bounded.deadline = Duration::from_millis(750);
        let failure =
            run_one_case(&mut command, &request(), bounded, expected_hello_line()).unwrap_err();
        assert_eq!(failure.kind, SessionFailureKind::Deadline);
        assert_eq!(failure.last_phase, Some(WorkerPhase::Check));
    }

    #[test]
    fn response_line_limit_stops_before_json_allocation() {
        let mut command = worker("oversized");
        let mut bounded = limits();
        bounded.max_response_line_bytes = 128;
        let failure =
            run_one_case(&mut command, &request(), bounded, expected_hello_line()).unwrap_err();
        assert_eq!(failure.kind, SessionFailureKind::ResponseLineTooLong);
    }

    #[test]
    fn malformed_response_is_not_retried() {
        let serial = NEXT_MARKER.fetch_add(1, Ordering::Relaxed);
        let marker = PathBuf::from(format!(
            "/tmp/tsc-rs-process-session-{}-{serial}.marker",
            std::process::id()
        ));
        let mut command = worker("malformed");
        command.env("TSC_RS_SESSION_MARKER", &marker);
        let failure =
            run_one_case(&mut command, &request(), limits(), expected_hello_line()).unwrap_err();
        assert_eq!(failure.kind, SessionFailureKind::MalformedFrame);
        assert_eq!(fs::read(&marker).unwrap(), b"x");
        fs::remove_file(marker).unwrap();
    }

    #[test]
    fn trusted_hello_is_checked_before_any_phase_or_terminal_mapping() {
        let mut command = worker("wrong-hello");
        let failure =
            run_one_case(&mut command, &request(), limits(), expected_hello_line()).unwrap_err();
        assert_eq!(failure.kind, SessionFailureKind::Handshake);
        assert_eq!(failure.last_phase, None);
    }

    #[test]
    fn a_second_result_is_rejected_before_session_success() {
        let mut command = worker("trailing-result");
        let failure =
            run_one_case(&mut command, &request(), limits(), expected_hello_line()).unwrap_err();
        assert_eq!(failure.kind, SessionFailureKind::FrameAfterResult);
        assert_eq!(failure.last_phase, Some(WorkerPhase::Format));
    }
}
