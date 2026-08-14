use super::*;
use tsc_program::{PathContext, ProgramPath};

#[test]
fn prepared_to_checker_projection_preserves_the_exact_snapshot_arc() {
    let path = ProgramPath::from_trusted_parts("main.ts", "main.ts").expect("test path");
    let source = PreparedSourceFile::new(path, "export const value = 1;");
    let (input, _) = project_source(&source, SourceFileId::from_raw(0)).expect("projection");

    assert!(Arc::ptr_eq(source.snapshot(), input.snapshot()));
    assert_eq!(input.text(), source.text());
    assert_eq!(
        input.snapshot().positions().kind(),
        tsc_diagnostics::PositionIndexKind::StaticDense
    );
}

#[test]
fn emit_diagnostic_assembly_uses_the_whole_program_semantic_stream() {
    let current_directory =
        ProgramPath::from_trusted_parts("/.src", "/.src").expect("current directory");
    let prepared = PreparedProgram::emitting_builder(
        PathContext::new(current_directory, true),
        CompilerOptions::default(),
    )
    .build()
    .expect("empty emitting program");
    let diagnostic = |code, text: &str| {
        Diagnostic::new(
            None,
            None,
            None,
            MessageChain {
                code,
                category: tsc_diagnostics::DiagnosticCategory::Error,
                text: text.to_owned(),
                next_present: false,
                next: Vec::new(),
            },
        )
    };
    let fixture_only = diagnostic(9001, "fixture getter");
    let whole_program = diagnostic(9002, "whole Program getter");
    let checked = CheckResult {
        semantic_diagnostics: vec![fixture_only],
        program_semantic_diagnostics: Some(vec![whole_program]),
        ..CheckResult::default()
    };

    let diagnostics = emit_session_diagnostics(&prepared, &checked);
    assert_eq!(
        diagnostics
            .semantic
            .iter()
            .map(Diagnostic::code)
            .collect::<Vec<_>>(),
        [9002]
    );
}

fn option_diagnostics(options: CompilerOptions) -> DiagnosticList {
    let current_directory =
        ProgramPath::from_trusted_parts("/.src", "/.src").expect("current directory");
    let prepared =
        PreparedProgram::emitting_builder(PathContext::new(current_directory, true), options)
            .build()
            .expect("empty emitting program");
    programmatic_option_diagnostics(&prepared)
}

#[test]
fn exact_compiler_fixture_option_diagnostics_are_fileless_and_exact() {
    let cases = [
        (
            CompilerOptions {
                isolated_declarations: Some(true),
                ..CompilerOptions::default()
            },
            5069,
            "Option 'isolatedDeclarations' cannot be specified without specifying option 'declaration' or option 'composite'.",
        ),
        (
            CompilerOptions {
                jsx: Some(2),
                jsx_factory: Some("h".to_owned()),
                jsx_fragment_factory: Some("234".to_owned()),
                ..CompilerOptions::default()
            },
            18035,
            "Invalid value for 'jsxFragmentFactory'. '234' is not a valid identifier or qualified-name.",
        ),
        (
            CompilerOptions {
                jsx_factory: Some("Element.createElement".to_owned()),
                react_namespace: Some("Element".to_owned()),
                ..CompilerOptions::default()
            },
            5053,
            "Option 'reactNamespace' cannot be specified with option 'jsxFactory'.",
        ),
        (
            CompilerOptions {
                jsx_factory: Some("Element.createElement=".to_owned()),
                ..CompilerOptions::default()
            },
            5067,
            "Invalid value for 'jsxFactory'. 'Element.createElement=' is not a valid identifier or qualified-name.",
        ),
        (
            CompilerOptions {
                jsx_factory: Some("id1 id2".to_owned()),
                ..CompilerOptions::default()
            },
            5067,
            "Invalid value for 'jsxFactory'. 'id1 id2' is not a valid identifier or qualified-name.",
        ),
        (
            CompilerOptions {
                react_namespace: Some("my-React-Lib".to_owned()),
                ..CompilerOptions::default()
            },
            5059,
            "Invalid value for '--reactNamespace'. 'my-React-Lib' is not a valid identifier.",
        ),
        (
            CompilerOptions {
                strict: Some(false),
                exact_optional_property_types: Some(true),
                ..CompilerOptions::default()
            },
            5052,
            "Option 'exactOptionalPropertyTypes' cannot be specified without specifying option 'strictNullChecks'.",
        ),
    ];

    for (options, expected_code, expected_message) in cases {
        let diagnostics = option_diagnostics(options);
        let [diagnostic] = diagnostics.as_slice() else {
            panic!("expected one option diagnostic, got {diagnostics:#?}");
        };
        assert_eq!(diagnostic.code(), expected_code);
        assert_eq!(diagnostic.message_text(), expected_message);
        assert_eq!(diagnostic.file_name, None);
        assert_eq!(diagnostic.start, None);
        assert_eq!(diagnostic.length, None);
    }
}

#[test]
fn option_dependencies_prevent_false_positive_diagnostics() {
    for options in [
        CompilerOptions {
            isolated_declarations: Some(true),
            declaration: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            strict: Some(false),
            strict_null_checks: Some(true),
            exact_optional_property_types: Some(true),
            ..CompilerOptions::default()
        },
        CompilerOptions {
            jsx_factory: Some("Element.createElement".to_owned()),
            jsx_fragment_factory: Some("Element.Fragment".to_owned()),
            ..CompilerOptions::default()
        },
    ] {
        assert!(option_diagnostics(options).is_empty());
    }
}
