//! The build info of an incremental program (tsgo `execute/incremental`):
//! the Program's facts, the checker's per-file facts and the emit's
//! declaration outputs assembled into the document `tsc_incremental`
//! serializes, and the sink wrapper that hashes the declaration files as
//! they are written (tsgo's emit signatures).

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use tsc_checker::{IncrementalCheckFacts, IncrementalFileFacts};
use tsc_diagnostics::{
    by_code, Diagnostic, DiagnosticCategory, JsStr, JsString, MessageChain, RelatedInfo,
};
use tsc_emitter::{
    EmitArtifact, EmitArtifactKind, EmitBuildInfoMetadata, EmitIoError, EmitOutcome,
    EmitWriteDisposition, EmitWriteMetadata, OutputSink, SharedOutputSink,
};
use tsc_incremental::{
    build_fresh_build_info, compute_hash, BuildInfoDiagnostic, DeclarationEmitFacts,
    DeclarationOutput, FileState, FreshSnapshotInput, ProgramFileFacts, ProgramState,
};
use tsc_program::{
    PackageJsonType, PreparedProgram, ResolutionOutcome, TypeReferenceResolutionOrigin,
};

use crate::ProgramDiagnostics;

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
    fn semantic_cached(self) -> bool {
        !self.syntactic && !self.options && !self.global
    }
}

/// One declaration file the emit wrote: the source it was emitted from, the
/// output file and the signature (the hash of the text up to its source map
/// comment).
#[derive(Clone, Debug)]
pub(crate) struct DeclarationRecord {
    source: JsString,
    output: JsString,
    signature: String,
}

fn record_of(artifact: &EmitArtifact) -> Option<DeclarationRecord> {
    if artifact.kind() != EmitArtifactKind::Declaration {
        return None;
    }
    let source = artifact.source_files()?.first()?.clone();
    let text = artifact.callback_text();
    let mut cut = text.len();
    if let Some(EmitWriteMetadata::Text(metadata)) = artifact.metadata() {
        if let Some(position) = metadata.source_map_url_position() {
            let mut units = 0u32;
            for (offset, ch) in text.char_indices() {
                if units >= position.value() {
                    cut = offset;
                    break;
                }
                units += ch.len_utf16() as u32;
            }
        }
    }
    Some(DeclarationRecord {
        source,
        output: artifact.path().to_owned(),
        signature: compute_hash(&text.as_bytes()[..cut]),
    })
}

/// The shared half of [`SignatureRecordingSink`].
pub(crate) struct RecordingSharedSink<'s> {
    inner: &'s dyn SharedOutputSink,
    records: Mutex<Vec<DeclarationRecord>>,
    writes: AtomicUsize,
}

impl SharedOutputSink for RecordingSharedSink<'_> {
    fn write_shared(&self, artifact: EmitArtifact) -> Result<EmitWriteDisposition, EmitIoError> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        if let Some(record) = record_of(&artifact) {
            self.records
                .lock()
                .expect("declaration records are never poisoned")
                .push(record);
        }
        self.inner.write_shared(artifact)
    }
}

/// An output sink that records the signature of every declaration file
/// written through it (tsgo `emitFilesHandler.getEmitOptions`' write hook)
/// and forwards the write.
pub(crate) struct SignatureRecordingSink<'s> {
    ordered: Option<&'s mut dyn OutputSink>,
    shared: Option<RecordingSharedSink<'s>>,
    eager_source_roots: bool,
    records: Vec<DeclarationRecord>,
    writes: usize,
}

impl<'s> SignatureRecordingSink<'s> {
    pub(crate) fn new(sink: &'s mut dyn OutputSink) -> Self {
        let eager_source_roots = sink.writes_source_roots_eagerly();
        if sink.shared().is_some() {
            let shared = sink
                .shared()
                .expect("the shared handle was present a moment ago");
            Self {
                ordered: None,
                shared: Some(RecordingSharedSink {
                    inner: shared,
                    records: Mutex::new(Vec::new()),
                    writes: AtomicUsize::new(0),
                }),
                eager_source_roots,
                records: Vec::new(),
                writes: 0,
            }
        } else {
            Self {
                ordered: Some(sink),
                shared: None,
                eager_source_roots,
                records: Vec::new(),
                writes: 0,
            }
        }
    }

