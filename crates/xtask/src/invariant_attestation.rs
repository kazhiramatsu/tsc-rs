use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(crate) const ATTESTATION_SCHEMA: u32 = 1;
pub(crate) const ATTESTATION_RELATIVE_PATH: &str = "target/invariants/full-corpus-attestation.json";
pub(crate) const FULL_CORPUS_COMMAND: &str = "cargo xtask invariants --suite all --full-corpus";
pub(crate) const REQUIRED_SUITES: [&str; 6] = [
    "prefix-determinism",
    "idempotence",
    "jobs-independence",
    "encodings",
    "matrix-independence",
    "unsupported-unwind",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ControlledInputFingerprint {
    pub(crate) name: String,
    pub(crate) files: usize,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct CorpusObservation {
    pub(crate) fixtures: usize,
    pub(crate) programs: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FullCorpusAttestation {
    pub(crate) schema: u32,
    pub(crate) outcome: String,
    pub(crate) command: String,
    pub(crate) full_corpus: bool,
    pub(crate) suites: Vec<String>,
    pub(crate) corpus: CorpusObservation,
    pub(crate) workspace: String,
    pub(crate) created_unix_seconds: u64,
    pub(crate) controlled_inputs: Vec<ControlledInputFingerprint>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AttestationProbe {
    pub(crate) ready: bool,
    pub(crate) detail: String,
}

impl AttestationProbe {
    fn red(kind: &str, detail: impl AsRef<str>) -> Self {
        Self {
            ready: false,
            detail: format!(
                "full-corpus invariant attestation {kind}: {}",
                detail.as_ref()
            ),
        }
    }
}

pub(crate) fn attestation_path(workspace: &Path) -> PathBuf {
    workspace.join(ATTESTATION_RELATIVE_PATH)
}

/// Every invariant invocation invalidates the old success first. A sampled
/// run, a partial suite, an error, or a panic therefore cannot leave row 10
/// green by accidentally reusing a prior full-corpus observation.
pub(crate) fn invalidate(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let path = attestation_path(workspace);
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to invalidate full-corpus invariant attestation {}: {error}",
                path.display()
            )
            .into());
        }
    }
    Ok(())
}

pub(crate) fn write_success(
    workspace: &Path,
    fixtures: usize,
    programs: usize,
) -> Result<PathBuf, Box<dyn Error>> {
    if fixtures == 0 || programs == 0 {
        return Err("refusing to attest an empty invariant corpus".into());
    }
    let canonical_workspace = fs::canonicalize(workspace)?;
    let attestation = FullCorpusAttestation {
        schema: ATTESTATION_SCHEMA,
        outcome: "passed".to_owned(),
        command: FULL_CORPUS_COMMAND.to_owned(),
        full_corpus: true,
        suites: REQUIRED_SUITES
            .iter()
            .map(|suite| (*suite).to_owned())
            .collect(),
        corpus: CorpusObservation { fixtures, programs },
        workspace: normalize_path(&canonical_workspace),
        created_unix_seconds: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        controlled_inputs: controlled_input_fingerprints(workspace)?,
    };
    let mut bytes = serde_json::to_vec_pretty(&attestation)?;
    bytes.push(b'\n');
    let path = attestation_path(workspace);
    atomic_write(&path, &bytes)?;
    Ok(path)
}

pub(crate) fn verify(workspace: &Path) -> AttestationProbe {
    match verify_inner(workspace) {
        Ok(attestation) => AttestationProbe {
            ready: true,
            detail: format!(
                "fresh full-corpus invariant attestation: suites={} fixtures={} programs={} command=`{}`",
                attestation.suites.len(),
                attestation.corpus.fixtures,
                attestation.corpus.programs,
                attestation.command
            ),
        },
        Err(error) => error,
    }
}

fn verify_inner(workspace: &Path) -> Result<FullCorpusAttestation, AttestationProbe> {
    let path = attestation_path(workspace);
    let bytes = fs::read(&path).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AttestationProbe::red(
                "missing",
                format!(
                    "{}; run `{FULL_CORPUS_COMMAND}` in this workspace",
                    path.display()
                ),
            )
        } else {
            AttestationProbe::red(
                "invalid",
                format!("cannot read {}: {error}", path.display()),
            )
        }
    })?;
    let attestation: FullCorpusAttestation = serde_json::from_slice(&bytes).map_err(|error| {
        AttestationProbe::red(
            "invalid",
            format!("{} is not valid JSON: {error}", path.display()),
        )
    })?;
    if attestation.schema != ATTESTATION_SCHEMA {
        return Err(AttestationProbe::red(
            "invalid",
            format!(
                "schema={} expected={ATTESTATION_SCHEMA}",
                attestation.schema
            ),
        ));
    }
    if attestation.outcome != "passed" {
        return Err(AttestationProbe::red(
            "failed",
            format!("outcome={}", attestation.outcome),
        ));
    }
    let expected_suites = REQUIRED_SUITES
        .iter()
        .map(|suite| (*suite).to_owned())
        .collect::<Vec<_>>();
    if !attestation.full_corpus
        || attestation.command != FULL_CORPUS_COMMAND
        || attestation.suites != expected_suites
        || attestation.corpus.fixtures == 0
        || attestation.corpus.programs == 0
    {
        return Err(AttestationProbe::red(
            "partial",
            format!(
                "full_corpus={} command=`{}` suites={:?} fixtures={} programs={}",
                attestation.full_corpus,
                attestation.command,
                attestation.suites,
                attestation.corpus.fixtures,
                attestation.corpus.programs
            ),
        ));
    }
    let canonical_workspace = fs::canonicalize(workspace).map_err(|error| {
        AttestationProbe::red("invalid", format!("cannot canonicalize workspace: {error}"))
    })?;
    let expected_workspace = normalize_path(&canonical_workspace);
    if attestation.workspace != expected_workspace {
        return Err(AttestationProbe::red(
            "stale",
            format!(
                "workspace={} current={expected_workspace}",
                attestation.workspace
            ),
        ));
    }
    let current = controlled_input_fingerprints(workspace).map_err(|error| {
        AttestationProbe::red(
            "stale",
            format!("cannot re-fingerprint controlled inputs: {error}"),
        )
    })?;
    if attestation.controlled_inputs != current {
        let changed = changed_fingerprint_names(&attestation.controlled_inputs, &current);
        return Err(AttestationProbe::red(
            "stale",
            format!("controlled inputs changed: {}", changed.join(",")),
        ));
    }
    Ok(attestation)
}

