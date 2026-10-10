//! File systems a command-line run reads and writes: the process's
//! ([`OsFs`]) and an in-memory one ([`MemFs`]) behind one trait, so a run
//! without a disk — a test, a WebAssembly embedding — drives the same
//! compiler. TypeScript 7.1 (tsgo) has the same layer: `vfs.FS`, with
//! `osvfs` and the `vfstest` map file system.
//!
//! Paths are TypeScript paths: absolute, rooted at `/` or at a drive
//! (`c:/`), with `/` separators. Every operation normalizes the path it is
//! given (`\` becomes `/`, `.` and `..` segments are resolved, a trailing
//! separator is dropped), as tsgo's file systems do.

mod host;
mod memory;
mod os;

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

pub use host::VfsCompilerHost;
pub use memory::{Entry, EntryContents, MemFs, Seed};
pub use os::OsFs;

/// A file system the compiler reads and writes through (tsgo `vfs.FS`).
///
/// Reads follow symbolic links. A missing entry is an error of kind
/// [`io::ErrorKind::NotFound`]; [`FileSystem::is_file`] and
/// [`FileSystem::is_dir`] answer existence questions without one.
pub trait FileSystem: Send + Sync {
    /// Whether names that differ only in case name different entries.
    fn case_sensitive(&self) -> bool;

    /// The contents of the file at `path`.
    fn read(&self, path: &str) -> io::Result<Vec<u8>>;

    /// What `path` names.
    fn metadata(&self, path: &str) -> io::Result<Metadata>;

    /// The entries of the directory at `path`, ordered by name. Each entry
    /// reports its own type: a link is a [`FileType::Symlink`].
    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>>;

    /// The path of what `path` names once every link on the way is
    /// followed, spelled as the file system spells it (tsgo `Realpath`).
    fn canonicalize(&self, path: &str) -> io::Result<String>;

    /// Replaces the contents of the file at `path`, or creates the file in
    /// an existing directory.
    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()>;

    /// Appends to the file at `path`, creating it as [`FileSystem::write`]
    /// does.
    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()>;

    /// Creates the directory at `path` and every missing one above it.
    fn create_dir_all(&self, path: &str) -> io::Result<()>;

    /// Removes the entry at `path`, a directory with everything below it.
    /// A missing entry is not an error, and a link is removed itself.
    fn remove(&self, path: &str) -> io::Result<()>;

    /// Sets the modification time of the entry at `path`.
    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()>;

    /// Whether `path` names a file.
    fn is_file(&self, path: &str) -> bool {
        self.metadata(path).is_ok_and(|metadata| metadata.is_file())
    }

    /// Whether `path` names a directory.
    fn is_dir(&self, path: &str) -> bool {
        self.metadata(path).is_ok_and(|metadata| metadata.is_dir())
    }

    /// Writes the file at `path`, creating its missing directories when
    /// the plain write fails (tsgo's `vfs.FS.WriteFile` of the OS and
    /// in-memory file systems). A file system that hands its writes to a
    /// client or to the layer below passes the whole write on, as tsgo's
    /// wrappers pass `WriteFile` on.
    fn write_creating_dirs(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        if self.write(path, contents).is_ok() {
            return Ok(());
        }
        self.create_dir_all(parent(&normalize(path)?))?;
        self.write(path, contents)
    }

    /// The files and directories in the directory at `path`, links
    /// followed and broken links left out (tsgo `GetAccessibleEntries`).
    /// A missing directory has no entries.
    fn accessible_entries(&self, path: &str) -> Entries {
        let mut entries = Entries::default();
        let Ok(listing) = self.read_dir(path) else {
            return entries;
        };
        for entry in listing {
            let file_type = match entry.file_type() {
                FileType::Symlink => {
                    let target = format!("{}/{}", path.trim_end_matches('/'), entry.name());
                    match self.metadata(&target) {
                        Ok(metadata) => {
                            entries.symlinks.insert(entry.name().to_owned());
                            metadata.file_type()
                        }
                        Err(_) => continue,
                    }
                }
                file_type => file_type,
            };
            match file_type {
                FileType::File => entries.files.push(entry.name().to_owned()),
                FileType::Directory => entries.directories.push(entry.name().to_owned()),
                FileType::Symlink => {}
            }
        }
        entries
    }
}

macro_rules! delegate_file_system {
    ($($pointer:ty),*) => {$(
        impl<T: FileSystem + ?Sized> FileSystem for $pointer {
            fn case_sensitive(&self) -> bool {
                (**self).case_sensitive()
            }
            fn read(&self, path: &str) -> io::Result<Vec<u8>> {
                (**self).read(path)
            }
            fn metadata(&self, path: &str) -> io::Result<Metadata> {
                (**self).metadata(path)
            }
            fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
                (**self).read_dir(path)
            }
            fn canonicalize(&self, path: &str) -> io::Result<String> {
                (**self).canonicalize(path)
            }
            fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
                (**self).write(path, contents)
            }
            fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
                (**self).append(path, contents)
            }
            fn create_dir_all(&self, path: &str) -> io::Result<()> {
                (**self).create_dir_all(path)
            }
            fn remove(&self, path: &str) -> io::Result<()> {
                (**self).remove(path)
            }
            fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
                (**self).set_modified(path, modified)
            }
            fn is_file(&self, path: &str) -> bool {
                (**self).is_file(path)
            }
            fn is_dir(&self, path: &str) -> bool {
                (**self).is_dir(path)
            }
            fn write_creating_dirs(&self, path: &str, contents: &[u8]) -> io::Result<()> {
                (**self).write_creating_dirs(path, contents)
            }
            fn accessible_entries(&self, path: &str) -> Entries {
                (**self).accessible_entries(path)
            }
        }
    )*};
}

