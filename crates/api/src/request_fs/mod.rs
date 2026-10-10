//! tsgo `api/requestfilesystem` (19dadef8): the file system a request
//! supplies for its snapshot. A full file system is canonical and total; a
//! layer is checked before the host's. A layer over a request's file system
//! is composed with it at once (tsgo's eager compaction), so a request file
//! system always lies directly over the host's and keeps no history.

mod path_tree;
mod paths;

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, Metadata};
use tsc_project::FileChangeSummary;

use path_tree::{
    compose, merge_entries, path_contains, Entry, Fallback, PathNode, RequestDirectory,
    RequestFile, RequestSymlink,
};
use paths::{
    base_file_name, canonical_file_name, combine_paths, directory_path, has_trailing_separator,
    is_disk_path_root, normalized_absolute_path, remove_trailing_separator, trim_file_path_prefix,
};

/// A field's value, or its zero value when the JSON sets it to `null`.
fn nullable<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// tsgo `RequestDirectoryEntries`: a complete listing.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RequestDirectoryEntries {
    #[serde(default, deserialize_with = "nullable")]
    pub files: Vec<String>,
    #[serde(default, deserialize_with = "nullable")]
    pub directories: Vec<String>,
}

/// tsgo `RequestSymlink`: a link's target, relative to the link's
/// directory, in this file system or the host's.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct RequestSymlinkParams {
    #[serde(default, deserialize_with = "nullable")]
    pub target: String,
    #[serde(default, deserialize_with = "nullable")]
    pub host: bool,
}

/// tsgo `RequestFileSystem`: the request's files, listings, links and
/// removed paths.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFileSystemParams {
    #[serde(default, deserialize_with = "nullable")]
    pub kind: String,
    #[serde(default, deserialize_with = "nullable")]
    pub files: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "nullable")]
    pub directories: BTreeMap<String, RequestDirectoryEntries>,
    #[serde(default, deserialize_with = "nullable")]
    pub symlinks: BTreeMap<String, RequestSymlinkParams>,
    #[serde(default, deserialize_with = "nullable")]
    pub removed_paths: Vec<String>,
}

impl RequestFileSystemParams {
    /// tsgo `KindFull`.
    pub fn is_full(&self) -> bool {
        self.kind == "full"
    }
}

/// tsgo `Kind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Full,
    Layer,
}

/// The file system a snapshot reads: the session's, or a request's over it.
#[derive(Clone)]
pub enum SnapshotFileSystem {
    Host(Arc<dyn FileSystem>),
    Request(Arc<RequestFileSystem>),
}

impl SnapshotFileSystem {
    fn as_file_system(&self) -> &dyn FileSystem {
        match self {
            Self::Host(fs) => fs.as_ref(),
            Self::Request(fs) => fs.as_ref(),
        }
    }
}

/// tsgo `requestFileSystem`.
pub struct RequestFileSystem {
    kind: Kind,
    base: Arc<dyn FileSystem>,
    current_directory: String,
    case_sensitive: bool,
    paths: Arc<PathNode>,
}

/// tsgo `resolvedRequestPath`.
#[derive(Debug, Default)]
struct ResolvedPath {
    path: String,
    followed_symlink: bool,
    host: bool,
    ok: bool,
}

/// What a lookup found locally: a file or a directory (tsgo's `FileInfo`
/// entries; a link has none of its own).
#[derive(Clone, Debug)]
enum Info {
    File(Arc<RequestFile>),
    Directory,
}

/// tsgo `requestPathLookup`.
#[derive(Default)]
struct Lookup {
    path: String,
    info: Option<Info>,
    /// The base file system answers for the path.
    base: bool,
    followed_symlink: bool,
    ok: bool,
}

