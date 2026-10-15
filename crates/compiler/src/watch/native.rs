//! The process's watch backend: the operating system's file-system
//! notifications through the `notify` crate (FSEvents on macOS, inotify on
//! Linux, ReadDirectoryChangesW on Windows, kqueue on the BSDs). One
//! notifier serves every watch of the backend, and a thread of the backend
//! hands each watch the events under its directory (tsgo `fswatch`), so a
//! watch's callback may start and stop watches.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::manager::{
    WatchBackend, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One watch of the backend.
struct Registration {
    /// The directory's real path, the notifier's spelling of the paths
    /// under it.
    real: PathBuf,
    /// The directory as the watch asked for it: tsgo keeps an event rooted
    /// at the watched directory's spelling, so `/tmp/p` stays `/tmp/p` when
    /// the system reports `/private/tmp/p`.
    spelling: String,
    recursive: bool,
    events: WatchEvents,
    /// The directory went away (tsgo `ErrWatchTerminated`): the watch
    /// reports nothing more.
    terminated: bool,
}

impl Registration {
    /// Whether the watch reports a change of `path` (not the directory
    /// itself).
    fn covers(&self, path: &Path) -> bool {
        if self.recursive {
            path != self.real && path.starts_with(&self.real)
        } else {
            path.parent() == Some(self.real.as_path())
        }
    }

    /// `path` under the directory's spelling.
    fn spelled(&self, path: &Path) -> String {
        let rest = path
            .strip_prefix(&self.real)
            .map(|rest| rest.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        if rest.is_empty() {
            self.spelling.clone()
        } else {
            format!("{}/{rest}", self.spelling.trim_end_matches('/'))
        }
    }
}

#[derive(Default)]
struct Registry {
    registrations: BTreeMap<u64, Registration>,
    next_id: u64,
}

/// What the notifier watches of a directory: how many watches want it, how
/// many of those want its whole tree, and whether the notifier watches the
/// tree.
struct Watched {
    count: usize,
    recursive: usize,
    tree: bool,
}

type Sender = mpsc::Sender<notify::Result<notify::Event>>;

struct Notifier {
    watcher: Option<RecommendedWatcher>,
    /// Where the notifiers' events go: the thread that hands them out.
    sender: Option<Sender>,
    watched: BTreeMap<PathBuf, Watched>,
}

/// A change of the notifier's directories.
#[derive(Clone, Copy)]
enum Change<'a> {
    Add(&'a Path, bool),
    Remove(&'a Path),
    /// The tree, or only the entries.
    Mode(&'a Path, bool),
}

struct Inner {
    notifier: Mutex<Notifier>,
    registry: Arc<Mutex<Registry>>,
}

/// The process's watch backend.
pub struct NativeWatchBackend {
    inner: Arc<Inner>,
}

impl Default for NativeWatchBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeWatchBackend {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                notifier: Mutex::new(Notifier {
                    watcher: None,
                    sender: None,
                    watched: BTreeMap::new(),
                }),
                registry: Arc::default(),
            }),
        }
    }

    /// tsgo `HasFastRecursiveBackend`: whether the system watches a whole
    /// tree at once (FSEvents, ReadDirectoryChangesW) rather than a
    /// directory at a time.
    pub fn has_fast_recursive_backend() -> bool {
        cfg!(any(target_os = "macos", windows))
    }

    fn watch(&self, request: WatchRequest) -> io::Result<Box<dyn WatchHandle>> {
        // tsgo `WatchDirectories`: an absolute directory (a bundled
        // library's is not), and `validateWatchDirectory`, with Go's messages.
        let directory = PathBuf::from(&request.directory);
        if !directory.is_absolute() {
            return Err(io::Error::other("fswatch: path must be absolute"));
        }
        match std::fs::metadata(&directory) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::NotADirectory,
                    "not a directory",
                ))
            }
            Err(error) => {
                let text = match error.kind() {
                    io::ErrorKind::NotFound => "no such file or directory".to_owned(),
                    io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
                    _ => error.to_string(),
                };
                return Err(io::Error::new(
                    error.kind(),
                    format!("stat {}: {text}", request.directory),
                ));
            }
        }
        let real = std::fs::canonicalize(&directory)?;
        let mut notifier = lock(&self.inner.notifier);
        // Registered first, so that the changes right after the notifier
        // starts reach the watch.
        let id = {
            let mut registry = lock(&self.inner.registry);
            let id = registry.next_id;
            registry.next_id += 1;
            registry.registrations.insert(
                id,
                Registration {
                    real: real.clone(),
                    spelling: request.directory,
                    recursive: request.recursive,
                    events: request.events,
                    terminated: false,
                },
            );
            id
        };
        if let Err(error) = notifier.start(&self.inner.registry, &real, request.recursive) {
            lock(&self.inner.registry).registrations.remove(&id);
            return Err(error);
        }
        Ok(Box::new(NativeWatch {
            inner: Arc::clone(&self.inner),
            id,
            real,
            recursive: request.recursive,
        }))
    }
}

