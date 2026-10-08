//! The `.tsbuildinfo` document (tsgo `incremental.BuildInfo`, buildInfo.go)
//! and its exact JSON form.

use std::collections::HashMap;

use tsc_types::CompilerOptions;

use crate::json::{write_number, write_numbers, write_string, ObjectWriter};
use crate::options::{emit_declarations, parse_build_info_options};
use crate::snapshot::FileEmitKind;

/// tsgo `core.Version()`: the only version the reader accepts.
pub const VERSION: &str = "7.1.0-dev";

/// A one-based index into `fileNames`.
pub type FileId = u32;

/// tsgo `BuildInfoRoot`: a root file id, a run of consecutive root file ids,
/// or (non-incremental build info) a root file name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BuildInfoRoot {
    Single(FileId),
    Range(FileId, FileId),
    NonIncremental(String),
}

/// tsgo `BuildInfoFileInfo`: the file's version (the hash of its text), its
/// signature (the hash of its declaration output) and the facts the change
/// computation reads, in the compact forms `newBuildInfoFileInfo` chooses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileInfoEntry {
    /// The signature alone: it equals the version, the file does not affect
    /// the global scope and its implied format is CommonJS.
    Signature(String),
    /// `noSignature: true`: the file has no signature yet.
    NoSignature {
        version: String,
        affects_global_scope: bool,
        implied_node_format: u32,
    },
    /// The full form; `signature` is absent when it equals the version.
    Full {
        version: String,
        signature: Option<String>,
        affects_global_scope: bool,
        implied_node_format: u32,
    },
}

impl FileInfoEntry {
    /// tsgo `newBuildInfoFileInfo`.
    pub fn new(
        version: &str,
        signature: Option<&str>,
        affects_global_scope: bool,
        implied_node_format: u32,
    ) -> Self {
        match signature {
            Some(signature) if signature == version => {
                if !affects_global_scope && implied_node_format == 1 {
                    Self::Signature(signature.to_owned())
                } else {
                    Self::Full {
                        version: version.to_owned(),
                        signature: None,
                        affects_global_scope,
                        implied_node_format,
                    }
                }
            }
            Some(signature) => Self::Full {
                version: version.to_owned(),
                signature: Some(signature.to_owned()),
                affects_global_scope,
                implied_node_format,
            },
            None => Self::NoSignature {
                version: version.to_owned(),
                affects_global_scope,
                implied_node_format,
            },
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Self::Signature(signature) => signature,
            Self::NoSignature { version, .. } | Self::Full { version, .. } => version,
        }
    }

    fn write(&self, out: &mut String) {
        match self {
            Self::Signature(signature) => write_string(out, signature),
            Self::NoSignature {
                version,
                affects_global_scope,
                implied_node_format,
            } => {
                let mut object = ObjectWriter::begin(out);
                object.string_omitzero("version", version);
                object.bool_omitzero("noSignature", true);
                object.bool_omitzero("affectsGlobalScope", *affects_global_scope);
                object.number_omitzero("impliedNodeFormat", *implied_node_format);
                object.end();
            }
            Self::Full {
                version,
                signature,
                affects_global_scope,
                implied_node_format,
            } => {
                let mut object = ObjectWriter::begin(out);
                object.string_omitzero("version", version);
                object.string_omitzero("signature", signature.as_deref().unwrap_or(""));
                object.bool_omitzero("affectsGlobalScope", *affects_global_scope);
                object.number_omitzero("impliedNodeFormat", *implied_node_format);
                object.end();
            }
        }
    }
}

/// tsgo `BuildInfoRepopulateInfo`: a diagnostic chain entry the reader
/// recomputes from the current program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepopulateInfo {
    pub kind: u32,
    pub module_reference: String,
    pub mode: u32,
    pub package_name: String,
}

