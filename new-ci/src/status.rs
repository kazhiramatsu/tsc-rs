//! Typed attempt receipts and verified-success cache eligibility.
//!
//! A producer first creates a [`SuccessCandidate`].  The only public path to a
//! success status hashes both output projections and returns a private-marker
//! [`VerifiedOutputs`] value. Failed, cancelled, timed-out, and diagnostic
//! attempts remain first-class receipts but cannot yield a [`CacheHit`].

use crate::{sha256, Digest, ExecutionMetadata, Projection, ReceiptKey, ReceiptOutputs};
use std::error::Error;
use std::fmt;

/// Semantic identity plus execution-only provenance shared by every status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptContext {
    key: ReceiptKey,
    attempt: u64,
    execution: ExecutionMetadata,
}

impl AttemptContext {
    pub const fn new(key: ReceiptKey, attempt: u64, execution: ExecutionMetadata) -> Self {
        Self {
            key,
            attempt,
            execution,
        }
    }

    pub const fn key(&self) -> ReceiptKey {
        self.key
    }

    pub const fn attempt(&self) -> u64 {
        self.attempt
    }

    pub const fn execution(&self) -> &ExecutionMetadata {
        &self.execution
    }
}

/// Untrusted producer claim awaiting content verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SuccessCandidate {
    context: AttemptContext,
    declared_outputs: ReceiptOutputs,
}

impl SuccessCandidate {
    pub const fn new(context: AttemptContext, declared_outputs: ReceiptOutputs) -> Self {
        Self {
            context,
            declared_outputs,
        }
    }

    pub const fn context(&self) -> &AttemptContext {
        &self.context
    }

    pub const fn declared_outputs(&self) -> ReceiptOutputs {
        self.declared_outputs
    }

    /// Verifies the actual projection bytes against both declared digests.
    /// No status receipt is returned if either object is absent or forged.
    pub fn verify(
        self,
        core_bytes: &[u8],
        envelope_bytes: &[u8],
    ) -> Result<StatusReceipt, VerificationError> {
        verify_projection(Projection::Core, self.declared_outputs.core, core_bytes)?;
        verify_projection(
            Projection::Envelope,
            self.declared_outputs.envelope,
            envelope_bytes,
        )?;

        let mut proof = Vec::new();
        proof.extend_from_slice(b"verified-success/v1\0");
        proof.extend_from_slice(self.context.key.digest().as_bytes());
        proof.extend_from_slice(self.declared_outputs.core.as_bytes());
        proof.extend_from_slice(self.declared_outputs.envelope.as_bytes());
        Ok(StatusReceipt {
            context: self.context,
            status: AttemptStatus::Success(VerifiedOutputs {
                outputs: self.declared_outputs,
                verification_digest: sha256(&proof),
                marker: VerifiedMarker,
            }),
        })
    }
}

fn verify_projection(
    projection: Projection,
    expected: Digest,
    bytes: &[u8],
) -> Result<(), VerificationError> {
    let actual = sha256(bytes);
    if actual == expected {
        Ok(())
    } else {
        Err(VerificationError {
            projection,
            expected,
            actual,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct VerifiedMarker;

/// Output digests carrying the module-private proof-of-verification marker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedOutputs {
    outputs: ReceiptOutputs,
    verification_digest: Digest,
    marker: VerifiedMarker,
}

impl VerifiedOutputs {
    pub const fn outputs(&self) -> ReceiptOutputs {
        self.outputs
    }

    pub const fn verification_digest(&self) -> Digest {
        self.verification_digest
    }
}

/// Typed payload for one producer attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttemptStatus {
    Success(VerifiedOutputs),
    Failed {
        error_code: Option<i32>,
        message: String,
    },
    Cancelled {
        reason: String,
    },
    TimedOut {
        deadline_tick: u64,
    },
    Diagnostic {
        code: String,
        message: String,
        observed_outputs: Option<ReceiptOutputs>,
    },
}

/// Stable status discriminant for policy and observability.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusKind {
    Success,
    Failed,
    Cancelled,
    TimedOut,
    Diagnostic,
}

/// A typed attempt receipt. Only its verified success variant can create a
/// cache-hit capability.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatusReceipt {
    context: AttemptContext,
    status: AttemptStatus,
}

