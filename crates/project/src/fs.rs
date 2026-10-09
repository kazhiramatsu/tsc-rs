//! The files a snapshot has read (tsgo `SnapshotFS`, snapshotfs.go): the
//! base file system (the host's, or one an API request supplies) and the
//! contents read through it, kept until a file change says otherwise.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use tsc_diagnostics::JsStr;
use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, Metadata};

/// tsgo `excessiveChangeThreshold`.
const EXCESSIVE_CHANGE_THRESHOLD: usize = 1000;

/// File changes a snapshot update reports (tsgo `FileChangeSummary`), by
/// absolute file name. `invalidate_all` drops every assumption about the
/// files read so far.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileChangeSummary {
    pub changed: BTreeSet<String>,
    pub created: BTreeSet<String>,
    pub deleted: BTreeSet<String>,
    pub invalidate_all: bool,
}

impl FileChangeSummary {
    pub fn is_empty(&self) -> bool {
        !self.invalidate_all
            && self.changed.is_empty()
            && self.created.is_empty()
            && self.deleted.is_empty()
    }

    /// tsgo `HasExcessiveWatchEvents`.
    pub(crate) fn has_excessive_watch_events(&self) -> bool {
        self.invalidate_all
            || self.created.len() + self.deleted.len() + self.changed.len()
                > EXCESSIVE_CHANGE_THRESHOLD
    }

    /// tsgo `HasExcessiveNonCreateWatchEvents`.
    pub(crate) fn has_excessive_non_create_watch_events(&self) -> bool {
        self.invalidate_all || self.deleted.len() + self.changed.len() > EXCESSIVE_CHANGE_THRESHOLD
    }
}

/// How a session names files: tsgo `tspath.ToPath` against its current
/// directory and file system case sensitivity.
#[derive(Clone, Debug)]
pub(crate) struct Paths {
    pub(crate) current_directory: String,
    pub(crate) case_sensitive: bool,
}

impl Paths {
    /// tsgo `GetNormalizedAbsolutePath`.
    pub(crate) fn absolute(&self, file_name: &str) -> String {
        tsc_program::get_normalized_absolute_path(
            JsStr::from_str(file_name),
            JsStr::from_str(&self.current_directory),
        )
        .to_string_lossy()
        .into_owned()
    }

    /// tsgo `tspath.ToPath`.
    pub(crate) fn to_path(&self, file_name: &str) -> String {
        let absolute = self.absolute(file_name);
        if self.case_sensitive {
            absolute
        } else {
            tsc_host::to_file_name_lower_case(&absolute)
        }
    }
}

/// A file read through a snapshot (tsgo `cachedFile`).
#[derive(Debug)]
pub(crate) struct CachedFile {
    pub(crate) file_name: String,
    pub(crate) content: Arc<[u8]>,
    /// The file may have changed: read it again before use (tsgo
    /// `needsReload`).
    pub(crate) needs_reload: bool,
}

type Files = Arc<BTreeMap<String, Arc<CachedFile>>>;

/// A snapshot's view of the files (tsgo `SnapshotFS`).
#[derive(Clone)]
pub(crate) struct SnapshotFs {
    pub(crate) base: Arc<dyn FileSystem>,
    pub(crate) files: Files,
    paths: Paths,
}

impl SnapshotFs {
    pub(crate) fn new(base: Arc<dyn FileSystem>, paths: Paths) -> Self {
        Self {
            base,
            files: Arc::default(),
            paths,
        }
    }

    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }

    /// tsgo `SnapshotFS.GetFile`: the file as the snapshot read it, or as
    /// its file system holds it.
    pub(crate) fn read(&self, file_name: &str) -> Option<Vec<u8>> {
        match self.files.get(&self.paths.to_path(file_name)) {
            Some(file) => Some(file.content.to_vec()),
            None => self.base.read(file_name).ok(),
        }
    }
}

/// The view a snapshot build reads through (tsgo `snapshotFSBuilder`): the
/// base snapshot's files over the new base file system, read again or
/// dropped as the build's file changes say.
pub(crate) struct SnapshotFsBuilder {
    base: Arc<dyn FileSystem>,
    paths: Paths,
    files: Mutex<Files>,
}

