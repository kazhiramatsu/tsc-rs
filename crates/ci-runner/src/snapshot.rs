use core::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use tsc_ci_core::{
    ApplicationNamespaceV1, ImplementationIdV1, InvocationIdV1, InvocationIdentityV1,
    ObjectDigestV1, ProcessObservationV1, SandboxCapabilitiesV1,
};

use crate::{ByteLimit, EffectPhase, InfraError};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PathError {
    Empty,
    Absolute,
    ParentComponent,
    EmptyComponent,
    Backslash,
    Nul,
    InvalidUtf8,
}

impl fmt::Display for PathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Empty => "path is empty",
            Self::Absolute => "path is absolute",
            Self::ParentComponent => "path contains a parent component",
            Self::EmptyComponent => "path contains an empty component",
            Self::Backslash => "path contains a backslash",
            Self::Nul => "path contains NUL",
            Self::InvalidUtf8 => "path is not UTF-8",
        };
        formatter.write_str(name)
    }
}

impl std::error::Error for PathError {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RelativePathV1(Box<[u8]>);

impl RelativePathV1 {
    pub fn try_new(bytes: Vec<u8>) -> Result<Self, PathError> {
        if bytes.is_empty() {
            return Err(PathError::Empty);
        }
        if bytes.contains(&0) {
            return Err(PathError::Nul);
        }
        if bytes.contains(&b'\\') {
            return Err(PathError::Backslash);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| PathError::InvalidUtf8)?;
        if text.starts_with('/') {
            return Err(PathError::Absolute);
        }
        for component in text.split('/') {
            if component.is_empty() {
                return Err(PathError::EmptyComponent);
            }
            if component == "." || component == ".." {
                return Err(PathError::ParentComponent);
            }
        }
        Ok(Self(bytes.into_boxed_slice()))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    fn as_path(&self) -> &Path {
        // Construction validated UTF-8 and path separators, so a borrowed
        // platform path cannot introduce a hidden parent component here.
        Path::new(std::str::from_utf8(&self.0).expect("validated UTF-8 path"))
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceSnapshotLimits {
    pub max_entries: u64,
    pub max_bytes: u64,
    pub max_path_bytes: u64,
}

impl SourceSnapshotLimits {
    pub const fn new(
        max_entries: u64,
        max_bytes: u64,
        max_path_bytes: u64,
    ) -> Result<Self, InfraError> {
        if max_entries == 0 || max_bytes == 0 || max_path_bytes == 0 {
            return Err(InfraError::Quota {
                phase: EffectPhase::Acquire,
            });
        }
        Ok(Self {
            max_entries,
            max_bytes,
            max_path_bytes,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceSnapshotRequestV1 {
    namespace: ApplicationNamespaceV1,
    revision: ObjectDigestV1,
    provider: ImplementationIdV1,
    entries: ObjectDigestV1,
}

impl SourceSnapshotRequestV1 {
    pub const fn new(
        namespace: ApplicationNamespaceV1,
        revision: ObjectDigestV1,
        provider: ImplementationIdV1,
        entries: ObjectDigestV1,
    ) -> Self {
        Self {
            namespace,
            revision,
            provider,
            entries,
        }
    }

    pub const fn namespace(&self) -> ApplicationNamespaceV1 {
        self.namespace
    }

    pub const fn revision(&self) -> ObjectDigestV1 {
        self.revision
    }

    pub const fn provider(&self) -> ImplementationIdV1 {
        self.provider
    }

    pub const fn entries(&self) -> ObjectDigestV1 {
        self.entries
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceSnapshotV1 {
    request: SourceSnapshotRequestV1,
    entry_count: u64,
    byte_count: u64,
    mount_digest: ObjectDigestV1,
}

impl SourceSnapshotV1 {
    pub const fn request(&self) -> SourceSnapshotRequestV1 {
        self.request
    }

    pub const fn entry_count(&self) -> u64 {
        self.entry_count
    }

    pub const fn byte_count(&self) -> u64 {
        self.byte_count
    }

    pub const fn mount_digest(&self) -> ObjectDigestV1 {
        self.mount_digest
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SourceSnapshotGuardV1(ObjectDigestV1);

impl SourceSnapshotGuardV1 {
    pub const fn digest(&self) -> ObjectDigestV1 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VerifiedSourceSnapshot {
    snapshot: SourceSnapshotV1,
    guard: SourceSnapshotGuardV1,
}

impl VerifiedSourceSnapshot {
    pub const fn snapshot(&self) -> SourceSnapshotV1 {
        self.snapshot
    }

    pub const fn guard(&self) -> SourceSnapshotGuardV1 {
        self.guard
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountedSourceSnapshot {
    verified: VerifiedSourceSnapshot,
    root: PathBuf,
}

impl MountedSourceSnapshot {
    pub const fn verified(&self) -> VerifiedSourceSnapshot {
        self.verified
    }
}

pub trait SourceSnapshotProvider: Send + Sync {
    fn seal(
        &self,
        request: &SourceSnapshotRequestV1,
        limits: SourceSnapshotLimits,
    ) -> Result<VerifiedSourceSnapshot, InfraError>;
}

pub trait Sandbox: Send + Sync {
    fn execute(
        &self,
        invocation: &InvocationIdentityV1,
        source: &MountedSourceSnapshot,
        guard: SandboxExecutionGuardV1,
    ) -> Result<GuardedProcessObservationV1, InfraError>;
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SandboxExecutionGuardV1 {
    invocation: InvocationIdV1,
    capabilities: SandboxCapabilitiesV1,
}

impl SandboxExecutionGuardV1 {
    pub const fn invocation(&self) -> InvocationIdV1 {
        self.invocation
    }

    pub const fn capabilities(&self) -> SandboxCapabilitiesV1 {
        self.capabilities
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct GuardedProcessObservationV1 {
    observation: ProcessObservationV1,
    invocation: InvocationIdV1,
}

impl GuardedProcessObservationV1 {
    pub const fn observation(&self) -> ProcessObservationV1 {
        self.observation
    }

    pub const fn invocation(&self) -> InvocationIdV1 {
        self.invocation
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedFileBytes(Box<[u8]>);

impl BoundedFileBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

pub fn read_regular_file_bounded(
    root: &Path,
    relative: &RelativePathV1,
    limit: ByteLimit,
) -> Result<BoundedFileBytes, InfraError> {
    let path = root.join(relative.as_path());
    if fs::symlink_metadata(root)
        .map_err(|error| InfraError::from_io(EffectPhase::Read, error))?
        .file_type()
        .is_symlink()
    {
        return Err(InfraError::Guard {
            phase: EffectPhase::Read,
        });
    }
    let mut prefix = PathBuf::from(root);
    for component in relative.as_path().components() {
        prefix.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&prefix)
            .map_err(|error| InfraError::from_io(EffectPhase::Read, error))?;
        if metadata.file_type().is_symlink() {
            return Err(InfraError::Guard {
                phase: EffectPhase::Read,
            });
        }
    }

    #[cfg(unix)]
    let options = {
        let mut options = OpenOptions::new();
        options.read(true).custom_flags(libc::O_NOFOLLOW);
        options
    };
    #[cfg(not(unix))]
    let mut options = {
        // A platform without a reviewed no-follow primitive fails closed.
        return Err(InfraError::Guard {
            phase: EffectPhase::Read,
        });
    };

    let mut file = options
        .open(&path)
        .map_err(|error| InfraError::from_io(EffectPhase::Read, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| InfraError::from_io(EffectPhase::Read, error))?;
    if !metadata.is_file() {
        return Err(InfraError::Guard {
            phase: EffectPhase::Read,
        });
    }
    let max = usize::try_from(limit.get()).unwrap_or(usize::MAX);
    let read_limit = max.checked_add(1).unwrap_or(max);
    let mut bytes = Vec::with_capacity(max.min(64 * 1024));
    let mut buffer = [0u8; 8192];
    while bytes.len() < read_limit {
        let remaining = read_limit - bytes.len();
        let chunk_size = remaining.min(buffer.len());
        let count = file
            .read(&mut buffer[..chunk_size])
            .map_err(|error| InfraError::from_io(EffectPhase::Read, error))?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    if bytes.len() > max {
        return Err(InfraError::Quota {
            phase: EffectPhase::Read,
        });
    }
    Ok(BoundedFileBytes(bytes.into_boxed_slice()))
}

pub fn stage_no_replace(path: &Path, bytes: &[u8], limit: ByteLimit) -> Result<(), InfraError> {
    if bytes.len() as u64 > limit.get() {
        return Err(InfraError::Quota {
            phase: EffectPhase::Commit,
        });
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| InfraError::from_io(EffectPhase::Commit, error))?;
    file.write_all(bytes)
        .map_err(|error| InfraError::from_io(EffectPhase::Commit, error))?;
    file.sync_all()
        .map_err(|error| InfraError::from_io(EffectPhase::Commit, error))?;
    Ok(())
}

#[cfg(test)]
#[path = "../tests/unit/snapshot_tests.rs"]
mod tests;
