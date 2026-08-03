use tsc_checker::{
    check_program_with_owned_libs_at, AuthoritativeModuleFailure, AuthoritativeModuleLookupFailure,
    InputFile, UnsupportedAuthoritativeResolution,
};
use tsc_compiler::{DriverError, NoEmitOutcome, ProgramSession};
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, MessageChain};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    CompilerOptions, ModuleExtension, ModuleResolution, ModuleResolver, PackageId, PathContext,
    PreparationDiagnostics, PreparedAuxiliaryFile, PreparedProgram, PreparedProgramBuilder,
    PreparedSourceFile, ProgramOptions, ProgramPath, ResolutionKey, ResolutionMode,
    ResolutionOutcome, ResolutionRequestKind, ResolvedModule, ResolvedModuleTarget, SourceFileId,
    TypeReferenceResolution, TypeReferenceResolutionKey,
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

fn authoritative_program(
    files: &[(&str, &str)],
    roots: &[usize],
    mut options: CompilerOptions,
    add_resolutions: impl FnOnce(&mut PreparedProgramBuilder, &[SourceFileId]),
) -> PreparedProgram {
    options.no_emit = Some(true);
    let mut builder =
        PreparedProgram::builder(PathContext::new(current_directory(), true), options);
    let lib = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add lib");
    builder.add_library_file(lib).expect("add library");
    let ids = files
        .iter()
        .map(|(name, text)| {
            builder
                .add_source_file(PreparedSourceFile::new(path(name), *text))
                .expect("add source")
        })
        .collect::<Vec<_>>();
    for &root in roots {
        builder.add_root_file(ids[root]).expect("add root");
    }
    add_resolutions(&mut builder, &ids);
    builder.build().expect("build authoritative program")
}

fn authoritative_program_with_emit_eligibility(
    files: &[(&str, &str, bool)],
    roots: &[usize],
    mut options: CompilerOptions,
    add_resolutions: impl FnOnce(&mut PreparedProgramBuilder, &[SourceFileId]),
) -> PreparedProgram {
    options.no_emit = Some(true);
    let mut builder =
        PreparedProgram::builder(PathContext::new(current_directory(), true), options);
    let lib = builder
        .add_source_file(
            PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS).with_may_be_emitted(false),
        )
        .expect("add lib");
    builder.add_library_file(lib).expect("add library");
    let ids = files
        .iter()
        .map(|(name, text, may_be_emitted)| {
            builder
                .add_source_file(
                    PreparedSourceFile::new(path(name), *text).with_may_be_emitted(*may_be_emitted),
                )
                .expect("add source")
        })
        .collect::<Vec<_>>();
    for &root in roots {
        builder.add_root_file(ids[root]).expect("add root");
    }
    add_resolutions(&mut builder, &ids);
    builder.build().expect("build authoritative program")
}

fn module_key(source: &str, specifier: &str, mode: ResolutionMode) -> ResolutionKey {
    ResolutionKey::new(path(source).canonical().clone(), specifier, mode)
}

fn source_resolution(
    source: SourceFileId,
    resolved_file: &str,
    extension: ModuleExtension,
) -> ModuleResolution {
    ModuleResolution::resolved(ResolvedModule::new(
        ResolvedModuleTarget::Source {
            source,
            resolved_file: path(resolved_file),
        },
        extension,
    ))
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
    assert_eq!(
        outcome
            .conformance_diagnostics()
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 9501 | 9502))
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [9502]
    );
}

#[test]
fn type_reference_resolution_diagnostics_join_program_diagnostics_once_in_key_order() {
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
        .add_source_file(PreparedSourceFile::new(path("/main.ts"), "export {};"))
        .expect("add source");
    builder.add_library_file(lib).expect("add library");
    builder.add_root_file(source).expect("add root");

    // Insert in the opposite order to prove the public table traversal is
    // derived from exact key order rather than discovery order.
    builder
        .add_type_reference_resolution(
            TypeReferenceResolutionKey::source(
                path("/main.ts").canonical().clone(),
                "zeta",
                ResolutionMode::Unspecified,
            ),
            Ok(
                TypeReferenceResolution::not_found().with_diagnostics(vec![located_diagnostic(
                    9602,
                    "/main.ts",
                    "missing zeta types",
                )]),
            ),
        )
        .expect("add zeta type-reference row");
    builder
        .add_type_reference_resolution(
            TypeReferenceResolutionKey::source(
                path("/main.ts").canonical().clone(),
                "alpha",
                ResolutionMode::Unspecified,
            ),
            Ok(
                TypeReferenceResolution::not_found().with_diagnostics(vec![located_diagnostic(
                    9601,
                    "/main.ts",
                    "missing alpha types",
                )]),
            ),
        )
        .expect("add alpha type-reference row");

    let prepared = builder.build().expect("build prepared program");
    let specifiers = prepared
        .resolutions()
        .type_references()
        .map(|(key, _)| key.specifier())
        .collect::<Vec<_>>();
    assert_eq!(specifiers, ["alpha", "zeta"]);

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(codes(outcome.semantic_diagnostics()), [9601, 9602]);
    assert_eq!(
        outcome
            .conformance_diagnostics()
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 9601 | 9602))
            .map(|diagnostic| diagnostic.code())
            .collect::<Vec<_>>(),
        [9601, 9602]
    );
    assert_eq!(
        outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| matches!(diagnostic.code(), 9601 | 9602))
            .count(),
        2
    );
}

#[test]
fn program_options_preserve_absent_and_explicit_types() {
    assert_eq!(ProgramOptions::default().types(), None);

    let options = ProgramOptions::default().with_types(Vec::new());
    assert_eq!(options.types(), Some([].as_slice()));

    let options = ProgramOptions::default().with_types(vec!["jquery".to_owned()]);
    assert_eq!(options.types(), Some(["jquery".to_owned()].as_slice()));
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
    assert!(outcome
        .conformance_diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.code() == 6133));
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

#[test]
fn conformance_harness_lib_cache_preserves_authoritative_diagnostics() {
    fn make_program() -> PreparedProgram {
        authoritative_program(
            &[
                ("/dep.ts", "export const value: 'actual' = 'actual';\n"),
                (
                    "/main.ts",
                    "import { value } from './dep';\nconst expected: 'other' = value;\n",
                ),
            ],
            &[1],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                ..CompilerOptions::default()
            },
            |builder, ids| {
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "./dep", ResolutionMode::Unspecified),
                        Ok(source_resolution(ids[0], "/dep.ts", ModuleExtension::Ts)),
                    )
                    .expect("add authoritative source resolution");
            },
        )
    }

    let owned = ProgramSession::new(make_program())
        .run()
        .expect("owned authoritative session");
    let cached = ProgramSession::new(make_program())
        .run_for_conformance_with_harness_lib_cache()
        .expect("cached conformance authoritative session");

    assert_eq!(owned, cached);
    assert_eq!(codes(cached.semantic_diagnostics()), [2322]);
}

