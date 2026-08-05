use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;
use tsc_program::{LibraryCatalog, H0_SUPPORTED_CONFIG_OPTIONS};

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Qualification {
    schema: u32,
    status: String,
    typescript_version: String,
    scope: Scope,
    option_profile: OptionProfile,
    host_profiles: Vec<HostProfile>,
    library_profile: LibraryProfile,
    suite_evidence: SuiteEvidence,
    resource_profiles: Vec<ResourceProfile>,
    release_gate: ReleaseGate,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scope {
    mode: String,
    emit: bool,
    build: bool,
    watch: bool,
    incremental: bool,
    language_service: bool,
    unsupported_policy: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OptionProfile {
    config_options: Vec<String>,
    command_line_options: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostProfile {
    id: String,
    os: String,
    arch: String,
    case_profile: String,
    qualification: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LibraryProfile {
    logical_entries: usize,
    distinct_catalog_files: usize,
    embedded_files: usize,
    compatibility_file: String,
    runtime_vendor_directory_required: bool,
    runtime_node_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteEvidence {
    host_resolution_rows: usize,
    host_resolution_closed: usize,
    compiler_plans: usize,
    compiler_loaded: usize,
    compiler_executed: usize,
    compiler_failures: usize,
    compiler_audit_wall_seconds: f64,
    project_plans: usize,
    project_h0_compatible: usize,
    project_executed: usize,
    project_declared_non_scope: usize,
    project_failures: usize,
    cli_oracle_matrices: usize,
    cli_oracle_failures: usize,
    program_oracle_contracts: usize,
    program_oracle_failures: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceProfile {
    id: String,
    workload: String,
    measurement_backend: String,
    cargo_build_jobs: usize,
    cold_wall_seconds: Option<f64>,
    warm_wall_seconds: Option<f64>,
    cold_max_rss_bytes: Option<u64>,
    warm_max_rss_bytes: Option<u64>,
    ceilings: ResourceCeilings,
    measured_on: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceCeilings {
    cold_wall_seconds: Option<f64>,
    warm_wall_seconds: Option<f64>,
    max_rss_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReleaseGate {
    local_acceptance_required: bool,
    github_actions_policy: String,
    publish_only_after_green: bool,
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn qualification() -> Qualification {
    serde_json::from_slice(
        &fs::read(workspace_root().join("ratchets/h0-qualification.v1.json"))
            .expect("read H0 qualification artifact"),
    )
    .expect("H0 qualification artifact has the exact schema")
}

#[test]
fn frozen_h0_profile_matches_the_executable_boundaries() {
    let profile = qualification();
    assert_eq!(profile.schema, 1);
    assert_eq!(profile.status, "frozen");
    assert_eq!(profile.typescript_version, "6.0.3");
    assert_eq!(profile.scope.mode, "single-project-no-emit");
    assert!(!profile.scope.emit);
    assert!(!profile.scope.build);
    assert!(!profile.scope.watch);
    assert!(!profile.scope.incremental);
    assert!(!profile.scope.language_service);
    assert_eq!(profile.scope.unsupported_policy, "typed-fail-closed");

    assert_eq!(
        profile
            .option_profile
            .config_options
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        H0_SUPPORTED_CONFIG_OPTIONS
    );
    assert_eq!(
        profile.option_profile.command_line_options,
        [
            "--noEmit",
            "--project",
            "--ignoreConfig",
            "--pretty",
            "--version",
        ]
    );

    assert_eq!(profile.host_profiles.len(), 2);
    assert_eq!(profile.host_profiles[0].id, "macos-arm64-local");
    assert_eq!(profile.host_profiles[0].os, "macos");
    assert_eq!(profile.host_profiles[0].arch, "aarch64");
    assert_eq!(profile.host_profiles[0].case_profile, "native-detected");
    assert_eq!(
        profile.host_profiles[0].qualification,
        "full-local-cli-and-filesystem"
    );
    assert_eq!(profile.host_profiles[1].id, "windows-x64-hosted-canary");
    assert_eq!(profile.host_profiles[1].os, "windows");
    assert_eq!(profile.host_profiles[1].arch, "x86_64");
    assert_eq!(profile.host_profiles[1].case_profile, "case-insensitive");
    assert_eq!(
        profile.host_profiles[1].qualification,
        "focused-host-and-program-filesystem"
    );

    let catalog = LibraryCatalog::typescript_6_0_3("/profile/lib");
    assert_eq!(
        profile.library_profile.logical_entries,
        catalog.logical_entry_count()
    );
    assert_eq!(
        profile.library_profile.distinct_catalog_files,
        catalog.distinct_file_count()
    );
    assert_eq!(profile.library_profile.embedded_files, 108);
    assert_eq!(profile.library_profile.compatibility_file, "lib.d.ts");
    assert!(!profile.library_profile.runtime_vendor_directory_required);
    assert!(!profile.library_profile.runtime_node_required);

    let suites = profile.suite_evidence;
    assert_eq!(suites.host_resolution_rows, 241);
    assert_eq!(suites.host_resolution_closed, suites.host_resolution_rows);
    assert_eq!(suites.compiler_plans, 7_276);
    assert_eq!(suites.compiler_loaded, suites.compiler_plans);
    assert_eq!(suites.compiler_executed, suites.compiler_plans);
    assert_eq!(suites.compiler_failures, 0);
    assert!(suites.compiler_audit_wall_seconds <= 900.0);
    assert_eq!(suites.project_plans, 632);
    assert_eq!(
        suites.project_h0_compatible + suites.project_declared_non_scope,
        suites.project_plans
    );
    assert_eq!(suites.project_executed, suites.project_h0_compatible);
    assert_eq!(suites.project_failures, 0);
    assert_eq!(suites.cli_oracle_matrices, 10);
    assert_eq!(suites.cli_oracle_failures, 0);
    assert_eq!(suites.program_oracle_contracts, 5);
    assert_eq!(suites.program_oracle_failures, 0);

    assert_eq!(profile.resource_profiles.len(), 2);
    for resource in profile.resource_profiles {
        assert!(!resource.id.is_empty());
        assert!(!resource.workload.is_empty());
        assert!(!resource.measurement_backend.is_empty());
        assert_eq!(resource.cargo_build_jobs, 2);
        if let (Some(measured), Some(ceiling)) = (
            resource.cold_wall_seconds,
            resource.ceilings.cold_wall_seconds,
        ) {
            assert!(measured <= ceiling);
        }
        if let (Some(measured), Some(ceiling)) = (
            resource.warm_wall_seconds,
            resource.ceilings.warm_wall_seconds,
        ) {
            assert!(measured <= ceiling);
        }
        if let Some(ceiling) = resource.ceilings.max_rss_bytes {
            if let Some(measured) = resource.cold_max_rss_bytes {
                assert!(measured <= ceiling);
            }
            if let Some(measured) = resource.warm_max_rss_bytes {
                assert!(measured <= ceiling);
            }
        }
        assert_eq!(resource.measured_on, "2026-08-06");
    }

    assert!(profile.release_gate.local_acceptance_required);
    assert_eq!(
        profile.release_gate.github_actions_policy,
        "classifier-plus-focused-windows-canary"
    );
    assert!(profile.release_gate.publish_only_after_green);
}

#[test]
fn standalone_binary_loads_the_embedded_default_library_without_node() {
    let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "tsc-rs-h0-qualification-{}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&root).expect("create standalone binary tree");
    fs::write(
        root.join("main.ts"),
        "const values: Array<number> = [1, 2, 3];\n",
    )
    .expect("write standalone source");

    let output = Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .current_dir(&root)
        .args(["--noEmit", "--ignoreConfig", "main.ts"])
        .output()
        .expect("run standalone tsc-rs binary");
    let cleanup = fs::remove_dir_all(&root);
    assert!(
        output.status.success(),
        "standalone compiler failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    cleanup.expect("remove standalone binary tree");
}
