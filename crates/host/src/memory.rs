use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{CompilerHost, HostError, HostErrorKind, HostOperation};
use tsc_diagnostics::{JsStr, JsString};

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileEntry {
    display_path: JsString,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectoryEntry {
    display_path: JsString,
}

type DirectoryEntryMap = BTreeMap<(JsString, JsString), (JsString, JsString)>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FailureKey {
    operation: HostOperation,
    path: Option<JsString>,
}

/// Immutable in-memory implementation of [`CompilerHost`].
///
/// The builder infers parent directories but performs no lexical path
/// normalization and no current-directory joining. File bytes are retained
/// exactly. On a case-insensitive profile, lookup identities use the exact
/// TypeScript 6.0.3 file-name case fold rather than a locale-sensitive fold.
#[derive(Clone, Debug)]
pub struct MemoryCompilerHost {
    current_directory: JsString,
    case_sensitive: bool,
    files: BTreeMap<JsString, FileEntry>,
    directories: BTreeMap<JsString, DirectoryEntry>,
    directory_entries: BTreeMap<JsString, Vec<JsString>>,
    realpaths: BTreeMap<JsString, JsString>,
    failures: BTreeMap<FailureKey, HostError>,
}

impl MemoryCompilerHost {
    pub fn builder(current_directory: impl Into<PathBuf>) -> MemoryCompilerHostBuilder {
        MemoryCompilerHostBuilder::new(current_directory)
    }

    pub fn builder_js(current_directory: impl Into<JsString>) -> MemoryCompilerHostBuilder {
        MemoryCompilerHostBuilder::new_js(current_directory)
    }

    fn key(&self, path: JsStr<'_>, operation: HostOperation) -> Result<JsString, HostError> {
        crate::js_path::validate(path, operation)?;
        Ok(path_key(path, self.case_sensitive))
    }

    fn failure(&self, operation: HostOperation, path: Option<JsStr<'_>>) -> Option<HostError> {
        let path = path.map(|path| path_key(path, self.case_sensitive));
        self.failures.get(&FailureKey { operation, path }).cloned()
    }

    fn existing_display_path(&self, key: &JsString) -> Option<JsStr<'_>> {
        self.files
            .get(key)
            .map(|entry| entry.display_path.as_js())
            .or_else(|| {
                self.directories
                    .get(key)
                    .map(|entry| entry.display_path.as_js())
            })
    }
}

impl CompilerHost for MemoryCompilerHost {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        if let Some(error) = self.failure(HostOperation::CurrentDirectory, None) {
            return Err(error);
        }
        Ok(self.current_directory.clone())
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.case_sensitive
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        let key = self.key(path, HostOperation::ReadFile)?;
        if let Some(error) = self.failure(HostOperation::ReadFile, Some(path)) {
            return Err(error);
        }
        Ok(self.files.get(&key).map(|entry| entry.bytes.clone()))
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let key = self.key(path, HostOperation::FileExists)?;
        if let Some(error) = self.failure(HostOperation::FileExists, Some(path)) {
            return Err(error);
        }
        Ok(self.files.contains_key(&key))
    }

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let key = self.key(path, HostOperation::DirectoryExists)?;
        if let Some(error) = self.failure(HostOperation::DirectoryExists, Some(path)) {
            return Err(error);
        }
        Ok(self.directories.contains_key(&key))
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
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

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
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
            .into_iter()
            .flatten()
            .filter(|entry| {
                self.directories
                    .contains_key(&path_key(entry.as_js(), self.case_sensitive))
            })
            .cloned()
            .collect())
    }

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        let key = self.key(path, HostOperation::Realpath)?;
        if let Some(error) = self.failure(HostOperation::Realpath, Some(path)) {
            return Err(error);
        }
        if let Some(target) = self.realpaths.get(&key) {
            return Ok(Some(target.clone()));
        }
        Ok(self.existing_display_path(&key).map(JsStr::to_owned))
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        scalar_native_result(
            self.current_directory_js()?.as_js(),
            HostOperation::CurrentDirectory,
        )
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.read_file_js(native_query(path, HostOperation::ReadFile)?.as_js())
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.file_exists_js(native_query(path, HostOperation::FileExists)?.as_js())
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.directory_exists_js(native_query(path, HostOperation::DirectoryExists)?.as_js())
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.read_directory_js(native_query(path, HostOperation::ReadDirectory)?.as_js())?
            .into_iter()
            .map(|path| scalar_native_result(path.as_js(), HostOperation::ReadDirectory))
            .collect()
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        self.get_directories_js(native_query(path, HostOperation::ReadDirectory)?.as_js())?
            .into_iter()
            .map(|path| scalar_native_result(path.as_js(), HostOperation::ReadDirectory))
            .collect()
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        self.realpath_js(native_query(path, HostOperation::Realpath)?.as_js())?
            .map(|path| scalar_native_result(path.as_js(), HostOperation::Realpath))
            .transpose()
    }
}

