//! The JSON tsgo writes (`encoding/json/v2` through `tsc/internal/json`)
//! for its options: compact and two-space indented text, the field order
//! of `core.CompilerOptions`, `core.BuildOptions` and `core.TypeAcquisition`
//! (`omitzero` leaves out unset fields), and the numbers of tsgo's option
//! enums. tsgo's API sends a project's options in this form, and its tests
//! print them so.

use std::sync::Arc;

use crate::config::{ConfigTypedObjectProperty, ConfigTypedObjectShape, ConfigTypedOptionValue};
use crate::{
    ConfigOptionBag, ConfigOptionValueState, ConfigRootPlan, ConfigTypedJsonValue,
    ConfigTypedListElement, ConfigTypedObjectValue, JsonValue,
};

/// A JSON value in the order tsgo writes it.
#[derive(Clone, Debug, PartialEq)]
pub enum GoJson {
    Null,
    Bool(bool),
    /// The decimal text of a number.
    Number(String),
    String(String),
    Array(Vec<GoJson>),
    Object(Vec<(String, GoJson)>),
}

impl GoJson {
    pub fn number(value: i64) -> Self {
        Self::Number(value.to_string())
    }

    /// A tsc-rs JSON value; a number takes Go's shortest form (an integral
    /// `float64` has no fraction).
    pub fn from_json(value: &JsonValue) -> Self {
        match value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(value) => Self::Bool(*value),
            JsonValue::Number(number) => Self::Number(number_text(number)),
            JsonValue::String(text) => Self::String(text.to_string_lossy().into_owned()),
            JsonValue::Array(values) => Self::Array(values.iter().map(Self::from_json).collect()),
            JsonValue::Object(object) => Self::Object(
                object
                    .iter()
                    .map(|(key, value)| {
                        (key.to_string_lossy().into_owned(), Self::from_json(value))
                    })
                    .collect(),
            ),
        }
    }

    /// `json.Marshal`.
    pub fn compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// `json.MarshalIndent(value, "", "  ")`.
    pub fn indented(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, Some("  "), 0);
        out
    }

    fn write(&self, out: &mut String, indent: Option<&str>, depth: usize) {
        let line = |out: &mut String, depth: usize| {
            if let Some(indent) = indent {
                out.push('\n');
                for _ in 0..depth {
                    out.push_str(indent);
                }
            }
        };
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Number(text) => out.push_str(text),
            Self::String(text) => out.push_str(&quote(text)),
            Self::Array(values) => {
                out.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    line(out, depth + 1);
                    value.write(out, indent, depth + 1);
                }
                if !values.is_empty() {
                    line(out, depth);
                }
                out.push(']');
            }
            Self::Object(entries) => {
                out.push('{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    line(out, depth + 1);
                    out.push_str(&quote(key));
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    value.write(out, indent, depth + 1);
                }
                if !entries.is_empty() {
                    line(out, depth);
                }
                out.push('}');
            }
        }
    }
}

/// `strconv.FormatFloat(value, 'f', -1, 64)` for the numbers the tests
/// write: an integer as is, a fraction in its shortest form.
fn number_text(number: &serde_json::Number) -> String {
    if number.is_i64() || number.is_u64() {
        return number.to_string();
    }
    match number.as_f64() {
        Some(value) if value.fract() == 0.0 && value.abs() < 1e15 => format!("{}", value as i64),
        Some(value) => format!("{value}"),
        None => number.to_string(),
    }
}

/// A JSON string as `encoding/json/v2` writes it without HTML or JavaScript
/// escaping: serde_json's quoting is the same.
fn quote(text: &str) -> String {
    serde_json::to_string(text).expect("a string serializes")
}

/// How a struct field is written.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKind {
    /// `Tristate`: `true` or `false`.
    Tristate,
    /// `string`: left out when empty.
    String,
    /// An option enum: its tsgo number.
    Enum,
    /// `[]string`; `lib` holds the library file names.
    StringList,
    /// `*collections.OrderedMap[string, []string]`.
    Paths,
    /// `[]PluginImport`.
    Plugins,
    /// `*int`.
    Int,
}

