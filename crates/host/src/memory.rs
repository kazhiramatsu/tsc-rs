use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{to_file_name_lower_case, CompilerHost, HostError, HostErrorKind, HostOperation};

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileEntry {
    display_path: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryEntry {
    display_path: PathBuf,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FailureKey {
    operation: HostOperation,
    path: Option<String>,
}

/// Immutable in-memory implementation of [`CompilerHost`].
///
/// The builder infers parent directories but performs no lexical path
/// normalization and no current-directory joining. File bytes are retained
/// exactly. On a case-insensitive profile, lookup identities use the exact
/// TypeScript 6.0.3 file-name case fold rather than a locale-sensitive fold.
#[derive(Clone, Debug)]
pub struct MemoryCompilerHost {
    current_directory: PathBuf,
    case_sensitive: bool,
    files: BTreeMap<String, FileEntry>,
    directories: BTreeMap<String, DirectoryEntry>,
    directory_entries: BTreeMap<String, Vec<PathBuf>>,
    realpaths: BTreeMap<String, PathBuf>,
    failures: BTreeMap<FailureKey, HostError>,
}

impl MemoryCompilerHost {
    pub fn builder(current_directory: impl Into<PathBuf>) -> MemoryCompilerHostBuilder {
        MemoryCompilerHostBuilder::new(current_directory)
    }

    fn key(&self, path: &Path, operation: HostOperation) -> Result<String, HostError> {
        path_key(path, self.case_sensitive).map_err(|detail| {
            HostError::new(
                HostErrorKind::InvalidInput,
                operation,
                Some(path.to_path_buf()),
                detail,
            )
        })
    }

    fn failure(&self, operation: HostOperation, path: Option<&Path>) -> Option<HostError> {
        let path = match path {
            Some(path) => Some(path_key(path, self.case_sensitive).ok()?),
            None => None,
        };
        self.failures.get(&FailureKey { operation, path }).cloned()
    }

    fn existing_display_path(&self, key: &str) -> Option<&Path> {
        self.files
            .get(key)
            .map(|entry| entry.display_path.as_path())
            .or_else(|| {
                self.directories
                    .get(key)
                    .map(|entry| entry.display_path.as_path())
            })
    }
}

impl CompilerHost for MemoryCompilerHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        if let Some(error) = self.failure(HostOperation::CurrentDirectory, None) {
            return Err(error);
        }
        Ok(self.current_directory.clone())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        let key = self.key(path, HostOperation::ReadFile)?;
        if let Some(error) = self.failure(HostOperation::ReadFile, Some(path)) {
            return Err(error);
        }
        Ok(self.files.get(&key).map(|entry| entry.bytes.clone()))
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        let key = self.key(path, HostOperation::FileExists)?;
        if let Some(error) = self.failure(HostOperation::FileExists, Some(path)) {
            return Err(error);
        }
        Ok(self.files.contains_key(&key))
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        let key = self.key(path, HostOperation::DirectoryExists)?;
        if let Some(error) = self.failure(HostOperation::DirectoryExists, Some(path)) {
            return Err(error);
        }
        Ok(self.directories.contains_key(&key))
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        let key = self.key(path, HostOperation::ReadDirectory)?;
        if let Some(error) = self.failure(HostOperation::ReadDirectory, Some(path)) {
            return Err(error);
        }
        if !self.directories.contains_key(&key) {
            return Ok(Vec::new());
        }

        Ok(self
            .directory_entries
            .get(&key)
            .cloned()
            .unwrap_or_default())
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        let key = self.key(path, HostOperation::Realpath)?;
        if let Some(error) = self.failure(HostOperation::Realpath, Some(path)) {
            return Err(error);
        }
        if let Some(target) = self.realpaths.get(&key) {
            return Ok(Some(target.clone()));
        }
        Ok(self
            .existing_display_path(&key)
            .map(std::path::Path::to_path_buf))
    }
}

/// Consuming builder for an immutable [`MemoryCompilerHost`].
#[derive(Clone, Debug)]
pub struct MemoryCompilerHostBuilder {
    current_directory: PathBuf,
    case_sensitive: bool,
    files: Vec<(PathBuf, Vec<u8>)>,
    directories: Vec<PathBuf>,
    realpaths: Vec<(PathBuf, PathBuf)>,
    failures: Vec<HostError>,
}

