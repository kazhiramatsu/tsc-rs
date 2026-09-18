//! H2.8a-A-RES-EMITTER-FINAL (`docs/design/greenfield/slices/emitter-final-batch/`):
//! focused re-measurement entries for the handoff's historical rows.
//!
//! * EF4 / EF5: the 40 class commands (`class-field-alias-map-positions` 28,
//!   `class-header-token` 8, `hoisted-declaration-export-ranges` 4) that the
//!   PLAN-BASE ledger records as `historical-class-failure-not-remeasured`.
//!   Each selected command runs through the unchanged shared exact
//!   comparator (`h2_7c_declaration_blocking::assert_cases_with_inspection`),
//!   twice on failure (a failure that passes on repetition is reported as
//!   non-deterministic); the summary line counts exact / failed / selected.
//! * EF6: the 14 global commands of `ratchets/h2-8a-global-after-a6-37.v1.json`
//!   projected through the unchanged 769-command output-matrix comparator.
//!
//! The row sets are fixed from `inventory.v1.json` at the batch start;
//! `TSC_RS_EMITTER_FINAL_CASE_FILTER` (a case-id substring) narrows a run
//! while editing and a selection that matches nothing fails. Set
//! `TSC_RS_EMITTER_FINAL_CAPTURE_DIR` to an absolute directory to save every
//! selected command's native writes and exit next to its frozen expectation.
//! Independent integration target: the shared comparator modules are included
//! for their functions only; their own tests are filtered by the runner.
#[path = "integration/h2_7b_w4a_controls.rs"]
#[allow(dead_code)]
mod h2_7b_w4a_controls;
#[path = "integration/h2_7c_declaration_blocking.rs"]
#[allow(dead_code)]
mod h2_7c_declaration_blocking;
#[path = "integration/h2_7d_original_corpus_shared.rs"]
#[allow(dead_code)]
mod h2_7d_original_corpus_shared;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

const FILTER_ENV: &str = "TSC_RS_EMITTER_FINAL_CASE_FILTER";
const CAPTURE_ENV: &str = "TSC_RS_EMITTER_FINAL_CAPTURE_DIR";

/// (fixture, case id): EF4 first (28), then EF5 (12), in inventory order.
const EF4_EF5_ROWS: &[(&str, &str)] = &[
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es2015/commonjs/define/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es2015/commonjs/set/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es2015/esnext/define/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es2015/esnext/set/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/define/concise-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/define/field-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/define/legacy-bound-this",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/define/legacy-static-block",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/define/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/define/static-block-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/set/concise-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/set/field-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/set/legacy-bound-this",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/set/legacy-static-block",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/set/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/commonjs/set/static-block-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/define/concise-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/define/field-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/define/legacy-bound-this",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/define/legacy-static-block",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/define/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/define/static-block-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/set/concise-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/set/field-arrow",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/set/legacy-bound-this",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/set/legacy-static-block",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/set/nested-computed-name",
    ),
    (
        "class-field-alias-map-positions.json",
        "class-field-alias-map-positions/es5/esnext/set/static-block-arrow",
    ),
    (
        "class-header-token.json",
        "class-header-token/js/es2015/after-default",
    ),
    (
        "class-header-token.json",
        "class-header-token/js/es2015/after-export",
    ),
    (
        "class-header-token.json",
        "class-header-token/js/esnext/after-default",
    ),
    (
        "class-header-token.json",
        "class-header-token/js/esnext/after-export",
    ),
    (
        "class-header-token.json",
        "class-header-token/ts/es2015/after-default",
    ),
    (
        "class-header-token.json",
        "class-header-token/ts/es2015/after-export",
    ),
    (
        "class-header-token.json",
        "class-header-token/ts/esnext/after-default",
    ),
    (
        "class-header-token.json",
        "class-header-token/ts/esnext/after-export",
    ),
    (
        "hoisted-declaration-export-ranges.json",
        "hoisted-export/es5/amd/class/direct-escaped",
    ),
    (
        "hoisted-declaration-export-ranges.json",
        "hoisted-export/es5/amd/class/escaped",
    ),
    (
        "hoisted-declaration-export-ranges.json",
        "hoisted-export/es5/commonjs/class/direct-escaped",
    ),
    (
        "hoisted-declaration-export-ranges.json",
        "hoisted-export/es5/commonjs/class/escaped",
    ),
];

