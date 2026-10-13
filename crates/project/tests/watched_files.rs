//! tsgo's watched-file tests (`project/session_test.go`
//! "DidChangeWatchedFiles"): the editor's watch events reach the projects,
//! and the session watches the files the programs read. The tests of the
//! diagnostics refresh a watch event schedules come with L2c.

mod common;

use std::sync::Arc;

use common::*;
use tsc_host::vfs::{FileSystem, Seed};
use tsc_lsproto::FileChangeType;
use tsc_project::{Session, Snapshot};

const P1_TSCONFIG: &str = r#"{
    "compilerOptions": {
        "noLib": true,
        "module": "nodenext",
        "strict": true
    },
    "include": ["src"]
}"#;
const P1_INDEX: &str = r#"import { x } from "./x";"#;
const P1_X: &str = "export const x = 1;";

fn default_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/home/projects/TS/p1/tsconfig.json", P1_TSCONFIG),
        ("/home/projects/TS/p1/src/index.ts", P1_INDEX),
        ("/home/projects/TS/p1/src/x.ts", P1_X),
        ("/home/projects/TS/p1/config.ts", "let x = 1, y = 2;"),
    ]
}

const INDEX: &str = "file:///home/projects/TS/p1/src/index.ts";
const INDEX_FILE: &str = "/home/projects/TS/p1/src/index.ts";

fn event(session: &Session, document: &str, kind: FileChangeType) {
    session.did_change_watched_files(&[watch_event(document, kind)]);
}

/// tsgo `CommandLine().ParsedConfig.FileNames` of the document's default
/// project.
fn root_file_names(session: &Session, document: &str) -> Vec<String> {
    let (_, project) = session
        .get_snapshot_and_default_project(&uri(document))
        .expect("the document's project");
    project.root_file_names()
}

fn has_source(session: &Session, document: &str, file_name: &str) -> bool {
    source_text(&program(session, document), file_name).is_some()
}

#[test]
fn a_change_of_an_open_file_keeps_the_program() {
    // "change open file": an open file changes through its document.
    let files = default_files();
    let (session, fs) = setup(&files);
    open(&session, "file:///home/projects/TS/p1/src/x.ts", P1_X);
    open(&session, INDEX, P1_INDEX);
    let before = program(&session, INDEX);
    fs.write("/home/projects/TS/p1/src/x.ts", b"export const x = 2;")
        .expect("write x.ts");
    event(
        &session,
        "file:///home/projects/TS/p1/src/x.ts",
        FileChangeType::CHANGED,
    );
    assert!(Arc::ptr_eq(&program(&session, INDEX), &before));
}

#[test]
fn a_change_of_a_closed_program_file_updates_the_program() {
    // "change closed program file".
    let files = default_files();
    let (session, fs) = setup(&files);
    open(&session, INDEX, P1_INDEX);
    let before = program(&session, INDEX);
    fs.write("/home/projects/TS/p1/src/x.ts", b"export const x = 2;")
        .expect("write x.ts");
    event(
        &session,
        "file:///home/projects/TS/p1/src/x.ts",
        FileChangeType::CHANGED,
    );
    assert!(!Arc::ptr_eq(&program(&session, INDEX), &before));
}

#[test]
fn a_program_file_outside_the_roots_is_watched() {
    // "change program file not in tsconfig root files", in each of tsgo's
    // workspaces.
    for workspace in ["/", "/home/projects/TS/p1", "/somewhere/else/entirely"] {
        let files = [
            (
                "/home/projects/TS/p1/tsconfig.json",
                r#"{
                    "compilerOptions": { "noLib": true, "module": "nodenext", "strict": true },
                    "files": ["src/index.ts"]
                }"#,
            ),
            (INDEX_FILE, r#"import { x } from "../../x";"#),
            ("/home/projects/TS/x.ts", "export const x = 1;"),
        ];
        let (session, fs, client) = setup_entries(
            files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), Seed::file(text)))
                .collect(),
            workspace,
        );
        open(&session, INDEX, files[1].1);
        let before = program(&session, INDEX);
        session.wait_for_background_tasks();
        assert!(
            watches_file(&client, "/home/projects/TS/x.ts"),
            "workspace {workspace}"
        );

        fs.write("/home/projects/TS/x.ts", b"export const x = 2;")
            .expect("write x.ts");
        event(
            &session,
            "file:///home/projects/TS/x.ts",
            FileChangeType::CHANGED,
        );
        assert!(!Arc::ptr_eq(&program(&session, INDEX), &before));
    }
}

