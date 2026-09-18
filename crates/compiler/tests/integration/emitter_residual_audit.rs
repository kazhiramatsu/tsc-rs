//! Focused reproductions found by the integration audit; frozen original
//! complete commands use the same comparator as the full output matrix.

#[path = "h2_7d_original_corpus_shared.rs"]
#[allow(dead_code)]
mod original_corpus;

#[test]
fn anonymous_class_names_match_complete_original_commands() {
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    let roster: Value = serde_json::from_str(include_str!(
        "../fixtures/emitter-audit-class-regressions.json"
    ))
    .unwrap();
    let rows = roster["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 24);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut cases = Vec::new();
    for (name, hash) in roster["fixtures"].as_object().unwrap() {
        let bytes = std::fs::read(root.join(name)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            hash.as_str().unwrap()
        );
        let fixture: Value = serde_json::from_slice(&bytes).unwrap();
        for row in rows.iter().filter(|row| row["fixture"] == *name) {
            let matching: Vec<_> = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|case| case["case_id"] == row["case_id"])
                .collect();
            assert_eq!(matching.len(), 1);
            cases.push(matching[0].clone());
        }
    }
    assert_eq!(cases.len(), 24);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(&json!({"cases":cases}), true);
}

#[test]
fn javascript_regressions_match_complete_original_commands() {
    let ids = [
        "typescript-6.0.3/compiler/jsFileCompilationAwaitModifier.ts#default",
        "typescript-6.0.3/conformance/jsdoc/declarations/jsDeclarationsTypeAliases.ts#default",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(
        original_corpus::assert_output_matrix_projection(&root, &ids).len(),
        ids.len()
    );
}

#[test]
fn empty_block_comments_match_complete_typescript_commands() {
    use serde_json::{json, Value};
    let artifact: Value =
        serde_json::from_slice(include_bytes!("../fixtures/empty-block-comments.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 72);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        let result = std::panic::catch_unwind(|| {
            super::h2_7c_declaration_blocking::assert_cases_with_command_inspection(
                &json!({"cases": [case]}),
                true,
                super::h2_8a_import_helpers::capture_complete_command,
            );
        });
        if result.is_err() {
            failures.push(id);
        } else {
            eprintln!("empty-block comments EXACT x2 {id}");
        }
    }
    eprintln!(
        "empty-block comments SUMMARY exact={} failed={} selected={}",
        cases.len() - failures.len(),
        failures.len(),
        cases.len()
    );
    assert!(
        failures.is_empty(),
        "complete empty-block comment failures: {failures:?}"
    );
}

#[test]
fn historical_session_refusals_match_complete_typescript_commands() {
    let artifact: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../fixtures/emitter-session-retirements.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 4);
    super::h2_7c_declaration_blocking::assert_cases_with_command_inspection(
        &artifact,
        true,
        super::h2_8a_import_helpers::capture_complete_command,
    );
}
