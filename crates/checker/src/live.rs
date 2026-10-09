//! One Program kept alive with its checker (tsgo's `compiler.Program` with
//! the API checker of its pool): the sources are parsed and bound once, the
//! checker is initialized without checking a source, and each file is
//! checked when its diagnostics are first asked for. The query entry hands
//! the initialized checker to the caller.
//!
//! The batch drivers expose the checker only to a scoped callback, because
//! the checker borrows the Program snapshot, the options and the module
//! provider. [`LiveChecker`] owns those values and the checker that borrows
//! them together, so a project system can keep a Program between requests.
//! It uses no thread of its own.

use std::sync::Arc;

use self_cell::self_cell;
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, DiagnosticList};
use tsc_types::{IdentityDomain, JsStr, JsString};

use crate::program::{EphemeralDocumentStore, ProgramFileFacts, ProgramFileId, ProgramSnapshot};
use crate::state::CheckerState;
use crate::{
    bind_sources_in_program_order, check_program_file, globals, init_checker_state, lib_bundle,
    parse_program_inputs, program_file_id, reserve_type_tables,
    semantic_diagnostics_for_program_file, snapshot_node_count, syntactic_file_rows,
    validate_authoritative_metadata, AuthoritativeModuleFailure, AuthoritativeModuleProvider,
    AuthoritativeProviderSource, AuthoritativeRun, AuthoritativeSourceMetadata, CheckWorkCounters,
    CompilerOptions, DiagnosticSchedule, FileDiagnosticPasses, HostFacts, InputFile,
    LibraryPrefixCompletion, ParsedProgramInputs, WorkerBudget,
};

/// The owned inputs of a [`LiveChecker`]: the Program's library and source
/// files with their authoritative metadata, as the batch drivers take them.
pub struct LiveCheckerInputs {
    pub libs: Vec<InputFile>,
    pub files: Vec<InputFile>,
    pub lib_metadata: Vec<AuthoritativeSourceMetadata>,
    pub file_metadata: Vec<AuthoritativeSourceMetadata>,
    pub options: CompilerOptions,
    pub current_directory: JsString,
}

/// What the checker borrows, owned.
struct LiveOwner {
    snapshot: ProgramSnapshot,
    options: CompilerOptions,
    provider: Box<dyn AuthoritativeModuleProvider + Send>,
    metadata: Vec<AuthoritativeSourceMetadata>,
    host: HostFacts,
    lib_count: usize,
    /// The syntactic rows of every Program source (after the libraries).
    passes: Vec<FileDiagnosticPasses>,
    /// The Program's preparation diagnostics.
    program_diagnostics: Vec<Diagnostic>,
}

/// The checker and the bookkeeping of the files it has checked.
struct LiveState<'a> {
    state: CheckerState<'a>,
    checked: Vec<bool>,
    /// The global rows each file's check published (getDiagnosticsWorker's
    /// attribution).
    globals_by_file: Vec<Vec<Diagnostic>>,
}

self_cell!(
    struct LiveCell {
        owner: LiveOwner,

        #[not_covariant]
        dependent: LiveState,
    }
);

/// One Program and its checker, owned together.
pub struct LiveChecker {
    cell: LiveCell,
}

