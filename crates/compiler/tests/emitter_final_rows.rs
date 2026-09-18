//! H2.8a-A-RES-EMITTER-FINAL (`docs/design/greenfield/slices/emitter-final-batch/`):
//! focused replay of the guarded known rows of the H2.5h / H2.6a / H2.6c
//! profiles (EF2: 12 ES5 rows, EF3: 8 map/output rows, one of them also an
//! H2.6a row) against their frozen qualification observations, through the
//! same qualified-vfs / recorded-compiler-plan adapters and option floors as
//! the acceptance runner: two deterministic Rust runs per row comparing every
//! write (path, callback bytes, byte-order mark, order), the reported
//! diagnostics, the emit result, and the exit code. A typed option refusal is
//! reported with its option instead of a write comparison.
//!
//! Rows still listed in `KNOWN` are expected to diverge (the frozen manifest
//! reason stays with the manifest); a known row that becomes exact fails until
//! it is retired here, and an unlisted row that diverges fails. The row sets
//! are fixed from `inventory.v1.json` at the batch start;
//! `TSC_RS_EMITTER_FINAL_CASE_FILTER` narrows a run while editing (a selection
//! matching nothing fails) and `TSC_RS_EMITTER_FINAL_CAPTURE_DIR` saves every
//! selected row's native and frozen writes as files. Independent target.
use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Value};
use tsc_compiler::{DriverError, MemoryOutputSink, ProgramSession};
use tsc_diagnostics::{Diagnostic, MessageChain};
use tsc_emitter::EmitFailure;
use tsc_harness::upstream_suites::execution::{
    load_compiler_emit_with_option_floor, load_qualified_compiler_emit_with_option_floor,
    load_recorded_execution_plans, CompilerExecutionPlan, EmitOptionFloor, UpstreamExecutionCorpus,
    UpstreamExecutionInput,
};
use tsc_program::{PreparedProgram, ProgramLoadLimits};

const FILTER_ENV: &str = "TSC_RS_EMITTER_FINAL_CASE_FILTER";
const CAPTURE_ENV: &str = "TSC_RS_EMITTER_FINAL_CAPTURE_DIR";

/// EF2: `ratchets/h2-5h-known-divergences.v1.json` (owner `h2-5h-ca-2a-r4`).
const EF2_ROWS: &[&str] = &[
    "typescript-6.0.3/compiler/emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5.ts#target%3Des5",
    "typescript-6.0.3/conformance/async/es5/asyncAwait_es5.ts#target%3Des5",
    "typescript-6.0.3/conformance/async/es5/asyncImportedPromise_es5.ts#target%3Des5",
    "typescript-6.0.3/conformance/decorators/class/decoratedBlockScopedClass2.ts#target%3Des5",
    "typescript-6.0.3/conformance/decorators/class/decoratedBlockScopedClass3.ts#target%3Des5",
    "typescript-6.0.3/conformance/emitter/es5/asyncGenerators/emitter.asyncGenerators.classMethods.es5.ts#target%3Des5",
    "typescript-6.0.3/conformance/es6/destructuring/destructuringVariableDeclaration1ES5iterable.ts#target%3Des5",
    "typescript-6.0.3/conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForAwaitOf.3.ts#target%3Des5",
    "typescript-6.0.3/conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForAwaitOf.ts#target%3Des5",
    "typescript-6.0.3/conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForOf.1.ts#target%3Des5",
    "typescript-6.0.3/conformance/statements/VariableStatements/usingDeclarations/awaitUsingDeclarationsInForOf.5.ts#target%3Des5",
    "typescript-6.0.3/conformance/statements/for-ofStatements/ES5For-of37.ts#target%3Des5",
];