fn io_error(error: notify::Error) -> io::Error {
    io::Error::other(error.to_string())
}

fn mode(tree: bool) -> RecursiveMode {
    if tree {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    }
}

impl Notifier {
    /// The sender of the thread that hands out the notifiers' events,
    /// started with the first.
    fn sender(&mut self, registry: &Arc<Mutex<Registry>>) -> io::Result<Sender> {
        if let Some(sender) = &self.sender {
            return Ok(sender.clone());
        }
        let (sender, receiver) = mpsc::channel();
        let registry = Arc::clone(registry);
        std::thread::Builder::new()
            .name("tsc-rs watch".to_owned())
            .spawn(move || {
                // Ends when the notifier goes, with its sender.
                for result in receiver {
                    dispatch(&registry, result);
                }
            })?;
        self.sender = Some(sender.clone());
        Ok(sender)
    }

    /// One more watch of `real`: the notifier starts watching it, or its
    /// tree once a watch wants that.
    fn start(
        &mut self,
        registry: &Arc<Mutex<Registry>>,
        real: &Path,
        recursive: bool,
    ) -> io::Result<()> {
        let before = self
            .watched
            .get(real)
            .map(|watched| (watched.count, watched.recursive, watched.tree));
        let (count, tree) = before.map_or((0, false), |(count, _, tree)| (count, tree));
        let watched = self.watched.entry(real.to_owned()).or_insert(Watched {
            count: 0,
            recursive: 0,
            tree: false,
        });
        watched.count += 1;
        if recursive {
            watched.recursive += 1;
        }
        watched.tree |= recursive;
        let change = if count == 0 {
            Some(Change::Add(real, recursive))
        } else if recursive && !tree {
            Some(Change::Mode(real, true))
        } else {
            None
        };
        if let Some(change) = change {
            if let Err(error) = self.apply(registry, change) {
                match before {
                    None => {
                        self.watched.remove(real);
                    }
                    Some((count, recursive, tree)) => {
                        let watched = self.watched.get_mut(real).expect("updated above");
                        watched.count = count;
                        watched.recursive = recursive;
                        watched.tree = tree;
                    }
                }
                return Err(error);
            }
        }
        Ok(())
    }

    /// One watch of `real` less: the notifier stops watching it with the
    /// last, and watches only its entries once no watch wants the tree.
    fn stop(&mut self, registry: &Arc<Mutex<Registry>>, real: &Path, recursive: bool) {
        let Some(watched) = self.watched.get_mut(real) else {
            return;
        };
        watched.count -= 1;
        if recursive {
            watched.recursive -= 1;
        }
        let change = if watched.count == 0 {
            self.watched.remove(real);
            Change::Remove(real)
        } else if watched.tree && watched.recursive == 0 {
            watched.tree = false;
            Change::Mode(real, false)
        } else {
            return;
        };
        let _ = self.apply(registry, change);
    }

    /// Makes the notifier watch the directories as `watched` says after
    /// `change`.
    fn apply(&mut self, registry: &Arc<Mutex<Registry>>, change: Change<'_>) -> io::Result<()> {
        if cfg!(target_os = "macos") {
            return self.replace(registry, change);
        }
        if self.watcher.is_none() {
            let sender = self.sender(registry)?;
            self.watcher = Some(notify::recommended_watcher(sender).map_err(io_error)?);
        }
        let watcher = self.watcher.as_mut().expect("created above");
        match change {
            Change::Add(path, tree) => watcher.watch(path, mode(tree)).map_err(io_error),
            Change::Remove(path) => watcher.unwatch(path).map_err(io_error),
            Change::Mode(path, tree) => {
                let _ = watcher.unwatch(path);
                watcher.watch(path, mode(tree)).map_err(|error| {
                    // The directory stays watched as it was.
                    let _ = watcher.watch(path, mode(!tree));
                    io_error(error)
                })
            }
        }
    }

