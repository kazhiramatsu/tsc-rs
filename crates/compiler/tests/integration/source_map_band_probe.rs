//! H2.6a/H2.6c source-map band burn-down probe.
//!
//! The probe is intentionally ignored: select one row with
//! TSRS_H2_6A_PROBE_CASE or TSRS_H2_6C_PROBE_CASE and run
//! cargo test -p tsc-rs-compiler --test contracts source_map_band_probe -- --ignored.
//! The 6c selector mirrors every frozen H2.6c execution route exactly as the
//! acceptance runner prepares it: recorded compiler plans and conformance
//! qualified-vfs rows carry the complete map-family option floor, and project
//! rows use the hermetic project mount with the acceptance's structural-key
//! filtering (W5 extension; the option layer is the descriptor projection).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tsc_compiler::{EmitArtifact, EmitWriteMetadata, MemoryOutputSink, ProgramSession};

use tsc_harness::upstream_suites::execution::{
    load_compiler_emit_with_option_floor, load_project_emit,
    load_qualified_compiler_emit_with_option_floor, load_recorded_execution_plans, EmitOptionFloor,
    UpstreamExecutionCorpus, UpstreamExecutionInput,
};
use tsc_program::ProgramLoadLimits;

const H2_6A_SELECTOR: &str = "TSRS_H2_6A_PROBE_CASE";
const H2_6C_SELECTOR: &str = "TSRS_H2_6C_PROBE_CASE";
const H2_6C_OUT: &str = "TSRS_H2_6C_PROBE_OUT";
/// W5 census mode: a file listing one exact 6c case id per line (# comments
/// allowed). Each case is prepared on its frozen route and compared to the
/// frozen observation with a PATH-JOINED write comparison (the acceptance
/// ratchet counter is positional by design; the census separates true byte
/// divergences from declaration-absence sequence shifts). One JSON line per
/// case is appended to census.jsonl under TSRS_H2_6C_PROBE_OUT.
const H2_6C_LIST: &str = "TSRS_H2_6C_PROBE_LIST";

#[test]
#[ignore = "burn-down probe; select a row with TSRS_H2_6A_PROBE_CASE or TSRS_H2_6C_PROBE_CASE"]
fn probe_one_band_row() {
    let six_a = std::env::var_os(H2_6A_SELECTOR);
    let six_c = std::env::var_os(H2_6C_SELECTOR);
    let six_c_list = std::env::var_os(H2_6C_LIST);
    if [six_a.is_some(), six_c.is_some(), six_c_list.is_some()]
        .into_iter()
        .filter(|set| *set)
        .count()
        > 1
    {
        panic!("set only one of {H2_6A_SELECTOR}, {H2_6C_SELECTOR}, and {H2_6C_LIST}");
    }

    match (six_a, six_c, six_c_list) {
        (Some(selector), None, None) => run_h2_6a_probe(&selector_text(H2_6A_SELECTOR, selector)),
        (None, Some(selector), None) => run_h2_6c_probe(&selector_text(H2_6C_SELECTOR, selector)),
        (None, None, Some(list)) => run_h2_6c_census(&selector_text(H2_6C_LIST, list)),
        (None, None, None) => panic!("set {H2_6A_SELECTOR}, {H2_6C_SELECTOR}, or {H2_6C_LIST}"),
        _ => unreachable!("selector conflict was checked above"),
    }
}

