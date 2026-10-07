//! The build info document read back (tsgo `buildInfoReader.ReadBuildInfo`:
//! `json.Unmarshal` into `BuildInfo` with the `UnmarshalJSON` forms of
//! buildInfo.go). Any shape the unmarshaller rejects makes the whole
//! document unusable (tsgo returns no build info, and the program starts
//! fresh).

use serde_json::Value;

use crate::build_info::{
    BuildInfo, BuildInfoDiagnostic, BuildInfoRoot, EmitSignatureEntry, FileId, FileInfoEntry,
    OptionValue, RepopulateInfo, SemanticDiagnosticEntry,
};
use crate::options::known_option_name;

fn number(value: &Value) -> Option<u32> {
    let number = value.as_number()?;
    if let Some(value) = number.as_u64() {
        return u32::try_from(value).ok();
    }
    // `int` from a float with no fraction.
    let float = number.as_f64()?;
    (float.fract() == 0.0 && float >= 0.0 && float <= f64::from(u32::MAX)).then_some(float as u32)
}

fn string(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn bool(value: &Value) -> Option<bool> {
    value.as_bool()
}

fn field<'v>(object: &'v Value, name: &str) -> Option<&'v Value> {
    object.get(name).filter(|value| !value.is_null())
}

/// A field of a struct: absent is the zero value; present must parse.
fn optional<T>(
    object: &Value,
    name: &str,
    parse: impl FnOnce(&Value) -> Option<T>,
) -> Option<Option<T>> {
    match field(object, name) {
        None => Some(None),
        Some(value) => parse(value).map(Some),
    }
}

fn list<T>(value: &Value, parse: impl Fn(&Value) -> Option<T>) -> Option<Vec<T>> {
    value.as_array()?.iter().map(parse).collect()
}

fn strings(value: &Value) -> Option<Vec<String>> {
    list(value, string)
}

fn numbers(value: &Value) -> Option<Vec<u32>> {
    list(value, number)
}

fn pair(value: &Value) -> Option<(u32, u32)> {
    let items = value.as_array()?;
    if items.len() != 2 {
        return None;
    }
    Some((number(&items[0])?, number(&items[1])?))
}

fn root(value: &Value) -> Option<BuildInfoRoot> {
    if let Some(items) = value.as_array() {
        if items.len() != 2 {
            return None;
        }
        return Some(BuildInfoRoot::Range(number(&items[0])?, number(&items[1])?));
    }
    if let Some(start) = number(value) {
        return Some(BuildInfoRoot::Single(start));
    }
    string(value).map(BuildInfoRoot::NonIncremental)
}

fn file_info(value: &Value) -> Option<FileInfoEntry> {
    if let Some(signature) = value.as_str() {
        return Some(FileInfoEntry::Signature(signature.to_owned()));
    }
    if !value.is_object() {
        return None;
    }
    let version = optional(value, "version", string)?.unwrap_or_default();
    let affects_global_scope = optional(value, "affectsGlobalScope", bool)?.unwrap_or(false);
    let implied_node_format = optional(value, "impliedNodeFormat", number)?.unwrap_or(0);
    if optional(value, "noSignature", bool)?.unwrap_or(false) {
        return Some(FileInfoEntry::NoSignature {
            version,
            affects_global_scope,
            implied_node_format,
        });
    }
    let signature = optional(value, "signature", string)?.filter(|signature| !signature.is_empty());
    Some(FileInfoEntry::Full {
        version,
        signature,
        affects_global_scope,
        implied_node_format,
    })
}

fn repopulate_info(value: &Value) -> Option<RepopulateInfo> {
    if !value.is_object() {
        return None;
    }
    Some(RepopulateInfo {
        kind: optional(value, "kind", number)?.unwrap_or(0),
        module_reference: optional(value, "moduleReference", string)?.unwrap_or_default(),
        mode: optional(value, "mode", number)?.unwrap_or(0),
        package_name: optional(value, "packageName", string)?.unwrap_or_default(),
    })
}

