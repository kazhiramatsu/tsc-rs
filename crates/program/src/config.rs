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
//! deliberately preserves them. Object option schemas and the remaining
//! `ParsedCommandLine` fields are later slices.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tsc_diagnostics::{gen, Diagnostic, DiagnosticMessage, MessageChain};
use tsc_host::{to_file_name_lower_case, CompilerHost, HostError, HostErrorKind, HostOperation};
use tsc_syntax::{NodeId, SourceFile, SyntaxKind};
use tsc_types::{js_number_to_string, CompilerOptions};

use crate::config_options::{
    compiler_option_declaration, compiler_option_spelling_suggestion,
    is_command_option_without_build, jsconfig_defaults, CompilerOptionListDescriptor,
    CompilerOptionListElementKind, CompilerOptionValueKind, JsConfigDefaultValue,
};
use crate::json::{
    convert_recoverable_json_node_to_value, convert_recoverable_json_source_file_to_value,
    decode_user_object_key, is_double_quoted_json_string, json_number_as_f64, json_object_get,
    json_object_own_get, json_parser_preflight, json_source_file_is_empty, JsonParserPreflight,
    RecoverableJsonValue,
};
use crate::module_resolution::{
    directory_name, normalize_absolute_path_lexical, normalized_root_parts, ModuleResolver,
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
/// `matchFiles` semantics. The current compiler-fixture adapter is qualified
/// only against the frozen config-bearing corpus; a general filesystem adapter
/// remains a later slice.
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
    pub text: String,
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
    PositiveInfinity,
    NegativeInfinity,
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
        match self.typed_value_state(name) {
            ConfigOptionValueState::Value(value) => Some(value),
            ConfigOptionValueState::Absent
            | ConfigOptionValueState::Undefined
            | ConfigOptionValueState::List(_)
            | ConfigOptionValueState::PositiveInfinity
            | ConfigOptionValueState::NegativeInfinity => None,
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

    /// Apply TypeScript's final `${configDir}` substitution pass to the two
    /// file-path list options. Ordinary relative entries were already made
    /// absolute against the config that declared them; template entries alone
    /// are intentionally delayed until the outermost consuming config is
    /// known.
    ///
    /// tsc-port: handleOptionConfigDirTemplateSubstitution @6.0.3
    /// tsc-hash: b8be2c1ed12416218b6fb0619c12276cf3d395da3853ae7931ed6caafd1e2ca6
    /// tsc-span: _tsc.js:39175-39207
    fn finalize_config_dir_list_templates(
        &mut self,
        config_base_path: &str,
    ) -> Result<(), ConfigParseError> {
        for option in &mut self.typed_entries {
            let substitutes_config_dir = compiler_option_declaration(&option.name)
                .and_then(|declaration| declaration.value_kind().list_descriptor())
                .is_some_and(|descriptor| descriptor.allow_config_dir_template_substitution());
            if !substitutes_config_dir {
                continue;
            }
            let Some(ConfigTypedOptionValue::List(elements)) = &mut option.value else {
                continue;
            };
            for element in elements {
                let ConfigTypedListElement::Value(Value::String(value)) = element else {
                    continue;
                };
                let Some(substituted) = normalized_config_dir_path(value, config_base_path) else {
                    continue;
                };
                *value = substituted?;
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

/// Program-owned root-planning projection, not a complete `ParsedCommandLine`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigRootPlan {
    config_file_name: String,
    source: ConfigSourceText,
    extended_sources: Vec<ConfigSourceText>,
    raw: Value,
    options: ConfigOptionBag,
    discovery_options: ConfigDiscoveryOptions,
    file_names: Vec<String>,
    root_parse_diagnostics: Vec<Diagnostic>,
    errors: Vec<Diagnostic>,
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

    pub fn file_names(&self) -> &[String] {
        &self.file_names
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ConfigLocation {
    file_name: String,
    start: u32,
    length: u32,
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
            ConfigSourceText {
                file_name: request.file_name,
                text: request.text,
            },
            &config_file_name,
            &config_base,
            true,
        )?
        .expect("the primary config cannot be a recursive child of itself");
    node.options
        .finalize_config_dir_list_templates(&config_base)?;
    let discovery_options = effective_discovery_options(&node.options, &config_base)?;
    let file_names = derive_file_names(
        host,
        &node,
        &config_base,
        &config_file_name,
        &discovery_options,
        &mut context.errors,
    )?;
    node.options.restore_public_entry_order();
    Ok(ConfigRootPlan {
        config_file_name,
        source: node.source,
        extended_sources: node.extended_sources,
        extended_source_files: node.extended_source_files,
        raw: node.raw,
        options: node.options,
        discovery_options,
        file_names,
        root_parse_diagnostics: context.root_parse_diagnostics,
        errors: context.errors,
    })
}

impl ParseContext<'_> {
    fn parse_node(
        &mut self,
        source: ConfigSourceText,
        normalized_file_name: &str,
        base_path: &str,
        is_root: bool,
    ) -> Result<Option<ParsedConfigNode>, ConfigParseError> {
        match json_parser_preflight(&source.text) {
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
        let parsed = tsc_syntax::parse_json_text(&source.file_name, &source.text);
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

        let mut own_options = default_compiler_options(normalized_file_name, base_path);
        own_options.extend_from(&compiler_options(base_path, &parsed, &mut own_errors)?);
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
            let extended_source = ConfigSourceText {
                file_name: extended_path.clone(),
                text,
            };
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

fn config_location(source: &SourceFile, node: NodeId) -> Option<ConfigLocation> {
    let node = source.arena.node(node);
    let end_byte = usize::try_from(node.end).ok()?.min(source.text.len());
    let start_byte = tsc_syntax::skip_trivia(&source.text, node.pos as usize).min(end_byte);
    let start = *source.line_map.byte_to_utf16.get(start_byte)?;
    let end = *source.line_map.byte_to_utf16.get(end_byte)?;
    Some(ConfigLocation {
        file_name: source.file_name.clone(),
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
                                    Some(declaration)
                                        if matches!(
                                            declaration.value_kind(),
                                            CompilerOptionValueKind::Object
                                        ) =>
                                    {
                                        ConfigJsonConversionContext::Unported
                                    }
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
        CompilerOptionValueKind::Object => value.is_object(),
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
        CompilerOptionValueKind::Object => "object",
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
    value
        .get(..CONFIG_DIR_TEMPLATE.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(CONFIG_DIR_TEMPLATE))
        .then(|| {
            let substituted = value.replacen(CONFIG_DIR_TEMPLATE, "./", 1);
            if substituted.contains('*') || substituted.contains('?') {
                normalize_pattern_path(&substituted, config_base_path)
            } else {
                normalized_path(&substituted, config_base_path)
            }
        })
}

const CONFIG_DIR_TEMPLATE: &str = "${configDir}";

fn starts_with_config_dir_template(value: &str) -> bool {
    value
        .get(..CONFIG_DIR_TEMPLATE.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(CONFIG_DIR_TEMPLATE))
}

fn normalize_pattern_path(path: &str, base: &str) -> Result<String, ConfigParseError> {
    let slashed = path.replace('\\', "/");
    let wildcard = slashed.find(['*', '?']).unwrap_or(slashed.len());
    let directory_end = slashed[..wildcard].rfind('/').map_or(0, |index| index + 1);
    let (prefix, pattern) = slashed.split_at(directory_end);
    let normalized_prefix = if prefix.is_empty() {
        base.trim_end_matches('/').to_owned()
    } else {
        let directory = if prefix == "/" || (prefix.len() == 3 && is_drive_rooted(prefix)) {
            prefix
        } else {
            prefix.trim_end_matches('/')
        };
        normalized_path(directory, base)?
    };
    Ok(if pattern.is_empty() {
        normalized_prefix
    } else if normalized_prefix.ends_with('/') {
        format!("{normalized_prefix}{pattern}")
    } else {
        format!("{normalized_prefix}/{pattern}")
    })
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
