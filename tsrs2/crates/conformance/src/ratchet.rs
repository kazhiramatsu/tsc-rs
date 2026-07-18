//! A1 set-monotone conformance state (measurement-integrity.md §2).
//!
//! Correctness progress is a growing SET of proven matches, never an
//! integer that can trade one correct diagnostic for another. Two
//! versioned artifacts live under `ratchets/`:
//!
//! - `oracle-inputs.v1.json.zst` — the immutable oracle-input
//!   manifest: fixture bytes, per-case matrix expansion and oracle
//!   records (as canonical-serialization SHA-256 pins), the vendored
//!   `_tsc.js` pin, and one comparator entry per tier where inactive
//!   tiers carry an explicit `"absent"` marker. It contains no tsrs
//!   output and no accepted-tsrs baseline.
//! - `conformance-matches.v1.json.zst` — the accepted state: per
//!   fixture/matrix matched T0 bucket identities plus
//!   multiplicity-complete buckets for the fixed All/2XXX/syntactic
//!   views. Multiplicity-complete is ratcheted separately because a
//!   2/2 bucket can regress to 2/1 while its T0 key stays matched.
//!
//! Both artifacts use the §1.1 append-only lineage anchor: every
//! version records `previous = {commit, sha256}`, the checker walks
//! every committed version of the path back to the unique oldest
//! `bootstrap` version, and at each edge requires the immediate-
//! predecessor pointer, the exact predecessor bytes, protected-content
//! monotonicity, and equal input pins outside a declared transition.
//! Hosted PR CI additionally compares HEAD against the trusted PR-base
//! artifact (`ratchet check --baseline <ref>`) so a rewritten branch
//! cannot manufacture a smaller self-consistent chain.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml_edit::{value as toml_value, Item, Table};

use super::{
    fixture_key, read_golden, read_ratchet_section, select_fixtures, t0_key, ConformanceOptions,
    ConformanceResult, DiagnosticBand, GoldenDiag, RefreshOptions, T0Key,
};

pub const MATCHES_REL_PATH: &str = "ratchets/conformance-matches.v1.json.zst";
pub const ORACLE_INPUTS_REL_PATH: &str = "ratchets/oracle-inputs.v1.json.zst";

const MATCHES_SCHEMA: u32 = 1;
const ORACLE_INPUTS_SCHEMA: u32 = 1;
/// The golden comparator schema T0 grades against (GoldenFile.schema
/// with pass provenance; schema 1 lacks it and cannot feed the
/// syntactic view).
const T0_COMPARATOR_SCHEMA: u32 = 2;
/// Reviewed input transition: enumerated corpus growth where every
/// old identity and byte stays unchanged. The A3
/// `input-schema-extension` is taught by its own slice, and A2's
/// stays reserved: exact scope identities derive live from the pinned
/// golden record bytes (encoder v1, `crate::identity`), so landing A2
/// touched no manifest byte — the extension activates only if the
/// canonical encoding itself ever changes. An unknown transition name
/// always fails the walk.
const UNIVERSE_TRANSITION: &str = "universe-transition";
/// Reviewed one-time input transition that ADDS the producer pins
/// (generator + normalization modules and the Node launch contract)
/// to a manifest that predates them. Detection-only: every other
/// input byte must stay unchanged.
const PRODUCER_PIN_EXTENSION: &str = "producer-pin-extension";
/// Reviewed correction epoch: the oracle producer was wrong (or its
/// fix predates most goldens), so pinned oracle RECORDS change for
/// the SAME fixtures under the same vendor. Fixture bytes, matrix
/// expansion, and the corpus itself stay byte-identical; totals are
/// remeasured; and every accepted identity the corrected truth
/// invalidates must be enumerated in the paired accepted-match
/// version's `lapsed` sets — the one sanctioned exception to
/// append-only growth, exact to the identity.
const ORACLE_CORRECTION: &str = "oracle-correction";

/// The fixed recorded views (measurement-integrity.md §2). A
/// supported fixed intersection added later needs its own declared
/// view; exact A2 scope is deliberately NOT a ratchet view.
pub(crate) const FIXED_VIEWS: [DiagnosticBand; 3] = [
    DiagnosticBand::All,
    DiagnosticBand::TwoXxx,
    DiagnosticBand::Syntactic,
];

/// One case's accepted/current identity sets for a single view.
/// `multiplicity_complete` ⊆ `matched` (a complete bucket has equal
/// nonzero oracle/tsrs record counts, so its key is matched).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaseSets {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub matched: BTreeSet<T0Key>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub multiplicity_complete: BTreeSet<T0Key>,
}

