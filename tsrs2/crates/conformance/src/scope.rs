//! A2 exact scope state (measurement-integrity.md §3).
//!
//! The manifest (`m8-scope.json`, schema 2) enumerates the reviewed
//! out-of-scope oracle diagnostic occurrences by exact identity, plus
//! the anchors protecting them: draft band pins (reviewed snapshot
//! protocol), standing A1 tombstones for resolved exclusions, and the
//! one two-step global-freeze record. The selector removes exact
//! oracle records before the supported comparison; it can never
//! remove a T0 bucket another occurrence still demands, and syntactic
//! diagnostics are non-excludable. Schema 1 is rejected with a
//! migration message — it cannot freeze or satisfy readiness.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use super::{
    fixture_key, read_golden, select_fixtures, t0_key, t0_set, ConformanceResult, DiagnosticBand,
    GoldenCase, GoldenDiag, RefreshOptions, T0Key, TWO_XXX_CODES,
};
use crate::identity::{
    assign_case_identities, case_identity_report, CaseIdentityReport, ExactIdentity,
    ENCODER_VERSION,
};
use crate::ratchet::{self, git_blob_optional, git_rel_path, git_root_for, resolve_commit};

pub(crate) const SCOPE_REL_PATH: &str = "m8-scope.json";
const SCOPE_SCHEMA: u32 = 2;
/// Path of the committed cross-language encoder canaries, relative to
/// the workspace root.
const VECTORS_REL_PATH: &str = "crates/conformance/identity-vectors-v1.json";
/// measurement-integrity.md §3.3: the duplicate T0 buckets in the
/// adopted corpus are permanent canaries — (all bands, 2XXX). Only a
/// reviewed universe/correction transition may move these numbers,
/// and that slice updates the pin.
const DUP_BUCKET_CANARIES: (usize, usize) = (68, 65);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ScopeStatus {
    Draft,
    Frozen,
}

impl ScopeStatus {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Frozen => "frozen",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ScopeReason {
    HostResolution,
    JsdocSemantics,
    EmitDependent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScopeFile {
    schema: u32,
    /// Canonical encoder version the identities were computed under.
    /// Changing the encoding is A2's one reviewed schema extension.
    encoder: u32,
    status: ScopeStatus,
    #[serde(default)]
    exclusions: Vec<ScopeExclusion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    band_pins: Vec<BandPin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tombstones: Vec<Tombstone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    global: Option<GlobalFreeze>,
}

/// One reviewed exclusion: the exact schema-2 occurrence identity,
/// redundant review fields, and the adjudication evidence. Line and
/// column are verified against the pinned oracle record (they derive
/// from `start`); they are never identity.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct ScopeExclusion {
    #[serde(flatten)]
    identity: ExactIdentity,
    line: Option<u32>,
    col: Option<u32>,
    reason: ScopeReason,
    evidence: String,
}

/// A band-freeze record (measurement-integrity.md §3.1): the band,
/// the adjudication commit, and the complete enumerated identity set.
/// Count/hash are derived, never stored.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct BandPin {
    band: String,
    adjudication_commit: String,
    identities: Vec<ExactIdentity>,
}

/// A resolution tombstone (measurement-integrity.md §3.2): the exact
/// deleted identity and the resolving commit. Its standing proof is
/// A1 membership under the applicable full-corpus fixed view — unless
/// it is `lapsed`, in which case the proof obligation inverts: the
/// occurrence itself left the pinned goldens under a reviewed
/// universe/oracle-correction transition, so the identity must NOT
/// resolve anymore.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Tombstone {
    #[serde(flatten)]
    identity: ExactIdentity,
    /// The ancestor commit whose change resolved the occurrence.
    /// `None` only on a lapsed tombstone recording an occurrence a
    /// reviewed transition removed while it was still excluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolving_commit: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    lapsed: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The one global-freeze record (measurement-integrity.md §3.3),
/// added by the follow-up change of the two-step freeze.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct GlobalFreeze {
    adjudication_commit: String,
    identities: Vec<ExactIdentity>,
}

/// Code range selected by a band pin. Only bands with their own fixed
/// A1 view can be pinned — a tombstone under that pin must prove A1
/// membership in that view.
fn band_code_range(band: &str) -> ConformanceResult<std::ops::Range<u32>> {
    match band {
        "2xxx" => Ok(TWO_XXX_CODES),
        other => Err(format!(
            "unsupported scope pin band {other:?}: band pins are declared per fixed A1 view \
             and only \"2xxx\" has one (a new band needs its own declared view first)"
        )
        .into()),
    }
}

/// The A1 fixed view a band pin reads for tombstone standing proofs.
/// A hard error for unknown bands — never a fallback view, which would
/// silently prove a tombstone against the wrong accepted subset.
fn ratchet_view_for_band(band: &str) -> ConformanceResult<DiagnosticBand> {
    match band {
        "2xxx" => Ok(DiagnosticBand::TwoXxx),
        other => Err(format!("no fixed A1 view declared for scope pin band {other:?}").into()),
    }
}

/// Probe only the schema number, so retired/unknown schemas report
/// their migration path instead of a field-level parse error.
fn scope_schema_of(bytes: &[u8], origin: &str) -> ConformanceResult<u64> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|err| format!("failed to parse M8 scope manifest {origin}: {err}"))?;
    value
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("M8 scope manifest {origin} lacks a schema number").into())
}

/// Probe only the encoder pin, so a manifest written under another
/// encoder version reports its migration path instead of a parse
/// error (identities across encoder versions are incomparable).
fn scope_encoder_of(bytes: &[u8], origin: &str) -> ConformanceResult<u64> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|err| format!("failed to parse M8 scope manifest {origin}: {err}"))?;
    value
        .get("encoder")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| format!("M8 scope manifest {origin} lacks an encoder pin").into())
}

/// Probe only the status, for manifests that cannot fully parse under
/// this tree's encoder (the baseline side of an encoder migration).
fn scope_status_of(bytes: &[u8], origin: &str) -> ConformanceResult<ScopeStatus> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|err| format!("failed to parse M8 scope manifest {origin}: {err}"))?;
    match value.get("status").and_then(serde_json::Value::as_str) {
        Some("draft") => Ok(ScopeStatus::Draft),
        Some("frozen") => Ok(ScopeStatus::Frozen),
        other => Err(format!("M8 scope manifest {origin} has unknown status {other:?}").into()),
    }
}

fn parse_scope_bytes(bytes: &[u8], origin: &str) -> ConformanceResult<ScopeFile> {
    let schema = scope_schema_of(bytes, origin)?;
    if schema == 1 {
        return Err(format!(
            "M8 scope manifest {origin} uses retired schema 1: migrate every exclusion to an \
             exact schema-2 occurrence identity (measurement-integrity.md §3); schema 1 cannot \
             freeze or satisfy readiness"
        )
        .into());
    }
    if schema != u64::from(SCOPE_SCHEMA) {
        return Err(format!(
            "unsupported M8 scope schema {schema} in {origin} (expected {SCOPE_SCHEMA})"
        )
        .into());
    }
    let file: ScopeFile = serde_json::from_slice(bytes)
        .map_err(|err| format!("failed to parse M8 scope manifest {origin}: {err}"))?;
    if file.encoder != ENCODER_VERSION {
        return Err(format!(
            "M8 scope manifest {origin} pins canonical encoder v{} but this tree implements \
             v{ENCODER_VERSION}; changing the encoding is A2's one reviewed schema extension",
            file.encoder
        )
        .into());
    }
    validate_structure(&file, origin)?;
    Ok(file)
}