/// tsgo `BuildInfoDiagnostic`: one cached diagnostic, located in the file
/// it is stored under unless `file` (another file) or `no_file` says
/// otherwise; `pos`/`end` are tsgo's text offsets (UTF-8 bytes).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BuildInfoDiagnostic {
    pub file: Option<FileId>,
    pub no_file: bool,
    pub pos: u32,
    pub end: u32,
    pub code: u32,
    /// tsgo `diagnostics.Category`: 0 warning, 1 error, 2 suggestion, 3
    /// message.
    pub category: u32,
    pub source: String,
    pub message_text: String,
    pub message_key: String,
    pub message_args: Vec<String>,
    pub message_chain: Vec<BuildInfoDiagnostic>,
    pub related_information: Vec<BuildInfoDiagnostic>,
    pub reports_unnecessary: bool,
    pub reports_deprecated: bool,
    pub skipped_on_no_emit: bool,
    pub repopulate_info: Option<RepopulateInfo>,
}

impl BuildInfoDiagnostic {
    fn write(&self, out: &mut String) {
        let mut object = ObjectWriter::begin(out);
        object.number_omitzero("file", self.file.unwrap_or(0));
        object.bool_omitzero("noFile", self.no_file);
        object.number_omitzero("pos", self.pos);
        object.number_omitzero("end", self.end);
        object.number_omitzero("code", self.code);
        object.number_omitzero("category", self.category);
        object.string_omitzero("source", &self.source);
        object.string_omitzero("messageText", &self.message_text);
        object.string_omitzero("messageKey", &self.message_key);
        object.strings_omitzero("messageArgs", &self.message_args);
        object.list_omitzero("messageChain", &self.message_chain, |out, chain| {
            chain.write(out)
        });
        object.list_omitzero(
            "relatedInformation",
            &self.related_information,
            |out, related| related.write(out),
        );
        object.bool_omitzero("reportsUnnecessary", self.reports_unnecessary);
        object.bool_omitzero("reportsDeprecated", self.reports_deprecated);
        object.bool_omitzero("skippedOnNoEmit", self.skipped_on_no_emit);
        if let Some(info) = &self.repopulate_info {
            object.raw("repopulateInfo", |out| {
                let mut object = ObjectWriter::begin(out);
                object.raw("kind", |out| write_number(out, info.kind));
                object.string_omitzero("moduleReference", &info.module_reference);
                object.number_omitzero("mode", info.mode);
                object.string_omitzero("packageName", &info.package_name);
                object.end();
            });
        }
        object.end();
    }
}

fn write_diagnostics_of_file(out: &mut String, file: FileId, diagnostics: &[BuildInfoDiagnostic]) {
    out.push('[');
    write_number(out, file);
    out.push_str(",[");
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        diagnostic.write(out);
    }
    out.push_str("]]");
}

/// tsgo `BuildInfoSemanticDiagnostic`: a file whose semantic diagnostics
/// are not cached (and which is not in the change set), or a file's cached
/// diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SemanticDiagnosticEntry {
    NotCached(FileId),
    Diagnostics(FileId, Vec<BuildInfoDiagnostic>),
}

/// tsgo `BuildInfoEmitSignature`: a file emitted with no recorded
/// declaration signature, or whose recorded signature differs from the
/// file's signature (as a plain string, or in the `declarationMap`-flavored
/// forms).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EmitSignatureEntry {
    NoEmitSignature(FileId),
    Signature(FileId, String),
    DiffersOnlyInDtsMap(FileId),
    DiffersInOptions(FileId, String),
}

/// A serialized compiler option value.
#[derive(Clone, Debug, PartialEq)]
pub enum OptionValue {
    Bool(bool),
    Number(i64),
    String(String),
}

