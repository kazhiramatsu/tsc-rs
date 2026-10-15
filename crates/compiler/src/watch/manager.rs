//! The directory watches of a watch run and the events they report (tsgo
//! `execute/watchmanager`): a [`WatchBackend`] watches directories and
//! delivers their changes to a queue; the [`WatchManager`] decides which
//! directories to watch, keeps the backend's watches in step with that set,
//! and hands the accumulated changes to the next cycle.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// What happened to a path (tsgo `fswatch.EventKind`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchEventKind {
    Create,
    Update,
    Delete,
}

/// A change a watched directory reports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchEvent {
    pub kind: WatchEventKind,
    pub path: String,
}

/// What keeps a watch from reporting its changes (tsgo `fswatch`'s
/// errors).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchError {
    /// The backend missed events (tsgo `ErrOverflow`).
    Overflow,
    /// The watched directory went away (tsgo `ErrWatchTerminated`): the
    /// watch reports nothing more.
    Terminated,
}

impl std::fmt::Display for WatchError {
    /// tsgo's messages of the errors.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Overflow => "fswatch: event overflow; some changes were missed",
            Self::Terminated => "fswatch: watch terminated",
        })
    }
}

type WatchCallback = dyn Fn(Vec<WatchEvent>, Option<WatchError>) + Send + Sync;

/// Where a backend delivers the events of a watch (tsgo `WatchCallback`).
/// Cloning shares the callback.
#[derive(Clone)]
pub struct WatchEvents {
    callback: Arc<WatchCallback>,
}

impl WatchEvents {
    /// Events that go to `callback`, with the error the watch reports.
    pub fn new(
        callback: impl Fn(Vec<WatchEvent>, Option<WatchError>) + Send + Sync + 'static,
    ) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Reports changes.
    pub fn send(&self, events: impl IntoIterator<Item = WatchEvent>) {
        let events = events.into_iter().collect::<Vec<_>>();
        if !events.is_empty() {
            (self.callback)(events, None);
        }
    }

    /// Reports that the backend missed events.
    pub fn overflow(&self) {
        (self.callback)(Vec::new(), Some(WatchError::Overflow));
    }

    /// Reports that the watched directory went away.
    pub fn terminate(&self) {
        (self.callback)(Vec::new(), Some(WatchError::Terminated));
    }
}

/// One directory a watch run asks its backend to watch.
pub struct WatchRequest {
    pub directory: String,
    /// The whole tree below the directory, not only its entries.
    pub recursive: bool,
    pub events: WatchEvents,
}

/// A watch a backend started; dropping it stops the watch.
pub trait WatchHandle: Send {}

/// Watches directories for a watch run (tsgo `watchmanager.WatchBackend`).
/// The process watches the operating system's file system; a test or a
/// WebAssembly embedding provides its own.
pub trait WatchBackend: Send + Sync {
    /// Starts one watch per request, in order. A failure starts none.
    fn watch_directories(
        &self,
        requests: Vec<WatchRequest>,
    ) -> io::Result<Vec<Box<dyn WatchHandle>>>;
}

#[derive(Default)]
struct Pending {
    changed: BTreeMap<String, WatchEventKind>,
    overflow: bool,
    /// Events arrived since the last drain (a cycle is due).
    signalled: bool,
    /// The watches whose directory went away, by directory and generation.
    terminated: Vec<(String, u64)>,
}

struct EventQueue {
    pending: Mutex<Pending>,
    signal: Condvar,
}