impl LiveChecker {
    /// Parse and bind the Program's sources and initialize the checker
    /// (globals merged, no source checked): the on-demand schedule of the
    /// batch drivers, kept instead of handed to a callback.
    pub fn new(
        inputs: LiveCheckerInputs,
        provider: Box<dyn AuthoritativeModuleProvider + Send>,
        workers: WorkerBudget,
    ) -> Result<Self, AuthoritativeModuleFailure> {
        let LiveCheckerInputs {
            libs,
            files,
            lib_metadata,
            file_metadata,
            options,
            current_directory,
        } = inputs;
        validate_authoritative_metadata(&libs, &lib_metadata, "library")?;
        validate_authoritative_metadata(&files, &file_metadata, "program")?;
        let mut seen_tokens = rustc_hash::FxHashSet::default();
        for source in lib_metadata.iter().chain(&file_metadata) {
            if !seen_tokens.insert(source.token) {
                return Err(AuthoritativeModuleFailure::InvalidMetadata {
                    detail: format!(
                        "authoritative source token {} occurs more than once",
                        source.token.0
                    ),
                });
            }
        }
        // A Program source shadows the library of the same name, as in the
        // batch drivers.
        let fixture_names: rustc_hash::FxHashSet<JsStr<'_>> = files
            .iter()
            .filter(|file| !file.host_only)
            .map(|file| file.name.as_js())
            .collect();
        let mut effective_libs = Vec::new();
        let mut effective_lib_metadata = Vec::new();
        for (lib, metadata) in libs.iter().zip(&lib_metadata) {
            if !fixture_names.contains(&lib.name.as_js()) {
                effective_libs.push(lib);
                effective_lib_metadata.push(metadata.clone());
            }
        }
        let bundle = (!effective_libs.is_empty()).then(|| lib_bundle(&effective_libs, &options));
        let lib_documents = bundle.map_or(&[][..], |bundle| bundle.documents);
        let identity_domain = bundle
            .map(|bundle| bundle.identity_domain.clone())
            .unwrap_or_else(IdentityDomain::ephemeral);

        let mut work_counters = CheckWorkCounters::default();
        let ParsedProgramInputs {
            program_sources,
            authoritative_program_metadata,
            program_diagnostics,
            host,
        } = {
            let run = AuthoritativeRun {
                provider: AuthoritativeProviderSource::Shared(&*provider),
                lib_metadata: effective_lib_metadata.clone(),
                file_metadata: file_metadata.clone(),
                library_prefix: LibraryPrefixCompletion::Complete,
                diagnostic_schedule: DiagnosticSchedule::OnDemand,
                incremental_facts: false,
                tracing: None,
                bind_span: std::sync::Mutex::new(None),
            };
            parse_program_inputs(
                &libs,
                &files,
                &options,
                current_directory.as_js(),
                &identity_domain,
                Some(&run),
                &mut work_counters,
                workers,
            )
        };

        let lib_count = lib_documents.len();
        let mut document_store = EphemeralDocumentStore::with_documents(
            identity_domain.clone(),
            lib_documents.iter().cloned(),
        );
        let bind_data = bind_sources_in_program_order(
            &program_sources,
            &options,
            &identity_domain,
            workers,
            None,
        );
        for (source_file, data) in program_sources.iter().zip(bind_data) {
            document_store
                .publish(Arc::clone(source_file), data)
                .expect("completed bind must belong to the ephemeral document domain");
        }
        let mut file_facts = vec![ProgramFileFacts::DEFAULT_LIBRARY; lib_count];
        file_facts.resize(
            lib_count + program_sources.len(),
            ProgramFileFacts::ORDINARY,
        );
        let snapshot = document_store
            .into_snapshot_with_file_facts(file_facts)
            .expect("program snapshot identity allocation failed");
        let passes = syntactic_file_rows(&snapshot, lib_count, &options);
        let mut metadata = effective_lib_metadata;
        metadata.extend(authoritative_program_metadata);
        let owner = LiveOwner {
            snapshot,
            options,
            provider,
            metadata,
            host,
            lib_count,
            passes,
            program_diagnostics,
        };
        let cell = LiveCell::new(owner, |owner| {
            let mut state = init_checker_state(
                &owner.snapshot,
                &owner.options,
                Some((&*owner.provider, &owner.metadata)),
                owner.host.clone(),
                Some(0),
            );
            reserve_type_tables(&mut state, snapshot_node_count(&owner.snapshot));
            let file_count = state.binder.file_count();
            LiveState {
                state,
                checked: vec![false; file_count],
                globals_by_file: vec![Vec::new(); file_count],
            }
        });
        Ok(Self { cell })
    }

    /// The Program's files, libraries first.
    pub fn snapshot(&self) -> &ProgramSnapshot {
        &self.cell.borrow_owner().snapshot
    }

    pub fn options(&self) -> &CompilerOptions {
        &self.cell.borrow_owner().options
    }

