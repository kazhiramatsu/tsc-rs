use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct TestWorkspace {
    root: PathBuf,
}

impl TestWorkspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tsc-rs-fuzz-preflight-{}-{timestamp}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("ratchets")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/input.txt"), "pinned\n").unwrap();
        Self { root }
    }

    fn pinned_hash(&self) -> String {
        sha256_file(&self.root.join("src/input.txt")).unwrap()
    }

    fn write_valid_manifests(
        &self,
        manifest_status: &str,
        branch_status: &str,
        deviation_status: &str,
    ) {
        let hash = self.pinned_hash();
        let branch_blocks = if branch_status == "ready" {
            "[]"
        } else {
            "[\"M9.1\"]"
        };
        let deviation_blocks = if deviation_status == "ready" {
            "[]"
        } else {
            "[\"M9.1\"]"
        };
        let exact_hashes = if deviation_status == "ready" {
            format!(
                r#"
    ,"input_sha256": "{hash}"
    ,"oracle_outcome_sha256": "{hash}"
    ,"rust_outcome_sha256": "{hash}"
    ,"positive_canary_sha256": "{hash}"
    ,"adjacent_negative_canary_sha256": "{hash}""#
            )
        } else {
            String::new()
        };
        let domain = format!(
            r#"schema = 1
status = "{manifest_status}"

[[source_references]]
path = "src/input.txt"
sha256 = "{hash}"

[[branches]]
id = "domain.branch.assignment"
status = "{branch_status}"
detail = "legacy assignment branch"
blocks = {branch_blocks}
evidence = ["generated_source arm 0"]
role = "legacy-smoke-only"
witness_seed = 1
witness_case = 0
strata = ["relations"]
script_kinds = ["ts"]
topologies = ["single-file"]
options = ["default"]

[[requirements]]
id = "domain.requirement.syntax"
status = "ready"
detail = "syntax stratum is inventoried"
blocks = []
evidence = ["manifest"]
"#
        );
        fs::write(self.root.join(DOMAIN_MANIFEST_REL), domain).unwrap();
        let oracle = format!(
            r#"{{
  "schema": 1,
  "status": "{manifest_status}",
  "source_references": [{{"path":"src/input.txt","sha256":"{hash}"}}],
  "deviations": [{{
    "id": "oracle.deviation.async-sync",
    "status": "{deviation_status}",
    "detail": "recorded M8 crash shape",
    "blocks": {deviation_blocks},
    "evidence": ["m8-readiness row 1"],
    "source_contract": "m8-readiness.md#recorded"{exact_hashes}
  }}]
}}"#
        );
        fs::write(self.root.join(ORACLE_DEVIATIONS_REL), oracle).unwrap();
        let report = format!(
            r#"{{
  "schema": 1,
  "status": "{manifest_status}",
  "source_references": [{{"path":"src/input.txt","sha256":"{hash}"}}],
  "checks": [{{
    "id": "preflight.true-replay",
    "status": "ready",
    "detail": "replay surface inventoried",
    "blocks": [],
    "evidence": ["audit"]
  }}]
}}"#
        );
        fs::write(self.root.join(PREFLIGHT_REPORT_REL), report).unwrap();
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn pending_and_unknown_are_reported_without_becoming_ready() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "pending", "unknown");

    let inventory = load_preflight_inventory(&workspace.root).unwrap();
    assert!(!inventory.is_ready());
    assert_eq!(inventory.summary().total_checks, 7);
    assert_eq!(inventory.summary().ready_checks, 2);
    assert_eq!(inventory.summary().pending_checks, 4);
    assert_eq!(inventory.summary().unknown_checks, 1);
    assert_eq!(
        inventory.blocker_ids().collect::<Vec<_>>(),
        [
            "manifest.domain.status",
            "manifest.oracle-deviations.status",
            "manifest.preflight.status",
            "domain.branch.assignment",
            "oracle.deviation.async-sync"
        ]
    );
    let rendered = inventory.summary().render_text();
    assert!(rendered.contains("[pending] domain.branch.assignment"));
    assert!(rendered.contains("[unknown] oracle.deviation.async-sync"));
    assert!(inventory.require_ready().is_err());
}

#[test]
fn all_rows_ready_still_block_while_manifests_are_draft() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "ready", "ready");

    let inventory = load_preflight_inventory(&workspace.root).unwrap();
    assert!(!inventory.is_ready());
    assert!(inventory.require_ready().is_err());
    assert_eq!(
        inventory.blocker_ids().collect::<Vec<_>>(),
        [
            "manifest.domain.status",
            "manifest.oracle-deviations.status",
            "manifest.preflight.status",
        ]
    );
}

#[test]
fn frozen_is_unsupported_in_report_only_schema_one() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("frozen", "ready", "ready");

    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("report-only schema 1 accepts only draft"),
        "{error}"
    );
}

#[test]
fn unknown_domain_values_cannot_hide_in_an_empty_ready_branch() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "ready", "ready");
    let path = workspace.root.join(DOMAIN_MANIFEST_REL);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("strata = [\"relations\"]", "strata = []");
    fs::write(path, text).unwrap();

    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("has no strata"), "{error}");
}