fn selector_text(name: &str, selector: std::ffi::OsString) -> String {
    selector
        .into_string()
        .unwrap_or_else(|_| panic!("{name} must be valid UTF-8"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn sha256(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn read_artifact(relative_path: &str) -> Value {
    serde_json::from_slice(
        &std::fs::read(workspace_root().join(relative_path))
            .unwrap_or_else(|error| panic!("read {relative_path}: {error}")),
    )
    .unwrap_or_else(|error| panic!("parse {relative_path}: {error}"))
}

/// Prefer an exact case-id match. A non-exact selector remains useful for the
/// old 6a workflow, but it is accepted only when it identifies one case.
fn select_case<'a>(artifact: &'a Value, selector: &str, label: &str) -> &'a Value {
    let cases = artifact["cases"]
        .as_array()
        .unwrap_or_else(|| panic!("{label} artifact cases are not an array"));
    let exact_matches = cases
        .iter()
        .filter(|case| case["case_id"].as_str() == Some(selector))
        .collect::<Vec<_>>();
    match exact_matches.as_slice() {
        [case] => return case,
        [] => {}
        matches => panic!(
            "{label} selector {selector:?} matches {} exact case ids",
            matches.len()
        ),
    }

    let matches = cases
        .iter()
        .filter(|case| {
            case["case_id"]
                .as_str()
                .is_some_and(|case_id| case_id.contains(selector))
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [case] => case,
        [] => panic!("{label} selector {selector:?} matches no case id"),
        matches => panic!(
            "{label} selector {selector:?} matches {} case ids; use an exact case id",
            matches.len()
        ),
    }
}

fn case_id(case: &Value) -> &str {
    case["case_id"].as_str().expect("case id")
}

// ---- H2.6a compatibility mode ------------------------------------------------

fn run_h2_6a_probe(selector: &str) {
    let artifact = read_artifact("ratchets/h2-6a-qualification.v1.json");
    let case = select_case(&artifact, selector, "H2.6a");
    let case_id = case_id(case);
    let input = &case["input"];
    let current_directory = input["current_directory"].as_str().expect("cwd");
    let mut files = Vec::new();
    for file in input["files"].as_array().expect("files") {
        files.push((
            PathBuf::from(file["path"].as_str().expect("path")),
            base64::engine::general_purpose::STANDARD
                .decode(file["utf8_base64"].as_str().expect("bytes"))
                .expect("base64"),
        ));
    }
    if let Some(config) = input["virtual_config"].as_object() {
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            base64::engine::general_purpose::STANDARD
                .decode(config["utf8_base64"].as_str().expect("config bytes"))
                .expect("config base64"),
        ));
    }
    let roots: Vec<PathBuf> = input["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root")))
        .collect();
    let settings: Vec<(String, String)> = input["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("name").to_owned(),
                setting["value"].as_str().expect("value").to_owned(),
            )
        })
        .collect();
    let prepared = load_qualified_compiler_emit_with_option_floor(
        &workspace_root(),
        current_directory,
        &files,
        &roots,
        &settings,
        limits(),
        EmitOptionFloor::SourceMap,
    )
    .expect("prepare 6a probe program");
    let mut sink = MemoryOutputSink::new();
    let outcome = ProgramSession::new(prepared).emit(&mut sink);
    let out_dir = PathBuf::from("/tmp/h2-6a-probe");
    std::fs::create_dir_all(&out_dir).expect("6a probe out dir");
    let slug = case_id.replace(['/', '#'], "_");
    for write in sink.writes() {
        let name = write
            .path()
            .file_name()
            .expect("write basename")
            .to_string_lossy()
            .into_owned();
        std::fs::write(
            out_dir.join(format!("{slug}.produced.{name}")),
            write.callback_text(),
        )
        .expect("dump 6a produced");
    }
    for frozen in case["typescript_observation"]["writes"]
        .as_array()
        .expect("frozen writes")
    {
        let path = frozen["path"].as_str().expect("frozen path");
        let name = path.rsplit('/').next().expect("frozen basename");
        std::fs::write(
            out_dir.join(format!("{slug}.frozen.{name}")),
            base64::engine::general_purpose::STANDARD
                .decode(
                    frozen["callback_utf8_base64"]
                        .as_str()
                        .expect("frozen bytes"),
                )
                .expect("dump 6a frozen base64"),
        )
        .expect("dump 6a frozen");
    }
    println!(
        "H2.6a probe {case_id}: outcome={:?}, dumps under {}",
        outcome.as_ref().map(|_| "ok"),
        out_dir.display()
    );
}

// ---- H2.6c recorded-plan probe ----------------------------------------------

fn prepare_h2_6c_compiler_case(
    workspace: &Path,
    case: &Value,
    corpus: &UpstreamExecutionCorpus,
) -> tsc_program::PreparedProgram {
    let case_id = case_id(case);
    assert_eq!(
        case["suite"], "compiler",
        "{case_id}: recorded-compiler-plan route carries a non-compiler suite"
    );
    assert_eq!(
        case["execution_route"], "recorded-compiler-plan",
        "{case_id}: unexpected 6c compiler execution route"
    );

    let recorded = corpus
        .plans
        .iter()
        .find(|recorded| recorded.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("{case_id}: recorded compiler plan is absent"));
    assert_eq!(
        case["expansion_case"].as_u64(),
        Some(u64::from(recorded.provenance.case_index)),
        "{case_id}: recorded compiler-plan provenance differs"
    );
    let UpstreamExecutionInput::Compiler(plan) = &recorded.input else {
        panic!("{case_id}: recorded plan is not a compiler plan");
    };
    load_compiler_emit_with_option_floor(workspace, plan, limits(), EmitOptionFloor::MapFamily)
        .unwrap_or_else(|error| panic!("{case_id}: prepare 6c compiler plan: {error}"))
}

