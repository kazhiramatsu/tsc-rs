#![forbid(unsafe_code)]

//! One-shot execution of an owned H0 prepared program.
//!
//! This crate is the dependency boundary between the owned program contract
//! and the parser/binder/checker implementation. A [`ProgramSession`] owns
//! exactly one [`PreparedProgram`], projects its already-final source order
//! into the checker, and is consumed by either the no-emit
//! [`ProgramSession::run`] entry or the distinct emitting
//! [`ProgramSession::emit`] entry.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use tsc_checker::{IncrementalPlanner, IncrementalRequest};
use tsc_incremental::OldState;
use tsc_types::tracing::{Args as TraceArgs, Phase as TracePhase, Tracing};

use tsc_checker::emit::CheckerSession;
pub use tsc_checker::CheckerBudget;
use tsc_checker::{
    check_program_with_authoritative_modules_at_emit_first_with_workers,
    check_program_with_authoritative_modules_at_for_emit_with_checkers,
    check_program_with_authoritative_modules_at_for_emit_with_harness_lib_bundle,
    check_program_with_authoritative_modules_at_for_emit_with_workers,
    check_program_with_authoritative_modules_at_harness_cached,
    check_program_with_authoritative_modules_at_with_checkers_gated,
    check_program_with_authoritative_modules_at_with_workers_gated,
    prepare_authoritative_harness_lib_bundle, AuthoritativeModuleFailure,
    AuthoritativeModuleLookupFailure, AuthoritativeModuleProvider,
    AuthoritativeModuleProviderFactory, AuthoritativeModuleRequest, AuthoritativeModuleResolution,
    AuthoritativeModuleResolutionDiagnostic, AuthoritativeNotFoundModule, AuthoritativePackageId,
    AuthoritativeResolutionDiagnosticModule, AuthoritativeResolutionMode,
    AuthoritativeResolvedModule, AuthoritativeSourceMetadata, AuthoritativeSourceToken,
    AuthoritativeUntypedModule, CheckResult, InputFile, LibraryPrefixCompletion,
    OwnedHarnessLibBundle, ProgramSnapshot, ShardEmission, ShardedEmit, SyntacticDiagnosticsGate,
    UnsupportedAuthoritativeResolution,
};
use tsc_diagnostics::{
    gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticList, JsStr, JsString, MessageChain,
};
use tsc_emitter::{
    begin_emit_files, emit_files, finish_emit_files, preflight_emit,
    print_script_units_with_recording_for_harness, validate_emit_request, EmitDiagnosticGate,
    EmitFilesSession, EmitFilesStart, EmitHost, EmitSource, PrintedText, SourceMapRecordingInputs,
    UnavailableEmitResolver, UnitEmitError,
};
pub use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitContractViolation, EmitFailure,
    EmitFileSystem, EmitIoError, EmitIoOperation, EmitMode, EmitOutcome, EmitOutputPaths,
    EmitOutputPlan, EmitOutputUnit, EmitRoot, EmitSelection, EmitTextMetadata,
    EmitWriteDisposition, EmitWriteMetadata, FsOutputSink, GeneratedUtf16Position,
    MemoryOutputSink, OutputSink, SourceMapObservation, UnsupportedEmitFeature,
};
pub use tsc_program::PreparedProgramMode;
pub use tsc_program::WorkerBudget;
use tsc_program::{
    plan_source_requests, removed_base_url_paths_suggestion, validate_compiler_options,
    validate_paths_option_diagnostics, CompilerOptionValidationLocation, CompilerOptions,
    MissingResolutionError, ModuleExtension, PreparedProgram, PreparedSourceFile, ResolutionKey,
    ResolutionMode, ResolutionOutcome, ResolvedModuleTarget, SourceFileId, SourceRequestPlan,
    UnloadedModuleReason,
};

mod build;
mod cli;
mod declaration_diagnostics;
mod help;
mod incremental;
mod init;
pub mod locale;
mod options;
mod show_config;
mod statistics;
pub mod system;
pub use incremental::{BuildInfoDocument, ProgramFileReport};
pub use tsc_incremental::{SemanticDiagnosticsState, SignatureUpdateKind};
pub mod transpile;
pub mod watch;

pub use cli::{
    execute_command_line, run_cli, CliOutput, CommandLineResult, TYPESCRIPT_VERSION as CLI_VERSION,
};
pub use declaration_diagnostics::DeclarationSession;
pub use tsc_checker::JSDocParsingMode;
pub use tsc_emitter::EmitRouteKind;

/// A one-shot owner for one mode-validated prepared program.
///
/// The consuming [`run`](Self::run) method keeps every parser, binder, and
/// checker borrow inside the call. No retained checker or self-referential
/// session escapes this boundary.
#[derive(Debug)]
pub struct ProgramSession {
    prepared: PreparedProgram,
    /// H2.8c research route (see [`EmitRouteKind`]); ordinary sessions keep
    /// the Program route and every existing option refusal.
    emit_route: EmitRouteKind,
    /// API-supplied per-source facts (see [`SourceApiFacts`]); empty for
    /// ordinary sessions.
    source_api_facts: BTreeMap<SourceFileId, SourceApiFacts>,
    /// Worker budget for the checker's scoped per-file binding; serial by
    /// default (see [`WorkerBudget`]). The CLI passes its own budget.
    worker_budget: WorkerBudget,
    /// Leak the prepared program (source texts, resolution tables) when the
    /// session ends instead of dropping it: a one-shot process exits right
    /// afterwards and only pays for the teardown. Off by default.
    leak_program: bool,
    /// The native compiler runner's collection (see
    /// [`run_for_native_harness`](Self::run_for_native_harness)); `None` for
    /// every other run.
    native_harness: Option<NativeHarnessCollection>,
    /// Checker budget for the no-emit whole-Program check; one checker by
    /// default (see [`CheckerBudget`]). Independent of `worker_budget`:
    /// each additional checker duplicates checker-local state over the one
    /// shared immutable snapshot. The emit and declaration paths stay serial
    /// until their coordinated write finalization exists.
    checker_budget: CheckerBudget,
    /// Whether the command owns options diagnostics the prepared program
    /// does not carry (the config plan's non-fatal option rows, such as
    /// TS5108, which the CLI reports in the options bucket itself). They
    /// close the check of the sources as the Program's own options rows do.
    command_options_diagnostics: bool,
    /// Whether this session is tsgo's `tsc` command, which compiles an
    /// `incremental`/`composite` project as an incremental program and
    /// writes its build info (execute/tsc.go:245); a Program of the harness
    /// or of the API (`compiler.Program`) never writes one.
    command_build_info: bool,
    /// The state of the program's old build info (tsgo
    /// `ReadBuildInfoProgram`), when the command found one: the session then
    /// checks and emits only what that state does not cover.
    old_build_info: Option<Arc<OldState>>,
    /// The command is `tsc -b` (tsgo `CompilerOptions.Build`): every
    /// project writes a build info and a declaration file that differs only
    /// in its map is reported for its timestamp.
    build_mode: bool,
    /// `--listFilesOnly`: the command reports the syntactic and options
    /// diagnostics alone and lists the files (tsgo GetDiagnosticsOfAnyProgram
    /// with ListFilesOnly), without the declaration getter.
    list_files_only: bool,
    /// The command runs under tsgo's test harness (`CommandLineTesting`):
    /// versions and signatures carry their text and the emitted files are
    /// collected for it.
    testing: bool,
    /// The command's `--generateTrace` session (tsgo
    /// `ProgramOptions.Tracing`): the checkers and the emit record into it.
    tracing: Option<Arc<Tracing>>,
    /// A watch run's cycle: the program is incremental in memory and its
    /// state is returned for the next cycle (see `incremental::WatchState`).
    watch: bool,
}

/// Facts TypeScript assigns to a created `SourceFile` before `createProgram`
/// in `transpileWorker` (typescript.js:146090-146104): the caller's file-name
/// spelling (`sourceFile.fileName`), `moduleName`, `renamedDependencies` and
/// the `createSourceFile` `jsDocParsingMode`. They reach only the parsed
/// syntax and the checker-edge input name; Program identity, resolution rows
/// and output paths keep the prepared spelling.
/// tsrs-native: typed carrier for the H2.8c transpile adapter.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SourceApiFacts {
    pub file_name: Option<JsString>,
    pub module_name: Option<JsString>,
    pub renamed_dependencies: Vec<(JsString, JsString)>,
    pub js_doc_parsing_mode: Option<JSDocParsingMode>,
}

pub(crate) struct CliEmitSessionOutcome {
    pub(crate) emit: EmitOutcome,
    pub(crate) config_diagnostics: DiagnosticList,
    pub(crate) syntactic_diagnostics: DiagnosticList,
    pub(crate) options_diagnostics: DiagnosticList,
    pub(crate) global_diagnostics: DiagnosticList,
    pub(crate) semantic_diagnostics: DiagnosticList,
    /// `noEmit && getEmitDeclarations(options)` only: the declaration getter's
    /// rows after a clean semantic pass (emitFilesAndReportErrors,
    /// _tsc.js:129433-129440). Emitting programs leave this empty.
    pub(crate) declaration_diagnostics: DiagnosticList,
    pub(crate) work_counters: NoEmitWorkCounters,
    /// H2.8c evidence: source files whose checkSourceFileWorker body ran.
    pub(crate) checked_source_files: u32,
    /// What `tsc -b` learns from the emit.
    pub(crate) build_emit: incremental::BuildEmitFacts,
    /// The incremental program's files for tsgo's test harness (a testing
    /// session only).
    pub(crate) program_report: Option<Vec<incremental::ProgramFileReport>>,
    /// A watch run's state for its next cycle.
    pub(crate) watch_state: Option<incremental::WatchState>,
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
                let semantic_is_empty = self.semantic_diagnostics.is_empty();
                diagnostics.extend(self.semantic_diagnostics);
                // getDeclarationDiagnostics joins only while allDiagnostics still
                // holds the config-file parsing rows alone (129433-129440); the
                // caller fills this stream under the same gate.
                if semantic_is_empty {
                    diagnostics.extend(self.declaration_diagnostics);
                }
            }
        }
        diagnostics.extend(self.emit.diagnostics().iter().cloned());
        sort_and_dedupe_diagnostics(&mut diagnostics);
        (self.emit, diagnostics, self.work_counters)
    }
}

/// Whether the per-shard emit covers this option set. outFile bundles write
/// at bundle-local points of one resolver's pass and keep the single-checker
/// session; the noEmitOnError declaration gate runs per shard
/// (`sharded_declaration_diagnostics`).
fn sharded_emit_supported(options: &CompilerOptions) -> bool {
    options.out_file.as_ref().is_none_or(|path| path.is_empty())
}

/// The whole-Program declaration diagnostics over the checker sessions of one
/// sharded check (the noEmitOnError gate of a sharded emit, and the --noEmit
/// command's declaration getter): every source selected for emit is
/// transformed against the resolver of the shard that checked it (matched by
/// source name, as the emit pool matches its units), each session's sources
/// in one resolver borrow on the worker budget, so no two workers contend
/// for one session. Sorted and deduplicated like the serial getter's result.
#[allow(clippy::too_many_arguments)]
fn sharded_declaration_diagnostics(
    checked_host: &CheckedEmitHost<'_, '_>,
    emit_host: &PreparedEmitHost<'_>,
    paths: &tsc_emitter::PlanDeclarationPaths,
    sessions: &[CheckerSession<'_>],
    files_by_shard: &[Vec<usize>],
    worker_budget: WorkerBudget,
    shard_label_offset: usize,
    partial: bool,
    selected: Option<&[bool]>,
) -> Result<Vec<Diagnostic>, EmitFailure> {
    let mut owner_by_name = std::collections::HashMap::new();
    for (shard, files) in files_by_shard.iter().enumerate() {
        for &file in files {
            owner_by_name.insert(
                checked_host
                    .snapshot
                    .document(file)
                    .source()
                    .file_name
                    .as_js(),
                shard,
            );
        }
    }
    let mut sources_by_shard: Vec<Vec<SourceFileId>> = vec![Vec::new(); sessions.len()];
    for source in tsc_emitter::get_source_files_to_emit(checked_host, EmitSelection::WholeProgram)?
    {
        // An incremental program computes the declaration diagnostics of
        // its pending files only (tsgo emitFilesIncremental for dts errors).
        if selected.is_some_and(|selected| !selected.get(source.index()).copied().unwrap_or(true)) {
            continue;
        }
        let name = emit_host
            .expected_source_name(source)
            .ok_or(EmitFailure::Contract(
                EmitContractViolation::PlannedSourceMissing(source),
            ))?;
        let shard = match owner_by_name.get(&name.as_js()) {
            Some(&shard) => shard,
            // The sessions given cover a part of the program (a shard's
            // eager share): the other shards' sources are theirs.
            None if partial => continue,
            None => {
                return Err(EmitFailure::Contract(
                    EmitContractViolation::PlannedSourceMissing(source),
                ))
            }
        };
        sources_by_shard[shard].push(source);
    }
    let jobs = sources_by_shard
        .into_iter()
        .enumerate()
        .filter(|(_, sources)| !sources.is_empty())
        .collect::<Vec<_>>();
    let weight = |(_, sources): &(usize, Vec<SourceFileId>)| {
        sources
            .iter()
            .map(|&source| {
                emit_host
                    .prepared
                    .source_file(source)
                    .map_or(0, |source| source.text().len())
            })
            .sum::<usize>()
    };
    let results = worker_budget.map_ordered(jobs, weight, |(shard, sources)| {
        let started = std::time::Instant::now();
        let source_count = sources.len();
        let result = sessions[shard].with_emit_resolver(|resolver| {
            let mut diagnostics = Vec::new();
            for source in sources {
                diagnostics.extend(tsc_emitter::get_declaration_diagnostics(
                    resolver,
                    checked_host,
                    paths,
                    source,
                )?);
            }
            Ok::<_, EmitFailure>(diagnostics)
        });
        if tsc_types::trace::enabled() {
            tsc_types::trace::mark(
                &format!(
                    "shard {}: declaration diagnostics ({source_count} files)",
                    shard + shard_label_offset
                ),
                started,
            );
        }
        result
    });
    let mut diagnostics = Vec::new();
    for result in results {
        diagnostics.extend(result?);
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    Ok(diagnostics)
}

/// Whether the --noEmit command reports nothing beyond the config-file
/// parsing diagnostics: the emitFilesAndReportErrors gate
/// (_tsc.js:129433-129440) that admits `program.getDeclarationDiagnostics()`,
/// decided on the checker's result before `run_inner` assembles its buckets.
/// Every source those buckets draw from must be empty (sorting and
/// deduplication never empty a bucket), so the test is exact; a partial
/// check fails the command instead of reaching the getter.
fn no_emit_report_is_clean(prepared: &PreparedProgram, checked: &CheckResult) -> bool {
    let preparation = prepared.diagnostics();
    checked.syntactic_diagnostics.is_empty()
        && checked.partial_checks.is_empty()
        && checked.global_diagnostics.is_empty()
        && checked
            .program_semantic_diagnostics
            .as_ref()
            .is_some_and(Vec::is_empty)
        && preparation.options().is_empty()
        && programmatic_option_diagnostics(prepared).is_empty()
        && program_rows_outside_sources(prepared).is_empty()
}

/// `program.getDeclarationDiagnostics()` for the --noEmit command, over the
/// checker sessions of the whole-Program check that just completed: each
/// source is transformed for diagnostics by the shard that checked it, on
/// the worker budget, and nothing is emitted. The declaration request is
/// validated here, once the command's gate admits the getter, exactly as
/// the explicit getter session validates it.
#[allow(clippy::too_many_arguments)]
fn no_emit_declaration_diagnostics(
    prepared: &PreparedProgram,
    emit_route: EmitRouteKind,
    source_api_facts: &BTreeMap<SourceFileId, SourceApiFacts>,
    snapshot: &ProgramSnapshot,
    sessions: &[CheckerSession<'_>],
    files_by_shard: &[Vec<usize>],
    worker_budget: WorkerBudget,
    shard_label_offset: usize,
    partial: bool,
    selected: Option<&[bool]>,
) -> Result<DiagnosticList, DriverError> {
    let emit_host = PreparedEmitHost::new_for_route(prepared, emit_route, source_api_facts)?;
    tsc_emitter::validate_declaration_diagnostics_request(&emit_host).map_err(DriverError::Emit)?;
    let checked_host = CheckedEmitHost {
        prepared: &emit_host,
        snapshot,
        prepared_sources: None,
    };
    let paths = tsc_emitter::PlanDeclarationPaths::for_declaration_diagnostics(&checked_host)
        .map_err(DriverError::Emit)?;
    sharded_declaration_diagnostics(
        &checked_host,
        &emit_host,
        &paths,
        sessions,
        files_by_shard,
        worker_budget,
        shard_label_offset,
        partial,
        selected,
    )
    .map_err(DriverError::Emit)
}

/// tsc-port: getEmitDeclarations @6.0.3
/// tsc-hash: f385c1e1ef2b314fab891cf63605d192e845fd49775c89849d96a7fa96389541
/// tsc-span: _tsc.js:18151-18156
pub(crate) fn get_emit_declarations(options: &CompilerOptions) -> bool {
    options.declaration == Some(true) || options.composite == Some(true)
}

/// One ordinary whole-Program emit with its command reporting observation.
/// File writes are delivered to the caller's OutputSink; these status entries
/// are the same TSFILE strings consumed by the command-line driver.
pub struct EmitCommandOutcome {
    emit: EmitOutcome,
    diagnostics: DiagnosticList,
    status_writes: Vec<JsString>,
    exit_code: i32,
    checked_source_files: u32,
}

impl EmitCommandOutcome {
    fn new(outcome: CliEmitSessionOutcome, current_directory: JsStr<'_>) -> Self {
        Self::with_options_diagnostics(outcome, current_directory, &[])
    }

    fn with_options_diagnostics(
        outcome: CliEmitSessionOutcome,
        current_directory: JsStr<'_>,
        additional_options_diagnostics: &[Diagnostic],
    ) -> Self {
        let checked_source_files = outcome.checked_source_files;
        let (emit, diagnostics, _) = outcome.into_reported(additional_options_diagnostics);
        let (status_writes, exit_code) =
            cli::emit_command_status(current_directory, &emit, &diagnostics, true);
        Self {
            emit,
            diagnostics,
            status_writes,
            exit_code,
            checked_source_files,
        }
    }

    /// H2.8c evidence: how many source files ran the checker's
    /// checkSourceFileWorker body (0 under an admitted `noCheck`).
    pub fn checked_source_files(&self) -> u32 {
        self.checked_source_files
    }

    pub fn emit(&self) -> &EmitOutcome {
        &self.emit
    }
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
    pub fn status_writes(&self) -> &[JsString] {
        &self.status_writes
    }
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

/// Separate Program diagnostic streams in the public getter order.
/// Config parsing remains independent from compiler option diagnostics.
pub struct ProgramDiagnostics {
    config: DiagnosticList,
    syntactic: DiagnosticList,
    options: DiagnosticList,
    global: DiagnosticList,
    semantic: DiagnosticList,
}

impl ProgramDiagnostics {
    pub fn config(&self) -> &[Diagnostic] {
        &self.config
    }
    pub fn options(&self) -> &[Diagnostic] {
        &self.options
    }
    pub fn syntactic(&self) -> &[Diagnostic] {
        &self.syntactic
    }
    pub fn global(&self) -> &[Diagnostic] {
        &self.global
    }
    pub fn semantic(&self) -> &[Diagnostic] {
        &self.semantic
    }
}

impl ProgramDiagnostics {
    /// The semantic rows an incremental program takes from its old build
    /// info (the files its check did not cover) join the fresh ones.
    fn extend_semantic(&mut self, rows: Vec<Diagnostic>) {
        if rows.is_empty() {
            return;
        }
        self.semantic.extend(rows);
        sort_and_dedupe_diagnostics(&mut self.semantic);
    }

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
            declaration_diagnostics: DiagnosticList::new(),
            work_counters,
            checked_source_files: 0,
            build_emit: incremental::BuildEmitFacts::default(),
            program_report: None,
            watch_state: None,
        }
    }
}

struct PreparedModuleProvider<'a> {
    prepared: &'a PreparedProgram,
    request_plans: std::sync::Mutex<BTreeMap<SourceFileId, SourceRequestPlan>>,
    tracing: Option<Arc<Tracing>>,
}

/// Constructs one [`PreparedModuleProvider`] per checker state over the
/// shared immutable [`PreparedProgram`]; the provider's request-plan cache is
/// therefore checker-local and the public provider trait needs no `Sync`.
/// tsrs-native: the compiler's per-checker provider seam for sharded checking.
struct PreparedProviderFactory<'a> {
    prepared: &'a PreparedProgram,
    tracing: Option<Arc<Tracing>>,
}

