//! Explicit option parity for input verification before comparing emit results.
//! All CompilerOptions fields are covered by the exhaustive pattern below.
use super::{diagnostics, sha256, string_value};
use serde_json::{json, Map, Value};
use tsc_program::{LibraryCatalog, PreparedProgram};

pub(super) fn snapshot(program: &PreparedProgram) -> Value {
    let options = program.compiler_options();
    let tsc_types::CompilerOptions {
        allow_js: _,
        force_consistent_casing_in_file_names: _,
        max_node_module_js_depth: _,
        experimental_decorators: _,
        target: _,
        module: _,
        module_detection: _,
        always_strict: _,
        strict: _,
        strict_null_checks: _,
        strict_function_types: _,
        no_implicit_any: _,
        no_error_truncation: _,
        no_implicit_this: _,
        no_implicit_override: _,
        strict_bind_call_apply: _,
        exact_optional_property_types: _,
        no_fallthrough_cases_in_switch: _,
        no_implicit_returns: _,
        no_unused_locals: _,
        no_unused_parameters: _,
        allow_unreachable_code: _,
        allow_unused_labels: _,
        check_js: _,
        no_unchecked_indexed_access: _,
        no_property_access_from_index_signature: _,
        no_unchecked_side_effect_imports: _,
        strict_property_initialization: _,
        use_define_for_class_fields: _,
        use_unknown_in_catch_variables: _,
        lib: _,
        lib_replacement: _,
        jsx: _,
        no_emit: _,
        no_emit_for_js_files: _,
        allow_non_ts_extensions: _,
        list_emitted_files: _,
        emit_bom: _,
        no_emit_on_error: _,
        no_check: _,
        erasable_syntax_only: _,
        out_dir: _,
        root_dir: _,
        source_map: _,
        inline_source_map: _,
        inline_sources: _,
        source_root: _,
        map_root: _,
        declaration: _,
        declaration_map: _,
        emit_declaration_only: _,
        isolated_declarations: _,
        stable_type_ordering: _,
        reference_profile: _,
        declaration_dir: _,
        strip_internal: _,
        out_file: _,
        out: _,
        incremental: _,
        composite: _,
        assume_changes_only_affect_direct_dependencies: _,
        ts_build_info_file: _,
        imports_not_used_as_values: _,
        preserve_value_imports: _,
        keyof_strings_only: _,
        suppress_excess_property_errors: _,
        suppress_implicit_any_index_errors: _,
        no_strict_generic_checks: _,
        charset: _,
        emit_decorator_metadata: _,
        new_line: _,
        remove_comments: _,
        no_implicit_use_strict: _,
        no_emit_helpers: _,
        no_resolve: _,
        import_helpers: _,
        downlevel_iteration: _,
        strict_builtin_iterator_return: _,
        module_resolution: _,
        es_module_interop: _,
        allow_synthetic_default_imports: _,
        preserve_const_enums: _,
        isolated_modules: _,
        verbatim_module_syntax: _,
        allow_umd_global_access: _,
        base_url: _,
        module_suffixes: _,
        resolve_package_json_exports: _,
        resolve_package_json_imports: _,
        custom_conditions: _,
        no_dts_resolution: _,
        allow_arbitrary_extensions: _,
        allow_importing_ts_extensions: _,
        rewrite_relative_import_extensions: _,
        resolve_json_module: _,
        skip_lib_check: _,
        skip_default_lib_check: _,
        jsx_factory: _,
        jsx_fragment_factory: _,
        jsx_import_source: _,
        react_namespace: _,
        ignore_deprecations: _,
    } = options;
    let catalog = LibraryCatalog::typescript_6_0_3("");
    let mut result = Map::new();
    result.insert("allowJs".to_owned(), json!(options.allow_js));
    result.insert(
        "forceConsistentCasingInFileNames".to_owned(),
        json!(options.force_consistent_casing_in_file_names),
    );
    result.insert(
        "maxNodeModuleJsDepth".to_owned(),
        json!(options
            .max_node_module_js_depth
            .map(|number| number.value())),
    );
    result.insert(
        "experimentalDecorators".to_owned(),
        json!(options.experimental_decorators),
    );
    result.insert("target".to_owned(), json!(options.target));
    result.insert("module".to_owned(), json!(options.module));
    result.insert(
        "moduleDetection".to_owned(),
        json!(options.module_detection),
    );
    result.insert("alwaysStrict".to_owned(), json!(options.always_strict));
    result.insert("strict".to_owned(), json!(options.strict));
    result.insert(
        "strictNullChecks".to_owned(),
        json!(options.strict_null_checks),
    );
    result.insert(
        "strictFunctionTypes".to_owned(),
        json!(options.strict_function_types),
    );
    result.insert("noImplicitAny".to_owned(), json!(options.no_implicit_any));
    result.insert(
        "noErrorTruncation".to_owned(),
        json!(options.no_error_truncation),
    );
    result.insert("noImplicitThis".to_owned(), json!(options.no_implicit_this));
    result.insert(
        "noImplicitOverride".to_owned(),
        json!(options.no_implicit_override),
    );
    result.insert(
        "strictBindCallApply".to_owned(),
        json!(options.strict_bind_call_apply),
    );
    result.insert(
        "exactOptionalPropertyTypes".to_owned(),
        json!(options.exact_optional_property_types),
    );
    result.insert(
        "noFallthroughCasesInSwitch".to_owned(),
        json!(options.no_fallthrough_cases_in_switch),
    );
    result.insert(
        "noImplicitReturns".to_owned(),
        json!(options.no_implicit_returns),
    );
    result.insert("noUnusedLocals".to_owned(), json!(options.no_unused_locals));
    result.insert(
        "noUnusedParameters".to_owned(),
        json!(options.no_unused_parameters),
    );
    result.insert(
        "allowUnreachableCode".to_owned(),
        json!(options.allow_unreachable_code),
    );
    result.insert(
        "allowUnusedLabels".to_owned(),
        json!(options.allow_unused_labels),
    );
    result.insert("checkJs".to_owned(), json!(options.check_js));
    result.insert(
        "noUncheckedIndexedAccess".to_owned(),
        json!(options.no_unchecked_indexed_access),
    );
    result.insert(
        "noPropertyAccessFromIndexSignature".to_owned(),
        json!(options.no_property_access_from_index_signature),
    );
    result.insert(
        "noUncheckedSideEffectImports".to_owned(),
        json!(options.no_unchecked_side_effect_imports),
    );
    result.insert(
        "strictPropertyInitialization".to_owned(),
        json!(options.strict_property_initialization),
    );
    result.insert(
        "useDefineForClassFields".to_owned(),
        json!(options.use_define_for_class_fields),
    );
    result.insert(
        "useUnknownInCatchVariables".to_owned(),
        json!(options.use_unknown_in_catch_variables),
    );
    result.insert(
        "lib".to_owned(),
        json!(options.lib.as_ref().map(|values| values
            .iter()
            .map(|value| catalog.option_file_name(value).unwrap_or(value))
            .collect::<Vec<_>>())),
    );
    result.insert("libReplacement".to_owned(), json!(options.lib_replacement));
    result.insert("jsx".to_owned(), json!(options.jsx));
    result.insert("noEmit".to_owned(), json!(options.no_emit));
    result.insert(
        "noEmitForJsFiles".to_owned(),
        json!(options.no_emit_for_js_files),
    );
    result.insert(
        "allowNonTsExtensions".to_owned(),
        json!(options.allow_non_ts_extensions),
    );
    result.insert(
        "listEmittedFiles".to_owned(),
        json!(options.list_emitted_files),
    );
    result.insert("emitBOM".to_owned(), json!(options.emit_bom));
    result.insert("noEmitOnError".to_owned(), json!(options.no_emit_on_error));
    result.insert("noCheck".to_owned(), json!(options.no_check));
    result.insert(
        "erasableSyntaxOnly".to_owned(),
        json!(options.erasable_syntax_only),
    );
    result.insert(
        "outDir".to_owned(),
        json!(options.out_dir.as_ref().map(string_value)),
    );
    result.insert(
        "rootDir".to_owned(),
        json!(options.root_dir.as_ref().map(string_value)),
    );
    result.insert("sourceMap".to_owned(), json!(options.source_map));
    result.insert(
        "inlineSourceMap".to_owned(),
        json!(options.inline_source_map),
    );
    result.insert("inlineSources".to_owned(), json!(options.inline_sources));
    result.insert(
        "sourceRoot".to_owned(),
        json!(options.source_root.as_ref().map(string_value)),
    );
    result.insert(
        "mapRoot".to_owned(),
        json!(options.map_root.as_ref().map(string_value)),
    );
    result.insert("declaration".to_owned(), json!(options.declaration));
    result.insert("declarationMap".to_owned(), json!(options.declaration_map));
    result.insert(
        "emitDeclarationOnly".to_owned(),
        json!(options.emit_declaration_only),
    );
    result.insert(
        "isolatedDeclarations".to_owned(),
        json!(options.isolated_declarations),
    );
    result.insert(
        "stableTypeOrdering".to_owned(),
        json!(options.stable_type_ordering),
    );
    result.insert(
        "declarationDir".to_owned(),
        json!(options.declaration_dir.as_ref().map(string_value)),
    );
    result.insert("stripInternal".to_owned(), json!(options.strip_internal));
    result.insert(
        "outFile".to_owned(),
        json!(options.out_file.as_ref().map(string_value)),
    );
    result.insert(
        "out".to_owned(),
        json!(options.out.as_ref().map(string_value)),
    );
    result.insert("incremental".to_owned(), json!(options.incremental));
    result.insert("composite".to_owned(), json!(options.composite));
    result.insert(
        "assumeChangesOnlyAffectDirectDependencies".to_owned(),
        json!(options.assume_changes_only_affect_direct_dependencies),
    );
    result.insert(
        "tsBuildInfoFile".to_owned(),
        json!(options.ts_build_info_file.as_ref().map(string_value)),
    );
    result.insert(
        "importsNotUsedAsValues".to_owned(),
        json!(options.imports_not_used_as_values),
    );
    result.insert(
        "preserveValueImports".to_owned(),
        json!(options.preserve_value_imports),
    );
    result.insert(
        "keyofStringsOnly".to_owned(),
        json!(options.keyof_strings_only),
    );
    result.insert(
        "suppressExcessPropertyErrors".to_owned(),
        json!(options.suppress_excess_property_errors),
    );
    result.insert(
        "suppressImplicitAnyIndexErrors".to_owned(),
        json!(options.suppress_implicit_any_index_errors),
    );
    result.insert(
        "noStrictGenericChecks".to_owned(),
        json!(options.no_strict_generic_checks),
    );
    result.insert(
        "charset".to_owned(),
        json!(options.charset.as_ref().map(string_value)),
    );
    result.insert(
        "emitDecoratorMetadata".to_owned(),
        json!(options.emit_decorator_metadata),
    );
    result.insert("newLine".to_owned(), json!(options.new_line));
    result.insert("removeComments".to_owned(), json!(options.remove_comments));
    result.insert(
        "noImplicitUseStrict".to_owned(),
        json!(options.no_implicit_use_strict),
    );
    result.insert("noEmitHelpers".to_owned(), json!(options.no_emit_helpers));
    result.insert("noResolve".to_owned(), json!(options.no_resolve));
    result.insert("importHelpers".to_owned(), json!(options.import_helpers));
    result.insert(
        "downlevelIteration".to_owned(),
        json!(options.downlevel_iteration),
    );
    result.insert(
        "strictBuiltinIteratorReturn".to_owned(),
        json!(options.strict_builtin_iterator_return),
    );
    result.insert(
        "moduleResolution".to_owned(),
        json!(options.module_resolution),
    );
    result.insert(
        "esModuleInterop".to_owned(),
        json!(options.es_module_interop),
    );
    result.insert(
        "allowSyntheticDefaultImports".to_owned(),
        json!(options.allow_synthetic_default_imports),
    );
    result.insert(
        "preserveConstEnums".to_owned(),
        json!(options.preserve_const_enums),
    );
    result.insert(
        "isolatedModules".to_owned(),
        json!(options.isolated_modules),
    );
    result.insert(
        "verbatimModuleSyntax".to_owned(),
        json!(options.verbatim_module_syntax),
    );
    result.insert(
        "allowUmdGlobalAccess".to_owned(),
        json!(options.allow_umd_global_access),
    );
    result.insert(
        "baseUrl".to_owned(),
        json!(options.base_url.as_ref().map(string_value)),
    );
    result.insert(
        "moduleSuffixes".to_owned(),
        json!(options.module_suffixes.as_ref().map(|values| values
            .iter()
            .map(|value| value.value_js().map(string_value))
            .collect::<Vec<_>>())),
    );
    result.insert(
        "resolvePackageJsonExports".to_owned(),
        json!(options.resolve_package_json_exports),
    );
    result.insert(
        "resolvePackageJsonImports".to_owned(),
        json!(options.resolve_package_json_imports),
    );
    result.insert(
        "customConditions".to_owned(),
        json!(options
            .custom_conditions
            .as_ref()
            .map(|values| values.iter().map(string_value).collect::<Vec<_>>())),
    );
    result.insert(
        "noDtsResolution".to_owned(),
        json!(options.no_dts_resolution),
    );
    result.insert(
        "allowArbitraryExtensions".to_owned(),
        json!(options.allow_arbitrary_extensions),
    );
    result.insert(
        "allowImportingTsExtensions".to_owned(),
        json!(options.allow_importing_ts_extensions),
    );
    result.insert(
        "rewriteRelativeImportExtensions".to_owned(),
        json!(options.rewrite_relative_import_extensions),
    );
    result.insert(
        "resolveJsonModule".to_owned(),
        json!(options.resolve_json_module),
    );
    result.insert("skipLibCheck".to_owned(), json!(options.skip_lib_check));
    result.insert(
        "skipDefaultLibCheck".to_owned(),
        json!(options.skip_default_lib_check),
    );
    result.insert(
        "jsxFactory".to_owned(),
        json!(options.jsx_factory.as_ref().map(string_value)),
    );
    result.insert(
        "jsxFragmentFactory".to_owned(),
        json!(options.jsx_fragment_factory.as_ref().map(string_value)),
    );
    result.insert(
        "jsxImportSource".to_owned(),
        json!(options.jsx_import_source.as_ref().map(string_value)),
    );
    result.insert(
        "reactNamespace".to_owned(),
        json!(options.react_namespace.as_ref().map(string_value)),
    );
    result.insert(
        "ignoreDeprecations".to_owned(),
        json!(options.ignore_deprecations.as_ref().map(string_value)),
    );
    result.insert(
        "configParsingDiagnostics".to_owned(),
        diagnostics(program.diagnostics().config()),
    );
    let program = program.program_options();
    result.insert(
        "paths".to_owned(),
        json!(program.paths().map(|entries| entries
            .iter()
            .map(|entry| json!({
                "pattern":string_value(entry.pattern()),
                "substitutions":entry.substitutions().iter().map(string_value).collect::<Vec<_>>()
            }))
            .collect::<Vec<_>>())),
    );
    result.insert(
        "pathsBasePath".to_owned(),
        json!(program.paths_base_path().map(string_value)),
    );
    result.insert(
        "defaultLibraryFileName".to_owned(),
        json!(program
            .default_library_file_name()
            .unwrap_or_else(|| catalog.default_file_name(options))),
    );
    result.insert(
        "configFile".to_owned(),
        json!(program.config_file().map(|file| json!({
        "file":string_value(file.diagnostic_file_name()), "sha256":sha256(file.text().as_bytes())
    }))),
    );
    result.insert(
        "externalConfigOptionDiagnostics".to_owned(),
        json!(program.external_config_option_diagnostics()),
    );
    result.insert("noLib".to_owned(), json!(program.no_lib()));
    result.insert(
        "preserveSymlinks".to_owned(),
        json!(program.preserve_symlinks()),
    );
    result.insert(
        "types".to_owned(),
        json!(program
            .types()
            .map(|values| values.iter().map(string_value).collect::<Vec<_>>())),
    );
    result.insert(
        "typeRoots".to_owned(),
        json!(program.type_roots().map(|values| values
            .iter()
            .map(|value| string_value(value.display()))
            .collect::<Vec<_>>())),
    );
    result.insert(
        "rootDirs".to_owned(),
        json!(program.root_dirs().map(|values| values
            .iter()
            .map(|value| string_value(value.display()))
            .collect::<Vec<_>>())),
    );
    result.insert(
        "configFilePath".to_owned(),
        json!(program
            .config_file_path()
            .map(|value| string_value(value.display()))),
    );
    Value::Object(result)
}