/// Conformance rows run the same qualified-vfs construction as the H2.6c
/// acceptance's `case_input_with_floor`, on the complete map-family floor.
fn prepare_h2_6c_qualified_case(workspace: &Path, case: &Value) -> tsc_program::PreparedProgram {
    let case_id = case_id(case);
    let input = &case["input"];
    let current_directory = input["current_directory"].as_str().expect("cwd");
    let mut files = Vec::new();
    for file in input["files"].as_array().expect("files") {
        files.push((
            PathBuf::from(file["path"].as_str().expect("path")),
            base64::engine::general_purpose::STANDARD
                .decode(file["utf8_base64"].as_str().expect("bytes"))
                .expect("base64"),
        ));
    }
    if let Some(config) = input["virtual_config"].as_object() {
        files.push((
            PathBuf::from(config["path"].as_str().expect("config path")),
            base64::engine::general_purpose::STANDARD
                .decode(config["utf8_base64"].as_str().expect("config bytes"))
                .expect("config base64"),
        ));
    }
    let roots: Vec<PathBuf> = input["roots"]
        .as_array()
        .expect("roots")
        .iter()
        .map(|root| PathBuf::from(root.as_str().expect("root")))
        .collect();
    let settings: Vec<(String, String)> = input["settings"]
        .as_array()
        .expect("settings")
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().expect("name").to_owned(),
                setting["value"].as_str().expect("value").to_owned(),
            )
        })
        .collect();
    load_qualified_compiler_emit_with_option_floor(
        workspace,
        current_directory,
        &files,
        &roots,
        &settings,
        limits(),
        EmitOptionFloor::MapFamily,
    )
    .unwrap_or_else(|error| panic!("{case_id}: prepare 6c qualified-vfs case: {error}"))
}

/// Project rows mirror the acceptance's hermetic mount: the recorded project
/// plan with the oracle's PROJECT_STRUCTURAL_KEYS runner controls removed
/// from a case-local clone before `load_project_emit`.
fn prepare_h2_6c_project_case(
    workspace: &Path,
    case: &Value,
    corpus: &UpstreamExecutionCorpus,
) -> tsc_program::PreparedProgram {
    let case_id = case_id(case);
    assert_eq!(
        case["suite"], "project",
        "{case_id}: project-mount route carries a non-project suite"
    );
    let recorded = corpus
        .plans
        .iter()
        .find(|recorded| recorded.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("{case_id}: recorded project plan is absent"));
    let UpstreamExecutionInput::Project(plan) = &recorded.input else {
        panic!("{case_id}: recorded plan is not a project plan");
    };
    let mut project_plan = plan.clone();
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
    load_project_emit(workspace, &project_plan, limits())
        .unwrap_or_else(|error| panic!("{case_id}: prepare 6c project mount: {error}"))
        .prepared_program
}

fn prepare_h2_6c_route(
    workspace: &Path,
    case: &Value,
    corpus: &UpstreamExecutionCorpus,
) -> tsc_program::PreparedProgram {
    match case["execution_route"].as_str().expect("execution route") {
        "recorded-compiler-plan" => prepare_h2_6c_compiler_case(workspace, case, corpus),
        "qualified-vfs" => prepare_h2_6c_qualified_case(workspace, case),
        "project-mount" => prepare_h2_6c_project_case(workspace, case, corpus),
        other => panic!("{}: unsupported 6c probe route {other}", case_id(case)),
    }
}

