use std::process::Command;

use super::*;
use crate::test_git::{git_test, init_repo, temp_dir};
use crate::{GoldenMessageChain, T0Key};

fn commit_families_at(root: &Path, rel: &str, file: &FamiliesFile, message: &str) -> String {
    if let Some(parent) = root.join(rel).parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(root.join(rel), serde_json::to_vec_pretty(file).unwrap()).unwrap();
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

fn commit_families(root: &Path, file: &FamiliesFile, message: &str) -> String {
    commit_families_at(root, FAMILIES_REL_PATH, file, message)
}

fn row(code: u32, pass: &str) -> FamilyRow {
    FamilyRow {
        code,
        pass: Pass::from_oracle(pass).unwrap(),
    }
}

fn family(name: &str, owner: &str, rows: &[(u32, &str)]) -> Family {
    Family {
        name: name.to_owned(),
        owner: owner.to_owned(),
        note: format!("{name} test family"),
        rows: rows.iter().map(|(code, pass)| row(*code, pass)).collect(),
        canaries: Vec::new(),
    }
}

fn band_partition() -> BandPartition {
    BandPartition {
        family: "2xxx-band".to_owned(),
        owner: "band phase plan".to_owned(),
        note: "codes 2000-2999 wholesale".to_owned(),
    }
}

fn draft_file(families: Vec<Family>) -> FamiliesFile {
    FamiliesFile {
        schema: FAMILIES_SCHEMA,
        status: FamiliesStatus::Draft,
        band_partition: band_partition(),
        families,
        freeze: None,
        universe_extensions: Vec::new(),
    }
}

fn freeze_record(commit: &str, families: &[Family]) -> FreezeRecord {
    let mut rows: Vec<FrozenRow> = families
        .iter()
        .flat_map(|family| {
            family
                .rows
                .iter()
                .map(|row| frozen_row(&family.name, row))
                .collect::<Vec<_>>()
        })
        .collect();
    rows.sort();
    FreezeRecord {
        adjudication_commit: commit.to_owned(),
        oracle_inputs_sha256: "0".repeat(64),
        rows,
    }
}

fn frozen_from(draft: &FamiliesFile, commit: &str) -> FamiliesFile {
    let mut file = draft.clone();
    file.status = FamiliesStatus::Frozen;
    file.freeze = Some(freeze_record(commit, &draft.families));
    file
}

fn err(result: ConformanceResult<()>) -> String {
    result.unwrap_err().to_string()
}

// -- map structure (measurement-integrity.md §7: A5 map rows) ----

#[test]
fn duplicate_row_across_families_fails() {
    let file = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7027, "semantic")]),
    ]);
    let message = err(validate_structure(&file));
    assert!(
        message.contains("duplicate diag-families row (7027, semantic)"),
        "{message}"
    );
    assert!(message.contains("exactly one owner family"), "{message}");
}

#[test]
fn enumerated_two_xxx_row_fails() {
    let file = draft_file(vec![family("a", "M5", &[(2304, "semantic")])]);
    let message = err(validate_structure(&file));
    assert!(message.contains("2XXX row (2304, semantic)"), "{message}");
    assert!(message.contains("band partition"), "{message}");
}

#[test]
fn unsorted_rows_fail() {
    let mut file = draft_file(vec![family(
        "a",
        "M5",
        &[(7028, "semantic"), (7027, "semantic")],
    )]);
    let message = err(validate_structure(&file));
    assert!(message.contains("strictly sorted"), "{message}");
    file.families[0].rows.sort();
    validate_structure(&file).unwrap();
}

#[test]
fn unmapped_and_stale_domain_rows_fail() {
    let file = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let map_rows = enumerated_rows(&file);
    let corpus: BTreeSet<FamilyRow> = [row(7027, "semantic"), row(6133, "suggestion")].into();
    let message = err(verify_domain(&map_rows, &corpus));
    assert!(
        message.contains("unmapped corpus row (6133, suggestion)"),
        "{message}"
    );

    let corpus: BTreeSet<FamilyRow> = BTreeSet::new();
    let message = err(verify_domain(&map_rows, &corpus));
    assert!(message.contains("(7027, semantic)"), "{message}");
    assert!(message.contains("not exercised"), "{message}");

    let corpus: BTreeSet<FamilyRow> = [row(7027, "semantic")].into();
    verify_domain(&map_rows, &corpus).unwrap();
}

