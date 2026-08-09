use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const JOURNAL_SCHEMA: u32 = 1;
const PRODUCER_VERSION: &str = "local-ci-resume-v1";
const JOURNAL_RELATIVE_PATH: &str = "target/local-ci-resume/v1/journal.json";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputScope {
    /// Every tracked or non-ignored untracked repository file.
    All,
    /// Executable verification inputs. Markdown is covered by the cheap
    /// workspace/readme phases and is deliberately excluded from expensive
    /// compiler, oracle, and corpus phases.
    Verification,
    /// Rust sources plus the formatter/toolchain configuration.
    RustFormat,
}

impl InputScope {
    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Verification => "verification",
            Self::RustFormat => "rust-format",
        }
    }

    fn includes(self, relative: &str) -> bool {
        match self {
            Self::All => true,
            Self::Verification => !relative
                .rsplit_once('.')
                .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("md")),
            Self::RustFormat => {
                relative.ends_with(".rs")
                    || matches!(
                        relative,
                        "rustfmt.toml" | ".rustfmt.toml" | "rust-toolchain.toml"
                    )
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct LocalCiResume {
    workspace: PathBuf,
    journal_path: PathBuf,
    invocation: String,
    tool_fingerprint: String,
    snapshot: WorkspaceSnapshot,
    journal: Journal,
    reused: usize,
    recorded: usize,
}

impl LocalCiResume {
    pub(crate) fn open(
        workspace: &Path,
        invocation: String,
        fresh: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let workspace = fs::canonicalize(workspace)?;
        let journal_path = workspace.join(JOURNAL_RELATIVE_PATH);
        if fresh {
            match fs::remove_file(&journal_path) {
                Ok(()) => println!("local CI resume: discarded the previous failed-run journal"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "cannot discard local CI resume journal {}: {error}",
                        journal_path.display()
                    )
                    .into())
                }
            }
        }

        let snapshot = WorkspaceSnapshot::collect(&workspace)?;
        let tool_fingerprint = tool_fingerprint(&workspace)?;
        let journal = load_journal(&journal_path, &invocation).unwrap_or_else(|error| {
            eprintln!(
                "local CI resume: ignoring unusable journal {}: {error}",
                journal_path.display()
            );
            Journal::new(invocation.clone())
        });

        Ok(Self {
            workspace,
            journal_path,
            invocation,
            tool_fingerprint,
            snapshot,
            journal,
            reused: 0,
            recorded: 0,
        })
    }

    pub(crate) fn run_phase<F>(
        &mut self,
        name: &'static str,
        scope: InputScope,
        salt: &str,
        outputs: &[&str],
        operation: F,
    ) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce() -> Result<(), Box<dyn Error>>,
    {
        validate_phase_name(name)?;
        let expected_outputs = outputs
            .iter()
            .map(|relative| validate_relative_path(relative).map(|_| (*relative).to_owned()))
            .collect::<Result<Vec<_>, _>>()?;
        let fingerprint = self.phase_fingerprint(name, scope, salt, &expected_outputs);

        if self.journal.phases.get(name).is_some_and(|receipt| {
            receipt_is_reusable(&self.workspace, receipt, &fingerprint, &expected_outputs)
                .unwrap_or(false)
        }) {
            self.reused += 1;
            println!("local CI resume: reuse {name} (exact inputs and outputs)");
            return Ok(());
        }

        if self.journal.phases.remove(name).is_some() {
            self.persist()?;
        }
        println!("local CI phase: run {name}");
        operation()?;

        let current_stability = workspace_stability_marker(&self.workspace)?;
        if current_stability != self.snapshot.stability_marker {
            return Err(format!(
                "repository inputs changed while local CI phase {name} was running; rerun the gate"
            )
            .into());
        }

        let bindings = bind_outputs(&self.workspace, &expected_outputs)?;
        self.journal.phases.insert(
            name.to_owned(),
            PhaseReceipt {
                fingerprint_sha256: fingerprint,
                outputs: bindings,
            },
        );
        self.persist()?;
        self.recorded += 1;
        println!("local CI checkpoint: recorded {name}");
        Ok(())
    }

    pub(crate) fn finish(self) -> Result<(), Box<dyn Error>> {
        match fs::remove_file(&self.journal_path) {
            Ok(()) => {
                println!(
                    "local CI resume: complete; cleared failed-run journal (reused={} recorded={})",
                    self.reused, self.recorded
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "cannot clear completed local CI resume journal {}: {error}",
                    self.journal_path.display()
                )
                .into())
            }
        }
        if let Some(directory) = self.journal_path.parent() {
            let _ = fs::remove_dir(directory);
        }
        Ok(())
    }

    pub(crate) fn failure_hint(&self) {
        if self.journal.phases.is_empty() {
            return;
        }
        eprintln!(
            "local CI resume: retained {} successful phase(s) in {}; rerun the same command to reuse exact unaffected phases, or pass --fresh",
            self.journal.phases.len(),
            self.journal_path.display()
        );
    }

    fn phase_fingerprint(
        &self,
        name: &str,
        scope: InputScope,
        salt: &str,
        outputs: &[String],
    ) -> String {
        phase_fingerprint(
            &self.snapshot,
            &self.invocation,
            &self.tool_fingerprint,
            name,
            scope,
            salt,
            outputs,
        )
    }

    fn persist(&self) -> Result<(), Box<dyn Error>> {
        write_journal(&self.journal_path, &self.journal)
    }
}

