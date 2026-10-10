//! tsgo `api/proto.go` (19dadef8): the parameters and responses of the
//! methods ported so far, in tsgo's JSON form. Parameters decode as Go's
//! `json.Unmarshal` does: unknown keys are ignored and `null` leaves a field
//! at its zero value. Responses keep tsgo's field order and its omitted
//! fields: `encoding/json/v2`'s `omitempty` leaves out a value that encodes
//! as `null`, `""`, `{}` or `[]` (not `false`), and `omitzero` a zero value.

use std::collections::BTreeMap;

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, JsStr, JsString, MessageChain, RelatedInfo};
use tsc_program::go_json::compiler_options_bag;
use tsc_program::ConfigOptionBag;

/// tsgo `SnapshotID`.
pub type SnapshotId = u64;

/// A field's value, or its zero value when the JSON sets it to `null`.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// tsgo `DocumentIdentifier`: a file name (a plain string) or `{ uri }`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DocumentIdentifier {
    pub file_name: String,
    pub uri: String,
}

impl<'de> Deserialize<'de> for DocumentIdentifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DocumentVisitor;

        impl<'de> Visitor<'de> for DocumentVisitor {
            type Value = DocumentIdentifier;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string or an object")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(DocumentIdentifier {
                    file_name: value.to_owned(),
                    uri: String::new(),
                })
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut document = DocumentIdentifier::default();
                while let Some(key) = map.next_key::<String>()? {
                    let value = map.next_value::<serde_json::Value>()?;
                    if key == "uri" {
                        if let serde_json::Value::String(uri) = value {
                            document.uri = uri;
                        }
                    }
                }
                Ok(document)
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Err(Self::kind_error(if value { "true" } else { "false" }))
            }

            fn visit_i64<E: de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Err(Self::kind_error("number"))
            }

            fn visit_u64<E: de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Err(Self::kind_error("number"))
            }

            fn visit_f64<E: de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Err(Self::kind_error("number"))
            }

            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Err(Self::kind_error("null"))
            }

            fn visit_seq<A: de::SeqAccess<'de>>(self, _: A) -> Result<Self::Value, A::Error> {
                Err(Self::kind_error("["))
            }
        }

        impl DocumentVisitor {
            fn kind_error<E: de::Error>(kind: &str) -> E {
                E::custom(format!(
                    "DocumentIdentifier: expected string or object, got {kind}"
                ))
            }
        }

        deserializer.deserialize_any(DocumentVisitor)
    }
}

impl DocumentIdentifier {
    /// tsgo `ToAbsoluteFileName`: the URI's file name, or the file name made
    /// absolute against `current_directory` and normalized.
    pub fn to_absolute_file_name(&self, current_directory: &str) -> Result<String, String> {
        if self.uri.is_empty() {
            Ok(tsc_program::get_normalized_absolute_path(
                JsStr::from_str(&self.file_name),
                JsStr::from_str(current_directory),
            )
            .to_string_lossy()
            .into_owned())
        } else {
            uri_to_file_name(&self.uri)
        }
    }
}

/// tsgo `bundled.IsBundled`: a file of the embedded standard library.
fn is_bundled(name: &str) -> bool {
    name.starts_with("bundled:///")
}

/// tsgo `DocumentUri.FileName` (lsp/lsproto/lsp.go): a `file:` URI's path,
/// percent-decoded (`//host/path` for one with a host, without the slash
/// before a drive letter), and any other URI as `^/scheme/authority/path`
/// with its escapes kept.
pub fn uri_to_file_name(uri: &str) -> Result<String, String> {
    if is_bundled(uri) {
        return Ok(uri.to_owned());
    }
    if let Some(rest) = uri.strip_prefix("file://") {
        // url.Parse: the authority up to the path, the path up to a query
        // or fragment.
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let (host, path) = match rest.find('/') {
            Some(index) => rest.split_at(index),
            None => (rest, ""),
        };
        let invalid = || format!("invalid file URI: {uri}");
        let host = percent_decode(host).ok_or_else(invalid)?;
        let path = percent_decode(path).ok_or_else(invalid)?;
        if !host.is_empty() {
            return Ok(format!("//{host}{path}"));
        }
        // fixWindowsURIPath.
        if let Some(rest) = path.strip_prefix('/') {
            let bytes = rest.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                return Ok(rest.to_owned());
            }
        }
        return Ok(path);
    }
    let Some((scheme, path)) = uri.split_once(':') else {
        return Err(format!("invalid URI: {uri}"));
    };
    let (authority, path) = match path.strip_prefix("//") {
        Some(rest) => rest
            .split_once('/')
            .ok_or_else(|| format!("invalid URI: {uri}"))?,
        None => ("ts-nul-authority", path),
    };
    Ok(format!("^/{scheme}/{authority}/{path}"))
}

