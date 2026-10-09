//! The process's watch backend: the operating system's file-system
//! notifications through the `notify` crate (FSEvents on macOS, inotify on
//! Linux, ReadDirectoryChangesW on Windows, kqueue on the BSDs). One
//! notifier serves every directory of the run.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

use super::manager::{
    WatchBackend, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};

/// The directories being watched: the real path the notifier reports each
/// under, and the spelling the run asked for (tsgo keeps an event rooted at
/// the watched directory's spelling, so `/tmp/p` stays `/tmp/p` when the
/// system reports `/private/tmp/p`).
#[derive(Default)]
struct Roots {
    by_real_path: BTreeMap<PathBuf, (String, usize)>,
}

impl Roots {
    fn add(&mut self, real: PathBuf, requested: String) {
        self.by_real_path
            .entry(real)
            .and_modify(|(_, count)| *count += 1)
            .or_insert((requested, 1));
    }

    fn remove(&mut self, real: &Path) {
        if let Some((_, count)) = self.by_real_path.get_mut(real) {
            *count -= 1;
            if *count == 0 {
                self.by_real_path.remove(real);
            }
        }
    }

    /// `path` with its watched root's real path replaced by the root's
    /// requested spelling.
    fn spelled(&self, path: &Path) -> String {
        for (real, (requested, _)) in self.by_real_path.iter().rev() {
            if let Ok(rest) = path.strip_prefix(real) {
                let rest = rest.to_string_lossy().replace('\\', "/");
                return if rest.is_empty() {
                    requested.clone()
                } else {
                    format!("{}/{rest}", requested.trim_end_matches('/'))
                };
            }
        }
        path.to_string_lossy().replace('\\', "/")
    }
}

struct Shared {
    watcher: Option<RecommendedWatcher>,
    roots: Arc<Mutex<Roots>>,
}

/// The process's watch backend.
pub(crate) struct NativeWatchBackend {
    shared: Arc<Mutex<Shared>>,
}

impl NativeWatchBackend {
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(Mutex::new(Shared {
                watcher: None,
                roots: Arc::default(),
            })),
        }
    }
}

fn io_error(error: notify::Error) -> io::Error {
    io::Error::other(error.to_string())
}

impl WatchBackend for NativeWatchBackend {
    fn watch_directories(
        &self,
        requests: Vec<WatchRequest>,
    ) -> io::Result<Vec<Box<dyn WatchHandle>>> {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        let mut handles: Vec<Box<dyn WatchHandle>> = Vec::with_capacity(requests.len());
        for request in requests {
            if shared.watcher.is_none() {
                let events: WatchEvents = request.events.clone();
                let roots = Arc::clone(&shared.roots);
                let watcher =
                    notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                        let Ok(event) = result else {
                            return;
                        };
                        if event.need_rescan() {
                            events.overflow();
                            return;
                        }
                        let kind = match event.kind {
                            EventKind::Create(_) => WatchEventKind::Create,
                            EventKind::Remove(_) => WatchEventKind::Delete,
                            EventKind::Access(_) => return,
                            _ => WatchEventKind::Update,
                        };
                        let roots = roots.lock().unwrap_or_else(PoisonError::into_inner);
                        events.send(event.paths.iter().map(|path| WatchEvent {
                            kind,
                            path: roots.spelled(path),
                        }));
                    })
                    .map_err(io_error)?;
                shared.watcher = Some(watcher);
            }
            let directory = PathBuf::from(&request.directory);
            let real = std::fs::canonicalize(&directory).unwrap_or_else(|_| directory.clone());
            let mode = if request.recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            shared
                .watcher
                .as_mut()
                .expect("the notifier was created above")
                .watch(&directory, mode)
                .map_err(io_error)?;
            shared
                .roots
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .add(real.clone(), request.directory.clone());
            handles.push(Box::new(NativeWatch {
                shared: Arc::clone(&self.shared),
                directory,
                real,
            }));
        }
        Ok(handles)
    }
}

/// One watched directory; dropping it stops the watch.
struct NativeWatch {
    shared: Arc<Mutex<Shared>>,
    directory: PathBuf,
    real: PathBuf,
}

impl WatchHandle for NativeWatch {}

impl Drop for NativeWatch {
    fn drop(&mut self) {
        let mut shared = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(watcher) = shared.watcher.as_mut() {
            let _ = watcher.unwatch(&self.directory);
        }
        shared
            .roots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.real);
    }
}
