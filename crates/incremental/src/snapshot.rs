//! The incremental program's state after one compilation and its
//! serialization (tsgo `programToSnapshot`, `emitFilesHandler`,
//! `snapshotToBuildInfo`) for a program that starts without build info.
//!
//! The compiler driver supplies the facts; nothing here reads a file.

use std::collections::BTreeMap;

use tsc_types::CompilerOptions;

use crate::build_info::{
    BuildInfo, BuildInfoDiagnostic, BuildInfoRoot, EmitSignatureEntry, FileId, FileInfoEntry,
    SemanticDiagnosticEntry, VERSION,
};
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
    pub current_directory: String,
    pub use_case_sensitive_file_names: bool,
    /// The package.json files the resolver read and the ones under
    /// `node_modules` it did not find, as absolute paths, sorted and
    /// deduplicated (tsgo `ensurePackageJsonsForState`).
    pub package_jsons: Vec<String>,
    pub missing_package_jsons: Vec<String>,
}

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

/// tsgo `programToSnapshot` without an old program, the emit or
/// declaration-diagnostics handling of the command, and
/// `snapshotToBuildInfo`.
pub fn build_fresh_build_info(input: &FreshSnapshotInput<'_>) -> BuildInfo {
    let program = input.program;
    let options = &program.options;
    let file_count = program.files.len();
    let full_kind = FileEmitKind::of_options(options);
    let composite = options.composite == Some(true);

    // computeProgramFileChanges without an old program: every file's
    // signature is its version and every file is pending the full emit.
    let mut signatures: Vec<String> = program
        .files
        .iter()
        .map(|file| file.version.clone())
        .collect();
    let mut emit_signatures: Vec<Option<String>> = vec![None; file_count];
    let mut pending: Vec<u32> = vec![full_kind; file_count];
    let mut emit_diagnostics: Vec<Vec<BuildInfoDiagnostic>> = vec![Vec::new(); file_count];
    let mut latest_changed_dts_file: Option<String> = None;

    match input.state {
        FileState::NoEmit {
            declaration_diagnostics: Some(declaration_diagnostics),
        } => {
            // GetDeclarationDiagnostics → emitFilesIncremental(isForDtsErrors):
            // every pending file that may be emitted has its declaration
            // errors handled, which clears that bit of its pending kind.
            for (index, file) in program.files.iter().enumerate() {
                if !file.may_be_emitted {
                    pending[index] = FileEmitKind::NONE;
                    continue;
                }
                let handled = pending[index] & FileEmitKind::DTS_ERRORS;
                if handled == 0 {
                    continue;
                }
                pending[index] = FileEmitKind::pending(pending[index], handled);
                if let Some(diagnostics) = declaration_diagnostics.get(index) {
                    if !diagnostics.is_empty() {
                        emit_diagnostics[index] = diagnostics.clone();
                    }
                }
            }
        }
        FileState::NoEmit {
            declaration_diagnostics: None,
        } => {}
        FileState::Emit(emit) if !emit.skipped => {
            for (index, file) in program.files.iter().enumerate() {
                if !file.may_be_emitted {
                    pending[index] = FileEmitKind::NONE;
                    continue;
                }
                // The file's whole pending kind was emitted.
                pending[index] = FileEmitKind::NONE;
                if let Some(output) = emit.declaration_outputs.get(index).and_then(Option::as_ref) {
                    // getEmitOptions' write hook: the signature of a file
                    // whose signature is still its version becomes the hash
                    // of its declaration text; a composite project records
                    // the emit signature and the latest changed file.
                    signatures[index] = output.signature.clone();
                    if composite {
                        emit_signatures[index] = Some(output.signature.clone());
                        latest_changed_dts_file = Some(output.output_file_name.clone());
                    }
                }
                if let Some(diagnostics) = emit.emit_diagnostics.get(index) {
                    if !diagnostics.is_empty() {
                        emit_diagnostics[index] = diagnostics.clone();
                    }
                }
            }
        }
        FileState::Emit(_) => {}
    }

    // ensureHasErrorsForState for an incremental program.
    let has_emit_diagnostics = emit_diagnostics.iter().any(|rows| !rows.is_empty());
    let errors = if has_emit_diagnostics {
        !is_incremental(options)
    } else {
        input.has_errors_outside_cache
    };

    let mut to = ToBuildInfo::new(program);
    let mut info = BuildInfo {
        version: VERSION.to_owned(),
        ..BuildInfo::default()
    };

    // setFileInfoAndEmitSignatures: the ids of the program files, in order.
    for (index, file) in program.files.iter().enumerate() {
        let id = to.file_id(&file.path);
        debug_assert_eq!(id as usize, index + 1);
        if composite && !file.is_json && file.may_be_emitted {
            match &emit_signatures[index] {
                None => info
                    .emit_signatures
                    .push(EmitSignatureEntry::NoEmitSignature(id)),
                Some(signature) if *signature != signatures[index] => info
                    .emit_signatures
                    .push(EmitSignatureEntry::Signature(id, signature.clone())),
                Some(_) => {}
            }
        }
        info.file_infos.push(FileInfoEntry::new(
            &file.version,
            Some(&signatures[index]),
            file.affects_global_scope,
            file.implied_node_format,
        ));
    }

    // setRootOfIncrementalProgram: roots by their resolved file's id, runs
    // of consecutive ids merged; a root whose file is not the root's own
    // path (a redirect) is recorded as a resolved root.
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

    // setSemanticDiagnostics: the files in Program order; a file without
    // cached rows (and not in the empty change set) is listed alone.
    for (index, file) in program.files.iter().enumerate() {
        let id = to.file_id(&file.path);
        match input.semantic_rows.get(index).and_then(Option::as_ref) {
            None => info
                .semantic_diagnostics_per_file
                .push(SemanticDiagnosticEntry::NotCached(id)),
            Some(rows) if !rows.is_empty() => info
                .semantic_diagnostics_per_file
                .push(SemanticDiagnosticEntry::Diagnostics(id, rows.clone())),
            Some(_) => {}
        }
    }

    // setEmitDiagnostics: by path.
    let mut with_emit_diagnostics: Vec<usize> = (0..file_count)
        .filter(|index| !emit_diagnostics[*index].is_empty())
        .collect();
    with_emit_diagnostics
        .sort_by(|left, right| program.files[*left].path.cmp(&program.files[*right].path));
    for index in with_emit_diagnostics {
        let id = to.file_id(&program.files[index].path);
        info.emit_diagnostics_per_file
            .push((id, emit_diagnostics[index].clone()));
    }

    // setAffectedFilesPendingEmit: by path; files that may not be emitted
    // are dropped; the full kind is written as 0.
    let mut pending_files: Vec<usize> = (0..file_count)
        .filter(|index| {
            pending[*index] != FileEmitKind::NONE && program.files[*index].may_be_emitted
        })
        .collect();
    pending_files.sort_by(|left, right| program.files[*left].path.cmp(&program.files[*right].path));
    for index in pending_files {
        let id = to.file_id(&program.files[index].path);
        let kind = pending[index];
        info.affected_files_pending_emit
            .push((id, if kind == full_kind { 0 } else { kind }));
    }

    if let Some(latest) = &latest_changed_dts_file {
        info.latest_changed_dts_file = to.relative_to_build_info(latest);
    }
    info.errors = errors;
    info.semantic_errors = false;
    info.check_pending = options.no_check == Some(true);
    info.package_jsons = program
        .package_jsons
        .iter()
        .map(|path| to.relative_to_build_info(path))
        .collect();
    info.missing_package_jsons = program
        .missing_package_jsons
        .iter()
        .map(|path| to.relative_to_build_info(path))
        .collect();
    info.file_names = to.file_names;
    info.file_ids_list = to.file_ids_list;
    info
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
}

