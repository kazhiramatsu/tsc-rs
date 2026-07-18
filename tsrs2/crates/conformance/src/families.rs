//! A5 family ownership and supported rollup
//! (measurement-integrity.md §5).
//!
//! The map (`diag-families.json`, schema 1) enumerates every
//! corpus-exercised non-2XXX `(code, pass)` row exactly once under an
//! owner family; codes 2000-2999 belong wholesale to the band
//! partition and may never appear as enumerated rows. The map starts
//! `draft` and freezes through the reviewed snapshot protocol: the
//! adjudicated content lands while `draft`, a follow-up change records
//! the adjudication commit plus the complete enumerated row set. The
//! map's introduction PR lands the DRAFT only — like A2's
//! missing-base window, the first freeze cannot ride it; the freeze
//! is its own reviewed change against a trusted base that already
//! carries the draft. After the freeze, old ownership (owner strings,
//! rows, canaries, notes) is byte-stable — the artifact parse rejects
//! unknown fields so nothing can ride the frozen file outside the
//! compared content — and the domain grows only through anchored
//! `universe-extension` records that ride an A1 universe transition
//! and add new rows (and, for new families, new owners) only.
//!
//! The rollup (`families report`) derives from one current
//! full-conformance observation plus four verified inputs: the A1
//! accepted state (monotonic guard, enforced by the gating run
//! itself), the immutable oracle-input manifest, the exact A2 scope,
//! and this map. Exact scope is applied to the oracle occurrence
//! multisets before supported grading — a partially excluded
//! duplicate bucket keeps its surviving neighbors in the supported
//! denominator — and the grading never substitutes A1 summaries for
//! the current observation: a partial-fixture projection cannot
//! produce a rollup at all.
//!
//! The `oracle_inputs_sha256` fields on freeze/extension records are
//! review provenance, not a gate: an `oracle-correction` epoch may
//! move the manifest without touching the domain, so the live
//! corpus-domain equality check is the binding coupling.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{
    fixture_key, read_golden, select_fixtures, t0_key, ConformanceResult, GoldenDiag,
    RefreshOptions, TWO_XXX_CODES,
};
use crate::ratchet::{
    git_blob_optional, git_rel_path, git_root_for, resolve_commit, sha256_hex, vendor_tsc_js_path,
    MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH,
};
use crate::scope::{is_ancestor, resolve_anchor, validate_anchor_commit, SCOPE_REL_PATH};

pub(crate) const FAMILIES_REL_PATH: &str = "diag-families.json";
const FAMILIES_SCHEMA: u32 = 1;

/// Oracle pass provenance as a closed type: the map, the observation,
/// and the rollup never carry it as a string. Variants stay in
/// ALPHABETICAL kebab-case order so the derived `Ord` matches the
/// string order the sorted artifact rows were authored under
/// ("semantic" < "suggestion" < "syntactic").
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Pass {
    Semantic,
    Suggestion,
    Syntactic,
}

impl Pass {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Semantic => "semantic",
            Self::Suggestion => "suggestion",
            Self::Syntactic => "syntactic",
        }
    }

    fn from_oracle(pass: &str) -> Option<Self> {
        match pass {
            "semantic" => Some(Self::Semantic),
            "suggestion" => Some(Self::Suggestion),
            "syntactic" => Some(Self::Syntactic),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FamiliesStatus {
    Draft,
    Frozen,
}

impl FamiliesStatus {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Frozen => "frozen",
        }
    }
}

