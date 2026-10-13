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
use tsc_host::ipc::jsonrpc::{Reader, Writer};
use tsc_lsp::lsproto::message::RawJson;
use tsc_lsp::lsproto::{self, Id, Message, ResponseMessage};
use tsc_lsp::{Server, ServerOptions};

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
        let input_pipe = Pipe::default();
        let output_pipe = Pipe::default();
        let stderr = Pipe::default();
        let server = Server::new(ServerOptions {
            stderr: Box::new(stderr.clone()),
            cwd: "/".to_owned(),
            set_parent_process_id: None,
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
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let script: Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures.join(format!("{name}.json"))).unwrap(),
    )
    .unwrap();
    let expected = std::fs::read_to_string(fixtures.join(format!("{name}.tsgo.txt"))).unwrap();
    let answers = script["answers"].as_object().cloned().unwrap_or_default();
    let mut session = Session::start();
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
    assert_eq!(lines.join("\n"), expected.trim_end());
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
#[test]
fn errors_match_tsgo() {
    replay("errors");
}

/// `exit` ends the server with tsgo's `context canceled` (the command's
/// status 1).
#[test]
fn exit_stops_the_server() {
    let mut session = Session::start();
    session.send(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"processId":null,"rootUri":null,"capabilities":{}}}"#);
    session.send(r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#);
    while let Some(text) = session.receive() {
        let message: Value = serde_json::from_str(&text).unwrap();
        if message.get("method") == Some(&Value::from("client/registerCapability")) {
            session.send(&format!(
                r#"{{"jsonrpc":"2.0","id":{},"result":null}}"#,
                message["id"]
            ));
            break;
        }
    }
    session.server.wait_init_complete();
    session.send(r#"{"jsonrpc":"2.0","method":"exit"}"#);
    assert_eq!(session.finish(), Err("context canceled".to_owned()));
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
