//! The build info of an incremental program (tsgo `execute/incremental`):
//! the old build info read back, the Program's facts, the checker's per-file
//! facts and the emit's declaration outputs driven through the snapshot
//! `tsc_incremental` keeps (`IncrementalDriver`), and the sink wrapper that
//! hashes the declaration files as they are written (tsgo's emit
//! signatures, and the skipped write of a composite project's unchanged
//! declaration file).

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tsc_checker::emit::CheckerSession;
use tsc_checker::{IncrementalCheckFacts, IncrementalFileFacts, IncrementalPlan, ProgramSnapshot};
use tsc_diagnostics::{
    by_code, Diagnostic, DiagnosticCategory, JsStr, JsString, MessageChain, RelatedInfo, Repopulate,
};
use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitIoError, EmitOutcome, EmitPreflight,
    EmitWriteDisposition, EmitWriteMetadata, ForcedDeclarationEmitter, ForcedDeclarationOutput,
    OutputSink, SharedOutputSink, UnitEmitRequest,
};
use tsc_host::CompilerHost;
use tsc_incremental::options::{emit_declarations, is_incremental};
use tsc_incremental::{
    compute_hash_with_text, declaration_write_decision, ensure_path_is_non_module_name,
    fresh_emit_updates, BuildInfo, BuildInfoDiagnostic, CachedDiagnostic, CachedRows,
    DeclarationEmit, DeclarationEmitFacts, DeclarationOutput, EmitUpdate, FileEmitKind, FileState,
    OldState, OldStatePaths, ProgramFileFacts, ProgramState, RepopulateInfo, Snapshot,
};
use tsc_program::{
    PackageJsonType, PreparedProgram, ResolutionKey, ResolutionMode, ResolutionOutcome,
    SourceFileId, TypeReferenceResolutionOrigin,
};

use crate::{CheckedEmitHost, EmitRouteKind, PreparedEmitHost, ProgramDiagnostics, SourceApiFacts};

/// A build info document ready to be written: tsgo `emitBuildInfo`'s file
/// name and `json.Marshal` bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildInfoDocument {
    pub file_name: JsString,
    pub text: String,
}

/// Whether the command reported diagnostics outside the semantic cache
/// (tsgo `GetDiagnosticsOfAnyProgram`'s gates and `ensureHasErrorsForState`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CommandDiagnosticFacts {
    pub(crate) config: bool,
    pub(crate) syntactic: bool,
    /// The options rows, including ones the command owns itself.
    pub(crate) options: bool,
    pub(crate) global: bool,
}

impl CommandDiagnosticFacts {
    pub(crate) fn of(diagnostics: &ProgramDiagnostics, command_options: bool) -> Self {
        Self {
            config: !diagnostics.config().is_empty(),
            syntactic: !diagnostics.syntactic().is_empty(),
            options: !diagnostics.options().is_empty() || command_options,
            global: !diagnostics.global().is_empty(),
        }
    }

    /// tsgo asks for the semantic diagnostics (and so caches them) only
    /// when the syntactic, options and global diagnostics are empty.
    pub(crate) fn semantic_cached(self) -> bool {
        !self.syntactic && !self.options && !self.global
    }
}

/// One declaration file the emit produced: the source it was emitted from,
/// the output file and the signature (the hash of the text up to its
/// source map comment).
#[derive(Clone, Debug)]
pub(crate) struct DeclarationRecord {
    source: JsString,
    output: JsString,
    signature: String,
    /// `tsc -b`: written although only its map changed (tsgo
    /// `differsOnlyInMap`).
    differs_only_in_map: bool,
}

/// The byte length of `text` up to a UTF-16 position (tsgo
/// `getTextHandlingSourceMapForSignature`: the text before its
/// `sourceMappingURL` comment).
fn byte_cut(text: &str, utf16_position: Option<u32>) -> usize {
    let Some(position) = utf16_position else {
        return text.len();
    };
    let mut units = 0u32;
    for (offset, ch) in text.char_indices() {
        if units >= position {
            return offset;
        }
        units += ch.len_utf16() as u32;
    }
    text.len()
}

fn record_of(artifact: &EmitArtifact, hash_with_text: bool) -> Option<DeclarationRecord> {
    if artifact.kind() != EmitArtifactKind::Declaration {
        return None;
    }
    let source = artifact.source_files()?.first()?.clone();
    let text = artifact.callback_text();
    let position = match artifact.metadata() {
        Some(EmitWriteMetadata::Text(metadata)) => metadata
            .source_map_url_position()
            .map(|position| position.value()),
        _ => None,
    };
    let cut = byte_cut(text, position);
    Some(DeclarationRecord {
        source,
        output: artifact.path().to_owned(),
        signature: compute_hash_with_text(&text[..cut], hash_with_text),
        differs_only_in_map: false,
    })
}

/// The old emit signatures of a composite project's files, by the source's
/// name as the emit names it: the hash and whether it is in the plain form
/// (tsgo `skipDtsOutputOfComposite` reads them in the write hook).
pub(crate) type CompositeSignatures = HashMap<JsString, (String, bool)>;

/// Whether a declaration file's write is skipped, and whether it is written
/// although only its map changed (`build` only: tsgo `differsOnlyInMap`).
fn skip_declaration_write(
    composite: Option<&CompositeSignatures>,
    build: bool,
    record: &DeclarationRecord,
) -> (bool, bool) {
    let Some(composite) = composite else {
        return (false, false);
    };
    let old = composite
        .get(&record.source)
        .map(|(signature, plain)| (signature.as_str(), *plain));
    let decision = declaration_write_decision(true, old, &record.signature);
    (decision.skip, build && !decision.skip && !decision.changed)
}

/// The shared half of [`SignatureRecordingSink`].
pub(crate) struct RecordingSharedSink<'s> {
    inner: &'s dyn SharedOutputSink,
    composite: Option<Arc<CompositeSignatures>>,
    build: bool,
    hash_with_text: bool,
    records: Mutex<Vec<DeclarationRecord>>,
    writes: AtomicUsize,
}

impl SharedOutputSink for RecordingSharedSink<'_> {
    fn write_shared(&self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if let Some(mut record) = record_of(&artifact, self.hash_with_text) {
            let (skip, differs_only_in_map) =
                skip_declaration_write(self.composite.as_deref(), self.build, &record);
            record.differs_only_in_map = differs_only_in_map;
            self.records
                .lock()
                .expect("declaration records are never poisoned")
                .push(record);
            if skip {
                return Ok(EmitWriteDisposition::SkippedUnchanged);
            }
        }
        self.inner.write_shared(artifact)
    }
}

/// An output sink that records the signature of every declaration file
/// written through it (tsgo `emitFilesHandler.getEmitOptions`' write hook),
/// skips the write of a composite project's unchanged declaration file
/// (`skipDtsOutputOfComposite`) and forwards the other writes.
pub(crate) struct SignatureRecordingSink<'s> {
    ordered: Option<&'s mut dyn OutputSink>,
    shared: Option<RecordingSharedSink<'s>>,
    composite: Option<Arc<CompositeSignatures>>,
    build: bool,
    hash_with_text: bool,
    eager_source_roots: bool,
    records: Vec<DeclarationRecord>,
    writes: usize,
}

impl<'s> SignatureRecordingSink<'s> {
    pub(crate) fn new(sink: &'s mut dyn OutputSink, hash_with_text: bool) -> Self {
        Self::with_composite_signatures(sink, None, false, hash_with_text)
    }

    /// `build`: the command is `tsc -b` (a declaration file that differs
    /// only in its map is recorded as such); `hash_with_text`: the
    /// signatures carry the declaration text (tsgo `hashWithText`).
    pub(crate) fn with_composite_signatures(
        sink: &'s mut dyn OutputSink,
        composite: Option<CompositeSignatures>,
        build: bool,
        hash_with_text: bool,
    ) -> Self {
        let composite = composite.map(Arc::new);
        let eager_source_roots = sink.writes_source_roots_eagerly();
        if sink.shared().is_some() {
            let shared = sink
                .shared()
                .expect("the shared handle was present a moment ago");
            Self {
                ordered: None,
                shared: Some(RecordingSharedSink {
                    inner: shared,
                    composite: composite.clone(),
                    build,
                    hash_with_text,
                    records: Mutex::new(Vec::new()),
                    writes: AtomicUsize::new(0),
                }),
                composite,
                build,
                hash_with_text,
                eager_source_roots,
                records: Vec::new(),
                writes: 0,
            }
        } else {
            Self {
                ordered: Some(sink),
                shared: None,
                composite,
                build,
                hash_with_text,
                eager_source_roots,
                records: Vec::new(),
                writes: 0,
            }
        }
    }

