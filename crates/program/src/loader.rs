use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tsc_diagnostics::{gen, Diagnostic, MessageChain};
use tsc_host::{to_file_name_lower_case, CompilerHost, HostError};
use tsc_types::CompilerOptions;

use crate::json::{json_object_get, parse_json_object};
use crate::library::LibraryCatalog;
use crate::module_requests::{
    is_declaration_file_name, plan_source_requests, PlannedLibReferenceDirective,
    PlannedPathReference, PlannedTypeReferenceDirective,
};
use crate::module_resolution::{
    directory_name, make_program_path, normalize_absolute_path, HostModuleResolution,
    HostResolvedTypeReferenceDirective, ModuleResolver,
};
use crate::path::{CanonicalPath, ProgramPath};
use crate::prepared::{
    PackageJsonType, PackageMetadata, PathContext, PreparationDiagnostics, PreparedProgram,
    PreparedRoot, PreparedSourceFile, ProgramOptions, SourceFileId,
};
use crate::resolution::{
    ModuleExtension, ModuleResolution, PackageId, ResolutionError, ResolutionKey, ResolutionMode,
    ResolutionOutcome, ResolvedModuleTarget, TypeReferenceResolution, TypeReferenceResolutionKey,
    UnloadedModuleReason,
};
use crate::text::{decode_host_text, HostTextDecodeError};
use crate::PreparationError;

/// The deepest source chain admitted by the recursive H0.4 loader worker.
///
/// A caller may declare a larger resource ceiling, but the structural ceiling
/// remains in force so adversarial input cannot overflow the Rust call stack.
const MAX_RECURSIVE_SOURCE_DEPTH: usize = 256;

const TYPESCRIPT_SOURCE_EXTENSIONS: [&str; 7] =
    [".ts", ".tsx", ".d.ts", ".cts", ".d.cts", ".mts", ".d.mts"];
const JAVASCRIPT_SOURCE_EXTENSIONS: [&str; 4] = [".js", ".jsx", ".mjs", ".cjs"];
const TYPESCRIPT_PATH_REFERENCE_PROBE_EXTENSIONS: [&str; 3] = [".ts", ".tsx", ".d.ts"];
const JAVASCRIPT_PATH_REFERENCE_PROBE_EXTENSIONS: [&str; 2] = [".js", ".jsx"];
const TYPESCRIPT_SOURCE_EXTENSION_LIST: &str =
    "'.ts', '.tsx', '.d.ts', '.cts', '.d.cts', '.mts', '.d.mts'";
const ALL_SOURCE_EXTENSION_LIST: &str =
    "'.ts', '.tsx', '.d.ts', '.js', '.jsx', '.cts', '.d.cts', '.cjs', '.mts', '.d.mts', '.mjs'";
const INFERRED_TYPES_CONTAINING_FILE: &str = "__inferred type names__.ts";

/// Explicit resource limits for one [`load_no_lib_program`] or [`load_program`]
/// call.
///
/// Byte limits apply to unique source payloads after `CompilerHost::read_file`
/// returns. The current host contract returns an owned `Vec<u8>`, so these
/// limits bound retained/decoded source work but cannot prevent the host's
/// one-call allocation. Resolver- and wildcard-discovery-owned `package.json`
/// payloads are likewise outside these source-byte counters. The request-edge
/// limit counts the final automatic-name occurrences after wildcard filtering,
/// not raw directory entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLoadLimits {
    max_source_files: usize,
    max_request_edges: usize,
    max_source_depth: usize,
    max_source_file_bytes: usize,
    max_total_source_bytes: usize,
}

impl ProgramLoadLimits {
    pub const fn new(
        max_source_files: usize,
        max_request_edges: usize,
        max_source_depth: usize,
        max_source_file_bytes: usize,
        max_total_source_bytes: usize,
    ) -> Self {
        Self {
            max_source_files,
            max_request_edges,
            max_source_depth,
            max_source_file_bytes,
            max_total_source_bytes,
        }
    }

    pub const fn max_source_files(self) -> usize {
        self.max_source_files
    }

    pub const fn max_request_edges(self) -> usize {
        self.max_request_edges
    }

    /// Maximum zero-based source depth. Roots have depth zero.
    pub const fn max_source_depth(self) -> usize {
        self.max_source_depth
    }

    pub const fn max_source_file_bytes(self) -> usize {
        self.max_source_file_bytes
    }

    pub const fn max_total_source_bytes(self) -> usize {
        self.max_total_source_bytes
    }
}

/// The independently bounded dimensions of program source discovery.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProgramLoadLimit {
    SourceFiles,
    RequestEdges,
    SourceDepth,
    SourceFileBytes,
    TotalSourceBytes,
}

impl ProgramLoadLimit {
    pub const fn name(self) -> &'static str {
        match self {
            Self::SourceFiles => "source files",
            Self::RequestEdges => "source request edges",
            Self::SourceDepth => "source depth",
            Self::SourceFileBytes => "source file bytes",
            Self::TotalSourceBytes => "total source bytes",
        }
    }
}

/// Structured evidence for a rejected resource observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramLoadLimitExceeded {
    limit: ProgramLoadLimit,
    path: Option<PathBuf>,
    maximum: usize,
    observed: usize,
}

impl ProgramLoadLimitExceeded {
    pub const fn limit(&self) -> ProgramLoadLimit {
        self.limit
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub const fn maximum(&self) -> usize {
        self.maximum
    }

    pub const fn observed(&self) -> usize {
        self.observed
    }
}

/// Stable loader stages used to preserve deterministic failure context.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProgramLoadOperation {
    ValidateOptions,
    InitializeResolver,
    NormalizeRoot,
    NormalizeReference,
    ReadSource,
    ObserveRealPath,
    DecodeSource,
    ObservePackageScope,
    PlanSourceRequests,
    DiscoverAutomaticTypes,
    ResolveTypeReference,
    ResolveModule,
    BindResolutions,
    BuildPreparedProgram,
}

impl ProgramLoadOperation {
    pub const fn name(self) -> &'static str {
        match self {
            Self::ValidateOptions => "validate program loader options",
            Self::InitializeResolver => "initialize program resolver",
            Self::NormalizeRoot => "normalize root path",
            Self::NormalizeReference => "normalize path reference",
            Self::ReadSource => "read program source",
            Self::ObserveRealPath => "observe source real path",
            Self::DecodeSource => "decode program source",
            Self::ObservePackageScope => "observe source package scope",
            Self::PlanSourceRequests => "plan source requests",
            Self::DiscoverAutomaticTypes => "discover automatic type directives",
            Self::ResolveTypeReference => "resolve type-reference directive",
            Self::ResolveModule => "resolve module request",
            Self::BindResolutions => "bind authoritative resolutions",
            Self::BuildPreparedProgram => "build loaded prepared program",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProgramLoadErrorKind {
    InvalidInput,
    Unsupported,
    InvalidData,
    ResourceLimit,
    Host,
    Decode,
    Resolution,
    Preparation,
}

/// Typed failure from deterministic program source discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramLoadError {
    InvalidInput {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        detail: String,
    },
    Unsupported {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        feature: String,
        detail: String,
    },
    InvalidData {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        detail: String,
    },
    LimitExceeded {
        operation: ProgramLoadOperation,
        exceeded: ProgramLoadLimitExceeded,
    },
    Host {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        source: HostError,
    },
    Decode {
        operation: ProgramLoadOperation,
        path: PathBuf,
        source: HostTextDecodeError,
    },
    Resolution {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        specifier: Option<String>,
        source: ResolutionError,
    },
    Preparation {
        operation: ProgramLoadOperation,
        source: PreparationError,
    },
}

impl ProgramLoadError {
    pub const fn kind(&self) -> ProgramLoadErrorKind {
        match self {
            Self::InvalidInput { .. } => ProgramLoadErrorKind::InvalidInput,
            Self::Unsupported { .. } => ProgramLoadErrorKind::Unsupported,
            Self::InvalidData { .. } => ProgramLoadErrorKind::InvalidData,
            Self::LimitExceeded { .. } => ProgramLoadErrorKind::ResourceLimit,
            Self::Host { .. } => ProgramLoadErrorKind::Host,
            Self::Decode { .. } => ProgramLoadErrorKind::Decode,
            Self::Resolution { .. } => ProgramLoadErrorKind::Resolution,
            Self::Preparation { .. } => ProgramLoadErrorKind::Preparation,
        }
    }

    pub const fn operation(&self) -> ProgramLoadOperation {
        match self {
            Self::InvalidInput { operation, .. }
            | Self::Unsupported { operation, .. }
            | Self::InvalidData { operation, .. }
            | Self::LimitExceeded { operation, .. }
            | Self::Host { operation, .. }
            | Self::Decode { operation, .. }
            | Self::Resolution { operation, .. }
            | Self::Preparation { operation, .. } => *operation,
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::InvalidInput { path, .. }
            | Self::Unsupported { path, .. }
            | Self::InvalidData { path, .. }
            | Self::Host { path, .. }
            | Self::Resolution { path, .. } => path.as_deref(),
            Self::LimitExceeded { exceeded, .. } => exceeded.path(),
            Self::Decode { path, .. } => Some(path),
            Self::Preparation { source, .. } => source.path(),
        }
    }

    pub fn limit_exceeded(&self) -> Option<&ProgramLoadLimitExceeded> {
        match self {
            Self::LimitExceeded { exceeded, .. } => Some(exceeded),
            _ => None,
        }
    }

    fn invalid_input(
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        detail: impl Into<String>,
    ) -> Self {
        Self::InvalidInput {
            operation,
            path,
            detail: detail.into(),
        }
    }

    fn unsupported(
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        feature: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::Unsupported {
            operation,
            path,
            feature: feature.into(),
            detail: detail.into(),
        }
    }

    fn invalid_data(
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        detail: impl Into<String>,
    ) -> Self {
        Self::InvalidData {
            operation,
            path,
            detail: detail.into(),
        }
    }

    fn host(operation: ProgramLoadOperation, path: Option<PathBuf>, source: HostError) -> Self {
        Self::Host {
            operation,
            path,
            source,
        }
    }

