//! The language server's transcripts (tsgo `internal/fourslash` through
//! `internal/testutil/lsptestutil`): `scripts/lsp_transcripts.py` records
//! what tsgo's fourslash harness and its in-process server send each other,
//! and keeps in the profile's `fourslash-transcripts.jsonl.gz`, per harness,
//! the files, the inferred projects' options, the client's messages and the
//! harness's file operations in order, its answers to
//! `workspace/configuration`, and the digest of each response.
//!
//! A scenario runs tsc-rs's server in process over the same files: each
//! message goes to it in order, a request waits for its response as the
//! harness's do, the server's requests get the harness's answers
//! (`handleServerRequest`), and every response's digest must be tsgo's.

use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_host::ipc::jsonrpc::{Reader, Writer};
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsp::{Server, ServerOptions};
use tsc_program::go_json::compiler_options_bag;

/// How long a request may take before the scenario fails.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(60);

/// One recorded harness.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Scenario {
    /// The Go test's name (`TestX`, `TestX/subtest`, `#2` for a second
    /// harness).
    pub(super) name: String,
    case_sensitive: bool,
    /// Each file's text, or `{"symlink": target}`.
    files: Map<String, Value>,
    inferred_options: Map<String, Value>,
    steps: Vec<Step>,
    /// The harness's answers to `workspace/configuration`, in order.
    configuration: Vec<Value>,
}

/// A client message (with the digest of tsgo's response to a request), or
/// a file the harness wrote or removed.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Step {
    Send {
        send: Map<String, Value>,
        #[serde(default)]
        expect: Option<String>,
    },
    Write {
        write: String,
        text: Value,
    },
    Remove {
        remove: String,
    },
}

/// The recorded scenarios of the profile.
pub(super) fn cases(workspace: &Path, profile: &str) -> Result<Vec<Scenario>, String> {
    let path = workspace
        .join("vendor/typescript-native")
        .join(profile)
        .join("fourslash-transcripts.jsonl.gz");
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let text = gunzip(&bytes).ok_or_else(|| format!("{}: not a gzip member", path.display()))?;
    let text = String::from_utf8(text).map_err(|error| format!("{}: {error}", path.display()))?;
    text.lines()
        .map(|line| {
            serde_json::from_str(line).map_err(|error| format!("{}: {error}", path.display()))
        })
        .collect()
}

/// The data of a gzip member (RFC 1952).
fn gunzip(bytes: &[u8]) -> Option<Vec<u8>> {
    const FHCRC: u8 = 1 << 1;
    const FEXTRA: u8 = 1 << 2;
    const FNAME: u8 = 1 << 3;
    const FCOMMENT: u8 = 1 << 4;
    if bytes.len() < 18 || bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 8 {
        return None;
    }
    let flags = bytes[3];
    let mut offset = 10;
    if flags & FEXTRA != 0 {
        let length = usize::from(*bytes.get(offset)?) | usize::from(*bytes.get(offset + 1)?) << 8;
        offset += 2 + length;
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            offset += bytes.get(offset..)?.iter().position(|&byte| byte == 0)? + 1;
        }
    }
    if flags & FHCRC != 0 {
        offset += 2;
    }
    miniz_oxide::inflate::decompress_to_vec(bytes.get(offset..bytes.len() - 8)?).ok()
}

/// How a scenario's responses compare with tsgo's.
pub(super) struct Replay {
    pub(super) requests: usize,
    /// The requests whose responses differ, as `<method> (<id>)`.
    pub(super) differing: Vec<String>,
    /// By request method, the responses with tsgo's digest and all.
    pub(super) methods: BTreeMap<String, [usize; 2]>,
    /// Each request and tsc-rs's response, for `--dump`.
    pub(super) transcript: String,
}

