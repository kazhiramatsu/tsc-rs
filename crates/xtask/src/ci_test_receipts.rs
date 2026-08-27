use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RECEIPT_SCHEMA: u32 = 1;
const RECEIPT_KIND: &str = "workspace-test-target-receipt";
const PRODUCER_VERSION: &str = "gate-tax-6-report-only-v1";

#[derive(Clone, Copy, Debug)]
enum InputTree {
    File(&'static str),
    Directory(&'static str),
}

#[derive(Clone, Copy, Debug)]
struct TargetInputScope {
    label: &'static str,
    inputs: &'static [InputTree],
}

const NO_RUNTIME_INPUTS: &[InputTree] = &[];
const CONTROL_INPUTS: &[InputTree] = &[
    InputTree::File(".github/ci/plans/h2-5g.v1.json"),
    InputTree::File("crates/ci-adapter-tsc-rs-control/src/lib.rs"),
    InputTree::File("crates/ci-adapter-tsc-rs-protocol/src/lib.rs"),
];
const TESTKIT_INPUTS: &[InputTree] = &[InputTree::File("crates/ci-testkit/src/lib.rs")];
const CI_CORE_INPUTS: &[InputTree] = &[
    InputTree::Directory("crates/ci-core/tests/contracts"),
    InputTree::Directory("crates/ci-core/src"),
    InputTree::File("docs/design/greenfield/slices/explanation-cases.v1.json"),
    InputTree::File("docs/design/greenfield/slices/impact-cases.v1.json"),
];

/// Curated report-only pilot. Each label is package-qualified by the Cargo
/// artifact reader. Tests outside this table are deliberately uncached.
const TEST_TARGET_INPUT_SCOPES: &[TargetInputScope] = &[
    TargetInputScope {
        label: "tsc-rs-checker::authoritative_external_fact [test]",
        inputs: NO_RUNTIME_INPUTS,
    },
    TargetInputScope {
        label: "tsc-rs-ci-adapter-control::plan [test]",
        inputs: CONTROL_INPUTS,
    },
    TargetInputScope {
        label: "tsc-rs-ci-adapter-protocol::protocol [test]",
        inputs: NO_RUNTIME_INPUTS,
    },
    TargetInputScope {
        label: "tsc-rs-ci-core::contracts [test]",
        inputs: CI_CORE_INPUTS,
    },
    TargetInputScope {
        label: "tsc-rs-ci-testkit::fixtures [test]",
        inputs: TESTKIT_INPUTS,
    },
    TargetInputScope {
        label: "tsc-rs-emitter::source_comment_topology_contract [test]",
        inputs: NO_RUNTIME_INPUTS,
    },
    TargetInputScope {
        label: "tsc-rs-types::compiler_option_number_contract [test]",
        inputs: NO_RUNTIME_INPUTS,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Decision {
    Hit,
    Miss(&'static str),
    Undeclared,
}

impl Decision {
    pub(crate) fn render(self) -> String {
        match self {
            Self::Hit => "hit".to_owned(),
            Self::Miss(term) => format!("miss({term})"),
            Self::Undeclared => "undeclared".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct FileBinding {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentBinding {
    name: String,
    value: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct KeyTerms {
    binary_sha256: String,
    inputs: Vec<FileBinding>,
    environment: Vec<EnvironmentBinding>,
    harness_threads: usize,
    rustc_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReceiptBody {
    schema: u32,
    kind: String,
    producer_version: String,
    outcome: String,
    label: String,
    key: KeyTerms,
    key_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    #[serde(flatten)]
    body: ReceiptBody,
    receipt_fingerprint_sha256: String,
}

#[derive(Clone, Debug)]
struct Publication {
    workspace: PathBuf,
    path: PathBuf,
    label: String,
    key: KeyTerms,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedReceipt {
    decision: Decision,
    publication: Option<Publication>,
    diagnostic: Option<String>,
}

impl PreparedReceipt {
    pub(crate) fn decision(&self) -> Decision {
        self.decision
    }

    pub(crate) fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }

    /// Mint only after the associated executable has completed successfully.
    pub(crate) fn publish(&self) -> Result<(), Box<dyn Error>> {
        let Some(publication) = &self.publication else {
            return Ok(());
        };
        let body = ReceiptBody {
            schema: RECEIPT_SCHEMA,
            kind: RECEIPT_KIND.to_owned(),
            producer_version: PRODUCER_VERSION.to_owned(),
            outcome: "passed".to_owned(),
            label: publication.label.clone(),
            key: publication.key.clone(),
            key_sha256: key_fingerprint(&publication.key)?,
        };
        let receipt = Receipt {
            receipt_fingerprint_sha256: body_fingerprint(&body)?,
            body,
        };
        let mut bytes = serde_json::to_vec_pretty(&receipt)?;
        bytes.push(b'\n');
        atomic_write(&publication.workspace, &publication.path, &bytes)
    }
}

/// Capture the exact compiler identity used by the receipt key.
pub(crate) fn rustc_version() -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-V")
        .output()
        .map_err(|error| format!("cannot run rustc -V: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "rustc -V failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let version = String::from_utf8(output.stdout)
        .map_err(|error| format!("rustc -V returned non-UTF-8 output: {error}"))?;
    Ok(version.trim().to_owned())
}

/// Prepare a report-only decision. Any receipt-side error becomes a miss so
/// the caller can continue to run every test target.
pub(crate) fn prepare(
    workspace: &Path,
    label: &str,
    binary: &Path,
    environment: &[(&str, Option<String>)],
    harness_threads: usize,
    rustc_version: Result<&str, &str>,
) -> PreparedReceipt {
    let Some(scope) = TEST_TARGET_INPUT_SCOPES
        .iter()
        .find(|scope| scope.label == label)
    else {
        return PreparedReceipt {
            decision: Decision::Undeclared,
            publication: None,
            diagnostic: None,
        };
    };
    let rustc_version = match rustc_version {
        Ok(version) => version,
        Err(error) => {
            return PreparedReceipt {
                decision: Decision::Miss("rustc"),
                publication: None,
                diagnostic: Some(error.to_owned()),
            }
        }
    };
    match prepare_declared(
        workspace,
        label,
        binary,
        environment,
        harness_threads,
        rustc_version,
        scope,
    ) {
        Ok(prepared) => prepared,
        Err(error) => PreparedReceipt {
            decision: Decision::Miss("key"),
            publication: None,
            diagnostic: Some(error.to_string()),
        },
    }
}

fn prepare_declared(
    workspace: &Path,
    label: &str,
    binary: &Path,
    environment: &[(&str, Option<String>)],
    harness_threads: usize,
    rustc_version: &str,
    scope: &TargetInputScope,
) -> Result<PreparedReceipt, Box<dyn Error>> {
    let workspace = canonical_workspace(workspace)?;
    let key = KeyTerms {
        binary_sha256: sha256_regular_file(binary)?,
        inputs: collect_inputs(&workspace, scope.inputs)?,
        environment: environment
            .iter()
            .map(|(name, value)| EnvironmentBinding {
                name: (*name).to_owned(),
                value: value.clone(),
            })
            .collect(),
        harness_threads,
        rustc_version: rustc_version.to_owned(),
    };
    let path = receipt_path(&workspace, label)?;
    let decision = receipt_decision(&path, label, &key);
    Ok(PreparedReceipt {
        decision,
        publication: Some(Publication {
            workspace,
            path,
            label: label.to_owned(),
            key,
        }),
        diagnostic: None,
    })
}

fn receipt_decision(path: &Path, label: &str, expected: &KeyTerms) -> Decision {
    let bytes = match read_regular_file(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Decision::Miss("absent")
        }
        Err(_) => return Decision::Miss("invalid"),
    };
    let Ok(receipt) = serde_json::from_slice::<Receipt>(&bytes) else {
        return Decision::Miss("invalid");
    };
    if receipt.body.schema != RECEIPT_SCHEMA
        || receipt.body.kind != RECEIPT_KIND
        || receipt.body.producer_version != PRODUCER_VERSION
        || receipt.body.outcome != "passed"
        || receipt.body.label != label
        || body_fingerprint(&receipt.body).ok().as_deref()
            != Some(receipt.receipt_fingerprint_sha256.as_str())
        || key_fingerprint(&receipt.body.key).ok().as_deref()
            != Some(receipt.body.key_sha256.as_str())
    {
        return Decision::Miss("invalid");
    }
    if receipt.body.key.binary_sha256 != expected.binary_sha256 {
        return Decision::Miss("binary");
    }
    if receipt.body.key.inputs != expected.inputs {
        return Decision::Miss("inputs");
    }
    if receipt.body.key.environment != expected.environment {
        return Decision::Miss("environment");
    }
    if receipt.body.key.harness_threads != expected.harness_threads {
        return Decision::Miss("harness-threads");
    }
    if receipt.body.key.rustc_version != expected.rustc_version {
        return Decision::Miss("rustc");
    }
    if receipt.body.key_sha256 != key_fingerprint(expected).unwrap_or_default() {
        return Decision::Miss("invalid");
    }
    Decision::Hit
}

fn collect_inputs(
    workspace: &Path,
    input_trees: &[InputTree],
) -> Result<Vec<FileBinding>, Box<dyn Error>> {
    let mut paths = BTreeMap::<String, PathBuf>::new();
    for input in input_trees {
        match input {
            InputTree::File(relative) => {
                let path = secure_input_path(workspace, Path::new(relative), false)?;
                paths.insert((*relative).to_owned(), path);
            }
            InputTree::Directory(relative) => {
                let directory = secure_input_path(workspace, Path::new(relative), true)?;
                collect_directory_files(workspace, &directory, &mut paths)?;
            }
        }
    }
    paths
        .into_iter()
        .map(|(path, absolute)| {
            let bytes = read_regular_file(&absolute)?;
            Ok(FileBinding {
                path,
                bytes: bytes.len() as u64,
                sha256: sha256_bytes(&bytes),
            })
        })
        .collect()
}

fn collect_directory_files(
    workspace: &Path,
    directory: &Path,
    paths: &mut BTreeMap<String, PathBuf>,
) -> Result<(), Box<dyn Error>> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(format!("test receipt input is a symlink: {}", path.display()).into());
        }
        if metadata.is_dir() {
            collect_directory_files(workspace, &path, paths)?;
        } else if metadata.is_file() {
            let relative = workspace_relative(workspace, &path)?;
            paths.insert(relative, path);
        } else {
            return Err(format!(
                "test receipt input is not a file or directory: {}",
                path.display()
            )
            .into());
        }
    }
    Ok(())
}

fn canonical_workspace(workspace: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let metadata = fs::symlink_metadata(workspace)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("test receipt workspace must be a real directory".into());
    }
    Ok(workspace.canonicalize()?)
}

fn secure_input_path(
    workspace: &Path,
    relative: &Path,
    require_directory: bool,
) -> Result<PathBuf, Box<dyn Error>> {
    if relative.is_absolute()
        || relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("invalid test receipt input path: {}", relative.display()).into());
    }
    let mut current = workspace.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            unreachable!("input components were validated")
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(format!("test receipt input is a symlink: {}", current.display()).into());
        }
    }
    let metadata = fs::symlink_metadata(&current)?;
    if (require_directory && !metadata.is_dir()) || (!require_directory && !metadata.is_file()) {
        return Err(format!(
            "test receipt input has the wrong type: {}",
            current.display()
        )
        .into());
    }
    Ok(current)
}

fn receipt_path(workspace: &Path, label: &str) -> Result<PathBuf, Box<dyn Error>> {
    let directory = receipt_directory(workspace)?;
    let mut slug = String::with_capacity(label.len());
    let mut separator = false;
    for character in label.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !slug.is_empty() {
            slug.push('-');
            separator = true;
        }
        if slug.len() >= 72 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        slug.push_str("target");
    }
    Ok(directory.join(format!(
        "{slug}-{}.json",
        &sha256_bytes(label.as_bytes())[..16]
    )))
}

fn receipt_directory(workspace: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let target = workspace.join("target");
    ensure_real_directory(&target)?;
    let receipts = target.join("ci-test-receipts");
    ensure_real_directory(&receipts)?;
    Ok(receipts)
}

fn ensure_real_directory(path: &Path) -> Result<(), Box<dyn Error>> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "test receipt directory is not a real directory: {}",
            path.display()
        )
        .into());
    }
    Ok(())
}

