//! The incremental program's state over one compilation (tsgo `snapshot`:
//! programtosnapshot.go's change computation from the old state,
//! affectedfileshandler.go's affected files, the emit handler's updates and
//! snapshottobuildinfo.go's serialization), starting from the old build
//! info's state or from nothing.
//!
//! The compiler driver supplies the facts and runs the checker and the
//! emitter; nothing here reads a file, checks a source or prints a
//! declaration. Where tsgo asks its checker or emitter for something (a
//! declaration signature, whether a file exports a const enum), the driver
//! passes a closure.

use std::collections::{BTreeMap, BTreeSet};

use tsc_types::CompilerOptions;

use crate::build_info::{
    BuildInfo, BuildInfoDiagnostic, BuildInfoRoot, EmitSignatureEntry, FileId, FileInfoEntry,
    SemanticDiagnosticEntry, VERSION,
};
use crate::old_state::{CachedDiagnostic, EmitSignature, OldState};
use crate::options::{build_info_options, emit_declarations, is_incremental};

/// tsgo `FileEmitKind`: the bits of what a file still has to emit.
pub struct FileEmitKind;

impl FileEmitKind {
    pub const NONE: u32 = 0;
    pub const JS: u32 = 1 << 0;
    pub const JS_MAP: u32 = 1 << 1;
    pub const JS_INLINE_MAP: u32 = 1 << 2;
    pub const DTS_ERRORS: u32 = 1 << 3;
    pub const DTS_EMIT: u32 = 1 << 4;
    pub const DTS_MAP: u32 = 1 << 5;
    pub const DTS: u32 = Self::DTS_ERRORS | Self::DTS_EMIT;
    pub const ALL_JS: u32 = Self::JS | Self::JS_MAP | Self::JS_INLINE_MAP;
    pub const ALL_DTS_EMIT: u32 = Self::DTS_EMIT | Self::DTS_MAP;
    pub const ALL_DTS: u32 = Self::DTS | Self::DTS_MAP;
    pub const ALL: u32 = Self::ALL_JS | Self::ALL_DTS;

    /// tsgo `GetFileEmitKind`: everything the options emit.
    pub fn of_options(options: &CompilerOptions) -> u32 {
        let mut result = Self::JS;
        if options.source_map == Some(true) {
            result |= Self::JS_MAP;
        }
        if options.inline_source_map == Some(true) {
            result |= Self::JS_INLINE_MAP;
        }
        if emit_declarations(options) {
            result |= Self::DTS;
        }
        if options.declaration_map == Some(true) {
            result |= Self::DTS_MAP;
        }
        if options.emit_declaration_only == Some(true) {
            result &= Self::ALL_DTS;
        }
        result
    }

    /// tsgo `getPendingEmitKind`: what remains of `emit_kind` once
    /// `old_emit_kind` has been emitted.
    pub fn pending(emit_kind: u32, old_emit_kind: u32) -> u32 {
        if old_emit_kind == emit_kind {
            return Self::NONE;
        }
        if old_emit_kind == 0 || emit_kind == 0 {
            return emit_kind;
        }
        let diff = old_emit_kind ^ emit_kind;
        let mut result = Self::NONE;
        if diff & Self::ALL_JS != 0 {
            result |= emit_kind & Self::ALL_JS;
        }
        if diff & Self::DTS_ERRORS != 0 {
            result |= emit_kind & Self::ALL_DTS;
        }
        if diff & Self::ALL_DTS_EMIT != 0 {
            result |= emit_kind & Self::ALL_DTS_EMIT;
        }
        result
    }
}

/// One program file as the snapshot sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramFileFacts {
    /// The file's canonical path (tsgo `tspath.Path`).
    pub path: String,
    /// The bare library file name when the file is a default library the
    /// program did not replace (tsgo `LibFile.Name`); the build info names
    /// such a file by it.
    pub default_library_name: Option<String>,
    /// The hash of the file's text.
    pub version: String,
    /// tsgo `fileAffectsGlobalScope`.
    pub affects_global_scope: bool,
    /// tsgo `SourceFileMetaData.ImpliedNodeFormat` (0 none, 1 CommonJS, 99
    /// ESM).
    pub implied_node_format: u32,
    /// tsgo `SourceFileMayBeEmitted(file, false)`.
    pub may_be_emitted: bool,
    pub is_json: bool,
    pub is_declaration_file: bool,
    /// tsgo `IsSourceFileDefaultLibrary`.
    pub is_default_library: bool,
    /// tsgo `SkipTypeChecking(file, ignoreNoCheck=true)`.
    pub type_checking_skipped_ignoring_no_check: bool,
}

/// The program's facts the snapshot and its serialization read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgramState {
    /// The files in Program order.
    pub files: Vec<ProgramFileFacts>,
    /// The config's root file names that resolved to a program file: the
    /// root's canonical path and the file's index (the output of a
    /// referenced project's source when the root was redirected).
    pub roots: Vec<(String, usize)>,
    /// Per file: the canonical paths of the files it references (tsgo
    /// `getReferencedFiles`); a path need not be a program file.
    pub referenced_files: Vec<Vec<String>>,
    pub options: CompilerOptions,
    /// The absolute, normalized build info file name.
    pub build_info_file_name: String,
    /// `tsc -b` (tsgo `CompilerOptions.Build`): a non-incremental program
    /// then keeps no incremental state and writes the non-incremental
    /// build info (`canUseIncrementalState`).
    pub build: bool,
    /// The config's file names as canonical paths, in config order (the
    /// roots of a non-incremental build info).
    pub root_file_names: Vec<String>,
    pub current_directory: String,
    pub use_case_sensitive_file_names: bool,
    /// The package.json files the resolver read and the ones under
    /// `node_modules` it did not find, as absolute paths, sorted and
    /// deduplicated (tsgo `ensurePackageJsonsForState`).
    pub package_jsons: Vec<String>,
    pub missing_package_jsons: Vec<String>,
}

/// tsgo `FileInfo` of the current program; an empty signature is none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileInfo {
    pub version: String,
    pub signature: String,
    pub affects_global_scope: bool,
    pub implied_node_format: u32,
}

/// The rows cached for a file: the old state's, or this compilation's.
#[derive(Clone, Debug, PartialEq)]
pub enum CachedRows {
    Old(Vec<CachedDiagnostic>),
    New(Vec<BuildInfoDiagnostic>),
}

impl CachedRows {
    fn is_empty(&self) -> bool {
        match self {
            Self::Old(rows) => rows.is_empty(),
            Self::New(rows) => rows.is_empty(),
        }
    }
}

/// The declaration file an emit produced for a file (tsgo's write hook in
/// `emitFilesHandler.getEmitOptions`): the hash of its text up to the
/// source map comment, and its name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationEmit {
    pub signature: String,
    pub output_file_name: String,
}

/// What the emit (or the declaration-diagnostics getter) did for one file
/// (tsgo `emitUpdate`).
#[derive(Clone, Debug, PartialEq)]
pub struct EmitUpdate {
    pub file: usize,
    /// The bits of the file's pending kind that were handled.
    pub emitted_kind: u32,
    /// The declaration diagnostics came from the cache (nothing was
    /// re-emitted; the pending kind stays).
    pub from_cache: bool,
    pub diagnostics: Vec<BuildInfoDiagnostic>,
    pub declaration: Option<DeclarationEmit>,
}

/// Whether a composite project's declaration file is written (tsgo
/// `skipDtsOutputOfComposite`): an unchanged one is not, and a changed one
/// becomes the latest changed declaration file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclarationWrite {
    pub skip: bool,
    pub changed: bool,
}

/// tsgo `skipDtsOutputOfComposite`'s decision for one declaration file,
/// from the old emit signature (its hash and whether it is in the plain
/// form) and the new hash.
pub fn declaration_write_decision(
    composite: bool,
    old: Option<(&str, bool)>,
    new_signature: &str,
) -> DeclarationWrite {
    if !composite {
        return DeclarationWrite {
            skip: false,
            changed: false,
        };
    }
    match old {
        Some((old_signature, plain)) if old_signature == new_signature => DeclarationWrite {
            skip: plain,
            changed: false,
        },
        _ => DeclarationWrite {
            skip: false,
            changed: true,
        },
    }
}

/// What the old state recorded that the new one compares itself with.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct OldSummary {
    has_errors: bool,
    has_semantic_errors: bool,
    package_jsons: Vec<String>,
    missing_package_jsons: Vec<String>,
}

/// How a file's semantic diagnostics stand after a run: tsgo's testing data
/// compares the run's map with the old program's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticDiagnosticsState {
    /// Computed by this run (`*refresh*`).
    Refreshed,
    /// Kept from the old state.
    Kept,
    /// Not cached (`*not cached*`).
    NotCached,
}

/// tsgo `SignatureUpdateKind`: how this run updated a file's signature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureUpdateKind {
    /// From the declaration output of the affected-file walk.
    ComputedDts,
    /// From the declaration file the emit wrote.
    StoredAtEmit,
    /// The file's version stands for its shape.
    UsedVersion,
}

/// tsgo `snapshot`.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    program: ProgramState,
    index_by_path: BTreeMap<String, usize>,
    file_infos: Vec<FileInfo>,
    /// tsgo `changedFilesSet`, by path (an old path need not be a program
    /// file).
    changed: BTreeSet<String>,
    semantic: Vec<Option<CachedRows>>,
    emit_rows: Vec<Option<CachedRows>>,
    /// tsgo `affectedFilesPendingEmit`, by path.
    pending_emit: BTreeMap<String, u32>,
    /// Absolute.
    latest_changed_dts_file: Option<String>,
    emit_signatures: Vec<Option<EmitSignature>>,
    has_errors: Option<bool>,
    has_semantic_errors: bool,
    check_pending: bool,
    package_jsons: Option<(Vec<String>, Vec<String>)>,
    build_info_emit_pending: bool,
    /// tsgo `hasChangedDtsFile`: this run's emit wrote a declaration file
    /// whose signature changed.
    has_changed_dts_file: bool,
    old: Option<OldSummary>,
    global_file_removed: bool,
    /// Per file: the files referencing it (the inverse of
    /// `referenced_files` over program files).
    referenced_by: Vec<Vec<usize>>,
    /// The semantic check stored rows for some file.
    stored_rows: bool,
    /// tsgo `TestingData.UpdatedSignatureKinds`.
    signature_updates: BTreeMap<usize, SignatureUpdateKind>,
}