    fn resolution(
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        specifier: Option<String>,
        source: ResolutionError,
    ) -> Self {
        Self::Resolution {
            operation,
            path,
            specifier,
            source,
        }
    }

    fn preparation(operation: ProgramLoadOperation, source: PreparationError) -> Self {
        Self::Preparation { operation, source }
    }
}

impl fmt::Display for ProgramLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.operation().name())?;
        if let Some(path) = self.path() {
            write!(formatter, " for {}", path.display())?;
        }
        match self {
            Self::InvalidInput { detail, .. } | Self::InvalidData { detail, .. } => {
                write!(formatter, ": {detail}")
            }
            Self::Unsupported {
                feature, detail, ..
            } => {
                write!(formatter, ": unsupported feature {feature}")?;
                if !detail.is_empty() {
                    write!(formatter, ": {detail}")?;
                }
                Ok(())
            }
            Self::LimitExceeded { exceeded, .. } => write!(
                formatter,
                ": {} limit {} exceeded by observation {}",
                exceeded.limit.name(),
                exceeded.maximum,
                exceeded.observed
            ),
            Self::Host { source, .. } => write!(formatter, ": {source}"),
            Self::Decode { source, .. } => write!(formatter, ": {source}"),
            Self::Resolution {
                specifier, source, ..
            } => {
                if let Some(specifier) = specifier {
                    write!(formatter, " for specifier {specifier:?}")?;
                }
                write!(formatter, ": {source}")
            }
            Self::Preparation { source, .. } => write!(formatter, ": {source}"),
        }
    }
}

impl Error for ProgramLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Host { source, .. } => Some(source),
            Self::Decode { source, .. } => Some(source),
            Self::Resolution { source, .. } => Some(source),
            Self::Preparation { source, .. } => Some(source),
            Self::InvalidInput { .. }
            | Self::Unsupported { .. }
            | Self::InvalidData { .. }
            | Self::LimitExceeded { .. } => None,
        }
    }
}

/// Load one finite TypeScript root closure without default libraries.
///
/// Roots are processed in input order. Each source is discovered with the
/// upstream path-reference, type-reference, skipped-lib, and module phases,
/// and is published after its children. Type and module phases resolve every
/// exact key before descending into the first target, preserving observable
/// host-failure precedence. Explicit or wildcard automatic type directives
/// run after all requested roots when that list is non-empty. The returned
/// program owns all source text and resolution facts and no longer borrows
/// `host`.
pub fn load_no_lib_program(
    host: &dyn CompilerHost,
    root_names: &[PathBuf],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_program_worker(
        host,
        root_names,
        compiler_options,
        program_options,
        None,
        true,
        limits,
    )
}

/// Load one finite TypeScript root closure with an injected standard-library
/// catalog.
///
/// User roots retain their observable discovery order. Within each source,
/// path, type, library, and module phases run in the vendored order; selected
/// default or explicit library roots run only after every user root and the
/// post-root automatic type-directive phase. The returned source list is then
/// published as the stable default-library prefix followed by ordinary
/// dependency postorder, without replaying any host operation. Library-owned
/// path references fail typed until
/// [`PreparedProgram`] can represent TypeScript's distinct processing-order
/// and checker-membership sets.
pub fn load_program(
    host: &dyn CompilerHost,
    root_names: &[PathBuf],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_program_worker(
        host,
        root_names,
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
    )
}

fn load_program_worker(
    host: &dyn CompilerHost,
    root_names: &[PathBuf],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: Option<&LibraryCatalog>,
    require_no_lib: bool,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    validate_admitted_options(
        &compiler_options,
        &program_options,
        library_catalog,
        require_no_lib,
    )?;

    let mut resolver =
        ModuleResolver::new_with_program_options(host, &compiler_options, &program_options)
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::InitializeResolver,
                    error.path().map(Path::to_path_buf),
                    None,
                    error,
                )
            })?;
    let path_context = resolver.path_context().clone();
    validate_type_roots(&program_options, &path_context)?;
    let library_directory = if program_options.no_lib() == Some(true) {
        None
    } else {
        Some(normalize_library_directory(
            library_catalog.expect("validated library-enabled load has a catalog"),
            &path_context,
        )?)
    };

    let mut graph = StagedGraph::new(
        host,
        &compiler_options,
        &program_options,
        library_catalog,
        library_directory,
        limits,
        &mut resolver,
    );
    for root_name in root_names {
        let root = normalize_root(root_name, &path_context)?;
        graph.load_root(root)?;
    }
    if !root_names.is_empty() {
        graph.load_automatic_type_directives()?;
        if program_options.no_lib() != Some(true) {
            graph.load_selected_libraries()?;
        }
    }
    let staged = graph.finish();
    let packages = resolver
        .observed_package_metadata()
        .cloned()
        .collect::<Vec<_>>();
    drop(resolver);

    publish_program(
        staged,
        packages,
        path_context,
        compiler_options,
        program_options,
    )
}

fn validate_admitted_options(
    compiler_options: &CompilerOptions,
    program_options: &ProgramOptions,
    library_catalog: Option<&LibraryCatalog>,
    require_no_lib: bool,
) -> Result<(), ProgramLoadError> {
    let reject_input = |detail| {
        ProgramLoadError::invalid_input(ProgramLoadOperation::ValidateOptions, None, detail)
    };
    let reject_feature = |feature, detail| {
        ProgramLoadError::unsupported(ProgramLoadOperation::ValidateOptions, None, feature, detail)
    };

    if compiler_options.no_emit != Some(true) {
        return Err(reject_input(
            "compilerOptions.noEmit must be explicitly true",
        ));
    }
    if require_no_lib && program_options.no_lib() != Some(true) {
        return Err(reject_input("programOptions.noLib must be explicitly true"));
    }
    if program_options.no_lib() == Some(true) && compiler_options.lib.is_some() {
        return Err(reject_feature(
            "explicit-libraries",
            "the noLib/lib option diagnostic is owned by the later H0.5 driver",
        ));
    }
    if program_options.no_lib() != Some(true) {
        let Some(catalog) = library_catalog else {
            return Err(reject_input(
                "library-enabled program loading requires an injected LibraryCatalog",
            ));
        };
        if let Some(value) = compiler_options.lib.as_deref().and_then(|libs| {
            libs.iter()
                .find(|value| catalog.option_file_name(value).is_none())
        }) {
            return Err(ProgramLoadError::invalid_input(
                ProgramLoadOperation::ValidateOptions,
                None,
                format!("compilerOptions.lib contains unknown library key {value:?}"),
            ));
        }
    }
    if compiler_options.no_dts_resolution == Some(true) {
        return Err(reject_feature(
            "noDtsResolution",
            "the first recursive loader requires ordinary declaration resolution",
        ));
    }
    Ok(())
}

fn normalize_library_directory(
    catalog: &LibraryCatalog,
    path_context: &PathContext,
) -> Result<ProgramPath, ProgramLoadError> {
    reject_unowned_windows_path(catalog.directory(), ProgramLoadOperation::ValidateOptions)?;
    let current_directory = path_context
        .current_directory()
        .display()
        .to_str()
        .expect("resolver path context is representable");
    let normalized = normalize_absolute_path(catalog.directory(), Some(current_directory))
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::ValidateOptions,
                Some(catalog.directory().to_path_buf()),
                None,
                error,
            )
        })?;
    make_program_path(&normalized, path_context.use_case_sensitive_file_names()).map_err(|error| {
        ProgramLoadError::resolution(
            ProgramLoadOperation::ValidateOptions,
            Some(catalog.directory().to_path_buf()),
            None,
            error,
        )
    })
}

fn normalize_root(
    root: &Path,
    path_context: &PathContext,
) -> Result<ProgramPath, ProgramLoadError> {
    let current_directory = path_context
        .current_directory()
        .display()
        .to_str()
        .expect("resolver path context is representable");
    reject_unowned_windows_path(root, ProgramLoadOperation::NormalizeRoot)?;
    let normalized = normalize_absolute_path(root, Some(current_directory)).map_err(|error| {
        ProgramLoadError::resolution(
            ProgramLoadOperation::NormalizeRoot,
            Some(root.to_path_buf()),
            None,
            error,
        )
    })?;
    if !normalized
        .rsplit('/')
        .next()
        .is_some_and(|base_name| base_name.contains('.'))
    {
        return Err(ProgramLoadError::unsupported(
            ProgramLoadOperation::NormalizeRoot,
            Some(PathBuf::from(&normalized)),
            "root-extensionless",
            "extensionless roots require the upstream .ts/.tsx/.d.ts/.js/.jsx probe and TS6231 diagnostic boundary",
        ));
    }
    let path = make_program_path(&normalized, path_context.use_case_sensitive_file_names())
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::NormalizeRoot,
                Some(root.to_path_buf()),
                None,
                error,
            )
        })?;
    Ok(path)
}

fn validate_type_roots(
    options: &ProgramOptions,
    path_context: &PathContext,
) -> Result<(), ProgramLoadError> {
    let Some(type_roots) = options.type_roots() else {
        return Ok(());
    };
    let current_directory = path_context
        .current_directory()
        .display()
        .to_str()
        .expect("resolver path context is representable");
    for type_root in type_roots {
        reject_unowned_windows_path(type_root.display(), ProgramLoadOperation::ValidateOptions)?;
        let normalized = normalize_absolute_path(type_root.display(), Some(current_directory))
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::ValidateOptions,
                    Some(type_root.display().to_path_buf()),
                    None,
                    error,
                )
            })?;
        let normalized =
            make_program_path(&normalized, path_context.use_case_sensitive_file_names()).map_err(
                |error| {
                    ProgramLoadError::resolution(
                        ProgramLoadOperation::ValidateOptions,
                        Some(type_root.display().to_path_buf()),
                        None,
                        error,
                    )
                },
            )?;
        if &normalized != type_root {
            return Err(ProgramLoadError::invalid_input(
                ProgramLoadOperation::ValidateOptions,
                Some(type_root.display().to_path_buf()),
                "typeRoots entries must already carry normalized display and canonical identities",
            ));
        }
    }
    Ok(())
}