// Every map struct rejects unknown fields: the freeze/baseline guards
// compare parsed structs, so a field serde silently dropped would be
// invisible to every "byte-stable after the freeze" comparison.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FamiliesFile {
    pub(crate) schema: u32,
    pub(crate) status: FamiliesStatus,
    /// The 2XXX band partition claim: codes 2000-2999 wholesale, never
    /// enumerated row-by-row (they are owned by the band phase plan).
    pub(crate) band_partition: BandPartition,
    pub(crate) families: Vec<Family>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) freeze: Option<FreezeRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) universe_extensions: Vec<UniverseExtension>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BandPartition {
    pub(crate) family: String,
    pub(crate) owner: String,
    pub(crate) note: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Family {
    pub(crate) name: String,
    pub(crate) owner: String,
    pub(crate) note: String,
    /// Sorted by (code, pass), unique across the whole map. A family
    /// with no rows (e.g. the suppression surfaces) is legal: its
    /// acceptance is carried entirely by its canaries.
    pub(crate) rows: Vec<FamilyRow>,
    /// Sorted by (fixture, matrix_key), unique within the family. The
    /// exact fixture + matrix anchors the family's owner-stage gate
    /// must match at T0. A rowed family's canary case must contain at
    /// least one family-owned bucket — a vacuous canary cannot anchor
    /// anything.
    pub(crate) canaries: Vec<Canary>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FamilyRow {
    pub(crate) code: u32,
    pub(crate) pass: Pass,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Canary {
    pub(crate) fixture: String,
    pub(crate) matrix_key: String,
}

/// Reviewed snapshot anchor (measurement-integrity.md §1.2): the
/// adjudication commit landed the complete reviewed content while the
/// map was `draft`; this record enumerates that content so an
/// add-and-reanchor pair of branch commits cannot redefine it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FreezeRecord {
    pub(crate) adjudication_commit: String,
    /// Review provenance only (see module docs).
    pub(crate) oracle_inputs_sha256: String,
    pub(crate) rows: Vec<FrozenRow>,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrozenRow {
    pub(crate) family: String,
    pub(crate) code: u32,
    pub(crate) pass: Pass,
}

/// An A1 universe transition introducing new `(code, pass)` rows adds
/// them through one of these anchored records: the rows (and any new
/// families) land in one commit, the record naming that commit lands
/// in a follow-up change. Every old row remains byte-identical.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UniverseExtension {
    pub(crate) adjudication_commit: String,
    /// Review provenance only (see module docs).
    pub(crate) oracle_inputs_sha256: String,
    pub(crate) added: Vec<FrozenRow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) new_families: Vec<NewFamily>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewFamily {
    pub(crate) name: String,
    pub(crate) owner: String,
    pub(crate) note: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) canaries: Vec<Canary>,
}

#[derive(Deserialize)]
struct SchemaProbe {
    schema: u64,
}

/// The single probe + shape parse both entry points share; only the
/// schema-mismatch guidance differs between the current tree and a
/// historical anchor blob.
fn parse_families_bytes_raw(
    bytes: &[u8],
    origin: &str,
    schema_note: &str,
) -> ConformanceResult<FamiliesFile> {
    let probe: SchemaProbe = serde_json::from_slice(bytes)
        .map_err(|err| format!("diag-families map at {origin} is not valid JSON: {err}"))?;
    if probe.schema != u64::from(FAMILIES_SCHEMA) {
        return Err(format!(
            "diag-families map at {origin} has schema {}; {schema_note}",
            probe.schema
        )
        .into());
    }
    serde_json::from_slice(bytes)
        .map_err(|err| format!("diag-families map at {origin} failed to parse: {err}").into())
}

pub(crate) fn parse_families_bytes(bytes: &[u8], origin: &str) -> ConformanceResult<FamiliesFile> {
    let file = parse_families_bytes_raw(
        bytes,
        origin,
        &format!("this tree implements schema {FAMILIES_SCHEMA}"),
    )?;
    validate_structure(&file)?;
    Ok(file)
}

/// Pure, git-free validation: structure, sorted/unique rows and
/// canaries, the 2XXX partition, status/anchor coherence, and (when
/// frozen) the freeze ⊕ extensions row composition.
fn validate_structure(file: &FamiliesFile) -> ConformanceResult<()> {
    if file.band_partition.family.is_empty() || file.band_partition.owner.is_empty() {
        return Err("diag-families band partition needs a family name and owner".into());
    }
    let mut names = BTreeSet::new();
    let mut owners_by_row = BTreeMap::<FamilyRow, &str>::new();
    for family in &file.families {
        if family.name.is_empty() || family.owner.is_empty() {
            return Err(format!(
                "diag-families family {:?} needs a non-empty name and owner",
                family.name
            )
            .into());
        }
        if family.name == file.band_partition.family {
            return Err(format!(
                "diag-families family {:?} collides with the band partition family",
                family.name
            )
            .into());
        }
        if !names.insert(family.name.as_str()) {
            return Err(format!("duplicate diag-families family {:?}", family.name).into());
        }
        for row in &family.rows {
            validate_row(row, &family.name)?;
            if let Some(previous) = owners_by_row.insert(*row, family.name.as_str()) {
                return Err(format!(
                    "duplicate diag-families row ({}, {}) in {:?} and {previous:?}; every \
                     corpus-exercised row has exactly one owner family",
                    row.code,
                    row.pass.name(),
                    family.name
                )
                .into());
            }
        }
        require_sorted_unique(&family.rows, || {
            format!("diag-families family {:?} rows", family.name)
        })?;
        require_sorted_unique(&family.canaries, || {
            format!("diag-families family {:?} canaries", family.name)
        })?;
        for canary in &family.canaries {
            if canary.fixture.is_empty() {
                return Err(format!(
                    "diag-families family {:?} canary with empty fixture",
                    family.name
                )
                .into());
            }
        }
    }
    match (file.status, &file.freeze) {
        (FamiliesStatus::Frozen, Some(freeze)) => {
            validate_anchor_commit(&freeze.adjudication_commit, "diag-families freeze")?;
            require_sorted_unique(&freeze.rows, || "diag-families freeze rows".to_owned())?;
            for extension in &file.universe_extensions {
                validate_anchor_commit(
                    &extension.adjudication_commit,
                    "diag-families universe extension",
                )?;
                require_sorted_unique(&extension.added, || {
                    "diag-families universe-extension added rows".to_owned()
                })?;
                if extension.added.is_empty() {
                    return Err(
                        "diag-families universe extension adds no rows; an extension exists \
                         only to assign new domain rows"
                            .into(),
                    );
                }
            }
            verify_frozen_row_composition(file)?;
        }
        (FamiliesStatus::Draft, None) => {
            if !file.universe_extensions.is_empty() {
                return Err(
                    "diag-families map is draft but carries universe extensions; extensions \
                     exist only against a frozen base"
                        .into(),
                );
            }
        }
        (FamiliesStatus::Frozen, None) => {
            return Err("diag-families map is frozen but has no freeze record".into());
        }
        (FamiliesStatus::Draft, Some(_)) => {
            return Err(
                "diag-families map is draft but carries a freeze record; record the freeze \
                 in the status change"
                    .into(),
            );
        }
    }
    Ok(())
}

fn validate_row(row: &FamilyRow, family: &str) -> ConformanceResult<()> {
    // Pass validity is a parse-time guarantee (the closed `Pass`
    // enum); only the band partition needs a semantic check.
    if TWO_XXX_CODES.contains(&row.code) {
        return Err(format!(
            "diag-families family {family:?} enumerates 2XXX row ({}, {}); codes 2000-2999 \
             belong wholesale to the band partition",
            row.code,
            row.pass.name()
        )
        .into());
    }
    Ok(())
}

fn require_sorted_unique<T: Ord>(items: &[T], what: impl Fn() -> String) -> ConformanceResult<()> {
    if items.windows(2).all(|pair| pair[0] < pair[1]) {
        Ok(())
    } else {
        Err(format!(
            "{} must be strictly sorted and unique for deterministic anchor comparison",
            what()
        )
        .into())
    }
}

fn enumerated_rows(file: &FamiliesFile) -> BTreeMap<FamilyRow, String> {
    let mut rows = BTreeMap::new();
    for family in &file.families {
        for row in &family.rows {
            rows.insert(*row, family.name.clone());
        }
    }
    rows
}

fn frozen_row(family: &str, row: &FamilyRow) -> FrozenRow {
    FrozenRow {
        family: family.to_owned(),
        code: row.code,
        pass: row.pass,
    }
}

/// Frozen maps only: current rows must equal the freeze enumeration ⊕
/// every extension's additions, identity-for-identity. An old owner
/// change disguised as an extension leaves a row missing from its
/// frozen family and fails here by name.
fn verify_frozen_row_composition(file: &FamiliesFile) -> ConformanceResult<()> {
    let freeze = file
        .freeze
        .as_ref()
        .expect("caller checks frozen status has a freeze record");
    let mut expected = BTreeSet::new();
    for row in &freeze.rows {
        if !expected.insert(row.clone()) {
            return Err(format!(
                "diag-families freeze enumerates ({}, {}) twice",
                row.code,
                row.pass.name()
            )
            .into());
        }
    }
    for extension in &file.universe_extensions {
        for row in &extension.added {
            if !expected.insert(row.clone()) {
                return Err(format!(
                    "diag-families universe extension re-adds frozen row ({}, {}) of {:?}",
                    row.code,
                    row.pass.name(),
                    row.family
                )
                .into());
            }
        }
    }
    let current: BTreeSet<FrozenRow> = enumerated_rows(file)
        .iter()
        .map(|(row, family)| frozen_row(family, row))
        .collect();
    if let Some(missing) = expected.difference(&current).next() {
        return Err(format!(
            "diag-families frozen row ({}, {}) of {:?} is missing from the current map; \
             old ownership is byte-stable after the freeze",
            missing.code,
            missing.pass.name(),
            missing.family
        )
        .into());
    }
    if let Some(extra) = current.difference(&expected).next() {
        return Err(format!(
            "diag-families row ({}, {}) of {:?} is neither in the freeze enumeration nor in \
             a universe extension; additions require an anchored extension record",
            extra.code,
            extra.pass.name(),
            extra.family
        )
        .into());
    }
    Ok(())
}

/// The corpus-exercised domain: for every golden case, T0 buckets over
/// ALL oracle records; each bucket must carry exactly one pass. The
/// non-2XXX `(code, pass)` set is the map's required domain; the 2XXX
/// bucket count is reported as the band partition census. For the
/// cases named in `retain_rows_for`, the per-case row set is kept so
/// canary anchoring (a rowed family's canary case must own at least
/// one bucket) can be verified without a checker run.
pub(crate) struct CorpusDomain {
    pub(crate) rows: BTreeSet<FamilyRow>,
    pub(crate) cases: BTreeSet<(String, String)>,
    pub(crate) retained_case_rows: BTreeMap<(String, String), BTreeSet<FamilyRow>>,
    pub(crate) two_xxx_buckets: usize,
    pub(crate) fixtures: usize,
}

pub(crate) fn corpus_domain(
    workspace: &Path,
    retain_rows_for: &BTreeSet<(String, String)>,
) -> ConformanceResult<CorpusDomain> {
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: workspace.to_path_buf(),
        limit: None,
        files: Vec::new(),
    })?;
    let goldens_root = workspace.join("goldens");
    let mut rows = BTreeSet::new();
    let mut cases = BTreeSet::new();
    let mut retained_case_rows = BTreeMap::new();
    let mut two_xxx_buckets = 0usize;
    for fixture in &fixtures {
        let fixture_key = fixture_key(workspace, fixture)?;
        let golden = read_golden(&goldens_root, &fixture_key)?;
        if golden.schema < 2 {
            return Err(format!(
                "golden {fixture_key} has schema {} without pass provenance; the family map \
                 is keyed by (code, pass)",
                golden.schema
            )
            .into());
        }
        for case in &golden.cases {
            let case_key = (fixture_key.clone(), case.matrix_key.clone());
            let mut case_rows = retain_rows_for
                .contains(&case_key)
                .then(BTreeSet::<FamilyRow>::new);
            cases.insert(case_key.clone());
            for (key, pass) in case_bucket_passes(&fixture_key, &case.matrix_key, &case.oracle)? {
                if TWO_XXX_CODES.contains(&key.code) {
                    two_xxx_buckets += 1;
                } else {
                    let row = FamilyRow {
                        code: key.code,
                        pass,
                    };
                    rows.insert(row);
                    if let Some(case_rows) = case_rows.as_mut() {
                        case_rows.insert(row);
                    }
                }
            }
            if let Some(case_rows) = case_rows {
                retained_case_rows.insert(case_key, case_rows);
            }
        }
    }
    Ok(CorpusDomain {
        rows,
        cases,
        retained_case_rows,
        two_xxx_buckets,
        fixtures: fixtures.len(),
    })
}

