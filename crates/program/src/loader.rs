use crate::js_string_ops::types_package_name;
use rustc_hash::{FxHashMap, FxHashSet as HashSet};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use tsc_diagnostics::{gen, Diagnostic, JsStr, JsString, MessageChain, RelatedInfo};
use tsc_host::{to_file_name_lower_case_js, CompilerHost, HostError, ParallelSourceReader};
use tsc_types::CompilerOptions;

use crate::json::{json_object_get, parse_json_object};
use crate::library::{replacement_package_name, LibraryCatalog};
use crate::module_requests::{
    is_declaration_file_name, plan_source_requests_retaining_syntax, PlannedLibReferenceDirective,
    PlannedPathReference, PlannedTypeReferenceDirective, SourceRequestPlan,
};
use crate::module_resolution::PackageJsonProbe;
use crate::module_resolution::{
    make_program_path, HostModuleResolution, HostResolvedTypeReferenceDirective, ModuleResolver,
};
use crate::path::{CanonicalPath, ProgramPath};
use crate::prepared::{
    extensionless_source_probe_extensions, PackageJsonType, PackageMetadata, PathContext,
    PreparationDiagnostics, PreparedAuxiliaryFile, PreparedProgram, PreparedProgramMode,
    PreparedRoot, PreparedSourceFile, ProgramConfigFile, ProgramOptions, SourceFileId,
};
use crate::project_references::ResolvedProjectReferences;
use crate::resolution::{
    ModuleExtension, ModuleResolution, PackageId, ResolutionError, ResolutionKey, ResolutionMode,
    ResolutionOutcome, ResolvedModuleTarget, TypeReferenceResolution, TypeReferenceResolutionKey,
    UnloadedModuleReason,
};
use crate::text::{decode_host_text, HostTextDecodeError};
use crate::workers::WorkerBudget;
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
///
/// The source-count and byte limits are joint bounds over the admitted
/// program sources and any root payloads the loader reads ahead of its
/// sequential discovery (see `CompilerHost::permits_source_read_ahead`): a
/// read-ahead payload is retained only while `admitted + retained` stays
/// within both limits, so the limits bound the live decoded/parsed source
/// state regardless of read-ahead.
///
/// `workers` is the [`WorkerBudget`] for the load's scoped parse-ahead.
/// [`Self::new`] keeps the serial default (no read-ahead, no worker thread),
/// which is the exact pre-concurrency behaviour; callers opt in with
/// [`Self::with_workers`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProgramLoadLimits {
    max_source_files: usize,
    max_request_edges: usize,
    max_source_depth: usize,
    max_source_file_bytes: usize,
    max_total_source_bytes: usize,
    workers: WorkerBudget,
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
            workers: WorkerBudget::serial(),
        }
    }

    /// The same limits with an explicit worker budget for parse-ahead.
    /// tsrs-native: see [`WorkerBudget`].
    pub const fn with_workers(mut self, workers: WorkerBudget) -> Self {
        self.workers = workers;
        self
    }

    pub const fn workers(self) -> WorkerBudget {
        self.workers
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
    js_path: Option<JsString>,
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

    pub fn js_path(&self) -> Option<JsStr<'_>> {
        self.js_path.as_ref().map(JsString::as_js)
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
    ResolveLibrary,
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
            Self::ResolveLibrary => "resolve standard-library replacement",
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
        js_path: Option<JsString>,
        detail: String,
    },
    Unsupported {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        js_path: Option<JsString>,
        feature: String,
        detail: String,
    },
    InvalidData {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        js_path: Option<JsString>,
        detail: String,
    },
    LimitExceeded {
        operation: ProgramLoadOperation,
        exceeded: ProgramLoadLimitExceeded,
    },
    Host {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        js_path: Option<JsString>,
        source: Box<HostError>,
    },
    Decode {
        operation: ProgramLoadOperation,
        path: PathBuf,
        js_path: Option<JsString>,
        source: HostTextDecodeError,
    },
    Resolution {
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        js_path: Option<JsString>,
        specifier: Option<JsString>,
        source: Box<ResolutionError>,
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
            js_path: path
                .as_ref()
                .and_then(|path| path.to_str())
                .map(JsString::from),
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
            js_path: path
                .as_ref()
                .and_then(|path| path.to_str())
                .map(JsString::from),
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
            js_path: path
                .as_ref()
                .and_then(|path| path.to_str())
                .map(JsString::from),
            path,
            detail: detail.into(),
        }
    }

    fn host(operation: ProgramLoadOperation, path: Option<PathBuf>, source: HostError) -> Self {
        Self::Host {
            operation,
            js_path: path
                .as_ref()
                .and_then(|path| path.to_str())
                .map(JsString::from),
            path,
            source: Box::new(source),
        }
    }

    fn resolution(
        operation: ProgramLoadOperation,
        path: Option<PathBuf>,
        specifier: Option<JsString>,
        source: ResolutionError,
    ) -> Self {
        Self::Resolution {
            operation,
            js_path: path
                .as_ref()
                .and_then(|path| path.to_str())
                .map(JsString::from),
            path,
            specifier,
            source: Box::new(source),
        }
    }

    pub fn js_path(&self) -> Option<JsStr<'_>> {
        match self {
            Self::InvalidInput { js_path, .. }
            | Self::Unsupported { js_path, .. }
            | Self::InvalidData { js_path, .. }
            | Self::Host { js_path, .. }
            | Self::Resolution { js_path, .. }
            | Self::Decode { js_path, .. } => js_path.as_ref().map(JsString::as_js),
            Self::LimitExceeded { exceeded, .. } => exceeded.js_path(),
            Self::Preparation { source, .. } => source.js_path(),
        }
    }

    fn with_js_path(mut self, value: Option<JsString>) -> Self {
        match &mut self {
            Self::InvalidInput { js_path, .. }
            | Self::Unsupported { js_path, .. }
            | Self::InvalidData { js_path, .. }
            | Self::Host { js_path, .. }
            | Self::Resolution { js_path, .. }
            | Self::Decode { js_path, .. } => *js_path = value,
            Self::LimitExceeded { .. } | Self::Preparation { .. } => {
                unreachable!("this error owns its path in its source")
            }
        }
        self
    }

    fn invalid_input_js(
        operation: ProgramLoadOperation,
        path: Option<JsString>,
        detail: impl Into<String>,
    ) -> Self {
        Self::invalid_input(operation, error_display_path(path.as_ref()), detail).with_js_path(path)
    }

    pub(crate) fn unsupported_js(
        operation: ProgramLoadOperation,
        path: Option<JsString>,
        feature: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self::unsupported(
            operation,
            error_display_path(path.as_ref()),
            feature,
            detail,
        )
        .with_js_path(path)
    }

    pub(crate) fn invalid_data_js(
        operation: ProgramLoadOperation,
        path: Option<JsString>,
        detail: impl Into<String>,
    ) -> Self {
        Self::invalid_data(operation, error_display_path(path.as_ref()), detail).with_js_path(path)
    }

    pub(crate) fn host_js(
        operation: ProgramLoadOperation,
        path: Option<JsString>,
        source: HostError,
    ) -> Self {
        Self::host(operation, error_display_path(path.as_ref()), source).with_js_path(path)
    }

    fn resolution_js(
        operation: ProgramLoadOperation,
        path: Option<JsString>,
        specifier: Option<JsString>,
        source: ResolutionError,
    ) -> Self {
        Self::resolution(
            operation,
            error_display_path(path.as_ref()),
            specifier,
            source,
        )
        .with_js_path(path)
    }

    fn resolution_with_source_path(
        operation: ProgramLoadOperation,
        specifier: Option<JsString>,
        source: ResolutionError,
    ) -> Self {
        let js_path = source.js_path().map(JsStr::to_owned);
        let native_path = source.path().map(Path::to_owned);
        Self::resolution(operation, native_path, specifier, source).with_js_path(js_path)
    }

    fn decode_js(
        operation: ProgramLoadOperation,
        path: JsString,
        source: HostTextDecodeError,
    ) -> Self {
        Self::Decode {
            operation,
            path: error_display_path(Some(&path)).expect("decode has a source"),
            js_path: Some(path),
            source,
        }
    }

    fn preparation(operation: ProgramLoadOperation, source: PreparationError) -> Self {
        Self::Preparation { operation, source }
    }
}

/// Only the infrastructure error's native display context uses this encoding.
/// JS path identity is retained independently and never reconstructed from it.
fn error_display_path(path: Option<&JsString>) -> Option<PathBuf> {
    path.map(|path| PathBuf::from(path.to_string_lossy().into_owned()))
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
            Self::Host { source, .. } => Some(source.as_ref()),
            Self::Decode { source, .. } => Some(source),
            Self::Resolution { source, .. } => Some(source.as_ref()),
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
        PreparedProgramMode::NoEmit,
        host,
        RootNames::Native(root_names),
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
        PreparedProgramMode::NoEmit,
        host,
        RootNames::Native(root_names),
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
        None,
    )
}

/// Load one finite source closure for the distinct H1 emitting session.
///
/// This entry deliberately does not generalize [`load_program`]: the H0
/// loader continues to require `noEmit=true`, while this loader rejects an
/// effective `noEmit=true` before source discovery. Emit-profile validation
/// remains owned by the later emitter preflight so unsupported requests can
/// retain the same prepared source/config diagnostics without reaching an
/// output sink.
pub fn load_emitting_program(
    host: &dyn CompilerHost,
    root_names: &[PathBuf],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_program_worker(
        PreparedProgramMode::Emit,
        host,
        RootNames::Native(root_names),
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
pub enum RootFileReason {
    Explicit,
    FilesList {
        spec: Arc<JsString>,
    },
    IncludePattern {
        spec: Arc<JsString>,
        config_file: Arc<JsString>,
    },
    DefaultInclude,
}

/// Load a config-derived root closure while retaining the source of each root
/// spelling for TypeScript's inclusion-chain diagnostics.
pub(crate) fn load_program_with_root_reasons(
    host: &dyn CompilerHost,
    roots: &[(JsString, RootFileReason)],
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
        PreparedProgramMode::NoEmit,
        host,
        RootNames::Js(&root_names),
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
        Some(&root_reasons),
    )
}

pub(crate) fn load_emitting_program_with_root_reasons(
    host: &dyn CompilerHost,
    roots: &[(JsString, RootFileReason)],
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
        PreparedProgramMode::Emit,
        host,
        RootNames::Js(&root_names),
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
        Some(&root_reasons),
    )
}

#[derive(Clone, Copy)]
enum RootNames<'a> {
    Native(&'a [PathBuf]),
    Js(&'a [JsString]),
}

impl<'a> RootNames<'a> {
    fn len(self) -> usize {
        match self {
            Self::Native(names) => names.len(),
            Self::Js(names) => names.len(),
        }
    }

    fn name(self, index: usize) -> Result<JsStr<'a>, ProgramLoadError> {
        match self {
            Self::Native(names) => {
                native_load_path(&names[index], ProgramLoadOperation::NormalizeRoot)
            }
            Self::Js(names) => Ok(names[index].as_js()),
        }
    }
}

fn native_load_path(
    path: &Path,
    operation: ProgramLoadOperation,
) -> Result<JsStr<'_>, ProgramLoadError> {
    path.to_str().map(JsStr::from_str).ok_or_else(|| {
        ProgramLoadError::invalid_input(
            operation,
            Some(path.to_owned()),
            "path is not valid Unicode",
        )
    })
}

#[allow(clippy::too_many_arguments)] // Root provenance is an orthogonal config-only input.
fn load_program_worker(
    mode: PreparedProgramMode,
    host: &dyn CompilerHost,
    root_names: RootNames<'_>,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: Option<&LibraryCatalog>,
    require_no_lib: bool,
    limits: ProgramLoadLimits,
    root_reasons: Option<&[RootFileReason]>,
) -> Result<PreparedProgram, ProgramLoadError> {
    validate_admitted_options(
        mode,
        &compiler_options,
        &program_options,
        library_catalog,
        require_no_lib,
    )?;

    let mut resolver =
        ModuleResolver::new_with_program_options(host, &compiler_options, &program_options)
            .map_err(|error| {
                ProgramLoadError::resolution_with_source_path(
                    ProgramLoadOperation::InitializeResolver,
                    None,
                    error,
                )
            })?;
    // tsc resolves replacement libraries with an isolated Node10 option set;
    // ordinary module options such as paths, baseUrl, moduleSuffixes, and
    // package exports must not influence this lookup.
    let library_resolution_options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut library_resolver = (compiler_options.lib_replacement == Some(true)
        && program_options.no_lib() != Some(true))
    .then(|| ModuleResolver::new(host, &library_resolution_options))
    .transpose()
    .map_err(|error| {
        ProgramLoadError::resolution_js(ProgramLoadOperation::InitializeResolver, None, None, error)
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

    let mut graph = StagedGraph::new(StagedGraphConfig {
        host,
        compiler_options: &compiler_options,
        program_options: &program_options,
        library_catalog,
        library_directory,
        limits,
        resolver: &mut resolver,
        library_resolver: library_resolver.as_mut(),
    });
    // Parse-ahead of the explicit roots. Normalization errors are left for
    // the sequential loop below, which reports them in root order.
    let mut prefetch_roots = Vec::with_capacity(root_names.len());
    for index in 0..root_names.len() {
        let Ok(root_spelling) = root_names.name(index) else {
            break;
        };
        let Ok(root) = normalize_root(root_spelling, &path_context) else {
            break;
        };
        prefetch_roots.push(root);
    }
    let phase_started = std::time::Instant::now();
    // What the walk loads after the roots (the selected libraries and the
    // automatic type directives' targets) reads ahead with them, so those
    // parses overlap the largest root's instead of following it. A program
    // without roots loads neither (tsgo processAllProgramFiles adds the lib
    // and automatic type directive tasks only with root files), so nothing
    // is read ahead or resolved for it.
    if root_names.len() != 0 {
        prefetch_roots.extend(graph.read_ahead_seeds());
    }
    graph.prefetch_roots(&prefetch_roots);
    drop(prefetch_roots);
    tsc_types::trace::mark("load: read-ahead parse of roots", phase_started);
    let phase_started = std::time::Instant::now();
    graph.prefetch_dependencies();
    tsc_types::trace::mark("load: read-ahead of dependencies", phase_started);
    let phase_started = std::time::Instant::now();
    // tsgo processRootFiles: the root tasks are the root files, then the
    // selected libraries, then the automatic type directives; the
    // single-threaded work queue then runs them, the last queued first.
    for index in 0..root_names.len() {
        let root_spelling = root_names.name(index)?;
        let root = normalize_root(root_spelling, &path_context)?;
        let reason = root_reasons
            .and_then(|reasons| reasons.get(index).cloned())
            .unwrap_or(RootFileReason::Explicit);
        graph.load_root(root, root_spelling, reason)?;
    }
    if root_names.len() != 0 {
        if program_options.no_lib() != Some(true) {
            graph.load_selected_libraries()?;
        }
        graph.load_automatic_type_directives()?;
    }
    graph.run_tasks()?;
    tsc_types::trace::mark("load: parse tasks", phase_started);
    tsc_types::trace::mark(
        &format!(
            "load: root walk ({} pre-resolved hits, {} directory hits, {} left, {} resolutions)",
            graph.pre_resolved_hits,
            graph.directory_resolution_hits,
            graph.pre_resolved.len(),
            graph.module_resolutions.len()
        ),
        phase_started,
    );
    if tsc_types::trace::enabled() {
        // The sources whose read-ahead resolutions the walk never took: read
        // ahead in vain, so worth knowing about.
        let unvisited = graph.pre_resolved.keys().fold(
            BTreeMap::<&CanonicalPath, usize>::new(),
            |mut unvisited, key| {
                *unvisited.entry(key.source()).or_default() += 1;
                unvisited
            },
        );
        for (source, requests) in unvisited.iter().take(8) {
            eprintln!(
                "[phase] load: read ahead in vain: {:?} ({requests} requests)",
                source.as_js()
            );
        }
        if unvisited.len() > 8 {
            eprintln!(
                "[phase] load: read ahead in vain: {} more sources",
                unvisited.len() - 8
            );
        }
    }
    let mut staged = graph.finish()?;
    tsc_types::trace::mark("load: root walk and graph finish", phase_started);
    let phase_started = std::time::Instant::now();
    // The package.json files the walk probed, taken before the dependency
    // prelude below: tsgo's symlink walk (program.go GetSymlinkCache →
    // ResolvePackageDirectory) tests package directories without reading a
    // package.json, so the build info's package.json lists never carry the
    // prelude's probes.
    staged
        .package_json_probes
        .extend(resolver.take_package_json_probes());
    // Before the package table is collected: the prelude's resolutions read
    // package.json files under their symlink spellings, and module-specifier
    // generation later reads those spellings back (upstream shares one
    // package.json info cache between resolution and specifier generation).
    let dependency_symlink_resolutions =
        resolve_runtime_dependency_symlinks(&mut resolver, &staged)?;
    let mut packages_by_path = BTreeMap::new();
    for package in resolver.observed_package_metadata().chain(
        library_resolver
            .iter()
            .flat_map(|resolver| resolver.observed_package_metadata()),
    ) {
        packages_by_path
            .entry(package.package_json().canonical().clone())
            .or_insert_with(|| package.clone());
    }
    // tsc's module-specifier host reads package.json through the real file
    // system (_tsc.js:46119-46120, 123503-123508), so every manifest a
    // resolution went through must be in the table under the spelling it was
    // read at. Read-ahead resolutions run on worker resolvers whose package
    // caches are dropped; their results still carry the governing manifest —
    // for a pnpm package that is `node_modules/<pkg>/package.json`, while the
    // realpath'd source's own scope records the `.pnpm/...` spelling.
    for resolution in &staged.module_resolutions {
        if let ResolutionOutcome::Resolved(module) = resolution.host.outcome() {
            if let Some(package) = module.package_metadata() {
                packages_by_path
                    .entry(package.package_json().canonical().clone())
                    .or_insert_with(|| package.clone());
            }
        }
    }
    for resolution in &staged.type_resolutions {
        if let ResolutionOutcome::Resolved(directive) = &resolution.host {
            if let Some(package) = directive.package_metadata() {
                packages_by_path
                    .entry(package.package_json().canonical().clone())
                    .or_insert_with(|| package.clone());
            }
        }
    }
    let packages = packages_by_path.into_values().collect::<Vec<_>>();
    if let Some(library_resolver) = library_resolver.as_mut() {
        staged
            .package_json_probes
            .extend(library_resolver.take_package_json_probes());
    }
    drop(resolver);
    drop(library_resolver);
    tsc_types::trace::mark("load: dependency symlinks and packages", phase_started);

    publish_program(
        mode,
        staged,
        packages,
        dependency_symlink_resolutions,
        path_context,
        compiler_options,
        program_options,
    )
}

/// tsc-port: getAllModulePathsWorker @6.0.3
/// tsc-hash: 29f3f2796fae09795c9ba292485de9c6f7c91888e1b419dc364964bc6814c791
/// tsc-span: _tsc.js:45717-45741
///
/// The prelude of upstream's module-path enumeration: for an importing file
/// outside `node_modules`, every runtime dependency of its package scope is
/// resolved from `<packageDirectory>/package.json` and the resolution is fed
/// to the symlink cache (`links.setSymlinksFromResolution`). Upstream runs it
/// lazily per importing file whenever a module specifier is computed; the
/// program runs it once here, over every non-library source outside
/// `node_modules`, and keeps only the resolutions that went through a
/// symlink — the only observable product (the cache) is identical.
fn resolve_runtime_dependency_symlinks(
    resolver: &mut ModuleResolver<'_>,
    staged: &CompleteGraph,
) -> Result<Vec<(ProgramPath, ProgramPath)>, ProgramLoadError> {
    let mut seen_scopes: BTreeSet<CanonicalPath> = BTreeSet::new();
    let mut seen_pairs: BTreeSet<(CanonicalPath, CanonicalPath)> = BTreeSet::new();
    let mut resolutions = Vec::new();
    for source in &staged.sources {
        if source.library_priority.is_some() {
            continue;
        }
        let file = source.prepared.path();
        if display_path_contains_node_modules(file.display()) {
            continue;
        }
        let Some(package) = resolver
            .package_scope_for_file(file.display())
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::ObservePackageScope,
                    Some(file.display().to_owned()),
                    None,
                    error,
                )
            })?
        else {
            continue;
        };
        if !seen_scopes.insert(package.package_json().canonical().clone()) {
            continue;
        }
        for name in runtime_dependency_names(package.package_json(), package.text()) {
            let outcome = resolver
                .resolve(
                    package.package_json().display(),
                    &name,
                    ResolutionMode::Unspecified,
                )
                .map_err(|error| {
                    ProgramLoadError::resolution_js(
                        ProgramLoadOperation::ResolveModule,
                        Some(package.package_json().display().to_owned()),
                        Some(name.clone()),
                        error,
                    )
                })?;
            let ResolutionOutcome::Resolved(module) = outcome else {
                continue;
            };
            let Some(original_path) = module.original_path() else {
                continue;
            };
            let pair = (
                module.resolved_file().canonical().clone(),
                original_path.canonical().clone(),
            );
            if seen_pairs.insert(pair) {
                resolutions.push((module.resolved_file().clone(), original_path.clone()));
            }
        }
    }
    Ok(resolutions)
}

/// tsc-port: getAllRuntimeDependencies @6.0.3
/// tsc-hash: 62d9e01fb8c9f3f49fcd53deafb8cb72ff3d1b91d8b9f86ad573d1f29900a484
/// tsc-span: _tsc.js:45707-45716
fn runtime_dependency_names(package_json: &ProgramPath, text: &str) -> Vec<JsString> {
    let (_, object) = parse_json_object(package_json.display(), text.to_owned());
    let mut names = Vec::new();
    for field in ["dependencies", "peerDependencies", "optionalDependencies"] {
        if let Some(crate::JsonValue::Object(dependencies)) = json_object_get(&object, field) {
            names.extend(
                dependencies
                    .keys()
                    .filter_map(crate::json::decode_user_object_key)
                    .map(JsStr::to_owned),
            );
        }
    }
    names
}

