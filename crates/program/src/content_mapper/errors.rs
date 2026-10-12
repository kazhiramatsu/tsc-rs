//! tsgo `contentmapper` host.go's errors: why a mapper could not start, a
//! project could not be opened or a transform could not be used.

use std::fmt;

use tsc_diagnostics::spanmap::MappingError;

/// tsgo `InitializeErrorKind`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InitializeErrorKind {
    ProcessStart { command: String, detail: String },
    ProcessExit { exit_code: i32 },
    NoResponse { timeout_seconds: u64 },
    InvalidResponse { detail: String },
    Request { detail: String },
    PositionEncoding { encoding: String },
    EmptyDiagnosticSource,
    ReservedDiagnosticSource { source: String },
}

/// tsgo `InitializeError`: a mapper that could not be started or
/// initialized, by its diagnostic name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitializeError {
    pub kind: InitializeErrorKind,
    pub mapper_name: String,
}

impl fmt::Display for InitializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            InitializeErrorKind::ProcessStart { command, detail } => write!(
                formatter,
                "could not start content mapper command {command:?}: {detail}"
            ),
            InitializeErrorKind::ProcessExit { exit_code } => write!(
                formatter,
                "content mapper process exited before initialization with code {exit_code}"
            ),
            InitializeErrorKind::NoResponse { .. } => {
                formatter.write_str("content mapper did not respond to the initialize request")
            }
            InitializeErrorKind::InvalidResponse { detail } => write!(
                formatter,
                "content mapper returned an invalid initialize response: {detail}"
            ),
            InitializeErrorKind::Request { detail } => {
                write!(
                    formatter,
                    "content mapper initialize request failed: {detail}"
                )
            }
            InitializeErrorKind::PositionEncoding { encoding } => {
                write!(formatter, "unsupported position encoding {encoding:?}")
            }
            InitializeErrorKind::EmptyDiagnosticSource => {
                formatter.write_str("diagnostic source must not be empty")
            }
            InitializeErrorKind::ReservedDiagnosticSource { source } => write!(
                formatter,
                "diagnostic source {source:?} is reserved by TypeScript"
            ),
        }
    }
}

/// tsgo `ProjectErrorKind`: why a mapper's `openProject` response was
/// rejected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectErrorKind {
    MalformedResponse,
    MissingConfigIdentity,
    NonAbsoluteWatchedFile,
    UnexpectedConfigIdentity,
    UnexpectedWatchedFiles,
}

impl fmt::Display for ProjectErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MalformedResponse => "content mapper returned a malformed project response",
            Self::MissingConfigIdentity => {
                "content mapper did not return configIdentity for dynamic configuration"
            }
            Self::NonAbsoluteWatchedFile => {
                "content mapper returned a non-absolute path in watchedFiles"
            }
            Self::UnexpectedConfigIdentity => {
                "content mapper returned configIdentity without declaring dynamicConfig"
            }
            Self::UnexpectedWatchedFiles => {
                "content mapper returned watchedFiles without declaring dynamicConfig"
            }
        })
    }
}

/// Why a mapper project could not be opened: the mapper did not start, a
/// request failed, or the response broke the protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectFailure {
    Initialize(InitializeError),
    Request(String),
    Project(ProjectErrorKind),
}

impl fmt::Display for ProjectFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialize(error) => error.fmt(formatter),
            Self::Request(detail) => formatter.write_str(detail),
            Self::Project(kind) => kind.fmt(formatter),
        }
    }
}

/// tsgo `DiagnosticDirectiveErrorKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticDirectiveErrorKind {
    InvalidRange,
    InvalidPolicy,
    ExpectMissingUnusedDiagnostic,
    InvalidUnusedDiagnosticIndex,
    Overlap,
}

/// tsgo `DiagnosticDirectiveError`: an invalid directive, by its index in
/// its output (the canonical one, or the supplemental output at
/// `supplemental_index`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DiagnosticDirectiveError {
    pub kind: DiagnosticDirectiveErrorKind,
    pub index: usize,
    pub supplemental_index: Option<usize>,
    pub policy: u8,
}

impl fmt::Display for DiagnosticDirectiveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid content mapper diagnostic directive {}",
            self.index
        )
    }
}

/// What made a transform response unusable, as the file loader tells them
/// apart (tsgo's typed errors inside `TransformErrorKindResponse`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResponseFailure {
    /// tsgo `InvalidVirtualExtensionError`.
    InvalidVirtualExtension(String),
    DiagnosticDirective(DiagnosticDirectiveError),
    /// Any other malformed response, with tsgo's message.
    Malformed(String),
}

impl fmt::Display for ResponseFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVirtualExtension(extension) => {
                write!(formatter, "invalid virtual extension {extension:?}")
            }
            Self::DiagnosticDirective(error) => error.fmt(formatter),
            Self::Malformed(detail) => formatter.write_str(detail),
        }
    }
}

/// tsgo `TransformError` with its kind (`TransformErrorKind`) and the
/// typed error inside it, plus the failures tsgo reports without one
/// (an invalid span map, a supplemental file name that is taken).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransformFailure {
    /// The mapper could not be started or initialized.
    Initialize(InitializeError),
    /// The mapper project could not be opened.
    Project(ProjectFailure),
    /// The transform request failed.
    Request(String),
    /// The response could not be decoded.
    Response(ResponseFailure),
    /// tsgo `TransformErrorKindMappings`: the span map is invalid.
    Mappings(MappingError),
    /// tsgo `SupplementalFileCollisionError`.
    SupplementalFileCollision(String),
    /// The project was closed, or another host-side failure.
    Host(String),
}

impl fmt::Display for TransformFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialize(error) => {
                write!(formatter, "content mapper transform failed: {error}")
            }
            Self::Project(failure) => {
                write!(formatter, "content mapper transform failed: {failure}")
            }
            Self::Request(detail) | Self::Host(detail) => {
                write!(formatter, "content mapper transform failed: {detail}")
            }
            Self::Response(failure) => {
                write!(formatter, "content mapper transform failed: {failure}")
            }
            Self::Mappings(error) => error.fmt(formatter),
            Self::SupplementalFileCollision(file_name) => write!(
                formatter,
                "content mapper supplemental output file {file_name:?} already exists"
            ),
        }
    }
}

impl std::error::Error for TransformFailure {}
