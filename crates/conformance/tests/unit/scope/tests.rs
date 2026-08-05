use std::path::PathBuf;

use super::*;
use crate::identity::assign_case_identities;
use crate::ratchet::{git, CaseSets, MatchesArtifact, MatchesInputs, RunSets};
use crate::test_git::{git_test, init_repo, temp_dir};
use crate::GoldenMessageChain;

fn commit_scope_at(root: &Path, rel: &str, file: &ScopeFile, message: &str) -> String {
    if let Some(parent) = root.join(rel).parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(root.join(rel), serde_json::to_vec_pretty(file).unwrap()).unwrap();
    git_test(root, &["add", rel]);
    git_test(root, &["commit", "-q", "-m", message]);
    String::from_utf8(git(root, &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .to_owned()
}

fn commit_scope(root: &Path, file: &ScopeFile, message: &str) -> String {
    commit_scope_at(root, SCOPE_REL_PATH, file, message)
}

fn diag(code: u32, start: u32, pass: &str, text: &str) -> GoldenDiag {
    GoldenDiag {
        file: Some("a.ts".to_owned()),
        start: Some(start),
        length: Some(1),
        line: Some(0),
        col: Some(start),
        code,
        pass: Some(pass.to_owned()),
        category: "error".to_owned(),
        chain: GoldenMessageChain {
            text: text.to_owned(),
            code,
            category: "error".to_owned(),
            next: Vec::new(),
        },
        related: Vec::new(),
        reports_unnecessary: false,
        reports_deprecated: false,
        source: None,
    }
}

const FIXTURE: &str = "conformance/a.ts";

fn identity_at(oracle: &[GoldenDiag], index: usize) -> ExactIdentity {
    assign_case_identities(FIXTURE, "", oracle).unwrap()[index].clone()
}

fn exclusion_of(oracle: &[GoldenDiag], index: usize) -> ScopeExclusion {
    ScopeExclusion {
        identity: identity_at(oracle, index),
        line: oracle[index].line,
        col: oracle[index].col,
        reason: ScopeReason::HostResolution,
        evidence: "adjudicated: outside the batch host".to_owned(),
    }
}

fn scope_file(status: ScopeStatus, exclusions: Vec<ScopeExclusion>) -> ScopeFile {
    ScopeFile {
        schema: SCOPE_SCHEMA,
        encoder: ENCODER_VERSION,
        status,
        exclusions,
        band_pins: Vec::new(),
        tombstones: Vec::new(),
        global: None,
    }
}

fn load_file(name: &str, file: &ScopeFile) -> ConformanceResult<ScopeManifest> {
    let path = temp_dir(name).join("m8-scope.json");
    fs::write(&path, serde_json::to_vec_pretty(file).unwrap()).unwrap();
    ScopeManifest::load(&path)
}

fn load_err(name: &str, file: &ScopeFile) -> String {
    load_file(name, file).map(|_| ()).unwrap_err().to_string()
}

/// 40-hex fake anchors for structural tests (never resolved).
const FAKE_SHA: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
const FAKE_SHA_2: &str = "cafebabecafebabecafebabecafebabecafebabe";

fn tombstone_of(identity: ExactIdentity, resolving_commit: &str) -> Tombstone {
    Tombstone {
        identity,
        resolving_commit: Some(resolving_commit.to_owned()),
        lapsed: false,
    }
}

fn lapsed_tombstone_of(identity: ExactIdentity, resolving_commit: Option<&str>) -> Tombstone {
    Tombstone {
        identity,
        resolving_commit: resolving_commit.map(str::to_owned),
        lapsed: true,
    }
}

fn matches_stub(views: RunSets) -> MatchesArtifact {
    MatchesArtifact {
        schema: 1,
        bootstrap: true,
        previous: None,
        transition: None,
        inputs: MatchesInputs {
            oracle_inputs_sha256: "inputs".to_owned(),
            tsc_js_sha256: "tsc".to_owned(),
        },
        views,
        lapsed: None,
    }
}

fn views_with(view: &str, bucket: &T0Key, complete: bool) -> RunSets {
    let mut sets = CaseSets::default();
    sets.matched.insert(bucket.clone());
    if complete {
        sets.multiplicity_complete.insert(bucket.clone());
    }
    let mut views = RunSets::new();
    views
        .entry(view.to_owned())
        .or_default()
        .entry(FIXTURE.to_owned())
        .or_default()
        .insert(String::new(), sets);
    views
}

// -- schema / structural loading ---------------------------------------

#[test]
fn schema_1_is_rejected_with_a_migration_message() {
    let path = temp_dir("schema1").join("m8-scope.json");
    fs::write(&path, br#"{"schema":1,"status":"draft","exclusions":[]}"#).unwrap();
    let error = ScopeManifest::load(&path)
        .map(|_| ())
        .unwrap_err()
        .to_string();
    assert!(error.contains("retired schema 1"), "{error}");
    assert!(
        error.contains("cannot freeze or satisfy readiness"),
        "{error}"
    );
}

#[test]
fn encoder_version_drift_is_rejected() {
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.encoder = 2;
    let error = load_err("encoder", &file);
    assert!(error.contains("one reviewed schema extension"), "{error}");
}

#[test]
fn empty_schema_2_manifest_loads() {
    let manifest = load_file("empty", &scope_file(ScopeStatus::Draft, Vec::new())).unwrap();
    assert_eq!(manifest.entry_count(), 0);
    assert_eq!(manifest.status().name(), "draft");
}

#[test]
fn host_resolution_projection_preserves_draft_status_from_bytes() {
    let bytes = br#"{
        "schema": 2,
        "encoder": 1,
        "status": "draft",
        "exclusions": []
    }"#;
    let state = host_resolution_state_from_bytes(bytes, "draft-byte-fixture").unwrap();
    assert!(!state.frozen);
    assert!(state.live.is_empty());
    assert!(state.tombstones.is_empty());
}

#[test]
fn host_resolution_projection_preserves_frozen_status_from_bytes() {
    let bytes = br#"{
        "schema": 2,
        "encoder": 1,
        "status": "frozen",
        "exclusions": [],
        "global": {
            "adjudication_commit": "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
            "identities": []
        }
    }"#;
    let state = host_resolution_state_from_bytes(bytes, "frozen-byte-fixture").unwrap();
    assert!(state.frozen);
    assert!(state.live.is_empty());
    assert!(state.tombstones.is_empty());
}

#[test]
fn duplicate_exclusion_identities_are_rejected() {
    let oracle = [diag(2307, 0, "semantic", "missing")];
    let file = scope_file(
        ScopeStatus::Draft,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 0)],
    );
    let error = load_err("dup", &file);
    assert!(error.contains("duplicate M8 scope exclusion"), "{error}");
}

