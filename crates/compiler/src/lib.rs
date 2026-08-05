#![forbid(unsafe_code)]

//! One-shot execution of an owned H0 prepared program.
//!
//! This crate is the dependency boundary between the owned program contract
//! and the parser/binder/checker implementation. A [`ProgramSession`] owns
//! exactly one [`PreparedProgram`], projects its already-final source order
//! into the checker, and is consumed by [`ProgramSession::run`].

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use tsc_checker::{
    check_program_with_authoritative_modules_at,
    check_program_with_authoritative_modules_at_harness_cached, AuthoritativeModuleFailure,
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
    AuthoritativeModuleResolution, AuthoritativeModuleResolutionDiagnostic,
    AuthoritativeNotFoundModule, AuthoritativePackageId, AuthoritativeResolutionMode,
    AuthoritativeResolvedModule, AuthoritativeSourceMetadata, AuthoritativeSourceToken,
    AuthoritativeUntypedModule, InputFile, UnsupportedAuthoritativeResolution,
};
use tsc_diagnostics::{sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList};
use tsc_program::{
    plan_source_requests, MissingResolutionError, ModuleExtension, PreparedProgram,
    PreparedSourceFile, ResolutionKey, ResolutionMode, ResolutionOutcome, ResolvedModuleTarget,
    SourceFileId, SourceRequestPlan, UnloadedModuleReason,
};

mod cli;

pub use cli::{run_cli, CliOutput};

/// A one-shot owner for one prepared no-emit program.
///
/// The consuming [`run`](Self::run) method keeps every parser, binder, and
/// checker borrow inside the call. No retained checker or self-referential
/// session escapes this boundary.
#[derive(Debug)]
pub struct ProgramSession {
    prepared: PreparedProgram,
}

struct PreparedModuleProvider<'a> {
    prepared: &'a PreparedProgram,
    request_plans: RefCell<BTreeMap<SourceFileId, SourceRequestPlan>>,
}

impl PreparedModuleProvider<'_> {
    fn module_request_loads_source(
        &self,
        source_file: SourceFileId,
        source: &PreparedSourceFile,
        key: &ResolutionKey,
    ) -> Result<bool, AuthoritativeModuleLookupFailure> {
        if let Some(plan) = self.request_plans.borrow().get(&source_file) {
            return plan.module_request_loads_source(key).ok_or(
                AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                ),
            );
        }

        // Only sources that actually reach an unloaded row pay for this
        // second plan. Cache the exact aggregate request loadability so a
        // reason cannot turn a normal import into a resolution-only lookup.
        let plan =
            plan_source_requests(source, self.prepared.compiler_options()).map_err(|_| {
                AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                )
            })?;
        let loads_source = plan.module_request_loads_source(key).ok_or(
            AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
            ),
        )?;
        self.request_plans.borrow_mut().insert(source_file, plan);
        Ok(loads_source)
    }
}

