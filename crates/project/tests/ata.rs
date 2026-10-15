//! tsgo `discovertypings_test.go` and `validatepackagename_test.go`: the
//! typings a project's files need, and npm's package name rules.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_project::ata::{
    discover_typings, validate_package_name, CachedTyping, NameValidationResult, NpmExecutor,
    TypeAcquisition, TypingsInfo, TypingsInstallRequest, TypingsInstaller,
};
use tsc_types::{Version, TYPESCRIPT_VERSION_MAJOR_MINOR};

/// tsgo `projecttestutil.TestTypingsLocation`.
const TEST_TYPINGS_LOCATION: &str = "/home/src/Library/Caches/typescript";

fn fs(files: &[(&str, &str)]) -> MemFs {
    MemFs::from_entries(
        files.iter().map(|(name, text)| (*name, Seed::file(*text))),
        false,
        Arc::new(SystemClock),
    )
    .unwrap()
}

fn typings_info(unresolved_imports: &[&str]) -> TypingsInfo {
    TypingsInfo {
        type_acquisition: TypeAcquisition {
            enable: Some(true),
            ..TypeAcquisition::default()
        },
        allow_js: false,
        has_types_option: false,
        unresolved_imports: (!unresolved_imports.is_empty()).then(|| {
            unresolved_imports
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        }),
    }
}

/// tsgo `projecttestutil.TypesRegistryConfig`.
fn types_registry_config() -> BTreeMap<String, String> {
    [
        ("latest", "1.3.0"),
        ("ts2.0", "1.0.0"),
        ("ts2.1", "1.0.0"),
        ("ts2.2", "1.2.0"),
        ("ts2.3", "1.3.0"),
        ("ts2.4", "1.3.0"),
        ("ts2.5", "1.3.0"),
        ("ts2.6", "1.3.0"),
        ("ts2.7", "1.3.0"),
    ]
    .map(|(tag, version)| (tag.to_owned(), version.to_owned()))
    .into()
}

fn cached(location: &str, version: &str) -> CachedTyping {
    CachedTyping {
        typings_location: location.to_owned(),
        version: Version::parse(version).unwrap(),
    }
}

struct Discovered {
    cached_typing_paths: Vec<String>,
    new_typing_names: BTreeSet<String>,
    files_to_watch: Vec<String>,
}

fn discover(
    files: &[(&str, &str)],
    info: &TypingsInfo,
    file_names: &[&str],
    cache: &BTreeMap<String, CachedTyping>,
    registry: &BTreeMap<String, BTreeMap<String, String>>,
) -> Discovered {
    let fs = fs(files);
    let file_names = file_names
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let (cached_typing_paths, new_typing_names, files_to_watch) = discover_typings(
        &fs,
        &mut |_| {},
        info,
        &file_names,
        "/home/src/projects/project",
        cache,
        registry,
    );
    Discovered {
        cached_typing_paths,
        new_typing_names: new_typing_names.into_iter().collect(),
        files_to_watch,
    }
}

