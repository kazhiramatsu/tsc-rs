use tsc_ci_runner::{BoundedChunk, ByteLimit, ChunkSource, EffectPhase, EffectResult, InfraError};

struct FakeSource {
    chunks: Vec<Vec<u8>>,
}

impl ChunkSource for FakeSource {
    fn next_chunk(&mut self, limit: ByteLimit) -> EffectResult<Option<BoundedChunk>> {
        match self.chunks.pop() {
            Some(bytes) => BoundedChunk::try_new(bytes, limit)
                .map(Some)
                .map_or_else(EffectResult::Failed, EffectResult::Complete),
            None => EffectResult::Complete(None),
        }
    }
}

#[test]
fn source_is_bounded_and_eof_is_not_a_miss() {
    let limit = ByteLimit::try_new(2).expect("positive limit");
    let mut source = FakeSource {
        chunks: vec![vec![2, 3], vec![1]],
    };
    let first = source.next_chunk(limit);
    assert_eq!(
        first,
        EffectResult::Complete(Some(
            BoundedChunk::try_new(vec![1], limit).expect("chunk fits")
        ))
    );
    let second = source.next_chunk(limit);
    assert_eq!(
        second,
        EffectResult::Complete(Some(
            BoundedChunk::try_new(vec![2, 3], limit).expect("chunk fits")
        ))
    );
    assert_eq!(source.next_chunk(limit), EffectResult::Complete(None));
}

#[test]
fn oversized_chunk_is_quota_failure_and_never_truncates() {
    let limit = ByteLimit::try_new(2).expect("positive limit");
    let mut source = FakeSource {
        chunks: vec![vec![1, 2, 3]],
    };
    assert_eq!(
        source.next_chunk(limit),
        EffectResult::Failed(InfraError::Quota {
            phase: EffectPhase::Read,
        })
    );
}

#[test]
fn effect_failure_remains_an_error_after_projection() {
    let failure = EffectResult::<Option<BoundedChunk>>::Failed(InfraError::Cancelled {
        phase: EffectPhase::Read,
        reason: tsc_ci_runner::RunCancellation::UserRequested,
    });
    assert!(failure.is_failed());
    assert_eq!(
        failure
            .into_result()
            .expect_err("failure cannot become a value"),
        InfraError::Cancelled {
            phase: EffectPhase::Read,
            reason: tsc_ci_runner::RunCancellation::UserRequested,
        }
    );
}
