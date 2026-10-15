//! tsgo `ata_test.go` (`TestATA`): the session acquires the typings its
//! projects' JavaScript needs, with tsgo's npm mock, and the projects take
//! them as roots.

mod common;

use std::collections::BTreeMap;

use common::*;
use tsc_lsproto::{FileChangeType, LanguageKind};

const APP: &str = "file:///user/username/projects/project/app.js";

fn typings(types_registry: &[&str], package_to_file: &[(&str, &str)]) -> TypingsInstallerOptions {
    TypingsInstallerOptions {
        types_registry: types_registry
            .iter()
            .map(|name| (*name).to_owned())
            .collect(),
        package_to_file: package_to_file
            .iter()
            .map(|(name, text)| ((*name).to_owned(), (*text).to_owned()))
            .collect::<BTreeMap<_, _>>(),
    }
}

fn open_app(session: &tsc_project::Session, text: &str) {
    open_as(session, APP, text, LanguageKind::JAVA_SCRIPT);
    session.wait_for_background_tasks();
}

fn registry_call() -> Vec<String> {
    ["install", "--ignore-scripts", "types-registry@latest"]
        .map(str::to_owned)
        .to_vec()
}

fn typings_file(name: &str) -> String {
    format!("{TEST_TYPINGS_LOCATION}/node_modules/@types/{name}/index.d.ts")
}

#[test]
fn a_local_module_is_not_picked_up() {
    // "local module should not be picked up".
    let (session, _, _, npm) = setup_with_typings_installer(
        &[
            (
                "/user/username/projects/project/app.js",
                "const c = require('./config');",
            ),
            (
                "/user/username/projects/project/config.js",
                "export let x = 1",
            ),
            (
                "/user/username/projects/project/jsconfig.json",
                r#"{ "compilerOptions": { "moduleResolution": "commonjs" }, "typeAcquisition": { "enable": true } }"#,
            ),
        ],
        typings(&["config"], &[]),
    );
    open_app(&session, "const c = require('./config');");
    let program = program(&session, APP);
    assert!(source_text(&program, "/user/username/projects/project/config.js").is_some());
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1[2], "types-registry@latest");
}

#[test]
fn configured_projects() {
    let (session, _, client, npm) = setup_with_typings_installer(
        &[
            ("/user/username/projects/project/app.js", ""),
            (
                "/user/username/projects/project/tsconfig.json",
                r#"{ "compilerOptions": { "allowJs": true }, "typeAcquisition": { "enable": true } }"#,
            ),
            (
                "/user/username/projects/project/package.json",
                r#"{ "name": "test", "dependencies": { "jquery": "^3.1.0" } }"#,
            ),
        ],
        typings(&[], &[("jquery", "declare const $: { x: number }")]),
    );
    open_app(&session, "");
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, TEST_TYPINGS_LOCATION);
    assert_eq!(calls[0].1[2], "types-registry@latest");
    assert_eq!(calls[1].0, TEST_TYPINGS_LOCATION);
    assert!(calls[1].1.contains(&"@types/jquery@latest".to_owned()));
    assert_eq!(client.refresh_diagnostics_calls(), 1);
}

#[test]
fn inferred_projects() {
    let (session, _, _, npm) = setup_with_typings_installer(
        &[
            ("/user/username/projects/project/app.js", ""),
            (
                "/user/username/projects/project/package.json",
                r#"{ "name": "test", "dependencies": { "jquery": "^3.1.0" } }"#,
            ),
        ],
        typings(&[], &[("jquery", "declare const $: { x: number }")]),
    );
    open_app(&session, "");
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, TEST_TYPINGS_LOCATION);
    assert_eq!(calls[0].1, registry_call());
    assert_eq!(calls[1].0, TEST_TYPINGS_LOCATION);
    assert_eq!(calls[1].1[2], "@types/jquery@latest");
    let program = program(&session, APP);
    assert!(source_text(&program, &typings_file("jquery")).is_some());
}

#[test]
fn file_name_based_acquisition_can_be_disabled() {
    // "type acquisition with disableFilenameBasedTypeAcquisition:true".
    let (session, _, _, npm) = setup_with_typings_installer(
        &[
            ("/user/username/projects/project/jquery.js", ""),
            (
                "/user/username/projects/project/tsconfig.json",
                r#"{ "compilerOptions": { "allowJs": true }, "typeAcquisition": { "enable": true, "disableFilenameBasedTypeAcquisition": true } }"#,
            ),
        ],
        typings(&["jquery"], &[]),
    );
    open_as(
        &session,
        "file:///user/username/projects/project/jquery.js",
        "",
        LanguageKind::JAVA_SCRIPT,
    );
    session.wait_for_background_tasks();
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, TEST_TYPINGS_LOCATION);
    assert_eq!(calls[0].1, registry_call());
}

