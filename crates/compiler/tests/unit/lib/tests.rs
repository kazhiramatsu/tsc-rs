use super::*;
use tsc_program::{PathContext, ProgramPath};

#[test]
fn prepared_package_objects_are_shared_only_with_the_exact_host_snapshot() {
    for case_sensitive in [true, false] {
        for text in [
            r#"{"name":"pkg", "type":"module", "exports":{".":"./main.js"}}"#,
            "\u{feff}{ // JSONC\n \"name\": \"pkg\\ud800\", \"type\": \"commonjs\", }",
            r#"{"__proto__":{"name":"inherited"}, "type":null, trailing: true}"#,
            "{ invalid",
        ] {
            let directory = ProgramPath::from_trusted_parts("/work", "/work").unwrap();
            let mut builder = PreparedProgram::builder(
                PathContext::new(directory, case_sensitive),
                CompilerOptions {
                    no_emit: Some(true),
                    ..CompilerOptions::default()
                },
            );
            let display = "/work/Package/package.json";
            let canonical = if case_sensitive {
                display
            } else {
                "/work/package/package.json"
            };
            builder
                .add_package_metadata(tsc_program::PackageMetadata::new(
                    ProgramPath::from_trusted_parts(display, canonical).unwrap(),
                    text,
                ))
                .unwrap();
            let prepared = builder.build().unwrap();
            let package = prepared.packages().next().unwrap();
            let uncached = package.clone();
            let expected = tsc_program::JsonValue::Object(
                tsc_program::read_package_json_object_from_snapshot(
                    package.package_json().display(),
                    package.snapshot(),
                ),
            );
            let provider = PreparedModuleProvider {
                prepared: &prepared,
                request_plans: Default::default(),
                tracing: None,
            };
            let value = provider
                .package_json_value(display.into(), package.snapshot())
                .unwrap();
            assert!(Arc::ptr_eq(&value, package.parsed_json()));
            assert_eq!(*value, expected);
            assert_eq!(
                *package, uncached,
                "cache initialization preserves metadata equality"
            );
            let other_snapshot = tsc_diagnostics::TextSnapshot::new(
                text.to_owned(),
                tsc_diagnostics::DocumentVersion::default(),
            );
            assert!(provider
                .package_json_value(display.into(), &other_snapshot)
                .is_none());
            assert!(provider
                .package_json_value("/work/unknown/package.json".into(), package.snapshot())
                .is_none());
        }
    }
}

#[test]
fn prepared_to_checker_projection_preserves_the_exact_snapshot_arc() {
    let path = ProgramPath::from_trusted_parts("main.ts", "main.ts").expect("test path");
    let source = PreparedSourceFile::new(path, "export const value = 1;");
    let (input, _) = project_source(&source, SourceFileId::from_raw(0), None).expect("projection");

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
                text: text.into(),
                key: None,
                args: Vec::new(),
                next_present: false,
                next: Vec::new(),
                related: Vec::new(),
                repopulate: None,
                without_location: false,
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
                jsx_factory: Some("h".into()),
                jsx_fragment_factory: Some("234".into()),
                ..CompilerOptions::default()
            },
            18035,
            "Invalid value for 'jsxFragmentFactory'. '234' is not a valid identifier or qualified-name.",
        ),
        (
            CompilerOptions {
                jsx_factory: Some("Element.createElement".into()),
                react_namespace: Some("Element".into()),
                ..CompilerOptions::default()
            },
            5053,
            "Option 'reactNamespace' cannot be specified with option 'jsxFactory'.",
        ),
        (
            CompilerOptions {
                jsx_factory: Some("Element.createElement=".into()),
                ..CompilerOptions::default()
            },
            5067,
            "Invalid value for 'jsxFactory'. 'Element.createElement=' is not a valid identifier or qualified-name.",
        ),
        (
            CompilerOptions {
                jsx_factory: Some("id1 id2".into()),
                ..CompilerOptions::default()
            },
            5067,
            "Invalid value for 'jsxFactory'. 'id1 id2' is not a valid identifier or qualified-name.",
        ),
        (
            CompilerOptions {
                react_namespace: Some("my-React-Lib".into()),
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
            jsx_factory: Some("Element.createElement".into()),
            jsx_fragment_factory: Some("Element.Fragment".into()),
            ..CompilerOptions::default()
        },
    ] {
        assert!(option_diagnostics(options).is_empty());
    }
}

