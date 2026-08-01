use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::resolution::ResolutionError;

/// Stable classes for fail-closed prepared-program construction errors.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PreparationErrorKind {
    InvalidInput,
    InvalidData,
    IdentityConflict,
    InvalidReference,
    ResolutionFailure,
    ResourceLimit,
}

/// The preparation operation that rejected an input fact.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PreparationOperation {
    CreateProgramPath,
    AddSourceFile,
    AddRootFile,
    AddLibraryFile,
    AddAuxiliaryFile,
    AddPackageMetadata,
    AddModuleResolution,
    AddTypeReferenceResolution,
    BuildPreparedProgram,
}

impl PreparationOperation {
    pub const fn name(self) -> &'static str {
        match self {
            Self::CreateProgramPath => "create program path",
            Self::AddSourceFile => "add prepared source file",
            Self::AddRootFile => "add prepared root file",
            Self::AddLibraryFile => "add prepared library file",
            Self::AddAuxiliaryFile => "add prepared auxiliary file",
            Self::AddPackageMetadata => "add package metadata",
            Self::AddModuleResolution => "add module resolution",
            Self::AddTypeReferenceResolution => "add type-reference resolution",
            Self::BuildPreparedProgram => "build prepared program",
        }
    }
}

/// A typed error from validating the owned H0 program contract.
///
/// Resolution infrastructure failures retain their typed cause. In
/// particular, callers cannot turn an unsupported resolver branch or host
/// failure into an ordinary resolution miss by inserting it into a table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparationError {
    kind: PreparationErrorKind,
    operation: PreparationOperation,
    path: Option<PathBuf>,
    detail: String,
    resolution: Option<ResolutionError>,
}

impl PreparationError {
    pub(crate) fn new(
        kind: PreparationErrorKind,
        operation: PreparationOperation,
        path: Option<PathBuf>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation,
            path,
            detail: detail.into(),
            resolution: None,
        }
    }

    pub(crate) fn from_resolution(
        operation: PreparationOperation,
        path: Option<PathBuf>,
        error: ResolutionError,
    ) -> Self {
        Self {
            kind: PreparationErrorKind::ResolutionFailure,
            operation,
            path,
            detail: error.to_string(),
            resolution: Some(error),
        }
    }

    pub const fn kind(&self) -> PreparationErrorKind {
        self.kind
    }

    pub const fn operation(&self) -> PreparationOperation {
        self.operation
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn resolution(&self) -> Option<&ResolutionError> {
        self.resolution.as_ref()
    }
}

impl fmt::Display for PreparationError {
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

impl Error for PreparationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.resolution
            .as_ref()
            .map(|error| error as &(dyn Error + 'static))
    }
}
