//! tsgo `api/callbackfs.go` (aa8149273, its `callbackfs_test.go` cases): the
//! operations the client answers, their `{ kind, value }` answers, the
//! defaults and `:error` callbacks, and the base file system for a `useOS`
//! answer or an operation the client does not answer.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use tsc_api::callback_fs::CallbackFs;
use tsc_api::ipc::{Conn, Error, Handler, Id, Message, Mode, Payload, Protocol, ResponseError};
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};

/// A client whose answers are queued: each call reads the next one.
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
            .unwrap_or_else(PoisonError::into_inner)
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

struct NoRequests;

impl Handler for NoRequests {
    fn handle_request(&self, _method: &str, _params: &[u8]) -> Result<Payload, String> {
        Ok(Payload::null())
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// The client's answer to the call named `method`.
fn answer(method: &str, result: &str) -> Message {
    Message {
        id: Some(Id::String(method.to_owned())),
        result: result.as_bytes().to_vec(),
        ..Message::default()
    }
}

/// The client's error for the call named `method`, as vscode-jsonrpc
/// answers when the client's handler throws.
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

type Calls = Arc<Mutex<Vec<(String, String)>>>;

fn base() -> Arc<MemFs> {
    Arc::new(
        MemFs::from_entries(
            [
                ("/base.ts", Seed::file("base")),
                ("/dir/base.ts", Seed::file("base")),
            ],
            true,
            Arc::new(SystemClock),
        )
        .unwrap(),
    )
}

fn names(callbacks: &[&str]) -> Vec<String> {
    callbacks.iter().map(|name| (*name).to_owned()).collect()
}

/// A callback file system with `callbacks` enabled over a base holding
/// `/base.ts` and `/dir/base.ts`, its client answering `answers`.
fn callback_fs(
    callbacks: &[&str],
    answers: Vec<Message>,
) -> (Arc<CallbackFs<Arc<MemFs>>>, Arc<Conn>, Calls) {
    callback_fs_over(base(), callbacks, answers)
}

fn callback_fs_over(
    base: Arc<MemFs>,
    callbacks: &[&str],
    answers: Vec<Message>,
) -> (Arc<CallbackFs<Arc<MemFs>>>, Arc<Conn>, Calls) {
    let fs = Arc::new(CallbackFs::new(base, &names(callbacks), None).unwrap());
    let calls = Calls::default();
    let conn = Arc::new(Conn::new(
        Box::new(ScriptedClient {
            answers: answers.into(),
            calls: Arc::clone(&calls),
        }),
        Arc::new(NoRequests),
        Mode::Sync,
    ));
    fs.set_connection(&conn);
    (fs, conn, calls)
}

fn calls(calls: &Calls) -> Vec<(String, String)> {
    calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

fn panic_message(operation: impl FnOnce()) -> String {
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)).unwrap_err();
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_default()
}

#[test]
fn read_file_takes_a_value_missing_or_the_os() {
    let (fs, _conn, made) = callback_fs(
        &["readFile"],
        vec![
            answer("readFile", r#"{"kind":"value","value":"client"}"#),
            answer("readFile", r#"{"kind":"missing"}"#),
            answer("readFile", r#"{"kind":"useOS"}"#),
        ],
    );
    assert_eq!(fs.read("/client.ts").unwrap(), b"client");
    assert_eq!(
        fs.read("/missing.ts").unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    assert_eq!(fs.read("/base.ts").unwrap(), b"base");
    assert_eq!(
        calls(&made),
        [
            ("readFile".to_owned(), r#""/client.ts""#.to_owned()),
            ("readFile".to_owned(), r#""/missing.ts""#.to_owned()),
            ("readFile".to_owned(), r#""/base.ts""#.to_owned()),
        ]
    );
}

#[test]
fn existence_is_a_value_or_the_os() {
    let (fs, _conn, _) = callback_fs(
        &["fileExists", "directoryExists"],
        vec![
            answer("fileExists", r#"{"kind":"value","value":true}"#),
            answer("fileExists", r#"{"kind":"value","value":false}"#),
            answer("fileExists", r#"{"kind":"useOS"}"#),
            answer("directoryExists", r#"{"kind":"value","value":true}"#),
            answer("directoryExists", r#"{"kind":"useOS"}"#),
        ],
    );
    assert!(fs.is_file("/virtual.ts"));
    assert!(!fs.is_file("/base.ts"));
    assert!(fs.is_file("/base.ts"));
    assert!(fs.is_dir("/virtual"));
    assert!(fs.is_dir("/dir"));
}

#[test]
fn listings_and_real_paths_come_from_the_client() {
    let (fs, _conn, _) = callback_fs(
        &["getAccessibleEntries", "realpath"],
        vec![
            answer(
                "getAccessibleEntries",
                r#"{"kind":"value","value":{"files":["a.ts"],"directories":["sub"]}}"#,
            ),
            answer("getAccessibleEntries", r#"{"kind":"useOS"}"#),
            answer("realpath", r#"{"kind":"value","value":"/real/a.ts"}"#),
            answer("realpath", r#"{"kind":"value","value":"../real/b.ts"}"#),
            answer("realpath", r#"{"kind":"useOS"}"#),
        ],
    );
    let entries = fs.accessible_entries("/virtual");
    assert_eq!(entries.files, ["a.ts"]);
    assert_eq!(entries.directories, ["sub"]);
    assert_eq!(fs.accessible_entries("/dir").files, ["base.ts"]);
    assert_eq!(fs.canonicalize("/link/a.ts").unwrap(), "/real/a.ts");
    // tspath.ToRootedPath: a relative answer is the path's directory's.
    assert_eq!(
        fs.canonicalize("/link/sub/b.ts").unwrap(),
        "/link/real/b.ts"
    );
    assert_eq!(fs.canonicalize("/base.ts").unwrap(), "/base.ts");
}

// tsgo TestCallbackFSDefaults.
#[test]
fn defaults_answer_without_the_client() {
    let base = Arc::new(
        MemFs::from_entries(
            [("/file.ts", Seed::file("content"))],
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let fs = CallbackFs::new(
        base,
        &names(&["realpath:identity", "stat:fakeStat"]),
        Some(true),
    )
    .unwrap();
    assert!(fs.case_sensitive());
    assert_eq!(fs.canonicalize("/file.ts").unwrap(), "/file.ts");
    let file = fs.metadata("/file.ts").unwrap();
    assert!(file.is_file());
    assert_eq!(file.len(), 0);
    // A fake stat has Go's zero `ModTime`, which a build reads as no time.
    assert_eq!(file.modified(), None);
    assert!(fs.metadata("/").unwrap().is_dir());
    assert_eq!(
        fs.metadata("/missing").unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
}

// tsgo TestCallbackFSStatAndEntries (the port's metadata has no permission
// bits, and its entries no "unavailable" symlink set).
#[test]
fn stats_and_entries_take_the_clients_metadata() {
    let (fs, _conn, _) = callback_fs(
        &["stat", "getAccessibleEntries"],
        vec![
            answer(
                "stat",
                r#"{"kind":"value","value":{"mode":33060,"size":12,"mtime":"2024-01-02T03:04:05.000Z"}}"#,
            ),
            answer("stat", r#"{"kind":"missing"}"#),
            answer(
                "getAccessibleEntries",
                r#"{"kind":"value","value":{"files":["link.ts"],"directories":["pkg"],"symlinks":["link.ts","pkg"]}}"#,
            ),
        ],
    );
    let info = fs.metadata("/link.ts").unwrap();
    assert!(info.is_file());
    assert_eq!(info.len(), 12);
    assert_eq!(
        info.modified(),
        Some(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_704_164_645))
    );
    assert_eq!(
        fs.metadata("/missing.ts").unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    let entries = fs.accessible_entries("/");
    assert!(entries.symlinks.contains("link.ts"));
    assert!(entries.symlinks.contains("pkg"));
}

// tsgo TestCallbackFSWriteFilePassthrough.
#[test]
fn a_write_answer_says_who_writes() {
    let base = base();
    let (fs, _conn, made) = callback_fs_over(
        Arc::clone(&base),
        &["writeFile"],
        vec![
            answer("writeFile", r#"{"kind":"useOS"}"#),
            answer("writeFile", r#"{"kind":"value"}"#),
            answer("writeFile", r#"{"kind":"noop"}"#),
        ],
    );
    fs.write("/use-os.ts", b"content").unwrap();
    assert_eq!(base.read("/use-os.ts").unwrap(), b"content");
    fs.write("/handled.ts", b"content").unwrap();
    assert!(!base.is_file("/handled.ts"));
    fs.write("/noop.ts", b"content").unwrap();
    assert!(!base.is_file("/noop.ts"));
    assert_eq!(
        calls(&made)[0],
        (
            "writeFile".to_owned(),
            r#"{"path":"/use-os.ts","data":"content"}"#.to_owned()
        )
    );
}

// tsgo TestCallbackFSWriteFileNoop and TestCallbackFSRemoveFileNoop.
#[test]
fn noop_defaults_keep_writes_and_removals_from_the_base() {
    let base = base();
    let fs = CallbackFs::new(
        Arc::clone(&base),
        &names(&["writeFile:noop", "removeFile:noop"]),
        None,
    )
    .unwrap();
    fs.write("/ignored.ts", b"content").unwrap();
    assert!(!base.is_file("/ignored.ts"));
    fs.remove("/base.ts").unwrap();
    assert!(base.is_file("/base.ts"));
}

// tsgo TestCallbackFSPerCallFakeStat and TestCallbackFSPerCallIdentityRealpath.
#[test]
fn per_call_answers_take_the_defaults_forms() {
    let (fs, _conn, _) = callback_fs(
        &["stat", "directoryExists", "fileExists", "realpath"],
        vec![
            answer("stat", r#"{"kind":"fakeStat"}"#),
            answer("directoryExists", r#"{"kind":"value","value":false}"#),
            answer("fileExists", r#"{"kind":"value","value":true}"#),
            answer("realpath", r#"{"kind":"identity"}"#),
        ],
    );
    assert!(fs.metadata("/virtual.ts").unwrap().is_file());
    assert_eq!(fs.canonicalize("/virtual.ts").unwrap(), "/virtual.ts");
}

// tsgo TestCallbackFSError.
#[test]
fn error_callbacks_and_error_answers_fail() {
    let callbacks = tsc_api::callback_fs::CALLBACK_NAMES
        .iter()
        .map(|name| format!("{name}:error"))
        .collect::<Vec<_>>();
    let fs = CallbackFs::new(base(), &callbacks, None).unwrap();
    assert!(panic_message(|| {
        let _ = fs.read("/unexpected.ts");
    })
    .contains("serverFS.error: readFile"));
    let (fs, _conn, _) = callback_fs(
        &["fileExists"],
        vec![answer("fileExists", r#"{"kind":"error"}"#)],
    );
    assert!(panic_message(|| {
        fs.is_file("/unexpected.ts");
    })
    .contains("serverFS.error: fileExists"));
}

#[test]
fn a_written_file_is_the_clients_alone() {
    // tsgo `WriteFile` calls the client once and returns its error; the
    // base creates no directory for the file.
    let (fs, _conn, made) = callback_fs(
        &["writeFile"],
        vec![failure(
            "writeFile",
            "Request writeFile failed with message: write failed",
        )],
    );
    assert_eq!(
        fs.write_creating_dirs("/out/dir/a.js", b"output")
            .unwrap_err()
            .to_string(),
        "ipc: remote error [-32603]: Request writeFile failed with message: write failed"
    );
    assert_eq!(calls(&made).len(), 1);
    assert!(!fs.is_dir("/out"));
}

#[test]
fn a_written_file_without_the_callback_is_the_bases() {
    let (fs, _conn, made) = callback_fs(&["fileExists"], Vec::new());
    fs.write_creating_dirs("/out/dir/a.js", b"output").unwrap();
    assert_eq!(fs.read("/out/dir/a.js").unwrap(), b"output");
    assert!(calls(&made).is_empty());
}

#[test]
fn an_operation_the_client_does_not_answer_is_the_bases() {
    let (fs, _conn, made) = callback_fs(&["fileExists"], Vec::new());
    assert_eq!(fs.read("/base.ts").unwrap(), b"base");
    assert!(fs.is_dir("/dir"));
    assert_eq!(fs.accessible_entries("/dir").files, ["base.ts"]);
    assert!(calls(&made).is_empty());
}

#[test]
fn an_unknown_callback_is_rejected() {
    for callbacks in [&["readFile", "bogus"][..], &["bogus:error"]] {
        let error = CallbackFs::new(base(), &names(callbacks), None)
            .err()
            .unwrap();
        assert_eq!(error, "unknown callback name: bogus");
    }
}

#[test]
fn a_callback_before_the_connection_fails() {
    let fs = CallbackFs::new(base(), &names(&["readFile", "writeFile"]), None).unwrap();
    assert_eq!(
        panic_message(|| {
            let _ = fs.read("/a.ts");
        }),
        "CallbackFS: readFile called before connection set"
    );
    assert_eq!(
        fs.write("/a.ts", b"x").unwrap_err().to_string(),
        "CallbackFS: writeFile called before connection set"
    );
}