#[test]
fn status_and_anchor_coherence() {
    let base = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);

    let mut frozen_missing_record = base.clone();
    frozen_missing_record.status = FamiliesStatus::Frozen;
    let message = err(validate_structure(&frozen_missing_record));
    assert!(message.contains("no freeze record"), "{message}");

    let mut draft_with_record = base.clone();
    draft_with_record.freeze = Some(freeze_record(&"a".repeat(40), &base.families));
    let message = err(validate_structure(&draft_with_record));
    assert!(
        message.contains("draft but carries a freeze record"),
        "{message}"
    );

    let mut draft_with_extension = base.clone();
    draft_with_extension
        .universe_extensions
        .push(UniverseExtension {
            adjudication_commit: "b".repeat(40),
            oracle_inputs_sha256: "0".repeat(64),
            added: vec![FrozenRow {
                family: "a".to_owned(),
                code: 7050,
                pass: Pass::Suggestion,
            }],
            new_families: Vec::new(),
        });
    let message = err(validate_structure(&draft_with_extension));
    assert!(
        message.contains("draft but carries universe extensions"),
        "{message}"
    );
}

#[test]
fn movable_ref_anchor_fails() {
    let base = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let mut frozen = frozen_from(&base, "main");
    let message = err(validate_structure(&frozen));
    assert!(
        message.contains("not a full 40-hex commit SHA"),
        "{message}"
    );
    assert!(message.contains("movable refs"), "{message}");
    frozen.freeze.as_mut().unwrap().adjudication_commit = "A".repeat(40);
    let message = err(validate_structure(&frozen));
    assert!(
        message.contains("not a full 40-hex commit SHA"),
        "{message}"
    );
}

#[test]
fn frozen_row_composition_catches_moves_and_extras() {
    let draft = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7034, "semantic")]),
    ]);
    let commit = "c".repeat(40);
    let mut frozen = frozen_from(&draft, &commit);
    validate_structure(&frozen).unwrap();

    // An old owner change disguised as plain content: the row moves
    // family, the enumerated set still "adds up" by (code, pass).
    frozen.families[0].rows.clear();
    frozen.families[1].rows = vec![row(7027, "semantic"), row(7034, "semantic")];
    let message = err(validate_structure(&frozen));
    assert!(message.contains("(7027, semantic)"), "{message}");
    assert!(
        message.contains("old ownership is byte-stable"),
        "{message}"
    );

    // An unrecorded addition.
    let mut frozen = frozen_from(&draft, &commit);
    frozen.families[0].rows.push(row(7050, "suggestion"));
    frozen.families[0].rows.sort();
    let message = err(validate_structure(&frozen));
    assert!(message.contains("(7050, suggestion)"), "{message}");
    assert!(message.contains("anchored extension record"), "{message}");
}

// -- freeze + extension anchors (git) ----------------------------

#[test]
fn freeze_anchor_round_trip_and_post_freeze_tampers() {
    let root = init_repo("freeze");
    let draft = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7034, "semantic")]),
    ]);
    let adjudication = commit_families(&root, &draft, "draft content");
    let frozen = frozen_from(&draft, &adjudication);
    let head = commit_families(&root, &frozen, "freeze anchor");
    verify_freeze_anchors(&root, FAMILIES_REL_PATH, &head, &frozen).unwrap();

    let mut owner_tamper = frozen.clone();
    owner_tamper.families[0].owner = "M8".to_owned();
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &owner_tamper,
    ));
    assert!(message.contains("owner changed"), "{message}");

    let mut canary_tamper = frozen.clone();
    canary_tamper.families[1].canaries.push(Canary {
        fixture: "conformance/x.ts".to_owned(),
        matrix_key: String::new(),
    });
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &canary_tamper,
    ));
    assert!(message.contains("canaries changed"), "{message}");

    let mut note_tamper = frozen.clone();
    note_tamper.families[0].note = "reworded".to_owned();
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &note_tamper,
    ));
    assert!(message.contains("note changed"), "{message}");
}

#[test]
fn legacy_workspace_family_anchor_and_baseline_survive_root_promotion() {
    let root = init_repo("legacy-families-promotion");
    let legacy_rel = format!("tsrs2/{FAMILIES_REL_PATH}");
    let draft = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7034, "semantic")]),
    ]);
    let adjudication = commit_families_at(&root, &legacy_rel, &draft, "legacy family draft");
    let frozen = frozen_from(&draft, &adjudication);
    let legacy_baseline = commit_families_at(&root, &legacy_rel, &frozen, "legacy frozen families");

    git_test(&root, &["mv", &legacy_rel, FAMILIES_REL_PATH]);
    git_test(&root, &["commit", "-q", "-m", "promote families to root"]);
    let head = resolve_commit(&root, "HEAD").unwrap();

    verify_freeze_anchors(&root, FAMILIES_REL_PATH, &head, &frozen).unwrap();
    verify_families_baseline(&root, FAMILIES_REL_PATH, &legacy_baseline, &frozen).unwrap();
}