impl AuthoritativeModuleProviderFactory for PreparedProviderFactory<'_> {
    fn provider(&self) -> Box<dyn AuthoritativeModuleProvider + '_> {
        Box::new(PreparedModuleProvider {
            prepared: self.prepared,
            request_plans: std::sync::Mutex::new(BTreeMap::new()),
            tracing: self.tracing.clone(),
        })
    }

    fn tracing(&self) -> Option<Arc<Tracing>> {
        self.tracing.clone()
    }
}

struct PreparedEmitHost<'program> {
    prepared: &'program PreparedProgram,
    /// `tsc -b` lists the emitted files whatever `listEmittedFiles` says.
    collect_emitted_files: bool,
    source_files: Vec<SourceFileId>,
    common_source_directory: JsString,
    symlinks: tsc_program::SymlinkFacts,
    emit_route: EmitRouteKind,
    /// Caller file-name spellings (SourceApiFacts::file_name): the parsed
    /// syntax carries this name, so the checked host matches documents by it.
    display_names: BTreeMap<SourceFileId, JsString>,
    /// Canonical output path → source, built on the first module-specifier
    /// `fileExists`/`readFile` probe (every candidate path of every specifier
    /// asks; scanning the sources per probe made a 3,000-file declaration
    /// emit quadratic).
    canonical_index: std::sync::OnceLock<std::collections::HashMap<Box<[u8]>, SourceFileId>>,
    /// The `--generateTrace` session the units' emit is recorded in.
    tracing: Option<Arc<Tracing>>,
}

impl<'program> PreparedEmitHost<'program> {
    /// The parsed-syntax file name the checked host matches a Program source
    /// by (see `CheckedEmitHost::source_file`): the caller's spelling when
    /// one was supplied, otherwise the prepared display path.
    fn expected_source_name(&self, id: SourceFileId) -> Option<JsString> {
        let source = self.prepared.source_file(id)?;
        Some(
            self.display_names
                .get(&id)
                .cloned()
                .unwrap_or_else(|| source.path().display().to_owned()),
        )
    }

    fn new_for_route(
        prepared: &'program PreparedProgram,
        emit_route: EmitRouteKind,
        source_api_facts: &BTreeMap<SourceFileId, SourceApiFacts>,
    ) -> Result<Self, DriverError> {
        let display_names = source_api_facts
            .iter()
            .filter_map(|(id, facts)| Some((*id, facts.file_name.clone()?)))
            .collect();
        let source_files = prepared
            .source_files()
            .iter()
            .map(|source| {
                prepared
                    .source_id(source.path().canonical())
                    .ok_or_else(|| DriverError::MissingPreparedSourceIdentity {
                        path: source.path().display().to_owned(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let symlinks = tsc_program::discover_symlink_facts(prepared);
        let mut host = Self {
            prepared,
            collect_emitted_files: false,
            source_files,
            common_source_directory: prepared.current_directory().display().to_owned(),
            symlinks,
            canonical_index: std::sync::OnceLock::new(),
            emit_route,
            display_names,
            tracing: None,
        };
        // getCommonSourceDirectory2 first applies ordinary sourceFileMayBeEmitted
        // (_tsc.js:123142-123157), including noEmitForJsFiles, before comparing
        // source directories. It does not apply getSourceFilesToEmit's
        // outFile external-module filter, which needs checked source syntax.
        let emitted_files = host
            .source_files
            .iter()
            .copied()
            .filter_map(|id| match host.source_file(id) {
                Some(source) => {
                    tsc_emitter::source_file_may_be_emitted(source, &host).then_some(Ok(id))
                }
                None => Some(Err(tsc_emitter::EmitFailure::Contract(
                    tsc_emitter::EmitContractViolation::PlannedSourceMissing(id),
                ))),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(DriverError::Emit)?;
        host.common_source_directory = common_emit_source_directory(prepared, &emitted_files);
        Ok(host)
    }
}

impl PreparedEmitHost<'_> {
    /// Whether the emit lists its files regardless of `listEmittedFiles`.
    fn with_collected_emitted_files(mut self, collect: bool) -> Self {
        self.collect_emitted_files = collect;
        self
    }

    fn with_tracing(mut self, tracing: Option<Arc<Tracing>>) -> Self {
        self.tracing = tracing;
        self
    }
}

impl EmitHost for PreparedEmitHost<'_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.prepared.compiler_options()
    }

    fn tracing(&self) -> Option<&Arc<Tracing>> {
        self.tracing.as_ref()
    }

    fn collects_emitted_files(&self) -> bool {
        self.collect_emitted_files || self.compiler_options().list_emitted_files == Some(true)
    }

    fn emit_route(&self) -> EmitRouteKind {
        self.emit_route
    }

    fn symlinked_files(&self) -> Vec<(JsString, JsString)> {
        self.symlinks.files.clone()
    }

    fn symlinked_directories(&self) -> Vec<(JsString, JsString)> {
        self.symlinks.directories.clone()
    }

    fn project_reference_source_of_output(&self, id: SourceFileId) -> Option<JsString> {
        self.prepared
            .source_file(id)?
            .project_reference_source_paths()
            .last()
            .map(|path| path.display().to_owned())
    }

    fn project_reference_output_of_source(&self, canonical_source: JsStr<'_>) -> Option<JsString> {
        let source = tsc_program::CanonicalPath::from_js_normalized(canonical_source).ok()?;
        self.prepared
            .program_options()
            .project_references()?
            .output_for_source(&source)?
            .output_dts()
            .map(JsStr::to_owned)
    }

    fn project_reference_redirect(&self, canonical_source: JsStr<'_>) -> Option<SourceFileId> {
        self.source_files.iter().copied().find(|&id| {
            self.prepared.source_file(id).is_some_and(|source| {
                source.project_reference_source_paths().iter().any(|path| {
                    self.canonical_output_path(path.display()).as_js() == canonical_source
                })
            })
        })
    }

    fn redirect_targets(&self, canonical_path: JsStr<'_>) -> Vec<JsString> {
        // The copies of a package the loader redirected to this source by
        // their exact package identity (`PreparedSourceFile::
        // package_redirect_paths`): tsgo's redirectTargetsMap.
        self.source_file_by_canonical_path(canonical_path)
            .and_then(|id| self.prepared.source_file(id))
            .map(|source| {
                source
                    .package_redirect_paths()
                    .iter()
                    .map(|path| path.display().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn current_directory(&self) -> JsStr<'_> {
        self.prepared.current_directory().display()
    }

    fn common_source_directory(&self) -> JsStr<'_> {
        self.common_source_directory.as_js()
    }

    fn config_file_path(&self) -> Option<JsStr<'_>> {
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

    fn source_file_by_canonical_path(&self, canonical: JsStr<'_>) -> Option<SourceFileId> {
        let index = self.canonical_index.get_or_init(|| {
            self.source_files
                .iter()
                .filter_map(|&id| {
                    let source = self.prepared.source_file(id)?;
                    let canonical = self.canonical_output_path(source.path().display());
                    Some((Box::<[u8]>::from(canonical.as_bytes()), id))
                })
                .collect()
        });
        index.get(canonical.as_bytes()).copied()
    }

    fn source_file(&self, id: SourceFileId) -> Option<EmitSource<'_>> {
        let source = self.prepared.source_file(id)?;
        Some(
            EmitSource::new(
                id,
                source.path().display(),
                source.path().canonical().as_js(),
                source.may_be_emitted(),
                source.implied_node_format_for_emit(),
                None,
            )
            .with_may_emit_forced_declaration(source.may_emit_forced_declaration())
            .with_is_external_module(source.is_external_module()),
        )
    }
}

/// A source's emit copy prepared beside the checkers (the `ShardedEmit`
/// prelude): pending until prepared, taken once by the source's JavaScript
/// emit. A slot taken while pending tells the prelude to drop its result.
enum PreludeSlot {
    Pending,
    Ready(Box<tsc_emitter::PreparedEmitSource>),
    Taken,
}

type PreludeSlots = [std::sync::Mutex<PreludeSlot>];

struct CheckedEmitHost<'host, 'snapshot> {
    prepared: &'host PreparedEmitHost<'host>,
    snapshot: &'snapshot ProgramSnapshot,
    /// Prepared emit copies by source index, when a prelude ran.
    prepared_sources: Option<&'host PreludeSlots>,
}

impl EmitHost for CheckedEmitHost<'_, '_> {
    fn compiler_options(&self) -> &CompilerOptions {
        self.prepared.compiler_options()
    }

    fn tracing(&self) -> Option<&Arc<Tracing>> {
        self.prepared.tracing()
    }

    fn collects_emitted_files(&self) -> bool {
        self.prepared.collects_emitted_files()
    }

    fn take_prepared_source(&self, id: SourceFileId) -> Option<tsc_emitter::PreparedEmitSource> {
        let slot = self.prepared_sources?.get(id.index())?;
        let mut slot = slot.lock().ok()?;
        match std::mem::replace(&mut *slot, PreludeSlot::Taken) {
            PreludeSlot::Ready(prepared) => Some(*prepared),
            PreludeSlot::Pending | PreludeSlot::Taken => None,
        }
    }

    fn source_file_by_canonical_path(&self, canonical: JsStr<'_>) -> Option<SourceFileId> {
        self.prepared.source_file_by_canonical_path(canonical)
    }

    fn emit_route(&self) -> EmitRouteKind {
        self.prepared.emit_route()
    }

    fn symlinked_files(&self) -> Vec<(JsString, JsString)> {
        self.prepared.symlinked_files()
    }

    fn symlinked_directories(&self) -> Vec<(JsString, JsString)> {
        self.prepared.symlinked_directories()
    }

    fn redirect_targets(&self, canonical_path: JsStr<'_>) -> Vec<JsString> {
        self.prepared.redirect_targets(canonical_path)
    }

    fn project_reference_redirect(&self, canonical_source: JsStr<'_>) -> Option<SourceFileId> {
        self.prepared.project_reference_redirect(canonical_source)
    }

    fn project_reference_source_of_output(&self, id: SourceFileId) -> Option<JsString> {
        self.prepared.project_reference_source_of_output(id)
    }

    fn project_reference_output_of_source(&self, canonical_source: JsStr<'_>) -> Option<JsString> {
        self.prepared
            .project_reference_output_of_source(canonical_source)
    }

    fn current_directory(&self) -> JsStr<'_> {
        self.prepared.current_directory()
    }

    fn common_source_directory(&self) -> JsStr<'_> {
        self.prepared.common_source_directory()
    }

    fn config_file_path(&self) -> Option<JsStr<'_>> {
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
        let expected_name = self
            .prepared
            .display_names
            .get(&id)
            .map_or_else(|| source.path().display(), JsString::as_js);
        let syntax = self
            .snapshot
            .documents()
            .get(id.index())
            .filter(|document| document.source().file_name.as_js() == expected_name)
            .or_else(|| {
                self.snapshot
                    .documents()
                    .iter()
                    .find(|document| document.source().file_name.as_js() == expected_name)
            })
            .map(|document| document.source());
        Some(
            EmitSource::new(
                id,
                source.path().display(),
                source.path().canonical().as_js(),
                source.may_be_emitted(),
                source.implied_node_format_for_emit(),
                syntax,
            )
            .with_may_emit_forced_declaration(source.may_emit_forced_declaration())
            .with_is_external_module(source.is_external_module()),
        )
    }
}

fn common_emit_source_directory(
    prepared: &PreparedProgram,
    source_files: &[SourceFileId],
) -> JsString {
    let sources = source_files
        .iter()
        .filter_map(|id| prepared.source_file(*id))
        .map(|source| source.path().display())
        .collect::<Vec<_>>();
    tsc_program::common_source_directory(
        prepared.compiler_options(),
        prepared
            .program_options()
            .config_file_path()
            .map(|path| path.display()),
        &sources,
        prepared.current_directory().display(),
        prepared.path_context().use_case_sensitive_file_names(),
    )
}

impl PreparedModuleProvider<'_> {
    fn source_request_plan(
        &self,
        source_file: SourceFileId,
        source: &PreparedSourceFile,
    ) -> Result<SourceRequestPlan, AuthoritativeModuleLookupFailure> {
        if let Some(plan) = self
            .request_plans
            .lock()
            .expect("request plan cache")
            .get(&source_file)
        {
            return Ok(plan.clone());
        }
        let plan =
            plan_source_requests(source, self.prepared.compiler_options()).map_err(|_| {
                AuthoritativeModuleLookupFailure::Unsupported(
                    UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
                )
            })?;
        self.request_plans
            .lock()
            .expect("request plan cache")
            .insert(source_file, plan.clone());
        Ok(plan)
    }

    fn module_request_loads_source(
        &self,
        source_file: SourceFileId,
        source: &PreparedSourceFile,
        key: &ResolutionKey,
    ) -> Result<bool, AuthoritativeModuleLookupFailure> {
        if let Some(plan) = self
            .request_plans
            .lock()
            .expect("request plan cache")
            .get(&source_file)
        {
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
    fn common_source_directory(&self) -> Option<JsString> {
        Some(self.prepared.common_source_directory())
    }

    fn tracing(&self) -> Option<Arc<Tracing>> {
        self.tracing.clone()
    }

    fn path_context(&self) -> Option<&tsc_program::PathContext> {
        Some(self.prepared.path_context())
    }

    fn is_project_reference_source(&self, file_name: JsStr<'_>) -> bool {
        self.prepared
            .project_reference_output_of_source(file_name)
            .is_some()
    }

    fn program_options_for_module_specifiers(&self) -> Option<&tsc_program::ProgramOptions> {
        Some(self.prepared.program_options())
    }

    fn include_processor_diagnostics(&self) -> Vec<(AuthoritativeSourceToken, Diagnostic)> {
        include_processor_rows(self.prepared)
            .into_iter()
            .map(|(source, diagnostic)| (AuthoritativeSourceToken(source.raw()), diagnostic))
            .collect()
    }

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
                            project_reference_output: None,
                        },
                    ));
                }
                return Err(AuthoritativeModuleLookupFailure::Missing);
            }
        };
        // A fileless package-root diagnostic does not invalidate the resolved
        // module. Program getters report these owned facts independently.
        // Unqualified host diagnostic records retain their typed boundary.
        if resolution.diagnostics().iter().any(|diagnostic| {
            !matches!(diagnostic.code(), 2209 | 2210)
                || diagnostic.file_name.is_some()
                || diagnostic.start.is_some()
                || diagnostic.length.is_some()
                || diagnostic.category() != tsc_diagnostics::DiagnosticCategory::Error
                || !diagnostic.message.next.is_empty()
                || diagnostic.related_information_present
                || !diagnostic.related.is_empty()
        }) {
            return Err(AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::ResolutionDiagnostics,
            ));
        }
        let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
            let alternate_result = resolution
                .alternate_result()
                .map(|path| path.display().to_owned());
            return Ok(AuthoritativeModuleResolution::NotFound(
                AuthoritativeNotFoundModule {
                    alternate_result,
                    project_reference_output: None,
                },
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
            let loads_source = self.module_request_loads_source(source_file, source, &key)?;
            if !module.extension().is_javascript()
                && !jsx_syntax_extension
                && !arbitrary_declaration
                && !loads_source
                && matches!(reason, UnloadedModuleReason::ResolutionOnly)
            {
                // The name of a module augmentation resolved to a TypeScript
                // or JSON file that no import loads (DefinitelyTyped's
                // `declare module "htmlbars-inline-precompile"` in
                // `ember/index.d.ts`). resolveExternalModule finds the
                // resolution but no source file for it and takes the path of
                // an unresolved name: an augmentation in an ambient context
                // stays unmerged without a diagnostic, one in a `.ts` module
                // reports TS2664 (checker.go resolveExternalModule;
                // mergeModuleAugmentation passes no error for an ambient
                // augmentation).
                let alternate_result = resolution
                    .alternate_result()
                    .map(|path| path.display().to_owned());
                return Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule {
                        alternate_result,
                        project_reference_output: None,
                    },
                ));
            }
            if matches!(reason, UnloadedModuleReason::ProjectReferenceOutputNotBuilt) {
                // The source of a referenced project is loaded as its output
                // declaration file (tsgo getParseFileRedirect); without the
                // output nothing was loaded, and the checker reports TS6305.
                let project_reference_output = self
                    .prepared
                    .program_options()
                    .project_references()
                    .and_then(|references| references.output_for_source(resolved_file.canonical()))
                    .and_then(|output| {
                        output
                            .output_dts()
                            .map(|dts| (dts.to_owned(), output.source().to_owned()))
                    });
                return Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule {
                        alternate_result: None,
                        project_reference_output,
                    },
                ));
            }
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
                    .map(|path| path.display().to_owned());
                return Ok(AuthoritativeModuleResolution::NotFound(
                    AuthoritativeNotFoundModule {
                        alternate_result,
                        project_reference_output: None,
                    },
                ));
            }
            let node_modules_depth_applies = module.is_external_library_import()
                && (module.original_path().is_none()
                    || path_contains_node_modules(resolved_file.canonical().as_js()));
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
            let resolved_file_name = resolved_file.display().to_owned();
            let alternate_result = resolution
                .alternate_result()
                .map(|path| path.display().to_owned());
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
                resolved_file_name: resolved_file.display().to_owned(),
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
                        peer_dependencies: package_id.peer_dependencies().map(JsStr::to_owned),
                    }),
                alternate_result: resolution
                    .alternate_result()
                    .map(|path| path.display().to_owned()),
                types_package_exists: resolution.types_package_exists(),
                package_bundles_types: resolution.package_bundles_types(),
            },
        ))
    }
}

