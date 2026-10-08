//! `tsc --showConfig` (tsgo tsoptions/showconfig.go `ConvertToTSConfig`):
//! the configuration in effect, written as a `tsconfig.json`.

use serde_json::{Map, Value};
use tsc_diagnostics::{gen, JsStr, JsString};
use tsc_program::{
    relative_path_from_directory, ConfigOptionBag, ConfigOptionValueState, ConfigRootPlan,
    ConfigTypedListElement, JsonValue,
};

use crate::options::{EnumValue, OptionDeclaration, OptionKind, COMPILER_OPTION_FIELDS};

/// What `--showConfig` reads: the options in effect, the root files and,
/// for a config file, its plan (references, specs, `compileOnSave`).
pub(crate) struct ShowConfig<'a> {
    pub(crate) options: &'a ConfigOptionBag,
    /// The config file's absolute path (`tsconfig.json` in the current
    /// directory without one): relative paths are written from it.
    pub(crate) config_file: String,
    /// The root files, absolute.
    pub(crate) file_names: Vec<String>,
    pub(crate) plan: Option<&'a ConfigRootPlan>,
    pub(crate) case_sensitive: bool,
}

/// The options tsgo removes from the output: the command's own.
const COMMAND_OPTIONS: &[&str] = &[
    "showConfig",
    "configFile",
    "configFilePath",
    "help",
    "init",
    "listFilesOnly",
    "listEmittedFiles",
    "project",
    "build",
    "version",
];

impl ShowConfig<'_> {
    /// tsgo `ConvertToTSConfig` as `json.MarshalIndentWrite` writes it: four
    /// spaces of indentation and no final new line.
    pub(crate) fn to_json(&self) -> String {
        let mut config = Map::new();
        config.insert(
            "compilerOptions".to_owned(),
            Value::Object(self.compiler_options()),
        );
        if let Some(references) = self
            .plan
            .and_then(ConfigRootPlan::project_references)
            .filter(|references| !references.is_empty())
        {
            let references = references
                .iter()
                .map(|reference| {
                    let mut entry = Map::new();
                    entry.insert("path".to_owned(), text(reference.original_path.as_js()));
                    if reference.circular == Some(true) {
                        entry.insert("circular".to_owned(), Value::Bool(true));
                    }
                    Value::Object(entry)
                })
                .collect();
            config.insert("references".to_owned(), Value::Array(references));
        }
        if !self.file_names.is_empty() {
            let files = self
                .file_names
                .iter()
                .map(|file| Value::String(self.relative(file)))
                .collect();
            config.insert("files".to_owned(), Value::Array(files));
        }
        if let Some(plan) = self.plan {
            let include = plan.include_specs();
            // tsgo `filterSameAsDefaultInclude`: nothing, or only `**/*`,
            // is left out.
            let default_include = include.iter().all(|spec| *spec == "**/*") && include.len() <= 1;
            if !default_include {
                config.insert(
                    "include".to_owned(),
                    Value::Array(include.iter().map(|spec| text(spec.as_js())).collect()),
                );
            }
            if let Some(exclude) = plan.exclude_specs().filter(|specs| !specs.is_empty()) {
                config.insert(
                    "exclude".to_owned(),
                    Value::Array(exclude.iter().map(|spec| text(spec.as_js())).collect()),
                );
            }
            if plan.compile_on_save_enabled() {
                config.insert("compileOnSave".to_owned(), Value::Bool(true));
            }
        }
        let mut output = String::new();
        write_indented(&Value::Object(config), 0, &mut output);
        output
    }

    /// tsgo `serializeCompilerOptions` in `core.CompilerOptions`' field
    /// order, without the command's own options, then the implied ones.
    fn compiler_options(&self) -> Map<String, Value> {
        let mut result = Map::new();
        for field in COMPILER_OPTION_FIELDS {
            let Some(option) = OptionDeclaration::compiler_options()
                .find(|option| option.name.eq_ignore_ascii_case(field))
            else {
                continue;
            };
            let skipped_category = option.category.is_some_and(|category| {
                std::ptr::eq(category, &gen::Command_line_Options)
                    || std::ptr::eq(category, &gen::Output_Formatting)
            });
            if skipped_category || COMMAND_OPTIONS.contains(&option.name) {
                continue;
            }
            if let Some(value) = self.option_value(option) {
                result.insert(option.name.to_owned(), value);
            }
        }
        add_implied_options(&mut result, &CoreOptions::of(self.options));
        result
    }

    /// One option's serialized value; `None` for a zero value (unset, an
    /// empty string, an unknown enum).
    fn option_value(&self, option: &OptionDeclaration) -> Option<Value> {
        match self.options.typed_value_state(option.name) {
            ConfigOptionValueState::Absent | ConfigOptionValueState::Undefined => None,
            ConfigOptionValueState::Value(value) => match option.kind {
                OptionKind::Enum => {
                    let value = enum_value(self.options, option)?;
                    option
                        .enum_key(value)
                        .map(|key| Value::String(key.to_owned()))
                }
                OptionKind::String => {
                    let path = value.as_js()?.to_string_lossy();
                    if path.is_empty() {
                        None
                    } else if option.is_file_path {
                        Some(Value::String(self.relative(&path)))
                    } else {
                        Some(Value::String(path.into_owned()))
                    }
                }
                _ => Some(json(value)),
            },
            ConfigOptionValueState::List(elements) => {
                let element = option.elements();
                let values = elements
                    .iter()
                    .filter_map(|element| match element {
                        ConfigTypedListElement::Value(value) => Some(value),
                        ConfigTypedListElement::Undefined => None,
                    })
                    .map(|value| match (element, value.as_js()) {
                        (Some(element), Some(text)) if element.is_file_path => {
                            Value::String(self.relative(&text.to_string_lossy()))
                        }
                        (Some(element), Some(text)) if element.enum_map().is_some() => {
                            Value::String(enum_element_key(element, &text.to_string_lossy()))
                        }
                        _ => json(value),
                    })
                    .collect();
                Some(Value::Array(values))
            }
            ConfigOptionValueState::Object(object) => Some(Value::Object(
                object
                    .properties()
                    .iter()
                    .filter_map(|property| {
                        let value = property.value()?;
                        Some((
                            property.name().to_string_lossy().into_owned(),
                            json(&value.json_projection()),
                        ))
                    })
                    .collect(),
            )),
            ConfigOptionValueState::PositiveInfinity | ConfigOptionValueState::NegativeInfinity => {
                Some(Value::Null)
            }
        }
    }

    /// tsgo `GetRelativePathFromFile` from the config file.
    fn relative(&self, path: &str) -> String {
        let directory = match self.config_file.rfind('/') {
            Some(0) => "/",
            Some(index) => &self.config_file[..index],
            None => "",
        };
        let relative = relative_path_from_directory(
            JsStr::from_str(directory),
            JsStr::from_str(path),
            self.case_sensitive,
        )
        .to_string_lossy()
        .into_owned();
        ensure_path_is_non_module_name(relative)
    }
}