    /// Every declaration file produced so far (written or skipped as
    /// unchanged), in write order.
    pub(crate) fn records(&self) -> Vec<DeclarationRecord> {
        let mut records = self.records.clone();
        if let Some(shared) = &self.shared {
            records.extend(
                shared
                    .records
                    .lock()
                    .expect("declaration records are never poisoned")
                    .iter()
                    .cloned(),
            );
        }
        records
    }

    /// Whether any artifact was written through this sink.
    pub(crate) fn wrote_anything(&self) -> bool {
        self.writes > 0
            || self
                .shared
                .as_ref()
                .is_some_and(|shared| shared.writes.load(Ordering::Relaxed) > 0)
    }

    /// Write the build info document through the underlying sink; the
    /// failure is tsgo's TS5033.
    pub(crate) fn write_build_info(&mut self, document: &BuildInfoDocument) -> Option<Diagnostic> {
        let artifact = EmitArtifact::build_info(
            document.file_name.clone(),
            document.text.as_str(),
            EmitBuildInfoMetadata::new(1, ""),
        );
        let result = match (&mut self.ordered, &self.shared) {
            (Some(ordered), _) => ordered.write(artifact),
            (None, Some(shared)) => shared.inner.write_shared(artifact),
            (None, None) => unreachable!("a recording sink wraps one sink"),
        };
        result.err().map(|error| write_failure(&error))
    }
}

impl OutputSink for SignatureRecordingSink<'_> {
    fn write(&mut self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        if let Some(shared) = &self.shared {
            return shared.write_shared(artifact);
        }
        self.writes += 1;
        if let Some(mut record) = record_of(&artifact, self.hash_with_text) {
            let (skip, differs_only_in_map) =
                skip_declaration_write(self.composite.as_deref(), self.build, &record);
            record.differs_only_in_map = differs_only_in_map;
            self.records.push(record);
            if skip {
                return Ok(EmitWriteDisposition::SkippedUnchanged);
            }
        }
        self.ordered
            .as_mut()
            .expect("a recording sink wraps one sink")
            .write(artifact)
    }

    fn shared(&self) -> Option<&dyn SharedOutputSink> {
        self.shared
            .as_ref()
            .map(|shared| shared as &dyn SharedOutputSink)
    }

    fn writes_source_roots_eagerly(&self) -> bool {
        self.eager_source_roots
    }
}

/// tsgo `Could_not_write_file_0_Colon_1` for the build info (TS5033).
fn write_failure(error: &EmitIoError) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain::new_js(
            &tsc_diagnostics::gen::Could_not_write_file_0_1,
            &[error.path().to_owned(), error.message().to_owned()],
        ),
    )
}

/// Write the document through an ordinary sink (the `--noEmit` command).
pub(crate) fn write_build_info(
    sink: &mut dyn OutputSink,
    document: &BuildInfoDocument,
) -> Option<Diagnostic> {
    let artifact = EmitArtifact::build_info(
        document.file_name.clone(),
        document.text.as_str(),
        EmitBuildInfoMetadata::new(1, ""),
    );
    sink.write(artifact)
        .err()
        .map(|error| write_failure(&error))
}

/// tsgo `GetBuildInfoFileName`: the build info of an incremental or
/// composite program.
pub(crate) fn build_info_file_name(prepared: &PreparedProgram) -> Option<JsString> {
    build_info_file_name_for(prepared, false)
}

/// The build info a program writes; under `tsc -b` (`build`, tsgo
/// `CompilerOptions.Build`) every project writes one.
pub(crate) fn build_info_file_name_for(
    prepared: &PreparedProgram,
    build: bool,
) -> Option<JsString> {
    let name = if build {
        tsc_program::build_info_file_name_in_build_mode
    } else {
        tsc_program::build_info_file_name
    };
    name(
        prepared.compiler_options(),
        prepared
            .program_options()
            .config_file_path()
            .map(|path| path.display()),
        prepared.current_directory().display(),
        prepared.path_context().use_case_sensitive_file_names(),
    )
}

fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

/// tsgo `ReadBuildInfoProgram` (execute/incremental/incremental.go): the
/// state of the program's old build info, when the file exists, parses, is
/// of this version and holds an incremental program. `default_library_
/// directory` names the default libraries the document records by their
/// bare names.
pub fn read_old_build_info(
    host: &dyn CompilerHost,
    prepared: &PreparedProgram,
    default_library_directory: &str,
) -> Option<OldState> {
    if !is_incremental(prepared.compiler_options()) {
        return None;
    }
    let file_name = build_info_file_name(prepared)?;
    let bytes = host.read_file_js(file_name.as_js()).ok()??;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let info = BuildInfo::from_json(text)?;
    old_state_of(&info, prepared, default_library_directory)
}

/// The state of a build info a build already read (tsgo
/// `ReadBuildInfoProgram` with the build's `BuildInfoReader`): `None`
/// unless the program is incremental and the document is a valid
/// incremental one of this version.
pub(crate) fn old_state_of(
    info: &BuildInfo,
    prepared: &PreparedProgram,
    default_library_directory: &str,
) -> Option<OldState> {
    if !is_incremental(prepared.compiler_options()) {
        return None;
    }
    let file_name = build_info_file_name_for(prepared, true)?;
    if !info.is_valid_version() || !info.is_incremental() {
        return None;
    }
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    let directory = directory_of(&file_name.to_string_lossy());
    let absolute = |name: &str, directory: &str| {
        tsc_program::normalize_absolute_js_path_lexical(name.into(), Some(directory.into()))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| name.to_owned())
    };
    let canonical = |name: &str, directory: &str| {
        let absolute = absolute(name, directory);
        if case_sensitive {
            absolute
        } else {
            tsc_host::to_file_name_lower_case_js(absolute.as_str().into())
                .to_string_lossy()
                .into_owned()
        }
    };
    Some(OldState::from_build_info(
        info,
        &OldStatePaths {
            build_info_directory: &directory,
            default_library_directory,
            canonical: &canonical,
            absolute: &absolute,
        },
    ))
}

/// When tsgo runs `collectAllAffectedFiles` in this command: in its semantic
/// getter (when the checker's gates let it run and `noCheck` is off), and in
/// its emit (unless `noEmitOnError` stops the emit before the files are
/// emitted).
#[derive(Clone, Copy, Debug)]
pub(crate) struct AffectedPolicy {
    /// The command emits.
    pub(crate) emits: bool,
    /// The command reports program (options) rows, which close tsgo's
    /// semantic getter (the checker's planner gate sees the syntactic and
    /// global rows only).
    pub(crate) options_diagnostics: bool,
    /// The command is `tsc -b` (tsgo `CompilerOptions.Build`): every project
    /// writes a build info, a non-incremental one tracking its errors only.
    pub(crate) build: bool,
    /// Versions and signatures carry their text (tsgo `hashWithText`, set
    /// under its test harness).
    pub(crate) hash_with_text: bool,
    /// A watch run's cycle: the program is incremental in memory whatever
    /// its options say, and its state is kept for the next cycle (tsgo's
    /// watcher builds an `incremental.Program` every cycle).
    pub(crate) watch: bool,
}

/// The state a watch run carries from one cycle to the next: the program's
/// snapshot as an incremental build info, with the directory it is relative
/// to and the options it was built with.
#[derive(Clone, Debug)]
pub struct WatchState {
    info: tsc_incremental::BuildInfo,
    directory: String,
    options: tsc_program::CompilerOptions,
}

/// The name a watch run's state is relative to when the program writes no
/// build info: the one a build would write, else one in the current
/// directory.
fn watch_state_file_name(prepared: &PreparedProgram) -> JsString {
    build_info_file_name_for(prepared, true).unwrap_or_else(|| {
        let mut name = prepared.current_directory().display().to_owned();
        name.push_str("/tsconfig.tsbuildinfo");
        name
    })
}

