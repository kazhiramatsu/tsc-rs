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
/// The one reviewed input transition A1 knows: enumerated corpus
/// growth where every old identity and byte stays unchanged. The A2
/// and A3 `input-schema-extension` transitions are taught by their own
/// slices; an unknown transition name always fails the walk.
const UNIVERSE_TRANSITION: &str = "universe-transition";

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
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VendorPins {
    pub tsc_js_sha256: String,
    /// Combined pin over the vendored `lib.*.d.ts` inputs (sorted
    /// name+bytes): the lib texts are program inputs, so silent lib
    /// edits would change what the pinned oracle records mean.
    pub lib_sha256: String,
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
        validate_lineage_fields("accepted-match artifact", self.bootstrap, &self.previous)?;
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
        validate_lineage_fields("oracle-inputs artifact", self.bootstrap, &self.previous)?;
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
            && self.comparators == other.comparators
            && self.fixtures == other.fixtures
            && self.totals == other.totals
    }
}

fn validate_lineage_fields(
    what: &str,
    bootstrap: bool,
    previous: &Option<Lineage>,
) -> ConformanceResult<()> {
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

fn decode_artifact<T: DeserializeOwned>(bytes: &[u8], what: &str) -> ConformanceResult<T> {
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

/// Identities present in `older` but missing from `newer` — the
/// removals every gate rejects. Labels carry view/fixture/matrix/key
/// and which of the two protected sets lost the identity.
fn collect_set_removals(older: &RunSets, newer: &RunSets) -> Vec<String> {
    let empty_view = ViewSets::new();
    let empty_cases = BTreeMap::new();
    let empty_sets = CaseSets::default();
    let mut removals = Vec::new();
    for (view, older_fixtures) in older {
        let newer_fixtures = newer.get(view).unwrap_or(&empty_view);
        for (fixture, older_cases) in older_fixtures {
            let newer_cases = newer_fixtures.get(fixture).unwrap_or(&empty_cases);
            for (matrix, older_sets) in older_cases {
                let newer_sets = newer_cases.get(matrix).unwrap_or(&empty_sets);
                for key in older_sets.matched.difference(&newer_sets.matched) {
                    removals.push(format!(
                        "matched ({view}): {fixture} [{matrix}] {}",
                        t0_label(key)
                    ));
                }
                for key in older_sets
                    .multiplicity_complete
                    .difference(&newer_sets.multiplicity_complete)
                {
                    removals.push(format!(
                        "multiplicity-complete ({view}): {fixture} [{matrix}] {}",
                        t0_label(key)
                    ));
                }
            }
        }
    }
    removals
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

/// Reject `accepted − current ≠ ∅` for both protected sets in every
/// fixed view. Partial runs (`--limit`, `--files`) project every view
/// to the executed fixtures and still enforce both subsets there; a
/// full run additionally requires every accepted fixture to be
/// present, so deleting a fixture cannot silently drop its identities.
pub(crate) fn enforce_accepted(
    accepted: &MatchesArtifact,
    current: &RunSets,
    executed_fixtures: &BTreeSet<String>,
    full_run: bool,
) -> ConformanceResult<()> {
    if full_run {
        for (view, fixtures) in &accepted.views {
            if let Some(fixture) = fixtures
                .keys()
                .find(|fixture| !executed_fixtures.contains(*fixture))
            {
                return Err(format!(
                    "accepted fixture {fixture} (view {view}) is no longer in the corpus — \
                     accepted identities are never removed; corpus changes need a reviewed \
                     universe transition"
                )
                .into());
            }
        }
    }
    let mut projected = accepted.views.clone();
    for fixtures in projected.values_mut() {
        fixtures.retain(|fixture, _| executed_fixtures.contains(fixture));
    }
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
    if older.vendor != newer.vendor {
        return Err("universe-transition cannot change vendor pins".into());
    }
    if older.comparators != newer.comparators {
        return Err("universe-transition cannot change comparator entries".into());
    }
    for (key, older_entry) in &older.fixtures {
        let Some(newer_entry) = newer.fixtures.get(key) else {
            return Err(format!("universe-transition removed pinned fixture {key}").into());
        };
        if older_entry != newer_entry {
            return Err(format!(
                "universe-transition changed pinned fixture {key} (old identities and bytes \
                 must remain unchanged)"
            )
            .into());
        }
    }
    for view in FIXED_VIEWS {
        let older_total = older.totals.get(view.name()).copied().unwrap_or(0);
        let newer_total = newer.totals.get(view.name()).copied().unwrap_or(0);
        if newer_total < older_total {
            return Err(format!(
                "universe-transition shrank the {} T0 bucket total ({older_total} -> {newer_total})",
                view.name()
            )
            .into());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Git lineage (measurement-integrity.md §1.1)
// ---------------------------------------------------------------------------

fn git(root: &Path, args: &[&str]) -> ConformanceResult<Vec<u8>> {
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

fn git_optional(root: &Path, args: &[&str]) -> ConformanceResult<Option<Vec<u8>>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if output.status.success() {
        Ok(Some(output.stdout))
    } else {
        Ok(None)
    }
}

fn git_root_for(workspace: &Path) -> ConformanceResult<PathBuf> {
    let out = git(workspace, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(String::from_utf8(out)?.trim()))
}

/// The artifact's path relative to the git root, forward-slashed
/// (the workspace `tsrs2/` is a subdirectory of the repository).
fn git_rel_path(git_root: &Path, workspace: &Path, rel: &str) -> ConformanceResult<String> {
    let abs = workspace.join(rel);
    let rel_to_root = abs
        .strip_prefix(git_root)
        .map_err(|_| format!("workspace {} is outside the git root", workspace.display()))?;
    Ok(rel_to_root.to_string_lossy().replace('\\', "/"))
}

/// Committed versions of the path, newest first, as (commit, bytes).
/// `rev-list` with a path filter applies git's history simplification:
/// a merge whose result is TREESAME to a parent follows that parent
/// and creates no lineage version — exactly the §1.1 "a merge that
/// carries unchanged bytes creates no lineage version" rule.
fn committed_versions(git_root: &Path, rel: &str) -> ConformanceResult<Vec<(String, Vec<u8>)>> {
    let out = git(git_root, &["rev-list", "--topo-order", "HEAD", "--", rel])?;
    let mut versions = Vec::new();
    for commit in String::from_utf8(out)?.lines() {
        let commit = commit.trim();
        if commit.is_empty() {
            continue;
        }
        let spec = format!("{commit}:{rel}");
        let Some(bytes) = git_optional(git_root, &["show", &spec])? else {
            return Err(format!(
                "artifact {rel} unreadable at commit {commit} (deleted version or shallow \
                 history — lineage requires every version back to the bootstrap)"
            )
            .into());
        };
        versions.push((commit.to_owned(), bytes));
    }
    Ok(versions)
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
            // The paired manifest edge (same commit) proves the growth
            // itself; the accepted sets stay monotone either way.
            Some(UNIVERSE_TRANSITION) => {}
            Some(other) => {
                return Err(format!(
                    "{}: unknown transition {other:?} (A1 knows only {UNIVERSE_TRANSITION:?}; \
                     the A2/A3 input-schema-extensions land with their own slices)",
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
            Some(other) => Err(format!(
                "{}: unknown transition {other:?} (A1 knows only {UNIVERSE_TRANSITION:?})",
                Self::WHAT
            )
            .into()),
        }
    }
}

/// Walk every version of the artifact path back to the unique oldest
/// bootstrap version (§1.1). The chain is the committed versions plus
/// the working-tree bytes when they differ from the committed tip.
fn verify_lineage<T: LineageArtifact>(
    git_root: &Path,
    rel: &str,
    working_bytes: &[u8],
) -> ConformanceResult<usize> {
    let committed = committed_versions(git_root, rel)?;
    let mut chain: Vec<(String, Vec<u8>)> = Vec::new();
    match committed.first() {
        Some((_, tip)) if tip.as_slice() == working_bytes => {}
        _ => chain.push(("<working tree>".to_owned(), working_bytes.to_vec())),
    }
    chain.extend(committed);

    let versions = chain
        .iter()
        .map(|(label, bytes)| {
            let version = T::decode_validated(bytes)
                .map_err(|err| format!("{} version at {label}: {err}", T::WHAT))?;
            Ok((label.as_str(), bytes, version))
        })
        .collect::<ConformanceResult<Vec<_>>>()?;

    for (index, (label, _bytes, version)) in versions.iter().enumerate() {
        let oldest = index + 1 == versions.len();
        if oldest {
            if !version.bootstrap() {
                return Err(format!(
                    "{}: oldest reachable version at {label} is not the bootstrap \
                     (missing history? lineage needs the full clone depth)",
                    T::WHAT
                )
                .into());
            }
            continue;
        }
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
        let (older_label, older_bytes, older) = &versions[index + 1];
        if previous.commit != *older_label {
            let known = versions
                .iter()
                .skip(index + 2)
                .any(|(label, _, _)| *label == previous.commit);
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
    }
    Ok(versions.len())
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
) -> ConformanceResult<()> {
    let spec = format!("{baseline}^{{commit}}");
    let commit = git(git_root, &["rev-parse", "--verify", &spec])
        .map_err(|err| format!("cannot resolve baseline {baseline}: {err}"))?;
    let commit = String::from_utf8(commit)?.trim().to_owned();

    let matches_spec = format!("{commit}:{matches_rel}");
    match git_optional(git_root, &["show", &matches_spec])? {
        None => {
            // Initial bootstrap PR: the base has no artifact and the
            // candidate chain's unique oldest version is the bootstrap
            // — which verify_lineage already proved. Nothing to
            // compare against.
        }
        Some(base_bytes) => {
            let base = MatchesArtifact::decode_validated(&base_bytes)?;
            removals_error(
                &format!("baseline {baseline} accepted-match compare failed"),
                collect_set_removals(&base.views, &head_matches.views),
            )?;
        }
    }

    let inputs_spec = format!("{commit}:{inputs_rel}");
    if let Some(base_bytes) = git_optional(git_root, &["show", &inputs_spec])? {
        let base = OracleInputsArtifact::decode_validated(&base_bytes)?;
        if base.vendor != head_inputs.vendor {
            return Err(format!("baseline {baseline}: vendor pins differ from HEAD").into());
        }
        if base.comparators != head_inputs.comparators {
            return Err(format!("baseline {baseline}: comparator entries differ from HEAD").into());
        }
        for (key, base_entry) in &base.fixtures {
            match head_inputs.fixtures.get(key) {
                None => {
                    return Err(format!(
                        "baseline {baseline}: pinned fixture {key} was removed on this branch"
                    )
                    .into());
                }
                Some(head_entry) if head_entry != base_entry => {
                    return Err(format!(
                        "baseline {baseline}: pinned fixture {key} was edited on this branch"
                    )
                    .into());
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
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

/// `cargo xtask ratchet check [--baseline <ref>]`: verify both
/// artifacts against the current tree (vendor pins, fixture bytes,
/// expansion, golden oracle records, ratchet.toml derived summaries)
/// and their full append-only lineage; with `--baseline`, also the
/// trusted PR-base direct compare.
pub fn check(workspace: &Path, baseline: Option<&str>) -> ConformanceResult<()> {
    let (matches, matches_bytes): (MatchesArtifact, _) =
        read_artifact(&workspace.join(MATCHES_REL_PATH), "accepted-match artifact")?;
    matches.validate()?;
    let (inputs, inputs_bytes): (OracleInputsArtifact, _) = read_artifact(
        &workspace.join(ORACLE_INPUTS_REL_PATH),
        "oracle-inputs artifact",
    )?;
    inputs.validate()?;

    if matches.inputs.oracle_inputs_sha256 != sha256_hex(&inputs_bytes) {
        return Err(
            "accepted-match artifact pins a different oracle-inputs artifact (stale pair — \
             both are written together by `ratchet update`)"
                .into(),
        );
    }
    let built = build_oracle_inputs(workspace)?;
    if matches.inputs.tsc_js_sha256 != built.vendor.tsc_js_sha256 {
        return Err("vendored _tsc.js pin drift against the accepted-match artifact".into());
    }
    diff_oracle_inputs(&inputs, &built)?;

    // ratchet.toml counts are derived summaries of the artifact, never
    // an independent authority.
    let counts = view_counts(&matches.views);
    for view in FIXED_VIEWS {
        let (matched, _) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let total = inputs.totals.get(view.name()).copied().unwrap_or(0);
        let section = read_ratchet_section(&workspace.join("ratchet.toml"), view.ratchet_key())?;
        if section.matched != Some(matched) || section.total != Some(total) {
            return Err(format!(
                "ratchet.toml [{}] matched/total ({:?}/{:?}) diverges from the artifact \
                 ({matched}/{total}) — run `cargo xtask ratchet update`",
                view.ratchet_key(),
                section.matched,
                section.total
            )
            .into());
        }
    }

    let git_root = git_root_for(workspace)?;
    let matches_rel = git_rel_path(&git_root, workspace, MATCHES_REL_PATH)?;
    let inputs_rel = git_rel_path(&git_root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let matches_versions =
        verify_lineage::<MatchesArtifact>(&git_root, &matches_rel, &matches_bytes)?;
    let inputs_versions =
        verify_lineage::<OracleInputsArtifact>(&git_root, &inputs_rel, &inputs_bytes)?;

    if let Some(baseline) = baseline {
        verify_baseline(
            &git_root,
            baseline,
            &matches_rel,
            &inputs_rel,
            &matches,
            &inputs,
        )?;
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
        if transition != UNIVERSE_TRANSITION {
            return Err(format!(
                "unknown transition {transition:?} (A1 knows only {UNIVERSE_TRANSITION:?})"
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

    // Oracle-inputs manifest first: the accepted artifact pins its
    // final bytes. The growth reference is the working version when
    // present (it may already hold uncommitted growth), else the
    // committed tip; the lineage pointer always targets the committed
    // tip — a discarded working intermediate is regenerated, never
    // chained through.
    let inputs_path = workspace.join(ORACLE_INPUTS_REL_PATH);
    let inputs_rel = git_rel_path(&git_root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let working_inputs = match fs::read(&inputs_path) {
        Ok(bytes) => Some((
            decode_artifact::<OracleInputsArtifact>(&bytes, "oracle-inputs artifact")?,
            bytes,
        )),
        Err(_) => None,
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
    let (inputs_bytes, inputs_transition) = match reference {
        Some(reference) if reference.content_eq(&built) => match &working_inputs {
            Some((_, bytes)) => (bytes.clone(), None),
            None => {
                // Working file deleted but the committed tip already
                // matches the tree: restore it instead of forging a
                // second bootstrap.
                let (_, _, bytes) = tip_inputs.as_ref().expect("reference implies a version");
                fs::create_dir_all(inputs_path.parent().expect("ratchets dir"))?;
                fs::write(&inputs_path, bytes)?;
                (bytes.clone(), None)
            }
        },
        Some(reference) => {
            let Some(transition) = transition else {
                return Err(
                    "oracle inputs changed (fixtures / goldens / vendor). Inputs are immutable: \
                     enumerated corpus growth needs `ratchet update --transition \
                     universe-transition`; a vendor or comparator change is a separate project"
                        .into(),
                );
            };
            verify_universe_growth(reference, &built)?;
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
            fs::create_dir_all(inputs_path.parent().expect("ratchets dir"))?;
            fs::write(&inputs_path, &bytes)?;
            (bytes, artifact.transition)
        }
        None => {
            let bytes = encode_artifact(&built)?;
            fs::create_dir_all(inputs_path.parent().expect("ratchets dir"))?;
            fs::write(&inputs_path, &bytes)?;
            (bytes, None)
        }
    };

    // Accepted-match artifact: additions only, against the working
    // version when present (never lose an identity someone measured
    // but has not committed yet).
    let matches_path = workspace.join(MATCHES_REL_PATH);
    let matches_rel = git_rel_path(&git_root, workspace, MATCHES_REL_PATH)?;
    let existing_matches = match fs::read(&matches_path) {
        Ok(bytes) => Some(decode_artifact::<MatchesArtifact>(
            &bytes,
            "accepted-match artifact",
        )?),
        Err(_) => None,
    };
    let old_counts = existing_matches
        .as_ref()
        .map(|artifact| view_counts(&artifact.views))
        .unwrap_or_default();
    if let Some(existing) = &existing_matches {
        removals_error(
            "ratchet update refused (updates add identities only)",
            collect_set_removals(&existing.views, &run.sets),
        )?;
    }

    let inputs = MatchesInputs {
        oracle_inputs_sha256: sha256_hex(&inputs_bytes),
        tsc_js_sha256: vendor.tsc_js_sha256,
    };
    let counts = view_counts(&run.sets);
    if let Some(existing) = &existing_matches {
        if existing.views == run.sets && existing.inputs == inputs {
            // Still self-heal a drifted ratchet.toml before declaring
            // the state current.
            rewrite_ratchet_summaries(&workspace.join("ratchet.toml"), &counts, &totals)?;
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
    let artifact = MatchesArtifact {
        schema: MATCHES_SCHEMA,
        bootstrap,
        previous,
        transition: if bootstrap { None } else { inputs_transition },
        inputs,
        views: run.sets,
    };
    artifact.validate()?;
    let matches_bytes = encode_artifact(&artifact)?;
    fs::create_dir_all(matches_path.parent().expect("ratchets dir"))?;
    fs::write(&matches_path, &matches_bytes)?;

    rewrite_ratchet_summaries(&workspace.join("ratchet.toml"), &counts, &totals)?;

    for view in FIXED_VIEWS {
        let (matched, complete) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let (old_matched, old_complete) = old_counts.get(view.name()).copied().unwrap_or((0, 0));
        println!(
            "ratchet update {}: matched {old_matched} -> {matched} (+{}), multiplicity-complete {old_complete} -> {complete} (+{})",
            view.name(),
            matched - old_matched,
            complete - old_complete,
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

/// Rewrite the [t0]/[t0-2xxx]/[t0-syntactic] `rate`/`matched`/`total`
/// value lines in place. Comments and every other line survive — the
/// per-slice annotations in ratchet.toml are review surface.
fn rewrite_ratchet_summaries(
    path: &Path,
    counts: &BTreeMap<String, (u64, u64)>,
    totals: &BTreeMap<String, u64>,
) -> ConformanceResult<()> {
    let text = fs::read_to_string(path)?;
    let mut sections: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    for view in FIXED_VIEWS {
        let (matched, _) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let total = totals.get(view.name()).copied().unwrap_or(0);
        sections.insert(view.ratchet_key(), (matched, total));
    }

    let mut out = Vec::new();
    let mut current: Option<(u64, u64)> = None;
    for line in text.lines() {
        let bare = line.split('#').next().unwrap_or("").trim();
        if bare.starts_with('[') && bare.ends_with(']') {
            current = sections.get(&bare[1..bare.len() - 1]).copied();
            out.push(line.to_owned());
            continue;
        }
        let (Some((matched, total)), Some((key, _))) = (current, bare.split_once('=')) else {
            out.push(line.to_owned());
            continue;
        };
        match key.trim() {
            "rate" => out.push(format!("rate = {:.6}", matched as f64 / total as f64)),
            "matched" => out.push(format!("matched = {matched}")),
            "total" => out.push(format!("total = {total}")),
            _ => out.push(line.to_owned()),
        }
    }
    let mut rendered = out.join("\n");
    rendered.push('\n');
    if rendered != text {
        fs::write(path, rendered)?;
    }
    Ok(())
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
            comparators: inactive_comparators(),
            fixtures,
            totals: FIXED_VIEWS
                .iter()
                .map(|view| (view.name().to_owned(), 1u64))
                .collect(),
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
        let err = enforce_accepted(&accepted, &current, &executed, true)
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
        let err = enforce_accepted(&accepted, &current, &executed, true)
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
        let err = enforce_accepted(&accepted, &current, &executed, true)
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
        enforce_accepted(&accepted, &current, &executed, false).unwrap();

        // But the executed fixture's accepted subset still gates.
        let regressed = views_with(&[], &[]);
        let err = enforce_accepted(&accepted, &regressed, &executed, false)
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
        let err = enforce_accepted(&accepted, &views_with(&[], &[]), &executed, true)
            .unwrap_err()
            .to_string();
        assert!(err.contains("no longer in the corpus"), "{err}");
        assert!(err.contains("conformance/a.ts"), "{err}");
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
        let stored = inputs_stub();
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
        let stored = inputs_stub();
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
        assert!(err.contains("removed on this branch"), "{err}");
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
        // (proved by verify_lineage) makes the compare vacuously pass.
        verify_lineage::<MatchesArtifact>(&repo, MATCHES_REL_PATH, &head_bytes).unwrap();
        verify_baseline(
            &repo,
            "base",
            MATCHES_REL_PATH,
            ORACLE_INPUTS_REL_PATH,
            &head,
            &inputs_stub(),
        )
        .unwrap();
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
        diff_oracle_inputs(&stored, &build_oracle_inputs(&ws).unwrap()).unwrap();

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
            "[t0]\n# integer gate commentary\nrate = 0.1\nmatched = 1\ntotal = 10\nallowed_regression = 0.0\n\n\
             [t1]\nrate = 0.0\nallowed_regression = 0.0\n\n\
             [t0-2xxx]\nrate = 0.2\nmatched = 2\ntotal = 10\nallowed_regression = 0.0\n\n\
             [t0-syntactic]\nrate = 0.3\nmatched = 3\ntotal = 10\nallowed_regression = 0.0\n\n\
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
        assert!(text.contains("# escape commentary"));
        assert!(text.contains("rate = 0.411585"));
        assert!(text.contains("matched = 20052"));
        assert!(text.contains("total = 48719"));
        assert!(text.contains("matched = 10921"));
        assert!(text.contains("rate = 0.522136"));
        assert!(text.contains("matched = 2242"));
        assert!(text.contains("rate = 0.998219"));
        assert!(text.contains("max_untagged = 9"));
        // The t1 section has no matched/total and stays untouched.
        assert!(text.contains("[t1]\nrate = 0.0"));
    }
}