impl AuthoritativeModuleProvider for PreparedModuleProvider<'_> {
    fn resolve_module(
        &self,
        request: AuthoritativeModuleRequest<'_>,
    ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure> {
        let source_file = SourceFileId::from_raw(request.source_token.0);
        let Some(source) = self.prepared.source_file(source_file) else {
            return Err(AuthoritativeModuleLookupFailure::InvalidSourceToken);
        };
        let key = ResolutionKey::new(
            source.path().canonical().clone(),
            request.specifier,
            program_resolution_mode(request.mode),
        );
        let resolution = self
            .prepared
            .resolutions()
            .require_module(&key)
            .map_err(|_| AuthoritativeModuleLookupFailure::Missing)?;
        if !resolution.diagnostics().is_empty() {
            return Err(AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::ResolutionDiagnostics,
            ));
        }
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            let alternate_result = resolution
                .alternate_result()
                .map(|path| {
                    path.display().to_str().map(str::to_owned).ok_or(
                        AuthoritativeModuleLookupFailure::Unsupported(
                            UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                        ),
                    )
                })
                .transpose()?;
            return Ok(AuthoritativeModuleResolution::NotFound(
                AuthoritativeNotFoundModule { alternate_result },
            ));
        };
        if let ResolvedModuleTarget::Unloaded {
            resolved_file,
            reason,
        } = module.target()
        {
            let arbitrary_declaration = matches!(
                module.extension(),
                ModuleExtension::Arbitrary(extension)
                    if extension.starts_with(".d.") && extension.ends_with(".ts")
            );
            if !module.extension().is_javascript()
                && !arbitrary_declaration
                && !matches!(reason, UnloadedModuleReason::NoResolve)
            {
                return Err(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetExtension,
                ));
            }
            if matches!(module.extension(), ModuleExtension::Jsx)
                && self.prepared.compiler_options().jsx.unwrap_or(0) == 0
                && !matches!(reason, UnloadedModuleReason::JsxWithoutJsxOption)
            {
                return Err(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedJsxWithoutJsxOption,
                ));
            }
            let loads_source = self.module_request_loads_source(source_file, source, &key)?;
            if arbitrary_declaration
                && matches!(reason, UnloadedModuleReason::ResolutionOnly)
                && !loads_source
                && (is_declaration_file_name(source.path().display())
                    || self.prepared.compiler_options().allow_arbitrary_extensions == Some(true))
            {
                // A declaration-file module declaration may introduce an
                // otherwise unowned augmentation target, while an ordinary
                // source with allowArbitraryExtensions reaches TS2664 rather
                // than the TS6263 resolution-diagnostic face. Resolution
                // still records the arbitrary twin, but resolveExternalModule
                // must receive the missed face used by both branches.
                let alternate_result = resolution
                    .alternate_result()
                    .map(|path| {
                        path.display().to_str().map(str::to_owned).ok_or(
                            AuthoritativeModuleLookupFailure::Unsupported(
                                UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                            ),
                        )
                    })
                    .transpose()?;
                return Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule { alternate_result },
                ));
            }
            let node_modules_depth_applies = module.is_external_library_import()
                && (module.original_path().is_none()
                    || path_contains_node_modules(resolved_file.canonical().as_path()));
            // At the first external layer, TypeScript tests `1 > maximum`
            // before allowJs. Negating that exact comparison, rather than
            // testing maximum's sign, also preserves NaN and fractional
            // precedence for authoritative unloaded rows.
            let first_node_modules_javascript_layer_is_admitted = !self
                .prepared
                .compiler_options()
                .node_modules_depth_exceeds_limit(1);
            let resolution_diagnostic = match reason {
                UnloadedModuleReason::NoResolve
                    if self.prepared.compiler_options().no_resolve == Some(true) =>
                {
                    None
                }
                UnloadedModuleReason::JsxWithoutJsxOption
                    if matches!(module.extension(), ModuleExtension::Jsx)
                        && self.prepared.compiler_options().jsx.unwrap_or(0) == 0 =>
                {
                    Some(AuthoritativeModuleResolutionDiagnostic::JsxWithoutJsxOption)
                }
                UnloadedModuleReason::ArbitraryExtensionWithoutOption
                    if arbitrary_declaration
                        && loads_source
                        && self.prepared.compiler_options().allow_arbitrary_extensions
                            != Some(true)
                        && !is_declaration_file_name(source.path().display()) =>
                {
                    Some(AuthoritativeModuleResolutionDiagnostic::ArbitraryExtensionWithoutOption)
                }
                UnloadedModuleReason::ResolutionOnly if !loads_source => (arbitrary_declaration
                    && self.prepared.compiler_options().allow_arbitrary_extensions != Some(true)
                    && !is_declaration_file_name(source.path().display()))
                .then_some(
                    AuthoritativeModuleResolutionDiagnostic::ArbitraryExtensionWithoutOption,
                ),
                UnloadedModuleReason::NodeModulesDepth
                    if module.extension().is_javascript()
                        && loads_source
                        && node_modules_depth_applies =>
                {
                    None
                }
                UnloadedModuleReason::JavaScriptNotAdmitted
                    if module.extension().is_javascript()
                        && loads_source
                        && !self.prepared.compiler_options().allow_js
                        && (!node_modules_depth_applies
                            || first_node_modules_javascript_layer_is_admitted) =>
                {
                    None
                }
                _ => {
                    return Err(AuthoritativeModuleLookupFailure::Unsupported(
                        UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                    ));
                }
            };
            let resolved_file_name = resolved_file
                .display()
                .to_str()
                .ok_or(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                ))?
                .to_owned();
            let alternate_result = resolution
                .alternate_result()
                .map(|path| {
                    path.display().to_str().map(str::to_owned).ok_or(
                        AuthoritativeModuleLookupFailure::Unsupported(
                            UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                        ),
                    )
                })
                .transpose()?;
            return Ok(AuthoritativeModuleResolution::Untyped(
                AuthoritativeUntypedModule {
                    resolved_file_name,
                    package_name: module
                        .package_id()
                        .map(|package_id| package_id.name().to_owned()),
                    alternate_result,
                    types_package_exists: resolution.types_package_exists(),
                    package_bundles_types: resolution.package_bundles_types(),
                    resolution_diagnostic,
                },
            ));
        }
        // PreparedProgramBuilder already validated the target/originalPath
        // transition against this SourceFileId. The checker consumes the
        // selected source through its stable token; originalPath is resolver
        // provenance and does not replace that source identity.
        let ResolvedModuleTarget::Source {
            source,
            resolved_file,
        } = module.target()
        else {
            unreachable!("unloaded target returned above")
        };
        if self.prepared.source_file(*source).is_none() {
            return Err(AuthoritativeModuleLookupFailure::InvalidSourceToken);
        }
        Ok(AuthoritativeModuleResolution::Resolved(
            AuthoritativeResolvedModule {
                target_token: AuthoritativeSourceToken(source.raw()),
                resolved_file_name: resolved_file
                    .display()
                    .to_str()
                    .ok_or(AuthoritativeModuleLookupFailure::Unsupported(
                        UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                    ))?
                    .to_owned(),
                resolved_using_ts_extension: module.resolved_using_ts_extension(),
                is_tsx: matches!(
                    module.extension(),
                    ModuleExtension::Tsx | ModuleExtension::Jsx
                ),
                is_arbitrary_extension: matches!(module.extension(), ModuleExtension::Arbitrary(_)),
                is_external_library_import: module.is_external_library_import(),
                package_id: module
                    .package_id()
                    .map(|package_id| AuthoritativePackageId {
                        name: package_id.name().to_owned(),
                        submodule_name: package_id.submodule_name().to_owned(),
                        version: package_id.version().to_owned(),
                        peer_dependencies: package_id.peer_dependencies().map(str::to_owned),
                    }),
                alternate_result: resolution
                    .alternate_result()
                    .map(|path| {
                        path.display().to_str().map(str::to_owned).ok_or(
                            AuthoritativeModuleLookupFailure::Unsupported(
                                UnsupportedAuthoritativeResolution::ResolvedFileIdentity,
                            ),
                        )
                    })
                    .transpose()?,
                types_package_exists: resolution.types_package_exists(),
                package_bundles_types: resolution.package_bundles_types(),
            },
        ))
    }
}

