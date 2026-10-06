//! The compiler options recorded in the build info (tsgo
//! `toBuildInfo.setCompilerOptions`): every option whose declaration has
//! `AffectsBuildInfo`, in the order of tsgo's `core.CompilerOptions`
//! struct, with the unset ones omitted and the file-path ones relative to
//! the build info directory.

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

struct Serialized<'r> {
    out: Vec<(&'static str, OptionValue)>,
    relative: &'r dyn Fn(&str) -> String,
}

impl Serialized<'_> {
    fn tristate(&mut self, name: &'static str, value: Option<bool>) {
        if let Some(value) = value {
            self.out.push((name, OptionValue::Bool(value)));
        }
    }

    fn number(&mut self, name: &'static str, value: Option<i64>) {
        if let Some(value) = value.filter(|value| *value != 0) {
            self.out.push((name, OptionValue::Number(value)));
        }
    }

    fn string(&mut self, name: &'static str, value: &Option<JsString>) {
        if let Some(value) = value.as_ref().filter(|value| !value.is_empty()) {
            self.out.push((
                name,
                OptionValue::String(value.to_string_lossy().into_owned()),
            ));
        }
    }

    fn path(&mut self, name: &'static str, value: &Option<JsString>) {
        if let Some(value) = value.as_ref().filter(|value| !value.is_empty()) {
            let relative = (self.relative)(&value.to_string_lossy());
            self.out.push((name, OptionValue::String(relative)));
        }
    }
}

/// The serialized options. `relative` makes a file-path option relative to
/// the build info directory.
pub fn build_info_options(
    options: &CompilerOptions,
    relative: &dyn Fn(&str) -> String,
) -> Vec<(&'static str, OptionValue)> {
    let mut s = Serialized {
        out: Vec::new(),
        relative,
    };
    s.tristate("allowJs", options.allow_js_specified);
    s.tristate(
        "allowImportingTsExtensions",
        options.allow_importing_ts_extensions,
    );
    s.tristate("allowUmdGlobalAccess", options.allow_umd_global_access);
    s.tristate("allowUnreachableCode", options.allow_unreachable_code);
    s.tristate("allowUnusedLabels", options.allow_unused_labels);
    s.tristate(
        "assumeChangesOnlyAffectDirectDependencies",
        options.assume_changes_only_affect_direct_dependencies,
    );
    s.tristate("checkJs", options.check_js);
    s.tristate("composite", options.composite);
    s.tristate("emitDeclarationOnly", options.emit_declaration_only);
    s.tristate("emitBOM", options.emit_bom);
    s.tristate("emitDecoratorMetadata", options.emit_decorator_metadata);
    s.tristate("declaration", options.declaration);
    s.path("declarationDir", &options.declaration_dir);
    s.tristate("declarationMap", options.declaration_map);
    s.tristate("erasableSyntaxOnly", options.erasable_syntax_only);
    s.tristate(
        "exactOptionalPropertyTypes",
        options.exact_optional_property_types,
    );
    s.tristate(
        "experimentalDecorators",
        options.experimental_decorators_specified,
    );
    s.tristate("isolatedDeclarations", options.isolated_declarations);
    s.tristate("importHelpers", options.import_helpers);
    s.tristate("inlineSourceMap", options.inline_source_map);
    s.tristate("inlineSources", options.inline_sources);
    s.number("jsx", options.jsx.map(i64::from));
    s.string("jsxImportSource", &options.jsx_import_source);
    s.string("mapRoot", &options.map_root);
    s.number("module", options.module.map(i64::from));
    // The port's newLine numbering is TypeScript's (CRLF 0, LF 1); tsgo's
    // is None 0, CRLF 1, LF 2.
    s.number(
        "newLine",
        options.new_line.map(|value| i64::from(value) + 1),
    );
    s.tristate("noErrorTruncation", options.no_error_truncation);
    s.tristate(
        "noFallthroughCasesInSwitch",
        options.no_fallthrough_cases_in_switch,
    );
    s.tristate("noImplicitAny", options.no_implicit_any);
    s.tristate("noImplicitThis", options.no_implicit_this);
    s.tristate("noImplicitReturns", options.no_implicit_returns);
    s.tristate("noEmitHelpers", options.no_emit_helpers);
    s.tristate(
        "noPropertyAccessFromIndexSignature",
        options.no_property_access_from_index_signature,
    );
    s.tristate(
        "noUncheckedIndexedAccess",
        options.no_unchecked_indexed_access,
    );
    s.tristate("noEmitOnError", options.no_emit_on_error);
    s.tristate("noUnusedLocals", options.no_unused_locals);
    s.tristate("noUnusedParameters", options.no_unused_parameters);
    s.tristate("noImplicitOverride", options.no_implicit_override);
    s.tristate(
        "noUncheckedSideEffectImports",
        options.no_unchecked_side_effect_imports,
    );
    s.path("outDir", &options.out_dir);
    s.tristate("preserveConstEnums", options.preserve_const_enums);
    s.tristate("removeComments", options.remove_comments);
    s.tristate(
        "rewriteRelativeImportExtensions",
        options.rewrite_relative_import_extensions,
    );
    s.string("reactNamespace", &options.react_namespace);
    s.path("rootDir", &options.root_dir);
    s.tristate("skipLibCheck", options.skip_lib_check);
    s.tristate("stableTypeOrdering", options.stable_type_ordering);
    s.tristate("strict", options.strict);
    s.tristate("strictBindCallApply", options.strict_bind_call_apply);
    s.tristate(
        "strictBuiltinIteratorReturn",
        options.strict_builtin_iterator_return,
    );
    s.tristate("strictFunctionTypes", options.strict_function_types);
    s.tristate("strictNullChecks", options.strict_null_checks);
    s.tristate(
        "strictPropertyInitialization",
        options.strict_property_initialization,
    );
    s.tristate("stripInternal", options.strip_internal);
    s.tristate("skipDefaultLibCheck", options.skip_default_lib_check);
    s.tristate("sourceMap", options.source_map);
    s.string("sourceRoot", &options.source_root);
    s.number("target", options.target.map(i64::from));
    s.path("tsBuildInfoFile", &options.ts_build_info_file);
    s.tristate(
        "useDefineForClassFields",
        options.use_define_for_class_fields,
    );
    s.tristate(
        "useUnknownInCatchVariables",
        options.use_unknown_in_catch_variables,
    );
    s.tristate("verbatimModuleSyntax", options.verbatim_module_syntax);
    s.tristate(
        "allowSyntheticDefaultImports",
        options.allow_synthetic_default_imports,
    );
    s.tristate("alwaysStrict", options.always_strict);
    s.tristate("downlevelIteration", options.downlevel_iteration);
    s.tristate("esModuleInterop", options.es_module_interop);
    s.path("outFile", &options.out_file);
    s.out
}
