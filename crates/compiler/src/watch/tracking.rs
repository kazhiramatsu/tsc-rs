//! A compiler host that records every path a program's creation asks
//! about (tsgo `trackingvfs`): the files it read, the files and directories
//! it probed, listed or resolved, the missing ones included. A watch run
//! watches the directories of those paths and treats a change to any of
//! them as a change to the program.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{CompilerHost, DirectoryListingEntry, HostError, ParallelSourceReader};

type Seen = Arc<Mutex<BTreeSet<String>>>;

fn record(seen: &Seen, path: impl Into<String>) {
    seen.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.into());
}

fn record_js(seen: &Seen, path: JsStr<'_>) {
    record(seen, path.to_string_lossy().into_owned());
}

fn record_native(seen: &Seen, path: &Path) {
    record(seen, path.to_string_lossy().into_owned());
}

/// The recording host over `inner`.
pub(crate) struct TrackingHost<'a> {
    inner: &'a dyn CompilerHost,
    seen: Seen,
    parallel: Option<ParallelTracking<'a>>,
}

impl<'a> TrackingHost<'a> {
    pub(crate) fn new(inner: &'a dyn CompilerHost) -> Self {
        let seen = Seen::default();
        // The parallel views are recorded too: a source read ahead or a
        // module resolved on another thread is part of the program.
        let parallel = inner
            .parallel_source_reader()
            .map(|reader| ParallelTracking {
                reader,
                resolution: inner.parallel_resolution_host(),
                seen: Arc::clone(&seen),
            });
        Self {
            inner,
            seen,
            parallel,
        }
    }

    /// Records a path the program depends on outside the host's view (the
    /// configuration files, the include directories).
    pub(crate) fn add(&self, path: &str) {
        record(&self.seen, path);
    }

    /// Every path recorded, sorted.
    pub(crate) fn seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }
}

impl CompilerHost for TrackingHost<'_> {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.inner.current_directory_js()
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        record_js(&self.seen, path);
        self.inner.read_file_js(path)
    }

    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        self.inner.file_size_hint_js(path)
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        record_js(&self.seen, path);
        self.inner.file_exists_js(path)
    }

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        record_js(&self.seen, path);
        self.inner.directory_exists_js(path)
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        record_js(&self.seen, path);
        self.inner.read_directory_js(path)
    }

    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        record_js(&self.seen, path);
        self.inner.read_directory_listing_js(path)
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        record_js(&self.seen, path);
        self.inner.get_directories_js(path)
    }

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        record_js(&self.seen, path);
        self.inner.realpath_js(path)
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.inner.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.inner.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        record_native(&self.seen, path);
        self.inner.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        record_native(&self.seen, path);
        self.inner.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        record_native(&self.seen, path);
        self.inner.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        record_native(&self.seen, path);
        self.inner.read_directory(path)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        record_native(&self.seen, path);
        self.inner.get_directories(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        record_native(&self.seen, path);
        self.inner.realpath(path)
    }

    fn permits_source_read_ahead(&self) -> bool {
        self.inner.permits_source_read_ahead()
    }

    fn parallel_source_reader(&self) -> Option<&(dyn ParallelSourceReader + Sync)> {
        self.parallel
            .as_ref()
            .map(|parallel| parallel as &(dyn ParallelSourceReader + Sync))
    }

    fn parallel_resolution_host(&self) -> Option<&(dyn CompilerHost + Sync)> {
        self.parallel
            .as_ref()
            .filter(|parallel| parallel.resolution.is_some())
            .map(|parallel| parallel as &(dyn CompilerHost + Sync))
    }
}

/// The host's views other threads use, recording into the same set.
struct ParallelTracking<'a> {
    reader: &'a (dyn ParallelSourceReader + Sync),
    resolution: Option<&'a (dyn CompilerHost + Sync)>,
    seen: Seen,
}

impl ParallelTracking<'_> {
    fn host(&self) -> &(dyn CompilerHost + Sync) {
        self.resolution
            .expect("the resolution view exists only over a resolution host")
    }
}

impl ParallelSourceReader for ParallelTracking<'_> {
    fn read_source_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        record_js(&self.seen, path);
        self.reader.read_source_js(path)
    }
}

impl CompilerHost for ParallelTracking<'_> {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.host().current_directory_js()
    }

    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        record_js(&self.seen, path);
        self.host().read_file_js(path)
    }

    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        self.host().file_size_hint_js(path)
    }

    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        record_js(&self.seen, path);
        self.host().file_exists_js(path)
    }

    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        record_js(&self.seen, path);
        self.host().directory_exists_js(path)
    }

    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        record_js(&self.seen, path);
        self.host().read_directory_js(path)
    }

    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<DirectoryListingEntry>, HostError> {
        record_js(&self.seen, path);
        self.host().read_directory_listing_js(path)
    }

    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        record_js(&self.seen, path);
        self.host().get_directories_js(path)
    }

    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        record_js(&self.seen, path);
        self.host().realpath_js(path)
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.host().current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.host().use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        record_native(&self.seen, path);
        self.host().read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        record_native(&self.seen, path);
        self.host().file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        record_native(&self.seen, path);
        self.host().directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        record_native(&self.seen, path);
        self.host().read_directory(path)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        record_native(&self.seen, path);
        self.host().get_directories(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        record_native(&self.seen, path);
        self.host().realpath(path)
    }

    fn permits_source_read_ahead(&self) -> bool {
        self.host().permits_source_read_ahead()
    }
}
