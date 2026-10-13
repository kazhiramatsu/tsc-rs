//! tsgo's session tests (`project/session_test.go`): documents opened,
//! changed, closed and saved through the language server's session, and the
//! projects and programs they load.

mod common;

use std::sync::Arc;

use common::*;
use tsc_host::vfs::FileSystem;
use tsc_ls::lsutil::UserPreferences;
use tsc_lsproto::{
    FileChangeType, LanguageKind, TextDocumentContentChangePartialOrWholeDocument,
    TextDocumentContentChangeWholeDocument,
};
use tsc_program::uri::uri_to_file_name;
use tsc_project::{DocumentProjectError, ProgramUpdateKind, ProjectKind, ProjectProgram, Session};
use tsc_syntax::ScriptKind;

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
const P1_CONFIG_TS: &str = "let x = 1, y = 2;";

/// tsgo's `defaultFiles`.
fn default_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/home/projects/TS/p1/tsconfig.json", P1_TSCONFIG),
        ("/home/projects/TS/p1/src/index.ts", P1_INDEX),
        ("/home/projects/TS/p1/src/x.ts", P1_X),
        ("/home/projects/TS/p1/config.ts", P1_CONFIG_TS),
    ]
}

#[test]
fn did_open_file_creates_a_configured_project() {
    // "DidOpenFile/create configured project".
    let files = default_files();
    let (session, _) = setup(&files);
    assert_eq!(session.snapshot().projects().count(), 0);

    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );

    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot
        .project(&configured("/home/projects/ts/p1/tsconfig.json"))
        .is_some());
    let program = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    assert_eq!(
        source_text(&program, "/home/projects/TS/p1/src/x.ts").as_deref(),
        Some(P1_X)
    );
}

#[test]
fn did_open_file_creates_an_inferred_project() {
    // "DidOpenFile/create inferred project": the config is loaded, does
    // not include config.ts, and the file goes to the inferred project.
    let files = default_files();
    let (session, _) = setup(&files);

    open(
        &session,
        "file:///home/projects/TS/p1/config.ts",
        P1_CONFIG_TS,
    );

    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 2);
    assert!(snapshot
        .project(&configured("/home/projects/ts/p1/tsconfig.json"))
        .is_some());
    assert!(snapshot.inferred_project().is_some());
}

#[test]
fn in_memory_files_go_to_the_inferred_project() {
    // "DidOpenFile/inferred project for in-memory files".
    let files = default_files();
    let (session, _) = setup(&files);

    open(
        &session,
        "file:///home/projects/TS/p1/config.ts",
        P1_CONFIG_TS,
    );
    open(&session, "untitled:Untitled-1", "x");
    open(&session, "untitled:Untitled-2", "y");

    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
}

#[test]
fn a_js_file_goes_to_the_inferred_project() {
    // "DidOpenFile/inferred project JS file".
    let files = [(
        "/home/projects/TS/p1/index.js",
        r#"import { x } from "./x";"#,
    )];
    let (session, _) = setup(&files);

    open_as(
        &session,
        "file:///home/projects/TS/p1/index.js",
        files[0].1,
        LanguageKind::JAVA_SCRIPT,
    );

    assert_eq!(session.snapshot().projects().count(), 1);
    let program = program(&session, "file:///home/projects/TS/p1/index.js");
    assert!(source_text(&program, "/home/projects/TS/p1/index.js").is_some());
}

#[test]
fn an_extensionless_file_goes_to_the_inferred_project() {
    // "DidOpenFile/inferred project extensionless file".
    let files = [("/home/projects/TS/p1/script", "const x = 1;")];
    let (session, _) = setup(&files);

    open_as(
        &session,
        "file:///home/projects/TS/p1/script",
        files[0].1,
        LanguageKind("plaintext".into()),
    );

    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    let program = program(&session, "file:///home/projects/TS/p1/script");
    assert_eq!(
        document(&program, "/home/projects/TS/p1/script")
            .source()
            .script_kind,
        ScriptKind::Ts
    );
}

#[test]
fn an_open_document_parses_as_its_language() {
    // tsgo's parse cache key takes the file handle's kind: an untitled
    // document opened as JavaScript parses as JavaScript (JSX and the
    // JavaScript context), one opened as TypeScript as TypeScript.
    let (session, _) = setup(&[]);
    let text = "const element = <div />;\nmodule.exports = element;\n";
    open_as(
        &session,
        "untitled:Untitled-1",
        text,
        LanguageKind::JAVA_SCRIPT,
    );
    open_as(
        &session,
        "untitled:Untitled-2",
        text,
        LanguageKind::TYPE_SCRIPT,
    );
    let parsed = |document_uri: &str| {
        let program = program(&session, document_uri);
        let file_name = uri_to_file_name(document_uri).expect("a dynamic file name");
        let document = document(&program, &file_name);
        let source = document.source();
        (source.script_kind, source.parse_diagnostics.len())
    };
    assert_eq!(parsed("untitled:Untitled-1"), (ScriptKind::Js, 0));
    let (kind, errors) = parsed("untitled:Untitled-2");
    assert_eq!(kind, ScriptKind::Ts);
    assert!(errors > 0, "a TypeScript parse of JSX has errors");
}

#[test]
fn a_watch_change_and_an_open_in_one_batch_rebuild_the_program() {
    // "watchChange and didOpen in same batch rebuilds program".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true, "strict": true } }"#,
        ),
        ("/home/projects/TS/p1/src/a.ts", "export const a = 1;\n"),
        ("/home/projects/TS/p1/src/b.ts", "export const b = 1;\n"),
    ];
    let (session, fs) = setup(&files);

    open(&session, "file:///home/projects/TS/p1/src/b.ts", files[2].1);
    let before = program(&session, "file:///home/projects/TS/p1/src/b.ts");
    assert_eq!(
        source_text(&before, "/home/projects/TS/p1/src/a.ts").as_deref(),
        Some(files[1].1)
    );

    // A build tool or a checkout writes a.ts; its watch event waits, and
    // the open of a.ts flushes both.
    let new_content = "export const a = 2;\nexport const extra = true;\n";
    fs.write("/home/projects/TS/p1/src/a.ts", new_content.as_bytes())
        .expect("write a.ts");
    session.did_change_watched_files(&[watch_event(
        "file:///home/projects/TS/p1/src/a.ts",
        FileChangeType::CHANGED,
    )]);
    open(
        &session,
        "file:///home/projects/TS/p1/src/a.ts",
        new_content,
    );

    let after = program(&session, "file:///home/projects/TS/p1/src/a.ts");
    assert_eq!(
        source_text(&after, "/home/projects/TS/p1/src/a.ts").as_deref(),
        Some(new_content)
    );
}

