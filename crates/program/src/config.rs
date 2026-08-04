//! TypeScript config-file root planning.
//!
//! This module owns the production boundary immediately before
//! [`crate::load_program`]. It deliberately keeps the compiler runner's
//! virtual filesystem adapter outside `tsc_program`, while the config source,
//! `extends` graph, four effective discovery-option values, path normalization,
//! and root-name selection remain program-owned.
//!
//! The first H0.5 slice is intentionally fail-closed for malformed config
//! syntax and the root-spec shapes covered by its focused contracts.
//! Compiler-option values remain a source-order-preserving raw-value merge
//! alongside the typed discovery projection; complete located option
//! diagnostics must land before the production CLI is released.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use tsc_diagnostics::Diagnostic;
use tsc_host::{to_file_name_lower_case, CompilerHost, HostError, HostErrorKind, HostOperation};
use tsc_types::CompilerOptions;

use crate::json::{
    convert_json_source_file_to_value, decode_user_object_key, json_object_get,
    json_parser_preflight, json_source_file_is_empty, JsonParserPreflight,
};
use crate::module_resolution::{directory_name, normalize_absolute_path, ModuleResolver};
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

/// Fail-closed config-planning failure. Parser-produced syntax diagnostics are
/// retained with their locations for the later H0.5 diagnostic lane; this is
/// not the complete `ParsedCommandLine.errors` surface.
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

    fn syntax(path: String, diagnostics: Vec<Diagnostic>) -> Self {
        Self {
            kind: ConfigParseErrorKind::Syntax,
            path: Some(path),
            detail: "config source contains parse diagnostics".to_owned(),
            diagnostics,
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

/// Source-order-preserving merge of raw compiler-option property values.
///
/// TypeScript config keys are case-sensitive. This root-planning slice retains
/// every property spelling for future unknown-option diagnostics; replacement
/// never moves the first insertion. This is neither source text nor typed
/// `CompilerOptions`, and it has no complete located-diagnostic semantics.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigOptionBag {
    entries: Vec<ConfigOption>,
}

impl ConfigOptionBag {
    pub fn entries(&self) -> &[ConfigOption] {
        &self.entries
    }

    pub fn get(&self, name: &str) -> Option<&ConfigOption> {
        self.entries.iter().find(|entry| entry.name == name)
    }

    fn insert(&mut self, option: ConfigOption) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|entry| entry.name == option.name)
        {
            *existing = option;
        } else {
            self.entries.push(option);
        }
    }

    fn extend_from(&mut self, other: &Self) {
        for option in &other.entries {
            self.insert(option.clone());
        }
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
}

#[derive(Clone, Debug, PartialEq)]
struct ConfigSpec {
    text: String,
    base_path: String,
}

#[derive(Clone, Debug, PartialEq)]
struct ParsedConfigNode {
    source: ConfigSourceText,
    raw: Value,
    options: ConfigOptionBag,
    files: Option<Vec<ConfigSpec>>,
    include: Option<Vec<ConfigSpec>>,
    exclude: Option<Vec<ConfigSpec>>,
    inheritable_files: Option<Vec<ConfigSpec>>,
    inheritable_include: Option<Vec<ConfigSpec>>,
    inheritable_exclude: Option<Vec<ConfigSpec>>,
    extended_sources: Vec<ConfigSourceText>,
}

struct ParseContext<'a> {
    host: &'a dyn ConfigParseHost,
    stack: Vec<String>,
}

/// Parse one config graph and derive the `ConfigRootPlan` projection qualified
/// for the frozen valid compiler-config corpus and focused contract canaries.
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
    };
    let node = context.parse_node(
        ConfigSourceText {
            file_name: request.file_name,
            text: request.text,
        },
        &config_file_name,
        &config_base,
    )?;
    let discovery_options = effective_discovery_options(&node.options, &config_base)?;
    let file_names = derive_file_names(host, &node, &config_base, &discovery_options)?;
    Ok(ConfigRootPlan {
        config_file_name,
        source: node.source,
        extended_sources: node.extended_sources,
        raw: node.raw,
        options: node.options,
        discovery_options,
        file_names,
    })
}