fn validate_admitted_options(
    mode: PreparedProgramMode,
    compiler_options: &CompilerOptions,
    program_options: &ProgramOptions,
    library_catalog: Option<&LibraryCatalog>,
    require_no_lib: bool,
) -> Result<(), ProgramLoadError> {
    let reject_input = |detail| {
        ProgramLoadError::invalid_input_js(ProgramLoadOperation::ValidateOptions, None, detail)
    };
    match mode {
        PreparedProgramMode::NoEmit if compiler_options.no_emit != Some(true) => {
            return Err(reject_input(
                "compilerOptions.noEmit must be explicitly true",
            ));
        }
        PreparedProgramMode::Emit if compiler_options.no_emit == Some(true) => {
            return Err(reject_input(
                "emitting program rejects effective compilerOptions.noEmit=true",
            ));
        }
        PreparedProgramMode::NoEmit | PreparedProgramMode::Emit => {}
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
            return Err(ProgramLoadError::invalid_input_js(
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
            return Err(ProgramLoadError::invalid_input_js(
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
    let directory = native_load_path(catalog.directory(), ProgramLoadOperation::ValidateOptions)?;
    reject_unowned_drive_relative_path(directory, ProgramLoadOperation::ValidateOptions)?;
    let normalized = crate::module_resolution::normalize_absolute_js_path(
        directory,
        Some(path_context.current_directory().display()),
        true,
    )
    .map_err(|error| {
        ProgramLoadError::resolution_js(
            ProgramLoadOperation::ValidateOptions,
            Some(directory.to_owned()),
            None,
            error,
        )
    })?;
    make_program_path(
        normalized.as_js(),
        path_context.use_case_sensitive_file_names(),
    )
    .map_err(|error| {
        ProgramLoadError::resolution_js(
            ProgramLoadOperation::ValidateOptions,
            Some(directory.to_owned()),
            None,
            error,
        )
    })
}

fn normalize_root(
    root: JsStr<'_>,
    path_context: &PathContext,
) -> Result<ProgramPath, ProgramLoadError> {
    reject_unowned_drive_relative_path(root, ProgramLoadOperation::NormalizeRoot)?;
    let trailing_separator = root.ends_with("/") || root.ends_with("\\");
    let mut normalized = crate::module_resolution::normalize_absolute_js_path(
        root,
        Some(path_context.current_directory().display()),
        true,
    )
    .map_err(|error| {
        ProgramLoadError::resolution_js(
            ProgramLoadOperation::NormalizeRoot,
            Some(root.to_owned()),
            None,
            error,
        )
    })?;
    if trailing_separator && !normalized.ends_with("/") {
        normalized.push('/');
    }
    make_program_path(
        normalized.as_js(),
        path_context.use_case_sensitive_file_names(),
    )
    .map_err(|error| {
        ProgramLoadError::resolution_js(
            ProgramLoadOperation::NormalizeRoot,
            Some(root.to_owned()),
            None,
            error,
        )
    })
}

fn path_has_extension(path: JsStr<'_>) -> bool {
    let basename = path.split_ascii(b'/').next_back().unwrap_or(path);
    let basename = basename.split_ascii(b'\\').next_back().unwrap_or(basename);
    basename.contains(".")
}

fn validate_type_roots(
    options: &ProgramOptions,
    path_context: &PathContext,
) -> Result<(), ProgramLoadError> {
    let Some(type_roots) = options.type_roots() else {
        return Ok(());
    };
    for type_root in type_roots {
        let path = type_root.display();
        reject_unowned_drive_relative_path(path, ProgramLoadOperation::ValidateOptions)?;
        let normalized = crate::module_resolution::normalize_absolute_js_path(
            path,
            Some(path_context.current_directory().display()),
            true,
        )
        .map_err(|error| {
            ProgramLoadError::resolution_js(
                ProgramLoadOperation::ValidateOptions,
                Some(path.to_owned()),
                None,
                error,
            )
        })?;
        let normalized = make_program_path(
            normalized.as_js(),
            path_context.use_case_sensitive_file_names(),
        )
        .map_err(|error| {
            ProgramLoadError::resolution_js(
                ProgramLoadOperation::ValidateOptions,
                Some(path.to_owned()),
                None,
                error,
            )
        })?;
        if &normalized != type_root {
            return Err(ProgramLoadError::invalid_input_js(
                ProgramLoadOperation::ValidateOptions,
                Some(path.to_owned()),
                "typeRoots entries must already carry normalized display and canonical identities",
            ));
        }
    }
    Ok(())
}

fn reject_unowned_drive_relative_path(
    path: JsStr<'_>,
    operation: ProgramLoadOperation,
) -> Result<(), ProgramLoadError> {
    let slashed = crate::js_path::normalize_slashes(path);
    let bytes = slashed.as_bytes();
    let drive_relative =
        bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] != b'/';
    if drive_relative {
        return Err(ProgramLoadError::unsupported_js(
            operation,
            Some(path.to_owned()),
            "windows-path-form",
            "drive-relative root spellings are not yet owned",
        ));
    }
    Ok(())
}

/// Why a file is in the program (tsgo `FileIncludeReason`), as the loader
/// recorded it: one entry per occurrence of the reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceInclusionReason {
    Root(RootFileReason),
    Import {
        parent: JsString,
        reference_text: String,
        pos: u32,
        end: u32,
        /// The resolution's package identity text, when it has one: the
        /// explanation then reads "... with packageId '...'" (tsgo
        /// fileInclude.go computeReferenceFileDiagnostic).
        package_id: Option<JsString>,
    },
    PathReference {
        parent: JsString,
        specifier: JsString,
        pos: u32,
        end: u32,
    },
    TypeReference {
        parent: JsString,
        specifier: JsString,
        pos: u32,
        end: u32,
        package_id: Option<JsString>,
    },
    AutomaticType {
        name: JsString,
        package_id: Option<JsString>,
        /// `types` names `*` (tsgo CompilerOptions.UsesWildcardTypes): the
        /// entry point is then explained as that of an implicit type
        /// library.
        implicit: bool,
    },
    Synthetic,
    /// A library root (tsgo `fileIncludeKindLibFile`).
    LibraryRoot(LibraryRootReason),
    /// A `/// <reference lib="..." />` directive (tsgo
    /// `fileIncludeKindLibReferenceDirective`).
    LibraryReference {
        parent: JsString,
        specifier: JsString,
    },
}

impl SourceInclusionReason {
    const fn is_referenced(&self) -> bool {
        !matches!(self, Self::Root(_) | Self::LibraryRoot(_))
    }

    /// Records the resolution's package identity on the reasons whose
    /// explanation names it.
    fn with_package_id(mut self, package_id: Option<&PackageId>) -> Self {
        let text = package_id.map(PackageId::display_text);
        match &mut self {
            Self::Import { package_id, .. }
            | Self::TypeReference { package_id, .. }
            | Self::AutomaticType { package_id, .. } => *package_id = text,
            Self::Root(_)
            | Self::PathReference { .. }
            | Self::Synthetic
            | Self::LibraryRoot(_)
            | Self::LibraryReference { .. } => {}
        }
        self
    }
}

#[derive(Clone, Debug)]
struct DiscoveryReason {
    inclusion: SourceInclusionReason,
    package_id: Option<PackageId>,
}

impl DiscoveryReason {
    fn root(reason: RootFileReason) -> Self {
        Self {
            inclusion: SourceInclusionReason::Root(reason),
            package_id: None,
        }
    }

    fn dependency(inclusion: SourceInclusionReason) -> Self {
        Self {
            inclusion,
            package_id: None,
        }
    }

    fn automatic_type(name: JsString, implicit: bool) -> Self {
        Self {
            inclusion: SourceInclusionReason::AutomaticType {
                name,
                package_id: None,
                implicit,
            },
            package_id: None,
        }
    }

    fn with_package_id(mut self, package_id: Option<PackageId>) -> Self {
        self.inclusion = self.inclusion.with_package_id(package_id.as_ref());
        self.package_id = package_id;
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SourceClass {
    Ordinary,
    Library { priority: usize, replacement: bool },
}

impl SourceClass {
    const fn library_priority(self) -> Option<usize> {
        match self {
            Self::Ordinary => None,
            Self::Library { priority, .. } => Some(priority),
        }
    }

    const fn is_library(self) -> bool {
        self.library_priority().is_some()
    }

    const fn is_replacement(self) -> bool {
        matches!(
            self,
            Self::Library {
                replacement: true,
                ..
            }
        )
    }
}

struct StagedSource {
    prepared: PreparedSourceFile,
    external_module_diagnostic_span: Option<(u32, u32)>,
    /// Root-file inclusion occurrences are retained separately from the
    /// canonical source identity.  They are observable in the TS1149
    /// program-preprocessing message chain when two root spellings collapse
    /// on a case-insensitive host.
    root_inclusions: Vec<JsString>,
    /// The include reasons in collect order (tsgo `fileIncludeReasons`),
    /// attached by the collect walk.
    inclusion_reasons: Vec<SourceInclusionReason>,
    alternate_inclusion_reasons: Vec<(JsString, SourceInclusionReason)>,
    /// tsgo `!sourceFilesFoundSearchingNodeModules`: the least depth any
    /// task of the path ran at is zero (`TaskData::lowest_depth`); decided
    /// by the collect walk.
    has_non_external_reason: bool,
    /// Program-owned default-library membership is independent of how the
    /// source first entered the graph. A replacement declaration may already
    /// be an explicit root when a later lib lookup selects the same identity.
    library_priority: Option<usize>,
    library_replacement: bool,
    /// findSourceFileWorker chooses its processing bucket at first load.
    /// A root later selected by a lib reference remains in processingOtherFiles.
    initially_library: bool,
    path_references: Vec<PlannedPathReference>,
    type_reference_directives: Vec<PlannedTypeReferenceDirective>,
    lib_reference_directives: Vec<PlannedLibReferenceDirective>,
    module_requests: Vec<(ResolutionKey, bool)>,
    /// The spans of the occurrences of each request that load a source, in
    /// source order; a request without one is synthetic.
    module_request_spans: rustc_hash::FxHashMap<ResolutionKey, Vec<(u32, u32)>>,
}

impl StagedSource {
    const fn source_class(&self) -> SourceClass {
        match self.library_priority {
            Some(priority) => SourceClass::Library {
                priority,
                replacement: self.library_replacement,
            },
            None => SourceClass::Ordinary,
        }
    }
}

/// A pair of distinct source identities that collide only under tsc's
/// locale-independent file-name fold on a case-sensitive host. Both sources
/// remain in the program; this record exists only to publish TS1149/TS1261.
struct CaseSensitiveCasingConflict {
    existing_source: usize,
    incoming_path: JsString,
    incoming_reason: SourceInclusionReason,
}

struct StagedRoot {
    path: ProgramPath,
    source: Option<usize>,
    missing_diagnostic: Option<Diagnostic>,
    /// The root's parse task; its source is read after the tasks ran.
    task: Option<usize>,
}

/// Why a parse task exists (the origin of tsgo `parseTask.includeReason`),
/// with what the diagnostic of a target that loads nothing needs.
#[derive(Clone, Debug)]
enum TaskOrigin {
    /// A root file, spelled as the request wrote it.
    Root {
        spelling: JsString,
        reason: RootFileReason,
    },
    LibraryRoot(LibraryRootReason),
    /// tsgo `isForAutomaticTypeDirective`: the task that resolves the
    /// automatic type directives; its subtasks are their targets.
    AutomaticTypes,
    PathReference {
        parent: usize,
        pos: u32,
        length: u32,
        reference_text: JsString,
    },
    TypeReference,
    LibReference {
        parent: usize,
        pos: u32,
        length: u32,
    },
    Import,
    /// tsgo `parseTask.redirect`: the output declaration file of a source
    /// of a referenced project, loaded in the source's place.
    Redirect,
}

/// tsgo `parseTask` (compiler/filesparser.go): one arrival at a file
/// through one reference. The tasks of one path share a [`TaskData`]; a
/// task whose spelling the data already registered when it runs is an alias
/// of the registered task (`loaded_task`) and loads nothing itself.
struct ParseTask {
    path: ProgramPath,
    origin: TaskOrigin,
    reason: DiscoveryReason,
    class: SourceClass,
    /// tsgo `increaseDepth`: the reference resolved into an external
    /// library, so the target runs one depth deeper.
    increase_depth: bool,
    /// tsgo `elideOnDepth`: a JavaScript file from `node_modules` is not
    /// loaded at a depth beyond `maxNodeModuleJsDepth`.
    elide_on_depth: bool,
    /// The depth in the task tree, for the source-depth limit.
    tree_depth: usize,
    data: usize,
    /// tsgo `loaded` of `taskDataByPath.LoadOrStore`: the data existed when
    /// the task was queued, so the task registers its spelling (or finds
    /// it registered) when it runs.
    queued_with_data: bool,
    subtasks: Vec<usize>,
    loaded: bool,
    started_subtasks: bool,
    loaded_task: Option<usize>,
    source: Option<usize>,
    redirect: Option<usize>,
}

/// tsgo `parseTaskData`: what the tasks of one path share.
struct TaskData {
    /// The task registered for each spelling (tsgo `tasks` by file name).
    spellings: Vec<(JsString, usize)>,
    /// tsgo `lowestDepth`: the least depth any task of the path ran at.
    lowest_depth: usize,
    started_subtasks: bool,
    /// The first non-empty package identity of the path's tasks.
    package_id: Option<PackageId>,
}

/// What the collect walk decided (see `StagedGraph::collect_files`).
struct CollectedFiles {
    /// The kept sources in program order (each after its subtasks).
    order: Vec<usize>,
    /// `(source, owner, index)`: the source's package identity belongs to
    /// `owner`, which was entered earlier; the source is dropped as a
    /// redirect, listed at `index` of the program's file order (tsgo
    /// `redirectsFile.index`).
    redirects: Vec<(usize, usize, usize)>,
}

/// A root source read ahead of its sequential visit; see
/// [`StagedGraph::prefetch_roots`] for the host contract and the resource
/// bound that permit it.
///
/// `admit_source` performs every admission, limit and package-scope step in
/// its original order. A retained read applies only to a task under
/// the identical display spelling (another spelling of the same canonical
/// path is a distinct host query and reads as before). A retained parse is
/// adopted only when the facts the worker assumed (display spelling and
/// implied module formats) are the ones the visit computed itself; otherwise
/// the retained text is planned on the loading thread exactly as a fresh
/// read would be.
struct PrefetchedSource {
    /// The display spelling the read-ahead queried.
    display: JsString,
    read: PrefetchedRead,
}

/// The fewest read-ahead roots for which the parse workers read the sources
/// themselves (see `StagedGraph::prefetch_roots`).
pub const PARALLEL_READ_AHEAD_MIN_ROOTS: usize = 64;

enum PrefetchedRead {
    /// `read_file_js` answered `Ok(None)`; the visit records the miss.
    Missing,
    /// `read_file_js` failed; the visit reports this error, never re-reads.
    Failed(HostError),
    /// Decoded and, when decoding succeeded, parsed and planned on a worker.
    /// `byte_len` is reserved against the load budget while the entry is
    /// retained. The parse is boxed so the small `Missing`/`Failed` entries
    /// do not carry its size.
    Parsed {
        byte_len: usize,
        decoded: Result<Box<PrefetchedParse>, HostTextDecodeError>,
    },
}

impl PrefetchedRead {
    /// The payload this entry holds against the joint load budget, if any.
    fn reserved_bytes(&self) -> Option<usize> {
        match self {
            Self::Parsed { byte_len, .. } => Some(*byte_len),
            Self::Missing | Self::Failed(_) => None,
        }
    }
}

struct PrefetchedParse {
    implied: Option<ResolutionMode>,
    implied_for_emit: Option<ResolutionMode>,
    prepared: PreparedSourceFile,
    plan: Result<SourceRequestPlan, ResolutionError>,
}

/// One unit of the pipelined dependency read-ahead
/// (`StagedGraph::prefetch_dependencies_pipelined`).
enum ReadAheadTask {
    Resolve {
        containing_file: JsString,
        key: ResolutionKey,
        loads_source: bool,
    },
    Read {
        path: ProgramPath,
        scope: Option<PackageMetadata>,
    },
}

enum ReadAheadOutcome {
    Resolved {
        key: ResolutionKey,
        loads_source: bool,
        host: Option<Box<HostModuleResolution>>,
    },
    Read {
        path: ProgramPath,
        read: Option<PrefetchedRead>,
    },
    /// A worker's resolver is done: the package.json files it probed.
    Probes(Vec<PackageJsonProbe>),
}

struct ReadAheadQueue {
    pending: VecDeque<ReadAheadTask>,
    closed: bool,
    /// Workers blocked in `take`: a push signals only while one waits, so a
    /// burst of tasks into busy workers costs no wake-up system calls (VS
    /// Code: 57k pushes, and the signals were 8% of the loading thread).
    waiting: usize,
}

struct ReadAheadPipeline {
    queue: std::sync::Mutex<ReadAheadQueue>,
    ready: std::sync::Condvar,
}

impl ReadAheadPipeline {
    fn push(&self, task: ReadAheadTask) {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        queue.pending.push_back(task);
        let waiting = queue.waiting > 0;
        drop(queue);
        if waiting {
            self.ready.notify_one();
        }
    }

    /// The next task, waiting for one; `None` once the queue is closed and
    /// drained.
    fn take(&self) -> Option<ReadAheadTask> {
        let mut queue = self
            .queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(task) = queue.pending.pop_front() {
                return Some(task);
            }
            if queue.closed {
                return None;
            }
            queue.waiting += 1;
            queue = self
                .ready
                .wait(queue)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            queue.waiting -= 1;
        }
    }

    fn close(&self) {
        self.queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
        self.ready.notify_all();
    }
}

/// The loading thread's bookkeeping for the pipelined read-ahead.
#[derive(Default)]
struct ReadAheadState {
    outstanding: usize,
    reads_in_flight: usize,
    resolutions: usize,
    directory_reuses: usize,
    reads: usize,
    /// Targets queued for a read (canonical), so a target reached twice is
    /// read once.
    queued: BTreeSet<CanonicalPath>,
    /// Requests waiting for the in-flight resolution of their (containing
    /// directory, specifier, mode).
    waiting:
        rustc_hash::FxHashMap<(JsString, JsString, ResolutionMode), Vec<(ResolutionKey, bool)>>,
    /// Set once a read hit the load budget or failed: nothing more is read
    /// ahead (the walk reads and reports as before).
    stopped: bool,
}

/// The text of a source about to be planned: a fresh host read, or the
/// retained read-ahead of the same bytes.
enum SourceInput {
    Fresh(Vec<u8>),
    Prefetched {
        byte_len: usize,
        decoded: Result<Box<PrefetchedParse>, HostTextDecodeError>,
    },
}

enum DecodedSource {
    Text(String),
    Parsed(Box<PrefetchedParse>),
}

/// Decode, snapshot, parse and request-plan one read-ahead root on a worker.
/// The implied module formats are computed without a package scope (roots
/// whose format needs one never reach this path); the sequential visit
/// re-derives them and adopts the parse only on agreement.
///
/// The parser and planner are total over decoded text: they report
/// malformed input through diagnostics and resolution errors, never by
/// panicking. A panic here is therefore a compiler defect, and it
/// propagates through the scoped worker to the loading thread exactly as a
/// sequential parse panic would; read-ahead does not catch or defer it.
fn parse_root_ahead(
    path: &ProgramPath,
    bytes: Vec<u8>,
    options: &CompilerOptions,
    package_scope: Option<&PackageMetadata>,
) -> PrefetchedRead {
    let byte_len = bytes.len();
    // Under the phase trace a large source reports its decode, snapshot and
    // parse-plus-plan steps separately.
    let traced = (tsc_types::trace::enabled() && byte_len >= 256 << 10).then(|| {
        (
            std::time::Instant::now(),
            path.display().to_string_lossy().into_owned(),
        )
    });
    let decoded = decode_host_text(bytes).map(|text| {
        if let Some((started, name)) = &traced {
            tsc_types::trace::mark(&format!("load: decode {name} ({byte_len} bytes)"), *started);
        }
        let prepare_started = std::time::Instant::now();
        let implied = implied_node_format(path.display(), package_scope, options);
        let implied_for_emit = implied_node_format_for_emit(path.display(), package_scope, options);
        let prepared = PreparedSourceFile::new(path.clone(), text)
            .with_implied_node_formats(implied, implied_for_emit);
        if let Some((_, name)) = &traced {
            tsc_types::trace::mark(&format!("load: snapshot {name}"), prepare_started);
        }
        let plan_started = std::time::Instant::now();
        let planned = plan_source_requests_retaining_syntax(&prepared, options);
        if let Some((_, name)) = &traced {
            tsc_types::trace::mark(&format!("load: parse and plan {name}"), plan_started);
        }
        Box::new(match planned {
            Ok((plan, syntax)) => {
                if !plan.module_requests().is_empty() {
                    // The walk slices each request's text for its inclusion
                    // reason through the UTF-16 index: built here, on the
                    // worker, instead of on the loading thread per source.
                    let _ = prepared.snapshot().positions().utf16_len();
                }
                PrefetchedParse {
                    implied,
                    implied_for_emit,
                    prepared: prepared.with_preparsed_syntax(syntax),
                    plan: Ok(plan),
                }
            }
            Err(error) => PrefetchedParse {
                implied,
                implied_for_emit,
                prepared,
                plan: Err(error),
            },
        })
    });
    PrefetchedRead::Parsed { byte_len, decoded }
}

/// Why a library root is in the program (tsgo `fileIncludeKindLibFile`):
/// the default library of the target, or an entry of `lib`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LibraryRootReason {
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
    option_diagnostics: Vec<Diagnostic>,
    project_reference_redirects: Vec<(ProgramPath, usize)>,
    /// The package.json files the read-ahead workers' and the referenced
    /// projects' resolvers probed.
    package_json_probes: Vec<PackageJsonProbe>,
    /// The deduplicated package copies: path, owner, program index and the
    /// copy's own include reasons (see `PackageRedirectFile`).
    package_redirect_files: Vec<(ProgramPath, usize, usize, Vec<SourceInclusionReason>)>,
}

struct StagedGraph<'host, 'options, 'resolver> {
    host: &'host dyn CompilerHost,
    compiler_options: &'options CompilerOptions,
    program_options: &'options ProgramOptions,
    library_catalog: Option<&'options LibraryCatalog>,
    library_directory: Option<ProgramPath>,
    limits: ProgramLoadLimits,
    resolver: &'resolver mut ModuleResolver<'host>,
    library_resolver: Option<&'resolver mut ModuleResolver<'host>>,
    resolved_library_paths: BTreeMap<String, ProgramPath>,
    source_by_canonical: FxHashMap<CanonicalPath, usize>,
    files_by_name_ignore_case: FxHashMap<JsString, usize>,
    case_sensitive_casing_conflicts: Vec<CaseSensitiveCasingConflict>,
    sources: Vec<StagedSource>,
    /// tsgo filesParser: the parse tasks, their shared data by path, the
    /// root tasks in tsgo's order and the single-threaded work queue (a
    /// stack: the last queued task runs first).
    tasks: Vec<ParseTask>,
    task_data: Vec<TaskData>,
    task_data_by_path: FxHashMap<CanonicalPath, usize>,
    root_tasks: Vec<usize>,
    stack: Vec<(usize, usize)>,
    /// The missing-library diagnostics by path: a root of the same path
    /// reports that diagnostic instead of its own.
    missing_library_diagnostics: FxHashMap<JsString, Diagnostic>,
    /// The kept sources in program order, once the files are collected
    /// (`finish`); the diagnostics located by program order read it.
    program_order: Vec<usize>,
    roots: Vec<StagedRoot>,
    module_resolution_by_key: rustc_hash::FxHashMap<ResolutionKey, usize>,
    module_resolutions: Vec<StagedModuleResolution>,
    type_resolution_by_key: BTreeMap<TypeReferenceResolutionKey, usize>,
    type_resolutions: Vec<StagedTypeResolution>,
    diagnosed_missing_roots: HashSet<JsString>,
    diagnosed_missing_library_roots: HashSet<JsString>,
    program_diagnostics: Vec<Diagnostic>,
    request_edges: usize,
    total_source_bytes: usize,
    /// Roots parsed ahead of their sequential visit, by canonical path.
    prefetched: FxHashMap<CanonicalPath, PrefetchedSource>,
    /// The canonical paths of `prefetched` in root order; eviction under
    /// budget pressure drops the last (latest-visited) payload first.
    prefetch_order: Vec<CanonicalPath>,
    /// Retained read-ahead payloads not yet admitted: their count and bytes
    /// are held against the load limits together with the admitted sources
    /// (`sources.len()` / `total_source_bytes`).
    reserved_sources: usize,
    reserved_bytes: usize,
    /// Module resolutions computed by the dependency read-ahead
    /// (`prefetch_dependencies`) that the walk has not reached yet; the walk
    /// takes each one at the request that would have computed it.
    /// Hashed, never iterated except for the trace summary: the walk takes
    /// one entry per request (VS Code: 112k) and an ordered map compared the
    /// key's path strings at every step.
    pre_resolved: rustc_hash::FxHashMap<ResolutionKey, HostModuleResolution>,
    pre_resolved_hits: usize,
    /// Type-reference resolutions computed by the read-ahead that the walk
    /// has not reached yet, taken like `pre_resolved`.
    pre_resolved_types:
        BTreeMap<TypeReferenceResolutionKey, ResolutionOutcome<HostResolvedTypeReferenceDirective>>,
    /// One resolution per (containing directory, specifier, mode): tsc's
    /// perDirectoryResolutionCache. A module resolution depends on the
    /// containing file only through its directory (and the package scope
    /// that directory selects), so every file of a directory shares it.
    /// Hashed, never iterated: an ordered map compared the directory strings
    /// of every lookup (VS Code: 112k lookups of 60-byte paths).
    directory_resolutions:
        rustc_hash::FxHashMap<(JsString, JsString, ResolutionMode), HostModuleResolution>,
    directory_resolution_hits: usize,
    /// One resolver per referenced project whose output is in the program
    /// (tsgo GetCompilerOptionsWithRedirect: the module names of such an
    /// output resolve with its project's options), by the project's
    /// canonical config path.
    project_resolvers: BTreeMap<CanonicalPath, ModuleResolver<'options>>,
    /// The package.json files the read-ahead workers' resolvers probed (their
    /// resolvers are dropped with the workers).
    read_ahead_package_json_probes: Vec<PackageJsonProbe>,
    /// The sources of referenced projects loaded as their outputs: the
    /// source's path and the output's staged source.
    project_reference_redirects: Vec<(ProgramPath, usize)>,
}

/// Immutable and borrowed inputs for one staged graph. Keeping this boundary
/// typed avoids a positional constructor with unrelated resolver, option, and
/// resource arguments while preserving the loader's separate lifetimes.
struct StagedGraphConfig<'host, 'options, 'resolver> {
    host: &'host dyn CompilerHost,
    compiler_options: &'options CompilerOptions,
    program_options: &'options ProgramOptions,
    library_catalog: Option<&'options LibraryCatalog>,
    library_directory: Option<ProgramPath>,
    limits: ProgramLoadLimits,
    resolver: &'resolver mut ModuleResolver<'host>,
    library_resolver: Option<&'resolver mut ModuleResolver<'host>>,
}

impl<'host: 'options, 'options, 'resolver> StagedGraph<'host, 'options, 'resolver> {
    fn new(config: StagedGraphConfig<'host, 'options, 'resolver>) -> Self {
        Self {
            host: config.host,
            compiler_options: config.compiler_options,
            program_options: config.program_options,
            library_catalog: config.library_catalog,
            library_directory: config.library_directory,
            limits: config.limits,
            resolver: config.resolver,
            library_resolver: config.library_resolver,
            resolved_library_paths: BTreeMap::new(),
            source_by_canonical: FxHashMap::default(),
            files_by_name_ignore_case: FxHashMap::default(),
            case_sensitive_casing_conflicts: Vec::new(),
            sources: Vec::new(),
            tasks: Vec::new(),
            task_data: Vec::new(),
            task_data_by_path: FxHashMap::default(),
            root_tasks: Vec::new(),
            stack: Vec::new(),
            missing_library_diagnostics: FxHashMap::default(),
            program_order: Vec::new(),
            roots: Vec::new(),
            module_resolution_by_key: rustc_hash::FxHashMap::default(),
            module_resolutions: Vec::new(),
            type_resolution_by_key: BTreeMap::new(),
            type_resolutions: Vec::new(),
            diagnosed_missing_roots: HashSet::default(),
            diagnosed_missing_library_roots: HashSet::default(),
            program_diagnostics: Vec::new(),
            request_edges: 0,
            total_source_bytes: 0,
            prefetched: FxHashMap::default(),
            prefetch_order: Vec::new(),
            pre_resolved: rustc_hash::FxHashMap::default(),
            pre_resolved_hits: 0,
            pre_resolved_types: BTreeMap::new(),
            directory_resolutions: rustc_hash::FxHashMap::default(),
            directory_resolution_hits: 0,
            project_resolvers: BTreeMap::new(),
            read_ahead_package_json_probes: Vec::new(),
            project_reference_redirects: Vec::new(),
            reserved_sources: 0,
            reserved_bytes: 0,
        }
    }

    /// Read explicit roots ahead of the sequential graph walk and parse them
    /// on scoped standard-library worker threads.
    ///
    /// This runs only when the load's [`WorkerBudget`] is parallel and the
    /// host declares [`CompilerHost::permits_source_read_ahead`]: its reads
    /// are then pure and order-independent, so reading a root before the walk
    /// changes no observable host behavior. A read result that is retained
    /// (bytes, `Ok(None)`, `Err`) stands in for the host call at the root's
    /// original visit and is not re-read; results the walk never reaches are
    /// dropped.
    ///
    /// Resource bound: the source-count and byte limits are joint bounds over
    /// admitted sources and retained read-ahead payloads
    /// (`sources.len() + reserved_sources` and
    /// `total_source_bytes + reserved_bytes`). A read whose payload would
    /// exceed either bound, or the per-file limit, is never retained: the
    /// bytes are dropped after the host's one-call allocation and the visit
    /// reads the root again (equivalent under the purity contract), which
    /// keeps every limit error at its sequential position with its sequential
    /// observed value. Read-ahead stops at the first such root, at a host
    /// error (the walk fails at that root's visit at the latest, so a later
    /// root is only ever reached as a dependency of an earlier one, which the
    /// walk discovers and reads normally) and when the count bound is
    /// reached. When the walk later admits a
    /// source that was not read ahead (a dependency or a skipped root) and
    /// the joint bound would break, [`Self::evict_read_ahead_for_admission`]
    /// drops retained payloads from the tail of root order until it holds;
    /// those roots read fresh at their visit.
    ///
    /// Roots the tasks do not read through `admit_source` (extensionless,
    /// unsupported extension), JSON roots, roots whose implied module format
    /// needs the package scope, and duplicate or already-visited paths stay
    /// entirely on the sequential path. Hosts that keep the trait default and
    /// serial budgets see the unchanged sequential discovery.
    fn prefetch_roots(&mut self, roots: &[ProgramPath]) {
        let workers = self.limits.workers;
        if !workers.is_parallel() || !self.host.permits_source_read_ahead() {
            return;
        }
        let mut pending: Vec<ProgramPath> = Vec::new();
        let mut seen = BTreeSet::new();
        for path in roots {
            if !path_has_extension(path.display())
                || (!is_admitted_source(path.canonical(), self.compiler_options)
                    && self.compiler_options.allow_non_ts_extensions != Some(true))
                || is_json_source(path.canonical())
                || self.source_by_canonical.contains_key(path.canonical())
                || !seen.insert(path.canonical().clone())
            {
                continue;
            }
            pending.push(path.clone());
        }
        if pending.len() < 2 {
            return;
        }
        // A source whose implied module format depends on its package scope
        // (under NodeNext, every .ts source) is parsed against that scope:
        // looked up here, on the loading thread, from the resolver's
        // per-directory memo, exactly as its visit looks it up. A lookup
        // failure leaves the scope unknown; the visit then reports it.
        let mut pending: Vec<(ProgramPath, Option<PackageMetadata>)> = pending
            .into_iter()
            .map(|path| {
                let scope = if implied_node_format_needs_package_scope(
                    path.display(),
                    self.compiler_options,
                ) {
                    self.resolver
                        .package_scope_for_file(path.display())
                        .ok()
                        .flatten()
                } else {
                    None
                };
                (path, scope)
            })
            .collect();
        // The largest roots first: the longest parse then starts as early as
        // the streaming allows instead of after every smaller read before it
        // in root order. Hosts without a cheap size answer keep root order.
        let sizes: Vec<Option<u64>> = pending
            .iter()
            .map(|(path, _)| self.host.file_size_hint_js(path.display()).ok().flatten())
            .collect();
        if sizes.iter().any(Option::is_some) {
            let mut order: Vec<usize> = (0..pending.len()).collect();
            order.sort_by_key(|&index| (std::cmp::Reverse(sizes[index].unwrap_or(0)), index));
            pending = order
                .into_iter()
                .map(|index| pending[index].clone())
                .collect();
        }
        let phase_started = std::time::Instant::now();
        // Parallel reads pay off once the root list is long enough for the
        // loading thread's reads to be the critical path (256 roots: -4 ms
        // here); for a short list they only add concurrent syscalls to
        // reads that already overlap the parses under way (58 roots: +0.7 ms).
        if let Some(reader) = self
            .host
            .parallel_source_reader()
            .filter(|_| pending.len() >= PARALLEL_READ_AHEAD_MIN_ROOTS)
        {
            let reads = self.read_roots_ahead_in_parallel(pending, reader, workers);
            tsc_types::trace::mark("load: read-ahead streamed read and parse", phase_started);
            self.retain_read_ahead(reads);
            return;
        }
        // Host reads: loading thread, root order, each retained payload
        // reserved against the joint budget before the next read. Each
        // retained payload is handed to the parse workers as soon as it is
        // read, so the reads overlap the parses already running.
        let mut reads: Vec<(ProgramPath, Option<PrefetchedRead>)> =
            Vec::with_capacity(pending.len());
        let capacity = pending.len();
        let mut pending = pending.into_iter();
        let mut stopped = false;
        let options = self.compiler_options;
        let parsed = workers.map_streamed(
            capacity,
            |_| {
                if stopped {
                    return None;
                }
                loop {
                    let (path, scope) = pending.next()?;
                    if self.sources.len() + self.reserved_sources + 1 > self.limits.max_source_files
                    {
                        stopped = true;
                        return None;
                    }
                    match self.host.read_file_js(path.display()) {
                        Err(error) => {
                            // Reported at this root's visit, where the walk
                            // fails at the latest. A later root can still be
                            // admitted before that as a dependency of an
                            // earlier root; the walk then reads it itself, so
                            // stopping here is merely conservative.
                            reads.push((path, Some(PrefetchedRead::Failed(error))));
                            stopped = true;
                            return None;
                        }
                        Ok(None) => reads.push((path, Some(PrefetchedRead::Missing))),
                        Ok(Some(bytes)) => {
                            if bytes.len() > self.limits.max_source_file_bytes
                                || self
                                    .total_source_bytes
                                    .saturating_add(self.reserved_bytes)
                                    .saturating_add(bytes.len())
                                    > self.limits.max_total_source_bytes
                            {
                                // Not retained: dropped here, read again at
                                // the visit.
                                stopped = true;
                                return None;
                            }
                            self.reserved_sources += 1;
                            self.reserved_bytes += bytes.len();
                            let index = reads.len();
                            reads.push((path.clone(), None));
                            return Some((index, path, scope, bytes));
                        }
                    }
                }
            },
            |(index, path, scope, bytes)| {
                let large_parse_started = (tsc_types::trace::enabled() && bytes.len() >= 256 << 10)
                    .then(|| (std::time::Instant::now(), bytes.len()));
                let parsed = parse_root_ahead(&path, bytes, options, scope.as_ref());
                if let Some((started, len)) = large_parse_started {
                    tsc_types::trace::mark(
                        &format!("load: parse {:?} ({len} bytes)", path.display()),
                        started,
                    );
                }
                (index, parsed)
            },
        );
        tsc_types::trace::mark("load: read-ahead streamed read and parse", phase_started);
        for (index, read) in parsed {
            reads[index].1 = Some(read);
        }
        self.retain_read_ahead(
            reads
                .into_iter()
                .map(|(path, read)| {
                    (
                        path,
                        read.expect("every read-ahead root was read or parsed"),
                    )
                })
                .collect(),
        );
    }

    /// Read-ahead over a host that reads from any thread: every root is read
    /// and parsed on the worker that takes it, and the loading thread only
    /// hands out paths. Admission — the joint load limits and the first
    /// failed read, in root order — is then decided over the results exactly
    /// as the sequential reads decide it before each read, so the retained
    /// payloads and reservations are the same; a root after the stop was
    /// read but is dropped and read again at its visit, which the host's
    /// purity makes unobservable. Production stops early once the bytes read
    /// so far exceed the total budget, so the payloads in flight beyond it
    /// stay bounded by the worker count.
    fn read_roots_ahead_in_parallel(
        &mut self,
        pending: Vec<(ProgramPath, Option<PackageMetadata>)>,
        reader: &(dyn ParallelSourceReader + Sync),
        workers: WorkerBudget,
    ) -> Vec<(ProgramPath, PrefetchedRead)> {
        let options = self.compiler_options;
        let max_source_file_bytes = self.limits.max_source_file_bytes;
        let max_source_files = self.limits.max_source_files;
        let max_total_source_bytes = self.limits.max_total_source_bytes;
        let base_sources = self.sources.len() + self.reserved_sources;
        let base_bytes = self.total_source_bytes.saturating_add(self.reserved_bytes);
        let read_bytes = AtomicUsize::new(0);
        let capacity = pending.len();
        let mut pending = pending.into_iter();
        let results = workers.map_streamed(
            capacity,
            |produced| {
                if base_sources + produced + 1 > max_source_files
                    || base_bytes.saturating_add(read_bytes.load(Ordering::Relaxed))
                        > max_total_source_bytes
                {
                    return None;
                }
                pending.next()
            },
            |(path, scope)| {
                let read = match reader.read_source_js(path.display()) {
                    Err(error) => Some(PrefetchedRead::Failed(error)),
                    Ok(None) => Some(PrefetchedRead::Missing),
                    Ok(Some(bytes)) => {
                        read_bytes.fetch_add(bytes.len(), Ordering::Relaxed);
                        if bytes.len() > max_source_file_bytes {
                            // Not retained: read again at the visit.
                            None
                        } else {
                            let large_parse_started = (tsc_types::trace::enabled()
                                && bytes.len() >= 256 << 10)
                                .then(|| (std::time::Instant::now(), bytes.len()));
                            let parsed = parse_root_ahead(&path, bytes, options, scope.as_ref());
                            if let Some((started, len)) = large_parse_started {
                                tsc_types::trace::mark(
                                    &format!("load: parse {:?} ({len} bytes)", path.display()),
                                    started,
                                );
                            }
                            Some(parsed)
                        }
                    }
                };
                (path, read)
            },
        );
        let mut reads = Vec::with_capacity(results.len());
        for (path, read) in results {
            if self.sources.len() + self.reserved_sources + 1 > max_source_files {
                break;
            }
            match read {
                None => break,
                Some(PrefetchedRead::Failed(error)) => {
                    reads.push((path, PrefetchedRead::Failed(error)));
                    break;
                }
                Some(PrefetchedRead::Missing) => reads.push((path, PrefetchedRead::Missing)),
                Some(read @ PrefetchedRead::Parsed { .. }) => {
                    let byte_len = read.reserved_bytes().unwrap_or(0);
                    if self
                        .total_source_bytes
                        .saturating_add(self.reserved_bytes)
                        .saturating_add(byte_len)
                        > max_total_source_bytes
                    {
                        break;
                    }
                    self.reserved_sources += 1;
                    self.reserved_bytes += byte_len;
                    reads.push((path, read));
                }
            }
        }
        reads
    }

    /// Record the read-ahead results, in root order, for the visits.
    fn retain_read_ahead(&mut self, reads: Vec<(ProgramPath, PrefetchedRead)>) {
        for (path, read) in reads {
            self.prefetch_order.push(path.canonical().clone());
            self.prefetched.insert(
                path.canonical().clone(),
                PrefetchedSource {
                    display: path.display().to_owned(),
                    read,
                },
            );
        }
    }

    /// The retained read-ahead result for a visit under `path`, if the
    /// read-ahead queried this exact display spelling. Another spelling of
    /// the same canonical path is a distinct host query: its entry is
    /// discarded and the visit reads as before. Either way the entry's
    /// reservation is released; the visit's own admission accounts for it.
    /// tsrs-native: read ahead the dependencies of the read-ahead roots.
    ///
    /// Resolve every module request of the sources read ahead so far (the
    /// walk takes these resolutions in its own order, see `pre_resolved`),
    /// read and parse their loadable targets on the workers as
    /// `prefetch_roots` does, and repeat with the targets' own requests,
    /// along with the reference edges (path, lib and type directives) the
    /// walk follows. The sequential walk keeps every decision it makes
    /// (visit order, unloaded reasons, diagnostics, limits); this only
    /// answers its reads and resolutions from memory. A request whose
    /// resolution fails here is left for the walk to resolve and report in
    /// order.
    ///
    /// Over a host that resolves and reads on several threads the work is
    /// one pipeline: a parsed source's requests resolve while other sources
    /// are still being read, and a resolved target is read as soon as it is
    /// known, so no level barrier waits for the largest parse in progress.
    /// Otherwise the same edges are followed level by level.
    fn prefetch_dependencies(&mut self) {
        let workers = self.limits.workers;
        let host_ref = self.host;
        if workers.is_parallel() && host_ref.permits_source_read_ahead() {
            if let (Some(host), Some(reader)) = (
                host_ref.parallel_resolution_host(),
                host_ref.parallel_source_reader(),
            ) {
                self.prefetch_dependencies_pipelined(host, reader, workers);
                return;
            }
        }
        self.prefetch_dependencies_by_level();
    }

    fn prefetch_dependencies_by_level(&mut self) {
        const MAX_LEVELS: usize = 64;
        let mut frontier: Vec<CanonicalPath> = self.prefetch_order.clone();
        for level in 0..MAX_LEVELS {
            if frontier.is_empty() {
                break;
            }
            let level_started = std::time::Instant::now();
            let mut requests: Vec<(JsString, ResolutionKey, bool)> = Vec::new();
            let mut targets: Vec<ProgramPath> = Vec::new();
            let mut queued: BTreeSet<CanonicalPath> = BTreeSet::new();
            for canonical in &frontier {
                let (edges, references) = self.read_ahead_edges(canonical);
                requests.extend(edges);
                for target in references {
                    self.queue_read_ahead_target(&mut targets, &mut queued, &target);
                }
            }
            let request_count = requests.len();
            let resolved = self.resolve_requests_ahead_by_directory(requests);
            for (key, loads_source, host) in resolved {
                let Some(host) = host else {
                    continue;
                };
                if self.pre_resolved.contains_key(&key) {
                    continue;
                }
                if loads_source {
                    if let ResolutionOutcome::Resolved(target) = host.outcome() {
                        if !matches!(target.extension(), ModuleExtension::Json)
                            && !self.read_ahead_elides_target(target)
                        {
                            self.queue_read_ahead_target(
                                &mut targets,
                                &mut queued,
                                target.resolved_file(),
                            );
                        }
                    }
                }
                self.pre_resolved.insert(key, host);
            }
            if targets.is_empty() {
                tsc_types::trace::mark(
                    &format!(
                        "load: dependency level {} ({} sources, {} requests, no new targets, {} pre-resolved)",
                        level,
                        frontier.len(),
                        request_count,
                        self.pre_resolved.len()
                    ),
                    level_started,
                );
                break;
            }
            let before = self.prefetch_order.len();
            self.prefetch_roots(&targets);
            frontier = self.prefetch_order[before..].to_vec();
            tsc_types::trace::mark(
                &format!(
                    "load: dependency level {} ({} requests, {} targets, {} read ahead, {} pre-resolved)",
                    level,
                    request_count,
                    targets.len(),
                    frontier.len(),
                    self.pre_resolved.len()
                ),
                level_started,
            );
        }
    }

    /// The edges of a source read ahead: its module requests not resolved
    /// yet (containing file, key, loadability) and the reference targets
    /// (path, lib and type directives, computed as the walk computes them)
    /// the walk will visit.
    fn read_ahead_edges(
        &mut self,
        canonical: &CanonicalPath,
    ) -> (Vec<(JsString, ResolutionKey, bool)>, Vec<ProgramPath>) {
        let mut requests = Vec::new();
        let mut path_references: Vec<PlannedPathReference> = Vec::new();
        let mut lib_references: Vec<PlannedLibReferenceDirective> = Vec::new();
        let mut type_references: Vec<PlannedTypeReferenceDirective> = Vec::new();
        // A referenced project's source is read ahead in vain: its output is
        // what the walk loads, and the output is read when it is reached.
        if self
            .project_reference_redirect(canonical)
            .ok()
            .flatten()
            .is_some()
        {
            return (requests, Vec::new());
        }
        let containing_path = {
            let Some(entry) = self.prefetched.get(canonical) else {
                return (requests, Vec::new());
            };
            let PrefetchedRead::Parsed {
                decoded: Ok(parse), ..
            } = &entry.read
            else {
                return (requests, Vec::new());
            };
            let Ok(plan) = &parse.plan else {
                return (requests, Vec::new());
            };
            let containing_file = parse.prepared.path().display().to_owned();
            // The output of a referenced project resolves with its project's
            // options on the walk, not with the read-ahead's resolver.
            let resolved_ahead = self.project_for_resolution(canonical).is_none();
            for (key, loads_source) in plan.module_requests_with_loadability() {
                if !resolved_ahead
                    || self.module_resolution_by_key.contains_key(key)
                    || self.pre_resolved.contains_key(key)
                {
                    continue;
                }
                requests.push((containing_file.clone(), key.clone(), loads_source));
            }
            if self.compiler_options.no_resolve != Some(true) {
                path_references.extend(plan.path_references().iter().cloned());
                type_references.extend(plan.type_reference_directives().iter().cloned());
            }
            if self.program_options.no_lib() != Some(true) {
                lib_references.extend(plan.lib_reference_directives().iter().cloned());
            }
            parse.prepared.path().clone()
        };
        let mut targets = Vec::new();
        for reference in &path_references {
            if let Some(target) =
                self.path_reference_target_ahead(containing_path.display(), reference)
            {
                targets.push(target);
            }
        }
        for directive in &lib_references {
            if let Some(target) = self.lib_reference_target_ahead(directive) {
                targets.push(target);
            }
        }
        for directive in &type_references {
            if let Some(target) =
                self.type_reference_target_ahead(&containing_path, directive.key())
            {
                targets.push(target);
            }
        }
        (requests, targets)
    }

    /// The pipelined form of the dependency read-ahead: `threads` workers
    /// take resolution and read-and-parse tasks from one queue (each with
    /// its own resolver, whose package memo stays warm for the whole
    /// read-ahead) and hand every outcome to this thread, which records it
    /// as the level form does and queues the work it uncovers.
    fn prefetch_dependencies_pipelined(
        &mut self,
        host: &(dyn CompilerHost + Sync),
        reader: &(dyn ParallelSourceReader + Sync),
        workers: WorkerBudget,
    ) {
        let phase_started = std::time::Instant::now();
        let options = self.compiler_options;
        let program_options = self.program_options;
        let max_source_file_bytes = self.limits.max_source_file_bytes;
        let pipeline = ReadAheadPipeline {
            queue: std::sync::Mutex::new(ReadAheadQueue {
                pending: VecDeque::new(),
                closed: false,
                waiting: 0,
            }),
            ready: std::sync::Condvar::new(),
        };
        let (sender, receiver) = std::sync::mpsc::channel::<ReadAheadOutcome>();
        let run = |sender: std::sync::mpsc::Sender<ReadAheadOutcome>| {
            let mut resolver =
                ModuleResolver::new_with_program_options(host, options, program_options).ok();
            while let Some(task) = pipeline.take() {
                let outcome = match task {
                    ReadAheadTask::Resolve {
                        containing_file,
                        key,
                        loads_source,
                    } => {
                        let host = resolver.as_mut().and_then(|resolver| {
                            resolver
                                .resolve_with_facts(&containing_file, key.specifier(), key.mode())
                                .ok()
                        });
                        ReadAheadOutcome::Resolved {
                            key,
                            loads_source,
                            host: host.map(Box::new),
                        }
                    }
                    ReadAheadTask::Read { path, scope } => {
                        let read = match reader.read_source_js(path.display()) {
                            Err(error) => Some(PrefetchedRead::Failed(error)),
                            Ok(None) => Some(PrefetchedRead::Missing),
                            Ok(Some(bytes)) => {
                                if bytes.len() > max_source_file_bytes {
                                    // Not retained: read again at the visit.
                                    None
                                } else {
                                    let large_parse_started = (tsc_types::trace::enabled()
                                        && bytes.len() >= 256 << 10)
                                        .then(|| (std::time::Instant::now(), bytes.len()));
                                    let parsed =
                                        parse_root_ahead(&path, bytes, options, scope.as_ref());
                                    if let Some((started, len)) = large_parse_started {
                                        tsc_types::trace::mark(
                                            &format!(
                                                "load: parse {:?} ({len} bytes)",
                                                path.display()
                                            ),
                                            started,
                                        );
                                    }
                                    Some(parsed)
                                }
                            }
                        };
                        ReadAheadOutcome::Read { path, read }
                    }
                };
                if sender.send(outcome).is_err() {
                    break;
                }
            }
            if let Some(resolver) = resolver.as_mut() {
                let _ = sender.send(ReadAheadOutcome::Probes(
                    resolver.take_package_json_probes(),
                ));
            }
        };
        let threads = workers.max_workers();
        let mut state = ReadAheadState::default();
        std::thread::scope(|scope| {
            for _ in 0..threads {
                let sender = sender.clone();
                // A refused thread is not an error: the threads that started
                // finish the work.
                let _ = std::thread::Builder::new()
                    .name("tsc-rs-worker".to_owned())
                    .stack_size(crate::workers::WORKER_STACK_BYTES)
                    .spawn_scoped(scope, move || {
                        crate::workers::run_thread_start_hook();
                        run(sender);
                    });
            }
            drop(sender);
            for canonical in self.prefetch_order.clone() {
                self.pipeline_follow_source(&canonical, &pipeline, &mut state);
            }
            while state.outstanding > 0 {
                // Every sender is a live worker; none left means the work
                // cannot complete (no thread could be started).
                let Ok(outcome) = receiver.recv() else {
                    break;
                };
                match outcome {
                    ReadAheadOutcome::Resolved {
                        key,
                        loads_source,
                        host,
                    } => {
                        state.outstanding -= 1;
                        self.pipeline_apply_resolution(
                            key,
                            loads_source,
                            host.map(|host| *host),
                            &pipeline,
                            &mut state,
                        );
                    }
                    ReadAheadOutcome::Read { path, read } => {
                        state.outstanding -= 1;
                        self.pipeline_retain_read(path, read, &pipeline, &mut state);
                    }
                    ReadAheadOutcome::Probes(probes) => {
                        self.read_ahead_package_json_probes.extend(probes);
                    }
                }
            }
            pipeline.close();
        });
        // The workers sent their probes after the pipeline closed.
        while let Ok(outcome) = receiver.try_recv() {
            if let ReadAheadOutcome::Probes(probes) = outcome {
                self.read_ahead_package_json_probes.extend(probes);
            }
        }
        tsc_types::trace::mark(
            &format!(
                "load: read-ahead of dependencies (pipelined on {threads} threads: {} resolutions, {} directory reuses, {} reads, {} pre-resolved)",
                state.resolutions,
                state.directory_reuses,
                state.reads,
                self.pre_resolved.len()
            ),
            phase_started,
        );
    }

    /// Queue what a source read ahead leads to: its module requests (one
    /// resolution per containing directory, specifier and mode, the others
    /// waiting for it) and its reference targets.
    fn pipeline_follow_source(
        &mut self,
        canonical: &CanonicalPath,
        pipeline: &ReadAheadPipeline,
        state: &mut ReadAheadState,
    ) {
        let (requests, targets) = self.read_ahead_edges(canonical);
        // The requests of one source share its directory: derive it once
        // per containing file instead of per request (VS Code: 57k requests).
        let mut last_directory: Option<(JsString, JsString)> = None;
        for (containing_file, key, loads_source) in requests {
            let directory = match &last_directory {
                Some((file, directory)) if *file == containing_file => directory.clone(),
                _ => {
                    let directory = crate::js_path::directory_name(containing_file.as_js());
                    last_directory = Some((containing_file.clone(), directory.clone()));
                    directory
                }
            };
            let directory_key = (directory, key.specifier().to_owned(), key.mode());
            if let Some(host) = self.directory_resolutions.get(&directory_key) {
                self.directory_resolution_hits += 1;
                state.directory_reuses += 1;
                let host = host.clone();
                self.pipeline_record_resolution(key, loads_source, host, pipeline, state);
                continue;
            }
            match state.waiting.entry(directory_key) {
                std::collections::hash_map::Entry::Occupied(mut waiting) => {
                    self.directory_resolution_hits += 1;
                    state.directory_reuses += 1;
                    waiting.get_mut().push((key, loads_source));
                }
                std::collections::hash_map::Entry::Vacant(waiting) => {
                    waiting.insert(Vec::new());
                    state.outstanding += 1;
                    state.resolutions += 1;
                    pipeline.push(ReadAheadTask::Resolve {
                        containing_file,
                        key,
                        loads_source,
                    });
                }
            }
        }
        for target in targets {
            self.pipeline_queue_read(target, pipeline, state);
        }
    }

    /// A resolution outcome from a worker: memoized for its directory and
    /// recorded for its request and every request that waited for it.
    fn pipeline_apply_resolution(
        &mut self,
        key: ResolutionKey,
        loads_source: bool,
        host: Option<HostModuleResolution>,
        pipeline: &ReadAheadPipeline,
        state: &mut ReadAheadState,
    ) {
        let directory_key = (
            crate::js_path::directory_name(self.pipeline_containing_file(&key).as_js()),
            key.specifier().to_owned(),
            key.mode(),
        );
        let waiting = state.waiting.remove(&directory_key).unwrap_or_default();
        let Some(host) = host else {
            // Left for the walk to resolve and report, like the waiters.
            return;
        };
        self.directory_resolutions
            .insert(directory_key, host.clone());
        for (key, loads_source) in waiting {
            self.pipeline_record_resolution(key, loads_source, host.clone(), pipeline, state);
        }
        self.pipeline_record_resolution(key, loads_source, host, pipeline, state);
    }

    /// The display spelling of a request's containing file: the request's
    /// source is a read-ahead entry (its requests were planned from it).
    fn pipeline_containing_file(&self, key: &ResolutionKey) -> JsString {
        self.prefetched
            .get(key.source())
            .map(|entry| entry.display.clone())
            .unwrap_or_else(|| key.source().as_js().to_owned())
    }

    fn pipeline_record_resolution(
        &mut self,
        key: ResolutionKey,
        loads_source: bool,
        host: HostModuleResolution,
        pipeline: &ReadAheadPipeline,
        state: &mut ReadAheadState,
    ) {
        if self.pre_resolved.contains_key(&key) {
            return;
        }
        if loads_source {
            if let ResolutionOutcome::Resolved(target) = host.outcome() {
                if !matches!(target.extension(), ModuleExtension::Json)
                    && !self.read_ahead_elides_target(target)
                {
                    self.pipeline_queue_read(target.resolved_file().clone(), pipeline, state);
                }
            }
        }
        self.pre_resolved.insert(key, host);
    }

    /// Whether the walk elides a JavaScript target found through a
    /// node_modules search at every depth (processImportedModules'
    /// maxNodeModuleJsDepth rule at the shallowest depth, 1): reading it
    /// ahead would be wasted. A deeper visit elides at least as much.
    fn read_ahead_elides_target(
        &self,
        target: &crate::module_resolution::HostResolvedModule,
    ) -> bool {
        target.extension().is_javascript()
            && target.is_external_library_import()
            && (target.original_path().is_none()
                || path_contains_node_modules(target.resolved_file().canonical().as_js()))
            && self.compiler_options.node_modules_depth_exceeds_limit(1)
    }

    /// Queue a target's read-and-parse under the same admission as
    /// `prefetch_roots`, with the package scope its parse needs looked up
    /// here as the visit looks it up.
    fn pipeline_queue_read(
        &mut self,
        path: ProgramPath,
        pipeline: &ReadAheadPipeline,
        state: &mut ReadAheadState,
    ) {
        if state.stopped
            || !path_has_extension(path.display())
            || (!is_admitted_source(path.canonical(), self.compiler_options)
                && self.compiler_options.allow_non_ts_extensions != Some(true))
            || is_json_source(path.canonical())
            || self.source_by_canonical.contains_key(path.canonical())
            || self.prefetched.contains_key(path.canonical())
            || !state.queued.insert(path.canonical().clone())
        {
            return;
        }
        if self.sources.len() + self.reserved_sources + state.reads_in_flight + 1
            > self.limits.max_source_files
        {
            state.stopped = true;
            return;
        }
        let scope =
            if implied_node_format_needs_package_scope(path.display(), self.compiler_options) {
                self.resolver
                    .package_scope_for_file(path.display())
                    .ok()
                    .flatten()
            } else {
                None
            };
        state.outstanding += 1;
        state.reads += 1;
        state.reads_in_flight += 1;
        pipeline.push(ReadAheadTask::Read { path, scope });
    }

    /// A read outcome from a worker: retained under the load budget as
    /// `read_roots_ahead_in_parallel` retains a batch, then followed.
    fn pipeline_retain_read(
        &mut self,
        path: ProgramPath,
        read: Option<PrefetchedRead>,
        pipeline: &ReadAheadPipeline,
        state: &mut ReadAheadState,
    ) {
        state.reads_in_flight -= 1;
        if state.stopped {
            return;
        }
        let Some(read) = read else {
            // Over the single-source limit: read again at the visit.
            return;
        };
        if self.sources.len() + self.reserved_sources + 1 > self.limits.max_source_files {
            state.stopped = true;
            return;
        }
        match read {
            PrefetchedRead::Failed(error) => {
                // The visit reports this error; nothing after it is needed.
                state.stopped = true;
                self.retain_read_ahead(vec![(path, PrefetchedRead::Failed(error))]);
            }
            PrefetchedRead::Missing => {
                self.retain_read_ahead(vec![(path, PrefetchedRead::Missing)])
            }
            read @ PrefetchedRead::Parsed { .. } => {
                let byte_len = read.reserved_bytes().unwrap_or(0);
                if self
                    .total_source_bytes
                    .saturating_add(self.reserved_bytes)
                    .saturating_add(byte_len)
                    > self.limits.max_total_source_bytes
                {
                    state.stopped = true;
                    return;
                }
                self.reserved_sources += 1;
                self.reserved_bytes += byte_len;
                let canonical = path.canonical().clone();
                self.retain_read_ahead(vec![(path, read)]);
                self.pipeline_follow_source(&canonical, pipeline, state);
            }
        }
    }

    fn queue_read_ahead_target(
        &self,
        targets: &mut Vec<ProgramPath>,
        queued: &mut BTreeSet<CanonicalPath>,
        path: &ProgramPath,
    ) {
        if !self.source_by_canonical.contains_key(path.canonical())
            && !self.prefetched.contains_key(path.canonical())
            && queued.insert(path.canonical().clone())
        {
            targets.push(path.clone());
        }
    }

    /// The files the walk loads after the roots: the selected libraries
    /// (their lib references chain on through the dependency levels) and the
    /// automatic type directives' targets, resolved as the walk resolves them
    /// (it takes each resolution from `pre_resolved_types`).
    fn read_ahead_seeds(&mut self) -> Vec<ProgramPath> {
        let mut seeds = Vec::new();
        if self.program_options.no_lib() != Some(true) {
            if let Some(catalog) = self.library_catalog {
                let selected: Vec<String> = match self.compiler_options.lib.as_deref() {
                    Some(libraries) => libraries
                        .iter()
                        .filter_map(|value| catalog.option_file_name(value))
                        .map(str::to_owned)
                        .collect(),
                    None => vec![self
                        .program_options
                        .default_library_file_name()
                        .unwrap_or_else(|| catalog.default_file_name(self.compiler_options))
                        .to_owned()],
                };
                for file_name in selected {
                    if let Ok(path) = self.resolved_library_path(&file_name) {
                        seeds.push(path);
                    }
                }
            }
        }
        if let Ok((names, _)) = self.automatic_type_directive_names() {
            if !names.is_empty() {
                if let Ok(containing_file) = self.automatic_types_containing_file() {
                    for name in names {
                        let key = TypeReferenceResolutionKey::automatic(
                            containing_file.canonical().clone(),
                            name,
                        );
                        if let Some(target) =
                            self.type_reference_target_ahead(&containing_file, &key)
                        {
                            seeds.push(target);
                        }
                    }
                }
            }
        }
        seeds
    }

    /// The path a `/// <reference path>` names, as `process_path_reference`
    /// computes it, when the walk would load it.
    fn path_reference_target_ahead(
        &self,
        containing_file: JsStr<'_>,
        reference: &PlannedPathReference,
    ) -> Option<ProgramPath> {
        if reference.file_name().is_empty() {
            return None;
        }
        let base = crate::js_path::directory_name(containing_file);
        let normalized = crate::module_resolution::normalize_absolute_js_path(
            reference.file_name(),
            Some(base.as_js()),
            true,
        )
        .ok()?;
        let reference_path = crate::js_path::normalize_slashes(reference.file_name());
        let has_extension = reference_path
            .as_js()
            .split_ascii(b'/')
            .next_back()
            .is_some_and(|name| name.contains("."));
        if !has_extension {
            return None;
        }
        let target = make_program_path(
            &normalized,
            self.resolver.path_context().use_case_sensitive_file_names(),
        )
        .ok()?;
        (is_typescript_source(target.canonical())
            || is_javascript_source(target.canonical()) && self.compiler_options.allow_js)
            .then_some(target)
    }

    /// The library file a `/// <reference lib>` names, as
    /// `process_lib_references` resolves it.
    fn lib_reference_target_ahead(
        &mut self,
        directive: &PlannedLibReferenceDirective,
    ) -> Option<ProgramPath> {
        let catalog = self.library_catalog?;
        let lib_name = to_file_name_lower_case_js(directive.file_name());
        let file_name = catalog.reference_file_name(&lib_name)?;
        self.resolved_library_path(file_name).ok()
    }

    /// Resolve a type reference directive ahead of the walk (which takes the
    /// resolution from `pre_resolved_types`) and name its loadable target.
    fn type_reference_target_ahead(
        &mut self,
        containing_file: &ProgramPath,
        key: &TypeReferenceResolutionKey,
    ) -> Option<ProgramPath> {
        if self.type_resolution_by_key.contains_key(key)
            || self.pre_resolved_types.contains_key(key)
        {
            return None;
        }
        let type_roots = self.program_options.type_roots().map(<[_]>::to_vec);
        let host = self
            .resolver
            .resolve_type_reference(
                containing_file.display(),
                key.specifier(),
                key.mode(),
                type_roots.as_deref(),
            )
            .ok()?;
        let target = match &host {
            ResolutionOutcome::Resolved(target)
                if is_loadable_typescript_extension(target.extension()) =>
            {
                Some(target.resolved_file().clone())
            }
            _ => None,
        };
        self.pre_resolved_types.insert(key.clone(), host);
        target
    }

    /// [`Self::resolve_requests_ahead`] behind the per-directory memo: one
    /// resolution per (containing directory, specifier, mode), reused for
    /// every request that shares them, now and in the walk.
    fn resolve_requests_ahead_by_directory(
        &mut self,
        requests: Vec<(JsString, ResolutionKey, bool)>,
    ) -> Vec<(ResolutionKey, bool, Option<HostModuleResolution>)> {
        let mut unique: Vec<(JsString, ResolutionKey, bool)> = Vec::new();
        let mut by_directory: BTreeMap<(JsString, JsString, ResolutionMode), usize> =
            BTreeMap::new();
        let mut planned: Vec<(ResolutionKey, bool, Result<HostModuleResolution, usize>)> =
            Vec::with_capacity(requests.len());
        for (containing_file, key, loads_source) in requests {
            let directory_key = (
                crate::js_path::directory_name(containing_file.as_js()),
                key.specifier().to_owned(),
                key.mode(),
            );
            if let Some(host) = self.directory_resolutions.get(&directory_key) {
                self.directory_resolution_hits += 1;
                planned.push((key, loads_source, Ok(host.clone())));
                continue;
            }
            let index = match by_directory.get(&directory_key) {
                Some(&index) => {
                    self.directory_resolution_hits += 1;
                    index
                }
                None => {
                    let index = unique.len();
                    unique.push((containing_file, key.clone(), loads_source));
                    by_directory.insert(directory_key, index);
                    index
                }
            };
            planned.push((key, loads_source, Err(index)));
        }
        let resolved = self.resolve_requests_ahead(unique);
        for (directory_key, index) in by_directory {
            if let Some(host) = &resolved[index].2 {
                self.directory_resolutions
                    .insert(directory_key, host.clone());
            }
        }
        planned
            .into_iter()
            .map(|(key, loads_source, lookup)| match lookup {
                Ok(host) => (key, loads_source, Some(host)),
                Err(index) => (key, loads_source, resolved[index].2.clone()),
            })
            .collect()
    }

    /// Resolve the read-ahead's requests, in request order: on the workers
    /// when the host can be shared across threads (each thread constructs
    /// its own resolver over it; a resolution is a pure observation of the
    /// host, so every result is the one the walk's resolver would compute),
    /// otherwise on the loading thread. A failed resolution is `None` and
    /// is left for the walk to resolve and report.
    fn resolve_requests_ahead(
        &mut self,
        requests: Vec<(JsString, ResolutionKey, bool)>,
    ) -> Vec<(ResolutionKey, bool, Option<HostModuleResolution>)> {
        // Spawning the workers costs tens of microseconds; a level of a few
        // dozen requests already resolves faster on them than in sequence.
        const MIN_PARALLEL_REQUESTS: usize = 32;
        let workers = self.limits.workers;
        if let Some(host) = self
            .host
            .parallel_resolution_host()
            .filter(|_| workers.is_parallel() && requests.len() >= MIN_PARALLEL_REQUESTS)
        {
            let threads = workers.max_workers().clamp(1, requests.len());
            let chunk_size = requests.len().div_ceil(threads);
            let resolve_started = std::time::Instant::now();
            let request_count = requests.len();
            let options = self.compiler_options;
            let program_options = self.program_options;
            let resolve_chunk = |chunk: &[(JsString, ResolutionKey, bool)]| {
                let host: &dyn CompilerHost = host;
                let mut resolver =
                    ModuleResolver::new_with_program_options(host, options, program_options).ok();
                let resolved = chunk
                    .iter()
                    .map(|(containing_file, key, loads_source)| {
                        let result = resolver.as_mut().and_then(|resolver| {
                            resolver
                                .resolve_with_facts(containing_file, key.specifier(), key.mode())
                                .ok()
                        });
                        (key.clone(), *loads_source, result)
                    })
                    .collect::<Vec<_>>();
                let probes = resolver
                    .as_mut()
                    .map(ModuleResolver::take_package_json_probes)
                    .unwrap_or_default();
                (resolved, probes)
            };
            let results = std::thread::scope(|scope| {
                let handles = requests
                    .chunks(chunk_size)
                    .map(|chunk| {
                        std::thread::Builder::new()
                            .name("tsc-rs-worker".to_owned())
                            .stack_size(crate::workers::WORKER_STACK_BYTES)
                            .spawn_scoped(scope, || resolve_chunk(chunk))
                    })
                    .collect::<Vec<_>>();
                handles
                    .into_iter()
                    .zip(requests.chunks(chunk_size))
                    .map(|(handle, chunk)| match handle {
                        Ok(handle) => handle.join().expect("a resolution worker panicked"),
                        // The OS refused the thread: its chunk runs here.
                        Err(_) => resolve_chunk(chunk),
                    })
                    .collect::<Vec<_>>()
            });
            tsc_types::trace::mark(
                &format!(
                    "load: read-ahead resolution ({request_count} requests on {threads} threads)"
                ),
                resolve_started,
            );
            let mut resolved = Vec::with_capacity(request_count);
            for (chunk, probes) in results {
                resolved.extend(chunk);
                self.read_ahead_package_json_probes.extend(probes);
            }
            return resolved;
        }
        let resolve_started = std::time::Instant::now();
        let request_count = requests.len();
        let resolved = requests
            .into_iter()
            .map(|(containing_file, key, loads_source)| {
                let result = self
                    .resolver
                    .resolve_with_facts(&containing_file, key.specifier(), key.mode())
                    .ok();
                (key, loads_source, result)
            })
            .collect();
        tsc_types::trace::mark(
            &format!("load: read-ahead resolution ({request_count} requests, loading thread)"),
            resolve_started,
        );
        resolved
    }

    fn take_prefetched(&mut self, path: &ProgramPath) -> Option<PrefetchedRead> {
        let prefetched = self.prefetched.remove(path.canonical())?;
        self.release_read_ahead_reservation(&prefetched.read);
        (prefetched.display.as_js() == path.display()).then_some(prefetched.read)
    }

    fn release_read_ahead_reservation(&mut self, read: &PrefetchedRead) {
        if let Some(byte_len) = read.reserved_bytes() {
            self.reserved_sources = self.reserved_sources.saturating_sub(1);
            self.reserved_bytes = self.reserved_bytes.saturating_sub(byte_len);
        }
    }

    /// Keep the joint bound over admitted and retained sources when the
    /// walk is about to admit `byte_len` more bytes of a source that its own
    /// limit checks have already accepted: drop retained read-ahead payloads
    /// from the tail of root order until `admitted + 1 + retained` fits the
    /// source-count limit and the bytes fit the total-byte limit. Dropped
    /// roots are read from the (pure) host again at their visit.
    fn evict_read_ahead_for_admission(&mut self, byte_len: usize) {
        while self.reserved_sources > 0
            && (self.sources.len() + 1 + self.reserved_sources > self.limits.max_source_files
                || self
                    .total_source_bytes
                    .saturating_add(byte_len)
                    .saturating_add(self.reserved_bytes)
                    > self.limits.max_total_source_bytes)
        {
            let Some(canonical) = self.prefetch_order.pop() else {
                break;
            };
            let holds_payload = self
                .prefetched
                .get(&canonical)
                .is_some_and(|entry| entry.read.reserved_bytes().is_some());
            if holds_payload {
                let entry = self
                    .prefetched
                    .remove(&canonical)
                    .expect("entry was just observed");
                self.release_read_ahead_reservation(&entry.read);
            }
        }
    }

    fn load_root(
        &mut self,
        path: ProgramPath,
        root_spelling: JsStr<'_>,
        root_reason: RootFileReason,
    ) -> Result<(), ProgramLoadError> {
        if !path_has_extension(path.display()) {
            return self.load_extensionless_root(path, root_spelling, root_reason);
        }
        // tsc getSourceFileFromReferenceWorker (_tsc.js:124176): the
        // supported-extension check is skipped entirely under
        // allowNonTsExtensions; the script kind then derives from the name.
        if !is_admitted_source(path.canonical(), self.compiler_options)
            && self.compiler_options.allow_non_ts_extensions != Some(true)
        {
            let diagnostic = unsupported_root_extension_diagnostic(
                &path,
                root_spelling,
                self.compiler_options.allow_js,
                root_reason,
            )?;
            if self
                .diagnosed_missing_roots
                .insert(path.display().to_owned())
            {
                self.program_diagnostics.push(diagnostic.clone());
            }
            self.roots.push(StagedRoot {
                path,
                source: None,
                missing_diagnostic: Some(diagnostic),
                task: None,
            });
            return Ok(());
        }
        let task = self.new_root_task(path.clone(), root_spelling, root_reason);
        self.roots.push(StagedRoot {
            path,
            source: None,
            missing_diagnostic: None,
            task: Some(task),
        });
        Ok(())
    }

    /// tsgo addRootFileTask: a root task with its spelling and reason; the
    /// missing-root diagnostic is reported once the tasks ran.
    fn new_root_task(
        &mut self,
        path: ProgramPath,
        root_spelling: JsStr<'_>,
        root_reason: RootFileReason,
    ) -> usize {
        let task = self.new_task(
            path,
            TaskOrigin::Root {
                spelling: root_spelling.to_owned(),
                reason: root_reason.clone(),
            },
            DiscoveryReason::root(root_reason),
            SourceClass::Ordinary,
            false,
            false,
            0,
        );
        self.root_tasks.push(task);
        task
    }

    /// tsgo getSourceFileFromReference for a root without an extension: the
    /// host is asked whether each candidate exists; the first that does is
    /// the root's task.
    fn load_extensionless_root(
        &mut self,
        path: ProgramPath,
        root_spelling: JsStr<'_>,
        root_reason: RootFileReason,
    ) -> Result<(), ProgramLoadError> {
        let requested_text = path.display();
        if self.compiler_options.allow_non_ts_extensions == Some(true) {
            if self.file_exists(&path)? {
                let task = self.new_root_task(path.clone(), root_spelling, root_reason);
                self.roots.push(StagedRoot {
                    path,
                    source: None,
                    missing_diagnostic: None,
                    task: Some(task),
                });
                return Ok(());
            }
            let diagnostic = missing_root_diagnostic(root_spelling, root_reason);
            if self
                .diagnosed_missing_roots
                .insert(path.display().to_owned())
            {
                self.program_diagnostics.push(diagnostic.clone());
            }
            self.roots.push(StagedRoot {
                path,
                source: None,
                missing_diagnostic: Some(diagnostic),
                task: None,
            });
            return Ok(());
        }
        for &extension in extensionless_source_probe_extensions(self.compiler_options.allow_js) {
            let mut candidate_text = requested_text.to_owned();
            candidate_text.push_str(extension);
            let candidate = make_program_path(
                &candidate_text,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::NormalizeRoot,
                    Some(path.display().to_owned()),
                    None,
                    error,
                )
            })?;
            if self.file_exists(&candidate)? {
                let task = self.new_root_task(candidate, root_spelling, root_reason);
                self.roots.push(StagedRoot {
                    path,
                    source: None,
                    missing_diagnostic: None,
                    task: Some(task),
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
            .insert(path.display().to_owned())
        {
            self.program_diagnostics.push(diagnostic.clone());
        }
        self.roots.push(StagedRoot {
            path,
            source: None,
            missing_diagnostic: Some(diagnostic),
            task: None,
        });
        Ok(())
    }

    fn file_exists(&self, path: &ProgramPath) -> Result<bool, ProgramLoadError> {
        self.host.file_exists_js(path.display()).map_err(|error| {
            ProgramLoadError::host_js(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_owned()),
                error,
            )
        })
    }

    /// tsgo addAutomaticTypeDirectiveTasks: the last root task; its
    /// subtasks are the automatic type directives' targets.
    fn load_automatic_type_directives(&mut self) -> Result<(), ProgramLoadError> {
        let containing_file = self.automatic_types_containing_file()?;
        let task = self.new_task(
            containing_file,
            TaskOrigin::AutomaticTypes,
            DiscoveryReason::dependency(SourceInclusionReason::Synthetic),
            SourceClass::Ordinary,
            false,
            false,
            0,
        );
        self.root_tasks.push(task);
        Ok(())
    }

    /// tsgo parseTask.loadAutomaticTypeDirectives.
    fn load_automatic_types_task(&mut self, task: usize) -> Result<(), ProgramLoadError> {
        let (names, uses_wildcard) = self.automatic_type_directive_names()?;
        if names.is_empty() {
            return Ok(());
        }

        let containing_file = self.tasks[task].path.clone();
        let request_edges = self.request_edges.saturating_add(names.len());
        self.enforce_limit(
            ProgramLoadOperation::DiscoverAutomaticTypes,
            ProgramLoadLimit::RequestEdges,
            Some(containing_file.display().to_owned()),
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
                let host = match self.pre_resolved_types.remove(&key) {
                    Some(host) => host,
                    None => self
                        .resolver
                        .resolve_type_reference(
                            containing_file.display(),
                            name,
                            ResolutionMode::Unspecified,
                            type_roots.as_deref(),
                        )
                        .map_err(|error| {
                            ProgramLoadError::resolution_js(
                                ProgramLoadOperation::ResolveTypeReference,
                                Some(containing_file.display().to_owned()),
                                Some(name.clone()),
                                error,
                            )
                        })?,
                };
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
        for (name, index) in names.into_iter().zip(resolution_indices) {
            let target = match &self.type_resolutions[index].host {
                ResolutionOutcome::Resolved(target) => Some((
                    target.resolved_file().clone(),
                    target.extension().clone(),
                    target.is_external_library_import(),
                    target.package_id().cloned(),
                )),
                ResolutionOutcome::NotFound => None,
            };
            let Some((target, extension, external, package_id)) = target else {
                self.type_resolutions[index]
                    .diagnostics
                    .push(automatic_type_reference_diagnostic(
                        name.as_js(),
                        uses_wildcard,
                        self.program_options.config_file(),
                    ));
                continue;
            };
            if !is_loadable_typescript_extension(&extension) {
                return Err(ProgramLoadError::invalid_data_js(
                    ProgramLoadOperation::ResolveTypeReference,
                    Some(target.display().to_owned()),
                    "a resolved automatic type-reference target is not a TypeScript source file",
                ));
            }
            // One subtask per name (tsgo adds one per resolved directive).
            let subtask = self.new_task(
                target,
                TaskOrigin::TypeReference,
                DiscoveryReason::automatic_type(name.clone(), uses_wildcard)
                    .with_package_id(package_id),
                SourceClass::Ordinary,
                external,
                false,
                1,
            );
            self.tasks[task].subtasks.push(subtask);
        }
        Ok(())
    }

    fn automatic_type_directive_names(
        &mut self,
    ) -> Result<(Vec<JsString>, bool), ProgramLoadError> {
        let configured = self
            .program_options
            .types()
            .map_or_else(Vec::new, <[_]>::to_vec);
        let uses_wildcard = configured.iter().any(|name| name == "*");
        if !uses_wildcard {
            return Ok((configured, false));
        }

        let wildcard_matches: Vec<JsString> = self.discover_wildcard_type_directives()?;
        let mut seen = HashSet::default();
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

    fn discover_wildcard_type_directives(&mut self) -> Result<Vec<JsString>, ProgramLoadError> {
        let roots = self
            .resolver
            .effective_type_roots(self.program_options.type_roots())
            .map_err(|error| {
                ProgramLoadError::resolution_with_source_path(
                    ProgramLoadOperation::DiscoverAutomaticTypes,
                    None,
                    error,
                )
            })?;
        discover_wildcard_type_directive_names(self.host, &roots).map_err(|error| match error {
            WildcardDiscoveryError::Host { path, error } => ProgramLoadError::host_js(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                Some(path),
                error,
            ),
            WildcardDiscoveryError::Decode { path, source } => ProgramLoadError::decode_js(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                path,
                source,
            ),
            WildcardDiscoveryError::InvalidData { path, detail } => {
                ProgramLoadError::invalid_data_js(
                    ProgramLoadOperation::DiscoverAutomaticTypes,
                    Some(path),
                    detail,
                )
            }
        })
    }

    fn automatic_types_containing_file(&self) -> Result<ProgramPath, ProgramLoadError> {
        let normalized = crate::module_resolution::normalize_absolute_js_path(
            INFERRED_TYPES_CONTAINING_FILE.into(),
            Some(self.resolver.type_root_base_directory()),
            true,
        )
        .map_err(|error| {
            ProgramLoadError::resolution_js(
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
            ProgramLoadError::resolution_js(
                ProgramLoadOperation::DiscoverAutomaticTypes,
                Some(normalized),
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
            let catalog_path = self.catalog_library_path(file_name)?;
            let path = self.resolved_library_path(file_name)?;
            let class = SourceClass::Library {
                priority: catalog.source_file_priority(
                    &path,
                    self.library_directory.as_ref().expect("library directory"),
                ),
                replacement: path.canonical() != catalog_path.canonical(),
            };
            let task = self.new_task(
                path,
                TaskOrigin::LibraryRoot(reason.clone()),
                DiscoveryReason::dependency(SourceInclusionReason::LibraryRoot(reason)),
                class,
                false,
                false,
                0,
            );
            self.root_tasks.push(task);
        }
        Ok(())
    }

    fn finish(mut self) -> Result<CompleteGraph, ProgramLoadError> {
        self.resolve_root_sources();
        self.record_unloaded_targets()?;
        let collected = self.collect_files()?;
        self.program_order = collected.order.clone();
        let mut kept = vec![false; self.sources.len()];
        for &source in &collected.order {
            kept[source] = true;
        }
        // A redirected copy's path names the owner of its package identity
        // (tsgo filesByPath[task.path] = packageIdFile, redirectTargetsMap);
        // its own subtree was not walked. The copy keeps its include reasons
        // (filesparser.go addIncludeReason records them on the loaded task):
        // explainFiles lists them with the copy.
        let mut redirect_targets: FxHashMap<usize, usize> = FxHashMap::default();
        let mut package_redirect_files = Vec::new();
        for &(source, target, index) in &collected.redirects {
            redirect_targets.insert(source, target);
            let reasons = std::mem::take(&mut self.sources[source].inclusion_reasons);
            let path = self.sources[source].prepared.path().clone();
            self.sources[target]
                .prepared
                .remember_package_redirect(path.clone());
            package_redirect_files.push((path, target, index, reasons));
        }
        for root in &mut self.roots {
            if let Some(source) = root.source {
                if let Some(&target) = redirect_targets.get(&source) {
                    root.source = Some(target);
                }
            }
        }
        self.project_reference_redirects = std::mem::take(&mut self.project_reference_redirects)
            .into_iter()
            .filter_map(|(path, source)| match redirect_targets.get(&source) {
                Some(&target) => Some((path, target)),
                None => kept[source].then_some((path, source)),
            })
            .collect();
        // Only a collected file's resolutions are the program's (tsgo
        // resolvedModules[path] = task.resolutionsInFile inside the collect
        // walk): the rows of a redirected copy and of a source reached only
        // through one are dropped with it.
        let dropped_paths = self
            .sources
            .iter()
            .enumerate()
            .filter(|(source, _)| !kept[*source])
            .map(|(_, source)| source.prepared.path().canonical().clone())
            .collect::<rustc_hash::FxHashSet<_>>();
        if !dropped_paths.is_empty() {
            self.module_resolutions
                .retain(|resolution| !dropped_paths.contains(resolution.key.source()));
            self.type_resolutions
                .retain(|resolution| match resolution.key.origin() {
                    crate::resolution::TypeReferenceResolutionOrigin::Source(source) => {
                        !dropped_paths.contains(source)
                    }
                    crate::resolution::TypeReferenceResolutionOrigin::Automatic(_) => true,
                });
        }
        if self
            .compiler_options
            .force_consistent_casing_in_file_names_effective()
        {
            let casing_diagnostics = self
                .sources
                .iter()
                .enumerate()
                .filter(|(source, _)| kept[*source])
                .flat_map(|(_, source)| {
                    source
                        .alternate_inclusion_reasons
                        .iter()
                        .map(|(alias, reason)| {
                            casing_alias_diagnostic(
                                &source.prepared,
                                alias.as_js(),
                                &source.inclusion_reasons,
                                reason,
                                self.program_options.config_file(),
                            )
                        })
                })
                .collect::<Vec<_>>();
            self.program_diagnostics.extend(casing_diagnostics);
        }
        // On a case-sensitive host tsc keeps distinct physical spellings in
        // `filesByName` and independently checks them through
        // `filesByNameIgnoreCase`. Unlike aliases collapsed by the host's
        // canonical key, these diagnostics are unconditional even when
        // `forceConsistentCasingInFileNames` is explicitly false.
        let case_sensitive_casing_diagnostics = self
            .case_sensitive_casing_conflicts
            .iter()
            .map(|conflict| {
                let existing = &self.sources[conflict.existing_source];
                casing_distinct_file_diagnostic(
                    &existing.prepared,
                    conflict.incoming_path.as_js(),
                    &existing.inclusion_reasons,
                    &conflict.incoming_reason,
                    self.program_options.config_file(),
                )
            })
            .collect::<Vec<_>>();
        self.program_diagnostics
            .extend(case_sensitive_casing_diagnostics);
        let (option_diagnostics, root_diagnostics) = self.output_directory_diagnostics();
        // getOptionsDiagnostics selects only global/config-file rows from
        // the combined collection (_tsc.js:124024-124036). Source-owned
        // module constraints instead feed getSemanticDiagnostics, which
        // command reporting skips after an option/global diagnostic.
        self.program_diagnostics
            .extend(self.source_module_option_diagnostics());
        self.program_diagnostics
            .extend(self.project_reference_diagnostics());
        self.program_diagnostics.extend(root_diagnostics);
        let mut library_postorder = collected
            .order
            .iter()
            .copied()
            .filter(|&source| self.sources[source].library_priority.is_some())
            .collect::<Vec<_>>();
        if !library_postorder.is_empty() {
            library_postorder.sort_by_key(|&source| {
                let staged = &self.sources[source];
                let priority = staged
                    .library_priority
                    .expect("filtered library source has a stable priority");
                // Only processingDefaultLibFiles is sorted upstream. Retain
                // first-load postorder for roots promoted to lib membership.
                (
                    !staged.initially_library,
                    if staged.initially_library {
                        priority
                    } else {
                        0
                    },
                )
            });
        }
        let ordinary_postorder = collected
            .order
            .iter()
            .copied()
            .filter(|&source| self.sources[source].library_priority.is_none())
            .collect();
        let mut package_json_probes = self.read_ahead_package_json_probes;
        for resolver in self.project_resolvers.values_mut() {
            package_json_probes.extend(resolver.take_package_json_probes());
        }
        Ok(CompleteGraph {
            sources: self.sources,
            library_postorder,
            ordinary_postorder,
            roots: self.roots,
            module_resolutions: self.module_resolutions,
            type_resolutions: self.type_resolutions,
            program_diagnostics: self.program_diagnostics,
            option_diagnostics,
            project_reference_redirects: self.project_reference_redirects,
            package_json_probes,
            package_redirect_files,
        })
    }

    /// tsgo verifyProjectReferences: for every reference of the root config
    /// and of the referenced projects, the config must exist (TS6053) and,
    /// when the referencing project has files, be composite (TS6306) and
    /// emit (TS6310); a referenced project must not write the same build
    /// info file (TS5056). Each diagnostic is located at the reference's
    /// syntax in the referencing config.
    fn project_reference_diagnostics(&self) -> Vec<Diagnostic> {
        let Some(references) = self.project_references() else {
            return Vec::new();
        };
        let context = self.resolver.path_context();
        let current_directory = context.current_directory().display();
        let case_sensitive = context.use_case_sensitive_file_names();
        let root_config = self.program_options.config_file();
        let root_build_info = (self.compiler_options.suppress_output_path_check != Some(true))
            .then(|| {
                crate::output_directories::build_info_file_name(
                    self.compiler_options,
                    root_config.map(|config| config.path().display()),
                    current_directory,
                    case_sensitive,
                )
            })
            .flatten();
        let mut diagnostics = Vec::new();
        let mut pending = vec![(
            root_config.map(|config| (config.diagnostic_file_name().to_owned(), config.clone())),
            !self.roots.is_empty(),
            references.root_references(),
        )];
        // tsgo rangeResolvedProjectReference: each config once, the root
        // config never (a reference back to it, in a cycle, is skipped).
        let mut seen: HashSet<JsString> = HashSet::default();
        if let Some(config) = root_config {
            seen.insert(config.path().canonical().as_js().to_owned());
        }
        while let Some((parent_config, parent_has_files, entries)) = pending.pop() {
            for (index, entry) in entries.iter().enumerate() {
                let key = crate::output_directories::canonical_emit_path(
                    entry.config_file_name(),
                    current_directory,
                    case_sensitive,
                );
                if !seen.insert(key) {
                    continue;
                }
                let location = parent_config.as_ref().and_then(|(file_name, config)| {
                    config
                        .project_reference_location(index)
                        .map(|span| (file_name.clone(), span))
                });
                let at = |message: &'static tsc_diagnostics::DiagnosticMessage,
                          args: &[JsString]| {
                    let chain = MessageChain::new_js(message, args);
                    match &location {
                        Some((file_name, span)) => Diagnostic::new_js(
                            Some(file_name.clone()),
                            Some(span.start()),
                            Some(span.length()),
                            chain,
                        ),
                        None => Diagnostic::new_js(None, None, None, chain),
                    }
                };
                let written = entry.reference().path.clone();
                let Some(project) = entry.project() else {
                    diagnostics.push(at(&gen::File_0_not_found, &[written]));
                    continue;
                };
                let options = project.compiler_options();
                if (options.composite != Some(true) || options.no_emit == Some(true))
                    && parent_has_files
                {
                    if options.composite != Some(true) {
                        diagnostics.push(at(
                            &gen::Referenced_project_0_must_have_setting_composite_true,
                            std::slice::from_ref(&written),
                        ));
                    }
                    if options.no_emit == Some(true) {
                        diagnostics.push(at(
                            &gen::Referenced_project_0_may_not_disable_emit,
                            std::slice::from_ref(&written),
                        ));
                    }
                }
                if let (Some(mine), Some(theirs)) =
                    (&root_build_info, project.build_info_file_name())
                {
                    if mine.as_js() == theirs {
                        diagnostics.push(at(
                            &gen::Cannot_write_file_0_because_it_will_overwrite_tsbuildinfo_file_generated_by_referenced_project_1,
                            &[mine.clone(), written],
                        ));
                    }
                }
                pending.push((
                    project
                        .plan()
                        .program_options()
                        .config_file()
                        .map(|config| (config.diagnostic_file_name().to_owned(), config.clone())),
                    !project.plan().file_names().is_empty(),
                    references.references_in_config(project.canonical()),
                ));
            }
        }
        diagnostics
    }

    // tsc-port: verifyCompilerOptions (source module constraints) @6.0.3
    // tsc-hash: 38bf2ea163bba1c3f40bceda14aeb650fd97e8f3893c6e669b2e8a8bca289f78
    // tsc-span: _tsc.js:124874-124898
    fn source_module_option_diagnostics(&self) -> Vec<Diagnostic> {
        let options = self.compiler_options;
        let message = if options.isolated_modules != Some(true)
            && options.verbatim_module_syntax != Some(true)
            && options.module == Some(0)
            && options.emit_script_target() < tsc_types::ScriptTarget::ES2015
        {
            MessageChain::new(
                &gen::Cannot_use_imports_exports_or_module_augmentations_when_module_is_none,
                &[],
            )
        } else if options
            .out_file
            .as_ref()
            .is_some_and(|path| !path.is_empty())
            && options.emit_declaration_only != Some(true)
            && options.module.is_none()
        {
            MessageChain::new(
                &gen::Cannot_compile_modules_using_option_0_unless_the_module_flag_is_amd_or_system,
                &["outFile".to_owned()],
            )
        } else {
            return Vec::new();
        };
        self.program_order
            .iter()
            .find_map(|&index| {
                let source = &self.sources[index];
                let (start, length) = source.external_module_diagnostic_span?;
                Some(Diagnostic::new_js(
                    Some(source.prepared.path().display().to_owned()),
                    Some(start),
                    Some(length),
                    message.clone(),
                ))
            })
            .into_iter()
            .collect()
    }

    /// tsc-port: verifyCompilerOptions @6.0.3 (output directories)
    /// tsc-hash: 37d75cd1533127623cddc13e96bd9dec83595c91e1c2cc7166c7e2b27bfeaeeb
    /// tsc-span: _tsc.js:124907-124943
    /// tsc-port: checkSourceFilesBelongToPath @6.0.3
    /// tsc-hash: ce80660462a7406eafca61b870f95cc1d860753bbf6a7f7b4c97a857a0fec137
    /// tsc-span: _tsc.js:124639-124657
    /// Root diagnostics join the Program collection; its consumers select
    /// fileless option diagnostics and source-owned semantic diagnostics.
    fn output_directory_diagnostics(&self) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
        use crate::output_directories::{
            canonical_emit_path, common_source_directory, directory_relative_to_config,
            inferred_common_source_directory, source_file_may_be_emitted_for_options,
        };
        let options = self.compiler_options;
        let active =
            |value: &Option<JsString>| value.as_ref().is_some_and(|value| !value.is_empty());
        let declarations = options.declaration == Some(true) || options.composite == Some(true);
        let config_path = self
            .program_options
            .config_file_path()
            .map(ProgramPath::display);
        let verify = active(&options.out_dir)
            || active(&options.root_dir)
            || active(&options.source_root)
            || active(&options.map_root)
            || declarations && active(&options.declaration_dir);
        let migration = options.no_emit != Some(true)
            && options.composite != Some(true)
            && !active(&options.root_dir)
            && config_path.is_some()
            && (active(&options.out_dir)
                || active(&options.out_file)
                || declarations && active(&options.declaration_dir));
        let composite = options.composite == Some(true);
        if !verify && !migration && !composite {
            return (Vec::new(), Vec::new());
        }

        let context = self.resolver.path_context();
        let current_directory = context.current_directory().display();
        let case_sensitive = context.use_case_sensitive_file_names();
        let emitted = self
            .program_order
            .iter()
            .map(|&index| &self.sources[index])
            .filter(|source| {
                source_file_may_be_emitted_for_options(
                    source.prepared.path().display(),
                    source.prepared.may_be_emitted() && source.has_non_external_reason,
                    options,
                    config_path,
                    current_directory,
                    case_sensitive,
                )
            })
            .collect::<Vec<_>>();
        let paths = emitted
            .iter()
            .map(|source| source.prepared.path().display())
            .collect::<Vec<_>>();
        let common = common_source_directory(
            options,
            config_path,
            &paths,
            current_directory,
            case_sensitive,
        );
        let mut diagnostics = Vec::new();
        let mut root_diagnostics = Vec::new();
        let packages = self
            .resolver
            .observed_package_metadata()
            .map(|package| (package.package_json().canonical(), package))
            .collect::<BTreeMap<_, _>>();
        let root = (verify || migration)
            .then(|| {
                options
                    .root_dir
                    .as_ref()
                    .filter(|root| !root.is_empty())
                    .cloned()
                    .or_else(|| config_path.map(crate::js_path::directory_name))
            })
            .flatten();
        if let Some(root) = root {
            let canonical_root =
                canonical_emit_path(root.as_js(), current_directory, case_sensitive);
            for source in &emitted {
                let file = canonical_emit_path(
                    source.prepared.path().display(),
                    current_directory,
                    case_sensitive,
                );
                if !file.as_js().starts_with_js(canonical_root.as_js()) {
                    root_diagnostics.push(root_directory_diagnostic(
                        source,
                        root.as_js(),
                        self.program_options.config_file(),
                        self.compiler_options,
                        source
                            .prepared
                            .package_scope()
                            .and_then(|key| packages.get(key).copied()),
                    ));
                }
            }
        }
        // tsc verifyCompilerOptions: a composite project lists every file it
        // would emit, so a file reached only through an import is reported at
        // that import, explained like a rootDir violation.
        if composite {
            let root_paths = self
                .roots
                .iter()
                .map(|root| root.path.canonical())
                .collect::<HashSet<_>>();
            let empty = JsString::new();
            let project = config_path.unwrap_or_else(|| empty.as_js());
            for source in &emitted {
                if !root_paths.contains(source.prepared.path().canonical()) {
                    root_diagnostics.push(file_list_diagnostic(
                        source,
                        project,
                        self.program_options.config_file(),
                        self.compiler_options,
                        source
                            .prepared
                            .package_scope()
                            .and_then(|key| packages.get(key).copied()),
                    ));
                }
            }
        }
        if active(&options.out_dir)
            && common.is_empty()
            && self.sources.iter().any(|source| {
                crate::js_path::root_parts(source.prepared.path().display())
                    .is_some_and(|(root, _)| root.len_units() > 1)
            })
        {
            append_output_option_diagnostic(
                &mut diagnostics,
                self.program_options.config_file(),
                &["outDir"],
                MessageChain::new(
                    &gen::Cannot_find_the_common_subdirectory_path_for_the_input_files,
                    &[],
                ),
            );
        }
        if migration {
            let config = config_path.expect("migration requires a config path");
            let inferred =
                inferred_common_source_directory(&paths, current_directory, case_sensitive);
            if !inferred.is_empty()
                && canonical_emit_path(common.as_js(), current_directory, case_sensitive)
                    != canonical_emit_path(inferred.as_js(), current_directory, case_sensitive)
            {
                let names: &[&str] = if active(&options.out_file) {
                    &["outFile"]
                } else if active(&options.out_dir) {
                    &["outDir", "declarationDir"]
                } else {
                    &["declarationDir"]
                };
                let message = MessageChain::new_js(
                    &gen::The_common_source_directory_of_0_is_1_The_rootDir_setting_must_be_explicitly_set_to_this_or_another_path_to_adjust_your_output_s_file_layout,
                    &[crate::js_path::base_file_name(config),
                        directory_relative_to_config(config, inferred.as_js(), case_sensitive)],
                ).with_next(vec![MessageChain::new(&gen::Visit_https_aka_ms_ts6_for_migration_information, &[])]);
                append_output_option_diagnostic(
                    &mut diagnostics,
                    self.program_options.config_file(),
                    names,
                    message,
                );
            }
        }
        (diagnostics, root_diagnostics)
    }

    fn project_references(&self) -> Option<&'options Arc<ResolvedProjectReferences>> {
        self.program_options.project_references()
    }

    /// tsgo getParseFileRedirect for the command (no source of a project
    /// reference is used): the output declaration file loaded in place of a
    /// referenced project's source, with the source's name.
    fn project_reference_redirect(
        &self,
        path: &CanonicalPath,
    ) -> Result<Option<(ProgramPath, JsString)>, ProgramLoadError> {
        let Some(output) = self
            .project_references()
            .and_then(|references| references.output_for_source(path))
        else {
            return Ok(None);
        };
        let Some(output_dts) = output.output_dts() else {
            return Ok(None);
        };
        let output_path = crate::module_resolution::make_program_path(
            output_dts,
            self.resolver.path_context().use_case_sensitive_file_names(),
        )
        .map_err(|error| {
            ProgramLoadError::resolution_js(
                ProgramLoadOperation::NormalizeReference,
                Some(output.source().to_owned()),
                None,
                error,
            )
        })?;
        Ok(Some((output_path, output.source().to_owned())))
    }

    /// tsgo getRedirectForResolution: the referenced project whose options
    /// resolve the module names of `file`, when it is such a project's
    /// source or output and not the root project itself.
    fn project_for_resolution(
        &self,
        file: &CanonicalPath,
    ) -> Option<&'options Arc<crate::project_references::ResolvedProjectReference>> {
        let project = self.project_references()?.project_for_resolution(file)?;
        let root_config = self
            .program_options
            .config_file_path()
            .map(ProgramPath::canonical);
        (root_config != Some(project.canonical())).then_some(project)
    }

    /// tsgo ResolveModuleName with the redirect's options: the resolver of
    /// the referenced project `file` belongs to, created on first use.
    fn resolve_module_for_file(
        &mut self,
        file: &CanonicalPath,
        containing_file: &JsString,
        key: &ResolutionKey,
    ) -> Result<HostModuleResolution, ResolutionError> {
        let Some(project) = self.project_for_resolution(file) else {
            return self
                .resolver
                .resolve_with_facts(containing_file, key.specifier(), key.mode());
        };
        if !self.project_resolvers.contains_key(project.canonical()) {
            let resolver = ModuleResolver::new_with_program_options(
                self.host,
                project.compiler_options(),
                project.plan().program_options(),
            )?;
            self.project_resolvers
                .insert(project.canonical().clone(), resolver);
        }
        self.project_resolvers
            .get_mut(project.canonical())
            .expect("inserted above")
            .resolve_with_facts(containing_file, key.specifier(), key.mode())
    }

    /// tsgo filesParser: a task for one arrival at `path`. The data of the
    /// path is created (and the task registered under its spelling) when no
    /// task of the path was queued before (`taskDataByPath.LoadOrStore`).
    #[allow(clippy::too_many_arguments)]
    fn new_task(
        &mut self,
        path: ProgramPath,
        origin: TaskOrigin,
        reason: DiscoveryReason,
        class: SourceClass,
        increase_depth: bool,
        elide_on_depth: bool,
        tree_depth: usize,
    ) -> usize {
        let task = self.tasks.len();
        let (data, queued_with_data) = match self.task_data_by_path.get(path.canonical()) {
            Some(&data) => (data, true),
            None => {
                let data = self.task_data.len();
                self.task_data.push(TaskData {
                    spellings: vec![(path.display().to_owned(), task)],
                    lowest_depth: usize::MAX,
                    started_subtasks: false,
                    package_id: None,
                });
                self.task_data_by_path
                    .insert(path.canonical().clone(), data);
                (data, false)
            }
        };
        self.tasks.push(ParseTask {
            path,
            origin,
            reason,
            class,
            increase_depth,
            elide_on_depth,
            tree_depth,
            data,
            queued_with_data,
            subtasks: Vec::new(),
            loaded: false,
            started_subtasks: false,
            loaded_task: None,
            source: None,
            redirect: None,
        });
        task
    }

    /// tsgo filesParser.start: queue the tasks in order at `depth`.
    fn start_tasks(&mut self, tasks: &[usize], depth: usize) {
        for &task in tasks {
            self.stack.push((task, depth));
        }
    }

    /// tsgo filesParser.parse with the single-threaded work group: the root
    /// tasks are queued in order and the queue runs the last queued task
    /// first (core/workgroup.go singleThreadedWorkGroup.pop), so a task's
    /// subtasks run, the last first, before anything queued earlier.
    fn run_tasks(&mut self) -> Result<(), ProgramLoadError> {
        let roots = self.root_tasks.clone();
        self.start_tasks(&roots, 0);
        while let Some((task, depth)) = self.stack.pop() {
            self.execute_task(task, depth)?;
        }
        Ok(())
    }

    /// tsgo filesParser.start's queued function for one task
    /// (compiler/filesparser.go:278-328).
    fn execute_task(&mut self, task: usize, depth: usize) -> Result<(), ProgramLoadError> {
        let data = self.tasks[task].data;
        let mut start_subtasks = false;
        if self.tasks[task].queued_with_data {
            let spelling = self.tasks[task].path.display().to_owned();
            match self.task_data[data]
                .spellings
                .iter()
                .find(|(existing, _)| existing.as_js() == spelling.as_js())
            {
                Some(&(_, existing)) => self.tasks[task].loaded_task = Some(existing),
                None => {
                    self.task_data[data].spellings.push((spelling, task));
                    // A new task for the file name: load its subtasks if any
                    // other spelling's were started.
                    start_subtasks = self.task_data[data].started_subtasks;
                }
            }
        }
        if self.task_data[data].package_id.is_none() {
            if let Some(package_id) = self.tasks[task].reason.package_id.clone() {
                self.task_data[data].package_id = Some(package_id);
            }
        }
        let current_depth = depth.saturating_add(usize::from(self.tasks[task].increase_depth));
        if current_depth < self.task_data[data].lowest_depth {
            self.task_data[data].lowest_depth = current_depth;
            start_subtasks = true;
            self.task_data[data].started_subtasks = true;
        }
        if self.tasks[task].elide_on_depth
            && self
                .compiler_options
                .node_modules_depth_exceeds_limit(current_depth)
        {
            return Ok(());
        }
        let registered = self.task_data[data]
            .spellings
            .iter()
            .map(|&(_, task)| task)
            .collect::<Vec<_>>();
        for by_name in registered {
            let mut load_subtasks = start_subtasks;
            if !self.tasks[by_name].loaded {
                self.load_task(by_name)?;
                if self.tasks[by_name].redirect.is_some() {
                    // A redirected task always loads its redirect.
                    load_subtasks = true;
                    self.task_data[data].started_subtasks = true;
                }
            }
            if !self.tasks[by_name].started_subtasks && load_subtasks {
                self.tasks[by_name].started_subtasks = true;
                let subtasks = self.tasks[by_name].subtasks.clone();
                let lowest_depth = self.task_data[data].lowest_depth;
                self.start_tasks(&subtasks, lowest_depth);
            }
        }
        Ok(())
    }

    /// tsgo parseTask.load: the automatic directives, a project-reference
    /// redirect, or the file's admission and its subtasks.
    fn load_task(&mut self, task: usize) -> Result<(), ProgramLoadError> {
        self.tasks[task].loaded = true;
        if matches!(self.tasks[task].origin, TaskOrigin::AutomaticTypes) {
            return self.load_automatic_types_task(task);
        }
        let path = self.tasks[task].path.clone();
        if let Some((output, _source_name)) = self.project_reference_redirect(path.canonical())? {
            // tsgo parseTask.redirect: the output declaration file is loaded
            // with this task's reason (increaseDepth and elideOnDepth are not
            // copied), and the source path then names it.
            let reason = self.tasks[task].reason.clone();
            let class = self.tasks[task].class;
            let tree_depth = self.tasks[task].tree_depth;
            let redirect = self.new_task(
                output,
                TaskOrigin::Redirect,
                reason,
                class,
                false,
                false,
                tree_depth,
            );
            self.tasks[task].subtasks = vec![redirect];
            self.tasks[task].redirect = Some(redirect);
            return Ok(());
        }
        let source = self.admit_source(task)?;
        self.tasks[task].source = source;
        match source {
            Some(source) => self.plan_subtasks(task, source),
            None => self.report_missing_target(task),
        }
    }

    /// The registered task of a task's spelling (tsgo `loadedTask`).
    fn task_target(&self, task: usize) -> usize {
        let mut current = task;
        while let Some(alias) = self.tasks[current].loaded_task {
            current = alias;
        }
        current
    }

    /// The source a task loaded, through its alias and redirect chains.
    fn task_source(&self, task: usize) -> Option<usize> {
        let mut current = self.task_target(task);
        loop {
            if let Some(redirect) = self.tasks[current].redirect {
                current = self.task_target(redirect);
                continue;
            }
            return self.tasks[current].source;
        }
    }

    /// What a task whose file the host does not return reports: a reference
    /// its diagnostic (tsgo getSourceFileFromReference File_0_not_found), a
    /// library root its diagnostic, a resolved target an error (the resolver
    /// reported a file the host no longer returns); a root is reported once
    /// the tasks ran, a redirect through the import's unloaded reason.
    fn report_missing_target(&mut self, task: usize) -> Result<(), ProgramLoadError> {
        let path = self.tasks[task].path.clone();
        match self.tasks[task].origin.clone() {
            TaskOrigin::Root { .. } | TaskOrigin::Redirect | TaskOrigin::AutomaticTypes => Ok(()),
            TaskOrigin::LibraryRoot(reason) => {
                if self
                    .diagnosed_missing_library_roots
                    .insert(path.display().to_owned())
                {
                    let diagnostic = missing_library_root_diagnostic(&path, &reason);
                    self.missing_library_diagnostics
                        .insert(path.display().to_owned(), diagnostic.clone());
                    self.program_diagnostics.push(diagnostic);
                }
                Ok(())
            }
            TaskOrigin::PathReference {
                parent,
                pos,
                length,
                reference_text,
            } => {
                self.program_diagnostics.push(located_diagnostic(
                    &self.sources[parent].prepared,
                    pos,
                    length,
                    &gen::File_0_not_found,
                    std::slice::from_ref(&reference_text),
                )?);
                Ok(())
            }
            TaskOrigin::LibReference {
                parent,
                pos,
                length,
            } => {
                self.program_diagnostics.push(located_diagnostic(
                    &self.sources[parent].prepared,
                    pos,
                    length,
                    &gen::File_0_not_found,
                    &[path.display().to_owned()],
                )?);
                Ok(())
            }
            TaskOrigin::TypeReference => Err(ProgramLoadError::invalid_data_js(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_owned()),
                "resolver reported a type-reference target that the host no longer returns",
            )),
            TaskOrigin::Import => Err(ProgramLoadError::invalid_data_js(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_owned()),
                "resolver reported a module target that the host no longer returns",
            )),
        }
    }

    /// Admit the task's file: read (or take the read-ahead result), decode,
    /// plan and stage it. A path staged under another spelling binds to its
    /// source instead (tsgo parses each spelling; the port keeps one source
    /// per path and the collect walk keeps the first collected spelling).
    fn admit_source(&mut self, task: usize) -> Result<Option<usize>, ProgramLoadError> {
        let path = self.tasks[task].path.clone();
        let class = self.tasks[task].class;
        let tree_depth = self.tasks[task].tree_depth;
        if let Some(&existing) = self.source_by_canonical.get(path.canonical()) {
            self.bind_existing_source(existing, task)?;
            return Ok(Some(existing));
        }
        // A retained read-ahead result stands in for the host call it already
        // made (see `prefetch_roots`); otherwise the host is queried here.
        let read = match self.take_prefetched(&path) {
            Some(PrefetchedRead::Failed(error)) => Err(error),
            Some(PrefetchedRead::Missing) => Ok(None),
            Some(PrefetchedRead::Parsed { byte_len, decoded }) => {
                Ok(Some(SourceInput::Prefetched { byte_len, decoded }))
            }
            None => self
                .host
                .read_file_js(path.display())
                .map(|bytes| bytes.map(SourceInput::Fresh)),
        };
        let input = read.map_err(|error| {
            ProgramLoadError::host_js(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_owned()),
                error,
            )
        })?;
        let Some(input) = input else {
            return Ok(None);
        };
        let byte_len = match &input {
            SourceInput::Fresh(bytes) => bytes.len(),
            SourceInput::Prefetched { byte_len, .. } => *byte_len,
        };

        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::SourceDepth,
            Some(path.display().to_owned()),
            self.limits.max_source_depth.min(MAX_RECURSIVE_SOURCE_DEPTH),
            tree_depth,
        )?;
        let source_count = self.sources.len().saturating_add(1);
        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::SourceFiles,
            Some(path.display().to_owned()),
            self.limits.max_source_files,
            source_count,
        )?;
        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::SourceFileBytes,
            Some(path.display().to_owned()),
            self.limits.max_source_file_bytes,
            byte_len,
        )?;
        let total_source_bytes = self.total_source_bytes.saturating_add(byte_len);
        self.enforce_limit(
            ProgramLoadOperation::ReadSource,
            ProgramLoadLimit::TotalSourceBytes,
            Some(path.display().to_owned()),
            self.limits.max_total_source_bytes,
            total_source_bytes,
        )?;

        // The limit checks above admitted this source; keep the joint bound
        // with the payloads still retained by read-ahead (see prefetch_roots)
        // before any of its text is decoded or retained here.
        self.evict_read_ahead_for_admission(byte_len);
        let decoded = match input {
            SourceInput::Fresh(bytes) => decode_host_text(bytes).map(DecodedSource::Text),
            SourceInput::Prefetched { decoded, .. } => decoded.map(DecodedSource::Parsed),
        }
        .map_err(|source| {
            ProgramLoadError::decode_js(
                ProgramLoadOperation::DecodeSource,
                path.display().to_owned(),
                source,
            )
        })?;
        // Package deduplication is decided when the files are collected
        // (`collect_files`), as tsgo's parse tasks load every copy and its
        // subtasks and the collect walk redirects the later copies.
        let package_scope = self
            .resolver
            .package_scope_for_file(path.display())
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::ObservePackageScope,
                    Some(path.display().to_owned()),
                    None,
                    error,
                )
            })?;
        let file_name = path.display();
        // The output of a referenced project takes its project's options
        // (tsgo getCompilerOptionsForFile): its format and its requests
        // follow them.
        let options_for_file = self
            .project_for_resolution(path.canonical())
            .map_or(self.compiler_options, |project| project.compiler_options());
        let implied = implied_node_format(file_name, package_scope.as_ref(), options_for_file);
        let implied_for_emit =
            implied_node_format_for_emit(file_name, package_scope.as_ref(), options_for_file);
        // A read-ahead parse is adopted only when its assumptions are the
        // facts computed above; otherwise its decoded text is planned here.
        let (mut prepared, prefetched_plan) = match decoded {
            DecodedSource::Text(text) => (PreparedSourceFile::new(path.clone(), text), None),
            DecodedSource::Parsed(parsed)
                if parsed.implied == implied
                    && parsed.implied_for_emit == implied_for_emit
                    && parsed.prepared.path().display() == path.display() =>
            {
                let parsed = *parsed;
                (parsed.prepared, Some(parsed.plan))
            }
            DecodedSource::Parsed(parsed) => (
                PreparedSourceFile::from_snapshot(
                    path.clone(),
                    Arc::clone(parsed.prepared.snapshot()),
                ),
                None,
            ),
        };
        prepared = prepared.with_implied_node_formats(implied, implied_for_emit);
        if is_json_source(path.canonical())
            && self.compiler_options.out_dir.is_none()
            && self.compiler_options.out_file.is_none()
        {
            // tsc sourceFileMayBeEmitted: a JSON source is copied only when a
            // distinct output location (or a future bundle output) exists.
            prepared = prepared
                .with_may_be_emitted(false)
                .with_may_emit_forced_declaration(true);
        }
        if let Some(package_scope) = package_scope {
            prepared =
                prepared.with_package_scope(package_scope.package_json().canonical().clone());
        }
        let plan = if is_json_source(path.canonical()) {
            None
        } else {
            let planned = match prefetched_plan {
                Some(planned) => planned,
                // The planning parse is the only parse of this snapshot: the
                // checker session adopts it after proving equal parse options.
                None => match plan_source_requests_retaining_syntax(&prepared, options_for_file) {
                    Ok((plan, syntax)) => {
                        prepared = prepared.with_preparsed_syntax(syntax);
                        Ok(plan)
                    }
                    Err(error) => Err(error),
                },
            };
            Some(planned.map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::PlanSourceRequests,
                    Some(path.display().to_owned()),
                    None,
                    error,
                )
            })?)
        };
        let request_edges = self.request_edges.saturating_add(
            plan.as_ref()
                .map_or(0, |plan| plan.observed_request_occurrence_count()),
        );
        prepared = prepared
            .with_is_external_module(plan.as_ref().is_some_and(|plan| plan.is_external_module()));
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
        let module_request_spans =
            plan.as_ref()
                .map_or_else(rustc_hash::FxHashMap::default, |plan| {
                    plan.module_requests()
                        .iter()
                        .filter_map(|key| {
                            let spans = plan.module_request_spans(key);
                            (!spans.is_empty()).then(|| (key.clone(), spans.to_vec()))
                        })
                        .collect::<rustc_hash::FxHashMap<_, _>>()
                });
        self.enforce_limit(
            ProgramLoadOperation::PlanSourceRequests,
            ProgramLoadLimit::RequestEdges,
            Some(path.display().to_owned()),
            self.limits.max_request_edges,
            request_edges,
        )?;

        self.total_source_bytes = total_source_bytes;
        self.request_edges = request_edges;
        let source = self.sources.len();
        self.sources.push(StagedSource {
            prepared,
            external_module_diagnostic_span: plan
                .as_ref()
                .and_then(|plan| plan.external_module_diagnostic_span()),
            root_inclusions: Vec::new(),
            inclusion_reasons: Vec::new(),
            alternate_inclusion_reasons: Vec::new(),
            has_non_external_reason: false,
            library_priority: class.library_priority(),
            library_replacement: class.is_replacement(),
            initially_library: class.is_library(),
            path_references,
            type_reference_directives,
            lib_reference_directives,
            module_requests,
            module_request_spans,
        });
        // The joint bound over admitted sources and retained read-ahead
        // payloads holds after every admission (see prefetch_roots).
        debug_assert!(
            self.sources.len() + self.reserved_sources <= self.limits.max_source_files,
            "retained read-ahead payloads exceed the source-count limit"
        );
        debug_assert!(
            self.total_source_bytes + self.reserved_bytes <= self.limits.max_total_source_bytes,
            "retained read-ahead payloads exceed the total-byte limit"
        );
        self.source_by_canonical
            .insert(path.canonical().clone(), source);

        if self.sources[source].library_priority.is_some()
            && !self.sources[source].path_references.is_empty()
        {
            return Err(ProgramLoadError::unsupported_js(
                ProgramLoadOperation::PlanSourceRequests,
                Some(path.display().to_owned()),
                "default-library-path-references",
                "default-library path-reference descendants have processing-prefix order without checker-visible library membership, which the current PreparedProgram prefix cannot represent",
            ));
        }
        Ok(Some(source))
    }

    /// A task of another spelling of a staged path (tsc findSourceFileWorker's
    /// existing-source branch): the spelling is remembered as an alias and
    /// the library classification merges.
    fn bind_existing_source(&mut self, source: usize, task: usize) -> Result<(), ProgramLoadError> {
        let path = self.tasks[task].path.clone();
        let class = self.tasks[task].class;
        let first_path = self.sources[source].prepared.path().clone();
        if first_path.display() != path.display()
            && !self.normalized_display_paths_are_equal(&first_path, &path)?
        {
            self.sources[source]
                .prepared
                .remember_display_alias(path.display());
        }
        let existing_class = self.sources[source].source_class();
        if existing_class.is_library() != class.is_library()
            && !existing_class.is_replacement()
            && !class.is_replacement()
        {
            return Err(ProgramLoadError::unsupported_js(
                ProgramLoadOperation::ReadSource,
                Some(path.display().to_owned()),
                "library-source-classification-collision",
                format!(
                    "the source was first discovered as {existing_class:?} and later requested as {class:?}"
                ),
            ));
        }
        if let Some(priority) = class.library_priority() {
            if self.sources[source].library_priority.is_none()
                && !self.sources[source].path_references.is_empty()
            {
                return Err(ProgramLoadError::unsupported_js(
                    ProgramLoadOperation::PlanSourceRequests,
                    Some(path.display().to_owned()),
                    "default-library-path-references",
                    "a source promoted to default-library membership already has path-reference descendants whose checker-visible membership cannot be represented",
                ));
            }
            self.sources[source].library_priority = Some(
                self.sources[source]
                    .library_priority
                    .map_or(priority, |existing| existing.min(priority)),
            );
            self.sources[source].library_replacement |= class.is_replacement();
        }
        self.tasks[task].source = Some(source);
        Ok(())
    }

    /// tsgo parseTask.load's subtasks, in its order: path references, type
    /// reference directives, lib references, then the imports.
    fn plan_subtasks(&mut self, task: usize, source: usize) -> Result<(), ProgramLoadError> {
        // `noResolve` only suppresses path/type-reference source discovery.
        // Module requests still go through the resolver below so their
        // authoritative resolution facts and diagnostics remain available.
        if self.compiler_options.no_resolve != Some(true) {
            let path_references = self.sources[source].path_references.clone();
            for reference in &path_references {
                self.plan_path_reference(task, source, reference)?;
            }
            let type_reference_directives = self.sources[source].type_reference_directives.clone();
            self.plan_type_references(task, source, type_reference_directives)?;
        }
        if self.program_options.no_lib() != Some(true) {
            let lib_reference_directives = self.sources[source].lib_reference_directives.clone();
            self.plan_lib_references(task, source, lib_reference_directives)?;
        }
        // `noLib=true` deliberately performs no host operation for lib
        // directives, although their occurrences were counted above.
        let requests = self.sources[source].module_requests.clone();
        self.plan_module_requests(task, source, requests)
    }

    fn catalog_library_path(&self, file_name: &str) -> Result<ProgramPath, ProgramLoadError> {
        let directory = self
            .library_directory
            .as_ref()
            .expect("library-enabled graph has a normalized catalog directory");
        let base = directory.display();
        let normalized = crate::module_resolution::normalize_absolute_js_path(
            file_name.into(),
            Some(base),
            true,
        )
        .map_err(|error| {
            ProgramLoadError::resolution_js(
                ProgramLoadOperation::NormalizeReference,
                Some(directory.display().to_owned()),
                Some((file_name.to_owned()).into()),
                error,
            )
        })?;
        make_program_path(
            &normalized,
            self.resolver.path_context().use_case_sensitive_file_names(),
        )
        .map_err(|error| {
            ProgramLoadError::resolution_js(
                ProgramLoadOperation::NormalizeReference,
                Some(directory.display().to_owned()),
                Some((file_name.to_owned()).into()),
                error,
            )
        })
    }

    /// Resolve a logical TypeScript library through `@typescript/lib-*` and
    /// retain the catalog path as the ordinary miss fallback. The synthetic
    /// containing file fixes node_modules ancestry without becoming a source
    /// or a module-resolution row in the prepared Program.
    ///
    /// tsc-port: getInferredLibraryNameResolveFrom @6.0.3
    /// tsc-hash: 3b0042413535c17a745ef10495c1a068db91f949611f8459cc5cf5d678d103b4
    /// tsc-span: _tsc.js:122398-122401
    /// tsc-port: pathForLibFileWorker @6.0.3
    /// tsc-hash: 55c88bbf3d28be6acc2005e67cc61be44ab47825d457b1455253b597d75bb258
    /// tsc-span: _tsc.js:124526-124573
    fn resolved_library_path(&mut self, file_name: &str) -> Result<ProgramPath, ProgramLoadError> {
        if let Some(path) = self.resolved_library_paths.get(file_name) {
            return Ok(path.clone());
        }
        let fallback = self.catalog_library_path(file_name)?;
        let actual = if self.compiler_options.lib_replacement == Some(true) {
            let base = self.program_options.config_file_path().map_or_else(
                || {
                    self.resolver
                        .path_context()
                        .current_directory()
                        .display()
                        .to_owned()
                },
                |config| crate::js_path::directory_name(config.display()),
            );
            let synthetic_name = format!("__lib_node_modules_lookup_{file_name}__.ts");
            let resolve_from = crate::module_resolution::normalize_absolute_js_path(
                synthetic_name.as_str().into(),
                Some(base.as_js()),
                true,
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::ResolveLibrary,
                    Some(base.clone()),
                    Some((replacement_package_name(file_name)).into()),
                    error,
                )
            })?;
            let package_name = replacement_package_name(file_name);
            let resolution = self
                .library_resolver
                .as_deref_mut()
                .expect("libReplacement=true initializes the library resolver")
                .resolve(
                    resolve_from.as_js(),
                    &package_name,
                    ResolutionMode::Unspecified,
                )
                .map_err(|error| {
                    ProgramLoadError::resolution_js(
                        ProgramLoadOperation::ResolveLibrary,
                        Some(resolve_from.clone()),
                        Some((package_name.clone()).into()),
                        error,
                    )
                })?;
            match resolution {
                ResolutionOutcome::Resolved(module) => module.resolved_file().clone(),
                ResolutionOutcome::NotFound => fallback,
            }
        } else {
            fallback
        };
        self.resolved_library_paths
            .insert(file_name.to_owned(), actual.clone());
        Ok(actual)
    }

    /// tsgo parseTask.load's lib reference subtasks: an unknown name is a
    /// diagnostic at the directive; a known one a subtask with the library
    /// class and no depth flags.
    fn plan_lib_references(
        &mut self,
        task: usize,
        source: usize,
        directives: Vec<PlannedLibReferenceDirective>,
    ) -> Result<(), ProgramLoadError> {
        let catalog = self
            .library_catalog
            .expect("library-enabled graph has an injected catalog");
        let tree_depth = self.tasks[task].tree_depth.saturating_add(1);
        for directive in directives {
            let lib_name = to_file_name_lower_case_js(directive.file_name());
            let Some(file_name) = catalog.reference_file_name(&lib_name) else {
                let suggestion = catalog.spelling_suggestion(&lib_name);
                let (message, arguments) = match suggestion {
                    Some(suggestion) => (
                        &gen::Cannot_find_lib_definition_for_0_Did_you_mean_1,
                        vec![lib_name, suggestion.into()],
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
            let target = self.resolved_library_path(file_name)?;
            let catalog_path = self.catalog_library_path(file_name)?;
            let reference = SourceInclusionReason::LibraryReference {
                parent: self.sources[source].prepared.path().display().to_owned(),
                specifier: directive.file_name().to_owned(),
            };
            let class = SourceClass::Library {
                priority: catalog.source_file_priority(
                    &target,
                    self.library_directory.as_ref().expect("library directory"),
                ),
                replacement: target.canonical() != catalog_path.canonical(),
            };
            let subtask = self.new_task(
                target,
                TaskOrigin::LibReference {
                    parent: source,
                    pos: directive.pos(),
                    length: directive.length(),
                },
                DiscoveryReason::dependency(reference),
                class,
                false,
                false,
                tree_depth,
            );
            self.tasks[task].subtasks.push(subtask);
        }
        Ok(())
    }

    fn normalized_display_paths_are_equal(
        &self,
        left: &ProgramPath,
        right: &ProgramPath,
    ) -> Result<bool, ProgramLoadError> {
        let current_directory = self.resolver.path_context().current_directory().display();
        let normalize = |path: &ProgramPath| {
            crate::module_resolution::normalize_absolute_js_path(
                path.display(),
                Some(current_directory),
                true,
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::ReadSource,
                    Some(path.display().to_owned()),
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
    /// tsgo resolveTripleslashPathReference / getSourceFileFromReference:
    /// the extension checks, the candidates' existence and a self-reference
    /// are the parent's diagnostics; an admitted target is a subtask with the
    /// parent's class and no depth flags.
    fn plan_path_reference(
        &mut self,
        task: usize,
        source: usize,
        reference: &PlannedPathReference,
    ) -> Result<(), ProgramLoadError> {
        let source_path = self.sources[source].prepared.path().clone();
        let base = crate::js_path::directory_name(source_path.display());
        let normalized = if reference.file_name().is_empty() {
            base.clone()
        } else {
            crate::module_resolution::normalize_absolute_js_path(
                reference.file_name(),
                Some(base.as_js()),
                true,
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_owned()),
                    Some(reference.file_name().to_owned()),
                    error,
                )
            })?
        };
        // tsgo names the reference as written in its diagnostics
        // (`diagnosticFileName`, compiler/fileloader.go:697), where tsc 6.0
        // named the resolved path.
        let reference_path = crate::js_path::normalize_slashes(reference.file_name());
        let has_extension = reference_path
            .as_js()
            .split_ascii(b'/')
            .next_back()
            .is_some_and(|name| name.contains("."));
        let tree_depth = self.tasks[task].tree_depth.saturating_add(1);
        let class = self.sources[source].source_class();
        if has_extension {
            let target = make_program_path(
                &normalized,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_owned()),
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
                        vec![reference_path.clone()],
                    )
                } else {
                    (
                        &gen::File_0_has_an_unsupported_extension_The_only_supported_extensions_are_1,
                        vec![
                            reference_path.clone(),
                            JsString::from(supported_source_extension_list(self.compiler_options.allow_js)),
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
            // tsgo getSourceFileFromReference: the host is asked whether the
            // file exists (File_0_not_found at the directive, no subtask),
            // then a self-reference is rejected.
            if !self.file_exists(&target)? {
                self.program_diagnostics.push(located_diagnostic(
                    &self.sources[source].prepared,
                    reference.pos(),
                    reference.length(),
                    &gen::File_0_not_found,
                    std::slice::from_ref(&reference_path),
                )?);
                return Ok(());
            }
            if target.canonical() == source_path.canonical() {
                self.program_diagnostics.push(located_diagnostic(
                    &self.sources[source].prepared,
                    reference.pos(),
                    reference.length(),
                    &gen::A_file_cannot_have_a_reference_to_itself,
                    &[] as &[String],
                )?);
                return Ok(());
            }
            self.plan_path_reference_subtask(
                task,
                source,
                reference,
                reference_path,
                target,
                class,
                tree_depth,
            );
            return Ok(());
        }

        if self.compiler_options.allow_non_ts_extensions == Some(true) {
            let target = make_program_path(
                &normalized,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_owned()),
                    Some(reference.file_name().to_owned()),
                    error,
                )
            })?;
            if self.file_exists(&target)? {
                self.plan_path_reference_subtask(
                    task,
                    source,
                    reference,
                    reference_path,
                    target,
                    class,
                    tree_depth,
                );
                return Ok(());
            }
            self.program_diagnostics.push(located_diagnostic(
                &self.sources[source].prepared,
                reference.pos(),
                reference.length(),
                &gen::File_0_not_found,
                std::slice::from_ref(&reference_path),
            )?);
            return Ok(());
        }
        for &extension in extensionless_source_probe_extensions(self.compiler_options.allow_js) {
            let mut target_text = normalized.clone();
            target_text.push_str(extension);
            let target = make_program_path(
                &target_text,
                self.resolver.path_context().use_case_sensitive_file_names(),
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::NormalizeReference,
                    Some(source_path.display().to_owned()),
                    Some(reference.file_name().to_owned()),
                    error,
                )
            })?;
            if self.file_exists(&target)? {
                if target.canonical() == source_path.canonical() {
                    self.program_diagnostics.push(located_diagnostic(
                        &self.sources[source].prepared,
                        reference.pos(),
                        reference.length(),
                        &gen::A_file_cannot_have_a_reference_to_itself,
                        &[] as &[String],
                    )?);
                    return Ok(());
                }
                self.plan_path_reference_subtask(
                    task,
                    source,
                    reference,
                    reference_path,
                    target,
                    class,
                    tree_depth,
                );
                return Ok(());
            }
        }
        self.program_diagnostics.push(located_diagnostic(
            &self.sources[source].prepared,
            reference.pos(),
            reference.length(),
            &gen::Could_not_resolve_the_path_0_with_the_extensions_1,
            &[
                reference_path,
                JsString::from(supported_source_extension_list(
                    self.compiler_options.allow_js,
                )),
            ],
        )?);
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn plan_path_reference_subtask(
        &mut self,
        task: usize,
        source: usize,
        reference: &PlannedPathReference,
        reference_text: JsString,
        target: ProgramPath,
        class: SourceClass,
        tree_depth: usize,
    ) {
        let parent = self.sources[source].prepared.path().display().to_owned();
        let subtask = self.new_task(
            target,
            TaskOrigin::PathReference {
                parent: source,
                pos: reference.pos(),
                length: reference.length(),
                reference_text,
            },
            DiscoveryReason::dependency(SourceInclusionReason::PathReference {
                parent,
                specifier: reference.file_name().to_owned(),
                pos: reference.pos(),
                end: reference.end(),
            }),
            class,
            false,
            false,
            tree_depth,
        );
        self.tasks[task].subtasks.push(subtask);
    }

    /// tsgo resolveTypeReferenceDirectives: every directive is resolved,
    /// an unresolved one is a diagnostic, and each resolved one is a subtask
    /// (one per directive; `increaseDepth` when the target is an external
    /// library).
    fn plan_type_references(
        &mut self,
        task: usize,
        source: usize,
        directives: Vec<PlannedTypeReferenceDirective>,
    ) -> Result<(), ProgramLoadError> {
        let containing_source = self.sources[source].prepared.path().clone();
        let type_roots = self.program_options.type_roots().map(<[_]>::to_vec);
        for directive in &directives {
            let key = directive.key().clone();
            if !self.type_resolution_by_key.contains_key(&key) {
                let host = match self.pre_resolved_types.remove(&key) {
                    Some(host) => host,
                    None => self
                        .resolver
                        .resolve_type_reference(
                            containing_source.display(),
                            key.specifier(),
                            key.mode(),
                            type_roots.as_deref(),
                        )
                        .map_err(|error| {
                            ProgramLoadError::resolution_js(
                                ProgramLoadOperation::ResolveTypeReference,
                                Some(containing_source.display().to_owned()),
                                Some(key.specifier().to_owned()),
                                error,
                            )
                        })?,
                };
                let index = self.type_resolutions.len();
                self.type_resolutions.push(StagedTypeResolution {
                    key: key.clone(),
                    host,
                    diagnostics: Vec::new(),
                });
                self.type_resolution_by_key.insert(key, index);
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

        let tree_depth = self.tasks[task].tree_depth.saturating_add(1);
        for directive in &directives {
            let index = self.type_resolution_by_key[directive.key()];
            let target = match &self.type_resolutions[index].host {
                ResolutionOutcome::Resolved(target) => Some((
                    target.resolved_file().clone(),
                    target.extension().clone(),
                    target.is_external_library_import(),
                    target.package_id().cloned(),
                )),
                ResolutionOutcome::NotFound => None,
            };
            let Some((target, extension, external, package_id)) = target else {
                continue;
            };
            if !is_loadable_typescript_extension(&extension) {
                return Err(ProgramLoadError::invalid_data_js(
                    ProgramLoadOperation::ResolveTypeReference,
                    Some(target.display().to_owned()),
                    "a resolved type-reference target is not a TypeScript source file",
                ));
            }
            let type_inclusion = SourceInclusionReason::TypeReference {
                parent: containing_source.display().to_owned(),
                specifier: directive.key().specifier().to_owned(),
                pos: directive.pos(),
                end: directive.end(),
                package_id: None,
            };
            let subtask = self.new_task(
                target,
                TaskOrigin::TypeReference,
                DiscoveryReason::dependency(type_inclusion).with_package_id(package_id),
                SourceClass::Ordinary,
                external,
                false,
                tree_depth,
            );
            self.tasks[task].subtasks.push(subtask);
        }
        Ok(())
    }

    /// tsc-port: processImportedModules @6.0.3
    /// tsc-hash: 5fb6c5d9e11130467d843f258aeb726b1cbca21cd00923b0f1c7da3097f9cc98
    /// tsc-span: _tsc.js:124595-124635
    fn plan_module_requests(
        &mut self,
        task: usize,
        source: usize,
        requests: Vec<(ResolutionKey, bool)>,
    ) -> Result<(), ProgramLoadError> {
        let tree_depth = self.tasks[task].tree_depth.saturating_add(1);
        let mut phase_indices = Vec::with_capacity(requests.len());
        let containing_file = self.sources[source].prepared.path().display().to_owned();
        let containing_canonical = self.sources[source].prepared.path().canonical().clone();
        let containing_file_is_declaration = is_declaration_file_name(containing_file.as_js());
        // The output of a referenced project resolves with its project's
        // options; its resolutions share no per-directory cache with the
        // root project's files.
        let containing_project = self
            .project_for_resolution(&containing_canonical)
            .map(|project| project.canonical().clone());
        for (key, loads_source) in requests {
            // One inclusion reason per occurrence that loads the target:
            // processImportedModules adds the resolved file once for every
            // import of it (tsgo fileloader.go:928-940), and the file's
            // explanation then lists each import. A request without a span
            // is a synthetic import.
            let staged = &self.sources[source];
            let inclusions = staged.module_request_spans.get(&key).map_or_else(
                || vec![SourceInclusionReason::Synthetic],
                |spans| {
                    let prepared = &staged.prepared;
                    let positions = prepared.snapshot().positions();
                    spans
                        .iter()
                        .map(|&(pos, end)| {
                            let start_byte = positions
                                .utf16_to_byte(pos)
                                .expect("module request starts at a source token boundary");
                            let end_byte = positions
                                .utf16_to_byte(end)
                                .expect("module request ends at a source token boundary");
                            SourceInclusionReason::Import {
                                parent: containing_file.clone(),
                                reference_text: prepared.text()
                                    [start_byte as usize..end_byte as usize]
                                    .to_owned(),
                                pos,
                                end,
                                package_id: None,
                            }
                        })
                        .collect::<Vec<_>>()
                },
            );
            let index = if let Some(index) = self.module_resolution_by_key.get(&key).copied() {
                self.module_resolutions[index].loads_source |= loads_source;
                index
            } else {
                // A resolution the read-ahead computed for this exact request
                // is the same pure host observation the resolver would make
                // here; it is taken in this request's turn.
                let host = match self.pre_resolved.remove(&key) {
                    Some(host) => {
                        self.pre_resolved_hits += 1;
                        host
                    }
                    None => {
                        let directory_key = (
                            crate::js_path::directory_name(containing_file.as_js()),
                            key.specifier().to_owned(),
                            key.mode(),
                        );
                        match self
                            .directory_resolutions
                            .get(&directory_key)
                            .filter(|_| containing_project.is_none())
                        {
                            Some(host) => {
                                self.directory_resolution_hits += 1;
                                host.clone()
                            }
                            None => {
                                let host = self
                                    .resolve_module_for_file(
                                        &containing_canonical,
                                        &containing_file,
                                        &key,
                                    )
                                    .map_err(|error| {
                                        ProgramLoadError::resolution_js(
                                            ProgramLoadOperation::ResolveModule,
                                            Some(containing_file.clone()),
                                            Some(key.specifier().to_owned()),
                                            error,
                                        )
                                    })?;
                                if containing_project.is_none() {
                                    self.directory_resolutions
                                        .insert(directory_key, host.clone());
                                }
                                host
                            }
                        }
                    }
                };
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
            phase_indices.push((index, inclusions));
        }

        // As with type directives, all requests in this source are resolved
        // before the first successful target starts its DFS.
        for (index, inclusions) in phase_indices {
            let loads_source = self.module_resolutions[index].loads_source;
            let target = match self.module_resolutions[index].host.outcome() {
                ResolutionOutcome::Resolved(target) => Some((
                    target.resolved_file().clone(),
                    target.extension().clone(),
                    target.is_external_library_import(),
                    target.original_path().is_some(),
                    target.package_id().cloned(),
                )),
                ResolutionOutcome::NotFound => None,
            };
            let Some((target, extension, external, has_original_path, package_id)) = target else {
                continue;
            };
            if let Some(reason) = resolution_diagnostic_unloaded_reason(
                &extension,
                self.compiler_options,
                containing_file_is_declaration,
                loads_source,
            ) {
                // createProgram records the successful resolution, but a
                // getResolutionDiagnostic result prevents findSourceFile.
                // Retain that distinction as typed graph-admission state so
                // the checker can report the resolution diagnostic without
                // treating the absent source as a failed module lookup.
                self.module_resolutions[index].unloaded_reason = Some(reason);
                continue;
            }
            // tsgo fileloader.go:911: a resolved file is JavaScript unless it
            // is a source of a referenced project (its output is loaded).
            let redirected = self
                .project_reference_redirect(target.canonical())?
                .is_some();
            // tsgo isJsFileFromNodeModules: the subtask of a JavaScript file
            // found searching node_modules is elided beyond
            // maxNodeModuleJsDepth when it runs (`elideOnDepth`).
            let elide_on_depth = extension.is_javascript()
                && !redirected
                && external
                && path_contains_node_modules(target.canonical().as_js());
            if extension.is_javascript() && !redirected {
                let reason = unloaded_javascript_reason(
                    &extension,
                    self.compiler_options,
                    external,
                    has_original_path,
                    target.canonical(),
                    loads_source,
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
                // A name that does not load a source (a module augmentation's)
                // is resolved and recorded, but its target joins the program
                // only when an import of some file loads it
                // (processImportedModules adds a file for the names of
                // imports alone; tsgo fileloader.go:915-923). The checker
                // then finds the resolution without a source file.
                self.module_resolutions[index].unloaded_reason =
                    Some(UnloadedModuleReason::ResolutionOnly);
                continue;
            }
            // Resolver-owned external symlink handling has already replaced
            // this target with its physical resolvedFileName. Visit that
            // identity directly; the lexical spelling remains on the host
            // resolution and is published as originalPath below.
            if matches!(extension, ModuleExtension::Json) {
                if !self.compiler_options.resolve_json_module_effective() {
                    return Err(ProgramLoadError::unsupported_js(
                        ProgramLoadOperation::ResolveModule,
                        Some(target.display().to_owned()),
                        "resolveJsonModule",
                        "a JSON target was resolved while resolveJsonModule is disabled",
                    ));
                }
                self.plan_import_subtasks(
                    task,
                    &target,
                    external,
                    elide_on_depth,
                    package_id,
                    inclusions,
                    tree_depth,
                );
                continue;
            }
            if !is_loadable_typescript_extension(&extension) && !extension.is_javascript() {
                return Err(ProgramLoadError::unsupported_js(
                    ProgramLoadOperation::ResolveModule,
                    Some(target.display().to_owned()),
                    "resolved-module-extension",
                    format!(
                        "loadable target extension {} is outside the admitted source loader",
                        extension.as_js().to_string_lossy()
                    ),
                ));
            }
            self.plan_import_subtasks(
                task,
                &target,
                external,
                elide_on_depth,
                package_id,
                inclusions,
                tree_depth,
            );
        }
        Ok(())
    }

    /// One subtask per occurrence of the import (tsgo processImportedModules
    /// adds the resolved file once per import of it), so the target carries
    /// one inclusion reason per occurrence.
    #[allow(clippy::too_many_arguments)]
    fn plan_import_subtasks(
        &mut self,
        task: usize,
        target: &ProgramPath,
        external: bool,
        elide_on_depth: bool,
        package_id: Option<PackageId>,
        inclusions: Vec<SourceInclusionReason>,
        tree_depth: usize,
    ) {
        for inclusion in inclusions {
            let subtask = self.new_task(
                target.clone(),
                TaskOrigin::Import,
                DiscoveryReason::dependency(inclusion).with_package_id(package_id.clone()),
                SourceClass::Ordinary,
                external,
                elide_on_depth,
                tree_depth,
            );
            self.tasks[task].subtasks.push(subtask);
        }
    }

    fn enforce_limit(
        &self,
        operation: ProgramLoadOperation,
        limit: ProgramLoadLimit,
        path: Option<JsString>,
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
                path: error_display_path(path.as_ref()),
                js_path: path,
                maximum,
                observed,
            },
        })
    }

    /// tsgo filesParser.getProcessedFiles collectFiles
    /// (compiler/filesparser.go:380-491): the walk over the root tasks and
    /// their subtasks that attaches the include reasons (a task's to the
    /// task its spelling loaded, a redirect's to its output, none for a
    /// task that loaded nothing), orders the program's files (each after its
    /// subtasks), keeps the first collected spelling of a path and reports
    /// the other spellings as casing aliases, decides package
    /// deduplication (the first source entered with a package identity owns
    /// it; a later source with the same identity becomes a redirect to the
    /// owner and its subtasks are not walked) and marks the files found
    /// searching node_modules (the path's `lowest_depth` above zero).
    fn collect_files(&mut self) -> Result<CollectedFiles, ProgramLoadError> {
        // tsgo keeps the package-id map only while `deduplicatePackages` is
        // not `false` (compiler/filesparser.go:362-368).
        let deduplicate_packages = self.compiler_options.deduplicate_packages != Some(false);
        let case_sensitive = self.resolver.path_context().use_case_sensitive_file_names();
        let mut seen: Vec<Option<JsString>> = vec![None; self.task_data.len()];
        let mut registered: BTreeMap<PackageId, usize> = BTreeMap::new();
        let mut order: Vec<usize> = Vec::with_capacity(self.sources.len());
        let mut ordinary_count = 0usize;
        let mut library_count = 0usize;
        let mut redirects: Vec<(usize, usize, usize)> = Vec::new();
        let mut recorded_redirects: HashSet<CanonicalPath> = HashSet::default();
        // The entered tasks with the index of their next subtask.
        let mut frames: Vec<(usize, usize)> = Vec::new();
        let roots = self.root_tasks.clone();
        let mut next_root = 0usize;
        loop {
            let next = match frames.last_mut() {
                Some((task, index)) => {
                    let subtasks = &self.tasks[*task].subtasks;
                    if *index < subtasks.len() {
                        let subtask = subtasks[*index];
                        *index += 1;
                        Some(subtask)
                    } else {
                        None
                    }
                }
                None => {
                    if next_root < roots.len() {
                        next_root += 1;
                        Some(roots[next_root - 1])
                    } else {
                        break;
                    }
                }
            };
            let Some(task) = next else {
                let (task, _) = frames.pop().expect("an entered task to leave");
                self.leave_collected_task(
                    task,
                    &mut order,
                    &mut ordinary_count,
                    &mut library_count,
                    &mut recorded_redirects,
                );
                continue;
            };
            let redirect_index = ordinary_count + redirects.len();
            if let Some(entered) = self.enter_collected_task(
                task,
                &mut seen,
                &mut registered,
                deduplicate_packages,
                case_sensitive,
                redirect_index,
                &mut redirects,
            )? {
                frames.push((entered, 0));
            }
        }
        // tsgo lists the library files first: a redirect's index counts them
        // all (`redirectFile.index += len(libFiles)`).
        for redirect in &mut redirects {
            redirect.2 += library_count;
        }
        Ok(CollectedFiles { order, redirects })
    }

    /// tsgo collectFiles for one task before its subtasks: its include
    /// reason, the alias and casing checks of its spelling, and package
    /// deduplication. Returns the task whose subtasks are walked.
    #[allow(clippy::too_many_arguments)]
    fn enter_collected_task(
        &mut self,
        task: usize,
        seen: &mut [Option<JsString>],
        registered: &mut BTreeMap<PackageId, usize>,
        deduplicate_packages: bool,
        case_sensitive: bool,
        redirect_index: usize,
        redirects: &mut Vec<(usize, usize, usize)>,
    ) -> Result<Option<usize>, ProgramLoadError> {
        let reason = self.tasks[task].reason.inclusion.clone();
        // A redirect's reason reaches its output through its subtask; the
        // automatic type directives' task is an implementation detail.
        if self.tasks[task].redirect.is_none()
            && !matches!(self.tasks[task].origin, TaskOrigin::AutomaticTypes)
        {
            let target = self.task_target(task);
            self.add_include_reason(target, reason.clone());
        }
        let task = self.task_target(task);
        if !self.tasks[task].loaded {
            return Ok(None);
        }
        let data = self.tasks[task].data;
        let spelling = self.tasks[task].path.display().to_owned();
        if let Some(checked) = seen[data].clone() {
            // Another spelling of a walked path is a casing alias of the
            // kept file (tsgo addProcessingDiagnosticsForFileCasing).
            if checked.as_js() != spelling.as_js() {
                if let Some(source) = self.tasks[task].source {
                    let first_path = self.sources[source].prepared.path().clone();
                    let path = self.tasks[task].path.clone();
                    if !self.normalized_display_paths_are_equal(&first_path, &path)? {
                        self.sources[source]
                            .alternate_inclusion_reasons
                            .push((spelling, reason));
                    }
                }
            }
            return Ok(None);
        }
        seen[data] = Some(spelling.clone());
        let source = self.tasks[task].source;
        if let Some(source) = source {
            // The kept spelling is the first collected (tsgo keeps that
            // task's parse); the one that admitted the file becomes an alias.
            if self.sources[source].prepared.path().display() != spelling.as_js() {
                self.sources[source]
                    .prepared
                    .respell(spelling.as_js())
                    .map_err(|error| {
                        ProgramLoadError::preparation(ProgramLoadOperation::ReadSource, error)
                    })?;
            }
            // On a case-sensitive host tsc keeps distinct physical spellings
            // and independently checks them through `filesByNameIgnoreCase`
            // (tsgo tasksSeenByNameIgnoreCase).
            if case_sensitive {
                let path_lower_case =
                    crate::js_path::file_name_lower_case(self.tasks[task].path.canonical().as_js());
                if let Some(&existing_source) = self.files_by_name_ignore_case.get(&path_lower_case)
                {
                    if existing_source != source {
                        self.case_sensitive_casing_conflicts
                            .push(CaseSensitiveCasingConflict {
                                existing_source,
                                incoming_path: spelling,
                                incoming_reason: reason,
                            });
                    }
                } else {
                    self.files_by_name_ignore_case
                        .insert(path_lower_case, source);
                }
            }
        }
        if deduplicate_packages {
            if let Some(package_id) = self.task_data[data].package_id.clone() {
                if let Some(&owner) = registered.get(&package_id) {
                    if let Some(source) = source {
                        if owner != source {
                            redirects.push((source, owner, redirect_index));
                        }
                    }
                    return Ok(None);
                }
                if let Some(source) = source {
                    registered.insert(package_id, source);
                }
            }
        }
        Ok(Some(task))
    }

    /// tsgo collectFiles for one task after its subtasks: a redirect records
    /// its output's source, a file joins the program order and learns
    /// whether it was found searching node_modules.
    fn leave_collected_task(
        &mut self,
        task: usize,
        order: &mut Vec<usize>,
        ordinary_count: &mut usize,
        library_count: &mut usize,
        recorded_redirects: &mut HashSet<CanonicalPath>,
    ) {
        if let Some(redirect) = self.tasks[task].redirect {
            if let Some(output) = self.task_source(redirect) {
                let path = self.tasks[task].path.clone();
                if recorded_redirects.insert(path.canonical().clone()) {
                    self.sources[output]
                        .prepared
                        .remember_project_reference_source(path.clone());
                    self.project_reference_redirects.push((path, output));
                }
            }
            return;
        }
        if matches!(self.tasks[task].origin, TaskOrigin::AutomaticTypes) {
            return;
        }
        let Some(source) = self.tasks[task].source else {
            return;
        };
        let data = self.tasks[task].data;
        order.push(source);
        if self.sources[source].library_priority.is_some() {
            *library_count += 1;
        } else {
            *ordinary_count += 1;
        }
        self.sources[source].has_non_external_reason = self.task_data[data].lowest_depth == 0;
    }

    /// tsgo filesParser.addIncludeReason: a redirect's reason goes to its
    /// redirect task as queued (not to that task's alias), a loaded task's
    /// to its path.
    fn add_include_reason(&mut self, task: usize, reason: SourceInclusionReason) {
        let mut current = task;
        loop {
            if let Some(redirect) = self.tasks[current].redirect {
                current = redirect;
                continue;
            }
            if self.tasks[current].loaded {
                if let Some(source) = self.tasks[current].source {
                    self.sources[source].inclusion_reasons.push(reason);
                }
            }
            return;
        }
    }

    /// The roots' sources once the tasks ran, with the missing-root
    /// diagnostics (a root that is also a missing library reports the
    /// library's diagnostic in place of its own).
    fn resolve_root_sources(&mut self) {
        for index in 0..self.roots.len() {
            let Some(task) = self.roots[index].task else {
                continue;
            };
            let source = self.task_source(task);
            self.roots[index].source = source;
            let path = self.roots[index].path.clone();
            match source {
                Some(source) => self.sources[source]
                    .root_inclusions
                    .push(path.display().to_owned()),
                None => {
                    let diagnostic = match self
                        .missing_library_diagnostics
                        .get(&path.display().to_owned())
                    {
                        Some(diagnostic) => diagnostic.clone(),
                        None => {
                            let TaskOrigin::Root { spelling, reason } =
                                self.tasks[task].origin.clone()
                            else {
                                unreachable!("a root's task has a root origin");
                            };
                            let diagnostic = missing_root_diagnostic(spelling.as_js(), reason);
                            if self
                                .diagnosed_missing_roots
                                .insert(path.display().to_owned())
                            {
                                self.program_diagnostics.push(diagnostic.clone());
                            }
                            diagnostic
                        }
                    };
                    self.roots[index].missing_diagnostic = Some(diagnostic);
                }
            }
        }
    }

    /// tsgo leaves a resolved target out of the program when its task was
    /// elided (a JavaScript file beyond maxNodeModuleJsDepth) or when the
    /// output of a referenced project's source is not built; the checker
    /// reads why from the resolution.
    fn record_unloaded_targets(&mut self) -> Result<(), ProgramLoadError> {
        for index in 0..self.module_resolutions.len() {
            if self.module_resolutions[index].unloaded_reason.is_some()
                || !self.module_resolutions[index].loads_source
            {
                continue;
            }
            let (canonical, is_javascript) = match self.module_resolutions[index].host.outcome() {
                ResolutionOutcome::Resolved(target) => (
                    target.resolved_file().canonical().clone(),
                    target.extension().is_javascript(),
                ),
                ResolutionOutcome::NotFound => continue,
            };
            let loaded = self.task_data_by_path.get(&canonical).is_some_and(|&data| {
                self.task_data[data]
                    .spellings
                    .iter()
                    .any(|&(_, task)| self.task_source(task).is_some())
            });
            if loaded {
                continue;
            }
            let reason = if self.project_reference_redirect(&canonical)?.is_some() {
                UnloadedModuleReason::ProjectReferenceOutputNotBuilt
            } else if is_javascript {
                UnloadedModuleReason::NodeModulesDepth
            } else {
                continue;
            };
            self.module_resolutions[index].unloaded_reason = Some(reason);
        }
        Ok(())
    }
}

