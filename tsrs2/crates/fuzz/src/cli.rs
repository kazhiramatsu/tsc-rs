//! Dedicated M9 producer command line.

use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::executor::replay_artifact;
use crate::replay::ReplayArtifact;
use crate::{rust_worker, FoundationError, FoundationResult};

pub const REDUCE_UNAVAILABLE: &str =
    "fuzz reduce is fail-closed until the M9.1d real-replay reducer lands";
pub const MAX_REPLAY_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

pub fn run(args: impl IntoIterator<Item = OsString>) -> FoundationResult<()> {
    let mut args = args.into_iter();
    let command = args
        .next()
        .ok_or_else(|| FoundationError::new("missing producer command (replay|reduce)"))?;
    match command.to_str() {
        Some("replay") => replay(args),
        Some("reduce") => Err(FoundationError::new(REDUCE_UNAVAILABLE)),
        Some("__worker") => {
            require_no_args(args, "__worker")?;
            rust_worker::serve_stdio()
        }
        Some(other) => Err(FoundationError::new(format!(
            "unknown producer command {other:?}"
        ))),
        None => Err(FoundationError::new("producer command must be valid UTF-8")),
    }
}

fn replay(mut args: impl Iterator<Item = OsString>) -> FoundationResult<()> {
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| FoundationError::new("fuzz replay requires one artifact path"))?,
    );
    if args.next().is_some() {
        return Err(FoundationError::new(
            "fuzz replay requires one artifact path",
        ));
    }
    let bytes = read_bounded_artifact(&path, MAX_REPLAY_ARTIFACT_BYTES).map_err(|error| {
        FoundationError::new(format!(
            "cannot read replay artifact {}: {error}",
            path.display()
        ))
    })?;
    let artifact = ReplayArtifact::from_canonical_slice(&bytes)?;
    let executable = std::env::current_exe().map_err(|error| {
        FoundationError::new(format!("cannot resolve producer executable: {error}"))
    })?;
    let replay = replay_artifact(&artifact, &executable)?;
    println!(
        "true replay verified: case_sha256={} execution_sha256={} artifact={}",
        artifact.case_sha256,
        replay.evaluated().execution_sha256(),
        path.display()
    );
    Ok(())
}

fn read_bounded_artifact(path: &Path, limit: u64) -> std::io::Result<Vec<u8>> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("artifact exceeds the {limit}-byte replay limit"),
        ));
    }
    Ok(bytes)
}

fn require_no_args(
    mut args: impl Iterator<Item = OsString>,
    command: &str,
) -> FoundationResult<()> {
    if args.next().is_some() {
        return Err(FoundationError::new(format!(
            "{command} does not accept arguments"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn reduce_is_explicitly_fail_closed() {
        let error = run([OsString::from("reduce"), OsString::from("artifact.json")])
            .unwrap_err()
            .to_string();
        assert_eq!(error, REDUCE_UNAVAILABLE);
    }

    #[test]
    fn replay_requires_exactly_one_path() {
        assert!(run([OsString::from("replay")]).is_err());
        assert!(run([
            OsString::from("replay"),
            OsString::from("one"),
            OsString::from("two")
        ])
        .is_err());
    }

    #[test]
    fn replay_artifact_reader_stops_at_its_hard_limit() {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = PathBuf::from(format!(
            "/tmp/tsrs2-fuzz-cli-{}-{serial}.json",
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.write_all(b"12345").unwrap();
        drop(file);

        assert_eq!(read_bounded_artifact(&path, 5).unwrap(), b"12345");
        assert_eq!(
            read_bounded_artifact(&path, 4).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        fs::remove_file(path).unwrap();
    }
}
