#![forbid(unsafe_code)]

//! Owned H0 prepared-program data contract.
//!
//! This crate sits between read-only compiler hosts and the one-shot
//! parser/binder/checker session. It owns decoded source text, keeps display
//! paths separate from canonical lookup identities, preserves final program
//! order independently from root order, and accepts only authoritative typed
//! resolution outcomes.
//!
//! H0.4 adds the first bounded recursive program loader through
//! [`load_no_lib_program`]. It accepts explicit TypeScript-family roots under
//! explicit `noEmit=true` and `noLib=true`, discovers path-, type-, and
//! source-loading module dependencies through the shared [`tsc_host::CompilerHost`]
//! seam, and publishes unique sources in dependency postorder while preserving
//! root order and multiplicity. Resolution within each source deliberately
//! follows the vendored path/type/skipped-lib/module phases and their observable
//! failure precedence.
//!
//! Every load requires explicit source-count, request-occurrence, depth, and
//! raw-byte ceilings and reports host, decode, resolution, preparation,
//! unsupported-scope, and resource failures as typed [`ProgramLoadError`]s.
//! This first slice does not load default or explicit libraries, discover
//! automatic `types`, admit JavaScript sources, apply `paths`/`baseUrl` or
//! `rootDirs`, discover config roots, or claim the remaining platform and CLI
//! surfaces of H0.4 and H0.5.

mod error;
mod loader;
mod module_requests;
mod module_resolution;
mod path;
mod prepared;
mod resolution;
mod text;

pub use error::{PreparationError, PreparationErrorKind, PreparationOperation};
pub use loader::{
    load_no_lib_program, ProgramLoadError, ProgramLoadErrorKind, ProgramLoadLimit,
    ProgramLoadLimitExceeded, ProgramLoadLimits, ProgramLoadOperation,
};
pub use module_requests::{
    plan_module_requests, plan_source_requests, plan_static_module_requests,
    PlannedLibReferenceDirective, PlannedPathReference, PlannedTypeReferenceDirective,
    SourceRequestPlan,
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
pub use text::{decode_host_text, HostTextDecodeError, HostTextEncoding};
pub use tsc_types::CompilerOptions;
