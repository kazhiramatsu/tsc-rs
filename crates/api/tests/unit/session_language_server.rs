//! tsgo's session tests over a language server's session
//! (`NewLSPSession`): the language server cases of
//! `session_apistate_test.go`, and those of
//! `session_requestfilesystem_test.go` and `session_completion_test.go`
//! with the editor's open documents, sent as a client sends them.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_compiler::DEFAULT_LOAD_LIMITS;
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, LanguageKind,
    TextDocumentContentChangePartialOrWholeDocument, TextDocumentContentChangeWholeDocument,
};
use tsc_program::LibraryCatalog;

use super::*;

/// tsgo `projecttestutil.Setup`: a language server's session over an
/// in-memory, case-insensitive file system with the bundled libraries.
pub(super) fn project_session(files: &[(&str, &str)]) -> (tsc_project::Session, Arc<MemFs>) {
    project_session_in("/", files)
}

pub(super) fn project_session_in(
    current_directory: &str,
    files: &[(&str, &str)],
) -> (tsc_project::Session, Arc<MemFs>) {
    let fs = Arc::new(
        MemFs::from_entries(
            files.iter().map(|(name, text)| (*name, Seed::file(*text))),
            false,
            Arc::new(SystemClock),
        )
        .expect("build the file system"),
    );
    let session = tsc_project::Session::new(
        SessionOptions {
            current_directory: current_directory.to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1(Path::new(EMBEDDED_LIBRARY_DIRECTORY)),
            load_limits: DEFAULT_LOAD_LIMITS,
            run_external_code: false,
            position_encoding: tsc_lsproto::PositionEncodingKind::UTF8,
            watch_enabled: true,
            logging_enabled: true,
            telemetry_enabled: false,
            push_diagnostics_enabled: true,
            debounce_delay: std::time::Duration::ZERO,
            client_capabilities: Default::default(),
        },
        Arc::new(BundledFs::new(Arc::clone(&fs))) as Arc<dyn FileSystem>,
        None,
        None,
        None,
    );
    (session, fs)
}

pub(super) fn uri(file_name: &str) -> DocumentUri {
    DocumentUri(tsc_program::uri::file_name_to_document_uri(file_name))
}

pub(super) fn open(project_session: &tsc_project::Session, file_name: &str, text: &str) {
    project_session
        .did_open_file(&uri(file_name), 1, text, &LanguageKind::TYPE_SCRIPT)
        .expect("open the document");
}

pub(super) fn changed_on_disk(project_session: &tsc_project::Session, file_name: &str) {
    project_session.did_change_watched_files(&[FileEvent {
        uri: uri(file_name),
        r#type: FileChangeType::CHANGED,
    }]);
}

pub(super) fn current(session: &Session, params: Value) -> Result<Value, String> {
    let text = match session.handle(
        "getCurrentLanguageServerSnapshot",
        params.to_string().as_bytes(),
    )? {
        Payload::Json(text) => text,
        Payload::Binary(_) => panic!("answered in binary"),
    };
    Ok(serde_json::from_str(&text).unwrap())
}

fn open_projects(session: &Session) -> usize {
    lock(&session.language_server).open.projects.len()
}

fn open_files(session: &Session) -> usize {
    lock(&session.language_server).open.files.len()
}

fn configured(path: &str) -> ProjectId {
    ProjectId::configured(path).expect("a configured project ID")
}

fn has_configured_project(project_session: &tsc_project::Session, config: &str) -> bool {
    project_session
        .snapshot()
        .project(&configured(config))
        .is_some()
}

fn configured_project_is_dirty(project_session: &tsc_project::Session, config: &str) -> bool {
    project_session
        .snapshot()
        .project(&configured(config))
        .expect("the configured project")
        .is_dirty()
}

fn synthetic_projects(project_session: &tsc_project::Session) -> usize {
    project_session
        .snapshot()
        .projects()
        .filter(|project| project.kind() == ProjectKind::Synthetic)
        .count()
}

fn create_program(file_name: &str) -> Value {
    json!({ "createPrograms": [{
        "rootFiles": [file_name],
        "compilerOptions": { "noLib": true },
    }] })
}