#[test]
fn syntactic_exclusions_are_rejected() {
    let oracle = [diag(1005, 0, "syntactic", "expected ';'")];
    let file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    let error = load_err("syntactic", &file);
    assert!(error.contains("non-excludable"), "{error}");
}

#[test]
fn missing_evidence_is_rejected() {
    let oracle = [diag(2307, 0, "semantic", "missing")];
    let mut exclusion = exclusion_of(&oracle, 0);
    exclusion.evidence = "  ".to_owned();
    let error = load_err("evidence", &scope_file(ScopeStatus::Draft, vec![exclusion]));
    assert!(error.contains("no evidence"), "{error}");
}

// -- A2 identity: the exact selector ------------------------------------

#[test]
fn exact_occurrence_is_selected_and_bucket_survives() {
    // Duplicate bucket: two byte-identical records. Excluding
    // occurrence 1 removes exactly one record; the bucket stays
    // in the supported denominator.
    let oracle = vec![
        diag(2695, 29, "semantic", "unused"),
        diag(2695, 29, "semantic", "unused"),
    ];
    let identities = assign_case_identities(FIXTURE, "", &oracle).unwrap();
    assert_eq!(identities[1].occurrence, 1);
    let file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 1)]);
    let mut manifest = load_file("exact", &file).unwrap();
    let excluded = manifest.exclusions_for_case(FIXTURE, "", &oracle).unwrap();
    assert_eq!(excluded, [1usize].into_iter().collect());

    let (supported, fully_excluded) = supported_case_view(&oracle, DiagnosticBand::All, &excluded);
    assert!(
        supported.contains(&t0_key(&oracle[0])),
        "bucket must survive"
    );
    assert!(fully_excluded.is_empty());

    // Excluding BOTH occurrences removes the bucket.
    let both = [0usize, 1].into_iter().collect();
    let (supported, fully_excluded) = supported_case_view(&oracle, DiagnosticBand::All, &both);
    assert!(supported.is_empty());
    assert_eq!(fully_excluded.len(), 1);
    manifest.finish_full_validation().unwrap();
}

#[test]
fn same_t0_key_different_message_is_not_conflated() {
    // Two records share the T0 key but differ in message; the
    // exclusion selects only its own record.
    let oracle = vec![
        diag(2769, 8, "semantic", "no overload matches"),
        diag(2769, 8, "semantic", "overload 2 of 3 failed"),
    ];
    let file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    let mut manifest = load_file("t0-collision", &file).unwrap();
    let excluded = manifest.exclusions_for_case(FIXTURE, "", &oracle).unwrap();
    assert_eq!(excluded, [0usize].into_iter().collect());
    let (supported, fully_excluded) = supported_case_view(&oracle, DiagnosticBand::All, &excluded);
    assert!(supported.contains(&t0_key(&oracle[1])));
    assert!(fully_excluded.is_empty());
}

#[test]
fn stale_exclusion_is_rejected() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let mut exclusion = exclusion_of(&oracle, 0);
    exclusion.identity.occurrence = 1; // no such occurrence
    let file = scope_file(ScopeStatus::Draft, vec![exclusion]);
    let mut manifest = load_file("stale", &file).unwrap();
    let error = manifest
        .exclusions_for_case(FIXTURE, "", &oracle)
        .unwrap_err()
        .to_string();
    assert!(error.contains("stale M8 scope exclusion"), "{error}");
}

#[test]
fn review_field_mismatch_is_rejected() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let mut exclusion = exclusion_of(&oracle, 0);
    exclusion.line = Some(7);
    let file = scope_file(ScopeStatus::Draft, vec![exclusion]);
    let mut manifest = load_file("review", &file).unwrap();
    let error = manifest
        .exclusions_for_case(FIXTURE, "", &oracle)
        .unwrap_err()
        .to_string();
    assert!(error.contains("review fields"), "{error}");
}

#[test]
fn full_validation_reports_unmatched_exclusions() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    let manifest = load_file("unseen", &file).unwrap();
    let error = manifest.finish_full_validation().unwrap_err().to_string();
    assert!(
        error.contains("outside the full conformance corpus"),
        "{error}"
    );
}

#[test]
fn node_rust_divergence_fails_the_cross_check() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let rust = crate::identity::case_identity_report(FIXTURE, "", &oracle).unwrap();
    let mut node = rust.clone();
    node.identity_sha256[0] = format!("{}0", &node.identity_sha256[0][..63]);
    let error = compare_reports("vector unicode", &rust, &node)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Node/Rust canonical encoders differ"),
        "{error}"
    );
    assert!(error.contains("record 0"), "{error}");
}