fn names(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

const WATCHED: [&str; 2] = [
    "/home/src/projects/project/bower_components",
    "/home/src/projects/project/node_modules",
];

#[test]
fn should_use_mappings_from_safe_list() {
    let discovered = discover(
        &[
            ("/home/src/projects/project/app.js", ""),
            ("/home/src/projects/project/jquery.js", ""),
            ("/home/src/projects/project/chroma.min.js", ""),
        ],
        &typings_info(&[]),
        &[
            "/home/src/projects/project/app.js",
            "/home/src/projects/project/jquery.js",
            "/home/src/projects/project/chroma.min.js",
        ],
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["jquery", "chroma-js"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_return_node_for_core_modules() {
    let discovered = discover(
        &[("/home/src/projects/project/app.js", "")],
        &typings_info(&["assert", "somename"]),
        &["/home/src/projects/project/app.js"],
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["node", "somename"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_use_cached_locations() {
    let cache = BTreeMap::from([(
        "node".to_owned(),
        cached("/home/src/projects/project/node.d.ts", "1.3.0"),
    )]);
    let discovered = discover(
        &[
            ("/home/src/projects/project/app.js", ""),
            ("/home/src/projects/project/node.d.ts", ""),
        ],
        &typings_info(&["fs", "bar"]),
        &["/home/src/projects/project/app.js"],
        &cache,
        &BTreeMap::from([("node".to_owned(), types_registry_config())]),
    );
    assert_eq!(
        discovered.cached_typing_paths,
        ["/home/src/projects/project/node.d.ts"]
    );
    assert_eq!(discovered.new_typing_names, names(&["bar"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_gracefully_handle_packages_that_have_been_removed_from_the_types_registry() {
    let cache = BTreeMap::from([(
        "node".to_owned(),
        cached("/home/src/projects/project/node.d.ts", "1.3.0"),
    )]);
    let discovered = discover(
        &[
            ("/home/src/projects/project/app.js", ""),
            ("/home/src/projects/project/node.d.ts", ""),
        ],
        &typings_info(&["fs", "bar"]),
        &["/home/src/projects/project/app.js"],
        &cache,
        &BTreeMap::new(),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["node", "bar"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_search_only_2_levels_deep() {
    let discovered = discover(
        &[
            ("/home/src/projects/project/app.js", ""),
            (
                "/home/src/projects/project/node_modules/a/package.json",
                r#"{ "name": "a" }"#,
            ),
            (
                "/home/src/projects/project/node_modules/a/b/package.json",
                r#"{ "name": "b" }"#,
            ),
        ],
        &typings_info(&[]),
        &["/home/src/projects/project/app.js"],
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["a"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_support_scoped_packages() {
    let discovered = discover(
        &[
            ("/home/src/projects/project/app.js", ""),
            (
                "/home/src/projects/project/node_modules/@a/b/package.json",
                r#"{ "name": "@a/b" }"#,
            ),
        ],
        &typings_info(&[]),
        &["/home/src/projects/project/app.js"],
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["@a/b"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_install_expired_typings() {
    let cache = BTreeMap::from([
        (
            "node".to_owned(),
            cached(
                &format!("{TEST_TYPINGS_LOCATION}/node_modules/@types/node/index.d.ts"),
                "1.3.0",
            ),
        ),
        (
            "commander".to_owned(),
            cached(
                &format!("{TEST_TYPINGS_LOCATION}/node_modules/@types/commander/index.d.ts"),
                "1.0.0",
            ),
        ),
    ]);
    let discovered = discover(
        &[("/home/src/projects/project/app.js", "")],
        &typings_info(&["http", "commander"]),
        &["/home/src/projects/project/app.js"],
        &cache,
        &BTreeMap::from([
            ("node".to_owned(), types_registry_config()),
            ("commander".to_owned(), types_registry_config()),
        ]),
    );
    assert_eq!(
        discovered.cached_typing_paths,
        ["/home/src/Library/Caches/typescript/node_modules/@types/node/index.d.ts"]
    );
    assert_eq!(discovered.new_typing_names, names(&["commander"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn should_install_expired_typings_with_prerelease_version_of_tsserver() {
    let cache = BTreeMap::from([(
        "node".to_owned(),
        cached(
            &format!("{TEST_TYPINGS_LOCATION}/node_modules/@types/node/index.d.ts"),
            "1.0.0",
        ),
    )]);
    let mut config = types_registry_config();
    config.remove(&format!("ts{TYPESCRIPT_VERSION_MAJOR_MINOR}"));
    let discovered = discover(
        &[("/home/src/projects/project/app.js", "")],
        &typings_info(&["http"]),
        &["/home/src/projects/project/app.js"],
        &cache,
        &BTreeMap::from([("node".to_owned(), config)]),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["node"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn prerelease_typings_are_properly_handled() {
    let cache = BTreeMap::from([
        (
            "node".to_owned(),
            cached(
                &format!("{TEST_TYPINGS_LOCATION}/node_modules/@types/node/index.d.ts"),
                "1.3.0-next.0",
            ),
        ),
        (
            "commander".to_owned(),
            cached(
                &format!("{TEST_TYPINGS_LOCATION}/node_modules/@types/commander/index.d.ts"),
                "1.3.0-next.0",
            ),
        ),
    ]);
    let mut config = types_registry_config();
    config.insert(
        format!("ts{TYPESCRIPT_VERSION_MAJOR_MINOR}"),
        "1.3.0-next.1".to_owned(),
    );
    let discovered = discover(
        &[("/home/src/projects/project/app.js", "")],
        &typings_info(&["http", "commander"]),
        &["/home/src/projects/project/app.js"],
        &cache,
        &BTreeMap::from([
            ("node".to_owned(), config),
            ("commander".to_owned(), types_registry_config()),
        ]),
    );
    assert!(discovered.cached_typing_paths.is_empty());
    assert_eq!(discovered.new_typing_names, names(&["node", "commander"]));
    assert_eq!(discovered.files_to_watch, WATCHED);
}

#[test]
fn versions_order_as_semver_precedence() {
    let version = |text| Version::parse(text).unwrap();
    assert!(version("1.3.0-next.0") < version("1.3.0-next.1"));
    assert!(version("1.3.0-next.1") < version("1.3.0"));
    assert!(version("1.2") < version("1.3.0"));
    assert_eq!(version("1.3.0+build.1"), version("1.3.0+build.2"));
    for invalid in [
        "",
        "01.0.0",
        "1.0.0-",
        "1.0-beta",
        "1.0.0-01",
        "1.0.0-1abc",
        "a.b.c",
    ] {
        assert_eq!(Version::parse(invalid), None, "{invalid}");
    }
}

#[test]
fn a_package_name_cannot_be_too_long() {
    let mut name = "a".to_owned();
    for _ in 0..8 {
        name = name.repeat(2);
    }
    assert_eq!(
        validate_package_name(&name).0,
        NameValidationResult::NameTooLong
    );
}

#[test]
fn a_package_name_cannot_start_with_a_dot_or_an_underscore() {
    assert_eq!(
        validate_package_name(".foo").0,
        NameValidationResult::NameStartsWithDot
    );
    assert_eq!(
        validate_package_name("_foo").0,
        NameValidationResult::NameStartsWithUnderscore
    );
}

#[test]
fn non_uri_safe_characters_are_not_supported() {
    for name in ["  scope  ", "; say ‘Hello from TypeScript!’ #", "a/b/c"] {
        assert_eq!(
            validate_package_name(name).0,
            NameValidationResult::NameContainsNonUriSafeCharacters,
            "{name}"
        );
    }
}

#[test]
fn scoped_package_names_are_validated_by_their_parts() {
    let check = |package_name: &str, result, name: &str, is_scope_name| {
        assert_eq!(
            validate_package_name(package_name),
            (result, name.to_owned(), is_scope_name),
            "{package_name}"
        );
    };
    use NameValidationResult::*;
    check("@scope/bar", NameOk, "", false);
    check("@.scope/bar", NameStartsWithDot, ".scope", true);
    check("@.scope/.bar", NameStartsWithDot, ".scope", true);
    check("@_scope/bar", NameStartsWithUnderscore, "_scope", true);
    check("@_scope/_bar", NameStartsWithUnderscore, "_scope", true);
    check(
        "@  scope  /bar",
        NameContainsNonUriSafeCharacters,
        "  scope  ",
        true,
    );
    check(
        "@; say ‘Hello from TypeScript!’ #/bar",
        NameContainsNonUriSafeCharacters,
        "; say ‘Hello from TypeScript!’ #",
        true,
    );
    check(
        "@  scope  /  bar  ",
        NameContainsNonUriSafeCharacters,
        "  scope  ",
        true,
    );
    check("@scope/.bar", NameStartsWithDot, ".bar", false);
    check("@scope/_bar", NameStartsWithUnderscore, "_bar", false);
    check(
        "@scope/  bar  ",
        NameContainsNonUriSafeCharacters,
        "  bar  ",
        false,
    );
    check(
        "@scope/; say ‘Hello from TypeScript!’ #",
        NameContainsNonUriSafeCharacters,
        "; say ‘Hello from TypeScript!’ #",
        false,
    );
}

/// tsgo `SetupNpmExecutorForTypingsInstaller`: npm writes the registry (the
/// known packages with tsgo's test versions) and each `@types` package's
/// file into the cache.
struct NpmMock {
    fs: Arc<MemFs>,
    types_registry: Vec<String>,
    package_to_file: BTreeMap<String, String>,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl NpmExecutor for NpmMock {
    fn npm_install(&self, cwd: &str, args: &[String]) -> Result<Vec<u8>, String> {
        self.calls
            .lock()
            .unwrap()
            .push((cwd.to_owned(), args.to_vec()));
        if args.len() == 3 && args[2] == "types-registry@latest" {
            let config = types_registry_config();
            let mut packages = self.types_registry.clone();
            packages.extend(self.package_to_file.keys().cloned());
            let entries = packages
                .iter()
                .map(|name| (name.clone(), config.clone()))
                .collect::<BTreeMap<_, _>>();
            let text = serde_json::json!({ "entries": entries }).to_string();
            self.fs
                .write_creating_dirs(
                    &format!("{cwd}/node_modules/types-registry/index.json"),
                    text.as_bytes(),
                )
                .map_err(|error| error.to_string())?;
            return Ok(Vec::new());
        }
        let end = args
            .iter()
            .skip(2)
            .position(|arg| arg.starts_with("--"))
            .map_or(args.len(), |index| index + 2);
        for package in &args[2..end] {
            let package = package
                .rsplit_once('@')
                .map_or(package.as_str(), |(name, _)| name);
            let name = &package["@types/".len()..];
            let content = self
                .package_to_file
                .get(name)
                .ok_or_else(|| format!("content not provided for {name}"))?;
            self.fs
                .write_creating_dirs(
                    &format!("{cwd}/node_modules/@types/{name}/index.d.ts"),
                    content.as_bytes(),
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(Vec::new())
    }
}

/// tsgo's "inferred projects" at the installer: the registry installed,
/// then jQuery's typings for the manifest's dependency, whose file the
/// cache resolves.
#[test]
fn the_installer_installs_a_manifests_typings() {
    let fs = Arc::new(fs(&[
        ("/user/username/projects/project/app.js", ""),
        (
            "/user/username/projects/project/package.json",
            r#"{ "name": "test", "dependencies": { "jquery": "^3.1.0" } }"#,
        ),
    ]));
    let npm = Arc::new(NpmMock {
        fs: Arc::clone(&fs),
        types_registry: Vec::new(),
        package_to_file: BTreeMap::from([(
            "jquery".to_owned(),
            "declare const $: { x: number }".to_owned(),
        )]),
        calls: Mutex::default(),
    });
    let installer = TypingsInstaller::new(
        TEST_TYPINGS_LOCATION.to_owned(),
        Arc::clone(&fs) as Arc<dyn FileSystem>,
        Arc::clone(&npm) as Arc<dyn NpmExecutor>,
    );
    let mut log = Vec::new();
    let result = installer
        .install_typings(TypingsInstallRequest {
            project_id: "/dev/null/inferred",
            typings_info: &typings_info(&[]),
            file_names: &["/user/username/projects/project/app.js".to_owned()],
            project_root_path: "/user/username/projects/project",
            fs: &*fs,
            logger: &mut |line| log.push(line),
        })
        .unwrap();
    let calls = npm.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2, "{log:#?}");
    assert_eq!(calls[0].0, TEST_TYPINGS_LOCATION);
    assert_eq!(
        calls[0].1,
        ["install", "--ignore-scripts", "types-registry@latest"]
    );
    assert_eq!(calls[1].1[2], "@types/jquery@latest");
    assert_eq!(
        result.typings_files,
        [format!(
            "{TEST_TYPINGS_LOCATION}/node_modules/@types/jquery/index.d.ts"
        )]
    );
    assert_eq!(
        result.files_to_watch,
        [
            "/user/username/projects/project/bower_components",
            "/user/username/projects/project/node_modules",
            "/user/username/projects/project/package.json",
        ]
    );
    // The cache has an up-to-date jQuery now: no install the next time.
    let again = installer
        .install_typings(TypingsInstallRequest {
            project_id: "/dev/null/inferred",
            typings_info: &typings_info(&[]),
            file_names: &["/user/username/projects/project/app.js".to_owned()],
            project_root_path: "/user/username/projects/project",
            fs: &*fs,
            logger: &mut |_| {},
        })
        .unwrap();
    assert_eq!(npm.calls.lock().unwrap().len(), 2);
    assert_eq!(again.typings_files, result.typings_files);
}
