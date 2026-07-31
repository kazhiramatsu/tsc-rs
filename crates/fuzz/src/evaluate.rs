//! Atomic raw-evidence, comparison, and class evaluation.
//!
//! The producer-facing path validates and indexes a [`CaseSpec`] once, then
//! derives every downstream artifact from that same context. This prevents a
//! caller from accidentally hashing one execution while comparing or
//! classifying another.

use crate::classify::{classify_validated, CanonicalClass};
use crate::compare::{compare_validated, Comparison};
use crate::model::CaseExecution;
use crate::normalize::NormalizationSpec;
use crate::schema::{sha256_hex, CaseSpec};
use crate::FoundationResult;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvaluatedCase {
    /// Exact schema-1 `CaseExecution` envelope, including the CaseSpec hash.
    execution_canonical_bytes: Vec<u8>,
    execution_sha256: String,
    comparison: Comparison,
    canonical_class: Option<CanonicalClass>,
}

impl EvaluatedCase {
    pub fn execution_canonical_bytes(&self) -> &[u8] {
        &self.execution_canonical_bytes
    }

    pub fn execution_sha256(&self) -> &str {
        &self.execution_sha256
    }

    pub const fn comparison(&self) -> &Comparison {
        &self.comparison
    }

    pub const fn canonical_class(&self) -> Option<&CanonicalClass> {
        self.canonical_class.as_ref()
    }
}

/// Produce the raw execution envelope, structured comparison, and canonical
/// class through one validated CaseSpec context.
///
/// Producer failures and non-reviewed oracle terminal outcomes still return
/// an error because they cannot be counted as a comparison. Their typed raw
/// evidence remains serializable through [`CaseExecution::canonical_bytes`].
pub fn evaluate_case(
    case: &CaseSpec,
    execution: &CaseExecution,
) -> FoundationResult<EvaluatedCase> {
    let validated = case.validated_context()?;
    execution.validate_with_context(&validated)?;
    let normalization = NormalizationSpec::for_validated_case(validated.case())?;
    let execution_canonical_bytes = execution.canonical_bytes_after_validation(&validated)?;
    let execution_sha256 = sha256_hex(&execution_canonical_bytes);
    let comparison = compare_validated(execution, &validated, &normalization)?;
    let canonical_class = classify_validated(&comparison, &normalization)?;

    Ok(EvaluatedCase {
        execution_canonical_bytes,
        execution_sha256,
        comparison,
        canonical_class,
    })
}