#[test]
fn the_language_server_adopts_the_changes() {
    // TestGetCurrentLanguageServerSnapshotAdoptsChanges.
    let config = "/home/projects/p/tsconfig.json";
    let file = "/home/projects/p/src/index.ts";
    let (project_session, fs) = project_session(&[
        (config, r#"{ "compilerOptions": { "strict": true } }"#),
        (file, "export const x = 1;"),
    ]);
    let session = Session::new_lsp(&project_session, false);
    let response = current(&session, json!({ "changes": { "openProjects": [config] } })).unwrap();
    assert_eq!(open_projects(&session), 1);
    assert_eq!(response["snapshot"], project_session.snapshot().id());
    assert!(has_configured_project(&project_session, config));
    fs.write(file, b"export const x = 2;").unwrap();
    changed_on_disk(&project_session, file);
    let dirty = current(&session, json!({ "baseSnapshot": response["snapshot"] })).unwrap();
    assert!(configured_project_is_dirty(&project_session, config));

    let unchanged = current(
        &session,
        json!({ "baseSnapshot": dirty["snapshot"], "changes": { "openProjects": [config] } }),
    )
    .unwrap();
    assert_eq!(unchanged["projects"][0]["dirty"], false);
    assert_eq!(open_projects(&session), 1);

    let removed = current(
        &session,
        json!({ "baseSnapshot": unchanged["snapshot"], "changes": { "closeProjects": [config] } }),
    )
    .unwrap();
    assert_eq!(removed["projects"], json!([]));
    assert_eq!(removed["changes"]["removedProjects"], json!([config]));
    assert_eq!(open_projects(&session), 0);

    session.close();
    assert_eq!(open_projects(&session), 0);
    assert!(!has_configured_project(&project_session, config));
    project_session.close();
}

#[test]
fn a_project_closed_and_opened_again_stays_open() {
    // TestGetCurrentLanguageServerSnapshotCloseAndReopenProject.
    let config = "/home/projects/p/tsconfig.json";
    let (project_session, _) = project_session(&[(config, "{}")]);
    let session = Session::new_lsp(&project_session, false);
    current(&session, json!({ "changes": { "openProjects": [config] } })).unwrap();
    current(
        &session,
        json!({ "changes": { "closeProjects": [config], "openProjects": [config] } }),
    )
    .unwrap();
    assert_eq!(open_projects(&session), 1);
    assert!(has_configured_project(&project_session, config));
    current(
        &session,
        json!({ "changes": { "closeProjects": [config] } }),
    )
    .unwrap();
    assert_eq!(open_projects(&session), 0);
    session.close();
    project_session.close();
}

#[test]
fn a_file_closed_and_opened_again_stays_open() {
    // TestGetCurrentLanguageServerSnapshotCloseAndReopenFile.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[
        ("/home/projects/p/tsconfig.json", "{}"),
        (file, "export const value = 1;"),
    ]);
    let session = Session::new_lsp(&project_session, false);
    current(&session, json!({ "changes": { "openFiles": [file] } })).unwrap();
    current(
        &session,
        json!({ "changes": { "closeFiles": [file], "openFiles": [file] } }),
    )
    .unwrap();
    assert_eq!(open_files(&session), 1);
    assert!(project_session.snapshot().default_project(file).is_some());
    current(&session, json!({ "changes": { "closeFiles": [file] } })).unwrap();
    assert_eq!(open_files(&session), 0);
    session.close();
    project_session.close();
}

