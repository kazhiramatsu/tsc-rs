use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_fuzz::adapters::oracle::{TRUSTED_NODE_ARGUMENT, TRUSTED_NODE_EXECUTABLE_ID};
use tsc_fuzz::executor::{
    TRUSTED_CHILD_CASES, TRUSTED_CHILD_POLICY_ID, TRUSTED_NODE_DEADLINE_MS,
    TRUSTED_NODE_ROLLOVER_CASES, TRUSTED_TSRS_DEADLINE_MS, TRUSTED_TSRS_ROLLOVER_CASES,
    TRUSTED_TSRS_WORKER_CAP,
};
use tsc_fuzz::model::{
    AssembledDiagnostic, CanonicalHead, CaseExecution, CompletedOutcome, DiagnosticCategory,
    DiagnosticFile, DiagnosticPass, DiagnosticRecord, EngineResult, MessageChain, OptionalBool,
    OptionalString, OptionalU32, RenderSegment, RendererObservation,
};
use tsc_fuzz::replay::ReplayArtifact;
use tsc_fuzz::schema::{
    CanonicalU64, CaseProvenance, CaseSpec, ChildProcessPolicy, CompilerOptionValue, DecisionValue,
    DomainMembership, EncodedFile, NodeProcessPolicy, OrderedArgument, OrderedSetting,
    ProcessPolicy, RustProcessPolicy, StableDecision, CASE_SPEC_SCHEMA,
};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct Cleanup {
    paths: Vec<PathBuf>,
}

impl Cleanup {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn path(&mut self, label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after the Unix epoch")
            .as_nanos();
        let sequence = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/tsc-rs-fuzz-executor-e2e-{}-{timestamp}-{sequence}-{label}",
            std::process::id()
        ));
        self.paths.push(path.clone());
        path
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

fn trusted_process_policy() -> ProcessPolicy {
    ProcessPolicy {
        schema: 1,
        oracle_node: NodeProcessPolicy {
            executable_id: TRUSTED_NODE_EXECUTABLE_ID.to_owned(),
            arguments: vec![OrderedArgument {
                ordinal: 0,
                value: TRUSTED_NODE_ARGUMENT.to_owned(),
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

fn case() -> CaseSpec {
    CaseSpec {
        schema: CASE_SPEC_SCHEMA,
        case_id: "executor-e2e".to_owned(),
        generator_id: "executor-e2e".to_owned(),
        provenance: CaseProvenance {
            root_seed: CanonicalU64::new(9),
            case_index: CanonicalU64::new(0),
            case_seed: CanonicalU64::new(11),
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
            id: "executor-e2e".to_owned(),
        }],
        cwd: "/work".to_owned(),
        options: vec![
            OrderedSetting {
                ordinal: 0,
                name: "noLib".to_owned(),
                value: CompilerOptionValue::Boolean { value: true },
            },
            OrderedSetting {
                ordinal: 1,
                name: "strict".to_owned(),
                value: CompilerOptionValue::Null,
            },
        ],
        libs: Vec::new(),
        files: vec![EncodedFile {
            ordinal: 0,
            name: "main.ts".to_owned(),
            text_base64: String::new(),
        }],
        matrix_key: String::new(),
        matrix: Vec::new(),
        normalization_schema: 1,
        process_policy: trusted_process_policy(),
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

fn exact_execution() -> CaseExecution {
    CaseExecution::Compared {
        oracle: EngineResult::Completed {
            outcome: completed(Vec::new()),
        },
        tsrs: EngineResult::Completed {
            outcome: completed(Vec::new()),
        },
    }
}

fn fake_saved_divergence() -> CaseExecution {
    let diagnostic = DiagnosticRecord {
        pass: DiagnosticPass::Semantic,
        file: DiagnosticFile::File {
            path: "main.ts".to_owned(),
        },
        code: 99_999,
        line: OptionalU32::Present { value: 0 },
        column: OptionalU32::Present { value: 0 },
        category: DiagnosticCategory::Error,
        start: OptionalU32::Present { value: 0 },
        length: OptionalU32::Present { value: 0 },
        chain: MessageChain {
            text: "deliberate saved-only divergence".to_owned(),
            code: 99_999,
            category: DiagnosticCategory::Error,
            next_present: false,
            next: Vec::new(),
        },
        related_information_present: false,
        related: Vec::new(),
        reports_unnecessary: OptionalBool::absent(),
        reports_deprecated: OptionalBool::absent(),
        source: OptionalString::absent(),
    };
    CaseExecution::Compared {
        oracle: EngineResult::Completed {
            outcome: completed(vec![diagnostic]),
        },
        tsrs: EngineResult::Completed {
            outcome: completed(Vec::new()),
        },
    }
}

fn write_artifact(cleanup: &mut Cleanup, artifact: &ReplayArtifact, label: &str) -> PathBuf {
    let path = cleanup.path(label);
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .expect("artifact path must be unique");
    file.write_all(&artifact.canonical_bytes().unwrap())
        .expect("artifact must be writable");
    path
}

fn replay(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_tsc-rs-fuzz-producer"))
        .arg("replay")
        .arg(path)
        .output()
        .expect("producer binary must launch")
}

fn output_detail(output: &Output) -> String {
    format!(
        "status={:?}\nstdout={}\nstderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn true_replay_runs_actual_node_and_same_binary_rust_worker() {
    let mut cleanup = Cleanup::new();
    let artifact = ReplayArtifact::from_observation(&case(), &exact_execution()).unwrap();
    let path = write_artifact(&mut cleanup, &artifact, "positive.json");

    let output = replay(&path);
    assert!(output.status.success(), "{}", output_detail(&output));
    assert!(
        String::from_utf8_lossy(&output.stdout).starts_with("true replay verified:"),
        "{}",
        output_detail(&output)
    );
}

#[test]
fn fresh_replay_rejects_an_internally_coherent_fake_saved_divergence() {
    let mut cleanup = Cleanup::new();
    let artifact = ReplayArtifact::from_observation(&case(), &fake_saved_divergence()).unwrap();
    artifact.verify_saved().unwrap();
    let path = write_artifact(&mut cleanup, &artifact, "fake-divergence.json");

    let output = replay(&path);
    assert!(!output.status.success(), "{}", output_detail(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("replayed comparison does not match the saved comparison"),
        "{}",
        output_detail(&output)
    );
}

#[cfg(unix)]
#[test]
fn artifact_controlled_oracle_executable_is_never_launched() {
    use std::os::unix::fs::PermissionsExt;

    let mut cleanup = Cleanup::new();
    let script_path = cleanup.path("malicious-oracle.sh");
    let marker_path = cleanup.path("malicious-oracle-launched");
    let script = format!("#!/bin/sh\nprintf launched > '{}'\n", marker_path.display());
    let mut script_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&script_path)
        .expect("script path must be unique");
    script_file
        .write_all(script.as_bytes())
        .expect("script must be writable");
    drop(script_file);
    fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700))
        .expect("script must be executable");

    let mut malicious_case = case();
    malicious_case.process_policy.oracle_node.executable_id =
        script_path.to_string_lossy().into_owned();
    let artifact = ReplayArtifact::from_observation(&malicious_case, &exact_execution()).unwrap();
    let artifact_path = write_artifact(&mut cleanup, &artifact, "malicious-policy.json");

    let output = replay(&artifact_path);
    assert!(!output.status.success(), "{}", output_detail(&output));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("oracle executable_id must be \"node-pinned\""),
        "{}",
        output_detail(&output)
    );
    assert!(
        !marker_path.exists(),
        "artifact-controlled executable was launched"
    );
}
