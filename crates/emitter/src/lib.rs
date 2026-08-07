#![forbid(unsafe_code)]

//! Typed ownership boundary for JavaScript emission.
//!
//! H1.1 freezes the output topology and callback protocol before transformer,
//! printer, and filesystem behavior become executable. The crate deliberately
//! has no dependency on `tsc-rs-checker`; checker-owned semantic state will
//! implement emitter-owned protocols without creating a dependency cycle.

mod artifact;
mod error;
mod outcome;
mod plan;
mod sink;

pub use artifact::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitTextMetadata, EmitWriteMetadata,
    GeneratedUtf16Position,
};
pub use error::{
    EmitContractViolation, EmitFailure, EmitIoError, EmitIoOperation, EmitStage,
    UnsupportedEmitFeature,
};
pub use outcome::{EmitOutcome, SourceMapObservation};
pub use plan::{
    EmitBundle, EmitMode, EmitOutputPaths, EmitOutputPlan, EmitOutputUnit, EmitRoot, EmitSelection,
};
pub use sink::{EmitWriteDisposition, MemoryOutputSink, OutputSink};

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;
