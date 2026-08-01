use std::fmt;
use std::path::{Path, PathBuf};

use crate::error::{PreparationError, PreparationErrorKind, PreparationOperation};

/// A normalized, host-profile-canonical lookup identity.
///
/// H0 loaders produce this value with the vendored `toPath` semantics. This
/// data-contract slice intentionally accepts the already-derived identity:
/// it does not duplicate path normalization or TypeScript's case fold from
/// the host crate. The wrapper prevents display spellings from being used as
/// lookup keys accidentally.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalPath(PathBuf);

impl CanonicalPath {
    /// Wrap an identity already normalized with the vendored `toPath`
    /// semantics. This boundary validates representation only; the future H0
    /// loader is responsible for deriving the value rather than guessing it.
    pub fn from_trusted_normalized(path: impl Into<PathBuf>) -> Result<Self, PreparationError> {
        let path = path.into();
        validate_path(&path, "canonical path")?;
        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl AsRef<Path> for CanonicalPath {
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl fmt::Display for CanonicalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.display().fmt(formatter)
    }
}

/// One path's user-facing spelling and independent canonical lookup key.
///
/// A physical `realpath` is deliberately not inferred here. Loaders retain
/// it as a separate resolution fact when symlink handling needs it; lexical
/// canonicalization and physical identity are not interchangeable.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgramPath {
    display: PathBuf,
    canonical: CanonicalPath,
}

impl ProgramPath {
    /// Pair a display spelling with its already-derived canonical identity.
    pub fn from_trusted_parts(
        display: impl Into<PathBuf>,
        canonical: impl Into<PathBuf>,
    ) -> Result<Self, PreparationError> {
        let display = display.into();
        validate_path(&display, "display path")?;
        Ok(Self {
            display,
            canonical: CanonicalPath::from_trusted_normalized(canonical)?,
        })
    }

    pub fn display(&self) -> &Path {
        &self.display
    }

    pub fn canonical(&self) -> &CanonicalPath {
        &self.canonical
    }

    pub fn into_parts(self) -> (PathBuf, CanonicalPath) {
        (self.display, self.canonical)
    }
}

fn validate_path(path: &Path, label: &str) -> Result<(), PreparationError> {
    if path.as_os_str().is_empty() {
        return Err(PreparationError::new(
            PreparationErrorKind::InvalidInput,
            PreparationOperation::CreateProgramPath,
            Some(path.to_path_buf()),
            format!("{label} is empty"),
        ));
    }
    let Some(text) = path.to_str() else {
        return Err(PreparationError::new(
            PreparationErrorKind::InvalidInput,
            PreparationOperation::CreateProgramPath,
            Some(path.to_path_buf()),
            format!("{label} is not valid Unicode"),
        ));
    };
    if text.contains('\0') {
        return Err(PreparationError::new(
            PreparationErrorKind::InvalidInput,
            PreparationOperation::CreateProgramPath,
            Some(path.to_path_buf()),
            format!("{label} contains a NUL byte"),
        ));
    }
    Ok(())
}