#[test]
fn the_pending_editor_changes_are_flushed() {
    // TestGetCurrentLanguageServerSnapshotFlushesPendingLSPChanges.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[(file, "export const value: string = 1;")]);
    open(
        &project_session,
        file,
        r#"export const value: string = "ok";"#,
    );
    let session = Session::new_lsp(&project_session, false);
    let response = current(&session, json!({})).unwrap();
    assert_eq!(response["snapshot"], project_session.snapshot().id());
    let snapshot = session
        .snapshot(response["snapshot"].as_u64().unwrap())
        .unwrap();
    assert_eq!(
        snapshot.read_file(file).as_deref(),
        Some(br#"export const value: string = "ok";"#.as_slice())
    );
    session.close();
    project_session.close();
}

#[test]
fn opened_files_are_reported_in_the_request_order() {
    // TestGetCurrentLanguageServerSnapshotReportsOpenedFilesInRequestOrder.
    let config = "/home/projects/p/tsconfig.json";
    let configured_file = "/home/projects/p/index.ts";
    let inferred_file = "/home/projects/loose.ts";
    let (project_session, fs) = project_session(&[
        (config, "{}"),
        (configured_file, "export const configured = 1;"),
        (inferred_file, "export const inferred = 1;"),
    ]);
    let session = Session::new_lsp(&project_session, false);
    let changes = json!({ "openFiles": [inferred_file, configured_file] });
    let first = current(&session, json!({ "changes": changes })).unwrap();
    let opened = first["operation"]["openedFiles"].as_array().unwrap();
    assert_eq!(opened.len(), 2);
    assert_eq!(opened[0]["project"], "/dev/null/inferred");
    assert_eq!(opened[1]["project"], config);
    fs.write(configured_file, b"export const configured = 2;")
        .unwrap();
    changed_on_disk(&project_session, configured_file);
    let dirty = current(&session, json!({ "baseSnapshot": first["snapshot"] })).unwrap();
    assert!(configured_project_is_dirty(&project_session, config));

    let reopened = current(
        &session,
        json!({ "baseSnapshot": dirty["snapshot"], "changes": changes }),
    )
    .unwrap();
    assert_eq!(
        reopened["operation"]["openedFiles"],
        first["operation"]["openedFiles"]
    );
    assert!(!configured_project_is_dirty(&project_session, config));
    assert_eq!(open_files(&session), 2);
    session.close();
    project_session.close();
}

#[test]
fn programs_are_created_and_removed() {
    // TestGetCurrentLanguageServerSnapshotCreatesAndRemovesPrograms.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[(file, "export const value = 1;")]);
    let session = Session::new_lsp(&project_session, false);
    let created = current(&session, json!({ "changes": create_program(file) })).unwrap();
    assert_eq!(created["projects"].as_array().unwrap().len(), 1);
    assert_eq!(synthetic_projects(&project_session), 1);
    let program = created["operation"]["createdPrograms"][0].clone();
    let removed = current(
        &session,
        json!({ "changes": { "removePrograms": [program, program] } }),
    )
    .unwrap();
    assert_eq!(removed["projects"], json!([]));
    assert_eq!(synthetic_projects(&project_session), 0);
    session.close();
    project_session.close();
}

#[test]
fn an_opened_file_keeps_the_case_of_its_drive_letter() {
    // TestOpenFilePreservesWindowsDriveLetterCase.
    let file = "D:/repo/index.ts";
    let (project_session, _) = project_session_in(
        "D:/repo",
        &[
            ("D:/repo/tsconfig.json", "{}"),
            (file, "export const value = 1;"),
        ],
    );
    let session = Session::new_lsp(&project_session, false);
    let response = current(&session, json!({ "changes": { "openFiles": [file] } })).unwrap();
    let snapshot = session
        .snapshot(response["snapshot"].as_u64().unwrap())
        .unwrap();
    let project = snapshot
        .project(&ProjectId::new(
            response["projects"][0]["id"].as_str().unwrap(),
        ))
        .unwrap();
    let names = project.program().unwrap().with_live(|live| {
        (0..live.file_count())
            .filter_map(|index| live.file_name(index))
            .map(|name| name.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    });
    assert!(names.iter().any(|name| name == file), "{names:?}");
    session.close();
    project_session.close();
}

#[test]
fn closing_the_session_removes_the_programs_it_created() {
    // TestClosingAPISessionRemovesCreatedLanguageServerPrograms.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[(file, "export const value = 1;")]);
    let session = Session::new_lsp(&project_session, false);
    current(&session, json!({ "changes": create_program(file) })).unwrap();
    assert_eq!(synthetic_projects(&project_session), 1);
    session.close();
    assert_eq!(synthetic_projects(&project_session), 0);
    project_session.close();
}

#[test]
fn a_session_removes_only_the_programs_it_created() {
    // TestLanguageServerProgramOwnershipIsIsolatedByAPISession.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[(file, "export const value = 1;")]);
    let owner = Session::new_lsp(&project_session, false);
    let created = current(&owner, json!({ "changes": create_program(file) })).unwrap();
    let program = created["operation"]["createdPrograms"][0].clone();
    let other = Session::new_lsp(&project_session, false);
    current(
        &other,
        json!({ "changes": { "removePrograms": [program] } }),
    )
    .unwrap();
    assert_eq!(synthetic_projects(&project_session), 1);
    other.close();
    assert_eq!(synthetic_projects(&project_session), 1);
    owner.close();
    assert_eq!(synthetic_projects(&project_session), 0);
    project_session.close();
}

#[test]
fn a_session_reconfigures_only_the_programs_it_created() {
    // TestLanguageServerProgramReconfigurationIsIsolatedByAPISession.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[(file, "export const value = 1;")]);
    let owner = Session::new_lsp(&project_session, false);
    let created = current(&owner, json!({ "changes": create_program(file) })).unwrap();
    let program = created["projects"][0]["id"].clone();
    let other = Session::new_lsp(&project_session, false);
    let error = current(
        &other,
        json!({ "changes": { "reconfigurePrograms": [{
            "id": program,
            "rootFiles": [file],
            "compilerOptions": { "noLib": true, "strict": true },
        }] } }),
    )
    .unwrap_err();
    assert!(error.contains("not owned by this API session"), "{error}");
    other.close();
    owner.close();
    project_session.close();
}

#[test]
fn opening_a_project_another_session_holds_brings_its_program_up_to_date() {
    // TestOpeningProjectOwnedByAnotherAPISessionEnsuresProgram.
    let config = "/home/projects/p/tsconfig.json";
    let file = "/home/projects/p/index.ts";
    let (project_session, fs) =
        project_session(&[(config, "{}"), (file, "export const value = 1;")]);
    let open_project = json!({ "changes": { "openProjects": [config] } });
    let owner = Session::new_lsp(&project_session, false);
    current(&owner, open_project.clone()).unwrap();
    fs.write(file, b"export const value = 2;").unwrap();
    changed_on_disk(&project_session, file);
    current(&owner, json!({})).unwrap();
    assert!(configured_project_is_dirty(&project_session, config));

    let other = Session::new_lsp(&project_session, false);
    let opened = current(&other, open_project).unwrap();
    assert_eq!(opened["projects"][0]["dirty"], false);
    assert_eq!(open_projects(&other), 1);

    other.close();
    assert!(has_configured_project(&project_session, config));
    owner.close();
    assert!(!has_configured_project(&project_session, config));
    project_session.close();
}

#[test]
fn a_failed_open_is_not_adopted() {
    // TestFailedLanguageServerSnapshotOpenIsNotAdopted.
    let (project_session, _) = project_session(&[("/notes.txt", "text")]);
    let session = Session::new_lsp(&project_session, false);
    let base = project_session.snapshot();
    let error = current(
        &session,
        json!({ "changes": { "openFiles": ["/notes.txt"] } }),
    )
    .unwrap_err();
    assert!(
        error.contains("no project found for opened file"),
        "{error}"
    );
    assert!(Arc::ptr_eq(&project_session.snapshot(), &base));
    assert_eq!(open_files(&session), 0);
    session.close();
    project_session.close();
}

#[test]
fn opening_an_editor_file_brings_its_configured_program_up_to_date() {
    // TestGetCurrentLanguageServerSnapshotOpeningLSPFileEnsuresConfiguredProgram.
    let config = "/home/projects/p/tsconfig.json";
    let file = "/home/projects/p/index.ts";
    let (project_session, _) =
        project_session(&[(config, "{}"), (file, "export const value = 1;")]);
    open(&project_session, file, "export const value = 1;");
    let session = Session::new_lsp(&project_session, false);
    let initial = current(&session, json!({})).unwrap();
    assert_eq!(initial["projects"][0]["dirty"], false);
    let project = initial["projects"][0]["id"].clone();

    project_session.did_change_file(
        &uri(file),
        2,
        vec![
            TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "export const value = 2;".to_owned(),
                },
            ),
        ],
    );
    let dirty = current(&session, json!({ "baseSnapshot": initial["snapshot"] })).unwrap();
    assert_eq!(dirty["projects"][0]["dirty"], true);

    let ensured = current(
        &session,
        json!({ "baseSnapshot": dirty["snapshot"], "changes": { "openFiles": [file] } }),
    )
    .unwrap();
    assert_eq!(ensured["projects"][0]["dirty"], false);
    assert_eq!(ensured["operation"]["openedFiles"][0]["project"], project);
    session.close();
    project_session.close();
}