/// An enum option's value: the bag keeps the spelling as written.
fn enum_value(options: &ConfigOptionBag, option: &OptionDeclaration) -> Option<EnumValue> {
    let spelling = options.get(option.name)?.value.as_js()?.to_string_lossy();
    option.enum_value(&spelling)
}

/// A library element's first key (tsgo stores the file name, so `es2015`
/// is written as `es6`); an unknown element as written.
fn enum_element_key(element: &OptionDeclaration, value: &str) -> String {
    let stored = element
        .enum_map()
        .unwrap_or_default()
        .iter()
        .find(|(_, candidate)| matches!(candidate, EnumValue::Text(text) if *text == value))
        .map(|(_, stored)| *stored)
        .or_else(|| element.enum_value(value));
    stored
        .and_then(|stored| element.enum_key(stored))
        .map_or_else(|| value.to_owned(), str::to_owned)
}

/// tsgo `EnsurePathIsNonModuleName`: `./` before a path that is neither
/// rooted nor relative.
fn ensure_path_is_non_module_name(path: String) -> String {
    let rooted = path.starts_with('/') || path.as_bytes().get(1) == Some(&b':');
    let relative = path == "." || path == ".." || path.starts_with("./") || path.starts_with("../");
    if rooted || relative {
        path
    } else {
        format!("./{path}")
    }
}

/// A JSON value as Go's `jsontext` writes it with a four-space indent:
/// one member or element per line, `{}` and `[]` when empty.
fn write_indented(value: &Value, depth: usize, output: &mut String) {
    const INDENT: &str = "    ";
    let open = |output: &mut String, depth: usize| {
        output.push('\n');
        output.push_str(&INDENT.repeat(depth));
    };
    match value {
        Value::Object(members) if !members.is_empty() => {
            output.push('{');
            for (index, (key, member)) in members.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                open(output, depth + 1);
                output.push_str(&Value::String(key.clone()).to_string());
                output.push_str(": ");
                write_indented(member, depth + 1, output);
            }
            open(output, depth);
            output.push('}');
        }
        Value::Array(elements) if !elements.is_empty() => {
            output.push('[');
            for (index, element) in elements.iter().enumerate() {
                if index != 0 {
                    output.push(',');
                }
                open(output, depth + 1);
                write_indented(element, depth + 1, output);
            }
            open(output, depth);
            output.push(']');
        }
        // Scalars and empty containers print compactly.
        value => output.push_str(&value.to_string()),
    }
}

