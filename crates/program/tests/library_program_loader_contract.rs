use std::path::{Path, PathBuf};

use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_program, plan_source_requests, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadError, ProgramLoadErrorKind, ProgramLoadLimit, ProgramLoadLimits,
    ProgramLoadOperation, ProgramOptions, ResolutionOutcome,
};

const LIBRARY_DIRECTORY: &str = "/typescript/lib";
const GENEROUS_LIMIT: usize = 1_048_576;

fn catalog() -> LibraryCatalog {
    LibraryCatalog::typescript_6_0_3(LIBRARY_DIRECTORY)
}

fn compiler_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    }
}

fn program_options() -> ProgramOptions {
    ProgramOptions::default().with_types(Vec::new())
}

fn limits(
    max_source_files: usize,
    max_request_edges: usize,
    max_source_depth: usize,
    max_source_file_bytes: usize,
    max_total_source_bytes: usize,
) -> ProgramLoadLimits {
    ProgramLoadLimits::new(
        max_source_files,
        max_request_edges,
        max_source_depth,
        max_source_file_bytes,
        max_total_source_bytes,
    )
}

fn generous_limits() -> ProgramLoadLimits {
    limits(
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
    )
}

fn load(
    host: &dyn CompilerHost,
    roots: &[&str],
    options: CompilerOptions,
    load_limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    let roots = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
    load_program(
        host,
        &roots,
        options,
        program_options(),
        &catalog(),
        load_limits,
    )
}

fn source_paths(program: &PreparedProgram) -> Vec<&Path> {
    program
        .source_files()
        .iter()
        .map(|source| source.path().display())
        .collect()
}

fn library_paths(program: &PreparedProgram) -> Vec<&Path> {
    program
        .library_files()
        .iter()
        .map(|&source| {
            program
                .source_file(source)
                .expect("library id belongs to the program")
                .path()
                .display()
        })
        .collect()
}

fn assert_library_prefix(program: &PreparedProgram, expected: &[&str]) {
    assert_eq!(
        program
            .library_files()
            .iter()
            .map(|source| source.index())
            .collect::<Vec<_>>(),
        (0..expected.len()).collect::<Vec<_>>()
    );
    assert_eq!(
        library_paths(program),
        expected.iter().map(Path::new).collect::<Vec<_>>()
    );
}

fn assert_limit_error(
    error: ProgramLoadError,
    expected_operation: ProgramLoadOperation,
    expected_limit: ProgramLoadLimit,
    expected_path: &Path,
    expected_maximum: usize,
    expected_observed: usize,
) {
    assert_eq!(error.kind(), ProgramLoadErrorKind::ResourceLimit);
    assert_eq!(error.operation(), expected_operation);
    let exceeded = error
        .limit_exceeded()
        .expect("resource-limit error carries structured evidence");
    assert_eq!(exceeded.limit(), expected_limit);
    assert_eq!(exceeded.path(), Some(expected_path));
    assert_eq!(exceeded.maximum(), expected_maximum);
    assert_eq!(exceeded.observed(), expected_observed);
}

#[test]
fn default_target_library_graph_is_a_sorted_prefix_before_dependency_postorder() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"import './dependency';\n".to_vec())
        .file(
            "/work/dependency.ts",
            b"import './leaf';\nexport {};\n".to_vec(),
        )
        .file("/work/leaf.ts", b"export {};\n".to_vec())
        .file(
            "/typescript/lib/lib.es6.d.ts",
            concat!(
                "/// <reference lib=\"dom\" />\n",
                "/// <reference lib=\"es5\" />\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file(
            "/typescript/lib/lib.dom.d.ts",
            b"/// <reference lib=\"es2015\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es2015.d.ts",
            b"/// <reference lib=\"es5\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const es5: true;\n".to_vec(),
        )
        .build()
        .expect("build fake default-library graph");
    let options = CompilerOptions {
        target: Some(2),
        ..compiler_options()
    };

    let program = load(&host, &["/work/root.ts"], options, generous_limits())
        .expect("load ES2015 default-library graph");

    let expected_libraries = [
        "/typescript/lib/lib.es6.d.ts",
        "/typescript/lib/lib.es5.d.ts",
        "/typescript/lib/lib.es2015.d.ts",
        "/typescript/lib/lib.dom.d.ts",
    ];
    assert_library_prefix(&program, &expected_libraries);
    assert_eq!(
        source_paths(&program),
        [
            "/typescript/lib/lib.es6.d.ts",
            "/typescript/lib/lib.es5.d.ts",
            "/typescript/lib/lib.es2015.d.ts",
            "/typescript/lib/lib.dom.d.ts",
            "/work/leaf.ts",
            "/work/dependency.ts",
            "/work/root.ts",
        ]
        .into_iter()
        .map(Path::new)
        .collect::<Vec<_>>()
    );
    assert!(program.diagnostics().program().is_empty());
}

