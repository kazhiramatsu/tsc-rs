use std::collections::{btree_map::Entry, BTreeMap};

use tsc_checker::emit::CheckerSession;
use tsc_checker::AuthoritativeSourceToken;
use tsc_diagnostics::{sort_and_dedupe_diagnostics, DiagnosticList};
use tsc_emitter::{
    get_declaration_diagnostics, EmitContractViolation, EmitFailure, EmitHost, EmitSelection,
    H2ActivityCanary, H2ActivityCounters, H2RuntimeSlice, PlanDeclarationPaths,
};
use tsc_program::{PreparedProgram, SourceFileId};

use super::{map_authoritative_failure, DriverError};

/// Program diagnostics, declaration getters and emits over one initialized checker.
/// Each uncached non-declaration source prepares its resolver before the
/// emitter applies source eligibility. Only owned diagnostics are cached.
pub struct DeclarationSession<'session, 'program> {
    prepared: &'session PreparedProgram,
    host: &'session dyn EmitHost,
    checker: Option<&'session CheckerSession<'program>>,
    paths: PlanDeclarationPaths,
    cache: BTreeMap<SourceFileId, DiagnosticList>,
    activity: H2ActivityCanary,
    initial_diagnostics: tsc_checker::CheckResult,
}

impl<'session, 'program> DeclarationSession<'session, 'program> {
    pub(super) fn new(
        prepared: &'session PreparedProgram,
        host: &'session dyn EmitHost,
        checker: Option<&'session CheckerSession<'program>>,
        initial_diagnostics: &tsc_checker::CheckResult,
    ) -> Result<Self, DriverError> {
        let paths =
            PlanDeclarationPaths::for_declaration_diagnostics(host).map_err(DriverError::Emit)?;
        Ok(Self {
            prepared,
            host,
            checker,
            paths,
            cache: BTreeMap::new(),
            activity: H2ActivityCanary::h2_7e_profile(),
            initial_diagnostics: initial_diagnostics.clone(),
        })
    }

    /// tsc-port: getDiagnosticsHelper @6.0.3
    /// tsc-hash: e39fa16e7de6dea8d5d79332ae4afc22514aa1600c873cfe18cb7dd596fff252
    /// tsc-span: _tsc.js:123631-123641
    /// The per-file worker below retains the declaration getter's cache and
    /// source preparation order (_tsc.js:124005-124023).
    pub fn get_declaration_diagnostics(
        &mut self,
        selection: EmitSelection,
    ) -> Result<DiagnosticList, DriverError> {
        self.activity.observe_runtime_slice(H2RuntimeSlice::H2_7c);
        let options = self.host.compiler_options();
        if options
            .out_file
            .as_deref()
            .is_some_and(|path| !path.is_empty())
        {
            self.activity.observe_runtime_slice(H2RuntimeSlice::H2_7d);
        }
        if options.declaration_map == Some(true) {
            self.activity.observe_runtime_slice(H2RuntimeSlice::H2_7e);
        }
        self.declaration_diagnostics(selection)
    }

