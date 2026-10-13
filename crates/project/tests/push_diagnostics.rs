//! tsgo's push diagnostics tests (`project/project_test.go`
//! `TestPushDiagnostics`): the config diagnostics the session publishes on
//! a configured project's `tsconfig.json`. The global diagnostics found by
//! checking come with the language service's requests (L3).

mod common;

use common::*;
use tsc_host::vfs::FileSystem;
use tsc_ls::lsutil::UserPreferences;
use tsc_lsproto::{FileChangeType, PublishDiagnosticsParams, StringOrMarkupContent};
use tsc_project::Session;

const INDEX: &str = "file:///src/index.ts";
const TSCONFIG: &str = "file:///src/tsconfig.json";

/// tsgo `filterDiagnosticsByURI`: the calls for `uri` from `start` on.
fn calls_for(
    calls: &[PublishDiagnosticsParams],
    uri: &str,
    start: usize,
) -> Vec<PublishDiagnosticsParams> {
    calls[start.min(calls.len())..]
        .iter()
        .filter(|call| call.uri.as_str() == uri)
        .cloned()
        .collect()
}

fn message(diagnostic: &tsc_lsproto::Diagnostic) -> &str {
    match &diagnostic.message {
        StringOrMarkupContent::String(text) => text,
        StringOrMarkupContent::MarkupContent(content) => &content.value,
    }
}

/// The files opened with /src/index.ts, its project loaded and the
/// background work done.
fn opened(
    files: &[(&str, &str)],
) -> (
    Session,
    std::sync::Arc<tsc_host::vfs::MemFs>,
    std::sync::Arc<ClientMock>,
) {
    let (session, fs, client) = setup_with_client(files);
    open(&session, INDEX, "export const x = 1;");
    program(&session, INDEX);
    session.wait_for_background_tasks();
    (session, fs, client)
}

