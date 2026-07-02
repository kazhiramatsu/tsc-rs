//! Compiler options + tsc-style CLI flag parsing (subset).

/// `paths` substitution-map value for one pattern key (TS5063/5064 keep the
/// JSON value kind; element display uses JS String() semantics).
#[derive(Clone, Debug)]
pub enum PathsValue {
    Array(Vec<PathsElem>),
    NotArray,
}

#[derive(Clone, Debug)]
pub enum PathsElem {
    Str(String),
    Other {
        display: String,
        type_name: &'static str,
    },
}

#[derive(Clone, Debug, Default)]
pub struct CompilerOptions {
    pub strict: Option<bool>,
    pub strict_null_checks: Option<bool>,
    pub strict_function_types: Option<bool>,
    pub strict_property_initialization: Option<bool>,
    pub strict_bind_call_apply: Option<bool>,
    pub no_implicit_any: Option<bool>,
    pub no_implicit_this: Option<bool>,
    pub use_unknown_in_catch_variables: Option<bool>,
    pub no_unused_locals: bool,
    pub no_unused_parameters: bool,
    pub no_implicit_returns: bool,
    pub no_fallthrough_cases_in_switch: bool,
    pub exact_optional_property_types: bool,
    pub no_unchecked_indexed_access: bool,
    pub no_implicit_override: bool,
    pub erasable_syntax_only: bool,
    pub allow_unreachable_code: Option<bool>,
    pub allow_unused_labels: Option<bool>,
    pub no_emit: bool,
    pub use_define_for_class_fields: Option<bool>,
    // emit-adjacent options (validated, not used for emission)
    pub experimental_decorators: bool,
    pub emit_decorator_metadata: bool,
    pub source_map: bool,
    pub inline_source_map: bool,
    pub inline_sources: bool,
    pub declaration: bool,
    pub declaration_map: bool,
    pub declaration_dir: Option<String>,
    pub composite: bool,
    pub isolated_declarations: bool,
    pub allow_importing_ts_extensions: bool,
    pub rewrite_relative_import_extensions: bool,
    pub resolve_package_json_exports: Option<bool>,
    pub resolve_package_json_imports: Option<bool>,
    pub emit_declaration_only: bool,
    pub out_file: Option<String>,
    pub resolve_json_module: Option<bool>,
    pub module_resolution: Option<String>,
    pub incremental: bool,
    pub ts_build_info_file: Option<String>,
    pub map_root: Option<String>,
    pub module: Option<String>,
    /// lowercase ("es3" | "es5" | "es2015" | ...). None ≡ es5 (the harness
    /// BASE_OPTIONS always passes target: ES5, which tsrs mirrors).
    pub target: Option<String>,
    pub isolated_modules: bool,
    pub verbatim_module_syntax: bool,
    pub preserve_const_enums: Option<bool>,
    pub jsx: Option<String>,
    pub jsx_factory: Option<String>,
    pub jsx_fragment_factory: Option<String>,
    pub react_namespace: Option<String>,
    pub jsx_import_source: Option<String>,
    pub paths: Option<Vec<(String, PathsValue)>>,
    pub base_url: Option<String>,
    /// None ≡ "6.0" (mirrors the harness BASE_OPTIONS silencer); only an
    /// explicit value changes deprecation reporting.
    pub ignore_deprecations: Option<String>,
    /// `--locale` — validated for form (TS6048); tsrs ships English only, so a
    /// well-formed locale is otherwise a no-op (like tsc with no locale file).
    pub locale: Option<String>,
    pub root_dir: Option<String>,
    /// Current directory used for DISPLAY purposes only (absolute resolved
    /// paths embedded in TS6059/TS6142/TS6263 messages). The CLI fills it from
    /// getcwd / TSRS_VIRTUAL_CWD; harnesses pin "/difftest".
    pub current_directory: Option<String>,
    pub always_strict: Option<bool>,
    pub es_module_interop: Option<bool>,
    pub allow_synthetic_default_imports: Option<bool>,
    pub downlevel_iteration: Option<bool>,
    pub import_helpers: bool,
    // options removed in TS 6.0 (still parsed; reported via TS5102/5108)
    pub charset: Option<String>,
    pub out: Option<String>,
    pub imports_not_used_as_values: Option<String>,
    pub no_implicit_use_strict: bool,
    pub keyof_strings_only: bool,
    pub suppress_excess_property_errors: bool,
    pub suppress_implicit_any_index_errors: bool,
    pub no_strict_generic_checks: bool,
    pub preserve_value_imports: bool,
    /// `--types a,b` — tsrs performs no @types resolution; the value only
    /// drives usesWildcardTypes (the 2580/2581/2582 vs 2591/2592/2593
    /// message-variant selection). Fixtures use `@types: *` exclusively.
    pub types: Option<Vec<String>>,
    /// Harness-only: emit diagnostics as structured JSON (code, location,
    /// messageChain, relatedInformation) instead of tsc-style text, for the
    /// Phase-2 diagnostic oracle comparison.
    pub diag_json: bool,
}

impl CompilerOptions {
    fn strict_default(&self, explicit: Option<bool>) -> bool {
        explicit.unwrap_or_else(|| self.strict.unwrap_or(false))
    }
    pub fn strict_null_checks(&self) -> bool {
        self.strict_default(self.strict_null_checks)
    }
    pub fn strict_function_types(&self) -> bool {
        self.strict_default(self.strict_function_types)
    }
    pub fn strict_property_initialization(&self) -> bool {
        self.strict_default(self.strict_property_initialization)
    }
    pub fn no_implicit_any(&self) -> bool {
        self.strict_default(self.no_implicit_any)
    }
    pub fn use_unknown_in_catch_variables(&self) -> bool {
        self.strict_default(self.use_unknown_in_catch_variables)
    }
    pub fn no_implicit_this(&self) -> bool {
        self.strict_default(self.no_implicit_this)
    }
    pub fn use_define_for_class_fields(&self) -> bool {
        self.use_define_for_class_fields
            .unwrap_or_else(|| self.script_target_rank() >= 9)
    }

    /// tsc _computedOptions.alwaysStrict (TS 6.0): true unless explicitly false
    /// (no longer derived from `strict`).
    pub fn always_strict(&self) -> bool {
        self.always_strict != Some(false)
    }