/// The real cross-language check over the committed vector file:
/// both encoders must produce byte-identical output (requires
/// `node`, which the oracle workflow and hosted CI already pin).
#[test]
fn node_encoder_matches_rust_over_the_vector_file() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let vectors: VectorFile = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/identity-vectors-v1.json"
    )))
    .unwrap();
    let cases = vectors
        .cases
        .into_iter()
        .map(|case| CrossCheckCase {
            label: format!("vector {}", case.name),
            fixture: case.fixture,
            matrix_key: case.matrix_key,
            records: case.records,
        })
        .collect::<Vec<_>>();
    let checked = run_cross_language_check(workspace, &cases).unwrap();
    assert_eq!(checked, 10);
}

// -- resolution predicate (§3.2) ----------------------------------------

#[test]
fn resolution_predicate_requires_multiplicity_completeness() {
    // Matched singleton: resolved.
    assert!(occurrence_resolved(true, 1, 1));
    assert!(occurrence_resolved(true, 1, 3));
    // Unmatched: never resolved.
    assert!(!occurrence_resolved(false, 1, 1));
    // Matched duplicate bucket at 2/1: a match cannot prove which
    // occurrence resolved.
    assert!(!occurrence_resolved(true, 2, 1));
    // Matched multiplicity-complete duplicate bucket: resolved.
    assert!(occurrence_resolved(true, 2, 2));
}

// -- A2 pin --------------------------------------------------------------

fn in_band_oracle() -> Vec<GoldenDiag> {
    vec![
        diag(2307, 0, "semantic", "missing module"),
        diag(2322, 5, "semantic", "not assignable"),
    ]
}

fn pin_of(band: &str, commit: &str, identities: Vec<ExactIdentity>) -> BandPin {
    BandPin {
        band: band.to_owned(),
        adjudication_commit: commit.to_owned(),
        identities,
    }
}

#[test]
fn pinned_band_addition_fails_structurally() {
    // Pin enumerates only exclusion 0; a second in-band exclusion
    // appears -> load fails without any git access.
    let oracle = in_band_oracle();
    let mut file = scope_file(
        ScopeStatus::Draft,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 1)],
    );
    file.band_pins = vec![pin_of("2xxx", FAKE_SHA, vec![identity_at(&oracle, 0)])];
    let error = load_err("pin-add", &file);
    assert!(error.contains("not in its pinned identity set"), "{error}");
}

#[test]
fn pinned_identity_disappearance_needs_a_tombstone() {
    let oracle = in_band_oracle();
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.band_pins = vec![pin_of("2xxx", FAKE_SHA, vec![identity_at(&oracle, 0)])];
    let error = load_err("pin-gone", &file);
    assert!(error.contains("disappeared without a tombstone"), "{error}");

    file.tombstones = vec![tombstone_of(identity_at(&oracle, 0), FAKE_SHA)];
    load_file("pin-tombstoned", &file).unwrap();
}

#[test]
fn out_of_band_pin_identity_is_rejected() {
    let oracle = vec![diag(6133, 0, "suggestion", "unused")];
    let mut file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    file.band_pins = vec![pin_of("2xxx", FAKE_SHA, vec![identity_at(&oracle, 0)])];
    let error = load_err("pin-band", &file);
    assert!(error.contains("out-of-band identity"), "{error}");
}

#[test]
fn unknown_pin_band_is_rejected() {
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.band_pins = vec![pin_of("5xxx", FAKE_SHA, Vec::new())];
    let error = load_err("pin-unknown", &file);
    assert!(error.contains("only \"2xxx\""), "{error}");
}

#[test]
fn band_pin_anchor_round_trip_and_rewrite_attacks() {
    let root = init_repo("pin-anchor");
    let oracle = in_band_oracle();
    // Reviewed content lands first (both in-band exclusions).
    let adjudicated = scope_file(
        ScopeStatus::Draft,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 1)],
    );
    let commit = commit_scope(&root, &adjudicated, "adjudicated content");
    // The pin follows, enumerating exactly that band subset.
    let mut pinned = adjudicated.clone();
    pinned.band_pins = vec![pin_of(
        "2xxx",
        &commit,
        vec![identity_at(&oracle, 0), identity_at(&oracle, 1)],
    )];
    commit_scope(&root, &pinned, "band pin");
    let head = resolve_commit(&root, "HEAD").unwrap();
    verify_band_pin(&root, SCOPE_REL_PATH, &head, &pinned.band_pins[0]).unwrap();

    // Rewritten pin: enumerates a subset (edit + rewritten
    // set/count/hash) -> the identity comparison against the
    // adjudication commit fails.
    let rewritten = pin_of("2xxx", &commit, vec![identity_at(&oracle, 0)]);
    let error = verify_band_pin(&root, SCOPE_REL_PATH, &head, &rewritten)
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not equal the band subset"), "{error}");

    // Over-enumeration fails the same way.
    let extra = diag(2999, 60, "semantic", "invented");
    let over = pin_of(
        "2xxx",
        &commit,
        vec![
            identity_at(&oracle, 0),
            identity_at(&oracle, 1),
            identity_at(&[extra], 0),
        ],
    );
    let error = verify_band_pin(&root, SCOPE_REL_PATH, &head, &over)
        .unwrap_err()
        .to_string();
    assert!(error.contains("does not equal the band subset"), "{error}");
}