fn text(value: JsStr<'_>) -> Value {
    Value::String(value.to_string_lossy().into_owned())
}

/// A config value as JSON.
fn json(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => Value::Bool(*value),
        JsonValue::Number(number) => Value::Number(number.clone()),
        JsonValue::String(text) => Value::String(text.to_string_lossy().into_owned()),
        JsonValue::Array(values) => Value::Array(values.iter().map(json).collect()),
        JsonValue::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value): (&JsString, &JsonValue)| {
                    (key.to_string_lossy().into_owned(), json(value))
                })
                .collect(),
        ),
    }
}

/// The options tsgo's implied values read, as `core.CompilerOptions` holds
/// them: enum numbers (0 when unset) and tristates.
#[derive(Clone, Copy, Debug, Default)]
struct CoreOptions {
    target: i64,
    module: i64,
    module_resolution: i64,
    module_detection: i64,
    isolated_modules: Option<bool>,
    verbatim_module_syntax: Option<bool>,
    preserve_const_enums: Option<bool>,
    composite: Option<bool>,
    declaration: Option<bool>,
    declaration_map: Option<bool>,
    incremental: Option<bool>,
    use_define_for_class_fields: Option<bool>,
    resolve_package_json_exports: Option<bool>,
    resolve_package_json_imports: Option<bool>,
    resolve_json_module: Option<bool>,
    check_js: Option<bool>,
    allow_js: Option<bool>,
    rewrite_relative_import_extensions: Option<bool>,
    allow_importing_ts_extensions: Option<bool>,
}

// tsgo's core enum values the getters compare with.
const TARGET_ES2015: i64 = 2;
const TARGET_ES2020: i64 = 7;
const TARGET_ES2022: i64 = 9;
const TARGET_ES_NEXT: i64 = 99;
const TARGET_LATEST_STANDARD: i64 = 13;
const MODULE_COMMON_JS: i64 = 1;
const MODULE_ES2015: i64 = 5;
const MODULE_ES2020: i64 = 6;
const MODULE_ES2022: i64 = 7;
const MODULE_ES_NEXT: i64 = 99;
const MODULE_NODE16: i64 = 100;
const MODULE_NODE18: i64 = 101;
const MODULE_NODE20: i64 = 102;
const MODULE_NODE_NEXT: i64 = 199;
const RESOLUTION_CLASSIC: i64 = 1;
const RESOLUTION_NODE10: i64 = 2;
const RESOLUTION_NODE16: i64 = 3;
const RESOLUTION_NODE_NEXT: i64 = 99;
const RESOLUTION_BUNDLER: i64 = 100;
const DETECTION_AUTO: i64 = 1;
const DETECTION_FORCE: i64 = 3;

impl CoreOptions {
    fn of(options: &ConfigOptionBag) -> Self {
        let flag = |name: &str| match options.typed_value_state(name) {
            ConfigOptionValueState::Value(value) => value.as_bool(),
            _ => None,
        };
        let number = |name: &str| {
            OptionDeclaration::compiler_option(name)
                .and_then(|option| enum_value(options, option))
                .map_or(0, |value| match value {
                    EnumValue::Number(number) => number,
                    EnumValue::Text(_) => 0,
                })
        };
        Self {
            target: number("target"),
            module: number("module"),
            module_resolution: number("moduleResolution"),
            module_detection: number("moduleDetection"),
            isolated_modules: flag("isolatedModules"),
            verbatim_module_syntax: flag("verbatimModuleSyntax"),
            preserve_const_enums: flag("preserveConstEnums"),
            composite: flag("composite"),
            declaration: flag("declaration"),
            declaration_map: flag("declarationMap"),
            incremental: flag("incremental"),
            use_define_for_class_fields: flag("useDefineForClassFields"),
            resolve_package_json_exports: flag("resolvePackageJsonExports"),
            resolve_package_json_imports: flag("resolvePackageJsonImports"),
            resolve_json_module: flag("resolveJsonModule"),
            check_js: flag("checkJs"),
            allow_js: flag("allowJs"),
            rewrite_relative_import_extensions: flag("rewriteRelativeImportExtensions"),
            allow_importing_ts_extensions: flag("allowImportingTsExtensions"),
        }
    }