fn diagnostic(value: &Value) -> Option<BuildInfoDiagnostic> {
    if !value.is_object() {
        return None;
    }
    Some(BuildInfoDiagnostic {
        file: optional(value, "file", number)?.filter(|file| *file != 0),
        no_file: optional(value, "noFile", bool)?.unwrap_or(false),
        pos: optional(value, "pos", number)?.unwrap_or(0),
        end: optional(value, "end", number)?.unwrap_or(0),
        code: optional(value, "code", number)?.unwrap_or(0),
        category: optional(value, "category", number)?.unwrap_or(0),
        source: optional(value, "source", string)?.unwrap_or_default(),
        message_text: optional(value, "messageText", string)?.unwrap_or_default(),
        message_key: optional(value, "messageKey", string)?.unwrap_or_default(),
        message_args: optional(value, "messageArgs", strings)?.unwrap_or_default(),
        message_chain: optional(value, "messageChain", diagnostics)?.unwrap_or_default(),
        related_information: optional(value, "relatedInformation", diagnostics)?
            .unwrap_or_default(),
        reports_unnecessary: optional(value, "reportsUnnecessary", bool)?.unwrap_or(false),
        reports_deprecated: optional(value, "reportsDeprecated", bool)?.unwrap_or(false),
        skipped_on_no_emit: optional(value, "skippedOnNoEmit", bool)?.unwrap_or(false),
        repopulate_info: optional(value, "repopulateInfo", repopulate_info)?,
    })
}

fn diagnostics(value: &Value) -> Option<Vec<BuildInfoDiagnostic>> {
    list(value, diagnostic)
}

/// `[fileId, [diagnostics]]`.
fn diagnostics_of_file(value: &Value) -> Option<(FileId, Vec<BuildInfoDiagnostic>)> {
    let items = value.as_array()?;
    if items.len() != 2 {
        return None;
    }
    Some((number(&items[0])?, diagnostics(&items[1])?))
}

fn semantic_entry(value: &Value) -> Option<SemanticDiagnosticEntry> {
    if let Some(file) = number(value) {
        return Some(SemanticDiagnosticEntry::NotCached(file));
    }
    diagnostics_of_file(value)
        .map(|(file, diagnostics)| SemanticDiagnosticEntry::Diagnostics(file, diagnostics))
}

/// `fileId`, `[fileId]` (declaration emit only) or `[fileId, emitKind]`.
fn pending_emit(value: &Value) -> Option<(FileId, u32)> {
    if let Some(file) = number(value) {
        return Some((file, 0));
    }
    let items = numbers(value)?;
    match items.as_slice() {
        [file] => Some((*file, crate::snapshot::FileEmitKind::DTS)),
        [file, kind] => Some((*file, *kind)),
        _ => None,
    }
}

fn emit_signature(value: &Value) -> Option<EmitSignatureEntry> {
    if let Some(file) = number(value) {
        return Some(EmitSignatureEntry::NoEmitSignature(file));
    }
    let items = value.as_array()?;
    if items.len() != 2 {
        return None;
    }
    let file = number(&items[0])?;
    if let Some(signature) = items[1].as_str() {
        return Some(EmitSignatureEntry::Signature(file, signature.to_owned()));
    }
    match strings(&items[1])?.as_slice() {
        [] => Some(EmitSignatureEntry::DiffersOnlyInDtsMap(file)),
        [signature] => Some(EmitSignatureEntry::DiffersInOptions(
            file,
            signature.clone(),
        )),
        _ => None,
    }
}

fn options(value: &Value) -> Option<Vec<(&'static str, OptionValue)>> {
    let object = value.as_object()?;
    let mut entries = Vec::new();
    for (name, value) in object {
        let Some(name) = known_option_name(name) else {
            // An unknown option parses to nothing (ParseCompilerOptions'
            // switch has no case for it).
            continue;
        };
        let value = match value {
            Value::Bool(value) => OptionValue::Bool(*value),
            Value::Number(number) => OptionValue::Number(number.as_i64()?),
            Value::String(value) => OptionValue::String(value.clone()),
            _ => continue,
        };
        entries.push((name, value));
    }
    Some(entries)
}