#[test]
fn freeze_add_and_reanchor_fails() {
    let root = init_repo("reanchor");
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let adjudication = commit_families(&root, &draft, "draft content");
    // The freeze enumerates MORE than the adjudicated content: a
    // branch pair adding a row and re-enumerating in one go.
    let mut grown = draft.clone();
    grown.families[0].rows.push(row(7028, "semantic"));
    let mut frozen = grown.clone();
    frozen.status = FamiliesStatus::Frozen;
    frozen.freeze = Some(freeze_record(&adjudication, &grown.families));
    let head = commit_families(&root, &frozen, "freeze anchor");
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &frozen,
    ));
    assert!(
        message.contains("does not equal the map at its adjudication commit"),
        "{message}"
    );
    assert!(message.contains("add-and-reanchor"), "{message}");
}

#[test]
fn freeze_anchor_non_ancestor_fails() {
    let root = init_repo("non-ancestor");
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    commit_families(&root, &draft, "base");
    git_test(&root, &["checkout", "-q", "-b", "side"]);
    let mut side_draft = draft.clone();
    side_draft.families[0].note = "side".to_owned();
    let side = commit_families(&root, &side_draft, "side content");
    git_test(&root, &["checkout", "-q", "main"]);
    let frozen = frozen_from(&side_draft, &side);
    let head = commit_families(&root, &frozen, "freeze anchor");
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &frozen,
    ));
    assert!(message.contains("not an ancestor of HEAD"), "{message}");
}

#[test]
fn freeze_anchor_must_target_the_reviewed_draft() {
    let root = init_repo("anchor-frozen");
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let adjudication = commit_families(&root, &draft, "draft content");
    let frozen = frozen_from(&draft, &adjudication);
    let freeze_commit = commit_families(&root, &frozen, "freeze anchor");
    // Re-anchor on the freeze commit itself: the map there is not
    // the reviewed draft.
    let mut reanchored = frozen.clone();
    reanchored.freeze.as_mut().unwrap().adjudication_commit = freeze_commit.clone();
    let head = commit_families(&root, &reanchored, "reanchor");
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &reanchored,
    ));
    assert!(message.contains("not the reviewed draft"), "{message}");
}

#[test]
fn universe_extension_round_trip_and_disguised_move() {
    let root = init_repo("extension");
    let draft = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7034, "semantic")]),
    ]);
    let adjudication = commit_families(&root, &draft, "draft content");
    let frozen = frozen_from(&draft, &adjudication);
    commit_families(&root, &frozen, "freeze anchor");

    // Extension content commit: the new row lands in its family.
    let mut extended_content = frozen.clone();
    extended_content.families[0]
        .rows
        .push(row(7028, "semantic"));
    extended_content.families[0].rows.sort();
    let extension_commit = commit_families(&root, &extended_content, "extension rows");

    // Follow-up records the anchored extension.
    let mut extended = extended_content.clone();
    extended.universe_extensions.push(UniverseExtension {
        adjudication_commit: extension_commit,
        oracle_inputs_sha256: "1".repeat(64),
        added: vec![FrozenRow {
            family: "a".to_owned(),
            code: 7028,
            pass: Pass::Semantic,
        }],
        new_families: Vec::new(),
    });
    let head = commit_families(&root, &extended, "extension record");
    validate_structure(&extended).unwrap();
    verify_freeze_anchors(&root, FAMILIES_REL_PATH, &head, &extended).unwrap();

    // Disguise: the "extension" also moves an old row to another
    // family. The composition already fails structurally.
    let mut disguised = extended.clone();
    disguised.families[0].rows.retain(|row| row.code != 7027);
    disguised.families[1].rows.insert(0, row(7027, "semantic"));
    disguised.families[1].rows.sort();
    let message = err(validate_structure(&disguised));
    assert!(message.contains("(7027, semantic)"), "{message}");
    assert!(
        message.contains("old ownership is byte-stable"),
        "{message}"
    );

    // And a disguise that rewrites the freeze enumeration too is
    // caught by the anchor compare.
    let mut reanchored = disguised.clone();
    let mut rows = reanchored.freeze.as_ref().unwrap().rows.clone();
    for frozen_row in &mut rows {
        if frozen_row.code == 7027 {
            frozen_row.family = "b".to_owned();
        }
    }
    rows.sort();
    reanchored.freeze.as_mut().unwrap().rows = rows;
    validate_structure(&reanchored).unwrap();
    let head = commit_families(&root, &reanchored, "disguised move");
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &reanchored,
    ));
    assert!(
        message.contains("does not equal the map at its adjudication commit"),
        "{message}"
    );
}