impl RequestFileSystem {
    /// tsgo `NewForUpdate`: the request's file system for a snapshot whose
    /// file system is `base`. A full file system starts over from the host;
    /// a layer over a request's file system is composed with it; a layer's
    /// files, removals, listings and links are added to `file_changes`.
    pub fn new_for_update(
        params: &RequestFileSystemParams,
        base: &SnapshotFileSystem,
        current_directory: &str,
        file_changes: &mut FileChangeSummary,
    ) -> Result<Arc<RequestFileSystem>, String> {
        let base_file_system = match (params.is_full(), base) {
            (true, SnapshotFileSystem::Request(request)) => {
                SnapshotFileSystem::Host(Arc::clone(&request.base))
            }
            _ => base.clone(),
        };
        let mut file_system = match &base_file_system {
            SnapshotFileSystem::Host(host) => {
                Self::new_worker(params, Arc::clone(host), current_directory)?
            }
            SnapshotFileSystem::Request(request) => {
                Self::new_worker(params, Arc::clone(&request.base), current_directory)?
            }
        };
        if let SnapshotFileSystem::Request(request) = &base_file_system {
            // tsgo `applyTo`.
            file_system.paths = compose(
                Some(&request.paths),
                Some(&file_system.paths),
                Fallback::Allowed,
                file_system.case_sensitive,
            )
            .expect("a composition over a base has a root");
            file_system.kind = request.kind;
        }
        if !params.is_full() {
            add_file_changes(file_changes, params, &base_file_system, current_directory);
        }
        Ok(Arc::new(file_system))
    }

    /// tsgo `newRequestFileSystemWorker`.
    fn new_worker(
        params: &RequestFileSystemParams,
        base: Arc<dyn FileSystem>,
        current_directory: &str,
    ) -> Result<RequestFileSystem, String> {
        let kind = match params.kind.as_str() {
            "full" => Kind::Full,
            "layer" => Kind::Layer,
            other => return Err(format!("unknown request filesystem kind {other:?}")),
        };
        let mut result = RequestFileSystem {
            kind,
            case_sensitive: base.case_sensitive(),
            base,
            current_directory: current_directory.to_owned(),
            paths: Arc::default(),
        };
        let mut paths = PathNode::default();
        result.register_directory(&mut paths, current_directory);
        for (file_name, content) in &params.files {
            let absolute = result.to_absolute_path(file_name);
            let node = paths.ensure(&result.to_path(&absolute));
            if let Some(Entry::File(existing)) = &node.entry {
                return Err(format!(
                    "duplicate request filesystem file path {:?} and {absolute:?}",
                    existing.file_name
                ));
            }
            node.entry = Some(Entry::File(Arc::new(RequestFile {
                file_name: absolute.clone(),
                content: content.clone(),
            })));
            result.register_directory(&mut paths, &directory_path(&absolute));
        }
        let mut seen_directories = BTreeSet::new();
        let mut listed_directories = Vec::new();
        for (directory_name, entries) in &params.directories {
            let absolute = result.to_absolute_path(directory_name);
            let path = result.to_path(&absolute);
            let node = paths.ensure(&path);
            if !seen_directories.insert(path) {
                return Err(format!(
                    "duplicate request filesystem directory path {absolute:?}"
                ));
            }
            if !matches!(node.entry, Some(Entry::File(_))) {
                node.entry = Some(Entry::Directory(Arc::new(RequestDirectory {
                    directory_name: absolute.clone(),
                    listing: Some(Entries {
                        files: entries.files.clone(),
                        directories: entries.directories.clone(),
                        symlinks: BTreeSet::new(),
                    }),
                })));
            }
            result.register_directory(&mut paths, &directory_path(&absolute));
            listed_directories.extend(
                entries
                    .directories
                    .iter()
                    .map(|child| combine_paths(&absolute, child)),
            );
        }
        for link_name in params.symlinks.keys() {
            let absolute = result.to_absolute_path(link_name);
            result.register_directory(&mut paths, &directory_path(&absolute));
        }
        let mut seen_symlinks = BTreeMap::new();
        for (link_name, symlink) in &params.symlinks {
            let absolute_link = result.to_absolute_path(link_name);
            let path = result.to_path(&absolute_link);
            let node = paths.ensure(&path);
            if let Some(existing) = seen_symlinks.insert(path, absolute_link.clone()) {
                return Err(format!(
                    "duplicate request filesystem symlink path {existing:?} and {absolute_link:?}"
                ));
            }
            let target =
                result.to_absolute_path_from(&symlink.target, &directory_path(&absolute_link));
            if node.entry.is_none() {
                node.entry = Some(Entry::Symlink(Arc::new(RequestSymlink {
                    link_name: absolute_link,
                    target,
                    host: symlink.host,
                })));
            }
        }
        for directory_name in &listed_directories {
            result.register_directory(&mut paths, directory_name);
        }
        for path in &params.removed_paths {
            let path = result.to_path(&result.to_absolute_path(path));
            paths.ensure(&path).fallback = Fallback::Missing;
        }
        result.paths = compose(
            None,
            Some(&Arc::new(paths)),
            Fallback::Allowed,
            result.case_sensitive,
        )
        .expect("a composition has a root");
        Ok(result)
    }

