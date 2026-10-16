//! The files a snapshot has read (tsgo `SnapshotFS`, snapshotfs.go): the
//! base file system (the host's, or one an API request supplies) and the
//! contents read through it, kept until a file change says otherwise.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::SystemTime;

use tsc_diagnostics::JsStr;
use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, Metadata};
use tsc_ls::lsconv::LspLineMap;

use crate::overlay::Overlays;

/// tsgo `excessiveChangeThreshold`.
const EXCESSIVE_CHANGE_THRESHOLD: usize = 1000;

/// File changes a snapshot update reports (tsgo `FileChangeSummary`), by
/// absolute file name. `invalidate_all` drops every assumption about the
/// files read so far.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileChangeSummary {
    /// The file the editor opened (one at a time).
    pub opened: Option<String>,
    /// The file the editor closed and opened again in one batch, unchanged.
    pub reopened: Option<String>,
    pub closed: BTreeSet<String>,
    pub changed: BTreeSet<String>,
    pub created: BTreeSet<String>,
    pub deleted: BTreeSet<String>,
    /// tsgo `IncludesWatchChangeOutsideNodeModules`: a watch event names a
    /// file outside `node_modules`.
    pub includes_watch_change_outside_node_modules: bool,
    pub invalidate_all: bool,
}

impl FileChangeSummary {
    pub fn is_empty(&self) -> bool {
        !self.invalidate_all
            && self.opened.is_none()
            && self.reopened.is_none()
            && self.closed.is_empty()
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

    /// tsgo `tspath.ToPath` (`CaseSensitivity.PathKey`): an encoded dynamic
    /// URI name keeps its case, its root ending with a separator.
    pub(crate) fn to_path(&self, file_name: &str) -> String {
        let absolute = self.absolute(file_name);
        if tsc_program::is_encoded_dynamic_file_name(&absolute) {
            tsc_program::canonical_dynamic_uri_path(absolute)
        } else if self.case_sensitive {
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
    /// tsgo `realpathPath`: the path of the file's real name, for a file in
    /// `node_modules` read through a link.
    pub(crate) realpath_path: Option<String>,
    /// tsgo `fileBase.lineMap`: the protocol line starts of the decoded
    /// text, made once.
    pub(crate) line_map: OnceLock<Arc<LspLineMap>>,
}

impl CachedFile {
    /// The same file with `content`, read again.
    fn reloaded(&self, content: Arc<[u8]>) -> Self {
        Self {
            file_name: self.file_name.clone(),
            content,
            needs_reload: false,
            realpath_path: self.realpath_path.clone(),
            line_map: OnceLock::new(),
        }
    }

    /// tsgo `LSPLineMap`: the line starts of the file's text.
    pub(crate) fn line_map(&self) -> Arc<LspLineMap> {
        Arc::clone(self.line_map.get_or_init(|| {
            let text = tsc_program::decode_host_text(self.content.to_vec())
                .unwrap_or_else(|_| String::from_utf8_lossy(&self.content).into_owned());
            Arc::new(LspLineMap::new(&text))
        }))
    }
}

type Files = Arc<BTreeMap<String, Arc<CachedFile>>>;

/// tsgo `nodeModulesRealpathAliases`: the path of a real file, to the paths
/// of `node_modules` links it was read through.
pub(crate) type Aliases = Arc<BTreeMap<String, BTreeSet<String>>>;

/// A snapshot's view of the files (tsgo `SnapshotFS`).
#[derive(Clone)]
pub(crate) struct SnapshotFs {
    /// The file system the snapshot reads, the open documents over it.
    pub(crate) base: Arc<dyn FileSystem>,
    pub(crate) files: Files,
    pub(crate) aliases: Aliases,
    /// The open documents by path (tsgo `snapshotOverlays`).
    pub(crate) overlays: Overlays,
    paths: Paths,
    /// tsgo `readFiles`: the files the snapshot did not cache, read once.
    reads: Arc<Mutex<BTreeMap<String, Option<Arc<CachedFile>>>>>,
}

impl SnapshotFs {
    pub(crate) fn new(base: Arc<dyn FileSystem>, paths: Paths) -> Self {
        Self {
            base,
            files: Arc::default(),
            aliases: Arc::default(),
            overlays: Arc::default(),
            paths,
            reads: Arc::default(),
        }
    }

    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }

    /// tsgo `SnapshotFS.GetFile`: the open document, the file as the
    /// snapshot read it, or as its file system holds it, read once per
    /// snapshot.
    pub(crate) fn file(&self, file_name: &str) -> Option<Arc<CachedFile>> {
        let path = self.paths.to_path(file_name);
        if let Some(overlay) = self.overlays.get(&path) {
            return Some(overlay.cached_file());
        }
        if let Some(file) = self.files.get(&path) {
            return Some(Arc::clone(file));
        }
        self.reads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(path)
            .or_insert_with(|| {
                self.base.read(file_name).ok().map(|content| {
                    Arc::new(CachedFile {
                        file_name: file_name.to_owned(),
                        content: content.into(),
                        needs_reload: false,
                        realpath_path: None,
                        line_map: OnceLock::new(),
                    })
                })
            })
            .clone()
    }

    pub(crate) fn read(&self, file_name: &str) -> Option<Vec<u8>> {
        self.file(file_name).map(|file| file.content.to_vec())
    }

    /// tsgo `expandRealpathAliases`: a change or deletion of a real file
    /// also names the `node_modules` links the snapshot read it through.
    pub(crate) fn expand_realpath_aliases(
        &self,
        mut changes: FileChangeSummary,
    ) -> FileChangeSummary {
        if self.aliases.is_empty() {
            return changes;
        }
        for names in [&mut changes.changed, &mut changes.deleted] {
            let links = names
                .iter()
                .filter_map(|file_name| self.aliases.get(&self.paths.to_path(file_name)))
                .flatten()
                .cloned()
                .collect::<Vec<_>>();
            names.extend(links);
        }
        changes
    }
}

/// The view a snapshot build reads through (tsgo `snapshotFSBuilder`): the
/// base snapshot's files over the new base file system, read again or
/// dropped as the build's file changes say.
pub(crate) struct SnapshotFsBuilder {
    /// The file system the build reads, the open documents over it.
    base: Arc<dyn FileSystem>,
    paths: Paths,
    files: Mutex<Files>,
    aliases: Mutex<Aliases>,
    overlays: Overlays,
}

impl SnapshotFsBuilder {
    pub(crate) fn new(
        base: Arc<dyn FileSystem>,
        files: Files,
        aliases: Aliases,
        paths: Paths,
        overlays: Overlays,
    ) -> Self {
        Self {
            base,
            paths,
            files: Mutex::new(files),
            aliases: Mutex::new(aliases),
            overlays,
        }
    }

    /// The open documents by path.
    pub(crate) fn overlays(&self) -> &Overlays {
        &self.overlays
    }

    /// tsgo `isOpenFile`: whether the editor has the file at `path` open.
    pub(crate) fn is_open_file(&self, path: &str) -> bool {
        self.overlays.contains_key(path)
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
            aliases: self
                .aliases
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner),
            overlays: self.overlays,
            paths: self.paths,
            reads: Arc::default(),
        }
    }

