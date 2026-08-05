use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_diagnostics::{gen, Diagnostic, MessageChain, RelatedInfo};
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
    extensionless_source_probe_extensions, PackageJsonType, PackageMetadata, PathContext,
    PreparationDiagnostics, PreparedAuxiliaryFile, PreparedProgram, PreparedRoot,
    PreparedSourceFile, ProgramConfigFile, ProgramOptions, SourceFileId,
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

    pub(crate) fn unsupported(
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
        None,
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
        None,
    )
}

/// Root inclusion provenance used by config-backed program construction.
/// TypeScript exposes this in the TS6053/unsupported-root diagnostic chain;
/// preserving it here keeps the loader independent of the config parser while
/// allowing `files` roots to differ from explicit command-line roots.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RootFileReason {
    Explicit,
    FilesList {
        spec: Arc<str>,
    },
    IncludePattern {
        spec: Arc<str>,
        config_file: Arc<str>,
    },
    DefaultInclude,
}

/// Load a config-derived root closure while retaining the source of each root
/// spelling for TypeScript's inclusion-chain diagnostics.
pub(crate) fn load_program_with_root_reasons(
    host: &dyn CompilerHost,
    roots: &[(PathBuf, RootFileReason)],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    let root_names = roots
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
    let root_reasons = roots
        .iter()
        .map(|(_, reason)| reason.clone())
        .collect::<Vec<_>>();
    load_program_worker(
        host,
        &root_names,
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
        Some(&root_reasons),
    )
}

