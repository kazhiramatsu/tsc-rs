use std::path::{Path, PathBuf};

use tsc_diagnostics::{JsStr, JsString};

use crate::error::{PreparationError, PreparationErrorKind, PreparationOperation};

/// A normalized, host-profile-canonical lookup identity.
///
/// H0 loaders produce this value with the vendored `toPath` semantics. This
/// data-contract slice intentionally accepts the already-derived identity:
/// it does not duplicate path normalization or TypeScript's case fold from
/// the host crate. The wrapper prevents display spellings from being used as
/// lookup keys accidentally.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalPath(JsString);

impl CanonicalPath {
    /// Wrap an identity already normalized with the vendored `toPath`
    /// semantics. This boundary validates representation only; the future H0
    /// loader is responsible for deriving the value rather than guessing it.
    pub fn from_trusted_normalized(path: impl Into<PathBuf>) -> Result<Self, PreparationError> {
        let path = path.into();
        let text = native_path_text(&path, "canonical path")?;
        Self::from_js_normalized(text.into())
    }

    /// Wrap an already normalized JavaScript path without crossing a host
    /// encoding boundary. Ordering is canonical byte order, not JS ordering.
    pub fn from_js_normalized(path: JsStr<'_>) -> Result<Self, PreparationError> {
        validate_js_path(path, "canonical path")?;
        Ok(Self(path.to_owned()))
    }

    pub fn as_js(&self) -> JsStr<'_> {
        self.0.as_js()
    }

    pub fn into_js_string(self) -> JsString {
        self.0
    }
}

/// One path's user-facing spelling and independent canonical lookup key.
///
/// A physical `realpath` is deliberately not inferred here. Loaders retain
/// it as a separate resolution fact when symlink handling needs it; lexical
/// canonicalization and physical identity are not interchangeable.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ProgramPath {
    display: JsString,
    canonical: CanonicalPath,
}

impl ProgramPath {
    /// Pair a display spelling with its already-derived canonical identity.
    pub fn from_trusted_parts(
        display: impl Into<PathBuf>,
        canonical: impl Into<PathBuf>,
    ) -> Result<Self, PreparationError> {
        let display = display.into();
        let canonical = canonical.into();
        let display = native_path_text(&display, "display path")?;
        validate_js_path(display.into(), "display path")?;
        Self::from_js_parts(
            display.into(),
            native_path_text(&canonical, "canonical path")?.into(),
        )
    }

    /// Pair compiler spellings before any conversion to a filesystem name.
    /// Distinct lone surrogates remain distinct even if a physical host maps
    /// both names to the same UTF-8 byte sequence for a read operation.
    pub fn from_js_parts(
        display: JsStr<'_>,
        canonical: JsStr<'_>,
    ) -> Result<Self, PreparationError> {
        validate_js_path(display, "display path")?;
        Ok(Self {
            display: display.to_owned(),
            canonical: CanonicalPath::from_js_normalized(canonical)?,
        })
    }

    pub fn display(&self) -> JsStr<'_> {
        self.display.as_js()
    }

    pub fn canonical(&self) -> &CanonicalPath {
        &self.canonical
    }

    pub fn into_parts(self) -> (JsString, CanonicalPath) {
        (self.display, self.canonical)
    }
}

fn native_path_text<'p>(path: &'p Path, label: &str) -> Result<&'p str, PreparationError> {
    let Some(text) = path.to_str() else {
        return Err(PreparationError::new(
            PreparationErrorKind::InvalidInput,
            PreparationOperation::CreateProgramPath,
            Some(path.to_path_buf()),
            format!("{label} is not valid Unicode"),
        ));
    };
    Ok(text)
}

fn validate_js_path(path: JsStr<'_>, label: &str) -> Result<(), PreparationError> {
    let detail = if path.is_empty() {
        Some(format!("{label} is empty"))
    } else if path.contains("\0") {
        Some(format!("{label} contains a NUL byte"))
    } else {
        None
    };
    if let Some(detail) = detail {
        return Err(PreparationError::new_js(
            PreparationErrorKind::InvalidInput,
            PreparationOperation::CreateProgramPath,
            Some(path),
            detail,
        ));
    }
    Ok(())
}