fn reject_unowned_windows_path(
    path: &Path,
    operation: ProgramLoadOperation,
) -> Result<(), ProgramLoadError> {
    let Some(text) = path.to_str() else {
        return Err(ProgramLoadError::invalid_input(
            operation,
            Some(path.to_path_buf()),
            "path is not valid Unicode",
        ));
    };
    let slashed = text.replace('\\', "/");
    let drive_relative = slashed.len() >= 2
        && slashed.as_bytes()[0].is_ascii_alphabetic()
        && slashed.as_bytes()[1] == b':'
        && slashed.as_bytes().get(2) != Some(&b'/');
    if slashed.starts_with("//") || slashed.starts_with("//?/") || drive_relative {
        return Err(ProgramLoadError::unsupported(
            operation,
            Some(path.to_path_buf()),
            "windows-path-form",
            "UNC, extended-length, and drive-relative paths are not yet owned",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum VisitState {
    Visiting(usize),
    Complete(usize),
    Missing,
}

impl VisitState {
    const fn source(self) -> Option<usize> {
        match self {
            Self::Visiting(source) | Self::Complete(source) => Some(source),
            Self::Missing => None,
        }
    }
}

#[derive(Clone, Copy)]
struct DiscoveryReason {
    seeds_non_external_reachability: bool,
}

impl DiscoveryReason {
    const ROOT: Self = Self {
        seeds_non_external_reachability: true,
    };
    const DEPENDENCY: Self = Self {
        seeds_non_external_reachability: false,
    };

    const fn automatic_type(is_external_library_import: bool) -> Self {
        Self {
            seeds_non_external_reachability: !is_external_library_import,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceClass {
    Ordinary,
    Library,
}

struct StagedSource {
    prepared: PreparedSourceFile,
    has_non_external_reason: bool,
    class: SourceClass,
}

struct StagedRoot {
    path: ProgramPath,
    source: Option<usize>,
    missing_diagnostic: Option<Diagnostic>,
}

enum LibraryRootReason {
    Default { target: String },
    Explicit { file_name: String },
}

struct StagedModuleResolution {
    key: ResolutionKey,
    host: HostModuleResolution,
    loads_source: bool,
}

struct StagedTypeResolution {
    key: TypeReferenceResolutionKey,
    host: ResolutionOutcome<HostResolvedTypeReferenceDirective>,
    diagnostics: Vec<Diagnostic>,
}

struct CompleteGraph {
    sources: Vec<StagedSource>,
    library_postorder: Vec<usize>,
    ordinary_postorder: Vec<usize>,
    roots: Vec<StagedRoot>,
    module_resolutions: Vec<StagedModuleResolution>,
    type_resolutions: Vec<StagedTypeResolution>,
    program_diagnostics: Vec<Diagnostic>,
}

struct StagedGraph<'host, 'options, 'resolver> {
    host: &'host dyn CompilerHost,
    compiler_options: &'options CompilerOptions,
    program_options: &'options ProgramOptions,
    library_catalog: Option<&'options LibraryCatalog>,
    library_directory: Option<ProgramPath>,
    limits: ProgramLoadLimits,
    resolver: &'resolver mut ModuleResolver<'host>,
    states: BTreeMap<CanonicalPath, VisitState>,
    physical_owners: BTreeMap<CanonicalPath, usize>,
    sources: Vec<StagedSource>,
    source_edges: Vec<Vec<(usize, bool)>>,
    postorder: Vec<usize>,
    roots: Vec<StagedRoot>,
    module_resolution_by_key: BTreeMap<ResolutionKey, usize>,
    module_resolutions: Vec<StagedModuleResolution>,
    type_resolution_by_key: BTreeMap<TypeReferenceResolutionKey, usize>,
    type_resolutions: Vec<StagedTypeResolution>,
    package_targets: BTreeMap<PackageId, CanonicalPath>,
    diagnosed_missing_roots: BTreeSet<PathBuf>,
    diagnosed_missing_library_roots: BTreeSet<PathBuf>,
    program_diagnostics: Vec<Diagnostic>,
    request_edges: usize,
    total_source_bytes: usize,
}

impl<'host, 'options, 'resolver> StagedGraph<'host, 'options, 'resolver> {
    fn new(
        host: &'host dyn CompilerHost,
        compiler_options: &'options CompilerOptions,
        program_options: &'options ProgramOptions,
        library_catalog: Option<&'options LibraryCatalog>,
        library_directory: Option<ProgramPath>,
        limits: ProgramLoadLimits,
        resolver: &'resolver mut ModuleResolver<'host>,
    ) -> Self {
        Self {
            host,
            compiler_options,
            program_options,
            library_catalog,
            library_directory,
            limits,
            resolver,
            states: BTreeMap::new(),
            physical_owners: BTreeMap::new(),
            sources: Vec::new(),
            source_edges: Vec::new(),
            postorder: Vec::new(),
            roots: Vec::new(),
            module_resolution_by_key: BTreeMap::new(),
            module_resolutions: Vec::new(),
            type_resolution_by_key: BTreeMap::new(),
            type_resolutions: Vec::new(),
            package_targets: BTreeMap::new(),
            diagnosed_missing_roots: BTreeSet::new(),
            diagnosed_missing_library_roots: BTreeSet::new(),
            program_diagnostics: Vec::new(),
            request_edges: 0,
            total_source_bytes: 0,
        }
    }

    fn load_root(&mut self, path: ProgramPath) -> Result<(), ProgramLoadError> {
        if !is_admitted_source(path.canonical(), self.compiler_options) {
            let diagnostic =
                unsupported_root_extension_diagnostic(&path, self.compiler_options.allow_js)?;
            if self
                .diagnosed_missing_roots
                .insert(path.display().to_path_buf())
            {
                self.program_diagnostics.push(diagnostic.clone());
            }
            self.roots.push(StagedRoot {
                path,
                source: None,
                missing_diagnostic: Some(diagnostic),
            });
            return Ok(());
        }
        let source = self.visit_source(
            path.clone(),
            0,
            DiscoveryReason::ROOT,
            SourceClass::Ordinary,
        )?;
        let missing_diagnostic = source.is_none().then(|| missing_root_diagnostic(&path));
        if let Some(diagnostic) = missing_diagnostic.clone() {
            if self
                .diagnosed_missing_roots
                .insert(path.display().to_path_buf())
            {
                self.program_diagnostics.push(diagnostic);
            }
        }
        self.roots.push(StagedRoot {
            path,
            source,
            missing_diagnostic,
        });
        Ok(())
    }

    fn load_automatic_type_directives(&mut self) -> Result<(), ProgramLoadError> {
        let (names, uses_wildcard) = self.automatic_type_directive_names()?;
        if names.is_empty() {
            return Ok(());
        }

        let containing_file = self.automatic_types_containing_file()?;
        let request_edges = self.request_edges.saturating_add(names.len());
        self.enforce_limit(
            ProgramLoadOperation::DiscoverAutomaticTypes,
            ProgramLoadLimit::RequestEdges,
            Some(containing_file.display().to_path_buf()),
            self.limits.max_request_edges,
            request_edges,
        )?;
        self.request_edges = request_edges;

        let type_roots = self.program_options.type_roots().map(<[_]>::to_vec);
        let mut resolution_indices = Vec::with_capacity(names.len());
        for name in &names {
            let key = TypeReferenceResolutionKey::automatic(
                containing_file.canonical().clone(),
                name.clone(),
            );
            let index = if let Some(index) = self.type_resolution_by_key.get(&key).copied() {
                index
            } else {
                let host = self
                    .resolver
                    .resolve_type_reference(
                        containing_file.display(),
                        name,
                        ResolutionMode::Unspecified,
                        type_roots.as_deref(),
                    )
                    .map_err(|error| {
                        ProgramLoadError::resolution(
                            ProgramLoadOperation::ResolveTypeReference,
                            Some(containing_file.display().to_path_buf()),
                            Some(name.clone()),
                            error,
                        )
                    })?;
                let index = self.type_resolutions.len();
                self.type_resolutions.push(StagedTypeResolution {
                    key: key.clone(),
                    host,
                    diagnostics: Vec::new(),
                });
                self.type_resolution_by_key.insert(key, index);
                index
            };
            resolution_indices.push(index);
        }

        // Vendored createProgram resolves the complete batch before it starts
        // processing the first target, then processes names sequentially.
        // Repeated explicit names reuse the same mode-aware cache entry.
        let mut processed = BTreeSet::new();
        for (name, index) in names.into_iter().zip(resolution_indices) {
            let target = match &self.type_resolutions[index].host {
                ResolutionOutcome::Resolved(target) => Some((
                    target.resolved_file().clone(),
                    target.extension().clone(),
                    target.is_external_library_import(),
                )),
                ResolutionOutcome::NotFound => None,
            };
            let Some((target, extension, external)) = target else {
                self.type_resolutions[index]
                    .diagnostics
                    .push(automatic_type_reference_diagnostic(&name, uses_wildcard));
                continue;
            };
            if !processed.insert(index) {
                continue;
            }
            if !is_loadable_typescript_extension(&extension) {
                return Err(ProgramLoadError::invalid_data(
                    ProgramLoadOperation::ResolveTypeReference,
                    Some(target.display().to_path_buf()),
                    "a resolved automatic type-reference target is not a TypeScript source file",
                ));
            }
            if self
                .visit_source(
                    target.clone(),
                    0,
                    DiscoveryReason::automatic_type(external),
                    SourceClass::Ordinary,
                )?
                .is_none()
            {
                return Err(ProgramLoadError::invalid_data(
                    ProgramLoadOperation::ReadSource,
                    Some(target.display().to_path_buf()),
                    "resolver reported an automatic type-reference target that the host no longer returns",
                ));
            }
        }
        Ok(())
    }

    fn automatic_type_directive_names(&mut self) -> Result<(Vec<String>, bool), ProgramLoadError> {
        let configured = self
            .program_options
            .types()
            .map_or_else(Vec::new, <[_]>::to_vec);
        let uses_wildcard = configured.iter().any(|name| name == "*");
        if !uses_wildcard {
            return Ok((configured, false));
        }

        let wildcard_matches = self.discover_wildcard_type_directives()?;
        let mut seen = BTreeSet::new();
        let mut names = Vec::new();
        for configured_name in configured {
            if configured_name == "*" {
                for wildcard_match in &wildcard_matches {
                    if seen.insert(wildcard_match.clone()) {
                        names.push(wildcard_match.clone());
                    }
                }
            } else if seen.insert(configured_name.clone()) {
                names.push(configured_name);
            }
        }
        Ok((names, true))
    }

    fn discover_wildcard_type_directives(&mut self) -> Result<Vec<String>, ProgramLoadError> {
        let roots = self
            .resolver
            .effective_type_roots(self.program_options.type_roots())
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::DiscoverAutomaticTypes,
                    error.path().map(Path::to_path_buf),
                    None,
                    error,
                )
            })?;
        let mut matches = Vec::new();
        for root in roots {
            let root_path = Path::new(&root);
            if !self.host.directory_exists(root_path).map_err(|error| {
                ProgramLoadError::host(
                    ProgramLoadOperation::DiscoverAutomaticTypes,
                    Some(root_path.to_path_buf()),
                    error,
                )
            })? {
                continue;
            }
            let directories = self.host.get_directories(root_path).map_err(|error| {
                ProgramLoadError::host(
                    ProgramLoadOperation::DiscoverAutomaticTypes,
                    Some(root_path.to_path_buf()),
                    error,
                )
            })?;
            for directory in directories {
                let name = directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| {
                        ProgramLoadError::invalid_data(
                            ProgramLoadOperation::DiscoverAutomaticTypes,
                            Some(directory.clone()),
                            "automatic type directory has no Unicode base name",
                        )
                    })?
                    .to_owned();
                let package_json = root_path.join(&name).join("package.json");
                if self.automatic_package_has_null_typings(&package_json)? {
                    continue;
                }
                // TypeScript probes package.json before applying the hidden
                // directory filter, so retain that observable failure order.
                if !name.starts_with('.') {
                    matches.push(name);
                }
            }
        }
        Ok(matches)
    }

    fn automatic_package_has_null_typings(
        &self,
        package_json: &Path,
    ) -> Result<bool, ProgramLoadError> {
        if !self.host.file_exists(package_json).map_err(|error| {
            ProgramLoadError::host(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                Some(package_json.to_path_buf()),
                error,
            )
        })? {
            return Ok(false);
        }
        let Some(bytes) = self.host.read_file(package_json).map_err(|error| {
            ProgramLoadError::host(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                Some(package_json.to_path_buf()),
                error,
            )
        })?
        else {
            return Ok(false);
        };
        let text = decode_host_text(bytes).map_err(|source| ProgramLoadError::Decode {
            operation: ProgramLoadOperation::DiscoverAutomaticTypes,
            path: package_json.to_path_buf(),
            source,
        })?;
        let (_, object) = parse_json_object(package_json, text);
        Ok(json_object_get(&object, "typings").is_some_and(serde_json::Value::is_null))
    }

    fn automatic_types_containing_file(&self) -> Result<ProgramPath, ProgramLoadError> {
        let normalized = normalize_absolute_path(
            Path::new(INFERRED_TYPES_CONTAINING_FILE),
            Some(self.resolver.type_root_base_directory()),
        )
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                None,
                None,
                error,
            )
        })?;
        make_program_path(
            &normalized,
            self.resolver.path_context().use_case_sensitive_file_names(),
        )
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                Some(PathBuf::from(normalized)),
                None,
                error,
            )
        })
    }

    fn load_selected_libraries(&mut self) -> Result<(), ProgramLoadError> {
        let catalog = self
            .library_catalog
            .expect("library-enabled graph has an injected catalog");
        let selected = match self.compiler_options.lib.as_deref() {
            Some(libraries) => libraries
                .iter()
                .map(|value| {
                    let file_name = catalog
                        .option_file_name(value)
                        .expect("unknown library keys were rejected during option validation");
                    (
                        file_name,
                        LibraryRootReason::Explicit {
                            file_name: file_name.to_owned(),
                        },
                    )
                })
                .collect::<Vec<_>>(),
            None => {
                let file_name = catalog.default_file_name(self.compiler_options);
                vec![(
                    file_name,
                    LibraryRootReason::Default {
                        target: script_target_name(self.compiler_options).to_owned(),
                    },
                )]
            }
        };
        for (file_name, reason) in selected {
            let path = self.library_path(file_name)?;
            if self
                .visit_source(
                    path.clone(),
                    0,
                    DiscoveryReason::DEPENDENCY,
                    SourceClass::Library,
                )?
                .is_none()
                && self
                    .diagnosed_missing_library_roots
                    .insert(path.display().to_path_buf())
            {
                let diagnostic = missing_library_root_diagnostic(&path, &reason);
                let replaced_root_diagnostics = self
                    .roots
                    .iter_mut()
                    .filter(|root| root.source.is_none() && root.path.display() == path.display())
                    .filter_map(|root| root.missing_diagnostic.replace(diagnostic.clone()))
                    .collect::<Vec<_>>();
                for replaced in replaced_root_diagnostics {
                    self.program_diagnostics
                        .retain(|existing| existing != &replaced);
                }
                self.program_diagnostics.push(diagnostic);
            }
        }
        Ok(())
    }

    fn finish(mut self) -> CompleteGraph {
        self.propagate_non_external_reachability();
        let mut library_postorder = self
            .postorder
            .iter()
            .copied()
            .filter(|&source| self.sources[source].class == SourceClass::Library)
            .collect::<Vec<_>>();
        if !library_postorder.is_empty() {
            let catalog = self
                .library_catalog
                .expect("a library source requires an injected catalog");
            let directory = self
                .library_directory
                .as_ref()
                .expect("a library source requires a normalized catalog directory")
                .display();
            library_postorder.sort_by_key(|&source| {
                let path = self.sources[source].prepared.path().display();
                catalog.priority(directory, path)
            });
        }
        let ordinary_postorder = self
            .postorder
            .iter()
            .copied()
            .filter(|&source| self.sources[source].class == SourceClass::Ordinary)
            .collect();
        CompleteGraph {
            sources: self.sources,
            library_postorder,
            ordinary_postorder,
            roots: self.roots,
            module_resolutions: self.module_resolutions,
            type_resolutions: self.type_resolutions,
            program_diagnostics: self.program_diagnostics,
        }
    }

    fn visit_source(
        &mut self,
        path: ProgramPath,
        depth: usize,
        reason: DiscoveryReason,
        class: SourceClass,
    ) -> Result<Option<usize>, ProgramLoadError> {
        if let Some(state) = self.states.get(path.canonical()).copied() {
            if let Some(source) = state.source() {
                let first_path = self.sources[source].prepared.path();
                if first_path.display() != path.display() {
                    return Err(ProgramLoadError::unsupported(
                        ProgramLoadOperation::ReadSource,
                        Some(path.display().to_path_buf()),
                        "canonical-source-display-alias",
                        format!(
                            "the path has the same canonical identity as already discovered source {} but a different display spelling",
                            first_path.display().display()
                        ),
                    ));
                }
                if self.sources[source].class != class {
                    return Err(ProgramLoadError::unsupported(
                        ProgramLoadOperation::ReadSource,
                        Some(path.display().to_path_buf()),
                        "library-source-classification-collision",
                        format!(
                            "the source was first discovered as {:?} and later requested as {:?}",
                            self.sources[source].class, class
                        ),
                    ));
                }
                self.sources[source].has_non_external_reason |=
                    reason.seeds_non_external_reachability;
            }
            return Ok(state.source());
        }
        if let Some(&owner) = self.physical_owners.get(path.canonical()) {
            return Err(ProgramLoadError::unsupported(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_path_buf()),
                "physical-source-alias",
                format!(
                    "the path aliases already discovered source {} whose lexical identity differs",
                    self.sources[owner].prepared.path().display().display()
                ),
            ));
        }

        let bytes = self.host.read_file(path.display()).map_err(|error| {
            ProgramLoadError::host(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_path_buf()),
                error,
            )
        })?;
        let Some(bytes) = bytes else {
            self.states
                .insert(path.canonical().clone(), VisitState::Missing);
            return Ok(None);
        };

        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::SourceDepth,
            Some(path.display().to_path_buf()),
            self.limits.max_source_depth.min(MAX_RECURSIVE_SOURCE_DEPTH),
            depth,
        )?;
        let source_count = self.sources.len().saturating_add(1);
        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::SourceFiles,
            Some(path.display().to_path_buf()),
            self.limits.max_source_files,
            source_count,
        )?;
        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::SourceFileBytes,
            Some(path.display().to_path_buf()),
            self.limits.max_source_file_bytes,
            bytes.len(),
        )?;
        let total_source_bytes = self.total_source_bytes.saturating_add(bytes.len());
        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::TotalSourceBytes,
            Some(path.display().to_path_buf()),
            self.limits.max_total_source_bytes,
            total_source_bytes,
        )?;

        let text = decode_host_text(bytes).map_err(|source| ProgramLoadError::Decode {
            operation: ProgramLoadOperation::DecodeSource,
            path: path.display().to_path_buf(),
            source,
        })?;
        let real_path = self.observe_real_path(&path)?;
        if let Some(real_path) = &real_path {
            if let Some(existing) = self
                .states
                .get(real_path.canonical())
                .and_then(|state| state.source())
            {
                return Err(self.physical_alias_error(&path, existing));
            }
            if let Some(&existing) = self.physical_owners.get(real_path.canonical()) {
                return Err(self.physical_alias_error(&path, existing));
            }
        }
        let package_scope = self
            .resolver
            .package_scope_for_file(path.display())
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::ObservePackageScope,
                    Some(path.display().to_path_buf()),
                    None,
                    error,
                )
            })?;
        let file_name = path
            .display()
            .to_str()
            .expect("program paths are representable");
        let implied = implied_node_format(file_name, package_scope.as_ref(), self.compiler_options);
        let implied_for_emit =
            implied_node_format_for_emit(file_name, package_scope.as_ref(), self.compiler_options);
        let mut prepared = PreparedSourceFile::new(path.clone(), text)
            .with_implied_node_formats(implied, implied_for_emit);
        if is_json_source(path.canonical()) {
            prepared = prepared.with_may_be_emitted(false);
        }
        if let Some(real_path) = real_path {
            prepared = prepared.with_real_path(real_path);
        }
        if let Some(package_scope) = package_scope {
            prepared =
                prepared.with_package_scope(package_scope.package_json().canonical().clone());
        }
        let plan = if is_json_source(path.canonical()) {
            None
        } else {
            Some(
                plan_source_requests(&prepared, self.compiler_options).map_err(|error| {
                    ProgramLoadError::resolution(
                        ProgramLoadOperation::PlanSourceRequests,
                        Some(path.display().to_path_buf()),
                        None,
                        error,
                    )
                })?,
            )
        };
        let request_edges = self.request_edges.saturating_add(
            plan.as_ref()
                .map_or(0, |plan| plan.observed_request_occurrence_count()),
        );
        self.enforce_limit(
            ProgramLoadOperation::PlanSourceRequests,
            ProgramLoadLimit::RequestEdges,
            Some(path.display().to_path_buf()),
            self.limits.max_request_edges,
            request_edges,
        )?;

        self.total_source_bytes = total_source_bytes;
        self.request_edges = request_edges;
        let source = self.sources.len();
        if let Some(real_path) = prepared.real_path() {
            self.physical_owners
                .insert(real_path.canonical().clone(), source);
        }
        self.sources.push(StagedSource {
            prepared,
            has_non_external_reason: reason.seeds_non_external_reachability,
            class,
        });
        self.source_edges.push(Vec::new());
        self.states
            .insert(path.canonical().clone(), VisitState::Visiting(source));

        if let Some(plan) = plan {
            if self.sources[source].class == SourceClass::Library
                && !plan.path_references().is_empty()
            {
                return Err(ProgramLoadError::unsupported(
                    ProgramLoadOperation::PlanSourceRequests,
                    Some(path.display().to_path_buf()),
                    "default-library-path-references",
                    "default-library path-reference descendants have processing-prefix order without checker-visible library membership, which the current PreparedProgram prefix cannot represent",
                ));
            }
            let path_references = plan.path_references().to_vec();
            for reference in path_references {
                self.process_path_reference(source, &reference, depth)?;
            }
            self.process_type_references(source, plan.type_reference_directives().to_vec(), depth)?;
            if self.program_options.no_lib() != Some(true) {
                self.process_lib_references(
                    source,
                    plan.lib_reference_directives().to_vec(),
                    depth,
                )?;
            }
            // `noLib=true` deliberately performs no host operation for lib
            // directives, although their occurrences were counted above.
            self.process_module_requests(
                source,
                plan.module_requests_with_loadability()
                    .map(|(key, loads_source)| (key.clone(), loads_source))
                    .collect(),
                depth,
            )?;
        }

        self.states
            .insert(path.canonical().clone(), VisitState::Complete(source));
        self.postorder.push(source);
        Ok(Some(source))
    }

    fn observe_real_path(
        &self,
        path: &ProgramPath,
    ) -> Result<Option<ProgramPath>, ProgramLoadError> {
        let observed = self.host.realpath(path.display()).map_err(|error| {
            ProgramLoadError::host(
                ProgramLoadOperation::ObserveRealPath,
                Some(path.display().to_path_buf()),
                error,
            )
        })?;
        let Some(observed) = observed else {
            return Err(ProgramLoadError::invalid_data(
                ProgramLoadOperation::ObserveRealPath,
                Some(path.display().to_path_buf()),
                "host returned source bytes but no real path for the same entry",
            ));
        };
        let current_directory = self
            .resolver
            .path_context()
            .current_directory()
            .display()
            .to_str()
            .expect("resolver path context is representable");
        let normalized =
            normalize_absolute_path(&observed, Some(current_directory)).map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::ObserveRealPath,
                    Some(observed.clone()),
                    None,
                    error,
                )
            })?;
        let real = make_program_path(
            &normalized,
            self.resolver.path_context().use_case_sensitive_file_names(),
        )
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::ObserveRealPath,
                Some(observed),
                None,
                error,
            )
        })?;
        Ok((real.canonical() != path.canonical()).then_some(real))
    }

    fn library_path(&self, file_name: &str) -> Result<ProgramPath, ProgramLoadError> {
        let directory = self
            .library_directory
            .as_ref()
            .expect("library-enabled graph has a normalized catalog directory");
        let base = directory
            .display()
            .to_str()
            .expect("program paths are representable");
        let normalized =
            normalize_absolute_path(Path::new(file_name), Some(base)).map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::NormalizeReference,
                    Some(directory.display().to_path_buf()),
                    Some(file_name.to_owned()),
                    error,
                )
            })?;
        make_program_path(
            &normalized,
            self.resolver.path_context().use_case_sensitive_file_names(),
        )
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::NormalizeReference,
                Some(directory.display().to_path_buf()),
                Some(file_name.to_owned()),
                error,
            )
        })
    }

    fn process_lib_references(
        &mut self,
        source: usize,
        directives: Vec<PlannedLibReferenceDirective>,
        depth: usize,
    ) -> Result<(), ProgramLoadError> {
        let catalog = self
            .library_catalog
            .expect("library-enabled graph has an injected catalog");
        for directive in directives {
            let lib_name = to_file_name_lower_case(directive.file_name());
            let Some(file_name) = catalog.reference_file_name(&lib_name) else {
                let suggestion = catalog.spelling_suggestion(&lib_name);
                let (message, arguments) = match suggestion {
                    Some(suggestion) => (
                        &gen::Cannot_find_lib_definition_for_0_Did_you_mean_1,
                        vec![lib_name, suggestion.to_owned()],
                    ),
                    None => (&gen::Cannot_find_lib_definition_for_0, vec![lib_name]),
                };
                self.program_diagnostics.push(located_diagnostic(
                    &self.sources[source].prepared,
                    directive.pos(),
                    directive.length(),
                    message,
                    &arguments,
                )?);
                continue;
            };
            let target = self.library_path(file_name)?;
            match self.visit_source(
                target.clone(),
                depth.saturating_add(1),
                DiscoveryReason::DEPENDENCY,
                SourceClass::Library,
            )? {
                Some(target_source) if target_source == source => {
                    self.program_diagnostics.push(located_diagnostic(
                        &self.sources[source].prepared,
                        directive.pos(),
                        directive.length(),
                        &gen::A_file_cannot_have_a_reference_to_itself,
                        &[],
                    )?);
                }
                Some(_) => {}
                None => {
                    self.program_diagnostics.push(located_diagnostic(
                        &self.sources[source].prepared,
                        directive.pos(),
                        directive.length(),
                        &gen::File_0_not_found,
                        &[path_text(target.display())?],
                    )?);
                }
            }
        }
        Ok(())
    }

    fn process_path_reference(
        &mut self,
        source: usize,
        reference: &PlannedPathReference,
        depth: usize,
    ) -> Result<(), ProgramLoadError> {
        if reference.file_name().is_empty() {
            return Err(ProgramLoadError::unsupported(
                ProgramLoadOperation::NormalizeReference,
                Some(self.sources[source].prepared.path().display().to_path_buf()),
                "empty-path-reference",
                "empty triple-slash path references are not yet admitted",
            ));
        }
        let source_path = self.sources[source].prepared.path().clone();
        let source_text = source_path
            .display()
            .to_str()
            .expect("program paths are representable");
        let base = directory_name(source_text);
        let normalized = normalize_absolute_path(Path::new(reference.file_name()), Some(&base))
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_path_buf()),
                    Some(reference.file_name().to_owned()),
                    error,
                )
            })?;
        let has_extension = reference
            .file_name()
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.contains('.'));
        let child_depth = depth.saturating_add(1);
        if has_extension {
            let target = make_program_path(
                &normalized,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_path_buf()),
                    Some(reference.file_name().to_owned()),
                    error,
                )
            })?;
            let is_json = is_json_source(target.canonical());
            if !(is_typescript_source(target.canonical())
                || is_javascript_source(target.canonical()) && self.compiler_options.allow_js
                || is_json && self.compiler_options.resolve_json_module_effective())
            {
                let (message, arguments) = if is_javascript_source(target.canonical()) {
                    (
                        &gen::File_0_is_a_JavaScript_file_Did_you_mean_to_enable_the_allowJs_option,
                        vec![path_text(target.display())?],
                    )
                } else {
                    (
                        &gen::File_0_has_an_unsupported_extension_The_only_supported_extensions_are_1,
                        vec![
                            path_text(target.display())?,
                            supported_source_extension_list(self.compiler_options.allow_js)
                                .to_owned(),
                        ],
                    )
                };
                self.program_diagnostics.push(located_diagnostic(
                    &self.sources[source].prepared,
                    reference.pos(),
                    reference.length(),
                    message,
                    &arguments,
                )?);
                return Ok(());
            }
            let target_source = self.visit_source(
                target.clone(),
                child_depth,
                DiscoveryReason::DEPENDENCY,
                self.sources[source].class,
            )?;
            match target_source {
                Some(target_source) if target_source == source => {
                    self.record_source_edge(source, target_source, false);
                    self.program_diagnostics.push(located_diagnostic(
                        &self.sources[source].prepared,
                        reference.pos(),
                        reference.length(),
                        &gen::A_file_cannot_have_a_reference_to_itself,
                        &[],
                    )?)
                }
                Some(target_source) => self.record_source_edge(source, target_source, false),
                None => self.program_diagnostics.push(located_diagnostic(
                    &self.sources[source].prepared,
                    reference.pos(),
                    reference.length(),
                    &gen::File_0_not_found,
                    &[path_text(target.display())?],
                )?),
            }
            return Ok(());
        }

        let javascript_extensions: &[&str] = if self.compiler_options.allow_js {
            &JAVASCRIPT_PATH_REFERENCE_PROBE_EXTENSIONS
        } else {
            &[]
        };
        for &extension in TYPESCRIPT_PATH_REFERENCE_PROBE_EXTENSIONS
            .iter()
            .chain(javascript_extensions)
        {
            let target_text = format!("{normalized}{extension}");
            let target = make_program_path(
                &target_text,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_path_buf()),
                    Some(reference.file_name().to_owned()),
                    error,
                )
            })?;
            if let Some(target_source) = self.visit_source(
                target,
                child_depth,
                DiscoveryReason::DEPENDENCY,
                self.sources[source].class,
            )? {
                self.record_source_edge(source, target_source, false);
                if target_source == source {
                    self.program_diagnostics.push(located_diagnostic(
                        &self.sources[source].prepared,
                        reference.pos(),
                        reference.length(),
                        &gen::A_file_cannot_have_a_reference_to_itself,
                        &[],
                    )?);
                }
                return Ok(());
            }
        }
        self.program_diagnostics.push(located_diagnostic(
            &self.sources[source].prepared,
            reference.pos(),
            reference.length(),
            &gen::Could_not_resolve_the_path_0_with_the_extensions_1,
            &[
                normalized,
                supported_source_extension_list(self.compiler_options.allow_js).to_owned(),
            ],
        )?);
        Ok(())
    }

    fn process_type_references(
        &mut self,
        source: usize,
        directives: Vec<PlannedTypeReferenceDirective>,
        depth: usize,
    ) -> Result<(), ProgramLoadError> {
        let mut phase_indices = Vec::new();
        let mut phase_seen = BTreeSet::new();
        let containing_source = self.sources[source].prepared.path().clone();
        let type_roots = self.program_options.type_roots().map(<[_]>::to_vec);
        for directive in &directives {
            let key = directive.key().clone();
            let index = if let Some(index) = self.type_resolution_by_key.get(&key).copied() {
                index
            } else {
                let host = self
                    .resolver
                    .resolve_type_reference(
                        containing_source.display(),
                        key.specifier(),
                        key.mode(),
                        type_roots.as_deref(),
                    )
                    .map_err(|error| {
                        ProgramLoadError::resolution(
                            ProgramLoadOperation::ResolveTypeReference,
                            Some(containing_source.display().to_path_buf()),
                            Some(key.specifier().to_owned()),
                            error,
                        )
                    })?;
                let index = self.type_resolutions.len();
                self.type_resolutions.push(StagedTypeResolution {
                    key: key.clone(),
                    host,
                    diagnostics: Vec::new(),
                });
                self.type_resolution_by_key.insert(key.clone(), index);
                index
            };
            if phase_seen.insert(index) {
                phase_indices.push(index);
            }
        }

        // Resolution of every key above succeeds before diagnostics or child
        // traversal from the first directive can occur.
        for directive in &directives {
            let index = self.type_resolution_by_key[directive.key()];
            if matches!(
                self.type_resolutions[index].host,
                ResolutionOutcome::NotFound
            ) {
                let diagnostic = unresolved_type_reference_diagnostic(
                    &self.sources[source].prepared,
                    directive,
                )?;
                self.type_resolutions[index].diagnostics.push(diagnostic);
            }
        }

        for index in phase_indices {
            let target = match &self.type_resolutions[index].host {
                ResolutionOutcome::Resolved(target) => Some((
                    target.resolved_file().clone(),
                    target.extension().clone(),
                    target.is_external_library_import(),
                )),
                ResolutionOutcome::NotFound => None,
            };
            let Some((target, extension, external)) = target else {
                continue;
            };
            if !is_loadable_typescript_extension(&extension) {
                return Err(ProgramLoadError::invalid_data(
                    ProgramLoadOperation::ResolveTypeReference,
                    Some(target.display().to_path_buf()),
                    "a resolved type-reference target is not a TypeScript source file",
                ));
            }
            let loaded = self.visit_source(
                target.clone(),
                depth.saturating_add(1),
                DiscoveryReason::DEPENDENCY,
                SourceClass::Ordinary,
            )?;
            let Some(target_source) = loaded else {
                return Err(ProgramLoadError::invalid_data(
                    ProgramLoadOperation::ReadSource,
                    Some(target.display().to_path_buf()),
                    "resolver reported a type-reference target that the host no longer returns",
                ));
            };
            self.record_source_edge(source, target_source, external);
        }
        Ok(())
    }

    /// tsc-port: processImportedModules @6.0.3
    /// tsc-hash: 5fb6c5d9e11130467d843f258aeb726b1cbca21cd00923b0f1c7da3097f9cc98
    /// tsc-span: _tsc.js:124595-124635
    fn process_module_requests(
        &mut self,
        source: usize,
        requests: Vec<(ResolutionKey, bool)>,
        depth: usize,
    ) -> Result<(), ProgramLoadError> {
        let mut phase_indices = Vec::with_capacity(requests.len());
        let containing_file = self.sources[source].prepared.path().display().to_path_buf();
        let containing_file_is_declaration = containing_file
            .to_str()
            .is_some_and(is_declaration_file_name);
        for (key, loads_source) in requests {
            let index = if let Some(index) = self.module_resolution_by_key.get(&key).copied() {
                self.module_resolutions[index].loads_source |= loads_source;
                index
            } else {
                let host = self
                    .resolver
                    .resolve_with_facts(&containing_file, key.specifier(), key.mode())
                    .map_err(|error| {
                        ProgramLoadError::resolution(
                            ProgramLoadOperation::ResolveModule,
                            Some(containing_file.clone()),
                            Some(key.specifier().to_owned()),
                            error,
                        )
                    })?;
                self.observe_package_target(&host)?;
                let index = self.module_resolutions.len();
                self.module_resolutions.push(StagedModuleResolution {
                    key: key.clone(),
                    host,
                    loads_source,
                });
                self.module_resolution_by_key.insert(key, index);
                index
            };
            phase_indices.push(index);
        }

        // As with type directives, all requests in this source are resolved
        // before the first successful target starts its DFS.
        for index in phase_indices {
            let loads_source = self.module_resolutions[index].loads_source;
            if !loads_source {
                continue;
            }
            let target = match self.module_resolutions[index].host.outcome() {
                ResolutionOutcome::Resolved(target) => Some((
                    target.resolved_file().clone(),
                    target.extension().clone(),
                    target.is_external_library_import(),
                    target.original_path().is_some(),
                )),
                ResolutionOutcome::NotFound => None,
            };
            let Some((target, extension, external, has_original_path)) = target else {
                continue;
            };
            if extension.is_javascript() {
                // tsc's default maxNodeModuleJsDepth is zero. Local JS joins
                // the program under allowJs, while a JS target found through
                // node_modules retains its resolution row without becoming a
                // source. Non-zero depth ownership is a later H0.4 slice.
                if let Some(reason) = unloaded_javascript_reason(
                    &extension,
                    self.compiler_options,
                    external,
                    has_original_path,
                    target.canonical(),
                    loads_source,
                ) {
                    if has_original_path
                        && !matches!(reason, UnloadedModuleReason::JsxWithoutJsxOption)
                    {
                        return Err(ProgramLoadError::unsupported(
                            ProgramLoadOperation::ResolveModule,
                            Some(target.display().to_path_buf()),
                            "unloaded-original-path",
                            "an unloaded JavaScript target cannot retain a lexical-to-physical transition",
                        ));
                    }
                    continue;
                }
            }
            if is_arbitrary_declaration_extension(&extension)
                && self.compiler_options.allow_arbitrary_extensions != Some(true)
                && !containing_file_is_declaration
            {
                // The resolution row remains authoritative so the diagnostic
                // layer can report TS6263, but createProgram does not add the
                // declaration twin to source membership in this case.
                continue;
            }
            if has_original_path {
                return Err(ProgramLoadError::unsupported(
                    ProgramLoadOperation::ResolveModule,
                    Some(target.display().to_path_buf()),
                    "loaded-original-path",
                    "a loaded source target cannot retain a lexical-to-physical transition at the authoritative checker boundary",
                ));
            }
            if matches!(extension, ModuleExtension::Json) {
                if !self.compiler_options.resolve_json_module_effective() {
                    return Err(ProgramLoadError::unsupported(
                        ProgramLoadOperation::ResolveModule,
                        Some(target.display().to_path_buf()),
                        "resolveJsonModule",
                        "a JSON target was resolved while resolveJsonModule is disabled",
                    ));
                }
                let loaded = self.visit_source(
                    target.clone(),
                    depth.saturating_add(1),
                    DiscoveryReason::DEPENDENCY,
                    SourceClass::Ordinary,
                )?;
                let Some(target_source) = loaded else {
                    return Err(ProgramLoadError::invalid_data(
                        ProgramLoadOperation::ReadSource,
                        Some(target.display().to_path_buf()),
                        "resolver reported a JSON module target that the host no longer returns",
                    ));
                };
                self.record_source_edge(source, target_source, external);
                continue;
            }
            if !is_loadable_typescript_extension(&extension) && !extension.is_javascript() {
                return Err(ProgramLoadError::unsupported(
                    ProgramLoadOperation::ResolveModule,
                    Some(target.display().to_path_buf()),
                    "resolved-module-extension",
                    format!(
                        "loadable target extension {} is outside the admitted source loader",
                        extension.as_str()
                    ),
                ));
            }
            let loaded = self.visit_source(
                target.clone(),
                depth.saturating_add(1),
                DiscoveryReason::DEPENDENCY,
                SourceClass::Ordinary,
            )?;
            let Some(target_source) = loaded else {
                return Err(ProgramLoadError::invalid_data(
                    ProgramLoadOperation::ReadSource,
                    Some(target.display().to_path_buf()),
                    "resolver reported a module target that the host no longer returns",
                ));
            };
            self.record_source_edge(source, target_source, external);
        }
        Ok(())
    }

    fn observe_package_target(
        &mut self,
        resolution: &HostModuleResolution,
    ) -> Result<(), ProgramLoadError> {
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            return Ok(());
        };
        let Some(package_id) = module.package_id() else {
            return Ok(());
        };
        let target = module.resolved_file().canonical();
        match self.package_targets.get(package_id) {
            Some(existing) if existing != target => Err(ProgramLoadError::unsupported(
                ProgramLoadOperation::ResolveModule,
                Some(module.resolved_file().display().to_path_buf()),
                "package-source-redirect",
                format!(
                    "package identity {:?} resolves to both {} and {}",
                    package_id.name(),
                    existing,
                    target
                ),
            )),
            Some(_) => Ok(()),
            None => {
                self.package_targets
                    .insert(package_id.clone(), target.clone());
                Ok(())
            }
        }
    }

    fn enforce_limit(
        &self,
        operation: ProgramLoadOperation,
        limit: ProgramLoadLimit,
        path: Option<PathBuf>,
        maximum: usize,
        observed: usize,
    ) -> Result<(), ProgramLoadError> {
        if observed <= maximum {
            return Ok(());
        }
        Err(ProgramLoadError::LimitExceeded {
            operation,
            exceeded: ProgramLoadLimitExceeded {
                limit,
                path,
                maximum,
                observed,
            },
        })
    }

    fn record_source_edge(
        &mut self,
        source: usize,
        target: usize,
        crosses_external_library_boundary: bool,
    ) {
        self.source_edges[source].push((target, crosses_external_library_boundary));
    }

    fn propagate_non_external_reachability(&mut self) {
        let mut pending = self
            .sources
            .iter()
            .enumerate()
            .filter_map(|(source, staged)| staged.has_non_external_reason.then_some(source))
            .collect::<Vec<_>>();
        while let Some(source) = pending.pop() {
            for &(target, crosses_external_library_boundary) in &self.source_edges[source] {
                if crosses_external_library_boundary || self.sources[target].has_non_external_reason
                {
                    continue;
                }
                self.sources[target].has_non_external_reason = true;
                pending.push(target);
            }
        }
    }

    fn physical_alias_error(&self, path: &ProgramPath, existing: usize) -> ProgramLoadError {
        ProgramLoadError::unsupported(
            ProgramLoadOperation::ObserveRealPath,
            Some(path.display().to_path_buf()),
            "physical-source-alias",
            format!(
                "physical identity is already owned by lexical source {}",
                self.sources[existing].prepared.path().display().display()
            ),
        )
    }
}

