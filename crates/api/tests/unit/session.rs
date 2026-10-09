//! tsgo's session tests on the standalone session: `session_batch_test.go`,
//! `session_createprogram_test.go` and the standalone cases of
//! `session_apistate_test.go` (the others need tsgo's language server),
//! sent as a client sends them (JSON parameters through
//! [`Session::handle`]). The snapshot updates are pinned to tsgo's
//! responses.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_compiler::DEFAULT_LOAD_LIMITS;
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_program::LibraryCatalog;

use super::*;

/// tsgo `projecttestutil`: an in-memory, case-insensitive file system with
/// the bundled libraries.
fn session_in(current_directory: &str, files: &[(&str, &str)]) -> (Session, Arc<MemFs>) {
    let fs = Arc::new(
        MemFs::from_entries(
            files.iter().map(|(name, text)| (*name, Seed::file(*text))),
            false,
            Arc::new(SystemClock),
        )
        .expect("build the file system"),
    );
    let session = Session::new(
        SessionOptions {
            current_directory: current_directory.to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1(Path::new(EMBEDDED_LIBRARY_DIRECTORY)),
            load_limits: DEFAULT_LOAD_LIMITS,
        },
        Arc::new(BundledFs::new(Arc::clone(&fs))),
        false,
    );
    (session, fs)
}

fn session(files: &[(&str, &str)]) -> (Session, Arc<MemFs>) {
    session_in("/", files)
}

/// A request's JSON text, or its error.
fn call_text(session: &Session, method: &str, params: &Value) -> Result<String, String> {
    match session.handle(method, params.to_string().as_bytes())? {
        Payload::Json(text) => Ok(text),
        Payload::Binary(_) => panic!("{method} answered in binary"),
    }
}

fn call(session: &Session, method: &str, params: Value) -> Result<Value, String> {
    call_text(session, method, &params).map(|text| serde_json::from_str(&text).unwrap())
}

fn ids(response: &Value) -> Vec<&str> {
    response["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["id"].as_str().unwrap())
        .collect()
}

#[test]
fn a_batch_answers_each_request_in_order() {
    // TestHandleBatchRequests.
    let (session, _) = session(&[]);
    let text = call_text(
        &session,
        "batchRequests",
        &json!({ "requests": [{ "method": "ping" }, { "method": "unknown" }] }),
    )
    .unwrap();
    let response: Value = serde_json::from_str(&text).unwrap();
    let responses = response["responses"].as_array().unwrap();
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0], json!({ "method": "ping", "result": "pong" }));
    assert_eq!(responses[1]["method"], "unknown");
    assert!(
        text.contains(r#""error":"api: invalid request: unknown API method \"unknown\"""#),
        "{text}"
    );
}

#[test]
fn a_panicking_request_in_a_batch_fails_alone() {
    // TestHandleBatchRequestsRecoversPerRequestPanics.
    let (session, _) = session(&[]);
    let response = call(
        &session,
        "batchRequests",
        json!({ "requests": [{ "method": "ping" }, { "method": "panicForTest" }, { "method": "ping" }] }),
    )
    .unwrap();
    assert_eq!(response["responses"][0]["result"], "pong");
    assert!(response["responses"][1]["error"]
        .as_str()
        .unwrap()
        .starts_with("panic:"));
    assert_eq!(response["responses"][2]["result"], "pong");
}

#[test]
fn an_empty_result_is_written_as_is() {
    // TestBatchResponseEncodesEmptyResult, through `echo`, whose result is
    // its parameters.
    let (session, _) = session(&[]);
    assert_eq!(
        call_text(
            &session,
            "batchRequests",
            &json!({ "requests": [{ "method": "echo", "params": [] }] }),
        )
        .unwrap(),
        r#"{"responses":[{"method":"echo","result":[]}]}"#
    );
}

#[test]
fn a_batch_is_paged_by_size() {
    // TestHandleBatchRequestsPaginatesResponses.
    let (session, _) = session(&[]);
    let max = 150;
    let mut text = call_text(
        &session,
        "batchRequests",
        &json!({ "requests": vec![json!({ "method": "ping" }); 10], "maxResponseBytesPerPage": max }),
    )
    .unwrap();
    let mut responses = Vec::new();
    loop {
        assert!(text.len() <= max, "{text}");
        let page: Value = serde_json::from_str(&text).unwrap();
        responses.extend(page["responses"].as_array().unwrap().iter().cloned());
        let Some(token) = page["continuationToken"].as_str() else {
            break;
        };
        text = call_text(
            &session,
            "batchRequests",
            &json!({ "continuationToken": token, "maxResponseBytesPerPage": max }),
        )
        .unwrap();
    }
    assert_eq!(responses.len(), 10);
    for response in responses {
        assert_eq!(response, json!({ "method": "ping", "result": "pong" }));
    }
}

