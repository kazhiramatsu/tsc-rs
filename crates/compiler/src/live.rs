//! A Program kept alive between requests (tsgo's `compiler.Program` and the
//! API checker of its pool): the batch session's checker inputs and module
//! provider over an owned [`PreparedProgram`], and a [`LiveChecker`] that
//! checks each file when its diagnostics are first asked for.

use std::collections::BTreeMap;
use std::sync::Arc;

use tsc_checker::live::{LiveChecker, LiveCheckerInputs};
use tsc_checker::state::CheckerState;
use tsc_checker::{BoundDocument, DocumentRegistry};
use tsc_diagnostics::{sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList};
use tsc_emitter::{preflight_emit, validate_emit_request, EmitRouteKind, EmitSelection};
use tsc_program::{PreparedProgram, PreparedProgramMode, WorkerBudget};
use tsc_types::JsStr;

use crate::{
    map_authoritative_failure, program_rows_outside_sources, programmatic_option_diagnostics,
    project_checker_inputs, DriverError, PreparedEmitHost, PreparedModuleProvider,
};

/// One Program and its checker, kept for queries (tsgo `compiler.Program`
/// with its API checker).
pub struct LiveProgram {
    prepared: Arc<PreparedProgram>,
    checker: LiveChecker,
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
        Ok(Self { prepared, checker })
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
        self.checker
            .suggestion_diagnostics(file)
            .map_err(|failure| map_authoritative_failure(&self.prepared, failure))
    }

    /// tsgo `GetGlobalDiagnostics`: the checker's global rows so far, none
    /// for a Program without root names.
    pub fn global_diagnostics(&self) -> DiagnosticList {
        if self.prepared.roots().is_empty() {
            return Vec::new();
        }
        self.checker.global_diagnostics()
    }

    /// Run `query` over the Program's checker; none for a Program without
    /// files, which has no checker.
    pub fn with_checker<T>(&mut self, query: impl FnOnce(&mut CheckerState<'_>) -> T) -> Option<T> {
        self.checker.with_checker(query)
    }
}
