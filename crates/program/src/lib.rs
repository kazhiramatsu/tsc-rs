#![forbid(unsafe_code)]

//! Owned H0 prepared-program data contract.
//!
//! This crate sits between read-only compiler hosts and the one-shot
//! parser/binder/checker session. It owns decoded source text, keeps display
//! paths separate from canonical lookup identities, preserves final program
//! order independently from root order, and accepts only authoritative typed
//! resolution outcomes.
//!
//! H0.4 exposes a bounded recursive no-lib loader through
//! [`load_no_lib_program`] and the catalog-enabled [`load_program`]. Both
//! discover path-, type-, and source-loading module dependencies through the
//! shared [`tsc_host::CompilerHost`] seam. The catalog-enabled route also owns
//! triple-slash lib references plus default and explicit library selection,
//! and publishes the stable default-library prefix before ordinary dependency
//! postorder while preserving root order and multiplicity. Library-owned path
//! references fail typed until [`PreparedProgram`] can represent TypeScript's
//! distinct processing-order and checker-membership sets. Discovery within
//! each source deliberately follows the vendored path/type/lib/module phases
//! and their observable failure precedence.
//!
//! Every load requires explicit source-count, request-occurrence, depth, and
//! raw-byte ceilings and reports host, decode, resolution, preparation,
//! unsupported-scope, and resource failures as typed [`ProgramLoadError`]s.
//! Relative source discovery applies ordered `rootDirs` with the vendored
//! longest-prefix and candidate ordering, while non-relative discovery applies
//! ordered `paths` mappings and `baseUrl` through the same resolver used by
//! direct resolution. Source-owned
//! type-reference directives use the shared Classic, Node10, Node16/NodeNext,
//! or Bundler primary/secondary lookup selected by the compiler options. Both
//! loaders also discover explicit and wildcard automatic type directives
//! after requested roots; the catalog-enabled route does so before selected
//! libraries. Wildcard discovery uses effective `typeRoots` and the host's
//! directory-only projection; a normalized config-file identity, when
//! supplied, anchors both that lookup and the synthetic inferred-types origin.
//! With `allowJs`, explicit JavaScript roots, local JavaScript module
//! dependencies, and supported JavaScript path references join ordinary source
//! membership. JavaScript targets found through `node_modules` remain
//! authoritative unloaded rows, matching the default
//! `maxNodeModuleJsDepth=0`; each unloaded row retains its source-membership
//! exclusion. A `.jsx` module target without an active JSX mode remains
//! unloaded for TS6142, while explicit `.jsx` roots and path references are
//! admitted. Effective `resolveJsonModule` also admits explicit JSON roots.
//! Nonzero depths remain outside this slice.
//! The library catalog is injected, version-pinned metadata; bytes remain owned
//! by the same host and no production path parses `_tsc.js`. This slice does
//! not discover config-derived root files or own extensionless/arbitrary
//! declaration root admission and the remaining path, physical-alias,
//! platform, and CLI surfaces of H0.4 and H0.5.

mod error;
mod json;
mod library;
mod loader;
mod module_requests;
mod module_resolution;
mod path;
mod prepared;
mod resolution;
mod text;

pub use error::{PreparationError, PreparationErrorKind, PreparationOperation};
pub use library::LibraryCatalog;
pub use loader::{
    load_no_lib_program, load_program, ProgramLoadError, ProgramLoadErrorKind, ProgramLoadLimit,
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
    TypeReferenceResolutionKey, TypeReferenceResolutionOrigin, UnloadedModuleReason,
};
pub use text::{decode_host_text, HostTextDecodeError, HostTextEncoding};
pub use tsc_types::CompilerOptions;
