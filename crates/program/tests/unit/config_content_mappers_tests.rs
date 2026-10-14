//! tsgo contentmappers_test.go (`TestResolveContentMapperManifest`) and the
//! helpers the content mapper config parse shares with tsgo's resolver and
//! path functions.

use super::*;
use crate::{parse_package_name, CompilerConfigHost};
use tsc_host::MemoryCompilerHost;

#[test]
fn package_names_split_and_mangle_as_tsgo_s_resolver_does() {
    assert_eq!(parse_package_name("vue-mapper"), ("vue-mapper", ""));
    assert_eq!(parse_package_name("pkg/sub/path"), ("pkg", "sub/path"));
    assert_eq!(parse_package_name("@scope/name"), ("@scope/name", ""));
    assert_eq!(
        parse_package_name("@scope/name/sub"),
        ("@scope/name", "sub")
    );
    assert_eq!(parse_package_name("@scope"), ("@scope", ""));
    let mangle = |name: &str| {
        crate::mangle_scoped_package_name(JsStr::from_str(name))
            .to_string_lossy()
            .into_owned()
    };
    assert_eq!(mangle("@scope/name"), "scope__name");
    assert_eq!(mangle("@scope"), "@scope");
    assert_eq!(mangle("plain"), "plain");
}

#[test]
fn equal_fold_follows_go_simple_case_folding() {
    assert!(equal_fold(".ts", ".TS"));
    assert!(equal_fold(".ts", ".tſ"));
    assert!(equal_fold(".vue", ".VUE"));
    assert!(!equal_fold(".ts", ".tsx"));
    assert!(!equal_fold(".ts", ".t"));
}

/// tsgo path_test.go `TestGetLongestExtensionFromPath`, and the file name's
/// spelling coming back on a case-insensitive host.
#[test]
fn the_longest_registered_extension_wins() {
    let extensions = [".z", ".y.z"];
    let longest = |file_name: &str, case_sensitive| {
        longest_extension(
            JsStr::from(file_name),
            extensions.iter().copied(),
            case_sensitive,
        )
    };
    assert_eq!(longest("/src/Component.y.z", true).as_deref(), Some(".y.z"));
    assert_eq!(longest("/src/Component.z", true).as_deref(), Some(".z"));
    assert_eq!(longest("/src/Component.Y.Z", true), None);
    assert_eq!(
        longest("/src/Component.Y.Z", false).as_deref(),
        Some(".Y.Z")
    );
    assert_eq!(longest("/src/Componentz", true), None);
}

fn host(files: &[(&str, &str)]) -> MemoryCompilerHost {
    files
        .iter()
        .fold(
            MemoryCompilerHost::builder("/").case_sensitive(true),
            |builder, (path, text)| builder.file(path, text.as_bytes()),
        )
        .build()
        .unwrap()
}

fn resolve(host: &MemoryCompilerHost, containing_file: &str, package: &str) -> ManifestResolution {
    resolve_content_mapper_manifest(
        &CompilerConfigHost::new(host),
        JsStr::from(containing_file),
        package,
    )
    .unwrap()
}

