use super::*;
use crate::test_git::{git_test, init_repo, temp_dir};
use crate::GoldenMessageChain;

fn commit_bytes(root: &Path, rel: &str, bytes: &[u8], message: &str) -> String {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, bytes).unwrap();
    git_test(root, &["add", rel]);
    git_test(root, &["commit", "-q", "-m", message]);
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn commit_artifact_pair(
    root: &Path,
    matches_bytes: &[u8],
    inputs_bytes: &[u8],
    message: &str,
) -> String {
    commit_artifact_pair_at(
        root,
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        matches_bytes,
        inputs_bytes,
        message,
    )
}

fn commit_artifact_pair_at(
    root: &Path,
    matches_rel: &str,
    inputs_rel: &str,
    matches_bytes: &[u8],
    inputs_bytes: &[u8],
    message: &str,
) -> String {
    for (rel, bytes) in [(matches_rel, matches_bytes), (inputs_rel, inputs_bytes)] {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    git_test(root, &["add", matches_rel, inputs_rel]);
    git_test(root, &["commit", "-q", "-m", message]);
    let out = git(root, &["rev-parse", "HEAD"]).unwrap();
    String::from_utf8(out).unwrap().trim().to_owned()
}

fn legacy_rel(rel: &str) -> String {
    format!("{LEGACY_WORKSPACE_PREFIX}{rel}")
}

fn commit_all(root: &Path, message: &str) -> String {
    git_test(root, &["add", "-A"]);
    git_test(root, &["commit", "-q", "-m", message]);
    String::from_utf8(git(root, &["rev-parse", "HEAD"]).unwrap())
        .unwrap()
        .trim()
        .to_owned()
}

fn key(code: u32) -> T0Key {
    T0Key {
        file: Some("a.ts".to_owned()),
        code,
        line: Some(1),
        col: Some(2),
    }
}

/// One case in the "all" view on fixture `conformance/a.ts`; the
/// other fixed views stay present-but-empty.
fn views_with(matched: &[u32], complete: &[u32]) -> RunSets {
    views_with_tiers(matched, complete, &[], &[], &[])
}

fn views_with_tiers(
    matched: &[u32],
    complete: &[u32],
    t1: &[u32],
    t2: &[u32],
    t3: &[u32],
) -> RunSets {
    let mut sets = CaseSets::default();
    for code in matched {
        sets.matched.insert(key(*code));
    }
    for code in complete {
        sets.multiplicity_complete.insert(key(*code));
    }
    for code in t1 {
        sets.t1.insert(key(*code));
    }
    for code in t2 {
        sets.t2.insert(key(*code));
    }
    for code in t3 {
        sets.t3.insert(key(*code));
    }
    let mut views: RunSets = FIXED_VIEWS
        .iter()
        .map(|view| (view.name().to_owned(), ViewSets::new()))
        .collect();
    views
        .get_mut("all")
        .unwrap()
        .entry("conformance/a.ts".to_owned())
        .or_default()
        .insert(String::new(), sets);
    views
}

fn matches_artifact(
    views: RunSets,
    bootstrap: bool,
    previous: Option<Lineage>,
    transition: Option<String>,
) -> MatchesArtifact {
    MatchesArtifact {
        schema: MATCHES_SCHEMA,
        bootstrap,
        previous,
        transition,
        inputs: MatchesInputs {
            oracle_inputs_sha256: "inputs".to_owned(),
            tsc_js_sha256: "tsc".to_owned(),
        },
        views,
        lapsed: None,
    }
}

fn lineage_to(commit: &str, bytes: &[u8]) -> Lineage {
    Lineage {
        commit: commit.to_owned(),
        sha256: sha256_hex(bytes),
    }
}

fn inputs_stub() -> OracleInputsArtifact {
    let mut fixtures = BTreeMap::new();
    fixtures.insert(
        "conformance/a.ts".to_owned(),
        FixturePins {
            fixture_sha256: "f".to_owned(),
            cases: [(
                String::new(),
                CasePins {
                    oracle_sha256: "o".to_owned(),
                    program_sha256: "p".to_owned(),
                    oracle_t4_sha256: None,
                },
            )]
            .into_iter()
            .collect(),
        },
    );
    OracleInputsArtifact {
        schema: ORACLE_INPUTS_SCHEMA,
        bootstrap: true,
        previous: None,
        transition: None,
        vendor: VendorPins {
            tsc_js_sha256: "tsc".to_owned(),
            lib_sha256: "lib".to_owned(),
        },
        producer: None,
        comparators: inactive_comparators(),
        fixtures,
        totals: FIXED_VIEWS
            .iter()
            .map(|view| (view.name().to_owned(), 1u64))
            .collect(),
    }
}

fn producer_stub() -> ProducerPins {
    ProducerPins {
        driver_sha256: "driver".to_owned(),
        program_host_sha256: "host".to_owned(),
        typescript_js_sha256: "tsjs".to_owned(),
        node_version: "25.2.1".to_owned(),
        render_driver_sha256: None,
    }
}

fn active_t4_inputs() -> OracleInputsArtifact {
    let mut inputs = inputs_stub();
    inputs.producer = Some(producer_stub());
    inputs.producer.as_mut().unwrap().render_driver_sha256 = Some("a".repeat(64));
    inputs.comparators = tier_1_4_comparators();
    inputs
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_t4_sha256 = Some("b".repeat(64));
    inputs
}

fn diag(code: u32, start: u32, pass: &str) -> GoldenDiag {
    GoldenDiag {
        file: Some("a.ts".to_owned()),
        start: Some(start),
        length: Some(1),
        line: Some(1),
        col: Some(start),
        code,
        pass: Some(pass.to_owned()),
        category: "error".to_owned(),
        chain: GoldenMessageChain {
            text: format!("diag {code} at {start}"),
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

// -- A1 views ----------------------------------------------------------

#[test]
fn bucket_sets_grade_matched_and_multiplicity() {
    // Duplicate bucket 2/2 → matched + complete; 2/1 → matched
    // only; 1/0 → neither; a tsrs-only key never enters either set.
    let mut a = diag(2322, 5, "semantic");
    a.col = Some(5);
    let mut b = diag(2322, 5, "semantic");
    b.chain.text = "second occurrence".to_owned();
    b.col = Some(5);
    let oracle = [a.clone(), b.clone(), {
        let mut c = diag(2454, 9, "semantic");
        c.col = Some(9);
        c
    }];

    let complete = bucket_sets(oracle.iter(), [a.clone(), b.clone()].iter());
    assert!(complete.matched.contains(&t0_key(&a)));
    assert!(complete.multiplicity_complete.contains(&t0_key(&a)));
    assert!(complete.t1.contains(&t0_key(&a)));
    assert!(complete.t2.contains(&t0_key(&a)));
    assert!(complete.t3.contains(&t0_key(&a)));
    assert!(!complete.matched.contains(&t0_key(&oracle[2])));

    let partial = bucket_sets(oracle.iter(), std::slice::from_ref(&a).iter());
    assert!(partial.matched.contains(&t0_key(&a)));
    assert!(
        !partial.multiplicity_complete.contains(&t0_key(&a)),
        "a 2/1 bucket must not be multiplicity-complete"
    );
    assert!(!partial.t1.contains(&t0_key(&a)));
    assert!(!partial.t2.contains(&t0_key(&a)));
    assert!(!partial.t3.contains(&t0_key(&a)));

    let fp_side = diag(9999, 1, "semantic");
    let fp = bucket_sets(oracle.iter(), std::slice::from_ref(&fp_side).iter());
    assert!(!fp.matched.contains(&t0_key(&fp_side)));
    assert!(!fp.multiplicity_complete.contains(&t0_key(&fp_side)));
}

#[test]
fn bucket_sets_grade_complete_multisets_at_each_tier() {
    let oracle = diag(2322, 5, "semantic");
    let mut category = oracle.clone();
    category.category = "suggestion".to_owned();
    assert!(bucket_sets([&oracle].into_iter(), [&category].into_iter())
        .t1
        .is_empty());

    let mut span = oracle.clone();
    span.length = Some(2);
    let span_sets = bucket_sets([&oracle].into_iter(), [&span].into_iter());
    assert!(span_sets.t1.contains(&t0_key(&oracle)));
    assert!(span_sets.t2.is_empty());

    let mut chain = oracle.clone();
    chain.chain.next.push(GoldenMessageChain {
        text: "nested".to_owned(),
        code: 2322,
        category: "error".to_owned(),
        next: Vec::new(),
    });
    let chain_sets = bucket_sets([&oracle].into_iter(), [&chain].into_iter());
    assert!(chain_sets.t2.contains(&t0_key(&oracle)));
    assert!(chain_sets.t3.is_empty());

    // Independent complete-multiset matching: reversing duplicate
    // records does not change the tier identity.
    let mut second = oracle.clone();
    second.category = "warning".to_owned();
    let permuted = bucket_sets(
        [&oracle, &second].into_iter(),
        [&second, &oracle].into_iter(),
    );
    assert!(permuted.t3.contains(&t0_key(&oracle)));
}

#[test]
fn accepted_artifact_validates_nested_tier_sets() {
    let valid = matches_artifact(
        views_with_tiers(&[100], &[100], &[100], &[100], &[100]),
        true,
        None,
        None,
    );
    valid.validate().unwrap();

    for (label, views) in [
        ("T1", views_with_tiers(&[100], &[], &[100], &[], &[])),
        ("T2", views_with_tiers(&[100], &[100], &[], &[100], &[])),
        ("T3", views_with_tiers(&[100], &[100], &[100], &[], &[100])),
    ] {
        let err = matches_artifact(views, true, None, None)
            .validate()
            .unwrap_err()
            .to_string();
        assert!(err.contains(label), "{label}: {err}");
    }
}

#[test]
fn enforce_names_exact_tier_removal_and_projects_partial_runs() {
    let mut accepted_views = views_with_tiers(&[2322], &[2322], &[2322], &[2322], &[2322]);
    accepted_views.get_mut("all").unwrap().insert(
        "conformance/b.ts".to_owned(),
        [(
            String::new(),
            views_with_tiers(&[2345], &[2345], &[2345], &[2345], &[2345])["all"]
                ["conformance/a.ts"][""]
                .clone(),
        )]
        .into_iter()
        .collect(),
    );
    let accepted = matches_artifact(accepted_views, true, None, None);
    let current = views_with_tiers(&[2322], &[2322], &[2322], &[2322], &[]);
    let executed: BTreeSet<String> = [String::from("conformance/a.ts")].into_iter().collect();
    let err = enforce_accepted(&accepted, &current, DiagnosticBand::All, &executed, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("T3 (all)"), "{err}");
    assert!(err.contains("code 2322"), "{err}");
    assert!(!err.contains("code 2345"), "{err}");
}

#[test]
fn enforce_names_matched_removal() {
    let accepted = matches_artifact(views_with(&[2322, 2345], &[2322]), true, None, None);
    let current = views_with(&[2322], &[2322]);
    let executed: BTreeSet<String> = [String::from("conformance/a.ts")].into_iter().collect();
    let err = enforce_accepted(&accepted, &current, DiagnosticBand::All, &executed, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("matched (all)"), "{err}");
    assert!(err.contains("code 2345"), "{err}");
    assert!(err.contains("conformance/a.ts"), "{err}");
}

#[test]
fn enforce_names_multiplicity_regression_2_2_to_2_1() {
    // The T0 key stays matched; only the completeness set loses
    // the bucket. The gate must still fail and name it.
    let accepted = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let current = views_with(&[2322], &[]);
    let executed: BTreeSet<String> = [String::from("conformance/a.ts")].into_iter().collect();
    let err = enforce_accepted(&accepted, &current, DiagnosticBand::All, &executed, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("multiplicity-complete (all)"), "{err}");
    assert!(err.contains("code 2322"), "{err}");
}

#[test]
fn enforce_syntactic_view_is_independent() {
    // A semantic gain cannot hide a syntactic FN: the syntactic
    // view's accepted subset is enforced on its own.
    let mut accepted_views = views_with(&[1005], &[1005]);
    accepted_views
        .get_mut("syntactic")
        .unwrap()
        .entry("conformance/a.ts".to_owned())
        .or_default()
        .insert(
            String::new(),
            CaseSets {
                matched: [key(1005)].into_iter().collect(),
                multiplicity_complete: [key(1005)].into_iter().collect(),
                t1: [key(1005)].into_iter().collect(),
                t2: [key(1005)].into_iter().collect(),
                t3: [key(1005)].into_iter().collect(),
                t4: false,
            },
        );
    let accepted = matches_artifact(accepted_views, true, None, None);
    let mut current = views_with(&[1005, 2322, 2345], &[1005]);
    current
        .get_mut("syntactic")
        .unwrap()
        .entry("conformance/a.ts".to_owned())
        .or_default()
        .insert(
            String::new(),
            CaseSets {
                matched: [key(1005)].into_iter().collect(),
                multiplicity_complete: [key(1005)].into_iter().collect(),
                t1: [key(1005)].into_iter().collect(),
                t2: [key(1005)].into_iter().collect(),
                t3: BTreeSet::new(),
                t4: false,
            },
        );
    let executed: BTreeSet<String> = [String::from("conformance/a.ts")].into_iter().collect();
    enforce_accepted(&accepted, &current, DiagnosticBand::All, &executed, true).unwrap();
    let err = enforce_accepted(
        &accepted,
        &current,
        DiagnosticBand::Syntactic,
        &executed,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("T3 (syntactic)"), "{err}");
    assert!(err.contains("code 1005"), "{err}");
}

#[test]
fn enforce_projects_partial_runs_to_executed_fixtures() {
    let mut accepted_views = views_with(&[2322], &[2322]);
    accepted_views
        .get_mut("all")
        .unwrap()
        .entry("conformance/b.ts".to_owned())
        .or_default()
        .insert(
            String::new(),
            CaseSets {
                matched: [key(2345)].into_iter().collect(),
                multiplicity_complete: BTreeSet::new(),
                ..CaseSets::default()
            },
        );
    let accepted = matches_artifact(accepted_views, true, None, None);
    let executed: BTreeSet<String> = [String::from("conformance/a.ts")].into_iter().collect();

    // b.ts was not executed: its accepted identity is not demanded.
    let current = views_with(&[2322], &[2322]);
    enforce_accepted(&accepted, &current, DiagnosticBand::All, &executed, false).unwrap();

    // But the executed fixture's accepted subset still gates.
    let regressed = views_with(&[], &[]);
    let err = enforce_accepted(&accepted, &regressed, DiagnosticBand::All, &executed, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("code 2322"), "{err}");
    assert!(!err.contains("code 2345"), "{err}");
}

#[test]
fn enforce_full_run_requires_every_accepted_fixture() {
    let accepted = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let executed: BTreeSet<String> = [String::from("conformance/other.ts")].into_iter().collect();
    let err = enforce_accepted(
        &accepted,
        &views_with(&[], &[]),
        DiagnosticBand::All,
        &executed,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("no longer in the corpus"), "{err}");
    assert!(err.contains("conformance/a.ts"), "{err}");
}

#[test]
fn bootstrap_measurement_requires_the_exact_current_sets() {
    let exact = views_with(&[2322, 2345], &[2322]);
    verify_bootstrap_measurement(&exact, &exact).unwrap();

    let incomplete = views_with(&[2322], &[2322]);
    let err = verify_bootstrap_measurement(&incomplete, &exact)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 omitted, 0 stale"), "{err}");
    assert!(err.contains("code 2345"), "{err}");

    let err = verify_bootstrap_measurement(&exact, &incomplete)
        .unwrap_err()
        .to_string();
    assert!(err.contains("0 omitted, 1 stale"), "{err}");
    assert!(err.contains("code 2345"), "{err}");
}

#[test]
fn bootstrap_artifacts_cannot_record_transitions() {
    let matches = matches_artifact(
        views_with(&[2322], &[2322]),
        true,
        None,
        Some(UNIVERSE_TRANSITION.to_owned()),
    );
    let err = matches.validate().unwrap_err().to_string();
    assert!(
        err.contains("bootstrap version cannot record a transition"),
        "{err}"
    );

    let mut inputs = inputs_stub();
    inputs.transition = Some(UNIVERSE_TRANSITION.to_owned());
    let err = inputs.validate().unwrap_err().to_string();
    assert!(
        err.contains("bootstrap version cannot record a transition"),
        "{err}"
    );
}

#[test]
fn artifact_roundtrip_is_lossless() {
    let artifact = matches_artifact(views_with(&[2322, 2345], &[2322]), true, None, None);
    let bytes = encode_artifact(&artifact).unwrap();
    let decoded: MatchesArtifact = decode_artifact(&bytes, "test").unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.views, artifact.views);
    assert_eq!(decoded.inputs, artifact.inputs);
    assert!(decoded.bootstrap);
}

// -- A1 inputs ---------------------------------------------------------

#[test]
fn inactive_tier_requires_absent_marker() {
    let mut inputs = inputs_stub();
    inputs.comparators.remove("t2");
    let err = inputs.validate().unwrap_err().to_string();
    assert!(err.contains("lacks comparator entry t2"), "{err}");

    let mut inputs = inputs_stub();
    inputs
        .comparators
        .insert("t3".to_owned(), ComparatorEntry::Marker("off".to_owned()));
    let err = inputs.validate().unwrap_err().to_string();
    assert!(err.contains("t3"), "{err}");
}

#[test]
fn inputs_diff_names_edited_oracle_records() {
    let mut stored = inputs_stub();
    stored.producer = Some(producer_stub());
    let mut built = stored.clone();
    built
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "edited".to_owned();
    let err = diff_oracle_inputs(&stored, &built).unwrap_err().to_string();
    assert!(err.contains("oracle records edited"), "{err}");
    assert!(err.contains("conformance/a.ts"), "{err}");
}

#[test]
fn inputs_diff_names_deleted_fixture_and_undeclared_growth() {
    let mut stored = inputs_stub();
    stored.producer = Some(producer_stub());
    let mut built = stored.clone();
    built.fixtures.clear();
    let err = diff_oracle_inputs(&stored, &built).unwrap_err().to_string();
    assert!(err.contains("missing from the corpus/goldens"), "{err}");

    let mut built = stored.clone();
    built.fixtures.insert(
        "conformance/new.ts".to_owned(),
        stored.fixtures["conformance/a.ts"].clone(),
    );
    let err = diff_oracle_inputs(&stored, &built).unwrap_err().to_string();
    assert!(err.contains("unpinned fixture"), "{err}");
    assert!(err.contains("universe-transition"), "{err}");
}

#[test]
fn inputs_diff_names_vendor_drift() {
    let stored = inputs_stub();
    let mut built = stored.clone();
    built.vendor.tsc_js_sha256 = "other".to_owned();
    let err = diff_oracle_inputs(&stored, &built).unwrap_err().to_string();
    assert!(err.contains("_tsc.js pin drift"), "{err}");
}

#[test]
fn universe_transition_adds_only() {
    let older = inputs_stub();
    let mut case_grown = older.clone();
    case_grown
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .insert(
            "new-matrix".to_owned(),
            CasePins {
                oracle_sha256: "new-oracle".to_owned(),
                program_sha256: "new-program".to_owned(),
                oracle_t4_sha256: None,
            },
        );
    verify_universe_growth(&older, &case_grown).unwrap();

    let mut grown = older.clone();
    grown.fixtures.insert(
        "conformance/new.ts".to_owned(),
        older.fixtures["conformance/a.ts"].clone(),
    );
    *grown.totals.get_mut("all").unwrap() += 1;
    verify_universe_growth(&older, &grown).unwrap();

    let mut edited = grown.clone();
    edited
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .fixture_sha256 = "edited".to_owned();
    let err = verify_universe_growth(&older, &edited)
        .unwrap_err()
        .to_string();
    assert!(err.contains("changed pinned fixture"), "{err}");

    let mut edited_case = case_grown.clone();
    edited_case
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "edited".to_owned();
    let err = verify_universe_growth(&older, &edited_case)
        .unwrap_err()
        .to_string();
    assert!(err.contains("changed pinned matrix case"), "{err}");

    let mut removed = older.clone();
    removed.fixtures.clear();
    let err = verify_universe_growth(&older, &removed)
        .unwrap_err()
        .to_string();
    assert!(err.contains("removed pinned fixture"), "{err}");

    let mut vendor_changed = grown.clone();
    vendor_changed.vendor.lib_sha256 = "other".to_owned();
    let err = verify_universe_growth(&older, &vendor_changed)
        .unwrap_err()
        .to_string();
    assert!(err.contains("vendor"), "{err}");
}

// -- producer pins -------------------------------------------------------

#[test]
fn universe_transition_cannot_change_producer_pins() {
    let mut older = inputs_stub();
    older.producer = Some(producer_stub());
    let mut node_changed = older.clone();
    node_changed.producer.as_mut().unwrap().node_version = "26.0.0".to_owned();
    let err = verify_universe_growth(&older, &node_changed)
        .unwrap_err()
        .to_string();
    assert!(err.contains("producer"), "{err}");

    let mut dropped = older.clone();
    dropped.producer = None;
    let err = verify_universe_growth(&older, &dropped)
        .unwrap_err()
        .to_string();
    assert!(err.contains("producer"), "{err}");
}

#[test]
fn producer_pin_extension_adds_pins_and_nothing_else() {
    let older = inputs_stub();
    let mut extended = older.clone();
    extended.producer = Some(producer_stub());
    verify_producer_pin_extension(&older, &extended).unwrap();

    // Riding an oracle edit on the extension fails.
    let mut edited = extended.clone();
    edited
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "edited".to_owned();
    let err = verify_producer_pin_extension(&older, &edited)
        .unwrap_err()
        .to_string();
    assert!(err.contains("only add producer pins"), "{err}");

    // The extension is one-time: a pinned predecessor rejects it.
    let err = verify_producer_pin_extension(&extended, &extended)
        .unwrap_err()
        .to_string();
    assert!(err.contains("one-time"), "{err}");

    // And it must actually add the pins.
    let err = verify_producer_pin_extension(&older, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("must add"), "{err}");
}

// -- T1-T3 input-schema activation ---------------------------------------

#[test]
fn comparator_state_accepts_only_atomic_tier_activation_states() {
    assert_eq!(
        comparator_state(&inactive_comparators()).unwrap(),
        TierComparatorState::Inactive
    );
    assert_eq!(
        comparator_state(&tier_1_3_comparators()).unwrap(),
        TierComparatorState::T1ThroughT3
    );
    assert_eq!(
        comparator_state(&tier_1_4_comparators()).unwrap(),
        TierComparatorState::T1ThroughT4
    );

    let mut partial = inactive_comparators();
    partial.insert("t1".to_owned(), active_comparator(T1_T3_COMPARATOR_SCHEMA));
    let err = comparator_state(&partial).unwrap_err().to_string();
    assert!(err.contains("all active"), "{err}");

    let mut wrong_schema = tier_1_3_comparators();
    wrong_schema.insert("t2".to_owned(), active_comparator(99));
    let err = comparator_state(&wrong_schema).unwrap_err().to_string();
    assert!(err.contains("all active"), "{err}");

    let mut t4_early = inactive_comparators();
    t4_early.insert("t4".to_owned(), active_comparator(1));
    let err = comparator_state(&t4_early).unwrap_err().to_string();
    assert!(err.contains("before the T1-T3"), "{err}");
}

#[test]
fn tier_1_3_input_schema_extension_changes_only_three_comparators_once() {
    let older = inputs_stub();
    let mut activated = older.clone();
    activated.comparators = tier_1_3_comparators();
    verify_tier_1_3_input_schema_extension(&older, &activated).unwrap();

    let err = verify_tier_1_3_input_schema_extension(&activated, &activated)
        .unwrap_err()
        .to_string();
    assert!(err.contains("one-time"), "{err}");

    let err = verify_tier_1_3_input_schema_extension(&activated, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("inactive predecessor"), "{err}");

    let mut riding_oracle_edit = activated.clone();
    riding_oracle_edit
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "edited".to_owned();
    let err = verify_tier_1_3_input_schema_extension(&older, &riding_oracle_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("may only activate"), "{err}");

    let mut partial = older.clone();
    partial
        .comparators
        .insert("t1".to_owned(), active_comparator(T1_T3_COMPARATOR_SCHEMA));
    let err = verify_tier_1_3_input_schema_extension(&older, &partial)
        .unwrap_err()
        .to_string();
    assert!(err.contains("all active"), "{err}");
}

#[test]
fn trusted_baseline_accepts_activation_but_rejects_downgrade() {
    let older = inputs_stub();
    let mut activated = older.clone();
    activated.comparators = tier_1_3_comparators();
    verify_baseline_inputs(&older, &activated, false).unwrap();

    let err = verify_baseline_inputs(&activated, &older, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot move"), "{err}");
}

#[test]
fn t4_input_schema_extension_adds_only_renderer_and_case_pins_once() {
    let mut older = inputs_stub();
    older.producer = Some(producer_stub());
    older.comparators = tier_1_3_comparators();

    let mut activated = older.clone();
    activated.comparators = tier_1_4_comparators();
    activated.producer.as_mut().unwrap().render_driver_sha256 = Some("a".repeat(64));
    activated
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_t4_sha256 = Some("b".repeat(64));
    verify_t4_input_schema_extension(&older, &activated).unwrap();
    verify_baseline_inputs(&older, &activated, false).unwrap();

    let err = verify_t4_input_schema_extension(&activated, &activated)
        .unwrap_err()
        .to_string();
    assert!(err.contains("T1-T3 predecessor"), "{err}");

    let mut riding_program_edit = activated.clone();
    riding_program_edit
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .program_sha256 = "edited".to_owned();
    let err = verify_t4_input_schema_extension(&older, &riding_program_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("existing oracle/program"), "{err}");

    let mut riding_old_producer_edit = activated.clone();
    riding_old_producer_edit
        .producer
        .as_mut()
        .unwrap()
        .driver_sha256 = "edited".to_owned();
    let err = verify_t4_input_schema_extension(&older, &riding_old_producer_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("pre-existing producer"), "{err}");

    let mut riding_oracle_edit = activated.clone();
    riding_oracle_edit
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "edited".to_owned();
    let err = verify_t4_input_schema_extension(&older, &riding_oracle_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("existing oracle/program"), "{err}");

    let mut riding_vendor_edit = activated.clone();
    riding_vendor_edit.vendor.lib_sha256 = "edited".to_owned();
    let err = verify_t4_input_schema_extension(&older, &riding_vendor_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("vendor pins"), "{err}");

    let err = verify_baseline_inputs(&activated, &older, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("render-driver pin changed"), "{err}");
}

#[test]
fn active_t4_manifest_requires_nonempty_valid_complete_pins() {
    active_t4_inputs().validate().unwrap();

    let mut missing_renderer = active_t4_inputs();
    missing_renderer
        .producer
        .as_mut()
        .unwrap()
        .render_driver_sha256 = None;
    let err = missing_renderer.validate().unwrap_err().to_string();
    assert!(err.contains("render driver"), "{err}");

    let mut empty_renderer = active_t4_inputs();
    empty_renderer
        .producer
        .as_mut()
        .unwrap()
        .render_driver_sha256 = Some(String::new());
    let err = empty_renderer.validate().unwrap_err().to_string();
    assert!(err.contains("empty"), "{err}");

    let mut missing_case = active_t4_inputs();
    missing_case
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_t4_sha256 = None;
    let err = missing_case.validate().unwrap_err().to_string();
    assert!(err.contains("lacks a genuine"), "{err}");

    let mut invalid_case = active_t4_inputs();
    invalid_case
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_t4_sha256 = Some("not-a-hash".to_owned());
    let err = invalid_case.validate().unwrap_err().to_string();
    assert!(err.contains("lacks a genuine"), "{err}");

    let mut partial = active_t4_inputs();
    partial
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .insert(
            "second".to_owned(),
            CasePins {
                oracle_sha256: "o2".to_owned(),
                program_sha256: "p2".to_owned(),
                oracle_t4_sha256: None,
            },
        );
    let err = partial.validate().unwrap_err().to_string();
    assert!(err.contains("[second]"), "{err}");
}

#[test]
fn trusted_base_accepts_composed_t1_t3_and_t4_extensions() {
    let older = inputs_stub();
    let mut newer = active_t4_inputs();
    // The producer-pin extension may also sit between the trusted
    // base and head; direct compare validates the composition.
    newer.producer.as_mut().unwrap().render_driver_sha256 = Some("c".repeat(64));
    verify_baseline_inputs(&older, &newer, false).unwrap();
}

#[test]
fn oracle_correction_cannot_change_active_renderer_semantics() {
    let older = active_t4_inputs();
    let mut newer = older.clone();
    newer.producer.as_mut().unwrap().render_driver_sha256 = Some("c".repeat(64));
    let err = verify_producer_correction(&older, &newer)
        .unwrap_err()
        .to_string();
    assert!(err.contains("render-driver pin"), "{err}");
    let err = verify_baseline_inputs(&older, &newer, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("render-driver pin"), "{err}");
}

#[test]
fn t4_transition_and_gate_protect_complete_case_identity_including_empty_buckets() {
    let older_views = views_with(&[], &[]);
    let mut newer_views = older_views.clone();
    newer_views
        .get_mut("all")
        .unwrap()
        .get_mut("conformance/a.ts")
        .unwrap()
        .get_mut("")
        .unwrap()
        .t4 = true;
    let older = matches_artifact(older_views.clone(), false, None, None);
    let mut newer = matches_artifact(
        newer_views.clone(),
        false,
        None,
        Some(T4_INPUT_SCHEMA_EXTENSION.to_owned()),
    );
    newer.inputs.oracle_inputs_sha256 = "activated-inputs".to_owned();
    <MatchesArtifact as LineageArtifact>::verify_edge(&newer, &older).unwrap();

    let executed = BTreeSet::from(["conformance/a.ts".to_owned()]);
    let err = enforce_accepted(&newer, &older_views, DiagnosticBand::All, &executed, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("T4 (all)"), "{err}");

    let mut illegal = newer.clone();
    illegal
        .views
        .get_mut("all")
        .unwrap()
        .get_mut("conformance/a.ts")
        .unwrap()
        .get_mut("")
        .unwrap()
        .matched
        .insert(key(9999));
    let err = <MatchesArtifact as LineageArtifact>::verify_edge(&illegal, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("only T4 case identities"), "{err}");

    let mut outside_all = newer.clone();
    outside_all.bootstrap = true;
    outside_all.previous = None;
    outside_all.transition = None;
    outside_all
        .views
        .get_mut("2xxx")
        .unwrap()
        .entry("conformance/a.ts".to_owned())
        .or_default()
        .entry(String::new())
        .or_default()
        .t4 = true;
    let err = outside_all.validate().unwrap_err().to_string();
    assert!(err.contains("only in the All view"), "{err}");
}

#[test]
fn artifact_pair_requires_comparators_before_tier_membership() {
    let inactive = inputs_stub();
    let matches = matches_artifact(
        views_with_tiers(&[100], &[100], &[100], &[100], &[100]),
        true,
        None,
        None,
    );
    let input_bytes = encode_artifact(&inactive).unwrap();
    let mut matches = matches;
    matches.inputs.oracle_inputs_sha256 = sha256_hex(&input_bytes);
    let err = verify_pair_values("test", &matches, &inactive, &input_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("comparators are explicitly absent"), "{err}");

    let mut active = inactive;
    active.comparators = tier_1_3_comparators();
    let active_bytes = encode_artifact(&active).unwrap();
    matches.inputs.oracle_inputs_sha256 = sha256_hex(&active_bytes);
    verify_pair_values("test", &matches, &active, &active_bytes).unwrap();

    matches
        .views
        .get_mut("all")
        .unwrap()
        .get_mut("conformance/a.ts")
        .unwrap()
        .get_mut("")
        .unwrap()
        .t4 = true;
    let err = verify_pair_values("test", &matches, &active, &active_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("T4 case identities"), "{err}");

    let active_t4 = active_t4_inputs();
    let active_t4_bytes = encode_artifact(&active_t4).unwrap();
    matches.inputs.oracle_inputs_sha256 = sha256_hex(&active_t4_bytes);
    verify_pair_values("test", &matches, &active_t4, &active_t4_bytes).unwrap();
}

#[test]
fn matches_activation_edge_adds_only_tier_sets() {
    let older = matches_artifact(views_with(&[100], &[100]), true, None, None);
    let mut activated = matches_artifact(
        views_with_tiers(&[100], &[100], &[100], &[100], &[100]),
        false,
        Some(lineage_to("c0", b"prev")),
        Some(TIER_1_3_INPUT_SCHEMA_EXTENSION.to_owned()),
    );
    activated.inputs.oracle_inputs_sha256 = "active-inputs".to_owned();
    MatchesArtifact::verify_edge(&activated, &older).unwrap();

    let mut riding_t0_gain = activated.clone();
    riding_t0_gain.views = views_with_tiers(&[100, 101], &[100, 101], &[100], &[100], &[100]);
    let err = MatchesArtifact::verify_edge(&riding_t0_gain, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("may add only T1-T3"), "{err}");

    let mut activated_again = activated.clone();
    activated_again.inputs.oracle_inputs_sha256 = "active-inputs-again".to_owned();
    let err = MatchesArtifact::verify_edge(&activated_again, &activated)
        .unwrap_err()
        .to_string();
    assert!(err.contains("predecessor already contains"), "{err}");
}

#[test]
fn baseline_inputs_accept_producer_extension_but_not_change() {
    let older = inputs_stub();
    let mut extended = older.clone();
    extended.producer = Some(producer_stub());
    // base predates the extension -> head may add the pins.
    verify_baseline_inputs(&older, &extended, false).unwrap();

    // A pinned base cannot see different pins at head.
    let mut changed = extended.clone();
    changed.producer.as_mut().unwrap().driver_sha256 = "other".to_owned();
    let err = verify_baseline_inputs(&extended, &changed, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("producer pins changed"), "{err}");
}

#[test]
fn matches_edge_accepts_producer_pin_extension() {
    let older = matches_artifact(views_with(&[100], &[]), true, None, None);
    let mut newer = matches_artifact(
        views_with(&[100], &[]),
        false,
        Some(lineage_to("c0", b"prev")),
        Some(PRODUCER_PIN_EXTENSION.to_owned()),
    );
    newer.inputs.oracle_inputs_sha256 = "extended-inputs".to_owned();
    MatchesArtifact::verify_edge(&newer, &older).unwrap();

    // The extension still cannot shrink the accepted sets.
    let mut shrunk = newer.clone();
    shrunk.views = views_with(&[], &[]);
    let err = MatchesArtifact::verify_edge(&shrunk, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("regressed"), "{err}");
}

#[test]
fn node_version_normalization_strips_the_v_prefix() {
    assert_eq!(normalize_node_version("v25.2.1\n"), "25.2.1");
    assert_eq!(normalize_node_version("25.2.1"), "25.2.1");
    assert_eq!(normalize_node_version("  v25.2.1  "), "25.2.1");
}

// -- oracle correction ---------------------------------------------------

fn correction_artifact(views: RunSets, lapsed: RunSets) -> MatchesArtifact {
    let mut artifact = matches_artifact(
        views,
        false,
        Some(lineage_to("c0", b"prev")),
        Some(ORACLE_CORRECTION.to_owned()),
    );
    artifact.inputs.oracle_inputs_sha256 = "corrected-inputs".to_owned();
    artifact.lapsed = Some(lapsed);
    artifact
}

#[test]
fn lapsed_field_pairs_exactly_with_the_correction_transition() {
    // lapsed without the transition is invalid.
    let mut stray = matches_artifact(views_with(&[100], &[]), true, None, None);
    stray.lapsed = Some(views_with(&[], &[]));
    let err = stray.validate().unwrap_err().to_string();
    assert!(err.contains("without an"), "{err}");

    // The transition without lapsed is invalid.
    let mut missing = matches_artifact(
        views_with(&[100], &[]),
        false,
        Some(lineage_to("c0", b"prev")),
        Some(ORACLE_CORRECTION.to_owned()),
    );
    missing.lapsed = None;
    let err = missing.validate().unwrap_err().to_string();
    assert!(err.contains("lacks its lapsed enumeration"), "{err}");

    // A lapsed identity still present in the accepted sets is
    // incoherent — for either protected tier.
    let mut incoherent = correction_artifact(views_with(&[100], &[]), views_with(&[100], &[]));
    let err = incoherent.validate().unwrap_err().to_string();
    assert!(err.contains("still accepted"), "{err}");
    incoherent.lapsed = Some(views_with(&[], &[]));
    incoherent.validate().unwrap();

    let complete_overlap = correction_artifact(views_with(&[100], &[100]), views_with(&[], &[100]));
    let err = complete_overlap.validate().unwrap_err().to_string();
    assert!(err.contains("still accepted"), "{err}");
    assert!(err.contains("multiplicity-complete"), "{err}");
}

#[test]
fn correction_edge_requires_exact_lapse_enumeration() {
    let older = matches_artifact(views_with(&[100, 101], &[100]), true, None, None);

    // Exactly enumerated: 101 lapses from matched, 100 from the
    // multiplicity-complete tier while its matched key stays.
    let corrected = correction_artifact(views_with(&[100, 102], &[]), views_with(&[101], &[100]));
    MatchesArtifact::verify_edge(&corrected, &older).unwrap();

    // An unenumerated removal names the identity.
    let unenumerated = correction_artifact(views_with(&[100, 102], &[]), views_with(&[], &[100]));
    let err = MatchesArtifact::verify_edge(&unenumerated, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("missing from the lapsed enumeration"), "{err}");
    assert!(err.contains("code 101"), "{err}");

    // Over-enumeration (claiming a lapse that did not happen) is
    // rejected too — lapsed is exact, not an allowance pool.
    let over = correction_artifact(
        views_with(&[100, 101, 102], &[100]),
        views_with(&[101], &[]),
    );
    let err = MatchesArtifact::verify_edge(&over, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("did not lapse"), "{err}");

    // A correction must ride a corrected manifest: same input
    // pins as the predecessor is refused.
    let mut same_inputs =
        correction_artifact(views_with(&[100, 102], &[]), views_with(&[101], &[100]));
    same_inputs.inputs = older.inputs.clone();
    let err = MatchesArtifact::verify_edge(&same_inputs, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("must ride a corrected"), "{err}");
}

#[test]
fn correction_enumerates_tier_lapses_independently() {
    let older = matches_artifact(
        views_with_tiers(&[100], &[100], &[100], &[100], &[100]),
        true,
        None,
        None,
    );
    let corrected = correction_artifact(
        views_with_tiers(&[100], &[100], &[100], &[100], &[]),
        views_with_tiers(&[], &[], &[], &[], &[100]),
    );
    MatchesArtifact::verify_edge(&corrected, &older).unwrap();

    let missing = correction_artifact(
        views_with_tiers(&[100], &[100], &[100], &[100], &[]),
        views_with(&[], &[]),
    );
    let err = MatchesArtifact::verify_edge(&missing, &older)
        .unwrap_err()
        .to_string();
    assert!(err.contains("T3"), "{err}");
    assert!(err.contains("code 100"), "{err}");
}

#[test]
fn correction_inputs_change_records_only() {
    let mut older = inputs_stub();
    older.producer = Some(producer_stub());

    // Only oracle record pins (and totals) move: accepted.
    let mut corrected = older.clone();
    corrected
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "corrected".to_owned();
    *corrected.totals.get_mut("all").unwrap() = 0;
    verify_producer_correction(&older, &corrected).unwrap();

    // The producer itself may change under a correction (that is
    // usually the point), but must stay pinned.
    let mut producer_fixed = corrected.clone();
    producer_fixed.producer.as_mut().unwrap().driver_sha256 = "fixed-driver".to_owned();
    verify_producer_correction(&older, &producer_fixed).unwrap();
    let mut unpinned = corrected.clone();
    unpinned.producer = None;
    let err = verify_producer_correction(&older, &unpinned)
        .unwrap_err()
        .to_string();
    assert!(err.contains("requires producer pins"), "{err}");

    // Everything else is immutable under a correction.
    let mut fixture_edit = corrected.clone();
    fixture_edit
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .fixture_sha256 = "edited".to_owned();
    let err = verify_producer_correction(&older, &fixture_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("fixture bytes"), "{err}");

    let mut expansion_edit = corrected.clone();
    expansion_edit
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .program_sha256 = "edited".to_owned();
    let err = verify_producer_correction(&older, &expansion_edit)
        .unwrap_err()
        .to_string();
    assert!(err.contains("matrix expansion"), "{err}");

    let mut grown = corrected.clone();
    grown.fixtures.insert(
        "conformance/new.ts".to_owned(),
        older.fixtures["conformance/a.ts"].clone(),
    );
    let err = verify_producer_correction(&older, &grown)
        .unwrap_err()
        .to_string();
    assert!(err.contains("universe transition"), "{err}");

    let mut vendor_changed = corrected.clone();
    vendor_changed.vendor.tsc_js_sha256 = "other".to_owned();
    let err = verify_producer_correction(&older, &vendor_changed)
        .unwrap_err()
        .to_string();
    assert!(err.contains("vendor"), "{err}");

    // And the universe transition still refuses oracle edits —
    // the correction is not a loophole in the growth rule.
    let err = verify_universe_growth(&older, &corrected)
        .unwrap_err()
        .to_string();
    assert!(err.contains("changed pinned matrix case"), "{err}");
}

#[test]
fn baseline_compare_across_a_correction_accepts_enumerated_lapses_only() {
    let repo = init_repo("baseline-correction");
    let base_matches = matches_artifact(views_with(&[2322, 2345], &[2322]), true, None, None);
    let base_inputs = {
        let mut inputs = inputs_stub();
        inputs.producer = Some(producer_stub());
        inputs
    };
    commit_artifact_pair(
        &repo,
        &encode_artifact(&base_matches).unwrap(),
        &encode_artifact(&base_inputs).unwrap(),
        "base pair",
    );
    git_test(&repo, &["branch", "-q", "base"]);

    let mut head_inputs = base_inputs.clone();
    head_inputs
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "corrected".to_owned();

    // The corrected head lapses 2345 (enumerated) and gains 2454.
    let head_matches =
        correction_artifact(views_with(&[2322, 2454], &[2322]), views_with(&[2345], &[]));
    verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_matches,
        &head_inputs,
    )
    .unwrap();

    // A removal beyond the enumeration still fails, naming it.
    let head_extra_removal =
        correction_artifact(views_with(&[2454], &[]), views_with(&[2345], &[2322]));
    let err = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_extra_removal,
        &head_inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("beyond the enumerated"), "{err}");
    assert!(err.contains("code 2322"), "{err}");

    // Without any correction between base and head, changed oracle
    // pins keep failing the strict growth compare.
    let plain_head = matches_artifact(views_with(&[2322, 2345], &[2322]), true, None, None);
    let err = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &plain_head,
        &head_inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("changed pinned matrix case"), "{err}");

    // Expansion stays immutable even across a correction.
    let mut expansion_changed = head_inputs.clone();
    expansion_changed
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .program_sha256 = "edited".to_owned();
    let err = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_matches,
        &expansion_changed,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("matrix expansion"), "{err}");

    // A COMMITTED correction on the branch sanctions the lapse for
    // every later plain version too.
    commit_artifact_pair(
        &repo,
        &encode_artifact(&head_matches).unwrap(),
        &encode_artifact(&head_inputs).unwrap(),
        "correction pair",
    );
    let later = matches_artifact(views_with(&[2322, 2454, 2564], &[2322]), true, None, None);
    verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &later,
        &head_inputs,
    )
    .unwrap();
}