impl Snapshot {
    /// tsgo `programToSnapshot`.
    pub fn new(program: ProgramState, old: Option<&OldState>) -> Self {
        let file_count = program.files.len();
        let index_by_path: BTreeMap<String, usize> = program
            .files
            .iter()
            .enumerate()
            .map(|(index, file)| (file.path.clone(), index))
            .collect();
        let mut referenced_by = vec![Vec::new(); file_count];
        for (file, references) in program.referenced_files.iter().enumerate() {
            for reference in references {
                if let Some(&target) = index_by_path.get(reference) {
                    referenced_by[target].push(file);
                }
            }
        }
        let options = program.options.clone();
        let composite = options.composite == Some(true);
        let full_kind = FileEmitKind::of_options(&options);
        let mut snapshot = Snapshot {
            index_by_path,
            file_infos: Vec::with_capacity(file_count),
            changed: BTreeSet::new(),
            semantic: vec![None; file_count],
            emit_rows: vec![None; file_count],
            pending_emit: BTreeMap::new(),
            latest_changed_dts_file: None,
            emit_signatures: vec![None; file_count],
            has_errors: None,
            has_semantic_errors: false,
            check_pending: options.no_check == Some(true),
            package_jsons: None,
            build_info_emit_pending: false,
            has_changed_dts_file: false,
            old: None,
            global_file_removed: false,
            referenced_by,
            stored_rows: false,
            signature_updates: BTreeMap::new(),
            program,
        };

        // reuseFromOldProgram
        match old {
            Some(old) => {
                if composite {
                    snapshot.latest_changed_dts_file = old.latest_changed_dts_file.clone();
                }
                snapshot.changed = old.changed_files.clone();
                snapshot.pending_emit = old.pending_emit.clone();
                snapshot.old = Some(OldSummary {
                    has_errors: old.has_errors,
                    has_semantic_errors: old.has_semantic_errors,
                    package_jsons: old.package_jsons.clone(),
                    missing_package_jsons: old.missing_package_jsons.clone(),
                });
            }
            None => snapshot.build_info_emit_pending = is_incremental(&options),
        }

        // computeProgramFileChanges
        let can_copy_semantic = old.is_some_and(|old| {
            !crate::options::affects_semantic_diagnostics(&old.options, &options)
        });
        let can_copy_emit_signatures = composite
            && old.is_some_and(|old| {
                !crate::options::affects_declaration_path(&old.options, &options)
            });
        let copy_declaration_rows = can_copy_semantic
            && old.is_some_and(|old| {
                (old.options.skip_lib_check == Some(true)) == (options.skip_lib_check == Some(true))
            });
        let copy_lib_rows = copy_declaration_rows
            && old.is_some_and(|old| {
                (old.options.skip_default_lib_check == Some(true))
                    == (options.skip_default_lib_check == Some(true))
            });
        for index in 0..file_count {
            let file = snapshot.program.files[index].clone();
            let path = file.path.clone();
            let mut signature = String::new();
            match old {
                Some(old) => {
                    match old.file_infos.get(&path) {
                        Some(old_info) => {
                            signature = old_info.signature.clone();
                            let new_references = snapshot.program.referenced_files[index]
                                .iter()
                                .cloned()
                                .collect::<BTreeSet<_>>();
                            let old_references = old.references.get(&path);
                            if old_info.version != file.version
                                || old_info.affects_global_scope != file.affects_global_scope
                                || old_info.implied_node_format != file.implied_node_format
                            {
                                snapshot.add_file_to_change_set(&path);
                            } else if old_references
                                .map_or(!new_references.is_empty(), |old| *old != new_references)
                            {
                                // Referenced files changed
                                snapshot.add_file_to_change_set(&path);
                            } else if new_references.iter().any(|reference| {
                                !snapshot.index_by_path.contains_key(reference)
                                    && old.file_infos.contains_key(reference)
                            }) {
                                // Referenced file was deleted in the new program
                                snapshot.add_file_to_change_set(&path);
                            }
                        }
                        None => snapshot.add_file_to_change_set(&path),
                    }
                    if !snapshot.changed.contains(&path) {
                        if let Some(rows) = old.emit_rows.get(&path) {
                            snapshot.emit_rows[index] = Some(CachedRows::Old(rows.clone()));
                        }
                        if can_copy_semantic
                            && (!file.is_declaration_file || copy_declaration_rows)
                            && (!file.is_default_library || copy_lib_rows)
                        {
                            if let Some(rows) = old.semantic_rows.get(&path) {
                                snapshot.semantic[index] = Some(CachedRows::Old(rows.clone()));
                            }
                        }
                    }
                    if can_copy_emit_signatures {
                        if let Some(old_signature) = old.emit_signatures.get(&path) {
                            snapshot.emit_signatures[index] = Some(old_signature.for_new_options(
                                old.options.declaration_map == Some(true),
                                options.declaration_map == Some(true),
                            ));
                        }
                    }
                }
                None => {
                    snapshot.add_file_to_affected_files_pending_emit(&path, full_kind);
                    signature = file.version.clone();
                }
            }
            snapshot.file_infos.push(FileInfo {
                version: file.version.clone(),
                signature,
                affects_global_scope: file.affects_global_scope,
                implied_node_format: file.implied_node_format,
            });
        }

        // handleFileDelete
        if let Some(old) = old {
            for (path, old_info) in &old.file_infos {
                if snapshot.index_by_path.contains_key(path) {
                    continue;
                }
                if old_info.affects_global_scope {
                    for file in snapshot.all_files_excluding_default_library() {
                        let path = snapshot.program.files[file].path.clone();
                        snapshot.add_file_to_change_set(&path);
                    }
                    snapshot.global_file_removed = true;
                } else {
                    snapshot.build_info_emit_pending = true;
                }
                break;
            }
        }

        // handleGlobalScopeChange
        if let Some(old) = old {
            if !snapshot.global_file_removed {
                let global_scope_lost = old.file_infos.iter().any(|(path, old_info)| {
                    old_info.affects_global_scope
                        && snapshot
                            .index_by_path
                            .get(path)
                            .is_some_and(|&index| !snapshot.file_infos[index].affects_global_scope)
                });
                if global_scope_lost {
                    for file in snapshot.all_files_excluding_default_library() {
                        let path = snapshot.program.files[file].path.clone();
                        snapshot.add_file_to_change_set(&path);
                    }
                }
            }
        }

        // handlePendingEmit
        if let Some(old) = old {
            if !snapshot.global_file_removed {
                let pending_kind = if crate::options::affects_emit(&old.options, &options) {
                    full_kind
                } else {
                    FileEmitKind::pending(full_kind, FileEmitKind::of_options(&old.options))
                };
                if pending_kind != FileEmitKind::NONE {
                    for index in 0..file_count {
                        let path = snapshot.program.files[index].path.clone();
                        if !snapshot.changed.contains(&path) {
                            snapshot.add_file_to_affected_files_pending_emit(&path, pending_kind);
                        }
                    }
                    snapshot.build_info_emit_pending = true;
                }
            }
        }

        // handlePendingCheck
        if let Some(old) = old {
            let cached = snapshot
                .semantic
                .iter()
                .filter(|rows| rows.is_some())
                .count();
            if cached != file_count && old.check_pending != snapshot.check_pending {
                snapshot.build_info_emit_pending = true;
            }
        }
        snapshot
    }

    pub fn program(&self) -> &ProgramState {
        &self.program
    }

    fn add_file_to_change_set(&mut self, path: &str) {
        self.changed.insert(path.to_owned());
        self.build_info_emit_pending = true;
    }

    fn add_file_to_affected_files_pending_emit(&mut self, path: &str, kind: u32) {
        let existing = self.pending_emit.get(path).copied().unwrap_or(0);
        self.pending_emit.insert(path.to_owned(), existing | kind);
        if kind & FileEmitKind::DTS_ERRORS != 0 {
            if let Some(&index) = self.index_by_path.get(path) {
                self.emit_rows[index] = None;
            }
        }
        self.build_info_emit_pending = true;
    }

    /// tsgo `getAllFilesExcludingDefaultLibraryFile`, in Program order.
    fn all_files_excluding_default_library(&self) -> Vec<usize> {
        (0..self.program.files.len())
            .filter(|&index| !self.program.files[index].is_default_library)
            .collect()
    }

    /// Whether any file changed since the old state (tsgo
    /// `changedFilesSet.Size() != 0`).
    pub fn has_changed_files(&self) -> bool {
        !self.changed.is_empty()
    }

    /// tsgo `collectAllAffectedFiles`: the files affected by the changed
    /// ones lose their cached rows, get their signatures updated and are
    /// queued for emit; the change set is consumed. `signature_of` returns
    /// the hash of a file's declaration output (tsgo `computeDtsSignature`)
    /// or `None` when it has none; `exports_const_enum` says whether the
    /// file's module symbol exports a const enum (through an alias declared
    /// in the file too).
    pub fn collect_all_affected_files(
        &mut self,
        signature_of: &mut dyn FnMut(usize) -> Option<String>,
        exports_const_enum: &mut dyn FnMut(usize) -> bool,
    ) {
        if self.changed.is_empty() {
            return;
        }
        let mut handler = AffectedFiles {
            snapshot: self,
            updated_signatures: BTreeMap::new(),
            has_all_files_excluding_default_library: false,
            remove_rows: BTreeSet::new(),
            cleaned_library_rows: false,
            seen_file_and_references: BTreeMap::new(),
            pending: BTreeMap::new(),
            signature_of,
        };
        let changed = handler.snapshot.changed.iter().cloned().collect::<Vec<_>>();
        let mut result = BTreeSet::new();
        for path in &changed {
            for file in handler.files_affected_by(path) {
                result.insert(file);
            }
        }
        let full_kind = FileEmitKind::of_options(&handler.snapshot.program.options);
        for &file in &result {
            // remove the cached semantic diagnostics and handle dts emit
            // and js emit if needed
            handler.pending.insert(file, full_kind);
            handler.handle_dts_may_change_of_affected_file(file, exports_const_enum);
        }
        let AffectedFiles {
            updated_signatures,
            remove_rows,
            pending,
            ..
        } = handler;
        // updateSnapshot
        for (file, (signature, kind)) in updated_signatures {
            self.file_infos[file].signature = signature;
            self.signature_updates.insert(file, kind);
        }
        for file in remove_rows {
            self.semantic[file] = None;
        }
        for (file, kind) in pending {
            let path = self.program.files[file].path.clone();
            self.add_file_to_affected_files_pending_emit(&path, kind);
        }
        self.changed.clear();
        self.build_info_emit_pending = true;
    }

