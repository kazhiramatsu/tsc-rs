//! Resolver failures, separate from successful/missing resolution identities.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::HostError;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ResolutionErrorKind {
    Host,
    Unsupported,
    Canonicalization,
    InvalidData,
    ResourceLimit,
}

/// A resolver failure that is structurally separate from `NotFound`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolutionError {
    Host(HostError),
    Unsupported {
        feature: String,
        detail: String,
    },
    Canonicalization {
        path: Option<PathBuf>,
        js_path: Option<JsString>,
        detail: String,
    },
    InvalidData(String),
    ResourceLimit(String),
}

impl ResolutionError {
    pub fn unsupported(feature: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            feature: feature.into(),
            detail: detail.into(),
        }
    }

    pub fn canonicalization(path: Option<PathBuf>, detail: impl Into<String>) -> Self {
        Self::Canonicalization {
            js_path: path
                .as_ref()
                .and_then(|path| path.to_str())
                .map(JsString::from),
            path,
            detail: detail.into(),
        }
    }

    pub fn canonicalization_js(path: Option<JsStr<'_>>, detail: impl Into<String>) -> Self {
        Self::Canonicalization {
            path: path.map(|path| PathBuf::from(path.to_string_lossy().into_owned())),
            js_path: path.map(JsStr::to_owned),
            detail: detail.into(),
        }
    }

    pub fn invalid_data(detail: impl Into<String>) -> Self {
        Self::InvalidData(detail.into())
    }

    pub fn resource_limit(detail: impl Into<String>) -> Self {
        Self::ResourceLimit(detail.into())
    }

    pub const fn kind(&self) -> ResolutionErrorKind {
        match self {
            Self::Host(_) => ResolutionErrorKind::Host,
            Self::Unsupported { .. } => ResolutionErrorKind::Unsupported,
            Self::Canonicalization { .. } => ResolutionErrorKind::Canonicalization,
            Self::InvalidData(_) => ResolutionErrorKind::InvalidData,
            Self::ResourceLimit(_) => ResolutionErrorKind::ResourceLimit,
        }
    }

    /// Native error/display context. Use `js_path` for the requested JS name.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Host(error) => error.path(),
            Self::Canonicalization { path, .. } => path.as_deref(),
            Self::Unsupported { .. } | Self::InvalidData(_) | Self::ResourceLimit(_) => None,
        }
    }

    pub fn js_path(&self) -> Option<JsStr<'_>> {
        match self {
            Self::Host(error) => error.js_path(),
            Self::Canonicalization { js_path, .. } => js_path.as_ref().map(JsString::as_js),
            Self::Unsupported { .. } | Self::InvalidData(_) | Self::ResourceLimit(_) => None,
        }
    }
}

impl From<HostError> for ResolutionError {
    fn from(error: HostError) -> Self {
        Self::Host(error)
    }
}

impl fmt::Display for ResolutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(error) => write!(formatter, "host failure: {error}"),
            Self::Unsupported { feature, detail } => {
                write!(formatter, "unsupported resolution feature {feature}")?;
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::Canonicalization { path, detail, .. } => {
                formatter.write_str("path canonicalization failed")?;
                if let Some(path) = path {
                    write!(formatter, " for {}", path.display())?;
                }
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::InvalidData(detail) => write!(formatter, "invalid resolution data: {detail}"),
            Self::ResourceLimit(detail) => {
                write!(formatter, "resolution resource limit exceeded: {detail}")
            }
        }
    }
}

impl Error for ResolutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host(error) => Some(error),
            Self::Unsupported { .. }
            | Self::Canonicalization { .. }
            | Self::InvalidData(_)
            | Self::ResourceLimit(_) => None,
        }
    }
}
