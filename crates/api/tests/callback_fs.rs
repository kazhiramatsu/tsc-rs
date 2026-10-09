//! tsgo `api/callbackfs.go`: the operations the client answers, their
//! answers' forms, and the base file system for a `null` answer or an
//! operation the client does not answer.

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

type Calls = Arc<Mutex<Vec<(String, String)>>>;

/// A callback file system with `callbacks` enabled over a base holding
/// `/base.ts` and `/dir/base.ts`, its client answering `answers`.
fn callback_fs(
    callbacks: &[&str],
    answers: Vec<Message>,
) -> (Arc<CallbackFs<Arc<MemFs>>>, Arc<Conn>, Calls) {
    let base = Arc::new(
        MemFs::from_entries(
            [
                ("/base.ts", Seed::file("base")),
                ("/dir/base.ts", Seed::file("base")),
            ],
            true,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let callbacks = callbacks
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let fs = Arc::new(CallbackFs::new(base, &callbacks).unwrap());
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

#[test]
fn read_file_takes_a_content_wrapper() {
    let (fs, _conn, made) = callback_fs(
        &["readFile"],
        vec![
            answer("readFile", r#"{"content":"client"}"#),
            answer("readFile", r#"{"content":null}"#),
            answer("readFile", "null"),
        ],
    );
    assert_eq!(fs.read("/client.ts").unwrap(), b"client");
    assert_eq!(
        fs.read("/missing.ts").unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    // `null`: the base answers.
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
fn existence_is_true_false_or_the_bases() {
    let (fs, _conn, _) = callback_fs(
        &["fileExists", "directoryExists"],
        vec![
            answer("fileExists", "true"),
            answer("fileExists", "false"),
            answer("fileExists", "null"),
            answer("directoryExists", "true"),
            answer("directoryExists", "null"),
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
                r#"{"files":["a.ts"],"directories":["sub"]}"#,
            ),
            answer("getAccessibleEntries", "null"),
            answer("realpath", r#""/real/a.ts""#),
            answer("realpath", "null"),
        ],
    );
    let entries = fs.accessible_entries("/virtual");
    assert_eq!(entries.files, ["a.ts"]);
    assert_eq!(entries.directories, ["sub"]);
    assert_eq!(fs.accessible_entries("/dir").files, ["base.ts"]);
    assert_eq!(fs.canonicalize("/link/a.ts").unwrap(), "/real/a.ts");
    assert_eq!(fs.canonicalize("/base.ts").unwrap(), "/base.ts");
}

#[test]
fn writes_and_removals_go_to_the_client() {
    let (fs, _conn, made) = callback_fs(
        &["writeFile", "removeFile"],
        vec![answer("writeFile", "null"), answer("removeFile", "null")],
    );
    fs.write("/out.js", b"output").unwrap();
    fs.remove("/out.js").unwrap();
    assert_eq!(
        calls(&made),
        [
            (
                "writeFile".to_owned(),
                r#"{"path":"/out.js","data":"output"}"#.to_owned()
            ),
            ("removeFile".to_owned(), r#""/out.js""#.to_owned()),
        ]
    );
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
    let base = Arc::new(MemFs::new(true, Arc::new(SystemClock)));
    let error = CallbackFs::new(base, &["readFile".to_owned(), "stat".to_owned()])
        .err()
        .unwrap();
    assert_eq!(error, "unknown callback name: stat");
}

#[test]
fn a_callback_before_the_connection_fails() {
    let base = Arc::new(MemFs::new(true, Arc::new(SystemClock)));
    let fs = CallbackFs::new(base, &["readFile".to_owned(), "writeFile".to_owned()]).unwrap();
    let panic =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fs.read("/a.ts"))).unwrap_err();
    assert_eq!(
        panic.downcast_ref::<String>().map(String::as_str),
        Some("CallbackFS: readFile called before connection set")
    );
    assert_eq!(
        fs.write("/a.ts", b"x").unwrap_err().to_string(),
        "CallbackFS: writeFile called before connection set"
    );
}