#[test]
fn a_change_updates_the_file_and_the_program() {
    // "DidChangeFile/update file and program".
    let files = default_files();
    let (session, _) = setup(&files);
    open(&session, "file:///home/projects/TS/p1/src/x.ts", P1_X);
    let before = program(&session, "file:///home/projects/TS/p1/src/x.ts");

    session.did_change_file(
        &uri("file:///home/projects/TS/p1/src/x.ts"),
        2,
        vec![edit(17, 18, "2")],
    );

    let after = program(&session, "file:///home/projects/TS/p1/src/x.ts");
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(
        source_text(&after, "/home/projects/TS/p1/src/x.ts").as_deref(),
        Some("export const x = 2;")
    );
}

#[test]
fn a_change_updates_an_untitled_file() {
    // "DidChangeFile/update untitled file".
    let files = default_files();
    let (session, _) = setup(&files);
    open(&session, "untitled:Untitled-1", "let x = 1;");
    let untitled = &uri_to_file_name("untitled:Untitled-1").expect("a dynamic file name");
    let before = program(&session, "untitled:Untitled-1");
    assert_eq!(
        source_text(&before, untitled).as_deref(),
        Some("let x = 1;")
    );

    session.did_change_file(&uri("untitled:Untitled-1"), 2, vec![edit(8, 9, "2")]);

    let after = program(&session, "untitled:Untitled-1");
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(source_text(&after, untitled).as_deref(), Some("let x = 2;"));
}

#[test]
fn unchanged_source_files_are_reused() {
    // "DidChangeFile/unchanged source files are reused".
    let files = default_files();
    let (session, _) = setup(&files);
    open(&session, "file:///home/projects/TS/p1/src/x.ts", P1_X);
    let before = program(&session, "file:///home/projects/TS/p1/src/x.ts");

    session.did_change_file(
        &uri("file:///home/projects/TS/p1/src/x.ts"),
        2,
        vec![edit(0, 0, ";")],
    );

    let after = program(&session, "file:///home/projects/TS/p1/src/x.ts");
    assert!(Arc::ptr_eq(
        &document(&before, "/home/projects/TS/p1/src/index.ts"),
        &document(&after, "/home/projects/TS/p1/src/index.ts")
    ));
}

#[test]
fn a_change_can_pull_in_new_files() {
    // "DidChangeFile/change can pull in new files".
    let mut files = default_files();
    files.push(("/home/projects/TS/p1/y.ts", "export const y = 2;"));
    let (session, _) = setup(&files);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );
    let before = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    assert!(source_text(&before, "/home/projects/TS/p1/y.ts").is_none());

    session.did_change_file(
        &uri("file:///home/projects/TS/p1/src/index.ts"),
        2,
        vec![edit(0, 0, "import { y } from \"../y\";\n")],
    );

    let after = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    assert!(source_text(&after, "/home/projects/TS/p1/y.ts").is_some());
}

#[test]
fn a_file_change_then_a_config_change_reload_the_program() {
    // "DidChangeFile/single-file change followed by config change reloads
    // program".
    let mut files = default_files();
    files[0].1 = r#"{
        "compilerOptions": { "noLib": true, "module": "nodenext", "strict": true },
        "include": ["src/index.ts"]
    }"#;
    let (session, fs) = setup(&files);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );
    let before = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    assert_eq!(before.prepared().source_files().len(), 2);

    session.did_change_file(
        &uri("file:///home/projects/TS/p1/src/index.ts"),
        2,
        vec![edit(0, 0, "\n")],
    );
    fs.write(
        "/home/projects/TS/p1/tsconfig.json",
        br#"{
            "compilerOptions": { "noLib": true, "module": "nodenext", "strict": true },
            "include": ["./**/*"]
        }"#,
    )
    .expect("write the config");
    session.did_change_watched_files(&[watch_event(
        "file:///home/projects/TS/p1/tsconfig.json",
        FileChangeType::CHANGED,
    )]);

    let after = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    assert_eq!(after.prepared().source_files().len(), 3);
}

/// "DidCloseFile/.../delete a file, close it, recreate it".
fn delete_close_recreate(files: &[(&str, &str)]) {
    let (session, fs) = setup(files);
    open(&session, "file:///home/projects/TS/p1/src/x.ts", P1_X);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );

    fs.remove("/home/projects/TS/p1/src/x.ts")
        .expect("delete x.ts");
    session.did_close_file(&uri("file:///home/projects/TS/p1/src/x.ts"));
    let closed = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    assert!(source_text(&closed, "/home/projects/TS/p1/src/x.ts").is_none());

    fs.write("/home/projects/TS/p1/src/x.ts", b"")
        .expect("recreate x.ts");
    open(&session, "file:///home/projects/TS/p1/src/x.ts", "");
    let reopened = program(&session, "file:///home/projects/TS/p1/src/x.ts");
    assert_eq!(
        source_text(&reopened, "/home/projects/TS/p1/src/x.ts").as_deref(),
        Some("")
    );
}

#[test]
fn a_deleted_and_closed_file_of_a_configured_project_comes_back() {
    delete_close_recreate(&default_files());
}

#[test]
fn a_deleted_and_closed_file_of_the_inferred_project_comes_back() {
    let mut files = default_files();
    files.retain(|(name, _)| !name.ends_with("tsconfig.json"));
    delete_close_recreate(&files);
}

#[test]
fn closing_an_untitled_file() {
    // "DidCloseFile/Inferred projects/close untitled file".
    let files = default_files();
    let (session, _) = setup(&files);
    open(&session, "untitled:Untitled-1", "let x = 1;");
    session.did_close_file(&uri("untitled:Untitled-1"));
    open(&session, "untitled:Untitled-2", "");
}