fn path_contains_node_modules(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|path| path.split('/').any(|component| component == "node_modules"))
}

fn is_declaration_file_name(path: &Path) -> bool {
    path.to_str().is_some_and(|file_name| {
        if file_name.ends_with(".d.ts")
            || file_name.ends_with(".d.cts")
            || file_name.ends_with(".d.mts")
        {
            return true;
        }
        let base_name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
        base_name.ends_with(".ts") && base_name.contains(".d.")
    })
}

const fn program_resolution_mode(mode: AuthoritativeResolutionMode) -> ResolutionMode {
    match mode {
        AuthoritativeResolutionMode::CommonJs => ResolutionMode::CommonJs,
        AuthoritativeResolutionMode::EsNext => ResolutionMode::EsNext,
        AuthoritativeResolutionMode::Unspecified => ResolutionMode::Unspecified,
    }
}

fn map_authoritative_failure(
    prepared: &PreparedProgram,
    failure: AuthoritativeModuleFailure,
) -> DriverError {
    if let AuthoritativeModuleFailure::Lookup {
        source_token,
        specifier,
        mode,
        failure: AuthoritativeModuleLookupFailure::Missing,
        ..
    } = &failure
    {
        if let Some(source) = prepared.source_file(SourceFileId::from_raw(source_token.0)) {
            let key = ResolutionKey::new(
                source.path().canonical().clone(),
                specifier.clone(),
                program_resolution_mode(*mode),
            );
            if let Err(missing) = prepared.resolutions().require_module(&key) {
                return DriverError::MissingResolution(missing);
            }
        }
    }
    DriverError::AuthoritativeResolution(failure)
}

