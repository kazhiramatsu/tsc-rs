//! TypeScript config-file root planning.
//!
//! This module owns the production boundary immediately before
//! [`crate::load_program`]. It deliberately keeps the compiler runner's
//! virtual filesystem adapter outside `tsc_program`, while the config source,
//! `extends` graph, four effective discovery-option values, path normalization,
//! and root-name selection remain program-owned.
//!
//! H0.5 now retains TypeScript's partial plan for the focused malformed-config
//! contract: primary parse diagnostics, ordered config errors, recoverable
//! `extends` branches, validated root specs, and the absent/undefined/value
//! compiler-option distinction. Compiler-option values also remain available
//! as a source-order-preserving raw merge. List options retain their converted
//! element values, including JavaScript `undefined` slots where TypeScript
//! deliberately preserves them. The `paths` object option additionally keeps
//! recursive JavaScript own-property order and `undefined` identity plus its
//! outer object/array shape as the canonical typed representation. Its six
//! option diagnostics are produced after final substitution with root-syntax
//! locations, and the effective map plus declaring base is projected into an
//! immutable resolver snapshot. Remaining root schemas and `ParsedCommandLine`
//! fields are later slices.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::js_path::{
    combine_paths, directory_name, file_name_key, normalize_slashes, normalized_absolute_path,
    normalized_config_dir_value_path, normalized_config_value_path, relative_path_from_directory,
    root_parts, starts_with_config_dir_template,
};
use crate::json_value::{JsonObject as Map, JsonValue as Value};
use tsc_diagnostics::{
    gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticArgument, DiagnosticMessage,
    DocumentVersion, JsStr, JsString, MessageChain, TextSnapshot,
};
use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation};
use tsc_syntax::{NodeId, SourceFile, SyntaxKind};
use tsc_types::{js_number_to_string, CompilerOptionNumber, CompilerOptions, ModuleSuffix};

use crate::config_options::{
    compiler_option_declaration, compiler_option_spelling_suggestion,
    is_command_option_without_build, jsconfig_defaults, libraries, named_value_in,
    CompilerOptionListDescriptor, CompilerOptionListElementKind, CompilerOptionValueKind,
    JsConfigDefaultValue,
};
use crate::json::{
    config_parser_preflight, convert_recoverable_json_node_to_value,
    convert_recoverable_json_source_file_to_value, decode_user_object_key, json_number_as_f64,
    json_object_get, json_object_own_get, json_source_file_is_empty, JsonParserPreflight,
    RecoverableJsonValue,
};
use crate::library::LibraryCatalog;
use crate::loader::{
    load_emitting_program_with_root_reasons, load_program_with_root_reasons, ProgramLoadError,
    ProgramLoadLimits, RootFileReason,
};
use crate::module_resolution::{normalize_absolute_js_path_lexical, ModuleResolver};
use crate::option_validation::{
    has_zero_or_one_asterisk, path_is_absolute, path_is_relative, validate_compiler_options,
    validate_paths_option_diagnostics, CompilerOptionValidationLocation, CompilerOptionViolation,
};
use crate::path::ProgramPath;
use crate::prepared::{
    PathMapping, PathsOptionDiagnosticLocation, PathsOptionValidationPlan, PathsOptionViolation,
    PathsOptionViolationKind, PreparedAuxiliaryFile, PreparedProgram, ProgramConfigFile,
    ProgramConfigSpan, ProgramOptions,
};
use crate::resolution::{ResolutionError, ResolutionOutcome};
use crate::ConfigFilePattern;

const TYPESCRIPT_EXTENSIONS: &[&[&str]] = &[
    &[".ts", ".tsx", ".d.ts"],
    &[".cts", ".d.cts"],
    &[".mts", ".d.mts"],
];
const ALL_EXTENSIONS: &[&[&str]] = &[
    &[".ts", ".tsx", ".d.ts", ".js", ".jsx"],
    &[".cts", ".d.cts", ".cjs"],
    &[".mts", ".d.mts", ".mjs"],
];
// Keep the recursive merge worker below Rust's smaller test-thread stacks.
// A future general config graph planner can replace this with an iterative
// postorder walk without changing the public resource-limit failure kind.
const MAX_CONFIG_EXTENDS_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigHostOperation {
    FileExists,
    ReadFile,
    ReadDirectory,
    Realpath,
}

impl fmt::Display for ConfigHostOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::FileExists => "fileExists",
            Self::ReadFile => "readFile",
            Self::ReadDirectory => "readDirectory",
            Self::Realpath => "realpath",
        };
        formatter.write_str(name)
    }
}

/// A typed failure from the exact host observation requested by config
/// parsing. Absence remains `Ok(false)`/`Ok(None)` and is never represented by
/// this error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigHostError {
    operation: ConfigHostOperation,
    path: JsString,
    detail: String,
}

impl ConfigHostError {
    pub fn new(
        operation: ConfigHostOperation,
        path: impl Into<JsString>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            operation,
            path: path.into(),
            detail: detail.into(),
        }
    }

    pub const fn operation(&self) -> ConfigHostOperation {
        self.operation
    }

    pub fn path(&self) -> JsStr<'_> {
        self.path.as_js()
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for ConfigHostError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "config host {} failed for {:?}: {}",
            self.operation, self.path, self.detail
        )
    }
}

impl Error for ConfigHostError {}

/// The TypeScript `ParseConfigHost` observations needed by config planning.
///
/// `read_directory` has the shape of TypeScript's filtered recursive callback,
/// not a raw operating-system listing. Implementors own its filtering and
/// `matchFiles` semantics. The production [`crate::CompilerConfigHost`]
/// supplies that contract for both filesystem and memory hosts; specialized
/// fixture hosts may intentionally expose a narrower files-only surface.
pub trait ConfigParseHost {
    fn use_case_sensitive_file_names(&self) -> bool;

    fn file_exists(&self, path: JsStr<'_>) -> Result<bool, ConfigHostError>;

    fn read_file(&self, path: JsStr<'_>) -> Result<Option<String>, ConfigHostError>;

    fn read_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError>;

    /// The real path of a file (tsgo `FS().Realpath`): a config found in
    /// `node_modules` resolves to where it really is. `None` keeps the path.
    fn realpath(&self, _path: JsStr<'_>) -> Result<Option<JsString>, ConfigHostError> {
        Ok(None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigParseErrorKind {
    InvalidPath,
    Host,
    Syntax,
    InvalidConfig,
    Unsupported,
    CircularExtends,
    MissingExtends,
    ResourceLimit,
}

/// Infrastructure or unsupported-surface failure which prevents even a
/// partial plan. Ordinary config syntax, option, spec, missing-extends, and
/// circularity errors live on [`ConfigRootPlan`] instead.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigParseError {
    kind: ConfigParseErrorKind,
    path: Option<JsString>,
    detail: String,
    diagnostics: Vec<Diagnostic>,
    host_error: Option<Box<ConfigHostError>>,
}

impl ConfigParseError {
    pub(crate) fn new_js(
        kind: ConfigParseErrorKind,
        path: Option<JsString>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path,
            detail: detail.into(),
            diagnostics: Vec::new(),
            host_error: None,
        }
    }

    pub const fn kind(&self) -> ConfigParseErrorKind {
        self.kind
    }

    pub fn path(&self) -> Option<JsStr<'_>> {
        self.path.as_ref().map(JsString::as_js)
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn host_error(&self) -> Option<&ConfigHostError> {
        self.host_error.as_deref()
    }
}

impl fmt::Display for ConfigParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(
                formatter,
                "config planning failed for {path:?}: {}",
                self.detail
            )
        } else {
            write!(formatter, "config planning failed: {}", self.detail)
        }
    }
}

impl Error for ConfigParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.host_error
            .as_ref()
            .map(|error| error.as_ref() as &(dyn Error + 'static))
    }
}

impl From<ConfigHostError> for ConfigParseError {
    fn from(error: ConfigHostError) -> Self {
        Self {
            kind: ConfigParseErrorKind::Host,
            path: Some(error.path.clone()),
            detail: error.to_string(),
            diagnostics: Vec::new(),
            host_error: Some(Box::new(error)),
        }
    }
}

/// An owned source participating in the primary/extended config graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigSourceText {
    pub file_name: JsString,
    snapshot: Arc<TextSnapshot>,
}

impl ConfigSourceText {
    pub fn new(file_name: impl Into<JsString>, text: impl Into<String>) -> Self {
        Self {
            file_name: file_name.into(),
            snapshot: TextSnapshot::new(text.into(), DocumentVersion::default()),
        }
    }

    pub fn text(&self) -> &str {
        self.snapshot.text()
    }

    pub fn snapshot(&self) -> &Arc<TextSnapshot> {
        &self.snapshot
    }
}

/// One merged compiler-option property with its defining config directory.
/// The origin is required for inherited path-valued options such as `paths`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigOption {
    pub name: JsString,
    pub value: Value,
    pub base_path: JsString,
}

/// Source-order-preserving merge of raw compiler-option property values plus
/// a separate converted three-state projection.
///
/// TypeScript config keys are case-sensitive. This root-planning slice retains
/// every property spelling; replacement never moves the first insertion. This
/// is neither source text nor a complete `CompilerOptions`. Use
/// [`Self::typed_value_state`] when converted `undefined` must be observable.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigOptionBag {
    entries: Vec<ConfigOption>,
    entry_indices: BTreeMap<JsString, usize>,
    typed_entries: Vec<ConfigTypedOption>,
    typed_indices: BTreeMap<String, usize>,
    raw_order: Vec<JsString>,
    raw_indices: BTreeMap<JsString, usize>,
    removed_names: BTreeSet<JsString>,
}

#[derive(Clone, Debug, PartialEq)]
struct ConfigTypedOption {
    name: String,
    value: Option<ConfigTypedOptionValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ConfigTypedOptionValue {
    Json(Value),
    List(Vec<ConfigTypedListElement>),
    Object(Arc<ConfigTypedObjectValue>),
    PositiveInfinity,
    NegativeInfinity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigTypedObjectShape {
    Object,
    Array,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConfigTypedObjectProperty {
    name: JsString,
    value: Option<ConfigTypedJsonValue>,
}

impl ConfigTypedObjectProperty {
    pub fn name(&self) -> JsStr<'_> {
        self.name.as_js()
    }

    /// Converted own-property value. `None` means the property exists with a
    /// JavaScript `undefined` value; it is not an absent mapping key.
    pub fn value(&self) -> Option<&ConfigTypedJsonValue> {
        self.value.as_ref()
    }
}

/// Lossless converted JSONC value used below object-like compiler options.
/// Arrays have already filtered JavaScript `undefined` elements; objects keep
/// it as an own-property state instead of collapsing it into JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigTypedJsonValue {
    Json(Value),
    Array(Vec<ConfigTypedJsonValue>),
    Object(Box<ConfigTypedObjectValue>),
}

impl ConfigTypedJsonValue {
    pub fn json_projection(&self) -> Value {
        match self {
            Self::Json(Value::Number(number)) => {
                let number = json_number_as_f64(number)
                    .expect("config JSON numbers have a JavaScript numeric projection");
                if number.is_finite() {
                    serde_json::from_str::<serde_json::Value>(&js_number_to_string(number))
                        .expect("a finite JavaScript number string is valid JSON")
                        .into()
                } else {
                    // JSON.stringify emits null for Infinity and -Infinity.
                    Value::Null
                }
            }
            Self::Json(value) => value.clone(),
            Self::Array(values) => Value::Array(values.iter().map(Self::json_projection).collect()),
            Self::Object(value) => value.json_projection(),
        }
    }

    fn inherited_proto_setter(&self) -> Option<bool> {
        match self {
            Self::Json(Value::Null) => Some(false),
            Self::Array(_) => Some(true),
            Self::Object(value) => Some(value.inherits_proto_setter),
            Self::Json(Value::Bool(_) | Value::Number(_) | Value::String(_)) => None,
            Self::Json(Value::Array(_) | Value::Object(_)) => {
                unreachable!("structured typed JSON values use dedicated variants")
            }
        }
    }

    fn append_compiler_option_cache_identity(&self, result: &mut JsString) {
        match self {
            Self::Json(Value::Null) => result.push_str("null"),
            Self::Json(Value::Bool(value)) => {
                result.push_str(if *value { "true" } else { "false" });
            }
            Self::Json(Value::Number(value)) => {
                let value = json_number_as_f64(value)
                    .expect("config JSON numbers have a JavaScript numeric projection");
                result.push_str(&js_number_to_string(value));
            }
            Self::Json(Value::String(value)) => result.push_js(value.as_js()),
            Self::Array(values) => {
                result.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        result.push(',');
                    }
                    value.append_compiler_option_cache_identity(result);
                }
                result.push(']');
            }
            Self::Object(value) => value.append_compiler_option_cache_identity(result),
            Self::Json(Value::Array(_) | Value::Object(_)) => {
                unreachable!("structured typed JSON values use dedicated variants")
            }
        }
    }
}

/// JavaScript object-like compiler option value.
///
/// A property assigned an unsupported JSONC expression remains an own key
/// whose value is JavaScript `undefined`, including in nested objects used by
/// TypeScript's compiler-option cache identity. Keeping that state outside
/// serde JSON prevents invalid configurations from aliasing an empty object.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigTypedObjectValue {
    shape: ConfigTypedObjectShape,
    properties: Vec<ConfigTypedObjectProperty>,
    inherits_proto_setter: bool,
}

impl ConfigTypedObjectValue {
    /// The properties in their source order: tsgo keeps an object value in
    /// an ordered map, where a numeric key has no place of its own.
    fn new(
        shape: ConfigTypedObjectShape,
        properties: Vec<ConfigTypedObjectProperty>,
        inherits_proto_setter: bool,
    ) -> Self {
        Self {
            shape,
            properties,
            inherits_proto_setter,
        }
    }

    pub const fn shape(&self) -> ConfigTypedObjectShape {
        self.shape
    }

    pub fn properties(&self) -> &[ConfigTypedObjectProperty] {
        &self.properties
    }

    /// Build the ordinary JSON observation of this JavaScript object-like
    /// value. Own `undefined` properties are omitted. The compiler keeps the
    /// lossless property representation above and allocates this projection
    /// only at serialization/oracle boundaries.
    pub fn json_projection(&self) -> Value {
        match self.shape {
            ConfigTypedObjectShape::Object => {
                let mut object = Map::new();
                for property in &self.properties {
                    if let Some(value) = &property.value {
                        object.insert(property.name.clone(), value.json_projection());
                    }
                }
                Value::Object(object)
            }
            ConfigTypedObjectShape::Array => Value::Array(
                self.properties
                    .iter()
                    .map(|property| {
                        property
                            .value
                            .as_ref()
                            .expect("converted JSON arrays filter undefined elements")
                            .json_projection()
                    })
                    .collect(),
            ),
        }
    }

    /// TypeScript's recursive string identity for module-resolution-affecting
    /// compiler options. Unlike a JSON projection, this preserves own
    /// `undefined` and therefore keeps invalid nested maps from aliasing an
    /// empty object in redirect caches.
    ///
    /// tsc-port: compilerOptionValueToString @6.0.3
    /// tsc-hash: 47e7644c9afbf6ce03d7ce0591d09b74dff44bc1538ef08c02b4eb698a8f58a5
    /// tsc-span: _tsc.js:40327-40341
    pub fn compiler_option_cache_identity(&self) -> JsString {
        let mut result = JsString::new();
        self.append_compiler_option_cache_identity(&mut result);
        result
    }

    fn append_compiler_option_cache_identity(&self, result: &mut JsString) {
        match self.shape {
            ConfigTypedObjectShape::Array => {
                result.push('[');
                for (index, property) in self.properties.iter().enumerate() {
                    if index != 0 {
                        result.push(',');
                    }
                    property
                        .value
                        .as_ref()
                        .expect("converted JSON arrays filter undefined elements")
                        .append_compiler_option_cache_identity(result);
                }
                result.push(']');
            }
            ConfigTypedObjectShape::Object => {
                result.push('{');
                for property in &self.properties {
                    result.push_js(property.name.as_js());
                    result.push_str(": ");
                    if let Some(value) = &property.value {
                        value.append_compiler_option_cache_identity(result);
                    } else {
                        result.push_str("undefined");
                    }
                }
                result.push('}');
            }
        }
    }

    fn finalize_config_dir_templates<'j0>(
        &mut self,
        config_base_path: impl Into<JsStr<'j0>>,
    ) -> Result<(), ConfigParseError> {
        let config_base_path = config_base_path.into();
        let mut changed = false;
        for property in &mut self.properties {
            let Some(ConfigTypedJsonValue::Array(values)) = &mut property.value else {
                continue;
            };
            changed |= substitute_config_dir_typed_string_array(values, config_base_path)?;
        }
        if changed {
            // TypeScript clones every changed map-like value with assign({},
            // value). This turns an Array into an object and routes an own
            // `__proto__` key through the fresh target's legacy setter rather
            // than creating an own property.
            if self.shape == ConfigTypedObjectShape::Array {
                self.shape = ConfigTypedObjectShape::Object;
            }
            if let Some(index) = self
                .properties
                .iter()
                .position(|property| property.name == "__proto__")
            {
                self.inherits_proto_setter = self.properties[index]
                    .value
                    .as_ref()
                    .and_then(ConfigTypedJsonValue::inherited_proto_setter)
                    .unwrap_or(true);
                self.properties.remove(index);
            } else {
                self.inherits_proto_setter = true;
            }
        }
        Ok(())
    }
}

/// One converted `compilerOptions` list element.
///
/// TypeScript normally filters falsy converted list elements, but
/// `moduleSuffixes` opts into `listPreserveFalsyValues` and therefore retains
/// JavaScript `undefined` entries produced by null or invalid source values.
/// Keeping that state distinct from JSON `null` is required by module
/// resolution and by the public config-plan observation boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigTypedListElement {
    Value(Value),
    Undefined,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConfigOptionValueState<'a> {
    Absent,
    Undefined,
    Value(&'a Value),
    List(&'a [ConfigTypedListElement]),
    Object(&'a ConfigTypedObjectValue),
    PositiveInfinity,
    NegativeInfinity,
}

impl ConfigOptionBag {
    pub fn entries(&self) -> &[ConfigOption] {
        &self.entries
    }

    pub fn get<'n>(&self, name: impl Into<JsStr<'n>>) -> Option<&ConfigOption> {
        self.entry_indices
            .get(name.into().as_bytes())
            .map(|index| &self.entries[*index])
    }

    /// Stored `pathsBasePath` compiler-option value. It can remain inherited
    /// when an own null or invalid `paths` masks the effective map, so this is
    /// deliberately not TypeScript's `getPathsBasePath` result. It is absent
    /// from the raw [`Self::entries`] and [`Self::get`] views.
    pub fn stored_paths_base_path(&self) -> Option<JsStr<'_>> {
        self.typed_value("pathsBasePath").and_then(Value::as_js)
    }

    pub fn typed_object_value(&self, name: &str) -> Option<&ConfigTypedObjectValue> {
        let index = self.typed_indices.get(name)?;
        match &self.typed_entries[*index].value {
            Some(ConfigTypedOptionValue::Object(value)) => Some(value),
            Some(
                ConfigTypedOptionValue::Json(_)
                | ConfigTypedOptionValue::List(_)
                | ConfigTypedOptionValue::PositiveInfinity
                | ConfigTypedOptionValue::NegativeInfinity,
            )
            | None => None,
        }
    }

    /// Ordered own-property view for an object-like compiler option: the
    /// keys in their first insertion slots, including own `undefined` values.
    pub fn typed_object_properties(&self, name: &str) -> Option<&[ConfigTypedObjectProperty]> {
        self.typed_object_value(name)
            .map(ConfigTypedObjectValue::properties)
    }

    /// The converted TypeScript option state, distinct from the raw config
    /// property retained by [`Self::get`]. An invalid own value is
    /// `Undefined` and therefore masks an inherited value; unknown spellings
    /// remain `Absent`.
    pub fn typed_value_state(&self, name: &str) -> ConfigOptionValueState<'_> {
        match self
            .typed_indices
            .get(name)
            .map(|index| &self.typed_entries[*index])
        {
            Some(ConfigTypedOption {
                value: Some(ConfigTypedOptionValue::Json(value)),
                ..
            }) => ConfigOptionValueState::Value(value),
            Some(ConfigTypedOption {
                value: Some(ConfigTypedOptionValue::List(elements)),
                ..
            }) => ConfigOptionValueState::List(elements),
            Some(ConfigTypedOption {
                value: Some(ConfigTypedOptionValue::Object(value)),
                ..
            }) => ConfigOptionValueState::Object(value),
            Some(ConfigTypedOption {
                value: Some(ConfigTypedOptionValue::PositiveInfinity),
                ..
            }) => ConfigOptionValueState::PositiveInfinity,
            Some(ConfigTypedOption {
                value: Some(ConfigTypedOptionValue::NegativeInfinity),
                ..
            }) => ConfigOptionValueState::NegativeInfinity,
            Some(_) => ConfigOptionValueState::Undefined,
            None => ConfigOptionValueState::Absent,
        }
    }

    fn typed_value(&self, name: &str) -> Option<&Value> {
        match self
            .typed_indices
            .get(name)
            .map(|index| &self.typed_entries[*index].value)
        {
            Some(Some(ConfigTypedOptionValue::Json(value))) => Some(value),
            Some(None)
            | Some(Some(
                ConfigTypedOptionValue::List(_)
                | ConfigTypedOptionValue::Object(_)
                | ConfigTypedOptionValue::PositiveInfinity
                | ConfigTypedOptionValue::NegativeInfinity,
            ))
            | None => None,
        }
    }

    pub(crate) fn insert(&mut self, option: ConfigOption) {
        self.observe_raw_name(&option.name);
        self.removed_names.remove(&option.name);
        if let Some(index) = self.entry_indices.get(&option.name).copied() {
            self.entries[index] = option;
        } else {
            let index = self.entries.len();
            let name = option.name.clone();
            self.entries.push(option);
            self.entry_indices.insert(name, index);
        }
    }

    pub(crate) fn remove<'n>(&mut self, name: impl Into<JsStr<'n>>) {
        let name = name.into();
        self.observe_raw_name(name);
        if let Some(index) = self.entry_indices.remove(name.as_bytes()) {
            self.entries.swap_remove(index);
            if let Some(moved) = self.entries.get(index) {
                self.entry_indices.insert(moved.name.clone(), index);
            }
        }
        self.removed_names.insert(name.to_owned());
    }

    fn observe_raw_name<'n>(&mut self, name: impl Into<JsStr<'n>>) {
        let name = name.into();
        if self.raw_indices.contains_key(name.as_bytes()) {
            return;
        }
        let index = self.raw_order.len();
        let name = name.to_owned();
        self.raw_order.push(name.clone());
        self.raw_indices.insert(name, index);
    }

    pub(crate) fn insert_typed(
        &mut self,
        name: impl Into<String>,
        value: Option<ConfigTypedOptionValue>,
    ) {
        let name = name.into();
        if let Some(index) = self.typed_indices.get(&name).copied() {
            self.typed_entries[index].value = value;
        } else {
            let index = self.typed_entries.len();
            self.typed_entries.push(ConfigTypedOption {
                name: name.clone(),
                value,
            });
            self.typed_indices.insert(name, index);
        }
    }

    pub(crate) fn option_bool(&self, name: &str) -> Option<bool> {
        config_option_bool(self, name)
    }

    pub(crate) fn option_string_list(&self, name: &str) -> Option<Vec<JsString>> {
        config_option_string_list(self, name)
    }

    /// tsgo `mergeCompilerOptions`: the other bag's values replace these
    /// (its explicit `null`s remove the option).
    pub(crate) fn extend_from(&mut self, other: &Self) {
        for name in &other.raw_order {
            if other.removed_names.contains(name) {
                self.remove(name);
            } else if let Some(index) = other.entry_indices.get(name) {
                self.insert(other.entries[*index].clone());
            }
        }
        for option in &other.typed_entries {
            self.insert_typed(option.name.clone(), option.value.clone());
        }
    }

    /// Apply TypeScript's final `${configDir}` substitution pass after every
    /// extended config has been merged. File-path strings, the two file-path
    /// lists, and array-valued entries in `paths` all use the outermost
    /// consuming config directory. Ordinary relative values retain their
    /// declaration-time conversion and are not revisited here.
    ///
    /// tsc-port: handleOptionConfigDirTemplateSubstitution @6.0.3
    /// tsc-hash: b8be2c1ed12416218b6fb0619c12276cf3d395da3853ae7931ed6caafd1e2ca6
    /// tsc-span: _tsc.js:39175-39207
    /// tsc-port: getSubstitutedMapLikeOfStringArrayWithConfigDirTemplate @6.0.3
    /// tsc-hash: 0d887c86b4808b665c81b73f083409d2818f9bc41612c64254fa1aa817fa3e97
    /// tsc-span: _tsc.js:39229-39239
    fn finalize_config_dir_templates<'j0>(
        &mut self,
        config_base_path: impl Into<JsStr<'j0>>,
    ) -> Result<(), ConfigParseError> {
        let config_base_path = config_base_path.into();
        self.finalize_group_config_dir_templates(config_base_path, ConfigOptionGroup::Compiler)
    }

    fn finalize_group_config_dir_templates<'j0>(
        &mut self,
        config_base_path: impl Into<JsStr<'j0>>,
        group: ConfigOptionGroup,
    ) -> Result<(), ConfigParseError> {
        let config_base_path = config_base_path.into();
        for option in &mut self.typed_entries {
            let Some(declaration) = group.declaration(&option.name) else {
                continue;
            };
            match (declaration.value_kind(), &mut option.value) {
                (
                    CompilerOptionValueKind::String,
                    Some(ConfigTypedOptionValue::Json(Value::String(value))),
                ) if declaration.is_file_path() => {
                    substitute_config_dir_string(value, config_base_path)?;
                }
                (
                    CompilerOptionValueKind::List(descriptor),
                    Some(ConfigTypedOptionValue::List(elements)),
                ) if descriptor.allow_config_dir_template_substitution() => {
                    for element in elements {
                        let ConfigTypedListElement::Value(Value::String(value)) = element else {
                            continue;
                        };
                        substitute_config_dir_string(value, config_base_path)?;
                    }
                }
                (
                    CompilerOptionValueKind::Object(descriptor),
                    Some(ConfigTypedOptionValue::Object(value)),
                ) if descriptor.allow_config_dir_template_substitution() => {
                    Arc::make_mut(value).finalize_config_dir_templates(config_base_path)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Mutation keeps active entries densely packed so removals do not shift
    /// every later option. Restore JavaScript's first-property insertion order
    /// once, immediately before the root plan becomes public.
    fn restore_public_entry_order(&mut self) {
        let raw_indices = &self.raw_indices;
        self.entries.sort_by_cached_key(|entry| {
            raw_indices
                .get(&entry.name)
                .copied()
                .expect("every active option has an observed raw slot")
        });
        self.entry_indices.clear();
        self.entry_indices.extend(
            self.entries
                .iter()
                .enumerate()
                .map(|(index, entry)| (entry.name.clone(), index)),
        );
    }
}

/// Typed compiler-option projection which can affect config root discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigDiscoveryOptions {
    allow_js: bool,
    resolve_json_module: bool,
    out_dir: Option<JsString>,
    declaration_dir: Option<JsString>,
}

/// Immutable config projection consumed by [`ModuleResolver`].
///
/// This is deliberately narrower than a complete `ParsedCommandLine`: it
/// carries the resolver-facing compiler/program subset modeled by this slice,
/// including an atomic `paths`/`pathsBasePath` pair suitable for sharing across
/// independent resolver workers. Other converted options are not implicitly
/// claimed by this boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigModuleResolutionOptions {
    compiler_options: CompilerOptions,
    program_options: ProgramOptions,
}

impl ConfigModuleResolutionOptions {
    pub const fn compiler_options(&self) -> &CompilerOptions {
        &self.compiler_options
    }

    pub const fn program_options(&self) -> &ProgramOptions {
        &self.program_options
    }
}

impl ConfigDiscoveryOptions {
    pub const fn allow_js(&self) -> bool {
        self.allow_js
    }

    pub const fn resolve_json_module(&self) -> bool {
        self.resolve_json_module
    }

    pub fn out_dir(&self) -> Option<JsStr<'_>> {
        self.out_dir.as_ref().map(JsString::as_js)
    }

    pub fn declaration_dir(&self) -> Option<JsStr<'_>> {
        self.declaration_dir.as_ref().map(JsString::as_js)
    }
}

