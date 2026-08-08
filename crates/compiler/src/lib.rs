#![forbid(unsafe_code)]

//! One-shot execution of an owned H0 prepared program.
//!
//! This crate is the dependency boundary between the owned program contract
//! and the parser/binder/checker implementation. A [`ProgramSession`] owns
//! exactly one [`PreparedProgram`], projects its already-final source order
//! into the checker, and is consumed by either the no-emit
//! [`ProgramSession::run`] entry or the distinct emitting
//! [`ProgramSession::emit`] entry.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_checker::{
    check_program_with_authoritative_modules_at,
    check_program_with_authoritative_modules_at_for_emit,
    check_program_with_authoritative_modules_at_harness_cached, AuthoritativeModuleFailure,
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
    AuthoritativeModuleResolution, AuthoritativeModuleResolutionDiagnostic,
    AuthoritativeNotFoundModule, AuthoritativePackageId, AuthoritativeResolutionMode,
    AuthoritativeResolvedModule, AuthoritativeSourceMetadata, AuthoritativeSourceToken,
    AuthoritativeUntypedModule, CheckResult, InputFile, ProgramSnapshot,
    UnsupportedAuthoritativeResolution,
};
use tsc_diagnostics::{sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList};
use tsc_emitter::{
    emit_files_with_activity, preflight_emit, validate_bootstrap_emit_request, EmitDiagnosticGate,
    EmitHost, EmitSource, H2ActivityCanary, UnavailableEmitResolver,
};
pub use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitContractViolation, EmitFailure,
    EmitFileSystem, EmitIoError, EmitIoOperation, EmitMode, EmitOutcome, EmitOutputPaths,
    EmitOutputPlan, EmitOutputUnit, EmitRoot, EmitSelection, EmitStage, EmitTextMetadata,
    EmitWriteDisposition, EmitWriteMetadata, FsOutputSink, GeneratedUtf16Position,
    H2ActivityCounters, H2RuntimeSlice, MemoryOutputSink, OutputSink, SourceMapObservation,
    UnsupportedEmitFeature,
};
pub use tsc_program::PreparedProgramMode;
use tsc_program::{
    plan_source_requests, CompilerOptions, MissingResolutionError, ModuleExtension,
    PreparedProgram, PreparedSourceFile, ResolutionKey, ResolutionMode, ResolutionOutcome,
    ResolvedModuleTarget, SourceFileId, SourceRequestPlan, UnloadedModuleReason,
};

mod cli;
mod no_emit_canary;

pub use cli::{run_cli, CliOutput};
pub use no_emit_canary::NoEmitActivityCounters;

/// A one-shot owner for one mode-validated prepared program.
///
/// The consuming [`run`](Self::run) method keeps every parser, binder, and
/// checker borrow inside the call. No retained checker or self-referential
/// session escapes this boundary.
#[derive(Debug)]
pub struct ProgramSession {
    prepared: PreparedProgram,
}

pub(crate) struct CliEmitSessionOutcome {
    pub(crate) emit: EmitOutcome,
    pub(crate) config_diagnostics: DiagnosticList,
    pub(crate) syntactic_diagnostics: DiagnosticList,
    pub(crate) options_diagnostics: DiagnosticList,
    pub(crate) global_diagnostics: DiagnosticList,
    pub(crate) semantic_diagnostics: DiagnosticList,
    pub(crate) work_counters: NoEmitWorkCounters,
}

struct EmitSessionDiagnostics {
    config: DiagnosticList,
    syntactic: DiagnosticList,
    options: DiagnosticList,
    global: DiagnosticList,
    semantic: DiagnosticList,
}

impl EmitSessionDiagnostics {
    fn gate(&self) -> EmitDiagnosticGate {
        EmitDiagnosticGate::new(
            self.options.clone(),
            self.syntactic.clone(),
            self.global.clone(),
            self.semantic.clone(),
        )
    }