impl ProgramSession {
    pub fn new(prepared: PreparedProgram) -> Self {
        Self { prepared }
    }

    /// Consume the prepared program and execute the no-emit diagnostic pass.
    ///
    /// Module lookups use only [`PreparedProgram::resolutions`]. A missing
    /// exact `(source, specifier, mode)` row is an infrastructure error; the
    /// checker never falls back to its legacy heuristic resolver.
    pub fn run(self) -> Result<NoEmitOutcome, DriverError> {
        self.run_inner(false)
    }

    /// Upstream-harness execution with exact-match vendored-lib reuse.
    ///
    /// This is deliberately not the production H0 entry: [`run`](Self::run)
    /// keeps every parsed and bound source owned by its one-shot session.
    /// Only pinned, immutable upstream harnesses may opt into the checker's
    /// process-lifetime lib bundle to avoid rebuilding an identical standard
    /// library prefix for every fixture case. The cache validates the ordered
    /// library names, full source text, and parser/binder option projection
    /// before reuse, so compiler/project suite audits can share the same safe
    /// path as the conformance runner.
    #[doc(hidden)]
    pub fn run_for_harness_with_lib_cache(self) -> Result<NoEmitOutcome, DriverError> {
        self.run_inner(true)
    }

    fn run_inner(self, harness_lib_cache: bool) -> Result<NoEmitOutcome, DriverError> {
        let inputs = project_checker_inputs(&self.prepared)?;
        let has_roots = !self.prepared.roots().is_empty();
        let provider = PreparedModuleProvider {
            prepared: &self.prepared,
            request_plans: RefCell::new(BTreeMap::new()),
        };
        let checked = if harness_lib_cache {
            check_program_with_authoritative_modules_at_harness_cached(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
            )
        } else {
            check_program_with_authoritative_modules_at(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
            )
        }
        .map_err(|failure| map_authoritative_failure(&self.prepared, failure))?;

        let preparation = self.prepared.diagnostics();
        let mut conformance_diagnostics = checked.diagnostics;
        let config_diagnostics = preparation.config().to_vec();
        let mut syntactic_diagnostics = checked.syntactic_diagnostics;
        sort_and_dedupe_diagnostics(&mut syntactic_diagnostics);
        let partial_checks = checked.partial_checks;

        // Program-construction diagnostics are part of tsc's
        // combined diagnostic map. File-less rows and rows owned by config
        // or other auxiliary files feed getOptionsDiagnostics; rows owned by
        // a program SourceFile feed that source's getSemanticDiagnostics.
        // Each public getter applies sortAndDeduplicateDiagnostics to its
        // combined result.
        let mut available_options = preparation.options().to_vec();
        let mut available_semantic = checked.semantic_diagnostics;
        let program_diagnostics = self
            .prepared
            .resolutions()
            .type_references()
            .flat_map(|(_, resolution)| resolution.diagnostics())
            .cloned()
            .collect::<Vec<_>>();
        // The conformance evidence stream is the aggregate of public
        // per-source getters. Source-owned program rows therefore join it,
        // while file-less/config-owned rows remain options diagnostics only.
        conformance_diagnostics.extend(
            preparation
                .program()
                .iter()
                .chain(program_diagnostics.iter())
                .filter(|diagnostic| {
                    diagnostic.file_name.as_deref().is_some_and(|file_name| {
                        prepared_source_owns_diagnostic(&self.prepared, file_name)
                    })
                })
                .cloned(),
        );
        sort_and_dedupe_diagnostics(&mut conformance_diagnostics);

        let mut route_program_diagnostic =
            |diagnostic: &Diagnostic| {
                if diagnostic.file_name.as_deref().is_some_and(|file_name| {
                    prepared_source_owns_diagnostic(&self.prepared, file_name)
                }) {
                    available_semantic.push(diagnostic.clone());
                } else {
                    available_options.push(diagnostic.clone());
                }
            };
        for diagnostic in preparation.program() {
            route_program_diagnostic(diagnostic);
        }
        for diagnostic in &program_diagnostics {
            route_program_diagnostic(diagnostic);
        }
        sort_and_dedupe_diagnostics(&mut available_options);
        sort_and_dedupe_diagnostics(&mut available_semantic);

        // emitFilesAndReportErrors compares the aggregate length with the
        // original config-diagnostic length. Config errors therefore remain
        // visible but do not themselves close any of the later gates.
        let (options_diagnostics, global_diagnostics, semantic_diagnostics) =
            if syntactic_diagnostics.is_empty() {
                let options_diagnostics = available_options;
                let global_diagnostics = if has_roots {
                    checked.global_diagnostics
                } else {
                    Vec::new()
                };
                let semantic_diagnostics =
                    if options_diagnostics.is_empty() && global_diagnostics.is_empty() {
                        if let Some(partial) = partial_checks.first() {
                            return Err(DriverError::IncompleteCheck {
                                file_name: partial.file_name.clone(),
                                start: partial.start,
                                length: partial.length,
                                reason: partial.reason.clone(),
                                additional_partial_checks: partial_checks.len().saturating_sub(1),
                            });
                        }
                        available_semantic
                    } else {
                        Vec::new()
                    };
                (
                    options_diagnostics,
                    global_diagnostics,
                    semantic_diagnostics,
                )
            } else {
                (Vec::new(), Vec::new(), Vec::new())
            };

        // `checked.suggestion_diagnostics` is deliberately dropped here.
        // Suggestions remain a legacy per-file getter surface and are not
        // part of `tsc --noEmit` command output.
        Ok(NoEmitOutcome {
            config_diagnostics,
            syntactic_diagnostics,
            options_diagnostics,
            global_diagnostics,
            semantic_diagnostics,
            conformance_diagnostics,
        })
    }
}