    /// tsc _computedOptions.module: explicit, else from target (es5 → commonjs).
    /// tsc usesWildcardTypes: `types` contains "*" — selects the short
    /// install-type-definitions messages (2580/2581/2582) over the
    /// "...and then add X to the types field" variants (2591/2592/2593).
    pub fn uses_wildcard_types(&self) -> bool {
        self.types
            .as_ref()
            .is_some_and(|ts| ts.iter().any(|t| t == "*"))
    }

    pub fn module_kind(&self) -> &str {
        if let Some(m) = self.module.as_deref() {
            return m;
        }
        match self.script_target_rank() {
            r if r >= 2 => "es2015",
            _ => "commonjs",
        }
    }

    /// moduleKind in tsc's ES2015..ESNext range (5 <= kind <= 99) — gates
    /// 1202/1203 and selects the flag name in the 1259 message.
    pub fn module_kind_is_es(&self) -> bool {
        matches!(
            self.module_kind(),
            "es6" | "es2015" | "es2020" | "es2022" | "esnext"
        )
    }

    /// tsc moduleSupportsImportAttributes (2821/2823 gate)
    pub fn module_supports_import_attributes(&self) -> bool {
        matches!(
            self.module_kind(),
            "esnext" | "node18" | "node20" | "nodenext" | "preserve"
        )
    }

    /// tsc Node16 <= moduleKind <= NodeNext
    pub fn module_kind_is_node(&self) -> bool {
        matches!(
            self.module_kind(),
            "node16" | "node18" | "node20" | "nodenext"
        )
    }

    /// checkGrammarImportCallExpression's second-argument gate (1324):
    /// node16..nodenext, esnext, and preserve allow `import(spec, opts)`
    pub fn module_allows_import_call_options(&self) -> bool {
        self.module_kind_is_node() || matches!(self.module_kind(), "esnext" | "preserve")
    }

    /// raw ignoreDeprecations compared by tsc's 2880 sites; the harness pins
    /// "6.0", so an unset value behaves as "6.0" (same convention as
    /// dep60_active in verify_compiler_options)
    pub fn ignore_deprecations_below_60(&self) -> bool {
        matches!(self.ignore_deprecations.as_deref(), Some(v) if v != "6.0")
    }

    /// tsc getIsolatedModules: isolatedModules || verbatimModuleSyntax —
    /// gates the type-only import/export agreement checks (1205/1448/...)
    pub fn isolated_modules_like(&self) -> bool {
        self.isolated_modules || self.verbatim_module_syntax
    }

    /// tsc isolatedModulesLikeFlagName: the flag named in the 1205/1448
    /// message family
    pub fn isolated_modules_like_flag_name(&self) -> &'static str {
        if self.verbatim_module_syntax {
            "verbatimModuleSyntax"
        } else {
            "isolatedModules"
        }
    }

    /// tsc _computedOptions.allowSyntheticDefaultImports (TS 6.0): explicit
    /// value, else true (no longer derived from esModuleInterop).
    pub fn allow_synthetic_default_imports(&self) -> bool {
        self.allow_synthetic_default_imports != Some(false)
    }

    /// tsc getESModuleInterop (TS 6.0 computed default): explicit value,
    /// else true. Selects between the 2595/2596 and 2616/2617 message pairs.
    pub fn es_module_interop(&self) -> bool {
        self.es_module_interop != Some(false)
    }

    /// ScriptTarget ordering for the comparisons verifyCompilerOptions makes
    /// (ES5=1, ES2015=2, ...). target:ES3 computes to LatestStandard in 6.0.
    pub fn script_target_rank(&self) -> u8 {
        match self.target.as_deref().unwrap_or("es5") {
            "es3" => 12, // ES3 → undefined → LatestStandard (tsc 6.0)
            "es5" => 1,
            "es2015" | "es6" => 2,
            "es2016" => 3,
            "es2017" => 4,
            "es2018" => 5,
            "es2019" => 6,
            "es2020" => 7,
            "es2021" => 8,
            "es2022" => 9,
            _ => 12,
        }
    }

    /// Exact tsc ScriptTarget value (ES5=1 … ES2024=11, ES2025=12, ESNext=99)
    /// for the scanner-level languageVersion comparisons the regex validator
    /// makes (flag availability 1501, named groups 1503). script_target_rank
    /// collapses ≥ES2023 to 12, which is too coarse here: the `v` flag needs
    /// ES2024 exactly.
    pub fn language_version(&self) -> u32 {
        match self.target.as_deref().unwrap_or("es5") {
            "es5" => 1,
            "es2015" | "es6" => 2,
            "es2016" => 3,
            "es2017" => 4,
            "es2018" => 5,
            "es2019" => 6,
            "es2020" => 7,
            "es2021" => 8,
            "es2022" => 9,
            "es2023" => 10,
            "es2024" => 11,
            "esnext" => 99,
            _ => 12, // es3/es2025/unknown → LatestStandard (tsc 6.0)
        }
    }

    /// tsc _computedOptions.moduleResolution: explicit, else by module kind
    /// (none/amd/umd/system → classic; node16/nodenext → same; else bundler).
    pub fn module_resolution_kind(&self) -> &str {
        if let Some(mr) = self.module_resolution.as_deref() {
            return mr;
        }
        match self.module_kind() {
            "none" | "amd" | "umd" | "system" => "classic",
            "nodenext" => "nodenext",
            "node16" | "node18" | "node20" => "node16",
            _ => "bundler",
        }
    }

    /// tsc getResolveJsonModule: explicit, else node20/nodenext module, else
    /// moduleResolution === bundler.
    pub fn resolve_json_module(&self) -> bool {
        if let Some(b) = self.resolve_json_module {
            return b;
        }
        matches!(self.module_kind(), "node20" | "nodenext")
            || self.module_resolution_kind() == "bundler"
    }
}

/// tsc enum-typed CLI option: every accepted value (the option's `type` map
/// keys, in definition order) plus which of them are deprecated. TS6046 lists
/// only the non-deprecated ones but deprecated values still parse.
struct EnumOption {
    /// camelCase name as echoed in `Argument for '--{name}' option must be:`.
    name: &'static str,
    values: &'static [&'static str],
    deprecated: &'static [&'static str],
}