#[test]
fn legacy_workspace_scope_anchors_and_baseline_survive_root_promotion() {
    let root = init_repo("legacy-scope-promotion");
    let legacy_rel = format!("tsrs2/{SCOPE_REL_PATH}");
    let oracle = in_band_oracle();
    let adjudicated = scope_file(
        ScopeStatus::Draft,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 1)],
    );
    let adjudication =
        commit_scope_at(&root, &legacy_rel, &adjudicated, "legacy adjudicated scope");

    let identities = vec![identity_at(&oracle, 0), identity_at(&oracle, 1)];
    let mut frozen = adjudicated.clone();
    frozen.status = ScopeStatus::Frozen;
    frozen.band_pins = vec![pin_of("2xxx", &adjudication, identities.clone())];
    frozen.global = Some(GlobalFreeze {
        adjudication_commit: adjudication,
        identities,
    });
    let legacy_baseline = commit_scope_at(&root, &legacy_rel, &frozen, "legacy frozen scope");

    git_test(&root, &["mv", &legacy_rel, SCOPE_REL_PATH]);
    git_test(&root, &["commit", "-q", "-m", "promote scope to root"]);
    let head = resolve_commit(&root, "HEAD").unwrap();

    verify_band_pin(&root, SCOPE_REL_PATH, &head, &frozen.band_pins[0]).unwrap();
    verify_global_freeze(
        &root,
        SCOPE_REL_PATH,
        &head,
        &frozen,
        frozen.global.as_ref().unwrap(),
    )
    .unwrap();
    verify_scope_baseline(&root, SCOPE_REL_PATH, &legacy_baseline, &frozen).unwrap();
}

#[test]
fn band_pin_non_ancestor_adjudication_is_rejected() {
    let root = init_repo("pin-ancestor");
    let oracle = in_band_oracle();
    let content = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    commit_scope(&root, &content, "main content");
    // A side branch holds the claimed adjudication commit.
    git_test(&root, &["checkout", "-q", "-b", "side"]);
    let mut side_content = content.clone();
    side_content.exclusions[0].evidence = "side variant".to_owned();
    let side = commit_scope(&root, &side_content, "side adjudication");
    git_test(&root, &["checkout", "-q", "main"]);
    let mut main_content = content.clone();
    main_content.exclusions[0].evidence = "main variant".to_owned();
    let main_head = commit_scope(&root, &main_content, "advance main");

    let pin = pin_of("2xxx", &side, vec![identity_at(&oracle, 0)]);
    let error = verify_band_pin(&root, SCOPE_REL_PATH, &main_head, &pin)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not an ancestor of HEAD"), "{error}");
}

#[test]
fn band_pin_without_manifest_at_adjudication_is_rejected() {
    let root = init_repo("pin-missing");
    fs::write(root.join("other.txt"), b"x").unwrap();
    git_test(&root, &["add", "other.txt"]);
    git_test(&root, &["commit", "-q", "-m", "no manifest"]);
    let bare = String::from_utf8(git(&root, &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .to_owned();
    let oracle = in_band_oracle();
    commit_scope(
        &root,
        &scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]),
        "manifest arrives later",
    );
    let head = resolve_commit(&root, "HEAD").unwrap();
    let pin = pin_of("2xxx", &bare, vec![identity_at(&oracle, 0)]);
    let error = verify_band_pin(&root, SCOPE_REL_PATH, &head, &pin)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no M8 scope manifest"), "{error}");
}

#[test]
fn band_pin_anchored_on_a_frozen_commit_is_rejected() {
    // Reviewed snapshot protocol: content lands while draft; a
    // pin cannot anchor on a commit that is already frozen.
    let root = init_repo("pin-frozen-anchor");
    let oracle = in_band_oracle();
    let mut frozen = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    frozen.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let commit = commit_scope(&root, &frozen, "frozen commit");
    let head = resolve_commit(&root, "HEAD").unwrap();
    let pin = pin_of("2xxx", &commit, vec![identity_at(&oracle, 0)]);
    let error = verify_band_pin(&root, SCOPE_REL_PATH, &head, &pin)
        .unwrap_err()
        .to_string();
    assert!(error.contains("draft manifest"), "{error}");
}

// -- A2 tombstone ---------------------------------------------------------

fn golden_case(oracle: Vec<GoldenDiag>) -> GoldenCase {
    GoldenCase {
        matrix_key: String::new(),
        tsrs: Vec::new(),
        oracle,
        oracle_empty_related_information: Vec::new(),
        tsrs_cli_hash: String::new(),
        oracle_cli_hash: String::new(),
    }
}

struct TombstoneFixture {
    root: PathBuf,
    head: String,
    file: ScopeFile,
    cases: BTreeMap<(String, String), GoldenCase>,
    bucket: T0Key,
    identity: ExactIdentity,
}

fn tombstone_fixture(oracle: Vec<GoldenDiag>) -> TombstoneFixture {
    let root = init_repo("tombstone");
    let resolving = commit_scope(
        &root,
        &scope_file(ScopeStatus::Draft, Vec::new()),
        "resolving commit",
    );
    let identity = identity_at(&oracle, 0);
    let bucket = t0_key(&oracle[0]);
    let mut cases = BTreeMap::new();
    cases.insert((FIXTURE.to_owned(), String::new()), golden_case(oracle));
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.tombstones = vec![tombstone_of(identity.clone(), &resolving)];
    let head = resolve_commit(&root, "HEAD").unwrap();
    TombstoneFixture {
        root,
        head,
        file,
        cases,
        bucket,
        identity,
    }
}

#[test]
fn tombstone_singleton_proof_round_trip() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let TombstoneFixture {
        root,
        head,
        file,
        cases,
        bucket,
        ..
    } = tombstone_fixture(oracle);
    let matches = matches_stub(views_with("all", &bucket, false));
    verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches).unwrap();
}

#[test]
fn tombstone_without_a1_membership_is_rejected() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let TombstoneFixture {
        root,
        head,
        file,
        cases,
        ..
    } = tombstone_fixture(oracle);
    let matches = matches_stub(RunSets::new());
    let error = verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches)
        .unwrap_err()
        .to_string();
    assert!(error.contains("lacks its standing proof"), "{error}");
    assert!(error.contains("A1's all view"), "{error}");
}