/// "DidSaveFile/...": the save and the watch event need no new snapshot,
/// and the next update sees the document matching the disk.
fn save_and_watch(save_first: bool) {
    let files = default_files();
    let (session, _) = setup(&files);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );
    assert_eq!(session.snapshot().id(), 1);

    let index = uri("file:///home/projects/TS/p1/src/index.ts");
    let watch = [watch_event(index.as_str(), FileChangeType::CHANGED)];
    if save_first {
        session.did_save_file(&index);
        session.did_change_watched_files(&watch);
    } else {
        session.did_change_watched_files(&watch);
        session.did_save_file(&index);
    }
    session.wait_for_background_tasks();
    // No new snapshot was needed, but the open documents changed.
    assert_eq!(session.snapshot().id(), 1);

    open(&session, "file:///home/projects/TS/p1/src/x.ts", P1_X);
    let snapshot = session.snapshot();
    assert!(snapshot
        .overlay("/home/projects/TS/p1/src/index.ts")
        .expect("the open document")
        .matches_disk_text());
}

#[test]
fn a_save_then_its_watch_event() {
    save_and_watch(true);
}

#[test]
fn a_watch_event_then_its_save() {
    save_and_watch(false);
}

#[test]
fn projects_with_similar_options_share_source_files() {
    // "Source file sharing/projects with similar options share source
    // files".
    let mut files = default_files();
    files.push((
        "/home/projects/TS/p2/tsconfig.json",
        r#"{
            "compilerOptions": {
                "noLib": true,
                "module": "nodenext",
                "strict": true,
                "noCheck": true
            }
        }"#,
    ));
    files.push((
        "/home/projects/TS/p2/src/index.ts",
        r#"import { x } from "../../p1/src/x";"#,
    ));
    let (session, _) = setup(&files);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );
    open(
        &session,
        "file:///home/projects/TS/p2/src/index.ts",
        files[5].1,
    );
    assert_eq!(session.snapshot().projects().count(), 2);

    let program1 = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    let program2 = program(&session, "file:///home/projects/TS/p2/src/index.ts");
    assert!(Arc::ptr_eq(
        &document(&program1, "/home/projects/TS/p1/src/x.ts"),
        &document(&program2, "/home/projects/TS/p1/src/x.ts")
    ));
}

#[test]
fn projects_with_different_options_do_not_share_source_files() {
    // "Source file sharing/projects with different options do not share
    // source files".
    let mut files = default_files();
    files.push((
        "/home/projects/TS/p2/tsconfig.json",
        r#"{
            "compilerOptions": {
                "noLib": true,
                "module": "nodenext",
                "strict": true,
                "moduleDetection": "auto"
            },
            "include": ["src"]
        }"#,
    ));
    files.push((
        "/home/projects/TS/p2/src/index.ts",
        r#"import { x } from "../../p1/src/x";"#,
    ));
    let (session, _) = setup(&files);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );
    open(
        &session,
        "file:///home/projects/TS/p2/src/index.ts",
        files[5].1,
    );
    assert_eq!(session.snapshot().projects().count(), 2);

    let program1 = program(&session, "file:///home/projects/TS/p1/src/index.ts");
    let program2 = program(&session, "file:///home/projects/TS/p2/src/index.ts");
    assert!(!Arc::ptr_eq(
        &document(&program1, "/home/projects/TS/p1/src/x.ts"),
        &document(&program2, "/home/projects/TS/p1/src/x.ts")
    ));
}

#[test]
fn a_document_without_a_project_is_an_error() {
    // tsgo `getSnapshotAndDefaultProject`'s "no project found for URI".
    let files = default_files();
    let (session, _) = setup(&files);
    let error = session
        .get_snapshot_and_default_project(&uri("file:///home/projects/TS/p1/missing.ts"))
        .err()
        .expect("no project");
    assert_eq!(
        error,
        DocumentProjectError::NoProject {
            uri: "file:///home/projects/TS/p1/missing.ts".to_owned(),
            unknown_script_kind: false,
        }
    );
    assert_eq!(
        error.to_string(),
        "no project found for URI file:///home/projects/TS/p1/missing.ts"
    );
}

#[test]
fn the_snapshot_keeps_the_editor_documents() {
    let files = default_files();
    let (session, _) = setup(&files);
    open(
        &session,
        "file:///home/projects/TS/p1/src/index.ts",
        P1_INDEX,
    );
    let snapshot = session.snapshot();
    assert!(snapshot.is_open_file("/home/projects/TS/p1/src/index.ts"));
    assert!(!snapshot.is_open_file("/home/projects/TS/p1/src/x.ts"));
    let project = snapshot
        .default_project("/home/projects/TS/p1/src/index.ts")
        .expect("the default project");
    assert_eq!(project.kind(), ProjectKind::Configured);
}

// tsgo `TestProjectLifetime` (projectlifetime_test.go), without the file
// watcher calls (L2b).

/// tsgo's three projects `p1`, `p2` and `p3` of "configured project".
fn three_projects(with_configs: bool) -> Vec<(String, &'static str)> {
    let mut files = Vec::new();
    for project in ["p1", "p2", "p3"] {
        if with_configs {
            files.push((
                format!("/home/projects/TS/{project}/tsconfig.json"),
                P1_TSCONFIG,
            ));
        }
        files.push((
            format!("/home/projects/TS/{project}/src/index.ts"),
            P1_INDEX,
        ));
        files.push((format!("/home/projects/TS/{project}/src/x.ts"), P1_X));
        files.push((
            format!("/home/projects/TS/{project}/config.ts"),
            P1_CONFIG_TS,
        ));
    }
    files
}