impl SnapshotFsBuilder {
    pub(crate) fn new(base: Arc<dyn FileSystem>, files: Files, paths: Paths) -> Self {
        Self {
            base,
            paths,
            files: Mutex::new(files),
        }
    }

    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }

    pub(crate) fn finish(self) -> SnapshotFs {
        SnapshotFs {
            base: self.base,
            files: self
                .files
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner),
            paths: self.paths,
        }
    }

    fn with_files<T>(&self, operation: impl FnOnce(&mut Files) -> T) -> T {
        operation(&mut self.files.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// tsgo `GetFileByPath`: the cached file (read again when marked, dropped
    /// when gone), or the file read from the base and cached.
    pub(crate) fn get(&self, file_name: &str) -> Option<Arc<CachedFile>> {
        let path = self.paths.to_path(file_name);
        self.with_files(|files| {
            let read_name = match files.get(&path) {
                Some(entry) if !entry.needs_reload => return Some(Arc::clone(entry)),
                Some(entry) => entry.file_name.clone(),
                None => file_name.to_owned(),
            };
            match self.base.read(&read_name) {
                Ok(content) => {
                    let entry = Arc::new(CachedFile {
                        file_name: read_name,
                        content: content.into(),
                        needs_reload: false,
                    });
                    Arc::make_mut(files).insert(path, Arc::clone(&entry));
                    Some(entry)
                }
                Err(_) => {
                    if files.contains_key(&path) {
                        Arc::make_mut(files).remove(&path);
                    }
                    None
                }
            }
        })
    }

    /// tsgo `DirectoryExists` through the build's (untracked) view.
    pub(crate) fn is_dir(&self, path: &str) -> bool {
        self.base.is_dir(path)
    }

    /// tsgo `FileExists`: a cached file exists while it can be read; others
    /// as the base says.
    pub(crate) fn file_exists(&self, file_name: &str) -> bool {
        let cached = self.with_files(|files| files.contains_key(&self.paths.to_path(file_name)));
        if cached {
            self.get(file_name).is_some()
        } else {
            self.base.is_file(file_name)
        }
    }

    /// tsgo `processFileChanges` (snapshot.go:134-191) for a build without
    /// open files: the changes a build acts on, the cache updated for them.
    pub(crate) fn process_file_changes(&self, mut changes: FileChangeSummary) -> FileChangeSummary {
        if changes.has_excessive_watch_events() {
            if changes.invalidate_all || self.changes_overlap_cache(&changes) {
                // The API's notifications always count as changes outside
                // node_modules (tsgo `IncludesWatchChangeOutsideNodeModules`).
                self.invalidate_cache();
            } else {
                changes.changed.clear();
                changes.deleted.clear();
            }
            return changes;
        }
        self.expand_and_filter(&mut changes);
        self.mark_dirty_files(&mut changes);
        changes
    }

    /// tsgo `watchChangesOverlapCache`.
    fn changes_overlap_cache(&self, changes: &FileChangeSummary) -> bool {
        self.with_files(|files| {
            changes
                .changed
                .iter()
                .chain(&changes.deleted)
                .any(|file_name| files.contains_key(&self.paths.to_path(file_name)))
        })
    }

    /// tsgo `invalidateCache`: every cached file is read again before use.
    fn invalidate_cache(&self) {
        self.with_files(|files| {
            let marked = files
                .iter()
                .map(|(path, entry)| {
                    let entry = Arc::new(CachedFile {
                        file_name: entry.file_name.clone(),
                        content: Arc::clone(&entry.content),
                        needs_reload: true,
                    });
                    (path.clone(), entry)
                })
                .collect();
            *files = Arc::new(marked);
        });
    }

    /// tsgo `expandAndFilterWatchEvents`: a deleted directory stands for the
    /// cached files below it; other deletions and changes count only for
    /// files that can affect a program (or anything in node_modules, for a
    /// deletion). Creations are kept: one may be a directory link.
    fn expand_and_filter(&self, changes: &mut FileChangeSummary) {
        if !changes.deleted.is_empty() {
            let mut deleted = BTreeSet::new();
            self.with_files(|files| {
                for file_name in &changes.deleted {
                    let path = self.paths.to_path(file_name);
                    let prefix = format!("{}/", path.trim_end_matches('/'));
                    let below = files
                        .range(prefix.clone()..)
                        .take_while(|(cached, _)| cached.starts_with(&prefix))
                        .map(|(_, entry)| entry.file_name.clone())
                        .collect::<Vec<_>>();
                    if !below.is_empty() {
                        deleted.extend(below);
                    } else if is_relevant_file_name(file_name)
                        || path.ends_with("/node_modules")
                        || path.contains("/node_modules/")
                    {
                        deleted.insert(file_name.clone());
                    }
                }
            });
            changes.deleted = deleted;
        }
        changes
            .changed
            .retain(|file_name| is_relevant_file_name(file_name));
    }

    /// tsgo `markDirtyFiles`: a changed file that is cached is read again
    /// and no longer counts as changed when its text is the same (or when
    /// it is gone, its entry is dropped); a deleted file's entry is dropped.
    fn mark_dirty_files(&self, changes: &mut FileChangeSummary) {
        let base = &self.base;
        self.with_files(|files| {
            changes.changed.retain(|file_name| {
                let path = self.paths.to_path(file_name);
                let Some(entry) = files.get(&path) else {
                    return true;
                };
                match base.read(&entry.file_name) {
                    Ok(content) if *content == *entry.content => {
                        if entry.needs_reload {
                            let entry = Arc::new(CachedFile {
                                file_name: entry.file_name.clone(),
                                content: Arc::clone(&entry.content),
                                needs_reload: false,
                            });
                            Arc::make_mut(files).insert(path, entry);
                        }
                        false
                    }
                    Ok(content) => {
                        let entry = Arc::new(CachedFile {
                            file_name: entry.file_name.clone(),
                            content: content.into(),
                            needs_reload: false,
                        });
                        Arc::make_mut(files).insert(path, entry);
                        true
                    }
                    Err(_) => {
                        Arc::make_mut(files).remove(&path);
                        true
                    }
                }
            });
            for file_name in &changes.deleted {
                let path = self.paths.to_path(file_name);
                if files.contains_key(&path) {
                    Arc::make_mut(files).remove(&path);
                }
            }
        });
    }
}

/// tsgo `isRelevantFileName` without open files and content mappers: a
/// dynamic name or one with a script or JSON extension.
fn is_relevant_file_name(file_name: &str) -> bool {
    if file_name.starts_with("^/") {
        return true;
    }
    let Some(index) = file_name.rfind('.') else {
        return false;
    };
    matches!(
        &file_name[index..],
        ".js" | ".jsx" | ".mjs" | ".cjs" | ".ts" | ".tsx" | ".mts" | ".cts" | ".json"
    )
}

/// The snapshot build's view as a file system for the loader and the
/// config parser (tsgo `sourceFS`): reads go through the cache, writes are
/// refused. A tracking view records what a program build looked for (tsgo
/// `seenFiles` and `missingDirectories`), which decides whether a later
/// file change concerns the program.
pub(crate) struct SourceFs<'a> {
    builder: &'a SnapshotFsBuilder,
    seen: Option<Mutex<SeenFiles>>,
}