#[test]
fn extension_readding_existing_row_fails() {
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let commit = "d".repeat(40);
    let mut frozen = frozen_from(&draft, &commit);
    frozen.universe_extensions.push(UniverseExtension {
        adjudication_commit: "e".repeat(40),
        oracle_inputs_sha256: "0".repeat(64),
        added: vec![FrozenRow {
            family: "a".to_owned(),
            code: 7027,
            pass: Pass::Semantic,
        }],
        new_families: Vec::new(),
    });
    let message = err(validate_structure(&frozen));
    assert!(
        message.contains("re-adds frozen row (7027, semantic)"),
        "{message}"
    );
}

// -- trusted-base compare ----------------------------------------

#[test]
fn baseline_windows_and_attacks() {
    let root = init_repo("baseline");
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);

    // Introduction window: trusted base predates the map.
    let pre_map = {
        fs::write(root.join("other.txt"), b"x").unwrap();
        git_test(&root, &["add", "other.txt"]);
        git_test(&root, &["commit", "-q", "-m", "pre-map"]);
        String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    };
    let adjudication = commit_families(&root, &draft, "draft content");
    verify_families_baseline(&root, FAMILIES_REL_PATH, &pre_map, &draft).unwrap();

    // Draft edits against a draft base pass.
    let mut edited = draft.clone();
    edited.families[0].owner = "M6".to_owned();
    verify_families_baseline(&root, FAMILIES_REL_PATH, &adjudication, &edited).unwrap();

    // First freeze: extensions cannot ride it.
    let frozen = frozen_from(&draft, &adjudication);
    verify_families_baseline(&root, FAMILIES_REL_PATH, &adjudication, &frozen).unwrap();
    let mut frozen_with_extension = frozen.clone();
    frozen_with_extension.families[0]
        .rows
        .push(row(7028, "semantic"));
    frozen_with_extension.families[0].rows.sort();
    frozen_with_extension
        .universe_extensions
        .push(UniverseExtension {
            adjudication_commit: "f".repeat(40),
            oracle_inputs_sha256: "0".repeat(64),
            added: vec![FrozenRow {
                family: "a".to_owned(),
                code: 7028,
                pass: Pass::Semantic,
            }],
            new_families: Vec::new(),
        });
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &adjudication,
        &frozen_with_extension,
    ));
    assert!(
        message.contains("first freeze cannot carry universe extensions"),
        "{message}"
    );

    let frozen_base = commit_families(&root, &frozen, "freeze anchor");

    // Status downgrade.
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &frozen_base,
        &draft,
    ));
    assert!(message.contains("status downgrade"), "{message}");

    // Freeze record rewrite against the trusted base.
    let mut refrozen = frozen.clone();
    refrozen.freeze.as_mut().unwrap().oracle_inputs_sha256 = "9".repeat(64);
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &frozen_base,
        &refrozen,
    ));
    assert!(message.contains("freeze record differs"), "{message}");
    assert!(message.contains("add-and-reanchor"), "{message}");

    // Owner edit beyond appended extensions.
    let mut owner_edit = frozen.clone();
    owner_edit.families[0].owner = "M8".to_owned();
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &frozen_base,
        &owner_edit,
    ));
    assert!(
        message.contains("beyond appended universe extensions"),
        "{message}"
    );

    // A legitimate appended extension passes the base compare.
    let mut extended = frozen.clone();
    extended.families[0].rows.push(row(7028, "semantic"));
    extended.families[0].rows.sort();
    extended.universe_extensions.push(UniverseExtension {
        adjudication_commit: "f".repeat(40),
        oracle_inputs_sha256: "1".repeat(64),
        added: vec![FrozenRow {
            family: "a".to_owned(),
            code: 7028,
            pass: Pass::Semantic,
        }],
        new_families: Vec::new(),
    });
    verify_families_baseline(&root, FAMILIES_REL_PATH, &frozen_base, &extended).unwrap();

    // But a REWRITTEN extension prefix fails once it is in the base.
    let extended_base = commit_families(&root, &extended, "extension");
    let mut rewritten = extended.clone();
    rewritten.universe_extensions[0].oracle_inputs_sha256 = "2".repeat(64);
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &extended_base,
        &rewritten,
    ));
    assert!(
        message.contains("append-only against the trusted base"),
        "{message}"
    );
}

// -- rollup (measurement-integrity.md §7: A5 rollup rows) --------

#[test]
fn partial_or_banded_observation_is_refused() {
    let message = err(ensure_observation_eligible(
        crate::DiagnosticBand::TwoXxx,
        true,
    ));
    assert!(message.contains("A1 summaries cannot supply"), "{message}");
    let message = err(ensure_observation_eligible(
        crate::DiagnosticBand::All,
        false,
    ));
    assert!(message.contains("full_run=false"), "{message}");
    ensure_observation_eligible(crate::DiagnosticBand::All, true).unwrap();
}