/// Group one case's oracle records into T0 buckets and attribute each
/// bucket to its single pass. A mixed-pass bucket has no unambiguous
/// owner row: it fails loudly and its handling is a reviewed
/// adjudication at the introducing universe transition, never a silent
/// tie-break.
fn case_bucket_passes(
    fixture: &str,
    matrix_key: &str,
    oracle: &[GoldenDiag],
) -> ConformanceResult<Vec<(crate::T0Key, Pass)>> {
    let mut passes = BTreeMap::<crate::T0Key, BTreeSet<Pass>>::new();
    for diag in oracle {
        let Some(pass) = diag.pass.as_deref().and_then(Pass::from_oracle) else {
            return Err(format!(
                "oracle record without recognized pass provenance in {fixture} \
                 [{matrix_key}] (code {}, pass {:?})",
                diag.code, diag.pass
            )
            .into());
        };
        passes.entry(t0_key(diag)).or_default().insert(pass);
    }
    let mut out = Vec::with_capacity(passes.len());
    for (key, bucket_passes) in passes {
        if bucket_passes.len() > 1 {
            let names = bucket_passes
                .iter()
                .map(|pass| pass.name())
                .collect::<Vec<_>>();
            return Err(format!(
                "mixed-pass T0 bucket in {fixture} [{matrix_key}]: code {} at {:?}:{:?}:{:?} \
                 arrives from passes {names:?}; adjudicate the bucket at its universe \
                 transition",
                key.code, key.file, key.line, key.col
            )
            .into());
        }
        let pass = *bucket_passes
            .iter()
            .next()
            .expect("bucket has at least one record");
        out.push((key, pass));
    }
    Ok(out)
}

/// Domain equality, both directions: an unmapped corpus row and a
/// mapped-but-unexercised row both fail by name.
fn verify_domain(
    map_rows: &BTreeMap<FamilyRow, String>,
    corpus_rows: &BTreeSet<FamilyRow>,
) -> ConformanceResult<()> {
    let mapped: BTreeSet<&FamilyRow> = map_rows.keys().collect();
    let exercised: BTreeSet<&FamilyRow> = corpus_rows.iter().collect();
    if let Some(missing) = exercised.difference(&mapped).next() {
        return Err(format!(
            "unmapped corpus row ({}, {}): every corpus-exercised non-2XXX (code, pass) row \
             needs exactly one owner family (a new row rides an anchored universe extension)",
            missing.code,
            missing.pass.name()
        )
        .into());
    }
    if let Some(stale) = mapped.difference(&exercised).next() {
        return Err(format!(
            "diag-families row ({}, {}) of {:?} is not exercised by the current corpus; a \
             domain shrink is a reviewed re-baseline event, not drift",
            stale.code,
            stale.pass.name(),
            map_rows[*stale]
        )
        .into());
    }
    Ok(())
}

fn families_file_at(
    root: &Path,
    commit: &str,
    rel: &str,
    origin: &str,
) -> ConformanceResult<FamiliesFile> {
    let bytes = git_blob_optional(root, commit, rel)?
        .ok_or_else(|| format!("no diag-families map at {origin}"))?;
    parse_families_bytes_historical(&bytes, origin)
}

/// Historical blobs (anchor targets, extension chain states) get the
/// schema guard and shape parse but not current-tree validation: an
/// extension's row-content commit legitimately precedes its own
/// record, so full composition only holds at the chain's end.
fn parse_families_bytes_historical(bytes: &[u8], origin: &str) -> ConformanceResult<FamiliesFile> {
    parse_families_bytes_raw(
        bytes,
        origin,
        "identities across schema versions are incomparable, so the anchor cannot verify",
    )
}