/// The five diagnostic collections exposed by the no-emit driver.
///
/// Buckets retain their getter-local ordering. [`diagnostics`](Self::diagnostics)
/// and [`into_diagnostics`](Self::into_diagnostics) expose the command driver
/// order without re-sorting across bucket boundaries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct NoEmitOutcome {
    config_diagnostics: DiagnosticList,
    syntactic_diagnostics: DiagnosticList,
    options_diagnostics: DiagnosticList,
    global_diagnostics: DiagnosticList,
    semantic_diagnostics: DiagnosticList,
    // The legacy differential harness compares the aggregate of public
    // per-file getters, including suggestions. This stream is retained only
    // as evidence; diagnostics()/into_diagnostics intentionally exclude it.
    conformance_diagnostics: DiagnosticList,
}

impl NoEmitOutcome {
    pub fn config_diagnostics(&self) -> &[Diagnostic] {
        &self.config_diagnostics
    }

    pub fn syntactic_diagnostics(&self) -> &[Diagnostic] {
        &self.syntactic_diagnostics
    }

    pub fn options_diagnostics(&self) -> &[Diagnostic] {
        &self.options_diagnostics
    }

    pub fn global_diagnostics(&self) -> &[Diagnostic] {
        &self.global_diagnostics
    }

    pub fn semantic_diagnostics(&self) -> &[Diagnostic] {
        &self.semantic_diagnostics
    }