    // Internal noEmitOnError requests share the cache without crossing the
    // explicit public getter request boundary a second time.
    fn declaration_diagnostics(
        &mut self,
        selection: EmitSelection,
    ) -> Result<DiagnosticList, DriverError> {
        let sources = match selection {
            EmitSelection::WholeProgram => self.host.source_file_ids().to_vec(),
            EmitSelection::TargetSourceFile(source) => vec![source],
        };
        let mut diagnostics = Vec::new();
        for source in sources {
            let file = self.host.source_file(source).ok_or({
                DriverError::Emit(EmitFailure::Contract(
                    EmitContractViolation::PlannedSourceMissing(source),
                ))
            })?;
            let syntax = file.syntax().ok_or({
                DriverError::Emit(EmitFailure::Contract(
                    EmitContractViolation::CheckedSyntaxUnavailable(source),
                ))
            })?;
            if syntax.is_declaration_file {
                continue;
            }
            let cached = match self.cache.entry(source) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    let checker = self.checker.ok_or({
                        DriverError::Emit(EmitFailure::Contract(
                            EmitContractViolation::CheckedSyntaxUnavailable(source),
                        ))
                    })?;
                    let partials = checker
                        .prepare_declaration_source(AuthoritativeSourceToken(source.raw()))
                        .map_err(|failure| map_authoritative_failure(self.prepared, failure))?;
                    if let Some(partial) = partials.first() {
                        return Err(DriverError::IncompleteCheck {
                            file_name: partial.file_name.clone(),
                            start: partial.start,
                            length: partial.length,
                            reason: partial.reason.clone(),
                            additional_partial_checks: partials.len().saturating_sub(1),
                        });
                    }
                    self.activity.borrow_emit_resolver();
                    let result = checker
                        .with_emit_resolver(|resolver| {
                            get_declaration_diagnostics(
                                resolver,
                                self.host,
                                &self.paths,
                                source,
                                &mut self.activity,
                            )
                        })
                        .map_err(DriverError::Emit)?;
                    entry.insert(result)
                }
            };
            diagnostics.extend_from_slice(cached);
        }
        sort_and_dedupe_diagnostics(&mut diagnostics);
        Ok(diagnostics)
    }

    /// tsc-port: emitWorker @6.0.3 (forced declaration-only branch)
    /// tsc-hash: 55f7ba7bf02c6e2cae7d1c89577e5c98990352d46d16dd5092e68989e93fab80
    /// tsc-span: _tsc.js:123595-123625
    /// Force declaration-only output without scheduling semantic diagnostics.
    /// Every nonempty Program call borrows this same initialized checker.
    /// A source-less Program retains the existing unavailable-resolver adapter.
    pub fn emit_forced_declarations(
        &mut self,
        selection: EmitSelection,
        sink: &mut dyn tsc_emitter::OutputSink,
    ) -> Result<tsc_emitter::EmitOutcome, DriverError> {
        tsc_emitter::validate_forced_declaration_request(self.host).map_err(DriverError::Emit)?;
        if let Some(checker) = self.checker {
            self.activity.borrow_emit_resolver();
            checker
                .with_emit_resolver(|resolver| {
                    tsc_emitter::emit_forced_declarations_with_activity(
                        resolver,
                        self.host,
                        selection,
                        sink,
                        &mut self.activity,
                    )
                })
                .map_err(DriverError::Emit)
        } else {
            // Source-less Programs retain the existing Rust no-checker
            // adaptation. The unavailable resolver rejects any accidental
            // query; the forced plan has no source that can reach one.
            if !self.host.source_file_ids().is_empty() {
                return Err(DriverError::AuthoritativeResolution(
                    tsc_checker::AuthoritativeModuleFailure::InvalidMetadata {
                        detail: "nonempty declaration session requires an initialized checker"
                            .to_owned(),
                    },
                ));
            }
            tsc_emitter::emit_forced_declarations_with_activity(
                &tsc_emitter::UnavailableEmitResolver,
                self.host,
                selection,
                sink,
                &mut self.activity,
            )
            .map_err(DriverError::Emit)
        }
    }

    fn current_diagnostics(&self) -> tsc_checker::CheckResult {
        let mut checked = self.initial_diagnostics.clone();
        if let Some(checker) = self.checker {
            checked.global_diagnostics = checker.get_global_diagnostics();
        }
        checked
    }

    fn complete_semantics(
        &self,
        checked: &mut tsc_checker::CheckResult,
    ) -> Result<(), DriverError> {
        if let Some(checker) = self.checker {
            let (diagnostics, partials) = checker
                .get_program_semantic_diagnostics()
                .map_err(|failure| map_authoritative_failure(self.prepared, failure))?;
            checked.program_semantic_diagnostics = Some(diagnostics);
            checked.partial_checks = partials;
        }
        if let Some(partial) = checked.partial_checks.first() {
            return Err(DriverError::IncompleteCheck {
                file_name: partial.file_name.clone(),
                start: partial.start,
                length: partial.length,
                reason: partial.reason.clone(),
                additional_partial_checks: checked.partial_checks.len().saturating_sub(1),
            });
        }
        Ok(())
    }

    fn project_diagnostics(
        &self,
        checked: &tsc_checker::CheckResult,
        preflight_diagnostics: &[tsc_diagnostics::Diagnostic],
    ) -> super::ProgramDiagnostics {
        let mut diagnostics = super::emit_session_diagnostics(self.prepared, checked);
        diagnostics.options.extend_from_slice(preflight_diagnostics);
        sort_and_dedupe_diagnostics(&mut diagnostics.options);
        diagnostics
    }

    /// Observe options, syntactic, global and whole-Program semantic diagnostics
    /// in getter order without replacing this session's checker or declaration
    /// cache. Semantic checking completes lazily on the first such request.
    pub fn get_program_diagnostics(&mut self) -> Result<super::ProgramDiagnostics, DriverError> {
        let preflight = tsc_emitter::preflight_emit(self.host, EmitSelection::WholeProgram)
            .map_err(DriverError::Emit)?;
        let mut checked = self.current_diagnostics();
        self.complete_semantics(&mut checked)?;
        Ok(self.project_diagnostics(&checked, preflight.diagnostics()))
    }

    /// Run ordinary whole-Program emit and reporting on this initialized
    /// checker. The same public option guard as ProgramSession::emit applies;
    /// noEmit and ordinary target/emitOnly requests gain no alternate route.
    pub fn emit_with_reported_diagnostics(
        &mut self,
        sink: &mut dyn tsc_emitter::OutputSink,
    ) -> Result<super::EmitCommandOutcome, DriverError> {
        let actual = self.prepared.mode();
        if actual != tsc_program::PreparedProgramMode::Emit {
            return Err(DriverError::InvalidProgramMode {
                expected: tsc_program::PreparedProgramMode::Emit,
                actual,
            });
        }
        tsc_emitter::validate_bootstrap_emit_request(self.host).map_err(DriverError::Emit)?;
        self.activity.construct_emit_session();
        self.activity.construct_output_plan();
        let selection = EmitSelection::WholeProgram;
        let preflight =
            tsc_emitter::preflight_emit(self.host, selection).map_err(DriverError::Emit)?;
        let mut checked = self.current_diagnostics();
        let mut diagnostics = self.project_diagnostics(&checked, preflight.diagnostics());
        let options = self.host.compiler_options();
        // emitFilesAndReportErrors requests semantics only after its earlier
        // buckets are empty; handleNoEmitOptions requests all four buckets.
        if options.no_emit_on_error == Some(true)
            || (diagnostics.syntactic.is_empty()
                && diagnostics.options.is_empty()
                && diagnostics.global.is_empty())
        {
            self.complete_semantics(&mut checked)?;
            diagnostics = self.project_diagnostics(&checked, preflight.diagnostics());
        }
        let mut gate = diagnostics.gate();
        let mut blocked = options.no_emit_on_error == Some(true)
            && !(diagnostics.options.is_empty()
                && diagnostics.syntactic.is_empty()
                && diagnostics.global.is_empty()
                && diagnostics.semantic.is_empty());
        if options.no_emit_on_error == Some(true)
            && !blocked
            && (options.declaration == Some(true) || options.composite == Some(true))
        {
            let declarations = self.declaration_diagnostics(EmitSelection::WholeProgram)?;
            blocked = !declarations.is_empty();
            gate = gate.with_declaration_diagnostics(declarations);
        }
        let preflight_diagnostics = preflight.diagnostics().to_vec();
        let emit = if let Some(checker) = self.checker.filter(|_| !blocked) {
            let partials = checker
                .prepare_program_emit()
                .map_err(|failure| map_authoritative_failure(self.prepared, failure))?;
            if let Some(partial) = partials.first() {
                return Err(DriverError::IncompleteCheck {
                    file_name: partial.file_name.clone(),
                    start: partial.start,
                    length: partial.length,
                    reason: partial.reason.clone(),
                    additional_partial_checks: partials.len().saturating_sub(1),
                });
            }
            self.activity.borrow_emit_resolver();
            checker.with_emit_resolver(|resolver| {
                tsc_emitter::emit_files_with_activity(
                    resolver,
                    self.host,
                    preflight,
                    selection,
                    &gate,
                    sink,
                    &mut self.activity,
                )
            })
        } else {
            // Early diagnostic gates and empty Programs cannot query a
            // resolver. In particular, blocked emits must not add a request.
            tsc_emitter::emit_files_with_activity(
                &tsc_emitter::UnavailableEmitResolver,
                self.host,
                preflight,
                selection,
                &gate,
                sink,
                &mut self.activity,
            )
        }
        .map_err(DriverError::Emit)?;
        Ok(super::EmitCommandOutcome::new(
            diagnostics.with_emit(
                &preflight_diagnostics,
                emit,
                super::check_work_counters(&checked),
            ),
            self.host.current_directory(),
        ))
    }

    pub fn activity(&self) -> H2ActivityCounters {
        self.activity.counters()
    }
}
