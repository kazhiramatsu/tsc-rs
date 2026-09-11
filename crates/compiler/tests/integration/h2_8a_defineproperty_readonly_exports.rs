//! Complete TypeScript commands for DefineProperty readonly exports.
#[test]
fn defineproperty_readonly_exports_matches_complete_typescript_observations() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/defineproperty-readonly-exports.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    let mut failures = Vec::new();
    for case in cases {
        let case_id = case["case_id"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            super::h2_7c_declaration_blocking::assert_cases_with_inspection(
                &serde_json::json!({"cases":[case]}),
                true,
                capture_artifact_bytes,
            );
        });
        if result.is_err() {
            failures.push(case_id);
        } else {
            eprintln!("DefineProperty readonly exports EXACT x2 {case_id}");
        }
    }
    assert!(
        failures.is_empty(),
        "complete DefineProperty readonly exports failures: {failures:?}"
    );
}

// Optional source-cause trace. These additional command executions are counted
// separately from the unchanged complete comparison above.
fn capture_artifact_bytes(
    case_id: &str,
    prepared: &tsc_program::PreparedProgram,
    expected: &serde_json::Value,
) {
    use sha2::Digest;
    let Some(directory) = std::env::var_os("TSC_RS_H2_8A_CAPTURE_WRITES_DIR") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
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
            serde_json::json!({
                "path": write.path().to_string_lossy(),
                "kind": format!("{:?}", write.kind()),
                "callback_text": write.callback_text(),
            })
        })
        .collect::<Vec<_>>();
    let captured = serde_json::json!({"case_id":case_id,"capture_index":index,
        "capture_kind":"supplemental-artifact-bytes","exit_code":exit_code,
        "error":error,"writes":writes,"expected":expected});
    std::fs::write(
        directory.join(format!("{key}-{index}.json")),
        serde_json::to_vec_pretty(&captured).unwrap(),
    )
    .unwrap();
}
