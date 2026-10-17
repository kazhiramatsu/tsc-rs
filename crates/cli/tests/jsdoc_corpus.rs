//! The JSDoc corpus (tests/jsdoc-corpus/README.md): every case's command-line
//! output and language-server answers against the baselines recorded from
//! tsgo by scripts/jsdoc_corpus.py. A difference whose id `known.txt` does
//! not list fails, and so does a listed id whose answer now matches (the
//! ratchet moves only forward).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Map, Value};

const ENCODING: &str = "utf-16";
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(120);

fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/jsdoc-corpus")
        .canonicalize()
        .expect("tests/jsdoc-corpus")
}

/// The arrays tsgo does not order, per method (the recorder's UNORDERED).
fn unordered(method: &str) -> &'static [&'static [&'static str]] {
    match method {
        "textDocument/completion" => &[&["items"]],
        "textDocument/references"
        | "textDocument/implementation"
        | "callHierarchy/incomingCalls"
        | "textDocument/definition"
        | "textDocument/typeDefinition"
        | "textDocument/documentHighlight" => &[&[]],
        "textDocument/rename" | "workspace/willRenameFiles" => {
            &[&["documentChanges"], &["changes", "*"]]
        }
        "codeLens/resolve" => &[&["command", "arguments", "2"]],
        _ => &[],
    }
}

/// Canonical JSON: keys sorted, compact separators, non-ASCII kept (Python's
/// `json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False)`).
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<&String, &Value> = map.iter().collect();
            out.push('{');
            for (index, (key, item)) in sorted.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("a key"));
                out.push(':');
                canonical(item, out);
            }
            out.push('}');
        }
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
        other => out.push_str(&serde_json::to_string(other).expect("a scalar")),
    }
}

fn canonical_string(value: &Value) -> String {
    let mut out = String::new();
    canonical(value, &mut out);
    out
}

fn sort_arrays(value: Value, path: &[&str]) -> Value {
    match path.split_first() {
        None => match value {
            Value::Array(mut items) => {
                items.sort_by_cached_key(canonical_string);
                Value::Array(items)
            }
            other => other,
        },
        Some((&"*", rest)) => match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(key, item)| (key, sort_arrays(item, rest)))
                    .collect::<Map<_, _>>(),
            ),
            other => other,
        },
        Some((part, rest)) => match value {
            Value::Object(mut map) => {
                if let Some(item) = map.remove(*part) {
                    map.insert((*part).to_owned(), sort_arrays(item, rest));
                }
                Value::Object(map)
            }
            Value::Array(items) => {
                let index: Option<usize> = part.parse().ok();
                Value::Array(
                    items
                        .into_iter()
                        .enumerate()
                        .map(|(i, item)| {
                            if Some(i) == index {
                                sort_arrays(item, rest)
                            } else {
                                item
                            }
                        })
                        .collect(),
                )
            }
            other => other,
        },
    }
}

fn normalize(method: &str, response: &Value, root: &str, root_uri: &str) -> String {
    let normalized = if let Some(error) = response.get("error") {
        json!({"error": error.get("message").cloned().unwrap_or(Value::Null)})
    } else {
        let mut result = response.get("result").cloned().unwrap_or(Value::Null);
        for path in unordered(method) {
            result = sort_arrays(result, path);
        }
        json!({"result": result})
    };
    canonical_string(&normalized)
        .replace(root_uri, "file://${ROOT}")
        .replace(root, "${ROOT}")
}

fn file_uri(path: &Path) -> String {
    // The recorder's `quote`: the corpus paths contain no character it escapes.
    format!("file://{}", path.to_string_lossy())
}

fn language_id(name: &str) -> &'static str {
    if name.ends_with(".js")
        || name.ends_with(".cjs")
        || name.ends_with(".mjs")
        || name.ends_with(".jsx")
    {
        "javascript"
    } else if name.ends_with(".json") {
        "json"
    } else {
        "typescript"
    }
}

/// tsc-rs's language server over stdio, driven as the recorder drives one:
/// configuration answered with nulls, every other server request with null.
struct Server {
    child: Child,
    stdin: ChildStdin,
    messages: mpsc::Receiver<Value>,
    next_id: i64,
}