    /// Every declaration file written so far, in write order.
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
        if let Some(record) = record_of(&artifact) {
            self.records.push(record);
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
    tsc_program::build_info_file_name(
        prepared.compiler_options(),
        prepared
            .program_options()
            .config_file_path()
            .map(|path| path.display()),
        prepared.current_directory().display(),
        prepared.path_context().use_case_sensitive_file_names(),
    )
}

/// The document of a `--noEmit` command.
pub(crate) fn no_emit_build_info(
    prepared: &PreparedProgram,
    facts: &IncrementalCheckFacts,
    command: CommandDiagnosticFacts,
    declaration_diagnostics: Option<&[Diagnostic]>,
) -> Option<BuildInfoDocument> {
    let file_name = build_info_file_name(prepared)?;
    let assembly = Assembly::new(prepared, facts, &file_name);
    let declaration_rows = declaration_diagnostics.map(|rows| assembly.rows_by_file(rows));
    let state = FileState::NoEmit {
        declaration_diagnostics: declaration_rows.as_deref(),
    };
    Some(assembly.document(file_name, command, state))
}

/// The document of an emitting command, after its emit.
pub(crate) fn emit_build_info(
    prepared: &PreparedProgram,
    facts: &IncrementalCheckFacts,
    command: CommandDiagnosticFacts,
    emit: &EmitOutcome,
    records: &[DeclarationRecord],
    wrote_anything: bool,
) -> Option<BuildInfoDocument> {
    let file_name = build_info_file_name(prepared)?;
    let assembly = Assembly::new(prepared, facts, &file_name);
    let mut declaration_outputs = vec![None; assembly.file_count()];
    for record in records {
        if let Some(index) = assembly.index_of(record.source.as_js()) {
            declaration_outputs[index] = Some(DeclarationOutput {
                signature: record.signature.clone(),
                output_file_name: record.output.to_string_lossy().into_owned(),
            });
        }
    }
    let emit_facts = DeclarationEmitFacts {
        skipped: emit.emit_skipped() && !wrote_anything,
        declaration_outputs,
        emit_diagnostics: assembly.rows_by_file(emit.diagnostics()),
    };
    Some(assembly.document(file_name, command, FileState::Emit(&emit_facts)))
}

struct Assembly<'p> {
    prepared: &'p PreparedProgram,
    facts: &'p IncrementalCheckFacts,
    build_info_file_name: String,
    index_by_name: HashMap<JsString, usize>,
    /// The `type` of every package.json the program read, by canonical path.
    package_types: HashMap<&'p tsc_program::CanonicalPath, PackageJsonType>,
}