impl MemoryCompilerHostBuilder {
    pub fn new(current_directory: impl Into<PathBuf>) -> Self {
        Self {
            current_directory: current_directory.into(),
            case_sensitive: true,
            files: Vec::new(),
            directories: Vec::new(),
            realpaths: Vec::new(),
            failures: Vec::new(),
        }
    }

    pub fn case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.case_sensitive = case_sensitive;
        self
    }

    pub fn file(mut self, path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.push((path.into(), bytes.into()));
        self
    }

    pub fn directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.directories.push(path.into());
        self
    }

    /// Override `realpath(path)` for an entry. Both `path` and `target`
    /// must also exist as a file or directory in the completed host.
    pub fn realpath(mut self, path: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        self.realpaths.push((path.into(), target.into()));
        self
    }

    /// Inject a deterministic operation failure for contract and resolver
    /// tests. The error's operation and optional path select the call.
    pub fn failure(mut self, error: HostError) -> Self {
        self.failures.push(error);
        self
    }

    pub fn build(self) -> Result<MemoryCompilerHost, HostError> {
        let operation = HostOperation::BuildMemoryHost;
        path_key(&self.current_directory, self.case_sensitive).map_err(|detail| {
            HostError::new(
                HostErrorKind::InvalidInput,
                operation,
                Some(self.current_directory.clone()),
                detail,
            )
        })?;

        let mut files: BTreeMap<String, FileEntry> = BTreeMap::new();
        for (display_path, bytes) in self.files {
            let key = build_key(&display_path, self.case_sensitive)?;
            match files.get(&key) {
                Some(existing) if existing.bytes == bytes => {}
                Some(_) => {
                    return Err(identity_conflict(
                        &display_path,
                        "the same host file identity has incompatible bytes",
                    ));
                }
                None => {
                    files.insert(
                        key,
                        FileEntry {
                            display_path,
                            bytes,
                        },
                    );
                }
            }
        }

        let mut directory_paths = Vec::new();
        directory_paths.push(self.current_directory.clone());
        directory_paths.extend(self.directories);
        for entry in files.values() {
            add_parent_directories(&entry.display_path, &mut directory_paths);
        }
        let explicit_directories = directory_paths.clone();
        for directory in explicit_directories {
            add_parent_directories(&directory, &mut directory_paths);
        }

        let mut directories: BTreeMap<String, DirectoryEntry> = BTreeMap::new();
        for display_path in directory_paths {
            if display_path.as_os_str().is_empty() {
                continue;
            }
            let key = build_key(&display_path, self.case_sensitive)?;
            directories
                .entry(key)
                .or_insert(DirectoryEntry { display_path });
        }

        if let Some((key, file)) = files.iter().find(|(key, _)| directories.contains_key(*key)) {
            let directory = &directories[key].display_path;
            return Err(identity_conflict(
                &file.display_path,
                format!(
                    "host identity is both file {} and directory {}",
                    file.display_path.display(),
                    directory.display()
                ),
            ));
        }

        let mut directory_entry_maps: BTreeMap<String, BTreeMap<(String, String), PathBuf>> =
            BTreeMap::new();
        for display_path in files
            .values()
            .map(|entry| entry.display_path.as_path())
            .chain(
                directories
                    .values()
                    .map(|entry| entry.display_path.as_path()),
            )
        {
            let Some(parent) = display_path.parent() else {
                continue;
            };
            if parent.as_os_str().is_empty() {
                continue;
            }
            let parent_key = build_key(parent, self.case_sensitive)?;
            let canonical = build_key(display_path, self.case_sensitive)?;
            let display = display_path
                .to_str()
                .expect("stored memory-host paths are representable")
                .to_owned();
            directory_entry_maps
                .entry(parent_key)
                .or_default()
                .insert((canonical, display), display_path.to_path_buf());
        }
        let directory_entries = directory_entry_maps
            .into_iter()
            .map(|(directory, entries)| (directory, entries.into_values().collect()))
            .collect();

        let existing_keys: BTreeSet<&str> = files
            .keys()
            .chain(directories.keys())
            .map(String::as_str)
            .collect();
        let mut realpaths = BTreeMap::new();
        for (display_path, target) in self.realpaths {
            let key = build_key(&display_path, self.case_sensitive)?;
            let target_key = build_key(&target, self.case_sensitive)?;
            if !existing_keys.contains(key.as_str()) || !existing_keys.contains(target_key.as_str())
            {
                return Err(HostError::new(
                    HostErrorKind::InvalidData,
                    operation,
                    Some(display_path),
                    format!(
                        "realpath source and target must both exist; target was {}",
                        target.display()
                    ),
                ));
            }
            let source_is_file = files.contains_key(&key);
            let target_is_file = files.contains_key(&target_key);
            if source_is_file != target_is_file {
                return Err(HostError::new(
                    HostErrorKind::InvalidData,
                    operation,
                    Some(display_path),
                    format!(
                        "realpath source and target must have the same entry kind; target was {}",
                        target.display()
                    ),
                ));
            }
            let target_display = files
                .get(&target_key)
                .map(|entry| entry.display_path.clone())
                .or_else(|| {
                    directories
                        .get(&target_key)
                        .map(|entry| entry.display_path.clone())
                })
                .expect("realpath target key was validated");
            match realpaths.get(&key) {
                Some(existing) if existing == &target_display => {}
                Some(_) => {
                    return Err(identity_conflict(
                        &display_path,
                        "the same realpath identity has incompatible targets",
                    ));
                }
                None => {
                    realpaths.insert(key, target_display);
                }
            }
        }

        let mut failures = BTreeMap::new();
        for error in self.failures {
            let path_is_valid = match error.operation() {
                HostOperation::CurrentDirectory => error.path().is_none(),
                HostOperation::ReadFile
                | HostOperation::FileExists
                | HostOperation::DirectoryExists
                | HostOperation::ReadDirectory
                | HostOperation::Realpath => error.path().is_some(),
                HostOperation::BuildMemoryHost | HostOperation::DetectCaseSensitivity => false,
            };
            if !path_is_valid {
                return Err(HostError::new(
                    HostErrorKind::InvalidData,
                    operation,
                    error.path().map(Path::to_path_buf),
                    "injected failure operation and path do not form a callable host query",
                ));
            }
            let path = error
                .path()
                .map(|path| build_key(path, self.case_sensitive))
                .transpose()?;
            let key = FailureKey {
                operation: error.operation(),
                path,
            };
            if failures.insert(key, error.clone()).is_some() {
                return Err(HostError::new(
                    HostErrorKind::IdentityConflict,
                    operation,
                    error.path().map(Path::to_path_buf),
                    "duplicate injected failure identity",
                ));
            }
        }

        Ok(MemoryCompilerHost {
            current_directory: self.current_directory,
            case_sensitive: self.case_sensitive,
            files,
            directories,
            directory_entries,
            realpaths,
            failures,
        })
    }
}