fn publish_program(
    staged: CompleteGraph,
    packages: Vec<PackageMetadata>,
    path_context: PathContext,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
) -> Result<PreparedProgram, ProgramLoadError> {
    let mut builder = PreparedProgram::builder(path_context, compiler_options.clone());
    builder.set_program_options(program_options);

    let mut published_ids = vec![None; staged.sources.len()];
    let mut source_by_canonical = BTreeMap::<CanonicalPath, (SourceFileId, ProgramPath)>::new();
    let publish_order = staged
        .library_postorder
        .into_iter()
        .chain(staged.ordinary_postorder);
    for source_index in publish_order {
        let staged_source = &staged.sources[source_index];
        let may_be_emitted =
            staged_source.prepared.may_be_emitted() && staged_source.has_non_external_reason;
        let prepared = staged_source
            .prepared
            .clone()
            .with_may_be_emitted(may_be_emitted);
        let source_id = builder.add_source_file(prepared.clone()).map_err(|error| {
            ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
        })?;
        if staged_source.class == SourceClass::Library {
            builder.add_library_file(source_id).map_err(|error| {
                ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
            })?;
        }
        published_ids[source_index] = Some(source_id);
        source_by_canonical.insert(
            prepared.path().canonical().clone(),
            (source_id, prepared.path().clone()),
        );
    }

    for root in staged.roots {
        let prepared_root = match root.source {
            Some(source) => PreparedRoot::loaded(
                root.path,
                published_ids[source].expect("postorder publishes every staged source"),
            ),
            None => PreparedRoot::missing(
                root.path,
                root.missing_diagnostic
                    .expect("missing roots retain their diagnostic"),
            ),
        };
        builder.add_root(prepared_root).map_err(|error| {
            ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
        })?;
    }

    for package in packages {
        builder.add_package_metadata(package).map_err(|error| {
            ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
        })?;
    }

    let package_map =
        package_map_from_facts(staged.module_resolutions.iter().filter_map(|resolution| {
            let ResolutionOutcome::Resolved(module) = resolution.host.outcome() else {
                return None;
            };
            Some((module.package_id()?, module.extension()))
        }));
    for resolution in staged.module_resolutions {
        let key_path = resolution.key.source().as_path().to_path_buf();
        let specifier = resolution.key.specifier().to_owned();
        let bound = bind_module_resolution(
            resolution.host,
            &source_by_canonical,
            &compiler_options,
            &package_map,
            resolution.loads_source,
        )
        .map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::BindResolutions,
                Some(key_path),
                Some(specifier),
                error,
            )
        })?;
        builder
            .add_module_resolution(resolution.key, Ok(bound))
            .map_err(|error| {
                ProgramLoadError::preparation(ProgramLoadOperation::BindResolutions, error)
            })?;
    }

    for resolution in staged.type_resolutions {
        let key_path = resolution
            .key
            .origin()
            .canonical_path()
            .as_path()
            .to_path_buf();
        let specifier = resolution.key.specifier().to_owned();
        let bound =
            bind_type_resolution(resolution.host, &source_by_canonical).map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::BindResolutions,
                    Some(key_path),
                    Some(specifier),
                    error,
                )
            })?;
        let bound = bound.with_diagnostics(resolution.diagnostics);
        builder
            .add_type_reference_resolution(resolution.key, Ok(bound))
            .map_err(|error| {
                ProgramLoadError::preparation(ProgramLoadOperation::BindResolutions, error)
            })?;
    }

    builder.set_diagnostics(PreparationDiagnostics::new(
        Vec::new(),
        Vec::new(),
        staged.program_diagnostics,
    ));
    builder.build().map_err(|error| {
        ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
    })
}

