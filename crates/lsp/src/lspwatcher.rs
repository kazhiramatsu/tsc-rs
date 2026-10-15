//! tsgo `lsp/lspwatcher`: the server's own file watching, for a client that
//! cannot register watchers (no dynamic registration of
//! `workspace/didChangeWatchedFiles`). Each of the session's
//! `FileSystemWatcher`s becomes a watch of the process's backend, and the
//! changes come back to the session in batches shaped as the client's
//! notification.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use tsc_compiler::watch::{
    WatchBackend, WatchError, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};
use tsc_diagnostics::JsStr;
use tsc_host::vfs::FileSystem;
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, FileSystemWatcher, PatternOrRelativePattern, WatchKind,
    WorkspaceFolderOrURI,
};
use tsc_program::uri::{file_name_to_document_uri, uri_to_file_name};

use crate::gofmt::quote;

/// tsgo `throttleWindow`: as VS Code's parcel watcher integration, the
/// first change of a batch waits this long so that a burst coalesces.
const THROTTLE_WINDOW: Duration = Duration::from_millis(75);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

type OnChanges = dyn Fn(Vec<FileEvent>) + Send + Sync;
type Log = dyn Fn(&str) + Send + Sync;

/// tsgo `Watcher`: the watches of the session's watcher ids.
pub struct Watcher {
    inner: Arc<Inner>,
}

struct Inner {
    fs: Arc<dyn FileSystem>,
    backend: Arc<dyn WatchBackend>,
    on_changes: Box<OnChanges>,
    log: Box<Log>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The watches of each id: one for each of its `FileSystemWatcher`s
    /// (their roots and kinds differ).
    watches: BTreeMap<String, Vec<Arc<Watch>>>,
    closed: bool,
    /// The batch the next flush hands over, by URI.
    pending: BTreeMap<DocumentUri, FileEvent>,
    flush_scheduled: bool,
}

/// tsgo `watch`: one `FileSystemWatcher` of a registration.
///
/// The directory the session asks to watch may not exist yet (each probed
/// but missing package directory becomes a watch) or may go while
/// watched. A watch keeps either a subscription on the directory itself
/// (the target), once it exists, or one on its nearest existing ancestor,
/// not recursive, to see the directory (or a directory on the way) come;
/// the watch then moves down toward the target. When the target appears,
/// the watch reports it and what it holds (its entries, or its tree for a
/// recursive watch) as created, so that the session sees the files that
/// came before the subscription.
struct Watch {
    watcher: Weak<Inner>,
    requested_directory: String,
    kind: WatchKind,
    /// Whether the target subscription watches the directory's tree.
    recursive: bool,
    state: Mutex<WatchState>,
}

#[derive(Default)]
struct WatchState {
    subscription: Option<Box<dyn WatchHandle>>,
    /// The directory the subscription watches.
    watched_directory: String,
    watching_target: bool,
    closed: bool,
}

impl Watcher {
    /// tsgo `New` and `newWithBackend`: watches of `backend`, the
    /// directories' existence and entries from `fs`.
    pub fn new(
        fs: Arc<dyn FileSystem>,
        backend: Arc<dyn WatchBackend>,
        on_changes: impl Fn(Vec<FileEvent>) + Send + Sync + 'static,
        log: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                fs,
                backend,
                on_changes: Box::new(on_changes),
                log: Box::new(log),
                state: Mutex::new(State::default()),
            }),
        }
    }

    /// tsgo `WatchFiles`: watches each watcher under `id`. A directory that
    /// does not exist yet is watched through its nearest existing ancestor,
    /// so the id stays alive; only a failure of the backend (resource
    /// exhaustion on an existing directory) fails, and takes the whole id
    /// back so that the session can register it again.
    pub fn watch_files(&self, id: &str, watchers: &[FileSystemWatcher]) -> Result<(), String> {
        {
            let mut state = lock(&self.inner.state);
            if state.closed {
                return Err("lspwatcher: closed".to_owned());
            }
            if state.watches.contains_key(id) {
                return Err(format!("lspwatcher: watcher {} already exists", quote(id)));
            }
            // Taken before the watches start, so a concurrent call for the
            // same id fails above.
            state.watches.insert(id.to_owned(), Vec::new());
        }
        let mut failed = false;
        for file_system_watcher in watchers {
            let Some(directory) = watch_root(file_system_watcher) else {
                self.inner.log(&format!(
                    "lspwatcher: skipping watcher {}: unrecognized pattern {}",
                    quote(id),
                    quote(&watch_pattern_string(file_system_watcher))
                ));
                continue;
            };
            let watch = Arc::new(Watch {
                watcher: Arc::downgrade(&self.inner),
                requested_directory: directory,
                kind: effective_kind(file_system_watcher),
                recursive: is_recursive_glob(file_system_watcher),
                state: Mutex::new(WatchState::default()),
            });
            if let Err(error) = watch.reconcile(false) {
                self.inner.log(&format!(
                    "lspwatcher: failed to register watcher {} for {}: {error}",
                    quote(id),
                    quote(&watch.requested_directory)
                ));
                watch.close();
                failed = true;
                break;
            }
            let mut state = lock(&self.inner.state);
            if state.closed {
                drop(state);
                watch.close();
                return Err("lspwatcher: closed".to_owned());
            }
            state.watches.entry(id.to_owned()).or_default().push(watch);
        }
        if failed {
            let _ = self.unwatch_files(id);
            return Err(format!(
                "lspwatcher: failed to register one or more watchers for {}",
                quote(id)
            ));
        }
        Ok(())
    }

    /// tsgo `UnwatchFiles`.
    pub fn unwatch_files(&self, id: &str) -> Result<(), String> {
        let watches = lock(&self.inner.state)
            .watches
            .remove(id)
            .ok_or_else(|| format!("lspwatcher: no watcher with id {}", quote(id)))?;
        for watch in watches {
            watch.close();
        }
        Ok(())
    }

    /// tsgo `Close`: every watch stops, and no change is reported after.
    pub fn close(&self) {
        let watches = {
            let mut state = lock(&self.inner.state);
            if state.closed {
                return;
            }
            state.closed = true;
            state.pending.clear();
            std::mem::take(&mut state.watches)
        };
        for watch in watches.into_values().flatten() {
            watch.close();
        }
    }
}