    fn emit_script_target(&self) -> i64 {
        if self.target != 0 {
            self.target
        } else {
            TARGET_LATEST_STANDARD
        }
    }

    fn emit_module_kind(&self) -> i64 {
        if self.module != 0 {
            return self.module;
        }
        match self.emit_script_target() {
            TARGET_ES_NEXT => MODULE_ES_NEXT,
            target if target >= TARGET_ES2022 => MODULE_ES2022,
            target if target >= TARGET_ES2020 => MODULE_ES2020,
            target if target >= TARGET_ES2015 => MODULE_ES2015,
            _ => MODULE_COMMON_JS,
        }
    }

    fn module_resolution_kind(&self) -> i64 {
        match self.module_resolution {
            0 | RESOLUTION_CLASSIC | RESOLUTION_NODE10 => match self.emit_module_kind() {
                MODULE_NODE16 | MODULE_NODE18 | MODULE_NODE20 => RESOLUTION_NODE16,
                MODULE_NODE_NEXT => RESOLUTION_NODE_NEXT,
                _ => RESOLUTION_BUNDLER,
            },
            resolution => resolution,
        }
    }

    fn module_detection_kind(&self) -> i64 {
        if self.module_detection != 0 {
            return self.module_detection;
        }
        if (MODULE_NODE16..=MODULE_NODE_NEXT).contains(&self.emit_module_kind()) {
            DETECTION_FORCE
        } else {
            DETECTION_AUTO
        }
    }

    fn isolated_modules(&self) -> bool {
        self.isolated_modules == Some(true) || self.verbatim_module_syntax == Some(true)
    }

    fn preserve_const_enums(&self) -> bool {
        self.preserve_const_enums == Some(true) || self.isolated_modules()
    }

    fn emit_declarations(&self) -> bool {
        self.declaration == Some(true) || self.composite == Some(true)
    }

    fn declaration_maps(&self) -> bool {
        self.declaration_map == Some(true) && self.emit_declarations()
    }

    fn incremental(&self) -> bool {
        self.incremental == Some(true) || self.composite == Some(true)
    }

    fn use_define_for_class_fields(&self) -> bool {
        match self.use_define_for_class_fields {
            None => self.emit_script_target() >= TARGET_ES2022,
            Some(value) => value,
        }
    }

    fn resolve_json_module(&self) -> bool {
        if let Some(value) = self.resolve_json_module {
            return value;
        }
        matches!(self.emit_module_kind(), MODULE_NODE20 | MODULE_NODE_NEXT)
            || self.module_resolution_kind() == RESOLUTION_BUNDLER
    }

    fn allow_js(&self) -> bool {
        self.allow_js.unwrap_or(self.check_js == Some(true))
    }

    fn allow_importing_ts_extensions(&self) -> bool {
        self.allow_importing_ts_extensions == Some(true)
            || self.rewrite_relative_import_extensions == Some(true)
    }
}

/// An implied option's value: an enum number or a boolean.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Implied {
    Enum(i64),
    Bool(bool),
}

/// tsgo `impliedOptions`: an option, the options it depends on, and its
/// value as the getter computes it.
type ImpliedOption = (
    &'static str,
    &'static [&'static str],
    fn(&CoreOptions) -> Implied,
);

const IMPLIED_OPTIONS: &[ImpliedOption] = &[
    ("module", &["target"], |options| {
        Implied::Enum(options.emit_module_kind())
    }),
    ("moduleResolution", &["module", "target"], |options| {
        Implied::Enum(options.module_resolution_kind())
    }),
    ("moduleDetection", &["module", "target"], |options| {
        Implied::Enum(options.module_detection_kind())
    }),
    ("isolatedModules", &["verbatimModuleSyntax"], |options| {
        Implied::Bool(options.isolated_modules())
    }),
    (
        "preserveConstEnums",
        &["isolatedModules", "verbatimModuleSyntax"],
        |options| Implied::Bool(options.preserve_const_enums()),
    ),
    ("declaration", &["composite"], |options| {
        Implied::Bool(options.emit_declarations())
    }),
    ("declarationMap", &["declaration", "composite"], |options| {
        Implied::Bool(options.declaration_maps())
    }),
    ("incremental", &["composite"], |options| {
        Implied::Bool(options.incremental())
    }),
    (
        "useDefineForClassFields",
        &["target", "module"],
        |options| Implied::Bool(options.use_define_for_class_fields()),
    ),
    (
        "resolvePackageJsonExports",
        &["moduleResolution", "module", "target"],
        |options| Implied::Bool(options.resolve_package_json_exports != Some(false)),
    ),
    (
        "resolvePackageJsonImports",
        &[
            "moduleResolution",
            "resolvePackageJsonExports",
            "module",
            "target",
        ],
        |options| Implied::Bool(options.resolve_package_json_imports != Some(false)),
    ),
    (
        "resolveJsonModule",
        &["moduleResolution", "module", "target"],
        |options| Implied::Bool(options.resolve_json_module()),
    ),
    ("allowJs", &["checkJs"], |options| {
        Implied::Bool(options.allow_js())
    }),
    (
        "allowImportingTsExtensions",
        &["rewriteRelativeImportExtensions"],
        |options| Implied::Bool(options.allow_importing_ts_extensions()),
    ),
];

