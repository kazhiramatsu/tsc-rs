use super::*;

#[test]
fn exact_h2_5g_option_failures_are_typed_in_tsc_order() {
    let cases = [
        (
            CompilerOptions {
                isolated_declarations: Some(true),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::IsolatedDeclarationsRequiresDeclaration,
        ),
        (
            CompilerOptions {
                jsx: Some(2),
                jsx_factory: Some("h".to_owned()),
                jsx_fragment_factory: Some("234".to_owned()),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::InvalidJsxFragmentFactory {
                value: "234".to_owned(),
            },
        ),
        (
            CompilerOptions {
                jsx_factory: Some("Element.createElement".to_owned()),
                react_namespace: Some("Element".to_owned()),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::ReactNamespaceConflictsWithJsxFactory,
        ),
        (
            CompilerOptions {
                jsx_factory: Some("Element.createElement=".to_owned()),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::InvalidJsxFactory {
                value: "Element.createElement=".to_owned(),
            },
        ),
        (
            CompilerOptions {
                jsx_factory: Some("id1 id2".to_owned()),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::InvalidJsxFactory {
                value: "id1 id2".to_owned(),
            },
        ),
        (
            CompilerOptions {
                react_namespace: Some("my-React-Lib".to_owned()),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::InvalidReactNamespace {
                value: "my-React-Lib".to_owned(),
            },
        ),
        (
            CompilerOptions {
                strict: Some(false),
                exact_optional_property_types: Some(true),
                ..CompilerOptions::default()
            },
            CompilerOptionViolation::ExactOptionalPropertyTypesRequiresStrictNullChecks,
        ),
    ];

    for (options, expected) in cases {
        assert_eq!(validate_compiler_options(&options), [expected]);
    }
}

#[test]
fn valid_dependency_options_close_their_relationships() {
    for options in [
        CompilerOptions {
            isolated_declarations: Some(true),
            declaration: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            isolated_declarations: Some(true),
            composite: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            strict: Some(false),
            strict_null_checks: Some(true),
            exact_optional_property_types: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            jsx_factory: Some("Element . createElement".to_owned()),
            jsx_fragment_factory: Some("Element.Fragment".to_owned()),
            ..CompilerOptions::default()
        },
    ] {
        assert!(validate_compiler_options(&options).is_empty());
    }
}

#[test]
fn source_map_relationships_follow_tsc_order_and_messages() {
    assert_eq!(
        validate_compiler_options(&CompilerOptions {
            source_root: Some("sources".to_owned()),
            map_root: Some("maps".to_owned()),
            ..CompilerOptions::default()
        }),
        [
            CompilerOptionViolation::SourceRootRequiresSourceMap,
            CompilerOptionViolation::MapRootRequiresSourceMapOrDeclarationMap,
        ]
    );
    assert_eq!(
        validate_compiler_options(&CompilerOptions {
            inline_source_map: Some(true),
            map_root: Some("maps".to_owned()),
            ..CompilerOptions::default()
        }),
        [
            CompilerOptionViolation::MapRootConflictsWithInlineSourceMap,
            CompilerOptionViolation::MapRootRequiresSourceMapOrDeclarationMap,
        ]
    );
    assert_eq!(
        validate_compiler_options(&CompilerOptions {
            source_map: Some(true),
            inline_source_map: Some(true),
            ..CompilerOptions::default()
        }),
        [CompilerOptionViolation::SourceMapConflictsWithInlineSourceMap]
    );
    assert_eq!(
        validate_compiler_options(&CompilerOptions {
            inline_sources: Some(true),
            ..CompilerOptions::default()
        }),
        [CompilerOptionViolation::InlineSourcesRequiresSourceMap]
    );

    let source_root = CompilerOptionViolation::SourceRootRequiresSourceMap;
    assert_eq!(
        source_root.location(),
        CompilerOptionValidationLocation::Name
    );
    assert_eq!(source_root.message().code, 5051);
    assert_eq!(
        source_root.message().text,
        "Option 'sourceRoot can only be used when either option '--inlineSourceMap' or option '--sourceMap' is provided."
    );
    let map_root = CompilerOptionViolation::MapRootRequiresSourceMapOrDeclarationMap;
    assert_eq!(map_root.message().code, 5069);
    assert_eq!(
        map_root.message().text,
        "Option 'mapRoot' cannot be specified without specifying option 'sourceMap' or option 'declarationMap'."
    );
}

#[test]
fn source_map_relationships_accept_their_prerequisites() {
    for options in [
        CompilerOptions {
            source_map: Some(true),
            source_root: Some("sources".to_owned()),
            map_root: Some("maps".to_owned()),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            declaration_map: Some(true),
            map_root: Some("maps".to_owned()),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            inline_source_map: Some(true),
            source_root: Some("sources".to_owned()),
            inline_sources: Some(true),
            ..CompilerOptions::default()
        },
    ] {
        assert!(validate_compiler_options(&options).is_empty());
    }
}

#[test]
fn empty_jsx_strings_follow_javascript_truthiness() {
    let options = CompilerOptions {
        jsx: Some(2),
        jsx_factory: Some(String::new()),
        jsx_fragment_factory: Some(String::new()),
        jsx_import_source: Some(String::new()),
        react_namespace: Some(String::new()),
        ..CompilerOptions::default()
    };

    assert!(validate_compiler_options(&options).is_empty());
}

#[test]
fn one_snapshot_can_report_multiple_violations_without_filtering() {
    let options = CompilerOptions {
        allow_js: true,
        isolated_declarations: Some(true),
        jsx: Some(4),
        jsx_factory: Some("not valid".to_owned()),
        react_namespace: Some("React".to_owned()),
        ..CompilerOptions::default()
    };

    assert_eq!(
        validate_compiler_options(&options),
        [
            CompilerOptionViolation::IsolatedDeclarationsConflictsWithAllowJs,
            CompilerOptionViolation::IsolatedDeclarationsRequiresDeclaration,
            CompilerOptionViolation::ReactNamespaceConflictsWithJsxFactory,
            CompilerOptionViolation::JsxFactoryConflictsWithAutomaticRuntime { jsx: "react-jsx" },
            CompilerOptionViolation::InvalidJsxFactory {
                value: "not valid".to_owned()
            },
            CompilerOptionViolation::ReactNamespaceConflictsWithAutomaticRuntime {
                jsx: "react-jsx"
            },
        ]
    );
}

#[test]
fn source_map_relationship_diagnostics_stay_non_fatal_in_the_config_gate() {
    // The W5 K22 rows report like the 5101/5107 family: the program still
    // loads, checks, and emits (verifyCompilerOptions rows are not a
    // source-loading gate). A fatal classification would suppress the
    // frozen target writes and flip emit_skipped.
    for (code, violation) in [
        (5051, CompilerOptionViolation::SourceRootRequiresSourceMap),
        (
            5053,
            CompilerOptionViolation::SourceMapConflictsWithInlineSourceMap,
        ),
        (
            5069,
            CompilerOptionViolation::MapRootRequiresSourceMapOrDeclarationMap,
        ),
    ] {
        assert_eq!(violation.message().code, code);
        let diagnostic = tsc_diagnostics::Diagnostic::new(None, None, None, violation.message());
        assert!(
            crate::is_non_fatal_option_diagnostic(&diagnostic),
            "code {code} must pass the non-fatal option gate"
        );
    }
}
