//! tsgo's project system tests of content mappers (`project/contentmapper_test.go`)
//! that go through the language server's session: open documents, edits,
//! watch events, the editor's locale and contributions, and the extensions
//! the session registers with the editor. The API's session tests cover the
//! rest (`crates/api/tests/unit/session_content_mappers.rs`).

mod common;

use std::sync::atomic::Ordering;
use std::sync::Arc;

use common::*;
use tsc_compiler::locale::Locale;
use tsc_harness::content_mappers::{
    package_json, RecordingSpawner, TestSpawner, TRANSFORMING_MAPPER,
};
use tsc_host::vfs::{FileSystem, Seed};
use tsc_lsproto::{
    FileChangeType, LanguageKind, TextDocumentContentChangePartialOrWholeDocument,
    TextDocumentContentChangeWholeDocument,
};
use tsc_program::content_mapper::{Definition, Manifest, Mapper};
use tsc_project::{ContentMapperContributions, ProjectKind};

/// tsgo `TestContentMapperInProject`'s config.
const TARGET_CONFIG: &str = r#"{
    "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler", "strict": true },
    "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
}"#;

const TARGET_APP: &str = "export const version = #{target};\n";

fn entries(files: &[(&str, String)]) -> Vec<(String, Seed)> {
    files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), Seed::file(text)))
        .collect()
}

fn whole_document(text: &str) -> TextDocumentContentChangePartialOrWholeDocument {
    TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
        TextDocumentContentChangeWholeDocument {
            text: text.to_owned(),
        },
    )
}

fn box_kind() -> LanguageKind {
    LanguageKind("box".into())
}

#[test]
fn editing_an_open_mapped_file_reparses_it_through_the_mapper() {
    // tsgo `TestContentMapperInProject` ("editing an open content-mapped
    // file reparses it through the mapper"): the .box opened with its
    // mapped language id, then edited, runs through the mapper again.
    let files = [
        ("/home/project/tsconfig.json", TARGET_CONFIG.to_owned()),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
        ("/home/project/app.box", TARGET_APP.to_owned()),
        (
            "/home/project/main.ts",
            "import { version } from \"./app.box\";\nexport const twice: number = version * 2;\n"
                .to_owned(),
        ),
    ];
    let (session, _fs, _client) = setup_with_spawner(
        entries(&files),
        "/home/project",
        Arc::new(TestSpawner::default()),
    );
    open(&session, "file:///home/project/main.ts", &files[3].1);
    open_as(
        &session,
        "file:///home/project/app.box",
        TARGET_APP,
        box_kind(),
    );
    program(&session, "file:///home/project/main.ts");
    session.did_change_file(
        &uri("file:///home/project/app.box"),
        2,
        vec![whole_document(
            "export const version = #{target};\nexport const extra = 1;\n",
        )],
    );
    let program = program(&session, "file:///home/project/main.ts");
    let text = source_text(&program, "/home/project/app.box").expect("app.box is loaded");
    assert!(text.contains("export const version = 7;"), "{text:?}");
    assert!(text.contains("export const extra = 1;"), "{text:?}");
}

#[test]
fn a_created_mapped_file_is_adopted_by_the_configured_project() {
    // tsgo `TestContentMapperCreatedFileAdoptedByConfiguredProject`: the
    // created-file root matching takes the content mappers' extensions.
    let files = [
        (
            "/home/project/tsconfig.json",
            r#"{
                "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler" },
                "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
            }"#
            .to_owned(),
        ),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
        (
            "/home/project/main.ts",
            "export const main = true;\n".to_owned(),
        ),
    ];
    let (session, fs, _client) = setup_with_spawner(
        entries(&files),
        "/home/project",
        Arc::new(TestSpawner::default()),
    );
    open(&session, "file:///home/project/main.ts", &files[2].1);
    program(&session, "file:///home/project/main.ts");
    fs.write("/home/project/new.box", TARGET_APP.as_bytes())
        .expect("create new.box");
    session.did_change_watched_files(&[watch_event(
        "file:///home/project/new.box",
        FileChangeType::CREATED,
    )]);
    open_as(
        &session,
        "file:///home/project/new.box",
        TARGET_APP,
        box_kind(),
    );
    let (_, project) = session
        .get_snapshot_and_default_project(&uri("file:///home/project/new.box"))
        .expect("a default project for the created new.box");
    assert_eq!(project.kind(), ProjectKind::Configured);
    let program = program(&session, "file:///home/project/new.box");
    let text = source_text(&program, "/home/project/new.box").expect("new.box is loaded");
    assert!(!text.contains("#{target}"), "{text:?}");
}