    fn with_emit(
        mut self,
        preflight_diagnostics: &[Diagnostic],
        emit: EmitOutcome,
        work_counters: NoEmitWorkCounters,
    ) -> CliEmitSessionOutcome {
        self.options.extend_from_slice(preflight_diagnostics);
        sort_and_dedupe_diagnostics(&mut self.options);
        CliEmitSessionOutcome {
            emit,
            config_diagnostics: self.config,
            syntactic_diagnostics: self.syntactic,
            options_diagnostics: self.options,
            global_diagnostics: self.global,
            semantic_diagnostics: self.semantic,
            work_counters,
        }
    }
}

struct PreparedModuleProvider<'a> {
    prepared: &'a PreparedProgram,
    request_plans: RefCell<BTreeMap<SourceFileId, SourceRequestPlan>>,
}

struct PreparedEmitHost<'program> {
    prepared: &'program PreparedProgram,
    source_files: Vec<SourceFileId>,
    common_source_directory: PathBuf,
}

impl<'program> PreparedEmitHost<'program> {
    fn new(prepared: &'program PreparedProgram) -> Result<Self, DriverError> {
        let source_files = prepared
            .source_files()
            .iter()
            .map(|source| {
                prepared
                    .source_id(source.path().canonical())
                    .ok_or_else(|| DriverError::MissingPreparedSourceIdentity {
                        path: source.path().display().to_path_buf(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let common_source_directory = common_emit_source_directory(prepared, &source_files);
        Ok(Self {
            prepared,
            source_files,
            common_source_directory,
        })
    }
}

impl EmitHost for PreparedEmitHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.prepared.compiler_options()
    }

    fn current_directory(&self) -> &Path {
        self.prepared.current_directory().display()
    }

    fn common_source_directory(&self) -> &Path {
        &self.common_source_directory
    }

    fn config_file_path(&self) -> Option<&Path> {
        self.prepared
            .program_options()
            .config_file_path()
            .map(|path| path.display())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.prepared.path_context().use_case_sensitive_file_names()
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        &self.source_files
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let source = self.prepared.source_file(id)?;
        Some(EmitSource::new(
            id,
            source.path().display(),
            source.path().canonical().as_path(),
            source.may_be_emitted(),
            None,
        ))
    }
}

struct CheckedEmitHost<'host, 'snapshot> {
    prepared: &'host PreparedEmitHost<'host>,
    snapshot: &'snapshot ProgramSnapshot,
}

impl EmitHost for CheckedEmitHost<'_, '_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.prepared.compiler_options()
    }

    fn current_directory(&self) -> &Path {
        self.prepared.current_directory()
    }

    fn common_source_directory(&self) -> &Path {
        self.prepared.common_source_directory()
    }

    fn config_file_path(&self) -> Option<&Path> {
        self.prepared.config_file_path()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.prepared.use_case_sensitive_file_names()
    }

    fn source_file_ids(&self) -> &[SourceFileId] {
        self.prepared.source_file_ids()
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let source = self.prepared.prepared.source_file(id)?;
        let expected_name = source.path().display().to_string_lossy();
        let syntax = self
            .snapshot
            .documents()
            .get(id.index())
            .filter(|document| document.source().file_name == expected_name)
            .or_else(|| {
                self.snapshot
                    .documents()
                    .iter()
                    .find(|document| document.source().file_name == expected_name)
            })
            .map(|document| document.source());
        Some(EmitSource::new(
            id,
            source.path().display(),
            source.path().canonical().as_path(),
            source.may_be_emitted(),
            syntax,
        ))
    }
}

fn common_emit_source_directory(
    prepared: &PreparedProgram,
    source_files: &[SourceFileId],
) -> PathBuf {
    if let Some(root_dir) = prepared.compiler_options().root_dir.as_deref() {
        let root = Path::new(root_dir);
        return if root.is_absolute() {
            root.to_path_buf()
        } else {
            prepared.current_directory().display().join(root)
        };
    }

    let mut directories = source_files.iter().filter_map(|id| {
        let source = prepared.source_file(*id)?;
        (source.may_be_emitted() && !is_declaration_file_name(source.path().display()))
            .then(|| source.path().display().parent().map(Path::to_path_buf))
            .flatten()
    });
    let Some(mut common) = directories.next() else {
        return prepared.current_directory().display().to_path_buf();
    };
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    for directory in directories {
        while !path_starts_with(&directory, &common, case_sensitive) {
            if !common.pop() {
                return prepared.current_directory().display().to_path_buf();
            }
        }
    }
    common
}

fn path_starts_with(path: &Path, prefix: &Path, case_sensitive: bool) -> bool {
    if case_sensitive {
        path.starts_with(prefix)
    } else {
        path.to_string_lossy()
            .to_lowercase()
            .starts_with(&prefix.to_string_lossy().to_lowercase())
    }
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
        self.require_mode(PreparedProgramMode::NoEmit)?;
        let mut no_emit_canary = no_emit_canary::NoEmitCanary::new();
        self.run_with_no_emit_canary(false, &mut no_emit_canary)
    }