#[derive(Clone, Debug)]
pub struct ConfigRootPlanRequest {
    pub file_name: JsString,
    pub text: String,
    pub base_path: JsString,
}

/// One normalized `ParsedCommandLine.projectReferences` entry.  The H0
/// loader still rejects non-empty project references at execution time, but
/// parsing must retain the same primary-config observation for embeddings and
/// diagnostics that inspect a partial command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigProjectReference {
    pub path: JsString,
    pub original_path: JsString,
    pub prepend: Option<bool>,
    pub circular: Option<bool>,
}

/// One `ParsedCommandLine.wildcardDirectories` entry.  TypeScript encodes the
/// flag as `Recursive=1` or `None=0`; a bool keeps that boundary explicit and
/// avoids exposing the internal watcher enum to the no-emit loader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigWildcardDirectory {
    pub path: JsString,
    pub recursive: bool,
}

/// Program-owned root-planning projection, not a complete `ParsedCommandLine`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigRootPlan {
    config_file_name: JsString,
    source: ConfigSourceText,
    extended_sources: Vec<ConfigSourceText>,
    raw: Value,
    options: ConfigOptionBag,
    discovery_options: ConfigDiscoveryOptions,
    module_resolution_options: ConfigModuleResolutionOptions,
    /// Effective root specs after the extends merge. These remain separate
    /// from `file_names`: TypeScript exposes both the declarative
    /// `ParsedCommandLine` lists and the discovered file-name projection.
    files: Option<Vec<JsString>>,
    include: Option<Vec<JsString>>,
    exclude: Option<Vec<JsString>>,
    /// Root-level `references` are observable on the primary config only;
    /// TypeScript does not inherit them through `extends`.
    references: Option<Value>,
    project_references: Option<Vec<ConfigProjectReference>>,
    /// Converted root schema projections. Watch options merge through
    /// extends; type acquisition keeps this config's own defaults. The H0
    /// loader separately retains its explicit root-scope validation.
    watch_options: Option<Value>,
    type_acquisition: Option<Value>,
    watch_option_bag: Option<ConfigOptionBag>,
    type_acquisition_option_bag: ConfigOptionBag,
    compile_on_save: Option<Value>,
    /// Truthy root-level schemas which the single-project no-emit loader does
    /// not consume. Keep this separate from `raw`: `raw` is intentionally a
    /// projection of the primary config and therefore cannot, by itself,
    /// distinguish a value inherited from an `extends` source.
    unsupported_root_scopes: BTreeSet<String>,
    file_names: Vec<JsString>,
    /// tsgo `configFileSpecs.validatedIncludeSpecs` and
    /// `validatedExcludeSpecs`: the specs the file names were matched with.
    include_specs: Vec<JsString>,
    exclude_specs: Option<Vec<JsString>>,
    root_reasons: Vec<RootFileReason>,
    wildcard_directories: Vec<ConfigWildcardDirectory>,
    root_parse_diagnostics: Vec<Diagnostic>,
    errors: Vec<Diagnostic>,
    option_diagnostics: Vec<Diagnostic>,
    extended_source_files: Vec<JsString>,
}

impl ConfigRootPlan {
    pub fn config_file_name(&self) -> JsStr<'_> {
        self.config_file_name.as_js()
    }

    pub fn source(&self) -> &ConfigSourceText {
        &self.source
    }

    pub fn extended_sources(&self) -> &[ConfigSourceText] {
        &self.extended_sources
    }

    pub fn raw(&self) -> &Value {
        &self.raw
    }

    pub fn options(&self) -> &ConfigOptionBag {
        &self.options
    }

    pub const fn discovery_options(&self) -> &ConfigDiscoveryOptions {
        &self.discovery_options
    }

    pub const fn module_resolution_options(&self) -> &ConfigModuleResolutionOptions {
        &self.module_resolution_options
    }

    /// Effective `files` entries after extends rebasing. `None` preserves an
    /// absent/undefined property, while `Some([])` is an explicit empty list.
    pub fn files(&self) -> Option<&[JsString]> {
        self.files.as_deref()
    }

    /// Effective `include` entries after extends rebasing.
    pub fn include(&self) -> Option<&[JsString]> {
        self.include.as_deref()
    }

    /// Effective `exclude` entries after extends rebasing.
    pub fn exclude(&self) -> Option<&[JsString]> {
        self.exclude.as_deref()
    }

    /// The primary config's raw `references` value. Project references are
    /// deliberately not inherited by TypeScript's config merge.
    pub fn references(&self) -> Option<&Value> {
        self.references.as_ref()
    }

    /// Normalized project-reference entries for the primary config.  This is
    /// observation-only; the H0 single-project loader rejects non-empty
    /// references before source loading.
    pub fn project_references(&self) -> Option<&[ConfigProjectReference]> {
        self.project_references.as_deref()
    }

    /// JSON projection of converted, merged watch options. Undefined values
    /// are omitted; use `watch_option_bag` to preserve their presence.
    pub fn watch_options(&self) -> Option<&Value> {
        self.watch_options.as_ref()
    }

    /// JSON projection of converted type acquisition options, including
    /// tsconfig/jsconfig defaults. These options do not inherit from extends.
    pub fn type_acquisition(&self) -> Option<&Value> {
        self.type_acquisition.as_ref()
    }

    pub fn watch_option_bag(&self) -> Option<&ConfigOptionBag> {
        self.watch_option_bag.as_ref()
    }

    pub fn type_acquisition_option_bag(&self) -> &ConfigOptionBag {
        &self.type_acquisition_option_bag
    }

    /// ParsedCommandLine.compileOnSave uses the raw value's truthiness.
    pub fn compile_on_save_enabled(&self) -> bool {
        self.compile_on_save
            .as_ref()
            .is_some_and(json_value_is_truthy)
    }

    /// Effective raw `compileOnSave` after `extends` merging.
    pub fn compile_on_save(&self) -> Option<&Value> {
        self.compile_on_save.as_ref()
    }

    /// Root-level config scopes retained for the fail-closed program gate.
    /// These may originate in an `extends` source and therefore are not
    /// recoverable from the primary `raw` projection alone.
    pub fn unsupported_root_scopes(&self) -> impl Iterator<Item = &str> {
        self.unsupported_root_scopes.iter().map(String::as_str)
    }

    /// The checker-facing compiler options projected from the merged config.
    ///
    /// This is intentionally a borrowed view of the immutable plan. Callers
    /// that need a filesystem program should use [`load_config_program`],
    /// which preserves the config diagnostic gate and the mandatory H0
    /// `noEmit` boundary before invoking the recursive loader.
    pub const fn compiler_options(&self) -> &CompilerOptions {
        self.module_resolution_options.compiler_options()
    }

    /// The host/program options projected from the merged config.
    pub const fn program_options(&self) -> &ProgramOptions {
        self.module_resolution_options.program_options()
    }

    /// The plan of a `tsc -b` project (tsgo merges `Build` into each
    /// project's options).
    pub fn into_build_mode(mut self) -> Self {
        let options = std::mem::take(&mut self.module_resolution_options.program_options);
        self.module_resolution_options.program_options = options.with_build_mode(true);
        self
    }

    /// tsgo `validatedIncludeSpecs`: the valid include specs (`**/*` when
    /// neither `files` nor `include` is given) with `${configDir}`
    /// substituted.
    pub fn include_specs(&self) -> &[JsString] {
        &self.include_specs
    }

    /// tsgo `validatedExcludeSpecs`: the valid exclude specs with
    /// `${configDir}` substituted; without an `exclude` property, the
    /// output and declaration directories.
    pub fn exclude_specs(&self) -> Option<&[JsString]> {
        self.exclude_specs.as_deref()
    }

    pub fn file_names(&self) -> &[JsString] {
        &self.file_names
    }

    /// Directory watcher roots derived from the effective include/exclude
    /// specs, in TypeScript's stable insertion order.
    pub fn wildcard_directories(&self) -> &[ConfigWildcardDirectory] {
        &self.wildcard_directories
    }

    /// Parse diagnostics owned by the primary config source. TypeScript keeps
    /// these outside `ParsedCommandLine.errors` and prepends them only at the
    /// compiler-facing config-diagnostic boundary.
    pub fn root_parse_diagnostics(&self) -> &[Diagnostic] {
        &self.root_parse_diagnostics
    }

    /// Ordered config-content diagnostics (`ParsedCommandLine.errors`).
    pub fn errors(&self) -> &[Diagnostic] {
        &self.errors
    }

    /// Program option diagnostics produced after config conversion and the
    /// final `${configDir}` substitution pass. TypeScript keeps these out of
    /// `ParsedCommandLine.errors` and exposes them through
    /// `Program.getOptionsDiagnostics()`.
    pub fn option_diagnostics(&self) -> &[Diagnostic] {
        &self.option_diagnostics
    }

    /// Compiler-visible config diagnostics: primary parse diagnostics first,
    /// followed by the parsed-command-line errors.
    pub fn diagnostics(&self) -> impl Iterator<Item = &Diagnostic> {
        self.root_parse_diagnostics.iter().chain(self.errors.iter())
    }

    /// tsgo `ReloadFileNamesOfParsedCommandLine` (a watch run's build): this
    /// plan with the root files `reloaded` matched again; the errors stay
    /// this plan's, so a watch whose last root went away reports no TS18003.
    pub fn with_reloaded_file_names(mut self, reloaded: ConfigRootPlan) -> Self {
        self.file_names = reloaded.file_names;
        self.root_reasons = reloaded.root_reasons;
        self
    }

    /// TypeScript's identity-only `extendedSourceFiles` projection. Unlike
    /// `extended_sources`, this also represents an explicitly resolved config
    /// whose read failed and therefore has no source text.
    pub fn extended_source_files(&self) -> &[JsString] {
        &self.extended_source_files
    }
}

/// A config plan cannot be turned into a prepared no-emit program when the
/// config itself has diagnostics, when a fatal option diagnostic is present,
/// when `noEmit` is absent/false, or when the filesystem loader rejects a
/// typed host/resolution boundary. TypeScript 6.0 deprecation rows (5101 and
/// 5107) are reportable but do not stop program construction. Keeping these
/// cases distinct lets a CLI render those rows while treating the latter
/// failures as fail-closed driver outcomes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigProgramLoadError {
    Diagnostics {
        config: Vec<Diagnostic>,
        options: Vec<Diagnostic>,
    },
    NoEmitRequired {
        value: Option<bool>,
    },
    EmitRequired {
        value: Option<bool>,
    },
    Program(ProgramLoadError),
}

impl ConfigProgramLoadError {
    pub fn config_diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Diagnostics { config, .. } => config,
            Self::NoEmitRequired { .. } | Self::EmitRequired { .. } | Self::Program(_) => &[],
        }
    }

    pub fn options_diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Diagnostics { options, .. } => options,
            Self::NoEmitRequired { .. } | Self::EmitRequired { .. } | Self::Program(_) => &[],
        }
    }

    pub const fn program_error(&self) -> Option<&ProgramLoadError> {
        match self {
            Self::Program(error) => Some(error),
            Self::Diagnostics { .. } | Self::NoEmitRequired { .. } | Self::EmitRequired { .. } => {
                None
            }
        }
    }
}

impl fmt::Display for ConfigProgramLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Diagnostics { config, options } => write!(
                formatter,
                "config plan has {} config and {} option diagnostic(s)",
                config.len(),
                options.len()
            ),
            Self::NoEmitRequired { value } => write!(
                formatter,
                "compilerOptions.noEmit must be explicitly true (observed {value:?})"
            ),
            Self::EmitRequired { value } => write!(
                formatter,
                "emitting config requires compilerOptions.noEmit to be absent or false (observed {value:?})"
            ),
            Self::Program(error) => error.fmt(formatter),
        }
    }
}

impl Error for ConfigProgramLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Program(error) => Some(error),
            Self::Diagnostics { .. } | Self::NoEmitRequired { .. } | Self::EmitRequired { .. } => {
                None
            }
        }
    }
}

/// Turn a parsed config/root plan into the owned no-emit program consumed by
/// [`tsc_compiler::ProgramSession`].
///
/// Config diagnostics and fatal option diagnostics are a gate: no source host
/// work is started while either collection is non-empty. TypeScript 6.0
/// deprecation diagnostics are retained on the plan but do not block loading.
/// A config without an explicit
/// `noEmit: true` is rejected before `load_program`; this prevents an omitted
/// or false value from accidentally entering an emitter-capable path. The
/// input plan remains immutable and can be reused by a caller for rendering or
/// for an independent MemoryHost/FsHost comparison.
pub fn load_config_program(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    load_config_program_inner(
        host,
        plan,
        library_catalog,
        limits,
        ConfigProgramMode::NoEmit { force: false },
    )
}

/// The no-emit program of a config whatever its `noEmit` (tsgo's --noEmit
/// command, --listFilesOnly).
pub fn load_config_program_with_no_emit_override(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    load_config_program_inner(
        host,
        plan,
        library_catalog,
        limits,
        ConfigProgramMode::NoEmit { force: true },
    )
}

pub fn load_emitting_config_program(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    load_config_program_inner(
        host,
        plan,
        library_catalog,
        limits,
        ConfigProgramMode::Emit { force: false },
    )
}

/// The emitting program of a config whatever its `noEmit` (`--noEmit false`).
pub fn load_emitting_config_program_with_no_emit_override(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    load_config_program_inner(
        host,
        plan,
        library_catalog,
        limits,
        ConfigProgramMode::Emit { force: true },
    )
}

pub fn validate_config_plan(plan: &ConfigRootPlan) -> Result<(), ConfigProgramLoadError> {
    let config = plan.diagnostics().cloned().collect::<Vec<_>>();
    let options = plan
        .option_diagnostics()
        .iter()
        .filter(|diagnostic| !is_non_fatal_option_diagnostic(diagnostic))
        .cloned()
        .collect::<Vec<_>>();
    if !config.is_empty() || !options.is_empty() {
        return Err(ConfigProgramLoadError::Diagnostics { config, options });
    }
    validate_config_plan_for_mode(plan, false)
}

/// The gate of a config Program load. tsgo creates the Program whatever the
/// config reports: the config parsing diagnostics and the option
/// diagnostics are the Program's (`GetConfigFileParsingDiagnostics`,
/// `GetProgramDiagnostics`) while it still loads, checks and emits
/// (compiler/program.go:2010-2065). Only an unsupported config scope stops
/// the load.
fn validate_config_plan_for_mode(
    plan: &ConfigRootPlan,
    emitting: bool,
) -> Result<(), ConfigProgramLoadError> {
    if let Some((feature, detail)) = unsupported_config_scope(&plan.options, emitting) {
        return Err(ConfigProgramLoadError::Program(
            ProgramLoadError::unsupported_js(
                crate::loader::ProgramLoadOperation::ValidateOptions,
                Some(plan.config_file_name().to_owned()),
                feature,
                detail,
            ),
        ));
    }
    Ok(())
}

/// Whether an option diagnostic is reportable while the program still enters
/// the checker. The removed-option rows (TS5102, TS5108) and the source-map
/// relationship rows are program diagnostics that TypeScript 7.1 reports
/// while it still checks and emits; malformed values and structural option
/// errors remain a source-loading gate.
pub fn is_non_fatal_option_diagnostic(diagnostic: &Diagnostic) -> bool {
    matches!(diagnostic.code(), 5051 | 5053 | 5069 | 5102 | 5108)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConfigProgramMode {
    NoEmit { force: bool },
    Emit { force: bool },
}

fn load_config_program_inner(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
    mode: ConfigProgramMode,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    validate_config_plan_for_mode(plan, matches!(mode, ConfigProgramMode::Emit { .. }))?;

    match mode {
        ConfigProgramMode::NoEmit { force: false }
            if plan.compiler_options().no_emit != Some(true) =>
        {
            return Err(ConfigProgramLoadError::NoEmitRequired {
                value: plan.compiler_options().no_emit,
            });
        }
        ConfigProgramMode::Emit { force: false }
            if plan.compiler_options().no_emit == Some(true) =>
        {
            return Err(ConfigProgramLoadError::EmitRequired {
                value: plan.compiler_options().no_emit,
            });
        }
        ConfigProgramMode::NoEmit { .. } | ConfigProgramMode::Emit { .. } => {}
    }

    let roots = plan
        .file_names()
        .iter()
        .zip(&plan.root_reasons)
        .map(|(file_name, reason)| (file_name.clone(), reason.clone()))
        .collect::<Vec<_>>();
    let mut compiler_options = plan.compiler_options().clone();
    let mut program_options = plan.program_options().clone();
    if plan
        .project_references()
        .is_some_and(|references| !references.is_empty())
    {
        // tsgo parses every referenced project before the files are loaded
        // (fileloader.go: projectReferenceParser.parse); the loader redirects
        // the sources of those projects to their outputs.
        let current_directory = host.current_directory_js().map_err(|error| {
            ConfigProgramLoadError::Program(ProgramLoadError::host_js(
                crate::loader::ProgramLoadOperation::ValidateOptions,
                Some(plan.config_file_name().to_owned()),
                error,
            ))
        })?;
        let references = crate::project_references::resolve_project_references(
            &crate::config_host::CompilerConfigHost::new(host),
            plan,
            current_directory.as_js(),
        )
        .map_err(|error| {
            ConfigProgramLoadError::Program(ProgramLoadError::invalid_data_js(
                crate::loader::ProgramLoadOperation::ValidateOptions,
                Some(plan.config_file_name().to_owned()),
                format!("project references: {error}"),
            ))
        })?;
        program_options = program_options.with_project_references(Arc::new(references));
    }
    match mode {
        ConfigProgramMode::NoEmit { force: true } => compiler_options.no_emit = Some(true),
        ConfigProgramMode::Emit { force: true } => compiler_options.no_emit = Some(false),
        ConfigProgramMode::NoEmit { force: false } | ConfigProgramMode::Emit { force: false } => {}
    }
    if matches!(mode, ConfigProgramMode::Emit { .. }) {
        // Emit must see effective option diagnostics before noEmitOnError
        // decides whether to run declaration transforms or write output.
        program_options = program_options.with_program_owned_config_option_diagnostics();
    }
    let loaded = match mode {
        ConfigProgramMode::NoEmit { .. } => load_program_with_root_reasons(
            host,
            &roots,
            compiler_options,
            program_options,
            library_catalog,
            limits,
        ),
        ConfigProgramMode::Emit { .. } => load_emitting_program_with_root_reasons(
            host,
            &roots,
            compiler_options,
            program_options,
            library_catalog,
            limits,
        ),
    };
    loaded.map_err(ConfigProgramLoadError::Program)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigLocation {
    file_name: JsString,
    start: u32,
    length: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ConfigSpan {
    start: u32,
    length: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ConfigPathsSyntaxIndex {
    // All indexed nodes belong to the root config. Store its identity once;
    // large maps retain compact UTF-16 spans instead of cloning the file name
    // into every key and element location.
    file_name: Option<JsString>,
    compiler_options_name: Option<ConfigSpan>,
    mapping_locations: BTreeMap<JsString, ConfigPathsKeySyntax>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct ConfigPathsKeySyntax {
    mapping_locations: Vec<ConfigPathMappingLocation>,
    element_locations: BTreeMap<usize, Vec<ConfigSpan>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigPathMappingLocation {
    key_location: ConfigSpan,
    value_location: ConfigSpan,
}

#[derive(Clone, Debug, PartialEq)]
struct ConfigSpec {
    text: JsString,
    base_path: JsString,
    location: Option<ConfigLocation>,
}

#[derive(Clone, Debug, PartialEq)]
struct ConfigExtendsSpec {
    text: JsString,
    location: Option<ConfigLocation>,
}

/// An element of a config's `files`, `include` or `exclude` array as the
/// configs extending it inherit it (tsgo `applyExtendedConfig`): a path,
/// rebased to the extending config, or another value kept as written (the
/// spec validation then skips it).
#[derive(Clone, Debug, PartialEq)]
enum InheritedSpec {
    Path(ConfigSpec),
    Other(Value),
}

impl InheritedSpec {
    fn raw_value(&self) -> Value {
        match self {
            Self::Path(spec) => Value::String(spec.text.clone()),
            Self::Other(value) => value.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedConfigNode {
    source: ConfigSourceText,
    raw: Value,
    raw_property_names: BTreeSet<JsString>,
    options: ConfigOptionBag,
    files: Option<Vec<ConfigSpec>>,
    files_location: Option<ConfigLocation>,
    include: Option<Vec<ConfigSpec>>,
    exclude: Option<Vec<ConfigSpec>>,
    inheritable_files: Option<Vec<InheritedSpec>>,
    inheritable_include: Option<Vec<InheritedSpec>>,
    inheritable_exclude: Option<Vec<InheritedSpec>>,
    references: Option<Value>,
    watch_options: Option<ConfigOptionBag>,
    type_acquisition: ConfigOptionBag,
    compile_on_save: Option<Value>,
    unsupported_root_scopes: BTreeSet<String>,
    /// tsgo `getProjectReferences`' errors for the root config's
    /// `references` (reported after the file names').
    reference_errors: Vec<Diagnostic>,
    extended_sources: Vec<ConfigSourceText>,
    extended_source_files: Vec<JsString>,
}

/// Caller-owned cache of extended configs, matching TypeScript's optional
/// extendedConfigCache. Reuse retains source snapshots until `clear` is called.
/// A fresh parse without this object performs every read again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigExtendedCache {
    entries: BTreeMap<JsString, CachedExtendedConfig>,
}

impl ConfigExtendedCache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[derive(Clone, Debug, PartialEq)]
struct CachedExtendedConfig {
    file_name: JsString,
    source: Option<ConfigSourceText>,
    node: Option<Box<ParsedConfigNode>>,
    // Source parse/read diagnostics replay on a hit; option conversion
    // diagnostics belong only to the first parse's error collection.
    read_parse_diagnostics: Vec<Diagnostic>,
}

struct ParseContext<'a> {
    host: &'a dyn ConfigParseHost,
    extended_cache: Option<&'a mut ConfigExtendedCache>,
    stack: Vec<JsString>,
    root_parse_diagnostics: Vec<Diagnostic>,
    errors: Vec<Diagnostic>,
}

/// Parse one config graph and derive the `ConfigRootPlan` projection qualified
/// for the frozen valid compiler-config corpus and focused diagnostic
/// contracts.
/// This is not a complete `ParsedCommandLine` implementation.
///
/// Discovery is sequential because host-call and failure precedence are
/// observable. Parallel case execution belongs above this API and may share
/// the immutable returned plan.
///
/// The source pins below identify semantic references for this projection; they
/// do not claim complete ports of those functions.
///
/// tsc-port: parseJsonSourceFileConfigFileContent @6.0.3
/// tsc-hash: 07f1b78d7a64e7de9a0242477b0f035046682d3dea94128b82f5a3d8e477a7f2
/// tsc-span: _tsc.js:38973-39171
/// tsc-port: parseConfig @6.0.3
/// tsc-hash: 1f07635fad8d6fc935271b45fea3dc451ccb10e65298a000fa858f5d5c2cd883
/// tsc-span: _tsc.js:39272-39330
/// tsc-port: getFileNamesFromConfigSpecs @6.0.3
/// tsc-hash: e3e964c4d98e994b15426ba1aa62f92a633c16e08f78aba69ee958b88e5ab3c4
/// tsc-span: _tsc.js:39608-39661
pub fn parse_config_root_plan(
    host: &dyn ConfigParseHost,
    request: ConfigRootPlanRequest,
) -> Result<ConfigRootPlan, ConfigParseError> {
    parse_config_root_plan_inner(host, request, None, None)
}

/// Parse with the caller's extended-config cache. The root is always parsed
/// afresh. Cache hits preserve the first source spelling and conversion result.
pub fn parse_config_root_plan_with_cache(
    host: &dyn ConfigParseHost,
    request: ConfigRootPlanRequest,
    cache: &mut ConfigExtendedCache,
) -> Result<ConfigRootPlan, ConfigParseError> {
    parse_config_root_plan_inner(host, request, Some(cache), None)
}

/// tsgo `GetParsedCommandLineOfConfigFile` with the command line's options
/// (`mergeCompilerOptions`): they replace the config's after the extends
/// chain, and everything derived from the config reads the merged options.
pub fn parse_config_root_plan_with_command_line(
    host: &dyn ConfigParseHost,
    request: ConfigRootPlanRequest,
    command_line_options: &ConfigOptionBag,
    cache: &mut ConfigExtendedCache,
) -> Result<ConfigRootPlan, ConfigParseError> {
    parse_config_root_plan_inner(host, request, Some(cache), Some(command_line_options))
}

fn parse_config_root_plan_inner(
    host: &dyn ConfigParseHost,
    request: ConfigRootPlanRequest,
    extended_cache: Option<&mut ConfigExtendedCache>,
    command_line_options: Option<&ConfigOptionBag>,
) -> Result<ConfigRootPlan, ConfigParseError> {
    let config_file_name = normalized_path(&request.file_name, &request.base_path)?;
    let config_base = js_directory_name(&config_file_name);
    let mut context = ParseContext {
        host,
        extended_cache,
        stack: Vec::new(),
        root_parse_diagnostics: Vec::new(),
        errors: Vec::new(),
    };
    let phase_started = std::time::Instant::now();
    let mut node = context
        .parse_node(
            ConfigSourceText::new(request.file_name, request.text),
            &config_file_name,
            &config_base,
            true,
        )?
        .expect("the primary config cannot be a recursive child of itself");
    tsc_types::trace::mark("config: parse and options", phase_started);
    if let Some(command_line_options) = command_line_options {
        node.options.extend_from(command_line_options);
    }
    node.options.finalize_config_dir_templates(&config_base)?;
    if let Some(watch) = &mut node.watch_options {
        watch.finalize_group_config_dir_templates(&config_base, ConfigOptionGroup::Watch)?;
        watch.restore_public_entry_order();
    }
    node.type_acquisition.restore_public_entry_order();
    let paths_option_validation = paths_option_validation_plan(&node.options, &node.source);
    let discovery_options = effective_discovery_options(&node.options, &config_base)?;
    let mut module_resolution_options = config_module_resolution_options(
        &node.options,
        &discovery_options,
        &config_file_name,
        &node.source,
        host.use_case_sensitive_file_names(),
        paths_option_validation,
    )?;
    let mut option_diagnostics = validate_paths_option_diagnostics(
        module_resolution_options.compiler_options(),
        module_resolution_options.program_options(),
    );
    option_diagnostics.extend(no_lib_lib_option_diagnostics(&node.options, &node.source));
    option_diagnostics.extend(removed_option_diagnostics(
        &node.options,
        &node.source,
        config_file_name.as_js(),
        host.use_case_sensitive_file_names(),
    ));
    option_diagnostics.extend(option_relationship_diagnostics(&node.options, &node.source));
    sort_and_dedupe_diagnostics(&mut option_diagnostics);
    let phase_started = std::time::Instant::now();
    let DerivedFileNames {
        file_names,
        include_specs,
        exclude_specs,
    } = derive_file_names(
        host,
        &node,
        &config_base,
        &config_file_name,
        &discovery_options,
        &mut context.errors,
    )?;
    tsc_types::trace::mark("config: file names", phase_started);
    context.errors.append(&mut node.reference_errors);
    let phase_started = std::time::Instant::now();
    let root_reasons = config_root_reasons(
        &file_names,
        node.files.as_deref(),
        node.include.as_deref(),
        &config_base,
        &node.source.file_name,
        host.use_case_sensitive_file_names(),
    )?;
    let project_references = config_project_references(node.references.as_ref(), &config_base);
    let wildcard_directories = derive_wildcard_directories(
        &node,
        &config_base,
        &discovery_options,
        host.use_case_sensitive_file_names(),
    )?;
    tsc_types::trace::mark("config: root reasons, references, wildcards", phase_started);
    node.options.restore_public_entry_order();
    let files = node
        .files
        .as_ref()
        .map(|specs| specs.iter().map(|spec| spec.text.clone()).collect());
    let include = node
        .include
        .as_ref()
        .map(|specs| specs.iter().map(|spec| spec.text.clone()).collect());
    let exclude = node
        .exclude
        .as_ref()
        .map(|specs| specs.iter().map(|spec| spec.text.clone()).collect());
    let config_diagnostics = context
        .root_parse_diagnostics
        .iter()
        .chain(&context.errors)
        .cloned()
        .collect();
    let config_sources = node
        .extended_sources
        .iter()
        .map(|source| {
            Ok(PreparedAuxiliaryFile::from_snapshot(
                config_program_path(&source.file_name, host.use_case_sensitive_file_names())?,
                Arc::clone(source.snapshot()),
            ))
        })
        .collect::<Result<Vec<_>, ConfigParseError>>()?;
    module_resolution_options.program_options = module_resolution_options
        .program_options
        .with_config_parsing_diagnostics(config_diagnostics, config_sources);
    Ok(ConfigRootPlan {
        config_file_name,
        source: node.source,
        extended_sources: node.extended_sources,
        extended_source_files: node.extended_source_files,
        raw: node.raw,
        options: node.options,
        discovery_options,
        module_resolution_options,
        files,
        include,
        exclude,
        references: node.references,
        project_references,
        watch_options: node.watch_options.as_ref().map(typed_option_bag_json),
        type_acquisition: Some(typed_option_bag_json(&node.type_acquisition)),
        watch_option_bag: node.watch_options,
        type_acquisition_option_bag: node.type_acquisition,
        compile_on_save: node.compile_on_save,
        unsupported_root_scopes: node.unsupported_root_scopes,
        file_names,
        include_specs,
        exclude_specs,
        root_reasons,
        wildcard_directories,
        root_parse_diagnostics: context.root_parse_diagnostics,
        errors: context.errors,
        option_diagnostics,
    })
}

/// H0 is a single-project, no-emit driver. Recognized options which would
/// select an emitter, build graph, watch/incremental state, or a plugin are
/// therefore an unsupported *scope* failure, not an option we may silently
/// carry through the narrower `CompilerOptions` projection. This check runs
/// at the program-load gate rather than during parsing so the config oracle
/// can still observe TypeScript's complete partial `ParsedCommandLine` shape.
/// Root config scopes (`watchOptions`, `typeAcquisition`, `compileOnSave`)
/// are inert for a command that neither watches nor serves an editor: tsgo
/// reports their conversion diagnostics and reads them no further, so every
/// command admits them and the plan only observes them.
fn unsupported_config_scope(
    options: &ConfigOptionBag,
    emitting: bool,
) -> Option<(&'static str, String)> {
    // `references` are resolved by the config program load
    // (`resolve_project_references`) and consumed by the loader.
    for option in options.entries() {
        // Unknown names have already produced config conversion diagnostics
        // (TS5023) and do not request a feature in the converted options;
        // tsgo creates the Program after reporting them.
        if compiler_option_declaration(&option.name).is_none() {
            continue;
        }
        // The source map options are already projected and validated. An
        // ordinary no-emit command retains them without constructing an
        // emitter, as it does the emitter-only options below, which change
        // no diagnostic but their option rows. Keep this later extension
        // separate from the frozen H0 qualification inventory.
        let no_emit_projection = !emitting
            && H0_NO_EMIT_SOURCE_MAP_CONFIG_OPTIONS
                .iter()
                .chain(H0_NO_EMIT_NEUTRAL_CONFIG_OPTIONS)
                .chain(H0_NO_EMIT_DECLARATION_CONFIG_OPTIONS)
                .chain(H0_NO_EMIT_CHECKER_CONFIG_OPTIONS)
                .any(|candidate| option.name == *candidate);
        let service_option = LANGUAGE_SERVICE_CONFIG_OPTIONS
            .iter()
            .any(|candidate| option.name == *candidate);
        if !(config_option_is_supported_by_h0(&option.name)
            || no_emit_projection
            || service_option
            || emitting && config_option_is_projected_for_h1_emit(&option.name))
            && config_value_requests_feature(&option.value)
        {
            return Some((
                "unsupported-config-option",
                format!(
                    "compiler option {:?} is outside the {} single-project driver",
                    option.name,
                    if emitting {
                        "H1 emitting"
                    } else {
                        "H0 no-emit"
                    },
                ),
            ));
        }
    }
    None
}

fn config_project_references<'j0>(
    references: Option<&Value>,
    config_base_path: impl Into<JsStr<'j0>>,
) -> Option<Vec<ConfigProjectReference>> {
    let config_base_path = config_base_path.into();
    let values = references?.as_array()?;
    // tsgo `getProjectReferences`: an array, even an empty one, is a list
    // of references; an element without a non-empty string path is left
    // out (`project_reference_diagnostics` reports it).
    let mut result = Vec::new();
    for reference in values {
        let Some(object) = reference.as_object() else {
            continue;
        };
        let Some(original_path) = object
            .get("path")
            .and_then(Value::as_js)
            .filter(|path| !path.is_empty())
        else {
            continue;
        };
        let path = crate::js_path::normalized_absolute_path(original_path, config_base_path);
        result.push(ConfigProjectReference {
            path,
            original_path: original_path.to_owned(),
            prepend: object.get("prepend").and_then(Value::as_bool),
            circular: object.get("circular").and_then(Value::as_bool),
        });
    }
    Some(result)
}

/// tsgo `getProjectReferences`' checks of each reference: a missing or
/// non-string `path` (TS5024), an empty one (TS18051), a non-boolean
/// `circular` (TS5024), at the property's value or else the element.
/// tsgo's notifier converts `references` as a list option
/// (`convertJsonOption`): a value that is neither an array nor nil is TS5024.
/// Whether `node` is written as an array literal: tsgo converts one none of
/// whose elements converted to a nil slice, which its type checks still
/// read as an array.
fn is_array_literal(source: &SourceFile, node: NodeId) -> bool {
    source.arena.node(node).kind == SyntaxKind::ArrayLiteralExpression
}

fn validate_references_value(source: &SourceFile, errors: &mut Vec<Diagnostic>) {
    for property in config_root_object(source)
        .into_iter()
        .flat_map(|root| config_object_properties(source, root))
        .filter(|property| property.name == "references")
    {
        let Some(RecoverableJsonValue::Defined(value)) =
            convert_recoverable_json_node_to_value(source, property.initializer)
        else {
            continue;
        };
        if !value.is_array() && !value.is_null() {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &["references", "Array"],
                config_location(source, property.initializer),
            ));
        }
    }
}

fn project_reference_diagnostics(source: &SourceFile, references: &[Value]) -> Vec<Diagnostic> {
    let elements = config_property_initializer(source, "references")
        .map(|array| config_array_elements(source, array))
        .unwrap_or_default();
    let location = |index: usize, property: &str| {
        let element = *elements.get(index)?;
        let node = config_object_properties(source, element)
            .into_iter()
            .find(|candidate| candidate.name == property)
            .map_or(element, |candidate| candidate.initializer);
        config_location(source, node)
    };
    let mut diagnostics = Vec::new();
    for (index, reference) in references.iter().enumerate() {
        let Some(object) = reference.as_object() else {
            continue;
        };
        match object.get("path") {
            Some(Value::String(path)) if path.is_empty() => {
                diagnostics.push(config_diagnostic(
                    &gen::Compiler_option_0_cannot_be_given_an_empty_string,
                    &["reference.path"],
                    location(index, "path"),
                ));
                continue;
            }
            Some(Value::String(_)) => {}
            _ => {
                diagnostics.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &["reference.path", "string"],
                    location(index, "path"),
                ));
                continue;
            }
        }
        if object
            .get("circular")
            .is_some_and(|circular| !matches!(circular, Value::Bool(_)))
        {
            diagnostics.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &["reference.circular", "boolean"],
                location(index, "circular"),
            ));
        }
    }
    diagnostics
}

