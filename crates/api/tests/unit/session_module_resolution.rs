//! tsgo `session_module_resolution_test.go` (the standalone cases) and the
//! TypeScript client's module resolver tests: a client's module resolvers,
//! their static resolutions and callbacks, for `resolveModuleName` and for
//! the programs of a snapshot.

use std::collections::VecDeque;
use std::sync::Mutex;

use super::*;
use crate::ipc::{Conn, Error, Id, Message, Mode, Protocol, ResponseError};

fn resolve(session: &Session, params: Value) -> Result<Value, String> {
    call(session, "resolveModuleName", params)
}

fn resolver(session: &Session, params: Value) -> Value {
    call(session, "createModuleResolver", params).unwrap()
}

#[test]
fn a_resolver_reads_the_snapshots_files() {
    // TestModuleResolverUsesSnapshotFileSystem.
    let (session, _) = session(&[
        (
            "/home/projects/p/node_modules/pkg/package.json",
            r#"{"name":"pkg","version":"1.0.0","exports":{".":{"types":"./index.d.ts","default":"./index.js"}}}"#,
        ),
        (
            "/home/projects/p/node_modules/pkg/index.d.ts",
            "export declare const value: string;",
        ),
        (
            "/home/projects/p/node_modules/pkg/index.js",
            r#"exports.value = "value";"#,
        ),
    ]);
    let snapshot = call(&session, "createSnapshot", json!({})).unwrap()["snapshot"].clone();
    let id = resolver(
        &session,
        json!({ "compilerOptions": { "module": 199, "moduleResolution": 99, "traceResolution": true } }),
    );
    let result = resolve(
        &session,
        json!({ "snapshot": snapshot, "resolver": id, "moduleName": "pkg", "containingDirectory": "/home/projects/p/src" }),
    )
    .unwrap();
    assert_eq!(
        result["resolvedModule"]["resolvedFileName"],
        "/home/projects/p/node_modules/pkg/index.d.ts"
    );
    assert_eq!(result["resolvedModule"]["packageId"]["name"], "pkg");
    assert_eq!(
        result["trace"][0],
        "======== Resolving module 'pkg' from '/home/projects/p/src'. ========"
    );
}

#[test]
fn static_resolutions_take_the_most_specific_entry() {
    // TestStaticModuleResolutionSpecificityAndLifetime.
    let (session, _) = session(&[]);
    let entry = |directory: Option<&str>, mode: Option<u32>, file: &str| {
        let mut entry = json!({ "moduleName": "pkg", "result": { "resolvedFileName": file } });
        if let Some(directory) = directory {
            entry["containingDirectory"] = json!(directory);
        }
        if let Some(mode) = mode {
            entry["resolutionMode"] = json!(mode);
        }
        entry
    };
    let id = resolver(
        &session,
        json!({
            "compilerOptions": { "moduleResolution": 99 },
            "moduleResolutions": { "fallback": "unresolved", "entries": [
                entry(None, None, "/home/projects/p/global.d.ts"),
                entry(None, Some(99), "/home/projects/p/mode.d.ts"),
                entry(Some("/home/projects/p/src"), None, "/home/projects/p/dir.d.ts"),
                entry(Some("/home/projects/p/src"), Some(99), "/home/projects/p/exact.d.ts"),
            ] },
        }),
    );
    for (directory, mode, expected) in [
        ("/home/projects/p/src", 99, "/home/projects/p/exact.d.ts"),
        ("/home/projects/p/src", 1, "/home/projects/p/dir.d.ts"),
        ("/home/projects/p/other", 99, "/home/projects/p/mode.d.ts"),
        ("/home/projects/p/other", 1, "/home/projects/p/global.d.ts"),
    ] {
        assert_eq!(
            resolve(
                &session,
                json!({ "resolver": id, "moduleName": "pkg", "containingDirectory": directory, "resolutionMode": mode }),
            )
            .unwrap(),
            json!({ "resolvedModule": {
                "resolvedFileName": expected,
                "extension": ".d.ts",
                "resolvedUsingTsExtension": false,
                "resolvedUsingExtraExtensions": false,
                "isExternalLibraryImport": false,
            } })
        );
    }
    // No entry and no fallback: unresolved, without a trace.
    assert_eq!(
        resolve(
            &session,
            json!({ "resolver": id, "moduleName": "other", "containingDirectory": "/home/projects/p/src" }),
        )
        .unwrap(),
        json!({})
    );
}