#[test]
fn tombstone_duplicate_bucket_requires_multiplicity_completeness() {
    let oracle = vec![
        diag(2695, 29, "semantic", "unused"),
        diag(2695, 29, "semantic", "unused"),
    ];
    let TombstoneFixture {
        root,
        head,
        file,
        cases,
        bucket,
        ..
    } = tombstone_fixture(oracle);
    // Matched but not multiplicity-complete: cannot prove which
    // occurrence resolved.
    let matches = matches_stub(views_with("all", &bucket, false));
    let error = verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches)
        .unwrap_err()
        .to_string();
    assert!(error.contains("multiplicity-complete"), "{error}");
    // Multiplicity-complete: proven.
    let matches = matches_stub(views_with("all", &bucket, true));
    verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches).unwrap();
}

#[test]
fn tombstone_under_a_band_pin_reads_the_band_view() {
    // The identity is pinned in 2xxx: membership in the All view
    // alone cannot prove it — the pin's own view must hold it.
    let oracle = in_band_oracle();
    let TombstoneFixture {
        root,
        head,
        mut file,
        cases,
        bucket,
        identity,
    } = tombstone_fixture(oracle);
    file.band_pins = vec![pin_of("2xxx", FAKE_SHA, vec![identity])];
    let matches = matches_stub(views_with("all", &bucket, false));
    let error = verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches)
        .unwrap_err()
        .to_string();
    assert!(error.contains("A1's 2xxx view"), "{error}");
    let matches = matches_stub(views_with("2xxx", &bucket, false));
    verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches).unwrap();
}

#[test]
fn tombstone_non_ancestor_resolving_commit_is_rejected() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let root = init_repo("tombstone-ancestor");
    commit_scope(&root, &scope_file(ScopeStatus::Draft, Vec::new()), "base");
    git_test(&root, &["checkout", "-q", "-b", "side"]);
    let mut side_content = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    side_content.exclusions[0].evidence = "side variant".to_owned();
    let side = commit_scope(&root, &side_content, "side");
    git_test(&root, &["checkout", "-q", "main"]);
    let mut main_content = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    main_content.exclusions[0].evidence = "main variant".to_owned();
    commit_scope(&root, &main_content, "main");
    let head = resolve_commit(&root, "HEAD").unwrap();

    let identity = identity_at(&oracle, 0);
    let bucket = t0_key(&oracle[0]);
    let mut cases = BTreeMap::new();
    cases.insert((FIXTURE.to_owned(), String::new()), golden_case(oracle));
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.tombstones = vec![tombstone_of(identity, &side)];
    let matches = matches_stub(views_with("all", &bucket, false));
    let error = verify_tombstone(&root, &head, &file, &file.tombstones[0], &cases, &matches)
        .unwrap_err()
        .to_string();
    assert!(error.contains("not an ancestor of HEAD"), "{error}");
}

#[test]
fn tombstone_still_live_is_rejected() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let mut file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    file.tombstones = vec![tombstone_of(identity_at(&oracle, 0), FAKE_SHA)];
    let error = load_err("tombstone-live", &file);
    assert!(error.contains("still a live exclusion"), "{error}");
}

// -- A2 global ------------------------------------------------------------

#[test]
fn frozen_without_global_record_is_rejected() {
    let file = scope_file(ScopeStatus::Frozen, Vec::new());
    let error = load_err("frozen-bare", &file);
    assert!(error.contains("without a global-freeze record"), "{error}");
}

#[test]
fn draft_with_global_record_is_rejected() {
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: Vec::new(),
    });
    let error = load_err("draft-global", &file);
    assert!(error.contains("while draft"), "{error}");
}

#[test]
fn frozen_addition_fails_structurally() {
    // After freeze, additions and edits never occur: a live
    // exclusion outside the global set fails at load.
    let oracle = in_band_oracle();
    let mut file = scope_file(
        ScopeStatus::Frozen,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 1)],
    );
    file.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let error = load_err("frozen-add", &file);
    assert!(error.contains("not in the global-freeze set"), "{error}");
}

#[test]
fn frozen_disappearance_needs_a_tombstone() {
    let oracle = in_band_oracle();
    let mut file = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    file.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0), identity_at(&oracle, 1)],
    });
    let error = load_err("frozen-gone", &file);
    assert!(error.contains("disappeared without a tombstone"), "{error}");

    file.tombstones = vec![tombstone_of(identity_at(&oracle, 1), FAKE_SHA)];
    load_file("frozen-tombstoned", &file).unwrap();
}

#[test]
fn global_freeze_anchor_round_trip_and_attacks() {
    let root = init_repo("global-anchor");
    let oracle = in_band_oracle();
    let content = scope_file(
        ScopeStatus::Draft,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 1)],
    );
    let adjudication = commit_scope(&root, &content, "reviewed content lands while draft");
    let mut frozen = content.clone();
    frozen.status = ScopeStatus::Frozen;
    frozen.global = Some(GlobalFreeze {
        adjudication_commit: adjudication.clone(),
        identities: vec![identity_at(&oracle, 0), identity_at(&oracle, 1)],
    });
    commit_scope(&root, &frozen, "freeze record");
    let head = resolve_commit(&root, "HEAD").unwrap();
    verify_global_freeze(
        &root,
        SCOPE_REL_PATH,
        &head,
        &frozen,
        frozen.global.as_ref().unwrap(),
    )
    .unwrap();

    // Rewritten set: the identity comparison against the
    // adjudication commit fails.
    let rewritten = GlobalFreeze {
        adjudication_commit: adjudication.clone(),
        identities: vec![identity_at(&oracle, 0)],
    };
    let error = verify_global_freeze(&root, SCOPE_REL_PATH, &head, &frozen, &rewritten)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("does not equal the live identity set"),
        "{error}"
    );

    // Anchoring on the freeze commit itself (already frozen, not
    // draft) violates the two-step protocol.
    let self_anchored = GlobalFreeze {
        adjudication_commit: head.clone(),
        identities: vec![identity_at(&oracle, 0), identity_at(&oracle, 1)],
    };
    let error = verify_global_freeze(&root, SCOPE_REL_PATH, &head, &frozen, &self_anchored)
        .unwrap_err()
        .to_string();
    assert!(error.contains("two-step freeze"), "{error}");
}