fn publish_program(
    mode: PreparedProgramMode,
    mut staged: CompleteGraph,
    packages: Vec<PackageMetadata>,
    dependency_symlink_resolutions: Vec<(ProgramPath, ProgramPath)>,
    path_context: PathContext,
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
) -> Result<PreparedProgram, ProgramLoadError> {
    let case_sensitive_file_names = path_context.use_case_sensitive_file_names();
    let mut builder = match mode {
        PreparedProgramMode::NoEmit => {
            PreparedProgram::builder(path_context, compiler_options.clone())
        }
        PreparedProgramMode::Emit => {
            PreparedProgram::emitting_builder(path_context, compiler_options.clone())
        }
    };
    builder = builder.with_dependency_symlink_resolutions(dependency_symlink_resolutions);
    // The package.json files every resolver of the load probed, each once.
    let mut package_json_probes = BTreeMap::new();
    for probe in staged.package_json_probes.iter().cloned() {
        package_json_probes
            .entry(crate::js_path::file_name_key(
                probe.path.as_js(),
                case_sensitive_file_names,
            ))
            .or_insert(probe);
    }
    builder = builder.with_package_json_probes(package_json_probes.into_values().collect());
    let config_file = program_options.config_file().cloned();
    let config_diagnostics = program_options.config_parsing_diagnostics().to_vec();
    let mut auxiliary_paths = HashSet::default();
    for source in program_options.config_parsing_sources() {
        auxiliary_paths.insert(source.path().canonical().clone());
        builder
            .add_auxiliary_file(source.clone())
            .map_err(|error| {
                ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
            })?;
    }
    if let Some(config_file) = &config_file {
        auxiliary_paths.insert(config_file.path().canonical().clone());
    }
    // The configs of the referenced projects: a diagnostic at a reference
    // of a referenced project is located in that project's config.
    if let Some(references) = program_options.project_references() {
        for project in references.projects() {
            let path = crate::module_resolution::make_program_path(
                project.config_file_name(),
                case_sensitive_file_names,
            )
            .map_err(|error| {
                ProgramLoadError::resolution_js(
                    ProgramLoadOperation::BuildPreparedProgram,
                    Some(project.config_file_name().to_owned()),
                    None,
                    error,
                )
            })?;
            if !auxiliary_paths.insert(path.canonical().clone()) {
                continue;
            }
            builder
                .add_auxiliary_file(PreparedAuxiliaryFile::from_snapshot(
                    path,
                    Arc::clone(project.plan().source().snapshot()),
                ))
                .map_err(|error| {
                    ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
                })?;
        }
    }
    builder.set_program_options(program_options);

    if let Some(config_file) = config_file {
        builder
            .add_auxiliary_file(PreparedAuxiliaryFile::from_snapshot(
                config_file.path().clone(),
                std::sync::Arc::clone(config_file.snapshot()),
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
            .with_may_be_emitted(may_be_emitted)
            .with_may_emit_forced_declaration(
                staged_source.prepared.may_emit_forced_declaration()
                    && staged_source.has_non_external_reason,
            )
            .with_inclusion_reasons(staged_source.inclusion_reasons.clone());
        let source_id = builder.add_source_file(prepared.clone()).map_err(|error| {
            ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
        })?;
        if staged_source.library_priority.is_some() {
            builder.add_library_file(source_id).map_err(|error| {
                ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
            })?;
        }
        published_ids[source_index] = Some(source_id);
        source_by_canonical.insert(prepared.path().canonical().clone(), source_id);
        for (redirected, output) in &staged.project_reference_redirects {
            if *output == source_index {
                source_by_canonical.insert(redirected.canonical().clone(), source_id);
            }
        }
        for redirect in prepared.package_redirect_paths() {
            if let Some(previous) =
                source_by_canonical.insert(redirect.canonical().clone(), source_id)
            {
                if previous != source_id {
                    return Err(ProgramLoadError::invalid_data_js(
                        ProgramLoadOperation::BuildPreparedProgram,
                        Some(redirect.display().to_owned()),
                        "package redirect identity belongs to more than one staged source",
                    ));
                }
            }
        }
    }

    let package_redirect_files = std::mem::take(&mut staged.package_redirect_files)
        .into_iter()
        .map(
            |(path, target, index, reasons)| crate::PackageRedirectFile {
                path,
                target: published_ids[target].expect("postorder publishes every staged source"),
                index,
                reasons,
            },
        )
        .collect();
    builder = builder.with_package_redirect_files(package_redirect_files);

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
        let key_path = resolution.key.source().as_js().to_owned();
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
            ProgramLoadError::resolution_js(
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
        let key_path = resolution.key.origin().canonical_path().as_js().to_owned();
        let specifier = resolution.key.specifier().to_owned();
        let bound =
            bind_type_resolution(resolution.host, &source_by_canonical).map_err(|error| {
                ProgramLoadError::resolution_js(
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
        config_diagnostics,
        staged.option_diagnostics,
        staged.program_diagnostics,
    ));
    builder.build().map_err(|error| {
        ProgramLoadError::preparation(ProgramLoadOperation::BuildPreparedProgram, error)
    })
}

fn bind_module_resolution(
    host: HostModuleResolution,
    source_by_canonical: &BTreeMap<CanonicalPath, SourceFileId>,
    package_map: &BTreeMap<JsString, bool>,
    loads_source: bool,
    unloaded_reason: Option<UnloadedModuleReason>,
    no_resolve: bool,
) -> Result<ModuleResolution, ResolutionError> {
    let alternate_result = host.alternate_result().cloned();
    let (outcome, diagnostics) = host.into_parts();
    let ResolutionOutcome::Resolved(module) = outcome else {
        let mut resolution = ModuleResolution::not_found().with_diagnostics(diagnostics);
        if let Some(alternate_result) = alternate_result {
            resolution = resolution.with_alternate_result(alternate_result);
        }
        return Ok(resolution);
    };
    let (types_package_exists, package_bundles_types) =
        module.package_id().map_or((false, false), |package_id| {
            (
                package_map.contains_key(&types_package_name(package_id.name())),
                package_map
                    .get(package_id.name().as_bytes())
                    .copied()
                    .unwrap_or(false),
            )
        });
    let owned_source = source_by_canonical.get(module.resolved_file().canonical());
    let target = if no_resolve && owned_source.is_none() {
        ResolvedModuleTarget::Unloaded {
            resolved_file: module.resolved_file().clone(),
            reason: unloaded_reason.unwrap_or(UnloadedModuleReason::NoResolve),
        }
    } else if let (None, Some(reason)) = (owned_source, unloaded_reason) {
        ResolvedModuleTarget::Unloaded {
            resolved_file: module.resolved_file().clone(),
            reason,
        }
    } else if module.extension().is_javascript() && owned_source.is_none() {
        return Err(ResolutionError::unsupported(
            "unexplained-unloaded-javascript",
            format!(
                "resolved JavaScript target {} has no source-membership exclusion",
                module.resolved_file().display().to_string_lossy()
            ),
        ));
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
                module.resolved_file().display().to_string_lossy()
            ),
        ));
    };
    let mut resolution = ModuleResolution::resolved(module.into_resolved_module(target)?)
        .with_diagnostics(diagnostics)
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
            host.resolved_file().display().to_string_lossy()
        )));
    };
    let target = host.resolved_file().clone();
    Ok(TypeReferenceResolution::resolved(
        host.into_resolved_type_reference_directive(target, *source)?,
    ))
}