impl Server {
    fn start(root: &Path) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
            .args(["--lsp", "--stdio"])
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("run tsc-rs --lsp --stdio");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || read_messages(stdout, sender));
        Server {
            child,
            stdin,
            messages,
            next_id: 1,
        }
    }

    fn send(&mut self, message: &Value) {
        let body = serde_json::to_string(message).expect("a message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
            .expect("write to the server");
        self.stdin.flush().expect("flush");
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self
                .messages
                .recv_timeout(RESPONSE_TIMEOUT)
                .unwrap_or_else(|_| {
                    panic!("no answer to {method} (request {id}) within {RESPONSE_TIMEOUT:?}")
                });
            if let Some(server_method) = message.get("method").and_then(Value::as_str) {
                if let Some(request_id) = message.get("id").cloned() {
                    let result = if server_method == "workspace/configuration" {
                        let count = message["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; count])
                    } else {
                        Value::Null
                    };
                    self.send(&json!({"jsonrpc": "2.0", "id": request_id, "result": result}));
                }
                continue;
            }
            if message.get("id") == Some(&json!(id)) {
                return message;
            }
        }
    }

    fn close(mut self) {
        self.send(&json!({"jsonrpc": "2.0", "id": self.next_id, "method": "shutdown"}));
        self.send(&json!({"jsonrpc": "2.0", "method": "exit"}));
        drop(self.stdin);
        let _ = self.child.wait();
    }
}

fn read_messages(stdout: ChildStdout, sender: mpsc::Sender<Value>) {
    let mut reader = BufReader::new(stdout);
    loop {
        let mut length: Option<usize> = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return;
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    length = value.trim().parse().ok();
                }
            }
        }
        let Some(length) = length else { return };
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        match serde_json::from_slice::<Value>(&body) {
            Ok(message) => {
                if sender.send(message).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

struct Request {
    id: String,
    file: String,
    method: String,
    params: Map<String, Value>,
}

fn read_requests(case: &Path) -> Vec<Request> {
    let text = std::fs::read_to_string(case.join("requests.jsonl")).expect("requests.jsonl");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("a request line");
            Request {
                id: value["id"].as_str().expect("id").to_owned(),
                file: value["file"].as_str().expect("file").to_owned(),
                method: value["method"].as_str().expect("method").to_owned(),
                params: value
                    .get("params")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default(),
            }
        })
        .collect()
}

fn read_expected(case: &Path) -> BTreeMap<String, String> {
    let text =
        std::fs::read_to_string(case.join("expected/lsp.jsonl")).expect("expected/lsp.jsonl");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: Value = serde_json::from_str(line).expect("an expected line");
            (
                value["id"].as_str().expect("id").to_owned(),
                value["answer"].as_str().expect("answer").to_owned(),
            )
        })
        .collect()
}