    /// Consume the prepared program through the separately typed emit path.
    pub fn emit(self, sink: &mut dyn OutputSink) -> Result<EmitOutcome, DriverError> {
        self.emit_with_command_outcome(sink)
            .map(|outcome| outcome.emit)
    }

    pub(crate) fn emit_for_cli(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<CliEmitSessionOutcome, DriverError> {
        self.emit_with_command_outcome(sink)
    }

    fn emit_with_command_outcome(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<CliEmitSessionOutcome, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let prepared = self.prepared;
        let mut h2_activity = H2ActivityCanary::h1_profile();
        h2_activity.construct_emit_session();
        let emit_host = PreparedEmitHost::new(&prepared)?;
        validate_bootstrap_emit_request(&emit_host).map_err(DriverError::Emit)?;
        let selection = EmitSelection::WholeProgram;
        h2_activity.construct_output_plan();
        let preflight = preflight_emit(&emit_host, selection).map_err(DriverError::Emit)?;

        let inputs = project_checker_inputs(&prepared)?;
        let provider = PreparedModuleProvider {
            prepared: &prepared,
            request_plans: RefCell::new(BTreeMap::new()),
        };
        let mut pending_preflight = Some(preflight);
        let mut emit_result: Option<Result<CliEmitSessionOutcome, DriverError>> = None;
        let checked = check_program_with_authoritative_modules_at_for_emit(
            &inputs.libs,
            &inputs.files,
            &inputs.lib_metadata,
            &inputs.file_metadata,
            prepared.compiler_options(),
            &inputs.current_directory,
            &provider,
            |snapshot, checker, checked| {
                if emit_result.is_some() {
                    return;
                }
                if let Some(partial) = checked.partial_checks.first() {
                    emit_result = Some(Err(DriverError::IncompleteCheck {
                        file_name: partial.file_name.clone(),
                        start: partial.start,
                        length: partial.length,
                        reason: partial.reason.clone(),
                        additional_partial_checks: checked.partial_checks.len().saturating_sub(1),
                    }));
                    return;
                }
                let diagnostics = emit_session_diagnostics(&prepared, checked);
                let diagnostic_gate = diagnostics.gate();
                let work_counters = check_work_counters(checked);
                let checked_host = CheckedEmitHost {
                    prepared: &emit_host,
                    snapshot,
                };
                let preflight = pending_preflight
                    .take()
                    .expect("checked emit callback runs once");
                let preflight_diagnostics = preflight.diagnostics().to_vec();
                h2_activity.borrow_emit_resolver();
                emit_result = Some(checker.with_emit_resolver(|resolver| {
                    emit_files_with_activity(
                        resolver,
                        &checked_host,
                        preflight,
                        selection,
                        &diagnostic_gate,
                        sink,
                        &mut h2_activity,
                    )
                    .map(|emit| diagnostics.with_emit(&preflight_diagnostics, emit, work_counters))
                    .map_err(DriverError::Emit)
                }));
            },
        )
        .map_err(|failure| map_authoritative_failure(&prepared, failure))?;

        if let Some(result) = emit_result {
            return result;
        }

        // An empty Program has no snapshot from which to construct a checker
        // resolver. Its empty output plan cannot query one, so retain the same
        // diagnostics gate and execute with the fail-closed unavailable
        // projection.
        let diagnostics = emit_session_diagnostics(&prepared, &checked);
        let diagnostic_gate = diagnostics.gate();
        let work_counters = check_work_counters(&checked);
        let preflight = pending_preflight.expect("empty Program did not consume preflight");
        let preflight_diagnostics = preflight.diagnostics().to_vec();
        emit_files_with_activity(
            &UnavailableEmitResolver,
            &emit_host,
            preflight,
            selection,
            &diagnostic_gate,
            sink,
            &mut h2_activity,
        )
        .map(|emit| diagnostics.with_emit(&preflight_diagnostics, emit, work_counters))
        .map_err(DriverError::Emit)
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
        self.require_mode(PreparedProgramMode::NoEmit)?;
        let mut no_emit_canary = no_emit_canary::NoEmitCanary::new();
        self.run_with_no_emit_canary(true, &mut no_emit_canary)
    }

    pub(crate) fn run_with_no_emit_canary(
        self,
        harness_lib_cache: bool,
        no_emit_canary: &mut no_emit_canary::NoEmitCanary,
    ) -> Result<NoEmitOutcome, DriverError> {
        self.run_inner(harness_lib_cache, no_emit_canary)
    }

    fn require_mode(&self, expected: PreparedProgramMode) -> Result<(), DriverError> {
        let actual = self.prepared.mode();
        if actual == expected {
            Ok(())
        } else {
            Err(DriverError::InvalidProgramMode { expected, actual })
        }
    }

    fn run_inner(
        self,
        harness_lib_cache: bool,
        _no_emit_canary: &mut no_emit_canary::NoEmitCanary,
    ) -> Result<NoEmitOutcome, DriverError> {
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
        let checker_work = checked.work_counters;
        let work_counters = NoEmitWorkCounters {
            parsed_documents: checker_work.parsed_documents(),
            bound_documents: checker_work.bound_documents(),
            full_text_copies: checker_work.full_text_copies(),
            full_text_bytes_copied: checker_work.full_text_bytes_copied(),
        };

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
            work_counters,
            no_emit_activity: NoEmitActivityCounters,
        })
    }
}

