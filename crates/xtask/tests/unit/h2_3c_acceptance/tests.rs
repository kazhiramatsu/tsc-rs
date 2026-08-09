use std::path::{Path, PathBuf};
use std::{fs, thread};

const AUTOMATIC_CASE_IDS: [&str; 4] = [
    "typescript-6.0.3/compiler/jsxNamespacedNameNotComparedToNonMatchingIndexSignature.tsx#default",
    "typescript-6.0.3/conformance/jsx/tsxReactEmit8.tsx#jsx%3Dreact-jsx",
    "typescript-6.0.3/conformance/jsx/tsxReactEmit8.tsx#jsx%3Dreact-jsxdev",
    "typescript-6.0.3/conformance/jsx/tsxReactEmitSpreadAttribute.ts#target%3Desnext",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn read_json(workspace: &Path, relative_path: &str) -> serde_json::Value {
    serde_json::from_slice(&fs::read(workspace.join(relative_path)).expect("read H2.3c artifact"))
        .expect("parse H2.3c artifact")
}

#[test]
fn all_h2_3c_candidate_dispositions_and_owner_controls_execute() {
    super::run(&workspace()).expect("H2.3c acceptance");
}

#[test]
fn h2_3c_two_worker_execution_is_exact_and_isolated() {
    let workspace = workspace();
    let artifact = read_json(&workspace, super::QUALIFICATION_RELATIVE_PATH);
    let case = artifact["cases"]
        .as_array()
        .expect("qualification cases")
        .iter()
        .find(|case| case["case_id"] == AUTOMATIC_CASE_IDS[0])
        .expect("automatic compiler case")
        .clone();
    let results = thread::scope(|scope| {
        (0..2)
            .map(|_| {
                let case = case.clone();
                let workspace = &workspace;
                scope.spawn(move || {
                    super::execute_exact_case(workspace, &case)
                        .unwrap_or_else(|error| panic!("two-worker H2.3c emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.3c worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results, [(1, 4), (1, 4)]);
}

#[test]
fn h2_3c_denominator_promotes_every_automatic_runtime_carry_forward() {
    let workspace = workspace();
    let artifact = read_json(&workspace, super::QUALIFICATION_RELATIVE_PATH);
    let cases = artifact["cases"].as_array().expect("qualification cases");
    let admitted = cases
        .iter()
        .filter(|case| case["disposition"] == "admitted-for-execution")
        .map(|case| case["case_id"].as_str().expect("case id"))
        .collect::<Vec<_>>();
    assert_eq!(admitted, AUTOMATIC_CASE_IDS);
    assert!(cases
        .iter()
        .all(|case| case["required_slices"] == serde_json::json!([])));
}

#[test]
fn h2_3c_owner_controls_cover_runtimes_imports_keys_modules_and_file_kinds() {
    let workspace = workspace();
    let artifact = read_json(&workspace, super::OWNER_CONTROLS_RELATIVE_PATH);
    let controls = artifact["controls"].as_array().expect("owner controls");
    assert_eq!(controls.len(), 9);
    assert_eq!(
        controls
            .iter()
            .map(|control| control["input"]["compiler_options"]["jsx"]
                .as_i64()
                .expect("jsx mode"))
            .collect::<Vec<_>>(),
        [4, 5, 4, 4, 4, 4, 4, 4, 2]
    );
    assert_eq!(
        controls
            .iter()
            .map(|control| control["observation"]["writes"][0]["path"]
                .as_str()
                .expect("write path"))
            .collect::<Vec<_>>(),
        [
            "/project/automatic.js",
            "/project/emoji-😀.js",
            "/project/dist/input.js",
            "/project/pragma.js",
            "/project/classic.js",
            "/project/key.js",
            "/project/commonjs.js",
            "/project/indicator.js",
            "/project/automatic-pragma.js",
        ]
    );
}
