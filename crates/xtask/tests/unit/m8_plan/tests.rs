use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static TEMP_REPO_SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct TempRepo(PathBuf);

impl TempRepo {
    fn new(label: &str) -> Self {
        let sequence = TEMP_REPO_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "tsc-rs-m8-plan-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        git_test(&path, &["init", "-q"]);
        git_test(&path, &["config", "user.email", "tests@example.invalid"]);
        git_test(&path, &["config", "user.name", "tsc-rs tests"]);
        Self(path)
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git_test(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn draft_parser_requires_exact_inputs_and_positive_cache_bound() {
    let parsed = parse_draft_args(
        [
            "--conformance-json",
            "conformance.json",
            "--out",
            "plan.json",
            "--sibling-fixture",
            "conformance/sibling.ts",
            "--max-lib-cache-buckets",
            "2",
        ]
        .into_iter()
        .map(str::to_owned),
    )
    .unwrap();
    assert_eq!(parsed.conformance_json, PathBuf::from("conformance.json"));
    assert_eq!(parsed.out, PathBuf::from("plan.json"));
    assert_eq!(
        parsed.sibling_fixtures,
        vec!["conformance/sibling.ts".to_owned()]
    );
    assert_eq!(parsed.max_lib_cache_buckets, 2);
    assert!(parse_draft_args(
        [
            "--conformance-json",
            "c.json",
            "--out",
            "p.json",
            "--max-lib-cache-buckets",
            "0"
        ]
        .into_iter()
        .map(str::to_owned)
    )
    .is_err());
}

#[test]
fn residual_seed_rejects_count_only_or_duplicate_claims() {
    let identity = ExactIdentity {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
        pass: "semantic".to_owned(),
        file: Some("a.ts".to_owned()),
        start: Some(0),
        length: Some(1),
        code: 7005,
        category: "error".to_owned(),
        chain_sha256: "a".repeat(64),
        related_sha256: "b".repeat(64),
        occurrence: 0,
    };
    assert!(validate_residual_seed(&ResidualSeed {
        supported_false_negative_diagnostics: 2,
        supported_false_negative_identities: vec![identity.clone()],
    })
    .is_err());
    assert!(validate_residual_seed(&ResidualSeed {
        supported_false_negative_diagnostics: 2,
        supported_false_negative_identities: vec![identity.clone(), identity],
    })
    .is_err());
}

#[test]
fn plan_audit_rejects_disagreeing_identity_and_cluster_assignment() {
    let exact = ExactIdentity {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
        pass: "semantic".to_owned(),
        file: Some("a.ts".to_owned()),
        start: Some(0),
        length: Some(1),
        code: 7005,
        category: "error".to_owned(),
        chain_sha256: "a".repeat(64),
        related_sha256: "b".repeat(64),
        occurrence: 0,
    };
    let identity_id = exact.sha256();
    let plan = json!({
        "schema": 1,
        "status": "draft",
        "summary": {
            "identities": 1,
            "programs": 1,
            "codes": 1,
            "clusters": 2,
        },
        "programs": [{"key": "conformance/a.ts"}],
        "identities": [{
            "id": identity_id,
            "identity": exact,
            "family": "implicit-any",
            "program": "conformance/a.ts",
            "cluster": "cluster-b",
        }],
        "clusters": [
            {
                "id": "cluster-a",
                "family": "implicit-any",
                "identity_ids": [identity_id],
                "codes_and_passes": [{"code": 7005, "pass": "semantic"}],
            },
            {
                "id": "cluster-b",
                "family": "implicit-any",
                "identity_ids": [],
                "codes_and_passes": [],
            },
        ],
    });
    assert!(audit_plan(Path::new("."), &plan, false)
        .unwrap_err()
        .to_string()
        .contains("disagree"));
}

#[test]
fn scc_review_requires_exact_ids_and_keeps_large_components_separate() {
    let singleton = json!([{"id": "scc:one", "member_count": 1}]);
    assert!(validate_scc_decisions(
        "cluster",
        singleton.as_array().unwrap(),
        &[SccDecision {
            id: "scc:one".to_owned(),
            decision: "singleton".to_owned(),
            rationale: "one exact producer".to_owned(),
        }],
    )
    .is_ok());
    let large = json!([{"id": "scc:large", "member_count": 1396}]);
    assert!(validate_scc_decisions(
        "cluster",
        large.as_array().unwrap(),
        &[SccDecision {
            id: "scc:large".to_owned(),
            decision: "singleton".to_owned(),
            rationale: "invalid collapse".to_owned(),
        }],
    )
    .is_err());
    assert!(validate_scc_decisions(
        "cluster",
        large.as_array().unwrap(),
        &[SccDecision {
            id: "scc:large".to_owned(),
            decision: "keep-separate".to_owned(),
            rationale: "mechanical TypeChecker SCC crosses subsystems".to_owned(),
        }],
    )
    .is_ok());
}

#[test]
fn freeze_transition_accepts_only_the_anchored_identical_draft() {
    let anchor = "a".repeat(40);
    let trusted = json!({
        "schema": 1,
        "status": "draft",
        "payload": {"identities": 333},
    });
    let mut frozen = trusted.clone();
    frozen["status"] = json!("frozen");
    frozen["freeze"] = json!({"adjudication_commit": anchor});
    assert!(validate_plan_transition(&trusted, &frozen, &anchor).is_ok());

    let mut changed = frozen.clone();
    changed["payload"]["identities"] = json!(332);
    assert!(validate_plan_transition(&trusted, &changed, &anchor).is_err());
    assert!(validate_plan_transition(&trusted, &frozen, &"b".repeat(40)).is_err());
    assert!(validate_plan_transition(&frozen, &frozen, &anchor).is_ok());
    assert!(validate_plan_transition(&frozen, &trusted, &anchor).is_err());
}

#[test]
fn historical_plan_inputs_follow_the_workspace_promotion() {
    let repo = TempRepo::new("workspace-promotion");
    let legacy = repo.0.join("tsrs2");
    fs::create_dir_all(&legacy).unwrap();
    let plan = json!({"schema": 1, "status": "draft"});
    let review = json!({"schema": 1, "review": "complete"});
    fs::write(
        legacy.join("m8-owner-plan.json"),
        serde_json::to_vec(&plan).unwrap(),
    )
    .unwrap();
    fs::write(
        legacy.join("m8-owner-plan-review.json"),
        serde_json::to_vec(&review).unwrap(),
    )
    .unwrap();
    git_test(&repo.0, &["add", "tsrs2"]);
    git_test(&repo.0, &["commit", "-q", "-m", "legacy plan inputs"]);
    let commit = String::from_utf8(git_test(&repo.0, &["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();

    assert_eq!(
        plan_at(&repo.0, &commit, &repo.0.join("m8-owner-plan.json")).unwrap(),
        plan
    );
    let review_bytes =
        git_blob_at(&repo.0, &commit, &repo.0.join("m8-owner-plan-review.json")).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&review_bytes).unwrap(),
        review
    );
}