/// The five diagnostic collections exposed by the no-emit driver.
///
/// Buckets retain their getter-local ordering. [`diagnostics`](Self::diagnostics)
/// and [`into_diagnostics`](Self::into_diagnostics) expose the command driver
/// order without re-sorting across bucket boundaries.
#[derive(Clone, Debug, Default)]
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
    // Operational evidence is not part of diagnostic-result equality. Tests
    // and qualification compare it explicitly through work_counters().
    work_counters: NoEmitWorkCounters,
    // H1.0b proof is zero-sized; successful construction means every guarded
    // emitter factory and output-sink call remained unreachable.
    no_emit_activity: NoEmitActivityCounters,
}

impl PartialEq for NoEmitOutcome {
    fn eq(&self, other: &Self) -> bool {
        self.config_diagnostics == other.config_diagnostics
            && self.syntactic_diagnostics == other.syntactic_diagnostics
            && self.options_diagnostics == other.options_diagnostics
            && self.global_diagnostics == other.global_diagnostics
            && self.semantic_diagnostics == other.semantic_diagnostics
            && self.conformance_diagnostics == other.conformance_diagnostics
    }
}

impl Eq for NoEmitOutcome {}

/// Coarse H0/L0 work observations for one no-emit session.
///
/// The program-session fields cover the current owned projections from
/// prepared source text through checker input into a parsed source. The CLI
/// augments the same counters with its diagnostic-rendering text projection.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NoEmitWorkCounters {
    parsed_documents: u64,
    bound_documents: u64,
    full_text_copies: u64,
    full_text_bytes_copied: u64,
}