#[derive(Clone, Debug)]
struct WorkspaceSnapshot {
    entries: Vec<WorkspaceEntry>,
    stability_marker: String,
}

impl WorkspaceSnapshot {
    fn collect(workspace: &Path) -> Result<Self, Box<dyn Error>> {
        let paths = repository_paths(workspace)?;
        let mut entries = Vec::with_capacity(paths.len());
        for relative in &paths {
            entries.push(WorkspaceEntry {
                relative: relative.clone(),
                sha256: path_sha256(&workspace.join(relative))?,
            });
        }
        Ok(Self {
            entries,
            stability_marker: stability_marker(workspace, &paths)?,
        })
    }
}

#[derive(Clone, Debug)]
struct WorkspaceEntry {
    relative: String,
    sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u32,
    producer_version: String,
    invocation: String,
    phases: BTreeMap<String, PhaseReceipt>,
}

impl Journal {
    fn new(invocation: String) -> Self {
        Self {
            schema: JOURNAL_SCHEMA,
            producer_version: PRODUCER_VERSION.to_owned(),
            invocation,
            phases: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PhaseReceipt {
    fingerprint_sha256: String,
    outputs: Vec<OutputBinding>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct OutputBinding {
    relative: String,
    sha256: String,
}

fn load_journal(path: &Path, invocation: &str) -> Result<Journal, Box<dyn Error>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Journal::new(invocation.to_owned()))
        }
        Err(error) => return Err(error.into()),
    };
    let journal: Journal = serde_json::from_slice(&bytes)?;
    if journal.schema != JOURNAL_SCHEMA
        || journal.producer_version != PRODUCER_VERSION
        || journal.invocation != invocation
    {
        return Ok(Journal::new(invocation.to_owned()));
    }
    Ok(journal)
}

