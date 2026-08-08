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

use serde_json::{Map, Value};
use tsc_diagnostics::{
    gen, sort_and_dedupe_diagnostics, Diagnostic, DiagnosticMessage, DocumentVersion, MessageChain,
    TextSnapshot,
};
use tsc_host::{to_file_name_lower_case, CompilerHost, HostError, HostErrorKind, HostOperation};
use tsc_syntax::{NodeId, SourceFile, SyntaxKind};
use tsc_types::{js_number_to_string, CompilerOptionNumber, CompilerOptions, ModuleSuffix};

use crate::config_options::{
    compiler_option_declaration, compiler_option_spelling_suggestion,
    is_command_option_without_build, jsconfig_defaults, typescript_6_0_3_libraries,
    CompilerOptionListDescriptor, CompilerOptionListElementKind, CompilerOptionValueKind,
    JsConfigDefaultValue,
};
use crate::json::{
    convert_recoverable_json_node_to_value, convert_recoverable_json_source_file_to_value,
    decode_user_object_key, is_double_quoted_json_string, json_number_as_f64, json_object_get,
    json_object_own_get, json_parser_preflight, json_source_file_is_empty, JsonParserPreflight,
    RecoverableJsonValue,
};
use crate::library::LibraryCatalog;
use crate::loader::{
    load_program_with_root_reasons, ProgramLoadError, ProgramLoadLimits, RootFileReason,
};
use crate::module_resolution::{
    directory_name, normalize_absolute_path_lexical, normalized_root_parts, ModuleResolver,
};
use crate::path::ProgramPath;
use crate::prepared::{
    PathMapping, PreparedProgram, ProgramConfigFile, ProgramConfigSpan, ProgramOptions,
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
}

impl fmt::Display for ConfigHostOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::FileExists => "fileExists",
            Self::ReadFile => "readFile",
            Self::ReadDirectory => "readDirectory",
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
    path: String,
    detail: String,
}

