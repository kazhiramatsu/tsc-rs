use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_compiler::{
    DriverError, EmitArtifact, EmitFailure, EmitWriteDisposition, H2RuntimeSlice, MemoryOutputSink,
    OutputSink, ProgramSession,
};
use tsc_emitter::{TransformError, UnsupportedTransformFeature};
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit, load_recorded_execution_plans, CompilerExecutionPlan,
    UpstreamExecutionInput,
};
use tsc_program::{
    CompilerOptions, PathContext, PreparedProgram, PreparedSourceFile, ProgramLoadLimits,
    ProgramOptions, ProgramPath,
};

const QUALIFICATION_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-emit-qualification.v1.json"
));
const CALLBACK_ORACLE_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-emit-oracle.v1.json"
));
const CASE_ID: &str = "typescript-6.0.3/compiler/esmNoSynthesizedDefault.ts#module%3Dpreserve";

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct CountingSink {
    writes: usize,
}

impl OutputSink for CountingSink {
    fn write(
        &mut self,
        _artifact: EmitArtifact,
    ) -> Result<EmitWriteDisposition, tsc_compiler::EmitIoError> {
        self.writes += 1;
        Ok(EmitWriteDisposition::Written)
    }
}

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock follows Unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "tsc-rs-h1-qualification-{timestamp}-{sequence}-{}",
                std::process::id(),
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self { root },
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create H1 qualification tree: {error}"),
            }
        }
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove H1 qualification tree: {error}");
            }
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn qualification() -> Value {
    serde_json::from_slice(QUALIFICATION_BYTES).expect("H1 qualification is JSON")
}

fn callback_oracle() -> Value {
    serde_json::from_slice(CALLBACK_ORACLE_BYTES).expect("H1 callback oracle is JSON")
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("trusted qualification path")
}

fn option_bool(options: &Value, name: &str) -> Option<bool> {
    options.get(name).and_then(Value::as_bool)
}

