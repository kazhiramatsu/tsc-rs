use std::path::{Path, PathBuf};

use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    load_no_lib_program, load_program, CompilerOptions, LibraryCatalog, PreparedProgram,
    ProgramLoadError, ProgramLoadErrorKind, ProgramLoadLimit, ProgramLoadLimits,
    ProgramLoadOperation, ProgramOptions, ProgramPath, ResolutionError, ResolutionOutcome,
    TypeReferenceResolution, TypeReferenceResolutionKey,
};

const LIBRARY_DIRECTORY: &str = "/typescript/lib";
const GENEROUS_LIMIT: usize = 1_048_576;

fn compiler_options() -> CompilerOptions {
    CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    }
}

fn path(value: &str) -> ProgramPath {
    ProgramPath::from_trusted_parts(value, value).expect("construct case-sensitive program path")
}

fn limits(max_request_edges: usize) -> ProgramLoadLimits {
    ProgramLoadLimits::new(
        GENEROUS_LIMIT,
        max_request_edges,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
        GENEROUS_LIMIT,
    )
}

fn generous_limits() -> ProgramLoadLimits {
    limits(GENEROUS_LIMIT)
}

fn load_no_lib(
    host: &dyn CompilerHost,
    roots: &[&str],
    program_options: ProgramOptions,
    load_limits: ProgramLoadLimits,
) -> Result<PreparedProgram, ProgramLoadError> {
    load_no_lib_program(
        host,
        &roots.iter().map(PathBuf::from).collect::<Vec<_>>(),
        compiler_options(),
        program_options,
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

fn automatic_key(containing_file: &str, name: &str) -> TypeReferenceResolutionKey {
    TypeReferenceResolutionKey::automatic(path(containing_file).canonical().clone(), name)
}

fn automatic_resolution<'a>(
    program: &'a PreparedProgram,
    containing_file: &str,
    name: &str,
) -> &'a TypeReferenceResolution {
    program
        .resolutions()
        .require_type_reference(&automatic_key(containing_file, name))
        .expect("automatic type-reference row is authoritative")
}