impl Inner {
    fn log(&self, message: &str) {
        (self.log)(message);
    }

    fn watch_directory(
        &self,
        directory: &str,
        recursive: bool,
        events: WatchEvents,
    ) -> io::Result<Box<dyn WatchHandle>> {
        let mut handles = self.backend.watch_directories(vec![WatchRequest {
            directory: directory.to_owned(),
            recursive,
            events,
        }])?;
        Ok(handles.pop().expect("one watch for one request"))
    }

    /// tsgo `forwardEvents`: the backend's changes as the LSP's, for the
    /// next flush.
    fn forward_events(self: &Arc<Self>, kind: WatchKind, events: Vec<WatchEvent>) {
        let mut state = lock(&self.state);
        if state.closed {
            return;
        }
        for event in events {
            let change_type = match event.kind {
                // tsgo's backend tells no creation from a change; the
                // session tells them apart from what it knows, and both
                // make it drop what it had.
                WatchEventKind::Create | WatchEventKind::Update => {
                    if kind.0 & (WatchKind::CREATE.0 | WatchKind::CHANGE.0) == 0 {
                        continue;
                    }
                    FileChangeType::CHANGED
                }
                WatchEventKind::Delete => {
                    if kind.0 & WatchKind::DELETE.0 == 0 {
                        continue;
                    }
                    FileChangeType::DELETED
                }
            };
            let uri = DocumentUri(file_name_to_document_uri(&event.path));
            state.pending.insert(
                uri.clone(),
                FileEvent {
                    uri,
                    r#type: change_type,
                },
            );
        }
        self.schedule_flush(&mut state);
    }

    /// tsgo `emitSyntheticCreates`: once a target is watched again after
    /// it was missing, the directory and its entries (its tree, for a
    /// recursive watch) as created, so that the session finds the files
    /// that came before the watch.
    fn emit_synthetic_creates(self: &Arc<Self>, directory: &str, kind: WatchKind, recursive: bool) {
        if kind.0 & WatchKind::CREATE.0 == 0 {
            return;
        }
        let mut paths = vec![directory.to_owned()];
        if recursive {
            walk_dir(&*self.fs, directory, &mut paths);
        } else {
            let entries = self.fs.accessible_entries(directory);
            for name in entries.files.iter().chain(&entries.directories) {
                paths.push(join(directory, name));
            }
        }
        self.enqueue_synthetic_creates(paths);
    }

    /// tsgo `enqueueSyntheticCreates`: a pending change of a path (a real
    /// deletion) stays.
    fn enqueue_synthetic_creates(self: &Arc<Self>, paths: Vec<String>) {
        let mut state = lock(&self.state);
        if state.closed {
            return;
        }
        for path in paths {
            let uri = DocumentUri(file_name_to_document_uri(&path));
            state
                .pending
                .entry(uri.clone())
                .or_insert_with(|| FileEvent {
                    uri,
                    r#type: FileChangeType::CREATED,
                });
        }
        self.schedule_flush(&mut state);
    }

