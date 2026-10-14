//! tsgo `GetEntrypointsFromPackageJsonInfo`: a package's entrypoints and
//! their module specifiers, as the auto-import registry reads them.

use std::collections::BTreeSet;

use tsc_diagnostics::JsStr;
use tsc_host::MemoryCompilerHost;
use tsc_program::{CompilerOptions, EntrypointEnding, ModuleResolver, ResolvedEntrypoint};

fn entrypoints(
    files: &[(&str, &str)],
    realpaths: &[(&str, &str)],
    package_directory: &str,
    package_name: &str,
    enable_directory_search: bool,
) -> Vec<ResolvedEntrypoint> {
    let mut builder = MemoryCompilerHost::builder("/repo");
    for (path, text) in files {
        builder = builder.file(*path, text.as_bytes().to_vec());
    }
    for (path, target) in realpaths {
        builder = builder.realpath(*path, *target);
    }
    let host = builder.build().expect("a host");
    let options = CompilerOptions::default();
    let mut resolver = ModuleResolver::new(&host, &options).expect("a resolver");
    resolver
        .get_entrypoints_from_package_json_info(
            JsStr::from_str(package_directory),
            package_name,
            enable_directory_search,
        )
        .expect("entrypoints")
}

fn conditions(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn summary(entrypoints: &[ResolvedEntrypoint]) -> Vec<(String, String, EntrypointEnding)> {
    entrypoints
        .iter()
        .map(|entrypoint| {
            (
                entrypoint.module_specifier.clone(),
                entrypoint.resolved_file_name.clone(),
                entrypoint.ending,
            )
        })
        .collect()
}

#[test]
fn main_entry_and_directory_search() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","types":"lib/index.d.ts"}"#,
        ),
        ("/repo/node_modules/pkg/lib/index.d.ts", "export {};"),
        ("/repo/node_modules/pkg/lib/util.d.ts", "export {};"),
        (
            "/repo/node_modules/pkg/node_modules/dep/index.d.ts",
            "export {};",
        ),
        ("/repo/node_modules/pkg/readme.md", ""),
    ];
    let main_only = entrypoints(&files, &[], "/repo/node_modules/pkg", "pkg", false);
    assert_eq!(
        summary(&main_only),
        [(
            "pkg".to_owned(),
            "/repo/node_modules/pkg/lib/index.d.ts".to_owned(),
            EntrypointEnding::Fixed
        )]
    );
    // The search skips the main entry and nested node_modules.
    let searched = entrypoints(&files, &[], "/repo/node_modules/pkg", "pkg", true);
    assert_eq!(
        summary(&searched),
        [
            (
                "pkg".to_owned(),
                "/repo/node_modules/pkg/lib/index.d.ts".to_owned(),
                EntrypointEnding::Fixed
            ),
            (
                "pkg/lib/util.d.ts".to_owned(),
                "/repo/node_modules/pkg/lib/util.d.ts".to_owned(),
                EntrypointEnding::Changeable
            ),
        ]
    );
}

#[test]
fn a_package_without_package_json_has_its_index() {
    let files = [("/repo/node_modules/pkg/index.d.ts", "export {};")];
    let found = entrypoints(&files, &[], "/repo/node_modules/pkg", "pkg", false);
    assert_eq!(
        summary(&found),
        [(
            "pkg".to_owned(),
            "/repo/node_modules/pkg/index.d.ts".to_owned(),
            EntrypointEnding::Fixed
        )]
    );
}