#[test]
fn ready_oracle_deviation_requires_every_exact_hash() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "ready", "ready");
    let path = workspace.root.join(ORACLE_DEVIATIONS_REL);
    let text = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|line| !line.contains("\"positive_canary_sha256\""))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(path, text).unwrap();

    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("lacks an exact input/outcome/canary hash"));
}

#[test]
fn source_reference_hash_drift_is_an_operational_error() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "pending", "unknown");
    fs::write(workspace.root.join("src/input.txt"), "drift\n").unwrap();

    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("hash mismatch"), "{error}");
}

#[test]
fn malformed_missing_and_unknown_status_are_operational_errors() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "pending", "unknown");
    let report_path = workspace.root.join(PREFLIGHT_REPORT_REL);
    fs::write(&report_path, "{").unwrap();
    assert!(load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string()
        .contains("malformed"));

    workspace.write_valid_manifests("draft", "pending", "unknown");
    fs::remove_file(&report_path).unwrap();
    assert!(load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string()
        .contains("cannot read"));

    workspace.write_valid_manifests("draft", "pending", "unknown");
    let text = fs::read_to_string(&report_path)
        .unwrap()
        .replace("\"status\": \"ready\"", "\"status\": \"waived\"");
    fs::write(&report_path, text).unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown status"), "{error}");

    workspace.write_valid_manifests("draft", "pending", "unknown");
    let text = fs::read_to_string(&report_path)
        .unwrap()
        .replace("\"status\": \"draft\"", "\"status\": \"reviewed\"");
    fs::write(&report_path, text).unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsupported manifest status"), "{error}");
}

#[test]
fn duplicate_check_ids_across_manifests_are_rejected() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "pending", "unknown");
    let path = workspace.root.join(PREFLIGHT_REPORT_REL);
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("preflight.true-replay", "domain.branch.assignment");
    fs::write(path, text).unwrap();

    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("duplicated across manifests"), "{error}");
}

#[test]
fn schema_extensions_and_wrapping_schema_values_are_rejected() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "pending", "unknown");
    let report_path = workspace.root.join(PREFLIGHT_REPORT_REL);
    let text = fs::read_to_string(&report_path)
        .unwrap()
        .replace("\"schema\": 1,", "\"schema\": 1, \"surprise\": true,");
    fs::write(&report_path, text).unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field"), "{error}");

    workspace.write_valid_manifests("draft", "pending", "unknown");
    let domain_path = workspace.root.join(DOMAIN_MANIFEST_REL);
    let text = fs::read_to_string(&domain_path)
        .unwrap()
        .replace("schema = 1", "schema = 4294967297");
    fs::write(&domain_path, text).unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsigned 32-bit integer"), "{error}");
}

#[test]
fn unsafe_invalid_empty_and_duplicate_source_references_are_rejected() {
    let workspace = TestWorkspace::new();
    workspace.write_valid_manifests("draft", "pending", "unknown");
    let report_path = workspace.root.join(PREFLIGHT_REPORT_REL);
    let text = fs::read_to_string(&report_path).unwrap().replace(
        "\"path\":\"src/input.txt\"",
        "\"path\":\"../src/input.txt\"",
    );
    fs::write(&report_path, text).unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("unsafe source reference path"), "{error}");

    workspace.write_valid_manifests("draft", "pending", "unknown");
    let text = fs::read_to_string(&report_path)
        .unwrap()
        .replace(&workspace.pinned_hash(), &"A".repeat(64));
    fs::write(&report_path, text).unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("lowercase 64-character SHA-256"), "{error}");

    workspace.write_valid_manifests("draft", "pending", "unknown");
    let text = fs::read_to_string(&report_path).unwrap();
    let reference = format!(
        r#""source_references": [{{"path":"src/input.txt","sha256":"{}"}}]"#,
        workspace.pinned_hash()
    );
    fs::write(
        &report_path,
        text.replace(&reference, r#""source_references": []"#),
    )
    .unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("requires at least one source reference"),
        "{error}"
    );

    workspace.write_valid_manifests("draft", "pending", "unknown");
    let text = fs::read_to_string(&report_path).unwrap();
    let hash = workspace.pinned_hash();
    let reference = format!(r#"{{"path":"src/input.txt","sha256":"{hash}"}}"#);
    let aliased_reference = format!(r#"{{"path":"src/./input.txt","sha256":"{hash}"}}"#);
    fs::write(
        &report_path,
        text.replace(
            &format!("[{reference}]"),
            &format!("[{reference},{aliased_reference}]"),
        ),
    )
    .unwrap();
    let error = load_preflight_inventory(&workspace.root)
        .unwrap_err()
        .to_string();
    assert!(error.contains("repeats source reference"), "{error}");
}

#[test]
fn checked_in_manifests_load_as_an_explicitly_blocked_draft() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inventory = load_preflight_inventory(workspace).unwrap();

    assert_eq!(inventory.domain().status, ManifestStatus::Draft);
    assert_eq!(inventory.oracle_deviations().status, ManifestStatus::Draft);
    assert_eq!(inventory.preflight_report().status, ManifestStatus::Draft);
    assert!(!inventory.is_ready());
    assert!(inventory.summary().pending_checks > 0);
    assert!(inventory.summary().unknown_checks > 0);
    assert_eq!(
        inventory.blocker_ids().take(3).collect::<Vec<_>>(),
        [
            "manifest.domain.status",
            "manifest.oracle-deviations.status",
            "manifest.preflight.status",
        ]
    );
}