    /// The FSEvents notifier stops its stream, changes its paths and starts
    /// it again from then on, losing the changes on the way; tsgo starts
    /// the new streams before it stops the old ones. Here a notifier with
    /// every directory starts before the old one goes. A directory that
    /// went (its watches end) is left out; the changed one must start.
    fn replace(&mut self, registry: &Arc<Mutex<Registry>>, change: Change<'_>) -> io::Result<()> {
        if self.watched.is_empty() {
            self.watcher = None;
            return Ok(());
        }
        let required = match change {
            Change::Add(path, _) | Change::Mode(path, _) => Some(path),
            Change::Remove(_) => None,
        };
        let mut watcher = notify::recommended_watcher(self.sender(registry)?).map_err(io_error)?;
        let mut paths = watcher.paths_mut();
        for (path, watched) in &self.watched {
            if let Err(error) = paths.add(path, mode(watched.tree)) {
                if required == Some(path.as_path()) {
                    return Err(io_error(error));
                }
            }
        }
        paths.commit().map_err(io_error)?;
        self.watcher = Some(watcher);
        Ok(())
    }
}

/// What one watch is handed of an event.
struct Delivery {
    events: WatchEvents,
    changes: Vec<WatchEvent>,
    overflow: bool,
    terminated: bool,
}

/// Hands an event of the notifier to the watches it concerns, outside the
/// registry's lock (a watch's callback may start or stop watches).
fn dispatch(registry: &Mutex<Registry>, result: notify::Result<notify::Event>) {
    let Ok(event) = result else {
        return;
    };
    if matches!(event.kind, EventKind::Access(_)) {
        return;
    }
    // tsgo's classification: FSEvents accumulates a path's flags (a
    // removal may come as created, modified and removed at once, which the
    // notifier reports as an event each), so whether the path is still
    // there tells a deletion from a change.
    let paths = event
        .paths
        .iter()
        .map(|path| {
            let kind = if std::fs::symlink_metadata(path).is_err() {
                WatchEventKind::Delete
            } else if matches!(event.kind, EventKind::Create(_)) {
                WatchEventKind::Create
            } else {
                WatchEventKind::Update
            };
            (path, kind)
        })
        .collect::<Vec<_>>();
    let mut deliveries = Vec::new();
    {
        let mut registry = lock(registry);
        for registration in registry.registrations.values_mut() {
            if registration.terminated {
                continue;
            }
            if event.need_rescan() {
                // tsgo `ErrOverflow`: the watch may have missed events.
                deliveries.push(Delivery {
                    events: registration.events.clone(),
                    changes: Vec::new(),
                    overflow: true,
                    terminated: false,
                });
                continue;
            }
            let mut changes = Vec::new();
            let mut terminated = false;
            for &(path, kind) in &paths {
                if *path == registration.real {
                    // tsgo: a directory's own changes are its entries'; only
                    // its removal is reported, and it ends the watch.
                    if !registration.real.is_dir() {
                        changes.push(WatchEvent {
                            kind: WatchEventKind::Delete,
                            path: registration.spelling.clone(),
                        });
                        terminated = true;
                    }
                } else if registration.covers(path) {
                    changes.push(WatchEvent {
                        kind,
                        path: registration.spelled(path),
                    });
                }
            }
            registration.terminated = terminated;
            if !changes.is_empty() || terminated {
                deliveries.push(Delivery {
                    events: registration.events.clone(),
                    changes,
                    overflow: false,
                    terminated,
                });
            }
        }
    }
    for delivery in deliveries {
        if delivery.overflow {
            delivery.events.overflow();
            continue;
        }
        delivery.events.send(delivery.changes);
        if delivery.terminated {
            delivery.events.terminate();
        }
    }
}

/// One watch of the backend; dropping it stops the watch.
struct NativeWatch {
    inner: Arc<Inner>,
    id: u64,
    real: PathBuf,
    recursive: bool,
}

impl WatchHandle for NativeWatch {}

impl Drop for NativeWatch {
    fn drop(&mut self) {
        let mut notifier = lock(&self.inner.notifier);
        lock(&self.inner.registry).registrations.remove(&self.id);
        notifier.stop(&self.inner.registry, &self.real, self.recursive);
    }
}

impl WatchBackend for NativeWatchBackend {
    fn watch_directories(
        &self,
        requests: Vec<WatchRequest>,
    ) -> io::Result<Vec<Box<dyn WatchHandle>>> {
        let mut handles = Vec::with_capacity(requests.len());
        for request in requests {
            // A failure stops the watches this call started, as `handles`
            // goes.
            handles.push(self.watch(request)?);
        }
        Ok(handles)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/watch_native.rs"]
mod tests;