impl StatusReceipt {
    pub fn failed(
        context: AttemptContext,
        error_code: Option<i32>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            context,
            status: AttemptStatus::Failed {
                error_code,
                message: message.into(),
            },
        }
    }

    pub fn cancelled(context: AttemptContext, reason: impl Into<String>) -> Self {
        Self {
            context,
            status: AttemptStatus::Cancelled {
                reason: reason.into(),
            },
        }
    }

    pub const fn timed_out(context: AttemptContext, deadline_tick: u64) -> Self {
        Self {
            context,
            status: AttemptStatus::TimedOut { deadline_tick },
        }
    }

    /// Diagnostic output digests are retained as observations only. They are
    /// intentionally not verified into the success type.
    pub fn diagnostic(
        context: AttemptContext,
        code: impl Into<String>,
        message: impl Into<String>,
        observed_outputs: Option<ReceiptOutputs>,
    ) -> Self {
        Self {
            context,
            status: AttemptStatus::Diagnostic {
                code: code.into(),
                message: message.into(),
                observed_outputs,
            },
        }
    }

    pub const fn context(&self) -> &AttemptContext {
        &self.context
    }

    pub const fn status(&self) -> &AttemptStatus {
        &self.status
    }

    pub const fn kind(&self) -> StatusKind {
        match self.status {
            AttemptStatus::Success(_) => StatusKind::Success,
            AttemptStatus::Failed { .. } => StatusKind::Failed,
            AttemptStatus::Cancelled { .. } => StatusKind::Cancelled,
            AttemptStatus::TimedOut { .. } => StatusKind::TimedOut,
            AttemptStatus::Diagnostic { .. } => StatusKind::Diagnostic,
        }
    }

    pub const fn is_hit_eligible(&self) -> bool {
        matches!(self.status, AttemptStatus::Success(_))
    }

    /// Returns a capability object only for a verified success.
    pub const fn cache_hit(&self) -> Option<CacheHit> {
        match &self.status {
            AttemptStatus::Success(verified) => Some(CacheHit {
                key: self.context.key,
                outputs: verified.outputs,
                verification_digest: verified.verification_digest,
            }),
            AttemptStatus::Failed { .. }
            | AttemptStatus::Cancelled { .. }
            | AttemptStatus::TimedOut { .. }
            | AttemptStatus::Diagnostic { .. } => None,
        }
    }
}

/// Cache lookup result that cannot be constructed without verified outputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheHit {
    key: ReceiptKey,
    outputs: ReceiptOutputs,
    verification_digest: Digest,
}

impl CacheHit {
    pub const fn key(self) -> ReceiptKey {
        self.key
    }

    pub const fn outputs(self) -> ReceiptOutputs {
        self.outputs
    }

    pub const fn verification_digest(self) -> Digest {
        self.verification_digest
    }
}

/// A producer's bytes did not match its claimed content digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationError {
    pub projection: Projection,
    pub expected: Digest,
    pub actual: Digest,
}

impl fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} output digest mismatch: expected {}, got {}",
            self.projection, self.expected, self.actual
        )
    }
}

impl Error for VerificationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(attempt: u64) -> AttemptContext {
        AttemptContext::new(
            ReceiptKey::from(sha256(b"semantic-key")),
            attempt,
            ExecutionMetadata {
                worker_id: "worker-7".to_string(),
                ..ExecutionMetadata::default()
            },
        )
    }

    #[test]
    fn only_digest_verified_success_is_hit_eligible() {
        let outputs = ReceiptOutputs::new(sha256(b"core"), sha256(b"envelope"));
        let forged = SuccessCandidate::new(context(1), outputs)
            .verify(b"forged-core", b"envelope")
            .expect_err("forged output must not verify");
        assert_eq!(forged.projection, Projection::Core);
        assert_eq!(forged.expected, outputs.core);

        let receipt = SuccessCandidate::new(context(2), outputs)
            .verify(b"core", b"envelope")
            .expect("both projections verify");
        assert_eq!(receipt.kind(), StatusKind::Success);
        assert!(receipt.is_hit_eligible());
        let hit = receipt.cache_hit().expect("verified cache hit");
        assert_eq!(hit.key(), receipt.context().key());
        assert_eq!(hit.outputs(), outputs);
        assert_ne!(hit.verification_digest(), Digest::default());
    }

    #[test]
    fn every_non_success_status_is_observable_but_ineligible() {
        let observed = ReceiptOutputs::new(sha256(b"partial-core"), sha256(b"partial-envelope"));
        let receipts = [
            StatusReceipt::failed(context(1), Some(17), "producer failed"),
            StatusReceipt::cancelled(context(2), "coordinator cancelled"),
            StatusReceipt::timed_out(context(3), 9_000),
            StatusReceipt::diagnostic(
                context(4),
                "partial-measurement",
                "retained for review",
                Some(observed),
            ),
        ];
        assert_eq!(
            receipts.each_ref().map(|receipt| receipt.kind()),
            [
                StatusKind::Failed,
                StatusKind::Cancelled,
                StatusKind::TimedOut,
                StatusKind::Diagnostic,
            ]
        );
        for receipt in &receipts {
            assert!(!receipt.is_hit_eligible());
            assert_eq!(receipt.cache_hit(), None);
            assert_eq!(receipt.context().execution().worker_id, "worker-7");
        }
    }

    #[test]
    fn envelope_verification_is_mandatory_even_when_core_matches() {
        let outputs = ReceiptOutputs::new(sha256(b"core"), sha256(b"envelope"));
        let error = SuccessCandidate::new(context(1), outputs)
            .verify(b"core", b"forged-envelope")
            .expect_err("envelope mismatch must reject success");
        assert_eq!(error.projection, Projection::Envelope);
    }
}