fn borrowed<'a>(files: &'a [(String, &'static str)]) -> Vec<(&'a str, &'a str)> {
    files
        .iter()
        .map(|(name, text)| (name.as_str(), *text))
        .collect()
}

#[test]
fn configured_projects_follow_the_open_files() {
    // "configured project".
    let files = three_projects(true);
    let (session, _) = setup(&borrowed(&files));
    let index = |project: &str| format!("file:///home/projects/TS/{project}/src/index.ts");
    let config = |project: &str| format!("/home/projects/ts/{project}/tsconfig.json");
    let has_project = |snapshot: &tsc_project::Snapshot, project: &str| {
        snapshot.project(&configured(&config(project))).is_some()
    };
    let has_config = |snapshot: &tsc_project::Snapshot, project: &str| {
        snapshot.config(&config(project)).is_some()
    };

    open(&session, &index("p1"), P1_INDEX);
    open(&session, &index("p2"), P1_INDEX);
    session.wait_for_background_tasks();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 2);
    assert!(has_project(&snapshot, "p1") && has_project(&snapshot, "p2"));
    assert!(has_config(&snapshot, "p1") && has_config(&snapshot, "p2"));

    // Closing p1's file and opening p3's replaces p1 with p3.
    session.did_close_file(&uri(&index("p1")));
    open(&session, &index("p3"), P1_INDEX);
    session.wait_for_background_tasks();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 2);
    assert!(!has_project(&snapshot, "p1"));
    assert!(has_project(&snapshot, "p2") && has_project(&snapshot, "p3"));
    assert!(!has_config(&snapshot, "p1"));
    assert!(has_config(&snapshot, "p2") && has_config(&snapshot, "p3"));

    session.did_close_file(&uri(&index("p2")));
    session.did_close_file(&uri(&index("p3")));
    open(&session, &index("p1"), P1_INDEX);
    session.wait_for_background_tasks();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(has_project(&snapshot, "p1"));
    assert!(has_config(&snapshot, "p1"));
    assert!(!has_config(&snapshot, "p2") && !has_config(&snapshot, "p3"));
}

#[test]
fn unrooted_files_share_one_inferred_project() {
    // "unrooted inferred projects".
    let files = three_projects(false);
    let (session, _) = setup(&borrowed(&files));
    let index = |project: &str| format!("file:///home/projects/TS/{project}/src/index.ts");
    let one_inferred = |session: &Session| {
        let snapshot = session.snapshot();
        assert_eq!(snapshot.projects().count(), 1);
        assert!(snapshot.inferred_project().is_some());
    };

    open(&session, &index("p1"), P1_INDEX);
    open(&session, &index("p2"), P1_INDEX);
    one_inferred(&session);
    session.did_close_file(&uri(&index("p1")));
    open(&session, &index("p3"), P1_INDEX);
    one_inferred(&session);
    session.did_close_file(&uri(&index("p2")));
    session.did_close_file(&uri(&index("p3")));
    open(&session, &index("p1"), P1_INDEX);
    one_inferred(&session);
}

#[test]
fn a_file_moves_from_the_inferred_to_a_configured_project() {
    // "file moves from inferred to configured project".
    let files = [
        ("/home/projects/ts/foo.ts", "export const foo = 1;"),
        (
            "/home/projects/ts/p1/tsconfig.json",
            r#"{
                "compilerOptions": { "noLib": true, "module": "nodenext", "strict": true },
                "include": ["main.ts"]
            }"#,
        ),
        (
            "/home/projects/ts/p1/main.ts",
            r#"import { foo } from "../foo"; console.log(foo);"#,
        ),
    ];
    let (session, _) = setup(&files);
    let p1 = configured("/home/projects/ts/p1/tsconfig.json");

    open(&session, "file:///home/projects/ts/foo.ts", files[0].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    assert!(snapshot.project(&p1).is_none());

    // main.ts finds the config, whose program has foo.ts.
    open(&session, "file:///home/projects/ts/p1/main.ts", files[2].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_none());
    assert!(snapshot.project(&p1).is_some());
    assert!(snapshot
        .config("/home/projects/ts/p1/tsconfig.json")
        .is_some());

    // foo.ts keeps the project open.
    session.did_close_file(&uri("file:///home/projects/ts/p1/main.ts"));
    session.wait_for_background_tasks();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.project(&p1).is_some());

    // The project stays until the next open.
    session.did_close_file(&uri("file:///home/projects/ts/foo.ts"));
    session.wait_for_background_tasks();
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot
        .config("/home/projects/ts/p1/tsconfig.json")
        .is_some());
}

#[test]
fn a_moved_file_joins_its_configured_project() {
    // "file move from inferred to configured via didOpen/didClose
    // sequence".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
        ),
        ("/home/projects/TS/p1/index.ts", "export const x = 1;"),
    ];
    let (session, fs) = setup(&files);
    let p1 = configured("/home/projects/ts/p1/tsconfig.json");
    let index = "file:///home/projects/TS/p1/index.ts";
    let src_index = "file:///home/projects/TS/p1/src/index.ts";

    open(&session, index, files[1].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    assert!(snapshot.project(&p1).is_none());

    // The editor moves the file: it opens the new one, closes the old,
    // and the watcher reports the creation and the deletion.
    fs.write_creating_dirs("/home/projects/TS/p1/src/index.ts", files[1].1.as_bytes())
        .expect("write src/index.ts");
    fs.remove("/home/projects/TS/p1/index.ts")
        .expect("delete index.ts");
    open(&session, src_index, files[1].1);
    session.did_close_file(&uri(index));
    session.did_change_watched_files(&[
        watch_event(src_index, FileChangeType::CREATED),
        watch_event(index, FileChangeType::DELETED),
    ]);

    session
        .get_snapshot_and_default_project(&uri(src_index))
        .expect("the project of src/index.ts");
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_none());
    assert!(snapshot.project(&p1).is_some());
}