/// The bytes `%XX` escapes stand for, as UTF-8; nothing for a malformed
/// escape or text.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

/// tsgo `FileNotifications`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNotifications {
    #[serde(default, deserialize_with = "nullable")]
    pub invalidate_all: bool,
    #[serde(default, deserialize_with = "nullable")]
    pub changed: Vec<DocumentIdentifier>,
    #[serde(default, deserialize_with = "nullable")]
    pub created: Vec<DocumentIdentifier>,
    #[serde(default, deserialize_with = "nullable")]
    pub deleted: Vec<DocumentIdentifier>,
}

/// tsgo `EnsurePrograms`: `true` for every project, or project IDs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnsurePrograms {
    All,
    Projects(Vec<String>),
}

impl<'de> Deserialize<'de> for EnsurePrograms {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match serde_json::Value::deserialize(deserializer)? {
            serde_json::Value::Bool(true) => Ok(Self::All),
            value @ serde_json::Value::Array(_) => serde_json::from_value(value)
                .map(Self::Projects)
                .map_err(de::Error::custom),
            _ => Err(de::Error::custom(
                "ensurePrograms must be true or an array of project IDs",
            )),
        }
    }
}

/// tsgo `project.SyntheticProjectID`: validated (and canonicalized) as it
/// is decoded.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SyntheticProjectIdParam(pub String);

impl<'de> Deserialize<'de> for SyntheticProjectIdParam {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        tsc_project::ProjectId::parse_synthetic(&value)
            .map(|id| Self(id.to_string()))
            .ok_or_else(|| de::Error::custom(format!("invalid synthetic project ID: {value}")))
    }
}

/// tsgo `core.CompilerOptions` as the API receives them.
#[derive(Clone, Debug, Default)]
pub struct CompilerOptionsParam(pub ConfigOptionBag);

impl<'de> Deserialize<'de> for CompilerOptionsParam {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match Option::<serde_json::Map<String, serde_json::Value>>::deserialize(deserializer)? {
            Some(options) => compiler_options_bag(&options)
                .map(Self)
                .map_err(de::Error::custom),
            None => Ok(Self::default()),
        }
    }
}

/// tsgo `core.ProjectReference`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectReference {
    #[serde(default, deserialize_with = "nullable")]
    pub path: String,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub original_path: String,
    /// `omitempty`, which keeps `false`.
    #[serde(default, deserialize_with = "nullable")]
    pub circular: bool,
}

/// tsgo `CreateProgramOptions`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateProgramOptions {
    #[serde(default, deserialize_with = "nullable")]
    pub project_references: Vec<ProjectReference>,
    #[serde(default, deserialize_with = "nullable")]
    pub config_file_parsing_diagnostics: Vec<DiagnosticResponse>,
    #[serde(default, deserialize_with = "nullable")]
    pub module_resolver: u64,
}

/// tsgo `CreateSnapshotProgramParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSnapshotProgramParams {
    #[serde(default, deserialize_with = "nullable")]
    pub root_files: Vec<DocumentIdentifier>,
    #[serde(default)]
    pub compiler_options: CompilerOptionsParam,
    #[serde(default)]
    pub options: Option<CreateProgramOptions>,
}

/// tsgo `ReconfigureSnapshotProgramParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconfigureSnapshotProgramParams {
    #[serde(default, deserialize_with = "nullable")]
    pub id: SyntheticProjectIdParam,
    #[serde(default, deserialize_with = "nullable")]
    pub root_files: Vec<DocumentIdentifier>,
    #[serde(default)]
    pub compiler_options: CompilerOptionsParam,
    #[serde(default)]
    pub options: Option<CreateProgramOptions>,
}