    /// Aggregate public-getter stream used only by differential conformance.
    /// It includes suggestions and is therefore not CLI output.
    pub fn conformance_diagnostics(&self) -> &[Diagnostic] {
        &self.conformance_diagnostics
    }

    /// Iterate in the no-emit command's bucket order.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.config_diagnostics
            .iter()
            .chain(&self.syntactic_diagnostics)
            .chain(&self.options_diagnostics)
            .chain(&self.global_diagnostics)
            .chain(&self.semantic_diagnostics)
    }

    /// Consume the outcome and flatten it in no-emit command bucket order.
    pub fn into_diagnostics(self) -> DiagnosticList {
        let capacity = self.config_diagnostics.len()
            + self.syntactic_diagnostics.len()
            + self.options_diagnostics.len()
            + self.global_diagnostics.len()
            + self.semantic_diagnostics.len();
        let mut diagnostics = Vec::with_capacity(capacity);
        diagnostics.extend(self.config_diagnostics);
        diagnostics.extend(self.syntactic_diagnostics);
        diagnostics.extend(self.options_diagnostics);
        diagnostics.extend(self.global_diagnostics);
        diagnostics.extend(self.semantic_diagnostics);
        diagnostics
    }
}

/// A fail-closed failure while projecting trusted prepared data into the
/// checker execution boundary.
///
/// [`PreparedProgram`] construction already rejects the projection variants;
/// `IncompleteCheck` rejects checker containment after execution, and
/// `MissingResolution` reserves the fail-closed H0.2 table connection. The
/// typed boundary prevents any of them from becoming a partial success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriverError {
    InvalidLibraryPrefix {
        position: usize,
        source_file: SourceFileId,
    },
    MissingPreparedSource {
        source_file: SourceFileId,
    },
    MissingPreparedSourceIdentity {
        path: PathBuf,
    },
    NonUnicodeDisplayPath {
        source_file: Option<SourceFileId>,
        path: PathBuf,
    },
    IncompleteCheck {
        file_name: String,
        start: u32,
        length: u32,
        reason: String,
        additional_partial_checks: usize,
    },
    MissingResolution(MissingResolutionError),
    AuthoritativeResolution(AuthoritativeModuleFailure),
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLibraryPrefix {
                position,
                source_file,
            } => write!(
                formatter,
                "project prepared program for no-emit execution: library prefix position {position} names SourceFileId {}, expected SourceFileId {position}",
                source_file.raw()
            ),
            Self::MissingPreparedSource { source_file } => write!(
                formatter,
                "project prepared program for no-emit execution: library prefix names missing SourceFileId {}",
                source_file.raw()
            ),
            Self::MissingPreparedSourceIdentity { path } => write!(
                formatter,
                "project prepared program for no-emit execution: source {} has no stable SourceFileId",
                path.display()
            ),
            Self::NonUnicodeDisplayPath { path, .. } => write!(
                formatter,
                "project prepared program for no-emit execution for {}: prepared display path is not valid Unicode",
                path.display()
            ),
            Self::IncompleteCheck {
                file_name,
                start,
                length,
                reason,
                additional_partial_checks,
            } => write!(
                formatter,
                "no-emit check was incomplete at {file_name}:{start}+{length}: {reason} ({additional_partial_checks} additional partial checks)",
            ),
            Self::MissingResolution(error) => error.fmt(formatter),
            Self::AuthoritativeResolution(error) => error.fmt(formatter),
        }
    }
}

impl Error for DriverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MissingResolution(error) => Some(error),
            Self::AuthoritativeResolution(error) => Some(error),
            _ => None,
        }
    }
}

impl From<MissingResolutionError> for DriverError {
    fn from(error: MissingResolutionError) -> Self {
        Self::MissingResolution(error)
    }
}