#[test]
fn a_static_resolution_keeps_its_identity() {
    // TestStaticModuleResolutionPreservesStaticIdentity.
    let (session, _) = session(&[]);
    let id = resolver(
        &session,
        json!({
            "compilerOptions": { "moduleResolution": 99 },
            "moduleResolutions": { "fallback": "unresolved", "entries": [{
                "moduleName": "pkg",
                "result": {
                    "resolvedFileName": "/store/pkg/index.d.ts",
                    "originalPath": "/node_modules/pkg/index.d.ts",
                    "packageId": { "name": "pkg", "subModuleName": "", "version": "1.2.3" },
                },
            }] },
        }),
    );
    let module = resolve(
        &session,
        json!({ "resolver": id, "moduleName": "pkg", "containingDirectory": "/src" }),
    )
    .unwrap()["resolvedModule"]
        .clone();
    assert_eq!(module["originalPath"], "/node_modules/pkg/index.d.ts");
    assert_eq!(module["packageId"]["name"], "pkg");
    assert_eq!(module["packageId"]["version"], "1.2.3");
    assert_eq!(module["isExternalLibraryImport"], true);
}

const PROGRAM_OPTIONS: &str = r#"{ "noLib": true, "module": 199, "moduleResolution": 99 }"#;

fn program_options() -> Value {
    serde_json::from_str(PROGRAM_OPTIONS).unwrap()
}

#[test]
fn a_program_resolves_through_static_resolutions() {
    // TestCreateProgramUsesStaticModuleResolutions.
    let (session, _) = session(&[
        (
            "/home/projects/p/src/index.ts",
            r#"import { value } from "pkg"; export { value };"#,
        ),
        (
            "/home/projects/p/provided.d.ts",
            "export declare const value: string;",
        ),
    ]);
    let id = resolver(
        &session,
        json!({
            "compilerOptions": program_options(),
            "moduleResolutions": { "fallback": "unresolved", "entries": [
                { "moduleName": "pkg", "result": { "resolvedFileName": "/home/projects/p/provided.d.ts" } },
            ] },
        }),
    );
    let created = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{
            "rootFiles": ["/home/projects/p/src/index.ts"],
            "compilerOptions": program_options(),
            "options": { "moduleResolver": id },
        }] }),
    )
    .unwrap();
    let program = created["operation"]["createdPrograms"][0].clone();
    assert_eq!(
        call(
            &session,
            "getSourceFileNames",
            json!({ "snapshot": created["snapshot"], "project": program }),
        )
        .unwrap(),
        json!([
            "/home/projects/p/provided.d.ts",
            "/home/projects/p/src/index.ts"
        ])
    );
    // The same resolver keeps the program; another builds it again.
    let same = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": created["snapshot"], "changes": { "reconfigurePrograms": [{
            "id": program,
            "rootFiles": ["/home/projects/p/src/index.ts"],
            "compilerOptions": program_options(),
            "options": { "moduleResolver": id },
        }] } }),
    )
    .unwrap();
    let id_of = |response: &Value| response["snapshot"].clone();
    let program_id = program.as_str().unwrap();
    assert!(Arc::ptr_eq(
        &program_of(&session, &id_of(&same), program_id),
        &program_of(&session, &id_of(&created), program_id),
    ));
    let without = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": created["snapshot"], "changes": { "reconfigurePrograms": [{
            "id": program,
            "rootFiles": ["/home/projects/p/src/index.ts"],
            "compilerOptions": program_options(),
        }] } }),
    )
    .unwrap();
    assert_eq!(
        call(
            &session,
            "getSourceFileNames",
            json!({ "snapshot": without["snapshot"], "project": program }),
        )
        .unwrap(),
        json!(["/home/projects/p/src/index.ts"])
    );
}