fn package_map_from_facts<'a>(
    facts: impl IntoIterator<Item = (&'a PackageId, &'a ModuleExtension)>,
) -> BTreeMap<JsString, bool> {
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

/// A wildcard automatic-type discovery failure, retained with the exact
/// path so the loader can map it to its established `ProgramLoadError`
/// variants and the L2.3 cache can map it to a `ResolutionError`.
pub(crate) enum WildcardDiscoveryError {
    Host {
        path: JsString,
        error: tsc_host::HostError,
    },
    Decode {
        path: JsString,
        source: crate::text::HostTextDecodeError,
    },
    InvalidData {
        path: JsString,
        detail: &'static str,
    },
}

/// Enumerate wildcard automatic type directive names below the effective
/// type roots with the vendored host-operation order: a missing root is
/// skipped, every immediate directory is a candidate, a `package.json` with a
/// null `typings` field is excluded, and the hidden-directory filter applies
/// only after that manifest probe.
///
/// tsc-port: getAutomaticTypeDirectiveNames @6.0.3
/// tsc-hash: db3bfa1a221287c533bc20917feaad712bfe322265c8c09aaded542ab0ab4461
/// tsc-span: _tsc.js:40294-40320
pub(crate) fn discover_wildcard_type_directive_names(
    host: &dyn CompilerHost,
    roots: &[JsString],
) -> Result<Vec<JsString>, WildcardDiscoveryError> {
    let mut matches = Vec::new();
    for root in roots {
        let root_path: JsStr<'_> = root.into();
        if !host
            .directory_exists_js(root_path)
            .map_err(|error| WildcardDiscoveryError::Host {
                path: root_path.to_owned(),
                error,
            })?
        {
            continue;
        }
        let directories =
            host.get_directories_js(root_path)
                .map_err(|error| WildcardDiscoveryError::Host {
                    path: root_path.to_owned(),
                    error,
                })?;
        for directory in directories {
            let name = crate::js_path::base_file_name(directory.as_js());
            if name.is_empty() {
                return Err(WildcardDiscoveryError::InvalidData {
                    path: directory,
                    detail: "automatic type directory has no base name",
                });
            }
            let package_directory = crate::js_path::combine_paths(root_path, name.as_js());
            let package_json =
                crate::js_path::combine_paths(package_directory.as_js(), "package.json".into());
            if wildcard_package_has_null_typings(host, package_json.as_js())? {
                continue;
            }
            // TypeScript probes package.json before applying the hidden
            // directory filter, so retain that observable failure order.
            if !name.starts_with(".") {
                matches.push(name);
            }
        }
    }
    Ok(matches)
}

fn wildcard_package_has_null_typings(
    host: &dyn CompilerHost,
    package_json: JsStr<'_>,
) -> Result<bool, WildcardDiscoveryError> {
    if !host
        .file_exists_js(package_json)
        .map_err(|error| WildcardDiscoveryError::Host {
            path: package_json.to_owned(),
            error,
        })?
    {
        return Ok(false);
    }
    let Some(bytes) =
        host.read_file_js(package_json)
            .map_err(|error| WildcardDiscoveryError::Host {
                path: package_json.to_owned(),
                error,
            })?
    else {
        return Ok(false);
    };
    let text = decode_host_text(bytes).map_err(|source| WildcardDiscoveryError::Decode {
        path: package_json.to_owned(),
        source,
    })?;
    let (_, object) = parse_json_object(package_json, text);
    Ok(json_object_get(&object, "typings").is_some_and(crate::JsonValue::is_null))
}

pub(crate) fn implied_node_format(
    file_name: JsStr<'_>,
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
                .split_ascii(b'/')
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

/// Whether [`implied_node_format`] consults the package scope for this file
/// name. Only such files need the sequential resolver before their parse
/// options are known; the parse-ahead skips them.
fn implied_node_format_needs_package_scope(
    file_name: JsStr<'_>,
    options: &CompilerOptions,
) -> bool {
    if file_name.ends_with(".mts")
        || file_name.ends_with(".mjs")
        || file_name.ends_with(".cts")
        || file_name.ends_with(".cjs")
    {
        return false;
    }
    let package_eligible = file_name.ends_with(".ts")
        || file_name.ends_with(".tsx")
        || file_name.ends_with(".js")
        || file_name.ends_with(".jsx");
    package_eligible
        && (matches!(options.emit_module_resolution_kind(), 3..=99)
            || file_name
                .split_ascii(b'/')
                .any(|segment| segment == "node_modules"))
}

fn implied_node_format_for_emit(
    file_name: JsStr<'_>,
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
    TYPESCRIPT_SOURCE_EXTENSIONS
        .iter()
        .any(|extension| path.as_js().ends_with(extension))
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
    path.as_js().ends_with(".json")
}

fn is_javascript_source(path: &CanonicalPath) -> bool {
    JAVASCRIPT_SOURCE_EXTENSIONS
        .iter()
        .any(|extension| path.as_js().ends_with(extension))
}

fn path_contains_node_modules(path: JsStr<'_>) -> bool {
    path.split_ascii(b'/')
        .any(|component| component == "node_modules")
}

/// tsc-port: getResolutionDiagnostic @6.0.3 (source-admission projection)
/// tsc-hash: 6a5b5f0cdb2e104edc5249432808de1d68f96a32df84f5d0437d28669a348431
/// tsc-span: _tsc.js:125687-125721
///
/// A successful resolution can still carry a diagnostic that prevents
/// `processImportedModules` from calling `findSourceFile`. Keep that verdict
/// separate from extension loadability: `.tsx` is ordinarily a TypeScript
/// source, yet without a JSX mode it is resolution-only just like `.jsx`.
fn resolution_diagnostic_unloaded_reason(
    extension: &ModuleExtension,
    options: &CompilerOptions,
    containing_file_is_declaration: bool,
    loads_source: bool,
) -> Option<UnloadedModuleReason> {
    match extension {
        ModuleExtension::Tsx | ModuleExtension::Jsx if options.jsx.unwrap_or(0) == 0 => {
            Some(UnloadedModuleReason::JsxWithoutJsxOption)
        }
        ModuleExtension::Arbitrary(_)
            if loads_source
                && !containing_file_is_declaration
                && options.allow_arbitrary_extensions != Some(true) =>
        {
            Some(UnloadedModuleReason::ArbitraryExtensionWithoutOption)
        }
        _ => None,
    }
}

/// tsgo shouldAddFile for a JavaScript target: a name that adds no file
/// (an augmentation's) is resolution only, and JavaScript is admitted only
/// under `allowJs`. Under `allowJs` the depth elision is the subtask's
/// (`elideOnDepth`); without it no task exists, and the reason keeps
/// TypeScript's order at the first external layer (the depth before
/// `allowJs`), which the checker's provider reads.
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
    if !loads_source {
        return Some(UnloadedModuleReason::ResolutionOnly);
    }
    if options.allow_js {
        return None;
    }
    if external
        && (!has_original_path || path_contains_node_modules(resolved_file.as_js()))
        && options.node_modules_depth_exceeds_limit(1)
    {
        return Some(UnloadedModuleReason::NodeModulesDepth);
    }
    Some(UnloadedModuleReason::JavaScriptNotAdmitted)
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
    root_spelling: JsStr<'_>,
    allow_js: bool,
    root_reason: RootFileReason,
) -> Result<Diagnostic, ProgramLoadError> {
    let javascript = is_javascript_source(path.canonical());
    let path = root_spelling.to_owned();
    let (message, arguments) = if javascript {
        (
            &gen::File_0_is_a_JavaScript_file_Did_you_mean_to_enable_the_allowJs_option,
            vec![path],
        )
    } else {
        (
            &gen::File_0_has_an_unsupported_extension_The_only_supported_extensions_are_1,
            vec![
                path,
                JsString::from(supported_source_extension_list(allow_js)),
            ],
        )
    };
    let root_reason = root_file_reason_message(&root_reason, &same_file_name);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Ok(Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js(message, &arguments).with_next(vec![inclusion]),
    ))
}