/// Pure structural validation: everything checkable without git or
/// goldens. Runs on every conformance load, so plain gating runs also
/// enforce pin/tombstone set discipline.
fn validate_structure(file: &ScopeFile, origin: &str) -> ConformanceResult<()> {
    let mut live = BTreeSet::new();
    for exclusion in &file.exclusions {
        let identity = &exclusion.identity;
        if identity.fixture.is_empty() {
            return Err(format!("M8 scope exclusion in {origin} has an empty fixture").into());
        }
        validate_identity_pass(identity, "exclusion")?;
        if exclusion.evidence.trim().is_empty() {
            return Err(format!("M8 scope exclusion {} has no evidence", identity.label()).into());
        }
        if !live.insert(identity.clone()) {
            return Err(format!("duplicate M8 scope exclusion {}", identity.label()).into());
        }
    }

    let mut tombstoned = BTreeSet::new();
    for tombstone in &file.tombstones {
        validate_identity_pass(&tombstone.identity, "tombstone")?;
        match &tombstone.resolving_commit {
            Some(commit) => {
                validate_anchor_commit(commit, "tombstone", &tombstone.identity.label())?;
            }
            None if !tombstone.lapsed => {
                return Err(format!(
                    "M8 scope tombstone {} has no resolving commit",
                    tombstone.identity.label()
                )
                .into());
            }
            None => {}
        }
        if live.contains(&tombstone.identity) {
            return Err(format!(
                "M8 scope tombstone {} is still a live exclusion; a tombstone records a \
                 proven deletion",
                tombstone.identity.label()
            )
            .into());
        }
        if !tombstoned.insert(tombstone.identity.clone()) {
            return Err(format!(
                "duplicate M8 scope tombstone {}",
                tombstone.identity.label()
            )
            .into());
        }
    }

    let mut bands = BTreeSet::new();
    for pin in &file.band_pins {
        if !bands.insert(pin.band.clone()) {
            return Err(format!("duplicate M8 scope band pin for {:?}", pin.band).into());
        }
        validate_anchor_commit(
            &pin.adjudication_commit,
            "band pin",
            &format!("{:?}", pin.band),
        )?;
        let band_range = band_code_range(&pin.band)?;
        let mut pinned = BTreeSet::new();
        for identity in &pin.identities {
            validate_identity_pass(identity, "band-pin")?;
            if !band_range.contains(&identity.code) {
                return Err(format!(
                    "M8 scope band pin {:?} enumerates out-of-band identity {}",
                    pin.band,
                    identity.label()
                )
                .into());
            }
            if !pinned.insert(identity.clone()) {
                return Err(format!(
                    "M8 scope band pin {:?} enumerates duplicate identity {}",
                    pin.band,
                    identity.label()
                )
                .into());
            }
        }
        // While the manifest is draft (and forever after freeze):
        // current identities in a pinned band must be members of the
        // pinned set, so additions and edits fail...
        for identity in &live {
            if band_range.contains(&identity.code) && !pinned.contains(identity) {
                return Err(format!(
                    "M8 scope exclusion {} is in pinned band {:?} but not in its pinned \
                     identity set (band additions/edits after the pin fail)",
                    identity.label(),
                    pin.band
                )
                .into());
            }
        }
        // ...and a pinned identity may disappear only with a tombstone.
        for identity in &pinned {
            if !live.contains(identity) && !tombstoned.contains(identity) {
                return Err(format!(
                    "pinned M8 scope identity {} (band {:?}) disappeared without a tombstone",
                    identity.label(),
                    pin.band
                )
                .into());
            }
        }
    }

    match (&file.status, &file.global) {
        (ScopeStatus::Frozen, None) => {
            return Err(format!(
                "M8 scope manifest {origin} is frozen without a global-freeze record"
            )
            .into());
        }
        (ScopeStatus::Draft, Some(_)) => {
            return Err(format!(
                "M8 scope manifest {origin} carries a global-freeze record while draft; the \
                 two-step freeze flips status in the same change that adds the record"
            )
            .into());
        }
        _ => {}
    }
    if let Some(global) = &file.global {
        validate_anchor_commit(&global.adjudication_commit, "global-freeze record", "")?;
        let mut frozen = BTreeSet::new();
        for identity in &global.identities {
            validate_identity_pass(identity, "global-freeze")?;
            if !frozen.insert(identity.clone()) {
                return Err(format!(
                    "M8 scope global-freeze record enumerates duplicate identity {}",
                    identity.label()
                )
                .into());
            }
        }
        // The global set never changes: live exclusions must be
        // members, and a frozen identity may leave only by tombstone.
        for identity in &live {
            if !frozen.contains(identity) {
                return Err(format!(
                    "M8 scope exclusion {} is not in the global-freeze set (additions and \
                     edits never occur after the freeze)",
                    identity.label()
                )
                .into());
            }
        }
        for identity in &frozen {
            if !live.contains(identity) && !tombstoned.contains(identity) {
                return Err(format!(
                    "globally frozen M8 scope identity {} disappeared without a tombstone",
                    identity.label()
                )
                .into());
            }
        }
    }
    Ok(())
}

/// Manifest anchors are full 40-hex commit SHAs. A movable ref
/// ("HEAD", a branch, a tag) would let the reviewed-snapshot compare
/// degenerate to self-compare the moment the ref moves.
fn validate_anchor_commit(commit: &str, what: &str, label: &str) -> ConformanceResult<()> {
    let full_hex = commit.len() == 40
        && commit
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    if !full_hex {
        return Err(format!(
            "M8 scope {what} {label} anchor {commit:?} is not a full 40-hex commit SHA; \
             movable refs (branches, tags, HEAD) cannot anchor a reviewed snapshot"
        )
        .into());
    }
    Ok(())
}

/// Resolve one manifest identity to its oracle record index: exactly
/// one occurrence must carry it. Zero matches is the stale error; two
/// or more is unreachable by construction (occurrence numbering makes
/// case identities unique), guarded so a canonical-encoder bug cannot
/// silently widen a selection or a proof.
fn resolve_identity_index(
    identities: &[ExactIdentity],
    identity: &ExactIdentity,
    what: &str,
) -> ConformanceResult<usize> {
    let mut matches = identities
        .iter()
        .enumerate()
        .filter(|(_, entry)| *entry == identity);
    let Some((record_index, _)) = matches.next() else {
        return Err(format!(
            "stale M8 scope {what} {}: no oracle occurrence carries this identity under \
             encoder v{ENCODER_VERSION}",
            identity.label()
        )
        .into());
    };
    let extra = matches.count();
    if extra > 0 {
        return Err(format!(
            "ambiguous M8 scope {what} {}: {} oracle occurrences share the identity \
             (canonical encoder bug)",
            identity.label(),
            extra + 1
        )
        .into());
    }
    Ok(record_index)
}

