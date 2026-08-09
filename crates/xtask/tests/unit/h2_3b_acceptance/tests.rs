use std::path::{Path, PathBuf};
use std::{fs, thread};

const CLASSIC_CASE_IDS: [&str; 2] = [
    "typescript-6.0.3/compiler/jsxSpreadTag.ts#target%3Desnext",
    "typescript-6.0.3/conformance/jsx/tsxEmitSpreadAttribute.ts#target%3Desnext",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn read_json(workspace: &Path, relative_path: &str) -> serde_json::Value {
    serde_json::from_slice(&fs::read(workspace.join(relative_path)).expect("read H2.3b artifact"))
        .expect("parse H2.3b artifact")
}

#[test]
fn all_h2_3b_candidate_dispositions_and_owner_controls_execute() {
    super::run(&workspace()).expect("H2.3b acceptance");
}

#[test]
fn h2_3b_two_worker_execution_is_exact_and_isolated() {
    let workspace = workspace();
    let artifact = read_json(&workspace, super::QUALIFICATION_RELATIVE_PATH);
    let case = artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"] == CLASSIC_CASE_IDS[0])
        .expect("classic compiler case")
        .clone();
    let results = thread::scope(|scope| {
        (0..2)
            .map(|_| {
                let case = case.clone();
                let workspace = &workspace;
                scope.spawn(move || {
                    super::execute_exact_case(workspace, &case)
                        .unwrap_or_else(|error| panic!("two-worker H2.3b emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.3b worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results, [(1, 4), (1, 4)]);
}

#[test]
fn h2_3b_denominator_separates_classic_from_automatic_runtime() {
    let workspace = workspace();
    let artifact = read_json(&workspace, super::QUALIFICATION_RELATIVE_PATH);
    let cases = artifact["cases"].as_array().expect("qualification cases");
    let admitted = cases
        .iter()
        .filter(|case| case["disposition"] == "admitted-for-execution")
        .map(|case| case["case_id"].as_str().expect("case id"))
        .collect::<Vec<_>>();
    assert_eq!(admitted, CLASSIC_CASE_IDS);

    let deferred = cases
        .iter()
        .filter(|case| case["disposition"] == "deferred-to-slices")
        .collect::<Vec<_>>();
    assert_eq!(deferred.len(), 4);
    assert!(deferred
        .iter()
        .all(|case| case["required_slices"] == serde_json::json!(["H2.3c"])));
}

#[test]
fn h2_3b_owner_controls_cover_modes_factories_pragmas_and_extensions() {
    let workspace = workspace();
    let artifact = read_json(&workspace, super::OWNER_CONTROLS_RELATIVE_PATH);
    let controls = artifact["controls"].as_array().expect("owner controls");
    assert_eq!(controls.len(), 8);
    assert_eq!(
        controls
            .iter()
            .map(|control| control["input"]["compiler_options"]["jsx"]
                .as_i64()
                .expect("jsx mode"))
            .collect::<Vec<_>>(),
        [2, 2, 2, 2, 1, 3, 1, 2]
    );
    assert_eq!(
        controls
            .iter()
            .map(|control| control["observation"]["writes"][0]["path"]
                .as_str()
                .expect("write path"))
            .collect::<Vec<_>>(),
        [
            "/project/emoji-😀.js",
            "/project/options.js",
            "/project/pragma.js",
            "/project/namespace.js",
            "/project/preserve.jsx",
            "/project/native.js",
            "/project/dist-preserve/input.jsx",
            "/project/dist-classic/input.js",
        ]
    );
}