/// tsgo `BuildInfo`, with its fields in the struct (and JSON) order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildInfo {
    pub version: String,
    /// The incremental form (tsgo's non-nil `FileInfos`): an incremental
    /// program writes its file infos even when it has no file; the
    /// non-incremental build info of `tsc -b` writes none.
    pub incremental: bool,
    pub errors: bool,
    pub check_pending: bool,
    pub root: Vec<BuildInfoRoot>,
    pub package_jsons: Vec<String>,
    pub missing_package_jsons: Vec<String>,
    pub content_mapper_identities: Vec<String>,
    pub file_names: Vec<String>,
    pub file_infos: Vec<FileInfoEntry>,
    pub file_ids_list: Vec<Vec<FileId>>,
    pub options: Vec<(&'static str, OptionValue)>,
    pub referenced_map: Vec<(FileId, u32)>,
    pub semantic_diagnostics_per_file: Vec<SemanticDiagnosticEntry>,
    pub emit_diagnostics_per_file: Vec<(FileId, Vec<BuildInfoDiagnostic>)>,
    pub change_file_set: Vec<FileId>,
    /// `(file, emit kind)`; the kind is 0 when it is the options' full kind.
    pub affected_files_pending_emit: Vec<(FileId, u32)>,
    pub latest_changed_dts_file: String,
    pub emit_signatures: Vec<EmitSignatureEntry>,
    pub resolved_root: Vec<(FileId, FileId)>,
    pub semantic_errors: bool,
}

impl BuildInfo {
    /// The document's bytes: `json.Marshal` of tsgo's `BuildInfo`.
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(4096);
        let mut object = ObjectWriter::begin(&mut out);
        object.string_omitzero("version", &self.version);
        object.bool_omitzero("errors", self.errors);
        object.bool_omitzero("checkPending", self.check_pending);
        object.list_omitzero("root", &self.root, |out, root| match root {
            BuildInfoRoot::Single(file) => write_number(out, *file),
            BuildInfoRoot::Range(start, end) => write_numbers(out, &[*start, *end]),
            BuildInfoRoot::NonIncremental(name) => write_string(out, name),
        });
        object.strings_omitzero("packageJsons", &self.package_jsons);
        object.strings_omitzero("missingPackageJsons", &self.missing_package_jsons);
        object.strings_omitzero("contentMapperIdentities", &self.content_mapper_identities);
        object.strings_omitzero("fileNames", &self.file_names);
        // tsgo maps the program's files into a non-nil slice
        // (snapshottobuildinfo.go setFileInfoAndEmitSignatures), so a
        // program without files writes `"fileInfos":[]` while its other
        // lists stay absent.
        if self.incremental {
            object.list("fileInfos", &self.file_infos, |out, info| info.write(out));
        }
        object.list_omitzero("fileIdsList", &self.file_ids_list, |out, ids| {
            write_numbers(out, ids)
        });
        if !self.options.is_empty() {
            object.raw("options", |out| {
                let mut options = ObjectWriter::begin(out);
                for (name, value) in &self.options {
                    match value {
                        OptionValue::Bool(value) => options.raw(name, |out| {
                            out.push_str(if *value { "true" } else { "false" })
                        }),
                        OptionValue::Number(value) => {
                            options.raw(name, |out| out.push_str(&value.to_string()))
                        }
                        OptionValue::String(value) => options.string(name, value),
                    }
                }
                options.end();
            });
        }
        object.list_omitzero(
            "referencedMap",
            &self.referenced_map,
            |out, (file, list)| write_numbers(out, &[*file, *list]),
        );
        object.list_omitzero(
            "semanticDiagnosticsPerFile",
            &self.semantic_diagnostics_per_file,
            |out, entry| match entry {
                SemanticDiagnosticEntry::NotCached(file) => write_number(out, *file),
                SemanticDiagnosticEntry::Diagnostics(file, diagnostics) => {
                    write_diagnostics_of_file(out, *file, diagnostics)
                }
            },
        );
        object.list_omitzero(
            "emitDiagnosticsPerFile",
            &self.emit_diagnostics_per_file,
            |out, (file, diagnostics)| write_diagnostics_of_file(out, *file, diagnostics),
        );
        object.list_omitzero("changeFileSet", &self.change_file_set, |out, file| {
            write_number(out, *file)
        });
        object.list_omitzero(
            "affectedFilesPendingEmit",
            &self.affected_files_pending_emit,
            |out, (file, kind)| match *kind {
                0 => write_number(out, *file),
                crate::snapshot::FileEmitKind::DTS => write_numbers(out, &[*file]),
                kind => write_numbers(out, &[*file, kind]),
            },
        );
        object.string_omitzero("latestChangedDtsFile", &self.latest_changed_dts_file);
        object.list_omitzero(
            "emitSignatures",
            &self.emit_signatures,
            |out, entry| match entry {
                EmitSignatureEntry::NoEmitSignature(file) => write_number(out, *file),
                EmitSignatureEntry::Signature(file, signature) => {
                    out.push('[');
                    write_number(out, *file);
                    out.push(',');
                    write_string(out, signature);
                    out.push(']');
                }
                EmitSignatureEntry::DiffersOnlyInDtsMap(file) => {
                    out.push('[');
                    write_number(out, *file);
                    out.push_str(",[]]");
                }
                EmitSignatureEntry::DiffersInOptions(file, signature) => {
                    out.push('[');
                    write_number(out, *file);
                    out.push_str(",[");
                    write_string(out, signature);
                    out.push_str("]]");
                }
            },
        );
        object.list_omitzero(
            "resolvedRoot",
            &self.resolved_root,
            |out, (resolved, root)| write_numbers(out, &[*resolved, *root]),
        );
        object.bool_omitzero("semanticErrors", self.semantic_errors);
        object.end();
        out
    }
}

