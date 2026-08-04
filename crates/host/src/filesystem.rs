use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::ordering::compare_utf16;
use crate::{CompilerHost, HostError, HostErrorKind, HostOperation};

/// Read-only [`CompilerHost`] backed by the process filesystem.
///
/// Construction snapshots both the current-directory spelling and the case
/// profile. Queries preserve file bytes exactly and never normalize, join, or
/// decode the paths and contents supplied by the caller.
#[derive(Clone, Debug)]
pub struct FsCompilerHost {
    current_directory: PathBuf,
    case_sensitive: bool,
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
        validate_input_path(path, HostOperation::ReadDirectory)?;
        let Some(metadata) = metadata_if_present(path, HostOperation::ReadDirectory)? else {
            return Ok(Vec::new());
        };
        if !metadata.is_dir() {
            return Ok(Vec::new());
        }

        let reader = match fs::read_dir(path) {
            Ok(reader) => reader,
            Err(error) if is_absence(&error) => return Ok(Vec::new()),
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
            let entry_metadata = match fs::metadata(&entry_path) {
                Ok(metadata) => metadata,
                Err(error) if is_absence(&error) => continue,
                Err(error) => {
                    return Err(map_io_error(
                        error,
                        HostOperation::ReadDirectory,
                        Some(entry_path),
                    ));
                }
            };
            if directories_only {
                if !entry_metadata.is_dir() {
                    continue;
                }
            } else if !entry_metadata.is_file() && !entry_metadata.is_dir() {
                continue;
            }

            let display_name = entry
                .file_name()
                .into_string()
                .expect("validated filesystem-host entry name is Unicode");
            entries.push((display_name, entry_path));
        }
        entries.sort_by(|left, right| compare_utf16(&left.0, &right.0));
        Ok(entries.into_iter().map(|(_, path)| path).collect())
    }
}

impl CompilerHost for FsCompilerHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        Ok(self.current_directory.clone())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        validate_input_path(path, HostOperation::ReadFile)?;
        let Some(metadata) = metadata_if_present(path, HostOperation::ReadFile)? else {
            return Ok(None);
        };
        if !metadata.is_file() {
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
        Ok(metadata_if_present(path, HostOperation::FileExists)?
            .is_some_and(|metadata| metadata.is_file()))
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        validate_input_path(path, HostOperation::DirectoryExists)?;
        Ok(metadata_if_present(path, HostOperation::DirectoryExists)?
            .is_some_and(|metadata| metadata.is_dir()))
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.read_immediate_entries(path, false)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.read_immediate_entries(path, true)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        validate_input_path(path, HostOperation::Realpath)?;
        if metadata_if_present(path, HostOperation::Realpath)?.is_none() {
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
    }
}

fn metadata_if_present(
    path: &Path,
    operation: HostOperation,
) -> Result<Option<fs::Metadata>, HostError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if is_absence(&error) => Ok(None),
        Err(error) => Err(map_io_error(error, operation, Some(path.to_path_buf()))),
    }
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
mod tests {
    use std::io;

    use super::map_io_error;
    use crate::{HostErrorKind, HostOperation};

    #[test]
    fn maps_stable_io_error_classes() {
        for (source, expected) in [
            (
                io::ErrorKind::PermissionDenied,
                HostErrorKind::PermissionDenied,
            ),
            (io::ErrorKind::InvalidInput, HostErrorKind::InvalidInput),
            (io::ErrorKind::InvalidData, HostErrorKind::InvalidData),
            (io::ErrorKind::OutOfMemory, HostErrorKind::ResourceLimit),
            (io::ErrorKind::StorageFull, HostErrorKind::ResourceLimit),
            (io::ErrorKind::FileTooLarge, HostErrorKind::ResourceLimit),
            (io::ErrorKind::QuotaExceeded, HostErrorKind::ResourceLimit),
            (io::ErrorKind::Other, HostErrorKind::Other),
        ] {
            let error = map_io_error(io::Error::from(source), HostOperation::ReadFile, None);
            assert_eq!(error.kind(), expected);
            assert_eq!(error.operation(), HostOperation::ReadFile);
        }
    }

    #[cfg(windows)]
    #[test]
    fn removes_only_verbatim_disk_realpath_prefixes() {
        use std::path::PathBuf;

        use super::normalize_windows_realpath;

        assert_eq!(
            normalize_windows_realpath(PathBuf::from(r"\\?\C:\work\a.ts")),
            PathBuf::from(r"C:\work\a.ts")
        );
        assert_eq!(
            normalize_windows_realpath(PathBuf::from(r"\\?\UNC\server\share\a.ts")),
            PathBuf::from(r"\\?\UNC\server\share\a.ts")
        );
        assert_eq!(
            normalize_windows_realpath(PathBuf::from(r"\\?\Volume{1234}\a.ts")),
            PathBuf::from(r"\\?\Volume{1234}\a.ts")
        );
    }
}