/// Runs a scenario against tsc-rs's server.
pub(super) fn replay(scenario: &Scenario) -> Result<Replay, String> {
    let fs = Arc::new(seeded_fs(scenario)?);
    let input = Pipe::default();
    let output = Pipe::default();
    let server = Server::new(ServerOptions {
        stderr: Box::new(io::sink()),
        cwd: "/".to_owned(),
        set_parent_process_id: None,
        fs: Arc::new(BundledFs::new(Arc::clone(&fs))),
        default_library_path: EMBEDDED_LIBRARY_DIRECTORY.to_owned(),
        // tsgo's fourslash content mapper tests: the test mappers.
        spawner: Some(Arc::new(
            tsc_harness::content_mappers::TestSpawner::default(),
        )),
    });
    server.set_compiler_options_for_inferred_projects(compiler_options_bag(
        &scenario.inferred_options,
    )?);
    let run = {
        let server = Arc::clone(&server);
        let input = input.clone();
        let output = output.clone();
        std::thread::spawn(move || {
            let result = server.run(Box::new(input), Box::new(output.clone()));
            output.close();
            result
        })
    };
    let mut client = Client {
        writer: Writer::new(input.clone()),
        reader: Reader::new(output),
        preferences: initial_preferences(scenario),
    };
    let result = client.replay(scenario, &fs);
    // tsgo's harness ends by closing the client's stream. Its session stays
    // until the garbage collector takes it; here it closes, so its programs
    // and timers go now.
    input.close();
    if result.is_ok() {
        let _ = run.join();
    }
    if let Some(session) = server.session() {
        session.close();
    }
    result
}

