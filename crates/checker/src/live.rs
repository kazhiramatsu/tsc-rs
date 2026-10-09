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

use crate::program::{
    BoundDocument, DocumentRegistry, EphemeralDocumentStore, ProgramFileFacts, ProgramFileId,
    ProgramSnapshot,
};
use crate::state::CheckerState;
use crate::{
    bind_sources_in_program_order, check_program_file, globals, init_checker_state, lib_bundle,
    parse_program_inputs, program_file_id, reserve_type_tables,
    semantic_diagnostics_for_program_file, snapshot_node_count, syntactic_file_rows,
    validate_authoritative_metadata, AuthoritativeModuleFailure, AuthoritativeModuleProvider,
    AuthoritativeProviderSource, AuthoritativeRun, AuthoritativeSourceMetadata, CheckWorkCounters,
    CompilerOptions, DiagnosticSchedule, FileDiagnosticPasses, HostFacts, InputFile,
    LibraryPrefixCompletion, ParsedProgramInputs, SharedDocument, WorkerBudget,
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
    live: Live,
}

enum Live {
    /// A Program without files has no checker, as in the batch drivers.
    Empty {
        options: Box<CompilerOptions>,
        program_diagnostics: Vec<Diagnostic>,
    },
    Checked(LiveCell),
}

impl LiveChecker {
    /// Parse and bind the Program's sources and initialize the checker
    /// (globals merged, no source checked): the on-demand schedule of the
    /// batch drivers, kept instead of handed to a callback. With `documents`
    /// (tsgo's parse cache), a source whose text and address a Program
    /// already holds is that document, and a new one is recorded there.
    pub fn new(
        inputs: LiveCheckerInputs,
        provider: Box<dyn AuthoritativeModuleProvider + Send>,
        workers: WorkerBudget,
        documents: Option<&DocumentRegistry>,
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
            .or_else(|| documents.map(|documents| documents.identity_domain().clone()))
            .unwrap_or_else(IdentityDomain::ephemeral);

        let mut work_counters = CheckWorkCounters::default();
        let ParsedProgramInputs {
            program_sources,
            shared_documents,
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
                documents,
            )
        };

        if lib_documents.is_empty() && program_sources.is_empty() {
            return Ok(Self {
                live: Live::Empty {
                    options: Box::new(options),
                    program_diagnostics,
                },
            });
        }
        let lib_count = lib_documents.len();
        let mut document_store = EphemeralDocumentStore::with_documents(
            identity_domain.clone(),
            lib_documents.iter().cloned(),
        );
        // The sources no Program holds a document for are bound, in program
        // order; the others are those documents.
        let found = |index: usize| match shared_documents.get(index) {
            Some(SharedDocument::Found(document)) => Some(document),
            _ => None,
        };
        let new_sources = program_sources
            .iter()
            .enumerate()
            .filter(|&(index, _)| found(index).is_none())
            .map(|(_, source_file)| Arc::clone(source_file))
            .collect::<Vec<_>>();
        let mut bind_data =
            bind_sources_in_program_order(&new_sources, &options, &identity_domain, workers, None)
                .into_iter();
        for (index, source_file) in program_sources.iter().enumerate() {
            if let Some(document) = found(index) {
                document_store
                    .adopt(Arc::clone(document))
                    .expect("a shared document must belong to the Program's identity domain");
                continue;
            }
            let document = document_store
                .publish(
                    Arc::clone(source_file),
                    bind_data.next().expect("a bind of every new source"),
                )
                .expect("completed bind must belong to the ephemeral document domain");
            if let (Some(documents), Some(SharedDocument::New(address))) =
                (documents, shared_documents.get(index))
            {
                documents.insert(address.clone(), &document);
            }
        }
        let file_facts = crate::program_file_facts(
            ProgramFileFacts::DEFAULT_LIBRARY,
            lib_count,
            program_sources.len(),
            &authoritative_program_metadata,
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
        Ok(Self {
            live: Live::Checked(cell),
        })
    }