fn build_key(path: &Path, case_sensitive: bool) -> Result<String, HostError> {
    path_key(path, case_sensitive).map_err(|detail| {
        HostError::new(
            HostErrorKind::InvalidInput,
            HostOperation::BuildMemoryHost,
            Some(path.to_path_buf()),
            detail,
        )
    })
}

fn path_key(path: &Path, case_sensitive: bool) -> Result<String, &'static str> {
    if path.as_os_str().is_empty() {
        return Err("path is empty");
    }
    let path = path
        .to_str()
        .ok_or("path is not representable as Unicode text")?;
    if path.contains('\0') {
        return Err("path contains a null character");
    }
    Ok(if case_sensitive {
        path.to_owned()
    } else {
        to_file_name_lower_case(path)
    })
}

fn add_parent_directories(path: &Path, directories: &mut Vec<PathBuf>) {
    let mut parent = path.parent();
    while let Some(directory) = parent {
        if directory.as_os_str().is_empty() {
            break;
        }
        directories.push(directory.to_path_buf());
        parent = directory.parent();
    }
}

fn identity_conflict(path: &Path, detail: impl Into<String>) -> HostError {
    HostError::new(
        HostErrorKind::IdentityConflict,
        HostOperation::BuildMemoryHost,
        Some(path.to_path_buf()),
        detail,
    )
}

#[cfg(test)]
mod tests {
    use crate::to_file_name_lower_case;

    #[test]
    fn vendored_file_name_fold_preserves_special_code_points() {
        assert_eq!(
            to_file_name_lower_case("/W/Ä/İıß/FILE.TS"),
            "/w/ä/İıß/file.ts"
        );
        assert_eq!(to_file_name_lower_case("/W/ẞ/ΣΟΣ.TS"), "/w/ß/σος.ts");
    }
}
