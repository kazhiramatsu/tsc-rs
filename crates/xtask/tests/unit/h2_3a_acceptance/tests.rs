use std::path::{Path, PathBuf};
use std::{fs, thread};

const CASE_ID: &str = "typescript-6.0.3/conformance/jsdoc/jsdocTypeTag.ts#default";

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

fn qualification(workspace: &Path) -> serde_json::Value {
    serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.3a qualification"),
    )
    .expect("parse H2.3a qualification")
}

#[test]
fn all_h2_3a_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.3a acceptance");
}

#[test]
fn h2_3a_two_worker_execution_is_exact_and_isolated() {
    let workspace = workspace();
    let artifact = qualification(&workspace);
    let case = artifact["cases"][0].clone();
    let results = thread::scope(|scope| {
        (0..2)
            .map(|_| {
                let case = case.clone();
                let workspace = &workspace;
                scope.spawn(move || {
                    super::execute_observed(workspace, &case)
                        .unwrap_or_else(|error| panic!("two-worker H2.3a emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.3a worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results, [(1, 1), (1, 1)]);
}

#[test]
fn h2_3a_javascript_collision_and_typescript_sibling_write_are_exact() {
    let workspace = workspace();
    let artifact = qualification(&workspace);
    let case = &artifact["cases"][0];
    assert_eq!(case["case_id"], CASE_ID);
    assert_eq!(case["disposition"], "admitted-for-execution");
    assert_eq!(case["required_slices"], serde_json::json!([]));

    let files = case["files"].as_array().expect("case files");
    assert_eq!(files.len(), 2);
    assert_eq!(files[0]["script_kind"], "JS");
    assert_eq!(files[0]["path"], "/.src/a.js");
    assert_eq!(files[1]["script_kind"], "TS");
    assert_eq!(files[1]["path"], "/.src/b.ts");

    let run = &case["typescript_runs"][0];
    assert_eq!(run["reported_diagnostics"][0]["code"], 5055);
    assert_eq!(run["emit_result"]["emit_skipped"], true);
    assert_eq!(run["writes"].as_array().expect("writes").len(), 1);
    assert_eq!(run["writes"][0]["path"], "/.src/b.js");
    assert_eq!(
        run["writes"][0]["source_files"],
        serde_json::json!(["/.src/b.ts"])
    );
    super::execute_observed(&workspace, case).expect("exact H2.3a observation");
}

#[test]
fn h2_3a_denominator_is_the_single_dependency_closed_global_row() {
    let workspace = workspace();
    let artifact = qualification(&workspace);
    let dispositions: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join("ratchets/h2-candidate-dispositions.v1.json"))
            .expect("read global dispositions"),
    )
    .expect("parse global dispositions");
    let closed_before = [
        "H2.1a", "H2.1b", "H2.1c", "H2.1d", "H2.1e", "H2.2a", "H2.2b", "H2.2c", "H2.2d",
    ];
    let expected = dispositions["cases"]
        .as_array()
        .expect("global cases")
        .iter()
        .filter(|case| matches!(case["suite"].as_str(), Some("compiler" | "conformance")))
        .filter(|case| {
            case["required_slices"]
                .as_array()
                .expect("required slices")
                .iter()
                .filter_map(serde_json::Value::as_str)
                .filter(|slice| !closed_before.contains(slice))
                .collect::<Vec<_>>()
                == ["H2.3a"]
        })
        .map(|case| case["id"].as_str().expect("case id"))
        .collect::<Vec<_>>();
    assert_eq!(expected, [CASE_ID]);
    assert_eq!(artifact["cases"][0]["case_id"], expected[0]);
}