#[test]
fn a_config_change_updates_the_diagnostics() {
    // "change config file".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true, "strict": false } }"#,
        ),
        (
            "/home/projects/TS/p1/src/x.ts",
            "export declare const x: number | undefined;",
        ),
        (
            INDEX_FILE,
            "\n\t\t\t\t\timport { x } from \"./x\";\n\t\t\t\t\tlet y: number = x;",
        ),
    ];
    let (session, fs) = setup(&files);
    open(&session, INDEX, files[2].1);
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 0);
    fs.write(
        "/home/projects/TS/p1/tsconfig.json",
        br#"{ "compilerOptions": { "noLib": false, "strict": true } }"#,
    )
    .expect("write the config");
    event(
        &session,
        "file:///home/projects/TS/p1/tsconfig.json",
        FileChangeType::CHANGED,
    );
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 1);
}

#[test]
fn a_deleted_explicit_root_stays_a_root() {
    // "delete explicitly included file".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "files": ["src/index.ts", "src/x.ts"] }"#,
        ),
        (
            "/home/projects/TS/p1/src/x.ts",
            "export declare const x: number | undefined;",
        ),
        (INDEX_FILE, P1_INDEX),
    ];
    let (session, fs) = setup(&files);
    open(&session, INDEX, P1_INDEX);
    let x = "/home/projects/TS/p1/src/x.ts".to_owned();
    assert!(root_file_names(&session, INDEX).contains(&x));
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 0);

    fs.remove(&x).expect("delete x.ts");
    event(
        &session,
        "file:///home/projects/TS/p1/src/x.ts",
        FileChangeType::DELETED,
    );
    // Explicitly included, it stays in the command line.
    assert!(root_file_names(&session, INDEX).contains(&x));
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 1);
    assert!(!has_source(&session, INDEX, &x));

    // An open triggers the clean-up.
    open(&session, "untitled:Untitled-1", "");
    assert!(session.snapshot().read_file(&x).is_none());
}

#[test]
fn a_deleted_wildcard_root_leaves_the_roots() {
    // "delete wildcard included file".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
        ),
        (INDEX_FILE, "let x = 2;"),
        ("/home/projects/TS/p1/src/x.ts", "let y = x;"),
    ];
    let (session, fs) = setup(&files);
    let x_document = "file:///home/projects/TS/p1/src/x.ts";
    let x_file = "/home/projects/TS/p1/src/x.ts";
    open(&session, x_document, files[2].1);
    assert!(root_file_names(&session, x_document).contains(&INDEX_FILE.to_owned()));
    assert_eq!(semantic_diagnostic_count(&session, x_document, x_file), 0);

    fs.remove(INDEX_FILE).expect("delete index.ts");
    event(&session, INDEX, FileChangeType::DELETED);
    assert!(!root_file_names(&session, x_document).contains(&INDEX_FILE.to_owned()));
    assert_eq!(semantic_diagnostic_count(&session, x_document, x_file), 1);

    open(&session, "untitled:Untitled-1", "");
    assert!(session.snapshot().read_file(INDEX_FILE).is_none());
}

/// "delete directory with wildcard included files" and "delete directory
/// with program-only files": the client sends one event for the directory.
fn a_deleted_directory_leaves_the_program(config: &str) {
    let files = [
        ("/home/projects/TS/p1/tsconfig.json", config),
        (INDEX_FILE, r#"import { x } from "./sub/x";"#),
        ("/home/projects/TS/p1/src/sub/x.ts", "export const x = 1;"),
    ];
    let (session, fs) = setup(&files);
    open(&session, INDEX, files[1].1);
    let x = "/home/projects/TS/p1/src/sub/x.ts";
    assert!(has_source(&session, INDEX, x));
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 0);

    fs.remove(x).expect("delete x.ts");
    fs.remove("/home/projects/TS/p1/src/sub")
        .expect("delete the directory");
    event(
        &session,
        "file:///home/projects/TS/p1/src/sub",
        FileChangeType::DELETED,
    );
    assert!(!root_file_names(&session, INDEX).contains(&x.to_owned()));
    assert!(!has_source(&session, INDEX, x));
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 1);
}

#[test]
fn a_deleted_directory_of_wildcard_roots_leaves_the_program() {
    a_deleted_directory_leaves_the_program(
        r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
    );
}

#[test]
fn a_deleted_directory_of_imported_files_leaves_the_program() {
    a_deleted_directory_leaves_the_program(
        r#"{ "compilerOptions": { "noLib": true }, "files": ["src/index.ts"] }"#,
    );
}

/// "create explicitly included file", "create failed lookup location",
/// "create file in non-existent directory": a created file the program
/// looked for joins it.
fn a_created_file_joins_the_program(config: &str, index: &str, created: &str, text: &str) {
    let files = [
        ("/home/projects/TS/p1/tsconfig.json", config),
        (INDEX_FILE, index),
    ];
    let (session, fs) = setup(&files);
    open(&session, INDEX, index);
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 1);
    fs.write_creating_dirs(created, text.as_bytes())
        .expect("write the file");
    event(
        &session,
        &format!("file://{created}"),
        FileChangeType::CREATED,
    );
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 0);
    assert!(has_source(&session, INDEX, created));
}

