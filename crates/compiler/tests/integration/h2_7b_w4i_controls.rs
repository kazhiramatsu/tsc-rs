//! h2-7b-w4 integrator controls — the frozen closure rows of the harness
//! single-writer classes: S2 (directory symlinks published into the suite VFS)
//! and P1 (the project runner keeps the parsed config identity so the TS5055
//! hint condition matches upstream's `!options.configFilePath`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use serde_json::Value;
use tsc_compiler::{MemoryOutputSink, ProgramSession};
use tsc_diagnostics::MessageChain;
use tsc_emitter::{EmitArtifactKind, EmitWriteMetadata};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit_with_option_floor, load_project_emit_with_option_floor,
    load_qualified_compiler_emit_with_symlinks, load_recorded_execution_plans, EmitOptionFloor,
    UpstreamExecutionInput,
};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

const QUALIFICATION_RELATIVE_PATH: &str = "ratchets/h2-7b-qualification.v1.json";

/// S2 — eight rows whose fixtures link DIRECTORIES (`@link: dir -> dir`):
/// upstream's vfs mounts the target directory at the link path, so every
/// descendant file is visible under the link spelling with `realpath`
/// resolving to the physical file. The frozen input records those file-level
/// aliases (`vfs_symlinks`); the runner's VFS builder now publishes the same
/// set from the directives.
const DIRECTORY_SYMLINK_CASES: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitForGlobalishSpecifierSymlink.ts#default",
    "typescript-6.0.3/compiler/declarationEmitForGlobalishSpecifierSymlink2.ts#default",
    "typescript-6.0.3/compiler/symbolLinkDeclarationEmitModuleNames.ts#default",
    "typescript-6.0.3/compiler/symbolLinkDeclarationEmitModuleNamesImportRef.ts#default",
    "typescript-6.0.3/compiler/symlinkedWorkspaceDependenciesNoDirectLinkGeneratesDeepNonrelativeName.ts#default",
    "typescript-6.0.3/compiler/symlinkedWorkspaceDependenciesNoDirectLinkGeneratesNonrelativeName.ts#default",
    "typescript-6.0.3/compiler/symlinkedWorkspaceDependenciesNoDirectLinkOptionalGeneratesNonrelativeName.ts#default",
    "typescript-6.0.3/compiler/symlinkedWorkspaceDependenciesNoDirectLinkPeerGeneratesNonrelativeName.ts#default",
];

/// P1 — the project pair whose TS5055 messages carry no tsconfig hint:
/// upstream only chains the hint when `!options.configFilePath`
/// (`_tsc.js:125033-125040`), and the project runner parses the config WITH
/// its file name, so the emitting adapter must keep the config identity.
const PROJECT_CONFIG_IDENTITY_CASES: &[&str] = &[
    "typescript-6.0.3/project/jsFileCompilationSameNameDtsNotSpecifiedWithAllowJs.json#module%3Damd",
    "typescript-6.0.3/project/jsFileCompilationSameNameDtsNotSpecifiedWithAllowJs.json#module%3Dcommonjs",
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn frozen_case(case_id: &str) -> Value {
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(workspace_root().join(QUALIFICATION_RELATIVE_PATH))
            .expect("the frozen qualification artifact"),
    )
    .expect("valid qualification JSON");
    artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"] == case_id)
        .cloned()
        .unwrap_or_else(|| panic!("{case_id} is not a frozen band row"))
}

fn decode(value: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(value.as_str().expect("base64 text"))
        .expect("valid base64")
}

/// The frozen input replayed through the qualified loader with the recorded
/// file-level symlinks — the oracle's own view of the vfs.
fn prepared_symlink_row(case: &Value) -> PreparedProgram {
    let input = &case["input"];
    let mut files = input["files"]
        .as_array()
        .expect("case input files")
        .iter()
        .map(|file| {
            (
                PathBuf::from(file["path"].as_str().expect("file path")),
                decode(&file["utf8_base64"]),
            )
        })
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            decode(&config["utf8_base64"]),
        ));
    }
    let symlinks = input["vfs_symlinks"]
        .as_array()
        .expect("case vfs symlinks")
        .iter()
        .map(|link| {
            (
                PathBuf::from(link["link_path"].as_str().expect("link path")),
                PathBuf::from(link["target_path"].as_str().expect("target path")),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        !symlinks.is_empty(),
        "{}: the frozen input records the vfs symlinks",
        case["case_id"]
    );
    let roots = input["roots"]
        .as_array()
        .expect("case roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root path")))
        .collect::<Vec<_>>();
    let settings = input["settings"]
        .as_array()
        .expect("case settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("setting name").to_owned(),
                setting["value"].as_str().expect("setting value").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    load_qualified_compiler_emit_with_symlinks(
        &workspace_root(),
        input["current_directory"]
            .as_str()
            .expect("case current directory"),
        &files,
        &symlinks,
        &roots,
        &settings,
        ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024),
        EmitOptionFloor::DeclarationFamily,
    )
    .expect("the frozen band row loads through the declaration-family floor")
}

