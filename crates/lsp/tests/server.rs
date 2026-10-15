//! The language server's loops and lifecycle (tsgo `lsp/server_test.go` and
//! sessions recorded from tsgo).
//!
//! `fixtures/<name>.json` is a scripted session and `<name>.tsgo.txt` what
//! tsgo sent in it, recorded at aa8149273 with
//! `scripts/lsp_transcript.py --normalize fixtures/<name>.json -- tsgo --lsp --stdio`.
//! The tests replay each script against the server in process and require
//! the same messages.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_host::ipc::jsonrpc::{Reader, Writer};
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_ls::lsutil::{
    parse_user_preferences, QuotePreference, SemicolonPreference, UserPreferences,
};
use tsc_lsp::{Server, ServerOptions};
use tsc_lsproto::message::RawJson;
use tsc_lsproto::{self as lsproto, Id, Message, ResponseMessage};

const TIMEOUT: Duration = Duration::from_secs(30);

/// One direction of an in-memory stream: reads wait for data until the
/// writer closes it.
#[derive(Clone, Default)]
struct Pipe(Arc<(Mutex<PipeState>, Condvar)>);

/// The bytes not read yet, and whether the writer closed the stream.
type PipeState = (VecDeque<u8>, bool);

impl Pipe {
    fn close(&self) {
        let (state, ready) = &*self.0;
        state.lock().unwrap().1 = true;
        ready.notify_all();
    }

    fn contents(&self) -> String {
        let (state, _) = &*self.0;
        String::from_utf8(state.lock().unwrap().0.iter().copied().collect()).unwrap()
    }
}

