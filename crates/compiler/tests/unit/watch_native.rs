//! The process's watch backend over the real file system: watches share
//! the notifier, each gets the changes under its directory with the
//! directory's spelling, a removed directory ends its watch, and a watch's
//! callback may start another.

use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::*;
use crate::watch::WatchError;

/// A directory of its own under the system's temporary directory, removed
/// at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "tsc-rs-watch-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    fn path(&self) -> String {
        self.0.to_string_lossy().replace('\\', "/")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Report = (Vec<WatchEvent>, Option<WatchError>);

fn events() -> (WatchEvents, Receiver<Report>) {
    let (sender, receiver) = channel();
    let sender = Mutex::new(sender);
    (
        WatchEvents::new(move |events, error| {
            let _ = lock(&sender).send((events, error));
        }),
        receiver,
    )
}

fn request(directory: &str, recursive: bool, events: WatchEvents) -> WatchRequest {
    WatchRequest {
        directory: directory.to_owned(),
        recursive,
        events,
    }
}

/// Waits for a report of a change of `path`, or of the error; the reports
/// on the way are dropped.
fn wait_for(receiver: &Receiver<Report>, what: impl Fn(&Report) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok(report) if what(&report) => return true,
            Ok(_) => {}
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return false,
        }
    }
}

fn changes(path: &str) -> impl Fn(&Report) -> bool + '_ {
    move |(events, _)| events.iter().any(|event| event.path == path)
}

/// Whether no report of `path` comes for a while.
fn quiet(receiver: &Receiver<Report>, path: &str) -> bool {
    let deadline = Instant::now() + Duration::from_millis(500);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok(report) if changes(path)(&report) => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
    }
}

/// Lets the notifier start before the changes (FSEvents reports only the
/// changes after its stream starts).
fn settle() {
    std::thread::sleep(Duration::from_millis(200));
}

#[test]
fn a_missing_directory_is_an_error() {
    // tsgo `validateWatchDirectory`.
    let directory = TempDir::new();
    let backend = NativeWatchBackend::new();
    let (events, _) = events();
    let missing = format!("{}/missing", directory.path());
    let error = backend
        .watch_directories(vec![request(&missing, false, events)])
        .err()
        .expect("an error");
    assert_eq!(
        error.to_string(),
        format!("stat {missing}: no such file or directory")
    );
}

#[test]
fn a_directory_off_the_disk_is_an_error() {
    // tsgo `WatchDirectories`: the bundled libraries' directory, which the
    // session's program files watch asks for, is not absolute.
    let backend = NativeWatchBackend::new();
    let (events, _) = events();
    let error = backend
        .watch_directories(vec![request("bundled:///libs", true, events)])
        .err()
        .expect("an error");
    assert_eq!(error.to_string(), "fswatch: path must be absolute");
}

#[test]
fn watches_of_one_directory_share_the_notifier() {
    let directory = TempDir::new();
    let backend = NativeWatchBackend::new();
    let (first, first_reports) = events();
    let (second, second_reports) = events();
    let mut handles = backend
        .watch_directories(vec![
            request(&directory.path(), false, first),
            request(&directory.path(), false, second),
        ])
        .unwrap();
    settle();
    let file = format!("{}/a.ts", directory.path());
    std::fs::write(&file, "export {}").unwrap();
    assert!(wait_for(&first_reports, changes(&file)));
    assert!(wait_for(&second_reports, changes(&file)));
    // The first goes; the second still watches.
    drop(handles.remove(0));
    settle();
    let other = format!("{}/b.ts", directory.path());
    std::fs::write(&other, "export {}").unwrap();
    assert!(wait_for(&second_reports, changes(&other)));
    assert!(quiet(&first_reports, &other));
}

#[test]
fn only_a_recursive_watch_sees_its_tree() {
    let directory = TempDir::new();
    std::fs::create_dir_all(directory.0.join("sub")).unwrap();
    let backend = NativeWatchBackend::new();
    let (tree, tree_reports) = events();
    let (entries, entry_reports) = events();
    let _handles = backend
        .watch_directories(vec![
            request(&directory.path(), true, tree),
            request(&directory.path(), false, entries),
        ])
        .unwrap();
    settle();
    let deep = format!("{}/sub/deep.ts", directory.path());
    std::fs::write(&deep, "export {}").unwrap();
    assert!(wait_for(&tree_reports, changes(&deep)));
    assert!(quiet(&entry_reports, &deep));
}

#[test]
fn a_removed_directory_ends_its_watch() {
    // tsgo `ErrWatchTerminated`: the directory's deletion, then the end.
    let base = TempDir::new();
    let directory = format!("{}/pkg", base.path());
    std::fs::create_dir_all(&directory).unwrap();
    let backend = NativeWatchBackend::new();
    let (watch, reports) = events();
    let _handles = backend
        .watch_directories(vec![request(&directory, false, watch)])
        .unwrap();
    settle();
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(wait_for(&reports, |(_, error)| *error
        == Some(WatchError::Terminated)));
}

#[test]
fn a_watchs_callback_may_start_a_watch() {
    // The lspwatcher's ancestor watch moves down from its callback.
    let base = TempDir::new();
    let backend = Arc::new(NativeWatchBackend::new());
    let (started, started_reports) = channel();
    let started = Mutex::new(Some(started));
    let callback_backend = Arc::clone(&backend);
    let target = format!("{}/pkg", base.path());
    let watched = target.clone();
    let ancestor = WatchEvents::new(move |_, _| {
        let Some(started) = lock(&started).take() else {
            return;
        };
        let (events, _) = events();
        let handles = callback_backend.watch_directories(vec![request(&watched, false, events)]);
        let _ = started.send(handles.map(|handles| handles.len()));
    });
    let _handles = backend
        .watch_directories(vec![request(&base.path(), false, ancestor)])
        .unwrap();
    settle();
    std::fs::create_dir_all(&target).unwrap();
    let started = started_reports
        .recv_timeout(Duration::from_secs(5))
        .expect("the callback ran");
    assert_eq!(started.unwrap(), 1);
}
