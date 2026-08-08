use std::path::Path;

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
}

/// Filesystem operations required by [`FsOutputSink`].
///
/// The protocol returns already-stable host messages so the sink can preserve
/// the exact error chosen by the first failing create operation or the final
/// write retry. Read-only program hosts intentionally do not implement this
/// boundary.
pub trait EmitFileSystem {
    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<(), String>;

    fn create_directory(&mut self, path: &Path) -> Result<(), String>;

    /// TypeScript's system `directoryExists` query is non-throwing. A native
    /// adapter therefore maps an uninspectable path to `false`; the following
    /// create/write operation owns the stable reportable failure.
    fn directory_exists(&mut self, path: &Path) -> bool;
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
    fn ensure_parent_directories(&mut self, output: &Path) -> Result<(), EmitIoError> {
        let Some(mut directory) = output.parent() else {
            return Ok(());
        };
        let mut missing = Vec::new();
        while directory.parent().is_some() {
            if self.filesystem.directory_exists(directory) {
                break;
            }
            missing.push(directory.to_path_buf());
            directory = directory
                .parent()
                .expect("loop only retains paths with a parent");
        }
        for directory in missing.into_iter().rev() {
            self.filesystem
                .create_directory(&directory)
                .map_err(|message| {
                    EmitIoError::new(EmitIoOperation::CreateParentDirectory, directory, message)
                })?;
        }
        Ok(())
    }
}

impl OutputSink for FsOutputSink<'_> {
    /// tsc-port: writeFileEnsuringDirectories @6.0.3
    /// tsc-hash: 7a161f0c5aec317eb20a1f26977e5929c56ec7608d9f621e19eb1235322f9cd2
    /// tsc-span: _tsc.js:16663-16670
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        let path = artifact.path().to_path_buf();
        let bytes = artifact.materialized_bytes();
        if self.filesystem.write_file(&path, bytes.as_ref()).is_ok() {
            return Ok(EmitWriteDisposition::Written);
        }

        self.ensure_parent_directories(&path)?;
        self.filesystem
            .write_file(&path, bytes.as_ref())
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