fn assert_missing_reason(
    resolution: &TypeReferenceResolution,
    expected_occurrences: usize,
    expected_name: &str,
    expected_reason_code: u32,
    expected_reason: &str,
) {
    assert_eq!(resolution.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(resolution.diagnostics().len(), expected_occurrences);
    for diagnostic in resolution.diagnostics() {
        assert_eq!(diagnostic.file_name, None);
        assert_eq!(diagnostic.start, None);
        assert_eq!(diagnostic.length, None);
        assert_eq!(diagnostic.code(), 2688);
        assert_eq!(
            diagnostic.message.text,
            format!("Cannot find type definition file for '{expected_name}'.")
        );
        let [inclusion] = diagnostic.message.next.as_slice() else {
            panic!("TS2688 must carry one inclusion chain");
        };
        assert_eq!(inclusion.code, 1430);
        assert_eq!(inclusion.text, "The file is in the program because:");
        let [reason] = inclusion.next.as_slice() else {
            panic!("the inclusion chain must carry one automatic-types reason");
        };
        assert_eq!(reason.code, expected_reason_code);
        assert_eq!(reason.text, expected_reason);
    }
}

#[test]
fn absent_empty_and_requested_root_states_gate_automatic_types_exactly() {
    let unexpected_discovery = HostError::new(
        HostErrorKind::Other,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work/node_modules/@types")),
        "absent and empty types must not inspect default type roots",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .failure(unexpected_discovery)
        .build()
        .expect("build no-automatic-types host");

    for options in [
        ProgramOptions::default().with_no_lib(true),
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new()),
    ] {
        let program = load_no_lib(&host, &["/work/root.ts"], options, generous_limits())
            .expect("absent and empty types both remain no-op states");
        assert_eq!(program.resolutions().type_reference_len(), 0);
        assert_eq!(source_paths(&program), [Path::new("/work/root.ts")]);
    }

    let explicit_missing = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(vec!["missing".to_owned()])
        .with_type_roots(Vec::new());
    let empty = load_no_lib(&host, &[], explicit_missing.clone(), generous_limits())
        .expect("an empty requested root list suppresses explicit automatic types");
    assert!(empty.roots().is_empty());
    assert_eq!(empty.resolutions().type_reference_len(), 0);

    let missing_root = load_no_lib(
        &host,
        &["/work/missing.ts"],
        explicit_missing,
        generous_limits(),
    )
    .expect("one requested but missing root still runs automatic types");
    assert_eq!(missing_root.roots().len(), 1);
    assert!(missing_root.roots()[0].source().is_none());
    assert_eq!(missing_root.diagnostics().program().len(), 1);
    assert_missing_reason(
        automatic_resolution(&missing_root, "/work/__inferred type names__.ts", "missing"),
        1,
        "missing",
        1417,
        "Entry point of type library 'missing' specified in compilerOptions",
    );
}

#[test]
fn wildcard_expansion_preserves_root_and_host_order_filters_and_stably_deduplicates() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/types/z-root/.hidden/package.json",
            br#"{"types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/types/z-root/.hidden/index.d.ts",
            b"declare const hidden: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/alpha/index.d.ts",
            b"declare const alpha: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/dup/index.d.ts",
            b"declare const firstDup: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/post/index.d.ts",
            b"declare const post: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/pre/index.d.ts",
            b"declare const pre: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/skip/package.json",
            br#"{/* JSONC */"typings":null,}"#.to_vec(),
        )
        .file(
            "/types/z-root/skip/index.d.ts",
            b"declare const skipped: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/proto-skip/package.json",
            br#"{/* force convertToJson */"__proto__":{"typings":null},}"#.to_vec(),
        )
        .file(
            "/types/z-root/proto-skip/index.d.ts",
            b"declare const inheritedSkipped: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/types-null/package.json",
            br#"{"name":"types-null","version":"1.0.0","types":null}"#.to_vec(),
        )
        .file(
            "/types/z-root/types-null/index.d.ts",
            b"declare const typesNull: true;\n".to_vec(),
        )
        .file(
            "/types/z-root/invalid/package.json",
            br#"{"typings":null,"nested":{unquoted:true}}"#.to_vec(),
        )
        .file(
            "/types/z-root/invalid/index.d.ts",
            b"declare const invalidManifest: true;\n".to_vec(),
        )
        .file("/types/z-root/README.txt", b"not a package\n".to_vec())
        .file(
            "/types/a-root/beta/index.d.ts",
            b"declare const beta: true;\n".to_vec(),
        )
        .file(
            "/types/a-root/dup/index.d.ts",
            b"declare const secondDup: true;\n".to_vec(),
        )
        .build()
        .expect("build wildcard type-root graph");
    let options = ProgramOptions::default()
        .with_no_lib(true)
        .with_types(vec![
            "pre".to_owned(),
            "*".to_owned(),
            "dup".to_owned(),
            "*".to_owned(),
            "post".to_owned(),
        ])
        .with_type_roots(vec![path("/types/z-root"), path("/types/a-root")]);

    let program = load_no_lib(&host, &["/work/root.ts"], options, generous_limits())
        .expect("load stable wildcard expansion");

    assert_eq!(
        source_paths(&program),
        [
            "/work/root.ts",
            "/types/z-root/pre/index.d.ts",
            "/types/z-root/alpha/index.d.ts",
            "/types/z-root/dup/index.d.ts",
            "/types/z-root/invalid/index.d.ts",
            "/types/z-root/post/index.d.ts",
            "/types/z-root/types-null/index.d.ts",
            "/types/a-root/beta/index.d.ts",
        ]
        .into_iter()
        .map(Path::new)
        .collect::<Vec<_>>()
    );
    assert_eq!(program.resolutions().type_reference_len(), 7);
    assert!(source_paths(&program)
        .iter()
        .all(|source| !source.to_string_lossy().contains(".hidden")));
    assert!(source_paths(&program)
        .iter()
        .all(|source| !source.to_string_lossy().contains("/skip/")));
    assert!(source_paths(&program)
        .iter()
        .all(|source| !source.to_string_lossy().contains("/proto-skip/")));
    assert!(source_paths(&program)
        .iter()
        .all(|source| *source != Path::new("/types/a-root/dup/index.d.ts")));
}

