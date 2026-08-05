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
    let outcome = run_one_case(&mut command, &request(), limits(), expected_hello_line()).unwrap();
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
