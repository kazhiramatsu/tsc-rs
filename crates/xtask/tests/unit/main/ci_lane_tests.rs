use super::*;

#[test]
fn only_the_conformance_library_uses_a_second_harness_thread() {
    let test = |label: &str| CiTestExecutable {
        label: label.to_owned(),
        executable: PathBuf::from("test"),
        package_directory: PathBuf::from("crate"),
    };
    assert_eq!(
        ci_test_target_harness_threads(&test("tsc_conformance [lib]"), 2),
        2
    );
    assert_eq!(
        ci_test_target_harness_threads(&test("contracts [test]"), 2),
        1
    );
    assert_eq!(
        ci_test_target_harness_threads(&test("tsc_conformance [lib]"), 1),
        1
    );
}

#[test]
fn defaults_to_the_full_local_gate() {
    let args = parse_ci_args(std::iter::empty()).unwrap();
    assert_eq!(args.baseline, "origin/main");
    assert_eq!(args.lane, CiLane::All);
    assert!(!args.history_sensitive);
    assert!(!args.fresh);
}

#[test]
fn fresh_discards_only_the_failed_run_resume_journal() {
    let args = parse_ci_args(
        ["--fresh", "--baseline", "base-sha"]
            .into_iter()
            .map(str::to_owned),
    )
    .unwrap();
    assert!(args.fresh);
    assert_eq!(args.baseline, "base-sha");
    assert!(parse_ci_args(["--fresh", "--fresh"].into_iter().map(str::to_owned)).is_err());
}

#[test]
fn parses_hosted_semantic_lane_and_baseline_in_either_order() {
    for arguments in [
        ["--lane", "semantic", "--baseline", "base-sha"],
        ["--baseline", "base-sha", "--lane", "semantic"],
    ] {
        let args = parse_ci_args(arguments.into_iter().map(str::to_owned)).unwrap();
        assert_eq!(args.baseline, "base-sha");
        assert_eq!(args.lane, CiLane::Semantic);
    }
}

#[test]
fn hosted_lane_is_explicit_and_excluded_from_the_full_local_plan() {
    let hosted = parse_ci_args(["--lane", "hosted"].into_iter().map(str::to_owned)).unwrap();
    assert_eq!(hosted.lane, CiLane::Hosted);
    assert!(!hosted.history_sensitive);
    assert_eq!(
        hosted.lane.plan(),
        CiPlan {
            rust: false,
            semantic: false,
            hosted: true,
        }
    );
    assert_eq!(
        CiLane::All.plan(),
        CiPlan {
            rust: true,
            semantic: true,
            hosted: false,
        }
    );
}

