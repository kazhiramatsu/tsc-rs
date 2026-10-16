//! tsgo `cachedvfs`: a file system that remembers what it answered about
//! entries (their metadata, real paths and accessible entries) for as long
//! as it lives. The project system puts one over each snapshot's file
//! system, which treats the disk as fixed while it builds and serves the
//! snapshot.

use std::collections::HashMap;
use std::io;
use std::sync::{PoisonError, RwLock};
use std::time::SystemTime;

use super::{DirEntry, Entries, FileSystem, Metadata};

/// Each map is read by the loader's threads at once (tsgo's `SyncMap`s).
pub struct CachedFs<F> {
    fs: F,
    metadata: RwLock<HashMap<String, Result<Metadata, io::ErrorKind>>>,
    files: RwLock<HashMap<String, bool>>,
    directories: RwLock<HashMap<String, bool>>,
    realpaths: RwLock<HashMap<String, Result<String, io::ErrorKind>>>,
    entries: RwLock<HashMap<String, Entries>>,
}

impl<F: FileSystem> CachedFs<F> {
    pub fn new(fs: F) -> Self {
        Self {
            fs,
            metadata: RwLock::default(),
            files: RwLock::default(),
            directories: RwLock::default(),
            realpaths: RwLock::default(),
            entries: RwLock::default(),
        }
    }
}

/// The cached answer for `path`, else `compute`'s (with no lock held),
/// remembered; the answer stored first is kept (tsgo `LoadOrStore`).
fn cached<T: Clone>(
    cache: &RwLock<HashMap<String, T>>,
    path: &str,
    compute: impl FnOnce() -> T,
) -> T {
    if let Some(value) = cache
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .get(path)
    {
        return value.clone();
    }
    let value = compute();
    cache
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(path.to_owned())
        .or_insert(value)
        .clone()
}

impl<F: FileSystem> FileSystem for CachedFs<F> {
    fn case_sensitive(&self) -> bool {
        self.fs.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.fs.read(path)
    }

    /// tsgo `Stat`.
    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        cached(&self.metadata, path, || {
            self.fs.metadata(path).map_err(|error| error.kind())
        })
        .map_err(io::Error::from)
    }

    /// tsgo `FileExists`.
    fn is_file(&self, path: &str) -> bool {
        cached(&self.files, path, || self.fs.is_file(path))
    }

    /// tsgo `DirectoryExists`.
    fn is_dir(&self, path: &str) -> bool {
        cached(&self.directories, path, || self.fs.is_dir(path))
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.fs.read_dir(path)
    }

    /// tsgo `Realpath`.
    fn canonicalize(&self, path: &str) -> io::Result<String> {
        cached(&self.realpaths, path, || {
            self.fs.canonicalize(path).map_err(|error| error.kind())
        })
        .map_err(io::Error::from)
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.fs.write(path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.fs.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.fs.create_dir_all(path)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.fs.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.fs.set_modified(path, modified)
    }

    fn write_creating_dirs(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.fs.write_creating_dirs(path, contents)
    }

    /// tsgo `GetAccessibleEntries`.
    fn accessible_entries(&self, path: &str) -> Entries {
        cached(&self.entries, path, || self.fs.accessible_entries(path))
    }

    fn permits_parallel_reads(&self) -> bool {
        self.fs.permits_parallel_reads()
    }
}