#[test]
fn exports_conditions_include_and_exclude() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{
                ".":{"node":"./node.d.ts","import":"./import.d.ts","default":"./default.d.ts"},
                "./sub":{"types":"./sub.d.ts","import":"./never.d.ts"}
            }}"#,
        ),
        ("/repo/node_modules/pkg/node.d.ts", "export {};"),
        ("/repo/node_modules/pkg/import.d.ts", "export {};"),
        ("/repo/node_modules/pkg/default.d.ts", "export {};"),
        ("/repo/node_modules/pkg/sub.d.ts", "export {};"),
        ("/repo/node_modules/pkg/never.d.ts", "export {};"),
    ];
    let found = entrypoints(&files, &[], "/repo/node_modules/pkg", "pkg", true);
    let rows: Vec<_> = found
        .iter()
        .map(|entrypoint| {
            (
                entrypoint.module_specifier.as_str(),
                entrypoint.resolved_file_name.as_str(),
                entrypoint.include_conditions.clone(),
                entrypoint.exclude_conditions.clone(),
            )
        })
        .collect();
    // tsgo excludes the earlier conditions of an object only from the later
    // conditions that do not always match: `default` keeps the exclusions
    // `import` saw, not `import` itself.
    assert_eq!(
        rows,
        [
            (
                "pkg",
                "/repo/node_modules/pkg/node.d.ts",
                conditions(&["node"]),
                conditions(&[])
            ),
            (
                "pkg",
                "/repo/node_modules/pkg/import.d.ts",
                conditions(&["import"]),
                conditions(&["node"])
            ),
            (
                "pkg",
                "/repo/node_modules/pkg/default.d.ts",
                conditions(&[]),
                conditions(&["node"])
            ),
            (
                "pkg/sub",
                "/repo/node_modules/pkg/sub.d.ts",
                conditions(&[]),
                conditions(&[])
            ),
        ]
    );
    assert!(found
        .iter()
        .all(|entrypoint| entrypoint.ending == EntrypointEnding::Fixed));
}

#[test]
fn exports_patterns_match_files_by_their_javascript_extension() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{"./*":"./dist/*.js","./features/*":"./features/*","./index*":"./index*.d.ts"}}"#,
        ),
        ("/repo/node_modules/pkg/dist/a.d.ts", "export {};"),
        ("/repo/node_modules/pkg/dist/nested/b.d.ts", "export {};"),
        ("/repo/node_modules/pkg/features/c.d.ts", "export {};"),
        ("/repo/node_modules/pkg/index.d.ts", "export {};"),
    ];
    let found = entrypoints(&files, &[], "/repo/node_modules/pkg", "pkg", false);
    assert_eq!(
        summary(&found),
        [
            (
                "pkg/a".to_owned(),
                "/repo/node_modules/pkg/dist/a.d.ts".to_owned(),
                EntrypointEnding::Fixed
            ),
            (
                "pkg/nested/b".to_owned(),
                "/repo/node_modules/pkg/dist/nested/b.d.ts".to_owned(),
                EntrypointEnding::Fixed
            ),
            (
                "pkg/features/c.d.ts".to_owned(),
                "/repo/node_modules/pkg/features/c.d.ts".to_owned(),
                EntrypointEnding::ExtensionChangeable
            ),
        ],
        "a star inside a file name (`./index*.d.ts`) finds no file to match"
    );
}

#[test]
fn unsafe_and_non_relative_targets_are_no_entrypoints() {
    let files = [
        (
            "/repo/node_modules/pkg/package.json",
            r#"{"name":"pkg","exports":{"./a":"./x/../a.d.ts","./b":"b.d.ts","./c":"./node_modules/c.d.ts","./d":"./d/","./e":"./*/*.d.ts"}}"#,
        ),
        ("/repo/node_modules/pkg/a.d.ts", "export {};"),
        ("/repo/node_modules/pkg/b.d.ts", "export {};"),
        ("/repo/node_modules/pkg/node_modules/c.d.ts", "export {};"),
        ("/repo/node_modules/pkg/d/index.d.ts", "export {};"),
    ];
    assert!(entrypoints(&files, &[], "/repo/node_modules/pkg", "pkg", true).is_empty());
}

#[test]
fn a_symlinked_entrypoint_is_at_its_real_path() {
    let files = [
        (
            "/repo/packages/lib/package.json",
            r#"{"name":"lib","types":"index.d.ts"}"#,
        ),
        ("/repo/packages/lib/index.d.ts", "export {};"),
        (
            "/repo/node_modules/lib/package.json",
            r#"{"name":"lib","types":"index.d.ts"}"#,
        ),
        ("/repo/node_modules/lib/index.d.ts", "export {};"),
    ];
    let found = entrypoints(
        &files,
        &[(
            "/repo/node_modules/lib/index.d.ts",
            "/repo/packages/lib/index.d.ts",
        )],
        "/repo/node_modules/lib",
        "lib",
        false,
    );
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].original_file_name.as_deref(),
        Some("/repo/node_modules/lib/index.d.ts")
    );
    assert_eq!(found[0].resolved_file_name, "/repo/packages/lib/index.d.ts");
    assert_eq!(found[0].resolved_path, "/repo/packages/lib/index.d.ts");
    assert_eq!(
        found[0].symlink_or_realpath(),
        "/repo/node_modules/lib/index.d.ts"
    );
}