#[test]
fn config_anchor_and_type_roots_preserve_absent_empty_and_nonempty_states() {
    let config = path("/cfg/project/tsconfig.json");
    let host = MemoryCompilerHost::builder("/cwd")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/cfg/project/node_modules/@types/configured/index.d.ts",
            b"declare const configured: true;\n".to_vec(),
        )
        .file(
            "/cwd/node_modules/@types/cwd-only/index.d.ts",
            b"declare const cwdOnly: true;\n".to_vec(),
        )
        .file(
            "/custom/types/custom/index.d.ts",
            b"declare const custom: true;\n".to_vec(),
        )
        .file(
            "/cfg/project/node_modules/fallback/index.d.ts",
            b"declare const forbiddenSecondary: true;\n".to_vec(),
        )
        .build()
        .expect("build config-anchor graph");

    let defaults = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_config_file_path(config.clone())
            .with_types(vec!["*".to_owned()]),
        generous_limits(),
    )
    .expect("absent typeRoots use config-file ancestors");
    assert_eq!(
        source_paths(&defaults),
        [
            Path::new("/work/root.ts"),
            Path::new("/cfg/project/node_modules/@types/configured/index.d.ts"),
        ]
    );
    assert!(defaults
        .resolutions()
        .require_type_reference(&automatic_key(
            "/cfg/project/__inferred type names__.ts",
            "configured",
        ))
        .is_ok());

    let custom = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_config_file_path(config.clone())
            .with_types(vec!["*".to_owned()])
            .with_type_roots(vec![path("/custom/types")]),
        generous_limits(),
    )
    .expect("non-empty typeRoots override config-file ancestors");
    assert_eq!(
        source_paths(&custom),
        [
            Path::new("/work/root.ts"),
            Path::new("/custom/types/custom/index.d.ts"),
        ]
    );

    let empty = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_config_file_path(config)
            .with_types(vec!["fallback".to_owned()])
            .with_type_roots(Vec::new()),
        generous_limits(),
    )
    .expect("explicitly empty typeRoots suppress automatic secondary lookup");
    assert_eq!(source_paths(&empty), [Path::new("/work/root.ts")]);
    assert_missing_reason(
        automatic_resolution(
            &empty,
            "/cfg/project/__inferred type names__.ts",
            "fallback",
        ),
        1,
        "fallback",
        1417,
        "Entry point of type library 'fallback' specified in compilerOptions",
    );
}

#[test]
fn config_anchor_also_controls_source_owned_default_type_roots() {
    let host = MemoryCompilerHost::builder("/cwd")
        .file(
            "/work/root.ts",
            b"/// <reference types=\"configured\" />\nexport {};\n".to_vec(),
        )
        .file(
            "/cfg/node_modules/@types/configured/index.d.ts",
            b"declare const configured: true;\n".to_vec(),
        )
        .file(
            "/cwd/node_modules/@types/configured/index.d.ts",
            b"declare const wrongAnchor: true;\n".to_vec(),
        )
        .build()
        .expect("build source-owned config-anchor graph");

    let program = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(Vec::new())
            .with_config_file_path(path("/cfg/tsconfig.json")),
        generous_limits(),
    )
    .expect("source-owned type reference uses config-file ancestors");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/cfg/node_modules/@types/configured/index.d.ts"),
            Path::new("/work/root.ts"),
        ]
    );
}

