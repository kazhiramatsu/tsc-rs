//! The old program's state read back from its build info (tsgo
//! `buildInfoToSnapshot`, buildinfotosnapshot.go): file infos, options,
//! references, cached rows, pending emits and emit signatures keyed by the
//! files' canonical paths.

use std::collections::{BTreeMap, BTreeSet};

use tsc_types::CompilerOptions;

use crate::build_info::{
    BuildInfo, BuildInfoDiagnostic, EmitSignatureEntry, FileId, FileInfoEntry, RepopulateInfo,
    SemanticDiagnosticEntry,
};
use crate::options::parse_build_info_options;
use crate::snapshot::FileEmitKind;

/// tsgo `buildInfoDiagnosticWithFileName`: a cached diagnostic whose other
/// file (when it has one) is named by its canonical path.
#[derive(Clone, Debug, PartialEq)]
pub struct CachedDiagnostic {
    /// The file the diagnostic is located in when it is not the file it is
    /// stored under.
    pub file: Option<String>,
    pub no_file: bool,
    pub pos: u32,
    pub end: u32,
    pub code: u32,
    pub category: u32,
    pub source: String,
    pub message_text: String,
    pub message_key: String,
    pub message_args: Vec<String>,
    pub message_chain: Vec<CachedDiagnostic>,
    pub related_information: Vec<CachedDiagnostic>,
    pub reports_unnecessary: bool,
    pub reports_deprecated: bool,
    pub skipped_on_no_emit: bool,
    pub repopulate_info: Option<RepopulateInfo>,
}

/// tsgo `FileInfo` of the old program; an empty `signature` is none.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OldFileInfo {
    pub version: String,
    pub signature: String,
    pub affects_global_scope: bool,
    pub implied_node_format: u32,
}

/// tsgo `emitSignature`: the hash of the emitted declaration file, as a
/// plain string when it was emitted with the current `declarationMap`
/// setting, else in the one-element form.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmitSignature {
    pub signature: String,
    pub with_different_options: Option<String>,
}

impl EmitSignature {
    pub fn plain(signature: String) -> Self {
        Self {
            signature,
            with_different_options: None,
        }
    }

    /// The hash either form holds (tsgo `skipDtsOutputOfComposite`'s
    /// `oldSignature`).
    pub fn value(&self) -> &str {
        if self.signature.is_empty() {
            self.with_different_options.as_deref().unwrap_or("")
        } else {
            &self.signature
        }
    }

    /// Whether the form is the plain one (emitted with the same
    /// `declarationMap` setting).
    pub fn is_plain(&self) -> bool {
        !self.signature.is_empty()
    }

    /// tsgo `getNewEmitSignature`: the form under the new options (swapped
    /// when the `declarationMap` setting differs).
    pub fn for_new_options(&self, old_declaration_map: bool, new_declaration_map: bool) -> Self {
        if old_declaration_map == new_declaration_map {
            return self.clone();
        }
        match &self.with_different_options {
            None => Self {
                signature: String::new(),
                with_different_options: Some(self.signature.clone()),
            },
            Some(signature) => Self::plain(signature.clone()),
        }
    }
}

/// How the old state's paths are resolved: `canonical(name, directory)` is
/// tsgo `tspath.ToPath` (normalized, absolute against `directory`, cased
/// per the host), `absolute(name, directory)` is
/// `tspath.GetNormalizedAbsolutePath`.
pub struct OldStatePaths<'a> {
    pub build_info_directory: &'a str,
    pub default_library_directory: &'a str,
    pub canonical: &'a dyn Fn(&str, &str) -> String,
    pub absolute: &'a dyn Fn(&str, &str) -> String,
}

/// tsgo `snapshot` as `buildInfoToSnapshot` restores it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OldState {
    pub file_infos: BTreeMap<String, OldFileInfo>,
    pub options: CompilerOptions,
    pub references: BTreeMap<String, BTreeSet<String>>,
    pub changed_files: BTreeSet<String>,
    /// The files with cached semantic rows (empty rows included); a file
    /// without an entry has none.
    pub semantic_rows: BTreeMap<String, Vec<CachedDiagnostic>>,
    pub emit_rows: BTreeMap<String, Vec<CachedDiagnostic>>,
    pub pending_emit: BTreeMap<String, u32>,
    /// Absolute.
    pub latest_changed_dts_file: Option<String>,
    pub emit_signatures: BTreeMap<String, EmitSignature>,
    pub has_errors: bool,
    pub has_semantic_errors: bool,
    pub check_pending: bool,
    /// Absolute, as recorded (sorted and deduplicated by the writer).
    pub package_jsons: Vec<String>,
    pub missing_package_jsons: Vec<String>,
}

