//! EF8 (H2.8a-A-RES-EMITTER-FINAL): the minimum additional output-axis inputs P1–P8
//! (`docs/design/greenfield/slices/emitter-final-batch/ef8/README.md` §5) as complete
//! commands. P1–P7 (target/module families with no tuple, BOM on every map product,
//! removeComments with declaration maps under outFile, both layouts / noEmitOnError
//! with outFile maps, reversed roots under outDir, escaped and non-BMP names into
//! d.ts maps, a case-folding declarationDir collision) replay through the memory
//! sink; P8 (multi-product write sequences with injected faults) through the
//! production `FsOutputSink` protocol. Expectations are TS-produced twice
//! (`scripts/observe-output-matrix.mjs`).

#[test]
fn output_matrix_matches_complete_typescript_observations() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-matrix.json"))
            .expect("frozen TypeScript output-matrix observations");
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    assert_eq!(artifact["cases"].as_array().unwrap().len(), 22);
    super::h2_7c_declaration_blocking::assert_cases_with_reporting(&artifact, true);
}

#[test]
fn output_matrix_filesystem_matches_complete_typescript_observations() {
    let fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("../fixtures/output-matrix-filesystem.json"))
            .expect("frozen TypeScript output-matrix filesystem observations");
    assert_eq!(fixture["typescript"], "6.0.3");
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 4);
    super::h2_8a_output_filesystem::assert_filesystem_cases(&fixture);
}