fn validate_identity_pass(identity: &ExactIdentity, what: &str) -> ConformanceResult<()> {
    match identity.pass.as_str() {
        "semantic" | "suggestion" => Ok(()),
        "syntactic" => Err(format!(
            "M8 scope {what} {} targets a syntactic diagnostic; parser fidelity is always \
             supported scope (syntactic diagnostics are non-excludable)",
            identity.label()
        )
        .into()),
        other => Err(format!(
            "M8 scope {what} {} has unknown pass {other:?}",
            identity.label()
        )
        .into()),
    }
}

pub(crate) struct ScopeManifest {
    file: ScopeFile,
    /// exclusion index by identity, for per-case selection.
    by_case: BTreeMap<(String, String), Vec<usize>>,
    seen: BTreeSet<ExactIdentity>,
}

impl ScopeManifest {
    pub(crate) fn load(path: &Path) -> ConformanceResult<Self> {
        let bytes = fs::read(path)
            .map_err(|err| format!("failed to read M8 scope manifest {}: {err}", path.display()))?;
        let file = parse_scope_bytes(&bytes, &path.display().to_string())?;
        let mut by_case: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        for (index, exclusion) in file.exclusions.iter().enumerate() {
            by_case
                .entry((
                    exclusion.identity.fixture.clone(),
                    exclusion.identity.matrix_key.clone(),
                ))
                .or_default()
                .push(index);
        }
        Ok(Self {
            file,
            by_case,
            seen: BTreeSet::new(),
        })
    }

    pub(crate) fn status(&self) -> ScopeStatus {
        self.file.status
    }

    pub(crate) fn entry_count(&self) -> usize {
        self.file.exclusions.len()
    }

    /// Select the exact oracle records this case excludes from the
    /// supported denominator, as indices into `oracle`. Each manifest
    /// entry must match exactly one occurrence — the identity's
    /// occurrence field disambiguates duplicate buckets — and its
    /// redundant line/column review fields must equal the pinned
    /// record's. A stale entry (occurrence no longer present) fails.
    pub(crate) fn exclusions_for_case(
        &mut self,
        fixture: &str,
        matrix_key: &str,
        oracle: &[GoldenDiag],
    ) -> ConformanceResult<BTreeSet<usize>> {
        let Some(indices) = self
            .by_case
            .get(&(fixture.to_owned(), matrix_key.to_owned()))
            .cloned()
        else {
            return Ok(BTreeSet::new());
        };
        let identities = assign_case_identities(fixture, matrix_key, oracle)?;
        let mut excluded = BTreeSet::new();
        for exclusion_index in indices {
            let exclusion = &self.file.exclusions[exclusion_index];
            let record_index =
                resolve_identity_index(&identities, &exclusion.identity, "exclusion")?;
            // No syntactic re-check here: an identity match implies the
            // record's pass equals the exclusion's, and syntactic
            // exclusions never load (validate_identity_pass).
            let record = &oracle[record_index];
            if exclusion.line != record.line || exclusion.col != record.col {
                return Err(format!(
                    "M8 scope exclusion {} review fields line={:?} col={:?} do not match the \
                     pinned oracle record's line={:?} col={:?} (they are derived from start \
                     and must agree)",
                    exclusion.identity.label(),
                    exclusion.line,
                    exclusion.col,
                    record.line,
                    record.col
                )
                .into());
            }
            self.seen.insert(exclusion.identity.clone());
            excluded.insert(record_index);
        }
        Ok(excluded)
    }

    pub(crate) fn finish_full_validation(&self) -> ConformanceResult<()> {
        let unseen = self
            .file
            .exclusions
            .iter()
            .filter(|exclusion| !self.seen.contains(&exclusion.identity))
            .collect::<Vec<_>>();
        if unseen.is_empty() {
            return Ok(());
        }
        let preview = unseen
            .iter()
            .take(5)
            .map(|exclusion| exclusion.identity.label())
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "M8 scope manifest contains {} exclusion(s) outside the full conformance corpus: \
             {preview}",
            unseen.len()
        )
        .into())
    }
}

/// The supported-view selector for one case: exact oracle records are
/// removed by index; a T0 bucket leaves the supported denominator
/// (`fully_excluded`) only when every one of its band records is
/// excluded — an exclusion can never remove a bucket another
/// occurrence still demands.
pub(crate) fn supported_case_view(
    oracle: &[GoldenDiag],
    band: DiagnosticBand,
    excluded_indices: &BTreeSet<usize>,
) -> (BTreeSet<T0Key>, BTreeSet<T0Key>) {
    let supported_expected = t0_set(
        oracle
            .iter()
            .enumerate()
            .filter(|(index, diag)| band.matches_oracle(diag) && !excluded_indices.contains(index))
            .map(|(_, diag)| diag),
    );
    let fully_excluded = excluded_indices
        .iter()
        .filter(|index| band.matches_oracle(&oracle[**index]))
        .map(|index| t0_key(&oracle[*index]))
        .filter(|key| !supported_expected.contains(key))
        .collect();
    (supported_expected, fully_excluded)
}

/// The resolution predicate (measurement-integrity.md §3.2) for one
/// excluded occurrence under the current run: a matched singleton
/// bucket, or a matched multiplicity-complete duplicate bucket, is
/// resolved. A duplicate bucket with unequal record counts cannot
/// prove WHICH occurrence resolved, so it stays unresolved.
pub(crate) fn occurrence_resolved(
    bucket_matched: bool,
    oracle_multiplicity: usize,
    tsrs_multiplicity: usize,
) -> bool {
    bucket_matched && (oracle_multiplicity == 1 || tsrs_multiplicity == oracle_multiplicity)
}

// ---------------------------------------------------------------------------
// `cargo xtask scope audit [--baseline <trusted-ref>]`
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct EncoderInput<'a> {
    encoder: u32,
    cases: Vec<EncoderInputCase<'a>>,
}

#[derive(Serialize)]
struct EncoderInputCase<'a> {
    fixture: &'a str,
    matrix_key: &'a str,
    records: &'a [GoldenDiag],
}