struct ProjectedCheckerInputs {
    libs: Vec<InputFile>,
    files: Vec<InputFile>,
    lib_metadata: Vec<AuthoritativeSourceMetadata>,
    file_metadata: Vec<AuthoritativeSourceMetadata>,
    current_directory: String,
}

fn project_checker_inputs(
    prepared: &PreparedProgram,
) -> Result<ProjectedCheckerInputs, DriverError> {
    let sources = prepared.source_files();
    let library_ids = prepared.library_files();
    let mut libs = Vec::with_capacity(library_ids.len());
    let mut lib_metadata = Vec::with_capacity(library_ids.len());

    for (position, source_file) in library_ids.iter().copied().enumerate() {
        if source_file.index() != position {
            return Err(DriverError::InvalidLibraryPrefix {
                position,
                source_file,
            });
        }
        let source = prepared
            .source_file(source_file)
            .ok_or(DriverError::MissingPreparedSource { source_file })?;
        let (input, metadata) = project_source(source, source_file)?;
        libs.push(input);
        lib_metadata.push(metadata);
    }

    let mut files = Vec::with_capacity(sources.len().saturating_sub(library_ids.len()));
    let mut file_metadata = Vec::with_capacity(files.capacity());
    for source in sources.iter().skip(library_ids.len()) {
        let source_file = prepared
            .source_id(source.path().canonical())
            .ok_or_else(|| DriverError::MissingPreparedSourceIdentity {
                path: source.path().display().to_path_buf(),
            })?;
        let (input, metadata) = project_source(source, source_file)?;
        files.push(input);
        file_metadata.push(metadata);
    }

    let current_directory_path = prepared.current_directory().display();
    let current_directory = current_directory_path
        .to_str()
        .ok_or_else(|| DriverError::NonUnicodeDisplayPath {
            source_file: None,
            path: current_directory_path.to_path_buf(),
        })?
        .to_owned();
    Ok(ProjectedCheckerInputs {
        libs,
        files,
        lib_metadata,
        file_metadata,
        current_directory,
    })
}

fn project_source(
    source: &PreparedSourceFile,
    source_file: SourceFileId,
) -> Result<(InputFile, AuthoritativeSourceMetadata), DriverError> {
    let display_path = source.path().display();
    let name = display_path
        .to_str()
        .ok_or_else(|| DriverError::NonUnicodeDisplayPath {
            source_file: Some(source_file),
            path: display_path.to_path_buf(),
        })?
        .to_owned();
    let metadata = AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(source_file.raw()),
        file_name: name.clone(),
        may_be_emitted: source.may_be_emitted(),
        implied_node_format: source.implied_node_format().map(checker_resolution_mode),
        implied_node_format_for_emit: source
            .implied_node_format_for_emit()
            .map(checker_resolution_mode),
    };
    Ok((
        InputFile {
            name,
            text: source.text().to_owned(),
        },
        metadata,
    ))
}

const fn checker_resolution_mode(mode: ResolutionMode) -> AuthoritativeResolutionMode {
    match mode {
        ResolutionMode::CommonJs => AuthoritativeResolutionMode::CommonJs,
        ResolutionMode::EsNext => AuthoritativeResolutionMode::EsNext,
        ResolutionMode::Unspecified => AuthoritativeResolutionMode::Unspecified,
    }
}

fn prepared_source_owns_diagnostic(prepared: &PreparedProgram, file_name: &str) -> bool {
    let names_equal = |candidate: &std::path::Path| {
        candidate.to_str().is_some_and(|candidate| {
            candidate == file_name || candidate.replace('\\', "/") == file_name.replace('\\', "/")
        })
    };
    prepared.source_files().iter().any(|source| {
        names_equal(source.path().display())
            || names_equal(source.path().canonical().as_path())
            || source
                .alternate_display_paths()
                .iter()
                .any(|path| names_equal(path))
            || source.real_path().is_some_and(|path| {
                names_equal(path.display()) || names_equal(path.canonical().as_path())
            })
    })
}
