use std::path::{Path, PathBuf};

use tsc_host::{HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    CompilerOptions, HostResolvedModule, ModuleExtension, ModuleResolver, PackageId,
    PackageJsonType, PathMapping, ProgramOptions, ProgramPath, ResolutionError, ResolutionMode,
    ResolutionOutcome, ResolvedModuleTarget, SourceFileId,
};

const INNER_PACKAGE_JSON: &str = r#"{
    "name": "inner",
    "private": true,
    "exports": {
        "./cjs/*": "./*.cjs",
        "./cjs/exclude/*": null,
        "./mjs/*": "./*.mjs",
        "./mjs/exclude/*": null,
        "./js/*": "./*.js",
        "./js/exclude/*": null,
        "./conditional": { "types": "./index.js" },
        "./array": ["./index.js"]
    }
}"#;

fn fixture_host() -> (MemoryCompilerHost, HostError) {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from("/node_modules/denied/package.json")),
        "denied by module-resolution contract",
    );
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br#"{"name":"root","private":true,"type":"module"}"#.to_vec(),
        )
        .file("/index.ts", b"export {};".to_vec())
        .file(
            "/node_modules/inner/package.json",
            INNER_PACKAGE_JSON.as_bytes().to_vec(),
        )
        .file("/node_modules/inner/test.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/inner/index.d.cts",
            b"export const cjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/index.d.mts",
            b"export const mjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/index.d.ts",
            b"export const js: true;".to_vec(),
        )
        // These files make an incorrect broad-pattern or legacy fallback
        // observable: the more-specific null entry must still terminate.
        .file(
            "/node_modules/inner/exclude/index.d.cts",
            b"export const excludedCjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/exclude/index.d.mts",
            b"export const excludedMjs: true;".to_vec(),
        )
        .file(
            "/node_modules/inner/exclude/index.d.ts",
            b"export const excludedJs: true;".to_vec(),
        )
        .file(
            "/node_modules/denied/package.json",
            br#"{"name":"denied","exports":"./index.js"}"#.to_vec(),
        )
        .failure(denied.clone())
        .build()
        .expect("build one package-exports host tree");
    (host, denied)
}

fn options_for_module(module: i32) -> CompilerOptions {
    CompilerOptions {
        module: Some(module),
        ..CompilerOptions::default()
    }
}

fn resolved(outcome: ResolutionOutcome<HostResolvedModule>) -> HostResolvedModule {
    let ResolutionOutcome::Resolved(resolved) = outcome else {
        panic!("expected a resolved package export");
    };
    resolved
}

fn assert_unsupported(error: ResolutionError, expected_feature: &str) {
    let ResolutionError::Unsupported { feature, detail } = error else {
        panic!("expected unsupported resolution, got {error:?}");
    };
    assert_eq!(feature, expected_feature);
    assert!(!detail.is_empty());
}