#[test]
fn a_locale_change_restarts_the_mappers() {
    // tsgo `TestContentMapperLocaleChange`: the mappers' processes run in
    // the editor's locale; a new locale closes them, and the next program
    // starts them again.
    let files = [
        (
            "/home/project/tsconfig.json",
            r#"{ "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ] }"#
                .to_owned(),
        ),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(tsc_harness::content_mappers::VERBATIM_MAPPER),
        ),
        (
            "/home/project/app.box",
            "export const value = 1;\n".to_owned(),
        ),
        (
            "/home/project/main.ts",
            r#"import { value } from "./app.box"; value;"#.to_owned(),
        ),
    ];
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, _fs, client) =
        setup_with_spawner(entries(&files), "/home/project", Arc::clone(&spawner) as _);
    let current_locale = Arc::new(std::sync::RwLock::new(Locale::default()));
    *client.locale_func.lock().unwrap() = Some(Box::new({
        let current_locale = Arc::clone(&current_locale);
        move || *current_locale.read().unwrap()
    }));
    *client.set_locale_func.lock().unwrap() = Some(Box::new({
        let current_locale = Arc::clone(&current_locale);
        move |value| {
            *current_locale.write().unwrap() = Locale::parse(value).expect("a locale");
        }
    }));
    open(&session, "file:///home/project/main.ts", &files[3].1);
    program(&session, "file:///home/project/main.ts");
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 1);

    let mut preferences = session.config();
    preferences.locale = "fr".to_owned();
    session.configure(preferences);
    assert_eq!(spawner.closes.load(Ordering::SeqCst), 1);

    program(&session, "file:///home/project/main.ts");
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 2);
}

/// tsgo `TestContentMapperInProject`'s files.
fn in_project_files() -> Vec<(&'static str, String)> {
    vec![
        ("/home/project/tsconfig.json", TARGET_CONFIG.to_owned()),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
        ("/home/project/app.box", TARGET_APP.to_owned()),
        (
            "/home/project/main.ts",
            "import { version } from \"./app.box\";\nexport const twice: number = version * 2;\n"
                .to_owned(),
        ),
    ]
}

#[test]
fn a_trusted_session_registers_the_configs_mapper_extensions() {
    // tsgo `TestContentMapperInProject` ("trusted workspace transforms the
    // content-mapped file"): the config's .box mapper is registered for
    // text document synchronization.
    let files = in_project_files();
    let (session, _fs, client) = setup_with_spawner(
        entries(&files),
        "/home/project",
        Arc::new(TestSpawner::default()),
    );
    open(&session, "file:///home/project/main.ts", &files[3].1);
    let program = program(&session, "file:///home/project/main.ts");
    let text = source_text(&program, "/home/project/app.box").expect("app.box is loaded");
    assert!(text.contains("export const version = 7;"), "{text:?}");
    session.wait_for_background_tasks();
    let calls = client.register_content_mapper_extensions_calls();
    assert_eq!(
        calls.last().map(Vec::as_slice),
        Some(&[".box".to_owned()][..])
    );
}

#[test]
fn an_untrusted_session_registers_no_mapper_extensions() {
    // tsgo `TestContentMapperInProject` ("untrusted workspace does not run
    // the content mapper"): without trust the mappers are dropped, so .box
    // is not a recognized extension and nothing is registered.
    let files = in_project_files();
    let (session, _fs, client) = setup_entries(entries(&files), "/home/project");
    open(&session, "file:///home/project/main.ts", &files[3].1);
    let program = program(&session, "file:///home/project/main.ts");
    assert_eq!(source_text(&program, "/home/project/app.box"), None);
    session.wait_for_background_tasks();
    assert!(client
        .register_content_mapper_extensions_calls()
        .iter()
        .all(Vec::is_empty));
}