impl BuildInfo {
    /// The name of a file by its id (ids are 1-based).
    pub fn file_name_of(&self, id: FileId) -> Option<&str> {
        self.file_names
            .get(id.checked_sub(1)? as usize)
            .map(String::as_str)
    }

    /// The file info of a file by its id.
    pub fn file_info_of(&self, id: FileId) -> Option<&FileInfoEntry> {
        self.file_infos.get(id.checked_sub(1)? as usize)
    }

    /// tsgo `BuildInfo.IsEmitPending` (`tsc -b`'s up-to-date check):
    /// whether the options the build info recorded leave an emit pending
    /// under the current ones; `absolute` resolves a recorded path option.
    pub fn is_emit_pending(
        &self,
        options: &CompilerOptions,
        absolute: &dyn Fn(&str) -> String,
    ) -> bool {
        let no_emit = options.no_emit == Some(true);
        if no_emit && !emit_declarations(options) {
            return false;
        }
        let old = parse_build_info_options(&self.options, absolute);
        let mut pending = FileEmitKind::pending(
            FileEmitKind::of_options(options),
            FileEmitKind::of_options(&old),
        );
        if no_emit {
            pending &= FileEmitKind::DTS_ERRORS;
        }
        pending != FileEmitKind::NONE
    }

    /// tsgo `GetBuildInfoRootInfoReader`: the roots the build info recorded
    /// and the file info of each, through the resolved roots.
    /// `canonical(name, directory)` resolves a recorded name against the
    /// build info directory to a canonical path.
    pub fn root_info_reader(
        &self,
        build_info_directory: &str,
        canonical: &dyn Fn(&str, &str) -> String,
    ) -> RootInfoReader {
        let to_path = |name: &str| canonical(name, build_info_directory);
        let mut resolved_to_root: HashMap<String, String> = HashMap::new();
        // tsgo `BuildInfoResolvedRoot` is `[resolved, root]`.
        for (resolved, root) in &self.resolved_root {
            if let (Some(resolved), Some(root)) =
                (self.file_name_of(*resolved), self.file_name_of(*root))
            {
                resolved_to_root.insert(to_path(resolved), to_path(root));
            }
        }
        let mut reader = RootInfoReader::default();
        let mut add_root = |resolved_root: &str, file_info: Option<&FileInfoEntry>| {
            if resolved_root.is_empty() {
                return;
            }
            let resolved_path = to_path(resolved_root);
            let root_path = resolved_to_root
                .get(&resolved_path)
                .cloned()
                .unwrap_or_else(|| resolved_path.clone());
            reader.set_root(root_path, resolved_path.clone());
            if let Some(file_info) = file_info {
                reader
                    .resolved_root_file_infos
                    .insert(resolved_path, file_info.clone());
            }
        };
        for root in &self.root {
            match root {
                BuildInfoRoot::NonIncremental(name) => add_root(name, None),
                BuildInfoRoot::Single(id) => {
                    if let Some(name) = self.file_name_of(*id) {
                        add_root(name, self.file_info_of(*id));
                    }
                }
                BuildInfoRoot::Range(start, end) => {
                    for id in *start..=*end {
                        if let Some(name) = self.file_name_of(id) {
                            add_root(name, self.file_info_of(id));
                        }
                    }
                }
            }
        }
        reader
    }
}