fn run_h2_6c_probe(selector: &str) {
    let artifact = read_artifact("ratchets/h2-6c-qualification.v1.json");
    let case = select_case(&artifact, selector, "H2.6c");
    let case_id = case_id(case);
    let workspace = workspace_root();
    let route = case["execution_route"].as_str().expect("execution route");
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded execution plans");
    let first_program = prepare_h2_6c_route(&workspace, case, &corpus);
    let second_program = first_program.clone();

    // Keep the exact two-run harness route used by run_h2_6c, including its
    // bounded library bundle and reported-diagnostics wrapper.
    let first_session = ProgramSession::new(first_program);
    let harness_lib_bundle = first_session
        .prepare_harness_lib_bundle()
        .unwrap_or_else(|error| panic!("{case_id}: prepare harness library bundle: {error}"));
    let mut first_sink = MemoryOutputSink::new();
    let (first_outcome, first_reported) = first_session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut first_sink,
            harness_lib_bundle.as_ref(),
        )
        .unwrap_or_else(|error| panic!("{case_id}: first Rust emit: {error}"));

    let mut second_sink = MemoryOutputSink::new();
    let (second_outcome, second_reported) = ProgramSession::new(second_program)
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut second_sink,
            harness_lib_bundle.as_ref(),
        )
        .unwrap_or_else(|error| panic!("{case_id}: second Rust emit: {error}"));
    assert_eq!(first_outcome, second_outcome, "{case_id}: repeated outcome");
    assert_eq!(first_sink, second_sink, "{case_id}: repeated callbacks");
    assert_eq!(
        first_reported, second_reported,
        "{case_id}: repeated reported diagnostics"
    );

    let out_dir = std::env::var_os(H2_6C_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target/h2-6c-probe"));
    std::fs::create_dir_all(&out_dir).expect("6c probe out dir");
    let manifest = dump_h2_6c(&out_dir, case, &first_outcome, &first_reported, &first_sink);
    let rendered = format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest).expect("render 6c probe manifest")
    );
    std::fs::write(out_dir.join("manifest.json"), rendered).expect("write 6c probe manifest");
    let floor_label = if route == "project-mount" {
        "descriptor-projection"
    } else {
        "MapFamily"
    };
    println!(
        "H2.6c probe {case_id}: route={route} floor={floor_label} writes={} reported_diagnostics={} dumps under {}",
        first_sink.writes().len(),
        first_reported.len(),
        out_dir.display()
    );
}

/// W5 census: path-joined write comparison for every listed case, one JSON
/// line per case. Single emission per case (the gating acceptance already
/// enforces double-run determinism); no per-write byte dumps.
fn run_h2_6c_census(list_path: &str) {
    let artifact = read_artifact("ratchets/h2-6c-qualification.v1.json");
    let workspace = workspace_root();
    let corpus = load_recorded_execution_plans(&workspace).expect("recorded execution plans");
    let out_dir = std::env::var_os(H2_6C_OUT)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace.join("target/h2-6c-probe"));
    std::fs::create_dir_all(&out_dir).expect("6c census out dir");
    let list = std::fs::read_to_string(list_path)
        .unwrap_or_else(|error| panic!("read {list_path}: {error}"));
    let mut lines = Vec::new();
    let mut done = 0usize;
    for raw in list.lines() {
        let selector = raw.trim();
        if selector.is_empty() || selector.starts_with('#') {
            continue;
        }
        let case = select_case(&artifact, selector, "H2.6c census");
        lines.push(census_h2_6c_case(&workspace, case, &corpus));
        done += 1;
        if done.is_multiple_of(10) {
            eprintln!("H2.6c census: {done} cases done");
        }
    }
    let mut rendered = String::new();
    for line in &lines {
        rendered.push_str(&serde_json::to_string(line).expect("render census line"));
        rendered.push('\n');
    }
    std::fs::write(out_dir.join("census.jsonl"), rendered).expect("write census.jsonl");
    println!(
        "H2.6c census: {done} cases -> {}",
        out_dir.join("census.jsonl").display()
    );
}