#[test]
fn the_mappers_of_parallel_project_references_are_registered() {
    // tsgo `TestContentMappersInParallelProjectReferences`.
    let files = [
        (
            "/home/project/tsconfig.json",
            r#"{
                "files": ["src/index.d.ts"],
                "references": [{ "path": "./a" }, { "path": "./b" }]
            }"#
            .to_owned(),
        ),
        ("/home/project/src/index.d.ts", "export {};".to_owned()),
        (
            "/home/project/a/tsconfig.json",
            r#"{
                "compilerOptions": { "composite": true },
                "files": ["../src/index.d.ts"],
                "contentMappers": [{ "package": "mapper", "extensions": [".vue"] }]
            }"#
            .to_owned(),
        ),
        (
            "/home/project/b/tsconfig.json",
            r#"{
                "compilerOptions": { "composite": true },
                "files": ["../src/index.d.ts"],
                "contentMappers": [{ "package": "mapper", "extensions": [".svelte"] }]
            }"#
            .to_owned(),
        ),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
    ];
    let (session, _fs, client) = setup_with_spawner(
        entries(&files),
        "/home/project",
        Arc::new(TestSpawner::default()),
    );
    open(&session, "file:///home/project/src/index.d.ts", &files[1].1);
    session.wait_for_background_tasks();
    let calls = client.register_content_mapper_extensions_calls();
    let mut extensions = calls.last().cloned().expect("a registration");
    extensions.sort();
    assert_eq!(extensions, [".svelte", ".vue"]);
}

/// tsgo's extension contribution of the tests: the transforming mapper
/// for .box files, from `directory`.
fn box_contribution(directory: &str) -> ContentMapperContributions {
    ContentMapperContributions {
        mappers: vec![Arc::new(Mapper {
            definition: Definition {
                package: "test.extension".to_owned(),
                extensions: Some(vec![".box".to_owned()]),
                options: None,
            },
            manifest: Manifest {
                name: "mapper".to_owned(),
                version: "1.0.0".to_owned(),
                exec: Some(vec![TRANSFORMING_MAPPER.to_owned()]),
                compiler_options: Some(vec!["target".to_owned(), "jsx".to_owned()]),
                dynamic_config: false,
            },
            package_directory: directory.to_owned(),
            contribution_id: "test.extension[0]".to_owned(),
        })],
        extensions: vec![".box".to_owned()],
    }
}

#[test]
fn an_inferred_project_uses_the_editors_contributions() {
    // tsgo `TestContentMapperInferredProjectUsesExtensionContributions`: a
    // configured project's mapper does not reach an inferred project; the
    // editor's contribution does.
    let files = [
        (
            "/home/configured/tsconfig.json",
            r#"{
                "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler" },
                "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
            }"#
            .to_owned(),
        ),
        (
            "/home/configured/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
        (
            "/home/configured/main.ts",
            "export const main = true;\n".to_owned(),
        ),
        ("/home/loose/app.box", TARGET_APP.to_owned()),
    ];
    let (session, _fs, _client) =
        setup_with_spawner(entries(&files), "/home", Arc::new(TestSpawner::default()));
    open(&session, "file:///home/configured/main.ts", &files[2].1);
    program(&session, "file:///home/configured/main.ts");
    open_as(
        &session,
        "file:///home/loose/app.box",
        TARGET_APP,
        box_kind(),
    );
    let error = session
        .get_snapshot_and_default_project(&uri("file:///home/loose/app.box"))
        .err()
        .expect("configured mapper must not leak into inferred projects");
    assert!(error.to_string().contains("no project found"), "{error}");
    session.set_content_mapper_contributions(
        box_contribution("/home"),
        &[uri("file:///home/loose/app.box")],
    );
    let (_, project) = session
        .get_snapshot_and_default_project(&uri("file:///home/loose/app.box"))
        .expect("a default project for the loose app.box");
    assert_eq!(project.kind(), ProjectKind::Inferred);
    let program = program(&session, "file:///home/loose/app.box");
    let text = source_text(&program, "/home/loose/app.box").expect("the loose app.box");
    assert!(!text.contains("#{target}"), "{text:?}");
}