#[test]
fn a_single_oversized_response_makes_a_page() {
    // TestHandleBatchRequestsAllowsOversizedSingleResponse.
    let (session, _) = session(&[]);
    let response = call(
        &session,
        "batchRequests",
        json!({ "requests": [{ "method": "ping" }], "maxResponseBytesPerPage": 1 }),
    )
    .unwrap();
    assert_eq!(response["responses"].as_array().unwrap().len(), 1);
    assert_eq!(response.get("continuationToken"), None);
}

#[test]
fn the_page_limit_holds_for_its_request_only() {
    // TestHandleBatchRequestsPageLimitIsRequestScoped.
    let (session, _) = session(&[]);
    let requests = json!([{ "method": "ping" }, { "method": "ping" }]);
    let limited = call(
        &session,
        "batchRequests",
        json!({ "requests": requests, "maxResponseBytesPerPage": 1 }),
    )
    .unwrap();
    assert_eq!(limited["responses"].as_array().unwrap().len(), 1);
    assert!(limited["continuationToken"].as_str().is_some());
    let unlimited = call(&session, "batchRequests", json!({ "requests": requests })).unwrap();
    assert_eq!(unlimited["responses"].as_array().unwrap().len(), 2);
    assert_eq!(unlimited.get("continuationToken"), None);
}

#[test]
fn an_unknown_continuation_token_is_rejected() {
    // TestHandleBatchRequestsRejectsInvalidContinuationToken.
    let (session, _) = session(&[]);
    let error = call(
        &session,
        "batchRequests",
        json!({ "continuationToken": "invalid" }),
    )
    .unwrap_err();
    assert!(
        error.contains("invalid batch continuation token"),
        "{error}"
    );
}

#[test]
fn a_batch_cannot_hold_a_batch() {
    // TestHandleBatchRequestsRejectsNestedBatch.
    let (session, _) = session(&[]);
    let response = call(
        &session,
        "batchRequests",
        json!({ "requests": [{ "method": "batchRequests", "params": { "requests": [] } }] }),
    )
    .unwrap();
    assert_eq!(response["responses"].as_array().unwrap().len(), 1);
    assert!(response["responses"][0]["error"]
        .as_str()
        .unwrap()
        .contains("batchRequests cannot be nested"));
}

#[test]
fn each_created_snapshot_starts_from_nothing() {
    // TestCreateSnapshotUsesIndependentRoots.
    let file = "/home/projects/p/src/index.ts";
    let (session, _) = session(&[(file, "export const x = 1;")]);
    let first = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{ "rootFiles": [file], "compilerOptions": { "noLib": true } }] }),
    )
    .unwrap();
    assert_eq!(first["snapshot"], 1);
    assert_eq!(ids(&first).len(), 1);
    let second = call(&session, "createSnapshot", json!({})).unwrap();
    assert_eq!(second["snapshot"], 2);
    assert!(ids(&second).is_empty());
}

#[test]
fn created_programs_are_reported_in_order() {
    // TestCreateSnapshotCreatesPrograms.
    let a = "/home/projects/p/a.ts";
    let b = "/home/projects/p/b.ts";
    let (session, _) = session(&[(a, "export const a = 1;"), (b, "export const b = 1;")]);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [
            { "rootFiles": [a, b], "compilerOptions": { "noLib": true, "strict": true } },
            { "rootFiles": [b], "compilerOptions": { "noLib": true } },
        ] }),
    )
    .unwrap();
    assert_eq!(
        ids(&response),
        ["/dev/null/synthetic/1", "/dev/null/synthetic/2"]
    );
    assert_eq!(
        response["operation"]["createdPrograms"],
        json!(["/dev/null/synthetic/1", "/dev/null/synthetic/2"])
    );
    let projects = &response["projects"];
    assert_eq!(projects[0]["configFileName"], "");
    assert_eq!(projects[1]["configFileName"], "");
    assert_eq!(projects[0]["rootFiles"], json!([a, b]));
    assert_eq!(projects[0]["compilerOptions"]["strict"], true);
    assert_eq!(projects[1]["rootFiles"], json!([b]));
    let snapshot = session.snapshot(1).unwrap();
    assert_eq!(snapshot.created_programs().len(), 2);
    for id in ids(&response) {
        assert!(snapshot.project(&ProjectId::new(id)).is_some(), "{id}");
    }
}