#[allow(clippy::too_many_arguments)] // Root provenance is an orthogonal config-only input.
fn load_program_worker(
    host: &dyn CompilerHost,
    root_names: &[PathBuf],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: Option<&LibraryCatalog>,
    require_no_lib: bool,
    limits: ProgramLoadLimits,
    root_reasons: Option<&[RootFileReason]>,
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
    for (index, root_name) in root_names.iter().enumerate() {
        let root_spelling = root_name.clone();
        let root = normalize_root(root_name, &path_context)?;
        let reason = root_reasons
            .and_then(|reasons| reasons.get(index).cloned())
            .unwrap_or(RootFileReason::Explicit);
        graph.load_root(root, &root_spelling, reason)?;
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
    if compiler_options.no_emit != Some(true) {
        return Err(reject_input(
            "compilerOptions.noEmit must be explicitly true",
        ));
    }
    if require_no_lib && program_options.no_lib() != Some(true) {
        return Err(reject_input("programOptions.noLib must be explicitly true"));
    }
    // `noLib` suppresses library loading even when an explicit `lib` list is
    // present.  TypeScript reports TS5053 for that combination from
    // `getOptionsDiagnostics`; the config/CLI driver owns that diagnostic
    // gate, while the lower-level program loader must still mirror
    // createProgram's source graph (which simply skips the library phase).
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
        if compiler_options.lib.is_none()
            && program_options
                .default_library_file_name()
                .is_some_and(|value| !catalog.contains_file_name(value))
        {
            return Err(ProgramLoadError::invalid_input(
                ProgramLoadOperation::ValidateOptions,
                None,
                format!(
                    "programOptions.defaultLibraryFileName contains unknown catalog file {:?}",
                    program_options
                        .default_library_file_name()
                        .expect("checked host default library")
                ),
            ));
        }
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
    let trailing_separator = root
        .to_str()
        .is_some_and(|text| text.ends_with(['/', '\\']));
    let mut normalized =
        normalize_absolute_path(root, Some(current_directory)).map_err(|error| {
            ProgramLoadError::resolution(
                ProgramLoadOperation::NormalizeRoot,
                Some(root.to_path_buf()),
                None,
                error,
            )
        })?;
    if trailing_separator && !normalized.ends_with('/') {
        normalized.push('/');
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

fn path_has_extension(path: &Path) -> bool {
    path.to_str()
        .expect("program paths are representable")
        .rsplit(['/', '\\'])
        .next()
        .is_some_and(|base_name| base_name.contains('.'))
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceInclusionReason {
    Root(RootFileReason),
    Import {
        parent: PathBuf,
        specifier: String,
        pos: u32,
        end: u32,
    },
    PathReference {
        parent: PathBuf,
        specifier: String,
        pos: u32,
        end: u32,
    },
    TypeReference {
        parent: PathBuf,
        specifier: String,
        pos: u32,
        end: u32,
    },
    AutomaticType {
        name: String,
    },
    Synthetic,
    Library,
}

impl SourceInclusionReason {
    const fn is_referenced(&self) -> bool {
        !matches!(self, Self::Root(_))
    }
}

#[derive(Clone, Debug)]
struct DiscoveryReason {
    seeds_non_external_reachability: bool,
    inclusion: SourceInclusionReason,
}

impl DiscoveryReason {
    fn root(reason: RootFileReason) -> Self {
        Self {
            seeds_non_external_reachability: true,
            inclusion: SourceInclusionReason::Root(reason),
        }
    }

    fn dependency(inclusion: SourceInclusionReason) -> Self {
        Self {
            seeds_non_external_reachability: false,
            inclusion,
        }
    }

    fn automatic_type(is_external_library_import: bool, name: String) -> Self {
        Self {
            seeds_non_external_reachability: !is_external_library_import,
            inclusion: SourceInclusionReason::AutomaticType { name },
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
    /// Root-file inclusion occurrences are retained separately from the
    /// canonical source identity.  They are observable in the TS1149
    /// program-preprocessing message chain when two root spellings collapse
    /// on a case-insensitive host.
    root_inclusions: Vec<PathBuf>,
    inclusion_reasons: Vec<SourceInclusionReason>,
    alternate_inclusion_reasons: Vec<(PathBuf, SourceInclusionReason)>,
    has_non_external_reason: bool,
    class: SourceClass,
    path_references: Vec<PlannedPathReference>,
    type_reference_directives: Vec<PlannedTypeReferenceDirective>,
    lib_reference_directives: Vec<PlannedLibReferenceDirective>,
    module_requests: Vec<(ResolutionKey, bool)>,
    module_request_spans: BTreeMap<ResolutionKey, (u32, u32)>,
    found_searching_node_modules: bool,
    modules_with_elided_imports: bool,
    processing_references: bool,
    pending_reprocesses: VecDeque<SourceReprocess>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceReprocessKind {
    AllReferences,
    ImportedModules,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceReprocess {
    kind: SourceReprocessKind,
    source_depth: usize,
    node_modules_depth: usize,
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
    unloaded_reason: Option<UnloadedModuleReason>,
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
    sources: Vec<StagedSource>,
    source_edges: Vec<Vec<(usize, bool)>>,
    postorder: Vec<usize>,
    roots: Vec<StagedRoot>,
    module_resolution_by_key: BTreeMap<ResolutionKey, usize>,
    module_resolutions: Vec<StagedModuleResolution>,
    type_resolution_by_key: BTreeMap<TypeReferenceResolutionKey, usize>,
    type_resolutions: Vec<StagedTypeResolution>,
    diagnosed_missing_roots: BTreeSet<String>,
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
            sources: Vec::new(),
            source_edges: Vec::new(),
            postorder: Vec::new(),
            roots: Vec::new(),
            module_resolution_by_key: BTreeMap::new(),
            module_resolutions: Vec::new(),
            type_resolution_by_key: BTreeMap::new(),
            type_resolutions: Vec::new(),
            diagnosed_missing_roots: BTreeSet::new(),
            diagnosed_missing_library_roots: BTreeSet::new(),
            program_diagnostics: Vec::new(),
            request_edges: 0,
            total_source_bytes: 0,
        }
    }

    fn load_root(
        &mut self,
        path: ProgramPath,
        root_spelling: &Path,
        root_reason: RootFileReason,
    ) -> Result<(), ProgramLoadError> {
        if !path_has_extension(path.display()) {
            return self.load_extensionless_root(path, root_spelling, root_reason);
        }
        if !is_admitted_source(path.canonical(), self.compiler_options) {
            let diagnostic = unsupported_root_extension_diagnostic(
                &path,
                root_spelling,
                self.compiler_options.allow_js,
                root_reason,
            )?;
            if self
                .diagnosed_missing_roots
                .insert(path_text(path.display())?)
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
            0,
            DiscoveryReason::root(root_reason.clone()),
            SourceClass::Ordinary,
        )?;
        if let Some(source) = source {
            self.sources[source]
                .root_inclusions
                .push(path.display().to_path_buf());
        }
        let missing_diagnostic = source
            .is_none()
            .then(|| missing_root_diagnostic(root_spelling, root_reason));
        if let Some(diagnostic) = missing_diagnostic.clone() {
            if self
                .diagnosed_missing_roots
                .insert(path_text(path.display())?)
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

    fn load_extensionless_root(
        &mut self,
        path: ProgramPath,
        root_spelling: &Path,
        root_reason: RootFileReason,
    ) -> Result<(), ProgramLoadError> {
        let requested_text = path
            .display()
            .to_str()
            .expect("program paths are representable");
        for &extension in extensionless_source_probe_extensions(self.compiler_options.allow_js) {
            let candidate_text = format!("{requested_text}{extension}");
            let candidate = make_program_path(
                &candidate_text,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::NormalizeRoot,
                    Some(path.display().to_path_buf()),
                    None,
                    error,
                )
            })?;
            if let Some(source) = self.visit_source(
                candidate,
                0,
                0,
                DiscoveryReason::root(root_reason.clone()),
                SourceClass::Ordinary,
            )? {
                self.sources[source]
                    .root_inclusions
                    .push(path.display().to_path_buf());
                self.roots.push(StagedRoot {
                    path,
                    source: Some(source),
                    missing_diagnostic: None,
                });
                return Ok(());
            }
        }

        let diagnostic = unresolved_extensionless_root_diagnostic(
            root_spelling,
            self.compiler_options.allow_js,
            root_reason,
        )?;
        if self
            .diagnosed_missing_roots
            .insert(path_text(path.display())?)
        {
            self.program_diagnostics.push(diagnostic.clone());
        }
        self.roots.push(StagedRoot {
            path,
            source: None,
            missing_diagnostic: Some(diagnostic),
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
                    .push(automatic_type_reference_diagnostic(
                        &name,
                        uses_wildcard,
                        self.program_options.config_file(),
                    ));
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
                    usize::from(external),
                    DiscoveryReason::automatic_type(external, name.clone()),
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
                let file_name = self
                    .program_options
                    .default_library_file_name()
                    .unwrap_or_else(|| catalog.default_file_name(self.compiler_options));
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
                    0,
                    DiscoveryReason::dependency(SourceInclusionReason::Library),
                    SourceClass::Library,
                )?
                .is_none()
                && self
                    .diagnosed_missing_library_roots
                    .insert(path.display().to_path_buf())
            {
                let diagnostic = missing_library_root_diagnostic(
                    &path,
                    &reason,
                    self.program_options.config_file(),
                );
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
        if self
            .compiler_options
            .force_consistent_casing_in_file_names_effective()
        {
            let casing_diagnostics = self
                .sources
                .iter()
                .flat_map(|source| {
                    source
                        .alternate_inclusion_reasons
                        .iter()
                        .map(|(alias, reason)| {
                            casing_alias_diagnostic(
                                &source.prepared,
                                alias,
                                &source.inclusion_reasons,
                                reason,
                                self.program_options.config_file(),
                            )
                        })
                })
                .collect::<Vec<_>>();
            self.program_diagnostics.extend(casing_diagnostics);
        }
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
        node_modules_depth: usize,
        reason: DiscoveryReason,
        class: SourceClass,
    ) -> Result<Option<usize>, ProgramLoadError> {
        if let Some(state) = self.states.get(path.canonical()).copied() {
            if let Some(source) = state.source() {
                let first_path = self.sources[source].prepared.path();
                if first_path.display() != path.display()
                    && !self.normalized_display_paths_are_equal(first_path, &path)?
                {
                    self.sources[source]
                        .prepared
                        .remember_display_alias(path.display());
                    self.sources[source]
                        .alternate_inclusion_reasons
                        .push((path.display().to_path_buf(), reason.inclusion.clone()));
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
                let reprocess = {
                    let staged = &mut self.sources[source];
                    staged.inclusion_reasons.push(reason.inclusion.clone());
                    staged.has_non_external_reason |= reason.seeds_non_external_reachability;
                    if staged.found_searching_node_modules && node_modules_depth == 0 {
                        // tsc clears both latches before recursively processing
                        // the source again. A cycle can therefore observe the
                        // promoted state without scheduling duplicate work.
                        staged.found_searching_node_modules = false;
                        staged.modules_with_elided_imports = false;
                        Some(SourceReprocess {
                            kind: SourceReprocessKind::AllReferences,
                            source_depth: depth,
                            node_modules_depth,
                        })
                    } else if staged.modules_with_elided_imports
                        && self
                            .compiler_options
                            .node_modules_depth_below_limit(node_modules_depth)
                    {
                        staged.modules_with_elided_imports = false;
                        Some(SourceReprocess {
                            kind: SourceReprocessKind::ImportedModules,
                            source_depth: depth,
                            node_modules_depth,
                        })
                    } else {
                        None
                    }
                };
                if let Some(reprocess) = reprocess {
                    self.schedule_source_reprocess(source, reprocess)?;
                }
            }
            return Ok(state.source());
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
        let path_references = plan
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.path_references().to_vec());
        let type_reference_directives = plan
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.type_reference_directives().to_vec());
        let lib_reference_directives = plan
            .as_ref()
            .map_or_else(Vec::new, |plan| plan.lib_reference_directives().to_vec());
        let module_requests = plan.as_ref().map_or_else(Vec::new, |plan| {
            plan.module_requests_with_loadability()
                .map(|(key, loads_source)| (key.clone(), loads_source))
                .collect::<Vec<_>>()
        });
        let module_request_spans = plan.as_ref().map_or_else(BTreeMap::new, |plan| {
            plan.module_requests()
                .iter()
                .filter_map(|key| {
                    plan.module_request_span(key)
                        .map(|span| (key.clone(), span))
                })
                .collect::<BTreeMap<_, _>>()
        });
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
        self.sources.push(StagedSource {
            prepared,
            root_inclusions: Vec::new(),
            inclusion_reasons: vec![reason.inclusion.clone()],
            alternate_inclusion_reasons: Vec::new(),
            has_non_external_reason: reason.seeds_non_external_reachability,
            class,
            path_references,
            type_reference_directives,
            lib_reference_directives,
            module_requests,
            module_request_spans,
            found_searching_node_modules: node_modules_depth > 0,
            modules_with_elided_imports: false,
            processing_references: false,
            pending_reprocesses: VecDeque::new(),
        });
        self.source_edges.push(Vec::new());
        self.states
            .insert(path.canonical().clone(), VisitState::Visiting(source));

        if self.sources[source].class == SourceClass::Library
            && !self.sources[source].path_references.is_empty()
        {
            return Err(ProgramLoadError::unsupported(
                ProgramLoadOperation::PlanSourceRequests,
                Some(path.display().to_path_buf()),
                "default-library-path-references",
                "default-library path-reference descendants have processing-prefix order without checker-visible library membership, which the current PreparedProgram prefix cannot represent",
            ));
        }
        self.schedule_source_reprocess(
            source,
            SourceReprocess {
                kind: SourceReprocessKind::AllReferences,
                source_depth: depth,
                node_modules_depth,
            },
        )?;

        self.states
            .insert(path.canonical().clone(), VisitState::Complete(source));
        self.postorder.push(source);
        Ok(Some(source))
    }

    fn schedule_source_reprocess(
        &mut self,
        source: usize,
        reprocess: SourceReprocess,
    ) -> Result<(), ProgramLoadError> {
        self.sources[source]
            .pending_reprocesses
            .push_back(reprocess);
        self.drain_source_reprocesses(source)
    }

    fn drain_source_reprocesses(&mut self, source: usize) -> Result<(), ProgramLoadError> {
        if self.sources[source].processing_references {
            return Ok(());
        }
        self.sources[source].processing_references = true;
        let result = (|| {
            while let Some(reprocess) = self.sources[source].pending_reprocesses.pop_front() {
                match reprocess.kind {
                    SourceReprocessKind::AllReferences => self.process_all_source_references(
                        source,
                        reprocess.source_depth,
                        reprocess.node_modules_depth,
                    )?,
                    SourceReprocessKind::ImportedModules => self.process_source_module_requests(
                        source,
                        reprocess.source_depth,
                        reprocess.node_modules_depth,
                    )?,
                }
            }
            Ok(())
        })();
        self.sources[source].processing_references = false;
        result
    }

    fn process_all_source_references(
        &mut self,
        source: usize,
        depth: usize,
        node_modules_depth: usize,
    ) -> Result<(), ProgramLoadError> {
        let path_references = self.sources[source].path_references.clone();
        let type_reference_directives = self.sources[source].type_reference_directives.clone();
        let lib_reference_directives = self.sources[source].lib_reference_directives.clone();

        // `noResolve` only suppresses path/type-reference source discovery.
        // Module requests still go through the resolver below so their
        // authoritative resolution facts and diagnostics remain available.
        if self.compiler_options.no_resolve != Some(true) {
            for reference in path_references {
                self.process_path_reference(source, &reference, depth, node_modules_depth)?;
            }
            self.process_type_references(
                source,
                type_reference_directives,
                depth,
                node_modules_depth,
            )?;
        }
        if self.program_options.no_lib() != Some(true) {
            self.process_lib_references(
                source,
                lib_reference_directives,
                depth,
                node_modules_depth,
            )?;
        }
        // `noLib=true` deliberately performs no host operation for lib
        // directives, although their occurrences were counted above.
        self.process_source_module_requests(source, depth, node_modules_depth)
    }

    fn process_source_module_requests(
        &mut self,
        source: usize,
        depth: usize,
        node_modules_depth: usize,
    ) -> Result<(), ProgramLoadError> {
        // tsc clears this latch before every explicit reprocessing attempt;
        // an over-depth request encountered below sets it again.
        self.sources[source].modules_with_elided_imports = false;
        let requests = self.sources[source].module_requests.clone();
        self.process_module_requests(source, requests, depth, node_modules_depth)
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
        node_modules_depth: usize,
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
                node_modules_depth,
                DiscoveryReason::dependency(SourceInclusionReason::Library),
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

    /// `findSourceFileWorker` keys every selected display spelling through
    /// `toPath`. This admits separator/dot-segment aliases created by an
    /// unvalidated `moduleSuffixes` entry while retaining the existing typed
    /// boundary for unresolved case-only aliases.
    ///
    /// tsc-port: findSourceFileWorker @6.0.3
    /// tsc-hash: faea3c8c14640ae05ef40c40bd6f0126bf9d59ed7af080a38d14019b93912e1e
    /// tsc-span: _tsc.js:124274-124277
    fn normalized_display_paths_are_equal(
        &self,
        left: &ProgramPath,
        right: &ProgramPath,
    ) -> Result<bool, ProgramLoadError> {
        let current_directory = self
            .resolver
            .path_context()
            .current_directory()
            .display()
            .to_str()
            .expect("program current directory is Unicode");
        let normalize = |path: &ProgramPath| {
            normalize_absolute_path(path.display(), Some(current_directory)).map_err(|error| {
                ProgramLoadError::resolution(
                    ProgramLoadOperation::ReadSource,
                    Some(path.display().to_path_buf()),
                    None,
                    error,
                )
            })
        };
        Ok(normalize(left)? == normalize(right)?)
    }

    /// Empty reference text is intentional: `combinePaths(basePath, "")`
    /// selects the containing directory, after which the ordinary
    /// extensionless source probe and TS6231 diagnostic apply.
    ///
    /// tsc-port: resolveTripleslashReference @6.0.3
    /// tsc-hash: c265a32a7d63be44dc5f33017bd2a5e51263f267c3222e20a37afdd59f649bfc
    /// tsc-span: _tsc.js:121904-121908
    /// tsc-port: processReferencedFiles @6.0.3
    /// tsc-hash: 921ee36a44bea86b4495ac4d7f7046aa22d889a2f712097a273a8fc77cecf386
    /// tsc-span: _tsc.js:124459-124468
    fn process_path_reference(
        &mut self,
        source: usize,
        reference: &PlannedPathReference,
        depth: usize,
        node_modules_depth: usize,
    ) -> Result<(), ProgramLoadError> {
        let source_path = self.sources[source].prepared.path().clone();
        let source_text = source_path
            .display()
            .to_str()
            .expect("program paths are representable");
        let base = directory_name(source_text);
        let normalized = if reference.file_name().is_empty() {
            base.clone()
        } else {
            normalize_absolute_path(Path::new(reference.file_name()), Some(&base)).map_err(
                |error| {
                    ProgramLoadError::resolution(
                        ProgramLoadOperation::NormalizeReference,
                        Some(source_path.display().to_path_buf()),
                        Some(reference.file_name().to_owned()),
                        error,
                    )
                },
            )?
        };
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
                node_modules_depth,
                DiscoveryReason::dependency(SourceInclusionReason::PathReference {
                    parent: source_path.display().to_path_buf(),
                    specifier: reference.file_name().to_owned(),
                    pos: reference.pos(),
                    end: reference.end(),
                }),
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

        for &extension in extensionless_source_probe_extensions(self.compiler_options.allow_js) {
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
                node_modules_depth,
                DiscoveryReason::dependency(SourceInclusionReason::PathReference {
                    parent: source_path.display().to_path_buf(),
                    specifier: reference.file_name().to_owned(),
                    pos: reference.pos(),
                    end: reference.end(),
                }),
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
        node_modules_depth: usize,
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
            let type_key = self.type_resolutions[index].key.clone();
            let directive = directives
                .iter()
                .find(|directive| directive.key() == &type_key);
            let type_inclusion = SourceInclusionReason::TypeReference {
                parent: containing_source.display().to_path_buf(),
                specifier: type_key.specifier().to_owned(),
                pos: directive.map_or(0, PlannedTypeReferenceDirective::pos),
                end: directive.map_or(0, PlannedTypeReferenceDirective::end),
            };
            let loaded = self.visit_source(
                target.clone(),
                depth.saturating_add(1),
                node_modules_depth.saturating_add(usize::from(external)),
                DiscoveryReason::dependency(type_inclusion),
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
        node_modules_depth: usize,
    ) -> Result<(), ProgramLoadError> {
        let mut phase_indices = Vec::with_capacity(requests.len());
        let containing_file = self.sources[source].prepared.path().display().to_path_buf();
        let containing_file_is_declaration = containing_file
            .to_str()
            .is_some_and(is_declaration_file_name);
        for (key, loads_source) in requests {
            let inclusion = self.sources[source]
                .module_request_spans
                .get(&key)
                .map(|(pos, end)| SourceInclusionReason::Import {
                    parent: containing_file.clone(),
                    specifier: key.specifier().to_owned(),
                    pos: *pos,
                    end: *end,
                })
                .unwrap_or(SourceInclusionReason::Synthetic);
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
                let index = self.module_resolutions.len();
                self.module_resolutions.push(StagedModuleResolution {
                    key: key.clone(),
                    host,
                    loads_source,
                    unloaded_reason: None,
                });
                self.module_resolution_by_key.insert(key, index);
                index
            };
            phase_indices.push((index, inclusion));
        }

        // As with type directives, all requests in this source are resolved
        // before the first successful target starts its DFS.
        for (index, inclusion) in phase_indices {
            let loads_source = self.module_resolutions[index].loads_source;
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
            let child_node_modules_depth = node_modules_depth.saturating_add(usize::from(external));
            if extension.is_javascript() {
                // tsc records the reprocessing latch from depth elision before
                // checking whether this occurrence can add a source. That is
                // observable for JSX errors, augmentation-only resolutions,
                // and allowJs=false as well as ordinary imports.
                let elided_by_node_modules_depth = external
                    && (!has_original_path
                        || path_contains_node_modules(target.canonical().as_path()))
                    && self
                        .compiler_options
                        .node_modules_depth_exceeds_limit(child_node_modules_depth);
                if elided_by_node_modules_depth {
                    self.sources[source].modules_with_elided_imports = true;
                }
                let reason = unloaded_javascript_reason(
                    &extension,
                    self.compiler_options,
                    external,
                    has_original_path,
                    target.canonical(),
                    loads_source,
                    child_node_modules_depth,
                );
                self.module_resolutions[index].unloaded_reason = reason;
                if reason.is_some() {
                    continue;
                }
            }
            if self.compiler_options.no_resolve == Some(true) {
                self.module_resolutions[index].unloaded_reason =
                    Some(UnloadedModuleReason::NoResolve);
                continue;
            }
            if !loads_source {
                continue;
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
            // Resolver-owned external symlink handling has already replaced
            // this target with its physical resolvedFileName. Visit that
            // identity directly; the lexical spelling remains on the host
            // resolution and is published as originalPath below.
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
                    child_node_modules_depth,
                    DiscoveryReason::dependency(inclusion.clone()),
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
                child_node_modules_depth,
                DiscoveryReason::dependency(inclusion),
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
}

fn publish_program(
    staged: CompleteGraph,
    packages: Vec<PackageMetadata>,
    path_context: PathContext,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
) -> Result<PreparedProgram, ProgramLoadError> {
    let mut builder = PreparedProgram::builder(path_context, compiler_options.clone());
    let config_file = program_options.config_file().cloned();
    builder.set_program_options(program_options);

    if let Some(config_file) = config_file {
        builder
            .add_auxiliary_file(PreparedAuxiliaryFile::new(
                config_file.path().clone(),
                config_file.text().to_owned(),
            ))
            .map_err(|error| {
                ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
            })?;
    }

    let mut published_ids = vec![None; staged.sources.len()];
    let mut source_by_canonical = BTreeMap::<CanonicalPath, SourceFileId>::new();
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
        source_by_canonical.insert(prepared.path().canonical().clone(), source_id);
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
            &package_map,
            resolution.loads_source,
            resolution.unloaded_reason,
            compiler_options.no_resolve == Some(true),
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
    source_by_canonical: &BTreeMap<CanonicalPath, SourceFileId>,
    package_map: &BTreeMap<String, bool>,
    loads_source: bool,
    unloaded_reason: Option<UnloadedModuleReason>,
    no_resolve: bool,
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
    let target = if no_resolve && owned_source.is_none() {
        ResolvedModuleTarget::Unloaded {
            resolved_file: module.resolved_file().clone(),
            reason: unloaded_reason.unwrap_or(UnloadedModuleReason::NoResolve),
        }
    } else if module.extension().is_javascript() && owned_source.is_none() {
        let reason = unloaded_reason.ok_or_else(|| {
            ResolutionError::unsupported(
                "unexplained-unloaded-javascript",
                format!(
                    "resolved JavaScript target {} has no source-membership exclusion",
                    module.resolved_file().display().display()
                ),
            )
        })?;
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
    } else if let Some(source) = owned_source {
        ResolvedModuleTarget::Source {
            source: *source,
            resolved_file: module.resolved_file().clone(),
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
    source_by_canonical: &BTreeMap<CanonicalPath, SourceFileId>,
) -> Result<TypeReferenceResolution, ResolutionError> {
    let ResolutionOutcome::Resolved(host) = host else {
        return Ok(TypeReferenceResolution::not_found());
    };
    let Some(source) = source_by_canonical.get(host.resolved_file().canonical()) else {
        return Err(ResolutionError::invalid_data(format!(
            "resolved type-reference target {} is not owned by the prepared program",
            host.resolved_file().display().display()
        )));
    };
    let target = host.resolved_file().clone();
    Ok(TypeReferenceResolution::resolved(
        host.into_resolved_type_reference_directive(target, *source)?,
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
    node_modules_depth: usize,
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
    if external
        && (!has_original_path || path_contains_node_modules(resolved_file.as_path()))
        && options.node_modules_depth_exceeds_limit(node_modules_depth)
    {
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
    root_spelling: &Path,
    allow_js: bool,
    root_reason: RootFileReason,
) -> Result<Diagnostic, ProgramLoadError> {
    let javascript = is_javascript_source(path.canonical());
    let path = root_spelling.to_str().ok_or_else(|| {
        ProgramLoadError::invalid_input(
            ProgramLoadOperation::NormalizeRoot,
            Some(root_spelling.to_path_buf()),
            "root spelling is not valid Unicode",
        )
    })?;
    let path = path.to_owned();
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
    let root_reason = root_file_reason_message(&root_reason);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Ok(Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(message, &arguments).with_next(vec![inclusion]),
    ))
}

fn missing_root_diagnostic(path: &Path, root_file_reason: RootFileReason) -> Diagnostic {
    let root_reason = root_file_reason_message(&root_file_reason);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::File_0_not_found,
            &[path
                .to_str()
                .expect("root paths are representable")
                .to_owned()],
        )
        .with_next(vec![inclusion]),
    )
}

fn unresolved_extensionless_root_diagnostic(
    path: &Path,
    allow_js: bool,
    root_reason: RootFileReason,
) -> Result<Diagnostic, ProgramLoadError> {
    let root_reason = root_file_reason_message(&root_reason);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Ok(Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Could_not_resolve_the_path_0_with_the_extensions_1,
            &[
                path.to_str()
                    .ok_or_else(|| {
                        ProgramLoadError::invalid_input(
                            ProgramLoadOperation::NormalizeRoot,
                            Some(path.to_path_buf()),
                            "root spelling is not valid Unicode",
                        )
                    })?
                    .to_owned(),
                supported_source_extension_list(allow_js).to_owned(),
            ],
        )
        .with_next(vec![inclusion]),
    ))
}

/// tsc-port: fileIncludeReasonToDiagnostics @6.0.3 (RootFile)
/// tsc-hash: 30e07b28f72a81d3eb29d0ab7e49d8d2a65a20dedc61205c00e488973787233a
/// tsc-span: _tsc.js:129341-129369
fn root_file_reason_message(reason: &RootFileReason) -> MessageChain {
    match reason {
        RootFileReason::Explicit => {
            MessageChain::new(&gen::Root_file_specified_for_compilation, &[])
        }
        RootFileReason::FilesList { .. } => {
            MessageChain::new(&gen::Part_of_files_list_in_tsconfig_json, &[])
        }
        RootFileReason::IncludePattern { spec, config_file } => MessageChain::new(
            &gen::Matched_by_include_pattern_0_in_1,
            &[spec.to_string(), config_file.to_string()],
        ),
        RootFileReason::DefaultInclude => {
            MessageChain::new(&gen::Matched_by_default_include_pattern, &[])
        }
    }
}

fn missing_library_root_diagnostic(
    path: &ProgramPath,
    reason: &LibraryRootReason,
    config_file: Option<&ProgramConfigFile>,
) -> Diagnostic {
    let inclusion_reason = match reason {
        LibraryRootReason::Default { target } => MessageChain::new(
            &gen::Default_library_for_target_0,
            std::slice::from_ref(target),
        ),
        LibraryRootReason::Explicit { file_name } => MessageChain::new(
            &gen::Library_0_specified_in_compilerOptions,
            std::slice::from_ref(file_name),
        ),
    };
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![inclusion_reason]);
    let mut diagnostic = Diagnostic::new(
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
    );
    if let LibraryRootReason::Default { target } = reason {
        if let Some((config_file, location)) = config_file.and_then(|config_file| {
            config_file
                .compiler_option_string_location("target", target)
                .map(|location| (config_file, location))
        }) {
            diagnostic.related_information_present = true;
            diagnostic.related.push(RelatedInfo {
                file_name: Some(
                    config_file
                        .path()
                        .display()
                        .to_str()
                        .expect("validated config paths are Unicode")
                        .to_owned(),
                ),
                start: Some(location.start()),
                length: Some(location.length()),
                message: MessageChain::new(
                    &gen::File_is_default_library_for_target_specified_here,
                    &[],
                ),
            });
        }
    }
    diagnostic
}

fn automatic_type_reference_diagnostic(
    name: &str,
    uses_wildcard: bool,
    config_file: Option<&ProgramConfigFile>,
) -> Diagnostic {
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
    let mut diagnostic = Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new(
            &gen::Cannot_find_type_definition_file_for_0,
            &[name.to_owned()],
        )
        .with_next(vec![inclusion]),
    );
    let syntax_name = if uses_wildcard { "*" } else { name };
    if let Some((config_file, location)) = config_file.and_then(|config_file| {
        config_file
            .automatic_type_directive_location(syntax_name)
            .map(|location| (config_file, location))
    }) {
        diagnostic.related_information_present = true;
        diagnostic.related.push(RelatedInfo {
            file_name: Some(
                config_file
                    .path()
                    .display()
                    .to_str()
                    .expect("validated config paths are Unicode")
                    .to_owned(),
            ),
            start: Some(location.start()),
            length: Some(location.length()),
            message: MessageChain::new(
                &gen::File_is_entry_point_of_type_library_specified_here,
                &[],
            ),
        });
    }
    diagnostic
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

/// Reproduce tsc's file-preprocessing casing diagnostic while retaining the
/// first discovered source as the canonical program identity. TypeScript
/// chooses TS1261 when a root spelling arrives after a referenced spelling;
/// otherwise it uses TS1149. Referenced reasons also own the source span used
/// by the renderer (the import/reference literal), while root-only collisions
/// remain compiler diagnostics with no source span. Config-backed `files`
/// roots retain TS1410 related information at the matching root literal.
///
/// tsc-port: fileIncludeReasonToRelatedInformation @6.0.3 (RootFile)
/// tsc-hash: 2a9e2f89989b2c92cc283fc2abc093c67973e2ddfd81145bab64b9c98004eab7
/// tsc-span: _tsc.js:125971-125985
fn casing_alias_diagnostic(
    existing: &PreparedSourceFile,
    incoming: &Path,
    existing_reasons: &[SourceInclusionReason],
    incoming_reason: &SourceInclusionReason,
    config_file: Option<&ProgramConfigFile>,
) -> Diagnostic {
    let existing_name = existing
        .path()
        .display()
        .to_str()
        .expect("validated program paths are Unicode");
    let incoming_name = incoming
        .to_str()
        .expect("alternate display aliases come from validated program paths");
    let existing_has_reference = existing_reasons
        .iter()
        .any(SourceInclusionReason::is_referenced);
    let root_arrived_after_reference = !incoming_reason.is_referenced() && existing_has_reference;
    let (message, arguments) = if root_arrived_after_reference {
        (
            &gen::Already_included_file_name_0_differs_from_file_name_1_only_in_casing,
            vec![existing_name.to_owned(), incoming_name.to_owned()],
        )
    } else {
        (
            &gen::File_name_0_differs_from_already_included_file_name_1_only_in_casing,
            vec![incoming_name.to_owned(), existing_name.to_owned()],
        )
    };
    // `visit_source` records the incoming occurrence before `finish` builds
    // this diagnostic, so `existing_reasons` already contains the alias
    // reason.  Appending it again would duplicate root/file-list entries.
    let mut reasons = existing_reasons
        .iter()
        .filter_map(source_inclusion_reason_message)
        .collect::<Vec<_>>();
    // Root aliases retain one reason per explicit root occurrence (the
    // program-preprocessing contract exposes that multiplicity).  A root
    // spelling arriving after an import/reference is different: tsc reports
    // the dependency chain once and does not repeat the same files-list
    // reason for the alias occurrence.
    if root_arrived_after_reference {
        reasons.dedup();
    }
    let message = MessageChain::new(message, &arguments).with_next(vec![MessageChain::new(
        &gen::The_file_is_in_the_program_because,
        &[],
    )
    .with_next(reasons)]);
    let location_reason = if incoming_reason.is_referenced() {
        Some(incoming_reason)
    } else {
        existing_reasons
            .iter()
            .find(|reason| reason.is_referenced())
    };
    let (file_name, start, length) = location_reason
        .and_then(source_inclusion_location)
        .map_or((None, None, None), |(path, start, end)| {
            (Some(path), Some(start), Some(end.saturating_sub(start)))
        });
    let mut diagnostic = Diagnostic::new(file_name, start, length, message);
    for reason in existing_reasons {
        let SourceInclusionReason::Root(reason) = reason else {
            continue;
        };
        let (option_name, spec, related_message) = match reason {
            RootFileReason::FilesList { spec } => (
                "files",
                spec,
                &gen::File_is_matched_by_files_list_specified_here,
            ),
            RootFileReason::IncludePattern { spec, .. } => (
                "include",
                spec,
                &gen::File_is_matched_by_include_pattern_specified_here,
            ),
            RootFileReason::Explicit | RootFileReason::DefaultInclude => continue,
        };
        let Some((config_file, location)) = config_file.and_then(|config_file| {
            config_file
                .root_option_array_location(option_name, spec)
                .map(|location| (config_file, location))
        }) else {
            continue;
        };
        diagnostic.related.push(RelatedInfo {
            file_name: Some(
                config_file
                    .path()
                    .display()
                    .to_str()
                    .expect("validated config paths are Unicode")
                    .to_owned(),
            ),
            start: Some(location.start()),
            length: Some(location.length()),
            message: MessageChain::new(related_message, &[]),
        });
    }
    diagnostic.related_information_present = !diagnostic.related.is_empty();
    diagnostic
}

fn source_inclusion_reason_message(reason: &SourceInclusionReason) -> Option<MessageChain> {
    let path_text = |path: &Path| path.to_str().map(str::to_owned);
    match reason {
        SourceInclusionReason::Root(root) => Some(root_file_reason_message(root)),
        SourceInclusionReason::Import {
            parent, specifier, ..
        } => Some(MessageChain::new(
            &gen::Imported_via_0_from_file_1,
            &[format!("'{specifier}'"), path_text(parent)?],
        )),
        SourceInclusionReason::PathReference {
            parent, specifier, ..
        } => Some(MessageChain::new(
            &gen::Referenced_via_0_from_file_1,
            &[format!("'{specifier}'"), path_text(parent)?],
        )),
        SourceInclusionReason::TypeReference {
            parent, specifier, ..
        } => Some(MessageChain::new(
            &gen::Type_library_referenced_via_0_from_file_1,
            &[format!("'{specifier}'"), path_text(parent)?],
        )),
        SourceInclusionReason::AutomaticType { name } => Some(MessageChain::new(
            &gen::Entry_point_of_type_library_0_specified_in_compilerOptions,
            std::slice::from_ref(name),
        )),
        SourceInclusionReason::Library => {
            Some(MessageChain::new(&gen::File_is_library_specified_here, &[]))
        }
        SourceInclusionReason::Synthetic => None,
    }
}

fn source_inclusion_location(reason: &SourceInclusionReason) -> Option<(String, u32, u32)> {
    let (parent, pos, end) = match reason {
        SourceInclusionReason::Import {
            parent, pos, end, ..
        }
        | SourceInclusionReason::PathReference {
            parent, pos, end, ..
        }
        | SourceInclusionReason::TypeReference {
            parent, pos, end, ..
        } => (parent, *pos, *end),
        _ => return None,
    };
    Some((parent.to_str()?.to_owned(), pos, end))
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