#[test]
fn a_program_resolves_with_the_resolvers_options() {
    // The client's `program module resolution uses the resolver compiler
    // options`: bundler with a custom condition, not the program's node16.
    let (session, _) = session(&[
        (
            "/src/index.ts",
            "/// <reference types=\"resolver-types\" />\nimport \"pkg/feature\";",
        ),
        (
            "/node_modules/pkg/package.json",
            r#"{ "name": "pkg", "version": "1.0.0", "exports": { "./feature": { "resolver": "./dist/feature.d.ts" } } }"#,
        ),
        ("/node_modules/pkg/dist/feature.d.ts", "export {};"),
        (
            "/node_modules/@types/resolver-types/index.d.ts",
            "export {};",
        ),
    ]);
    let id = resolver(
        &session,
        json!({ "compilerOptions": { "module": 99, "moduleResolution": 100, "customConditions": ["resolver"] } }),
    );
    let created = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{
            "rootFiles": ["/src/index.ts"],
            "compilerOptions": { "noLib": true, "module": 100, "moduleResolution": 3 },
            "options": { "moduleResolver": id },
        }] }),
    )
    .unwrap();
    let mut names = call(
        &session,
        "getSourceFileNames",
        json!({ "snapshot": created["snapshot"], "project": created["operation"]["createdPrograms"][0] }),
    )
    .unwrap()
    .as_array()
    .unwrap()
    .clone();
    names.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    assert_eq!(
        names,
        [
            "/node_modules/@types/resolver-types/index.d.ts",
            "/node_modules/pkg/dist/feature.d.ts",
            "/src/index.ts"
        ]
    );
}

#[test]
fn a_snapshot_keeps_what_its_resolutions_read() {
    // The client's `module resolver runs against snapshots or the host
    // filesystem`.
    let package_json = "/node_modules/pkg/package.json";
    let (session, fs) = session(&[
        (
            package_json,
            r#"{ "name": "pkg", "version": "1.0.0", "types": "a.d.ts" }"#,
        ),
        (
            "/node_modules/pkg/a.d.ts",
            r#"export declare const value: "a";"#,
        ),
    ]);
    let id = resolver(
        &session,
        json!({ "compilerOptions": { "module": 199, "moduleResolution": 99 } }),
    );
    let first = call(&session, "createSnapshot", json!({})).unwrap()["snapshot"].clone();
    let resolved = |snapshot: Option<&Value>| {
        let mut params =
            json!({ "resolver": id, "moduleName": "pkg", "containingDirectory": "/src" });
        if let Some(snapshot) = snapshot {
            params["snapshot"] = snapshot.clone();
        }
        resolve(&session, params).unwrap()["resolvedModule"]["resolvedFileName"].clone()
    };
    assert_eq!(resolved(Some(&first)), "/node_modules/pkg/a.d.ts");
    fs.write(
        package_json,
        br#"{ "name": "pkg", "version": "1.0.0", "types": "b.d.ts" }"#,
    )
    .unwrap();
    fs.write(
        "/node_modules/pkg/b.d.ts",
        br#"export declare const value: "b";"#,
    )
    .unwrap();
    let second = call(
        &session,
        "updateSnapshot",
        json!({ "snapshot": first, "changes": { "fileNotifications": {
            "changed": [package_json],
            "created": ["/node_modules/pkg/b.d.ts"],
        } } }),
    )
    .unwrap()["snapshot"]
        .clone();
    assert_eq!(resolved(Some(&first)), "/node_modules/pkg/a.d.ts");
    assert_eq!(resolved(Some(&second)), "/node_modules/pkg/b.d.ts");
    assert_eq!(resolved(None), "/node_modules/pkg/b.d.ts");
}

/// A client whose answers to the session's calls are queued.
struct ScriptedClient {
    answers: VecDeque<Message>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

impl Protocol for ScriptedClient {
    fn read_message(&mut self) -> Result<Option<Message>, Error> {
        Ok(self.answers.pop_front())
    }