fn census_h2_6c_case(workspace: &Path, case: &Value, corpus: &UpstreamExecutionCorpus) -> Value {
    let case_id = case_id(case);
    let program = prepare_h2_6c_route(workspace, case, corpus);
    let session = ProgramSession::new(program);
    let harness_lib_bundle = session
        .prepare_harness_lib_bundle()
        .unwrap_or_else(|error| panic!("{case_id}: prepare harness library bundle: {error}"));
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(
            &mut sink,
            harness_lib_bundle.as_ref(),
        )
        .unwrap_or_else(|error| panic!("{case_id}: Rust emit: {error}"));

    let expected_writes = case["typescript_observation"]["writes"]
        .as_array()
        .expect("6c expected writes");
    let mut expected_by_path: std::collections::BTreeMap<&str, &Value> = expected_writes
        .iter()
        .map(|write| (write["path"].as_str().expect("expected write path"), write))
        .collect();
    assert_eq!(
        expected_by_path.len(),
        expected_writes.len(),
        "{case_id}: expected write paths are not unique"
    );

    let mut exact = 0u64;
    let mut diverging = Vec::new();
    let mut rust_only = Vec::new();
    for write in sink.writes() {
        let path = write.path().to_string_lossy().into_owned();
        match expected_by_path.remove(path.as_str()) {
            None => rust_only.push(path),
            Some(expected) => {
                let bytes_match = expected["callback_utf8_sha256"].as_str()
                    == Some(sha256(write.callback_text().as_bytes()).as_str());
                let bom_match = expected["write_byte_order_mark"].as_bool()
                    == Some(write.write_byte_order_mark());
                if bytes_match && bom_match {
                    exact += 1;
                } else {
                    diverging.push(json!({
                        "path": path,
                        "kind": expected["kind"],
                        "bytes_equal": bytes_match,
                        "bom_equal": bom_match,
                    }));
                }
            }
        }
    }
    let missing: Vec<Value> = expected_by_path
        .values()
        .map(|write| json!({"path": write["path"], "kind": write["kind"]}))
        .collect();

    let expected_reported = case["typescript_observation"]["reported_diagnostics"]
        .as_array()
        .map_or(0, Vec::len);
    json!({
        "case_id": case_id,
        "route": case["execution_route"],
        "writes_exact": exact,
        "writes_diverging": diverging,
        "writes_missing": missing,
        "writes_rust_only": rust_only,
        "reported_diagnostics": {
            "rust": reported.len(),
            "expected": expected_reported,
            "rust_rendered": reported
                .iter()
                .map(|diagnostic| format!("{diagnostic:?}"))
                .collect::<Vec<_>>(),
        },
        "emit_skipped": outcome.emit_skipped(),
        "source_maps_count": outcome.source_maps().map_or(0, <[_]>::len),
    })
}