/// "newly created file joins configured project with ./-prefixed includes".
fn a_new_file_joins_its_nested_project(open_before_create_event: bool) {
    const ES2015: i32 = 2;
    const ESNEXT: i32 = 99;
    let files = [
        (
            "/home/projects/TS/monorepo/tsconfig.json",
            r#"{ "compilerOptions": { "target": "ES2015", "lib": ["DOM"] }, "include": ["apps"] }"#,
        ),
        (
            "/home/projects/TS/monorepo/apps/web/tsconfig.json",
            r#"{
                "compilerOptions": { "target": "ESNext", "lib": ["ESNext"] },
                "include": ["./app/**/*.ts"],
                "exclude": ["**/.*/"]
            }"#,
        ),
        (
            "/home/projects/TS/monorepo/apps/web/app/existing.ts",
            "export const existing = 1;",
        ),
    ];
    let (session, fs) = setup(&files);
    let target = |document: &str| {
        program(&session, document)
            .prepared()
            .compiler_options()
            .target
    };

    let existing = "file:///home/projects/TS/monorepo/apps/web/app/existing.ts";
    open(&session, existing, files[2].1);
    assert_eq!(target(existing), Some(ESNEXT));

    let new_file = "file:///home/projects/TS/monorepo/apps/web/app/subdir/new.ts";
    let content = "export const newFile = 1;";
    fs.write_creating_dirs(
        "/home/projects/TS/monorepo/apps/web/app/subdir/new.ts",
        content.as_bytes(),
    )
    .expect("write new.ts");
    let create = || {
        session.did_change_watched_files(&[watch_event(new_file, FileChangeType::CREATED)]);
    };
    if open_before_create_event {
        open(&session, new_file, content);
        create();
    } else {
        create();
        open(&session, new_file, content);
    }

    assert_eq!(target(new_file), Some(ESNEXT));
    assert_ne!(target(new_file), Some(ES2015));
    assert!(session.snapshot().inferred_project().is_none());
}

#[test]
fn a_new_file_opened_before_its_create_event_joins_its_project() {
    a_new_file_joins_its_nested_project(true);
}

#[test]
fn a_new_file_opened_after_its_create_event_joins_its_project() {
    a_new_file_joins_its_nested_project(false);
}

#[test]
fn a_config_moved_to_the_parent_takes_the_file() {
    // "tsconfig move from subdirectory to parent via didChangeWatchedFiles".
    let config = r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#;
    let files = [
        ("/home/projects/TS/p1/src/tsconfig.json", config),
        ("/home/projects/TS/p1/src/index.ts", "export const x = 1;"),
        ("/home/projects/TS/p1/src/other.ts", "export const y = 2;"),
    ];
    let (session, fs) = setup(&files);
    let index = "file:///home/projects/TS/p1/src/index.ts";

    open(&session, index, files[1].1);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
    assert!(snapshot
        .project(&configured("/home/projects/ts/p1/src/tsconfig.json"))
        .is_none());

    fs.write("/home/projects/TS/p1/tsconfig.json", config.as_bytes())
        .expect("write the new config");
    fs.remove("/home/projects/TS/p1/src/tsconfig.json")
        .expect("delete the old config");
    session.did_change_watched_files(&[
        watch_event(
            "file:///home/projects/TS/p1/tsconfig.json",
            FileChangeType::CREATED,
        ),
        watch_event(
            "file:///home/projects/TS/p1/src/tsconfig.json",
            FileChangeType::DELETED,
        ),
    ]);
    session.wait_for_background_tasks();

    // The background update gives index.ts the new project; the inferred
    // project stays until the next open.
    session
        .get_snapshot_and_default_project(&uri(index))
        .expect("the project of index.ts");
    let snapshot = session.snapshot();
    eprintln!(
        "PROBE {:?}",
        snapshot
            .projects()
            .map(|p| (p.kind(), p.config_file_name().map(str::to_owned)))
            .collect::<Vec<_>>()
    );
    assert_eq!(snapshot.projects().count(), 2);
    assert!(snapshot.inferred_project().is_some());
    assert_eq!(
        snapshot
            .default_project("/home/projects/TS/p1/src/index.ts")
            .and_then(|project| project.config_file_name()),
        Some("/home/projects/TS/p1/tsconfig.json")
    );

    open(
        &session,
        "file:///home/projects/TS/p1/src/other.ts",
        files[2].1,
    );
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_none());
    assert!(snapshot
        .project(&configured("/home/projects/ts/p1/tsconfig.json"))
        .is_some());
}

#[test]
fn a_deleted_open_file_stays_in_its_project_until_closed() {
    // "deleted open file remains in project until closed".
    let files = [
        (
            "/home/projects/TS/p1/tsconfig.json",
            r#"{ "compilerOptions": { "noLib": true }, "include": ["src"] }"#,
        ),
        ("/home/projects/TS/p1/src/index.ts", ""),
        ("/home/projects/TS/p1/src/x.ts", "export const x = 1;"),
    ];
    let (session, fs) = setup(&files);
    let index = "file:///home/projects/TS/p1/src/index.ts";
    let x = "file:///home/projects/TS/p1/src/x.ts";
    open(&session, index, files[1].1);
    open(&session, x, files[2].1);
    let has = |document: &str, file_name: &str| {
        source_text(&program(&session, document), file_name).is_some()
    };
    assert!(has(index, "/home/projects/TS/p1/src/index.ts"));
    assert!(has(index, "/home/projects/TS/p1/src/x.ts"));

    // One batch deletes x.ts, still open, and creates y.ts.
    fs.remove("/home/projects/TS/p1/src/x.ts")
        .expect("delete x.ts");
    fs.write("/home/projects/TS/p1/src/y.ts", b"export const y = 2;")
        .expect("write y.ts");
    session.did_change_watched_files(&[
        watch_event(x, FileChangeType::DELETED),
        watch_event(
            "file:///home/projects/TS/p1/src/y.ts",
            FileChangeType::CREATED,
        ),
    ]);
    assert!(has(x, "/home/projects/TS/p1/src/index.ts"));
    assert!(has(x, "/home/projects/TS/p1/src/x.ts"));
    assert!(has(x, "/home/projects/TS/p1/src/y.ts"));

    session.did_close_file(&uri(x));
    assert!(has(index, "/home/projects/TS/p1/src/index.ts"));
    assert!(!has(index, "/home/projects/TS/p1/src/x.ts"));
    assert!(has(index, "/home/projects/TS/p1/src/y.ts"));
}

// tsgo `TestCustomConfigFileName` (customconfigfilename_test.go), without
// the diagnostics refresh (L2c).

const CUSTOM_FILES: [(&str, &str); 3] = [
    (
        "/src/tsconfig.json",
        r#"{"compilerOptions": {"strict": false}}"#,
    ),
    (
        "/src/tsconfig.all.json",
        r#"{"compilerOptions": {"strict": true}}"#,
    ),
    ("/src/index.ts", "export const x = 1;"),
];
const CUSTOM_INDEX: &str = "file:///src/index.ts";

fn custom_config(name: &str) -> UserPreferences {
    let mut preferences = UserPreferences::new_default();
    preferences.custom_config_file_name = name.to_owned();
    preferences
}