    /// Where a file's semantic diagnostics came from (tsgo's testing data).
    pub fn semantic_diagnostics_state(&self, file: usize) -> SemanticDiagnosticsState {
        match self.semantic.get(file) {
            Some(Some(CachedRows::New(_))) => SemanticDiagnosticsState::Refreshed,
            Some(Some(CachedRows::Old(_))) => SemanticDiagnosticsState::Kept,
            _ => SemanticDiagnosticsState::NotCached,
        }
    }

    /// How this run updated a file's signature, if it did (tsgo's testing
    /// data).
    pub fn signature_update(&self, file: usize) -> Option<SignatureUpdateKind> {
        self.signature_updates.get(&file).copied()
    }

    /// The files whose semantic rows are not cached: the ones the program
    /// checks (tsgo `collectSemanticDiagnosticsOfAffectedFiles`).
    pub fn files_without_rows(&self) -> Vec<bool> {
        self.semantic.iter().map(Option::is_none).collect()
    }

    /// The rows cached for a file from the old state, when the file was not
    /// (re)checked.
    pub fn cached_rows(&self, file: usize) -> Option<&[CachedDiagnostic]> {
        match self.semantic.get(file)? {
            Some(CachedRows::Old(rows)) => Some(rows),
            _ => None,
        }
    }

    /// tsgo converts a file's cached diagnostics when it reports them, and
    /// its next build info records the converted ones: `convert` rewrites
    /// the file's old semantic rows (`None` leaves a row as it is).
    pub fn convert_old_semantic_rows(
        &mut self,
        file: usize,
        convert: impl Fn(&CachedDiagnostic) -> Option<CachedDiagnostic>,
    ) {
        if let Some(Some(CachedRows::Old(rows))) = self.semantic.get_mut(file) {
            convert_rows(rows, convert);
        }
    }

    /// [`Self::convert_old_semantic_rows`] for the file's old declaration
    /// emit rows.
    pub fn convert_old_emit_rows(
        &mut self,
        file: usize,
        convert: impl Fn(&CachedDiagnostic) -> Option<CachedDiagnostic>,
    ) {
        if let Some(Some(CachedRows::Old(rows))) = self.emit_rows.get_mut(file) {
            convert_rows(rows, convert);
        }
    }

    /// Store the rows the check produced for a file.
    pub fn store_fresh_rows(&mut self, file: usize, rows: Vec<BuildInfoDiagnostic>) {
        self.semantic[file] = Some(CachedRows::New(rows));
        self.stored_rows = true;
    }

    /// After the semantic check (tsgo's commit in
    /// `collectSemanticDiagnosticsOfAffectedFiles`): the check is no longer
    /// pending once every file has rows.
    pub fn finish_check(&mut self) {
        if !self.stored_rows {
            return;
        }
        if self.semantic.iter().all(Option::is_some)
            && self.check_pending
            && self.program.options.no_check != Some(true)
        {
            self.check_pending = false;
        }
        self.build_info_emit_pending = true;
    }

    /// The emit (declaration) diagnostics cached for a file.
    pub fn emit_rows(&self, file: usize) -> Option<&CachedRows> {
        self.emit_rows.get(file)?.as_ref()
    }

    /// The pending emit kind of a program file (0 when none).
    pub fn pending_emit_kind(&self, file: usize) -> u32 {
        self.pending_emit
            .get(&self.program.files[file].path)
            .copied()
            .unwrap_or(FileEmitKind::NONE)
    }

    /// tsgo `snapshot.canUseIncrementalState`: a non-incremental program
    /// built by `tsc -b` tracks nothing but its diagnostics.
    pub fn can_use_incremental_state(&self) -> bool {
        is_incremental(&self.program.options) || !self.program.build
    }

    /// tsgo `Program.HasChangedDtsFile`.
    pub fn has_changed_dts_file(&self) -> bool {
        self.has_changed_dts_file
    }

    /// The old emit signature of a file as `declaration_write_decision`
    /// reads it: its hash and whether it is in the plain form.
    pub fn old_emit_signature(&self, file: usize) -> Option<(&str, bool)> {
        self.emit_signatures
            .get(file)?
            .as_ref()
            .map(|signature| (signature.value(), signature.is_plain()))
    }

    /// tsgo `emitFilesHandler.updateSnapshot` (and the write hook of
    /// `getEmitOptions`): the emits of this run applied. `deleted` lists the
    /// pending files that are no longer emitted (tsgo
    /// `deletedPendingKinds`).
    pub fn record_emit(&mut self, updates: Vec<EmitUpdate>, deleted: &[usize]) {
        let options = self.program.options.clone();
        // tsgo's write hook computes signatures only while the incremental
        // state is in use.
        let declarations = emit_declarations(&options) && self.can_use_incremental_state();
        let composite = options.composite == Some(true);
        let mut new_signatures: BTreeMap<usize, String> = BTreeMap::new();
        let mut new_emit_signatures: BTreeMap<usize, EmitSignature> = BTreeMap::new();
        let mut latest_changed: BTreeMap<usize, String> = BTreeMap::new();
        let mut by_file: BTreeMap<usize, EmitUpdate> = BTreeMap::new();
        for update in updates {
            if let (true, Some(declaration)) = (declarations, &update.declaration) {
                let info = &self.file_infos[update.file];
                if info.signature == info.version && declaration.signature != info.version {
                    new_signatures.insert(update.file, declaration.signature.clone());
                }
                if composite {
                    let decision = declaration_write_decision(
                        composite,
                        self.old_emit_signature(update.file),
                        &declaration.signature,
                    );
                    if decision.changed {
                        latest_changed.insert(update.file, declaration.output_file_name.clone());
                    }
                    new_emit_signatures.insert(
                        update.file,
                        EmitSignature::plain(declaration.signature.clone()),
                    );
                }
            }
            by_file.insert(update.file, update);
        }
        for (file, signature) in new_signatures {
            self.file_infos[file].signature = signature;
            self.signature_updates
                .insert(file, SignatureUpdateKind::StoredAtEmit);
            self.build_info_emit_pending = true;
        }
        for (file, signature) in new_emit_signatures {
            self.emit_signatures[file] = Some(signature);
            self.build_info_emit_pending = true;
        }
        for &file in deleted {
            let path = self.program.files[file].path.clone();
            self.pending_emit.remove(&path);
            self.build_info_emit_pending = true;
        }
        // Always use correct order when to collect the result
        for file in 0..self.program.files.len() {
            if let Some(output) = latest_changed.get(&file) {
                self.latest_changed_dts_file = Some(output.clone());
                self.build_info_emit_pending = true;
                self.has_changed_dts_file = true;
            }
            let Some(update) = by_file.remove(&file) else {
                continue;
            };
            if !update.from_cache {
                let path = self.program.files[file].path.clone();
                let current = self.pending_emit_kind(file);
                let remaining = FileEmitKind::pending(current, update.emitted_kind);
                if remaining == FileEmitKind::NONE {
                    self.pending_emit.remove(&path);
                } else {
                    self.pending_emit.insert(path, remaining);
                }
                self.build_info_emit_pending = true;
            }
            if !update.diagnostics.is_empty() {
                self.emit_rows[file] = Some(CachedRows::New(update.diagnostics));
            }
        }
    }

    /// tsgo `ensureHasErrorsForState` of an incremental program;
    /// `has_errors_outside_cache`: config parsing, syntactic, program
    /// (options), global or file-located include-processor diagnostics
    /// exist.
    fn ensure_has_errors(&mut self, has_errors_outside_cache: bool) {
        if self.has_errors.is_some() {
            return;
        }
        let incremental = is_incremental(&self.program.options);
        let has_emit_rows = self.emit_rows.iter().any(Option::is_some);
        if has_emit_rows {
            // Record this for only non incremental build info
            self.has_errors = Some(!incremental);
            self.has_semantic_errors = false;
        } else if has_errors_outside_cache {
            self.has_errors = Some(true);
            self.has_semantic_errors = false;
        } else {
            self.has_errors = Some(false);
            let semantic_errors = self.semantic.iter().any(|rows| match rows {
                // Missing semantic diagnostics in cache will be encoded in
                // incremental buildInfo
                None => incremental,
                Some(rows) => !rows.is_empty(),
            });
            if semantic_errors {
                self.has_semantic_errors = !incremental;
            }
        }
        match &self.old {
            Some(old) => {
                if self.has_errors != Some(old.has_errors)
                    || self.has_semantic_errors != old.has_semantic_errors
                {
                    self.build_info_emit_pending = true;
                }
            }
            // tsgo compares with the unknown flags of a missing old state:
            // they differ.
            None => self.build_info_emit_pending = true,
        }
    }

    /// tsgo `ensurePackageJsonsForState`'s effect on the pending flag.
    fn ensure_package_jsons(&mut self) {
        if self.package_jsons.is_some() {
            return;
        }
        let current = (
            self.program.package_jsons.clone(),
            self.program.missing_package_jsons.clone(),
        );
        let (old_jsons, old_missing) = self
            .old
            .as_ref()
            .map(|old| {
                (
                    old.package_jsons.as_slice(),
                    old.missing_package_jsons.as_slice(),
                )
            })
            .unwrap_or((&[], &[]));
        if current.0 != old_jsons || current.1 != old_missing {
            self.build_info_emit_pending = true;
        }
        self.package_jsons = Some(current);
    }

    /// tsgo `emitBuildInfo` up to the write: the document when one is to be
    /// written, `None` when nothing changed since the old build info.
    pub fn to_build_info(&mut self, has_errors_outside_cache: bool) -> Option<BuildInfo> {
        self.ensure_has_errors(has_errors_outside_cache);
        self.ensure_package_jsons();
        if !self.build_info_emit_pending {
            return None;
        }
        Some(self.serialize())
    }

