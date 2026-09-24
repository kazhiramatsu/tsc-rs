use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tsc_diagnostics::{JsStr, JsString};

use crate::ordering::compare_utf16;
use crate::{
    CompilerHost, DirectoryListingEntry, DirectoryListingKind, HostError, HostErrorKind,
    HostOperation,
};

/// Read-only [`CompilerHost`] backed by the process filesystem.
///
/// Construction snapshots both the current-directory spelling and the case
/// profile. Queries preserve file bytes exactly and never normalize, join, or
/// decode the paths and contents supplied by the caller.
#[derive(Clone, Debug)]
pub struct FsCompilerHost {
    current_directory: PathBuf,
    case_sensitive: bool,
    observations: Arc<ObservationCache>,
}

/// What one `stat` of a path observed.
#[derive(Clone, Copy, Debug)]
enum Presence {
    Missing,
    File { len: u64 },
    Directory,
    Other,
}

const OBSERVATION_SHARDS: usize = 32;

/// The host's memo of what it observed on disk, shared by every clone of
/// the host and every thread that resolves over it. One program
/// construction treats the disk as fixed, exactly as TypeScript's
/// `createProgram` does (`changeCompilerHostLikeToUseCache` memoizes
/// fileExists/directoryExists/readFile for the same reason), and module
/// resolution probes the same node_modules ancestors, package directories
/// and symlinked packages once per request from every file: the memo turns
/// those repeats into a lookup instead of a syscall. Only successful
/// observations are kept; a host error is reported every time.
#[derive(Debug, Default)]
struct ObservationCache {
    presence: [RwLock<HashMap<PathBuf, Presence>>; OBSERVATION_SHARDS],
    realpath: [RwLock<HashMap<PathBuf, Option<PathBuf>>>; OBSERVATION_SHARDS],
}

impl ObservationCache {
    fn shard(path: &Path) -> usize {
        // FNV-1a over the path bytes: cheap, and the shard only spreads
        // lock contention.
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for &byte in path.as_os_str().as_encoded_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        (hash % OBSERVATION_SHARDS as u64) as usize
    }

    fn presence(&self, path: &Path, operation: HostOperation) -> Result<Presence, HostError> {
        let shard = &self.presence[Self::shard(path)];
        if let Some(present) = shard
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(path)
        {
            return Ok(*present);
        }
        let present = match metadata_if_present(path, operation)? {
            None => Presence::Missing,
            Some(metadata) if metadata.is_file() => Presence::File {
                len: metadata.len(),
            },
            Some(metadata) if metadata.is_dir() => Presence::Directory,
            Some(_) => Presence::Other,
        };
        shard
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(path.to_path_buf(), present);
        Ok(present)
    }

    fn realpath(
        &self,
        path: &Path,
        compute: impl FnOnce() -> Result<Option<PathBuf>, HostError>,
    ) -> Result<Option<PathBuf>, HostError> {
        let shard = &self.realpath[Self::shard(path)];
        if let Some(physical) = shard
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(path)
        {
            return Ok(physical.clone());
        }
        let physical = compute()?;
        shard
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(path.to_path_buf(), physical.clone());
        Ok(physical)
    }
}

impl FsCompilerHost {
    /// Construct a filesystem host under an explicit platform profile.
    ///
    /// The caller-provided case profile must describe the filesystem that
    /// owns `current_directory`. This entry is useful for a driver with an
    /// already-declared profile and for MemoryHost/FsHost equivalence tests.
    pub fn new(
        current_directory: impl Into<PathBuf>,
        use_case_sensitive_file_names: bool,
    ) -> Result<Self, HostError> {
        let current_directory = current_directory.into();
        validate_input_path(&current_directory, HostOperation::CurrentDirectory)?;
        if !current_directory.is_absolute() {
            return Err(HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::CurrentDirectory,
                Some(current_directory),
                "filesystem host current directory must be absolute",
            ));
        }