fn derive_wildcard_directories<'j0>(
    config: &ParsedConfigNode,
    config_base_path: impl Into<JsStr<'j0>>,
    discovery: &ConfigDiscoveryOptions,
    case_sensitive: bool,
) -> Result<Vec<ConfigWildcardDirectory>, ConfigParseError> {
    let config_base_path = config_base_path.into();
    // getWildcardDirectories consumes validated include specs even when
    // `files` is present. Only the implicit **/* depends on files being absent.
    let includes = if let Some(includes) = &config.include {
        includes.clone()
    } else if config.files.is_some() {
        Vec::new()
    } else {
        vec![ConfigSpec {
            text: "**/*".into(),
            base_path: config_base_path.to_owned(),
            location: None,
        }]
    };
    let excludes = if let Some(excludes) = &config.exclude {
        excludes.clone()
    } else {
        [discovery.out_dir.clone(), discovery.declaration_dir.clone()]
            .into_iter()
            .flatten()
            .map(|path| ConfigSpec {
                text: path,
                base_path: config_base_path.to_owned(),
                location: None,
            })
            .collect()
    };
    let excludes = excludes
        .iter()
        .map(|exclude| normalized_spec_path(exclude, config_base_path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut directories = Vec::new();
    for include in includes {
        let spec = normalized_spec_path(&include, config_base_path)?;
        if excludes
            .iter()
            .any(|exclude| wildcard_spec_is_excluded(&spec, exclude, case_sensitive))
        {
            continue;
        }
        let Some((path, recursive)) = wildcard_directory_from_spec(&spec) else {
            continue;
        };
        let key = if case_sensitive {
            path.clone()
        } else {
            file_name_key(path.as_js(), false)
        };
        if let Some(existing) =
            directories
                .iter_mut()
                .find(|entry: &&mut ConfigWildcardDirectory| {
                    let existing_key = if case_sensitive {
                        entry.path.clone()
                    } else {
                        file_name_key(entry.path.as_js(), false)
                    };
                    existing_key == key
                })
        {
            existing.recursive |= recursive;
        } else {
            directories.push(ConfigWildcardDirectory { path, recursive });
        }
    }

    // Watcher roots nested below an already-recursive root are removed by
    // TypeScript's canonical-key cleanup.  Keep insertion order for the
    // remaining entries; it is observable through ParsedCommandLine.
    let recursive_paths = directories
        .iter()
        .filter(|entry| entry.recursive)
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    directories.retain(|entry| {
        !recursive_paths.iter().any(|parent| {
            if same_path(parent, &entry.path, case_sensitive) {
                return false;
            }
            path_is_descendant(parent, &entry.path, case_sensitive)
        })
    });
    Ok(directories)
}

fn trim_config_path_separators(path: JsStr<'_>) -> JsStr<'_> {
    let length = path
        .as_bytes()
        .iter()
        .rposition(|byte| *byte != b'/')
        .map_or(0, |index| index + 1);
    path.split_at_byte(length)
        .expect("ASCII separator boundary")
        .0
}

fn wildcard_spec_is_excluded(spec: &JsString, exclude: &JsString, case_sensitive: bool) -> bool {
    let spec = file_name_key(spec.as_js(), case_sensitive);
    let exclude = file_name_key(exclude.as_js(), case_sensitive);
    let exclude = trim_config_path_separators(exclude.as_js());
    if !exclude
        .as_bytes()
        .iter()
        .any(|byte| matches!(byte, b'*' | b'?'))
    {
        return spec.as_js() == exclude || path_is_descendant(exclude, spec.as_js(), true);
    }
    ConfigFilePattern::new(exclude, "/", case_sensitive)
        .ok()
        .flatten()
        .is_some_and(|pattern| pattern.matches(&spec))
}

fn wildcard_directory_from_spec(spec: &JsString) -> Option<(JsString, bool)> {
    let spec = trim_config_path_separators(spec.as_js());
    if spec.is_empty() {
        return None;
    }
    let bytes = spec.as_bytes();
    let last_separator = bytes.iter().rposition(|byte| *byte == b'/');
    let wildcard = bytes.iter().position(|byte| matches!(byte, b'*' | b'?'));
    // Every measured position is an ASCII delimiter; only relative ordering
    // is used here, so byte ordering gives the same branch as UTF-16 offsets.
    let prefix = |index| spec.split_at_byte(index).expect("ASCII boundary").0;
    if let Some(wildcard) = wildcard {
        let recursive = wildcard < last_separator.unwrap_or(bytes.len());
        let path = if recursive {
            let component_separator = bytes[..wildcard]
                .iter()
                .rposition(|byte| *byte == b'/')
                .unwrap_or(0);
            if component_separator == 0 {
                JsStr::from("/")
            } else {
                prefix(component_separator)
            }
        } else {
            last_separator
                .map(|index| {
                    if index == 0 {
                        JsStr::from("/")
                    } else {
                        prefix(index)
                    }
                })
                .unwrap_or_else(|| ".".into())
        };
        return Some((path.to_owned(), recursive));
    }
    let file_name = last_separator
        .map(|index| spec.split_at_byte(index + 1).expect("ASCII boundary").1)
        .unwrap_or(spec);
    (!file_name.contains(".")).then(|| (spec.to_owned(), true))
}

fn same_path<'l, 'r>(
    left: impl Into<JsStr<'l>>,
    right: impl Into<JsStr<'r>>,
    case_sensitive: bool,
) -> bool {
    file_name_key(left.into(), case_sensitive) == file_name_key(right.into(), case_sensitive)
}

fn path_is_descendant<'p, 'c>(
    parent: impl Into<JsStr<'p>>,
    child: impl Into<JsStr<'c>>,
    case_sensitive: bool,
) -> bool {
    let parent = file_name_key(parent.into(), case_sensitive);
    let child = file_name_key(child.into(), case_sensitive);
    let mut prefix = trim_config_path_separators(parent.as_js()).to_owned();
    prefix.push('/');
    child.as_js().starts_with_js(prefix.as_js())
}

/// The config parser deliberately knows the complete TypeScript option
/// declaration table so it can reproduce `ParsedCommandLine` diagnostics.
/// That does not mean the no-emit loader can consume every recognized option:
/// an option which never reaches either `CompilerOptions`, `ProgramOptions`,
/// or root discovery would otherwise be silently ignored. Keep this allowlist
/// next to the fail-closed gate so adding a new projection requires an
/// explicit review of its execution semantics.
pub const H0_SUPPORTED_CONFIG_OPTIONS: &[&str] = &[
    // Discovery and checker-facing compiler options.
    "allowJs",
    "checkJs",
    "forceConsistentCasingInFileNames",
    "deduplicatePackages",
    "maxNodeModuleJsDepth",
    "experimentalDecorators",
    "target",
    "module",
    "moduleDetection",
    "alwaysStrict",
    "strict",
    "strictNullChecks",
    "strictFunctionTypes",
    "noImplicitAny",
    "noErrorTruncation",
    "noImplicitThis",
    "noImplicitOverride",
    "strictBindCallApply",
    "exactOptionalPropertyTypes",
    "noFallthroughCasesInSwitch",
    "noImplicitReturns",
    "noUnusedLocals",
    "noUnusedParameters",
    "allowUnreachableCode",
    "allowUnusedLabels",
    "noUncheckedIndexedAccess",
    "noPropertyAccessFromIndexSignature",
    "noUncheckedSideEffectImports",
    "strictPropertyInitialization",
    "useDefineForClassFields",
    "useUnknownInCatchVariables",
    "lib",
    "libReplacement",
    "jsx",
    "noEmit",
    "noResolve",
    "importHelpers",
    "downlevelIteration",
    "strictBuiltinIteratorReturn",
    "moduleResolution",
    "esModuleInterop",
    "allowSyntheticDefaultImports",
    "preserveConstEnums",
    "isolatedModules",
    "verbatimModuleSyntax",
    "allowUmdGlobalAccess",
    "baseUrl",
    "moduleSuffixes",
    "resolvePackageJsonExports",
    "resolvePackageJsonImports",
    "customConditions",
    "noDtsResolution",
    "allowArbitraryExtensions",
    "allowImportingTsExtensions",
    "rewriteRelativeImportExtensions",
    "resolveJsonModule",
    "skipLibCheck",
    "skipDefaultLibCheck",
    "jsxFactory",
    "jsxFragmentFactory",
    "jsxImportSource",
    "reactNamespace",
    "ignoreDeprecations",
    // Program-facing roots/resolution and default-exclude inputs.
    "noLib",
    "preserveSymlinks",
    "types",
    "typeRoots",
    "rootDirs",
    "paths",
    "outDir",
    "declarationDir",
];

/// Source map options a no-emit command retains without an emitter: they
/// select no checker behaviour, and their cross-option rows (TS5053,
/// TS5051, TS5069) are the option diagnostics tsgo reports for a `--noEmit`
/// check as well.
const H0_NO_EMIT_SOURCE_MAP_CONFIG_OPTIONS: &[&str] = &[
    "sourceMap",
    "inlineSourceMap",
    "inlineSources",
    "sourceRoot",
    "mapRoot",
];

/// Emitter-only options a no-emit command retains without an emitter: none
/// of them selects a checker behaviour or carries a cross-option config
/// diagnostic, so a `--noEmit` check of a project that sets them (Next.js's
/// `stripInternal`) reports exactly what tsc reports. An emitting command
/// still projects them through the H1 inventory.
const H0_NO_EMIT_NEUTRAL_CONFIG_OPTIONS: &[&str] = &[
    // The statistics are printed after the run.
    "diagnostics",
    "extendedDiagnostics",
    // The command records the compilation in a trace directory.
    "generateTrace",
    // A watch run keeps its output instead of clearing the screen.
    "preserveWatchOutput",
    "stripInternal",
    "newLine",
    "removeComments",
    "noEmitHelpers",
    "emitBOM",
    "listEmittedFiles",
    "listFiles",
    "explainFiles",
    "listFilesOnly",
    "pretty",
    // The resolution trace is printed while the program is created.
    "traceResolution",
    // The incremental options change what a command writes, not what it
    // reports; the build info file is not written yet (the roadmap's
    // incremental slice).
    "incremental",
    "tsBuildInfoFile",
    "assumeChangesOnlyAffectDirectDependencies",
    // A program that does not emit has no output to withhold, and `outFile`
    // is a removed option (TS5102) tsgo otherwise ignores.
    "noEmitOnError",
    "outFile",
];

/// Declaration-product options a no-emit command admits: tsc's
/// emitFilesAndReportErrors reports the declaration diagnostics of a
/// `--noEmit` command when getEmitDeclarations(options) holds, and the
/// driver's no-emit hook runs the same getter; `composite` adds the
/// project-listing and `rootDir` program diagnostics the loader reports for
/// an emitting command as well.
const H0_NO_EMIT_DECLARATION_CONFIG_OPTIONS: &[&str] = &[
    "declaration",
    "declarationMap",
    "emitDeclarationOnly",
    "isolatedDeclarations",
    "composite",
    "rootDir",
];

/// Language-service options every command retains: tsc declares `plugins`
/// under Editor Support ("A list of plugins to load in the language
/// service", _tsc.js:37896) and no tsc code path outside the service reads
/// `options.plugins`, so a check or an emit of a project that lists service
/// plugins (VS Code's tsec, Effect's language service) reports and writes
/// exactly what tsc does.
const LANGUAGE_SERVICE_CONFIG_OPTIONS: &[&str] = &["plugins"];

/// Checker-selecting emit options a no-emit command admits because the
/// checker implements their diagnostics: `emitDecoratorMetadata` marks the
/// decorator metadata type references (markDecoratorMetadataTypeNodeAsReferenced,
/// so `import type` aliases used only in decorated signatures count as
/// referenced) and the loader reports its experimentalDecorators
/// requirement, so a `--noEmit` check of a project that sets it (zod's base
/// tsconfig) reports exactly what tsc reports. `erasableSyntaxOnly` only
/// selects the checker's TS1294 rows (enums, instantiated namespaces,
/// parameter properties, `import =`/`export =`, angle-bracket assertions),
/// which a `--noEmit` check of Effect's base tsconfig reports like tsc.
/// `stableTypeOrdering` selects the checker's stable type order (tsc 6.0.3's
/// preview of the TypeScript 7 order), which changes union member and
/// property order in diagnostics and declarations but no file set.
const H0_NO_EMIT_CHECKER_CONFIG_OPTIONS: &[&str] = &[
    "emitDecoratorMetadata",
    "erasableSyntaxOnly",
    "stableTypeOrdering",
    // tsgo SkipTypeChecking: no source is checked; the build info of an
    // incremental program records the check as pending.
    "noCheck",
];

fn config_option_is_supported_by_h0<'n>(name: impl Into<JsStr<'n>>) -> bool {
    let name = name.into();
    H0_SUPPORTED_CONFIG_OPTIONS
        .iter()
        .any(|candidate| name == *candidate)
}

const H1_EMIT_PROJECTED_CONFIG_OPTIONS: &[&str] = &[
    "diagnostics",
    "extendedDiagnostics",
    "generateTrace",
    "preserveWatchOutput",
    // `pretty` only selects the diagnostic renderer, which the command line
    // already decides (`--pretty false`); zod's base tsconfig sets it, so an
    // emitting command admits it like the no-emit inventory does.
    "pretty",
    "listEmittedFiles",
    "listFiles",
    "explainFiles",
    "listFilesOnly",
    "traceResolution",
    "emitBOM",
    "noEmitOnError",
    "noCheck",
    "erasableSyntaxOnly",
    "rootDir",
    "sourceMap",
    "inlineSourceMap",
    "inlineSources",
    "sourceRoot",
    "mapRoot",
    "declaration",
    "declarationMap",
    "emitDeclarationOnly",
    "isolatedDeclarations",
    "stableTypeOrdering",
    "stripInternal",
    "outFile",
    "incremental",
    "composite",
    "assumeChangesOnlyAffectDirectDependencies",
    "tsBuildInfoFile",
    "emitDecoratorMetadata",
    "newLine",
    "removeComments",
    "noEmitHelpers",
];

fn config_option_is_projected_for_h1_emit<'n>(name: impl Into<JsStr<'n>>) -> bool {
    let name = name.into();
    H1_EMIT_PROJECTED_CONFIG_OPTIONS
        .iter()
        .any(|candidate| name == *candidate)
}

fn config_value_requests_feature(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(_) | Value::String(_) => true,
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
    }
}