#[derive(Deserialize)]
struct EncoderOutput {
    encoder: u32,
    cases: Vec<CaseIdentityReport>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct VectorFile {
    pub(crate) encoder: u32,
    pub(crate) cases: Vec<VectorCase>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct VectorCase {
    pub(crate) name: String,
    pub(crate) fixture: String,
    pub(crate) matrix_key: String,
    pub(crate) records: Vec<GoldenDiag>,
}

/// One case selected for the cross-language check, with the golden
/// records it carries.
struct CrossCheckCase {
    label: String,
    fixture: String,
    matrix_key: String,
    records: Vec<GoldenDiag>,
}

/// What the corpus scan must establish for one manifest-referenced
/// identity.
#[derive(Clone, Copy, Eq, PartialEq)]
enum ReferenceNeed {
    /// Live selection: exactly one occurrence, review fields agree.
    Exclusion,
    /// Pin/global membership: exactly one occurrence, unless a lapsed
    /// tombstone records that a reviewed transition removed it.
    Anchored,
    /// Active tombstone: exactly one occurrence (its bucket feeds the
    /// A1 standing proof).
    Resolved,
    /// Lapsed tombstone: no occurrence — the identity must have left
    /// the pinned goldens.
    Lapsed,
}

/// One manifest reference for the corpus scan: the manifest section it
/// came from, what the scan must establish, and the identity.
type ScopeReference = (&'static str, ReferenceNeed, ExactIdentity);

/// The A2 scope audit: structural manifest validation, occurrence
/// resolution against the pinned goldens, the duplicate-bucket
/// canaries, the Node/Rust canonical-encoder cross-check, the
/// reviewed-snapshot anchors (band pins and the global freeze), the
/// standing A1 tombstone proofs, and the trusted-base compare.
pub fn audit(workspace: &Path, baseline: Option<&str>) -> ConformanceResult<()> {
    let path = workspace.join(SCOPE_REL_PATH);
    let bytes = fs::read(&path)
        .map_err(|err| format!("failed to read M8 scope manifest {}: {err}", path.display()))?;
    let file = parse_scope_bytes(&bytes, &path.display().to_string())?;

    // -- Corpus scan: occurrence resolution + duplicate-bucket census.
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
    })?;
    let goldens_root = workspace.join("goldens");

    // Identities the manifest references, grouped per (fixture, case).
    let mut referenced: BTreeMap<(String, String), Vec<ScopeReference>> = BTreeMap::new();
    let mut reference = |what: &'static str, need: ReferenceNeed, identity: &ExactIdentity| {
        referenced
            .entry((identity.fixture.clone(), identity.matrix_key.clone()))
            .or_default()
            .push((what, need, identity.clone()));
    };
    for exclusion in &file.exclusions {
        reference("exclusion", ReferenceNeed::Exclusion, &exclusion.identity);
    }
    for pin in &file.band_pins {
        for identity in &pin.identities {
            reference("band-pin identity", ReferenceNeed::Anchored, identity);
        }
    }
    for tombstone in &file.tombstones {
        let need = if tombstone.lapsed {
            ReferenceNeed::Lapsed
        } else {
            ReferenceNeed::Resolved
        };
        reference("tombstone", need, &tombstone.identity);
    }
    if let Some(global) = &file.global {
        for identity in &global.identities {
            reference("global-freeze identity", ReferenceNeed::Anchored, identity);
        }
    }
    let lapsed_identities = file
        .tombstones
        .iter()
        .filter(|tombstone| tombstone.lapsed)
        .map(|tombstone| tombstone.identity.clone())
        .collect::<BTreeSet<_>>();

    let mut dup_buckets = 0usize;
    let mut dup_buckets_2xxx = 0usize;
    let mut cross_cases = Vec::new();
    let mut resolved_case_ids = BTreeSet::new();
    // Per-case bucket multiplicities for every case an ACTIVE tombstone
    // references, per applicable fixed view (a lapsed tombstone has no
    // A1 proof to feed).
    let mut tombstone_cases: BTreeMap<(String, String), GoldenCase> = BTreeMap::new();
    let tombstone_case_keys = file
        .tombstones
        .iter()
        .filter(|tombstone| !tombstone.lapsed)
        .map(|tombstone| {
            (
                tombstone.identity.fixture.clone(),
                tombstone.identity.matrix_key.clone(),
            )
        })
        .collect::<BTreeSet<_>>();

    for fixture in &fixtures {
        let key = fixture_key(workspace, fixture)?;
        let golden = read_golden(&goldens_root, &key)
            .map_err(|err| format!("golden for {key} unreadable: {err}"))?;
        for case in &golden.cases {
            let mut buckets: BTreeMap<T0Key, usize> = BTreeMap::new();
            for diag in &case.oracle {
                *buckets.entry(t0_key(diag)).or_default() += 1;
            }
            let case_dups = buckets.values().filter(|count| **count >= 2).count();
            if case_dups > 0 {
                dup_buckets += case_dups;
                dup_buckets_2xxx += buckets
                    .iter()
                    .filter(|(bucket, count)| **count >= 2 && TWO_XXX_CODES.contains(&bucket.code))
                    .count();
                // Exercise the canary: occurrence assignment must
                // yield distinct identities across the whole case.
                let identities = assign_case_identities(&key, &case.matrix_key, &case.oracle)?;
                let unique = identities.iter().collect::<BTreeSet<_>>();
                if unique.len() != identities.len() {
                    return Err(format!(
                        "duplicate-bucket canary failed: {key} [{}] assigns {} identities \
                         but only {} are distinct",
                        case.matrix_key,
                        identities.len(),
                        unique.len()
                    )
                    .into());
                }
                cross_cases.push(CrossCheckCase {
                    label: format!("golden {key} [{}]", case.matrix_key),
                    fixture: key.clone(),
                    matrix_key: case.matrix_key.clone(),
                    records: case.oracle.clone(),
                });
            }
            let case_id = (key.clone(), case.matrix_key.clone());
            if let Some(entries) = referenced.get(&case_id) {
                resolve_referenced(&key, case, entries, &file, &lapsed_identities)?;
                resolved_case_ids.insert(case_id.clone());
            }
            if tombstone_case_keys.contains(&case_id) {
                tombstone_cases.insert(case_id, case.clone());
            }
        }
    }
    for (case_id, entries) in &referenced {
        if resolved_case_ids.contains(case_id) {
            continue;
        }
        for (what, need, identity) in entries {
            // A whole case may leave the corpus under a reviewed
            // universe transition: that is a lapse, so only entries
            // that must (or may, via a lapsed tombstone) be absent
            // tolerate the missing case.
            let tolerated = match need {
                ReferenceNeed::Lapsed => true,
                ReferenceNeed::Anchored => lapsed_identities.contains(identity),
                ReferenceNeed::Exclusion | ReferenceNeed::Resolved => false,
            };
            if !tolerated {
                return Err(format!(
                    "M8 scope {what} {} references fixture {} [{}] outside the pinned corpus",
                    identity.label(),
                    case_id.0,
                    case_id.1
                )
                .into());
            }
        }
    }
    if (dup_buckets, dup_buckets_2xxx) != DUP_BUCKET_CANARIES {
        return Err(format!(
            "duplicate-bucket canary drift: corpus has {dup_buckets} duplicate T0 bucket(s) \
             ({dup_buckets_2xxx} in 2XXX), pinned {}/{} (measurement-integrity.md §3.3; only \
             a reviewed universe/correction transition updates this pin)",
            DUP_BUCKET_CANARIES.0, DUP_BUCKET_CANARIES.1
        )
        .into());
    }

    // -- Cross-language canonical-encoder check (vectors + canaries).
    let vectors_path = workspace.join(VECTORS_REL_PATH);
    let vectors: VectorFile = serde_json::from_slice(
        &fs::read(&vectors_path)
            .map_err(|err| format!("failed to read {}: {err}", vectors_path.display()))?,
    )
    .map_err(|err| format!("failed to parse {}: {err}", vectors_path.display()))?;
    if vectors.encoder != ENCODER_VERSION {
        return Err(format!(
            "identity vector file pins encoder v{} but this tree implements v{ENCODER_VERSION}",
            vectors.encoder
        )
        .into());
    }
    for case in &vectors.cases {
        cross_cases.push(CrossCheckCase {
            label: format!("vector {}", case.name),
            fixture: case.fixture.clone(),
            matrix_key: case.matrix_key.clone(),
            records: case.records.clone(),
        });
    }
    verify_reorder_canaries(&vectors)?;
    let cross_checked = run_cross_language_check(workspace, &cross_cases)?;