    /// tsgo `scheduleFlushLocked`.
    fn schedule_flush(self: &Arc<Self>, state: &mut State) {
        if state.flush_scheduled {
            return;
        }
        state.flush_scheduled = true;
        let inner = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(THROTTLE_WINDOW);
            if let Some(inner) = inner.upgrade() {
                inner.flush();
            }
        });
    }

    /// tsgo `flush`.
    fn flush(&self) {
        let pending = {
            let mut state = lock(&self.state);
            if state.closed {
                return;
            }
            state.flush_scheduled = false;
            std::mem::take(&mut state.pending)
        };
        if pending.is_empty() {
            return;
        }
        (self.on_changes)(pending.into_values().collect());
    }
}

impl Watch {
    /// tsgo `close`: the subscription goes, and a reconcile under way
    /// installs none.
    fn close(&self) {
        let subscription = {
            let mut state = lock(&self.state);
            state.closed = true;
            state.watched_directory.clear();
            state.subscription.take()
        };
        drop(subscription);
    }

    /// tsgo `reconcile`: watches the target if it exists, or else moves
    /// the ancestor subscription to the nearest existing ancestor. Called
    /// at registration, on any change the ancestor subscription sees, and
    /// after the target went. `emit_synthetic_creates`: whether reaching
    /// the target reports what it holds (not at registration, when the
    /// session knows the files). Fails only when the backend cannot
    /// watch; a missing target is watched through an ancestor.
    fn reconcile(self: &Arc<Self>, emit_synthetic_creates: bool) -> io::Result<()> {
        let Some(watcher) = self.watcher.upgrade() else {
            return Ok(());
        };
        let mut emit_synthetic_creates = emit_synthetic_creates;
        let mut state = lock(&self.state);
        loop {
            if state.closed {
                return Ok(());
            }
            if watcher.fs.is_dir(&self.requested_directory) {
                if state.watching_target && state.subscription.is_some() {
                    return Ok(());
                }
                let target = self.requested_directory.clone();
                let subscription = watcher.watch_directory(
                    &target,
                    self.recursive,
                    self.target_callback(&target),
                )?;
                let previous = state.subscription.replace(subscription);
                state.watched_directory = target.clone();
                state.watching_target = true;
                drop(previous);
                if emit_synthetic_creates {
                    watcher.emit_synthetic_creates(&target, self.kind, self.recursive);
                }
                return Ok(());
            }
            let Some(ancestor) = nearest_existing_ancestor(&*watcher.fs, &self.requested_directory)
            else {
                // Nothing on the way exists, not even the root.
                let previous = state.subscription.take();
                state.watched_directory.clear();
                state.watching_target = false;
                drop(previous);
                return Ok(());
            };
            if !state.watching_target
                && state.subscription.is_some()
                && state.watched_directory == ancestor
            {
                return Ok(());
            }
            let subscription =
                watcher.watch_directory(&ancestor, false, self.ancestor_callback())?;
            let previous = state.subscription.replace(subscription);
            state.watched_directory = ancestor;
            state.watching_target = false;
            drop(previous);
            // The target may have come between the check above and this
            // subscription (a tree created at once): go on down, and a
            // target reached from here was missing.
            emit_synthetic_creates = true;
        }
    }

    /// tsgo `targetCallback`: hands the changes to the session and, once
    /// the directory went, falls back to an ancestor so that the watch
    /// comes back with it.
    fn target_callback(self: &Arc<Self>, directory: &str) -> WatchEvents {
        let watch = Arc::downgrade(self);
        let directory = directory.to_owned();
        WatchEvents::new(move |events, error| {
            let Some(watch) = watch.upgrade() else {
                return;
            };
            let Some(watcher) = watch.watcher.upgrade() else {
                return;
            };
            let terminated = match error {
                Some(WatchError::Overflow) => {
                    watcher.log(&format!(
                        "lspwatcher: watch overflow in {} (some events may have been dropped): {}",
                        quote(&directory),
                        WatchError::Overflow
                    ));
                    false
                }
                Some(WatchError::Terminated) => {
                    watcher.log(&format!(
                        "lspwatcher: watch terminated in {} (directory removed): {}",
                        quote(&directory),
                        WatchError::Terminated
                    ));
                    true
                }
                None => false,
            };
            if !events.is_empty() {
                watcher.forward_events(watch.kind, events);
            }
            if terminated {
                watch.handle_terminated();
            }
        })
    }

    /// tsgo `handleTerminated`: the dead subscription goes first (or the
    /// reconcile would take the target for watched), then the watch falls
    /// back to an ancestor.
    fn handle_terminated(self: &Arc<Self>) {
        let previous = {
            let mut state = lock(&self.state);
            if state.closed {
                return;
            }
            state.watched_directory.clear();
            state.watching_target = false;
            state.subscription.take()
        };
        drop(previous);
        let _ = self.reconcile(true);
    }