fn native_query(path: &Path, operation: HostOperation) -> Result<JsString, HostError> {
    crate::js_path::from_native(path, operation, HostErrorKind::InvalidInput)
}

fn scalar_native_result(path: JsStr<'_>, operation: HostOperation) -> Result<PathBuf, HostError> {
    path.as_str().map(PathBuf::from).ok_or_else(|| {
        HostError::new_js(
            HostErrorKind::InvalidData,
            operation,
            Some(path),
            "use the JavaScript host method for a non-scalar path result",
        )
    })
}

#[derive(Clone, Debug)]
enum PathInput {
    Native(PathBuf),
    Js(JsString),
}

impl PathInput {
    fn into_js(self) -> Result<JsString, HostError> {
        let operation = HostOperation::BuildMemoryHost;
        let path = match self {
            Self::Native(path) => native_query(&path, operation)?,
            Self::Js(path) => path,
        };
        crate::js_path::validate(path.as_js(), operation)?;
        Ok(path)
    }
}

/// Consuming builder for an immutable [`MemoryCompilerHost`].
#[derive(Clone, Debug)]
pub struct MemoryCompilerHostBuilder {
    current_directory: PathInput,
    case_sensitive: bool,
    files: Vec<(PathInput, Vec<u8>)>,
    directories: Vec<PathInput>,
    realpaths: Vec<(PathInput, PathInput)>,
    failures: Vec<HostError>,
}

impl MemoryCompilerHostBuilder {
    pub fn new(current_directory: impl Into<PathBuf>) -> Self {
        Self::with_path_input(PathInput::Native(current_directory.into()))
    }

    pub fn new_js(current_directory: impl Into<JsString>) -> Self {
        Self::with_path_input(PathInput::Js(current_directory.into()))
    }