        match metadata_if_present(&current_directory, HostOperation::CurrentDirectory)? {
            Some(metadata) if metadata.is_dir() => Ok(Self {
                current_directory,
                case_sensitive: use_case_sensitive_file_names,
                observations: Arc::default(),
            }),
            Some(_) => Err(HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::CurrentDirectory,
                Some(current_directory),
                "filesystem host current directory is not a directory",
            )),
            None => Err(HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::CurrentDirectory,
                Some(current_directory),
                "filesystem host current directory does not exist",
            )),
        }
    }

    /// Snapshot the current process directory and the native case profile.
    pub fn from_process() -> Result<Self, HostError> {
        let current_directory = env::current_dir()
            .map_err(|error| map_io_error(error, HostOperation::CurrentDirectory, None))?;
        validate_observed_path(&current_directory, HostOperation::CurrentDirectory)?;
        let case_sensitive = detect_case_sensitivity()?;
        Self::new(current_directory, case_sensitive)
    }

    fn read_immediate_entries(
        &self,
        path: &Path,
        directories_only: bool,
    ) -> Result<Vec<PathBuf>, HostError> {
        Ok(self
            .read_immediate_entry_records(path)?
            .into_iter()
            .filter(|record| record.directory || !directories_only)
            .map(|record| record.path)
            .collect())
    }

    /// The immediate file and directory entries below `path` in display-name
    /// order, each with the kind its directory entry names. Only a symbolic
    /// link has its target inspected (a dangling link is not an entry); every
    /// other entry's kind comes from the listing itself.
    fn read_immediate_entry_records(&self, path: &Path) -> Result<Vec<EntryRecord>, HostError> {
        validate_input_path(path, HostOperation::ReadDirectory)?;
        // An absent path or a non-directory has no entries; read_dir reports
        // both itself, without a separate stat.
        let reader = match fs::read_dir(path) {
            Ok(reader) => reader,
            Err(error) if is_absence(&error) => return Ok(Vec::new()),
            #[cfg(windows)]
            Err(_) if is_incomplete_windows_namespace_ancestor(path) => return Ok(Vec::new()),
            Err(error) => {
                return Err(map_io_error(
                    error,
                    HostOperation::ReadDirectory,
                    Some(path.to_path_buf()),
                ));
            }
        };

        let mut entries = Vec::new();
        for entry in reader {
            let entry = entry.map_err(|error| {
                map_io_error(
                    error,
                    HostOperation::ReadDirectory,
                    Some(path.to_path_buf()),
                )
            })?;
            let entry_path = entry.path();
            validate_observed_path(&entry_path, HostOperation::ReadDirectory)?;
            let file_type = entry.file_type().map_err(|error| {
                map_io_error(
                    error,
                    HostOperation::ReadDirectory,
                    Some(entry_path.clone()),
                )
            })?;
            let symlink = file_type.is_symlink();
            let directory = if symlink {
                match fs::metadata(&entry_path) {
                    Ok(metadata) if metadata.is_dir() => true,
                    Ok(metadata) if metadata.is_file() => false,
                    Ok(_) => continue,
                    Err(error) if is_absence(&error) => continue,
                    Err(error) => {
                        return Err(map_io_error(
                            error,
                            HostOperation::ReadDirectory,
                            Some(entry_path),
                        ));
                    }
                }
            } else if file_type.is_dir() {
                true
            } else if file_type.is_file() {
                false
            } else {
                continue;
            };

            let display_name = entry
                .file_name()
                .into_string()
                .expect("validated filesystem-host entry name is Unicode");
            entries.push(EntryRecord {
                name: display_name,
                path: entry_path,
                directory,
                symlink,
            });
        }
        entries.sort_by(|left, right| compare_utf16(&left.name, &right.name));
        Ok(entries)
    }
}

/// One entry of [`FsCompilerHost::read_immediate_entry_records`].
struct EntryRecord {
    name: String,
    path: PathBuf,
    directory: bool,
    symlink: bool,
}

impl crate::ParallelSourceReader for FsCompilerHost {
    fn read_source_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        CompilerHost::read_file_js(self, path)
    }
}