impl NoEmitWorkCounters {
    pub const fn parsed_documents(self) -> u64 {
        self.parsed_documents
    }

    pub const fn bound_documents(self) -> u64 {
        self.bound_documents
    }

    pub const fn full_text_copies(self) -> u64 {
        self.full_text_copies
    }

    pub const fn full_text_bytes_copied(self) -> u64 {
        self.full_text_bytes_copied
    }
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

    /// Parse/bind/full-text-copy evidence for this consumed session.
    pub const fn work_counters(&self) -> NoEmitWorkCounters {
        self.work_counters
    }

    /// H1 constructor/output-write observations for this no-emit session.
    pub const fn no_emit_activity(&self) -> NoEmitActivityCounters {
        self.no_emit_activity
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
    InvalidProgramMode {
        expected: PreparedProgramMode,
        actual: PreparedProgramMode,
    },
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
    Emit(EmitFailure),
}

impl fmt::Display for DriverError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgramMode { expected, actual } => write!(
                formatter,
                "program session entry requires a {expected:?} prepared program, received {actual:?}",
            ),
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
            Self::Emit(error) => error.fmt(formatter),
        }
    }
}

impl Error for DriverError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::MissingResolution(error) => Some(error),
            Self::AuthoritativeResolution(error) => Some(error),
            Self::Emit(error) => Some(error),
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
        InputFile::from_snapshot(name, Arc::clone(source.snapshot())),
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

/// Assemble the four `handleNoEmitOptions` getter streams in their vendored
/// order. These diagnostics are returned by `Program.emit` only when
/// `noEmitOnError` closes the emit path; an ordinary emit with type errors
/// still returns emitter diagnostics only.
fn emit_session_diagnostics(
    prepared: &PreparedProgram,
    checked: &CheckResult,
) -> EmitSessionDiagnostics {
    let preparation = prepared.diagnostics();
    let type_reference_diagnostics = prepared
        .resolutions()
        .type_references()
        .flat_map(|(_, resolution)| resolution.diagnostics())
        .cloned()
        .collect::<Vec<_>>();

    let mut options = preparation.options().to_vec();
    let mut semantic = checked.semantic_diagnostics.clone();
    for diagnostic in preparation
        .program()
        .iter()
        .chain(type_reference_diagnostics.iter())
    {
        if diagnostic
            .file_name
            .as_deref()
            .is_some_and(|file_name| prepared_source_owns_diagnostic(prepared, file_name))
        {
            semantic.push(diagnostic.clone());
        } else {
            options.push(diagnostic.clone());
        }
    }
    sort_and_dedupe_diagnostics(&mut options);
    sort_and_dedupe_diagnostics(&mut semantic);
    let mut syntactic = checked.syntactic_diagnostics.clone();
    sort_and_dedupe_diagnostics(&mut syntactic);

    EmitSessionDiagnostics {
        config: preparation.config().to_vec(),
        options,
        syntactic,
        global: checked.global_diagnostics.clone(),
        semantic,
    }
}

fn check_work_counters(checked: &CheckResult) -> NoEmitWorkCounters {
    NoEmitWorkCounters {
        parsed_documents: checked.work_counters.parsed_documents(),
        bound_documents: checked.work_counters.bound_documents(),
        full_text_copies: checked.work_counters.full_text_copies(),
        full_text_bytes_copied: checked.work_counters.full_text_bytes_copied(),
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

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;