fn whole_document(text: &str) -> Vec<TextDocumentContentChangePartialOrWholeDocument> {
    vec![
        TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
            TextDocumentContentChangeWholeDocument {
                text: text.to_owned(),
            },
        ),
    ]
}

/// tsgo `updateCurrentLanguageServerSnapshot`: the language server's
/// snapshot with the request's projects, then a snapshot derived from it
/// with the request's file system and every program brought up to date.
fn update_current(session: &Session, changes: Value) -> Result<Value, String> {
    let mut request_changes = changes.clone();
    let file_system = request_changes
        .as_object_mut()
        .unwrap()
        .remove("fileSystem");
    let base = current(session, json!({ "changes": request_changes }))?;
    let mut update = request_changes;
    update["ensurePrograms"] = json!(true);
    if let Some(file_system) = file_system {
        update["fileSystem"] = file_system;
    }
    call(
        session,
        "updateSnapshot",
        json!({ "snapshot": base["snapshot"], "changes": update }),
    )
}

const ROOT_CONFIG: &str = r#"{ "compilerOptions": { "noLib": true }, "files": ["index.ts"] }"#;

#[test]
fn an_editor_change_reaches_a_request_symlink() {
    // TestEditorChangeInvalidatesRequestSymlinkAlias.
    let (project_session, _) = project_session(&[(
        "/tsconfig.json",
        r#"{ "compilerOptions": { "noLib": true }, "files": ["alias.ts"] }"#,
    )]);
    open(&project_session, "/target.ts", "old");
    let session = Session::new_lsp(&project_session, false);
    let changes = json!({
        "openProjects": ["/tsconfig.json"],
        "fileSystem": { "kind": "layer", "symlinks": { "/alias.ts": { "target": "/target.ts" } } },
    });
    let base = update_current(&session, changes.clone()).unwrap();
    let program = program_of(&session, &base["snapshot"], "/tsconfig.json");
    assert_eq!(source_text(&program, "/alias.ts").as_deref(), Some("old"));

    project_session.did_change_file(&uri("/target.ts"), 2, whole_document("new"));
    let updated = update_current(&session, changes).unwrap();
    let program = program_of(&session, &updated["snapshot"], "/tsconfig.json");
    assert_eq!(source_text(&program, "/alias.ts").as_deref(), Some("new"));
    session.close();
    project_session.close();
}