#[test]
fn conformance_harness_lib_cache_preserves_authoritative_failure() {
    fn make_program() -> PreparedProgram {
        authoritative_program(
            &[("/main.cts", "import 'pkg';\n")],
            &[0],
            CompilerOptions {
                module: Some(100),
                module_resolution: Some(3),
                ..CompilerOptions::default()
            },
            |_, _| {},
        )
    }

    let owned = ProgramSession::new(make_program())
        .run()
        .expect_err("owned session must reject the missing exact row");
    let cached = ProgramSession::new(make_program())
        .run_for_conformance_with_harness_lib_cache()
        .expect_err("cached session must reject the missing exact row");

    assert_eq!(owned, cached);
    assert!(matches!(cached, DriverError::MissingResolution(_)));
}

#[test]
fn session_fails_closed_when_exact_module_resolution_key_is_absent() {
    let prepared = authoritative_program(
        &[("/main.cts", "import \"pkg\";\n")],
        &[0],
        CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
        |_, _| {},
    );

    let error = ProgramSession::new(prepared)
        .run()
        .expect_err("missing exact row must fail the session");
    let DriverError::MissingResolution(missing) = error else {
        panic!("unexpected driver error: {error:?}");
    };
    assert_eq!(missing.request_kind(), ResolutionRequestKind::Module);
    assert_eq!(missing.origin(), path("/main.cts").canonical());
    assert_eq!(missing.specifier(), "pkg");
    assert_eq!(missing.mode(), ResolutionMode::CommonJs);
}

#[test]
fn authoritative_not_found_does_not_enter_legacy_node_modules_suppression() {
    let prepared = authoritative_program(
        &[
            (
                "/node_modules/pkg/index.d.ts",
                "export const value: number;\n",
            ),
            ("/main.cts", "import { value } from \"pkg\";\nvalue;\n"),
        ],
        &[1],
        CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
        |builder, _| {
            builder
                .add_module_resolution(
                    module_key("/main.cts", "pkg", ResolutionMode::CommonJs),
                    Ok(ModuleResolution::not_found()),
                )
                .expect("add authoritative miss");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(
        outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2307)
            .count(),
        1
    );
    assert!(!codes(outcome.semantic_diagnostics()).contains(&2305));
}

#[test]
fn authoritative_not_found_does_not_fall_through_to_a_relative_probe_hit() {
    let prepared = authoritative_program(
        &[
            ("/dep.ts", "export const value = 1;\n"),
            ("/main.ts", "import { value } from \"./dep\";\nvalue;\n"),
        ],
        &[1],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, _| {
            builder
                .add_module_resolution(
                    module_key("/main.ts", "./dep", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::not_found()),
                )
                .expect("add authoritative relative miss");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(
        outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2307)
            .count(),
        1
    );
}

#[test]
fn authoritative_resolution_selects_the_recorded_source_not_the_probe_candidate() {
    let prepared = authoritative_program(
        &[
            ("/ignored.txt", "not a checker source"),
            ("/decoy.ts", "export const picked: \"heuristic\";\n"),
            (
                "/types/hidden.d.ts",
                "export const picked: \"authoritative\";\n",
            ),
            (
                "/main.ts",
                "import { picked } from \"./decoy\";\nconst mustFail: \"heuristic\" = picked;\n",
            ),
        ],
        &[3],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            builder
                .add_module_resolution(
                    module_key("/main.ts", "./decoy", ResolutionMode::Unspecified),
                    Ok(source_resolution(
                        ids[2],
                        "/types/hidden.d.ts",
                        ModuleExtension::Dts,
                    )),
                )
                .expect("add authoritative source resolution");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(
        outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2322)
            .count(),
        1
    );
    assert!(!codes(outcome.semantic_diagnostics()).contains(&2307));
    assert!(!codes(outcome.semantic_diagnostics()).contains(&2305));
}

#[test]
fn authoritative_ts_extension_fact_controls_non_relative_rewrite_diagnostic() {
    struct Case {
        name: &'static str,
        importer: &'static str,
        specifier: &'static str,
        target_name: &'static str,
        target_text: &'static str,
        resolved_using_ts_extension: bool,
        is_external_library_import: bool,
        target_may_be_emitted: bool,
        target_is_root: bool,
        expect_2877: bool,
    }

    let cases = [
        Case {
            name: "package imports pattern target",
            importer: "import {} from \"#internal/foo.ts\";\n",
            specifier: "#internal/foo.ts",
            target_name: "/internal/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: false,
            target_may_be_emitted: true,
            target_is_root: false,
            expect_2877: true,
        },
        Case {
            name: "package imports exact target",
            importer: "import {} from \"#foo.ts\";\n",
            specifier: "#foo.ts",
            target_name: "/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: false,
            is_external_library_import: false,
            target_may_be_emitted: true,
            target_is_root: false,
            expect_2877: false,
        },
        Case {
            name: "type-only import keeps the upstream module-specifier location boundary",
            importer: "import type {} from \"#internal/foo.ts\";\n",
            specifier: "#internal/foo.ts",
            target_name: "/internal/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: false,
            target_may_be_emitted: true,
            target_is_root: false,
            expect_2877: true,
        },
        Case {
            name: "literal import type",
            importer: "export type T = import(\"#internal/foo.ts\");\n",
            specifier: "#internal/foo.ts",
            target_name: "/internal/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: false,
            target_may_be_emitted: true,
            target_is_root: false,
            expect_2877: false,
        },
        Case {
            name: "ambient import",
            importer: "declare module \"ambient\" {\n  import internal = require(\"#internal/foo.ts\");\n}\n",
            specifier: "#internal/foo.ts",
            target_name: "/internal/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: false,
            target_may_be_emitted: true,
            target_is_root: false,
            expect_2877: false,
        },
        Case {
            name: "declaration target",
            importer: "import {} from \"#internal/foo.ts\";\n",
            specifier: "#internal/foo.ts",
            target_name: "/internal/foo.d.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: false,
            target_may_be_emitted: false,
            target_is_root: false,
            expect_2877: false,
        },
        Case {
            name: "external package lookup can select an emit-eligible root input",
            importer: "import {} from \"#internal/foo.ts\";\n",
            specifier: "#internal/foo.ts",
            target_name: "/node_modules/pkg/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: true,
            target_may_be_emitted: true,
            target_is_root: true,
            expect_2877: true,
        },
        Case {
            name: "non-emitted external dependency suppresses the diagnostic",
            importer: "import {} from \"#internal/foo.ts\";\n",
            specifier: "#internal/foo.ts",
            target_name: "/node_modules/pkg/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: true,
            target_may_be_emitted: false,
            target_is_root: false,
            expect_2877: false,
        },
        Case {
            name: "relative rewrite",
            importer: "import {} from \"./internal/foo.ts\";\n",
            specifier: "./internal/foo.ts",
            target_name: "/internal/foo.ts",
            target_text: "export {};\n",
            resolved_using_ts_extension: true,
            is_external_library_import: false,
            target_may_be_emitted: true,
            target_is_root: false,
            expect_2877: false,
        },
    ];

    for case in cases {
        let roots: &[usize] = if case.target_is_root { &[0, 1] } else { &[1] };
        let prepared = authoritative_program_with_emit_eligibility(
            &[
                (
                    case.target_name,
                    case.target_text,
                    case.target_may_be_emitted,
                ),
                ("/main.ts", case.importer, true),
            ],
            roots,
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                rewrite_relative_import_extensions: Some(true),
                ..CompilerOptions::default()
            },
            |builder, ids| {
                let module = ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: ids[0],
                        resolved_file: path(case.target_name),
                    },
                    if case.target_name.ends_with(".d.ts") {
                        ModuleExtension::Dts
                    } else {
                        ModuleExtension::Ts
                    },
                )
                .with_resolved_using_ts_extension(case.resolved_using_ts_extension)
                .with_external_library_import(case.is_external_library_import);
                builder
                    .add_module_resolution(
                        module_key("/main.ts", case.specifier, ResolutionMode::Unspecified),
                        Ok(ModuleResolution::resolved(module)),
                    )
                    .expect("add authoritative rewrite resolution");
            },
        );

        let owned = consume(ProgramSession::new(prepared.clone()));
        let cached = ProgramSession::new(prepared)
            .run_for_conformance_with_harness_lib_cache()
            .expect("cached authoritative rewrite session");
        assert_eq!(owned, cached, "{} cache mode", case.name);
        let outcome = owned;
        let diagnostics = outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2877)
            .collect::<Vec<_>>();
        assert_eq!(
            diagnostics.len(),
            usize::from(case.expect_2877),
            "{}",
            case.name
        );
        if case.expect_2877 {
            assert_eq!(
                diagnostics[0].message_text(),
                "This import uses a '.ts' extension to resolve to an input TypeScript file, but will not be rewritten during emit because it is not a relative path."
            );
        }
    }
}

