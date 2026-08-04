use tsc_program::{
    compiler_option_declaration, compiler_option_declarations, compiler_option_spelling_suggestion,
    is_command_option_without_build, jsconfig_defaults, CompilerOptionValueKind,
    JsConfigDefaultValue,
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
