//! A Program kept alive between requests (tsgo's `compiler.Program` and the
//! checker pool of its project): the batch session's checker inputs and
//! module provider over an owned [`PreparedProgram`], and a [`LiveChecker`]
//! that checks each file when its diagnostics are first asked for.

use std::collections::BTreeMap;
use std::sync::Arc;

use tsc_checker::live::{CheckerLifetime, LiveChecker, LiveCheckerInputs};
use tsc_checker::state::CheckerState;
use tsc_checker::{
    check_directive, AuthoritativeSourceToken, BoundDocument, CheckResult, DocumentRegistry,
    PartialCheck,
};
use tsc_diagnostics::{
    sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, DocumentVersion, TextSnapshot,
};
use tsc_emitter::{
    emit_forced_declarations, emit_forced_javascript, emit_planned_files, preflight_emit,
    validate_emit_request, EmitDiagnosticGate, EmitOutcome, EmitRouteKind, EmitSelection,
    OutputSink, UnitEmitRequest,
};
use tsc_program::{
    plan_source_requests, plan_source_requests_retaining_syntax, CanonicalPath, CompilerOptions,
    PreparedProgram, PreparedProgramMode, PreparedSourceFile, ProgramPath, SourceFileId,
    WorkerBudget,
};
use tsc_types::{JsStr, JsString};

use crate::{
    get_emit_declarations, include_processor_rows, map_authoritative_failure,
    program_rows_outside_sources, programmatic_option_diagnostics, project_checker_inputs,
    CheckedEmitHost, DeclarationSession, DriverError, PreparedEmitHost, PreparedModuleProvider,
};

/// One Program and its checkers, kept for queries (tsgo `compiler.Program`
/// with its project's checker pool).
pub struct LiveProgram {
    prepared: Arc<PreparedProgram>,
    checker: LiveChecker,
    /// tsgo `Program.declarationDiagnosticCache`: each file's declaration
    /// diagnostics (by checker index), whichever checker computed them.
    declaration_diagnostics: BTreeMap<usize, DiagnosticList>,
}

/// tsgo `compiler.EmitOnly` of an emit request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmitOnly {
    All,
    Js,
    Dts,
}

/// tsgo `compiler.EmitResult` without the source maps: the emitted files in
/// order, and the diagnostics of the emit (each file's in turn), or the
/// diagnostics `noEmitOnError` stopped it with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EmitResult {
    pub emit_skipped: bool,
    pub diagnostics: DiagnosticList,
    pub emitted_files: Vec<JsString>,
}

impl EmitResult {
    /// tsgo `CombineEmitResults`.
    fn combine(&mut self, outcome: EmitOutcome) {
        self.emit_skipped |= outcome.emit_skipped();
        self.diagnostics
            .extend(outcome.diagnostics().iter().cloned());
        self.emitted_files
            .extend(outcome.emitted_files().unwrap_or_default().iter().cloned());
    }
}

impl LiveProgram {
    /// Parse and bind the Program's sources and initialize its checker; no
    /// source is checked until its diagnostics or a query asks for it.
    pub fn new(prepared: PreparedProgram) -> Result<Self, DriverError> {
        Self::build(prepared, None)
    }

    /// [`Self::new`] over the documents Programs share (tsgo's parse cache):
    /// a source whose text and parse a Program already holds is that
    /// document, and a new one is recorded in `documents`.
    pub fn with_documents(
        prepared: PreparedProgram,
        documents: &DocumentRegistry,
    ) -> Result<Self, DriverError> {
        Self::build(prepared, Some(documents))
    }