fn read_regular_file(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(std::io::Error::other(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    fs::read(path)
}

fn sha256_regular_file(path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(sha256_bytes(&read_regular_file(path)?))
}

fn key_fingerprint(key: &KeyTerms) -> Result<String, serde_json::Error> {
    Ok(sha256_bytes(&serde_json::to_vec(key)?))
}

fn body_fingerprint(body: &ReceiptBody) -> Result<String, serde_json::Error> {
    Ok(sha256_bytes(&serde_json::to_vec(body)?))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn atomic_write(workspace: &Path, path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let expected_parent = receipt_directory(workspace)?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("test receipt path has no parent: {}", path.display()))?;
    if parent != expected_parent {
        return Err("test receipt publication escaped its receipt directory".into());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("test receipt file name is not UTF-8")?;
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), counter));
    let result = (|| -> Result<(), Box<dyn Error>> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if receipt_directory(workspace)? != parent {
            return Err("test receipt directory changed during publication".into());
        }
        fs::rename(&temporary, path)?;
        if read_regular_file(path)? != bytes {
            return Err("test receipt changed during atomic publication".into());
        }
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn workspace_relative(workspace: &Path, path: &Path) -> Result<String, Box<dyn Error>> {
    Ok(path
        .strip_prefix(workspace)?
        .to_str()
        .ok_or("test receipt input path is not UTF-8")?
        .replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_workspace() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tsc-rs-test-receipt-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn curated_table_is_small_unique_and_package_qualified() {
        assert!((5..=8).contains(&TEST_TARGET_INPUT_SCOPES.len()));
        let mut labels = TEST_TARGET_INPUT_SCOPES
            .iter()
            .map(|scope| scope.label)
            .collect::<Vec<_>>();
        assert!(labels.iter().all(|label| label.contains("::")));
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), TEST_TARGET_INPUT_SCOPES.len());
    }

    #[test]
    fn green_receipt_hits_and_input_changes_name_the_miss_term() {
        let workspace = temporary_workspace();
        let source = workspace.join("crates/ci-testkit/src/lib.rs");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"first").unwrap();
        let binary = workspace.join("test-binary");
        fs::write(&binary, b"binary").unwrap();
        let environment = [("TSRS_CI_TEST_WORKERS", Some("1".to_owned()))];
        let label = "tsc-rs-ci-testkit::fixtures [test]";

        let first = prepare(
            &workspace,
            label,
            &binary,
            &environment,
            1,
            Ok("rustc test"),
        );
        assert_eq!(first.decision(), Decision::Miss("absent"));
        first.publish().unwrap();
        assert_eq!(
            prepare(
                &workspace,
                label,
                &binary,
                &environment,
                1,
                Ok("rustc test")
            )
            .decision(),
            Decision::Hit
        );

        fs::write(&binary, b"changed binary").unwrap();
        assert_eq!(
            prepare(
                &workspace,
                label,
                &binary,
                &environment,
                1,
                Ok("rustc test")
            )
            .decision(),
            Decision::Miss("binary")
        );
        fs::write(&binary, b"binary").unwrap();
        assert_eq!(
            prepare(
                &workspace,
                label,
                &binary,
                &[("TSRS_CI_TEST_WORKERS", Some("2".to_owned()))],
                1,
                Ok("rustc test")
            )
            .decision(),
            Decision::Miss("environment")
        );
        assert_eq!(
            prepare(
                &workspace,
                label,
                &binary,
                &environment,
                2,
                Ok("rustc test")
            )
            .decision(),
            Decision::Miss("harness-threads")
        );
        assert_eq!(
            prepare(
                &workspace,
                label,
                &binary,
                &environment,
                1,
                Ok("rustc changed")
            )
            .decision(),
            Decision::Miss("rustc")
        );
        fs::write(source, b"second").unwrap();
        assert_eq!(
            prepare(
                &workspace,
                label,
                &binary,
                &environment,
                1,
                Ok("rustc test")
            )
            .decision(),
            Decision::Miss("inputs")
        );
        fs::remove_dir_all(workspace).unwrap();
    }

    #[test]
    fn receipt_fingerprint_rejects_tampering() {
        let workspace = temporary_workspace();
        let binary = workspace.join("test-binary");
        fs::write(&binary, b"binary").unwrap();
        let label = "tsc-rs-types::compiler_option_number_contract [test]";
        let prepared = prepare(&workspace, label, &binary, &[], 1, Ok("rustc test"));
        prepared.publish().unwrap();
        let publication = prepared.publication.as_ref().unwrap();
        let mut receipt: serde_json::Value =
            serde_json::from_slice(&fs::read(&publication.path).unwrap()).unwrap();
        receipt["outcome"] = serde_json::Value::String("failed".to_owned());
        fs::write(&publication.path, serde_json::to_vec(&receipt).unwrap()).unwrap();

        assert_eq!(
            prepare(&workspace, label, &binary, &[], 1, Ok("rustc test")).decision(),
            Decision::Miss("invalid")
        );
        fs::remove_dir_all(workspace).unwrap();
    }
}