fn golden_diag(code: u32, start: u32, pass: &str) -> GoldenDiag {
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
            text: "t".to_owned(),
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

#[test]
fn mixed_pass_bucket_fails() {
    let oracle = vec![
        golden_diag(6133, 4, "semantic"),
        golden_diag(6133, 4, "suggestion"),
    ];
    let message = case_bucket_passes("conformance/a.ts", "", &oracle)
        .unwrap_err()
        .to_string();
    assert!(message.contains("mixed-pass T0 bucket"), "{message}");
    assert!(message.contains("adjudicate"), "{message}");
}

fn bucket(
    code: u32,
    pass: &str,
    oracle_multiplicity: usize,
    excluded: usize,
    matched: bool,
) -> BucketObservation {
    BucketObservation {
        code,
        pass: Pass::from_oracle(pass).unwrap(),
        oracle_multiplicity,
        tsrs_multiplicity: if matched { oracle_multiplicity } else { 0 },
        excluded_occurrences: excluded,
        matched,
    }
}

fn dummy_inputs() -> InputFingerprints {
    InputFingerprints {
        diag_families_sha256: "0".repeat(64),
        m8_scope_sha256: "0".repeat(64),
        oracle_inputs_sha256: "0".repeat(64),
        conformance_matches_sha256: "0".repeat(64),
        tsc_js_sha256: "0".repeat(64),
        tsrs_exe_sha256: "0".repeat(64),
    }
}

#[test]
fn partial_exclusion_keeps_the_surviving_neighbor_supported() {
    let mut file = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    file.families[0].canaries.push(Canary {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
    });
    let observation = Observation {
        fixtures_total: 1,
        cases: vec![CaseObservation {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            false_positives: 0,
            // Duplicate bucket, one of two occurrences excluded:
            // the neighbor keeps the bucket in the supported
            // denominator.
            buckets: vec![bucket(7027, "semantic", 2, 1, true)],
        }],
    };
    let report = grade(&file, &observation, dummy_inputs()).unwrap();
    assert_eq!(report.families[0].grade.total, 1);
    assert_eq!(report.families[0].grade.supported_total, 1);
    assert_eq!(report.families[0].grade.supported_matched, 1);

    // Excluding EVERY occurrence removes the bucket from the
    // supported denominator but never from the all-corpus one.
    let observation = Observation {
        fixtures_total: 1,
        cases: vec![CaseObservation {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            false_positives: 0,
            buckets: vec![bucket(7027, "semantic", 2, 2, false)],
        }],
    };
    let report = grade(&file, &observation, dummy_inputs()).unwrap();
    assert_eq!(report.families[0].grade.total, 1);
    assert_eq!(report.families[0].grade.false_negative, 1);
    assert_eq!(report.families[0].grade.supported_total, 0);
    assert_eq!(report.families[0].grade.supported_false_negative, 0);
}

#[test]
fn grade_enforces_domain_equality() {
    let file = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let observation = Observation {
        fixtures_total: 1,
        cases: vec![CaseObservation {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            false_positives: 0,
            buckets: vec![
                bucket(7027, "semantic", 1, 0, true),
                bucket(6133, "suggestion", 1, 0, false),
            ],
        }],
    };
    let message = grade(&file, &observation, dummy_inputs())
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("unmapped corpus row (6133, suggestion)"),
        "{message}"
    );
}

#[test]
fn canary_grading_is_family_scoped() {
    let mut file = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7034, "semantic")]),
    ]);
    file.families[0].canaries.push(Canary {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
    });
    // Row-less family: the whole case must match, and a false
    // positive fails it.
    file.families.push(Family {
        name: "suppression".to_owned(),
        owner: "M7 8.2".to_owned(),
        note: "audit".to_owned(),
        rows: Vec::new(),
        canaries: vec![Canary {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
        }],
    });
    let observation = Observation {
        fixtures_total: 1,
        cases: vec![CaseObservation {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            false_positives: 1,
            buckets: vec![
                bucket(7027, "semantic", 1, 0, true),
                bucket(7034, "semantic", 1, 0, false),
            ],
        }],
    };
    let report = grade(&file, &observation, dummy_inputs()).unwrap();
    // Family a's canary sees only its own matched row.
    assert!(report.families[0].canaries[0].passed);
    // The row-less family sees the case-wide FN and the FP.
    let suppression = &report.families[2];
    assert!(!suppression.canaries[0].passed);
    assert_eq!(suppression.canaries[0].family_false_negative, 1);
}