#[test]
fn a_large_layer_update_keeps_its_changes() {
    // TestLargeRequestLayerUpdateRetainsChanges: over a thousand changes
    // make the snapshot drop what it read.
    let (project_session, _) = project_session(&[("/tsconfig.json", ROOT_CONFIG)]);
    open(&project_session, "/index.ts", "old");
    let session = Session::new_lsp(&project_session, false);
    let filler = |text: &str| {
        (0..1000)
            .map(|index| (format!("/unused/file{index}.ts"), json!(text)))
            .collect::<serde_json::Map<_, _>>()
    };
    let base = update_current(
        &session,
        json!({
            "openProjects": ["/tsconfig.json"],
            "fileSystem": { "kind": "layer", "files": filler("old") },
        }),
    )
    .unwrap();
    let mut updated_files = filler("new");
    updated_files.insert("/index.ts".to_owned(), json!("new"));
    let updated = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": base["snapshot"], "changes": {
            "ensurePrograms": true,
            "fileSystem": { "kind": "layer", "files": updated_files },
        } }),
    )
    .unwrap();
    assert_eq!(
        read_snapshot(&session, &updated["snapshot"], "/index.ts").as_deref(),
        Some("new")
    );
    let program = program_of(&session, &updated["snapshot"], "/tsconfig.json");
    assert_eq!(source_text(&program, "/index.ts").as_deref(), Some("new"));
    session.close();
    project_session.close();
}

