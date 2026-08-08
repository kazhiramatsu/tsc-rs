#![forbid(unsafe_code)]

//! Typed ownership boundary for JavaScript emission.
//!
//! H1.4 executes output planning, transform/print, and sink dispatch through
//! the topology frozen in H1.1. The crate deliberately has no dependency on
//! `tsc-rs-checker`; live checker state implements emitter-owned protocols
//! without creating a dependency cycle.

mod artifact;
mod builtins;
mod error;
mod execute;
mod factory;
mod host;
mod metadata;
mod outcome;
mod plan;
mod position;
mod printer;
mod resolver;
mod sink;
mod transform;
mod writer;

pub use artifact::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitTextMetadata, EmitWriteMetadata,
};
pub use builtins::{
    get_script_transformers, transform_class_fields, transform_ecmascript_module,
    transform_type_script,
};
pub use error::{
    EmitContractViolation, EmitFailure, EmitIoError, EmitIoOperation, EmitStage,
    UnsupportedEmitFeature,
};
pub use execute::{
    emit_files, validate_bootstrap_emit_options, validate_bootstrap_emit_request,
    EmitDiagnosticGate,
};
pub use factory::{
    NodeFactory, TransformArena, TransformNode, TransformNodeArray, TransformSource,
    TransformSourceId,
};
pub use host::{EmitHost, EmitSource};
pub use metadata::{
    EmitConstantValue, EmitFlags, EmitMetadata, InternalEmitFlags, JavaScriptString,
    SourceMapRange, SyntheticComment, SyntheticCommentKind,
};
pub use outcome::{EmitOutcome, SourceMapObservation};
pub use plan::{
    for_each_emitted_file, get_output_paths_for, get_source_files_to_emit, preflight_emit,
    source_file_may_be_emitted, EmitBundle, EmitMode, EmitOutputPaths, EmitOutputPlan,
    EmitOutputUnit, EmitPreflight, EmitRoot, EmitSelection,
};
pub use position::{
    GeneratedUtf16Location, GeneratedUtf16Position, PositionDomain, SourceBytePosition,
    SourceByteRange, SourcePositionError, SourceRange, SourceUtf16Location, SourceUtf16Position,
};
pub use printer::{
    create_printer, DisabledSourceMapRecorder, PrintRequest, PrintedText, Printer, PrinterError,
    PrinterOptions, SourceMapHookEvent, SourceMapHookPhase, SourceMapRecorder,
};
pub use resolver::{
    EmitResolver, EmitResolverError, EmitResolverMethod, EmitResolverNode, UnavailableEmitResolver,
};
pub use sink::{EmitFileSystem, EmitWriteDisposition, FsOutputSink, MemoryOutputSink, OutputSink};
pub use transform::{
    transform_nodes, EmitHelper, EmitHint, LexicalEnvironment, LexicalEnvironmentFlags,
    TransformBundle, TransformError, TransformFlags, TransformRoot, TransformationContext,
    TransformationResult, TransformationState, Transformer, UnsupportedTransformFeature,
};
pub use tsc_program::SourceFileId;
pub use writer::{create_text_writer, NewLineKind, TextWriter};

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;
