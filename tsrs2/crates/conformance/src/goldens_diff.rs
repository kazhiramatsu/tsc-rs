//! Old→new golden oracle diff — the oracle-correction epoch's review
//! surface (measurement-integrity.md §2).
//!
//! A correction rewrites pinned oracle records wholesale, so its PR
//! cannot be reviewed from raw `.json.zst` diffs. This report makes
//! the change auditable at the granularity the contract demands:
//! every added/removed oracle occurrence (not pooled counts), the
//! per-(code, pass) deltas, per fixed view bucket totals, and the
//! accepted identities the correction is guaranteed to lapse (their
//! oracle-side bucket vanished, so no checker behavior can keep them
//! matched).

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use super::ratchet::{self, MatchesArtifact, RunSets, FIXED_VIEWS, MATCHES_REL_PATH};
use super::{
    fixture_key, read_golden, select_fixtures, t0_key, ConformanceResult, GoldenCase, GoldenDiag,
    GoldenFile, RefreshOptions, T0Key,
};

pub struct GoldensDiffOptions {
    pub workspace: PathBuf,
    /// Git ref whose committed goldens are the OLD side; the working
    /// tree is the NEW side.
    pub baseline: String,
    pub out_json: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct OccurrenceLabel {
    pub fixture: String,
    pub matrix: String,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub code: u32,
    pub pass: Option<String>,
    pub category: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CodePassDelta {
    pub added: u64,
    pub removed: u64,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ViewTotals {
    pub old_buckets: u64,
    pub new_buckets: u64,
}

#[derive(Serialize)]
pub struct GoldensDiffReport {
    pub baseline: String,
    pub fixtures_total: usize,
    pub fixtures_changed: usize,
    pub cases_changed: usize,
    /// Per fixed view: oracle T0 bucket totals on both sides.
    pub view_totals: BTreeMap<String, ViewTotals>,
    /// Key `"<code>/<pass>"` — deltas are per view-independent
    /// occurrence, never pooled across passes.
    pub code_pass_deltas: BTreeMap<String, CodePassDelta>,
    pub added: Vec<OccurrenceLabel>,
    pub removed: Vec<OccurrenceLabel>,
    /// Per fixed view: accepted MATCHED identities whose oracle
    /// bucket no longer exists in the new goldens — these lapse no
    /// matter what the checker does. The authoritative (complete)
    /// lapse set is enumerated by `ratchet update --transition
    /// oracle-correction`; this preview exists for review before the
    /// transition runs.
    pub guaranteed_lapses: BTreeMap<String, Vec<String>>,
}

fn occurrence_label(fixture: &str, matrix: &str, diag: &GoldenDiag) -> OccurrenceLabel {
    let mut text = diag.chain.text.clone();
    if text.len() > 160 {
        let cut = text
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= 160)
            .last()
            .unwrap_or(0);
        text.truncate(cut);
        text.push('…');
    }
    OccurrenceLabel {
        fixture: fixture.to_owned(),
        matrix: matrix.to_owned(),
        file: diag.file.clone(),
        line: diag.line,
        col: diag.col,
        code: diag.code,
        pass: diag.pass.clone(),
        category: diag.category.clone(),
        text,
    }
}

/// Occurrence-level multiset diff between two oracle record lists:
/// indices of records only in `new` (added) and only in `old`
/// (removed). Identity is the full canonical record byte string, so
/// two occurrences differing anywhere (span, chain, related, pass)
/// are distinct.
pub(crate) fn diff_case_records(
    old: &[GoldenDiag],
    new: &[GoldenDiag],
) -> ConformanceResult<(Vec<usize>, Vec<usize>)> {
    let canon = |diag: &GoldenDiag| -> ConformanceResult<Vec<u8>> { Ok(serde_json::to_vec(diag)?) };
    let mut old_counts: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    for diag in old {
        *old_counts.entry(canon(diag)?).or_default() += 1;
    }
    let mut remaining = old_counts.clone();
    let mut added = Vec::new();
    for (index, diag) in new.iter().enumerate() {
        let key = canon(diag)?;
        match remaining.get_mut(&key) {
            Some(count) if *count > 0 => *count -= 1,
            _ => added.push(index),
        }
    }
    let mut new_counts: BTreeMap<Vec<u8>, u64> = BTreeMap::new();
    for diag in new {
        *new_counts.entry(canon(diag)?).or_default() += 1;
    }
    let mut removed = Vec::new();
    let mut new_remaining = new_counts;
    for (index, diag) in old.iter().enumerate() {
        let key = canon(diag)?;
        match new_remaining.get_mut(&key) {
            Some(count) if *count > 0 => *count -= 1,
            _ => removed.push(index),
        }
    }
    Ok((added, removed))
}

fn view_buckets(case: &GoldenCase) -> BTreeMap<String, BTreeSet<T0Key>> {
    FIXED_VIEWS
        .iter()
        .map(|view| {
            (
                view.name().to_owned(),
                case.oracle
                    .iter()
                    .filter(|diag| view.matches_oracle(diag))
                    .map(t0_key)
                    .collect(),
            )
        })
        .collect()
}

fn code_pass_key(diag: &GoldenDiag) -> String {
    format!("{}/{}", diag.code, diag.pass.as_deref().unwrap_or("-"))
}

pub fn goldens_diff(options: &GoldensDiffOptions) -> ConformanceResult<GoldensDiffReport> {
    let workspace = &options.workspace;
    let git_root = ratchet::git_root_for(workspace)?;
    let commit = ratchet::git(
        &git_root,
        &[
            "rev-parse",
            "--verify",
            &format!("{}^{{commit}}", options.baseline),
        ],
    )
    .map_err(|err| format!("cannot resolve baseline {}: {err}", options.baseline))?;
    let commit = String::from_utf8(commit)?.trim().to_owned();

    let fixtures = select_fixtures(&RefreshOptions {
        workspace: workspace.clone(),
        limit: None,
        files: Vec::new(),
    })?;
    let goldens_root = workspace.join("goldens");

    let mut report = GoldensDiffReport {
        baseline: format!("{} ({commit})", options.baseline),
        fixtures_total: fixtures.len(),
        fixtures_changed: 0,
        cases_changed: 0,
        view_totals: FIXED_VIEWS
            .iter()
            .map(|view| (view.name().to_owned(), ViewTotals::default()))
            .collect(),
        code_pass_deltas: BTreeMap::new(),
        added: Vec::new(),
        removed: Vec::new(),
        guaranteed_lapses: BTreeMap::new(),
    };
    // fixture -> matrix -> view -> new oracle T0 buckets, for the
    // lapse preview join below.
    let mut new_buckets: BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeSet<T0Key>>>> =
        BTreeMap::new();

    for fixture in &fixtures {
        let key = fixture_key(workspace, fixture)?;
        let new_golden = read_golden(&goldens_root, &key)
            .map_err(|err| format!("golden for {key} unreadable: {err}"))?;
        let golden_rel = ratchet::git_rel_path(&git_root, workspace, "goldens")?;
        let old_bytes = ratchet::git_blob_optional(
            &git_root,
            &commit,
            &format!("{golden_rel}/{key}.json.zst"),
        )?;
        let old_golden: Option<GoldenFile> = match old_bytes {
            Some(bytes) => {
                let json = zstd::stream::decode_all(bytes.as_slice())
                    .map_err(|err| format!("old golden for {key}: {err}"))?;
                Some(
                    serde_json::from_slice(&json)
                        .map_err(|err| format!("old golden for {key}: {err}"))?,
                )
            }
            None => None,
        };

        let mut fixture_changed = false;
        for case in &new_golden.cases {
            let buckets = view_buckets(case);
            for (view, keys) in &buckets {
                report
                    .view_totals
                    .get_mut(view)
                    .expect("fixed views seeded")
                    .new_buckets += keys.len() as u64;
            }
            new_buckets
                .entry(key.clone())
                .or_default()
                .insert(case.matrix_key.clone(), buckets);

            let empty = Vec::new();
            let old_case_records = old_golden
                .as_ref()
                .and_then(|golden| {
                    golden
                        .cases
                        .iter()
                        .find(|old_case| old_case.matrix_key == case.matrix_key)
                })
                .map_or(&empty, |old_case| &old_case.oracle);
            let (added, removed) = diff_case_records(old_case_records, &case.oracle)?;
            if !added.is_empty() || !removed.is_empty() {
                report.cases_changed += 1;
                fixture_changed = true;
            }
            for index in added {
                let diag = &case.oracle[index];
                report
                    .code_pass_deltas
                    .entry(code_pass_key(diag))
                    .or_default()
                    .added += 1;
                report
                    .added
                    .push(occurrence_label(&key, &case.matrix_key, diag));
            }
            for index in removed {
                let diag = &old_case_records[index];
                report
                    .code_pass_deltas
                    .entry(code_pass_key(diag))
                    .or_default()
                    .removed += 1;
                report
                    .removed
                    .push(occurrence_label(&key, &case.matrix_key, diag));
            }
        }
        if let Some(old_golden) = &old_golden {
            for old_case in &old_golden.cases {
                for (view, keys) in view_buckets(old_case) {
                    report
                        .view_totals
                        .get_mut(&view)
                        .expect("fixed views seeded")
                        .old_buckets += keys.len() as u64;
                }
                // A case present only on the old side is wholly
                // removed occurrences.
                if !new_golden
                    .cases
                    .iter()
                    .any(|case| case.matrix_key == old_case.matrix_key)
                {
                    report.cases_changed += 1;
                    fixture_changed = true;
                    for diag in &old_case.oracle {
                        report
                            .code_pass_deltas
                            .entry(code_pass_key(diag))
                            .or_default()
                            .removed += 1;
                        report
                            .removed
                            .push(occurrence_label(&key, &old_case.matrix_key, diag));
                    }
                }
            }
        }
        if fixture_changed {
            report.fixtures_changed += 1;
        }
    }

    // Guaranteed-lapse preview: accepted matched identities whose
    // oracle bucket vanished on the new side.
    let matches_path = workspace.join(MATCHES_REL_PATH);
    if let Ok(bytes) = fs::read(&matches_path) {
        let accepted: MatchesArtifact =
            ratchet::decode_artifact(&bytes, "accepted-match artifact")?;
        report.guaranteed_lapses = lapse_preview(&accepted.views, &new_buckets);
    }

    fs::create_dir_all(
        options
            .out_json
            .parent()
            .ok_or("goldens-diff out path has no parent")?,
    )?;
    fs::write(&options.out_json, serde_json::to_vec_pretty(&report)?)?;
    Ok(report)
}

fn lapse_preview(
    accepted: &RunSets,
    new_buckets: &BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeSet<T0Key>>>>,
) -> BTreeMap<String, Vec<String>> {
    let mut preview: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (view, fixtures) in accepted {
        let entries = preview.entry(view.clone()).or_default();
        for (fixture, cases) in fixtures {
            for (matrix, sets) in cases {
                let buckets = new_buckets
                    .get(fixture)
                    .and_then(|cases| cases.get(matrix))
                    .and_then(|views| views.get(view));
                for key in &sets.matched {
                    let vanished = buckets.is_none_or(|buckets| !buckets.contains(key));
                    if vanished {
                        entries.push(format!(
                            "{fixture} [{matrix}] {}:{}:{} code {}",
                            key.file.as_deref().unwrap_or("<none>"),
                            key.line.map_or_else(|| "-".to_owned(), |v| v.to_string()),
                            key.col.map_or_else(|| "-".to_owned(), |v| v.to_string()),
                            key.code
                        ));
                    }
                }
            }
        }
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GoldenMessageChain;

    fn diag(code: u32, start: u32, pass: &str, text: &str) -> GoldenDiag {
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

    #[test]
    fn case_record_diff_is_occurrence_exact() {
        // old: two identical 2322s and one 1005; new: ONE 2322 (one
        // occurrence removed), the same 1005, and a new 2451.
        let old = vec![
            diag(2322, 4, "semantic", "not assignable"),
            diag(2322, 4, "semantic", "not assignable"),
            diag(1005, 9, "syntactic", "expected"),
        ];
        let new = vec![
            diag(2322, 4, "semantic", "not assignable"),
            diag(1005, 9, "syntactic", "expected"),
            diag(2451, 12, "semantic", "redeclare"),
        ];
        let (added, removed) = diff_case_records(&old, &new).unwrap();
        assert_eq!(added, vec![2]);
        assert_eq!(removed, vec![1]);

        // A span move is a remove+add pair, never a silent match.
        let moved = vec![diag(2322, 6, "semantic", "not assignable")];
        let (added, removed) =
            diff_case_records(&[diag(2322, 4, "semantic", "not assignable")], &moved).unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(removed.len(), 1);
    }
}
