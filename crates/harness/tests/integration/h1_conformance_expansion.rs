use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tsc_harness::upstream_suites::h1_conformance::{
    check_recorded_manifest, generate_manifest, render_manifest, validate_manifest,
    ConformanceExpansionManifest, ConformanceExpansionSummary, ReferenceBaselineState,
    CONTRACT_RELATIVE_PATH, INDEPENDENT_ORACLE_RELATIVE_PATH,
};
use tsc_harness::upstream_suites::{ExecutionState, SourceEncoding};

const RECORDED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../vendor/typescript-6.0.3/conformance-suite-expansion.v1.json"
));
const MANIFEST_SHA256: &str = "924d4007b3ac93a3ee57032ea6089b649bab2902e30ee64cff02f4c9404b7bbd";
const CONTRACT_SHA256: &str = "7066837207d71f1fc8c4bb1cfe6263537b1fe0d6d82de9970ba74fe1c3338963";
const ORACLE_SHA256: &str = "ec6e7de0883caad8400f2158a6a14675e798efcc1c7a5440b2465825f2b99528";
const NOT_ENUMERATED_PATH: &str =
    "parser/ecmascript5/Statements/ReturnStatements/parserReturnStatement4.js";

static PARSED: OnceLock<ConformanceExpansionManifest> = OnceLock::new();
static GENERATED: OnceLock<ConformanceExpansionManifest> = OnceLock::new();

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("harness crate must be inside workspace")
        .to_path_buf()
}

fn parsed() -> &'static ConformanceExpansionManifest {
    PARSED.get_or_init(|| {
        serde_json::from_slice(RECORDED)
            .expect("H1 conformance expansion must be strict, valid JSON")
    })
}

fn generated() -> &'static ConformanceExpansionManifest {
    GENERATED.get_or_init(|| {
        generate_manifest(&workspace())
            .unwrap_or_else(|error| panic!("failed to reconstruct conformance runner: {error}"))
    })
}

fn expected_summary() -> ConformanceExpansionSummary {
    ConformanceExpansionSummary {
        source_files: 5_908,
        source_bytes: 3_825_804,
        unique_blobs: 5_862,
        enumerated_fixtures: 5_907,
        not_enumerated_sources: 1,
        default_fixtures: 4_809,
        matrix_fixtures: 1_098,
        cases: 7_697,
        normal_units: 8_055,
        virtual_configs: 27,
        present_empty_units: 14,
        missing_content_units: 0,
        link_directives: 0,
        document_symlink_directives: 0,
        document_symlink_paths: 0,
        runner_observations: 6,
        case_observations: 46_182,
        not_run_cases: 7_697,
        not_run_case_observations: 46_182,
        execution_results_recorded: 0,
        reference_baselines_compared: 0,
    }
}

#[test]
fn recorded_manifest_is_the_exact_complete_not_run_expansion() {
    assert_eq!(format!("{:x}", Sha256::digest(RECORDED)), MANIFEST_SHA256);
    assert_eq!(parsed().summary, expected_summary());
    assert_eq!(validate_manifest(parsed()).unwrap(), expected_summary());
    assert_eq!(parsed().sources.len(), 5_908);
    assert_eq!(parsed().fixtures.len(), 5_907);
    assert_eq!(parsed().cases.len(), 7_697);
    assert!(parsed().cases.iter().all(|case| {
        case.initial_execution_state == ExecutionState::NotRun
            && case.reference_baseline_state == ReferenceBaselineState::ContentNotVendoredOrCompared
            && case.observations == [0, 1, 2, 3, 4, 5]
    }));
    assert_eq!(parsed().runner_contract.observations.len(), 6);
    assert!(parsed()
        .runner_contract
        .observations
        .iter()
        .all(|observation| observation.initial_execution_state == ExecutionState::NotRun));
}