#[test]
fn program_diagnostics_are_published_on_the_first_program() {
    // "publishes program diagnostics on initial program creation".
    let (_, _, client) = opened(&[
        (
            "/src/tsconfig.json",
            r#"{"compilerOptions": {"baseUrl": "."}}"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let calls = calls_for(&client.publish_diagnostics_calls(), TSCONFIG, 0);
    assert!(!calls.is_empty(), "a call for tsconfig.json");
    assert!(!calls[0].diagnostics.is_empty(), "a diagnostic");
}

#[test]
fn config_parsing_diagnostics_are_published_on_the_first_program() {
    // "publishes config parsing diagnostics on initial program creation".
    let (_, _, client) = opened(&[
        (
            "/src/tsconfig.json",
            r#"{ "compilerOptions": { "target": "nope" } }"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let calls = calls_for(&client.publish_diagnostics_calls(), TSCONFIG, 0);
    let last = calls.last().expect("a call for tsconfig.json");
    assert!(
        last.diagnostics
            .iter()
            .any(|diagnostic| message(diagnostic)
                .contains("Argument for '--target' option must be:")),
        "{:?}",
        last.diagnostics
    );
}

#[test]
fn a_removed_project_clears_its_diagnostics() {
    // "clears diagnostics when project is removed".
    let files = [
        (
            "/src/tsconfig.json",
            r#"{"compilerOptions": {"baseUrl": "."}}"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
        ("/src2/tsconfig.json", r#"{"compilerOptions": {}}"#),
        ("/src2/index.ts", "export const y = 2;"),
    ];
    let (session, _, client) = opened(&files);
    session.did_close_file(&uri(INDEX));
    open(&session, "file:///src2/index.ts", files[3].1);
    program(&session, "file:///src2/index.ts");
    session.wait_for_background_tasks();
    let calls = calls_for(&client.publish_diagnostics_calls(), TSCONFIG, 0);
    assert!(calls.len() >= 2, "{calls:?}");
    assert!(calls.last().unwrap().diagnostics.is_empty());
}

#[test]
fn a_program_change_updates_the_diagnostics() {
    // "updates diagnostics when program changes".
    let (session, fs, client) = opened(&[
        (
            "/src/tsconfig.json",
            r#"{"compilerOptions": {"baseUrl": "."}}"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let before = client.publish_diagnostics_calls().len();
    fs.write("/src/tsconfig.json", br#"{"compilerOptions": {}}"#)
        .expect("write the config");
    session.did_change_watched_files(&[watch_event(TSCONFIG, FileChangeType::CHANGED)]);
    program(&session, INDEX);
    session.wait_for_background_tasks();
    let calls = client.publish_diagnostics_calls();
    assert!(calls.len() > before);
    let last = calls_for(&calls, TSCONFIG, 0)
        .pop()
        .expect("a call for tsconfig.json");
    assert!(last.diagnostics.is_empty(), "{:?}", last.diagnostics);
}

#[test]
fn a_config_change_alone_republishes_its_diagnostics() {
    // "updates diagnostics when a config file changes on disk with no
    // follow-up request": the watch event's scheduled update builds the
    // program again.
    let (session, fs, client) = opened(&[
        ("/src/tsconfig.json", r#"{"compilerOptions": {}}"#),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let before = client.publish_diagnostics_calls().len();
    fs.write(
        "/src/tsconfig.json",
        br#"{"compilerOptions": {"target": "nope"}}"#,
    )
    .expect("write the config");
    session.did_change_watched_files(&[watch_event(TSCONFIG, FileChangeType::CHANGED)]);
    session.wait_for_background_tasks();
    let calls = calls_for(&client.publish_diagnostics_calls(), TSCONFIG, before);
    let last = calls.last().expect("a call for tsconfig.json");
    assert!(
        last.diagnostics
            .iter()
            .any(|diagnostic| message(diagnostic)
                .contains("Argument for '--target' option must be:")),
        "{:?}",
        last.diagnostics
    );
}

#[test]
fn inferred_projects_publish_nothing() {
    // "does not publish for inferred projects".
    let (session, _, client) =
        setup_with_client(&[("/src/index.ts", "let x: number = 'not a number';")]);
    open(&session, INDEX, "let x: number = 'not a number';");
    program(&session, INDEX);
    session.wait_for_background_tasks();
    assert!(client.publish_diagnostics_calls().is_empty());
}

fn without_validation() -> UserPreferences {
    let mut preferences = UserPreferences::new_default();
    preferences.validate_enabled = Some(false);
    preferences
}

#[test]
fn nothing_is_published_without_validation() {
    // "does not publish when validation is disabled".
    let files = [
        (
            "/src/tsconfig.json",
            r#"{ "compilerOptions": { "target": "nope" } }"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ];
    let (session, _, client) = setup_with_client(&files);
    session.configure(without_validation());
    open(&session, INDEX, files[1].1);
    program(&session, INDEX);
    session.wait_for_background_tasks();
    for call in client.publish_diagnostics_calls() {
        assert!(call.diagnostics.is_empty(), "{call:?}");
    }
}

#[test]
fn turning_validation_off_clears_the_diagnostics() {
    // "clears diagnostics when validation is disabled".
    let (session, _, client) = opened(&[
        (
            "/src/tsconfig.json",
            r#"{ "compilerOptions": { "target": "nope" } }"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let calls = calls_for(&client.publish_diagnostics_calls(), TSCONFIG, 0);
    assert!(!calls.last().expect("the first call").diagnostics.is_empty());

    session.configure(without_validation());
    program(&session, INDEX);
    session.wait_for_background_tasks();
    let calls = calls_for(&client.publish_diagnostics_calls(), TSCONFIG, 0);
    assert!(calls.last().unwrap().diagnostics.is_empty());
}

#[test]
fn closing_clears_the_diagnostics_and_reopening_restores_them() {
    // "cleans tsconfig diagnostics after TS files close and restores them
    // after TS file is reopened".
    let (session, _, client) = opened(&[
        (
            "/src/tsconfig.json",
            r#"{"compilerOptions": {"baseUrl": "."}}"#,
        ),
        ("/src/index.ts", "export const x = 1;"),
    ]);
    let calls = client.publish_diagnostics_calls();
    let first = calls_for(&calls, TSCONFIG, 0);
    assert_eq!(first.first().expect("a call").diagnostics.len(), 1);

    let before_close = calls.len();
    session.did_close_file(&uri(INDEX));
    session.wait_for_background_tasks();
    let calls = client.publish_diagnostics_calls();
    let cleared = calls_for(&calls, TSCONFIG, before_close);
    assert!(cleared
        .last()
        .expect("a call after the close")
        .diagnostics
        .is_empty());

    let before_reopen = calls.len();
    session
        .did_open_file(
            &uri(INDEX),
            2,
            "export const x = 1;",
            &tsc_lsproto::LanguageKind::TYPE_SCRIPT,
        )
        .expect("reopen the document");
    program(&session, INDEX);
    session.wait_for_background_tasks();
    let calls = client.publish_diagnostics_calls();
    let restored = calls_for(&calls, TSCONFIG, before_reopen);
    assert_eq!(
        restored
            .last()
            .expect("a call after the reopen")
            .diagnostics
            .len(),
        1
    );
}