impl EventQueue {
    fn lock(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// tsgo `onWatchEvents`: records changes for the next cycle. Paths a
    /// watch run ignores (`.git`, a package manager's `node_modules/.*`
    /// stores, editor lock files) are dropped, as tsgo's watches drop them.
    fn record(&self, events: Vec<WatchEvent>) {
        let mut pending = self.lock();
        let mut recorded = false;
        for event in events {
            if should_ignore_watch_path(&event.path) {
                continue;
            }
            pending.changed.insert(event.path, event.kind);
            recorded = true;
        }
        if recorded {
            pending.signalled = true;
            self.signal.notify_all();
        }
    }

    /// The next cycle rebuilds from scratch.
    fn overflow(&self) {
        let mut pending = self.lock();
        pending.overflow = true;
        pending.signalled = true;
        self.signal.notify_all();
    }

    /// tsgo `handleWatchTerminated`: the next cycle drops the watch, starts
    /// its directory's again and rebuilds from scratch.
    fn terminated(&self, directory: String, generation: u64) {
        self.lock().terminated.push((directory, generation));
        self.overflow();
    }
}

/// The changes of a cycle (tsgo `DrainEvents`).
#[derive(Debug, Default)]
pub(crate) struct DrainedEvents {
    pub(crate) changed: BTreeMap<String, WatchEventKind>,
    pub(crate) overflow: bool,
}

impl DrainedEvents {
    pub(crate) fn is_empty(&self) -> bool {
        self.changed.is_empty() && !self.overflow
    }
}

/// The backend a manager watches with: the test harness's, or one the run
/// owns (the process's).
pub(crate) enum Backend<'a> {
    Borrowed(&'a dyn WatchBackend),
    Owned(Box<dyn WatchBackend + 'a>),
}

impl Backend<'_> {
    fn get(&self) -> &dyn WatchBackend {
        match self {
            Self::Borrowed(backend) => *backend,
            Self::Owned(backend) => &**backend,
        }
    }
}

struct Watched {
    recursive: bool,
    /// Tells a watch of the directory from the ones before it (tsgo's
    /// `watchedDir` identity).
    generation: u64,
    _handle: Box<dyn WatchHandle>,
}

/// tsgo `WatchManager`.
pub(crate) struct WatchManager<'a> {
    backend: Option<Backend<'a>>,
    watched: BTreeMap<String, Watched>,
    queue: Arc<EventQueue>,
    next_generation: u64,
}

impl<'a> WatchManager<'a> {
    pub(crate) fn new(backend: Option<Backend<'a>>) -> Self {
        Self {
            backend,
            watched: BTreeMap::new(),
            queue: Arc::new(EventQueue {
                pending: Mutex::new(Pending::default()),
                signal: Condvar::new(),
            }),
            next_generation: 0,
        }
    }

    pub(crate) fn drain_events(&self) -> DrainedEvents {
        let mut pending = self.queue.lock();
        pending.signalled = false;
        DrainedEvents {
            changed: std::mem::take(&mut pending.changed),
            overflow: std::mem::take(&mut pending.overflow),
        }
    }

    pub(crate) fn force_overflow(&self) {
        self.queue.lock().overflow = true;
    }