#[test]
fn file_transaction_rolls_inputs_back_when_matches_replace_fails() {
    let dir = temp_dir("pair-rollback");
    let inputs_path = dir.join("inputs.zst");
    let matches_path = dir.join("matches.zst");
    fs::write(&inputs_path, b"old inputs").unwrap();
    // An existing directory cannot be replaced by the temporary
    // matches file, forcing the second half of the pair to fail.
    fs::create_dir(&matches_path).unwrap();

    let updates = [
        AtomicFileUpdate {
            path: &inputs_path,
            original: Some(b"old inputs"),
            replacement: b"new inputs",
        },
        AtomicFileUpdate {
            path: &matches_path,
            original: None,
            replacement: b"new matches",
        },
    ];
    let err = write_file_updates(&updates).unwrap_err().to_string();
    assert!(err.contains("failed to replace"), "{err}");
    assert_eq!(fs::read(&inputs_path).unwrap(), b"old inputs");
}

#[test]
fn file_transaction_rolls_artifacts_back_when_ratchet_replace_fails() {
    let dir = temp_dir("ratchet-rollback");
    let inputs_path = dir.join("inputs.zst");
    let matches_path = dir.join("matches.zst");
    let ratchet_path = dir.join("ratchet.toml");
    fs::write(&inputs_path, b"old inputs").unwrap();
    // A directory at the final target forces the third write to
    // fail after both artifacts have already been replaced.
    fs::create_dir(&ratchet_path).unwrap();

    let updates = [
        AtomicFileUpdate {
            path: &inputs_path,
            original: Some(b"old inputs"),
            replacement: b"new inputs",
        },
        AtomicFileUpdate {
            path: &matches_path,
            original: None,
            replacement: b"new matches",
        },
        AtomicFileUpdate {
            path: &ratchet_path,
            original: None,
            replacement: b"new summary",
        },
    ];
    let err = write_file_updates(&updates).unwrap_err().to_string();
    assert!(err.contains("failed to replace"), "{err}");
    assert_eq!(fs::read(&inputs_path).unwrap(), b"old inputs");
    assert!(!matches_path.exists());
    assert!(ratchet_path.is_dir());
}