    /// tsgo `HasFullFileSystem`.
    pub fn is_full(&self) -> bool {
        self.kind == Kind::Full
    }

    /// The host's file system the request's lies over.
    pub fn base_file_system(&self) -> &Arc<dyn FileSystem> {
        &self.base
    }

    /// tsgo `toAbsolutePath`.
    fn to_absolute_path(&self, path: &str) -> String {
        self.to_absolute_path_from(path, &self.current_directory)
    }

    /// tsgo `toAbsolutePathFrom`: normalized, without a trailing separator
    /// except on a root.
    fn to_absolute_path_from(&self, path: &str, current_directory: &str) -> String {
        let absolute = normalized_absolute_path(path, current_directory);
        if is_disk_path_root(&absolute) {
            absolute
        } else {
            remove_trailing_separator(&absolute).to_owned()
        }
    }

    /// tsgo `toPath`.
    fn to_path(&self, path: &str) -> String {
        paths::to_path(path, &self.current_directory, self.case_sensitive)
    }

    fn equal_entry_names(&self, left: &str, right: &str) -> bool {
        canonical_file_name(left, self.case_sensitive)
            == canonical_file_name(right, self.case_sensitive)
    }

    /// tsgo `registerDirectory`: the directory and its ancestors, up to the
    /// first that has an entry.
    fn register_directory(&self, paths: &mut PathNode, directory_name: &str) {
        let mut directory_name = self.to_absolute_path(directory_name);
        loop {
            let node = paths.ensure(&self.to_path(&directory_name));
            if node.entry.is_some() {
                return;
            }
            node.entry = Some(Entry::Directory(Arc::new(RequestDirectory {
                directory_name: directory_name.clone(),
                listing: None,
            })));
            let parent = directory_path(&directory_name);
            if parent == directory_name {
                return;
            }
            directory_name = parent;
        }
    }

    /// tsgo `blocksFallback`.
    fn blocks_fallback(&self, path: &str) -> bool {
        self.paths.lookup(&self.to_path(path)).1 == Fallback::Missing
    }

    /// tsgo `resolvePath`: the path with the request's links followed,
    /// stopping at a host link; a cycle or a path under a file is missing.
    fn resolve_path(&self, path: &str) -> ResolvedPath {
        let mut result = ResolvedPath {
            path: self.to_absolute_path(path),
            ok: true,
            ..ResolvedPath::default()
        };
        let mut seen = BTreeSet::new();
        loop {
            let canonical = self.to_path(&result.path);
            if self.paths.contains_file_ancestor(&canonical) {
                result.ok = false;
                return result;
            }
            let Some((match_path, symlink)) = self.paths.first_symlink(&canonical) else {
                result.host = self.is_host_path(&result.path);
                return result;
            };
            if !seen.insert(match_path) {
                result.ok = false;
                return result;
            }
            result.followed_symlink = true;
            let Some(suffix) =
                trim_file_path_prefix(&result.path, &symlink.link_name, self.case_sensitive)
            else {
                result.ok = false;
                return result;
            };
            let suffix = suffix.strip_prefix('/').unwrap_or(suffix).to_owned();
            result.path = self.to_absolute_path(&combine_paths(&symlink.target, &suffix));
            if symlink.host {
                result.host = true;
                return result;
            }
        }
    }

    /// tsgo `isHostPath`: under the target of a host link.
    fn is_host_path(&self, path: &str) -> bool {
        let canonical = self.to_path(path);
        let mut found = false;
        self.paths.walk_symlinks(&mut |_, symlink| {
            if symlink.host && path_contains(&self.to_path(&symlink.target), &canonical) {
                found = true;
            }
        });
        found
    }

