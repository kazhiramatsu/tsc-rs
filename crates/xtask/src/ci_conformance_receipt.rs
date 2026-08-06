use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RECEIPT_SCHEMA: u32 = 1;
const PRODUCER_VERSION: &str = "ci-conformance-receipt-v1";
const COMMAND: &str = "perf ci-conformance-child";
const CACHE_POLICY: &str = "bounded-default";
pub(crate) const OUTPUT_ROLES: [&str; 4] = ["all", "2xxx", "syntactic", "families"];

#[derive(Debug)]
pub(crate) struct OutputSpec {
    pub(crate) role: String,
    pub(crate) path: PathBuf,
}

#[derive(Debug)]
pub(crate) struct Invocation {
    pub(crate) workspace: PathBuf,
    pub(crate) receipt_path: PathBuf,
    pub(crate) nonce: String,
    pub(crate) head: String,
    pub(crate) producer_executable_sha256: String,
    pub(crate) fingerprint_sha256: String,
    pub(crate) started_unix_ms: u128,
    pub(crate) outputs: Vec<OutputSpec>,
}

/// An in-process authority for one receipt publication. It deliberately has
/// no `Clone`, serialization, or public constructor: a later command cannot
/// discover a receipt on disk and promote it to same-job evidence.
#[derive(Debug)]
pub(crate) struct ReceiptToken {
    receipt_path: PathBuf,
    nonce: String,
    receipt_sha256: String,
}

/// A one-shot publication capability created only after the old receipt has
/// been invalidated. The parent keeps it in memory while the measured child
/// runs and consumes it when publishing the new receipt.
#[derive(Debug)]
pub(crate) struct PublicationGuard {
    receipt_path: PathBuf,
    nonce: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BoundOutput {
    pub(crate) role: String,
    pub(crate) path: String,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    schema: u32,
    producer_version: String,
    outcome: String,
    command: String,
    workspace: String,
    nonce: String,
    head: String,
    producer_executable_sha256: String,
    fingerprint_sha256: String,
    started_unix_ms: u128,
    finished_unix_ms: u128,
    full_corpus: bool,
    lib_bundle_cache: String,
    view_order: Vec<String>,
    pub(crate) bindings: Vec<BoundOutput>,
}

#[derive(Debug)]
pub(crate) struct ConsumedOutput {
    pub(crate) binding: BoundOutput,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Debug)]
pub(crate) struct ConsumedReceipt {
    pub(crate) receipt: Receipt,
    pub(crate) outputs: Vec<ConsumedOutput>,
}

pub(crate) fn fresh_nonce() -> Result<String, Box<dyn Error>> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(sha256_bytes(
        format!("{}:{now}:{counter}", std::process::id()).as_bytes(),
    ))
}

/// Invalidates the only discoverable authority before a producer starts.
/// Output files may remain for failure diagnostics, but without a receipt they
/// are never consumable.
pub(crate) fn begin(invocation: &Invocation) -> Result<PublicationGuard, Box<dyn Error>> {
    validate_invocation(invocation)?;
    let workspace = canonical_workspace(&invocation.workspace)?;
    let receipt_path = secure_output_path(&workspace, &invocation.receipt_path, false)?;
    match fs::remove_file(&receipt_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "failed to invalidate CI conformance receipt {}: {error}",
                receipt_path.display()
            )
            .into())
        }
    }
    Ok(PublicationGuard {
        receipt_path,
        nonce: invocation.nonce.clone(),
    })
}

/// Publishes only after every declared output exists and can be bound. The
/// receipt itself is written last with a sibling create-new + fsync + rename.
pub(crate) fn publish(
    guard: PublicationGuard,
    invocation: &Invocation,
) -> Result<ReceiptToken, Box<dyn Error>> {
    validate_invocation(invocation)?;
    let workspace = canonical_workspace(&invocation.workspace)?;
    let receipt_path = secure_output_path(&workspace, &invocation.receipt_path, false)?;
    if guard.receipt_path != receipt_path || guard.nonce != invocation.nonce {
        return Err("CI conformance publication guard belongs to another invocation".into());
    }
    let bindings = invocation
        .outputs
        .iter()
        .map(|output| bind_output(&workspace, output))
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let finished_unix_ms = now_unix_ms()?;
    if finished_unix_ms < invocation.started_unix_ms {
        return Err("CI conformance receipt has a reversed time interval".into());
    }
    let receipt = Receipt {
        schema: RECEIPT_SCHEMA,
        producer_version: PRODUCER_VERSION.to_owned(),
        outcome: "passed".to_owned(),
        command: COMMAND.to_owned(),
        workspace: path_text(&workspace)?,
        nonce: invocation.nonce.clone(),
        head: invocation.head.clone(),
        producer_executable_sha256: invocation.producer_executable_sha256.clone(),
        fingerprint_sha256: invocation.fingerprint_sha256.clone(),
        started_unix_ms: invocation.started_unix_ms,
        finished_unix_ms,
        full_corpus: true,
        lib_bundle_cache: CACHE_POLICY.to_owned(),
        view_order: OUTPUT_ROLES[..3]
            .iter()
            .map(|role| (*role).to_owned())
            .collect(),
        bindings,
    };
    let mut bytes = serde_json::to_vec_pretty(&receipt)?;
    bytes.push(b'\n');
    atomic_write(&workspace, &receipt_path, &bytes)?;
    Ok(ReceiptToken {
        receipt_path,
        nonce: invocation.nonce.clone(),
        receipt_sha256: sha256_bytes(&bytes),
    })
}