impl Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let (state, ready) = &*self.0;
        let deadline = Instant::now() + TIMEOUT;
        let mut guard = state.lock().unwrap();
        while guard.0.is_empty() && !guard.1 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            guard = ready.wait_timeout(guard, left).unwrap().0;
        }
        let count = buffer.len().min(guard.0.len());
        for (slot, byte) in buffer.iter_mut().zip(guard.0.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

impl Write for Pipe {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let (state, ready) = &*self.0;
        state.lock().unwrap().0.extend(data);
        ready.notify_all();
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A server running in process, with the client's ends of its streams.
struct Session {
    server: Arc<Server>,
    input: Writer<Pipe>,
    input_pipe: Pipe,
    output: Reader<Pipe>,
    stderr: Pipe,
    run: Option<thread::JoinHandle<Result<(), String>>>,
}

impl Session {
    fn start() -> Self {
        Self::start_with(Arc::new(MemFs::new(false, Arc::new(SystemClock))))
    }

    /// A server whose session reads `fs` with the embedded library, as
    /// tsgo's (`bundled.WrapFS`).
    fn start_bundled(fs: Arc<MemFs>) -> Self {
        Self::start_over(
            Arc::new(BundledFs::new(fs)),
            EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
        )
    }

    /// A server whose session reads `fs`.
    fn start_with(fs: Arc<MemFs>) -> Self {
        Self::start_over(fs, "/typescript/lib".to_owned())
    }

    fn start_over(fs: Arc<dyn FileSystem>, default_library_path: String) -> Self {
        Self::start_in("/", fs, default_library_path, None)
    }

    /// A server in `cwd` whose content mappers start through `spawner`.
    fn start_in(
        cwd: &str,
        fs: Arc<dyn FileSystem>,
        default_library_path: String,
        spawner: Option<Arc<dyn tsc_program::content_mapper::host::Spawner>>,
    ) -> Self {
        let input_pipe = Pipe::default();
        let output_pipe = Pipe::default();
        let stderr = Pipe::default();
        let server = Server::new(ServerOptions {
            stderr: Box::new(stderr.clone()),
            cwd: cwd.to_owned(),
            set_parent_process_id: None,
            fs,
            default_library_path,
            spawner,
            progress_delay: Duration::ZERO,
        });
        let run = {
            let server = Arc::clone(&server);
            let input = input_pipe.clone();
            let output = output_pipe.clone();
            thread::spawn(move || {
                let result = server.run(Box::new(input), Box::new(output.clone()));
                output.close();
                result
            })
        };
        Self {
            server,
            input: Writer::new(input_pipe.clone()),
            input_pipe,
            output: Reader::new(output_pipe),
            stderr,
            run: Some(run),
        }
    }

    fn send(&mut self, text: &str) {
        self.input.write(text.as_bytes()).unwrap();
    }

    /// The next message the server wrote, or none at the end.
    fn receive(&mut self) -> Option<String> {
        self.output
            .read()
            .unwrap()
            .map(|data| String::from_utf8(data).unwrap())
    }

    fn close(&mut self) {
        self.input_pipe.close();
    }

    fn finish(&mut self) -> Result<(), String> {
        self.run.take().unwrap().join().unwrap()
    }
}

/// tsgo's handler durations as `<time>` (`scripts/lsp_transcript.py
/// --normalize`).
fn normalize(line: &str) -> String {
    let mut text = String::new();
    let mut rest = line;
    while let Some(index) = rest.find(" in ") {
        text.push_str(&rest[..index + 4]);
        let after = &rest[index + 4..];
        let digits = after
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(after.len());
        let unit = ["ns\"", "µs\"", "ms\"", "s\""]
            .iter()
            .find(|unit| digits > 0 && after[digits..].starts_with(*unit));
        match unit {
            Some(unit) => {
                text.push_str("<time>\"");
                rest = &after[digits + unit.len()..];
            }
            None => rest = after,
        }
    }
    text.push_str(rest);
    text
}

/// Replays `fixtures/<name>.json` as `scripts/lsp_transcript.py` does and
/// compares the messages, the result and the standard error with tsgo's.
fn replay(name: &str) {
    let (expected, actual) = run_fixture(name);
    assert_eq!(actual, expected);
}

/// [`replay`] without the logs (the window's and the standard error's), and
/// with the counters of tsgo's watch IDs (`watcher <n>.<i>`) left out.
fn replay_messages(name: &str) {
    let messages = |transcript: &str| {
        transcript
            .lines()
            .filter(|line| {
                !line.contains(r#""method":"window/logMessage""#) && !line.starts_with("STDERR ")
            })
            .map(|line| {
                let mut text = String::new();
                let mut rest = line;
                while let Some(index) = rest.find(" watcher ") {
                    text.push_str(&rest[..index + 9]);
                    rest = rest[index + 9..].trim_start_matches(|c: char| c.is_ascii_digit());
                    text.push('#');
                }
                text.push_str(rest);
                text
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let (expected, actual) = run_fixture(name);
    assert_eq!(messages(&actual), messages(&expected));
}

/// The fixture's tsgo transcript and the in-process server's. A fixture
/// with files runs over them in its workspace (`${WS}`), with the embedded
/// library, as the recording did.
fn run_fixture(name: &str) -> (String, String) {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut script: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures.join(format!("{name}.json"))).unwrap(),
    )
    .unwrap();
    let expected = std::fs::read_to_string(fixtures.join(format!("{name}.tsgo.txt"))).unwrap();
    let answers = script["answers"].as_object().cloned().unwrap_or_default();
    let workspace = script["workspace"].as_str().map(str::to_owned);
    let mut session = match (&workspace, script["files"].as_object()) {
        (Some(workspace), Some(files)) => {
            let entries = files
                .iter()
                .map(|(name, text)| {
                    (
                        format!("{workspace}/{name}"),
                        Seed::file(text.as_str().unwrap()),
                    )
                })
                .collect::<Vec<_>>();
            let steps = serde_json::to_string(&script["steps"])
                .unwrap()
                .replace("${WS}", workspace);
            script["steps"] = serde_json::from_str(&steps).unwrap();
            Session::start_bundled(Arc::new(
                MemFs::from_entries(entries, false, Arc::new(SystemClock)).unwrap(),
            ))
        }
        _ => Session::start(),
    };
    let mut lines = Vec::new();
    let mut receive = |session: &mut Session, until: &dyn Fn(&Value) -> bool| {
        while let Some(text) = session.receive() {
            lines.push(normalize(&text));
            let message: Value = serde_json::from_str(&text).unwrap();
            if let (Some(method), Some(id)) = (message.get("method"), message.get("id")) {
                let result = answers
                    .get(method.as_str().unwrap())
                    .cloned()
                    .unwrap_or(Value::Null);
                let answer = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result});
                session.send(&answer.to_string());
            }
            if until(&message) {
                return;
            }
        }
    };
    for step in script["steps"].as_array().unwrap() {
        if let Some(message) = step.get("send") {
            session.send(&message.to_string());
        } else if let Some(text) = step.get("raw") {
            session.send(text.as_str().unwrap());
        } else if let Some(id) = step.get("wait") {
            receive(&mut session, &|message| {
                message.get("id") == Some(id) && message.get("method").is_none()
            });
        } else if let Some(method) = step.get("waitMethod") {
            receive(&mut session, &|message| {
                message.get("method") == Some(method)
            });
        } else if step.get("close").is_some() {
            session.close();
        }
    }
    receive(&mut session, &|_| false);
    let result = session.finish();
    lines.push(format!("EXIT {}", if result.is_ok() { 0 } else { 1 }));
    lines.push(format!(
        "STDERR {}",
        serde_json::to_string(&session.stderr.contents()).unwrap()
    ));
    let mut actual = lines.join("\n");
    if let Some(workspace) = &workspace {
        actual = actual
            .replace(workspace.as_str(), "${WS}")
            .replace(workspace.to_lowercase().as_str(), "${WS}");
    }
    (expected.trim_end().to_owned(), actual)
}

/// initialize with VS Code's kind of capabilities, initialized with the
/// configuration round trip, an unknown method, notifications, a second
/// initialize and shutdown.
#[test]
fn lifecycle_matches_tsgo() {
    replay("lifecycle");
}

/// Requests before initialization, invalid params and messages,
/// preferences from the initialization options, log verbosity and string
/// ids.
/// An opened config's watches and its diagnostics pushed on
/// `tsconfig.json` (`publishProgramDiagnostics`), as tsgo sends them.
#[test]
fn pushed_config_diagnostics_match_tsgo() {
    replay_messages("push_diagnostics");
}

#[test]
fn errors_match_tsgo() {
    replay("errors");
}

impl Session {
    /// `initialize` with `params`, then `initialized`, answering the
    /// server's `workspace/configuration` with `configuration` and its
    /// registration with null, until the server is initialized.
    fn initialize(&mut self, params: Value, configuration: Value) {
        let initialize = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params});
        self.send(&initialize.to_string());
        self.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#);
        while let Some(text) = self.receive() {
            let message: Value = serde_json::from_str(&text).unwrap();
            let result = match message.get("method").and_then(Value::as_str) {
                Some("workspace/configuration") => configuration.clone(),
                Some("client/registerCapability") => Value::Null,
                _ => continue,
            };
            let answer =
                serde_json::json!({"jsonrpc": "2.0", "id": message["id"], "result": result});
            self.send(&answer.to_string());
            if message["method"] == "client/registerCapability" {
                break;
            }
        }
        self.server.wait_init_complete();
    }
}