/// `core.CompilerOptions`' fields in declaration order (core/compileroptions.go).
pub const COMPILER_OPTIONS_FIELDS: &[(&str, FieldKind)] = &[
    ("allowJs", FieldKind::Tristate),
    ("allowArbitraryExtensions", FieldKind::Tristate),
    ("allowImportingTsExtensions", FieldKind::Tristate),
    ("allowNonTsExtensions", FieldKind::Tristate),
    ("allowUmdGlobalAccess", FieldKind::Tristate),
    ("allowUnreachableCode", FieldKind::Tristate),
    ("allowUnusedLabels", FieldKind::Tristate),
    (
        "assumeChangesOnlyAffectDirectDependencies",
        FieldKind::Tristate,
    ),
    ("checkJs", FieldKind::Tristate),
    ("customConditions", FieldKind::StringList),
    ("composite", FieldKind::Tristate),
    ("emitDeclarationOnly", FieldKind::Tristate),
    ("emitBOM", FieldKind::Tristate),
    ("emitDecoratorMetadata", FieldKind::Tristate),
    ("declaration", FieldKind::Tristate),
    ("declarationDir", FieldKind::String),
    ("declarationMap", FieldKind::Tristate),
    ("deduplicatePackages", FieldKind::Tristate),
    ("disableSizeLimit", FieldKind::Tristate),
    (
        "disableSourceOfProjectReferenceRedirect",
        FieldKind::Tristate,
    ),
    ("disableSolutionSearching", FieldKind::Tristate),
    ("disableReferencedProjectLoad", FieldKind::Tristate),
    ("erasableSyntaxOnly", FieldKind::Tristate),
    ("exactOptionalPropertyTypes", FieldKind::Tristate),
    ("experimentalDecorators", FieldKind::Tristate),
    ("forceConsistentCasingInFileNames", FieldKind::Tristate),
    ("isolatedModules", FieldKind::Tristate),
    ("isolatedDeclarations", FieldKind::Tristate),
    ("ignoreConfig", FieldKind::Tristate),
    ("ignoreDeprecations", FieldKind::String),
    ("importHelpers", FieldKind::Tristate),
    ("inlineSourceMap", FieldKind::Tristate),
    ("inlineSources", FieldKind::Tristate),
    ("init", FieldKind::Tristate),
    ("incremental", FieldKind::Tristate),
    ("jsx", FieldKind::Enum),
    ("jsxFactory", FieldKind::String),
    ("jsxFragmentFactory", FieldKind::String),
    ("jsxImportSource", FieldKind::String),
    ("lib", FieldKind::StringList),
    ("libReplacement", FieldKind::Tristate),
    ("locale", FieldKind::String),
    ("mapRoot", FieldKind::String),
    ("module", FieldKind::Enum),
    ("moduleResolution", FieldKind::Enum),
    ("moduleSuffixes", FieldKind::StringList),
    ("moduleDetection", FieldKind::Enum),
    ("newLine", FieldKind::Enum),
    ("noEmit", FieldKind::Tristate),
    ("noCheck", FieldKind::Tristate),
    ("noErrorTruncation", FieldKind::Tristate),
    ("noFallthroughCasesInSwitch", FieldKind::Tristate),
    ("noImplicitAny", FieldKind::Tristate),
    ("noImplicitThis", FieldKind::Tristate),
    ("noImplicitReturns", FieldKind::Tristate),
    ("noEmitHelpers", FieldKind::Tristate),
    ("noLib", FieldKind::Tristate),
    ("noPropertyAccessFromIndexSignature", FieldKind::Tristate),
    ("noUncheckedIndexedAccess", FieldKind::Tristate),
    ("noEmitOnError", FieldKind::Tristate),
    ("noUnusedLocals", FieldKind::Tristate),
    ("noUnusedParameters", FieldKind::Tristate),
    ("noResolve", FieldKind::Tristate),
    ("noImplicitOverride", FieldKind::Tristate),
    ("noUncheckedSideEffectImports", FieldKind::Tristate),
    ("outDir", FieldKind::String),
    ("paths", FieldKind::Paths),
    ("plugins", FieldKind::Plugins),
    ("preserveConstEnums", FieldKind::Tristate),
    ("preserveSymlinks", FieldKind::Tristate),
    ("project", FieldKind::String),
    ("resolveJsonModule", FieldKind::Tristate),
    ("resolvePackageJsonExports", FieldKind::Tristate),
    ("resolvePackageJsonImports", FieldKind::Tristate),
    ("removeComments", FieldKind::Tristate),
    ("rewriteRelativeImportExtensions", FieldKind::Tristate),
    ("reactNamespace", FieldKind::String),
    ("rootDir", FieldKind::String),
    ("rootDirs", FieldKind::StringList),
    ("skipLibCheck", FieldKind::Tristate),
    ("stableTypeOrdering", FieldKind::Tristate),
    ("strict", FieldKind::Tristate),
    ("strictBindCallApply", FieldKind::Tristate),
    ("strictBuiltinIteratorReturn", FieldKind::Tristate),
    ("strictFunctionTypes", FieldKind::Tristate),
    ("strictNullChecks", FieldKind::Tristate),
    ("strictPropertyInitialization", FieldKind::Tristate),
    ("stripInternal", FieldKind::Tristate),
    ("skipDefaultLibCheck", FieldKind::Tristate),
    ("sourceMap", FieldKind::Tristate),
    ("sourceRoot", FieldKind::String),
    ("suppressOutputPathCheck", FieldKind::Tristate),
    ("target", FieldKind::Enum),
    ("traceResolution", FieldKind::Tristate),
    ("tsBuildInfoFile", FieldKind::String),
    ("typeRoots", FieldKind::StringList),
    ("types", FieldKind::StringList),
    ("useDefineForClassFields", FieldKind::Tristate),
    ("useUnknownInCatchVariables", FieldKind::Tristate),
    ("verbatimModuleSyntax", FieldKind::Tristate),
    ("maxNodeModuleJsDepth", FieldKind::Int),
    ("allowSyntheticDefaultImports", FieldKind::Tristate),
    ("alwaysStrict", FieldKind::Tristate),
    ("baseUrl", FieldKind::String),
    ("downlevelIteration", FieldKind::Tristate),
    ("esModuleInterop", FieldKind::Tristate),
    ("outFile", FieldKind::String),
    ("configFilePath", FieldKind::String),
    ("noDtsResolution", FieldKind::Tristate),
    ("pathsBasePath", FieldKind::String),
    ("diagnostics", FieldKind::Tristate),
    ("extendedDiagnostics", FieldKind::Tristate),
    ("generateCpuProfile", FieldKind::String),
    ("generateTrace", FieldKind::String),
    ("listEmittedFiles", FieldKind::Tristate),
    ("listFiles", FieldKind::Tristate),
    ("explainFiles", FieldKind::Tristate),
    ("listFilesOnly", FieldKind::Tristate),
    ("noEmitForJsFiles", FieldKind::Tristate),
    ("preserveWatchOutput", FieldKind::Tristate),
    ("pretty", FieldKind::Tristate),
    ("version", FieldKind::Tristate),
    ("watch", FieldKind::Tristate),
    ("showConfig", FieldKind::Tristate),
    ("build", FieldKind::Tristate),
    ("help", FieldKind::Tristate),
    ("all", FieldKind::Tristate),
    ("runExternalCode", FieldKind::Tristate),
    ("pprofDir", FieldKind::String),
    ("singleThreaded", FieldKind::Tristate),
    ("quiet", FieldKind::Tristate),
    ("checkers", FieldKind::Int),
];