#[test]
fn wildcard_manifest_failure_precedes_the_hidden_directory_filter() {
    let manifest_failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/types/.hidden/package.json")),
        "hidden package manifest probe remains observable",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .directory("/types/.hidden")
        .failure(manifest_failure.clone())
        .build()
        .expect("build hidden automatic-package graph");

    let error = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec!["*".to_owned()])
            .with_type_roots(vec![path("/types")]),
        generous_limits(),
    )
    .expect_err("hidden package manifest failure precedes the dot filter");

    assert_eq!(error.kind(), ProgramLoadErrorKind::Host);
    assert_eq!(
        error.operation(),
        ProgramLoadOperation::DiscoverAutomaticTypes
    );
    assert_eq!(error.path(), Some(Path::new("/types/.hidden/package.json")));
    let ProgramLoadError::Host { source, .. } = error else {
        unreachable!("kind identifies the host variant");
    };
    assert_eq!(source, manifest_failure);
}

#[test]
fn unusual_explicit_names_reach_primary_root_probes_before_becoming_misses() {
    let primary_failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::FileExists,
        Some(PathBuf::from("/types/a:b.d.ts")),
        "primary custom-root probe remains observable",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .directory("/types")
        .failure(primary_failure.clone())
        .build()
        .expect("build unusual automatic-name graph");

    let error = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec!["a:b".to_owned()])
            .with_type_roots(vec![path("/types")]),
        generous_limits(),
    )
    .expect_err("the primary custom-root probe precedes package-name parsing");

    assert_eq!(error.kind(), ProgramLoadErrorKind::Resolution);
    assert_eq!(
        error.operation(),
        ProgramLoadOperation::ResolveTypeReference
    );
    assert_eq!(
        error.path(),
        Some(Path::new("/work/__inferred type names__.ts"))
    );
    let ProgramLoadError::Resolution { source, .. } = error else {
        unreachable!("kind identifies the resolution variant");
    };
    assert_eq!(source, ResolutionError::Host(primary_failure));
}

#[test]
fn every_automatic_resolution_precedes_the_first_target_and_library_read() {
    let first_target_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/types/first/index.d.ts")),
        "the first resolved target must not be read yet",
    );
    let second_resolution = HostError::new(
        HostErrorKind::Other,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/types/second")),
        "the second resolution wins before target traversal",
    );
    let library_read = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        Some(PathBuf::from("/typescript/lib/lib.es5.d.ts")),
        "libraries run after automatic types",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/types/first/index.d.ts",
            b"declare const first: true;\n".to_vec(),
        )
        .directory("/types/second")
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const es5: true;\n".to_vec(),
        )
        .failure(first_target_read)
        .failure(second_resolution.clone())
        .failure(library_read)
        .build()
        .expect("build automatic batch-precedence host");
    let roots = [PathBuf::from("/work/root.ts")];
    let error = load_program(
        &host,
        &roots,
        CompilerOptions {
            lib: Some(vec!["es5".to_owned()]),
            ..compiler_options()
        },
        ProgramOptions::default()
            .with_types(vec!["first".to_owned(), "second".to_owned()])
            .with_type_roots(vec![path("/types")]),
        &LibraryCatalog::typescript_6_0_3(LIBRARY_DIRECTORY),
        generous_limits(),
    )
    .expect_err("the second resolution failure precedes target and library reads");

    assert_eq!(error.kind(), ProgramLoadErrorKind::Resolution);
    assert_eq!(
        error.operation(),
        ProgramLoadOperation::ResolveTypeReference
    );
    assert_eq!(
        error.path(),
        Some(Path::new("/work/__inferred type names__.ts"))
    );
    let ProgramLoadError::Resolution {
        specifier, source, ..
    } = error
    else {
        unreachable!("kind identifies the resolution variant");
    };
    assert_eq!(specifier.as_deref(), Some("second"));
    assert_eq!(source, ResolutionError::Host(second_resolution));
}

