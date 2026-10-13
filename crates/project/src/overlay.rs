//! The editor's open files (tsgo `overlayfs.go` and `filechange.go`): each
//! open document's text as the editor has it (an overlay over the file
//! system), and the editor's and the watcher's notifications reduced to the
//! changes a snapshot update takes.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, Metadata};
use tsc_ls::lsconv::{language_kind_to_script_kind, Converters, LspLineMap, Script};
use tsc_lsproto::{
    DocumentUri, LanguageKind, PositionEncodingKind,
    TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::uri::uri_to_file_name;
use tsc_syntax::ScriptKind;
use xxhash_rust::xxh3::xxh3_128;

use crate::fs::{CachedFile, FileChangeSummary, Paths};

/// tsgo `FileChangeKind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileChangeKind {
    Open,
    Close,
    Change,
    Save,
    WatchCreate,
    WatchChange,
    WatchDelete,
}

impl FileChangeKind {
    /// tsgo `IsWatchKind`.
    pub fn is_watch_kind(self) -> bool {
        matches!(
            self,
            Self::WatchCreate | Self::WatchChange | Self::WatchDelete
        )
    }
}

/// tsgo `FileChange`: one notification about a document.
#[derive(Clone, Debug)]
pub struct FileChange {
    pub kind: FileChangeKind,
    pub uri: DocumentUri,
    /// The document's version (open and change).
    pub version: i32,
    /// The document's text (open).
    pub content: String,
    /// The document's language (open).
    pub language_kind: LanguageKind,
    /// The edits (change).
    pub changes: Vec<TextDocumentContentChangePartialOrWholeDocument>,
}

impl FileChange {
    /// A change of `kind` to `uri` with nothing else (close, save and the
    /// watch kinds).
    pub fn new(kind: FileChangeKind, uri: DocumentUri) -> Self {
        Self {
            kind,
            uri,
            version: 0,
            content: String::new(),
            language_kind: LanguageKind::default(),
            changes: Vec::new(),
        }
    }
}

/// tsgo `Overlay`: an open document's text, version and script kind, and
/// whether the text is the file's on disk (false negatives possible, never
/// false positives).
#[derive(Debug)]
pub struct Overlay {
    file_name: String,
    content: Arc<str>,
    hash: u128,
    version: i32,
    kind: ScriptKind,
    matches_disk_text: bool,
    line_map: OnceLock<Arc<LspLineMap>>,
    /// The document as a file the snapshot reads.
    file: OnceLock<Arc<CachedFile>>,
}

impl Overlay {
    /// tsgo `newOverlay`.
    pub fn new(file_name: &str, content: &str, version: i32, kind: ScriptKind) -> Self {
        Self {
            file_name: file_name.to_owned(),
            content: content.into(),
            hash: xxh3_128(content.as_bytes()),
            version,
            kind,
            matches_disk_text: false,
            line_map: OnceLock::new(),
            file: OnceLock::new(),
        }
    }