#[test]
fn optional_artifact_read_ignores_only_not_found() {
    let dir = temp_dir("optional-read");
    let missing = dir.join("missing.zst");
    assert!(read_optional_bytes(&missing, "test artifact")
        .unwrap()
        .is_none());

    let unreadable = dir.join("artifact-as-directory");
    fs::create_dir(&unreadable).unwrap();
    let err = read_optional_bytes(&unreadable, "test artifact")
        .unwrap_err()
        .to_string();
    assert!(err.contains("failed to read test artifact"), "{err}");
}

// -- A1 lineage --------------------------------------------------------

#[test]
fn workspace_history_paths_normalize_pre_and_post_move_names() {
    let current = WorkspaceHistoryPaths::new(MATCHES_REL_PATH).unwrap();
    let legacy = WorkspaceHistoryPaths::new(&legacy_rel(MATCHES_REL_PATH)).unwrap();
    assert_eq!(current, legacy);
    assert_eq!(current.current, MATCHES_REL_PATH);
    assert_eq!(current.legacy, legacy_rel(MATCHES_REL_PATH));
}

#[test]
fn workspace_history_paths_reject_escaping_or_ambiguous_names() {
    for rel in ["", ".", "../ratchets/x", "/ratchets/x", "tsrs2/"] {
        assert!(
            WorkspaceHistoryPaths::new(rel).is_err(),
            "{rel:?} must not become a Git pathspec"
        );
    }
}