#[test]
fn a_created_explicit_root_joins_the_program() {
    a_created_file_joins_the_program(
        r#"{ "compilerOptions": { "noLib": true }, "files": ["src/index.ts", "src/y.ts"] }"#,
        r#"import { y } from "./y";"#,
        "/home/projects/TS/p1/src/y.ts",
        "export const y = 1;",
    );
}

#[test]
fn a_created_failed_lookup_joins_the_program() {
    a_created_file_joins_the_program(
        r#"{ "compilerOptions": { "noLib": true }, "files": ["src/index.ts"] }"#,
        r#"import { z } from "./z";"#,
        "/home/projects/TS/p1/src/z.ts",
        "export const z = 1;",
    );
}

#[test]
fn a_file_created_in_a_new_directory_joins_the_program() {
    a_created_file_joins_the_program(
        r#"{ "compilerOptions": { "noLib": true }, "files": ["src/index.ts"] }"#,
        r#"import { helper } from "./lib/helper";"#,
        "/home/projects/TS/p1/src/lib/helper.ts",
        "export const helper = 1;",
    );
}

#[test]
fn a_created_wildcard_root_joins_the_program() {
    // "create wildcard included file".
    a_created_file_joins_the_program(
        r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
        "a;",
        "/home/projects/TS/p1/src/a.ts",
        "const a = 1;",
    );
}

#[test]
fn irrelevant_extensions_keep_the_program() {
    // "irrelevant extension changes are filtered out".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
        ),
        (INDEX_FILE, "export const x = 1;"),
        ("/home/projects/TS/p1/src/data.txt", "some text"),
    ];
    let (session, fs) = setup(&files);
    open(&session, INDEX, files[1].1);
    assert_eq!(semantic_diagnostic_count(&session, INDEX, INDEX_FILE), 0);
    let before = program(&session, INDEX);
    fs.write("/home/projects/TS/p1/src/data.txt", b"updated text")
        .expect("write data.txt");
    session.did_change_watched_files(&[
        watch_event(
            "file:///home/projects/TS/p1/src/data.txt",
            FileChangeType::CHANGED,
        ),
        watch_event(
            "file:///home/projects/TS/p1/src/styles.css",
            FileChangeType::CREATED,
        ),
        watch_event(
            "file:///home/projects/TS/p1/src/image.png",
            FileChangeType::CREATED,
        ),
    ]);
    assert!(Arc::ptr_eq(&program(&session, INDEX), &before));
}

#[test]
fn a_pnpm_install_links_a_local_package() {
    // "pnpm install links local package".
    let files = [
        (
            "/home/projects/pnpm/pnpm-workspace.yaml",
            "packages:\n  - 'packages/*'",
        ),
        (
            "/home/projects/pnpm/packages/alpha/package.json",
            r#"{ "name": "@repo/alpha", "main": "index.ts" }"#,
        ),
        (
            "/home/projects/pnpm/packages/alpha/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true, "composite": true } }"#,
        ),
        (
            "/home/projects/pnpm/packages/alpha/index.ts",
            "export const alpha = 1;",
        ),
        (
            "/home/projects/pnpm/packages/beta/package.json",
            r#"{ "name": "@repo/beta" }"#,
        ),
        (
            "/home/projects/pnpm/packages/beta/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true } }"#,
        ),
        (
            "/home/projects/pnpm/packages/beta/index.ts",
            r#"import { alpha } from "@repo/alpha";"#,
        ),
    ];
    let (session, fs) = setup(&files);
    let beta = "file:///home/projects/pnpm/packages/beta/index.ts";
    let beta_file = "/home/projects/pnpm/packages/beta/index.ts";
    open(&session, beta, files[6].1);
    assert_eq!(semantic_diagnostic_count(&session, beta, beta_file), 1);

    fs.create_dir_all("/home/projects/pnpm/packages/beta/node_modules/@repo")
        .expect("create node_modules");
    fs.symlink(
        "/home/projects/pnpm/packages/alpha",
        "/home/projects/pnpm/packages/beta/node_modules/@repo/alpha",
    )
    .expect("link the package");
    let created = |path: &str| watch_event(&format!("file://{path}"), FileChangeType::CREATED);
    let changed = |path: &str| watch_event(&format!("file://{path}"), FileChangeType::CHANGED);
    session.did_change_watched_files(&[
        created("/home/projects/pnpm/packages/beta/node_modules"),
        created("/home/projects/pnpm/packages/beta/node_modules/%40repo"),
        created("/home/projects/pnpm/packages/beta/node_modules/%40repo/alpha"),
        created("/home/projects/pnpm/pnpm-lock.yaml"),
        changed("/home/projects/pnpm/packages/beta/node_modules/.bin/tsc"),
        changed("/home/projects/pnpm/packages/beta/node_modules/.bin/tsserver"),
    ]);
    assert_eq!(semantic_diagnostic_count(&session, beta, beta_file), 0);
}

