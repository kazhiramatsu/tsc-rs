use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

fn temporary_workspace() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tsc-rs-test-receipt-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

#[test]
fn receipt_mode_parses_operator_values() {
    assert_eq!(Mode::parse(None), Mode::Enforce);
    assert_eq!(Mode::parse(Some("fresh")), Mode::Fresh);
    assert_eq!(Mode::parse(Some("report")), Mode::Report);
    assert_eq!(Mode::parse(Some("garbage")), Mode::Enforce);
}

#[test]
fn only_enforcement_hits_are_skipped() {
    for decision in [
        Decision::Hit,
        Decision::Miss("binary"),
        Decision::Undeclared,
    ] {
        assert_eq!(Mode::Enforce.skips(decision), decision == Decision::Hit);
        assert!(!Mode::Fresh.skips(decision));
        assert!(!Mode::Report.skips(decision));
    }
}

#[test]
fn curated_table_is_small_unique_and_package_qualified() {
    assert_eq!(TEST_TARGET_INPUT_SCOPES.len(), 3);
    let mut labels = TEST_TARGET_INPUT_SCOPES
        .iter()
        .map(|scope| scope.label)
        .collect::<Vec<_>>();
    assert!(labels.iter().all(|label| label.contains("::")));
    labels.sort_unstable();
    labels.dedup();
    assert_eq!(labels.len(), TEST_TARGET_INPUT_SCOPES.len());
}

#[test]
fn green_receipt_hits_and_input_changes_name_the_miss_term() {
    let workspace = temporary_workspace();
    let source = workspace.join("crates/ci-testkit/src/lib.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, b"first").unwrap();
    let binary = workspace.join("test-binary");
    fs::write(&binary, b"binary").unwrap();
    let environment = [("TSRS_CI_TEST_WORKERS", Some("1".to_owned()))];
    let label = "test-only::input_invalidation [test]";
    // Synthetic only: the production receipt table no longer names legacy crates.
    let test_scope = TargetInputScope {
        label,
        inputs: &[InputTree("crates/ci-testkit/src/lib.rs")],
    };

    let first = prepare_with_test_scope(
        &workspace,
        label,
        &binary,
        &environment,
        1,
        Ok("rustc test"),
        &test_scope,
    );
    assert_eq!(first.decision(), Decision::Miss("absent"));
    first.publish().unwrap();
    assert_eq!(
        prepare_with_test_scope(
            &workspace,
            label,
            &binary,
            &environment,
            1,
            Ok("rustc test"),
            &test_scope,
        )
        .decision(),
        Decision::Hit
    );

    fs::write(&binary, b"changed binary").unwrap();
    assert_eq!(
        prepare_with_test_scope(
            &workspace,
            label,
            &binary,
            &environment,
            1,
            Ok("rustc test"),
            &test_scope,
        )
        .decision(),
        Decision::Miss("binary")
    );
    fs::write(&binary, b"binary").unwrap();
    assert_eq!(
        prepare_with_test_scope(
            &workspace,
            label,
            &binary,
            &[("TSRS_CI_TEST_WORKERS", Some("2".to_owned()))],
            1,
            Ok("rustc test"),
            &test_scope,
        )
        .decision(),
        Decision::Miss("environment")
    );
    assert_eq!(
        prepare_with_test_scope(
            &workspace,
            label,
            &binary,
            &environment,
            2,
            Ok("rustc test"),
            &test_scope,
        )
        .decision(),
        Decision::Miss("harness-threads")
    );
    assert_eq!(
        prepare_with_test_scope(
            &workspace,
            label,
            &binary,
            &environment,
            1,
            Ok("rustc changed"),
            &test_scope,
        )
        .decision(),
        Decision::Miss("rustc")
    );
    fs::write(source, b"second").unwrap();
    assert_eq!(
        prepare_with_test_scope(
            &workspace,
            label,
            &binary,
            &environment,
            1,
            Ok("rustc test"),
            &test_scope,
        )
        .decision(),
        Decision::Miss("inputs")
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn receipt_fingerprint_rejects_tampering() {
    let workspace = temporary_workspace();
    let binary = workspace.join("test-binary");
    fs::write(&binary, b"binary").unwrap();
    let label = "tsc-rs-types::compiler_option_number_contract [test]";
    let prepared = prepare(&workspace, label, &binary, &[], 1, Ok("rustc test"));
    prepared.publish().unwrap();
    let publication = prepared.publication.as_ref().unwrap();
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&publication.path).unwrap()).unwrap();
    receipt["outcome"] = serde_json::Value::String("failed".to_owned());
    fs::write(&publication.path, serde_json::to_vec(&receipt).unwrap()).unwrap();

    assert_eq!(
        prepare(&workspace, label, &binary, &[], 1, Ok("rustc test")).decision(),
        Decision::Miss("invalid")
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn producer_version_mismatch_forces_a_miss() {
    let workspace = temporary_workspace();
    let binary = workspace.join("test-binary");
    fs::write(&binary, b"binary").unwrap();
    let label = "tsc-rs-types::compiler_option_number_contract [test]";
    let prepared = prepare(&workspace, label, &binary, &[], 1, Ok("rustc test"));
    prepared.publish().unwrap();
    let publication = prepared.publication.as_ref().unwrap();
    let mut receipt: Receipt =
        serde_json::from_slice(&fs::read(&publication.path).unwrap()).unwrap();
    receipt.body.producer_version = "gate-tax-6-report-only-v1".to_owned();
    receipt.receipt_fingerprint_sha256 = body_fingerprint(&receipt.body).unwrap();
    fs::write(&publication.path, serde_json::to_vec(&receipt).unwrap()).unwrap();

    assert_eq!(
        prepare(&workspace, label, &binary, &[], 1, Ok("rustc test")).decision(),
        Decision::Miss("invalid")
    );
    fs::remove_dir_all(workspace).unwrap();
}