    /// Blocks until a change arrives, then waits for the changes to settle:
    /// until none arrived for `quiet`, or for at most `longest` (tsgo's
    /// debounce, 50 ms and 500 ms).
    pub(crate) fn wait_for_events(&self, quiet: Duration, longest: Duration) {
        let mut pending = self.queue.lock();
        while !pending.signalled {
            pending = self
                .queue
                .signal
                .wait(pending)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let started = Instant::now();
        loop {
            let count = pending.changed.len();
            let (next, timeout) = self
                .queue
                .signal
                .wait_timeout(pending, quiet)
                .unwrap_or_else(PoisonError::into_inner);
            pending = next;
            let settled = timeout.timed_out() && pending.changed.len() == count;
            if settled || started.elapsed() >= longest {
                return;
            }
        }
    }

    /// Whether a watched directory contains `path` (tsgo
    /// `IsPathUnderWatch`).
    pub(crate) fn is_path_under_watch(&self, path: &str, case_sensitive: bool) -> bool {
        self.watched
            .keys()
            .any(|directory| contains_path(directory, path, case_sensitive))
    }

    /// tsgo `ReconcileWatches`: starts the directories `desired` adds,
    /// stops the ones it drops, and restarts the ones whose recursion
    /// changed.
    pub(crate) fn reconcile(&mut self, desired: &BTreeMap<String, bool>) -> io::Result<()> {
        let Some(backend) = &self.backend else {
            return Ok(());
        };
        let terminated = std::mem::take(&mut self.queue.lock().terminated);
        for (directory, generation) in terminated {
            if self
                .watched
                .get(&directory)
                .is_some_and(|watched| watched.generation == generation)
            {
                self.watched.remove(&directory);
            }
        }
        let mut updates = Vec::new();
        let mut changes = Vec::new();
        self.watched
            .retain(|directory, watched| match desired.get(directory) {
                None => false,
                Some(&recursive) if recursive != watched.recursive => {
                    changes.push((directory.clone(), recursive));
                    false
                }
                Some(_) => true,
            });
        for (directory, &recursive) in desired {
            if !self.watched.contains_key(directory)
                && !changes.iter().any(|(changed, _)| changed == directory)
            {
                updates.push((directory.clone(), recursive));
            }
        }
        updates.extend(changes);
        if updates.is_empty() {
            return Ok(());
        }
        let first_generation = self.next_generation;
        self.next_generation += updates.len() as u64;
        let requests = updates
            .iter()
            .zip(first_generation..)
            .map(|((directory, recursive), generation)| {
                let queue = Arc::clone(&self.queue);
                let watched = directory.clone();
                WatchRequest {
                    directory: directory.clone(),
                    recursive: *recursive,
                    events: WatchEvents::new(move |events, error| match error {
                        None => queue.record(events),
                        Some(WatchError::Overflow) => queue.overflow(),
                        Some(WatchError::Terminated) => {
                            queue.terminated(watched.clone(), generation);
                        }
                    }),
                }
            })
            .collect();
        let handles = backend.get().watch_directories(requests)?;
        for (((directory, recursive), handle), generation) in
            updates.into_iter().zip(handles).zip(first_generation..)
        {
            self.watched.insert(
                directory,
                Watched {
                    recursive,
                    generation,
                    _handle: handle,
                },
            );
        }
        Ok(())
    }
}

/// tsgo `ResolveDesiredDirs`: a directory that does not exist is watched
/// through its nearest existing ancestor, non-recursively; a directory with
/// no watchable ancestor is not watched.
pub(crate) fn resolve_desired_dirs(
    desired: &BTreeMap<String, bool>,
    directory_exists: &dyn Fn(&str) -> bool,
) -> BTreeMap<String, bool> {
    let mut resolved = BTreeMap::new();
    for (directory, &recursive) in desired {
        let mut watch = directory.clone();
        let mut watch_recursive = recursive;
        while !directory_exists(&watch) {
            let parent = directory_path(&watch);
            if parent == watch {
                break;
            }
            watch = parent;
            watch_recursive = false;
        }
        if !directory_exists(&watch) || !can_watch_directory(&watch) {
            continue;
        }
        let entry = resolved.entry(watch).or_insert(false);
        *entry = *entry || watch_recursive;
    }
    resolved
}

/// The directories a watch run wants and whether each covers a path
/// (tsgo `DirWatchSet`): a directory in the set, or below a recursive one.
pub(crate) struct DirWatchSet {
    case_sensitive: bool,
    /// By the canonical name: the first spelling, and the recursion.
    directories: BTreeMap<String, (String, bool)>,
}

impl DirWatchSet {
    pub(crate) fn new(case_sensitive: bool) -> Self {
        Self {
            case_sensitive,
            directories: BTreeMap::new(),
        }
    }

    fn canonical(&self, directory: &str) -> String {
        if self.case_sensitive {
            directory.to_owned()
        } else {
            tsc_host::to_file_name_lower_case(directory)
        }
    }

    pub(crate) fn set(&mut self, directory: &str, recursive: bool) {
        let entry = self
            .directories
            .entry(self.canonical(directory))
            .or_insert_with(|| (directory.to_owned(), false));
        entry.1 = entry.1 || recursive;
    }

