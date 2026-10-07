//! The compiler options recorded in the build info (tsgo
//! `toBuildInfo.setCompilerOptions`): every option whose declaration has
//! `AffectsBuildInfo`, in the order of tsgo's `core.CompilerOptions`
//! struct, with the unset ones omitted and the file-path ones relative to
//! the build info directory; their parsing back (`GetCompilerOptions`) and
//! the option-change predicates of `tsoptions/declscompiler.go`
//! (`CompilerOptionsAffectSemanticDiagnostics`, `…AffectEmit`,
//! `…AffectDeclarationPath`), which compare the options flagged
//! `AffectsSemanticDiagnostics`, `AffectsEmit` and `AffectsDeclarationPath`
//! — every one of them is also recorded.

use tsc_types::{CompilerOptions, JsString};

use crate::build_info::OptionValue;

/// tsgo `core.CompilerOptions.IsIncremental`.
pub fn is_incremental(options: &CompilerOptions) -> bool {
    options.incremental == Some(true) || options.composite == Some(true)
}

/// tsgo `core.CompilerOptions.GetEmitDeclarations`.
pub fn emit_declarations(options: &CompilerOptions) -> bool {
    options.declaration == Some(true) || options.composite == Some(true)
}

/// How one recorded option is read from the options struct (tsgo's
/// `ForEachCompilerOptionValue` over the struct fields, with the
/// `strictFlag`/`allowJsFlag` comparisons of `optionsHaveChanges`).
#[derive(Clone, Copy)]
enum Field {
    Tristate(fn(&CompilerOptions) -> Option<bool>),
    /// A strict-family flag: compared through `GetStrictOptionValue`.
    Strict(fn(&CompilerOptions) -> Option<bool>),
    /// `allowJs`: compared through `GetAllowJS`.
    AllowJs,
    Number(fn(&CompilerOptions) -> Option<i64>),
    Text(fn(&CompilerOptions) -> Option<&JsString>),
    Path(fn(&CompilerOptions) -> Option<&JsString>),
}

struct Recorded {
    name: &'static str,
    semantic: bool,
    emit: bool,
    declaration_path: bool,
    field: Field,
}

const fn recorded(name: &'static str, flags: &'static str, field: Field) -> Recorded {
    let bytes = flags.as_bytes();
    let mut semantic = false;
    let mut emit = false;
    let mut declaration_path = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'S' => semantic = true,
            b'E' => emit = true,
            b'D' => declaration_path = true,
            _ => {}
        }
        index += 1;
    }
    Recorded {
        name,
        semantic,
        emit,
        declaration_path,
        field,
    }
}