#[test]
fn absent_target_selects_es2025_full_and_explicit_empty_suppresses_default() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/typescript/lib/lib.es2025.full.d.ts",
            b"declare const latestStandard: true;\n".to_vec(),
        )
        .build()
        .expect("build absent-target host");

    let defaulted = load(
        &host,
        &["/work/root.ts"],
        compiler_options(),
        generous_limits(),
    )
    .expect("absent target computes the ES2025 default library");
    assert_library_prefix(&defaulted, &["/typescript/lib/lib.es2025.full.d.ts"]);
    assert_eq!(
        source_paths(&defaulted),
        ["/typescript/lib/lib.es2025.full.d.ts", "/work/root.ts",]
            .into_iter()
            .map(Path::new)
            .collect::<Vec<_>>()
    );

    let suppressed = load(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            lib: Some(Vec::new()),
            ..compiler_options()
        },
        generous_limits(),
    )
    .expect("an explicit empty lib list suppresses the computed default");
    assert!(suppressed.library_files().is_empty());
    assert_eq!(source_paths(&suppressed), [Path::new("/work/root.ts")]);
    assert!(suppressed.diagnostics().program().is_empty());
}

#[test]
fn explicit_lowercase_raw_lib_names_expand_deduplicate_and_sort() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/typescript/lib/lib.dom.d.ts",
            b"/// <reference lib=\"ES2015\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es2015.d.ts",
            b"/// <reference lib=\"es5\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const es5: true;\n".to_vec(),
        )
        .build()
        .expect("build explicit-library graph");
    let options = CompilerOptions {
        lib: Some(vec![
            "dom".to_owned(),
            "es5".to_owned(),
            "dom".to_owned(),
            "es6".to_owned(),
            "es2015".to_owned(),
        ]),
        ..compiler_options()
    };

    let program = load(&host, &["/work/root.ts"], options, generous_limits())
        .expect("expand and deduplicate explicit libraries");

    assert_library_prefix(
        &program,
        &[
            "/typescript/lib/lib.es5.d.ts",
            "/typescript/lib/lib.es2015.d.ts",
            "/typescript/lib/lib.dom.d.ts",
        ],
    );
    assert_eq!(
        source_paths(&program),
        [
            "/typescript/lib/lib.es5.d.ts",
            "/typescript/lib/lib.es2015.d.ts",
            "/typescript/lib/lib.dom.d.ts",
            "/work/root.ts",
        ]
        .into_iter()
        .map(Path::new)
        .collect::<Vec<_>>()
    );
    assert!(program.diagnostics().program().is_empty());
}