/// EF3: `ratchets/h2-6c-known-divergences.v1.json` (owner
/// `h2-6c-m-2-divergence-closure`); the destructuring row is also the sole
/// `ratchets/h2-6a-known-divergences.v1.json` row and replays under both floors.
const EF3_ROWS: &[&str] = &[
    "typescript-6.0.3/compiler/isolatedModulesSourceMap.ts#default",
    "typescript-6.0.3/compiler/jsFileCompilationWithMapFileAsJsWithOutDir.ts#default",
    "typescript-6.0.3/compiler/requireOfJsonFileWithSourceMap.ts#default",
    "typescript-6.0.3/compiler/sourceMapValidationDestructuringForArrayBindingPattern.ts#target%3Des2015",
    "typescript-6.0.3/compiler/sourceMapValidationVarInDownLevelGenerator.ts#target%3Des5",
    "typescript-6.0.3/compiler/sourceMapWithCaseSensitiveFileNamesAndOutDir.ts#default",
    "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default",
    "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNamesAndOutDir.ts#default",
];
const EF3_SHARED_H2_6A_ROW: &str = "typescript-6.0.3/compiler/sourceMapValidationDestructuringForArrayBindingPattern.ts#target%3Des2015";

/// Rows expected to diverge at the current bytes (start: every row of the
/// three frozen manifests). Retire a row here once it replays exact.
/// Retired (exact x2): isolatedModulesSourceMap (EF3-ISOLATED),
/// jsFileCompilationWithMapFileAsJsWithOutDir, requireOfJsonFileWithSourceMap,
/// sourceMapValidationVarInDownLevelGenerator,
/// sourceMapWithCaseSensitiveFileNamesAndOutDir (EF3-HARNESS-FLOOR);
/// emitAccessExpressionOfCastedObjectLiteralExpressionInArrowFunctionES5
/// (EF2-ARROW-PARENS); asyncAwait_es5 (EF2-PROMISE-CTOR);
/// destructuringVariableDeclaration1ES5iterable (EF2-READ-COMMENT, r6);
/// asyncImportedPromise_es5 (EF2-ASYNC-ALIAS-MARK, r7);
/// emitter.asyncGenerators.classMethods.es5 (EF2-ASYNC-GEN-BODY-FLAG, r7);
/// decoratedBlockScopedClass2 (EF2-ALIAS-NUMBERING, r8); ES5For-of37
/// (EF2-DETACHED-COMMENT, r8); sourceMapValidationDestructuringForArrayBindingPattern
/// ×2 profiles (EF3-ITERABLE-2318, r8).
const KNOWN: &[&str] = &[
    "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNames.ts#default",
    "typescript-6.0.3/compiler/sourceMapWithNonCaseSensitiveFileNamesAndOutDir.ts#default",
];

#[derive(Clone, Copy, Debug)]
struct Profile {
    phase: &'static str,
    qualification: &'static str,
    floor: EmitOptionFloor,
}

const H2_5H: Profile = Profile {
    phase: "H2.5h-es5-target",
    qualification: "ratchets/h2-5h-qualification.v1.json",
    floor: EmitOptionFloor::Established,
};
const H2_6A: Profile = Profile {
    phase: "H2.6a-source-map",
    qualification: "ratchets/h2-6a-qualification.v1.json",
    floor: EmitOptionFloor::SourceMapWithOptions,
};
const H2_6C: Profile = Profile {
    phase: "H2.6c-map-observation",
    qualification: "ratchets/h2-6c-qualification.v1.json",
    floor: EmitOptionFloor::MapFamilyWithDeclarationOnly,
};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(256, 2_048, 64, 16 * 1_024 * 1_024, 128 * 1_024 * 1_024)
}

fn artifact(path: &str) -> Value {
    serde_json::from_slice(&std::fs::read(workspace().join(path)).unwrap()).unwrap()
}

fn selected(id: &str) -> bool {
    std::env::var(FILTER_ENV)
        .ok()
        .filter(|value| !value.is_empty())
        .is_none_or(|filter| id.contains(&filter))
}

fn decode(file: &Value) -> Vec<u8> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(file["utf8_base64"].as_str().unwrap())
        .unwrap();
    assert_eq!(bytes.len() as u64, file["utf8_bytes"].as_u64().unwrap());
    bytes
}

