//! tsgo `ls/lsutil/userpreferences.go`: the preference types the schema
//! names, tsgo's defaults, the parsing of the client's configuration
//! sections, and the helpers the generated parsing uses.

use serde_json::{Map, Value};
use tsc_checker::ModuleSpecifierUserPreferences;

use super::format_code_options::get_default_format_code_settings;
use super::UserPreferences;

/// tsgo `QuotePreference`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct QuotePreference(pub &'static str);

impl QuotePreference {
    pub const UNKNOWN: Self = Self("");
    pub const AUTO: Self = Self("auto");
    pub const DOUBLE: Self = Self("double");
    pub const SINGLE: Self = Self("single");
}

/// tsgo `WorkspaceSymbolsScope`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct WorkspaceSymbolsScope(pub &'static str);

impl WorkspaceSymbolsScope {
    pub const ALL_OPEN_PROJECTS: Self = Self("allOpenProjects");
    pub const CURRENT_PROJECT: Self = Self("currentProject");
}

/// tsgo `JsxAttributeCompletionStyle`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct JsxAttributeCompletionStyle(pub &'static str);

impl JsxAttributeCompletionStyle {
    pub const UNKNOWN: Self = Self("");
    pub const AUTO: Self = Self("auto");
    pub const BRACES: Self = Self("braces");
    pub const NONE: Self = Self("none");
}

/// tsgo `IncludeInlayParameterNameHints` (`none` is the empty value).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct IncludeInlayParameterNameHints(pub &'static str);

impl IncludeInlayParameterNameHints {
    pub const NONE: Self = Self("");
    pub const ALL: Self = Self("all");
    pub const LITERALS: Self = Self("literals");
}

/// tsgo `OrganizeImportsSort`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OrganizeImportsSort(pub i64);

impl OrganizeImportsSort {
    pub const AUTO: Self = Self(0);
    pub const ORDINAL: Self = Self(1);
    pub const ORDINAL_IGNORE_CASE: Self = Self(2);
    pub const NATURAL: Self = Self(3);
    pub const NATURAL_IGNORE_CASE: Self = Self(4);
}

/// tsgo `OrganizeImportsCollation`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OrganizeImportsCollation(pub bool);

impl OrganizeImportsCollation {
    pub const ORDINAL: Self = Self(false);
    pub const UNICODE: Self = Self(true);
}

/// tsgo `OrganizeImportsCaseFirst`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OrganizeImportsCaseFirst(pub i64);

impl OrganizeImportsCaseFirst {
    pub const DEFAULT: Self = Self(0);
    pub const LOWER: Self = Self(1);
    pub const UPPER: Self = Self(2);
}

/// tsgo `OrganizeImportsTypeOrder`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct OrganizeImportsTypeOrder(pub i64);

impl OrganizeImportsTypeOrder {
    pub const AUTO: Self = Self(0);
    pub const LAST: Self = Self(1);
    pub const INLINE: Self = Self(2);
    pub const FIRST: Self = Self(3);
}

/// tsgo `parsePreferenceTristate`: a boolean, else unknown.
pub(super) fn parse_preference_tristate(value: &Value) -> Option<bool> {
    value.as_bool()
}

/// tsgo `parseOrganizeImportsCaseSensitivity`: `caseInsensitive` and
/// `caseSensitive` in any case, else a boolean.
pub(super) fn parse_organize_imports_case_sensitivity(value: &Value) -> Option<bool> {
    if let Some(text) = value.as_str() {
        match text.to_lowercase().as_str() {
            "caseinsensitive" => return Some(true),
            "casesensitive" => return Some(false),
            _ => {}
        }
    }
    parse_preference_tristate(value)
}

/// tsgo `getPreferenceConfigValue`: the value at the first configuration
/// path present (even null), negated for a path tagged `,invert`.
pub(super) fn get_preference_config_value(
    config: &Map<String, Value>,
    paths: &[&str],
) -> Option<Value> {
    for tag in paths {
        let (path, modifier) = tag.split_once(',').unwrap_or((tag, ""));
        if let Some(value) = get_nested_value(config, path) {
            if modifier == "invert" {
                if let Some(value) = value.as_bool() {
                    return Some(Value::Bool(!value));
                }
            }
            return Some(value.clone());
        }
    }
    None
}

/// tsgo `getNestedValue`: the value at a `.`-separated path of objects.
fn get_nested_value<'a>(config: &'a Map<String, Value>, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut current = config.get(parts.next()?)?;
    for part in parts {
        current = current.as_object()?.get(part)?;
    }
    Some(current)
}