fn dump_h2_6c(
    out_dir: &Path,
    case: &Value,
    outcome: &tsc_compiler::EmitOutcome,
    reported_diagnostics: &[tsc_diagnostics::Diagnostic],
    sink: &MemoryOutputSink,
) -> Value {
    let case_id = case_id(case);
    let expected_writes = case["typescript_observation"]["writes"]
        .as_array()
        .expect("6c expected writes");
    let case_slug = case_file_slug(case_id);

    // Path-joined pairing (W5): a declaration-absence sequence shift must not
    // make the detail tables compare different paths. Expected rows keep
    // their frozen order; rust-only writes follow in sink order.
    let mut rust_by_path: std::collections::BTreeMap<String, &EmitArtifact> = Default::default();
    for write in sink.writes() {
        let path = write.path().to_string_lossy().into_owned();
        assert!(
            rust_by_path.insert(path, write).is_none(),
            "{case_id}: rust write paths are not unique"
        );
    }
    let expected_paths: std::collections::BTreeSet<&str> = expected_writes
        .iter()
        .map(|write| write["path"].as_str().expect("expected write path"))
        .collect();
    assert_eq!(
        expected_paths.len(),
        expected_writes.len(),
        "{case_id}: expected write paths are not unique"
    );
    let mut pairs: Vec<(Option<&EmitArtifact>, Option<&Value>)> = expected_writes
        .iter()
        .map(|expected| {
            let path = expected["path"].as_str().expect("expected write path");
            (rust_by_path.remove(path), Some(expected))
        })
        .collect();
    for write in sink.writes() {
        let path = write.path().to_string_lossy();
        if rust_by_path.remove(path.as_ref()).is_some() {
            pairs.push((Some(write), None));
        }
    }
    let mut manifest_writes = Vec::with_capacity(pairs.len());

    for (index, (actual, expected)) in pairs.into_iter().enumerate() {
        let actual_path = actual.map(|write| write.path().to_string_lossy().into_owned());
        let expected_path = expected
            .and_then(|write| write["path"].as_str())
            .map(str::to_owned);
        let path_for_slug = actual_path
            .as_deref()
            .or(expected_path.as_deref())
            .unwrap_or("missing-path");
        let prefix = format!("{case_slug}.write-{index:04}.{}", path_slug(path_for_slug));

        let actual_callback = actual.map(|write| write.callback_bytes().to_vec());
        let actual_materialized = actual.map(|write| write.materialized_bytes().into_owned());
        let expected_callback = expected
            .map(|write| decode_expected_bytes(write, "callback_utf8_base64", case_id, index));
        let expected_materialized = expected
            .map(|write| decode_expected_bytes(write, "materialized_utf8_base64", case_id, index));

        let actual_callback_sha = actual_callback.as_deref().map(sha256);
        let expected_callback_sha = expected_callback.as_deref().map(sha256);
        let actual_materialized_sha = actual_materialized.as_deref().map(sha256);
        let expected_materialized_sha = expected_materialized.as_deref().map(sha256);
        let actual_bom = actual.map(EmitArtifact::write_byte_order_mark);
        let expected_bom = expected.and_then(|write| write["write_byte_order_mark"].as_bool());

        let actual_callback_file = actual_callback.as_deref().map(|bytes| {
            let name = format!("{prefix}.rust.callback");
            write_dump_file(out_dir, &name, bytes);
            name
        });
        let actual_materialized_file = actual_materialized.as_deref().map(|bytes| {
            let name = format!("{prefix}.rust.materialized");
            write_dump_file(out_dir, &name, bytes);
            name
        });
        let expected_callback_file = expected_callback.as_deref().map(|bytes| {
            let name = format!("{prefix}.expected.callback");
            write_dump_file(out_dir, &name, bytes);
            name
        });
        let expected_materialized_file = expected_materialized.as_deref().map(|bytes| {
            let name = format!("{prefix}.expected.materialized");
            write_dump_file(out_dir, &name, bytes);
            name
        });

        let rust_map_bytes = actual_path
            .as_deref()
            .filter(|path| path.ends_with(".map"))
            .and(actual_callback.as_deref());
        let expected_map_bytes = expected_path
            .as_deref()
            .filter(|path| path.ends_with(".map"))
            .and(expected_callback.as_deref());
        let mapping_segments_file = if rust_map_bytes.is_some() || expected_map_bytes.is_some() {
            let name = format!("{prefix}.mapping-segments.txt");
            let table =
                render_mapping_table(case_id, path_for_slug, rust_map_bytes, expected_map_bytes);
            write_dump_file(out_dir, &name, table.as_bytes());
            Some(name)
        } else {
            None
        };

        let callback_equal = actual_callback_sha == expected_callback_sha;
        let materialized_equal = actual_materialized_sha == expected_materialized_sha;
        let bom_equal = actual_bom == expected_bom;
        let path_equal = actual_path == expected_path;
        let entry = json!({
            "write_index": index,
            "expected_index": expected.and_then(|write| write["index"].as_u64()),
            "path": actual_path.clone().or_else(|| expected_path.clone()),
            "rust": {
                "present": actual.is_some(),
                "path": actual_path,
                "callback_sha256": actual_callback_sha,
                "materialized_sha256": actual_materialized_sha,
                "write_byte_order_mark": actual_bom,
                "callback_file": actual_callback_file,
                "materialized_file": actual_materialized_file,
            },
            "expected": {
                "present": expected.is_some(),
                "path": expected_path,
                "callback_sha256": expected_callback_sha,
                "materialized_sha256": expected_materialized_sha,
                "write_byte_order_mark": expected_bom,
                "callback_file": expected_callback_file,
                "materialized_file": expected_materialized_file,
                "declared_callback_sha256": expected.map(|write| write["callback_utf8_sha256"].clone()),
                "declared_materialized_sha256": expected.map(|write| write["materialized_utf8_sha256"].clone()),
            },
            "source_provenance": {
                "rust": actual.map(|write| paths_json(write.source_files())).unwrap_or(Value::Null),
                "expected": expected.map(|write| write["source_files"].clone()).unwrap_or(Value::Null),
            },
            "callback_metadata": {
                "rust": actual.map(rust_callback_metadata).unwrap_or(Value::Null),
                "expected": expected.map(expected_callback_metadata).unwrap_or(Value::Null),
            },
            "mapping_segments_file": mapping_segments_file,
            "facets": {
                "path": facet(path_equal, actual_path.clone(), expected_path.clone()),
                "callback_sha256": facet(callback_equal, actual_callback_sha, expected_callback_sha),
                "materialized_sha256": facet(materialized_equal, actual_materialized_sha, expected_materialized_sha),
                "bom": facet(bom_equal, actual_bom, expected_bom),
            },
        });
        println!(
            "H2.6c write {index} {path_for_slug}: callback_sha256={} materialized_sha256={} bom={}",
            verdict(callback_equal),
            verdict(materialized_equal),
            verdict(bom_equal)
        );
        manifest_writes.push(entry);
    }

    json!({
        "schema": 1,
        "probe": "h2-6c-source-map-band",
        "case_id": case_id,
        "execution_route": case["execution_route"],
        "emit_option_floor": if case["execution_route"] == "project-mount" {
            "descriptor-projection"
        } else {
            "MapFamily"
        },
        "source_provenance": case["source"],
        "rust": {
            "repetitions": 2,
            "emit_skipped": outcome.emit_skipped(),
            "emit_diagnostics_count": outcome.diagnostics().len(),
            "reported_diagnostics_count": reported_diagnostics.len(),
            "source_maps_count": outcome.source_maps().map_or(0, <[_]>::len),
            "reported_diagnostics": reported_diagnostics
                .iter()
                .map(|diagnostic| format!("{diagnostic:?}"))
                .collect::<Vec<_>>(),
        },
        "expected": {
            "reported_diagnostics": case["typescript_observation"]["reported_diagnostics"].clone(),
            "emit_refused": case["typescript_observation"]["emit_refused"].clone(),
            "emit_result": case["typescript_observation"]["emit_result"].clone(),
        },
        "writes": manifest_writes,
    })
}