/// tsgo `IsBuildInfoFileNameDefaultLibrary`: neither relative nor
/// absolute.
fn is_default_library_name(name: &str) -> bool {
    !crate::snapshot::path_is_relative(name) && !crate::snapshot::path_is_absolute(name)
}

struct ToSnapshot<'a> {
    paths: &'a OldStatePaths<'a>,
    file_paths: Vec<String>,
}

impl ToSnapshot<'_> {
    fn file_path(&self, id: FileId) -> Option<&str> {
        self.file_paths
            .get(usize::try_from(id).ok()?.checked_sub(1)?)
            .map(String::as_str)
    }

    fn absolute(&self, name: &str) -> String {
        (self.paths.absolute)(name, self.paths.build_info_directory)
    }

    fn cached(&self, diagnostic: &BuildInfoDiagnostic) -> CachedDiagnostic {
        CachedDiagnostic {
            file: diagnostic
                .file
                .and_then(|file| self.file_path(file))
                .map(str::to_owned),
            no_file: diagnostic.no_file,
            pos: diagnostic.pos,
            end: diagnostic.end,
            code: diagnostic.code,
            category: diagnostic.category,
            source: diagnostic.source.clone(),
            message_text: diagnostic.message_text.clone(),
            message_key: diagnostic.message_key.clone(),
            message_args: diagnostic.message_args.clone(),
            message_chain: diagnostic
                .message_chain
                .iter()
                .map(|chain| self.cached(chain))
                .collect(),
            related_information: diagnostic
                .related_information
                .iter()
                .map(|related| self.cached(related))
                .collect(),
            reports_unnecessary: diagnostic.reports_unnecessary,
            reports_deprecated: diagnostic.reports_deprecated,
            skipped_on_no_emit: diagnostic.skipped_on_no_emit,
            repopulate_info: diagnostic.repopulate_info.clone(),
        }
    }
}