impl CompilerHost for FsCompilerHost {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        crate::js_path::from_native(
            &self.current_directory()?,
            HostOperation::CurrentDirectory,
            HostErrorKind::InvalidData,
        )
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::ReadFile)?;
        self.read_file(&native)
            .map_err(|error| retain_query_path(error, path, &native))
    }

    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::ReadFile)?;
        validate_input_path(&native, HostOperation::ReadFile)
            .and_then(|_| self.observations.presence(&native, HostOperation::ReadFile))
            .map(|present| match present {
                Presence::File { len } => Some(len),
                _ => None,
            })
            .map_err(|error| retain_query_path(error, path, &native))
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::FileExists)?;
        self.file_exists(&native)
            .map_err(|error| retain_query_path(error, path, &native))
    }

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::DirectoryExists)?;
        self.directory_exists(&native)
            .map_err(|error| retain_query_path(error, path, &native))
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        self.read_immediate_entries_js(path, false)
    }

    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::ReadDirectory)?;
        Ok(self
            .read_immediate_entry_records(&native)
            .map_err(|error| retain_query_path(error, path, &native))?
            .into_iter()
            .map(|record| DirectoryListingEntry {
                path: crate::js_path::join_observed_name(path, &record.name),
                kind: if record.directory {
                    DirectoryListingKind::Directory
                } else {
                    DirectoryListingKind::File
                },
                symlink: record.symlink,
            })
            .collect())
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        self.read_immediate_entries_js(path, true)
    }

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::Realpath)?;
        self.realpath(&native)
            .map_err(|error| retain_query_path(error, path, &native))?
            .map(|observed| {
                crate::js_path::from_native(
                    &observed,
                    HostOperation::Realpath,
                    HostErrorKind::InvalidData,
                )
            })
            .transpose()
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        Ok(self.current_directory.clone())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        validate_input_path(path, HostOperation::ReadFile)?;
        if !matches!(
            self.observations.presence(path, HostOperation::ReadFile)?,
            Presence::File { .. }
        ) {
            return Ok(None);
        }

        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if is_absence(&error) => Ok(None),
            Err(error) => Err(map_io_error(
                error,
                HostOperation::ReadFile,
                Some(path.to_path_buf()),
            )),
        }
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        validate_input_path(path, HostOperation::FileExists)?;
        Ok(matches!(
            self.observations
                .presence(path, HostOperation::FileExists)?,
            Presence::File { .. }
        ))
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        validate_input_path(path, HostOperation::DirectoryExists)?;
        Ok(matches!(
            self.observations
                .presence(path, HostOperation::DirectoryExists)?,
            Presence::Directory
        ))
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.read_immediate_entries(path, false)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.read_immediate_entries(path, true)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        validate_input_path(path, HostOperation::Realpath)?;
        self.observations.realpath(path, || {
            if matches!(
                self.observations.presence(path, HostOperation::Realpath)?,
                Presence::Missing
            ) {
                return Ok(None);
            }
            let physical = match fs::canonicalize(path) {
                Ok(physical) => physical,
                Err(error) if is_absence(&error) => return Ok(None),
                Err(error) => {
                    return Err(map_io_error(
                        error,
                        HostOperation::Realpath,
                        Some(path.to_path_buf()),
                    ));
                }
            };
            let physical = normalize_windows_realpath(physical);
            validate_observed_path(&physical, HostOperation::Realpath)?;
            Ok(Some(physical))
        })
    }

    /// Filesystem reads are pure functions of the on-disk state, which one
    /// program construction treats as fixed exactly as TypeScript's
    /// `createProgram` does; a concurrent external modification during a
    /// load is outside the host contract either way.
    fn permits_source_read_ahead(&self) -> bool {
        true
    }

    fn parallel_source_reader(&self) -> Option<&(dyn crate::ParallelSourceReader + Sync)> {
        Some(self)
    }

    fn parallel_resolution_host(&self) -> Option<&(dyn CompilerHost + Sync)> {
        Some(self)
    }
}

impl FsCompilerHost {
    fn read_immediate_entries_js(
        &self,
        path: JsStr<'_>,
        directories_only: bool,
    ) -> Result<Vec<JsString>, HostError> {
        let native = crate::js_path::filesystem_path(path, HostOperation::ReadDirectory)?;
        self.read_immediate_entries(&native, directories_only)
            .map_err(|error| retain_query_path(error, path, &native))?
            .into_iter()
            .map(|entry| {
                let name = entry
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("filesystem entries were validated as Unicode");
                Ok(crate::js_path::join_observed_name(path, name))
            })
            .collect()
    }
}

fn retain_query_path(error: HostError, requested: JsStr<'_>, native: &Path) -> HostError {
    let original = error.path().and_then(|observed| {
        let suffix = observed.strip_prefix(native).ok()?;
        if suffix.as_os_str().is_empty() {
            Some(requested.to_owned())
        } else {
            suffix
                .to_str()
                .map(|suffix| crate::js_path::join_observed_name(requested, suffix))
        }
    });
    match original {
        Some(original) => error.with_js_path(original),
        None => error,
    }
}