/// `core.BuildOptions`' fields in declaration order (core/buildoptions.go).
pub const BUILD_OPTIONS_FIELDS: &[(&str, FieldKind)] = &[
    ("dry", FieldKind::Tristate),
    ("force", FieldKind::Tristate),
    ("verbose", FieldKind::Tristate),
    ("builders", FieldKind::Int),
    ("stopBuildOnErrors", FieldKind::Tristate),
    ("clean", FieldKind::Tristate),
];

/// `core.TypeAcquisition`'s fields in declaration order
/// (core/typeacquisition.go).
pub const TYPE_ACQUISITION_FIELDS: &[(&str, FieldKind)] = &[
    ("enable", FieldKind::Tristate),
    ("include", FieldKind::StringList),
    ("exclude", FieldKind::StringList),
    ("disableFilenameBasedTypeAcquisition", FieldKind::Tristate),
];

/// tsgo's number for the spelling of an enum option (tsoptions/enummaps.go
/// with the core and watch-option constants), case-insensitively.
pub fn enum_number(option: &str, spelling: &str) -> Option<i64> {
    let table: &[(&str, i64)] = match option {
        "target" => &[
            ("es5", 1),
            ("es6", 2),
            ("es2015", 2),
            ("es2016", 3),
            ("es2017", 4),
            ("es2018", 5),
            ("es2019", 6),
            ("es2020", 7),
            ("es2021", 8),
            ("es2022", 9),
            ("es2023", 10),
            ("es2024", 11),
            ("es2025", 12),
            ("es2026", 13),
            ("esnext", 99),
        ],
        "module" => &[
            ("commonjs", 1),
            ("amd", 2),
            ("system", 4),
            ("umd", 3),
            ("es6", 5),
            ("es2015", 5),
            ("es2020", 6),
            ("es2022", 7),
            ("esnext", 99),
            ("node16", 100),
            ("node18", 101),
            ("node20", 102),
            ("nodenext", 199),
            ("preserve", 200),
        ],
        "moduleResolution" => &[
            ("node16", 3),
            ("nodenext", 99),
            ("bundler", 100),
            ("classic", 1),
            ("node", 2),
            ("node10", 2),
        ],
        "moduleDetection" => &[("auto", 1), ("legacy", 2), ("force", 3)],
        "jsx" => &[
            ("preserve", 1),
            ("react-native", 3),
            ("react-jsx", 4),
            ("react-jsxdev", 5),
            ("react", 2),
        ],
        "newLine" => &[("crlf", 1), ("lf", 2)],
        "watchFile" => &[
            ("fixedpollinginterval", 1),
            ("prioritypollinginterval", 2),
            ("dynamicprioritypolling", 3),
            ("fixedchunksizepolling", 4),
            ("usefsevents", 5),
            ("usefseventsonparentdirectory", 6),
        ],
        "watchDirectory" => &[
            ("usefsevents", 1),
            ("fixedpollinginterval", 2),
            ("dynamicprioritypolling", 3),
            ("fixedchunksizepolling", 4),
        ],
        "fallbackPolling" => &[
            ("fixedinterval", 1),
            ("priorityinterval", 2),
            ("dynamicpriority", 3),
            ("fixedchunksize", 4),
        ],
        _ => return None,
    };
    let spelling = spelling.to_ascii_lowercase();
    table
        .iter()
        .find(|(name, _)| *name == spelling)
        .map(|(_, number)| *number)
}

