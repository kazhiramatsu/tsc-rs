//! Report-only identity snapshots and before/after diffs for the
//! T1/T2/T3 observations (phase 9.3c). After M8 activation A1 gates
//! its own per-case copies of these bucket sets; this report remains
//! evidence-only and never mutates accepted state.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{ConformanceResult, T0Key};

const OBSERVATION_SCHEMA: u32 = 1;
const DIFF_SCHEMA: u32 = 1;

/// A shadow-tier bucket identity. Tier grading is bucket-granular, so
/// fixture + matrix case + T0 key is the complete stable identity of
/// one matched tier row.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct ShadowTierIdentity {
    pub fixture: String,
    pub matrix_key: String,
    pub diagnostic: T0Key,
}

/// Exact matched identities for one conformance observation.
///
/// The vectors deliberately repeat identities across tiers. That makes
/// each tier independently diffable and preserves the grading contract
/// that T1 is not reconstructed from T2/T3 pairings.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShadowTierObservation {
    pub schema: u32,
    pub oracle_universe_sha256: String,
    pub t1_matched: Vec<ShadowTierIdentity>,
    pub t2_matched: Vec<ShadowTierIdentity>,
    pub t3_matched: Vec<ShadowTierIdentity>,
}

impl ShadowTierObservation {
    pub(crate) fn new(
        oracle_records: Vec<Vec<u8>>,
        t1_matched: BTreeSet<ShadowTierIdentity>,
        t2_matched: BTreeSet<ShadowTierIdentity>,
        t3_matched: BTreeSet<ShadowTierIdentity>,
    ) -> Self {
        Self {
            schema: OBSERVATION_SCHEMA,
            oracle_universe_sha256: oracle_universe_sha256(oracle_records),
            t1_matched: t1_matched.into_iter().collect(),
            t2_matched: t2_matched.into_iter().collect(),
            t3_matched: t3_matched.into_iter().collect(),
        }
    }

    pub(crate) fn validate(&self, label: &str, counts: [usize; 3]) -> ConformanceResult<()> {
        if self.schema != OBSERVATION_SCHEMA {
            return Err(format!(
                "{label}: unsupported shadow-tier observation schema {}, expected {}",
                self.schema, OBSERVATION_SCHEMA
            )
            .into());
        }
        let tiers = [
            ("T1", &self.t1_matched, counts[0]),
            ("T2", &self.t2_matched, counts[1]),
            ("T3", &self.t3_matched, counts[2]),
        ];
        for (tier, identities, count) in tiers {
            if identities.len() != count {
                return Err(format!(
                    "{label}: {tier} identity count {} disagrees with aggregate matched count {count}",
                    identities.len()
                )
                .into());
            }
            if !identities.windows(2).all(|pair| pair[0] < pair[1]) {
                return Err(format!(
                    "{label}: {tier} identities must be strictly sorted and unique"
                )
                .into());
            }
        }

        let t1 = self.t1_matched.iter().collect::<BTreeSet<_>>();
        let t2 = self.t2_matched.iter().collect::<BTreeSet<_>>();
        let t3 = self.t3_matched.iter().collect::<BTreeSet<_>>();
        if !t2.is_subset(&t1) {
            return Err(format!("{label}: T2 identities are not a subset of T1").into());
        }
        if !t3.is_subset(&t2) {
            return Err(format!("{label}: T3 identities are not a subset of T2").into());
        }
        Ok(())
    }
}

fn oracle_universe_sha256(mut records: Vec<Vec<u8>>) -> String {
    records.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"tsrs2-shadow-tier-oracle-universe-v1\0");
    for record in records {
        hasher.update((record.len() as u64).to_be_bytes());
        hasher.update(record);
    }
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Deserialize)]
struct ConformanceDiffInput {
    band: String,
    shadow_t1_matched: usize,
    shadow_t2_matched: usize,
    shadow_t3_matched: usize,
    supported_t1_matched: usize,
    supported_t2_matched: usize,
    supported_t3_matched: usize,
    shadow_tier_identities: ShadowTierObservation,
    supported_shadow_tier_identities: ShadowTierObservation,
}