// -- trusted-base compare ---------------------------------------------------

/// Baseline harness: base manifest committed on main, head file
/// held in memory (the working tree under audit).
fn baseline_repo(base: &ScopeFile) -> (PathBuf, String) {
    let root = init_repo("baseline");
    let commit = commit_scope(&root, base, "trusted base");
    (root, commit)
}

#[test]
fn baseline_pre_a2_base_allows_draft_but_not_freeze() {
    let root = init_repo("baseline-absent");
    fs::write(root.join("other.txt"), b"x").unwrap();
    git_test(&root, &["add", "other.txt"]);
    git_test(&root, &["commit", "-q", "-m", "no manifest"]);

    let draft = scope_file(ScopeStatus::Draft, Vec::new());
    verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &draft).unwrap();

    let oracle = in_band_oracle();
    let mut frozen = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    frozen.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &frozen)
        .unwrap_err()
        .to_string();
    assert!(error.contains("schema-2 draft trusted base"), "{error}");
}

#[test]
fn baseline_schema_1_base_allows_draft_but_not_freeze() {
    let root = init_repo("baseline-schema1");
    fs::write(
        root.join(SCOPE_REL_PATH),
        br#"{"schema":1,"status":"draft","exclusions":[]}"#,
    )
    .unwrap();
    git_test(&root, &["add", SCOPE_REL_PATH]);
    git_test(&root, &["commit", "-q", "-m", "schema 1 base"]);

    let draft = scope_file(ScopeStatus::Draft, Vec::new());
    verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &draft).unwrap();

    let oracle = in_band_oracle();
    let mut frozen = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    frozen.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &frozen)
        .unwrap_err()
        .to_string();
    assert!(error.contains("retired schema-1"), "{error}");
}

#[test]
fn baseline_draft_edits_stay_reviewable() {
    // Unpinned draft exclusions may change between base and head.
    let oracle = in_band_oracle();
    let base = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    let (root, _) = baseline_repo(&base);
    let head = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 1)]);
    verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head).unwrap();
}

#[test]
fn baseline_status_downgrade_is_rejected() {
    let oracle = in_band_oracle();
    let mut base = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    base.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let (root, _) = baseline_repo(&base);
    let head = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(error.contains("status downgrade"), "{error}");
}

#[test]
fn baseline_global_reanchor_is_rejected() {
    // A branch cannot delete-and-recreate the freeze record with
    // a different anchor: the global records must be
    // byte-identical after the first valid transition.
    let oracle = in_band_oracle();
    let mut base = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    base.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let (root, _) = baseline_repo(&base);

    let mut head = base.clone();
    head.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA_2.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(error.contains("global-freeze record changed"), "{error}");
}

#[test]
fn baseline_frozen_resurrection_is_rejected() {
    // A tombstoned identity cannot quietly return to the live set
    // on a branch: the head exclusion does not exist at the base.
    let oracle = in_band_oracle();
    let global = GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0), identity_at(&oracle, 1)],
    };
    let mut base = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    base.global = Some(global.clone());
    base.tombstones = vec![tombstone_of(identity_at(&oracle, 1), FAKE_SHA)];
    let (root, _) = baseline_repo(&base);

    let mut head = scope_file(
        ScopeStatus::Frozen,
        vec![exclusion_of(&oracle, 0), exclusion_of(&oracle, 1)],
    );
    head.global = Some(global);
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("does not exist at the trusted base"),
        "{error}"
    );
}

#[test]
fn baseline_band_pin_mutation_and_removal_are_rejected() {
    let oracle = in_band_oracle();
    let mut base = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    base.band_pins = vec![pin_of("2xxx", FAKE_SHA, vec![identity_at(&oracle, 0)])];
    let (root, _) = baseline_repo(&base);

    // Mutation: same band, different anchor (add-and-reanchor).
    let mut head = base.clone();
    head.band_pins = vec![pin_of("2xxx", FAKE_SHA_2, vec![identity_at(&oracle, 0)])];
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("changed against the trusted base"),
        "{error}"
    );

    // Removal.
    let mut head = base.clone();
    head.band_pins = Vec::new();
    // Structural validation would also complain about the pin's
    // identities, but the baseline compare must fail on its own.
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(error.contains("was removed"), "{error}");
}

#[test]
fn baseline_tombstone_removal_is_rejected() {
    let oracle = in_band_oracle();
    let mut base = scope_file(ScopeStatus::Draft, Vec::new());
    base.tombstones = vec![tombstone_of(identity_at(&oracle, 0), FAKE_SHA)];
    let (root, _) = baseline_repo(&base);

    let head = scope_file(ScopeStatus::Draft, Vec::new());
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(error.contains("tombstone"), "{error}");
    assert!(error.contains("was removed"), "{error}");
}

#[test]
fn baseline_first_freeze_transition_from_draft_base_passes() {
    let oracle = in_band_oracle();
    let base = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    let (root, commit) = baseline_repo(&base);
    let mut head = base.clone();
    head.status = ScopeStatus::Frozen;
    head.global = Some(GlobalFreeze {
        adjudication_commit: commit,
        identities: vec![identity_at(&oracle, 0)],
    });
    verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head).unwrap();
}

// -- anchors are full commit SHAs -----------------------------------------

#[test]
fn movable_ref_anchors_are_rejected_structurally() {
    let oracle = in_band_oracle();
    let mut file = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    file.band_pins = vec![pin_of("2xxx", "HEAD", vec![identity_at(&oracle, 0)])];
    let error = load_err("pin-movable", &file);
    assert!(error.contains("full 40-hex commit SHA"), "{error}");

    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.tombstones = vec![tombstone_of(identity_at(&oracle, 0), "main")];
    let error = load_err("tombstone-movable", &file);
    assert!(error.contains("full 40-hex commit SHA"), "{error}");

    let mut file = scope_file(ScopeStatus::Frozen, Vec::new());
    file.global = Some(GlobalFreeze {
        adjudication_commit: "v1.0".to_owned(),
        identities: Vec::new(),
    });
    let error = load_err("global-movable", &file);
    assert!(error.contains("full 40-hex commit SHA"), "{error}");
}