const ENUM_OPTIONS: [EnumOption; 6] = [
    EnumOption {
        name: "target",
        values: &[
            "es3", "es5", "es6", "es2015", "es2016", "es2017", "es2018", "es2019", "es2020",
            "es2021", "es2022", "es2023", "es2024", "es2025", "esnext",
        ],
        deprecated: &["es3", "es5"],
    },
    EnumOption {
        name: "module",
        values: &[
            "none", "commonjs", "amd", "system", "umd", "es6", "es2015", "es2020", "es2022",
            "esnext", "node16", "node18", "node20", "nodenext", "preserve",
        ],
        deprecated: &["none", "amd", "system", "umd"],
    },
    EnumOption {
        name: "moduleResolution",
        values: &["classic", "node", "node10", "node16", "nodenext", "bundler"],
        deprecated: &["classic", "node", "node10"],
    },
    EnumOption {
        name: "jsx",
        values: &[
            "preserve",
            "react-native",
            "react-jsx",
            "react-jsxdev",
            "react",
        ],
        deprecated: &[],
    },
    EnumOption {
        name: "newLine",
        values: &["crlf", "lf"],
        deprecated: &[],
    },
    EnumOption {
        name: "importsNotUsedAsValues",
        values: &["remove", "preserve", "error"],
        deprecated: &[],
    },
];

fn file_less(msg: &'static DiagnosticMessage, args: &[&str]) -> Diagnostic {
    Diagnostic {
        file: None,
        start: 0,
        length: 0,
        message: MessageChain::new(msg, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
        related: Vec::new(),
    }
}

/// tsc createDiagnosticForInvalidCustomType: list the non-deprecated values.
fn invalid_enum_value(opt: &EnumOption) -> Diagnostic {
    let list = opt
        .values
        .iter()
        .filter(|v| !opt.deprecated.contains(v))
        .map(|v| format!("'{v}'"))
        .collect::<Vec<_>>()
        .join(", ");
    file_less(
        &gen::Argument_for_0_option_must_be_Colon_1,
        &[&format!("--{}", opt.name), &list],
    )
}

pub struct ParsedCommandLine {
    pub options: CompilerOptions,
    pub files: Vec<String>,
    /// Command-line errors (TS6045/5083 response files, TS6046 enum values,
    /// TS6048 locale form). tsc reports these and exits 1 without compiling.
    pub errors: Vec<Diagnostic>,
}

/// tsc parseCommandLine + executeCommandLineWorker's locale validation.
/// `read` supplies `@file` response-file contents (host-dependent).
pub fn parse_command_line(
    args: &[String],
    read: &mut dyn FnMut(&str) -> Option<String>,
) -> ParsedCommandLine {
    let mut errors = Vec::new();
    let mut expanded = Vec::new();
    expand_response_files(args, read, &mut expanded, &mut errors);
    let (options, files) = parse_flags(&expanded, &mut errors);
    if let Some(locale) = &options.locale {
        if !is_valid_locale_form(locale) {
            errors.push(file_less(
                &gen::Locale_must_be_of_the_form_language_or_language_territory_For_example_0_or_1,
                &["en", "ja-jp"],
            ));
        }
    }
    ParsedCommandLine {
        options,
        files,
        errors,
    }
}

/// `/^([a-z]+)(?:[_-]([a-z]+))?$/` over the lowercased locale.
fn is_valid_locale_form(locale: &str) -> bool {
    let lower = locale.to_lowercase();
    let mut parts = lower.splitn(2, ['-', '_']);
    let lang = parts.next().unwrap_or("");
    let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase());
    ok(lang) && parts.next().map_or(true, ok)
}

/// `@file` arguments expand in place to the file's whitespace-separated tokens
/// (double-quote groups, no escapes); nested response files recurse. Ports
/// tsc parseResponseFile.
fn expand_response_files(
    args: &[String],
    read: &mut dyn FnMut(&str) -> Option<String>,
    out: &mut Vec<String>,
    errors: &mut Vec<Diagnostic>,
) {
    for a in args {
        let Some(name) = a.strip_prefix('@') else {
            out.push(a.clone());
            continue;
        };
        let Some(text) = read(name) else {
            errors.push(file_less(&gen::Cannot_read_file_0, &[name]));
            continue;
        };
        let chars: Vec<char> = text.chars().collect();
        let mut tokens = Vec::new();
        let mut pos = 0;
        while pos < chars.len() {
            while pos < chars.len() && (chars[pos] as u32) <= 32 {
                pos += 1;
            }
            if pos >= chars.len() {
                break;
            }
            let start = pos;
            if chars[start] == '"' {
                pos += 1;
                while pos < chars.len() && chars[pos] != '"' {
                    pos += 1;
                }
                if pos < chars.len() {
                    tokens.push(chars[start + 1..pos].iter().collect::<String>());
                    pos += 1;
                } else {
                    errors.push(file_less(
                        &gen::Unterminated_quoted_string_in_response_file_0,
                        &[name],
                    ));
                }
            } else {
                while pos < chars.len() && (chars[pos] as u32) > 32 {
                    pos += 1;
                }
                tokens.push(chars[start..pos].iter().collect::<String>());
            }
        }
        expand_response_files(&tokens, read, out, errors);
    }
}