/// The document's default project's config and its program's `strict`.
fn config_and_strict(session: &Session, document: &str) -> (String, Option<bool>) {
    let (snapshot, project) = session
        .get_snapshot_and_default_project(&uri(document))
        .expect("the document's project");
    let file_name = uri_to_file_name(document).expect("a file name");
    let default = snapshot
        .default_project(&file_name)
        .expect("the default project");
    assert!(Arc::ptr_eq(default, &project));
    let strict = project
        .program()
        .expect("the project's program")
        .prepared()
        .compiler_options()
        .strict;
    (
        project.config_file_name().unwrap_or_default().to_owned(),
        strict,
    )
}

#[test]
fn a_custom_config_is_picked_up_when_the_preference_changes() {
    // "picks up custom config and switches on preference change".
    let (session, _) = setup(&CUSTOM_FILES);
    open(&session, CUSTOM_INDEX, CUSTOM_FILES[2].1);
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX),
        ("/src/tsconfig.json".to_owned(), Some(false))
    );

    session.configure(custom_config("tsconfig.all.json"));
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX),
        ("/src/tsconfig.all.json".to_owned(), Some(true))
    );
}

#[test]
fn an_empty_custom_config_name_uses_tsconfig_json() {
    // "uses tsconfig.json when customConfigFileName is empty".
    let (session, _) = setup(&CUSTOM_FILES);
    assert_eq!(UserPreferences::new_default().custom_config_file_name, "");
    session.configure(UserPreferences::new_default());
    open(&session, CUSTOM_INDEX, CUSTOM_FILES[2].1);
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX).0,
        "/src/tsconfig.json"
    );
}

#[test]
fn a_missing_custom_config_falls_back_to_tsconfig_json() {
    // "falls back to tsconfig.json when custom config missing".
    let (session, _) = setup(&CUSTOM_FILES);
    session.configure(custom_config("tsconfig.nonexistent.json"));
    open(&session, CUSTOM_INDEX, CUSTOM_FILES[2].1);
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX).0,
        "/src/tsconfig.json"
    );
}

#[test]
fn clearing_the_custom_config_reverts_to_tsconfig_json() {
    // "reverts to tsconfig.json when custom config preference is cleared".
    let (session, _) = setup(&CUSTOM_FILES);
    open(&session, CUSTOM_INDEX, CUSTOM_FILES[2].1);
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX),
        ("/src/tsconfig.json".to_owned(), Some(false))
    );
    session.configure(custom_config("tsconfig.all.json"));
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX),
        ("/src/tsconfig.all.json".to_owned(), Some(true))
    );
    session.configure(custom_config(""));
    assert_eq!(
        config_and_strict(&session, CUSTOM_INDEX),
        ("/src/tsconfig.json".to_owned(), Some(false))
    );
}