/// tsgo `convertConfigFileToObject` (tsoptions/tsconfigparsing.go:311-337):
/// the config's object, the empty object for a text without a value. A root
/// that is not an object is TS5092, unless it is an array holding an object,
/// which tsgo recovers without reporting the root.
fn config_file_object(
    source: &ConfigSourceText,
    parsed: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Result<Value, ConfigParseError> {
    let mut raw = if json_source_file_is_empty(parsed) {
        Value::Object(Map::new())
    } else {
        convert_recoverable_json_source_file_to_value(parsed).ok_or_else(|| {
            ConfigParseError::new_js(
                ConfigParseErrorKind::Unsupported,
                Some(source.file_name.clone()),
                "the recovered config syntax tree is outside the currently ported JSONC conversion surface",
            )
        })?
    };
    let first_object = raw
        .as_array()
        .and_then(|values| values.iter().find(|value| value.is_object()))
        .cloned();
    if let Some(first_object) = first_object {
        raw = first_object;
    }
    if !raw.is_object() {
        let config_kind = if source
            .file_name
            .as_js()
            .split_ascii(b'/')
            .next_back()
            .and_then(|tail| tail.split_ascii(b'\\').next_back())
            == Some("jsconfig.json".into())
        {
            "jsconfig.json"
        } else {
            "tsconfig.json"
        };
        errors.push(config_diagnostic(
            &gen::The_root_value_of_a_0_file_must_be_an_object,
            &[config_kind.to_owned()],
            config_root_expression(parsed).and_then(|node| config_location(parsed, node)),
        ));
        raw = Value::Object(Map::new());
    }
    Ok(raw)
}

/// tsgo `ParseConfigFileTextToJson` (tsoptions/tsconfigparsing.go:703-713):
/// the config text's object and its errors, which are the text's first
/// parse diagnostic when it has any.
pub fn parse_config_file_text_to_json(
    file_name: impl Into<JsString>,
    text: impl Into<String>,
) -> Result<(Value, Vec<Diagnostic>), ConfigParseError> {
    let source = ConfigSourceText::new(file_name, text);
    let parsed = parse_config_source(&source)?;
    let mut errors = config_json_conversion_diagnostics(&parsed);
    let raw = config_file_object(&source, &parsed, &mut errors)?;
    if let Some(first) = parsed.parse_diagnostics.first() {
        errors = vec![first.clone()];
    }
    Ok((config_raw_projection(raw), errors))
}

/// tsgo parses a config as a JSON source file (`parseJSONText`), whose
/// parse also validates the value: TS1327 for a name or string that is not
/// double-quoted, TS1328 for a value JSON has no form for, TS1136 for a
/// member that is not a property assignment.
fn parse_config_source(source: &ConfigSourceText) -> Result<SourceFile, ConfigParseError> {
    if config_parser_preflight(source.text()) == JsonParserPreflight::ResourceLimit {
        return Err(ConfigParseError::new_js(
            ConfigParseErrorKind::ResourceLimit,
            Some(source.file_name.clone()),
            "config JSON nesting exceeds the 256-level parser limit",
        ));
    }
    Ok(tsc_syntax::parse_json_source_text_from_snapshot(
        source.file_name.clone(),
        Arc::clone(source.snapshot()),
    ))
}

impl ParseContext<'_> {
    // tsc-port: getExtendedConfig @6.0.3
    // tsc-hash: 545d6ab16e97cc943150aa4dd577a88bb693aa81ca82a86f7a75328b37d9084f
    // tsc-span: _tsc.js:39460-39500
    fn extended_config<'j0>(
        &mut self,
        path: impl Into<JsStr<'j0>>,
    ) -> Result<CachedExtendedConfig, ConfigParseError> {
        let path = path.into();
        let key = canonical_key(path, self.host.use_case_sensitive_file_names());
        if let Some(entry) = self
            .extended_cache
            .as_ref()
            .and_then(|cache| cache.entries.get(&key))
            .cloned()
        {
            self.errors
                .extend(entry.read_parse_diagnostics.iter().cloned());
            return Ok(entry);
        }
        let mut entry = CachedExtendedConfig {
            file_name: path.to_owned(),
            source: None,
            node: None,
            read_parse_diagnostics: Vec::new(),
        };
        match self.host.read_file(path) {
            Ok(Some(text)) => {
                let source = ConfigSourceText::new(path, text);
                let parsed = parse_config_source(&source)?;
                entry.read_parse_diagnostics = parsed.parse_diagnostics.to_vec();
                entry.source = Some(source.clone());
                entry.node = self
                    .parse_node_from_source(source, parsed, path, &js_directory_name(path), false)?
                    .map(Box::new);
            }
            Ok(None) => {
                entry.read_parse_diagnostics.push(config_diagnostic(
                    &gen::Cannot_read_file_0,
                    &[path.to_owned()],
                    None,
                ));
                self.errors
                    .extend(entry.read_parse_diagnostics.iter().cloned());
            }
            Err(error) => {
                entry.read_parse_diagnostics.push(config_diagnostic(
                    &gen::Cannot_read_file_0_1,
                    &[path.to_owned(), JsString::from(error.detail())],
                    None,
                ));
                self.errors
                    .extend(entry.read_parse_diagnostics.iter().cloned());
            }
        }
        if let Some(cache) = &mut self.extended_cache {
            cache.entries.insert(key, entry.clone());
        }
        Ok(entry)
    }

    fn parse_node<'j0, 'j1>(
        &mut self,
        source: ConfigSourceText,
        normalized_file_name: impl Into<JsStr<'j0>>,
        base_path: impl Into<JsStr<'j1>>,
        is_root: bool,
    ) -> Result<Option<ParsedConfigNode>, ConfigParseError> {
        let normalized_file_name = normalized_file_name.into();
        let base_path = base_path.into();
        let parsed = parse_config_source(&source)?;
        self.parse_node_from_source(source, parsed, normalized_file_name, base_path, is_root)
    }

    fn parse_node_from_source<'j0, 'j1>(
        &mut self,
        source: ConfigSourceText,
        parsed: SourceFile,
        normalized_file_name: impl Into<JsStr<'j0>>,
        base_path: impl Into<JsStr<'j1>>,
        is_root: bool,
    ) -> Result<Option<ParsedConfigNode>, ConfigParseError> {
        let normalized_file_name = normalized_file_name.into();
        let base_path = base_path.into();
        if !parsed.parse_diagnostics.is_empty() {
            if is_root {
                self.root_parse_diagnostics
                    .extend(parsed.parse_diagnostics.iter().cloned());
            } else {
                self.errors.extend(parsed.parse_diagnostics.iter().cloned());
                return Ok(None);
            }
        }
        if self.stack.len() >= MAX_CONFIG_EXTENDS_DEPTH {
            return Err(ConfigParseError::new_js(
                ConfigParseErrorKind::ResourceLimit,
                Some(normalized_file_name.to_owned()),
                format!("config extends depth exceeds the {MAX_CONFIG_EXTENDS_DEPTH}-source limit"),
            ));
        }
        let cache_key = normalized_file_name.to_owned();
        if self.stack.iter().any(|entry| entry == &cache_key) {
            // parseConfig's cycle arm still converts the source object, but it
            // does not run the option notifier or publish a successful node.
            // Conversion here retains the same unsupported recovery boundary.
            // The owned TS1327/TS1328 conversion diagnostics are appended
            // after the circularity diagnostic below, matching parseConfig's
            // cycle arm; unported syntax shapes stay a later slice.
            if !json_source_file_is_empty(&parsed) {
                convert_recoverable_json_source_file_to_value(&parsed).ok_or_else(|| {
                    ConfigParseError::new_js(
                        ConfigParseErrorKind::Unsupported,
                        Some(source.file_name.clone()),
                        "the cyclic config syntax tree is outside the currently ported JSONC conversion surface",
                    )
                })?;
            }
            // tsgo reports the cycle without its argument, so the message
            // keeps `{0}` (tsoptions/tsconfigparsing.go:1096).
            self.errors.push(config_diagnostic(
                &gen::Circularity_detected_while_resolving_configuration_0,
                &[] as &[String],
                None,
            ));
            self.errors
                .extend(config_json_cycle_conversion_diagnostics(&parsed));
            return Ok(None);
        }
        self.stack.push(cache_key.clone());
        let result =
            self.parse_node_uncached(source, parsed, normalized_file_name, base_path, is_root);
        self.stack.pop();
        result
    }

    fn parse_node_uncached<'j0, 'j1>(
        &mut self,
        source: ConfigSourceText,
        parsed: SourceFile,
        normalized_file_name: impl Into<JsStr<'j0>>,
        base_path: impl Into<JsStr<'j1>>,
        is_root: bool,
    ) -> Result<Option<ParsedConfigNode>, ConfigParseError> {
        let normalized_file_name = normalized_file_name.into();
        let base_path = base_path.into();
        let mut own_errors = config_json_conversion_diagnostics(&parsed);
        let json_conversion_error_count = own_errors.len();
        let mut raw = config_file_object(&source, &parsed, &mut own_errors)?;
        let object = raw
            .as_object()
            .expect("a non-object config was replaced with the empty object");
        // JavaScript objects retain an own key even when its assigned value is
        // `undefined`. The serde projection deliberately omits that value, so
        // preserve root presence separately for hasProperty-based config
        // decisions such as TS18002/TS18003.
        let mut raw_property_names = config_root_object(&parsed)
            .into_iter()
            .flat_map(|root| config_object_properties(&parsed, root))
            .map(|property| property.name)
            .collect::<BTreeSet<_>>();
        let mut unsupported_root_scopes = BTreeSet::new();
        let own_references =
            config_property_get(object, &raw_property_names, "references").cloned();
        let raw_watch_options =
            config_property_get(object, &raw_property_names, "watchOptions").cloned();
        let raw_type_acquisition =
            config_property_get(object, &raw_property_names, "typeAcquisition").cloned();
        let own_compile_on_save_present = raw_property_names.contains("compileOnSave".as_bytes());
        let own_compile_on_save =
            config_property_get(object, &raw_property_names, "compileOnSave").cloned();

        let mut own_options = default_compiler_options(normalized_file_name, base_path);
        let mut converted_own_options = config_option_group(
            base_path,
            ConfigOptionGroup::Compiler,
            &parsed,
            &mut own_errors,
        )?;
        // parseConfig records the declaring config directory beside every
        // truthy own `paths` value before extends are merged. An invalid or
        // null own value masks inherited paths but deliberately leaves an
        // inherited pathsBasePath untouched, matching ordinary JavaScript
        // assignment of the two independent option properties.
        if converted_own_options.typed_object_value("paths").is_some() {
            converted_own_options.insert_typed(
                "pathsBasePath",
                Some(ConfigTypedOptionValue::Json(Value::String(
                    base_path.into(),
                ))),
            );
        }
        own_options.extend_from(&converted_own_options);
        // tsgo's tsconfig root options do not declare `watchOptions`: its
        // value is converted as plain JSON and nothing in it is diagnosed.
        let own_watch_options = config_option_group(
            base_path,
            ConfigOptionGroup::Watch,
            &parsed,
            &mut Vec::new(),
        )?;
        let own_watch_options =
            (!own_watch_options.typed_entries.is_empty()).then_some(own_watch_options);
        let mut type_acquisition = default_type_acquisition(normalized_file_name);
        type_acquisition.extend_from(&config_option_group(
            base_path,
            ConfigOptionGroup::Acquisition,
            &parsed,
            &mut own_errors,
        )?);
        validate_compile_on_save(&parsed, base_path, &mut own_errors)?;
        validate_references_value(&parsed, &mut own_errors);
        let own_files = specs("files", base_path, &parsed, &mut own_errors);
        let own_include = specs("include", base_path, &parsed, &mut own_errors);
        let own_exclude = specs("exclude", base_path, &parsed, &mut own_errors);
        for property in config_root_object(&parsed)
            .into_iter()
            .flat_map(|root| config_object_properties(&parsed, root))
            .filter(|property| property.name == "excludes")
        {
            own_errors.push(config_diagnostic(
                &gen::Unknown_option_excludes_Did_you_mean_exclude,
                &[] as &[String],
                config_location(&parsed, property.name_node),
            ));
        }
        // tsgo applyExtendedConfig: a property the config writes, whatever
        // its value, is not inherited.
        let blocks_inherited_files = raw_property_names.contains("files".as_bytes());
        let blocks_inherited_include = raw_property_names.contains("include".as_bytes());
        let blocks_inherited_exclude = raw_property_names.contains("exclude".as_bytes());
        let has_own_files = own_files.is_some();
        let has_own_include = own_include.is_some();
        let has_own_exclude = own_exclude.is_some();

        let mut inherited_options = ConfigOptionBag::default();
        let mut inherited_files = None;
        let mut inherited_include = None;
        let mut inherited_exclude = None;
        let mut inherited_watch_options: Option<ConfigOptionBag> = None;
        let mut inherited_compile_on_save = None;
        let mut extended_sources = Vec::new();
        let mut seen_sources = BTreeSet::new();
        let mut extended_source_files = Vec::new();
        let mut seen_source_files = BTreeSet::new();

        // parseOwnConfig resolves every array entry before parseConfig reads
        // any extended source. That two-phase host order is observable when a
        // later path probe fails.
        let mut extended_paths = Vec::new();
        for extends in extends_value_occurrences(&parsed, &mut own_errors) {
            extended_paths = extends
                .into_iter()
                .map(|extends| self.resolve_extends(&extends, base_path, &mut own_errors))
                .collect::<Result<Vec<_>, _>>()?;
        }
        // tsgo `!jsonObject.Has("compilerOptions")`: the key is set whatever
        // its value converted to (nil included).
        let misplaced_root_option = (!raw_property_names.contains("compilerOptions".as_bytes()))
            .then(|| {
                config_root_object(&parsed)
                    .into_iter()
                    .flat_map(|root| config_object_properties(&parsed, root))
                    .find(|property| is_command_option_without_build(&property.name))
            })
            .flatten();
        order_config_conversion_and_notifier_diagnostics(
            &parsed,
            &mut own_errors,
            json_conversion_error_count,
        );
        if let Some(property) = misplaced_root_option {
            own_errors.push(config_diagnostic(
                &gen::_0_should_be_set_inside_the_compilerOptions_object_of_the_config_json_file,
                std::slice::from_ref(&property.name),
                config_location(&parsed, property.name_node),
            ));
        }
        self.errors.extend(own_errors);
        for extended_path in extended_paths.into_iter().flatten() {
            let entry = self.extended_config(&extended_path)?;
            if seen_source_files.insert(entry.file_name.clone()) {
                extended_source_files.push(entry.file_name);
            }
            if let Some(source) = entry.source {
                if seen_sources.insert(source.file_name.clone()) {
                    extended_sources.push(source);
                }
            }
            let Some(extended) = entry.node else {
                continue;
            };
            inherited_options.extend_from(&extended.options);
            let case_sensitive = self.host.use_case_sensitive_file_names();
            for (blocked, inheritable, inherited) in [
                (
                    blocks_inherited_files,
                    &extended.inheritable_files,
                    &mut inherited_files,
                ),
                (
                    blocks_inherited_include,
                    &extended.inheritable_include,
                    &mut inherited_include,
                ),
                (
                    blocks_inherited_exclude,
                    &extended.inheritable_exclude,
                    &mut inherited_exclude,
                ),
            ] {
                if let Some(specs) = inheritable.as_deref().filter(|_| !blocked) {
                    *inherited = Some(rebase_inherited_specs(specs, base_path, case_sensitive)?);
                }
            }
            for extended_source in &extended.extended_sources {
                if seen_sources.insert(extended_source.file_name.clone()) {
                    extended_sources.push(extended_source.clone());
                }
            }
            for extended_source_file in &extended.extended_source_files {
                if seen_source_files.insert(extended_source_file.clone()) {
                    extended_source_files.push(extended_source_file.clone());
                }
            }
            if let Some(watch) = &extended.watch_options {
                inherited_watch_options
                    .get_or_insert_with(ConfigOptionBag::default)
                    .extend_from(watch);
            }
            if extended.compile_on_save.is_some() {
                inherited_compile_on_save = extended.compile_on_save.clone();
            }
        }
        inherited_options.extend_from(&own_options);
        own_options = inherited_options;

        // The spec validation keeps the inherited paths only.
        let inherited_paths = |inherited: &Option<Vec<InheritedSpec>>| {
            inherited.as_ref().map(|specs| {
                specs
                    .iter()
                    .filter_map(|spec| match spec {
                        InheritedSpec::Path(spec) => Some(spec.clone()),
                        InheritedSpec::Other(_) => None,
                    })
                    .collect::<Vec<_>>()
            })
        };
        let files = own_files.or_else(|| inherited_paths(&inherited_files));
        let files_location = has_own_files
            .then(|| config_property_initializer(&parsed, "files"))
            .flatten()
            .and_then(|node| config_location(&parsed, node));
        let include = own_include.or_else(|| inherited_paths(&inherited_include));
        let exclude = own_exclude.or_else(|| inherited_paths(&inherited_exclude));
        let mut watch_options = inherited_watch_options;
        if let Some(own) = own_watch_options {
            watch_options
                .get_or_insert_with(ConfigOptionBag::default)
                .extend_from(&own);
        }
        let compile_on_save = if own_compile_on_save_present {
            own_compile_on_save
        } else {
            // parseConfig preserves the last base value, but copies it into
            // the child's raw config only when that value is truthy.
            inherited_compile_on_save.filter(json_value_is_truthy)
        };
        for (name, value) in [
            ("watchOptions", raw_watch_options.as_ref()),
            ("typeAcquisition", raw_type_acquisition.as_ref()),
            ("compileOnSave", compile_on_save.as_ref()),
        ] {
            if value.is_some_and(json_value_is_truthy) {
                unsupported_root_scopes.insert(name.to_owned());
            }
        }
        if watch_options.is_some() {
            unsupported_root_scopes.insert("watchOptions".to_owned());
        }
        let raw_object = raw
            .as_object_mut()
            .expect("config raw was validated as an object");
        for (name, was_own, inherited) in [
            ("files", has_own_files, &inherited_files),
            ("include", has_own_include, &inherited_include),
            ("exclude", has_own_exclude, &inherited_exclude),
        ] {
            if !was_own {
                if let Some(specs) = inherited {
                    raw_object.insert(
                        name.to_owned(),
                        Value::Array(specs.iter().map(InheritedSpec::raw_value).collect()),
                    );
                    raw_property_names.insert(name.into());
                }
            }
        }
        if !raw_property_names.contains("compileOnSave".as_bytes()) {
            if let Some(value) = &compile_on_save {
                raw_object.insert("compileOnSave".to_owned(), value.clone());
                raw_property_names.insert("compileOnSave".into());
            }
        }
        let inheritable_files =
            inheritable_specs(raw_object, &raw_property_names, "files", base_path, &parsed);
        let inheritable_include = inheritable_specs(
            raw_object,
            &raw_property_names,
            "include",
            base_path,
            &parsed,
        );
        let inheritable_exclude = inheritable_specs(
            raw_object,
            &raw_property_names,
            "exclude",
            base_path,
            &parsed,
        );

        let reference_errors = match (&own_references, is_root) {
            (Some(Value::Array(references)), true) => {
                project_reference_diagnostics(&parsed, references.as_slice())
            }
            _ => Vec::new(),
        };
        Ok(Some(ParsedConfigNode {
            source,
            raw: config_raw_projection(raw),
            raw_property_names,
            options: own_options,
            files,
            files_location,
            include,
            exclude,
            inheritable_files,
            inheritable_include,
            inheritable_exclude,
            references: own_references,
            watch_options,
            type_acquisition,
            compile_on_save,
            unsupported_root_scopes,
            reference_errors,
            extended_sources,
            extended_source_files,
        }))
    }

    fn resolve_extends<'j0>(
        &self,
        extends: &ConfigExtendsSpec,
        base_path: impl Into<JsStr<'j0>>,
        errors: &mut Vec<Diagnostic>,
    ) -> Result<Option<JsString>, ConfigParseError> {
        let base_path = base_path.into();
        let slashed = normalize_slashes(extends.text.as_js());
        if slashed.starts_with("/")
            || is_drive_rooted(&slashed)
            || slashed.starts_with("./")
            || slashed.starts_with("../")
        {
            let candidate = normalized_path(&slashed, base_path)?;
            let candidate_exists = self.host.file_exists(candidate.as_js())?;
            if candidate_exists || candidate.ends_with(".json") {
                return Ok(Some(candidate));
            }
            if !candidate.ends_with(".json") {
                let mut json = candidate.clone();
                json.push_str(".json");
                if self.host.file_exists(json.as_js())? {
                    return Ok(Some(json));
                }
            }
            errors.push(config_diagnostic(
                &gen::File_0_not_found,
                std::slice::from_ref(&extends.text),
                extends.location.clone(),
            ));
            return Ok(None);
        }
        let resolved = self.resolve_package_extends(&slashed, base_path)?;
        if resolved.is_none() {
            let (message, args) = if extends.text.is_empty() {
                (
                    &gen::Compiler_option_0_cannot_be_given_an_empty_string,
                    vec![JsString::from("extends")],
                )
            } else {
                (&gen::File_0_not_found, vec![extends.text.clone()])
            };
            errors.push(config_diagnostic(message, &args, extends.location.clone()));
        }
        Ok(resolved)
    }

    fn resolve_package_extends<'j0, 'j1>(
        &self,
        specifier: impl Into<JsStr<'j0>>,
        base_path: impl Into<JsStr<'j1>>,
    ) -> Result<Option<JsString>, ConfigParseError> {
        let specifier = specifier.into();
        let base_path = base_path.into();
        let compiler_host = ConfigCompilerHostAdapter {
            host: self.host,
            current_directory: base_path,
        };
        let options = CompilerOptions {
            module_resolution: Some(99),
            resolve_json_module: Some(true),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&compiler_host, &options).map_err(config_error_from_resolution)?;
        let containing_file = join_path(base_path, "tsconfig.json");
        match resolver
            .resolve_json_config(&containing_file, specifier)
            .map_err(config_error_from_resolution)?
        {
            ResolutionOutcome::Resolved(module) => {
                Ok(Some(module.resolved_file().display().to_owned()))
            }
            ResolutionOutcome::NotFound => Ok(None),
        }
    }
}

#[derive(Clone, Debug)]
struct ConfigPropertyNode {
    name: JsString,
    name_node: NodeId,
    initializer: NodeId,
}

fn order_config_conversion_and_notifier_diagnostics(
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
    json_conversion_error_count: usize,
) {
    // convertToJson completes each property initializer before onPropertySet
    // runs its option notifier. A compacted list can therefore publish a
    // notifier diagnostic at an earlier AST element than a conversion-time
    // diagnostic which must still precede it. Group diagnostics by the direct
    // root/schema-option property and order the two phases explicitly.
    // This replaces the former adjacent-swap repair, whose inversion count
    // could make a large invalid list quadratic.
    let diagnostic_owners = config_diagnostic_owners(source);
    let mut indexed = std::mem::take(errors)
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    indexed.sort_by_cached_key(|(original_index, diagnostic)| {
        let owner_start = config_diagnostic_owner(&diagnostic_owners, diagnostic)
            .map_or_else(|| diagnostic.start.unwrap_or(u32::MAX), |owner| owner.start);
        let phase = u8::from(*original_index >= json_conversion_error_count);
        (owner_start, phase, *original_index)
    });
    errors.extend(indexed.into_iter().map(|(_, diagnostic)| diagnostic));
}

fn config_diagnostic_owner<'a>(
    owners: &'a [ConfigLocation],
    diagnostic: &Diagnostic,
) -> Option<&'a ConfigLocation> {
    let diagnostic_start = diagnostic.start?;
    let diagnostic_end = diagnostic_start.saturating_add(diagnostic.length?);
    owners
        .iter()
        .filter(|owner| {
            owner.file_name.as_js()
                == diagnostic
                    .file_name
                    .as_ref()
                    .map(JsString::as_js)
                    .unwrap_or_else(|| "".into())
                && owner.start <= diagnostic_start
                && diagnostic_end <= owner.start.saturating_add(owner.length)
        })
        .min_by_key(|owner| owner.length)
}

fn config_diagnostic_owners(source: &SourceFile) -> Vec<ConfigLocation> {
    let mut owners = Vec::new();
    let Some(root) = config_root_object(source) else {
        return owners;
    };
    for property in config_object_properties(source, root) {
        if let Some(owner) = config_property_owner_location(source, &property) {
            owners.push(owner);
        }
        if matches!(
            property.name.as_str(),
            Some("compilerOptions" | "watchOptions" | "typeAcquisition")
        ) {
            owners.extend(
                config_object_properties(source, property.initializer)
                    .into_iter()
                    .filter_map(|property| config_property_owner_location(source, &property)),
            );
        }
    }
    owners
}

fn config_property_owner_location(
    source: &SourceFile,
    property: &ConfigPropertyNode,
) -> Option<ConfigLocation> {
    let name = config_location(source, property.name_node)?;
    let initializer = config_location(source, property.initializer)?;
    let end = initializer.start.saturating_add(initializer.length);
    Some(ConfigLocation {
        file_name: name.file_name,
        start: name.start,
        length: end.saturating_sub(name.start),
    })
}

fn config_diagnostic<A: DiagnosticArgument>(
    message: &'static DiagnosticMessage,
    args: &[A],
    location: Option<ConfigLocation>,
) -> Diagnostic {
    let args = args
        .iter()
        .map(|arg| arg.diagnostic_value().to_owned())
        .collect::<Vec<_>>();
    config_diagnostic_from_chain(MessageChain::new_js(message, &args), location)
}

fn config_diagnostic_from_chain(
    message: MessageChain,
    location: Option<ConfigLocation>,
) -> Diagnostic {
    match location {
        Some(location) => Diagnostic::new_js(
            Some(location.file_name),
            Some(location.start),
            Some(location.length),
            message,
        ),
        None => Diagnostic::new(None, None, None, message),
    }
}