/// The old state of a watch run's cycle: the previous cycle's (tsgo's
/// watcher passes the previous incremental program), with its options.
pub(crate) fn old_state_for_watch(
    state: &WatchState,
    prepared: &PreparedProgram,
    default_library_directory: &str,
) -> OldState {
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    let absolute = |name: &str, directory: &str| {
        tsc_program::normalize_absolute_js_path_lexical(name.into(), Some(directory.into()))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| name.to_owned())
    };
    let canonical = |name: &str, directory: &str| {
        let absolute = absolute(name, directory);
        if case_sensitive {
            absolute
        } else {
            tsc_host::to_file_name_lower_case_js(absolute.as_str().into())
                .to_string_lossy()
                .into_owned()
        }
    };
    let mut old = OldState::from_build_info(
        &state.info,
        &OldStatePaths {
            build_info_directory: &state.directory,
            default_library_directory,
            canonical: &canonical,
            absolute: &absolute,
        },
    );
    old.options = state.options.clone();
    old
}

/// One file of an incremental program's run as tsgo's test harness sees it
/// (`incremental.TestingData` over the program's files).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFileReport {
    pub file_name: String,
    pub semantic_diagnostics: tsc_incremental::SemanticDiagnosticsState,
    pub signature_update: Option<tsc_incremental::SignatureUpdateKind>,
}

/// What `tsc -b` learns from a project's emit (tsgo
/// `Program.HasChangedDtsFile` and the write hook's `differsOnlyInMap`).
#[derive(Clone, Debug, Default)]
pub(crate) struct BuildEmitFacts {
    /// A declaration file whose signature changed was written.
    pub(crate) has_changed_dts_file: bool,
    /// Declaration files written although only their map changed; `tsc -b`
    /// restores their modification times so the downstream projects see no
    /// change.
    pub(crate) declarations_differing_only_in_map: Vec<JsString>,
}

struct DriverState {
    snapshot: Snapshot,
    /// The planner's facts (every file's module facts and program rows).
    facts: Vec<IncrementalFileFacts>,
    /// A global row the check deferred into a file's rows.
    /// The check of the files reported file-less rows.
    check_global_rows: bool,
    /// The planner's declaration signatures reported file-less rows.
    planner_global_rows: bool,
    /// The declaration files of this emit that differ only in their map.
    differing_only_in_map: Vec<JsString>,
}

/// tsgo `incremental.Program` for one command: the snapshot over the old
/// state, fed by the checker's planner hook, the check's rows, the
/// declaration getter and the emit, and the build info they produce.
pub(crate) struct IncrementalDriver<'p> {
    prepared: &'p PreparedProgram,
    old: Option<Arc<OldState>>,
    /// The host the planner's declaration signatures are computed over;
    /// `None` without an old state (no signature is computed then).
    emit_host: Option<PreparedEmitHost<'p>>,
    /// The build info the snapshot's paths are relative to: the one the
    /// program writes, or a watch run's in-memory state's.
    state_file_name: Option<JsString>,
    /// The program writes its build info (an incremental or composite
    /// program, every project of a build).
    writes_build_info: bool,
    policy: AffectedPolicy,
    state: Mutex<Option<DriverState>>,
    /// A watch run's state for its next cycle, kept when the build info is
    /// assembled.
    watch_state: Mutex<Option<WatchState>>,
}

impl<'p> IncrementalDriver<'p> {
    pub(crate) fn new(
        prepared: &'p PreparedProgram,
        old: Option<Arc<OldState>>,
        emit_route: EmitRouteKind,
        source_api_facts: &std::collections::BTreeMap<SourceFileId, SourceApiFacts>,
        policy: AffectedPolicy,
    ) -> Self {
        let emit_host = old
            .is_some()
            .then(|| PreparedEmitHost::new_for_route(prepared, emit_route, source_api_facts).ok())
            .flatten();
        let build_info_file_name = build_info_file_name_for(prepared, policy.build);
        let writes_build_info = build_info_file_name.is_some();
        let state_file_name =
            build_info_file_name.or_else(|| policy.watch.then(|| watch_state_file_name(prepared)));
        Self {
            prepared,
            old,
            emit_host,
            state_file_name,
            writes_build_info,
            policy,
            state: Mutex::new(None),
            watch_state: Mutex::new(None),
        }
    }

    fn state(&self) -> MutexGuard<'_, Option<DriverState>> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The checker's planner hook (tsgo `programToSnapshot` and
    /// `collectAllAffectedFiles`): the snapshot over the old state, the
    /// affected files handled with their declaration signatures, and the
    /// files the check covers.
    pub(crate) fn planner(
        &self,
    ) -> impl FnMut(
        &ProgramSnapshot,
        &CheckerSession<'_>,
        &[IncrementalFileFacts],
        bool,
    ) -> IncrementalPlan
           + '_ {
        move |snapshot: &ProgramSnapshot,
              session: &CheckerSession<'_>,
              facts: &[IncrementalFileFacts],
              check_runs: bool| {
            let Some(file_name) = &self.state_file_name else {
                return IncrementalPlan::default();
            };
            let assembly =
                Assembly::new(self.prepared, facts, file_name, self.policy.hash_with_text);
            let options = self.prepared.compiler_options();
            let mut snap = Snapshot::new(
                assembly.program_state(self.policy.build),
                self.old.as_deref(),
            );
            let check_runs = check_runs && !self.policy.options_diagnostics;
            let no_check = options.no_check == Some(true);
            let no_emit_on_error = options.no_emit_on_error == Some(true);
            // The emit runs unless noEmitOnError meets a gated check.
            let emit_runs = self.policy.emits && (check_runs || !no_emit_on_error);
            let run_affected = (check_runs && !no_check) || emit_runs;
            let trace = std::env::var_os("TSRS_INCREMENTAL_TRACE").is_some();
            if trace {
                eprintln!(
                    "[incremental] old state: {}, check runs: {check_runs}, emits: {}, affected handling: {run_affected}, changed: {}",
                    self.old.is_some(),
                    self.policy.emits,
                    snap.has_changed_files()
                );
            }
            if run_affected && snap.has_changed_files() {
                let checked_host = self.emit_host.as_ref().map(|host| CheckedEmitHost {
                    prepared: host,
                    snapshot,
                    prepared_sources: None,
                });
                let emitter = checked_host
                    .as_ref()
                    .and_then(|host| ForcedDeclarationEmitter::new(host).ok());
                if trace && emitter.is_none() {
                    eprintln!(
                        "[incremental] no forced declaration emitter: signatures use the versions"
                    );
                }
                let mut signature_of = |file: usize| -> Option<String> {
                    let host = checked_host.as_ref()?;
                    let emitter = emitter.as_ref()?;
                    let source = self.prepared.source_files().get(file)?;
                    let id = self.prepared.source_id(source.path().canonical())?;
                    let output = session
                        .with_emit_resolver(|resolver| emitter.emit(resolver, host, id))
                        .ok()??;
                    let signature = assembly.declaration_signature(file, &output);
                    if trace {
                        eprintln!(
                            "[incremental] signature of {}: {signature} ({} bytes, map url at {:?}, {} diagnostics)",
                            source.path().display().to_string_lossy(),
                            output.text.len(),
                            output.source_map_url_position,
                            output.diagnostics.len()
                        );
                    }
                    Some(signature)
                };
                let mut exports_const_enum = |file: usize| {
                    facts
                        .get(file)
                        .is_some_and(|facts| facts.exports_const_enum)
                };
                snap.collect_all_affected_files(&mut signature_of, &mut exports_const_enum);
            }
            let check = snap.files_without_rows();
            if trace {
                for (index, source) in self.prepared.source_files().iter().enumerate() {
                    let pending = snap.pending_emit_kind(index);
                    if check[index] || pending != FileEmitKind::NONE {
                        eprintln!(
                            "[incremental] {}: check {}, pending emit {pending}",
                            source.path().display().to_string_lossy(),
                            check[index]
                        );
                    }
                }
            }
            *self.state() = Some(DriverState {
                snapshot: snap,
                facts: facts.to_vec(),
                check_global_rows: false,
                planner_global_rows: false,
                differing_only_in_map: Vec::new(),
            });
            IncrementalPlan { check }
        }
    }

