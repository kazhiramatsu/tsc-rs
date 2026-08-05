use super::*;
use crate::test_git::{git_test, init_repo, temp_dir};
use std::path::PathBuf;
use std::sync::OnceLock;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn committed_registry(workspace: &Path) -> RegistryFile {
    let path = workspace.join(HOST_RESOLUTION_REL_PATH);
    parse_registry(&fs::read(&path).unwrap(), &path.display().to_string()).unwrap()
}

fn accepted_history_proof(workspace: &Path) -> &'static AcceptedPairHistoryProof {
    static PROOF: OnceLock<AcceptedPairHistoryProof> = OnceLock::new();
    PROOF
        .get_or_init(|| crate::ratchet::verify_accepted_pair_history_with_proof(workspace).unwrap())
}

fn validate_registry_with_cached_history(
    workspace: &Path,
    registry: &RegistryFile,
    scope: &HostResolutionScopeState,
    inventory: &D2Inventory,
    inputs: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    validate_registry_with_options(
        workspace,
        registry,
        scope,
        inventory,
        inputs,
        RegistryValidationOptions {
            verify_history: false,
            verify_request_producer: false,
            history_proof: Some(accepted_history_proof(workspace)),
            history_head: None,
        },
    )
    .map(drop)
}

fn commit_bytes(root: &Path, rel: &str, bytes: &[u8], message: &str) -> String {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
    git_test(root, &["add", rel]);
    git_test(root, &["commit", "-q", "-m", message]);
    String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned()
}

#[test]
fn expected_owner_families_are_complete_and_ordered() {
    let families = expected_families();
    assert_eq!(families.len(), 8);
    assert_eq!(families[0].id, FAMILY_EXPORTS);
    assert_eq!(families[7].id, FAMILY_CLI);
    assert_eq!(
        families
            .iter()
            .map(|family| family.id.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
}

#[test]
fn row_id_uses_the_exact_occurrence_identity() {
    let mut identity = ExactIdentity {
        fixture: "conformance/example.ts".to_owned(),
        matrix_key: String::new(),
        pass: "semantic".to_owned(),
        file: Some("/a.ts".to_owned()),
        start: Some(1),
        length: Some(2),
        code: 2307,
        category: "Error".to_owned(),
        chain_sha256: "a".repeat(64),
        related_sha256: "b".repeat(64),
        occurrence: 0,
    };
    let first = format!("h0:{}", identity.sha256());
    identity.occurrence = 1;
    assert_ne!(first, format!("h0:{}", identity.sha256()));
}

#[test]
fn closure_shape_is_fail_closed() {
    assert!(safe_relative_path("ratchets/h0/evidence.json"));
    assert!(!safe_relative_path("../evidence.json"));
    assert!(!safe_relative_path("/tmp/evidence.json"));
    assert!(valid_commit(&"a".repeat(40)));
    assert!(!valid_commit(&"A".repeat(40)));
}

#[test]
fn all_open_closure_authorities_do_not_require_a_git_repository() {
    let workspace = workspace();
    let mut row = committed_registry(&workspace).rows[0].clone();
    row.status = RowStatus::Open;
    row.closing_commit = None;
    row.closure_evidence = None;
    let non_git_workspace = temp_dir("h0-all-open-no-git");

    let ClosureHistoryLoad {
        authorities,
        git_memo,
        owned_history_proof,
    } = load_closure_authorities(&non_git_workspace, &[row], None, None).unwrap();
    assert!(authorities.is_empty());
    assert!(git_memo.is_none());
    assert!(owned_history_proof.is_none());
}

#[test]
fn committed_registry_passes_full_owner_and_canary_validation() {
    let workspace = workspace();
    let scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH)).unwrap();
    let inventory = read_inventory(&workspace).unwrap();
    let inputs = read_oracle_inputs(&workspace).unwrap();
    validate_registry_with_cached_history(
        &workspace,
        &committed_registry(&workspace),
        &scope,
        &inventory,
        &inputs,
    )
    .unwrap();
}

#[test]
fn strict_schema_rejects_unreviewed_fields() {
    let workspace = workspace();
    let bytes = fs::read(workspace.join(HOST_RESOLUTION_REL_PATH)).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["unreviewed"] = serde_json::json!(true);
    let error = parse_registry(&serde_json::to_vec(&value).unwrap(), "mutation")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `unreviewed`"), "{error}");

    let mut nested: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    nested["rows"][0]["identity"]["unreviewed"] = serde_json::json!(true);
    let error = parse_registry(&serde_json::to_vec(&nested).unwrap(), "nested-mutation")
        .unwrap_err()
        .to_string();
    assert!(error.contains("unknown field `unreviewed`"), "{error}");
}

