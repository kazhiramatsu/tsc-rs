use std::path::{Path, PathBuf};
use std::{fs, thread};

use serde_json::{json, Value};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_1e_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.1e acceptance");
}

#[test]
fn h2_1e_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/compiler/importAssertionsDeprecatedIgnored.ts#default",
        "typescript-6.0.3/compiler/moduleExportNonStructured.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1e qualification"),
    )
    .expect("parse H2.1e qualification");
    let cases = CASE_IDS.map(|case_id| {
        artifact["cases"]
            .as_array()
            .and_then(|cases| cases.iter().find(|case| case["case_id"] == case_id))
            .cloned()
            .unwrap_or_else(|| panic!("missing two-worker case {case_id}"))
    });

    let results = thread::scope(|scope| {
        let workspace = &workspace;
        cases
            .into_iter()
            .map(|case| {
                scope.spawn(move || {
                    super::execute_observed(workspace, &case)
                        .unwrap_or_else(|error| panic!("two-worker H2.1e emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.1e worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results[0].0, 3);
    assert_eq!(results[1].0, 1);
}

#[test]
fn h2_1e_import_attribute_order_and_bytes_are_exact() {
    const CASE_ID: &str = "typescript-6.0.3/compiler/importAssertionsDeprecatedIgnored.ts#default";
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1e qualification"),
    )
    .expect("parse H2.1e qualification");
    let case = artifact["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["case_id"] == CASE_ID))
        .expect("missing import-attributes case");

    let mut sink = tsc_compiler::MemoryOutputSink::new();
    let (outcome, diagnostics) = tsc_compiler::ProgramSession::new(
        super::case_input(&workspace, case).expect("load import-attributes case"),
    )
    .emit_with_reported_diagnostics_for_harness(&mut sink)
    .expect("emit import-attributes case");
    super::assert_reported_diagnostics(
        CASE_ID,
        super::array(&case["typescript_runs"][0], "reported_diagnostics")
            .expect("expected diagnostics"),
        &diagnostics,
    )
    .expect("exact diagnostics");
    assert!(!outcome.emit_skipped());
    super::assert_exact_writes(
        CASE_ID,
        super::array(&case["typescript_runs"][0], "writes").expect("expected writes"),
        &sink,
    )
    .expect("exact import-attributes writes");

    assert_eq!(
        sink.writes()
            .iter()
            .map(|write| write.path())
            .collect::<Vec<_>>(),
        [Path::new("/a.js"), Path::new("/b.js"), Path::new("/c.js")]
    );
    assert_eq!(
        sink.writes()[2].callback_text(),
        "export { default as config } from \"./config.json\" assert { type: \"json\" };\r\n"
    );
}

fn module_setting(module_state: &str) -> &'static str {
    match module_state {
        "Node16(100)" => "node16",
        "Node18(101)" => "node18",
        "Node20(102)" => "node20",
        "NodeNext(199)" => "nodenext",
        "CommonJS(1)" => "commonjs",
        "AMD(2)" => "amd",
        "UMD(3)" => "umd",
        "System(4)" => "system",
        "ESNext(99)" => "esnext",
        "Preserve(200)" => "preserve",
        other => panic!("unexpected owner-control module state {other}"),
    }
}

fn owner_case(input: &Value, files: Value, roots: Value, module_state: &str) -> Value {
    json!({
        "input": {
            "current_directory": input["current_directory"],
            "roots": roots,
            "settings": [
                {"name": "target", "value": "esnext"},
                {"name": "module", "value": module_setting(module_state)},
                {"name": "rewriteRelativeImportExtensions", "value": "true"},
                {"name": "newLine", "value": "lf"},
                {"name": "ignoreDeprecations", "value": "6.0"}
            ],
            "virtual_config": null,
            "files": files
        }
    })
}

fn execute_owner_observation(workspace: &Path, case_id: &str, case: &Value, observation: &Value) {
    let mut sink = tsc_compiler::MemoryOutputSink::new();
    let (outcome, diagnostics) = tsc_compiler::ProgramSession::new(
        super::case_input(workspace, case).expect("load owner-control project"),
    )
    .emit_with_reported_diagnostics_for_harness(&mut sink)
    .unwrap_or_else(|error| panic!("{case_id}: owner-control emit failed: {error}"));
    assert!(!outcome.emit_skipped(), "{case_id}: emit skipped");
    assert!(diagnostics.is_empty(), "{case_id}: gained diagnostics");
    super::assert_exact_writes(
        case_id,
        super::array(observation, "writes").expect("owner writes"),
        &sink,
    )
    .unwrap_or_else(|error| panic!("{case_id}: {error}"));
    let reached = observation["writes"].as_array().map_or(0, Vec::len) as u64;
    assert_eq!(
        outcome
            .h2_activity()
            .runtime_slice(tsc_compiler::H2RuntimeSlice::H2_1e),
        reached,
        "{case_id}: H2.1e activity"
    );
}

#[test]
fn h2_1e_node_format_owner_controls_match_pinned_typescript() {
    let workspace = workspace();
    let artifact: Value = serde_json::from_slice(
        &fs::read(workspace.join("ratchets/h2-1e-owner-controls.v1.json"))
            .expect("read H2.1e owner controls"),
    )
    .expect("parse H2.1e owner controls");
    for control_index in [0usize, 1, 3] {
        let control = &artifact["controls"][control_index];
        let input = &control["input"];
        for run in control["runs"].as_array().expect("owner module runs") {
            let module_state = run["module_state"].as_str().expect("module state");
            let case = owner_case(
                input,
                input["files"].clone(),
                input["roots"].clone(),
                module_state,
            );
            execute_owner_observation(
                &workspace,
                &format!("{}:{module_state}", control["control_id"]),
                &case,
                &run["observation"],
            );
        }
    }
}

#[test]
fn h2_1e_fresh_package_type_and_path_casing_are_isolated() {
    let workspace = workspace();
    let artifact: Value = serde_json::from_slice(
        &fs::read(workspace.join("ratchets/h2-1e-owner-controls.v1.json"))
            .expect("read H2.1e owner controls"),
    )
    .expect("parse H2.1e owner controls");
    let control = &artifact["controls"][2];
    let input = &control["input"];
    for repetition in 0..2 {
        for variant in control["variants"].as_array().expect("package variants") {
            let files = json!([
                {
                    "path": "/Fresh/package.json",
                    "utf8_base64": variant["package_json"]["utf8_base64"],
                    "utf8_sha256": variant["package_json"]["utf8_sha256"],
                    "utf8_bytes": variant["package_json"]["utf8_bytes"]
                },
                {
                    "path": input["root"],
                    "utf8_base64": input["source"]["utf8_base64"],
                    "utf8_sha256": input["source"]["utf8_sha256"],
                    "utf8_bytes": input["source"]["utf8_bytes"]
                }
            ]);
            let case = owner_case(
                input,
                files,
                json!([input["root"]]),
                input["module"].as_str().expect("fresh module"),
            );
            execute_owner_observation(
                &workspace,
                &format!(
                    "fresh:{}:{repetition}",
                    variant["package_type"].as_str().expect("package type")
                ),
                &case,
                &variant["observation"],
            );
        }
    }
}