#[test]
fn anchor_must_name_the_commit_directly() {
    let root = init_repo("anchor-direct");
    let oracle = in_band_oracle();
    let commit = commit_scope(
        &root,
        &scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]),
        "content",
    );
    // An abbreviation resolves, but not to itself: the recorded
    // anchor must literally be the commit SHA.
    let error = resolve_anchor(&root, &commit[..12], "M8 scope band pin \"2xxx\"")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("does not name a commit object directly"),
        "{error}"
    );
    assert_eq!(
        resolve_anchor(&root, &commit, "M8 scope band pin \"2xxx\"").unwrap(),
        commit
    );
}

// -- lapsed tombstones (reviewed-transition window) -----------------------

#[test]
fn active_tombstone_without_resolving_commit_is_rejected() {
    let oracle = in_band_oracle();
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.tombstones = vec![Tombstone {
        identity: identity_at(&oracle, 0),
        resolving_commit: None,
        lapsed: false,
    }];
    let error = load_err("tombstone-no-commit", &file);
    assert!(error.contains("no resolving commit"), "{error}");
}

#[test]
fn lapsed_tombstone_satisfies_a_pinned_disappearance() {
    // The wedge this unblocks: a pinned occurrence removed by a
    // reviewed transition can never prove A1 membership again, so
    // its record is a lapsed tombstone (here without a resolving
    // commit — it lapsed while still excluded).
    let oracle = in_band_oracle();
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.band_pins = vec![pin_of("2xxx", FAKE_SHA, vec![identity_at(&oracle, 0)])];
    file.tombstones = vec![lapsed_tombstone_of(identity_at(&oracle, 0), None)];
    load_file("pin-lapsed", &file).unwrap();
}

#[test]
fn lapsed_tombstone_with_surviving_occurrence_is_rejected() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let identity = identity_at(&oracle, 0);
    let mut file = scope_file(ScopeStatus::Draft, Vec::new());
    file.tombstones = vec![lapsed_tombstone_of(identity.clone(), None)];
    let lapsed = [identity.clone()].into_iter().collect::<BTreeSet<_>>();
    let entries = vec![("tombstone", ReferenceNeed::Lapsed, identity)];
    let error = resolve_referenced(FIXTURE, &golden_case(oracle), &entries, &file, &lapsed)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("marked lapsed but its occurrence still exists"),
        "{error}"
    );
}

#[test]
fn lapsed_coverage_spares_pin_members_but_not_exclusions() {
    // A pinned identity covered by a lapsed tombstone tolerates the
    // vanished occurrence; a live exclusion never does.
    let present = vec![diag(2307, 0, "semantic", "missing")];
    let vanished_identity = {
        let gone = vec![diag(2322, 5, "semantic", "not assignable")];
        identity_at(&gone, 0)
    };
    let file = scope_file(ScopeStatus::Draft, Vec::new());
    let lapsed = [vanished_identity.clone()]
        .into_iter()
        .collect::<BTreeSet<_>>();
    let entries = vec![(
        "band-pin identity",
        ReferenceNeed::Anchored,
        vanished_identity.clone(),
    )];
    resolve_referenced(
        FIXTURE,
        &golden_case(present.clone()),
        &entries,
        &file,
        &lapsed,
    )
    .unwrap();

    let entries = vec![(
        "band-pin identity",
        ReferenceNeed::Anchored,
        vanished_identity,
    )];
    let error = resolve_referenced(
        FIXTURE,
        &golden_case(present),
        &entries,
        &file,
        &BTreeSet::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("stale M8 scope band-pin identity"),
        "{error}"
    );
}

#[test]
fn lapsed_tombstone_anchor_still_verifies() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let root = init_repo("lapsed-anchor");
    let resolving = commit_scope(&root, &scope_file(ScopeStatus::Draft, Vec::new()), "base");
    git_test(&root, &["checkout", "-q", "-b", "side"]);
    let mut side_content = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    side_content.exclusions[0].evidence = "side variant".to_owned();
    let side = commit_scope(&root, &side_content, "side");
    git_test(&root, &["checkout", "-q", "main"]);
    let mut main_content = scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]);
    main_content.exclusions[0].evidence = "main variant".to_owned();
    commit_scope(&root, &main_content, "main");
    let head = resolve_commit(&root, "HEAD").unwrap();
    let identity = identity_at(&oracle, 0);

    verify_lapsed_tombstone_anchor(&root, &head, &lapsed_tombstone_of(identity.clone(), None))
        .unwrap();
    verify_lapsed_tombstone_anchor(
        &root,
        &head,
        &lapsed_tombstone_of(identity.clone(), Some(&resolving)),
    )
    .unwrap();
    let error =
        verify_lapsed_tombstone_anchor(&root, &head, &lapsed_tombstone_of(identity, Some(&side)))
            .unwrap_err()
            .to_string();
    assert!(error.contains("not an ancestor of HEAD"), "{error}");
}