#[test]
fn git_blob_optional_reads_legacy_golden_from_either_spelling() {
    let repo = init_repo("history-legacy-golden");
    let current = "goldens/conformance/a.json.zst";
    let legacy = legacy_rel(current);
    let commit = commit_bytes(&repo, &legacy, b"legacy golden", "legacy golden");

    assert_eq!(
        git_blob_optional(&repo, &commit, current).unwrap(),
        Some(b"legacy golden".to_vec())
    );
    assert_eq!(
        git_blob_optional(&repo, &commit, &legacy).unwrap(),
        Some(b"legacy golden".to_vec())
    );
}

#[test]
fn git_blob_optional_reads_root_and_preserves_absence_and_git_errors() {
    let repo = init_repo("history-root-blob");
    let current = "goldens/conformance/a.json.zst";
    let legacy = legacy_rel(current);
    let commit = commit_bytes(&repo, current, b"root golden", "root golden");

    assert_eq!(
        git_blob_optional(&repo, &commit, current).unwrap(),
        Some(b"root golden".to_vec())
    );
    assert_eq!(
        git_blob_optional(&repo, &commit, &legacy).unwrap(),
        Some(b"root golden".to_vec())
    );
    assert_eq!(
        git_blob_optional(&repo, &commit, "goldens/conformance/missing.json.zst").unwrap(),
        None
    );
    assert!(
        git_blob_optional(&repo, "not-a-commit", current).is_err(),
        "an invalid commit must not be treated as an absent path"
    );
}

#[test]
fn workspace_bridge_rejects_dual_location_even_when_bytes_match() {
    let repo = init_repo("history-dual-location");
    let current = "goldens/conformance/a.json.zst";
    let legacy = legacy_rel(current);
    commit_bytes(&repo, current, b"same", "root golden");
    let commit = commit_bytes(&repo, &legacy, b"same", "duplicate legacy golden");

    let err = git_blob_optional(&repo, &commit, current)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ambiguous"), "{err}");
    assert!(err.contains(current), "{err}");
    assert!(err.contains(&legacy), "{err}");
}

#[test]
fn lineage_pair_and_baseline_survive_atomic_workspace_promotion() {
    let repo = init_repo("history-workspace-promotion");
    let legacy_matches = legacy_rel(MATCHES_REL_PATH);
    let legacy_inputs = legacy_rel(ORACLE_INPUTS_REL_PATH);

    let v1_inputs = inputs_stub();
    let v1_inputs_bytes = encode_artifact(&v1_inputs).unwrap();
    let mut v1_matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    v1_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&v1_inputs_bytes),
        tsc_js_sha256: v1_inputs.vendor.tsc_js_sha256.clone(),
    };
    let v1_matches_bytes = encode_artifact(&v1_matches).unwrap();
    let c1 = commit_artifact_pair_at(
        &repo,
        &legacy_matches,
        &legacy_inputs,
        &v1_matches_bytes,
        &v1_inputs_bytes,
        "legacy bootstrap pair",
    );

    let mut v2_matches = matches_artifact(
        views_with(&[2322, 2345], &[2322]),
        false,
        Some(lineage_to(&c1, &v1_matches_bytes)),
        None,
    );
    v2_matches.inputs = v1_matches.inputs.clone();
    let v2_matches_bytes = encode_artifact(&v2_matches).unwrap();
    commit_bytes(
        &repo,
        &legacy_matches,
        &v2_matches_bytes,
        "legacy matches growth",
    );
    git_test(&repo, &["branch", "-q", "base"]);

    fs::rename(repo.join("tsrs2/ratchets"), repo.join("ratchets")).unwrap();
    commit_all(&repo, "promote workspace to repository root");

    assert_eq!(
        verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v2_matches_bytes).unwrap(),
        2,
        "an unchanged path move must not become a material artifact version"
    );
    assert_eq!(
        verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &v1_inputs_bytes,)
            .unwrap(),
        1,
    );
    verify_committed_artifact_pairs(&repo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH).unwrap();
    assert!(!verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &v2_matches,
        &v1_inputs,
    )
    .unwrap());
}