/// The project of tsgo's "discover from node_modules" cases with `config`
/// as its jsconfig and `app` as its file.
fn node_modules_project(config: &str, app: &str) -> Vec<(String, String)> {
    [
        ("/user/username/projects/project/app.js", app),
        (
            "/user/username/projects/project/package.json",
            r#"{"dependencies": {"jquery": "1.0.0"}}"#,
        ),
        ("/user/username/projects/project/jsconfig.json", config),
        (
            "/user/username/projects/project/node_modules/commander/index.js",
            "",
        ),
        (
            "/user/username/projects/project/node_modules/commander/package.json",
            r#"{ "name": "commander" }"#,
        ),
        (
            "/user/username/projects/project/node_modules/jquery/index.js",
            "",
        ),
        (
            "/user/username/projects/project/node_modules/jquery/package.json",
            r#"{ "name": "jquery" }"#,
        ),
        (
            "/user/username/projects/project/node_modules/jquery/nested/package.json",
            r#"{ "name": "nested" }"#,
        ),
    ]
    .map(|(name, text)| (name.to_owned(), text.to_owned()))
    .to_vec()
}

fn npm_calls_for(files: &[(String, String)], app: &str) -> Vec<(String, Vec<String>)> {
    let files = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    let (session, _, _, npm) = setup_with_typings_installer(
        &files,
        typings(
            &["nested", "commander"],
            &[("jquery", "declare const jquery: { x: number }")],
        ),
    );
    open_app(&session, app);
    npm.npm_install_calls()
}

#[test]
fn discover_from_node_modules() {
    let calls = npm_calls_for(&node_modules_project("{}", ""), "");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, registry_call());
    assert_eq!(calls[1].0, TEST_TYPINGS_LOCATION);
    assert_eq!(calls[1].1[2], "@types/jquery@latest");
}

#[test]
fn discover_from_node_modules_empty_types() {
    let calls = npm_calls_for(
        &node_modules_project(r#"{"compilerOptions": {"types": []}}"#, ""),
        "",
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, registry_call());
}

#[test]
fn discover_from_node_modules_explicit_types() {
    let calls = npm_calls_for(
        &node_modules_project(r#"{"compilerOptions": {"types": ["jquery"]}}"#, ""),
        "",
    );
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, registry_call());
}

#[test]
fn discover_from_node_modules_empty_types_has_import() {
    let app = r#"import "jquery";"#;
    let calls = npm_calls_for(
        &node_modules_project(r#"{"compilerOptions": {"types": []}}"#, app),
        app,
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, registry_call());
    assert!(calls[1].1.contains(&"@types/jquery@latest".to_owned()));
}

#[test]
fn discover_from_bower_components_and_bower_json() {
    for manifest in [
        (
            "/user/username/projects/project/bower_components/jquery/bower.json",
            r#"{ "name": "jquery" }"#,
        ),
        (
            "/user/username/projects/project/bower.json",
            r#"{ "dependencies": { "jquery": "^3.1.0" } }"#,
        ),
    ] {
        let (session, _, _, npm) = setup_with_typings_installer(
            &[
                ("/user/username/projects/project/app.js", ""),
                ("/user/username/projects/project/jsconfig.json", "{}"),
                (
                    "/user/username/projects/project/bower_components/jquery/index.js",
                    "",
                ),
                manifest,
            ],
            typings(&[], &[("jquery", "declare const jquery: { x: number }")]),
        );
        open_app(&session, "");
        let calls = npm.npm_install_calls();
        assert_eq!(calls.len(), 2, "{}", manifest.0);
        assert_eq!(calls[0].1, registry_call());
        assert_eq!(calls[1].1[2], "@types/jquery@latest");
        let program = program(&session, APP);
        assert!(
            source_text(&program, &typings_file("jquery")).is_some(),
            "{}",
            manifest.0
        );
    }
}

#[test]
fn a_malformed_package_json_is_watched() {
    // "Malformed package.json should be watched".
    let (session, fs, _, npm) = setup_with_typings_installer(
        &[
            ("/user/username/projects/project/app.js", ""),
            (
                "/user/username/projects/project/package.json",
                r#"{"dependencies": { "co } }"#,
            ),
        ],
        typings(&[], &[("commander", "export let x: number")]),
    );
    open_app(&session, "");
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].1, registry_call());
    // Fixed, and the watcher notified.
    use tsc_host::vfs::FileSystem;
    fs.write(
        "/user/username/projects/project/package.json",
        br#"{ "dependencies": { "commander": "0.0.2" } }"#,
    )
    .unwrap();
    session.did_change_watched_files(&[watch_event(
        "file:///user/username/projects/project/package.json",
        FileChangeType::CHANGED,
    )]);
    // The diagnostics refresh, as a request for the document.
    let _ = session.get_snapshot_and_default_project(&uri(APP));
    session.wait_for_background_tasks();
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].1.contains(&"@types/commander@latest".to_owned()));
    let program = program(&session, APP);
    assert!(source_text(&program, &typings_file("commander")).is_some());
}