/// tsc-port: forEachOptionsSyntaxByName @6.0.3
/// tsc-hash: ed0d42bfbaa8ddec3118b39c50eb3f8265a9c5c84d6919f9e17eab11ec9cfe87
/// tsc-span: _tsc.js:20114-20116
/// tsc-port: forEachOptionPathsSyntax @6.0.3
/// tsc-hash: a2286273dd795f88a63a473304f050689ca82b479837b5589edc6e675674f7b6
/// tsc-span: _tsc.js:125334-125336
/// tsc-port: getCompilerOptionsObjectLiteralSyntax @6.0.3
/// tsc-hash: 678b62e231c7b528be19bf93d113fccf8e6dcad5035300f5d4b5b6b6c97876e8
/// tsc-span: _tsc.js:125389-125395
/// tsc-port: getCompilerOptionsPropertySyntax @6.0.3
/// tsc-hash: 4c61a15e16a789ede2365cd0244cf8457f4de81567509ffc83aac12c00ae5206
/// tsc-span: _tsc.js:125396-125405
fn config_paths_syntax_index(source: &SourceFile) -> ConfigPathsSyntaxIndex {
    let mut result = ConfigPathsSyntaxIndex::default();
    let Some(root) = config_root_expression(source) else {
        return result;
    };
    if source.arena.node(root).kind != SyntaxKind::ObjectLiteralExpression {
        return result;
    }
    let Some(compiler_options) = config_object_properties(source, root)
        .into_iter()
        .find(|property| property.name == "compilerOptions")
    else {
        return result;
    };
    result.compiler_options_name = config_span(source, compiler_options.name_node);
    if result.compiler_options_name.is_some() {
        result.file_name = Some(source.file_name.clone());
    }
    if source.arena.node(compiler_options.initializer).kind != SyntaxKind::ObjectLiteralExpression {
        return result;
    }

    for paths in config_object_properties(source, compiler_options.initializer)
        .into_iter()
        .filter(|property| property.name == "paths")
    {
        if source.arena.node(paths.initializer).kind != SyntaxKind::ObjectLiteralExpression {
            continue;
        }
        let mut object_locations = BTreeMap::<JsString, ConfigPathsKeySyntax>::new();
        for mapping in config_object_properties(source, paths.initializer) {
            let Some(key_location) = config_span(source, mapping.name_node) else {
                continue;
            };
            let Some(value_location) = config_span(source, mapping.initializer) else {
                continue;
            };
            let key_locations = object_locations.entry(mapping.name).or_default();
            if source.arena.node(mapping.initializer).kind == SyntaxKind::ArrayLiteralExpression {
                for (index, element) in config_array_elements(source, mapping.initializer)
                    .into_iter()
                    .enumerate()
                {
                    if let Some(location) = config_span(source, element) {
                        key_locations
                            .element_locations
                            .entry(index)
                            .or_default()
                            .push(location);
                    }
                }
            }
            key_locations
                .mapping_locations
                .push(ConfigPathMappingLocation {
                    key_location,
                    value_location,
                });
        }
        for (key, locations) in object_locations {
            // `forEachOptionPathsSyntax` visits every duplicate `paths`
            // property because the diagnostic callback returns `undefined`.
            // Each object then reports every duplicate occurrence of the
            // effective mapping key.
            let indexed = result.mapping_locations.entry(key).or_default();
            indexed
                .mapping_locations
                .extend(locations.mapping_locations);
            for (index, spans) in locations.element_locations {
                indexed
                    .element_locations
                    .entry(index)
                    .or_default()
                    .extend(spans);
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
enum ConfigPathsDiagnosticLocation {
    Key,
    Value,
    Element(usize),
}

struct PendingConfigPathsViolation<'a> {
    key: JsStr<'a>,
    target: ConfigPathsDiagnosticLocation,
    kind: PathsOptionViolationKind,
}

/// Validate the converted paths map at the same post-substitution boundary as
/// `verifyCompilerOptions`. The syntax index deliberately retains only the
/// first direct root `compilerOptions` object: inherited options and recovered
/// objects from a non-object root use TypeScript's compiler-options fallback.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (paths block)
/// tsc-hash: e18b8511def0edd57da25ed1bbcbd52b5d675efdeba80d8f8e924b5cb2a9b391
/// tsc-span: _tsc.js:124805-124854
/// tsc-port: createDiagnosticForOptionPathKeyValue @6.0.3
/// tsc-hash: beca42abce599ae7f74d7261ade13fbf4927951d2809838f8132108602cd1784
/// tsc-span: _tsc.js:125298-125314
/// tsc-port: createDiagnosticForOptionPaths @6.0.3
/// tsc-hash: ac32dabd364ccbd1f794ab7c115dbfe7eb1485ecb209dea03b68f997e20bb787
/// tsc-span: _tsc.js:125315-125333
fn paths_option_validation_plan(
    options: &ConfigOptionBag,
    source: &ConfigSourceText,
) -> PathsOptionValidationPlan {
    let pending = pending_paths_option_violations(options);
    if pending.is_empty() {
        return PathsOptionValidationPlan::default();
    }

    // Location indexing is used only when a map contains a potential
    // violation. Bare substitutions are retained even when the config's own
    // baseUrl suppresses TS5090 because an embedding may replace that option
    // before Program construction.
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let syntax = config_paths_syntax_index(&parsed);
    let mut violations = Vec::with_capacity(pending.len());
    for pending in pending {
        push_paths_violations(
            &mut violations,
            &syntax,
            pending.key,
            pending.target,
            pending.kind,
        );
    }
    PathsOptionValidationPlan::new(violations)
}

/// Validate the `lib`/`noLib` option pair at the same post-conversion
/// boundary as `verifyCompilerOptions`. The row lands on the first of the two
/// properties in document order, as tsgo's `createDiagnosticForOptionName`
/// over `tsoptions.ForEachPropertyAssignment` does (tsc 6.0 reported both).
///
/// tsgo-port: verifyCompilerOptions (lib/noLib) @7.1 (program.go:1184-1186)
fn no_lib_lib_option_diagnostics(
    options: &ConfigOptionBag,
    source: &ConfigSourceText,
) -> Vec<Diagnostic> {
    let has_lib = matches!(
        options.typed_value_state("lib"),
        ConfigOptionValueState::List(_)
    );
    let no_lib_enabled = matches!(
        options.typed_value_state("noLib"),
        ConfigOptionValueState::Value(Value::Bool(true))
    );
    if !has_lib || !no_lib_enabled {
        return Vec::new();
    }

    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let mut locations = Vec::new();
    if let Some(root) = config_root_object(&parsed) {
        for compiler_options in config_object_properties(&parsed, root)
            .into_iter()
            .filter(|property| property.name == "compilerOptions")
        {
            for property in config_object_properties(&parsed, compiler_options.initializer) {
                if matches!(property.name.as_str(), Some("lib" | "noLib")) {
                    locations.push(config_location(&parsed, property.name_node));
                }
            }
        }
    }
    let location = locations.into_iter().flatten().next().or_else(|| {
        config_property(&parsed, "compilerOptions")
            .and_then(|property| config_location(&parsed, property.name_node))
    });
    vec![config_diagnostic(
        &gen::Option_0_cannot_be_specified_with_option_1,
        &["lib".to_owned(), "noLib".to_owned()],
        location,
    )]
}

/// Produce the option diagnostics TypeScript 7.1 reports for the options
/// TypeScript 7 removed. tsgo reports them as program diagnostics next to the
/// other option rows and still builds, checks and emits the program, so they
/// stay attached to the config plan as non-fatal rows (see
/// `is_non_fatal_option_diagnostic`). `ignoreDeprecations` is parsed but has
/// no effect in 7.1: it neither silences a row nor is validated.
///
/// tsgo-port: verifyCompilerOptions "Removed in TS7" @7.1 (program.go:976-1033)
fn removed_option_diagnostics(
    options: &ConfigOptionBag,
    source: &ConfigSourceText,
    config_file_name: JsStr<'_>,
    use_case_sensitive_file_names: bool,
) -> Vec<Diagnostic> {
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let compiler_properties = config_compiler_option_properties(&parsed);
    let fallback = config_property(&parsed, "compilerOptions")
        .and_then(|property| config_location(&parsed, property.name_node));
    let mut diagnostics = Vec::new();
    let mut removed = |name: &str, value: Option<&str>, use_instead: Option<JsStr<'_>>| {
        emit_removed_option_diagnostic(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &fallback,
            name,
            value,
            use_instead,
        );
    };

    // Removed in TS7. The typed `baseUrl` value is already absolute.
    if let Some(base_url) = config_option_string(options, "baseUrl") {
        let use_instead = removed_base_url_paths_suggestion(
            config_file_name,
            base_url.as_js(),
            JsStr::from(""),
            use_case_sensitive_file_names,
        );
        removed("baseUrl", None, Some(use_instead.as_js()));
    }
    if config_option_string(options, "outFile").is_some() {
        removed("outFile", None, None);
    }
    if config_option_i32(options, "target") == Some(1) {
        removed("target", Some("ES5"), None);
    }
    let module_name = match config_option_i32(options, "module") {
        Some(2) => Some("AMD"),
        Some(4) => Some("System"),
        Some(3) => Some("UMD"),
        _ => None,
    };
    if let Some(module_name) = module_name {
        removed("module", Some(module_name), None);
    }
    let module_resolution = config_option_i32(options, "moduleResolution");
    if module_resolution == Some(1) {
        removed("moduleResolution", Some("Classic"), None);
    }
    for name in [
        "alwaysStrict",
        "esModuleInterop",
        "allowSyntheticDefaultImports",
    ] {
        if config_option_bool(options, name) == Some(false) {
            removed(name, Some("false"), None);
        }
    }
    if module_resolution == Some(2) {
        removed("moduleResolution", Some("node10"), None);
    }
    if config_option_bool(options, "downlevelIteration").is_some() {
        removed("downlevelIteration", None, None);
    }

    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

/// The `Use '"paths": {"*": ["./…/*"]}' instead.` suggestion TypeScript 7.1
/// attaches to the removed `baseUrl` option when a config file exists: the
/// base URL relative to the config file's directory, quoted as Go's
/// `encoding/json` does.
///
/// tsgo-port: verifyCompilerOptions (baseUrl) @7.1 (program.go:953-966),
/// tspath.GetRelativePathFromFile and tspath.EnsurePathIsNonModuleName
/// (path.go:817-819, 974-979)
pub fn removed_base_url_paths_suggestion(
    config_file_name: JsStr<'_>,
    base_url: JsStr<'_>,
    current_directory: JsStr<'_>,
    use_case_sensitive_file_names: bool,
) -> JsString {
    let base_url = normalized_absolute_path(base_url, current_directory);
    let directory = directory_name(config_file_name);
    let mut relative = relative_path_from_directory(
        directory.as_js(),
        base_url.as_js(),
        use_case_sensitive_file_names,
    );
    if !path_is_absolute(relative.as_js()) && !path_is_relative(relative.as_js()) {
        relative = prefixed_path("./", relative.as_js());
    }
    if !(relative.starts_with("./") || relative.starts_with("../")) {
        relative = prefixed_path("./", relative.as_js());
    }
    let suggestion = combine_paths(relative.as_js(), JsStr::from("*"));
    let mut text = JsString::from("\"paths\": {\"*\": [");
    text.push_str(&go_json_string(suggestion.as_js()));
    text.push_str("]}");
    text
}

fn prefixed_path(prefix: &str, path: JsStr<'_>) -> JsString {
    let mut prefixed = JsString::from(prefix);
    prefixed.push_js(path);
    prefixed
}

/// `encoding/json.Marshal` of a string: Go's default escaping, including the
/// HTML-safe `<`, `>` and `&` forms and `�` for an
/// unpaired surrogate.
fn go_json_string(text: JsStr<'_>) -> String {
    use std::fmt::Write as _;
    let mut quoted = String::from("\"");
    for unit in char::decode_utf16(text.code_units()) {
        match unit {
            Ok('"') => quoted.push_str("\\\""),
            Ok('\\') => quoted.push_str("\\\\"),
            Ok('\n') => quoted.push_str("\\n"),
            Ok('\r') => quoted.push_str("\\r"),
            Ok('\t') => quoted.push_str("\\t"),
            Ok(ch @ ('<' | '>' | '&' | '\u{2028}' | '\u{2029}' | '\0'..='\u{1f}')) => {
                let _ = write!(quoted, "\\u{:04x}", u32::from(ch));
            }
            Ok(ch) => quoted.push(ch),
            Err(_) => quoted.push_str("\\ufffd"),
        }
    }
    quoted.push('"');
    quoted
}

/// Produce option-combination diagnostics which TypeScript evaluates after
/// computing the effective module and module-resolution kinds. Keeping these
/// rows on the immutable config plan prevents an incompatible resolver mode
/// from reaching source discovery, while still allowing the CLI to render the
/// exact option diagnostics before the no-emit gate fails closed.
///
/// tsc-port: verifyCompilerOptions @6.0.3
/// tsc-hash: 379bc580139f96f948f7e041ea76b960282efe6c7924b06a4de1f11bffb9b558
/// tsc-span: _tsc.js:124936-125020
fn option_relationship_diagnostics(
    options: &ConfigOptionBag,
    source: &ConfigSourceText,
) -> Vec<Diagnostic> {
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let compiler_properties = config_compiler_option_properties(&parsed);
    // createCompilerOptionsDiagnostic falls back to the root compilerOptions
    // property name when an effective (possibly inherited) option is absent.
    let fallback = config_property(&parsed, "compilerOptions")
        .and_then(|property| config_location(&parsed, property.name_node));
    let projected = CompilerOptions {
        allow_js: config_option_bool(options, "allowJs")
            .unwrap_or_else(|| config_option_bool(options, "checkJs").unwrap_or(false)),
        allow_js_specified: config_option_bool(options, "allowJs"),
        no_emit: config_option_bool(options, "noEmit"),
        allow_importing_ts_extensions: config_option_bool(options, "allowImportingTsExtensions"),
        rewrite_relative_import_extensions: config_option_bool(
            options,
            "rewriteRelativeImportExtensions",
        ),
        resolve_package_json_exports: config_option_bool(options, "resolvePackageJsonExports"),
        resolve_package_json_imports: config_option_bool(options, "resolvePackageJsonImports"),
        custom_conditions: config_option_string_list(options, "customConditions"),
        check_js: config_option_bool(options, "checkJs"),
        isolated_modules: config_option_bool(options, "isolatedModules"),
        verbatim_module_syntax: config_option_bool(options, "verbatimModuleSyntax"),
        preserve_const_enums: config_option_bool(options, "preserveConstEnums"),
        incremental: config_option_bool(options, "incremental"),
        emit_decorator_metadata: config_option_bool(options, "emitDecoratorMetadata"),
        experimental_decorators: config_option_bool(options, "experimentalDecorators")
            == Some(true),
        experimental_decorators_specified: config_option_bool(options, "experimentalDecorators"),
        target: config_option_i32(options, "target"),
        module: config_option_i32(options, "module"),
        module_resolution: config_option_i32(options, "moduleResolution"),
        resolve_json_module: config_option_bool(options, "resolveJsonModule"),
        strict: config_option_bool(options, "strict"),
        strict_null_checks: config_option_bool(options, "strictNullChecks"),
        strict_property_initialization: config_option_bool(options, "strictPropertyInitialization"),
        exact_optional_property_types: config_option_bool(options, "exactOptionalPropertyTypes"),
        isolated_declarations: config_option_bool(options, "isolatedDeclarations"),
        declaration: config_option_bool(options, "declaration"),
        declaration_dir: config_option_string(options, "declarationDir"),
        out_file: config_option_string(options, "outFile"),
        emit_declaration_only: config_option_bool(options, "emitDeclarationOnly"),
        composite: config_option_bool(options, "composite"),
        jsx: config_option_i32(options, "jsx"),
        source_map: config_option_bool(options, "sourceMap"),
        inline_source_map: config_option_bool(options, "inlineSourceMap"),
        inline_sources: config_option_bool(options, "inlineSources"),
        source_root: config_option_string(options, "sourceRoot"),
        map_root: config_option_string(options, "mapRoot"),
        jsx_factory: config_option_string(options, "jsxFactory"),
        jsx_fragment_factory: config_option_string(options, "jsxFragmentFactory"),
        jsx_import_source: config_option_string(options, "jsxImportSource"),
        react_namespace: config_option_string(options, "reactNamespace"),
        declaration_map: config_option_bool(options, "declarationMap"),
        ..CompilerOptions::default()
    };
    let mut diagnostics = Vec::new();

    for violation in validate_compiler_options(&projected) {
        emit_option_validation_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &fallback,
            &violation,
        );
    }

    diagnostics
}

fn emit_option_validation_diagnostic_for_properties(
    diagnostics: &mut Vec<Diagnostic>,
    source: &SourceFile,
    properties: &[ConfigPropertyNode],
    fallback: &Option<ConfigLocation>,
    violation: &CompilerOptionViolation,
) {
    let message = violation.message();
    let names = violation.option_names();
    let mut locations = properties
        .iter()
        .filter(|property| {
            property
                .name
                .as_str()
                .is_some_and(|name| names.contains(&name))
        })
        .filter_map(|property| {
            config_location(
                source,
                match violation.location() {
                    CompilerOptionValidationLocation::Name => property.name_node,
                    CompilerOptionValidationLocation::Value => property.initializer,
                },
            )
        })
        .collect::<Vec<_>>();
    // tsoptions.ForEachPropertyAssignment (tsconfigparsing.go:1777-1791)
    // stops at the first property, in document order, that one of the
    // violation's names matches: a second name or a duplicate key adds no
    // row. Without a match the row goes to the `compilerOptions` name.
    locations.sort_unstable_by_key(|location| location.start);
    let location = locations.into_iter().next().or_else(|| fallback.clone());
    diagnostics.push(config_diagnostic_from_chain(message, location));
}

fn config_compiler_option_properties(source: &SourceFile) -> Vec<ConfigPropertyNode> {
    let Some(root) = config_root_object(source) else {
        return Vec::new();
    };
    config_object_properties(source, root)
        .into_iter()
        .find(|property| property.name == "compilerOptions")
        .into_iter()
        .flat_map(|property| config_object_properties(source, property.initializer))
        .collect()
}

/// Report a removed option at the first `compilerOptions` property with that
/// name (the property name for a name-only row, the value otherwise), else at
/// the `compilerOptions` property name, else without a location.
///
/// tsgo-port: createRemovedOptionDiagnostic @7.1 (program.go:934-949),
/// createDiagnosticForOption and createOptionDiagnosticInObjectLiteralSyntax
/// (program.go:893-921) over tsoptions.ForEachPropertyAssignment
/// (tsconfigparsing.go:1777-1791), which stops at the first matching property.
fn emit_removed_option_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    source: &SourceFile,
    properties: &[ConfigPropertyNode],
    fallback: &Option<ConfigLocation>,
    name: &str,
    value: Option<&str>,
    use_instead: Option<JsStr<'_>>,
) {
    let mut message = match value {
        Some(value) => MessageChain::new(
            &gen::Option_0_1_has_been_removed_Please_remove_it_from_your_configuration,
            &[name.to_owned(), value.to_owned()],
        ),
        None => MessageChain::new(
            &gen::Option_0_has_been_removed_Please_remove_it_from_your_configuration,
            &[name.to_owned()],
        ),
    };
    if let Some(use_instead) = use_instead {
        message = message.with_next(vec![MessageChain::new_js(
            &gen::Use_0_instead,
            &[use_instead.to_owned()],
        )]);
    }
    let location = properties
        .iter()
        .find(|property| property.name == name)
        .and_then(|property| {
            config_location(
                source,
                if value.is_some() {
                    property.initializer
                } else {
                    property.name_node
                },
            )
        })
        .or_else(|| fallback.clone());
    diagnostics.push(config_diagnostic_from_chain(message, location));
}

fn pending_paths_option_violations(
    options: &ConfigOptionBag,
) -> Vec<PendingConfigPathsViolation<'_>> {
    let Some(paths) = options.typed_object_value("paths") else {
        return Vec::new();
    };
    let mut violations = Vec::new();
    for mapping in paths.properties() {
        let key = mapping.name();
        if !has_zero_or_one_asterisk(key) {
            violations.push(PendingConfigPathsViolation {
                key,
                target: ConfigPathsDiagnosticLocation::Key,
                kind: PathsOptionViolationKind::PatternHasMultipleAsterisks {
                    pattern: key.to_owned(),
                },
            });
        }
        let Some(ConfigTypedJsonValue::Array(substitutions)) = mapping.value() else {
            violations.push(PendingConfigPathsViolation {
                key,
                target: ConfigPathsDiagnosticLocation::Value,
                kind: PathsOptionViolationKind::SubstitutionsNotArray {
                    pattern: key.to_owned(),
                },
            });
            continue;
        };
        // tsgo converts the substitutions to a string list first, so a list
        // whose elements are all non-strings is an empty one (TS5066) and a
        // non-string element reports nothing on its own.
        if !substitutions.iter().any(|substitution| {
            matches!(substitution, ConfigTypedJsonValue::Json(Value::String(_)))
        }) {
            violations.push(PendingConfigPathsViolation {
                key,
                target: ConfigPathsDiagnosticLocation::Value,
                kind: PathsOptionViolationKind::EmptySubstitutions {
                    pattern: key.to_owned(),
                },
            });
        }
        for (index, substitution) in substitutions.iter().enumerate() {
            if let ConfigTypedJsonValue::Json(Value::String(substitution)) = substitution {
                if !has_zero_or_one_asterisk(substitution) {
                    violations.push(PendingConfigPathsViolation {
                        key,
                        target: ConfigPathsDiagnosticLocation::Element(index),
                        kind: PathsOptionViolationKind::SubstitutionHasMultipleAsterisks {
                            pattern: key.to_owned(),
                            substitution: substitution.clone(),
                        },
                    });
                }
                if !path_is_relative(substitution) && !path_is_absolute(substitution) {
                    violations.push(PendingConfigPathsViolation {
                        key,
                        target: ConfigPathsDiagnosticLocation::Element(index),
                        kind: PathsOptionViolationKind::NonRelativeSubstitutionWithoutBaseUrl,
                    });
                }
            }
        }
    }
    violations
}

fn push_paths_violations(
    violations: &mut Vec<PathsOptionViolation>,
    syntax: &ConfigPathsSyntaxIndex,
    key: JsStr<'_>,
    target: ConfigPathsDiagnosticLocation,
    kind: PathsOptionViolationKind,
) {
    let key_syntax = syntax.mapping_locations.get(key.as_bytes());
    match target {
        ConfigPathsDiagnosticLocation::Key | ConfigPathsDiagnosticLocation::Value => {
            if let Some(key_syntax) = key_syntax {
                for mapping in &key_syntax.mapping_locations {
                    let span = match target {
                        ConfigPathsDiagnosticLocation::Key => mapping.key_location,
                        ConfigPathsDiagnosticLocation::Value => mapping.value_location,
                        ConfigPathsDiagnosticLocation::Element(_) => unreachable!(),
                    };
                    violations.push(PathsOptionViolation::new(
                        kind.clone(),
                        config_paths_violation_location(syntax, span),
                    ));
                }
                return;
            }
        }
        ConfigPathsDiagnosticLocation::Element(index) => {
            if let Some(locations) =
                key_syntax.and_then(|locations| locations.element_locations.get(&index))
            {
                for &span in locations {
                    violations.push(PathsOptionViolation::new(
                        kind.clone(),
                        config_paths_violation_location(syntax, span),
                    ));
                }
                return;
            }
        }
    }
    violations.push(PathsOptionViolation::new(
        kind,
        syntax
            .compiler_options_name
            .and_then(|span| config_paths_violation_location(syntax, span)),
    ));
}

fn config_paths_violation_location(
    syntax: &ConfigPathsSyntaxIndex,
    span: ConfigSpan,
) -> Option<PathsOptionDiagnosticLocation> {
    Some(PathsOptionDiagnosticLocation::new(
        syntax.file_name.clone()?,
        ProgramConfigSpan::new(span.start, span.length),
    ))
}

fn config_location(source: &SourceFile, node: NodeId) -> Option<ConfigLocation> {
    let span = config_span(source, node)?;
    Some(ConfigLocation {
        file_name: source.file_name.clone(),
        start: span.start,
        length: span.length,
    })
}

fn config_span(source: &SourceFile, node: NodeId) -> Option<ConfigSpan> {
    let node = source.arena.node(node);
    let end_byte = usize::try_from(node.end).ok()?.min(source.text().len());
    let start_byte = tsc_syntax::skip_trivia(source.text(), node.pos as usize).min(end_byte);
    let start = source.positions().byte_to_utf16(start_byte as u32)?;
    let end = source.positions().byte_to_utf16(end_byte as u32)?;
    Some(ConfigSpan {
        start,
        length: end.saturating_sub(start),
    })
}

/// tsgo `convertConfigFileToObject`'s conversion diagnostics: the root
/// object (or the first object of a root array, which tsgo converts in its
/// place) converted against the tsconfig root options.
fn config_json_conversion_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
    let Some(root) = config_root_object(source) else {
        return Vec::new();
    };
    config_json_conversion_diagnostics_from_root(source, root, Some(JsonConversionOption::Root))
}

/// tsgo parseConfig's cycle arm: `convertToObject` converts the root
/// expression without a notifier, so no option applies. TS5092 itself is
/// not emitted a second time.
fn config_json_cycle_conversion_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
    let Some(root) = config_root_expression(source) else {
        return Vec::new();
    };
    config_json_conversion_diagnostics_from_root(source, root, None)
}

/// The option tsgo's `convertToJson` passes down with a value (a
/// `CommandLineOption`): it names a value's TS5024 and holds the options an
/// object value's properties look up (`ElementOptions`).
#[derive(Clone, Copy)]
enum JsonConversionOption {
    /// `tsconfigRootOptionsMap`.
    Root,
    /// `compilerOptions` or `typeAcquisition`.
    Group(ConfigOptionGroup),
    /// Any other option: its name and `getCompilerOptionValueTypeString`.
    Declared {
        name: &'static str,
        value_type: &'static str,
    },
}

impl JsonConversionOption {
    fn name(self) -> &'static str {
        match self {
            Self::Root => "undefined",
            Self::Group(group) => group.name(),
            Self::Declared { name, .. } => name,
        }
    }

    /// tsgo `getCompilerOptionValueTypeString`.
    fn value_type(self) -> &'static str {
        match self {
            Self::Root | Self::Group(_) => "object",
            Self::Declared { value_type, .. } => value_type,
        }
    }

    /// The option of an object value's property `key` (tsgo
    /// `ElementOptions.Get` with its exact-name check). The root options
    /// have no `watchOptions`, and no other option has element options.
    fn element(self, key: &JsString) -> Option<Self> {
        let declared = |name, value_type| Some(Self::Declared { name, value_type });
        match self {
            Self::Root => match key.as_str()? {
                "compilerOptions" => Some(Self::Group(ConfigOptionGroup::Compiler)),
                "typeAcquisition" => Some(Self::Group(ConfigOptionGroup::Acquisition)),
                "extends" => declared("extends", "string or Array"),
                "references" => declared("references", "Array"),
                "contentMappers" => declared("contentMappers", "Array"),
                "files" => declared("files", "Array"),
                "include" => declared("include", "Array"),
                "exclude" => declared("exclude", "Array"),
                "compileOnSave" => declared("compileOnSave", "boolean"),
                _ => None,
            },
            Self::Group(group) => group.declaration(key.as_js()).and_then(|declaration| {
                declared(
                    declaration.name(),
                    compiler_option_expected_type(*declaration),
                )
            }),
            Self::Declared { .. } => None,
        }
    }
}

enum ConfigJsonConversionTask {
    Visit {
        node: NodeId,
        option: Option<JsonConversionOption>,
    },
    Report(Box<Diagnostic>),
}

/// tsgo `convertPropertyValueToJson`'s diagnostics, at each node's whole
/// range (leading trivia included): a member that is not a property
/// assignment is TS1136, a `?` is TS8009, and a value JSON has no form for
/// is TS5024 against the value's option, or TS1328 without one. An array's
/// elements are converted with the array's own option. The parser reports
/// the source's own JSON diagnostics (TS1327, TS1328, TS1136) separately.
fn config_json_conversion_diagnostics_from_root(
    source: &SourceFile,
    root: NodeId,
    option: Option<JsonConversionOption>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut tasks = vec![ConfigJsonConversionTask::Visit { node: root, option }];
    while let Some(task) = tasks.pop() {
        let (node_id, option) = match task {
            ConfigJsonConversionTask::Report(diagnostic) => {
                diagnostics.push(*diagnostic);
                continue;
            }
            ConfigJsonConversionTask::Visit { node, option } => (node, option),
        };
        let node = source.arena.node(node_id);
        match node.kind {
            SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::NumericLiteral
            | SyntaxKind::StringLiteral => {}
            SyntaxKind::PrefixUnaryExpression if is_negated_json_number(source, node_id) => {}
            SyntaxKind::ObjectLiteralExpression => {
                let members = node
                    .data
                    .as_object_literal_expression()
                    .and_then(|object| object.properties)
                    .map(|members| source.arena.node_array(members).nodes.to_vec())
                    .unwrap_or_default();
                // Pushed last to first, so each member's diagnostics come out
                // in source order.
                for member in members.into_iter().rev() {
                    let Some(property) = source.arena.node(member).data.as_property_assignment()
                    else {
                        tasks.push(ConfigJsonConversionTask::Report(Box::new(config_diagnostic(
                            &gen::Property_assignment_expected,
                            &[] as &[String],
                            config_raw_location(source, member),
                        ))));
                        continue;
                    };
                    if let Some(initializer) = property.initializer {
                        let element = property
                            .name
                            .and_then(|name| config_property_name(source, name))
                            .and_then(|key| option.and_then(|option| option.element(&key)));
                        tasks.push(ConfigJsonConversionTask::Visit {
                            node: initializer,
                            option: element,
                        });
                    }
                    if let Some(question) = property.question_token {
                        tasks.push(ConfigJsonConversionTask::Report(Box::new(config_diagnostic(
                            &gen::The_0_modifier_can_only_be_used_in_TypeScript_files,
                            &["?"],
                            config_raw_location(source, question),
                        ))));
                    }
                }
            }
            SyntaxKind::ArrayLiteralExpression => {
                let elements = config_array_elements(source, node_id);
                tasks.extend(
                    elements
                        .into_iter()
                        .rev()
                        .map(|node| ConfigJsonConversionTask::Visit { node, option }),
                );
            }
            _ => diagnostics.push(match option {
                Some(option) => config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[option.name(), option.value_type()],
                    config_raw_location(source, node_id),
                ),
                None => config_diagnostic(
                    &gen::Property_value_can_only_be_string_literal_numeric_literal_true_false_null_object_literal_or_array_literal,
                    &[] as &[String],
                    config_raw_location(source, node_id),
                ),
            }),
        }
    }
    diagnostics
}

/// `-` applied to a numeric literal, the one prefix expression JSON has.
fn is_negated_json_number(source: &SourceFile, node: NodeId) -> bool {
    source
        .arena
        .node(node)
        .data
        .as_prefix_unary_expression()
        .filter(|unary| unary.operator == SyntaxKind::MinusToken)
        .and_then(|unary| unary.operand)
        .is_some_and(|operand| source.arena.node(operand).kind == SyntaxKind::NumericLiteral)
}

/// A node's whole range, leading trivia included: tsgo's `convertToJson`
/// reports at a node's `Loc` (`ast.NewDiagnostic`), where a diagnostic about
/// a node otherwise starts at its first token.
fn config_raw_location(source: &SourceFile, node: NodeId) -> Option<ConfigLocation> {
    let node = source.arena.node(node);
    let end_byte = usize::try_from(node.end).ok()?.min(source.text().len());
    let start_byte = (node.pos as usize).min(end_byte);
    let start = source.positions().byte_to_utf16(start_byte as u32)?;
    let end = source.positions().byte_to_utf16(end_byte as u32)?;
    Some(ConfigLocation {
        file_name: source.file_name.clone(),
        start,
        length: end.saturating_sub(start),
    })
}

fn config_root_expression(source: &SourceFile) -> Option<NodeId> {
    let source_file = source.arena.node(source.root).data.as_source_file()?;
    let statements = &source.arena.node_array(source_file.statements?).nodes;
    let statement = *statements.first()?;
    source
        .arena
        .node(statement)
        .data
        .as_expression_statement()?
        .expression
}

fn config_property_initializer(source: &SourceFile, name: &str) -> Option<NodeId> {
    config_property(source, name).map(|property| property.initializer)
}

fn config_property(source: &SourceFile, name: &str) -> Option<ConfigPropertyNode> {
    let root = config_root_expression(source)?;
    if source.arena.node(root).kind != SyntaxKind::ObjectLiteralExpression {
        return None;
    }
    config_object_properties(source, root)
        .into_iter()
        .find(|property| property.name == name)
}

fn config_root_object(source: &SourceFile) -> Option<NodeId> {
    let root = config_root_expression(source)?;
    if source.arena.node(root).kind == SyntaxKind::ObjectLiteralExpression {
        return Some(root);
    }
    config_array_elements(source, root)
        .into_iter()
        .find(|element| source.arena.node(*element).kind == SyntaxKind::ObjectLiteralExpression)
}

fn config_object_properties(source: &SourceFile, object: NodeId) -> Vec<ConfigPropertyNode> {
    let Some(properties) = source
        .arena
        .node(object)
        .data
        .as_object_literal_expression()
        .and_then(|object| object.properties)
    else {
        return Vec::new();
    };
    source
        .arena
        .node_array(properties)
        .nodes
        .iter()
        .filter_map(|property| {
            let property = source.arena.node(*property).data.as_property_assignment()?;
            let name = config_property_name(source, property.name?)?;
            Some(ConfigPropertyNode {
                name,
                name_node: property.name?,
                initializer: property.initializer?,
            })
        })
        .collect()
}

/// tsgo TryGetTextOfPropertyName: a string, numeric or identifier name, or
/// a computed name whose expression is a string or numeric literal.
fn config_property_name(source: &SourceFile, name: NodeId) -> Option<JsString> {
    let node = source.arena.node(name);
    match node.kind {
        SyntaxKind::StringLiteral => node
            .data
            .as_string_literal()
            .map(|literal| literal.text.clone()),
        SyntaxKind::Identifier => node
            .data
            .as_identifier()
            .map(|identifier| identifier.text().to_owned().into()),
        SyntaxKind::NumericLiteral => node
            .data
            .as_numeric_literal()
            .map(|literal| literal.text.clone().into()),
        SyntaxKind::ComputedPropertyName => node
            .data
            .as_computed_property_name()
            .and_then(|computed| computed.expression)
            .filter(|expression| {
                matches!(
                    source.arena.node(*expression).kind,
                    SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral
                )
            })
            .and_then(|expression| config_property_name(source, expression)),
        _ => None,
    }
}

fn config_array_elements(source: &SourceFile, array: NodeId) -> Vec<NodeId> {
    source
        .arena
        .node(array)
        .data
        .as_array_literal_expression()
        .and_then(|array| array.elements)
        .map(|elements| source.arena.node_array(elements).nodes.to_vec())
        .unwrap_or_default()
}

/// The source locations of the string elements of every root property
/// named `name`, keyed by element text. The first element with a text
/// wins, exactly as the former per-spec search did, but the root object is
/// walked once per property instead of once per spec (quadratic in the
/// `files` count).
fn config_spec_locations(
    source: &SourceFile,
    name: &str,
) -> rustc_hash::FxHashMap<JsString, Option<ConfigLocation>> {
    let mut locations = rustc_hash::FxHashMap::default();
    let Some(root) = config_root_expression(source) else {
        return locations;
    };
    if source.arena.node(root).kind != SyntaxKind::ObjectLiteralExpression {
        return locations;
    }
    for property in config_object_properties(source, root) {
        if property.name != name {
            continue;
        }
        for element in config_array_elements(source, property.initializer) {
            if let Some(RecoverableJsonValue::Defined(Value::String(written))) =
                convert_recoverable_json_node_to_value(source, element)
            {
                locations
                    .entry(written)
                    .or_insert_with(|| config_location(source, element));
            }
        }
    }
    locations
}

