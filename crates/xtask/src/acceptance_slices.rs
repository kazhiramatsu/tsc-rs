//! Non-authoritative, individually rerunnable projections of hosted acceptance.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::acceptance_plan::SLICE_IDS;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FailureClass {
    Environment,
    Semantic,
}

#[derive(Debug, Serialize)]
struct FailureArtifact {
    schema: u32,
    kind: &'static str,
    slice: String,
    class: FailureClass,
    retry: &'static str,
    input_sha256: String,
    message_sha256: String,
    message: String,
    message_bytes: usize,
    truncated: bool,
}

pub(crate) fn run(slice: &str, workspace: &Path) -> Result<(), Box<dyn Error>> {
    if slice == "no-impact" {
        println!("acceptance slice skipped: no semantic inputs");
        return Ok(());
    }
    if !SLICE_IDS.contains(&slice) {
        return Err(format!("unknown acceptance slice: {slice}").into());
    }

    let result = run_slice(slice, workspace);
    if let Err(error) = result {
        let message = stable_message(workspace, &error.to_string());
        let class = classify_failure(&message);
        match write_failure_artifact(workspace, slice, class, &message) {
            Ok(artifact) => eprintln!(
                "acceptance failure: class={} slice={} artifact={}",
                class.as_str(),
                slice,
                artifact.display()
            ),
            Err(artifact_error) => eprintln!(
                "acceptance failure: class={} slice={} (artifact unavailable: {artifact_error})",
                class.as_str(),
                slice
            ),
        }
        return Err(error);
    }
    Ok(())
}

fn run_slice(slice: &str, workspace: &Path) -> Result<(), Box<dyn Error>> {
    match slice {
        "conformance" => crate::conformance(std::iter::empty()),
        "h1" => crate::h1_emit_acceptance::run(workspace),
        "h2-1a" => crate::h2_1a_acceptance::run(workspace),
        "h2-1b" => crate::h2_1b_acceptance::run(workspace),
        "h2-1c" => crate::h2_1c_acceptance::run(workspace),
        "h2-1d" => crate::h2_1d_acceptance::run(workspace),
        "h2-1e" => crate::h2_1e_acceptance::run(workspace),
        "h2-2a" => crate::h2_2a_acceptance::run(workspace),
        "h2-2b" => crate::h2_2b_acceptance::run(workspace),
        "h2-2c" => crate::h2_2c_acceptance::run(workspace),
        "h2-2d" => crate::h2_2d_acceptance::run(workspace),
        "h2-3a" => crate::h2_3a_acceptance::run(workspace),
        "h2-3b" => crate::h2_3b_acceptance::run(workspace),
        "h2-3c" => crate::h2_3c_acceptance::run(workspace),
        "h2-3d" => crate::h2_3d_acceptance::run(workspace),
        "h2-4a" => crate::h2_2c_acceptance::run_h2_4a(workspace),
        "h2-4b" => crate::h2_2c_acceptance::run_h2_4b(workspace),
        "h2-5a" => crate::h2_2c_acceptance::run_h2_5a(workspace),
        "h2-5b" => crate::h2_2c_acceptance::run_h2_5b(workspace),
        "h2-5c" => crate::h2_2c_acceptance::run_h2_5c(workspace),
        "h2-5d" => crate::h2_2c_acceptance::run_h2_5d(workspace),
        "h2-5e" => crate::h2_2c_acceptance::run_h2_5e(workspace),
        "h2-5f" => crate::h2_2c_acceptance::run_h2_5f(workspace),
        "h2-5g" => crate::h2_2c_acceptance::run_h2_5g(workspace),
        _ => unreachable!("slice validated by run"),
    }
}

fn classify_failure(message: &str) -> FailureClass {
    let lower = message.to_ascii_lowercase();
    const ENVIRONMENT_MARKERS: &[&str] = &[
        "no such file or directory",
        "permission denied",
        "failed to spawn",
        "timed out",
        "deadline exceeded",
        "broken pipe",
        "out of memory",
        "cannot read input",
        "repository inputs changed while",
        "worker process exited",
        "process was killed",
        "received signal",
        "runner environment",
        "i/o error",
        "os error",
    ];
    if ENVIRONMENT_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        FailureClass::Environment
    } else {
        // Unknown failures are semantic for retry policy purposes. This is
        // intentionally fail-closed: no unclassified mismatch may be retried
        // as if it were a transient runner problem.
        FailureClass::Semantic
    }
}

fn stable_message(workspace: &Path, message: &str) -> String {
    message.replace(workspace.to_string_lossy().as_ref(), "<workspace>")
}

fn write_failure_artifact(
    workspace: &Path,
    slice: &str,
    class: FailureClass,
    message: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let directory = workspace.join("target/ci/acceptance-failures");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("{slice}.json"));
    let input_sha256 = digest(format!(
        "acceptance-slice-v1\0{slice}\0{}",
        git_head(workspace).unwrap_or_else(|_| "unknown".to_owned())
    ));
    let (message, truncated) = bounded_message(message);
    let message_sha256 = digest(message.clone());
    let artifact = FailureArtifact {
        schema: 1,
        kind: "acceptance-failure",
        slice: slice.to_owned(),
        class,
        retry: match class {
            FailureClass::Environment => "retry-same-slice",
            FailureClass::Semantic => "fix-before-rerun",
        },
        input_sha256,
        message_sha256,
        message_bytes: message.len(),
        message,
        truncated,
    };
    let bytes = serde_json::to_vec_pretty(&artifact)?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, [bytes.as_slice(), b"\n"].concat())?;
    fs::rename(&temporary, &path)?;
    Ok(path)
}

fn bounded_message(message: &str) -> (String, bool) {
    const MAX_BYTES: usize = 256 * 1024;
    if message.len() <= MAX_BYTES {
        return (message.to_owned(), false);
    }
    let mut end = MAX_BYTES;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    (message[..end].to_owned(), true)
}

fn git_head(workspace: &Path) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .current_dir(workspace)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cannot resolve acceptance input revision: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn digest(value: String) -> String {
    let mut hash = Sha256::new();
    hash.update(value.as_bytes());
    format!("{:x}", hash.finalize())
}

impl FailureClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Environment => "environment",
            Self::Semantic => "semantic",
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/acceptance_slices/tests.rs"]
mod tests;