    /// The number of files, libraries included.
    pub fn file_count(&self) -> usize {
        self.cell.borrow_owner().snapshot.documents().len()
    }

    /// The number of library files at the start of the Program.
    pub fn lib_count(&self) -> usize {
        self.cell.borrow_owner().lib_count
    }

    /// The Program's preparation diagnostics (the loader's program rows).
    pub fn program_diagnostics(&self) -> &[Diagnostic] {
        &self.cell.borrow_owner().program_diagnostics
    }

    /// The syntactic rows of a Program source (`file` counts the libraries;
    /// a library has none here, as in the batch drivers).
    pub fn syntactic_diagnostics(&self, file: usize) -> &[Diagnostic] {
        let owner = self.cell.borrow_owner();
        file.checked_sub(owner.lib_count)
            .and_then(|index| owner.passes.get(index))
            .map_or(&[][..], |passes| &passes.syntactic)
    }

    /// Check `file` unless it has been checked (tsgo's
    /// `getSemanticDiagnosticsForFile` on the API checker). An index past the
    /// Program's files checks nothing.
    fn ensure_checked(&mut self, file: usize) -> Result<(), AuthoritativeModuleFailure> {
        self.cell.with_dependent_mut(|_, live| {
            if live.checked.get(file).copied().unwrap_or(true) {
                return Ok(());
            }
            live.checked[file] = true;
            check_program_file(
                &mut live.state,
                program_file_id(file),
                &mut live.globals_by_file,
            );
            match live.state.take_authoritative_module_failure() {
                Some(failure) => Err(failure),
                None => Ok(()),
            }
        })
    }

    /// The semantic rows of `file`, checking it first. A file the options
    /// skip (`skipLibCheck`, a JavaScript file without `checkJs`) has none.
    pub fn semantic_diagnostics(
        &mut self,
        file: usize,
    ) -> Result<DiagnosticList, AuthoritativeModuleFailure> {
        self.ensure_checked(file)?;
        Ok(self.cell.with_dependent(|owner, live| {
            if file >= live.checked.len()
                || live.state.skip_type_checking_file(program_file_id(file))
            {
                return Vec::new();
            }
            semantic_diagnostics_for_program_file(
                &live.state,
                file,
                &live.globals_by_file[file],
                &owner.program_diagnostics,
                &owner.options,
            )
        }))
    }

    /// The suggestion rows of `file`, checking it first.
    pub fn suggestion_diagnostics(
        &mut self,
        file: usize,
    ) -> Result<DiagnosticList, AuthoritativeModuleFailure> {
        self.ensure_checked(file)?;
        Ok(self.cell.with_dependent(|_, live| {
            if file >= live.checked.len()
                || live.state.skip_type_checking_file(program_file_id(file))
            {
                return Vec::new();
            }
            let file_name = live.state.binder.source(file).file_name.clone();
            live.state
                .diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.file_name.as_ref() == Some(&file_name)
                        && diagnostic.category() == DiagnosticCategory::Suggestion
                })
                .cloned()
                .collect()
        }))
    }

    /// The global rows the checker has published so far, sorted and
    /// deduplicated (tsgo `getGlobalDiagnostics`).
    pub fn global_diagnostics(&self) -> DiagnosticList {
        self.cell.with_dependent(|owner, live| {
            if owner.snapshot.documents().is_empty() {
                return globals::missing_init_global_type_diagnostics(&owner.options);
            }
            let mut diagnostics = live.state.visible_global_diagnostics.clone();
            tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
            diagnostics
        })
    }

    /// Run `query` over the checker (the API checker's queries: types,
    /// symbols, signatures and the node builder).
    pub fn with_checker<T>(&mut self, query: impl FnOnce(&mut CheckerState<'_>) -> T) -> T {
        self.cell
            .with_dependent_mut(|_, live| query(&mut live.state))
    }

    /// The Program file id of the file at `index` (libraries first).
    pub fn file_id(index: usize) -> ProgramFileId {
        program_file_id(index)
    }
}