fn config_raw_projection(value: Value) -> Value {
    match value {
        Value::Array(values) => {
            Value::Array(values.into_iter().map(config_raw_projection).collect())
        }
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .filter_map(|(name, value)| {
                    decode_user_object_key(&name)
                        .map(|name| (name.to_owned(), config_raw_projection(value)))
                })
                .collect(),
        ),
        value => value,
    }
}

/// `nodeNextJsonConfigResolver` accepts the narrower ModuleResolutionHost
/// shape where directory and realpath observations are optional. Returning
/// optimistic directory existence and lexical realpaths reproduces the
/// absence of those optional callbacks while all file bytes still flow
/// through the caller-supplied config host.
struct ConfigCompilerHostAdapter<'a> {
    host: &'a dyn ConfigParseHost,
    current_directory: JsStr<'a>,
}

impl CompilerHost for ConfigCompilerHostAdapter<'_> {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.current_directory
            .as_str()
            .map(PathBuf::from)
            .ok_or_else(|| {
                HostError::new_js(
                    HostErrorKind::InvalidData,
                    HostOperation::CurrentDirectory,
                    Some(self.current_directory),
                    "the native current-directory callback cannot represent a non-scalar JS path",
                )
            })
    }

    fn current_directory_js(&self) -> Result<JsString, HostError> {
        Ok(self.current_directory.to_owned())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.read_file_js(native_config_query(path, HostOperation::ReadFile)?)
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        self.host
            .read_file(path)
            .map(|text| text.map(String::into_bytes))
            .map_err(config_host_error_for_resolver)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.file_exists_js(native_config_query(path, HostOperation::FileExists)?)
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        self.host
            .file_exists(path)
            .map_err(config_host_error_for_resolver)
    }

    fn directory_exists(&self, _path: &Path) -> Result<bool, HostError> {
        Ok(true)
    }
    fn directory_exists_js(&self, _path: JsStr<'_>) -> Result<bool, HostError> {
        Ok(true)
    }
    fn read_directory(&self, _path: &Path) -> Result<Vec<PathBuf>, HostError> {
        Ok(Vec::new())
    }
    fn read_directory_js(&self, _path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        Ok(Vec::new())
    }
    fn get_directories_js(&self, _path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        Ok(Vec::new())
    }
    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        let query = native_config_query(path, HostOperation::Realpath)?;
        Ok(self
            .realpath_js(query)?
            .map(|real| PathBuf::from(real.to_string_lossy().into_owned())))
    }
    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        Ok(Some(
            self.host
                .realpath(path)
                .map_err(config_host_error_for_resolver)?
                .unwrap_or_else(|| path.to_owned()),
        ))
    }
}

fn native_config_query(path: &Path, operation: HostOperation) -> Result<JsStr<'_>, HostError> {
    path.to_str().map(JsStr::from_str).ok_or_else(|| {
        HostError::new(
            HostErrorKind::InvalidInput,
            operation,
            Some(path.to_owned()),
            "path is not valid Unicode",
        )
    })
}

fn config_host_error_for_resolver(error: ConfigHostError) -> HostError {
    let operation = match error.operation() {
        ConfigHostOperation::FileExists => HostOperation::FileExists,
        ConfigHostOperation::ReadFile => HostOperation::ReadFile,
        ConfigHostOperation::ReadDirectory => HostOperation::ReadDirectory,
        ConfigHostOperation::Realpath => HostOperation::Realpath,
    };
    HostError::new_js(
        HostErrorKind::Other,
        operation,
        Some(error.path()),
        error.detail().to_owned(),
    )
}

fn config_error_from_resolution(error: ResolutionError) -> ConfigParseError {
    match error {
        ResolutionError::Host(error) => {
            let operation = match error.operation() {
                HostOperation::FileExists => Some(ConfigHostOperation::FileExists),
                HostOperation::ReadFile => Some(ConfigHostOperation::ReadFile),
                HostOperation::ReadDirectory => Some(ConfigHostOperation::ReadDirectory),
                HostOperation::Realpath => Some(ConfigHostOperation::Realpath),
                _ => None,
            };
            if let Some(operation) = operation {
                return ConfigHostError::new(
                    operation,
                    error.js_path().map(JsStr::to_owned).unwrap_or_else(|| {
                        error
                            .path()
                            .map_or_else(JsString::new, |path| path.display().to_string().into())
                    }),
                    error.detail().to_owned(),
                )
                .into();
            }
            ConfigParseError::new_js(
                ConfigParseErrorKind::Host,
                error
                    .js_path()
                    .map(JsStr::to_owned)
                    .or_else(|| error.path().map(|path| path.display().to_string().into())),
                error.to_string(),
            )
        }
        ResolutionError::Unsupported { feature, detail } => ConfigParseError::new_js(
            ConfigParseErrorKind::Unsupported,
            None,
            format!("unsupported config resolution feature {feature}: {detail}"),
        ),
        ResolutionError::Canonicalization {
            path,
            js_path,
            detail,
        } => ConfigParseError::new_js(
            ConfigParseErrorKind::InvalidPath,
            js_path.or_else(|| path.map(|path| path.display().to_string().into())),
            detail,
        ),
        ResolutionError::InvalidData(detail) => {
            ConfigParseError::new_js(ConfigParseErrorKind::InvalidConfig, None, detail)
        }
        ResolutionError::ResourceLimit(detail) => {
            ConfigParseError::new_js(ConfigParseErrorKind::ResourceLimit, None, detail)
        }
    }
}

/// A config's root file names and the specs they were matched with.
struct DerivedFileNames {
    file_names: Vec<JsString>,
    include_specs: Vec<JsString>,
    exclude_specs: Option<Vec<JsString>>,
}

fn derive_file_names<'j0, 'j1>(
    host: &dyn ConfigParseHost,
    config: &ParsedConfigNode,
    base_path: impl Into<JsStr<'j0>>,
    config_file_name: impl Into<JsStr<'j1>>,
    discovery_options: &ConfigDiscoveryOptions,
    errors: &mut Vec<Diagnostic>,
) -> Result<DerivedFileNames, ConfigParseError> {
    let base_path = base_path.into();
    let config_file_name = config_file_name.into();
    let case_sensitive = host.use_case_sensitive_file_names();
    let mut literal = Vec::<(JsString, JsString)>::new();
    if let Some(files) = &config.files {
        // Same map semantics as before (a repeated key replaces the value in
        // place and keeps its first position), indexed instead of scanned.
        let mut literal_index: rustc_hash::FxHashMap<JsString, usize> =
            rustc_hash::FxHashMap::with_capacity_and_hasher(files.len(), Default::default());
        for file in files {
            let normalized = normalized_spec_path(file, base_path)?;
            let key = file_name_key(normalized.as_js(), case_sensitive);
            match literal_index.get(&key) {
                Some(&position) => literal[position].1 = normalized,
                None => {
                    literal_index.insert(key.clone(), literal.len());
                    literal.push((key, normalized));
                }
            }
        }
    }

    let include = match &config.include {
        Some(include) => include.clone(),
        None if config.files.is_none() => vec![ConfigSpec {
            text: "**/*".into(),
            base_path: base_path.to_owned(),
            location: None,
        }],
        None => Vec::new(),
    };
    report_empty_files(config, config_file_name, errors);
    let include = validate_config_specs(
        &include, /* disallow_trailing_recursion */ true, errors,
    );
    let exclude = config.exclude.as_ref().map(|exclude| {
        validate_config_specs(
            exclude, /* disallow_trailing_recursion */ false, errors,
        )
    });
    let include_values = include
        .iter()
        .map(|spec| config_host_spec(spec, base_path))
        .collect::<Result<Vec<_>, _>>()?;
    let exclude_values = if let Some(exclude) = &exclude {
        Some(
            exclude
                .iter()
                .map(|spec| config_host_spec(spec, base_path))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else if config
        .raw
        .as_object()
        .and_then(|raw| raw.get("exclude"))
        .is_none_or(Value::is_null)
    {
        let defaults = [
            discovery_options.out_dir.clone(),
            discovery_options.declaration_dir.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        (!defaults.is_empty()).then_some(defaults)
    } else {
        None
    };

    let extension_groups = if discovery_options.allow_js {
        ALL_EXTENSIONS
    } else {
        TYPESCRIPT_EXTENSIONS
    };
    let mut flat_extensions = extension_groups
        .iter()
        .flat_map(|group| group.iter().copied())
        .collect::<Vec<_>>();
    if discovery_options.resolve_json_module {
        flat_extensions.push(".json");
    }
    let wildcard_candidates = if include_values.is_empty() {
        Vec::new()
    } else {
        host.read_directory(
            base_path,
            &flat_extensions,
            exclude_values.as_deref(),
            Some(include_values.as_slice()),
            None,
        )?
    };
    let json_include_patterns = include_values
        .iter()
        .filter(|include| include.ends_with(".json"))
        .map(|include| {
            ConfigFilePattern::new(include, base_path, case_sensitive).map_err(|detail| {
                ConfigParseError::new_js(
                    ConfigParseErrorKind::InvalidPath,
                    Some(include.clone()),
                    detail,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    // 39611-39613: literalFileMap / wildcardFileMap / wildCardJsonFileMap
    // are Maps keyed by the canonical name, so each candidate costs one
    // lookup. Scanning the wildcard list instead was quadratic in the
    // candidate count (VS Code's 9,400 sources: ~90 ms of key compares).
    let literal_keys = literal
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<rustc_hash::FxHashSet<_>>();
    let mut wildcard = OrderedFileMap::default();
    let mut wildcard_json = OrderedFileMap::default();
    for file in wildcard_candidates {
        if file_extension_is(&file, ".json") {
            if discovery_options.resolve_json_module
                && json_include_patterns
                    .iter()
                    .any(|include| include.matches(&file))
            {
                let key = file_name_key(file.as_js(), case_sensitive);
                if !literal_keys.contains(&key) && !wildcard_json.contains(&key) {
                    wildcard_json.insert(key, file);
                }
            }
            continue;
        }
        if has_higher_priority(
            file.as_js(),
            &literal_keys,
            &wildcard,
            extension_groups,
            case_sensitive,
        ) {
            continue;
        }
        remove_lower_priority(
            file.as_js(),
            &mut wildcard,
            extension_groups,
            case_sensitive,
        );
        let key = file_name_key(file.as_js(), case_sensitive);
        if !literal_keys.contains(&key) && !wildcard.contains(&key) {
            wildcard.insert(key, file);
        }
    }

    let file_names = literal
        .into_iter()
        .map(|(_, file)| file)
        .chain(wildcard.into_files())
        .chain(wildcard_json.into_files())
        .collect::<Vec<_>>();
    report_no_input_files(
        config,
        config_file_name,
        &file_names,
        exclude_values.as_deref(),
        errors,
    );
    Ok(DerivedFileNames {
        file_names,
        include_specs: include_values,
        exclude_specs: exclude_values,
    })
}

fn validate_config_specs(
    specs: &[ConfigSpec],
    disallow_trailing_recursion: bool,
    errors: &mut Vec<Diagnostic>,
) -> Vec<ConfigSpec> {
    let mut validated = Vec::with_capacity(specs.len());
    for spec in specs {
        if disallow_trailing_recursion && invalid_trailing_recursion_pattern(&spec.text) {
            errors.push(config_diagnostic(
                &gen::File_specification_cannot_end_in_a_recursive_directory_wildcard_0,
                std::slice::from_ref(&spec.text),
                spec.location.clone(),
            ));
            continue;
        }
        if invalid_dot_dot_after_recursive_wildcard(&spec.text) {
            errors.push(config_diagnostic(
                &gen::File_specification_cannot_contain_a_parent_directory_that_appears_after_a_recursive_directory_wildcard_0,
                std::slice::from_ref(&spec.text),
                spec.location.clone(),
            ));
            continue;
        }
        validated.push(spec.clone());
    }
    validated
}

fn invalid_trailing_recursion_pattern<'s>(spec: impl Into<JsStr<'s>>) -> bool {
    let spec = spec.into();
    let candidate = spec.strip_suffix("/").unwrap_or(spec);
    candidate == "**" || candidate.ends_with("/**")
}

pub(crate) fn invalid_dot_dot_after_recursive_wildcard<'s>(spec: impl Into<JsStr<'s>>) -> bool {
    let spec = spec.into();
    let bytes = spec.as_bytes();
    let wildcard_index = if spec.starts_with("**/") {
        Some(0)
    } else {
        bytes.windows(4).position(|window| window == b"/**/")
    };
    let Some(wildcard_index) = wildcard_index else {
        return false;
    };
    let last_dot_index = if spec.ends_with("/..") {
        Some(bytes.len())
    } else {
        bytes.windows(4).rposition(|window| window == b"/../")
    };
    last_dot_index.is_some_and(|index| index > wildcard_index)
}

fn report_empty_files<'j0>(
    config: &ParsedConfigNode,
    config_file_name: impl Into<JsStr<'j0>>,
    errors: &mut Vec<Diagnostic>,
) {
    let config_file_name = config_file_name.into();
    let Some(raw) = config.raw.as_object() else {
        return;
    };
    let files_are_empty = raw
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    // tsgo getPropFromRaw (with a source file): only an array value is
    // read; any other value counts as no property. `extends` counts by its
    // value (`GetOrZero("extends") == nil`), so a null or missing value
    // does not suppress the diagnostic.
    let references_are_zero_or_absent = match raw.get("references") {
        Some(Value::Array(references)) => references.is_empty(),
        _ => true,
    };
    if files_are_empty
        && references_are_zero_or_absent
        && raw.get("extends").is_none_or(Value::is_null)
    {
        errors.push(config_diagnostic(
            &gen::The_files_list_in_config_file_0_is_empty,
            &[config_file_name.to_owned()],
            config.files_location.clone(),
        ));
    }
}

fn report_no_input_files<'j0>(
    config: &ParsedConfigNode,
    config_file_name: impl Into<JsStr<'j0>>,
    file_names: &[JsString],
    effective_excludes: Option<&[JsString]>,
    errors: &mut Vec<Diagnostic>,
) {
    let config_file_name = config_file_name.into();
    let Some(raw) = config.raw.as_object() else {
        return;
    };
    if !file_names.is_empty()
        || config.raw_property_names.contains("files".as_bytes())
        || config.raw_property_names.contains("references".as_bytes())
    {
        return;
    }

    let include = raw
        .get("include")
        .filter(|value| value.is_array() && !is_nil_json_array(value))
        .map(without_json_nulls)
        .unwrap_or_else(|| Value::Array(vec![Value::String("**/*".into())]));
    let exclude = raw
        .get("exclude")
        .filter(|value| value.is_array() && !is_nil_json_array(value))
        .map(without_json_nulls)
        .unwrap_or_else(|| {
            Value::Array(
                effective_excludes
                    .unwrap_or(&[])
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            )
        });
    let include = javascript_json_stringify(&include);
    let exclude = javascript_json_stringify(&exclude);
    errors.push(config_diagnostic(
        &gen::No_inputs_were_found_in_config_file_0_Specified_include_paths_were_1_and_exclude_paths_were_2,
        &[config_file_name.to_owned(), include.into(), exclude.into()],
        None,
    ));
}

/// tsgo's JSON conversion keeps the non-null elements of an array
/// (`convertArrayLiteralExpressionToJson`): an array literal of nulls only
/// becomes a nil slice, which the spec readers take for an absent value.
fn is_nil_json_array(value: &Value) -> bool {
    matches!(value, Value::Array(values) if !values.is_empty() && values.iter().all(Value::is_null))
}

/// An array as tsgo's JSON conversion leaves it: without its nulls.
fn without_json_nulls(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(
            values
                .iter()
                .filter(|value| !value.is_null())
                .cloned()
                .collect(),
        ),
        other => other.clone(),
    }
}

fn javascript_json_stringify(value: &Value) -> String {
    let mut result = String::new();
    append_javascript_json(value, &mut result);
    result
}

fn append_javascript_json(value: &Value, result: &mut String) {
    match value {
        Value::Null => result.push_str("null"),
        Value::Bool(value) => result.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            let value = json_number_as_f64(number)
                .expect("config JSON numbers have a JavaScript numeric projection");
            if value.is_finite() {
                result.push_str(&js_number_to_string(value));
            } else {
                result.push_str("null");
            }
        }
        Value::String(value) => crate::append_json_quoted(value.as_js(), result),
        Value::Array(values) => {
            result.push('[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    result.push(',');
                }
                append_javascript_json(value, result);
            }
            result.push(']');
        }
        Value::Object(object) => {
            result.push('{');
            let mut indexed = object
                .iter()
                .filter_map(|(name, value)| {
                    javascript_array_index(name).map(|index| (index, name, value))
                })
                .collect::<Vec<_>>();
            indexed.sort_by_key(|(index, _, _)| *index);
            let mut first = true;
            for (name, value) in indexed
                .into_iter()
                .map(|(_, name, value)| (name, value))
                .chain(
                    object
                        .iter()
                        .filter(|(name, _)| javascript_array_index(*name).is_none()),
                )
            {
                if !first {
                    result.push(',');
                }
                first = false;
                crate::append_json_quoted(name.as_js(), result);
                result.push(':');
                append_javascript_json(value, result);
            }
            result.push('}');
        }
    }
}

fn javascript_array_index<'n>(name: impl Into<JsStr<'n>>) -> Option<u32> {
    // Object index grammar is canonical ASCII decimal. Non-scalar names are
    // ordinary string properties and keep their complete spelling elsewhere.
    let name = name.into().as_str()?;
    let index = name.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == name).then_some(index)
}

pub(crate) fn effective_discovery_options<'j0>(
    options: &ConfigOptionBag,
    config_base_path: impl Into<JsStr<'j0>>,
) -> Result<ConfigDiscoveryOptions, ConfigParseError> {
    let config_base_path = config_base_path.into();
    let allow_js = options
        .typed_value("allowJs")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            options
                .typed_value("checkJs")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        });
    let resolve_json_module = options
        .typed_value("resolveJsonModule")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| computed_resolve_json_module(options));
    Ok(ConfigDiscoveryOptions {
        allow_js,
        resolve_json_module,
        out_dir: normalized_option_path(options, "outDir", config_base_path)?,
        declaration_dir: normalized_option_path(options, "declarationDir", config_base_path)?,
    })
}

/// The compiler options a config's (or the command line's) option bag
/// names, with the discovery options already derived from it.
pub(crate) fn bag_compiler_options(
    options: &ConfigOptionBag,
    discovery: &ConfigDiscoveryOptions,
) -> CompilerOptions {
    CompilerOptions {
        allow_js: discovery.allow_js,
        allow_js_specified: config_option_bool(options, "allowJs"),
        force_consistent_casing_in_file_names: config_option_bool(
            options,
            "forceConsistentCasingInFileNames",
        ),
        max_node_module_js_depth: config_option_number(options, "maxNodeModuleJsDepth"),
        experimental_decorators: config_option_bool(options, "experimentalDecorators")
            .unwrap_or(false),
        experimental_decorators_specified: config_option_bool(options, "experimentalDecorators"),
        target: config_option_i32(options, "target"),
        module: config_option_i32(options, "module"),
        module_detection: config_option_i32(options, "moduleDetection"),
        always_strict: config_option_bool(options, "alwaysStrict"),
        strict: config_option_bool(options, "strict"),
        strict_null_checks: config_option_bool(options, "strictNullChecks"),
        strict_function_types: config_option_bool(options, "strictFunctionTypes"),
        no_implicit_any: config_option_bool(options, "noImplicitAny"),
        no_error_truncation: config_option_bool(options, "noErrorTruncation"),
        no_implicit_this: config_option_bool(options, "noImplicitThis"),
        no_implicit_override: config_option_bool(options, "noImplicitOverride"),
        strict_bind_call_apply: config_option_bool(options, "strictBindCallApply"),
        exact_optional_property_types: config_option_bool(options, "exactOptionalPropertyTypes"),
        no_fallthrough_cases_in_switch: config_option_bool(options, "noFallthroughCasesInSwitch"),
        no_implicit_returns: config_option_bool(options, "noImplicitReturns"),
        no_unused_locals: config_option_bool(options, "noUnusedLocals"),
        no_unused_parameters: config_option_bool(options, "noUnusedParameters"),
        allow_unreachable_code: config_option_bool(options, "allowUnreachableCode"),
        allow_unused_labels: config_option_bool(options, "allowUnusedLabels"),
        check_js: config_option_bool(options, "checkJs"),
        no_unchecked_indexed_access: config_option_bool(options, "noUncheckedIndexedAccess"),
        no_property_access_from_index_signature: config_option_bool(
            options,
            "noPropertyAccessFromIndexSignature",
        ),
        no_unchecked_side_effect_imports: config_option_bool(
            options,
            "noUncheckedSideEffectImports",
        ),
        strict_property_initialization: config_option_bool(options, "strictPropertyInitialization"),
        use_define_for_class_fields: config_option_bool(options, "useDefineForClassFields"),
        use_unknown_in_catch_variables: config_option_bool(options, "useUnknownInCatchVariables"),
        // Config conversion stores TypeScript's canonical file names
        // (`lib.es5.d.ts`), while the recursive loader's public
        // `CompilerOptions` contract deliberately consumes the lower-cased
        // logical keys (`es5`). Bridge that representation at the config
        // boundary so direct programmatic callers retain their fail-closed
        // raw-key contract without making config programs unusable.
        lib: config_option_lib(options),
        lib_replacement: config_option_bool(options, "libReplacement"),
        jsx: config_option_i32(options, "jsx"),
        no_emit_for_js_files: None, // internal Program API option, not a tsconfig setting
        suppress_output_path_check: None, // internal harness option, not a tsconfig setting
        allow_non_ts_extensions: None, // internal transpile API option, not a tsconfig setting
        no_emit: config_option_bool(options, "noEmit"),
        list_emitted_files: config_option_bool(options, "listEmittedFiles"),
        list_files: config_option_bool(options, "listFiles"),
        explain_files: config_option_bool(options, "explainFiles"),
        diagnostics: config_option_bool(options, "diagnostics"),
        extended_diagnostics: config_option_bool(options, "extendedDiagnostics"),
        trace_resolution: config_option_bool(options, "traceResolution"),
        generate_trace: config_option_string(options, "generateTrace"),
        preserve_watch_output: config_option_bool(options, "preserveWatchOutput"),
        list_files_only: config_option_bool(options, "listFilesOnly"),
        emit_bom: config_option_bool(options, "emitBOM"),
        no_emit_on_error: config_option_bool(options, "noEmitOnError"),
        no_check: config_option_bool(options, "noCheck"),
        deduplicate_packages: config_option_bool(options, "deduplicatePackages"),
        erasable_syntax_only: config_option_bool(options, "erasableSyntaxOnly"),
        out_dir: config_option_string(options, "outDir"),
        root_dir: config_option_string(options, "rootDir"),
        source_map: config_option_bool(options, "sourceMap"),
        inline_source_map: config_option_bool(options, "inlineSourceMap"),
        inline_sources: config_option_bool(options, "inlineSources"),
        source_root: config_option_string(options, "sourceRoot"),
        map_root: config_option_string(options, "mapRoot"),
        declaration: config_option_bool(options, "declaration"),
        declaration_map: config_option_bool(options, "declarationMap"),
        emit_declaration_only: config_option_bool(options, "emitDeclarationOnly"),
        isolated_declarations: config_option_bool(options, "isolatedDeclarations"),
        stable_type_ordering: config_option_bool(options, "stableTypeOrdering"),
        declaration_dir: config_option_string(options, "declarationDir"),
        strip_internal: config_option_bool(options, "stripInternal"),
        out_file: config_option_string(options, "outFile"),
        incremental: config_option_bool(options, "incremental"),
        composite: config_option_bool(options, "composite"),
        assume_changes_only_affect_direct_dependencies: config_option_bool(
            options,
            "assumeChangesOnlyAffectDirectDependencies",
        ),
        ts_build_info_file: config_option_string(options, "tsBuildInfoFile"),
        emit_decorator_metadata: config_option_bool(options, "emitDecoratorMetadata"),
        new_line: config_option_i32(options, "newLine"),
        remove_comments: config_option_bool(options, "removeComments"),
        no_emit_helpers: config_option_bool(options, "noEmitHelpers"),
        no_resolve: config_option_bool(options, "noResolve"),
        import_helpers: config_option_bool(options, "importHelpers"),
        downlevel_iteration: config_option_bool(options, "downlevelIteration"),
        strict_builtin_iterator_return: config_option_bool(options, "strictBuiltinIteratorReturn"),
        module_resolution: config_option_i32(options, "moduleResolution"),
        es_module_interop: config_option_bool(options, "esModuleInterop"),
        allow_synthetic_default_imports: config_option_bool(
            options,
            "allowSyntheticDefaultImports",
        ),
        preserve_const_enums: config_option_bool(options, "preserveConstEnums"),
        isolated_modules: config_option_bool(options, "isolatedModules"),
        verbatim_module_syntax: config_option_bool(options, "verbatimModuleSyntax"),
        allow_umd_global_access: config_option_bool(options, "allowUmdGlobalAccess"),
        base_url: config_option_string(options, "baseUrl"),
        module_suffixes: config_option_module_suffixes(options),
        resolve_package_json_exports: config_option_bool(options, "resolvePackageJsonExports"),
        resolve_package_json_imports: config_option_bool(options, "resolvePackageJsonImports"),
        custom_conditions: config_option_string_list(options, "customConditions"),
        no_dts_resolution: config_option_bool(options, "noDtsResolution"),
        allow_arbitrary_extensions: config_option_bool(options, "allowArbitraryExtensions"),
        allow_importing_ts_extensions: config_option_bool(options, "allowImportingTsExtensions"),
        rewrite_relative_import_extensions: config_option_bool(
            options,
            "rewriteRelativeImportExtensions",
        ),
        // Preserve the raw option presence for emit-profile validation. The
        // loader and resolver read the computed value through
        // `resolve_json_module_effective`; root discovery separately retains
        // the already-computed value on `ConfigDiscoveryOptions`.
        resolve_json_module: config_option_bool(options, "resolveJsonModule"),
        skip_lib_check: config_option_bool(options, "skipLibCheck"),
        skip_default_lib_check: config_option_bool(options, "skipDefaultLibCheck"),
        jsx_factory: config_option_string(options, "jsxFactory"),
        jsx_fragment_factory: config_option_string(options, "jsxFragmentFactory"),
        jsx_import_source: config_option_string(options, "jsxImportSource"),
        react_namespace: config_option_string(options, "reactNamespace"),
        ignore_deprecations: config_option_string(options, "ignoreDeprecations"),
    }
}