#[test]
fn history_sensitive_diagnostics_are_manual_and_hosted_only() {
    let args = parse_ci_args(
        [
            "--history-sensitive",
            "--baseline",
            "base-sha",
            "--lane",
            "hosted",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert!(args.history_sensitive);
    assert_eq!(args.baseline, "base-sha");
    assert!(parse_ci_args(
        ["--lane", "all", "--history-sensitive"]
            .into_iter()
            .map(str::to_owned)
    )
    .is_err());
    assert!(parse_ci_args(
        [
            "--lane",
            "hosted",
            "--history-sensitive",
            "--history-sensitive",
        ]
        .into_iter()
        .map(str::to_owned)
    )
    .is_err());
}

#[test]
fn rejects_unknown_or_incomplete_lane_arguments() {
    assert!(parse_ci_args(["--lane", "fast"].into_iter().map(str::to_owned)).is_err());
    assert!(parse_ci_args(["--lane"].into_iter().map(str::to_owned)).is_err());
}

#[test]
fn local_test_pipeline_is_bounded_by_the_reviewed_ceiling() {
    assert_eq!(select_ci_test_workers(None, 1, 4).unwrap(), 1);
    assert_eq!(select_ci_test_workers(None, 8, 4).unwrap(), 4);
    assert_eq!(select_ci_test_workers(None, 8, 2).unwrap(), 2);
    assert_eq!(select_ci_test_workers(Some("1"), 8, 4).unwrap(), 1);
    assert_eq!(select_ci_test_workers(Some("4"), 1, 4).unwrap(), 1);
    assert!(select_ci_test_workers(Some("0"), 8, 4).is_err());
    assert!(select_ci_test_workers(Some("5"), 8, 4).is_err());
    assert!(select_ci_test_workers(Some("two"), 8, 4).is_err());
    assert!(select_ci_test_workers(None, 0, 4).is_err());
    assert!(select_ci_test_workers(None, 8, 0).is_err());
}

#[test]
fn cargo_test_artifacts_are_deduplicated_and_keep_package_roots() {
    let artifact = |name: &str, executable: &str, test: bool| {
        serde_json::json!({
            "reason": "compiler-artifact",
            "manifest_path": "workspace/crate/Cargo.toml",
            "target": { "name": name, "kind": ["test"] },
            "profile": { "test": test },
            "executable": executable,
        })
        .to_string()
    };
    let stdout = [
        artifact("one", "target/one", true),
        artifact("dependency", "target/dependency", false),
        artifact("duplicate", "target/one", true),
        serde_json::json!({ "reason": "build-finished", "success": true }).to_string(),
    ]
    .join("\n");
    let tests = cargo_test_executables(stdout.as_bytes()).unwrap();

    assert_eq!(
        tests,
        [CiTestExecutable {
            label: "one [test]".to_owned(),
            executable: PathBuf::from("target/one"),
            package_directory: PathBuf::from("workspace/crate"),
        }]
    );
}

#[test]
fn test_capture_directory_is_unique_scoped_and_removed_on_drop() {
    let workspace = std::env::temp_dir().join(format!(
        "tsc-rs-ci-capture-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&workspace).unwrap();
    let capture = CiTestCaptureDirectory::new(&workspace).unwrap();
    let path = capture.path().to_path_buf();
    assert!(path.starts_with(workspace.join("target/ci-test-output")));
    fs::write(path.join("probe.stdout"), b"captured").unwrap();
    drop(capture);
    assert!(!path.exists());
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn hosted_acceptance_and_oracle_phases_cover_h2_2c_rungs() {
    let workspace = find_workspace_root().expect("workspace");
    let h2_2c = fs::read_to_string(workspace.join("crates/xtask/src/h2_2c_acceptance.rs"))
        .expect("h2_2c_acceptance.rs");
    let main = fs::read_to_string(workspace.join("crates/xtask/src/main.rs")).expect("main.rs");
    let acceptance = extract_balanced_after(&main, "fn acceptance(", '{', '}')
        .expect("acceptance function body");

    let hosted_rungs = h2_2c
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("pub fn run_h2_"))
        .filter_map(|rest| {
            rest.split_once('(')
                .map(|(name, _)| format!("run_h2_{name}"))
        })
        .filter(|name| {
            *name != "run_h2_5g_inventory"
                && *name != "run_h2_5g_probe"
                && !name.ends_with("_owner_controls")
        })
        .collect::<Vec<_>>();
    assert!(
        !hosted_rungs.is_empty(),
        "h2_2c acceptance rungs are discoverable"
    );
    for rung in hosted_rungs {
        let call = format!("h2_2c_acceptance::{rung}(&workspace)");
        assert!(
            acceptance.contains(&call),
            "{call} must be inside fn acceptance"
        );
    }

    for entry in fs::read_dir(workspace.join("crates/oracle")).expect("oracle directory") {
        let entry = entry.expect("oracle directory entry");
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(rung) = file_name
            .strip_prefix("h2-")
            .and_then(|name| name.strip_suffix("-qualification.mjs"))
        else {
            continue;
        };
        let mut chars = rung.chars();
        let Some(major) = chars.next().and_then(|value| value.to_digit(10)) else {
            continue;
        };
        let Some(minor) = chars.next() else {
            continue;
        };
        if major > 5 || (major == 5 && minor >= 'g') {
            let phase = format!("\"h2-{rung}-oracle\"");
            assert!(main.contains(&phase), "main.rs must register {phase}");
        }
    }
}