fn resolve_families_anchor(root: &Path, recorded: &str, what: &str) -> ConformanceResult<String> {
    let context = format!("diag-families {what}");
    validate_anchor_commit(recorded, &context)?;
    resolve_anchor(root, recorded, &context)
}

/// The reviewed snapshot anchor plus the extension chain
/// (measurement-integrity.md §1.2, §5):
///
/// 1. the freeze's adjudication commit is an ancestor of HEAD and the
///    map there is the complete reviewed draft — its enumerated rows
///    equal the freeze enumeration identity-for-identity;
/// 2. each extension's adjudication commit is an ancestor of HEAD
///    whose map is frozen with THIS freeze record byte-identical and
///    rows equal to the freeze ⊕ extensions up to and including it;
/// 3. the current families content equals the anchored base content
///    with every extension applied — so an owner, canary, or note
///    edit after the freeze fails even when the row set still adds up.
fn verify_freeze_anchors(
    root: &Path,
    rel: &str,
    head: &str,
    file: &FamiliesFile,
) -> ConformanceResult<()> {
    let freeze = file
        .freeze
        .as_ref()
        .expect("caller checks frozen status has a freeze record");
    let commit = resolve_families_anchor(root, &freeze.adjudication_commit, "freeze")?;
    if !is_ancestor(root, &commit, head)? {
        return Err(format!(
            "diag-families freeze adjudication commit {commit} is not an ancestor of HEAD; \
             a reviewed snapshot anchors on reachable history"
        )
        .into());
    }
    let adjudicated = families_file_at(
        root,
        &commit,
        rel,
        &format!("freeze adjudication commit {commit}"),
    )?;
    if adjudicated.status != FamiliesStatus::Draft
        || adjudicated.freeze.is_some()
        || !adjudicated.universe_extensions.is_empty()
    {
        return Err(format!(
            "diag-families map at freeze adjudication commit {commit} is not the reviewed \
             draft (status {}, freeze {}, extensions {}); the content lands while draft and \
             the follow-up change records the anchor",
            adjudicated.status.name(),
            if adjudicated.freeze.is_some() {
                "present"
            } else {
                "absent"
            },
            adjudicated.universe_extensions.len()
        )
        .into());
    }
    let adjudicated_rows: BTreeSet<FrozenRow> = enumerated_rows(&adjudicated)
        .iter()
        .map(|(row, family)| frozen_row(family, row))
        .collect();
    let frozen_rows: BTreeSet<FrozenRow> = freeze.rows.iter().cloned().collect();
    if adjudicated_rows != frozen_rows {
        let diff = adjudicated_rows
            .symmetric_difference(&frozen_rows)
            .next()
            .expect("unequal sets have a witness");
        return Err(format!(
            "diag-families freeze enumeration does not equal the map at its adjudication \
             commit {commit} (first difference: ({}, {}) of {:?}); an add-and-reanchor pair \
             cannot redefine the reviewed snapshot",
            diff.code,
            diff.pass.name(),
            diff.family
        )
        .into());
    }

    // Replay the extension chain over the adjudicated base.
    let mut expected = adjudicated.families.clone();
    let band_partition = adjudicated.band_partition.clone();
    for (index, extension) in file.universe_extensions.iter().enumerate() {
        let what = format!("universe extension {index}");
        let ext_commit = resolve_families_anchor(root, &extension.adjudication_commit, &what)?;
        if !is_ancestor(root, &ext_commit, head)? {
            return Err(format!(
                "diag-families {what} adjudication commit {ext_commit} is not an ancestor of \
                 HEAD"
            )
            .into());
        }
        apply_extension(&mut expected, extension, &what)?;
        let at_commit = families_file_at(
            root,
            &ext_commit,
            rel,
            &format!("{what} adjudication commit {ext_commit}"),
        )?;
        if at_commit.status != FamiliesStatus::Frozen || at_commit.freeze.as_ref() != Some(freeze) {
            return Err(format!(
                "diag-families map at {what} adjudication commit {ext_commit} does not carry \
                 the frozen base this record extends"
            )
            .into());
        }
        // The anchored commit's own recorded history must be exactly
        // the prior extensions: rows land first, THIS record lands in
        // the follow-up, and no other provenance may exist there —
        // otherwise the anchor-verified audit trail and the live map
        // could tell different extension histories.
        if at_commit.universe_extensions != file.universe_extensions[..index] {
            return Err(format!(
                "diag-families map at {what} adjudication commit {ext_commit} records a \
                 different extension history than the current chain's prefix; the anchored \
                 provenance and the live map must agree"
            )
            .into());
        }
        if at_commit.families != expected {
            return Err(format!(
                "diag-families {what} does not equal its content at adjudication commit \
                 {ext_commit}; extension rows land in one commit and the follow-up records \
                 that commit"
            )
            .into());
        }
        if at_commit.band_partition != band_partition {
            return Err(format!(
                "diag-families band partition changed at {what} adjudication commit \
                 {ext_commit}"
            )
            .into());
        }
    }
    if file.families != expected {
        let detail = first_family_difference(&expected, &file.families);
        return Err(format!(
            "diag-families content differs from the anchored base with every universe \
             extension applied ({detail}); owners, canaries, and notes are byte-stable after \
             the freeze"
        )
        .into());
    }
    if file.band_partition != band_partition {
        return Err(
            "diag-families band partition differs from the anchored base; the partition \
             claim is byte-stable after the freeze"
                .into(),
        );
    }
    Ok(())
}

/// Apply one extension: new families append in record order; added
/// rows insert into their (existing or new) family preserving sorted
/// order. Old rows are never moved or removed.
fn apply_extension(
    families: &mut Vec<Family>,
    extension: &UniverseExtension,
    what: &str,
) -> ConformanceResult<()> {
    for new_family in &extension.new_families {
        if families.iter().any(|family| family.name == new_family.name) {
            return Err(format!(
                "diag-families {what} re-introduces family {:?}",
                new_family.name
            )
            .into());
        }
        families.push(Family {
            name: new_family.name.clone(),
            owner: new_family.owner.clone(),
            note: new_family.note.clone(),
            rows: Vec::new(),
            canaries: new_family.canaries.clone(),
        });
    }
    for row in &extension.added {
        let family = families
            .iter_mut()
            .find(|family| family.name == row.family)
            .ok_or_else(|| {
                format!(
                    "diag-families {what} adds ({}, {}) to unknown family {:?}",
                    row.code,
                    row.pass.name(),
                    row.family
                )
            })?;
        let new_row = FamilyRow {
            code: row.code,
            pass: row.pass,
        };
        match family.rows.binary_search(&new_row) {
            Ok(_) => {
                return Err(format!(
                    "diag-families {what} re-adds existing row ({}, {}) of {:?}",
                    row.code,
                    row.pass.name(),
                    row.family
                )
                .into());
            }
            Err(position) => family.rows.insert(position, new_row),
        }
    }
    Ok(())
}