#[test]
fn ambient_modules_keep_their_priority_around_authoritative_not_found() {
    let exact = authoritative_program(
        &[
            (
                "/types.d.ts",
                "declare module \"pkg\" { export const value: number; }\n",
            ),
            ("/main.ts", "import { value } from \"pkg\";\nvalue;\n"),
        ],
        &[1],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |_, _| {},
    );
    let exact = consume(ProgramSession::new(exact));
    assert!(!codes(exact.semantic_diagnostics()).contains(&2307));

    let pattern = authoritative_program(
        &[
            (
                "/patterns.d.ts",
                "declare module \"*.css\" { const value: string; export = value; }\n",
            ),
            (
                "/main.ts",
                "import value = require(\"./theme.css\");\nvalue;\n",
            ),
        ],
        &[1],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, _| {
            builder
                .add_module_resolution(
                    module_key("/main.ts", "./theme.css", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::not_found()),
                )
                .expect("add pattern ambient miss");
        },
    );
    let pattern = consume(ProgramSession::new(pattern));
    assert!(!codes(pattern.semantic_diagnostics()).contains(&2307));

    let pattern_over_untyped = authoritative_program(
        &[
            (
                "/patterns.d.ts",
                "declare module \"pkg-*\" { export const value: number; }\n",
            ),
            (
                "/main.ts",
                "import { value } from \"pkg-js\";\nconst checked: number = value;\n",
            ),
        ],
        &[1],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
        |builder, _| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Unloaded(path("/node_modules/pkg-js/index.js")),
                ModuleExtension::Js,
            )
            .with_external_library_import(true)
            .with_package_id(PackageId::new("pkg-js", "index.js", "1.0.0"));
            builder
                .add_module_resolution(
                    module_key("/main.ts", "pkg-js", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::resolved(module)),
                )
                .expect("add pattern-over-untyped row");
        },
    );
    let pattern_over_untyped = consume(ProgramSession::new(pattern_over_untyped));
    assert!(pattern_over_untyped.semantic_diagnostics().is_empty());
    assert!(pattern_over_untyped
        .conformance_diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.code() != 7016));
}

#[test]
fn synthetic_tslib_uses_the_same_fail_closed_authoritative_table() {
    let make_program = |include_tslib_miss: bool| {
        authoritative_program(
            &[
                ("/a.ts", "export {};\n"),
                ("/main.ts", "export * as ns from \"./a\";\n"),
            ],
            &[1],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                import_helpers: Some(true),
                ..CompilerOptions::default()
            },
            |builder, ids| {
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "./a", ResolutionMode::Unspecified),
                        Ok(source_resolution(ids[0], "/a.ts", ModuleExtension::Ts)),
                    )
                    .expect("add source row");
                if include_tslib_miss {
                    builder
                        .add_module_resolution(
                            module_key("/main.ts", "tslib", ResolutionMode::Unspecified),
                            Ok(ModuleResolution::not_found()),
                        )
                        .expect("add tslib miss");
                }
            },
        )
    };

    let error = ProgramSession::new(make_program(false))
        .run()
        .expect_err("missing synthetic tslib row must fail the session");
    let DriverError::MissingResolution(missing) = error else {
        panic!("unexpected driver error: {error:?}");
    };
    assert_eq!(missing.origin(), path("/main.ts").canonical());
    assert_eq!(missing.specifier(), "tslib");
    assert_eq!(missing.mode(), ResolutionMode::Unspecified);

    let outcome = consume(ProgramSession::new(make_program(true)));
    assert!(codes(outcome.semantic_diagnostics()).contains(&2354));
}

#[test]
fn private_emit_helpers_consume_the_authoritative_tslib_declaration_target() {
    let tslib = "export declare function __classPrivateFieldGet<T extends object, V>(receiver: T, state: any): V;\n\
                 export declare function __classPrivateFieldSet<T extends object, V>(receiver: T, state: any, value: V): V;\n";
    let main = "export class C {\n\
                    #a = 1;\n\
                    #b() { this.#c = 42; }\n\
                    set #c(v: number) { this.#a += v; }\n\
                }\n";
    let prepared = authoritative_program(
        &[
            ("/node_modules/tslib/index.d.ts", tslib),
            ("/main.ts", main),
        ],
        &[1],
        CompilerOptions {
            target: Some(2), // ScriptTarget.ES2015
            import_helpers: Some(true),
            isolated_modules: Some(true),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: ids[0],
                    resolved_file: path("/node_modules/tslib/index.d.ts"),
                },
                ModuleExtension::Dts,
            )
            .with_external_library_import(true)
            .with_package_id(PackageId::new("tslib", "index.d.ts", "1.0.0"));
            builder
                .add_module_resolution(
                    module_key("/main.ts", "tslib", ResolutionMode::EsNext),
                    Ok(ModuleResolution::resolved(module)),
                )
                .expect("add authoritative tslib row");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    let rows = outcome
        .semantic_diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2807)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                Some("/main.ts"),
                Some(main.find("this.#c").expect("private setter") as u32),
                Some("this.#c".len() as u32),
                "This syntax requires an imported helper named '__classPrivateFieldSet' with 5 parameters, which is not compatible with the one in 'tslib'. Consider upgrading your version of 'tslib'.",
            ),
            (
                Some("/main.ts"),
                Some(main.find("this.#a").expect("private getter") as u32),
                Some("this.#a".len() as u32),
                "This syntax requires an imported helper named '__classPrivateFieldGet' with 4 parameters, which is not compatible with the one in 'tslib'. Consider upgrading your version of 'tslib'.",
            ),
        ]
    );
}