/// tsgo's document notifications reach the session: an open loads the
/// document's project and logs tsgo's "Processed 1 file changes", a change
/// edits the document at the next request, a close leaves it to the disk,
/// and the settings of `workspace/didChangeConfiguration` choose the
/// config file name.
#[test]
fn documents_reach_the_session() {
    let fs = Arc::new(
        MemFs::from_entries(
            [
                (
                    "/ws/tsconfig.json",
                    Seed::file(r#"{"compilerOptions": {"noLib": true}, "include": ["src"]}"#),
                ),
                (
                    "/ws/tsconfig.all.json",
                    Seed::file(r#"{"compilerOptions": {"noLib": true, "strict": true}}"#),
                ),
                ("/ws/src/index.ts", Seed::file("export const x = 1;")),
            ],
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let mut session = Session::start_with(fs);
    session.initialize(
        serde_json::json!({"processId": null, "rootUri": "file:///ws", "capabilities": {}}),
        Value::Null,
    );
    let index = "file:///ws/src/index.ts";
    let document = serde_json::json!({
        "uri": index, "languageId": "typescript", "version": 1, "text": "export const x = 1;",
    });
    let edit = serde_json::json!({
        "textDocument": {"uri": index, "version": 2},
        "contentChanges": [{"range": {"start": {"line": 0, "character": 17}, "end": {"line": 0, "character": 18}}, "text": "2"}],
    });
    for (method, params) in [
        (
            "textDocument/didOpen",
            serde_json::json!({ "textDocument": document }),
        ),
        ("textDocument/didChange", edit),
        (
            "workspace/didChangeConfiguration",
            serde_json::json!({"settings": {"js/ts": {"customConfigFileName": "tsconfig.all.json"}}}),
        ),
    ] {
        let message = serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params});
        session.send(&message.to_string());
    }
    session.send(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    let mut logs = Vec::new();
    while let Some(text) = session.receive() {
        if text.contains(r#""id":2"#) {
            break;
        }
        let message: Value = serde_json::from_str(&text).unwrap();
        if message["method"] == "window/logMessage" {
            logs.push(message["params"]["message"].as_str().unwrap().to_owned());
        }
    }
    // The open's log, then the handler's, as tsgo writes them (the
    // snapshot's adoption logs in the background may come between).
    let open = logs
        .iter()
        .position(|log| log.starts_with("Processed 1 file changes in "))
        .expect("the open's log");
    assert!(logs[open + 1..]
        .iter()
        .any(|log| log.starts_with("handled method 'textDocument/didOpen' in ")));

    let projects = session.server.session().expect("the session");
    let (snapshot, project) = projects
        .get_snapshot_and_default_project(&lsproto::DocumentUri::from(index))
        .expect("the document's project");
    assert!(snapshot.is_open_file("/ws/src/index.ts"));
    assert_eq!(project.config_file_name(), Some("/ws/tsconfig.all.json"));
    let program = project.program().expect("the program");
    let text = program
        .prepared()
        .source_files()
        .iter()
        .find(|source| source.path().display().to_string_lossy() == "/ws/src/index.ts")
        .map(|source| source.text().to_owned());
    assert_eq!(text.as_deref(), Some("export const x = 2;"));

    session.close();
    assert_eq!(session.finish(), Ok(()));
}

/// tsgo `WatchFiles`: with a client that registers watchers, the session's
/// watches of an opened config's roots and program reach the client as
/// `client/registerCapability` of `workspace/didChangeWatchedFiles`, one
/// recursive glob of the workspace here.
#[test]
fn watches_reach_the_client() {
    let fs = Arc::new(
        MemFs::from_entries(
            [
                (
                    "/ws/tsconfig.json",
                    Seed::file(r#"{"compilerOptions": {"noLib": true}, "include": ["src"]}"#),
                ),
                ("/ws/src/index.ts", Seed::file("export const x = 1;")),
            ],
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let mut session = Session::start_with(fs);
    session.initialize(
        serde_json::json!({
            "processId": null, "rootUri": "file:///ws",
            "capabilities": {"workspace": {"didChangeWatchedFiles": {"dynamicRegistration": true}}},
        }),
        Value::Null,
    );
    let open = serde_json::json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
        "textDocument": {"uri": "file:///ws/src/index.ts", "languageId": "typescript", "version": 1, "text": "export const x = 1;"},
    }});
    session.send(&open.to_string());
    let registration = loop {
        let text = session.receive().expect("a message");
        let message: Value = serde_json::from_str(&text).unwrap();
        if message["method"] != "client/registerCapability" {
            continue;
        }
        let answer = serde_json::json!({"jsonrpc": "2.0", "id": message["id"], "result": null});
        session.send(&answer.to_string());
        break message["params"]["registrations"][0].clone();
    };
    assert_eq!(registration["method"], "workspace/didChangeWatchedFiles");
    let id = registration["id"].as_str().unwrap();
    assert!(
        id.starts_with("root files for /ws/tsconfig.json watcher ") && id.ends_with(".0"),
        "{id}"
    );
    assert_eq!(
        registration["registerOptions"]["watchers"],
        serde_json::json!([{"globPattern": "/ws/**/*", "kind": 7}])
    );
    session.send(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    while let Some(text) = session.receive() {
        if text.contains(r#""id":2"#) {
            break;
        }
    }
    session.close();
    assert_eq!(session.finish(), Ok(()));
}

/// `exit` ends the server with tsgo's `context canceled` (the command's
/// status 1).
#[test]
fn exit_stops_the_server() {
    let mut session = Session::start();
    session.initialize(
        serde_json::json!({"processId": null, "rootUri": null, "capabilities": {}}),
        Value::Null,
    );
    session.send(r#"{"jsonrpc":"2.0","method":"exit"}"#);
    assert_eq!(session.finish(), Err("context canceled".to_owned()));
}

/// tsgo `RequestConfiguration`: the preferences of the four sections the
/// client answers, and of the settings `workspace/didChangeConfiguration`
/// sends.
#[test]
fn preferences_come_from_the_configuration() {
    let mut session = Session::start();
    let configuration = serde_json::json!([
        {"preferences": {"quoteStyle": "single"}},
        {"format": {"semicolons": "remove"}},
        null,
        {"tabSize": 2, "insertSpaces": false},
    ]);
    session.initialize(
        serde_json::json!({
            "processId": null, "rootUri": null,
            "capabilities": {"workspace": {"configuration": true}},
        }),
        configuration.clone(),
    );
    let expected = parse_user_preferences(
        &serde_json::from_value(serde_json::json!({
            "js/ts": configuration[0], "typescript": configuration[1],
            "javascript": null, "editor": configuration[3],
        }))
        .unwrap(),
    );
    let preferences = session.server.user_preferences().unwrap();
    assert_eq!(preferences, expected);
    assert_eq!(preferences.quote_preference, QuotePreference::SINGLE);
    assert_eq!(
        preferences.format_code_settings.semicolons,
        SemicolonPreference::REMOVE
    );
    assert_eq!(preferences.format_code_settings.editor_settings.tab_size, 2);

    session.send(r#"{"jsonrpc":"2.0","method":"workspace/didChangeConfiguration","params":{"settings":{"js/ts":{"preferences":{"quoteStyle":"double"}}}}}"#);
    session.send(r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#);
    while let Some(text) = session.receive() {
        if text.contains(r#""id":2"#) {
            break;
        }
    }
    let preferences = session.server.user_preferences().unwrap();
    assert_eq!(preferences.quote_preference, QuotePreference::DOUBLE);
    session.close();
    assert_eq!(session.finish(), Ok(()));
}

/// Without the configuration capability, the initialization options'
/// preferences are the `js/ts` section; none are tsgo's defaults.
#[test]
fn preferences_come_from_the_initialization_options() {
    let mut session = Session::start();
    session.initialize(
        serde_json::json!({
            "processId": null, "rootUri": null, "capabilities": {},
            "initializationOptions": {"userPreferences": {"preferences": {"quoteStyle": "single"}}},
        }),
        Value::Null,
    );
    assert_eq!(
        session.server.user_preferences().unwrap().quote_preference,
        QuotePreference::SINGLE
    );
    session.close();
    assert_eq!(session.finish(), Ok(()));

    let mut session = Session::start();
    session.initialize(
        serde_json::json!({"processId": null, "rootUri": null, "capabilities": {}}),
        Value::Null,
    );
    assert_eq!(
        session.server.user_preferences(),
        Some(UserPreferences::new_default())
    );
    session.close();
    assert_eq!(session.finish(), Ok(()));
}

/// Before `initialize` the server logs to its standard error.
#[test]
fn logs_before_initialize_go_to_stderr() {
    let mut session = Session::start();
    session.send(r#"{"jsonrpc":"2.0","method":"exit"}"#);
    session.send(r#"{"jsonrpc":"2.0","id":1,"method":"shutdown"}"#);
    assert_eq!(
        session.receive().as_deref(),
        Some(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32002,"message":"ServerNotInitialized"}}"#
        )
    );
    // The dispatch loop logs the notification's error; wait for it.
    let deadline = Instant::now() + TIMEOUT;
    while session.stderr.contents().lines().count() < 2 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    session.close();
    assert_eq!(session.finish(), Ok(()));
    assert_eq!(
        session.stderr.contents(),
        "error handling method 'exit': ServerNotInitialized\nerror handling notification: ServerNotInitialized\n"
    );
}

/// tsgo `TestServerOutgoingQueueDoesNotBlockWithoutWriter`.
#[test]
fn outgoing_queue_does_not_block_without_writer() {
    let server = Server::new(ServerOptions {
        stderr: Box::new(io::sink()),
        cwd: "/test".to_owned(),
        set_parent_process_id: None,
        fs: Arc::new(MemFs::new(false, Arc::new(SystemClock))),
        default_library_path: "/typescript/lib".to_owned(),
        spawner: None,
        progress_delay: Duration::ZERO,
    });
    for _ in 0..1000 {
        let message = lsproto::RequestMessage::new(
            None,
            lsproto::method::WINDOW_LOG_MESSAGE,
            &lsproto::LogMessageParams {
                r#type: lsproto::MessageType::INFO,
                message: "queued".to_owned(),
            },
        )
        .unwrap();
        server.send(Message::Notification(message)).unwrap();
    }
}

/// A value nested `depth` arrays deep, serialized without a tree.
struct Nested(usize);

impl serde::Serialize for Nested {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(Some(usize::from(self.0 > 0)))?;
        if self.0 > 0 {
            sequence.serialize_element(&Nested(self.0 - 1))?;
        }
        sequence.end()
    }
}

/// tsgo `TestWriteLoopRecoversFromUnserializableResponse`: a response
/// nested deeper than Go's encoder allows fails only its request.
#[test]
fn write_loop_recovers_from_unserializable_response() {
    // Serializing 20,000 levels recurses that deep.
    let deep: RawJson = thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(|| serde_json::value::to_raw_value(&Nested(20_000)).unwrap())
        .unwrap()
        .join()
        .unwrap();
    let mut session = Session::start();
    let response = |id: &str, result: RawJson| {
        Message::Response(ResponseMessage {
            id: Some(Id::String(id.to_owned())),
            result: Some(result),
            error: None,
        })
    };
    session.server.send(response("bad", deep)).unwrap();
    session
        .server
        .send(response(
            "good",
            serde_json::value::to_raw_value(&Value::Null).unwrap(),
        ))
        .unwrap();
    // The error response is queued after the good one, as in tsgo.
    assert_eq!(
        session.receive().as_deref(),
        Some(r#"{"jsonrpc":"2.0","id":"good","result":null}"#)
    );
    assert_eq!(
        session.receive().as_deref(),
        Some(
            r#"{"jsonrpc":"2.0","id":"bad","error":{"code":-32603,"message":"failed to marshal message: jsontext: exceeded max depth"}}"#
        )
    );
    session.close();
    assert_eq!(session.finish(), Ok(()));
}

/// tsgo's test client (`lsptestutil`'s `onServerRequest` of
/// `TestSetContentMapperContributionsBeforeDidOpen`): the server's
/// configuration requests answered with nulls, its registrations and
/// unregistrations answered with null and kept.
#[derive(Default)]
struct RegistrationClient {
    registrations: Vec<Value>,
    unregistrations: Vec<Value>,
    /// The `$/progress` notifications.
    progress: Vec<Value>,
}

impl RegistrationClient {
    /// Answer one server request (or keep a notification away).
    fn answer(&mut self, session: &mut Session, message: &Value) {
        let result = match message.get("method").and_then(Value::as_str) {
            Some("workspace/configuration") => serde_json::json!([null, null, null, null]),
            Some("client/registerCapability") => {
                self.registrations.extend(
                    message["params"]["registrations"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
                Value::Null
            }
            Some("client/unregisterCapability") => {
                self.unregistrations.extend(
                    message["params"]["unregisterations"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default(),
                );
                Value::Null
            }
            Some("window/workDoneProgress/create") => Value::Null,
            Some("$/progress") => {
                self.progress.push(message["params"].clone());
                return;
            }
            _ => return,
        };
        if message.get("id").is_some() {
            let answer =
                serde_json::json!({"jsonrpc": "2.0", "id": message["id"], "result": result});
            session.send(&answer.to_string());
        }
    }

    /// The response to request `id`, the server's requests on the way
    /// answered.
    fn request(&mut self, session: &mut Session, id: i64, method: &str, params: Value) -> Value {
        let request =
            serde_json::json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        session.send(&request.to_string());
        loop {
            let Some(text) = session.receive() else {
                panic!("no message; server stderr: {}", session.stderr.contents());
            };
            let message: Value = serde_json::from_str(&text).unwrap();
            if message.get("method").is_none() && message["id"] == id {
                return message;
            }
            self.answer(session, &message);
        }
    }

    /// `initialize` with the client's `capabilities`, then `initialized`,
    /// the server's registration of it answered.
    fn initialize(&mut self, session: &mut Session, capabilities: Value) -> Value {
        let initialize = self.request(
            session,
            1,
            "initialize",
            serde_json::json!({"processId": null, "rootUri": null, "capabilities": capabilities}),
        );
        assert!(
            initialize.get("error").is_none(),
            "initialize failed: {initialize}"
        );
        self.notify(session, "initialized", serde_json::json!({}));
        loop {
            let text = session.receive().expect("a message");
            let message: Value = serde_json::from_str(&text).unwrap();
            self.answer(session, &message);
            if message["method"] == "client/registerCapability" {
                break;
            }
        }
        session.server.wait_init_complete();
        initialize
    }

    fn notify(&mut self, session: &mut Session, method: &str, params: Value) {
        let notification =
            serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params});
        session.send(&notification.to_string());
    }
}

/// tsgo `TestSetContentMapperContributionsBeforeDidOpen`: the contribution
/// finds the open .vue's configured project, whose mapper's extension the
/// server registers for each feature the client registers dynamically;
/// once the config drops the mapper, the open document's requests answer
/// null and the registrations go.
#[test]
fn content_mapper_contributions_before_did_open() {
    use tsc_harness::content_mappers::{package_json, TestSpawner, COMPONENT_MAPPER};
    const COMPONENT: &str = "<component name=\"ProfileCard\">\n<template><h1>{{ title }}</h1></template>\n<script lang=\"ts\">\nexport const title = \"Profile\";\n</script>";
    let fs = Arc::new(
        MemFs::from_entries(
            [
                (
                    "/home/project/tsconfig.json",
                    Seed::file(
                        r#"{
                            "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler", "strict": true },
                            "contentMappers": [ { "package": "mapper", "extensions": [".vue"] } ]
                        }"#,
                    ),
                ),
                (
                    "/home/project/node_modules/mapper/package.json",
                    Seed::file(package_json(COMPONENT_MAPPER)),
                ),
                ("/home/project/ProfileCard.vue", Seed::file(COMPONENT)),
            ],
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let mut session = Session::start_in(
        "/home/project",
        Arc::new(BundledFs::new(Arc::clone(&fs))),
        EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
        Some(Arc::new(TestSpawner::default())),
    );
    let mut client = RegistrationClient::default();
    let dynamic = serde_json::json!({"dynamicRegistration": true});
    let initialize = client.request(
        &mut session,
        1,
        "initialize",
        serde_json::json!({
            "processId": null,
            "rootUri": null,
            "capabilities": {
                "workspace": {"fileOperations": {"dynamicRegistration": true, "willRename": true}},
                "textDocument": {
                    "synchronization": dynamic,
                    "documentSymbol": dynamic,
                    "foldingRange": dynamic,
                    "selectionRange": dynamic,
                    "inlayHint": dynamic,
                    "codeLens": dynamic,
                    "codeAction": dynamic,
                    "formatting": dynamic,
                    "rangeFormatting": dynamic,
                    "onTypeFormatting": dynamic,
                    "linkedEditingRange": dynamic,
                    "callHierarchy": dynamic,
                    "semanticTokens": {
                        "dynamicRegistration": true,
                        "requests": {},
                        "tokenTypes": [],
                        "tokenModifiers": [],
                        "formats": ["relative"],
                    },
                },
            },
            "initializationOptions": {"runExternalCode": true},
        }),
    );
    assert!(
        initialize.get("error").is_none(),
        "initialize failed: {initialize}"
    );
    client.notify(&mut session, "initialized", serde_json::json!({}));
    // The server's registration of `initialized` answered, as tsgo's client
    // answers it on its own reader.
    loop {
        let text = session.receive().expect("a message");
        let message: Value = serde_json::from_str(&text).unwrap();
        client.answer(&mut session, &message);
        if message["method"] == "client/registerCapability" {
            break;
        }
    }
    session.server.wait_init_complete();
    client.registrations.clear();

    let uri = "file:///home/project/ProfileCard.vue";
    let response = client.request(
        &mut session,
        2,
        "custom/setContentMapperContributions",
        serde_json::json!({
            "openDocuments": [{"uri": uri}],
            "contributions": [{"contributorId": "test", "extensions": [".vue", ".svelte"]}],
        }),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert!(
        !client.registrations.is_empty(),
        "expected dynamic registrations"
    );
    let expected = [
        "content-mapper-did-open",
        "content-mapper-did-change",
        "content-mapper-did-close",
        "content-mapper-semantic-tokens",
        "content-mapper-document-symbol",
        "content-mapper-folding-range",
        "content-mapper-selection-range",
        "content-mapper-inlay-hint",
        "content-mapper-code-lens",
        "content-mapper-code-action",
        "content-mapper-formatting",
        "content-mapper-range-formatting",
        "content-mapper-on-type-formatting",
        "content-mapper-linked-editing",
        "content-mapper-call-hierarchy",
        "content-mapper-will-rename-files",
    ];
    for registration in &client.registrations {
        let id = registration["id"].as_str().unwrap();
        assert!(
            !id.starts_with("content-mapper-") || expected.contains(&id),
            "unexpected unsupported content mapper registration {id}"
        );
        if id == "content-mapper-did-open" || id == "content-mapper-semantic-tokens" {
            assert_eq!(
                registration["registerOptions"]["documentSelector"],
                serde_json::json!([{"pattern": "**/*.vue"}])
            );
        }
        if id == "content-mapper-code-action" {
            assert_eq!(
                registration["registerOptions"]["codeActionKinds"],
                serde_json::json!([
                    "quickfix",
                    "source.organizeImports.ts",
                    "source.removeUnusedImports.ts",
                    "source.sortImports.ts",
                    "source.fixAll.ts"
                ])
            );
        }
    }
    for id in expected {
        assert!(
            client
                .registrations
                .iter()
                .any(|registration| registration["id"] == id),
            "expected {id} registration for .vue"
        );
    }

    client.notify(
        &mut session,
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": uri, "languageId": "vue", "version": 1, "text": COMPONENT}}),
    );
    let position = serde_json::json!({"line": 3, "character": 15});
    let hover = client.request(
        &mut session,
        3,
        "textDocument/hover",
        serde_json::json!({"textDocument": {"uri": uri}, "position": position}),
    );
    assert!(hover.get("error").is_none(), "{hover}");
    assert!(
        !hover["result"].is_null(),
        "expected hover after first foreign didOpen"
    );

    fs.write(
        "/home/project/tsconfig.json",
        br#"{ "compilerOptions": { "target": "es2020", "module": "esnext", "moduleResolution": "bundler", "strict": true } }"#,
    )
    .unwrap();
    client.notify(
        &mut session,
        "workspace/didChangeWatchedFiles",
        serde_json::json!({"changes": [{"uri": "file:///home/project/tsconfig.json", "type": 2}]}),
    );
    let hover = client.request(
        &mut session,
        4,
        "textDocument/hover",
        serde_json::json!({"textDocument": {"uri": uri}, "position": position}),
    );
    assert!(
        hover.get("error").is_none(),
        "request before didClose should return a null result: {hover}"
    );
    assert!(hover["result"].is_null(), "{hover}");
    let diagnostics = client.request(
        &mut session,
        5,
        "textDocument/diagnostic",
        serde_json::json!({"textDocument": {"uri": uri}}),
    );
    assert!(
        diagnostics.get("error").is_none(),
        "diagnostics before didClose should return an empty report: {diagnostics}"
    );
    assert_eq!(diagnostics["result"]["kind"], "full");
    assert_eq!(diagnostics["result"]["items"], serde_json::json!([]));
    let completion = client.request(
        &mut session,
        6,
        "textDocument/completion",
        serde_json::json!({"textDocument": {"uri": uri}, "position": position}),
    );
    assert!(completion.get("error").is_none(), "{completion}");
    assert!(completion["result"].is_null(), "{completion}");
    let references = client.request(
        &mut session,
        7,
        "textDocument/references",
        serde_json::json!({"textDocument": {"uri": uri}, "position": position, "context": {"includeDeclaration": true}}),
    );
    assert!(references.get("error").is_none(), "{references}");
    assert!(references["result"].is_null(), "{references}");
    let rename = client.request(
        &mut session,
        8,
        "textDocument/rename",
        serde_json::json!({"textDocument": {"uri": uri}, "position": position, "newName": "renamed"}),
    );
    assert!(rename.get("error").is_none(), "{rename}");
    assert!(rename["result"].is_null(), "{rename}");
    // The unregistration comes from the background work of the update.
    let deadline = Instant::now() + TIMEOUT;
    while client.unregistrations.is_empty() {
        assert!(Instant::now() < deadline, "expected dynamic unregistration");
        let text = session.receive().expect("a message");
        let message: Value = serde_json::from_str(&text).unwrap();
        client.answer(&mut session, &message);
    }
    for unregistration in &client.unregistrations {
        let id = unregistration["id"].as_str().unwrap();
        assert!(
            !id.starts_with("content-mapper-") || expected.contains(&id),
            "unexpected unsupported content mapper unregistration {id}"
        );
    }
    for id in expected {
        assert!(
            client
                .unregistrations
                .iter()
                .any(|unregistration| unregistration["id"] == id),
            "expected {id} unregistration"
        );
    }

    client.notify(
        &mut session,
        "textDocument/didClose",
        serde_json::json!({"textDocument": {"uri": uri}}),
    );
    session.send(r#"{"jsonrpc":"2.0","id":9,"method":"shutdown"}"#);
    while let Some(text) = session.receive() {
        let message: Value = serde_json::from_str(&text).unwrap();
        if message.get("method").is_none() && message["id"] == 9 {
            assert!(message.get("error").is_none(), "{message}");
            break;
        }
        client.answer(&mut session, &message);
    }
    session.close();
    assert_eq!(session.finish(), Ok(()));
}

/// tsgo `initProjectInfoClient`'s session: `/home/projects` with `files`.
fn project_info_session(files: &[(&str, &str)]) -> (Session, RegistrationClient) {
    let fs = Arc::new(
        MemFs::from_entries(
            files
                .iter()
                .map(|(name, text)| (*name, Seed::file(*text)))
                .collect::<Vec<_>>(),
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let session = Session::start_in(
        "/home/projects",
        Arc::new(BundledFs::new(fs)),
        EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
        None,
    );
    (session, RegistrationClient::default())
}

fn open_document(client: &mut RegistrationClient, session: &mut Session, uri: &str, text: &str) {
    client.notify(
        session,
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": uri, "languageId": "typescript", "version": 0, "text": text}}),
    );
}

/// tsgo `TestInitializeCodeActionKinds`.
#[test]
fn initialize_code_action_kinds() {
    let (mut session, mut client) = project_info_session(&[]);
    let initialize = client.initialize(&mut session, serde_json::json!({}));
    assert_eq!(
        initialize["result"]["capabilities"]["codeActionProvider"]["codeActionKinds"],
        serde_json::json!([
            "quickfix",
            "source.organizeImports.ts",
            "source.removeUnusedImports.ts",
            "source.sortImports.ts",
            "source.fixAll.ts"
        ])
    );
}

/// tsgo `TestProjectInfoConfiguredProject`.
#[test]
fn project_info_of_a_configured_project() {
    let (mut session, mut client) = project_info_session(&[
        ("/home/projects/tsconfig.json", "{}"),
        ("/home/projects/index.ts", "export const x = 1;"),
    ]);
    client.initialize(&mut session, serde_json::json!({}));
    let uri = "file:///home/projects/index.ts";
    open_document(&mut client, &mut session, uri, "export const x = 1;");
    let response = client.request(
        &mut session,
        2,
        "custom/projectInfo",
        serde_json::json!({"textDocument": {"uri": uri}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(
        response["result"]["configFilePath"],
        "/home/projects/tsconfig.json"
    );
}

/// tsgo `TestProjectInfoInferredProject`.
#[test]
fn project_info_of_an_inferred_project() {
    let (mut session, mut client) =
        project_info_session(&[("/home/projects/index.ts", "export const x = 1;")]);
    client.initialize(&mut session, serde_json::json!({}));
    let uri = "file:///home/projects/index.ts";
    open_document(&mut client, &mut session, uri, "export const x = 1;");
    let response = client.request(
        &mut session,
        2,
        "custom/projectInfo",
        serde_json::json!({"textDocument": {"uri": uri}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["configFilePath"], "");
}

/// tsgo `TestProgressNotificationsEndToEnd`: loading the project shows its
/// progress, begun as tsgo's `Loading` and ended.
#[test]
fn progress_notifications_end_to_end() {
    let (mut session, mut client) = project_info_session(&[
        ("/home/projects/tsconfig.json", "{}"),
        ("/home/projects/index.ts", "export const x = 1;"),
    ]);
    client.initialize(
        &mut session,
        serde_json::json!({"window": {"workDoneProgress": true}}),
    );
    let uri = "file:///home/projects/index.ts";
    open_document(&mut client, &mut session, uri, "export const x = 1;");
    let response = client.request(
        &mut session,
        2,
        "custom/projectInfo",
        serde_json::json!({"textDocument": {"uri": uri}}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(
        response["result"]["configFilePath"],
        "/home/projects/tsconfig.json"
    );
    let deadline = Instant::now() + TIMEOUT;
    while !client
        .progress
        .iter()
        .any(|progress| progress["value"]["kind"] == "end")
    {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for progress end notification"
        );
        let text = session.receive().expect("a message");
        let message: Value = serde_json::from_str(&text).unwrap();
        client.answer(&mut session, &message);
    }
    let progress = &client.progress;
    assert!(progress.len() >= 2, "{progress:?}");
    assert_eq!(progress[0]["value"]["kind"], "begin");
    assert_eq!(progress[0]["value"]["title"], "Loading");
    assert_eq!(progress.last().unwrap()["value"]["kind"], "end");
}

/// tsgo `handleInitializeAPISession`: an API session over the server's
/// session, served on the pipe's first client, sees the editor's projects
/// and ends with the server's shutdown; without a path the server makes one
/// in the temporary directory.
#[cfg(unix)]
#[test]
fn api_session_over_the_language_server() {
    let (mut session, mut client) = project_info_session(&[
        ("/home/projects/tsconfig.json", "{}"),
        ("/home/projects/index.ts", "export const x = 1;"),
    ]);
    client.initialize(&mut session, serde_json::json!({}));
    let uri = "file:///home/projects/index.ts";
    open_document(&mut client, &mut session, uri, "export const x = 1;");
    let pipe = std::env::temp_dir()
        .join(format!(
            "tsc-rs-api-session-test-{}.sock",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let response = client.request(
        &mut session,
        2,
        "custom/initializeAPISession",
        serde_json::json!({"pipe": pipe}),
    );
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["pipe"], pipe);
    assert!(response["result"]["sessionId"]
        .as_str()
        .unwrap()
        .starts_with("api-session-"));

    let stream = std::os::unix::net::UnixStream::connect(&pipe).unwrap();
    let mut api_writer = Writer::new(stream.try_clone().unwrap());
    let mut api_reader = Reader::new(stream);
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getCurrentLanguageServerSnapshot",
        "params": {"changes": {"openFiles": ["/home/projects/index.ts"]}},
    });
    api_writer.write(request.to_string().as_bytes()).unwrap();
    let answer: Value =
        serde_json::from_slice(&api_reader.read().unwrap().expect("an answer")).unwrap();
    assert_eq!(answer["id"], 1, "{answer}");
    assert_eq!(
        answer["result"]["operation"]["openedFiles"][0]["project"], "/home/projects/tsconfig.json",
        "{answer}"
    );
    // The pipe's file goes once its client connected.
    assert!(!Path::new(&pipe).exists());

    let generated = client.request(
        &mut session,
        3,
        "custom/initializeAPISession",
        serde_json::json!({}),
    );
    let generated_pipe = generated["result"]["pipe"].as_str().unwrap().to_owned();
    assert!(
        Path::new(&generated_pipe)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("tsgo-api-"),
        "{generated_pipe}"
    );
    assert!(Path::new(&generated_pipe).exists());

    session.send(r#"{"jsonrpc":"2.0","id":4,"method":"shutdown"}"#);
    loop {
        let message: Value = serde_json::from_str(&session.receive().unwrap()).unwrap();
        if message.get("method").is_none() && message["id"] == 4 {
            assert!(message.get("error").is_none(), "{message}");
            break;
        }
        client.answer(&mut session, &message);
    }
    // The connected client's session ended, and the waiting one's pipe went.
    assert!(api_reader.read().unwrap().is_none());
    assert!(!Path::new(&generated_pipe).exists());
    session.close();
    session.finish().unwrap();
}