#[test]
fn a_resolution_to_javascript_is_redone_once_typings_are_installed() {
    // "should redo resolution that resolved to '.js' file after typings are
    // installed".
    let app = "\n                import * as commander from \"commander\";\n            ";
    let (session, _, _, npm) = setup_with_typings_installer(
        &[
            ("/user/username/projects/project/app.js", app),
            (
                "/user/username/projects/node_modules/commander/index.js",
                "module.exports = 0",
            ),
        ],
        typings(&[], &[("commander", "export let commander: number")]),
    );
    open_app(&session, app);
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 2);
    assert!(calls[1].1.contains(&"@types/commander@latest".to_owned()));
    let program = program(&session, APP);
    assert!(source_text(&program, &typings_file("commander")).is_some());
    // The resolution to JavaScript is gone: the name resolves in the
    // typings cache (tsgo `tryResolveFromTypingsLocation`).
    assert!(source_text(
        &program,
        "/user/username/projects/node_modules/commander/index.js"
    )
    .is_none());
    let resolved = program
        .prepared()
        .resolutions()
        .modules()
        .find(|(key, _)| key.specifier() == "commander")
        .and_then(|(_, resolution)| match resolution.outcome() {
            tsc_program::ResolutionOutcome::Resolved(module) => Some(
                module
                    .target()
                    .resolved_file()
                    .display()
                    .to_string_lossy()
                    .into_owned(),
            ),
            tsc_program::ResolutionOutcome::NotFound => None,
        });
    assert_eq!(resolved, Some(typings_file("commander")));
}

/// tsgo's cache entry cases: the cache's jQuery at `version` (in a lock
/// file of version 1, or 3 with `packages`); the registry's is 1.3.0.
fn cached_jquery(version: &str, lockfile3: bool) -> Option<String> {
    let lock = if lockfile3 {
        format!(r#"{{"packages":{{"node_modules/@types/jquery":{{"version":"{version}"}}}}}}"#)
    } else {
        format!(r#"{{"dependencies":{{"@types/jquery":{{"version":"{version}"}}}}}}"#)
    };
    let package = format!(
        r#"{{"dependencies":{{"types-registry":"^0.1.317"}},"devDependencies":{{"@types/jquery":"^{version}"}}}}"#
    );
    let files = [
        (
            "/user/username/projects/project/app.js".to_owned(),
            String::new(),
        ),
        (
            "/user/username/projects/project/package.json".to_owned(),
            r#"{"name":"test","dependencies":{"jquery":"^3.1.0"}}"#.to_owned(),
        ),
        (typings_file("jquery"), "export const x = 10;".to_owned()),
        (format!("{TEST_TYPINGS_LOCATION}/package.json"), package),
        (format!("{TEST_TYPINGS_LOCATION}/package-lock.json"), lock),
    ];
    let files = files
        .iter()
        .map(|(name, text)| (name.as_str(), text.as_str()))
        .collect::<Vec<_>>();
    let typings = if version == "1.0.0" {
        typings(&[], &[("jquery", "export const y = 10")])
    } else {
        typings(&["jquery"], &[])
    };
    let (session, _, _, _) = setup_with_typings_installer(&files, typings);
    open_app(&session, "");
    source_text(&program(&session, APP), &typings_file("jquery"))
}

#[test]
fn an_expired_cache_entry_is_installed() {
    // "expired cache entry (inferred project, should install typings)" and
    // its lockfile3 case.
    for lockfile3 in [false, true] {
        assert_eq!(
            cached_jquery("1.0.0", lockfile3).as_deref(),
            Some("export const y = 10"),
            "lockfile3 {lockfile3}"
        );
    }
}

#[test]
fn a_current_cache_entry_is_kept() {
    // "non-expired cache entry (inferred project, should not install
    // typings)" and its lockfile3 case.
    for lockfile3 in [false, true] {
        assert_eq!(
            cached_jquery("1.3.0", lockfile3).as_deref(),
            Some("export const x = 10;"),
            "lockfile3 {lockfile3}"
        );
    }
}

#[test]
fn typings_are_installed_for_unresolved_imports() {
    // "should install typings for unresolved imports".
    let app = "\n\t\t\t\timport * as fs from \"fs\";\n                import * as commander from \"commander\";\n                import * as component from \"@ember/component\";\n\t\t\t";
    let (session, _, _, npm) = setup_with_typings_installer(
        &[("/user/username/projects/project/app.js", app)],
        typings(
            &[],
            &[
                ("node", "export let node: number"),
                ("commander", "export let commander: number"),
                ("ember__component", "export let ember__component: number"),
            ],
        ),
    );
    open_app(&session, app);
    let calls = npm.npm_install_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].1, registry_call());
    // All three in one command.
    let args = &calls[1].1;
    assert_eq!(args[0], "install");
    assert_eq!(args[1], "--ignore-scripts");
    for package in [
        "@types/ember__component@latest",
        "@types/commander@latest",
        "@types/node@latest",
    ] {
        assert!(args.contains(&package.to_owned()), "{package}: {args:?}");
    }
    let program = program(&session, APP);
    for name in ["node", "commander", "ember__component"] {
        assert!(
            source_text(&program, &typings_file(name)).is_some(),
            "{name}"
        );
    }
}

