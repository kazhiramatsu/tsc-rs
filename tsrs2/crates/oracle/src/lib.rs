#![forbid(unsafe_code)]

use std::error::Error;
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleRequest {
    pub program_json_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleError {
    pub message: String,
}

impl OracleError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for OracleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for OracleError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OracleDiag {
    pub file: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub code: u32,
    /// Which tsc pass produced the diagnostic: "syntactic" | "semantic" |
    /// "suggestion". The --syntactic-only band keys off this.
    #[serde(default)]
    pub pass: Option<String>,
    pub category: String,
    pub chain: OracleMessageChain,
    #[serde(default)]
    pub related: Vec<OracleRelated>,
    #[serde(default, rename = "reportsUnnecessary")]
    pub reports_unnecessary: bool,
    #[serde(default, rename = "reportsDeprecated")]
    pub reports_deprecated: bool,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OracleRelated {
    pub file: Option<String>,
    pub start: Option<u32>,
    pub length: Option<u32>,
    pub code: u32,
    pub category: String,
    pub chain: OracleMessageChain,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OracleMessageChain {
    pub text: String,
    pub code: u32,
    pub category: String,
    #[serde(default)]
    pub next: Vec<OracleMessageChain>,
}

#[derive(Debug, Serialize)]
struct DriverRequest {
    id: u64,
    #[serde(rename = "programJsonPath")]
    program_json_path: String,
}

#[derive(Debug, Deserialize)]
struct DriverResponse {
    id: u64,
    diagnostics: Option<Vec<OracleDiag>>,
    error: Option<String>,
}

pub fn oracle_diags(program_json: &Path) -> Result<Vec<OracleDiag>, OracleError> {
    default_pool().diagnostics(program_json)
}

pub struct OraclePool {
    workers: Vec<Mutex<DriverProcess>>,
    next_worker: AtomicUsize,
}

impl OraclePool {
    pub fn new(size: usize) -> Result<Self, OracleError> {
        let size = size.max(1);
        let mut workers = Vec::with_capacity(size);
        for _ in 0..size {
            workers.push(Mutex::new(DriverProcess::spawn()?));
        }
        Ok(Self {
            workers,
            next_worker: AtomicUsize::new(0),
        })
    }

    pub fn default_size() -> usize {
        let parallelism = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        (parallelism / 2).clamp(1, 4)
    }

    pub fn diagnostics(&self, program_json: &Path) -> Result<Vec<OracleDiag>, OracleError> {
        let index = self.next_worker.fetch_add(1, Ordering::Relaxed) % self.workers.len();
        let mut worker = self.workers[index]
            .lock()
            .map_err(|_| OracleError::new("oracle worker mutex poisoned"))?;

        match worker.diagnostics(program_json) {
            Ok(diagnostics) => Ok(diagnostics),
            Err(first_error) => {
                worker.restart()?;
                worker.diagnostics(program_json).map_err(|second_error| {
                    OracleError::new(format!(
                        "oracle worker failed after restart: {second_error}; initial error: {first_error}"
                    ))
                })
            }
        }
    }
}

struct DriverProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: AtomicU64,
}

impl DriverProcess {
    fn spawn() -> Result<Self, OracleError> {
        let mut child = Command::new("node")
            .arg(driver_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|err| OracleError::new(format!("failed to spawn oracle driver: {err}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| OracleError::new("oracle driver stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| OracleError::new("oracle driver stdout unavailable"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: AtomicU64::new(1),
        })
    }

    fn diagnostics(&mut self, program_json: &Path) -> Result<Vec<OracleDiag>, OracleError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let program_json_path = absolute_path(program_json)?;
        let request = DriverRequest {
            id,
            program_json_path: program_json_path.display().to_string(),
        };
        let request = serde_json::to_string(&request).map_err(|err| {
            OracleError::new(format!("failed to serialize oracle request: {err}"))
        })?;
        writeln!(self.stdin, "{request}")
            .and_then(|_| self.stdin.flush())
            .map_err(|err| OracleError::new(format!("failed to write oracle request: {err}")))?;

        let mut line = String::new();
        let read = self
            .stdout
            .read_line(&mut line)
            .map_err(|err| OracleError::new(format!("failed to read oracle response: {err}")))?;
        if read == 0 {
            return Err(OracleError::new("oracle driver exited without a response"));
        }

        let response: DriverResponse = serde_json::from_str(&line).map_err(|err| {
            OracleError::new(format!("failed to parse oracle response: {err}: {line}"))
        })?;
        if response.id != id {
            return Err(OracleError::new(format!(
                "oracle response id mismatch: expected {id}, got {}",
                response.id
            )));
        }
        if let Some(error) = response.error {
            return Err(OracleError::new(format!("oracle driver error: {error}")));
        }
        response
            .diagnostics
            .ok_or_else(|| OracleError::new("oracle response missing diagnostics"))
    }

    fn restart(&mut self) -> Result<(), OracleError> {
        let _ = self.child.kill();
        let _ = self.child.wait();
        *self = Self::spawn()?;
        Ok(())
    }
}

impl Drop for DriverProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn default_pool() -> &'static OraclePool {
    static POOL: OnceLock<OraclePool> = OnceLock::new();
    POOL.get_or_init(|| {
        OraclePool::new(OraclePool::default_size())
            .unwrap_or_else(|err| panic!("failed to initialize oracle pool: {err}"))
    })
}

fn driver_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("driver.mjs")
}

fn absolute_path(path: &Path) -> Result<PathBuf, OracleError> {
    if path.is_absolute() {
        Ok(path.to_owned())
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .map_err(|err| OracleError::new(format!("failed to resolve current directory: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);
        std::env::temp_dir().join(format!(
            "tsrs2-oracle-test-{}-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos(),
            NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn write_program_json(source_b64: &str) -> PathBuf {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("program.json");
        fs::write(
            &path,
            format!(
                "{{\n  \"schema\": 1,\n  \"cwd\": \"/\",\n  \"options\": {{\"noLib\": true}},\n  \"libs\": [],\n  \"files\": [{{\"name\": \"main.ts\", \"textB64\": \"{source_b64}\"}}],\n  \"matrixKey\": \"\"\n}}\n"
            ),
        )
        .expect("write program");
        path
    }

    #[test]
    fn oracle_driver_returns_syntactic_diagnostics() {
        let program_json = write_program_json("bGV0ID0gOwo=");
        let pool = OraclePool::new(1).expect("pool");
        let diagnostics = pool.diagnostics(&program_json).expect("diagnostics");

        assert!(diagnostics
            .iter()
            .any(|diag| diag.code == 1389 || diag.code == 1109));
        fs::remove_dir_all(program_json.parent().unwrap()).expect("remove temp dir");
    }

    #[test]
    fn oracle_driver_preserves_implied_node_format() {
        let dir = temp_dir();
        fs::create_dir_all(&dir).expect("temp dir");
        let program_json = dir.join("program.json");
        fs::write(
            &program_json,
            r#"{
  "schema": 1,
  "cwd": "/",
  "options": {"module": "node16", "moduleResolution": "node16", "target": "es2022"},
  "libs": [],
  "files": [
    {"name": "/foo.ts", "textB64": "ZXhwb3J0IGNvbnN0IHggPSAxOwo="},
    {"name": "/main.mts", "textB64": "aW1wb3J0IHsgeCB9IGZyb20gIi4vZm9vIjsKeDsK"}
  ],
  "matrixKey": ""
}
"#,
        )
        .expect("write program");

        let pool = OraclePool::new(1).expect("pool");
        let diagnostics = pool.diagnostics(&program_json).expect("diagnostics");
        assert!(
            diagnostics.iter().any(|diagnostic| diagnostic.code == 2835),
            "{diagnostics:?}"
        );
        fs::remove_dir_all(dir).expect("remove temp dir");
    }

    #[test]
    fn oracle_pool_is_deterministic_and_respawns_worker() {
        let program_json = write_program_json("bGV0ID0gOwo=");
        let pool = OraclePool::new(1).expect("pool");
        let first = pool.diagnostics(&program_json).expect("first diagnostics");
        {
            let mut worker = pool.workers[0].lock().expect("worker lock");
            worker.child.kill().expect("kill worker");
            worker.child.wait().expect("wait worker");
        }
        let second = pool.diagnostics(&program_json).expect("second diagnostics");

        assert_eq!(first, second);
        fs::remove_dir_all(program_json.parent().unwrap()).expect("remove temp dir");
    }
}