    /// tsgo's commit of the semantic getter: the rows the check produced
    /// for the files it covered, when the command asked for the semantic
    /// diagnostics (`rows_cached`; nothing is cached under `noCheck` or
    /// when an earlier gate closed the getter).
    pub(crate) fn store_check(&self, facts: &IncrementalCheckFacts, rows_cached: bool) {
        let Some(file_name) = &self.state_file_name else {
            return;
        };
        let mut guard = self.state();
        let state = guard.get_or_insert_with(|| {
            let assembly = Assembly::new(
                self.prepared,
                &facts.files,
                file_name,
                self.policy.hash_with_text,
            );
            DriverState {
                snapshot: Snapshot::new(
                    assembly.program_state(self.policy.build),
                    self.old.as_deref(),
                ),
                facts: facts.files.clone(),
                check_global_rows: false,
                planner_global_rows: false,
                differing_only_in_map: Vec::new(),
            }
        });
        state.planner_global_rows |= facts.planner_global_rows;
        state.check_global_rows |= facts.check_global_rows;
        if rows_cached {
            let assembly = Assembly::new(
                self.prepared,
                &facts.files,
                file_name,
                self.policy.hash_with_text,
            );
            for (index, file) in facts.files.iter().enumerate() {
                let Some(rows) = &file.semantic_rows else {
                    continue;
                };
                state.snapshot.store_fresh_rows(
                    index,
                    rows.iter().map(|row| assembly.cached(index, row)).collect(),
                );
            }
        }
        state.snapshot.finish_check();
    }

    /// The semantic diagnostics of the files the check did not cover, from
    /// the old state (tsgo `getSemanticDiagnosticsOfFile` from the cache:
    /// the rows after the `noEmit` filter, then the file's include-processor
    /// rows); none under `noCheck` (tsgo's getter returns nothing then).
    pub(crate) fn cached_semantic_diagnostics(&self) -> Vec<Diagnostic> {
        if self.prepared.compiler_options().no_check == Some(true) {
            return Vec::new();
        }
        let mut guard = self.state();
        let (Some(state), Some(file_name)) = (guard.as_mut(), &self.state_file_name) else {
            return Vec::new();
        };
        let assembly = Assembly::new(
            self.prepared,
            &state.facts,
            file_name,
            self.policy.hash_with_text,
        );
        let no_emit = self.prepared.compiler_options().no_emit == Some(true);
        let mut diagnostics = Vec::new();
        for index in 0..self.prepared.source_files().len() {
            state
                .snapshot
                .convert_old_semantic_rows(index, |row| assembly.repopulated(index, row));
            let Some(rows) = state.snapshot.cached_rows(index) else {
                continue;
            };
            diagnostics.extend(
                rows.iter()
                    .filter(|row| !(no_emit && row.skipped_on_no_emit))
                    .map(|row| assembly.to_diagnostic(index, row)),
            );
            if let Some(facts) = state.facts.get(index) {
                if !facts.skipped {
                    diagnostics.extend(facts.program_rows.iter().cloned());
                }
            }
        }
        diagnostics
    }

    /// The files whose declaration diagnostics the command computes (tsgo
    /// `emitFilesIncremental(isForDtsErrors)`: the pending ones with the
    /// errors bit), per Program file; `None` without an old state (every
    /// file).
    pub(crate) fn declaration_file_filter(&self) -> Option<Vec<bool>> {
        self.old.as_ref()?;
        let guard = self.state();
        let state = guard.as_ref()?;
        Some(
            state
                .snapshot
                .program()
                .files
                .iter()
                .enumerate()
                .map(|(index, file)| {
                    file.may_be_emitted
                        && state.snapshot.pending_emit_kind(index) & FileEmitKind::DTS_ERRORS != 0
                })
                .collect(),
        )
    }

    /// The declaration getter's results committed (tsgo
    /// `emitFilesIncremental(isForDtsErrors)`): the rows of the files it
    /// covered are cached; the rows cached for the other files are returned
    /// for the report.
    pub(crate) fn record_declaration_diagnostics(&self, rows: &[Diagnostic]) -> Vec<Diagnostic> {
        let mut guard = self.state();
        let (Some(state), Some(file_name)) = (guard.as_mut(), &self.state_file_name) else {
            return Vec::new();
        };
        let assembly = Assembly::new(
            self.prepared,
            &state.facts,
            file_name,
            self.policy.hash_with_text,
        );
        let by_file = assembly.rows_by_file(rows);
        if self.old.is_none() {
            let (updates, deleted) = fresh_emit_updates(
                &state.snapshot,
                FileState::NoEmit {
                    declaration_diagnostics: Some(&by_file),
                },
            );
            state.snapshot.record_emit(updates, &deleted);
            return Vec::new();
        }
        // tsgo converts the cached rows it reports (toDiagnostic).
        for index in 0..self.prepared.source_files().len() {
            state
                .snapshot
                .convert_old_emit_rows(index, |row| assembly.repopulated(index, row));
        }
        let files = &state.snapshot.program().files;
        let mut updates = Vec::new();
        let mut deleted = Vec::new();
        let mut updated = BTreeSet::new();
        for (index, file) in files.iter().enumerate() {
            let pending = state.snapshot.pending_emit_kind(index);
            if pending == FileEmitKind::NONE {
                continue;
            }
            if !file.may_be_emitted {
                deleted.push(index);
                continue;
            }
            if pending & FileEmitKind::DTS_ERRORS != 0 {
                updated.insert(index);
                updates.push(EmitUpdate {
                    file: index,
                    emitted_kind: FileEmitKind::DTS_ERRORS,
                    from_cache: false,
                    diagnostics: by_file.get(index).cloned().unwrap_or_default(),
                    declaration: None,
                });
            }
        }
        // Get updated errors that were not included in affected files emit
        let mut from_cache = Vec::new();
        for (index, file) in files.iter().enumerate() {
            if updated.contains(&index) {
                continue;
            }
            let Some(rows) = state.snapshot.emit_rows(index) else {
                continue;
            };
            if !file.may_be_emitted {
                deleted.push(index);
                continue;
            }
            match rows {
                CachedRows::Old(rows) => {
                    from_cache.extend(rows.iter().map(|row| assembly.to_diagnostic(index, row)))
                }
                CachedRows::New(_) => {}
            }
        }
        state.snapshot.record_emit(updates, &deleted);
        from_cache
    }

    /// The units the emit covers and which of their members (tsgo
    /// `emitFilesIncremental` over the pending files); `None` without an
    /// old state (the whole plan).
    pub(crate) fn unit_requests(&self, preflight: &EmitPreflight) -> Option<Vec<UnitEmitRequest>> {
        self.old.as_ref()?;
        let guard = self.state();
        let state = guard.as_ref()?;
        let declarations = emit_declarations(self.prepared.compiler_options());
        let files = &state.snapshot.program().files;
        let mut requests = Vec::new();
        for (unit_index, unit) in preflight.plan().units().iter().enumerate() {
            let mut javascript = false;
            let mut declaration = false;
            for &id in unit.root().source_files() {
                let index = id.index();
                if !files.get(index).is_some_and(|file| file.may_be_emitted) {
                    continue;
                }
                let kind = state.snapshot.pending_emit_kind(index);
                javascript |= kind & FileEmitKind::ALL_JS != 0;
                declaration |= declarations && kind & FileEmitKind::ALL_DTS != 0;
            }
            if javascript || declaration {
                requests.push(UnitEmitRequest {
                    unit: unit_index,
                    javascript,
                    declaration,
                });
            }
        }
        Some(requests)
    }

    /// The old emit signatures of a composite project's files for the
    /// recording sink (tsgo `computeProgramFileChanges`' copy of the old
    /// emit signatures, as `skipDtsOutputOfComposite` reads them); `None`
    /// for other programs.
    pub(crate) fn composite_signatures(&self) -> Option<CompositeSignatures> {
        let old = self.old.as_deref()?;
        let options = self.prepared.compiler_options();
        if options.composite != Some(true)
            || tsc_incremental::options::affects_declaration_path(&old.options, options)
        {
            return None;
        }
        let old_map = old.options.declaration_map == Some(true);
        let new_map = options.declaration_map == Some(true);
        let mut signatures = HashMap::new();
        for source in self.prepared.source_files() {
            let canonical = source.path().canonical().as_js().to_string_lossy();
            if let Some(signature) = old.emit_signatures.get(canonical.as_ref()) {
                let signature = signature.for_new_options(old_map, new_map);
                signatures.insert(
                    source.path().display().to_owned(),
                    (signature.value().to_owned(), signature.is_plain()),
                );
            }
        }
        Some(signatures)
    }