    fn with_path_input(current_directory: PathInput) -> Self {
        Self {
            current_directory,
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
        self.files
            .push((PathInput::Native(path.into()), bytes.into()));
        self
    }

    pub fn file_js(mut self, path: impl Into<JsString>, bytes: impl Into<Vec<u8>>) -> Self {
        self.files.push((PathInput::Js(path.into()), bytes.into()));
        self
    }

    pub fn directory(mut self, path: impl Into<PathBuf>) -> Self {
        self.directories.push(PathInput::Native(path.into()));
        self
    }

    pub fn directory_js(mut self, path: impl Into<JsString>) -> Self {
        self.directories.push(PathInput::Js(path.into()));
        self
    }

    /// Override `realpath(path)` for an entry. Both `path` and `target`
    /// must also exist as a file or directory in the completed host.
    pub fn realpath(mut self, path: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        self.realpaths.push((
            PathInput::Native(path.into()),
            PathInput::Native(target.into()),
        ));
        self
    }

    pub fn realpath_js(mut self, path: impl Into<JsString>, target: impl Into<JsString>) -> Self {
        self.realpaths
            .push((PathInput::Js(path.into()), PathInput::Js(target.into())));
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
        let current_directory = self.current_directory.into_js()?;

        let mut files: BTreeMap<JsString, FileEntry> = BTreeMap::new();
        for (display_path, bytes) in self.files {
            let display_path = display_path.into_js()?;
            let key = build_key(display_path.as_js(), self.case_sensitive)?;
            match files.get(&key) {
                Some(existing) if existing.bytes == bytes => {}
                Some(_) => {
                    return Err(identity_conflict(
                        display_path.as_js(),
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
        directory_paths.push(current_directory.clone());
        directory_paths.extend(
            self.directories
                .into_iter()
                .map(PathInput::into_js)
                .collect::<Result<Vec<_>, _>>()?,
        );
        for entry in files.values() {
            add_parent_directories(entry.display_path.as_js(), &mut directory_paths);
        }
        let explicit_directories = directory_paths.clone();
        for directory in explicit_directories {
            add_parent_directories(directory.as_js(), &mut directory_paths);
        }

        let mut directories: BTreeMap<JsString, DirectoryEntry> = BTreeMap::new();
        for display_path in directory_paths {
            if display_path.is_empty() {
                continue;
            }
            let key = build_key(display_path.as_js(), self.case_sensitive)?;
            directories
                .entry(key)
                .or_insert(DirectoryEntry { display_path });
        }

        if let Some((key, file)) = files.iter().find(|(key, _)| directories.contains_key(*key)) {
            let directory = &directories[key].display_path;
            return Err(identity_conflict(
                file.display_path.as_js(),
                format!(
                    "host identity is both file {} and directory {}",
                    file.display_path.to_string_lossy(),
                    directory.to_string_lossy()
                ),
            ));
        }

        let mut directory_entry_maps: BTreeMap<JsString, DirectoryEntryMap> = BTreeMap::new();
        for display_path in files
            .values()
            .map(|entry| entry.display_path.as_js())
            .chain(directories.values().map(|entry| entry.display_path.as_js()))
        {
            let Some(parent) = crate::js_path::parent(display_path) else {
                continue;
            };
            if parent.is_empty() {
                continue;
            }
            let parent_key = build_key(parent, self.case_sensitive)?;
            let canonical = build_key(display_path, self.case_sensitive)?;
            let display = display_path.to_owned();
            let display_name = crate::js_path::child_name(display_path)
                .expect("stored entry is below its parent")
                .to_owned();
            directory_entry_maps.entry(parent_key).or_default().insert(
                (canonical, display),
                (display_name, display_path.to_owned()),
            );
        }
        let directory_entries = directory_entry_maps
            .into_iter()
            .map(|(directory, entries)| {
                let mut entries = entries.into_values().collect::<Vec<_>>();
                entries.sort_by(|left, right| left.0.cmp_utf16(right.0.as_js()));
                (
                    directory,
                    entries.into_iter().map(|(_, path)| path).collect(),
                )
            })
            .collect();

        let existing_keys: BTreeSet<JsString> =
            files.keys().chain(directories.keys()).cloned().collect();
        let mut realpaths = BTreeMap::new();
        for (display_path, target) in self.realpaths {
            let display_path = display_path.into_js()?;
            let target = target.into_js()?;
            let key = build_key(display_path.as_js(), self.case_sensitive)?;
            let target_key = build_key(target.as_js(), self.case_sensitive)?;
            if !existing_keys.contains(&key) || !existing_keys.contains(&target_key) {
                return Err(HostError::new_js(
                    HostErrorKind::InvalidData,
                    operation,
                    Some(display_path.as_js()),
                    format!(
                        "realpath source and target must both exist; target was {}",
                        target.to_string_lossy()
                    ),
                ));
            }
            let source_is_file = files.contains_key(&key);
            let target_is_file = files.contains_key(&target_key);
            if source_is_file != target_is_file {
                return Err(HostError::new_js(
                    HostErrorKind::InvalidData,
                    operation,
                    Some(display_path.as_js()),
                    format!(
                        "realpath source and target must have the same entry kind; target was {}",
                        target.to_string_lossy()
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
                        display_path.as_js(),
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
            let path = if let Some(path) = error.js_path() {
                Some(build_key(path, self.case_sensitive)?)
            } else if let Some(path) = error.path() {
                // Native errors may retain an unrepresentable OS path. It
                // remains an invalid injected key, not a pathless failure.
                Some(build_key(
                    native_query(path, operation)?.as_js(),
                    self.case_sensitive,
                )?)
            } else {
                None
            };
            let key = FailureKey {
                operation: error.operation(),
                path,
            };
            if failures.insert(key, error.clone()).is_some() {
                return Err(HostError::new_js(
                    HostErrorKind::IdentityConflict,
                    operation,
                    error.js_path(),
                    "duplicate injected failure identity",
                ));
            }
        }

        Ok(MemoryCompilerHost {
            current_directory,
            case_sensitive: self.case_sensitive,
            files,
            directories,
            directory_entries,
            realpaths,
            failures,
        })
    }
}

fn build_key(path: JsStr<'_>, case_sensitive: bool) -> Result<JsString, HostError> {
    crate::js_path::validate(path, HostOperation::BuildMemoryHost)?;
    Ok(path_key(path, case_sensitive))
}

fn path_key(path: JsStr<'_>, case_sensitive: bool) -> JsString {
    if case_sensitive {
        path.to_owned()
    } else {
        crate::to_file_name_lower_case_js(path)
    }
}

fn add_parent_directories(path: JsStr<'_>, directories: &mut Vec<JsString>) {
    let mut parent = crate::js_path::parent(path);
    while let Some(directory) = parent {
        if directory.is_empty() {
            break;
        }
        directories.push(directory.to_owned());
        parent = crate::js_path::parent(directory);
    }
}

fn identity_conflict(path: JsStr<'_>, detail: impl Into<String>) -> HostError {
    HostError::new_js(
        HostErrorKind::IdentityConflict,
        HostOperation::BuildMemoryHost,
        Some(path),
        detail,
    )
}

#[cfg(test)]
#[path = "../tests/unit/memory/tests.rs"]
mod tests;