/// The recorded options in tsgo's struct order, each with the flags of its
/// declaration (`S` AffectsSemanticDiagnostics, `E` AffectsEmit,
/// `D` AffectsDeclarationPath).
const RECORDED: &[Recorded] = &[
    recorded("allowJs", "", Field::AllowJs),
    recorded(
        "allowImportingTsExtensions",
        "S",
        Field::Tristate(|options| options.allow_importing_ts_extensions),
    ),
    recorded(
        "allowUmdGlobalAccess",
        "S",
        Field::Tristate(|options| options.allow_umd_global_access),
    ),
    recorded(
        "allowUnreachableCode",
        "S",
        Field::Tristate(|options| options.allow_unreachable_code),
    ),
    recorded(
        "allowUnusedLabels",
        "S",
        Field::Tristate(|options| options.allow_unused_labels),
    ),
    recorded(
        "assumeChangesOnlyAffectDirectDependencies",
        "SE",
        Field::Tristate(|options| options.assume_changes_only_affect_direct_dependencies),
    ),
    recorded("checkJs", "S", Field::Tristate(|options| options.check_js)),
    recorded(
        "composite",
        "",
        Field::Tristate(|options| options.composite),
    ),
    recorded(
        "emitDeclarationOnly",
        "",
        Field::Tristate(|options| options.emit_declaration_only),
    ),
    recorded("emitBOM", "E", Field::Tristate(|options| options.emit_bom)),
    recorded(
        "emitDecoratorMetadata",
        "SE",
        Field::Tristate(|options| options.emit_decorator_metadata),
    ),
    recorded(
        "declaration",
        "",
        Field::Tristate(|options| options.declaration),
    ),
    recorded(
        "declarationDir",
        "ED",
        Field::Path(|options| options.declaration_dir.as_ref()),
    ),
    recorded(
        "declarationMap",
        "",
        Field::Tristate(|options| options.declaration_map),
    ),
    recorded(
        "erasableSyntaxOnly",
        "S",
        Field::Tristate(|options| options.erasable_syntax_only),
    ),
    recorded(
        "exactOptionalPropertyTypes",
        "S",
        Field::Tristate(|options| options.exact_optional_property_types),
    ),
    recorded(
        "experimentalDecorators",
        "SE",
        Field::Tristate(|options| options.experimental_decorators_specified),
    ),
    recorded(
        "isolatedDeclarations",
        "S",
        Field::Tristate(|options| options.isolated_declarations),
    ),
    recorded(
        "importHelpers",
        "E",
        Field::Tristate(|options| options.import_helpers),
    ),
    recorded(
        "inlineSourceMap",
        "",
        Field::Tristate(|options| options.inline_source_map),
    ),
    recorded(
        "inlineSources",
        "E",
        Field::Tristate(|options| options.inline_sources),
    ),
    recorded(
        "jsx",
        "SE",
        Field::Number(|options| options.jsx.map(i64::from)),
    ),
    recorded(
        "jsxImportSource",
        "SE",
        Field::Text(|options| options.jsx_import_source.as_ref()),
    ),
    recorded(
        "mapRoot",
        "E",
        Field::Text(|options| options.map_root.as_ref()),
    ),
    recorded(
        "module",
        "E",
        Field::Number(|options| options.module.map(i64::from)),
    ),
    // The port's newLine numbering is TypeScript's (CRLF 0, LF 1); tsgo's
    // is None 0, CRLF 1, LF 2.
    recorded(
        "newLine",
        "E",
        Field::Number(|options| options.new_line.map(|value| i64::from(value) + 1)),
    ),
    recorded(
        "noErrorTruncation",
        "S",
        Field::Tristate(|options| options.no_error_truncation),
    ),
    recorded(
        "noFallthroughCasesInSwitch",
        "S",
        Field::Tristate(|options| options.no_fallthrough_cases_in_switch),
    ),
    recorded(
        "noImplicitAny",
        "S",
        Field::Strict(|options| options.no_implicit_any),
    ),
    recorded(
        "noImplicitThis",
        "S",
        Field::Strict(|options| options.no_implicit_this),
    ),
    recorded(
        "noImplicitReturns",
        "S",
        Field::Tristate(|options| options.no_implicit_returns),
    ),
    recorded(
        "noEmitHelpers",
        "E",
        Field::Tristate(|options| options.no_emit_helpers),
    ),
    recorded(
        "noPropertyAccessFromIndexSignature",
        "S",
        Field::Tristate(|options| options.no_property_access_from_index_signature),
    ),
    recorded(
        "noUncheckedIndexedAccess",
        "S",
        Field::Tristate(|options| options.no_unchecked_indexed_access),
    ),
    recorded(
        "noEmitOnError",
        "E",
        Field::Tristate(|options| options.no_emit_on_error),
    ),
    recorded(
        "noUnusedLocals",
        "S",
        Field::Tristate(|options| options.no_unused_locals),
    ),
    recorded(
        "noUnusedParameters",
        "S",
        Field::Tristate(|options| options.no_unused_parameters),
    ),
    recorded(
        "noImplicitOverride",
        "S",
        Field::Tristate(|options| options.no_implicit_override),
    ),
    recorded(
        "noUncheckedSideEffectImports",
        "S",
        Field::Tristate(|options| options.no_unchecked_side_effect_imports),
    ),
    recorded(
        "outDir",
        "ED",
        Field::Path(|options| options.out_dir.as_ref()),
    ),
    recorded(
        "preserveConstEnums",
        "E",
        Field::Tristate(|options| options.preserve_const_enums),
    ),
    recorded(
        "removeComments",
        "E",
        Field::Tristate(|options| options.remove_comments),
    ),
    recorded(
        "rewriteRelativeImportExtensions",
        "S",
        Field::Tristate(|options| options.rewrite_relative_import_extensions),
    ),
    recorded(
        "reactNamespace",
        "E",
        Field::Text(|options| options.react_namespace.as_ref()),
    ),
    recorded(
        "rootDir",
        "ED",
        Field::Path(|options| options.root_dir.as_ref()),
    ),
    recorded(
        "skipLibCheck",
        "",
        Field::Tristate(|options| options.skip_lib_check),
    ),
    recorded(
        "stableTypeOrdering",
        "S",
        Field::Tristate(|options| options.stable_type_ordering),
    ),
    recorded("strict", "", Field::Tristate(|options| options.strict)),
    recorded(
        "strictBindCallApply",
        "S",
        Field::Strict(|options| options.strict_bind_call_apply),
    ),
    recorded(
        "strictBuiltinIteratorReturn",
        "S",
        Field::Strict(|options| options.strict_builtin_iterator_return),
    ),
    recorded(
        "strictFunctionTypes",
        "S",
        Field::Strict(|options| options.strict_function_types),
    ),
    recorded(
        "strictNullChecks",
        "S",
        Field::Strict(|options| options.strict_null_checks),
    ),
    recorded(
        "strictPropertyInitialization",
        "S",
        Field::Strict(|options| options.strict_property_initialization),
    ),
    recorded(
        "stripInternal",
        "E",
        Field::Tristate(|options| options.strip_internal),
    ),
    recorded(
        "skipDefaultLibCheck",
        "",
        Field::Tristate(|options| options.skip_default_lib_check),
    ),
    recorded(
        "sourceMap",
        "",
        Field::Tristate(|options| options.source_map),
    ),
    recorded(
        "sourceRoot",
        "E",
        Field::Text(|options| options.source_root.as_ref()),
    ),
    recorded(
        "target",
        "E",
        Field::Number(|options| options.target.map(i64::from)),
    ),
    recorded(
        "tsBuildInfoFile",
        "E",
        Field::Path(|options| options.ts_build_info_file.as_ref()),
    ),
    recorded(
        "useDefineForClassFields",
        "SE",
        Field::Tristate(|options| options.use_define_for_class_fields),
    ),
    recorded(
        "useUnknownInCatchVariables",
        "S",
        Field::Strict(|options| options.use_unknown_in_catch_variables),
    ),
    recorded(
        "verbatimModuleSyntax",
        "SE",
        Field::Tristate(|options| options.verbatim_module_syntax),
    ),
    recorded(
        "allowSyntheticDefaultImports",
        "S",
        Field::Tristate(|options| options.allow_synthetic_default_imports),
    ),
    recorded(
        "alwaysStrict",
        "E",
        Field::Tristate(|options| options.always_strict),
    ),
    recorded(
        "downlevelIteration",
        "E",
        Field::Tristate(|options| options.downlevel_iteration),
    ),
    recorded(
        "esModuleInterop",
        "SE",
        Field::Tristate(|options| options.es_module_interop),
    ),
    recorded(
        "outFile",
        "ED",
        Field::Path(|options| options.out_file.as_ref()),
    ),
];

