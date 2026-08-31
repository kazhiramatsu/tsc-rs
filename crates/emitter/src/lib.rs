#![forbid(unsafe_code)]

//! Typed ownership boundary for JavaScript emission.
//!
//! H1.4 executes output planning, transform/print, and sink dispatch through
//! the topology frozen in H1.1. The crate deliberately has no dependency on
//! `tsc-rs-checker`; live checker state implements emitter-owned protocols
//! without creating a dependency cycle.

mod activity;
mod artifact;
mod builtins;
mod comment_cursor;
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
mod source_map;
mod token_cursor;
mod transform;
mod writer;

pub use activity::{H2ActivityCanary, H2ActivityCounters, H2RuntimeSlice};
pub use artifact::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitTextMetadata, EmitWriteMetadata,
};
pub use builtins::{
    get_script_transformers, get_script_transformers_for_source, transform_class_fields,
    transform_ecmascript_module, transform_type_script,
};
pub use error::{
    EmitContractViolation, EmitFailure, EmitIoError, EmitIoOperation, EmitStage,
    UnsupportedEmitFeature,
};
pub use execute::{
    base64_encode, emit_files, emit_files_with_activity,
    print_script_units_with_recording_for_harness, source_map_directory,
    source_map_recording_inputs_for, source_mapping_url, source_root_field,
    validate_bootstrap_emit_options, validate_bootstrap_emit_request, EmitDiagnosticGate,
    MapLaneInputs,
};
pub use factory::{
    NodeFactory, TransformArena, TransformNode, TransformNodeArray, TransformSource,
    TransformSourceId,
};
pub use host::{EmitHost, EmitSource};
pub use metadata::{
    CommentRange, EmitConstantValue, EmitEnumMemberValue, EmitFlags, EmitMetadata,
    InternalEmitFlags, JavaScriptNumber, JavaScriptString, SourceMapRange, SyntheticComment,
    SyntheticCommentKind,
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
    create_printer, PrintRequest, PrintedText, Printer, PrinterError, PrinterOptions,
    SourceFileTextMode,
};
pub use resolver::{
    EmitExportContainerMode, EmitFunctionProperty, EmitResolver, EmitResolverError,
    EmitResolverMethod, EmitResolverNode, EmitResolverSymbol, EmitSymbolAccessibility,
    EmitSymbolAccessibilityResult, EmitSymbolMeaning, EmitTypeReferenceSerializationKind,
    UnavailableEmitResolver,
};
pub use sink::{EmitFileSystem, EmitWriteDisposition, FsOutputSink, MemoryOutputSink, OutputSink};
pub use source_map::{SourceMapGenerator, SourceMapRecordingInputs, SourceMappingFields};
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

#[cfg(test)]
#[path = "../tests/unit/token_cursor/tests.rs"]
mod token_cursor_tests;
