//! tsgo `lspwatcher_test.go`: the watches of the session's watchers over
//! tsgo's fake backend, and over the process's for the real file system.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use tsc_compiler::watch::NativeWatchBackend;
use tsc_host::vfs::OsFs;
use tsc_lsproto::{RelativePattern, Uri};

use super::*;

fn os_fs() -> Arc<dyn FileSystem> {
    Arc::new(OsFs::new(!cfg!(any(target_os = "macos", windows))))
}

/// A directory of its own under the system's temporary directory (tsgo
/// `t.TempDir`), removed at the end.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "tsc-rs-lspwatcher-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    /// The directory as tsgo spells it (`tspath.NormalizeSlashes`).
    fn path(&self) -> String {
        self.0.to_string_lossy().replace('\\', "/")
    }

    fn join(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn wait_for(condition: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {what}");
}

fn pattern(pattern: String, kind: Option<WatchKind>) -> FileSystemWatcher {
    FileSystemWatcher {
        glob_pattern: PatternOrRelativePattern::Pattern(pattern),
        kind,
    }
}

const ALL: Option<WatchKind> = Some(WatchKind(7));

/// The changes a watcher hands over.
#[derive(Clone, Default)]
struct Collected(Arc<Mutex<Vec<FileEvent>>>);

impl Collected {
    fn sink(&self) -> impl Fn(Vec<FileEvent>) + Send + Sync + 'static {
        let collected = Arc::clone(&self.0);
        move |changes| collected.lock().unwrap().extend(changes)
    }

    fn events(&self) -> Vec<FileEvent> {
        self.0.lock().unwrap().clone()
    }

    fn has(&self, change_type: FileChangeType, suffix: &str) -> bool {
        self.events()
            .iter()
            .any(|event| event.r#type == change_type && event.uri.0.ends_with(suffix))
    }
}

/// tsgo's `fakeBackend`: watches by directory, the callbacks the test
/// calls.
#[derive(Default)]
struct FakeBackend {
    state: Mutex<FakeState>,
    this: Mutex<Weak<FakeBackend>>,
}

#[derive(Default)]
struct FakeState {
    by_directory: BTreeMap<String, WatchEvents>,
    recursive: BTreeMap<String, bool>,
    fail_directories: BTreeMap<String, String>,
}

impl FakeBackend {
    fn new() -> Arc<Self> {
        let backend = Arc::new(Self::default());
        *backend.this.lock().unwrap() = Arc::downgrade(&backend);
        backend
    }

    fn watched_directories(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .by_directory
            .keys()
            .cloned()
            .collect()
    }

    fn is_watching(&self, directory: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .by_directory
            .contains_key(directory)
    }

    /// tsgo `emit`: the watch of `directory` reports, outside the lock.
    fn emit(&self, directory: &str, events: Vec<WatchEvent>, error: Option<WatchError>) {
        let callback = self
            .state
            .lock()
            .unwrap()
            .by_directory
            .get(directory)
            .cloned();
        if let Some(callback) = callback {
            report(&callback, events, error);
        }
    }

    /// tsgo `emitAll`.
    fn emit_all(&self, events: Vec<WatchEvent>, error: Option<WatchError>) {
        let callbacks = self
            .state
            .lock()
            .unwrap()
            .by_directory
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for callback in callbacks {
            report(&callback, events.clone(), error);
        }
    }
}

fn report(callback: &WatchEvents, events: Vec<WatchEvent>, error: Option<WatchError>) {
    callback.send(events);
    match error {
        Some(WatchError::Overflow) => callback.overflow(),
        Some(WatchError::Terminated) => callback.terminate(),
        None => {}
    }
}

struct FakeWatch {
    backend: Weak<FakeBackend>,
    directory: String,
}

impl WatchHandle for FakeWatch {}

impl Drop for FakeWatch {
    fn drop(&mut self) {
        if let Some(backend) = self.backend.upgrade() {
            backend
                .state
                .lock()
                .unwrap()
                .by_directory
                .remove(&self.directory);
        }
    }
}

impl WatchBackend for FakeBackend {
    fn watch_directories(
        &self,
        requests: Vec<WatchRequest>,
    ) -> io::Result<Vec<Box<dyn WatchHandle>>> {
        let mut state = self.state.lock().unwrap();
        let mut handles: Vec<Box<dyn WatchHandle>> = Vec::new();
        for request in requests {
            if let Some(error) = state.fail_directories.get(&request.directory) {
                return Err(io::Error::other(error.clone()));
            }
            state
                .recursive
                .insert(request.directory.clone(), request.recursive);
            state
                .by_directory
                .insert(request.directory.clone(), request.events);
            handles.push(Box::new(FakeWatch {
                backend: self.this.lock().unwrap().clone(),
                directory: request.directory,
            }));
        }
        Ok(handles)
    }
}

fn update(path: &str) -> WatchEvent {
    WatchEvent {
        kind: WatchEventKind::Update,
        path: path.to_owned(),
    }
}

fn delete(path: &str) -> WatchEvent {
    WatchEvent {
        kind: WatchEventKind::Delete,
        path: path.to_owned(),
    }
}

fn fake_watcher(backend: &Arc<FakeBackend>, collected: &Collected) -> Watcher {
    Watcher::new(os_fs(), backend.clone(), collected.sink(), |_| {})
}

#[test]
fn creations_changes_and_deletions_reach_the_session() {
    // tsgo `TestWatcher_CreateChangeDelete`, over the process's backend.
    let directory = TempDir::new();
    let collected = Collected::default();
    let watcher = Watcher::new(
        os_fs(),
        Arc::new(NativeWatchBackend::new()),
        collected.sink(),
        |_| {},
    );
    watcher
        .watch_files(
            "test",
            &[pattern(format!("{}/**/*", directory.path()), ALL)],
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let file = directory.join("a.ts");
    std::fs::write(&file, "export {}").unwrap();
    wait_for(
        || {
            collected
                .events()
                .iter()
                .any(|event| event.r#type == FileChangeType::CHANGED)
        },
        "update event",
    );
    std::fs::remove_file(&file).unwrap();
    wait_for(
        || {
            collected
                .events()
                .iter()
                .any(|event| event.r#type == FileChangeType::DELETED)
        },
        "delete event",
    );
    watcher.unwatch_files("test").unwrap();
    watcher.close();
}

#[test]
fn a_watchers_kind_filters_its_events() {
    // tsgo `TestWatcher_KindFilter`.
    let directory = TempDir::new();
    let backend = FakeBackend::new();
    let collected = Collected::default();
    let watcher = fake_watcher(&backend, &collected);
    watcher
        .watch_files(
            "test",
            &[pattern(
                format!("{}/**/*", directory.path()),
                Some(WatchKind::DELETE),
            )],
        )
        .unwrap();
    let file = format!("{}/x.ts", directory.path());
    backend.emit_all(vec![update(&file), delete(&file)], None);
    wait_for(
        || {
            collected
                .events()
                .iter()
                .any(|event| event.r#type == FileChangeType::DELETED)
        },
        "delete event",
    );
    for event in collected.events() {
        assert_eq!(event.r#type, FileChangeType::DELETED, "{event:?}");
    }
    watcher.close();
}

#[test]
fn the_root_of_a_glob() {
    // tsgo `TestRootFromGlob`.
    for (glob, root) in [
        ("/abs/path/**/*", "/abs/path"),
        ("/abs/path/", "/abs/path"),
        ("/abs/path/?.ts", "/abs/path"),
        ("/abs/path/{a,b}/*", "/abs/path"),
        ("/abs/path/../shared/*", "/abs/shared"),
    ] {
        assert_eq!(root_from_glob(glob), root, "{glob}");
    }
}

#[test]
fn the_root_of_a_relative_pattern() {
    // tsgo `TestWatchRootFromRelativePattern`.
    for (relative, root) in [
        ("**/*", "/workspace/project"),
        ("src/**/*", "/workspace/project/src"),
        ("../shared/*", "/workspace/shared"),
    ] {
        let watcher = FileSystemWatcher {
            glob_pattern: PatternOrRelativePattern::RelativePattern(RelativePattern {
                base_uri: WorkspaceFolderOrURI::URI(Uri("file:///workspace/project".to_owned())),
                pattern: relative.to_owned(),
            }),
            kind: None,
        };
        assert_eq!(watch_root(&watcher).as_deref(), Some(root), "{relative}");
    }
}

#[test]
fn ids_are_kept_and_an_overflow_keeps_the_events() {
    // tsgo `TestWatcher_BookkeepingAndOverflow`.
    let directory = TempDir::new();
    let glob = format!("{}/**/*", directory.path());
    let backend = FakeBackend::new();
    let collected = Collected::default();
    let watcher = fake_watcher(&backend, &collected);
    watcher
        .watch_files("id", &[pattern(glob.clone(), None)])
        .unwrap();
    assert!(
        watcher.watch_files("id", &[pattern(glob, None)]).is_err(),
        "expected duplicate-id error"
    );
    backend.emit_all(
        vec![update(&format!("{}/a.ts", directory.path()))],
        Some(WatchError::Overflow),
    );
    wait_for(|| !collected.events().is_empty(), "events after overflow");
    assert!(
        watcher.unwatch_files("missing").is_err(),
        "expected unknown-id error"
    );
    watcher.unwatch_files("id").unwrap();
    watcher.watch_files("id2", &[]).unwrap();
    watcher.close();
    assert!(
        watcher.watch_files("id3", &[]).is_err(),
        "expected closed error"
    );
}

#[test]
fn a_glob_without_double_star_is_not_recursive() {
    // tsgo `TestWatcher_NonRecursiveGlobIsNotRecursive`.
    let directory = TempDir::new();
    std::fs::create_dir_all(directory.join("sub")).unwrap();
    let sub = format!("{}/sub", directory.path());
    let backend = FakeBackend::new();
    let watcher = fake_watcher(&backend, &Collected::default());
    watcher
        .watch_files(
            "id",
            &[
                pattern(format!("{}/**/*", directory.path()), None),
                pattern(format!("{sub}/*"), None),
            ],
        )
        .unwrap();
    let state = backend.state.lock().unwrap();
    assert_eq!(state.recursive.get(&directory.path()), Some(&true));
    assert_eq!(state.recursive.get(&sub), Some(&false));
    drop(state);
    watcher.close();
}

#[test]
fn a_file_in_a_directory_created_later_reaches_the_session() {
    // tsgo `TestWatcher_RealBackend_MissingThenCreate`.
    let base = TempDir::new();
    let collected = Collected::default();
    let watcher = Watcher::new(
        os_fs(),
        Arc::new(NativeWatchBackend::new()),
        collected.sink(),
        |_| {},
    );
    watcher
        .watch_files("test", &[pattern(format!("{}/pkg/*", base.path()), ALL)])
        .unwrap();
    // The ancestor watch installs.
    std::thread::sleep(Duration::from_millis(200));
    std::fs::create_dir_all(base.join("pkg")).unwrap();
    std::fs::write(base.join("pkg").join("index.ts"), "export {}").unwrap();
    wait_for(
        || {
            collected
                .events()
                .iter()
                .any(|event| event.uri.0.ends_with("/pkg/index.ts"))
        },
        "event for file created in a previously-missing directory",
    );
    watcher.close();
}

#[test]
fn a_missing_directory_is_watched_through_its_ancestor() {
    // tsgo `TestWatcher_MissingDirectoryTracksAncestor`.
    let base = TempDir::new();
    let backend = FakeBackend::new();
    let watcher = fake_watcher(&backend, &Collected::default());
    watcher
        .watch_files("id", &[pattern(format!("{}/pkg/*", base.path()), None)])
        .unwrap();
    assert_eq!(backend.watched_directories(), [base.path()]);
    watcher.unwatch_files("id").unwrap();
    assert!(backend.watched_directories().is_empty());
    watcher.close();
}

#[test]
fn a_missing_directory_is_watched_once_created() {
    // tsgo `TestWatcher_MissingDirectoryPromotesOnCreate`.
    let base = TempDir::new();
    let target = format!("{}/pkg", base.path());
    let backend = FakeBackend::new();
    let collected = Collected::default();
    let watcher = fake_watcher(&backend, &collected);
    watcher
        .watch_files("id", &[pattern(format!("{target}/*"), ALL)])
        .unwrap();
    std::fs::create_dir_all(base.join("pkg")).unwrap();
    std::fs::write(base.join("pkg").join("index.ts"), "export {}").unwrap();
    backend.emit(&base.path(), vec![update(&target)], None);
    wait_for(|| backend.is_watching(&target), "promotion to target watch");
    // The directory and its entry, as created: the session finds the files
    // that came before the watch.
    wait_for(
        || {
            collected.has(FileChangeType::CREATED, "/pkg")
                && collected.has(FileChangeType::CREATED, "/pkg/index.ts")
        },
        "synthetic create events for target and child",
    );
    watcher.close();
}

#[test]
fn the_ancestor_watch_moves_down() {
    // tsgo `TestWatcher_MultiLevelDescend`.
    let base = TempDir::new();
    let backend = FakeBackend::new();
    let watcher = fake_watcher(&backend, &Collected::default());
    watcher
        .watch_files("id", &[pattern(format!("{}/a/b/c/*", base.path()), None)])
        .unwrap();
    assert!(backend.is_watching(&base.path()));
    let mut parent = base.path();
    for relative in ["a", "a/b", "a/b/c"] {
        std::fs::create_dir_all(base.join(relative)).unwrap();
        let directory = format!("{}/{relative}", base.path());
        backend.emit(&parent, vec![update(&directory)], None);
        wait_for(|| backend.is_watching(&directory), relative);
        parent = directory;
    }
    watcher.close();
}

#[test]
fn a_tree_created_at_once_is_reached_in_one_pass() {
    // tsgo `TestWatcher_AtomicTreeCreateRace`.
    let base = TempDir::new();
    let target = format!("{}/a/b/c", base.path());
    let backend = FakeBackend::new();
    let watcher = fake_watcher(&backend, &Collected::default());
    watcher
        .watch_files("id", &[pattern(format!("{target}/*"), None)])
        .unwrap();
    std::fs::create_dir_all(base.join("a/b/c")).unwrap();
    backend.emit(
        &base.path(),
        vec![update(&format!("{}/a", base.path()))],
        None,
    );
    wait_for(
        || backend.is_watching(&target),
        "promote to target in one pass",
    );
    watcher.close();
}

#[test]
fn the_synthetic_creations_follow_the_watchs_depth() {
    // tsgo `TestWatcher_SyntheticCreateDepth`.
    for recursive in [false, true] {
        let base = TempDir::new();
        let target = format!("{}/pkg", base.path());
        let backend = FakeBackend::new();
        let collected = Collected::default();
        let watcher = fake_watcher(&backend, &collected);
        let glob = if recursive {
            format!("{target}/**/*")
        } else {
            format!("{target}/*")
        };
        watcher.watch_files("id", &[pattern(glob, ALL)]).unwrap();
        std::fs::create_dir_all(base.join("pkg/sub")).unwrap();
        std::fs::write(base.join("pkg/top.ts"), "export {}").unwrap();
        std::fs::write(base.join("pkg/sub/deep.ts"), "export {}").unwrap();
        backend.emit(&base.path(), vec![update(&target)], None);
        wait_for(|| backend.is_watching(&target), "promotion");
        wait_for(
            || collected.has(FileChangeType::CREATED, "/pkg/top.ts"),
            "synthetic create for immediate child",
        );
        if recursive {
            wait_for(
                || collected.has(FileChangeType::CREATED, "/pkg/sub/deep.ts"),
                "synthetic create for deep descendant",
            );
        } else {
            // Any wrong deep event would have arrived.
            std::thread::sleep(Duration::from_millis(300));
            assert!(
                !collected.has(FileChangeType::CREATED, "/pkg/sub/deep.ts"),
                "non-recursive watch must not synthesize deep descendant"
            );
        }
        watcher.close();
    }
}

#[test]
fn a_removed_directory_is_watched_again_once_recreated() {
    // tsgo `TestWatcher_TerminatedFallsBackAndRecovers`.
    let base = TempDir::new();
    let target = format!("{}/pkg", base.path());
    std::fs::create_dir_all(base.join("pkg")).unwrap();
    let backend = FakeBackend::new();
    let collected = Collected::default();
    let watcher = fake_watcher(&backend, &collected);
    watcher
        .watch_files("id", &[pattern(format!("{target}/*"), ALL)])
        .unwrap();
    assert!(backend.is_watching(&target));
    // The directory goes; its own deletion comes with the termination, as
    // the real backends report it.
    std::fs::remove_dir_all(base.join("pkg")).unwrap();
    backend.emit(&target, vec![delete(&target)], Some(WatchError::Terminated));
    wait_for(
        || collected.has(FileChangeType::DELETED, "/pkg"),
        "forwarded delete of terminated dir",
    );
    wait_for(
        || backend.is_watching(&base.path()) && !backend.is_watching(&target),
        "fallback to ancestor watch",
    );
    std::fs::create_dir_all(base.join("pkg")).unwrap();
    backend.emit(&base.path(), vec![update(&target)], None);
    wait_for(
        || backend.is_watching(&target),
        "recovery to target watch after recreation",
    );
    watcher.close();
}

#[test]
fn a_failure_of_the_backend_takes_the_id_back() {
    // tsgo `TestWatcher_GenuineFailureRollsBackForRetry`.
    let directory = TempDir::new();
    let glob = format!("{}/*", directory.path());
    let backend = FakeBackend::new();
    let watcher = fake_watcher(&backend, &Collected::default());
    backend
        .state
        .lock()
        .unwrap()
        .fail_directories
        .insert(directory.path(), "too many open files".to_owned());
    assert!(watcher
        .watch_files("id", &[pattern(glob.clone(), None)])
        .is_err());
    // The pressure eases; the id registers again.
    backend
        .state
        .lock()
        .unwrap()
        .fail_directories
        .remove(&directory.path());
    watcher.watch_files("id", &[pattern(glob, None)]).unwrap();
    assert!(backend.is_watching(&directory.path()));
    watcher.close();
}

#[test]
fn a_terminated_watch_keeps_its_events() {
    // tsgo `TestWatcher_WatchTerminatedDoesNotDropEvents`.
    let directory = TempDir::new();
    let backend = FakeBackend::new();
    let collected = Collected::default();
    let watcher = fake_watcher(&backend, &collected);
    watcher
        .watch_files("id", &[pattern(format!("{}/**/*", directory.path()), None)])
        .unwrap();
    backend.emit_all(
        vec![update(&format!("{}/b.ts", directory.path()))],
        Some(WatchError::Terminated),
    );
    wait_for(
        || !collected.events().is_empty(),
        "events with watch-terminated error",
    );
    watcher.close();
}

/// tsgo's `blockingBackend`: a watch that waits for the test.
struct BlockingBackend {
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}

struct NoWatch;

impl WatchHandle for NoWatch {}

impl WatchBackend for BlockingBackend {
    fn watch_directories(
        &self,
        _requests: Vec<WatchRequest>,
    ) -> io::Result<Vec<Box<dyn WatchHandle>>> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        let _ = self.release.lock().unwrap().recv();
        Ok(vec![Box::new(NoWatch)])
    }
}

#[test]
fn closing_during_a_registration_fails_it() {
    // tsgo `TestWatcher_CloseWhileWatchFilesReconciles`.
    let directory = TempDir::new();
    let glob = format!("{}/**/*", directory.path());
    let (entered_sender, entered) = std::sync::mpsc::channel();
    let (release, release_receiver) = std::sync::mpsc::channel();
    let watcher = Arc::new(Watcher::new(
        os_fs(),
        Arc::new(BlockingBackend {
            entered: Mutex::new(Some(entered_sender)),
            release: Mutex::new(release_receiver),
        }),
        |_| {},
        |_| {},
    ));
    let registration = {
        let watcher = Arc::clone(&watcher);
        std::thread::spawn(move || watcher.watch_files("id", &[pattern(glob, None)]))
    };
    entered.recv().unwrap();
    watcher.close();
    release.send(()).unwrap();
    assert!(
        registration.join().unwrap().is_err(),
        "expected WatchFiles to report that the watcher was closed"
    );
}