/// Parse argv (excluding argv[0]) into options + file list.
/// Accepts the tsc spellings used by the harness: `--flag`, `--flag true|false`,
/// `--target es5`, `--module commonjs`, `--ignoreDeprecations 6.0`. Unknown
/// flags are ignored so a tsc-compatible command line can be passed verbatim;
/// enum-typed values are validated (TS6046 into `errors`).
fn parse_flags(args: &[String], errors: &mut Vec<Diagnostic>) -> (CompilerOptions, Vec<String>) {
    let mut opts = CompilerOptions::default();
    let mut files = Vec::new();
    let mut i = 0;

    fn bool_value(args: &[String], i: &mut usize) -> bool {
        if *i + 1 < args.len() {
            match args[*i + 1].as_str() {
                "true" => {
                    *i += 1;
                    return true;
                }
                "false" => {
                    *i += 1;
                    return false;
                }
                _ => {}
            }
        }
        true
    }

    while i < args.len() {
        let a = args[i].as_str();
        if let Some(flag) = a.strip_prefix("--") {
            // enum-typed options: consume the next token unconditionally (tsc
            // parseOptionValue), validate against the option's value map
            if let Some(eo) = ENUM_OPTIONS
                .iter()
                .find(|e| e.name.eq_ignore_ascii_case(flag))
            {
                let raw = if i + 1 < args.len() {
                    i += 1;
                    args[i].trim().to_string()
                } else {
                    String::new()
                };
                if raw.is_empty() {
                    errors.push(file_less(
                        &gen::Compiler_option_0_expects_an_argument,
                        &[eo.name],
                    ));
                    errors.push(invalid_enum_value(eo));
                } else {
                    let v = raw.to_ascii_lowercase();
                    if eo.values.contains(&v.as_str()) {
                        match eo.name {
                            "target" => opts.target = Some(v),
                            "module" => opts.module = Some(v),
                            "moduleResolution" => opts.module_resolution = Some(v),
                            "jsx" => opts.jsx = Some(v),
                            "importsNotUsedAsValues" => opts.imports_not_used_as_values = Some(v),
                            _ => {} // newLine: validated, not stored
                        }
                    } else {
                        errors.push(invalid_enum_value(eo));
                    }
                }
                i += 1;
                continue;
            }
            match flag.to_ascii_lowercase().as_str() {
                "strict" => opts.strict = Some(bool_value(args, &mut i)),
                "strictnullchecks" => opts.strict_null_checks = Some(bool_value(args, &mut i)),
                "strictfunctiontypes" => {
                    opts.strict_function_types = Some(bool_value(args, &mut i))
                }
                "strictpropertyinitialization" => {
                    opts.strict_property_initialization = Some(bool_value(args, &mut i))
                }
                "strictbindcallapply" => {
                    opts.strict_bind_call_apply = Some(bool_value(args, &mut i))
                }
                "noimplicitany" => opts.no_implicit_any = Some(bool_value(args, &mut i)),
                "noimplicitthis" => opts.no_implicit_this = Some(bool_value(args, &mut i)),
                "useunknownincatchvariables" => {
                    opts.use_unknown_in_catch_variables = Some(bool_value(args, &mut i))
                }
                "nounusedlocals" => opts.no_unused_locals = bool_value(args, &mut i),
                "nounusedparameters" => opts.no_unused_parameters = bool_value(args, &mut i),
                "noimplicitreturns" => opts.no_implicit_returns = bool_value(args, &mut i),
                "nofallthroughcasesinswitch" => {
                    opts.no_fallthrough_cases_in_switch = bool_value(args, &mut i)
                }
                "usedefineforclassfields" => {
                    opts.use_define_for_class_fields = Some(bool_value(args, &mut i))
                }
                "exactoptionalpropertytypes" => {
                    opts.exact_optional_property_types = bool_value(args, &mut i)
                }
                "nouncheckedindexedaccess" => {
                    opts.no_unchecked_indexed_access = bool_value(args, &mut i)
                }
                "noimplicitoverride" => opts.no_implicit_override = bool_value(args, &mut i),
                "erasablesyntaxonly" => opts.erasable_syntax_only = bool_value(args, &mut i),
                "allowunreachablecode" => {
                    opts.allow_unreachable_code = Some(bool_value(args, &mut i))
                }
                "allowunusedlabels" => opts.allow_unused_labels = Some(bool_value(args, &mut i)),
                "noemit" => opts.no_emit = bool_value(args, &mut i),
                "experimentaldecorators" => opts.experimental_decorators = bool_value(args, &mut i),
                "emitdecoratormetadata" => opts.emit_decorator_metadata = bool_value(args, &mut i),
                "sourcemap" => opts.source_map = bool_value(args, &mut i),
                "inlinesourcemap" => opts.inline_source_map = bool_value(args, &mut i),
                "inlinesources" => opts.inline_sources = bool_value(args, &mut i),
                "declaration" => opts.declaration = bool_value(args, &mut i),
                "declarationmap" => opts.declaration_map = bool_value(args, &mut i),
                "composite" => opts.composite = bool_value(args, &mut i),
                "isolateddeclarations" => opts.isolated_declarations = bool_value(args, &mut i),
                "allowimportingtsextensions" => {
                    opts.allow_importing_ts_extensions = bool_value(args, &mut i)
                }
                "rewriterelativeimportextensions" => {
                    opts.rewrite_relative_import_extensions = bool_value(args, &mut i)
                }
                "resolvepackagejsonexports" => {
                    opts.resolve_package_json_exports = Some(bool_value(args, &mut i))
                }
                "resolvepackagejsonimports" => {
                    opts.resolve_package_json_imports = Some(bool_value(args, &mut i))
                }
                "emitdeclarationonly" => opts.emit_declaration_only = bool_value(args, &mut i),
                "resolvejsonmodule" => opts.resolve_json_module = Some(bool_value(args, &mut i)),
                "incremental" => opts.incremental = bool_value(args, &mut i),
                "isolatedmodules" => opts.isolated_modules = bool_value(args, &mut i),
                "verbatimmodulesyntax" => opts.verbatim_module_syntax = bool_value(args, &mut i),
                "preserveconstenums" => opts.preserve_const_enums = Some(bool_value(args, &mut i)),
                "alwaysstrict" => opts.always_strict = Some(bool_value(args, &mut i)),
                "esmoduleinterop" => opts.es_module_interop = Some(bool_value(args, &mut i)),
                "allowsyntheticdefaultimports" => {
                    opts.allow_synthetic_default_imports = Some(bool_value(args, &mut i))
                }
                "downleveliteration" => opts.downlevel_iteration = Some(bool_value(args, &mut i)),
                "importhelpers" => opts.import_helpers = bool_value(args, &mut i),
                "noimplicitusestrict" => opts.no_implicit_use_strict = bool_value(args, &mut i),
                "keyofstringsonly" => opts.keyof_strings_only = bool_value(args, &mut i),
                "suppressexcesspropertyerrors" => {
                    opts.suppress_excess_property_errors = bool_value(args, &mut i)
                }
                "suppressimplicitanyindexerrors" => {
                    opts.suppress_implicit_any_index_errors = bool_value(args, &mut i)
                }
                "nostrictgenericchecks" => opts.no_strict_generic_checks = bool_value(args, &mut i),
                "preservevalueimports" => opts.preserve_value_imports = bool_value(args, &mut i),
                "declarationdir" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.declaration_dir = Some(args[i].clone());
                    }
                }
                "outfile" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.out_file = Some(args[i].clone());
                    }
                }
                "locale" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.locale = Some(args[i].clone());
                    }
                }
                "rootdir" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.root_dir = Some(args[i].clone());
                    }
                }
                "tsbuildinfofile" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.ts_build_info_file = Some(args[i].clone());
                    }
                }
                "maproot" => {
                    if i + 1 < args.len() {
                        i += 1;
                        opts.map_root = Some(args[i].clone());
                    }
                }
                "jsxfactory" | "jsxfragmentfactory" | "reactnamespace" | "jsximportsource"
                | "baseurl" | "charset" | "out" | "paths" | "ignoredeprecations" => {
                    if i + 1 < args.len() {
                        i += 1;
                        let v = args[i].clone();
                        match flag.to_ascii_lowercase().as_str() {
                            "jsxfactory" => opts.jsx_factory = Some(v),
                            "jsxfragmentfactory" => opts.jsx_fragment_factory = Some(v),
                            "reactnamespace" => opts.react_namespace = Some(v),
                            "jsximportsource" => opts.jsx_import_source = Some(v),
                            "baseurl" => opts.base_url = Some(v),
                            "charset" => opts.charset = Some(v),
                            "out" => opts.out = Some(v),
                            "paths" => opts.paths = parse_paths_json(&v),
                            "ignoredeprecations" => opts.ignore_deprecations = Some(v),
                            _ => unreachable!(),
                        }
                    }
                }
                "nolib" | "pretty" | "skipdefaultlibcheck" | "skiplibcheck" => {
                    // accepted for tsc-compatible argv; --noLib is implicit (we
                    // have no default lib), --pretty false is the only mode,
                    // and lib checking is irrelevant to the fixed explicit lib.
                    let _ = bool_value(args, &mut i);
                }
                "types" => {
                    if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                        i += 1;
                        opts.types = Some(
                            args[i]
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect(),
                        );
                    }
                }
                _ => {}
            }
        } else {
            files.push(a.to_string());
        }
        i += 1;
    }
    (opts, files)
}