#[test]
fn committed_mode_and_canary_censuses_are_exact() {
    let workspace = workspace();
    let registry = committed_registry(&workspace);
    validate_expected_module_resolution_counts(&registry.rows).unwrap();
    validate_expected_request_mode_counts(&recorded_resolution_requests(&registry.rows).unwrap())
        .unwrap();
    let relations = registry
        .rows
        .iter()
        .fold(BTreeMap::new(), |mut counts, row| {
            *counts
                .entry(row.canaries.non_emitting_control.relation)
                .or_insert(0usize) += 1;
            counts
        });
    assert_eq!(
        relations,
        BTreeMap::from([
            (CanaryRelation::ExactFeature, 226),
            (CanaryRelation::ClosestAvailable, 9),
            (CanaryRelation::IntentionalAlternate, 6),
        ])
    );
}

#[test]
fn registry_rejects_a_draft_current_scope() {
    let workspace = workspace();
    let mut scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH)).unwrap();
    scope.frozen = false;
    let error = validate_registry(
        &workspace,
        &committed_registry(&workspace),
        &scope,
        &read_inventory(&workspace).unwrap(),
        &read_oracle_inputs(&workspace).unwrap(),
        false,
        false,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("current M8 scope manifest"), "{error}");
}

#[test]
fn trusted_baseline_must_be_on_the_head_ancestry() {
    let workspace = workspace();
    let repo = init_repo("h0-sideways-baseline");
    commit_bytes(&repo, "seed", b"seed\n", "seed");
    git_test(&repo, &["branch", "side"]);
    commit_bytes(&repo, "main", b"main\n", "main");
    git_test(&repo, &["checkout", "-q", "side"]);
    let side = commit_bytes(&repo, "side", b"side\n", "side");
    git_test(&repo, &["checkout", "-q", "main"]);

    let error = validate_trusted_baseline(&repo, &side, &committed_registry(&workspace))
        .unwrap_err()
        .to_string();
    assert!(error.contains("not an ancestor of HEAD"), "{error}");
}

#[test]
fn initial_scope_history_and_closing_anchors_are_commit_local() {
    let workspace = workspace();
    let registry = committed_registry(&workspace);
    let repo = init_repo("h0-commit-local-provenance");
    let workspace_root = git_root_for(&workspace).unwrap();
    let scope_rel = workspace_history_rel(&workspace_root, &workspace, SCOPE_REL_PATH).unwrap();
    let scope_bytes = git_blob_optional(
        &workspace_root,
        &registry.source.initial_scope_commit,
        &scope_rel,
    )
    .unwrap()
    .expect("initial frozen scope blob");
    let commit = commit_bytes(&repo, SCOPE_REL_PATH, &scope_bytes, "scope");

    let mut source = registry.source.clone();
    source.initial_scope_commit = commit.clone();
    source.initial_seed_sha256 = "0".repeat(64);
    let error = validate_initial_scope_history(&repo, &source)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("differs from its frozen source pin"),
        "{error}"
    );

    let mut row = registry.rows[0].clone();
    row.rust_boundary.authoritative_anchors = vec![RustBoundaryAnchor {
        role: RustBoundaryRole::Producer,
        crate_name: "test".to_owned(),
        path: SCOPE_REL_PATH.to_owned(),
        symbol: "symbol-that-was-added-after-closing".to_owned(),
    }];
    let error = validate_authoritative_anchors_at_commit(&repo, &row, &commit)
        .unwrap_err()
        .to_string();
    assert!(error.contains("is absent at closing commit"), "{error}");
}

#[test]
fn closure_git_memo_reuses_commit_reachability_and_anchor_text() {
    let workspace = workspace();
    let registry = committed_registry(&workspace);
    let repo = init_repo("h0-closure-git-memo");
    let commit = commit_bytes(
        &repo,
        "src/authority.rs",
        b"fn producer_symbol() {}\nfn consumer_symbol() {}\n",
        "authority",
    );
    let mut row = registry.rows[0].clone();
    row.rust_boundary.authoritative_anchors = vec![
        RustBoundaryAnchor {
            role: RustBoundaryRole::Producer,
            crate_name: "test".to_owned(),
            path: "src/authority.rs".to_owned(),
            symbol: "producer_symbol".to_owned(),
        },
        RustBoundaryAnchor {
            role: RustBoundaryRole::TableConsumer,
            crate_name: "test".to_owned(),
            path: "src/authority.rs".to_owned(),
            symbol: "consumer_symbol".to_owned(),
        },
    ];

    let memo = RegistryGitMemo::new(&repo).unwrap();
    assert!(memo.is_ancestor_of_head(&commit).unwrap());
    assert!(memo.is_ancestor_of_head(&commit).unwrap());
    assert_eq!(memo.ancestors_of_head.borrow().len(), 1);

    validate_authoritative_anchors_at_commit_with_memo(&memo, &row, &commit).unwrap();
    validate_authoritative_anchors_at_commit_with_memo(&memo, &row, &commit).unwrap();
    assert_eq!(
        memo.historical_texts.borrow().len(),
        1,
        "two symbols and repeated row validation must share one commit/path blob"
    );

    let moved = commit_bytes(&repo, "unrelated", b"move HEAD", "move HEAD");
    let error = memo.verify_head_unchanged().unwrap_err().to_string();
    assert!(error.contains("HEAD moved"), "{error}");
    assert!(error.contains(&moved), "{error}");
}