/// tsgo `tspath.GetDirectoryPath` of a normalized absolute path.
fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

/// tsgo `tspath.EnsurePathIsNonModuleName`: a path that is neither
/// absolute nor relative gets a `./` prefix.
fn ensure_path_is_non_module_name(path: &str) -> String {
    let relative = path == "." || path == ".." || path.starts_with("./") || path.starts_with("../");
    let absolute = path.starts_with('/')
        || path.starts_with("\\\\")
        || path
            .as_bytes()
            .get(1)
            .is_some_and(|byte| *byte == b':' && path.as_bytes()[0].is_ascii_alphabetic())
        || path.contains("://");
    if relative || absolute {
        path.to_owned()
    } else {
        format!("./{path}")
    }
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
    }

    #[test]
    fn non_module_names_get_a_dot_prefix() {
        assert_eq!(ensure_path_is_non_module_name(""), "./");
        assert_eq!(ensure_path_is_non_module_name("src/a.ts"), "./src/a.ts");
        assert_eq!(ensure_path_is_non_module_name("../a.ts"), "../a.ts");
        assert_eq!(ensure_path_is_non_module_name("/a.ts"), "/a.ts");
        assert_eq!(ensure_path_is_non_module_name("c:/a.ts"), "c:/a.ts");
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
}