    /// The emit committed (tsgo `emitFilesHandler.updateSnapshot`):
    /// `requests` and `preflight` name the units emitted (the whole plan
    /// without an old state).
    /// It returns the cached declaration diagnostics of the files the emit
    /// did not cover (tsgo reports them with the emit).
    pub(crate) fn record_emit(
        &self,
        emit: &EmitOutcome,
        records: &[DeclarationRecord],
        wrote_anything: bool,
        requests: Option<(&[UnitEmitRequest], &EmitPreflight)>,
    ) -> Vec<Diagnostic> {
        let mut guard = self.state();
        let (Some(state), Some(file_name)) = (guard.as_mut(), &self.state_file_name) else {
            return Vec::new();
        };
        state.differing_only_in_map = records
            .iter()
            .filter(|record| record.differs_only_in_map)
            .map(|record| record.output.clone())
            .collect();
        let assembly = Assembly::new(
            self.prepared,
            &state.facts,
            file_name,
            self.policy.hash_with_text,
        );
        let file_count = assembly.file_count();
        let mut declaration_outputs = vec![None; file_count];
        for record in records {
            if let Some(index) = assembly.index_of(record.source.as_js()) {
                declaration_outputs[index] = Some(DeclarationOutput {
                    signature: record.signature.clone(),
                    output_file_name: record.output.to_string_lossy().into_owned(),
                });
            }
        }
        let emit_rows = assembly.rows_by_file(emit.diagnostics());
        if self.old.is_none() {
            let facts = DeclarationEmitFacts {
                skipped: emit.emit_skipped() && !wrote_anything,
                declaration_outputs,
                emit_diagnostics: emit_rows,
            };
            let (updates, deleted) = fresh_emit_updates(&state.snapshot, FileState::Emit(&facts));
            state.snapshot.record_emit(updates, &deleted);
            return Vec::new();
        }
        if emit.emit_skipped() && !wrote_anything {
            // HandleNoEmitOptions stopped the emit: nothing changes.
            return Vec::new();
        }
        let Some((requests, preflight)) = requests else {
            return Vec::new();
        };
        // tsgo converts the cached rows it reports (toDiagnostic).
        for index in 0..file_count {
            state
                .snapshot
                .convert_old_emit_rows(index, |row| assembly.repopulated(index, row));
        }
        let files = &state.snapshot.program().files;
        let mut updates = Vec::new();
        let mut seen = BTreeSet::new();
        for request in requests {
            for &id in preflight.plan().units()[request.unit].root().source_files() {
                let index = id.index();
                if !seen.insert(index) || index >= file_count {
                    continue;
                }
                updates.push(EmitUpdate {
                    file: index,
                    emitted_kind: state.snapshot.pending_emit_kind(index),
                    from_cache: false,
                    diagnostics: emit_rows.get(index).cloned().unwrap_or_default(),
                    declaration: declaration_outputs[index].as_ref().map(|output| {
                        DeclarationEmit {
                            signature: output.signature.clone(),
                            output_file_name: output.output_file_name.clone(),
                        }
                    }),
                });
            }
        }
        let deleted = files
            .iter()
            .enumerate()
            .filter(|(index, file)| {
                !file.may_be_emitted
                    && state.snapshot.pending_emit_kind(*index) != FileEmitKind::NONE
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        // Get updated errors that were not included in affected files emit
        let cached = files
            .iter()
            .enumerate()
            .filter(|(index, file)| !seen.contains(index) && file.may_be_emitted)
            .filter_map(|(index, _)| match state.snapshot.emit_rows(index)? {
                CachedRows::Old(rows) => Some(
                    rows.iter()
                        .map(|row| assembly.to_diagnostic(index, row))
                        .collect::<Vec<_>>(),
                ),
                CachedRows::New(_) => None,
            })
            .flatten()
            .collect();
        state.snapshot.record_emit(updates, &deleted);
        cached
    }

    /// tsgo `ensureHasErrorsForState`'s non-cached errors: the command's
    /// diagnostics, a global row the check deferred into a file's rows, or
    /// an include-processor row located in a file that is checked.
    fn has_errors_outside_cache(
        &self,
        state: &DriverState,
        command: CommandDiagnosticFacts,
    ) -> bool {
        if command.config || command.syntactic || command.options || command.global {
            return true;
        }
        // tsgo `GetGlobalDiagnostics` when it writes the build info: the
        // rows the checks reported (when the command asked for the semantic
        // diagnostics) and the ones the planner's signatures reported.
        if (state.check_global_rows && command.semantic_cached()) || state.planner_global_rows {
            return true;
        }
        let Some(file_name) = &self.state_file_name else {
            return false;
        };
        let assembly = Assembly::new(
            self.prepared,
            &state.facts,
            file_name,
            self.policy.hash_with_text,
        );
        self.prepared
            .diagnostics()
            .program()
            .iter()
            .any(|diagnostic| {
                diagnostic
                    .file_name
                    .as_ref()
                    .and_then(|name| assembly.index_of(name.as_js()))
                    .is_some_and(|index| !state.facts.get(index).is_some_and(|facts| facts.skipped))
            })
    }

    /// tsgo `emitBuildInfo` up to the write: the document when the state
    /// changed since the old build info.
    /// What `tsc -b` learns from this emit.
    pub(crate) fn build_emit_facts(&self) -> BuildEmitFacts {
        let guard = self.state();
        guard
            .as_ref()
            .map(|state| BuildEmitFacts {
                has_changed_dts_file: state.snapshot.has_changed_dts_file(),
                declarations_differing_only_in_map: state.differing_only_in_map.clone(),
            })
            .unwrap_or_default()
    }

    /// A watch run's state for its next cycle (see [`WatchState`]), once
    /// the build info was assembled.
    pub(crate) fn take_watch_state(&self) -> Option<WatchState> {
        self.watch_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }

    /// The program's files as the run left them (tsgo `GetTestingData`).
    pub(crate) fn program_report(&self) -> Option<Vec<ProgramFileReport>> {
        let guard = self.state();
        let state = guard.as_ref()?;
        Some(
            self.prepared
                .source_files()
                .iter()
                .enumerate()
                .map(|(index, source)| ProgramFileReport {
                    file_name: source.path().display().to_string_lossy().into_owned(),
                    semantic_diagnostics: state.snapshot.semantic_diagnostics_state(index),
                    signature_update: state.snapshot.signature_update(index),
                })
                .collect(),
        )
    }

    pub(crate) fn build_info(&self, command: CommandDiagnosticFacts) -> Option<BuildInfoDocument> {
        let mut guard = self.state();
        let state = guard.as_mut()?;
        let outside = self.has_errors_outside_cache(state, command);
        if self.policy.watch {
            if let Some(file_name) = &self.state_file_name {
                let info = state.snapshot.to_watch_state(outside);
                *self
                    .watch_state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(WatchState {
                    info,
                    directory: directory_of(&file_name.to_string_lossy()),
                    options: self.prepared.compiler_options().clone(),
                });
            }
        }
        // tsgo `emitBuildInfo`: nothing when a referenced project writes the
        // same file (TS6377 blocks it).
        if !self.writes_build_info || self.prepared.build_info_emit_blocked() {
            return None;
        }
        let file_name = self.state_file_name.clone()?;
        let info = state.snapshot.to_build_info(outside)?;
        Some(BuildInfoDocument {
            file_name,
            text: info.to_json(),
        })
    }
}

/// The Program's facts and the conversions between the compiler's
/// diagnostics and the build info's rows (tsgo `toBuildInfo` and
/// `buildInfoDiagnosticWithFileName.toDiagnostic`).
/// tsgo `toBuildInfoRepopulateInfo`.
fn repopulate_info(repopulate: &Repopulate) -> RepopulateInfo {
    match repopulate {
        Repopulate::ModeMismatch => RepopulateInfo {
            kind: 1,
            module_reference: String::new(),
            mode: 0,
            package_name: String::new(),
        },
        Repopulate::ModuleNotFound {
            module_reference,
            mode,
            package_name,
        } => RepopulateInfo {
            kind: 2,
            module_reference: module_reference.to_string_lossy().into_owned(),
            mode: *mode,
            package_name: package_name
                .as_ref()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
        },
    }
}

struct Assembly<'p> {
    prepared: &'p PreparedProgram,
    facts: &'p [IncrementalFileFacts],
    /// Versions and signatures carry their text (tsgo `hashWithText`).
    hash_with_text: bool,
    build_info_file_name: String,
    index_by_name: HashMap<JsString, usize>,
    index_by_canonical: HashMap<String, usize>,
    /// The `type` of every package.json the program read, by canonical path.
    package_types: HashMap<&'p tsc_program::CanonicalPath, PackageJsonType>,
}

impl<'p> Assembly<'p> {
    fn new(
        prepared: &'p PreparedProgram,
        facts: &'p [IncrementalFileFacts],
        file_name: &JsString,
        hash_with_text: bool,
    ) -> Self {
        let index_by_name = prepared
            .source_files()
            .iter()
            .enumerate()
            .map(|(index, source)| (source.path().display().to_owned(), index))
            .collect();
        let index_by_canonical = prepared
            .source_files()
            .iter()
            .enumerate()
            .map(|(index, source)| {
                (
                    source
                        .path()
                        .canonical()
                        .as_js()
                        .to_string_lossy()
                        .into_owned(),
                    index,
                )
            })
            .collect();
        let package_types = prepared
            .packages()
            .map(|package| (package.package_json().canonical(), package.module_type()))
            .collect();
        Self {
            prepared,
            facts,
            hash_with_text,
            build_info_file_name: file_name.to_string_lossy().into_owned(),
            index_by_name,
            index_by_canonical,
            package_types,
        }
    }