/// The harness's files (tsgo `vfstest.FromMap`).
fn seeded_fs(scenario: &Scenario) -> Result<MemFs, String> {
    let entries = scenario
        .files
        .iter()
        .map(|(path, file)| match file {
            Value::String(text) => Ok((path.clone(), Seed::file(text))),
            Value::Object(object) => match object.get("symlink") {
                Some(Value::String(target)) => Ok((path.clone(), Seed::symlink(target.clone()))),
                _ => Err(format!("{path}: an unsupported file {file}")),
            },
            _ => Err(format!("{path}: an unsupported file {file}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    MemFs::from_entries(entries, scenario.case_sensitive, Arc::new(SystemClock))
        .map_err(|error| format!("the files: {error}"))
}

/// The preferences the harness answers for `js/ts`: those of its first
/// answer (tsgo's defaults).
fn initial_preferences(scenario: &Scenario) -> Value {
    match scenario.configuration.first() {
        Some(Value::Array(sections)) => sections.first().cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// The harness's end of the streams.
struct Client {
    writer: Writer<Pipe>,
    reader: Reader<Pipe>,
    /// tsgo `FourslashTest.userPreferences`: the latest `js/ts` settings the
    /// harness sent.
    preferences: Value,
}

impl Client {
    fn replay(&mut self, scenario: &Scenario, fs: &MemFs) -> Result<Replay, String> {
        let mut replay = Replay {
            requests: 0,
            differing: Vec::new(),
            methods: BTreeMap::new(),
            transcript: String::new(),
        };
        for step in &scenario.steps {
            match step {
                Step::Write { write, text } => {
                    let Value::String(text) = text else {
                        return Err(format!("{write}: unsupported text {text}"));
                    };
                    fs.write_creating_dirs(write, text.as_bytes())
                        .map_err(|error| format!("write {write}: {error}"))?;
                }
                Step::Remove { remove } => {
                    fs.remove(remove)
                        .map_err(|error| format!("remove {remove}: {error}"))?;
                }
                Step::Send { send, expect } => {
                    let method = send
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned();
                    if method == "workspace/didChangeConfiguration" {
                        if let Some(settings) = send
                            .get("params")
                            .and_then(|params| params.get("settings"))
                            .and_then(|settings| settings.get("js/ts"))
                        {
                            self.preferences = settings.clone();
                        }
                    }
                    self.write(&Value::Object(send.clone()))?;
                    let Some(id) = send.get("id") else {
                        continue;
                    };
                    replay.requests += 1;
                    let response = self.response(id, &method)?;
                    let actual = digest(&method, &response);
                    let _ = writeln!(replay.transcript, "{}", Value::Object(send.clone()));
                    let _ = writeln!(replay.transcript, "{response}");
                    let same = expect.as_deref() == Some(actual.as_str());
                    let counts = replay.methods.entry(method.clone()).or_default();
                    counts[0] += usize::from(same);
                    counts[1] += 1;
                    if !same {
                        replay.differing.push(format!("{method} ({id})"));
                    }
                }
            }
        }
        Ok(replay)
    }

    fn write(&mut self, message: &Value) -> Result<(), String> {
        self.writer
            .write(message.to_string().as_bytes())
            .map_err(|error| format!("write to the server: {error}"))
    }

    /// The response to request `id`, the server's requests in between
    /// answered as the harness answers them.
    fn response(&mut self, id: &Value, method: &str) -> Result<Value, String> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        loop {
            if Instant::now() > deadline {
                return Err(format!("no response to {method} ({id})"));
            }
            let message = match self.reader.read() {
                Ok(Some(message)) => message,
                Ok(None) => {
                    return Err(format!("the server ended before answering {method} ({id})"))
                }
                Err(error) => return Err(format!("{method} ({id}): {error}")),
            };
            let message: Value = serde_json::from_slice(&message)
                .map_err(|error| format!("a message from the server: {error}"))?;
            match (message.get("method"), message.get("id")) {
                (Some(server_method), Some(server_id)) => {
                    let answer = self.answer(server_method.as_str().unwrap_or_default(), &message);
                    let mut response = Map::new();
                    response.insert("jsonrpc".to_owned(), Value::from("2.0"));
                    response.insert("id".to_owned(), server_id.clone());
                    match answer {
                        Ok(result) => response.insert("result".to_owned(), result),
                        Err(error) => response.insert("error".to_owned(), error),
                    };
                    self.write(&Value::Object(response))?;
                }
                (None, Some(response_id)) if response_id == id => return Ok(message),
                _ => {}
            }
        }
    }

    /// tsgo `FourslashTest.handleServerRequest`.
    fn answer(&self, method: &str, request: &Value) -> Result<Value, Value> {
        match method {
            "workspace/configuration" => {
                let items = request
                    .get("params")
                    .and_then(|params| params.get("items"))
                    .and_then(Value::as_array);
                Ok(match items {
                    Some(items) => Value::Array(
                        items
                            .iter()
                            .map(|item| {
                                if item.get("section").and_then(Value::as_str) == Some("js/ts") {
                                    self.preferences.clone()
                                } else {
                                    Value::Null
                                }
                            })
                            .collect(),
                    ),
                    None => Value::Array(vec![self.preferences.clone()]),
                })
            }
            "client/registerCapability" | "client/unregisterCapability" => Ok(Value::Null),
            _ => Err(serde_json::json!({
                "code": -32601,
                "message": format!("Unknown method: {method}"),
            })),
        }
    }
}

/// A part of a path from a response's result (`scripts/lsp_transcripts.py`
/// `UNORDERED`).
#[derive(Clone, Copy)]
enum Part {
    Key(&'static str),
    /// Each value of an object.
    Each,
    Index(usize),
}

/// The arrays of a response whose order tsgo does not keep, by request
/// method: completions come from symbol tables (Go maps), and the searches
/// over several projects (`crossproject.go`) combine the projects' results
/// in a sync map's order. Their digests take them sorted.
const UNORDERED: &[(&str, &[&[Part]])] = &[
    ("textDocument/completion", &[&[Part::Key("items")]]),
    ("textDocument/references", &[&[]]),
    ("textDocument/implementation", &[&[]]),
    (
        "textDocument/rename",
        &[
            &[Part::Key("documentChanges")],
            &[Part::Key("changes"), Part::Each],
        ],
    ),
    ("callHierarchy/incomingCalls", &[&[]]),
    (
        "codeLens/resolve",
        &[&[Part::Key("command"), Part::Key("arguments"), Part::Index(2)]],
    ),
    (
        "workspace/willRenameFiles",
        &[
            &[Part::Key("documentChanges")],
            &[Part::Key("changes"), Part::Each],
        ],
    ),
];

/// The script's `digest`: the first 16 hex digits of the SHA-256 of the
/// canonical JSON of the response's result or error.
pub(super) fn digest(method: &str, response: &Value) -> String {
    let mut value = Map::new();
    for key in ["result", "error"] {
        if let Some(part) = response.get(key) {
            value.insert(key.to_owned(), part.clone());
        }
    }
    if let Some(result) = value.get_mut("result") {
        let paths = UNORDERED
            .iter()
            .find(|(unordered, _)| *unordered == method)
            .map_or(&[][..], |(_, paths)| *paths);
        for path in paths {
            sort_arrays(result, path);
        }
    }
    let mut text = String::new();
    canonical(&Value::Object(value), &mut text);
    Sha256::digest(text.as_bytes())
        .iter()
        .take(8)
        .fold(String::new(), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// The script's `sort_arrays`: the arrays at `path` sorted by their
/// elements' canonical JSON.
fn sort_arrays(value: &mut Value, path: &[Part]) {
    let Some((part, rest)) = path.split_first() else {
        if let Value::Array(items) = value {
            let mut keyed = std::mem::take(items)
                .into_iter()
                .map(|item| {
                    let mut text = String::new();
                    canonical(&item, &mut text);
                    (text, item)
                })
                .collect::<Vec<_>>();
            keyed.sort_by(|left, right| left.0.cmp(&right.0));
            *items = keyed.into_iter().map(|(_, item)| item).collect();
        }
        return;
    };
    match (part, value) {
        (Part::Each, Value::Object(object)) => {
            for item in object.values_mut() {
                sort_arrays(item, rest);
            }
        }
        (Part::Key(key), Value::Object(object)) => {
            if let Some(item) = object.get_mut(*key) {
                sort_arrays(item, rest);
            }
        }
        (Part::Index(index), Value::Array(items)) => {
            if let Some(item) = items.get_mut(*index) {
                sort_arrays(item, rest);
            }
        }
        _ => {}
    }
}

/// Python's `json.dumps(value, sort_keys=True, separators=(",", ":"),
/// ensure_ascii=False)`.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Value::Number(number) => {
            let _ = write!(out, "{number}");
        }
        Value::String(text) => quote(text, out),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                quote(key, out);
                out.push(':');
                canonical(&object[key], out);
            }
            out.push('}');
        }
    }
}

fn quote(text: &str, out: &mut String) {
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            character if u32::from(character) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(character));
            }
            character => out.push(character),
        }
    }
    out.push('"');
}