#[test]
fn a_linked_package_manifest_change_invalidates_the_resolution() {
    // "symlinked node_modules package.json change invalidates resolution".
    let entries = vec![
        (
            "/home/projects/myproject/tsconfig.json",
            Seed::file(
                r#"{
                    "compilerOptions": { "noLib": true, "module": "nodenext", "moduleResolution": "nodenext" },
                    "files": ["src/index.ts"]
                }"#,
            ),
        ),
        (
            "/home/projects/myproject/src/index.ts",
            Seed::file(r#"import { foo } from "mylib";"#),
        ),
        (
            "/home/projects/mylib/package.json",
            Seed::file(r#"{ "name": "mylib", "main": "dist/index.js" }"#),
        ),
        (
            "/home/projects/mylib/dist/index.js",
            Seed::file("exports.foo = function() { return 1; };"),
        ),
        (
            "/home/projects/mylib/dist/index.d.ts",
            Seed::file("export declare function foo(): number;"),
        ),
        (
            "/home/projects/myproject/node_modules/mylib",
            Seed::symlink("/home/projects/mylib"),
        ),
    ];
    let (session, fs, client) = setup_entries(
        entries
            .into_iter()
            .map(|(name, seed)| (name.to_owned(), seed))
            .collect(),
        "/home/projects/myproject",
    );
    let index = "file:///home/projects/myproject/src/index.ts";
    let index_file = "/home/projects/myproject/src/index.ts";
    open(&session, index, r#"import { foo } from "mylib";"#);
    assert_eq!(semantic_diagnostic_count(&session, index, index_file), 0);
    session.wait_for_background_tasks();
    // The real paths of the package's files are watched.
    assert!(watches_file(&client, "/home/projects/mylib/package.json"));
    assert!(watches_file(
        &client,
        "/home/projects/mylib/dist/index.d.ts"
    ));

    fs.write(
        "/home/projects/mylib/package.json",
        br#"{ "name": "mylib" }"#,
    )
    .expect("write the manifest");
    event(
        &session,
        "file:///home/projects/mylib/package.json",
        FileChangeType::CHANGED,
    );
    assert!(semantic_diagnostic_count(&session, index, index_file) > 0);
}

#[test]
fn a_created_linked_directory_adds_its_files_to_the_roots() {
    // "create symlink directory matching include pattern".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
        ),
        (INDEX_FILE, "export const x = 1;"),
        (
            "/home/projects/TS/shared/utils.ts",
            r#"export const util = "hello";"#,
        ),
        (
            "/home/projects/TS/shared/helpers.ts",
            "export const helper = 42;",
        ),
    ];
    let (session, fs) = setup(&files);
    open(&session, INDEX, files[1].1);
    let roots = root_file_names(&session, INDEX);
    assert!(roots.contains(&INDEX_FILE.to_owned()));
    assert!(!roots.contains(&"/home/projects/TS/p1/src/linked/utils.ts".to_owned()));

    fs.symlink(
        "/home/projects/TS/shared",
        "/home/projects/TS/p1/src/linked",
    )
    .expect("link the directory");
    event(
        &session,
        "file:///home/projects/TS/p1/src/linked",
        FileChangeType::CREATED,
    );
    let roots = root_file_names(&session, INDEX);
    for root in [
        INDEX_FILE,
        "/home/projects/TS/p1/src/linked/utils.ts",
        "/home/projects/TS/p1/src/linked/helpers.ts",
    ] {
        assert!(roots.contains(&root.to_owned()), "{root}");
    }
}

#[test]
fn the_default_files_are_watched_below_the_workspace() {
    // tsgo's watches of an opened config's roots and program files below
    // the workspace: one recursive glob of the workspace.
    let files = default_files();
    let (session, _, client) = setup_with_client(&files);
    open(&session, INDEX, P1_INDEX);
    session.wait_for_background_tasks();
    let calls = client.watch_files_calls();
    assert_eq!(calls.len(), 1);
    let (id, watchers) = &calls[0];
    assert!(
        id.starts_with("root files for /home/projects/TS/p1/tsconfig.json watcher "),
        "{id}"
    );
    assert_eq!(watchers.len(), 1);
    assert!(watches_file(&client, INDEX_FILE));
    let snapshot: Arc<Snapshot> = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
}