    /// The same overlay, with `matches_disk_text`.
    fn with_matches_disk_text(&self, matches_disk_text: bool) -> Self {
        Self {
            matches_disk_text,
            ..Self::new(&self.file_name, &self.content, self.version, self.kind)
        }
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    /// tsgo `Hash`: the content's xxh3-128.
    pub fn hash(&self) -> u128 {
        self.hash
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn kind(&self) -> ScriptKind {
        self.kind
    }

    pub fn matches_disk_text(&self) -> bool {
        self.matches_disk_text
    }

    /// tsgo `LSPLineMap`.
    pub fn line_map(&self) -> Arc<LspLineMap> {
        Arc::clone(
            self.line_map
                .get_or_init(|| Arc::new(LspLineMap::new(&self.content))),
        )
    }

    /// The document as a file the snapshot reads.
    pub(crate) fn cached_file(&self) -> Arc<CachedFile> {
        Arc::clone(self.file.get_or_init(|| {
            Arc::new(CachedFile {
                file_name: self.file_name.clone(),
                content: Arc::from(self.content.as_bytes()),
                needs_reload: false,
                realpath_path: None,
            })
        }))
    }

    /// tsgo `computeMatchesDiskText`: whether the file on disk has the
    /// overlay's text, and whether it exists (a dynamic name has no file).
    fn compute_matches_disk_text(&self, fs: &dyn FileSystem) -> (bool, bool) {
        if self.file_name.starts_with("^/") {
            return (false, false);
        }
        match fs.read(&self.file_name) {
            Ok(content) => (xxh3_128(&content) == self.hash, true),
            Err(_) => (false, false),
        }
    }
}

impl Script for Overlay {
    fn file_name(&self) -> &str {
        &self.file_name
    }

    fn text(&self) -> &str {
        &self.content
    }
}

/// The open documents by path.
pub(crate) type Overlays = Arc<BTreeMap<String, Arc<Overlay>>>;

/// tsgo `overlayFS`'s state: the open documents over the host's file
/// system, changed by the editor's and the watcher's notifications.
pub(crate) struct OverlayFs {
    host: Arc<dyn FileSystem>,
    paths: Paths,
    position_encoding: PositionEncodingKind,
    overlays: Overlays,
}

/// tsgo `fileEvents`: one file's notifications in a batch, reduced.
#[derive(Default)]
struct FileEvents {
    open: Option<FileChange>,
    close: bool,
    watch_changed: bool,
    changes: Vec<FileChange>,
    saved: bool,
    created: bool,
    deleted: bool,
}

impl OverlayFs {
    pub(crate) fn new(
        host: Arc<dyn FileSystem>,
        paths: Paths,
        position_encoding: PositionEncodingKind,
    ) -> Self {
        Self {
            host,
            paths,
            position_encoding,
            overlays: Arc::default(),
        }
    }

    pub(crate) fn overlays(&self) -> Overlays {
        Arc::clone(&self.overlays)
    }

    /// tsgo `processChanges`: the batch's notifications applied, each
    /// file's reduced first (a delete after a create cancels it, a create
    /// after a delete is a change, an open replaces what came before), and
    /// the changes a snapshot update takes. Two opens in one batch are a
    /// bug in the caller.
    pub(crate) fn process_changes(
        &mut self,
        changes: Vec<FileChange>,
    ) -> (FileChangeSummary, Overlays) {
        let mut result = FileChangeSummary::default();
        let mut overlays = (*self.overlays).clone();
        let mut events = Vec::<(String, FileEvents)>::new();
        for change in changes {
            let index = match events.iter().position(|(uri, _)| *uri == change.uri.0) {
                Some(index) => {
                    assert!(
                        events[index].1.open.is_none(),
                        "should see no changes after open"
                    );
                    index
                }
                None => {
                    events.push((change.uri.0.clone(), FileEvents::default()));
                    events.len() - 1
                }
            };
            if !result.includes_watch_change_outside_node_modules
                && change.kind.is_watch_kind()
                && !change.uri.0.contains("/node_modules/")
            {
                result.includes_watch_change_outside_node_modules = true;
            }
            let file = &mut events[index].1;
            match change.kind {
                FileChangeKind::Open => {
                    *file = FileEvents {
                        open: Some(change),
                        ..FileEvents::default()
                    };
                }
                FileChangeKind::Close => {
                    file.close = true;
                    file.changes.clear();
                    file.saved = false;
                    file.watch_changed = false;
                }
                FileChangeKind::Change => {
                    assert!(!file.close, "should see no changes after close");
                    file.changes.push(change);
                    file.saved = false;
                    file.watch_changed = false;
                }
                FileChangeKind::Save => file.saved = true,
                FileChangeKind::WatchCreate => {
                    if file.deleted {
                        file.deleted = false;
                        file.watch_changed = true;
                    } else {
                        file.created = true;
                    }
                }
                FileChangeKind::WatchChange => {
                    if !file.created {
                        file.watch_changed = true;
                        file.saved = false;
                    }
                }
                FileChangeKind::WatchDelete => {
                    file.watch_changed = false;
                    file.saved = false;
                    if file.created {
                        file.created = false;
                    } else {
                        file.deleted = true;
                    }
                }
            }
        }
        for (uri, file) in events {
            // A document whose URI does not name a file is left out.
            let Ok(file_name) = uri_to_file_name(&uri) else {
                continue;
            };
            let path = self.paths.to_path(&file_name);
            let mut overlay = overlays.get(&path).cloned();
            if let Some(open) = file.open {
                assert!(
                    result.opened.is_none() && result.reopened.is_none(),
                    "can only process one file open event at a time"
                );
                match &overlay {
                    Some(previous) if *previous.content != open.content => {
                        result.changed.insert(file_name.clone());
                    }
                    None => result.opened = Some(file_name.clone()),
                    Some(_) => result.reopened = Some(file_name.clone()),
                }
                let mut kind = language_kind_to_script_kind(&open.language_kind);
                if kind == ScriptKind::Unknown {
                    kind = ScriptKind::from_file_name(&file_name);
                }
                overlays.insert(
                    path,
                    Arc::new(Overlay::new(&file_name, &open.content, open.version, kind)),
                );
                continue;
            }
            if file.close && overlay.is_some() {
                result.closed.insert(file_name.clone());
                overlays.remove(&path);
                overlay = None;
            }
            if file.watch_changed {
                match &overlay {
                    None => {
                        result.changed.insert(file_name.clone());
                    }
                    Some(current) if !file.saved => {
                        let (matches, _) = current.compute_matches_disk_text(&*self.host);
                        if matches != current.matches_disk_text {
                            let updated = Arc::new(current.with_matches_disk_text(matches));
                            overlays.insert(path.clone(), Arc::clone(&updated));
                            overlay = Some(updated);
                        }
                    }
                    Some(_) => {}
                }
            }
            if let Some(current) = overlay.as_ref().filter(|_| !file.changes.is_empty()) {
                result.changed.insert(file_name.clone());
                let mut current = Arc::clone(current);
                for change in &file.changes {
                    for edit in &change.changes {
                        let content = match edit {
                            TextDocumentContentChangePartialOrWholeDocument::Partial(partial) => {
                                let line_map = current.line_map();
                                let converters =
                                    Converters::new(self.position_encoding.clone(), move |_| {
                                        Arc::clone(&line_map)
                                    });
                                let range = converters.from_lsp_range(&*current, &partial.range);
                                let text = current.content();
                                let end = range.end.max(range.start);
                                format!("{}{}{}", &text[..range.start], partial.text, &text[end..])
                            }
                            TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
                                whole,
                            ) => whole.text.clone(),
                        };
                        current = Arc::new(Overlay::new(
                            &current.file_name,
                            &content,
                            change.version,
                            current.kind,
                        ));
                    }
                    if !change.changes.is_empty() {
                        overlays.insert(path.clone(), Arc::clone(&current));
                    }
                }
                overlay = Some(current);
            }
            if file.saved {
                match &overlay {
                    Some(current) => {
                        let saved = Arc::new(current.with_matches_disk_text(true));
                        overlays.insert(path.clone(), Arc::clone(&saved));
                        overlay = Some(saved);
                    }
                    // A file saved but never opened is a change on disk.
                    None if !file.watch_changed => {
                        result.changed.insert(file_name.clone());
                    }
                    None => {}
                }
            }
            if file.created && overlay.is_none() {
                result.created.insert(file_name.clone());
            }
            if file.deleted && overlay.is_none() {
                result.deleted.insert(file_name);
            }
        }
        self.overlays = Arc::new(overlays);
        (result, self.overlays())
    }
}

/// tsgo `overlayFS` as a file system: the open documents over the host's
/// file system, each directory above a document a directory (even where
/// the host has a file).
pub(crate) struct OverlayLayer {
    host: Arc<dyn FileSystem>,
    paths: Paths,
    overlays: Overlays,
    /// tsgo `overlayDirectories`: each directory above a document, with its
    /// children on the way (by path, with their names).
    directories: BTreeMap<String, BTreeMap<String, String>>,
}

impl OverlayLayer {
    pub(crate) fn new(host: Arc<dyn FileSystem>, paths: Paths, overlays: Overlays) -> Self {
        let mut directories = BTreeMap::<String, BTreeMap<String, String>>::new();
        for (path, overlay) in overlays.iter() {
            let mut child_path = path.clone();
            let mut child = overlay.file_name.clone();
            loop {
                let parent_path = parent(&child_path);
                if parent_path == child_path {
                    break;
                }
                let name = child[parent(&child).len()..]
                    .trim_start_matches('/')
                    .to_owned();
                directories
                    .entry(parent_path.clone())
                    .or_default()
                    .insert(child_path.clone(), name);
                child = parent(&child);
                child_path = parent_path;
            }
        }
        Self {
            host,
            paths,
            overlays,
            directories,
        }
    }