#[test]
fn checker_association_keeps_syntactic_multiplicity_and_resolution_keys_separate() {
    let path = |name: &str| ProgramPath::from_trusted_parts(name, name).unwrap();
    let mut builder = PreparedProgram::builder(
        PathContext::new(path("/src"), true),
        CompilerOptions {
            no_emit: Some(true),
            module: Some(99),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        },
    );
    let a = builder
        .add_source_file(
            PreparedSourceFile::new(path("/src/a.ts"), "export const a = 1;").with_import_count(0),
        )
        .unwrap();
    let b = builder
        .add_source_file(
            PreparedSourceFile::new(
                path("/src/b.ts"),
                "import './a'; import './a'; import './alias'; import './missing';",
            )
            .with_import_count(4),
        )
        .unwrap();
    for specifier in ["./a", "./alias"] {
        builder
            .add_module_resolution(
                ResolutionKey::new(
                    path("/src/b.ts").canonical().clone(),
                    specifier,
                    ResolutionMode::Unspecified,
                ),
                Ok(tsc_program::ModuleResolution::resolved(
                    tsc_program::ResolvedModule::new(
                        ResolvedModuleTarget::Source {
                            source: a,
                            resolved_file: path("/src/a.ts"),
                        },
                        ModuleExtension::Ts,
                    ),
                )),
            )
            .unwrap();
    }
    let prepared = builder.build().unwrap();
    let provider = PreparedModuleProvider {
        prepared: &prepared,
        request_plans: Default::default(),
        tracing: None,
    };
    let imports = provider.checker_association_imports();
    assert_eq!(imports[b.index()].1, 4);
    assert_eq!(imports[b.index()].2, [AuthoritativeSourceToken(a.raw()); 2]);
    assert!(imports[a.index()].2.is_empty());
}

#[test]
fn declaration_order_consumption_replays_after_pending_prints_complete() {
    use std::num::NonZeroUsize;
    use tsc_program::PreparedSourceFile;
    let run = |workers, checkers| {
        let directory = ProgramPath::from_trusted_parts("/work", "/work").unwrap();
        let mut builder = PreparedProgram::emitting_builder(
            PathContext::new(directory, true),
            CompilerOptions {
                target: Some(9),
                module: Some(99),
                declaration: Some(true),
                source_map: Some(true),
                declaration_map: Some(true),
                no_check: Some(true),
                stable_type_ordering: Some(false),
                list_emitted_files: Some(true),
                out_dir: Some("/work/out".into()),
                ..CompilerOptions::default()
            },
        );
        let globals = "interface IArguments { length: number; callee: Function; } interface Array<T> { length: number; [index: number]: T; } interface Object {} interface Function {} interface CallableFunction extends Function {} interface NewableFunction extends Function {} interface String {} interface Number {} interface Boolean {} interface RegExp {}";
        let lib = builder
            .add_source_file(PreparedSourceFile::new(
                ProgramPath::from_trusted_parts("/work/lib.d.ts", "/work/lib.d.ts").unwrap(),
                globals,
            ))
            .unwrap();
        builder.add_library_file(lib).unwrap();
        for index in 0..4 {
            let path = format!("/work/unit{index}.ts");
            let text = format!("interface Alpha{index} {{ alpha: number; }}\ninterface Beta{index} {{ beta: string; }}\ndeclare const choose: boolean;\nexport const value = choose ? ({{}} as Alpha{index}) : ({{}} as Beta{index});\n");
            let source = builder
                .add_source_file(PreparedSourceFile::new(
                    ProgramPath::from_trusted_parts(path.as_str(), path.as_str()).unwrap(),
                    text,
                ))
                .unwrap();
            builder.add_root_file(source).unwrap();
        }
        let mut sink = MemoryOutputSink::new();
        let result = ProgramSession::new(builder.build().unwrap())
            .with_worker_budget(WorkerBudget::new(NonZeroUsize::new(workers).unwrap()))
            .with_checker_budget(
                CheckerBudget::new(NonZeroUsize::new(checkers).unwrap()).with_order_replay(true),
            )
            .emit_for_cli(&mut sink)
            .unwrap();
        (result, sink)
    };
    let (serial, expected) = run(1, 1);
    assert_eq!(serial.work_counters.checker_serial_replay(), 0);
    for (workers, checkers) in [(4, 4), (10, 4)] {
        let (actual, sink) = run(workers, checkers);
        assert_eq!(sink, expected);
        assert_eq!(actual.emit, serial.emit);
        assert_eq!(actual.work_counters.checker_serial_replay(), 1);
        assert_ne!(actual.work_counters.checker_replay_reasons(), 0);
    }
}