/// Re-reads every byte using a fresh hash pass. Any missing or stale receipt
/// is a hard error; callers must not fall back to running conformance again.
pub(crate) fn consume(
    token: ReceiptToken,
    invocation: &Invocation,
) -> Result<ConsumedReceipt, Box<dyn Error>> {
    validate_invocation(invocation)?;
    let workspace = canonical_workspace(&invocation.workspace)?;
    let expected_path = secure_output_path(&workspace, &invocation.receipt_path, true)?;
    if token.receipt_path != expected_path || token.nonce != invocation.nonce {
        return Err("CI conformance receipt token does not belong to this invocation".into());
    }
    let receipt_bytes = read_regular_file(&expected_path)?;
    if sha256_bytes(&receipt_bytes) != token.receipt_sha256 {
        return Err("CI conformance receipt changed after publication".into());
    }
    let receipt: Receipt = serde_json::from_slice(&receipt_bytes)?;
    let output_bytes = validate_receipt(&workspace, &receipt, invocation)?;
    let outputs = receipt
        .bindings
        .iter()
        .cloned()
        .zip(output_bytes)
        .map(|(binding, bytes)| ConsumedOutput { binding, bytes })
        .collect();
    Ok(ConsumedReceipt { receipt, outputs })
}

fn validate_invocation(invocation: &Invocation) -> Result<(), Box<dyn Error>> {
    if !is_hex(&invocation.nonce, 64)
        || !is_hex(&invocation.head, 40)
        || !is_hex(&invocation.producer_executable_sha256, 64)
        || !is_hex(&invocation.fingerprint_sha256, 64)
    {
        return Err("CI conformance invocation has malformed identity fields".into());
    }
    if invocation.outputs.len() != OUTPUT_ROLES.len()
        || invocation
            .outputs
            .iter()
            .zip(OUTPUT_ROLES)
            .any(|(output, role)| output.role != role)
    {
        return Err("CI conformance outputs must be exactly all,2xxx,syntactic,families".into());
    }
    let workspace = canonical_workspace(&invocation.workspace)?;
    let receipt = secure_output_path(&workspace, &invocation.receipt_path, false)?;
    let mut distinct_paths = BTreeSet::new();
    for output in &invocation.outputs {
        let path = secure_output_path(&workspace, &output.path, false)?;
        if path == receipt {
            return Err("CI conformance receipt cannot also be a bound output".into());
        }
        if !distinct_paths.insert(path) {
            return Err("CI conformance roles must bind four distinct output paths".into());
        }
    }
    Ok(())
}