fn missing_root_diagnostic(path: JsStr<'_>, root_file_reason: RootFileReason) -> Diagnostic {
    let root_reason = root_file_reason_message(&root_file_reason, &same_file_name);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js_parts(&gen::File_0_not_found, &[path]).with_next(vec![inclusion]),
    )
}

fn unresolved_extensionless_root_diagnostic(
    path: JsStr<'_>,
    allow_js: bool,
    root_reason: RootFileReason,
) -> Result<Diagnostic, ProgramLoadError> {
    let root_reason = root_file_reason_message(&root_reason, &same_file_name);
    let inclusion = MessageChain::new(&gen::The_file_is_in_the_program_because, &[])
        .with_next(vec![root_reason]);
    Ok(Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js_parts(
            &gen::Could_not_resolve_the_path_0_with_the_extensions_1,
            &[path, supported_source_extension_list(allow_js).into()],
        )
        .with_next(vec![inclusion]),
    ))
}

/// tsc-port: fileIncludeReasonToDiagnostics @6.0.3 (RootFile)
/// tsc-hash: 30e07b28f72a81d3eb29d0ab7e49d8d2a65a20dedc61205c00e488973787233a
/// tsc-span: _tsc.js:129341-129369
/// The identity file-name conversion of the diagnostics (tsgo
/// `toDiagnostic(program, false)`); explainFiles passes a relative one.
fn same_file_name(name: JsStr<'_>) -> JsString {
    name.to_owned()
}