/// fixture key → matrix key → case sets (empty cases are omitted).
pub type ViewSets = BTreeMap<String, BTreeMap<String, CaseSets>>;
/// view name ("all" | "2xxx" | "syntactic") → view sets.
pub type RunSets = BTreeMap<String, ViewSets>;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Lineage {
    pub commit: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MatchesInputs {
    /// SHA-256 of the sibling oracle-inputs artifact FILE bytes the
    /// accepted sets were measured against. A2 §3.2: an accepted match
    /// is proof only while its input pins verify.
    pub oracle_inputs_sha256: String,
    pub tsc_js_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MatchesArtifact {
    pub schema: u32,
    #[serde(default)]
    pub bootstrap: bool,
    #[serde(default)]
    pub previous: Option<Lineage>,
    #[serde(default)]
    pub transition: Option<String>,
    pub inputs: MatchesInputs,
    pub views: RunSets,
    /// Present exactly when `transition == "oracle-correction"`: the
    /// complete enumerated identities (per view, matched and
    /// multiplicity-complete separately — never pooled) that lapsed
    /// under the corrected oracle. The lineage edge requires the
    /// actual removals to equal this set identity-for-identity; the
    /// trusted-base compare accepts a removal only when a correction
    /// version between base and head enumerates it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lapsed: Option<RunSets>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VendorPins {
    pub tsc_js_sha256: String,
    /// Combined pin over the vendored `lib.*.d.ts` inputs (sorted
    /// name+bytes): the lib texts are program inputs, so silent lib
    /// edits would change what the pinned oracle records mean.
    pub lib_sha256: String,
}

/// The oracle PRODUCER pins: exactly the generator + normalization
/// modules whose bytes determine golden oracle records, plus the Node
/// launch contract. Deliberately this narrow — the other
/// `crates/oracle/*.mjs` tools (ast/symbol/token dumps) never touch
/// goldens, and an overbroad producer pin would invalidate the
/// manifest on unrelated tooling churn.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProducerPins {
    /// `crates/oracle/driver.mjs` — serialization/normalization of
    /// oracle diagnostics into golden records.
    pub driver_sha256: String,
    /// `crates/oracle/program-host.mjs` — program construction,
    /// option decoding, and file-name normalization.
    pub program_host_sha256: String,
    /// `vendor/typescript-6.0.3/lib/typescript.js` — the compiler
    /// bundle the driver actually executes (the vendored `_tsc.js`
    /// pin identifies the vendor snapshot, not the executed module).
    pub typescript_js_sha256: String,
    /// Required Node version for oracle launches that write goldens
    /// (normalized, no leading `v`), sourced from the workspace
    /// `.node-version`. `oracle-refresh` verifies the LAUNCHED
    /// driver's `process.version` against the tree pin — the file
    /// alone is a declaration, not enforcement.
    pub node_version: String,
}

/// A tier's comparator entry. Inactive tiers must carry the explicit
/// `"absent"` marker — they never silently inherit an active
/// comparator (measurement-integrity.md §2).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ComparatorEntry {
    Active { schema: u32 },
    Marker(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CasePins {
    /// SHA-256 of the canonical serialization of the golden case's
    /// oracle records (every diagnostic record, order-preserving).
    pub oracle_sha256: String,
    /// SHA-256 of the expanded ProgramJson (`to_json()` — the exact
    /// bytes the oracle host consumed): pins matrix expansion,
    /// options, libs, and the file split.
    pub program_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FixturePins {
    pub fixture_sha256: String,
    pub cases: BTreeMap<String, CasePins>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OracleInputsArtifact {
    pub schema: u32,
    #[serde(default)]
    pub bootstrap: bool,
    #[serde(default)]
    pub previous: Option<Lineage>,
    #[serde(default)]
    pub transition: Option<String>,
    pub vendor: VendorPins,
    /// Producer pins. `None` only on historical pre-extension
    /// versions; the current tree always carries `Some` (the
    /// `producer-pin-extension` transition is one-time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer: Option<ProducerPins>,
    pub comparators: BTreeMap<String, ComparatorEntry>,
    pub fixtures: BTreeMap<String, FixturePins>,
    /// Derived coherence field (never the authority): oracle T0
    /// bucket totals per fixed view, recomputed from goldens on every
    /// `ratchet check`.
    pub totals: BTreeMap<String, u64>,
}

impl MatchesArtifact {
    fn validate(&self) -> ConformanceResult<()> {
        if self.schema != MATCHES_SCHEMA {
            return Err(format!(
                "accepted-match artifact schema {} unsupported (expected {MATCHES_SCHEMA})",
                self.schema
            )
            .into());
        }
        validate_lineage_fields(
            "accepted-match artifact",
            self.bootstrap,
            &self.previous,
            &self.transition,
        )?;
        let view_names: BTreeSet<&str> = self.views.keys().map(String::as_str).collect();
        let fixed: BTreeSet<&str> = FIXED_VIEWS.iter().map(|view| view.name()).collect();
        if view_names != fixed {
            return Err(format!(
                "accepted-match artifact must record exactly the fixed views {fixed:?}, found {view_names:?}"
            )
            .into());
        }
        for (view, fixtures) in &self.views {
            for (fixture, cases) in fixtures {
                for (matrix, sets) in cases {
                    if !sets.multiplicity_complete.is_subset(&sets.matched) {
                        return Err(format!(
                            "accepted-match artifact incoherent: {view} {fixture} [{matrix}] has a multiplicity-complete bucket outside the matched set"
                        )
                        .into());
                    }
                }
            }
        }
        match (&self.transition, &self.lapsed) {
            (Some(transition), Some(_)) if transition == ORACLE_CORRECTION => {}
            (_, Some(_)) => {
                return Err(format!(
                    "accepted-match artifact records lapsed identities without an \
                     {ORACLE_CORRECTION:?} transition"
                )
                .into());
            }
            (Some(transition), None) if transition == ORACLE_CORRECTION => {
                return Err(format!(
                    "accepted-match {ORACLE_CORRECTION:?} version lacks its lapsed enumeration \
                     (an empty correction records empty sets, never an absent field)"
                )
                .into());
            }
            (_, None) => {}
        }
        if let Some(lapsed) = &self.lapsed {
            let view_names: BTreeSet<&str> = lapsed.keys().map(String::as_str).collect();
            let fixed: BTreeSet<&str> = FIXED_VIEWS.iter().map(|view| view.name()).collect();
            if view_names != fixed {
                return Err(format!(
                    "lapsed enumeration must record exactly the fixed views {fixed:?}, found {view_names:?}"
                )
                .into());
            }
            // A lapsed identity is one the current state no longer
            // holds; an identity in both places is incoherent. The
            // tiers are checked separately: a 2/2 -> 2/1 correction
            // lapses only the multiplicity-complete membership while
            // the matched key legitimately stays.
            for (view, fixtures) in lapsed {
                let current_view = self.views.get(view).expect("fixed views verified above");
                for (fixture, cases) in fixtures {
                    for (matrix, sets) in cases {
                        let current = current_view
                            .get(fixture)
                            .and_then(|cases| cases.get(matrix));
                        let Some(current) = current else { continue };
                        if let Some(key) = sets.matched.intersection(&current.matched).next() {
                            return Err(format!(
                                "lapsed identity is still accepted: matched ({view}): {fixture} [{matrix}] {}",
                                t0_label(key)
                            )
                            .into());
                        }
                        if let Some(key) = sets
                            .multiplicity_complete
                            .intersection(&current.multiplicity_complete)
                            .next()
                        {
                            return Err(format!(
                                "lapsed identity is still accepted: multiplicity-complete ({view}): {fixture} [{matrix}] {}",
                                t0_label(key)
                            )
                            .into());
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl OracleInputsArtifact {
    fn validate(&self) -> ConformanceResult<()> {
        if self.schema != ORACLE_INPUTS_SCHEMA {
            return Err(format!(
                "oracle-inputs artifact schema {} unsupported (expected {ORACLE_INPUTS_SCHEMA})",
                self.schema
            )
            .into());
        }
        validate_lineage_fields(
            "oracle-inputs artifact",
            self.bootstrap,
            &self.previous,
            &self.transition,
        )?;
        for tier in ["t0", "t1", "t2", "t3", "t4"] {
            match (tier, self.comparators.get(tier)) {
                ("t0", Some(ComparatorEntry::Active { schema })) => {
                    if *schema != T0_COMPARATOR_SCHEMA {
                        return Err(format!(
                            "oracle-inputs t0 comparator schema {schema} unsupported (expected {T0_COMPARATOR_SCHEMA})"
                        )
                        .into());
                    }
                }
                ("t0", entry) => {
                    return Err(format!(
                        "oracle-inputs t0 comparator must be active, found {entry:?}"
                    )
                    .into());
                }
                (_, Some(ComparatorEntry::Marker(marker))) if marker == "absent" => {}
                // Tier activation (T1-T3 comparators, A3's T4) lands
                // through its declared input-schema-extension, which
                // teaches this validator the activated shape.
                (_, entry) => {
                    return Err(format!(
                        "oracle-inputs inactive tier {tier} lacks its explicit \"absent\" marker (found {entry:?})"
                    )
                    .into());
                }
            }
        }
        if let Some(extra) = self
            .comparators
            .keys()
            .find(|key| !["t0", "t1", "t2", "t3", "t4"].contains(&key.as_str()))
        {
            return Err(format!("oracle-inputs has an undeclared comparator entry {extra}").into());
        }
        if let Some(producer) = &self.producer {
            for (label, value) in [
                ("driver_sha256", &producer.driver_sha256),
                ("program_host_sha256", &producer.program_host_sha256),
                ("typescript_js_sha256", &producer.typescript_js_sha256),
                ("node_version", &producer.node_version),
            ] {
                if value.is_empty() {
                    return Err(
                        format!("oracle-inputs producer pin {label} is present but empty").into(),
                    );
                }
            }
        }
        let totals: BTreeSet<&str> = self.totals.keys().map(String::as_str).collect();
        let fixed: BTreeSet<&str> = FIXED_VIEWS.iter().map(|view| view.name()).collect();
        if totals != fixed {
            return Err(format!(
                "oracle-inputs totals must cover exactly the fixed views {fixed:?}, found {totals:?}"
            )
            .into());
        }
        Ok(())
    }

    /// Content identity, ignoring the lineage envelope (bootstrap /
    /// previous / transition).
    fn content_eq(&self, other: &Self) -> bool {
        self.vendor == other.vendor
            && self.producer == other.producer
            && self.comparators == other.comparators
            && self.fixtures == other.fixtures
            && self.totals == other.totals
    }
}

fn validate_lineage_fields(
    what: &str,
    bootstrap: bool,
    previous: &Option<Lineage>,
    transition: &Option<String>,
) -> ConformanceResult<()> {
    if bootstrap && transition.is_some() {
        return Err(format!("{what}: a bootstrap version cannot record a transition").into());
    }
    match (bootstrap, previous) {
        (true, Some(_)) => {
            Err(format!("{what}: a bootstrap version cannot record a previous version").into())
        }
        (false, None) => {
            Err(format!("{what}: a non-bootstrap version must record its previous version").into())
        }
        _ => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// Canonical bytes, hashing, io
// ---------------------------------------------------------------------------

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn encode_artifact<T: Serialize>(value: &T) -> ConformanceResult<Vec<u8>> {
    // Compact JSON over BTree collections: canonical bytes for a given
    // content + zstd version. Byte identity across versions is never
    // ASSUMED — lineage hashes always pin the bytes actually
    // committed, read back through git.
    let json = serde_json::to_vec(value)?;
    Ok(zstd::stream::encode_all(json.as_slice(), 3)?)
}

pub(crate) fn decode_artifact<T: DeserializeOwned>(
    bytes: &[u8],
    what: &str,
) -> ConformanceResult<T> {
    let json = zstd::stream::decode_all(bytes).map_err(|err| format!("{what}: {err}"))?;
    serde_json::from_slice(&json).map_err(|err| format!("{what}: {err}").into())
}

fn read_artifact<T: DeserializeOwned>(path: &Path, what: &str) -> ConformanceResult<(T, Vec<u8>)> {
    let bytes = fs::read(path).map_err(|err| {
        format!(
            "{what} missing at {} ({err}) — bootstrap with `cargo xtask ratchet update`",
            path.display()
        )
    })?;
    let value = decode_artifact(&bytes, what)?;
    Ok((value, bytes))
}

fn read_optional_bytes(path: &Path, what: &str) -> ConformanceResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("failed to read {what} at {}: {err}", path.display()).into()),
    }
}

// ---------------------------------------------------------------------------
// Current-run set computation (shared with lib.rs's conformance loop)
// ---------------------------------------------------------------------------

/// Bucket both sides of one case/view by T0 key. `matched` = key
/// present on both sides (set semantics, the T0 metric); a bucket is
/// `multiplicity_complete` when the oracle and tsrs record counts at
/// the key are EQUAL after the view's fixed predicate — the separate
/// ratchet that catches a 2/2 bucket regressing to 2/1 while its T0
/// key stays matched.
pub(crate) fn bucket_sets<'a>(
    oracle: impl Iterator<Item = &'a GoldenDiag>,
    tsrs: impl Iterator<Item = &'a GoldenDiag>,
) -> CaseSets {
    let mut oracle_counts: BTreeMap<T0Key, usize> = BTreeMap::new();
    for diag in oracle {
        *oracle_counts.entry(t0_key(diag)).or_default() += 1;
    }
    let mut tsrs_counts: BTreeMap<T0Key, usize> = BTreeMap::new();
    for diag in tsrs {
        *tsrs_counts.entry(t0_key(diag)).or_default() += 1;
    }
    let mut sets = CaseSets::default();
    for (key, oracle_count) in &oracle_counts {
        let Some(tsrs_count) = tsrs_counts.get(key) else {
            continue;
        };
        sets.matched.insert(key.clone());
        if tsrs_count == oracle_count {
            sets.multiplicity_complete.insert(key.clone());
        }
    }
    sets
}

fn t0_label(key: &T0Key) -> String {
    format!(
        "{}:{}:{} code {}",
        key.file.as_deref().unwrap_or("<none>"),
        key.line
            .map_or_else(|| "-".to_owned(), |line| line.to_string()),
        key.col
            .map_or_else(|| "-".to_owned(), |col| col.to_string()),
        key.code
    )
}

/// Identities present in `older` but missing from `newer`, as
/// structured per-view/fixture/matrix sets — the exact shape a
/// correction's `lapsed` enumeration must equal. Empty cases are
/// omitted; every view key of `older` is kept (so a stored lapsed
/// enumeration always carries exactly the fixed views).
fn collect_removal_sets(older: &RunSets, newer: &RunSets) -> RunSets {
    let empty_view = ViewSets::new();
    let empty_cases = BTreeMap::new();
    let empty_sets = CaseSets::default();
    let mut removals: RunSets = older
        .keys()
        .map(|view| (view.clone(), ViewSets::new()))
        .collect();
    for (view, older_fixtures) in older {
        let newer_fixtures = newer.get(view).unwrap_or(&empty_view);
        let removal_view = removals.get_mut(view).expect("seeded above");
        for (fixture, older_cases) in older_fixtures {
            let newer_cases = newer_fixtures.get(fixture).unwrap_or(&empty_cases);
            for (matrix, older_sets) in older_cases {
                let newer_sets = newer_cases.get(matrix).unwrap_or(&empty_sets);
                let matched: BTreeSet<T0Key> = older_sets
                    .matched
                    .difference(&newer_sets.matched)
                    .cloned()
                    .collect();
                let multiplicity_complete: BTreeSet<T0Key> = older_sets
                    .multiplicity_complete
                    .difference(&newer_sets.multiplicity_complete)
                    .cloned()
                    .collect();
                if matched.is_empty() && multiplicity_complete.is_empty() {
                    continue;
                }
                removal_view.entry(fixture.clone()).or_default().insert(
                    matrix.clone(),
                    CaseSets {
                        matched,
                        multiplicity_complete,
                    },
                );
            }
        }
    }
    removals
}

/// Human labels for structured removals: view/fixture/matrix/key and
/// which of the two protected sets lost the identity.
fn removal_labels(removals: &RunSets) -> Vec<String> {
    let mut labels = Vec::new();
    for (view, fixtures) in removals {
        for (fixture, cases) in fixtures {
            for (matrix, sets) in cases {
                for key in &sets.matched {
                    labels.push(format!(
                        "matched ({view}): {fixture} [{matrix}] {}",
                        t0_label(key)
                    ));
                }
                for key in &sets.multiplicity_complete {
                    labels.push(format!(
                        "multiplicity-complete ({view}): {fixture} [{matrix}] {}",
                        t0_label(key)
                    ));
                }
            }
        }
    }
    labels
}

/// Identities present in `older` but missing from `newer` — the
/// removals every gate rejects.
fn collect_set_removals(older: &RunSets, newer: &RunSets) -> Vec<String> {
    removal_labels(&collect_removal_sets(older, newer))
}

/// Merge `other` into `acc` (set union per view/fixture/matrix).
fn merge_run_sets(acc: &mut RunSets, other: &RunSets) {
    for (view, fixtures) in other {
        let acc_view = acc.entry(view.clone()).or_default();
        for (fixture, cases) in fixtures {
            let acc_cases = acc_view.entry(fixture.clone()).or_default();
            for (matrix, sets) in cases {
                let acc_sets = acc_cases.entry(matrix.clone()).or_default();
                acc_sets.matched.extend(sets.matched.iter().cloned());
                acc_sets
                    .multiplicity_complete
                    .extend(sets.multiplicity_complete.iter().cloned());
            }
        }
    }
}

fn removals_error(context: &str, removals: Vec<String>) -> ConformanceResult<()> {
    if removals.is_empty() {
        return Ok(());
    }
    let shown = removals.iter().take(8).cloned().collect::<Vec<_>>();
    Err(format!(
        "{context}: {} accepted identit{} regressed:\n  {}{}",
        removals.len(),
        if removals.len() == 1 { "y" } else { "ies" },
        shown.join("\n  "),
        if removals.len() > shown.len() {
            format!("\n  ... and {} more", removals.len() - shown.len())
        } else {
            String::new()
        }
    )
    .into())
}

/// The state a gating conformance run enforces against, with the
/// cheap per-run pin checks already applied (full inputs-vs-tree
/// verification is `ratchet check`'s job).
pub(crate) struct AcceptedState {
    pub(crate) artifact: MatchesArtifact,
}

pub(crate) fn load_accepted_for_gating(workspace: &Path) -> ConformanceResult<AcceptedState> {
    let (artifact, _bytes): (MatchesArtifact, _) =
        read_artifact(&workspace.join(MATCHES_REL_PATH), "accepted-match artifact")?;
    artifact.validate()?;
    let inputs_bytes = fs::read(workspace.join(ORACLE_INPUTS_REL_PATH)).map_err(|err| {
        format!(
            "oracle-inputs artifact missing ({err}) — bootstrap with `cargo xtask ratchet update`"
        )
    })?;
    if artifact.inputs.oracle_inputs_sha256 != sha256_hex(&inputs_bytes) {
        return Err(
            "accepted-match artifact was measured against a different oracle-inputs artifact \
             (pin mismatch) — run `cargo xtask ratchet check` for the full diagnosis"
                .into(),
        );
    }
    let tsc_js = fs::read(vendor_tsc_js_path(workspace))?;
    if artifact.inputs.tsc_js_sha256 != sha256_hex(&tsc_js) {
        return Err(
            "vendored _tsc.js pin drift: the accepted-match artifact was measured against a \
             different vendored tsc (a vendor change is a separate project, never a ratchet update)"
                .into(),
        );
    }
    Ok(AcceptedState { artifact })
}

/// Reject `accepted − current ≠ ∅` for both protected sets in the
/// selected fixed view. Partial runs (`--limit`, `--files`) project that
/// view to the executed fixtures and still enforce both subsets there;
/// a full run additionally requires every accepted fixture in the view
/// to be present, so deleting a fixture cannot silently drop its
/// identities.
pub(crate) fn enforce_accepted(
    accepted: &MatchesArtifact,
    current: &RunSets,
    selected_view: DiagnosticBand,
    executed_fixtures: &BTreeSet<String>,
    full_run: bool,
) -> ConformanceResult<()> {
    let view_name = selected_view.name();
    let accepted_view = accepted
        .views
        .get(view_name)
        .ok_or_else(|| format!("accepted-match artifact lacks selected fixed view {view_name}"))?;
    if full_run {
        if let Some(fixture) = accepted_view
            .keys()
            .find(|fixture| !executed_fixtures.contains(*fixture))
        {
            return Err(format!(
                "accepted fixture {fixture} (view {view_name}) is no longer in the corpus — \
                 accepted identities are never removed; corpus changes need a reviewed \
                 universe transition"
            )
            .into());
        }
    }
    let mut selected = accepted_view.clone();
    selected.retain(|fixture, _| executed_fixtures.contains(fixture));
    let projected = [(view_name.to_owned(), selected)].into_iter().collect();
    removals_error(
        "set-ratchet gate failed",
        collect_set_removals(&projected, current),
    )
}

// ---------------------------------------------------------------------------
// Oracle-input manifest construction and verification
// ---------------------------------------------------------------------------

fn vendor_lib_dir(workspace: &Path) -> PathBuf {
    workspace.join("vendor/typescript-6.0.3/lib")
}

fn vendor_tsc_js_path(workspace: &Path) -> PathBuf {
    vendor_lib_dir(workspace).join("_tsc.js")
}

/// The golden-producing module set, workspace-relative. Everything on
/// the oracle launch path and nothing else (see `ProducerPins`).
fn producer_module_paths(workspace: &Path) -> [(&'static str, PathBuf); 3] {
    [
        (
            "crates/oracle/driver.mjs",
            workspace.join("crates/oracle/driver.mjs"),
        ),
        (
            "crates/oracle/program-host.mjs",
            workspace.join("crates/oracle/program-host.mjs"),
        ),
        (
            "vendor/typescript-6.0.3/lib/typescript.js",
            vendor_lib_dir(workspace).join("typescript.js"),
        ),
    ]
}

pub(crate) const NODE_VERSION_REL_PATH: &str = ".node-version";

/// Normalized Node version: trimmed, no leading `v` (so the
/// `.node-version` convention and `process.version` compare equal).
pub(crate) fn normalize_node_version(raw: &str) -> String {
    raw.trim().trim_start_matches('v').to_owned()
}

pub(crate) fn pinned_node_version(workspace: &Path) -> ConformanceResult<String> {
    let path = workspace.join(NODE_VERSION_REL_PATH);
    let raw = fs::read_to_string(&path).map_err(|err| {
        format!(
            "failed to read the producer Node pin {} ({err})",
            path.display()
        )
    })?;
    let version = normalize_node_version(&raw);
    if !version.starts_with(|c: char| c.is_ascii_digit()) {
        return Err(format!(
            "{} does not contain a Node version (found {raw:?})",
            path.display()
        )
        .into());
    }
    Ok(version)
}

fn producer_pins(workspace: &Path) -> ConformanceResult<ProducerPins> {
    let mut hashes = Vec::with_capacity(3);
    for (label, path) in producer_module_paths(workspace) {
        let bytes = fs::read(&path)
            .map_err(|err| format!("failed to read producer module {label}: {err}"))?;
        hashes.push(sha256_hex(&bytes));
    }
    let [driver_sha256, program_host_sha256, typescript_js_sha256]: [String; 3] =
        hashes.try_into().expect("three producer modules");
    Ok(ProducerPins {
        driver_sha256,
        program_host_sha256,
        typescript_js_sha256,
        node_version: pinned_node_version(workspace)?,
    })
}

/// Launch-time half of the producer Node pin (the manifest/tree half
/// is `diff_oracle_inputs`): the LAUNCHED driver's `process.version`
/// must equal the tree's `.node-version`. Called before any golden is
/// written — goldens are the gating truth, and a version-skewed
/// producer would silently redefine it.
pub(crate) fn verify_launched_node(
    workspace: &Path,
    pool: &tsrs2_oracle::OraclePool,
) -> ConformanceResult<()> {
    let pinned = pinned_node_version(workspace)?;
    let launched = pool
        .node_version()
        .map_err(|err| format!("failed to query the launched oracle Node version: {err}"))?;
    let launched = normalize_node_version(&launched);
    if launched != pinned {
        return Err(format!(
            "oracle launch refused: the driver is running Node v{launched} but {NODE_VERSION_REL_PATH} \
             pins v{pinned} — install the pinned Node; changing the pin is a reviewed producer \
             transition, never a refresh side effect"
        )
        .into());
    }
    Ok(())
}

fn vendor_pins(workspace: &Path) -> ConformanceResult<VendorPins> {
    let tsc_js = fs::read(vendor_tsc_js_path(workspace))
        .map_err(|err| format!("failed to read vendored _tsc.js: {err}"))?;
    let lib_dir = vendor_lib_dir(workspace);
    let mut lib_names = Vec::new();
    for entry in fs::read_dir(&lib_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".d.ts") {
            lib_names.push(name);
        }
    }
    lib_names.sort();
    let mut hasher = Sha256::new();
    for name in &lib_names {
        let bytes = fs::read(lib_dir.join(name))?;
        hasher.update(name.as_bytes());
        hasher.update([0u8]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(VendorPins {
        tsc_js_sha256: sha256_hex(&tsc_js),
        lib_sha256: format!("{:x}", hasher.finalize()),
    })
}

fn inactive_comparators() -> BTreeMap<String, ComparatorEntry> {
    let mut comparators = BTreeMap::new();
    comparators.insert(
        "t0".to_owned(),
        ComparatorEntry::Active {
            schema: T0_COMPARATOR_SCHEMA,
        },
    );
    for tier in ["t1", "t2", "t3", "t4"] {
        comparators.insert(
            tier.to_owned(),
            ComparatorEntry::Marker("absent".to_owned()),
        );
    }
    comparators
}

/// Rebuild the oracle-input manifest content from the current tree:
/// corpus fixture bytes, harness matrix expansion, and golden oracle
/// records. `ratchet check` compares this against the stored artifact
/// so an edited/deleted golden, a changed fixture, expansion drift, or
/// undeclared corpus growth fails with the divergent entry named.
pub(crate) fn build_oracle_inputs(workspace: &Path) -> ConformanceResult<OracleInputsArtifact> {
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
    })?;
    let lib_dir = vendor_lib_dir(workspace);
    let goldens_root = workspace.join("goldens");
    let mut entries = BTreeMap::new();
    let mut totals: BTreeMap<String, u64> = FIXED_VIEWS
        .iter()
        .map(|view| (view.name().to_owned(), 0u64))
        .collect();

    for fixture in &fixtures {
        let key = fixture_key(workspace, fixture)?;
        let bytes = fs::read(fixture)?;
        let golden = read_golden(&goldens_root, &key)
            .map_err(|err| format!("golden for {key} unreadable: {err}"))?;
        if golden.schema != T0_COMPARATOR_SCHEMA {
            return Err(format!(
                "golden {key} has schema {} (t0 comparator pins schema {T0_COMPARATOR_SCHEMA}); \
                 run `cargo xtask oracle-refresh`",
                golden.schema
            )
            .into());
        }
        let programs = tsrs2_harness::expand_fixture_file(fixture, &lib_dir)?;
        if programs.len() != golden.cases.len() {
            return Err(format!(
                "golden {key} has {} case(s) but the fixture expands to {} program(s)",
                golden.cases.len(),
                programs.len()
            )
            .into());
        }
        let mut cases = BTreeMap::new();
        for program in &programs {
            let golden_case = golden
                .cases
                .iter()
                .find(|case| case.matrix_key == program.matrix_key)
                .ok_or_else(|| {
                    format!(
                        "golden {key} lacks expanded matrix case [{}]",
                        program.matrix_key
                    )
                })?;
            for view in FIXED_VIEWS {
                let buckets = golden_case
                    .oracle
                    .iter()
                    .filter(|diag| view.matches_oracle(diag))
                    .map(t0_key)
                    .collect::<BTreeSet<_>>();
                *totals.get_mut(view.name()).expect("fixed view total") += buckets.len() as u64;
            }
            cases.insert(
                program.matrix_key.clone(),
                CasePins {
                    oracle_sha256: sha256_hex(&serde_json::to_vec(&golden_case.oracle)?),
                    program_sha256: sha256_hex(program.to_json().as_bytes()),
                },
            );
        }
        entries.insert(
            key,
            FixturePins {
                fixture_sha256: sha256_hex(&bytes),
                cases,
            },
        );
    }

    Ok(OracleInputsArtifact {
        schema: ORACLE_INPUTS_SCHEMA,
        bootstrap: true,
        previous: None,
        transition: None,
        vendor: vendor_pins(workspace)?,
        producer: Some(producer_pins(workspace)?),
        comparators: inactive_comparators(),
        fixtures: entries,
        totals,
    })
}

/// Stored manifest vs freshly built content: any divergence names the
/// first offending entry and what class of drift it is.
fn diff_oracle_inputs(
    stored: &OracleInputsArtifact,
    built: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if stored.vendor.tsc_js_sha256 != built.vendor.tsc_js_sha256 {
        return Err(
            "vendored _tsc.js pin drift: the tree's _tsc.js is not the manifest's \
                    (a vendor upgrade is a separate project)"
                .into(),
        );
    }
    if stored.vendor.lib_sha256 != built.vendor.lib_sha256 {
        return Err(
            "vendored lib pin drift: the tree's lib.*.d.ts set is not the manifest's".into(),
        );
    }
    match (&stored.producer, &built.producer) {
        (Some(stored_producer), Some(built_producer)) => {
            for (label, stored_hash, built_hash) in [
                (
                    "crates/oracle/driver.mjs",
                    &stored_producer.driver_sha256,
                    &built_producer.driver_sha256,
                ),
                (
                    "crates/oracle/program-host.mjs",
                    &stored_producer.program_host_sha256,
                    &built_producer.program_host_sha256,
                ),
                (
                    "vendor/typescript-6.0.3/lib/typescript.js",
                    &stored_producer.typescript_js_sha256,
                    &built_producer.typescript_js_sha256,
                ),
            ] {
                if stored_hash != built_hash {
                    return Err(format!(
                        "oracle producer module drifted under the pin: {label} \
                         (a producer change is a reviewed transition, never a silent edit)"
                    )
                    .into());
                }
            }
            if stored_producer.node_version != built_producer.node_version {
                return Err(format!(
                    "producer Node pin drift: manifest pins v{} but {NODE_VERSION_REL_PATH} \
                     declares v{}",
                    stored_producer.node_version, built_producer.node_version
                )
                .into());
            }
        }
        (None, _) => {
            return Err(format!(
                "oracle-inputs manifest predates the producer pins — record them with \
                 `cargo xtask ratchet update --transition {PRODUCER_PIN_EXTENSION}`"
            )
            .into());
        }
        (Some(_), None) => {
            return Err("rebuilt oracle-inputs manifest lacks producer pins \
                 (build_oracle_inputs always pins the producer)"
                .into());
        }
    }
    if stored.comparators != built.comparators {
        return Err(format!(
            "comparator entries drifted: manifest {:?} vs expected {:?}",
            stored.comparators, built.comparators
        )
        .into());
    }
    for (key, stored_entry) in &stored.fixtures {
        let Some(built_entry) = built.fixtures.get(key) else {
            return Err(format!(
                "oracle input {key} is pinned in the manifest but missing from the corpus/goldens \
                 (oracle records are immutable; deletion is never a valid transition)"
            )
            .into());
        };
        if stored_entry.fixture_sha256 != built_entry.fixture_sha256 {
            return Err(format!("fixture bytes edited under the pin: {key}").into());
        }
        for (matrix, stored_case) in &stored_entry.cases {
            let Some(built_case) = built_entry.cases.get(matrix) else {
                return Err(format!("pinned matrix case deleted: {key} [{matrix}]").into());
            };
            if stored_case.oracle_sha256 != built_case.oracle_sha256 {
                return Err(format!(
                    "oracle records edited under the pin: {key} [{matrix}] \
                     (old oracle bytes are immutable)"
                )
                .into());
            }
            if stored_case.program_sha256 != built_case.program_sha256 {
                return Err(format!(
                    "matrix expansion/options/libs drifted under the pin: {key} [{matrix}]"
                )
                .into());
            }
        }
        if let Some(extra) = built_entry
            .cases
            .keys()
            .find(|matrix| !stored_entry.cases.contains_key(*matrix))
        {
            return Err(format!(
                "unpinned matrix case appeared: {key} [{extra}] — corpus growth needs \
                 `ratchet update --transition {UNIVERSE_TRANSITION}`"
            )
            .into());
        }
    }
    if let Some(extra) = built
        .fixtures
        .keys()
        .find(|key| !stored.fixtures.contains_key(*key))
    {
        return Err(format!(
            "unpinned fixture appeared: {extra} — corpus growth needs \
             `ratchet update --transition {UNIVERSE_TRANSITION}`"
        )
        .into());
    }
    if stored.totals != built.totals {
        return Err(format!(
            "oracle T0 bucket totals drifted: manifest {:?} vs recomputed {:?}",
            stored.totals, built.totals
        )
        .into());
    }
    Ok(())
}

/// Universe transition rule: every old identity and byte stays
/// unchanged; only enumerated new fixtures/cases may appear.
fn verify_universe_growth(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if older.producer != newer.producer {
        return Err("universe-transition cannot change producer pins".into());
    }
    verify_input_growth("universe-transition", older, newer)
}

/// Growth core shared by the universe transition and the trusted-base
/// compare: vendor/comparators byte-stable, no pinned fixture or case
/// removed or changed, totals never shrink.
fn verify_input_growth(
    context: &str,
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if older.vendor != newer.vendor {
        return Err(format!("{context} cannot change vendor pins").into());
    }
    if older.comparators != newer.comparators {
        return Err(format!("{context} cannot change comparator entries").into());
    }
    for (key, older_entry) in &older.fixtures {
        let Some(newer_entry) = newer.fixtures.get(key) else {
            return Err(format!("{context} removed pinned fixture {key}").into());
        };
        if older_entry.fixture_sha256 != newer_entry.fixture_sha256 {
            return Err(format!("{context} changed pinned fixture bytes for {key}").into());
        }
        for (matrix, older_case) in &older_entry.cases {
            match newer_entry.cases.get(matrix) {
                None => {
                    return Err(
                        format!("{context} removed pinned matrix case {key} [{matrix}]").into(),
                    );
                }
                Some(newer_case) if newer_case != older_case => {
                    return Err(format!(
                        "{context} changed pinned matrix case {key} [{matrix}] \
                         (old identities and bytes must remain unchanged)"
                    )
                    .into());
                }
                Some(_) => {}
            }
        }
    }
    for view in FIXED_VIEWS {
        let older_total = older.totals.get(view.name()).copied().unwrap_or(0);
        let newer_total = newer.totals.get(view.name()).copied().unwrap_or(0);
        if newer_total < older_total {
            return Err(format!(
                "{context} shrank the {} T0 bucket total ({older_total} -> {newer_total})",
                view.name()
            )
            .into());
        }
    }
    Ok(())
}

/// `producer-pin-extension` rule: the one-time detection-only
/// extension adds the producer pins to a manifest that lacked them;
/// every other input — vendor, comparators, every fixture/case pin,
/// totals — must stay byte-identical, so the extension cannot ride on
/// any other change.
fn verify_producer_pin_extension(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if older.producer.is_some() {
        return Err(format!(
            "{PRODUCER_PIN_EXTENSION} requires a predecessor without producer pins \
             (the extension is one-time)"
        )
        .into());
    }
    if newer.producer.is_none() {
        return Err(format!("{PRODUCER_PIN_EXTENSION} must add the producer pins").into());
    }
    if older.vendor != newer.vendor
        || older.comparators != newer.comparators
        || older.fixtures != newer.fixtures
        || older.totals != newer.totals
    {
        return Err(format!(
            "{PRODUCER_PIN_EXTENSION} may only add producer pins; every other input must \
             stay unchanged"
        )
        .into());
    }
    Ok(())
}

/// `oracle-correction` rule, inputs half: same corpus, same vendor,
/// same matrix expansion — only golden oracle records (and, when the
/// fix itself changes the producer, the producer pins) may differ,
/// and totals are remeasured rather than monotone. Corpus changes
/// stay a separate universe transition so a correction is exactly a
/// re-reading of the same universe under the corrected producer.
fn verify_producer_correction(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if older.vendor != newer.vendor {
        return Err(format!(
            "{ORACLE_CORRECTION} cannot change vendor pins (a vendor upgrade is a separate \
             project, never a correction)"
        )
        .into());
    }
    if older.comparators != newer.comparators {
        return Err(format!("{ORACLE_CORRECTION} cannot change comparator entries").into());
    }
    if newer.producer.is_none() {
        return Err(format!(
            "{ORACLE_CORRECTION} requires producer pins on the corrected manifest"
        )
        .into());
    }
    if let Some(removed) = older
        .fixtures
        .keys()
        .find(|key| !newer.fixtures.contains_key(*key))
    {
        return Err(format!(
            "{ORACLE_CORRECTION} removed pinned fixture {removed} (corpus changes are a \
             universe transition, never a correction)"
        )
        .into());
    }
    if let Some(added) = newer
        .fixtures
        .keys()
        .find(|key| !older.fixtures.contains_key(*key))
    {
        return Err(format!(
            "{ORACLE_CORRECTION} added fixture {added} (corpus growth is a universe \
             transition, never a correction)"
        )
        .into());
    }
    for (key, older_entry) in &older.fixtures {
        let newer_entry = &newer.fixtures[key];
        if older_entry.fixture_sha256 != newer_entry.fixture_sha256 {
            return Err(format!(
                "{ORACLE_CORRECTION} changed pinned fixture bytes for {key} (a correction \
                 re-reads the same fixtures)"
            )
            .into());
        }
        if let Some(removed) = older_entry
            .cases
            .keys()
            .find(|matrix| !newer_entry.cases.contains_key(*matrix))
        {
            return Err(format!(
                "{ORACLE_CORRECTION} removed pinned matrix case {key} [{removed}]"
            )
            .into());
        }
        if let Some(added) = newer_entry
            .cases
            .keys()
            .find(|matrix| !older_entry.cases.contains_key(*matrix))
        {
            return Err(format!("{ORACLE_CORRECTION} added matrix case {key} [{added}]").into());
        }
        for (matrix, older_case) in &older_entry.cases {
            let newer_case = &newer_entry.cases[matrix];
            if older_case.program_sha256 != newer_case.program_sha256 {
                return Err(format!(
                    "{ORACLE_CORRECTION} changed matrix expansion/options/libs for {key} \
                     [{matrix}] (only oracle records may change under a correction)"
                )
                .into());
            }
        }
    }
    Ok(())
}

/// The trusted-base inputs compare accepts any COMPOSITION of valid
/// transitions between base and head — the lineage walk has already
/// verified every individual edge; this direct compare only has to
/// reject what no composition of reviewed transitions could produce.
/// `corrected` is true when at least one oracle-correction version
/// sits between base and head: oracle record pins and totals are
/// then free (fixture bytes and expansion stay immutable under every
/// composition).
fn verify_baseline_inputs(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
    corrected: bool,
) -> ConformanceResult<()> {
    if !corrected {
        if older.producer.is_some() && older.producer != newer.producer {
            return Err("baseline compare: producer pins changed against the trusted base".into());
        }
        return verify_input_growth("baseline compare", older, newer);
    }
    if older.vendor != newer.vendor {
        return Err("baseline compare cannot change vendor pins".into());
    }
    if older.comparators != newer.comparators {
        return Err("baseline compare cannot change comparator entries".into());
    }
    if newer.producer.is_none() {
        return Err(
            "baseline compare: head manifest lacks producer pins across a correction".into(),
        );
    }
    for (key, older_entry) in &older.fixtures {
        let Some(newer_entry) = newer.fixtures.get(key) else {
            return Err(format!("baseline compare removed pinned fixture {key}").into());
        };
        if older_entry.fixture_sha256 != newer_entry.fixture_sha256 {
            return Err(format!(
                "baseline compare changed pinned fixture bytes for {key} (immutable under \
                 every reviewed transition)"
            )
            .into());
        }
        for (matrix, older_case) in &older_entry.cases {
            let Some(newer_case) = newer_entry.cases.get(matrix) else {
                return Err(format!(
                    "baseline compare removed pinned matrix case {key} [{matrix}]"
                )
                .into());
            };
            if older_case.program_sha256 != newer_case.program_sha256 {
                return Err(format!(
                    "baseline compare changed matrix expansion/options/libs for {key} \
                     [{matrix}] (immutable under every reviewed transition)"
                )
                .into());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Git lineage (measurement-integrity.md §1.1)
// ---------------------------------------------------------------------------

pub(crate) fn git(root: &Path, args: &[&str]) -> ConformanceResult<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(output.stdout)
}

/// Resolve a revision to its full commit SHA, peeling annotated tags
/// to the commit they name.
pub(crate) fn resolve_commit(root: &Path, reference: &str) -> ConformanceResult<String> {
    let spec = format!("{reference}^{{commit}}");
    let commit = git(root, &["rev-parse", "--verify", &spec])
        .map_err(|err| format!("cannot resolve {reference}: {err}"))?;
    Ok(String::from_utf8(commit)?.trim().to_owned())
}

/// Read one blob from a commit, distinguishing an absent path from a
/// real Git failure. `git show` errors must never become the bootstrap
/// exception: missing/corrupt objects and insufficient clone data are
/// integrity failures.
pub(crate) fn git_blob_optional(
    root: &Path,
    commit: &str,
    rel: &str,
) -> ConformanceResult<Option<Vec<u8>>> {
    let tree = git(root, &["ls-tree", "-z", commit, "--", rel])?;
    if tree.is_empty() {
        return Ok(None);
    }
    let spec = format!("{commit}:{rel}");
    Ok(Some(git(root, &["show", &spec])?))
}

pub(crate) fn git_root_for(workspace: &Path) -> ConformanceResult<PathBuf> {
    let out = git(workspace, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(String::from_utf8(out)?.trim()))
}

/// The artifact's path relative to the git root, forward-slashed
/// (the workspace `tsrs2/` is a subdirectory of the repository).
pub(crate) fn git_rel_path(
    git_root: &Path,
    workspace: &Path,
    rel: &str,
) -> ConformanceResult<String> {
    let abs = workspace.join(rel);
    let rel_to_root = abs
        .strip_prefix(git_root)
        .map_err(|_| format!("workspace {} is outside the git root", workspace.display()))?;
    Ok(rel_to_root.to_string_lossy().replace('\\', "/"))
}

fn git_commit_parents(git_root: &Path, commit: &str) -> ConformanceResult<Vec<String>> {
    let out = git(git_root, &["rev-list", "--parents", "-n", "1", commit])?;
    let line = String::from_utf8(out)?;
    Ok(line.split_whitespace().skip(1).map(str::to_owned).collect())
}

/// Every committed version of the path reachable from HEAD, newest
/// first, as (commit, bytes). `--full-history` is essential: default
/// path history simplification can discard a side branch that shrank
/// an artifact and later restored the merge-base bytes.
///
/// A merge whose result merely carries one parent's bytes is filtered
/// out after the full walk; the versions on every parent remain in the
/// graph and are still validated.
fn committed_versions(git_root: &Path, rel: &str) -> ConformanceResult<Vec<(String, Vec<u8>)>> {
    let out = git(
        git_root,
        &[
            "rev-list",
            "--full-history",
            "--topo-order",
            "HEAD",
            "--",
            rel,
        ],
    )?;
    let mut versions = Vec::new();
    for commit in String::from_utf8(out)?.lines() {
        let commit = commit.trim();
        if commit.is_empty() {
            continue;
        }
        let bytes = git_blob_optional(git_root, commit, rel)?;
        let parents = git_commit_parents(git_root, commit)?;
        let mut carried_from_parent = false;
        for parent in &parents {
            if git_blob_optional(git_root, parent, rel)? == bytes {
                carried_from_parent = true;
                break;
            }
        }
        if carried_from_parent {
            continue;
        }
        let Some(bytes) = bytes else {
            return Err(format!(
                "artifact {rel} was deleted at commit {commit} (artifact versions are append-only)"
            )
            .into());
        };
        versions.push((commit.to_owned(), bytes));
    }
    Ok(versions)
}

fn version_ancestry(
    git_root: &Path,
    versions: &[(String, Vec<u8>)],
) -> ConformanceResult<Vec<Vec<bool>>> {
    let mut ancestry = vec![vec![false; versions.len()]; versions.len()];
    let indices = versions
        .iter()
        .enumerate()
        .map(|(index, (commit, _))| (commit.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    // One ancestry walk per artifact version, rather than spawning
    // `merge-base` for every pair (which would become quadratic in
    // process launches as the ratchet grows).
    for (newer, (commit, _)) in versions.iter().enumerate() {
        let out = git(git_root, &["rev-list", commit])?;
        for ancestor in String::from_utf8(out)?.lines() {
            if let Some(older) = indices.get(ancestor.trim()) {
                if *older != newer {
                    ancestry[*older][newer] = true;
                }
            }
        }
    }
    Ok(ancestry)
}

fn immediate_predecessors(index: usize, ancestry: &[Vec<bool>]) -> Vec<usize> {
    let ancestors = (0..ancestry.len())
        .filter(|candidate| ancestry[*candidate][index])
        .collect::<Vec<_>>();
    ancestors
        .iter()
        .copied()
        .filter(|candidate| {
            !ancestors
                .iter()
                .any(|other| candidate != other && ancestry[*candidate][*other])
        })
        .collect()
}

fn maximal_versions(ancestry: &[Vec<bool>]) -> Vec<usize> {
    (0..ancestry.len())
        .filter(|candidate| !(0..ancestry.len()).any(|other| ancestry[*candidate][other]))
        .collect()
}

trait LineageArtifact: Sized {
    const WHAT: &'static str;
    fn decode_validated(bytes: &[u8]) -> ConformanceResult<Self>;
    fn bootstrap(&self) -> bool;
    fn previous(&self) -> Option<&Lineage>;
    /// Edge rule from `older` to `newer`: protected content is
    /// monotone and input pins are equal outside a declared
    /// transition.
    fn verify_edge(newer: &Self, older: &Self) -> ConformanceResult<()>;
}

impl LineageArtifact for MatchesArtifact {
    const WHAT: &'static str = "accepted-match artifact";

    fn decode_validated(bytes: &[u8]) -> ConformanceResult<Self> {
        let artifact: Self = decode_artifact(bytes, Self::WHAT)?;
        artifact.validate()?;
        Ok(artifact)
    }

    fn bootstrap(&self) -> bool {
        self.bootstrap
    }

    fn previous(&self) -> Option<&Lineage> {
        self.previous.as_ref()
    }

    fn verify_edge(newer: &Self, older: &Self) -> ConformanceResult<()> {
        match newer.transition.as_deref() {
            None => {
                if newer.inputs != older.inputs {
                    return Err(format!(
                        "{}: input pins changed without a declared transition",
                        Self::WHAT
                    )
                    .into());
                }
            }
            // The paired manifest edge (same commit) proves the input
            // change itself; the accepted sets stay monotone either
            // way.
            Some(UNIVERSE_TRANSITION) | Some(PRODUCER_PIN_EXTENSION) => {}
            // The one sanctioned exception to append-only growth:
            // removals are allowed but must equal the version's
            // lapsed enumeration identity-for-identity, and the
            // version must ride an actually-corrected manifest.
            Some(ORACLE_CORRECTION) => {
                let Some(lapsed) = newer.lapsed.as_ref() else {
                    return Err(format!(
                        "{}: {ORACLE_CORRECTION:?} version lacks its lapsed enumeration",
                        Self::WHAT
                    )
                    .into());
                };
                if newer.inputs.oracle_inputs_sha256 == older.inputs.oracle_inputs_sha256 {
                    return Err(format!(
                        "{}: an {ORACLE_CORRECTION:?} version must ride a corrected \
                         oracle-inputs manifest (input pins are unchanged)",
                        Self::WHAT
                    )
                    .into());
                }
                let actual = collect_removal_sets(&older.views, &newer.views);
                removals_error(
                    &format!("{ORACLE_CORRECTION} removal(s) missing from the lapsed enumeration"),
                    removal_labels(&collect_removal_sets(&actual, lapsed)),
                )?;
                let phantom = removal_labels(&collect_removal_sets(lapsed, &actual));
                if !phantom.is_empty() {
                    return Err(format!(
                        "{ORACLE_CORRECTION} lapsed enumeration claims {} identit{} that did \
                         not lapse:\n  {}",
                        phantom.len(),
                        if phantom.len() == 1 { "y" } else { "ies" },
                        phantom.join("\n  ")
                    )
                    .into());
                }
                return Ok(());
            }
            Some(other) => {
                return Err(format!(
                    "{}: unknown transition {other:?} (A1 knows {UNIVERSE_TRANSITION:?}, \
                     {PRODUCER_PIN_EXTENSION:?}, and {ORACLE_CORRECTION:?}; the A2/A3 \
                     input-schema-extensions land with their own slices)",
                    Self::WHAT
                )
                .into());
            }
        }
        removals_error(
            "accepted-match lineage edge shrank",
            collect_set_removals(&older.views, &newer.views),
        )
    }
}

impl LineageArtifact for OracleInputsArtifact {
    const WHAT: &'static str = "oracle-inputs artifact";

    fn decode_validated(bytes: &[u8]) -> ConformanceResult<Self> {
        let artifact: Self = decode_artifact(bytes, Self::WHAT)?;
        artifact.validate()?;
        Ok(artifact)
    }

    fn bootstrap(&self) -> bool {
        self.bootstrap
    }

    fn previous(&self) -> Option<&Lineage> {
        self.previous.as_ref()
    }

    fn verify_edge(newer: &Self, older: &Self) -> ConformanceResult<()> {
        match newer.transition.as_deref() {
            None => {
                if !newer.content_eq(older) {
                    return Err(format!(
                        "{}: oracle inputs changed without a declared transition \
                         (old oracle bytes are immutable)",
                        Self::WHAT
                    )
                    .into());
                }
                Ok(())
            }
            Some(UNIVERSE_TRANSITION) => verify_universe_growth(older, newer),
            Some(PRODUCER_PIN_EXTENSION) => verify_producer_pin_extension(older, newer),
            Some(ORACLE_CORRECTION) => verify_producer_correction(older, newer),
            Some(other) => Err(format!(
                "{}: unknown transition {other:?} (A1 knows {UNIVERSE_TRANSITION:?}, \
                 {PRODUCER_PIN_EXTENSION:?}, and {ORACLE_CORRECTION:?})",
                Self::WHAT
            )
            .into()),
        }
    }
}

fn verify_version_edge<T: LineageArtifact>(
    label: &str,
    version: &T,
    older_label: &str,
    older_bytes: &[u8],
    older: &T,
    known_commits: impl Iterator<Item = String>,
) -> ConformanceResult<()> {
    if version.bootstrap() {
        return Err(format!(
            "{}: second bootstrap version at {label} (the bootstrap is unique)",
            T::WHAT
        )
        .into());
    }
    let previous = version
        .previous()
        .ok_or_else(|| format!("{}: version at {label} lacks its previous pointer", T::WHAT))?;
    if previous.commit != older_label {
        let known = known_commits
            .into_iter()
            .any(|commit| commit == previous.commit);
        return Err(format!(
            "{}: version at {label} points at previous commit {} but the immediate \
             preceding version of the path is {older_label}{}",
            T::WHAT,
            previous.commit,
            if known {
                " (an older-but-not-immediate ancestor cannot hide the versions between)"
            } else {
                " (unknown or unreachable previous version)"
            }
        )
        .into());
    }
    if previous.sha256 != sha256_hex(older_bytes) {
        return Err(format!(
            "{}: version at {label} records a stale previous.sha256 for commit {older_label}",
            T::WHAT
        )
        .into());
    }
    T::verify_edge(version, older)
        .map_err(|err| format!("edge {label} -> {older_label}: {err}"))?;
    Ok(())
}

/// Validate every version in the full reachable version DAG back to
/// one bootstrap (§1.1). A path may have only one live maximal version:
/// concurrent artifact updates must be rebased and regenerated, not
/// merged by selecting one side and silently abandoning the other.
///
/// Working-tree bytes form one additional version when they differ
/// from HEAD's blob.
fn verify_lineage<T: LineageArtifact>(
    git_root: &Path,
    rel: &str,
    working_bytes: &[u8],
) -> ConformanceResult<usize> {
    let committed = committed_versions(git_root, rel)?;
    let versions = committed
        .iter()
        .map(|(label, bytes)| {
            T::decode_validated(bytes)
                .map_err(|err| format!("{} version at {label}: {err}", T::WHAT).into())
        })
        .collect::<ConformanceResult<Vec<_>>>()?;
    let ancestry = version_ancestry(git_root, &committed)?;
    let roots = (0..committed.len())
        .filter(|index| immediate_predecessors(*index, &ancestry).is_empty())
        .collect::<Vec<_>>();
    if committed.len() > 1 && roots.len() != 1 {
        return Err(format!(
            "{}: reachable history has {} bootstrap roots (expected exactly one)",
            T::WHAT,
            roots.len()
        )
        .into());
    }

    // `rev-list --topo-order` is newest-first. Reversing it reports an
    // invalid old edge before a later restoration of the same bytes.
    for index in (0..versions.len()).rev() {
        let (label, _bytes) = &committed[index];
        let version = &versions[index];
        let predecessors = immediate_predecessors(index, &ancestry);
        match predecessors.as_slice() {
            [] => {
                if !version.bootstrap() {
                    return Err(format!(
                        "{}: oldest reachable version at {label} is not the bootstrap \
                         (missing history? lineage needs the full clone depth)",
                        T::WHAT
                    )
                    .into());
                }
            }
            [older_index] => {
                let (older_label, older_bytes) = &committed[*older_index];
                verify_version_edge::<T>(
                    label,
                    version,
                    older_label,
                    older_bytes,
                    &versions[*older_index],
                    committed.iter().map(|(commit, _)| commit.clone()),
                )?;
            }
            _ => {
                return Err(format!(
                    "{}: version at {label} has {} concurrent preceding path versions; \
                     rebase and regenerate the artifact before merging",
                    T::WHAT,
                    predecessors.len()
                )
                .into());
            }
        }
    }

    let maxima = maximal_versions(&ancestry);
    if committed.len() > 1 && maxima.len() != 1 {
        return Err(format!(
            "{}: reachable history has {} concurrent live path versions; \
             rebase and regenerate the artifact before merging",
            T::WHAT,
            maxima.len()
        )
        .into());
    }

    let head_bytes = git_blob_optional(git_root, "HEAD", rel)?;
    let working_is_version = head_bytes.as_deref() != Some(working_bytes);
    if working_is_version {
        let working = T::decode_validated(working_bytes)
            .map_err(|err| format!("{} version at <working tree>: {err}", T::WHAT))?;
        match maxima.as_slice() {
            [] => {
                if !working.bootstrap() {
                    return Err(format!(
                        "{}: oldest reachable version at <working tree> is not the bootstrap",
                        T::WHAT
                    )
                    .into());
                }
            }
            [older_index] => {
                let (older_label, older_bytes) = &committed[*older_index];
                verify_version_edge::<T>(
                    "<working tree>",
                    &working,
                    older_label,
                    older_bytes,
                    &versions[*older_index],
                    committed.iter().map(|(commit, _)| commit.clone()),
                )?;
            }
            _ => {
                return Err(format!(
                    "{}: working version has {} concurrent committed predecessors; \
                     rebase and regenerate the artifact",
                    T::WHAT,
                    maxima.len()
                )
                .into());
            }
        }
        Ok(committed.len() + 1)
    } else {
        let Some(maximum) = maxima.first() else {
            return Err(format!("{}: HEAD contains no reachable artifact version", T::WHAT).into());
        };
        if committed[*maximum].1.as_slice() != working_bytes {
            return Err(format!(
                "{}: HEAD bytes do not match the unique maximal path version {}",
                T::WHAT,
                committed[*maximum].0
            )
            .into());
        }
        Ok(committed.len())
    }
}

fn verify_pair_values(
    label: &str,
    matches: &MatchesArtifact,
    inputs: &OracleInputsArtifact,
    inputs_bytes: &[u8],
) -> ConformanceResult<()> {
    if matches.inputs.oracle_inputs_sha256 != sha256_hex(inputs_bytes) {
        return Err(format!(
            "artifact pair at {label} is incoherent: accepted matches pin a different \
             oracle-inputs blob"
        )
        .into());
    }
    if matches.inputs.tsc_js_sha256 != inputs.vendor.tsc_js_sha256 {
        return Err(format!(
            "artifact pair at {label} is incoherent: accepted matches and oracle inputs \
             pin different vendored _tsc.js bytes"
        )
        .into());
    }
    Ok(())
}

/// Every historical version commit must contain a complete coherent
/// pair. This proves the `MatchesArtifact` transition rule's
/// "paired manifest edge (same commit)" premise and rejects a history
/// that updates inputs first and repairs matches in a later commit.
fn verify_committed_artifact_pairs(
    git_root: &Path,
    matches_rel: &str,
    inputs_rel: &str,
) -> ConformanceResult<()> {
    // Walk the combined path history, rather than only the union of
    // each path's material versions. That also exposes a merge which
    // carries matches from one parent and inputs from another.
    let out = git(
        git_root,
        &[
            "rev-list",
            "--full-history",
            "--topo-order",
            "HEAD",
            "--",
            matches_rel,
            inputs_rel,
        ],
    )?;
    for commit in String::from_utf8(out)?.lines() {
        let commit = commit.trim();
        let matches_bytes = git_blob_optional(git_root, commit, matches_rel)?;
        let inputs_bytes = git_blob_optional(git_root, commit, inputs_rel)?;
        let (Some(matches_bytes), Some(inputs_bytes)) = (matches_bytes, inputs_bytes) else {
            return Err(format!(
                "incomplete ratchet artifact pair at historical version commit {commit}"
            )
            .into());
        };
        let matches = MatchesArtifact::decode_validated(&matches_bytes)
            .map_err(|err| format!("accepted-match artifact at {commit}: {err}"))?;
        let inputs = OracleInputsArtifact::decode_validated(&inputs_bytes)
            .map_err(|err| format!("oracle-inputs artifact at {commit}: {err}"))?;
        verify_pair_values(commit, &matches, &inputs, &inputs_bytes)?;
    }
    Ok(())
}

/// The trusted PR-base compare: HEAD (working) content must contain
/// the resolved base artifact's protected content, so a rewritten
/// branch cannot manufacture a smaller self-consistent chain. The
/// only missing-base exception is the initial bootstrap PR.
fn verify_baseline(
    git_root: &Path,
    baseline: &str,
    matches_rel: &str,
    inputs_rel: &str,
    head_matches: &MatchesArtifact,
    head_inputs: &OracleInputsArtifact,
) -> ConformanceResult<bool> {
    let commit =
        resolve_commit(git_root, baseline).map_err(|err| format!("baseline compare: {err}"))?;

    let base_matches = git_blob_optional(git_root, &commit, matches_rel)?;
    let base_inputs = git_blob_optional(git_root, &commit, inputs_rel)?;
    let (base_matches, base_inputs) = match (base_matches, base_inputs) {
        (None, None) => {
            // Initial bootstrap PR: the base has no artifact and the
            // candidate chain's unique oldest version is the bootstrap
            // — which verify_lineage already proved. The caller must
            // additionally remeasure the full corpus and require this
            // first accepted state to be exact.
            return Ok(true);
        }
        (Some(matches), Some(inputs)) => (matches, inputs),
        (matches, inputs) => {
            return Err(format!(
                "baseline {baseline}: incomplete ratchet artifact pair (matches={}, inputs={})",
                if matches.is_some() {
                    "present"
                } else {
                    "absent"
                },
                if inputs.is_some() {
                    "present"
                } else {
                    "absent"
                },
            )
            .into());
        }
    };

    let base_matches = MatchesArtifact::decode_validated(&base_matches)?;
    let sanctioned = correction_lapses_after_base(git_root, matches_rel, &commit, head_matches)?;
    let removals = collect_removal_sets(&base_matches.views, &head_matches.views);
    match &sanctioned {
        None => removals_error(
            &format!("baseline {baseline} accepted-match compare failed"),
            removal_labels(&removals),
        )?,
        Some(sanctioned) => removals_error(
            &format!(
                "baseline {baseline} accepted-match compare failed (removal(s) beyond the \
                 enumerated correction lapses)"
            ),
            removal_labels(&collect_removal_sets(&removals, sanctioned)),
        )?,
    }

    let base_inputs = OracleInputsArtifact::decode_validated(&base_inputs)?;
    verify_baseline_inputs(&base_inputs, head_inputs, sanctioned.is_some())
        .map_err(|err| format!("baseline {baseline} oracle-input compare failed: {err}"))?;
    Ok(false)
}

/// Union of the lapsed enumerations of every `oracle-correction`
/// version that sits AFTER the trusted base: reachable from HEAD but
/// not in the base's ancestry, plus the head/working version itself
/// (which may be uncommitted during the epoch slice). `None` when no
/// such correction exists — the strict growth compare then applies
/// unchanged, so corrections never relax an ordinary PR.
fn correction_lapses_after_base(
    git_root: &Path,
    matches_rel: &str,
    base_commit: &str,
    head_matches: &MatchesArtifact,
) -> ConformanceResult<Option<RunSets>> {
    let mut sanctioned = RunSets::new();
    let mut found = false;
    if head_matches.transition.as_deref() == Some(ORACLE_CORRECTION) {
        if let Some(lapsed) = &head_matches.lapsed {
            merge_run_sets(&mut sanctioned, lapsed);
            found = true;
        }
    }
    let base_ancestors: BTreeSet<String> =
        String::from_utf8(git(git_root, &["rev-list", base_commit])?)?
            .lines()
            .map(|line| line.trim().to_owned())
            .collect();
    for (commit, bytes) in committed_versions(git_root, matches_rel)? {
        if base_ancestors.contains(&commit) {
            continue;
        }
        let artifact: MatchesArtifact = decode_artifact(&bytes, "accepted-match artifact")?;
        if artifact.transition.as_deref() == Some(ORACLE_CORRECTION) {
            if let Some(lapsed) = &artifact.lapsed {
                merge_run_sets(&mut sanctioned, lapsed);
                found = true;
            }
        }
    }
    Ok(found.then_some(sanctioned))
}

fn verify_bootstrap_measurement(accepted: &RunSets, current: &RunSets) -> ConformanceResult<()> {
    let omitted = collect_set_removals(current, accepted);
    let stale = collect_set_removals(accepted, current);
    if omitted.is_empty() && stale.is_empty() {
        return Ok(());
    }

    let discrepancies = omitted
        .iter()
        .map(|item| format!("omitted current {item}"))
        .chain(stale.iter().map(|item| format!("stale accepted {item}")))
        .collect::<Vec<_>>();
    let shown = discrepancies.iter().take(8).cloned().collect::<Vec<_>>();
    Err(format!(
        "initial bootstrap accepted state does not exactly match the current full measurement: \
         {} omitted, {} stale:\n  {}{}",
        omitted.len(),
        stale.len(),
        shown.join("\n  "),
        if discrepancies.len() > shown.len() {
            format!("\n  ... and {} more", discrepancies.len() - shown.len())
        } else {
            String::new()
        }
    )
    .into())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

fn view_counts(views: &RunSets) -> BTreeMap<String, (u64, u64)> {
    let mut counts = BTreeMap::new();
    for (view, fixtures) in views {
        let mut matched = 0u64;
        let mut complete = 0u64;
        for cases in fixtures.values() {
            for sets in cases.values() {
                matched += sets.matched.len() as u64;
                complete += sets.multiplicity_complete.len() as u64;
            }
        }
        counts.insert(view.clone(), (matched, complete));
    }
    counts
}

fn canonical_summary_rate(matched: u64, total: u64) -> f64 {
    let rate = if total == 0 {
        1.0
    } else {
        matched as f64 / total as f64
    };
    format!("{rate:.6}")
        .parse()
        .expect("a formatted finite f64 parses")
}

fn verify_ratchet_summaries(
    path: &Path,
    counts: &BTreeMap<String, (u64, u64)>,
    totals: &BTreeMap<String, u64>,
) -> ConformanceResult<()> {
    for view in FIXED_VIEWS {
        let (matched, _) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let total = totals.get(view.name()).copied().unwrap_or(0);
        let section = read_ratchet_section(path, view.ratchet_key())?;
        let expected_rate = canonical_summary_rate(matched, total);
        if section.matched != Some(matched)
            || section.total != Some(total)
            || section.rate != expected_rate
        {
            return Err(format!(
                "ratchet.toml [{}] rate/matched/total ({:.6}/{:?}/{:?}) diverges from the \
                 artifact ({expected_rate:.6}/{matched}/{total}) — run `cargo xtask ratchet update`",
                view.ratchet_key(),
                section.rate,
                section.matched,
                section.total
            )
            .into());
        }
    }
    Ok(())
}

/// Read the accepted-state pair and verify it against the current
/// tree: pair coherence, vendored `_tsc.js` pin, and the immutable
/// oracle-input diff. This is the standing-proof precondition A2 §3.2
/// requires before a tombstone may cite A1 membership — the proof is
/// invalid unless the vendor, oracle-input, and comparator pins verify
/// against the current tree.
pub(crate) fn verify_current_pair(
    workspace: &Path,
) -> ConformanceResult<(MatchesArtifact, Vec<u8>, OracleInputsArtifact, Vec<u8>)> {
    let (matches, matches_bytes): (MatchesArtifact, _) =
        read_artifact(&workspace.join(MATCHES_REL_PATH), "accepted-match artifact")?;
    matches.validate()?;
    let (inputs, inputs_bytes): (OracleInputsArtifact, _) = read_artifact(
        &workspace.join(ORACLE_INPUTS_REL_PATH),
        "oracle-inputs artifact",
    )?;
    inputs.validate()?;

    verify_pair_values("<working tree>", &matches, &inputs, &inputs_bytes)?;
    let built = build_oracle_inputs(workspace)?;
    if matches.inputs.tsc_js_sha256 != built.vendor.tsc_js_sha256 {
        return Err("vendored _tsc.js pin drift against the accepted-match artifact".into());
    }
    diff_oracle_inputs(&inputs, &built)?;
    Ok((matches, matches_bytes, inputs, inputs_bytes))
}

/// `cargo xtask ratchet check [--baseline <ref>]`: verify both
/// artifacts against the current tree (vendor pins, fixture bytes,
/// expansion, golden oracle records, ratchet.toml derived summaries)
/// and their full append-only lineage; with `--baseline`, also the
/// trusted PR-base direct compare.
pub fn check(workspace: &Path, baseline: Option<&str>) -> ConformanceResult<()> {
    let (matches, matches_bytes, inputs, inputs_bytes) = verify_current_pair(workspace)?;

    // ratchet.toml counts are derived summaries of the artifact, never
    // an independent authority.
    let counts = view_counts(&matches.views);
    verify_ratchet_summaries(&workspace.join("ratchet.toml"), &counts, &inputs.totals)?;

    let git_root = git_root_for(workspace)?;
    let matches_rel = git_rel_path(&git_root, workspace, MATCHES_REL_PATH)?;
    let inputs_rel = git_rel_path(&git_root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let matches_versions =
        verify_lineage::<MatchesArtifact>(&git_root, &matches_rel, &matches_bytes)?;
    let inputs_versions =
        verify_lineage::<OracleInputsArtifact>(&git_root, &inputs_rel, &inputs_bytes)?;
    verify_committed_artifact_pairs(&git_root, &matches_rel, &inputs_rel)?;

    let bootstrap_base = if let Some(baseline) = baseline {
        verify_baseline(
            &git_root,
            baseline,
            &matches_rel,
            &inputs_rel,
            &matches,
            &inputs,
        )?
    } else {
        false
    };
    if bootstrap_base {
        let run = super::run_conformance_collect(&ConformanceOptions {
            workspace: workspace.to_owned(),
            limit: None,
            files: Vec::new(),
            out_json: workspace.join("target/conformance/bootstrap-check.json"),
            band: DiagnosticBand::All,
        })?;
        verify_bootstrap_measurement(&matches.views, &run.sets)?;
    }

    let describe = |view: DiagnosticBand| {
        let (matched, complete) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let total = inputs.totals.get(view.name()).copied().unwrap_or(0);
        format!("{}={matched}/{total} (complete {complete})", view.name())
    };
    println!(
        "ratchet check ok: {} {} {}; fixtures={} versions matches={matches_versions} inputs={inputs_versions} baseline={}",
        describe(DiagnosticBand::All),
        describe(DiagnosticBand::TwoXxx),
        describe(DiagnosticBand::Syntactic),
        inputs.fixtures.len(),
        baseline.unwrap_or("none"),
    );
    Ok(())
}

/// `cargo xtask ratchet update [--transition universe-transition]`:
/// measure the full corpus, refuse any removal, and write both
/// artifacts plus the ratchet.toml derived summaries. Additions only.
pub fn update(workspace: &Path, transition: Option<&str>) -> ConformanceResult<()> {
    if let Some(transition) = transition {
        if ![
            UNIVERSE_TRANSITION,
            PRODUCER_PIN_EXTENSION,
            ORACLE_CORRECTION,
        ]
        .contains(&transition)
        {
            return Err(format!(
                "unknown transition {transition:?} (A1 knows {UNIVERSE_TRANSITION:?}, \
                 {PRODUCER_PIN_EXTENSION:?}, and {ORACLE_CORRECTION:?})"
            )
            .into());
        }
    }

    let run = super::run_conformance_collect(&ConformanceOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
        out_json: workspace.join("target/conformance/mismatches.json"),
        band: DiagnosticBand::All,
    })?;
    if run.summary.false_positive_diagnostics > 0 {
        return Err(format!(
            "refusing to accept a state with {} false positive diagnostic(s)",
            run.summary.false_positive_diagnostics
        )
        .into());
    }

    let git_root = git_root_for(workspace)?;
    let built = build_oracle_inputs(workspace)?;
    let vendor = built.vendor.clone();
    let totals = built.totals.clone();

    // Plan the oracle-inputs manifest first, but do not write it yet:
    // the accepted-set additions check below must succeed before either
    // half of the pinned pair changes. The growth reference is the
    // working version when present (it may already hold uncommitted
    // growth), else the committed tip; the lineage pointer always
    // targets the committed tip — a discarded working intermediate is
    // regenerated, never chained through.
    let inputs_path = workspace.join(ORACLE_INPUTS_REL_PATH);
    let inputs_rel = git_rel_path(&git_root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let working_inputs = match read_optional_bytes(&inputs_path, "oracle-inputs artifact")? {
        Some(bytes) => Some((
            decode_artifact::<OracleInputsArtifact>(&bytes, "oracle-inputs artifact")?,
            bytes,
        )),
        None => None,
    };
    let committed_inputs = committed_versions(&git_root, &inputs_rel)?;
    let tip_inputs = match committed_inputs.first() {
        Some((commit, bytes)) => Some((
            commit.clone(),
            decode_artifact::<OracleInputsArtifact>(bytes, "oracle-inputs artifact")?,
            bytes.clone(),
        )),
        None => None,
    };
    let reference = working_inputs
        .as_ref()
        .map(|(artifact, _)| artifact)
        .or(tip_inputs.as_ref().map(|(_, artifact, _)| artifact));
    let (inputs_bytes, inputs_transition, write_inputs) = match reference {
        Some(reference) if reference.content_eq(&built) => match &working_inputs {
            Some((artifact, bytes)) => {
                // An uncommitted input transition still belongs on a
                // subsequently enlarged accepted-match artifact. The
                // latter points directly to its committed tip too, so
                // dropping this marker would make that edge appear to
                // change input pins without a transition.
                let transition = tip_inputs
                    .as_ref()
                    .filter(|(_, _, tip_bytes)| tip_bytes != bytes)
                    .and(artifact.transition.clone());
                (bytes.clone(), transition, false)
            }
            None => {
                // Working file deleted but the committed tip already
                // matches the tree: plan to restore it instead of
                // forging a second bootstrap.
                let (_, _, bytes) = tip_inputs.as_ref().expect("reference implies a version");
                (bytes.clone(), None, true)
            }
        },
        Some(reference) => {
            let Some(transition) = transition else {
                return Err(
                    "oracle inputs changed (fixtures / goldens / vendor / producer). Inputs are \
                     immutable: enumerated corpus growth needs `ratchet update --transition \
                     universe-transition`; recording the producer pins needs `--transition \
                     producer-pin-extension`; a vendor or comparator change is a separate project"
                        .into(),
                );
            };
            match transition {
                UNIVERSE_TRANSITION => verify_universe_growth(reference, &built)?,
                PRODUCER_PIN_EXTENSION => verify_producer_pin_extension(reference, &built)?,
                ORACLE_CORRECTION => verify_producer_correction(reference, &built)?,
                // The allow-list at the top of `update` admits exactly
                // the names dispatched here.
                other => unreachable!("transition {other:?} validated above"),
            }
            let mut artifact = built;
            match &tip_inputs {
                Some((commit, _, bytes)) => {
                    artifact.bootstrap = false;
                    artifact.previous = Some(Lineage {
                        commit: commit.clone(),
                        sha256: sha256_hex(bytes),
                    });
                    artifact.transition = Some(transition.to_owned());
                }
                // Growing a never-committed bootstrap just regenerates
                // the bootstrap.
                None => {
                    artifact.bootstrap = true;
                    artifact.previous = None;
                    artifact.transition = None;
                }
            }
            let bytes = encode_artifact(&artifact)?;
            (bytes, artifact.transition, true)
        }
        None => {
            let bytes = encode_artifact(&built)?;
            (bytes, None, true)
        }
    };

    // The correction that will be recorded on the accepted-match
    // version. Effective (not just requested): an uncommitted
    // correction manifest carries the marker through re-runs while
    // the fixes iterate, and a REQUESTED correction with an unchanged
    // manifest would sanction arbitrary removals — refused.
    let effective_correction = inputs_transition.as_deref() == Some(ORACLE_CORRECTION);
    if transition == Some(ORACLE_CORRECTION) && !effective_correction {
        return Err(format!(
            "{ORACLE_CORRECTION} requires corrected oracle inputs, but the manifest content \
             is unchanged (nothing to correct)"
        )
        .into());
    }

    // Accepted-match artifact: additions only, against the working
    // version when present (never lose an identity someone measured
    // but has not committed yet). Under an effective correction the
    // working floor is superseded — the committed tip is the lineage
    // reference, and every removal against IT is enumerated below.
    let matches_path = workspace.join(MATCHES_REL_PATH);
    let matches_rel = git_rel_path(&git_root, workspace, MATCHES_REL_PATH)?;
    let existing_matches = match read_optional_bytes(&matches_path, "accepted-match artifact")? {
        Some(bytes) => Some((
            decode_artifact::<MatchesArtifact>(&bytes, "accepted-match artifact")?,
            bytes,
        )),
        None => None,
    };
    let old_counts = existing_matches
        .as_ref()
        .map(|(artifact, _)| view_counts(&artifact.views))
        .unwrap_or_default();
    if let Some((existing, _)) = &existing_matches {
        existing.validate()?;
        if !effective_correction {
            removals_error(
                "ratchet update refused (updates add identities only)",
                collect_set_removals(&existing.views, &run.sets),
            )?;
        }
    }

    let inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&inputs_bytes),
        tsc_js_sha256: vendor.tsc_js_sha256,
    };
    let counts = view_counts(&run.sets);
    let ratchet_path = workspace.join("ratchet.toml");
    // Render and validate every required summary section before either
    // artifact changes. Missing fields are repaired in the rendered
    // value; a missing/duplicate section is an error with no mutation.
    let (original_ratchet, ratchet_update) =
        render_ratchet_summaries(&ratchet_path, &counts, &totals)?;
    if let Some((existing, existing_bytes)) = &existing_matches {
        if existing.views == run.sets && existing.inputs == inputs {
            // Validate both complete lineages before repairing any
            // missing working file or derived summary.
            verify_lineage::<OracleInputsArtifact>(&git_root, &inputs_rel, &inputs_bytes)?;
            verify_lineage::<MatchesArtifact>(&git_root, &matches_rel, existing_bytes)?;
            verify_committed_artifact_pairs(&git_root, &matches_rel, &inputs_rel)?;
            // Still self-heal a missing working input or drifted
            // ratchet.toml before declaring the state current. Treat
            // those repairs as one transaction so a summary failure
            // cannot leave only the input artifact changed.
            let mut updates = Vec::new();
            if write_inputs {
                updates.push(AtomicFileUpdate {
                    path: &inputs_path,
                    original: working_inputs.as_ref().map(|(_, bytes)| bytes.as_slice()),
                    replacement: &inputs_bytes,
                });
            }
            if let Some(rendered) = ratchet_update.as_deref() {
                updates.push(AtomicFileUpdate {
                    path: &ratchet_path,
                    original: Some(&original_ratchet),
                    replacement: rendered,
                });
            }
            write_file_updates(&updates)?;
            println!("ratchet update: no additions; artifacts unchanged");
            return Ok(());
        }
    }

    let committed = committed_versions(&git_root, &matches_rel)?;
    let (bootstrap, previous) = match committed.first() {
        None => (true, None),
        Some((commit, bytes)) => (
            false,
            Some(Lineage {
                commit: commit.clone(),
                sha256: sha256_hex(bytes),
            }),
        ),
    };
    // A correction enumerates its lapses against the COMMITTED tip —
    // the same reference the lineage edge will verify — never against
    // a working intermediate.
    let lapsed = if effective_correction {
        let Some((_, tip_bytes)) = committed.first() else {
            return Err(format!(
                "{ORACLE_CORRECTION} has no committed accepted state to correct against \
                 (bootstrap the ratchet instead)"
            )
            .into());
        };
        let tip: MatchesArtifact = decode_artifact(tip_bytes, "accepted-match artifact")?;
        let sets = collect_removal_sets(&tip.views, &run.sets);
        let labels = removal_labels(&sets);
        for view in FIXED_VIEWS {
            let empty = ViewSets::new();
            let view_sets = sets.get(view.name()).unwrap_or(&empty);
            let (mut matched, mut complete) = (0usize, 0usize);
            for cases in view_sets.values() {
                for case in cases.values() {
                    matched += case.matched.len();
                    complete += case.multiplicity_complete.len();
                }
            }
            println!(
                "ratchet update {}: {matched} matched / {complete} multiplicity-complete \
                 identit{} lapse under the corrected oracle",
                view.name(),
                if matched + complete == 1 { "y" } else { "ies" },
            );
        }
        let shown = labels.iter().take(12).cloned().collect::<Vec<_>>();
        if !shown.is_empty() {
            println!(
                "lapsed identities:\n  {}{}",
                shown.join("\n  "),
                if labels.len() > shown.len() {
                    format!("\n  ... and {} more", labels.len() - shown.len())
                } else {
                    String::new()
                }
            );
        }
        Some(sets)
    } else {
        None
    };
    let artifact = MatchesArtifact {
        schema: MATCHES_SCHEMA,
        bootstrap,
        previous,
        transition: if bootstrap { None } else { inputs_transition },
        inputs,
        views: run.sets,
        lapsed: if bootstrap { None } else { lapsed },
    };
    artifact.validate()?;
    let matches_bytes = encode_artifact(&artifact)?;

    // Preflight the exact bytes that will be written. In particular,
    // an additions failure or malformed transition cannot leave only
    // oracle-inputs updated and the accepted artifact pinning the old
    // bytes.
    verify_lineage::<OracleInputsArtifact>(&git_root, &inputs_rel, &inputs_bytes)?;
    verify_lineage::<MatchesArtifact>(&git_root, &matches_rel, &matches_bytes)?;
    verify_committed_artifact_pairs(&git_root, &matches_rel, &inputs_rel)?;

    let mut updates = Vec::new();
    if write_inputs {
        updates.push(AtomicFileUpdate {
            path: &inputs_path,
            original: working_inputs.as_ref().map(|(_, bytes)| bytes.as_slice()),
            replacement: &inputs_bytes,
        });
    }
    updates.push(AtomicFileUpdate {
        path: &matches_path,
        original: existing_matches.as_ref().map(|(_, bytes)| bytes.as_slice()),
        replacement: &matches_bytes,
    });
    if let Some(rendered) = ratchet_update.as_deref() {
        updates.push(AtomicFileUpdate {
            path: &ratchet_path,
            original: Some(&original_ratchet),
            replacement: rendered,
        });
    }
    write_file_updates(&updates)?;

    for view in FIXED_VIEWS {
        let (matched, complete) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let (old_matched, old_complete) = old_counts.get(view.name()).copied().unwrap_or((0, 0));
        println!(
            "ratchet update {}: matched {old_matched} -> {matched} ({:+}), multiplicity-complete {old_complete} -> {complete} ({:+})",
            view.name(),
            matched as i64 - old_matched as i64,
            complete as i64 - old_complete as i64,
        );
    }
    println!(
        "ratchet update: wrote {} ({} KB) and {} ({} KB){}",
        MATCHES_REL_PATH,
        matches_bytes.len() / 1024,
        ORACLE_INPUTS_REL_PATH,
        inputs_bytes.len() / 1024,
        if bootstrap { " [bootstrap]" } else { "" },
    );
    Ok(())
}

/// Replace one artifact through a sibling temporary file, so readers
/// never observe a truncated zstd stream.
fn atomic_write(path: &Path, bytes: &[u8]) -> ConformanceResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("artifact path {} has no parent", path.display()))?;
    fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("artifact path {} has no file name", path.display()))?
        .to_string_lossy();
    let temp = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    if let Err(err) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("failed to replace {} atomically: {err}", path.display()).into());
    }
    Ok(())
}

struct AtomicFileUpdate<'a> {
    path: &'a Path,
    original: Option<&'a [u8]>,
    replacement: &'a [u8],
}

fn restore_file(path: &Path, original: Option<&[u8]>) -> ConformanceResult<()> {
    match original {
        Some(bytes) => atomic_write(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!(
                "failed to remove newly-created {} during rollback: {err}",
                path.display()
            )
            .into()),
        },
    }
}

/// Commit every planned update after validation. If any replacement
/// fails, restore all earlier files to their exact pre-update bytes
/// (or remove files created by this transaction).
fn write_file_updates(updates: &[AtomicFileUpdate<'_>]) -> ConformanceResult<()> {
    for (index, update) in updates.iter().enumerate() {
        if let Err(update_err) = atomic_write(update.path, update.replacement) {
            let mut rollback_errors = Vec::new();
            for applied in updates[..index].iter().rev() {
                if let Err(err) = restore_file(applied.path, applied.original) {
                    rollback_errors.push(format!("{}: {err}", applied.path.display()));
                }
            }
            if rollback_errors.is_empty() {
                return Err(update_err);
            }
            return Err(format!(
                "{update_err}; additionally failed to roll back {}",
                rollback_errors.join("; ")
            )
            .into());
        }
    }
    Ok(())
}

/// Rewrite the [t0]/[t0-2xxx]/[t0-syntactic] `rate`/`matched`/`total`
/// values in place. Comments and every unrelated value survive — the
/// per-slice annotations are review surface.
#[cfg(test)]
fn rewrite_ratchet_summaries(
    path: &Path,
    counts: &BTreeMap<String, (u64, u64)>,
    totals: &BTreeMap<String, u64>,
) -> ConformanceResult<()> {
    let (_, rendered) = render_ratchet_summaries(path, counts, totals)?;
    if let Some(bytes) = rendered {
        atomic_write(path, &bytes)?;
    }
    Ok(())
}

fn set_summary_value(
    table: &mut Table,
    section: &str,
    key: &str,
    mut replacement: Item,
) -> ConformanceResult<()> {
    if let Some(existing) = table.get_mut(key) {
        let decor = existing
            .as_value()
            .ok_or_else(|| format!("[{section}].{key} must be a scalar value"))?
            .decor()
            .clone();
        *replacement
            .as_value_mut()
            .expect("summary replacements are scalar values")
            .decor_mut() = decor;
        *existing = replacement;
    } else {
        table.insert(key, replacement);
    }
    Ok(())
}

fn render_ratchet_summaries(
    path: &Path,
    counts: &BTreeMap<String, (u64, u64)>,
    totals: &BTreeMap<String, u64>,
) -> ConformanceResult<(Vec<u8>, Option<Vec<u8>>)> {
    let text = fs::read_to_string(path)?;
    let original = text.as_bytes().to_vec();
    let mut document = super::parse_ratchet_document(path, &text)?;
    for view in FIXED_VIEWS {
        let (matched, _) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let total = totals.get(view.name()).copied().unwrap_or(0);
        let section = view.ratchet_key();
        let table = document
            .as_table_mut()
            .get_mut(section)
            .and_then(Item::as_table_mut)
            .ok_or_else(|| {
                format!(
                    "missing ratchet summary section [{section}] in {}",
                    path.display()
                )
            })?;
        let rate = canonical_summary_rate(matched, total);
        let matched = i64::try_from(matched)
            .map_err(|_| format!("[{section}].matched exceeds TOML's integer range"))?;
        let total = i64::try_from(total)
            .map_err(|_| format!("[{section}].total exceeds TOML's integer range"))?;
        set_summary_value(table, section, "rate", toml_value(rate))?;
        set_summary_value(table, section, "matched", toml_value(matched))?;
        set_summary_value(table, section, "total", toml_value(total))?;
    }
    let rendered = document.to_string();
    if rendered != text {
        Ok((original, Some(rendered.into_bytes())))
    } else {
        Ok((original, None))
    }
}

// ---------------------------------------------------------------------------
// Required adversarial tests (measurement-integrity.md §7, A1 rows)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::GoldenMessageChain;

    fn temp_dir(name: &str) -> PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "tsrs2-ratchet-{name}-{}-{}",
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
        for (rel, bytes) in [
            (MATCHES_REL_PATH, matches_bytes),
            (ORACLE_INPUTS_REL_PATH, inputs_bytes),
        ] {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        git_test(root, &["add", MATCHES_REL_PATH, ORACLE_INPUTS_REL_PATH]);
        git_test(root, &["commit", "-q", "-m", message]);
        let out = git(root, &["rev-parse", "HEAD"]).unwrap();
        String::from_utf8(out).unwrap().trim().to_owned()
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
        let mut sets = CaseSets::default();
        for code in matched {
            sets.matched.insert(key(*code));
        }
        for code in complete {
            sets.multiplicity_complete.insert(key(*code));
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
        }
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
        assert!(!complete.matched.contains(&t0_key(&oracle[2])));

        let partial = bucket_sets(oracle.iter(), std::slice::from_ref(&a).iter());
        assert!(partial.matched.contains(&t0_key(&a)));
        assert!(
            !partial.multiplicity_complete.contains(&t0_key(&a)),
            "a 2/1 bucket must not be multiplicity-complete"
        );

        let fp_side = diag(9999, 1, "semantic");
        let fp = bucket_sets(oracle.iter(), std::slice::from_ref(&fp_side).iter());
        assert!(!fp.matched.contains(&t0_key(&fp_side)));
        assert!(!fp.multiplicity_complete.contains(&t0_key(&fp_side)));
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
                },
            );
        let accepted = matches_artifact(accepted_views, true, None, None);
        let current = views_with(&[1005, 2322, 2345], &[1005]);
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
        assert!(err.contains("(syntactic)"), "{err}");
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
        let executed: BTreeSet<String> =
            [String::from("conformance/other.ts")].into_iter().collect();
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
        assert!(err.contains("t2") && err.contains("absent"), "{err}");

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

        let complete_overlap =
            correction_artifact(views_with(&[100], &[100]), views_with(&[], &[100]));
        let err = complete_overlap.validate().unwrap_err().to_string();
        assert!(err.contains("still accepted"), "{err}");
        assert!(err.contains("multiplicity-complete"), "{err}");
    }

    #[test]
    fn correction_edge_requires_exact_lapse_enumeration() {
        let older = matches_artifact(views_with(&[100, 101], &[100]), true, None, None);

        // Exactly enumerated: 101 lapses from matched, 100 from the
        // multiplicity-complete tier while its matched key stays.
        let corrected =
            correction_artifact(views_with(&[100, 102], &[]), views_with(&[101], &[100]));
        MatchesArtifact::verify_edge(&corrected, &older).unwrap();

        // An unenumerated removal names the identity.
        let unenumerated =
            correction_artifact(views_with(&[100, 102], &[]), views_with(&[], &[100]));
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

        let err = verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &right_bytes)
            .unwrap_err()
            .to_string();
        assert!(err.contains("concurrent live path versions"), "{err}");
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
        let err =
            verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &edited_bytes)
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
        let err =
            verify_lineage::<OracleInputsArtifact>(&repo, ORACLE_INPUTS_REL_PATH, &unknown_bytes)
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
        let head_grown =
            matches_artifact(views_with(&[2322, 2345, 2454], &[2322]), true, None, None);
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

    // -- ratchet.toml derived summaries --------------------------------------

    #[test]
    fn ratchet_toml_rewrite_preserves_comments() {
        let dir = temp_dir("toml");
        let path = dir.join("ratchet.toml");
        fs::write(
            &path,
            "[\"t0\"]\n# integer gate commentary\n\"rate\" = 0.1 # display-only\nmatched = 1\ntotal = 10\nallowed_regression = 0.0\n\n\
             [t1]\nrate = 0.0\nallowed_regression = 0.0\n\n\
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
        rewrite_ratchet_summaries(&path, &counts, &totals).unwrap();
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
        // The t1 section has no matched/total and stays untouched.
        assert!(text.contains("[t1]\nrate = 0.0"));

        verify_ratchet_summaries(&path, &counts, &totals).unwrap();
        let stale = text.replacen(
            "\"rate\" = 0.411585 # display-only",
            "\"rate\" = 0.000000 # display-only",
            1,
        );
        fs::write(&path, &stale).unwrap();
        let err = verify_ratchet_summaries(&path, &counts, &totals)
            .unwrap_err()
            .to_string();
        assert!(err.contains("rate/matched/total"), "{err}");

        let duplicate = stale.replacen("matched = 20052", "matched = 20052\nmatched = 20052", 1);
        fs::write(&path, duplicate).unwrap();
        let err = rewrite_ratchet_summaries(&path, &counts, &totals)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid ratchet.toml"), "{err}");
    }
}