/// Minimal JSON object parser for the `--paths {"pat":["sub",...]}` flag /
/// `@paths:` directive. Only the shapes verifyCompilerOptions distinguishes
/// are kept: per-key array-or-not, per-element string-or-(typeof, String()).
pub fn parse_paths_json(s: &str) -> Option<Vec<(String, PathsValue)>> {
    let b = s.as_bytes();
    let mut i = 0usize;
    fn skip_ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && (b[*i] as char).is_ascii_whitespace() {
            *i += 1;
        }
    }
    fn parse_string(b: &[u8], i: &mut usize) -> Option<String> {
        if b.get(*i) != Some(&b'"') {
            return None;
        }
        *i += 1;
        let mut out = String::new();
        while *i < b.len() {
            match b[*i] {
                b'"' => {
                    *i += 1;
                    return Some(out);
                }
                b'\\' => {
                    *i += 1;
                    match b.get(*i)? {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'n' => out.push('\n'),
                        b't' => out.push('\t'),
                        c => out.push(*c as char),
                    }
                    *i += 1;
                }
                c => {
                    out.push(c as char);
                    *i += 1;
                }
            }
        }
        None
    }
    // returns (display via JS String(), typeof name); consumes one JSON value
    fn parse_other(b: &[u8], i: &mut usize) -> Option<(String, &'static str)> {
        let rest = &b[*i..];
        for (lit, disp, ty) in [
            ("true", "true", "boolean"),
            ("false", "false", "boolean"),
            ("null", "null", "object"),
        ] {
            if rest.starts_with(lit.as_bytes()) {
                *i += lit.len();
                return Some((disp.to_string(), ty));
            }
        }
        let start = *i;
        if matches!(b.get(*i), Some(b'-') | Some(b'0'..=b'9')) {
            *i += 1;
            while matches!(
                b.get(*i),
                Some(b'0'..=b'9') | Some(b'.') | Some(b'e') | Some(b'E') | Some(b'+') | Some(b'-')
            ) {
                *i += 1;
            }
            let txt = std::str::from_utf8(&b[start..*i]).ok()?;
            let n: f64 = txt.parse().ok()?;
            return Some((crate::js_num::to_js_string(n), "number"));
        }
        if b.get(*i) == Some(&b'{') {
            // skip a (non-nested-string-aware enough) object literal
            let mut depth = 0i32;
            while *i < b.len() {
                match b[*i] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            *i += 1;
                            return Some(("[object Object]".to_string(), "object"));
                        }
                    }
                    b'"' => {
                        parse_string(b, i);
                        continue;
                    }
                    _ => {}
                }
                *i += 1;
            }
        }
        None
    }
    skip_ws(b, &mut i);
    if b.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    let mut out = Vec::new();
    loop {
        skip_ws(b, &mut i);
        if b.get(i) == Some(&b'}') {
            return Some(out);
        }
        let key = parse_string(b, &mut i)?;
        skip_ws(b, &mut i);
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        skip_ws(b, &mut i);
        let value = if b.get(i) == Some(&b'[') {
            i += 1;
            let mut elems = Vec::new();
            loop {
                skip_ws(b, &mut i);
                if b.get(i) == Some(&b']') {
                    i += 1;
                    break;
                }
                if b.get(i) == Some(&b'"') {
                    elems.push(PathsElem::Str(parse_string(b, &mut i)?));
                } else {
                    let (display, type_name) = parse_other(b, &mut i)?;
                    elems.push(PathsElem::Other { display, type_name });
                }
                skip_ws(b, &mut i);
                if b.get(i) == Some(&b',') {
                    i += 1;
                }
            }
            PathsValue::Array(elems)
        } else if b.get(i) == Some(&b'"') {
            parse_string(b, &mut i)?;
            PathsValue::NotArray
        } else {
            parse_other(b, &mut i)?;
            PathsValue::NotArray
        };
        out.push((key, value));
        skip_ws(b, &mut i);
        if b.get(i) == Some(&b',') {
            i += 1;
        }
    }
}

use crate::diagnostics::{gen, Diagnostic, DiagnosticMessage, MessageChain};