/// EF6: the 14 rows of `ratchets/h2-8a-global-after-a6-37.v1.json` `failures`.
const EF6_ROWS: &[&str] = &[
    "typescript-6.0.3/compiler/declarationEmitPathMappingMonorepo2.ts#default",
    "typescript-6.0.3/compiler/jsDeclarationEmitExportAssignedFunctionWithExtraTypedefsMembers.ts#default",
    "typescript-6.0.3/compiler/reactImportDropped.ts#default",
    "typescript-6.0.3/conformance/externalModules/rewriteRelativeImportExtensions/emitModuleCommonJS.ts#module%3Dcommonjs",
    "typescript-6.0.3/conformance/externalModules/rewriteRelativeImportExtensions/emitModuleCommonJS.ts#module%3Dnodenext",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsClassMethod.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionAnonymousWithSub.ts#target%3Des2015",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsExportAssignedClassExpressionAnonymousWithSub.ts#target%3Des5",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsFunctionsCjs.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsParameterTagReusesInputNodeInEmit1.ts#default",
    "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsParameterTagReusesInputNodeInEmit2.ts#default",
    "typescript-6.0.3/conformance/jsdoc/linkTagEmit1.ts#default",
    "typescript-6.0.3/conformance/moduleResolution/bundler/bundlerImportTsExtensions.ts#allowimportingtsextensions%3Dtrue%2Cnoemit%3Dfalse",
    "typescript-6.0.3/conformance/salsa/plainJSGrammarErrors.ts#default",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn filter() -> Option<String> {
    std::env::var(FILTER_ENV)
        .ok()
        .filter(|value| !value.is_empty())
}

fn selected(id: &str) -> bool {
    filter().is_none_or(|filter| id.contains(&filter))
}

fn load_fixture(name: &str, expected_cases: usize) -> Value {
    let path = workspace()
        .join("crates/compiler/tests/fixtures")
        .join(name);
    let artifact: Value = serde_json::from_slice(
        &std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display())),
    )
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3", "{name}");
    assert_eq!(artifact["repetitions"], 2, "{name}");
    assert_eq!(
        artifact["cases"].as_array().unwrap().len(),
        expected_cases,
        "{name}"
    );
    artifact
}

// Optional source-cause trace, counted separately from the unchanged complete
// comparison (the `class-field-alias-map-positions` test's capture, keyed by
// the batch's own environment variable).
fn capture_artifact_bytes(
    case_id: &str,
    prepared: &tsc_program::PreparedProgram,
    expected: &Value,
) {
    use sha2::Digest;
    let Some(directory) = std::env::var_os(CAPTURE_ENV) else {
        return;
    };
    let directory = PathBuf::from(directory);
    assert!(directory.is_absolute(), "capture path must be absolute");
    std::fs::create_dir_all(&directory).unwrap();
    let key = format!("{:x}", sha2::Sha256::digest(case_id.as_bytes()));
    let index = (0..)
        .find(|index| !directory.join(format!("{key}-{index}.json")).exists())
        .unwrap();
    let mut sink = tsc_compiler::MemoryOutputSink::new();
    let command =
        tsc_compiler::ProgramSession::new(prepared.clone()).emit_command_for_harness(&mut sink);
    let (exit_code, error) = match command {
        Ok(command) => (Some(command.exit_code()), None),
        Err(error) => (None, Some(error.to_string())),
    };
    let writes = sink
        .writes()
        .iter()
        .map(|write| {
            json!({
                "path": write.path().to_string_lossy(),
                "kind": format!("{:?}", write.kind()),
                "callback_text": write.callback_text(),
            })
        })
        .collect::<Vec<_>>();
    let captured = json!({"case_id": case_id, "capture_index": index,
        "capture_kind": "supplemental-artifact-bytes", "exit_code": exit_code,
        "error": error, "writes": writes, "expected": expected});
    std::fs::write(
        directory.join(format!("{key}-{index}.json")),
        serde_json::to_vec_pretty(&captured).unwrap(),
    )
    .unwrap();
}

#[test]
fn ef4_ef5_historical_class_rows_match_complete_typescript_observations() {
    let mut fixtures = BTreeMap::new();
    for (name, count) in [
        ("class-field-alias-map-positions.json", 384),
        ("class-header-token.json", 88),
        ("hoisted-declaration-export-ranges.json", 168),
    ] {
        fixtures.insert(name, load_fixture(name, count));
    }
    assert_eq!(EF4_EF5_ROWS.len(), 40);
    let mut failures = Vec::new();
    let mut executed = 0usize;
    for (fixture, id) in EF4_EF5_ROWS {
        if !selected(id) {
            continue;
        }
        executed += 1;
        let case = fixtures[fixture]["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case_id"] == *id)
            .unwrap_or_else(|| panic!("{id}: absent from {fixture}"));
        let run = || {
            h2_7c_declaration_blocking::assert_cases_with_inspection(
                &json!({"cases": [case]}),
                true,
                capture_artifact_bytes,
            )
        };
        if std::panic::catch_unwind(run).is_ok() {
            eprintln!("EF4/EF5 EXACT x2 {id}");
        } else {
            assert!(
                std::panic::catch_unwind(run).is_err(),
                "{id}: failed then passed"
            );
            eprintln!("EF4/EF5 REPEATED FAILURE {id}");
            failures.push(*id);
        }
    }
    assert!(executed > 0, "the case filter selected nothing");
    eprintln!(
        "EF4/EF5 SUMMARY exact={} failed={} selected={executed}",
        executed - failures.len(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "EF4/EF5 complete-command failures ({} of {executed}): {failures:#?}",
        failures.len()
    );
}

#[test]
fn ef6_historical_global_rows_match_complete_production_commands() {
    assert_eq!(EF6_ROWS.len(), 14);
    let rows = EF6_ROWS
        .iter()
        .copied()
        .filter(|id| selected(id))
        .collect::<Vec<_>>();
    assert!(!rows.is_empty(), "the case filter selected nothing");
    eprintln!("EF6 selected={}", rows.len());
    let exact = h2_7d_original_corpus_shared::assert_output_matrix_projection(&workspace(), &rows);
    assert_eq!(exact.len(), rows.len());
    eprintln!("EF6 SUMMARY exact={} selected={}", exact.len(), rows.len());
}