/// tsgo `addImpliedOptions`: an option not written whose value one of the
/// written options changes from its default.
fn add_implied_options(result: &mut Map<String, Value>, options: &CoreOptions) {
    let provided = result.keys().cloned().collect::<Vec<_>>();
    let defaults = CoreOptions::default();
    for (name, dependencies, compute) in IMPLIED_OPTIONS {
        if provided.iter().any(|option| option == name)
            || !dependencies
                .iter()
                .any(|dependency| provided.iter().any(|option| option == dependency))
        {
            continue;
        }
        let implied = compute(options);
        if implied == compute(&defaults) {
            continue;
        }
        let value = match implied {
            Implied::Bool(value) => Value::Bool(value),
            Implied::Enum(value) => {
                let Some(key) = OptionDeclaration::compiler_option(name)
                    .and_then(|option| option.enum_key(EnumValue::Number(value)))
                else {
                    continue;
                };
                Value::String(key.to_owned())
            }
        };
        result.insert((*name).to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsc_program::command_line_option_bag;

    fn text(value: &str) -> JsonValue {
        JsonValue::String(JsString::from(value))
    }

    fn show(options: &[(&str, JsonValue)], files: &[&str]) -> String {
        let options = options
            .iter()
            .map(|(name, value)| ((*name).to_owned(), value.clone()))
            .collect::<Vec<_>>();
        let bag = command_line_option_bag(&options, JsStr::from_str("/p"));
        ShowConfig {
            options: &bag,
            config_file: "/p/tsconfig.json".to_owned(),
            file_names: files.iter().map(|file| (*file).to_owned()).collect(),
            plan: None,
            case_sensitive: true,
        }
        .to_json()
    }

    // tsgo's Show-TSConfig-with-transitively-implied-options baseline.
    #[test]
    fn options_imply_the_values_they_change() {
        assert_eq!(
            show(&[("module", text("nodenext"))], &["/p/src/index.ts"]),
            r#"{
    "compilerOptions": {
        "module": "nodenext",
        "moduleResolution": "nodenext",
        "moduleDetection": "force"
    },
    "files": [
        "./src/index.ts"
    ]
}"#
        );
    }

    // tsgo's references and compileOnSave-and-more baselines: options in
    // `core.CompilerOptions` order, implied values only where they differ
    // from the defaults.
    #[test]
    fn options_follow_the_struct_order_then_the_implied_ones() {
        assert_eq!(
            show(
                &[
                    ("esModuleInterop", JsonValue::Bool(true)),
                    ("target", text("ES5")),
                    ("strict", JsonValue::Bool(true)),
                    ("module", text("commonjs")),
                ],
                &[],
            ),
            r#"{
    "compilerOptions": {
        "module": "commonjs",
        "strict": true,
        "target": "es5",
        "esModuleInterop": true,
        "useDefineForClassFields": false
    }
}"#
        );
        let composite = show(&[("composite", JsonValue::Bool(true))], &[]);
        assert!(composite.contains(
            "\"composite\": true,\n        \"declaration\": true,\n        \"incremental\": true\n"
        ));
    }

    // Paths are written from the config file; a library by its first key;
    // nothing set is an empty object.
    #[test]
    fn paths_are_relative_and_libraries_are_keys() {
        let shown = show(
            &[
                ("outDir", text("lib")),
                ("lib", JsonValue::Array(vec![text("es2015"), text("dom")])),
            ],
            &["/p/a.ts"],
        );
        assert!(shown.contains("\"lib\": [\n            \"es6\",\n            \"dom\"\n        ],"));
        assert!(shown.contains("\"outDir\": \"./lib\""));
        assert_eq!(
            show(&[], &["/p/a.ts"]),
            "{\n    \"compilerOptions\": {},\n    \"files\": [\n        \"./a.ts\"\n    ]\n}"
        );
    }
}
