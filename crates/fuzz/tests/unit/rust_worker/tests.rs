use std::io::{BufReader, Cursor};

use super::*;
use crate::worker_protocol::{ResponseValidator, ValidatedWorkerResult};

fn request_line(files_json: &str) -> (ExecuteCaseRequest, Vec<u8>) {
    let seed = format!(
        "{{\"schema\":1,\"id\":\"7\",\"op\":\"execute-case\",\"case_sha256\":\"{}\",\"program\":{{\"cwd\":\"/work\",\"options\":[],\"libs\":[],\"files\":{files_json}}}}}",
        "a".repeat(64)
    );
    let request: ExecuteCaseRequest = serde_json::from_str(&seed).unwrap();
    let line = request
        .canonical_line(DEFAULT_MAX_REQUEST_LINE_BYTES)
        .unwrap();
    (request, line)
}

#[test]
fn worker_emits_hello_phases_and_one_completed_result() {
    let (request, line) = request_line(
        r#"[{"ordinal":0,"name":"main.ts","text_base64":"Y29uc3QgYnJva2VuID0gOwo="}]"#,
    );
    let mut input = BufReader::new(Cursor::new(line));
    let mut output = Vec::new();
    serve_one(&mut input, &mut output, DEFAULT_MAX_REQUEST_LINE_BYTES).unwrap();

    let mut validator = ResponseValidator::new(&request);
    let mut final_result = None;
    for line in output.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        if let Some(result) = validator.accept_line(line).unwrap() {
            assert!(final_result.replace(result).is_none());
        }
    }
    assert_eq!(validator.phases(), WorkerPhase::ORDERED);
    assert_eq!(
        validator.hello().expect("hello").implementation,
        TSRS_WORKER_IMPLEMENTATION
    );
    assert!(matches!(
        final_result,
        Some(ValidatedWorkerResult::Engine(
            EngineResult::Completed { .. }
        ))
    ));
}

#[test]
fn invalid_program_is_rejected_before_parse() {
    let (request, line) = request_line("[]");
    let mut input = BufReader::new(Cursor::new(line));
    let mut output = Vec::new();
    serve_one(&mut input, &mut output, DEFAULT_MAX_REQUEST_LINE_BYTES).unwrap();

    let mut validator = ResponseValidator::new(&request);
    let mut result = None;
    for line in output.split(|byte| *byte == b'\n') {
        if !line.is_empty() {
            result = validator.accept_line(line).unwrap().or(result);
        }
    }
    assert!(validator.phases().is_empty());
    assert!(matches!(result, Some(ValidatedWorkerResult::Rejected(_))));
}

#[test]
fn panic_is_caught_at_the_last_flushed_phase() {
    let (request, line) =
        request_line(r#"[{"ordinal":0,"name":"main.ts","text_base64":"Y29uc3QgeCA9IDE7Cg=="}]"#);
    let mut input = BufReader::new(Cursor::new(line));
    let mut output = Vec::new();
    serve_one_with(
        &mut input,
        &mut output,
        DEFAULT_MAX_REQUEST_LINE_BYTES,
        |_prepared, observe_phase| {
            observe_phase(WorkerPhase::Parse);
            panic!("parse canary");
        },
    )
    .unwrap();

    let mut validator = ResponseValidator::new(&request);
    let mut result = None;
    for line in output.split(|byte| *byte == b'\n') {
        if !line.is_empty() {
            result = validator.accept_line(line).unwrap().or(result);
        }
    }
    let Some(ValidatedWorkerResult::Engine(EngineResult::Terminal { outcome })) = result else {
        panic!("panic terminal");
    };
    assert_eq!(outcome.phase, crate::model::TerminalPhase::Parse);
    assert_eq!(outcome.kind, TerminalKind::Panic);
    assert_eq!(outcome.boundary_id, TerminalBoundaryId::PhaseInvariant);
}

#[test]
fn panic_before_the_first_phase_does_not_fabricate_parse() {
    let (request, line) =
        request_line(r#"[{"ordinal":0,"name":"main.ts","text_base64":"Y29uc3QgeCA9IDE7Cg=="}]"#);
    let mut input = BufReader::new(Cursor::new(line));
    let mut output = Vec::new();
    let error = serve_one_with(
        &mut input,
        &mut output,
        DEFAULT_MAX_REQUEST_LINE_BYTES,
        |_prepared, _observe_phase| panic!("before phase"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("before the first phase"));

    let lines = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    let mut validator = ResponseValidator::new(&request);
    assert!(validator.accept_line(lines[0]).unwrap().is_none());
    assert!(validator.phases().is_empty());
}

#[test]
fn request_reader_is_bounded_and_requires_a_delimiter() {
    let mut long = BufReader::new(Cursor::new(b"12345\n".to_vec()));
    assert!(matches!(
        read_bounded_line(&mut long, 4).unwrap(),
        BoundedRequestLine::TooLong
    ));
    let mut unterminated = BufReader::new(Cursor::new(b"1234".to_vec()));
    assert!(matches!(
        read_bounded_line(&mut unterminated, 4).unwrap(),
        BoundedRequestLine::Unterminated { bytes: 4 }
    ));
}