impl<'p> Assembly<'p> {
    fn new(
        prepared: &'p PreparedProgram,
        facts: &'p IncrementalCheckFacts,
        file_name: &JsString,
    ) -> Self {
        let index_by_name = prepared
            .source_files()
            .iter()
            .enumerate()
            .map(|(index, source)| (source.path().display().to_owned(), index))
            .collect();
        let package_types = prepared
            .packages()
            .map(|package| (package.package_json().canonical(), package.module_type()))
            .collect();
        Self {
            prepared,
            facts,
            build_info_file_name: file_name.to_string_lossy().into_owned(),
            index_by_name,
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
        self.facts.files.get(index)
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

    fn category(category: DiagnosticCategory) -> u32 {
        match category {
            DiagnosticCategory::Warning => 0,
            DiagnosticCategory::Error => 1,
            DiagnosticCategory::Suggestion => 2,
            DiagnosticCategory::Message => 3,
        }
    }

    /// tsgo `toBuildInfoDiagnosticsFromDiagnostics` of a chain entry: its
    /// file and location are the diagnostic's.
    fn chain_entry(
        &self,
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
                .map(|next| self.chain_entry(next, file, no_file, pos, end))
                .collect(),
            reports_unnecessary: metadata.is_some_and(|message| message.reports_unnecessary),
            reports_deprecated: metadata.is_some_and(|message| message.reports_deprecated),
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
        self.chain_entry(&related.message, file, no_file, pos, end)
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
        let mut entry = self.chain_entry(&diagnostic.message, file, no_file, pos, end);
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

    /// The rows the program cached per file: the checker's rows when the
    /// command asked for the semantic diagnostics, nothing otherwise.
    fn semantic_rows(
        &self,
        command: CommandDiagnosticFacts,
    ) -> Vec<Option<Vec<BuildInfoDiagnostic>>> {
        // tsgo GetSemanticDiagnostics caches nothing under noCheck.
        let no_check = self.prepared.compiler_options().no_check == Some(true);
        (0..self.file_count())
            .map(|index| {
                if !command.semantic_cached() || no_check {
                    return None;
                }
                let rows = self.file_facts(index)?.semantic_rows.as_ref()?;
                Some(
                    rows.iter()
                        .map(|diagnostic| self.cached(index, diagnostic))
                        .collect(),
                )
            })
            .collect()
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

    fn program_state(&self) -> ProgramState {
        let prepared = self.prepared;
        let sources = prepared.source_files();
        let library_count = prepared.library_files().len();
        let files = sources
            .iter()
            .enumerate()
            .map(|(index, source)| {
                let display = source.path().display().to_string_lossy().into_owned();
                let base = display.rsplit('/').next().unwrap_or(&display).to_owned();
                let default_library_name =
                    (index < library_count && base.starts_with("lib.") && base.ends_with(".d.ts"))
                        .then_some(base);
                ProgramFileFacts {
                    path: source
                        .path()
                        .canonical()
                        .as_js()
                        .to_string_lossy()
                        .into_owned(),
                    default_library_name,
                    version: compute_hash(source.text().as_bytes()),
                    affects_global_scope: self
                        .file_facts(index)
                        .is_some_and(|facts| facts.affects_global_scope),
                    implied_node_format: self.stored_implied_node_format(index),
                    may_be_emitted: source.may_be_emitted(),
                    is_json: display.ends_with(".json"),
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
            options: prepared.compiler_options().clone(),
            build_info_file_name: self.build_info_file_name.clone(),
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

    /// tsgo `ensureHasErrorsForState`'s non-cached errors: the command's
    /// diagnostics, a global row the check deferred into a file's rows, or
    /// an include-processor row located in a file that is checked.
    fn has_errors_outside_cache(&self, command: CommandDiagnosticFacts) -> bool {
        if command.config || command.syntactic || command.options || command.global {
            return true;
        }
        let deferred_global = self.facts.files.iter().any(|facts| {
            facts
                .semantic_rows
                .as_ref()
                .is_some_and(|rows| rows.iter().any(|row| row.file_name.is_none()))
        });
        if deferred_global && command.semantic_cached() {
            return true;
        }
        self.prepared
            .diagnostics()
            .program()
            .iter()
            .any(|diagnostic| {
                diagnostic
                    .file_name
                    .as_ref()
                    .and_then(|name| self.index_of(name.as_js()))
                    .is_some_and(|index| !self.file_facts(index).is_some_and(|facts| facts.skipped))
            })
    }

    fn document(
        &self,
        file_name: JsString,
        command: CommandDiagnosticFacts,
        state: FileState<'_>,
    ) -> BuildInfoDocument {
        let program = self.program_state();
        let semantic_rows = self.semantic_rows(command);
        let info = build_fresh_build_info(&FreshSnapshotInput {
            program: &program,
            semantic_rows: &semantic_rows,
            state,
            has_errors_outside_cache: self.has_errors_outside_cache(command),
        });
        BuildInfoDocument {
            file_name,
            text: info.to_json(),
        }
    }
}

fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}
