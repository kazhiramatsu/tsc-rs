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
