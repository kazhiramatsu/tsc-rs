use tsc_diagnostics::{JsStr, JsString};

use crate::{EmitArtifact, EmitIoError, EmitIoOperation};

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

    /// Write a batch of artifacts and return each artifact's result in input
    /// order. The default writes them one after another through
    /// [`Self::write`]; a sink over a shared, stateless boundary may write
    /// them concurrently, which changes nothing the caller observes (the
    /// results, and every diagnostic derived from them, keep input order).
    fn write_all(
        &mut self,
        artifacts: Vec<EmitArtifact>,
    ) -> Vec<Result<EmitWriteDisposition, EmitIoError>> {
        artifacts
            .into_iter()
            .map(|artifact| self.write(artifact))
            .collect()
    }

    /// This sink as a shared handle that several emit workers may write
    /// through at once, when its boundary is stateless (the process's real
    /// filesystem); `None` (the default) keeps every write in the finishing
    /// step, in plan order.
    fn shared(&self) -> Option<&dyn SharedOutputSink> {
        None
    }

    /// Whether ordinary source roots may write their artifacts as soon as
    /// they are printed (a worker's eager handle) instead of in the
    /// finishing step. Bundle roots always write through a supplied sink.
    fn writes_source_roots_eagerly(&self) -> bool {
        false
    }
}

/// A write boundary usable from several threads at once.
pub trait SharedOutputSink: Sync {
    fn write_shared(&self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError>;
}

/// An emit worker's sink over a shared boundary: every artifact of the unit
/// it emits is written right after it is printed, overlapping the other
/// workers' transform and print. The finishing step then has nothing left to
/// write for that unit; its listing, dispositions and diagnostics are those
/// of the plan-ordered writes.
pub struct EagerUnitSink<'shared>(pub &'shared dyn SharedOutputSink);

impl OutputSink for EagerUnitSink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.0.write_shared(artifact)
    }

    fn writes_source_roots_eagerly(&self) -> bool {
        true
    }
}

/// The file system [`FsOutputSink`] writes through.
///
/// A write creates the missing directories above the file (tsgo
/// `vfs.FS.WriteFile`) and returns the file system's own message on
/// failure. Read-only program hosts do not implement this boundary.
pub trait EmitFileSystem {
    fn write_file(&mut self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString>;

    /// The same boundary as a shared handle when the filesystem is stateless
    /// (the process's real filesystem), so several artifacts may be written
    /// at once; `None` (the default) keeps writes sequential and ordered.
    fn shared(&self) -> Option<&dyn SharedEmitFileSystem> {
        None
    }
}

/// A stateless filesystem boundary usable from several threads at once.
pub trait SharedEmitFileSystem: Sync {
    fn write_file(&self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString>;
}

/// One artifact of a concurrent batch: the job until a worker takes it, then
/// its result.
type WriteSlot = std::sync::Mutex<(
    Option<EmitArtifact>,
    Option<Result<EmitWriteDisposition, EmitIoError>>,
)>;

/// [`FsOutputSink`] over a shared filesystem: a batch is written on up to
/// `workers` scoped threads.
pub struct SharedFsOutputSink<'filesystem> {
    filesystem: &'filesystem dyn SharedEmitFileSystem,
    workers: usize,
}

impl<'filesystem> SharedFsOutputSink<'filesystem> {
    pub fn new(filesystem: &'filesystem dyn SharedEmitFileSystem, workers: usize) -> Self {
        Self {
            filesystem,
            workers: workers.max(1),
        }
    }

    fn write_one(&self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let path = artifact.path();
        self.filesystem
            .write_file(path, artifact.materialized_bytes().as_ref())
            .map(|()| EmitWriteDisposition::Written)
            .map_err(|message| EmitIoError::new(EmitIoOperation::WriteFile, path, message))
    }
}

impl SharedOutputSink for SharedFsOutputSink<'_> {
    fn write_shared(&self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.write_one(artifact)
    }
}

impl OutputSink for SharedFsOutputSink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.write_one(artifact)
    }

    fn shared(&self) -> Option<&dyn SharedOutputSink> {
        Some(self)
    }

    fn write_all(
        &mut self,
        artifacts: Vec<EmitArtifact>,
    ) -> Vec<Result<EmitWriteDisposition, EmitIoError>> {
        let workers = self.workers.min(artifacts.len());
        if workers < 2 {
            return artifacts
                .into_iter()
                .map(|artifact| self.write_one(artifact))
                .collect();
        }
        // Heaviest first from one shared counter; each slot holds its own
        // job and result, so the results come back in input order.
        let mut order: Vec<usize> = (0..artifacts.len()).collect();
        order.sort_by_key(|&index| std::cmp::Reverse(artifacts[index].materialized_bytes().len()));
        let slots: Vec<WriteSlot> = artifacts
            .into_iter()
            .map(|artifact| std::sync::Mutex::new((Some(artifact), None)))
            .collect();
        let next = std::sync::atomic::AtomicUsize::new(0);
        let sink = &*self;
        let run = || loop {
            let Some(&index) = order.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
            else {
                break;
            };
            let artifact = slots[index]
                .lock()
                .expect("write slot")
                .0
                .take()
                .expect("each artifact is written once");
            let result = sink.write_one(artifact);
            slots[index].lock().expect("write slot").1 = Some(result);
        };
        std::thread::scope(|scope| {
            for _ in 1..workers {
                // A refused thread is not an error: the remaining threads
                // share the writes.
                let _ = std::thread::Builder::new()
                    .name("tsc-rs-writer".to_owned())
                    .spawn_scoped(scope, run);
            }
            run();
        });
        slots
            .into_iter()
            .map(|slot| {
                slot.into_inner()
                    .expect("write slot")
                    .1
                    .expect("every artifact produced a result")
            })
            .collect()
    }
}

/// Filesystem sink: each artifact is one write through the file system
/// (tsgo's compiler host `WriteFile`).
pub struct FsOutputSink<'filesystem> {
    filesystem: &'filesystem mut dyn EmitFileSystem,
}

impl<'filesystem> FsOutputSink<'filesystem> {
    pub fn new(filesystem: &'filesystem mut dyn EmitFileSystem) -> Self {
        Self { filesystem }
    }
}

impl OutputSink for FsOutputSink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let path = artifact.path();
        self.filesystem
            .write_file(path, artifact.materialized_bytes().as_ref())
            .map(|()| EmitWriteDisposition::Written)
            .map_err(|message| EmitIoError::new(EmitIoOperation::WriteFile, path, message))
    }
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
