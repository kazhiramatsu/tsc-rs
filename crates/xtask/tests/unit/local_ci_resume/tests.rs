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

    let fci_original = snapshot(&[
        ("crates/checker/src/lib.rs", "rust-a"),
        ("ratchets/fci-readiness/fci-3c.v1.json", "envelope-a"),
        ("ratchets/fci-packet-bootstrap.v1.json", "bootstrap-a"),
    ]);
    let fci_envelope_edit = snapshot(&[
        ("crates/checker/src/lib.rs", "rust-a"),
        ("ratchets/fci-readiness/fci-3c.v1.json", "envelope-b"),
        ("ratchets/fci-packet-bootstrap.v1.json", "bootstrap-b"),
    ]);
    // Packet-control envelopes are validated by their own slice-readiness
    // proofs; no resumable verification phase reads them.
    assert_eq!(
        fingerprint(&fci_original, InputScope::Verification),
        fingerprint(&fci_envelope_edit, InputScope::Verification)
    );
    assert_ne!(
        fingerprint(&fci_original, InputScope::All),
        fingerprint(&fci_envelope_edit, InputScope::All)
    );
}

#[test]
fn node_runtime_oracle_scope_excludes_only_non_xtask_crate_rust() {
    let original = snapshot(&[
        ("crates/checker/src/lib.rs", "rust-a"),
        ("crates/xtask/src/main.rs", "xtask-a"),
        ("crates/oracle/h2-5g-qualification.mjs", "driver-a"),
        ("ratchets/h2-5g-qualification.v1.json", "artifact-a"),
        ("ts-tests/tests/cases/compiler/a.ts", "fixture-a"),
        ("vendor/typescript-6.0.3/lib/_tsc.js", "vendor-a"),
        (".node-version", "node-a"),
        ("Cargo.lock", "lock-a"),
    ]);
    let with = |relative: &str, value: &str| {
        let mut entries: Vec<(String, String)> = original
            .entries
            .iter()
            .map(|entry| (entry.relative.clone(), entry.sha256.clone()))
            .collect();
        for entry in &mut entries {
            if entry.0 == relative {
                entry.1 = value.to_owned();
            }
        }
        snapshot(
            &entries
                .iter()
                .map(|(relative, sha)| (relative.as_str(), sha.as_str()))
                .collect::<Vec<_>>(),
        )
    };
    let scope = InputScope::NodeRuntimeOracle;
    // The node drivers never read non-xtask crate Rust: a checker edit
    // keeps the freshness-proof receipt alive.
    assert_eq!(
        fingerprint(&original, scope),
        fingerprint(&with("crates/checker/src/lib.rs", "rust-b"), scope)
    );
    // Everything the phase actually consumes still re-runs it.
    for (relative, value) in [
        ("crates/xtask/src/main.rs", "xtask-b"),
        ("crates/oracle/h2-5g-qualification.mjs", "driver-b"),
        ("ratchets/h2-5g-qualification.v1.json", "artifact-b"),
        ("ts-tests/tests/cases/compiler/a.ts", "fixture-b"),
        ("vendor/typescript-6.0.3/lib/_tsc.js", "vendor-b"),
        (".node-version", "node-b"),
    ] {
        assert_ne!(
            fingerprint(&original, scope),
            fingerprint(&with(relative, value), scope),
            "{relative} must invalidate the node-runtime-oracle receipt"
        );
    }
    // Rust-only build metadata stays outside the node phase.
    assert_eq!(
        fingerprint(&original, scope),
        fingerprint(&with("Cargo.lock", "lock-b"), scope)
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

fn diagnostic_resume(
    workspace: PathBuf,
    previous: Option<PreviousComponents>,
    tool_inventory: ToolInventory,
    snapshot: WorkspaceSnapshot,
) -> LocalCiResume {
    LocalCiResume {
        journal_path: workspace.join("journal.json"),
        workspace,
        invocation: "lane=test".to_owned(),
        tool_fingerprint: tool_inventory.rolled(),
        tool_inventory,
        snapshot,
        journal: Journal::new("lane=test".to_owned()),
        previous,
        reused: 0,
        recorded: 0,
    }
}

fn string_map(entries: &[(&str, &str)]) -> BTreeMap<String, String> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn decline_names_the_divergent_environment_key() {
    // The recorded incident: TSRS_H2_5G_CHECK_SHARDS set in the failed run,
    // unset on the resume, silently zeroed reuse. The decline line must
    // name the key.
    let workspace = temporary_directory("decline-environment");
    let resume = diagnostic_resume(
        workspace.clone(),
        Some(PreviousComponents {
            tools: string_map(&[("node", "node-a")]),
            environment: string_map(&[("PATH", "path-a"), ("TSRS_H2_5G_CHECK_SHARDS", "two")]),
            snapshot: string_map(&[("crates/xtask/src/main.rs", "xtask-a")]),
        }),
        ToolInventory {
            tools: string_map(&[("node", "node-a")]),
            environment: string_map(&[("PATH", "path-a")]),
        },
        snapshot(&[("crates/xtask/src/main.rs", "xtask-a")]),
    );
    let receipt = PhaseReceipt {
        fingerprint_sha256: "stored".to_owned(),
        outputs: Vec::new(),
    };
    let message = resume.describe_decline(InputScope::All, &receipt, "current", &[]);
    assert_eq!(message, "environment TSRS_H2_5G_CHECK_SHARDS removed");

    let changed = diagnostic_resume(
        workspace.clone(),
        Some(PreviousComponents {
            tools: string_map(&[("node", "node-a")]),
            environment: string_map(&[("TSRS_H2_5G_CHECK_SHARDS", "two")]),
            snapshot: BTreeMap::new(),
        }),
        ToolInventory {
            tools: string_map(&[("node", "node-b")]),
            environment: string_map(&[("TSRS_H2_5G_CHECK_SHARDS", "four")]),
        },
        snapshot(&[]),
    );
    let message = changed.describe_decline(InputScope::All, &receipt, "current", &[]);
    assert_eq!(
        message,
        "tool node changed; environment TSRS_H2_5G_CHECK_SHARDS changed"
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn decline_names_scoped_files_and_ignores_out_of_scope_changes() {
    let workspace = temporary_directory("decline-files");
    let previous = PreviousComponents {
        tools: BTreeMap::new(),
        environment: BTreeMap::new(),
        snapshot: string_map(&[
            ("README.md", "readme-a"),
            ("crates/checker/src/lib.rs", "rust-a"),
        ]),
    };
    let inventory = ToolInventory {
        tools: BTreeMap::new(),
        environment: BTreeMap::new(),
    };
    let resume = diagnostic_resume(
        workspace.clone(),
        Some(previous),
        inventory,
        snapshot(&[
            ("README.md", "readme-b"),
            ("crates/checker/src/lib.rs", "rust-b"),
        ]),
    );
    let receipt = PhaseReceipt {
        fingerprint_sha256: "stored".to_owned(),
        outputs: Vec::new(),
    };
    // Markdown sits outside Verification: only the Rust divergence is named.
    let message = resume.describe_decline(InputScope::Verification, &receipt, "current", &[]);
    assert_eq!(
        message,
        "file(s) in scope: crates/checker/src/lib.rs changed"
    );
    let message = resume.describe_decline(InputScope::WorkspaceAudit, &receipt, "current", &[]);
    assert_eq!(
        message,
        "file(s) in scope: crates/checker/src/lib.rs changed"
    );
    // With no divergent component inside the scope the fallback line
    // reports honestly instead of guessing.
    let quiet = diagnostic_resume(
        workspace.clone(),
        Some(PreviousComponents {
            tools: BTreeMap::new(),
            environment: BTreeMap::new(),
            snapshot: string_map(&[("README.md", "readme-a")]),
        }),
        ToolInventory {
            tools: BTreeMap::new(),
            environment: BTreeMap::new(),
        },
        snapshot(&[("README.md", "readme-b")]),
    );
    let message = quiet.describe_decline(InputScope::Verification, &receipt, "current", &[]);
    assert_eq!(
        message,
        "the phase definition or an earlier interrupted run's inputs changed (no recorded component differs)"
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn decline_names_stale_outputs_when_inputs_match() {
    let workspace = temporary_directory("decline-outputs");
    let output = workspace.join("target/m8/readiness.json");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(&output, b"ready-a").unwrap();
    let expected = vec!["target/m8/readiness.json".to_owned()];
    let receipt = PhaseReceipt {
        fingerprint_sha256: "same".to_owned(),
        outputs: bind_outputs(&workspace, &expected).unwrap(),
    };
    let resume = diagnostic_resume(
        workspace.clone(),
        None,
        ToolInventory {
            tools: BTreeMap::new(),
            environment: BTreeMap::new(),
        },
        snapshot(&[]),
    );
    fs::write(&output, b"ready-b").unwrap();
    let message = resume.describe_decline(InputScope::All, &receipt, "same", &expected);
    assert_eq!(
        message,
        "recorded output(s) changed on disk: target/m8/readiness.json"
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn schema_one_journals_are_replaced_silently_and_component_maps_round_trip() {
    let workspace = temporary_directory("journal-schema");
    let path = workspace.join("journal.json");
    let mut journal = Journal::new("lane=all".to_owned());
    journal.tools = string_map(&[("node", "node-a")]);
    journal.environment = string_map(&[("PATH", "path-a")]);
    journal.snapshot = string_map(&[("input.txt", "input-a")]);
    journal.phases.insert(
        "rustfmt".to_owned(),
        PhaseReceipt {
            fingerprint_sha256: "fingerprint".to_owned(),
            outputs: Vec::new(),
        },
    );
    write_journal(&path, &journal).unwrap();

    let loaded = load_journal(&path, "lane=all").unwrap();
    assert_eq!(loaded.tools, journal.tools);
    assert_eq!(loaded.environment, journal.environment);
    assert_eq!(loaded.snapshot, journal.snapshot);
    assert_eq!(loaded.phases.len(), 1);

    // A schema-1 journal (no component maps) parses via the defaults and
    // is replaced silently by the schema check, never an error.
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["schema"] = serde_json::json!(1);
    value.as_object_mut().unwrap().remove("tools");
    value.as_object_mut().unwrap().remove("environment");
    value.as_object_mut().unwrap().remove("snapshot");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let replaced = load_journal(&path, "lane=all").unwrap();
    assert!(replaced.phases.is_empty());
    assert_eq!(replaced.schema, JOURNAL_SCHEMA);
    fs::remove_dir_all(workspace).unwrap();
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