#[test]
fn private_emit_helpers_use_the_source_files_static_resolution_mode() {
    let tslib = "export declare function __classPrivateFieldGet<T extends object, V>(receiver: T, state: any): V;\n";
    let main = "export class C {\n\
                    #specifier = \"pkg\";\n\
                    load() { return import(this.#specifier); }\n\
                }\n";
    let options = CompilerOptions {
        no_emit: Some(true),
        target: Some(2),            // ScriptTarget.ES2015
        module: Some(100),          // ModuleKind.Node16
        module_resolution: Some(3), // ModuleResolutionKind.Node16
        import_helpers: Some(true),
        isolated_modules: Some(true),
        ..CompilerOptions::default()
    };
    let mut builder =
        PreparedProgram::builder(PathContext::new(current_directory(), true), options);
    let lib = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add lib");
    let tslib_source = builder
        .add_source_file(PreparedSourceFile::new(
            path("/node_modules/tslib/index.d.ts"),
            tslib,
        ))
        .expect("add tslib");
    let main_source = builder
        .add_source_file(
            PreparedSourceFile::new(path("/main.ts"), main)
                .with_implied_node_format(ResolutionMode::CommonJs),
        )
        .expect("add main");
    builder.add_library_file(lib).expect("add library");
    builder.add_root_file(main_source).expect("add root");
    builder
        .add_module_resolution(
            module_key("/main.ts", "tslib", ResolutionMode::CommonJs),
            Ok(ModuleResolution::resolved(
                ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: tslib_source,
                        resolved_file: path("/node_modules/tslib/index.d.ts"),
                    },
                    ModuleExtension::Dts,
                )
                .with_external_library_import(true)
                .with_package_id(PackageId::new("tslib", "index.d.ts", "1.0.0")),
            )),
        )
        .expect("add CommonJS tslib row");

    let outcome = consume(ProgramSession::new(
        builder.build().expect("build authoritative program"),
    ));
    let rows = outcome
        .semantic_diagnostics()
        .iter()
        .filter(|diagnostic| diagnostic.code() == 2807)
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [(
            Some("/main.ts"),
            Some(main.find("this.#specifier").expect("private getter") as u32),
            Some("this.#specifier".len() as u32),
            "This syntax requires an imported helper named '__classPrivateFieldGet' with 4 parameters, which is not compatible with the one in 'tslib'. Consider upgrading your version of 'tslib'.",
        )]
    );
}

#[test]
fn prepared_implied_node_format_selects_the_exact_esnext_key() {
    let mut builder = PreparedProgram::builder(
        PathContext::new(current_directory(), true),
        CompilerOptions {
            no_emit: Some(true),
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
    );
    let lib = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add lib");
    let main = builder
        .add_source_file(
            PreparedSourceFile::new(path("/main.ts"), "import { value } from \"pkg\";\nvalue;\n")
                .with_implied_node_format(ResolutionMode::EsNext),
        )
        .expect("add main");
    builder.add_library_file(lib).expect("add library");
    builder.add_root_file(main).expect("add root");
    builder
        .add_module_resolution(
            module_key("/main.ts", "pkg", ResolutionMode::EsNext),
            Ok(ModuleResolution::not_found()),
        )
        .expect("add ESNext miss");

    let outcome = consume(ProgramSession::new(
        builder.build().expect("build prepared program"),
    ));
    assert_eq!(
        outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2307)
            .count(),
        1
    );
}

#[test]
fn prepared_effective_implied_format_selects_non_node_static_and_dynamic_keys() {
    let make_program = |effective: Option<ResolutionMode>, expected: ResolutionMode| {
        let mut builder = PreparedProgram::builder(
            PathContext::new(current_directory(), true),
            CompilerOptions {
                no_emit: Some(true),
                module: Some(99),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
        );
        let lib = builder
            .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
            .expect("add lib");
        let main = builder
            .add_source_file(
                PreparedSourceFile::new(
                    path("/node_modules/pkg/index.ts"),
                    "import 'static-pkg';\nvoid import('dynamic-pkg');\n",
                )
                .with_implied_node_formats(Some(ResolutionMode::CommonJs), effective),
            )
            .expect("add main");
        builder.add_library_file(lib).expect("add library");
        builder.add_root_file(main).expect("add root");
        for specifier in ["static-pkg", "dynamic-pkg"] {
            builder
                .add_module_resolution(
                    module_key("/node_modules/pkg/index.ts", specifier, expected),
                    Ok(ModuleResolution::not_found()),
                )
                .expect("add exact effective-format row");
        }
        builder.build().expect("build prepared program")
    };

    // An explicit CommonJS package scope overrides the otherwise-ESNext
    // module kind for both static and transformed dynamic imports.
    consume(ProgramSession::new(make_program(
        Some(ResolutionMode::CommonJs),
        ResolutionMode::CommonJs,
    )));

    // The raw Node-format default below node_modules is not effective for
    // emit without an explicit package type under a non-Node module kind.
    consume(ProgramSession::new(make_program(
        None,
        ResolutionMode::EsNext,
    )));
}

#[test]
fn empty_static_and_jsdoc_specifiers_do_not_query_the_authoritative_table() {
    let prepared = authoritative_program(
        &[
            (
                "/static.ts",
                "import '';\nexport * from '';\nimport alias = require('');\n",
            ),
            (
                "/jsdoc.js",
                "/** @import { Empty } from '' */\nexport const value = 0;\n",
            ),
        ],
        &[0, 1],
        CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(99),
            module_resolution: Some(100),
            ..CompilerOptions::default()
        },
        |_, _| {},
    );

    consume(ProgramSession::new(prepared));
}

#[test]
fn empty_dynamic_import_type_and_js_require_still_query_exact_rows() {
    for (file_name, text, mode, javascript) in [
        (
            "/dynamic.ts",
            "void import('');\n",
            ResolutionMode::EsNext,
            false,
        ),
        (
            "/import-type.ts",
            "type Imported = import('').Value;\n",
            ResolutionMode::EsNext,
            false,
        ),
        (
            "/require.js",
            "const loaded = require('');\n",
            ResolutionMode::CommonJs,
            true,
        ),
    ] {
        let prepared = authoritative_program(
            &[(file_name, text)],
            &[0],
            CompilerOptions {
                allow_js: javascript,
                check_js: javascript.then_some(true),
                module: Some(99),
                module_resolution: Some(100),
                ..CompilerOptions::default()
            },
            |_, _| {},
        );

        let error = ProgramSession::new(prepared)
            .run()
            .expect_err("retained empty request must require an exact row");
        let DriverError::MissingResolution(missing) = error else {
            panic!("unexpected driver error: {error:?}");
        };
        assert_eq!(missing.request_kind(), ResolutionRequestKind::Module);
        assert_eq!(missing.origin(), path(file_name).canonical());
        assert_eq!(missing.specifier(), "");
        assert_eq!(missing.mode(), mode);
    }
}

