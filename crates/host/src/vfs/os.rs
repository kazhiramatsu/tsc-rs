//! The process's file system.

use std::fs;
use std::io;
use std::time::SystemTime;

use super::{DirEntry, FileSystem, FileType, Metadata};

/// The process's file system, through `std::fs` (tsgo `osvfs`). Paths go to
/// the operating system as they are given; a relative path is resolved
/// against the process's current directory.
#[derive(Clone, Copy, Debug)]
pub struct OsFs {
    case_sensitive: bool,
}

impl OsFs {
    /// The process's file system, whose case profile the caller has
    /// determined.
    pub const fn new(case_sensitive: bool) -> Self {
        Self { case_sensitive }
    }
}

impl FileSystem for OsFs {
    fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        fs::metadata(path).map(|metadata| convert(&metadata))
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let file_type = if file_type.is_symlink() {
                FileType::Symlink
            } else if file_type.is_dir() {
                FileType::Directory
            } else if file_type.is_file() {
                FileType::File
            } else {
                continue;
            };
            if let Some(name) = entry.file_name().to_str() {
                entries.push(DirEntry::new(name, file_type));
            }
        }
        entries.sort_by(|left, right| left.name().cmp(right.name()));
        Ok(entries)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        let physical = fs::canonicalize(path)?;
        #[cfg(windows)]
        let physical = dunce::simplified(&physical).to_path_buf();
        physical
            .to_str()
            .map(|path| path.replace('\\', "/"))
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}: not Unicode", physical.display()),
                )
            })
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        fs::write(path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        use io::Write;
        fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?
            .write_all(contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        let result = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
            Ok(_) => fs::remove_file(path),
            Err(error) => Err(error),
        };
        match result {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        fs::OpenOptions::new()
            .write(true)
            .open(path)?
            .set_modified(modified)
    }
}

fn convert(metadata: &fs::Metadata) -> Metadata {
    let file_type = if metadata.is_dir() {
        FileType::Directory
    } else {
        FileType::File
    };
    Metadata::new(
        file_type,
        metadata.len(),
        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    )
}