fn root_file_reason_message(
    reason: &RootFileReason,
    to_file_name: &dyn Fn(JsStr<'_>) -> JsString,
) -> MessageChain {
    match reason {
        RootFileReason::Explicit => {
            MessageChain::new(&gen::Root_file_specified_for_compilation, &[])
        }
        RootFileReason::FilesList { .. } => {
            MessageChain::new(&gen::Part_of_files_list_in_tsconfig_json, &[])
        }
        RootFileReason::IncludePattern { spec, config_file } => MessageChain::new_js(
            &gen::Matched_by_include_pattern_0_in_1,
            &[spec.as_ref().clone(), to_file_name(config_file.as_js())],
        ),
        RootFileReason::DefaultInclude => {
            MessageChain::new(&gen::Matched_by_default_include_pattern, &[])
        }
    }
}

fn missing_library_root_diagnostic(path: &ProgramPath, reason: &LibraryRootReason) -> Diagnostic {
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
    let diagnostic = Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js_parts(&gen::File_0_not_found, &[path.display()])
            .with_next(vec![inclusion]),
    );
    // tsgo toRelatedInformation (fileIncludeKindLibFile) looks the target up
    // with GetCallbackForFindingPropertyAssignmentByValue, which matches
    // array elements only, so a string `target` never yields "File is
    // default library for target specified here".
    diagnostic
}