impl ConformanceDiffInput {
    fn validate(&self, label: &str) -> ConformanceResult<()> {
        self.shadow_tier_identities.validate(
            &format!("{label} all-corpus"),
            [
                self.shadow_t1_matched,
                self.shadow_t2_matched,
                self.shadow_t3_matched,
            ],
        )?;
        self.supported_shadow_tier_identities.validate(
            &format!("{label} supported"),
            [
                self.supported_t1_matched,
                self.supported_t2_matched,
                self.supported_t3_matched,
            ],
        )
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ShadowTierDiff {
    pub before_matched: usize,
    pub after_matched: usize,
    pub lost: Vec<ShadowTierIdentity>,
    pub gained: Vec<ShadowTierIdentity>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShadowTierSetDiff {
    pub t1: ShadowTierDiff,
    pub t2: ShadowTierDiff,
    pub t3: ShadowTierDiff,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConformanceDiffReport {
    pub schema: u32,
    pub band: String,
    pub oracle_universe_sha256: String,
    pub supported_oracle_universe_before_sha256: String,
    pub supported_oracle_universe_after_sha256: String,
    pub supported_oracle_universe_unchanged: bool,
    pub all_corpus: ShadowTierSetDiff,
    pub supported: ShadowTierSetDiff,
}

/// Compare two `conformance --out-json` observations without changing
/// any ratchet or accepted-state artifact.
pub fn conformance_diff(
    before_path: &Path,
    after_path: &Path,
) -> ConformanceResult<ConformanceDiffReport> {
    let before: ConformanceDiffInput =
        serde_json::from_slice(&fs::read(before_path)?).map_err(|error| {
            format!(
                "invalid before conformance report {}: {error}",
                before_path.display()
            )
        })?;
    let after: ConformanceDiffInput =
        serde_json::from_slice(&fs::read(after_path)?).map_err(|error| {
            format!(
                "invalid after conformance report {}: {error}",
                after_path.display()
            )
        })?;
    diff_observations(before, after)
}

fn diff_observations(
    before: ConformanceDiffInput,
    after: ConformanceDiffInput,
) -> ConformanceResult<ConformanceDiffReport> {
    before.validate("before report")?;
    after.validate("after report")?;
    if before.band != after.band {
        return Err(format!(
            "conformance reports use different bands: before={} after={}",
            before.band, after.band
        )
        .into());
    }
    if before.shadow_tier_identities.oracle_universe_sha256
        != after.shadow_tier_identities.oracle_universe_sha256
    {
        return Err(
            "conformance reports use different all-corpus oracle universes; rerun both observations against the same goldens and projection"
                .into(),
        );
    }

    let supported_before = before
        .supported_shadow_tier_identities
        .oracle_universe_sha256
        .clone();
    let supported_after = after
        .supported_shadow_tier_identities
        .oracle_universe_sha256
        .clone();
    Ok(ConformanceDiffReport {
        schema: DIFF_SCHEMA,
        band: before.band,
        oracle_universe_sha256: before.shadow_tier_identities.oracle_universe_sha256.clone(),
        supported_oracle_universe_unchanged: supported_before == supported_after,
        supported_oracle_universe_before_sha256: supported_before,
        supported_oracle_universe_after_sha256: supported_after,
        all_corpus: diff_tier_sets(
            &before.shadow_tier_identities,
            &after.shadow_tier_identities,
        ),
        supported: diff_tier_sets(
            &before.supported_shadow_tier_identities,
            &after.supported_shadow_tier_identities,
        ),
    })
}

fn diff_tier_sets(
    before: &ShadowTierObservation,
    after: &ShadowTierObservation,
) -> ShadowTierSetDiff {
    ShadowTierSetDiff {
        t1: diff_tier(&before.t1_matched, &after.t1_matched),
        t2: diff_tier(&before.t2_matched, &after.t2_matched),
        t3: diff_tier(&before.t3_matched, &after.t3_matched),
    }
}

fn diff_tier(before: &[ShadowTierIdentity], after: &[ShadowTierIdentity]) -> ShadowTierDiff {
    let before_set = before.iter().cloned().collect::<BTreeSet<_>>();
    let after_set = after.iter().cloned().collect::<BTreeSet<_>>();
    ShadowTierDiff {
        before_matched: before.len(),
        after_matched: after.len(),
        lost: before_set.difference(&after_set).cloned().collect(),
        gained: after_set.difference(&before_set).cloned().collect(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/shadow_diff/tests.rs"]
mod tests;
