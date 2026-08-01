use tsc_checker::{check_program_with_owned_libs_at, InputFile};
use tsc_compiler::{NoEmitOutcome, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_program::{
    CompilerOptions, PathContext, PreparationDiagnostics, PreparedAuxiliaryFile, PreparedProgram,
    PreparedSourceFile, ProgramPath,
};

const MINIMAL_GLOBALS: &str = r#"
interface IArguments { length: number; callee: Function; }
interface Array<T> { length: number; [index: number]: T; }
interface Object {}
interface Function {}
interface CallableFunction extends Function {}
interface NewableFunction extends Function {}
interface String {}
interface Number {}
interface Boolean {}
interface RegExp {}
"#;

fn path(display: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(display, display).expect("trusted test path")
}

fn current_directory() -> ProgramPath {
    ProgramPath::from_trusted_parts("/Display/Project", "/canonical/project").expect("trusted cwd")
}

fn diagnostic(code: u32, text: &str) -> Diagnostic {
    Diagnostic::new(
        None,
        None,
        None,
        MessageChain {
            code,
            category: DiagnosticCategory::Error,
            text: text.to_owned(),
            next_present: false,
            next: Vec::new(),
        },
    )
}

fn located_diagnostic(code: u32, file_name: &str, text: &str) -> Diagnostic {
    Diagnostic::new(
        Some(file_name.to_owned()),
        Some(0),
        Some(1),
        MessageChain {
            code,
            category: DiagnosticCategory::Error,
            text: text.to_owned(),
            next_present: false,
            next: Vec::new(),
        },
    )
}

fn prepared_program(
    files: &[(&str, &str)],
    library_count: usize,
    diagnostics: PreparationDiagnostics,
    configure: impl FnOnce(&mut CompilerOptions),
) -> PreparedProgram {
    prepared_program_with_root_policy(files, library_count, diagnostics, true, configure)
}

fn prepared_program_with_root_policy(
    files: &[(&str, &str)],
    library_count: usize,
    diagnostics: PreparationDiagnostics,
    add_non_lib_roots: bool,
    configure: impl FnOnce(&mut CompilerOptions),
) -> PreparedProgram {
    let mut options = CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    configure(&mut options);
    let mut builder =
        PreparedProgram::builder(PathContext::new(current_directory(), true), options);
    let mut ids = Vec::new();
    for (name, text) in files {
        ids.push(
            builder
                .add_source_file(PreparedSourceFile::new(path(name), *text))
                .expect("add source"),
        );
    }
    for source in ids.iter().copied().take(library_count) {
        builder.add_library_file(source).expect("add library");
    }
    if add_non_lib_roots {
        for source in ids.iter().copied().skip(library_count) {
            builder.add_root_file(source).expect("add root");
        }
    }
    builder.set_diagnostics(diagnostics);
    builder.build().expect("build prepared program")
}

fn with_minimal_lib(
    files: &[(&str, &str)],
    diagnostics: PreparationDiagnostics,
    configure: impl FnOnce(&mut CompilerOptions),
) -> PreparedProgram {
    let mut all_files = vec![("lib.d.ts", MINIMAL_GLOBALS)];
    all_files.extend_from_slice(files);
    prepared_program(&all_files, 1, diagnostics, configure)
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<u32> {
    diagnostics.iter().map(Diagnostic::code).collect()
}

fn consume(session: ProgramSession) -> NoEmitOutcome {
    session.run().expect("one-shot session")
}

#[test]
fn session_consumes_owned_program_and_sorts_batch_syntax_diagnostics() {
    let prepared = prepared_program(
        &[
            ("lib.d.ts", MINIMAL_GLOBALS),
            ("second.ts", "const second = ;"),
            ("first.ts", "const first = ;"),
        ],
        1,
        PreparationDiagnostics::default(),
        |_| {},
    );

    let outcome = consume(ProgramSession::new(prepared));
    let file_names = outcome
        .syntactic_diagnostics()
        .iter()
        .filter_map(|diagnostic| diagnostic.file_name.as_deref())
        .collect::<Vec<_>>();
    assert_eq!(file_names, ["first.ts", "second.ts"]);
    assert!(file_names.iter().all(|name| *name != "lib.d.ts"));
    assert!(outcome.options_diagnostics().is_empty());
    assert!(outcome.global_diagnostics().is_empty());
    assert!(outcome.semantic_diagnostics().is_empty());
}

#[test]
fn located_program_diagnostics_route_by_text_owner() {
    let mut builder = PreparedProgram::builder(
        PathContext::new(current_directory(), true),
        CompilerOptions {
            no_emit: Some(true),
            ..CompilerOptions::default()
        },
    );
    let lib = builder
        .add_source_file(PreparedSourceFile::new(path("lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add lib");
    let source = builder
        .add_source_file(PreparedSourceFile::new(path("main.ts"), "export {};"))
        .expect("add source");
    builder.add_library_file(lib).expect("add library");
    builder.add_root_file(source).expect("add root");
    builder
        .add_auxiliary_file(PreparedAuxiliaryFile::new(path("tsconfig.json"), "{}"))
        .expect("add config text");
    builder.set_diagnostics(PreparationDiagnostics::new(
        Vec::new(),
        Vec::new(),
        vec![
            located_diagnostic(9502, "main.ts", "source program diagnostic"),
            located_diagnostic(9501, "tsconfig.json", "config program diagnostic"),
        ],
    ));

    let outcome = consume(ProgramSession::new(
        builder.build().expect("build prepared program"),
    ));
    assert_eq!(codes(outcome.options_diagnostics()), [9501]);
    assert!(outcome.global_diagnostics().is_empty());
    assert!(outcome.semantic_diagnostics().is_empty());
}

#[test]
fn noemit_gates_all_five_buckets_and_keeps_config_outside_the_gate() {
    let config = diagnostic(9100, "config");
    let config_and_semantic = consume(ProgramSession::new(with_minimal_lib(
        &[("semantic.ts", "const value: string = 1;")],
        PreparationDiagnostics::new(
            vec![config.clone()],
            Vec::new(),
            vec![located_diagnostic(
                9101,
                "semantic.ts",
                "located program diagnostic",
            )],
        ),
        |_| {},
    )));
    assert_eq!(codes(config_and_semantic.config_diagnostics()), [9100]);
    assert!(config_and_semantic.syntactic_diagnostics().is_empty());
    assert!(config_and_semantic.options_diagnostics().is_empty());
    assert!(config_and_semantic.global_diagnostics().is_empty());
    assert_eq!(
        codes(config_and_semantic.semantic_diagnostics()),
        [9101, 2322]
    );
    assert_eq!(
        codes(
            &config_and_semantic
                .diagnostics()
                .cloned()
                .collect::<Vec<_>>()
        ),
        [9100, 9101, 2322]
    );

    let syntactic_gate = consume(ProgramSession::new(with_minimal_lib(
        &[("syntax.ts", "const value = ;")],
        PreparationDiagnostics::new(
            vec![config],
            vec![diagnostic(9200, "hidden option")],
            Vec::new(),
        ),
        |_| {},
    )));
    assert_eq!(codes(syntactic_gate.config_diagnostics()), [9100]);
    assert!(!syntactic_gate.syntactic_diagnostics().is_empty());
    assert!(syntactic_gate.options_diagnostics().is_empty());
    assert!(syntactic_gate.global_diagnostics().is_empty());
    assert!(syntactic_gate.semantic_diagnostics().is_empty());

    let duplicate = diagnostic(9302, "duplicate option");
    let options_gate = consume(ProgramSession::new(with_minimal_lib(
        &[("semantic.ts", "const value: string = 1;")],
        PreparationDiagnostics::new(
            Vec::new(),
            vec![duplicate.clone()],
            vec![diagnostic(9301, "program option"), duplicate],
        ),
        |_| {},
    )));
    assert_eq!(codes(options_gate.options_diagnostics()), [9301, 9302]);
    assert!(options_gate.global_diagnostics().is_empty());
    assert!(options_gate.semantic_diagnostics().is_empty());

    let global_gate = consume(ProgramSession::new(prepared_program(
        &[("global.ts", "export {};")],
        0,
        PreparationDiagnostics::default(),
        |_| {},
    )));
    assert!(global_gate.syntactic_diagnostics().is_empty());
    assert!(global_gate.options_diagnostics().is_empty());
    assert!(!global_gate.global_diagnostics().is_empty());
    assert!(global_gate
        .global_diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.code() == 2318));
    assert!(global_gate.semantic_diagnostics().is_empty());

    let empty_program = consume(ProgramSession::new(prepared_program(
        &[],
        0,
        PreparationDiagnostics::default(),
        |_| {},
    )));
    assert!(empty_program.global_diagnostics().is_empty());

    let prepared_without_roots = prepared_program_with_root_policy(
        &[("rootless.ts", "export {};")],
        0,
        PreparationDiagnostics::default(),
        false,
        |_| {},
    );
    assert!(prepared_without_roots.roots().is_empty());
    let source_without_roots = consume(ProgramSession::new(prepared_without_roots));
    assert!(source_without_roots.global_diagnostics().is_empty());
}

#[test]
fn suggestion_getter_rows_never_enter_noemit_outcome() {
    let options = CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    let checked = check_program_with_owned_libs_at(
        &[InputFile {
            name: "lib.d.ts".to_owned(),
            text: MINIMAL_GLOBALS.to_owned(),
        }],
        &[InputFile {
            name: "suggestion.ts".to_owned(),
            text: "export {};\nconst dead = 1;\n".to_owned(),
        }],
        &options,
        "/Display/Project",
    );
    assert!(checked
        .suggestion_diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code() == 6133));

    let outcome = consume(ProgramSession::new(with_minimal_lib(
        &[("suggestion.ts", "export {};\nconst dead = 1;\n")],
        PreparationDiagnostics::default(),
        |_| {},
    )));

    assert!(outcome
        .diagnostics()
        .all(|diagnostic| diagnostic.category() != DiagnosticCategory::Suggestion));
    assert!(outcome
        .diagnostics()
        .all(|diagnostic| diagnostic.code() != 6133));
}

#[test]
fn checker_receives_the_current_directory_display_spelling() {
    let outcome = consume(ProgramSession::new(with_minimal_lib(
        &[(
            "src/main.ts",
            "/// <reference path=\"./missing.ts\" />\nexport {};\n",
        )],
        PreparationDiagnostics::default(),
        |_| {},
    )));

    let missing = outcome
        .semantic_diagnostics()
        .iter()
        .find(|diagnostic| diagnostic.code() == 6053)
        .expect("missing path reference diagnostic");
    assert!(missing
        .message_text()
        .contains("/Display/Project/src/missing.ts"));
    assert!(!missing.message_text().contains("/canonical/project"));
}

#[test]
fn repeated_owned_sessions_are_deterministic() {
    fn make_program() -> PreparedProgram {
        with_minimal_lib(
            &[
                ("b.ts", "const b: string = 1;"),
                ("a.ts", "const a: number = 'x';"),
            ],
            PreparationDiagnostics::new(vec![diagnostic(9400, "config")], Vec::new(), Vec::new()),
            |_| {},
        )
    }

    let first = consume(ProgramSession::new(make_program()));
    let second = consume(ProgramSession::new(make_program()));
    assert_eq!(first, second);
    assert_eq!(first.clone().into_diagnostics(), second.into_diagnostics());
}