#[test]
fn lib_reference_misses_route_exact_diagnostics() {
    let source = concat!(
        "/// <reference lib=\"es2025.promis\" />\n",
        "/// <reference lib=\"wat\" />\n",
        "export {};\n",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", source.as_bytes().to_vec())
        .build()
        .expect("build unknown-lib-reference host");
    let options = CompilerOptions {
        lib: Some(Vec::new()),
        ..compiler_options()
    };

    let program = load(&host, &["/work/root.ts"], options, generous_limits())
        .expect("unknown lib references become diagnostics");
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 2);

    let expected = [
        (
            2727,
            "es2025.promis",
            "Cannot find lib definition for 'es2025.promis'. Did you mean 'es2025.promise'?",
        ),
        (2726, "wat", "Cannot find lib definition for 'wat'."),
    ];
    for (diagnostic, (code, value, message)) in diagnostics.iter().zip(expected) {
        assert_eq!(diagnostic.code(), code);
        assert_eq!(diagnostic.file_name.as_deref(), Some("/work/root.ts"));
        assert_eq!(
            diagnostic.start,
            Some(source.find(value).expect("fixture contains lib value") as u32)
        );
        assert_eq!(diagnostic.length, Some(value.encode_utf16().count() as u32));
        assert_eq!(diagnostic.message_text(), message);
        assert!(!diagnostic.message.next_present);
    }
    assert!(program.library_files().is_empty());
}

#[test]
fn mapped_missing_lib_reference_is_located_but_missing_selected_roots_are_fileless() {
    let source = "/// <reference lib=\"es5\" />\nexport {};\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", source.as_bytes().to_vec())
        .build()
        .expect("build missing-library host");
    let options = CompilerOptions {
        lib: Some(vec!["dom".to_owned(), "dom".to_owned()]),
        ..compiler_options()
    };

    let program = load(&host, &["/work/root.ts"], options, generous_limits())
        .expect("missing mapped libraries become TS6053 diagnostics");
    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 2);

    let reference = &diagnostics[0];
    assert_eq!(reference.code(), 6053);
    assert_eq!(reference.file_name.as_deref(), Some("/work/root.ts"));
    assert_eq!(reference.start, Some(source.find("es5").unwrap() as u32));
    assert_eq!(reference.length, Some(3));
    assert_eq!(
        reference.message_text(),
        "File '/typescript/lib/lib.es5.d.ts' not found."
    );
    assert!(!reference.message.next_present);

    let explicit = &diagnostics[1];
    assert_eq!(explicit.code(), 6053);
    assert_eq!(explicit.file_name, None);
    assert_eq!(explicit.start, None);
    assert_eq!(explicit.length, None);
    assert_eq!(
        explicit.message_text(),
        "File '/typescript/lib/lib.dom.d.ts' not found."
    );
    assert!(explicit.message.next_present);
    assert_eq!(explicit.message.next.len(), 1);
    assert_eq!(explicit.message.next[0].code, 1430);
    assert!(explicit.message.next[0].next_present);
    assert_eq!(explicit.message.next[0].next.len(), 1);
    assert_eq!(explicit.message.next[0].next[0].code, 1422);
    assert_eq!(
        explicit.message.next[0].next[0].text,
        "Library 'lib.dom.d.ts' specified in compilerOptions"
    );

    let default_host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .build()
        .expect("build missing-default-library host");
    let defaulted = load(
        &default_host,
        &["/work/root.ts"],
        compiler_options(),
        generous_limits(),
    )
    .expect("a missing default library is also a program diagnostic");
    let diagnostics = defaulted.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    let default = &diagnostics[0];
    assert_eq!(default.code(), 6053);
    assert_eq!(default.file_name, None);
    assert_eq!(default.start, None);
    assert_eq!(default.length, None);
    assert_eq!(
        default.message_text(),
        "File '/typescript/lib/lib.es2025.full.d.ts' not found."
    );
    assert!(default.message.next_present);
    assert_eq!(default.message.next.len(), 1);
    assert_eq!(default.message.next[0].code, 1430);
    assert!(default.message.next[0].next_present);
    assert_eq!(default.message.next[0].next.len(), 1);
    assert_eq!(default.message.next[0].next[0].code, 1425);
    assert_eq!(
        default.message.next[0].next[0].text,
        "Default library for target 'es2025'"
    );
}