#[test]
fn lapsed_transition_state_machine_is_fail_closed_and_reactivatable() {
    let workspace = workspace();
    let mut open = committed_registry(&workspace)
        .rows
        .into_iter()
        .next()
        .expect("committed registry retains a row");
    // The production registry may legitimately reach zero open rows.
    // Reconstruct the row's historical pre-closure shape so the transition
    // state machine remains covered after complete H0 owner closure.
    open.status = RowStatus::Open;
    open.closing_commit = None;
    open.closure_evidence = None;
    open.rust_boundary.readiness = BoundaryReadiness::SeamOnly;
    open.rust_boundary.authoritative_anchors.clear();

    let mut lapsed = open.clone();
    lapsed.status = RowStatus::Lapsed;
    validate_row_transition(&open, &lapsed).unwrap();

    let fake_evidence = ClosureEvidence {
        tiers: ["t0", "t1", "t2", "t3", "t4"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        artifact: MATCHES_REL_PATH.to_owned(),
        artifact_sha256: "a".repeat(64),
        note: "test evidence".to_owned(),
    };
    let mut fabricated_lapse = lapsed.clone();
    fabricated_lapse.closing_commit = Some("b".repeat(40));
    fabricated_lapse.closure_evidence = Some(fake_evidence.clone());
    let error = validate_row_transition(&open, &fabricated_lapse)
        .unwrap_err()
        .to_string();
    assert!(error.contains("fabricated closure provenance"), "{error}");

    let mut reactivated = lapsed.clone();
    reactivated.status = RowStatus::Closed;
    reactivated.closing_commit = Some("b".repeat(40));
    reactivated.closure_evidence = Some(fake_evidence.clone());
    reactivated.rust_boundary.readiness = BoundaryReadiness::Authoritative;
    reactivated.rust_boundary.authoritative_anchors =
        reactivated.rust_boundary.seam_anchors.clone();
    reactivated.rust_boundary.authoritative_anchors[0].symbol =
        "try_add_module_resolution".to_owned();
    validate_row_transition(&lapsed, &reactivated).unwrap();

    let mut closed = reactivated.clone();
    let mut closed_lapse = closed.clone();
    closed_lapse.status = RowStatus::Lapsed;
    validate_row_transition(&closed, &closed_lapse).unwrap();
    closed.closure_evidence.as_mut().unwrap().note = "changed".to_owned();
    let error = validate_row_transition(&closed, &closed_lapse)
        .unwrap_err()
        .to_string();
    assert!(error.contains("immutable closure provenance"), "{error}");

    let error = validate_row_transition(&lapsed, &open)
        .unwrap_err()
        .to_string();
    assert!(error.contains("lapsed host-resolution row"), "{error}");
}

#[test]
fn full_validator_rejects_owner_canary_closure_and_universe_drift() {
    let workspace = workspace();
    let scope = host_resolution_state(&workspace.join(SCOPE_REL_PATH)).unwrap();
    let inventory = read_inventory(&workspace).unwrap();
    let inputs = read_oracle_inputs(&workspace).unwrap();
    let registry = committed_registry(&workspace);

    let mut stale_owner = registry.clone();
    stale_owner.rows[0].tsc_owners[0].source_slice_sha256 = "0".repeat(64);
    let error = validate_registry_with_cached_history(
        &workspace,
        &stale_owner,
        &scope,
        &inventory,
        &inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("stale D2 owner metadata"), "{error}");

    let mut stale_canary = registry.clone();
    stale_canary.rows[0]
        .canaries
        .non_emitting_control
        .forbidden_codes = vec![9999];
    let error = validate_registry_with_cached_history(
        &workspace,
        &stale_canary,
        &scope,
        &inventory,
        &inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("stale reviewed non-emitting control"),
        "{error}"
    );

    let mut false_closure = registry.clone();
    false_closure.rows[0].closing_commit = None;
    false_closure.rows[0].closure_evidence = None;
    false_closure.summary = summarize(&false_closure.rows);
    let error = validate_registry_with_cached_history(
        &workspace,
        &false_closure,
        &scope,
        &inventory,
        &inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("has no closing commit"), "{error}");

    let mut missing = registry.clone();
    missing.rows.pop();
    missing.summary = summarize(&missing.rows);
    let error =
        validate_registry_with_cached_history(&workspace, &missing, &scope, &inventory, &inputs)
            .unwrap_err()
            .to_string();
    assert!(error.contains("projection hash is stale"), "{error}");
}