fn bind_module_resolution(
    host: HostModuleResolution,
    source_by_canonical: &BTreeMap<CanonicalPath, (SourceFileId, ProgramPath)>,
    options: &CompilerOptions,
    package_map: &BTreeMap<String, bool>,
    loads_source: bool,
) -> Result<ModuleResolution, ResolutionError> {
    let alternate_result = host.alternate_result().cloned();
    let ResolutionOutcome::Resolved(module) = host.into_outcome() else {
        let mut resolution = ModuleResolution::not_found();
        if let Some(alternate_result) = alternate_result {
            resolution = resolution.with_alternate_result(alternate_result);
        }
        return Ok(resolution);
    };
    let (types_package_exists, package_bundles_types) =
        module.package_id().map_or((false, false), |package_id| {
            (
                package_map.contains_key(&types_package_name(package_id.name())),
                package_map.get(package_id.name()).copied().unwrap_or(false),
            )
        });
    let owned_source = source_by_canonical.get(module.resolved_file().canonical());
    let target = if module.extension().is_javascript() && owned_source.is_none() {
        let reason = unloaded_javascript_reason(
            module.extension(),
            options,
            module.is_external_library_import(),
            module.original_path().is_some(),
            module.resolved_file().canonical(),
            loads_source,
        )
        .ok_or_else(|| {
            ResolutionError::unsupported(
                "unexplained-unloaded-javascript",
                format!(
                    "resolved JavaScript target {} has no source-membership exclusion",
                    module.resolved_file().display().display()
                ),
            )
        })?;
        if module.original_path().is_some()
            && !matches!(reason, UnloadedModuleReason::JsxWithoutJsxOption)
        {
            return Err(ResolutionError::unsupported(
                "unloaded-original-path",
                format!(
                    "unloaded JavaScript target {} has a lexical-to-physical transition",
                    module.resolved_file().display().display()
                ),
            ));
        }
        ResolvedModuleTarget::Unloaded {
            resolved_file: module.resolved_file().clone(),
            reason,
        }
    } else if is_arbitrary_declaration_extension(module.extension()) && owned_source.is_none() {
        ResolvedModuleTarget::Unloaded {
            resolved_file: module.resolved_file().clone(),
            reason: if loads_source {
                UnloadedModuleReason::ArbitraryExtensionWithoutOption
            } else {
                UnloadedModuleReason::ResolutionOnly
            },
        }
    } else if module.original_path().is_some() {
        return Err(ResolutionError::unsupported(
            "loaded-original-path",
            format!(
                "loaded source target {} has a lexical-to-physical transition",
                module.resolved_file().display().display()
            ),
        ));
    } else if let Some((source, path)) = owned_source {
        ResolvedModuleTarget::Source {
            source: *source,
            resolved_file: path.clone(),
        }
    } else {
        return Err(ResolutionError::unsupported(
            "resolution-only-source-target",
            format!(
                "resolved non-JavaScript target {} has no independent program membership",
                module.resolved_file().display().display()
            ),
        ));
    };
    let mut resolution = ModuleResolution::resolved(module.into_resolved_module(target)?)
        .with_types_package_exists(types_package_exists)
        .with_package_bundles_types(package_bundles_types);
    if let Some(alternate_result) = alternate_result {
        resolution = resolution.with_alternate_result(alternate_result);
    }
    Ok(resolution)
}

