use std::path::{Path, PathBuf};
use std::{fs, thread};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_1a_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.1a acceptance");
}

#[test]
fn h2_1a_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/compiler/acceptSymbolAsWeakType.ts#default",
        "typescript-6.0.3/conformance/async/es6/functionDeclarations/asyncOrYieldAsBindingIdentifier1.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1a qualification"),
    )
    .expect("parse H2.1a qualification");
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
                    super::execute_observed(workspace, &case, true)
                        .unwrap_or_else(|error| panic!("two-worker H2.1a emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.1a worker panicked"))
            .collect::<Vec<_>>()
    });
    assert!(results.iter().all(|(writes, _)| *writes == 1));
}