/// tsgo `SnapshotRequestChangesParams`. A list that is absent or `null` is
/// none (Go's nil slice), which some responses tell from an empty one.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotRequestChanges {
    #[serde(default, deserialize_with = "nullable")]
    pub open_projects: Vec<DocumentIdentifier>,
    #[serde(default, deserialize_with = "nullable")]
    pub close_projects: Vec<DocumentIdentifier>,
    #[serde(default)]
    pub open_files: Option<Vec<DocumentIdentifier>>,
    #[serde(default, deserialize_with = "nullable")]
    pub close_files: Vec<DocumentIdentifier>,
    #[serde(default)]
    pub create_programs: Option<Vec<Option<CreateSnapshotProgramParams>>>,
    #[serde(default, deserialize_with = "nullable")]
    pub reconfigure_programs: Vec<Option<ReconfigureSnapshotProgramParams>>,
    #[serde(default, deserialize_with = "nullable")]
    pub remove_programs: Vec<SyntheticProjectIdParam>,
    #[serde(default)]
    pub ensure_programs: Option<EnsurePrograms>,
}

/// tsgo `CreateSnapshotParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSnapshotParams {
    #[serde(flatten)]
    pub changes: SnapshotRequestChanges,
    #[serde(default)]
    pub file_notifications: Option<FileNotifications>,
    /// tsgo's request file system (`requestfilesystem`).
    #[serde(default)]
    pub file_system: Option<crate::request_fs::RequestFileSystemParams>,
}

/// tsgo `UpdateSnapshotParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSnapshotParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default)]
    pub changes: Option<CreateSnapshotParams>,
}

/// tsgo `ReleaseParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReleaseParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
}

/// tsgo `GetDefaultProjectForFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GetDefaultProjectForFileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default)]
    pub file: DocumentIdentifier,
}

/// tsgo `BatchRequest`.
#[derive(Clone, Debug, Deserialize)]
pub struct BatchRequest {
    #[serde(default, deserialize_with = "nullable")]
    pub method: String,
    #[serde(default)]
    pub params: Option<Box<RawValue>>,
}

/// tsgo `BatchRequestsParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRequestsParams {
    #[serde(default, deserialize_with = "nullable")]
    pub requests: Vec<BatchRequest>,
    #[serde(default, deserialize_with = "nullable")]
    pub continuation_token: String,
    #[serde(default, deserialize_with = "nullable")]
    pub max_response_bytes_per_page: i64,
}

/// tsgo `InitializeResponse`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResponse {
    pub use_case_sensitive_file_names: bool,
    pub current_directory: String,
}

/// tsgo `ProjectFileChanges`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileChanges {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub deleted_files: Vec<String>,
}

/// tsgo `SnapshotChanges`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotChanges {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub changed_projects: BTreeMap<String, ProjectFileChanges>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed_projects: Vec<String>,
}

impl SnapshotChanges {
    pub fn is_empty(&self) -> bool {
        self.changed_projects.is_empty() && self.removed_projects.is_empty()
    }
}

/// `omitempty` of changes that encode as `{}`.
fn no_changes(changes: &Option<SnapshotChanges>) -> bool {
    changes.as_ref().is_none_or(SnapshotChanges::is_empty)
}

/// `omitempty` of a JSON value: none, or `null`, `""`, `{}` or `[]`.
fn empty_json(value: &Option<Box<RawValue>>) -> bool {
    value
        .as_ref()
        .is_none_or(|value| matches!(value.get(), "null" | r#""""# | "{}" | "[]"))
}

/// tsgo `OpenedFileOperationResult`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct OpenedFileOperationResult {
    pub project: String,
}

/// tsgo `SnapshotOperationResponse`.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOperationResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_programs: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_files: Option<Vec<OpenedFileOperationResult>>,
}

/// tsgo `CreateSnapshotResponse`.
#[derive(Clone, Debug, Serialize)]
pub struct CreateSnapshotResponse {
    pub snapshot: SnapshotId,
    pub projects: Vec<ProjectResponse>,
    #[serde(skip_serializing_if = "no_changes")]
    pub changes: Option<SnapshotChanges>,
    pub operation: SnapshotOperationResponse,
}

