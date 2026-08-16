use crate::{EffectPhase, InfraError};

/// A finite byte ceiling shared by chunk reads and invocation-private staging.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ByteLimit(u64);

impl ByteLimit {
    pub fn try_new(bytes: u64) -> Result<Self, InfraError> {
        if bytes == 0 {
            return Err(InfraError::Quota {
                phase: EffectPhase::Read,
            });
        }
        Ok(Self(bytes))
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    const fn allows(self, bytes: usize) -> bool {
        bytes as u64 <= self.0
    }
}

/// An owned chunk that has already passed its byte ceiling.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedChunk(Box<[u8]>);

impl BoundedChunk {
    pub fn try_new(bytes: Vec<u8>, limit: ByteLimit) -> Result<Self, InfraError> {
        if !limit.allows(bytes.len()) {
            return Err(InfraError::Quota {
                phase: EffectPhase::Read,
            });
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// A synchronous bounded source. EOF is `Ok(None)`; it is not a cache miss.
pub trait ChunkSource {
    fn next_chunk(&mut self, limit: ByteLimit) -> EffectResult<Option<BoundedChunk>>;
}

/// The only result wrapper available to the FCI-2 effect seam.
///
/// A failed effect remains an infrastructure error. There is no `Miss`,
/// semantic rejection, or implicit retry variant here.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectResult<T> {
    Complete(T),
    Failed(InfraError),
}

impl<T> EffectResult<T> {
    pub fn into_result(self) -> Result<T, InfraError> {
        match self {
            Self::Complete(value) => Ok(value),
            Self::Failed(error) => Err(error),
        }
    }

    pub const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// This buffer is intentionally private until the invocation and process
/// boundary packets define who may own staged bytes. It never exposes partial
/// contents and clears them when abandoned or over quota.
#[allow(dead_code)]
#[derive(Debug)]
struct StagingBuffer {
    bytes: Vec<u8>,
    limit: ByteLimit,
    abandoned: bool,
}

#[allow(dead_code)]
impl StagingBuffer {
    fn new(limit: ByteLimit) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            abandoned: false,
        }
    }

    fn append(&mut self, chunk: &BoundedChunk) -> EffectResult<()> {
        if self.abandoned {
            return EffectResult::Failed(InfraError::Guard {
                phase: EffectPhase::Execute,
            });
        }
        let Some(total) = self.bytes.len().checked_add(chunk.as_bytes().len()) else {
            self.abandon();
            return EffectResult::Failed(InfraError::Quota {
                phase: EffectPhase::Execute,
            });
        };
        if !self.limit.allows(total) {
            self.abandon();
            return EffectResult::Failed(InfraError::Quota {
                phase: EffectPhase::Execute,
            });
        }
        self.bytes.extend_from_slice(chunk.as_bytes());
        EffectResult::Complete(())
    }

    fn abandon(&mut self) {
        self.bytes.clear();
        self.abandoned = true;
    }

    #[cfg(test)]
    fn state(&self) -> (usize, bool) {
        (self.bytes.len(), self.abandoned)
    }
}

#[cfg(test)]
#[path = "../tests/unit/bounded_tests.rs"]
mod tests;
