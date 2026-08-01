use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// The host operation that failed.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostOperation {
    BuildMemoryHost,
    CurrentDirectory,
    ReadFile,
    FileExists,
    DirectoryExists,
    ReadDirectory,
    Realpath,
}

impl HostOperation {
    pub const fn name(self) -> &'static str {
        match self {
            Self::BuildMemoryHost => "build memory host",
            Self::CurrentDirectory => "read current directory",
            Self::ReadFile => "read file",
            Self::FileExists => "test file existence",
            Self::DirectoryExists => "test directory existence",
            Self::ReadDirectory => "read directory",
            Self::Realpath => "resolve real path",
        }
    }
}

/// Stable error classes shared by memory and filesystem host adapters.
///
/// Missing files and directories are not errors and therefore have no enum
/// variant here.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HostErrorKind {
    PermissionDenied,
    InvalidInput,
    InvalidData,
    ResourceLimit,
    IdentityConflict,
    Other,
}

/// A fail-closed host error, kept distinct from an ordinary missing entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HostError {
    kind: HostErrorKind,
    operation: HostOperation,
    path: Option<PathBuf>,
    detail: String,
}

impl HostError {
    pub fn new(
        kind: HostErrorKind,
        operation: HostOperation,
        path: Option<PathBuf>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            path,
            detail: detail.into(),
        }
    }

    pub const fn kind(&self) -> HostErrorKind {
        self.kind
    }

    pub const fn operation(&self) -> HostOperation {
        self.operation
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.operation.name())?;
        if let Some(path) = &self.path {
            write!(formatter, " for {}", path.display())?;
        }
        if !self.detail.is_empty() {
            write!(formatter, ": {}", self.detail)?;
        }
        Ok(())
    }
}

impl Error for HostError {}