impl ParseContext<'_> {
    fn parse_node(
        &mut self,
        source: ConfigSourceText,
        normalized_file_name: &str,
        base_path: &str,
    ) -> Result<ParsedConfigNode, ConfigParseError> {
        if self.stack.len() >= MAX_CONFIG_EXTENDS_DEPTH {
            return Err(ConfigParseError::new(
                ConfigParseErrorKind::ResourceLimit,
                Some(normalized_file_name.to_owned()),
                format!("config extends depth exceeds the {MAX_CONFIG_EXTENDS_DEPTH}-source limit"),
            ));
        }
        let cache_key = normalized_file_name.to_owned();
        if self.stack.iter().any(|entry| entry == &cache_key) {
            let mut cycle = self.stack.clone();
            cycle.push(cache_key);
            return Err(ConfigParseError::new(
                ConfigParseErrorKind::CircularExtends,
                Some(normalized_file_name.to_owned()),
                format!("circular extends graph: {}", cycle.join(" -> ")),
            ));
        }
        self.stack.push(cache_key.clone());
        let result = self.parse_node_uncached(source, normalized_file_name, base_path);
        self.stack.pop();
        result
    }

    fn parse_node_uncached(
        &mut self,
        source: ConfigSourceText,
        normalized_file_name: &str,
        base_path: &str,
    ) -> Result<ParsedConfigNode, ConfigParseError> {
        match json_parser_preflight(&source.text) {
            JsonParserPreflight::Safe => {}
            JsonParserPreflight::UnsafeSyntax => {
                return Err(ConfigParseError::new(
                    ConfigParseErrorKind::InvalidConfig,
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
            return Err(ConfigParseError::syntax(
                source.file_name.clone(),
                parsed.parse_diagnostics.clone(),
            ));
        }
        let mut raw = if json_source_file_is_empty(&parsed) {
            Value::Object(Map::new())
        } else {
            convert_json_source_file_to_value(&parsed).ok_or_else(|| {
                ConfigParseError::new(
                    ConfigParseErrorKind::InvalidConfig,
                    Some(source.file_name.clone()),
                    "config source is not one JSON object",
                )
            })?
        };
        let object = raw.as_object().ok_or_else(|| {
            ConfigParseError::new(
                ConfigParseErrorKind::InvalidConfig,
                Some(source.file_name.clone()),
                "config root value is not an object",
            )
        })?;

        let mut own_options = default_compiler_options(normalized_file_name, base_path);
        own_options.extend_from(&compiler_options(object, base_path, &source.file_name)?);
        let own_files = specs(object, "files", base_path, &source.file_name)?;
        let own_include = specs(object, "include", base_path, &source.file_name)?;
        let own_exclude = specs(object, "exclude", base_path, &source.file_name)?;
        // applyExtendedConfig uses ordinary JavaScript property access here,
        // so JSONC `__proto__` values can block or supply inheritance even
        // though the final config-file-spec pass accepts own properties only.
        let blocks_inherited_files = property_is_truthy(object, "files");
        let blocks_inherited_include = property_is_truthy(object, "include");
        let blocks_inherited_exclude = property_is_truthy(object, "exclude");
        let has_own_files = own_files.is_some();
        let has_own_include = own_include.is_some();
        let has_own_exclude = own_exclude.is_some();

        let mut inherited_options = ConfigOptionBag::default();
        let mut inherited_files = None;
        let mut inherited_include = None;
        let mut inherited_exclude = None;
        let mut extended_sources = Vec::new();
        let mut seen_sources = BTreeSet::new();

        // parseOwnConfig resolves every array entry before parseConfig reads
        // any extended source. That two-phase host order is observable when a
        // later path probe fails.
        let extended_paths = extends_values(object, &source.file_name)?
            .into_iter()
            .map(|extends| self.resolve_extends(&extends, base_path))
            .collect::<Result<Vec<_>, _>>()?;
        for extended_path in extended_paths {
            let text = self
                .host
                .read_file(&extended_path)?
                .ok_or_else(|| missing_extends(&extended_path))?;
            let extended_base = directory_name(&extended_path);
            let extended = self.parse_node(
                ConfigSourceText {
                    file_name: extended_path.clone(),
                    text,
                },
                &extended_path,
                &extended_base,
            )?;
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
            for extended_source in
                std::iter::once(&extended.source).chain(extended.extended_sources.iter())
            {
                let key = normalized_path(&extended_source.file_name, &extended_base)?;
                if seen_sources.insert(key) {
                    extended_sources.push(extended_source.clone());
                }
            }
        }
        inherited_options.extend_from(&own_options);
        own_options = inherited_options;

        let files = own_files.or(inherited_files);
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
                }
            }
        }
        let inheritable_files =
            inheritable_specs(raw_object, "files", base_path, &source.file_name)?;
        let inheritable_include =
            inheritable_specs(raw_object, "include", base_path, &source.file_name)?;
        let inheritable_exclude =
            inheritable_specs(raw_object, "exclude", base_path, &source.file_name)?;

        Ok(ParsedConfigNode {
            source,
            raw: config_raw_projection(raw),
            options: own_options,
            files,
            include,
            exclude,
            inheritable_files,
            inheritable_include,
            inheritable_exclude,
            extended_sources,
        })
    }

    fn resolve_extends(&self, extends: &str, base_path: &str) -> Result<String, ConfigParseError> {
        let slashed = extends.replace('\\', "/");
        if slashed.starts_with('/')
            || is_drive_rooted(&slashed)
            || slashed.starts_with("./")
            || slashed.starts_with("../")
        {
            let candidate = normalized_path(&slashed, base_path)?;
            let candidate_exists = self.host.file_exists(&candidate)?;
            if candidate_exists || candidate.ends_with(".json") {
                return Ok(candidate);
            }
            if !candidate.ends_with(".json") {
                let json = format!("{candidate}.json");
                if self.host.file_exists(&json)? {
                    return Ok(json);
                }
            }
            return Err(missing_extends(extends));
        }
        self.resolve_package_extends(&slashed, base_path)
    }

    fn resolve_package_extends(
        &self,
        specifier: &str,
        base_path: &str,
    ) -> Result<String, ConfigParseError> {
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
                .ok_or_else(|| {
                    ConfigParseError::new(
                        ConfigParseErrorKind::InvalidPath,
                        Some(module.resolved_file().display().display().to_string()),
                        "resolved config path is not valid Unicode",
                    )
                }),
            ResolutionOutcome::NotFound => Err(missing_extends(specifier)),
        }
    }
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
    discovery_options: &ConfigDiscoveryOptions,
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
        }],
        None => Vec::new(),
    };
    validate_config_specs(&include, /* disallow_trailing_recursion */ true)?;
    if let Some(exclude) = &config.exclude {
        validate_config_specs(exclude, /* disallow_trailing_recursion */ false)?;
    }
    let include_values = include
        .iter()
        .map(|spec| config_host_spec(spec, base_path))
        .collect::<Result<Vec<_>, _>>()?;
    let exclude_values = if let Some(exclude) = &config.exclude {
        Some(
            exclude
                .iter()
                .map(|spec| config_host_spec(spec, base_path))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        let defaults = [
            discovery_options.out_dir.clone(),
            discovery_options.declaration_dir.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        (!defaults.is_empty()).then_some(defaults)
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

    Ok(literal
        .into_iter()
        .chain(wildcard)
        .chain(wildcard_json)
        .map(|(_, file)| file)
        .collect())
}

fn validate_config_specs(
    specs: &[ConfigSpec],
    disallow_trailing_recursion: bool,
) -> Result<(), ConfigParseError> {
    for spec in specs {
        let slashed = spec.text.replace('\\', "/");
        let trimmed = slashed.trim_end_matches('/');
        if disallow_trailing_recursion && (trimmed == "**" || trimmed.ends_with("/**")) {
            return Err(ConfigParseError::new(
                ConfigParseErrorKind::InvalidConfig,
                None,
                format!(
                    "file specification cannot end in a recursive directory wildcard: {:?}",
                    spec.text
                ),
            ));
        }
        let mut recursive = false;
        for component in slashed.split('/') {
            if component == "**" {
                recursive = true;
            } else if recursive && component == ".." {
                return Err(ConfigParseError::new(
                    ConfigParseErrorKind::InvalidConfig,
                    None,
                    format!(
                        "file specification contains a parent directory after a recursive wildcard: {:?}",
                        spec.text
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn effective_discovery_options(
    options: &ConfigOptionBag,
    config_base_path: &str,
) -> Result<ConfigDiscoveryOptions, ConfigParseError> {
    let allow_js = options
        .get("allowJs")
        .and_then(|option| option.value.as_bool())
        .unwrap_or_else(|| {
            options
                .get("checkJs")
                .and_then(|option| option.value.as_bool())
                .unwrap_or(false)
        });
    let resolve_json_module = options
        .get("resolveJsonModule")
        .and_then(|option| option.value.as_bool())
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
        .get(name)
        .and_then(|option| {
            option.value.as_str().map(|value| {
                normalized_config_dir_path(value, config_base_path)
                    .unwrap_or_else(|| normalized_config_path(value, &option.base_path))
            })
        })
        .transpose()
}

fn computed_resolve_json_module(options: &ConfigOptionBag) -> bool {
    let module = option_text(options, "module");
    if matches!(module.as_deref(), Some("node20" | "nodenext")) {
        return true;
    }
    match option_text(options, "moduleResolution").as_deref() {
        Some("bundler") => true,
        Some("classic" | "node" | "node10" | "node16" | "nodenext") => false,
        // Invalid enum values are diagnosed and converted to `undefined` by
        // tsc, so root discovery falls through to the computed default.
        _ => !matches!(
            module.as_deref(),
            Some("none" | "amd" | "umd" | "system" | "node16" | "node18")
        ),
    }
}

fn option_text(options: &ConfigOptionBag, name: &str) -> Option<String> {
    options
        .get(name)
        .and_then(|option| option.value.as_str())
        .map(str::to_ascii_lowercase)
}

fn compiler_options(
    object: &Map<String, Value>,
    base_path: &str,
    file_name: &str,
) -> Result<ConfigOptionBag, ConfigParseError> {
    let Some(value) = object.get("compilerOptions") else {
        return Ok(ConfigOptionBag::default());
    };
    if value.is_null() {
        return Ok(ConfigOptionBag::default());
    }
    let options = value.as_object().ok_or_else(|| {
        ConfigParseError::new(
            ConfigParseErrorKind::InvalidConfig,
            Some(file_name.to_owned()),
            "compilerOptions is not an object",
        )
    })?;
    let mut bag = ConfigOptionBag::default();
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
    Ok(bag)
}

fn default_compiler_options(config_file_name: &str, base_path: &str) -> ConfigOptionBag {
    if config_file_name.rsplit('/').next() != Some("jsconfig.json") {
        return ConfigOptionBag::default();
    }

    let mut options = ConfigOptionBag::default();
    for (name, value) in [
        ("allowJs", Value::Bool(true)),
        ("maxNodeModuleJsDepth", Value::from(2)),
        ("allowSyntheticDefaultImports", Value::Bool(true)),
        ("skipLibCheck", Value::Bool(true)),
        ("noEmit", Value::Bool(true)),
    ] {
        options.insert(ConfigOption {
            name: name.to_owned(),
            value,
            base_path: base_path.to_owned(),
        });
    }
    options
}

fn specs(
    object: &Map<String, Value>,
    name: &str,
    base_path: &str,
    file_name: &str,
) -> Result<Option<Vec<ConfigSpec>>, ConfigParseError> {
    let Some(value) = object.get(name) else {
        return Ok(None);
    };
    specs_from_value(value, name, base_path, file_name)
}

fn inheritable_specs(
    object: &Map<String, Value>,
    name: &str,
    base_path: &str,
    file_name: &str,
) -> Result<Option<Vec<ConfigSpec>>, ConfigParseError> {
    let Some(value) = json_object_get(object, name) else {
        return Ok(None);
    };
    if !json_value_is_truthy(value) {
        return Ok(None);
    }
    specs_from_value(value, name, base_path, file_name)
}

fn specs_from_value(
    value: &Value,
    name: &str,
    base_path: &str,
    file_name: &str,
) -> Result<Option<Vec<ConfigSpec>>, ConfigParseError> {
    if value.is_null() {
        return Ok(None);
    }
    let values = value.as_array().ok_or_else(|| {
        ConfigParseError::new(
            ConfigParseErrorKind::InvalidConfig,
            Some(file_name.to_owned()),
            format!("{name} is not an array"),
        )
    })?;
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(|text| ConfigSpec {
                    text: text.to_owned(),
                    base_path: base_path.to_owned(),
                })
                .ok_or_else(|| {
                    ConfigParseError::new(
                        ConfigParseErrorKind::InvalidConfig,
                        Some(file_name.to_owned()),
                        format!("{name} contains a non-string entry"),
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn property_is_truthy(object: &Map<String, Value>, name: &str) -> bool {
    json_object_get(object, name).is_some_and(json_value_is_truthy)
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

fn extends_values(
    object: &Map<String, Value>,
    file_name: &str,
) -> Result<Vec<String>, ConfigParseError> {
    let Some(value) = object.get("extends") else {
        return Ok(Vec::new());
    };
    if let Some(value) = value.as_str() {
        return Ok(vec![value.to_owned()]);
    }
    let Some(values) = value.as_array() else {
        return Err(ConfigParseError::new(
            ConfigParseErrorKind::InvalidConfig,
            Some(file_name.to_owned()),
            "extends is neither a string nor an array",
        ));
    };
    values
        .iter()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                ConfigParseError::new(
                    ConfigParseErrorKind::InvalidConfig,
                    Some(file_name.to_owned()),
                    "extends contains a non-string entry",
                )
            })
        })
        .collect()
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
                || text.starts_with('/')
                || is_drive_rooted(&text)
            {
                text
            } else {
                let difference =
                    relative_directory_path(base_path, &spec.base_path, case_sensitive)?;
                if difference.starts_with('/') || is_drive_rooted(&difference) {
                    normalized_config_path(&text, &difference)?
                } else {
                    let combined = if difference.is_empty() {
                        text.clone()
                    } else {
                        format!("{difference}/{text}")
                    };
                    normalize_relative_path(&combined)
                }
            };
            Ok(ConfigSpec {
                text: rebased,
                base_path: base_path.to_owned(),
            })
        })
        .collect()
}

fn relative_directory_path(
    from: &str,
    to: &str,
    case_sensitive: bool,
) -> Result<String, ConfigParseError> {
    let (from_root, from_components) = rooted_components(from)?;
    let (to_root, to_components) = rooted_components(to)?;
    let equal = |left: &str, right: &str| {
        if case_sensitive {
            left == right
        } else {
            canonical_key(left, false) == canonical_key(right, false)
        }
    };
    if !equal(from_root, to_root) {
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

fn rooted_components(path: &str) -> Result<(&str, Vec<&str>), ConfigParseError> {
    let (root, tail) = if let Some(tail) = path.strip_prefix('/') {
        ("/", tail)
    } else if is_drive_rooted(path) {
        (&path[..3], &path[3..])
    } else {
        return Err(ConfigParseError::new(
            ConfigParseErrorKind::InvalidPath,
            Some(path.to_owned()),
            "config directory is not rooted",
        ));
    };
    Ok((
        root,
        tail.split('/')
            .filter(|component| !component.is_empty())
            .collect(),
    ))
}

fn normalize_relative_path(path: &str) -> String {
    let mut components = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => {}
            ".." if components.last().is_some_and(|last| *last != "..") => {
                components.pop();
            }
            component => components.push(component),
        }
    }
    components.join("/")
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
    normalize_absolute_path(Path::new(path), Some(base)).map_err(|error| {
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
    path.len() >= 3
        && path.as_bytes()[0].is_ascii_alphabetic()
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[2] == b'/'
}

fn join_path(parent: &str, child: &str) -> String {
    format!(
        "{}/{}",
        parent.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

fn missing_extends(path: &str) -> ConfigParseError {
    ConfigParseError::new(
        ConfigParseErrorKind::MissingExtends,
        Some(path.to_owned()),
        "extended config file was not found",
    )
}
