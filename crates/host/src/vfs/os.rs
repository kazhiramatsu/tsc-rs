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
        write_with(
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true),
            path,
            contents,
        )
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        write_with(
            fs::OpenOptions::new().append(true).create(true),
            path,
            contents,
        )
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        mkdir_all(path)
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

/// tsgo `osvfs.writeFileWithFlag`: open, then write; a failure is Go's
/// `*fs.PathError` (`open /p/a.js: is a directory`).
fn write_with(options: &fs::OpenOptions, path: &str, contents: &[u8]) -> io::Result<()> {
    use io::Write;
    let mut file = options
        .open(path)
        .map_err(|error| path_error("open", path, &error))?;
    file.write_all(contents)
        .map_err(|error| path_error("write", path, &error))
}

/// Go's `os.MkdirAll`, which tsgo's `osvfs` creates directories with: the
/// directory at `path` and every missing one above it. A failure names the
/// directory where it happened.
fn mkdir_all(path: &str) -> io::Result<()> {
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.is_dir() {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("mkdir {path}: not a directory"),
        ));
    }
    let trimmed = path.trim_end_matches(is_separator);
    let parent = &trimmed[..trimmed.rfind(is_separator).unwrap_or(0)];
    if parent.len() > volume_name_length(path) {
        mkdir_all(parent)?;
    }
    match fs::create_dir(path) {
        Ok(()) => Ok(()),
        // `a/.`, or another writer created it meanwhile.
        Err(_) if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) => Ok(()),
        Err(error) => Err(path_error("mkdir", path, &error)),
    }
}

fn is_separator(character: char) -> bool {
    character == '/' || (cfg!(windows) && character == '\\')
}

/// Go's `filepath.VolumeName` length: a drive (`c:`) on Windows.
fn volume_name_length(path: &str) -> usize {
    if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') {
        2
    } else {
        0
    }
}

/// Go's `*fs.PathError` text: `<operation> <path>: <error>`.
fn path_error(operation: &str, path: &str, error: &io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("{operation} {path}: {}", os_error_text(error)),
    )
}

/// Go's text of an operating system error: on Unix, `strerror` with a
/// lowercase first letter (Go's `mkerrors.sh`: `Is a directory` becomes
/// `is a directory`); elsewhere the system's message.
fn os_error_text(error: &io::Error) -> String {
    let text = error.to_string();
    let Some(code) = error.raw_os_error() else {
        return text;
    };
    let text = text
        .strip_suffix(&format!(" (os error {code})"))
        .unwrap_or(&text);
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(first), Some(second))
            if cfg!(unix) && first.is_ascii_uppercase() && second.is_ascii_lowercase() =>
        {
            format!("{}{}", first.to_ascii_lowercase(), &text[1..])
        }
        _ => text.to_owned(),
    }
}
