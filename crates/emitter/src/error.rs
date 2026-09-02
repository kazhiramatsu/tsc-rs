use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tsc_program::SourceFileId;

use crate::{PrinterError, TransformError};

/// Dormant request axis that the bounded H1 bootstrap cannot execute.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnsupportedEmitFeature {
    TargetedSelection,
    BundleRoot,
    CustomTransformers,
    IsolatedDeclarations,
    JavaScriptMap,
    Declaration,
    DeclarationMap,
    BuildInfo,
    DeclarationOnlyMode,
    BuilderSignatureMode,
    BuildInfoOnlyMode,
    StandaloneNodePrinting,
    NodeListPrinting,
}

impl UnsupportedEmitFeature {
    pub const fn name(self) -> &'static str {
        match self {
            Self::TargetedSelection => "targeted source-file selection",
            Self::BundleRoot => "bundle root",
            Self::CustomTransformers => "custom declaration transformers",
            Self::IsolatedDeclarations => "isolated declarations",
            Self::JavaScriptMap => "JavaScript source map",
            Self::Declaration => "declaration output",
            Self::DeclarationMap => "declaration map",
            Self::BuildInfo => "build info",
            Self::DeclarationOnlyMode => "declaration-only mode",
            Self::BuilderSignatureMode => "builder-signature mode",
            Self::BuildInfoOnlyMode => "build-info-only mode",
            Self::StandaloneNodePrinting => "standalone-node printing",
            Self::NodeListPrinting => "node-list printing",
        }
    }
}

/// Named orchestration stage retained for future requests that reach an
/// unconnected pipeline axis. The H1.4 bootstrap path itself reaches
/// transform/print and output planning.
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
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmitContractViolation {
    ScriptOutputMissingJavaScriptPath,
    PlannedSourceMissing(SourceFileId),
    CheckedSyntaxUnavailable(SourceFileId),
    /// A mapped unit's print returned no recorded generator, or the URL
    /// offset left the UTF-16 position domain (h2-6a-m-3).
    SourceMapRecordingUnavailable,
}

/// Typed failure before or while orchestrating emission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitFailure {
    Unsupported(UnsupportedEmitFeature),
    UnsupportedCompilerOption { option: &'static str },
    UnsupportedSourceExtension { path: PathBuf },
    StageUnavailable(EmitStage),
    Contract(EmitContractViolation),
    Transform(Box<TransformError>),
    Printer(Box<PrinterError>),
}

impl fmt::Display for EmitFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(feature) => {
                write!(formatter, "unsupported emit request: {}", feature.name())
            }
            Self::UnsupportedCompilerOption { option } => {
                write!(formatter, "unsupported emit compiler option: {option}")
            }
            Self::UnsupportedSourceExtension { path } => write!(
                formatter,
                "unsupported emit source extension: {}",
                path.display()
            ),
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
            Self::Contract(EmitContractViolation::PlannedSourceMissing(source)) => write!(
                formatter,
                "invalid emit plan: SourceFileId {} is not present in the emit host",
                source.raw()
            ),
            Self::Contract(EmitContractViolation::SourceMapRecordingUnavailable) => formatter
                .write_str(
                    "invalid emit execution: a mapped unit produced no source-map recording",
                ),
            Self::Contract(EmitContractViolation::CheckedSyntaxUnavailable(source)) => write!(
                formatter,
                "invalid emit execution: SourceFileId {} has no checked syntax",
                source.raw()
            ),
            Self::Transform(error) => error.fmt(formatter),
            Self::Printer(error) => error.fmt(formatter),
        }
    }
}

impl Error for EmitFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transform(error) => Some(error.as_ref()),
            Self::Printer(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

impl From<TransformError> for EmitFailure {
    fn from(value: TransformError) -> Self {
        Self::Transform(Box::new(value))
    }
}

impl From<PrinterError> for EmitFailure {
    fn from(value: PrinterError) -> Self {
        Self::Printer(Box::new(value))
    }
}

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