    // -- Reviewed-snapshot anchors + tombstone standing proofs.
    let git_root = git_root_for(workspace)?;
    let scope_rel = git_rel_path(&git_root, workspace, SCOPE_REL_PATH)?;
    let head = resolve_commit(&git_root, "HEAD")?;

    for pin in &file.band_pins {
        verify_band_pin(&git_root, &scope_rel, &head, pin)?;
    }
    if let Some(global) = &file.global {
        verify_global_freeze(&git_root, &scope_rel, &head, &file, global)?;
    }
    if file.tombstones.iter().any(|tombstone| !tombstone.lapsed) {
        // The standing proof is invalid unless A1's vendor,
        // oracle-input, and comparator pins verify against the
        // current tree.
        let (matches, _, _, _) = ratchet::verify_current_pair(workspace)?;
        for tombstone in file.tombstones.iter().filter(|tombstone| !tombstone.lapsed) {
            verify_tombstone(
                &git_root,
                &head,
                &file,
                tombstone,
                &tombstone_cases,
                &matches,
            )?;
        }
    }
    // A lapsed tombstone's absence proof ran in the corpus scan; its
    // historical resolving commit (when one exists) must still anchor.
    for tombstone in file.tombstones.iter().filter(|tombstone| tombstone.lapsed) {
        verify_lapsed_tombstone_anchor(&git_root, &head, tombstone)?;
    }

    // -- Trusted-base compare.
    if let Some(baseline) = baseline {
        verify_scope_baseline(&git_root, &scope_rel, baseline, &file)?;
    }

    println!(
        "scope audit ok: status={} encoder=v{} exclusions={} band-pins={} tombstones={} \
         lapsed={} global={} dup-canaries={dup_buckets}/{dup_buckets_2xxx} \
         cross-checked={cross_checked} baseline={}",
        file.status.name(),
        ENCODER_VERSION,
        file.exclusions.len(),
        file.band_pins.len(),
        file.tombstones.len(),
        lapsed_identities.len(),
        if file.global.is_some() {
            "frozen"
        } else {
            "none"
        },
        baseline.unwrap_or("none"),
    );
    Ok(())
}

/// Resolve every manifest-referenced identity for one golden case
/// against its declared need: live exclusions and active tombstones
/// must denote exactly one oracle occurrence (exclusions additionally
/// verify their redundant review fields); pin/global members may
/// instead be covered by a lapsed tombstone; a lapsed tombstone's
/// occurrence must be gone.
fn resolve_referenced(
    fixture: &str,
    case: &GoldenCase,
    entries: &[ScopeReference],
    file: &ScopeFile,
    lapsed_identities: &BTreeSet<ExactIdentity>,
) -> ConformanceResult<()> {
    let identities = assign_case_identities(fixture, &case.matrix_key, &case.oracle)?;
    for (what, need, identity) in entries {
        match need {
            ReferenceNeed::Exclusion => {
                let record_index = resolve_identity_index(&identities, identity, what)?;
                let record = &case.oracle[record_index];
                let exclusion = file
                    .exclusions
                    .iter()
                    .find(|exclusion| exclusion.identity == *identity)
                    .expect("referenced exclusion exists");
                if exclusion.line != record.line || exclusion.col != record.col {
                    return Err(format!(
                        "M8 scope exclusion {} review fields line={:?} col={:?} do not match \
                         the pinned oracle record's line={:?} col={:?}",
                        identity.label(),
                        exclusion.line,
                        exclusion.col,
                        record.line,
                        record.col
                    )
                    .into());
                }
            }
            ReferenceNeed::Resolved => {
                resolve_identity_index(&identities, identity, what)?;
            }
            ReferenceNeed::Anchored => {
                // The lapsed tombstone's own entry proves the absence.
                if !lapsed_identities.contains(identity) {
                    resolve_identity_index(&identities, identity, what)?;
                }
            }
            ReferenceNeed::Lapsed => {
                if identities.iter().any(|entry| entry == identity) {
                    return Err(format!(
                        "M8 scope tombstone {} is marked lapsed but its occurrence still \
                         exists in the pinned goldens (a lapse records a reviewed \
                         transition that removed the occurrence)",
                        identity.label()
                    )
                    .into());
                }
            }
        }
    }
    Ok(())
}

/// Reorder canaries: an observable reorder must change the encoded
/// hash the reorder targets. Whole-identity inequality would be
/// vacuous here — occurrence numbering disambiguates even identical
/// tuples — so the hash comparison is the real check.
fn verify_reorder_canaries(vectors: &VectorFile) -> ConformanceResult<()> {
    for (name, hash_name) in [
        ("nested-chains-child-order", "chain_sha256"),
        ("reordered-related-information", "related_sha256"),
    ] {
        let case = vectors
            .cases
            .iter()
            .find(|case| case.name == name)
            .ok_or_else(|| format!("identity vector file lacks required canary {name:?}"))?;
        let identities = assign_case_identities(&case.fixture, &case.matrix_key, &case.records)?;
        let [first, second] = identities.as_slice() else {
            return Err(format!(
                "encoder canary {name:?} must hold exactly two records (one observable \
                 reorder), found {}",
                identities.len()
            )
            .into());
        };
        let hashes = if hash_name == "chain_sha256" {
            (&first.chain_sha256, &second.chain_sha256)
        } else {
            (&first.related_sha256, &second.related_sha256)
        };
        if hashes.0 == hashes.1 {
            return Err(format!(
                "encoder canary {name:?} failed: an observable reorder must change \
                 {hash_name}, but both records hash identically"
            )
            .into());
        }
    }
    Ok(())
}

/// Feed every selected case through `crates/oracle/identity.mjs` and
/// require byte-identical canonical output from both encoders.
fn run_cross_language_check(
    workspace: &Path,
    cases: &[CrossCheckCase],
) -> ConformanceResult<usize> {
    let rust_reports = cases
        .iter()
        .map(|case| case_identity_report(&case.fixture, &case.matrix_key, &case.records))
        .collect::<ConformanceResult<Vec<_>>>()?;

    let input = EncoderInput {
        encoder: ENCODER_VERSION,
        cases: cases
            .iter()
            .map(|case| EncoderInputCase {
                fixture: &case.fixture,
                matrix_key: &case.matrix_key,
                records: &case.records,
            })
            .collect(),
    };
    let script = workspace.join("crates/oracle/identity.mjs");
    let mut child = Command::new("node")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            format!(
                "failed to launch node for the A2 encoder cross-check ({}): {err}",
                script.display()
            )
        })?;
    // A node-side startup failure exits before draining stdin, which
    // surfaces here as a broken pipe. Absorb the write error until the
    // child is reaped, so the failure reports node's stderr instead of
    // a bare EPIPE.
    let mut stdin = child.stdin.take().expect("piped stdin");
    let write_result = stdin.write_all(&serde_json::to_vec(&input)?);
    drop(stdin);
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "node encoder cross-check failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    write_result.map_err(|err| format!("node encoder cross-check: stdin write failed: {err}"))?;
    let node: EncoderOutput = serde_json::from_slice(&output.stdout)
        .map_err(|err| format!("node encoder output unparsable: {err}"))?;
    if node.encoder != ENCODER_VERSION {
        return Err(format!(
            "node encoder implements v{} but this tree pins v{ENCODER_VERSION}",
            node.encoder
        )
        .into());
    }
    if node.cases.len() != rust_reports.len() {
        return Err(format!(
            "node encoder returned {} case report(s) for {} case(s)",
            node.cases.len(),
            rust_reports.len()
        )
        .into());
    }
    for ((case, rust), node) in cases.iter().zip(&rust_reports).zip(&node.cases) {
        compare_reports(&case.label, rust, node)?;
    }
    Ok(cases.len())
}