    pub(crate) fn covered(&self, directory: &str) -> bool {
        let mut directory = self.canonical(directory);
        if self.directories.contains_key(&directory) {
            return true;
        }
        let root_length = root_length(&directory);
        while directory.len() > root_length {
            directory = directory_path(&directory);
            if self
                .directories
                .get(&directory)
                .is_some_and(|(_, recursive)| *recursive)
            {
                return true;
            }
        }
        false
    }

    pub(crate) fn into_directories(self) -> BTreeMap<String, bool> {
        self.directories.into_values().collect()
    }
}

/// tsgo `ShouldIgnoreWatchPath`.
pub(crate) fn should_ignore_watch_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.ends_with("/.git")
        || path.contains("/.git/")
        || path.contains("/node_modules/.")
        || path.contains("/.#")
}

/// tsgo `CanWatchDirectory`: not a root, and deeper than the system's
/// shared directories (`/home/<user>`, `C:/Users/<user>`).
pub(crate) fn can_watch_directory(directory: &str) -> bool {
    let components = path_components(directory);
    let length = components.len();
    if length <= 2 {
        return false;
    }
    length > perceived_os_root_length_for_watching(&components) + 1
}

/// tsgo `PerceivedOsRootLengthForWatching`.
fn perceived_os_root_length_for_watching(components: &[&str]) -> usize {
    let length = components.len();
    if length <= 1 {
        return 1;
    }
    let root = components[0];
    let mut index_after_os_root = 1;
    let is_volume = |text: &str| {
        let bytes = text.as_bytes();
        bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
    };
    let mut is_dos_style = is_volume(root);
    if root != "/" && !is_dos_style && components.len() > 1 {
        let second = components[1].as_bytes();
        if second.len() >= 2 && second[0].is_ascii_alphabetic() && components[1].ends_with('$') {
            if length == 2 {
                return 2;
            }
            index_after_os_root = 2;
            is_dos_style = true;
        }
    }
    if is_dos_style
        && (index_after_os_root >= length
            || !components[index_after_os_root].eq_ignore_ascii_case("users"))
    {
        return index_after_os_root;
    }
    if index_after_os_root < length
        && components[index_after_os_root].eq_ignore_ascii_case("workspaces")
    {
        return index_after_os_root + 1;
    }
    index_after_os_root + 2
}

/// tsgo `GetPathComponents` of a normalized path: the root (`/`, `c:/`,
/// `//server/`) and the names after it.
fn path_components(path: &str) -> Vec<&str> {
    let root = root_length(path);
    let mut components = vec![&path[..root]];
    components.extend(path[root..].split('/').filter(|part| !part.is_empty()));
    components
}

/// tsgo `GetRootLength` for the forms a watched path takes.
pub(crate) fn root_length(path: &str) -> usize {
    let bytes = path.as_bytes();
    if bytes.first() == Some(&b'/') {
        if bytes.get(1) == Some(&b'/') {
            // `//server/share/`
            let rest = &path[2..];
            return match rest.find('/') {
                Some(index) => 2 + index + 1,
                None => path.len(),
            };
        }
        return 1;
    }
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return if bytes.get(2) == Some(&b'/') { 3 } else { 2 };
    }
    if let Some(index) = path.find("://") {
        return path[index + 3..]
            .find('/')
            .map_or(path.len(), |slash| index + 3 + slash + 1);
    }
    0
}

/// tsgo `GetDirectoryPath`.
pub(crate) fn directory_path(path: &str) -> String {
    let root = root_length(path);
    let trimmed = path.trim_end_matches('/');
    if trimmed.len() <= root {
        return path[..root.min(path.len())].to_owned();
    }
    match trimmed[root..].rfind('/') {
        Some(index) => trimmed[..root + index].to_owned(),
        None => trimmed[..root].to_owned(),
    }
}

