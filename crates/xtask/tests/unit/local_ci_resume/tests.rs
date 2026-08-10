use super::*;

fn snapshot(entries: &[(&str, &str)]) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        entries: entries
            .iter()
            .map(|(relative, sha256)| WorkspaceEntry {
                relative: (*relative).to_owned(),
                sha256: (*sha256).to_owned(),
            })
            .collect(),
        stability_marker: "stable".to_owned(),
    }
}

fn fingerprint(snapshot: &WorkspaceSnapshot, scope: InputScope) -> String {
    phase_fingerprint(
        snapshot,
        "lane=all",
        "tools",
        "workspace-tests",
        scope,
        "",
        &[],
    )
}

fn temporary_directory(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "tsc-rs-local-ci-resume-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn run_git(workspace: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .current_dir(workspace)
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

#[test]
fn scopes_invalidate_only_on_declared_inputs() {
    let original = snapshot(&[
        ("README.md", "readme-a"),
        ("crates/checker/src/lib.rs", "rust-a"),
        ("ratchets/profile.json", "ratchet-a"),
    ]);
    let prose_edit = snapshot(&[
        ("README.md", "readme-b"),
        ("crates/checker/src/lib.rs", "rust-a"),
        ("ratchets/profile.json", "ratchet-a"),
    ]);
    let rust_edit = snapshot(&[
        ("README.md", "readme-a"),
        ("crates/checker/src/lib.rs", "rust-b"),
        ("ratchets/profile.json", "ratchet-a"),
    ]);
    let ratchet_edit = snapshot(&[
        ("README.md", "readme-a"),
        ("crates/checker/src/lib.rs", "rust-a"),
        ("ratchets/profile.json", "ratchet-b"),
    ]);

    assert_ne!(
        fingerprint(&original, InputScope::All),
        fingerprint(&prose_edit, InputScope::All)
    );
    assert_eq!(
        fingerprint(&original, InputScope::Verification),
        fingerprint(&prose_edit, InputScope::Verification)
    );
    assert_ne!(
        fingerprint(&original, InputScope::Verification),
        fingerprint(&rust_edit, InputScope::Verification)
    );
    assert_ne!(
        fingerprint(&original, InputScope::RustFormat),
        fingerprint(&rust_edit, InputScope::RustFormat)
    );
    assert_eq!(
        fingerprint(&original, InputScope::WorkspaceAudit),
        fingerprint(&ratchet_edit, InputScope::WorkspaceAudit)
    );
    assert_ne!(
        fingerprint(&original, InputScope::WorkspaceAudit),
        fingerprint(&rust_edit, InputScope::WorkspaceAudit)
    );
    assert_ne!(
        fingerprint(&original, InputScope::Verification),
        fingerprint(&ratchet_edit, InputScope::Verification)
    );
}

#[test]
fn output_binding_must_remain_exact_for_reuse() {
    let workspace = temporary_directory("output");
    let output = workspace.join("target/m8/readiness.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"ready-a").unwrap();
    let expected = vec!["target/m8/readiness.json".to_owned()];
    let receipt = PhaseReceipt {
        fingerprint_sha256: "fingerprint".to_owned(),
        outputs: bind_outputs(&workspace, &expected).unwrap(),
    };

    assert!(receipt_is_reusable(&workspace, &receipt, "fingerprint", &expected).unwrap());
    fs::write(&output, b"ready-b").unwrap();
    assert!(!receipt_is_reusable(&workspace, &receipt, "fingerprint", &expected).unwrap());
    fs::remove_file(&output).unwrap();
    assert!(!receipt_is_reusable(&workspace, &receipt, "fingerprint", &expected).unwrap());
    fs::remove_dir_all(workspace).unwrap();
}

#[cfg(unix)]
#[test]
fn output_bindings_reject_symlinks() {
    use std::os::unix::fs::symlink;

    let workspace = temporary_directory("output-symlink");
    let target = workspace.join("readiness-real.json");
    let output = workspace.join("target/m8/readiness.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&target, b"ready").unwrap();
    symlink(&target, &output).unwrap();
    assert!(bind_outputs(&workspace, &["target/m8/readiness.json".to_owned()]).is_err());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn journal_is_exact_to_invocation_and_rejects_unknown_schema() {
    let workspace = temporary_directory("journal");
    let path = workspace.join("journal.json");
    let mut journal = Journal::new("lane=all".to_owned());
    journal.phases.insert(
        "rustfmt".to_owned(),
        PhaseReceipt {
            fingerprint_sha256: "fingerprint".to_owned(),
            outputs: Vec::new(),
        },
    );
    write_journal(&path, &journal).unwrap();

    assert_eq!(load_journal(&path, "lane=all").unwrap().phases.len(), 1);
    assert!(load_journal(&path, "lane=semantic")
        .unwrap()
        .phases
        .is_empty());

    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["unknown"] = serde_json::json!(true);
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(load_journal(&path, "lane=all").is_err());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn paths_and_phase_names_cannot_escape_the_workspace() {
    for invalid in ["", "/absolute", "../outside", "target/../../outside"] {
        assert!(
            validate_relative_path(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    for invalid in ["", "Uppercase", "has_underscore", "has/slash"] {
        assert!(
            validate_phase_name(invalid).is_err(),
            "accepted {invalid:?}"
        );
    }
    assert!(validate_relative_path("target/m8/readiness.json").is_ok());
    assert!(validate_phase_name("semantic-evidence").is_ok());
}

#[test]
fn a_failed_run_journal_reuses_an_exact_phase_and_is_cleared_on_finish() {
    let workspace = temporary_directory("round-trip");
    fs::write(workspace.join(".gitignore"), "target/\n").unwrap();
    fs::write(workspace.join("input.txt"), "exact input\n").unwrap();
    run_git(&workspace, &["init", "-q"]);
    run_git(&workspace, &["add", ".gitignore", "input.txt"]);

    let mut first = LocalCiResume::open(&workspace, "lane=test".to_owned(), true).unwrap();
    let first_runs = std::cell::Cell::new(0);
    first
        .run_phase("probe", InputScope::All, "", &[], || {
            first_runs.set(first_runs.get() + 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(first_runs.get(), 1);
    drop(first); // Model a later phase failing or the process being interrupted.

    let journal_path = workspace.join(JOURNAL_RELATIVE_PATH);
    assert!(journal_path.is_file());
    let mut second = LocalCiResume::open(&workspace, "lane=test".to_owned(), false).unwrap();
    let second_runs = std::cell::Cell::new(0);
    second
        .run_phase("probe", InputScope::All, "", &[], || {
            second_runs.set(second_runs.get() + 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(second_runs.get(), 0);
    second.finish().unwrap();
    assert!(!journal_path.exists());
    fs::remove_dir_all(workspace).unwrap();
}