#[test]
fn bundler_without_package_maps_uses_unspecified_resolution_keys() {
    let prepared = authoritative_program(
        &[(
            "/main.ts",
            "import 'static-pkg';\nvoid import('dynamic-pkg');\n",
        )],
        &[0],
        CompilerOptions {
            module: Some(99),
            module_resolution: Some(100),
            resolve_package_json_exports: Some(false),
            resolve_package_json_imports: Some(false),
            ..CompilerOptions::default()
        },
        |builder, _| {
            for specifier in ["static-pkg", "dynamic-pkg"] {
                builder
                    .add_module_resolution(
                        module_key("/main.ts", specifier, ResolutionMode::Unspecified),
                        Ok(ModuleResolution::not_found()),
                    )
                    .expect("add exact Bundler-without-package-maps row");
            }
        },
    );

    consume(ProgramSession::new(prepared));
}

#[test]
fn resolution_mode_overrides_select_distinct_rows_for_the_same_request() {
    let prepared = authoritative_program(
        &[
            ("/types/cjs.d.ts", "export type Mode = \"cjs\";\n"),
            ("/types/esm.d.ts", "export type Mode = \"esm\";\n"),
            (
                "/main.mts",
                "import type { Mode as CjsMode } from \"pkg\" with { \"resolution-mode\": \"require\" };\n\
                 import type { Mode as EsmMode } from \"pkg\" with { \"resolution-mode\": \"import\" };\n\
                 declare let c: CjsMode;\n\
                 declare let e: EsmMode;\n\
                 const cOk: \"cjs\" = c;\n\
                 const eOk: \"esm\" = e;\n",
            ),
        ],
        &[2],
        CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            for (mode, target, target_path) in [
                (ResolutionMode::CommonJs, ids[0], "/types/cjs.d.ts"),
                (ResolutionMode::EsNext, ids[1], "/types/esm.d.ts"),
            ] {
                builder
                    .add_module_resolution(
                        module_key("/main.mts", "pkg", mode),
                        Ok(source_resolution(target, target_path, ModuleExtension::Dts)),
                    )
                    .expect("add mode-specific row");
            }
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert!(outcome.semantic_diagnostics().is_empty());
}

#[test]
fn jsdoc_import_resolution_mode_overrides_select_distinct_rows() {
    let prepared = authoritative_program(
        &[
            (
                "/types/import.d.mts",
                "export declare const Import: \"module\";\n",
            ),
            (
                "/types/require.d.cts",
                "export declare const Require: \"script\";\n",
            ),
            (
                "/main.js",
                concat!(
                    "/** @import { Import } from 'pkg' with { 'resolution-mode': 'import' } */\n",
                    "/** @import { Require } from 'pkg' with { 'resolution-mode': 'require' } */\n",
                    "/** @returns {Import} */\n",
                    "export function imported() { return 1; }\n",
                    "/** @returns {Require} */\n",
                    "export function required() { return 1; }\n",
                ),
            ),
        ],
        &[2],
        CompilerOptions {
            allow_js: true,
            check_js: Some(true),
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            for (mode, target, target_path, extension) in [
                (
                    ResolutionMode::EsNext,
                    ids[0],
                    "/types/import.d.mts",
                    ModuleExtension::Dmts,
                ),
                (
                    ResolutionMode::CommonJs,
                    ids[1],
                    "/types/require.d.cts",
                    ModuleExtension::Dcts,
                ),
            ] {
                builder
                    .add_module_resolution(
                        module_key("/main.js", "pkg", mode),
                        Ok(source_resolution(target, target_path, extension)),
                    )
                    .expect("add JSDoc mode-specific row");
            }
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(
        outcome
            .semantic_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 2322)
            .count(),
        2
    );
    assert!(outcome
        .semantic_diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.code() != 2305));
}

#[test]
fn require_call_in_an_esm_file_uses_the_commonjs_row() {
    let prepared = authoritative_program(
        &[
            ("/types/pkg.d.ts", "export const value: number;\n"),
            (
                "/main.mts",
                "const loaded = require(\"pkg\");\nconst mustFail: string = loaded.value;\n",
            ),
        ],
        &[1],
        CompilerOptions {
            module: Some(100),
            module_resolution: Some(3),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            builder
                .add_module_resolution(
                    module_key("/main.mts", "pkg", ResolutionMode::CommonJs),
                    Ok(source_resolution(
                        ids[0],
                        "/types/pkg.d.ts",
                        ModuleExtension::Dts,
                    )),
                )
                .expect("add CommonJS require row");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(codes(outcome.semantic_diagnostics()), [2591]);
}

#[test]
fn unsupported_authoritative_records_fail_closed_without_becoming_not_found() {
    let cases = [(
        UnsupportedAuthoritativeResolution::ResolutionDiagnostics,
        ModuleResolution::not_found().with_diagnostics(vec![located_diagnostic(
            9701,
            "/main.ts",
            "resolution diagnostic",
        )]),
    )];

    for (expected, resolution) in cases {
        let prepared = authoritative_program(
            &[("/main.ts", "import { value } from \"pkg\";\nvalue;\n")],
            &[0],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                ..CompilerOptions::default()
            },
            |builder, _| {
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                        Ok(resolution),
                    )
                    .expect("add unsupported record");
            },
        );
        let error = ProgramSession::new(prepared)
            .run()
            .expect_err("unsupported row must fail the session");
        let DriverError::AuthoritativeResolution(AuthoritativeModuleFailure::Lookup {
            source_token,
            containing_file,
            specifier,
            mode,
            failure: AuthoritativeModuleLookupFailure::Unsupported(actual),
        }) = error
        else {
            panic!("unexpected driver error: {error:?}");
        };
        assert_eq!(source_token.0, 1);
        assert_eq!(containing_file, "/main.ts");
        assert_eq!(specifier, "pkg");
        assert_eq!(mode, tsc_checker::AuthoritativeResolutionMode::Unspecified);
        assert_eq!(actual, expected);
    }
}

#[test]
fn authoritative_not_found_preserves_node10_alternate_result_chain() {
    let source = "import { pkg } from \"pkg\";\n";
    let alternate_result = "/node_modules/pkg/definitely-not-index.d.ts";
    let prepared = authoritative_program(
        &[("/index.ts", source)],
        &[0],
        CompilerOptions {
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, _| {
            builder
                .add_module_resolution(
                    module_key("/index.ts", "pkg", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::not_found().with_alternate_result(path(alternate_result))),
                )
                .expect("add authoritative alternate-result miss");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    let diagnostics = outcome.semantic_diagnostics();
    assert_eq!(codes(diagnostics), [2307]);
    let diagnostic = &diagnostics[0];
    assert_eq!(
        (
            diagnostic.file_name.as_deref(),
            diagnostic.start,
            diagnostic.length,
            diagnostic.message_text(),
        ),
        (
            Some("/index.ts"),
            Some(source.find("\"pkg\"").expect("module specifier") as u32),
            Some("\"pkg\"".len() as u32),
            "Cannot find module 'pkg' or its corresponding type declarations.",
        )
    );
    assert_eq!(diagnostic.message.next.len(), 1);
    assert_eq!(
        (
            diagnostic.message.next[0].code,
            diagnostic.message.next[0].category,
            diagnostic.message.next[0].text.as_str(),
        ),
        (
            6280,
            DiagnosticCategory::Message,
            "There are types at '/node_modules/pkg/definitely-not-index.d.ts', but this result could not be resolved under your current 'moduleResolution' setting. Consider updating to 'node16', 'nodenext', or 'bundler'.",
        )
    );
}

#[test]
fn authoritative_unloaded_javascript_preserves_implicit_any_detail_chains() {
    for (alternate, expected_tail) in [
        (None, 7058),
        (Some("/node_modules/pkg/types/foo.d.ts"), 6278),
    ] {
        let prepared = authoritative_program(
            &[("/main.mts", "import {} from \"pkg/foo\";\n")],
            &[0],
            CompilerOptions {
                module: Some(199),
                module_resolution: Some(99),
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
            |builder, _| {
                let module = ResolvedModule::new(
                    ResolvedModuleTarget::Unloaded(path("/node_modules/pkg/dist/foo.js")),
                    ModuleExtension::Js,
                )
                .with_external_library_import(true)
                .with_package_id(PackageId::new("pkg", "dist/foo.js", "1.0.0"));
                let mut resolution =
                    ModuleResolution::resolved(module).with_package_bundles_types(true);
                if let Some(alternate) = alternate {
                    resolution = resolution.with_alternate_result(path(alternate));
                }
                builder
                    .add_module_resolution(
                        module_key("/main.mts", "pkg/foo", ResolutionMode::EsNext),
                        Ok(resolution),
                    )
                    .expect("add unloaded authoritative row");
            },
        );

        let outcome = consume(ProgramSession::new(prepared));
        let diagnostics = outcome.semantic_diagnostics();
        assert_eq!(codes(diagnostics), [7016]);
        let mut chain_codes = vec![diagnostics[0].message.code];
        chain_codes.extend(
            diagnostics[0]
                .message
                .next
                .iter()
                .map(|message| message.code),
        );
        assert_eq!(chain_codes, [7016, expected_tail]);
    }
}

#[test]
fn authoritative_unloaded_javascript_keeps_suggestions_out_of_cli_output() {
    for (source_text, expect_suggestion) in [
        ("import {} from \"pkg\";\n", true),
        ("import \"pkg\";\n", false),
    ] {
        let prepared = authoritative_program(
            &[("/main.ts", source_text)],
            &[0],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                no_implicit_any: Some(false),
                ..CompilerOptions::default()
            },
            |builder, _| {
                let module = ResolvedModule::new(
                    ResolvedModuleTarget::Unloaded(path("/node_modules/pkg/index.js")),
                    ModuleExtension::Js,
                )
                .with_external_library_import(true)
                .with_package_id(PackageId::new("pkg", "index.js", "1.0.0"));
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                        Ok(ModuleResolution::resolved(module)),
                    )
                    .expect("add unloaded authoritative row");
            },
        );

        let outcome = consume(ProgramSession::new(prepared));
        assert!(outcome.semantic_diagnostics().is_empty());
        assert!(outcome
            .diagnostics()
            .all(|diagnostic| diagnostic.code() != 7016));
        let suggestions = outcome
            .conformance_diagnostics()
            .iter()
            .filter(|diagnostic| diagnostic.code() == 7016)
            .collect::<Vec<_>>();
        assert_eq!(suggestions.len(), usize::from(expect_suggestion));
        assert!(suggestions
            .iter()
            .all(|diagnostic| diagnostic.category() == DiagnosticCategory::Suggestion));
    }
}

#[test]
fn loaded_external_javascript_preserves_authoritative_package_detail_precedence() {
    for (alternate, types_package_exists, package_bundles_types, expected_tail) in [
        (Some("/types/pkg.d.ts"), true, true, 6280),
        (None, true, true, 7040),
        (None, false, true, 7058),
        (None, false, false, 7035),
    ] {
        let prepared = authoritative_program(
            &[
                ("/node_modules/pkg/index.js", "export const value = 1;\n"),
                (
                    "/main.ts",
                    "import { value } from \"pkg/subpath\";\nvalue;\n",
                ),
            ],
            &[1],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                allow_js: true,
                no_implicit_any: Some(true),
                ..CompilerOptions::default()
            },
            |builder, ids| {
                let module = ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: ids[0],
                        resolved_file: path("/node_modules/pkg/index.js"),
                    },
                    ModuleExtension::Js,
                )
                .with_external_library_import(true)
                .with_package_id(PackageId::new("pkg", "index.js", "1.0.0"));
                let mut resolution = ModuleResolution::resolved(module)
                    .with_types_package_exists(types_package_exists)
                    .with_package_bundles_types(package_bundles_types);
                if let Some(alternate) = alternate {
                    resolution = resolution.with_alternate_result(path(alternate));
                }
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "pkg/subpath", ResolutionMode::Unspecified),
                        Ok(resolution),
                    )
                    .expect("add loaded external JavaScript row");
            },
        );

        let outcome = consume(ProgramSession::new(prepared));
        assert!(outcome.semantic_diagnostics().is_empty());
        assert!(outcome
            .diagnostics()
            .all(|diagnostic| diagnostic.code() != 7016));
        let diagnostic = outcome
            .conformance_diagnostics()
            .iter()
            .find(|diagnostic| diagnostic.code() == 7016)
            .expect("loaded external JavaScript suggestion");
        assert_eq!(diagnostic.category(), DiagnosticCategory::Suggestion);
        assert_eq!(
            diagnostic
                .message
                .next
                .iter()
                .map(|message| message.code)
                .collect::<Vec<_>>(),
            [expected_tail]
        );
    }
}

