//! The readable form tsgo's test file system writes beside every build
//! info (`toReadableBuildInfo`, internal/execute/tsctests/
//! readablebuildinfo.go): the file ids replaced by the file names, each
//! encoded entry beside its original, and the build info's size.

use serde_json::Value;

use tsc_program::go_json::GoJson;

/// The readable text of a parsed build info whose (sanitized) text is
/// `text`.
pub(super) fn readable(info: &Value, text: &str) -> String {
    let names = info["fileNames"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .map(|name| name.as_str().unwrap_or_default().to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let readable = Readable {
        info,
        names: &names,
    };
    readable.render(text.len()).indented()
}

struct Readable<'a> {
    info: &'a Value,
    names: &'a [String],
}

impl Readable<'_> {
    fn name(&self, id: &Value) -> GoJson {
        let index = id.as_u64().unwrap_or(0) as usize;
        GoJson::String(
            index
                .checked_sub(1)
                .and_then(|index| self.names.get(index))
                .cloned()
                .unwrap_or_default(),
        )
    }

    fn field(&self, name: &str) -> Option<&Value> {
        self.info.get(name).filter(|value| !value.is_null())
    }

    fn render(&self, size: usize) -> GoJson {
        let mut out: Vec<(String, GoJson)> = Vec::new();
        let mut put = |key: &str, value: GoJson| out.push((key.to_owned(), value));
        if let Some(version) = self.field("version").and_then(Value::as_str) {
            if !version.is_empty() {
                put("version", GoJson::String(version.to_owned()));
            }
        }
        for flag in ["errors", "checkPending"] {
            if self.field(flag).and_then(Value::as_bool) == Some(true) {
                put(flag, GoJson::Bool(true));
            }
        }
        if let Some(roots) = self.field("root").and_then(Value::as_array) {
            put(
                "root",
                GoJson::Array(roots.iter().map(|root| self.root(root)).collect()),
            );
        }
        for list in ["packageJsons", "missingPackageJsons", "fileNames"] {
            if let Some(value) = self.field(list) {
                put(list, from_value(value));
            }
        }
        if let Some(infos) = self.field("fileInfos").and_then(Value::as_array) {
            put(
                "fileInfos",
                GoJson::Array(
                    infos
                        .iter()
                        .enumerate()
                        .map(|(index, info)| self.file_info(index, info))
                        .collect(),
                ),
            );
        }
        let id_lists = self.field("fileIdsList").and_then(Value::as_array);
        if let Some(lists) = id_lists {
            put(
                "fileIdsList",
                GoJson::Array(lists.iter().map(|list| self.names_of(list)).collect()),
            );
        }
        if let Some(options) = self.field("options") {
            put("options", from_value(options));
        }
        if let Some(map) = self.field("referencedMap").and_then(Value::as_array) {
            put(
                "referencedMap",
                GoJson::Object(
                    map.iter()
                        .map(|entry| {
                            let file = string_of(self.name(&entry[0]));
                            let list = entry[1].as_u64().unwrap_or(0) as usize;
                            let names = id_lists
                                .and_then(|lists| lists.get(list.wrapping_sub(1)))
                                .map(|list| self.names_of(list))
                                .unwrap_or(GoJson::Array(Vec::new()));
                            (file, names)
                        })
                        .collect(),
                ),
            );
        }
        if let Some(entries) = self
            .field("semanticDiagnosticsPerFile")
            .and_then(Value::as_array)
        {
            put(
                "semanticDiagnosticsPerFile",
                GoJson::Array(
                    entries
                        .iter()
                        .map(|entry| {
                            if entry.is_array() {
                                self.diagnostics_of_file(entry)
                            } else {
                                self.name(entry)
                            }
                        })
                        .collect(),
                ),
            );
        }
        if let Some(entries) = self
            .field("emitDiagnosticsPerFile")
            .and_then(Value::as_array)
        {
            put(
                "emitDiagnosticsPerFile",
                GoJson::Array(
                    entries
                        .iter()
                        .map(|entry| self.diagnostics_of_file(entry))
                        .collect(),
                ),
            );
        }
        if let Some(changed) = self.field("changeFileSet") {
            put("changeFileSet", self.names_of(changed));
        }
        if let Some(pending) = self
            .field("affectedFilesPendingEmit")
            .and_then(Value::as_array)
        {
            let full = full_emit_kind(self.field("options"));
            put(
                "affectedFilesPendingEmit",
                GoJson::Array(
                    pending
                        .iter()
                        .map(|entry| {
                            let (id, kind) = match entry {
                                Value::Array(tuple) if tuple.len() == 1 => {
                                    (&tuple[0], FILE_EMIT_KIND_DTS)
                                }
                                Value::Array(tuple) => {
                                    (&tuple[0], tuple[1].as_u64().unwrap_or(0) as u32)
                                }
                                id => (id, 0),
                            };
                            let kind = if kind == 0 { full } else { kind };
                            GoJson::Array(vec![
                                self.name(id),
                                GoJson::String(emit_kind_text(kind)),
                                from_value(entry),
                            ])
                        })
                        .collect(),
                ),
            );
        }
        if let Some(file) = self.field("latestChangedDtsFile").and_then(Value::as_str) {
            if !file.is_empty() {
                put("latestChangedDtsFile", GoJson::String(file.to_owned()));
            }
        }
        if let Some(signatures) = self.field("emitSignatures").and_then(Value::as_array) {
            put(
                "emitSignatures",
                GoJson::Array(
                    signatures
                        .iter()
                        .map(|entry| self.emit_signature(entry))
                        .collect(),
                ),
            );
        }
        if let Some(roots) = self.field("resolvedRoot").and_then(Value::as_array) {
            put(
                "resolvedRoot",
                GoJson::Array(
                    roots
                        .iter()
                        .map(|pair| GoJson::Array(vec![self.name(&pair[0]), self.name(&pair[1])]))
                        .collect(),
                ),
            );
        }
        if size != 0 {
            put("size", GoJson::number(size as i64));
        }
        if self.field("semanticErrors").and_then(Value::as_bool) == Some(true) {
            put("semanticErrors", GoJson::Bool(true));
        }
        GoJson::Object(out)
    }

    fn names_of(&self, ids: &Value) -> GoJson {
        GoJson::Array(
            ids.as_array()
                .map(|ids| ids.iter().map(|id| self.name(id)).collect())
                .unwrap_or_default(),
        )
    }

    /// `readableBuildInfoRoot`: the files of a root entry beside it.
    fn root(&self, root: &Value) -> GoJson {
        let files = match root {
            Value::String(name) => vec![GoJson::String(name.clone())],
            Value::Array(range) => {
                let start = range[0].as_u64().unwrap_or(0);
                let end = range[1].as_u64().unwrap_or(0);
                (start..=end)
                    .map(|id| self.name(&Value::from(id)))
                    .collect()
            }
            id => vec![self.name(id)],
        };
        GoJson::Object(vec![
            ("files".to_owned(), GoJson::Array(files)),
            ("original".to_owned(), from_value(root)),
        ])
    }

    /// `readableBuildInfoFileInfo`: a file's version, signature and format,
    /// with the encoded entry unless it is the plain version string.
    fn file_info(&self, index: usize, info: &Value) -> GoJson {
        let mut out = vec![("fileName".to_owned(), self.name(&Value::from(index + 1)))];
        let (version, signature, affects_global_scope, format, original) = match info {
            Value::String(version) => (version.clone(), version.clone(), false, 1, None),
            object => {
                let version = object["version"].as_str().unwrap_or_default().to_owned();
                let signature = if object["noSignature"].as_bool() == Some(true) {
                    String::new()
                } else {
                    object["signature"]
                        .as_str()
                        .filter(|signature| !signature.is_empty())
                        .unwrap_or(&version)
                        .to_owned()
                };
                (
                    version,
                    signature,
                    object["affectsGlobalScope"].as_bool() == Some(true),
                    object["impliedNodeFormat"].as_u64().unwrap_or(0),
                    Some(from_value(object)),
                )
            }
        };
        if !version.is_empty() {
            out.push(("version".to_owned(), GoJson::String(version)));
        }
        if !signature.is_empty() {
            out.push(("signature".to_owned(), GoJson::String(signature)));
        }
        if affects_global_scope {
            out.push(("affectsGlobalScope".to_owned(), GoJson::Bool(true)));
        }
        out.push((
            "impliedNodeFormat".to_owned(),
            GoJson::String(module_kind_name(format).to_owned()),
        ));
        if let Some(original) = original {
            out.push(("original".to_owned(), original));
        }
        GoJson::Object(out)
    }

    /// `readableBuildInfoDiagnosticsOfFile`: `[file, diagnostics]`.
    fn diagnostics_of_file(&self, entry: &Value) -> GoJson {
        GoJson::Array(vec![self.name(&entry[0]), self.diagnostics(&entry[1])])
    }

    fn diagnostics(&self, list: &Value) -> GoJson {
        GoJson::Array(
            list.as_array()
                .map(|list| {
                    list.iter()
                        .map(|diagnostic| self.diagnostic(diagnostic))
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    /// `readableBuildInfoDiagnostic`: the encoded diagnostic with its file
    /// named (its text and source are not part of the readable form).
    fn diagnostic(&self, diagnostic: &Value) -> GoJson {
        let mut out = Vec::new();
        let Some(object) = diagnostic.as_object() else {
            return GoJson::Object(out);
        };
        if let Some(file) = object
            .get("file")
            .filter(|file| file.as_u64().unwrap_or(0) != 0)
        {
            out.push(("file".to_owned(), self.name(file)));
        }
        for key in [
            "noFile",
            "pos",
            "end",
            "code",
            "category",
            "messageKey",
            "messageArgs",
        ] {
            if let Some(value) = object.get(key) {
                out.push((key.to_owned(), from_value(value)));
            }
        }
        for key in ["messageChain", "relatedInformation"] {
            if let Some(value) = object.get(key) {
                out.push((key.to_owned(), self.diagnostics(value)));
            }
        }
        for key in [
            "reportsUnnecessary",
            "reportsDeprecated",
            "skippedOnNoEmit",
            "repopulateInfo",
        ] {
            if let Some(value) = object.get(key) {
                out.push((key.to_owned(), from_value(value)));
            }
        }
        GoJson::Object(out)
    }

    /// `readableBuildInfoEmitSignature`.
    fn emit_signature(&self, entry: &Value) -> GoJson {
        let mut out = Vec::new();
        match entry {
            Value::Array(pair) => {
                out.push(("file".to_owned(), self.name(&pair[0])));
                match &pair[1] {
                    Value::String(signature) => {
                        if !signature.is_empty() {
                            out.push(("signature".to_owned(), GoJson::String(signature.clone())));
                        }
                    }
                    Value::Array(list) if list.is_empty() => {
                        out.push(("differsOnlyInDtsMap".to_owned(), GoJson::Bool(true)));
                    }
                    Value::Array(list) => {
                        if let Some(signature) = list[0].as_str().filter(|text| !text.is_empty()) {
                            out.push((
                                "signature".to_owned(),
                                GoJson::String(signature.to_owned()),
                            ));
                        }
                        out.push(("differsInOptions".to_owned(), GoJson::Bool(true)));
                    }
                    _ => {}
                }
            }
            id => out.push(("file".to_owned(), self.name(id))),
        }
        out.push(("original".to_owned(), from_value(entry)));
        GoJson::Object(out)
    }
}

const FILE_EMIT_KIND_JS: u32 = 1;
const FILE_EMIT_KIND_JS_MAP: u32 = 1 << 1;
const FILE_EMIT_KIND_JS_INLINE_MAP: u32 = 1 << 2;
const FILE_EMIT_KIND_DTS_ERRORS: u32 = 1 << 3;
const FILE_EMIT_KIND_DTS_EMIT: u32 = 1 << 4;
const FILE_EMIT_KIND_DTS_MAP: u32 = 1 << 5;
const FILE_EMIT_KIND_DTS: u32 = FILE_EMIT_KIND_DTS_ERRORS | FILE_EMIT_KIND_DTS_EMIT;

/// tsgo `GetFileEmitKind` of the build info's options.
fn full_emit_kind(options: Option<&Value>) -> u32 {
    let flag = |name: &str| options.and_then(|options| options[name].as_bool()) == Some(true);
    let mut kind = FILE_EMIT_KIND_JS;
    if flag("sourceMap") {
        kind |= FILE_EMIT_KIND_JS_MAP;
    }
    if flag("inlineSourceMap") {
        kind |= FILE_EMIT_KIND_JS_INLINE_MAP;
    }
    if flag("declaration") || flag("composite") {
        kind |= FILE_EMIT_KIND_DTS;
    }
    if flag("declarationMap") {
        kind |= FILE_EMIT_KIND_DTS_MAP;
    }
    if flag("emitDeclarationOnly") {
        kind &= FILE_EMIT_KIND_DTS | FILE_EMIT_KIND_DTS_MAP;
    }
    kind
}

/// tsgo `toReadableFileEmitKind`.
fn emit_kind_text(kind: u32) -> String {
    let mut parts = Vec::new();
    if kind & FILE_EMIT_KIND_JS != 0 {
        parts.push("Js");
    }
    if kind & FILE_EMIT_KIND_JS_MAP != 0 {
        parts.push("JsMap");
    }
    if kind & FILE_EMIT_KIND_JS_INLINE_MAP != 0 {
        parts.push("JsInlineMap");
    }
    if kind & FILE_EMIT_KIND_DTS == FILE_EMIT_KIND_DTS {
        parts.push("Dts");
    } else {
        if kind & FILE_EMIT_KIND_DTS_EMIT != 0 {
            parts.push("DtsEmit");
        }
        if kind & FILE_EMIT_KIND_DTS_ERRORS != 0 {
            parts.push("DtsErrors");
        }
    }
    if kind & FILE_EMIT_KIND_DTS_MAP != 0 {
        parts.push("DtsMap");
    }
    if parts.is_empty() {
        "None".to_owned()
    } else {
        parts.join("|")
    }
}

/// tsgo `core.ModuleKind.String()` (an implied node format).
fn module_kind_name(kind: u64) -> &'static str {
    match kind {
        0 => "None",
        1 => "CommonJS",
        2 => "AMD",
        3 => "UMD",
        4 => "System",
        5 => "ES2015",
        6 => "ES2020",
        7 => "ES2022",
        99 => "ESNext",
        100 => "Node16",
        101 => "Node18",
        102 => "Node20",
        199 => "NodeNext",
        200 => "Preserve",
        _ => "Unknown",
    }
}

fn string_of(value: GoJson) -> String {
    match value {
        GoJson::String(text) => text,
        _ => String::new(),
    }
}

/// A parsed value in its own key order (serde_json preserves it).
fn from_value(value: &Value) -> GoJson {
    match value {
        Value::Null => GoJson::Null,
        Value::Bool(value) => GoJson::Bool(*value),
        Value::Number(number) => GoJson::Number(number.to_string()),
        Value::String(text) => GoJson::String(text.clone()),
        Value::Array(values) => GoJson::Array(values.iter().map(from_value).collect()),
        Value::Object(object) => GoJson::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), from_value(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_kinds_read_as_tsgo_writes_them() {
        assert_eq!(emit_kind_text(0), "None");
        assert_eq!(
            emit_kind_text(FILE_EMIT_KIND_JS | FILE_EMIT_KIND_DTS),
            "Js|Dts"
        );
        assert_eq!(
            emit_kind_text(FILE_EMIT_KIND_DTS_EMIT | FILE_EMIT_KIND_DTS_MAP),
            "DtsEmit|DtsMap"
        );
    }

    #[test]
    fn a_small_build_info_reads_as_the_reference() {
        let text = r#"{"version":"FakeTSVersion","root":[2],"fileNames":["lib.d.ts","./src/main.ts"],"fileInfos":[{"version":"a","affectsGlobalScope":true,"impliedNodeFormat":1},"b"],"options":{"outDir":"./dist"}}"#;
        let info: Value = serde_json::from_str(text).unwrap();
        assert_eq!(
            readable(&info, text),
            r#"{
  "version": "FakeTSVersion",
  "root": [
    {
      "files": [
        "./src/main.ts"
      ],
      "original": 2
    }
  ],
  "fileNames": [
    "lib.d.ts",
    "./src/main.ts"
  ],
  "fileInfos": [
    {
      "fileName": "lib.d.ts",
      "version": "a",
      "signature": "a",
      "affectsGlobalScope": true,
      "impliedNodeFormat": "CommonJS",
      "original": {
        "version": "a",
        "affectsGlobalScope": true,
        "impliedNodeFormat": 1
      }
    },
    {
      "fileName": "./src/main.ts",
      "version": "b",
      "signature": "b",
      "impliedNodeFormat": "CommonJS"
    }
  ],
  "options": {
    "outDir": "./dist"
  },
  "size": 191
}"#
        );
    }
}