fn first_family_difference(expected: &[Family], current: &[Family]) -> String {
    for (index, expected_family) in expected.iter().enumerate() {
        match current.get(index) {
            None => return format!("family {:?} missing", expected_family.name),
            Some(current_family) if current_family != expected_family => {
                if current_family.name != expected_family.name {
                    return format!(
                        "family order: expected {:?}, found {:?}",
                        expected_family.name, current_family.name
                    );
                }
                if current_family.owner != expected_family.owner {
                    return format!("family {:?} owner changed", expected_family.name);
                }
                if current_family.canaries != expected_family.canaries {
                    return format!("family {:?} canaries changed", expected_family.name);
                }
                if current_family.rows != expected_family.rows {
                    return format!("family {:?} rows changed", expected_family.name);
                }
                return format!("family {:?} note changed", expected_family.name);
            }
            Some(_) => continue,
        }
    }
    format!("{} extra families", current.len() - expected.len())
}

/// Trusted-base comparison for hosted PR CI. The map's introduction PR
/// (no base artifact) is the one missing-base window and admits the
/// DRAFT only — a first freeze that rides it would be self-attested
/// against a same-branch commit, so it is rejected exactly like A2's
/// missing-base window rejects a frozen scope. Afterwards a frozen
/// base pins the freeze record, the extension prefix, and every
/// already-anchored family byte-for-byte.
fn verify_families_baseline(
    root: &Path,
    rel: &str,
    baseline: &str,
    current: &FamiliesFile,
) -> ConformanceResult<()> {
    let base_commit = resolve_commit(root, baseline)?;
    let Some(base_bytes) = git_blob_optional(root, &base_commit, rel)? else {
        // Introduction window: the trusted base predates the map.
        if current.status == FamiliesStatus::Frozen {
            return Err(
                "diag-families map is frozen but the trusted base has no map; the first \
                 freeze cannot ride the introduction PR — land the draft, then freeze \
                 against a reviewed base in its own change"
                    .into(),
            );
        }
        return Ok(());
    };
    let base = parse_families_bytes_historical(
        &base_bytes,
        &format!("baseline {baseline} ({base_commit})"),
    )?;
    match (base.status, current.status) {
        (FamiliesStatus::Frozen, FamiliesStatus::Draft) => {
            Err("diag-families status downgrade: baseline is frozen, candidate is draft".into())
        }
        (FamiliesStatus::Draft, FamiliesStatus::Draft) => Ok(()),
        (FamiliesStatus::Draft, FamiliesStatus::Frozen) => {
            // First freeze transition: exactly one fresh freeze record,
            // no extensions riding it.
            if !current.universe_extensions.is_empty() {
                return Err(
                    "diag-families first freeze cannot carry universe extensions; freeze \
                     the reviewed base first, extend in a later reviewed slice"
                        .into(),
                );
            }
            Ok(())
        }
        (FamiliesStatus::Frozen, FamiliesStatus::Frozen) => {
            // The base blob is historical and deliberately unvalidated
            // (parse_families_bytes_historical): a malformed frozen
            // base is an integrity error to report, not an invariant
            // to assert.
            let Some(base_freeze) = base.freeze.as_ref() else {
                return Err(format!(
                    "diag-families map at baseline {baseline} ({base_commit}) is frozen \
                     without a freeze record; the trusted base is malformed"
                )
                .into());
            };
            let current_freeze = current.freeze.as_ref().expect("validated frozen candidate");
            if base_freeze != current_freeze {
                return Err(
                    "diag-families freeze record differs from the trusted base; a branch \
                     add-and-reanchor pair cannot redefine the frozen snapshot"
                        .into(),
                );
            }
            if base.universe_extensions.len() > current.universe_extensions.len()
                || base
                    .universe_extensions
                    .iter()
                    .zip(current.universe_extensions.iter())
                    .any(|(base_ext, current_ext)| base_ext != current_ext)
            {
                return Err(
                    "diag-families universe extensions are append-only against the trusted \
                     base; an existing extension record never changes or disappears"
                        .into(),
                );
            }
            let mut expected = base.families.clone();
            for (index, extension) in current
                .universe_extensions
                .iter()
                .enumerate()
                .skip(base.universe_extensions.len())
            {
                apply_extension(
                    &mut expected,
                    extension,
                    &format!("universe extension {index}"),
                )?;
            }
            if current.families != expected {
                let detail = first_family_difference(&expected, &current.families);
                return Err(format!(
                    "diag-families content differs from the trusted base beyond appended \
                     universe extensions ({detail})"
                )
                .into());
            }
            if current.band_partition != base.band_partition {
                return Err("diag-families band partition differs from the trusted base".into());
            }
            Ok(())
        }
    }
}

/// Every canary must name a live golden case, and a rowed family's
/// canary case must contain at least one family-owned bucket: a
/// vacuous canary (drifted empty at a reviewed oracle refresh) would
/// otherwise keep "passing" while anchoring nothing, and the drift
/// belongs to the refresh slice as a reviewed re-baseline, not to a
/// later reader.
fn verify_canary_anchoring(file: &FamiliesFile, domain: &CorpusDomain) -> ConformanceResult<()> {
    for family in &file.families {
        let family_rows: BTreeSet<&FamilyRow> = family.rows.iter().collect();
        for canary in &family.canaries {
            let case_key = (canary.fixture.clone(), canary.matrix_key.clone());
            if !domain.cases.contains(&case_key) {
                return Err(format!(
                    "diag-families family {:?} canary {} [{}] names no golden case",
                    family.name, canary.fixture, canary.matrix_key
                )
                .into());
            }
            if family_rows.is_empty() {
                continue;
            }
            let case_rows = domain
                .retained_case_rows
                .get(&case_key)
                .expect("corpus_domain retains rows for every requested canary case");
            if !case_rows.iter().any(|row| family_rows.contains(row)) {
                return Err(format!(
                    "diag-families family {:?} canary {} [{}] is vacuous: the case contains \
                     no family-owned (code, pass) bucket, so it anchors nothing — re-anchor \
                     the canary in the slice that changed the goldens",
                    family.name, canary.fixture, canary.matrix_key
                )
                .into());
            }
        }
    }
    Ok(())
}