/// tsgo `ConfigFileResponse`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFileResponse {
    pub file_names: Vec<String>,
    pub options: Box<RawValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub project_references: Vec<ProjectReference>,
    #[serde(skip_serializing_if = "empty_json")]
    pub type_acquisition: Option<Box<RawValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compile_on_save: Option<bool>,
    #[serde(skip_serializing_if = "empty_json")]
    pub raw: Option<Box<RawValue>>,
    pub errors: Vec<DiagnosticResponse>,
}

/// tsgo `ProjectResponse`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResponse {
    pub id: String,
    pub config_file_name: String,
    pub current_directory: String,
    pub dirty: bool,
    pub parsed_command_line: ConfigFileResponse,
    pub root_files: Vec<String>,
    pub compiler_options: Box<RawValue>,
}

/// tsgo `DiagnosticPositionResponse`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticPositionResponse {
    pub line: usize,
    pub character: usize,
}

/// tsgo `DiagnosticSourceLineResponse`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiagnosticSourceLineResponse {
    pub line: usize,
    pub text: String,
}

/// tsgo `DiagnosticResponse`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticResponse {
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub file_name: String,
    /// -1 for a diagnostic without a location (tsgo's undefined range).
    #[serde(default, deserialize_with = "nullable")]
    pub pos: i64,
    #[serde(default, deserialize_with = "nullable")]
    pub end: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_position: Option<DiagnosticPositionResponse>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_position: Option<DiagnosticPositionResponse>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub source_lines: Vec<DiagnosticSourceLineResponse>,
    #[serde(default, deserialize_with = "nullable")]
    pub code: u32,
    #[serde(default, deserialize_with = "nullable")]
    pub category: u8,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "String::is_empty"
    )]
    pub source: String,
    #[serde(default, deserialize_with = "nullable")]
    pub text: String,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub reports_unnecessary: bool,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub reports_deprecated: bool,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub message_chain: Vec<DiagnosticResponse>,
    #[serde(
        default,
        deserialize_with = "nullable",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub related_information: Vec<DiagnosticResponse>,
}

/// tsgo's `diagnostics.Category` numbers.
fn category_number(category: DiagnosticCategory) -> u8 {
    match category {
        DiagnosticCategory::Warning => 0,
        DiagnosticCategory::Error => 1,
        DiagnosticCategory::Suggestion => 2,
        DiagnosticCategory::Message => 3,
    }
}

fn category_of_number(number: u8) -> DiagnosticCategory {
    match number {
        0 => DiagnosticCategory::Warning,
        2 => DiagnosticCategory::Suggestion,
        3 => DiagnosticCategory::Message,
        _ => DiagnosticCategory::Error,
    }
}

/// A file's text, measured as tsgo measures a diagnostic's file: UTF-16
/// positions, ECMAScript line terminators.
struct DiagnosticFile<'a> {
    text: &'a str,
    /// Each line's start, in UTF-8 bytes and in UTF-16 units.
    lines: Vec<(usize, usize)>,
    utf16_length: usize,
}

impl<'a> DiagnosticFile<'a> {
    fn new(text: &'a str) -> Self {
        let mut lines = vec![(0, 0)];
        let mut utf16 = 0;
        let mut characters = text.char_indices().peekable();
        while let Some((index, character)) = characters.next() {
            utf16 += character.len_utf16();
            let line_break = match character {
                '\r' => {
                    if let Some(&(_, '\n')) = characters.peek() {
                        characters.next();
                        utf16 += 1;
                        Some(index + 2)
                    } else {
                        Some(index + 1)
                    }
                }
                '\n' | '\u{2028}' | '\u{2029}' => Some(index + character.len_utf8()),
                _ => None,
            };
            if let Some(start) = line_break {
                lines.push((start, utf16));
            }
        }
        Self {
            text,
            lines,
            utf16_length: utf16,
        }
    }

    /// tsgo `GetECMALineAndUTF16CharacterOfPosition`.
    fn position(&self, utf16: usize) -> DiagnosticPositionResponse {
        let line = self
            .lines
            .partition_point(|&(_, start)| start <= utf16)
            .saturating_sub(1);
        DiagnosticPositionResponse {
            line,
            character: utf16 - self.lines[line].1,
        }
    }