fn validate_receipt(
    workspace: &Path,
    receipt: &Receipt,
    invocation: &Invocation,
) -> Result<Vec<Vec<u8>>, Box<dyn Error>> {
    let expected_views = OUTPUT_ROLES[..3]
        .iter()
        .map(|role| (*role).to_owned())
        .collect::<Vec<_>>();
    if receipt.schema != RECEIPT_SCHEMA
        || receipt.producer_version != PRODUCER_VERSION
        || receipt.outcome != "passed"
        || receipt.command != COMMAND
        || receipt.workspace != path_text(workspace)?
        || receipt.nonce != invocation.nonce
        || receipt.head != invocation.head
        || receipt.producer_executable_sha256 != invocation.producer_executable_sha256
        || receipt.fingerprint_sha256 != invocation.fingerprint_sha256
        || receipt.started_unix_ms != invocation.started_unix_ms
        || receipt.finished_unix_ms < receipt.started_unix_ms
        || !receipt.full_corpus
        || receipt.lib_bundle_cache != CACHE_POLICY
        || receipt.view_order != expected_views
        || receipt.bindings.len() != OUTPUT_ROLES.len()
    {
        return Err("CI conformance receipt identity or policy mismatch".into());
    }
    let mut output_bytes = Vec::with_capacity(OUTPUT_ROLES.len());
    for ((binding, output), role) in receipt
        .bindings
        .iter()
        .zip(&invocation.outputs)
        .zip(OUTPUT_ROLES)
    {
        if binding.role != role || output.role != role {
            return Err("CI conformance receipt output order mismatch".into());
        }
        let path = secure_output_path(workspace, &output.path, true)?;
        if binding.path != workspace_relative(workspace, &path)? {
            return Err(format!("CI conformance {role} output path mismatch").into());
        }
        let bytes = read_regular_file(&path)?;
        if binding.bytes != bytes.len() as u64 || binding.sha256 != sha256_bytes(&bytes) {
            return Err(format!("CI conformance {role} output digest mismatch").into());
        }
        output_bytes.push(bytes);
    }
    Ok(output_bytes)
}

fn bind_output(workspace: &Path, output: &OutputSpec) -> Result<BoundOutput, Box<dyn Error>> {
    let path = secure_output_path(workspace, &output.path, true)?;
    let bytes = read_regular_file(&path)?;
    Ok(BoundOutput {
        role: output.role.clone(),
        path: workspace_relative(workspace, &path)?,
        bytes: bytes.len() as u64,
        sha256: sha256_bytes(&bytes),
    })
}

fn canonical_workspace(workspace: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let metadata = fs::symlink_metadata(workspace)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("CI conformance workspace must be a real directory".into());
    }
    Ok(workspace.canonicalize()?)
}

fn secure_output_path(
    workspace: &Path,
    path: &Path,
    require_file: bool,
) -> Result<PathBuf, Box<dyn Error>> {
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(format!(
            "CI conformance path must be workspace-relative and lexical: {}",
            path.display()
        )
        .into());
    }
    let mut current = workspace.to_owned();
    let components = path.components().collect::<Vec<_>>();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            unreachable!("components were validated above")
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "CI conformance path contains a symlink: {}",
                        current.display()
                    )
                    .into());
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Err(format!(
                        "CI conformance parent is not a directory: {}",
                        current.display()
                    )
                    .into());
                }
                if index + 1 == components.len() && require_file && !metadata.is_file() {
                    return Err(format!(
                        "CI conformance output is not a regular file: {}",
                        current.display()
                    )
                    .into());
                }
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && index + 1 == components.len()
                    && !require_file => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(current)
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!(
            "CI conformance output is not a regular file: {}",
            path.display()
        )
        .into());
    }
    Ok(fs::read(path)?)
}

fn atomic_write(workspace: &Path, path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| format!("receipt path has no parent: {}", path.display()))?;
    let parent_relative = parent.strip_prefix(workspace)?;
    let secured_parent = secure_directory_path(workspace, parent_relative)?;
    if secured_parent != parent {
        return Err("CI conformance receipt parent changed before publication".into());
    }
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("receipt file name is not UTF-8")?;
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let metadata = fs::symlink_metadata(&temporary)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("CI conformance receipt temporary is not a regular file".into());
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if secure_directory_path(workspace, parent_relative)? != parent {
            return Err("CI conformance receipt parent changed during publication".into());
        }
        fs::rename(&temporary, path)?;
        let published = secure_output_path(workspace, path.strip_prefix(workspace)?, true)?;
        if published != path || fs::read(&published)? != bytes {
            return Err("CI conformance receipt changed during atomic publication".into());
        }
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn secure_directory_path(workspace: &Path, path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("CI conformance directory path is not workspace-relative".into());
    }
    let mut current = workspace.to_owned();
    for component in path.components() {
        let Component::Normal(name) = component else {
            unreachable!("directory components were validated above")
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "CI conformance directory is not a real directory: {}",
                current.display()
            )
            .into());
        }
    }
    Ok(current)
}

fn workspace_relative(workspace: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    path_text(path.strip_prefix(workspace)?)
}

fn path_text(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path
        .to_str()
        .ok_or("CI conformance path is not UTF-8")?
        .replace('\\', "/"))
}

fn is_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now_unix_ms() -> Result<u128, Box<dyn Error>> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())
}

#[cfg(test)]
#[path = "../tests/unit/ci_conformance_receipt/tests.rs"]
mod tests;