/// Anchor verification against the repository: shared by `check` and
/// the rollup preparation, so a working-tree edit of a frozen map can
/// never feed the report (owners/canaries/notes are pinned by the
/// anchored content, not only by the row composition).
fn verify_map_anchors(workspace: &Path, file: &FamiliesFile) -> ConformanceResult<()> {
    if file.status != FamiliesStatus::Frozen {
        return Ok(());
    }
    let root = git_root_for(workspace)?;
    let rel = git_rel_path(&root, workspace, FAMILIES_REL_PATH)?;
    let head = resolve_commit(&root, "HEAD")?;
    verify_freeze_anchors(&root, &rel, &head, file)
}

/// `cargo xtask families check`: structure, corpus-domain equality,
/// canary existence and anchoring, the freeze/extension anchors, and
/// the trusted-base comparison.
pub fn check(workspace: &Path, baseline: Option<&str>) -> ConformanceResult<()> {
    let path = workspace.join(FAMILIES_REL_PATH);
    let bytes =
        fs::read(&path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let file = parse_families_bytes(&bytes, FAMILIES_REL_PATH)?;
    let map_rows = enumerated_rows(&file);
    let canary_cases = file
        .families
        .iter()
        .flat_map(|family| family.canaries.iter())
        .map(|canary| (canary.fixture.clone(), canary.matrix_key.clone()))
        .collect::<BTreeSet<_>>();
    let domain = corpus_domain(workspace, &canary_cases)?;
    verify_domain(&map_rows, &domain.rows)?;
    verify_canary_anchoring(&file, &domain)?;

    verify_map_anchors(workspace, &file)?;
    if let Some(baseline) = baseline {
        let root = git_root_for(workspace)?;
        let rel = git_rel_path(&root, workspace, FAMILIES_REL_PATH)?;
        verify_families_baseline(&root, &rel, baseline, &file)?;
    }

    println!(
        "diag-families check: status={} families={} rows={} canaries={} corpus rows={} \
         2xxx buckets={} fixtures={}{}{}",
        file.status.name(),
        file.families.len(),
        map_rows.len(),
        file.families
            .iter()
            .map(|family| family.canaries.len())
            .sum::<usize>(),
        domain.rows.len(),
        domain.two_xxx_buckets,
        domain.fixtures,
        if file.status == FamiliesStatus::Frozen {
            format!(
                "; freeze anchor ok, extensions={}",
                file.universe_extensions.len()
            )
        } else {
            String::new()
        },
        match baseline {
            Some(baseline) => format!("; baseline {baseline} ok"),
            None => String::new(),
        },
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Rollup (families report)
// ---------------------------------------------------------------------------

/// One full band=all observation collected DURING the gating
/// conformance run — never reconstructed from A1 summaries. The
/// collecting run enforces the A1 set ratchet and FP=0 itself, so an
/// observation cannot exist on a regressed tree.
#[derive(Debug, Default)]
pub struct Observation {
    pub(crate) fixtures_total: usize,
    pub(crate) cases: Vec<CaseObservation>,
}

#[derive(Debug)]
pub(crate) struct CaseObservation {
    pub(crate) fixture: String,
    pub(crate) matrix_key: String,
    pub(crate) false_positives: usize,
    pub(crate) buckets: Vec<BucketObservation>,
}

#[derive(Debug)]
pub(crate) struct BucketObservation {
    pub(crate) code: u32,
    pub(crate) pass: Pass,
    pub(crate) oracle_multiplicity: usize,
    pub(crate) tsrs_multiplicity: usize,
    pub(crate) excluded_occurrences: usize,
    pub(crate) matched: bool,
}

impl BucketObservation {
    /// A bucket leaves the supported denominator only when every one
    /// of its occurrences is excluded; a partial exclusion keeps the
    /// surviving neighbors in scope (measurement-integrity.md §3).
    pub(crate) fn fully_excluded(&self) -> bool {
        self.excluded_occurrences == self.oracle_multiplicity
    }
}

impl CaseObservation {
    pub(crate) fn collect(
        fixture: &str,
        matrix_key: &str,
        oracle: &[GoldenDiag],
        tsrs_all: &[GoldenDiag],
        excluded_indices: &BTreeSet<usize>,
        matched_keys: &BTreeSet<crate::T0Key>,
        false_positives: usize,
    ) -> ConformanceResult<Self> {
        let mut excluded_by_key = BTreeMap::<crate::T0Key, usize>::new();
        for index in excluded_indices {
            *excluded_by_key.entry(t0_key(&oracle[*index])).or_default() += 1;
        }
        let mut oracle_mult = BTreeMap::<crate::T0Key, usize>::new();
        for diag in oracle {
            *oracle_mult.entry(t0_key(diag)).or_default() += 1;
        }
        let mut tsrs_mult = BTreeMap::<crate::T0Key, usize>::new();
        for diag in tsrs_all {
            *tsrs_mult.entry(t0_key(diag)).or_default() += 1;
        }
        let buckets = case_bucket_passes(fixture, matrix_key, oracle)?
            .into_iter()
            .map(|(key, pass)| BucketObservation {
                code: key.code,
                pass,
                oracle_multiplicity: oracle_mult[&key],
                tsrs_multiplicity: tsrs_mult.get(&key).copied().unwrap_or(0),
                excluded_occurrences: excluded_by_key.get(&key).copied().unwrap_or(0),
                matched: matched_keys.contains(&key),
            })
            .collect();
        Ok(Self {
            fixture: fixture.to_owned(),
            matrix_key: matrix_key.to_owned(),
            false_positives,
            buckets,
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RowGrade {
    pub(crate) total: usize,
    pub(crate) matched: usize,
    pub(crate) false_negative: usize,
    pub(crate) supported_total: usize,
    pub(crate) supported_matched: usize,
    pub(crate) supported_false_negative: usize,
}

impl RowGrade {
    fn add(&mut self, bucket: &BucketObservation) {
        self.total += 1;
        if bucket.matched {
            self.matched += 1;
        } else {
            self.false_negative += 1;
        }
        if !bucket.fully_excluded() {
            self.supported_total += 1;
            if bucket.matched {
                self.supported_matched += 1;
            } else {
                self.supported_false_negative += 1;
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RowReport {
    pub(crate) code: u32,
    pub(crate) pass: Pass,
    #[serde(flatten)]
    pub(crate) grade: RowGrade,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CanaryReport {
    pub(crate) fixture: String,
    pub(crate) matrix_key: String,
    /// Family-scoped: every family-owned bucket in the case matched at
    /// T0 (for a row-less family: the whole case, and no false
    /// positive). Duplicate buckets additionally report completeness.
    /// A vacuous canary (rowed family, no owned bucket in the case)
    /// never passes — `families check` rejects it outright; the flag
    /// here is defense in depth for the same condition.
    pub(crate) passed: bool,
    pub(crate) vacuous: bool,
    pub(crate) family_false_negative: usize,
    pub(crate) multiplicity_incomplete: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FamilyReport {
    pub(crate) name: String,
    pub(crate) owner: String,
    #[serde(flatten)]
    pub(crate) grade: RowGrade,
    pub(crate) canaries_passed: usize,
    pub(crate) canaries_total: usize,
    pub(crate) rows: Vec<RowReport>,
    pub(crate) canaries: Vec<CanaryReport>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct InputFingerprints {
    pub(crate) diag_families_sha256: String,
    pub(crate) m8_scope_sha256: String,
    pub(crate) oracle_inputs_sha256: String,
    pub(crate) conformance_matches_sha256: String,
    pub(crate) tsc_js_sha256: String,
    /// The executable that produced the observation — the tsrs checker
    /// is statically linked into it, so a checker rebuild (the input
    /// no file-side hash can see) moves this pin and staleness the
    /// other five fingerprints cannot express becomes visible.
    pub(crate) tsrs_exe_sha256: String,
}

impl InputFingerprints {
    fn current(workspace: &Path) -> ConformanceResult<Self> {
        let hash_file = |path: &Path| -> ConformanceResult<String> {
            let bytes = fs::read(path)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
            Ok(sha256_hex(&bytes))
        };
        let exe = std::env::current_exe()
            .map_err(|err| format!("failed to locate the running executable: {err}"))?;
        Ok(Self {
            diag_families_sha256: hash_file(&workspace.join(FAMILIES_REL_PATH))?,
            m8_scope_sha256: hash_file(&workspace.join(SCOPE_REL_PATH))?,
            oracle_inputs_sha256: hash_file(&workspace.join(ORACLE_INPUTS_REL_PATH))?,
            conformance_matches_sha256: hash_file(&workspace.join(MATCHES_REL_PATH))?,
            tsc_js_sha256: hash_file(&vendor_tsc_js_path(workspace))?,
            tsrs_exe_sha256: hash_file(&exe)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FamiliesReport {
    pub(crate) schema: u32,
    pub(crate) map_status: String,
    pub(crate) inputs: InputFingerprints,
    pub(crate) fixtures_total: usize,
    pub(crate) cases_total: usize,
    pub(crate) band_partition: BandPartitionReport,
    pub(crate) families: Vec<FamilyReport>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct BandPartitionReport {
    pub(crate) family: String,
    pub(crate) owner: String,
    #[serde(flatten)]
    pub(crate) grade: RowGrade,
}

/// Pure grading over the live observation: domain equality, per-row
/// and per-family supported grading, canary grading, and the band
/// partition census. A1 never appears as an input.
pub(crate) fn grade(
    file: &FamiliesFile,
    observation: &Observation,
    inputs: InputFingerprints,
) -> ConformanceResult<FamiliesReport> {
    let map_rows = enumerated_rows(file);
    let mut observed_rows = BTreeSet::new();
    let mut row_grades = BTreeMap::<FamilyRow, RowGrade>::new();
    let mut band_grade = RowGrade::default();
    let mut case_index = BTreeMap::<(String, String), &CaseObservation>::new();
    for case in &observation.cases {
        case_index.insert((case.fixture.clone(), case.matrix_key.clone()), case);
        for bucket in &case.buckets {
            if TWO_XXX_CODES.contains(&bucket.code) {
                band_grade.add(bucket);
                continue;
            }
            let row = FamilyRow {
                code: bucket.code,
                pass: bucket.pass,
            };
            observed_rows.insert(row);
            row_grades.entry(row).or_default().add(bucket);
        }
    }
    verify_domain(&map_rows, &observed_rows)?;

    let mut families = Vec::with_capacity(file.families.len());
    for family in &file.families {
        let family_rows: BTreeSet<&FamilyRow> = family.rows.iter().collect();
        let mut grade = RowGrade::default();
        let rows = family
            .rows
            .iter()
            .map(|row| {
                let row_grade = row_grades.get(row).cloned().unwrap_or_default();
                grade.total += row_grade.total;
                grade.matched += row_grade.matched;
                grade.false_negative += row_grade.false_negative;
                grade.supported_total += row_grade.supported_total;
                grade.supported_matched += row_grade.supported_matched;
                grade.supported_false_negative += row_grade.supported_false_negative;
                RowReport {
                    code: row.code,
                    pass: row.pass,
                    grade: row_grade,
                }
            })
            .collect::<Vec<_>>();
        let mut canaries = Vec::with_capacity(family.canaries.len());
        for canary in &family.canaries {
            let case = case_index
                .get(&(canary.fixture.clone(), canary.matrix_key.clone()))
                .ok_or_else(|| {
                    format!(
                        "diag-families family {:?} canary {} [{}] was not observed by the \
                         full conformance run",
                        family.name, canary.fixture, canary.matrix_key
                    )
                })?;
            let scoped = |bucket: &&BucketObservation| -> bool {
                family_rows.is_empty()
                    || family_rows.contains(&FamilyRow {
                        code: bucket.code,
                        pass: bucket.pass,
                    })
            };
            let scoped_buckets = case.buckets.iter().filter(scoped).count();
            let family_false_negative = case
                .buckets
                .iter()
                .filter(scoped)
                .filter(|bucket| !bucket.matched)
                .count();
            let multiplicity_incomplete = case
                .buckets
                .iter()
                .filter(scoped)
                .filter(|bucket| {
                    bucket.oracle_multiplicity > 1
                        && bucket.tsrs_multiplicity != bucket.oracle_multiplicity
                })
                .count();
            // A rowed family's canary anchors nothing when the case
            // owns no family bucket; it can never pass vacuously.
            let vacuous = !family_rows.is_empty() && scoped_buckets == 0;
            let passed = !vacuous
                && family_false_negative == 0
                && (!family_rows.is_empty() || case.false_positives == 0);
            canaries.push(CanaryReport {
                fixture: canary.fixture.clone(),
                matrix_key: canary.matrix_key.clone(),
                passed,
                vacuous,
                family_false_negative,
                multiplicity_incomplete,
            });
        }
        families.push(FamilyReport {
            name: family.name.clone(),
            owner: family.owner.clone(),
            grade,
            canaries_passed: canaries.iter().filter(|canary| canary.passed).count(),
            canaries_total: canaries.len(),
            rows,
            canaries,
        });
    }

    Ok(FamiliesReport {
        schema: 1,
        map_status: file.status.name().to_owned(),
        inputs,
        fixtures_total: observation.fixtures_total,
        cases_total: observation.cases.len(),
        band_partition: BandPartitionReport {
            family: file.band_partition.family.clone(),
            owner: file.band_partition.owner.clone(),
            grade: band_grade,
        },
        families,
    })
}

/// Everything the rollup needs BEFORE the observation run: the map
/// parsed, validated, and — when frozen — anchor-verified against the
/// repository (a working-tree owner/canary/note edit cannot feed the
/// report), plus the input fingerprints captured before the corpus is
/// touched.
#[derive(Debug)]
pub(crate) struct ReportPreparation {
    file: FamiliesFile,
    inputs: InputFingerprints,
}

pub(crate) fn prepare_report(workspace: &Path) -> ConformanceResult<ReportPreparation> {
    let path = workspace.join(FAMILIES_REL_PATH);
    let bytes =
        fs::read(&path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let file = parse_families_bytes(&bytes, FAMILIES_REL_PATH)?;
    verify_map_anchors(workspace, &file)?;
    let inputs = InputFingerprints::current(workspace)?;
    Ok(ReportPreparation { file, inputs })
}

/// Fingerprint stability across the observation run: the grading is
/// only as fresh as the inputs it was measured under, so an input
/// moving DURING the multi-second run invalidates the rollup instead
/// of binding new hashes to old numbers.
fn ensure_inputs_stable(
    before: &InputFingerprints,
    after: &InputFingerprints,
) -> ConformanceResult<()> {
    if before != after {
        return Err(
            "families rollup inputs changed while the observation ran; the grading no \
             longer corresponds to the recorded fingerprints — re-run `cargo xtask families \
             report` on a quiescent tree"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn finish_report(
    workspace: &Path,
    preparation: ReportPreparation,
    summary: &crate::ConformanceSummary,
    observation: &Observation,
    out_json: &Path,
) -> ConformanceResult<()> {
    let ReportPreparation { file, inputs } = preparation;
    ensure_inputs_stable(&inputs, &InputFingerprints::current(workspace)?)?;
    let report = grade(&file, observation, inputs)?;

    if let Some(parent) = out_json.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out_json, serde_json::to_string_pretty(&report)?)?;

    if file.status == FamiliesStatus::Draft {
        println!(
            "families report: map is DRAFT — the rollup is advisory until the reviewed freeze"
        );
    }
    println!(
        "families report: T0 {}/{} supported {}/{}; 2xxx partition {}/{}",
        summary.matched_t0_diagnostics,
        summary.oracle_diagnostics,
        summary.supported_matched_t0_diagnostics,
        summary.supported_oracle_diagnostics,
        report.band_partition.grade.matched,
        report.band_partition.grade.total,
    );
    for family in &report.families {
        println!(
            "  {:<28} {:<22} T0 {:>6}/{:<6} FN {:>6} supported FN {:>6} canaries {}/{}",
            family.name,
            family.owner,
            family.grade.matched,
            family.grade.total,
            family.grade.false_negative,
            family.grade.supported_false_negative,
            family.canaries_passed,
            family.canaries_total,
        );
    }
    println!("families report json: {}", out_json.display());
    Ok(())
}

/// `cargo xtask families report`: run the gating band=all conformance
/// observation, grade it against the anchor-verified map, and write
/// the rollup. In ci the same rollup rides the band=all conformance
/// step (`cargo xtask conformance --families-report`) so the corpus
/// is checked once.
pub fn report(workspace: &Path, out_json: &Path) -> ConformanceResult<()> {
    crate::run_conformance_with_families_report(
        &crate::ConformanceOptions {
            workspace: workspace.to_path_buf(),
            limit: None,
            files: Vec::new(),
            out_json: workspace.join("target/families/conformance.json"),
            band: crate::DiagnosticBand::All,
        },
        out_json,
    )
    .map(|_| ())
}

/// Consumer-side freshness check: a stored rollup is only meaningful
/// against the exact inputs it was produced from.
pub fn verify_report_freshness(workspace: &Path, report_path: &Path) -> ConformanceResult<()> {
    let bytes = fs::read(report_path)
        .map_err(|err| format!("failed to read {}: {err}", report_path.display()))?;
    let report: FamiliesReport = serde_json::from_slice(&bytes).map_err(|err| {
        format!(
            "families report {} failed to parse: {err}",
            report_path.display()
        )
    })?;
    let current = InputFingerprints::current(workspace)?;
    let pairs = [
        (
            "diag-families.json",
            &report.inputs.diag_families_sha256,
            &current.diag_families_sha256,
        ),
        (
            "m8-scope.json",
            &report.inputs.m8_scope_sha256,
            &current.m8_scope_sha256,
        ),
        (
            "ratchets/oracle-inputs.v1.json.zst",
            &report.inputs.oracle_inputs_sha256,
            &current.oracle_inputs_sha256,
        ),
        (
            "ratchets/conformance-matches.v1.json.zst",
            &report.inputs.conformance_matches_sha256,
            &current.conformance_matches_sha256,
        ),
        (
            "vendor _tsc.js",
            &report.inputs.tsc_js_sha256,
            &current.tsc_js_sha256,
        ),
        (
            "the tsrs checker binary",
            &report.inputs.tsrs_exe_sha256,
            &current.tsrs_exe_sha256,
        ),
    ];
    for (name, recorded, live) in pairs {
        if recorded != live {
            return Err(format!(
                "stale families report: {name} changed since the rollup was produced; \
                 re-run `cargo xtask families report`"
            )
            .into());
        }
    }
    // Coherence: the partition plus enumerated families cover the
    // whole observation exactly once.
    let family_total: usize = report
        .families
        .iter()
        .map(|family| family.grade.total)
        .sum();
    let row_total: usize = report
        .families
        .iter()
        .flat_map(|family| family.rows.iter())
        .map(|row| row.grade.total)
        .sum();
    if family_total != row_total {
        return Err("families report family totals do not equal their row totals".into());
    }
    println!(
        "families report fresh: {} families, {} + {} (2xxx) T0 buckets",
        report.families.len(),
        family_total,
        report.band_partition.grade.total
    );
    Ok(())
}

/// The observation is only collectable on a full band=all gating run:
/// a partial projection cannot supply the supported grading, and A1
/// summaries are never a substitute for it.
pub(crate) fn ensure_observation_eligible(
    band: crate::DiagnosticBand,
    full_run: bool,
) -> ConformanceResult<()> {
    if band != crate::DiagnosticBand::All || !full_run {
        return Err(format!(
            "families rollup requires a full band=all conformance run (got band={}, \
             full_run={full_run}); partial projections and A1 summaries cannot supply the \
             supported grading",
            band.name()
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::test_git::{git_test, init_repo, temp_dir};
    use crate::{GoldenMessageChain, T0Key};

    fn commit_families(root: &Path, file: &FamiliesFile, message: &str) -> String {
        fs::write(
            root.join(FAMILIES_REL_PATH),
            serde_json::to_vec_pretty(file).unwrap(),
        )
        .unwrap();
        git_test(root, &["add", FAMILIES_REL_PATH]);
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
}