#[test]
fn duplicate_canary_requires_multiplicity_complete_output() {
    let mut file = draft_file(vec![family("a", "M7 8.1", &[(7027, "semantic")])]);
    file.families[0].canaries.push(Canary {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
    });
    let mut duplicate = bucket(7027, "semantic", 2, 0, true);
    duplicate.tsrs_multiplicity = 1;
    let observation = Observation {
        fixtures_total: 1,
        cases: vec![CaseObservation {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            false_positives: 0,
            // T0 set membership alone matches, but one of the two
            // duplicate occurrences is still missing.
            buckets: vec![duplicate],
        }],
    };

    let report = grade(&file, &observation, dummy_inputs()).unwrap();
    let canary = &report.families[0].canaries[0];
    assert_eq!(canary.family_false_negative, 0);
    assert_eq!(canary.multiplicity_incomplete, 1);
    assert!(!canary.passed);
    assert_eq!(report.families[0].canaries_passed, 0);
}

#[test]
fn stale_report_fingerprints_and_totals_fail() {
    let workspace = temp_dir("report");
    fs::create_dir_all(workspace.join("ratchets")).unwrap();
    fs::create_dir_all(workspace.join("vendor/typescript-6.0.3/lib")).unwrap();
    fs::write(workspace.join(FAMILIES_REL_PATH), b"map").unwrap();
    fs::write(workspace.join("m8-scope.json"), b"scope").unwrap();
    fs::write(
        workspace.join(crate::ratchet::ORACLE_INPUTS_REL_PATH),
        b"inputs",
    )
    .unwrap();
    fs::write(workspace.join(crate::ratchet::MATCHES_REL_PATH), b"matches").unwrap();
    fs::write(
        workspace.join("vendor/typescript-6.0.3/lib/_tsc.js"),
        b"tsc",
    )
    .unwrap();

    let file = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let observation = Observation {
        fixtures_total: 1,
        cases: vec![CaseObservation {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
            false_positives: 0,
            buckets: vec![bucket(7027, "semantic", 1, 0, true)],
        }],
    };
    let inputs = InputFingerprints::current(&workspace).unwrap();
    let report = grade(&file, &observation, inputs).unwrap();
    let report_path = workspace.join("target/families/report.json");
    fs::create_dir_all(report_path.parent().unwrap()).unwrap();
    fs::write(&report_path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    verify_report_freshness(&workspace, &report_path).unwrap();

    // Any input moving under the stored rollup is a stale report.
    fs::write(workspace.join("m8-scope.json"), b"scope-v2").unwrap();
    let message = verify_report_freshness(&workspace, &report_path)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("stale families report: m8-scope.json"),
        "{message}"
    );
    fs::write(workspace.join("m8-scope.json"), b"scope").unwrap();

    // Doctored per-family counts cannot pass as a rollup.
    let mut doctored = report.clone();
    doctored.families[0].grade.total += 1;
    fs::write(&report_path, serde_json::to_vec_pretty(&doctored).unwrap()).unwrap();
    let message = verify_report_freshness(&workspace, &report_path)
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("family totals do not equal their row totals"),
        "{message}"
    );
}

#[test]
fn case_observation_counts_excluded_occurrences_per_bucket() {
    let oracle = vec![
        golden_diag(7027, 4, "semantic"),
        golden_diag(7027, 4, "semantic"),
        golden_diag(7034, 9, "semantic"),
    ];
    let tsrs = vec![golden_diag(7027, 4, "semantic")];
    let excluded: BTreeSet<usize> = [1usize].into();
    let matched: BTreeSet<T0Key> = [crate::t0_key(&oracle[0])].into();
    let case = CaseObservation::collect(
        "conformance/a.ts",
        "",
        &oracle,
        &tsrs,
        &excluded,
        &matched,
        0,
    )
    .unwrap();
    let dup = case
        .buckets
        .iter()
        .find(|bucket| bucket.code == 7027)
        .unwrap();
    assert_eq!(dup.oracle_multiplicity, 2);
    assert_eq!(dup.excluded_occurrences, 1);
    assert!(!dup.fully_excluded());
    assert!(dup.matched);
    assert_eq!(dup.tsrs_multiplicity, 1);
    let single = case
        .buckets
        .iter()
        .find(|bucket| bucket.code == 7034)
        .unwrap();
    assert!(!single.matched);
    assert_eq!(single.excluded_occurrences, 0);
}

// -- review-hardening rows (PR #23 max review) -------------------

#[test]
fn unknown_fields_are_rejected_everywhere() {
    let file = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let mut value = serde_json::to_value(&file).unwrap();
    value["families"][0]["adjudication_note"] = serde_json::json!("approved per review");
    let message = parse_families_bytes(&serde_json::to_vec(&value).unwrap(), "test")
        .unwrap_err()
        .to_string();
    assert!(message.contains("unknown field"), "{message}");

    let mut value = serde_json::to_value(&file).unwrap();
    value["ratified"] = serde_json::json!(true);
    let message = parse_families_bytes(&serde_json::to_vec(&value).unwrap(), "test")
        .unwrap_err()
        .to_string();
    assert!(message.contains("unknown field"), "{message}");
}

