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
//! membership. JavaScript targets found while searching `node_modules` join
//! membership through the inclusive `maxNodeModuleJsDepth` boundary; every
//! external-library resolution edge advances that independent depth. Deeper
//! targets remain authoritative unloaded rows, and a source first found at a
//! positive depth reprocesses its references when a shallower or root path is
//! discovered later. A `.jsx` module target without an active JSX mode remains
//! unloaded for TS6142, while explicit `.jsx` roots and path references are
//! admitted. Effective `resolveJsonModule` also admits explicit JSON roots.
//! The raw `preserveSymlinks` program option controls resolver identity:
//! absent or false follows the physical target of external non-relative
//! modules and type references while retaining their lexical `originalPath`;
//! true keeps each lexical link as the resolved source identity.
//! The library catalog is injected, version-pinned metadata; bytes remain owned
//! by the same host and no production path parses `_tsc.js`.
//!
//! H0.5 exposes [`parse_config_root_plan`] for the config/root-planning
//! projection currently owned here: JSONC values, recoverable `extends`
//! sources, ordered partial diagnostics, three-state compiler-option values,
//! four root-discovery option values, and configured root names. The valid
//! projection matches all 103 config-bearing compiler fixtures covering 106
//! case expansions; a separate 51-fixture TypeScript oracle fixes malformed
//! partial-plan behavior, all seven compiler-option list conversions, and the
//! `paths` object conversion/base/template boundary. Five official compiler
//! `pathsValidation` cases additionally fix the six paths option diagnostics,
//! their UTF-16 locations, and ordering. Config-derived resolver options carry
//! `paths` atomically with its declaring base and share precompiled matching
//! metadata across resolver instances. This does not execute the remaining
//! compiler/project cases or cover the full `ParsedCommandLine`, remaining
//! root object schemas, general filesystem `matchFiles`, project-runner
//! configs, or CLI ownership.

mod config;
mod config_matcher;
mod config_options;
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

pub use config::{
    parse_config_root_plan, ConfigDiscoveryOptions, ConfigHostError, ConfigHostOperation,
    ConfigModuleResolutionOptions, ConfigOption, ConfigOptionBag, ConfigOptionValueState,
    ConfigParseError, ConfigParseErrorKind, ConfigParseHost, ConfigRootPlan, ConfigRootPlanRequest,
    ConfigSourceText, ConfigTypedJsonValue, ConfigTypedListElement, ConfigTypedObjectProperty,
    ConfigTypedObjectShape, ConfigTypedObjectValue,
};
pub use config_matcher::ConfigFilePattern;
pub use config_options::{
    compiler_option_declaration, compiler_option_declarations, compiler_option_spelling_suggestion,
    is_command_option_without_build, jsconfig_defaults, typescript_6_0_3_libraries,
    CompilerOptionDeclaration, CompilerOptionListDescriptor, CompilerOptionListElementKind,
    CompilerOptionNamedStringValue, CompilerOptionNamedValue, CompilerOptionObjectDescriptor,
    CompilerOptionValueKind, JsConfigDefaultValue, COMPILER_OPTION_DECLARATIONS, JSCONFIG_DEFAULTS,
};
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
pub use tsc_types::{CompilerOptionNumber, CompilerOptions, ModuleSuffix};