fn bind_type_resolution(
    host: ResolutionOutcome<HostResolvedTypeReferenceDirective>,
    source_by_canonical: &BTreeMap<CanonicalPath, (SourceFileId, ProgramPath)>,
) -> Result<TypeReferenceResolution, ResolutionError> {
    let ResolutionOutcome::Resolved(host) = host else {
        return Ok(TypeReferenceResolution::not_found());
    };
    let Some((source, path)) = source_by_canonical.get(host.resolved_file().canonical()) else {
        return Err(ResolutionError::invalid_data(format!(
            "resolved type-reference target {} is not owned by the prepared program",
            host.resolved_file().display().display()
        )));
    };
    Ok(TypeReferenceResolution::resolved(
        host.into_resolved_type_reference_directive(path.clone(), *source)?,
    ))
}

fn package_map_from_facts<'a>(
    facts: impl IntoIterator<Item = (&'a PackageId, &'a ModuleExtension)>,
) -> BTreeMap<String, bool> {
    let mut packages = BTreeMap::new();
    for (package_id, extension) in facts {
        let bundles_declaration = matches!(extension, ModuleExtension::Dts);
        packages
            .entry(package_id.name().to_owned())
            .and_modify(|existing| *existing |= bundles_declaration)
            .or_insert(bundles_declaration);
    }
    packages
}