/// `ParsedConfig.CompilerOptions` in tsgo's struct order: the merged config
/// options, the config file's path and the directory `paths` is based on.
pub fn compiler_options_json(plan: &ConfigRootPlan) -> GoJson {
    let options = plan.options();
    let mut entries = Vec::new();
    for (field, kind) in COMPILER_OPTIONS_FIELDS {
        let value = match *field {
            "configFilePath" => Some(GoJson::String(
                plan.config_file_name().to_string_lossy().into_owned(),
            )),
            "pathsBasePath" => options
                .stored_paths_base_path()
                .map(|base| GoJson::String(base.to_string_lossy().into_owned())),
            _ => field_json(options, field, *kind),
        };
        if let Some(value) = value {
            entries.push(((*field).to_owned(), value));
        }
    }
    GoJson::Object(entries)
}

/// A struct of tsgo's (`fields` in declaration order) with the bag's
/// converted values; `omitzero` leaves out what the bag does not set.
pub fn struct_json(fields: &[(&str, FieldKind)], options: &ConfigOptionBag) -> GoJson {
    GoJson::Object(
        fields
            .iter()
            .filter_map(|(field, kind)| {
                field_json(options, field, *kind).map(|value| ((*field).to_owned(), value))
            })
            .collect(),
    )
}

/// One field's value, `None` when `omitzero` leaves it out.
fn field_json(options: &ConfigOptionBag, field: &str, kind: FieldKind) -> Option<GoJson> {
    Some(match options.typed_value_state(field) {
        ConfigOptionValueState::Absent | ConfigOptionValueState::Undefined => return None,
        ConfigOptionValueState::Value(value) => match kind {
            FieldKind::Enum => {
                let spelling = options
                    .get(field)
                    .and_then(|option| option.value.as_js())
                    .and_then(|spelling| spelling.as_str().map(str::to_owned));
                match spelling.and_then(|spelling| enum_number(field, &spelling)) {
                    Some(number) => GoJson::number(number),
                    None => GoJson::from_json(value),
                }
            }
            FieldKind::String if value.as_js().is_some_and(|text| text.is_empty()) => return None,
            _ => GoJson::from_json(value),
        },
        ConfigOptionValueState::List(elements) => GoJson::Array(
            elements
                .iter()
                .filter_map(|element| match element {
                    ConfigTypedListElement::Value(value) => Some(GoJson::from_json(value)),
                    ConfigTypedListElement::Undefined => None,
                })
                .collect(),
        ),
        ConfigOptionValueState::Object(object) => GoJson::Object(
            object
                .properties()
                .iter()
                .filter_map(|property| {
                    property.value().map(|value| {
                        (
                            property.name().to_string_lossy().into_owned(),
                            typed_json(value),
                        )
                    })
                })
                .collect(),
        ),
        ConfigOptionValueState::PositiveInfinity | ConfigOptionValueState::NegativeInfinity => {
            GoJson::Null
        }
    })
}

fn typed_json(value: &ConfigTypedJsonValue) -> GoJson {
    GoJson::from_json(&value.json_projection())
}