    fn with_files<T>(&self, operation: impl FnOnce(&mut Files) -> T) -> T {
        operation(&mut self.files.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// tsgo `recordRealpathAlias`: a file in `node_modules` first read
    /// through a link is recorded under its real path.
    fn realpath_alias(&self, file_name: &str, path: &str) -> Option<String> {
        if !path.contains("/node_modules/") {
            return None;
        }
        let real = self.base.canonicalize(file_name).ok()?;
        let real_path = self.paths.to_path(&real);
        if real_path == path {
            return None;
        }
        let mut aliases = self.aliases.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::make_mut(&mut aliases)
            .entry(real_path.clone())
            .or_default()
            .insert(path.to_owned());
        Some(real_path)
    }

    /// Drop a cached file (tsgo deletes the entry). Its link stops being an
    /// alias of its real path (tsgo `Finalize` prunes the aliases of deleted
    /// entries), unless `source_backed` and the file is still there (tsgo
    /// `deleteCacheEntry`'s source-backed replacements).
    fn remove_entry(&self, files: &mut Files, path: &str, source_backed: bool) {
        let Some(entry) = Arc::make_mut(files).remove(path) else {
            return;
        };
        let Some(real_path) = &entry.realpath_path else {
            return;
        };
        if source_backed && self.base.is_file(&entry.file_name) {
            return;
        }
        let mut aliases = self.aliases.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(links) = aliases.get(real_path) {
            if links.contains(path) {
                let aliases = Arc::make_mut(&mut aliases);
                let links = aliases.get_mut(real_path).expect("present above");
                links.remove(path);
                if links.is_empty() {
                    aliases.remove(real_path);
                }
            }
        }
    }

    /// tsgo `Clone`'s file cache clean-up: drop the cached files `keep`
    /// does not keep.
    pub(crate) fn retain_files(&self, keep: impl Fn(&str) -> bool) {
        self.with_files(|files| {
            let dropped = files
                .keys()
                .filter(|path| !keep(path))
                .cloned()
                .collect::<Vec<_>>();
            for path in dropped {
                self.remove_entry(files, &path, false);
            }
        });
    }

    /// tsgo `GetAccessibleEntries`: the base's entries, with the files and
    /// directories the snapshot cached there first (a cached file is in
    /// the snapshot's view whatever the disk now holds).
    pub(crate) fn accessible_entries(&self, directory: &str) -> Entries {
        let lower = self.base.accessible_entries(directory);
        let prefix = format!("{}/", self.paths.to_path(directory).trim_end_matches('/'));
        let mut entries = Entries {
            symlinks: lower.symlinks.clone(),
            ..Entries::default()
        };
        let mut cached = BTreeSet::new();
        self.with_files(|files| {
            for (path, file) in files
                .range(prefix.clone()..)
                .take_while(|(path, _)| path.starts_with(&prefix))
            {
                let rest = &path[prefix.len()..];
                let (child, is_file) = match rest.find('/') {
                    Some(end) => (&rest[..end], false),
                    None => (rest, true),
                };
                if !cached.insert(child.to_owned()) {
                    continue;
                }
                // The name as the file was read (the path folds case).
                let name = file
                    .file_name
                    .rsplit('/')
                    .nth(rest.matches('/').count())
                    .unwrap_or(child)
                    .to_owned();
                entries.symlinks.retain(|link| !self.same_name(link, &name));
                if is_file {
                    entries.files.push(name);
                } else {
                    entries.directories.push(name);
                }
            }
        });
        let listed = |entries: &Entries, name: &str| {
            entries
                .files
                .iter()
                .chain(&entries.directories)
                .any(|listed| self.same_name(listed, name))
        };
        for name in lower.files {
            if !listed(&entries, &name) {
                entries.files.push(name);
            }
        }
        for name in lower.directories {
            if !listed(&entries, &name) {
                entries.directories.push(name);
            }
        }
        entries
    }

    fn same_name(&self, left: &str, right: &str) -> bool {
        if self.paths.case_sensitive {
            left == right
        } else {
            tsc_host::to_file_name_lower_case(left) == tsc_host::to_file_name_lower_case(right)
        }
    }

    /// tsgo `GetFileByPath`: the cached file (read again when marked, dropped
    /// when gone), or the open document (never cached), or the file read
    /// from the base and cached.
    pub(crate) fn get(&self, file_name: &str) -> Option<Arc<CachedFile>> {
        let path = self.paths.to_path(file_name);
        if let Some(overlay) = self.overlays.get(&path) {
            if !self.with_files(|files| files.contains_key(&path)) {
                return Some(overlay.cached_file());
            }
        }
        self.with_files(|files| {
            let cached = files.get(&path).cloned();
            let read_name = match &cached {
                Some(entry) if !entry.needs_reload => return Some(Arc::clone(entry)),
                Some(entry) => entry.file_name.clone(),
                None => file_name.to_owned(),
            };
            match self.base.read(&read_name) {
                Ok(content) => {
                    let entry = Arc::new(match &cached {
                        Some(entry) => entry.reloaded(content.into()),
                        None => CachedFile {
                            realpath_path: self.realpath_alias(&read_name, &path),
                            line_map: OnceLock::new(),
                            file_name: read_name,
                            content: content.into(),
                            needs_reload: false,
                        },
                    });
                    Arc::make_mut(files).insert(path, Arc::clone(&entry));
                    Some(entry)
                }
                Err(_) => {
                    self.remove_entry(files, &path, false);
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
        if self.is_open_file(&self.paths.to_path(file_name)) {
            return true;
        }
        let cached = self.with_files(|files| files.contains_key(&self.paths.to_path(file_name)));
        if cached {
            self.get(file_name).is_some()
        } else {
            self.base.is_file(file_name)
        }
    }

    /// tsgo `processFileChanges` (snapshot.go:134-191): the changes a build
    /// acts on, the cache updated for them; `base` is the snapshot the build
    /// starts from (its open documents are the previous ones).
    pub(crate) fn process_file_changes(
        &self,
        mut changes: FileChangeSummary,
        base: &SnapshotFs,
        content_mapper_extensions: &[String],
        content_mapper_watched_files: &BTreeSet<String>,
    ) -> FileChangeSummary {
        let previous = &base.overlays;
        if changes.has_excessive_watch_events() {
            if changes.invalidate_all {
                self.invalidate_cache();
            } else if !self.changes_overlap_cache(&changes, previous) {
                // Every change names a file nothing read (an external
                // tool's build, say).
                changes.changed.clear();
                changes.deleted.clear();
            } else if changes.includes_watch_change_outside_node_modules {
                self.invalidate_cache();
            } else {
                // An npm install.
                self.invalidate_node_modules_cache();
            }
        } else {
            self.expand_and_filter(
                &mut changes,
                content_mapper_extensions,
                content_mapper_watched_files,
                previous,
            );
            changes = base.expand_realpath_aliases(changes);
            self.mark_dirty_files(&mut changes);
            self.convert_open_and_close_to_changes(&mut changes, previous);
        }
        // The open documents are read from the editor, never the cache.
        self.with_files(|files| {
            for path in self.overlays.keys() {
                if files.contains_key(path) {
                    self.remove_entry(files, path, true);
                }
            }
        });
        changes
    }

    /// tsgo `watchChangesOverlapCache`: a change or deletion names a file
    /// open before or now, cached, or read through a link.
    fn changes_overlap_cache(&self, changes: &FileChangeSummary, previous: &Overlays) -> bool {
        let aliases = Arc::clone(&self.aliases.lock().unwrap_or_else(PoisonError::into_inner));
        self.with_files(|files| {
            changes
                .changed
                .iter()
                .chain(&changes.deleted)
                .any(|file_name| {
                    let path = self.paths.to_path(file_name);
                    previous.contains_key(&path)
                        || self.overlays.contains_key(&path)
                        || files.contains_key(&path)
                        || aliases.contains_key(&path)
                })
        })
    }

    /// tsgo `invalidateCache`: every cached file is read again before use.
    fn invalidate_cache(&self) {
        self.mark_for_reload(|_| true);
    }

    /// tsgo `invalidateNodeModulesCache`: every cached file in a
    /// `node_modules` directory is read again before use.
    fn invalidate_node_modules_cache(&self) {
        self.mark_for_reload(|path| path.contains("/node_modules/"));
    }

    fn mark_for_reload(&self, marked: impl Fn(&str) -> bool) {
        self.with_files(|files| {
            let reloaded = files
                .iter()
                .map(|(path, entry)| {
                    let entry = if marked(path) {
                        Arc::new(CachedFile {
                            needs_reload: true,
                            ..entry.reloaded(Arc::clone(&entry.content))
                        })
                    } else {
                        Arc::clone(entry)
                    };
                    (path.clone(), entry)
                })
                .collect();
            *files = Arc::new(reloaded);
        });
    }

    /// tsgo `expandAndFilterWatchEvents`: a deleted directory stands for the
    /// cached and open files below it; other deletions and changes count
    /// only for files that can affect a program (an open one, or anything in
    /// node_modules for a deletion). Creations are kept: one may be a
    /// directory link.
    fn expand_and_filter(
        &self,
        changes: &mut FileChangeSummary,
        content_mapper_extensions: &[String],
        content_mapper_watched_files: &BTreeSet<String>,
        previous: &Overlays,
    ) {
        let relevant = |file_name: &str| {
            let path = self.paths.to_path(file_name);
            self.overlays.contains_key(&path)
                || is_relevant_file_name(
                    file_name,
                    &path,
                    content_mapper_extensions,
                    content_mapper_watched_files,
                )
        };
        if !changes.deleted.is_empty() {
            let mut deleted = BTreeSet::new();
            self.with_files(|files| {
                for file_name in &changes.deleted {
                    let path = self.paths.to_path(file_name);
                    let prefix = format!("{}/", path.trim_end_matches('/'));
                    let mut below = files
                        .range(prefix.clone()..)
                        .take_while(|(cached, _)| cached.starts_with(&prefix))
                        .map(|(_, entry)| entry.file_name.clone())
                        .collect::<Vec<_>>();
                    for overlays in [&self.overlays, previous] {
                        below.extend(
                            overlays
                                .range(prefix.clone()..)
                                .take_while(|(open, _)| open.starts_with(&prefix))
                                .map(|(_, overlay)| overlay.file_name().to_owned()),
                        );
                    }
                    if !below.is_empty() {
                        deleted.extend(below);
                    } else if relevant(file_name)
                        || path.ends_with("/node_modules")
                        || path.contains("/node_modules/")
                    {
                        deleted.insert(file_name.clone());
                    }
                }
            });
            changes.deleted = deleted;
        }
        changes.changed.retain(|file_name| relevant(file_name));
    }

    /// tsgo `markDirtyFiles`: a changed file that is cached is read again
    /// and no longer counts as changed when its text is the same (or when
    /// it is gone, its entry is dropped); an open document stays changed; a
    /// deleted file's entry is dropped.
    fn mark_dirty_files(&self, changes: &mut FileChangeSummary) {
        let base = &self.base;
        self.with_files(|files| {
            changes.changed.retain(|file_name| {
                let path = self.paths.to_path(file_name);
                if self.overlays.contains_key(&path) {
                    return true;
                }
                let Some(entry) = files.get(&path) else {
                    return true;
                };
                match base.read(&entry.file_name) {
                    Ok(content) if *content == *entry.content => {
                        if entry.needs_reload {
                            let entry = Arc::new(entry.reloaded(Arc::clone(&entry.content)));
                            Arc::make_mut(files).insert(path, entry);
                        }
                        false
                    }
                    Ok(content) => {
                        let entry = Arc::new(entry.reloaded(content.into()));
                        Arc::make_mut(files).insert(path, entry);
                        true
                    }
                    Err(_) => {
                        self.remove_entry(files, &path, false);
                        true
                    }
                }
            });
            for file_name in &changes.deleted {
                let path = self.paths.to_path(file_name);
                self.remove_entry(files, &path, true);
            }
        });
    }

    /// tsgo `convertOpenAndCloseToChanges`: an opened file nothing read is
    /// a created one, and one read with other text a changed one; a closed
    /// file is read from disk again (watch events may have been ignored
    /// while it was open) and is changed when that text is not the
    /// editor's, or deleted when it is gone.
    fn convert_open_and_close_to_changes(
        &self,
        changes: &mut FileChangeSummary,
        previous: &Overlays,
    ) {
        if let Some(opened) = changes
            .opened
            .clone()
            .filter(|name| !name.starts_with("^/"))
        {
            let path = self.paths.to_path(&opened);
            let cached = self.with_files(|files| files.get(&path).cloned());
            match (cached, self.overlays.get(&path)) {
                (None, _) => {
                    changes.created.insert(opened);
                }
                (Some(cached), Some(overlay)) => {
                    if *cached.content != *overlay.content().as_bytes() {
                        changes.changed.insert(opened);
                    }
                    self.with_files(|files| self.remove_entry(files, &path, true));
                }
                (Some(_), None) => {}
            }
        }
        for closed in changes.closed.clone() {
            if closed.starts_with("^/") {
                continue;
            }
            let path = self.paths.to_path(&closed);
            match self.reload(&closed, &path) {
                Some(file) => {
                    if previous
                        .get(&path)
                        .is_some_and(|overlay| *file.content != *overlay.content().as_bytes())
                    {
                        changes.changed.insert(closed);
                    }
                }
                None => {
                    changes.deleted.insert(closed);
                }
            }
        }
    }

    /// tsgo `getCachedFile` with `forceReload`: the file read from the base
    /// and cached, or none (and no entry) when it is gone.
    fn reload(&self, file_name: &str, path: &str) -> Option<Arc<CachedFile>> {
        self.with_files(|files| match self.base.read(file_name) {
            Ok(content) => {
                let entry = Arc::new(match files.get(path) {
                    Some(entry) => entry.reloaded(content.into()),
                    None => CachedFile {
                        realpath_path: self.realpath_alias(file_name, path),
                        line_map: OnceLock::new(),
                        file_name: file_name.to_owned(),
                        content: content.into(),
                        needs_reload: false,
                    },
                });
                Arc::make_mut(files).insert(path.to_owned(), Arc::clone(&entry));
                Some(entry)
            }
            Err(_) => {
                self.remove_entry(files, path, false);
                None
            }
        })
    }
}

/// tsgo `isRelevantFileName` (an open file is relevant too): a file a
/// content mapper watches (by `path`) or one of a content mapper's
/// extension, a dynamic name, or one with a script or JSON extension.
fn is_relevant_file_name(
    file_name: &str,
    path: &str,
    content_mapper_extensions: &[String],
    content_mapper_watched_files: &BTreeSet<String>,
) -> bool {
    if content_mapper_watched_files.contains(path)
        || content_mapper_extensions.iter().any(|extension| {
            file_name.len() > extension.len() && file_name.ends_with(extension.as_str())
        })
        || file_name.starts_with("^/")
    {
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

/// A snapshot's files as a file system (tsgo's snapshot as a resolver's
/// host): a file reads as the snapshot read it, or once from its file
/// system; anything else asks the file system.
pub(crate) struct SnapshotView {
    pub(crate) fs: SnapshotFs,
}

impl FileSystem for SnapshotView {
    fn case_sensitive(&self) -> bool {
        self.fs.base.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.fs
            .read(path)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        match self.fs.files.get(&self.fs.paths.to_path(path)) {
            // tsgo `overlayFileInfo.ModTime`: zero.
            Some(file) => Ok(Metadata::new(
                FileType::File,
                file.content.len() as u64,
                None,
            )),
            None => self.fs.base.metadata(path),
        }
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.fs.base.read_dir(path)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.fs.base.canonicalize(path)
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

    /// tsgo `SnapshotFS.FileExists`: a file the snapshot holds, or one its
    /// file system has.
    fn is_file(&self, path: &str) -> bool {
        self.fs.files.contains_key(&self.fs.paths.to_path(path)) || self.fs.base.is_file(path)
    }

    fn is_dir(&self, path: &str) -> bool {
        self.fs.base.is_dir(path)
    }

    fn accessible_entries(&self, path: &str) -> Entries {
        self.fs.base.accessible_entries(path)
    }
}

/// What a program build looked for.
#[derive(Clone, Debug, Default)]
pub(crate) struct SeenFiles {
    /// The files the build looked at: path to file name (tsgo
    /// `seenFiles`).
    files: BTreeMap<String, String>,
    missing_directories: BTreeSet<String>,
}

impl SeenFiles {
    /// tsgo `SeenFile`.
    pub(crate) fn seen_file(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    /// The files the build looked at, path to file name.
    pub(crate) fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }

    /// tsgo `SeenFileOrMissingParentDirectory`: the file, or the path or a
    /// directory above it that was looked for and missing.
    pub(crate) fn seen_file_or_missing_parent_directory(&self, path: &str) -> bool {
        if self.files.contains_key(path) {
            return true;
        }
        let mut directory = path;
        loop {
            if self.missing_directories.contains(directory) {
                return true;
            }
            let Some(index) = directory.rfind('/') else {
                return false;
            };
            directory = &directory[..index];
        }
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

    /// tsgo `Track`: a file the program read without this file system.
    pub(crate) fn track_file(&self, path: String, file_name: String) {
        self.track(|seen| {
            seen.files.insert(path, file_name);
        });
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

    /// A build's reads are the snapshot's files (read once from the file
    /// system, under its lock) and what it records is a set, so tsgo's
    /// parallel program loader reads it as the loading thread would.
    fn permits_parallel_reads(&self) -> bool {
        true
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.track(|seen| {
            seen.files.insert(
                self.builder.paths.to_path(path),
                self.builder.paths.absolute(path),
            );
        });
        self.builder
            .get(path)
            .map(|file| file.content.to_vec())
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        if self.builder.file_exists(path) {
            let len = self.builder.get(path).map_or(0, |file| file.content.len());
            return Ok(Metadata::new(FileType::File, len as u64, None));
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
            seen.files.insert(
                self.builder.paths.to_path(path),
                self.builder.paths.absolute(path),
            );
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
        self.builder.accessible_entries(path)
    }
}

#[cfg(test)]
#[path = "../tests/unit/fs.rs"]
mod tests;