fn path_contains_node_modules(path: JsStr<'_>) -> bool {
    path.split_ascii(b'/')
        .any(|component| component == "node_modules")
}

fn is_declaration_file_name(file_name: JsStr<'_>) -> bool {
    if [".d.ts", ".d.cts", ".d.mts"]
        .iter()
        .any(|suffix| file_name.ends_with(suffix))
    {
        return true;
    }
    let base = file_name.split_ascii(b'/').next_back().unwrap_or(file_name);
    let base = base.split_ascii(b'\\').next_back().unwrap_or(base);
    base.ends_with(".ts") && base.contains(".d.")
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
        Self {
            prepared,
            emit_route: EmitRouteKind::Program,
            source_api_facts: BTreeMap::new(),
            worker_budget: WorkerBudget::serial(),
            leak_program: false,
            native_harness: None,
            command_build_info: false,
            checker_budget: CheckerBudget::serial(),
            command_options_diagnostics: false,
            old_build_info: None,
            build_mode: false,
            list_files_only: false,
            testing: false,
            tracing: None,
            watch: false,
        }
    }

    /// The session is a watch run's cycle (see the `watch` field).
    pub(crate) fn with_watch(mut self, watch: bool) -> Self {
        self.watch = watch;
        self
    }

    /// Records the session's checks and emit in a `--generateTrace`
    /// session. Its checker states are then dropped, not leaked, as each
    /// writes its types file when it ends.
    pub fn with_tracing(mut self, tracing: Option<Arc<Tracing>>) -> Self {
        if tracing.is_some() {
            self.checker_budget = self.checker_budget.with_leaked_states(false);
        }
        self.tracing = tracing;
        self
    }

    /// Whether the prepared program is leaked at the end of the session (see
    /// the `leak_program` field); only a process that exits right afterwards
    /// should set this.
    pub fn with_leaked_program(mut self, leak: bool) -> Self {
        self.leak_program = leak;
        self
    }

    /// Tell the session that the command reports options diagnostics of its
    /// own (see the `command_options_diagnostics` field): tsc's
    /// emitFilesAndReportErrors then asks for no file's semantic
    /// diagnostics, and the session checks no source.
    pub fn with_command_options_diagnostics(mut self, present: bool) -> Self {
        self.command_options_diagnostics = present;
        self
    }

    /// Attach API-supplied facts to one prepared source (transpile adapter).
    /// tsrs-native: see [`SourceApiFacts`].
    pub fn with_source_api_facts(mut self, source: SourceFileId, facts: SourceApiFacts) -> Self {
        self.source_api_facts.insert(source, facts);
        self
    }

    /// Select the worker budget for the session's scoped per-file binding.
    /// Every budget publishes identical identities, diagnostics and output;
    /// the serial default is the reproducible control.
    /// tsrs-native: see [`WorkerBudget`].
    pub fn with_worker_budget(mut self, worker_budget: WorkerBudget) -> Self {
        self.worker_budget = worker_budget;
        self
    }

    pub fn worker_budget(&self) -> WorkerBudget {
        self.worker_budget
    }

    /// Select the checker budget for the no-emit whole-Program check.
    /// The serial default is the exact reference; a sharded budget must
    /// publish identical diagnostics, which the W controls observe.
    /// tsrs-native: see [`CheckerBudget`].
    pub fn with_checker_budget(mut self, checker_budget: CheckerBudget) -> Self {
        self.checker_budget = checker_budget;
        self
    }

    pub fn checker_budget(&self) -> CheckerBudget {
        self.checker_budget
    }

    /// Select an H2.8c research route. Only the emit option admission
    /// changes (`noCheck`, and the transpile-forced isolated-module
    /// options); the checker, planner and printer are the production ones.
    /// tsrs-native: typed route plan for the no-check prototypes.
    pub fn with_emit_route(mut self, emit_route: EmitRouteKind) -> Self {
        self.emit_route = emit_route;
        self
    }

    pub fn emit_route(&self) -> EmitRouteKind {
        self.emit_route
    }

    /// Consume the prepared program and execute the no-emit diagnostic pass.
    ///
    /// Module lookups use only [`PreparedProgram::resolutions`]. A missing
    /// exact `(source, specifier, mode)` row is an infrastructure error; the
    /// checker never falls back to its legacy heuristic resolver.
    pub fn run(self) -> Result<NoEmitOutcome, DriverError> {
        self.require_mode(PreparedProgramMode::NoEmit)?;
        self.run_no_emit_pass(false, LibraryPrefixCompletion::Complete, false)
    }

    /// The --noEmit command's report: the no-emit diagnostic pass of
    /// [`run`](Self::run) plus, when the options request declarations
    /// (getEmitDeclarations) and nothing beyond the config-file parsing
    /// diagnostics is reported, `program.getDeclarationDiagnostics()` as
    /// emitFilesAndReportErrors adds it (_tsc.js:129433-129440). The getter
    /// runs over the same checker sessions: each source is transformed for
    /// diagnostics by the shard that checked it, on the worker budget, and
    /// nothing is emitted. `run` itself keeps H0's no-emitter contract.
    pub fn run_no_emit_command(self) -> Result<NoEmitOutcome, DriverError> {
        self.require_mode(PreparedProgramMode::NoEmit)?;
        self.with_command_build_info().run_no_emit_pass(
            false,
            LibraryPrefixCompletion::Complete,
            true,
        )
    }

    /// This session is the `tsc` command: an `incremental`/`composite`
    /// project compiles as an incremental program whose --noEmit report or
    /// emit writes the build info (see `command_build_info`).
    pub(crate) fn with_command_build_info(mut self) -> Self {
        self.command_build_info = true;
        self
    }

    /// The state of the program's old build info (tsgo
    /// `ReadBuildInfoProgram`), read by the command before the session.
    /// The session runs for `tsc -b` (tsgo `CompilerOptions.Build`).
    pub fn with_build_mode(mut self, build: bool) -> Self {
        self.build_mode = build;
        self
    }

    /// The session runs for `--listFilesOnly`: the syntactic and options
    /// diagnostics alone, no declaration getter.
    pub fn with_list_files_only(mut self, list_files_only: bool) -> Self {
        self.list_files_only = list_files_only;
        self
    }

    pub fn with_old_build_info(mut self, old: Option<OldState>) -> Self {
        self.old_build_info = old.map(Arc::new);
        self
    }

    /// The command runs under tsgo's test harness (see the `testing`
    /// field).
    pub fn with_testing(mut self, testing: bool) -> Self {
        self.testing = testing;
        self
    }

    /// Consume this Program for a declaration-only diagnostic getter.
    /// Compiler options, including noEmit and noCheck, remain unchanged.
    pub fn get_declaration_diagnostics(
        self,
        selection: EmitSelection,
    ) -> Result<DiagnosticList, DriverError> {
        self.with_declarations(|diagnostics| diagnostics.get_declaration_diagnostics(selection))
    }

    /// Force declaration-only output, preserving options and the Program's
    /// ordinary path blocking while skipping handleNoEmitOptions.
    pub fn emit_forced_declarations(
        self,
        selection: EmitSelection,
        sink: &mut dyn OutputSink,
    ) -> Result<EmitOutcome, DriverError> {
        self.with_declarations(|declarations| {
            declarations.emit_forced_declarations(selection, sink)
        })
    }

    /// Lend Program diagnostics, declaration getters and emits over one checker.
    /// Source checking, resolver borrows and the per-file cache remain inside
    /// this call. The explicit getter accepts either prepared mode; run()
    /// retains its separate no-emitter contract.
    /// tsrs-native: scoped Rust lifetime adaptation of Program declaration operations.
    pub fn with_declarations<R>(
        self,
        operation: impl FnOnce(&mut DeclarationSession<'_, '_>) -> Result<R, DriverError>,
    ) -> Result<R, DriverError> {
        self.with_declarations_initialized(false, |declarations, _| operation(declarations))
    }

    /// Observe a forced emit after the existing whole-Program semantic pass,
    /// retaining the same checker for the callback. No second checker or
    /// declaration getter is substituted for semantic diagnostics.
    #[doc(hidden)]
    pub fn with_declarations_after_semantic_for_harness<R>(
        self,
        operation: impl FnOnce(&mut DeclarationSession<'_, '_>, &[Diagnostic]) -> Result<R, DriverError>,
    ) -> Result<R, DriverError> {
        self.with_declarations_initialized(true, operation)
    }

    fn with_declarations_initialized<R>(
        self,
        check_semantics: bool,
        operation: impl FnOnce(&mut DeclarationSession<'_, '_>, &[Diagnostic]) -> Result<R, DriverError>,
    ) -> Result<R, DriverError> {
        let ProgramSession {
            prepared,
            emit_route,
            source_api_facts,
            worker_budget,
            checker_budget: _,
            leak_program: _,
            native_harness: _,
            command_build_info: _,
            command_options_diagnostics: _,
            old_build_info: _,
            build_mode: _,
            list_files_only: _,
            testing: _,
            tracing: _,
            watch: _,
        } = self;
        let emit_host = PreparedEmitHost::new_for_route(&prepared, emit_route, &source_api_facts)?;
        tsc_emitter::validate_declaration_diagnostics_request(&emit_host)
            .map_err(DriverError::Emit)?;
        let inputs = project_checker_inputs(&prepared, &source_api_facts)?;
        let provider = PreparedModuleProvider {
            prepared: &prepared,
            request_plans: std::sync::Mutex::new(BTreeMap::new()),
            tracing: None,
        };
        let mut pending_operation = Some(operation);
        let mut diagnostic_result = None;
        let mut checked_operation =
            |snapshot: &ProgramSnapshot, checker: &CheckerSession<'_>, checked: &CheckResult| {
                if diagnostic_result.is_some() {
                    return;
                }
                let checked_host = CheckedEmitHost {
                    prepared: &emit_host,
                    snapshot,
                    prepared_sources: None,
                };
                diagnostic_result = Some((|| {
                    let mut diagnostics =
                        DeclarationSession::new(&prepared, &checked_host, Some(checker), checked)?;
                    pending_operation
                        .take()
                        .expect("checked callback runs once")(
                        &mut diagnostics,
                        checked
                            .program_semantic_diagnostics
                            .as_deref()
                            .expect("authoritative sessions publish whole-Program semantics"),
                    )
                })());
            };
        let checked = if check_semantics {
            check_program_with_authoritative_modules_at_for_emit_with_workers(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                worker_budget,
                IncrementalRequest::NONE,
                &mut checked_operation,
            )
        } else {
            tsc_checker::with_authoritative_modules_at_for_declarations(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                IncrementalRequest::NONE,
                &mut checked_operation,
            )
        }
        .map_err(|failure| map_authoritative_failure(&prepared, failure))?;
        if let Some(result) = diagnostic_result {
            return result;
        }
        let mut diagnostics = DeclarationSession::new(&prepared, &emit_host, None, &checked)?;
        pending_operation
            .take()
            .expect("empty Program did not run callback")(&mut diagnostics, &[])
    }

    /// Prepare a bounded, exact-match library prefix for harness repetitions.
    /// The returned value is owned by the caller and is never inserted into a
    /// process-lifetime cache.
    #[doc(hidden)]
    pub fn prepare_harness_lib_bundle(&self) -> Result<Option<OwnedHarnessLibBundle>, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let inputs = project_checker_inputs(&self.prepared, &self.source_api_facts)?;
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

    /// Retain ordinary Program emission and its production command reporting
    /// without initializing a declaration-diagnostic session.
    #[doc(hidden)]
    pub fn emit_command_for_harness(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<EmitCommandOutcome, DriverError> {
        self.emit_command_for_harness_with_options_diagnostics(sink, &[])
    }

    /// Retain the config plan's separately owned option diagnostics for a
    /// no-emit command, at the same reporting boundary used by the CLI.
    /// Emitting config programs already own these diagnostics themselves.
    #[doc(hidden)]
    pub fn emit_command_for_harness_with_options_diagnostics(
        self,
        sink: &mut dyn OutputSink,
        additional_options_diagnostics: &[Diagnostic],
    ) -> Result<EmitCommandOutcome, DriverError> {
        if !additional_options_diagnostics.is_empty() {
            self.require_mode(PreparedProgramMode::NoEmit)?;
        }
        let current_directory = self.prepared.current_directory().display().to_owned();
        if self.prepared.mode() == PreparedProgramMode::NoEmit {
            let options = self.prepared.compiler_options().clone();
            // tsc emitFilesAndReportErrors (_tsc.js:129433-129440): with noEmit
            // and getEmitDeclarations(options), program.getDeclarationDiagnostics()
            // joins the report after the semantic pass, only while nothing
            // beyond the config-file parsing diagnostics was reported; the
            // command session runs that getter over its own checker sessions.
            let outcome = self.run_no_emit_command()?;
            // Preserve H0's separate typed execution: only the upstream
            // whole-program empty build-info return value is added for the
            // command observer; no sink is invoked.
            let emit =
                EmitOutcome::no_emit_without_build_info(&options).map_err(DriverError::Emit)?;
            // The option rows supplied here close the declaration gate too;
            // the session could not see them.
            let declaration_diagnostics = if additional_options_diagnostics.is_empty() {
                outcome.declaration_diagnostics
            } else {
                DiagnosticList::new()
            };
            let reported = CliEmitSessionOutcome {
                emit,
                config_diagnostics: outcome.config_diagnostics,
                syntactic_diagnostics: outcome.syntactic_diagnostics,
                options_diagnostics: outcome.options_diagnostics,
                global_diagnostics: outcome.global_diagnostics,
                semantic_diagnostics: outcome.semantic_diagnostics,
                declaration_diagnostics,
                work_counters: outcome.work_counters,
                checked_source_files: 0,
                build_emit: incremental::BuildEmitFacts::default(),
                program_report: None,
                watch_state: None,
            };
            return Ok(EmitCommandOutcome::with_options_diagnostics(
                reported,
                current_directory.as_js(),
                additional_options_diagnostics,
            ));
        }
        self.emit_for_cli(sink)
            .map(|outcome| EmitCommandOutcome::new(outcome, current_directory.as_js()))
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
        // tsgo's command compiles an incremental program for `incremental`
        // or `composite` (`IsIncremental`, execute/tsc.go:245), whose emit
        // also writes the build info (execute/incremental/program.go:243-273):
        // the emit routes write it after the files (`incremental::emit_build_info`).
        self.with_command_build_info()
            .emit_with_command_outcome(sink, None)
    }

    /// Borrow the production checked host and live resolver for internal
    /// transform/print comparisons. This eager ordinary-emission facet keeps
    /// the prepared options, source identities, libraries and module tables.
    /// It does not run the public emit option guard, create artifacts/sinks,
    /// or confer runtime admission; public Program emit remains unchanged.
    ///
    /// The callback cannot retain the host/resolver borrow. The owned return
    /// includes the authoritative CheckResult, whose whole-Program semantic
    /// diagnostics remain explicitly available. None means the checker did
    /// not construct a snapshot/call the callback (the empty-Program case).
    /// This eager seam must not represent a cold getter or fresh forced emit.
    #[doc(hidden)]
    pub fn with_checked_emit_resolver_for_harness<R>(
        self,
        operation: impl FnOnce(
            &dyn EmitHost,
            &dyn tsc_emitter::EmitResolver,
            &CheckResult,
        ) -> Result<R, DriverError>,
    ) -> Result<(Option<R>, CheckResult), DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let ProgramSession {
            prepared,
            emit_route,
            source_api_facts,
            worker_budget,
            checker_budget: _,
            leak_program: _,
            native_harness: _,
            command_build_info: _,
            command_options_diagnostics: _,
            old_build_info: _,
            build_mode: _,
            list_files_only: _,
            testing: _,
            tracing: _,
            watch: _,
        } = self;
        let emit_host = PreparedEmitHost::new_for_route(&prepared, emit_route, &source_api_facts)?;
        let inputs = project_checker_inputs(&prepared, &source_api_facts)?;
        // The eager checker currently harvests authoritative failures before
        // its emit callback. Preserve failures from first-time emit queries as
        // well: the delegate owns every resolution rule, this wrapper only
        // retains the first exact failed request for the outer driver result.
        struct ObservedProvider<'a> {
            inner: PreparedModuleProvider<'a>,
            failure: std::sync::Mutex<Option<AuthoritativeModuleFailure>>,
        }
        impl AuthoritativeModuleProvider for ObservedProvider<'_> {
            fn program_options_for_module_specifiers(
                &self,
            ) -> Option<&tsc_program::ProgramOptions> {
                self.inner.program_options_for_module_specifiers()
            }

            fn include_processor_diagnostics(&self) -> Vec<(AuthoritativeSourceToken, Diagnostic)> {
                self.inner.include_processor_diagnostics()
            }

            fn resolve_module(
                &self,
                request: AuthoritativeModuleRequest<'_>,
            ) -> Result<AuthoritativeModuleResolution, AuthoritativeModuleLookupFailure>
            {
                let result = self.inner.resolve_module(request);
                if let Err(failure) = &result {
                    let mut first = self.failure.lock().expect("observed provider failure");
                    if first.is_none() {
                        *first = Some(AuthoritativeModuleFailure::Lookup {
                            source_token: request.source_token,
                            containing_file: request.containing_file.to_owned(),
                            specifier: request.specifier.to_owned(),
                            mode: request.mode,
                            failure: *failure,
                        });
                    }
                }
                result
            }
        }
        let provider = ObservedProvider {
            inner: PreparedModuleProvider {
                prepared: &prepared,
                request_plans: std::sync::Mutex::new(BTreeMap::new()),
                tracing: None,
            },
            failure: std::sync::Mutex::new(None),
        };
        let mut pending_operation = Some(operation);
        let mut operation_result = None;
        let checked = check_program_with_authoritative_modules_at_for_emit_with_workers(
            &inputs.libs,
            &inputs.files,
            &inputs.lib_metadata,
            &inputs.file_metadata,
            prepared.compiler_options(),
            &inputs.current_directory,
            &provider,
            worker_budget,
            IncrementalRequest::NONE,
            |snapshot, checker, checked| {
                if let Some(partial) = checked.partial_checks.first() {
                    operation_result = Some(Err(DriverError::IncompleteCheck {
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
                    prepared_sources: None,
                };
                operation_result = Some(checker.with_emit_resolver(|resolver| {
                    pending_operation
                        .take()
                        .expect("checked harness callback runs once")(
                        &checked_host,
                        resolver,
                        checked,
                    )
                }));
            },
        )
        .map_err(|failure| map_authoritative_failure(&prepared, failure))?;
        if let Some(failure) = provider
            .failure
            .into_inner()
            .expect("observed provider failure")
        {
            return Err(map_authoritative_failure(&prepared, failure));
        }
        Ok((operation_result.transpose()?, checked))
    }

    /// h2-6a-m-2 §8-A.1 harness-print bridge: run the production
    /// plan → checker-resolver → transform → print pipeline and return
    /// each script unit's printed text (with an optionally injected
    /// source-map recording), WITHOUT artifacts, sinks, or the emit
    /// option preflight. Qualification-only:
    /// the replay suite byte-compares the returned units against the
    /// frozen witnesses; production emits keep every refusal lane.
    #[doc(hidden)]
    pub fn print_units_with_source_map_recording_for_harness(
        self,
        recording_inputs_for: &dyn Fn(JsStr<'_>) -> Option<SourceMapRecordingInputs>,
    ) -> Result<Vec<(JsString, PrintedText)>, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        let ProgramSession {
            prepared,
            emit_route,
            source_api_facts,
            worker_budget,
            checker_budget: _,
            leak_program: _,
            native_harness: _,
            command_build_info: _,
            command_options_diagnostics: _,
            old_build_info: _,
            build_mode: _,
            list_files_only: _,
            testing: _,
            tracing: _,
            watch: _,
        } = self;
        let emit_host = PreparedEmitHost::new_for_route(&prepared, emit_route, &source_api_facts)?;
        let selection = EmitSelection::WholeProgram;
        let preflight = preflight_emit(&emit_host, selection).map_err(DriverError::Emit)?;

        let inputs = project_checker_inputs(&prepared, &source_api_facts)?;
        let provider = PreparedModuleProvider {
            prepared: &prepared,
            request_plans: std::sync::Mutex::new(BTreeMap::new()),
            tracing: None,
        };
        let mut print_result: Option<Result<Vec<(JsString, PrintedText)>, DriverError>> = None;
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
                    prepared_sources: None,
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
        let checked = check_program_with_authoritative_modules_at_for_emit_with_workers(
            &inputs.libs,
            &inputs.files,
            &inputs.lib_metadata,
            &inputs.file_metadata,
            prepared.compiler_options(),
            &inputs.current_directory,
            &provider,
            worker_budget,
            IncrementalRequest::NONE,
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
        self.emit_with_command_outcome_for_route(sink, harness_lib_bundle, false)
    }

    /// H2.8c transpileDeclaration adapter: the same checked session as an
    /// ordinary command (syntactic/options/global/semantic buckets retained
    /// for the caller's diagnostic selection) but executing
    /// `program.emit(undefined, undefined, undefined, /*emitOnlyDtsFiles*/ true,
    /// undefined, /*forceDtsEmit*/ true)` (typescript.js:146112-146121):
    /// handleNoEmitOptions is skipped and declaration output is forced.
    /// tsrs-native: route adapter over emit_forced_declarations.
    pub(crate) fn emit_forced_declarations_command_for_transpile(
        self,
        sink: &mut dyn OutputSink,
    ) -> Result<CliEmitSessionOutcome, DriverError> {
        self.emit_with_command_outcome_for_route(sink, None, true)
    }

    fn emit_with_command_outcome_for_route(
        self,
        sink: &mut dyn OutputSink,
        harness_lib_bundle: Option<&OwnedHarnessLibBundle>,
        forced_declarations: bool,
    ) -> Result<CliEmitSessionOutcome, DriverError> {
        self.require_mode(PreparedProgramMode::Emit)?;
        if self.checker_budget.is_sharded()
            && harness_lib_bundle.is_none()
            && !forced_declarations
            && sharded_emit_supported(self.prepared.compiler_options())
        {
            return self.emit_sharded(sink);
        }
        let ProgramSession {
            prepared,
            emit_route,
            source_api_facts,
            worker_budget,
            checker_budget: _,
            leak_program: _,
            native_harness: _,
            command_build_info,
            command_options_diagnostics: _,
            old_build_info,
            build_mode,
            list_files_only: _,
            testing,
            tracing,
            watch,
        } = self;
        let emit_host = PreparedEmitHost::new_for_route(&prepared, emit_route, &source_api_facts)?
            .with_collected_emitted_files(build_mode || testing)
            .with_tracing(tracing.clone());
        if forced_declarations {
            tsc_emitter::validate_forced_declaration_request(&emit_host)
                .map_err(DriverError::Emit)?;
        } else {
            validate_emit_request(&emit_host).map_err(DriverError::Emit)?;
        }
        let selection = EmitSelection::WholeProgram;
        let preflight = if forced_declarations {
            None
        } else {
            Some(preflight_emit(&emit_host, selection).map_err(DriverError::Emit)?)
        };

        let inputs = project_checker_inputs(&prepared, &source_api_facts)?;
        let provider = PreparedModuleProvider {
            prepared: &prepared,
            request_plans: std::sync::Mutex::new(BTreeMap::new()),
            tracing: tracing.clone(),
        };
        let incremental_facts_requested = command_build_info
            && (tsc_incremental::options::is_incremental(prepared.compiler_options())
                || build_mode
                || watch);
        // tsgo's semantic getter is gated by the program (options) rows,
        // which the planner cannot see from the checker.
        let options_rows = !prepared.diagnostics().options().is_empty()
            || !programmatic_option_diagnostics(&prepared).is_empty()
            || !program_rows_outside_sources(&prepared).is_empty()
            || preflight
                .as_ref()
                .is_some_and(|preflight| !preflight.diagnostics().is_empty());
        let driver = incremental::IncrementalDriver::new(
            &prepared,
            old_build_info.clone(),
            emit_route,
            &source_api_facts,
            incremental::AffectedPolicy {
                emits: true,
                options_diagnostics: options_rows,
                build: build_mode,
                hash_with_text: testing,
                watch,
            },
        );
        let mut planner = driver.planner();
        let mut pending_preflight = preflight;
        let mut emit_result: Option<Result<CliEmitSessionOutcome, DriverError>> = None;
        let mut operation = |snapshot: &ProgramSnapshot,
                             checker: &CheckerSession<'_>,
                             checked: &CheckResult| {
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
            let mut diagnostics = emit_session_diagnostics(&prepared, checked);
            let command = incremental::CommandDiagnosticFacts::of(&diagnostics, false);
            if let Some(facts) = &checked.incremental {
                driver.store_check(
                    facts,
                    command.semantic_cached() && prepared.compiler_options().no_check != Some(true),
                );
                diagnostics.extend_semantic(driver.cached_semantic_diagnostics());
            }
            let diagnostic_gate = diagnostics.gate();
            let work_counters = check_work_counters(checked);
            let checked_host = CheckedEmitHost {
                prepared: &emit_host,
                snapshot,
                prepared_sources: None,
            };
            let preflight = pending_preflight.take();
            let preflight_diagnostics = preflight
                .as_ref()
                .map(|preflight| preflight.diagnostics().to_vec())
                .unwrap_or_default();
            // tsgo `Program.Emit`: the program's emit is one span.
            let _span = tracing
                .as_ref()
                .map(|tracing| tracing.begin(TracePhase::Emit, "emit", TraceArgs::new()));
            emit_result = Some(checker.with_emit_resolver(|resolver| {
                let mut recording = incremental::SignatureRecordingSink::with_composite_signatures(
                    &mut *sink,
                    driver.composite_signatures(),
                    build_mode,
                    testing,
                );
                // The units an incremental program re-emits (tsgo
                // emitFilesIncremental), with the plan they index.
                let requests = preflight.as_ref().and_then(|preflight| {
                    driver
                        .unit_requests(preflight)
                        .map(|requests| (requests, preflight.clone()))
                });
                // tsgo `HandleNoEmitOnError` asks the incremental program for
                // the declaration diagnostics: it records each file's and
                // adds the cached ones of the others.
                let incremental_declaration = match &preflight {
                    Some(preflight)
                        if checked.incremental.is_some()
                            && diagnostic_gate.wants_declaration_diagnostics(
                                prepared.compiler_options(),
                                preflight.diagnostics(),
                            ) =>
                    {
                        let paths = preflight.declaration_paths(&checked_host);
                        let mut declaration = Vec::new();
                        let sources = tsc_emitter::get_source_files_to_emit(
                            &checked_host,
                            tsc_emitter::EmitSelection::WholeProgram,
                        );
                        match sources.and_then(|sources| {
                            sources.into_iter().try_for_each(|source| {
                                declaration.extend(tsc_emitter::get_declaration_diagnostics(
                                    resolver,
                                    &checked_host,
                                    paths,
                                    source,
                                )?);
                                Ok(())
                            })
                        }) {
                            Ok(()) => {
                                sort_and_dedupe_diagnostics(&mut declaration);
                                declaration
                                    .extend(driver.record_declaration_diagnostics(&declaration));
                                sort_and_dedupe_diagnostics(&mut declaration);
                                Some(Ok(declaration))
                            }
                            Err(error) => Some(Err(error)),
                        }
                    }
                    _ => None,
                };
                let incremental_gate;
                let diagnostic_gate = match incremental_declaration {
                    Some(Ok(declaration)) => {
                        incremental_gate = diagnostic_gate
                            .clone()
                            .with_declaration_diagnostics(declaration);
                        &incremental_gate
                    }
                    Some(Err(error)) => return Err(DriverError::Emit(error)),
                    None => &diagnostic_gate,
                };
                match (preflight, &requests) {
                    (Some(preflight), Some((requests, _))) => tsc_emitter::emit_planned_files(
                        resolver,
                        &checked_host,
                        preflight,
                        selection,
                        diagnostic_gate,
                        &mut recording,
                        requests,
                    ),
                    (Some(preflight), None) => emit_files(
                        resolver,
                        &checked_host,
                        preflight,
                        selection,
                        diagnostic_gate,
                        &mut recording,
                    ),
                    (None, _) => tsc_emitter::emit_forced_declarations(
                        resolver,
                        &checked_host,
                        selection,
                        &mut recording,
                    ),
                }
                .map(|emit| {
                    // tsgo emitBuildInfo after the emit of an incremental
                    // program.
                    let emit = if checked.incremental.is_some() {
                        let cached = driver.record_emit(
                            &emit,
                            &recording.records(),
                            recording.wrote_anything(),
                            requests
                                .as_ref()
                                .map(|(requests, preflight)| (requests.as_slice(), preflight)),
                        );
                        let emit = emit.with_cached_diagnostics(cached);
                        match driver.build_info(command) {
                            Some(document) => {
                                let _span = trace_build_info(tracing.as_ref());
                                let failure = recording.write_build_info(&document);
                                emit.with_build_info(document.file_name, failure)
                            }
                            None => emit,
                        }
                    } else {
                        emit
                    };
                    let mut outcome =
                        diagnostics.with_emit(&preflight_diagnostics, emit, work_counters);
                    outcome.checked_source_files = checker.checked_source_files();
                    outcome.build_emit = driver.build_emit_facts();
                    if testing {
                        outcome.program_report = driver.program_report();
                    }
                    outcome.watch_state = driver.take_watch_state();
                    outcome
                })
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
            check_program_with_authoritative_modules_at_for_emit_with_workers(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                worker_budget,
                IncrementalRequest {
                    facts: incremental_facts_requested,
                    planner: incremental_facts_requested
                        .then_some(&mut planner as &mut IncrementalPlanner<'_>),
                },
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
        let Some(preflight) = pending_preflight else {
            // Empty forced-declaration Program: no source can reach a resolver.
            return tsc_emitter::emit_forced_declarations(
                &UnavailableEmitResolver,
                &emit_host,
                selection,
                sink,
            )
            .map(|emit| diagnostics.with_emit(&[], emit, work_counters))
            .map_err(DriverError::Emit);
        };
        let preflight_diagnostics = preflight.diagnostics().to_vec();
        let mut recording = incremental::SignatureRecordingSink::new(sink, testing);
        emit_files(
            &UnavailableEmitResolver,
            &emit_host,
            preflight,
            selection,
            &diagnostic_gate,
            &mut recording,
        )
        .map(|emit| {
            // tsgo emitBuildInfo of an incremental program without files.
            let command = incremental::CommandDiagnosticFacts::of(&diagnostics, false);
            let emit = match checked.incremental.as_ref() {
                Some(facts) => {
                    driver.store_check(facts, false);
                    driver.record_emit(
                        &emit,
                        &recording.records(),
                        recording.wrote_anything(),
                        None,
                    );
                    match driver.build_info(command) {
                        Some(document) => {
                            let _span = trace_build_info(tracing.as_ref());
                            let failure = recording.write_build_info(&document);
                            emit.with_build_info(document.file_name, failure)
                        }
                        None => emit,
                    }
                }
                None => emit,
            };
            let mut outcome = diagnostics.with_emit(&preflight_diagnostics, emit, work_counters);
            outcome.build_emit = driver.build_emit_facts();
            if testing {
                outcome.program_report = driver.program_report();
            }
            outcome.watch_state = driver.take_watch_state();
            outcome
        })
        .map_err(DriverError::Emit)
    }

    /// The emitting session over the sharded checker: parse and bind once,
    /// then every checker shard checks and emits its own files while the
    /// coordinator gates the merged diagnostics (handleNoEmitOptions) and
    /// writes the products in plan order.
    /// tsrs-native: tsgo's per-checker emit; see [`ShardedEmit`].
    fn emit_sharded(self, sink: &mut dyn OutputSink) -> Result<CliEmitSessionOutcome, DriverError> {
        let ProgramSession {
            prepared,
            emit_route,
            source_api_facts,
            worker_budget,
            checker_budget,
            leak_program,
            native_harness: _,
            command_build_info,
            command_options_diagnostics: _,
            old_build_info,
            build_mode,
            list_files_only: _,
            testing,
            tracing,
            watch,
        } = self;
        let setup_started = std::time::Instant::now();
        let emit_host = PreparedEmitHost::new_for_route(&prepared, emit_route, &source_api_facts)?
            .with_collected_emitted_files(build_mode || testing)
            .with_tracing(tracing.clone());
        validate_emit_request(&emit_host).map_err(DriverError::Emit)?;
        tsc_types::trace::mark("emit: host and request validation", setup_started);
        let incremental_facts_requested = command_build_info
            && (tsc_incremental::options::is_incremental(prepared.compiler_options())
                || build_mode
                || watch);
        let selection = EmitSelection::WholeProgram;
        let preflight_started = std::time::Instant::now();
        let preflight = preflight_emit(&emit_host, selection).map_err(DriverError::Emit)?;
        let preflight_diagnostics = preflight.diagnostics().to_vec();
        tsc_types::trace::mark("emit: preflight (output plan)", preflight_started);
        // tsgo's semantic getter is gated by the program (options) rows,
        // which the planner cannot see from the checker.
        let options_rows = !prepared.diagnostics().options().is_empty()
            || !programmatic_option_diagnostics(&prepared).is_empty()
            || !program_rows_outside_sources(&prepared).is_empty()
            || !preflight_diagnostics.is_empty();
        let driver = incremental::IncrementalDriver::new(
            &prepared,
            old_build_info.clone(),
            emit_route,
            &source_api_facts,
            incremental::AffectedPolicy {
                emits: true,
                options_diagnostics: options_rows,
                build: build_mode,
                hash_with_text: testing,
                watch,
            },
        );
        let mut planner = driver.planner();
        let mut recording = incremental::SignatureRecordingSink::with_composite_signatures(
            sink,
            driver.composite_signatures(),
            build_mode,
            testing,
        );
        let sink: &mut dyn OutputSink = &mut recording;
        let inputs_started = std::time::Instant::now();
        // A shard owns the planned units whose source is one of the snapshot
        // documents it checked; the checked host matches them by this name.
        let unit_names = preflight
            .plan()
            .units()
            .iter()
            .map(|unit| {
                unit.root()
                    .source_files()
                    .first()
                    .and_then(|&id| emit_host.expected_source_name(id))
            })
            .collect::<Vec<Option<JsString>>>();
        let inputs = project_checker_inputs(&prepared, &source_api_facts)?;
        let factory = PreparedProviderFactory {
            prepared: &prepared,
            tracing: tracing.clone(),
        };
        tsc_types::trace::mark("emit: unit names and checker inputs", inputs_started);
        tsc_types::trace::mark("emit: host, preflight, checker inputs", setup_started);

        #[allow(clippy::large_enum_variant)]
        enum GateOutcome {
            Failed(DriverError),
            Blocked(EmitOutcome, ProgramDiagnostics, NoEmitWorkCounters),
            Ready(EmitFilesSession, ProgramDiagnostics, NoEmitWorkCounters),
        }
        let mut gate_outcome: Option<GateOutcome> = None;
        // A shared (stateless filesystem) sink lets each worker write its
        // unit's artifacts as soon as they are printed.
        let eager_sink = sink.shared();
        // Per-shard eager emit (tsgo's shape: every checker emits the files it
        // checked as soon as it has checked them). With noEmitOnError off the
        // gate admits every shard, so a shard emits its own units on its
        // thread right after its check, overlapping the shards still
        // checking; the coordinator's emit step then collects those units
        // and emits only what no shard emitted. A stash entry carries the
        // files it was computed for so a serial replay (fresh sessions,
        // another assignment) cannot consume a discarded run's products.
        let eager_emit_enabled = prepared.compiler_options().no_emit_on_error != Some(true);
        // `.d.ts` printing creates types in resolver order, so a declaration
        // run emits one session's units in one ordered job; JavaScript
        // output does not depend on that order.
        let declaration_output =
            tsc_checker::declaration_output_requested(prepared.compiler_options());
        // The units an incremental program re-emits, each with its pending
        // members (tsgo emitFilesIncremental); every unit, whole, otherwise.
        let unit_requests: std::sync::OnceLock<
            Option<std::collections::HashMap<usize, tsc_emitter::UnitEmitRequest>>,
        > = std::sync::OnceLock::new();
        let requests_for = |units: &[usize]| -> Vec<tsc_emitter::UnitEmitRequest> {
            let requests = unit_requests.get_or_init(|| {
                driver.unit_requests(&preflight).map(|requests| {
                    requests
                        .into_iter()
                        .map(|request| (request.unit, request))
                        .collect()
                })
            });
            match requests {
                None => units
                    .iter()
                    .map(|&unit| tsc_emitter::UnitEmitRequest::whole(unit))
                    .collect(),
                Some(requests) => units
                    .iter()
                    .filter_map(|unit| requests.get(unit).copied())
                    .collect(),
            }
        };
        type EagerShardEmission = (
            Vec<usize>,
            Result<Vec<tsc_emitter::UnitEmission>, UnitEmitError>,
        );
        let eager_emissions: std::sync::Mutex<Vec<Option<EagerShardEmission>>> =
            std::sync::Mutex::new(Vec::new());
        // The emit copies of the largest JavaScript-emitting sources are
        // prepared beside the checkers (clone + parse-time transform flags,
        // syntax only) so the unit that decides the tail starts its
        // transforms at once. Largest first, bounded so the copies waiting
        // for their emit stay a small share of the run's memory.
        let mut prelude_candidates: Vec<(SourceFileId, usize)> = preflight
            .plan()
            .units()
            .iter()
            .filter(|unit| unit.paths().javascript_path().is_some())
            .filter_map(|unit| unit.root().source_files().first().copied())
            .filter_map(|id| {
                let size = emit_host.prepared.source_file(id)?.text().len();
                (size >= 256 * 1024).then_some((id, size))
            })
            .collect();
        prelude_candidates.sort_by_key(|&(_, size)| std::cmp::Reverse(size));
        let mut candidate_bytes = 0usize;
        prelude_candidates.retain(|&(_, size)| {
            candidate_bytes += size;
            candidate_bytes <= 16 * 1024 * 1024
        });
        if worker_budget.max_workers() < 4 {
            prelude_candidates.clear();
        }
        let prepared_bytes: usize = prelude_candidates.iter().map(|&(_, size)| size).sum();
        let prelude_slots: Vec<std::sync::Mutex<PreludeSlot>> = {
            let slots = prelude_candidates
                .iter()
                .map(|(id, _)| id.index() + 1)
                .max()
                .unwrap_or(0);
            (0..slots)
                .map(|_| std::sync::Mutex::new(PreludeSlot::Pending))
                .collect()
        };
        let prelude = |snapshot: &ProgramSnapshot| {
            let started = std::time::Instant::now();
            let next = std::sync::atomic::AtomicUsize::new(0);
            let prepare_next = || {
                while let Some(&(id, _)) =
                    prelude_candidates.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
                {
                    let Some(expected_name) = emit_host.expected_source_name(id) else {
                        continue;
                    };
                    let document = snapshot
                        .documents()
                        .get(id.index())
                        .filter(|document| {
                            document.source().file_name.as_js() == expected_name.as_js()
                        })
                        .or_else(|| {
                            snapshot.documents().iter().find(|document| {
                                document.source().file_name.as_js() == expected_name.as_js()
                            })
                        });
                    let Some(document) = document else {
                        continue;
                    };
                    if let Ok(prepared) =
                        tsc_emitter::prepare_emit_source(document.source(), Some(id))
                    {
                        if let Ok(mut slot) = prelude_slots[id.index()].lock() {
                            if matches!(*slot, PreludeSlot::Pending) {
                                *slot = PreludeSlot::Ready(Box::new(prepared));
                            }
                        }
                    }
                }
            };
            // Two preparers: the largest source alone takes most of the time.
            std::thread::scope(|scope| {
                if prelude_candidates.len() > 1 {
                    let _ = std::thread::Builder::new()
                        .name("tsc-rs-emit-prelude".to_owned())
                        .stack_size(tsc_program::WORKER_STACK_BYTES)
                        .spawn_scoped(scope, || {
                            tsc_program::run_thread_start_hook();
                            prepare_next();
                        });
                }
                prepare_next();
            });
            if tsc_types::trace::enabled() {
                tsc_types::trace::mark(
                    &format!(
                        "emit: prelude ({} sources, {} bytes)",
                        prelude_candidates.len(),
                        prepared_bytes
                    ),
                    started,
                );
            }
        };
        let prelude: Option<tsc_checker::ShardPreludeClosure<'_>> =
            (!prelude_candidates.is_empty()).then_some(&prelude);
        let (checked, emissions) = {
            let mut gate = |snapshot: &ProgramSnapshot,
                            checked: &CheckResult,
                            sessions: &[CheckerSession<'_>],
                            files_by_shard: &[Vec<usize>]|
             -> bool {
                if let Some(partial) = checked.partial_checks.first() {
                    gate_outcome = Some(GateOutcome::Failed(DriverError::IncompleteCheck {
                        file_name: partial.file_name.clone(),
                        start: partial.start,
                        length: partial.length,
                        reason: partial.reason.clone(),
                        additional_partial_checks: checked.partial_checks.len().saturating_sub(1),
                    }));
                    return false;
                }
                let gate_started = std::time::Instant::now();
                let mut diagnostics = emit_session_diagnostics(&prepared, checked);
                if let Some(facts) = &checked.incremental {
                    let command = incremental::CommandDiagnosticFacts::of(&diagnostics, false);
                    driver.store_check(
                        facts,
                        command.semantic_cached()
                            && prepared.compiler_options().no_check != Some(true),
                    );
                    diagnostics.extend_semantic(driver.cached_semantic_diagnostics());
                }
                let mut diagnostic_gate = diagnostics.gate();
                let work_counters = check_work_counters(checked);
                let checked_host = CheckedEmitHost {
                    prepared: &emit_host,
                    snapshot,
                    prepared_sources: None,
                };
                tsc_types::trace::mark("emit: gate diagnostics", gate_started);
                if diagnostic_gate.wants_declaration_diagnostics(
                    prepared.compiler_options(),
                    preflight.diagnostics(),
                ) {
                    // tsc's handleNoEmitOptions transforms every source's
                    // declarations before any emit; here each source is
                    // transformed against the resolver of the shard that
                    // checked it, on the worker budget, and the merged
                    // result feeds the gate.
                    let declaration_started = std::time::Instant::now();
                    match sharded_declaration_diagnostics(
                        &checked_host,
                        &emit_host,
                        preflight.declaration_paths(&checked_host),
                        sessions,
                        files_by_shard,
                        worker_budget,
                        0,
                        false,
                        driver.declaration_file_filter().as_deref(),
                    ) {
                        Ok(mut declaration) => {
                            // tsgo `HandleNoEmitOnError` asks the incremental
                            // program, which records each file's declaration
                            // errors and adds the cached ones of the others.
                            let from_cache = driver.record_declaration_diagnostics(&declaration);
                            if !from_cache.is_empty() {
                                declaration.extend(from_cache);
                                sort_and_dedupe_diagnostics(&mut declaration);
                            }
                            diagnostic_gate =
                                diagnostic_gate.with_declaration_diagnostics(declaration);
                        }
                        Err(error) => {
                            gate_outcome = Some(GateOutcome::Failed(DriverError::Emit(error)));
                            return false;
                        }
                    }
                    tsc_types::trace::mark(
                        "emit: declaration diagnostics (per shard)",
                        declaration_started,
                    );
                }
                let begin_started = std::time::Instant::now();
                let started =
                    begin_emit_files(None, &checked_host, &preflight, selection, &diagnostic_gate);
                tsc_types::trace::mark("emit: begin_emit_files", begin_started);
                match started {
                    Ok(EmitFilesStart::Blocked(outcome)) => {
                        gate_outcome =
                            Some(GateOutcome::Blocked(*outcome, diagnostics, work_counters));
                        false
                    }
                    Ok(EmitFilesStart::Ready(session)) => {
                        gate_outcome =
                            Some(GateOutcome::Ready(session, diagnostics, work_counters));
                        true
                    }
                    Err(error) => {
                        gate_outcome = Some(GateOutcome::Failed(DriverError::Emit(error)));
                        false
                    }
                }
            };
            let emit = |snapshot: &ProgramSnapshot,
                        sessions: &[CheckerSession<'_>],
                        files_by_shard: &[Vec<usize>]|
             -> Result<Vec<ShardEmission>, UnitEmitError> {
                // tsgo `Program.Emit`: the program's emit is one span.
                let _span = tracing
                    .as_ref()
                    .map(|tracing| tracing.begin(TracePhase::Emit, "emit", TraceArgs::new()));
                let checked_host = CheckedEmitHost {
                    prepared: &emit_host,
                    snapshot,
                    prepared_sources: Some(&prelude_slots),
                };
                // Each planned unit belongs to the shard that checked its
                // source; every session then emits its own units in one
                // resolver borrow on the worker budget (tsgo's shape: each
                // checker emits the files it checked), so no two workers
                // contend for one session's resolver.
                let mut owner_by_name = std::collections::HashMap::new();
                for (shard, files) in files_by_shard.iter().enumerate() {
                    for &file in files {
                        owner_by_name
                            .insert(snapshot.document(file).source().file_name.as_js(), shard);
                    }
                }
                let mut units_by_shard: Vec<Vec<usize>> = vec![Vec::new(); sessions.len()];
                for (unit, name) in unit_names.iter().enumerate() {
                    if let Some(&shard) = name
                        .as_ref()
                        .and_then(|name| owner_by_name.get(&name.as_js()))
                    {
                        units_by_shard[shard].push(unit);
                    }
                }
                // Shards that already emitted their units eagerly, for this
                // very assignment (a serial replay presents another one).
                let mut stashed = std::mem::take(
                    &mut *eager_emissions
                        .lock()
                        .expect("eager shard emissions are never poisoned"),
                );
                stashed.resize_with(sessions.len().max(stashed.len()), || None);
                for (shard, entry) in stashed.iter_mut().enumerate() {
                    if entry
                        .as_ref()
                        .is_some_and(|(files, _)| files_by_shard.get(shard) != Some(files))
                    {
                        *entry = None;
                    }
                }
                // Units a shard already emitted eagerly leave its job; a
                // shard whose eager emit failed keeps no job (its error is
                // reported below). Without declaration output the pool takes
                // one unit per job, so every worker shares the tail the last
                // shards leave behind; with declaration output a shard's
                // units stay one ordered job.
                let mut jobs: Vec<(usize, Vec<usize>)> = Vec::new();
                for (shard, units) in units_by_shard.into_iter().enumerate() {
                    let remaining: Vec<usize> = match stashed.get(shard) {
                        Some(Some((_, Ok(emitted)))) => {
                            let emitted = emitted
                                .iter()
                                .map(|emission| emission.unit())
                                .collect::<std::collections::HashSet<_>>();
                            units
                                .into_iter()
                                .filter(|unit| !emitted.contains(unit))
                                .collect()
                        }
                        Some(Some((_, Err(_)))) => Vec::new(),
                        _ => units,
                    };
                    if remaining.is_empty() {
                        continue;
                    }
                    if declaration_output {
                        jobs.push((shard, remaining));
                    } else {
                        jobs.extend(remaining.into_iter().map(|unit| (shard, vec![unit])));
                    }
                }
                let weight = |(_, units): &(usize, Vec<usize>)| {
                    units
                        .iter()
                        .map(|&unit| {
                            preflight.plan().units()[unit]
                                .root()
                                .source_files()
                                .first()
                                .and_then(|&id| emit_host.prepared.source_file(id))
                                .map_or(0, |source| source.text().len())
                        })
                        .sum::<usize>()
                };
                let results = worker_budget.map_ordered(jobs, weight, |(shard, units)| {
                    let mut eager = eager_sink.map(tsc_emitter::EagerUnitSink);
                    sessions[shard].with_emit_resolver(|resolver| {
                        tsc_emitter::emit_planned_units_with_kinds(
                            resolver,
                            &checked_host,
                            &preflight,
                            &requests_for(&units),
                            eager
                                .as_mut()
                                .map(|sink| sink as &mut dyn tsc_emitter::OutputSink),
                        )
                    })
                });
                let mut units = Vec::with_capacity(results.len());
                let mut first_error: Option<UnitEmitError> = None;
                let eager_results = stashed.into_iter().flatten().map(|(_, result)| result);
                for result in eager_results.chain(results) {
                    match result {
                        Ok(emitted) => units.extend(emitted),
                        Err(error) => {
                            if first_error
                                .as_ref()
                                .is_none_or(|first| error.unit < first.unit)
                            {
                                first_error = Some(error);
                            }
                        }
                    }
                }
                // finish_emit_files orders the products by plan unit,
                // whichever thread emitted them.
                if let Some(error) = first_error {
                    return Err(error);
                }
                Ok(vec![ShardEmission {
                    units,
                    checked_source_files: sessions
                        .iter()
                        .map(CheckerSession::checked_source_files)
                        .fold(0u32, u32::saturating_add),
                }])
            };
            // The tail protocol of the eager emit: the last shard to finish
            // checking claims the tail (`tail_claimed`) and emits its
            // heaviest remaining unit at once; every other shard keeps
            // emitting its own units in plan order until the tail owner
            // opens the pool (`pool_open`), which then balances whatever is
            // left across every worker.
            let tail_claimed = std::sync::atomic::AtomicBool::new(false);
            let pool_open = std::sync::atomic::AtomicBool::new(false);
            let eager = |shard: usize,
                         snapshot: &ProgramSnapshot,
                         session: &CheckerSession<'_>,
                         files: &[usize],
                         checking: &std::sync::atomic::AtomicUsize| {
                use std::sync::atomic::Ordering;
                let owned = files
                    .iter()
                    .map(|&file| {
                        let source = snapshot.document(file).source();
                        (source.file_name.as_js(), source.text().len())
                    })
                    .collect::<std::collections::HashMap<_, _>>();
                // The shard's units in plan order, each with its source size
                // (the pool's job weight).
                let units = unit_names
                    .iter()
                    .enumerate()
                    .filter_map(|(unit, name)| {
                        name.as_ref()
                            .and_then(|name| owned.get(&name.as_js()))
                            .map(|&size| (unit, size))
                    })
                    .collect::<Vec<_>>();
                if units.is_empty() {
                    // A last shard without units of its own opens the pool
                    // for the units the other shards leave behind.
                    if checking.load(Ordering::Acquire) == 0 {
                        pool_open.store(true, Ordering::Release);
                    }
                    return;
                }
                let emit_started = std::time::Instant::now();
                let checked_host = CheckedEmitHost {
                    prepared: &emit_host,
                    snapshot,
                    prepared_sources: Some(&prelude_slots),
                };
                let mut eager_unit_sink = eager_sink.map(tsc_emitter::EagerUnitSink);
                // A shard emits its own units in plan order while another
                // shard is still checking: that work overlaps the check. Once
                // it is the last shard checking, the coordinator's pool takes
                // the remaining units so that every worker shares them. The
                // pool only starts after every shard thread has joined,
                // though, and another shard may still be inside an eager
                // unit; the heaviest remaining unit of the last shard (the
                // one that decides the tail) therefore starts here at once
                // instead of waiting for that join. With declaration output a
                // shard's units are one ordered pool job anyway, so the last
                // shard simply finishes them in order.
                let (emitted, failure, owns_tail) = session.with_emit_resolver(|resolver| {
                    let mut emitted = Vec::new();
                    let mut remaining = units.clone();
                    // The shard that finds every check finished before it
                    // has emitted anything is the last one checking (the
                    // count drops before this closure runs).
                    let mut owns_tail = false;
                    let mut first = true;
                    while !remaining.is_empty() {
                        if checking.load(Ordering::Acquire) == 0 && !owns_tail {
                            if first
                                && tail_claimed
                                    .compare_exchange(
                                        false,
                                        true,
                                        Ordering::AcqRel,
                                        Ordering::Acquire,
                                    )
                                    .is_ok()
                            {
                                owns_tail = true;
                            } else if pool_open.load(Ordering::Acquire) {
                                break;
                            }
                        }
                        first = false;
                        let index = if owns_tail && !declaration_output {
                            remaining
                                .iter()
                                .enumerate()
                                .max_by_key(|(_, (_, size))| *size)
                                .map_or(0, |(index, _)| index)
                        } else {
                            0
                        };
                        let (unit, _) = remaining.remove(index);
                        let requests = requests_for(&[unit]);
                        if requests.is_empty() {
                            // Not pending in the incremental program.
                            if owns_tail && !declaration_output {
                                break;
                            }
                            continue;
                        }
                        match tsc_emitter::emit_planned_units_with_kinds(
                            resolver,
                            &checked_host,
                            &preflight,
                            &requests,
                            eager_unit_sink
                                .as_mut()
                                .map(|sink| sink as &mut dyn tsc_emitter::OutputSink),
                        ) {
                            Ok(units) => emitted.extend(units),
                            Err(error) => return (emitted, Some(error), owns_tail),
                        }
                        if owns_tail && !declaration_output {
                            break;
                        }
                    }
                    (emitted, None, owns_tail)
                });
                if owns_tail {
                    pool_open.store(true, Ordering::Release);
                }
                if tsc_types::trace::enabled() {
                    tsc_types::trace::mark(
                        &format!(
                            "shard {shard}: emit (eager, {} of {} units{})",
                            emitted.len(),
                            units.len(),
                            if owns_tail { ", tail owner" } else { "" }
                        ),
                        emit_started,
                    );
                }
                if emitted.is_empty() && failure.is_none() {
                    // Nothing emitted here: the pool takes every unit.
                    return;
                }
                let mut stash = eager_emissions
                    .lock()
                    .expect("eager shard emissions are never poisoned");
                if stash.len() <= shard {
                    stash.resize_with(shard + 1, || None);
                }
                stash[shard] = Some((
                    files.to_vec(),
                    match failure {
                        Some(error) => Err(error),
                        None => Ok(emitted),
                    },
                ));
            };
            let eager: tsc_checker::ShardEagerClosure<'_> = &eager;
            let mut sharded_emit = ShardedEmit {
                gate: &mut gate,
                emit: &emit,
                emissions: None,
                eager: eager_emit_enabled.then_some(eager),
                prelude: if eager_emit_enabled { prelude } else { None },
            };
            let checked = check_program_with_authoritative_modules_at_for_emit_with_checkers(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                prepared.compiler_options(),
                &inputs.current_directory,
                &factory,
                worker_budget,
                checker_budget,
                &mut sharded_emit,
                IncrementalRequest {
                    facts: incremental_facts_requested,
                    planner: incremental_facts_requested
                        .then_some(&mut planner as &mut IncrementalPlanner<'_>),
                },
            )
            .map_err(|failure| map_authoritative_failure(&prepared, failure))?;
            (checked, sharded_emit.emissions.take())
        };

        // tsgo emitBuildInfo after the emit (or the skipped emit) of an
        // incremental program.
        let finish_build_info = |emit: EmitOutcome,
                                 diagnostics: &ProgramDiagnostics,
                                 recording: &mut incremental::SignatureRecordingSink<'_>|
         -> EmitOutcome {
            let command = incremental::CommandDiagnosticFacts::of(diagnostics, false);
            let Some(facts) = checked.incremental.as_ref() else {
                return emit;
            };
            driver.store_check(
                facts,
                command.semantic_cached() && prepared.compiler_options().no_check != Some(true),
            );
            let requests = driver.unit_requests(&preflight);
            let cached = driver.record_emit(
                &emit,
                &recording.records(),
                recording.wrote_anything(),
                requests.as_deref().map(|requests| (requests, &preflight)),
            );
            let emit = emit.with_cached_diagnostics(cached);
            match driver.build_info(command) {
                Some(document) => {
                    let _span = trace_build_info(tracing.as_ref());
                    let failure = recording.write_build_info(&document);
                    emit.with_build_info(document.file_name, failure)
                }
                None => emit,
            }
        };
        let outcome = match gate_outcome {
            Some(GateOutcome::Failed(error)) => Err(error),
            Some(GateOutcome::Blocked(outcome, diagnostics, work_counters)) => {
                let emit = finish_build_info(outcome, &diagnostics, &mut recording);
                Ok(diagnostics.with_emit(&preflight_diagnostics, emit, work_counters))
            }
            Some(GateOutcome::Ready(session, diagnostics, work_counters)) => {
                let emissions = emissions
                    .expect("an admitted sharded emit hands back every shard's products")
                    .map_err(|error| DriverError::Emit(error.failure))?;
                let mut units = Vec::new();
                let mut checked_source_files = 0u32;
                for shard in emissions {
                    checked_source_files =
                        checked_source_files.saturating_add(shard.checked_source_files);
                    units.extend(shard.units);
                }
                let finish_started = std::time::Instant::now();
                let emit =
                    finish_emit_files(session, units, &mut recording).map_err(DriverError::Emit)?;
                tsc_types::trace::mark("emit: finish_emit_files (assemble, write)", finish_started);
                let emit = finish_build_info(emit, &diagnostics, &mut recording);
                let mut outcome =
                    diagnostics.with_emit(&preflight_diagnostics, emit, work_counters);
                outcome.checked_source_files = checked_source_files;
                Ok(outcome)
            }
            None => {
                // An empty Program has no snapshot from which to construct a
                // checker resolver: the fail-closed unavailable projection,
                // as on the single-checker path.
                let diagnostics = emit_session_diagnostics(&prepared, &checked);
                let diagnostic_gate = diagnostics.gate();
                let work_counters = check_work_counters(&checked);
                emit_files(
                    &UnavailableEmitResolver,
                    &emit_host,
                    preflight.clone(),
                    selection,
                    &diagnostic_gate,
                    sink,
                )
                .map(|emit| {
                    let emit = finish_build_info(emit, &diagnostics, &mut recording);
                    diagnostics.with_emit(&preflight_diagnostics, emit, work_counters)
                })
                .map_err(DriverError::Emit)
            }
        };
        let outcome = outcome.map(|mut outcome| {
            outcome.build_emit = driver.build_emit_facts();
            if testing {
                outcome.program_report = driver.program_report();
            }
            outcome.watch_state = driver.take_watch_state();
            outcome
        });
        // The planner and the driver borrow the prepared program; both end
        // before the leak.
        drop(planner);
        drop(driver);
        if leak_program {
            std::mem::forget(prepared);
        }
        outcome
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
        self.run_no_emit_pass(true, LibraryPrefixCompletion::Complete, false)
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
        self.run_no_emit_pass(true, LibraryPrefixCompletion::FixtureObservedOnly, false)
    }

    /// The diagnostics TypeScript's native compiler runner records for one
    /// configuration (`compileFilesWithHost` in
    /// `tsc/internal/testutil/harnessutil`): config-file parsing, program
    /// (options), syntactic, semantic and global diagnostics of the whole
    /// Program without the command line's gating, the declaration diagnostics
    /// when the options emit declarations, and suggestions when requested. The
    /// union is [`NoEmitOutcome::native_harness_diagnostics`]; the other lists
    /// keep their command-line meaning. The standard-library bundle is built
    /// for this Program alone, as the command line builds it: the
    /// process-lifetime bundle cache would keep one bundle per library set
    /// and target of a long corpus run. The Program may be an emitting one;
    /// nothing is emitted.
    /// tsrs-native: harness execution mode; no tsc counterpart.
    #[doc(hidden)]
    pub fn run_for_native_harness(
        mut self,
        collection: NativeHarnessCollection,
    ) -> Result<NoEmitOutcome, DriverError> {
        self.native_harness = Some(collection);
        self.run_no_emit_pass(false, LibraryPrefixCompletion::Complete, false)
    }

    /// [`run_for_native_harness`](Self::run_for_native_harness), then `walk`
    /// over the checked session before it is released (the native runner's
    /// type and symbol baselines query the checker after the diagnostics).
    /// The walk needs the one checker of the comparison; a sharded budget
    /// skips it.
    /// tsrs-native: harness execution mode; no tsc counterpart.
    #[doc(hidden)]
    pub fn run_for_native_harness_with_walk(
        mut self,
        collection: NativeHarnessCollection,
        walk: &mut HarnessWalk<'_>,
    ) -> Result<NoEmitOutcome, DriverError> {
        self.native_harness = Some(collection);
        self.run_inner_after_emit(
            false,
            LibraryPrefixCompletion::Complete,
            false,
            None,
            Some(walk),
        )
        .map(|(outcome, _)| outcome)
    }

    /// The native compiler runner's second Program for one configuration
    /// (`compileFilesWithHost`: `program.Emit`, then the diagnostic getters).
    /// The Program emits into `sink` before any of its sources is checked,
    /// and the diagnostics of
    /// [`run_for_native_harness`](Self::run_for_native_harness) are then
    /// collected over the same checker: what the emit resolved through the
    /// checker stays resolved, so a diagnostic that depends on the order of
    /// resolution is reported where the emit first reached it. The second
    /// value is the emit's result.
    ///
    /// The session is handed back unconsumed (the inner `Err`) for a Program
    /// that does not emit before its diagnostics are known: one that cannot
    /// emit, one under `noEmitOnError` (`Emit` asks for every diagnostic
    /// first), and a sharded checker budget.
    /// tsrs-native: harness execution order; the command line checks first.
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    pub fn emit_then_run_for_native_harness(
        mut self,
        collection: NativeHarnessCollection,
        sink: &mut dyn OutputSink,
    ) -> Result<Result<(NoEmitOutcome, Option<Result<EmitOutcome, EmitFailure>>), Self>, DriverError>
    {
        if self.prepared.mode() != PreparedProgramMode::Emit
            || self.prepared.compiler_options().no_emit_on_error == Some(true)
            || self.checker_budget.is_sharded()
        {
            return Ok(Err(self));
        }
        self.native_harness = Some(collection);
        self.run_inner_after_emit(
            false,
            LibraryPrefixCompletion::Complete,
            false,
            Some(sink),
            None,
        )
        .map(Ok)
    }

    /// [`emit_then_run_for_native_harness`](Self::emit_then_run_for_native_harness),
    /// then `walk` over the checked session (see
    /// [`run_for_native_harness_with_walk`](Self::run_for_native_harness_with_walk)).
    /// tsrs-native: harness execution order; the command line checks first.
    #[doc(hidden)]
    #[allow(clippy::type_complexity)]
    pub fn emit_then_run_for_native_harness_with_walk(
        mut self,
        collection: NativeHarnessCollection,
        sink: &mut dyn OutputSink,
        walk: &mut HarnessWalk<'_>,
    ) -> Result<Result<(NoEmitOutcome, Option<Result<EmitOutcome, EmitFailure>>), Self>, DriverError>
    {
        if self.prepared.mode() != PreparedProgramMode::Emit
            || self.prepared.compiler_options().no_emit_on_error == Some(true)
            || self.checker_budget.is_sharded()
        {
            return Ok(Err(self));
        }
        self.native_harness = Some(collection);
        self.run_inner_after_emit(
            false,
            LibraryPrefixCompletion::Complete,
            false,
            Some(sink),
            Some(walk),
        )
        .map(Ok)
    }

    /// The output-path diagnostics an emitting Program reports before it
    /// writes anything (`verifyCompilerOptions`' emit blocking, such as
    /// TS5055), which the native runner collects with the options
    /// diagnostics; none for a Program that cannot emit, and none under
    /// `suppressOutputPathCheck`, which verifies no output path.
    /// tsc's emitFilesAndReportErrors (tsgo compiler/program.go
    /// GetDiagnosticsOfAnyProgram) asks for the options, global and semantic
    /// diagnostics only when the syntactic diagnostics are empty, so the
    /// command's check is closed by them: a Program with a parse error is
    /// neither bound nor checked. It asks for the semantic diagnostics only
    /// when the options and global diagnostics are empty too, so a Program
    /// whose options diagnostics (`available_options`, known before the
    /// check, or the command's own; see `command_options_diagnostics`) are
    /// not is bound and its checker initialized for the global diagnostics,
    /// and no source is checked. The native harness collects every
    /// diagnostic kind of the second Program and checks every source.
    fn syntactic_diagnostics_gate(
        &self,
        available_options: &[Diagnostic],
    ) -> SyntacticDiagnosticsGate {
        if self.native_harness.is_some() {
            SyntacticDiagnosticsGate::CheckEverySource
        } else if self.list_files_only {
            SyntacticDiagnosticsGate::SyntacticDiagnosticsOnly
        } else if available_options.is_empty() && !self.command_options_diagnostics {
            SyntacticDiagnosticsGate::CloseTheCheck
        } else {
            SyntacticDiagnosticsGate::GlobalDiagnosticsOnly
        }
    }

    /// The options diagnostics of the command's no-emit report, which tsc
    /// gathers as the Program's options diagnostics
    /// (`getOptionsDiagnostics`): the config's option rows, the programmatic
    /// option rows and the Program rows outside every source (the rows a
    /// source owns are its include-processor diagnostics, joined to its
    /// semantic diagnostics). Each public getter applies
    /// sortAndDeduplicateDiagnostics to its combined result.
    fn available_options_diagnostics(&self) -> Vec<Diagnostic> {
        let mut available_options = self.prepared.diagnostics().options().to_vec();
        available_options.extend(programmatic_option_diagnostics(&self.prepared));
        available_options.extend(program_rows_outside_sources(&self.prepared));
        sort_and_dedupe_diagnostics(&mut available_options);
        available_options
    }

    fn native_output_diagnostics(&self) -> Vec<Diagnostic> {
        if self.native_harness.is_none() || self.prepared.mode() != PreparedProgramMode::Emit {
            return Vec::new();
        }
        PreparedEmitHost::new_for_route(&self.prepared, self.emit_route, &self.source_api_facts)
            .ok()
            .filter(|host| validate_emit_request(host).is_ok())
            .and_then(|host| preflight_emit(&host, EmitSelection::WholeProgram).ok())
            .map(|preflight| preflight.diagnostics().to_vec())
            .unwrap_or_default()
    }

    /// `command_report` selects the --noEmit command's report: it adds the
    /// declaration getter when the options request declarations and skips
    /// the per-source aggregate that only the harnesses read
    /// ([`NoEmitOutcome::conformance_diagnostics`]).
    pub(crate) fn run_no_emit_pass(
        self,
        harness_lib_cache: bool,
        library_prefix: LibraryPrefixCompletion,
        command_report: bool,
    ) -> Result<NoEmitOutcome, DriverError> {
        self.run_inner(harness_lib_cache, library_prefix, command_report)
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
        command_report: bool,
    ) -> Result<NoEmitOutcome, DriverError> {
        self.run_inner_after_emit(
            harness_lib_cache,
            library_prefix,
            command_report,
            None,
            None,
        )
        .map(|(outcome, _)| outcome)
    }

    /// [`run_inner`](Self::run_inner), with `emit_first` in the order of the
    /// native compiler runner's second Program: the one checker emits into
    /// the sink before it checks a source. The second value is that emit's
    /// result, absent when nothing asked for it.
    fn run_inner_after_emit(
        self,
        harness_lib_cache: bool,
        library_prefix: LibraryPrefixCompletion,
        command_report: bool,
        emit_first: Option<&mut dyn OutputSink>,
        mut walk: Option<&mut HarnessWalk<'_>>,
    ) -> Result<(NoEmitOutcome, Option<Result<EmitOutcome, EmitFailure>>), DriverError> {
        let inputs = project_checker_inputs(&self.prepared, &self.source_api_facts)?;
        let has_roots = !self.prepared.roots().is_empty();
        let provider = PreparedModuleProvider {
            prepared: &self.prepared,
            request_plans: std::sync::Mutex::new(BTreeMap::new()),
            tracing: self.tracing.clone(),
        };
        // tsc emitFilesAndReportErrors (_tsc.js:129433-129440): the --noEmit
        // command's declaration getter, requested by `run_no_emit_command`
        // when the options ask for declarations.
        let native_harness = self.native_harness.is_some();
        let declaration_getter = !harness_lib_cache
            && !self.list_files_only
            && get_emit_declarations(self.prepared.compiler_options())
            && (native_harness
                || command_report && self.prepared.compiler_options().no_emit == Some(true));
        let mut declaration_diagnostics: Option<Result<DiagnosticList, DriverError>> = None;
        let mut first_emit: Option<Result<EmitOutcome, EmitFailure>> = None;
        // Program-construction diagnostics are part of tsc's combined
        // diagnostic map. File-less rows and rows owned by config or other
        // auxiliary files feed getOptionsDiagnostics; rows owned by a
        // program SourceFile feed that source's getSemanticDiagnostics. The
        // options rows are known before the check and gate it.
        let available_options = self.available_options_diagnostics();
        let syntactic_diagnostics_gate = self.syntactic_diagnostics_gate(&available_options);
        let incremental_facts_requested = self.command_build_info
            && (tsc_incremental::options::is_incremental(self.prepared.compiler_options())
                || self.build_mode
                || self.watch);
        let driver = incremental::IncrementalDriver::new(
            &self.prepared,
            self.old_build_info.clone(),
            self.emit_route,
            &self.source_api_facts,
            incremental::AffectedPolicy {
                emits: false,
                options_diagnostics: !available_options.is_empty()
                    || self.command_options_diagnostics,
                build: self.build_mode,
                hash_with_text: self.testing,
                watch: self.watch,
            },
        );
        let mut planner = driver.planner();
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
                syntactic_diagnostics_gate,
            )
        } else if let Some(sink) = emit_first {
            // tsgo's test harness, `compileFilesWithHost`
            // (testutil/harnessutil/harnessutil.go:673-688): the second
            // Program emits and then asks for the diagnostics, over one
            // checker. The operation's first call is that emit, over the
            // unchecked session; its second is the declaration getter.
            let emit_host = PreparedEmitHost::new_for_route(
                &self.prepared,
                self.emit_route,
                &self.source_api_facts,
            )?;
            let mut preflight = match validate_emit_request(&emit_host) {
                Ok(()) => Some(
                    preflight_emit(&emit_host, EmitSelection::WholeProgram)
                        .map_err(DriverError::Emit)?,
                ),
                Err(failure) => {
                    first_emit = Some(Err(failure));
                    None
                }
            };
            let mut unchecked = true;
            let mut operation = |snapshot: &ProgramSnapshot,
                                 session: &CheckerSession<'_>,
                                 checked: &CheckResult| {
                if std::mem::take(&mut unchecked) {
                    if let Some(preflight) = preflight.take() {
                        let gate = emit_session_diagnostics(&self.prepared, checked).gate();
                        let host = CheckedEmitHost {
                            prepared: &emit_host,
                            snapshot,
                            prepared_sources: None,
                        };
                        first_emit = Some(session.with_emit_resolver(|resolver| {
                            emit_files(
                                resolver,
                                &host,
                                preflight,
                                EmitSelection::WholeProgram,
                                &gate,
                                &mut *sink,
                            )
                        }));
                    }
                    return;
                }
                if declaration_getter {
                    let every_file = (0..snapshot.documents().len()).collect::<Vec<_>>();
                    declaration_diagnostics = Some(no_emit_declaration_diagnostics(
                        &self.prepared,
                        self.emit_route,
                        &self.source_api_facts,
                        snapshot,
                        std::slice::from_ref(session),
                        std::slice::from_ref(&every_file),
                        self.worker_budget,
                        0,
                        false,
                        driver.declaration_file_filter().as_deref(),
                    ));
                }
                if let Some(walk) = walk.as_deref_mut() {
                    walk(snapshot, session);
                }
            };
            check_program_with_authoritative_modules_at_emit_first_with_workers(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                self.worker_budget,
                &mut operation,
            )
        } else if (declaration_getter || walk.is_some()) && !self.checker_budget.is_sharded() {
            // One checker: the getter runs in the checked-session callback
            // over that session, as the shard gate does per shard below;
            // the harness walk runs there too.
            let mut operation = |snapshot: &ProgramSnapshot,
                                 session: &CheckerSession<'_>,
                                 checked: &CheckResult| {
                if declaration_getter
                    && (native_harness
                        || (no_emit_report_is_clean(&self.prepared, checked)
                            && driver.cached_semantic_diagnostics().is_empty()))
                {
                    let started = std::time::Instant::now();
                    let every_file = (0..snapshot.documents().len()).collect::<Vec<_>>();
                    declaration_diagnostics = Some(no_emit_declaration_diagnostics(
                        &self.prepared,
                        self.emit_route,
                        &self.source_api_facts,
                        snapshot,
                        std::slice::from_ref(session),
                        std::slice::from_ref(&every_file),
                        self.worker_budget,
                        0,
                        false,
                        driver.declaration_file_filter().as_deref(),
                    ));
                    tsc_types::trace::mark(
                        "checker: declaration diagnostics (one checker)",
                        started,
                    );
                }
                // tsgo's harness walks the types and symbols after every
                // diagnostic getter has run.
                if let Some(walk) = walk.as_deref_mut() {
                    walk(snapshot, session);
                }
            };
            check_program_with_authoritative_modules_at_for_emit_with_workers(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                self.worker_budget,
                IncrementalRequest {
                    facts: incremental_facts_requested,
                    planner: incremental_facts_requested
                        .then_some(&mut planner as &mut IncrementalPlanner<'_>),
                },
                &mut operation,
            )
        } else if declaration_getter {
            // The getter runs through the shard gate, while every shard's
            // checker session is alive; the gate admits nothing to emit.
            // Each shard's share is computed on the shard's own thread as
            // soon as it has checked; the gate joins those shares when it
            // admits, and computes the getter itself if any share is
            // missing (the getter over one shard is the same job).
            let eager_shares: std::sync::Mutex<Vec<Option<Result<DiagnosticList, DriverError>>>> =
                std::sync::Mutex::new(Vec::new());
            let eager = |shard: usize,
                         snapshot: &ProgramSnapshot,
                         session: &CheckerSession<'_>,
                         files: &[usize],
                         _checking: &std::sync::atomic::AtomicUsize| {
                let share = no_emit_declaration_diagnostics(
                    &self.prepared,
                    self.emit_route,
                    &self.source_api_facts,
                    snapshot,
                    std::slice::from_ref(session),
                    std::slice::from_ref(&files.to_vec()),
                    self.worker_budget,
                    shard,
                    true,
                    driver.declaration_file_filter().as_deref(),
                );
                let mut shares = eager_shares
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if shares.len() <= shard {
                    shares.resize_with(shard + 1, || None);
                }
                shares[shard] = Some(share);
            };
            let mut gate = |snapshot: &ProgramSnapshot,
                            checked: &CheckResult,
                            sessions: &[CheckerSession<'_>],
                            files_by_shard: &[Vec<usize>]|
             -> bool {
                if native_harness
                    || (no_emit_report_is_clean(&self.prepared, checked)
                        && driver.cached_semantic_diagnostics().is_empty())
                {
                    let started = std::time::Instant::now();
                    let mut shares = eager_shares
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let mut joined = Some(Ok(Vec::new()));
                    for shard in 0..sessions.len() {
                        match (shares.get_mut(shard).and_then(Option::take), &mut joined) {
                            (Some(Ok(share)), Some(Ok(diagnostics))) => diagnostics.extend(share),
                            (Some(Err(error)), _) => joined = Some(Err(error)),
                            (None, _) => {
                                joined = None;
                                break;
                            }
                            (Some(Ok(_)), _) => {}
                        }
                    }
                    declaration_diagnostics = Some(match joined {
                        Some(Ok(mut diagnostics)) => {
                            sort_and_dedupe_diagnostics(&mut diagnostics);
                            Ok(diagnostics)
                        }
                        Some(Err(error)) => Err(error),
                        None => no_emit_declaration_diagnostics(
                            &self.prepared,
                            self.emit_route,
                            &self.source_api_facts,
                            snapshot,
                            sessions,
                            files_by_shard,
                            self.worker_budget,
                            0,
                            false,
                            driver.declaration_file_filter().as_deref(),
                        ),
                    });
                    tsc_types::trace::mark("checker: declaration diagnostics (join)", started);
                }
                false
            };
            let emit = |_: &ProgramSnapshot,
                        _: &[CheckerSession<'_>],
                        _: &[Vec<usize>]|
             -> Result<Vec<ShardEmission>, UnitEmitError> { Ok(Vec::new()) };
            let mut sharded_emit = ShardedEmit {
                gate: &mut gate,
                emit: &emit,
                emissions: None,
                eager: Some(&eager),
                prelude: None,
            };
            check_program_with_authoritative_modules_at_for_emit_with_checkers(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &PreparedProviderFactory {
                    prepared: &self.prepared,
                    tracing: self.tracing.clone(),
                },
                self.worker_budget,
                self.checker_budget,
                &mut sharded_emit,
                IncrementalRequest {
                    facts: incremental_facts_requested,
                    planner: incremental_facts_requested
                        .then_some(&mut planner as &mut IncrementalPlanner<'_>),
                },
            )
        } else if self.checker_budget.is_sharded() {
            check_program_with_authoritative_modules_at_with_checkers_gated(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &PreparedProviderFactory {
                    prepared: &self.prepared,
                    tracing: self.tracing.clone(),
                },
                self.worker_budget,
                self.checker_budget,
                syntactic_diagnostics_gate,
                IncrementalRequest {
                    facts: incremental_facts_requested,
                    planner: incremental_facts_requested
                        .then_some(&mut planner as &mut IncrementalPlanner<'_>),
                },
            )
        } else {
            check_program_with_authoritative_modules_at_with_workers_gated(
                &inputs.libs,
                &inputs.files,
                &inputs.lib_metadata,
                &inputs.file_metadata,
                self.prepared.compiler_options(),
                &inputs.current_directory,
                &provider,
                self.worker_budget,
                syntactic_diagnostics_gate,
                IncrementalRequest {
                    facts: incremental_facts_requested,
                    planner: incremental_facts_requested
                        .then_some(&mut planner as &mut IncrementalPlanner<'_>),
                },
            )
        }
        .map_err(|failure| map_authoritative_failure(&self.prepared, failure))?;
        let mut checked = checked;
        let incremental_facts = checked.incremental.take();
        let declaration_requested = declaration_diagnostics.is_some();
        let mut declaration_diagnostics = declaration_diagnostics.transpose()?.unwrap_or_default();
        let checker_work = checked.work_counters;
        let work_counters = NoEmitWorkCounters {
            parsed_documents: checker_work.parsed_documents(),
            adopted_documents: checker_work.adopted_documents(),
            bound_documents: checker_work.bound_documents(),
            full_text_copies: checker_work.full_text_copies(),
            full_text_bytes_copied: checker_work.full_text_bytes_copied(),
            checker_shards: checker_work.checker_shards(),
            checker_threads: checker_work.checker_threads(),
            checker_serial_replay: checker_work.checker_serial_replay(),
            checker_replay_reasons: checker_work.checker_replay_reasons(),
        };

        let preparation = self.prepared.diagnostics();
        let conformance_diagnostics = if command_report {
            Vec::new()
        } else {
            checked.diagnostics
        };
        let config_diagnostics = preparation.config().to_vec();
        let mut syntactic_diagnostics = checked.syntactic_diagnostics;
        sort_and_dedupe_diagnostics(&mut syntactic_diagnostics);
        let partial_checks = checked.partial_checks;
        // tsgo emitBuildInfo of the --noEmit command: the incremental
        // program's facts, the rows it cached (only when the command asked
        // for the semantic diagnostics) and the declaration diagnostics it
        // asked for (only when nothing else was reported).
        let mut available_semantic = checked
            .program_semantic_diagnostics
            .expect("authoritative checker sessions publish whole-Program semantic diagnostics");
        let build_info = incremental_facts.as_ref().and_then(|facts| {
            let command = incremental::CommandDiagnosticFacts {
                config: !config_diagnostics.is_empty(),
                syntactic: !syntactic_diagnostics.is_empty(),
                options: !available_options.is_empty() || self.command_options_diagnostics,
                global: has_roots && !checked.global_diagnostics.is_empty(),
            };
            driver.store_check(
                facts,
                command.semantic_cached()
                    && self.prepared.compiler_options().no_check != Some(true),
            );
            // The rows of the files the check did not cover come from the
            // old state (tsgo getSemanticDiagnosticsOfFile from the cache).
            available_semantic.extend(driver.cached_semantic_diagnostics());
            if declaration_requested && !self.command_options_diagnostics {
                let from_cache = driver.record_declaration_diagnostics(&declaration_diagnostics);
                if !from_cache.is_empty() {
                    declaration_diagnostics.extend(from_cache);
                    sort_and_dedupe_diagnostics(&mut declaration_diagnostics);
                }
            }
            driver.build_info(command)
        });
        let program_report = self.testing.then(|| driver.program_report()).flatten();
        let watch_state = driver.take_watch_state();
        sort_and_dedupe_diagnostics(&mut available_semantic);

        // emitFilesAndReportErrors compares the aggregate length with the
        // original config-diagnostic length. Config errors therefore remain
        // visible but do not themselves close any of the later gates.
        let native_harness_diagnostics = match self.native_harness {
            None => Vec::new(),
            Some(NativeHarnessCollection {
                capture_suggestions,
                ..
            }) => {
                if let Some(partial) = partial_checks.first() {
                    return Err(DriverError::IncompleteCheck {
                        file_name: partial.file_name.clone(),
                        start: partial.start,
                        length: partial.length,
                        reason: partial.reason.clone(),
                        additional_partial_checks: partial_checks.len().saturating_sub(1),
                    });
                }
                let mut all = config_diagnostics.clone();
                all.extend(available_options.iter().cloned());
                all.extend(self.native_output_diagnostics());
                all.extend(syntactic_diagnostics.iter().cloned());
                all.extend(available_semantic.iter().cloned());
                if has_roots {
                    all.extend(checked.global_diagnostics.iter().cloned());
                }
                all.extend(declaration_diagnostics.iter().cloned());
                if capture_suggestions {
                    all.extend(checked.suggestion_diagnostics.iter().cloned());
                }
                sort_and_dedupe_diagnostics(&mut all);
                all
            }
        };

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
        drop(planner);
        drop(driver);
        if self.leak_program {
            std::mem::forget(self);
        }
        Ok((
            NoEmitOutcome {
                config_diagnostics,
                syntactic_diagnostics,
                options_diagnostics,
                global_diagnostics,
                semantic_diagnostics,
                declaration_diagnostics,
                conformance_diagnostics,
                native_harness_diagnostics,
                work_counters,
                build_info,
                program_report,
                watch_state,
            },
            first_emit,
        ))
    }
}

/// A harness query over the checked session (the native runner's type and
/// symbol baselines), run after every diagnostic was collected.
/// tsrs-native: harness callback; no tsc counterpart.
pub type HarnessWalk<'a> = dyn FnMut(&ProgramSnapshot, &CheckerSession<'_>) + 'a;

/// What [`ProgramSession::run_for_native_harness`] collects besides the
/// Program's config, options, syntactic, semantic, global and declaration
/// diagnostics.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeHarnessCollection {
    /// The suggestion diagnostics (`@captureSuggestions`).
    pub capture_suggestions: bool,
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
    /// `program.getDeclarationDiagnostics()` of the --noEmit command
    /// ([`ProgramSession::run_no_emit_command`]): empty for `run()` and
    /// whenever the command's gate is closed.
    declaration_diagnostics: DiagnosticList,
    // The differential conformance harness compares the aggregate of public
    // per-file getters, including suggestions. diagnostics()/into_diagnostics
    // exclude it, and the command report does not build it.
    conformance_diagnostics: DiagnosticList,
    // The native compiler runner's ungated union
    // (ProgramSession::run_for_native_harness); empty for every other run.
    native_harness_diagnostics: DiagnosticList,
    // Operational evidence is not part of diagnostic-result equality. Tests
    // and qualification compare it explicitly through work_counters().
    work_counters: NoEmitWorkCounters,
    /// The build info of an incremental program (tsgo `emitBuildInfo`, which
    /// a `--noEmit` command still writes): the command writes it.
    build_info: Option<BuildInfoDocument>,
    /// The incremental program's files for tsgo's test harness (a testing
    /// session only).
    program_report: Option<Vec<incremental::ProgramFileReport>>,
    /// A watch run's state for its next cycle.
    watch_state: Option<incremental::WatchState>,
}

impl PartialEq for NoEmitOutcome {
    fn eq(&self, other: &Self) -> bool {
        self.config_diagnostics == other.config_diagnostics
            && self.syntactic_diagnostics == other.syntactic_diagnostics
            && self.options_diagnostics == other.options_diagnostics
            && self.global_diagnostics == other.global_diagnostics
            && self.semantic_diagnostics == other.semantic_diagnostics
            && self.declaration_diagnostics == other.declaration_diagnostics
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
    adopted_documents: u64,
    bound_documents: u64,
    full_text_copies: u64,
    full_text_bytes_copied: u64,
    checker_shards: u64,
    checker_threads: u64,
    checker_serial_replay: u64,
    checker_replay_reasons: u64,
}

impl NoEmitWorkCounters {
    /// Documents the checker session parsed itself.
    pub const fn parsed_documents(self) -> u64 {
        self.parsed_documents
    }

    /// Documents whose loader-planned syntax tree the checker session adopted
    /// instead of parsing (see `CheckWorkCounters::adopted_documents`).
    pub const fn adopted_documents(self) -> u64 {
        self.adopted_documents
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

    /// Checker states the session constructed (see
    /// `CheckWorkCounters::checker_shards`): 1 for the serial checker, the
    /// effective shard count for a sharded budget.
    pub const fn checker_shards(self) -> u64 {
        self.checker_shards
    }

    /// Distinct threads that ran those checker states.
    pub const fn checker_threads(self) -> u64 {
        self.checker_threads
    }

    /// 1 when the sharded check was replayed serially because the
    /// order-sensitivity guard fired (see `CheckWorkCounters`).
    pub const fn checker_serial_replay(self) -> u64 {
        self.checker_serial_replay
    }

    /// The guard's reason bits (0 when nothing fired).
    pub const fn checker_replay_reasons(self) -> u64 {
        self.checker_replay_reasons
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

    /// The --noEmit command's declaration diagnostics
    /// ([`ProgramSession::run_no_emit_command`]); empty otherwise.
    pub fn declaration_diagnostics(&self) -> &[Diagnostic] {
        &self.declaration_diagnostics
    }

    /// The build info document an incremental program's `--noEmit` command
    /// writes (tsgo `emitBuildInfo`); `None` for other programs.
    pub fn build_info(&self) -> Option<&BuildInfoDocument> {
        self.build_info.as_ref()
    }

    /// The incremental program's files for tsgo's test harness (a testing
    /// session only).
    pub fn program_report(&self) -> Option<&[incremental::ProgramFileReport]> {
        self.program_report.as_deref()
    }

    /// A watch run's state for its next cycle.
    pub(crate) fn take_watch_state(&mut self) -> Option<incremental::WatchState> {
        self.watch_state.take()
    }

    /// Aggregate public-getter stream used only by differential conformance.
    /// It includes suggestions and is therefore not CLI output; the
    /// --noEmit command's report ([`ProgramSession::run_no_emit_command`])
    /// leaves it empty.
    pub fn conformance_diagnostics(&self) -> &[Diagnostic] {
        &self.conformance_diagnostics
    }

    /// The native compiler runner's ungated union
    /// ([`ProgramSession::run_for_native_harness`]); empty otherwise.
    #[doc(hidden)]
    pub fn native_harness_diagnostics(&self) -> &[Diagnostic] {
        &self.native_harness_diagnostics
    }

    /// Parse/bind/full-text-copy evidence for this consumed session.
    pub const fn work_counters(&self) -> NoEmitWorkCounters {
        self.work_counters
    }

    /// Iterate in the no-emit command's bucket order.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.config_diagnostics
            .iter()
            .chain(&self.syntactic_diagnostics)
            .chain(&self.options_diagnostics)
            .chain(&self.global_diagnostics)
            .chain(&self.semantic_diagnostics)
            .chain(&self.declaration_diagnostics)
    }

    /// Consume the outcome and flatten it in no-emit command bucket order.
    pub fn into_diagnostics(self) -> DiagnosticList {
        let capacity = self.config_diagnostics.len()
            + self.syntactic_diagnostics.len()
            + self.options_diagnostics.len()
            + self.global_diagnostics.len()
            + self.semantic_diagnostics.len()
            + self.declaration_diagnostics.len();
        let mut diagnostics = Vec::with_capacity(capacity);
        diagnostics.extend(self.config_diagnostics);
        diagnostics.extend(self.syntactic_diagnostics);
        diagnostics.extend(self.options_diagnostics);
        diagnostics.extend(self.global_diagnostics);
        diagnostics.extend(self.semantic_diagnostics);
        diagnostics.extend(self.declaration_diagnostics);
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
        path: JsString,
    },
    IncompleteCheck {
        file_name: JsString,
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
                path.to_string_lossy()
            ),
            Self::IncompleteCheck {
                file_name,
                start,
                length,
                reason,
                additional_partial_checks,
            } => write!(
                formatter,
                "no-emit check was incomplete at {}:{start}+{length}: {reason} ({additional_partial_checks} additional partial checks)", file_name.to_string_lossy(),
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
    current_directory: JsString,
}

fn project_checker_inputs(
    prepared: &PreparedProgram,
    source_api_facts: &BTreeMap<SourceFileId, SourceApiFacts>,
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
        let (input, metadata) =
            project_source(source, source_file, source_api_facts.get(&source_file))?;
        libs.push(input);
        lib_metadata.push(metadata);
    }

    let mut files = Vec::with_capacity(sources.len().saturating_sub(library_ids.len()));
    let mut file_metadata = Vec::with_capacity(files.capacity());
    for source in sources.iter().skip(library_ids.len()) {
        let source_file = prepared
            .source_id(source.path().canonical())
            .ok_or_else(|| DriverError::MissingPreparedSourceIdentity {
                path: source.path().display().to_owned(),
            })?;
        let (input, metadata) =
            project_source(source, source_file, source_api_facts.get(&source_file))?;
        files.push(input);
        file_metadata.push(metadata);
    }

    for package in prepared.packages() {
        let display_path = package.package_json().display();
        let name = display_path.to_owned();
        files.push(InputFile::host_only_from_snapshot(
            name,
            Arc::clone(package.snapshot()),
        ));
    }

    let current_directory_path = prepared.current_directory().display();
    let current_directory = current_directory_path.to_owned();
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
    facts: Option<&SourceApiFacts>,
) -> Result<(InputFile, AuthoritativeSourceMetadata), DriverError> {
    let display_path = source.path().display();
    // transpileWorker: `sourceFile.fileName` is the caller's spelling even
    // though the host resolves it at its own root.
    let name = facts
        .and_then(|facts| facts.file_name.clone())
        .unwrap_or_else(|| display_path.to_owned());
    let metadata = AuthoritativeSourceMetadata {
        token: AuthoritativeSourceToken(source_file.raw()),
        file_name: name.clone(),
        may_be_emitted: source.may_be_emitted(),
        implied_node_format: source.implied_node_format().map(checker_resolution_mode),
        implied_node_format_for_emit: source
            .implied_node_format_for_emit()
            .map(checker_resolution_mode),
        project_reference: source.project_reference().cloned(),
    };
    // The planning parse used the process default (or the API fact); the
    // checker adopts it only when its own expectation matches.
    let mut input = InputFile::from_snapshot(name, Arc::clone(source.snapshot()))
        .with_preparsed_syntax(source.preparsed_syntax().clone())
        .with_js_doc_parsing_mode(Some(tsc_program::default_js_doc_parsing_mode()));
    if let Some(facts) = facts {
        input = input
            .with_module_name(facts.module_name.clone())
            .with_renamed_dependencies(facts.renamed_dependencies.clone());
        if facts.js_doc_parsing_mode.is_some() {
            input = input.with_js_doc_parsing_mode(facts.js_doc_parsing_mode);
        }
    }
    Ok((input, metadata))
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
) -> ProgramDiagnostics {
    let preparation = prepared.diagnostics();
    let mut options = preparation.options().to_vec();
    options.extend(programmatic_option_diagnostics(prepared));
    // The rows located in a source joined its semantic diagnostics in the
    // checker (include-processor diagnostics); the rest are options rows.
    options.extend(program_rows_outside_sources(prepared));
    let mut semantic = checked
        .program_semantic_diagnostics
        .as_ref()
        .expect("authoritative checker sessions publish whole-Program semantic diagnostics")
        .clone();
    sort_and_dedupe_diagnostics(&mut options);
    sort_and_dedupe_diagnostics(&mut semantic);
    let mut syntactic = checked.syntactic_diagnostics.clone();
    sort_and_dedupe_diagnostics(&mut syntactic);
    let global = if prepared.roots().is_empty() {
        Vec::new()
    } else {
        checked.global_diagnostics.clone()
    };

    ProgramDiagnostics {
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
/// tsgo-port: verifyCompilerOptions "Removed in TS7" @7.1 (program.go:976-1033).
/// `ignoreDeprecations` is parsed but has no effect in 7.1.
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
    }

    // Removed in TS7. `ignoreDeprecations` is parsed but has no effect.
    if let Some(base_url) = options.base_url.as_ref() {
        // The suggestion exists only with a config file: the base URL
        // relative to that file.
        let use_instead = prepared.program_options().config_file().map(|config_file| {
            removed_base_url_paths_suggestion(
                config_file.path().display(),
                base_url.as_js(),
                prepared.current_directory().display(),
                prepared.path_context().use_case_sensitive_file_names(),
            )
        });
        push_programmatic_removed_option_name(
            prepared,
            &mut diagnostics,
            "baseUrl",
            use_instead.as_ref().map(JsString::as_js),
        );
    }
    if options.out_file.is_some() {
        push_programmatic_removed_option_name(prepared, &mut diagnostics, "outFile", None);
    }
    if options.target == Some(1) {
        push_programmatic_removed_option_value(prepared, &mut diagnostics, "target", "ES5");
    }
    let module_name = match options.module {
        Some(2) => Some("AMD"),
        Some(4) => Some("System"),
        Some(3) => Some("UMD"),
        _ => None,
    };
    if let Some(module_name) = module_name {
        push_programmatic_removed_option_value(prepared, &mut diagnostics, "module", module_name);
    }
    if options.module_resolution == Some(1) {
        push_programmatic_removed_option_value(
            prepared,
            &mut diagnostics,
            "moduleResolution",
            "Classic",
        );
    }
    for (removed, name) in [
        (options.always_strict == Some(false), "alwaysStrict"),
        (options.es_module_interop == Some(false), "esModuleInterop"),
        (
            options.allow_synthetic_default_imports == Some(false),
            "allowSyntheticDefaultImports",
        ),
    ] {
        if removed {
            push_programmatic_removed_option_value(prepared, &mut diagnostics, name, "false");
        }
    }
    if options.module_resolution == Some(2) {
        push_programmatic_removed_option_value(
            prepared,
            &mut diagnostics,
            "moduleResolution",
            "node10",
        );
    }
    if options.downlevel_iteration.is_some() {
        push_programmatic_removed_option_name(
            prepared,
            &mut diagnostics,
            "downlevelIteration",
            None,
        );
    }
    // tsgo verifyCompilerOptions (compiler/program.go:1068-1070).
    if options.ts_build_info_file.is_none()
        && options.incremental == Some(true)
        && prepared.program_options().config_file_path().is_none()
    {
        diagnostics.push(Diagnostic::new(
            None,
            None,
            None,
            MessageChain::new(
                &gen::Option_incremental_is_only_valid_with_a_known_configuration_file_like_tsconfig_json_or_when_tsBuildInfoFile_is_explicitly_provided,
                &[],
            ),
        ));
    }
    if !external_config_option_diagnostics {
        diagnostics.extend(validate_paths_option_diagnostics(
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

/// tsgo-port: createDiagnosticForOption and
/// createOptionDiagnosticInObjectLiteralSyntax @7.1 (program.go:893-921):
/// the first property, in document order, that one of `names` matches (a
/// second name or a duplicate key adds no row), else the `compilerOptions`
/// name, else a fileless diagnostic.
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
            .any(|name| !config_file.compiler_option_name_locations(*name).is_empty())
    {
        return;
    }

    let mut locations = names
        .iter()
        .flat_map(|name| match location {
            ProgrammaticOptionDiagnosticLocation::Name => {
                config_file.compiler_option_name_locations(*name)
            }
            ProgrammaticOptionDiagnosticLocation::Value => {
                config_file.compiler_option_value_locations(*name)
            }
        })
        .copied()
        .collect::<Vec<_>>();
    locations.sort_unstable_by_key(|location| location.start());
    locations.truncate(1);
    if locations.is_empty() && use_compiler_options_fallback {
        locations.extend(config_file.compiler_options_location());
    }
    if locations.is_empty() {
        diagnostics.push(Diagnostic::new(None, None, None, message));
        return;
    }

    let file_name = config_file.diagnostic_file_name().to_owned();
    diagnostics.extend(locations.into_iter().map(|location| {
        Diagnostic::new_js(
            Some(file_name.clone()),
            Some(location.start()),
            Some(location.length()),
            message.clone(),
        )
    }));
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
    use_instead: Option<JsStr<'_>>,
) {
    let mut message = MessageChain::new(
        &gen::Option_0_has_been_removed_Please_remove_it_from_your_configuration,
        &[name.to_owned()],
    );
    if let Some(use_instead) = use_instead {
        message = message.with_next(vec![MessageChain::new_js(
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

/// tsgo `emitBuildInfo`'s span: the write of an incremental program's build
/// info.
pub(crate) fn trace_build_info(tracing: Option<&Arc<Tracing>>) -> Option<tsc_types::tracing::Span> {
    tracing.map(|tracing| tracing.begin(TracePhase::Emit, "emitBuildInfo", TraceArgs::new()))
}

fn check_work_counters(checked: &CheckResult) -> NoEmitWorkCounters {
    NoEmitWorkCounters {
        parsed_documents: checked.work_counters.parsed_documents(),
        adopted_documents: checked.work_counters.adopted_documents(),
        bound_documents: checked.work_counters.bound_documents(),
        full_text_copies: checked.work_counters.full_text_copies(),
        full_text_bytes_copied: checked.work_counters.full_text_bytes_copied(),
        checker_shards: checked.work_counters.checker_shards(),
        checker_threads: checked.work_counters.checker_threads(),
        checker_serial_replay: checked.work_counters.checker_serial_replay(),
        checker_replay_reasons: checked.work_counters.checker_replay_reasons(),
    }
}

/// The Program's own rows: the loader's rows and the resolution diagnostics.
fn program_rows(prepared: &PreparedProgram) -> impl Iterator<Item = &Diagnostic> {
    prepared
        .diagnostics()
        .program()
        .iter()
        .chain(
            prepared
                .resolutions()
                .type_references()
                .flat_map(|(_, resolution)| resolution.diagnostics()),
        )
        .chain(
            prepared
                .resolutions()
                .modules()
                .flat_map(|(_, resolution)| resolution.diagnostics()),
        )
}

/// tsgo's include-processor diagnostics: the Program rows located in a
/// prepared source, with that source's id. The checker joins each to the
/// source's semantic diagnostics unless the source skips type checking or a
/// comment directive precedes the row (GetIncludeProcessorDiagnostics,
/// compiler/program.go:840-846).
fn include_processor_rows(prepared: &PreparedProgram) -> Vec<(SourceFileId, Diagnostic)> {
    program_rows(prepared)
        .filter_map(|diagnostic| {
            let file_name = diagnostic.file_name.as_ref()?;
            let source = prepared_source_of_diagnostic(prepared, file_name.as_js())?;
            Some((source, diagnostic.clone()))
        })
        .collect()
}

/// The Program rows outside every prepared source: file-less rows and rows
/// of the config or other auxiliary files, which are options diagnostics.
fn program_rows_outside_sources(prepared: &PreparedProgram) -> Vec<Diagnostic> {
    program_rows(prepared)
        .filter(|diagnostic| {
            diagnostic.file_name.as_ref().is_none_or(|file_name| {
                prepared_source_of_diagnostic(prepared, file_name.as_js()).is_none()
            })
        })
        .cloned()
        .collect()
}

/// The prepared source whose display, canonical, alternate or real path
/// names `file_name`.
fn prepared_source_of_diagnostic(
    prepared: &PreparedProgram,
    file_name: JsStr<'_>,
) -> Option<SourceFileId> {
    let normalized = normalize_source_slashes(file_name);
    let names_equal = |candidate: JsStr<'_>| {
        candidate == file_name || normalize_source_slashes(candidate) == normalized
    };
    let source = prepared.source_files().iter().find(|source| {
        names_equal(source.path().display())
            || names_equal(source.path().canonical().as_js())
            || source
                .alternate_display_paths()
                .iter()
                .any(|path| names_equal(path.as_js()))
            || source.real_path().is_some_and(|path| {
                names_equal(path.display()) || names_equal(path.canonical().as_js())
            })
    })?;
    prepared.source_id(source.path().canonical())
}

fn normalize_source_slashes(path: JsStr<'_>) -> JsString {
    let mut result = JsString::with_capacity(path.as_bytes().len());
    for unit in path.code_units() {
        result.push_code_unit(if unit == b'\\' as u16 {
            b'/' as u16
        } else {
            unit
        });
    }
    result
}

#[cfg(test)]
#[path = "../tests/unit/lib/tests.rs"]
mod tests;