#[test]
fn an_open_mapped_file_a_config_change_excludes_goes_to_the_inferred_project() {
    // tsgo `TestContentMapperOpenFileExcludedByConfigChange`: the open
    // app.box leaves the configured project and, with the editor's
    // contribution, is transformed in the inferred project.
    let files = [
        (
            "/home/project/tsconfig.json",
            r#"{
                "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler", "strict": true },
                "include": ["src"],
                "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
            }"#
            .to_owned(),
        ),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
        ("/home/project/src/app.box", TARGET_APP.to_owned()),
        (
            "/home/project/src/main.ts",
            "export const main = true;\n".to_owned(),
        ),
    ];
    let (session, fs, _client) = setup_with_spawner(
        entries(&files),
        "/home/project",
        Arc::new(TestSpawner::default()),
    );
    session.set_content_mapper_contributions(box_contribution("/home/project"), &[]);
    open_as(
        &session,
        "file:///home/project/src/app.box",
        TARGET_APP,
        box_kind(),
    );
    let program_with_box = program(&session, "file:///home/project/src/app.box");
    assert!(source_text(&program_with_box, "/home/project/src/app.box").is_some());

    fs.write(
        "/home/project/tsconfig.json",
        br#"{
            "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler", "strict": true },
            "include": ["src/**/*.ts"],
            "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
        }"#,
    )
    .expect("write the config");
    session.did_change_watched_files(&[watch_event(
        "file:///home/project/tsconfig.json",
        FileChangeType::CHANGED,
    )]);
    session.wait_for_background_tasks();
    // The background update removes app.box from the configured project, but inferred
    // project cleanup is deferred until the next file open.
    assert!(session
        .snapshot()
        .default_project("/home/project/src/app.box")
        .is_none());
    open(&session, "file:///home/project/src/main.ts", &files[3].1);
    let (_, project) = session
        .get_snapshot_and_default_project(&uri("file:///home/project/src/app.box"))
        .expect("a default project for the open app.box");
    assert_eq!(project.kind(), ProjectKind::Inferred);
    let program = program(&session, "file:///home/project/src/app.box");
    let text =
        source_text(&program, "/home/project/src/app.box").expect("the open app.box is loaded");
    assert!(!text.contains("#{target}"), "{text:?}");
}

#[test]
fn removing_the_mapper_of_an_open_file_closes_it_with_the_last_snapshot() {
    // tsgo `TestContentMapperRemovalWithOpenFile`: edits of the open file
    // keep the mapper's process; when its config drops the mapper, the
    // file is in no project, the process lives while an old snapshot
    // does, and the extensions are unregistered.
    let files = [
        (
            "/home/project/tsconfig.json",
            r#"{
                "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler" },
                "contentMappers": [ { "package": "mapper", "extensions": [".box"] } ]
            }"#
            .to_owned(),
        ),
        (
            "/home/project/node_modules/mapper/package.json",
            package_json(TRANSFORMING_MAPPER),
        ),
        ("/home/project/app.box", TARGET_APP.to_owned()),
    ];
    let spawner = Arc::new(RecordingSpawner::new());
    let (session, fs, client) =
        setup_with_spawner(entries(&files), "/home/project", Arc::clone(&spawner) as _);
    let box_uri = uri("file:///home/project/app.box");
    open_as(
        &session,
        "file:///home/project/app.box",
        TARGET_APP,
        box_kind(),
    );
    program(&session, "file:///home/project/app.box");
    assert_eq!(spawner.spawns.load(Ordering::SeqCst), 1);
    assert_eq!(spawner.closes.load(Ordering::SeqCst), 0);
    for version in 2..=4 {
        session.did_change_file(
            &box_uri,
            version,
            vec![whole_document(&format!(
                "export const version = {version};\n"
            ))],
        );
        program(&session, "file:///home/project/app.box");
        assert_eq!(
            spawner.spawns.load(Ordering::SeqCst),
            1,
            "snapshot clone should reuse the mapper process"
        );
        assert_eq!(
            spawner.closes.load(Ordering::SeqCst),
            0,
            "snapshot clone should preserve overlapping ownership"
        );
    }
    let old_snapshot = session.snapshot();

    fs.write(
        "/home/project/tsconfig.json",
        br#"{ "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler" } }"#,
    )
    .expect("write the config");
    session.did_change_watched_files(&[watch_event(
        "file:///home/project/tsconfig.json",
        FileChangeType::CHANGED,
    )]);
    let error = session
        .get_snapshot_and_default_project(&box_uri)
        .err()
        .expect("no project");
    assert!(error.to_string().contains("no project found"), "{error}");
    let snapshot = session.snapshot();
    assert!(
        snapshot.overlay("/home/project/app.box").is_some(),
        "overlay should remain until didClose"
    );
    assert!(
        snapshot.default_project("/home/project/app.box").is_none(),
        "unsupported file should not be in a project"
    );
    drop(snapshot);

    session.wait_for_background_tasks();
    assert_eq!(
        spawner.closes.load(Ordering::SeqCst),
        0,
        "live old snapshot should retain the mapper process"
    );
    drop(old_snapshot);
    assert_eq!(
        spawner.closes.load(Ordering::SeqCst),
        1,
        "process should close after the final live snapshot is released"
    );
    let calls = client.register_content_mapper_extensions_calls();
    assert!(
        !calls.is_empty(),
        "expected content mapper registration updates"
    );
    assert!(
        calls.last().unwrap().is_empty(),
        "expected content mapper extensions to be unregistered"
    );

    session.did_close_file(&box_uri);
    let error = session
        .get_snapshot_and_default_project(&box_uri)
        .err()
        .expect("no project");
    assert!(error.to_string().contains("no project found"), "{error}");
}

