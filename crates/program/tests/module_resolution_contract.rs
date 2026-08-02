use std::path::{Path, PathBuf};

use tsc_host::{HostError, HostErrorKind, HostOperation, MemoryCompilerHost};
use tsc_program::{
    CompilerOptions, HostResolvedModule, ModuleExtension, ModuleResolver, PackageJsonType,
    ProgramPath, ResolutionError, ResolutionMode, ResolutionOutcome, ResolvedModuleTarget,
    SourceFileId,
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
            assert_eq!(external.package_metadata().name(), Some("inner"));
            assert_eq!(
                external.package_metadata().module_type(),
                PackageJsonType::Unspecified
            );
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
fn host_failures_and_selected_unsupported_targets_do_not_become_not_found() {
    let (host, denied) = fixture_host();
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for (specifier, feature) in [
        ("inner/conditional", "conditional-package-exports"),
        ("inner/array", "package-exports-array"),
    ] {
        let error = resolver
            .resolve(Path::new("/index.ts"), specifier, ResolutionMode::EsNext)
            .expect_err("a selected unsupported target must fail closed");
        assert_unsupported(error, feature);
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
fn unported_legacy_fallback_and_overlapping_patterns_fail_closed() {
    for (package, exports) in [
        ("nullish", "null"),
        ("empty", "\"\""),
        ("falsey", "false"),
        ("zero", "0"),
    ] {
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
fn an_existing_near_package_without_exports_metadata_fails_closed() {
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

    let error = resolver
        .resolve(
            Path::new("/work/project/src/index.mts"),
            "inner/x",
            ResolutionMode::EsNext,
        )
        .expect_err("unported nearer legacy package must stop the walk");
    assert_unsupported(error, "legacy-node-package-entry");
}

#[test]
fn existing_at_types_fallbacks_fail_closed_instead_of_becoming_not_found() {
    let host = MemoryCompilerHost::builder("/work")
        .file("/work/index.mts", b"export {};".to_vec())
        .file(
            "/work/node_modules/@types/inner/index.d.ts",
            b"export const inner: true;".to_vec(),
        )
        .file(
            "/work/node_modules/@types/scope__pkg/index.d.ts",
            b"export const scoped: true;".to_vec(),
        )
        .build()
        .expect("build @types fallback tree");
    let options = options_for_module(199);
    let mut resolver = ModuleResolver::new(&host, &options).expect("create resolver");

    for specifier in ["inner", "@scope/pkg"] {
        let error = resolver
            .resolve(
                Path::new("/work/index.mts"),
                specifier,
                ResolutionMode::EsNext,
            )
            .expect_err("unported @types fallback must fail closed");
        assert_unsupported(error, "node-modules-at-types-fallback");
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