fn types_package_name(package_name: &str) -> String {
    let mangled = match package_name.strip_prefix('@') {
        Some(scoped) => scoped.replace('/', "__"),
        None => package_name.to_owned(),
    };
    format!("@types/{mangled}")
}

fn implied_node_format(
    file_name: &str,
    package_scope: Option<&PackageMetadata>,
    options: &CompilerOptions,
) -> Option<ResolutionMode> {
    if file_name.ends_with(".d.mts") || file_name.ends_with(".mts") || file_name.ends_with(".mjs") {
        return Some(ResolutionMode::EsNext);
    }
    if file_name.ends_with(".d.cts") || file_name.ends_with(".cts") || file_name.ends_with(".cjs") {
        return Some(ResolutionMode::CommonJs);
    }
    if file_name.ends_with(".d.ts")
        || file_name.ends_with(".ts")
        || file_name.ends_with(".tsx")
        || file_name.ends_with(".js")
        || file_name.ends_with(".jsx")
    {
        let package_lookup = matches!(options.emit_module_resolution_kind(), 3..=99)
            || file_name
                .split('/')
                .any(|segment| segment == "node_modules");
        if !package_lookup {
            return None;
        }
        return Some(
            if package_scope.is_some_and(|scope| scope.module_type() == PackageJsonType::Module) {
                ResolutionMode::EsNext
            } else {
                ResolutionMode::CommonJs
            },
        );
    }
    None
}