/// What a program build looked for.
#[derive(Clone, Debug, Default)]
pub(crate) struct SeenFiles {
    files: BTreeSet<String>,
    missing_directories: BTreeSet<String>,
}

impl SeenFiles {
    /// tsgo `SeenFile`.
    pub(crate) fn seen_file(&self, path: &str) -> bool {
        self.files.contains(path)
    }

    /// tsgo `SeenFileOrMissingParentDirectory`: the file, or a directory
    /// above it that was looked for and missing.
    pub(crate) fn seen_file_or_missing_parent_directory(&self, path: &str) -> bool {
        if self.files.contains(path) {
            return true;
        }
        let mut directory = path;
        while let Some(index) = directory.rfind('/') {
            directory = &directory[..index];
            if self.missing_directories.contains(directory) {
                return true;
            }
        }
        false
    }
}

impl<'a> SourceFs<'a> {
    pub(crate) fn new(builder: &'a SnapshotFsBuilder) -> Self {
        Self {
            builder,
            seen: None,
        }
    }

    pub(crate) fn tracking(builder: &'a SnapshotFsBuilder) -> Self {
        Self {
            builder,
            seen: Some(Mutex::default()),
        }
    }

    pub(crate) fn into_seen(self) -> SeenFiles {
        self.seen
            .map(|seen| seen.into_inner().unwrap_or_else(PoisonError::into_inner))
            .unwrap_or_default()
    }

    fn track(&self, record: impl FnOnce(&mut SeenFiles)) {
        if let Some(seen) = &self.seen {
            record(&mut seen.lock().unwrap_or_else(PoisonError::into_inner));
        }
    }
}

impl FileSystem for SourceFs<'_> {
    fn case_sensitive(&self) -> bool {
        self.builder.base.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.track(|seen| {
            seen.files.insert(self.builder.paths.to_path(path));
        });
        self.builder
            .get(path)
            .map(|file| file.content.to_vec())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        if self.builder.file_exists(path) {
            let len = self.builder.get(path).map_or(0, |file| file.content.len());
            return Ok(Metadata::new(FileType::File, len as u64, UNIX_EPOCH));
        }
        self.builder.base.metadata(path)
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.builder.base.read_dir(path)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.builder.base.canonicalize(path)
    }

    fn write(&self, _path: &str, _contents: &[u8]) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn append(&self, _path: &str, _contents: &[u8]) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn create_dir_all(&self, _path: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn remove(&self, _path: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn set_modified(&self, _path: &str, _modified: SystemTime) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn is_file(&self, path: &str) -> bool {
        self.track(|seen| {
            seen.files.insert(self.builder.paths.to_path(path));
        });
        self.builder.file_exists(path)
    }

    fn is_dir(&self, path: &str) -> bool {
        let exists = self.builder.base.is_dir(path);
        if !exists {
            self.track(|seen| {
                seen.missing_directories
                    .insert(self.builder.paths.to_path(path));
            });
        }
        exists
    }

    fn accessible_entries(&self, path: &str) -> Entries {
        self.builder.base.accessible_entries(path)
    }
}