#[test]
fn workspace_bridge_rejects_split_pair_location() {
    let repo = init_repo("history-split-workspace-pair");
    let legacy_matches = legacy_rel(MATCHES_REL_PATH);
    let legacy_inputs = legacy_rel(ORACLE_INPUTS_REL_PATH);
    let inputs = inputs_stub();
    let inputs_bytes = encode_artifact(&inputs).unwrap();
    let mut matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&inputs_bytes),
        tsc_js_sha256: inputs.vendor.tsc_js_sha256.clone(),
    };
    let matches_bytes = encode_artifact(&matches).unwrap();
    commit_artifact_pair_at(
        &repo,
        &legacy_matches,
        &legacy_inputs,
        &matches_bytes,
        &inputs_bytes,
        "legacy pair",
    );

    fs::create_dir_all(repo.join("ratchets")).unwrap();
    fs::rename(repo.join(&legacy_matches), repo.join(MATCHES_REL_PATH)).unwrap();
    commit_all(&repo, "move only matches artifact");

    let err = verify_committed_artifact_pairs(&repo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH)
        .unwrap_err()
        .to_string();
    assert!(err.contains("straddles"), "{err}");
}

#[test]
fn workspace_bridge_does_not_hide_a_deletion() {
    let repo = init_repo("history-workspace-deletion");
    let legacy = legacy_rel(MATCHES_REL_PATH);
    commit_bytes(&repo, &legacy, b"pinned", "legacy artifact");
    fs::remove_file(repo.join(&legacy)).unwrap();
    commit_all(&repo, "delete legacy artifact");

    let mut memo = GitMemo::new(&repo).unwrap();
    let err = memo
        .committed_versions(MATCHES_REL_PATH)
        .unwrap_err()
        .to_string();
    assert!(err.contains("was deleted"), "{err}");
}

#[test]
fn workspace_move_with_changed_bytes_is_a_real_version() {
    let repo = init_repo("history-workspace-move-change");
    let legacy = legacy_rel(MATCHES_REL_PATH);
    let v1 = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    commit_bytes(&repo, &legacy, &v1_bytes, "legacy bootstrap");

    let v2 = matches_artifact(views_with(&[2322, 2345], &[2322]), true, None, None);
    let v2_bytes = encode_artifact(&v2).unwrap();
    fs::create_dir_all(repo.join("ratchets")).unwrap();
    fs::rename(repo.join(&legacy), repo.join(MATCHES_REL_PATH)).unwrap();
    fs::write(repo.join(MATCHES_REL_PATH), &v2_bytes).unwrap();
    commit_all(&repo, "move and mutate artifact");

    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v2_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("second bootstrap"), "{err}");
}

#[test]
fn git_history_memo_reuses_blob_parent_and_path_queries() {
    let repo = init_repo("history-memo");
    let first_commit = commit_bytes(&repo, MATCHES_REL_PATH, b"first", "first");
    let second_commit = commit_bytes(&repo, MATCHES_REL_PATH, b"second", "second");
    let mut memo = GitMemo::new(&repo).unwrap();

    let first_walk = memo.committed_versions(MATCHES_REL_PATH).unwrap();
    assert_eq!(first_walk.len(), 2);
    let invocations_after_walk = memo.git_invocations;

    assert_eq!(
        memo.blob_optional(&second_commit, MATCHES_REL_PATH)
            .unwrap(),
        Some(b"second".to_vec())
    );
    assert_eq!(
        memo.commit_parents(&second_commit).unwrap(),
        vec![first_commit]
    );
    assert_eq!(
        memo.committed_versions(MATCHES_REL_PATH).unwrap(),
        first_walk
    );
    assert_eq!(
        memo.git_invocations, invocations_after_walk,
        "cached blob, parent, and path-version queries must not spawn Git"
    );
}

#[test]
fn blob_identity_lookup_does_not_read_or_decode_blob_bytes() {
    let repo = init_repo("history-blob-identity");
    commit_bytes(&repo, MATCHES_REL_PATH, b"artifact bytes", "artifact");
    let mut memo = GitMemo::new(&repo).unwrap();

    let blob = memo
        .blob_ref_optional("HEAD", MATCHES_REL_PATH)
        .unwrap()
        .expect("HEAD blob");
    assert!(!blob.object_id.is_empty());
    assert_eq!(
        memo.git_invocations, 1,
        "identity lookup needs only ls-tree"
    );
    assert!(
        memo.blob_objects.is_empty(),
        "identity lookup must not retain blob bytes"
    );

    memo.blob_ref_optional("HEAD", MATCHES_REL_PATH).unwrap();
    assert_eq!(memo.git_invocations, 1, "blob identity must be memoized");
    assert_eq!(
        memo.blob_optional("HEAD", MATCHES_REL_PATH).unwrap(),
        Some(b"artifact bytes".to_vec())
    );
    assert_eq!(memo.git_invocations, 2, "first byte read adds one git show");
    memo.blob_optional("HEAD", MATCHES_REL_PATH).unwrap();
    assert_eq!(memo.git_invocations, 2, "blob bytes must then be memoized");
}

fn proof_for_committed_pair(
    repo: &Path,
    matches_bytes: &[u8],
    inputs_bytes: &[u8],
) -> AcceptedPairHistoryProof {
    let root = git_root_for(repo).unwrap();
    let matches_rel = git_rel_path(&root, repo, MATCHES_REL_PATH).unwrap();
    let inputs_rel = git_rel_path(&root, repo, ORACLE_INPUTS_REL_PATH).unwrap();
    let mut memo = GitMemo::new(&root).unwrap();
    AcceptedPairHistoryProof::from_verified_history(
        repo,
        &mut memo,
        &matches_rel,
        &inputs_rel,
        matches_bytes,
        inputs_bytes,
    )
    .unwrap()
}

#[test]
fn accepted_pair_history_proof_is_bound_to_head_blobs_and_working_bytes() {
    let repo = init_repo("history-proof-binding");
    let matches_bytes = b"matches";
    let inputs_bytes = b"inputs";
    commit_artifact_pair(&repo, matches_bytes, inputs_bytes, "pair");
    let proof = proof_for_committed_pair(&repo, matches_bytes, inputs_bytes);
    proof.verify_current(&repo).unwrap();
    let error = proof
        .verify_current_at_head(&repo, &"0".repeat(40))
        .unwrap_err()
        .to_string();
    assert!(error.contains("dependent audit HEAD"), "{error}");

    fs::write(repo.join(MATCHES_REL_PATH), b"changed matches").unwrap();
    let error = proof.verify_current(&repo).unwrap_err().to_string();
    assert!(error.contains("working artifacts"), "{error}");
    fs::write(repo.join(MATCHES_REL_PATH), matches_bytes).unwrap();

    fs::write(repo.join(ORACLE_INPUTS_REL_PATH), b"changed inputs").unwrap();
    let error = proof.verify_current(&repo).unwrap_err().to_string();
    assert!(error.contains("working artifacts"), "{error}");
}

#[test]
fn accepted_pair_history_proof_rejects_head_moves_and_other_workspaces() {
    let repo = init_repo("history-proof-head");
    let matches_bytes = b"matches";
    let inputs_bytes = b"inputs";
    commit_artifact_pair(&repo, matches_bytes, inputs_bytes, "pair");
    let proof = proof_for_committed_pair(&repo, matches_bytes, inputs_bytes);

    commit_bytes(&repo, "unrelated", b"move HEAD", "unrelated commit");
    let error = proof.verify_current(&repo).unwrap_err().to_string();
    assert!(error.contains("HEAD moved"), "{error}");

    let other = init_repo("history-proof-other-workspace");
    commit_artifact_pair(&other, matches_bytes, inputs_bytes, "same pair");
    let error = proof.verify_current(&other).unwrap_err().to_string();
    assert!(error.contains("belongs to workspace"), "{error}");
}

#[test]
fn accepted_pair_history_proof_preserves_uncommitted_bootstrap_artifacts() {
    let repo = init_repo("history-proof-bootstrap");
    commit_bytes(&repo, "seed", b"seed", "seed");
    let matches_bytes = b"working matches";
    let inputs_bytes = b"working inputs";
    for (rel, bytes) in [
        (MATCHES_REL_PATH, matches_bytes.as_slice()),
        (ORACLE_INPUTS_REL_PATH, inputs_bytes.as_slice()),
    ] {
        let path = repo.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    let proof = proof_for_committed_pair(&repo, matches_bytes, inputs_bytes);
    assert_eq!(proof.head_matches_blob, None);
    assert_eq!(proof.head_inputs_blob, None);
    proof.verify_current(&repo).unwrap();
}

#[test]
fn git_history_memo_reuses_validated_blob_facts_across_lineage_and_pairs() {
    let repo = init_repo("history-decoded-facts");
    let inputs = inputs_stub();
    let inputs_bytes = encode_artifact(&inputs).unwrap();
    let mut first_matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    first_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&inputs_bytes),
        tsc_js_sha256: inputs.vendor.tsc_js_sha256.clone(),
    };
    let first_matches_bytes = encode_artifact(&first_matches).unwrap();
    let first_commit =
        commit_artifact_pair(&repo, &first_matches_bytes, &inputs_bytes, "bootstrap pair");

    let mut second_matches = matches_artifact(
        views_with(&[2322, 2345], &[2322]),
        false,
        Some(lineage_to(&first_commit, &first_matches_bytes)),
        None,
    );
    second_matches.inputs = first_matches.inputs.clone();
    let second_matches_bytes = encode_artifact(&second_matches).unwrap();
    commit_bytes(
        &repo,
        MATCHES_REL_PATH,
        &second_matches_bytes,
        "matches growth",
    );

    let mut memo = GitMemo::new(&repo).unwrap();
    assert_eq!(
        verify_lineage_with_memo::<MatchesArtifact>(
            &mut memo,
            MATCHES_REL_PATH,
            &second_matches_bytes,
        )
        .unwrap(),
        2,
    );
    assert_eq!(
        verify_lineage_with_memo::<OracleInputsArtifact>(
            &mut memo,
            ORACLE_INPUTS_REL_PATH,
            &inputs_bytes,
        )
        .unwrap(),
        1,
    );
    assert_eq!(memo.matches_pair_facts.len(), 2);
    assert_eq!(memo.inputs_pair_facts.len(), 1);
    assert_eq!(memo.lineage_facts.len(), 3);
    assert_eq!(
        memo.lineage_decode_misses, 3,
        "each committed matches/input blob must be decoded once"
    );
    assert_eq!(
        memo.lineage_peak_live_versions, 2,
        "lineage verification must retain only an edge's two endpoints"
    );
    assert_eq!(memo.pair_matches_decode_misses, 0);
    assert_eq!(memo.pair_inputs_decode_misses, 0);

    verify_committed_artifact_pairs_with_memo(&mut memo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH)
        .unwrap();
    assert_eq!(
        memo.pair_matches_decode_misses, 0,
        "lineage-validated matches blobs must not be decoded again"
    );
    assert_eq!(
        memo.pair_inputs_decode_misses, 0,
        "the carried input blob must reuse its lineage-validated facts"
    );

    let mut pair_only_memo = GitMemo::new(&repo).unwrap();
    verify_committed_artifact_pairs_with_memo(
        &mut pair_only_memo,
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
    )
    .unwrap();
    assert_eq!(pair_only_memo.pair_matches_decode_misses, 2);
    assert_eq!(pair_only_memo.pair_inputs_decode_misses, 1);
}

#[test]
fn git_history_memo_pins_head_and_rejects_a_mid_run_move() {
    let repo = init_repo("history-head-pin");
    let pinned = commit_bytes(&repo, MATCHES_REL_PATH, b"pinned", "pinned");
    let mut memo = GitMemo::new(&repo).unwrap();
    let moved = commit_bytes(&repo, MATCHES_REL_PATH, b"moved", "moved");

    assert_eq!(memo.head_commit, pinned);
    assert_eq!(
        memo.blob_optional("HEAD", MATCHES_REL_PATH).unwrap(),
        Some(b"pinned".to_vec())
    );
    let versions = memo.committed_versions(MATCHES_REL_PATH).unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].0, pinned);

    let err = memo.verify_head_unchanged().unwrap_err().to_string();
    assert!(err.contains("HEAD moved"), "{err}");
    assert!(err.contains(&moved), "{err}");
}