/// The static name of a recorded option, when `name` is one.
pub fn known_option_name(name: &str) -> Option<&'static str> {
    RECORDED
        .iter()
        .find(|option| option.name == name)
        .map(|option| option.name)
}

/// The serialized options. `relative` makes a file-path option relative to
/// the build info directory.
pub fn build_info_options(
    options: &CompilerOptions,
    relative: &dyn Fn(&str) -> String,
) -> Vec<(&'static str, OptionValue)> {
    let mut out = Vec::new();
    for option in RECORDED {
        let value = match option.field {
            Field::AllowJs => options.allow_js_specified.map(OptionValue::Bool),
            Field::Tristate(get) | Field::Strict(get) => get(options).map(OptionValue::Bool),
            Field::Number(get) => get(options)
                .filter(|value| *value != 0)
                .map(OptionValue::Number),
            Field::Text(get) => get(options)
                .filter(|value| !value.is_empty())
                .map(|value| OptionValue::String(value.to_string_lossy().into_owned())),
            Field::Path(get) => get(options)
                .filter(|value| !value.is_empty())
                .map(|value| OptionValue::String(relative(&value.to_string_lossy()))),
        };
        if let Some(value) = value {
            out.push((option.name, value));
        }
    }
    out
}

/// tsgo `BuildInfo.GetCompilerOptions`: the recorded options parsed back
/// (`ParseCompilerOptions`), a file path made absolute against the build
/// info directory by `absolute` (`ConvertOptionToAbsolutePath`). A value of
/// the wrong kind is read as tsgo's parsers read it (a non-boolean is
/// `false`, a non-string is empty, a non-number is zero).
pub fn parse_build_info_options(
    entries: &[(&'static str, OptionValue)],
    absolute: &dyn Fn(&str) -> String,
) -> CompilerOptions {
    let mut options = CompilerOptions::default();
    let tristate = |value: &OptionValue| Some(matches!(value, OptionValue::Bool(true)));
    let number = |value: &OptionValue| match value {
        OptionValue::Number(value) => i32::try_from(*value).ok(),
        _ => Some(0),
    };
    let text = |value: &OptionValue| match value {
        OptionValue::String(value) => Some(JsString::from(value.as_str())),
        _ => Some(JsString::from("")),
    };
    let path = |value: &OptionValue| match value {
        OptionValue::String(value) => Some(JsString::from(absolute(value).as_str())),
        _ => Some(JsString::from("")),
    };
    for (name, value) in entries {
        match *name {
            "allowJs" => options.allow_js_specified = tristate(value),
            "allowImportingTsExtensions" => options.allow_importing_ts_extensions = tristate(value),
            "allowUmdGlobalAccess" => options.allow_umd_global_access = tristate(value),
            "allowUnreachableCode" => options.allow_unreachable_code = tristate(value),
            "allowUnusedLabels" => options.allow_unused_labels = tristate(value),
            "assumeChangesOnlyAffectDirectDependencies" => {
                options.assume_changes_only_affect_direct_dependencies = tristate(value)
            }
            "checkJs" => options.check_js = tristate(value),
            "composite" => options.composite = tristate(value),
            "emitDeclarationOnly" => options.emit_declaration_only = tristate(value),
            "emitBOM" => options.emit_bom = tristate(value),
            "emitDecoratorMetadata" => options.emit_decorator_metadata = tristate(value),
            "declaration" => options.declaration = tristate(value),
            "declarationDir" => options.declaration_dir = path(value),
            "declarationMap" => options.declaration_map = tristate(value),
            "erasableSyntaxOnly" => options.erasable_syntax_only = tristate(value),
            "exactOptionalPropertyTypes" => options.exact_optional_property_types = tristate(value),
            "experimentalDecorators" => options.experimental_decorators_specified = tristate(value),
            "isolatedDeclarations" => options.isolated_declarations = tristate(value),
            "importHelpers" => options.import_helpers = tristate(value),
            "inlineSourceMap" => options.inline_source_map = tristate(value),
            "inlineSources" => options.inline_sources = tristate(value),
            "jsx" => options.jsx = number(value),
            "jsxImportSource" => options.jsx_import_source = text(value),
            "mapRoot" => options.map_root = text(value),
            "module" => options.module = number(value),
            "newLine" => options.new_line = number(value).map(|value| value - 1),
            "noErrorTruncation" => options.no_error_truncation = tristate(value),
            "noFallthroughCasesInSwitch" => {
                options.no_fallthrough_cases_in_switch = tristate(value)
            }
            "noImplicitAny" => options.no_implicit_any = tristate(value),
            "noImplicitThis" => options.no_implicit_this = tristate(value),
            "noImplicitReturns" => options.no_implicit_returns = tristate(value),
            "noEmitHelpers" => options.no_emit_helpers = tristate(value),
            "noPropertyAccessFromIndexSignature" => {
                options.no_property_access_from_index_signature = tristate(value)
            }
            "noUncheckedIndexedAccess" => options.no_unchecked_indexed_access = tristate(value),
            "noEmitOnError" => options.no_emit_on_error = tristate(value),
            "noUnusedLocals" => options.no_unused_locals = tristate(value),
            "noUnusedParameters" => options.no_unused_parameters = tristate(value),
            "noImplicitOverride" => options.no_implicit_override = tristate(value),
            "noUncheckedSideEffectImports" => {
                options.no_unchecked_side_effect_imports = tristate(value)
            }
            "outDir" => options.out_dir = path(value),
            "preserveConstEnums" => options.preserve_const_enums = tristate(value),
            "removeComments" => options.remove_comments = tristate(value),
            "rewriteRelativeImportExtensions" => {
                options.rewrite_relative_import_extensions = tristate(value)
            }
            "reactNamespace" => options.react_namespace = text(value),
            "rootDir" => options.root_dir = path(value),
            "skipLibCheck" => options.skip_lib_check = tristate(value),
            "stableTypeOrdering" => options.stable_type_ordering = tristate(value),
            "strict" => options.strict = tristate(value),
            "strictBindCallApply" => options.strict_bind_call_apply = tristate(value),
            "strictBuiltinIteratorReturn" => {
                options.strict_builtin_iterator_return = tristate(value)
            }
            "strictFunctionTypes" => options.strict_function_types = tristate(value),
            "strictNullChecks" => options.strict_null_checks = tristate(value),
            "strictPropertyInitialization" => {
                options.strict_property_initialization = tristate(value)
            }
            "stripInternal" => options.strip_internal = tristate(value),
            "skipDefaultLibCheck" => options.skip_default_lib_check = tristate(value),
            "sourceMap" => options.source_map = tristate(value),
            "sourceRoot" => options.source_root = text(value),
            "target" => options.target = number(value),
            "tsBuildInfoFile" => options.ts_build_info_file = path(value),
            "useDefineForClassFields" => options.use_define_for_class_fields = tristate(value),
            "useUnknownInCatchVariables" => {
                options.use_unknown_in_catch_variables = tristate(value)
            }
            "verbatimModuleSyntax" => options.verbatim_module_syntax = tristate(value),
            "allowSyntheticDefaultImports" => {
                options.allow_synthetic_default_imports = tristate(value)
            }
            "alwaysStrict" => options.always_strict = tristate(value),
            "downlevelIteration" => options.downlevel_iteration = tristate(value),
            "esModuleInterop" => options.es_module_interop = tristate(value),
            "outFile" => options.out_file = path(value),
            _ => {}
        }
    }
    // The derived flags (tsc getAllowJSCompilerOption, the decorators flag).
    options.allow_js = options
        .allow_js_specified
        .unwrap_or(options.check_js == Some(true));
    options.experimental_decorators = options.experimental_decorators_specified == Some(true);
    options
}

/// tsgo `core.CompilerOptions.GetAllowJS`.
fn allow_js(options: &CompilerOptions) -> bool {
    options
        .allow_js_specified
        .unwrap_or(options.check_js == Some(true))
}

/// tsgo `optionsHaveChanges` over the options `select`ed.
fn options_have_changes(
    old: &CompilerOptions,
    new: &CompilerOptions,
    select: impl Fn(&Recorded) -> bool,
) -> bool {
    RECORDED
        .iter()
        .filter(|option| select(option))
        .any(|option| match option.field {
            Field::AllowJs => allow_js(old) != allow_js(new),
            Field::Strict(get) => {
                old.strict_option_value(get(old)) != new.strict_option_value(get(new))
            }
            Field::Tristate(get) => get(old) != get(new),
            Field::Number(get) => get(old) != get(new),
            Field::Text(get) | Field::Path(get) => get(old) != get(new),
        })
}

/// tsgo `CompilerOptionsAffectSemanticDiagnostics`.
pub fn affects_semantic_diagnostics(old: &CompilerOptions, new: &CompilerOptions) -> bool {
    options_have_changes(old, new, |option| option.semantic)
}

/// tsgo `CompilerOptionsAffectEmit`.
pub fn affects_emit(old: &CompilerOptions, new: &CompilerOptions) -> bool {
    options_have_changes(old, new, |option| option.emit)
}

/// tsgo `CompilerOptionsAffectDeclarationPath`.
pub fn affects_declaration_path(old: &CompilerOptions, new: &CompilerOptions) -> bool {
    options_have_changes(old, new, |option| option.declaration_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> CompilerOptions {
        CompilerOptions {
            module: Some(99),
            target: Some(9),
            strict: Some(true),
            out_dir: Some("/work/dist".into()),
            new_line: Some(1),
            allow_js_specified: Some(true),
            allow_js: true,
            ..CompilerOptions::default()
        }
    }

    #[test]
    fn recorded_options_round_trip_through_the_build_info() {
        let relative = |path: &str| {
            path.strip_prefix("/work/dist").map_or_else(
                || path.to_owned(),
                |rest| format!("./{}", rest.trim_start_matches('/')),
            )
        };
        let entries = build_info_options(&options(), &relative);
        assert_eq!(
            entries,
            vec![
                ("allowJs", OptionValue::Bool(true)),
                ("module", OptionValue::Number(99)),
                ("newLine", OptionValue::Number(2)),
                ("outDir", OptionValue::String("./".into())),
                ("strict", OptionValue::Bool(true)),
                ("target", OptionValue::Number(9)),
            ]
        );
        let absolute = |path: &str| {
            if path == "./" {
                "/work/dist".to_owned()
            } else {
                format!("/work/dist/{path}")
            }
        };
        let parsed = parse_build_info_options(&entries, &absolute);
        assert_eq!(parsed.module, Some(99));
        assert_eq!(parsed.new_line, Some(1));
        assert_eq!(
            parsed
                .out_dir
                .as_ref()
                .map(|dir| dir.to_string_lossy().into_owned()),
            Some("/work/dist".to_owned())
        );
        assert!(parsed.allow_js);
        assert_eq!(parsed.allow_js_specified, Some(true));
        assert!(!affects_semantic_diagnostics(&parsed, &options()));
        assert!(!affects_emit(&parsed, &options()));
        assert!(!affects_declaration_path(&parsed, &options()));
    }

    #[test]
    fn the_change_predicates_follow_the_option_flags() {
        let old = options();
        let mut with_source_map = options();
        with_source_map.source_map = Some(true);
        // sourceMap carries no flag: neither predicate sees it.
        assert!(!affects_semantic_diagnostics(&old, &with_source_map));
        assert!(!affects_emit(&old, &with_source_map));
        let mut with_declaration_dir = options();
        with_declaration_dir.declaration_dir = Some("/work/types".into());
        assert!(affects_emit(&old, &with_declaration_dir));
        assert!(affects_declaration_path(&old, &with_declaration_dir));
        assert!(!affects_semantic_diagnostics(&old, &with_declaration_dir));
        // A strict-family flag compares its derived value: `strict` alone
        // already implies `strictNullChecks`.
        let mut explicit_null_checks = options();
        explicit_null_checks.strict_null_checks = Some(true);
        assert!(!affects_semantic_diagnostics(&old, &explicit_null_checks));
        let mut without_null_checks = options();
        without_null_checks.strict_null_checks = Some(false);
        assert!(affects_semantic_diagnostics(&old, &without_null_checks));
        let mut check_js = options();
        check_js.check_js = Some(true);
        assert!(affects_semantic_diagnostics(&old, &check_js));
        assert!(!affects_emit(&old, &check_js));
    }
}
