use crate::{EmitArtifact, EmitIoError};

/// Feedback from one output callback.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitWriteDisposition {
    Written,
    /// Reserved for a future builder that suppresses an unchanged write.
    SkippedUnchanged,
}

/// Write-only emitter boundary. The read-only compiler host never implements
/// or embeds this trait.
pub trait OutputSink {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError>;
}

/// Ordered in-memory authority used by emit acceptance tests.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct MemoryOutputSink {
    writes: Vec<EmitArtifact>,
}

impl MemoryOutputSink {
    pub const fn new() -> Self {
        Self { writes: Vec::new() }
    }

    pub fn writes(&self) -> &[EmitArtifact] {
        &self.writes
    }

    pub fn into_writes(self) -> Vec<EmitArtifact> {
        self.writes
    }
}

impl OutputSink for MemoryOutputSink {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes.push(artifact);
        Ok(EmitWriteDisposition::Written)
    }
}