    /// tsgo `ReuseProgram`: the Program `prepared` with the source at `path`
    /// read again as `text`, sharing the resolutions, the Program
    /// diagnostics and (through `documents`) every other file. `None` when
    /// the file cannot be replaced in place (tsgo `canReplaceFileInProgram`):
    /// its requests change (the imports and their modes, module
    /// augmentations, path, type and lib references, module status), its
    /// check directive changes, it has the synthesized `tslib` or JSX
    /// runtime import, it takes part in a package redirect, or a Program row
    /// is located in it (tsgo locates those in the new file).
    pub fn reuse(
        prepared: &PreparedProgram,
        path: &CanonicalPath,
        text: String,
        documents: &DocumentRegistry,
    ) -> Option<Result<Self, DriverError>> {
        let id = prepared.source_id(path)?;
        let old = prepared.source_file(id)?;
        if old.path().canonical() != path || !old.package_redirect_paths().is_empty() {
            return None;
        }
        if include_processor_rows(prepared)
            .iter()
            .any(|(source, _)| *source == id)
        {
            return None;
        }
        let options = options_for_source(prepared, old)?;
        let new = old.with_snapshot(TextSnapshot::new(text, DocumentVersion::default()));
        let old_plan = plan_source_requests(old, options).ok()?;
        let (new_plan, syntax) = plan_source_requests_retaining_syntax(&new, options).ok()?;
        if !old_plan.has_same_requests(&new_plan)
            || new_plan.has_synthetic_imports()
            || check_directive(old.text()) != check_directive(new.text())
        {
            return None;
        }
        Some(Self::build(
            prepared.with_replaced_source(id, new.with_preparsed_syntax(syntax)),
            Some(documents),
        ))
    }

    fn build(
        prepared: PreparedProgram,
        documents: Option<&DocumentRegistry>,
    ) -> Result<Self, DriverError> {
        let prepared = Arc::new(prepared);
        let inputs = project_checker_inputs(&prepared, &BTreeMap::new())?;
        let provider = PreparedModuleProvider {
            prepared: Arc::clone(&prepared),
            request_plans: std::sync::Mutex::new(BTreeMap::new()),
            tracing: None,
        };
        let checker = LiveChecker::new(
            LiveCheckerInputs {
                libs: inputs.libs,
                files: inputs.files,
                lib_metadata: inputs.lib_metadata,
                file_metadata: inputs.file_metadata,
                options: prepared.compiler_options().clone(),
                current_directory: inputs.current_directory,
            },
            Box::new(provider),
            WorkerBudget::serial(),
            documents,
        )
        .map_err(|failure| map_authoritative_failure(&prepared, failure))?;
        Ok(Self {
            prepared,
            checker,
            declaration_diagnostics: BTreeMap::new(),
        })
    }

    pub fn prepared(&self) -> &PreparedProgram {
        &self.prepared
    }

    /// The prepared program, shared.
    pub fn shared_prepared(&self) -> Arc<PreparedProgram> {
        Arc::clone(&self.prepared)
    }

    /// The checker's files, libraries first.
    pub fn file_count(&self) -> usize {
        self.checker.file_count()
    }