    fn write_request(&mut self, _id: &Id, method: &str, params: &str) -> Result<(), Error> {
        self.calls
            .lock()
            .unwrap()
            .push((method.to_owned(), params.to_owned()));
        Ok(())
    }

    fn write_notification(&mut self, _method: &str, _params: &str) -> Result<(), Error> {
        Ok(())
    }

    fn write_response(&mut self, _id: &Id, _result: &Payload) -> Result<(), Error> {
        Ok(())
    }

    fn write_error(&mut self, _id: &Id, _error: &ResponseError) -> Result<(), Error> {
        Ok(())
    }
}

/// The calls a client received: method and parameters.
type Calls = Arc<Mutex<Vec<(String, String)>>>;

/// A session whose client answers its calls with `answers`.
fn session_with_client(
    files: &[(&str, &str)],
    answers: Vec<Message>,
) -> (Arc<Session>, Arc<Conn>, Calls) {
    let (session, _) = session(files);
    let session = Arc::new(session);
    let calls = Arc::default();
    let conn = Arc::new(Conn::new(
        Box::new(ScriptedClient {
            answers: answers.into(),
            calls: Arc::clone(&calls),
        }),
        Arc::clone(&session) as Arc<dyn Handler>,
        Mode::Sync,
    ));
    session.set_connection(&conn);
    (session, conn, calls)
}

fn answer(method: &str, result: &str) -> Message {
    Message {
        id: Some(Id::String(method.to_owned())),
        result: result.as_bytes().to_vec(),
        ..Message::default()
    }
}

fn failure(method: &str, message: &str) -> Message {
    Message {
        id: Some(Id::String(method.to_owned())),
        error: Some(ResponseError {
            code: -32603,
            message: message.to_owned(),
        }),
        ..Message::default()
    }
}

#[test]
fn a_callback_answers_or_fails_the_resolution() {
    // TestModuleResolutionCallbackErrorsAreReturned.
    let (session, _conn, calls) = session_with_client(
        &[
            ("/src/index.ts", r#"import "custom";"#),
            ("/custom.d.ts", "export {};"),
        ],
        vec![
            answer(
                "resolveModuleName/1",
                r#"{"resolvedFileName":"/custom.d.ts"}"#,
            ),
            answer("resolveModuleName/1", "null"),
            failure("resolveModuleName/1", "callback error"),
            answer(
                "resolveModuleName/1",
                r#"{"resolvedFileName":"/custom.d.ts"}"#,
            ),
            failure("resolveModuleName/1", "callback error"),
        ],
    );
    let id = resolver(
        &session,
        json!({ "compilerOptions": program_options(), "resolveModuleNameCallback": "resolveModuleName/1" }),
    );
    let resolve_custom = || {
        resolve(
            &session,
            json!({ "resolver": id, "moduleName": "custom", "containingDirectory": "/src", "resolutionMode": 99 }),
        )
    };
    assert_eq!(
        resolve_custom().unwrap()["resolvedModule"]["resolvedFileName"],
        "/custom.d.ts"
    );
    assert_eq!(resolve_custom().unwrap(), json!({}));
    assert_eq!(
        resolve_custom().unwrap_err(),
        r#"resolveModuleName callback failed: ipc: remote error [-32603]: callback error"#
    );
    // A program's resolutions call the client in the snapshot being built.
    let created = call(
        &session,
        "createSnapshot",
        json!({ "createPrograms": [{
            "rootFiles": ["/src/index.ts"],
            "compilerOptions": program_options(),
            "options": { "moduleResolver": id },
        }] }),
    )
    .unwrap();
    assert_eq!(
        call(
            &session,
            "getSourceFileNames",
            json!({ "snapshot": created["snapshot"], "project": created["operation"]["createdPrograms"][0] }),
        )
        .unwrap(),
        json!(["/custom.d.ts", "/src/index.ts"])
    );
    let params = calls.lock().unwrap()[3].1.clone();
    assert_eq!(
        serde_json::from_str::<Value>(&params).unwrap(),
        json!({ "moduleName": "custom", "containingDirectory": "/src", "resolutionMode": 1, "inProgressSnapshot": 1 })
    );
    // A failed callback fails the snapshot it builds.
    assert_eq!(
        call(
            &session,
            "createSnapshot",
            json!({ "createPrograms": [{
                "rootFiles": ["/src/index.ts"],
                "compilerOptions": program_options(),
                "options": { "moduleResolver": id },
            }] }),
        )
        .unwrap_err(),
        r#"api: client error: failed to create snapshot: resolveModuleName callback failed: ipc: remote error [-32603]: callback error"#
    );
    assert_eq!(calls.lock().unwrap().len(), 5);
}

#[test]
fn a_callback_needs_the_client() {
    let (session, _) = session(&[]);
    let id = resolver(
        &session,
        json!({ "compilerOptions": {}, "resolveModuleNameCallback": "resolveModuleName/1" }),
    );
    assert_eq!(
        call(
            &session,
            "createSnapshot",
            json!({ "createPrograms": [{ "rootFiles": ["/a.ts"], "options": { "moduleResolver": id } }] }),
        )
        .unwrap_err(),
        "api: client error: API connection is not initialized"
    );
    assert_eq!(
        resolve(
            &session,
            json!({ "resolver": id, "moduleName": "x", "containingDirectory": "/" }),
        )
        .unwrap_err(),
        "API connection is not initialized"
    );
}

#[test]
fn resolvers_answer_tsgos_client_errors() {
    let (session, _) = session(&[]);
    for (spec, error) in [
        (
            json!({ "fallback": "maybe", "entries": [] }),
            r#"invalid module resolution fallback "maybe""#,
        ),
        (
            json!({ "fallback": "resolve", "entries": [null] }),
            "module resolution entry 0 is null",
        ),
        (
            json!({ "fallback": "resolve", "entries": [{ "moduleName": "", "result": {} }] }),
            "module resolution entry 0 has an empty moduleName",
        ),
        (
            json!({ "fallback": "resolve", "entries": [{ "moduleName": "x" }] }),
            "module resolution entry 0 has no result",
        ),
        (
            json!({ "fallback": "resolve", "entries": [{ "moduleName": "x", "resolutionMode": 7, "result": {} }] }),
            "module resolution entry 0 has invalid resolutionMode ES2022",
        ),
        (
            json!({ "fallback": "resolve", "entries": [{ "moduleName": "x", "result": {} }, { "moduleName": "x", "result": {} }] }),
            r#"duplicate static module resolution for "x""#,
        ),
    ] {
        assert_eq!(
            call(
                &session,
                "createModuleResolver",
                json!({ "compilerOptions": {}, "moduleResolutions": spec }),
            )
            .unwrap_err(),
            format!("api: client error: {error}")
        );
    }
    let id = resolver(&session, json!({ "compilerOptions": {} }));
    for (params, error) in [
        (
            json!({ "resolver": id, "moduleName": "", "containingDirectory": "/" }),
            "moduleName is empty",
        ),
        (
            json!({ "resolver": 99, "moduleName": "x", "containingDirectory": "/" }),
            "module resolver 99 not found",
        ),
        (
            json!({ "resolver": id, "moduleName": "x", "containingDirectory": "/", "resolutionMode": 200 }),
            "invalid resolutionMode Preserve",
        ),
        (
            json!({ "resolver": id, "moduleName": "x", "containingDirectory": "/", "snapshot": 1, "inProgressSnapshot": 1 }),
            "snapshot and inProgressSnapshot are mutually exclusive",
        ),
        (
            json!({ "resolver": id, "moduleName": "x", "containingDirectory": "/", "inProgressSnapshot": 3 }),
            "in-progress snapshot 3 not found",
        ),
    ] {
        assert_eq!(
            resolve(&session, params).unwrap_err(),
            format!("api: client error: {error}")
        );
    }
    assert_eq!(
        call_text(
            &session,
            "releaseModuleResolver",
            &json!({ "resolver": id })
        )
        .unwrap(),
        "null"
    );
    assert_eq!(
        call(&session, "releaseModuleResolver", json!({ "resolver": id })).unwrap_err(),
        format!("api: client error: module resolver {id} not found")
    );
}