#[test]
fn paths_exact_longest_prefix_and_substitution_order_are_stable() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/general/item.ts", b"export {};".to_vec())
        .file("/work/general/special/other.ts", b"export {};".to_vec())
        .file("/work/specific/item.ts", b"export {};".to_vec())
        .file("/work/specific/other.ts", b"export {};".to_vec())
        .file("/work/exact/item.ts", b"export {};".to_vec())
        .file("/work/tie-first/x.ts", b"export {};".to_vec())
        .file("/work/tie-second/x/tail.ts", b"export {};".to_vec())
        .file("/work/ordered/second.ts", b"export {};".to_vec())
        .build()
        .expect("build ordered paths host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("@pkg/*", vec!["general/*".to_owned()]),
        PathMapping::new("@pkg/special/*", vec!["specific/*".to_owned()]),
        PathMapping::new("@pkg/special/item", vec!["exact/item".to_owned()]),
        PathMapping::new("@tie/*/tail", vec!["tie-first/*".to_owned()]),
        PathMapping::new("@tie/*", vec!["tie-second/*".to_owned()]),
        PathMapping::new(
            "@ordered/*",
            vec!["ordered/missing/*".to_owned(), "ordered/*".to_owned()],
        ),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create paths resolver");

    for (specifier, expected) in [
        ("@pkg/item", "/work/general/item.ts"),
        ("@pkg/special/other", "/work/specific/other.ts"),
        ("@pkg/special/item", "/work/exact/item.ts"),
        ("@tie/x/tail", "/work/tie-first/x.ts"),
        ("@ordered/second", "/work/ordered/second.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("resolve ordered paths candidate"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "{specifier}"
        );
    }
}

#[test]
fn optional_settings_preserve_legacy_passes_and_modern_substitution_order() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/first/priority.js", b"module.exports = {};".to_vec())
        .file("/work/second/priority.ts", b"export {};".to_vec())
        .file("/work/first/explicit.js", b"module.exports = {};".to_vec())
        .build()
        .expect("build extension-pass paths host");
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new(
            "priority",
            vec!["first/priority".to_owned(), "second/priority".to_owned()],
        ),
        PathMapping::new("explicit", vec!["first/explicit.js".to_owned()]),
    ]);

    for (resolution_kind, expected_priority) in [
        (1, "/work/second/priority.ts"),
        (2, "/work/second/priority.ts"),
        (3, "/work/first/priority.js"),
        (99, "/work/first/priority.js"),
        (100, "/work/first/priority.js"),
    ] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create resolver-kind paths resolver");
        let priority = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "priority",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve extension-pass candidate"),
        );
        assert_eq!(
            priority.resolved_file().canonical().as_path(),
            Path::new(expected_priority),
            "moduleResolution={resolution_kind}"
        );

        let explicit = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "explicit",
                    ResolutionMode::CommonJs,
                )
                .expect("resolve raw explicit-extension substitution"),
        );
        assert_eq!(explicit.extension(), &ModuleExtension::Js);
        assert_eq!(
            explicit.resolved_file().canonical().as_path(),
            Path::new("/work/first/explicit.js")
        );
    }
}

#[test]
fn matched_paths_miss_suppresses_base_url_but_keeps_ordinary_fallbacks() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/base/pkg.ts", b"export const wrong = true;".to_vec())
        .file("/work/base/unmapped.ts", b"export {};".to_vec())
        .file("/work/node_modules/pkg/index.d.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/pkg/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build paths fallback host");
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "pkg",
        vec!["missing/pkg".to_owned()],
    )]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            base_url: Some("./base".to_owned()),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create fallback resolver");
        let module = resolved(
            resolver
                .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs)
                .expect("fall through from matched paths miss"),
        );
        let expected = if resolution_kind == 1 {
            "/work/node_modules/@types/pkg/index.d.ts"
        } else {
            "/work/node_modules/pkg/index.d.ts"
        };
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "moduleResolution={resolution_kind}"
        );

        let base_url = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    "unmapped",
                    ResolutionMode::CommonJs,
                )
                .expect("a paths non-match continues to baseUrl"),
        );
        assert_eq!(
            base_url.resolved_file().canonical().as_path(),
            Path::new("/work/base/unmapped.ts")
        );
    }
}

#[test]
fn paths_without_base_url_use_cwd_and_path_matching_remains_case_sensitive() {
    let host = MemoryCompilerHost::builder("/Work/Project")
        .case_sensitive(false)
        .file("/work/project/main.ts", b"export {};".to_vec())
        .file("/work/project/src/value.ts", b"export {};".to_vec())
        .file("/work/shared/parent.ts", b"export {};".to_vec())
        .file("/shared/absolute.ts", b"export {};".to_vec())
        .file(
            "/work/project/package.json",
            br#"{"name":"cwd","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/project/index.ts", b"export {};".to_vec())
        .build()
        .expect("build case-insensitive cwd host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("@Alias/*", vec!["./src/*".to_owned()]),
        PathMapping::new("@parent", vec!["../shared/parent".to_owned()]),
        PathMapping::new("@absolute", vec!["/shared/absolute".to_owned()]),
        PathMapping::new("@cwd", vec![String::new()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create cwd paths resolver");

    for (specifier, expected) in [
        ("@Alias/Value", "/work/project/src/value.ts"),
        ("@parent", "/work/shared/parent.ts"),
        ("@absolute", "/shared/absolute.ts"),
        ("@cwd", "/work/project/index.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/Work/Project/main.ts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("resolve cwd-relative paths mapping"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "{specifier}"
        );
    }
    assert_eq!(
        resolver
            .resolve(
                Path::new("/Work/Project/main.ts"),
                "@alias/Value",
                ResolutionMode::CommonJs,
            )
            .expect("case-distinct pattern is a supported miss"),
        ResolutionOutcome::NotFound
    );

    let base_options = CompilerOptions {
        module_resolution: Some(100),
        base_url: Some("./base/../src".to_owned()),
        ..CompilerOptions::default()
    };
    let mut base_resolver =
        ModuleResolver::new(&host, &base_options).expect("normalize relative baseUrl from cwd");
    let module = resolved(
        base_resolver
            .resolve(
                Path::new("/Work/Project/main.ts"),
                "Value",
                ResolutionMode::CommonJs,
            )
            .expect("resolve normalized baseUrl candidate"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/project/src/value.ts")
    );
}

#[test]
fn paths_host_failures_stop_before_later_substitutions() {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/first/value.ts")),
        "first paths substitution denied",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/first/placeholder.txt", b"present".to_vec())
        .file("/work/second/value.ts", b"export {};".to_vec())
        .failure(denied.clone())
        .build()
        .expect("build paths failure host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "@value",
        vec!["first/value".to_owned(), "second/value".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create paths failure resolver");

    let error = resolver
        .resolve(
            Path::new("/work/main.ts"),
            "@value",
            ResolutionMode::CommonJs,
        )
        .expect_err("first substitution host failure must not become a miss");
    assert_eq!(error, ResolutionError::Host(denied));
}

#[test]
fn malformed_paths_configuration_fails_before_resolution() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .build()
        .expect("build paths validation host");
    let options = CompilerOptions::default();
    let cases = [
        ProgramOptions::default().with_paths(vec![PathMapping::new("", vec!["value".to_owned()])]),
        ProgramOptions::default().with_paths(vec![
            PathMapping::new("dup", vec!["first".to_owned()]),
            PathMapping::new("dup", vec!["second".to_owned()]),
        ]),
        ProgramOptions::default()
            .with_paths(vec![PathMapping::new("two**", vec!["value".to_owned()])]),
        ProgramOptions::default().with_paths(vec![PathMapping::new("empty", Vec::new())]),
        ProgramOptions::default()
            .with_paths(vec![PathMapping::new("two", vec!["value**".to_owned()])]),
        ProgramOptions::default().with_paths(vec![PathMapping::new(
            "nul",
            vec!["value\0path".to_owned()],
        )]),
        ProgramOptions::default().with_paths(vec![PathMapping::new(
            "unc",
            vec![r"\\server\share\*".to_owned()],
        )]),
        ProgramOptions::default().with_paths(vec![PathMapping::new(
            "drive",
            vec!["C:relative/*".to_owned()],
        )]),
    ];

    for program_options in cases {
        let error =
            match ModuleResolver::new_with_program_options(&host, &options, &program_options) {
                Ok(_) => panic!("malformed paths configuration must fail closed"),
                Err(error) => error,
            };
        assert!(matches!(
            error,
            ResolutionError::InvalidData(_) | ResolutionError::Unsupported { .. }
        ));
    }

    for base_url in ["\0", r"\\server\share", "C:relative"] {
        let options = CompilerOptions {
            base_url: Some(base_url.to_owned()),
            ..CompilerOptions::default()
        };
        let error = match ModuleResolver::new(&host, &options) {
            Ok(_) => panic!("malformed baseUrl must fail closed"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            ResolutionError::InvalidData(_) | ResolutionError::Unsupported { .. }
        ));
    }
}

#[test]
fn optional_local_file_probe_does_not_read_an_ancestor_package() {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from("/work/package.json")),
        "local optional resolution must not inspect an ancestor package",
    );
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/src/value.ts", b"export const value = 1;".to_vec())
        .file(
            "/work/package.json",
            br#"{"name":"unrelated","version":"1.0.0"}"#.to_vec(),
        )
        .failure(denied)
        .build()
        .expect("build local optional package-failure host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "value",
        vec!["src/value".to_owned()],
    )]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create local optional resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "value",
                ResolutionMode::CommonJs,
            )
            .expect("resolve before unrelated ancestor package metadata"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/src/value.ts")
    );
    assert_eq!(module.package_id(), None);
    assert_eq!(module.package_metadata(), None);
}

#[test]
fn optional_external_files_use_the_package_root_and_follow_realpath() {
    let source = b"export const value = 1;".to_vec();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/pkg/package.json",
            br#"{"name":"pkg","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/pkg/sub/package.json",
            br#"{"name":"wrong-nested-package","version":"9.0.0"}"#.to_vec(),
        )
        .file("/work/node_modules/pkg/sub/value.ts", source.clone())
        .file("/store/pkg/sub/value.ts", source)
        .realpath(
            "/work/node_modules/pkg/sub/value.ts",
            "/store/pkg/sub/value.ts",
        )
        .build()
        .expect("build external optional realpath host");
    let options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("value", vec!["node_modules/pkg/sub/value".to_owned()]),
        PathMapping::new("exact", vec!["node_modules/pkg/sub/value.ts".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create external optional resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "value",
                ResolutionMode::CommonJs,
            )
            .expect("resolve external optional file"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/store/pkg/sub/value.ts")
    );
    assert_eq!(
        module
            .original_path()
            .expect("external lexical path is retained")
            .canonical()
            .as_path(),
        Path::new("/work/node_modules/pkg/sub/value.ts")
    );
    let package_id = module.package_id().expect("node package id is attached");
    assert_eq!(package_id.name(), "pkg");
    assert_eq!(package_id.submodule_name(), "sub/value.ts");
    assert_eq!(
        module
            .package_metadata()
            .and_then(|metadata| metadata.name()),
        Some("pkg")
    );

    let exact = resolved(
        resolver
            .resolve(
                Path::new("/work/main.ts"),
                "exact",
                ResolutionMode::CommonJs,
            )
            .expect("resolve exact-extension external optional file"),
    );
    assert_eq!(
        exact.resolved_file().canonical().as_path(),
        Path::new("/store/pkg/sub/value.ts")
    );
    assert!(exact.original_path().is_some());
    assert_eq!(exact.package_id(), None);
}

#[test]
fn arbitrary_extension_twins_resolve_in_legacy_and_node_esm_modes() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/src/theme.d.css.ts",
            b"declare const theme: string; export default theme;".to_vec(),
        )
        .build()
        .expect("build arbitrary declaration-twin host");
    let program_options = ProgramOptions::default().with_paths(vec![PathMapping::new(
        "theme",
        vec!["src/theme.css".to_owned()],
    )]);

    for resolution_kind in [1, 2, 3, 99, 100] {
        let options = CompilerOptions {
            module_resolution: Some(resolution_kind),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new_with_program_options(&host, &options, &program_options)
                .expect("create arbitrary-extension resolver");
        let mode = if matches!(resolution_kind, 3 | 99) {
            ResolutionMode::EsNext
        } else {
            ResolutionMode::CommonJs
        };
        let module = resolved(
            resolver
                .resolve(Path::new("/work/main.ts"), "theme", mode)
                .expect("resolve arbitrary declaration twin"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new("/work/src/theme.d.css.ts"),
            "moduleResolution={resolution_kind}"
        );
        assert_eq!(
            module.extension(),
            &ModuleExtension::Arbitrary(".d.css.ts".to_owned())
        );
    }
}

#[test]
fn empty_captures_keep_the_literal_star_and_paths_precede_modern_package_maps() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/package.json",
            br##"{
                "name":"app",
                "imports":{"#mapped":"./imports.ts","#fallback":"./fallback.ts"},
                "exports":{"./self":"./self.ts"}
            }"##
            .to_vec(),
        )
        .file("/work/literal/*.ts", b"export {};".to_vec())
        .file("/work/literal/bar.ts", b"export {};".to_vec())
        .file("/work/paths/imports.ts", b"export {};".to_vec())
        .file("/work/paths/self.ts", b"export {};".to_vec())
        .file("/work/imports.ts", b"export {};".to_vec())
        .file("/work/fallback.ts", b"export {};".to_vec())
        .file("/work/self.ts", b"export {};".to_vec())
        .build()
        .expect("build paths/package-map precedence host");
    let options = CompilerOptions {
        module: Some(199),
        ..CompilerOptions::default()
    };
    let program_options = ProgramOptions::default().with_paths(vec![
        PathMapping::new("foo*", vec!["literal/*.ts".to_owned()]),
        PathMapping::new("#mapped", vec!["paths/imports.ts".to_owned()]),
        PathMapping::new("#fallback", vec!["missing/imports".to_owned()]),
        PathMapping::new("app/self", vec!["paths/self.ts".to_owned()]),
    ]);
    let mut resolver = ModuleResolver::new_with_program_options(&host, &options, &program_options)
        .expect("create paths/package-map resolver");

    for (specifier, expected) in [
        ("foo", "/work/literal/*.ts"),
        ("foobar", "/work/literal/bar.ts"),
        ("#mapped", "/work/paths/imports.ts"),
        ("#fallback", "/work/fallback.ts"),
        ("app/self", "/work/paths/self.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/main.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve paths/package-map precedence candidate"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected),
            "{specifier}"
        );
    }
}

#[test]
fn classic_resolution_is_bounded_to_legacy_files_and_at_types() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/src/app.ts", b"export {};".to_vec())
        .file("/work/src/other.ts", b"export const x = 1;".to_vec())
        .file("/work/src/legacy.ts", b"export const x = 1;".to_vec())
        .file(
            "/work/node_modules/direct/index.d.ts",
            b"export const x: 1;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/traditional/package.json",
            br#"{"name":"@types/traditional","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/traditional/index.d.ts",
            b"export const x: 1;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/foo/package.json",
            br#"{
                "name":"@types/foo",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "import":"./index.d.mts",
                        "require":"./index.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/@types/foo/index.d.mts",
            b"export const x: \"module\";".to_vec(),
        )
        .file(
            "/work/node_modules/@types/foo/index.d.cts",
            b"export const x: \"script\";".to_vec(),
        )
        .build()
        .expect("build Classic resolver host");
    let options = CompilerOptions {
        module: Some(99),
        module_resolution: Some(1),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Classic resolver");

    for (specifier, expected) in [
        ("./other", "/work/src/other.ts"),
        ("legacy", "/work/src/legacy.ts"),
        (
            "traditional",
            "/work/node_modules/@types/traditional/index.d.ts",
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/src/app.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve a Classic legacy target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/src/app.ts"),
                "direct",
                ResolutionMode::EsNext,
            )
            .expect("ordinary node_modules packages are outside Classic"),
        ResolutionOutcome::NotFound
    );
    for mode in [
        ResolutionMode::Unspecified,
        ResolutionMode::EsNext,
        ResolutionMode::CommonJs,
    ] {
        let facts = resolver
            .resolve_with_facts(Path::new("/work/src/app.ts"), "foo", mode)
            .expect("Classic exports-only @types package is an authoritative miss");
        assert_eq!(facts.outcome(), &ResolutionOutcome::NotFound);
        assert_eq!(facts.alternate_result(), None);
    }
}

#[test]
fn node10_primary_miss_retains_the_bundler_declaration_alternate() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"import { pkg } from 'pkg';".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "exports":{".":"./definitely-not-index.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/pkg/definitely-not-index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build Node10 alternate host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    let facts = resolver
        .resolve_with_facts(Path::new("/index.ts"), "pkg", ResolutionMode::Unspecified)
        .expect("resolve Node10 primary and diagnostic alternate");
    assert_eq!(facts.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(
        facts
            .alternate_result()
            .expect("Bundler preferred retry finds the declaration twin")
            .canonical()
            .as_path(),
        Path::new("/node_modules/pkg/definitely-not-index.d.ts")
    );

    assert_eq!(
        resolver
            .resolve(Path::new("/index.ts"), "pkg", ResolutionMode::Unspecified)
            .expect("legacy wrapper keeps the primary outcome"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn node10_legacy_primary_and_bundler_retry_keep_their_exact_boundaries() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .file(
            "/node_modules/typed/package.json",
            br#"{
                "name":"typed",
                "version":"1.0.0",
                "types":"./legacy.d.ts",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file("/node_modules/typed/legacy.d.ts", b"export {};".to_vec())
        .file("/node_modules/typed/modern.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/untyped/package.json",
            br#"{
                "name":"untyped",
                "version":"1.0.0",
                "main":"./legacy.js",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/untyped/legacy.js",
            b"module.exports = {};".to_vec(),
        )
        .file("/node_modules/untyped/modern.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/js-only/package.json",
            br#"{
                "name":"js-only",
                "version":"1.0.0",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/js-only/modern.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/conditions/package.json",
            br#"{
                "name":"conditions",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "node":"./node.js",
                        "import":"./import.js",
                        "require":"./require.js"
                    }
                }
            }"#
            .to_vec(),
        )
        .file("/node_modules/conditions/node.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/conditions/import.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/conditions/require.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/manifestless/placeholder.txt",
            b"package directory without a manifest".to_vec(),
        )
        .file(
            "/node_modules/@types/manifestless/package.json",
            br#"{
                "name":"@types/manifestless",
                "version":"1.0.0",
                "exports":{".":"./modern.js"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/@types/manifestless/modern.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/no-manifests/placeholder.txt",
            b"package directory without a manifest".to_vec(),
        )
        .file(
            "/node_modules/@types/no-manifests/placeholder.txt",
            b"types package directory without a manifest".to_vec(),
        )
        .build()
        .expect("build bounded Node10 host");
    let options = CompilerOptions {
        module_resolution: Some(2),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &options).expect("create Node10 resolver");

    let typed = resolver
        .resolve_with_facts(Path::new("/index.ts"), "typed", ResolutionMode::Unspecified)
        .expect("Node10 legacy types field wins");
    let ResolutionOutcome::Resolved(typed_primary) = typed.outcome() else {
        panic!("expected typed legacy primary: {typed:#?}");
    };
    assert_eq!(
        typed_primary.resolved_file().canonical().as_path(),
        Path::new("/node_modules/typed/legacy.d.ts")
    );
    assert_eq!(typed.alternate_result(), None);

    let untyped = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "untyped",
            ResolutionMode::Unspecified,
        )
        .expect("Node10 JavaScript primary retains a declaration alternate");
    let ResolutionOutcome::Resolved(untyped_primary) = untyped.outcome() else {
        panic!("expected untyped legacy primary: {untyped:#?}");
    };
    assert_eq!(untyped_primary.extension(), &ModuleExtension::Js);
    assert_eq!(
        untyped
            .alternate_result()
            .expect("Bundler retry finds modern types")
            .canonical()
            .as_path(),
        Path::new("/node_modules/untyped/modern.d.ts")
    );

    let js_only = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "js-only",
            ResolutionMode::Unspecified,
        )
        .expect("preferred-only retry does not accept JavaScript");
    assert_eq!(js_only.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(js_only.alternate_result(), None);

    let conditions = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "conditions",
            ResolutionMode::Unspecified,
        )
        .expect("Bundler retry uses Bundler default conditions");
    assert_eq!(conditions.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(
        conditions
            .alternate_result()
            .expect("Bundler defaults select import and exclude node")
            .canonical()
            .as_path(),
        Path::new("/node_modules/conditions/import.d.ts")
    );

    let manifestless = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "manifestless",
            ResolutionMode::Unspecified,
        )
        .expect("an observed @types package manifest enables Bundler retry");
    assert_eq!(manifestless.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(
        manifestless
            .alternate_result()
            .expect("Bundler retry honors the observed @types exports")
            .canonical()
            .as_path(),
        Path::new("/node_modules/@types/manifestless/modern.d.ts")
    );

    let no_manifests = resolver
        .resolve_with_facts(
            Path::new("/index.ts"),
            "no-manifests",
            ResolutionMode::Unspecified,
        )
        .expect("manifestless package directories do not enable Bundler retry");
    assert_eq!(no_manifests.outcome(), &ResolutionOutcome::NotFound);
    assert_eq!(no_manifests.alternate_result(), None);
}

#[test]
fn classic_and_node10_remain_unsupported_for_type_reference_resolution() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.ts", b"export {};".to_vec())
        .build()
        .expect("build resolver-mode boundary host");
    for module_resolution in [1, 2] {
        let options = CompilerOptions {
            module_resolution: Some(module_resolution),
            ..CompilerOptions::default()
        };
        let mut resolver =
            ModuleResolver::new(&host, &options).expect("create legacy module resolver");
        let error = resolver
            .resolve_type_reference(
                Path::new("/index.ts"),
                "pkg",
                ResolutionMode::Unspecified,
                None,
            )
            .expect_err("legacy modes are admitted only for module resolution");
        assert_unsupported(error, "module-resolution-kind");
    }
}

#[test]
fn a_more_specific_null_pattern_is_a_terminal_not_found() {
    let (host, _) = fixture_host();
    let options = options_for_module(100);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, mode) in [
        ("inner/cjs/exclude/index", ResolutionMode::CommonJs),
        ("inner/mjs/exclude/index", ResolutionMode::EsNext),
        ("inner/js/exclude/index", ResolutionMode::EsNext),
    ] {
        assert_eq!(
            resolver
                .resolve(Path::new("/index.ts"), specifier, mode)
                .expect("resolve selected null export"),
            ResolutionOutcome::NotFound,
            "specific null export must beat the earlier broad pattern for {specifier}",
        );
    }
}

#[test]
fn declaration_twins_and_external_provenance_hold_for_all_node_module_kinds() {
    let (host, _) = fixture_host();
    let targets = [
        (
            "inner/cjs/index",
            ResolutionMode::CommonJs,
            "/node_modules/inner/index.d.cts",
            ModuleExtension::Dcts,
        ),
        (
            "inner/mjs/index",
            ResolutionMode::EsNext,
            "/node_modules/inner/index.d.mts",
            ModuleExtension::Dmts,
        ),
        (
            "inner/js/index",
            ResolutionMode::EsNext,
            "/node_modules/inner/index.d.ts",
            ModuleExtension::Dts,
        ),
    ];

    for module in [100, 101, 102, 199] {
        let options = options_for_module(module);
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

        for (specifier, mode, expected_path, expected_extension) in &targets {
            let external = resolved(
                resolver
                    .resolve(Path::new("/index.ts"), specifier, *mode)
                    .expect("resolve through node_modules"),
            );
            assert_eq!(external.resolved_file().display(), Path::new(expected_path));
            assert_eq!(
                external.resolved_file().canonical().as_path(),
                Path::new(expected_path)
            );
            assert_eq!(external.extension(), expected_extension);
            assert!(external.is_external_library_import());
            assert!(!external.resolved_using_ts_extension());
            assert_eq!(external.original_path(), None);
            assert_eq!(external.package_id(), None);
            let package_metadata = external
                .package_metadata()
                .expect("manifest-backed export retains package metadata");
            assert_eq!(package_metadata.name(), Some("inner"));
            assert_eq!(package_metadata.module_type(), PackageJsonType::Unspecified);
            let caller_spelling = ProgramPath::from_trusted_parts(
                expected_path.trim_start_matches('/'),
                *expected_path,
            )
            .expect("create caller-owned target spelling");
            let rebound = external
                .clone()
                .into_resolved_module(ResolvedModuleTarget::Source {
                    source: SourceFileId::from_raw(0),
                    resolved_file: caller_spelling,
                })
                .expect("canonical target identity binds across display spellings");
            assert_eq!(
                rebound.target().resolved_file().display(),
                Path::new(expected_path.trim_start_matches('/'))
            );

            let self_reference = resolved(
                resolver
                    .resolve(Path::new("/node_modules/inner/test.d.ts"), specifier, *mode)
                    .expect("resolve package self-reference"),
            );
            assert_eq!(
                self_reference.resolved_file().canonical().as_path(),
                Path::new(expected_path)
            );
            assert_eq!(self_reference.extension(), expected_extension);
            assert!(
                !self_reference.is_external_library_import(),
                "a package self-reference is not an external-library traversal"
            );
        }

        let inner_scope = resolver
            .package_scope_for_file(Path::new("/node_modules/inner/test.d.ts"))
            .expect("observe inner package scope")
            .expect("inner package scope exists");
        assert_eq!(inner_scope.module_type(), PackageJsonType::Unspecified);
        assert_eq!(
            inner_scope.package_json().canonical().as_path(),
            Path::new("/node_modules/inner/package.json")
        );
        let root_scope = resolver
            .package_scope_for_file(Path::new("/index.ts"))
            .expect("observe root package scope")
            .expect("root package scope exists");
        assert_eq!(root_scope.module_type(), PackageJsonType::Module);
        assert_eq!(resolver.observed_package_metadata().count(), 2);
    }
}

#[test]
fn conditional_and_array_targets_resolve_while_host_failures_propagate() {
    let (host, denied) = fixture_host();
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["inner/conditional", "inner/array"] {
        let resolution = resolved(
            resolver
                .resolve(Path::new("/index.ts"), specifier, ResolutionMode::EsNext)
                .expect("resolve selected package-map target"),
        );
        assert_eq!(
            resolution.resolved_file().canonical().as_path(),
            Path::new("/node_modules/inner/index.d.ts")
        );
    }

    let error = resolver
        .resolve(Path::new("/index.ts"), "denied", ResolutionMode::EsNext)
        .expect_err("host read failure must propagate");
    let ResolutionError::Host(actual) = error else {
        panic!("expected host resolution error, got {error:?}");
    };
    assert_eq!(actual, denied);
}

#[test]
fn h02c_exports_targets_and_relative_requests_follow_the_authoritative_map() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br#"{"name":"root","type":"module"}"#.to_vec(),
        )
        .file("/src/index.ts", b"export {};".to_vec())
        .file("/src/other.ts", b"export const other = true;".to_vec())
        .file(
            "/node_modules/source/package.json",
            br#"{"name":"source","version":"1.0.0","exports":"./index.ts"}"#.to_vec(),
        )
        .file(
            "/node_modules/source/index.ts",
            b"export const source = true;".to_vec(),
        )
        .file(
            "/node_modules/conditions/package.json",
            br#"{
                "name":"conditions",
                "version":"1.0.0",
                "exports": {
                    "./yes": {
                        "types@<4":"./wrong.d.ts",
                        "types@>=4":"./right.d.ts"
                    },
                    "./fallback": {
                        "types":"./missing.d.ts",
                        "default":"./right.d.ts"
                    },
                    "./null": {
                        "types":null,
                        "default":"./right.d.ts"
                    },
                    "./no": { "types@<4":"./wrong.d.ts" }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/conditions/right.d.ts",
            b"export const right: true;".to_vec(),
        )
        .file(
            "/node_modules/directory/package.json",
            br#"{"name":"directory","exports":{"./":"./"}}"#.to_vec(),
        )
        .file(
            "/node_modules/directory/index.d.ts",
            b"export const directory: true;".to_vec(),
        )
        .file(
            "/node_modules/directory/other.d.ts",
            b"export const mustNotResolveImplicitly: true;".to_vec(),
        )
        .file(
            "/node_modules/double/package.json",
            br#"{"name":"double","exports":{"./a/*/b/*":"./index.js"}}"#.to_vec(),
        )
        .file(
            "/node_modules/double/index.d.ts",
            b"export const wrong: true;".to_vec(),
        )
        .file(
            "/node_modules/versioned/package.json",
            br#"{
                "name":"versioned",
                "version":"1.0.0",
                "typesVersions":{"*":{"foo":["./types/foo.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/versioned/types/foo.d.ts",
            b"export const versioned: true;".to_vec(),
        )
        .build()
        .expect("build H0.2c package-map host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let relative = resolved(
        resolver
            .resolve(
                Path::new("/src/index.ts"),
                "./other.js",
                ResolutionMode::EsNext,
            )
            .expect("resolve relative written-JS request"),
    );
    assert_eq!(
        relative.resolved_file().canonical().as_path(),
        Path::new("/src/other.ts")
    );
    assert!(!relative.is_external_library_import());

    for (specifier, expected) in [
        ("source", "/node_modules/source/index.ts"),
        ("conditions/yes", "/node_modules/conditions/right.d.ts"),
        ("conditions/fallback", "/node_modules/conditions/right.d.ts"),
        ("directory/index.js", "/node_modules/directory/index.d.ts"),
        ("versioned/foo", "/node_modules/versioned/types/foo.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/src/index.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve H0.2c package request"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert!(module.is_external_library_import());
        if specifier == "source" {
            assert_eq!(module.extension(), &ModuleExtension::Ts);
            assert!(!module.resolved_using_ts_extension());
        }
        if specifier == "versioned/foo" {
            assert_eq!(module.package_id(), None);
        }
    }

    for specifier in [
        "conditions/no",
        "conditions/null",
        "directory/other",
        "double/a/*/b/*",
    ] {
        assert_eq!(
            resolver
                .resolve(
                    Path::new("/src/index.ts"),
                    specifier,
                    ResolutionMode::EsNext
                )
                .expect("unsupported package key is an authoritative miss"),
            ResolutionOutcome::NotFound
        );
    }
}

#[test]
fn package_imports_cover_relative_bare_conditional_array_null_and_cycles() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br##"{
                "name":"root",
                "version":"1.0.0",
                "type":"module",
                "exports":"./index.cjs",
                "imports": {
                    "#exact":"./src/exact.js",
                    "#pattern/*":"./src/*.js",
                    "#condition": {
                        "import":"./src/import.js",
                        "require":"./src/require.cjs"
                    },
                    "#array":["./src/missing.js", "./src/fallback.js"],
                    "#blocked":null,
                    "#external":"dep/subpath",
                    "#cycle-a":"#cycle-b",
                    "#cycle-b":"#cycle-a",
                    "#self":"root",
                    "#direct.ts":"./src/direct.ts",
                    "#mapped/*":"./src/*"
                }
            }"##
            .to_vec(),
        )
        .file("/index.ts", b"export {};".to_vec())
        // If an imports-to-self rewrite incorrectly re-enters this package's
        // exports map, the written .cjs target substitutes this source.
        .file("/index.cts", b"export const wrongSelf = true;".to_vec())
        .file("/src/exact.ts", b"export const exact = true;".to_vec())
        .file("/src/pattern.ts", b"export const pattern = true;".to_vec())
        .file("/src/import.ts", b"export const esm = true;".to_vec())
        .file("/src/require.cts", b"export const cjs = true;".to_vec())
        .file(
            "/src/fallback.ts",
            b"export const fallback = true;".to_vec(),
        )
        .file("/src/direct.ts", b"export const direct = true;".to_vec())
        .file(
            "/src/from-pattern.ts",
            b"export const mapped = true;".to_vec(),
        )
        .file(
            "/node_modules/dep/package.json",
            br#"{
                "name":"dep",
                "version":"2.0.0",
                "exports":{"./subpath":"./types.d.ts"}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/dep/types.d.ts",
            b"export const dep: true;".to_vec(),
        )
        .build()
        .expect("build package-imports host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, mode, expected) in [
        ("#exact", ResolutionMode::EsNext, "/src/exact.ts"),
        (
            "#pattern/pattern",
            ResolutionMode::EsNext,
            "/src/pattern.ts",
        ),
        ("#condition", ResolutionMode::EsNext, "/src/import.ts"),
        ("#condition", ResolutionMode::CommonJs, "/src/require.cts"),
        ("#array", ResolutionMode::EsNext, "/src/fallback.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/index.ts"), specifier, mode)
                .expect("resolve package-imports target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert!(!module.is_external_library_import());
        assert_eq!(module.package_id().map(PackageId::name), Some("root"));
    }

    let external = resolved(
        resolver
            .resolve(Path::new("/index.ts"), "#external", ResolutionMode::EsNext)
            .expect("reinsert bare imports target into node_modules lookup"),
    );
    assert_eq!(
        external.resolved_file().canonical().as_path(),
        Path::new("/node_modules/dep/types.d.ts")
    );
    assert_eq!(external.package_id().map(PackageId::name), Some("dep"));
    assert!(
        !external.is_external_library_import(),
        "the outer imports boundary owns an external bare target"
    );

    for specifier in ["#blocked", "#cycle-a", "#cycle-b", "#self"] {
        assert_eq!(
            resolver
                .resolve(Path::new("/index.ts"), specifier, ResolutionMode::EsNext)
                .expect("terminal or bounded imports miss"),
            ResolutionOutcome::NotFound,
            "{specifier} must not escape the package-map boundary"
        );
    }

    let direct = resolved(
        resolver
            .resolve(Path::new("/index.ts"), "#direct.ts", ResolutionMode::EsNext)
            .expect("resolve explicit TypeScript imports target"),
    );
    assert!(!direct.resolved_using_ts_extension());
    let substituted = resolved(
        resolver
            .resolve(
                Path::new("/index.ts"),
                "#mapped/from-pattern.ts",
                ResolutionMode::EsNext,
            )
            .expect("resolve TypeScript extension introduced by pattern capture"),
    );
    assert!(substituted.resolved_using_ts_extension());
}

#[test]
fn package_imports_preserve_non_root_self_and_option_boundaries() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/workspace/package.json",
            br##"{
                "name":"workspace",
                "exports":"./index.js",
                "imports": {
                    "#self":"workspace",
                    "#exact":"./src/exact.js",
                    "#x:y":"./src/exact.js",
                    "#x\\y":"./src/exact.js",
                    "#x\u0000y":"./src/exact.js",
                    "#x/../y":"./src/exact.js",
                    "#dot":".dependency",
                    "#blocked":null
                }
            }"##
            .to_vec(),
        )
        .file("/workspace/main.ts", b"export {};".to_vec())
        .file("/workspace/index.ts", b"export const self = true;".to_vec())
        .file(
            "/workspace/src/exact.ts",
            b"export const exact = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/.dependency/package.json",
            br#"{"name":".dependency","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/workspace/node_modules/.dependency/index.ts",
            b"export const dot = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/#missing/package.json",
            br##"{"name":"#missing","exports":"./index.js"}"##.to_vec(),
        )
        .file(
            "/workspace/node_modules/#missing/index.ts",
            b"export const fallback = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/#blocked/package.json",
            br##"{"name":"#blocked","exports":"./index.js"}"##.to_vec(),
        )
        .file(
            "/workspace/node_modules/#blocked/index.ts",
            b"export const blocked = true;".to_vec(),
        )
        .file(
            "/workspace/node_modules/#exact/package.json",
            br##"{"name":"#exact","exports":"./index.js"}"##.to_vec(),
        )
        .file(
            "/workspace/node_modules/#exact/index.ts",
            b"export const fallback = true;".to_vec(),
        )
        .build()
        .expect("build non-root package-imports host");

    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let self_target = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#self",
                ResolutionMode::EsNext,
            )
            .expect("resolve non-root imports-to-self target"),
    );
    assert_eq!(
        self_target.resolved_file().canonical().as_path(),
        Path::new("/workspace/index.ts")
    );

    for specifier in ["#x:y", "#x\\y", "#x\0y", "#x/../y"] {
        let exact = resolved(
            resolver
                .resolve(
                    Path::new("/workspace/main.ts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("exact imports keys are looked up before target validation"),
        );
        assert_eq!(
            exact.resolved_file().canonical().as_path(),
            Path::new("/workspace/src/exact.ts"),
            "{specifier:?}"
        );
    }

    let dot_package = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#dot",
                ResolutionMode::EsNext,
            )
            .expect("bare imports target beginning with a dot is not a relative target"),
    );
    assert_eq!(
        dot_package.resolved_file().canonical().as_path(),
        Path::new("/workspace/node_modules/.dependency/index.ts")
    );

    let missing_import = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#missing",
                ResolutionMode::EsNext,
            )
            .expect("a missing imports entry continues through node_modules"),
    );
    assert_eq!(
        missing_import.resolved_file().canonical().as_path(),
        Path::new("/workspace/node_modules/#missing/index.ts")
    );
    assert_eq!(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#blocked",
                ResolutionMode::EsNext,
            )
            .expect("an explicit null imports target remains terminal"),
        ResolutionOutcome::NotFound
    );

    let exports_disabled = CompilerOptions {
        module: Some(199),
        resolve_package_json_exports: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &exports_disabled).expect("create imports-only resolver");
    let exact = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#exact",
                ResolutionMode::EsNext,
            )
            .expect("imports remain enabled independently from exports"),
    );
    assert_eq!(
        exact.resolved_file().canonical().as_path(),
        Path::new("/workspace/src/exact.ts")
    );
    assert_eq!(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "workspace",
                ResolutionMode::EsNext,
            )
            .expect("disabled exports use ordinary legacy lookup"),
        ResolutionOutcome::NotFound,
    );

    let imports_disabled = CompilerOptions {
        module: Some(199),
        resolve_package_json_imports: Some(false),
        ..CompilerOptions::default()
    };
    let mut resolver =
        ModuleResolver::new(&host, &imports_disabled).expect("create exports-only resolver");
    let fallback = resolved(
        resolver
            .resolve(
                Path::new("/workspace/main.ts"),
                "#exact",
                ResolutionMode::EsNext,
            )
            .expect("disabled imports continue through ordinary package lookup"),
    );
    assert_eq!(
        fallback.resolved_file().canonical().as_path(),
        Path::new("/workspace/node_modules/#exact/index.ts")
    );
}

#[test]
fn root_imports_patterns_are_gated_for_node16_but_enabled_elsewhere() {
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/package.json",
            br##"{"name":"root","imports":{"#/*":"./src/*"}}"##.to_vec(),
        )
        .file("/index.ts", b"export {};".to_vec())
        .file("/src/foo.ts", b"export const foo = true;".to_vec())
        .build()
        .expect("build root-wildcard imports host");

    let node16_options = options_for_module(100);
    let mut node16 = ModuleResolver::new(&host, &node16_options).expect("create Node16 resolver");
    assert_eq!(
        node16
            .resolve(Path::new("/index.ts"), "#/foo.ts", ResolutionMode::EsNext,)
            .expect("Node16 rejects root imports patterns as an authoritative miss"),
        ResolutionOutcome::NotFound
    );

    let node_next_options = options_for_module(199);
    let mut node_next =
        ModuleResolver::new(&host, &node_next_options).expect("create NodeNext resolver");
    assert_eq!(
        resolved(
            node_next
                .resolve(Path::new("/index.ts"), "#/foo.ts", ResolutionMode::EsNext,)
                .expect("NodeNext enables root imports patterns"),
        )
        .resolved_file()
        .canonical()
        .as_path(),
        Path::new("/src/foo.ts")
    );

    let bundler_options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut bundler =
        ModuleResolver::new(&host, &bundler_options).expect("create Bundler resolver");
    assert_eq!(
        resolved(
            bundler
                .resolve(Path::new("/index.ts"), "#/foo.ts", ResolutionMode::EsNext,)
                .expect("Bundler enables root imports patterns"),
        )
        .resolved_file()
        .canonical()
        .as_path(),
        Path::new("/src/foo.ts")
    );
}

#[test]
fn relative_node_modules_targets_are_external_without_realpath_rewriting() {
    let forbidden_realpath = HostError::new(
        HostErrorKind::Other,
        HostOperation::Realpath,
        Some(PathBuf::from("/node_modules/pkg/other.ts")),
        "relative resolution must not rewrite through realpath",
    );
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/node_modules/pkg/package.json",
            br#"{"name":"pkg","type":"module"}"#.to_vec(),
        )
        .file("/node_modules/pkg/index.ts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/other.ts",
            b"export const other = true;".to_vec(),
        )
        .failure(forbidden_realpath)
        .build()
        .expect("build relative node_modules host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/pkg/index.ts"),
                "./other.js",
                ResolutionMode::EsNext,
            )
            .expect("relative package source resolves without realpath"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/node_modules/pkg/other.ts")
    );
    assert!(module.is_external_library_import());
    assert_eq!(module.original_path(), None);
}

#[test]
fn untyped_exports_retain_the_esm_legacy_alternate_and_package_facts() {
    let host = MemoryCompilerHost::builder("/")
        .file("/main.mts", b"export {};".to_vec())
        .file("/main.cts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "exports":{"./foo":"./dist/foo.js"},
                "typesVersions":{"*":{"foo":["./types/foo.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/pkg/dist/foo.js",
            b"module.exports = {};".to_vec(),
        )
        .file("/node_modules/pkg/types/foo.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/no-alternate/package.json",
            br#"{
                "name":"no-alternate",
                "exports":"./dist/index.js"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/no-alternate/dist/index.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/no-alternate/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build untyped package host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let esm = resolved(
        resolver
            .resolve(Path::new("/main.mts"), "pkg/foo", ResolutionMode::EsNext)
            .expect("resolve ESM implementation"),
    );
    assert_eq!(esm.extension(), &ModuleExtension::Js);
    assert_eq!(
        esm.alternate_result()
            .expect("ESM implementation has a legacy alternate")
            .canonical()
            .as_path(),
        Path::new("/node_modules/pkg/types/foo.d.ts")
    );
    assert_eq!(esm.package_id().map(PackageId::name), Some("pkg"));

    let commonjs = resolved(
        resolver
            .resolve(Path::new("/main.cts"), "pkg/foo", ResolutionMode::CommonJs)
            .expect("resolve CommonJS implementation"),
    );
    assert_eq!(commonjs.extension(), &ModuleExtension::Js);
    assert_eq!(commonjs.alternate_result(), None);

    let no_alternate = resolved(
        resolver
            .resolve(
                Path::new("/main.mts"),
                "no-alternate",
                ResolutionMode::EsNext,
            )
            .expect("resolve an exports implementation without a legacy type target"),
    );
    assert_eq!(no_alternate.extension(), &ModuleExtension::Js);
    assert_eq!(no_alternate.alternate_result(), None);
}

#[test]
fn null_exports_use_legacy_index_while_other_falsy_values_and_overlaps_fail_closed() {
    for (package, exports) in [("empty", "\"\""), ("falsey", "false"), ("zero", "0")] {
        let package_json = format!(r#"{{"name":"{package}","exports":{exports}}}"#);
        let package_path = format!("/work/node_modules/{package}/package.json");
        let host = MemoryCompilerHost::builder("/work")
            .file("/work/index.mts", b"export {};".to_vec())
            .file(package_path, package_json.into_bytes())
            .build()
            .expect("build falsy-exports host");
        let options = options_for_module(199);
        let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
        let error = resolver
            .resolve(
                Path::new("/work/index.mts"),
                package,
                ResolutionMode::EsNext,
            )
            .expect_err("falsy exports requires the unported legacy fallback");
        assert_unsupported(error, "legacy-node-package-entry-from-falsy-exports");
    }

    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/nullish/package.json",
            br#"{"name":"nullish","exports":null}"#.to_vec(),
        )
        .file(
            "/work/node_modules/nullish/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build null-exports legacy-index host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let nullish = resolved(
        resolver
            .resolve(
                Path::new("/work/index.mts"),
                "nullish",
                ResolutionMode::EsNext,
            )
            .expect("null exports permits the Node ESM package-root index exception"),
    );
    assert_eq!(
        nullish.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/nullish/index.d.ts")
    );

    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/overlap/package.json",
            br#"{"name":"overlap","exports":{"./aba*aba":"./index.js"}}"#.to_vec(),
        )
        .build()
        .expect("build overlapping-pattern host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let error = resolver
        .resolve(
            Path::new("/work/index.mts"),
            "overlap/aba",
            ResolutionMode::EsNext,
        )
        .expect_err("overlapping prefix and suffix must not become a supported miss");
    assert_unsupported(error, "overlapping-package-exports-pattern");
}

#[test]
fn external_walk_prefers_types_across_ancestors_and_continues_after_null() {
    let host = MemoryCompilerHost::builder("/work/project")
        .file("/work/project/src/index.mts", b"export {};".to_vec())
        .file(
            "/work/project/src/node_modules/inner/package.json",
            br#"{
                "name":"inner",
                "exports": {
                    "./typed":"./near.js",
                    "./blocked":null
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/project/src/node_modules/inner/near.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/package.json",
            br#"{
                "name":"inner",
                "exports": {
                    "./typed":"./outer.js",
                    "./blocked":"./outer.js"
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/outer.d.ts",
            b"export const typed: true;".to_vec(),
        )
        .build()
        .expect("build nested node_modules tree");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["inner/typed", "inner/blocked"] {
        let resolution = resolved(
            resolver
                .resolve(
                    Path::new("/work/project/src/index.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("resolve across node_modules ancestors"),
        );
        assert_eq!(resolution.extension(), &ModuleExtension::Dts);
        assert_eq!(
            resolution.resolved_file().canonical().as_path(),
            Path::new("/work/project/node_modules/inner/outer.d.ts")
        );
    }
}

#[test]
fn a_manifestless_near_package_miss_continues_to_an_outer_node_modules() {
    let host = MemoryCompilerHost::builder("/work/project")
        .file("/work/project/src/index.mts", b"export {};".to_vec())
        .file(
            "/work/project/src/node_modules/inner/placeholder.txt",
            b"legacy package".to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/package.json",
            br#"{"name":"inner","exports":{"./x":"./x.js"}}"#.to_vec(),
        )
        .file(
            "/work/project/node_modules/inner/x.d.ts",
            b"export const x: true;".to_vec(),
        )
        .build()
        .expect("build legacy-shadowing package tree");
    let options = options_for_module(100);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/project/src/index.mts"),
                "inner/x",
                ResolutionMode::EsNext,
            )
            .expect("a manifestless miss continues the ancestor walk"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/project/node_modules/inner/x.d.ts")
    );
}

#[test]
fn at_types_fallback_preserves_declaration_only_conditions_and_scoped_names() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/inner/package.json",
            br#"{
                "name":"@types/inner",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "import":"./index.d.mts",
                        "require":"./index.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/@types/inner/index.d.mts",
            b"export const mode: 'import';".to_vec(),
        )
        .file(
            "/work/node_modules/@types/inner/index.d.cts",
            b"export const mode: 'require';".to_vec(),
        )
        .file(
            "/work/node_modules/@types/scope__pkg/package.json",
            br#"{"name":"@types/scope__pkg","version":"2.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/@types/scope__pkg/index.d.ts",
            b"export const scoped: true;".to_vec(),
        )
        .build()
        .expect("build @types fallback tree");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (mode, expected, extension) in [
        (
            ResolutionMode::EsNext,
            "/work/node_modules/@types/inner/index.d.mts",
            ModuleExtension::Dmts,
        ),
        (
            ResolutionMode::CommonJs,
            "/work/node_modules/@types/inner/index.d.cts",
            ModuleExtension::Dcts,
        ),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/work/index.mts"), "inner", mode)
                .expect("resolve conditional @types fallback"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(module.extension(), &extension);
        assert_eq!(
            module.package_id().map(PackageId::name),
            Some("@types/inner")
        );
    }

    let scoped = resolved(
        resolver
            .resolve(
                Path::new("/work/index.mts"),
                "@scope/pkg",
                ResolutionMode::EsNext,
            )
            .expect("resolve a mangled scoped @types fallback"),
    );
    assert_eq!(
        scoped.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/@types/scope__pkg/index.d.ts")
    );
    assert_eq!(
        scoped.package_id().map(PackageId::name),
        Some("@types/scope__pkg")
    );
    assert!(scoped.is_external_library_import());
}

#[test]
fn at_types_fallback_is_declaration_only_for_manifestless_packages() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/pkg/index.ts",
            b"export const wrong: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/pkg/index.d.ts",
            b"export const right: true;".to_vec(),
        )
        .build()
        .expect("build manifestless @types package");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(Path::new("/work/main.ts"), "pkg", ResolutionMode::CommonJs)
            .expect("resolve declaration-only @types fallback"),
    );
    assert_eq!(
        module.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/@types/pkg/index.d.ts")
    );
    assert_eq!(module.extension(), &ModuleExtension::Dts);
    assert_eq!(module.package_id(), None);
}

#[test]
fn type_reference_primary_custom_roots_use_direct_then_directory_precedence() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/types/direct.d.ts",
            b"declare const direct: true;".to_vec(),
        )
        .file(
            "/work/types/direct/index.d.ts",
            b"declare const wrongDirectory: true;".to_vec(),
        )
        .file(
            "/work/types/versioned/package.json",
            br#"{
                "name":"versioned-types",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{"*":{"index":["v6/index"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/work/types/versioned/index.d.ts",
            b"declare const wrongVersion: true;".to_vec(),
        )
        .file(
            "/work/types/versioned/v6/index.d.ts",
            b"declare const versioned: true;".to_vec(),
        )
        .file(
            "/work/types/twinned/package.json",
            br#"{"name":"twinned-types","version":"1.0.0","types":"index.ts"}"#.to_vec(),
        )
        .file(
            "/work/types/twinned/index.ts",
            b"declare const wrongImplementation: true;".to_vec(),
        )
        .file(
            "/work/types/twinned/index.d.ts",
            b"declare const declarationTwin: true;".to_vec(),
        )
        .file(
            "/work/types/esm-index/package.json",
            br#"{"name":"esm-index-types","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/types/esm-index/index.d.ts",
            b"declare const cjsOnlyIndex: true;".to_vec(),
        )
        .file(
            "/work/types/esm-manifestless/index.d.ts",
            b"declare const cjsOnlyManifestlessIndex: true;".to_vec(),
        )
        .build()
        .expect("build custom typeRoots tree");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let type_root = ProgramPath::from_trusted_parts("/work/types", "/work/types")
        .expect("create custom type root");

    let ResolutionOutcome::Resolved(direct) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "direct",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve direct custom-root declaration")
    else {
        panic!("expected direct custom-root type reference");
    };
    assert_eq!(
        direct.resolved_file().canonical().as_path(),
        Path::new("/work/types/direct.d.ts")
    );
    assert!(direct.primary());
    assert!(!direct.is_external_library_import());

    let ResolutionOutcome::Resolved(versioned) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "versioned",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve directory package through custom type root")
    else {
        panic!("expected versioned custom-root type reference");
    };
    assert_eq!(
        versioned.resolved_file().canonical().as_path(),
        Path::new("/work/types/versioned/v6/index.d.ts")
    );
    assert!(versioned.primary());
    assert_eq!(
        versioned.package_id().map(PackageId::name),
        Some("versioned-types")
    );
    let bound_versioned = versioned
        .clone()
        .into_resolved_type_reference_directive(
            versioned.resolved_file().clone(),
            SourceFileId::from_raw(7),
        )
        .expect("bind the primary package-backed type reference");
    assert!(bound_versioned.primary());
    assert_eq!(bound_versioned.source(), SourceFileId::from_raw(7));
    assert_eq!(
        bound_versioned.package_id().map(PackageId::name),
        Some("versioned-types")
    );

    let ResolutionOutcome::Resolved(twinned) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "twinned",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve a package-field declaration twin before its implementation")
    else {
        panic!("expected twinned custom-root type reference");
    };
    assert_eq!(
        twinned.resolved_file().canonical().as_path(),
        Path::new("/work/types/twinned/index.d.ts")
    );

    let esm_options = options_for_module(199);
    let mut esm_resolver =
        ModuleResolver::new(&host, &esm_options).expect("create Node ESM resolver");
    for specifier in ["esm-index", "esm-manifestless"] {
        assert_eq!(
            esm_resolver
                .resolve_type_reference(
                    Path::new("/work/main.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                    Some(std::slice::from_ref(&type_root)),
                )
                .expect("Node ESM primary directory lookup is an ordinary miss"),
            ResolutionOutcome::NotFound
        );
    }
}

#[test]
fn non_external_custom_type_roots_retain_realpath_transitions() {
    let declaration = b"declare const linked: true;".to_vec();
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file("/work/types/linked.d.ts", declaration.clone())
        .file("/actual/linked.d.ts", declaration)
        .realpath("/work/types/linked.d.ts", "/actual/linked.d.ts")
        .build()
        .expect("build custom typeRoot symlink");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let type_root = ProgramPath::from_trusted_parts("/work/types", "/work/types")
        .expect("create custom type root");

    let ResolutionOutcome::Resolved(reference) = resolver
        .resolve_type_reference(
            Path::new("/work/main.ts"),
            "linked",
            ResolutionMode::CommonJs,
            Some(std::slice::from_ref(&type_root)),
        )
        .expect("resolve custom-root symlink")
    else {
        panic!("expected custom-root symlink type reference");
    };
    assert_eq!(
        reference.resolved_file().canonical().as_path(),
        Path::new("/actual/linked.d.ts")
    );
    assert_eq!(
        reference
            .original_path()
            .expect("lexical path")
            .canonical()
            .as_path(),
        Path::new("/work/types/linked.d.ts")
    );
    assert!(reference.primary());
    assert!(!reference.is_external_library_import());
    let caller_spelling = ProgramPath::from_trusted_parts(
        "actual/linked.d.ts",
        reference.resolved_file().canonical().as_path(),
    )
    .expect("create caller-owned target spelling");
    let bound = reference
        .clone()
        .into_resolved_type_reference_directive(caller_spelling, SourceFileId::from_raw(11))
        .expect("bind canonical target identity across display spellings");
    assert_eq!(bound.target().display(), Path::new("actual/linked.d.ts"));
    assert_eq!(bound.source(), SourceFileId::from_raw(11));
    assert_eq!(
        bound
            .original_path()
            .expect("bound directive retains lexical path")
            .canonical()
            .as_path(),
        Path::new("/work/types/linked.d.ts")
    );
    assert!(bound.primary());
    assert!(!bound.is_external_library_import());

    let mismatched = ProgramPath::from_trusted_parts("/other.d.ts", "/other.d.ts")
        .expect("create mismatched target");
    assert!(matches!(
        reference.into_resolved_type_reference_directive(mismatched, SourceFileId::from_raw(12)),
        Err(ResolutionError::InvalidData(_))
    ));
}

#[test]
fn type_reference_default_roots_and_secondary_lookup_preserve_spelling_and_origin() {
    let host = MemoryCompilerHost::builder("/work/project")
        .case_sensitive(true)
        .file("/work/project/src/main.ts", b"export {};".to_vec())
        .file(
            "/work/project/node_modules/@types/defaulted/package.json",
            br#"{"name":"@types/defaulted","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/project/node_modules/@types/defaulted/index.d.ts",
            b"declare const defaulted: true;".to_vec(),
        )
        .file(
            "/work/project/src/node_modules/secondary/package.json",
            br#"{"name":"secondary","version":"1.0.0","types":"index.d.ts"}"#.to_vec(),
        )
        .file(
            "/work/project/src/node_modules/secondary/index.d.ts",
            b"declare const secondary: true;".to_vec(),
        )
        .build()
        .expect("build default and secondary type-reference tree");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let ResolutionOutcome::Resolved(defaulted) = resolver
        .resolve_type_reference(
            Path::new("/work/project/src/main.ts"),
            "defaulted",
            ResolutionMode::CommonJs,
            None,
        )
        .expect("resolve from current-directory default type root")
    else {
        panic!("expected default-root type reference");
    };
    assert!(defaulted.primary());
    assert!(defaulted.is_external_library_import());
    assert_eq!(
        defaulted.resolved_file().canonical().as_path(),
        Path::new("/work/project/node_modules/@types/defaulted/index.d.ts")
    );
    let bound_defaulted = defaulted
        .clone()
        .into_resolved_type_reference_directive(
            defaulted.resolved_file().clone(),
            SourceFileId::from_raw(13),
        )
        .expect("bind an external default-root type reference");
    assert!(bound_defaulted.primary());
    assert!(bound_defaulted.is_external_library_import());

    let no_primary_roots: Vec<ProgramPath> = Vec::new();
    let ResolutionOutcome::Resolved(secondary) = resolver
        .resolve_type_reference(
            Path::new("/work/project/src/main.ts"),
            "secondary",
            ResolutionMode::CommonJs,
            Some(&no_primary_roots),
        )
        .expect("resolve from nearest secondary node_modules")
    else {
        panic!("expected secondary type reference");
    };
    assert!(!secondary.primary());
    assert_eq!(
        secondary.resolved_file().canonical().as_path(),
        Path::new("/work/project/src/node_modules/secondary/index.d.ts")
    );

    assert_eq!(
        resolver
            .resolve_type_reference(
                Path::new("/work/project/src/main.ts"),
                "DEFAULTED",
                ResolutionMode::CommonJs,
                None,
            )
            .expect("case-sensitive miss remains an ordinary miss"),
        ResolutionOutcome::NotFound
    );

    assert_eq!(
        resolver
            .resolve_type_reference(
                Path::new("/work/project/__inferred type names__.ts"),
                "defaulted",
                ResolutionMode::Unspecified,
                Some(&no_primary_roots),
            )
            .expect("custom automatic roots suppress secondary lookup"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn type_reference_secondary_at_types_exports_preserve_import_and_require_modes() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/main.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/mode/package.json",
            br#"{
                "name":"@types/mode",
                "version":"1.0.0",
                "exports":{
                    ".":{
                        "import":"./index.d.mts",
                        "require":"./index.d.cts"
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/node_modules/@types/mode/index.d.mts",
            b"export const mode: 'import';".to_vec(),
        )
        .file(
            "/work/node_modules/@types/mode/index.d.cts",
            b"export const mode: 'require';".to_vec(),
        )
        .build()
        .expect("build conditional type-reference package");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let no_primary_roots: Vec<ProgramPath> = Vec::new();

    for (mode, expected, extension) in [
        (
            ResolutionMode::EsNext,
            "/work/node_modules/@types/mode/index.d.mts",
            ModuleExtension::Dmts,
        ),
        (
            ResolutionMode::CommonJs,
            "/work/node_modules/@types/mode/index.d.cts",
            ModuleExtension::Dcts,
        ),
    ] {
        let ResolutionOutcome::Resolved(reference) = resolver
            .resolve_type_reference(
                Path::new("/work/main.ts"),
                "mode",
                mode,
                Some(&no_primary_roots),
            )
            .expect("resolve conditional secondary type reference")
        else {
            panic!("expected conditional secondary type reference");
        };
        assert_eq!(
            reference.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(reference.extension(), &extension);
        assert!(!reference.primary());
        assert_eq!(
            reference.package_id().map(PackageId::name),
            Some("@types/mode")
        );
    }
}

#[test]
fn manifest_and_target_directory_host_failures_propagate() {
    let manifest_failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::FileExists,
        Some(PathBuf::from("/work/node_modules/manifest/package.json")),
        "manifest existence denied",
    );
    let manifest_host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/manifest/package.json",
            br#"{"name":"manifest","exports":"./index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/manifest/index.d.ts",
            b"export const manifest: true;".to_vec(),
        )
        .failure(manifest_failure.clone())
        .build()
        .expect("build manifest-failure host");
    let options = options_for_module(199);
    let mut resolver =
        ModuleResolver::new(&manifest_host, &options).expect("create manifest resolver");
    let error = resolver
        .resolve(
            Path::new("/work/index.mts"),
            "manifest",
            ResolutionMode::EsNext,
        )
        .expect_err("manifest fileExists failure must propagate");
    let ResolutionError::Host(actual) = error else {
        panic!("expected manifest host error, got {error:?}");
    };
    assert_eq!(actual, manifest_failure);

    let directory_failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::DirectoryExists,
        Some(PathBuf::from("/work/node_modules/target/nested")),
        "target directory denied",
    );
    let target_host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/target/package.json",
            br#"{"name":"target","exports":"./nested/index.js"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/target/nested/index.d.ts",
            b"export const target: true;".to_vec(),
        )
        .failure(directory_failure.clone())
        .build()
        .expect("build target-directory-failure host");
    let mut resolver = ModuleResolver::new(&target_host, &options).expect("create target resolver");
    let error = resolver
        .resolve(
            Path::new("/work/index.mts"),
            "target",
            ResolutionMode::EsNext,
        )
        .expect_err("target directory failure must propagate");
    let ResolutionError::Host(actual) = error else {
        panic!("expected target directory host error, got {error:?}");
    };
    assert_eq!(actual, directory_failure);
}

#[test]
fn self_references_skip_external_realpath_and_case_only_realpaths_stay_lexical() {
    let realpath_failure = HostError::new(
        HostErrorKind::Other,
        HostOperation::Realpath,
        Some(PathBuf::from("/node_modules/inner/index.d.ts")),
        "realpath must not run for a self-reference",
    );
    let host = MemoryCompilerHost::builder("/")
        .file(
            "/node_modules/inner/package.json",
            br#"{"name":"inner","exports":{"./x":"./index.js"}}"#.to_vec(),
        )
        .file("/node_modules/inner/test.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/inner/index.d.ts",
            b"export const x: true;".to_vec(),
        )
        .failure(realpath_failure)
        .build()
        .expect("build self-reference host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let self_reference = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/inner/test.d.ts"),
                "inner/x",
                ResolutionMode::CommonJs,
            )
            .expect("self-reference does not query realpath"),
    );
    assert_eq!(self_reference.original_path(), None);

    let insensitive = MemoryCompilerHost::builder("/")
        .case_sensitive(false)
        .file(
            "/Node_Modules/Inner/package.json",
            br#"{"name":"inner","exports":{"./x":"./index.js"}}"#.to_vec(),
        )
        .file(
            "/Node_Modules/Inner/index.d.ts",
            b"export const x: true;".to_vec(),
        )
        .build()
        .expect("build case-insensitive host");
    let mut resolver = ModuleResolver::new(&insensitive, &options).expect("create resolver");
    let external = resolved(
        resolver
            .resolve(Path::new("/index.mts"), "inner/x", ResolutionMode::EsNext)
            .expect("resolve case-insensitive external package"),
    );
    assert_eq!(external.original_path(), None);
    assert_eq!(
        external.resolved_file().display(),
        Path::new("/node_modules/inner/index.d.ts")
    );
}

#[test]
fn self_reference_misses_continue_to_node_modules_but_null_stays_terminal() {
    let host = MemoryCompilerHost::builder("/work/package")
        .file(
            "/work/package/package.json",
            br#"{
                "name":"same-name",
                "exports": {
                    "./missing-file":"./missing.js",
                    "./blocked":null
                }
            }"#
            .to_vec(),
        )
        .file("/work/package/src/index.mts", b"export {};".to_vec())
        .file(
            "/work/package/node_modules/same-name/package.json",
            br#"{
                "name":"same-name",
                "exports": {
                    "./unmapped":"./index.js",
                    "./missing-file":"./index.js",
                    "./blocked":"./index.js"
                }
            }"#
            .to_vec(),
        )
        .file(
            "/work/package/node_modules/same-name/index.d.ts",
            b"export const external: true;".to_vec(),
        )
        .build()
        .expect("build self-reference fallback host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["same-name/unmapped", "same-name/missing-file"] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/package/src/index.mts"),
                    specifier,
                    ResolutionMode::EsNext,
                )
                .expect("ordinary self-reference miss falls through"),
        );
        assert!(module.is_external_library_import());
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new("/work/package/node_modules/same-name/index.d.ts")
        );
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/package/src/index.mts"),
                "same-name/blocked",
                ResolutionMode::EsNext,
            )
            .expect("explicit null self-reference is an authoritative miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn directory_export_targets_require_a_trailing_slash_before_appending_subpaths() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{"name":"pkg","exports":{"./foo/":"./bar.js"}}"#.to_vec(),
        )
        // Concatenating the invalid target and subpath would produce this
        // false hit (`./bar.js` + `x` => `./bar.jsx`).
        .file("/node_modules/pkg/bar.jsx", b"export {};".to_vec())
        .build()
        .expect("build invalid directory-target host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    assert_eq!(
        resolver
            .resolve(Path::new("/index.mts"), "pkg/foo/x", ResolutionMode::EsNext,)
            .expect("invalid directory target is an ordinary miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn empty_export_array_blocks_later_matching_conditions() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "exports": {
                    ".": {
                        "types":[],
                        "default":"./index.js"
                    }
                }
            }"#
            .to_vec(),
        )
        .file("/node_modules/pkg/index.d.ts", b"export {};".to_vec())
        .build()
        .expect("build empty-array condition host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    assert_eq!(
        resolver
            .resolve(Path::new("/index.mts"), "pkg", ResolutionMode::EsNext)
            .expect("empty active-condition array is terminal"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn types_versions_explicit_extensions_probe_exactly_before_loader_substitution() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/pkg/package.json",
            br#"{
                "name":"pkg",
                "version":"1.0.0",
                "typesVersions": {
                    "*": {
                        "prefer-exact":["./types/prefer.js"],
                        "exact-declaration":["./types/exact.d.ts"],
                        "fallback-after-miss":["./types/fallback.js"],
                        "missing":["./types/missing.js"]
                    }
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/pkg/types/prefer.js",
            b"module.exports = {};".to_vec(),
        )
        // This declaration used to win the preferred pass before the exact
        // JavaScript substitution was checked.
        .file(
            "/node_modules/pkg/types/prefer.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/pkg/types/exact.d.ts", b"export {};".to_vec())
        // An exact miss still enters the ordinary package loader, where the
        // written .js extension may substitute its declaration twin.
        .file(
            "/node_modules/pkg/types/fallback.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build explicit typesVersions target host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let exact_js = resolved(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/prefer-exact",
                ResolutionMode::EsNext,
            )
            .expect("resolve exact JavaScript substitution first"),
    );
    assert_eq!(exact_js.extension(), &ModuleExtension::Js);
    assert_eq!(
        exact_js.resolved_file().canonical().as_path(),
        Path::new("/node_modules/pkg/types/prefer.js")
    );
    assert_eq!(exact_js.package_id(), None);

    let exact_declaration = resolved(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/exact-declaration",
                ResolutionMode::EsNext,
            )
            .expect("resolve exact declaration substitution"),
    );
    assert_eq!(exact_declaration.extension(), &ModuleExtension::Dts);
    assert_eq!(exact_declaration.package_id(), None);

    let fallback = resolved(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/fallback-after-miss",
                ResolutionMode::EsNext,
            )
            .expect("fall through after an exact substitution miss"),
    );
    assert_eq!(fallback.extension(), &ModuleExtension::Dts);
    assert_eq!(fallback.package_id().map(PackageId::name), Some("pkg"));

    assert_eq!(
        resolver
            .resolve(
                Path::new("/index.mts"),
                "pkg/missing",
                ResolutionMode::EsNext,
            )
            .expect("missing exact and loader candidates are authoritative miss"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn manifestless_node_modules_probe_direct_files_and_commonjs_indexes_without_fake_facts() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.cts", b"export {};".to_vec())
        .file(
            "/work/node_modules/plain/direct.ts",
            b"export const direct = true;".to_vec(),
        )
        .file(
            "/work/node_modules/plain/folder/index.d.ts",
            b"export const folder: true;".to_vec(),
        )
        .file(
            "/work/node_modules/root-index/index.d.ts",
            b"export const root: true;".to_vec(),
        )
        .build()
        .expect("build manifestless node_modules host");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    let direct = resolved(
        resolver
            .resolve(
                Path::new("/work/index.cts"),
                "plain/direct.ts",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an explicit manifestless TypeScript subpath"),
    );
    assert_eq!(
        direct.resolved_file().canonical().as_path(),
        Path::new("/work/node_modules/plain/direct.ts")
    );
    assert!(direct.resolved_using_ts_extension());
    assert!(direct.is_external_library_import());
    assert_eq!(direct.package_id(), None);
    assert_eq!(direct.package_metadata(), None);

    for (specifier, expected) in [
        ("plain/folder", "/work/node_modules/plain/folder/index.d.ts"),
        ("root-index", "/work/node_modules/root-index/index.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(
                    Path::new("/work/index.cts"),
                    specifier,
                    ResolutionMode::CommonJs,
                )
                .expect("resolve a CommonJS manifestless index"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        assert_eq!(module.package_id(), None);
        assert_eq!(module.package_metadata(), None);
    }

    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/index.cts"),
                "plain/folder",
                ResolutionMode::EsNext,
            )
            .expect("Node ESM does not perform a manifestless directory lookup"),
        ResolutionOutcome::NotFound
    );
}

#[test]
fn legacy_package_fields_preserve_priority_nonrecursive_main_and_node_esm_directory_rules() {
    let host = MemoryCompilerHost::builder("/")
        .file("/index.mts", b"export {};".to_vec())
        .file(
            "/node_modules/priority/package.json",
            br#"{
                "name":"priority",
                "version":"1.0.0",
                "typings":"typings.d.ts",
                "types":"types.d.ts",
                "main":"main.js"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/priority/typings.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/priority/types.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/priority/main.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/package.json",
            br#"{
                "name":"first-field-miss",
                "typings":"missing.d.ts",
                "types":"must-not-win.d.ts",
                "main":"must-not-win.js"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/must-not-win.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/must-not-win.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/first-field-miss/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/direct-root/package.json",
            br#"{
                "name":"direct-root",
                "version":"1.0.0",
                "type":"module",
                "types":"index.d.ts"
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/direct-root/index.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/direct-root.ts", b"export {};".to_vec())
        .file(
            "/node_modules/nonrecursive/package.json",
            br#"{"name":"nonrecursive","main":"nested"}"#.to_vec(),
        )
        .file(
            "/node_modules/nonrecursive/nested/package.json",
            br#"{"main":"actual"}"#.to_vec(),
        )
        .file(
            "/node_modules/nonrecursive/nested/actual.js",
            b"module.exports = {};".to_vec(),
        )
        .file(
            "/node_modules/mode/package.json",
            br#"{
                "name":"mode",
                "version":"1.0.0",
                "type":"module",
                "main":"dist/index.js"
            }"#
            .to_vec(),
        )
        .file("/node_modules/mode/dist/index.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/mode/dist/dir/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build legacy package-field host");

    let bundler_options = CompilerOptions {
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut bundler =
        ModuleResolver::new(&host, &bundler_options).expect("create Bundler resolver");
    let priority = resolved(
        bundler
            .resolve(Path::new("/index.mts"), "priority", ResolutionMode::EsNext)
            .expect("typings wins over types and main"),
    );
    assert_eq!(
        priority.resolved_file().canonical().as_path(),
        Path::new("/node_modules/priority/typings.d.ts")
    );
    assert_eq!(priority.package_id().map(PackageId::name), Some("priority"));
    let first_field_miss = resolved(
        bundler
            .resolve(
                Path::new("/index.mts"),
                "first-field-miss",
                ResolutionMode::EsNext,
            )
            .expect("a selected typings miss falls through to index, not types or main"),
    );
    assert_eq!(
        first_field_miss.resolved_file().canonical().as_path(),
        Path::new("/node_modules/first-field-miss/index.d.ts")
    );
    let direct_root = resolved(
        bundler
            .resolve(
                Path::new("/index.mts"),
                "direct-root",
                ResolutionMode::CommonJs,
            )
            .expect("CommonJS probes the direct package-root file before package fields"),
    );
    assert_eq!(
        direct_root.resolved_file().canonical().as_path(),
        Path::new("/node_modules/direct-root.ts")
    );
    let direct_root_id = direct_root
        .package_id()
        .expect("the package-root loader attaches the manifest package id");
    assert_eq!(direct_root_id.name(), "direct-root");
    assert_eq!(direct_root_id.submodule_name(), "ts");
    assert_eq!(
        bundler
            .resolve(
                Path::new("/index.mts"),
                "nonrecursive",
                ResolutionMode::CommonJs,
            )
            .expect("a main target does not recursively consume nested package.json"),
        ResolutionOutcome::NotFound
    );

    let node_options = options_for_module(100);
    let mut node = ModuleResolver::new(&host, &node_options).expect("create Node16 resolver");
    let root = resolved(
        node.resolve(Path::new("/index.mts"), "mode", ResolutionMode::EsNext)
            .expect("an explicit main target resolves in Node ESM mode"),
    );
    assert_eq!(
        root.resolved_file().canonical().as_path(),
        Path::new("/node_modules/mode/dist/index.d.ts")
    );
    assert_eq!(
        node.resolve(
            Path::new("/index.mts"),
            "mode/dist/dir",
            ResolutionMode::EsNext,
        )
        .expect("Node ESM forbids package-subpath directory lookup"),
        ResolutionOutcome::NotFound
    );
    let commonjs_directory = resolved(
        node.resolve(
            Path::new("/index.mts"),
            "mode/dist/dir",
            ResolutionMode::CommonJs,
        )
        .expect("Node CommonJS permits package-subpath directory lookup"),
    );
    assert_eq!(
        commonjs_directory.resolved_file().canonical().as_path(),
        Path::new("/node_modules/mode/dist/dir/index.d.ts")
    );
}

#[test]
fn types_versions_root_back_references_unmapped_fallback_and_mapped_misses_are_distinct() {
    let host = MemoryCompilerHost::builder("/")
        .file("/main.ts", b"export {};".to_vec())
        .file(
            "/node_modules/ext/package.json",
            br#"{
                "name":"ext",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{">=3.1.0-0":{"*":["ts3.1/*"]}}
            }"#
            .to_vec(),
        )
        .file("/node_modules/ext/index.d.ts", b"export {};".to_vec())
        .file("/node_modules/ext/other.d.ts", b"export {};".to_vec())
        .file("/node_modules/ext/ts3.1/index.d.ts", b"export {};".to_vec())
        .file("/node_modules/ext/ts3.1/other.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/unmapped/package.json",
            br#"{
                "name":"unmapped",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{">=3.1.0-0":{"index":["ts3.1/index"]}}
            }"#
            .to_vec(),
        )
        .file("/node_modules/unmapped/index.d.ts", b"export {};".to_vec())
        .file("/node_modules/unmapped/other.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/unmapped/ts3.1/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/mapped-miss/package.json",
            br#"{
                "name":"mapped-miss",
                "types":"index",
                "typesVersions":{"*":{"*":["missing/*","also-missing/*"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/mapped-miss/index.d.ts",
            b"export {};".to_vec(),
        )
        .file("/node_modules/mapped-miss/foo.d.ts", b"export {};".to_vec())
        .file(
            "/node_modules/first-range/package.json",
            br#"{
                "name":"first-range",
                "version":"1.0.0",
                "types":"index",
                "typesVersions":{
                    "*":{"index":["first/index"]},
                    ">=3.1":{"index":["second/index"]}
                }
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/first-range/first/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/first-range/second/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/node_modules/root-exact/package.json",
            br#"{
                "name":"root-exact",
                "version":"1.0.0",
                "types":"index.d.ts",
                "typesVersions":{"*":{"index.d.ts":["types/root.d.ts"]}}
            }"#
            .to_vec(),
        )
        .file(
            "/node_modules/root-exact/types/root.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build typesVersions host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, expected) in [
        ("ext", "/node_modules/ext/ts3.1/index.d.ts"),
        ("ext/other", "/node_modules/ext/ts3.1/other.d.ts"),
        ("unmapped", "/node_modules/unmapped/ts3.1/index.d.ts"),
        ("unmapped/other", "/node_modules/unmapped/other.d.ts"),
        ("first-range", "/node_modules/first-range/first/index.d.ts"),
        ("root-exact", "/node_modules/root-exact/types/root.d.ts"),
    ] {
        let module = resolved(
            resolver
                .resolve(Path::new("/main.ts"), specifier, ResolutionMode::CommonJs)
                .expect("resolve versioned or legacy package target"),
        );
        assert_eq!(
            module.resolved_file().canonical().as_path(),
            Path::new(expected)
        );
        let expected_package = specifier.split('/').next().expect("package name");
        assert_eq!(
            module.package_id().map(PackageId::name),
            Some(expected_package)
        );
    }

    let self_back_reference = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/ext/ts3.1/index.d.ts"),
                "../",
                ResolutionMode::CommonJs,
            )
            .expect("a package-root back-reference re-enters typesVersions"),
    );
    assert_eq!(
        self_back_reference.resolved_file().canonical().as_path(),
        Path::new("/node_modules/ext/ts3.1/index.d.ts")
    );
    assert_eq!(
        self_back_reference.package_id().map(PackageId::name),
        Some("ext")
    );
    let root_other = resolved(
        resolver
            .resolve(
                Path::new("/node_modules/ext/ts3.1/other.d.ts"),
                "../other",
                ResolutionMode::CommonJs,
            )
            .expect("an extensionless relative file resolves before directory metadata"),
    );
    assert_eq!(
        root_other.resolved_file().canonical().as_path(),
        Path::new("/node_modules/ext/other.d.ts")
    );
    assert_eq!(root_other.package_id().map(PackageId::name), Some("ext"));

    for specifier in ["mapped-miss", "mapped-miss/foo"] {
        assert_eq!(
            resolver
                .resolve(Path::new("/main.ts"), specifier, ResolutionMode::CommonJs)
                .expect("a selected mapping owns an all-target miss"),
            ResolutionOutcome::NotFound,
            "{specifier} must not fall through to its legacy file"
        );
    }
}

#[test]
fn relative_package_ids_follow_file_and_directory_manifest_boundaries() {
    let host = MemoryCompilerHost::builder("/work")
        .file(
            "/work/package.json",
            br#"{"name":"workspace","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/index.ts", b"export {};".to_vec())
        .file("/work/other.ts", b"export {};".to_vec())
        .file(
            "/work/directory/package.json",
            br#"{"name":"directory","version":"1.0.0"}"#.to_vec(),
        )
        .file("/work/directory/index.d.ts", b"export {};".to_vec())
        .file(
            "/work/node_modules/outer/package.json",
            br#"{"name":"outer","version":"1.0.0"}"#.to_vec(),
        )
        .file(
            "/work/node_modules/outer/index.d.ts",
            b"export {};".to_vec(),
        )
        .file(
            "/work/node_modules/outer/nested/index.d.ts",
            b"export {};".to_vec(),
        )
        .build()
        .expect("build relative workspace host");
    let options = options_for_module(1);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");
    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                "./other",
                ResolutionMode::CommonJs,
            )
            .expect("resolve an ordinary relative source"),
    );
    assert_eq!(module.package_id(), None);
    assert!(!module.is_external_library_import());

    let directory_package = resolved(
        resolver
            .resolve(
                Path::new("/work/index.ts"),
                "./directory/",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a relative directory with its own package manifest"),
    );
    assert_eq!(
        directory_package.package_id().map(PackageId::name),
        Some("directory")
    );
    assert!(!directory_package.is_external_library_import());

    let manifestless_directory = resolved(
        resolver
            .resolve(
                Path::new("/work/node_modules/outer/index.d.ts"),
                "./nested/",
                ResolutionMode::CommonJs,
            )
            .expect("resolve a manifestless directory index inside a package"),
    );
    assert_eq!(manifestless_directory.package_id(), None);
    assert!(manifestless_directory.is_external_library_import());
}

#[test]
fn relative_json_requires_an_explicit_suffix_and_effective_json_resolution() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/root.ts", b"export {};".to_vec())
        .file("/work/data.json", br#"{"value":1}"#.to_vec())
        .build()
        .expect("build relative JSON host");
    let enabled = CompilerOptions {
        module: Some(99),
        module_resolution: Some(100),
        ..CompilerOptions::default()
    };
    let mut resolver = ModuleResolver::new(&host, &enabled).expect("create Bundler resolver");

    let module = resolved(
        resolver
            .resolve(
                Path::new("/work/root.ts"),
                "./data.json",
                ResolutionMode::EsNext,
            )
            .expect("resolve an explicitly named JSON module"),
    );
    assert_eq!(module.extension(), &ModuleExtension::Json);
    assert_eq!(
        module.resolved_file().display(),
        Path::new("/work/data.json")
    );
    assert!(!module.is_external_library_import());
    assert!(!module.resolved_using_ts_extension());
    assert_eq!(module.original_path(), None);
    assert_eq!(
        resolver
            .resolve(Path::new("/work/root.ts"), "./data", ResolutionMode::EsNext,)
            .expect("extensionless relative requests exclude JSON"),
        ResolutionOutcome::NotFound
    );

    let disabled = CompilerOptions {
        resolve_json_module: Some(false),
        ..enabled
    };
    let mut resolver = ModuleResolver::new(&host, &disabled).expect("create disabled resolver");
    assert_eq!(
        resolver
            .resolve(
                Path::new("/work/root.ts"),
                "./data.json",
                ResolutionMode::EsNext,
            )
            .expect("disabled JSON resolution is an authoritative miss"),
        ResolutionOutcome::NotFound
    );
}