#[test]
fn ata_without_watching_does_not_panic() {
    // "ATA with WatchEnabled false should not panic".
    let (session, _, _, npm) = setup_with_options_and_typings_installer(
        &[
            ("/user/username/projects/project/app.js", ""),
            (
                "/user/username/projects/project/package.json",
                r#"{ "name": "test", "dependencies": { "jquery": "^3.1.0" } }"#,
            ),
        ],
        |options| {
            options.watch_enabled = false;
            options.push_diagnostics_enabled = false;
        },
        typings(&[], &[("jquery", "declare const $: { x: number }")]),
    );
    open_app(&session, "");
    assert_eq!(npm.npm_install_calls().len(), 2);
    assert!(session.get_snapshot_and_default_project(&uri(APP)).is_ok());
}

fn ata_disabled() -> serde_json::Map<String, serde_json::Value> {
    serde_json::json!({ "js/ts": { "tsserver": { "automaticTypeAcquisition": { "enabled": false } } } })
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn ata_can_be_disabled() {
    // "ATA disabled via unified setting" and "via deprecated setting".
    let deprecated =
        serde_json::json!({ "typescript": { "disableAutomaticTypeAcquisition": true } })
            .as_object()
            .unwrap()
            .clone();
    for config in [ata_disabled(), deprecated] {
        let (session, _, _, npm) = setup_with_typings_installer(
            &[
                ("/user/username/projects/project/app.js", ""),
                (
                    "/user/username/projects/project/package.json",
                    r#"{ "name": "test", "dependencies": { "jquery": "^3.1.0" } }"#,
                ),
            ],
            typings(&[], &[("jquery", "declare const $: { x: number }")]),
        );
        session.configure(tsc_ls::lsutil::parse_user_preferences(&config));
        open_app(&session, "");
        assert_eq!(npm.npm_install_calls().len(), 0, "{config:?}");
    }
}

#[test]
fn ata_enabled_again_refreshes_diagnostics() {
    // "ATA re-enabled after being disabled triggers diagnostics refresh".
    let (session, _, client, npm) = setup_with_typings_installer(
        &[
            ("/user/username/projects/project/app.js", ""),
            (
                "/user/username/projects/project/package.json",
                r#"{ "name": "test", "dependencies": { "jquery": "^3.1.0" } }"#,
            ),
        ],
        typings(&[], &[("jquery", "declare const $: { x: number }")]),
    );
    session.configure(tsc_ls::lsutil::parse_user_preferences(&ata_disabled()));
    open_app(&session, "");
    assert_eq!(npm.npm_install_calls().len(), 0);
    let baseline = client.refresh_diagnostics_calls();
    session.configure(tsc_ls::lsutil::parse_user_preferences(
        &serde_json::Map::new(),
    ));
    session.wait_for_background_tasks();
    assert!(client.refresh_diagnostics_calls() > baseline);
}