    /// tsgo `ancestorCallback`: an ancestor's changes concern directories
    /// the session does not track; the watch only looks again.
    fn ancestor_callback(self: &Arc<Self>) -> WatchEvents {
        let watch = Arc::downgrade(self);
        WatchEvents::new(move |_, _| {
            if let Some(watch) = watch.upgrade() {
                let _ = watch.reconcile(true);
            }
        })
    }
}

/// tsgo `nearestExistingAncestor`: `directory` or the deepest existing
/// directory above it; none when not even the root exists.
fn nearest_existing_ancestor(fs: &dyn FileSystem, directory: &str) -> Option<String> {
    let mut directory = directory.to_owned();
    loop {
        if fs.is_dir(&directory) {
            return Some(directory);
        }
        let parent = tsc_program::get_directory_path(JsStr::from_str(&directory))
            .to_string_lossy()
            .into_owned();
        if parent == directory {
            return None;
        }
        directory = parent;
    }
}

fn join(directory: &str, name: &str) -> String {
    format!("{}/{name}", directory.trim_end_matches('/'))
}

/// tsgo `vfs.WalkDir` below `directory`, in name order: a linked
/// directory is reported, not entered.
fn walk_dir(fs: &dyn FileSystem, directory: &str, paths: &mut Vec<String>) {
    let entries = fs.accessible_entries(directory);
    let mut names = entries
        .directories
        .iter()
        .map(|name| (name, true))
        .chain(entries.files.iter().map(|name| (name, false)))
        .collect::<Vec<_>>();
    names.sort();
    for (name, is_directory) in names {
        let path = join(directory, name);
        paths.push(path.clone());
        if is_directory && !entries.symlinks.contains(name) {
            walk_dir(fs, &path, paths);
        }
    }
}

/// tsgo `watchRoot`: the directory to watch for a watcher, from its
/// pattern (an absolute path) or its relative pattern (a `file:` base
/// URI). The session's patterns are `<dir>/**/*` (recursive) or `<dir>/*`,
/// so the root is all before the first glob character.
fn watch_root(file_system_watcher: &FileSystemWatcher) -> Option<String> {
    match &file_system_watcher.glob_pattern {
        PatternOrRelativePattern::Pattern(pattern) => {
            let root = root_from_glob(pattern);
            (!root.is_empty() && tsc_program::path_is_absolute(root.as_str())).then_some(root)
        }
        PatternOrRelativePattern::RelativePattern(relative) => {
            let WorkspaceFolderOrURI::URI(base) = &relative.base_uri else {
                return None;
            };
            let base = uri_to_file_name(&base.0)
                .ok()
                .filter(|base| !base.is_empty())?;
            let root = root_from_glob(&relative.pattern);
            let combined =
                tsc_program::combine_paths(JsStr::from_str(&base), JsStr::from_str(&root));
            Some(
                tsc_program::normalize_path(combined.as_js())
                    .to_string_lossy()
                    .into_owned(),
            )
        }
    }
}

/// tsgo `rootFromGlob`.
fn root_from_glob(pattern: &str) -> String {
    let pattern = tsc_program::normalize_slashes(JsStr::from_str(pattern))
        .to_string_lossy()
        .into_owned();
    let directory = match pattern.find(['*', '?', '[', '{']) {
        Some(meta) => pattern[..meta].trim_end_matches('/'),
        None => pattern.trim_end_matches('/'),
    };
    if directory.is_empty() {
        return String::new();
    }
    tsc_program::normalize_path(JsStr::from_str(directory))
        .to_string_lossy()
        .into_owned()
}

/// tsgo `watchPatternString`.
fn watch_pattern_string(file_system_watcher: &FileSystemWatcher) -> String {
    match &file_system_watcher.glob_pattern {
        PatternOrRelativePattern::Pattern(pattern) => pattern.clone(),
        PatternOrRelativePattern::RelativePattern(relative) => {
            let base = match &relative.base_uri {
                WorkspaceFolderOrURI::URI(uri) => uri.0.as_str(),
                WorkspaceFolderOrURI::WorkspaceFolder(_) => "",
            };
            format!("{base}/{}", relative.pattern)
        }
    }
}

/// tsgo `isRecursiveGlob`: a `**` segment watches the tree (the session's
/// granular watches are `<dir>/*`).
fn is_recursive_glob(file_system_watcher: &FileSystemWatcher) -> bool {
    watch_pattern_string(file_system_watcher).contains("**")
}

/// tsgo `effectiveKind`.
fn effective_kind(file_system_watcher: &FileSystemWatcher) -> WatchKind {
    file_system_watcher.kind.unwrap_or(WatchKind(
        WatchKind::CREATE.0 | WatchKind::CHANGE.0 | WatchKind::DELETE.0,
    ))
}

#[cfg(test)]
#[path = "../tests/unit/lspwatcher.rs"]
mod tests;