fn write_journal(path: &Path, journal: &Journal) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("journal path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let mut bytes = serde_json::to_vec_pretty(journal)?;
    bytes.push(b'\n');
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let mut temporary = None;
    for sequence in 0_u32..100 {
        let candidate = parent.join(format!(
            ".journal-{}-{nonce}-{sequence}.tmp",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                file.write_all(&bytes)?;
                file.sync_all()?;
                temporary = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    let temporary = temporary.ok_or("cannot allocate local CI journal temporary file")?;
    if let Err(first_error) = fs::rename(&temporary, path) {
        // Windows does not replace an existing destination with `rename`.
        // Losing this ignored retry journal is fail-safe (the phases rerun),
        // so remove only this exact file and retry publication.
        if !path.is_file() || fs::remove_file(path).is_err() {
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot publish {}: {first_error}", path.display()).into());
        }
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot publish {}: {error}", path.display()).into());
        }
    }
    Ok(())
}

fn phase_fingerprint(
    snapshot: &WorkspaceSnapshot,
    invocation: &str,
    tool_fingerprint: &str,
    name: &str,
    scope: InputScope,
    salt: &str,
    outputs: &[String],
) -> String {
    let mut hash = Sha256::new();
    hash_field(&mut hash, b"producer", PRODUCER_VERSION.as_bytes());
    hash_field(&mut hash, b"invocation", invocation.as_bytes());
    hash_field(&mut hash, b"tools", tool_fingerprint.as_bytes());
    hash_field(&mut hash, b"phase", name.as_bytes());
    hash_field(&mut hash, b"scope", scope.label().as_bytes());
    hash_field(&mut hash, b"salt", salt.as_bytes());
    for output in outputs {
        hash_field(&mut hash, b"output", output.as_bytes());
    }
    for entry in &snapshot.entries {
        if scope.includes(&entry.relative) {
            hash_field(
                &mut hash,
                entry.relative.as_bytes(),
                entry.sha256.as_bytes(),
            );
        }
    }
    format!("{:x}", hash.finalize())
}

fn receipt_is_reusable(
    workspace: &Path,
    receipt: &PhaseReceipt,
    fingerprint: &str,
    expected_outputs: &[String],
) -> Result<bool, Box<dyn Error>> {
    if receipt.fingerprint_sha256 != fingerprint || receipt.outputs.len() != expected_outputs.len()
    {
        return Ok(false);
    }
    for (binding, expected) in receipt.outputs.iter().zip(expected_outputs) {
        if binding.relative != *expected {
            return Ok(false);
        }
        validate_relative_path(expected)?;
        let path = workspace.join(expected);
        if !is_regular_file_without_symlink(&path)? || path_sha256(&path)? != binding.sha256 {
            return Ok(false);
        }
    }
    Ok(true)
}

fn bind_outputs(
    workspace: &Path,
    expected_outputs: &[String],
) -> Result<Vec<OutputBinding>, Box<dyn Error>> {
    expected_outputs
        .iter()
        .map(|relative| {
            validate_relative_path(relative)?;
            let path = workspace.join(relative);
            if !is_regular_file_without_symlink(&path)? {
                return Err(format!(
                    "local CI phase did not produce required regular output {}",
                    path.display()
                )
                .into());
            }
            Ok(OutputBinding {
                relative: relative.clone(),
                sha256: path_sha256(&path)?,
            })
        })
        .collect()
}

fn is_regular_file_without_symlink(path: &Path) -> Result<bool, Box<dyn Error>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_file() && !metadata.file_type().is_symlink()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn repository_paths(workspace: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cannot enumerate local CI inputs: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let mut paths = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|bytes| !bytes.is_empty())
        .map(|bytes| {
            let relative = String::from_utf8(bytes.to_vec())?;
            validate_relative_path(&relative)?;
            Ok(relative)
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn workspace_stability_marker(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let paths = repository_paths(workspace)?;
    stability_marker(workspace, &paths)
}

fn stability_marker(workspace: &Path, paths: &[String]) -> Result<String, Box<dyn Error>> {
    let mut hash = Sha256::new();
    for relative in paths {
        hash_field(&mut hash, b"path", relative.as_bytes());
        let path = workspace.join(relative);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                let kind = if metadata.file_type().is_symlink() {
                    "symlink"
                } else if metadata.is_file() {
                    "file"
                } else if metadata.is_dir() {
                    "directory"
                } else {
                    "special"
                };
                hash_field(&mut hash, b"kind", kind.as_bytes());
                hash_field(&mut hash, b"length", metadata.len().to_string().as_bytes());
                if let Ok(modified) = metadata.modified() {
                    if let Ok(duration) = modified.duration_since(UNIX_EPOCH) {
                        hash_field(
                            &mut hash,
                            b"modified",
                            format!("{}:{}", duration.as_secs(), duration.subsec_nanos())
                                .as_bytes(),
                        );
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                hash_field(&mut hash, b"kind", b"missing");
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn path_sha256(path: &Path) -> Result<String, Box<dyn Error>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(sha256_bytes(b"missing"))
        }
        Err(error) => return Err(error.into()),
    };
    let mut hash = Sha256::new();
    if metadata.file_type().is_symlink() {
        hash.update(b"symlink\0");
        hash.update(fs::read_link(path)?.as_os_str().as_encoded_bytes());
    } else if metadata.is_file() {
        hash.update(b"file\0");
        hash.update(metadata.len().to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            hash.update((metadata.permissions().mode() & 0o111).to_le_bytes());
        }
        let mut file = fs::File::open(path)?;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hash.update(&buffer[..read]);
        }
    } else if metadata.is_dir() {
        hash.update(b"directory\0");
    } else {
        hash.update(b"special\0");
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn tool_fingerprint(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let mut hash = Sha256::new();
    hash_field(&mut hash, b"os", std::env::consts::OS.as_bytes());
    hash_field(&mut hash, b"arch", std::env::consts::ARCH.as_bytes());
    let executable = std::env::current_exe()?;
    hash_field(
        &mut hash,
        b"xtask-executable",
        path_sha256(&executable)?.as_bytes(),
    );
    for (program, arguments) in [
        ("cargo", &["--version", "--verbose"][..]),
        ("rustc", &["--version", "--verbose"][..]),
        ("rustfmt", &["--version"][..]),
        ("node", &["--version"][..]),
        ("git", &["--version"][..]),
    ] {
        let output = Command::new(program)
            .current_dir(workspace)
            .args(arguments)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "cannot identify local CI tool {program}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        hash_field(&mut hash, program.as_bytes(), &output.stdout);
    }

    let mut environment = std::env::vars_os()
        .filter(|(key, _)| environment_affects_ci(key))
        .collect::<Vec<_>>();
    environment.sort_by(|left, right| left.0.cmp(&right.0));
    for (key, value) in environment {
        hash_field(&mut hash, key.as_encoded_bytes(), value.as_encoded_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn environment_affects_ci(key: &OsStr) -> bool {
    let key = key.to_string_lossy();
    key == "PATH"
        || key == "CI"
        || key == "NODE_OPTIONS"
        || key.starts_with("CARGO_")
        || key.starts_with("RUST")
        || key.starts_with("TSRS_")
}

fn validate_phase_name(name: &str) -> Result<(), Box<dyn Error>> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(format!("invalid local CI phase name {name:?}").into());
    }
    Ok(())
}

fn validate_relative_path(relative: &str) -> Result<(), Box<dyn Error>> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(
            format!("local CI path must stay relative to the workspace: {relative:?}").into(),
        );
    }
    Ok(())
}

fn hash_field(hash: &mut Sha256, key: &[u8], value: &[u8]) {
    hash.update((key.len() as u64).to_le_bytes());
    hash.update(key);
    hash.update((value.len() as u64).to_le_bytes());
    hash.update(value);
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
#[path = "../tests/unit/local_ci_resume/tests.rs"]
mod tests;