/// One direction of an in-memory stream: reads wait for data until the
/// writer closes it.
#[derive(Clone, Default)]
struct Pipe(Arc<(Mutex<PipeState>, Condvar)>);

/// The bytes not read yet, and whether the writer closed the stream.
type PipeState = (VecDeque<u8>, bool);

impl Pipe {
    fn state(&self) -> MutexGuard<'_, PipeState> {
        self.0 .0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn close(&self) {
        self.state().1 = true;
        self.0 .1.notify_all();
    }
}

impl Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + RESPONSE_TIMEOUT;
        let mut state = self.state();
        while state.0.is_empty() && !state.1 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "no data"));
            }
            state = self
                .0
                 .1
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        let count = buffer.len().min(state.0.len());
        for (slot, byte) in buffer.iter_mut().zip(state.0.drain(..count)) {
            *slot = byte;
        }
        Ok(count)
    }
}

impl Write for Pipe {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.state().0.extend(data);
        self.0 .1.notify_all();
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digests_match_the_recorder() {
        // Values from scripts/lsp_transcripts.py's digest.
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {"b": [3, 1], "a": "x\u{1}\"\\\n\u{e9}"},
        });
        assert_eq!(digest("textDocument/hover", &response), "398dafd6e1b2865b");
        let completion = serde_json::json!({
            "id": 1,
            "result": {"isIncomplete": false, "items": [{"label": "b"}, {"label": "a"}]},
        });
        assert_eq!(
            digest("textDocument/completion", &completion),
            "98080c9bc4200951"
        );
    }
}