#[test]
fn lineage_bootstrap_and_additions_pass_shrink_fails() {
    let repo = init_repo("grow");
    let v1 = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");
    assert_eq!(
        verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v1_bytes).unwrap(),
        1
    );

    // Additions-only working version on top of the committed tip.
    let v2 = matches_artifact(
        views_with(&[2322, 2345], &[2322]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    assert_eq!(
        verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v2_bytes).unwrap(),
        2
    );
    let c2 = commit_bytes(&repo, MATCHES_REL_PATH, &v2_bytes, "v2");

    // A shrinking head: the coordinated artifact+summary edit.
    // The failure must name the removed identity.
    let v3 = matches_artifact(
        views_with(&[2345], &[]),
        false,
        Some(lineage_to(&c2, &v2_bytes)),
        None,
    );
    let v3_bytes = encode_artifact(&v3).unwrap();
    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v3_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("shrank"), "{err}");
    assert!(err.contains("code 2322"), "{err}");
}

#[test]
fn lineage_streams_committed_edges_and_keeps_only_the_tip_for_a_working_version() {
    let repo = init_repo("lineage-streaming-working");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    let c2 = commit_bytes(&repo, MATCHES_REL_PATH, &v2_bytes, "v2");

    let v3 = matches_artifact(
        views_with(&[2322, 2345, 2454], &[]),
        false,
        Some(lineage_to(&c2, &v2_bytes)),
        None,
    );
    let v3_bytes = encode_artifact(&v3).unwrap();
    let mut memo = GitMemo::new(&repo).unwrap();
    assert_eq!(
        verify_lineage_with_memo::<MatchesArtifact>(&mut memo, MATCHES_REL_PATH, &v3_bytes,)
            .unwrap(),
        3
    );
    assert_eq!(memo.lineage_decode_misses, 2);
    assert_eq!(memo.lineage_facts.len(), 2);
    assert_eq!(
        memo.lineage_peak_live_versions, 2,
        "the working-tree edge must overlap only the committed tip and working version"
    );
}

#[test]
fn lineage_shrinking_intermediate_version_fails() {
    // v1 {A,B} -> v2 {A} -> v3 {A,B}: HEAD looks fine against v1,
    // but the intermediate edge shrank and must fail.
    let repo = init_repo("intermediate");
    let v1 = matches_artifact(views_with(&[2322, 2345], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(
        views_with(&[2322], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    let c2 = commit_bytes(&repo, MATCHES_REL_PATH, &v2_bytes, "v2");

    let v3 = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(lineage_to(&c2, &v2_bytes)),
        None,
    );
    let v3_bytes = encode_artifact(&v3).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v3_bytes, "v3");

    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v3_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("shrank"), "{err}");
    assert!(err.contains("code 2345"), "{err}");
}

#[test]
fn lineage_non_immediate_predecessor_fails() {
    let repo = init_repo("skip");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v2_bytes, "v2");

    // v3 points past v2 at v1: a valid ancestor, correct bytes —
    // but not the immediate predecessor.
    let v3 = matches_artifact(
        views_with(&[2322, 2345, 2454], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let v3_bytes = encode_artifact(&v3).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v3_bytes, "v3");

    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v3_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("immediate"), "{err}");
}

#[test]
fn lineage_stale_previous_hash_fails() {
    let repo = init_repo("stale-hash");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(Lineage {
            commit: c1,
            sha256: "0".repeat(64),
        }),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v2_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("stale previous.sha256"), "{err}");
}

#[test]
fn lineage_second_bootstrap_fails() {
    let repo = init_repo("second-bootstrap");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(views_with(&[2322, 2345], &[]), true, None, None);
    let v2_bytes = encode_artifact(&v2).unwrap();
    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v2_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("second bootstrap"), "{err}");
}

#[test]
fn lineage_reused_blob_is_rejected_without_decoding_it_again() {
    let repo = init_repo("reused-bootstrap-blob");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v2_bytes, "v2");

    // Restoring v1's exact Git blob is still a material version after v2.
    // Its cached header must reproduce the ordinary second-bootstrap error
    // without expanding the same accepted sets a second time.
    commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "restore v1 blob");
    let mut memo = GitMemo::new(&repo).unwrap();
    let err = verify_lineage_with_memo::<MatchesArtifact>(&mut memo, MATCHES_REL_PATH, &v1_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("second bootstrap"), "{err}");
    assert_eq!(memo.lineage_decode_misses, 2);
    assert_eq!(memo.lineage_facts.len(), 2);
    assert_eq!(memo.lineage_peak_live_versions, 2);
}

#[test]
fn lineage_unknown_previous_commit_fails() {
    let repo = init_repo("unknown-prev");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    let v2 = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(Lineage {
            commit: "deadbeef".repeat(5),
            sha256: sha256_hex(&v1_bytes),
        }),
        None,
    );
    let v2_bytes = encode_artifact(&v2).unwrap();
    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v2_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown or unreachable"), "{err}");
}

#[test]
fn lineage_oldest_reachable_must_be_bootstrap() {
    // A chain whose oldest reachable version is not the bootstrap
    // is a truncated clone (or a forged root) and must fail.
    let repo = init_repo("no-bootstrap");
    let orphan = matches_artifact(
        views_with(&[2322], &[]),
        false,
        Some(Lineage {
            commit: "deadbeef".repeat(5),
            sha256: "0".repeat(64),
        }),
        None,
    );
    let bytes = encode_artifact(&orphan).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &bytes, "orphan");
    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("not the bootstrap"), "{err}");
}

#[test]
fn lineage_merge_with_unchanged_bytes_creates_no_version() {
    let repo = init_repo("merge");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");
    git_test(&repo, &["checkout", "-q", "-b", "feat"]);
    commit_bytes(&repo, "other.txt", b"feat side", "feat work");
    git_test(&repo, &["checkout", "-q", "main"]);
    commit_bytes(&repo, "main.txt", b"main side", "main work");
    git_test(
        &repo,
        &["merge", "-q", "--no-ff", "feat", "-m", "merge feat"],
    );

    assert_eq!(
        verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v1_bytes).unwrap(),
        1,
        "a merge carrying unchanged bytes must not create a lineage version"
    );
}

#[test]
fn lineage_side_branch_shrink_then_restore_is_not_simplified_away() {
    let repo = init_repo("merge-hidden-shrink");
    let v1 = matches_artifact(views_with(&[2322, 2345], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    git_test(&repo, &["checkout", "-q", "-b", "feat"]);
    let shrunk = matches_artifact(
        views_with(&[2322], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let shrunk_bytes = encode_artifact(&shrunk).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &shrunk_bytes, "shrink");
    // Restore the exact merge-base bytes. Default path history
    // simplification drops both side-branch commits after the merge.
    commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "restore");

    git_test(&repo, &["checkout", "-q", "main"]);
    commit_bytes(&repo, "main.txt", b"main side", "main work");
    git_test(
        &repo,
        &["merge", "-q", "--no-ff", "feat", "-m", "merge feat"],
    );

    let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &v1_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("shrank"), "{err}");
    assert!(err.contains("code 2345"), "{err}");
}

#[test]
fn lineage_rejects_concurrent_live_path_versions() {
    let repo = init_repo("concurrent-versions");
    let v1 = matches_artifact(views_with(&[2322], &[]), true, None, None);
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, MATCHES_REL_PATH, &v1_bytes, "v1");

    git_test(&repo, &["checkout", "-q", "-b", "left"]);
    let left = matches_artifact(
        views_with(&[2322, 2345], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let left_bytes = encode_artifact(&left).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &left_bytes, "left growth");

    git_test(&repo, &["checkout", "-q", "-b", "right", &c1]);
    let right = matches_artifact(
        views_with(&[2322, 2454], &[]),
        false,
        Some(lineage_to(&c1, &v1_bytes)),
        None,
    );
    let right_bytes = encode_artifact(&right).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &right_bytes, "right growth");
    // Select the current branch's bytes while retaining both path
    // histories. The accepted state must be regenerated as their
    // union, not silently choose either side.
    git_test(
        &repo,
        &[
            "merge", "-q", "--no-ff", "-s", "ours", "left", "-m", "merge",
        ],
    );

    let mut memo = GitMemo::new(&repo).unwrap();
    let err =
        verify_lineage_with_memo::<MatchesArtifact>(&mut memo, MATCHES_REL_PATH, &right_bytes)
            .unwrap_err()
            .to_string();
    assert!(err.contains("concurrent live path versions"), "{err}");
    assert_eq!(
        memo.lineage_decode_misses, 0,
        "a non-linear version DAG must fail before retaining decoded artifacts"
    );
    assert_eq!(memo.lineage_peak_live_versions, 0);
}

#[test]
fn lineage_undeclared_input_change_fails_and_universe_passes() {
    let repo = init_repo("inputs-lineage");
    let v1 = inputs_stub();
    let v1_bytes = encode_artifact(&v1).unwrap();
    let c1 = commit_bytes(&repo, ORACLE_INPUTS_REL_PATH, &v1_bytes, "v1");

    // Undeclared edit of a pinned oracle record.
    let mut edited = v1.clone();
    edited
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .get_mut("")
        .unwrap()
        .oracle_sha256 = "edited".to_owned();
    edited.bootstrap = false;
    edited.previous = Some(lineage_to(&c1, &v1_bytes));
    let edited_bytes = encode_artifact(&edited).unwrap();
    let err = verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &edited_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("without a declared transition"), "{err}");

    // Declared universe growth: old entries byte-identical, new
    // fixture enumerated.
    let mut grown = v1.clone();
    grown.fixtures.insert(
        "conformance/new.ts".to_owned(),
        v1.fixtures["conformance/a.ts"].clone(),
    );
    *grown.totals.get_mut("all").unwrap() += 1;
    grown.bootstrap = false;
    grown.previous = Some(lineage_to(&c1, &v1_bytes));
    grown.transition = Some(UNIVERSE_TRANSITION.to_owned());
    let grown_bytes = encode_artifact(&grown).unwrap();
    assert_eq!(
        verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &grown_bytes)
            .unwrap(),
        2
    );

    // A universe transition that EDITS an old entry still fails.
    let mut tampered = grown.clone();
    tampered
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .fixture_sha256 = "edited".to_owned();
    let tampered_bytes = encode_artifact(&tampered).unwrap();
    let err =
        verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &tampered_bytes)
            .unwrap_err()
            .to_string();
    assert!(err.contains("changed pinned fixture"), "{err}");

    // An unknown transition name is never accepted.
    let mut unknown = grown.clone();
    unknown.transition = Some("vendor-upgrade".to_owned());
    let unknown_bytes = encode_artifact(&unknown).unwrap();
    let err = verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &unknown_bytes)
        .unwrap_err()
        .to_string();
    assert!(err.contains("unknown transition"), "{err}");
}

#[test]
fn historical_input_transition_requires_a_same_commit_matches_pin() {
    let repo = init_repo("historical-pair");
    let v1_inputs = inputs_stub();
    let v1_inputs_bytes = encode_artifact(&v1_inputs).unwrap();
    let mut v1_matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    v1_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&v1_inputs_bytes),
        tsc_js_sha256: v1_inputs.vendor.tsc_js_sha256.clone(),
    };
    let v1_matches_bytes = encode_artifact(&v1_matches).unwrap();
    let c1 = commit_artifact_pair(&repo, &v1_matches_bytes, &v1_inputs_bytes, "bootstrap pair");
    verify_committed_artifact_pairs(&repo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH).unwrap();

    let mut grown_matches = matches_artifact(
        views_with(&[2322, 2345], &[2322]),
        false,
        Some(lineage_to(&c1, &v1_matches_bytes)),
        None,
    );
    grown_matches.inputs = v1_matches.inputs.clone();
    let grown_matches_bytes = encode_artifact(&grown_matches).unwrap();
    commit_bytes(
        &repo,
        MATCHES_REL_PATH,
        &grown_matches_bytes,
        "matches-only growth",
    );
    verify_committed_artifact_pairs(&repo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH).unwrap();

    let mut v2_inputs = v1_inputs.clone();
    v2_inputs.fixtures.insert(
        "conformance/new.ts".to_owned(),
        v1_inputs.fixtures["conformance/a.ts"].clone(),
    );
    *v2_inputs.totals.get_mut("all").unwrap() += 1;
    v2_inputs.bootstrap = false;
    v2_inputs.previous = Some(lineage_to(&c1, &v1_inputs_bytes));
    v2_inputs.transition = Some(UNIVERSE_TRANSITION.to_owned());
    let v2_inputs_bytes = encode_artifact(&v2_inputs).unwrap();
    commit_bytes(
        &repo,
        ORACLE_INPUTS_REL_PATH,
        &v2_inputs_bytes,
        "inputs only",
    );

    let err = verify_committed_artifact_pairs(&repo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH)
        .unwrap_err()
        .to_string();
    assert!(err.contains("artifact pair"), "{err}");
    assert!(err.contains("different oracle-inputs blob"), "{err}");
}