#[test]
fn authoritative_untyped_module_augmentation_reports_2665() {
    let prepared = authoritative_program(
        &[(
            "/main.ts",
            "export {};\ndeclare module \"pkg\" { export const extra: number; }\n",
        )],
        &[0],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, _| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Unloaded(path("/node_modules/pkg/index.js")),
                ModuleExtension::Js,
            )
            .with_external_library_import(true)
            .with_package_id(PackageId::new("pkg", "index.js", "1.0.0"));
            builder
                .add_module_resolution(
                    module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::resolved(module)),
                )
                .expect("add untyped augmentation row");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(codes(outcome.semantic_diagnostics()), [2665]);
    assert!(outcome.semantic_diagnostics()[0]
        .message_text()
        .contains("/node_modules/pkg/index.js"));
}

#[test]
fn authoritative_relative_untyped_module_ignores_inapplicable_package_details() {
    let prepared = authoritative_program(
        &[("/main.ts", "import {} from \"./impl.js\";\n")],
        &[0],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            no_implicit_any: Some(true),
            ..CompilerOptions::default()
        },
        |builder, _| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Unloaded(path("/impl.js")),
                ModuleExtension::Js,
            )
            .with_package_id(PackageId::new("inapplicable", "impl.js", "1.0.0"));
            builder
                .add_module_resolution(
                    module_key("/main.ts", "./impl.js", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::resolved(module).with_package_bundles_types(true)),
                )
                .expect("add relative untyped row");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    let diagnostics = outcome.semantic_diagnostics();
    assert_eq!(codes(diagnostics), [7016]);
    let mut chain_codes = vec![diagnostics[0].message.code];
    chain_codes.extend(
        diagnostics[0]
            .message
            .next
            .iter()
            .map(|message| message.code),
    );
    assert_eq!(chain_codes, [7016]);
}

