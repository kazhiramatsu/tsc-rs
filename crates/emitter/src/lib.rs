#![forbid(unsafe_code)]

//! Typed ownership boundary for JavaScript emission.
//!
//! H1.1 freezes the output topology and callback protocol before transformer,
//! printer, and filesystem behavior become executable. The crate deliberately
//! has no dependency on `tsc-rs-checker`; checker-owned semantic state will
//! implement emitter-owned protocols without creating a dependency cycle.

mod artifact;
mod error;
mod factory;
mod metadata;
mod outcome;
mod plan;
mod position;
mod printer;
mod sink;
mod transform;
mod writer;

pub use artifact::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitTextMetadata, EmitWriteMetadata,
};
pub use error::{
    EmitContractViolation, EmitFailure, EmitIoError, EmitIoOperation, EmitStage,
    UnsupportedEmitFeature,
};
pub use factory::{
    NodeFactory, TransformArena, TransformNode, TransformNodeArray, TransformSource,
    TransformSourceId,
};
pub use metadata::{
    EmitConstantValue, EmitFlags, EmitMetadata, InternalEmitFlags, JavaScriptString,
    SourceMapRange, SyntheticComment, SyntheticCommentKind,
};
pub use outcome::{EmitOutcome, SourceMapObservation};
pub use plan::{
    EmitBundle, EmitMode, EmitOutputPaths, EmitOutputPlan, EmitOutputUnit, EmitRoot, EmitSelection,
};
pub use position::{
    GeneratedUtf16Location, GeneratedUtf16Position, PositionDomain, SourceBytePosition,
    SourceByteRange, SourcePositionError, SourceRange, SourceUtf16Location, SourceUtf16Position,
};
pub use printer::{
    create_printer, DisabledSourceMapRecorder, PrintRequest, PrintedText, Printer, PrinterError,
    PrinterOptions, SourceMapHookEvent, SourceMapHookPhase, SourceMapRecorder,
};
pub use sink::{EmitWriteDisposition, MemoryOutputSink, OutputSink};
pub use transform::{
    transform_nodes, EmitHelper, EmitHint, LexicalEnvironment, LexicalEnvironmentFlags,
    TransformBundle, TransformError, TransformFlags, TransformRoot, TransformationContext,
    TransformationResult, TransformationState, Transformer,
};
pub use writer::{create_text_writer, NewLineKind, TextWriter};

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;