    /// tsgo `snapshotToBuildInfo` of an incremental program.
    fn serialize(&self) -> BuildInfo {
        let program = &self.program;
        let options = &program.options;
        let composite = options.composite == Some(true);
        let full_kind = FileEmitKind::of_options(options);
        let mut to = ToBuildInfo::new(program);
        let mut info = BuildInfo {
            version: VERSION.to_owned(),
            incremental: is_incremental(options),
            ..BuildInfo::default()
        };
        if !is_incremental(options) {
            // snapshotToBuildInfo of a non-incremental program (tsc -b):
            // the config's roots by their canonical names, the error flags
            // and the package.json files.
            info.root = program
                .root_file_names
                .iter()
                .map(|path| BuildInfoRoot::NonIncremental(to.relative_to_build_info(path)))
                .collect();
            self.serialize_errors_and_package_jsons(&mut info, &to);
            return info;
        }

        // setFileInfoAndEmitSignatures: the ids of the program files, in
        // order.
        for (index, file) in program.files.iter().enumerate() {
            let id = to.file_id(&file.path);
            debug_assert_eq!(id as usize, index + 1);
            let file_info = &self.file_infos[index];
            if composite && !file.is_json && file.may_be_emitted {
                match &self.emit_signatures[index] {
                    None => info
                        .emit_signatures
                        .push(EmitSignatureEntry::NoEmitSignature(id)),
                    Some(signature)
                        if signature.value() != file_info.signature || !signature.is_plain() =>
                    {
                        if signature.is_plain() {
                            info.emit_signatures.push(EmitSignatureEntry::Signature(
                                id,
                                signature.value().to_owned(),
                            ));
                        } else if signature.value() == file_info.signature {
                            info.emit_signatures
                                .push(EmitSignatureEntry::DiffersOnlyInDtsMap(id));
                        } else {
                            info.emit_signatures
                                .push(EmitSignatureEntry::DiffersInOptions(
                                    id,
                                    signature.value().to_owned(),
                                ));
                        }
                    }
                    Some(_) => {}
                }
            }
            info.file_infos.push(FileInfoEntry::new(
                &file_info.version,
                (!file_info.signature.is_empty()).then_some(file_info.signature.as_str()),
                file_info.affects_global_scope,
                file_info.implied_node_format,
            ));
        }

        // setRootOfIncrementalProgram: roots by their resolved file's id,
        // runs of consecutive ids merged; a root whose file is not the
        // root's own path (a redirect) is recorded as a resolved root.
        let mut roots: BTreeMap<FileId, &str> = BTreeMap::new();
        for (root_path, file) in &program.roots {
            roots.insert(to.file_id(&program.files[*file].path), root_path);
        }
        for (&resolved, root_path) in &roots {
            match info.root.last_mut() {
                Some(BuildInfoRoot::Range(_, end)) if *end + 1 == resolved => *end = resolved,
                Some(BuildInfoRoot::Single(start)) if *start + 1 == resolved => {
                    let start = *start;
                    *info.root.last_mut().expect("just matched") =
                        BuildInfoRoot::Range(start, resolved);
                }
                _ => info.root.push(BuildInfoRoot::Single(resolved)),
            }
            let root = to.file_id(root_path);
            if root != resolved {
                info.resolved_root.push((resolved, root));
            }
        }

        info.options = build_info_options(options, &|path| to.relative_to_build_info(path));

        // setReferencedMap: the files with references, by path.
        let mut referenced: Vec<(&str, &[String])> = program
            .referenced_files
            .iter()
            .enumerate()
            .filter(|(_, references)| !references.is_empty())
            .map(|(index, references)| (program.files[index].path.as_str(), references.as_slice()))
            .collect();
        referenced.sort_by(|left, right| left.0.cmp(right.0));
        for (path, references) in referenced {
            let file = to.file_id(path);
            let list = to.file_id_list_id(references);
            info.referenced_map.push((file, list));
        }

        // setChangeFileSet: by path.
        for path in &self.changed {
            let id = to.file_id(path);
            info.change_file_set.push(id);
        }

        // setSemanticDiagnostics: the files in Program order; a file without
        // cached rows (and not in the change set) is listed alone.
        for (index, file) in program.files.iter().enumerate() {
            let id = to.file_id(&file.path);
            match &self.semantic[index] {
                None => {
                    if !self.changed.contains(&file.path) {
                        info.semantic_diagnostics_per_file
                            .push(SemanticDiagnosticEntry::NotCached(id));
                    }
                }
                Some(rows) if !rows.is_empty() => info
                    .semantic_diagnostics_per_file
                    .push(SemanticDiagnosticEntry::Diagnostics(id, to.rows(rows))),
                Some(_) => {}
            }
        }

        // setEmitDiagnostics: by path.
        let mut with_emit_rows: Vec<usize> = (0..program.files.len())
            .filter(|index| self.emit_rows[*index].is_some())
            .collect();
        with_emit_rows
            .sort_by(|left, right| program.files[*left].path.cmp(&program.files[*right].path));
        for index in with_emit_rows {
            let Some(rows) = &self.emit_rows[index] else {
                continue;
            };
            let rows = to.rows(rows);
            if rows.is_empty() {
                continue;
            }
            let id = to.file_id(&program.files[index].path);
            info.emit_diagnostics_per_file.push((id, rows));
        }

        // setAffectedFilesPendingEmit: by path; files that may not be
        // emitted are dropped; the full kind is written as 0.
        for (path, kind) in &self.pending_emit {
            let Some(&index) = self.index_by_path.get(path) else {
                continue;
            };
            if !program.files[index].may_be_emitted {
                continue;
            }
            let id = to.file_id(path);
            info.affected_files_pending_emit
                .push((id, if *kind == full_kind { 0 } else { *kind }));
        }

        if let Some(latest) = &self.latest_changed_dts_file {
            info.latest_changed_dts_file = to.relative_to_build_info(latest);
        }
        self.serialize_errors_and_package_jsons(&mut info, &to);
        info.file_names = to.file_names;
        info.file_ids_list = to.file_ids_list;
        info
    }

    /// The error flags and the package.json files every build info carries.
    fn serialize_errors_and_package_jsons(&self, info: &mut BuildInfo, to: &ToBuildInfo<'_>) {
        info.errors = self.has_errors.unwrap_or(false);
        info.semantic_errors = self.has_semantic_errors;
        info.check_pending = self.check_pending;
        if let Some((package_jsons, missing)) = &self.package_jsons {
            info.package_jsons = package_jsons
                .iter()
                .map(|path| to.relative_to_build_info(path))
                .collect();
            info.missing_package_jsons = missing
                .iter()
                .map(|path| to.relative_to_build_info(path))
                .collect();
        }
    }
}

/// tsgo `affectedFilesHandler`: the state of one `collectAllAffectedFiles`.
struct AffectedFiles<'s> {
    snapshot: &'s Snapshot,
    /// tsgo `updatedSignatures`: computed once per file.
    updated_signatures: BTreeMap<usize, (String, SignatureUpdateKind)>,
    has_all_files_excluding_default_library: bool,
    remove_rows: BTreeSet<usize>,
    cleaned_library_rows: bool,
    /// tsgo `seenFileAndReferences`: the file was handled, and whether with
    /// JavaScript invalidation.
    seen_file_and_references: BTreeMap<usize, bool>,
    /// The pending kinds the handling adds (tsgo's `dtsMayChange` maps,
    /// merged as `updateSnapshot` merges them).
    pending: BTreeMap<usize, u32>,
    signature_of: &'s mut dyn FnMut(usize) -> Option<String>,
}

