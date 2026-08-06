use super::*;
use crate::model::TerminalPhase;
use crate::schema::{
    CanonicalU64, CaseProvenance, CaseSpec, ChildProcessPolicy, DecisionValue, DomainMembership,
    EncodedFile, NodeProcessPolicy, OrderedArgument, ProcessPolicy, RustProcessPolicy,
    StableDecision, CASE_SPEC_SCHEMA,
};

fn policy() -> NodeProcessPolicy {
    NodeProcessPolicy {
        executable_id: TRUSTED_NODE_EXECUTABLE_ID.to_owned(),
        arguments: vec![OrderedArgument {
            ordinal: 0,
            value: TRUSTED_NODE_ARGUMENT.to_owned(),
        }],
        single_threaded: true,
        deadline_ms: CanonicalU64::new(30_000),
        rollover_cases: CanonicalU64::new(500),
    }
}

fn case() -> CaseSpec {
    CaseSpec {
        schema: CASE_SPEC_SCHEMA,
        case_id: "oracle-adapter-case".to_owned(),
        generator_id: "oracle-adapter-test".to_owned(),
        provenance: CaseProvenance {
            root_seed: CanonicalU64::new(1),
            case_index: CanonicalU64::new(0),
            case_seed: CanonicalU64::new(2),
        },
        decisions: vec![StableDecision {
            ordinal: 0,
            id: "name".to_owned(),
            value: DecisionValue::Identifier {
                value: "generatedName".to_owned(),
            },
        }],
        domain_membership: vec![DomainMembership {
            ordinal: 0,
            id: "oracle-adapter".to_owned(),
        }],
        cwd: "/work".to_owned(),
        options: Vec::new(),
        libs: Vec::new(),
        files: vec![EncodedFile {
            ordinal: 0,
            name: "main.ts".to_owned(),
            text_base64: "Y29uc3QgeCA9IDE7Cg==".to_owned(),
        }],
        matrix_key: String::new(),
        matrix: Vec::new(),
        normalization_schema: 1,
        process_policy: ProcessPolicy {
            schema: 1,
            oracle_node: policy(),
            tsrs: RustProcessPolicy {
                worker_cap: 1,
                deadline_ms: CanonicalU64::new(30_000),
                rollover_cases: CanonicalU64::new(500),
            },
            child: ChildProcessPolicy {
                policy_id: "bounded-serial-v1".to_owned(),
                cases_per_child: CanonicalU64::new(500),
            },
        },
    }
}

fn session_failure(kind: SessionFailureKind, last_phase: Option<WorkerPhase>) -> SessionFailure {
    SessionFailure {
        kind,
        detail: "fixture interruption".to_owned(),
        last_phase,
        stderr: Default::default(),
        process_status: Some("fixture status".to_owned()),
    }
}

fn assert_adapter_malformed(failure: ProducerFailure) {
    assert_eq!(failure.source, ProducerFailureSource::OracleAdapter);
    assert_eq!(failure.kind, ProducerFailureKind::MalformedResponse);
    failure.validate().unwrap();
}

fn assert_worker_interruption(failure: ProducerFailure) {
    assert_eq!(failure.source, ProducerFailureSource::Worker);
    assert_eq!(failure.kind, ProducerFailureKind::WorkerInterruption);
    failure.validate().unwrap();
}

#[test]
fn policy_is_exact_and_artifact_strings_never_build_the_command() {
    let validated = validate_trusted_policy(&policy()).unwrap();
    assert_eq!(validated.deadline(), Duration::from_secs(30));
    assert_eq!(validated.rollover_cases(), 500);

    let mut wrong = policy();
    wrong.executable_id = "artifact-node".to_owned();
    assert!(validate_trusted_policy(&wrong).is_err());
    let mut extra = policy();
    extra.arguments.push(OrderedArgument {
        ordinal: 1,
        value: "--inspect".to_owned(),
    });
    assert!(validate_trusted_policy(&extra).is_err());

    let command = trusted_node_command();
    assert_eq!(command.get_program(), TRUSTED_NODE_PROGRAM);
    assert_eq!(
        command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        vec![
            TRUSTED_NODE_ARGUMENT.to_owned(),
            oracle_driver_path().display().to_string()
        ]
    );
}