fn implied_node_format_for_emit(
    file_name: &str,
    package_scope: Option<&PackageMetadata>,
    options: &CompilerOptions,
) -> Option<ResolutionMode> {
    let implied = implied_node_format(file_name, package_scope, options)?;
    if (100..=199).contains(&options.emit_module_kind())
        || [".mts", ".mjs", ".cts", ".cjs"]
            .iter()
            .any(|extension| file_name.ends_with(extension))
    {
        return Some(implied);
    }
    match package_scope.map(PackageMetadata::module_type) {
        Some(PackageJsonType::Module | PackageJsonType::CommonJs) => Some(implied),
        Some(PackageJsonType::Other | PackageJsonType::Unspecified) | None => None,
    }
}

fn is_typescript_source(path: &CanonicalPath) -> bool {
    path.as_path().to_str().is_some_and(|path| {
        TYPESCRIPT_SOURCE_EXTENSIONS
            .iter()
            .any(|extension| path.ends_with(extension))
    })
}

/// tsc-port: getSupportedExtensions/getSupportedExtensionsWithJsonIfResolveJsonModule @6.0.3
/// tsc-hash: 39020b78f2c3adb008f8559648a94f9773ed470050dea9d483a562bb66fe72cc
/// tsc-span: _tsc.js:18632-18651
fn is_admitted_source(path: &CanonicalPath, options: &CompilerOptions) -> bool {
    is_typescript_source(path)
        || options.allow_js && is_javascript_source(path)
        || options.resolve_json_module_effective() && is_json_source(path)
}

const fn supported_source_extension_list(allow_js: bool) -> &'static str {
    if allow_js {
        ALL_SOURCE_EXTENSION_LIST
    } else {
        TYPESCRIPT_SOURCE_EXTENSION_LIST
    }
}

fn is_json_source(path: &CanonicalPath) -> bool {
    path.as_path()
        .to_str()
        .is_some_and(|path| path.ends_with(".json"))
}

fn is_javascript_source(path: &CanonicalPath) -> bool {
    path.as_path().to_str().is_some_and(|path| {
        JAVASCRIPT_SOURCE_EXTENSIONS
            .iter()
            .any(|extension| path.ends_with(extension))
    })
}

fn path_contains_node_modules(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| path.split('/').any(|component| component == "node_modules"))
}

fn unloaded_javascript_reason(
    extension: &ModuleExtension,
    options: &CompilerOptions,
    external: bool,
    has_original_path: bool,
    resolved_file: &CanonicalPath,
    loads_source: bool,
) -> Option<UnloadedModuleReason> {
    if !extension.is_javascript() {
        return None;
    }
    if matches!(extension, ModuleExtension::Jsx) && options.jsx.unwrap_or(0) == 0 {
        return Some(UnloadedModuleReason::JsxWithoutJsxOption);
    }
    if !loads_source {
        return Some(UnloadedModuleReason::ResolutionOnly);
    }
    if external && (!has_original_path || path_contains_node_modules(resolved_file.as_path())) {
        return Some(UnloadedModuleReason::NodeModulesDepth);
    }
    (!options.allow_js).then_some(UnloadedModuleReason::JavaScriptNotAdmitted)
}

fn is_arbitrary_declaration_extension(extension: &ModuleExtension) -> bool {
    matches!(
        extension,
        ModuleExtension::Arbitrary(extension)
            if extension.starts_with(".d.") && extension.ends_with(".ts")
    )
}

fn is_loadable_declaration_extension(extension: &ModuleExtension) -> bool {
    matches!(
        extension,
        ModuleExtension::Dts | ModuleExtension::Dmts | ModuleExtension::Dcts
    ) || is_arbitrary_declaration_extension(extension)
}

fn is_loadable_typescript_extension(extension: &ModuleExtension) -> bool {
    matches!(
        extension,
        ModuleExtension::Ts | ModuleExtension::Tsx | ModuleExtension::Mts | ModuleExtension::Cts
    ) || is_loadable_declaration_extension(extension)
}

/// tsc-port: getSourceFileFromReferenceWorker @6.0.3
/// tsc-hash: 7812d8155c2ffdd584bf03bd3210c43fd1e2e5bdf13cfecfb66728cbdbcf8330
/// tsc-span: _tsc.js:124173-124209
fn unsupported_root_extension_diagnostic(
    path: &ProgramPath,
    allow_js: bool,
) -> Result<Diagnostic, ProgramLoadError> {
    let javascript = is_javascript_source(path.canonical());
    let path = path_text(path.display())?;
    let (message, arguments) = if javascript {
        (
            &gen::File_0_is_a_JavaScript_file_Did_you_mean_to_enable_the_allowJs_option,
            vec![path],
        )
    } else {
        (
            &gen::File_0_has_an_unsupported_extension_The_only_supported_extensions_are_1,
            vec![path, supported_source_extension_list(allow_js).to_owned()],
        )
    };
    let root_reason = MessageChain::new(&gen::Root_file_specified_for_compilation, &[]);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Ok(Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(message, &arguments).with_next(vec![inclusion]),
    ))
}

fn missing_root_diagnostic(path: &ProgramPath) -> Diagnostic {
    let root_reason = MessageChain::new(&gen::Root_file_specified_for_compilation, &[]);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::File_0_not_found,
            &[path
                .display()
                .to_str()
                .expect("program paths are representable")
                .to_owned()],
        )
        .with_next(vec![inclusion]),
    )
}

fn missing_library_root_diagnostic(path: &ProgramPath, reason: &LibraryRootReason) -> Diagnostic {
    let reason = match reason {
        LibraryRootReason::Default { target } => MessageChain::new(
            &gen::Default_library_for_target_0,
            std::slice::from_ref(target),
        ),
        LibraryRootReason::Explicit { file_name } => MessageChain::new(
            &gen::Library_0_specified_in_compilerOptions,
            std::slice::from_ref(file_name),
        ),
    };
    let inclusion =
        MessageChain::new(&gen::The_file_is_in_the_program_because, &[]).with_next(vec![reason]);
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::File_0_not_found,
            &[path
                .display()
                .to_str()
                .expect("program paths are representable")
                .to_owned()],
        )
        .with_next(vec![inclusion]),
    )
}

fn automatic_type_reference_diagnostic(name: &str, uses_wildcard: bool) -> Diagnostic {
    let reason = MessageChain::new(
        if uses_wildcard {
            &gen::Entry_point_for_implicit_type_library_0
        } else {
            &gen::Entry_point_of_type_library_0_specified_in_compilerOptions
        },
        &[name.to_owned()],
    );
    let inclusion =
        MessageChain::new(&gen::The_file_is_in_the_program_because, &[]).with_next(vec![reason]);
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Cannot_find_type_definition_file_for_0,
            &[name.to_owned()],
        )
        .with_next(vec![inclusion]),
    )
}

fn script_target_name(options: &CompilerOptions) -> &'static str {
    match options.emit_script_target().bits() {
        0 => "es3",
        1 => "es5",
        2 => "es2015",
        3 => "es2016",
        4 => "es2017",
        5 => "es2018",
        6 => "es2019",
        7 => "es2020",
        8 => "es2021",
        9 => "es2022",
        10 => "es2023",
        11 => "es2024",
        12 => "es2025",
        99 => "esnext",
        100 => "json",
        _ => "unknown",
    }
}

fn unresolved_type_reference_diagnostic(
    source: &PreparedSourceFile,
    directive: &PlannedTypeReferenceDirective,
) -> Result<Diagnostic, ProgramLoadError> {
    located_diagnostic(
        source,
        directive.pos(),
        directive.length(),
        &gen::Cannot_find_type_definition_file_for_0,
        &[directive.key().specifier().to_owned()],
    )
}

fn located_diagnostic(
    source: &PreparedSourceFile,
    start: u32,
    length: u32,
    message: &'static tsc_diagnostics::DiagnosticMessage,
    args: &[String],
) -> Result<Diagnostic, ProgramLoadError> {
    let file_name = source.path().display().to_str().ok_or_else(|| {
        ProgramLoadError::invalid_data(
            ProgramLoadOperation::BuildPreparedProgram,
            Some(source.path().display().to_path_buf()),
            "diagnostic source path is not valid Unicode",
        )
    })?;
    Ok(Diagnostic::new(
        Some(file_name.to_owned()),
        Some(start),
        Some(length),
        MessageChain::new(message, args),
    ))
}

fn path_text(path: &Path) -> Result<String, ProgramLoadError> {
    path.to_str().map(str::to_owned).ok_or_else(|| {
        ProgramLoadError::invalid_data(
            ProgramLoadOperation::BuildPreparedProgram,
            Some(path.to_path_buf()),
            "program path is not valid Unicode",
        )
    })
}