#[test]
fn a_custom_config_covering_the_file_cleans_up_the_inferred_project() {
    // "cleans up inferred project when custom config covers file".
    let files = [
        (
            "/src/tsconfig.all.json",
            r#"{"compilerOptions": {"strict": true}, "include": ["./**/*"]}"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ];
    let (session, _) = setup(&files);
    open(&session, CUSTOM_INDEX, files[1].1);
    let (snapshot, project) = session
        .get_snapshot_and_default_project(&uri(CUSTOM_INDEX))
        .expect("the document's project");
    assert_eq!(project.kind(), ProjectKind::Inferred);
    assert_eq!(snapshot.projects_containing_file("/src/index.ts").len(), 1);

    session.configure(custom_config("tsconfig.all.json"));
    let (snapshot, project) = session
        .get_snapshot_and_default_project(&uri(CUSTOM_INDEX))
        .expect("the document's project");
    assert_eq!(project.config_file_name(), Some("/src/tsconfig.all.json"));
    assert_eq!(snapshot.projects_containing_file("/src/index.ts").len(), 1);
}

// tsgo's untitled tests (untitled_test.go), as far as the projects go; the
// references come with the language service (L3).

#[test]
fn an_untitled_file_is_in_the_inferred_project() {
    // `TestUntitledFileInInferredProject`.
    let content = "let x = 42;\n\nx\n\nx++;";
    let (session, _) = setup(&[]);
    open(&session, "untitled:Untitled-1", "x\n\n");
    open(&session, "untitled:Untitled-2", content);
    assert!(session.snapshot().inferred_project().is_some());
    let program = program(&session, "untitled:Untitled-2");
    let file_name = uri_to_file_name("untitled:Untitled-2").expect("a dynamic file name");
    assert_eq!(source_text(&program, &file_name).as_deref(), Some(content));
}

#[test]
fn an_untitled_file_name_round_trips() {
    // `TestUntitledReferences`' URI conversion.
    let file_name = uri_to_file_name("untitled:Untitled-2").expect("a dynamic file name");
    assert_eq!(
        tsc_program::uri::file_name_to_document_uri(&file_name),
        "untitled:Untitled-2"
    );
}

#[test]
fn imports_in_an_untitled_file() {
    // `TestImportsInUntitled`, without automatic type acquisition.
    let (session, _) = setup(&[(
        "/home/src/Library/Caches/typescript/node_modules/@types/somelib/index.d.ts",
        "export const x: number;",
    )]);
    let content =
        "import \"https://deno.land/std@0.208.0/path/mod.ts\"\n\t\timport  \"./relative\"\n";
    open(&session, "untitled:Untitled-1", content);
    session
        .get_snapshot_and_default_project(&uri("untitled:Untitled-1"))
        .expect("the untitled file's project");
}

// tsgo `TestConfigFileChanges` (configfilechanges_test.go).

/// tsgo's files of the config change tests.
fn config_change_files() -> Vec<(&'static str, &'static str)> {
    vec![
        ("/tsconfig.more-base.json", "{}"),
        (
            "/tsconfig.base.json",
            r#"{"extends": "../tsconfig.more-base.json", "compilerOptions": {"strict": true}}"#,
        ),
        (
            "/src/tsconfig.json",
            r#"{"extends": "../tsconfig.base.json", "compilerOptions": {"target": "es6"}, "references": [{"path": "../utils"}]}"#,
        ),
        ("/src/index.ts", r#"console.log("Hello, world!");"#),
        ("/src/subfolder/foo.ts", r#"export const foo = "bar";"#),
        (
            "/utils/tsconfig.json",
            r#"{"compilerOptions": {"composite": true}}"#,
        ),
        ("/utils/index.ts", r#"console.log("Hello, test!");"#),
    ]
}

const CONFIG_INDEX: &str = "file:///src/index.ts";

/// The files opened with /src/index.ts, `file_name` written with `text`
/// and its change reported; the program of index.ts after it.
fn after_config_change(
    files: &[(&str, &str)],
    file_name: &str,
    text: &str,
    kind: FileChangeType,
) -> (Session, Arc<ProjectProgram>) {
    let (session, fs) = setup(files);
    open(&session, CONFIG_INDEX, r#"console.log("Hello, world!");"#);
    fs.write(file_name, text.as_bytes())
        .expect("write the config");
    session.did_change_watched_files(&[watch_event(&format!("file://{file_name}"), kind)]);
    let program = program(&session, CONFIG_INDEX);
    (session, program)
}

#[test]
fn a_config_change_updates_the_program_options() {
    // "should update program options on config file change".
    const ESNEXT: i32 = 99;
    let (_, program) = after_config_change(
        &config_change_files(),
        "/src/tsconfig.json",
        r#"{"extends": "../tsconfig.base.json", "compilerOptions": {"target": "esnext"}, "references": [{"path": "../utils"}]}"#,
        FileChangeType::CHANGED,
    );
    assert_eq!(program.prepared().compiler_options().target, Some(ESNEXT));
}

#[test]
fn an_extended_config_change_updates_the_project() {
    // "should update project on extended config file change".
    let (_, program) = after_config_change(
        &config_change_files(),
        "/tsconfig.base.json",
        r#"{"compilerOptions": {"strict": false}}"#,
        FileChangeType::CHANGED,
    );
    assert_eq!(program.prepared().compiler_options().strict, Some(false));
}

#[test]
fn a_doubly_extended_config_change_updates_the_project() {
    // "should update project on doubly extended config file change".
    let (_, program) = after_config_change(
        &config_change_files(),
        "/tsconfig.more-base.json",
        r#"{"compilerOptions": {"verbatimModuleSyntax": true}}"#,
        FileChangeType::CHANGED,
    );
    assert_eq!(
        program.prepared().compiler_options().verbatim_module_syntax,
        Some(true)
    );
}

#[test]
fn a_referenced_config_change_updates_the_snapshot() {
    // "should update project on referenced config file change".
    let files = config_change_files();
    let (session, fs) = setup(&files);
    open(&session, CONFIG_INDEX, files[3].1);
    let before = session.snapshot();
    fs.write(
        "/utils/tsconfig.json",
        br#"{"compilerOptions": {"composite": true, "target": "esnext"}}"#,
    )
    .expect("write the config");
    session.did_change_watched_files(&[watch_event(
        "file:///utils/tsconfig.json",
        FileChangeType::CHANGED,
    )]);
    program(&session, CONFIG_INDEX);
    assert!(!Arc::ptr_eq(&session.snapshot(), &before));
}

#[test]
fn a_deleted_config_closes_its_project() {
    // "should close project on config file deletion".
    let files = config_change_files();
    let (session, fs) = setup(&files);
    open(&session, CONFIG_INDEX, files[3].1);
    fs.remove("/src/tsconfig.json").expect("delete the config");
    session.did_change_watched_files(&[watch_event(
        "file:///src/tsconfig.json",
        FileChangeType::DELETED,
    )]);
    program(&session, CONFIG_INDEX);
    let snapshot = session.snapshot();
    assert_eq!(snapshot.projects().count(), 1);
    assert!(snapshot.inferred_project().is_some());
}

#[test]
fn a_config_created_then_deleted() {
    // "config file creation then deletion".
    let files = config_change_files();
    let (session, fs) = setup(&files);
    let foo = "file:///src/subfolder/foo.ts";
    open(&session, foo, files[4].1);

    fs.write("/src/subfolder/tsconfig.json", b"{}")
        .expect("write the config");
    session.did_change_watched_files(&[watch_event(
        "file:///src/subfolder/tsconfig.json",
        FileChangeType::CREATED,
    )]);
    let (snapshot, project) = session
        .get_snapshot_and_default_project(&uri(foo))
        .expect("the project of foo.ts");
    assert_eq!(snapshot.projects().count(), 2);
    assert_eq!(
        project.config_file_name(),
        Some("/src/subfolder/tsconfig.json")
    );

    fs.remove("/src/subfolder/tsconfig.json")
        .expect("delete the config");
    session.did_change_watched_files(&[watch_event(
        "file:///src/subfolder/tsconfig.json",
        FileChangeType::DELETED,
    )]);
    let (snapshot, project) = session
        .get_snapshot_and_default_project(&uri(foo))
        .expect("the project of foo.ts");
    assert_eq!(project.config_file_name(), Some("/src/tsconfig.json"));
    // The old project goes at the next open.
    assert_eq!(snapshot.projects().count(), 2);

    open(&session, CONFIG_INDEX, files[3].1);
    assert_eq!(session.snapshot().projects().count(), 1);
}

#[test]
fn a_created_missing_extended_config_updates_the_project() {
    // "should update project when missing extended config is created".
    let mut files = config_change_files();
    files.retain(|(name, _)| *name != "/tsconfig.base.json");
    let (_, program) = after_config_change(
        &files,
        "/tsconfig.base.json",
        r#"{"compilerOptions": {"strict": true}}"#,
        FileChangeType::CREATED,
    );
    assert_eq!(program.prepared().compiler_options().strict, Some(true));
}

// tsgo `TestProjectProgramUpdateKind` (project_test.go); "compiler options
// update inferred project" waits for the test harness's
// SetCompilerOptionsForInferredProjects (L3).

/// The configured project of /src/tsconfig.json.
fn src_project(session: &Session) -> Arc<tsc_project::Project> {
    Arc::clone(
        session
            .snapshot()
            .project(&configured("/src/tsconfig.json"))
            .expect("the configured project"),
    )
}

#[test]
fn an_initial_build_has_new_files() {
    // "NewFiles on initial build".
    let files = [
        ("/src/tsconfig.json", "{}"),
        ("/src/index.ts", "export const x = 1;"),
    ];
    let (session, _) = setup(&files);
    open(&session, CONFIG_INDEX, files[1].1);
    program(&session, CONFIG_INDEX);
    assert_eq!(
        src_project(&session).program_update_kind(),
        ProgramUpdateKind::NewFiles
    );
}

#[test]
fn a_one_file_change_clones_the_program() {
    // "Cloned on single-file change".
    let files = [
        ("/src/tsconfig.json", "{}"),
        ("/src/index.ts", "console.log('Hello');"),
    ];
    let (session, _) = setup(&files);
    open(&session, CONFIG_INDEX, files[1].1);
    program(&session, CONFIG_INDEX);
    session.did_change_file(&uri(CONFIG_INDEX), 2, vec![edit(20, 20, "\n")]);
    program(&session, CONFIG_INDEX);
    assert_eq!(
        src_project(&session).program_update_kind(),
        ProgramUpdateKind::Cloned
    );
}

#[test]
fn a_changed_resolution_mode_has_new_files() {
    // "NewFiles when import resolution mode changes".
    let files = [
        (
            "/src/tsconfig.json",
            r#"{
				"compilerOptions":{"module":"preserve","moduleResolution":"bundler","noEmit":true},
				"files":["index.ts"]
			}"#,
        ),
        (
            "/src/index.ts",
            "import type { Value } from \"pkg\" with { \"resolution-mode\": \"require\" };\nconst value: Value = { mode: \"require\" };",
        ),
        (
            "/src/node_modules/pkg/package.json",
            r#"{
				"name": "pkg",
				"version": "1.0.0",
				"exports": {
					".": {
						"import": "./index.mjs",
						"require": "./index.js"
					}
				}
			}"#,
        ),
        (
            "/src/node_modules/pkg/index.d.mts",
            r#"export interface Value { mode: "import" }"#,
        ),
        (
            "/src/node_modules/pkg/index.d.ts",
            r#"export interface Value { mode: "require" }"#,
        ),
    ];
    let (session, _) = setup(&files);
    open(&session, CONFIG_INDEX, files[1].1);
    let semantic = |session: &Session| {
        program(session, CONFIG_INDEX).with_live(|live| {
            let file = live.file_index("/src/index.ts").expect("index.ts");
            live.semantic_diagnostics(file)
                .expect("check index.ts")
                .iter()
                .map(|diagnostic| diagnostic.code())
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(semantic(&session), Vec::<u32>::new());

    session.did_change_file(
        &uri(CONFIG_INDEX),
        2,
        vec![TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
            TextDocumentContentChangeWholeDocument {
                text: "import type { Value } from \"pkg\" with { \"resolution-mode\": \"import\" };\nconst value: Value = { mode: \"require\" };".to_owned(),
            },
        )],
    );
    // TS2322: Type '{0}' is not assignable to type '{1}'.
    assert_eq!(semantic(&session), [2322]);
    assert_eq!(
        src_project(&session).program_update_kind(),
        ProgramUpdateKind::NewFiles
    );
}

#[test]
fn a_config_change_without_new_roots_keeps_the_file_names() {
    // "SameFileNames on config change without root changes".
    let files = [
        (
            "/src/tsconfig.json",
            r#"{"compilerOptions": {"strict": true}}"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ];
    let (session, fs) = setup(&files);
    open(&session, CONFIG_INDEX, files[1].1);
    program(&session, CONFIG_INDEX);
    fs.write(
        "/src/tsconfig.json",
        br#"{"compilerOptions": {"strict": false}}"#,
    )
    .expect("write the config");
    session.did_change_watched_files(&[watch_event(
        "file:///src/tsconfig.json",
        FileChangeType::CHANGED,
    )]);
    program(&session, CONFIG_INDEX);
    assert_eq!(
        src_project(&session).program_update_kind(),
        ProgramUpdateKind::SameFileNames
    );
}

#[test]
fn a_new_root_has_new_files() {
    // "NewFiles on root addition".
    let files = [("/src/tsconfig.json", "{}"), ("/src/index.ts", "export {}")];
    let (session, fs) = setup(&files);
    open(&session, CONFIG_INDEX, files[1].1);
    program(&session, CONFIG_INDEX);
    let content = "export const y = 2;";
    fs.write("/src/newfile.ts", content.as_bytes())
        .expect("write newfile.ts");
    session.did_change_watched_files(&[watch_event(
        "file:///src/newfile.ts",
        FileChangeType::CREATED,
    )]);
    open(&session, "file:///src/newfile.ts", content);
    program(&session, "file:///src/newfile.ts");
    assert_eq!(
        src_project(&session).program_update_kind(),
        ProgramUpdateKind::NewFiles
    );
}

#[test]
fn an_unresolvable_import_keeps_the_file_names() {
    // "SameFileNames when adding an unresolvable import with multi-file
    // change".
    let files = [
        ("/src/tsconfig.json", "{}"),
        ("/src/index.ts", "export const x = 1;"),
        ("/src/other.ts", "export const z = 3;"),
    ];
    let (session, _) = setup(&files);
    open(&session, CONFIG_INDEX, files[1].1);
    program(&session, CONFIG_INDEX);
    session.did_change_file(
        &uri(CONFIG_INDEX),
        2,
        vec![edit(0, 0, "\nimport \"./does-not-exist\";\n")],
    );
    program(&session, CONFIG_INDEX);
    assert_eq!(
        src_project(&session).program_update_kind(),
        ProgramUpdateKind::SameFileNames
    );
}

#[test]
fn a_missing_file_is_not_read() {
    // tsgo `TestSnapshot` "GetFile returns nil for non-existent files".
    let files = [
        ("/home/projects/TS/p1/tsconfig.json", "{}"),
        (
            "/home/projects/TS/p1/index.ts",
            "console.log('Hello, world!');",
        ),
    ];
    let (session, _) = setup(&files);
    open(&session, "file:///home/projects/TS/p1/index.ts", files[1].1);
    assert!(session
        .snapshot()
        .read_file("/home/projects/TS/p1/nonexistent.ts")
        .is_none());
}