    /// tsgo `aliasesForPath`: the other names of `path` through the
    /// request's links, transitively.
    fn aliases_for_path(&self, path: &str) -> Vec<String> {
        let mut symlinks = Vec::new();
        self.paths
            .walk_symlinks(&mut |_, symlink| symlinks.push(Arc::clone(symlink)));
        let mut seen = BTreeSet::from([self.to_path(path)]);
        let mut queue = VecDeque::from([self.to_absolute_path(path)]);
        let mut aliases = Vec::new();
        while let Some(candidate) = queue.pop_front() {
            for symlink in &symlinks {
                let Some(suffix) =
                    trim_file_path_prefix(&candidate, &symlink.target, self.case_sensitive)
                else {
                    continue;
                };
                if !suffix.is_empty()
                    && !has_trailing_separator(&symlink.target)
                    && !suffix.starts_with('/')
                {
                    continue;
                }
                let suffix = suffix.strip_prefix('/').unwrap_or(suffix);
                let alias = self.to_absolute_path(&combine_paths(&symlink.link_name, suffix));
                let alias_path = self.to_path(&alias);
                if seen.contains(&alias_path) || !self.resolve_path(&alias).ok {
                    continue;
                }
                seen.insert(alias_path);
                aliases.push(alias.clone());
                queue.push_back(alias);
            }
        }
        aliases
    }

    /// tsgo `localPathInfo`.
    fn local_path_info(&self, path: &str) -> (Option<Info>, Fallback) {
        let (node, fallback) = self.paths.lookup(&self.to_path(path));
        let info = node.and_then(|node| match &node.entry {
            Some(Entry::File(file)) => Some(Info::File(Arc::clone(file))),
            Some(Entry::Directory(_)) => Some(Info::Directory),
            Some(Entry::Symlink(_)) | None => None,
        });
        (info, fallback)
    }

    /// tsgo `lookupPath`: what answers for `path`: a local entry, the base
    /// file system at the resolved path, or nothing.
    fn lookup_path(&self, path: &str) -> Lookup {
        let absolute = self.to_absolute_path(path);
        let (info, fallback) = self.local_path_info(&absolute);
        if info.is_some() {
            return Lookup {
                path: absolute,
                info,
                ok: true,
                ..Lookup::default()
            };
        }
        if fallback == Fallback::Missing {
            return Lookup::default();
        }
        let resolved = self.resolve_path(path);
        if !resolved.ok {
            return Lookup::default();
        }
        let mut result = Lookup {
            followed_symlink: resolved.followed_symlink,
            ok: true,
            ..Lookup::default()
        };
        let (resolved_info, resolved_fallback) = self.local_path_info(&resolved.path);
        result.path = resolved.path;
        if !resolved.host && resolved_info.is_some() {
            result.info = resolved_info;
        } else if resolved.host || self.kind == Kind::Layer {
            if resolved_fallback == Fallback::Missing {
                return Lookup::default();
            }
            result.base = true;
        }
        result
    }