fn decode_expected_bytes(write: &Value, field: &str, case_id: &str, index: usize) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(
            write[field]
                .as_str()
                .unwrap_or_else(|| panic!("{case_id} write {index}: missing {field}")),
        )
        .unwrap_or_else(|error| panic!("{case_id} write {index}: invalid {field}: {error}"))
}

fn write_dump_file(out_dir: &Path, name: &str, bytes: &[u8]) {
    std::fs::write(out_dir.join(name), bytes).unwrap_or_else(|error| {
        panic!("write probe dump {}: {error}", out_dir.join(name).display())
    });
}

fn path_slug(value: &str) -> String {
    let slug = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if slug.is_empty() {
        "path".to_owned()
    } else {
        slug
    }
}

fn case_file_slug(case_id: &str) -> String {
    let slug = path_slug(case_id);
    if slug.len() <= 96 {
        return slug;
    }
    format!("{}-{}", &slug[..80], &sha256(case_id.as_bytes())[..12])
}

fn facet(equal: bool, rust: impl Into<Value>, expected: impl Into<Value>) -> Value {
    json!({
        "equal": equal,
        "verdict": verdict(equal),
        "rust": rust.into(),
        "expected": expected.into(),
    })
}

fn verdict(equal: bool) -> &'static str {
    if equal {
        "equal"
    } else {
        "diverging"
    }
}

fn paths_json(paths: Option<&[PathBuf]>) -> Value {
    paths.map_or(Value::Null, |paths| {
        Value::Array(
            paths
                .iter()
                .map(|path| Value::String(path.to_string_lossy().into_owned()))
                .collect(),
        )
    })
}

fn rust_callback_metadata(write: &EmitArtifact) -> Value {
    let metadata = match write.metadata() {
        None => json!({ "present": false }),
        Some(EmitWriteMetadata::Text(metadata)) => json!({
            "present": true,
            "kind": "text",
            "diagnostics_count": metadata.diagnostics().len(),
            "source_map_url_position_utf16": metadata.source_map_url_position().map(|position| position.value()),
        }),
        Some(EmitWriteMetadata::BuildInfo(metadata)) => json!({
            "present": true,
            "kind": "build-info",
            "schema_version": metadata.schema_version(),
        }),
    };
    json!({
        "artifact_kind": format!("{:?}", write.kind()),
        "metadata": metadata,
    })
}

fn expected_callback_metadata(write: &Value) -> Value {
    json!({
        "kind": write["kind"],
        "on_error_callback_present": write["on_error_callback_present"],
        "data_present": write["data_present"],
        "data_source_map_url_pos": write["data_source_map_url_pos"],
        "data_diagnostics_count": write["data_diagnostics_count"],
        "inline_source_map_payload": write["inline_source_map_payload"],
    })
}

// ---- Source-map VLQ table ----------------------------------------------------

#[derive(Clone, Debug)]
struct DecodedSegment {
    generated_column: i64,
    source_index: Option<i64>,
    original_line: Option<i64>,
    original_column: Option<i64>,
    name_index: Option<i64>,
}