/// tsgo `BuildInfoRootInfoReader`.
#[derive(Clone, Debug, Default)]
pub struct RootInfoReader {
    resolved_root_file_infos: HashMap<String, FileInfoEntry>,
    /// Root path to resolved path, in insertion order (tsgo's ordered map).
    root_to_resolved: Vec<(String, String)>,
    root_index: HashMap<String, usize>,
}

impl RootInfoReader {
    fn set_root(&mut self, root: String, resolved: String) {
        match self.root_index.get(&root) {
            Some(&index) => self.root_to_resolved[index].1 = resolved,
            None => {
                self.root_index
                    .insert(root.clone(), self.root_to_resolved.len());
                self.root_to_resolved.push((root, resolved));
            }
        }
    }

    /// tsgo `GetBuildInfoFileInfo`: the file info of a root or a resolved
    /// root (absent for a non-incremental root) and the resolved path.
    pub fn file_info(&self, path: &str) -> Option<(Option<&FileInfoEntry>, &str)> {
        if let Some((resolved, info)) = self.resolved_root_file_infos.get_key_value(path) {
            return Some((Some(info), resolved.as_str()));
        }
        let &index = self.root_index.get(path)?;
        let resolved = self.root_to_resolved[index].1.as_str();
        Some((self.resolved_root_file_infos.get(resolved), resolved))
    }

