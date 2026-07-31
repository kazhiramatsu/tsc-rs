use tsrs2_fuzz::compare::Comparison;
use tsrs2_fuzz::model::{
    AssembledDiagnostic, CanonicalHead, CaseExecution, CompletedOutcome, DiagnosticCategory,
    DiagnosticFile, DiagnosticPass, DiagnosticRecord, EngineResult, MessageChain, OptionalBool,
    OptionalString, OptionalU32, RenderSegment, RendererObservation,
};
use tsrs2_fuzz::replay::{
    ReplayArtifact, REPLAY_ARTIFACT_SCHEMA, REPLAY_COMPARATOR_ID, REPLAY_COMPARATOR_SCHEMA,
};
use tsrs2_fuzz::schema::{
    CanonicalU64, CaseProvenance, CaseSpec, ChildProcessPolicy, DecisionValue, DomainMembership,
    EncodedFile, NodeProcessPolicy, OrderedArgument, ProcessPolicy, RustProcessPolicy,
    StableDecision, CASE_SPEC_SCHEMA,
};

fn process_policy() -> ProcessPolicy {
    ProcessPolicy {
        schema: 1,
        oracle_node: NodeProcessPolicy {
            executable_id: "node-pinned".to_owned(),
            arguments: vec![OrderedArgument {
                ordinal: 0,
                value: "--single-threaded".to_owned(),
            }],
            single_threaded: true,
            deadline_ms: CanonicalU64::new(30_000),
            rollover_cases: CanonicalU64::new(500),
        },
        tsrs: RustProcessPolicy {
            worker_cap: 2,
            deadline_ms: CanonicalU64::new(30_000),
            rollover_cases: CanonicalU64::new(500),
        },
        child: ChildProcessPolicy {
            policy_id: "bounded-serial-v1".to_owned(),
            cases_per_child: CanonicalU64::new(500),
        },
    }
}

fn case() -> CaseSpec {
    CaseSpec {
        schema: CASE_SPEC_SCHEMA,
        case_id: "replay-case".to_owned(),
        generator_id: "replay-contract".to_owned(),
        provenance: CaseProvenance {
            root_seed: CanonicalU64::new(u64::MAX),
            case_index: CanonicalU64::new(0),
            case_seed: CanonicalU64::new(7),
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
            id: "supported-batch".to_owned(),
        }],
        cwd: "/work".to_owned(),
        options: Vec::new(),
        libs: Vec::new(),
        files: vec![EncodedFile {
            ordinal: 0,
            name: "main.ts".to_owned(),
            text_base64: "eAo=".to_owned(),
        }],
        matrix_key: String::new(),
        matrix: Vec::new(),
        normalization_schema: 1,
        process_policy: process_policy(),
    }
}

fn present(value: u32) -> OptionalU32 {
    OptionalU32::Present { value }
}

fn diagnostic(code: u32, text: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        pass: DiagnosticPass::Semantic,
        file: DiagnosticFile::File {
            path: "main.ts".to_owned(),
        },
        code,
        line: present(0),
        column: present(0),
        category: DiagnosticCategory::Error,
        start: present(0),
        length: present(1),
        chain: MessageChain {
            text: text.to_owned(),
            code,
            category: DiagnosticCategory::Error,
            next_present: false,
            next: Vec::new(),
        },
        related_information_present: false,
        related: Vec::new(),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    }
}

fn completed(diagnostics: Vec<DiagnosticRecord>) -> CompletedOutcome {
    let assembled = diagnostics
        .iter()
        .cloned()
        .map(|diagnostic| AssembledDiagnostic {
            diagnostic,
            canonical_head: CanonicalHead::absent(),
        })
        .collect::<Vec<_>>();
    CompletedOutcome {
        diagnostics,
        renderer: RendererObservation {
            assembled: assembled.clone(),
            deduped: assembled.clone(),
            segments: assembled
                .into_iter()
                .map(|diagnostic| RenderSegment {
                    diagnostic,
                    raw_text: String::new(),
                })
                .collect(),
            aggregate_text: String::new(),
        },
    }
}

fn execution(oracle: Vec<DiagnosticRecord>, tsrs: Vec<DiagnosticRecord>) -> CaseExecution {
    CaseExecution::Compared {
        oracle: EngineResult::Completed {
            outcome: completed(oracle),
        },
        tsrs: EngineResult::Completed {
            outcome: completed(tsrs),
        },
    }
}

fn exact_execution() -> CaseExecution {
    execution(Vec::new(), Vec::new())
}

fn fake_saved_divergence() -> CaseExecution {
    execution(
        vec![diagnostic(99_999, "deliberate saved-only divergence")],
        Vec::new(),
    )
}

#[test]
fn comparison_has_a_closed_canonical_shape() {
    let case = case();
    let artifact = ReplayArtifact::from_observation(&case, &fake_saved_divergence()).unwrap();
    let bytes = artifact.comparison.canonical_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with(r#"{"status":"divergence","divergence":{"kind":"diagnostic""#));
    assert_eq!(
        Comparison::from_canonical_slice(&bytes).unwrap(),
        artifact.comparison
    );
    assert!(Comparison::from_canonical_slice(format!("{text}\n").as_bytes()).is_err());

    let mut unknown = serde_json::to_value(&artifact.comparison).unwrap();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_owned(), serde_json::json!(true));
    assert!(Comparison::from_json_slice(&serde_json::to_vec(&unknown).unwrap()).is_err());
}