/// Project the converted config values needed by the production resolver.
/// Paths retain their declaring directory independently from `baseUrl`: the
/// latter also enables bare-specifier fallback and suppresses TS5090, while
/// `pathsBasePath` only anchors mapping substitutions.
///
/// tsc-port: getPathsBasePath @6.0.3
/// tsc-hash: c569002f6d6a8e7d3b4e2718964fae18fd77125393b0193997bf4cc1f38c494a
/// tsc-span: _tsc.js:16595-16599
fn config_module_resolution_options<'j0>(
    options: &ConfigOptionBag,
    discovery: &ConfigDiscoveryOptions,
    config_file_name: impl Into<JsStr<'j0>>,
    config_source: &ConfigSourceText,
    case_sensitive: bool,
    paths_option_validation: PathsOptionValidationPlan,
) -> Result<ConfigModuleResolutionOptions, ConfigParseError> {
    let config_file_name = config_file_name.into();
    let compiler_options = bag_compiler_options(options, discovery);

    let config_path = config_program_path(config_file_name, case_sensitive)?;
    let config_file = program_config_file(config_path, config_source);
    let mut program_options = ProgramOptions::default()
        .with_config_file(config_file)
        .with_external_config_option_diagnostics();
    if let Some(value) = config_option_bool(options, "noLib") {
        program_options = program_options.with_no_lib(value);
    }
    if let Some(value) = config_option_bool(options, "preserveSymlinks") {
        program_options = program_options.with_preserve_symlinks(value);
    }
    if let Some(value) = config_option_string_list(options, "types") {
        program_options = program_options.with_types(value);
    }
    if let Some(values) = config_option_string_list(options, "typeRoots") {
        program_options = program_options.with_type_roots(
            values
                .into_iter()
                .map(|value| config_program_path(&value, case_sensitive))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    if let Some(values) = config_option_string_list(options, "rootDirs") {
        program_options = program_options.with_root_dirs(
            values
                .into_iter()
                .map(|value| config_program_path(&value, case_sensitive))
                .collect::<Result<Vec<_>, _>>()?,
        );
    }
    if let Some(paths) = options.typed_object_value("paths") {
        let mappings = paths
            .properties()
            .iter()
            .map(|mapping| {
                let substitutions = match mapping.value() {
                    Some(ConfigTypedJsonValue::Array(values)) => values
                        .iter()
                        .filter_map(|value| match value {
                            ConfigTypedJsonValue::Json(Value::String(value)) => Some(value.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                PathMapping::new(mapping.name(), substitutions)
            })
            .collect();
        program_options = program_options.with_config_paths_validation(
            mappings,
            options.stored_paths_base_path().map(JsStr::to_owned),
            paths_option_validation,
        );
    }

    Ok(ConfigModuleResolutionOptions {
        compiler_options,
        program_options,
    })
}

fn config_option_bool(options: &ConfigOptionBag, name: &str) -> Option<bool> {
    options.typed_value(name).and_then(Value::as_bool)
}

fn config_option_i32(options: &ConfigOptionBag, name: &str) -> Option<i32> {
    options
        .typed_value(name)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

/// Preserve the JavaScript `number` domain used by createProgram instead of
/// narrowing numeric config options to Rust integers.
///
/// tsc-port: maxNodeModuleJsDepthInitialization @6.0.3
/// tsc-hash: d5a1d11457ee19a7c4d840633cd4bf52ba239d3a97ee4bac72fb62a85165dc62
/// tsc-span: _tsc.js:122659-122659
fn config_option_number(options: &ConfigOptionBag, name: &str) -> Option<CompilerOptionNumber> {
    let value = match options.typed_value_state(name) {
        ConfigOptionValueState::Value(Value::Number(value)) => json_number_as_f64(value)?,
        ConfigOptionValueState::PositiveInfinity => f64::INFINITY,
        ConfigOptionValueState::NegativeInfinity => f64::NEG_INFINITY,
        ConfigOptionValueState::Absent
        | ConfigOptionValueState::Undefined
        | ConfigOptionValueState::Value(_)
        | ConfigOptionValueState::List(_)
        | ConfigOptionValueState::Object(_) => return None,
    };
    Some(CompilerOptionNumber::new(value))
}

fn config_option_string(options: &ConfigOptionBag, name: &str) -> Option<JsString> {
    options
        .typed_value(name)
        .and_then(Value::as_js)
        .map(JsStr::to_owned)
}

fn config_option_string_list(options: &ConfigOptionBag, name: &str) -> Option<Vec<JsString>> {
    let ConfigOptionValueState::List(values) = options.typed_value_state(name) else {
        return None;
    };
    Some(
        values
            .iter()
            .filter_map(|value| match value {
                ConfigTypedListElement::Value(Value::String(value)) => Some(value.clone()),
                ConfigTypedListElement::Value(_) | ConfigTypedListElement::Undefined => None,
            })
            .collect(),
    )
}

fn config_option_lib(options: &ConfigOptionBag) -> Option<Vec<String>> {
    let values = config_option_string_list(options, "lib")?;
    Some(
        values
            .into_iter()
            .map(|file_name| {
                // lib's element converter selects a value from the fixed
                // ASCII library catalogue; arbitrary raw strings never reach
                // this projection (invalid entries are Undefined).
                let file_name = file_name
                    .as_str()
                    .expect("converted lib entries are scalar catalogue values");
                libraries()
                    .iter()
                    .find(|entry| entry.value() == file_name)
                    .map_or(file_name.to_owned(), |entry| entry.name().to_owned())
            })
            .collect(),
    )
}

/// Project the converted `moduleSuffixes` list without losing JavaScript
/// `undefined` slots: those slots remain observable during string coercion in
/// module resolution.
///
/// tsc-port: moduleSuffixesOptionDeclaration @6.0.3
/// tsc-hash: 67b4fc29e5cda537bb8b4b46f9fe6c9893f37adcdd1b288031e96cad4f40a5c5
/// tsc-span: _tsc.js:37455-37466
/// tsc-port: convertJsonOption/convertJsonOptionOfListType @6.0.3
/// tsc-hash: 4cff23e5f2618b2d041e50271a495efcd2efc423b7772b1ae526e5d91786f676
/// tsc-span: _tsc.js:39555-39605
fn config_option_module_suffixes(options: &ConfigOptionBag) -> Option<Vec<ModuleSuffix>> {
    let ConfigOptionValueState::List(values) = options.typed_value_state("moduleSuffixes") else {
        return None;
    };
    // tsgo convertJsonOptionOfListType keeps the elements that converted
    // (the strings, the empty one included), so no other value reaches the
    // resolver.
    Some(
        values
            .iter()
            .filter_map(|value| match value {
                ConfigTypedListElement::Value(Value::String(value)) => {
                    Some(ModuleSuffix::value(value.clone()))
                }
                ConfigTypedListElement::Value(_) | ConfigTypedListElement::Undefined => None,
            })
            .collect(),
    )
}

/// tsc-port: getMatchedFileSpec @6.0.3
/// tsc-hash: e2dca297bc277048704a713d9d169ddd59813bce20d200095738473671da5915
/// tsc-span: _tsc.js:129276-129284
/// tsc-port: getMatchedIncludeSpec @6.0.3
/// tsc-hash: c19a07b2779a4153a04034ed25d80da875bb9796ce33f327899e55168d145ca2
/// tsc-span: _tsc.js:129285-129299
fn config_root_reasons<'j0, 'j1>(
    file_names: &[JsString],
    files: Option<&[ConfigSpec]>,
    include: Option<&[ConfigSpec]>,
    config_base_path: impl Into<JsStr<'j0>>,
    config_file_name: impl Into<JsStr<'j1>>,
    case_sensitive: bool,
) -> Result<Vec<RootFileReason>, ConfigParseError> {
    let config_base_path = config_base_path.into();
    let config_file_name = config_file_name.into();
    // The first `files` entry with a given key owns the reason (the former
    // linear search returned the first match); the map keeps that entry.
    let mut normalized_files: rustc_hash::FxHashMap<JsString, Arc<JsString>> =
        rustc_hash::FxHashMap::with_capacity_and_hasher(
            files.map_or(0, <[ConfigSpec]>::len),
            Default::default(),
        );
    for spec in files.unwrap_or(&[]) {
        normalized_files
            .entry(file_name_key(
                normalized_spec_path(spec, config_base_path)?.as_js(),
                case_sensitive,
            ))
            .or_insert_with(|| Arc::new(spec.text.clone()));
    }

    let mut include_patterns = Vec::with_capacity(include.map_or(0, <[ConfigSpec]>::len));
    for spec in include.unwrap_or(&[]) {
        if invalid_trailing_recursion_pattern(&spec.text)
            || invalid_dot_dot_after_recursive_wildcard(&spec.text)
        {
            continue;
        }
        let host_spec = config_host_spec(spec, config_base_path)?;
        let pattern = ConfigFilePattern::new(&host_spec, config_base_path, case_sensitive)
            .map_err(|detail| {
                ConfigParseError::new_js(
                    ConfigParseErrorKind::InvalidPath,
                    Some(host_spec.clone()),
                    detail,
                )
            })?;
        if let Some(pattern) = pattern {
            include_patterns.push((pattern, host_spec, Arc::new(spec.text.clone())));
        }
    }
    let config_file = Arc::new(config_file_name.to_owned());
    let default_include = files.is_none() && include.is_none();

    Ok(file_names
        .iter()
        .map(|file_name| {
            let key = file_name_key(file_name.as_js(), case_sensitive);
            if let Some(spec) = normalized_files.get(&key) {
                return RootFileReason::FilesList { spec: spec.clone() };
            }
            // tsgo `getMatchedIncludeSpec`: the first include spec that
            // matches, a JSON file's too (Strada skipped the specs not
            // ending in `.json` for one).
            if let Some((_, _, spec)) = include_patterns
                .iter()
                .find(|(pattern, _, _)| pattern.matches(file_name))
            {
                return RootFileReason::IncludePattern {
                    spec: spec.clone(),
                    config_file: config_file.clone(),
                };
            }
            if default_include {
                RootFileReason::DefaultInclude
            } else {
                RootFileReason::Explicit
            }
        })
        .collect())
}

pub(crate) fn config_program_path<'p>(
    path: impl Into<JsStr<'p>>,
    case_sensitive: bool,
) -> Result<ProgramPath, ConfigParseError> {
    let path = path.into();
    ProgramPath::from_js_parts(path, file_name_key(path, case_sensitive).as_js()).map_err(|error| {
        ConfigParseError::new_js(
            ConfigParseErrorKind::InvalidPath,
            Some(path.to_owned()),
            error.to_string(),
        )
    })
}

/// Retain the root config text plus the exact string syntax consumed by
/// `fileIncludeReasonToRelatedInformation`. TypeScript selects the first root
/// property/value occurrence; inherited option and file-spec syntax therefore
/// intentionally has no root location.
///
/// tsc-port: getTsConfigPropArrayElementValue @6.0.3
/// tsc-hash: 891d5e562eb7429a579f16799d64da319620a7f23f6603171af1aacfdb167dcb
/// tsc-span: _tsc.js:14432-14434
/// tsc-port: getOptionsSyntaxByArrayElementValue @6.0.3
/// tsc-hash: b553000947caf2234186ed0101506333c7de4d13ce5df020b91939d173bd14c7
/// tsc-span: _tsc.js:20105-20111
/// tsc-port: getOptionsSyntaxByValue @6.0.3
/// tsc-hash: 17ba3301b0e0b235cd27fe80671cceec3dc0473af4f1aee5dcf1cacbbbe6fe66
/// tsc-span: _tsc.js:20111-20116
fn program_config_file(path: ProgramPath, source: &ConfigSourceText) -> ProgramConfigFile {
    let mut config_file = ProgramConfigFile::from_snapshot(path, Arc::clone(source.snapshot()))
        .with_diagnostic_file_name(source.file_name.clone());
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    // tsgo locates the program's option syntax in the root object literal
    // only (getTsConfigObjectLiteralExpression); the object a root array
    // recovers is converted but locates nothing.
    let Some(root) = config_root_expression(&parsed)
        .filter(|root| parsed.arena.node(*root).kind == SyntaxKind::ObjectLiteralExpression)
    else {
        return config_file;
    };
    let root_properties = config_object_properties(&parsed, root);
    for property in root_properties
        .iter()
        .filter(|property| matches!(property.name.as_str(), Some("files" | "include")))
    {
        for element in config_array_elements(&parsed, property.initializer) {
            let Some(literal) = parsed.arena.node(element).data.as_string_literal() else {
                continue;
            };
            let Some(span) = config_span(&parsed, element) else {
                continue;
            };
            config_file = config_file.with_root_option_array_location(
                property.name.clone(),
                literal.text.clone(),
                ProgramConfigSpan::new(span.start, span.length),
            );
        }
    }
    for property in root_properties
        .iter()
        .filter(|property| property.name == "references")
    {
        for (index, element) in config_array_elements(&parsed, property.initializer)
            .into_iter()
            .enumerate()
        {
            if let Some(span) = config_span(&parsed, element) {
                config_file = config_file.with_project_reference_location(
                    index,
                    ProgramConfigSpan::new(span.start, span.length),
                );
            }
        }
    }
    for compiler_options in root_properties
        .into_iter()
        .find(|property| property.name == "compilerOptions")
        .into_iter()
    {
        if let Some(span) = config_span(&parsed, compiler_options.name_node) {
            config_file = config_file
                .with_compiler_options_location(ProgramConfigSpan::new(span.start, span.length));
        }
        for property in config_object_properties(&parsed, compiler_options.initializer) {
            if let (Some(name_span), Some(value_span)) = (
                config_span(&parsed, property.name_node),
                config_span(&parsed, property.initializer),
            ) {
                config_file = config_file.with_compiler_option_location(
                    property.name.clone(),
                    ProgramConfigSpan::new(name_span.start, name_span.length),
                    ProgramConfigSpan::new(value_span.start, value_span.length),
                );
            }
            if let (Some(literal), Some(span)) = (
                parsed
                    .arena
                    .node(property.initializer)
                    .data
                    .as_string_literal(),
                config_span(&parsed, property.initializer),
            ) {
                config_file = config_file.with_compiler_option_string_location(
                    property.name.clone(),
                    literal.text.clone(),
                    ProgramConfigSpan::new(span.start, span.length),
                );
            }
            if property.name == "types" {
                for element in config_array_elements(&parsed, property.initializer) {
                    let Some(literal) = parsed.arena.node(element).data.as_string_literal() else {
                        continue;
                    };
                    let Some(span) = config_span(&parsed, element) else {
                        continue;
                    };
                    config_file = config_file.with_automatic_type_directive_location(
                        literal.text.clone(),
                        ProgramConfigSpan::new(span.start, span.length),
                    );
                }
            }
        }
    }
    config_file
}

fn normalized_option_path<'j0>(
    options: &ConfigOptionBag,
    name: &str,
    config_base_path: impl Into<JsStr<'j0>>,
) -> Result<Option<JsString>, ConfigParseError> {
    let config_base_path = config_base_path.into();
    Ok(options
        .typed_value(name)
        .and_then(Value::as_js)
        .and_then(|value| {
            options.get(name).map(|option| {
                normalized_config_dir_value_path(value, config_base_path)
                    .unwrap_or_else(|| normalized_config_value_path(value, &option.base_path))
            })
        }))
}

fn computed_resolve_json_module(options: &ConfigOptionBag) -> bool {
    let module = options.typed_value("module").and_then(Value::as_i64);
    if matches!(module, Some(102 | 199)) {
        return true;
    }
    match options
        .typed_value("moduleResolution")
        .and_then(Value::as_i64)
    {
        Some(100) => true,
        Some(1 | 2 | 3 | 99) => false,
        _ => !matches!(module, Some(0 | 2 | 3 | 4 | 100 | 101)),
    }
}

fn typed_option_bag_json(bag: &ConfigOptionBag) -> Value {
    let mut object = Map::new();
    for entry in &bag.typed_entries {
        let Some(value) = &entry.value else {
            continue;
        };
        let value = match value {
            ConfigTypedOptionValue::Json(value) => value.clone(),
            ConfigTypedOptionValue::Object(value) => value.json_projection(),
            ConfigTypedOptionValue::List(values) => Value::Array(
                values
                    .iter()
                    .map(|element| match element {
                        ConfigTypedListElement::Undefined => Value::Null,
                        ConfigTypedListElement::Value(value) => value.clone(),
                    })
                    .collect(),
            ),
            ConfigTypedOptionValue::PositiveInfinity | ConfigTypedOptionValue::NegativeInfinity => {
                Value::Null
            }
        };
        object.insert(entry.name.clone(), value);
    }
    Value::Object(object)
}

/// The type acquisition a config defaults: `enable` for a `jsconfig.json`,
/// nothing for a `tsconfig.json`.
///
/// tsgo-port: getDefaultTypeAcquisition @7.1 (tsoptions/tsconfigparsing.go:941-947)
fn default_type_acquisition<'j0>(file_name: impl Into<JsStr<'j0>>) -> ConfigOptionBag {
    let file_name = file_name.into();
    let mut bag = ConfigOptionBag::default();
    if file_name.split_ascii(b'/').next_back() == Some("jsconfig.json".into()) {
        let value = Value::Bool(true);
        bag.insert(ConfigOption {
            name: "enable".into(),
            value: value.clone(),
            base_path: JsString::new(),
        });
        bag.insert_typed("enable", Some(ConfigTypedOptionValue::Json(value)));
    }
    bag
}

fn validate_compile_on_save<'j0>(
    source: &SourceFile,
    base_path: impl Into<JsStr<'j0>>,
    errors: &mut Vec<Diagnostic>,
) -> Result<(), ConfigParseError> {
    let base_path = base_path.into();
    for property in config_root_object(source)
        .into_iter()
        .flat_map(|root| config_object_properties(source, root))
        .filter(|p| p.name == "compileOnSave")
    {
        match convert_recoverable_json_node_to_value(source, property.initializer) {
            Some(RecoverableJsonValue::Defined(value)) => {
                convert_compiler_option_value(
                    crate::config_options::COMPILE_ON_SAVE_DECLARATION,
                    "compileOnSave",
                    &value,
                    CompilerOptionConversionContext {
                        source,
                        value_node: property.initializer,
                        base_path,
                        value_location: config_location(source, property.initializer),
                        name_location: config_location(source, property.name_node),
                    },
                    errors,
                )?;
            }
            // The conversion reports a value JSON has no form for (TS5024).
            Some(RecoverableJsonValue::Undefined) | None => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ConfigOptionGroup {
    Compiler,
    Watch,
    Acquisition,
}

impl ConfigOptionGroup {
    const fn name(self) -> &'static str {
        match self {
            Self::Compiler => "compilerOptions",
            Self::Watch => "watchOptions",
            Self::Acquisition => "typeAcquisition",
        }
    }
    fn declaration<'n>(
        self,
        name: impl Into<JsStr<'n>>,
    ) -> Option<&'static crate::config_options::CompilerOptionDeclaration> {
        let name = name.into();
        match self {
            Self::Compiler => compiler_option_declaration(name),
            Self::Watch => crate::config_options::WATCH_OPTION_DECLARATIONS
                .iter()
                .find(|d| name == d.name()),
            Self::Acquisition => crate::config_options::ACQUISITION_OPTION_DECLARATIONS
                .iter()
                .find(|d| name == d.name()),
        }
    }
    fn unknown(self, name: JsStr<'_>) -> (&'static DiagnosticMessage, Vec<JsString>) {
        let (plain, suggested, suggestion) = match self {
            Self::Compiler => (
                &gen::Unknown_compiler_option_0,
                &gen::Unknown_compiler_option_0_Did_you_mean_1,
                compiler_option_spelling_suggestion(name),
            ),
            Self::Watch => (
                &gen::Unknown_watch_option_0,
                &gen::Unknown_watch_option_0_Did_you_mean_1,
                crate::config_options::option_spelling_suggestion(
                    name,
                    crate::config_options::WATCH_OPTION_DECLARATIONS,
                ),
            ),
            Self::Acquisition => (
                &gen::Unknown_type_acquisition_option_0,
                &gen::Unknown_type_acquisition_option_0_Did_you_mean_1,
                crate::config_options::option_spelling_suggestion(
                    name,
                    crate::config_options::ACQUISITION_OPTION_DECLARATIONS,
                ),
            ),
        };
        suggestion.map_or_else(
            || (plain, vec![name.to_owned()]),
            |d| (suggested, vec![name.to_owned(), d.name().into()]),
        )
    }
}

fn config_option_group<'j0>(
    base_path: impl Into<JsStr<'j0>>,
    group: ConfigOptionGroup,
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Result<ConfigOptionBag, ConfigParseError> {
    let base_path = base_path.into();
    let mut bag = ConfigOptionBag::default();
    let Some(root) = config_root_object(source) else {
        return Ok(bag);
    };
    for compiler_options in config_object_properties(source, root)
        .into_iter()
        .filter(|property| property.name == group.name())
    {
        let Some(value) =
            convert_recoverable_json_node_to_value(source, compiler_options.initializer)
        else {
            continue;
        };
        // A value JSON has no form for is the conversion's TS5024; tsgo
        // takes `null` for no value.
        let RecoverableJsonValue::Defined(value) = value else {
            continue;
        };
        // tsgo isCompilerOptionsValue: an object option takes an object (an
        // array is not one, nor an array none of whose elements converted).
        if value.is_null() && !is_array_literal(source, compiler_options.initializer) {
            continue;
        }
        let Some(options) = value.as_object() else {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &[group.name().to_owned(), "object".to_owned()],
                config_location(source, compiler_options.initializer),
            ));
            continue;
        };

        // convertConfigFileToObject invokes its option notifier for every
        // property assignment, including keys shadowed by a later duplicate.
        // This is observably different from iterating the final JSON object:
        // earlier diagnostics remain and compilerOptions objects accumulate.
        for property in config_object_properties(source, compiler_options.initializer) {
            let name = property.name.as_js();
            // Ordinary JavaScript assignment establishes property order even
            // when the recovered value is `undefined`. The legacy
            // `__proto__` setter is the exception: it may change only the
            // prototype, so its own-key order comes from the final converted
            // object below.
            if name != "__proto__" {
                bag.observe_raw_name(name);
            }
            let value = convert_recoverable_json_node_to_value(source, property.initializer);
            if matches!(&value, Some(RecoverableJsonValue::Undefined)) {
                bag.remove(name);
            }
            // tsgo converts `null` and a value JSON has no form for to nil,
            // and its notifier checks no option name for a nil value.
            let nil = matches!(
                &value,
                Some(RecoverableJsonValue::Undefined | RecoverableJsonValue::Defined(Value::Null))
            );
            let value_location = config_location(source, property.initializer);
            let name_location = config_location(source, property.name_node);
            if let Some(declaration) = group.declaration(name) {
                // Exact schema lookup proves this original key is the same
                // ASCII spelling as the declaration; typed names stay scalar.
                let name = declaration.name();
                let typed = match value {
                    Some(RecoverableJsonValue::Defined(value)) => convert_compiler_option_value(
                        *declaration,
                        name,
                        &value,
                        CompilerOptionConversionContext {
                            source,
                            value_node: property.initializer,
                            base_path,
                            value_location,
                            name_location,
                        },
                        errors,
                    )?,
                    // The conversion reports the value (TS5024).
                    Some(RecoverableJsonValue::Undefined) => {
                        if declaration.is_command_line_only() {
                            errors.push(config_diagnostic(
                                &gen::Option_0_can_only_be_specified_on_command_line,
                                &[name.to_owned()],
                                name_location,
                            ));
                        }
                        None
                    }
                    None => None,
                };
                bag.insert_typed(name, typed);
            } else if !nil {
                let (message, args) = group.unknown(name);
                errors.push(config_diagnostic(message, &args, name_location));
            }
        }

        // The raw projection follows the converted object's own enumerable
        // properties. This deliberately strips JSONC prototype state while
        // the typed notifier above still observes every written key.
        for (name, value) in options {
            let Some(name) = decode_user_object_key(name) else {
                continue;
            };
            // tsgo convertJsonOption drops a command-line-only option's value
            // (after TS6266), so a configuration never sets one.
            if group
                .declaration(name)
                .is_some_and(|declaration| declaration.is_command_line_only())
            {
                continue;
            }
            bag.insert(ConfigOption {
                name: name.to_owned(),
                value: config_raw_projection(value.clone()),
                base_path: base_path.to_owned(),
            });
        }
    }
    Ok(bag)
}

struct CompilerOptionConversionContext<'a> {
    source: &'a SourceFile,
    value_node: NodeId,
    base_path: JsStr<'a>,
    value_location: Option<ConfigLocation>,
    name_location: Option<ConfigLocation>,
}

/// Convert one compiler option using the pinned JSON option declaration.
///
/// tsc-port: convertJsonOption/convertJsonOptionOfListType @6.0.3
/// tsc-hash: 4cff23e5f2618b2d041e50271a495efcd2efc423b7772b1ae526e5d91786f676
/// tsc-span: _tsc.js:39555-39605
/// tsc-port: isCompilerOptionsValue @6.0.3
/// tsc-hash: 219b4850c3b03c080e414da8843c59e2651ba6f9de96d91b3d99bf0b927ed00b
/// tsc-span: _tsc.js:38604-38617
fn convert_compiler_option_value(
    declaration: crate::config_options::CompilerOptionDeclaration,
    name: &str,
    value: &Value,
    context: CompilerOptionConversionContext<'_>,
    errors: &mut Vec<Diagnostic>,
) -> Result<Option<ConfigTypedOptionValue>, ConfigParseError> {
    let CompilerOptionConversionContext {
        source,
        value_node,
        base_path,
        value_location,
        name_location,
    } = context;
    if declaration.is_command_line_only() {
        errors.push(config_diagnostic(
            &gen::Option_0_can_only_be_specified_on_command_line,
            &[name.to_owned()],
            name_location,
        ));
        return Ok(None);
    }
    if value.is_null() {
        // An array none of whose elements converted is nil but still an
        // array to tsgo's type check (isCompilerOptionsValue).
        if is_array_literal(source, value_node)
            && !matches!(declaration.value_kind(), CompilerOptionValueKind::List(_))
        {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &[
                    name.to_owned(),
                    compiler_option_expected_type(declaration).to_owned(),
                ],
                value_location,
            ));
        }
        return Ok(None);
    }
    let expected = compiler_option_expected_type(declaration);
    let kind_matches = match declaration.value_kind() {
        CompilerOptionValueKind::Boolean => value.is_boolean(),
        CompilerOptionValueKind::Number => value.is_number(),
        CompilerOptionValueKind::String | CompilerOptionValueKind::Named(_) => value.is_string(),
        // tsgo isCompilerOptionsValue: an array is not an object.
        CompilerOptionValueKind::Object(_) => value.is_object(),
        CompilerOptionValueKind::List(_) => value.is_array(),
    };
    if !kind_matches {
        errors.push(config_diagnostic(
            &gen::Compiler_option_0_requires_a_value_of_type_1,
            &[name.to_owned(), expected.to_owned()],
            value_location,
        ));
        return Ok(None);
    }
    if let CompilerOptionValueKind::List(descriptor) = declaration.value_kind() {
        return convert_compiler_option_list_value(
            descriptor,
            value
                .as_array()
                .expect("list options have already passed array validation"),
            source,
            value_node,
            base_path,
            value_location,
            errors,
        )
        .map(|elements| Some(ConfigTypedOptionValue::List(elements)));
    }
    if matches!(declaration.value_kind(), CompilerOptionValueKind::Object(_)) {
        return Ok(Some(ConfigTypedOptionValue::Object(Arc::new(
            convert_compiler_option_object_value(source, value_node),
        ))));
    }
    if let CompilerOptionValueKind::Named(values) = declaration.value_kind() {
        let written = value.as_js().expect("named options require a string");
        let Some(converted) = named_value_in(values, written) else {
            let choices = config_named_option_choices(name, values);
            errors.push(config_diagnostic(
                &gen::Argument_for_0_option_must_be_1,
                &[format!("--{name}"), choices],
                value_location,
            ));
            return Ok(None);
        };
        return Ok(Some(ConfigTypedOptionValue::Json(Value::from(converted))));
    }
    if let Some(number) = value.as_number() {
        if number.as_u64() == Some(u64::MAX) {
            return Ok(Some(ConfigTypedOptionValue::PositiveInfinity));
        }
        if number.as_i64() == Some(i64::MIN) {
            return Ok(Some(ConfigTypedOptionValue::NegativeInfinity));
        }
    }
    if declaration.is_file_path() {
        let written = crate::js_path::normalize_slashes(
            value
                .as_js()
                .expect("file-path options have already passed string validation"),
        );
        let normalized = if starts_with_config_dir_template(&written) {
            written
        } else {
            normalized_config_value_path(written.as_js(), base_path)
        };
        return Ok(Some(ConfigTypedOptionValue::Json(Value::String(
            normalized,
        ))));
    }
    Ok(Some(ConfigTypedOptionValue::Json(config_raw_projection(
        value.clone(),
    ))))
}

fn compiler_option_expected_type(
    declaration: crate::config_options::CompilerOptionDeclaration,
) -> &'static str {
    match declaration.value_kind() {
        CompilerOptionValueKind::Boolean => "boolean",
        CompilerOptionValueKind::Number => "number",
        CompilerOptionValueKind::String => "string",
        // tsgo names the kind of a map-valued option (`enum`).
        CompilerOptionValueKind::Named(_) => "enum",
        CompilerOptionValueKind::Object(_) => "object",
        CompilerOptionValueKind::List(_) => "Array",
    }
}

fn compiler_option_list_element_expected_type(
    descriptor: CompilerOptionListDescriptor,
) -> &'static str {
    match descriptor.element_kind() {
        CompilerOptionListElementKind::String | CompilerOptionListElementKind::FilePath => "string",
        CompilerOptionListElementKind::NamedString(_) => "enum",
        CompilerOptionListElementKind::Object => "object",
    }
}

