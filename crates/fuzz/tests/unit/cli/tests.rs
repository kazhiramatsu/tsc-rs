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
        "/tmp/tsc-rs-fuzz-cli-{}-{serial}.json",
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