    /// The overlay at `file_name`.
    pub(crate) fn overlay(&self, file_name: &str) -> Option<&Arc<Overlay>> {
        self.overlays.get(&self.paths.to_path(file_name))
    }

    fn is_overlay_directory(&self, file_name: &str) -> bool {
        self.directories
            .contains_key(&self.paths.to_path(file_name))
    }
}

/// tsgo `PathKey.Parent`: the path above `path` (a root is its own).
fn parent(path: &str) -> String {
    let root = tsc_program::path_root_parts(tsc_diagnostics::JsStr::from_str(path))
        .map_or(0, |(root, _)| root.as_bytes().len());
    match path[root..].rfind('/') {
        Some(index) => path[..root + index].to_owned(),
        None => path[..root].to_owned(),
    }
}

impl FileSystem for OverlayLayer {
    fn case_sensitive(&self) -> bool {
        self.host.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        if let Some(overlay) = self.overlay(path) {
            return Ok(overlay.content.as_bytes().to_vec());
        }
        if self.is_overlay_directory(path) {
            return Err(io::Error::new(io::ErrorKind::IsADirectory, "a directory"));
        }
        self.host.read(path)
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        if let Some(overlay) = self.overlay(path) {
            // tsgo `overlayFileInfo`: no modification time.
            return Ok(Metadata::new(
                FileType::File,
                overlay.content.len() as u64,
                None,
            ));
        }
        if self.is_overlay_directory(path) {
            return Ok(Metadata::new(FileType::Directory, 0, None));
        }
        self.host.metadata(path)
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let path_key = self.paths.to_path(path);
        if self.overlays.contains_key(&path_key) {
            return Err(io::Error::new(io::ErrorKind::NotADirectory, "a file"));
        }
        let Some(children) = self.directories.get(&path_key) else {
            return self.host.read_dir(path);
        };
        let mut entries = self.host.read_dir(path).unwrap_or_default();
        let fold = |name: &str| self.paths.to_path(name);
        for (child_path, name) in children {
            entries.retain(|entry| fold(entry.name()) != fold(name));
            let file_type = if self.overlays.contains_key(child_path) {
                FileType::File
            } else {
                FileType::Directory
            };
            entries.push(DirEntry::new(name.clone(), file_type));
        }
        entries.sort_by(|left, right| left.name().cmp(right.name()));
        Ok(entries)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.host.canonicalize(path)
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.host.write(path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.host.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.host.create_dir_all(path)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.host.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.host.set_modified(path, modified)
    }

    fn accessible_entries(&self, path: &str) -> Entries {
        let path_key = self.paths.to_path(path);
        if self.overlays.contains_key(&path_key) {
            return Entries::default();
        }
        let mut entries = self.host.accessible_entries(path);
        let Some(children) = self.directories.get(&path_key) else {
            return entries;
        };
        let same = |left: &str, right: &str| self.paths.to_path(left) == self.paths.to_path(right);
        for (child_path, name) in children {
            entries.files.retain(|entry| !same(entry, name));
            entries.directories.retain(|entry| !same(entry, name));
            if self.overlays.contains_key(child_path) {
                entries.files.push(name.clone());
            } else {
                entries.directories.push(name.clone());
            }
        }
        entries
    }
}

#[cfg(test)]
#[path = "../tests/unit/overlay.rs"]
mod tests;