#[test]
fn a_missing_selected_library_replaces_the_same_missing_root_reason() {
    let host = MemoryCompilerHost::builder("/work")
        .build()
        .expect("build missing-root/library overlap host");
    let program = load(
        &host,
        &["/typescript/lib/lib.dom.d.ts"],
        CompilerOptions {
            lib: Some(vec!["dom".to_owned()]),
            ..compiler_options()
        },
        generous_limits(),
    )
    .expect("the later selected-library reason replaces the root reason");

    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code(), 6053);
    assert_eq!(diagnostics[0].message.next[0].next[0].code, 1422);
    assert_eq!(
        program.roots()[0].missing_diagnostic(),
        Some(&diagnostics[0])
    );
}

#[test]
fn library_self_reference_produces_a_located_ts1006() {
    let lib_text = "/// <reference lib=\"es5\" />\ndeclare const es5: true;\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file("/typescript/lib/lib.es5.d.ts", lib_text.as_bytes().to_vec())
        .build()
        .expect("build self-referencing library host");
    let program = load(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            lib: Some(vec!["es5".to_owned()]),
            ..compiler_options()
        },
        generous_limits(),
    )
    .expect("a library self-reference is a program diagnostic");

    let diagnostics = program.diagnostics().program();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code(), 1006);
    assert_eq!(
        diagnostics[0].file_name.as_deref(),
        Some("/typescript/lib/lib.es5.d.ts")
    );
    assert_eq!(
        diagnostics[0].start,
        Some(lib_text.find("es5").unwrap() as u32)
    );
    assert_eq!(diagnostics[0].length, Some(3));
    assert_eq!(
        diagnostics[0].message_text(),
        "A file cannot have a reference to itself."
    );
}

#[test]
fn compiler_option_lib_keys_fail_closed_outside_the_lowercase_raw_contract() {
    let host = MemoryCompilerHost::builder("/work")
        .build()
        .expect("build validation-only host");
    let roots = [PathBuf::from("/work/root.ts")];

    for value in ["DOM", "lib.dom.d.ts"] {
        let error = load_program(
            &host,
            &roots,
            CompilerOptions {
                no_emit: Some(true),
                lib: Some(vec![value.to_owned()]),
                ..CompilerOptions::default()
            },
            program_options(),
            &catalog(),
            generous_limits(),
        )
        .expect_err("non-raw lib spellings must fail before host discovery");
        assert_eq!(error.kind(), ProgramLoadErrorKind::InvalidInput);
        assert_eq!(error.operation(), ProgramLoadOperation::ValidateOptions);
        let ProgramLoadError::InvalidInput { detail, .. } = error else {
            unreachable!("kind identifies the invalid-input variant");
        };
        assert!(detail.contains(value));
    }
}

#[test]
fn library_prefix_publication_remaps_roots_module_and_type_targets() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference types=\"pkg\" />\n",
                "import './dependency';\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file("/work/dependency.ts", b"export {};\n".to_vec())
        .file(
            "/work/node_modules/@types/pkg/index.d.ts",
            b"declare const fromTypes: true;\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const es5: true;\n".to_vec(),
        )
        .build()
        .expect("build publication-remap host");
    let program = load(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            lib: Some(vec!["es5".to_owned()]),
            ..compiler_options()
        },
        generous_limits(),
    )
    .expect("publish library prefix and remap graph identities");

    assert_eq!(
        source_paths(&program),
        [
            "/typescript/lib/lib.es5.d.ts",
            "/work/node_modules/@types/pkg/index.d.ts",
            "/work/dependency.ts",
            "/work/root.ts",
        ]
        .into_iter()
        .map(Path::new)
        .collect::<Vec<_>>()
    );
    assert_eq!(program.roots()[0].source().unwrap().index(), 3);

    let root = &program.source_files()[3];
    let plan = plan_source_requests(root, program.compiler_options())
        .expect("re-plan the published root's exact keys");
    let module = program
        .resolutions()
        .require_module(&plan.module_requests()[0])
        .expect("module row survives remapping");
    let ResolutionOutcome::Resolved(module) = module.outcome() else {
        panic!("dependency module resolves");
    };
    assert_eq!(module.target().source().unwrap().index(), 2);

    let type_reference = program
        .resolutions()
        .require_type_reference(plan.type_reference_directives()[0].key())
        .expect("type-reference row survives remapping");
    let ResolutionOutcome::Resolved(type_reference) = type_reference.outcome() else {
        panic!("type reference resolves");
    };
    assert_eq!(type_reference.source().index(), 1);
}

