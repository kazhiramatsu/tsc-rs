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
//!   fixture/matrix matched T0 bucket identities,
//!   multiplicity-complete buckets, and (after the reviewed M8
//!   activation) complete-multiset T1/T2/T3 bucket identities for the
//!   fixed All/2XXX/syntactic views. Multiplicity-complete is
//!   ratcheted separately because a 2/2 bucket can regress to 2/1
//!   while its T0 key stays matched.
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
/// Comparator schema for the category (T1), span/top-message (T2),
/// and full-chain/related-information (T3) complete-multiset
/// comparators. These three schemas activate atomically: a partial
/// activation would make the nesting contract unprovable.
const T1_T3_COMPARATOR_SCHEMA: u32 = 1;
/// Comparator schema for exact normalized rendered UTF-8 bytes.
const T4_COMPARATOR_SCHEMA: u32 = 1;
/// Reviewed input transition: enumerated corpus growth where every
/// old identity and byte stays unchanged. An unknown transition name
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
/// Reviewed one-time M8 input-schema extension. It changes exactly the
/// T1/T2/T3 comparator entries from explicit `"absent"` markers to
/// schema-v1 active entries. Vendor/producer pins, fixture/case pins,
/// T0/T4 comparator entries, totals, and pre-existing accepted T0 /
/// multiplicity sets remain byte-for-byte unchanged. The paired
/// accepted artifact adds the complete measured T1-T3 sets.
const TIER_1_3_INPUT_SCHEMA_EXTENSION: &str = "tier1-3-input-schema-extension";
/// Reviewed one-time A3 transition. It adds only the separately pinned
/// renderer producer, genuine oracle rendered hashes, the active T4
/// comparator, and paired accepted T4 case identities.
const T4_INPUT_SCHEMA_EXTENSION: &str = "t4-input-schema-extension";

/// The fixed recorded views (measurement-integrity.md §2). A
/// supported fixed intersection added later needs its own declared
/// view; exact A2 scope is deliberately NOT a ratchet view.
pub(crate) const FIXED_VIEWS: [DiagnosticBand; 3] = [
    DiagnosticBand::All,
    DiagnosticBand::TwoXxx,
    DiagnosticBand::Syntactic,
];

/// One case's accepted/current identity sets for a single view.
///
/// A tier identity is fixture + matrix + this T0 bucket key. Membership
/// means the COMPLETE oracle/tsrs bucket multisets agree under that
/// tier's comparator, never that one conveniently paired diagnostic
/// agrees. The coherence relation is:
///
/// `t3 ⊆ t2 ⊆ t1 ⊆ multiplicity_complete ⊆ matched`. T4 is a
/// whole-case identity (stored on the All view), not a bucket identity.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaseSets {
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub matched: BTreeSet<T0Key>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub multiplicity_complete: BTreeSet<T0Key>,
    /// T1: complete multiset equality by diagnostic category.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub t1: BTreeSet<T0Key>,
    /// T2: T1 plus exact span and top-level message text.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub t2: BTreeSet<T0Key>,
    /// T3: T2 plus the full message-chain tree and related information.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub t3: BTreeSet<T0Key>,
    /// T4: the supported-scope rendered output for this complete case is
    /// byte-exact. Only the All fixed view may carry this case marker.
    #[serde(default, skip_serializing_if = "is_false")]
    pub t4: bool,
}

/// fixture key → matrix key → case sets (empty cases are omitted).
pub type ViewSets = BTreeMap<String, BTreeMap<String, CaseSets>>;
/// view name ("all" | "2xxx" | "syntactic") → view sets.
pub type RunSets = BTreeMap<String, ViewSets>;

/// One case's bucket comparison together with the two T0 universes used to
/// derive its aggregate counts and mismatches. Keeping the universes produced
/// by `keyed` avoids walking both diagnostic streams a second time merely to
/// rebuild the same T0 keys in the conformance accumulator.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BucketGrading {
    pub sets: CaseSets,
    pub expected: BTreeSet<T0Key>,
    pub actual: BTreeSet<T0Key>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

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
    /// complete enumerated identities (per view and per protected set:
    /// T0 matched, multiplicity-complete, T1, T2, and T3 — never
    /// pooled) that lapsed under the corrected oracle. The lineage
    /// edge requires the actual removals to equal this set
    /// identity-for-identity; the
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
    /// A3-only `crates/oracle/render-driver.mjs`. Historical/pre-A3
    /// manifests omit it; the T4 extension may add it exactly once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub render_driver_sha256: Option<String>,
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
    /// Genuine lowercase SHA-256 of the exact normalized oracle
    /// rendered UTF-8 bytes. Historical/pre-A3 cases omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oracle_t4_sha256: Option<String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TierComparatorState {
    Inactive,
    T1ThroughT3,
    T1ThroughT4,
}

fn t1_t3_active(state: TierComparatorState) -> bool {
    matches!(
        state,
        TierComparatorState::T1ThroughT3 | TierComparatorState::T1ThroughT4
    )
}

fn t4_active(state: TierComparatorState) -> bool {
    state == TierComparatorState::T1ThroughT4
}

fn active_comparator(schema: u32) -> ComparatorEntry {
    ComparatorEntry::Active { schema }
}

fn absent_comparator() -> ComparatorEntry {
    ComparatorEntry::Marker("absent".to_owned())
}

fn comparator_state(
    comparators: &BTreeMap<String, ComparatorEntry>,
) -> ConformanceResult<TierComparatorState> {
    let expected_names = ["t0", "t1", "t2", "t3", "t4"];
    if let Some(extra) = comparators
        .keys()
        .find(|key| !expected_names.contains(&key.as_str()))
    {
        return Err(format!("oracle-inputs has an undeclared comparator entry {extra}").into());
    }
    if comparators.len() != expected_names.len() {
        let missing = expected_names
            .iter()
            .find(|tier| !comparators.contains_key(**tier))
            .copied()
            .unwrap_or("<unknown>");
        return Err(format!("oracle-inputs lacks comparator entry {missing}").into());
    }
    match comparators.get("t0") {
        Some(ComparatorEntry::Active { schema }) if *schema == T0_COMPARATOR_SCHEMA => {}
        entry => {
            return Err(format!(
                "oracle-inputs t0 comparator must be active at schema \
                 {T0_COMPARATOR_SCHEMA}, found {entry:?}"
            )
            .into());
        }
    }

    let inactive = ["t1", "t2", "t3"].iter().all(|tier| {
        matches!(
            comparators.get(*tier),
            Some(ComparatorEntry::Marker(marker)) if marker == "absent"
        )
    });
    let active = ["t1", "t2", "t3"].iter().all(|tier| {
        matches!(
            comparators.get(*tier),
            Some(ComparatorEntry::Active { schema }) if *schema == T1_T3_COMPARATOR_SCHEMA
        )
    });
    let t1_t3_state = match (inactive, active) {
        (true, false) => TierComparatorState::Inactive,
        (false, true) => TierComparatorState::T1ThroughT3,
        _ => {
            return Err(format!(
                "oracle-inputs T1-T3 comparators must be either all explicit \"absent\" markers \
                 or all active at schema {T1_T3_COMPARATOR_SCHEMA}; found t1={:?}, t2={:?}, \
                 t3={:?}",
                comparators.get("t1"),
                comparators.get("t2"),
                comparators.get("t3")
            )
            .into());
        }
    };
    match (t1_t3_state, comparators.get("t4")) {
        (state, Some(ComparatorEntry::Marker(marker))) if marker == "absent" => Ok(state),
        (TierComparatorState::T1ThroughT3, Some(ComparatorEntry::Active { schema }))
            if *schema == T4_COMPARATOR_SCHEMA =>
        {
            Ok(TierComparatorState::T1ThroughT4)
        }
        (TierComparatorState::Inactive, Some(ComparatorEntry::Active { .. })) => Err(
            "oracle-inputs t4 comparator cannot activate before the T1-T3 comparators"
                .to_string()
                .into(),
        ),
        (_, entry) => Err(format!(
            "oracle-inputs t4 comparator must be explicit \"absent\" or active at schema \
             {T4_COMPARATOR_SCHEMA} after {T4_INPUT_SCHEMA_EXTENSION:?}; found {entry:?}"
        )
        .into()),
    }
}

fn case_sets_have_t1_t3_membership(views: &RunSets) -> bool {
    views.values().any(|fixtures| {
        fixtures.values().any(|cases| {
            cases
                .values()
                .any(|sets| !sets.t1.is_empty() || !sets.t2.is_empty() || !sets.t3.is_empty())
        })
    })
}

fn case_sets_have_t4_membership(views: &RunSets) -> bool {
    views.values().any(|fixtures| {
        fixtures
            .values()
            .any(|cases| cases.values().any(|sets| sets.t4))
    })
}