fn metadata_if_present(
    path: &Path,
    operation: HostOperation,
) -> Result<Option<fs::Metadata>, HostError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if is_absence(&error) => Ok(None),
        // TypeScript's UNC-style root parsing treats a verbatim drive path
        // such as `//?/C:/work/a.ts` as rooted at `//?/`. Its ancestor walk
        // consequently probes the synthetic `//?/C:` and `//?/` directories.
        // Node's `statSync(..., { throwIfNoEntry: false })` wrapper converts
        // every error for those incomplete namespace spellings to absence.
        // Rust reports ERROR_INVALID_FUNCTION instead, so preserve the
        // upstream existence observation without weakening fail-closed I/O
        // handling for any complete filesystem path.
        #[cfg(windows)]
        Err(_) if is_incomplete_windows_namespace_ancestor(path) => Ok(None),
        Err(error) => Err(map_io_error(error, operation, Some(path.to_path_buf()))),
    }
}

#[cfg(windows)]
fn is_incomplete_windows_namespace_ancestor(path: &Path) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    let slashed = text.replace('\\', "/");
    let namespace_tail = slashed
        .strip_prefix("//?/")
        .or_else(|| slashed.strip_prefix("//./"));
    namespace_tail.is_some_and(|tail| tail.is_empty() || !tail.contains('/'))
}

fn validate_input_path(path: &Path, operation: HostOperation) -> Result<&str, HostError> {
    validate_path(path, operation, HostErrorKind::InvalidInput)
}

fn validate_observed_path(path: &Path, operation: HostOperation) -> Result<&str, HostError> {
    validate_path(path, operation, HostErrorKind::InvalidData)
}

fn validate_path(
    path: &Path,
    operation: HostOperation,
    kind: HostErrorKind,
) -> Result<&str, HostError> {
    if path.as_os_str().is_empty() {
        return Err(HostError::new(
            kind,
            operation,
            Some(path.to_path_buf()),
            "path is empty",
        ));
    }
    let text = path.to_str().ok_or_else(|| {
        HostError::new(
            kind,
            operation,
            Some(path.to_path_buf()),
            "path is not representable as Unicode text",
        )
    })?;
    if text.contains('\0') {
        return Err(HostError::new(
            kind,
            operation,
            Some(path.to_path_buf()),
            "path contains a null character",
        ));
    }
    Ok(text)
}

fn is_absence(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

fn map_io_error(error: io::Error, operation: HostOperation, path: Option<PathBuf>) -> HostError {
    let kind = match error.kind() {
        io::ErrorKind::PermissionDenied => HostErrorKind::PermissionDenied,
        io::ErrorKind::InvalidInput => HostErrorKind::InvalidInput,
        io::ErrorKind::InvalidData => HostErrorKind::InvalidData,
        io::ErrorKind::OutOfMemory
        | io::ErrorKind::StorageFull
        | io::ErrorKind::FileTooLarge
        | io::ErrorKind::QuotaExceeded => HostErrorKind::ResourceLimit,
        _ => HostErrorKind::Other,
    };
    HostError::new(kind, operation, path, error.to_string())
}

#[cfg(windows)]
fn detect_case_sensitivity() -> Result<bool, HostError> {
    Ok(false)
}

#[cfg(not(windows))]
fn detect_case_sensitivity() -> Result<bool, HostError> {
    let executable = env::current_exe()
        .map_err(|error| map_io_error(error, HostOperation::DetectCaseSensitivity, None))?;
    let executable_text =
        validate_observed_path(&executable, HostOperation::DetectCaseSensitivity)?;
    let swapped = PathBuf::from(swap_ascii_case(executable_text));
    match fs::metadata(&swapped) {
        Ok(_) => Ok(false),
        Err(error) if is_absence(&error) => Ok(true),
        Err(error) => Err(map_io_error(
            error,
            HostOperation::DetectCaseSensitivity,
            Some(swapped),
        )),
    }
}

#[cfg(not(windows))]
fn swap_ascii_case(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_lowercase() {
                character.to_ascii_uppercase()
            } else if character.is_ascii_uppercase() {
                character.to_ascii_lowercase()
            } else {
                character
            }
        })
        .collect()
}

#[cfg(not(windows))]
fn normalize_windows_realpath(path: PathBuf) -> PathBuf {
    path
}

#[cfg(windows)]
fn normalize_windows_realpath(path: PathBuf) -> PathBuf {
    dunce::simplified(&path).to_path_buf()
}

#[cfg(test)]
#[path = "../tests/unit/filesystem/tests.rs"]
mod tests;