fn convert_compiler_option_object_value(
    source: &SourceFile,
    value_node: NodeId,
) -> ConfigTypedObjectValue {
    match convert_config_typed_json_node(source, value_node)
        .expect("object options have a converted object-like source value")
    {
        ConfigTypedJsonValue::Array(values) => ConfigTypedObjectValue::new(
            ConfigTypedObjectShape::Array,
            values
                .into_iter()
                .enumerate()
                .map(|(index, value)| ConfigTypedObjectProperty {
                    name: index.to_string().into(),
                    value: Some(value),
                })
                .collect(),
            true,
        ),
        ConfigTypedJsonValue::Object(value) => *value,
        ConfigTypedJsonValue::Json(_) => {
            unreachable!("object options reject non-object source values")
        }
    }
}

enum ConfigTypedJsonConversionTask {
    Visit(NodeId),
    FinishArray(usize),
    FinishObject(Vec<JsString>),
}

/// Preserve convertToJson's complete JavaScript value identity for object
/// options. This postorder worker stays iterative like the primary JSONC
/// converter, while arrays filter undefined elements and object assignments
/// keep undefined own keys and legacy `__proto__` transitions.
fn convert_config_typed_json_node(
    source: &SourceFile,
    root: NodeId,
) -> Option<ConfigTypedJsonValue> {
    let mut tasks = vec![ConfigTypedJsonConversionTask::Visit(root)];
    let mut values = Vec::<Option<ConfigTypedJsonValue>>::new();

    while let Some(task) = tasks.pop() {
        match task {
            ConfigTypedJsonConversionTask::Visit(node_id) => {
                match source.arena.node(node_id).kind {
                    SyntaxKind::ArrayLiteralExpression => {
                        let elements = config_array_elements(source, node_id);
                        tasks.push(ConfigTypedJsonConversionTask::FinishArray(elements.len()));
                        tasks.extend(
                            elements
                                .into_iter()
                                .rev()
                                .map(ConfigTypedJsonConversionTask::Visit),
                        );
                    }
                    SyntaxKind::ObjectLiteralExpression => {
                        let properties = config_object_properties(source, node_id);
                        let keys = properties
                            .iter()
                            .map(|property| property.name.clone())
                            .collect();
                        tasks.push(ConfigTypedJsonConversionTask::FinishObject(keys));
                        tasks.extend(properties.into_iter().rev().map(|property| {
                            ConfigTypedJsonConversionTask::Visit(property.initializer)
                        }));
                    }
                    _ => values.push(
                        match convert_recoverable_json_node_to_value(source, node_id) {
                            Some(RecoverableJsonValue::Defined(value)) => {
                                debug_assert!(!value.is_array() && !value.is_object());
                                Some(ConfigTypedJsonValue::Json(config_raw_projection(value)))
                            }
                            Some(RecoverableJsonValue::Undefined) | None => None,
                        },
                    ),
                }
            }
            ConfigTypedJsonConversionTask::FinishArray(length) => {
                let start = values.len().checked_sub(length)?;
                // tsgo convertArrayLiteralExpressionToJson keeps the elements
                // that convert to a non-nil value (`null` converts to nil),
                // and an array none of whose elements survive is nil.
                let elements = values
                    .split_off(start)
                    .into_iter()
                    .flatten()
                    .filter(|element| !matches!(element, ConfigTypedJsonValue::Json(Value::Null)))
                    .collect::<Vec<_>>();
                values.push(
                    (length == 0 || !elements.is_empty())
                        .then_some(ConfigTypedJsonValue::Array(elements)),
                );
            }
            ConfigTypedJsonConversionTask::FinishObject(keys) => {
                let start = values.len().checked_sub(keys.len())?;
                let object_values = values.split_off(start);
                values.push(Some(ConfigTypedJsonValue::Object(Box::new(
                    converted_typed_object(keys.into_iter().zip(object_values)),
                ))));
            }
        }
    }

    let [value] = values.as_mut_slice() else {
        return None;
    };
    value.take()
}

fn converted_typed_object(
    assignments: impl IntoIterator<Item = (JsString, Option<ConfigTypedJsonValue>)>,
) -> ConfigTypedObjectValue {
    let mut properties = Vec::<ConfigTypedObjectProperty>::new();
    let mut indices = BTreeMap::<JsString, usize>::new();
    let mut inherits_proto_setter = true;
    for (name, value) in assignments {
        if name == "__proto__" && !indices.contains_key(name.as_bytes()) && inherits_proto_setter {
            if let Some(next_state) = value
                .as_ref()
                .and_then(ConfigTypedJsonValue::inherited_proto_setter)
            {
                inherits_proto_setter = next_state;
            }
            continue;
        }
        if let Some(index) = indices.get(name.as_bytes()).copied() {
            properties[index].value = value;
        } else {
            let index = properties.len();
            indices.insert(name.clone(), index);
            properties.push(ConfigTypedObjectProperty { name, value });
        }
    }

    ConfigTypedObjectValue::new(
        ConfigTypedObjectShape::Object,
        properties,
        inherits_proto_setter,
    )
}

fn convert_compiler_option_list_value<'j0>(
    descriptor: CompilerOptionListDescriptor,
    values: &[Value],
    source: &SourceFile,
    value_node: NodeId,
    base_path: impl Into<JsStr<'j0>>,
    value_location: Option<ConfigLocation>,
    errors: &mut Vec<Diagnostic>,
) -> Result<Vec<ConfigTypedListElement>, ConfigParseError> {
    let base_path = base_path.into();
    // convertToJson filters unsupported syntax out of the JSON array before
    // onPropertySet invokes convertJsonOption. TypeScript nevertheless indexes
    // the original AST array with the compacted value index. Preserve that
    // observable (and somewhat surprising) shifted diagnostic location.
    let source_elements = config_array_elements(source, value_node);
    let mut converted = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        let element_location = source_elements
            .get(index)
            .and_then(|node| config_location(source, *node))
            .or_else(|| value_location.clone());
        let element = convert_compiler_option_list_element(
            descriptor,
            value,
            base_path,
            element_location,
            errors,
        )?;
        if descriptor.preserve_falsy_values() || config_typed_list_element_is_truthy(&element) {
            converted.push(element);
        }
    }
    Ok(converted)
}

fn convert_compiler_option_list_element<'j0>(
    descriptor: CompilerOptionListDescriptor,
    value: &Value,
    base_path: impl Into<JsStr<'j0>>,
    location: Option<ConfigLocation>,
    errors: &mut Vec<Diagnostic>,
) -> Result<ConfigTypedListElement, ConfigParseError> {
    let base_path = base_path.into();
    if value.is_null() {
        return Ok(ConfigTypedListElement::Undefined);
    }

    let converted = match descriptor.element_kind() {
        CompilerOptionListElementKind::String | CompilerOptionListElementKind::FilePath => {
            let Some(written) = value.as_js() else {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[
                        descriptor.element_name().to_owned(),
                        compiler_option_list_element_expected_type(descriptor).to_owned(),
                    ],
                    location,
                ));
                return Ok(ConfigTypedListElement::Undefined);
            };
            // tsgo `validateJsonOptionValue` passes the spec message without
            // its argument, so the text keeps `'{0}'` (tsconfigparsing.go:387-391).
            if descriptor.validate_file_spec() && invalid_dot_dot_after_recursive_wildcard(written)
            {
                errors.push(config_diagnostic(
                    &gen::File_specification_cannot_contain_a_parent_directory_that_appears_after_a_recursive_directory_wildcard_0,
                    &[] as &[String], location,
                ));
                return Ok(ConfigTypedListElement::Undefined);
            }
            if matches!(
                descriptor.element_kind(),
                CompilerOptionListElementKind::FilePath
            ) {
                let written = crate::js_path::normalize_slashes(written);
                Value::String(if starts_with_config_dir_template(&written) {
                    written
                } else {
                    normalized_config_value_path(written.as_js(), base_path)
                })
            } else {
                Value::String(written.to_owned())
            }
        }
        CompilerOptionListElementKind::NamedString(_) => {
            let Some(written) = value.as_js() else {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[
                        descriptor.element_name().to_owned(),
                        compiler_option_list_element_expected_type(descriptor).to_owned(),
                    ],
                    location,
                ));
                return Ok(ConfigTypedListElement::Undefined);
            };
            let Some(mapped) = descriptor.named_string_value(written) else {
                errors.push(config_diagnostic(
                    &gen::Argument_for_0_option_must_be_1,
                    &[
                        format!("--{}", descriptor.element_name()),
                        config_named_string_option_choices(descriptor),
                    ],
                    location,
                ));
                return Ok(ConfigTypedListElement::Undefined);
            };
            Value::String(mapped.into())
        }
        CompilerOptionListElementKind::Object => {
            // tsgo isCompilerOptionsValue: an object option takes an object
            // (an array is not one).
            if !value.is_object() {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[
                        descriptor.element_name().to_owned(),
                        compiler_option_list_element_expected_type(descriptor).to_owned(),
                    ],
                    location,
                ));
                return Ok(ConfigTypedListElement::Undefined);
            }
            config_raw_projection(value.clone())
        }
    };
    Ok(ConfigTypedListElement::Value(converted))
}

fn config_typed_list_element_is_truthy(element: &ConfigTypedListElement) -> bool {
    match element {
        ConfigTypedListElement::Undefined => false,
        ConfigTypedListElement::Value(Value::Null) => false,
        ConfigTypedListElement::Value(Value::Bool(value)) => *value,
        ConfigTypedListElement::Value(Value::Number(value)) => {
            json_number_as_f64(value).is_some_and(|value| value != 0.0 && !value.is_nan())
        }
        ConfigTypedListElement::Value(Value::String(value)) => !value.is_empty(),
        ConfigTypedListElement::Value(Value::Array(_) | Value::Object(_)) => true,
    }
}

pub(crate) fn config_named_string_option_choices(
    descriptor: CompilerOptionListDescriptor,
) -> String {
    descriptor
        .named_string_choices()
        .expect("named-string list descriptors carry their choices")
        .iter()
        .map(|value| format!("'{}'", value.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// tsc-port: createDiagnosticForInvalidCustomType @6.0.3
/// tsc-hash: b69f5290ca841865e38c6a0bf5f6515d78d3935f2ca4f9ad1eaa65c67c3e3dfd
/// tsc-span: typescript.js:42341-42345
/// The `'a', 'b'` choice list for an enum-typed option, as printed by
/// TS6046. `None` when the option is not enum-typed.
pub fn compiler_option_named_choices(name: &str) -> Option<String> {
    let declaration = crate::config_options::compiler_option_declaration(name)?;
    match declaration.value_kind() {
        CompilerOptionValueKind::Named(values) => Some(config_named_option_choices(name, values)),
        _ => None,
    }
}

/// TypeScript 7.1 (`tsoptions/errors.go` `formatEnumTypeKeys`) omits the
/// `DeprecatedKeys` and lists `es2026`.
pub(crate) fn config_named_option_choices(
    name: &str,
    values: &[crate::config_options::CompilerOptionNamedValue],
) -> String {
    match name {
        "target" => "'es6', 'es2015', 'es2016', 'es2017', 'es2018', 'es2019', 'es2020', 'es2021', 'es2022', 'es2023', 'es2024', 'es2025', 'es2026', 'esnext'".to_owned(),
        "module" => "'commonjs', 'es6', 'es2015', 'es2020', 'es2022', 'esnext', 'node16', 'node18', 'node20', 'nodenext', 'preserve'".to_owned(),
        "moduleResolution" => "'node16', 'nodenext', 'bundler'".to_owned(),
        _ => values
            .iter()
            .map(|value| format!("'{}'", value.name()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn default_compiler_options<'j0, 'j1>(
    config_file_name: impl Into<JsStr<'j0>>,
    base_path: impl Into<JsStr<'j1>>,
) -> ConfigOptionBag {
    let config_file_name = config_file_name.into();
    let base_path = base_path.into();
    if config_file_name.split_ascii(b'/').next_back() != Some("jsconfig.json".into()) {
        return ConfigOptionBag::default();
    }

    let mut options = ConfigOptionBag::default();
    for &(name, default) in jsconfig_defaults() {
        let value = match default {
            JsConfigDefaultValue::Boolean(value) => Value::Bool(value),
            JsConfigDefaultValue::Number(value) => Value::from(value),
        };
        options.insert(ConfigOption {
            name: name.into(),
            value: value.clone(),
            base_path: base_path.to_owned(),
        });
        options.insert_typed(name, Some(ConfigTypedOptionValue::Json(value)));
    }
    options
}

fn specs<'j0>(
    name: &str,
    base_path: impl Into<JsStr<'j0>>,
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Option<Vec<ConfigSpec>> {
    let base_path = base_path.into();
    let root = config_root_object(source)?;
    let mut result = None;
    for property in config_object_properties(source, root)
        .into_iter()
        .filter(|property| property.name == name)
    {
        result = match convert_recoverable_json_node_to_value(source, property.initializer) {
            Some(RecoverableJsonValue::Defined(value)) => specs_from_value(
                &value,
                name,
                base_path,
                source,
                Some(property.initializer),
                errors,
            ),
            // The conversion reports a value JSON has no form for (TS5024).
            Some(RecoverableJsonValue::Undefined) | None => None,
        };
    }
    result
}

/// tsgo applyExtendedConfig reads the config's own `files`, `include` or
/// `exclude` array (after its own extends were applied); any other value is
/// not inherited.
fn inheritable_specs<'j0>(
    object: &Map,
    raw_property_names: &BTreeSet<JsString>,
    name: &str,
    base_path: impl Into<JsStr<'j0>>,
    source: &SourceFile,
) -> Option<Vec<InheritedSpec>> {
    let base_path = base_path.into();
    if !raw_property_names.contains(name.as_bytes()) {
        return None;
    }
    let value = json_object_own_get(object, name)?;
    let Value::Array(values) = value else {
        return None;
    };
    if is_nil_json_array(value) {
        return None;
    }
    let locations = config_spec_locations(source, name);
    Some(
        values
            .iter()
            .filter(|value| !value.is_null())
            .map(|value| match value {
                Value::String(text) => InheritedSpec::Path(ConfigSpec {
                    location: locations.get(text.as_js().as_bytes()).cloned().flatten(),
                    text: text.clone(),
                    base_path: base_path.to_owned(),
                }),
                other => InheritedSpec::Other(other.clone()),
            })
            .collect(),
    )
}

fn specs_from_value<'j0>(
    value: &Value,
    name: &str,
    base_path: impl Into<JsStr<'j0>>,
    source: &SourceFile,
    initializer: Option<NodeId>,
    errors: &mut Vec<Diagnostic>,
) -> Option<Vec<ConfigSpec>> {
    let base_path = base_path.into();
    if value.is_null() || is_nil_json_array(value) {
        return None;
    }
    let Some(values) = value.as_array() else {
        errors.push(config_diagnostic(
            &gen::Compiler_option_0_requires_a_value_of_type_1,
            &[name.to_owned(), "Array".to_owned()],
            initializer.and_then(|node| config_location(source, node)),
        ));
        return None;
    };
    let element_nodes = initializer.map_or_else(Vec::new, |initializer| {
        config_array_elements(source, initializer)
    });
    let locations = config_spec_locations(source, name);
    let mut specs = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        // convertToJson has already removed unsupported syntax, but the
        // notifier indexes the original array with this compacted index.
        let location = element_nodes
            .get(index)
            .and_then(|element| config_location(source, *element));
        if let Some(text) = value.as_js() {
            specs.push(ConfigSpec {
                text: text.to_owned(),
                base_path: base_path.to_owned(),
                // validateSpecs later recovers a node by written value and
                // therefore reuses the first matching source location for
                // duplicate strings, independently of the shifted notifier
                // location above.
                location: locations.get(text.as_bytes()).cloned().flatten(),
            });
        } else if !value.is_null() {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &[name.to_owned(), "string".to_owned()],
                location,
            ));
        }
    }
    Some(specs)
}

fn config_property_get<'a>(
    object: &'a Map,
    raw_property_names: &BTreeSet<JsString>,
    name: &str,
) -> Option<&'a Value> {
    if raw_property_names.contains(name.as_bytes()) {
        // A written own property whose recovered value is `undefined` still
        // shadows the JSONC object's prototype. serde_json omits that value,
        // so an own-only lookup must not fall through to the prototype.
        json_object_own_get(object, name)
    } else {
        json_object_get(object, name)
    }
}

fn json_value_is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value
            .as_f64()
            .is_some_and(|value| value != 0.0 && !value.is_nan()),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn extends_value_occurrences(
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Vec<Vec<ConfigExtendsSpec>> {
    let Some(root) = config_root_object(source) else {
        return Vec::new();
    };
    config_object_properties(source, root)
        .into_iter()
        .filter(|property| property.name == "extends")
        .map(|property| {
            let Some(value) = convert_recoverable_json_node_to_value(source, property.initializer)
            else {
                return Vec::new();
            };
            match value {
                // An array none of whose elements converted is a nil slice:
                // tsgo reads no path from it and reports nothing more.
                RecoverableJsonValue::Defined(Value::Null)
                    if is_array_literal(source, property.initializer) =>
                {
                    Vec::new()
                }
                RecoverableJsonValue::Defined(value) => {
                    extends_values_from_value(&value, property.initializer, source, errors)
                }
                // The conversion reports the value at its whole range;
                // tsgo getExtendsConfigPathOrArray reports it again.
                RecoverableJsonValue::Undefined => {
                    errors.push(config_diagnostic(
                        &gen::Compiler_option_0_requires_a_value_of_type_1,
                        &["extends".to_owned(), "string or Array".to_owned()],
                        config_location(source, property.initializer),
                    ));
                    Vec::new()
                }
            }
        })
        .collect()
}

fn extends_values_from_value(
    value: &Value,
    initializer: NodeId,
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Vec<ConfigExtendsSpec> {
    if let Some(value) = value.as_js() {
        return vec![ConfigExtendsSpec {
            text: value.to_owned(),
            location: config_location(source, initializer),
        }];
    }
    let Some(values) = value.as_array() else {
        errors.push(config_diagnostic(
            &gen::Compiler_option_0_requires_a_value_of_type_1,
            &["extends".to_owned(), "string or Array".to_owned()],
            config_location(source, initializer),
        ));
        return Vec::new();
    };
    let element_nodes = config_array_elements(source, initializer);
    let mut result = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let location = element_nodes
            .get(index)
            .and_then(|element| config_location(source, *element));
        if let Some(text) = value.as_js() {
            result.push(ConfigExtendsSpec {
                text: text.to_owned(),
                location,
            });
        } else {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &["extends".to_owned(), "string".to_owned()],
                location,
            ));
        }
    }
    result
}

/// tsgo applyExtendedConfig: a path not rooted and not starting with
/// `${configDir}` is made relative to the extending config; other values
/// stay as written.
fn rebase_inherited_specs<'j0>(
    specs: &[InheritedSpec],
    base_path: impl Into<JsStr<'j0>>,
    case_sensitive: bool,
) -> Result<Vec<InheritedSpec>, ConfigParseError> {
    let base_path = base_path.into();
    specs
        .iter()
        .map(|spec| {
            let spec = match spec {
                InheritedSpec::Path(spec) => spec,
                InheritedSpec::Other(value) => return Ok(InheritedSpec::Other(value.clone())),
            };
            let text = normalize_slashes(spec.text.as_js());
            let rebased = if starts_with_config_dir_template(text.as_js())
                || root_parts(text.as_js()).is_some()
            {
                text
            } else {
                let mut difference =
                    relative_directory_path(base_path, &spec.base_path, case_sensitive)?;
                if text.is_empty() {
                    difference
                } else if difference.is_empty() {
                    text
                } else {
                    if !difference.ends_with("/") {
                        difference.push('/');
                    }
                    difference.push_js(text.as_js());
                    difference
                }
            };
            Ok(InheritedSpec::Path(ConfigSpec {
                text: rebased,
                base_path: base_path.to_owned(),
                // Inherited specs are copied into the root raw object but do
                // not have a corresponding node in the root source file.
                location: None,
            }))
        })
        .collect()
}

fn relative_directory_path<'f, 't>(
    from: impl Into<JsStr<'f>>,
    to: impl Into<JsStr<'t>>,
    case_sensitive: bool,
) -> Result<JsString, ConfigParseError> {
    let from = from.into();
    let to = to.into();
    let (to_root, to_components) = rooted_components(to)?;
    // URL paths pass through before the base is inspected, as in TypeScript.
    if !is_disk_root(to_root) {
        return Ok(to.to_owned());
    }
    let (from_root, from_components) = rooted_components(from)?;
    if !config_root_eq_ignore_case(from_root, to_root) {
        return Ok(to.to_owned());
    }
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| {
            if case_sensitive {
                left == right
            } else {
                canonical_key(**left, false) == canonical_key(**right, false)
            }
        })
        .count();
    let mut result = JsString::new();
    for component in std::iter::repeat_n(JsStr::from(".."), from_components.len() - common)
        .chain(to_components[common..].iter().copied())
    {
        if !result.is_empty() {
            result.push('/');
        }
        result.push_js(component);
    }
    Ok(result)
}

fn config_root_eq_ignore_case(left: JsStr<'_>, right: JsStr<'_>) -> bool {
    crate::js_path::eq_ignore_case(left, right)
}

fn is_disk_root(root: JsStr<'_>) -> bool {
    root.starts_with("/")
        || (root.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && root.as_bytes().get(1) == Some(&b':'))
}

fn rooted_components<'p>(
    path: impl Into<JsStr<'p>>,
) -> Result<(JsStr<'p>, Vec<JsStr<'p>>), ConfigParseError> {
    let path = path.into();
    let (root, tail) = root_parts(path).ok_or_else(|| {
        ConfigParseError::new_js(
            ConfigParseErrorKind::InvalidPath,
            Some(path.to_owned()),
            "config directory is not rooted",
        )
    })?;
    Ok((
        root,
        tail.split_ascii(b'/')
            .filter(|component| !component.is_empty())
            .collect(),
    ))
}

fn normalized_spec_path<'j0>(
    spec: &ConfigSpec,
    config_base_path: impl Into<JsStr<'j0>>,
) -> Result<JsString, ConfigParseError> {
    let config_base_path = config_base_path.into();
    Ok(
        normalized_config_dir_value_path(spec.text.as_js(), config_base_path)
            .unwrap_or_else(|| normalized_config_value_path(spec.text.as_js(), &spec.base_path)),
    )
}

fn config_host_spec<'j0>(
    spec: &ConfigSpec,
    config_base_path: impl Into<JsStr<'j0>>,
) -> Result<JsString, ConfigParseError> {
    let config_base_path = config_base_path.into();
    Ok(
        normalized_config_dir_value_path(spec.text.as_js(), config_base_path)
            .unwrap_or_else(|| spec.text.clone()),
    )
}

fn substitute_config_dir_string<'j0>(
    value: &mut JsString,
    config_base_path: impl Into<JsStr<'j0>>,
) -> Result<bool, ConfigParseError> {
    let config_base_path = config_base_path.into();
    let Some(substituted) = normalized_config_dir_value_path(value.as_js(), config_base_path)
    else {
        return Ok(false);
    };
    *value = substituted;
    Ok(true)
}

fn substitute_config_dir_typed_string_array<'j0>(
    values: &mut [ConfigTypedJsonValue],
    config_base_path: impl Into<JsStr<'j0>>,
) -> Result<bool, ConfigParseError> {
    let config_base_path = config_base_path.into();
    let mut changed = false;
    for value in values {
        let ConfigTypedJsonValue::Json(Value::String(value)) = value else {
            continue;
        };
        changed |= substitute_config_dir_string(value, config_base_path)?;
    }
    Ok(changed)
}

/// tsc's wildcardFileMap (getFileNamesFromConfigSpecs, 39612): insertion
/// ordered with one lookup per key. A removed key leaves a tombstone so the
/// positions of the others stay put; the final listing skips it.
#[derive(Default)]
struct OrderedFileMap {
    entries: Vec<Option<(JsString, JsString)>>,
    index: rustc_hash::FxHashMap<JsString, usize>,
}

impl OrderedFileMap {
    fn contains(&self, key: &JsString) -> bool {
        self.index.contains_key(key)
    }

    /// Append a key the caller has checked is absent.
    fn insert(&mut self, key: JsString, file: JsString) {
        self.index.insert(key.clone(), self.entries.len());
        self.entries.push(Some((key, file)));
    }

    fn remove(&mut self, key: &JsString) {
        if let Some(position) = self.index.remove(key) {
            self.entries[position] = None;
        }
    }

    fn into_files(self) -> impl Iterator<Item = JsString> {
        self.entries.into_iter().flatten().map(|(_, file)| file)
    }
}

fn has_higher_priority(
    file: JsStr<'_>,
    literal: &rustc_hash::FxHashSet<JsString>,
    wildcard: &OrderedFileMap,
    groups: &[&[&str]],
    case_sensitive: bool,
) -> bool {
    let Some(group) = groups.iter().find(|group| {
        group
            .iter()
            .any(|extension| file_extension_is(file, extension))
    }) else {
        return false;
    };
    for extension in *group {
        if file_extension_is(file, extension)
            && (*extension != ".ts" || !file_extension_is(file, ".d.ts"))
        {
            return false;
        }
        let candidate = file_name_key(change_extension(file, extension).as_js(), case_sensitive);
        if literal.contains(&candidate) || wildcard.contains(&candidate) {
            if *extension == ".d.ts"
                && (file_extension_is(file, ".js") || file_extension_is(file, ".jsx"))
            {
                continue;
            }
            return true;
        }
    }
    false
}

fn remove_lower_priority(
    file: JsStr<'_>,
    wildcard: &mut OrderedFileMap,
    groups: &[&[&str]],
    case_sensitive: bool,
) {
    let Some(group) = groups.iter().find(|group| {
        group
            .iter()
            .any(|extension| file_extension_is(file, extension))
    }) else {
        return;
    };
    for extension in group.iter().rev() {
        if file_extension_is(file, extension) {
            return;
        }
        let candidate = file_name_key(change_extension(file, extension).as_js(), case_sensitive);
        wildcard.remove(&candidate);
    }
}

fn change_extension(file: JsStr<'_>, extension: &str) -> JsString {
    let current = [
        ".d.ts", ".d.cts", ".d.mts", ".tsx", ".cts", ".mts", ".jsx", ".cjs", ".mjs", ".ts", ".js",
        ".json",
    ]
    .into_iter()
    .find(|candidate| file_extension_is(file, candidate));
    let mut changed = current
        .and_then(|current| file.strip_suffix(current))
        .unwrap_or(file)
        .to_owned();
    changed.push_str(extension);
    changed
}

fn file_extension_is<'p>(file: impl Into<JsStr<'p>>, extension: &str) -> bool {
    let file = file.into();
    file.as_bytes().len() > extension.len() && file.ends_with(extension)
}

fn canonical_key<'p>(path: impl Into<JsStr<'p>>, case_sensitive: bool) -> JsString {
    file_name_key(path.into(), case_sensitive)
}

fn normalized_path<'p, 'b>(
    path: impl Into<JsStr<'p>>,
    base: impl Into<JsStr<'b>>,
) -> Result<JsString, ConfigParseError> {
    let path = path.into();
    normalize_absolute_js_path_lexical(path, Some(base.into())).map_err(|error| {
        ConfigParseError::new_js(
            ConfigParseErrorKind::InvalidPath,
            Some(path.to_owned()),
            error.to_string(),
        )
    })
}

fn is_drive_rooted<'p>(path: impl Into<JsStr<'p>>) -> bool {
    let bytes = path.into().as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes.get(2) == Some(&b'/'))
}

fn join_path<'p, 'c>(parent: impl Into<JsStr<'p>>, child: impl Into<JsStr<'c>>) -> JsString {
    let mut parent = parent.into();
    let mut child = child.into();
    while let Some(trimmed) = parent.strip_suffix("/") {
        parent = trimmed;
    }
    while let Some(trimmed) = child.strip_prefix("/") {
        child = trimmed;
    }
    let mut result = parent.to_owned();
    result.push('/');
    result.push_js(child);
    result
}

fn js_directory_name<'p>(path: impl Into<JsStr<'p>>) -> JsString {
    crate::js_path::directory_name(path.into())
}
