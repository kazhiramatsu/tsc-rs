use super::*;
use crate::model::{
    CanonicalHead, DiagnosticCategory, DiagnosticFile, DiagnosticPass, DiagnosticRecord,
    MessageChain, OptionalBool, OptionalString, OptionalU32,
};

fn binding_json(frame: serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(&frame).unwrap()
}

fn completed_result() -> serde_json::Value {
    serde_json::json!({
        "status": "completed",
        "assembled": [],
        "deduped_indices": [],
        "segments": [],
        "aggregate_text": ""
    })
}

fn global_diagnostic() -> AssembledDiagnostic {
    AssembledDiagnostic {
        diagnostic: DiagnosticRecord {
            pass: DiagnosticPass::Semantic,
            file: DiagnosticFile::Global,
            code: 1,
            line: OptionalU32::Absent,
            column: OptionalU32::Absent,
            category: DiagnosticCategory::Error,
            start: OptionalU32::Absent,
            length: OptionalU32::Absent,
            chain: MessageChain {
                text: "diagnostic".to_owned(),
                code: 1,
                category: DiagnosticCategory::Error,
                next_present: false,
                next: Vec::new(),
            },
            related_information_present: false,
            related: Vec::new(),
            reports_unnecessary: OptionalBool::absent(),
            reports_deprecated: OptionalBool::absent(),
            source: OptionalString::absent(),
        },
        canonical_head: CanonicalHead::absent(),
    }
}

fn feed_phases(validator: &mut ResponseValidator, request: &ExecuteCaseRequest) {
    let hello = binding_json(serde_json::json!({
        "schema": 1,
        "frame": "hello",
        "implementation": "test-worker",
        "version": "1.0.0"
    }));
    assert!(validator.accept_line(&hello).unwrap().is_none());
    for phase in ["parse", "bind", "check", "format"] {
        let line = binding_json(serde_json::json!({
            "schema": 1,
            "id": request.id().to_string(),
            "case_sha256": request.case_sha256(),
            "frame": "phase",
            "phase": phase
        }));
        assert!(validator.accept_line(&line).unwrap().is_none());
    }
}

#[test]
fn request_is_compact_bound_and_size_limited() {
    let request = ExecuteCaseRequest::fixture(7);
    let line = request.canonical_line(4_096).unwrap();
    assert_eq!(line.last(), Some(&b'\n'));
    assert!(!line[..line.len() - 1].contains(&b'\n'));
    let value: serde_json::Value = serde_json::from_slice(&line).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["id"], "7");
    assert_eq!(value["op"], EXECUTE_CASE_OPERATION);
    assert_eq!(value["case_sha256"], "a".repeat(64));
    assert!(request.canonical_line(line.len() - 2).is_err());
}

#[test]
fn ordered_bound_completed_result_is_converted() {
    let request = ExecuteCaseRequest::fixture(8);
    let mut validator = ResponseValidator::new(&request);
    feed_phases(&mut validator, &request);
    let line = binding_json(serde_json::json!({
        "schema": 1,
        "id": "8",
        "case_sha256": "a".repeat(64),
        "frame": "result",
        "result": completed_result()
    }));
    let result = validator.accept_line(&line).unwrap().unwrap();
    assert!(matches!(
        result,
        ValidatedWorkerResult::Engine(EngineResult::Completed { .. })
    ));
}

#[test]
fn unknown_fields_binding_drift_and_phase_skips_are_rejected() {
    let request = ExecuteCaseRequest::fixture(9);

    let mut validator = ResponseValidator::new(&request);
    let unknown = binding_json(serde_json::json!({
        "schema": 1,
        "id": "9",
        "case_sha256": "a".repeat(64),
        "frame": "phase",
        "phase": "parse",
        "unknown": true
    }));
    assert_eq!(
        validator.accept_line(&unknown).unwrap_err().kind(),
        ProtocolViolationKind::MalformedFrame
    );

    let mut validator = ResponseValidator::new(&request);
    let hello = binding_json(serde_json::json!({
        "schema": 1,
        "frame": "hello",
        "implementation": "test-worker",
        "version": "1.0.0"
    }));
    validator.accept_line(&hello).unwrap();
    let wrong_binding = binding_json(serde_json::json!({
        "schema": 1,
        "id": "10",
        "case_sha256": "a".repeat(64),
        "frame": "phase",
        "phase": "parse"
    }));
    assert_eq!(
        validator.accept_line(&wrong_binding).unwrap_err().kind(),
        ProtocolViolationKind::Binding
    );

    let mut validator = ResponseValidator::new(&request);
    validator.accept_line(&hello).unwrap();
    let skipped = binding_json(serde_json::json!({
        "schema": 1,
        "id": "9",
        "case_sha256": "a".repeat(64),
        "frame": "phase",
        "phase": "bind"
    }));
    assert_eq!(
        validator.accept_line(&skipped).unwrap_err().kind(),
        ProtocolViolationKind::PhaseOrder
    );
}

#[test]
fn hello_is_required_exactly_once_before_phases() {
    let request = ExecuteCaseRequest::fixture(11);
    let phase = binding_json(serde_json::json!({
        "schema": 1,
        "id": "11",
        "case_sha256": "a".repeat(64),
        "frame": "phase",
        "phase": "parse"
    }));
    let mut missing = ResponseValidator::new(&request);
    assert_eq!(
        missing.accept_line(&phase).unwrap_err().kind(),
        ProtocolViolationKind::Handshake
    );

    let hello = binding_json(serde_json::json!({
        "schema": 1,
        "frame": "hello",
        "implementation": "test-worker",
        "version": "1.0.0"
    }));
    let mut duplicate = ResponseValidator::new(&request);
    duplicate.accept_line(&hello).unwrap();
    assert_eq!(
        duplicate.accept_line(&hello).unwrap_err().kind(),
        ProtocolViolationKind::Handshake
    );
}

#[test]
fn completed_renderer_join_is_revalidated() {
    let request = ExecuteCaseRequest::fixture(10);
    let mut validator = ResponseValidator::new(&request);
    feed_phases(&mut validator, &request);
    let line = binding_json(serde_json::json!({
        "schema": 1,
        "id": "10",
        "case_sha256": "a".repeat(64),
        "frame": "result",
        "result": {
            "status": "completed",
            "assembled": [],
            "deduped_indices": [],
            "segments": [],
            "aggregate_text": "spliced"
        }
    }));
    assert_eq!(
        validator.accept_line(&line).unwrap_err().kind(),
        ProtocolViolationKind::ResultShape
    );
}

#[test]
fn completed_result_preserves_deduped_order_and_multiplicity() {
    let result = WorkerResult::Completed {
        assembled: vec![global_diagnostic()],
        deduped_indices: vec![0, 0],
        segments: vec![
            WireRenderSegment {
                assembled_index: 0,
                raw_text: "first".to_owned(),
            },
            WireRenderSegment {
                assembled_index: 0,
                raw_text: "second".to_owned(),
            },
        ],
        aggregate_text: "firstsecond".to_owned(),
    };
    let ValidatedWorkerResult::Engine(EngineResult::Completed { outcome }) =
        result.into_validated().unwrap()
    else {
        panic!("expected completed engine result");
    };
    assert_eq!(outcome.renderer.deduped.len(), 2);
    assert_eq!(outcome.renderer.segments.len(), 2);
    assert_eq!(outcome.renderer.aggregate_text, "firstsecond");
}
