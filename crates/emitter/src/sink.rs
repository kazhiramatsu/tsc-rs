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

/// Filesystem operations required by [`FsOutputSink`].
///
/// The protocol returns already-stable host messages so the sink can preserve
/// the exact error chosen by the first failing create operation or the final
/// write retry. Read-only program hosts intentionally do not implement this
/// boundary.
pub trait EmitFileSystem {
    fn write_file(&mut self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString>;

    fn create_directory(&mut self, path: JsStr<'_>) -> Result<(), JsString>;

    /// TypeScript's system `directoryExists` query is non-throwing. A native
    /// adapter therefore maps an uninspectable path to `false`; the following
    /// create/write operation owns the stable reportable failure.
    fn directory_exists(&mut self, path: JsStr<'_>) -> bool;

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

    fn create_directory(&self, path: JsStr<'_>) -> Result<(), JsString>;

    fn directory_exists(&self, path: JsStr<'_>) -> bool;
}

/// One artifact of a concurrent batch: the job until a worker takes it, then
/// its result.
type WriteSlot = std::sync::Mutex<(
    Option<EmitArtifact>,
    Option<Result<EmitWriteDisposition, EmitIoError>>,
)>;

/// [`FsOutputSink`]'s write/parent/retry boundary over a shared filesystem,
/// writing a batch on up to `workers` scoped threads.
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
        let path = artifact.path().to_owned();
        let bytes = artifact.materialized_bytes();
        if self
            .filesystem
            .write_file(path.as_js(), bytes.as_ref())
            .is_ok()
        {
            return Ok(EmitWriteDisposition::Written);
        }
        ensure_parent_directories(
            |directory| self.filesystem.directory_exists(directory),
            |directory| self.filesystem.create_directory(directory),
            path.as_js(),
        )?;
        self.filesystem
            .write_file(path.as_js(), bytes.as_ref())
            .map_err(|message| EmitIoError::new(EmitIoOperation::WriteFile, &path, message))?;
        Ok(EmitWriteDisposition::Written)
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

/// Filesystem sink that applies TypeScript's write/parent/retry boundary to
/// the same immutable artifacts accepted by [`MemoryOutputSink`].
pub struct FsOutputSink<'filesystem> {
    filesystem: &'filesystem mut dyn EmitFileSystem,
}

impl<'filesystem> FsOutputSink<'filesystem> {
    pub fn new(filesystem: &'filesystem mut dyn EmitFileSystem) -> Self {
        Self { filesystem }
    }

    /// tsc-port: ensureDirectoriesExist @6.0.3
    /// tsc-hash: 6d2d75310879fb4ad132c16f8817a30d754187c1c28691182d5eafb57f3aab28
    /// tsc-span: _tsc.js:16656-16662
    fn ensure_parent_directories(&mut self, output: JsStr<'_>) -> Result<(), EmitIoError> {
        let filesystem = &mut *self.filesystem;
        let mut exists = |directory: JsStr<'_>| filesystem.directory_exists(directory);
        let mut missing = missing_parent_directories(&mut exists, output);
        for directory in missing.drain(..) {
            filesystem
                .create_directory(directory.as_js())
                .map_err(|message| {
                    EmitIoError::new(
                        EmitIoOperation::CreateParentDirectory,
                        directory.as_js(),
                        message,
                    )
                })?;
        }
        Ok(())
    }
}

/// The parent directories of `output` that do not exist yet, outermost
/// first. Only retry-parent discovery is normalized; both write attempts
/// keep the caller's original path, including relative dot segments.
fn missing_parent_directories(
    exists: &mut dyn FnMut(JsStr<'_>) -> bool,
    output: JsStr<'_>,
) -> Vec<JsString> {
    use crate::source_map::paths;

    let normalized = paths::get_normalized_absolute_path(output, "");
    let mut directory = directory_path(normalized.as_js());
    let mut missing = Vec::new();
    while directory.as_bytes().len() > paths::get_root_length(directory) {
        if exists(directory) {
            break;
        }
        missing.push(directory.to_owned());
        directory = directory_path(directory);
    }
    missing.reverse();
    missing
}

fn ensure_parent_directories(
    mut exists: impl FnMut(JsStr<'_>) -> bool,
    create: impl Fn(JsStr<'_>) -> Result<(), JsString>,
    output: JsStr<'_>,
) -> Result<(), EmitIoError> {
    for directory in missing_parent_directories(&mut exists, output) {
        create(directory.as_js()).map_err(|message| {
            EmitIoError::new(
                EmitIoOperation::CreateParentDirectory,
                directory.as_js(),
                message,
            )
        })?;
    }
    Ok(())
}

/// Root-aware directory slicing on an already slash-normalized emitted path.
/// tsc-port: getDirectoryPath @6.0.3
/// tsc-hash: 7f2c6450b6b1c1bc4e1c65523c113201cd6cad6445d29d8c2d368913af418157
/// tsc-span: _tsc.js:5391-5397
fn directory_path(path: JsStr<'_>) -> JsStr<'_> {
    let root_length = crate::source_map::paths::get_root_length(path);
    if path.as_bytes().len() == root_length {
        return path;
    }
    let path = path.strip_suffix("/").unwrap_or(path);
    let slash = path
        .as_bytes()
        .iter()
        .rposition(|&byte| byte == b'/')
        .unwrap_or(0);
    path.split_at_byte(slash.max(root_length))
        .expect("directory boundary is ASCII")
        .0
}

impl OutputSink for FsOutputSink<'_> {
    /// tsc-port: writeFileEnsuringDirectories @6.0.3
    /// tsc-hash: 7a161f0c5aec317eb20a1f26977e5929c56ec7608d9f621e19eb1235322f9cd2
    /// tsc-span: _tsc.js:16663-16670
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let path = artifact.path().to_owned();
        let bytes = artifact.materialized_bytes();
        if self
            .filesystem
            .write_file(path.as_js(), bytes.as_ref())
            .is_ok()
        {
            return Ok(EmitWriteDisposition::Written);
        }

        self.ensure_parent_directories(path.as_js())?;
        self.filesystem
            .write_file(path.as_js(), bytes.as_ref())
            .map_err(|message| EmitIoError::new(EmitIoOperation::WriteFile, &path, message))?;
        Ok(EmitWriteDisposition::Written)
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