fn input_files(input: &Value) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = input["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| (PathBuf::from(file["path"].as_str().unwrap()), decode(file)))
        .collect::<Vec<_>>();
    if !input["virtual_config"].is_null() {
        let config = &input["virtual_config"];
        files.push((
            PathBuf::from(config["path"].as_str().unwrap()),
            decode(config),
        ));
    }
    files
}

fn settings(input: &Value) -> Vec<(String, String)> {
    input["settings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|setting| {
            (
                setting["name"].as_str().unwrap().to_owned(),
                setting["value"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn recorded<'a>(case: &Value, corpus: &'a UpstreamExecutionCorpus) -> &'a CompilerExecutionPlan {
    let case_id = case["case_id"].as_str().unwrap();
    let row = corpus
        .plans
        .iter()
        .find(|row| row.provenance.case_id.as_ref() == case_id)
        .unwrap_or_else(|| panic!("{case_id}: recorded compiler plan is absent"));
    assert_eq!(
        u64::from(row.provenance.case_index),
        case["expansion_case"].as_u64().unwrap(),
        "{case_id}: recorded compiler-plan provenance differs"
    );
    let UpstreamExecutionInput::Compiler(plan) = &row.input else {
        panic!("{case_id}: compiler plan expected")
    };
    plan
}

fn prepare(
    case: &Value,
    corpus: &mut Option<UpstreamExecutionCorpus>,
    floor: EmitOptionFloor,
) -> PreparedProgram {
    let case_id = case["case_id"].as_str().unwrap();
    match case["execution_route"].as_str().unwrap() {
        "qualified-vfs" => {
            let input = &case["input"];
            load_qualified_compiler_emit_with_option_floor(
                &workspace(),
                input["current_directory"].as_str().unwrap(),
                &input_files(input),
                &input["roots"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|root| PathBuf::from(root.as_str().unwrap()))
                    .collect::<Vec<_>>(),
                &settings(input),
                limits(),
                floor,
            )
            .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"))
        }
        "recorded-compiler-plan" => {
            let corpus =
                corpus.get_or_insert_with(|| load_recorded_execution_plans(&workspace()).unwrap());
            load_compiler_emit_with_option_floor(
                &workspace(),
                recorded(case, corpus),
                limits(),
                floor,
            )
            .unwrap_or_else(|error| panic!("{case_id}: prepare failed: {error}"))
        }
        route => panic!("{case_id}: unexpected execution route {route}"),
    }
}

fn message(chain: &MessageChain, indent: usize, text: &mut String) {
    if indent != 0 {
        text.push('\n');
        text.push_str(&"  ".repeat(indent));
    }
    text.push_str(chain.text.as_str().expect("scalar frozen diagnostic text"));
    for next in &chain.next {
        message(next, indent + 1, text);
    }
}

fn diagnostics(diagnostics: &[Diagnostic]) -> Value {
    json!(diagnostics
        .iter()
        .map(|diagnostic| {
            let mut text = String::new();
            message(&diagnostic.message, 0, &mut text);
            json!({
                "code": diagnostic.code(),
                "category": format!("{:?}", diagnostic.category()),
                "file": diagnostic.file_name.as_ref().map(|value| value.as_str().expect("scalar frozen diagnostic filename")),
                "start": diagnostic.start,
                "length": diagnostic.length,
                "message": text,
            })
        })
        .collect::<Vec<_>>())
}

fn project_expected_diagnostic(diagnostic: &Value) -> Value {
    json!({
        "code": diagnostic["code"],
        "category": diagnostic["category"],
        "file": diagnostic["file"],
        "start": diagnostic["start"],
        "length": diagnostic["length"],
        "message": diagnostic["message"],
    })
}

/// One Rust run: a completed command or a typed option refusal.
fn run(prepared: PreparedProgram) -> Value {
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    let session = ProgramSession::new(prepared);
    let bundle = session
        .prepare_harness_lib_bundle()
        .unwrap_or_else(|error| panic!("lib bundle failed: {error}"));
    let mut sink = MemoryOutputSink::new();
    let (outcome, reported) = match session
        .emit_with_reported_diagnostics_for_harness_with_lib_bundle(&mut sink, bundle.as_ref())
    {
        Ok(result) => result,
        Err(DriverError::Emit(EmitFailure::UnsupportedCompilerOption { option })) => {
            assert!(
                sink.writes().is_empty(),
                "typed refusal occurred after a sink write"
            );
            return json!({
                "emit_refused": true,
                "refused_option": option,
                "use_case_sensitive_file_names": case_sensitive,
                "writes": [],
                "reported_diagnostics": [],
                "emit_result": Value::Null,
                "exit_code": Value::Null,
            });
        }
        Err(error) => panic!("emit failed: {error}"),
    };
    let exit_code = if outcome.emit_skipped() && !reported.is_empty() {
        1
    } else if !reported.is_empty() {
        2
    } else {
        0
    };
    let writes = sink
        .writes()
        .iter()
        .enumerate()
        .map(|(index, artifact)| {
            json!({
                "index": index,
                "path": artifact.path().as_str().expect("scalar frozen callback filename"),
                "callback_utf8_base64": base64::engine::general_purpose::STANDARD.encode(artifact.callback_bytes()),
                "callback_utf8_bytes": artifact.callback_bytes().len(),
                "write_byte_order_mark": artifact.write_byte_order_mark(),
            })
        })
        .collect::<Vec<_>>();
    json!({
        "emit_refused": false,
        "refused_option": Value::Null,
        "use_case_sensitive_file_names": case_sensitive,
        "writes": writes,
        "reported_diagnostics": diagnostics(&reported),
        "emit_result": {
            "emit_skipped": outcome.emit_skipped(),
            "diagnostics": diagnostics(outcome.diagnostics()),
            "emitted_files_present": outcome.emitted_files().is_some(),
            "source_maps_present": outcome.source_maps().is_some(),
        },
        "exit_code": exit_code,
    })
}

fn first_difference(actual: &[u8], expected: &[u8]) -> String {
    let offset = actual
        .iter()
        .zip(expected)
        .position(|(a, b)| a != b)
        .unwrap_or(actual.len().min(expected.len()));
    let excerpt = |bytes: &[u8]| {
        let start = offset.saturating_sub(40);
        let end = (offset + 60).min(bytes.len());
        String::from_utf8_lossy(&bytes[start..end]).replace('\n', "\\n")
    };
    format!(
        "first difference at byte {offset} (actual {} bytes, expected {} bytes)\n      actual:   …{}…\n      expected: …{}…",
        actual.len(),
        expected.len(),
        excerpt(actual),
        excerpt(expected)
    )
}

/// Typed differences between one Rust run and the frozen observation.
fn compare(actual: &Value, expected: &Value) -> Vec<String> {
    let mut differences = Vec::new();
    if actual["emit_refused"] == Value::Bool(true) {
        differences.push(format!(
            "typed refusal of option {} (frozen TypeScript command completed with {} writes, exit {})",
            actual["refused_option"],
            expected["writes"].as_array().map_or(0, Vec::len),
            expected["exit_code"]
        ));
        return differences;
    }
    let expected_writes = expected["writes"].as_array().unwrap();
    let actual_writes = actual["writes"].as_array().unwrap();
    if expected_writes.len() != actual_writes.len() {
        differences.push(format!(
            "write count: expected {} observed {} (expected paths {:?}, observed paths {:?})",
            expected_writes.len(),
            actual_writes.len(),
            expected_writes
                .iter()
                .map(|write| write["path"].as_str().unwrap())
                .collect::<Vec<_>>(),
            actual_writes
                .iter()
                .map(|write| write["path"].as_str().unwrap())
                .collect::<Vec<_>>()
        ));
    }
    for (expected_write, actual_write) in expected_writes.iter().zip(actual_writes) {
        let path = expected_write["path"].as_str().unwrap();
        if actual_write["path"] != expected_write["path"] {
            differences.push(format!(
                "write path: expected {path} observed {}",
                actual_write["path"]
            ));
        }
        if actual_write["callback_utf8_base64"] != expected_write["callback_utf8_base64"] {
            let decode = |write: &Value| {
                base64::engine::general_purpose::STANDARD
                    .decode(write["callback_utf8_base64"].as_str().unwrap())
                    .unwrap()
            };
            differences.push(format!(
                "write bytes differ: {path}: {}",
                first_difference(&decode(actual_write), &decode(expected_write))
            ));
        }
        if actual_write["write_byte_order_mark"] != expected_write["write_byte_order_mark"] {
            differences.push(format!("write byte-order mark differs: {path}"));
        }
    }
    let expected_reported = expected["reported_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(project_expected_diagnostic)
        .collect::<Vec<_>>();
    if actual["reported_diagnostics"].as_array().unwrap() != &expected_reported {
        differences.push(format!(
            "reported diagnostics: expected {expected_reported:?} observed {:?}",
            actual["reported_diagnostics"]
        ));
    }
    let expected_result = &expected["emit_result"];
    let actual_result = &actual["emit_result"];
    if actual_result["emit_skipped"] != expected_result["emit_skipped"] {
        differences.push(format!(
            "emit_skipped differs: expected {} observed {}",
            expected_result["emit_skipped"], actual_result["emit_skipped"]
        ));
    }
    let expected_emit_diagnostics = expected_result["diagnostics"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(project_expected_diagnostic)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if actual_result["diagnostics"].as_array().unwrap() != &expected_emit_diagnostics {
        differences.push(format!(
            "emit result diagnostics differ: expected {expected_emit_diagnostics:?} observed {:?}",
            actual_result["diagnostics"]
        ));
    }
    if actual_result["emitted_files_present"].as_bool().unwrap()
        != !expected_result["emitted_files"].is_null()
    {
        differences.push("emitted_files presence differs".to_owned());
    }
    if actual_result["source_maps_present"].as_bool().unwrap()
        != !expected_result["source_maps"].is_null()
    {
        differences.push("source_maps presence differs".to_owned());
    }
    if !expected["status_writes"].as_array().unwrap().is_empty() {
        differences.push("expected status writes are not empty".to_owned());
    }
    if actual["exit_code"] != expected["exit_code"] {
        differences.push(format!(
            "exit code: expected {} observed {}",
            expected["exit_code"], actual["exit_code"]
        ));
    }
    differences
}

fn capture(id: &str, profile: Profile, attempt: usize, actual: &Value, expected: &Value) {
    let Some(directory) = std::env::var_os(CAPTURE_ENV) else {
        return;
    };
    let directory = PathBuf::from(directory).join(format!(
        "{}--{}",
        id.replace(['/', '#', '%'], "_"),
        profile.phase
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(format!("actual-{attempt}.json")),
        serde_json::to_vec_pretty(actual).unwrap(),
    )
    .unwrap();
    if attempt == 0 {
        std::fs::write(
            directory.join("expected.json"),
            serde_json::to_vec_pretty(expected).unwrap(),
        )
        .unwrap();
        for (label, observation) in [("actual", actual), ("expected", expected)] {
            for write in observation["writes"].as_array().into_iter().flatten() {
                let name = Path::new(write["path"].as_str().unwrap())
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                std::fs::write(
                    directory.join(format!("{label}-{name}")),
                    base64::engine::general_purpose::STANDARD
                        .decode(write["callback_utf8_base64"].as_str().unwrap())
                        .unwrap(),
                )
                .unwrap();
            }
        }
    }
}

/// Replays the selected rows of one profile; returns (exact ids, diverging ids with differences).
fn replay(
    profile: Profile,
    rows: &[&str],
    corpus: &mut Option<UpstreamExecutionCorpus>,
) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let qualification = artifact(profile.qualification);
    assert_eq!(qualification["phase"], profile.phase);
    let cases = qualification["cases"].as_array().unwrap();
    let mut exact = Vec::new();
    let mut diverging = Vec::new();
    for row in rows {
        if !selected(row) {
            continue;
        }
        let case = cases
            .iter()
            .find(|case| case["case_id"] == *row)
            .unwrap_or_else(|| panic!("{row}: absent from {}", profile.qualification));
        assert_eq!(case["disposition"], "admitted-for-execution", "{row}");
        let fingerprints = case["typescript_run_fingerprints"].as_array().unwrap();
        let expected = &case["typescript_observation"];
        assert_eq!(fingerprints.len(), 2, "{row}");
        assert!(
            fingerprints
                .iter()
                .all(|fingerprint| fingerprint == &expected["run_fingerprint_sha256"]),
            "{row}: compact TypeScript repetition proof differs"
        );
        let mut outcomes = Vec::new();
        for attempt in 0..2 {
            let actual = run(prepare(case, corpus, profile.floor));
            capture(row, profile, attempt, &actual, expected);
            outcomes.push(actual);
        }
        assert_eq!(
            outcomes[0], outcomes[1],
            "{row}: repeated Rust emit is not deterministic"
        );
        let differences = compare(&outcomes[0], expected);
        if differences.is_empty() {
            eprintln!("emitter final rows EXACT x2 [{}] {row}", profile.phase);
            exact.push((*row).to_owned());
        } else {
            eprintln!(
                "emitter final rows DIVERGING [{}] {row}:\n    {}",
                profile.phase,
                differences.join("\n    ")
            );
            diverging.push(((*row).to_owned(), differences));
        }
    }
    (exact, diverging)
}

#[test]
fn ef2_ef3_known_rows_replay_against_frozen_qualification_observations() {
    assert_eq!(EF2_ROWS.len(), 12);
    assert_eq!(EF3_ROWS.len(), 8);
    assert!(EF3_ROWS.contains(&EF3_SHARED_H2_6A_ROW));
    for (path, expected_ids) in [
        ("ratchets/h2-5h-known-divergences.v1.json", EF2_ROWS),
        ("ratchets/h2-6c-known-divergences.v1.json", EF3_ROWS),
        (
            "ratchets/h2-6a-known-divergences.v1.json",
            &[EF3_SHARED_H2_6A_ROW][..],
        ),
    ] {
        let manifest = artifact(path);
        let ids = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|case| case["case_id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        // The frozen manifests shrink only through the integrator's retire proposal;
        // the batch's fixed row set must still be a superset of the manifest.
        for id in &ids {
            assert!(
                expected_ids.contains(&id.as_str()),
                "{path}: manifest row outside the batch set: {id}"
            );
        }
    }
    let mut corpus = None;
    let mut exact = Vec::new();
    let mut diverging = Vec::new();
    for (profile, rows) in [
        (H2_5H, EF2_ROWS),
        (H2_6C, EF3_ROWS),
        (H2_6A, &[EF3_SHARED_H2_6A_ROW][..]),
    ] {
        let (profile_exact, profile_diverging) = replay(profile, rows, &mut corpus);
        exact.extend(
            profile_exact
                .into_iter()
                .map(|id| format!("{}:{id}", profile.phase)),
        );
        diverging.extend(
            profile_diverging
                .into_iter()
                .map(|(id, differences)| (format!("{}:{id}", profile.phase), differences)),
        );
    }
    let executed = exact.len() + diverging.len();
    assert!(executed > 0, "the case filter selected nothing");
    let known = |key: &str| KNOWN.contains(&key.split_once(':').unwrap().1);
    let unexpected_exact = exact
        .iter()
        .filter(|key| known(key))
        .cloned()
        .collect::<Vec<_>>();
    let unexpected_diverging = diverging
        .iter()
        .filter(|(key, _)| !known(key))
        .cloned()
        .collect::<Vec<_>>();
    eprintln!(
        "emitter final rows SUMMARY exact={} known={} failed={} selected={executed}",
        exact.len() - unexpected_exact.len(),
        diverging.len() - unexpected_diverging.len(),
        unexpected_exact.len() + unexpected_diverging.len()
    );
    assert!(
        unexpected_exact.is_empty(),
        "known rows now replay exact; retire them from KNOWN: {unexpected_exact:#?}"
    );
    assert!(
        unexpected_diverging.is_empty(),
        "rows outside KNOWN diverge: {unexpected_diverging:#?}"
    );
}