    /// The parsed and bound document of the file at `index` (libraries
    /// first); a Program built again over a [`DocumentRegistry`] has the
    /// same one for a file that did not change.
    pub fn document(&self, index: usize) -> Option<&Arc<BoundDocument>> {
        match &self.live {
            Live::Empty { .. } => None,
            Live::Checked(cell) => cell.borrow_owner().snapshot.documents().get(index),
        }
    }

    /// The name of the file at `index` (libraries first).
    pub fn file_name(&self, index: usize) -> Option<JsStr<'_>> {
        match &self.live {
            Live::Empty { .. } => None,
            Live::Checked(cell) => cell
                .borrow_owner()
                .snapshot
                .documents()
                .get(index)
                .map(|document| document.source().file_name.as_js()),
        }
    }

    pub fn options(&self) -> &CompilerOptions {
        match &self.live {
            Live::Empty { options, .. } => options,
            Live::Checked(cell) => &cell.borrow_owner().options,
        }
    }

    /// The number of files, libraries included.
    pub fn file_count(&self) -> usize {
        match &self.live {
            Live::Empty { .. } => 0,
            Live::Checked(cell) => cell.borrow_owner().snapshot.documents().len(),
        }
    }

    /// The number of library files at the start of the Program.
    pub fn lib_count(&self) -> usize {
        match &self.live {
            Live::Empty { .. } => 0,
            Live::Checked(cell) => cell.borrow_owner().lib_count,
        }
    }

    /// The Program's preparation diagnostics (the loader's program rows).
    pub fn program_diagnostics(&self) -> &[Diagnostic] {
        match &self.live {
            Live::Empty {
                program_diagnostics,
                ..
            } => program_diagnostics,
            Live::Checked(cell) => &cell.borrow_owner().program_diagnostics,
        }
    }

    /// The syntactic rows of a Program source (`file` counts the libraries;
    /// a library has none here, as in the batch drivers).
    pub fn syntactic_diagnostics(&self, file: usize) -> &[Diagnostic] {
        let Live::Checked(cell) = &self.live else {
            return &[];
        };
        let owner = cell.borrow_owner();
        file.checked_sub(owner.lib_count)
            .and_then(|index| owner.passes.get(index))
            .map_or(&[][..], |passes| &passes.syntactic)
    }

    /// Check `file` unless it has been checked (tsgo's
    /// `getSemanticDiagnosticsForFile` on the API checker). An index past the
    /// Program's files checks nothing.
    fn ensure_checked(&mut self, file: usize) -> Result<(), AuthoritativeModuleFailure> {
        let Live::Checked(cell) = &mut self.live else {
            return Ok(());
        };
        cell.with_dependent_mut(|_, live| {
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
        let Live::Checked(cell) = &self.live else {
            return Ok(Vec::new());
        };
        Ok(cell.with_dependent(|owner, live| {
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
        let Live::Checked(cell) = &self.live else {
            return Ok(Vec::new());
        };
        Ok(cell.with_dependent(|_, live| {
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
    /// deduplicated (tsgo `getGlobalDiagnostics`); a Program without files
    /// reports the global types it lacks.
    pub fn global_diagnostics(&self) -> DiagnosticList {
        match &self.live {
            Live::Empty { options, .. } => globals::missing_init_global_type_diagnostics(options),
            Live::Checked(cell) => cell.with_dependent(|_, live| {
                let mut diagnostics = live.state.visible_global_diagnostics.clone();
                tsc_diagnostics::sort_and_dedupe_diagnostics(&mut diagnostics);
                diagnostics
            }),
        }
    }

    /// Run `query` over the checker (the API checker's queries: types,
    /// symbols, signatures and the node builder); none for a Program without
    /// files.
    pub fn with_checker<T>(&mut self, query: impl FnOnce(&mut CheckerState<'_>) -> T) -> Option<T> {
        match &mut self.live {
            Live::Empty { .. } => None,
            Live::Checked(cell) => Some(cell.with_dependent_mut(|_, live| query(&mut live.state))),
        }
    }

    /// The Program file id of the file at `index` (libraries first).
    pub fn file_id(index: usize) -> ProgramFileId {
        program_file_id(index)
    }
}