#[test]
fn lib_phase_precedes_module_resolution_and_descends_sequentially() {
    let nested_lib_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/typescript/lib/lib.es2015.d.ts")),
        "the first lib directive descends before the next phase step",
    );
    let later_lib_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/typescript/lib/lib.dom.d.ts")),
        "the second lib directive must not win",
    );
    let module_resolution = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/module.ts")),
        "module resolution must follow the complete lib phase",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            concat!(
                "/// <reference lib=\"es5\" />\n",
                "/// <reference lib=\"dom\" />\n",
                "import './module';\n",
            )
            .as_bytes()
            .to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"/// <reference lib=\"es2015\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es2015.d.ts",
            b"declare const nested: true;\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.dom.d.ts",
            b"declare const later: true;\n".to_vec(),
        )
        .file("/work/module.ts", b"export {};\n".to_vec())
        .failure(nested_lib_read.clone())
        .failure(later_lib_read)
        .failure(module_resolution)
        .build()
        .expect("build library-phase precedence host");
    let options = CompilerOptions {
        lib: Some(Vec::new()),
        ..compiler_options()
    };

    let error = load(&host, &["/work/root.ts"], options, generous_limits())
        .expect_err("the nested first-lib read wins before later lib and module operations");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Host);
    assert_eq!(error.operation(), ProgramLoadOperation::ReadSource);
    assert_eq!(
        error.path(),
        Some(Path::new("/typescript/lib/lib.es2015.d.ts"))
    );
    let ProgramLoadError::Host { source, .. } = error else {
        unreachable!("kind identifies the host variant");
    };
    assert_eq!(source, nested_lib_read);
}

#[test]
fn path_references_from_library_order_sources_fail_closed_before_descending() {
    let child_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/typescript/lib/lib.es5.d.ts")),
        "the unsupported child must not be read",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/typescript/lib/lib.dom.d.ts",
            b"/// <reference path=\"./lib.es5.d.ts\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const pathChild: true;\n".to_vec(),
        )
        .failure(child_read)
        .build()
        .expect("build unsupported default-library path-reference host");
    let error = load(
        &host,
        &["/work/root.ts"],
        CompilerOptions {
            lib: Some(vec!["dom".to_owned()]),
            ..compiler_options()
        },
        generous_limits(),
    )
    .expect_err("the current checker prefix cannot encode the two upstream sets");

    assert_eq!(error.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(error.operation(), ProgramLoadOperation::PlanSourceRequests);
    assert_eq!(
        error.path(),
        Some(Path::new("/typescript/lib/lib.dom.d.ts"))
    );
    let ProgramLoadError::Unsupported { feature, .. } = error else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "default-library-path-references");
}

#[test]
fn ordinary_library_identity_collision_fails_closed() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/root.ts",
            b"/// <reference path=\"/typescript/lib/lib.es5.d.ts\" />\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const ordinaryFirst: true;\n".to_vec(),
        )
        .build()
        .expect("build source-class collision host");
    let options = CompilerOptions {
        lib: Some(vec!["es5".to_owned()]),
        ..compiler_options()
    };

    let error = load(&host, &["/work/root.ts"], options, generous_limits())
        .expect_err("one canonical source cannot be both ordinary and library input");
    assert_eq!(error.kind(), ProgramLoadErrorKind::Unsupported);
    assert_eq!(error.operation(), ProgramLoadOperation::ReadSource);
    assert_eq!(
        error.path(),
        Some(Path::new("/typescript/lib/lib.es5.d.ts"))
    );
    let ProgramLoadError::Unsupported {
        feature, detail, ..
    } = error
    else {
        unreachable!("kind identifies the unsupported variant");
    };
    assert_eq!(feature, "library-source-classification-collision");
    assert!(detail.contains("Ordinary"));
    assert!(detail.contains("Library"));
}

