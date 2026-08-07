use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

/// Dormant request axis that the bounded H1 bootstrap cannot execute.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnsupportedEmitFeature {
    TargetedSelection,
    BundleRoot,
    JavaScriptMap,
    Declaration,
    DeclarationMap,
    BuildInfo,
    DeclarationOnlyMode,
    BuilderSignatureMode,
    BuildInfoOnlyMode,
}

impl UnsupportedEmitFeature {
    pub const fn name(self) -> &'static str {
        match self {
            Self::TargetedSelection => "targeted source-file selection",
            Self::BundleRoot => "bundle root",
            Self::JavaScriptMap => "JavaScript source map",
            Self::Declaration => "declaration output",
            Self::DeclarationMap => "declaration map",
            Self::BuildInfo => "build info",
            Self::DeclarationOnlyMode => "declaration-only mode",
            Self::BuilderSignatureMode => "builder-signature mode",
            Self::BuildInfoOnlyMode => "build-info-only mode",
        }
    }
}

/// Internal pipeline stage intentionally unavailable at the current H1 slice.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitStage {
    TransformAndPrint,
    OutputPlanning,
    FilesystemSink,
}

impl EmitStage {
    pub const fn name(self) -> &'static str {
        match self {
            Self::TransformAndPrint => "transform and print",
            Self::OutputPlanning => "output planning",
            Self::FilesystemSink => "filesystem sink",
        }
    }
}

/// A malformed internal plan, distinct from a well-typed unsupported axis.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitContractViolation {
    ScriptOutputMissingJavaScriptPath,
}

/// Typed failure before or while orchestrating emission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitFailure {
    Unsupported(UnsupportedEmitFeature),
    StageUnavailable(EmitStage),
    Contract(EmitContractViolation),
}

impl fmt::Display for EmitFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(feature) => {
                write!(formatter, "unsupported emit request: {}", feature.name())
            }
            Self::StageUnavailable(stage) => {
                write!(
                    formatter,
                    "emit stage is not implemented yet: {}",
                    stage.name()
                )
            }
            Self::Contract(EmitContractViolation::ScriptOutputMissingJavaScriptPath) => {
                formatter.write_str("invalid emit plan: script output has no JavaScript path")
            }
        }
    }
}

impl Error for EmitFailure {}

/// Filesystem operation represented by an output-sink failure.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmitIoOperation {
    CreateParentDirectory,
    WriteFile,
}

/// Stable sink-owned I/O failure without embedding platform-specific error
/// objects in the emitter protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitIoError {
    operation: EmitIoOperation,
    path: PathBuf,
    message: String,
}

impl EmitIoError {
    pub fn new(
        operation: EmitIoOperation,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            path: path.into(),
            message: message.into(),
        }
    }

    pub const fn operation(&self) -> EmitIoOperation {
        self.operation
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EmitIoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}: {}",
            match self.operation {
                EmitIoOperation::CreateParentDirectory => "create output directory for",
                EmitIoOperation::WriteFile => "write output file",
            },
            self.path.display(),
            self.message,
        )
    }
}

impl Error for EmitIoError {}