#[test]
fn a_windows_root_keeps_its_drive_letter_case() {
    // TestCreateSnapshotPreservesWindowsRootDriveLetterCase.
    let file = "D:/repo/index.ts";
    let (session, _) = session_in("D:/repo", &[(file, "export const value = 1;")]);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{ "rootFiles": [{ "uri": "file:///D%3A/repo/index.ts" }], "compilerOptions": { "noLib": true } }] }),
    )
    .unwrap();
    let snapshot = session
        .snapshot(response["snapshot"].as_u64().unwrap())
        .unwrap();
    let id = ProjectId::new(response["projects"][0]["id"].as_str().unwrap());
    let program = snapshot.project(&id).unwrap().program().unwrap();
    let names = program.with_live(|live| {
        (0..live.file_count())
            .filter_map(|index| Some(live.file_name(index)?.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
    });
    assert!(names.iter().any(|name| name == file), "{names:?}");
}

#[test]
fn an_operation_holds_only_what_was_asked() {
    // TestSnapshotOperationResponseOmitsUnrequestedFields.
    let (session, _) = session(&[]);
    let response = call(&session, "createSnapshot", json!({})).unwrap();
    assert_eq!(response["operation"], json!({}));
    let text = call_text(
        &session,
        "createSnapshot",
        &json!({ "createPrograms": [], "reconfigurePrograms": [], "openFiles": [] }),
    )
    .unwrap();
    assert!(
        text.ends_with(r#""operation":{"createdPrograms":[],"openedFiles":[]}}"#),
        "{text}"
    );
}

#[test]
fn a_synthetic_program_is_configured_again() {
    // TestUpdateSnapshotReconfiguresSyntheticProgram.
    let (session, _) = session(&[
        ("/home/projects/p/a.ts", "export const a = 1;"),
        ("/home/projects/p/b.ts", "export const b = 2;"),
    ]);
    let created = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{ "rootFiles": ["/home/projects/p/a.ts"], "compilerOptions": { "noLib": true } }] }),
    )
    .unwrap();
    let program = created["operation"]["createdPrograms"][0].clone();
    let reconfigured = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": created["snapshot"], "changes": { "reconfigurePrograms": [{
            "id": program, "rootFiles": ["/home/projects/p/b.ts"], "compilerOptions": { "noLib": true, "strict": true },
        }] } }),
    )
    .unwrap();
    assert_eq!(reconfigured["projects"][0]["id"], program);
    assert_eq!(
        reconfigured["projects"][0]["rootFiles"],
        json!(["/home/projects/p/b.ts"])
    );
    assert_eq!(
        reconfigured["projects"][0]["compilerOptions"]["strict"],
        true
    );
}

#[test]
fn reconfiguring_a_synthetic_program_is_validated() {
    // TestReconfigureSyntheticProgramValidation, through the wire: an ID
    // that is not a synthetic program's fails to decode, as in tsgo.
    let (session, _) = session(&[]);
    let program = json!({ "id": "/dev/null/synthetic/1" });
    for (params, expected) in [
        (
            json!({ "reconfigurePrograms": [null] }),
            "reconfigurePrograms[0] must not be null",
        ),
        (
            json!({ "reconfigurePrograms": [{ "id": "/tsconfig.json" }] }),
            "invalid synthetic project ID: /tsconfig.json",
        ),
        (
            json!({ "reconfigurePrograms": [program, program] }),
            "reconfigured more than once",
        ),
        (
            json!({ "reconfigurePrograms": [program], "removePrograms": ["/dev/null/synthetic/1"] }),
            "cannot be reconfigured and removed",
        ),
        (
            json!({ "createPrograms": [{}], "reconfigurePrograms": [program] }),
            "not found for reconfiguration",
        ),
    ] {
        let error = call(&session, "createSnapshot", params).unwrap_err();
        assert!(error.contains(expected), "{error}");
    }
}

#[test]
fn a_null_created_program_is_a_client_error() {
    // TestCreateSyntheticProgramValidation.
    let (session, _) = session(&[]);
    let error = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [null] }),
    )
    .unwrap_err();
    assert_eq!(
        error,
        "api: client error: createPrograms[0] must not be null"
    );
}