impl ConfigHostError {
    pub fn new(
        operation: ConfigHostOperation,
        path: impl Into<String>,
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

    pub fn path(&self) -> &str {
        &self.path
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

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError>;

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError>;

    fn read_directory(
        &self,
        directory: &str,
        extensions: &[&str],
        excludes: Option<&[String]>,
        includes: Option<&[String]>,
        depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError>;
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
    path: Option<String>,
    detail: String,
    diagnostics: Vec<Diagnostic>,
    host_error: Option<Box<ConfigHostError>>,
}

impl ConfigParseError {
    fn new(kind: ConfigParseErrorKind, path: Option<String>, detail: impl Into<String>) -> Self {
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

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
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
    pub file_name: String,
    snapshot: Arc<TextSnapshot>,
}

impl ConfigSourceText {
    pub fn new(file_name: impl Into<String>, text: impl Into<String>) -> Self {
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
    pub name: String,
    pub value: Value,
    pub base_path: String,
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
    entry_indices: BTreeMap<String, usize>,
    typed_entries: Vec<ConfigTypedOption>,
    typed_indices: BTreeMap<String, usize>,
    raw_order: Vec<String>,
    raw_indices: BTreeMap<String, usize>,
    removed_names: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct ConfigTypedOption {
    name: String,
    value: Option<ConfigTypedOptionValue>,
}

#[derive(Clone, Debug, PartialEq)]
enum ConfigTypedOptionValue {
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
    name: String,
    value: Option<ConfigTypedJsonValue>,
}

impl ConfigTypedObjectProperty {
    pub fn name(&self) -> &str {
        &self.name
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
                    serde_json::from_str(&js_number_to_string(number))
                        .expect("a finite JavaScript number string is valid JSON")
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

    fn append_compiler_option_cache_identity(&self, result: &mut String) {
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
            Self::Json(Value::String(value)) => result.push_str(value),
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
    fn new(
        shape: ConfigTypedObjectShape,
        mut properties: Vec<ConfigTypedObjectProperty>,
        inherits_proto_setter: bool,
    ) -> Self {
        // Object.keys observes array-index properties first in ascending
        // numeric order, followed by other strings in first-insertion order.
        properties.sort_by_cached_key(|property| {
            javascript_array_index(&property.name)
                .map(|index| (0_u8, index))
                .unwrap_or((1, 0))
        });
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
    pub fn compiler_option_cache_identity(&self) -> String {
        let mut result = String::new();
        self.append_compiler_option_cache_identity(&mut result);
        result
    }

    fn append_compiler_option_cache_identity(&self, result: &mut String) {
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
                    result.push_str(&property.name);
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

    fn finalize_config_dir_templates(
        &mut self,
        config_base_path: &str,
    ) -> Result<(), ConfigParseError> {
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

    pub fn get(&self, name: &str) -> Option<&ConfigOption> {
        self.entry_indices
            .get(name)
            .map(|index| &self.entries[*index])
    }

    /// Stored `pathsBasePath` compiler-option value. It can remain inherited
    /// when an own null or invalid `paths` masks the effective map, so this is
    /// deliberately not TypeScript's `getPathsBasePath` result. It is absent
    /// from the raw [`Self::entries`] and [`Self::get`] views.
    pub fn stored_paths_base_path(&self) -> Option<&str> {
        self.typed_value("pathsBasePath").and_then(Value::as_str)
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

    /// Ordered own-property view for an object-like compiler option. Numeric
    /// array-index keys follow JavaScript's ascending order; other keys retain
    /// their first insertion slots, including own `undefined` values.
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

    fn insert(&mut self, option: ConfigOption) {
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

    fn remove(&mut self, name: &str) {
        self.observe_raw_name(name);
        if let Some(index) = self.entry_indices.remove(name) {
            self.entries.swap_remove(index);
            if let Some(moved) = self.entries.get(index) {
                self.entry_indices.insert(moved.name.clone(), index);
            }
        }
        self.removed_names.insert(name.to_owned());
    }

    fn observe_raw_name(&mut self, name: &str) {
        if self.raw_indices.contains_key(name) {
            return;
        }
        let index = self.raw_order.len();
        let name = name.to_owned();
        self.raw_order.push(name.clone());
        self.raw_indices.insert(name, index);
    }

    fn insert_typed(&mut self, name: impl Into<String>, value: Option<ConfigTypedOptionValue>) {
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

    fn extend_from(&mut self, other: &Self) {
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
    fn finalize_config_dir_templates(
        &mut self,
        config_base_path: &str,
    ) -> Result<(), ConfigParseError> {
        for option in &mut self.typed_entries {
            let Some(declaration) = compiler_option_declaration(&option.name) else {
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
    out_dir: Option<String>,
    declaration_dir: Option<String>,
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

    pub fn out_dir(&self) -> Option<&str> {
        self.out_dir.as_deref()
    }

    pub fn declaration_dir(&self) -> Option<&str> {
        self.declaration_dir.as_deref()
    }
}

#[derive(Clone, Debug)]
pub struct ConfigRootPlanRequest {
    pub file_name: String,
    pub text: String,
    pub base_path: String,
}

/// One normalized `ParsedCommandLine.projectReferences` entry.  The H0
/// loader still rejects non-empty project references at execution time, but
/// parsing must retain the same primary-config observation for embeddings and
/// diagnostics that inspect a partial command line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigProjectReference {
    pub path: String,
    pub original_path: String,
    pub prepend: Option<bool>,
    pub circular: Option<bool>,
}

/// One `ParsedCommandLine.wildcardDirectories` entry.  TypeScript encodes the
/// flag as `Recursive=1` or `None=0`; a bool keeps that boundary explicit and
/// avoids exposing the internal watcher enum to the no-emit loader.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigWildcardDirectory {
    pub path: String,
    pub recursive: bool,
}

/// Program-owned root-planning projection, not a complete `ParsedCommandLine`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigRootPlan {
    config_file_name: String,
    source: ConfigSourceText,
    extended_sources: Vec<ConfigSourceText>,
    raw: Value,
    options: ConfigOptionBag,
    discovery_options: ConfigDiscoveryOptions,
    module_resolution_options: ConfigModuleResolutionOptions,
    /// Effective root specs after the extends merge. These remain separate
    /// from `file_names`: TypeScript exposes both the declarative
    /// `ParsedCommandLine` lists and the discovered file-name projection.
    files: Option<Vec<String>>,
    include: Option<Vec<String>>,
    exclude: Option<Vec<String>>,
    /// Root-level `references` are observable on the primary config only;
    /// TypeScript does not inherit them through `extends`.
    references: Option<Value>,
    project_references: Option<Vec<ConfigProjectReference>>,
    /// These root schemas are inherited by `extends` and retained as raw
    /// recovered values for the ParsedCommandLine-facing boundary. The
    /// no-emit loader still rejects truthy values before source loading.
    watch_options: Option<Value>,
    type_acquisition: Option<Value>,
    compile_on_save: Option<Value>,
    /// Truthy root-level schemas which the single-project no-emit loader does
    /// not consume. Keep this separate from `raw`: `raw` is intentionally a
    /// projection of the primary config and therefore cannot, by itself,
    /// distinguish a value inherited from an `extends` source.
    unsupported_root_scopes: BTreeSet<String>,
    file_names: Vec<String>,
    root_reasons: Vec<RootFileReason>,
    wildcard_directories: Vec<ConfigWildcardDirectory>,
    root_parse_diagnostics: Vec<Diagnostic>,
    errors: Vec<Diagnostic>,
    option_diagnostics: Vec<Diagnostic>,
    extended_source_files: Vec<String>,
}

impl ConfigRootPlan {
    pub fn config_file_name(&self) -> &str {
        &self.config_file_name
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
    pub fn files(&self) -> Option<&[String]> {
        self.files.as_deref()
    }

    /// Effective `include` entries after extends rebasing.
    pub fn include(&self) -> Option<&[String]> {
        self.include.as_deref()
    }

    /// Effective `exclude` entries after extends rebasing.
    pub fn exclude(&self) -> Option<&[String]> {
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

    /// Effective raw `watchOptions` after `extends` merging.
    pub fn watch_options(&self) -> Option<&Value> {
        self.watch_options.as_ref()
    }

    /// Effective raw `typeAcquisition` after `extends` merging.
    pub fn type_acquisition(&self) -> Option<&Value> {
        self.type_acquisition.as_ref()
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

    pub fn file_names(&self) -> &[String] {
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

    /// TypeScript's identity-only `extendedSourceFiles` projection. Unlike
    /// `extended_sources`, this also represents an explicitly resolved config
    /// whose read failed and therefore has no source text.
    pub fn extended_source_files(&self) -> &[String] {
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
    Program(ProgramLoadError),
}

impl ConfigProgramLoadError {
    pub fn config_diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Diagnostics { config, .. } => config,
            Self::NoEmitRequired { .. } | Self::Program(_) => &[],
        }
    }

    pub fn options_diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Diagnostics { options, .. } => options,
            Self::NoEmitRequired { .. } | Self::Program(_) => &[],
        }
    }

    pub const fn program_error(&self) -> Option<&ProgramLoadError> {
        match self {
            Self::Program(error) => Some(error),
            Self::Diagnostics { .. } | Self::NoEmitRequired { .. } => None,
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
            Self::Program(error) => error.fmt(formatter),
        }
    }
}

impl Error for ConfigProgramLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Program(error) => Some(error),
            Self::Diagnostics { .. } | Self::NoEmitRequired { .. } => None,
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
    load_config_program_inner(host, plan, library_catalog, limits, false)
}

/// Load a config plan while applying the command-line `--noEmit` override.
///
/// TypeScript gives an explicit command-line value precedence over the config
/// file. The override is deliberately limited to `noEmit`; config and fatal
/// option diagnostics remain a gate and no other option is silently mutated.
pub fn load_config_program_with_no_emit_override(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    load_config_program_inner(host, plan, library_catalog, limits, true)
}

/// Validate the config-facing gates without starting source discovery.
///
/// Embedding runners such as the upstream project harness may need to apply
/// their own `existingOptions` projection before calling `load_program`.  The
/// validation must nevertheless remain owned by this crate so those runners
/// cannot accidentally bypass config diagnostics or the H0 fail-closed
/// option/root-scope boundary.
pub fn validate_config_plan(plan: &ConfigRootPlan) -> Result<(), ConfigProgramLoadError> {
    validate_config_plan_with_no_emit_override(plan, false)
}

fn validate_config_plan_with_no_emit_override(
    plan: &ConfigRootPlan,
    force_no_emit: bool,
) -> Result<(), ConfigProgramLoadError> {
    let config = plan.diagnostics().cloned().collect::<Vec<_>>();
    // TypeScript reports deprecation diagnostics from getOptionsDiagnostics
    // but still constructs and checks the program. Keep those non-fatal rows
    // out of the source-loading gate; malformed option values and structural
    // validation diagnostics remain fatal and fail closed before host work.
    let options = plan
        .option_diagnostics()
        .iter()
        .filter(|diagnostic| {
            !(is_non_fatal_option_diagnostic(diagnostic)
                || force_no_emit && diagnostic.code() == 5096)
        })
        .cloned()
        .collect::<Vec<_>>();
    if !config.is_empty() || !options.is_empty() {
        return Err(ConfigProgramLoadError::Diagnostics { config, options });
    }

    if let Some((feature, detail)) =
        unsupported_config_scope(&plan.options, &plan.raw, plan.unsupported_root_scopes())
    {
        return Err(ConfigProgramLoadError::Program(
            ProgramLoadError::unsupported(
                crate::loader::ProgramLoadOperation::ValidateOptions,
                Some(PathBuf::from(plan.config_file_name())),
                feature,
                detail,
            ),
        ));
    }
    Ok(())
}

/// Whether an option diagnostic is reportable while the program still enters
/// the checker. TypeScript 6.0 deprecation rows are non-fatal; malformed
/// values and structural option errors remain a source-loading gate.
pub fn is_non_fatal_option_diagnostic(diagnostic: &Diagnostic) -> bool {
    matches!(diagnostic.code(), 5101 | 5107)
}

fn load_config_program_inner(
    host: &dyn CompilerHost,
    plan: &ConfigRootPlan,
    library_catalog: &LibraryCatalog,
    limits: ProgramLoadLimits,
    force_no_emit: bool,
) -> Result<PreparedProgram, ConfigProgramLoadError> {
    validate_config_plan_with_no_emit_override(plan, force_no_emit)?;

    if !force_no_emit && plan.compiler_options().no_emit != Some(true) {
        return Err(ConfigProgramLoadError::NoEmitRequired {
            value: plan.compiler_options().no_emit,
        });
    }

    let roots = plan
        .file_names()
        .iter()
        .zip(&plan.root_reasons)
        .map(|(file_name, reason)| (PathBuf::from(file_name), reason.clone()))
        .collect::<Vec<_>>();
    let mut compiler_options = plan.compiler_options().clone();
    if force_no_emit {
        compiler_options.no_emit = Some(true);
    }
    load_program_with_root_reasons(
        host,
        &roots,
        compiler_options,
        plan.program_options().clone(),
        library_catalog,
        limits,
    )
    .map_err(ConfigProgramLoadError::Program)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigLocation {
    file_name: String,
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
    file_name: Option<String>,
    compiler_options_name: Option<ConfigSpan>,
    mapping_locations: BTreeMap<String, ConfigPathsKeySyntax>,
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
    text: String,
    base_path: String,
    location: Option<ConfigLocation>,
}

#[derive(Clone, Debug, PartialEq)]
struct ConfigExtendsSpec {
    text: String,
    location: Option<ConfigLocation>,
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedConfigNode {
    source: ConfigSourceText,
    raw: Value,
    raw_property_names: BTreeSet<String>,
    options: ConfigOptionBag,
    files: Option<Vec<ConfigSpec>>,
    files_location: Option<ConfigLocation>,
    include: Option<Vec<ConfigSpec>>,
    exclude: Option<Vec<ConfigSpec>>,
    inheritable_files: Option<Vec<ConfigSpec>>,
    inheritable_include: Option<Vec<ConfigSpec>>,
    inheritable_exclude: Option<Vec<ConfigSpec>>,
    references: Option<Value>,
    watch_options: Option<Value>,
    type_acquisition: Option<Value>,
    compile_on_save: Option<Value>,
    unsupported_root_scopes: BTreeSet<String>,
    extended_sources: Vec<ConfigSourceText>,
    extended_source_files: Vec<String>,
}

struct ParseContext<'a> {
    host: &'a dyn ConfigParseHost,
    stack: Vec<String>,
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
    let config_file_name = normalized_path(&request.file_name, &request.base_path)?;
    let config_base = directory_name(&config_file_name);
    let mut context = ParseContext {
        host,
        stack: Vec::new(),
        root_parse_diagnostics: Vec::new(),
        errors: Vec::new(),
    };
    let mut node = context
        .parse_node(
            ConfigSourceText::new(request.file_name, request.text),
            &config_file_name,
            &config_base,
            true,
        )?
        .expect("the primary config cannot be a recursive child of itself");
    node.options.finalize_config_dir_templates(&config_base)?;
    let mut option_diagnostics = paths_option_diagnostics(&node.options, &node.source);
    option_diagnostics.extend(no_lib_lib_option_diagnostics(&node.options, &node.source));
    option_diagnostics.extend(deprecation_option_diagnostics(&node.options, &node.source));
    option_diagnostics.extend(option_relationship_diagnostics(&node.options, &node.source));
    sort_and_dedupe_diagnostics(&mut option_diagnostics);
    let discovery_options = effective_discovery_options(&node.options, &config_base)?;
    let module_resolution_options = config_module_resolution_options(
        &node.options,
        &discovery_options,
        &config_file_name,
        &node.source,
        host.use_case_sensitive_file_names(),
    )?;
    let file_names = derive_file_names(
        host,
        &node,
        &config_base,
        &config_file_name,
        &discovery_options,
        &mut context.errors,
    )?;
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
        watch_options: node.watch_options,
        type_acquisition: node.type_acquisition,
        compile_on_save: node.compile_on_save,
        unsupported_root_scopes: node.unsupported_root_scopes,
        file_names,
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
fn unsupported_config_scope(
    options: &ConfigOptionBag,
    raw: &Value,
    unsupported_root_scopes: impl IntoIterator<Item = impl AsRef<str>>,
) -> Option<(&'static str, String)> {
    if let Some(references) = raw.as_object().and_then(|raw| raw.get("references")) {
        if config_value_requests_feature(references) {
            return Some((
                "project-references",
                "project references are outside the H0 single-project driver".to_owned(),
            ));
        }
    }

    if let Some(scope) = unsupported_root_scopes.into_iter().next() {
        let scope = scope.as_ref();
        let detail = match scope {
            "watchOptions" => "watchOptions are outside the H0 single-project no-emit driver",
            "typeAcquisition" => "typeAcquisition is outside the H0 single-project no-emit driver",
            "compileOnSave" => "compileOnSave is outside the H0 single-project no-emit driver",
            _ => "root config scope is outside the H0 single-project no-emit driver",
        };
        return Some(("unsupported-config-scope", detail.to_owned()));
    }

    for option in options.entries() {
        if !config_option_is_supported_by_h0(&option.name)
            && config_value_requests_feature(&option.value)
        {
            return Some((
                "unsupported-config-option",
                format!(
                    "compiler option {:?} is outside the H0 single-project no-emit driver",
                    option.name
                ),
            ));
        }
    }
    None
}

fn config_project_references(
    references: Option<&Value>,
    config_base_path: &str,
) -> Option<Vec<ConfigProjectReference>> {
    let values = references?.as_array()?;
    let mut result = Vec::new();
    for reference in values {
        let Some(object) = reference.as_object() else {
            continue;
        };
        let Some(original_path) = object.get("path").and_then(Value::as_str) else {
            continue;
        };
        let Ok(path) = normalized_path(original_path, config_base_path) else {
            continue;
        };
        result.push(ConfigProjectReference {
            path,
            original_path: original_path.to_owned(),
            prepend: object.get("prepend").and_then(Value::as_bool),
            circular: object.get("circular").and_then(Value::as_bool),
        });
    }
    (!result.is_empty()).then_some(result)
}

fn derive_wildcard_directories(
    config: &ParsedConfigNode,
    config_base_path: &str,
    discovery: &ConfigDiscoveryOptions,
    case_sensitive: bool,
) -> Result<Vec<ConfigWildcardDirectory>, ConfigParseError> {
    // A `files` property disables wildcard discovery.  Otherwise TypeScript
    // supplies the implicit `**/*` include when `include` is absent.
    let includes = if config.files.is_some() {
        Vec::new()
    } else if let Some(includes) = &config.include {
        includes.clone()
    } else {
        vec![ConfigSpec {
            text: "**/*".to_owned(),
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
            to_file_name_lower_case(&path)
        };
        if let Some(existing) =
            directories
                .iter_mut()
                .find(|entry: &&mut ConfigWildcardDirectory| {
                    let existing_key = if case_sensitive {
                        entry.path.clone()
                    } else {
                        to_file_name_lower_case(&entry.path)
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

fn wildcard_spec_is_excluded(spec: &str, exclude: &str, case_sensitive: bool) -> bool {
    let normalize = |value: &str| {
        if case_sensitive {
            value.to_owned()
        } else {
            to_file_name_lower_case(value)
        }
    };
    let spec = normalize(spec);
    let exclude = normalize(exclude).trim_end_matches('/').to_owned();
    if !exclude.contains(['*', '?']) {
        return spec == exclude
            || spec
                .strip_prefix(&exclude)
                .is_some_and(|tail| tail.starts_with('/'));
    }
    ConfigFilePattern::new(&exclude, "/", case_sensitive)
        .ok()
        .flatten()
        .is_some_and(|pattern| pattern.matches(&spec))
}

fn wildcard_directory_from_spec(spec: &str) -> Option<(String, bool)> {
    let spec = spec.trim_end_matches('/');
    if spec.is_empty() {
        return None;
    }
    let last_separator = spec.rfind('/');
    let wildcard = spec.find(['*', '?']);
    if let Some(wildcard) = wildcard {
        let recursive = wildcard < last_separator.unwrap_or(spec.len());
        let path = if recursive {
            let component_separator = spec[..wildcard].rfind('/').unwrap_or(0);
            if component_separator == 0 {
                "/"
            } else {
                &spec[..component_separator]
            }
        } else {
            last_separator
                .map(|index| if index == 0 { "/" } else { &spec[..index] })
                .unwrap_or(".")
        };
        return Some((path.to_owned(), recursive));
    }

    // `include: ["src"]` is TypeScript's implicit recursive glob.
    let file_name = last_separator
        .map(|index| &spec[index + 1..])
        .unwrap_or(spec);
    if !file_name.contains('.') {
        return Some((spec.to_owned(), true));
    }
    None
}

fn same_path(left: &str, right: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        left == right
    } else {
        to_file_name_lower_case(left) == to_file_name_lower_case(right)
    }
}

fn path_is_descendant(parent: &str, child: &str, case_sensitive: bool) -> bool {
    let parent = if case_sensitive {
        parent.to_owned()
    } else {
        to_file_name_lower_case(parent)
    };
    let child = if case_sensitive {
        child.to_owned()
    } else {
        to_file_name_lower_case(child)
    };
    child
        .strip_prefix(parent.trim_end_matches('/'))
        .is_some_and(|tail| tail.starts_with('/'))
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

fn config_option_is_supported_by_h0(name: &str) -> bool {
    H0_SUPPORTED_CONFIG_OPTIONS.contains(&name)
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

impl ParseContext<'_> {
    fn parse_node(
        &mut self,
        source: ConfigSourceText,
        normalized_file_name: &str,
        base_path: &str,
        is_root: bool,
    ) -> Result<Option<ParsedConfigNode>, ConfigParseError> {
        match json_parser_preflight(source.text()) {
            JsonParserPreflight::Safe => {}
            JsonParserPreflight::UnsafeSyntax => {
                return Err(ConfigParseError::new(
                    ConfigParseErrorKind::Unsupported,
                    Some(source.file_name.clone()),
                    "config source uses syntax outside the bounded JSONC grammar",
                ));
            }
            JsonParserPreflight::ResourceLimit => {
                return Err(ConfigParseError::new(
                    ConfigParseErrorKind::ResourceLimit,
                    Some(source.file_name.clone()),
                    "config JSON nesting exceeds the 256-level parser limit",
                ));
            }
        }
        let parsed = tsc_syntax::parse_json_text_from_snapshot(
            &source.file_name,
            Arc::clone(source.snapshot()),
        );
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
            return Err(ConfigParseError::new(
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
                    ConfigParseError::new(
                        ConfigParseErrorKind::Unsupported,
                        Some(source.file_name.clone()),
                        "the cyclic config syntax tree is outside the currently ported JSONC conversion surface",
                    )
                })?;
            }
            let mut cycle = self.stack.clone();
            cycle.push(cache_key);
            self.errors.push(config_diagnostic(
                &gen::Circularity_detected_while_resolving_configuration_0,
                &[cycle.join(" -> ")],
                None,
            ));
            self.errors
                .extend(config_json_cycle_conversion_diagnostics(&parsed));
            return Ok(None);
        }
        self.stack.push(cache_key.clone());
        let result = self.parse_node_uncached(source, parsed, normalized_file_name, base_path);
        self.stack.pop();
        result
    }

    fn parse_node_uncached(
        &mut self,
        source: ConfigSourceText,
        parsed: SourceFile,
        normalized_file_name: &str,
        base_path: &str,
    ) -> Result<Option<ParsedConfigNode>, ConfigParseError> {
        let mut own_errors = config_json_conversion_diagnostics(&parsed);
        let json_conversion_error_count = own_errors.len();
        let mut raw = if json_source_file_is_empty(&parsed) {
            Value::Object(Map::new())
        } else {
            convert_recoverable_json_source_file_to_value(&parsed).ok_or_else(|| {
                ConfigParseError::new(
                    ConfigParseErrorKind::Unsupported,
                    Some(source.file_name.clone()),
                    "the recovered config syntax tree is outside the currently ported JSONC conversion surface",
                )
            })?
        };
        if !raw.is_object() {
            let config_kind =
                if source.file_name.rsplit(['/', '\\']).next() == Some("jsconfig.json") {
                    "jsconfig.json"
                } else {
                    "tsconfig.json"
                };
            own_errors.push(config_diagnostic(
                &gen::The_root_value_of_a_0_file_must_be_an_object,
                &[config_kind.to_owned()],
                config_root_expression(&parsed).and_then(|node| config_location(&parsed, node)),
            ));
            raw = raw
                .as_array()
                .and_then(|values| values.iter().find(|value| value.is_object()))
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
        }
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
        let own_watch_options_present = raw_property_names.contains("watchOptions");
        let own_watch_options =
            config_property_get(object, &raw_property_names, "watchOptions").cloned();
        let own_type_acquisition_present = raw_property_names.contains("typeAcquisition");
        let own_type_acquisition =
            config_property_get(object, &raw_property_names, "typeAcquisition").cloned();
        let own_compile_on_save_present = raw_property_names.contains("compileOnSave");
        let own_compile_on_save =
            config_property_get(object, &raw_property_names, "compileOnSave").cloned();

        let mut own_options = default_compiler_options(normalized_file_name, base_path);
        let mut converted_own_options = compiler_options(base_path, &parsed, &mut own_errors)?;
        // parseConfig records the declaring config directory beside every
        // truthy own `paths` value before extends are merged. An invalid or
        // null own value masks inherited paths but deliberately leaves an
        // inherited pathsBasePath untouched, matching ordinary JavaScript
        // assignment of the two independent option properties.
        if converted_own_options.typed_object_value("paths").is_some() {
            converted_own_options.insert_typed(
                "pathsBasePath",
                Some(ConfigTypedOptionValue::Json(Value::String(
                    base_path.to_owned(),
                ))),
            );
        }
        own_options.extend_from(&converted_own_options);
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
                &[],
                config_location(&parsed, property.name_node),
            ));
        }
        // applyExtendedConfig uses ordinary JavaScript property access here,
        // so JSONC `__proto__` values can block or supply inheritance even
        // though the final config-file-spec pass accepts own properties only.
        let blocks_inherited_files = property_is_truthy(object, &raw_property_names, "files");
        let blocks_inherited_include = property_is_truthy(object, &raw_property_names, "include");
        let blocks_inherited_exclude = property_is_truthy(object, &raw_property_names, "exclude");
        let has_own_files = own_files.is_some();
        let has_own_include = own_include.is_some();
        let has_own_exclude = own_exclude.is_some();

        let mut inherited_options = ConfigOptionBag::default();
        let mut inherited_files = None;
        let mut inherited_include = None;
        let mut inherited_exclude = None;
        let mut inherited_watch_options = None;
        let mut inherited_type_acquisition = None;
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
        let misplaced_root_option =
            config_property_get(object, &raw_property_names, "compilerOptions")
                .is_none()
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
            if seen_source_files.insert(extended_path.clone()) {
                extended_source_files.push(extended_path.clone());
            }
            let text = match self.host.read_file(&extended_path) {
                Ok(Some(text)) => text,
                Ok(None) => {
                    self.errors.push(config_diagnostic(
                        &gen::Cannot_read_file_0,
                        std::slice::from_ref(&extended_path),
                        None,
                    ));
                    continue;
                }
                Err(error) => {
                    self.errors.push(config_diagnostic(
                        &gen::Cannot_read_file_0_1,
                        &[extended_path.clone(), error.detail().to_owned()],
                        None,
                    ));
                    continue;
                }
            };
            let extended_base = directory_name(&extended_path);
            let extended_source = ConfigSourceText::new(extended_path.clone(), text);
            if seen_sources.insert(extended_path.clone()) {
                extended_sources.push(extended_source.clone());
            }
            let Some(extended) =
                self.parse_node(extended_source, &extended_path, &extended_base, false)?
            else {
                continue;
            };
            inherited_options.extend_from(&extended.options);
            if !blocks_inherited_files && extended.inheritable_files.is_some() {
                inherited_files = Some(rebase_config_specs(
                    extended.inheritable_files.as_deref().unwrap_or(&[]),
                    base_path,
                    self.host.use_case_sensitive_file_names(),
                )?);
            }
            if !blocks_inherited_include && extended.inheritable_include.is_some() {
                inherited_include = Some(rebase_config_specs(
                    extended.inheritable_include.as_deref().unwrap_or(&[]),
                    base_path,
                    self.host.use_case_sensitive_file_names(),
                )?);
            }
            if !blocks_inherited_exclude && extended.inheritable_exclude.is_some() {
                inherited_exclude = Some(rebase_config_specs(
                    extended.inheritable_exclude.as_deref().unwrap_or(&[]),
                    base_path,
                    self.host.use_case_sensitive_file_names(),
                )?);
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
            if extended.watch_options.is_some() {
                inherited_watch_options = extended.watch_options.clone();
            }
            if extended.type_acquisition.is_some() {
                inherited_type_acquisition = extended.type_acquisition.clone();
            }
            if extended.compile_on_save.is_some() {
                inherited_compile_on_save = extended.compile_on_save.clone();
            }
        }
        inherited_options.extend_from(&own_options);
        own_options = inherited_options;

        let files = own_files.or(inherited_files);
        let files_location = has_own_files
            .then(|| config_property_initializer(&parsed, "files"))
            .flatten()
            .and_then(|node| config_location(&parsed, node));
        let include = own_include.or(inherited_include);
        let exclude = own_exclude.or(inherited_exclude);
        let watch_options = if own_watch_options_present {
            own_watch_options
        } else {
            inherited_watch_options
        };
        let type_acquisition = if own_type_acquisition_present {
            own_type_acquisition
        } else {
            inherited_type_acquisition
        };
        let compile_on_save = if own_compile_on_save_present {
            own_compile_on_save
        } else {
            inherited_compile_on_save
        };
        for (name, value) in [
            ("watchOptions", watch_options.as_ref()),
            ("typeAcquisition", type_acquisition.as_ref()),
            ("compileOnSave", compile_on_save.as_ref()),
        ] {
            if value.is_some_and(json_value_is_truthy) {
                unsupported_root_scopes.insert(name.to_owned());
            }
        }
        let raw_object = raw
            .as_object_mut()
            .expect("config raw was validated as an object");
        for (name, was_own, specs) in [
            ("files", has_own_files, files.as_ref()),
            ("include", has_own_include, include.as_ref()),
            ("exclude", has_own_exclude, exclude.as_ref()),
        ] {
            if !was_own {
                if let Some(specs) = specs {
                    raw_object.insert(
                        name.to_owned(),
                        Value::Array(
                            specs
                                .iter()
                                .map(|spec| Value::String(spec.text.clone()))
                                .collect(),
                        ),
                    );
                    raw_property_names.insert(name.to_owned());
                }
            }
        }
        for (name, value) in [
            ("watchOptions", watch_options.as_ref()),
            ("typeAcquisition", type_acquisition.as_ref()),
            ("compileOnSave", compile_on_save.as_ref()),
        ] {
            if !raw_property_names.contains(name) {
                if let Some(value) = value {
                    raw_object.insert(name.to_owned(), value.clone());
                    raw_property_names.insert(name.to_owned());
                }
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
            extended_sources,
            extended_source_files,
        }))
    }

    fn resolve_extends(
        &self,
        extends: &ConfigExtendsSpec,
        base_path: &str,
        errors: &mut Vec<Diagnostic>,
    ) -> Result<Option<String>, ConfigParseError> {
        let slashed = extends.text.replace('\\', "/");
        if slashed.starts_with('/')
            || is_drive_rooted(&slashed)
            || slashed.starts_with("./")
            || slashed.starts_with("../")
        {
            let candidate = normalized_path(&slashed, base_path)?;
            let candidate_exists = self.host.file_exists(&candidate)?;
            if candidate_exists || candidate.ends_with(".json") {
                return Ok(Some(candidate));
            }
            if !candidate.ends_with(".json") {
                let json = format!("{candidate}.json");
                if self.host.file_exists(&json)? {
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
                    vec!["extends".to_owned()],
                )
            } else {
                (&gen::File_0_not_found, vec![extends.text.clone()])
            };
            errors.push(config_diagnostic(message, &args, extends.location.clone()));
        }
        Ok(resolved)
    }

    fn resolve_package_extends(
        &self,
        specifier: &str,
        base_path: &str,
    ) -> Result<Option<String>, ConfigParseError> {
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
        let containing_file = PathBuf::from(join_path(base_path, "tsconfig.json"));
        match resolver
            .resolve_json_config(&containing_file, specifier)
            .map_err(config_error_from_resolution)?
        {
            ResolutionOutcome::Resolved(module) => module
                .resolved_file()
                .display()
                .to_str()
                .map(str::to_owned)
                .map(Some)
                .ok_or_else(|| {
                    ConfigParseError::new(
                        ConfigParseErrorKind::InvalidPath,
                        Some(module.resolved_file().display().display().to_string()),
                        "resolved config path is not valid Unicode",
                    )
                }),
            ResolutionOutcome::NotFound => Ok(None),
        }
    }
}

#[derive(Clone, Debug)]
struct ConfigPropertyNode {
    name: String,
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
    // root/compiler-option property and order the two phases explicitly.
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
            owner.file_name == diagnostic.file_name.as_deref().unwrap_or_default()
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
        if property.name == "compilerOptions" {
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

fn config_diagnostic(
    message: &'static DiagnosticMessage,
    args: &[String],
    location: Option<ConfigLocation>,
) -> Diagnostic {
    match location {
        Some(location) => Diagnostic::new(
            Some(location.file_name),
            Some(location.start),
            Some(location.length),
            MessageChain::new(message, args),
        ),
        None => Diagnostic::new(None, None, None, MessageChain::new(message, args)),
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
        let mut object_locations = BTreeMap::<String, ConfigPathsKeySyntax>::new();
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

struct PendingConfigPathsDiagnostic<'a> {
    key: &'a str,
    target: ConfigPathsDiagnosticLocation,
    message: &'static DiagnosticMessage,
    arguments: Vec<String>,
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
fn paths_option_diagnostics(
    options: &ConfigOptionBag,
    source: &ConfigSourceText,
) -> Vec<Diagnostic> {
    let pending = pending_paths_option_diagnostics(options);
    if pending.is_empty() {
        return Vec::new();
    }

    // Location indexing is used only by invalid `paths` configurations.
    // Valid large maps stay on the single-parse path and do not retain or sort
    // one syntax entry per substitution.
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let syntax = config_paths_syntax_index(&parsed);
    let mut diagnostics = Vec::with_capacity(pending.len());
    for pending in pending {
        push_paths_diagnostics(
            &mut diagnostics,
            &syntax,
            pending.key,
            pending.target,
            pending.message,
            &pending.arguments,
        );
    }
    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
}

/// Validate the `lib`/`noLib` option pair at the same post-conversion
/// boundary as TypeScript's `verifyCompilerOptions`. The two properties are
/// both diagnosed (rather than only the second one) and locations are tied to
/// every effective root-syntax occurrence, matching
/// `createOptionDiagnosticInObjectLiteralSyntax`.
///
/// tsc-port: verifyCompilerOptions @6.0.3 (lib/noLib block)
/// tsc-hash: 6cc5d6e4258b1645ed0788fb31322db101b9e6b9ae34f203e749610f23e48fb3
/// tsc-span: _tsc.js:124888-124890
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
                if matches!(property.name.as_str(), "lib" | "noLib") {
                    locations.push(config_location(&parsed, property.name_node));
                }
            }
        }
    }
    if locations.iter().all(Option::is_none) {
        locations.push(
            config_property(&parsed, "compilerOptions")
                .and_then(|property| config_location(&parsed, property.name_node)),
        );
    }

    locations
        .into_iter()
        .map(|location| {
            config_diagnostic(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["lib".to_owned(), "noLib".to_owned()],
                location,
            )
        })
        .collect()
}

/// Produce the non-fatal TypeScript 6.0 option-deprecation diagnostics owned
/// by `getOptionsDiagnostics`.  These diagnostics must remain attached to the
/// immutable config plan even though they do not prevent a no-emit program
/// from being constructed.
///
/// tsc-port: verifyDeprecatedCompilerOptions @6.0.3
/// tsc-hash: b6d09b278ef2bfb9854fcd27e61627d116411742e757e3113a5338df88bcb08d
/// tsc-span: _tsc.js:129942-130078
fn deprecation_option_diagnostics(
    options: &ConfigOptionBag,
    source: &ConfigSourceText,
) -> Vec<Diagnostic> {
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let compiler_properties = config_compiler_option_properties(&parsed);
    let fallback = config_property(&parsed, "compilerOptions")
        .and_then(|property| config_location(&parsed, property.name_node));
    let ignore_state = options.typed_value_state("ignoreDeprecations");
    let ignore = match ignore_state {
        ConfigOptionValueState::Value(Value::String(value)) => Some(value.as_str()),
        _ => None,
    };
    let ignore_invalid = match ignore_state {
        ConfigOptionValueState::Absent => false,
        ConfigOptionValueState::Value(Value::String(value)) => {
            !matches!(value.as_str(), "5.0" | "6.0")
        }
        ConfigOptionValueState::Value(_)
        | ConfigOptionValueState::Undefined
        | ConfigOptionValueState::List(_)
        | ConfigOptionValueState::Object(_)
        | ConfigOptionValueState::PositiveInfinity
        | ConfigOptionValueState::NegativeInfinity => true,
    };
    let mut diagnostics = Vec::new();

    if ignore_invalid {
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &fallback,
            "ignoreDeprecations",
            false,
            &gen::Invalid_value_for_ignoreDeprecations,
            &[],
        );
    }

    // A deprecation can be silenced only by the matching 6.0 suppression
    // version. `"5.0"` remains a valid value, but intentionally does not
    // silence options deprecated in 6.0.
    let silences_ts6 = ignore == Some("6.0");

    let target = config_option_i32(options, "target");
    if target == Some(0) {
        // ES3 was removed in 5.5, so ignoreDeprecations cannot silence this
        // row even when it is set to the current 6.0 version.
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &fallback,
            "target",
            false,
            &gen::Option_0_1_has_been_removed_Please_remove_it_from_your_configuration,
            &["target".to_owned(), "ES3".to_owned()],
        );
    }
    if !silences_ts6 {
        if target == Some(1) {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "target",
                "ES5",
                false,
            );
        }
        if config_option_bool(options, "alwaysStrict") == Some(false) {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "alwaysStrict",
                "false",
                false,
            );
        }
        if config_option_i32(options, "moduleResolution") == Some(1) {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "moduleResolution",
                "classic",
                false,
            );
        } else if config_option_i32(options, "moduleResolution") == Some(2) {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "moduleResolution",
                "node10",
                true,
            );
        }
        if options.typed_value("baseUrl").is_some() {
            emit_option_deprecation_name(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "baseUrl",
                true,
            );
        }
        if config_option_bool(options, "esModuleInterop") == Some(false) {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "esModuleInterop",
                "false",
                false,
            );
        }
        if config_option_bool(options, "allowSyntheticDefaultImports") == Some(false) {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "allowSyntheticDefaultImports",
                "false",
                false,
            );
        }
        if options.typed_value("outFile").is_some() {
            emit_option_deprecation_name(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "outFile",
                false,
            );
        }
        if config_option_bool(options, "downlevelIteration").is_some() {
            emit_option_deprecation_name(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "downlevelIteration",
                false,
            );
        }
        let module = config_option_i32(options, "module");
        let module_name = match module {
            Some(0) => Some("None"),
            Some(2) => Some("AMD"),
            Some(3) => Some("UMD"),
            Some(4) => Some("System"),
            _ => None,
        };
        if let Some(module_name) = module_name {
            emit_option_deprecation_5107(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &fallback,
                "module",
                module_name,
                false,
            );
        }
    }

    sort_and_dedupe_diagnostics(&mut diagnostics);
    diagnostics
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
    // Relationship diagnostics whose option is absent are compiler-level
    // rows in TypeScript, not diagnostics attached to the compilerOptions
    // object. Property-backed rows still use their exact value/key span.
    let no_fallback = None;
    let projected = CompilerOptions {
        target: config_option_i32(options, "target"),
        module: config_option_i32(options, "module"),
        module_resolution: config_option_i32(options, "moduleResolution"),
        ..CompilerOptions::default()
    };
    let module_kind = projected.emit_module_kind();
    let module_resolution = projected.emit_module_resolution_kind();
    let mut diagnostics = Vec::new();

    if module_resolution == 100 && !matches!(module_kind, 1 | 5..=99 | 200) {
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &no_fallback,
            "moduleResolution",
            false,
            &gen::Option_0_can_only_be_used_when_module_is_set_to_preserve_commonjs_or_es2015_or_later,
            &["bundler".to_owned()],
        );
    }

    if (3..=99).contains(&module_resolution) && !(100..=199).contains(&module_kind) {
        let module_resolution_name = if module_resolution == 99 {
            "NodeNext"
        } else {
            "Node16"
        };
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &no_fallback,
            "module",
            false,
            &gen::Option_module_must_be_set_to_0_when_option_moduleResolution_is_set_to_1,
            &[
                module_resolution_name.to_owned(),
                module_resolution_name.to_owned(),
            ],
        );
    } else if (100..=199).contains(&module_kind)
        && options.typed_value("moduleResolution").is_some()
        && !(3..=99).contains(&module_resolution)
    {
        let module_kind_name = if module_kind == 199 {
            "NodeNext"
        } else {
            "Node16"
        };
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &no_fallback,
            "moduleResolution",
            false,
            &gen::Option_moduleResolution_must_be_set_to_0_or_left_unspecified_when_option_module_is_set_to_1,
            &[
                module_kind_name.to_owned(),
                module_kind_name.to_owned(),
            ],
        );
    }

    let package_maps_supported = (3..=99).contains(&module_resolution) || module_resolution == 100;
    if !package_maps_supported {
        for name in ["resolvePackageJsonExports", "resolvePackageJsonImports"] {
            if config_option_bool(options, name) == Some(true) {
                emit_option_diagnostic_for_properties(
                    &mut diagnostics,
                    &parsed,
                    &compiler_properties,
                    &no_fallback,
                    name,
                    true,
                    &gen::Option_0_can_only_be_used_when_moduleResolution_is_set_to_node16_nodenext_or_bundler,
                    &[name.to_owned()],
                );
            }
        }
        if matches!(
            options.typed_value_state("customConditions"),
            ConfigOptionValueState::List(_)
        ) {
            emit_option_diagnostic_for_properties(
                &mut diagnostics,
                &parsed,
                &compiler_properties,
                &no_fallback,
                "customConditions",
                true,
                &gen::Option_0_can_only_be_used_when_moduleResolution_is_set_to_node16_nodenext_or_bundler,
                &["customConditions".to_owned()],
            );
        }
    }

    if config_option_bool(options, "verbatimModuleSyntax") == Some(true)
        && matches!(module_kind, 0 | 2..=4)
    {
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &no_fallback,
            "verbatimModuleSyntax",
            true,
            &gen::Option_verbatimModuleSyntax_cannot_be_used_when_module_is_set_to_UMD_AMD_or_System,
            &[],
        );
    }

    if config_option_bool(options, "allowImportingTsExtensions") == Some(true)
        && config_option_bool(options, "noEmit") != Some(true)
        && config_option_bool(options, "rewriteRelativeImportExtensions") != Some(true)
    {
        emit_option_diagnostic_for_properties(
            &mut diagnostics,
            &parsed,
            &compiler_properties,
            &no_fallback,
            "allowImportingTsExtensions",
            true,
            &gen::Option_allowImportingTsExtensions_can_only_be_used_when_one_of_noEmit_emitDeclarationOnly_or_rewriteRelativeImportExtensions_is_set,
            &[],
        );
    }

    diagnostics
}

fn config_compiler_option_properties(source: &SourceFile) -> Vec<ConfigPropertyNode> {
    let Some(root) = config_root_object(source) else {
        return Vec::new();
    };
    config_object_properties(source, root)
        .into_iter()
        .filter(|property| property.name == "compilerOptions")
        .flat_map(|property| config_object_properties(source, property.initializer))
        .collect()
}

fn emit_option_deprecation_5107(
    diagnostics: &mut Vec<Diagnostic>,
    source: &SourceFile,
    properties: &[ConfigPropertyNode],
    fallback: &Option<ConfigLocation>,
    name: &str,
    value: &str,
    related: bool,
) {
    let start = diagnostics.len();
    emit_option_diagnostic_for_properties(
        diagnostics,
        source,
        properties,
        fallback,
        name,
        false,
        &gen::Option_0_1_is_deprecated_and_will_stop_functioning_in_TypeScript_2_Specify_compilerOption_ignoreDeprecations_3_to_silence_this_error,
        &[
            name.to_owned(),
            value.to_owned(),
            "7.0".to_owned(),
            "6.0".to_owned(),
        ],
    );
    if related {
        // Replace only the rows created by this call. Other 5107 rows may
        // precede it in the same option pass (for example `module=AMD`).
        for diagnostic in &mut diagnostics[start..] {
            diagnostic.message = diagnostic.message.clone().with_next(vec![MessageChain::new(
                &gen::Visit_https_aka_ms_ts6_for_migration_information,
                &[],
            )]);
        }
    }
}

fn emit_option_deprecation_name(
    diagnostics: &mut Vec<Diagnostic>,
    source: &SourceFile,
    properties: &[ConfigPropertyNode],
    fallback: &Option<ConfigLocation>,
    name: &str,
    related: bool,
) {
    let start = diagnostics.len();
    emit_option_diagnostic_for_properties(
        diagnostics,
        source,
        properties,
        fallback,
        name,
        true,
        &gen::Option_0_is_deprecated_and_will_stop_functioning_in_TypeScript_1_Specify_compilerOption_ignoreDeprecations_2_to_silence_this_error,
        &[name.to_owned(), "7.0".to_owned(), "6.0".to_owned()],
    );
    if related {
        for diagnostic in &mut diagnostics[start..] {
            diagnostic.message = diagnostic.message.clone().with_next(vec![MessageChain::new(
                &gen::Visit_https_aka_ms_ts6_for_migration_information,
                &[],
            )]);
        }
    }
}

#[allow(clippy::too_many_arguments)] // Mirrors createOptionDiagnostic's location/message tuple.
fn emit_option_diagnostic_for_properties(
    diagnostics: &mut Vec<Diagnostic>,
    source: &SourceFile,
    properties: &[ConfigPropertyNode],
    fallback: &Option<ConfigLocation>,
    name: &str,
    on_key: bool,
    message: &'static DiagnosticMessage,
    arguments: &[String],
) {
    let mut emitted = false;
    for property in properties.iter().filter(|property| property.name == name) {
        let location = config_location(
            source,
            if on_key {
                property.name_node
            } else {
                property.initializer
            },
        );
        diagnostics.push(config_diagnostic(message, arguments, location));
        emitted = true;
    }
    if !emitted {
        diagnostics.push(config_diagnostic(message, arguments, fallback.clone()));
    }
}

fn pending_paths_option_diagnostics(
    options: &ConfigOptionBag,
) -> Vec<PendingConfigPathsDiagnostic<'_>> {
    let Some(paths) = options.typed_object_value("paths") else {
        return Vec::new();
    };
    let has_base_url = options
        .typed_value("baseUrl")
        .is_some_and(|value| value.as_str().is_some());
    let mut diagnostics = Vec::new();
    for mapping in paths.properties() {
        let key = mapping.name();
        if !has_zero_or_one_asterisk(key) {
            diagnostics.push(PendingConfigPathsDiagnostic {
                key,
                target: ConfigPathsDiagnosticLocation::Key,
                message: &gen::Pattern_0_can_have_at_most_one_character,
                arguments: vec![key.to_owned()],
            });
        }
        let Some(ConfigTypedJsonValue::Array(substitutions)) = mapping.value() else {
            diagnostics.push(PendingConfigPathsDiagnostic {
                key,
                target: ConfigPathsDiagnosticLocation::Value,
                message: &gen::Substitutions_for_pattern_0_should_be_an_array,
                arguments: vec![key.to_owned()],
            });
            continue;
        };
        if substitutions.is_empty() {
            diagnostics.push(PendingConfigPathsDiagnostic {
                key,
                target: ConfigPathsDiagnosticLocation::Value,
                message: &gen::Substitutions_for_pattern_0_shouldn_t_be_an_empty_array,
                arguments: vec![key.to_owned()],
            });
        }
        for (index, substitution) in substitutions.iter().enumerate() {
            if let ConfigTypedJsonValue::Json(Value::String(substitution)) = substitution {
                if !has_zero_or_one_asterisk(substitution) {
                    diagnostics.push(PendingConfigPathsDiagnostic {
                        key,
                        target: ConfigPathsDiagnosticLocation::Element(index),
                        message: &gen::Substitution_0_in_pattern_1_can_have_at_most_one_character,
                        arguments: vec![substitution.clone(), key.to_owned()],
                    });
                }
                if !has_base_url
                    && !config_path_is_relative(substitution)
                    && !config_path_is_absolute(substitution)
                {
                    diagnostics.push(PendingConfigPathsDiagnostic {
                        key,
                        target: ConfigPathsDiagnosticLocation::Element(index),
                        message: &gen::Non_relative_paths_are_not_allowed_when_baseUrl_is_not_set_Did_you_forget_a_leading,
                        arguments: Vec::new(),
                    });
                }
            } else {
                diagnostics.push(PendingConfigPathsDiagnostic {
                    key,
                    target: ConfigPathsDiagnosticLocation::Element(index),
                    message:
                        &gen::Substitution_0_for_pattern_1_has_incorrect_type_expected_string_got_2,
                    arguments: vec![
                        config_typed_json_to_js_string(substitution),
                        key.to_owned(),
                        config_typed_json_typeof(substitution).to_owned(),
                    ],
                });
            }
        }
    }
    diagnostics
}

fn push_paths_diagnostics(
    diagnostics: &mut Vec<Diagnostic>,
    syntax: &ConfigPathsSyntaxIndex,
    key: &str,
    target: ConfigPathsDiagnosticLocation,
    message: &'static DiagnosticMessage,
    args: &[String],
) {
    let key_syntax = syntax.mapping_locations.get(key);
    match target {
        ConfigPathsDiagnosticLocation::Key | ConfigPathsDiagnosticLocation::Value => {
            if let Some(key_syntax) = key_syntax {
                for mapping in &key_syntax.mapping_locations {
                    let span = match target {
                        ConfigPathsDiagnosticLocation::Key => mapping.key_location,
                        ConfigPathsDiagnosticLocation::Value => mapping.value_location,
                        ConfigPathsDiagnosticLocation::Element(_) => unreachable!(),
                    };
                    diagnostics.push(config_diagnostic(
                        message,
                        args,
                        config_paths_location(syntax, span),
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
                    diagnostics.push(config_diagnostic(
                        message,
                        args,
                        config_paths_location(syntax, span),
                    ));
                }
                return;
            }
        }
    }
    diagnostics.push(config_diagnostic(
        message,
        args,
        syntax
            .compiler_options_name
            .and_then(|span| config_paths_location(syntax, span)),
    ));
}

fn config_paths_location(
    syntax: &ConfigPathsSyntaxIndex,
    span: ConfigSpan,
) -> Option<ConfigLocation> {
    Some(ConfigLocation {
        file_name: syntax.file_name.clone()?,
        start: span.start,
        length: span.length,
    })
}

/// tsc-port: hasZeroOrOneAsteriskCharacter @6.0.3
/// tsc-hash: 28a64969081ad59009ed6f3fcb192a4ccef94471b01213a5de12c284cdd6eb45
/// tsc-span: _tsc.js:18318-18330
fn has_zero_or_one_asterisk(value: &str) -> bool {
    value.bytes().filter(|byte| *byte == b'*').take(2).count() <= 1
}

/// tsc-port: pathIsRelative @6.0.3
/// tsc-hash: f202555c891d7a914e21c5fe1199667a8d221940ce66c814b4898adfb228aac9
/// tsc-span: _tsc.js:5314-5316
fn config_path_is_relative(path: &str) -> bool {
    matches!(path, "." | "..")
        || path.starts_with("./")
        || path.starts_with(".\\")
        || path.starts_with("../")
        || path.starts_with("..\\")
}

/// tsc-port: pathIsAbsolute @6.0.3
/// tsc-hash: 0e64b150a899a6eb39ac2a3b370896f59ec02bdefdf07106a8740624318eb3f3
/// tsc-span: _tsc.js:5311-5313
/// tsc-port: getEncodedRootLength @6.0.3 (absolute/nonzero projection)
/// tsc-hash: ad42b701dd98c53ad89476947bccf551e3ab3db9ce0c9fc5009e16a41b49b1f9
/// tsc-span: _tsc.js:5349-5386
fn config_path_is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    if matches!(bytes.first(), Some(b'/' | b'\\')) {
        return true;
    }
    if bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && (bytes.len() == 2 || matches!(bytes.get(2), Some(b'/' | b'\\')))
    {
        return true;
    }
    // TypeScript recognizes URL roots only with the literal forward-slash
    // separator. Normalizing backslashes first would incorrectly treat
    // `scheme:\\host` as absolute and suppress TS5090.
    path.contains("://")
}

fn config_typed_json_typeof(value: &ConfigTypedJsonValue) -> &'static str {
    match value {
        ConfigTypedJsonValue::Json(Value::Null | Value::Array(_) | Value::Object(_))
        | ConfigTypedJsonValue::Array(_)
        | ConfigTypedJsonValue::Object(_) => "object",
        ConfigTypedJsonValue::Json(Value::Bool(_)) => "boolean",
        ConfigTypedJsonValue::Json(Value::Number(_)) => "number",
        ConfigTypedJsonValue::Json(Value::String(_)) => "string",
    }
}

fn config_typed_json_to_js_string(value: &ConfigTypedJsonValue) -> String {
    // TypeScript 6.0.3 can reach a Debug Failure while formatting a null
    // programmatic substitution. Config parsing should remain fail-safe after
    // publishing TS5064, so Rust applies JavaScript ToString to every retained
    // non-string shape instead of panicking at this diagnostic boundary.
    match value {
        ConfigTypedJsonValue::Json(value) => json_value_to_js_string(value),
        ConfigTypedJsonValue::Array(values) => values
            .iter()
            .map(|value| match value {
                ConfigTypedJsonValue::Json(Value::Null) => String::new(),
                value => config_typed_json_to_js_string(value),
            })
            .collect::<Vec<_>>()
            .join(","),
        ConfigTypedJsonValue::Object(_) => "[object Object]".to_owned(),
    }
}

fn json_value_to_js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => js_number_to_string(
            json_number_as_f64(value).expect("config JSON numbers have a JavaScript projection"),
        ),
        Value::String(value) => value.clone(),
        Value::Array(values) => values
            .iter()
            .map(|value| match value {
                Value::Null => String::new(),
                value => json_value_to_js_string(value),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
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

fn config_json_conversion_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
    // convertConfigFileToObject recovers only the first object from a root
    // array. Later array elements are not converted and therefore cannot
    // produce JSON conversion diagnostics such as TS1327.
    let Some(root) = config_root_object(source) else {
        return Vec::new();
    };
    config_json_conversion_diagnostics_from_root(source, root, ConfigJsonConversionContext::Root)
}

fn config_json_cycle_conversion_diagnostics(source: &SourceFile) -> Vec<Diagnostic> {
    // parseConfig's cycle arm calls convertToObject on the root expression,
    // so unlike the ordinary root-array recovery it also converts later array
    // elements. TS5092 itself is not emitted a second time.
    let Some(root) = config_root_expression(source) else {
        return Vec::new();
    };
    config_json_conversion_diagnostics_from_root(source, root, ConfigJsonConversionContext::Generic)
}

#[derive(Clone, Copy)]
enum ConfigJsonConversionContext {
    /// No option schema applies, so an unsupported value is TS1328.
    Generic,
    /// The ordinary top-level tsconfig option map.
    Root,
    /// The `compilerOptions` object and its known declaration lookup.
    CompilerOptions,
    /// A currently owned scalar option. Its direct invalid value is diagnosed
    /// by the existing notifier conversion, while nested structures lose that
    /// scalar schema and use ordinary JSON conversion diagnostics.
    KnownValue,
    /// A root string-list whose direct elements retain the named string
    /// schema.
    StringList(&'static str),
    /// `extends` accepts either a string or an array of strings.
    StringOrList(&'static str),
    /// One direct element of a root string-list. Unsupported syntax is a
    /// conversion-time TS5024 and is filtered before the later notifier.
    StringListElement(&'static str),
    /// The outer array of a known compiler list option. A direct invalid value
    /// is owned by the later option notifier, while a real array passes the
    /// element schema into `convertToJson`.
    CompilerOptionList(CompilerOptionListDescriptor),
    /// One direct compiler-list element. Unsupported syntax is diagnosed by
    /// `convertToJson` before its filtered array reaches the option notifier.
    CompilerOptionListElement(CompilerOptionListDescriptor),
    /// A nested list/object schema or remaining root field belongs to a later
    /// slice. Preserve TS1327 traversal without claiming its TS1328/TS5024
    /// option conversion yet.
    Unported,
}

#[derive(Clone, Copy)]
enum ConfigJsonConversionTask {
    Visit {
        node: NodeId,
        context: ConfigJsonConversionContext,
    },
    PropertyName(NodeId),
}

fn config_json_conversion_diagnostics_from_root(
    source: &SourceFile,
    root: NodeId,
    context: ConfigJsonConversionContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut stack = vec![ConfigJsonConversionTask::Visit {
        node: root,
        context,
    }];
    while let Some(task) = stack.pop() {
        let ConfigJsonConversionTask::Visit {
            node: node_id,
            context,
        } = task
        else {
            let ConfigJsonConversionTask::PropertyName(name) = task else {
                unreachable!("conversion diagnostic task kind is exhaustive")
            };
            if !is_double_quoted_json_string(source, name) {
                diagnostics.push(config_diagnostic(
                    &gen::String_literal_with_double_quotes_expected,
                    &[],
                    config_location(source, name),
                ));
            }
            continue;
        };
        let node = source.arena.node(node_id);
        match node.kind {
            SyntaxKind::StringLiteral => {
                if !is_double_quoted_json_string(source, node_id) {
                    diagnostics.push(config_diagnostic(
                        &gen::String_literal_with_double_quotes_expected,
                        &[],
                        config_location(source, node_id),
                    ));
                }
            }
            SyntaxKind::ArrayLiteralExpression => {
                if let Some(elements) = node
                    .data
                    .as_array_literal_expression()
                    .and_then(|array| array.elements)
                {
                    let element_context = match context {
                        ConfigJsonConversionContext::StringList(name)
                        | ConfigJsonConversionContext::StringOrList(name) => {
                            ConfigJsonConversionContext::StringListElement(name)
                        }
                        ConfigJsonConversionContext::CompilerOptionList(descriptor) => {
                            ConfigJsonConversionContext::CompilerOptionListElement(descriptor)
                        }
                        ConfigJsonConversionContext::Unported => {
                            ConfigJsonConversionContext::Unported
                        }
                        ConfigJsonConversionContext::Generic
                        | ConfigJsonConversionContext::Root
                        | ConfigJsonConversionContext::CompilerOptions
                        | ConfigJsonConversionContext::KnownValue
                        | ConfigJsonConversionContext::StringListElement(_)
                        | ConfigJsonConversionContext::CompilerOptionListElement(_) => {
                            ConfigJsonConversionContext::Generic
                        }
                    };
                    stack.extend(
                        source
                            .arena
                            .node_array(elements)
                            .nodes
                            .iter()
                            .rev()
                            .copied()
                            .map(|node| ConfigJsonConversionTask::Visit {
                                node,
                                context: element_context,
                            }),
                    );
                }
            }
            SyntaxKind::ObjectLiteralExpression => {
                if let Some(properties) = node
                    .data
                    .as_object_literal_expression()
                    .and_then(|object| object.properties)
                {
                    for property in source.arena.node_array(properties).nodes.iter().rev() {
                        let Some(property) =
                            source.arena.node(*property).data.as_property_assignment()
                        else {
                            continue;
                        };
                        if let Some(initializer) = property.initializer {
                            let property_name = property
                                .name
                                .and_then(|name| config_property_name(source, name));
                            let initializer_context = match context {
                                ConfigJsonConversionContext::Root => match property_name.as_deref()
                                {
                                    Some("compilerOptions") => {
                                        ConfigJsonConversionContext::CompilerOptions
                                    }
                                    Some("files") => {
                                        ConfigJsonConversionContext::StringList("files")
                                    }
                                    Some("include") => {
                                        ConfigJsonConversionContext::StringList("include")
                                    }
                                    Some("exclude") => {
                                        ConfigJsonConversionContext::StringList("exclude")
                                    }
                                    Some("extends") => {
                                        ConfigJsonConversionContext::StringOrList("extends")
                                    }
                                    Some(
                                        "watchOptions" | "typeAcquisition" | "references"
                                        | "compileOnSave",
                                    ) => ConfigJsonConversionContext::Unported,
                                    Some(_) | None => ConfigJsonConversionContext::Generic,
                                },
                                ConfigJsonConversionContext::CompilerOptions => match property_name
                                    .as_deref()
                                    .and_then(compiler_option_declaration)
                                {
                                    Some(declaration) => match declaration.value_kind() {
                                        CompilerOptionValueKind::List(descriptor) => {
                                            ConfigJsonConversionContext::CompilerOptionList(
                                                descriptor,
                                            )
                                        }
                                        _ => ConfigJsonConversionContext::KnownValue,
                                    },
                                    None => ConfigJsonConversionContext::Generic,
                                },
                                ConfigJsonConversionContext::Unported => {
                                    ConfigJsonConversionContext::Unported
                                }
                                ConfigJsonConversionContext::Generic
                                | ConfigJsonConversionContext::KnownValue
                                | ConfigJsonConversionContext::StringList(_)
                                | ConfigJsonConversionContext::StringOrList(_)
                                | ConfigJsonConversionContext::StringListElement(_)
                                | ConfigJsonConversionContext::CompilerOptionList(_)
                                | ConfigJsonConversionContext::CompilerOptionListElement(_) => {
                                    ConfigJsonConversionContext::Generic
                                }
                            };
                            stack.push(ConfigJsonConversionTask::Visit {
                                node: initializer,
                                context: initializer_context,
                            });
                        }
                        if let Some(name) = property.name {
                            stack.push(ConfigJsonConversionTask::PropertyName(name));
                        }
                    }
                }
            }
            SyntaxKind::NumericLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::PrefixUnaryExpression => {}
            _ if matches!(context, ConfigJsonConversionContext::Generic) => {
                diagnostics.push(config_diagnostic(
                    &gen::Property_value_can_only_be_string_literal_numeric_literal_true_false_null_object_literal_or_array_literal,
                    &[],
                    config_location(source, node_id),
                ));
            }
            _ if matches!(context, ConfigJsonConversionContext::StringListElement(_)) => {
                let ConfigJsonConversionContext::StringListElement(name) = context else {
                    unreachable!("string-list-element context was matched above")
                };
                diagnostics.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[name.to_owned(), "string".to_owned()],
                    config_location(source, node_id),
                ));
            }
            _ if matches!(
                context,
                ConfigJsonConversionContext::CompilerOptionListElement(_)
            ) =>
            {
                let ConfigJsonConversionContext::CompilerOptionListElement(descriptor) = context
                else {
                    unreachable!("list-element context was matched above")
                };
                diagnostics.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[
                        descriptor.element_name().to_owned(),
                        compiler_option_list_element_expected_type(descriptor).to_owned(),
                    ],
                    config_location(source, node_id),
                ));
            }
            _ => {}
        }
    }
    diagnostics
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

fn config_property_name(source: &SourceFile, name: NodeId) -> Option<String> {
    let node = source.arena.node(name);
    match node.kind {
        SyntaxKind::StringLiteral => node
            .data
            .as_string_literal()
            .map(|literal| literal.text.clone()),
        SyntaxKind::Identifier => node
            .data
            .as_identifier()
            .map(|identifier| identifier.text.clone()),
        SyntaxKind::NumericLiteral => node
            .data
            .as_numeric_literal()
            .map(|literal| literal.text.clone()),
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
        .map(|elements| source.arena.node_array(elements).nodes.clone())
        .unwrap_or_default()
}

fn config_spec_location(source: &SourceFile, name: &str, value: &str) -> Option<ConfigLocation> {
    let root = config_root_expression(source)?;
    if source.arena.node(root).kind != SyntaxKind::ObjectLiteralExpression {
        return None;
    }
    for property in config_object_properties(source, root) {
        if property.name != name {
            continue;
        }
        for element in config_array_elements(source, property.initializer) {
            if matches!(
                convert_recoverable_json_node_to_value(source, element),
                Some(RecoverableJsonValue::Defined(Value::String(written))) if written == value
            ) {
                return config_location(source, element);
            }
        }
    }
    None
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
    current_directory: &'a str,
}

impl CompilerHost for ConfigCompilerHostAdapter<'_> {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        Ok(PathBuf::from(self.current_directory))
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.host.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        let path = path.to_string_lossy();
        self.host
            .read_file(&path)
            .map(|text| text.map(String::into_bytes))
            .map_err(config_host_error_for_resolver)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        let path = path.to_string_lossy();
        self.host
            .file_exists(&path)
            .map_err(config_host_error_for_resolver)
    }

    fn directory_exists(&self, _path: &Path) -> Result<bool, HostError> {
        Ok(true)
    }

    fn read_directory(&self, _path: &Path) -> Result<Vec<PathBuf>, HostError> {
        Ok(Vec::new())
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        Ok(Some(path.to_path_buf()))
    }
}

fn config_host_error_for_resolver(error: ConfigHostError) -> HostError {
    let operation = match error.operation() {
        ConfigHostOperation::FileExists => HostOperation::FileExists,
        ConfigHostOperation::ReadFile => HostOperation::ReadFile,
        ConfigHostOperation::ReadDirectory => HostOperation::ReadDirectory,
    };
    HostError::new(
        HostErrorKind::Other,
        operation,
        Some(PathBuf::from(error.path())),
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
                _ => None,
            };
            if let Some(operation) = operation {
                return ConfigHostError::new(
                    operation,
                    error
                        .path()
                        .map_or_else(String::new, |path| path.display().to_string()),
                    error.detail().to_owned(),
                )
                .into();
            }
            ConfigParseError::new(
                ConfigParseErrorKind::Host,
                error.path().map(|path| path.display().to_string()),
                error.to_string(),
            )
        }
        ResolutionError::Unsupported { feature, detail } => ConfigParseError::new(
            ConfigParseErrorKind::Unsupported,
            None,
            format!("unsupported config resolution feature {feature}: {detail}"),
        ),
        ResolutionError::Canonicalization { path, detail } => ConfigParseError::new(
            ConfigParseErrorKind::InvalidPath,
            path.map(|path| path.display().to_string()),
            detail,
        ),
        ResolutionError::InvalidData(detail) => {
            ConfigParseError::new(ConfigParseErrorKind::InvalidConfig, None, detail)
        }
        ResolutionError::ResourceLimit(detail) => {
            ConfigParseError::new(ConfigParseErrorKind::ResourceLimit, None, detail)
        }
    }
}

fn derive_file_names(
    host: &dyn ConfigParseHost,
    config: &ParsedConfigNode,
    base_path: &str,
    config_file_name: &str,
    discovery_options: &ConfigDiscoveryOptions,
    errors: &mut Vec<Diagnostic>,
) -> Result<Vec<String>, ConfigParseError> {
    let case_sensitive = host.use_case_sensitive_file_names();
    let mut literal = Vec::<(String, String)>::new();
    if let Some(files) = &config.files {
        for file in files {
            let normalized = normalized_spec_path(file, base_path)?;
            map_insert(
                &mut literal,
                canonical_key(&normalized, case_sensitive),
                normalized,
            );
        }
    }

    let include = match &config.include {
        Some(include) => include.clone(),
        None if config.files.is_none() => vec![ConfigSpec {
            text: "**/*".to_owned(),
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
                ConfigParseError::new(
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

    let literal_keys = literal
        .iter()
        .map(|(key, _)| key.clone())
        .collect::<BTreeSet<_>>();
    let mut wildcard = Vec::<(String, String)>::new();
    let mut wildcard_json = Vec::<(String, String)>::new();
    for file in wildcard_candidates {
        if file_extension_is(&file, ".json") {
            if discovery_options.resolve_json_module
                && json_include_patterns
                    .iter()
                    .any(|include| include.matches(&file))
            {
                let key = canonical_key(&file, case_sensitive);
                if !literal_keys.contains(&key)
                    && !wildcard_json.iter().any(|(existing, _)| existing == &key)
                {
                    wildcard_json.push((key, file));
                }
            }
            continue;
        }
        if has_higher_priority(&file, &literal, &wildcard, extension_groups, case_sensitive) {
            continue;
        }
        remove_lower_priority(&file, &mut wildcard, extension_groups, case_sensitive);
        let key = canonical_key(&file, case_sensitive);
        if !literal_keys.contains(&key) && !wildcard.iter().any(|(existing, _)| existing == &key) {
            wildcard.push((key, file));
        }
    }

    let file_names = literal
        .into_iter()
        .chain(wildcard)
        .chain(wildcard_json)
        .map(|(_, file)| file)
        .collect::<Vec<_>>();
    report_no_input_files(
        config,
        config_file_name,
        &file_names,
        exclude_values.as_deref(),
        errors,
    );
    Ok(file_names)
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

fn invalid_trailing_recursion_pattern(spec: &str) -> bool {
    let candidate = spec.strip_suffix('/').unwrap_or(spec);
    candidate == "**" || candidate.ends_with("/**")
}

fn invalid_dot_dot_after_recursive_wildcard(spec: &str) -> bool {
    let wildcard_index = if spec.starts_with("**/") {
        Some(0)
    } else {
        spec.find("/**/")
    };
    let Some(wildcard_index) = wildcard_index else {
        return false;
    };
    let last_dot_index = if spec.ends_with("/..") {
        Some(spec.len())
    } else {
        spec.rfind("/../")
    };
    last_dot_index.is_some_and(|last_dot_index| last_dot_index > wildcard_index)
}

fn report_empty_files(
    config: &ParsedConfigNode,
    config_file_name: &str,
    errors: &mut Vec<Diagnostic>,
) {
    let Some(raw) = config.raw.as_object() else {
        return;
    };
    let files_are_empty = raw
        .get("files")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty);
    let references_are_zero_or_absent = match raw.get("references") {
        None => true,
        Some(Value::Array(references)) => references.is_empty(),
        Some(_) => false,
    };
    if files_are_empty
        && references_are_zero_or_absent
        && !config.raw_property_names.contains("extends")
    {
        errors.push(config_diagnostic(
            &gen::The_files_list_in_config_file_0_is_empty,
            &[config_file_name.to_owned()],
            config.files_location.clone(),
        ));
    }
}

fn report_no_input_files(
    config: &ParsedConfigNode,
    config_file_name: &str,
    file_names: &[String],
    effective_excludes: Option<&[String]>,
    errors: &mut Vec<Diagnostic>,
) {
    let Some(raw) = config.raw.as_object() else {
        return;
    };
    if !file_names.is_empty()
        || config.raw_property_names.contains("files")
        || config.raw_property_names.contains("references")
    {
        return;
    }

    let include = raw
        .get("include")
        .filter(|value| value.is_array())
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![Value::String("**/*".to_owned())]));
    let exclude = raw
        .get("exclude")
        .filter(|value| value.is_array())
        .cloned()
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
        &[config_file_name.to_owned(), include, exclude],
        None,
    ));
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
        Value::String(value) => result
            .push_str(&serde_json::to_string(value).expect("a Rust string is JSON serializable")),
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
                        .filter(|(name, _)| javascript_array_index(name).is_none()),
                )
            {
                if !first {
                    result.push(',');
                }
                first = false;
                result.push_str(
                    &serde_json::to_string(name).expect("an object key is JSON serializable"),
                );
                result.push(':');
                append_javascript_json(value, result);
            }
            result.push('}');
        }
    }
}

