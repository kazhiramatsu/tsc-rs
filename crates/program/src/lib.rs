#![forbid(unsafe_code)]

//! Owned H0 prepared-program data contract.
//!
//! This crate sits between read-only compiler hosts and the one-shot
//! parser/binder/checker session. It owns decoded source text, keeps display
//! paths separate from canonical lookup identities, preserves final program
//! order independently from root order, and accepts only authoritative typed
//! resolution outcomes.
//!
//! H0.2b adds the first bounded host producer: Node16/NodeNext/Bundler package
//! `exports` exact and pattern resolution. Broader source discovery, the
//! remaining resolver modes/features, and diagnostic execution stay in the
//! later program-loader and compiler-session slices.

mod error;
mod module_requests;
mod module_resolution;
mod path;
mod prepared;
mod resolution;

pub use error::{PreparationError, PreparationErrorKind, PreparationOperation};
pub use module_requests::{
    plan_module_requests, plan_source_requests, plan_static_module_requests,
    PlannedTypeReferenceDirective, SourceRequestPlan,
};
pub use module_resolution::{
    HostModuleResolution, HostResolvedModule, HostResolvedTypeReferenceDirective, ModuleResolver,
};
pub use path::{CanonicalPath, ProgramPath};
pub use prepared::{
    PackageJsonType, PackageMetadata, PathContext, PathMapping, PreparationDiagnostics,
    PreparedAuxiliaryFile, PreparedProgram, PreparedProgramBuilder, PreparedRoot,
    PreparedSourceFile, ProgramOptions, ResolutionTable, SourceFileId,
};
pub use resolution::{
    MissingResolutionError, ModuleExtension, ModuleResolution, PackageId, ResolutionError,
    ResolutionErrorKind, ResolutionKey, ResolutionMode, ResolutionOutcome, ResolutionRequestKind,
    ResolvedModule, ResolvedModuleTarget, ResolvedTypeReferenceDirective, TypeReferenceResolution,
    TypeReferenceResolutionKey, TypeReferenceResolutionOrigin,
};
pub use tsc_types::CompilerOptions;