/// The recorded compiler plan replayed exactly as the H2.7b runner's
/// `recorded-compiler-plan` route does — the suite VFS builder expands the
/// fixture's directory links itself.
fn prepared_recorded_compiler_row(case_id: &str) -> PreparedProgram {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("the recorded execution corpus");
    let plan = corpus
        .plans
        .iter()
        .find_map(|recorded| match &recorded.input {
            UpstreamExecutionInput::Compiler(plan)
                if recorded.provenance.case_id.as_ref() == case_id =>
            {
                Some(plan.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{case_id}: recorded compiler plan is absent"));
    load_compiler_emit_with_option_floor(
        &workspace,
        &plan,
        limits(),
        EmitOptionFloor::DeclarationFamily,
    )
    .unwrap_or_else(|error| panic!("{case_id}: compiler prepare failed: {error}"))
}

/// The recorded project plan replayed exactly as the H2.7b runner's
/// `project-mount` route does (structural runner controls removed, the
/// declaration-family floor applied).
fn prepared_project_row(case_id: &str) -> PreparedProgram {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("the recorded execution corpus");
    let plan = corpus
        .plans
        .iter()
        .find_map(|recorded| match &recorded.input {
            UpstreamExecutionInput::Project(plan)
                if recorded.provenance.case_id.as_ref() == case_id =>
            {
                Some(plan.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("{case_id}: recorded project plan is absent"));
    let mut project_plan = plan;
    let mut fixture = (*project_plan.fixture).clone();
    fixture.properties = Arc::from(
        fixture
            .properties
            .iter()
            .filter(|property| {
                !matches!(
                    property.name.as_ref(),
                    "emittedFiles" | "resolveMapRoot" | "resolveSourceRoot"
                )
            })
            .cloned()
            .collect::<Vec<_>>(),
    );
    project_plan.fixture = Arc::new(fixture);
    load_project_emit_with_option_floor(
        &workspace,
        &project_plan,
        limits(),
        EmitOptionFloor::DeclarationFamily,
    )
    .unwrap_or_else(|error| panic!("{case_id}: project prepare failed: {error}"))
    .prepared_program
}

type DiagnosticTuple = (
    u32,
    String,
    Option<String>,
    Option<u32>,
    Option<u32>,
    String,
);

fn expected_diagnostics(value: &Value) -> Vec<DiagnosticTuple> {
    value
        .as_array()
        .expect("frozen diagnostics")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["code"].as_u64().expect("diagnostic code") as u32,
                diagnostic["category"]
                    .as_str()
                    .expect("diagnostic category")
                    .to_owned(),
                diagnostic["file"].as_str().map(str::to_owned),
                diagnostic["start"].as_u64().map(|value| value as u32),
                diagnostic["length"].as_u64().map(|value| value as u32),
                diagnostic["message"]
                    .as_str()
                    .expect("diagnostic message")
                    .to_owned(),
            )
        })
        .collect()
}

fn actual_diagnostics(diagnostics: &[tsc_diagnostics::Diagnostic]) -> Vec<DiagnosticTuple> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            let mut message = String::new();
            flatten_message_chain(&diagnostic.message, 0, &mut message);
            (
                diagnostic.code(),
                format!("{:?}", diagnostic.category()),
                diagnostic.file_name.clone(),
                diagnostic.start,
                diagnostic.length,
                message,
            )
        })
        .collect()
}

fn flatten_message_chain(chain: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        for _ in 0..indent {
            output.push_str("  ");
        }
    }
    output.push_str(&chain.text);
    for child in &chain.next {
        flatten_message_chain(child, indent + 1, output);
    }
}

fn artifact_kind(kind: EmitArtifactKind, path: &Path) -> &'static str {
    match kind {
        EmitArtifactKind::JavaScript
            if path.extension().and_then(|value| value.to_str()) == Some("mjs") =>
        {
            "mjs"
        }
        EmitArtifactKind::JavaScript
            if path.extension().and_then(|value| value.to_str()) == Some("cjs") =>
        {
            "cjs"
        }
        EmitArtifactKind::JavaScript
            if path.extension().and_then(|value| value.to_str()) == Some("jsx") =>
        {
            "jsx"
        }
        EmitArtifactKind::JavaScript => "javascript",
        EmitArtifactKind::JavaScriptMap => "javascript-map",
        EmitArtifactKind::Declaration => "declaration",
        EmitArtifactKind::DeclarationMap => "declaration-map",
        EmitArtifactKind::BuildInfo => "build-info",
    }
}

