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