#[test]
fn a_new_snapshot_has_no_program_to_remove() {
    // TestCreateSnapshotRejectsRemovingProgramFromIndependentRoot.
    let (session, _) = session(&[]);
    let error = call(
        &session,
        "createSnapshot",
        json!({ "removePrograms": ["/dev/null/synthetic/1"] }),
    )
    .unwrap_err();
    assert!(
        error.contains("synthetic program not found for removal: /dev/null/synthetic/1"),
        "{error}"
    );
}

#[test]
fn a_dirty_synthetic_program_is_ensured() {
    // TestUpdateSnapshotEnsuresSyntheticProgram.
    let file = "/home/projects/p/index.ts";
    let (session, fs) = session(&[(file, "export const value = 1;")]);
    let created = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{ "rootFiles": [file], "compilerOptions": { "noLib": true } }] }),
    )
    .unwrap();
    assert_eq!(created["projects"][0]["dirty"], false);
    let project = created["projects"][0]["id"].clone();
    fs.write(file, b"export const value = 2;").unwrap();
    let dirty = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": created["snapshot"], "changes": { "fileNotifications": { "changed": [file] } } }),
    )
    .unwrap();
    assert_eq!(dirty["projects"][0]["dirty"], true);
    let ensured = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": dirty["snapshot"], "changes": { "ensurePrograms": [project] } }),
    )
    .unwrap();
    assert_eq!(ensured["projects"][0]["dirty"], false);
}

#[test]
fn a_standalone_session_has_no_language_server_snapshot() {
    // TestGetCurrentLanguageServerSnapshotRejectsStandaloneSession.
    let (session, _) = session(&[]);
    let error = call(&session, "getCurrentLanguageServerSnapshot", json!({})).unwrap_err();
    assert!(
        error.contains("requires an LSP-connected API session"),
        "{error}"
    );
}

#[test]
fn the_inferred_project_cannot_be_opened_as_a_config() {
    // TestOpenProjectRejectsReservedProjectID.
    let (session, _) = session(&[]);
    let error = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/dev/null/inferred"] }),
    )
    .unwrap_err();
    assert!(error.contains("invalid configured project ID"), "{error}");
}

#[test]
fn snapshot_updates_report_their_project_and_file_changes() {
    // tsgo's responses: a changed file makes its project dirty without a
    // new program, ensuring the project builds one that reports the file
    // changed, and a closed project is removed.
    let (session, fs) = session(&[
        ("/p/tsconfig.json", "{}"),
        ("/p/a.ts", "import { b } from './b';\nexport const a = b;\n"),
        ("/p/b.ts", "export const b = 1;\n"),
    ]);
    let opened = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/p/tsconfig.json"] }),
    )
    .unwrap();
    assert_eq!(ids(&opened), ["/p/tsconfig.json"]);
    fs.write("/p/b.ts", b"export const b = 2;\n").unwrap();
    let dirty = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": opened["snapshot"], "changes": { "fileNotifications": { "changed": ["/p/b.ts"] } } }),
    )
    .unwrap();
    assert_eq!(ids(&dirty), ["/p/tsconfig.json"]);
    assert_eq!(dirty["projects"][0]["dirty"], true);
    assert_eq!(dirty.get("changes"), None);
    let ensured = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": dirty["snapshot"], "changes": { "ensurePrograms": true } }),
    )
    .unwrap();
    assert_eq!(ensured["projects"][0]["dirty"], false);
    assert_eq!(
        ensured["changes"],
        json!({ "changedProjects": { "/p/tsconfig.json": { "changedFiles": ["/p/b.ts"] } } })
    );
    let closed = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": ensured["snapshot"], "changes": { "closeProjects": ["/p/tsconfig.json"] } }),
    )
    .unwrap();
    assert!(ids(&closed).is_empty());
    assert_eq!(
        closed["changes"],
        json!({ "removedProjects": ["/p/tsconfig.json"] })
    );
}