    fn file_count(&self) -> usize {
        self.prepared.source_files().len()
    }

    fn index_of(&self, file_name: JsStr<'_>) -> Option<usize> {
        self.index_by_name.get(&file_name.to_owned()).copied()
    }

    fn file_facts(&self, index: usize) -> Option<&IncrementalFileFacts> {
        self.facts.get(index)
    }

    /// The offset of a UTF-16 position in the file's UTF-8 text (tsgo's
    /// positions are byte offsets).
    fn byte_offset(&self, file: usize, position: u32) -> u32 {
        self.prepared.source_files()[file]
            .snapshot()
            .positions()
            .utf16_to_byte(position)
            .unwrap_or(position)
    }

    /// The UTF-16 position of a byte offset in the file's text.
    fn utf16_position(&self, file: usize, offset: u32) -> u32 {
        self.prepared.source_files()[file]
            .snapshot()
            .positions()
            .byte_to_utf16(offset)
            .unwrap_or(offset)
    }

    fn category(category: DiagnosticCategory) -> u32 {
        match category {
            DiagnosticCategory::Warning => 0,
            DiagnosticCategory::Error => 1,
            DiagnosticCategory::Suggestion => 2,
            DiagnosticCategory::Message => 3,
        }
    }

    fn category_of(category: u32) -> DiagnosticCategory {
        match category {
            0 => DiagnosticCategory::Warning,
            1 => DiagnosticCategory::Error,
            2 => DiagnosticCategory::Suggestion,
            _ => DiagnosticCategory::Message,
        }
    }

    /// tsgo `diagnostics.Category.Name`.
    fn category_name(category: DiagnosticCategory) -> &'static str {
        match category {
            DiagnosticCategory::Warning => "warning",
            DiagnosticCategory::Error => "error",
            DiagnosticCategory::Suggestion => "suggestion",
            DiagnosticCategory::Message => "message",
        }
    }

    /// tsgo `toBuildInfoDiagnosticsFromDiagnostics` of a chain entry: its
    /// file and location are the diagnostic's.
    fn chain_entry(
        &self,
        owner: usize,
        chain: &MessageChain,
        file: Option<u32>,
        no_file: bool,
        pos: u32,
        end: u32,
    ) -> BuildInfoDiagnostic {
        let metadata = by_code(chain.code);
        BuildInfoDiagnostic {
            file,
            no_file,
            pos,
            end,
            code: chain.code,
            category: Self::category(chain.category),
            message_key: chain.key.unwrap_or("").to_owned(),
            message_args: chain
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            message_chain: chain
                .next
                .iter()
                .map(|next| self.chain_entry(owner, next, file, no_file, pos, end))
                .collect(),
            // A nested entry's own related information (the head's is set by
            // `cached`).
            related_information: chain
                .related
                .iter()
                .map(|related| self.related(owner, related))
                .collect(),
            reports_unnecessary: metadata.is_some_and(|message| message.reports_unnecessary),
            reports_deprecated: metadata.is_some_and(|message| message.reports_deprecated),
            repopulate_info: chain.repopulate.as_ref().map(repopulate_info),
            ..BuildInfoDiagnostic::default()
        }
    }

    fn located(
        &self,
        owner: usize,
        file_name: Option<&JsString>,
        start: Option<u32>,
        length: Option<u32>,
    ) -> (Option<u32>, bool, u32, u32) {
        let Some(file_name) = file_name else {
            return (None, true, 0, 0);
        };
        let Some(index) = self.index_of(file_name.as_js()) else {
            return (None, true, 0, 0);
        };
        let start = start.unwrap_or(0);
        let pos = self.byte_offset(index, start);
        let end = self.byte_offset(index, start.saturating_add(length.unwrap_or(0)));
        let file = (index != owner).then(|| u32::try_from(index + 1).expect("file id"));
        (file, false, pos, end)
    }

    fn related(&self, owner: usize, related: &RelatedInfo) -> BuildInfoDiagnostic {
        let (file, no_file, pos, end) = self.located(
            owner,
            related.file_name.as_ref(),
            related.start,
            related.length,
        );
        self.chain_entry(owner, &related.message, file, no_file, pos, end)
    }

    /// tsgo `toBuildInfoDiagnosticsFromDiagnostics` of one diagnostic
    /// stored under file `owner`.
    fn cached(&self, owner: usize, diagnostic: &Diagnostic) -> BuildInfoDiagnostic {
        let (file, no_file, pos, end) = self.located(
            owner,
            diagnostic.file_name.as_ref(),
            diagnostic.start,
            diagnostic.length,
        );
        let mut entry = self.chain_entry(owner, &diagnostic.message, file, no_file, pos, end);
        entry.related_information = diagnostic
            .related
            .iter()
            .map(|related| self.related(owner, related))
            .collect();
        entry.reports_unnecessary = diagnostic.reports_unnecessary == Some(true);
        entry.reports_deprecated = diagnostic.reports_deprecated == Some(true);
        entry.skipped_on_no_emit = diagnostic.skipped_on_no_emit;
        if let Some(source) = &diagnostic.source {
            entry.source = source.clone();
        }
        entry
    }

    /// Diagnostics located in a Program file, grouped under that file.
    fn rows_by_file(&self, diagnostics: &[Diagnostic]) -> Vec<Vec<BuildInfoDiagnostic>> {
        let mut rows = vec![Vec::new(); self.file_count()];
        for diagnostic in diagnostics {
            if let Some(index) = diagnostic
                .file_name
                .as_ref()
                .and_then(|name| self.index_of(name.as_js()))
            {
                rows[index].push(self.cached(index, diagnostic));
            }
        }
        rows
    }

    /// tsgo `buildInfoDiagnosticWithFileName.toDiagnostic` as the next build
    /// info records the result: the entries of a cached row stored under
    /// file `owner` that depend on the program's package state
    /// (`repopulateInfo`) recomputed from this program. `None` when the
    /// row has no such entry.
    fn repopulated(&self, owner: usize, row: &CachedDiagnostic) -> Option<CachedDiagnostic> {
        self.repopulated_entry(Some(owner), row)
    }

