use std::path::{Path, PathBuf};
use std::{fs, thread};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical workspace")
}

#[test]
fn all_h2_1d_candidate_dispositions_execute() {
    super::run(&workspace()).expect("H2.1d acceptance");
}

#[test]
fn h2_1d_two_worker_execution_is_exact_and_isolated() {
    const CASE_IDS: [&str; 2] = [
        "typescript-6.0.3/conformance/dynamicImport/importCallExpressionInSystem1.ts#default",
        "typescript-6.0.3/conformance/dynamicImport/importCallExpressionInSystem2.ts#default",
    ];
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1d qualification"),
    )
    .expect("parse H2.1d qualification");
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
                        .unwrap_or_else(|error| panic!("two-worker H2.1d emit failed: {error}"))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|worker| worker.join().expect("H2.1d worker panicked"))
            .collect::<Vec<_>>()
    });
    assert_eq!(results[0].0, 2);
    assert_eq!(results[1].0, 2);
}

#[test]
fn h2_1d_multifile_order_and_dynamic_import_rewrite_are_exact() {
    const CASE_ID: &str =
        "typescript-6.0.3/conformance/dynamicImport/importCallExpressionInSystem1.ts#default";
    let workspace = workspace();
    let artifact: serde_json::Value = serde_json::from_slice(
        &fs::read(workspace.join(super::QUALIFICATION_RELATIVE_PATH))
            .expect("read H2.1d qualification"),
    )
    .expect("parse H2.1d qualification");
    let case = artifact["cases"]
        .as_array()
        .and_then(|cases| cases.iter().find(|case| case["case_id"] == CASE_ID))
        .expect("missing multi-file helper case");

    let mut sink = tsc_compiler::MemoryOutputSink::new();
    let (outcome, diagnostics) = tsc_compiler::ProgramSession::new(
        super::case_input(&workspace, case).expect("load multi-file helper case"),
    )
    .emit_with_reported_diagnostics_for_harness(&mut sink)
    .expect("emit multi-file helper case");
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
    .expect("exact multi-file writes");

    assert_eq!(
        sink.writes()
            .iter()
            .map(|write| write.path())
            .collect::<Vec<_>>(),
        [Path::new("/.src/0.js"), Path::new("/.src/1.js")]
    );
    let system_output = sink.writes()[1].callback_text();
    assert_eq!(system_output.matches("System.register(").count(), 1);
    assert_eq!(
        system_output.matches("context_1.import(\"./0\")").count(),
        4
    );
}