    /// tsgo `diagnosticSourceLines`: the lines from `first` to `last`, only
    /// the first two and last two of five or more.
    fn source_lines(&self, first: usize, last: usize) -> Vec<DiagnosticSourceLineResponse> {
        let lines = if last - first >= 4 {
            vec![first, first + 1, last - 1, last]
        } else {
            (first..=last).collect()
        };
        lines
            .into_iter()
            .map(|line| {
                let start = self.lines[line].0;
                let end = self
                    .lines
                    .get(line + 1)
                    .map_or(self.text.len(), |next| next.0);
                DiagnosticSourceLineResponse {
                    line,
                    text: self.text[start..end].to_owned(),
                }
            })
            .collect()
    }
}

impl DiagnosticResponse {
    /// tsgo `NewDiagnosticResponse`: the diagnostic, located in its file's
    /// text when `text_of` has it. A message chain's entries take their
    /// diagnostic's location, as tsgo's chained diagnostics do.
    pub fn new(diagnostic: &Diagnostic, text_of: &dyn Fn(&str) -> Option<String>) -> Self {
        Self::located(
            diagnostic.file_name.as_ref(),
            diagnostic.start,
            diagnostic.length,
            &diagnostic.message,
            &diagnostic.related,
            diagnostic.reports_unnecessary.unwrap_or(false),
            diagnostic.reports_deprecated.unwrap_or(false),
            diagnostic.source.as_deref().unwrap_or_default(),
            text_of,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn located(
        file_name: Option<&JsString>,
        start: Option<u32>,
        length: Option<u32>,
        message: &MessageChain,
        related: &[RelatedInfo],
        reports_unnecessary: bool,
        reports_deprecated: bool,
        source: &str,
        text_of: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        let file_name = file_name.map(|name| name.to_string_lossy().into_owned());
        let (pos, end) = match start {
            Some(start) => (
                i64::from(start),
                i64::from(start) + i64::from(length.unwrap_or(0)),
            ),
            None => (-1, -1),
        };
        let mut response = Self {
            pos,
            end,
            code: message.code,
            category: category_number(message.category),
            source: source.to_owned(),
            text: message.text.to_string_lossy().into_owned(),
            reports_unnecessary,
            reports_deprecated,
            ..Self::default()
        };
        if let Some(file_name) = &file_name {
            response.file_name.clone_from(file_name);
            if let Some(text) = text_of(file_name).filter(|_| start.is_some()) {
                let file = DiagnosticFile::new(&text);
                let pos = (pos as usize).min(file.utf16_length);
                let end = (end as usize).min(file.utf16_length).max(pos);
                response.pos = pos as i64;
                response.end = end as i64;
                let start_position = file.position(pos);
                let end_position = file.position(end);
                response.source_lines = file.source_lines(start_position.line, end_position.line);
                response.start_position = Some(start_position);
                response.end_position = Some(end_position);
            }
        }
        let chain_file = file_name.as_deref().map(JsString::from);
        response.message_chain = message
            .next
            .iter()
            .map(|next| {
                Self::located(
                    chain_file.as_ref(),
                    start,
                    length,
                    next,
                    &[],
                    false,
                    false,
                    "",
                    text_of,
                )
            })
            .collect();
        response.related_information = related
            .iter()
            .map(|related| {
                Self::located(
                    related.file_name.as_ref(),
                    related.start,
                    related.length,
                    &related.message,
                    &[],
                    false,
                    false,
                    "",
                    text_of,
                )
            })
            .collect();
        response
    }

    /// tsgo `ToDiagnostic`: a diagnostic without a file, at the response's
    /// range.
    pub fn to_diagnostic(&self) -> Diagnostic {
        let mut diagnostic = Diagnostic::new(
            None,
            u32::try_from(self.pos).ok(),
            u32::try_from(self.pos)
                .ok()
                .map(|_| self.end.saturating_sub(self.pos) as u32),
            self.to_message_chain(),
        );
        diagnostic.related = self
            .related_information
            .iter()
            .map(|related| RelatedInfo {
                file_name: None,
                start: u32::try_from(related.pos).ok(),
                length: u32::try_from(related.pos)
                    .ok()
                    .map(|_| related.end.saturating_sub(related.pos) as u32),
                message: related.to_message_chain(),
            })
            .collect();
        diagnostic.related_information_present = !diagnostic.related.is_empty();
        diagnostic.reports_unnecessary = self.reports_unnecessary.then_some(true);
        diagnostic.reports_deprecated = self.reports_deprecated.then_some(true);
        diagnostic
    }

    fn to_message_chain(&self) -> MessageChain {
        MessageChain {
            code: self.code,
            category: category_of_number(self.category),
            text: self.text.as_str().into(),
            key: None,
            args: Vec::new(),
            next_present: !self.message_chain.is_empty(),
            next: self
                .message_chain
                .iter()
                .map(Self::to_message_chain)
                .collect(),
            related: Vec::new(),
            repopulate: None,
        }
    }
}

/// tsgo `ParseCommandLineParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseCommandLineParams {
    #[serde(default, deserialize_with = "nullable")]
    pub command_line: Vec<String>,
}

/// tsgo `ReadConfigFileParams` and `ParseConfigFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ConfigFileParams {
    #[serde(default)]
    pub file: DocumentIdentifier,
}