    fn repopulated_entry(
        &self,
        file: Option<usize>,
        row: &CachedDiagnostic,
    ) -> Option<CachedDiagnostic> {
        // tsgo toDiagnostic's fileForDiagnostic.
        let file = if row.no_file {
            None
        } else {
            match &row.file {
                Some(path) => self.index_by_canonical.get(path).copied(),
                None => file,
            }
        };
        let converted = |entries: &[CachedDiagnostic]| {
            let entries = entries
                .iter()
                .map(|entry| self.repopulated_entry(file, entry))
                .collect::<Vec<_>>();
            entries.iter().any(Option::is_some).then_some(entries)
        };
        let keep = |converted: Option<Vec<Option<CachedDiagnostic>>>,
                    entries: &[CachedDiagnostic]| {
            match converted {
                Some(converted) => converted
                    .into_iter()
                    .zip(entries)
                    .map(|(converted, entry)| converted.unwrap_or_else(|| entry.clone()))
                    .collect(),
                None => entries.to_vec(),
            }
        };
        let chain = converted(&row.message_chain);
        let Some(info) = &row.repopulate_info else {
            let related = converted(&row.related_information);
            if chain.is_none() && related.is_none() {
                return None;
            }
            return Some(CachedDiagnostic {
                message_chain: keep(chain, &row.message_chain),
                related_information: keep(related, &row.related_information),
                ..row.clone()
            });
        };
        let message_chain = keep(chain, &row.message_chain);
        let Some(details) = file.and_then(|file| self.repopulated_details(file, info)) else {
            // tsgo toDiagnosticWithoutRepopulate.
            let related = converted(&row.related_information);
            return Some(CachedDiagnostic {
                source: String::new(),
                message_text: String::new(),
                message_chain,
                related_information: keep(related, &row.related_information),
                repopulate_info: None,
                ..row.clone()
            });
        };
        Some(CachedDiagnostic {
            file: row.file.clone(),
            no_file: row.no_file,
            pos: row.pos,
            end: row.end,
            code: details.code,
            category: Self::category(details.category),
            source: String::new(),
            message_text: String::new(),
            message_key: details.key.unwrap_or("").to_owned(),
            message_args: details
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
            message_chain,
            related_information: Vec::new(),
            reports_unnecessary: false,
            reports_deprecated: false,
            skipped_on_no_emit: false,
            repopulate_info: None,
        })
    }

    /// tsgo `repopulateDiagnosticChain`'s details for a program file.
    fn repopulated_details(&self, file: usize, info: &RepopulateInfo) -> Option<MessageChain> {
        let source = &self.prepared.source_files()[file];
        match info.kind {
            // tsgo `CreateModeMismatchDetails` over the file's metadata.
            1 => {
                let untyped_package_json = source.package_scope().and_then(|scope| {
                    (self.package_types.get(scope) == Some(&PackageJsonType::Unspecified))
                        .then(|| self.prepared.package(scope))
                        .flatten()
                        .map(|package| package.package_json().display())
                });
                Some(tsc_checker::modules::mode_mismatch_details(
                    source.path().display(),
                    untyped_package_json,
                ))
            }
            // tsgo `CreateModuleNotFoundChain` over the file's resolution.
            2 => {
                let module_reference = JsString::from(info.module_reference.as_str());
                let package_name = if info.package_name.is_empty() {
                    module_reference.clone()
                } else {
                    JsString::from(info.package_name.as_str())
                };
                let mode = match info.mode {
                    1 => ResolutionMode::CommonJs,
                    99 => ResolutionMode::EsNext,
                    _ => ResolutionMode::Unspecified,
                };
                let resolution = self
                    .prepared
                    .resolutions()
                    .require_module(&ResolutionKey::new(
                        source.path().canonical().clone(),
                        module_reference.as_js(),
                        mode,
                    ))
                    .ok();
                let alternate_result = resolution
                    .and_then(|resolution| resolution.alternate_result())
                    .map(|path| path.display());
                Some(tsc_checker::modules::module_not_found_details(
                    module_reference.as_js(),
                    package_name.as_js(),
                    tsc_checker::modules::ModuleNotFoundFacts {
                        alternate_result,
                        types_package_exists: resolution
                            .is_some_and(|resolution| resolution.types_package_exists()),
                        package_bundles_types: resolution
                            .is_some_and(|resolution| resolution.package_bundles_types()),
                    },
                    self.prepared
                        .compiler_options()
                        .emit_module_resolution_kind(),
                ))
            }
            _ => None,
        }
    }

    /// tsgo `buildInfoDiagnosticWithFileName.toDiagnostic`: a cached row
    /// stored under file `owner` as the compiler's diagnostic (its message
    /// formatted from the catalog message and the recorded arguments).
    fn to_diagnostic(&self, owner: usize, row: &CachedDiagnostic) -> Diagnostic {
        let file = if row.no_file {
            None
        } else {
            match &row.file {
                Some(path) => self.index_by_canonical.get(path).copied(),
                None => Some(owner),
            }
        };
        let (file_name, start, length) = match file {
            Some(index) => {
                let start = self.utf16_position(index, row.pos);
                let end = self.utf16_position(index, row.end);
                (
                    Some(
                        self.prepared.source_files()[index]
                            .path()
                            .display()
                            .to_owned(),
                    ),
                    Some(start),
                    Some(end.saturating_sub(start)),
                )
            }
            None => (None, None, None),
        };
        Diagnostic {
            file_name,
            start,
            length,
            message: self.message_of(owner, row),
            related_information_present: !row.related_information.is_empty(),
            related: row
                .related_information
                .iter()
                .map(|related| self.related_of(owner, related))
                .collect(),
            reports_unnecessary: row.reports_unnecessary.then_some(true),
            reports_deprecated: row.reports_deprecated.then_some(true),
            source: (!row.source.is_empty()).then(|| row.source.clone()),
            skipped_on_no_emit: row.skipped_on_no_emit,
        }
    }

    fn message_of(&self, owner: usize, row: &CachedDiagnostic) -> MessageChain {
        let category = Self::category_of(row.category);
        let mut message = match by_code(row.code) {
            Some(message) if !row.message_key.is_empty() => {
                MessageChain::new(message, &row.message_args)
            }
            _ => MessageChain {
                code: row.code,
                category,
                text: JsString::from(row.message_text.as_str()),
                key: None,
                args: row
                    .message_args
                    .iter()
                    .map(|arg| JsString::from(arg.as_str()))
                    .collect(),
                next_present: false,
                next: Vec::new(),
                related: Vec::new(),
                repopulate: None,
            },
        };
        message.code = row.code;
        message.category = category;
        if !row.message_chain.is_empty() {
            message = message.with_next(
                row.message_chain
                    .iter()
                    .map(|chain| self.message_of(owner, chain))
                    .collect(),
            );
        }
        message.related = row
            .related_information
            .iter()
            .map(|related| self.related_of(owner, related))
            .collect();
        message
    }

    fn related_of(&self, owner: usize, row: &CachedDiagnostic) -> RelatedInfo {
        let diagnostic = self.to_diagnostic(owner, row);
        RelatedInfo {
            file_name: diagnostic.file_name,
            start: diagnostic.start,
            length: diagnostic.length,
            message: diagnostic.message,
        }
    }

    /// tsgo `computeSignatureWithDiagnostics`: the hash of a file's forced
    /// declaration text (before its source map comment) followed by its
    /// declaration diagnostics (`diagnosticToStringBuilder`).
    fn declaration_signature(&self, file: usize, output: &ForcedDeclarationOutput) -> String {
        let cut = byte_cut(&output.text, output.source_map_url_position);
        let mut builder = output.text[..cut].to_owned();
        for diagnostic in &output.diagnostics {
            self.diagnostic_to_string(file, diagnostic, &mut builder);
        }
        compute_hash_with_text(&builder, self.hash_with_text)
    }

    fn diagnostic_to_string(&self, file: usize, diagnostic: &Diagnostic, builder: &mut String) {
        let located = diagnostic
            .file_name
            .as_ref()
            .and_then(|name| self.index_of(name.as_js()))
            .map(|index| (index, diagnostic.start, diagnostic.length));
        self.entry_to_string(file, &diagnostic.message, located, builder);
        for related in &diagnostic.related {
            let located = related
                .file_name
                .as_ref()
                .and_then(|name| self.index_of(name.as_js()))
                .map(|index| (index, related.start, related.length));
            self.entry_to_string(file, &related.message, located, builder);
        }
    }

    /// One entry of `diagnosticToStringBuilder`; a chain entry carries its
    /// head's file and location (tsgo `NewDiagnosticChain`).
    fn entry_to_string(
        &self,
        file: usize,
        message: &MessageChain,
        located: Option<(usize, Option<u32>, Option<u32>)>,
        builder: &mut String,
    ) {
        builder.push('\n');
        if let Some((index, start, length)) = located {
            if index != file {
                let sources = self.prepared.source_files();
                let from =
                    directory_of(&sources[file].path().canonical().as_js().to_string_lossy());
                let relative = tsc_program::relative_path_from_directory(
                    from.as_str().into(),
                    sources[index].path().canonical().as_js(),
                    self.prepared.path_context().use_case_sensitive_file_names(),
                );
                builder.push_str(&ensure_path_is_non_module_name(&relative.to_string_lossy()));
            }
            let start = start.unwrap_or(0);
            let pos = self.byte_offset(index, start);
            let end = self.byte_offset(index, start.saturating_add(length.unwrap_or(0)));
            builder.push_str(&format!("({pos},{}): ", end.saturating_sub(pos)));
        }
        builder.push_str(Self::category_name(message.category));
        builder.push_str(&format!("{}: ", message.code));
        builder.push_str(message.key.unwrap_or(""));
        builder.push('\n');
        for arg in &message.args {
            builder.push_str(&arg.to_string_lossy());
            builder.push('\n');
        }
        for chain in &message.next {
            self.entry_to_string(file, chain, located, builder);
        }
        for related in &message.related {
            let located = related
                .file_name
                .as_ref()
                .and_then(|name| self.index_of(name.as_js()))
                .map(|index| (index, related.start, related.length));
            self.entry_to_string(file, &related.message, located, builder);
        }
    }