fn automatic_type_reference_diagnostic(
    name: JsStr<'_>,
    uses_wildcard: bool,
    config_file: Option<&ProgramConfigFile>,
) -> Diagnostic {
    let reason = MessageChain::new_js(
        if uses_wildcard {
            &gen::Entry_point_for_implicit_type_library_0
        } else {
            &gen::Entry_point_of_type_library_0_specified_in_compilerOptions
        },
        &[name.to_owned()],
    );
    let inclusion =
        MessageChain::new_js(&gen::The_file_is_in_the_program_because, &[]).with_next(vec![reason]);
    let mut diagnostic = Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js(
            &gen::Cannot_find_type_definition_file_for_0,
            &[name.to_owned()],
        )
        .with_next(vec![inclusion]),
    );
    let syntax_name = if uses_wildcard { "*".into() } else { name };
    if let Some((config_file, location)) = config_file.and_then(|config_file| {
        config_file
            .automatic_type_directive_location(syntax_name)
            .map(|location| (config_file, location))
    }) {
        diagnostic.related_information_present = true;
        diagnostic.related.push(RelatedInfo {
            file_name: Some(config_file.path().display().to_owned()),
            start: Some(location.start()),
            length: Some(location.length()),
            message: MessageChain::new_js(
                &gen::File_is_entry_point_of_type_library_specified_here,
                &[],
            ),
        });
    }
    diagnostic
}