#[test]
fn unloaded_jsx_with_an_active_jsx_mode_reports_7016() {
    let prepared = authoritative_program(
        &[("/main.ts", "import {} from \"pkg\";\n")],
        &[0],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            no_implicit_any: Some(true),
            jsx: Some(1),
            ..CompilerOptions::default()
        },
        |builder, _| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Unloaded(path("/node_modules/pkg/index.jsx")),
                ModuleExtension::Jsx,
            )
            .with_external_library_import(true);
            builder
                .add_module_resolution(
                    module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::resolved(module)),
                )
                .expect("add unloaded JSX row");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert_eq!(codes(outcome.semantic_diagnostics()), [7016]);
}

#[test]
fn unloaded_targets_fail_closed_outside_the_unadmitted_javascript_case() {
    for (target, extension, allow_js, jsx, expected) in [
        (
            "/node_modules/pkg/index.ts",
            ModuleExtension::Ts,
            false,
            None,
            UnsupportedAuthoritativeResolution::UnloadedTargetExtension,
        ),
        (
            "/node_modules/pkg/index.js",
            ModuleExtension::Js,
            true,
            None,
            UnsupportedAuthoritativeResolution::UnloadedTargetAdmission,
        ),
        (
            "/node_modules/pkg/index.jsx",
            ModuleExtension::Jsx,
            false,
            None,
            UnsupportedAuthoritativeResolution::UnloadedJsxWithoutJsxOption,
        ),
    ] {
        let prepared = authoritative_program(
            &[("/main.ts", "import {} from \"pkg\";\n")],
            &[0],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                allow_js,
                jsx,
                ..CompilerOptions::default()
            },
            |builder, _| {
                let module =
                    ResolvedModule::new(ResolvedModuleTarget::Unloaded(path(target)), extension);
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                        Ok(ModuleResolution::resolved(module)),
                    )
                    .expect("add unloaded authoritative row");
            },
        );

        let error = ProgramSession::new(prepared)
            .run()
            .expect_err("unsupported unloaded target must fail the session");
        let DriverError::AuthoritativeResolution(AuthoritativeModuleFailure::Lookup {
            failure: AuthoritativeModuleLookupFailure::Unsupported(actual),
            ..
        }) = error
        else {
            panic!("unexpected driver error: {error:?}");
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn lossy_resolved_source_metadata_is_rejected_until_it_is_consumed() {
    let prepared = authoritative_program(
        &[("/main.ts", "import { value } from \"pkg\";\nvalue;\n")],
        &[0],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: ids[0],
                    resolved_file: path("/main.ts"),
                },
                ModuleExtension::Ts,
            )
            .with_original_path(path("/alias/main.ts"));
            builder
                .add_module_resolution(
                    module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::resolved(module)),
                )
                .expect("add metadata-bearing source row");
        },
    );
    let error = ProgramSession::new(prepared)
        .run()
        .expect_err("unconsumed resolution facts must fail closed");
    let DriverError::AuthoritativeResolution(AuthoritativeModuleFailure::Lookup {
        failure: AuthoritativeModuleLookupFailure::Unsupported(actual),
        ..
    }) = error
    else {
        panic!("unexpected driver error: {error:?}");
    };
    assert_eq!(actual, UnsupportedAuthoritativeResolution::OriginalPath);
}

#[test]
fn loaded_package_identity_is_consumed_at_the_authoritative_boundary() {
    let prepared = authoritative_program(
        &[
            (
                "/node_modules/pkg/index.d.ts",
                "export const value: number;\n",
            ),
            (
                "/main.ts",
                "import { value } from \"pkg\";\nconst checked: number = value;\n",
            ),
        ],
        &[1],
        CompilerOptions {
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            let module = ResolvedModule::new(
                ResolvedModuleTarget::Source {
                    source: ids[0],
                    resolved_file: path("/node_modules/pkg/index.d.ts"),
                },
                ModuleExtension::Dts,
            )
            .with_external_library_import(true)
            .with_package_id(PackageId::new("pkg", "index.d.ts", "1.0.0"));
            builder
                .add_module_resolution(
                    module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                    Ok(ModuleResolution::resolved(module)),
                )
                .expect("add package-identified source row");
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    assert!(outcome.semantic_diagnostics().is_empty());
}

#[test]
fn ambient_const_enum_aliases_consume_authoritative_bare_module_bindings() {
    let package = "export declare const enum E { A, B, C }\n\
                   declare global { const enum F { A, B, C } }\n";
    let a = "import { E } from \"pkg\";\n\
             import type { E as _E } from \"pkg\";\n\
             E.A;\n\
             F.A;\n";
    let b = "export { E } from \"pkg\";\n\
             export type { E as _E } from \"pkg\";\n";
    let prepared = authoritative_program(
        &[
            ("/node_modules/pkg/index.d.ts", package),
            ("/a.ts", a),
            ("/b.ts", b),
        ],
        &[1, 2],
        CompilerOptions {
            module: Some(200),
            module_resolution: Some(100),
            verbatim_module_syntax: Some(true),
            ..CompilerOptions::default()
        },
        |builder, ids| {
            for source in ["/a.ts", "/b.ts"] {
                let module = ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: ids[0],
                        resolved_file: path("/node_modules/pkg/index.d.ts"),
                    },
                    ModuleExtension::Dts,
                )
                .with_external_library_import(true)
                .with_package_id(PackageId::new("pkg", "index.d.ts", "1.0.0"));
                builder
                    .add_module_resolution(
                        module_key(source, "pkg", ResolutionMode::EsNext),
                        Ok(ModuleResolution::resolved(module)),
                    )
                    .expect("add ambient const-enum package row");
            }
        },
    );

    let outcome = consume(ProgramSession::new(prepared));
    let rows = outcome
        .semantic_diagnostics()
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.file_name.as_deref(),
                diagnostic.code(),
                diagnostic.start,
                diagnostic.length,
                diagnostic.message_text(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        [
            (
                Some("/a.ts"),
                2748,
                Some(a.find("{ E }").expect("import alias") as u32 + 2),
                Some(1),
                "Cannot access ambient const enums when 'verbatimModuleSyntax' is enabled.",
            ),
            (
                Some("/a.ts"),
                2748,
                Some(a.find("F.A").expect("global const-enum access") as u32),
                Some(1),
                "Cannot access ambient const enums when 'verbatimModuleSyntax' is enabled.",
            ),
            (
                Some("/b.ts"),
                2748,
                Some(b.find("{ E }").expect("export alias") as u32 + 2),
                Some(1),
                "Cannot access ambient const enums when 'verbatimModuleSyntax' is enabled.",
            ),
        ]
    );
}