fn changed_fingerprint_names(
    recorded: &[ControlledInputFingerprint],
    current: &[ControlledInputFingerprint],
) -> Vec<String> {
    let names = recorded
        .iter()
        .map(|entry| entry.name.as_str())
        .chain(current.iter().map(|entry| entry.name.as_str()))
        .collect::<std::collections::BTreeSet<_>>();
    names
        .into_iter()
        .filter(|name| {
            recorded.iter().find(|entry| entry.name == **name)
                != current.iter().find(|entry| entry.name == **name)
        })
        .map(str::to_owned)
        .collect()
}

pub(crate) fn controlled_input_fingerprints(
    workspace: &Path,
) -> Result<Vec<ControlledInputFingerprint>, Box<dyn Error>> {
    // These are the complete semantic inputs to expansion and checking plus
    // the immutable grading/scope anchors which define "the full corpus".
    // Directory entries are recursively path-and-content hashed, so adding,
    // deleting, renaming, or changing a file invalidates the attestation.
    let groups: [(&str, &[&str]); 14] = [
        ("checker", &["crates/checker"]),
        ("syntax", &["crates/syntax"]),
        ("binder", &["crates/binder"]),
        ("types", &["crates/types"]),
        ("diagnostics", &["crates/diagnostics"]),
        ("harness", &["crates/harness"]),
        ("conformance-options", &["crates/conformance"]),
        ("xtask", &["crates/xtask"]),
        (
            "rust-build",
            &[
                "Cargo.toml",
                "Cargo.lock",
                "rust-toolchain.toml",
                ".cargo/config.toml",
            ],
        ),
        ("corpus", &["ts-tests/tests/cases/conformance"]),
        ("vendor-libs", &["vendor/typescript-6.0.3/lib"]),
        (
            "host-resolution-producer",
            &[
                ".node-version",
                "crates/oracle/host-resolution-requests.mjs",
                "crates/oracle/program-host.mjs",
            ],
        ),
        (
            "immutable-oracle-state",
            &[
                "ratchets/oracle-inputs.v1.json.zst",
                "ratchets/conformance-matches.v1.json.zst",
                "ratchet.toml",
            ],
        ),
        (
            "scope-and-family-state",
            &[
                "m8-scope.json",
                "diag-families.json",
                "ratchets/host-resolution.v1.json",
                "STAGE",
            ],
        ),
    ];
    groups
        .into_iter()
        .map(|(name, roots)| fingerprint_group(workspace, name, roots))
        .collect()
}

fn fingerprint_group(
    workspace: &Path,
    name: &str,
    roots: &[&str],
) -> Result<ControlledInputFingerprint, Box<dyn Error>> {
    let mut paths = Vec::new();
    for relative in roots {
        collect_controlled_files(workspace, &workspace.join(relative), &mut paths)?;
    }
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        return Err(format!("controlled input group {name} is empty").into());
    }
    let mut hasher = Sha256::new();
    for path in &paths {
        let relative = path
            .strip_prefix(workspace)
            .map_err(|_| format!("controlled input escaped workspace: {}", path.display()))?;
        let relative = normalize_path(relative);
        let bytes = fs::read(path)?;
        hash_length_prefixed(&mut hasher, relative.as_bytes());
        hash_length_prefixed(&mut hasher, &bytes);
    }
    Ok(ControlledInputFingerprint {
        name: name.to_owned(),
        files: paths.len(),
        sha256: format!("{:x}", hasher.finalize()),
    })
}

fn collect_controlled_files(
    workspace: &Path,
    path: &Path,
    out: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        format!(
            "controlled input is missing or unreadable: {}: {error}",
            path.display()
        )
    })?;
    if metadata.file_type().is_symlink() {
        return Err(format!("controlled input symlink is forbidden: {}", path.display()).into());
    }
    if metadata.is_file() {
        if !path.starts_with(workspace) {
            return Err(format!("controlled input escaped workspace: {}", path.display()).into());
        }
        out.push(path.to_owned());
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err(format!("unsupported controlled input: {}", path.display()).into());
    }
    let mut children = fs::read_dir(path)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for child in children {
        collect_controlled_files(workspace, &child, out)?;
    }
    Ok(())
}

fn hash_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("attestation path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("full-corpus-attestation"),
        std::process::id()
    ));
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
#[path = "../tests/unit/invariant_attestation/tests.rs"]
mod tests;