#[test]
fn an_auto_import_clone_keeps_the_request_file_system() {
    // TestAutoImportCloneRetainsRequestFileSystem.
    let (project_session, _) = project_session(&[]);
    let session = Session::new_lsp(&project_session, false);
    let base = update_current(
        &session,
        json!({ "fileSystem": { "kind": "full", "files": { "/index.ts": "request" } } }),
    )
    .unwrap();
    let handle = base["snapshot"].as_u64().unwrap();
    let clone = session
        .clone_snapshot_with_auto_imports(handle, &session.snapshot(handle).unwrap(), "/index.ts")
        .unwrap();
    assert_eq!(
        clone.read_file("/index.ts").as_deref(),
        Some(b"request".as_slice())
    );
    session.close();
    project_session.close();
}

#[test]
fn an_unmasked_document_follows_the_editor_again() {
    // TestUnmaskedOverlayContinuesUpdating.
    let (project_session, _) =
        project_session(&[("/tsconfig.json", ROOT_CONFIG), ("/index.ts", "host")]);
    open(&project_session, "/index.ts", "overlay1");
    let session = Session::new_lsp(&project_session, false);
    let masked = update_current(
        &session,
        json!({
            "openProjects": ["/tsconfig.json"],
            "fileSystem": { "kind": "layer", "files": { "/index.ts": "request" } },
        }),
    )
    .unwrap();
    let program = program_of(&session, &masked["snapshot"], "/tsconfig.json");
    assert_eq!(
        source_text(&program, "/index.ts").as_deref(),
        Some("request")
    );

    let unmasked = update_current(&session, json!({ "openProjects": ["/tsconfig.json"] })).unwrap();
    let program = program_of(&session, &unmasked["snapshot"], "/tsconfig.json");
    assert_eq!(
        source_text(&program, "/index.ts").as_deref(),
        Some("overlay1")
    );

    project_session.did_change_file(&uri("/index.ts"), 2, whole_document("overlay2"));
    let updated = update_current(&session, json!({ "openProjects": ["/tsconfig.json"] })).unwrap();
    let program = program_of(&session, &updated["snapshot"], "/tsconfig.json");
    assert_eq!(
        source_text(&program, "/index.ts").as_deref(),
        Some("overlay2")
    );
    session.close();
    project_session.close();
}

#[test]
fn a_host_mount_reads_the_editors_documents() {
    // TestRequestHostMountReadsEditorOverlays.
    for kind in ["full", "layer"] {
        let (project_session, _) = project_session(&[("/host/index.ts", "host")]);
        open(&project_session, "/host/index.ts", "overlay");
        open(&project_session, "/host/new.ts", "new overlay");
        let session = Session::new_lsp(&project_session, false);
        let base = update_current(
            &session,
            json!({ "fileSystem": {
                "kind": kind,
                "files": { "/host/index.ts": "request", "/host/new.ts": "request" },
                "symlinks": { "/mounted": { "target": "/host", "host": true } },
            } }),
        )
        .unwrap();
        assert_eq!(
            read_snapshot(&session, &base["snapshot"], "/mounted/index.ts").as_deref(),
            Some("overlay"),
            "{kind}"
        );
        assert_eq!(
            read_snapshot(&session, &base["snapshot"], "/mounted/new.ts").as_deref(),
            Some("new overlay"),
            "{kind}"
        );
        session.close();
        project_session.close();
    }
}