#[test]
fn working_matches_only_growth_after_an_input_transition_needs_no_new_transition() {
    let repo = init_repo("working-matches-only-after-input-transition");
    let v1_inputs = inputs_stub();
    let v1_inputs_bytes = encode_artifact(&v1_inputs).unwrap();
    let mut v1_matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    v1_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&v1_inputs_bytes),
        tsc_js_sha256: v1_inputs.vendor.tsc_js_sha256.clone(),
    };
    let v1_matches_bytes = encode_artifact(&v1_matches).unwrap();
    let c1 = commit_artifact_pair(&repo, &v1_matches_bytes, &v1_inputs_bytes, "bootstrap");

    let mut v2_inputs = v1_inputs.clone();
    v2_inputs.bootstrap = false;
    v2_inputs.previous = Some(lineage_to(&c1, &v1_inputs_bytes));
    v2_inputs.transition = Some(PRODUCER_PIN_EXTENSION.to_owned());
    v2_inputs.producer = Some(producer_stub());
    let v2_inputs_bytes = encode_artifact(&v2_inputs).unwrap();
    let mut v2_matches = matches_artifact(
        views_with(&[2322], &[2322]),
        false,
        Some(lineage_to(&c1, &v1_matches_bytes)),
        Some(PRODUCER_PIN_EXTENSION.to_owned()),
    );
    v2_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&v2_inputs_bytes),
        tsc_js_sha256: v2_inputs.vendor.tsc_js_sha256.clone(),
    };
    let v2_matches_bytes = encode_artifact(&v2_matches).unwrap();
    let c2 = commit_artifact_pair(
        &repo,
        &v2_matches_bytes,
        &v2_inputs_bytes,
        "producer transition pair",
    );

    let mut working_matches = matches_artifact(
        views_with(&[2322, 2345], &[2322]),
        false,
        Some(lineage_to(&c2, &v2_matches_bytes)),
        None,
    );
    working_matches.inputs = v2_matches.inputs.clone();
    fs::write(
        repo.join(MATCHES_REL_PATH),
        encode_artifact(&working_matches).unwrap(),
    )
    .unwrap();

    verify_accepted_pair_history(&repo)
        .expect("matches-only growth reuses the activated input manifest without a transition");
}

#[test]
fn blob_fact_cache_does_not_hide_a_repaired_historical_pair() {
    let repo = init_repo("historical-pair-cache-integrity");
    let inputs = inputs_stub();
    let inputs_bytes = encode_artifact(&inputs).unwrap();
    let mut first_matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    first_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&inputs_bytes),
        tsc_js_sha256: inputs.vendor.tsc_js_sha256.clone(),
    };
    let first_matches_bytes = encode_artifact(&first_matches).unwrap();
    let first_commit =
        commit_artifact_pair(&repo, &first_matches_bytes, &inputs_bytes, "bootstrap pair");

    let mut broken_matches = matches_artifact(
        views_with(&[2322, 2345], &[2322]),
        false,
        Some(lineage_to(&first_commit, &first_matches_bytes)),
        None,
    );
    broken_matches.inputs = MatchesInputs {
        oracle_inputs_sha256: "0".repeat(64),
        tsc_js_sha256: inputs.vendor.tsc_js_sha256.clone(),
    };
    let broken_matches_bytes = encode_artifact(&broken_matches).unwrap();
    let broken_commit = commit_bytes(
        &repo,
        MATCHES_REL_PATH,
        &broken_matches_bytes,
        "broken historical pair",
    );

    let mut repaired_matches = matches_artifact(
        views_with(&[2322, 2345, 2454], &[2322]),
        false,
        Some(lineage_to(&broken_commit, &broken_matches_bytes)),
        None,
    );
    repaired_matches.inputs = first_matches.inputs.clone();
    let repaired_matches_bytes = encode_artifact(&repaired_matches).unwrap();
    commit_bytes(
        &repo,
        MATCHES_REL_PATH,
        &repaired_matches_bytes,
        "repair current pair",
    );

    let mut memo = GitMemo::new(&repo).unwrap();
    let err = verify_committed_artifact_pairs_with_memo(
        &mut memo,
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains(&broken_commit), "{err}");
    assert!(err.contains("different oracle-inputs blob"), "{err}");
    assert_eq!(memo.pair_matches_decode_misses, 2);
    assert_eq!(memo.pair_inputs_decode_misses, 1);
}

#[test]
fn historical_input_transition_requires_the_same_transition_name() {
    let repo = init_repo("historical-transition-name");
    let v1_inputs = inputs_stub();
    let v1_inputs_bytes = encode_artifact(&v1_inputs).unwrap();
    let mut v1_matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    v1_matches.inputs.oracle_inputs_sha256 = sha256_hex(&v1_inputs_bytes);
    let v1_matches_bytes = encode_artifact(&v1_matches).unwrap();
    let c1 = commit_artifact_pair(&repo, &v1_matches_bytes, &v1_inputs_bytes, "bootstrap pair");

    let mut v2_inputs = v1_inputs.clone();
    v2_inputs.comparators = tier_1_3_comparators();
    v2_inputs.bootstrap = false;
    v2_inputs.previous = Some(lineage_to(&c1, &v1_inputs_bytes));
    v2_inputs.transition = Some(TIER_1_3_INPUT_SCHEMA_EXTENSION.to_owned());
    let v2_inputs_bytes = encode_artifact(&v2_inputs).unwrap();

    let mut v2_matches = matches_artifact(
        views_with_tiers(&[2322], &[2322], &[2322], &[2322], &[2322]),
        false,
        Some(lineage_to(&c1, &v1_matches_bytes)),
        Some(UNIVERSE_TRANSITION.to_owned()),
    );
    v2_matches.inputs.oracle_inputs_sha256 = sha256_hex(&v2_inputs_bytes);
    let v2_matches_bytes = encode_artifact(&v2_matches).unwrap();
    commit_artifact_pair(
        &repo,
        &v2_matches_bytes,
        &v2_inputs_bytes,
        "mismatched transition pair",
    );

    let err = verify_committed_artifact_pairs(&repo, MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH)
        .unwrap_err()
        .to_string();
    assert!(err.contains("same-commit"), "{err}");
    assert!(err.contains(TIER_1_3_INPUT_SCHEMA_EXTENSION), "{err}");
    assert!(err.contains(UNIVERSE_TRANSITION), "{err}");
}

// -- Trusted PR-base compare --------------------------------------------

#[test]
fn baseline_compare_catches_branch_chain_smaller_than_base() {
    let repo = init_repo("baseline");
    let base_matches = matches_artifact(views_with(&[2322, 2345], &[2322]), true, None, None);
    let base_matches_bytes = encode_artifact(&base_matches).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &base_matches_bytes, "matches v1");
    let base_inputs = inputs_stub();
    let base_inputs_bytes = encode_artifact(&base_inputs).unwrap();
    commit_bytes(
        &repo,
        ORACLE_INPUTS_REL_PATH,
        &base_inputs_bytes,
        "inputs v1",
    );
    git_test(&repo, &["branch", "-q", "base"]);

    // A rewritten branch whose self-consistent chain lost an
    // accepted identity: the direct base compare still fails.
    let head_small = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let err = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_small,
        &base_inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("code 2345"), "{err}");

    // Growth passes.
    let head_grown = matches_artifact(views_with(&[2322, 2345, 2454], &[2322]), true, None, None);
    verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_grown,
        &base_inputs,
    )
    .unwrap();

    // Input growth inside an existing fixture is additions-only too:
    // the direct base comparison must apply the same per-case subset
    // rule as the lineage edge.
    let mut case_grown_inputs = base_inputs.clone();
    case_grown_inputs
        .fixtures
        .get_mut("conformance/a.ts")
        .unwrap()
        .cases
        .insert(
            "new-matrix".to_owned(),
            CasePins {
                oracle_sha256: "new-oracle".to_owned(),
                program_sha256: "new-program".to_owned(),
                oracle_t4_sha256: None,
            },
        );
    verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_grown,
        &case_grown_inputs,
    )
    .unwrap();

    // A branch that removed a pinned fixture fails the inputs half.
    let mut head_inputs = base_inputs.clone();
    head_inputs.fixtures.clear();
    let err = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head_grown,
        &head_inputs,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("removed pinned fixture"), "{err}");
}

#[test]
fn baseline_missing_base_is_only_the_bootstrap_exception() {
    let repo = init_repo("baseline-missing");
    commit_bytes(&repo, "unrelated.txt", b"pre-artifact", "pre");
    git_test(&repo, &["branch", "-q", "base"]);
    let head = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let head_bytes = encode_artifact(&head).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &head_bytes, "bootstrap");
    // Base has no artifact; the candidate's unique bootstrap chain
    // permits the exception but tells the caller to perform an exact
    // full-corpus measurement.
    verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &head_bytes).unwrap();
    let bootstrap_base = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &head,
        &inputs_stub(),
    )
    .unwrap();
    assert!(bootstrap_base);
}