#[test]
fn hello_is_canonical_closed_and_pinned_to_the_launched_process() {
    assert!(pinned_node_version_file_is_exact());
    let line =
        br#"{"schema":1,"frame":"hello","implementation":"oracle-node","version":"v25.2.1"}"#;
    assert_eq!(trusted_hello_line().unwrap(), line);
    validate_hello_line(line).unwrap();
    assert!(validate_hello_line(
        br#"{"schema":1, "frame":"hello","implementation":"oracle-node","version":"v25.2.1"}"#
    )
    .is_err());
    assert!(validate_hello_line(
        br#"{"schema":1,"frame":"hello","implementation":"oracle-node","version":"v25.2.2"}"#
    )
    .is_err());
    assert!(validate_hello_line(
        br#"{"schema":1,"frame":"hello","implementation":"oracle-node","version":"v25.2.1","extra":true}"#
    )
    .is_err());
}

#[test]
fn process_failures_use_the_last_observed_phase_without_oom_guessing() {
    let mut failure = session_failure(SessionFailureKind::UnexpectedEof, Some(WorkerPhase::Check));
    failure.detail = "heap out of memory".to_owned();
    let result = map_session_failure(failure).unwrap();
    let EngineResult::Terminal { outcome } = result else {
        panic!("terminal");
    };
    assert_eq!(outcome.phase, TerminalPhase::Check);
    assert_eq!(outcome.kind, TerminalKind::Crash);
    assert_eq!(outcome.boundary_id, TerminalBoundaryId::ProcessSignal);

    let deadline = session_failure(SessionFailureKind::Deadline, Some(WorkerPhase::Bind));
    let EngineResult::Terminal { outcome } = map_session_failure(deadline).unwrap() else {
        panic!("terminal");
    };
    assert_eq!(outcome.phase, TerminalPhase::Bind);
    assert_eq!(outcome.kind, TerminalKind::Timeout);
    assert_eq!(outcome.boundary_id, TerminalBoundaryId::Deadline);
}

#[test]
fn failures_before_the_first_phase_are_worker_interruptions() {
    for kind in [
        SessionFailureKind::Deadline,
        SessionFailureKind::UnexpectedEof,
        SessionFailureKind::Read,
    ] {
        let failure = map_session_failure(session_failure(kind, None)).unwrap_err();
        assert_worker_interruption(failure);
    }

    let case = case();
    let request = execute_request(7, &case).unwrap();
    let decoder = OracleResponseDecoder::new(&case, &request);
    assert_worker_interruption(decoder.deadline_result().unwrap_err());
    assert_worker_interruption(decoder.process_exit_result("signal").unwrap_err());
}

#[test]
fn malformed_oversize_binding_and_hello_are_adapter_failures() {
    let case = case();
    let request = execute_request(7, &case).unwrap();

    let mut malformed_decoder = OracleResponseDecoder::new(&case, &request);
    assert_adapter_malformed(malformed_decoder.accept_line(b"{").unwrap_err());

    for kind in [
        SessionFailureKind::InvalidRequest,
        SessionFailureKind::ResponseLineTooLong,
    ] {
        let oversized = session_failure(kind, None);
        assert_adapter_malformed(map_session_failure(oversized).unwrap_err());
    }

    let hello =
        br#"{"schema":1,"frame":"hello","implementation":"oracle-node","version":"v25.2.1"}"#;
    for (id, case_sha256) in [
        ("8".to_owned(), request.case_sha256().to_owned()),
        (request.id().to_string(), "0".repeat(64)),
    ] {
        let mut decoder = OracleResponseDecoder::new(&case, &request);
        decoder.accept_line(hello).unwrap();
        let line = format!(
            r#"{{"schema":1,"id":"{id}","case_sha256":"{case_sha256}","frame":"phase","phase":"parse"}}"#
        );
        assert_adapter_malformed(decoder.accept_line(line.as_bytes()).unwrap_err());
    }

    assert_adapter_malformed(
        validate_hello_line(
            br#"{"schema":1,"frame":"hello","implementation":"oracle-node","version":"v25.2.2"}"#,
        )
        .unwrap_err(),
    );
}

#[test]
fn trusted_node_session_completes_one_canonical_case() {
    let result = execute_oracle_case(7, &case()).unwrap().unwrap();
    let EngineResult::Completed { outcome } = result else {
        panic!("completed");
    };
    assert!(outcome.diagnostics.is_empty());
    assert!(outcome.renderer.assembled.is_empty());
    assert!(outcome.renderer.deduped.is_empty());
    assert!(outcome.renderer.aggregate_text.is_empty());
}