#[test]
fn a_configured_project_reports_its_parsed_config() {
    // tsgo `NewProjectResponse`: the config's options with its path, its
    // raw JSON, `compileOnSave` false unless set, an empty type
    // acquisition left out, and each config error located in the file.
    let config =
        "{\n  \"compilerOptions\": {\n    \"target\": \"nope\",\n    \"strict\": true\n  }\n}\n";
    let (session, _) = session(&[("/p/tsconfig.json", config), ("/p/a.ts", "")]);
    let response = call(
        &session,
        "createSnapshot",
        json!({ "openProjects": ["/p/tsconfig.json"] }),
    )
    .unwrap();
    let project = &response["projects"][0];
    let command_line = &project["parsedCommandLine"];
    assert_eq!(project["configFileName"], "/p/tsconfig.json");
    assert_eq!(project["currentDirectory"], "/p");
    assert_eq!(command_line["fileNames"], json!(["/p/a.ts"]));
    assert_eq!(
        command_line["options"],
        json!({ "strict": true, "configFilePath": "/p/tsconfig.json" })
    );
    assert_eq!(command_line["options"], project["compilerOptions"]);
    assert_eq!(command_line["compileOnSave"], false);
    assert_eq!(command_line.get("typeAcquisition"), None);
    assert_eq!(
        command_line["raw"],
        json!({ "compilerOptions": { "target": "nope", "strict": true } })
    );
    let error = &command_line["errors"][0];
    assert_eq!(error["code"], 6046);
    assert_eq!(
        error["startPosition"],
        json!({ "line": 2, "character": 14 })
    );
    assert_eq!(
        error["sourceLines"],
        json!([{ "line": 2, "text": "    \"target\": \"nope\",\n" }])
    );
}

#[test]
fn a_released_snapshot_is_gone() {
    let (session, _) = session(&[]);
    for params in [json!({ "snapshot": 0 }), Value::Null] {
        assert_eq!(
            call(&session, "release", params).unwrap_err(),
            "api: client error: empty handle"
        );
    }
    let created = call(&session, "createSnapshot", json!({})).unwrap();
    let handle = created["snapshot"].clone();
    assert_eq!(
        call(&session, "release", json!({ "snapshot": handle })).unwrap(),
        true
    );
    for (method, params) in [
        ("release", json!({ "snapshot": handle })),
        (
            "getDefaultProjectForFile",
            json!({ "snapshot": handle, "file": "/a.ts" }),
        ),
        ("updateSnapshot", json!({ "snapshot": handle })),
    ] {
        assert_eq!(
            call(&session, method, params).unwrap_err(),
            format!("api: client error: snapshot {handle} not found")
        );
    }
}

#[test]
fn a_file_no_project_has_has_no_default_project() {
    let (session, _) = session(&[("/a.ts", "")]);
    call(&session, "createSnapshot", json!({})).unwrap();
    assert_eq!(
        call(
            &session,
            "getDefaultProjectForFile",
            json!({ "snapshot": 1, "file": "/a.ts" }),
        )
        .unwrap(),
        Value::Null
    );
}

#[test]
fn initialize_reports_the_file_system_and_directory() {
    let (session, _) = session_in("/work", &[]);
    assert_eq!(
        call(&session, "initialize", Value::Null).unwrap(),
        json!({ "useCaseSensitiveFileNames": false, "currentDirectory": "/work" })
    );
}

#[test]
fn echo_answers_its_parameters() {
    let (session, _) = session(&[]);
    // Compacted as Go writes a JSON value.
    assert_eq!(
        session.handle("echo", br#"{ "a" : [1, "b c"] }"#),
        Ok(Payload::Json(r#"{"a":[1,"b c"]}"#.to_owned()))
    );
    let binary = Session::new(
        session.host.options().clone(),
        Arc::new(MemFs::new(false, Arc::new(SystemClock))),
        true,
    );
    assert_eq!(
        binary.handle("echo", b"raw bytes"),
        Ok(Payload::Binary(b"raw bytes".to_vec()))
    );
}

#[test]
fn a_method_tsgo_has_is_not_implemented_yet_and_another_is_unknown() {
    let (session, _) = session(&[]);
    assert_eq!(
        call(&session, "getSourceFile", json!({})).unwrap_err(),
        "api: invalid request: getSourceFile is not implemented yet"
    );
    assert_eq!(
        call(&session, "nope", json!({})).unwrap_err(),
        "api: invalid request: unknown API method \"nope\""
    );
    assert!(TSGO_METHODS.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn a_request_file_system_is_not_supported_yet() {
    // tsgo's request file system (`requestfilesystem`) comes with P5-2b.
    let (session, _) = session(&[]);
    for (method, params) in [
        (
            "createSnapshot",
            json!({ "fileSystem": { "kind": "full", "files": {} } }),
        ),
        (
            "updateSnapshot",
            json!({ "snapshot": 1, "changes": { "fileSystem": { "kind": "layer" } } }),
        ),
    ] {
        if method == "updateSnapshot" {
            call(&session, "createSnapshot", json!({})).unwrap();
        }
        assert_eq!(
            call(&session, method, params).unwrap_err(),
            "api: client error: a request's file system is not supported yet"
        );
    }
}