#[test]
fn first_freeze_cannot_ride_the_introduction_window() {
    let root = init_repo("intro-freeze");
    fs::write(root.join("other.txt"), b"x").unwrap();
    git_test(&root, &["add", "other.txt"]);
    git_test(&root, &["commit", "-q", "-m", "pre-map"]);
    let pre_map = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let adjudication = commit_families(&root, &draft, "draft content");
    let frozen = frozen_from(&draft, &adjudication);
    commit_families(&root, &frozen, "freeze anchor");
    // The anchors themselves verify (same-branch ancestor), but the
    // trusted-base leg must reject the self-attested first freeze.
    let head = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    verify_freeze_anchors(&root, FAMILIES_REL_PATH, &head, &frozen).unwrap();
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &pre_map,
        &frozen,
    ));
    assert!(
        message.contains("cannot ride the introduction PR"),
        "{message}"
    );
}

#[test]
fn extension_anchor_with_divergent_recorded_history_fails() {
    let root = init_repo("ext-history");
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let adjudication = commit_families(&root, &draft, "draft content");
    let frozen = frozen_from(&draft, &adjudication);
    commit_families(&root, &frozen, "freeze anchor");

    // The extension's content commit carries a FABRICATED prior
    // extension record alongside the correct rows.
    let mut fabricated = frozen.clone();
    fabricated.families[0].rows.push(row(7028, "semantic"));
    fabricated.families[0].rows.sort();
    fabricated.universe_extensions.push(UniverseExtension {
        adjudication_commit: "e".repeat(40),
        oracle_inputs_sha256: "5".repeat(64),
        added: vec![FrozenRow {
            family: "a".to_owned(),
            code: 7028,
            pass: Pass::Semantic,
        }],
        new_families: Vec::new(),
    });
    let ext_commit = commit_families(&root, &fabricated, "extension rows + fake history");

    let mut extended = frozen.clone();
    extended.families[0].rows.push(row(7028, "semantic"));
    extended.families[0].rows.sort();
    extended.universe_extensions.push(UniverseExtension {
        adjudication_commit: ext_commit,
        oracle_inputs_sha256: "1".repeat(64),
        added: vec![FrozenRow {
            family: "a".to_owned(),
            code: 7028,
            pass: Pass::Semantic,
        }],
        new_families: Vec::new(),
    });
    let head = commit_families(&root, &extended, "extension record");
    let message = err(verify_freeze_anchors(
        &root,
        FAMILIES_REL_PATH,
        &head,
        &extended,
    ));
    assert!(message.contains("different extension history"), "{message}");
}

