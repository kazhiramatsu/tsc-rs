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

use tsc_checker::emit::CheckerSession;
use tsc_checker::{
    check_program_with_authoritative_modules_at,
    check_program_with_authoritative_modules_at_for_emit,
    check_program_with_authoritative_modules_at_for_emit_with_harness_lib_bundle,
    check_program_with_authoritative_modules_at_harness_cached,
    prepare_authoritative_harness_lib_bundle, AuthoritativeModuleFailure,
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider, AuthoritativeModuleRequest,
    AuthoritativeModuleResolution, AuthoritativeModuleResolutionDiagnostic,
    AuthoritativeNotFoundModule, AuthoritativePackageId, AuthoritativeResolutionDiagnosticModule,
    AuthoritativeResolutionMode, AuthoritativeResolvedModule, AuthoritativeSourceMetadata,
    AuthoritativeSourceToken, AuthoritativeUntypedModule, CheckResult, InputFile,
    LibraryPrefixCompletion, OwnedHarnessLibBundle, ProgramSnapshot,
    UnsupportedAuthoritativeResolution,
};
use tsc_diagnostics::{gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, MessageChain};
use tsc_emitter::{
    emit_files_with_activity, preflight_emit, print_script_units_with_recording_for_harness,
    validate_bootstrap_emit_request, EmitDiagnosticGate, EmitHost, EmitSource, H2ActivityCanary,
    PrintedText, SourceMapRecordingInputs, UnavailableEmitResolver,
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
    plan_source_requests, validate_compiler_options, validate_paths_option_diagnostics,
    CompilerOptionValidationLocation, CompilerOptions, MissingResolutionError, ModuleExtension,
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

impl CliEmitSessionOutcome {
    /// tsc-port: emitFilesAndReportErrors @6.0.3
    /// tsc-hash: 9dc0128691c9a1bee5aeae85524cc8e2679b3905a4416a41095452e509951a8d
    /// tsc-span: _tsc.js:129412-129467
    fn into_reported(
        self,
        additional_diagnostics: &[Diagnostic],
    ) -> (EmitOutcome, DiagnosticList, NoEmitWorkCounters) {
        let mut diagnostics = self.config_diagnostics;
        diagnostics.extend(self.syntactic_diagnostics.iter().cloned());
        if self.syntactic_diagnostics.is_empty() {
            let options_are_empty =
                self.options_diagnostics.is_empty() && additional_diagnostics.is_empty();
            diagnostics.extend(self.options_diagnostics);
            diagnostics.extend(additional_diagnostics.iter().cloned());
            let global_is_empty = self.global_diagnostics.is_empty();
            diagnostics.extend(self.global_diagnostics);
            if options_are_empty && global_is_empty {
                diagnostics.extend(self.semantic_diagnostics);
            }
        }
        diagnostics.extend(self.emit.diagnostics().iter().cloned());
        sort_and_dedupe_diagnostics(&mut diagnostics);
        (self.emit, diagnostics, self.work_counters)
    }
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
            source.implied_node_format_for_emit(),
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
            source.implied_node_format_for_emit(),
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
    fn source_request_plan(
        &self,
        source_file: SourceFileId,
        source: &PreparedSourceFile,
    ) -> Result<SourceRequestPlan, AuthoritativeModuleLookupFailure> {
        if let Some(plan) = self.request_plans.borrow().get(&source_file) {
            return Ok(plan.clone());
        }
        let plan =
            plan_source_requests(source, self.prepared.compiler_options()).map_err(|_| {
                AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                )
            })?;
        self.request_plans
            .borrow_mut()
            .insert(source_file, plan.clone());
        Ok(plan)
    }

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
        let plan = self.source_request_plan(source_file, source)?;
        let loads_source = plan.module_request_loads_source(key).ok_or(
            AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
            ),
        )?;
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
        let resolution = match self.prepared.resolutions().require_module(&key) {
            Ok(resolution) => resolution,
            Err(_) => {
                let plan = self.source_request_plan(source_file, source)?;
                if plan
                    .unpreprocessed_module_requests()
                    .any(|unpreprocessed| unpreprocessed == &key)
                {
                    return Ok(AuthoritativeModuleResolution::NotFound(
                        AuthoritativeNotFoundModule {
                            alternate_result: None,
                        },
                    ));
                }
                return Err(AuthoritativeModuleLookupFailure::Missing);
            }
        };
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
            let jsx_syntax_extension = matches!(
                module.extension(),
                ModuleExtension::Tsx | ModuleExtension::Jsx
            );
            if !module.extension().is_javascript()
                && !jsx_syntax_extension
                && !arbitrary_declaration
                && !matches!(reason, UnloadedModuleReason::NoResolve)
            {
                return Err(AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetExtension,
                ));
            }
            if jsx_syntax_extension
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
                    if jsx_syntax_extension
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
            if let Some(diagnostic) = resolution_diagnostic {
                return Ok(AuthoritativeModuleResolution::ResolutionDiagnostic(
                    AuthoritativeResolutionDiagnosticModule {
                        resolved_file_name,
                        diagnostic,
                    },
                ));
            }
            return Ok(AuthoritativeModuleResolution::Untyped(
                AuthoritativeUntypedModule {
                    resolved_file_name,
                    package_name: module
                        .package_id()
                        .map(|package_id| package_id.name().to_owned()),
                    alternate_result,
                    types_package_exists: resolution.types_package_exists(),
                    package_bundles_types: resolution.package_bundles_types(),
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
        self.run_with_no_emit_canary(
            false,
            LibraryPrefixCompletion::Complete,
            &mut no_emit_canary,
        )
    }

    /// Prepare a bounded, exact-match library prefix for harness repetitions.
    /// The returned value is owned by the caller and is never inserted into a
    /// process-lifetime cache.
    #[doc(hidden)]
    pub fn prepare_harness_lib_bundle(&self) -> Result<Option<OwnedHarnessLibBundle>, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let inputs = project_checker_inputs(&self.prepared)?;
        Ok(prepare_authoritative_harness_lib_bundle(
            &inputs.libs,
            &inputs.files,
            self.prepared.compiler_options(),
        ))
    }

    /// Consume the prepared program through the separately typed emit path.
    pub fn emit(self, sink: &mut dyn OutputSink) -> Result<EmitOutcome, DriverError> {
        self.emit_with_command_outcome(sink, None)
            .map(|outcome| outcome.emit)
    }

    /// Execute the emitting Program while retaining the exact diagnostic
    /// sequence reported by TypeScript's `emitFilesAndReportErrors` wrapper.
    ///
    /// This is a qualification-only projection. Product callers should use
    /// [`emit`](Self::emit), while the CLI adds its own option diagnostics at
    /// the same owned boundary.
    #[doc(hidden)]
    pub fn emit_with_reported_diagnostics_for_harness(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<(EmitOutcome, DiagnosticList), DriverError> {
        self.emit_with_command_outcome(sink, None).map(|outcome| {
            let (emit, diagnostics, _) = outcome.into_reported(&[]);
            (emit, diagnostics)
        })
    }

    /// Emit through the harness-only bounded library-prefix scope.
    #[doc(hidden)]
    pub fn emit_with_reported_diagnostics_for_harness_with_lib_bundle(
        self,
        sink: &mut dyn OutputSink,
        bundle: Option<&OwnedHarnessLibBundle>,
    ) -> Result<(EmitOutcome, DiagnosticList), DriverError> {
        self.emit_with_command_outcome(sink, bundle).map(|outcome| {
            let (emit, diagnostics, _) = outcome.into_reported(&[]);
            (emit, diagnostics)
        })
    }

    pub(crate) fn emit_for_cli(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<CliEmitSessionOutcome, DriverError> {
        self.emit_with_command_outcome(sink, None)
    }

    /// h2-6a-m-2 §8-A.1 harness-print bridge: run the production
    /// plan → checker-resolver → transform → print pipeline and return
    /// each script unit's printed text (with an optionally injected
    /// source-map recording), WITHOUT artifacts, sinks, activity
    /// accounting, or the emit option preflight. Qualification-only:
    /// the replay suite byte-compares the returned units against the
    /// frozen witnesses; production emits keep every refusal lane.
    #[doc(hidden)]
    pub fn print_units_with_source_map_recording_for_harness(
        self,
        recording_inputs_for: &dyn Fn(&std::path::Path) -> Option<SourceMapRecordingInputs>,
    ) -> Result<Vec<(std::path::PathBuf, PrintedText)>, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let prepared = self.prepared;
        let emit_host = PreparedEmitHost::new(&prepared)?;
        let selection = EmitSelection::WholeProgram;
        let preflight = preflight_emit(&emit_host, selection).map_err(DriverError::Emit)?;

        let inputs = project_checker_inputs(&prepared)?;
        let provider = PreparedModuleProvider {
            prepared: &prepared,
            request_plans: RefCell::new(BTreeMap::new()),
        };
        let mut print_result: Option<Result<Vec<(std::path::PathBuf, PrintedText)>, DriverError>> =
            None;
        let mut operation =
            |snapshot: &ProgramSnapshot, checker: &CheckerSession<'_>, checked: &CheckResult| {
                if print_result.is_some() {
                    return;
                }
                if let Some(partial) = checked.partial_checks.first() {
                    print_result = Some(Err(DriverError::IncompleteCheck {
                        file_name: partial.file_name.clone(),
                        start: partial.start,
                        length: partial.length,
                        reason: partial.reason.clone(),
                        additional_partial_checks: checked.partial_checks.len().saturating_sub(1),
                    }));
                    return;
                }
                let checked_host = CheckedEmitHost {
                    prepared: &emit_host,
                    snapshot,
                };
                print_result = Some(checker.with_emit_resolver(|resolver| {
                    print_script_units_with_recording_for_harness(
                        resolver,
                        &checked_host,
                        &preflight,
                        recording_inputs_for,
                    )
                    .map_err(DriverError::Emit)
                }));
            };
        let checked = check_program_with_authoritative_modules_at_for_emit(
            &inputs.libs,
            &inputs.files,
            &inputs.lib_metadata,
            &inputs.file_metadata,
            prepared.compiler_options(),
            &inputs.current_directory,
            &provider,
            &mut operation,
        );
        drop(checked);
        print_result.expect("the checked emit callback runs exactly once")
    }

    fn emit_with_command_outcome(
        self,
        sink: &mut dyn OutputSink,
        harness_lib_bundle: Option<&OwnedHarnessLibBundle>,
    ) -> Result<CliEmitSessionOutcome, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let prepared = self.prepared;
        let mut h2_activity = H2ActivityCanary::h2_7b_profile();
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
        let mut operation =
            |snapshot: &ProgramSnapshot, checker: &CheckerSession<'_>, checked: &CheckResult| {
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
            };
        let checked = if let Some(bundle) = harness_lib_bundle {
            check_program_with_authoritative_modules_at_for_emit_with_harness_lib_bundle(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                bundle,
                &mut operation,
            )
        } else {
            check_program_with_authoritative_modules_at_for_emit(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                &mut operation,
            )
        }
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
        self.run_with_no_emit_canary(true, LibraryPrefixCompletion::Complete, &mut no_emit_canary)
    }

    /// Conformance-runner execution: the lib-cache harness path with the
    /// library-prefix completion pass elided.
    ///
    /// The runner compares only [`NoEmitOutcome::conformance_diagnostics`]
    /// and [`NoEmitOutcome::syntactic_diagnostics`], which are assembled
    /// from the fixture projections before the whole-Program completion
    /// pass and therefore cannot observe it. Without `skipDefaultLibCheck`
    /// that pass checks the standard library prefix (~1s per program), so
    /// eliding it is pure cost removal for this consumer. Any session whose
    /// whole-Program semantic surface is itself compared — the production
    /// CLI, qualification suites, and every emit path — must keep
    /// [`Self::run`]/[`Self::run_for_harness_with_lib_cache`].
    /// tsrs-native: consumer-scoped execution mode; no tsc counterpart.
    #[doc(hidden)]
    pub fn run_for_conformance_harness(self) -> Result<NoEmitOutcome, DriverError> {
        self.require_mode(PreparedProgramMode::NoEmit)?;
        let mut no_emit_canary = no_emit_canary::NoEmitCanary::new();
        self.run_with_no_emit_canary(
            true,
            LibraryPrefixCompletion::FixtureObservedOnly,
            &mut no_emit_canary,
        )
    }

    pub(crate) fn run_with_no_emit_canary(
        self,
        harness_lib_cache: bool,
        library_prefix: LibraryPrefixCompletion,
        no_emit_canary: &mut no_emit_canary::NoEmitCanary,
    ) -> Result<NoEmitOutcome, DriverError> {
        self.run_inner(harness_lib_cache, library_prefix, no_emit_canary)
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
        library_prefix: LibraryPrefixCompletion,
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
                library_prefix,
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
        available_options.extend(programmatic_option_diagnostics(&self.prepared));
        let mut available_semantic = checked
            .program_semantic_diagnostics
            .expect("authoritative checker sessions publish whole-Program semantic diagnostics");
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

    for package in prepared.packages() {
        let display_path = package.package_json().display();
        let name = display_path
            .to_str()
            .ok_or_else(|| DriverError::NonUnicodeDisplayPath {
                source_file: None,
                path: display_path.to_path_buf(),
            })?
            .to_owned();
        files.push(InputFile::host_only_from_snapshot(
            name,
            Arc::clone(package.snapshot()),
        ));
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
///
/// `Program.getGlobalDiagnostics` exposes the checker's global rows only when
/// the original `rootNames` list is non-empty. This matters for compiler-suite
/// JavaScript inputs rejected before `createProgram` because `allowJs` is off:
/// no default library is loaded, but the resulting empty Program must not
/// publish the checker's internal missing-global bootstrap rows.
///
/// tsc-port: getGlobalDiagnostics @6.0.3
/// tsc-hash: 6158e0d2a7114fa2a8b180f439cba3a3694ed722c664c2bab405b113541a32b4
/// tsc-span: _tsc.js:124038-124040
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
    options.extend(programmatic_option_diagnostics(prepared));
    let mut semantic = checked
        .program_semantic_diagnostics
        .as_ref()
        .expect("authoritative checker sessions publish whole-Program semantic diagnostics")
        .clone();
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
    let global = if prepared.roots().is_empty() {
        Vec::new()
    } else {
        checked.global_diagnostics.clone()
    };

    EmitSessionDiagnostics {
        config: preparation.config().to_vec(),
        options,
        syntactic,
        global,
        semantic,
    }
}

/// Produce the option diagnostics created by `createProgram`. Config-backed
/// validation diagnostics remain owned by `ConfigRootPlan`, while removed
/// and deprecated options are checked over the merged effective options and
/// skip names which already have config syntax.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (lib/noLib block)
/// tsc-hash: 6cc5d6e4258b1645ed0788fb31322db101b9e6b9ae34f203e749610f23e48fb3
/// tsc-span: _tsc.js:124888-124890
/// tsc-port: verifyCompilerOptions @6.0.3 (module/moduleResolution arms)
/// tsc-hash: 27def76917aef23a76e4b9d8b2036c28d04e47b44ef525ac578c6a1d48518e2d
/// tsc-span: _tsc.js:125007-125017
///
/// tsc-port: verifyDeprecatedCompilerOptions @6.0.3
/// tsc-hash: 2565bc5d5347775444bdbd8c11a3cc1ff2411d066648ec1f7786a231ec23a112
/// tsc-span: _tsc.js:125087-125250
fn programmatic_option_diagnostics(prepared: &PreparedProgram) -> DiagnosticList {
    let options = prepared.compiler_options();
    let external_config_option_diagnostics = prepared
        .program_options()
        .external_config_option_diagnostics();

    // The no-emit project adapter returns its ConfigRootPlan alongside the
    // PreparedProgram and keeps option-diagnostic ownership there. Emitting
    // project adapters instead need this Program pass for their merged
    // runner overlays, which are absent from the config syntax.
    if external_config_option_diagnostics && options.no_emit == Some(true) {
        return Vec::new();
    }

    let mut diagnostics = Vec::new();
    if !external_config_option_diagnostics {
        for violation in validate_compiler_options(options) {
            push_programmatic_option_diagnostic(
                prepared,
                &mut diagnostics,
                violation.option_names(),
                match violation.location() {
                    CompilerOptionValidationLocation::Name => {
                        ProgrammaticOptionDiagnosticLocation::Name
                    }
                    CompilerOptionValidationLocation::Value => {
                        ProgrammaticOptionDiagnosticLocation::Value
                    }
                },
                true,
                violation.message(),
            );
        }
        if options.lib.is_some() && prepared.program_options().no_lib() == Some(true) {
            push_programmatic_option_diagnostic(
                prepared,
                &mut diagnostics,
                &["lib", "noLib"],
                ProgrammaticOptionDiagnosticLocation::Name,
                true,
                MessageChain::new(
                    &gen::Option_0_cannot_be_specified_with_option_1,
                    &["lib".to_owned(), "noLib".to_owned()],
                ),
            );
        }
        let module_kind = options.emit_module_kind();
        let module_resolution = options.emit_module_resolution_kind();
        if module_resolution == 100 && !matches!(module_kind, 1 | 5..=99 | 200) {
            push_programmatic_option_diagnostic(
                prepared,
                &mut diagnostics,
                &["moduleResolution"],
                ProgrammaticOptionDiagnosticLocation::Value,
                false,
                MessageChain::new(
                    &gen::Option_0_can_only_be_used_when_module_is_set_to_preserve_commonjs_or_es2015_or_later,
                    &["bundler".to_owned()],
                ),
            );
        }
        if (3..=99).contains(&module_resolution) && !(100..=199).contains(&module_kind) {
            let module_resolution_name = if module_resolution == 99 {
                "NodeNext"
            } else {
                "Node16"
            };
            push_programmatic_option_diagnostic(
                prepared,
                &mut diagnostics,
                &["module"],
                ProgrammaticOptionDiagnosticLocation::Value,
                false,
                MessageChain::new(
                    &gen::Option_module_must_be_set_to_0_when_option_moduleResolution_is_set_to_1,
                    &[
                        module_resolution_name.to_owned(),
                        module_resolution_name.to_owned(),
                    ],
                ),
            );
        } else if (100..=199).contains(&module_kind)
            && options.module_resolution.is_some()
            && !(3..=99).contains(&module_resolution)
        {
            let module_kind_name = match module_kind {
                100 => "Node16",
                101 => "Node18",
                102 => "Node20",
                199 => "NodeNext",
                _ => "Node16",
            };
            let module_resolution_name = if module_kind == 199 {
                "NodeNext"
            } else {
                "Node16"
            };
            push_programmatic_option_diagnostic(
                prepared,
                &mut diagnostics,
                &["moduleResolution"],
                ProgrammaticOptionDiagnosticLocation::Value,
                false,
                MessageChain::new(
                    &gen::Option_moduleResolution_must_be_set_to_0_or_left_unspecified_when_option_module_is_set_to_1,
                    &[
                        module_resolution_name.to_owned(),
                        module_kind_name.to_owned(),
                    ],
                ),
            );
        }
    }

    if options.target == Some(0) {
        push_programmatic_removed_option_value(prepared, &mut diagnostics, "target", "ES3");
    }
    for (enabled, name) in [
        (
            options.no_implicit_use_strict == Some(true),
            "noImplicitUseStrict",
        ),
        (options.keyof_strings_only == Some(true), "keyofStringsOnly"),
        (
            options.suppress_excess_property_errors == Some(true),
            "suppressExcessPropertyErrors",
        ),
        (
            options.suppress_implicit_any_index_errors == Some(true),
            "suppressImplicitAnyIndexErrors",
        ),
        (
            options.no_strict_generic_checks == Some(true),
            "noStrictGenericChecks",
        ),
    ] {
        if enabled {
            push_programmatic_removed_option_name(prepared, &mut diagnostics, name, None);
        }
    }
    for (present, name) in [
        (
            options
                .charset
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "charset",
        ),
        (
            options
                .out
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "out",
        ),
    ] {
        if present {
            push_programmatic_removed_option_name(prepared, &mut diagnostics, name, None);
        }
    }
    if options
        .imports_not_used_as_values
        .is_some_and(|value| value != 0)
    {
        push_programmatic_removed_option_name(
            prepared,
            &mut diagnostics,
            "importsNotUsedAsValues",
            Some("verbatimModuleSyntax"),
        );
    }
    if options.preserve_value_imports == Some(true) {
        push_programmatic_removed_option_name(
            prepared,
            &mut diagnostics,
            "preserveValueImports",
            Some("verbatimModuleSyntax"),
        );
    }

    if let Some(value) = options.ignore_deprecations.as_deref() {
        // tsc getIgnoreDeprecationsVersion (_tsc.js:125052-125061) accepts
        // exactly "5.0" and "6.0"; any other value reports 5103 once
        // (reportInvalidIgnoreDeprecations, _tsc.js:122639) while the
        // deprecation rows below still fire.
        if !matches!(value, "5.0" | "6.0") {
            push_programmatic_option_diagnostic(
                prepared,
                &mut diagnostics,
                &["ignoreDeprecations"],
                ProgrammaticOptionDiagnosticLocation::Value,
                true,
                MessageChain::new(&gen::Invalid_value_for_ignoreDeprecations, &[]),
            );
        }
    }
    if options.ignore_deprecations.as_deref() != Some("6.0") {
        if options.target == Some(1) {
            push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "target",
                "ES5",
                false,
            );
        }
        if options.always_strict == Some(false) {
            push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "alwaysStrict",
                "false",
                false,
            );
        }
        match options.module_resolution {
            Some(1) => push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "moduleResolution",
                "classic",
                false,
            ),
            Some(2) => push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "moduleResolution",
                "node10",
                true,
            ),
            _ => {}
        }
        if options.base_url.is_some() {
            push_programmatic_option_deprecation_name(prepared, &mut diagnostics, "baseUrl", true);
        }
        if options.es_module_interop == Some(false) {
            push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "esModuleInterop",
                "false",
                false,
            );
        }
        if options.allow_synthetic_default_imports == Some(false) {
            push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "allowSyntheticDefaultImports",
                "false",
                false,
            );
        }
        if options.out_file.is_some() {
            push_programmatic_option_deprecation_name(prepared, &mut diagnostics, "outFile", false);
        }
        if options.downlevel_iteration.is_some() {
            push_programmatic_option_deprecation_name(
                prepared,
                &mut diagnostics,
                "downlevelIteration",
                false,
            );
        }
        let module_name = match options.module {
            Some(0) => Some("None"),
            Some(2) => Some("AMD"),
            Some(3) => Some("UMD"),
            Some(4) => Some("System"),
            _ => None,
        };
        if let Some(module_name) = module_name {
            push_programmatic_option_deprecation_value(
                prepared,
                &mut diagnostics,
                "module",
                module_name,
                false,
            );
        }
    }
    if !external_config_option_diagnostics {
        diagnostics.extend(validate_paths_option_diagnostics(
            options,
            prepared.program_options(),
        ));
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

#[derive(Clone, Copy)]
enum ProgrammaticOptionDiagnosticLocation {
    Name,
    Value,
}

/// tsc-port: createDiagnosticForOption @6.0.3
/// tsc-hash: 24da25470bdd02c4cde5520b78ea191837823bf1df686438144a8106edfd5f53
/// tsc-span: _tsc.js:125368-125386
fn push_programmatic_option_diagnostic(
    prepared: &PreparedProgram,
    diagnostics: &mut Vec<Diagnostic>,
    names: &[&str],
    location: ProgrammaticOptionDiagnosticLocation,
    use_compiler_options_fallback: bool,
    message: MessageChain,
) {
    let Some(config_file) = prepared.program_options().config_file() else {
        diagnostics.push(Diagnostic::new(None, None, None, message));
        return;
    };

    if prepared
        .program_options()
        .external_config_option_diagnostics()
        && names
            .iter()
            .any(|name| !config_file.compiler_option_name_locations(name).is_empty())
    {
        return;
    }

    let mut locations = names
        .iter()
        .flat_map(|name| match location {
            ProgrammaticOptionDiagnosticLocation::Name => {
                config_file.compiler_option_name_locations(name)
            }
            ProgrammaticOptionDiagnosticLocation::Value => {
                config_file.compiler_option_value_locations(name)
            }
        })
        .copied()
        .collect::<Vec<_>>();
    locations.sort_unstable_by_key(|location| location.start());
    if locations.is_empty() && use_compiler_options_fallback {
        locations.extend(config_file.compiler_options_location());
    }
    if locations.is_empty() {
        diagnostics.push(Diagnostic::new(None, None, None, message));
        return;
    }

    let file_name = config_file.diagnostic_file_name().to_owned();
    diagnostics.extend(locations.into_iter().map(|location| {
        Diagnostic::new(
            Some(file_name.clone()),
            Some(location.start()),
            Some(location.length()),
            message.clone(),
        )
    }));
}

fn push_programmatic_option_deprecation_value(
    prepared: &PreparedProgram,
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    value: &str,
    related: bool,
) {
    let mut message = MessageChain::new(
        &gen::Option_0_1_is_deprecated_and_will_stop_functioning_in_TypeScript_2_Specify_compilerOption_ignoreDeprecations_3_to_silence_this_error,
        &[
            name.to_owned(),
            value.to_owned(),
            "7.0".to_owned(),
            "6.0".to_owned(),
        ],
    );
    if related {
        message = message.with_next(vec![MessageChain::new(
            &gen::Visit_https_aka_ms_ts6_for_migration_information,
            &[],
        )]);
    }
    push_programmatic_option_diagnostic(
        prepared,
        diagnostics,
        &[name],
        ProgrammaticOptionDiagnosticLocation::Value,
        true,
        message,
    );
}

fn push_programmatic_removed_option_value(
    prepared: &PreparedProgram,
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    value: &str,
) {
    push_programmatic_option_diagnostic(
        prepared,
        diagnostics,
        &[name],
        ProgrammaticOptionDiagnosticLocation::Value,
        true,
        MessageChain::new(
            &gen::Option_0_1_has_been_removed_Please_remove_it_from_your_configuration,
            &[name.to_owned(), value.to_owned()],
        ),
    );
}

fn push_programmatic_removed_option_name(
    prepared: &PreparedProgram,
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    use_instead: Option<&str>,
) {
    let mut message = MessageChain::new(
        &gen::Option_0_has_been_removed_Please_remove_it_from_your_configuration,
        &[name.to_owned()],
    );
    if let Some(use_instead) = use_instead {
        message = message.with_next(vec![MessageChain::new(
            &gen::Use_0_instead,
            &[use_instead.to_owned()],
        )]);
    }
    push_programmatic_option_diagnostic(
        prepared,
        diagnostics,
        &[name],
        ProgrammaticOptionDiagnosticLocation::Name,
        true,
        message,
    );
}

fn push_programmatic_option_deprecation_name(
    prepared: &PreparedProgram,
    diagnostics: &mut Vec<Diagnostic>,
    name: &str,
    related: bool,
) {
    let mut message = MessageChain::new(
        &gen::Option_0_is_deprecated_and_will_stop_functioning_in_TypeScript_1_Specify_compilerOption_ignoreDeprecations_2_to_silence_this_error,
        &[name.to_owned(), "7.0".to_owned(), "6.0".to_owned()],
    );
    if related {
        message = message.with_next(vec![MessageChain::new(
            &gen::Visit_https_aka_ms_ts6_for_migration_information,
            &[],
        )]);
    }
    push_programmatic_option_diagnostic(
        prepared,
        diagnostics,
        &[name],
        ProgrammaticOptionDiagnosticLocation::Name,
        true,
        message,
    );
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
