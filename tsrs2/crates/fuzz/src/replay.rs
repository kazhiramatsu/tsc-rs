//! Canonical saved observations for true replay.
//!
//! This module deliberately owns only the pure artifact boundary. Engine
//! adapters load a [`ReplayArtifact`], call [`ReplayArtifact::verify_saved`]
//! before launching anything, then compare a newly evaluated execution with
//! the saved comparison and class. Process execution belongs outside this
//! module.

use serde::{Deserialize, Serialize};

use crate::classify::CanonicalClass;
use crate::compare::Comparison;
use crate::evaluate::{evaluate_case, EvaluatedCase};
use crate::model::CaseExecution;
use crate::schema::{sha256_hex, CaseSpec};
use crate::{FoundationError, FoundationResult};

pub const REPLAY_ARTIFACT_SCHEMA: u32 = 1;
pub const REPLAY_COMPARATOR_SCHEMA: u32 = 1;
pub const REPLAY_COMPARATOR_ID: &str = "tier-first-t0-set-t1-t3-multiset-t4-v1";

/// One saved observation, including every input needed to reconstruct the
/// program and every derived value that true replay must reproduce.
///
/// The hashes are not trusted summaries. Validation reserializes the
/// `CaseSpec`, reevaluates the `CaseExecution`, and requires both hashes,
/// the complete structured comparison, and the canonical class to match.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayArtifact {
    pub schema: u32,
    pub comparator_schema: u32,
    pub comparator_id: String,
    pub case_sha256: String,
    pub case: CaseSpec,
    pub saved_execution_sha256: String,
    pub saved_execution: CaseExecution,
    pub comparison: Comparison,
    pub canonical_class: Option<CanonicalClass>,
}

impl ReplayArtifact {
    /// Capture a saved observation only through the atomic evaluation path.
    ///
    /// Producer failures and unreviewed oracle terminal outcomes are rejected
    /// by `evaluate_case`; they cannot be turned into replay evidence merely
    /// by serializing them.
    pub fn from_observation(case: &CaseSpec, execution: &CaseExecution) -> FoundationResult<Self> {
        let case_sha256 = case.canonical_sha256()?;
        let evaluated = evaluate_case(case, execution)?;
        let artifact = Self {
            schema: REPLAY_ARTIFACT_SCHEMA,
            comparator_schema: REPLAY_COMPARATOR_SCHEMA,
            comparator_id: REPLAY_COMPARATOR_ID.to_owned(),
            case_sha256,
            case: case.clone(),
            saved_execution_sha256: evaluated.execution_sha256().to_owned(),
            saved_execution: execution.clone(),
            comparison: evaluated.comparison().clone(),
            canonical_class: evaluated.canonical_class().cloned(),
        };
        artifact.verify_saved()?;
        Ok(artifact)
    }

    /// Recompute every saved projection from the embedded raw observation.
    ///
    /// This is the mandatory pre-launch check for a future process executor.
    /// It prevents a CaseSpec, raw execution, comparison, or class from being
    /// spliced across independently valid observations.
    pub fn verify_saved(&self) -> FoundationResult<EvaluatedCase> {
        if self.schema != REPLAY_ARTIFACT_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported replay artifact schema {}; expected {REPLAY_ARTIFACT_SCHEMA}",
                self.schema
            )));
        }
        if self.comparator_schema != REPLAY_COMPARATOR_SCHEMA {
            return Err(FoundationError::new(format!(
                "unsupported replay comparator schema {}; expected {REPLAY_COMPARATOR_SCHEMA}",
                self.comparator_schema
            )));
        }
        if self.comparator_id != REPLAY_COMPARATOR_ID {
            return Err(FoundationError::new(format!(
                "unsupported replay comparator id {:?}; expected {REPLAY_COMPARATOR_ID:?}",
                self.comparator_id
            )));
        }

        let actual_case_sha256 = self.case.canonical_sha256()?;
        if self.case_sha256 != actual_case_sha256 {
            return Err(FoundationError::new(format!(
                "replay artifact CaseSpec hash mismatch: expected {actual_case_sha256}, found {}",
                self.case_sha256
            )));
        }

        self.comparison.validate()?;
        if let Some(class) = &self.canonical_class {
            class.validate()?;
        }

        let evaluated = evaluate_case(&self.case, &self.saved_execution)?;
        if self.saved_execution_sha256 != evaluated.execution_sha256() {
            return Err(FoundationError::new(format!(
                "replay artifact saved execution hash mismatch: expected {}, found {}",
                evaluated.execution_sha256(),
                self.saved_execution_sha256
            )));
        }
        if self.comparison != *evaluated.comparison() {
            return Err(FoundationError::new(
                "replay artifact comparison does not match the saved execution",
            ));
        }
        if self.canonical_class.as_ref() != evaluated.canonical_class() {
            return Err(FoundationError::new(
                "replay artifact canonical class does not match the saved execution",
            ));
        }
        Ok(evaluated)
    }

    /// Evaluate one newly executed observation against the saved comparator
    /// and class.
    ///
    /// The new raw execution hash is intentionally not compared with
    /// `saved_execution_sha256`: common rows and other raw provenance outside
    /// the selected comparator remain fresh evidence, while replay success is
    /// defined by the exact saved structured comparison and canonical class.
    pub fn verify_replayed_execution(
        &self,
        replayed_execution: &CaseExecution,
    ) -> FoundationResult<EvaluatedCase> {
        self.verify_saved()?;
        let evaluated = evaluate_case(&self.case, replayed_execution)?;
        if self.comparison != *evaluated.comparison() {
            return Err(FoundationError::new(
                "replayed comparison does not match the saved comparison",
            ));
        }
        if self.canonical_class.as_ref() != evaluated.canonical_class() {
            return Err(FoundationError::new(
                "replayed canonical class does not match the saved canonical class",
            ));
        }
        Ok(evaluated)
    }

    pub fn canonical_bytes(&self) -> FoundationResult<Vec<u8>> {
        self.verify_saved()?;
        serde_json::to_vec(self).map_err(|error| {
            FoundationError::new(format!("cannot serialize replay artifact: {error}"))
        })
    }

    pub fn canonical_sha256(&self) -> FoundationResult<String> {
        Ok(sha256_hex(&self.canonical_bytes()?))
    }

    pub fn from_json_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let artifact: Self = serde_json::from_slice(bytes).map_err(|error| {
            FoundationError::new(format!("invalid replay artifact JSON: {error}"))
        })?;
        artifact.verify_saved()?;
        Ok(artifact)
    }

    pub fn from_canonical_slice(bytes: &[u8]) -> FoundationResult<Self> {
        let artifact = Self::from_json_slice(bytes)?;
        if serde_json::to_vec(&artifact).map_err(|error| {
            FoundationError::new(format!("cannot reserialize replay artifact: {error}"))
        })? != bytes
        {
            return Err(FoundationError::new(
                "replay artifact input is valid JSON but not canonical compact schema-1 bytes",
            ));
        }
        Ok(artifact)
    }
}