#[test]
fn baseline_tombstone_provenance_mutation_is_rejected() {
    let oracle = in_band_oracle();
    let mut base = scope_file(ScopeStatus::Draft, Vec::new());
    base.tombstones = vec![tombstone_of(identity_at(&oracle, 0), FAKE_SHA)];
    let (root, _) = baseline_repo(&base);

    // Provenance mutation fails even though the identity survives.
    let mut head = scope_file(ScopeStatus::Draft, Vec::new());
    head.tombstones = vec![tombstone_of(identity_at(&oracle, 0), FAKE_SHA_2)];
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(error.contains("resolving commit changed"), "{error}");

    // Dropping the recorded commit fails the same way.
    let mut head = scope_file(ScopeStatus::Draft, Vec::new());
    head.tombstones = vec![lapsed_tombstone_of(identity_at(&oracle, 0), None)];
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head)
        .unwrap_err()
        .to_string();
    assert!(error.contains("resolving commit changed"), "{error}");

    // The lapsed flip with preserved provenance passes.
    let mut head = scope_file(ScopeStatus::Draft, Vec::new());
    head.tombstones = vec![lapsed_tombstone_of(identity_at(&oracle, 0), Some(FAKE_SHA))];
    verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &head).unwrap();
}

// -- encoder migration windows -------------------------------------------

fn commit_raw_scope(root: &Path, bytes: &[u8], message: &str) -> String {
    fs::write(root.join(SCOPE_REL_PATH), bytes).unwrap();
    git_test(root, &["add", SCOPE_REL_PATH]);
    git_test(root, &["commit", "-q", "-m", message]);
    String::from_utf8(git(root, &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .to_owned()
}

#[test]
fn baseline_older_encoder_base_is_the_migration_window() {
    let root = init_repo("baseline-encoder");
    commit_raw_scope(
        &root,
        br#"{"schema":2,"encoder":0,"status":"draft","exclusions":[]}"#,
        "old-encoder base",
    );
    let draft = scope_file(ScopeStatus::Draft, Vec::new());
    verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &draft).unwrap();

    let oracle = in_band_oracle();
    let mut frozen = scope_file(ScopeStatus::Frozen, vec![exclusion_of(&oracle, 0)]);
    frozen.global = Some(GlobalFreeze {
        adjudication_commit: FAKE_SHA.to_owned(),
        identities: vec![identity_at(&oracle, 0)],
    });
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &frozen)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("cannot ride the encoder migration"),
        "{error}"
    );
}

#[test]
fn baseline_frozen_base_has_no_encoder_bump_path() {
    let root = init_repo("baseline-encoder-frozen");
    commit_raw_scope(
        &root,
        br#"{"schema":2,"encoder":0,"status":"frozen","exclusions":[]}"#,
        "frozen old-encoder base",
    );
    let draft = scope_file(ScopeStatus::Draft, Vec::new());
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &draft)
        .unwrap_err()
        .to_string();
    assert!(error.contains("no sanctioned path"), "{error}");
}

#[test]
fn baseline_encoder_downgrade_is_rejected() {
    let root = init_repo("baseline-encoder-downgrade");
    commit_raw_scope(
        &root,
        br#"{"schema":2,"encoder":99,"status":"draft","exclusions":[]}"#,
        "future-encoder base",
    );
    let draft = scope_file(ScopeStatus::Draft, Vec::new());
    let error = verify_scope_baseline(&root, SCOPE_REL_PATH, "HEAD", &draft)
        .unwrap_err()
        .to_string();
    assert!(error.contains("downgrade never occurs"), "{error}");
}

#[test]
fn band_pin_anchor_under_another_encoder_reports_the_migration() {
    let root = init_repo("pin-encoder");
    let adjudication = commit_raw_scope(
        &root,
        br#"{"schema":2,"encoder":0,"status":"draft","exclusions":[]}"#,
        "old-encoder adjudication",
    );
    let oracle = in_band_oracle();
    commit_scope(
        &root,
        &scope_file(ScopeStatus::Draft, vec![exclusion_of(&oracle, 0)]),
        "current-encoder content",
    );
    let head = resolve_commit(&root, "HEAD").unwrap();
    let pin = pin_of("2xxx", &adjudication, vec![identity_at(&oracle, 0)]);
    let error = verify_band_pin(&root, SCOPE_REL_PATH, &head, &pin)
        .unwrap_err()
        .to_string();
    assert!(error.contains("incomparable"), "{error}");
    assert!(error.contains("re-anchor"), "{error}");
}

// -- shared resolution / canary / view helpers ---------------------------

#[test]
fn ambiguous_identity_resolution_is_a_hard_error() {
    let oracle = vec![diag(2307, 0, "semantic", "missing")];
    let identity = identity_at(&oracle, 0);
    let duplicated = vec![identity.clone(), identity.clone()];
    let error = resolve_identity_index(&duplicated, &identity, "exclusion")
        .unwrap_err()
        .to_string();
    assert!(error.contains("canonical encoder bug"), "{error}");
    assert!(error.contains("2 oracle occurrences"), "{error}");
}

#[test]
fn unknown_band_has_no_ratchet_view() {
    let error = ratchet_view_for_band("5xxx").unwrap_err().to_string();
    assert!(error.contains("no fixed A1 view"), "{error}");
}

#[test]
fn reorder_canary_shape_and_vacuity_are_hard_errors() {
    let canary_file = |records: Vec<GoldenDiag>| VectorFile {
        encoder: ENCODER_VERSION,
        cases: vec![VectorCase {
            name: "nested-chains-child-order".to_owned(),
            fixture: FIXTURE.to_owned(),
            matrix_key: String::new(),
            records,
        }],
    };
    // Wrong shape is an error, not an index panic.
    let error = verify_reorder_canaries(&canary_file(vec![diag(2307, 0, "semantic", "m")]))
        .unwrap_err()
        .to_string();
    assert!(error.contains("exactly two records"), "{error}");

    // Two records whose reorder-target hash is identical fail even
    // though their identities differ (start differs) — the check
    // the vacuous whole-identity comparison would have passed.
    let error = verify_reorder_canaries(&canary_file(vec![
        diag(2307, 0, "semantic", "m"),
        diag(2307, 5, "semantic", "m"),
    ]))
    .unwrap_err()
    .to_string();
    assert!(error.contains("hash identically"), "{error}");
}