impl AffectedFiles<'_> {
    fn is_changed_signature(&self, file: usize) -> bool {
        // This method is called after updating signatures of that path, so
        // signature is present in updatedSignatures
        self.updated_signatures
            .get(&file)
            .map(|(signature, _)| signature.as_str())
            != Some(self.snapshot.file_infos[file].signature.as_str())
    }

    fn remove_diagnostics_of_library_files(&mut self) {
        if self.cleaned_library_rows {
            return;
        }
        self.cleaned_library_rows = true;
        for (index, file) in self.snapshot.program.files.iter().enumerate() {
            if file.is_default_library && !file.type_checking_skipped_ignoring_no_check {
                self.remove_rows.insert(index);
            }
        }
    }

    /// tsgo `updateShapeSignature`: whether the file's signature changed;
    /// computed once per file.
    fn update_shape_signature(&mut self, file: usize, use_file_version_as_signature: bool) -> bool {
        if self.updated_signatures.contains_key(&file) {
            return false;
        }
        let facts = &self.snapshot.program.files[file];
        let info = &self.snapshot.file_infos[file];
        let mut signature = String::new();
        // JSON files have no declaration output from which to compute a
        // shape signature, so use the file version to conservatively
        // invalidate dependents.
        if !facts.is_declaration_file && !facts.is_json && !use_file_version_as_signature {
            signature = (self.signature_of)(file).unwrap_or_default();
        }
        // Default is to use file version as signature
        let mut kind = SignatureUpdateKind::ComputedDts;
        if signature.is_empty() {
            signature = info.version.clone();
            kind = SignatureUpdateKind::UsedVersion;
        }
        let changed = signature != info.signature;
        self.updated_signatures.insert(file, (signature, kind));
        changed
    }

    /// tsgo `getFilesAffectedBy`.
    fn files_affected_by(&mut self, path: &str) -> Vec<usize> {
        let Some(&file) = self.snapshot.index_by_path.get(path) else {
            return Vec::new();
        };
        if !self.update_shape_signature(file, false) {
            return vec![file];
        }
        if self.snapshot.file_infos[file].affects_global_scope {
            self.has_all_files_excluding_default_library = true;
            return self.snapshot.all_files_excluding_default_library();
        }
        if self.snapshot.program.options.isolated_modules == Some(true) {
            return vec![file];
        }
        // Now we need to if each file in the referencedBy list has a shape
        // change as well. Because if so, its own referencedBy files need to
        // be saved as well to make the emitting result consistent with files
        // on disk.
        let seen = self.for_each_file_referenced_by(file, |handler, current| {
            // If the current file is not nil and has a shape change, we need
            // to queue it for processing
            (handler.update_shape_signature(current, false), false)
        });
        seen.into_iter().collect()
    }

    /// tsgo `forEachFileReferencedBy`: the files reached from `file`
    /// through the referencing files, `action` deciding whether to continue
    /// through a file (and whether to stop).
    fn for_each_file_referenced_by(
        &mut self,
        file: usize,
        mut action: impl FnMut(&mut Self, usize) -> (bool, bool),
    ) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        seen.insert(file);
        let mut queue = self.snapshot.referenced_by[file].clone();
        while let Some(current) = queue.pop() {
            if seen.insert(current) {
                let (queue_for_file, fast_return) = action(self, current);
                if fast_return {
                    return seen;
                }
                if queue_for_file {
                    queue.extend(self.snapshot.referenced_by[current].iter().copied());
                }
            }
        }
        seen
    }

    /// tsgo `handleDtsMayChangeOfAffectedFile`.
    fn handle_dts_may_change_of_affected_file(
        &mut self,
        affected: usize,
        exports_const_enum: &mut dyn FnMut(usize) -> bool,
    ) {
        self.remove_rows.insert(affected);
        // If affected files is everything except default library, then
        // nothing more to do
        if self.has_all_files_excluding_default_library {
            self.remove_diagnostics_of_library_files();
            // When a change affects the global scope, all files are
            // considered to be affected without updating their signature;
            // ensure the signature of any affected file is up to date.
            self.update_shape_signature(affected, false);
            return;
        }
        let options = &self.snapshot.program.options;
        if options.assume_changes_only_affect_direct_dependencies == Some(true) {
            return;
        }
        // If there was change in signature (dts output) for the changed
        // file, then only we need to handle pending file emit
        let path = &self.snapshot.program.files[affected].path;
        if !self.snapshot.changed.contains(path) || !self.is_changed_signature(affected) {
            return;
        }
        // Since isolated modules dont change js files, files affected by
        // change in signature is itself. But we need to cleanup semantic
        // diagnostics and queue dts emit for affected files
        if options.isolated_modules == Some(true) {
            self.for_each_file_referenced_by(affected, |handler, current| {
                if handler.handle_dts_may_change_of_global_scope(current, false) {
                    return (false, true);
                }
                handler.handle_dts_may_change_of(current, false);
                (handler.is_changed_signature(current), false)
            });
        }
        // If exported const enum, we need to ensure that js files are
        // emitted as well since the const enum value changed
        let invalidate_js_files = exports_const_enum(affected);
        // Go through files that reference affected file and handle dts emit
        // and semantic diagnostics for them and their references
        let referencing = self.snapshot.referenced_by[affected].clone();
        for file_referencing_changed_file in referencing {
            if self.handle_dts_may_change_of_global_scope(
                file_referencing_changed_file,
                invalidate_js_files,
            ) {
                return;
            }
            // Since references of changed file = affected files - we would
            // have already handled d.ts emit and semantic diagnostics for
            // those files. Now we need to handle files referencing those
            // affected files to ensure correctness.
            let referencing_affected =
                self.snapshot.referenced_by[file_referencing_changed_file].clone();
            for file_referencing_affected_file in referencing_affected {
                if self.handle_dts_may_change_of_file_and_references(
                    file_referencing_affected_file,
                    invalidate_js_files,
                ) {
                    return;
                }
            }
        }
    }

    /// tsgo `handleDtsMayChangeOfFileAndReferences`.
    fn handle_dts_may_change_of_file_and_references(
        &mut self,
        file: usize,
        invalidate_js_files: bool,
    ) -> bool {
        match self.seen_file_and_references.get(&file).copied() {
            Some(existing) if existing || !invalidate_js_files => return false,
            Some(_) => {
                self.seen_file_and_references.insert(file, true);
            }
            None => {
                self.seen_file_and_references
                    .insert(file, invalidate_js_files);
            }
        }
        if self.handle_dts_may_change_of_global_scope(file, invalidate_js_files) {
            return true;
        }
        self.handle_dts_may_change_of(file, invalidate_js_files);
        // Remove the diagnostics of files that import this file and any
        // files that are referenced by it (directly or indirectly)
        let referencing = self.snapshot.referenced_by[file].clone();
        for referencing_file in referencing {
            if self
                .handle_dts_may_change_of_file_and_references(referencing_file, invalidate_js_files)
            {
                return true;
            }
        }
        false
    }

    /// tsgo `handleDtsMayChangeOfGlobalScope`.
    fn handle_dts_may_change_of_global_scope(
        &mut self,
        file: usize,
        invalidate_js_files: bool,
    ) -> bool {
        if !self.snapshot.file_infos[file].affects_global_scope {
            return false;
        }
        // Every file needs to be handled
        for file in self.snapshot.all_files_excluding_default_library() {
            self.handle_dts_may_change_of(file, invalidate_js_files);
        }
        self.remove_diagnostics_of_library_files();
        true
    }

    /// tsgo `handleDtsMayChangeOf`: a file whose declaration output may
    /// change loses its rows, takes its version as signature and is queued
    /// for the declaration emit (or the full emit).
    fn handle_dts_may_change_of(&mut self, file: usize, invalidate_js_files: bool) {
        let path = &self.snapshot.program.files[file].path;
        if self.snapshot.changed.contains(path) {
            return;
        }
        self.remove_rows.insert(file);
        self.update_shape_signature(file, true);
        let options = &self.snapshot.program.options;
        let kind = if invalidate_js_files {
            FileEmitKind::of_options(options)
        } else if emit_declarations(options) {
            if options.declaration_map == Some(true) {
                FileEmitKind::ALL_DTS
            } else {
                FileEmitKind::DTS
            }
        } else {
            return;
        };
        *self.pending.entry(file).or_insert(0) |= kind;
    }
}

/// tsgo `toBuildInfo`: the file-name table and the file-id-list table the
/// document's entries point into.
struct ToBuildInfo<'p> {
    program: &'p ProgramState,
    build_info_directory: String,
    file_names: Vec<String>,
    file_ids: BTreeMap<String, FileId>,
    file_ids_list: Vec<Vec<FileId>>,
    file_id_list_ids: BTreeMap<String, u32>,
    library_names: BTreeMap<&'p str, &'p str>,
}

impl<'p> ToBuildInfo<'p> {
    fn new(program: &'p ProgramState) -> Self {
        Self {
            program,
            build_info_directory: directory_of(&program.build_info_file_name),
            file_names: Vec::with_capacity(program.files.len()),
            file_ids: BTreeMap::new(),
            file_ids_list: Vec::new(),
            file_id_list_ids: BTreeMap::new(),
            library_names: program
                .files
                .iter()
                .filter_map(|file| {
                    file.default_library_name
                        .as_deref()
                        .map(|name| (file.path.as_str(), name))
                })
                .collect(),
        }
    }

    /// tsgo `relativeToBuildInfo`.
    fn relative_to_build_info(&self, path: &str) -> String {
        let relative = tsc_program::relative_path_from_directory(
            self.build_info_directory.as_str().into(),
            path.into(),
            self.program.use_case_sensitive_file_names,
        );
        ensure_path_is_non_module_name(&relative.to_string_lossy())
    }

    /// tsgo `toFileId`: a known file's id, or the next id with the file's
    /// name appended (a default library by its bare name).
    fn file_id(&mut self, path: &str) -> FileId {
        if let Some(id) = self.file_ids.get(path) {
            return *id;
        }
        let name = match self.library_names.get(path) {
            Some(name) => (*name).to_owned(),
            None => self.relative_to_build_info(path),
        };
        self.file_names.push(name);
        let id = FileId::try_from(self.file_names.len()).expect("file id fits u32");
        self.file_ids.insert(path.to_owned(), id);
        id
    }

    /// tsgo `toFileIdListId`: the id of the sorted id list, shared between
    /// equal lists.
    fn file_id_list_id(&mut self, paths: &[String]) -> u32 {
        let mut ids: Vec<FileId> = paths.iter().map(|path| self.file_id(path)).collect();
        ids.sort_unstable();
        ids.dedup();
        let key = ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        if let Some(id) = self.file_id_list_ids.get(&key) {
            return *id;
        }
        self.file_ids_list.push(ids);
        let id = u32::try_from(self.file_ids_list.len()).expect("file id list id fits u32");
        self.file_id_list_ids.insert(key, id);
        id
    }

    /// The document form of cached rows: an old row's other file by its
    /// id in this document (tsgo `toBuildInfoDiagnosticsFromFileNameDiagnostics`).
    fn rows(&mut self, rows: &CachedRows) -> Vec<BuildInfoDiagnostic> {
        match rows {
            CachedRows::New(rows) => rows.clone(),
            CachedRows::Old(rows) => rows.iter().map(|row| self.old_row(row)).collect(),
        }
    }

    fn old_row(&mut self, row: &CachedDiagnostic) -> BuildInfoDiagnostic {
        BuildInfoDiagnostic {
            file: row.file.as_deref().map(|path| self.file_id(path)),
            no_file: row.no_file,
            pos: row.pos,
            end: row.end,
            code: row.code,
            category: row.category,
            source: row.source.clone(),
            message_text: row.message_text.clone(),
            message_key: row.message_key.clone(),
            message_args: row.message_args.clone(),
            message_chain: row
                .message_chain
                .iter()
                .map(|chain| self.old_row(chain))
                .collect(),
            related_information: row
                .related_information
                .iter()
                .map(|related| self.old_row(related))
                .collect(),
            reports_unnecessary: row.reports_unnecessary,
            reports_deprecated: row.reports_deprecated,
            skipped_on_no_emit: row.skipped_on_no_emit,
            repopulate_info: row.repopulate_info.clone(),
        }
    }
}

fn convert_rows(
    rows: &mut [CachedDiagnostic],
    convert: impl Fn(&CachedDiagnostic) -> Option<CachedDiagnostic>,
) {
    for row in rows {
        if let Some(converted) = convert(row) {
            *row = converted;
        }
    }
}

/// tsgo `tspath.GetDirectoryPath` of a normalized absolute path.
fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

/// tsgo `tspath.PathIsRelative`.
pub(crate) fn path_is_relative(path: &str) -> bool {
    path == "." || path == ".." || path.starts_with("./") || path.starts_with("../")
}

/// tsgo `tspath.PathIsAbsolute`.
pub(crate) fn path_is_absolute(path: &str) -> bool {
    path.starts_with('/')
        || path.starts_with("\\\\")
        || path
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':' && path.as_bytes()[0].is_ascii_alphabetic())
        || path.contains("://")
}

/// tsgo `tspath.EnsurePathIsNonModuleName`: a path that is neither
/// absolute nor relative gets a `./` prefix.
pub fn ensure_path_is_non_module_name(path: &str) -> String {
    if path_is_relative(path) || path_is_absolute(path) {
        path.to_owned()
    } else {
        format!("./{path}")
    }
}

// ---------------------------------------------------------------------------
// A compilation without an old state, driven by its results at once (the
// first build's shape, as the compiler driver reported it before the state
// machine existed).

/// The emit's declaration output for one file: the signature (the hash of
/// the declaration text up to its source map comment) and the file name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationOutput {
    pub signature: String,
    pub output_file_name: String,
}

/// What the emit did (tsgo `emitFilesIncremental` over a fresh snapshot).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeclarationEmitFacts {
    /// `HandleNoEmitOptions` skipped the whole emit (`noEmitOnError` with
    /// diagnostics): nothing was emitted and every file stays pending.
    pub skipped: bool,
    /// Per file: the declaration file the emit wrote.
    pub declaration_outputs: Vec<Option<DeclarationOutput>>,
    /// Per file: the diagnostics the file's emit produced.
    pub emit_diagnostics: Vec<Vec<BuildInfoDiagnostic>>,
}