delegate_file_system!(&T, Box<T>, std::sync::Arc<T>);

/// What a directory holds once links are followed (tsgo `vfs.Entries`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Entries {
    pub files: Vec<String>,
    pub directories: Vec<String>,
    /// The names in `files` or `directories` that are links.
    pub symlinks: BTreeSet<String>,
}

/// What an entry is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileType {
    File,
    Directory,
    Symlink,
}

impl FileType {
    pub const fn is_file(self) -> bool {
        matches!(self, Self::File)
    }

    pub const fn is_dir(self) -> bool {
        matches!(self, Self::Directory)
    }

    pub const fn is_symlink(self) -> bool {
        matches!(self, Self::Symlink)
    }
}

/// What [`FileSystem::metadata`] reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    file_type: FileType,
    len: u64,
    modified: SystemTime,
}

impl Metadata {
    pub const fn new(file_type: FileType, len: u64, modified: SystemTime) -> Self {
        Self {
            file_type,
            len,
            modified,
        }
    }

    pub const fn file_type(&self) -> FileType {
        self.file_type
    }

    pub const fn is_file(&self) -> bool {
        self.file_type.is_file()
    }

    pub const fn is_dir(&self) -> bool {
        self.file_type.is_dir()
    }

    pub const fn is_symlink(&self) -> bool {
        self.file_type.is_symlink()
    }

    /// The size of a file's contents.
    pub const fn len(&self) -> u64 {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub const fn modified(&self) -> SystemTime {
        self.modified
    }
}

/// One entry of [`FileSystem::read_dir`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    name: String,
    file_type: FileType,
}

impl DirEntry {
    pub fn new(name: impl Into<String>, file_type: FileType) -> Self {
        Self {
            name: name.into(),
            file_type,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn file_type(&self) -> FileType {
        self.file_type
    }
}

/// Where a file system reads the time it stamps on what it writes.
pub trait Clock: Send + Sync + fmt::Debug {
    fn now(&self) -> SystemTime;
}

/// The process's clock.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// A clock that moves forward by a fixed step every time it is read, so
/// successive writes get distinct, ordered times (tsgo's `TestClock` steps
/// one second).
#[derive(Debug)]
pub struct SteppingClock {
    now: Mutex<SystemTime>,
    step: Duration,
}

impl SteppingClock {
    /// A clock whose first reading is `start + step`.
    pub fn new(start: SystemTime, step: Duration) -> Self {
        Self {
            now: Mutex::new(start),
            step,
        }
    }
}

impl Clock for SteppingClock {
    fn now(&self) -> SystemTime {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now += self.step;
        *now
    }
}

/// `path` with `/` separators, `.` and `..` segments resolved and no
/// trailing separator; an error unless it is rooted at `/` or a drive.
pub fn normalize(path: &str) -> io::Result<String> {
    let slashed = path.replace('\\', "/");
    let (mut normal, rest) = if let Some(rest) = slashed.strip_prefix('/') {
        (String::from("/"), rest)
    } else if is_drive(slashed.split('/').next().unwrap_or_default()) {
        let rest = slashed.get(3..).unwrap_or_default();
        (format!("{}/", &slashed[..2]), rest)
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{path:?} is not an absolute path"),
        ));
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    normal.push_str(&parts.join("/"));
    Ok(normal)
}

/// The components of a normalized path: `/a/b` is `a`, `b`; `c:/a` is
/// `c:`, `a`.
fn components(normal: &str) -> impl Iterator<Item = &str> {
    normal
        .strip_prefix('/')
        .unwrap_or(normal)
        .split('/')
        .filter(|part| !part.is_empty())
}

/// The normalized path of `components` (see [`components`]).
fn join<'a>(components: impl IntoIterator<Item = &'a str>) -> String {
    let mut path = String::new();
    for (index, component) in components.into_iter().enumerate() {
        if index > 0 || !is_drive(component) {
            path.push('/');
        }
        path.push_str(component);
    }
    if path.is_empty() || is_drive(&path) {
        path.push('/');
    }
    path
}

/// The directory of a normalized path (the root for the root).
fn parent(normal: &str) -> &str {
    match normal.trim_end_matches('/').rfind('/') {
        Some(0) => "/",
        Some(index) if is_drive(&normal[..index]) => &normal[..=index],
        Some(index) => &normal[..index],
        None => normal,
    }
}

/// `c:`: a drive.
fn is_drive(component: &str) -> bool {
    let bytes = component.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
#[path = "../../tests/unit/vfs/tests.rs"]
mod tests;
