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

    /// `data` and then the end, under one lock: a reader finds both at
    /// once.
    fn write_and_close(&self, data: &[u8]) {
        let (state, ready) = &*self.0;
        let mut guard = state.lock().unwrap();
        guard.0.extend(data);
        guard.1 = true;
        drop(guard);
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
            typings_location: None,
            npm_executor: None,
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

    /// `text` and the input's end together, as a client that closes its
    /// end right after a message (tsgo's transcript tool writes `exit` and
    /// closes stdin). The server's read loop reads the message and then the
    /// end at once, so the server ends as the input did, before the
    /// dispatch loop handles the message: tsgo's `Run` takes the first of
    /// the read loop's io.EOF and the dispatch loop's cancellation, and a
    /// separate close could come after the read loop blocked for the next
    /// message.
    fn send_and_close(&mut self, text: &str) {
        let frame = format!("Content-Length: {}\r\n\r\n{text}", text.len());
        self.input_pipe.write_and_close(frame.as_bytes());
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
    let steps = script["steps"].as_array().unwrap();
    let mut index = 0;
    while let Some(step) = steps.get(index) {
        index += 1;
        if let Some(message) = step.get("send") {
            if steps
                .get(index)
                .is_some_and(|next| next.get("close").is_some())
            {
                session.send_and_close(&message.to_string());
                index += 1;
            } else {
                session.send(&message.to_string());
            }
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
        typings_location: None,
        npm_executor: None,
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
        self.initialize_with_options(session, capabilities, Value::Null)
    }

    /// [`Self::initialize`] with the client's `initializationOptions`.
    fn initialize_with_options(
        &mut self,
        session: &mut Session,
        capabilities: Value,
        initialization_options: Value,
    ) -> Value {
        let initialize = self.request(
            session,
            1,
            "initialize",
            serde_json::json!({
                "processId": null,
                "rootUri": null,
                "capabilities": capabilities,
                "initializationOptions": initialization_options,
            }),
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

/// tsgo's profile commands: a CPU profile at a time, written where the
/// client asked when it stops; tsc-rs takes no heap or allocation profiles,
/// and `runGC` answers as tsgo's.
#[cfg(unix)]
#[test]
fn profile_commands() {
    let (mut session, mut client) = project_info_session(&[]);
    client.initialize(&mut session, serde_json::json!({}));
    let directory = std::env::temp_dir()
        .join(format!("tsc-rs-lsp-profile-test-{}", std::process::id()))
        .to_string_lossy()
        .into_owned();
    let params = serde_json::json!({"dir": directory});
    let error = |response: &Value| {
        response["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    let started = client.request(&mut session, 2, "custom/startCPUProfile", params.clone());
    assert_eq!(started["result"], Value::Null, "{started}");
    let again = client.request(&mut session, 3, "custom/startCPUProfile", params.clone());
    assert_eq!(error(&again), "CPU profiling already in progress");
    let heap = client.request(&mut session, 4, "custom/saveHeapProfile", params.clone());
    assert_eq!(error(&heap), "heap profiles are not supported by tsc-rs");
    let allocations = client.request(&mut session, 5, "custom/saveAllocProfile", params);
    assert_eq!(
        error(&allocations),
        "allocation profiles are not supported by tsc-rs"
    );
    session.send(r#"{"jsonrpc":"2.0","id":6,"method":"custom/runGC"}"#);
    session.send(r#"{"jsonrpc":"2.0","id":7,"method":"custom/stopCPUProfile"}"#);
    session.send(r#"{"jsonrpc":"2.0","id":8,"method":"custom/stopCPUProfile"}"#);
    let mut answers = std::collections::BTreeMap::new();
    while answers.len() < 3 {
        let message: Value = serde_json::from_str(&session.receive().unwrap()).unwrap();
        if message.get("method").is_none() {
            answers.insert(message["id"].as_i64().unwrap(), message);
        } else {
            client.answer(&mut session, &message);
        }
    }
    assert_eq!(answers[&6]["result"], Value::Null, "{}", answers[&6]);
    let file = answers[&7]["result"]["file"].as_str().unwrap().to_owned();
    assert!(
        file.starts_with(&directory) && file.ends_with("-cpuprofile.pb.gz"),
        "{file}"
    );
    assert!(Path::new(&file).exists());
    assert_eq!(error(&answers[&8]), "CPU profiling not in progress");
    let _ = std::fs::remove_dir_all(&directory);
}

/// tsgo `TestFlakyDiagnosticTrackingParallelEmit`: with flaky diagnostics
/// panicked on, the diagnostics after an emit are the same (none), with and
/// without `noEmitOnError`.
#[test]
fn flaky_diagnostic_tracking_parallel_emit() {
    for no_emit_on_error in [false, true] {
        let config = format!(
            r#"{{ "compilerOptions": {{ "strict": true, "declaration": true, "noEmitOnError": {no_emit_on_error}, "outDir": "out" }} }}"#
        );
        let a = "export function box<T>(value: T) { return { value }; }";
        let fs = Arc::new(
            MemFs::from_entries(
                [
                    ("/src/tsconfig.json", config.as_str()),
                    ("/src/a.ts", a),
                    (
                        "/src/b.ts",
                        r#"import { box } from "./a"; export const b = box("b");"#,
                    ),
                    (
                        "/src/c.ts",
                        r#"import { box } from "./a"; export const c = box(1);"#,
                    ),
                ]
                .map(|(name, text)| (name, Seed::file(text))),
                false,
                Arc::new(SystemClock),
            )
            .unwrap(),
        );
        let mut session = Session::start_in(
            "/src",
            Arc::new(BundledFs::new(fs)),
            EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
            None,
        );
        let mut client = RegistrationClient::default();
        client.initialize_with_options(
            &mut session,
            serde_json::json!({}),
            serde_json::json!({"trackFlakyDiagnostics": 2}),
        );
        let uri = "file:///src/a.ts";
        open_document(&mut client, &mut session, uri, a);
        let diagnostics = client.request(
            &mut session,
            2,
            "textDocument/diagnostic",
            serde_json::json!({"textDocument": {"uri": uri}}),
        );
        assert!(diagnostics.get("error").is_none(), "{diagnostics}");
        assert_eq!(diagnostics["result"]["kind"], "full", "{diagnostics}");
        assert_eq!(
            diagnostics["result"]["items"],
            serde_json::json!([]),
            "{diagnostics}"
        );
    }
}

/// tsgo's builtin watcher (`lspwatcher`): a client that cannot register
/// watchers, on a system that watches a tree at once, gets the server's own
/// watching. No watcher goes to the client, and a change on disk reaches
/// the session.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn the_builtin_watcher_reports_changes_on_disk() {
    use tsc_host::vfs::OsFs;
    let directory =
        std::env::temp_dir().join(format!("tsc-rs-lsp-builtin-watcher-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true } }"#,
    )
    .unwrap();
    let a = "import { b } from \"./b\";\nexport const n: number = b;\n";
    std::fs::write(directory.join("a.ts"), a).unwrap();
    std::fs::write(directory.join("b.ts"), "export const b = 1;\n").unwrap();
    let root = directory.to_string_lossy().replace('\\', "/");
    let mut session = Session::start_in(
        &root,
        Arc::new(BundledFs::new(OsFs::new(false))),
        EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
        None,
    );
    let mut client = RegistrationClient::default();
    client.initialize(&mut session, serde_json::json!({}));
    let uri = tsc_program::uri::file_name_to_document_uri(&format!("{root}/a.ts"));
    open_document(&mut client, &mut session, &uri, a);
    let errors = |client: &mut RegistrationClient, session: &mut Session, id: i64| {
        let diagnostics = client.request(
            session,
            id,
            "textDocument/diagnostic",
            serde_json::json!({"textDocument": {"uri": uri}}),
        );
        diagnostics["result"]["items"]
            .as_array()
            .map_or(0, Vec::len)
    };
    assert_eq!(errors(&mut client, &mut session, 2), 0);
    // The watch starts with the project; then b.ts changes on disk.
    thread::sleep(Duration::from_millis(500));
    std::fs::write(directory.join("b.ts"), "export const b = \"b\";\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut id = 3;
    while errors(&mut client, &mut session, id) == 0 {
        assert!(
            Instant::now() < deadline,
            "the change of b.ts never arrived"
        );
        thread::sleep(Duration::from_millis(100));
        id += 1;
    }
    assert!(
        client
            .registrations
            .iter()
            .all(|registration| registration["method"] != "workspace/didChangeWatchedFiles"),
        "{:?}",
        client.registrations
    );
    session.close();
    let _ = std::fs::remove_dir_all(&directory);
}

/// tsgo's auto-import registry extracts a package's exports with its alias
/// resolver, whose format hooks answer ESNext for every file
/// (`aliasresolver.go`): there a `.d.cts` file's
/// `export { default as ar } from "./ar.cjs"` of an `export =` module has
/// no synthetic default, so the re-export resolves to nothing (kind
/// unknown, the completion's Property) and groups with its re-export
/// through `export *`, which wins on the shorter specifier, while the
/// `export =` module's own default stays a group of its own. tsgo's answer
/// at aa814927 (playwright's zod 4 locales).
#[test]
fn auto_imports_resolve_a_package_s_aliases_as_esm() {
    const INDEX: &str = "import \"pkg/locales\";\nar\n";
    let fs = Arc::new(
        MemFs::from_entries(
            [
                (
                    "/ws/package.json",
                    Seed::file(r#"{"name": "app", "dependencies": {"pkg": "1.0.0"}}"#),
                ),
                (
                    "/ws/tsconfig.json",
                    Seed::file(
                        r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}}"#,
                    ),
                ),
                ("/ws/index.ts", Seed::file(INDEX)),
                (
                    "/ws/node_modules/pkg/package.json",
                    Seed::file(
                        r#"{"name": "pkg", "version": "1.0.0", "type": "module", "exports": {"./locales": {"types": "./locales/index.d.cts"}, "./v4/locales": {"types": "./v4/locales/index.d.cts"}, "./v4/locales/*": {"types": "./v4/locales/*"}}}"#,
                    ),
                ),
                (
                    "/ws/node_modules/pkg/locales/index.d.cts",
                    Seed::file("export * from \"../v4/locales/index.cjs\";\n"),
                ),
                (
                    "/ws/node_modules/pkg/v4/locales/index.d.cts",
                    Seed::file("export { default as ar } from \"./ar.cjs\";\n"),
                ),
                (
                    "/ws/node_modules/pkg/v4/locales/ar.d.cts",
                    Seed::file(
                        "declare function _default(): { localeError: string };\nexport = _default;\n",
                    ),
                ),
            ],
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let mut session = Session::start_with(fs);
    let mut client = RegistrationClient::default();
    client.initialize(&mut session, serde_json::json!({}));
    let index = "file:///ws/index.ts";
    client.notify(
        &mut session,
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": index, "languageId": "typescript", "version": 1, "text": INDEX}}),
    );
    let response = client.request(
        &mut session,
        2,
        "textDocument/completion",
        serde_json::json!({"textDocument": {"uri": index}, "position": {"line": 1, "character": 2}}),
    );
    let mut items = response["result"]["items"]
        .as_array()
        .expect("the items")
        .iter()
        .filter(|item| item["label"] == "ar")
        .map(|item| {
            let auto_import = &item["data"]["autoImport"];
            (
                item["kind"].as_i64().expect("a kind"),
                auto_import["moduleSpecifier"]
                    .as_str()
                    .expect("a specifier")
                    .to_owned(),
                auto_import["importKind"].as_i64().expect("an import kind"),
            )
        })
        .collect::<Vec<_>>();
    items.sort();
    assert_eq!(
        items,
        [
            (3, "pkg/v4/locales/ar.cjs".to_owned(), 1),
            (10, "pkg/locales".to_owned(), 0),
        ]
    );
    session.close();
}

/// tsgo's program symlink cache also looks up the runtime dependencies of
/// each emitted file's package scope as package directories
/// (`GetSymlinkCache` → `ResolvePackageDirectory`: the nearest
/// `node_modules/<name>`, no `paths`), so a workspace package that `paths`
/// maps and only another package's `node_modules` links is still known by
/// its link, and its file is named through the package's exports
/// (`"./*": "./*"`) from a package that does not see the link. tsgo's
/// answer at aa814927 (vue's `@vue/server-renderer/src/internal.js`).
#[test]
fn auto_imports_name_a_file_through_a_dependency_s_symlink() {
    const A: &str = "rend\n";
    let fs = Arc::new(
        MemFs::from_entries(
            [
                ("/ws/package.json", Seed::file(r#"{"name": "root", "private": true}"#)),
                (
                    "/ws/tsconfig.json",
                    Seed::file(
                        r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true, "paths": {"@scope/*": ["./packages/*/src"]}}, "include": ["packages/*/src"]}"#,
                    ),
                ),
                (
                    "/ws/packages/a/package.json",
                    Seed::file(r#"{"name": "@scope/a", "version": "1.0.0"}"#),
                ),
                ("/ws/packages/a/src/index.ts", Seed::file(A)),
                (
                    "/ws/packages/b/package.json",
                    Seed::file(
                        r#"{"name": "@scope/b", "version": "1.0.0", "exports": {".": {"types": "./dist/b.d.ts"}, "./*": "./*"}}"#,
                    ),
                ),
                ("/ws/packages/b/src/index.ts", Seed::file("export const b = 1;\n")),
                (
                    "/ws/packages/b/src/internal.ts",
                    Seed::file("export function renderThing(): void {}\n"),
                ),
                (
                    "/ws/packages/c/package.json",
                    Seed::file(
                        r#"{"name": "@scope/c", "version": "1.0.0", "dependencies": {"@scope/b": "workspace:*"}}"#,
                    ),
                ),
                ("/ws/packages/c/src/index.ts", Seed::file("export const c = 1;\n")),
                (
                    "/ws/packages/c/node_modules/@scope/b",
                    Seed::symlink("/ws/packages/b"),
                ),
            ],
            false,
            Arc::new(SystemClock),
        )
        .unwrap(),
    );
    let mut session = Session::start_with(fs);
    let mut client = RegistrationClient::default();
    client.initialize(&mut session, serde_json::json!({}));
    let a = "file:///ws/packages/a/src/index.ts";
    client.notify(
        &mut session,
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": a, "languageId": "typescript", "version": 1, "text": A}}),
    );
    let response = client.request(
        &mut session,
        2,
        "textDocument/completion",
        serde_json::json!({"textDocument": {"uri": a}, "position": {"line": 0, "character": 4}}),
    );
    let auto_imports = response["result"]["items"]
        .as_array()
        .expect("the items")
        .iter()
        .filter(|item| item["data"].get("autoImport").is_some())
        .map(|item| {
            (
                item["label"].as_str().expect("a label").to_owned(),
                item["data"]["autoImport"]["moduleSpecifier"]
                    .as_str()
                    .expect("a specifier")
                    .to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        auto_imports,
        [(
            "renderThing".to_owned(),
            "@scope/b/src/internal.js".to_owned()
        )]
    );
    session.close();
}

/// The auto-import items of `text` at `(line, character)` in a session over
/// `entries` (with `/ws/index.ts` holding `text`): label, kind, specifier,
/// import kind and tags.
fn auto_import_items(
    entries: Vec<(&'static str, Seed)>,
    text: &str,
    (line, character): (u32, u32),
) -> Vec<(String, i64, String, i64, Value)> {
    auto_import_items_in("file:///ws/index.ts", entries, text, (line, character))
}

/// The auto-import items of `text` at `(line, character)` in the open file
/// `index` of a session over `entries`.
fn auto_import_items_in(
    index: &str,
    entries: Vec<(&'static str, Seed)>,
    text: &str,
    (line, character): (u32, u32),
) -> Vec<(String, i64, String, i64, Value)> {
    let fs = Arc::new(MemFs::from_entries(entries, false, Arc::new(SystemClock)).unwrap());
    let mut session = Session::start_with(fs);
    let mut client = RegistrationClient::default();
    client.initialize(&mut session, serde_json::json!({}));
    client.notify(
        &mut session,
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": index, "languageId": "typescript", "version": 1, "text": text}}),
    );
    let response = client.request(
        &mut session,
        2,
        "textDocument/completion",
        serde_json::json!({"textDocument": {"uri": index}, "position": {"line": line, "character": character}}),
    );
    session.close();
    let mut items = response["result"]["items"]
        .as_array()
        .expect("the items")
        .iter()
        .filter(|item| item["data"].get("autoImport").is_some())
        .map(|item| {
            let auto_import = &item["data"]["autoImport"];
            (
                item["label"].as_str().expect("a label").to_owned(),
                item["kind"].as_i64().expect("a kind"),
                auto_import["moduleSpecifier"]
                    .as_str()
                    .expect("a specifier")
                    .to_owned(),
                auto_import["importKind"].as_i64().expect("an import kind"),
                item.get("tags").cloned().unwrap_or(Value::Null),
            )
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    items
}

/// tsgo's alias resolver parses the JavaScript file a package's re-export
/// resolves to (`GetSourceFileForResolvedModule`; an alias resolution
/// reports no resolution diagnostic), so `export { createRef } from 'jsdep'`
/// of a JavaScript-only package resolves to its CommonJS export, and the
/// package's other aliases still resolve: `hrtime` groups with its
/// declaration (found by the LSP test on next.js). tsc-rs's checker had
/// dropped the JavaScript file without `allowJs`, failing every later module
/// lookup of that checker. Pinned to tsgo's answers at aa814927.
#[test]
fn auto_imports_resolve_a_package_s_javascript_re_export() {
    let entries = || {
        vec![
            (
                "/ws/package.json",
                Seed::file(r#"{"name": "app", "dependencies": {"pkg": "1.0.0"}}"#),
            ),
            (
                "/ws/tsconfig.json",
                Seed::file(
                    r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}}"#,
                ),
            ),
            (
                "/ws/node_modules/pkg/package.json",
                Seed::file(r#"{"name": "pkg", "version": "1.0.0"}"#),
            ),
            (
                "/ws/node_modules/pkg/src/trace/index.ts",
                Seed::file("import { hrtime } from './trace';\nexport { hrtime };\n"),
            ),
            (
                "/ws/node_modules/pkg/src/trace/trace.ts",
                Seed::file("export function hrtime(): number { return 0; }\n"),
            ),
            (
                "/ws/node_modules/pkg/src/other.ts",
                Seed::file("export { createRef } from 'jsdep';\n"),
            ),
            (
                "/ws/node_modules/jsdep/package.json",
                Seed::file(r#"{"name": "jsdep", "version": "1.0.0", "main": "index.js"}"#),
            ),
            (
                "/ws/node_modules/jsdep/index.js",
                Seed::file("exports.createRef = function () {};\n"),
            ),
        ]
    };
    let with_index = |text: &'static str| {
        let mut entries = entries();
        entries.push(("/ws/index.ts", Seed::file(text)));
        entries
    };
    const HRT: &str = "import \"pkg/src/other\";\nhrt\n";
    assert_eq!(
        auto_import_items(with_index(HRT), HRT, (1, 3)),
        [(
            "hrtime".to_owned(),
            3,
            "pkg/src/trace".to_owned(),
            0,
            Value::Null
        )]
    );
    const CRE: &str = "import \"pkg/src/other\";\ncre\n";
    assert_eq!(
        auto_import_items(with_index(CRE), CRE, (1, 3)),
        [(
            "createRef".to_owned(),
            6,
            "pkg/src/other".to_owned(),
            0,
            Value::Null
        )]
    );
}

/// tsgo's alias resolver reads a module's file when its checker resolves
/// the module, so an export reached through a chain of re-exports
/// (`export *` twice, then `export { deepValue } from "dep"`) is found, and a
/// file only imported (`dep/unused`) is not read. tsc-rs's checker has its
/// files up front and is built again with the files it resolved modules to
/// until it resolved none it lacked. Pinned to tsgo's answer at aa814927.
#[test]
fn auto_imports_follow_a_chain_of_re_exports() {
    const TEXT: &str = "import \"pkg\";\ndeepV\n";
    let entries = vec![
        (
            "/ws/package.json",
            Seed::file(r#"{"name": "app", "dependencies": {"pkg": "1.0.0"}}"#),
        ),
        (
            "/ws/tsconfig.json",
            Seed::file(
                r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}}"#,
            ),
        ),
        (
            "/ws/node_modules/pkg/package.json",
            Seed::file(r#"{"name": "pkg", "version": "1.0.0", "types": "index.d.ts"}"#),
        ),
        (
            "/ws/node_modules/pkg/index.d.ts",
            Seed::file("export * from \"./a\";\n"),
        ),
        (
            "/ws/node_modules/pkg/a.d.ts",
            Seed::file("export * from \"./b\";\n"),
        ),
        (
            "/ws/node_modules/pkg/b.d.ts",
            Seed::file(
                "export { deepValue } from \"dep\";\nimport { unused } from \"dep/unused\";\n",
            ),
        ),
        (
            "/ws/node_modules/dep/package.json",
            Seed::file(r#"{"name": "dep", "version": "1.0.0", "types": "index.d.ts"}"#),
        ),
        (
            "/ws/node_modules/dep/index.d.ts",
            Seed::file("export declare const deepValue: number;\n"),
        ),
        (
            "/ws/node_modules/dep/unused.d.ts",
            Seed::file("export declare const unused: string;\n"),
        ),
        ("/ws/index.ts", Seed::file(TEXT)),
    ];
    assert_eq!(
        auto_import_items(entries, TEXT, (1, 5)),
        [("deepValue".to_owned(), 6, "pkg".to_owned(), 0, Value::Null)]
    );
}

/// tsgo fills an importing file's specifier cache (keyed by the file an
/// export was found in) for whichever completion group asks first, in Go's
/// map order. The `ContextVariableMap` augmentations of two middleware
/// files merge into one export whose path is the last file's and whose
/// module file is `src/index.ts`, so that file's own exports are named `..`
/// when the merged export asks before them: one of tsgo's orders in four
/// (2 of 8 runs at aa814927). tsc-rs asked in the order found, which was
/// that order every time; a file several groups reach is now filled from
/// the module file most of them give (found by the LSP test on hono).
#[test]
fn auto_imports_name_a_file_from_its_own_exports_despite_an_augmentation() {
    const TEXT: &str = "export function f(): void {\n  \n}\n";
    let middleware = |name: &str| {
        format!(
            "export const {name}One = 1\nexport const {name}Two = 2\nexport const {name}Three = 3\n\
             declare module '../..' {{\n  interface ContextVariableMap {{\n    {name}: number\n  }}\n}}\n"
        )
    };
    let entries = vec![
        (
            "/ws/tsconfig.json",
            Seed::file(
                r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}, "include": ["src"]}"#,
            ),
        ),
        (
            "/ws/src/context.ts",
            Seed::file("export interface ContextVariableMap {}\nexport class Context {}\n"),
        ),
        (
            "/ws/src/index.ts",
            Seed::file(
                "export type { ContextVariableMap } from './context'\nexport { Context } from './context'\n",
            ),
        ),
        (
            "/ws/src/middleware/alpha/index.ts",
            Seed::file(middleware("alpha")),
        ),
        (
            "/ws/src/middleware/beta/index.ts",
            Seed::file(middleware("beta")),
        ),
        ("/ws/src/helper/use.ts", Seed::file(TEXT)),
    ];
    let item = |label: &str, kind: i64, specifier: &str| {
        (label.to_owned(), kind, specifier.to_owned(), 0, Value::Null)
    };
    assert_eq!(
        auto_import_items_in("file:///ws/src/helper/use.ts", entries, TEXT, (1, 2)),
        [
            item("Context", 7, "../context"),
            item("alphaOne", 6, "../middleware/alpha"),
            item("alphaThree", 6, "../middleware/alpha"),
            item("alphaTwo", 6, "../middleware/alpha"),
            item("betaOne", 6, "../middleware/beta"),
            item("betaThree", 6, "../middleware/beta"),
            item("betaTwo", 6, "../middleware/beta"),
        ]
    );
}

/// An export the main file of a package and an addon's augmentation both
/// declare is one completion group, named by whichever export came later in
/// tsgo's index (Go's map order): the addon in 14 of 16 runs at aa814927,
/// which `import { on } from "cm/addon/mode/overlay"` then rejects (TS2305:
/// the addon exports nothing itself). The user decided (2026-10-16) that
/// where tsgo's random answers include an invalid import, tsc-rs gives the
/// valid one: the module file's own export keeps its path (found by the LSP
/// test on playwright's `@types/codemirror`).
#[test]
fn auto_imports_name_an_export_by_the_file_declaring_it() {
    const TEXT: &str = "export function f(): void {\n  \n}\n";
    let entries = vec![
        (
            "/ws/package.json",
            Seed::file(r#"{"name": "app", "dependencies": {"cm": "1.0.0"}}"#),
        ),
        (
            "/ws/tsconfig.json",
            Seed::file(
                r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}}"#,
            ),
        ),
        (
            "/ws/node_modules/cm/package.json",
            Seed::file(r#"{"name": "cm", "version": "1.0.0", "types": "index.d.ts"}"#),
        ),
        (
            "/ws/node_modules/cm/index.d.ts",
            Seed::file(
                "export function on(): void;\nexport function off(): void;\nexport function overlayMode(): void;\n",
            ),
        ),
        (
            "/ws/node_modules/cm/addon/mode/overlay.d.ts",
            Seed::file(
                "import \"../../\";\ndeclare module \"../../\" {\n    function overlayMode(): void;\n    function on(extra: number): void;\n}\n",
            ),
        ),
        (
            "/ws/other.ts",
            Seed::file("import \"cm/addon/mode/overlay\";\nexport const g = 1;\n"),
        ),
        ("/ws/index.ts", Seed::file(TEXT)),
    ];
    let item = |label: &str, kind: i64, specifier: &str| {
        (label.to_owned(), kind, specifier.to_owned(), 0, Value::Null)
    };
    assert_eq!(
        auto_import_items(entries, TEXT, (1, 2)),
        [
            item("g", 6, "./other"),
            item("off", 3, "cm"),
            item("on", 3, "cm"),
            item("overlayMode", 3, "cm"),
        ]
    );
}

/// A namespace declared in one project file and augmented from another
/// (`declare module "../Schema"`) is one completion group; tsgo names it by
/// the later of the two in its index (Go's map order): the augmenting file,
/// which does not export it, in about half of effect's sessions and in 1 of
/// 8 runs of this fixture at aa814927. The module file's own export keeps
/// its path (user decision of 2026-10-16; found by the LSP test on effect's
/// `Schema.Annotations`).
#[test]
fn auto_imports_name_an_augmented_namespace_by_its_declaring_file() {
    // A type position: the namespace holds types only.
    const TEXT: &str = "export function f(): void {\n  let x: \n}\n";
    let entries = vec![
        (
            "/ws/tsconfig.json",
            Seed::file(
                r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}}"#,
            ),
        ),
        (
            "/ws/Schema.ts",
            Seed::file(
                "export declare namespace Annotations {\n  interface Doc {\n    title?: string\n  }\n}\nexport const decode = 1\nexport const encode = 2\n",
            ),
        ),
        (
            "/ws/http/Api.ts",
            Seed::file(
                "import \"../Schema\"\ndeclare module \"../Schema\" {\n  namespace Annotations {\n    interface Doc {\n      status?: number\n    }\n  }\n}\nexport const handler = 1\nexport const route = 2\n",
            ),
        ),
        ("/ws/index.ts", Seed::file(TEXT)),
    ];
    let item = |label: &str, kind: i64, specifier: &str| {
        (label.to_owned(), kind, specifier.to_owned(), 0, Value::Null)
    };
    assert_eq!(
        auto_import_items(entries, TEXT, (1, 9)),
        [item("Annotations", 9, "./Schema")]
    );
}

/// tsgo answers a cached specifier as relative whatever kind computing it
/// gave, and ranks a `paths` kind against a relative one by directory
/// separators only, so the group that computes three packages' specifiers
/// first ranks them in a cycle with the penalized index re-export (`.`) and
/// the longer relative ones. That group is random in tsgo's map order, and
/// `version` (declared first in `core`'s index, re-exported by six files)
/// gets the three packages in 7 of 8 runs at aa814927; tsc-rs computed them
/// first for `version` every time and kept one (found by the LSP test on
/// vue).
#[test]
fn auto_imports_rank_a_shared_paths_specifier_as_cached() {
    const TEXT: &str = "export function f(): void {\n  \n}\n";
    let entries = vec![
        (
            "/ws/tsconfig.json",
            Seed::file(
                r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true, "paths": {"@v/*": ["./packages/*/src"]}}, "include": ["packages/*/src"]}"#,
            ),
        ),
        (
            "/ws/packages/core/src/index.ts",
            Seed::file(
                "export const version = \"1\"\nexport const alpha = 1\nexport const beta = 2\nexport const gamma = 3\nexport const delta = 4\n",
            ),
        ),
        ("/ws/packages/core/src/use.ts", Seed::file(TEXT)),
        ("/ws/packages/dom/src/index.ts", Seed::file("export * from '@v/core'\n")),
        ("/ws/packages/test/src/index.ts", Seed::file("export * from '@v/core'\n")),
        ("/ws/packages/app/src/index.ts", Seed::file("export * from '@v/dom'\n")),
        ("/ws/packages/app/src/runtime.ts", Seed::file("export * from '@v/dom'\n")),
        ("/ws/packages/compat/src/esm-index.ts", Seed::file("export * from '@v/dom'\n")),
        ("/ws/packages/compat/src/esm-runtime.ts", Seed::file("export * from '@v/dom'\n")),
    ];
    let expected: Vec<_> = ["alpha", "beta", "delta", "gamma", "version"]
        .iter()
        .flat_map(|name| {
            ["@v/app", "@v/dom", "@v/test"]
                .map(|package| (name.to_string(), 6, package.to_owned(), 0, Value::Null))
        })
        .collect();
    assert_eq!(
        auto_import_items_in("file:///ws/packages/core/src/use.ts", entries, TEXT, (1, 2)),
        expected
    );
}

/// tsgo's `getNodeModifiers` reads the modifiers of any declaration node
/// (`ast.IsDeclaration`: an export assignment included), so `export default`
/// under a JSDoc `@deprecated` makes its auto-import item deprecated (found
/// by the LSP test on next.js). Pinned to tsgo's answer at aa814927.
#[test]
fn a_deprecated_default_export_s_auto_import_is_deprecated() {
    const INDEX: &str = "export const x = 1;\noldV\n";
    let entries = vec![
        (
            "/ws/tsconfig.json",
            Seed::file(
                r#"{"compilerOptions": {"module": "esnext", "moduleResolution": "bundler", "noLib": true}}"#,
            ),
        ),
        (
            "/ws/oldValue.ts",
            Seed::file("/**\n * @deprecated Use the other one.\n */\nexport default 42;\n"),
        ),
        ("/ws/index.ts", Seed::file(INDEX)),
    ];
    assert_eq!(
        auto_import_items(entries, INDEX, (1, 4)),
        [(
            "oldValue".to_owned(),
            5,
            "./oldValue".to_owned(),
            1,
            serde_json::json!([1])
        )]
    );
}

/// Opens a JavaScript project of the given files (paths under
/// `/home/projects`) and returns the response to one request on `uri`.
fn request_in_javascript_project(
    files: &[(&str, &str)],
    uri: &str,
    method: &str,
    params: serde_json::Value,
) -> Value {
    let (mut session, mut client) = project_info_session(files);
    client.initialize(
        &mut session,
        serde_json::json!({"textDocument": {
            "documentSymbol": {"hierarchicalDocumentSymbolSupport": true},
            "semanticTokens": {
                "formats": ["relative"],
                "requests": {"full": true},
                "tokenModifiers": ["declaration", "definition", "readonly", "static", "deprecated", "abstract", "async", "modification", "documentation", "defaultLibrary", "local"],
                "tokenTypes": ["namespace", "class", "enum", "interface", "struct", "typeParameter", "type", "parameter", "variable", "property", "enumMember", "decorator", "event", "function", "method", "macro", "label", "comment", "string", "keyword", "number", "regexp", "operator"]
            }
        }}),
    );
    let path = uri.strip_prefix("file://").expect("a file URI");
    let text = files
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, text)| *text)
        .expect("the opened file");
    let language_id = if path.ends_with(".js") {
        "javascript"
    } else {
        "typescript"
    };
    client.notify(
        &mut session,
        "textDocument/didOpen",
        serde_json::json!({"textDocument": {"uri": uri, "languageId": language_id, "version": 0, "text": text}}),
    );
    let response = client.request(&mut session, 2, method, params);
    session.close();
    assert!(response.get("error").is_none(), "{response}");
    response
}

const JAVASCRIPT_PROJECT_CONFIG: &str = r#"{"compilerOptions": {"allowJs": true, "checkJs": true, "strict": true, "noEmit": true, "module": "esnext", "moduleResolution": "bundler", "target": "es2022", "types": []}}"#;

/// tsgo's document symbols of a JavaScript file take in what its reparser
/// makes of the comments' tags: an `@import`'s bindings and the members of
/// an inline `@typedef` type literal come before the typedef's own symbol
/// and the host (found by the LSP test on svelte; pinned to tsgo aa814927).
#[test]
fn document_symbols_of_a_javascript_file_s_jsdoc_tags() {
    let sym = "/** @import { A, B } from \"./types\" */\nimport { g } from \"./b.js\";\n\n/** @typedef {{ start?: number, end?: number }} NodeLike */\nexport class Parser {\n\tgenerate() {\n\t\treturn g(1);\n\t}\n}\n";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/sym.js", sym),
            (
                "/home/projects/b.js",
                "export function g(x) {\n\treturn x;\n}\n",
            ),
            (
                "/home/projects/types.d.ts",
                "export interface A { a: number }\nexport interface B { b: number }\n",
            ),
        ],
        "file:///home/projects/sym.js",
        "textDocument/documentSymbol",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/sym.js"}}),
    );
    fn flat(items: &[Value], depth: usize, acc: &mut Vec<String>) {
        for item in items {
            let range = &item["range"];
            acc.push(format!(
                "{}{} k{} {}:{}-{}:{}",
                "  ".repeat(depth),
                item["name"].as_str().unwrap(),
                item["kind"],
                range["start"]["line"],
                range["start"]["character"],
                range["end"]["line"],
                range["end"]["character"]
            ));
            if let Some(children) = item["children"].as_array() {
                flat(children, depth + 1, acc);
            }
        }
    }
    let mut lines = Vec::new();
    flat(
        response["result"].as_array().expect("symbols"),
        0,
        &mut lines,
    );
    assert_eq!(
        lines,
        [
            "A k13 0:14-0:15",
            "B k13 0:17-0:18",
            "start k7 3:16-3:31",
            "end k7 3:32-3:44",
            "NodeLike k5 3:4-3:56",
            "Parser k5 4:0-8:1",
            "  generate k6 5:1-7:2",
        ]
    );
}

/// A JavaScript function with a `@this` tag folds from its parameter list:
/// tsgo's reparser puts the `this` parameter at the tag's position, so the
/// list spans the lines down to the function's own parameters; a function
/// with the tag alone still folds from its brace (found by the LSP test on
/// svelte; pinned to tsgo aa814927).
#[test]
fn a_function_with_a_this_tag_folds_from_its_parameters() {
    let fold = "/**\n * @this {any}\n * @param {number} a\n * @param {number} b\n */\nexport function bubble(a, b) {\n\treturn this.x + a + b;\n}\n/**\n * @this {any}\n */\nexport function alone() {\n\treturn this.x;\n}\n";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/fold.js", fold),
        ],
        "file:///home/projects/fold.js",
        "textDocument/foldingRange",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/fold.js"}}),
    );
    let ranges = response["result"]
        .as_array()
        .expect("ranges")
        .iter()
        .filter(|range| range.get("kind").is_none())
        .map(|range| {
            (
                range["startLine"].as_u64().unwrap(),
                range["startCharacter"].as_u64().unwrap(),
                range["endLine"].as_u64().unwrap(),
                range["endCharacter"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(ranges, [(5, 22, 7, 1), (11, 23, 13, 1)]);
}

/// `noErrorTruncation` does not reach the signature of a deprecation
/// suggestion: tsgo's signatureToString builds its node without
/// NoTruncation whatever the option says, so a long signature is cut
/// (found by the LSP test on svelte; pinned to tsgo aa814927).
#[test]
fn a_deprecated_signature_s_message_is_cut_despite_no_error_truncation() {
    let member = "Record<string, Record<string, Record<string, number>>>";
    let dep = format!(
        "/** @deprecated */\nexport function create(options: {{ a: {member}; b: {member}; c: {member}; d: {member}; e: {member}; f: {member} }}): void;\nexport function create(): void;\nexport function create() {{}}\ncreate(0 as any);\n"
    );
    let response = request_in_javascript_project(
        &[
            (
                "/home/projects/tsconfig.json",
                r#"{"compilerOptions": {"strict": true, "noEmit": true, "noErrorTruncation": true, "types": [], "module": "esnext", "target": "es2022"}}"#,
            ),
            ("/home/projects/dep.ts", &dep),
        ],
        "file:///home/projects/dep.ts",
        "textDocument/diagnostic",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/dep.ts"}}),
    );
    let deprecated = response["result"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|item| item["code"] == 6387)
        .map(|item| item["message"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        deprecated,
        ["The signature '(options: { a: Record<string, Record<string, Record<string, number>>>; b: Record<string, Record<string, Record<string, number>>>; c: Record<...>; d: Record<...>; e: Record<...>; f: Record<...>; }): void' of 'create' is deprecated."]
    );
}

/// A JavaScript file's first relative import decides the ending of the
/// module specifiers its auto-imports get, and tsgo's reparse puts an
/// `@import` before the comment's host: `./types` (no extension) in front of
/// `./c.js` means no extension (found by the LSP test on svelte; tsgo
/// aa814927 names `./b`).
#[test]
fn a_jsdoc_import_decides_a_javascript_file_s_specifier_ending() {
    let a = "/** @import { A } from './types' */\nimport { g } from './c.js';\n\nexport const x = g(1) + CONST";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/a.js", a),
            ("/home/projects/b.js", "export const CONSTANT = 1;\n"),
            (
                "/home/projects/c.js",
                "export function g(x) {\n\treturn x;\n}\n",
            ),
            (
                "/home/projects/types.d.ts",
                "export interface A { a: number }\n",
            ),
        ],
        "file:///home/projects/a.js",
        "textDocument/completion",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/a.js"}, "position": {"line": 3, "character": 28}}),
    );
    let specifiers = response["result"]["items"]
        .as_array()
        .expect("the items")
        .iter()
        .filter(|item| item["label"] == "CONSTANT")
        .map(|item| item["data"]["autoImport"]["moduleSpecifier"].clone())
        .collect::<Vec<_>>();
    assert_eq!(specifiers, [serde_json::json!("./b")]);
}

/// The members of a `@typedef {Object}` with `@property` tags are properties
/// to the semantic tokens: tsgo's reparser makes property signatures of the
/// tags, and those are the members' declarations there (found by the LSP
/// test on svelte; pinned to tsgo aa814927).
#[test]
fn semantic_tokens_of_a_property_tag_typedef_s_members() {
    let a = "/**\n * @typedef {Object} Last\n * @property {string} tag\n * @property {number} depth\n */\n\nexport class Parser {\n\t/** @type {Last | undefined} */\n\tlast;\n\t/** @param {string} name */\n\tcheck(name) {\n\t\tif (!this.last || this.last.tag !== name) {\n\t\t\treturn 0;\n\t\t}\n\t\treturn this.last.depth;\n\t}\n}\n";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/a.js", a),
        ],
        "file:///home/projects/a.js",
        "textDocument/semanticTokens/full",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/a.js"}}),
    );
    let data = response["result"]["data"]
        .as_array()
        .expect("token data")
        .iter()
        .map(|value| value.as_u64().unwrap())
        .collect::<Vec<_>>();
    // (line, character, length, type, modifiers) of the tokens on the lines
    // reading the typedef's members.
    let mut tokens = Vec::new();
    let (mut line, mut character) = (0, 0);
    for token in data.chunks(5) {
        line += token[0];
        character = if token[0] == 0 {
            character + token[1]
        } else {
            token[1]
        };
        if line == 11 || line == 14 {
            tokens.push((line, character, token[2], token[3], token[4]));
        }
    }
    assert_eq!(
        tokens,
        [
            (11, 12, 4, 9, 0),
            (11, 25, 4, 9, 0),
            (11, 30, 3, 9, 0),
            (11, 38, 4, 7, 0),
            (14, 14, 4, 9, 0),
            (14, 19, 5, 9, 0),
        ]
    );
}

/// Signature help's JavaScript fallback (a call whose receiver has no call
/// signatures: tsgo's createJSSignatureHelpItems takes the first declaration
/// of the name with call signatures, lib files first) prefixes a signature
/// with the declaration's container as named from the declaring file's
/// scope: `IteratorObject.toArray`, not through the `declare global` block
/// declaring it, and nothing for a type literal's property (tsgo aa814927;
/// found by the LSP test on svelte).
#[test]
fn javascript_signature_help_fallback_prefixes_name_the_declaring_container() {
    let text = "export function f(x) {\n\treturn x.toArray();\n}\n/** @param {{ store: null; unsubscribe: Function }} entry */\nexport function g(entry) {\n\tentry.unsubscribe();\n}\n";
    let labels = |line: u32, character: u32| {
        let response = request_in_javascript_project(
            &[
                (
                    "/home/projects/tsconfig.json",
                    r#"{"compilerOptions": {"allowJs": true, "checkJs": true, "strict": true, "noEmit": true, "module": "esnext", "moduleResolution": "bundler", "target": "esnext", "lib": ["esnext", "dom"], "types": []}}"#,
                ),
                ("/home/projects/a.js", text),
            ],
            "file:///home/projects/a.js",
            "textDocument/signatureHelp",
            serde_json::json!({"textDocument": {"uri": "file:///home/projects/a.js"}, "position": {"line": line, "character": character}}),
        );
        response["result"]["signatures"]
            .as_array()
            .expect("signatures")
            .iter()
            .map(|signature| signature["label"].as_str().expect("a label").to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(labels(1, 18), ["IteratorObject.toArray(): T[]"]);
    assert_eq!(
        labels(5, 19),
        [
            "unsubscribe(subscriptions: CookieStoreGetOptions[]): Promise<void>",
            "unsubscribe(subscriptions: Iterable<CookieStoreGetOptions>): Promise<void>"
        ]
    );
}

/// A `@template` type parameter is not possibly referenced by a function
/// type written in the same comment's `@param` type literal: tsgo's reparser
/// moves the type parameter under the function, so the walk from the
/// function type reaches it and finds no reference, the property's type
/// stays the annotation's own type and the annotation is reused without
/// `| undefined` (tsgo aa814927; found by the LSP test on svelte's `render`).
#[test]
fn a_jsdoc_template_is_not_referenced_by_a_function_type_in_a_param_type_literal() {
    let index = "/**\n * @template {Record<string, any>} Props\n * @param {(props: Props) => void} component\n * @param {{ props?: Omit<Props, '$$slots'>; idPrefix?: string; transformError?: (error: unknown) => unknown | Promise<unknown> }} [options]\n * @returns {string}\n */\nexport function render(component, options = {}) {\n\treturn String(component) + String(options);\n}\n";
    let a = "import { render } from './index.js';\n\n/** @param {(props: Record<string, any>) => void} component */\nexport function go(component) {\n\treturn render(component, { idPrefix: 'x' });\n}\n";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/index.js", index),
            ("/home/projects/a.js", a),
        ],
        "file:///home/projects/a.js",
        "textDocument/signatureHelp",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/a.js"}, "position": {"line": 4, "character": 15}}),
    );
    let labels = response["result"]["signatures"]
        .as_array()
        .expect("signatures")
        .iter()
        .map(|signature| signature["label"].as_str().expect("a label").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        ["render(component: (props: Record<string, any>) => void, options?: { props?: Omit<Record<string, any>, \"$$slots\"> | undefined; idPrefix?: string; transformError?: (error: unknown) => unknown | Promise<unknown>; } | undefined): string"]
    );
}

/// tsgo's navigation bar walks a JavaScript file's reparsed JSDoc types: the
/// type of a `/** @type {T} */ (x)` cast (its `as` expression) after the
/// expression, also as a variable's initializer, and the `@param` types of a
/// function expression or arrow a variable is initialized with (its
/// parameters are children of the initializer); an inline type literal's
/// members are symbols of the enclosing declaration. A function
/// declaration's parameters are not walked (tsgo aa814927; found by the LSP
/// test on svelte).
#[test]
fn document_symbols_of_reparsed_jsdoc_type_literals() {
    let text = "/**\n * @template T\n * @param {T} value\n * @returns {T}\n */\nexport function clone(value) {\n\tif (typeof (/** @type {T & { toJSON?: any } } */ (value).toJSON) === 'function') {\n\t\treturn /** @type {T & { toJSON(): any } } */ (value).toJSON();\n\t}\n\tconst effect = /** @type {T & { nodes: string[] } } */ (value);\n\treturn effect;\n}\n\n/**\n * @param {string[]} comments\n * @param {{ preserve_whitespace: number }} state\n * @returns {number}\n */\nexport const visitors = (comments, state) => comments.length + state.preserve_whitespace;\n\n/**\n * @param {{ depth: number }} options\n */\nexport function walk(options) {\n\treturn options.depth;\n}\n";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/cast.js", text),
        ],
        "file:///home/projects/cast.js",
        "textDocument/documentSymbol",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/cast.js"}}),
    );
    fn flat(items: &[Value], depth: usize, acc: &mut Vec<String>) {
        for item in items {
            let range = &item["range"];
            acc.push(format!(
                "{}{} k{} {}:{}-{}:{}",
                "  ".repeat(depth),
                item["name"].as_str().unwrap(),
                item["kind"],
                range["start"]["line"],
                range["start"]["character"],
                range["end"]["line"],
                range["end"]["character"]
            ));
            if let Some(children) = item["children"].as_array() {
                flat(children, depth + 1, acc);
            }
        }
    }
    let mut lines = Vec::new();
    flat(
        response["result"].as_array().expect("symbols"),
        0,
        &mut lines,
    );
    assert_eq!(
        lines,
        [
            "clone k12 5:0-11:1",
            "  toJSON k7 6:30-6:42",
            "  toJSON k6 7:26-7:39",
            "  effect k13 9:7-9:63",
            "    nodes k7 9:33-9:48",
            "visitors k13 18:13-18:88",
            "  preserve_whitespace k7 15:13-15:40",
            "walk k12 23:0-25:1",
        ]
    );
}

/// A TypeScript file's declaration is deprecated for completions by tsgo's
/// scanner text scan of its comment, a `@deprecated` followed by whitespace,
/// `}`, `*` or the comment's end (the JSDoc itself is parsed lazily):
/// `@deprecated:` marks nothing (tsgo aa814927; found by the LSP test on
/// SolidJS).
#[test]
fn a_deprecated_tag_followed_by_a_colon_does_not_deprecate_a_typescript_declaration() {
    let lib = "/** @deprecated: use `other` instead */\nexport function fnColon() {}\n/** @deprecated use `other` instead */\nexport function fnPlain() {}\n";
    let a = "import { fnColon, fnPlain } from './lib.js';\nexport function use(foo) {\n\tfnColon();\n\tfnPlain();\n\treturn foo({ ...fn });\n}\n";
    let response = request_in_javascript_project(
        &[
            ("/home/projects/tsconfig.json", JAVASCRIPT_PROJECT_CONFIG),
            ("/home/projects/lib.ts", lib),
            ("/home/projects/a.js", a),
        ],
        "file:///home/projects/a.js",
        "textDocument/completion",
        serde_json::json!({"textDocument": {"uri": "file:///home/projects/a.js"}, "position": {"line": 4, "character": 18}, "context": {"triggerKind": 1}}),
    );
    let mut tags = response["result"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|item| {
            item["label"]
                .as_str()
                .is_some_and(|label| label.starts_with("fn"))
        })
        .map(|item| {
            (
                item["label"].as_str().unwrap().to_owned(),
                item["tags"].clone(),
            )
        })
        .collect::<Vec<_>>();
    tags.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        tags,
        [
            ("fnColon".to_owned(), serde_json::Value::Null),
            ("fnPlain".to_owned(), serde_json::json!([1])),
        ]
    );
}