/// tsgo `ReadConfigFileResponse`.
#[derive(Clone, Debug, Serialize)]
pub struct ReadConfigFileResponse {
    pub config: Box<RawValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DiagnosticResponse>,
}

/// tsgo `ParseJsonConfigFileContentParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseJsonConfigFileContentParams {
    #[serde(default)]
    pub json: serde_json::Value,
    #[serde(default)]
    pub config_directory: Option<String>,
    #[serde(default)]
    pub config_file_name: Option<DocumentIdentifier>,
}

/// tsgo `CreateSourceFileOptions`.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceFileOptions {
    /// tsgo `core.ScriptKind`; 0 (unknown) takes the file name's.
    #[serde(default, deserialize_with = "nullable")]
    pub script_kind: u32,
}

/// tsgo `CreateSourceFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceFileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub file_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub source_text: String,
    #[serde(default, deserialize_with = "nullable")]
    pub options: CreateSourceFileOptions,
}

/// tsgo `CreateSourceFileFromFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceFileFromFileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub file_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub options: CreateSourceFileOptions,
}

/// tsgo `ReleaseSourceFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReleaseSourceFileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub lease: u64,
}

/// tsgo `TranspileOptions`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranspileOptions {
    #[serde(default)]
    pub compiler_options: CompilerOptionsParam,
    #[serde(default, deserialize_with = "nullable")]
    pub file_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub report_diagnostics: bool,
}

/// tsgo `TranspileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TranspileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub input: String,
    #[serde(default, deserialize_with = "nullable")]
    pub options: TranspileOptions,
}

/// tsgo `TranspileFromFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranspileFromFileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub file_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub options: TranspileOptions,
}

/// tsgo `TranspileOutputResponse`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranspileOutputResponse {
    pub output_text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<DiagnosticResponse>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_map_text: String,
}

/// The parameters of tsgo's checker requests (`GetSymbolAtPositionParams`,
/// `GetTypeOfSymbolParams`, `GetSymbolPropertyParams`,
/// `TypeToTypeNodeParams`, …): each method reads the fields its tsgo
/// parameters have.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckerParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub file: DocumentIdentifier,
    #[serde(default, deserialize_with = "nullable")]
    pub files: Vec<DocumentIdentifier>,
    #[serde(default, deserialize_with = "nullable")]
    pub position: u32,
    #[serde(default, deserialize_with = "nullable")]
    pub positions: Vec<u32>,
    #[serde(default, deserialize_with = "nullable")]
    pub location: String,
    #[serde(default, deserialize_with = "nullable")]
    pub locations: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub symbol: u64,
    #[serde(default, deserialize_with = "nullable")]
    pub symbols: Vec<u64>,
    #[serde(default, rename = "type", deserialize_with = "nullable")]
    pub type_id: u32,
    /// A symbol, type or signature, by the property requests.
    #[serde(default, deserialize_with = "nullable")]
    pub object_id: u64,
    #[serde(default, deserialize_with = "nullable")]
    pub name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub flags: i32,
}

/// tsgo `GetSourceFileParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GetSourceFileParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub file: DocumentIdentifier,
}

