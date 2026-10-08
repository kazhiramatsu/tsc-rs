//! A compiler host over a file system.

use std::io;
use std::path::{Path, PathBuf};

use tsc_diagnostics::{JsStr, JsString};

use super::{normalize, FileSystem};
use crate::ordering::compare_utf16;
use crate::{
    CompilerHost, DirectoryListingEntry, DirectoryListingKind, HostError, HostErrorKind,
    HostOperation,
};

/// A [`CompilerHost`] over a [`FileSystem`]: program construction reads
/// what the file system holds, as tsgo's compiler host reads its `vfs.FS`.
/// A relative path is taken from the current directory.
///
/// As in tsgo, a read that cannot be answered is an absent entry; only a
/// denied permission is a [`HostError`].
#[derive(Clone, Debug)]
pub struct VfsCompilerHost<F> {
    fs: F,
    current_directory: String,
}

impl<F: FileSystem> VfsCompilerHost<F> {
    pub fn new(fs: F, current_directory: impl Into<String>) -> Self {
        Self {
            fs,
            current_directory: current_directory.into(),
        }
    }

    pub fn file_system(&self) -> &F {
        &self.fs
    }

    /// The file-system path a compiler path names.
    fn query(&self, path: JsStr<'_>, operation: HostOperation) -> Result<String, HostError> {
        crate::js_path::validate(path, operation)?;
        let path = path.to_string_lossy();
        Ok(if normalize(&path).is_ok() || path.contains("://") {
            path.into_owned()
        } else {
            format!("{}/{path}", self.current_directory.trim_end_matches('/'))
        })
    }

    fn error(
        &self,
        operation: HostOperation,
        path: &str,
        error: io::Error,
    ) -> Result<(), HostError> {
        if error.kind() == io::ErrorKind::PermissionDenied {
            Err(HostError::new_js(
                HostErrorKind::PermissionDenied,
                operation,
                Some(path.into()),
                error.to_string(),
            ))
        } else {
            Ok(())
        }
    }

    /// The accessible entries of a directory, ordered by name in UTF-16
    /// code-unit order, with their kinds.
    fn listing(
        &self,
        path: JsStr<'_>,
        operation: HostOperation,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        let directory = self.query(path, operation)?;
        if let Err(error) = self.fs.read_dir(&directory) {
            self.error(operation, &directory, error)?;
            return Ok(Vec::new());
        }
        let entries = self.fs.accessible_entries(&directory);
        let mut listing = entries
            .files
            .iter()
            .map(|name| (name, DirectoryListingKind::File))
            .chain(
                entries
                    .directories
                    .iter()
                    .map(|name| (name, DirectoryListingKind::Directory)),
            )
            .collect::<Vec<_>>();
        listing.sort_by(|(left, _), (right, _)| compare_utf16(left, right));
        Ok(listing
            .into_iter()
            .map(|(name, kind)| DirectoryListingEntry {
                path: crate::js_path::join_observed_name(path, name),
                kind,
                symlink: entries.symlinks.contains(name),
            })
            .collect())
    }
}

impl<F: FileSystem> CompilerHost for VfsCompilerHost<F> {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        Ok(JsString::from(self.current_directory.as_str()))
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        let path = self.query(path, HostOperation::ReadFile)?;
        match self.fs.read(&path) {
            Ok(contents) => Ok(Some(contents)),
            Err(error) => self
                .error(HostOperation::ReadFile, &path, error)
                .map(|()| None),
        }
    }

    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        let path = self.query(path, HostOperation::ReadFile)?;
        Ok(self
            .fs
            .metadata(&path)
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.len()))
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let path = self.query(path, HostOperation::FileExists)?;
        Ok(self.fs.is_file(&path))
    }

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        let path = self.query(path, HostOperation::DirectoryExists)?;
        Ok(self.fs.is_dir(&path))
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        Ok(self
            .listing(path, HostOperation::ReadDirectory)?
            .into_iter()
            .map(|entry| entry.path)
            .collect())
    }

    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        self.listing(path, HostOperation::ReadDirectory)
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        Ok(self
            .listing(path, HostOperation::ReadDirectory)?
            .into_iter()
            .filter(|entry| entry.kind == DirectoryListingKind::Directory)
            .map(|entry| entry.path)
            .collect())
    }

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        let query = self.query(path, HostOperation::Realpath)?;
        match self.fs.canonicalize(&query) {
            Ok(physical) => Ok(Some(JsString::from(physical))),
            Err(error) => self
                .error(HostOperation::Realpath, &query, error)
                .map(|()| None),
        }
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        Ok(PathBuf::from(&self.current_directory))
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.fs.case_sensitive()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        self.read_file_js(native(path, HostOperation::ReadFile)?.as_js())
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.file_exists_js(native(path, HostOperation::FileExists)?.as_js())
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        self.directory_exists_js(native(path, HostOperation::DirectoryExists)?.as_js())
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        Ok(self
            .read_directory_js(native(path, HostOperation::ReadDirectory)?.as_js())?
            .into_iter()
            .map(|entry| PathBuf::from(entry.to_string_lossy().into_owned()))
            .collect())
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        Ok(self
            .get_directories_js(native(path, HostOperation::ReadDirectory)?.as_js())?
            .into_iter()
            .map(|entry| PathBuf::from(entry.to_string_lossy().into_owned()))
            .collect())
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        Ok(self
            .realpath_js(native(path, HostOperation::Realpath)?.as_js())?
            .map(|physical| PathBuf::from(physical.to_string_lossy().into_owned())))
    }
}

fn native(path: &Path, operation: HostOperation) -> Result<JsString, HostError> {
    crate::js_path::from_native(path, operation, HostErrorKind::InvalidInput)
}