#[test]
fn replay_artifact_rederives_every_saved_projection() {
    let case = case();
    let execution = exact_execution();
    let artifact = ReplayArtifact::from_observation(&case, &execution).unwrap();
    assert_eq!(artifact.schema, REPLAY_ARTIFACT_SCHEMA);
    assert_eq!(artifact.comparator_schema, REPLAY_COMPARATOR_SCHEMA);
    assert_eq!(artifact.comparator_id, REPLAY_COMPARATOR_ID);
    assert_eq!(artifact.case_sha256, case.canonical_sha256().unwrap());
    assert_eq!(
        artifact.saved_execution_sha256,
        execution.canonical_sha256(&case).unwrap()
    );
    assert_eq!(artifact.comparison, Comparison::Exact);
    assert!(artifact.canonical_class.is_none());

    let bytes = artifact.canonical_bytes().unwrap();
    assert_eq!(
        ReplayArtifact::from_canonical_slice(&bytes).unwrap(),
        artifact
    );
    assert_eq!(
        artifact.canonical_sha256().unwrap(),
        tsrs2_fuzz::schema::sha256_hex(&bytes)
    );
}

#[test]
fn replay_artifact_rejects_comparator_identity_tampering() {
    let artifact = ReplayArtifact::from_observation(&case(), &exact_execution()).unwrap();

    let mut wrong_schema = artifact.clone();
    wrong_schema.comparator_schema = REPLAY_COMPARATOR_SCHEMA + 1;
    let error = wrong_schema.verify_saved().unwrap_err().to_string();
    assert!(error.contains("comparator schema"), "{error}");
    let bytes = serde_json::to_vec(&wrong_schema).unwrap();
    assert!(ReplayArtifact::from_json_slice(&bytes).is_err());

    let mut wrong_id = artifact;
    wrong_id.comparator_id = "tier-first-tampered-v1".to_owned();
    let error = wrong_id.verify_saved().unwrap_err().to_string();
    assert!(error.contains("comparator id"), "{error}");
    let bytes = serde_json::to_vec(&wrong_id).unwrap();
    assert!(ReplayArtifact::from_json_slice(&bytes).is_err());
}

#[test]
fn replay_artifact_rejects_unknown_noncanonical_and_hash_drift() {
    let artifact = ReplayArtifact::from_observation(&case(), &exact_execution()).unwrap();
    let canonical = artifact.canonical_bytes().unwrap();
    let text = String::from_utf8(canonical).unwrap();
    assert!(ReplayArtifact::from_json_slice(format!("{text}\n").as_bytes()).is_ok());
    assert!(ReplayArtifact::from_canonical_slice(format!("{text}\n").as_bytes()).is_err());

    let mut unknown = serde_json::to_value(&artifact).unwrap();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unknown".to_owned(), serde_json::json!(true));
    assert!(ReplayArtifact::from_json_slice(&serde_json::to_vec(&unknown).unwrap()).is_err());

    let mut wrong_case_hash = artifact.clone();
    wrong_case_hash.case_sha256 = "0".repeat(64);
    assert!(wrong_case_hash.verify_saved().is_err());

    let mut wrong_execution_hash = artifact;
    wrong_execution_hash.saved_execution_sha256 = "f".repeat(64);
    assert!(wrong_execution_hash.verify_saved().is_err());
}

#[test]
fn replay_artifact_rejects_execution_comparison_and_class_splices() {
    let case = case();
    let divergent_execution = fake_saved_divergence();
    let divergent = ReplayArtifact::from_observation(&case, &divergent_execution).unwrap();
    assert!(divergent.canonical_class.is_some());

    let exact_execution = exact_execution();
    let mut execution_splice = divergent.clone();
    execution_splice.saved_execution = exact_execution.clone();
    execution_splice.saved_execution_sha256 = exact_execution.canonical_sha256(&case).unwrap();
    let error = execution_splice.verify_saved().unwrap_err().to_string();
    assert!(error.contains("comparison"), "{error}");

    let mut comparison_splice = ReplayArtifact::from_observation(&case, &exact_execution).unwrap();
    comparison_splice.comparison = divergent.comparison.clone();
    let error = comparison_splice.verify_saved().unwrap_err().to_string();
    assert!(error.contains("comparison"), "{error}");

    let mut class_splice = divergent;
    class_splice.canonical_class = None;
    let error = class_splice.verify_saved().unwrap_err().to_string();
    assert!(error.contains("canonical class"), "{error}");
}

#[test]
fn internally_coherent_fake_divergence_is_preserved_for_true_replay() {
    // The pure artifact layer must accept an internally coherent observation;
    // it cannot claim that the current engines produced it. The executor E2E
    // uses this exact shape as its negative canary: saved-only verification
    // passes here, but true replay must fail when the current engines return
    // `Comparison::Exact`.
    let artifact = ReplayArtifact::from_observation(&case(), &fake_saved_divergence()).unwrap();
    artifact.verify_saved().unwrap();
    assert_ne!(artifact.comparison, Comparison::Exact);
    assert!(artifact.canonical_class.is_some());
    let error = artifact
        .verify_replayed_execution(&exact_execution())
        .unwrap_err()
        .to_string();
    assert!(error.contains("replayed comparison"), "{error}");
}

#[test]
fn true_replay_requires_comparison_and_class_but_not_raw_execution_hash() {
    let case = case();
    let saved_execution = exact_execution();
    let artifact = ReplayArtifact::from_observation(&case, &saved_execution).unwrap();

    let common = diagnostic(1_001, "same common row on both engines");
    let replayed_execution = execution(vec![common.clone()], vec![common]);
    assert_ne!(
        saved_execution.canonical_sha256(&case).unwrap(),
        replayed_execution.canonical_sha256(&case).unwrap()
    );
    let replayed = artifact
        .verify_replayed_execution(&replayed_execution)
        .unwrap();
    assert_eq!(replayed.comparison(), &Comparison::Exact);
    assert!(replayed.canonical_class().is_none());
}