impl MatchesArtifact {
    pub(crate) fn validate(&self) -> ConformanceResult<()> {
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
                    if sets.t4 && view != DiagnosticBand::All.name() {
                        return Err(format!(
                            "accepted-match artifact incoherent: T4 case identity may appear \
                             only in the All view, found {view} {fixture} [{matrix}]"
                        )
                        .into());
                    }
                    if !sets.multiplicity_complete.is_subset(&sets.matched) {
                        return Err(format!(
                            "accepted-match artifact incoherent: {view} {fixture} [{matrix}] has a multiplicity-complete bucket outside the matched set"
                        )
                        .into());
                    }
                    if !sets.t1.is_subset(&sets.multiplicity_complete) {
                        return Err(format!(
                            "accepted-match artifact incoherent: {view} {fixture} [{matrix}] has a T1 bucket outside the multiplicity-complete set"
                        )
                        .into());
                    }
                    if !sets.t2.is_subset(&sets.t1) {
                        return Err(format!(
                            "accepted-match artifact incoherent: {view} {fixture} [{matrix}] has a T2 bucket outside T1"
                        )
                        .into());
                    }
                    if !sets.t3.is_subset(&sets.t2) {
                        return Err(format!(
                            "accepted-match artifact incoherent: {view} {fixture} [{matrix}] has a T3 bucket outside T2"
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
                        if sets.t4 && view != DiagnosticBand::All.name() {
                            return Err(format!(
                                "lapsed T4 case identity may appear only in the All view, \
                                 found {view} {fixture} [{matrix}]"
                            )
                            .into());
                        }
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
                        for (tier, lapsed_tier, current_tier) in [
                            ("T1", &sets.t1, &current.t1),
                            ("T2", &sets.t2, &current.t2),
                            ("T3", &sets.t3, &current.t3),
                        ] {
                            if let Some(key) = lapsed_tier.intersection(current_tier).next() {
                                return Err(format!(
                                    "lapsed identity is still accepted: {tier} ({view}): \
                                     {fixture} [{matrix}] {}",
                                    t0_label(key)
                                )
                                .into());
                            }
                        }
                        if sets.t4 && current.t4 {
                            return Err(format!(
                                "lapsed identity is still accepted: T4 ({view}): \
                                 {fixture} [{matrix}]"
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
    pub(crate) fn validate(&self) -> ConformanceResult<()> {
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
        let comparator_state = comparator_state(&self.comparators)?;
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
            if matches!(producer.render_driver_sha256.as_deref(), Some("")) {
                return Err("oracle-inputs A3 render-driver pin is present but empty".into());
            }
        }
        let render_pin = self
            .producer
            .as_ref()
            .and_then(|producer| producer.render_driver_sha256.as_deref());
        if t4_active(comparator_state) {
            if !render_pin.is_some_and(valid_sha256) {
                return Err(
                    "oracle-inputs active T4 comparator requires the pinned A3 render driver"
                        .into(),
                );
            }
            for (fixture, fixture_pins) in &self.fixtures {
                for (matrix, case) in &fixture_pins.cases {
                    if !case.oracle_t4_sha256.as_deref().is_some_and(valid_sha256) {
                        return Err(format!(
                            "oracle-inputs active T4 comparator lacks a genuine rendered \
                             SHA-256 for {fixture} [{matrix}]"
                        )
                        .into());
                    }
                }
            }
        } else {
            if render_pin.is_some() {
                return Err(
                    "oracle-inputs renderer producer pin exists while T4 is inactive".into(),
                );
            }
            if let Some((fixture, matrix)) =
                self.fixtures.iter().find_map(|(fixture, fixture_pins)| {
                    fixture_pins
                        .cases
                        .iter()
                        .find(|(_, case)| case.oracle_t4_sha256.is_some())
                        .map(|(matrix, _)| (fixture, matrix))
                })
            {
                return Err(format!(
                    "oracle-inputs T4 pin exists while its comparator is absent: \
                     {fixture} [{matrix}]"
                )
                .into());
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
///
/// T1-T3 are graded independently as complete multisets. Greedy
/// matching is sufficient because each tier comparator is an
/// equivalence relation; buckets are tiny (almost always one), so the
/// O(n²) walk avoids inventing a second sortable serialization.
pub(crate) fn bucket_grading<'a>(
    oracle: impl Iterator<Item = &'a GoldenDiag>,
    tsrs: impl Iterator<Item = &'a GoldenDiag>,
) -> BucketGrading {
    fn keyed<'a>(
        diags: impl Iterator<Item = &'a GoldenDiag>,
    ) -> BTreeMap<T0Key, Vec<&'a GoldenDiag>> {
        let mut map: BTreeMap<T0Key, Vec<&'a GoldenDiag>> = BTreeMap::new();
        for diag in diags {
            map.entry(t0_key(diag)).or_default().push(diag);
        }
        map
    }
    fn multiset_eq(
        left: &[&GoldenDiag],
        right: &[&GoldenDiag],
        eq: impl Fn(&GoldenDiag, &GoldenDiag) -> bool,
    ) -> bool {
        if left.len() != right.len() {
            return false;
        }
        let mut used = vec![false; right.len()];
        'left: for left_diag in left {
            for (index, right_diag) in right.iter().enumerate() {
                if !used[index] && eq(left_diag, right_diag) {
                    used[index] = true;
                    continue 'left;
                }
            }
            return false;
        }
        true
    }
    fn t1_eq(left: &GoldenDiag, right: &GoldenDiag) -> bool {
        left.category == right.category
    }
    fn t2_eq(left: &GoldenDiag, right: &GoldenDiag) -> bool {
        t1_eq(left, right)
            && left.start == right.start
            && left.length == right.length
            && left.chain.text == right.chain.text
    }
    fn t3_eq(left: &GoldenDiag, right: &GoldenDiag) -> bool {
        t2_eq(left, right) && left.chain == right.chain && left.related == right.related
    }

    let oracle = keyed(oracle);
    let tsrs = keyed(tsrs);
    let mut sets = CaseSets::default();
    for (key, oracle_bucket) in &oracle {
        let Some(tsrs_bucket) = tsrs.get(key) else {
            continue;
        };
        sets.matched.insert(key.clone());
        if tsrs_bucket.len() == oracle_bucket.len() {
            sets.multiplicity_complete.insert(key.clone());
        }
        if !multiset_eq(oracle_bucket, tsrs_bucket, t1_eq) {
            continue;
        }
        sets.t1.insert(key.clone());
        if !multiset_eq(oracle_bucket, tsrs_bucket, t2_eq) {
            continue;
        }
        sets.t2.insert(key.clone());
        if multiset_eq(oracle_bucket, tsrs_bucket, t3_eq) {
            sets.t3.insert(key.clone());
        }
    }
    BucketGrading {
        sets,
        expected: oracle.into_keys().collect(),
        actual: tsrs.into_keys().collect(),
    }
}

pub(crate) fn bucket_sets<'a>(
    oracle: impl Iterator<Item = &'a GoldenDiag>,
    tsrs: impl Iterator<Item = &'a GoldenDiag>,
) -> CaseSets {
    bucket_grading(oracle, tsrs).sets
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
                let t1 = older_sets
                    .t1
                    .difference(&newer_sets.t1)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let t2 = older_sets
                    .t2
                    .difference(&newer_sets.t2)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let t3 = older_sets
                    .t3
                    .difference(&newer_sets.t3)
                    .cloned()
                    .collect::<BTreeSet<_>>();
                let t4 = older_sets.t4 && !newer_sets.t4;
                if matched.is_empty()
                    && multiplicity_complete.is_empty()
                    && t1.is_empty()
                    && t2.is_empty()
                    && t3.is_empty()
                    && !t4
                {
                    continue;
                }
                removal_view.entry(fixture.clone()).or_default().insert(
                    matrix.clone(),
                    CaseSets {
                        matched,
                        multiplicity_complete,
                        t1,
                        t2,
                        t3,
                        t4,
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
                for (tier, identities) in [("T1", &sets.t1), ("T2", &sets.t2), ("T3", &sets.t3)] {
                    for key in identities {
                        labels.push(format!(
                            "{tier} ({view}): {fixture} [{matrix}] {}",
                            t0_label(key)
                        ));
                    }
                }
                if sets.t4 {
                    labels.push(format!("T4 ({view}): {fixture} [{matrix}]"));
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
                acc_sets.t1.extend(sets.t1.iter().cloned());
                acc_sets.t2.extend(sets.t2.iter().cloned());
                acc_sets.t3.extend(sets.t3.iter().cloned());
                acc_sets.t4 |= sets.t4;
            }
        }
    }
}

fn t0_and_multiplicity_projection(views: &RunSets) -> RunSets {
    let mut projected = views.clone();
    for fixtures in projected.values_mut() {
        for cases in fixtures.values_mut() {
            for sets in cases.values_mut() {
                sets.t1.clear();
                sets.t2.clear();
                sets.t3.clear();
                sets.t4 = false;
            }
        }
    }
    projected
}

fn t0_through_t3_projection(views: &RunSets) -> RunSets {
    let mut projected = views.clone();
    for fixtures in projected.values_mut() {
        for cases in fixtures.values_mut() {
            for sets in cases.values_mut() {
                sets.t4 = false;
            }
            cases.retain(|_, sets| {
                !sets.matched.is_empty()
                    || !sets.multiplicity_complete.is_empty()
                    || !sets.t1.is_empty()
                    || !sets.t2.is_empty()
                    || !sets.t3.is_empty()
            });
        }
        fixtures.retain(|_, cases| !cases.is_empty());
    }
    projected
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
    pub(crate) t4_active: bool,
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
    let inputs: OracleInputsArtifact = decode_artifact(&inputs_bytes, "oracle-inputs artifact")?;
    inputs.validate()?;
    Ok(AcceptedState {
        artifact,
        t4_active: t4_active(comparator_state(&inputs.comparators)?),
    })
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

pub(crate) fn vendor_tsc_js_path(workspace: &Path) -> PathBuf {
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

fn producer_pins(workspace: &Path, include_render_driver: bool) -> ConformanceResult<ProducerPins> {
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
        render_driver_sha256: if include_render_driver {
            Some(sha256_hex(&fs::read(
                workspace.join("crates/oracle/render-driver.mjs"),
            )?))
        } else {
            None
        },
    })
}

/// Launch-time half of the producer Node pin (the manifest/tree half
/// is `diff_oracle_inputs`): the LAUNCHED driver's `process.version`
/// must equal the tree's `.node-version`. Called before any golden is
/// written — goldens are the gating truth, and a version-skewed
/// producer would silently redefine it.
pub(crate) fn verify_launched_node(
    workspace: &Path,
    pool: &tsc_oracle::OraclePool,
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

/// Launch-time enforcement for A3's separately pinned, lazy renderer
/// producer. It is called only by explicit render report/extension/check
/// paths and therefore never starts Node during ordinary conformance.
pub fn verify_launched_render_node(
    workspace: &Path,
    pool: &tsc_oracle::OraclePool,
) -> ConformanceResult<()> {
    let pinned = pinned_node_version(workspace)?;
    let launched = pool
        .render_node_version()
        .map_err(|err| format!("failed to query the launched renderer Node version: {err}"))?;
    let launched = normalize_node_version(&launched);
    if launched != pinned {
        return Err(format!(
            "oracle render launch refused: render-driver is running Node v{launched} but \
             {NODE_VERSION_REL_PATH} pins v{pinned}"
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
    comparators.insert("t0".to_owned(), active_comparator(T0_COMPARATOR_SCHEMA));
    for tier in ["t1", "t2", "t3", "t4"] {
        comparators.insert(tier.to_owned(), absent_comparator());
    }
    comparators
}

fn tier_1_3_comparators() -> BTreeMap<String, ComparatorEntry> {
    let mut comparators = inactive_comparators();
    for tier in ["t1", "t2", "t3"] {
        comparators.insert(tier.to_owned(), active_comparator(T1_T3_COMPARATOR_SCHEMA));
    }
    comparators
}

fn tier_1_4_comparators() -> BTreeMap<String, ComparatorEntry> {
    let mut comparators = tier_1_3_comparators();
    comparators.insert("t4".to_owned(), active_comparator(T4_COMPARATOR_SCHEMA));
    comparators
}

pub(crate) type T4OraclePins = BTreeMap<String, BTreeMap<String, String>>;

/// Rebuild the oracle-input manifest content from the current tree:
/// corpus fixture bytes, harness matrix expansion, and golden oracle
/// records. `ratchet check` compares this against the stored artifact
/// so an edited/deleted golden, a changed fixture, expansion drift, or
/// undeclared corpus growth fails with the divergent entry named.
pub(crate) fn build_oracle_inputs(workspace: &Path) -> ConformanceResult<OracleInputsArtifact> {
    build_oracle_inputs_with_t4_pins(workspace, None)
}

fn build_oracle_inputs_with_t4_pins(
    workspace: &Path,
    planned_t4_pins: Option<&T4OraclePins>,
) -> ConformanceResult<OracleInputsArtifact> {
    let fixtures = select_fixtures(&RefreshOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
    })?;
    let lib_dir = vendor_lib_dir(workspace);
    let goldens_root = workspace.join("goldens");
    let mut entries = BTreeMap::new();
    let mut has_t4_inputs = planned_t4_pins.is_some();
    let mut observed_golden_schema = None;
    let mut totals: BTreeMap<String, u64> = FIXED_VIEWS
        .iter()
        .map(|view| (view.name().to_owned(), 0u64))
        .collect();

    for fixture in &fixtures {
        let key = fixture_key(workspace, fixture)?;
        let bytes = fs::read(fixture)?;
        let golden = read_golden(&goldens_root, &key)
            .map_err(|err| format!("golden for {key} unreadable: {err}"))?;
        if ![T0_COMPARATOR_SCHEMA, 3].contains(&golden.schema) {
            return Err(format!(
                "golden {key} has unsupported schema {} (expected schema 2 before A3 or \
                 schema 3 after A3)",
                golden.schema
            )
            .into());
        }
        let golden_has_t4 = golden.schema == 3;
        if let Some(previous) = observed_golden_schema {
            if planned_t4_pins.is_none() && previous != golden_has_t4 {
                return Err(
                    "mixed schema-2/schema-3 goldens are not a valid A3 state; the T4 \
                     extension commits the complete universe atomically"
                        .into(),
                );
            }
        } else {
            observed_golden_schema = Some(golden_has_t4);
        }
        if planned_t4_pins.is_none() {
            has_t4_inputs = golden_has_t4;
        }
        let programs = tsc_harness::expand_fixture_file(fixture, &lib_dir)?;
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
                    oracle_t4_sha256: if let Some(planned) = planned_t4_pins {
                        Some(
                            planned
                                .get(&key)
                                .and_then(|cases| cases.get(&program.matrix_key))
                                .cloned()
                                .ok_or_else(|| {
                                    format!("A3 render plan lacks {key} [{}]", program.matrix_key)
                                })?,
                        )
                    } else if golden_has_t4 {
                        if !valid_sha256(&golden_case.oracle_cli_hash) {
                            return Err(format!(
                                "schema-3 golden {key} [{}] has invalid oracle rendered SHA-256",
                                program.matrix_key
                            )
                            .into());
                        }
                        Some(golden_case.oracle_cli_hash.clone())
                    } else {
                        None
                    },
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
        producer: Some(producer_pins(workspace, has_t4_inputs)?),
        comparators: if has_t4_inputs {
            tier_1_4_comparators()
        } else {
            tier_1_3_comparators()
        },
        fixtures: entries,
        totals,
    })
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
            if stored_producer.render_driver_sha256 != built_producer.render_driver_sha256 {
                return Err(
                    "A3 render-driver pin drifted (renderer changes require a reviewed \
                     comparator-semantic transition, never a silent edit)"
                        .into(),
                );
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
            if stored_case.oracle_t4_sha256 != built_case.oracle_t4_sha256 {
                return Err(
                    format!("oracle rendered hash edited under the pin: {key} [{matrix}]").into(),
                );
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
    verify_input_growth_with_comparators(context, older, newer, false)
}

fn verify_input_growth_with_comparators(
    context: &str,
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
    allow_schema_extensions: bool,
) -> ConformanceResult<()> {
    if older.vendor != newer.vendor {
        return Err(format!("{context} cannot change vendor pins").into());
    }
    if older.comparators != newer.comparators {
        if !allow_schema_extensions {
            return Err(format!("{context} cannot change comparator entries").into());
        }
        verify_comparator_schema_growth(older, newer)
            .map_err(|err| format!("{context} comparator transition is invalid: {err}"))?;
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
                Some(newer_case)
                    if newer_case.oracle_sha256 != older_case.oracle_sha256
                        || newer_case.program_sha256 != older_case.program_sha256
                        || (newer_case.oracle_t4_sha256 != older_case.oracle_t4_sha256
                            && (!allow_schema_extensions
                                || older_case.oracle_t4_sha256.is_some())) =>
                {
                    return Err(format!(
                        "{context} changed pinned matrix case {key} [{matrix}] \
                         (old identities and bytes must remain unchanged)"
                    )
                    .into());
                }
                Some(newer_case)
                    if allow_schema_extensions
                        && older_case.oracle_t4_sha256.is_none()
                        && newer_case
                            .oracle_t4_sha256
                            .as_deref()
                            .is_some_and(|hash| !valid_sha256(hash)) =>
                {
                    return Err(format!(
                        "{context} added an invalid oracle T4 pin for {key} [{matrix}]"
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

fn verify_comparator_schema_growth(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    match (
        comparator_state(&older.comparators)?,
        comparator_state(&newer.comparators)?,
    ) {
        (left, right) if left == right => {
            if older.comparators != newer.comparators {
                return Err("comparator entries changed without a schema-state change".into());
            }
            Ok(())
        }
        (TierComparatorState::Inactive, TierComparatorState::T1ThroughT3) => {
            verify_tier_1_3_comparator_change(older, newer)
        }
        (TierComparatorState::T1ThroughT3, TierComparatorState::T1ThroughT4) => {
            verify_t4_comparator_change(older, newer)
        }
        // A trusted-base direct compare may span both committed one-time
        // extensions even though each lineage edge remains atomic.
        (TierComparatorState::Inactive, TierComparatorState::T1ThroughT4) => {
            if older.comparators.get("t0") != newer.comparators.get("t0") {
                return Err("composed comparator growth changed T0".into());
            }
            Ok(())
        }
        (left, right) => {
            Err(format!("comparator schema state cannot move from {left:?} to {right:?}").into())
        }
    }
}

/// Comparator-only half of the M8 input schema transition. The strict
/// lineage-edge wrapper below additionally freezes every
/// non-comparator field; the trusted-base composition calls this half
/// because independent universe growth may also exist between base
/// and head.
fn verify_tier_1_3_comparator_change(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if comparator_state(&older.comparators)? != TierComparatorState::Inactive {
        return Err(format!(
            "{TIER_1_3_INPUT_SCHEMA_EXTENSION} requires an inactive predecessor \
             (the extension is one-time)"
        )
        .into());
    }
    if comparator_state(&newer.comparators)? != TierComparatorState::T1ThroughT3 {
        return Err(format!(
            "{TIER_1_3_INPUT_SCHEMA_EXTENSION} must activate T1, T2, and T3 together"
        )
        .into());
    }
    if older.comparators.get("t0") != newer.comparators.get("t0")
        || older.comparators.get("t4") != newer.comparators.get("t4")
    {
        return Err(format!(
            "{TIER_1_3_INPUT_SCHEMA_EXTENSION} may not change the T0 or T4 comparator"
        )
        .into());
    }
    Ok(())
}

fn verify_tier_1_3_input_schema_extension(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    verify_tier_1_3_comparator_change(older, newer)?;
    if older.vendor != newer.vendor
        || older.producer != newer.producer
        || older.fixtures != newer.fixtures
        || older.totals != newer.totals
    {
        return Err(format!(
            "{TIER_1_3_INPUT_SCHEMA_EXTENSION} may only activate the T1-T3 comparator \
             entries; every vendor, producer, fixture/case, oracle, expansion, and total \
             pin must stay unchanged"
        )
        .into());
    }
    Ok(())
}

fn verify_t4_comparator_change(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    if comparator_state(&older.comparators)? != TierComparatorState::T1ThroughT3 {
        return Err(format!(
            "{T4_INPUT_SCHEMA_EXTENSION} requires an active T1-T3 predecessor with T4 absent"
        )
        .into());
    }
    if comparator_state(&newer.comparators)? != TierComparatorState::T1ThroughT4 {
        return Err(
            format!("{T4_INPUT_SCHEMA_EXTENSION} must activate exactly the T4 comparator").into(),
        );
    }
    for tier in ["t0", "t1", "t2", "t3"] {
        if older.comparators.get(tier) != newer.comparators.get(tier) {
            return Err(format!(
                "{T4_INPUT_SCHEMA_EXTENSION} may not change the {tier} comparator"
            )
            .into());
        }
    }
    Ok(())
}

fn verify_t4_input_schema_extension(
    older: &OracleInputsArtifact,
    newer: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    verify_t4_comparator_change(older, newer)?;
    if older.vendor != newer.vendor || older.totals != newer.totals {
        return Err(
            format!("{T4_INPUT_SCHEMA_EXTENSION} may not change vendor pins or T0 totals").into(),
        );
    }
    let (Some(older_producer), Some(newer_producer)) = (&older.producer, &newer.producer) else {
        return Err(
            format!("{T4_INPUT_SCHEMA_EXTENSION} requires the existing producer pins").into(),
        );
    };
    if older_producer.render_driver_sha256.is_some()
        || !newer_producer
            .render_driver_sha256
            .as_deref()
            .is_some_and(valid_sha256)
    {
        return Err(format!(
            "{T4_INPUT_SCHEMA_EXTENSION} must add one genuine render-driver SHA-256"
        )
        .into());
    }
    let mut older_without_render = older_producer.clone();
    let mut newer_without_render = newer_producer.clone();
    older_without_render.render_driver_sha256 = None;
    newer_without_render.render_driver_sha256 = None;
    if older_without_render != newer_without_render {
        return Err(format!(
            "{T4_INPUT_SCHEMA_EXTENSION} may not change any pre-existing producer pin"
        )
        .into());
    }
    if older.fixtures.keys().collect::<Vec<_>>() != newer.fixtures.keys().collect::<Vec<_>>() {
        return Err(format!("{T4_INPUT_SCHEMA_EXTENSION} may not add or remove fixtures").into());
    }
    for (fixture, older_fixture) in &older.fixtures {
        let newer_fixture = &newer.fixtures[fixture];
        if older_fixture.fixture_sha256 != newer_fixture.fixture_sha256
            || older_fixture.cases.keys().collect::<Vec<_>>()
                != newer_fixture.cases.keys().collect::<Vec<_>>()
        {
            return Err(format!(
                "{T4_INPUT_SCHEMA_EXTENSION} changed fixture/case identity {fixture}"
            )
            .into());
        }
        for (matrix, older_case) in &older_fixture.cases {
            let newer_case = &newer_fixture.cases[matrix];
            if older_case.oracle_sha256 != newer_case.oracle_sha256
                || older_case.program_sha256 != newer_case.program_sha256
            {
                return Err(format!(
                    "{T4_INPUT_SCHEMA_EXTENSION} changed existing oracle/program bytes for \
                     {fixture} [{matrix}]"
                )
                .into());
            }
            if older_case.oracle_t4_sha256.is_some()
                || !newer_case
                    .oracle_t4_sha256
                    .as_deref()
                    .is_some_and(valid_sha256)
            {
                return Err(format!(
                    "{T4_INPUT_SCHEMA_EXTENSION} must add exactly one genuine oracle T4 \
                     SHA-256 for {fixture} [{matrix}]"
                )
                .into());
            }
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
    if older
        .producer
        .as_ref()
        .and_then(|producer| producer.render_driver_sha256.as_ref())
        != newer
            .producer
            .as_ref()
            .and_then(|producer| producer.render_driver_sha256.as_ref())
    {
        return Err(format!(
            "{ORACLE_CORRECTION} cannot change the A3 render-driver pin; renderer semantics \
             require a separate reviewed comparator transition"
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
        if let Some(older_producer) = &older.producer {
            let Some(newer_producer) = &newer.producer else {
                return Err("baseline compare: producer pins were removed".into());
            };
            let mut older_base = older_producer.clone();
            let mut newer_base = newer_producer.clone();
            let older_render = older_base.render_driver_sha256.take();
            let newer_render = newer_base.render_driver_sha256.take();
            if older_base != newer_base {
                return Err(
                    "baseline compare: pre-A3 producer pins changed against the trusted base"
                        .into(),
                );
            }
            if older_render.is_some() && older_render != newer_render {
                return Err(
                    "baseline compare: A3 render-driver pin changed against the trusted base"
                        .into(),
                );
            }
            if older_render.is_none()
                && newer_render
                    .as_deref()
                    .is_some_and(|hash| !valid_sha256(hash))
            {
                return Err("baseline compare: invalid A3 render-driver pin".into());
            }
        }
        return verify_input_growth_with_comparators("baseline compare", older, newer, true);
    }
    if older.vendor != newer.vendor {
        return Err("baseline compare cannot change vendor pins".into());
    }
    if older.comparators != newer.comparators {
        verify_comparator_schema_growth(older, newer)
            .map_err(|err| format!("baseline compare comparator transition is invalid: {err}"))?;
    }
    if newer.producer.is_none() {
        return Err(
            "baseline compare: head manifest lacks producer pins across a correction".into(),
        );
    }
    let older_render = older
        .producer
        .as_ref()
        .and_then(|producer| producer.render_driver_sha256.as_ref());
    let newer_render = newer
        .producer
        .as_ref()
        .and_then(|producer| producer.render_driver_sha256.as_ref());
    if older_render.is_some() && older_render != newer_render {
        return Err(
            "baseline compare changed the A3 render-driver pin across an oracle correction".into(),
        );
    }
    if older_render.is_none() && newer_render.is_some_and(|hash| !valid_sha256(hash)) {
        return Err("baseline compare added an invalid A3 render-driver pin".into());
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
///
/// Repository history before the root-workspace promotion stores every
/// workspace-relative path below `tsrs2/`; newer history stores the same
/// logical path at the repository root. Both spellings are inspected in one
/// tree query. A commit containing both is ambiguous and rejected even when
/// the blob bytes happen to agree.
pub fn git_blob_optional(
    root: &Path,
    commit: &str,
    rel: &str,
) -> ConformanceResult<Option<Vec<u8>>> {
    let paths = WorkspaceHistoryPaths::new(rel)?;
    let tree = git(
        root,
        &["ls-tree", "-z", commit, "--", &paths.current, &paths.legacy],
    )?;
    let Some(blob) = resolve_workspace_blob_ref(&tree, commit, &paths)? else {
        return Ok(None);
    };
    let spec = format!("{commit}:{}", paths.path(blob.location));
    Ok(Some(git(root, &["show", &spec])?))
}

const LEGACY_WORKSPACE_PREFIX: &str = "tsrs2/";

/// The two repository paths that have denoted one workspace-relative file.
/// Normalization is deliberately symmetric so the bridge can run both before
/// and after the atomic workspace promotion.
#[derive(Clone, Debug, Eq, PartialEq)]
struct WorkspaceHistoryPaths {
    current: String,
    legacy: String,
}

impl WorkspaceHistoryPaths {
    fn new(rel: &str) -> ConformanceResult<Self> {
        let path = Path::new(rel);
        if rel.is_empty()
            || path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::Prefix(_)
                        | std::path::Component::RootDir
                        | std::path::Component::CurDir
                        | std::path::Component::ParentDir
                )
            })
        {
            return Err(format!(
                "workspace history path must be a normalized repository-relative path: {rel:?}"
            )
            .into());
        }
        let current = rel
            .strip_prefix(LEGACY_WORKSPACE_PREFIX)
            .unwrap_or(rel)
            .to_owned();
        if current.is_empty() {
            return Err("workspace history path cannot name only the legacy prefix".into());
        }
        let legacy = format!("{LEGACY_WORKSPACE_PREFIX}{current}");
        Ok(Self { current, legacy })
    }

    fn path(&self, location: WorkspaceLocation) -> &str {
        match location {
            WorkspaceLocation::Root => &self.current,
            WorkspaceLocation::Legacy => &self.legacy,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceLocation {
    Root,
    Legacy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedBlobRef {
    object_id: String,
    location: WorkspaceLocation,
}

/// Opaque evidence that the accepted artifact pair's full append-only
/// history was verified for one exact repository state. The proof carries
/// only blob identities and hashes; decoded historical artifacts stay inside
/// the ratchet command and are released with it.
pub struct AcceptedPairHistoryProof {
    workspace: PathBuf,
    git_root: PathBuf,
    head_commit: String,
    head_matches_blob: Option<String>,
    head_inputs_blob: Option<String>,
    working_matches_sha256: String,
    working_inputs_sha256: String,
}

/// The small, artifact-agnostic header needed to reject an already-decoded
/// blob when it reappears at a different material history commit. A valid
/// linear lineage cannot reuse the same blob at two version commits: its
/// embedded predecessor still names the first occurrence's immediate parent.
#[derive(Clone, Debug, Eq, PartialEq)]
struct LineageFacts {
    bootstrap: bool,
    previous: Option<Lineage>,
}

/// The small, validated subset of an accepted-match artifact needed by the
/// historical pair audit. Keeping facts instead of the decoded artifact is
/// important: one current accepted artifact expands to tens of megabytes.
#[derive(Clone, Debug, Eq, PartialEq)]
struct MatchesPairFacts {
    oracle_inputs_sha256: String,
    tsc_js_sha256: String,
    transition: Option<String>,
    has_t1_t3_membership: bool,
    has_t4_membership: bool,
}

impl MatchesPairFacts {
    fn from_validated(artifact: &MatchesArtifact) -> Self {
        Self {
            oracle_inputs_sha256: artifact.inputs.oracle_inputs_sha256.clone(),
            tsc_js_sha256: artifact.inputs.tsc_js_sha256.clone(),
            transition: artifact.transition.clone(),
            has_t1_t3_membership: case_sets_have_t1_t3_membership(&artifact.views),
            has_t4_membership: case_sets_have_t4_membership(&artifact.views),
        }
    }
}

/// The small, validated subset of an oracle-input artifact needed by the
/// historical pair audit. `blob_sha256` is computed from the exact compressed
/// Git blob bytes rather than trusting a field inside either artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
struct InputsPairFacts {
    blob_sha256: String,
    tsc_js_sha256: String,
    transition: Option<String>,
    comparator_state: TierComparatorState,
}

impl InputsPairFacts {
    fn from_validated(
        artifact: &OracleInputsArtifact,
        blob_bytes: &[u8],
    ) -> ConformanceResult<Self> {
        Ok(Self {
            blob_sha256: sha256_hex(blob_bytes),
            tsc_js_sha256: artifact.vendor.tsc_js_sha256.clone(),
            transition: artifact.transition.clone(),
            comparator_state: comparator_state(&artifact.comparators)?,
        })
    }
}

/// Parse the exact two-path `ls-tree -z` result without treating a malformed
/// or ambiguous tree as absence. The selected object's bytes are read only
/// after this succeeds, so a real Git failure can never trigger fallback.
fn resolve_workspace_blob_ref(
    tree: &[u8],
    commit: &str,
    paths: &WorkspaceHistoryPaths,
) -> ConformanceResult<Option<ResolvedBlobRef>> {
    let mut resolved = None;
    for entry in tree
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let Some(tab) = entry.iter().position(|byte| *byte == b'\t') else {
            return Err(format!("git ls-tree returned malformed metadata for {commit}").into());
        };
        let header = String::from_utf8(entry[..tab].to_vec())?;
        let path = String::from_utf8(entry[tab + 1..].to_vec())?;
        let location = if path == paths.current {
            WorkspaceLocation::Root
        } else if path == paths.legacy {
            WorkspaceLocation::Legacy
        } else {
            return Err(format!(
                "git ls-tree returned unexpected workspace path {path:?} for {commit}"
            )
            .into());
        };
        let mut fields = header.split_whitespace();
        let mode = fields.next();
        let object_type = fields.next();
        let object_id = fields.next();
        if mode.is_none()
            || object_type != Some("blob")
            || object_id.is_none()
            || fields.next().is_some()
        {
            return Err(format!(
                "git ls-tree returned malformed blob metadata for {commit}:{path}"
            )
            .into());
        }
        if resolved.is_some() {
            return Err(format!(
                "workspace path is ambiguous at {commit}: both {} and {} exist",
                paths.current, paths.legacy
            )
            .into());
        }
        resolved = Some(ResolvedBlobRef {
            object_id: object_id.expect("validated above").to_owned(),
            location,
        });
    }
    Ok(resolved)
}

pub(crate) fn git_root_for(workspace: &Path) -> ConformanceResult<PathBuf> {
    let out = git(workspace, &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(String::from_utf8(out)?.trim()))
}

/// The artifact's path relative to the git root, forward-slashed.
/// Historical root/legacy aliases are resolved by the blob readers rather
/// than here, so this function always reports the checked-out layout.
pub(crate) fn git_rel_path(
    git_root: &Path,
    workspace: &Path,
    rel: &str,
) -> ConformanceResult<String> {
    let workspace = fs::canonicalize(workspace)?;
    let abs = workspace.join(rel);
    let rel_to_root = abs
        .strip_prefix(git_root)
        .map_err(|_| format!("workspace {} is outside the git root", workspace.display()))?;
    Ok(rel_to_root.to_string_lossy().replace('\\', "/"))
}

/// Per-command history cache. A memo is bound to one repository root,
/// pins HEAD once, and lives for exactly one ratchet operation. Lineage,
/// pair, and trusted-base validation therefore share one history view
/// while repeated queries avoid spawning Git again.
struct GitMemo {
    git_root: PathBuf,
    head_commit: String,
    blob_ids: BTreeMap<(String, String), Option<ResolvedBlobRef>>,
    blob_objects: BTreeMap<String, Vec<u8>>,
    lineage_facts: BTreeMap<(String, String), LineageFacts>,
    matches_pair_facts: BTreeMap<String, MatchesPairFacts>,
    inputs_pair_facts: BTreeMap<String, InputsPairFacts>,
    parents: BTreeMap<String, Vec<String>>,
    version_commits: BTreeMap<String, Vec<String>>,
    #[cfg(test)]
    git_invocations: usize,
    #[cfg(test)]
    pair_matches_decode_misses: usize,
    #[cfg(test)]
    pair_inputs_decode_misses: usize,
    #[cfg(test)]
    lineage_decode_misses: usize,
    #[cfg(test)]
    lineage_peak_live_versions: usize,
}

impl GitMemo {
    fn new(git_root: &Path) -> ConformanceResult<Self> {
        let head_commit = resolve_commit(git_root, "HEAD")?;
        Ok(Self {
            git_root: git_root.to_owned(),
            head_commit,
            blob_ids: BTreeMap::new(),
            blob_objects: BTreeMap::new(),
            lineage_facts: BTreeMap::new(),
            matches_pair_facts: BTreeMap::new(),
            inputs_pair_facts: BTreeMap::new(),
            parents: BTreeMap::new(),
            version_commits: BTreeMap::new(),
            #[cfg(test)]
            git_invocations: 0,
            #[cfg(test)]
            pair_matches_decode_misses: 0,
            #[cfg(test)]
            pair_inputs_decode_misses: 0,
            #[cfg(test)]
            lineage_decode_misses: 0,
            #[cfg(test)]
            lineage_peak_live_versions: 0,
        })
    }

    fn run_git(&mut self, args: &[&str]) -> ConformanceResult<Vec<u8>> {
        #[cfg(test)]
        {
            self.git_invocations += 1;
        }
        git(&self.git_root, args)
    }

    fn blob_ref_optional(
        &mut self,
        commit: &str,
        rel: &str,
    ) -> ConformanceResult<Option<ResolvedBlobRef>> {
        let commit = if commit == "HEAD" {
            self.head_commit.clone()
        } else {
            commit.to_owned()
        };
        let paths = WorkspaceHistoryPaths::new(rel)?;
        let key = (commit.clone(), paths.current.clone());
        if let Some(blob) = self.blob_ids.get(&key) {
            return Ok(blob.clone());
        }
        let tree = self.run_git(&[
            "ls-tree",
            "-z",
            &commit,
            "--",
            &paths.current,
            &paths.legacy,
        ])?;
        let Some(blob) = resolve_workspace_blob_ref(&tree, &commit, &paths)? else {
            self.blob_ids.insert(key, None);
            return Ok(None);
        };
        self.blob_ids.insert(key, Some(blob.clone()));
        Ok(Some(blob))
    }

    fn blob_optional_with_location(
        &mut self,
        commit: &str,
        rel: &str,
    ) -> ConformanceResult<Option<(Vec<u8>, ResolvedBlobRef)>> {
        let commit = if commit == "HEAD" {
            self.head_commit.clone()
        } else {
            commit.to_owned()
        };
        let paths = WorkspaceHistoryPaths::new(rel)?;
        let Some(blob) = self.blob_ref_optional(&commit, rel)? else {
            return Ok(None);
        };
        let bytes = if let Some(bytes) = self.blob_objects.get(&blob.object_id) {
            bytes.clone()
        } else {
            let spec = format!("{commit}:{}", paths.path(blob.location));
            let bytes = self.run_git(&["show", &spec])?;
            self.blob_objects
                .insert(blob.object_id.clone(), bytes.clone());
            bytes
        };
        Ok(Some((bytes, blob)))
    }

    fn blob_optional(&mut self, commit: &str, rel: &str) -> ConformanceResult<Option<Vec<u8>>> {
        Ok(self
            .blob_optional_with_location(commit, rel)?
            .map(|(bytes, _)| bytes))
    }

    fn cached_blob_ref(&self, commit: &str, rel: &str) -> ConformanceResult<ResolvedBlobRef> {
        let commit = if commit == "HEAD" {
            &self.head_commit
        } else {
            commit
        };
        let paths = WorkspaceHistoryPaths::new(rel)?;
        self.blob_ids
            .get(&(commit.to_owned(), paths.current.clone()))
            .and_then(|blob| blob.clone())
            .ok_or_else(|| {
                format!(
                    "internal Git memo lost artifact {} blob reference at commit {commit}",
                    paths.current
                )
                .into()
            })
    }

    fn remember_matches_pair_facts(
        &mut self,
        object_id: &str,
        bytes: &[u8],
        facts: MatchesPairFacts,
    ) -> ConformanceResult<()> {
        self.verify_blob_object_bytes(object_id, bytes, "accepted-match")?;
        remember_blob_facts(
            &mut self.matches_pair_facts,
            object_id,
            facts,
            "accepted-match",
        )
    }

    fn remember_lineage_facts(
        &mut self,
        artifact_kind: &str,
        object_id: &str,
        bytes: &[u8],
        facts: LineageFacts,
    ) -> ConformanceResult<()> {
        self.verify_blob_object_bytes(object_id, bytes, "lineage")?;
        let key = (artifact_kind.to_owned(), object_id.to_owned());
        if let Some(existing) = self.lineage_facts.get(&key) {
            if existing != &facts {
                return Err(format!(
                    "internal Git memo derived conflicting {artifact_kind} lineage facts for blob \
                     {object_id}"
                )
                .into());
            }
            return Ok(());
        }
        self.lineage_facts.insert(key, facts);
        Ok(())
    }

    fn lineage_facts(&self, artifact_kind: &str, object_id: &str) -> Option<&LineageFacts> {
        self.lineage_facts
            .get(&(artifact_kind.to_owned(), object_id.to_owned()))
    }

    fn remember_inputs_pair_facts(
        &mut self,
        object_id: &str,
        bytes: &[u8],
        facts: InputsPairFacts,
    ) -> ConformanceResult<()> {
        self.verify_blob_object_bytes(object_id, bytes, "oracle-inputs")?;
        remember_blob_facts(
            &mut self.inputs_pair_facts,
            object_id,
            facts,
            "oracle-inputs",
        )
    }

    fn matches_pair_facts(
        &mut self,
        object_id: &str,
        bytes: &[u8],
        label: &str,
    ) -> ConformanceResult<MatchesPairFacts> {
        if let Some(facts) = self.matches_pair_facts.get(object_id) {
            return Ok(facts.clone());
        }
        #[cfg(test)]
        {
            self.pair_matches_decode_misses += 1;
        }
        let artifact = MatchesArtifact::decode_validated(bytes)
            .map_err(|err| format!("accepted-match artifact at {label}: {err}"))?;
        let facts = MatchesPairFacts::from_validated(&artifact);
        self.remember_matches_pair_facts(object_id, bytes, facts.clone())?;
        Ok(facts)
    }

    fn inputs_pair_facts(
        &mut self,
        object_id: &str,
        bytes: &[u8],
        label: &str,
    ) -> ConformanceResult<InputsPairFacts> {
        if let Some(facts) = self.inputs_pair_facts.get(object_id) {
            return Ok(facts.clone());
        }
        #[cfg(test)]
        {
            self.pair_inputs_decode_misses += 1;
        }
        let artifact = OracleInputsArtifact::decode_validated(bytes)
            .map_err(|err| format!("oracle-inputs artifact at {label}: {err}"))?;
        let facts = InputsPairFacts::from_validated(&artifact, bytes)?;
        self.remember_inputs_pair_facts(object_id, bytes, facts.clone())?;
        Ok(facts)
    }

    fn verify_blob_object_bytes(
        &self,
        object_id: &str,
        bytes: &[u8],
        what: &str,
    ) -> ConformanceResult<()> {
        match self.blob_objects.get(object_id) {
            Some(cached) if cached == bytes => Ok(()),
            Some(_) => Err(format!(
                "internal Git memo associated {what} blob {object_id} with conflicting bytes"
            )
            .into()),
            None => Err(format!(
                "internal Git memo lacks {what} blob object {object_id} before fact caching"
            )
            .into()),
        }
    }

    fn commit_parents(&mut self, commit: &str) -> ConformanceResult<Vec<String>> {
        if let Some(parents) = self.parents.get(commit) {
            return Ok(parents.clone());
        }
        let out = self.run_git(&["rev-list", "--parents", "-n", "1", commit])?;
        let line = String::from_utf8(out)?;
        let parents = line
            .split_whitespace()
            .skip(1)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        self.parents.insert(commit.to_owned(), parents.clone());
        Ok(parents)
    }

    fn committed_versions(&mut self, rel: &str) -> ConformanceResult<Vec<(String, Vec<u8>)>> {
        let paths = WorkspaceHistoryPaths::new(rel)?;
        if let Some(commits) = self.version_commits.get(&paths.current).cloned() {
            return commits
                .into_iter()
                .map(|commit| -> ConformanceResult<_> {
                    let bytes = self
                        .blob_ids
                        .get(&(commit.clone(), paths.current.clone()))
                        .and_then(|blob| blob.as_ref())
                        .and_then(|blob| self.blob_objects.get(&blob.object_id))
                        .cloned()
                        .ok_or_else(|| {
                            format!(
                                "internal Git memo lost artifact {} bytes at commit {commit}",
                                paths.current
                            )
                        })?;
                    Ok((commit, bytes))
                })
                .collect();
        }
        let head_commit = self.head_commit.clone();
        let out = self.run_git(&[
            "rev-list",
            "--full-history",
            "--topo-order",
            &head_commit,
            "--",
            &paths.current,
            &paths.legacy,
        ])?;
        let mut versions = Vec::new();
        let mut version_commits = Vec::new();
        for commit in String::from_utf8(out)?.lines() {
            let commit = commit.trim();
            if commit.is_empty() {
                continue;
            }
            let bytes = self.blob_optional(commit, &paths.current)?;
            let parents = self.commit_parents(commit)?;
            let mut carried_from_parent = false;
            for parent in &parents {
                if self.blob_optional(parent, &paths.current)? == bytes {
                    carried_from_parent = true;
                    break;
                }
            }
            if carried_from_parent {
                continue;
            }
            let Some(bytes) = bytes else {
                return Err(format!(
                    "artifact {} was deleted at commit {commit} \
                     (artifact versions are append-only)",
                    paths.current
                )
                .into());
            };
            version_commits.push(commit.to_owned());
            versions.push((commit.to_owned(), bytes));
        }
        self.version_commits.insert(paths.current, version_commits);
        Ok(versions)
    }

    fn verify_head_unchanged(&self) -> ConformanceResult<()> {
        let current = resolve_commit(&self.git_root, "HEAD")?;
        if current != self.head_commit {
            return Err(format!(
                "ratchet history changed while it was being verified: HEAD moved from {} to {}",
                self.head_commit, current
            )
            .into());
        }
        Ok(())
    }
}

impl AcceptedPairHistoryProof {
    fn from_verified_history(
        workspace: &Path,
        git_memo: &mut GitMemo,
        matches_rel: &str,
        inputs_rel: &str,
        matches_bytes: &[u8],
        inputs_bytes: &[u8],
    ) -> ConformanceResult<Self> {
        let head_matches_blob = git_memo
            .blob_ref_optional("HEAD", matches_rel)?
            .map(|blob| blob.object_id);
        let head_inputs_blob = git_memo
            .blob_ref_optional("HEAD", inputs_rel)?
            .map(|blob| blob.object_id);
        git_memo.verify_head_unchanged()?;

        Ok(Self {
            workspace: fs::canonicalize(workspace)?,
            git_root: fs::canonicalize(&git_memo.git_root)?,
            head_commit: git_memo.head_commit.clone(),
            head_matches_blob,
            head_inputs_blob,
            working_matches_sha256: sha256_hex(matches_bytes),
            working_inputs_sha256: sha256_hex(inputs_bytes),
        })
    }

    /// Rebind the opaque proof to the repository immediately before a
    /// dependent audit consumes it. Only lightweight Git metadata and current
    /// compressed bytes are re-read; no historical artifact is decoded.
    pub(crate) fn verify_current(&self, workspace: &Path) -> ConformanceResult<()> {
        let workspace = fs::canonicalize(workspace)?;
        if workspace != self.workspace {
            return Err(format!(
                "accepted-pair history proof belongs to workspace {}, not {}",
                self.workspace.display(),
                workspace.display()
            )
            .into());
        }

        let git_root = fs::canonicalize(git_root_for(&workspace)?)?;
        if git_root != self.git_root {
            return Err(format!(
                "accepted-pair history proof belongs to Git root {}, not {}",
                self.git_root.display(),
                git_root.display()
            )
            .into());
        }

        let matches_rel = git_rel_path(&git_root, &workspace, MATCHES_REL_PATH)?;
        let inputs_rel = git_rel_path(&git_root, &workspace, ORACLE_INPUTS_REL_PATH)?;
        let mut git_memo = GitMemo::new(&git_root)?;
        if git_memo.head_commit != self.head_commit {
            return Err(format!(
                "accepted-pair history proof is stale: HEAD moved from {} to {}",
                self.head_commit, git_memo.head_commit
            )
            .into());
        }
        let current_matches_blob = git_memo
            .blob_ref_optional("HEAD", &matches_rel)?
            .map(|blob| blob.object_id);
        let current_inputs_blob = git_memo
            .blob_ref_optional("HEAD", &inputs_rel)?
            .map(|blob| blob.object_id);
        if current_matches_blob.as_deref() != self.head_matches_blob.as_deref()
            || current_inputs_blob.as_deref() != self.head_inputs_blob.as_deref()
        {
            return Err("accepted-pair history proof blob identities no longer match HEAD".into());
        }

        let working_matches = fs::read(workspace.join(MATCHES_REL_PATH))?;
        let working_inputs = fs::read(workspace.join(ORACLE_INPUTS_REL_PATH))?;
        if sha256_hex(&working_matches) != self.working_matches_sha256
            || sha256_hex(&working_inputs) != self.working_inputs_sha256
        {
            return Err("accepted-pair history proof does not match the working artifacts".into());
        }
        git_memo.verify_head_unchanged()
    }

    /// Bind a dependent audit's independently pinned HEAD token to this
    /// proof before consulting mutable repository state. Direct token equality
    /// closes ABA windows that sequential `HEAD` lookups alone cannot detect.
    pub(crate) fn verify_current_at_head(
        &self,
        workspace: &Path,
        expected_head: &str,
    ) -> ConformanceResult<()> {
        if self.head_commit != expected_head {
            return Err(format!(
                "accepted-pair history proof HEAD {} does not match dependent audit HEAD {expected_head}",
                self.head_commit
            )
            .into());
        }
        self.verify_current(workspace)
    }
}

fn remember_blob_facts<T: Eq>(
    cache: &mut BTreeMap<String, T>,
    object_id: &str,
    facts: T,
    what: &str,
) -> ConformanceResult<()> {
    if let Some(existing) = cache.get(object_id) {
        if existing != &facts {
            return Err(format!(
                "internal Git memo derived conflicting {what} facts for blob {object_id}"
            )
            .into());
        }
        return Ok(());
    }
    cache.insert(object_id.to_owned(), facts);
    Ok(())
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
    let mut memo = GitMemo::new(git_root)?;
    let versions = memo.committed_versions(rel)?;
    memo.verify_head_unchanged()?;
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
    /// Publish only the validated facts needed by the later historical pair
    /// walk. The full decoded artifact stays local to lineage verification.
    fn remember_pair_facts(
        &self,
        memo: &mut GitMemo,
        object_id: &str,
        bytes: &[u8],
    ) -> ConformanceResult<()>;
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

    fn remember_pair_facts(
        &self,
        memo: &mut GitMemo,
        object_id: &str,
        bytes: &[u8],
    ) -> ConformanceResult<()> {
        memo.remember_matches_pair_facts(object_id, bytes, MatchesPairFacts::from_validated(self))
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
            Some(TIER_1_3_INPUT_SCHEMA_EXTENSION) => {
                if newer.inputs.oracle_inputs_sha256 == older.inputs.oracle_inputs_sha256 {
                    return Err(format!(
                        "{}: {TIER_1_3_INPUT_SCHEMA_EXTENSION:?} must ride the activated \
                         oracle-inputs manifest (input pins are unchanged)",
                        Self::WHAT
                    )
                    .into());
                }
                if case_sets_have_t1_t3_membership(&older.views) {
                    return Err(format!(
                        "{}: {TIER_1_3_INPUT_SCHEMA_EXTENSION:?} predecessor already contains \
                         active tier identities (the extension is one-time)",
                        Self::WHAT
                    )
                    .into());
                }
                if t0_and_multiplicity_projection(&newer.views)
                    != t0_and_multiplicity_projection(&older.views)
                {
                    return Err(format!(
                        "{}: {TIER_1_3_INPUT_SCHEMA_EXTENSION:?} may add only T1-T3 \
                         identities; pre-existing T0 and multiplicity-complete sets must stay \
                         unchanged",
                        Self::WHAT
                    )
                    .into());
                }
            }
            Some(T4_INPUT_SCHEMA_EXTENSION) => {
                if newer.inputs.oracle_inputs_sha256 == older.inputs.oracle_inputs_sha256 {
                    return Err(format!(
                        "{}: {T4_INPUT_SCHEMA_EXTENSION:?} must ride the activated \
                         oracle-inputs manifest",
                        Self::WHAT
                    )
                    .into());
                }
                if case_sets_have_t4_membership(&older.views) {
                    return Err(format!(
                        "{}: {T4_INPUT_SCHEMA_EXTENSION:?} predecessor already contains T4 \
                         case identities (the extension is one-time)",
                        Self::WHAT
                    )
                    .into());
                }
                if t0_through_t3_projection(&newer.views) != t0_through_t3_projection(&older.views)
                {
                    return Err(format!(
                        "{}: {T4_INPUT_SCHEMA_EXTENSION:?} may add only T4 case identities; \
                         every pre-existing T0-T3 and multiplicity set must stay unchanged",
                        Self::WHAT
                    )
                    .into());
                }
            }
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
                     {PRODUCER_PIN_EXTENSION:?}, {ORACLE_CORRECTION:?}, and \
                     {TIER_1_3_INPUT_SCHEMA_EXTENSION:?}, and \
                     {T4_INPUT_SCHEMA_EXTENSION:?})",
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

    fn remember_pair_facts(
        &self,
        memo: &mut GitMemo,
        object_id: &str,
        bytes: &[u8],
    ) -> ConformanceResult<()> {
        memo.remember_inputs_pair_facts(
            object_id,
            bytes,
            InputsPairFacts::from_validated(self, bytes)?,
        )
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
            Some(TIER_1_3_INPUT_SCHEMA_EXTENSION) => {
                verify_tier_1_3_input_schema_extension(older, newer)
            }
            Some(T4_INPUT_SCHEMA_EXTENSION) => verify_t4_input_schema_extension(older, newer),
            Some(other) => Err(format!(
                "{}: unknown transition {other:?} (A1 knows {UNIVERSE_TRANSITION:?}, \
                 {PRODUCER_PIN_EXTENSION:?}, {ORACLE_CORRECTION:?}, and \
                 {TIER_1_3_INPUT_SCHEMA_EXTENSION:?}, and \
                 {T4_INPUT_SCHEMA_EXTENSION:?})",
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
    verify_version_pointer(
        T::WHAT,
        label,
        version.bootstrap(),
        version.previous(),
        older_label,
        older_bytes,
        known_commits,
    )?;
    T::verify_edge(version, older)
        .map_err(|err| format!("edge {label} -> {older_label}: {err}"))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify_version_pointer(
    what: &str,
    label: &str,
    bootstrap: bool,
    previous: Option<&Lineage>,
    older_label: &str,
    older_bytes: &[u8],
    known_commits: impl Iterator<Item = String>,
) -> ConformanceResult<()> {
    if bootstrap {
        return Err(format!(
            "{what}: second bootstrap version at {label} (the bootstrap is unique)"
        )
        .into());
    }
    let previous =
        previous.ok_or_else(|| format!("{what}: version at {label} lacks its previous pointer"))?;
    if previous.commit != older_label {
        let known = known_commits
            .into_iter()
            .any(|commit| commit == previous.commit);
        return Err(format!(
            "{what}: version at {label} points at previous commit {} but the immediate \
             preceding version of the path is {older_label}{}",
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
            "{what}: version at {label} records a stale previous.sha256 for commit {older_label}"
        )
        .into());
    }
    Ok(())
}

struct VerifiedLineageShape {
    /// Material version indices ordered oldest to newest.
    order: Vec<usize>,
    maximum: Option<usize>,
}

fn verify_lineage_shape<T: LineageArtifact>(
    committed: &[(String, Vec<u8>)],
    ancestry: &[Vec<bool>],
) -> ConformanceResult<VerifiedLineageShape> {
    let predecessors = (0..committed.len())
        .map(|index| immediate_predecessors(index, ancestry))
        .collect::<Vec<_>>();
    let roots = predecessors
        .iter()
        .enumerate()
        .filter_map(|(index, predecessors)| predecessors.is_empty().then_some(index))
        .collect::<Vec<_>>();
    if committed.len() > 1 && roots.len() != 1 {
        return Err(format!(
            "{}: reachable history has {} bootstrap roots (expected exactly one)",
            T::WHAT,
            roots.len()
        )
        .into());
    }

    // Within the shape audit, report the oldest merge which exposes multiple
    // material predecessors first.
    for index in (0..committed.len()).rev() {
        if predecessors[index].len() > 1 {
            return Err(format!(
                "{}: version at {} has {} concurrent preceding path versions; \
                 rebase and regenerate the artifact before merging",
                T::WHAT,
                committed[index].0,
                predecessors[index].len()
            )
            .into());
        }
    }

    let maxima = maximal_versions(ancestry);
    if committed.len() > 1 && maxima.len() != 1 {
        return Err(format!(
            "{}: reachable history has {} concurrent live path versions; \
             rebase and regenerate the artifact before merging",
            T::WHAT,
            maxima.len()
        )
        .into());
    }
    if committed.is_empty() {
        return Ok(VerifiedLineageShape {
            order: Vec::new(),
            maximum: None,
        });
    }

    let root = *roots
        .first()
        .ok_or_else(|| format!("internal {} lineage has no root", T::WHAT))?;
    let maximum = *maxima
        .first()
        .ok_or_else(|| format!("internal {} lineage has no maximum", T::WHAT))?;
    let mut order = Vec::with_capacity(committed.len());
    let mut current = root;
    loop {
        order.push(current);
        if current == maximum {
            break;
        }
        let successors = predecessors
            .iter()
            .enumerate()
            .filter_map(|(index, predecessors)| {
                (predecessors.as_slice() == [current]).then_some(index)
            })
            .collect::<Vec<_>>();
        let [successor] = successors.as_slice() else {
            return Err(format!(
                "internal {} lineage is not linear after its DAG shape passed",
                T::WHAT
            )
            .into());
        };
        current = *successor;
    }
    if order.len() != committed.len() {
        return Err(format!(
            "internal {} lineage omitted material versions after its DAG shape passed",
            T::WHAT
        )
        .into());
    }
    Ok(VerifiedLineageShape {
        order,
        maximum: Some(maximum),
    })
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
    let mut memo = GitMemo::new(git_root)?;
    let versions = verify_lineage_with_memo::<T>(&mut memo, rel, working_bytes)?;
    memo.verify_head_unchanged()?;
    Ok(versions)
}

fn verify_lineage_with_memo<T: LineageArtifact>(
    memo: &mut GitMemo,
    rel: &str,
    working_bytes: &[u8],
) -> ConformanceResult<usize> {
    let committed = memo.committed_versions(rel)?;
    let ancestry = version_ancestry(&memo.git_root, &committed)?;
    let shape = verify_lineage_shape::<T>(&committed, &ancestry)?;
    let mut previous: Option<(usize, T)> = None;
    let mut decoded_blob_ids = BTreeSet::new();

    for &index in &shape.order {
        let (label, bytes) = &committed[index];
        let blob = memo.cached_blob_ref(label, rel)?;
        if !decoded_blob_ids.insert(blob.object_id.clone()) {
            let facts = memo
                .lineage_facts(T::WHAT, &blob.object_id)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "internal Git memo lost {} lineage facts for blob {}",
                        T::WHAT,
                        blob.object_id
                    )
                })?;
            let Some((older_index, _)) = previous.as_ref() else {
                if !facts.bootstrap {
                    return Err(format!(
                        "{}: oldest reachable version at {label} is not the bootstrap \
                         (missing history? lineage needs the full clone depth)",
                        T::WHAT
                    )
                    .into());
                }
                return Err(format!(
                    "internal {} lineage reused its root blob at {label}",
                    T::WHAT
                )
                .into());
            };
            let (older_label, older_bytes) = &committed[*older_index];
            verify_version_pointer(
                T::WHAT,
                label,
                facts.bootstrap,
                facts.previous.as_ref(),
                older_label,
                older_bytes,
                committed.iter().map(|(commit, _)| commit.clone()),
            )?;
            return Err(format!(
                "internal {} lineage reused blob {} at two material version commits without a \
                 predecessor conflict",
                T::WHAT,
                blob.object_id
            )
            .into());
        }

        #[cfg(test)]
        {
            memo.lineage_decode_misses += 1;
            memo.lineage_peak_live_versions = memo
                .lineage_peak_live_versions
                .max(usize::from(previous.is_some()) + 1);
        }
        let version = T::decode_validated(bytes)
            .map_err(|err| format!("{} version at {label}: {err}", T::WHAT))?;
        memo.remember_lineage_facts(
            T::WHAT,
            &blob.object_id,
            bytes,
            LineageFacts {
                bootstrap: version.bootstrap(),
                previous: version.previous().cloned(),
            },
        )?;
        version.remember_pair_facts(memo, &blob.object_id, bytes)?;

        if let Some((older_index, older)) = previous.as_ref() {
            let (older_label, older_bytes) = &committed[*older_index];
            verify_version_edge::<T>(
                label,
                &version,
                older_label,
                older_bytes,
                older,
                committed.iter().map(|(commit, _)| commit.clone()),
            )?;
        } else if !version.bootstrap() {
            return Err(format!(
                "{}: oldest reachable version at {label} is not the bootstrap \
                 (missing history? lineage needs the full clone depth)",
                T::WHAT
            )
            .into());
        }
        // Assignment drops the prior decoded artifact. At the transient peak
        // only that predecessor and `version` are live.
        previous = Some((index, version));
    }

    let head_bytes = memo.blob_optional("HEAD", rel)?;
    let working_is_version = head_bytes.as_deref() != Some(working_bytes);
    if working_is_version {
        #[cfg(test)]
        {
            memo.lineage_peak_live_versions = memo
                .lineage_peak_live_versions
                .max(usize::from(previous.is_some()) + 1);
        }
        let working = T::decode_validated(working_bytes)
            .map_err(|err| format!("{} version at <working tree>: {err}", T::WHAT))?;
        match shape.maximum {
            None => {
                if !working.bootstrap() {
                    return Err(format!(
                        "{}: oldest reachable version at <working tree> is not the bootstrap",
                        T::WHAT
                    )
                    .into());
                }
            }
            Some(older_index) => {
                let (held_index, older) = previous.as_ref().ok_or_else(|| {
                    format!("internal {} lineage lost its maximum version", T::WHAT)
                })?;
                if *held_index != older_index {
                    return Err(format!(
                        "internal {} lineage retained version {} instead of maximum {}",
                        T::WHAT,
                        committed[*held_index].0,
                        committed[older_index].0
                    )
                    .into());
                }
                let (older_label, older_bytes) = &committed[older_index];
                verify_version_edge::<T>(
                    "<working tree>",
                    &working,
                    older_label,
                    older_bytes,
                    older,
                    committed.iter().map(|(commit, _)| commit.clone()),
                )?;
            }
        }
        Ok(committed.len() + 1)
    } else {
        let Some(maximum) = shape.maximum else {
            return Err(format!("{}: HEAD contains no reachable artifact version", T::WHAT).into());
        };
        if committed[maximum].1.as_slice() != working_bytes {
            return Err(format!(
                "{}: HEAD bytes do not match the unique maximal path version {}",
                T::WHAT,
                committed[maximum].0
            )
            .into());
        }
        Ok(committed.len())
    }
}

pub(crate) fn verify_pair_values(
    label: &str,
    matches: &MatchesArtifact,
    inputs: &OracleInputsArtifact,
    inputs_bytes: &[u8],
) -> ConformanceResult<()> {
    let matches = MatchesPairFacts::from_validated(matches);
    let inputs = InputsPairFacts::from_validated(inputs, inputs_bytes)?;
    verify_pair_facts(label, &matches, &inputs)
}

fn verify_pair_facts(
    label: &str,
    matches: &MatchesPairFacts,
    inputs: &InputsPairFacts,
) -> ConformanceResult<()> {
    if matches.oracle_inputs_sha256 != inputs.blob_sha256 {
        return Err(format!(
            "artifact pair at {label} is incoherent: accepted matches pin a different \
             oracle-inputs blob"
        )
        .into());
    }
    if matches.tsc_js_sha256 != inputs.tsc_js_sha256 {
        return Err(format!(
            "artifact pair at {label} is incoherent: accepted matches and oracle inputs \
             pin different vendored _tsc.js bytes"
        )
        .into());
    }
    if !t1_t3_active(inputs.comparator_state) && matches.has_t1_t3_membership {
        return Err(format!(
            "artifact pair at {label} is incoherent: accepted T1-T3 identities exist while \
             their oracle-input comparators are explicitly absent"
        )
        .into());
    }
    if !t4_active(inputs.comparator_state) && matches.has_t4_membership {
        return Err(format!(
            "artifact pair at {label} is incoherent: accepted T4 case identities exist while \
             their oracle-input comparator is explicitly absent"
        )
        .into());
    }
    Ok(())
}

fn verify_pair_transition(
    label: &str,
    matches: &MatchesArtifact,
    inputs: &OracleInputsArtifact,
) -> ConformanceResult<()> {
    verify_pair_transition_names(label, &matches.transition, &inputs.transition)
}

fn verify_pair_transition_facts(
    label: &str,
    matches: &MatchesPairFacts,
    inputs: &InputsPairFacts,
) -> ConformanceResult<()> {
    verify_pair_transition_names(label, &matches.transition, &inputs.transition)
}

fn verify_pair_transition_names(
    label: &str,
    matches_transition: &Option<String>,
    inputs_transition: &Option<String>,
) -> ConformanceResult<()> {
    if matches_transition != inputs_transition {
        return Err(format!(
            "artifact pair at {label} is incoherent: the oracle-input version records \
             transition {:?} but its same-commit accepted-match version records {:?}",
            inputs_transition, matches_transition
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
    let mut memo = GitMemo::new(git_root)?;
    verify_committed_artifact_pairs_with_memo(&mut memo, matches_rel, inputs_rel)?;
    memo.verify_head_unchanged()
}

fn verify_committed_artifact_pairs_with_memo(
    memo: &mut GitMemo,
    matches_rel: &str,
    inputs_rel: &str,
) -> ConformanceResult<()> {
    let matches_paths = WorkspaceHistoryPaths::new(matches_rel)?;
    let inputs_paths = WorkspaceHistoryPaths::new(inputs_rel)?;
    let input_version_commits = memo
        .committed_versions(&inputs_paths.current)?
        .into_iter()
        .map(|(commit, _)| commit)
        .collect::<BTreeSet<_>>();
    // Walk the combined path history, rather than only the union of
    // each path's material versions. That also exposes a merge which
    // carries matches from one parent and inputs from another.
    let head_commit = memo.head_commit.clone();
    let out = memo.run_git(&[
        "rev-list",
        "--full-history",
        "--topo-order",
        &head_commit,
        "--",
        &matches_paths.current,
        &matches_paths.legacy,
        &inputs_paths.current,
        &inputs_paths.legacy,
    ])?;
    for commit in String::from_utf8(out)?.lines() {
        let commit = commit.trim();
        let matches_blob = memo.blob_optional_with_location(commit, &matches_paths.current)?;
        let inputs_blob = memo.blob_optional_with_location(commit, &inputs_paths.current)?;
        let (Some((matches_bytes, matches_blob)), Some((inputs_bytes, inputs_blob))) =
            (matches_blob, inputs_blob)
        else {
            return Err(format!(
                "incomplete ratchet artifact pair at historical version commit {commit}"
            )
            .into());
        };
        if matches_blob.location != inputs_blob.location {
            return Err(format!(
                "ratchet artifact pair straddles the root and legacy workspaces at historical \
                 version commit {commit}"
            )
            .into());
        }
        let matches = memo.matches_pair_facts(&matches_blob.object_id, &matches_bytes, commit)?;
        let inputs = memo.inputs_pair_facts(&inputs_blob.object_id, &inputs_bytes, commit)?;
        // Cache only the independently validated blob facts. The pair verdict
        // remains commit-local so a merge that combines blobs from different
        // parents cannot inherit a cached success from either parent.
        verify_pair_facts(commit, &matches, &inputs)?;
        if input_version_commits.contains(commit) {
            verify_pair_transition_facts(commit, &matches, &inputs)?;
        }
    }
    Ok(())
}

/// The trusted PR-base compare: HEAD (working) content must contain
/// the resolved base artifact's protected content, so a rewritten
/// branch cannot manufacture a smaller self-consistent chain. The
/// only missing-base exception is the initial bootstrap PR.
#[cfg(test)]
fn verify_baseline(
    git_root: &Path,
    baseline: &str,
    matches_rel: &str,
    inputs_rel: &str,
    head_matches: &MatchesArtifact,
    head_inputs: &OracleInputsArtifact,
) -> ConformanceResult<bool> {
    let mut memo = GitMemo::new(git_root)?;
    let bootstrap = verify_baseline_with_memo(
        &mut memo,
        baseline,
        matches_rel,
        inputs_rel,
        head_matches,
        head_inputs,
    )?;
    memo.verify_head_unchanged()?;
    Ok(bootstrap)
}

fn verify_baseline_with_memo(
    memo: &mut GitMemo,
    baseline: &str,
    matches_rel: &str,
    inputs_rel: &str,
    head_matches: &MatchesArtifact,
    head_inputs: &OracleInputsArtifact,
) -> ConformanceResult<bool> {
    let commit = if baseline == "HEAD" {
        memo.head_commit.clone()
    } else {
        resolve_commit(&memo.git_root, baseline)
            .map_err(|err| format!("baseline compare: {err}"))?
    };

    let base_matches = memo.blob_optional(&commit, matches_rel)?;
    let base_inputs = memo.blob_optional(&commit, inputs_rel)?;
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
    let sanctioned =
        correction_lapses_after_base_with_memo(memo, matches_rel, &commit, head_matches)?;
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
fn correction_lapses_after_base_with_memo(
    memo: &mut GitMemo,
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
        String::from_utf8(git(&memo.git_root, &["rev-list", base_commit])?)?
            .lines()
            .map(|line| line.trim().to_owned())
            .collect();
    for (commit, bytes) in memo.committed_versions(matches_rel)? {
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

fn all_view_tier_counts(views: &RunSets) -> [u64; 3] {
    let mut counts = [0u64; 3];
    let Some(all) = views.get(DiagnosticBand::All.name()) else {
        return counts;
    };
    for cases in all.values() {
        for sets in cases.values() {
            counts[0] += sets.t1.len() as u64;
            counts[1] += sets.t2.len() as u64;
            counts[2] += sets.t3.len() as u64;
        }
    }
    counts
}

fn all_view_t4_count(views: &RunSets) -> u64 {
    views
        .get(DiagnosticBand::All.name())
        .into_iter()
        .flat_map(|fixtures| fixtures.values())
        .flat_map(|cases| cases.values())
        .filter(|sets| sets.t4)
        .count() as u64
}

fn total_case_count(inputs: &OracleInputsArtifact) -> u64 {
    inputs
        .fixtures
        .values()
        .map(|fixture| fixture.cases.len() as u64)
        .sum()
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
    tier_counts: Option<[u64; 3]>,
    t4_counts: Option<(u64, u64)>,
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
    if let Some(tier_counts) = tier_counts {
        let total = totals.get(DiagnosticBand::All.name()).copied().unwrap_or(0);
        for (section_name, matched) in ["t1", "t2", "t3"].into_iter().zip(tier_counts) {
            let section = read_ratchet_section(path, section_name)?;
            let expected_rate = canonical_summary_rate(matched, total);
            if section.matched != Some(matched)
                || section.total != Some(total)
                || section.rate != expected_rate
            {
                return Err(format!(
                    "ratchet.toml [{section_name}] rate/matched/total \
                     ({:.6}/{:?}/{:?}) diverges from the accepted artifact \
                     ({expected_rate:.6}/{matched}/{total}) — run \
                     `cargo xtask ratchet update`",
                    section.rate, section.matched, section.total
                )
                .into());
            }
        }
    }
    if let Some((matched, total)) = t4_counts {
        let section = read_ratchet_section(path, "t4")?;
        let expected_rate = canonical_summary_rate(matched, total);
        if section.matched != Some(matched)
            || section.total != Some(total)
            || section.rate != expected_rate
        {
            return Err(format!(
                "ratchet.toml [t4] rate/matched/total ({:.6}/{:?}/{:?}) diverges from the \
                 accepted case identities ({expected_rate:.6}/{matched}/{total}) — run \
                 `cargo xtask ratchet update`",
                section.rate, section.matched, section.total
            )
            .into());
        }
    }
    Ok(())
}

/// Exact, cheap activation proof consumed by the M8 completion row.
///
/// This deliberately does not rebuild the corpus or walk lineage:
/// `ratchet check` owns those heavier gates. It does decode and
/// validate both current artifacts, verifies their pair pins, requires
/// the atomic T1-T3 comparator state, and proves the three
/// `ratchet.toml` summaries are derived exactly from the accepted
/// bucket sets. Thus hand-writing nonzero TOML counts cannot make the
/// completion consumer report the tier schema active.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tier1Through3Activation {
    pub t1_matched: u64,
    pub t2_matched: u64,
    pub t3_matched: u64,
    pub total: u64,
}

pub fn verify_tier_1_through_3_activation(
    workspace: &Path,
) -> ConformanceResult<Tier1Through3Activation> {
    let (matches, _matches_bytes): (MatchesArtifact, _) =
        read_artifact(&workspace.join(MATCHES_REL_PATH), "accepted-match artifact")?;
    matches.validate()?;
    let (inputs, inputs_bytes): (OracleInputsArtifact, _) = read_artifact(
        &workspace.join(ORACLE_INPUTS_REL_PATH),
        "oracle-inputs artifact",
    )?;
    inputs.validate()?;
    verify_pair_values("<working tree>", &matches, &inputs, &inputs_bytes)?;
    if !t1_t3_active(comparator_state(&inputs.comparators)?) {
        return Err(format!(
            "A1 T1-T3 accepted sets are inactive: oracle-input comparators remain explicit \
             \"absent\" markers; run the reviewed \
             `ratchet update --transition {TIER_1_3_INPUT_SCHEMA_EXTENSION}` only after \
             supported T0-T3 closure"
        )
        .into());
    }

    let view_counts = view_counts(&matches.views);
    let tier_counts = all_view_tier_counts(&matches.views);
    verify_ratchet_summaries(
        &workspace.join("ratchet.toml"),
        &view_counts,
        &inputs.totals,
        Some(tier_counts),
        t4_active(comparator_state(&inputs.comparators)?)
            .then(|| (all_view_t4_count(&matches.views), total_case_count(&inputs))),
    )?;
    Ok(Tier1Through3Activation {
        t1_matched: tier_counts[0],
        t2_matched: tier_counts[1],
        t3_matched: tier_counts[2],
        total: inputs
            .totals
            .get(DiagnosticBand::All.name())
            .copied()
            .unwrap_or(0),
    })
}

/// Fresh, Node-free A3 activation proof consumed after completion's
/// ordinary full conformance run. That run has already enforced
/// accepted T4 case losses; this consumer rebuilds the current
/// schema-3 oracle-input manifest and proves the comparator, pair,
/// renderer, and every per-case oracle pin before checking that the
/// `[t4]` summary is exactly derived from the accepted artifact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct T4Activation {
    pub matched_cases: u64,
    pub total_cases: u64,
}

pub fn verify_t4_activation(workspace: &Path) -> ConformanceResult<T4Activation> {
    // Freshly rebuild the manifest view from the current schema-3
    // goldens and producer files. This proves the accepted pair pin,
    // every genuine per-case oracle hash, and the separate renderer
    // producer pin instead of trusting a hand-written summary.
    let (matches, _, inputs, _) = verify_current_pair(workspace)?;
    let state = comparator_state(&inputs.comparators)?;
    if !t4_active(state) {
        return Err(format!(
            "A3 T4 accepted cases are inactive: oracle-input comparator remains explicit \
             \"absent\"; run the reviewed `ratchet update --transition \
             {T4_INPUT_SCHEMA_EXTENSION}` only after A2 freeze"
        )
        .into());
    }
    let matched_cases = all_view_t4_count(&matches.views);
    let total_cases = total_case_count(&inputs);
    verify_ratchet_summaries(
        &workspace.join("ratchet.toml"),
        &view_counts(&matches.views),
        &inputs.totals,
        Some(all_view_tier_counts(&matches.views)),
        Some((matched_cases, total_cases)),
    )?;
    Ok(T4Activation {
        matched_cases,
        total_cases,
    })
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

/// Verify the accepted pair's append-only Git history without rebuilding the
/// current oracle-input manifest. H0 closure evidence uses this narrower
/// proof before trusting an artifact inherited at a historical closing
/// commit. The ordinary ratchet gate still performs the stronger current-tree
/// input diff through [`verify_current_pair`].
#[cfg(test)]
pub(crate) fn verify_accepted_pair_history(workspace: &Path) -> ConformanceResult<()> {
    verify_accepted_pair_history_with_proof(workspace).map(drop)
}

pub(crate) fn verify_accepted_pair_history_with_proof(
    workspace: &Path,
) -> ConformanceResult<AcceptedPairHistoryProof> {
    let (matches, matches_bytes): (MatchesArtifact, _) =
        read_artifact(&workspace.join(MATCHES_REL_PATH), "accepted-match artifact")?;
    matches.validate()?;
    let (inputs, inputs_bytes): (OracleInputsArtifact, _) = read_artifact(
        &workspace.join(ORACLE_INPUTS_REL_PATH),
        "oracle-inputs artifact",
    )?;
    inputs.validate()?;
    verify_pair_values("<working tree>", &matches, &inputs, &inputs_bytes)?;

    let git_root = git_root_for(workspace)?;
    let matches_rel = git_rel_path(&git_root, workspace, MATCHES_REL_PATH)?;
    let inputs_rel = git_rel_path(&git_root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let mut git_memo = GitMemo::new(&git_root)?;
    // Transition-name equality is required only on an oracle-input version
    // edge. A later accepted-match-only growth pins the same manifest bytes
    // but correctly records no new input transition, matching the committed
    // history rule in verify_committed_artifact_pairs_with_memo.
    let working_inputs_differs =
        git_memo.blob_optional("HEAD", &inputs_rel)?.as_deref() != Some(&inputs_bytes);
    if working_inputs_differs {
        verify_pair_transition("<working tree>", &matches, &inputs)?;
    }
    verify_lineage_with_memo::<MatchesArtifact>(&mut git_memo, &matches_rel, &matches_bytes)?;
    verify_lineage_with_memo::<OracleInputsArtifact>(&mut git_memo, &inputs_rel, &inputs_bytes)?;
    verify_committed_artifact_pairs_with_memo(&mut git_memo, &matches_rel, &inputs_rel)?;
    AcceptedPairHistoryProof::from_verified_history(
        workspace,
        &mut git_memo,
        &matches_rel,
        &inputs_rel,
        &matches_bytes,
        &inputs_bytes,
    )
}

/// `cargo xtask ratchet check [--baseline <ref>]`: verify both
/// artifacts against the current tree (vendor pins, fixture bytes,
/// expansion, golden oracle records, ratchet.toml derived summaries)
/// and their full append-only lineage; with `--baseline`, also the
/// trusted PR-base direct compare.
pub fn check(workspace: &Path, baseline: Option<&str>) -> ConformanceResult<()> {
    check_with_history_proof(workspace, baseline).map(drop)
}

/// Perform [`check`] and retain an opaque, repository-bound proof for another
/// audit in the same process. This lets dependent validators reuse the
/// successful blob-ID history decode without retaining the decoded artifacts.
pub fn check_with_history_proof(
    workspace: &Path,
    baseline: Option<&str>,
) -> ConformanceResult<AcceptedPairHistoryProof> {
    let (matches, matches_bytes, inputs, inputs_bytes) = verify_current_pair(workspace)?;

    // ratchet.toml counts are derived summaries of the artifact, never
    // an independent authority.
    let counts = view_counts(&matches.views);
    let comparator_state = comparator_state(&inputs.comparators)?;
    let tier_counts = t1_t3_active(comparator_state).then(|| all_view_tier_counts(&matches.views));
    let t4_counts = t4_active(comparator_state)
        .then(|| (all_view_t4_count(&matches.views), total_case_count(&inputs)));
    verify_ratchet_summaries(
        &workspace.join("ratchet.toml"),
        &counts,
        &inputs.totals,
        tier_counts,
        t4_counts,
    )?;

    let git_root = git_root_for(workspace)?;
    let matches_rel = git_rel_path(&git_root, workspace, MATCHES_REL_PATH)?;
    let inputs_rel = git_rel_path(&git_root, workspace, ORACLE_INPUTS_REL_PATH)?;
    let mut git_memo = GitMemo::new(&git_root)?;
    let working_inputs_differs =
        git_memo.blob_optional("HEAD", &inputs_rel)?.as_deref() != Some(&inputs_bytes);
    if working_inputs_differs {
        verify_pair_transition("<working tree>", &matches, &inputs)?;
    }
    let matches_versions =
        verify_lineage_with_memo::<MatchesArtifact>(&mut git_memo, &matches_rel, &matches_bytes)?;
    let inputs_versions = verify_lineage_with_memo::<OracleInputsArtifact>(
        &mut git_memo,
        &inputs_rel,
        &inputs_bytes,
    )?;
    verify_committed_artifact_pairs_with_memo(&mut git_memo, &matches_rel, &inputs_rel)?;

    let bootstrap_base = if let Some(baseline) = baseline {
        verify_baseline_with_memo(
            &mut git_memo,
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
        let options = ConformanceOptions {
            workspace: workspace.to_owned(),
            limit: None,
            files: Vec::new(),
            out_json: workspace.join("target/conformance/bootstrap-check.json"),
            band: DiagnosticBand::All,
        };
        let run = if t4_active(comparator_state) {
            super::run_conformance_collect_with_t4(&options, None, None)?
        } else {
            super::run_conformance_collect(&options)?
        };
        verify_bootstrap_measurement(&matches.views, &run.sets)?;
    }
    let history_proof = AcceptedPairHistoryProof::from_verified_history(
        workspace,
        &mut git_memo,
        &matches_rel,
        &inputs_rel,
        &matches_bytes,
        &inputs_bytes,
    )?;

    let describe = |view: DiagnosticBand| {
        let (matched, complete) = counts.get(view.name()).copied().unwrap_or((0, 0));
        let total = inputs.totals.get(view.name()).copied().unwrap_or(0);
        format!("{}={matched}/{total} (complete {complete})", view.name())
    };
    println!(
        "ratchet check ok: {} {} {}; tiers={}; t4={}; fixtures={} versions matches={matches_versions} inputs={inputs_versions} baseline={}",
        describe(DiagnosticBand::All),
        describe(DiagnosticBand::TwoXxx),
        describe(DiagnosticBand::Syntactic),
        tier_counts.map_or_else(
            || "inactive".to_owned(),
            |counts| format!(
                "T1={}/{} T2={}/{} T3={}/{}",
                counts[0],
                inputs.totals[DiagnosticBand::All.name()],
                counts[1],
                inputs.totals[DiagnosticBand::All.name()],
                counts[2],
                inputs.totals[DiagnosticBand::All.name()],
            )
        ),
        t4_counts.map_or_else(
            || "inactive".to_owned(),
            |(matched, total)| format!("{matched}/{total}")
        ),
        inputs.fixtures.len(),
        baseline.unwrap_or("none"),
    );
    Ok(history_proof)
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
            TIER_1_3_INPUT_SCHEMA_EXTENSION,
            T4_INPUT_SCHEMA_EXTENSION,
        ]
        .contains(&transition)
        {
            return Err(format!(
                "unknown transition {transition:?} (A1 knows {UNIVERSE_TRANSITION:?}, \
                 {PRODUCER_PIN_EXTENSION:?}, {ORACLE_CORRECTION:?}, and \
                 {TIER_1_3_INPUT_SCHEMA_EXTENSION:?}, and \
                 {T4_INPUT_SCHEMA_EXTENSION:?})"
            )
            .into());
        }
    }

    // The one-time A3 plan runs the separately pinned renderer first,
    // but performs no write. Its schema-3 golden bytes, manifest pins,
    // accepted T4 cases, and summaries commit together only after every
    // lineage/transition preflight below succeeds.
    let render_plan = if transition == Some(T4_INPUT_SCHEMA_EXTENSION) {
        Some(super::rendered::plan_rendered_hash_extension(
            &RefreshOptions {
                workspace: workspace.to_owned(),
                limit: None,
                files: Vec::new(),
            },
        )?)
    } else {
        None
    };
    let built =
        build_oracle_inputs_with_t4_pins(workspace, render_plan.as_ref().map(|plan| &plan.pins))?;
    let built_comparator_state = comparator_state(&built.comparators)?;
    let conformance_options = ConformanceOptions {
        workspace: workspace.to_owned(),
        limit: None,
        files: Vec::new(),
        out_json: workspace.join("target/conformance/mismatches.json"),
        band: DiagnosticBand::All,
    };
    let run = if t4_active(built_comparator_state) {
        super::run_conformance_collect_with_t4(
            &conformance_options,
            render_plan.as_ref().map(|plan| &plan.pins),
            render_plan
                .as_ref()
                .map(|plan| &plan.empty_related_information),
        )?
    } else {
        super::run_conformance_collect(&conformance_options)?
    };
    if run.summary.false_positive_diagnostics > 0 {
        return Err(format!(
            "refusing to accept a state with {} false positive diagnostic(s)",
            run.summary.false_positive_diagnostics
        )
        .into());
    }
    if transition == Some(T4_INPUT_SCHEMA_EXTENSION)
        && (run.summary.scope_status != "frozen" || run.summary.scope_resolved_t0_diagnostics > 0)
    {
        return Err(format!(
            "{T4_INPUT_SCHEMA_EXTENSION} requires the globally frozen A2 scope and zero live \
             resolved exclusions (status={}, resolved={})",
            run.summary.scope_status, run.summary.scope_resolved_t0_diagnostics
        )
        .into());
    }

    let git_root = git_root_for(workspace)?;
    let vendor = built.vendor.clone();
    let totals = built.totals.clone();
    let total_cases = total_case_count(&built);
    let tier_comparators_active = t1_t3_active(built_comparator_state);

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
                     producer-pin-extension`; activating the M8 T1-T3 comparators needs \
                     `--transition tier1-3-input-schema-extension`; a vendor or comparator \
                     semantic change is a separate project"
                        .into(),
                );
            };
            match transition {
                UNIVERSE_TRANSITION => verify_universe_growth(reference, &built)?,
                PRODUCER_PIN_EXTENSION => verify_producer_pin_extension(reference, &built)?,
                ORACLE_CORRECTION => verify_producer_correction(reference, &built)?,
                TIER_1_3_INPUT_SCHEMA_EXTENSION => {
                    verify_tier_1_3_input_schema_extension(reference, &built)?
                }
                T4_INPUT_SCHEMA_EXTENSION => verify_t4_input_schema_extension(reference, &built)?,
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
    let effective_tier_activation =
        inputs_transition.as_deref() == Some(TIER_1_3_INPUT_SCHEMA_EXTENSION);
    if transition == Some(TIER_1_3_INPUT_SCHEMA_EXTENSION) && !effective_tier_activation {
        return Err(format!(
            "{TIER_1_3_INPUT_SCHEMA_EXTENSION} requires an inactive T1-T3 manifest to \
             activate; the current input content has no such one-time transition"
        )
        .into());
    }
    let effective_t4_activation = inputs_transition.as_deref() == Some(T4_INPUT_SCHEMA_EXTENSION);
    if transition == Some(T4_INPUT_SCHEMA_EXTENSION) && !effective_t4_activation {
        return Err(format!(
            "{T4_INPUT_SCHEMA_EXTENSION} requires a T1-T3 manifest with T4 absent; the \
             current input content has no such one-time transition"
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
    let old_tier_counts = existing_matches
        .as_ref()
        .map(|(artifact, _)| all_view_tier_counts(&artifact.views))
        .unwrap_or_default();
    let old_t4_count = existing_matches
        .as_ref()
        .map(|(artifact, _)| all_view_t4_count(&artifact.views))
        .unwrap_or(0);
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
    let tier_counts = tier_comparators_active.then(|| all_view_tier_counts(&run.sets));
    let t4_counts =
        t4_active(built_comparator_state).then(|| (all_view_t4_count(&run.sets), total_cases));
    let (original_ratchet, ratchet_update) =
        render_ratchet_summaries(&ratchet_path, &counts, &totals, tier_counts, t4_counts)?;
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
            if let Some(plan) = &render_plan {
                for golden in &plan.updates {
                    updates.push(AtomicFileUpdate {
                        path: &golden.path,
                        original: Some(&golden.original),
                        replacement: &golden.replacement,
                    });
                }
            }
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
    if let Some(plan) = &render_plan {
        for golden in &plan.updates {
            updates.push(AtomicFileUpdate {
                path: &golden.path,
                original: Some(&golden.original),
                replacement: &golden.replacement,
            });
        }
    }
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
    if let Some(tier_counts) = tier_counts {
        for (tier, old, new) in ["T1", "T2", "T3"]
            .into_iter()
            .zip(old_tier_counts)
            .zip(tier_counts)
            .map(|((tier, old), new)| (tier, old, new))
        {
            println!(
                "ratchet update {tier}: matched {old} -> {new} ({:+})",
                new as i64 - old as i64
            );
        }
    }
    if let Some((t4_count, total)) = t4_counts {
        println!(
            "ratchet update T4: matched {old_t4_count} -> {t4_count} ({:+}) / {total} cases",
            t4_count as i64 - old_t4_count as i64
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
    if let Some(plan) = &render_plan {
        println!(
            "ratchet update T4: atomically upgraded {} golden fixture(s), {} case(s), {} \
             oracle diagnostic(s)",
            plan.summary.schema_2_upgraded, plan.summary.cases, plan.summary.oracle_diagnostics
        );
    }
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
    tier_counts: Option<[u64; 3]>,
) -> ConformanceResult<()> {
    let (_, rendered) = render_ratchet_summaries(path, counts, totals, tier_counts, None)?;
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
    tier_counts: Option<[u64; 3]>,
    t4_counts: Option<(u64, u64)>,
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
    if let Some(tier_counts) = tier_counts {
        let total = totals.get(DiagnosticBand::All.name()).copied().unwrap_or(0);
        for (section, matched) in ["t1", "t2", "t3"].into_iter().zip(tier_counts) {
            let table = document
                .as_table_mut()
                .get_mut(section)
                .and_then(Item::as_table_mut)
                .ok_or_else(|| {
                    format!(
                        "missing active-tier ratchet summary section [{section}] in {}",
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
    }
    if let Some((matched, total)) = t4_counts {
        if !document.as_table().contains_key("t4") {
            document
                .as_table_mut()
                .insert("t4", Item::Table(Table::new()));
        }
        let table = document
            .as_table_mut()
            .get_mut("t4")
            .and_then(Item::as_table_mut)
            .expect("T4 table inserted above");
        let rate = canonical_summary_rate(matched, total);
        let matched =
            i64::try_from(matched).map_err(|_| "[t4].matched exceeds TOML's integer range")?;
        let total = i64::try_from(total).map_err(|_| "[t4].total exceeds TOML's integer range")?;
        set_summary_value(table, "t4", "rate", toml_value(rate))?;
        set_summary_value(table, "t4", "matched", toml_value(matched))?;
        set_summary_value(table, "t4", "total", toml_value(total))?;
        set_summary_value(table, "t4", "allowed_regression", toml_value(0.0))?;
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
#[path = "../tests/unit/ratchet/tests.rs"]
mod tests;