/// tsgo `core.ScriptTarget.String()`: the target's display name
/// (`Default library for target 'ES2020'`).
fn script_target_name(options: &CompilerOptions) -> &'static str {
    match options.emit_script_target().bits() {
        0 => "None",
        1 => "ES5",
        2 => "ES2015",
        3 => "ES2016",
        4 => "ES2017",
        5 => "ES2018",
        6 => "ES2019",
        7 => "ES2020",
        8 => "ES2021",
        9 => "ES2022",
        10 => "ES2023",
        11 => "ES2024",
        12 => "ES2025",
        13 => "ES2026",
        99 => "ESNext",
        100 => "JSON",
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
    incoming: JsStr<'_>,
    existing_reasons: &[SourceInclusionReason],
    incoming_reason: &SourceInclusionReason,
    config_file: Option<&ProgramConfigFile>,
) -> Diagnostic {
    casing_diagnostic(
        existing,
        incoming,
        existing_reasons,
        incoming_reason,
        config_file,
        true,
    )
}

/// Publish the same explaining diagnostic for two independently loaded files
/// on a case-sensitive host. The incoming reason belongs to the second source
/// rather than `existing_reasons`, matching tsc's `filesByNameIgnoreCase`
/// branch.
///
/// tsc-port: findSourceFileWorker @6.0.3 (`filesByNameIgnoreCase`)
/// tsc-hash: 3fe29e4e1cb6c2e1b58594265c7e7140e0719cb08bedf239bebd99046cc3795b
/// tsc-span: _tsc.js:124396-124402
fn casing_distinct_file_diagnostic(
    existing: &PreparedSourceFile,
    incoming: JsStr<'_>,
    existing_reasons: &[SourceInclusionReason],
    incoming_reason: &SourceInclusionReason,
    config_file: Option<&ProgramConfigFile>,
) -> Diagnostic {
    casing_diagnostic(
        existing,
        incoming,
        existing_reasons,
        incoming_reason,
        config_file,
        false,
    )
}

/// tsc-port: createDiagnosticForOption @6.0.3
/// tsc-hash: 24da25470bdd02c4cde5520b78ea191837823bf1df686438144a8106edfd5f53
/// tsc-span: _tsc.js:125368-125386
fn append_output_option_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    config: Option<&ProgramConfigFile>,
    names: &[&str],
    message: MessageChain,
) {
    let Some(config) = config else {
        diagnostics.push(Diagnostic::new_js(None, None, None, message));
        return;
    };
    let mut locations = names
        .iter()
        .flat_map(|name| config.compiler_option_name_locations(*name))
        .copied()
        .collect::<Vec<_>>();
    locations.sort_by_key(|location| location.start());
    if locations.is_empty() {
        locations.extend(config.compiler_options_location());
    }
    if locations.is_empty() {
        diagnostics.push(Diagnostic::new_js(None, None, None, message));
    } else {
        for location in locations {
            diagnostics.push(Diagnostic::new_js(
                Some(config.diagnostic_file_name().to_owned()),
                Some(location.start()),
                Some(location.length()),
                message.clone(),
            ));
        }
    }
}

/// The rootDir violation of `source` (tsc checkSourceFilesBelongToPath),
/// explained like every program diagnostic about a file.
fn root_directory_diagnostic(
    source: &StagedSource,
    root: JsStr<'_>,
    config: Option<&ProgramConfigFile>,
    options: &CompilerOptions,
    package: Option<&PackageMetadata>,
) -> Diagnostic {
    explaining_file_diagnostic(
        source,
        MessageChain::new_js_parts(
            &gen::File_0_is_not_under_rootDir_1_rootDir_is_expected_to_contain_all_source_files,
            &[source.prepared.path().display(), root],
        ),
        config,
        options,
        package,
    )
}

/// The composite file-list violation of `source` (tsc verifyCompilerOptions:
/// a composite project lists every file it would emit).
fn file_list_diagnostic(
    source: &StagedSource,
    project: JsStr<'_>,
    config: Option<&ProgramConfigFile>,
    options: &CompilerOptions,
    package: Option<&PackageMetadata>,
) -> Diagnostic {
    explaining_file_diagnostic(
        source,
        MessageChain::new_js_parts(
            &gen::File_0_is_not_listed_within_the_file_list_of_project_1_Projects_must_list_all_files_or_use_an_include_pattern,
            &[source.prepared.path().display(), project],
        ),
        config,
        options,
        package,
    )
}

/// tsc-port: createDiagnosticExplainingFile @6.0.3
/// tsc-hash: a52da4c2aafdb0c939e2bf00de5064eb03340c4858ad40378af65b5b6c9de41d
/// tsc-span: _tsc.js:125851-125932
fn explaining_file_diagnostic(
    source: &StagedSource,
    mut message: MessageChain,
    config: Option<&ProgramConfigFile>,
    options: &CompilerOptions,
    package: Option<&PackageMetadata>,
) -> Diagnostic {
    let reasons = &source.inclusion_reasons;
    let located = reasons.iter().enumerate().find_map(|(index, reason)| {
        source_inclusion_location(reason).map(|location| (index, location))
    });
    if !reasons.is_empty() && (reasons.len() != 1 || located.is_none()) {
        message = message.with_next(vec![MessageChain::new(
            &gen::The_file_is_in_the_program_because,
            &[],
        )
        .with_next(
            reasons
                .iter()
                .filter_map(|reason| source_inclusion_reason_message(reason, &same_file_name))
                .collect(),
        )]);
    }
    if let Some(detail) =
        root_module_format_detail(&source.prepared, package, options, &same_file_name)
    {
        message.next_present = true;
        message.next.push(detail);
    }
    let (file, start, length) =
        located
            .as_ref()
            .map_or((None, None, None), |(_, (file, start, end))| {
                (
                    Some(file.clone()),
                    Some(*start),
                    Some(end.saturating_sub(*start)),
                )
            });
    let mut diagnostic = Diagnostic::new_js(file, start, length, message);
    for (index, reason) in reasons.iter().enumerate() {
        if located
            .as_ref()
            .is_some_and(|(location_index, _)| *location_index == index)
        {
            continue;
        }
        if let Some(related) = root_inclusion_related_information(reason, config) {
            diagnostic.related.push(related);
        } else if let Some((file, start, end)) = source_inclusion_location(reason) {
            let message = match reason {
                SourceInclusionReason::Import { .. } => &gen::File_is_included_via_import_here,
                SourceInclusionReason::PathReference { .. } => {
                    &gen::File_is_included_via_reference_here
                }
                SourceInclusionReason::TypeReference { .. } => {
                    &gen::File_is_included_via_type_library_reference_here
                }
                _ => unreachable!("only reference reasons carry source locations"),
            };
            diagnostic.related.push(RelatedInfo {
                file_name: Some(file),
                start: Some(start),
                length: Some(end.saturating_sub(start)),
                message: MessageChain::new(message, &[]),
            });
        }
    }
    diagnostic.related_information_present = !diagnostic.related.is_empty();
    diagnostic
}

/// tsc-port: explainIfFileIsRedirectAndImpliedFormat @6.0.3 (module-format branch)
/// tsc-hash: 4bd1d72257a11fc0d58f2ff3b8609170d5f225f9a8b5d801b67751dfbf9e001a
/// tsc-span: _tsc.js:129225-129275
/// The module-format line of tsgo `explainRedirectAndImpliedFormat`, read
/// through `loadSourceFileMetaData`'s facts: the package scope's string
/// `type` counts for a file of a non-fixed extension under node16/nodenext
/// resolution, or for any file under `node_modules`; the scope's existence
/// (`PackageJsonDirectory`) counts always. The port keeps no string value
/// for a non-string `type` field, which tsgo reads as absent as well.
fn root_module_format_detail(
    source: &PreparedSourceFile,
    package: Option<&PackageMetadata>,
    options: &CompilerOptions,
    to_file_name: &dyn Fn(JsStr<'_>) -> JsString,
) -> Option<MessageChain> {
    if source.is_external_module() != Some(true) {
        return None;
    }
    let name = source.path().display();
    let fixed_extension = [".mts", ".mjs", ".cts", ".cjs"]
        .iter()
        .any(|extension| name.ends_with(extension));
    let type_counts = (!fixed_extension && matches!(options.emit_module_resolution_kind(), 3..=99))
        || name
            .split_ascii(b'/')
            .any(|segment| segment == "node_modules");
    let package_json_type = package
        .filter(|_| type_counts)
        .map(PackageMetadata::module_type)
        .filter(|module_type| *module_type != PackageJsonType::Unspecified);
    let package_json = |package: &PackageMetadata| to_file_name(package.package_json().display());
    match source.implied_node_format_for_emit()? {
        ResolutionMode::EsNext => match package {
            Some(package) if package_json_type == Some(PackageJsonType::Module) => {
                Some(MessageChain::new_js(
                    &gen::File_is_ECMAScript_module_because_0_has_field_type_with_value_module,
                    &[package_json(package)],
                ))
            }
            _ => None,
        },
        ResolutionMode::CommonJs => Some(match package {
            Some(package) if package_json_type.is_some() => MessageChain::new_js(
                &gen::File_is_CommonJS_module_because_0_has_field_type_whose_value_is_not_module,
                &[package_json(package)],
            ),
            Some(package) => MessageChain::new_js(
                &gen::File_is_CommonJS_module_because_0_does_not_have_field_type,
                &[package_json(package)],
            ),
            None => MessageChain::new(
                &gen::File_is_CommonJS_module_because_package_json_was_not_found,
                &[],
            ),
        }),
        ResolutionMode::Unspecified => None,
    }
}

fn root_inclusion_related_information(
    reason: &SourceInclusionReason,
    config: Option<&ProgramConfigFile>,
) -> Option<RelatedInfo> {
    let SourceInclusionReason::Root(reason) = reason else {
        return None;
    };
    let (option, spec, message) = match reason {
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
        RootFileReason::Explicit | RootFileReason::DefaultInclude => return None,
    };
    let config = config?;
    let location = config.root_option_array_location(option, spec.as_ref())?;
    Some(RelatedInfo {
        file_name: Some(config.path().display().to_owned()),
        start: Some(location.start()),
        length: Some(location.length()),
        message: MessageChain::new(message, &[]),
    })
}

fn casing_diagnostic(
    existing: &PreparedSourceFile,
    incoming: JsStr<'_>,
    existing_reasons: &[SourceInclusionReason],
    incoming_reason: &SourceInclusionReason,
    config_file: Option<&ProgramConfigFile>,
    incoming_reason_is_recorded_on_existing: bool,
) -> Diagnostic {
    let existing_name = existing.path().display();
    let incoming_name = incoming;
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
    let mut all_reasons = existing_reasons.iter().collect::<Vec<_>>();
    // A host-collapsed alias is recorded on the existing source before
    // `finish`; an independently loaded case-sensitive source is not.
    if !incoming_reason_is_recorded_on_existing {
        all_reasons.push(incoming_reason);
    }
    let mut reasons = all_reasons
        .iter()
        .filter_map(|reason| source_inclusion_reason_message(reason, &same_file_name))
        .collect::<Vec<_>>();
    // Root aliases retain one reason per explicit root occurrence (the
    // program-preprocessing contract exposes that multiplicity).  A root
    // spelling arriving after an import/reference is different: tsc reports
    // the dependency chain once and does not repeat the same files-list
    // reason for the alias occurrence.
    if incoming_reason_is_recorded_on_existing && root_arrived_after_reference {
        reasons.dedup();
    }
    let message = MessageChain::new_js(message, &arguments).with_next(vec![MessageChain::new(
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
    let mut diagnostic = Diagnostic::new_js(file_name, start, length, message);
    for reason in all_reasons {
        if let Some(related) = root_inclusion_related_information(reason, config_file) {
            diagnostic.related.push(related);
        }
    }
    diagnostic.related_information_present = !diagnostic.related.is_empty();
    diagnostic
}

fn source_inclusion_reason_message(
    reason: &SourceInclusionReason,
    to_file_name: &dyn Fn(JsStr<'_>) -> JsString,
) -> Option<MessageChain> {
    match reason {
        SourceInclusionReason::Root(root) => Some(root_file_reason_message(root, to_file_name)),
        SourceInclusionReason::Import {
            parent,
            reference_text,
            package_id,
            ..
        } => Some(match package_id {
            Some(package_id) => MessageChain::new_js(
                &gen::Imported_via_0_from_file_1_with_packageId_2,
                &[
                    reference_text.clone().into(),
                    to_file_name(parent.as_js()),
                    package_id.clone(),
                ],
            ),
            None => MessageChain::new_js(
                &gen::Imported_via_0_from_file_1,
                &[reference_text.clone().into(), to_file_name(parent.as_js())],
            ),
        }),
        SourceInclusionReason::PathReference {
            parent, specifier, ..
        } => Some(MessageChain::new_js(
            &gen::Referenced_via_0_from_file_1,
            &[specifier.clone(), to_file_name(parent.as_js())],
        )),
        // tsgo getReferencedLocation gives a type reference directive no
        // package id (only an import's location has one), so the reason
        // never says "with packageId" whatever the resolution attached.
        SourceInclusionReason::TypeReference {
            parent, specifier, ..
        } => Some(MessageChain::new_js(
            &gen::Type_library_referenced_via_0_from_file_1,
            &[specifier.clone(), to_file_name(parent.as_js())],
        )),
        SourceInclusionReason::AutomaticType {
            name,
            package_id,
            implicit,
        } => Some(match (implicit, package_id) {
            (false, Some(package_id)) => MessageChain::new_js(
                &gen::Entry_point_of_type_library_0_specified_in_compilerOptions_with_packageId_1,
                &[name.clone(), package_id.clone()],
            ),
            (false, None) => MessageChain::new_js(
                &gen::Entry_point_of_type_library_0_specified_in_compilerOptions,
                std::slice::from_ref(name),
            ),
            (true, Some(package_id)) => MessageChain::new_js(
                &gen::Entry_point_for_implicit_type_library_0_with_packageId_1,
                &[name.clone(), package_id.clone()],
            ),
            (true, None) => MessageChain::new_js(
                &gen::Entry_point_for_implicit_type_library_0,
                std::slice::from_ref(name),
            ),
        }),
        SourceInclusionReason::LibraryRoot(LibraryRootReason::Default { target }) => {
            Some(MessageChain::new(
                &gen::Default_library_for_target_0,
                std::slice::from_ref(target),
            ))
        }
        SourceInclusionReason::LibraryRoot(LibraryRootReason::Explicit { file_name }) => {
            Some(MessageChain::new(
                &gen::Library_0_specified_in_compilerOptions,
                std::slice::from_ref(file_name),
            ))
        }
        SourceInclusionReason::LibraryReference { parent, specifier } => {
            Some(MessageChain::new_js(
                &gen::Library_referenced_via_0_from_file_1,
                &[specifier.clone(), to_file_name(parent.as_js())],
            ))
        }
        SourceInclusionReason::Synthetic => None,
    }
}

impl PreparedProgram {
    /// tsgo `Program.ExplainFiles`: every file of the program in order (a
    /// deduplicated package copy at the place it was collected), each as its
    /// name relative to `current_directory` with the lines explaining why it
    /// is in the program, what it redirects to, or which module format it
    /// has (`explainRedirectAndImpliedFormat`).
    pub fn explain_files(&self, current_directory: JsStr<'_>) -> Vec<(JsString, Vec<JsString>)> {
        let case_sensitive = self.path_context().use_case_sensitive_file_names();
        let to_file_name = |name: JsStr<'_>| -> JsString {
            crate::output_directories::relative_path_from_directory(
                current_directory,
                name,
                case_sensitive,
            )
        };
        let packages: FxHashMap<&CanonicalPath, &PackageMetadata> = self
            .packages()
            .map(|package| (package.package_json().canonical(), package))
            .collect();
        let sources = self.source_files();
        let mut redirects = self.package_redirect_files().to_vec();
        redirects.sort_by_key(|redirect| redirect.index);
        let mut explained: Vec<(JsString, Vec<JsString>)> = Vec::new();
        let explain_source = |source: &PreparedSourceFile| -> (JsString, Vec<JsString>) {
            let mut lines: Vec<JsString> = source
                .inclusion_reasons()
                .iter()
                .filter_map(|reason| source_inclusion_reason_message(reason, &to_file_name))
                .map(|chain| chain.text)
                .collect();
            if let Some(origin) = source.project_reference_source_paths().last() {
                lines.push(
                    MessageChain::new_js(
                        &gen::File_is_output_of_project_reference_source_0,
                        &[to_file_name(origin.display())],
                    )
                    .text,
                );
            }
            let package = source
                .package_scope()
                .and_then(|scope| packages.get(scope).copied());
            if let Some(detail) =
                root_module_format_detail(source, package, self.compiler_options(), &to_file_name)
            {
                lines.push(detail.text);
            }
            (to_file_name(source.path().display()), lines)
        };
        let explain_redirect =
            |redirect: &crate::PackageRedirectFile| -> (JsString, Vec<JsString>) {
                let mut lines: Vec<JsString> = redirect
                    .reasons
                    .iter()
                    .filter_map(|reason| source_inclusion_reason_message(reason, &to_file_name))
                    .map(|chain| chain.text)
                    .collect();
                let target = &sources[redirect.target.index()];
                lines.push(
                    MessageChain::new_js(
                        &gen::File_redirects_to_file_0,
                        &[to_file_name(target.path().display())],
                    )
                    .text,
                );
                (to_file_name(redirect.path.display()), lines)
            };
        let mut next_source = 0usize;
        for redirect in &redirects {
            while explained.len() < redirect.index && next_source < sources.len() {
                explained.push(explain_source(&sources[next_source]));
                next_source += 1;
            }
            explained.push(explain_redirect(redirect));
        }
        while next_source < sources.len() {
            explained.push(explain_source(&sources[next_source]));
            next_source += 1;
        }
        explained
    }
}

fn source_inclusion_location(reason: &SourceInclusionReason) -> Option<(JsString, u32, u32)> {
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
    Some((parent.clone(), pos, end))
}

fn located_diagnostic<A: tsc_diagnostics::DiagnosticArgument>(
    source: &PreparedSourceFile,
    start: u32,
    length: u32,
    message: &'static tsc_diagnostics::DiagnosticMessage,
    args: &[A],
) -> Result<Diagnostic, ProgramLoadError> {
    let file_name = source.path().display();
    Ok(Diagnostic::new_js(
        Some(file_name.to_owned()),
        Some(start),
        Some(length),
        MessageChain::new_js(
            message,
            &args
                .iter()
                .map(|arg| arg.diagnostic_value().to_owned())
                .collect::<Vec<_>>(),
        ),
    ))
}

/// JavaScript root-name entry point; retains every UTF-16 unit until the host boundary.
pub fn load_program_js(
    host: &dyn CompilerHost,
    root_names: &[JsString],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_program_worker(
        PreparedProgramMode::NoEmit,
        host,
        RootNames::Js(root_names),
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
        None,
    )
}

/// JavaScript root-name entry point; retains every UTF-16 unit until the host boundary.
pub fn load_emitting_program_js(
    host: &dyn CompilerHost,
    root_names: &[JsString],
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_program_worker(
        PreparedProgramMode::Emit,
        host,
        RootNames::Js(root_names),
        compiler_options,
        program_options,
        Some(library_catalog),
        false,
        limits,
        None,
    )
}

/// The `/node_modules/` membership test on a display path, evaluated on the
/// JavaScript string itself: either separator counts on both sides, exactly
/// like the former `replace('\\', "/").contains("/node_modules/")`, and no
/// UTF-8 projection of the path is made.
fn display_path_contains_node_modules(path: JsStr<'_>) -> bool {
    const NAME: &[u8] = b"node_modules";
    let is_separator = |byte: u8| byte == b'/' || byte == b'\\';
    path.as_bytes().windows(NAME.len() + 2).any(|window| {
        is_separator(window[0])
            && &window[1..=NAME.len()] == NAME
            && is_separator(window[NAME.len() + 1])
    })
}

#[cfg(test)]
#[path = "../tests/unit/loader/node_modules_membership_tests.rs"]
mod node_modules_membership_tests;

#[cfg(test)]
#[path = "../tests/unit/loader/typed_error_source_tests.rs"]
mod typed_error_source_tests;
