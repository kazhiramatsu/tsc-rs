use core::fmt;

use tsc_ci_adapter_protocol::{FixedPlanV1, ProtocolError};
use tsc_ci_core::{hash_object, BoundedBytesSink, CanonicalEncode, ObjectDigestV1};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PlanError {
    Protocol(ProtocolError),
    EmptyPolicy,
    CanonicalEncoding,
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "control plan error: {self:?}")
    }
}

impl std::error::Error for PlanError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VerifiedPlanV1 {
    plan: FixedPlanV1,
    digest: ObjectDigestV1,
}

impl VerifiedPlanV1 {
    pub const fn digest(&self) -> ObjectDigestV1 {
        self.digest
    }

    pub fn plan(&self) -> &FixedPlanV1 {
        &self.plan
    }
}

pub fn verify_plan(plan: FixedPlanV1) -> Result<VerifiedPlanV1, PlanError> {
    if plan.policy_ids().is_empty() {
        return Err(PlanError::EmptyPolicy);
    }
    let mut sink = BoundedBytesSink::new(16 * 1024 * 1024);
    plan.encode_canonical(&mut sink)
        .map_err(|_| PlanError::CanonicalEncoding)?;
    Ok(VerifiedPlanV1 {
        digest: hash_object(sink.bytes()),
        plan,
    })
}