#[test]
fn a_mapper_manifest_change_reloads_the_config_and_its_package_is_watched() {
    // tsgo `TestContentMapperPackageManifestChangeReloadsConfig`: the
    // linked mapper package's manifest is watched (with a relative
    // pattern watcher over the package), and its change reloads the
    // config.
    let package_json_path = "/home/mapper/package.json";
    let entries = vec![
        (
            "/home/project/tsconfig.json".to_owned(),
            Seed::file(
                r#"{
                    "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler" },
                    "contentMappers": [{ "package": "mapper", "extensions": [".box"] }]
                }"#,
            ),
        ),
        (
            "/home/project/node_modules/mapper".to_owned(),
            Seed::symlink("/home/mapper"),
        ),
        (
            package_json_path.to_owned(),
            Seed::file(
                r#"{
                    "name": "mapper",
                    "version": "1.0.0",
                    "typescript": { "contentMapper": { "exec": ["compiler-test-mapper"] } }
                }"#,
            ),
        ),
        ("/home/project/app.box".to_owned(), Seed::file(TARGET_APP)),
        (
            "/home/project/main.ts".to_owned(),
            Seed::file("import { version } from \"./app.box\";\n"),
        ),
    ];
    let (session, fs, client) = setup_with_spawner_and_relative_patterns(
        entries,
        "/home/project",
        Arc::new(TestSpawner::default()),
    );
    open(
        &session,
        "file:///home/project/main.ts",
        "import { version } from \"./app.box\";\n",
    );
    let (_, project) = session
        .get_snapshot_and_default_project(&uri("file:///home/project/main.ts"))
        .expect("the configured project");
    let Some(tsc_project::CommandLine::Config(plan)) = project.command_line() else {
        panic!("a configured project's config");
    };
    assert_eq!(plan.content_mappers().len(), 1);
    assert_eq!(plan.content_mappers()[0].package_directory, "/home/mapper");
    session.wait_for_background_tasks();
    assert!(
        watches_file(&client, package_json_path),
        "expected the invalid mapper package manifest to be watched"
    );
    assert!(
        client.watch_files_calls().iter().any(|(_, watchers)| {
            watchers.iter().any(|watcher| match &watcher.glob_pattern {
                tsc_lsproto::PatternOrRelativePattern::RelativePattern(relative) => {
                    matches!(&relative.base_uri, tsc_lsproto::WorkspaceFolderOrURI::URI(uri) if uri.0 == "file:///home/mapper")
                        && relative.pattern == "**/*"
                }
                _ => false,
            })
        }),
        "expected an external relative-pattern watcher for the mapper package"
    );

    let fixed_manifest = package_json(TRANSFORMING_MAPPER).replacen(
        r#""version": "1.0.0""#,
        r#""version": "2.0.0""#,
        1,
    );
    assert!(fixed_manifest.contains(r#""version": "2.0.0""#));
    fs.write(package_json_path, fixed_manifest.as_bytes())
        .expect("write the manifest");
    session.did_change_watched_files(&[watch_event(
        "file:///home/mapper/package.json",
        FileChangeType::CHANGED,
    )]);
    let program = program(&session, "file:///home/project/main.ts");
    let text = source_text(&program, "/home/project/app.box").expect("app.box is rebuilt");
    assert!(text.contains("export const version = 7;"), "{text:?}");
}