#[test]
fn external_library_source_metadata_is_consumed_authoritatively() {
    for is_external_library_import in [false, true] {
        let prepared = authoritative_program(
            &[
                (
                    "/node_modules/pkg/index.d.ts",
                    "export const value: number;\n",
                ),
                (
                    "/main.ts",
                    "import { value } from \"pkg\";\nconst checked: number = value;\n",
                ),
            ],
            &[1],
            CompilerOptions {
                module: Some(1),
                module_resolution: Some(2),
                ..CompilerOptions::default()
            },
            |builder, ids| {
                builder
                    .add_module_resolution(
                        module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
                        Ok(ModuleResolution::resolved(
                            ResolvedModule::new(
                                ResolvedModuleTarget::Source {
                                    source: ids[0],
                                    resolved_file: path("/node_modules/pkg/index.d.ts"),
                                },
                                ModuleExtension::Dts,
                            )
                            .with_external_library_import(is_external_library_import),
                        )),
                    )
                    .expect("add external library source row");
            },
        );

        let outcome = consume(ProgramSession::new(prepared));
        assert!(outcome.semantic_diagnostics().is_empty());
    }
}

#[test]
fn memory_host_exports_resolution_feeds_the_authoritative_session_table() {
    const PACKAGE_JSON: &str = r#"{
        "name": "inner",
        "exports": {
            "./mjs/*": "./*.mjs",
            "./mjs/exclude/*": null
        }
    }"#;
    let options = CompilerOptions {
        no_emit: Some(true),
        module: Some(102),
        ..CompilerOptions::default()
    };
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", "")
        .file("/node_modules/inner/package.json", PACKAGE_JSON)
        .file(
            "/node_modules/inner/index.d.mts",
            "export const mjsSource: number;\n",
        )
        .file(
            "/node_modules/inner/exclude/index.d.mts",
            "export const mustStayBlocked: number;\n",
        )
        .build()
        .expect("build memory host");
    let mut resolver = ModuleResolver::new(&host, &options).expect("create module resolver");

    let mut builder = PreparedProgram::builder(PathContext::new(path("/"), true), options.clone());
    let lib = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add lib");
    let target = builder
        .add_source_file(
            PreparedSourceFile::new(
                path("/node_modules/inner/index.d.mts"),
                "export const mjsSource: number;\n",
            )
            .with_implied_node_format(ResolutionMode::EsNext),
        )
        .expect("add exports target");
    let main = builder
        .add_source_file(
            PreparedSourceFile::new(
                path("/index.mts"),
                concat!(
                    "import { mjsSource } from \"inner/mjs/index\";\n",
                    "import * as blocked from \"inner/mjs/exclude/index\";\n",
                    "const checked: number = mjsSource;\n",
                    "blocked;\n",
                ),
            )
            .with_implied_node_format(ResolutionMode::EsNext),
        )
        .expect("add root");
    builder.add_library_file(lib).expect("add library");
    builder.add_root_file(main).expect("add root file");

    let allowed = resolver
        .resolve(
            std::path::Path::new("/index.mts"),
            "inner/mjs/index",
            ResolutionMode::EsNext,
        )
        .expect("resolve allowed export");
    let ResolutionOutcome::Resolved(allowed) = allowed else {
        panic!("allowed export must resolve");
    };
    let allowed = allowed
        .into_resolved_module(ResolvedModuleTarget::Source {
            source: target,
            resolved_file: path("/node_modules/inner/index.d.mts"),
        })
        .expect("bind allowed target");
    builder
        .add_module_resolution(
            module_key("/index.mts", "inner/mjs/index", ResolutionMode::EsNext),
            Ok(ModuleResolution::resolved(allowed)),
        )
        .expect("add allowed row");

    let blocked = resolver
        .resolve(
            std::path::Path::new("/index.mts"),
            "inner/mjs/exclude/index",
            ResolutionMode::EsNext,
        )
        .expect("resolve blocked export");
    assert_eq!(blocked, ResolutionOutcome::NotFound);
    builder
        .add_module_resolution(
            module_key(
                "/index.mts",
                "inner/mjs/exclude/index",
                ResolutionMode::EsNext,
            ),
            Ok(ModuleResolution::not_found()),
        )
        .expect("add blocked row");

    let outcome = consume(ProgramSession::new(
        builder.build().expect("build prepared program"),
    ));
    assert_eq!(codes(outcome.semantic_diagnostics()), [2307]);
}

#[test]
fn physical_resolved_file_identity_is_not_silently_replaced_by_source_name() {
    let mut builder = PreparedProgram::builder(
        PathContext::new(current_directory(), true),
        CompilerOptions {
            no_emit: Some(true),
            module: Some(1),
            module_resolution: Some(2),
            ..CompilerOptions::default()
        },
    );
    let lib = builder
        .add_source_file(PreparedSourceFile::new(path("/lib.d.ts"), MINIMAL_GLOBALS))
        .expect("add lib");
    let target = builder
        .add_source_file(
            PreparedSourceFile::new(path("/lexical/pkg.ts"), "export const value = 1;\n")
                .with_real_path(path("/physical/pkg.ts")),
        )
        .expect("add target");
    let main = builder
        .add_source_file(PreparedSourceFile::new(
            path("/main.ts"),
            "import { value } from \"pkg\";\nvalue;\n",
        ))
        .expect("add main");
    builder.add_library_file(lib).expect("add library");
    builder.add_root_file(main).expect("add root");
    builder
        .add_module_resolution(
            module_key("/main.ts", "pkg", ResolutionMode::Unspecified),
            Ok(ModuleResolution::resolved(
                ResolvedModule::new(
                    ResolvedModuleTarget::Source {
                        source: target,
                        resolved_file: path("/physical/pkg.ts"),
                    },
                    ModuleExtension::Ts,
                )
                .with_original_path(path("/lexical/pkg.ts")),
            )),
        )
        .expect("add physical source row");

    let error = ProgramSession::new(builder.build().expect("build prepared program"))
        .run()
        .expect_err("physical resolution identity must not be discarded");
    assert!(matches!(
        error,
        DriverError::AuthoritativeResolution(AuthoritativeModuleFailure::Lookup {
            failure: AuthoritativeModuleLookupFailure::Unsupported(
                UnsupportedAuthoritativeResolution::ResolvedFileIdentity
            ),
            ..
        })
    ));
}
