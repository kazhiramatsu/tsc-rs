//! Incremental compilation state (tsgo `execute/incremental`): the
//! `.tsbuildinfo` document an `incremental`/`composite` program writes, byte
//! for byte as tsgo writes it, the document read back, and the snapshot
//! the program keeps between the two.
//!
//! The crate owns no checking or emitting. The compiler driver hands it the
//! program's facts (files in Program order, roots, options, references),
//! the checker's cached rows, and the emit's declaration-file signatures;
//! the crate computes what the old state still covers and produces the
//! document.

#![forbid(unsafe_code)]

pub mod build_info;
pub mod hash;
pub mod json;
pub mod old_state;
pub mod options;
pub mod reader;
pub mod snapshot;

pub use build_info::{
    BuildInfo, BuildInfoDiagnostic, BuildInfoRoot, EmitSignatureEntry, FileInfoEntry,
    RepopulateInfo, SemanticDiagnosticEntry, VERSION,
};
pub use hash::compute_hash;
pub use old_state::{CachedDiagnostic, EmitSignature, OldFileInfo, OldState, OldStatePaths};
pub use snapshot::{
    build_fresh_build_info, declaration_write_decision, ensure_path_is_non_module_name,
    fresh_emit_updates, CachedRows, DeclarationEmit, DeclarationEmitFacts, DeclarationOutput,
    DeclarationWrite, EmitUpdate, FileEmitKind, FileState, FreshSnapshotInput, ProgramFileFacts,
    ProgramState, SemanticRowsFacts, Snapshot,
};
