use tsc_program::{
    compiler_option_declaration, compiler_option_declarations, compiler_option_spelling_suggestion,
    is_command_option_without_build, jsconfig_defaults, typescript_6_0_3_libraries,
    CompilerOptionListElementKind, CompilerOptionValueKind, JsConfigDefaultValue,
};

#[test]
fn catalog_preserves_the_typescript_6_0_3_declaration_order() {
    let expected = [
        "help",
        "help",
        "watch",
        "preserveWatchOutput",
        "listFiles",
        "explainFiles",
        "listEmittedFiles",
        "pretty",
        "traceResolution",
        "diagnostics",
        "extendedDiagnostics",
        "generateCpuProfile",
        "generateTrace",
        "incremental",
        "declaration",
        "declarationMap",
        "emitDeclarationOnly",
        "sourceMap",
        "inlineSourceMap",
        "noCheck",
        "noEmit",
        "assumeChangesOnlyAffectDirectDependencies",
        "locale",
        "all",
        "version",
        "init",
        "project",
        "showConfig",
        "listFilesOnly",
        "ignoreConfig",
        "target",
        "module",
        "lib",
        "allowJs",
        "checkJs",
        "jsx",
        "outFile",
        "outDir",
        "rootDir",
        "composite",
        "tsBuildInfoFile",
        "removeComments",
        "importHelpers",
        "importsNotUsedAsValues",
        "downlevelIteration",
        "isolatedModules",
        "verbatimModuleSyntax",
        "isolatedDeclarations",
        "erasableSyntaxOnly",
        "libReplacement",
        "strict",
        "noImplicitAny",
        "strictNullChecks",
        "strictFunctionTypes",
        "strictBindCallApply",
        "strictPropertyInitialization",
        "strictBuiltinIteratorReturn",
        "stableTypeOrdering",
        "noImplicitThis",
        "useUnknownInCatchVariables",
        "alwaysStrict",
        "noUnusedLocals",
        "noUnusedParameters",
        "exactOptionalPropertyTypes",
        "noImplicitReturns",
        "noFallthroughCasesInSwitch",
        "noUncheckedIndexedAccess",
        "noImplicitOverride",
        "noPropertyAccessFromIndexSignature",
        "moduleResolution",
        "baseUrl",
        "paths",
        "rootDirs",
        "typeRoots",
        "types",
        "allowSyntheticDefaultImports",
        "esModuleInterop",
        "preserveSymlinks",
        "allowUmdGlobalAccess",
        "moduleSuffixes",
        "allowImportingTsExtensions",
        "rewriteRelativeImportExtensions",
        "resolvePackageJsonExports",
        "resolvePackageJsonImports",
        "customConditions",
        "noUncheckedSideEffectImports",
        "sourceRoot",
        "mapRoot",
        "inlineSources",
        "experimentalDecorators",
        "emitDecoratorMetadata",
        "jsxFactory",
        "jsxFragmentFactory",
        "jsxImportSource",
        "resolveJsonModule",
        "allowArbitraryExtensions",
        "out",
        "reactNamespace",
        "skipDefaultLibCheck",
        "charset",
        "emitBOM",
        "newLine",
        "noErrorTruncation",
        "noLib",
        "noResolve",
        "stripInternal",
        "disableSizeLimit",
        "disableSourceOfProjectReferenceRedirect",
        "disableSolutionSearching",
        "disableReferencedProjectLoad",
        "noImplicitUseStrict",
        "noEmitHelpers",
        "noEmitOnError",
        "preserveConstEnums",
        "declarationDir",
        "skipLibCheck",
        "allowUnusedLabels",
        "allowUnreachableCode",
        "suppressExcessPropertyErrors",
        "suppressImplicitAnyIndexErrors",
        "forceConsistentCasingInFileNames",
        "maxNodeModuleJsDepth",
        "noStrictGenericChecks",
        "useDefineForClassFields",
        "preserveValueImports",
        "keyofStringsOnly",
        "plugins",
        "moduleDetection",
        "ignoreDeprecations",
    ];
    let actual = compiler_option_declarations()
        .iter()
        .map(|declaration| declaration.name())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn structured_metadata_and_the_shared_lib_map_match_typescript_6_0_3() {
    let expected = [
        ("lib", "lib", "named", false, false),
        ("rootDirs", "rootDirs", "path", false, true),
        ("typeRoots", "typeRoots", "path", false, true),
        ("types", "types", "string", false, false),
        ("moduleSuffixes", "suffix", "string", true, false),
        ("customConditions", "condition", "string", false, false),
        ("plugins", "plugin", "object", false, false),
    ];
    for (name, element_name, element_kind, preserve_falsy, substitute_config_dir) in expected {
        let descriptor = compiler_option_declaration(name)
            .unwrap_or_else(|| panic!("missing list declaration {name}"))
            .value_kind()
            .list_descriptor()
            .unwrap_or_else(|| panic!("{name} is not a list declaration"));
        assert_eq!(descriptor.element_name(), element_name, "{name}");
        assert_eq!(
            match descriptor.element_kind() {
                CompilerOptionListElementKind::String => "string",
                CompilerOptionListElementKind::FilePath => "path",
                CompilerOptionListElementKind::NamedString(_) => "named",
                CompilerOptionListElementKind::Object => "object",
            },
            element_kind,
            "{name}"
        );
        assert_eq!(descriptor.preserve_falsy_values(), preserve_falsy, "{name}");
        assert_eq!(
            descriptor.allow_config_dir_template_substitution(),
            substitute_config_dir,
            "{name}"
        );
    }

    let libraries = typescript_6_0_3_libraries();
    assert_eq!(libraries.len(), 107);
    assert_eq!(
        (libraries[0].name(), libraries[0].value()),
        ("es5", "lib.es5.d.ts")
    );
    assert_eq!(
        (libraries[1].name(), libraries[1].value()),
        ("es6", "lib.es2015.d.ts")
    );
    assert_eq!(
        (libraries[106].name(), libraries[106].value()),
        ("decorators.legacy", "lib.decorators.legacy.d.ts")
    );
    let lib = compiler_option_declaration("lib")
        .unwrap()
        .value_kind()
        .list_descriptor()
        .unwrap();
    assert_eq!(lib.named_string_value("DOM"), Some("lib.dom.d.ts"));
    assert_eq!(lib.named_string_value(" es5"), None);

    let object_options = compiler_option_declarations()
        .iter()
        .filter_map(|declaration| {
            declaration
                .value_kind()
                .object_descriptor()
                .map(|descriptor| (declaration, descriptor))
        })
        .collect::<Vec<_>>();
    let [(paths, descriptor)] = object_options.as_slice() else {
        panic!("paths is TypeScript 6.0.3's only object compiler option")
    };
    assert_eq!(paths.name(), "paths");
    assert!(paths.is_tsconfig_only());
    assert!(!paths.is_file_path());
    assert!(descriptor.allow_config_dir_template_substitution());
}

#[test]
fn exact_lookup_and_required_typed_metadata_are_exposed() {
    assert!(compiler_option_declaration("allowJs").is_some());
    assert!(compiler_option_declaration("ALLOWJS").is_none());

    for name in ["strict", "allowJs", "checkJs", "resolveJsonModule"] {
        assert_eq!(
            compiler_option_declaration(name).unwrap().value_kind(),
            CompilerOptionValueKind::Boolean
        );
    }
    for name in ["outDir", "declarationDir"] {
        let declaration = compiler_option_declaration(name).unwrap();
        assert_eq!(declaration.value_kind(), CompilerOptionValueKind::String);
        assert!(declaration.is_file_path());
    }

    let module = compiler_option_declaration("module").unwrap().value_kind();
    assert_eq!(module.named_value("node20"), Some(102));
    assert_eq!(module.named_value("NodeNext"), Some(199));
    let resolution = compiler_option_declaration("moduleResolution")
        .unwrap()
        .value_kind();
    assert_eq!(resolution.named_value("node"), Some(2));
    assert_eq!(resolution.named_value("bundler"), Some(100));

    for name in ["help", "watch", "incremental", "declaration", "locale"] {
        assert!(!is_command_option_without_build(name), "{name}");
    }
    for name in ["all", "version", "project", "target", "strict"] {
        assert!(is_command_option_without_build(name), "{name}");
    }
}

#[test]
fn jsconfig_defaults_preserve_upstream_object_insertion_order() {
    assert_eq!(
        jsconfig_defaults(),
        &[
            ("allowJs", JsConfigDefaultValue::Boolean(true)),
            ("maxNodeModuleJsDepth", JsConfigDefaultValue::Number(2)),
            (
                "allowSyntheticDefaultImports",
                JsConfigDefaultValue::Boolean(true)
            ),
            ("skipLibCheck", JsConfigDefaultValue::Boolean(true)),
            ("noEmit", JsConfigDefaultValue::Boolean(true)),
        ]
    );
}

#[test]
fn spelling_suggestions_follow_utf16_distance_and_tsc_candidate_order() {
    let suggest = |name| compiler_option_spelling_suggestion(name).map(|option| option.name());
    assert_eq!(suggest("strct"), Some("strict"));
    assert_eq!(suggest("ALLOWJS"), Some("allowJs"));
    assert_eq!(suggest("moduleResoluton"), Some("moduleResolution"));
    assert_eq!(suggest("declaratonDir"), Some("declarationDir"));
    assert_eq!(suggest("str😀ict"), Some("strict"));
    assert_eq!(suggest("not-an-option"), None);
    assert_eq!(suggest("help"), None);
}