#[test]
fn empty_roots_do_not_load_default_or_explicit_libraries() {
    let default_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/typescript/lib/lib.es2025.full.d.ts")),
        "empty programs must not read the computed default library",
    );
    let explicit_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/typescript/lib/lib.es5.d.ts")),
        "empty programs must not read explicit libraries",
    );
    let host = MemoryCompilerHost::builder("/work")
        .failure(default_read)
        .failure(explicit_read)
        .build()
        .expect("build empty-program host");

    let defaulted = load(&host, &[], compiler_options(), generous_limits())
        .expect("an empty program skips its computed default library");
    assert!(defaulted.source_files().is_empty());
    assert!(defaulted.library_files().is_empty());
    assert!(defaulted.roots().is_empty());
    assert!(defaulted.diagnostics().program().is_empty());

    let explicit = load(
        &host,
        &[],
        CompilerOptions {
            lib: Some(vec!["es5".to_owned()]),
            ..compiler_options()
        },
        generous_limits(),
    )
    .expect("an empty program also skips explicitly selected libraries");
    assert!(explicit.source_files().is_empty());
    assert!(explicit.library_files().is_empty());
    assert!(explicit.roots().is_empty());
    assert!(explicit.diagnostics().program().is_empty());
}

#[test]
fn library_sources_and_references_count_toward_every_resource_limit() {
    let lib_text = "/// <reference lib=\"dom\" />\ndeclare const es5: true;\n";
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", Vec::new())
        .file("/typescript/lib/lib.es5.d.ts", lib_text.as_bytes().to_vec())
        .file(
            "/typescript/lib/lib.dom.d.ts",
            b"declare const dom: true;\n".to_vec(),
        )
        .build()
        .expect("build resource-limit library graph");
    let options = CompilerOptions {
        lib: Some(vec!["es5".to_owned()]),
        ..compiler_options()
    };

    let error = load(
        &host,
        &["/work/root.ts"],
        options.clone(),
        limits(
            1,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    )
    .expect_err("the selected library is the second loaded source");
    assert_limit_error(
        error,
        ProgramLoadOperation::ReadSource,
        ProgramLoadLimit::SourceFiles,
        Path::new("/typescript/lib/lib.es5.d.ts"),
        1,
        2,
    );

    let error = load(
        &host,
        &["/work/root.ts"],
        options.clone(),
        limits(
            GENEROUS_LIMIT,
            0,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    )
    .expect_err("the selected library's lib reference consumes one edge");
    assert_limit_error(
        error,
        ProgramLoadOperation::PlanSourceRequests,
        ProgramLoadLimit::RequestEdges,
        Path::new("/typescript/lib/lib.es5.d.ts"),
        0,
        1,
    );

    let error = load(
        &host,
        &["/work/root.ts"],
        options.clone(),
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            0,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
        ),
    )
    .expect_err("a library reached from another library is at depth one");
    assert_limit_error(
        error,
        ProgramLoadOperation::ReadSource,
        ProgramLoadLimit::SourceDepth,
        Path::new("/typescript/lib/lib.dom.d.ts"),
        0,
        1,
    );

    let error = load(
        &host,
        &["/work/root.ts"],
        options.clone(),
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            lib_text.len() - 1,
            GENEROUS_LIMIT,
        ),
    )
    .expect_err("library bytes are subject to the per-source limit");
    assert_limit_error(
        error,
        ProgramLoadOperation::ReadSource,
        ProgramLoadLimit::SourceFileBytes,
        Path::new("/typescript/lib/lib.es5.d.ts"),
        lib_text.len() - 1,
        lib_text.len(),
    );

    let error = load(
        &host,
        &["/work/root.ts"],
        options,
        limits(
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            GENEROUS_LIMIT,
            lib_text.len() - 1,
        ),
    )
    .expect_err("library bytes are included in the aggregate limit");
    assert_limit_error(
        error,
        ProgramLoadOperation::ReadSource,
        ProgramLoadLimit::TotalSourceBytes,
        Path::new("/typescript/lib/lib.es5.d.ts"),
        lib_text.len() - 1,
        lib_text.len(),
    );
}