#[test]
fn a_request_file_overrides_an_open_document() {
    // TestUpdateSnapshotRequestFileOverridesOpenOverlay.
    let (project_session, _) = project_session(&[("/index.ts", "host")]);
    open(&project_session, "/index.ts", "overlay");
    let session = Session::new_lsp(&project_session, false);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "fileSystem": { "kind": "layer", "files": { "/index.ts": "request" } } }),
    )
    .unwrap();
    assert_eq!(
        read_snapshot(&session, &response["snapshot"], "/index.ts").as_deref(),
        Some("request")
    );
    session.close();
    project_session.close();
}

#[test]
fn a_request_tombstone_removes_an_open_document() {
    // TestUpdateSnapshotRequestTombstoneRemovesOpenOverlay and
    // TestUpdateSnapshotRequestTombstoneRemovesHostlessOpenOverlay.
    for files in [&[("/index.ts", "host")][..], &[]] {
        let (project_session, _) = project_session(files);
        open(&project_session, "/index.ts", "overlay");
        let session = Session::new_lsp(&project_session, false);
        let response = call(
            &session,
            "createSnapshot",
            json!({ "fileSystem": { "kind": "layer", "removedPaths": ["/index.ts"] } }),
        )
        .unwrap();
        assert_eq!(
            read_snapshot(&session, &response["snapshot"], "/index.ts"),
            None
        );
        let snapshot = session
            .snapshot(response["snapshot"].as_u64().unwrap())
            .unwrap();
        assert!(snapshot.default_project("/index.ts").is_none());
        session.close();
        project_session.close();
    }
}

#[test]
fn a_request_file_masks_the_directory_of_an_open_document() {
    // TestUpdateSnapshotRequestFileMasksHostlessOpenOverlayDirectory.
    let (project_session, _) = project_session(&[]);
    open(&project_session, "/src/index.ts", "overlay");
    let session = Session::new_lsp(&project_session, false);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "fileSystem": { "kind": "layer", "files": { "/src": "request" } } }),
    )
    .unwrap();
    assert_eq!(
        read_snapshot(&session, &response["snapshot"], "/src").as_deref(),
        Some("request")
    );
    assert_eq!(
        read_snapshot(&session, &response["snapshot"], "/src/index.ts"),
        None
    );
    let snapshot = session
        .snapshot(response["snapshot"].as_u64().unwrap())
        .unwrap();
    assert!(snapshot.default_project("/src/index.ts").is_none());
    session.close();
    project_session.close();
}

#[test]
fn a_request_mask_updates_the_open_configured_projects() {
    // TestUpdateSnapshotRequestMaskUpdatesOpenConfiguredProjects.
    let (project_session, _) =
        project_session(&[("/tsconfig.json", ROOT_CONFIG), ("/index.ts", "host")]);
    open(&project_session, "/index.ts", "overlay");
    let session = Session::new_lsp(&project_session, false);
    let open_project = json!({ "changes": { "openProjects": ["/tsconfig.json"] } });
    let is_open = |response: &Value| {
        session
            .snapshot(response["snapshot"].as_u64().unwrap())
            .unwrap()
            .open_configured_projects()
            .contains(&configured("/tsconfig.json"))
    };
    let base = current(&session, open_project.clone()).unwrap();
    assert!(is_open(&base));
    let masked = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": base["snapshot"], "changes": {
            "fileSystem": { "kind": "layer", "files": { "/index.ts": "request" } },
        } }),
    )
    .unwrap();
    assert!(!is_open(&masked));
    let unmasked = current(&session, open_project).unwrap();
    assert!(is_open(&unmasked));
    session.close();
    project_session.close();
}

#[test]
fn a_config_change_skips_a_masked_open_document() {
    // TestUpdateSnapshotConfigChangeSkipsMaskedOpenOverlay.
    let (project_session, _) = project_session(&[("/index.ts", "host")]);
    open(&project_session, "/index.ts", "overlay");
    let session = Session::new_lsp(&project_session, false);
    let base = call(&session, "createSnapshot", json!({})).unwrap();
    let updated = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": base["snapshot"], "changes": { "fileSystem": {
            "kind": "layer",
            "files": { "/tsconfig.json": ROOT_CONFIG },
            "removedPaths": ["/index.ts"],
        } } }),
    )
    .unwrap();
    assert_eq!(
        read_snapshot(&session, &updated["snapshot"], "/index.ts"),
        None
    );
    session.close();
    project_session.close();
}