fn option_i32(options: &Value, name: &str) -> Option<i32> {
    options
        .get(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn prepared_control(case: &Value, library: &Value) -> PreparedProgram {
    let options = &case["input"]["compiler_options"];
    let mut builder = PreparedProgram::emitting_builder(
        PathContext::new(path("/project"), true),
        CompilerOptions {
            target: option_i32(options, "target"),
            module: option_i32(options, "module"),
            use_define_for_class_fields: option_bool(options, "useDefineForClassFields"),
            list_emitted_files: option_bool(options, "listEmittedFiles"),
            emit_bom: option_bool(options, "emitBOM"),
            no_emit_on_error: option_bool(options, "noEmitOnError"),
            new_line: option_i32(options, "newLine"),
            jsx: option_i32(options, "jsx"),
            source_map: option_bool(options, "sourceMap"),
            declaration: option_bool(options, "declaration"),
            ..CompilerOptions::default()
        },
    );
    if let Some(no_lib) = option_bool(options, "noLib") {
        builder.set_program_options(ProgramOptions::default().with_no_lib(no_lib));
    }
    for file in case["input"]["root_files"]
        .as_array()
        .expect("control root files")
    {
        let file_name = file["path"].as_str().expect("control source path");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(file["utf8_base64"].as_str().expect("control source bytes"))
            .expect("valid control source base64");
        let source = builder
            .add_source_file(PreparedSourceFile::new(
                path(file_name),
                String::from_utf8(bytes).expect("control source is UTF-8"),
            ))
            .expect("add control source");
        builder.add_root_file(source).expect("add control root");
    }
    let library_path = library["path"].as_str().expect("oracle library path");
    let library_bytes = base64::engine::general_purpose::STANDARD
        .decode(
            library["utf8_base64"]
                .as_str()
                .expect("oracle library bytes"),
        )
        .expect("valid oracle library base64");
    let source = builder
        .add_source_file(PreparedSourceFile::new(
            path(library_path),
            String::from_utf8(library_bytes).expect("oracle library is UTF-8"),
        ))
        .expect("add oracle library");
    builder
        .add_root_file(source)
        .expect("add oracle library root");
    builder.build().expect("build adjacent control program")
}

fn assert_control_failure(id: &str, expected: &Value, error: DriverError) {
    let kind = expected["kind"].as_str().expect("failure kind");
    match kind {
        "unsupported-compiler-option" => {
            let option = expected["option"].as_str().expect("failure option");
            assert_eq!(
                error,
                DriverError::Emit(EmitFailure::UnsupportedCompilerOption {
                    option: match option {
                        "jsx" => "jsx",
                        "sourceMap" => "sourceMap",
                        "declaration" => "declaration",
                        _ => panic!("{id}: unknown expected option {option}"),
                    },
                }),
                "{id}: typed option failure",
            );
        }
        "unsupported-source-extension" => match error {
            DriverError::Emit(EmitFailure::UnsupportedSourceExtension { path }) => {
                assert_eq!(
                    path.extension().and_then(|extension| extension.to_str()),
                    expected["extension"]
                        .as_str()
                        .expect("failure extension")
                        .strip_prefix('.'),
                    "{id}: rejected extension",
                );
            }
            other => panic!("{id}: expected source-extension failure, got {other:?}"),
        },
        "unsupported-transform-feature" => {
            let feature = match expected["feature"].as_str().expect("failure feature") {
                "runtime-enum" => UnsupportedTransformFeature::RuntimeEnums,
                "runtime-namespace" => UnsupportedTransformFeature::RuntimeNamespaces,
                "parameter-property" => UnsupportedTransformFeature::ParameterProperties,
                feature => panic!("{id}: unknown expected feature {feature}"),
            };
            match error {
                DriverError::Emit(EmitFailure::Transform(error)) => assert!(
                    matches!(
                        error.as_ref(),
                        TransformError::UnsupportedSyntax {
                            feature: actual,
                            ..
                        } if *actual == feature
                    ),
                    "{id}: typed transform failure was {error:?}",
                ),
                other => panic!("{id}: expected transform failure, got {other:?}"),
            }
        }
        other => panic!("{id}: unknown expected failure kind {other}"),
    }
}

fn candidate_plan(
    plans: &[tsc_harness::upstream_suites::execution::UpstreamExecutionPlan],
) -> CompilerExecutionPlan {
    let plan = plans
        .iter()
        .find(|plan| plan.provenance.case_id.as_ref() == CASE_ID)
        .expect("compatible candidate plan");
    match &plan.input {
        UpstreamExecutionInput::Compiler(plan) => plan.clone(),
        UpstreamExecutionInput::Project(_) => panic!("compatible candidate is a project case"),
    }
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(128, 1_024, 32, 8 * 1_024 * 1_024, 64 * 1_024 * 1_024)
}

fn emit_candidate(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
) -> Result<(tsc_compiler::EmitOutcome, MemoryOutputSink), String> {
    let prepared =
        load_compiler_emit(workspace, plan, limits()).map_err(|error| error.to_string())?;
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared)
        .emit(&mut sink)
        .map_err(|error| error.to_string())?;
    Ok((outcome, sink))
}

fn materialize_cli_projection(case: &Value, tree: &TempTree) {
    for file in case["virtual_files"]
        .as_array()
        .expect("qualification virtual files")
    {
        let relative = file["path"]
            .as_str()
            .expect("qualification virtual path")
            .strip_prefix('/')
            .expect("qualification virtual path is absolute");
        let output = tree.path(relative);
        fs::create_dir_all(output.parent().expect("virtual file has a parent"))
            .expect("create CLI projection parent");
        fs::write(
            output,
            base64::engine::general_purpose::STANDARD
                .decode(
                    file["utf8_base64"]
                        .as_str()
                        .expect("qualification virtual bytes"),
                )
                .expect("decode qualification virtual bytes"),
        )
        .expect("write CLI projection source");
    }
    fs::write(
        tree.path("tsconfig.json"),
        base64::engine::general_purpose::STANDARD
            .decode(
                case["cli_projection"]["config_utf8_base64"]
                    .as_str()
                    .expect("qualification CLI config"),
            )
            .expect("decode qualification CLI config"),
    )
    .expect("write qualification tsconfig");
}

fn expected_cli_stdout(case: &Value) -> String {
    let mut output = String::new();
    for diagnostic in case["observation"]["reported_diagnostics"]
        .as_array()
        .expect("reported diagnostics")
    {
        let file = diagnostic["file"]["value"]
            .as_str()
            .expect("diagnostic file")
            .strip_prefix('/')
            .expect("diagnostic file is absolute");
        let line = diagnostic["line"]["value"]
            .as_u64()
            .expect("diagnostic line")
            + 1;
        let column = diagnostic["column"]["value"]
            .as_u64()
            .expect("diagnostic column")
            + 1;
        let category = diagnostic["category"]
            .as_str()
            .expect("diagnostic category");
        let code = diagnostic["code"].as_u64().expect("diagnostic code");
        let text = diagnostic["chain"]["text"]
            .as_str()
            .expect("diagnostic text");
        output.push_str(&format!(
            "{file}({line},{column}): {category} TS{code}: {text}\n"
        ));
    }
    output
}

#[test]
fn frozen_adjacent_controls_remain_rejected_or_are_exactly_promoted() {
    let qualification = qualification();
    let oracle = callback_oracle();
    let controls = qualification["adjacent_controls"]
        .as_array()
        .expect("qualification controls");
    assert_eq!(controls.len(), 7, "frozen adjacent-control count");
    let oracle_cases = oracle["cases"].as_array().expect("callback oracle cases");
    for control in controls {
        let id = control["id"].as_str().expect("control id");
        let case = oracle_cases
            .iter()
            .find(|case| case["input"]["id"] == id)
            .unwrap_or_else(|| panic!("callback oracle is missing {id}"));
        assert_eq!(case["input"]["classification"], "adjacent-unsupported");
        if matches!(
            id,
            "mts-output-control"
                | "runtime-enum-control"
                | "runtime-namespace-control"
                | "parameter-property-control"
                | "jsx-control"
        ) {
            let mut sink = MemoryOutputSink::new();
            let outcome = ProgramSession::new(prepared_control(
                case,
                &oracle["oracle_environment"]["library"],
            ))
            .emit(&mut sink)
            .expect("later H2 slices promote the frozen adjacent control");
            let expected = &case["observation"]["writes"][0];
            assert_eq!(sink.writes().len(), 1, "{id}: exact promoted write count");
            assert_eq!(
                sink.writes()[0].path(),
                Path::new(expected["path"].as_str().expect("expected write path")),
                "{id}: exact promoted output path",
            );
            assert_eq!(
                sink.writes()[0].callback_text(),
                expected["callback_text"]
                    .as_str()
                    .expect("expected write text"),
                "{id}: exact promoted output text",
            );
            let owner = match id {
                "mts-output-control" => H2RuntimeSlice::H2_1e,
                "runtime-enum-control" => H2RuntimeSlice::H2_2a,
                "runtime-namespace-control" => H2RuntimeSlice::H2_2b,
                "parameter-property-control" => H2RuntimeSlice::H2_2c,
                "jsx-control" => H2RuntimeSlice::H2_3b,
                _ => unreachable!("promoted adjacent control"),
            };
            assert_eq!(
                outcome.h2_activity().runtime_slice(owner),
                1,
                "{id}: later H2 slice owns the promotion",
            );
            continue;
        }
        let mut sink = CountingSink::default();
        let error = ProgramSession::new(prepared_control(
            case,
            &oracle["oracle_environment"]["library"],
        ))
        .emit(&mut sink)
        .unwrap_err();
        assert_control_failure(id, &control["expected_rust_failure"], error);
        assert_eq!(sink.writes, 0, "{id}: no partial output");
        assert_eq!(control["expected_rust_sink_writes"], 0);
    }
}

#[test]
fn compatible_upstream_emit_is_job_count_independent() {
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("load upstream plans");
    let plan = candidate_plan(&corpus.plans);
    let (left, right) = std::thread::scope(|scope| {
        let left_workspace = workspace.clone();
        let left_plan = plan.clone();
        let left = scope.spawn(move || emit_candidate(&left_workspace, &left_plan));
        let right_workspace = workspace.clone();
        let right_plan = plan.clone();
        let right = scope.spawn(move || emit_candidate(&right_workspace, &right_plan));
        (
            left.join().expect("first emit worker did not panic"),
            right.join().expect("second emit worker did not panic"),
        )
    });
    let left = left.expect("first emit worker succeeded");
    let right = right.expect("second emit worker succeeded");
    assert_eq!(left, right, "two-worker output differs");

    let qualification = qualification();
    let expected = &qualification["compatible_cases"][0]["observation"]["writes"][0];
    assert_eq!(left.1.writes().len(), 1);
    assert_eq!(
        left.1.writes()[0].callback_text(),
        expected["callback_text"]
            .as_str()
            .expect("expected callback text"),
    );
}

#[test]
fn standalone_production_cli_needs_neither_node_nor_repository_vendor_lookup() {
    let qualification = qualification();
    let case = &qualification["compatible_cases"][0];
    let tree = TempTree::new();
    materialize_cli_projection(case, &tree);
    let empty_path = tree.path("empty-path");
    fs::create_dir(&empty_path).expect("create empty executable search path");

    let output = Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .current_dir(&tree.root)
        .env("PATH", &empty_path)
        .env_remove("NODE_PATH")
        .env_remove("NODE_OPTIONS")
        .args(["--pretty", "false", "-p", "tsconfig.json"])
        .output()
        .expect("run standalone H1 CLI");
    assert_eq!(
        output.status.code(),
        case["cli_projection"]["expected_exit_code"]
            .as_i64()
            .and_then(|code| i32::try_from(code).ok()),
        "standalone CLI exit",
    );
    let compiler_root = fs::canonicalize(&tree.root).expect("canonicalize standalone root");
    let normalized_stdout = String::from_utf8(output.stdout)
        .expect("standalone stdout is UTF-8")
        .replace(&*compiler_root.to_string_lossy(), "");
    assert_eq!(
        normalized_stdout,
        expected_cli_stdout(case),
        "standalone CLI diagnostics",
    );
    assert!(
        output.stderr.is_empty(),
        "standalone CLI stderr is not empty"
    );

    let expected = &case["observation"]["writes"][0];
    let actual_bytes = fs::read(tree.path("index.js")).expect("read standalone output");
    assert_eq!(
        actual_bytes,
        base64::engine::general_purpose::STANDARD
            .decode(
                expected["materialized_utf8_base64"]
                    .as_str()
                    .expect("expected standalone bytes"),
            )
            .expect("decode expected standalone bytes"),
    );
    assert_eq!(sha256(&actual_bytes), expected["materialized_utf8_sha256"]);
    assert!(
        !tree.path("vendor").exists(),
        "standalone tree acquired vendor data"
    );
}

#[test]
fn qualification_authority_hashes_are_current() {
    let workspace = workspace_root();
    let qualification = qualification();
    let mut records = vec![
        &qualification["generator"],
        &qualification["contract"],
        &qualification["resource_summary"]["no_emit_performance"]["artifact"],
    ];
    records.extend(
        qualification["authorities"]
            .as_object()
            .expect("qualification authorities")
            .values(),
    );
    records.extend(
        qualification["upstream_closure"]["suites"]
            .as_array()
            .expect("qualification suites")
            .iter()
            .map(|suite| &suite["classification"]),
    );
    let mut seen = BTreeMap::new();
    for record in records {
        let relative = record["path"].as_str().expect("authority path");
        let expected = record["sha256"].as_str().expect("authority hash");
        let actual = sha256(&fs::read(workspace.join(relative)).expect("read authority"));
        assert_eq!(actual, expected, "stale H1 authority {relative}");
        assert!(
            seen.insert(relative, expected).is_none(),
            "duplicate authority {relative}"
        );
    }
}