fn assert_frozen_observation(case_id: &str, case: &Value, prepared: PreparedProgram) {
    assert_eq!(case["disposition"], "admitted-for-execution", "{case_id}");
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = ProgramSession::new(prepared)
        .emit_with_reported_diagnostics_for_harness(&mut sink)
        .unwrap_or_else(|error| panic!("{case_id}: the production emit completes: {error}"));

    assert_eq!(
        actual_diagnostics(&reported),
        expected_diagnostics(&case["typescript_observation"]["reported_diagnostics"]),
        "{case_id}: exact reported diagnostic tuples"
    );
    assert_eq!(
        actual_diagnostics(outcome.diagnostics()),
        expected_diagnostics(&case["typescript_observation"]["emit_result"]["diagnostics"]),
        "{case_id}: exact emit diagnostic tuples"
    );
    assert_eq!(
        outcome.emit_skipped(),
        case["typescript_observation"]["emit_result"]["emit_skipped"]
            .as_bool()
            .expect("frozen emitSkipped"),
        "{case_id}: emitSkipped"
    );

    let expected_writes = case["typescript_observation"]["writes"]
        .as_array()
        .expect("frozen writes");
    assert_eq!(
        sink.writes()
            .iter()
            .map(|write| write.path().to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        expected_writes
            .iter()
            .map(|expected| expected["path"]
                .as_str()
                .expect("frozen write path")
                .to_owned())
            .collect::<Vec<_>>(),
        "{case_id}: the ordered write paths"
    );
    for (write, expected) in sink.writes().iter().zip(expected_writes) {
        assert_eq!(
            artifact_kind(write.kind(), write.path()),
            expected["kind"].as_str().expect("frozen write kind"),
            "{case_id}: write kind for {}",
            write.path().display()
        );
        assert_eq!(
            String::from_utf8_lossy(write.callback_bytes()),
            String::from_utf8_lossy(&decode(&expected["callback_utf8_base64"])),
            "{case_id}: callback bytes for {}",
            write.path().display()
        );
        assert_eq!(
            write.write_byte_order_mark(),
            expected["write_byte_order_mark"]
                .as_bool()
                .expect("frozen BOM flag"),
            "{case_id}: BOM flag for {}",
            write.path().display()
        );
        assert_eq!(
            String::from_utf8_lossy(write.materialized_bytes().as_ref()),
            String::from_utf8_lossy(&decode(&expected["materialized_utf8_base64"])),
            "{case_id}: materialized bytes for {}",
            write.path().display()
        );
        let actual_sources = write.source_files().map(|sources| {
            sources
                .iter()
                .map(|source| source.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        });
        let expected_sources = expected["source_files"].as_array().map(|sources| {
            sources
                .iter()
                .map(|source| source.as_str().expect("source file path").to_owned())
                .collect::<Vec<_>>()
        });
        assert_eq!(actual_sources, expected_sources, "{case_id}: source files");
        assert_eq!(
            write.metadata().is_some(),
            expected["data_present"]
                .as_bool()
                .expect("data-present flag"),
            "{case_id}: callback data presence"
        );
        let actual_data_diagnostics = match write.metadata() {
            Some(EmitWriteMetadata::Text(metadata)) => actual_diagnostics(metadata.diagnostics()),
            _ => Vec::new(),
        };
        assert_eq!(
            actual_data_diagnostics,
            expected_diagnostics(&expected["data_diagnostics"]),
            "{case_id}: callback diagnostics"
        );
    }
}

/// Run every row of a class and report EVERY failing row's first assertion
/// (a class-wide port is judged on all of its rows at once).
fn assert_every_row(case_ids: &[&str], prepare: impl Fn(&str, &Value) -> PreparedProgram) {
    let mut failures = Vec::new();
    for case_id in case_ids {
        let case = frozen_case(case_id);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let prepared = prepare(case_id, &case);
            assert_frozen_observation(case_id, &case, prepared);
        }));
        if let Err(payload) = result {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_else(|| "non-string panic".to_owned());
            failures.push(format!("{case_id}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} rows diverge:\n{}",
        failures.len(),
        case_ids.len(),
        failures.join("\n---\n")
    );
}

#[test]
fn directory_symlinks_publish_every_descendant_under_the_link_spelling() {
    assert_every_row(DIRECTORY_SYMLINK_CASES, |_, case| {
        prepared_symlink_row(case)
    });
}

#[test]
fn the_suite_vfs_builder_expands_directory_links_like_the_runner() {
    assert_every_row(DIRECTORY_SYMLINK_CASES, |case_id, case| {
        assert_eq!(
            case["execution_route"], "recorded-compiler-plan",
            "{case_id}"
        );
        prepared_recorded_compiler_row(case_id)
    });
}

#[test]
fn project_runs_keep_their_config_identity_for_the_ts5055_hint_condition() {
    assert_every_row(PROJECT_CONFIG_IDENTITY_CASES, |case_id, _| {
        prepared_project_row(case_id)
    });
}