/// Byte-exact comparison of the two encoders' reports; the first
/// difference names the case, record, and field.
fn compare_reports(
    label: &str,
    rust: &CaseIdentityReport,
    node: &CaseIdentityReport,
) -> ConformanceResult<()> {
    fn compare_values(
        label: &str,
        what: &str,
        rust: &[String],
        node: &[String],
    ) -> ConformanceResult<()> {
        if rust.len() != node.len() {
            return Err(format!(
                "Node/Rust canonical encoders differ on {label}: {} {what} record(s) vs {}",
                rust.len(),
                node.len()
            )
            .into());
        }
        for (index, (ours, theirs)) in rust.iter().zip(node).enumerate() {
            if ours != theirs {
                return Err(format!(
                    "Node/Rust canonical encoders differ on {label}, record {index} {what}:\n  \
                     rust: {ours}\n  node: {theirs}"
                )
                .into());
            }
        }
        Ok(())
    }
    compare_values(
        label,
        "record canonical bytes",
        &rust.record_canonical,
        &node.record_canonical,
    )?;
    compare_values(
        label,
        "identity canonical bytes",
        &rust.identity_canonical,
        &node.identity_canonical,
    )?;
    compare_values(
        label,
        "identity sha256",
        &rust.identity_sha256,
        &node.identity_sha256,
    )?;
    if rust.identities != node.identities {
        return Err(format!(
            "Node/Rust canonical encoders differ on {label}: identity field values diverge \
             despite equal canonical bytes"
        )
        .into());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Git anchors (measurement-integrity.md §1.2, §3.1-§3.3)
// ---------------------------------------------------------------------------

/// Resolve a manifest anchor. The recorded value is already a full
/// 40-hex SHA (validate_structure); it must additionally name a commit
/// object directly — a 40-hex tag object would still peel somewhere
/// else, and an anchor that is not literally the commit would reopen
/// the movable-ref hole.
fn resolve_anchor(root: &Path, recorded: &str, what: &str) -> ConformanceResult<String> {
    let commit = resolve_commit(root, recorded)?;
    if commit != recorded {
        return Err(format!(
            "M8 scope {what} anchor {recorded} does not name a commit object directly \
             (it resolves to {commit}); anchors are full commit SHAs"
        )
        .into());
    }
    Ok(commit)
}

pub(crate) fn is_ancestor(
    root: &Path,
    ancestor: &str,
    descendant: &str,
) -> ConformanceResult<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .output()?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(format!(
            "git merge-base --is-ancestor {ancestor} {descendant} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into()),
    }
}

fn scope_file_at(
    root: &Path,
    commit: &str,
    rel: &str,
    origin: &str,
) -> ConformanceResult<ScopeFile> {
    let bytes = git_blob_optional(root, commit, rel)?
        .ok_or_else(|| format!("no M8 scope manifest at {origin}"))?;
    // A historical manifest written under another encoder version can
    // never satisfy this tree's anchor comparison — say so, instead of
    // failing as a parse error with no migration path.
    if scope_schema_of(&bytes, origin)? == u64::from(SCOPE_SCHEMA) {
        let encoder = scope_encoder_of(&bytes, origin)?;
        if encoder != u64::from(ENCODER_VERSION) {
            return Err(format!(
                "M8 scope manifest at {origin} was written under canonical encoder \
                 v{encoder} but this tree implements v{ENCODER_VERSION}; identities across \
                 encoder versions are incomparable, so the anchor cannot verify — re-anchor \
                 on a manifest re-encoded under the current encoder (the reviewed \
                 encoder-bump slice)"
            )
            .into());
        }
    }
    parse_scope_bytes(&bytes, origin)
}

/// §3.1 — the reviewed snapshot anchor for a draft band pin: the
/// adjudication commit is an ancestor of HEAD and the pinned set
/// equals the band subset of the manifest at that commit. The checker
/// compares identities, not only a self-hash, so an add-and-rewrite
/// of set/count/hash still fails.
fn verify_band_pin(
    root: &Path,
    scope_rel: &str,
    head: &str,
    pin: &BandPin,
) -> ConformanceResult<()> {
    let commit = resolve_anchor(
        root,
        &pin.adjudication_commit,
        &format!("band pin {:?}", pin.band),
    )?;
    if !is_ancestor(root, &commit, head)? {
        return Err(format!(
            "M8 scope band pin {:?} adjudication commit {} is not an ancestor of HEAD",
            pin.band, pin.adjudication_commit
        )
        .into());
    }
    let adjudicated = scope_file_at(
        root,
        &commit,
        scope_rel,
        &format!("band pin {:?} adjudication commit {commit}", pin.band),
    )?;
    if adjudicated.status != ScopeStatus::Draft {
        return Err(format!(
            "M8 scope band pin {:?} adjudication commit {commit} does not hold a draft \
             manifest; reviewed content lands while draft and the pin follows (reviewed \
             snapshot protocol)",
            pin.band
        )
        .into());
    }
    let band_range = band_code_range(&pin.band)?;
    let mut expected = BTreeSet::new();
    for exclusion in &adjudicated.exclusions {
        if band_range.contains(&exclusion.identity.code) {
            expected.insert(exclusion.identity.clone());
        }
    }
    let pinned = pin.identities.iter().cloned().collect::<BTreeSet<_>>();
    if pinned != expected {
        let missing = expected.difference(&pinned).collect::<Vec<_>>();
        let extra = pinned.difference(&expected).collect::<Vec<_>>();
        return Err(format!(
            "M8 scope band pin {:?} does not equal the band subset at its adjudication \
             commit {commit}: {} missing, {} extra{}{}",
            pin.band,
            missing.len(),
            extra.len(),
            missing
                .first()
                .map(|identity| format!("; first missing {}", identity.label()))
                .unwrap_or_default(),
            extra
                .first()
                .map(|identity| format!("; first extra {}", identity.label()))
                .unwrap_or_default(),
        )
        .into());
    }
    Ok(())
}

/// §3.3 — the global-freeze anchor: the adjudication commit is an
/// ancestor of HEAD, the manifest there was still draft (the reviewed
/// content landed first; the freeze record follows), and the frozen
/// set equals the complete live identity set at that commit.
fn verify_global_freeze(
    root: &Path,
    scope_rel: &str,
    head: &str,
    file: &ScopeFile,
    global: &GlobalFreeze,
) -> ConformanceResult<()> {
    debug_assert_eq!(file.status, ScopeStatus::Frozen);
    let commit = resolve_anchor(root, &global.adjudication_commit, "global-freeze record")?;
    if !is_ancestor(root, &commit, head)? {
        return Err(format!(
            "M8 scope global-freeze adjudication commit {} is not an ancestor of HEAD",
            global.adjudication_commit
        )
        .into());
    }
    let adjudicated = scope_file_at(
        root,
        &commit,
        scope_rel,
        &format!("global-freeze adjudication commit {commit}"),
    )?;
    if adjudicated.status != ScopeStatus::Draft {
        return Err(format!(
            "M8 scope global-freeze adjudication commit {commit} does not hold a draft \
             manifest; the two-step freeze reviews content first and records it second"
        )
        .into());
    }
    let expected = adjudicated
        .exclusions
        .iter()
        .map(|exclusion| exclusion.identity.clone())
        .collect::<BTreeSet<_>>();
    let frozen = global.identities.iter().cloned().collect::<BTreeSet<_>>();
    if frozen != expected {
        return Err(format!(
            "M8 scope global-freeze set does not equal the live identity set at its \
             adjudication commit {commit} ({} pinned vs {} adjudicated)",
            frozen.len(),
            expected.len()
        )
        .into());
    }
    Ok(())
}

/// §3.2 — the standing proof of an ACTIVE tombstone: the resolving
/// commit is an ancestor of HEAD, the identity still denotes a pinned
/// oracle occurrence, and A1 membership in every applicable
/// full-corpus fixed view proves the resolution (T0 membership for a
/// singleton bucket; a duplicate bucket must also be
/// multiplicity-complete). Lapsed tombstones prove the opposite in
/// the corpus scan and only anchor-check here.
fn verify_tombstone(
    root: &Path,
    head: &str,
    file: &ScopeFile,
    tombstone: &Tombstone,
    cases: &BTreeMap<(String, String), GoldenCase>,
    matches: &ratchet::MatchesArtifact,
) -> ConformanceResult<()> {
    let identity = &tombstone.identity;
    let recorded = tombstone.resolving_commit.as_deref().ok_or_else(|| {
        format!(
            "M8 scope tombstone {} is active but has no resolving commit",
            identity.label()
        )
    })?;
    let commit = resolve_anchor(root, recorded, &format!("tombstone {}", identity.label()))?;
    if !is_ancestor(root, &commit, head)? {
        return Err(format!(
            "M8 scope tombstone {} resolving commit {recorded} is not an ancestor of HEAD",
            identity.label(),
        )
        .into());
    }

    let case = cases
        .get(&(identity.fixture.clone(), identity.matrix_key.clone()))
        .ok_or_else(|| {
            format!(
                "M8 scope tombstone {} references a case outside the pinned corpus",
                identity.label()
            )
        })?;
    let identities = assign_case_identities(&identity.fixture, &identity.matrix_key, &case.oracle)?;
    let record_index = resolve_identity_index(&identities, identity, "tombstone")?;
    let bucket = t0_key(&case.oracle[record_index]);

    // The applicable full-corpus fixed views: the early band pin reads
    // its band view; the global freeze reads All; a tombstone under
    // neither still proves against All. Partial-fixture and supported
    // projections can never prove resolution — only the recorded
    // full-corpus views below exist in the artifact.
    let mut views = Vec::new();
    for pin in &file.band_pins {
        if pin.identities.contains(identity) {
            views.push(ratchet_view_for_band(&pin.band)?);
        }
    }
    if file
        .global
        .as_ref()
        .is_some_and(|global| global.identities.contains(identity))
    {
        views.push(DiagnosticBand::All);
    }
    if views.is_empty() {
        views.push(DiagnosticBand::All);
    }

    for view in views {
        let multiplicity = case
            .oracle
            .iter()
            .filter(|diag| view.matches_oracle(diag) && t0_key(diag) == bucket)
            .count();
        let sets = matches
            .views
            .get(view.name())
            .and_then(|view_sets| view_sets.get(&identity.fixture))
            .and_then(|fixture_sets| fixture_sets.get(&identity.matrix_key));
        let matched = sets.is_some_and(|sets| sets.matched.contains(&bucket));
        if !matched {
            return Err(format!(
                "M8 scope tombstone {} lacks its standing proof: T0 bucket \
                 {:?}/{}:{:?}:{:?} is not an accepted match in A1's {} view",
                identity.label(),
                bucket.file,
                bucket.code,
                bucket.line,
                bucket.col,
                view.name()
            )
            .into());
        }
        if multiplicity >= 2 {
            let complete = sets.is_some_and(|sets| sets.multiplicity_complete.contains(&bucket));
            if !complete {
                return Err(format!(
                    "M8 scope tombstone {} lacks its standing proof: its duplicate T0 bucket \
                     (multiplicity {multiplicity}) is not multiplicity-complete in A1's {} \
                     view, so a match cannot prove which occurrence resolved",
                    identity.label(),
                    view.name()
                )
                .into());
            }
        }
    }
    Ok(())
}

/// §3.2 — a lapsed tombstone's occurrence left the pinned goldens (the
/// corpus scan proved the absence); the historical resolving commit,
/// when the tombstone had resolved before the lapse, must still be a
/// real ancestor anchor.
fn verify_lapsed_tombstone_anchor(
    root: &Path,
    head: &str,
    tombstone: &Tombstone,
) -> ConformanceResult<()> {
    let Some(recorded) = tombstone.resolving_commit.as_deref() else {
        return Ok(());
    };
    let identity = &tombstone.identity;
    let commit = resolve_anchor(root, recorded, &format!("tombstone {}", identity.label()))?;
    if !is_ancestor(root, &commit, head)? {
        return Err(format!(
            "M8 scope tombstone {} resolving commit {recorded} is not an ancestor of HEAD",
            identity.label(),
        )
        .into());
    }
    Ok(())
}

/// The trusted-base compare (hosted PR CI passes the immutable PR
/// base). After the first valid freeze transition the global records
/// must be byte-identical; band pins never reanchor against the base;
/// a frozen base forbids downgrade and any live-set growth.
fn verify_scope_baseline(
    root: &Path,
    scope_rel: &str,
    baseline: &str,
    head: &ScopeFile,
) -> ConformanceResult<()> {
    let commit = resolve_commit(root, baseline)?;
    let Some(bytes) = git_blob_optional(root, &commit, scope_rel)? else {
        // Pre-A2 base (no manifest yet). The freeze cannot ride this:
        // the first transition requires a schema-2 draft base.
        if head.status == ScopeStatus::Frozen {
            return Err(format!(
                "baseline {baseline} has no M8 scope manifest; the first freeze transition \
                 requires a schema-2 draft trusted base"
            )
            .into());
        }
        return Ok(());
    };
    // A schema-1 base is the migration window: this slice may replace
    // it wholesale, but the freeze cannot ride the migration.
    if scope_schema_of(&bytes, &format!("baseline {baseline}"))? == 1 {
        if head.status == ScopeStatus::Frozen {
            return Err(format!(
                "baseline {baseline} holds the retired schema-1 manifest; the freeze \
                 transition requires a schema-2 draft trusted base"
            )
            .into());
        }
        return Ok(());
    }
    // An older-encoder base is the encoder-bump migration window:
    // identities across encoder versions are incomparable, so the
    // reviewed bump slice re-encodes the manifest wholesale and
    // re-anchors its pins. The freeze cannot ride the migration, and a
    // frozen base has no sanctioned bump path — the frozen set pins
    // its encoder.
    let base_encoder = scope_encoder_of(&bytes, &format!("baseline {baseline}"))?;
    if base_encoder != u64::from(ENCODER_VERSION) {
        if base_encoder > u64::from(ENCODER_VERSION) {
            return Err(format!(
                "baseline {baseline} pins canonical encoder v{base_encoder} but this tree \
                 implements v{ENCODER_VERSION}: an encoder downgrade never occurs"
            )
            .into());
        }
        if scope_status_of(&bytes, &format!("baseline {baseline}"))? == ScopeStatus::Frozen {
            return Err(format!(
                "baseline {baseline} holds a frozen manifest under canonical encoder \
                 v{base_encoder}; an encoder bump after the global freeze has no sanctioned \
                 path"
            )
            .into());
        }
        if head.status == ScopeStatus::Frozen {
            return Err(format!(
                "baseline {baseline} pins canonical encoder v{base_encoder}; the freeze \
                 transition cannot ride the encoder migration"
            )
            .into());
        }
        return Ok(());
    }
    let base = parse_scope_bytes(&bytes, &format!("baseline {baseline}"))?;

    match (base.status, head.status) {
        (ScopeStatus::Frozen, ScopeStatus::Draft) => {
            return Err(format!(
                "baseline {baseline} scope compare failed: status downgrade from frozen to \
                 draft never occurs"
            )
            .into());
        }
        (ScopeStatus::Frozen, ScopeStatus::Frozen) => {
            // After the first valid transition the global records are
            // byte-identical: an add-and-reanchor pair of branch
            // commits cannot redefine the frozen set.
            if serde_json::to_vec(&base.global)? != serde_json::to_vec(&head.global)? {
                return Err(format!(
                    "baseline {baseline} scope compare failed: the global-freeze record \
                     changed against the trusted base (adjudication commit and identity set \
                     are byte-identical after the first valid transition)"
                )
                .into());
            }
            if serde_json::to_vec(&base.band_pins)? != serde_json::to_vec(&head.band_pins)? {
                return Err(format!(
                    "baseline {baseline} scope compare failed: band pins changed after the \
                     global freeze"
                )
                .into());
            }
            let base_live = base
                .exclusions
                .iter()
                .map(|exclusion| exclusion.identity.clone())
                .collect::<BTreeSet<_>>();
            for exclusion in &head.exclusions {
                if !base_live.contains(&exclusion.identity) {
                    return Err(format!(
                        "baseline {baseline} scope compare failed: frozen exclusion {} does \
                         not exist at the trusted base (additions and edits never occur \
                         after the freeze)",
                        exclusion.identity.label()
                    )
                    .into());
                }
            }
        }
        (ScopeStatus::Draft, ScopeStatus::Frozen) => {
            // The first valid transition: the trusted base is draft
            // and the candidate carries exactly one valid global
            // record — schema shape (one optional record) plus the
            // anchor checks in verify_global_freeze.
        }
        (ScopeStatus::Draft, ScopeStatus::Draft) => {}
    }

    // Band pins never mutate against the trusted base; new reviewed
    // pins may land, existing ones are byte-stable.
    let head_pins: BTreeMap<&str, &BandPin> = head
        .band_pins
        .iter()
        .map(|pin| (pin.band.as_str(), pin))
        .collect();
    for base_pin in &base.band_pins {
        match head_pins.get(base_pin.band.as_str()) {
            None => {
                return Err(format!(
                    "baseline {baseline} scope compare failed: band pin {:?} was removed",
                    base_pin.band
                )
                .into());
            }
            Some(head_pin) if *head_pin != base_pin => {
                return Err(format!(
                    "baseline {baseline} scope compare failed: band pin {:?} changed against \
                     the trusted base (a re-baseline is an explicit reviewed event, not a \
                     branch edit)",
                    base_pin.band
                )
                .into());
            }
            Some(_) => {}
        }
    }

    // Tombstones are proofs of record: they never disappear against
    // the trusted base, and their provenance never changes — the
    // lapsed flag is the one field a reviewed transition may flip
    // (its truth is re-proven against the goldens on every audit).
    let head_tombstones: BTreeMap<&ExactIdentity, &Tombstone> = head
        .tombstones
        .iter()
        .map(|tombstone| (&tombstone.identity, tombstone))
        .collect();
    for base_tombstone in &base.tombstones {
        let Some(head_tombstone) = head_tombstones.get(&base_tombstone.identity) else {
            return Err(format!(
                "baseline {baseline} scope compare failed: tombstone {} was removed",
                base_tombstone.identity.label()
            )
            .into());
        };
        // A lapsed-only record (no commit) may gain its resolving
        // commit if the occurrence returns and resolves; a recorded
        // commit never changes or disappears.
        let commit_stable = match (
            &base_tombstone.resolving_commit,
            &head_tombstone.resolving_commit,
        ) {
            (Some(base_commit), Some(head_commit)) => base_commit == head_commit,
            (Some(_), None) => false,
            (None, _) => true,
        };
        if !commit_stable {
            return Err(format!(
                "baseline {baseline} scope compare failed: tombstone {} resolving commit \
                 changed against the trusted base (provenance is part of the proof of \
                 record)",
                base_tombstone.identity.label()
            )
            .into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Required adversarial tests (measurement-integrity.md §7, A2 rows:
// identity / pin / tombstone / global) and their positive companions.
// The "stale A1 pin" tombstone leg is enforced by wiring: the audit
// calls ratchet::verify_current_pair (whose failure classes carry
// their own A1 §7 tests) before accepting any tombstone proof.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::identity::assign_case_identities;
    use crate::ratchet::{git, CaseSets, MatchesArtifact, MatchesInputs, RunSets};
    use crate::GoldenMessageChain;

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tsrs2-scope-{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git_test(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args([
                "-c",
                "user.name=tsrs",
                "-c",
                "user.email=tsrs@test",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo(name: &str) -> PathBuf {
        let dir = temp_dir(name);
        git_test(&dir, &["init", "-q", "-b", "main"]);
        dir
    }

    fn commit_scope(root: &Path, file: &ScopeFile, message: &str) -> String {
        fs::write(
            root.join(SCOPE_REL_PATH),
            serde_json::to_vec_pretty(file).unwrap(),
        )
        .unwrap();
        git_test(root, &["add", SCOPE_REL_PATH]);
        git_test(root, &["commit", "-q", "-m", message]);
        String::from_utf8(git(root, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_owned()
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

        let (supported, fully_excluded) =
            supported_case_view(&oracle, DiagnosticBand::All, &excluded);
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
        let (supported, fully_excluded) =
            supported_case_view(&oracle, DiagnosticBand::All, &excluded);
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
        let vectors: VectorFile =
            serde_json::from_str(include_str!("../identity-vectors-v1.json")).unwrap();
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
        let error = resolve_anchor(&root, &commit[..12], "band pin \"2xxx\"")
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("does not name a commit object directly"),
            "{error}"
        );
        assert_eq!(
            resolve_anchor(&root, &commit, "band pin \"2xxx\"").unwrap(),
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
        let error = verify_lapsed_tombstone_anchor(
            &root,
            &head,
            &lapsed_tombstone_of(identity, Some(&side)),
        )
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
}