#[test]
fn malformed_frozen_base_is_an_error_not_a_panic() {
    let root = init_repo("bad-base");
    // A frozen map WITHOUT a freeze record can only exist as an
    // unvalidated historical blob; hand-craft and commit it.
    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let mut value = serde_json::to_value(&draft).unwrap();
    value["status"] = serde_json::json!("frozen");
    fs::write(
        root.join(FAMILIES_REL_PATH),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    git_test(&root, &["add", FAMILIES_REL_PATH]);
    git_test(&root, &["commit", "-q", "-m", "malformed frozen base"]);
    let base = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let adjudication = commit_families(&root, &draft, "draft again");
    let frozen = frozen_from(&draft, &adjudication);
    let message = err(verify_families_baseline(
        &root,
        FAMILIES_REL_PATH,
        &base,
        &frozen,
    ));
    assert!(
        message.contains("frozen without a freeze record"),
        "{message}"
    );
}

fn domain_with_case(rows: &[FamilyRow], case_rows: &[FamilyRow]) -> CorpusDomain {
    CorpusDomain {
        rows: rows.iter().copied().collect(),
        cases: [("conformance/a.ts".to_owned(), String::new())].into(),
        retained_case_rows: [(
            ("conformance/a.ts".to_owned(), String::new()),
            case_rows.iter().copied().collect(),
        )]
        .into(),
        two_xxx_buckets: 0,
        fixtures: 1,
    }
}

#[test]
fn vacuous_canary_fails_check_and_never_passes_grading() {
    let mut file = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    file.families[0].canaries.push(Canary {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
    });

    // check side: the canary case owns no family bucket.
    let empty_case = domain_with_case(&[row(7027, "semantic")], &[row(7034, "semantic")]);
    let message = err(verify_canary_anchoring(&file, &empty_case));
    assert!(message.contains("vacuous"), "{message}");
    assert!(message.contains("anchors nothing"), "{message}");

    let anchored = domain_with_case(&[row(7027, "semantic")], &[row(7027, "semantic")]);
    verify_canary_anchoring(&file, &anchored).unwrap();

    // A row-less family is exempt (whole-case semantics).
    let mut suppression = draft_file(vec![Family {
        name: "suppression".to_owned(),
        owner: "M7 8.2".to_owned(),
        note: "audit".to_owned(),
        rows: Vec::new(),
        canaries: vec![Canary {
            fixture: "conformance/a.ts".to_owned(),
            matrix_key: String::new(),
        }],
    }]);
    suppression.families[0].rows.clear();
    verify_canary_anchoring(&suppression, &empty_case).unwrap();

    // grade side: the same condition is defense in depth — the
    // canary reports vacuous and cannot pass even with zero FN.
    // Family a's canary case carries only family b's bucket; a's
    // row is exercised elsewhere so the domain stays balanced.
    let mut map = draft_file(vec![
        family("a", "M5", &[(7027, "semantic")]),
        family("b", "M6", &[(7034, "semantic")]),
    ]);
    map.families[0].canaries.push(Canary {
        fixture: "conformance/a.ts".to_owned(),
        matrix_key: String::new(),
    });
    let report = grade(
        &map,
        &Observation {
            fixtures_total: 2,
            cases: vec![
                CaseObservation {
                    fixture: "conformance/a.ts".to_owned(),
                    matrix_key: String::new(),
                    false_positives: 0,
                    buckets: vec![bucket(7034, "semantic", 1, 0, true)],
                },
                CaseObservation {
                    fixture: "conformance/b.ts".to_owned(),
                    matrix_key: String::new(),
                    false_positives: 0,
                    buckets: vec![bucket(7027, "semantic", 1, 0, true)],
                },
            ],
        },
        dummy_inputs(),
    )
    .unwrap();
    let canary = &report.families[0].canaries[0];
    assert!(canary.vacuous);
    assert!(!canary.passed);
    assert_eq!(canary.family_false_negative, 0);
}

#[test]
fn inputs_moving_during_the_run_invalidate_the_rollup() {
    let before = dummy_inputs();
    ensure_inputs_stable(&before, &before.clone()).unwrap();
    let mut after = before.clone();
    after.m8_scope_sha256 = "1".repeat(64);
    let message = err(ensure_inputs_stable(&before, &after));
    assert!(
        message.contains("changed while the observation ran"),
        "{message}"
    );
}

#[test]
fn prepare_report_rejects_working_tree_tampering_of_a_frozen_map() {
    // Canonicalize: prepare_report resolves the git toplevel,
    // which canonicalizes macOS /var -> /private/var temp paths.
    let root = init_repo("prepare").canonicalize().unwrap();
    fs::create_dir_all(root.join("ratchets")).unwrap();
    fs::create_dir_all(root.join("vendor/typescript-6.0.3/lib")).unwrap();
    fs::write(root.join(SCOPE_REL_PATH), b"scope").unwrap();
    fs::write(root.join(ORACLE_INPUTS_REL_PATH), b"inputs").unwrap();
    fs::write(root.join(MATCHES_REL_PATH), b"matches").unwrap();
    fs::write(root.join("vendor/typescript-6.0.3/lib/_tsc.js"), b"tsc").unwrap();

    let draft = draft_file(vec![family("a", "M5", &[(7027, "semantic")])]);
    let adjudication = commit_families(&root, &draft, "draft content");
    let frozen = frozen_from(&draft, &adjudication);
    commit_families(&root, &frozen, "freeze anchor");
    prepare_report(&root).unwrap();

    // A working-tree owner edit leaves the row composition intact;
    // only the anchor comparison can see it — and the rollup path
    // must run that comparison.
    let mut tampered = frozen.clone();
    tampered.families[0].owner = "M8".to_owned();
    fs::write(
        root.join(FAMILIES_REL_PATH),
        serde_json::to_vec_pretty(&tampered).unwrap(),
    )
    .unwrap();
    let message = prepare_report(&root).unwrap_err().to_string();
    assert!(message.contains("owner changed"), "{message}");
}

#[test]
fn orphan_golden_cases_are_named() {
    let case = |matrix_key: &str| crate::GoldenCase {
        matrix_key: matrix_key.to_owned(),
        tsrs: Vec::new(),
        oracle: Vec::new(),
        oracle_empty_related_information: Vec::new(),
        tsrs_cli_hash: String::new(),
        oracle_cli_hash: String::new(),
    };
    let cases = vec![case(""), case("target=es5")];
    let expanded: BTreeSet<&str> = ["", "target=es5"].into();
    assert_eq!(crate::orphan_golden_case(&cases, &expanded), None);
    let shrunk: BTreeSet<&str> = [""].into();
    assert_eq!(
        crate::orphan_golden_case(&cases, &shrunk),
        Some("target=es5")
    );
}