/// tsgo `ContainsPath`: `path` is `directory` or below it.
pub(crate) fn contains_path(directory: &str, path: &str, case_sensitive: bool) -> bool {
    let (directory, path) = if case_sensitive {
        (directory.to_owned(), path.to_owned())
    } else {
        (
            tsc_host::to_file_name_lower_case(directory),
            tsc_host::to_file_name_lower_case(path),
        )
    };
    let directory = directory.trim_end_matches('/');
    path == directory
        || path
            .strip_prefix(directory)
            .is_some_and(|rest| rest.starts_with('/') || directory.ends_with(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that keeps each watch's events.
    #[derive(Default)]
    struct Recording {
        requests: Mutex<Vec<(String, WatchEvents)>>,
    }

    struct Handle;

    impl WatchHandle for Handle {}

    impl WatchBackend for Recording {
        fn watch_directories(
            &self,
            requests: Vec<WatchRequest>,
        ) -> io::Result<Vec<Box<dyn WatchHandle>>> {
            let mut recorded = self.requests.lock().unwrap();
            Ok(requests
                .into_iter()
                .map(|request| {
                    recorded.push((request.directory, request.events));
                    Box::new(Handle) as Box<dyn WatchHandle>
                })
                .collect())
        }
    }

    #[test]
    fn a_terminated_watch_starts_again_with_a_rebuild() {
        // tsgo `handleWatchTerminated`.
        let backend = Recording::default();
        let mut manager = WatchManager::new(Some(Backend::Borrowed(&backend)));
        let desired = BTreeMap::from([("/project/src".to_owned(), true)]);
        manager.reconcile(&desired).unwrap();
        let events = backend.requests.lock().unwrap()[0].1.clone();
        events.terminate();
        assert!(manager.drain_events().overflow);
        manager.reconcile(&desired).unwrap();
        {
            let requests = backend.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[1].0, "/project/src");
        }
        // A late end of the old watch leaves the new one.
        events.terminate();
        manager.reconcile(&desired).unwrap();
        assert_eq!(backend.requests.lock().unwrap().len(), 2);
    }

    #[test]
    fn watchable_directories_follow_tsgo() {
        // tsgo `CanWatchDirectory`: deeper than the perceived root (two
        // names below `/`, one below a volume, three below `c:/Users`) plus
        // one more name.
        assert!(!can_watch_directory("/"));
        assert!(!can_watch_directory("/home"));
        assert!(!can_watch_directory("/home/src"));
        assert!(!can_watch_directory("/home/src/workspaces"));
        assert!(can_watch_directory("/home/src/workspaces/project"));
        assert!(can_watch_directory("/home/src/workspaces/node_modules"));
        assert!(can_watch_directory("/home/src/tslibs/TS/Lib"));
        assert!(!can_watch_directory("/user/username"));
        assert!(!can_watch_directory("/user/username/projects"));
        assert!(can_watch_directory("/user/username/projects/sample1"));
        assert!(!can_watch_directory("c:/"));
        assert!(!can_watch_directory("c:/project"));
        assert!(can_watch_directory("c:/project/src"));
        assert!(!can_watch_directory("c:/Users/name"));
        assert!(!can_watch_directory("c:/Users/name/project"));
        assert!(can_watch_directory("c:/Users/name/project/src"));
        // `//server/c$` is a volume; `//server/share` is not.
        assert!(!can_watch_directory("//server/c$/dir"));
        assert!(can_watch_directory("//server/c$/dir/sub"));
        assert!(!can_watch_directory("//server/share/dir/sub"));
        assert!(can_watch_directory("//server/share/dir/sub/deep"));
    }

    #[test]
    fn missing_directories_fall_back_to_an_ancestor() {
        let exists = |path: &str| {
            matches!(
                path,
                "/" | "/home"
                    | "/home/src"
                    | "/home/src/workspaces"
                    | "/home/src/workspaces/project"
            )
        };
        let desired = BTreeMap::from([
            ("/home/src/workspaces/project/lib".to_owned(), true),
            ("/home/src/workspaces/node_modules".to_owned(), false),
            ("/home/src/workspaces/project".to_owned(), true),
        ]);
        let resolved = resolve_desired_dirs(&desired, &exists);
        assert_eq!(
            resolved,
            BTreeMap::from([("/home/src/workspaces/project".to_owned(), true)])
        );
    }

    // tsgo `watchmanager_test.go`: the `DirWatchSet` tests.
    #[test]
    fn dir_watch_set_coverage() {
        let mut set = DirWatchSet::new(true);
        set.set("/repo/src", true);
        set.set("/repo/config", false);
        set.set("/repo/node_modules/a", false);
        for (directory, want) in [
            ("/repo/src", true),
            ("/repo/src/nested", true),
            ("/repo/src/nested/deep", true),
            ("/repo/config", true),
            ("/repo/config/nested", false),
            ("/repo/node_modules/a", true),
            ("/repo/node_modules/b", false),
            ("/repo", false),
            ("/other", false),
        ] {
            assert_eq!(set.covered(directory), want, "{directory}");
        }
    }

    #[test]
    fn dir_watch_set_case_sensitive() {
        let mut set = DirWatchSet::new(true);
        set.set("/repo/node_modules/a", false);
        set.set("/repo/Src", true);
        assert!(set.covered("/repo/node_modules/a"));
        assert!(!set.covered("/repo/node_modules/A"));
        assert!(set.covered("/repo/Src/nested"));
        assert!(!set.covered("/repo/src/nested"));
    }

    #[test]
    fn dir_watch_set_case_insensitive() {
        let mut set = DirWatchSet::new(false);
        set.set("/repo/node_modules/a", false);
        set.set("/repo/Src", true);
        assert!(set.covered("/repo/node_modules/A"));
        assert!(set.covered("/REPO/NODE_MODULES/a"));
        assert!(set.covered("/repo/src/nested/deep"));
    }

    #[test]
    fn dir_watch_set_canonical_dedup() {
        let mut insensitive = DirWatchSet::new(false);
        insensitive.set("/repo/Node_Modules/PkgName", false);
        insensitive.set("/repo/node_modules/pkgname", false);
        assert_eq!(
            insensitive.into_directories(),
            BTreeMap::from([("/repo/Node_Modules/PkgName".to_owned(), false)])
        );
        let mut sensitive = DirWatchSet::new(true);
        sensitive.set("/repo/Node_Modules/PkgName", false);
        sensitive.set("/repo/node_modules/pkgname", false);
        assert_eq!(sensitive.into_directories().len(), 2);
    }

    #[test]
    fn dir_watch_set_upgrades_and_never_downgrades() {
        let mut set = DirWatchSet::new(true);
        set.set("/repo/src", false);
        assert!(set.covered("/repo/src"));
        assert!(!set.covered("/repo/src/nested"));
        set.set("/repo/src", true);
        assert!(set.covered("/repo/src/nested"));
        set.set("/repo/src", false);
        assert!(set.covered("/repo/src/nested"));
        set.set("/repo/a", false);
        set.set("/repo/a", false);
        assert_eq!(
            set.into_directories(),
            BTreeMap::from([
                ("/repo/a".to_owned(), false),
                ("/repo/src".to_owned(), true)
            ])
        );
    }

    #[test]
    fn ignored_paths() {
        assert!(should_ignore_watch_path("/p/.git"));
        assert!(should_ignore_watch_path("/p/.git/index"));
        assert!(should_ignore_watch_path("/p/node_modules/.pnpm/x"));
        assert!(should_ignore_watch_path("/p/.#a.ts"));
        assert!(!should_ignore_watch_path("/p/a.ts"));
    }

    #[test]
    fn directory_paths() {
        assert_eq!(directory_path("/a/b/c.ts"), "/a/b");
        assert_eq!(directory_path("/a"), "/");
        assert_eq!(directory_path("/"), "/");
        assert_eq!(directory_path("c:/a"), "c:/");
        assert!(contains_path("/a/b", "/a/b/c", true));
        assert!(!contains_path("/a/b", "/a/bc", true));
        assert!(contains_path("/A/b", "/a/B/c", false));
    }
}