fn render_mapping_table(
    case_id: &str,
    path: &str,
    rust_bytes: Option<&[u8]>,
    expected_bytes: Option<&[u8]>,
) -> String {
    let rust_segments = rust_bytes
        .map(decode_source_map_segments)
        .transpose()
        .unwrap_or_else(|error| panic!("{case_id} {path}: decode Rust source map: {error}"));
    let expected_segments = expected_bytes
        .map(decode_source_map_segments)
        .transpose()
        .unwrap_or_else(|error| panic!("{case_id} {path}: decode expected source map: {error}"));
    let line_count = rust_segments
        .as_ref()
        .map_or(0, Vec::len)
        .max(expected_segments.as_ref().map_or(0, Vec::len));
    let mut table = String::new();
    writeln!(table, "# decoded source-map segments").expect("write mapping table header");
    writeln!(table, "# case_id: {case_id}").expect("write mapping table case");
    writeln!(table, "# path: {path}").expect("write mapping table path");
    writeln!(
        table,
        "# tuple: (generated_column[, source_index, original_line, original_column[, name_index]])"
    )
    .expect("write mapping table tuple description");
    writeln!(
        table,
        "generated_line_0_based\trust_segments\texpected_segments\tverdict"
    )
    .expect("write mapping table columns");
    for line in 0..line_count {
        let rust_line = rust_segments
            .as_ref()
            .and_then(|segments| segments.get(line))
            .map_or_else(
                || "<absent>".to_owned(),
                |segments| render_segments(segments),
            );
        let expected_line = expected_segments
            .as_ref()
            .and_then(|segments| segments.get(line))
            .map_or_else(
                || "<absent>".to_owned(),
                |segments| render_segments(segments),
            );
        writeln!(
            table,
            "{line}\t{rust_line}\t{expected_line}\t{}",
            verdict(rust_line == expected_line)
        )
        .expect("write mapping table row");
    }
    table
}

fn render_segments(segments: &[DecodedSegment]) -> String {
    let rendered = segments
        .iter()
        .map(|segment| {
            let Some(source_index) = segment.source_index else {
                return format!("({})", segment.generated_column);
            };
            let name = segment
                .name_index
                .map_or_else(String::new, |name| format!(", {name}"));
            format!(
                "({}, {}, {}, {}{})",
                segment.generated_column,
                source_index,
                segment.original_line.expect("mapped original line"),
                segment.original_column.expect("mapped original column"),
                name
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", rendered.join(", "))
}

fn decode_source_map_segments(bytes: &[u8]) -> Result<Vec<Vec<DecodedSegment>>, String> {
    let map: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let mappings = map["mappings"]
        .as_str()
        .ok_or_else(|| "source map mappings is not a string".to_owned())?;
    decode_mappings(mappings)
}

fn decode_mappings(mappings: &str) -> Result<Vec<Vec<DecodedSegment>>, String> {
    let mut source_index = 0;
    let mut original_line = 0;
    let mut original_column = 0;
    let mut name_index = 0;
    let mut lines = Vec::new();
    for line in mappings.split(';') {
        let mut generated_column = 0;
        let mut segments = Vec::new();
        if !line.is_empty() {
            for encoded_segment in line.split(',') {
                if encoded_segment.is_empty() {
                    return Err("empty mapping segment".to_owned());
                }
                let fields = decode_vlq_fields(encoded_segment)?;
                generated_column += fields[0];
                let (source, original, column, name) = match fields.len() {
                    1 => (None, None, None, None),
                    4 | 5 => {
                        source_index += fields[1];
                        original_line += fields[2];
                        original_column += fields[3];
                        if fields.len() == 5 {
                            name_index += fields[4];
                        }
                        (
                            Some(source_index),
                            Some(original_line),
                            Some(original_column),
                            (fields.len() == 5).then_some(name_index),
                        )
                    }
                    length => {
                        return Err(format!(
                            "mapping segment has {length} fields; expected 1, 4, or 5"
                        ));
                    }
                };
                segments.push(DecodedSegment {
                    generated_column,
                    source_index: source,
                    original_line: original,
                    original_column: column,
                    name_index: name,
                });
            }
        }
        lines.push(segments);
    }
    Ok(lines)
}

fn decode_vlq_fields(encoded: &str) -> Result<Vec<i64>, String> {
    let mut fields = Vec::new();
    let mut value = 0i64;
    let mut shift = 0u32;
    let mut terminated = false;
    for byte in encoded.bytes() {
        let digit = base64_value(byte)
            .ok_or_else(|| format!("invalid source-map base64 byte 0x{byte:02x} in {encoded:?}"))?;
        let continuation = digit & 0x20 != 0;
        let digit = i64::from(digit & 0x1f);
        if shift >= 63 {
            return Err(format!("source-map VLQ is too wide in {encoded:?}"));
        }
        value |= digit << shift;
        shift += 5;
        if !continuation {
            let signed = if value & 1 == 1 {
                -(value >> 1)
            } else {
                value >> 1
            };
            fields.push(signed);
            value = 0;
            shift = 0;
            terminated = true;
        }
    }
    if !terminated || shift != 0 {
        return Err(format!("unterminated source-map VLQ in {encoded:?}"));
    }
    Ok(fields)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
