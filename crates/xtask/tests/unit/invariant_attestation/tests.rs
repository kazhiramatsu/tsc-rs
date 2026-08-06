use super::*;

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tsc-rs-invariant-attestation-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn sample_attestation(workspace: &Path) -> FullCorpusAttestation {
    FullCorpusAttestation {
        schema: ATTESTATION_SCHEMA,
        outcome: "passed".to_owned(),
        command: FULL_CORPUS_COMMAND.to_owned(),
        full_corpus: true,
        suites: REQUIRED_SUITES
            .iter()
            .map(|suite| (*suite).to_owned())
            .collect(),
        corpus: CorpusObservation {
            fixtures: 2,
            programs: 3,
        },
        workspace: normalize_path(&fs::canonicalize(workspace).unwrap()),
        created_unix_seconds: 1,
        controlled_inputs: Vec::new(),
    }
}

fn write_test_attestation(workspace: &Path, attestation: &FullCorpusAttestation) {
    let path = attestation_path(workspace);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_vec(attestation).unwrap()).unwrap();
}

fn scaffold_controlled_workspace(workspace: &Path) {
    for directory in [
        "crates/checker",
        "crates/syntax",
        "crates/binder",
        "crates/types",
        "crates/diagnostics",
        "crates/harness",
        "crates/conformance",
        "crates/xtask",
        "ts-tests/tests/cases/conformance",
        "vendor/typescript-6.0.3/lib",
    ] {
        let directory = workspace.join(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("controlled-input"),
            directory.as_os_str().as_encoded_bytes(),
        )
        .unwrap();
    }
    for file in [
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "ratchets/oracle-inputs.v1.json.zst",
        "ratchets/conformance-matches.v1.json.zst",
        "ratchets/host-resolution.v1.json",
        ".node-version",
        "crates/oracle/host-resolution-requests.mjs",
        "crates/oracle/program-host.mjs",
        "ratchet.toml",
        "m8-scope.json",
        "diag-families.json",
        "STAGE",
    ] {
        let path = workspace.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, file.as_bytes()).unwrap();
    }
}

#[test]
fn invalidation_removes_old_success_and_is_idempotent() {
    let workspace = temp_dir("invalidate");
    let path = attestation_path(&workspace);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"old success").unwrap();
    invalidate(&workspace).unwrap();
    assert!(!path.exists());
    invalidate(&workspace).unwrap();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn verifier_names_missing_failed_and_partial_evidence() {
    let workspace = temp_dir("red-kinds");
    let missing = verify(&workspace);
    assert!(!missing.ready);
    assert!(missing.detail.contains(" missing:"), "{}", missing.detail);

    let mut failed = sample_attestation(&workspace);
    failed.outcome = "failed".to_owned();
    write_test_attestation(&workspace, &failed);
    let failed_probe = verify(&workspace);
    assert!(!failed_probe.ready);
    assert!(
        failed_probe.detail.contains(" failed:"),
        "{}",
        failed_probe.detail
    );

    let mut partial = sample_attestation(&workspace);
    partial.suites.pop();
    write_test_attestation(&workspace, &partial);
    let partial_probe = verify(&workspace);
    assert!(!partial_probe.ready);
    assert!(
        partial_probe.detail.contains(" partial:"),
        "{}",
        partial_probe.detail
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn verifier_names_workspace_mismatch_as_stale_before_rehashing() {
    let workspace = temp_dir("stale");
    let mut attestation = sample_attestation(&workspace);
    attestation.workspace.push_str("-different");
    write_test_attestation(&workspace, &attestation);
    let probe = verify(&workspace);
    assert!(!probe.ready);
    assert!(probe.detail.contains(" stale:"), "{}", probe.detail);
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn successful_attestation_is_fresh_until_a_controlled_input_changes() {
    let workspace = temp_dir("fresh");
    scaffold_controlled_workspace(&workspace);
    write_success(&workspace, 2, 3).unwrap();
    let fresh = verify(&workspace);
    assert!(fresh.ready, "{}", fresh.detail);

    fs::write(
        workspace.join("crates/checker/controlled-input"),
        b"changed",
    )
    .unwrap();
    let stale = verify(&workspace);
    assert!(!stale.ready);
    assert!(stale.detail.contains(" stale:"), "{}", stale.detail);
    assert!(stale.detail.contains("checker"), "{}", stale.detail);
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn group_fingerprint_covers_content_names_additions_and_deletions() {
    let workspace = temp_dir("fingerprint");
    let root = workspace.join("inputs");
    fs::create_dir_all(root.join("nested")).unwrap();
    fs::write(root.join("a"), b"one").unwrap();
    let first = fingerprint_group(&workspace, "test", &["inputs"]).unwrap();

    fs::write(root.join("a"), b"two").unwrap();
    let content = fingerprint_group(&workspace, "test", &["inputs"]).unwrap();
    assert_ne!(first, content);

    fs::rename(root.join("a"), root.join("b")).unwrap();
    let renamed = fingerprint_group(&workspace, "test", &["inputs"]).unwrap();
    assert_ne!(content, renamed);

    fs::write(root.join("nested/c"), b"three").unwrap();
    let added = fingerprint_group(&workspace, "test", &["inputs"]).unwrap();
    assert_eq!(added.files, 2);
    assert_ne!(renamed, added);

    fs::remove_file(root.join("nested/c")).unwrap();
    let deleted = fingerprint_group(&workspace, "test", &["inputs"]).unwrap();
    assert_eq!(deleted, renamed);
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn repository_controlled_input_groups_are_complete_and_hashable() {
    let workspace = crate::find_workspace_root().unwrap();
    let fingerprints = controlled_input_fingerprints(&workspace).unwrap();
    assert_eq!(
        fingerprints
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "checker",
            "syntax",
            "binder",
            "types",
            "diagnostics",
            "harness",
            "conformance-options",
            "xtask",
            "rust-build",
            "corpus",
            "vendor-libs",
            "host-resolution-producer",
            "immutable-oracle-state",
            "scope-and-family-state",
        ]
    );
    assert!(fingerprints.iter().all(|entry| {
        entry.files > 0
            && entry.sha256.len() == 64
            && entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    }));
}

#[test]
fn atomic_write_never_leaves_the_temporary_name() {
    let workspace = temp_dir("atomic");
    let path = workspace.join("out/report.json");
    atomic_write(&path, b"{\"ok\":true}\n").unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"{\"ok\":true}\n");
    let siblings = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    assert_eq!(siblings, vec!["report.json"]);
    fs::remove_dir_all(workspace).unwrap();
}