#[test]
fn runner_enumeration_excludes_only_the_pinned_javascript_control() {
    let row = &parsed().not_enumerated_sources[0];
    let source = &parsed().sources[row.source as usize];
    assert_eq!(row.source, 4_344);
    assert_eq!(source.path, NOT_ENUMERATED_PATH);
    assert_eq!(source.bytes, 62);
    assert_eq!(
        source.sha256,
        "acf079bf902587a88834d8fcd2d9060afbc96e052f71447613d421004596930b"
    );
    assert_eq!(
        source.git_blob_sha1,
        "011e715b8050deed3e5c72d99260696fcd0efe9a"
    );

    let enumerated = parsed()
        .fixtures
        .iter()
        .map(|fixture| fixture.source)
        .collect::<BTreeSet<_>>();
    assert!(!enumerated.contains(&row.source));
    assert_eq!(enumerated.len(), parsed().sources.len() - 1);
}

#[test]
fn configuration_and_encoding_distributions_are_frozen() {
    let mut configurations = BTreeMap::new();
    let mut encodings = BTreeMap::new();
    for fixture in &parsed().fixtures {
        *configurations
            .entry(fixture.configurations.len())
            .or_insert(0_usize) += 1;
        let encoding = match fixture.encoding {
            SourceEncoding::Utf8 => "utf-8",
            SourceEncoding::Utf8Bom => "utf-8-bom",
            SourceEncoding::Utf16Le => "utf-16le",
            SourceEncoding::Utf16Be => "utf-16be",
        };
        *encodings.entry(encoding).or_insert(0_usize) += 1;
    }
    assert_eq!(
        configurations,
        BTreeMap::from([
            (1, 4_809),
            (2, 818),
            (3, 77),
            (4, 142),
            (5, 14),
            (6, 14),
            (8, 7),
            (9, 24),
            (13, 1),
            (14, 1),
        ])
    );
    assert_eq!(
        encodings,
        BTreeMap::from([("utf-8", 5_305), ("utf-8-bom", 602)])
    );
}

#[test]
fn producer_schema_and_independent_oracle_identities_are_live() {
    let workspace = workspace();
    let contract = fs::read(workspace.join(CONTRACT_RELATIVE_PATH)).unwrap();
    let oracle = fs::read(workspace.join(INDEPENDENT_ORACLE_RELATIVE_PATH)).unwrap();
    assert_eq!(format!("{:x}", Sha256::digest(&contract)), CONTRACT_SHA256);
    assert_eq!(format!("{:x}", Sha256::digest(&oracle)), ORACLE_SHA256);
    assert_eq!(parsed().contract.path, CONTRACT_RELATIVE_PATH);
    assert_eq!(parsed().contract.sha256, CONTRACT_SHA256);
    assert_eq!(
        parsed().independent_oracle.path,
        INDEPENDENT_ORACLE_RELATIVE_PATH
    );
    assert_eq!(parsed().independent_oracle.sha256, ORACLE_SHA256);
    serde_json::from_slice::<Value>(&contract).expect("contract must be valid JSON");

    for source in &parsed().producer_sources {
        let bytes = fs::read(workspace.join(&source.path)).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), source.sha256);
    }
}

#[test]
fn rust_and_node_reconstructions_match_every_recorded_row() {
    assert_eq!(generated(), parsed());
    assert_eq!(render_manifest(generated()).unwrap(), RECORDED);
    assert_eq!(
        check_recorded_manifest(&workspace()).unwrap(),
        expected_summary()
    );

    let output = Command::new("node")
        .arg(INDEPENDENT_ORACLE_RELATIVE_PATH)
        .arg("--check")
        .current_dir(workspace())
        .output()
        .expect("failed to run independent Node conformance expansion oracle");
    assert!(
        output.status.success(),
        "Node oracle failed:\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout)
        .contains("sources=5908 fixtures=5907 cases=7697 observations=46182"));
}

#[test]
fn inventory_contains_no_execution_or_baseline_result_claim_keys() {
    fn visit(value: &Value) {
        match value {
            Value::Object(object) => {
                for (key, child) in object {
                    assert!(
                        !matches!(
                            key.as_str(),
                            "result"
                                | "results"
                                | "pass"
                                | "passed"
                                | "skip"
                                | "skipped"
                                | "baseline_content"
                                | "baseline_result"
                        ),
                        "inventory-only expansion contains result-like key {key:?}"
                    );
                    visit(child);
                }
            }
            Value::Array(values) => values.iter().for_each(visit),
            _ => {}
        }
    }

    let value: Value = serde_json::from_slice(RECORDED).unwrap();
    visit(&value);
}