/// How the command reached the build info.
#[derive(Clone, Copy, Debug)]
pub enum FileState<'a> {
    /// `--noEmit`; `declaration_diagnostics` (per file) is present when the
    /// command asked the program for them (tsgo `GetDiagnosticsOfAnyProgram`
    /// does when nothing else was reported and the options emit
    /// declarations).
    NoEmit {
        declaration_diagnostics: Option<&'a [Vec<BuildInfoDiagnostic>]>,
    },
    Emit(&'a DeclarationEmitFacts),
}

/// The semantic rows the checker cached per file, in Program order; `None`
/// when the file's check was not requested.
pub type SemanticRowsFacts<'a> = &'a [Option<Vec<BuildInfoDiagnostic>>];

/// Everything one fresh compilation produces for the build info.
#[derive(Clone, Copy, Debug)]
pub struct FreshSnapshotInput<'a> {
    pub program: &'a ProgramState,
    pub semantic_rows: SemanticRowsFacts<'a>,
    pub state: FileState<'a>,
    /// tsgo `ensureHasErrorsForState`'s non-cached errors: config parsing,
    /// syntactic, program (options), global or file-located include
    /// processing diagnostics exist.
    pub has_errors_outside_cache: bool,
}

/// The emit updates of a fresh compilation's results (tsgo
/// `emitFilesIncremental` over every pending file).
pub fn fresh_emit_updates(
    snapshot: &Snapshot,
    state: FileState<'_>,
) -> (Vec<EmitUpdate>, Vec<usize>) {
    let mut updates = Vec::new();
    let mut deleted = Vec::new();
    let files = &snapshot.program.files;
    match state {
        FileState::NoEmit {
            declaration_diagnostics: Some(declaration_diagnostics),
        } => {
            // GetDeclarationDiagnostics → emitFilesIncremental(isForDtsErrors):
            // every pending file that may be emitted has its declaration
            // errors handled, which clears that bit of its pending kind.
            for (index, file) in files.iter().enumerate() {
                let pending = snapshot.pending_emit_kind(index);
                if pending == FileEmitKind::NONE {
                    continue;
                }
                if !file.may_be_emitted {
                    deleted.push(index);
                    continue;
                }
                let handled = pending & FileEmitKind::DTS_ERRORS;
                if handled == 0 {
                    continue;
                }
                updates.push(EmitUpdate {
                    file: index,
                    emitted_kind: handled,
                    from_cache: false,
                    diagnostics: declaration_diagnostics
                        .get(index)
                        .cloned()
                        .unwrap_or_default(),
                    declaration: None,
                });
            }
        }
        FileState::NoEmit {
            declaration_diagnostics: None,
        } => {}
        FileState::Emit(emit) if !emit.skipped => {
            for (index, file) in files.iter().enumerate() {
                let pending = snapshot.pending_emit_kind(index);
                if pending == FileEmitKind::NONE {
                    continue;
                }
                if !file.may_be_emitted {
                    deleted.push(index);
                    continue;
                }
                updates.push(EmitUpdate {
                    file: index,
                    emitted_kind: pending,
                    from_cache: false,
                    diagnostics: emit
                        .emit_diagnostics
                        .get(index)
                        .cloned()
                        .unwrap_or_default(),
                    declaration: emit
                        .declaration_outputs
                        .get(index)
                        .and_then(Option::as_ref)
                        .map(|output| DeclarationEmit {
                            signature: output.signature.clone(),
                            output_file_name: output.output_file_name.clone(),
                        }),
                });
            }
        }
        FileState::Emit(_) => {}
    }
    (updates, deleted)
}