#[test]
fn automatic_ts2688_preserves_explicit_occurrences_and_uses_global_wildcard_reason() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .build()
        .expect("build automatic missing-types host");
    let explicit = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec![
                "zeta".to_owned(),
                "alpha".to_owned(),
                "zeta".to_owned(),
            ])
            .with_type_roots(Vec::new()),
        generous_limits(),
    )
    .expect("load repeated explicit automatic types");
    assert_eq!(explicit.resolutions().type_reference_len(), 2);
    assert_missing_reason(
        automatic_resolution(&explicit, "/work/__inferred type names__.ts", "alpha"),
        1,
        "alpha",
        1417,
        "Entry point of type library 'alpha' specified in compilerOptions",
    );
    assert_missing_reason(
        automatic_resolution(&explicit, "/work/__inferred type names__.ts", "zeta"),
        2,
        "zeta",
        1417,
        "Entry point of type library 'zeta' specified in compilerOptions",
    );

    let wildcard = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec![
                "missing".to_owned(),
                "*".to_owned(),
                "missing".to_owned(),
            ])
            .with_type_roots(Vec::new()),
        generous_limits(),
    )
    .expect("load mixed explicit and wildcard automatic types");
    assert_eq!(wildcard.resolutions().type_reference_len(), 1);
    assert_missing_reason(
        automatic_resolution(&wildcard, "/work/__inferred type names__.ts", "missing"),
        1,
        "missing",
        1420,
        "Entry point for implicit type library 'missing'",
    );
}

#[test]
fn automatic_names_count_toward_the_request_edge_limit_before_resolution() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .build()
        .expect("build automatic request-edge host");
    let error = load_no_lib(
        &host,
        &["/work/root.ts"],
        ProgramOptions::default()
            .with_no_lib(true)
            .with_types(vec!["one".to_owned(), "two".to_owned()])
            .with_type_roots(Vec::new()),
        limits(1),
    )
    .expect_err("two automatic names exceed the one-edge budget");

    assert_eq!(error.kind(), ProgramLoadErrorKind::ResourceLimit);
    assert_eq!(
        error.operation(),
        ProgramLoadOperation::DiscoverAutomaticTypes
    );
    let exceeded = error
        .limit_exceeded()
        .expect("resource limit carries structured evidence");
    assert_eq!(exceeded.limit(), ProgramLoadLimit::RequestEdges);
    assert_eq!(
        exceeded.path(),
        Some(Path::new("/work/__inferred type names__.ts"))
    );
    assert_eq!(exceeded.maximum(), 1);
    assert_eq!(exceeded.observed(), 2);
}

#[test]
fn library_prefix_publication_remaps_root_and_automatic_type_target_ids() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};\n".to_vec())
        .file(
            "/types/pkg/index.d.ts",
            b"declare const automaticPackage: true;\n".to_vec(),
        )
        .file(
            "/typescript/lib/lib.es5.d.ts",
            b"declare const es5: true;\n".to_vec(),
        )
        .build()
        .expect("build automatic target ID-remapping graph");
    let roots = [PathBuf::from("/work/root.ts")];
    let program = load_program(
        &host,
        &roots,
        CompilerOptions {
            lib: Some(vec!["es5".to_owned()]),
            ..compiler_options()
        },
        ProgramOptions::default()
            .with_types(vec!["pkg".to_owned()])
            .with_type_roots(vec![path("/types")]),
        &LibraryCatalog::typescript_6_0_3(LIBRARY_DIRECTORY),
        generous_limits(),
    )
    .expect("publish library prefix before root and automatic package");

    assert_eq!(
        source_paths(&program),
        [
            Path::new("/typescript/lib/lib.es5.d.ts"),
            Path::new("/work/root.ts"),
            Path::new("/types/pkg/index.d.ts"),
        ]
    );
    assert_eq!(
        program
            .library_files()
            .iter()
            .map(|source| source.index())
            .collect::<Vec<_>>(),
        [0]
    );
    assert_eq!(
        program.roots()[0].source().map(|source| source.index()),
        Some(1)
    );
    let resolution = automatic_resolution(&program, "/work/__inferred type names__.ts", "pkg");
    let ResolutionOutcome::Resolved(target) = resolution.outcome() else {
        panic!("automatic package must resolve");
    };
    assert_eq!(target.source().index(), 2);
    assert_eq!(
        target.target().display(),
        Path::new("/types/pkg/index.d.ts")
    );
    assert!(target.primary());
    assert!(!target.is_external_library_import());
}