    /// tsgo `mutationPath`: a layer writes through to the base at the
    /// resolved path; a full file system is read-only.
    fn mutation_path(&self, path: &str) -> io::Result<String> {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "invalid argument");
        if self.kind != Kind::Layer {
            return Err(invalid());
        }
        let resolved = self.resolve_path(path);
        if resolved.ok {
            Ok(resolved.path)
        } else {
            Err(invalid())
        }
    }

    /// tsgo `getLocalEntries`: the local listing and whether it is complete.
    fn local_entries(&self, directory_name: &str) -> (Entries, bool) {
        let (node, _) = self.paths.lookup(&self.to_path(directory_name));
        let entries = node.and_then(PathNode::entries).unwrap_or_default();
        let explicit = matches!(
            node.and_then(|node| node.entry.as_ref()),
            Some(Entry::Directory(directory)) if directory.listing.is_some()
        );
        (entries, explicit)
    }

    /// tsgo `removeEntries`: without the names a removal blocks.
    fn remove_entries(&self, directory_name: &str, entries: Entries) -> Entries {
        let blocked = |name: &String| self.blocks_fallback(&combine_paths(directory_name, name));
        let mut result = entries;
        result.files.retain(|name| !blocked(name));
        result.directories.retain(|name| !blocked(name));
        result.symlinks.retain(|name| !blocked(name));
        result
    }

    /// tsgo `filterLocalEntries`: without the names a removal blocks that
    /// have no local entry.
    fn filter_local_entries(&self, directory_name: &str, entries: Entries) -> Entries {
        let keep = |name: &String| {
            let file_name = combine_paths(directory_name, name);
            self.local_path_info(&file_name).0.is_some() || !self.blocks_fallback(&file_name)
        };
        let mut result = entries;
        result.files.retain(keep);
        result.directories.retain(keep);
        result.symlinks.retain(keep);
        result
    }

    /// tsgo `addSymlinkEntries`: the directory's own links, as the files or
    /// directories they resolve to.
    fn add_symlink_entries(&self, directory_name: &str, entries: Entries) -> Entries {
        let (node, _) = self.paths.lookup(&self.to_path(directory_name));
        let links = node
            .map(|node| {
                node.children
                    .values()
                    .filter_map(|child| match &child.entry {
                        Some(Entry::Symlink(symlink)) => Some(Arc::clone(symlink)),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut result = entries;
        if links.is_empty() {
            return result;
        }
        for symlink in links {
            let name = base_file_name(&symlink.link_name);
            result
                .files
                .retain(|value| !self.equal_entry_names(value, &name));
            result
                .directories
                .retain(|value| !self.equal_entry_names(value, &name));
            result
                .symlinks
                .retain(|value| !self.equal_entry_names(value, &name));
            if self.is_dir(&symlink.link_name) {
                result.directories.push(name.clone());
                result.symlinks.insert(name);
            } else if self.is_file(&symlink.link_name) {
                result.files.push(name.clone());
                result.symlinks.insert(name);
            }
        }
        result.files.sort();
        result.directories.sort();
        result
    }

    /// tsgo `ExpandFileChanges`: each changed, created or deleted file under
    /// its other names through the request's links.
    pub fn expand_file_changes(&self, mut summary: FileChangeSummary) -> FileChangeSummary {
        for set in [
            &mut summary.changed,
            &mut summary.created,
            &mut summary.deleted,
        ] {
            let additional = set
                .iter()
                .flat_map(|file_name| self.aliases_for_path(file_name))
                .collect::<Vec<_>>();
            set.extend(additional);
        }
        summary
    }
}

/// tsgo `addFileChanges`: a layer's files are changed or created, its
/// removals deleted, and its listings and links replace what was there
/// (the language server's overlays, which tsgo also settles here, do not
/// reach a standalone session).
fn add_file_changes(
    summary: &mut FileChangeSummary,
    params: &RequestFileSystemParams,
    base: &SnapshotFileSystem,
    current_directory: &str,
) {
    let base_request = match base {
        SnapshotFileSystem::Request(request) => Some(request),
        SnapshotFileSystem::Host(_) => None,
    };
    let base_fs = base.as_file_system();
    let to_path =
        |file_name: &str| paths::to_path(file_name, current_directory, base_fs.case_sensitive());
    let add_change = |summary: &mut FileChangeSummary, file_name: &str, deleted: bool| {
        if deleted {
            if base_fs.is_file(file_name) || base_fs.is_dir(file_name) {
                summary.deleted.insert(file_name.to_owned());
            }
        } else if base_fs.is_file(file_name) {
            summary.changed.insert(file_name.to_owned());
        } else {
            summary.created.insert(file_name.to_owned());
        }
    };
    let add_change_and_aliases =
        |summary: &mut FileChangeSummary, file_name: &str, deleted: bool| {
            add_change(summary, file_name, deleted);
            if let Some(request) = base_request {
                for alias in request.aliases_for_path(file_name) {
                    add_change(summary, &alias, deleted);
                }
            }
        };
    let mut overlay_files = BTreeSet::new();
    for file_name in params.files.keys() {
        let absolute = normalized_absolute_path(file_name, current_directory);
        overlay_files.insert(to_path(&absolute));
        add_change_and_aliases(summary, &absolute, false);
    }
    for removed in &params.removed_paths {
        let absolute = normalized_absolute_path(removed, current_directory);
        if overlay_files.contains(&to_path(&absolute)) {
            continue;
        }
        add_change_and_aliases(summary, &absolute, true);
    }
    // Replacing a listing or a link can change every cached descendant:
    // deleted and created again.
    let replacements = params.directories.keys().chain(params.symlinks.keys());
    for path in replacements {
        let absolute = normalized_absolute_path(path, current_directory);
        add_change_and_aliases(summary, &absolute, true);
        summary.created.insert(absolute.clone());
        if let Some(request) = base_request {
            summary.created.extend(request.aliases_for_path(&absolute));
        }
    }
}

impl FileSystem for RequestFileSystem {
    fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let lookup = self.lookup_path(path);
        if !lookup.ok || matches!(lookup.info, Some(Info::Directory)) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        if lookup.base {
            return self.base.read(&lookup.path);
        }
        match lookup.info {
            Some(Info::File(file)) => Ok(file.content.as_bytes().to_vec()),
            _ => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    /// tsgo `Stat`: a request entry's own metadata, or the base's (or an
    /// existence-only answer) for a path it answers.
    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        let lookup = self.lookup_path(path);
        if !lookup.ok {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        if lookup.base {
            if let Ok(metadata) = self.base.metadata(&lookup.path) {
                return Ok(metadata);
            }
            if self.base.is_dir(&lookup.path) {
                return Ok(Metadata::new(FileType::Directory, 0, UNIX_EPOCH));
            }
            if self.base.is_file(&lookup.path) {
                return Ok(Metadata::new(FileType::File, 0, UNIX_EPOCH));
            }
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        match lookup.info {
            Some(Info::File(file)) => Ok(Metadata::new(
                FileType::File,
                file.content.len() as u64,
                UNIX_EPOCH,
            )),
            Some(Info::Directory) => Ok(Metadata::new(FileType::Directory, 0, UNIX_EPOCH)),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        if !self.is_dir(path) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        let entries = self.accessible_entries(path);
        let kind = |name: &String, file_type| {
            if entries.symlinks.contains(name) {
                FileType::Symlink
            } else {
                file_type
            }
        };
        Ok(entries
            .files
            .iter()
            .map(|name| DirEntry::new(name.clone(), kind(name, FileType::File)))
            .chain(
                entries
                    .directories
                    .iter()
                    .map(|name| DirEntry::new(name.clone(), kind(name, FileType::Directory))),
            )
            .collect())
    }

    /// tsgo `Realpath`.
    fn canonicalize(&self, path: &str) -> io::Result<String> {
        let lookup = self.lookup_path(path);
        if !lookup.ok {
            return Ok(path.to_owned());
        }
        if lookup.base {
            return self.base.canonicalize(&lookup.path);
        }
        if lookup.info.is_some() || !lookup.followed_symlink {
            Ok(lookup.path)
        } else {
            Ok(path.to_owned())
        }
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let path = self.mutation_path(path)?;
        self.base.write(&path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let path = self.mutation_path(path)?;
        self.base.append(&path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        let path = self.mutation_path(path)?;
        self.base.create_dir_all(&path)
    }

    /// tsgo `WriteFile`: the host below writes the file.
    fn write_creating_dirs(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let path = self.mutation_path(path)?;
        self.base.write_creating_dirs(&path, contents)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        let path = self.mutation_path(path)?;
        self.base.remove(&path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        let path = self.mutation_path(path)?;
        self.base.set_modified(&path, modified)
    }

    /// tsgo `FileExists`.
    fn is_file(&self, path: &str) -> bool {
        let lookup = self.lookup_path(path);
        if !lookup.ok || matches!(lookup.info, Some(Info::Directory)) {
            return false;
        }
        lookup.info.is_some() || lookup.base && self.base.is_file(&lookup.path)
    }

    /// tsgo `DirectoryExists`.
    fn is_dir(&self, path: &str) -> bool {
        let lookup = self.lookup_path(path);
        if !lookup.ok || matches!(lookup.info, Some(Info::File(_))) {
            return false;
        }
        lookup.info.is_some() || lookup.base && self.base.is_dir(&lookup.path)
    }

    /// tsgo `GetAccessibleEntries`.
    fn accessible_entries(&self, path: &str) -> Entries {
        let lookup = self.lookup_path(path);
        if !lookup.ok || matches!(lookup.info, Some(Info::File(_))) {
            return Entries::default();
        }
        let result = if lookup.base {
            self.remove_entries(&lookup.path, self.base.accessible_entries(&lookup.path))
        } else {
            let (local, explicit) = self.local_entries(&lookup.path);
            let mut result = local.clone();
            if self.kind == Kind::Layer
                && !explicit
                && !self.blocks_fallback(path)
                && !self.blocks_fallback(&lookup.path)
            {
                result =
                    self.remove_entries(&lookup.path, self.base.accessible_entries(&lookup.path));
                result = merge_entries(&result, &local, &|left, right| {
                    self.equal_entry_names(left, right)
                });
            }
            self.add_symlink_entries(&lookup.path, result)
        };
        self.filter_local_entries(path, result)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/request_fs.rs"]
mod tests;
