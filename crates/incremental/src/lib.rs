//! Incremental compilation state (tsgo `execute/incremental`): the
//! `.tsbuildinfo` document an `incremental`/`composite` program writes, byte
//! for byte as tsgo writes it, and the snapshot it is produced from.
//!
//! The crate owns no checking or emitting. The compiler driver hands it the
//! program's facts (files in Program order, roots, options, references),
//! the checker's cached rows, and the emit's declaration-file signatures;
//! the crate produces the document.

#![forbid(unsafe_code)]

pub mod build_info;
pub mod hash;
pub mod json;
pub mod options;
pub mod snapshot;

pub use build_info::{
    BuildInfo, BuildInfoDiagnostic, BuildInfoRoot, EmitSignatureEntry, FileInfoEntry,
    SemanticDiagnosticEntry, VERSION,
};
pub use hash::compute_hash;
pub use snapshot::{
    build_fresh_build_info, DeclarationEmitFacts, DeclarationOutput, FileEmitKind, FileState,
    FreshSnapshotInput, ProgramFileFacts, ProgramState, SemanticRowsFacts,
};