/// tsgo `TestResolveContentMapperManifest`.
#[test]
fn manifests_resolve_from_node_modules_and_report_what_is_missing() {
    let host = host(&[
        (
            "/home/project/node_modules/vue-ts-mapper/package.json",
            r#"{
                "name": "vue-ts-mapper",
                "version": "1.2.3",
                "typescript": { "contentMapper": { "exec": ["node", "./dist/mapper.js"], "compilerOptions": ["target", "jsx"] } }
            }"#,
        ),
        (
            "/home/node_modules/@scope/noversion/package.json",
            r#"{ "name": "@scope/noversion", "typescript": { "contentMapper": { "exec": ["run"] } } }"#,
        ),
        (
            "/home/project/node_modules/no-name/package.json",
            r#"{ "version": "1.0.0" }"#,
        ),
        (
            "/home/project/node_modules/no-manifest/package.json",
            r#"{ "name": "no-manifest" }"#,
        ),
        (
            "/home/project/node_modules/no-exec/package.json",
            r#"{ "name": "no-exec", "typescript": { "contentMapper": {} } }"#,
        ),
        (
            "/home/project/node_modules/bad-exec/package.json",
            r#"{ "name": "bad-exec", "typescript": { "contentMapper": { "exec": "node ./mapper.js" } } }"#,
        ),
    ]);

    // Name, version and the verbatim exec argv are kept.
    let resolution = resolve(&host, "/home/project/tsconfig.json", "vue-ts-mapper");
    assert_eq!(
        resolution.package_directory,
        "/home/project/node_modules/vue-ts-mapper"
    );
    assert_eq!(
        resolution.manifest,
        Ok(Manifest {
            name: "vue-ts-mapper".to_owned(),
            version: "1.2.3".to_owned(),
            exec: Some(vec!["node".to_owned(), "./dist/mapper.js".to_owned()]),
            compiler_options: Some(vec!["target".to_owned(), "jsx".to_owned()]),
            dynamic_config: false,
        })
    );

    // Resolution walks up node_modules; a package with no version resolves
    // to its name and an empty version.
    let resolution = resolve(&host, "/home/project/src/tsconfig.json", "@scope/noversion");
    let manifest = resolution.manifest.unwrap();
    assert_eq!(
        (manifest.name.as_str(), manifest.version.as_str()),
        ("@scope/noversion", "")
    );

    let failure = |package: &str| resolve(&host, "/home/project/tsconfig.json", package);
    assert_eq!(
        failure("missing-mapper").manifest,
        Err(&gen::The_content_mapper_package_0_could_not_be_resolved)
    );
    let no_name = failure("no-name");
    assert_eq!(
        no_name.package_directory,
        "/home/project/node_modules/no-name"
    );
    assert_eq!(
        no_name.manifest,
        Err(&gen::The_package_json_of_the_content_mapper_package_0_does_not_specify_a_name)
    );
    assert_eq!(
        failure("no-manifest").manifest,
        Err(&gen::The_package_json_of_the_content_mapper_package_0_does_not_declare_a_typescript_contentMapper_object)
    );
    for package in ["no-exec", "bad-exec"] {
        assert_eq!(
            failure(package).manifest,
            Err(&gen::The_typescript_contentMapper_exec_of_the_content_mapper_package_0_must_be_a_non_empty_array_of_strings),
            "{package}"
        );
    }
}

/// tsgo `ResolvePackageDirectory`: `@types` when the package itself is not
/// in a `node_modules`, the real path of a symlinked package, and a
/// package.json that is not JSON.
#[test]
fn package_directories_follow_types_and_symlinks() {
    let host = MemoryCompilerHost::builder("/")
        .case_sensitive(true)
        .file(
            "/p/node_modules/@types/scope__typed/package.json",
            br#"{ "name": "typed", "typescript": { "contentMapper": { "exec": ["t"] } } }"#.to_vec(),
        )
        .file(
            "/store/linked@1.0.0/package.json",
            br#"{ "name": "linked", "typescript": { "contentMapper": { "exec": ["l"], "dynamicConfig": true } } }"#.to_vec(),
        )
        .directory("/p/node_modules/linked")
        .realpath("/p/node_modules/linked", "/store/linked@1.0.0")
        .file(
            "/p/node_modules/broken/package.json",
            b"{ \"name\": \"broken\", }".to_vec(),
        )
        .build()
        .unwrap();
    let typed = resolve(&host, "/p/tsconfig.json", "@scope/typed");
    assert_eq!(
        typed.package_directory,
        "/p/node_modules/@types/scope__typed"
    );
    assert_eq!(typed.manifest.unwrap().name, "typed");
    let linked = resolve(&host, "/p/tsconfig.json", "linked");
    assert_eq!(linked.package_directory, "/store/linked@1.0.0");
    assert!(linked.manifest.unwrap().dynamic_config);
    assert_eq!(
        resolve(&host, "/p/tsconfig.json", "broken").manifest,
        Err(&gen::The_package_json_of_the_content_mapper_package_0_could_not_be_parsed)
    );
}