impl OldState {
    /// tsgo `buildInfoToSnapshot` of a document that `IsValidVersion` and
    /// `IsIncremental`.
    pub fn from_build_info(info: &BuildInfo, paths: &OldStatePaths<'_>) -> Self {
        let file_paths = info
            .file_names
            .iter()
            .map(|name| {
                if is_default_library_name(name) {
                    let joined = format!(
                        "{}/{name}",
                        paths.default_library_directory.trim_end_matches('/')
                    );
                    (paths.canonical)(&joined, paths.build_info_directory)
                } else {
                    (paths.canonical)(name, paths.build_info_directory)
                }
            })
            .collect::<Vec<_>>();
        let to = ToSnapshot { paths, file_paths };
        let file_sets = info
            .file_ids_list
            .iter()
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| to.file_path(*id))
                    .map(str::to_owned)
                    .collect::<BTreeSet<_>>()
            })
            .collect::<Vec<_>>();
        let options = parse_build_info_options(&info.options, &|path| to.absolute(path));
        let composite = options.composite == Some(true);

        let mut state = OldState {
            options,
            ..OldState::default()
        };
        // setFileInfoAndEmitSignatures
        for (index, entry) in info.file_infos.iter().enumerate() {
            let Some(path) = to.file_path(FileId::try_from(index + 1).expect("file id")) else {
                continue;
            };
            let file_info = match entry {
                FileInfoEntry::Signature(signature) => OldFileInfo {
                    version: signature.clone(),
                    signature: signature.clone(),
                    affects_global_scope: false,
                    implied_node_format: 1,
                },
                FileInfoEntry::NoSignature {
                    version,
                    affects_global_scope,
                    implied_node_format,
                } => OldFileInfo {
                    version: version.clone(),
                    signature: String::new(),
                    affects_global_scope: *affects_global_scope,
                    implied_node_format: *implied_node_format,
                },
                FileInfoEntry::Full {
                    version,
                    signature,
                    affects_global_scope,
                    implied_node_format,
                } => OldFileInfo {
                    version: version.clone(),
                    signature: signature.clone().unwrap_or_else(|| version.clone()),
                    affects_global_scope: *affects_global_scope,
                    implied_node_format: *implied_node_format,
                },
            };
            if composite && !file_info.signature.is_empty() {
                state.emit_signatures.insert(
                    path.to_owned(),
                    EmitSignature::plain(file_info.signature.clone()),
                );
            }
            state.file_infos.insert(path.to_owned(), file_info);
        }
        for entry in &info.emit_signatures {
            match entry {
                EmitSignatureEntry::NoEmitSignature(file) => {
                    if let Some(path) = to.file_path(*file) {
                        state.emit_signatures.remove(path);
                    }
                }
                EmitSignatureEntry::Signature(file, signature) => {
                    if let Some(path) = to.file_path(*file) {
                        state
                            .emit_signatures
                            .insert(path.to_owned(), EmitSignature::plain(signature.clone()));
                    }
                }
                EmitSignatureEntry::DiffersOnlyInDtsMap(file) => {
                    if let Some(path) = to.file_path(*file) {
                        let existing = state
                            .emit_signatures
                            .get(path)
                            .map(|signature| signature.signature.clone())
                            .unwrap_or_default();
                        state.emit_signatures.insert(
                            path.to_owned(),
                            EmitSignature {
                                signature: String::new(),
                                with_different_options: Some(existing),
                            },
                        );
                    }
                }
                EmitSignatureEntry::DiffersInOptions(file, signature) => {
                    if let Some(path) = to.file_path(*file) {
                        state.emit_signatures.insert(
                            path.to_owned(),
                            EmitSignature {
                                signature: String::new(),
                                with_different_options: Some(signature.clone()),
                            },
                        );
                    }
                }
            }
        }
        // setReferencedMap
        for (file, list) in &info.referenced_map {
            if let (Some(path), Some(set)) = (
                to.file_path(*file),
                usize::try_from(*list)
                    .ok()
                    .and_then(|list| list.checked_sub(1))
                    .and_then(|list| file_sets.get(list)),
            ) {
                state.references.insert(path.to_owned(), set.clone());
            }
        }
        // setChangeFileSet
        for file in &info.change_file_set {
            if let Some(path) = to.file_path(*file) {
                state.changed_files.insert(path.to_owned());
            }
        }
        // setSemanticDiagnostics: every file not in the change set starts
        // with no diagnostics; the entries then remove or fill.
        for path in state.file_infos.keys() {
            if !state.changed_files.contains(path) {
                state.semantic_rows.insert(path.clone(), Vec::new());
            }
        }
        for entry in &info.semantic_diagnostics_per_file {
            match entry {
                SemanticDiagnosticEntry::NotCached(file) => {
                    if let Some(path) = to.file_path(*file) {
                        state.semantic_rows.remove(path);
                    }
                }
                SemanticDiagnosticEntry::Diagnostics(file, rows) => {
                    if let Some(path) = to.file_path(*file) {
                        state.semantic_rows.insert(
                            path.to_owned(),
                            rows.iter().map(|row| to.cached(row)).collect(),
                        );
                    }
                }
            }
        }
        // setEmitDiagnostics
        for (file, rows) in &info.emit_diagnostics_per_file {
            if let Some(path) = to.file_path(*file) {
                state.emit_rows.insert(
                    path.to_owned(),
                    rows.iter().map(|row| to.cached(row)).collect(),
                );
            }
        }
        // setAffectedFilesPendingEmit: the full kind of the old options is
        // written as 0.
        let own_kind = FileEmitKind::of_options(&state.options);
        for (file, kind) in &info.affected_files_pending_emit {
            if let Some(path) = to.file_path(*file) {
                state
                    .pending_emit
                    .insert(path.to_owned(), if *kind == 0 { own_kind } else { *kind });
            }
        }
        if !info.latest_changed_dts_file.is_empty() {
            state.latest_changed_dts_file = Some(to.absolute(&info.latest_changed_dts_file));
        }
        state.has_errors = info.errors;
        state.has_semantic_errors = info.semantic_errors;
        state.check_pending = info.check_pending;
        // setPackageJsons
        state.package_jsons = info
            .package_jsons
            .iter()
            .map(|path| to.absolute(path))
            .collect();
        state.missing_package_jsons = info
            .missing_package_jsons
            .iter()
            .map(|path| to.absolute(path))
            .collect();
        state
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A lexical join for the tests' `./`-relative names.
    pub(crate) fn join(name: &str, directory: &str) -> String {
        if name.starts_with('/') {
            return name.to_owned();
        }
        let mut parts: Vec<&str> = directory
            .split('/')
            .filter(|part| !part.is_empty())
            .collect();
        for part in name.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop();
                }
                part => parts.push(part),
            }
        }
        format!("/{}", parts.join("/"))
    }

    pub(crate) fn paths<'a>(directory: &'a str, library: &'a str) -> OldStatePaths<'a> {
        OldStatePaths {
            build_info_directory: directory,
            default_library_directory: library,
            canonical: &join,
            absolute: &join,
        }
    }

    #[test]
    fn restores_the_noemit_chain_state() {
        // tsgo's build info after the noemit-chain scenario's first step.
        let info = BuildInfo::from_json(concat!(
            r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","lib.es5.d.ts"],"#,
            r#""fileInfos":["1ee67a763a8635e8d85574ccc6e5740e","c4513d6b24fb8c179ec11446075e21ed",{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"#,
            r#""fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"#,
            r#""semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["number","string"]}]]],"#,
            r#""affectedFilesPendingEmit":[2,1]}"#
        ))
        .expect("parses");
        let state = OldState::from_build_info(&info, &paths("/work", "/libs"));
        assert_eq!(
            state.file_infos.keys().cloned().collect::<Vec<_>>(),
            ["/libs/lib.es5.d.ts", "/work/src/a.ts", "/work/src/b.ts"]
        );
        assert_eq!(
            state.file_infos["/work/src/b.ts"],
            OldFileInfo {
                version: "1ee67a763a8635e8d85574ccc6e5740e".into(),
                signature: "1ee67a763a8635e8d85574ccc6e5740e".into(),
                affects_global_scope: false,
                implied_node_format: 1
            }
        );
        assert_eq!(
            state.file_infos["/libs/lib.es5.d.ts"].signature,
            "bae41f699edb9c9e3f69cdbea446f1f5"
        );
        assert!(state.file_infos["/libs/lib.es5.d.ts"].affects_global_scope);
        assert_eq!(state.options.module, Some(99));
        assert_eq!(state.options.strict, Some(true));
        assert_eq!(
            state.references["/work/src/a.ts"],
            BTreeSet::from(["/work/src/b.ts".to_owned()])
        );
        assert_eq!(state.semantic_rows["/work/src/b.ts"], Vec::new());
        assert_eq!(state.semantic_rows["/libs/lib.es5.d.ts"], Vec::new());
        assert_eq!(state.semantic_rows["/work/src/a.ts"].len(), 1);
        assert_eq!(
            state.semantic_rows["/work/src/a.ts"][0].message_args,
            ["number", "string"]
        );
        // The full kind of the old options (JS only).
        assert_eq!(state.pending_emit["/work/src/a.ts"], FileEmitKind::JS);
        assert_eq!(state.pending_emit["/work/src/b.ts"], FileEmitKind::JS);
        assert!(state.emit_signatures.is_empty());
        assert!(!state.has_errors);
    }

    #[test]
    fn restores_composite_emit_signatures_and_not_cached_rows() {
        let info = BuildInfo::from_json(concat!(
            r#"{"version":"7.1.0-dev","errors":true,"root":[[1,2]],"fileNames":["../src/b.ts","../src/a.ts"],"#,
            r#""fileInfos":[{"version":"vb","signature":"sb","impliedNodeFormat":1},{"version":"va","signature":"sa","impliedNodeFormat":1}],"#,
            r#""options":{"composite":true,"declarationMap":true,"outDir":"./"},"semanticDiagnosticsPerFile":[1],"changeFileSet":[2],"#,
            r#""affectedFilesPendingEmit":[[1],[2,17]],"latestChangedDtsFile":"./src/a.d.ts","emitSignatures":[1,[2,[]]]}"#
        ))
        .expect("parses");
        let state = OldState::from_build_info(&info, &paths("/work/dist", "/libs"));
        assert_eq!(
            state
                .options
                .out_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
            Some("/work/dist".to_owned())
        );
        // b: no emit signature; a: the signature in the other-map form.
        assert!(!state.emit_signatures.contains_key("/work/src/b.ts"));
        assert_eq!(
            state.emit_signatures["/work/src/a.ts"],
            EmitSignature {
                signature: String::new(),
                with_different_options: Some("sa".into())
            }
        );
        assert_eq!(state.emit_signatures["/work/src/a.ts"].value(), "sa");
        // b's rows are not cached; a is in the change set (no rows).
        assert!(state.semantic_rows.is_empty());
        assert_eq!(
            state.changed_files,
            BTreeSet::from(["/work/src/a.ts".to_owned()])
        );
        assert_eq!(state.pending_emit["/work/src/b.ts"], FileEmitKind::DTS);
        assert_eq!(state.pending_emit["/work/src/a.ts"], 17);
        assert_eq!(
            state.latest_changed_dts_file.as_deref(),
            Some("/work/dist/src/a.d.ts")
        );
        assert!(state.has_errors);
    }
}