impl BuildInfo {
    /// `json.Unmarshal` of the document's text; `None` where tsgo's reader
    /// returns no build info (the program then starts fresh).
    pub fn from_json(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        if !value.is_object() {
            return None;
        }
        let file_infos = optional(&value, "fileInfos", |value| list(value, file_info))?;
        Some(Self {
            version: optional(&value, "version", string)?.unwrap_or_default(),
            incremental: file_infos.is_some(),
            errors: optional(&value, "errors", bool)?.unwrap_or(false),
            check_pending: optional(&value, "checkPending", bool)?.unwrap_or(false),
            root: optional(&value, "root", |value| list(value, root))?.unwrap_or_default(),
            package_jsons: optional(&value, "packageJsons", strings)?.unwrap_or_default(),
            missing_package_jsons: optional(&value, "missingPackageJsons", strings)?
                .unwrap_or_default(),
            content_mapper_identities: optional(&value, "contentMapperIdentities", strings)?
                .unwrap_or_default(),
            file_names: optional(&value, "fileNames", strings)?.unwrap_or_default(),
            file_infos: file_infos.unwrap_or_default(),
            file_ids_list: optional(&value, "fileIdsList", |value| list(value, numbers))?
                .unwrap_or_default(),
            options: optional(&value, "options", options)?.unwrap_or_default(),
            referenced_map: optional(&value, "referencedMap", |value| list(value, pair))?
                .unwrap_or_default(),
            semantic_diagnostics_per_file: optional(
                &value,
                "semanticDiagnosticsPerFile",
                |value| list(value, semantic_entry),
            )?
            .unwrap_or_default(),
            emit_diagnostics_per_file: optional(&value, "emitDiagnosticsPerFile", |value| {
                list(value, diagnostics_of_file)
            })?
            .unwrap_or_default(),
            change_file_set: optional(&value, "changeFileSet", numbers)?.unwrap_or_default(),
            affected_files_pending_emit: optional(&value, "affectedFilesPendingEmit", |value| {
                list(value, pending_emit)
            })?
            .unwrap_or_default(),
            latest_changed_dts_file: optional(&value, "latestChangedDtsFile", string)?
                .unwrap_or_default(),
            emit_signatures: optional(&value, "emitSignatures", |value| {
                list(value, emit_signature)
            })?
            .unwrap_or_default(),
            resolved_root: optional(&value, "resolvedRoot", |value| list(value, pair))?
                .unwrap_or_default(),
            semantic_errors: optional(&value, "semanticErrors", bool)?.unwrap_or(false),
        })
    }

    /// tsgo `IsValidVersion`.
    pub fn is_valid_version(&self) -> bool {
        self.version == crate::build_info::VERSION
    }