/// tsgo `programToSnapshot` without an old program, the emit or
/// declaration-diagnostics handling of the command, and
/// `snapshotToBuildInfo`.
pub fn build_fresh_build_info(input: &FreshSnapshotInput<'_>) -> BuildInfo {
    let mut snapshot = Snapshot::new(input.program.clone(), None);
    for (index, rows) in input.semantic_rows.iter().enumerate() {
        if let Some(rows) = rows {
            snapshot.store_fresh_rows(index, rows.clone());
        }
    }
    snapshot.finish_check();
    let (updates, deleted) = fresh_emit_updates(&snapshot, input.state);
    snapshot.record_emit(updates, &deleted);
    snapshot
        .to_build_info(input.has_errors_outside_cache)
        .expect("a fresh incremental program always writes its build info")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_emit_kinds_follow_tsgo() {
        assert_eq!(FileEmitKind::pending(25, 8), 17);
        assert_eq!(FileEmitKind::pending(57, 8), 49);
        assert_eq!(FileEmitKind::pending(1, 1), 0);
        assert_eq!(FileEmitKind::pending(25, 0), 25);
        assert_eq!(FileEmitKind::pending(25, 25), 0);
        // The option-change scenario: JS only → JS with maps, and back.
        assert_eq!(FileEmitKind::pending(3, 1), 3);
        assert_eq!(FileEmitKind::pending(1, 3), 1);
        // JS only → with declarations; → with declaration maps; → JS only.
        assert_eq!(FileEmitKind::pending(25, 1), 24);
        assert_eq!(FileEmitKind::pending(57, 25), 48);
        assert_eq!(FileEmitKind::pending(1, 57), 0);
    }

    #[test]
    fn non_module_names_get_a_dot_prefix() {
        assert_eq!(ensure_path_is_non_module_name(""), "./");
        assert_eq!(ensure_path_is_non_module_name("src/a.ts"), "./src/a.ts");
        assert_eq!(ensure_path_is_non_module_name("../a.ts"), "../a.ts");
        assert_eq!(ensure_path_is_non_module_name("/a.ts"), "/a.ts");
        assert_eq!(ensure_path_is_non_module_name("c:/a.ts"), "c:/a.ts");
    }

    #[test]
    fn composite_declaration_writes_follow_tsgo() {
        assert_eq!(
            declaration_write_decision(false, Some(("s", true)), "s"),
            DeclarationWrite {
                skip: false,
                changed: false
            }
        );
        assert_eq!(
            declaration_write_decision(true, Some(("s", true)), "s"),
            DeclarationWrite {
                skip: true,
                changed: false
            }
        );
        // The old form was emitted with another declarationMap setting: the
        // file is written again but is not a change.
        assert_eq!(
            declaration_write_decision(true, Some(("s", false)), "s"),
            DeclarationWrite {
                skip: false,
                changed: false
            }
        );
        assert_eq!(
            declaration_write_decision(true, Some(("s", true)), "t"),
            DeclarationWrite {
                skip: false,
                changed: true
            }
        );
        assert_eq!(
            declaration_write_decision(true, None, "t"),
            DeclarationWrite {
                skip: false,
                changed: true
            }
        );
    }

    fn options() -> CompilerOptions {
        CompilerOptions {
            module: Some(99),
            target: Some(9),
            strict: Some(true),
            composite: Some(true),
            out_dir: Some("/work/dist".into()),
            ..CompilerOptions::default()
        }
    }

    fn file(path: &str, version: &str, may_be_emitted: bool, global: bool) -> ProgramFileFacts {
        ProgramFileFacts {
            path: path.to_owned(),
            default_library_name: None,
            version: version.to_owned(),
            affects_global_scope: global,
            implied_node_format: 1,
            may_be_emitted,
            is_json: false,
            is_declaration_file: path.ends_with(".d.ts"),
            is_default_library: false,
            type_checking_skipped_ignoring_no_check: false,
        }
    }

    fn program() -> ProgramState {
        ProgramState {
            files: vec![
                file("/work/src/b.ts", "bbb", true, false),
                file("/work/src/a.ts", "aaa", true, false),
                file("/work/lib/min.d.ts", "lll", false, true),
            ],
            roots: vec![
                ("/work/src/a.ts".into(), 1),
                ("/work/src/b.ts".into(), 0),
                ("/work/lib/min.d.ts".into(), 2),
            ],
            referenced_files: vec![vec![], vec!["/work/src/b.ts".into()], vec![]],
            options: options(),
            build_info_file_name: "/work/dist/tsconfig.tsbuildinfo".into(),
            build: false,
            root_file_names: Vec::new(),
            current_directory: "/work".into(),
            use_case_sensitive_file_names: true,
            package_jsons: Vec::new(),
            missing_package_jsons: Vec::new(),
        }
    }

    #[test]
    fn a_no_emit_command_with_declaration_diagnostics_handled_writes_the_incr_fixture() {
        // The `rerun` fixture under --noEmit after a clean check: every
        // emittable file keeps its JS and declaration emit pending (17)
        // and records no emit signature.
        let program = program();
        let rows = vec![Some(Vec::new()), Some(Vec::new()), Some(Vec::new())];
        let declaration_diagnostics = vec![Vec::new(), Vec::new(), Vec::new()];
        let info = build_fresh_build_info(&FreshSnapshotInput {
            program: &program,
            semantic_rows: &rows,
            state: FileState::NoEmit {
                declaration_diagnostics: Some(&declaration_diagnostics),
            },
            has_errors_outside_cache: false,
        });
        assert_eq!(
            info.to_json(),
            concat!(
                r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/b.ts","../src/a.ts","../lib/min.d.ts"],"#,
                r#""fileInfos":["bbb","aaa",{"version":"lll","affectsGlobalScope":true,"impliedNodeFormat":1}],"#,
                r#""fileIdsList":[[1]],"options":{"composite":true,"module":99,"outDir":"./","strict":true,"target":9},"#,
                r#""referencedMap":[[2,1]],"affectedFilesPendingEmit":[[2,17],[1,17]],"emitSignatures":[1,2]}"#
            )
        );
    }

    #[test]
    fn an_emit_records_signatures_and_the_latest_declaration_file() {
        let program = program();
        let rows = vec![Some(Vec::new()), Some(Vec::new()), Some(Vec::new())];
        let emit = DeclarationEmitFacts {
            skipped: false,
            declaration_outputs: vec![
                Some(DeclarationOutput {
                    signature: "sb".into(),
                    output_file_name: "/work/dist/src/b.d.ts".into(),
                }),
                Some(DeclarationOutput {
                    signature: "sa".into(),
                    output_file_name: "/work/dist/src/a.d.ts".into(),
                }),
                None,
            ],
            emit_diagnostics: vec![Vec::new(), Vec::new(), Vec::new()],
        };
        let info = build_fresh_build_info(&FreshSnapshotInput {
            program: &program,
            semantic_rows: &rows,
            state: FileState::Emit(&emit),
            has_errors_outside_cache: false,
        });
        assert_eq!(
            info.to_json(),
            concat!(
                r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/b.ts","../src/a.ts","../lib/min.d.ts"],"#,
                r#""fileInfos":[{"version":"bbb","signature":"sb","impliedNodeFormat":1},{"version":"aaa","signature":"sa","impliedNodeFormat":1},"#,
                r#"{"version":"lll","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"#,
                r#""options":{"composite":true,"module":99,"outDir":"./","strict":true,"target":9},"referencedMap":[[2,1]],"#,
                r#""latestChangedDtsFile":"./src/a.d.ts"}"#
            )
        );
    }

    #[test]
    fn unchecked_files_are_listed_alone_and_errors_recorded() {
        let program = program();
        let rows = vec![None, None, None];
        let info = build_fresh_build_info(&FreshSnapshotInput {
            program: &program,
            semantic_rows: &rows,
            state: FileState::NoEmit {
                declaration_diagnostics: None,
            },
            has_errors_outside_cache: true,
        });
        assert!(info.errors);
        assert_eq!(
            info.semantic_diagnostics_per_file,
            vec![
                SemanticDiagnosticEntry::NotCached(1),
                SemanticDiagnosticEntry::NotCached(2),
                SemanticDiagnosticEntry::NotCached(3)
            ]
        );
        assert_eq!(info.affected_files_pending_emit, vec![(2, 0), (1, 0)]);
    }

    // The noemit-chain scenario (tsgo's recorded steps in
    // scratchpad/p36c/steps-out): a → b, a has a type error.

    fn noemit_options() -> CompilerOptions {
        CompilerOptions {
            module: Some(99),
            target: Some(9),
            strict: Some(true),
            incremental: Some(true),
            no_emit: Some(true),
            ..CompilerOptions::default()
        }
    }

    fn noemit_program(a_version: &str, b_version: &str) -> ProgramState {
        let mut lib = file(
            "/work/lib/min.d.ts",
            "bae41f699edb9c9e3f69cdbea446f1f5",
            false,
            true,
        );
        lib.is_default_library = false;
        ProgramState {
            files: vec![
                file("/work/src/b.ts", b_version, true, false),
                file("/work/src/a.ts", a_version, true, false),
                lib,
            ],
            roots: vec![
                ("/work/src/b.ts".into(), 0),
                ("/work/src/a.ts".into(), 1),
                ("/work/lib/min.d.ts".into(), 2),
            ],
            referenced_files: vec![vec![], vec!["/work/src/b.ts".into()], vec![]],
            options: noemit_options(),
            build_info_file_name: "/work/tsconfig.tsbuildinfo".into(),
            build: false,
            root_file_names: Vec::new(),
            current_directory: "/work".into(),
            use_case_sensitive_file_names: true,
            package_jsons: Vec::new(),
            missing_package_jsons: Vec::new(),
        }
    }

    fn error_row() -> BuildInfoDiagnostic {
        BuildInfoDiagnostic {
            pos: 38,
            end: 39,
            code: 2322,
            category: 1,
            message_key: "Type_0_is_not_assignable_to_type_1_2322".into(),
            message_args: vec!["number".into(), "string".into()],
            ..BuildInfoDiagnostic::default()
        }
    }

    const STEP0: &str = r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":["1ee67a763a8635e8d85574ccc6e5740e","c4513d6b24fb8c179ec11446075e21ed",{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["number","string"]}]]],"affectedFilesPendingEmit":[2,1]}"#;

    fn old_state(text: &str) -> OldState {
        let info = BuildInfo::from_json(text).expect("parses");
        OldState::from_build_info(&info, &crate::old_state::tests::paths("/work", "/libs"))
    }

    #[test]
    fn the_first_no_emit_build_writes_the_cached_error() {
        let program = noemit_program(
            "c4513d6b24fb8c179ec11446075e21ed",
            "1ee67a763a8635e8d85574ccc6e5740e",
        );
        let mut snapshot = Snapshot::new(program, None);
        assert_eq!(snapshot.files_without_rows(), vec![true, true, true]);
        snapshot.store_fresh_rows(0, Vec::new());
        snapshot.store_fresh_rows(1, vec![error_row()]);
        snapshot.store_fresh_rows(2, Vec::new());
        snapshot.finish_check();
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(info.to_json(), STEP0);
    }

    #[test]
    fn an_unchanged_program_checks_nothing_and_writes_nothing() {
        let old = old_state(STEP0);
        let program = noemit_program(
            "c4513d6b24fb8c179ec11446075e21ed",
            "1ee67a763a8635e8d85574ccc6e5740e",
        );
        let mut snapshot = Snapshot::new(program, Some(&old));
        assert!(!snapshot.has_changed_files());
        assert_eq!(snapshot.files_without_rows(), vec![false, false, false]);
        assert_eq!(snapshot.cached_rows(1).map(<[_]>::len), Some(1));
        snapshot.finish_check();
        assert!(snapshot.to_build_info(false).is_none());
    }

    #[test]
    fn a_fixed_file_is_rechecked_and_its_signature_computed() {
        // Step 2: a fixed; its declaration signature is computed and only a
        // is checked (clean), so no rows remain.
        let old = old_state(STEP0);
        let program = noemit_program(
            "68269c3d41c4ac083f31e281de1aadf5",
            "1ee67a763a8635e8d85574ccc6e5740e",
        );
        let mut snapshot = Snapshot::new(program, Some(&old));
        assert!(snapshot.has_changed_files());
        let mut asked = Vec::new();
        snapshot.collect_all_affected_files(
            &mut |file| {
                asked.push(file);
                Some("03ee330dc35a9c186b6cc67781eafb11".to_owned())
            },
            &mut |_| false,
        );
        assert_eq!(asked, vec![1]);
        assert_eq!(snapshot.files_without_rows(), vec![false, true, false]);
        snapshot.store_fresh_rows(1, Vec::new());
        snapshot.finish_check();
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":["1ee67a763a8635e8d85574ccc6e5740e",{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"affectedFilesPendingEmit":[2,1]}"#
        );
    }

    #[test]
    fn a_changed_dependency_shape_affects_the_importer() {
        // Step 3: b's type changes (its declaration signature with it), so a
        // is rechecked and reports the new error.
        let old = old_state(
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":["1ee67a763a8635e8d85574ccc6e5740e",{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"affectedFilesPendingEmit":[2,1]}"#,
        );
        let program = noemit_program(
            "68269c3d41c4ac083f31e281de1aadf5",
            "9f46d33e18af1e10a7882e45535f7c95",
        );
        let mut snapshot = Snapshot::new(program, Some(&old));
        let mut asked = Vec::new();
        snapshot.collect_all_affected_files(
            &mut |file| {
                asked.push(file);
                Some(match file {
                    0 => "b9dd207cca22183dade38bf73fc60850".to_owned(),
                    _ => "03ee330dc35a9c186b6cc67781eafb11".to_owned(),
                })
            },
            &mut |_| false,
        );
        assert_eq!(asked, vec![0, 1]);
        assert_eq!(snapshot.files_without_rows(), vec![true, true, false]);
        snapshot.store_fresh_rows(0, Vec::new());
        let mut row = error_row();
        row.message_args = vec!["string".into(), "number".into()];
        snapshot.store_fresh_rows(1, vec![row]);
        snapshot.finish_check();
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":[{"version":"9f46d33e18af1e10a7882e45535f7c95","signature":"b9dd207cca22183dade38bf73fc60850","impliedNodeFormat":1},{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["string","number"]}]]],"affectedFilesPendingEmit":[2,1]}"#
        );
    }

    #[test]
    fn a_gated_check_keeps_the_change_set_and_the_cached_rows() {
        // Step 7: b gets a syntax error; the check is gated, so the change
        // set persists, a's cached error is kept and the error flag is set.
        let step6 = r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":[{"version":"9f46d33e18af1e10a7882e45535f7c95","signature":"b9dd207cca22183dade38bf73fc60850","impliedNodeFormat":1},{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["string","number"]}]]],"affectedFilesPendingEmit":[2,1]}"#;
        let old = old_state(step6);
        let program = noemit_program(
            "68269c3d41c4ac083f31e281de1aadf5",
            "ea9eacd8a716e4489f851e65d64a8a5e",
        );
        let mut snapshot = Snapshot::new(program, Some(&old));
        assert!(snapshot.has_changed_files());
        // No affected-files handling, no check.
        snapshot.finish_check();
        let info = snapshot.to_build_info(true).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","errors":true,"root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":[{"version":"ea9eacd8a716e4489f851e65d64a8a5e","signature":"b9dd207cca22183dade38bf73fc60850","impliedNodeFormat":1},{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["string","number"]}]]],"changeFileSet":[1],"affectedFilesPendingEmit":[2,1]}"#
        );
    }

    #[test]
    fn a_new_global_file_affects_every_file() {
        // Step 4: g.d.ts added (global): every file is rechecked, the
        // signatures of a and b recomputed, and g has none of its own.
        let old = old_state(
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":[{"version":"9f46d33e18af1e10a7882e45535f7c95","signature":"b9dd207cca22183dade38bf73fc60850","impliedNodeFormat":1},{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["string","number"]}]]],"affectedFilesPendingEmit":[2,1]}"#,
        );
        let mut program = noemit_program(
            "68269c3d41c4ac083f31e281de1aadf5",
            "9f46d33e18af1e10a7882e45535f7c95",
        );
        let g = file(
            "/work/src/g.d.ts",
            "bc421679236b667e480887667bd8f44b",
            false,
            true,
        );
        program.files.insert(2, g);
        program.roots = vec![
            ("/work/src/b.ts".into(), 0),
            ("/work/src/a.ts".into(), 1),
            ("/work/src/g.d.ts".into(), 2),
            ("/work/lib/min.d.ts".into(), 3),
        ];
        program.referenced_files.insert(2, Vec::new());
        let mut snapshot = Snapshot::new(program, Some(&old));
        let mut asked = Vec::new();
        snapshot.collect_all_affected_files(
            &mut |file| {
                asked.push(file);
                Some(match file {
                    0 => "b9dd207cca22183dade38bf73fc60850".to_owned(),
                    _ => "03ee330dc35a9c186b6cc67781eafb11".to_owned(),
                })
            },
            &mut |_| false,
        );
        // g's signature is its version (a declaration file); a and b are
        // recomputed because every file is affected.
        assert_eq!(asked, vec![0, 1]);
        assert_eq!(snapshot.files_without_rows(), vec![true, true, true, true]);
        snapshot.store_fresh_rows(0, Vec::new());
        let mut row = error_row();
        row.message_args = vec!["string".into(), "number".into()];
        snapshot.store_fresh_rows(1, vec![row]);
        snapshot.store_fresh_rows(2, Vec::new());
        snapshot.store_fresh_rows(3, Vec::new());
        snapshot.finish_check();
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","root":[[1,4]],"fileNames":["./src/b.ts","./src/a.ts","./src/g.d.ts","./lib/min.d.ts"],"fileInfos":[{"version":"9f46d33e18af1e10a7882e45535f7c95","signature":"b9dd207cca22183dade38bf73fc60850","impliedNodeFormat":1},{"version":"68269c3d41c4ac083f31e281de1aadf5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bc421679236b667e480887667bd8f44b","affectsGlobalScope":true,"impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["string","number"]}]]],"affectedFilesPendingEmit":[2,1]}"#
        );
    }

    // The composite-emit scenario: a → b, composite with outDir.

    fn composite_program(a_version: &str, b_version: &str) -> ProgramState {
        ProgramState {
            files: vec![
                file("/work/src/b.ts", b_version, true, false),
                file("/work/src/a.ts", a_version, true, false),
                file(
                    "/work/lib/min.d.ts",
                    "bae41f699edb9c9e3f69cdbea446f1f5",
                    false,
                    true,
                ),
            ],
            roots: vec![
                ("/work/src/b.ts".into(), 0),
                ("/work/src/a.ts".into(), 1),
                ("/work/lib/min.d.ts".into(), 2),
            ],
            referenced_files: vec![vec![], vec!["/work/src/b.ts".into()], vec![]],
            options: CompilerOptions {
                module: Some(99),
                target: Some(9),
                strict: Some(true),
                composite: Some(true),
                out_dir: Some("/work/dist".into()),
                root_dir: Some("/work".into()),
                ..CompilerOptions::default()
            },
            build_info_file_name: "/work/dist/tsconfig.tsbuildinfo".into(),
            build: false,
            root_file_names: Vec::new(),
            current_directory: "/work".into(),
            use_case_sensitive_file_names: true,
            package_jsons: Vec::new(),
            missing_package_jsons: Vec::new(),
        }
    }

    const COMPOSITE_STEP0: &str = r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/b.ts","../src/a.ts","../lib/min.d.ts"],"fileInfos":[{"version":"e30b346f657401ecc816c389f76aa76b","signature":"49278f68676c75a5466dfa39c6559d18","impliedNodeFormat":1},{"version":"b6df5f2b27e276d9e3e67069347c11a5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"composite":true,"module":99,"outDir":"./","rootDir":"..","strict":true,"target":9},"referencedMap":[[2,1]],"latestChangedDtsFile":"./src/a.d.ts"}"#;

    fn composite_old(text: &str) -> OldState {
        let info = BuildInfo::from_json(text).expect("parses");
        OldState::from_build_info(
            &info,
            &crate::old_state::tests::paths("/work/dist", "/libs"),
        )
    }

    #[test]
    fn a_body_change_in_a_composite_project_re_emits_the_javascript_only() {
        // Step 2: b's body changes, its declaration does not: only b is
        // affected, its d.ts write is skipped and the latest changed
        // declaration file stays a's.
        let old = composite_old(COMPOSITE_STEP0);
        let program = composite_program(
            "b6df5f2b27e276d9e3e67069347c11a5",
            "c13e2f81e6f9d398368a8f1c415aa26e",
        );
        let mut snapshot = Snapshot::new(program, Some(&old));
        snapshot.collect_all_affected_files(
            &mut |_| Some("49278f68676c75a5466dfa39c6559d18".to_owned()),
            &mut |_| false,
        );
        assert_eq!(snapshot.files_without_rows(), vec![true, false, false]);
        assert_eq!(snapshot.pending_emit_kind(0), 25);
        assert_eq!(snapshot.pending_emit_kind(1), 0);
        snapshot.store_fresh_rows(0, Vec::new());
        snapshot.finish_check();
        assert_eq!(
            declaration_write_decision(
                true,
                snapshot.old_emit_signature(0),
                "49278f68676c75a5466dfa39c6559d18"
            ),
            DeclarationWrite {
                skip: true,
                changed: false
            }
        );
        snapshot.record_emit(
            vec![EmitUpdate {
                file: 0,
                emitted_kind: 25,
                from_cache: false,
                diagnostics: Vec::new(),
                declaration: Some(DeclarationEmit {
                    signature: "49278f68676c75a5466dfa39c6559d18".into(),
                    output_file_name: "/work/dist/src/b.d.ts".into(),
                }),
            }],
            &[],
        );
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/b.ts","../src/a.ts","../lib/min.d.ts"],"fileInfos":[{"version":"c13e2f81e6f9d398368a8f1c415aa26e","signature":"49278f68676c75a5466dfa39c6559d18","impliedNodeFormat":1},{"version":"b6df5f2b27e276d9e3e67069347c11a5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"composite":true,"module":99,"outDir":"./","rootDir":"..","strict":true,"target":9},"referencedMap":[[2,1]],"latestChangedDtsFile":"./src/a.d.ts"}"#
        );
    }

    #[test]
    fn a_shape_change_in_a_composite_project_queues_the_importer() {
        // Step 3: b's declaration changes: a is affected (full emit), its
        // d.ts is unchanged (skipped), and b.d.ts is the latest changed.
        let old = composite_old(COMPOSITE_STEP0);
        let program = composite_program(
            "b6df5f2b27e276d9e3e67069347c11a5",
            "5e6668347c7a11276510f95c8d489e3b",
        );
        let mut snapshot = Snapshot::new(program, Some(&old));
        snapshot.collect_all_affected_files(
            &mut |file| {
                Some(match file {
                    0 => "83a814ac21695c9615a460ea7365b64f".to_owned(),
                    _ => "03ee330dc35a9c186b6cc67781eafb11".to_owned(),
                })
            },
            &mut |_| false,
        );
        assert_eq!(snapshot.files_without_rows(), vec![true, true, false]);
        assert_eq!(snapshot.pending_emit_kind(0), 25);
        assert_eq!(snapshot.pending_emit_kind(1), 25);
        snapshot.store_fresh_rows(0, Vec::new());
        snapshot.store_fresh_rows(1, Vec::new());
        snapshot.finish_check();
        snapshot.record_emit(
            vec![
                EmitUpdate {
                    file: 0,
                    emitted_kind: 25,
                    from_cache: false,
                    diagnostics: Vec::new(),
                    declaration: Some(DeclarationEmit {
                        signature: "83a814ac21695c9615a460ea7365b64f".into(),
                        output_file_name: "/work/dist/src/b.d.ts".into(),
                    }),
                },
                EmitUpdate {
                    file: 1,
                    emitted_kind: 25,
                    from_cache: false,
                    diagnostics: Vec::new(),
                    declaration: Some(DeclarationEmit {
                        signature: "03ee330dc35a9c186b6cc67781eafb11".into(),
                        output_file_name: "/work/dist/src/a.d.ts".into(),
                    }),
                },
            ],
            &[],
        );
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/b.ts","../src/a.ts","../lib/min.d.ts"],"fileInfos":[{"version":"5e6668347c7a11276510f95c8d489e3b","signature":"83a814ac21695c9615a460ea7365b64f","impliedNodeFormat":1},{"version":"b6df5f2b27e276d9e3e67069347c11a5","signature":"03ee330dc35a9c186b6cc67781eafb11","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"composite":true,"module":99,"outDir":"./","rootDir":"..","strict":true,"target":9},"referencedMap":[[2,1]],"latestChangedDtsFile":"./src/b.d.ts"}"#
        );
    }

    #[test]
    fn an_option_change_queues_the_emit_of_the_changed_kind() {
        // The option-change scenario, step 3: `--declaration` added to a
        // JS-only program: every file pends its declaration emit (24).
        let old = {
            let info = BuildInfo::from_json(
                r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/a.ts","../src/b.ts","../lib/min.d.ts"],"fileInfos":["dcc03016c1e9fa8a9c06f4a9279fde2a","fd1625d5addd2389e2c382f96f76510c",{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"options":{"module":99,"outDir":"./","rootDir":"..","strict":true,"target":9}}"#,
            )
            .expect("parses");
            OldState::from_build_info(
                &info,
                &crate::old_state::tests::paths("/work/dist", "/libs"),
            )
        };
        let mut program = composite_program(
            "dcc03016c1e9fa8a9c06f4a9279fde2a",
            "fd1625d5addd2389e2c382f96f76510c",
        );
        program.files.swap(0, 1);
        program.roots = vec![
            ("/work/src/a.ts".into(), 0),
            ("/work/src/b.ts".into(), 1),
            ("/work/lib/min.d.ts".into(), 2),
        ];
        program.referenced_files = vec![vec![], vec![], vec![]];
        program.options.composite = None;
        program.options.incremental = Some(true);
        program.options.declaration = Some(true);
        let mut snapshot = Snapshot::new(program, Some(&old));
        assert!(!snapshot.has_changed_files());
        assert_eq!(snapshot.pending_emit_kind(0), 24);
        assert_eq!(snapshot.pending_emit_kind(1), 24);
        // The library pends too (every unchanged file does); the emit drops
        // it as a file that is not emitted, and the document never lists it.
        assert_eq!(snapshot.pending_emit_kind(2), 24);
        assert_eq!(snapshot.files_without_rows(), vec![false, false, false]);
        snapshot.finish_check();
        snapshot.record_emit(
            vec![
                EmitUpdate {
                    file: 0,
                    emitted_kind: 24,
                    from_cache: false,
                    diagnostics: Vec::new(),
                    declaration: Some(DeclarationEmit {
                        signature: "67cd7ccc14045107336f34154f76a8ca".into(),
                        output_file_name: "/work/dist/src/a.d.ts".into(),
                    }),
                },
                EmitUpdate {
                    file: 1,
                    emitted_kind: 24,
                    from_cache: false,
                    diagnostics: Vec::new(),
                    declaration: Some(DeclarationEmit {
                        signature: "e1d275f86bf4a4a1f6fd0e8d8709f902".into(),
                        output_file_name: "/work/dist/src/b.d.ts".into(),
                    }),
                },
            ],
            &[2],
        );
        let info = snapshot.to_build_info(false).expect("written");
        assert_eq!(
            info.to_json(),
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["../src/a.ts","../src/b.ts","../lib/min.d.ts"],"fileInfos":[{"version":"dcc03016c1e9fa8a9c06f4a9279fde2a","signature":"67cd7ccc14045107336f34154f76a8ca","impliedNodeFormat":1},{"version":"fd1625d5addd2389e2c382f96f76510c","signature":"e1d275f86bf4a4a1f6fd0e8d8709f902","impliedNodeFormat":1},{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"options":{"declaration":true,"module":99,"outDir":"./","rootDir":"..","strict":true,"target":9}}"#
        );
    }
}