    /// The recorded roots, in order.
    pub fn roots(&self) -> impl Iterator<Item = &str> {
        self.root_to_resolved.iter().map(|(root, _)| root.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_infos_take_the_compact_forms() {
        assert_eq!(
            FileInfoEntry::new("v", Some("v"), false, 1),
            FileInfoEntry::Signature("v".into())
        );
        let mut out = String::new();
        FileInfoEntry::new("v", Some("v"), true, 1).write(&mut out);
        assert_eq!(
            out,
            r#"{"version":"v","affectsGlobalScope":true,"impliedNodeFormat":1}"#
        );
        out.clear();
        FileInfoEntry::new("v", Some("s"), false, 1).write(&mut out);
        assert_eq!(
            out,
            r#"{"version":"v","signature":"s","impliedNodeFormat":1}"#
        );
        out.clear();
        FileInfoEntry::new("v", Some("v"), false, 0).write(&mut out);
        assert_eq!(out, r#"{"version":"v"}"#);
        out.clear();
        FileInfoEntry::new("v", None, false, 99).write(&mut out);
        assert_eq!(
            out,
            r#"{"version":"v","noSignature":true,"impliedNodeFormat":99}"#
        );
    }

    #[test]
    fn writes_the_document_in_struct_order() {
        let info = BuildInfo {
            version: VERSION.into(),
            incremental: true,
            errors: true,
            root: vec![BuildInfoRoot::Range(1, 2), BuildInfoRoot::Single(4)],
            file_names: vec!["../src/a.ts".into(), "../src/b.ts".into()],
            file_infos: vec![
                FileInfoEntry::Signature("aa".into()),
                FileInfoEntry::new("bb", Some("cc"), false, 1),
            ],
            file_ids_list: vec![vec![1]],
            options: vec![
                ("composite", OptionValue::Bool(true)),
                ("module", OptionValue::Number(99)),
                ("outDir", OptionValue::String("./".into())),
            ],
            referenced_map: vec![(2, 1)],
            semantic_diagnostics_per_file: vec![
                SemanticDiagnosticEntry::NotCached(1),
                SemanticDiagnosticEntry::Diagnostics(
                    2,
                    vec![BuildInfoDiagnostic {
                        pos: 0,
                        end: 3,
                        code: 2322,
                        category: 1,
                        message_key: "Type_0_is_not_assignable_to_type_1_2322".into(),
                        message_args: vec!["number".into(), "string".into()],
                        related_information: vec![BuildInfoDiagnostic {
                            file: Some(1),
                            pos: 5,
                            end: 6,
                            code: 6500,
                            category: 3,
                            message_key: "K_6500".into(),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }],
                ),
            ],
            affected_files_pending_emit: vec![(2, 17), (1, 0), (3, 24)],
            latest_changed_dts_file: "./src/a.d.ts".into(),
            emit_signatures: vec![
                EmitSignatureEntry::NoEmitSignature(1),
                EmitSignatureEntry::Signature(2, "s".into()),
                EmitSignatureEntry::DiffersOnlyInDtsMap(3),
                EmitSignatureEntry::DiffersInOptions(4, "t".into()),
            ],
            resolved_root: vec![(3, 5)],
            ..Default::default()
        };
        assert_eq!(
            info.to_json(),
            concat!(
                r#"{"version":"7.1.0-dev","errors":true,"root":[[1,2],4],"fileNames":["../src/a.ts","../src/b.ts"],"#,
                r#""fileInfos":["aa",{"version":"bb","signature":"cc","impliedNodeFormat":1}],"fileIdsList":[[1]],"#,
                r#""options":{"composite":true,"module":99,"outDir":"./"},"referencedMap":[[2,1]],"#,
                r#""semanticDiagnosticsPerFile":[1,[2,[{"end":3,"code":2322,"category":1,"#,
                r#""messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["number","string"],"#,
                r#""relatedInformation":[{"file":1,"pos":5,"end":6,"code":6500,"category":3,"messageKey":"K_6500"}]}]]],"#,
                r#""affectedFilesPendingEmit":[[2,17],1,[3]],"latestChangedDtsFile":"./src/a.d.ts","#,
                r#""emitSignatures":[1,[2,"s"],[3,[]],[4,["t"]]],"resolvedRoot":[[3,5]]}"#
            )
        );
    }

    #[test]
    fn a_resolved_root_stands_for_its_source() {
        // A root in a referenced project: the program holds the project's
        // output (file 2) in its source's place (file 3), and the build
        // info records `resolvedRoot` as [resolved, root].
        let info = BuildInfo {
            root: vec![BuildInfoRoot::Range(1, 2)],
            file_names: vec![
                "./src/a.ts".into(),
                "../shared/dist/b.d.ts".into(),
                "../shared/src/b.ts".into(),
            ],
            file_infos: vec![
                FileInfoEntry::Signature("aa".into()),
                FileInfoEntry::Signature("bb".into()),
                FileInfoEntry::Signature("cc".into()),
            ],
            resolved_root: vec![(2, 3)],
            ..Default::default()
        };
        let reader = info.root_info_reader("/p/out", &|name: &str, directory: &str| {
            let mut parts: Vec<&str> = directory.split('/').collect();
            for part in name.split('/') {
                match part {
                    "." => {}
                    ".." => {
                        parts.pop();
                    }
                    part => parts.push(part),
                }
            }
            parts.join("/")
        });
        assert_eq!(
            reader.roots().collect::<Vec<_>>(),
            ["/p/out/src/a.ts", "/p/shared/src/b.ts"]
        );
        let (file_info, resolved) = reader.file_info("/p/shared/src/b.ts").unwrap();
        assert_eq!(resolved, "/p/shared/dist/b.d.ts");
        assert_eq!(file_info, Some(&FileInfoEntry::Signature("bb".into())));
    }
}
