use super::{BoundedChunk, ByteLimit, StagingBuffer};
use crate::{EffectPhase, EffectResult, InfraError};

#[test]
fn staging_abandons_and_clears_on_over_limit() {
    let limit = ByteLimit::try_new(3).expect("positive limit");
    let chunk = BoundedChunk::try_new(vec![1, 2], limit).expect("chunk fits");
    let second = BoundedChunk::try_new(vec![3, 4], limit).expect("chunk fits alone");
    let mut staging = StagingBuffer::new(limit);
    assert_eq!(staging.append(&chunk), EffectResult::Complete(()));
    assert_eq!(staging.state(), (2, false));
    assert_eq!(
        staging.append(&second),
        EffectResult::Failed(InfraError::Quota {
            phase: EffectPhase::Execute,
        })
    );
    assert_eq!(staging.state(), (0, true));
}

#[test]
fn explicit_abandon_forbids_later_append() {
    let limit = ByteLimit::try_new(4).expect("positive limit");
    let chunk = BoundedChunk::try_new(vec![1], limit).expect("chunk fits");
    let mut staging = StagingBuffer::new(limit);
    staging.abandon();
    assert_eq!(staging.state(), (0, true));
    assert_eq!(
        staging.append(&chunk),
        EffectResult::Failed(InfraError::Guard {
            phase: EffectPhase::Execute,
        })
    );
}
