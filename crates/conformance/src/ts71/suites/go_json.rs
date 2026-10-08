//! The JSON tsgo's tests write (`encoding/json/v2` through
//! `tsc/internal/json`): compact and two-space indented text, the field
//! order of `core.CompilerOptions`, `core.BuildOptions` and
//! `core.TypeAcquisition` (`omitzero` leaves out unset fields), and the
//! numbers of tsgo's option enums.

/// A JSON value in the order tsgo writes it.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum GoJson {
    Null,
    Bool(bool),
    /// The decimal text of a number.
    Number(String),
    String(String),
    Array(Vec<GoJson>),
    Object(Vec<(String, GoJson)>),
}

impl GoJson {
    pub(super) fn number(value: i64) -> Self {
        Self::Number(value.to_string())
    }

    /// A tsc-rs JSON value; a number takes Go's shortest form (an integral
    /// `float64` has no fraction).
    pub(super) fn from_json(value: &tsc_program::JsonValue) -> Self {
        use tsc_program::JsonValue;
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
    pub(super) fn compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// `json.MarshalIndent(value, "", "  ")`.
    pub(super) fn indented(&self) -> String {
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
pub(super) enum FieldKind {
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
pub(super) const COMPILER_OPTIONS_FIELDS: &[(&str, FieldKind)] = &[
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
pub(super) const BUILD_OPTIONS_FIELDS: &[(&str, FieldKind)] = &[
    ("dry", FieldKind::Tristate),
    ("force", FieldKind::Tristate),
    ("verbose", FieldKind::Tristate),
    ("builders", FieldKind::Int),
    ("stopBuildOnErrors", FieldKind::Tristate),
    ("clean", FieldKind::Tristate),
];

/// `core.TypeAcquisition`'s fields in declaration order
/// (core/typeacquisition.go).
pub(super) const TYPE_ACQUISITION_FIELDS: &[(&str, FieldKind)] = &[
    ("enable", FieldKind::Tristate),
    ("include", FieldKind::StringList),
    ("exclude", FieldKind::StringList),
    ("disableFilenameBasedTypeAcquisition", FieldKind::Tristate),
];

/// tsgo's number for the spelling of an enum option (tsoptions/enummaps.go
/// with the core and watch-option constants), case-insensitively.
pub(super) fn enum_number(option: &str, spelling: &str) -> Option<i64> {
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