    /// The checker index of the file named `file_name` (as the Program
    /// spells it).
    pub fn file_index<'n>(&self, file_name: impl Into<JsStr<'n>>) -> Option<usize> {
        let file_name = file_name.into();
        (0..self.checker.file_count())
            .find(|&index| self.checker.file_name(index) == Some(file_name))
    }

    /// The parsed and bound document of the checker's file at `index`: the
    /// same one in every Program built over the same [`DocumentRegistry`]
    /// while the file's text and parse do not change.
    pub fn document(&self, index: usize) -> Option<&Arc<BoundDocument>> {
        self.checker.document(index)
    }

    /// The file name of the checker's file at `index`.
    pub fn file_name(&self, index: usize) -> Option<JsStr<'_>> {
        self.checker.file_name(index)
    }

    /// tsgo `GetConfigFileParsingDiagnostics`.
    pub fn config_file_parsing_diagnostics(&self) -> &[Diagnostic] {
        self.prepared.diagnostics().config()
    }

    /// tsgo `GetProgramDiagnostics`: the options rows of the preparation,
    /// the programmatic option rows, the program rows located outside the
    /// sources (the include processor's global rows) and, for a Program
    /// that emits, the output-path rows tsgo's `verifyCompilerOptions` adds
    /// (the batch session's emit preflight), sorted and deduplicated. The
    /// content mapper's rows are not ported.
    pub fn program_diagnostics(&self) -> DiagnosticList {
        let mut options = self.prepared.diagnostics().options().to_vec();
        options.extend(programmatic_option_diagnostics(&self.prepared));
        options.extend(program_rows_outside_sources(&self.prepared));
        if self.prepared.mode() == PreparedProgramMode::Emit {
            options.extend(
                PreparedEmitHost::new_for_route(
                    &self.prepared,
                    EmitRouteKind::Program,
                    &BTreeMap::new(),
                )
                .ok()
                .filter(|host| validate_emit_request(host).is_ok())
                .and_then(|host| preflight_emit(&host, EmitSelection::WholeProgram).ok())
                .map(|preflight| preflight.diagnostics().to_vec())
                .unwrap_or_default(),
            );
        }
        sort_and_dedupe_diagnostics(&mut options);
        options
    }

    /// tsgo `GetBindDiagnostics(file)`.
    pub fn bind_diagnostics(&self, file: usize) -> DiagnosticList {
        let mut diagnostics = self.checker.bind_diagnostics(file).to_vec();
        sort_and_dedupe_diagnostics(&mut diagnostics);
        diagnostics
    }

    /// tsgo `GetSyntacticDiagnostics(file)`.
    pub fn syntactic_diagnostics(&self, file: usize) -> DiagnosticList {
        let mut diagnostics = self.checker.syntactic_diagnostics(file).to_vec();
        sort_and_dedupe_diagnostics(&mut diagnostics);
        diagnostics
    }

    /// tsgo `GetSemanticDiagnostics(file)`, checking the file first.
    pub fn semantic_diagnostics(&mut self, file: usize) -> Result<DiagnosticList, DriverError> {
        let mut diagnostics = self
            .checker
            .semantic_diagnostics(file)
            .map_err(|failure| map_authoritative_failure(&self.prepared, failure))?;
        sort_and_dedupe_diagnostics(&mut diagnostics);
        Ok(diagnostics)
    }

    /// tsgo `GetSuggestionDiagnostics(file)`, checking the file first.
    pub fn suggestion_diagnostics(&mut self, file: usize) -> Result<DiagnosticList, DriverError> {
        let mut diagnostics = self
            .checker
            .suggestion_diagnostics(file)
            .map_err(|failure| map_authoritative_failure(&self.prepared, failure))?;
        sort_and_dedupe_diagnostics(&mut diagnostics);
        Ok(diagnostics)
    }

    /// tsgo `GetGlobalDiagnostics`: the checker's global rows so far, none
    /// for a Program without root names.
    pub fn global_diagnostics(&self) -> DiagnosticList {
        if self.prepared.roots().is_empty() {
            return Vec::new();
        }
        self.checker.global_diagnostics()
    }

    /// tsgo `GetDeclarationDiagnostics(file)` of each of `files` on the
    /// diagnostics checker.
    pub fn declaration_diagnostics(
        &mut self,
        files: &[usize],
    ) -> Result<Vec<DiagnosticList>, DriverError> {
        self.declaration_diagnostics_on(files, CheckerLifetime::Diagnostics)
    }

    /// tsgo `getDeclarationDiagnosticsForFile` on the checker of
    /// `lifetime`: none for a declaration file, otherwise the declaration
    /// transform's rows, computed once for the Program. The diagnostics
    /// checker checks the file as its semantic diagnostics would.
    fn declaration_diagnostics_on(
        &mut self,
        files: &[usize],
        lifetime: CheckerLifetime,
    ) -> Result<Vec<DiagnosticList>, DriverError> {
        let pending = files
            .iter()
            .copied()
            .filter(|file| !self.declaration_diagnostics.contains_key(file))
            .filter_map(|file| Some((file, self.source_id(file)?)))
            .collect::<BTreeMap<_, _>>();
        if !pending.is_empty() {
            if lifetime == CheckerLifetime::Diagnostics {
                for &file in pending.keys() {
                    self.checker
                        .check_file(file)
                        .map_err(|failure| map_authoritative_failure(&self.prepared, failure))?;
                }
            }
            let prepared = Arc::clone(&self.prepared);
            let host = PreparedEmitHost::new_for_route(
                &prepared,
                EmitRouteKind::Program,
                &BTreeMap::new(),
            )?;
            let computed = self
                .checker
                .with_session(lifetime, |snapshot, session| {
                    let checked = CheckedEmitHost {
                        prepared: &host,
                        snapshot,
                        prepared_sources: None,
                    };
                    let mut declarations = DeclarationSession::new(
                        &prepared,
                        &checked,
                        Some(session),
                        &CheckResult::default(),
                    )?;
                    pending
                        .iter()
                        .map(|(&file, &source)| {
                            declarations
                                .get_declaration_diagnostics(EmitSelection::TargetSourceFile(
                                    source,
                                ))
                                .map(|diagnostics| (file, diagnostics))
                        })
                        .collect::<Result<Vec<_>, DriverError>>()
                })
                .transpose()?
                .unwrap_or_default();
            self.declaration_diagnostics.extend(computed);
        }
        Ok(files
            .iter()
            .map(|file| {
                self.declaration_diagnostics
                    .get(file)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect())
    }

    /// tsgo `Program.Emit` without a target or `ForceEmit`: the
    /// `noEmit`/`noEmitOnError` gate (tsgo `HandleNoEmitOptions` over the
    /// project's checker pool), then each emitted file's `emitOnly` outputs
    /// through `sink`, on the query checker.
    pub fn emit(
        &mut self,
        emit_only: EmitOnly,
        sink: &mut dyn OutputSink,
    ) -> Result<EmitResult, DriverError> {
        let options = self.prepared.compiler_options();
        if options.no_emit == Some(true) {
            return Ok(EmitResult::default());
        }
        if options.no_emit_on_error == Some(true) {
            let diagnostics = self.diagnostics_of_any_program()?;
            if !diagnostics.is_empty() {
                return Ok(EmitResult {
                    emit_skipped: true,
                    diagnostics,
                    emitted_files: Vec::new(),
                });
            }
        }
        let prepared = Arc::clone(&self.prepared);
        let host =
            PreparedEmitHost::new_for_route(&prepared, EmitRouteKind::Program, &BTreeMap::new())?
                .with_collected_emitted_files(true);
        let outcome = self
            .checker
            .with_session(CheckerLifetime::Temporary, |snapshot, session| {
                let checked = CheckedEmitHost {
                    prepared: &host,
                    snapshot,
                    prepared_sources: None,
                };
                incomplete_check(
                    session
                        .prepare_program_emit()
                        .map_err(|failure| map_authoritative_failure(&prepared, failure))?,
                )?;
                let preflight = preflight_emit(&checked, EmitSelection::WholeProgram)
                    .map_err(DriverError::Emit)?;
                let requests = (0..preflight.plan().units().len())
                    .map(|unit| UnitEmitRequest {
                        unit,
                        javascript: emit_only != EmitOnly::Dts,
                        declaration: emit_only != EmitOnly::Js,
                    })
                    .collect::<Vec<_>>();
                // The gate above was tsgo's; the emitter's has nothing left.
                let gate = EmitDiagnosticGate::default().with_declaration_diagnostics(Vec::new());
                session
                    .with_emit_resolver(|resolver| {
                        emit_planned_files(
                            resolver,
                            &checked,
                            preflight,
                            EmitSelection::WholeProgram,
                            &gate,
                            sink,
                            &requests,
                        )
                    })
                    .map_err(DriverError::Emit)
            })
            .transpose()?;
        let mut result = EmitResult::default();
        if let Some(outcome) = outcome {
            result.combine(outcome);
        }
        Ok(result)
    }

    /// tsgo `Program.Emit` with `ForceEmit` and the `files` as targets (in
    /// order): their JavaScript (`EmitOnlyJs`) or declaration (`EmitOnlyDts`)
    /// outputs whatever `noEmit`, `emitDeclarationOnly`, `declaration` and
    /// `noEmitOnError` say, on the query checker.
    pub fn emit_forced(
        &mut self,
        files: &[usize],
        javascript: bool,
        sink: &mut dyn OutputSink,
    ) -> Result<EmitResult, DriverError> {
        let sources = files
            .iter()
            .filter_map(|&file| self.source_id(file))
            .collect::<Vec<_>>();
        let prepared = Arc::clone(&self.prepared);
        let host =
            PreparedEmitHost::new_for_route(&prepared, EmitRouteKind::Program, &BTreeMap::new())?
                .with_collected_emitted_files(true);
        let mut result = EmitResult::default();
        let outcomes = self
            .checker
            .with_session(CheckerLifetime::Temporary, |snapshot, session| {
                let checked = CheckedEmitHost {
                    prepared: &host,
                    snapshot,
                    prepared_sources: None,
                };
                sources
                    .iter()
                    .map(|&source| {
                        let selection = EmitSelection::TargetSourceFile(source);
                        if !javascript {
                            return session
                                .with_emit_resolver(|resolver| {
                                    emit_forced_declarations(resolver, &checked, selection, sink)
                                })
                                .map_err(DriverError::Emit);
                        }
                        // The source is checked before its resolver answers
                        // the transforms, as the batch emit prepares it.
                        incomplete_check(
                            session
                                .prepare_declaration_source(AuthoritativeSourceToken(source.raw()))
                                .map_err(|failure| map_authoritative_failure(&prepared, failure))?,
                        )?;
                        session
                            .with_emit_resolver(|resolver| {
                                emit_forced_javascript(resolver, &checked, selection, sink)
                            })
                            .map_err(DriverError::Emit)
                    })
                    .collect::<Result<Vec<_>, DriverError>>()
            })
            .transpose()?;
        for outcome in outcomes.unwrap_or_default() {
            result.combine(outcome);
        }
        Ok(result)
    }

    /// tsgo `GetDiagnosticsOfAnyProgram` for a whole-Program emit
    /// (`HandleNoEmitOptions` with `noEmitOnError`) over the project's
    /// checker pool: the config rows, then the syntactic rows; without
    /// those the Program's rows; without those either, the semantic rows on
    /// the query checker (the checker's global rows are not among them, and
    /// the Program's are its pool's, none here) and, with declarations, the
    /// declaration rows.
    fn diagnostics_of_any_program(&mut self) -> Result<DiagnosticList, DriverError> {
        let mut diagnostics = self.config_file_parsing_diagnostics().to_vec();
        let config_rows = diagnostics.len();
        let files = (0..self.file_count()).collect::<Vec<_>>();
        let mut syntactic = files
            .iter()
            .flat_map(|&file| self.syntactic_diagnostics(file))
            .collect::<Vec<_>>();
        sort_and_dedupe_diagnostics(&mut syntactic);
        diagnostics.extend(syntactic);
        if diagnostics.len() != config_rows {
            return Ok(diagnostics);
        }
        diagnostics.extend(self.program_diagnostics());
        let options = self.prepared.compiler_options();
        if options.list_files_only == Some(true) {
            return Ok(diagnostics);
        }
        if diagnostics.len() == config_rows {
            let semantic = self
                .checker
                .program_semantic_diagnostics(CheckerLifetime::Temporary)
                .map_err(|failure| map_authoritative_failure(&self.prepared, failure))?;
            diagnostics.extend(semantic);
        }
        if get_emit_declarations(self.prepared.compiler_options())
            && diagnostics.len() == config_rows
        {
            let mut declarations = self
                .declaration_diagnostics_on(&files, CheckerLifetime::Temporary)?
                .concat();
            sort_and_dedupe_diagnostics(&mut declarations);
            diagnostics.extend(declarations);
        }
        Ok(diagnostics)
    }

    /// The prepared program's identity of the checker's file at `index`.
    fn source_id(&self, index: usize) -> Option<SourceFileId> {
        self.checker
            .source_token(index)
            .map(|token| SourceFileId::from_raw(token.0))
    }

    /// Run `query` over the Program's API checker (tsgo's persistent
    /// checker for the API's queries), kept apart from the checker the
    /// diagnostics come from; none for a Program without files, which has no
    /// checker.
    pub fn with_checker<T>(&mut self, query: impl FnOnce(&mut CheckerState<'_>) -> T) -> Option<T> {
        self.checker.with_checker(query)
    }
}

/// A check that stopped partway, as the batch sessions report it.
fn incomplete_check(partials: Vec<PartialCheck>) -> Result<(), DriverError> {
    match partials.first() {
        Some(partial) => Err(DriverError::IncompleteCheck {
            file_name: partial.file_name.clone(),
            start: partial.start,
            length: partial.length,
            reason: partial.reason.clone(),
            additional_partial_checks: partials.len().saturating_sub(1),
        }),
        None => Ok(()),
    }
}

/// The options a source's requests were planned with (tsgo
/// `getCompilerOptionsForFile`): its referenced project's, or the
/// Program's.
fn options_for_source<'p>(
    prepared: &'p PreparedProgram,
    source: &PreparedSourceFile,
) -> Option<&'p CompilerOptions> {
    if source.project_reference().is_none() {
        return Some(prepared.compiler_options());
    }
    let project = prepared
        .program_options()
        .project_references()?
        .project_for_resolution(source.path().canonical())?;
    let root_config = prepared
        .program_options()
        .config_file_path()
        .map(ProgramPath::canonical);
    (root_config != Some(project.canonical())).then(|| project.compiler_options())
}