const PACKAGE_JSON: &str = r#"{"dependencies":{"my-pkg":"1.0.0"}}"#;
const MY_PKG_PACKAGE_JSON: &str = r#"{"name":"my-pkg","version":"1.0.0","types":"index.d.ts"}"#;
const MY_PKG_INDEX: &str = "export declare const packageValue: number;";

fn completion_names(
    session: &Session,
    snapshot: &Value,
    project: &Value,
    file: &str,
    include_symbol: bool,
) -> Vec<String> {
    let completions = call(
        session,
        "getCompletionsAtPosition",
        json!({
            "snapshot": snapshot,
            "project": project,
            "file": file,
            "position": 8,
            "includeSymbol": include_symbol,
        }),
    )
    .unwrap();
    completions["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["name"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_prepared_snapshot_keeps_the_editors_documents() {
    // TestPreparedIndependentSnapshotPreservesLSPOverlays.
    let file = "/home/projects/p/index.ts";
    let (project_session, _) = project_session(&[
        ("/home/projects/p/tsconfig.json", "{}"),
        (file, "diskOnly"),
        ("/home/projects/p/package.json", PACKAGE_JSON),
        (
            "/home/projects/p/node_modules/my-pkg/package.json",
            MY_PKG_PACKAGE_JSON,
        ),
        (
            "/home/projects/p/node_modules/my-pkg/index.d.ts",
            MY_PKG_INDEX,
        ),
    ]);
    open(&project_session, file, "packageV");
    let session = Session::new_lsp(&project_session, false);
    let base = current(&session, json!({})).unwrap();
    let prepared = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": base["snapshot"], "changes": { "prepareAutoImports": file } }),
    )
    .unwrap();
    assert_eq!(
        read_snapshot(&session, &prepared["snapshot"], file).as_deref(),
        Some("packageV")
    );
    let snapshot = session
        .snapshot(prepared["snapshot"].as_u64().unwrap())
        .unwrap();
    let project = snapshot.default_project(file).unwrap();
    assert_eq!(
        source_text(project.program().unwrap(), file).as_deref(),
        Some("packageV")
    );
    let project = json!(project.id().to_string());
    let names = completion_names(&session, &prepared["snapshot"], &project, file, true);
    assert!(names.iter().any(|name| name == "packageValue"), "{names:?}");
    session.close();
    project_session.close();
}

#[test]
fn a_fresh_snapshot_has_the_dependencies_auto_imports() {
    // TestFreshSnapshotIncludesDependencyAutoImports: a created snapshot
    // reads the disk, not the editor.
    for prepare in [false, true] {
        let file = "/home/projects/MixedCase/index.ts";
        let (project_session, _) = project_session(&[
            ("/home/projects/MixedCase/tsconfig.json", "{}"),
            (file, "packageV"),
            ("/home/projects/MixedCase/package.json", PACKAGE_JSON),
            (
                "/home/projects/MixedCase/node_modules/my-pkg/package.json",
                MY_PKG_PACKAGE_JSON,
            ),
            (
                "/home/projects/MixedCase/node_modules/my-pkg/index.d.ts",
                MY_PKG_INDEX,
            ),
        ]);
        open(&project_session, file, "editorOnly");
        let session = Session::new_lsp(&project_session, false);
        let mut params = json!({ "openProjects": ["/home/projects/MixedCase/tsconfig.json"] });
        if prepare {
            params["prepareAutoImports"] = json!(file);
        }
        let response = call(&session, "createSnapshot", params).unwrap();
        assert_eq!(
            read_snapshot(&session, &response["snapshot"], file).as_deref(),
            Some("packageV")
        );
        let snapshot = session
            .snapshot(response["snapshot"].as_u64().unwrap())
            .unwrap();
        let project = json!(snapshot.default_project(file).unwrap().id().to_string());
        let names = completion_names(&session, &response["snapshot"], &project, file, prepare);
        assert!(
            names.iter().any(|name| name == "packageValue"),
            "{prepare}: {names:?}"
        );
        session.close();
        project_session.close();
    }
}