/// tsgo `GetSourceFileNamesParams` and `GetProjectDiagnosticsParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ProjectParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
}

/// tsgo `GetModeForUsageLocationParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GetModeForUsageLocationParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub file: DocumentIdentifier,
    #[serde(default, deserialize_with = "nullable")]
    pub usage: String,
}

/// tsgo `GetModeForResolutionAtIndexParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct GetModeForResolutionAtIndexParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub file: DocumentIdentifier,
    #[serde(default, deserialize_with = "nullable")]
    pub index: i64,
}

/// tsgo `GetResolvedModuleParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResolvedModuleParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub file: DocumentIdentifier,
    #[serde(default, deserialize_with = "nullable")]
    pub module_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub mode: u32,
}

/// tsgo `GetResolvedModuleFromModuleSpecifierParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResolvedModuleFromModuleSpecifierParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default, deserialize_with = "nullable")]
    pub module_specifier: String,
    #[serde(default)]
    pub source_file: Option<DocumentIdentifier>,
}

/// tsgo `GetResolvedTypeReferenceDirectiveParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResolvedTypeReferenceDirectiveParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub file: DocumentIdentifier,
    #[serde(default, deserialize_with = "nullable")]
    pub type_directive_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub mode: u32,
}

/// tsgo `GetResolvedTypeReferenceDirectiveFromReferenceParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetResolvedTypeReferenceDirectiveFromReferenceParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub project: String,
    #[serde(default)]
    pub source_file: DocumentIdentifier,
    #[serde(default, deserialize_with = "nullable")]
    pub type_directive_name: String,
    #[serde(default, deserialize_with = "nullable")]
    pub resolution_mode: u32,
}

/// tsgo `SourceFileMetadata`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFileMetadata {
    pub is_default_library: bool,
    pub is_from_external_library: bool,
    pub package_json_type: String,
    pub package_json_directory: String,
    pub implied_node_format: u32,
}

/// tsgo `PackageId`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageIdResponse {
    pub name: String,
    pub sub_module_name: String,
    pub version: String,
    pub peer_dependencies: String,
}

/// tsgo `ResolvedModule`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedModuleResponse {
    pub resolved_file_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub original_path: String,
    pub extension: String,
    pub resolved_using_ts_extension: bool,
    pub resolved_using_extra_extensions: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_id: Option<PackageIdResponse>,
    pub is_external_library_import: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub alternate_result: String,
}

/// tsgo `ResolvedTypeReferenceDirective`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTypeReferenceDirectiveResponse {
    pub primary: bool,
    pub resolved_file_name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub original_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package_id: Option<PackageIdResponse>,
    pub is_external_library_import: bool,
}

/// tsgo `CreateModuleResolverParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateModuleResolverParams {
    #[serde(default)]
    pub compiler_options: CompilerOptionsParam,
    #[serde(default)]
    pub module_resolutions: Option<crate::module_resolution::ModuleResolutionSpec>,
    #[serde(default, deserialize_with = "nullable")]
    pub resolve_module_name_callback: String,
}

/// tsgo `ReleaseModuleResolverParams`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ReleaseModuleResolverParams {
    #[serde(default, deserialize_with = "nullable")]
    pub resolver: u64,
}

/// tsgo `ResolveModuleNameParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveModuleNameParams {
    #[serde(default, deserialize_with = "nullable")]
    pub snapshot: SnapshotId,
    #[serde(default, deserialize_with = "nullable")]
    pub in_progress_snapshot: u64,
    #[serde(default, deserialize_with = "nullable")]
    pub resolver: u64,
    #[serde(default, deserialize_with = "nullable")]
    pub module_name: String,
    #[serde(default)]
    pub containing_directory: DocumentIdentifier,
    #[serde(default)]
    pub resolution_mode: Option<i64>,
}

/// tsgo `ResolveModuleNameResult`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveModuleNameResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_module: Option<ResolvedModuleResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub trace: Vec<String>,
}

/// tsgo `SourceFileResponse`: a binary source file's bytes as base64 in a
/// batch.
#[derive(Clone, Debug, Serialize)]
pub struct SourceFileResponse {
    pub data: String,
}

#[cfg(test)]
#[path = "../tests/unit/proto.rs"]
mod tests;