/// tsc checkCompilerOptions subset: file-less options diagnostics. These gate
/// semantic output exactly like tsc's getOptionsDiagnostics.
pub fn check_options(opts: &CompilerOptions) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut err = |msg: &'static DiagnosticMessage, args: &[&str]| {
        out.push(Diagnostic {
            file: None,
            start: 0,
            length: 0,
            message: MessageChain::new(
                msg,
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            ),
            related: Vec::new(),
        });
    };
    let snc = opts.strict_null_checks();
    if opts.strict_property_initialization.unwrap_or(false) && !snc {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1,
            &["strictPropertyInitialization", "strictNullChecks"],
        );
    }
    if opts.exact_optional_property_types && !snc {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1,
            &["exactOptionalPropertyTypes", "strictNullChecks"],
        );
    }
    if opts.emit_decorator_metadata && !opts.experimental_decorators {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1,
            &["emitDecoratorMetadata", "experimentalDecorators"],
        );
    }
    if opts.source_map && opts.inline_source_map {
        err(
            &gen::Option_0_cannot_be_specified_with_option_1,
            &["sourceMap", "inlineSourceMap"],
        );
    }
    if opts.inline_sources && !opts.source_map && !opts.inline_source_map {
        err(
            &gen::Option_0_can_only_be_used_when_either_option_inlineSourceMap_or_option_sourceMap_is_provided,
            &["inlineSources"],
        );
    }
    let decl_or_composite = opts.declaration || opts.composite;
    if opts.declaration_dir.is_some() && !decl_or_composite {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
            &["declarationDir", "declaration", "composite"],
        );
    }
    if opts.declaration_map && !decl_or_composite {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
            &["declarationMap", "declaration", "composite"],
        );
    }
    if opts.isolated_declarations && !decl_or_composite {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
            &["isolatedDeclarations", "declaration", "composite"],
        );
    }
    if opts.map_root.is_some() && !(opts.source_map || opts.declaration_map) {
        err(
            &gen::Option_0_cannot_be_specified_without_specifying_option_1_or_option_2,
            &["mapRoot", "sourceMap", "declarationMap"],
        );
    }
    if opts.allow_importing_ts_extensions
        && !(opts.no_emit || opts.emit_declaration_only || opts.rewrite_relative_import_extensions)
    {
        err(
            &gen::Option_allowImportingTsExtensions_can_only_be_used_when_one_of_noEmit_emitDeclarationOnly_or_rewriteRelativeImportExtensions_is_set,
            &[],
        );
    }
    if opts.out_file.is_some() && !opts.emit_declaration_only {
        // RAW module only: when module is unset the program-level TS6131
        // (file diagnostic at the first external-module indicator) applies
        // instead — see check_program_core
        if let Some(m) = opts.module.as_deref() {
            if m != "amd" && m != "system" {
                err(
                    &gen::Only_amd_and_system_modules_are_supported_alongside_0,
                    &["outFile"],
                );
            }
        }
    }
    // tsc verifyCompilerOptions module/moduleResolution agreement (both sides
    // computed): a node-family `module` needs a node16..nodenext resolution
    // (5109; the suggested name is ModuleKind[moduleKind] when that is also a
    // ModuleResolutionKind name, else "Node16" — Node18/Node20 are not), and a
    // node16/nodenext resolution needs a node-family `module` (5110)
    {
        let mk = opts.module_kind();
        let mk_node = matches!(mk, "node16" | "node18" | "node20" | "nodenext");
        let mr = opts.module_resolution_kind();
        let mr_node = matches!(mr, "node16" | "nodenext");
        if mk_node && !mr_node {
            let res_label = match mk {
                "node16" => "Node16",
                "nodenext" => "NodeNext",
                _ => "Node16",
            };
            let mk_label = match mk {
                "node16" => "Node16",
                "node18" => "Node18",
                "node20" => "Node20",
                _ => "NodeNext",
            };
            err(
                &gen::Option_moduleResolution_must_be_set_to_0_or_left_unspecified_when_option_module_is_set_to_1,
                &[res_label, mk_label],
            );
        } else if mr_node && !mk_node {
            let label = if mr == "node16" { "Node16" } else { "NodeNext" };
            err(
                &gen::Option_module_must_be_set_to_0_when_option_moduleResolution_is_set_to_1,
                &[label, label],
            );
        }
    }
    // tsc: resolvePackageJsonExports/Imports demand a resolution mode that
    // reads package.json (node16..nodenext or bundler); raw option values —
    // the computed defaults never reach verifyCompilerOptions here
    if !matches!(
        opts.module_resolution_kind(),
        "node16" | "nodenext" | "bundler"
    ) {
        if opts.resolve_package_json_exports == Some(true) {
            err(
                &gen::Option_0_can_only_be_used_when_moduleResolution_is_set_to_node16_nodenext_or_bundler,
                &["resolvePackageJsonExports"],
            );
        }
        if opts.resolve_package_json_imports == Some(true) {
            err(
                &gen::Option_0_can_only_be_used_when_moduleResolution_is_set_to_node16_nodenext_or_bundler,
                &["resolvePackageJsonImports"],
            );
        }
    }
    // getResolveJsonModule is computed (bundler resolution implies true); the
    // classic branch wins over the none/system/umd branch (else-if in tsc).
    if opts.resolve_json_module() {
        if opts.module_resolution_kind() == "classic" {
            err(
                &gen::Option_resolveJsonModule_cannot_be_specified_when_moduleResolution_is_set_to_classic,
                &[],
            );
        } else if matches!(opts.module_kind(), "none" | "system" | "umd") {
            err(
                &gen::Option_resolveJsonModule_cannot_be_specified_when_module_is_set_to_none_system_or_umd,
                &[],
            );
        }
    }
    if opts.composite && !opts.declaration {
        // composite implies declaration; explicit declaration:false conflicts
        // (CLI: composite alone sets declaration, so only the explicit pair
        // reaches here through the harness)
    }
    if opts.incremental && opts.ts_build_info_file.is_none() && opts.out_file.is_none() {
        err(
            &gen::Option_incremental_can_only_be_specified_using_tsconfig_emitting_to_single_file_or_when_option_tsBuildInfoFile_is_specified,
            &[],
        );
    }

    fn is_ident(s: &str) -> bool {
        let mut chars = s.chars();
        match chars.next() {
            Some(c) if c == '$' || c == '_' || c.is_alphabetic() => {}
            _ => return false,
        }
        chars.all(|c| c == '$' || c == '_' || c.is_alphanumeric())
    }
    fn is_entity_name(s: &str) -> bool {
        !s.is_empty() && s.split('.').all(is_ident)
    }
    fn one_asterisk_max(s: &str) -> bool {
        s.bytes().filter(|&b| b == b'*').count() <= 1
    }
    fn path_is_relative(s: &str) -> bool {
        s == "." || s == ".." || s.starts_with("./") || s.starts_with("../")
    }
    fn path_is_absolute(s: &str) -> bool {
        s.starts_with('/')
            || s.starts_with('\\')
            || (s.len() >= 3
                && s.as_bytes()[0].is_ascii_alphabetic()
                && s.as_bytes()[1] == b':'
                && matches!(s.as_bytes()[2], b'/' | b'\\'))
            || s.contains("://")
    }

    // paths pattern validation (verifyCompilerOptions)
    if let Some(paths) = &opts.paths {
        for (key, value) in paths {
            if !one_asterisk_max(key) {
                out.push(Diagnostic {
                    file: None,
                    start: 0,
                    length: 0,
                    message: MessageChain::new(
                        &gen::Pattern_0_can_have_at_most_one_Asterisk_character,
                        &[key.clone()],
                    ),
                    related: Vec::new(),
                });
            }
            match value {
                PathsValue::Array(elems) => {
                    if elems.is_empty() {
                        out.push(Diagnostic {
                            file: None,
                            start: 0,
                            length: 0,
                            message: MessageChain::new(
                                &gen::Substitutions_for_pattern_0_shouldn_t_be_an_empty_array,
                                &[key.clone()],
                            ),
                            related: Vec::new(),
                        });
                    }
                    for e in elems {
                        match e {
                            PathsElem::Str(s) => {
                                if !one_asterisk_max(s) {
                                    out.push(Diagnostic {
                                        file: None,
                                        start: 0,
                                        length: 0,
                                        message: MessageChain::new(
                                            &gen::Substitution_0_in_pattern_1_can_have_at_most_one_Asterisk_character,
                                            &[s.clone(), key.clone()],
                                        ),
                                    related: Vec::new(),
                                    });
                                }
                                if opts.base_url.is_none()
                                    && !path_is_relative(s)
                                    && !path_is_absolute(s)
                                {
                                    out.push(Diagnostic {
                                        file: None,
                                        start: 0,
                                        length: 0,
                                        message: MessageChain::new(
                                            &gen::Non_relative_paths_are_not_allowed_when_baseUrl_is_not_set_Did_you_forget_a_leading_Slash,
                                            &[],
                                        ),
                                    related: Vec::new(),
                                    });
                                }
                            }
                            PathsElem::Other { display, type_name } => {
                                out.push(Diagnostic {
                                    file: None,
                                    start: 0,
                                    length: 0,
                                    message: MessageChain::new(
                                        &gen::Substitution_0_for_pattern_1_has_incorrect_type_expected_string_got_2,
                                        &[display.clone(), key.clone(), type_name.to_string()],
                                    ),
                                related: Vec::new(),
                                });
                            }
                        }
                    }
                }
                PathsValue::NotArray => {
                    out.push(Diagnostic {
                        file: None,
                        start: 0,
                        length: 0,
                        message: MessageChain::new(
                            &gen::Substitutions_for_pattern_0_should_be_an_array,
                            &[key.clone()],
                        ),
                        related: Vec::new(),
                    });
                }
            }
        }
    }

    let mut err = |msg: &'static DiagnosticMessage, args: &[&str]| {
        out.push(Diagnostic {
            file: None,
            start: 0,
            length: 0,
            message: MessageChain::new(
                msg,
                &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
            ),
            related: Vec::new(),
        });
    };

    // isolatedModules / verbatimModuleSyntax group
    if opts.isolated_modules || opts.verbatim_module_syntax {
        if opts.module.as_deref() == Some("none")
            && opts.script_target_rank() < 2
            && opts.isolated_modules
        {
            err(
                &gen::Option_isolatedModules_can_only_be_used_when_either_option_module_is_provided_or_option_target_is_ES2015_or_higher,
                &[],
            );
        }
        if opts.preserve_const_enums == Some(false) {
            err(
                &gen::Option_preserveConstEnums_cannot_be_disabled_when_0_is_enabled,
                &[if opts.verbatim_module_syntax {
                    "verbatimModuleSyntax"
                } else {
                    "isolatedModules"
                }],
            );
        }
    }

    // jsxFactory / jsxFragmentFactory / reactNamespace / jsxImportSource
    let jsx = opts.jsx.as_deref();
    let jsx_transform = matches!(jsx, Some("react-jsx") | Some("react-jsxdev"));
    let jsx_display = jsx.unwrap_or("");
    if let Some(factory) = opts.jsx_factory.as_deref().filter(|s| !s.is_empty()) {
        if opts
            .react_namespace
            .as_deref()
            .is_some_and(|s| !s.is_empty())
        {
            err(
                &gen::Option_0_cannot_be_specified_with_option_1,
                &["reactNamespace", "jsxFactory"],
            );
        }
        if jsx_transform {
            err(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxFactory", jsx_display],
            );
        }
        if !is_entity_name(factory) {
            err(
                &gen::Invalid_value_for_jsxFactory_0_is_not_a_valid_identifier_or_qualified_name,
                &[factory],
            );
        }
    } else if let Some(ns) = opts.react_namespace.as_deref().filter(|s| !s.is_empty()) {
        if !is_ident(ns) {
            err(
                &gen::Invalid_value_for_reactNamespace_0_is_not_a_valid_identifier,
                &[ns],
            );
        }
    }
    if let Some(frag) = opts
        .jsx_fragment_factory
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        if opts
            .jsx_factory
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_none()
        {
            err(
                &gen::Option_0_cannot_be_specified_without_specifying_option_1,
                &["jsxFragmentFactory", "jsxFactory"],
            );
        }
        if jsx_transform {
            err(
                &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
                &["jsxFragmentFactory", jsx_display],
            );
        }
        if !is_entity_name(frag) {
            err(
                &gen::Invalid_value_for_jsxFragmentFactory_0_is_not_a_valid_identifier_or_qualified_name,
                &[frag],
            );
        }
    }
    if opts
        .react_namespace
        .as_deref()
        .is_some_and(|s| !s.is_empty())
        && jsx_transform
    {
        err(
            &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
            &["reactNamespace", jsx_display],
        );
    }
    if opts
        .jsx_import_source
        .as_deref()
        .is_some_and(|s| !s.is_empty())
        && jsx == Some("react")
    {
        err(
            &gen::Option_0_cannot_be_specified_when_option_jsx_is_1,
            &["jsxImportSource", "react"],
        );
    }

    // verbatimModuleSyntax vs script-emitting module kinds
    if opts.verbatim_module_syntax && matches!(opts.module_kind(), "amd" | "umd" | "system") {
        err(
            &gen::Option_verbatimModuleSyntax_cannot_be_used_when_module_is_set_to_UMD_AMD_or_System,
            &[],
        );
    }

    // moduleResolution bundler requires an ES-module-shaped `module`
    if opts.module_resolution_kind() == "bundler" {
        let mk = opts.module_kind();
        let allowed = matches!(mk, "es2015" | "es6" | "es2020" | "es2022" | "esnext")
            || mk == "preserve"
            || mk == "commonjs";
        if !allowed {
            err(
                &gen::Option_0_can_only_be_used_when_module_is_set_to_preserve_commonjs_or_es2015_or_later,
                &["bundler"],
            );
        }
    }

    // verifyDeprecatedCompilerOptions: the 5.0→5.5 batch is past removal in
    // TS 6.0 (5102/5108, never silenceable); the 6.0→7.0 batch reports
    // 5101/5107 only when ignoreDeprecations is below 6.0 ("5.0" or invalid).
    let mut chain_err =
        |msg: &'static DiagnosticMessage,
         args: &[&str],
         child: Option<(&'static DiagnosticMessage, &[&str])>| {
            let mut m =
                MessageChain::new(msg, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            if let Some((cm, cargs)) = child {
                m.next.push(MessageChain::new(
                    cm,
                    &cargs.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                ));
            }
            out.push(Diagnostic {
                file: None,
                start: 0,
                length: 0,
                message: m,
                related: Vec::new(),
            });
        };
    let dep60_active = match opts.ignore_deprecations.as_deref() {
        None | Some("6.0") => false,
        Some("5.0") => true,
        Some(_) => {
            chain_err(&gen::Invalid_value_for_ignoreDeprecations, &[], None);
            true // Version.zero: everything unsilenced
        }
    };
    // removed (deprecated 5.0, removed 5.5)
    if opts.target.as_deref() == Some("es3") {
        chain_err(
            &gen::Option_0_1_has_been_removed_Please_remove_it_from_your_configuration,
            &["target", "ES3"],
            None,
        );
    }
    let removed = &gen::Option_0_has_been_removed_Please_remove_it_from_your_configuration;
    if opts.no_implicit_use_strict {
        chain_err(removed, &["noImplicitUseStrict"], None);
    }
    if opts.keyof_strings_only {
        chain_err(removed, &["keyofStringsOnly"], None);
    }
    if opts.suppress_excess_property_errors {
        chain_err(removed, &["suppressExcessPropertyErrors"], None);
    }
    if opts.suppress_implicit_any_index_errors {
        chain_err(removed, &["suppressImplicitAnyIndexErrors"], None);
    }
    if opts.no_strict_generic_checks {
        chain_err(removed, &["noStrictGenericChecks"], None);
    }
    if opts.charset.as_deref().is_some_and(|s| !s.is_empty()) {
        chain_err(removed, &["charset"], None);
    }
    if opts.out.as_deref().is_some_and(|s| !s.is_empty()) {
        chain_err(removed, &["out"], None);
    }
    if opts
        .imports_not_used_as_values
        .as_deref()
        .is_some_and(|v| v != "remove")
    {
        chain_err(
            removed,
            &["importsNotUsedAsValues"],
            Some((&gen::Use_0_instead, &["verbatimModuleSyntax"])),
        );
    }
    if opts.preserve_value_imports {
        chain_err(
            removed,
            &["preserveValueImports"],
            Some((&gen::Use_0_instead, &["verbatimModuleSyntax"])),
        );
    }
    // deprecated (6.0 → stop functioning in 7.0)
    if dep60_active {
        let dep_name = &gen::Option_0_is_deprecated_and_will_stop_functioning_in_TypeScript_1_Specify_compilerOption_ignoreDeprecations_Colon_2_to_silence_this_error;
        let dep_value = &gen::Option_0_1_is_deprecated_and_will_stop_functioning_in_TypeScript_2_Specify_compilerOption_ignoreDeprecations_Colon_3_to_silence_this_error;
        let visit: Option<(&'static DiagnosticMessage, &[&str])> = Some((
            &gen::Visit_https_Colon_Slash_Slashaka_ms_Slashts6_for_migration_information,
            &[],
        ));
        if opts.always_strict == Some(false) {
            chain_err(dep_value, &["alwaysStrict", "false", "7.0", "6.0"], None);
        }
        if opts.target.as_deref().unwrap_or("es5") == "es5" {
            chain_err(dep_value, &["target", "ES5", "7.0", "6.0"], None);
        }
        match opts.module_resolution.as_deref() {
            Some("node10") | Some("node") => {
                chain_err(
                    dep_value,
                    &["moduleResolution", "node10", "7.0", "6.0"],
                    visit,
                );
            }
            Some("classic") => {
                chain_err(
                    dep_value,
                    &["moduleResolution", "classic", "7.0", "6.0"],
                    None,
                );
            }
            _ => {}
        }
        if opts.base_url.is_some() {
            chain_err(dep_name, &["baseUrl", "7.0", "6.0"], visit);
        }
        if opts.es_module_interop == Some(false) {
            chain_err(dep_value, &["esModuleInterop", "false", "7.0", "6.0"], None);
        }
        if opts.allow_synthetic_default_imports == Some(false) {
            chain_err(
                dep_value,
                &["allowSyntheticDefaultImports", "false", "7.0", "6.0"],
                None,
            );
        }
        if opts.out_file.as_deref().is_some_and(|s| !s.is_empty()) {
            chain_err(dep_name, &["outFile", "7.0", "6.0"], None);
        }
        if let Some(m) = opts.module.as_deref() {
            let display = match m {
                "none" => Some("None"),
                "amd" => Some("AMD"),
                "umd" => Some("UMD"),
                "system" => Some("System"),
                _ => None,
            };
            if let Some(d) = display {
                chain_err(dep_value, &["module", d, "7.0", "6.0"], None);
            }
        }
        if opts.downlevel_iteration.is_some() {
            chain_err(dep_name, &["downlevelIteration", "7.0", "6.0"], None);
        }
    }
    out
}