#[test]
fn baseline_rejects_an_incomplete_artifact_pair() {
    let repo = init_repo("baseline-incomplete-pair");
    let matches = matches_artifact(views_with(&[2322], &[2322]), true, None, None);
    let matches_bytes = encode_artifact(&matches).unwrap();
    commit_bytes(&repo, MATCHES_REL_PATH, &matches_bytes, "matches only");
    git_test(&repo, &["branch", "-q", "base"]);

    let err = verify_baseline(
        &repo,
        "base",
        MATCHES_REL_PATH,
        ORACLE_INPUTS_REL_PATH,
        &matches,
        &inputs_stub(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("incomplete ratchet artifact pair"), "{err}");
    assert!(err.contains("matches=present, inputs=absent"), "{err}");
}

/// End-to-end inputs pinning through the REAL build path: a
/// synthetic workspace (vendored tsc via symlink, one fixture, one
/// golden) whose golden is edited after the manifest was built.
/// This pins the symmetric-blindness class a pure diff test cannot
/// see (a build_oracle_inputs that hashed the wrong bytes would
/// agree with itself forever).
#[test]
fn build_oracle_inputs_detects_golden_edit() {
    let real_workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let ws = temp_dir("build-inputs");
    fs::create_dir_all(ws.join("ts-tests/tests/cases/conformance")).unwrap();
    std::os::unix::fs::symlink(real_workspace.join("vendor"), ws.join("vendor")).unwrap();
    // Producer modules are COPIES (not symlinks): the drift case
    // below edits them, and writing through a symlink would edit
    // the real repository files.
    fs::create_dir_all(ws.join("crates/oracle")).unwrap();
    for module in ["driver.mjs", "program-host.mjs"] {
        fs::copy(
            real_workspace.join("crates/oracle").join(module),
            ws.join("crates/oracle").join(module),
        )
        .unwrap();
    }
    fs::write(ws.join(NODE_VERSION_REL_PATH), "25.2.1\n").unwrap();
    fs::write(
        ws.join("ts-tests/tests/cases/conformance/probe.ts"),
        "var x: number = 1;\n",
    )
    .unwrap();
    let golden = crate::GoldenFile {
        schema: 2,
        fixture: "conformance/probe.ts".to_owned(),
        cases: vec![crate::GoldenCase {
            matrix_key: String::new(),
            tsrs: Vec::new(),
            oracle: vec![diag(2322, 4, "semantic")],
            oracle_empty_related_information: Vec::new(),
            tsrs_cli_hash: String::new(),
            oracle_cli_hash: String::new(),
        }],
    };
    crate::write_golden(&ws.join("goldens"), &golden).unwrap();

    let stored = build_oracle_inputs(&ws).unwrap();
    assert_eq!(stored.totals["all"], 1);
    assert_eq!(stored.totals["2xxx"], 1);
    assert_eq!(stored.totals["syntactic"], 0);
    let producer = stored.producer.as_ref().expect("producer pinned");
    assert_eq!(producer.node_version, "25.2.1");
    diff_oracle_inputs(&stored, &build_oracle_inputs(&ws).unwrap()).unwrap();

    // A manifest predating the producer pins names the migration.
    let mut unpinned = stored.clone();
    unpinned.producer = None;
    let err = diff_oracle_inputs(&unpinned, &build_oracle_inputs(&ws).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("producer-pin-extension"), "{err}");

    // Editing a producer module under the pin is named drift.
    let driver_path = ws.join("crates/oracle/driver.mjs");
    let original_driver = fs::read(&driver_path).unwrap();
    fs::write(
        &driver_path,
        [original_driver.as_slice(), b"\n// x"].concat(),
    )
    .unwrap();
    let err = diff_oracle_inputs(&stored, &build_oracle_inputs(&ws).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("producer module drifted"), "{err}");
    assert!(err.contains("driver.mjs"), "{err}");
    fs::write(&driver_path, original_driver).unwrap();

    // So is a .node-version change.
    fs::write(ws.join(NODE_VERSION_REL_PATH), "26.0.0\n").unwrap();
    let err = diff_oracle_inputs(&stored, &build_oracle_inputs(&ws).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("Node pin drift"), "{err}");
    fs::write(ws.join(NODE_VERSION_REL_PATH), "25.2.1\n").unwrap();

    // Edit one oracle record byte-for-byte in place: the rebuilt
    // manifest must diverge and name the case.
    let mut edited = golden.clone();
    edited.cases[0].oracle[0].chain.text = "edited".to_owned();
    crate::write_golden(&ws.join("goldens"), &edited).unwrap();
    let err = diff_oracle_inputs(&stored, &build_oracle_inputs(&ws).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("oracle records edited"), "{err}");
    assert!(err.contains("conformance/probe.ts"), "{err}");

    // Deleting the golden is detected outright (unreadable input).
    fs::remove_file(ws.join("goldens/conformance/probe.ts.json.zst")).unwrap();
    let err = build_oracle_inputs(&ws).unwrap_err().to_string();
    assert!(err.contains("unreadable"), "{err}");

    // A fixture edit is its own named drift class.
    crate::write_golden(&ws.join("goldens"), &golden).unwrap();
    fs::write(
        ws.join("ts-tests/tests/cases/conformance/probe.ts"),
        "var x: number = 2;\n",
    )
    .unwrap();
    let err = diff_oracle_inputs(&stored, &build_oracle_inputs(&ws).unwrap())
        .unwrap_err()
        .to_string();
    assert!(err.contains("fixture bytes edited"), "{err}");
}

#[test]
fn planned_t4_pins_recover_a_mixed_schema_working_tree_atomically() {
    let real_workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let ws = temp_dir("mixed-t4-retry");
    fs::create_dir_all(ws.join("ts-tests/tests/cases/conformance")).unwrap();
    std::os::unix::fs::symlink(real_workspace.join("vendor"), ws.join("vendor")).unwrap();
    fs::create_dir_all(ws.join("crates/oracle")).unwrap();
    for module in ["driver.mjs", "program-host.mjs", "render-driver.mjs"] {
        fs::copy(
            real_workspace.join("crates/oracle").join(module),
            ws.join("crates/oracle").join(module),
        )
        .unwrap();
    }
    fs::write(ws.join(NODE_VERSION_REL_PATH), "25.2.1\n").unwrap();

    let mut pins = T4OraclePins::new();
    for (name, schema, hash_byte) in [("a.ts", 2, 'a'), ("b.ts", 3, 'b')] {
        fs::write(
            ws.join("ts-tests/tests/cases/conformance").join(name),
            "var x: number = 1;\n",
        )
        .unwrap();
        let fixture = format!("conformance/{name}");
        crate::write_golden(
            &ws.join("goldens"),
            &crate::GoldenFile {
                schema,
                fixture: fixture.clone(),
                cases: vec![crate::GoldenCase {
                    matrix_key: String::new(),
                    tsrs: Vec::new(),
                    oracle: vec![diag(2322, 4, "semantic")],
                    oracle_empty_related_information: Vec::new(),
                    tsrs_cli_hash: String::new(),
                    oracle_cli_hash: hash_byte.to_string().repeat(64),
                }],
            },
        )
        .unwrap();
        pins.insert(
            fixture,
            [(String::new(), hash_byte.to_string().repeat(64))]
                .into_iter()
                .collect(),
        );
    }

    let err = build_oracle_inputs(&ws).unwrap_err().to_string();
    assert!(err.contains("mixed schema-2/schema-3"), "{err}");
    let recovered = build_oracle_inputs_with_t4_pins(&ws, Some(&pins)).unwrap();
    assert!(t4_active(comparator_state(&recovered.comparators).unwrap()));
    assert!(recovered.fixtures.values().all(|fixture| fixture
        .cases
        .values()
        .all(|case| case.oracle_t4_sha256.is_some())));
}

// -- ratchet.toml derived summaries --------------------------------------

fn write_tier_activation_state(
    dir: &Path,
    comparators: BTreeMap<String, ComparatorEntry>,
    views: RunSets,
    tier_summaries: [u64; 3],
) {
    let mut inputs = inputs_stub();
    inputs.producer = Some(producer_stub());
    inputs.comparators = comparators;
    let inputs_bytes = encode_artifact(&inputs).unwrap();
    let mut matches = matches_artifact(views.clone(), true, None, None);
    matches.inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&inputs_bytes),
        tsc_js_sha256: inputs.vendor.tsc_js_sha256.clone(),
    };
    let matches_bytes = encode_artifact(&matches).unwrap();
    for (rel, bytes) in [
        (ORACLE_INPUTS_REL_PATH, inputs_bytes),
        (MATCHES_REL_PATH, matches_bytes),
    ] {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    let counts = view_counts(&views);
    let mut text = String::new();
    for view in FIXED_VIEWS {
        let matched = counts[view.name()].0;
        let total = inputs.totals[view.name()];
        text.push_str(&format!(
            "[{}]\nrate = {:.6}\nmatched = {matched}\ntotal = {total}\n\
             allowed_regression = 0.0\n\n",
            view.ratchet_key(),
            canonical_summary_rate(matched, total),
        ));
    }
    let total = inputs.totals[DiagnosticBand::All.name()];
    for (tier, matched) in ["t1", "t2", "t3"].into_iter().zip(tier_summaries) {
        text.push_str(&format!(
            "[{tier}]\nrate = {:.6}\nmatched = {matched}\ntotal = {total}\n\
             allowed_regression = 0.0\n\n",
            canonical_summary_rate(matched, total),
        ));
    }
    fs::write(dir.join("ratchet.toml"), text).unwrap();
}

#[test]
fn completion_activation_proof_rejects_toml_only_claim() {
    let dir = temp_dir("tier-activation-inactive");
    write_tier_activation_state(
        &dir,
        inactive_comparators(),
        views_with(&[100], &[100]),
        [1, 1, 1],
    );
    let err = verify_tier_1_through_3_activation(&dir)
        .unwrap_err()
        .to_string();
    assert!(err.contains("accepted sets are inactive"), "{err}");
    assert!(err.contains(TIER_1_3_INPUT_SCHEMA_EXTENSION), "{err}");
}

#[test]
fn completion_activation_proof_requires_exact_artifact_summaries() {
    let views = views_with_tiers(&[100], &[100], &[100], &[100], &[100]);
    let valid = temp_dir("tier-activation-valid");
    write_tier_activation_state(&valid, tier_1_3_comparators(), views.clone(), [1, 1, 1]);
    assert_eq!(
        verify_tier_1_through_3_activation(&valid).unwrap(),
        Tier1Through3Activation {
            t1_matched: 1,
            t2_matched: 1,
            t3_matched: 1,
            total: 1,
        }
    );

    let stale = temp_dir("tier-activation-stale-summary");
    write_tier_activation_state(&stale, tier_1_3_comparators(), views, [1, 0, 1]);
    let err = verify_tier_1_through_3_activation(&stale)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ratchet.toml [t2]"), "{err}");
    assert!(err.contains("accepted artifact"), "{err}");
}

#[test]
fn completion_activation_proof_requires_a_coherent_artifact_pair() {
    let dir = temp_dir("tier-activation-pair");
    write_tier_activation_state(
        &dir,
        tier_1_3_comparators(),
        views_with_tiers(&[100], &[100], &[100], &[100], &[100]),
        [1, 1, 1],
    );
    let matches_path = dir.join(MATCHES_REL_PATH);
    let (mut matches, _): (MatchesArtifact, _) =
        read_artifact(&matches_path, "test matches").unwrap();
    matches.inputs.oracle_inputs_sha256 = "wrong".to_owned();
    fs::write(matches_path, encode_artifact(&matches).unwrap()).unwrap();
    let err = verify_tier_1_through_3_activation(&dir)
        .unwrap_err()
        .to_string();
    assert!(err.contains("pin a different oracle-inputs blob"), "{err}");
}

#[test]
fn ratchet_toml_rewrite_preserves_comments() {
    let dir = temp_dir("toml");
    let path = dir.join("ratchet.toml");
    fs::write(
        &path,
        "[\"t0\"]\n# integer gate commentary\n\"rate\" = 0.1 # display-only\nmatched = 1\ntotal = 10\nallowed_regression = 0.0\n\n\
         [t1]\nrate = 0.0\nallowed_regression = 0.0\n\n\
         [t2]\nrate = 0.0\nallowed_regression = 0.0\n\n\
         [t3]\nrate = 0.0\nallowed_regression = 0.0\n\n\
         [t0-2xxx]\nrate = 0.2\nmatched = 2\ntotal = 10\nallowed_regression = 0.0\n\n\
         [t0-syntactic]\nallowed_regression = 0.0\n\n\
         [escapes]\n# escape commentary\nmax_untagged = 9\n",
    )
    .unwrap();
    let counts: BTreeMap<String, (u64, u64)> = [
        ("all".to_owned(), (20052, 0)),
        ("2xxx".to_owned(), (10921, 0)),
        ("syntactic".to_owned(), (2242, 0)),
    ]
    .into_iter()
    .collect();
    let totals: BTreeMap<String, u64> = [
        ("all".to_owned(), 48719),
        ("2xxx".to_owned(), 20916),
        ("syntactic".to_owned(), 2246),
    ]
    .into_iter()
    .collect();
    rewrite_ratchet_summaries(&path, &counts, &totals, Some([19000, 18000, 17000])).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("# integer gate commentary"));
    assert!(text.contains("\"rate\" = 0.411585 # display-only"));
    assert!(text.contains("# escape commentary"));
    assert!(text.contains("matched = 20052"));
    assert!(text.contains("total = 48719"));
    assert!(text.contains("matched = 10921"));
    assert!(text.contains("rate = 0.522136"));
    assert!(text.contains("matched = 2242"));
    assert!(text.contains("total = 2246"));
    assert!(text.contains("rate = 0.998219"));
    assert!(text.contains("max_untagged = 9"));
    assert!(text.contains("[t1]\nrate = 0.389992"));
    assert!(text.contains("matched = 19000"));
    assert!(text.contains("[t2]\nrate = 0.369466"));
    assert!(text.contains("matched = 18000"));
    assert!(text.contains("[t3]\nrate = 0.34894"));
    assert!(text.contains("matched = 17000"));

    verify_ratchet_summaries(&path, &counts, &totals, Some([19000, 18000, 17000]), None).unwrap();
    let (_, with_t4) = render_ratchet_summaries(
        &path,
        &counts,
        &totals,
        Some([19000, 18000, 17000]),
        Some((7, 10)),
    )
    .unwrap();
    fs::write(&path, with_t4.unwrap()).unwrap();
    verify_ratchet_summaries(
        &path,
        &counts,
        &totals,
        Some([19000, 18000, 17000]),
        Some((7, 10)),
    )
    .unwrap();
    let stale_t4 = fs::read_to_string(&path)
        .unwrap()
        .replacen("matched = 7", "matched = 6", 1);
    fs::write(&path, stale_t4).unwrap();
    let err = verify_ratchet_summaries(
        &path,
        &counts,
        &totals,
        Some([19000, 18000, 17000]),
        Some((7, 10)),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("ratchet.toml [t4]"), "{err}");

    let stale = text.replacen(
        "\"rate\" = 0.411585 # display-only",
        "\"rate\" = 0.000000 # display-only",
        1,
    );
    fs::write(&path, &stale).unwrap();
    let err = verify_ratchet_summaries(&path, &counts, &totals, Some([19000, 18000, 17000]), None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("rate/matched/total"), "{err}");

    let duplicate = stale.replacen("matched = 20052", "matched = 20052\nmatched = 20052", 1);
    fs::write(&path, duplicate).unwrap();
    let err = rewrite_ratchet_summaries(&path, &counts, &totals, Some([19000, 18000, 17000]))
        .unwrap_err()
        .to_string();
    assert!(err.contains("invalid ratchet.toml"), "{err}");
}