/// tsgo's `core.CompilerOptions` from its JSON, as tsgo's API receives a
/// program's options: each field of the struct (`COMPILER_OPTIONS_FIELDS`)
/// that the JSON sets, in the form the config converter stores (enum
/// numbers, `lib` file names, `paths` as an object). As Go's
/// `json.Unmarshal` does, unknown keys are ignored, `null` and a zero enum,
/// an empty string or a tristate that is neither `true` nor `false` leave a
/// field unset, and a value of another JSON type is an error.
pub fn compiler_options_bag(
    options: &serde_json::Map<String, serde_json::Value>,
) -> Result<ConfigOptionBag, String> {
    use serde_json::Value;

    let mut bag = ConfigOptionBag::default();
    for (field, kind) in COMPILER_OPTIONS_FIELDS {
        let Some(value) = options.get(*field) else {
            continue;
        };
        let mismatch = || {
            format!(
                "cannot unmarshal JSON {} into the compiler option {field}",
                json_kind(value)
            )
        };
        let strings = |values: &[Value]| {
            values
                .iter()
                .map(|value| match value {
                    Value::String(text) => Ok(JsonValue::String(text.as_str().into())),
                    _ => Err(mismatch()),
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let typed = match (kind, value) {
            (_, Value::Null) | (FieldKind::Tristate, _) => match value {
                Value::Bool(value) => Some(ConfigTypedOptionValue::Json(JsonValue::Bool(*value))),
                _ => None,
            },
            (FieldKind::String, Value::String(text)) => (!text.is_empty())
                .then(|| ConfigTypedOptionValue::Json(JsonValue::String(text.as_str().into()))),
            (FieldKind::Enum, Value::Number(number)) if number.as_i64().is_some() => {
                (number.as_i64() != Some(0))
                    .then(|| ConfigTypedOptionValue::Json(JsonValue::Number(number.clone())))
            }
            (FieldKind::Int, Value::Number(number)) if number.as_i64().is_some() => Some(
                ConfigTypedOptionValue::Json(JsonValue::Number(number.clone())),
            ),
            (FieldKind::StringList, Value::Array(values)) => Some(ConfigTypedOptionValue::List(
                strings(values)?
                    .into_iter()
                    .map(ConfigTypedListElement::Value)
                    .collect(),
            )),
            (FieldKind::Paths, Value::Object(paths)) => {
                let properties = paths
                    .iter()
                    .map(|(key, value)| match value {
                        Value::Array(values) => Ok(ConfigTypedObjectProperty::new(
                            key.as_str().into(),
                            Some(ConfigTypedJsonValue::Array(
                                strings(values)?
                                    .into_iter()
                                    .map(ConfigTypedJsonValue::Json)
                                    .collect(),
                            )),
                        )),
                        _ => Err(mismatch()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Some(ConfigTypedOptionValue::Object(Arc::new(
                    ConfigTypedObjectValue::new(ConfigTypedObjectShape::Object, properties, true),
                )))
            }
            (FieldKind::Plugins, Value::Array(plugins)) => Some(ConfigTypedOptionValue::List(
                plugins
                    .iter()
                    .map(|plugin| match plugin {
                        Value::Object(_) => Ok(ConfigTypedListElement::Value(JsonValue::from(
                            plugin.clone(),
                        ))),
                        _ => Err(mismatch()),
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            _ => return Err(mismatch()),
        };
        if let Some(typed) = typed {
            bag.insert_typed(*field, Some(typed));
        }
    }
    Ok(bag)
}

/// Go's name of a JSON value's kind (`jsontext.Kind`).
fn json_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indented_output_follows_marshal_indent() {
        let value = GoJson::Object(vec![
            ("a".to_owned(), GoJson::number(1)),
            (
                "b".to_owned(),
                GoJson::Array(vec![GoJson::String("x\"y".to_owned())]),
            ),
            ("c".to_owned(), GoJson::Array(Vec::new())),
            ("d".to_owned(), GoJson::Object(Vec::new())),
        ]);
        assert_eq!(
            value.indented(),
            "{\n  \"a\": 1,\n  \"b\": [\n    \"x\\\"y\"\n  ],\n  \"c\": [],\n  \"d\": {}\n}"
        );
        assert_eq!(
            value.compact(),
            "{\"a\":1,\"b\":[\"x\\\"y\"],\"c\":[],\"d\":{}}"
        );
        assert_eq!(GoJson::Object(Vec::new()).indented(), "{}");
    }

    #[test]
    fn enum_numbers_are_tsgo_numbers() {
        assert_eq!(enum_number("moduleDetection", "Auto"), Some(1));
        assert_eq!(enum_number("newLine", "crlf"), Some(1));
        assert_eq!(enum_number("watchFile", "UseFsEvents"), Some(5));
        assert_eq!(enum_number("target", "ES2017"), Some(4));
        assert_eq!(enum_number("target", "es3"), None);
    }
}