    /// tsgo `SourceFileMetaData.ImpliedNodeFormat` as the program stores
    /// it (`loadSourceFileMetaData`): the extension decides; a `.ts`-like
    /// file follows its package scope's `type` only under a Node module
    /// resolution or inside `node_modules`.
    fn stored_implied_node_format(&self, index: usize) -> u32 {
        let source = &self.prepared.source_files()[index];
        let file_name = source.path().display().to_string_lossy().into_owned();
        let lower = file_name.to_ascii_lowercase();
        if lower.ends_with(".d.mts") || lower.ends_with(".mts") || lower.ends_with(".mjs") {
            return 99;
        }
        if lower.ends_with(".d.cts") || lower.ends_with(".cts") || lower.ends_with(".cjs") {
            return 1;
        }
        if !(lower.ends_with(".d.ts")
            || lower.ends_with(".ts")
            || lower.ends_with(".tsx")
            || lower.ends_with(".js")
            || lower.ends_with(".jsx"))
        {
            return 0;
        }
        let resolution = self
            .prepared
            .compiler_options()
            .emit_module_resolution_kind();
        let consulted = (3..=99).contains(&resolution) || file_name.contains("/node_modules/");
        if !consulted {
            return 1;
        }
        let module = source
            .package_scope()
            .is_some_and(|scope| self.package_types.get(scope) == Some(&PackageJsonType::Module));
        if module {
            99
        } else {
            1
        }
    }

    /// tsgo `toPath(name, directory)`: the canonical path of a file name
    /// resolved against a directory.
    fn to_path(&self, name: JsStr<'_>, directory: JsStr<'_>) -> Option<String> {
        let normalized =
            tsc_program::normalize_absolute_js_path_lexical(name, Some(directory)).ok()?;
        let canonical = if self.prepared.path_context().use_case_sensitive_file_names() {
            normalized
        } else {
            tsc_host::to_file_name_lower_case_js(normalized.as_js())
        };
        Some(canonical.to_string_lossy().into_owned())
    }

    /// The output of a referenced project's source (tsgo
    /// `GetParseFileRedirect`), or the path itself.
    fn redirected(&self, canonical: String) -> String {
        let Some(references) = self.prepared.program_options().project_references() else {
            return canonical;
        };
        let Ok(path) = tsc_program::CanonicalPath::from_js_normalized(canonical.as_str().into())
        else {
            return canonical;
        };
        match references
            .output_for_source(&path)
            .and_then(|output| output.output_dts())
        {
            Some(output) => self
                .to_path(output, self.prepared.current_directory().display())
                .unwrap_or(canonical),
            None => canonical,
        }
    }

    /// tsgo `getReferencedFiles`: the checker's symbol-declaring files,
    /// the path references and the resolved type reference directives.
    fn referenced_files(&self) -> Vec<Vec<String>> {
        let sources = self.prepared.source_files();
        let mut referenced: Vec<Vec<String>> = vec![Vec::new(); sources.len()];
        for (index, source) in sources.iter().enumerate() {
            let Some(facts) = self.file_facts(index) else {
                continue;
            };
            let mut files = Vec::new();
            for &file in &facts.referenced_files {
                if let Some(target) = sources.get(file as usize) {
                    files.push(
                        target
                            .path()
                            .canonical()
                            .as_js()
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
            let directory = directory_of(&source.path().display().to_string_lossy());
            for name in &facts.path_references {
                if let Some(path) = self.to_path(name.as_js(), directory.as_str().into()) {
                    files.push(path);
                }
            }
            referenced[index] = files;
        }
        for (key, resolution) in self.prepared.resolutions().type_references() {
            let TypeReferenceResolutionOrigin::Source(origin) = key.origin() else {
                continue;
            };
            let Some(index) = self
                .prepared
                .source_id(origin)
                .map(tsc_program::SourceFileId::index)
            else {
                continue;
            };
            if let ResolutionOutcome::Resolved(directive) = resolution.outcome() {
                let target = directive
                    .target()
                    .canonical()
                    .as_js()
                    .to_string_lossy()
                    .into_owned();
                referenced[index].push(self.redirected(target));
            }
        }
        for files in &mut referenced {
            files.sort_unstable();
            files.dedup();
        }
        referenced
    }

    /// `build`: the command is `tsc -b`.
    fn program_state(&self, build: bool) -> ProgramState {
        let prepared = self.prepared;
        let sources = prepared.source_files();
        let library_count = prepared.library_files().len();
        let options = prepared.compiler_options();
        let files = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let display = source.path().display().to_string_lossy().into_owned();
                let base = display.rsplit('/').next().unwrap_or(&display).to_owned();
                let default_library_name =
                    (index < library_count && base.starts_with("lib.") && base.ends_with(".d.ts"))
                        .then_some(base);
                let lower = display.to_ascii_lowercase();
                let is_declaration_file = lower.ends_with(".d.ts")
                    || lower.ends_with(".d.mts")
                    || lower.ends_with(".d.cts");
                let is_default_library = default_library_name.is_some();
                // tsgo SkipTypeChecking(file, ignoreNoCheck=true): the
                // checker's fact (which honors noCheck) when noCheck is
                // unset, else the library-check rules alone.
                let type_checking_skipped_ignoring_no_check = if options.no_check == Some(true) {
                    (options.skip_lib_check == Some(true) && is_declaration_file)
                        || (options.skip_default_lib_check == Some(true) && is_default_library)
                } else {
                    self.file_facts(index).is_some_and(|facts| facts.skipped)
                };
                ProgramFileFacts {
                    path: source
                        .path()
                        .canonical()
                        .as_js()
                        .to_string_lossy()
                        .into_owned(),
                    default_library_name,
                    version: compute_hash_with_text(source.text(), self.hash_with_text),
                    affects_global_scope: self
                        .file_facts(index)
                        .is_some_and(|facts| facts.affects_global_scope),
                    implied_node_format: self.stored_implied_node_format(index),
                    may_be_emitted: source.may_be_emitted(),
                    is_json: display.ends_with(".json"),
                    is_declaration_file,
                    is_default_library,
                    type_checking_skipped_ignoring_no_check,
                }
            })
            .collect();
        let roots = prepared
            .roots()
            .iter()
            .filter_map(|root| {
                let source = root.source()?;
                Some((
                    root.path()
                        .canonical()
                        .as_js()
                        .to_string_lossy()
                        .into_owned(),
                    source.index(),
                ))
            })
            .collect();
        let (mut package_jsons, mut missing_package_jsons) = (Vec::new(), Vec::new());
        if prepared.program_options().config_file_path().is_some() {
            for probe in prepared.package_json_probes() {
                let path = probe.path.to_string_lossy().into_owned();
                if probe.exists {
                    package_jsons.push(path);
                } else if path.contains("/node_modules/") {
                    missing_package_jsons.push(path);
                }
            }
        }
        package_jsons.sort_unstable();
        package_jsons.dedup();
        missing_package_jsons.sort_unstable();
        missing_package_jsons.dedup();
        ProgramState {
            files,
            roots,
            referenced_files: self.referenced_files(),
            options: options.clone(),
            build_info_file_name: self.build_info_file_name.clone(),
            build,
            root_file_names: prepared
                .roots()
                .iter()
                .map(|root| {
                    root.path()
                        .canonical()
                        .as_js()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect(),
            current_directory: prepared
                .current_directory()
                .display()
                .to_string_lossy()
                .into_owned(),
            use_case_sensitive_file_names: prepared.path_context().use_case_sensitive_file_names(),
            package_jsons,
            missing_package_jsons,
        }
    }
}