fn javascript_array_index(name: &str) -> Option<u32> {
    let index = name.parse::<u32>().ok()?;
    (index != u32::MAX && index.to_string() == name).then_some(index)
}

fn effective_discovery_options(
    options: &ConfigOptionBag,
    config_base_path: &str,
) -> Result<ConfigDiscoveryOptions, ConfigParseError> {
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

/// Project the converted config values needed by the production resolver.
/// Paths retain their declaring directory independently from `baseUrl`: the
/// latter also enables bare-specifier fallback and suppresses TS5090, while
/// `pathsBasePath` only anchors mapping substitutions.
///
/// tsc-port: getPathsBasePath @6.0.3
/// tsc-hash: c569002f6d6a8e7d3b4e2718964fae18fd77125393b0193997bf4cc1f38c494a
/// tsc-span: _tsc.js:16595-16599
fn config_module_resolution_options(
    options: &ConfigOptionBag,
    discovery: &ConfigDiscoveryOptions,
    config_file_name: &str,
    config_source: &ConfigSourceText,
    case_sensitive: bool,
) -> Result<ConfigModuleResolutionOptions, ConfigParseError> {
    let compiler_options = CompilerOptions {
        allow_js: discovery.allow_js,
        force_consistent_casing_in_file_names: config_option_bool(
            options,
            "forceConsistentCasingInFileNames",
        ),
        max_node_module_js_depth: config_option_number(options, "maxNodeModuleJsDepth"),
        experimental_decorators: config_option_bool(options, "experimentalDecorators")
            .unwrap_or(false),
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
        jsx: config_option_i32(options, "jsx"),
        no_emit: config_option_bool(options, "noEmit"),
        list_emitted_files: config_option_bool(options, "listEmittedFiles"),
        emit_bom: config_option_bool(options, "emitBOM"),
        no_emit_on_error: config_option_bool(options, "noEmitOnError"),
        no_check: config_option_bool(options, "noCheck"),
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
        out: config_option_string(options, "out"),
        incremental: config_option_bool(options, "incremental"),
        composite: config_option_bool(options, "composite"),
        assume_changes_only_affect_direct_dependencies: config_option_bool(
            options,
            "assumeChangesOnlyAffectDirectDependencies",
        ),
        ts_build_info_file: config_option_string(options, "tsBuildInfoFile"),
        imports_not_used_as_values: config_option_i32(options, "importsNotUsedAsValues"),
        preserve_value_imports: config_option_bool(options, "preserveValueImports"),
        emit_decorator_metadata: config_option_bool(options, "emitDecoratorMetadata"),
        new_line: config_option_i32(options, "newLine"),
        remove_comments: config_option_bool(options, "removeComments"),
        no_implicit_use_strict: config_option_bool(options, "noImplicitUseStrict"),
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
        resolve_json_module: Some(discovery.resolve_json_module),
        skip_lib_check: config_option_bool(options, "skipLibCheck"),
        jsx_factory: config_option_string(options, "jsxFactory"),
        jsx_fragment_factory: config_option_string(options, "jsxFragmentFactory"),
        jsx_import_source: config_option_string(options, "jsxImportSource"),
        react_namespace: config_option_string(options, "reactNamespace"),
        ignore_deprecations: config_option_string(options, "ignoreDeprecations"),
    };

    let config_path = config_program_path(config_file_name, case_sensitive)?;
    let config_file = program_config_file(config_path, config_source);
    let mut program_options = ProgramOptions::default().with_config_file(config_file);
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
        program_options = match options.stored_paths_base_path() {
            Some(base_path) => program_options.with_config_paths(mappings, base_path),
            None => program_options.with_paths(mappings),
        };
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

fn config_option_string(options: &ConfigOptionBag, name: &str) -> Option<String> {
    options
        .typed_value(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn config_option_string_list(options: &ConfigOptionBag, name: &str) -> Option<Vec<String>> {
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
                typescript_6_0_3_libraries()
                    .iter()
                    .find(|entry| entry.value() == file_name)
                    .map_or(file_name.clone(), |entry| entry.name().to_owned())
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
    Some(
        values
            .iter()
            .map(|value| match value {
                ConfigTypedListElement::Value(Value::String(value)) => {
                    ModuleSuffix::value(value.clone())
                }
                ConfigTypedListElement::Value(_) | ConfigTypedListElement::Undefined => {
                    ModuleSuffix::Undefined
                }
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
fn config_root_reasons(
    file_names: &[String],
    files: Option<&[ConfigSpec]>,
    include: Option<&[ConfigSpec]>,
    config_base_path: &str,
    config_file_name: &str,
    case_sensitive: bool,
) -> Result<Vec<RootFileReason>, ConfigParseError> {
    let mut normalized_files = Vec::with_capacity(files.map_or(0, <[ConfigSpec]>::len));
    for spec in files.unwrap_or(&[]) {
        normalized_files.push((
            canonical_key(
                &normalized_spec_path(spec, config_base_path)?,
                case_sensitive,
            ),
            Arc::<str>::from(spec.text.as_str()),
        ));
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
                ConfigParseError::new(
                    ConfigParseErrorKind::InvalidPath,
                    Some(host_spec.clone()),
                    detail,
                )
            })?;
        if let Some(pattern) = pattern {
            include_patterns.push((pattern, host_spec, Arc::<str>::from(spec.text.as_str())));
        }
    }
    let config_file = Arc::<str>::from(config_file_name);
    let default_include = files.is_none() && include.is_none();

    Ok(file_names
        .iter()
        .map(|file_name| {
            let key = canonical_key(file_name, case_sensitive);
            if let Some((_, spec)) = normalized_files
                .iter()
                .find(|(candidate, _)| candidate == &key)
            {
                return RootFileReason::FilesList { spec: spec.clone() };
            }
            if let Some((_, _, spec)) = include_patterns.iter().find(|(pattern, host_spec, _)| {
                (!file_extension_is(file_name, ".json") || host_spec.ends_with(".json"))
                    && pattern.matches(file_name)
            }) {
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

fn config_program_path(path: &str, case_sensitive: bool) -> Result<ProgramPath, ConfigParseError> {
    ProgramPath::from_trusted_parts(path, canonical_key(path, case_sensitive)).map_err(|error| {
        ConfigParseError::new(
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
    let mut config_file = ProgramConfigFile::from_snapshot(path, Arc::clone(source.snapshot()));
    let parsed =
        tsc_syntax::parse_json_text_from_snapshot(&source.file_name, Arc::clone(source.snapshot()));
    let Some(root) = config_root_object(&parsed) else {
        return config_file;
    };
    let root_properties = config_object_properties(&parsed, root);
    for property in root_properties
        .iter()
        .filter(|property| matches!(property.name.as_str(), "files" | "include"))
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
    let Some(compiler_options) = root_properties
        .into_iter()
        .find(|property| property.name == "compilerOptions")
    else {
        return config_file;
    };
    for property in config_object_properties(&parsed, compiler_options.initializer) {
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
    config_file
}

fn normalized_option_path(
    options: &ConfigOptionBag,
    name: &str,
    config_base_path: &str,
) -> Result<Option<String>, ConfigParseError> {
    options
        .typed_value(name)
        .and_then(Value::as_str)
        .and_then(|value| {
            options.get(name).map(|option| {
                normalized_config_dir_path(value, config_base_path)
                    .unwrap_or_else(|| normalized_config_path(value, &option.base_path))
            })
        })
        .transpose()
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

fn compiler_options(
    base_path: &str,
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Result<ConfigOptionBag, ConfigParseError> {
    let mut bag = ConfigOptionBag::default();
    let Some(root) = config_root_object(source) else {
        return Ok(bag);
    };
    for compiler_options in config_object_properties(source, root)
        .into_iter()
        .filter(|property| property.name == "compilerOptions")
    {
        let Some(value) =
            convert_recoverable_json_node_to_value(source, compiler_options.initializer)
        else {
            continue;
        };
        let RecoverableJsonValue::Defined(value) = value else {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &["compilerOptions".to_owned(), "object".to_owned()],
                config_location(source, compiler_options.initializer),
            ));
            continue;
        };
        if value.is_null() {
            continue;
        }
        // convertOptionsFromJson receives JavaScript's broad `object` shape.
        // Arrays have no named compiler-option properties, so TypeScript
        // accepts them as an empty bag rather than issuing TS5024.
        if value.is_array() {
            continue;
        }
        let Some(options) = value.as_object() else {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &["compilerOptions".to_owned(), "object".to_owned()],
                config_location(source, compiler_options.initializer),
            ));
            continue;
        };

        // convertConfigFileToObject invokes its option notifier for every
        // property assignment, including keys shadowed by a later duplicate.
        // This is observably different from iterating the final JSON object:
        // earlier diagnostics remain and compilerOptions objects accumulate.
        for property in config_object_properties(source, compiler_options.initializer) {
            let name = property.name.as_str();
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
            let value_location = config_location(source, property.initializer);
            let name_location = config_location(source, property.name_node);
            if let Some(declaration) = compiler_option_declaration(name) {
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
                    Some(RecoverableJsonValue::Undefined) => {
                        errors.push(config_diagnostic(
                            &gen::Compiler_option_0_requires_a_value_of_type_1,
                            &[
                                name.to_owned(),
                                compiler_option_expected_type(*declaration).to_owned(),
                            ],
                            value_location,
                        ));
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
            } else {
                let (message, args) = compiler_option_spelling_suggestion(name).map_or_else(
                    || (&gen::Unknown_compiler_option_0, vec![name.to_owned()]),
                    |suggestion| {
                        (
                            &gen::Unknown_compiler_option_0_Did_you_mean_1,
                            vec![name.to_owned(), suggestion.name().to_owned()],
                        )
                    },
                );
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
    base_path: &'a str,
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
        return Ok(None);
    }
    let expected = compiler_option_expected_type(declaration);
    let kind_matches = match declaration.value_kind() {
        CompilerOptionValueKind::Boolean => value.is_boolean(),
        CompilerOptionValueKind::Number => value.is_number(),
        CompilerOptionValueKind::String | CompilerOptionValueKind::Named(_) => value.is_string(),
        CompilerOptionValueKind::Object(_) => value.is_object() || value.is_array(),
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
        let written = value.as_str().expect("named options require a string");
        let Some(converted) = declaration.value_kind().named_value(written) else {
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
        let written = value
            .as_str()
            .expect("file-path options have already passed string validation")
            .replace('\\', "/");
        let normalized = if starts_with_config_dir_template(&written) {
            written
        } else {
            normalized_config_path(&written, base_path)?
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
        CompilerOptionValueKind::String | CompilerOptionValueKind::Named(_) => "string",
        CompilerOptionValueKind::Object(_) => "object",
        CompilerOptionValueKind::List(_) => "Array",
    }
}

fn compiler_option_list_element_expected_type(
    descriptor: CompilerOptionListDescriptor,
) -> &'static str {
    match descriptor.element_kind() {
        CompilerOptionListElementKind::String
        | CompilerOptionListElementKind::FilePath
        | CompilerOptionListElementKind::NamedString(_) => "string",
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
                    name: index.to_string(),
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
    FinishObject(Vec<String>),
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
                let elements = values.split_off(start).into_iter().flatten().collect();
                values.push(Some(ConfigTypedJsonValue::Array(elements)));
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
    assignments: impl IntoIterator<Item = (String, Option<ConfigTypedJsonValue>)>,
) -> ConfigTypedObjectValue {
    let mut properties = Vec::<ConfigTypedObjectProperty>::new();
    let mut indices = BTreeMap::<String, usize>::new();
    let mut inherits_proto_setter = true;
    for (name, value) in assignments {
        if name == "__proto__" && !indices.contains_key(&name) && inherits_proto_setter {
            if let Some(next_state) = value
                .as_ref()
                .and_then(ConfigTypedJsonValue::inherited_proto_setter)
            {
                inherits_proto_setter = next_state;
            }
            continue;
        }
        if let Some(index) = indices.get(&name).copied() {
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

fn convert_compiler_option_list_value(
    descriptor: CompilerOptionListDescriptor,
    values: &[Value],
    source: &SourceFile,
    value_node: NodeId,
    base_path: &str,
    value_location: Option<ConfigLocation>,
    errors: &mut Vec<Diagnostic>,
) -> Result<Vec<ConfigTypedListElement>, ConfigParseError> {
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

fn convert_compiler_option_list_element(
    descriptor: CompilerOptionListDescriptor,
    value: &Value,
    base_path: &str,
    location: Option<ConfigLocation>,
    errors: &mut Vec<Diagnostic>,
) -> Result<ConfigTypedListElement, ConfigParseError> {
    if value.is_null() {
        return Ok(ConfigTypedListElement::Undefined);
    }

    let converted = match descriptor.element_kind() {
        CompilerOptionListElementKind::String | CompilerOptionListElementKind::FilePath => {
            let Some(written) = value.as_str() else {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[descriptor.element_name().to_owned(), "string".to_owned()],
                    location,
                ));
                return Ok(ConfigTypedListElement::Undefined);
            };
            if matches!(
                descriptor.element_kind(),
                CompilerOptionListElementKind::FilePath
            ) {
                let written = written.replace('\\', "/");
                Value::String(if starts_with_config_dir_template(&written) {
                    written
                } else {
                    normalized_config_path(&written, base_path)?
                })
            } else {
                Value::String(written.to_owned())
            }
        }
        CompilerOptionListElementKind::NamedString(_) => {
            let Some(written) = value.as_str() else {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[descriptor.element_name().to_owned(), "string".to_owned()],
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
            Value::String(mapped.to_owned())
        }
        CompilerOptionListElementKind::Object => {
            if !matches!(value, Value::Object(_) | Value::Array(_)) {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[descriptor.element_name().to_owned(), "object".to_owned()],
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

fn config_named_string_option_choices(descriptor: CompilerOptionListDescriptor) -> String {
    descriptor
        .named_string_choices()
        .expect("named-string list descriptors carry their choices")
        .iter()
        .map(|value| format!("'{}'", value.name()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn config_named_option_choices(
    name: &str,
    values: &[crate::config_options::CompilerOptionNamedValue],
) -> String {
    match name {
        "target" => "'es6', 'es2015', 'es2016', 'es2017', 'es2018', 'es2019', 'es2020', 'es2021', 'es2022', 'es2023', 'es2024', 'es2025', 'esnext'".to_owned(),
        "module" => "'commonjs', 'es6', 'es2015', 'es2020', 'es2022', 'esnext', 'node16', 'node18', 'node20', 'nodenext', 'preserve'".to_owned(),
        "moduleResolution" => "'node16', 'nodenext', 'bundler'".to_owned(),
        _ => values
            .iter()
            .map(|value| format!("'{}'", value.name()))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn default_compiler_options(config_file_name: &str, base_path: &str) -> ConfigOptionBag {
    if config_file_name.rsplit('/').next() != Some("jsconfig.json") {
        return ConfigOptionBag::default();
    }

    let mut options = ConfigOptionBag::default();
    for &(name, default) in jsconfig_defaults() {
        let value = match default {
            JsConfigDefaultValue::Boolean(value) => Value::Bool(value),
            JsConfigDefaultValue::Number(value) => Value::from(value),
        };
        options.insert(ConfigOption {
            name: name.to_owned(),
            value: value.clone(),
            base_path: base_path.to_owned(),
        });
        options.insert_typed(name, Some(ConfigTypedOptionValue::Json(value)));
    }
    options
}

fn specs(
    name: &str,
    base_path: &str,
    source: &SourceFile,
    errors: &mut Vec<Diagnostic>,
) -> Option<Vec<ConfigSpec>> {
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
            Some(RecoverableJsonValue::Undefined) => {
                errors.push(config_diagnostic(
                    &gen::Compiler_option_0_requires_a_value_of_type_1,
                    &[name.to_owned(), "Array".to_owned()],
                    config_location(source, property.initializer),
                ));
                None
            }
            None => None,
        };
    }
    result
}

fn inheritable_specs(
    object: &Map<String, Value>,
    raw_property_names: &BTreeSet<String>,
    name: &str,
    base_path: &str,
    source: &SourceFile,
) -> Option<Vec<ConfigSpec>> {
    let value = config_property_get(object, raw_property_names, name)?;
    if !json_value_is_truthy(value) {
        return None;
    }

    // applyExtendedConfig deliberately maps the extended config's raw value,
    // not the validated ConfigFileSpecs projection. TypeScript's generic
    // `map` treats truthy booleans, numbers, and ordinary objects as empty
    // array-like values, iterates strings by character, and lets falsey array
    // elements flow through combinePaths as an empty path. Keep that recovery
    // separate from `specs`, which already emitted the value/type diagnostics.
    let texts = match value {
        Value::Array(values) => values
            .iter()
            .filter_map(config_array_like_path_text)
            .collect::<Vec<_>>(),
        Value::String(value) => value
            .chars()
            .map(|character| character.to_string())
            .collect(),
        Value::Bool(_) | Value::Number(_) | Value::Object(_) => Vec::new(),
        Value::Null => unreachable!("falsey raw spec values returned above"),
    };
    Some(
        texts
            .into_iter()
            .map(|text| ConfigSpec {
                location: config_spec_location(source, name, &text),
                text,
                base_path: base_path.to_owned(),
            })
            .collect(),
    )
}

fn config_array_like_path_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Null | Value::Bool(false) => Some(String::new()),
        Value::Number(value) if value.as_f64().is_some_and(|value| value == 0.0) => {
            Some(String::new())
        }
        // For a truthy non-string element TypeScript itself throws while
        // probing the path. The Rust planner remains fail-safe and omits that
        // unusable element after `specs` has already diagnosed its type.
        Value::Bool(true) | Value::Number(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

fn specs_from_value(
    value: &Value,
    name: &str,
    base_path: &str,
    source: &SourceFile,
    initializer: Option<NodeId>,
    errors: &mut Vec<Diagnostic>,
) -> Option<Vec<ConfigSpec>> {
    if value.is_null() {
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
    let mut specs = Vec::with_capacity(values.len());
    for (index, value) in values.iter().enumerate() {
        // convertToJson has already removed unsupported syntax, but the
        // notifier indexes the original array with this compacted index.
        let location = element_nodes
            .get(index)
            .and_then(|element| config_location(source, *element));
        if let Some(text) = value.as_str() {
            specs.push(ConfigSpec {
                text: text.to_owned(),
                base_path: base_path.to_owned(),
                // validateSpecs later recovers a node by written value and
                // therefore reuses the first matching source location for
                // duplicate strings, independently of the shifted notifier
                // location above.
                location: config_spec_location(source, name, text),
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
    object: &'a Map<String, Value>,
    raw_property_names: &BTreeSet<String>,
    name: &str,
) -> Option<&'a Value> {
    if raw_property_names.contains(name) {
        // A written own property whose recovered value is `undefined` still
        // shadows the JSONC object's prototype. serde_json omits that value,
        // so an own-only lookup must not fall through to the prototype.
        json_object_own_get(object, name)
    } else {
        json_object_get(object, name)
    }
}

fn property_is_truthy(
    object: &Map<String, Value>,
    raw_property_names: &BTreeSet<String>,
    name: &str,
) -> bool {
    config_property_get(object, raw_property_names, name).is_some_and(json_value_is_truthy)
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
                RecoverableJsonValue::Defined(value) => {
                    extends_values_from_value(&value, property.initializer, source, errors)
                }
                RecoverableJsonValue::Undefined => {
                    for _ in 0..2 {
                        errors.push(config_diagnostic(
                            &gen::Compiler_option_0_requires_a_value_of_type_1,
                            &["extends".to_owned(), "string or Array".to_owned()],
                            config_location(source, property.initializer),
                        ));
                    }
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
    if let Some(value) = value.as_str() {
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
        if let Some(text) = value.as_str() {
            result.push(ConfigExtendsSpec {
                text: text.to_owned(),
                location,
            });
        } else if !value.is_null() {
            errors.push(config_diagnostic(
                &gen::Compiler_option_0_requires_a_value_of_type_1,
                &["extends".to_owned(), "string".to_owned()],
                location,
            ));
        }
    }
    result
}

fn rebase_config_specs(
    specs: &[ConfigSpec],
    base_path: &str,
    case_sensitive: bool,
) -> Result<Vec<ConfigSpec>, ConfigParseError> {
    specs
        .iter()
        .map(|spec| {
            let text = spec.text.replace('\\', "/");
            let rebased = if starts_with_config_dir_template(&text)
                || normalized_root_parts(&text).is_some()
            {
                text
            } else {
                let difference =
                    relative_directory_path(base_path, &spec.base_path, case_sensitive)?;
                if text.is_empty() {
                    difference
                } else if difference.is_empty() {
                    text
                } else if difference.ends_with('/') {
                    format!("{difference}{text}")
                } else {
                    format!("{difference}/{text}")
                }
            };
            Ok(ConfigSpec {
                text: rebased,
                base_path: base_path.to_owned(),
                // Inherited specs are copied into the root raw object but do
                // not have a corresponding node in the root source file.
                location: None,
            })
        })
        .collect()
}

fn relative_directory_path(
    from: &str,
    to: &str,
    case_sensitive: bool,
) -> Result<String, ConfigParseError> {
    let (to_root, to_components) = rooted_components(to)?;
    // convertToRelativePath only relativizes rooted disk paths. URL roots
    // have an encoded negative root length in TypeScript and therefore pass
    // through unchanged before the base path is inspected.
    if !is_disk_root(to_root) {
        return Ok(to.to_owned());
    }
    let (from_root, from_components) = rooted_components(from)?;
    let equal = |left: &str, right: &str| {
        if case_sensitive {
            left == right
        } else {
            canonical_key(left, false) == canonical_key(right, false)
        }
    };
    // getPathComponentsRelativeTo compares the root component without case,
    // independently from the host casing profile used by later components.
    if !config_root_eq_ignore_case(from_root, to_root) {
        return Ok(to.to_owned());
    }
    let common = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| equal(left, right))
        .count();
    Ok(std::iter::repeat_n("..", from_components.len() - common)
        .chain(to_components[common..].iter().copied())
        .collect::<Vec<_>>()
        .join("/"))
}

fn config_root_eq_ignore_case(left: &str, right: &str) -> bool {
    left == right || left.to_uppercase() == right.to_uppercase()
}

fn is_disk_root(root: &str) -> bool {
    root.starts_with('/')
        || (root.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && root.as_bytes().get(1) == Some(&b':'))
}

fn rooted_components(path: &str) -> Result<(&str, Vec<&str>), ConfigParseError> {
    let (root, tail) = normalized_root_parts(path).ok_or_else(|| {
        ConfigParseError::new(
            ConfigParseErrorKind::InvalidPath,
            Some(path.to_owned()),
            "config directory is not rooted",
        )
    })?;
    Ok((
        root,
        tail.split('/')
            .filter(|component| !component.is_empty())
            .collect(),
    ))
}

fn normalized_spec_path(
    spec: &ConfigSpec,
    config_base_path: &str,
) -> Result<String, ConfigParseError> {
    normalized_config_dir_path(&spec.text, config_base_path)
        .unwrap_or_else(|| normalized_config_path(&spec.text, &spec.base_path))
}

fn config_host_spec(spec: &ConfigSpec, config_base_path: &str) -> Result<String, ConfigParseError> {
    if let Some(substituted) = normalized_config_dir_path(&spec.text, config_base_path) {
        return substituted;
    }
    Ok(spec.text.clone())
}

fn normalized_config_dir_path(
    value: &str,
    config_base_path: &str,
) -> Option<Result<String, ConfigParseError>> {
    starts_with_ignore_case(value, CONFIG_DIR_TEMPLATE).then(|| {
        let substituted = value.replacen(CONFIG_DIR_TEMPLATE, "./", 1);
        normalized_path(&substituted, config_base_path)
    })
}

fn substitute_config_dir_string(
    value: &mut String,
    config_base_path: &str,
) -> Result<bool, ConfigParseError> {
    let Some(substituted) = normalized_config_dir_path(value, config_base_path) else {
        return Ok(false);
    };
    *value = substituted?;
    Ok(true)
}

fn substitute_config_dir_typed_string_array(
    values: &mut [ConfigTypedJsonValue],
    config_base_path: &str,
) -> Result<bool, ConfigParseError> {
    let mut changed = false;
    for value in values {
        let ConfigTypedJsonValue::Json(Value::String(value)) = value else {
            continue;
        };
        changed |= substitute_config_dir_string(value, config_base_path)?;
    }
    Ok(changed)
}

const CONFIG_DIR_TEMPLATE: &str = "${configDir}";

fn starts_with_config_dir_template(value: &str) -> bool {
    starts_with_ignore_case(value, CONFIG_DIR_TEMPLATE)
}

/// TypeScript's ignore-case startsWith uppercases a UTF-16 slice instead of
/// applying ASCII-only folding. Keep the allocation-free common ASCII path,
/// then reproduce that Unicode behavior for spellings such as dotless-i.
///
/// tsc-port: equateStringsCaseInsensitive @6.0.3
/// tsc-hash: ab81c5a8cd044f72148e7e8ecb60f7003c0c3afb2b7ecde10d6bc4f48132975a
/// tsc-span: _tsc.js:905-906
/// tsc-port: startsWith @6.0.3
/// tsc-hash: b0a4b4a17f81742d08ed6267db9860c810ceb118696b1c83bd7655f9fa1b10b4
/// tsc-span: _tsc.js:1078-1079
fn starts_with_ignore_case(value: &str, prefix: &str) -> bool {
    if let Some(candidate) = value.get(..prefix.len()) {
        if candidate.eq_ignore_ascii_case(prefix) {
            return true;
        }
        if candidate.is_ascii() {
            return false;
        }
    }

    let prefix_length = prefix.encode_utf16().count();
    let candidate = value.encode_utf16().take(prefix_length).collect::<Vec<_>>();
    String::from_utf16(&candidate)
        .is_ok_and(|candidate| candidate.to_uppercase() == prefix.to_uppercase())
}

fn has_higher_priority(
    file: &str,
    literal: &[(String, String)],
    wildcard: &[(String, String)],
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
        let candidate = canonical_key(&change_extension(file, extension), case_sensitive);
        if literal.iter().any(|(key, _)| key == &candidate)
            || wildcard.iter().any(|(key, _)| key == &candidate)
        {
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
    file: &str,
    wildcard: &mut Vec<(String, String)>,
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
        let candidate = canonical_key(&change_extension(file, extension), case_sensitive);
        wildcard.retain(|(key, _)| key != &candidate);
    }
}

fn change_extension(file: &str, extension: &str) -> String {
    let current = [
        ".d.ts", ".d.cts", ".d.mts", ".tsx", ".cts", ".mts", ".jsx", ".cjs", ".mjs", ".ts", ".js",
        ".json",
    ]
    .into_iter()
    .find(|candidate| file_extension_is(file, candidate));
    match current {
        Some(current) => format!("{}{extension}", &file[..file.len() - current.len()]),
        None => format!("{file}{extension}"),
    }
}

fn file_extension_is(file: &str, extension: &str) -> bool {
    file.len() > extension.len() && file.ends_with(extension)
}

fn map_insert(entries: &mut Vec<(String, String)>, key: String, value: String) {
    if let Some((_, existing)) = entries.iter_mut().find(|(existing, _)| existing == &key) {
        *existing = value;
    } else {
        entries.push((key, value));
    }
}

fn canonical_key(path: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        path.to_owned()
    } else {
        to_file_name_lower_case(path)
    }
}

fn normalized_path(path: &str, base: &str) -> Result<String, ConfigParseError> {
    normalize_absolute_path_lexical(Path::new(path), Some(base)).map_err(|error| {
        ConfigParseError::new(
            ConfigParseErrorKind::InvalidPath,
            Some(path.to_owned()),
            error.to_string(),
        )
    })
}

fn normalized_config_path(path: &str, base: &str) -> Result<String, ConfigParseError> {
    if path.is_empty() {
        normalized_path(".", base)
    } else {
        normalized_path(path, base)
    }
}

fn is_drive_rooted(path: &str) -> bool {
    path.len() >= 2
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && (path.len() == 2 || path.as_bytes().get(2) == Some(&b'/'))
}

fn join_path(parent: &str, child: &str) -> String {
    format!(
        "{}/{}",
        parent.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}