    /// tsgo `IsIncremental`: the document holds an incremental program.
    pub fn is_incremental(&self) -> bool {
        !self.file_names.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOEMIT_CHAIN: &str = r#"{"version":"7.1.0-dev","root":[[1,3]],"fileNames":["./src/b.ts","./src/a.ts","./lib/min.d.ts"],"fileInfos":["1ee67a763a8635e8d85574ccc6e5740e","c4513d6b24fb8c179ec11446075e21ed",{"version":"bae41f699edb9c9e3f69cdbea446f1f5","affectsGlobalScope":true,"impliedNodeFormat":1}],"fileIdsList":[[1]],"options":{"module":99,"strict":true,"target":9},"referencedMap":[[2,1]],"semanticDiagnosticsPerFile":[[2,[{"pos":38,"end":39,"code":2322,"category":1,"messageKey":"Type_0_is_not_assignable_to_type_1_2322","messageArgs":["number","string"]}]]],"affectedFilesPendingEmit":[2,1]}"#;

    #[test]
    fn reads_what_the_writer_wrote() {
        // tsgo's build info of the noemit-chain scenario's first step; the
        // document written back is byte-identical.
        let info = BuildInfo::from_json(NOEMIT_CHAIN).expect("parses");
        assert!(info.is_valid_version());
        assert!(info.is_incremental());
        assert_eq!(info.root, vec![BuildInfoRoot::Range(1, 3)]);
        assert_eq!(
            info.file_infos[2],
            FileInfoEntry::Full {
                version: "bae41f699edb9c9e3f69cdbea446f1f5".into(),
                signature: None,
                affects_global_scope: true,
                implied_node_format: 1
            }
        );
        assert_eq!(info.options[0], ("module", OptionValue::Number(99)));
        assert_eq!(info.affected_files_pending_emit, vec![(2, 0), (1, 0)]);
        assert_eq!(info.to_json(), NOEMIT_CHAIN);
    }

    #[test]
    fn reads_every_compact_form() {
        let text = concat!(
            r#"{"version":"7.1.0-dev","errors":true,"checkPending":true,"root":[1,[2,3],"./x.ts"],"#,
            r#""fileNames":["a","b","c"],"fileInfos":["s",{"version":"v","noSignature":true,"impliedNodeFormat":99},{"version":"v","signature":"t"}],"#,
            r#""fileIdsList":[[1,2]],"options":{"declaration":true,"outDir":"./","target":9,"unknown":5},"#,
            r#""semanticDiagnosticsPerFile":[1,[2,[{"file":3,"pos":1,"end":2,"code":2307,"category":1,"messageKey":"K","messageArgs":["m"],"#,
            r#""messageChain":[{"code":1,"messageText":"t","source":"s"}],"relatedInformation":[{"noFile":true,"code":2}],"#,
            r#""reportsUnnecessary":true,"skippedOnNoEmit":true,"repopulateInfo":{"kind":1,"moduleReference":"m","mode":99,"packageName":"p"}}]]],"#,
            r#""emitDiagnosticsPerFile":[[1,[]]],"changeFileSet":[2],"affectedFilesPendingEmit":[1,[2],[3,24]],"#,
            r#""latestChangedDtsFile":"./a.d.ts","emitSignatures":[1,[2,"s"],[3,[]],[1,["t"]]],"resolvedRoot":[[1,3]],"semanticErrors":true}"#
        );
        let info = BuildInfo::from_json(text).expect("parses");
        assert_eq!(
            info.root,
            vec![
                BuildInfoRoot::Single(1),
                BuildInfoRoot::Range(2, 3),
                BuildInfoRoot::NonIncremental("./x.ts".into())
            ]
        );
        assert_eq!(
            info.file_infos[1],
            FileInfoEntry::NoSignature {
                version: "v".into(),
                affects_global_scope: false,
                implied_node_format: 99
            }
        );
        // The unknown option is dropped.
        assert_eq!(info.options.len(), 3);
        let SemanticDiagnosticEntry::Diagnostics(2, rows) = &info.semantic_diagnostics_per_file[1]
        else {
            panic!("second entry holds rows");
        };
        assert_eq!(rows[0].file, Some(3));
        assert_eq!(rows[0].message_chain[0].message_text, "t");
        assert!(rows[0].related_information[0].no_file);
        assert_eq!(
            rows[0].repopulate_info.as_ref().map(|info| info.mode),
            Some(99)
        );
        assert_eq!(
            info.affected_files_pending_emit,
            vec![(1, 0), (2, crate::snapshot::FileEmitKind::DTS), (3, 24)]
        );
        assert_eq!(
            info.emit_signatures,
            vec![
                EmitSignatureEntry::NoEmitSignature(1),
                EmitSignatureEntry::Signature(2, "s".into()),
                EmitSignatureEntry::DiffersOnlyInDtsMap(3),
                EmitSignatureEntry::DiffersInOptions(1, "t".into())
            ]
        );
        assert_eq!(info.resolved_root, vec![(1, 3)]);
        assert!(info.semantic_errors);
    }

    #[test]
    fn rejects_what_the_unmarshaller_rejects() {
        assert!(BuildInfo::from_json(r#"{"version":"7.1.0-dev","fileNames":["#).is_none());
        assert!(BuildInfo::from_json(r#"[]"#).is_none());
        assert!(BuildInfo::from_json(r#"{"fileNames":"a"}"#).is_none());
        assert!(BuildInfo::from_json(r#"{"root":[[1]]}"#).is_none());
        assert!(BuildInfo::from_json(r#"{"fileInfos":[1]}"#).is_none());
        assert!(BuildInfo::from_json(r#"{"affectedFilesPendingEmit":[[1,2,3]]}"#).is_none());
        let other_version = BuildInfo::from_json(
            r#"{"version":"5.9.0","fileNames":["./src/a.ts"],"fileInfos":["x"]}"#,
        )
        .expect("parses");
        assert!(!other_version.is_valid_version());
        assert!(BuildInfo::from_json(r#"{"version":"7.1.0-dev"}"#)
            .is_some_and(|info| !info.is_incremental()));
    }
}