fn read_known(case: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(case.join("known.txt"))
        .map(|text| {
            text.lines()
                .map(|line| line.split('#').next().unwrap_or("").trim().to_owned())
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// `tsc -p . --pretty false` and every request of the case.
fn run_case(case: &Path) -> (String, BTreeMap<String, String>) {
    let root = case.to_string_lossy().to_string();
    let root_uri = file_uri(case);
    let cli = Command::new(env!("CARGO_BIN_EXE_tsc-rs"))
        .args(["-p", ".", "--pretty", "false"])
        .current_dir(case)
        .output()
        .expect("run tsc-rs -p .");
    let mut cli_text = format!(
        "{}{}",
        String::from_utf8_lossy(&cli.stdout),
        String::from_utf8_lossy(&cli.stderr)
    )
    .replace("\r\n", "\n")
    .replace(&root, "${ROOT}");
    while cli_text.ends_with('\n') {
        cli_text.pop();
    }
    cli_text.push_str(&format!("\nexit {}\n", cli.status.code().unwrap_or(-1)));

    let mut initialize: Value = serde_json::from_str(
        &std::fs::read_to_string(corpus_root().join("initialize.json")).expect("initialize.json"),
    )
    .expect("initialize.json parses");
    initialize["capabilities"]["general"]["positionEncodings"] = json!([ENCODING]);
    initialize["capabilities"]["workspace"]["workspaceFolders"] = json!(true);
    initialize["rootUri"] = json!(root_uri);
    initialize["workspaceFolders"] =
        json!([{"uri": root_uri, "name": case.file_name().unwrap().to_string_lossy()}]);
    initialize["processId"] = json!(std::process::id());

    let mut server = Server::start(case);
    server.request("initialize", initialize);
    server.notify("initialized", json!({}));
    let mut opened = BTreeSet::new();
    let mut answers = BTreeMap::new();
    for request in read_requests(case) {
        let path = case.join(&request.file);
        let uri = file_uri(&path);
        if opened.insert(request.file.clone()) {
            let bytes = std::fs::read(&path).expect("the request's file");
            let text = String::from_utf8_lossy(&bytes);
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned();
            server.notify(
                "textDocument/didOpen",
                json!({"textDocument": {"uri": uri, "languageId": language_id(&request.file), "version": 1, "text": text}}),
            );
        }
        let mut params = request.params.clone();
        params
            .entry("textDocument".to_owned())
            .or_insert_with(|| json!({"uri": uri}));
        let response = server.request(&request.method, Value::Object(params));
        answers.insert(
            request.id.clone(),
            normalize(&request.method, &response, &root, &root_uri),
        );
    }
    server.close();
    (cli_text, answers)
}

#[test]
fn every_case_answers_as_tsgo_did() {
    let corpus = corpus_root();
    let only = std::env::var("JSDOC_CORPUS_CASE").ok();
    let dump = std::env::var("JSDOC_CORPUS_DUMP").ok().map(PathBuf::from);
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .expect("the corpus")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join("requests.jsonl").is_file())
        .filter(|path| {
            only.as_deref()
                .is_none_or(|name| path.file_name().unwrap() == name)
        })
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "no corpus case under {}",
        corpus.display()
    );

    let mut report = String::new();
    let mut failed = 0;
    let mut answered = 0;
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let expected_cli =
            std::fs::read_to_string(case.join("expected/cli.txt")).expect("expected/cli.txt");
        let expected = read_expected(case);
        let known = read_known(case);
        let (cli_text, answers) = run_case(case);
        answered += answers.len();
        let mut differing: Vec<String> = Vec::new();
        if cli_text != expected_cli {
            differing.push("cli".to_owned());
        }
        for (id, answer) in &answers {
            if expected.get(id) != Some(answer) {
                differing.push(id.clone());
            }
        }
        let unexpected: Vec<&String> = differing.iter().filter(|id| !known.contains(*id)).collect();
        let stale: Vec<&String> = known.iter().filter(|id| !differing.contains(id)).collect();
        if let Some(dump) = &dump {
            let dir = dump.join(&name);
            std::fs::create_dir_all(&dir).expect("dump dir");
            std::fs::write(dir.join("cli.txt"), &cli_text).expect("dump cli");
            let lines: Vec<String> = answers
                .iter()
                .map(|(id, answer)| {
                    serde_json::to_string(&json!({"id": id, "answer": answer})).expect("a line")
                })
                .collect();
            std::fs::write(dir.join("lsp.jsonl"), lines.join("\n") + "\n").expect("dump lsp");
        }
        if unexpected.is_empty() && stale.is_empty() {
            continue;
        }
        failed += 1;
        report.push_str(&format!(
            "\n{name}: {} answers, {} differ ({} known)\n",
            answers.len(),
            differing.len(),
            known.len()
        ));
        for id in unexpected {
            let (want, got) = if id == "cli" {
                (
                    expected_cli.replace('\n', " | "),
                    cli_text.replace('\n', " | "),
                )
            } else {
                (
                    expected
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| "(no baseline)".to_owned()),
                    answers[id].clone(),
                )
            };
            report.push_str(&format!(
                "  differs: {id}\n    tsgo  : {}\n    tsc-rs: {}\n",
                truncate(&want, 400),
                truncate(&got, 400)
            ));
        }
        for id in stale {
            report.push_str(&format!("  now matches, remove from known.txt: {id}\n"));
        }
    }
    assert!(
        failed == 0,
        "{failed} of {} corpus cases differ from tsgo beyond known.txt ({answered} answers):{report}",
        cases.len()
    );
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_owned()
    } else {
        let head: String = text.chars().take(limit).collect();
        format!("{head}…")
    }
}