/// tsgo `setNestedValue`: the value at a `.`-separated path, making (or
/// replacing a non-object with) an object at each step.
pub(super) fn set_nested_value(config: &mut Map<String, Value>, path: &str, value: Value) {
    let mut parts = path.split('.').collect::<Vec<_>>();
    let last = parts.pop().unwrap_or_default();
    let mut current = config;
    for part in parts {
        let entry = current
            .entry(part.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        current = entry.as_object_mut().expect("an object was just ensured");
    }
    current.insert(last.to_owned(), value);
}

/// Go's deterministic JSON object order: every object's keys sorted.
pub(super) fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sort_keys(value)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

/// tsgo `normalizeCustomConfigFileName`: a file name without directories.
pub(super) fn normalize_custom_config_file_name(name: &str) -> String {
    let name = name.trim();
    if name.contains(['/', '\\']) || name == ".." || name == "." {
        return String::new();
    }
    name.to_owned()
}

impl UserPreferences {
    /// tsgo `NewDefaultUserPreferences`.
    pub fn new_default() -> Self {
        let mut preferences = Self {
            format_code_settings: get_default_format_code_settings(),
            ..Self::default()
        };
        preferences.auto_closing_tags = Some(true);
        preferences.complete_js_docs = Some(true);
        preferences.generate_return_in_doc_template = Some(true);
        preferences.allow_rename_of_import_path = Some(true);
        preferences.provide_refactor_not_applicable_reason = Some(true);
        preferences.format_enabled = Some(true);
        preferences.validate_enabled = Some(true);
        preferences.display_parts_for_js_doc = Some(true);
        preferences.disable_line_text_in_references = Some(true);
        preferences.report_style_checks_as_warnings = Some(true);
        preferences.exclude_library_symbols_in_nav_to = Some(true);
        preferences.workspace_symbols_scope = WorkspaceSymbolsScope::ALL_OPEN_PROJECTS;
        preferences
            .completion_preferences
            .include_completions_for_module_exports = Some(true);
        preferences
            .completion_preferences
            .include_completions_for_import_statements = Some(true);
        preferences
    }

    /// tsgo `UnmarshalJSONFrom`: tsgo's defaults with a JSON object's
    /// configuration (null is none).
    pub fn from_json(value: &Value) -> Result<Self, String> {
        match value {
            Value::Object(config) => Ok(Self::new_default().with_config(config)),
            Value::Null => Ok(Self::new_default()),
            other => Err(format!(
                "json: cannot unmarshal JSON {} into Go map[string]interface {{}}",
                json_kind(other)
            )),
        }
    }

    /// tsgo `IsATADisabled`: the unified setting, else the deprecated one.
    pub fn is_ata_disabled(&self) -> bool {
        match self.automatic_type_acquisition_enabled {
            Some(enabled) => !enabled,
            None => self.disable_automatic_type_acquisition == Some(true),
        }
    }

    /// tsgo `ModuleSpecifierPreferences`.
    pub fn module_specifier_preferences(&self) -> ModuleSpecifierUserPreferences {
        ModuleSpecifierUserPreferences {
            import_module_specifier_preference: self.import_module_specifier_preference,
            import_module_specifier_ending: self.import_module_specifier_ending,
            auto_import_specifier_exclude_regexes: self
                .auto_import_specifier_exclude_regexes
                .clone()
                .unwrap_or_default(),
        }
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// tsgo `ParseUserPreferences`: the client's `editor` settings as raw
/// names (`tabSize` also giving `indentSize`, `insertSpaces`
/// `convertTabsToSpaces`), then `javascript`, `typescript` and `js/ts`, each
/// taking precedence over the ones before.
pub fn parse_user_preferences(items: &Map<String, Value>) -> UserPreferences {
    let mut preferences = UserPreferences::new_default();
    if let Some(Value::Object(editor)) = items.get("editor") {
        let mut settings = editor.clone();
        if let Some(tab_size) = settings.get("tabSize").cloned() {
            settings.entry("indentSize").or_insert(tab_size);
        }
        if let Some(insert_spaces) = settings.get("insertSpaces").cloned() {
            settings
                .entry("convertTabsToSpaces")
                .or_insert(insert_spaces);
        }
        let mut config = Map::new();
        config.insert("unstable".to_owned(), Value::Object(settings));
        preferences = preferences.with_config(&config);
    }
    for section in ["javascript", "typescript", "js/ts"] {
        if let Some(Value::Object(settings)) = items.get(section) {
            preferences = preferences.with_config(settings);
        }
    }
    preferences
}

#[cfg(test)]
#[path = "../../tests/unit/user_preferences.rs"]
mod tests;
