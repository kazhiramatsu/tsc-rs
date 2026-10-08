//! The in-memory file system.

use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::SystemTime;

use super::{components, join, normalize, parent, Clock, DirEntry, FileSystem, FileType, Metadata};
use crate::to_file_name_lower_case;

/// An in-memory [`FileSystem`]: Go's `testing/fstest.MapFS` with the layer
/// TypeScript's native test harness puts over it (`internal/vfs/vfstest`):
/// names that fold case, symbolic links, writes and removals, and
/// modification times read from a [`Clock`].
///
/// Each directory keys its entries by their canonical name (folded on a
/// case-insensitive file system), and each entry remembers the path it was
/// created or last written at: a lookup in any casing finds it, and
/// [`FileSystem::canonicalize`] answers with that spelling, as tsgo's map
/// file system does. A write through a link keeps the spelling it was
/// given.
///
/// As in tsgo, [`FileSystem::remove`], [`FileSystem::set_modified`],
/// [`MemFs::entry`] and [`MemFs::read_link`] name an entry without
/// following links; reads, writes and [`FileSystem::create_dir_all`]
/// follow them.
pub struct MemFs {
    case_sensitive: bool,
    clock: Arc<dyn Clock>,
    root: RwLock<Node>,
}

/// An entry to create with [`MemFs::from_entries`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Seed {
    File(Arc<[u8]>),
    /// A symbolic link to the absolute path given.
    Symlink(String),
}

impl Seed {
    pub fn file(contents: impl AsRef<[u8]>) -> Self {
        Self::File(Arc::from(contents.as_ref()))
    }

    pub fn symlink(target: impl Into<String>) -> Self {
        Self::Symlink(target.into())
    }
}

/// An entry as [`MemFs::entry`] and [`MemFs::entries`] report it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    path: String,
    modified: SystemTime,
    contents: EntryContents,
}

/// What an [`Entry`] holds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryContents {
    File(Arc<[u8]>),
    Directory,
    /// A link to this absolute path.
    Symlink(String),
}

impl Entry {
    /// The path the entry was created or last written at.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub const fn modified(&self) -> SystemTime {
        self.modified
    }

    pub const fn contents(&self) -> &EntryContents {
        &self.contents
    }
}

#[derive(Clone, Debug)]
struct Node {
    /// The path the node was created or last written at.
    path: Arc<str>,
    modified: SystemTime,
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    File(Arc<[u8]>),
    /// The entries by canonical name.
    Directory(BTreeMap<Box<str>, Node>),
    Symlink(Arc<str>),
}

/// A node's place in the tree: the canonical names from the root.
type Route = Vec<Box<str>>;

/// Where a walk along a path ended.
enum Lookup {
    /// At the node of this route.
    Found(Route),
    /// At a missing name: the walk stood in the directory at `parent`,
    /// `rest` are the names left, the missing one first, and `link` is the
    /// last link followed on the way.
    Missing {
        parent: Route,
        rest: Vec<String>,
        link: Option<Link>,
    },
}

/// A followed link, for the error of a walk that ends behind it.
struct Link {
    path: Arc<str>,
    target: Arc<str>,
}

/// Links a walk follows before it gives up (POSIX's `SYMLOOP_MAX`).
const MAX_LINK_HOPS: usize = 40;

impl Node {
    fn directory(path: impl Into<Arc<str>>, modified: SystemTime) -> Self {
        Self {
            path: path.into(),
            modified,
            kind: Kind::Directory(BTreeMap::new()),
        }
    }

    fn file_type(&self) -> FileType {
        match self.kind {
            Kind::File(_) => FileType::File,
            Kind::Directory(_) => FileType::Directory,
            Kind::Symlink(_) => FileType::Symlink,
        }
    }

    fn metadata(&self) -> Metadata {
        let len = match &self.kind {
            Kind::File(contents) => contents.len() as u64,
            _ => 0,
        };
        Metadata::new(self.file_type(), len, self.modified)
    }

    fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or_default()
    }

    fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Directory(_))
    }

    /// The node at `route` below this one, through directories only.
    fn get(&self, route: &[Box<str>]) -> Option<&Node> {
        route.iter().try_fold(self, |node, name| match &node.kind {
            Kind::Directory(entries) => entries.get(name),
            _ => None,
        })
    }

    fn get_mut(&mut self, route: &[Box<str>]) -> Option<&mut Node> {
        route
            .iter()
            .try_fold(self, |node, name| match &mut node.kind {
                Kind::Directory(entries) => entries.get_mut(name),
                _ => None,
            })
    }

    /// The entries of the directory at `route`.
    fn entries_mut(&mut self, route: &[Box<str>]) -> Option<&mut BTreeMap<Box<str>, Node>> {
        match &mut self.get_mut(route)?.kind {
            Kind::Directory(entries) => Some(entries),
            _ => None,
        }
    }

    fn to_entry(&self) -> Entry {
        Entry {
            path: self.path.to_string(),
            modified: self.modified,
            contents: match &self.kind {
                Kind::File(contents) => EntryContents::File(Arc::clone(contents)),
                Kind::Directory(_) => EntryContents::Directory,
                Kind::Symlink(target) => EntryContents::Symlink(target.to_string()),
            },
        }
    }
}

impl fmt::Debug for MemFs {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("MemFs")
            .field("case_sensitive", &self.case_sensitive)
            .field("clock", &self.clock)
            .finish_non_exhaustive()
    }
}

impl MemFs {
    /// An empty file system.
    pub fn new(case_sensitive: bool, clock: Arc<dyn Clock>) -> Self {
        Self {
            case_sensitive,
            clock,
            root: RwLock::new(Node::directory("/", SystemTime::UNIX_EPOCH)),
        }
    }

    /// A file system holding `entries` (tsgo `vfstest.FromMapWithClock`).
    ///
    /// The paths must be absolute and normalized, all rooted at `/` or all
    /// at drives. The entries are stamped in path order, compared component
    /// by component, and then created in that order with their missing
    /// directories (which are stamped as they are created). Two paths that
    /// name the same entry are an error, and so is an entry below a file or
    /// a link: tsgo would keep such an entry where no lookup through the
    /// link reaches it.
    pub fn from_entries<P: Into<String>>(
        entries: impl IntoIterator<Item = (P, Seed)>,
        case_sensitive: bool,
        clock: Arc<dyn Clock>,
    ) -> io::Result<Self> {
        let fs = Self::new(case_sensitive, clock);
        let mut entries = entries
            .into_iter()
            .map(|(path, seed)| (path.into(), seed))
            .collect::<Vec<(String, Seed)>>();
        let mut rooted_at_slash = None;
        for (path, seed) in &entries {
            check_seed_path(path, &mut rooted_at_slash)?;
            if let Seed::Symlink(target) = seed {
                check_seed_path(target, &mut rooted_at_slash)?;
            }
        }
        let mut canonical = BTreeMap::new();
        for (path, _) in &entries {
            if let Some(other) = canonical.insert(fs.fold(path), path) {
                let (first, second) = if other < path {
                    (other, path)
                } else {
                    (path, other)
                };
                return Err(invalid_input(format!(
                    "duplicate path: {first:?} and {second:?} have the same canonical path"
                )));
            }
        }
        entries.sort_by(|(left, _), (right, _)| {
            compare_paths_by_parts(
                left.strip_prefix('/').unwrap_or(left),
                right.strip_prefix('/').unwrap_or(right),
            )
        });
        let stamped = entries
            .into_iter()
            .map(|(path, seed)| {
                let modified = fs.clock.now();
                (path, seed, modified)
            })
            .collect::<Vec<_>>();
        let mut root = fs.write_lock();
        for (path, seed, modified) in stamped {
            let directory = parent(&path);
            if directory != "/" {
                fs.create_dir_all_in(&mut root, directory)
                    .map_err(|error| {
                        io::Error::new(
                            error.kind(),
                            format!(
                                "failed to create intermediate directories for {path:?}: {error}"
                            ),
                        )
                    })?;
            }
            let route = fs.literal_route(&path);
            let (name, directory_route) = route.split_last().expect("a seed below the root");
            let directory = root.entries_mut(directory_route).ok_or_else(|| {
                invalid_input(format!(
                    "{path:?} is below a symbolic link; tsgo's map would keep it where no lookup through the link reaches"
                ))
            })?;
            let kind = match seed {
                Seed::File(contents) => Kind::File(contents),
                Seed::Symlink(target) => Kind::Symlink(Arc::from(target)),
            };
            directory.insert(
                name.clone(),
                Node {
                    path: Arc::from(path),
                    modified,
                    kind,
                },
            );
        }
        drop(root);
        Ok(fs)
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Adds a link at `link` to the absolute path `target`, replacing what
    /// is there (tsgo `MapFS.AddSymlink`). The link's directory must exist;
    /// the link is not stamped by the clock.
    pub fn symlink(&self, target: &str, link: &str) -> io::Result<()> {
        let target = normalize(target)?;
        let link = normalize(link)?;
        let route = self.literal_route(&link);
        let Some((name, directory_route)) = route.split_last() else {
            return Err(invalid_input(format!("{link:?} names the root")));
        };
        let mut root = self.write_lock();
        let directory = root
            .entries_mut(directory_route)
            .ok_or_else(|| not_found(parent(&link)))?;
        directory.insert(
            name.clone(),
            Node {
                path: Arc::from(link),
                modified: SystemTime::UNIX_EPOCH,
                kind: Kind::Symlink(Arc::from(target)),
            },
        );
        Ok(())
    }

    /// The entry at `path` itself, no link followed (tsgo
    /// `MapFS.GetFileInfo`).
    pub fn entry(&self, path: &str) -> Option<Entry> {
        let route = self.literal_route(&normalize(path).ok()?);
        if route.is_empty() {
            return None;
        }
        self.read_lock().get(&route).map(Node::to_entry)
    }

    /// The target of the link at `path` (tsgo `MapFS.GetTargetOfSymlink`).
    pub fn read_link(&self, path: &str) -> io::Result<String> {
        match self.entry(path) {
            Some(Entry {
                contents: EntryContents::Symlink(target),
                ..
            }) => Ok(target),
            Some(_) => Err(invalid_input(format!("{path:?} is not a symbolic link"))),
            None => Err(not_found(path)),
        }
    }

    /// What `path` names: the links above it are followed, the entry itself
    /// is reported as it is.
    pub fn symlink_metadata(&self, path: &str) -> io::Result<Metadata> {
        let root = self.read_lock();
        match self.lookup(&root, &normalize(path)?, false)? {
            Lookup::Found(route) => Ok(root.get(&route).expect("found").metadata()),
            Lookup::Missing { .. } => Err(not_found(path)),
        }
    }

    /// Every entry below the root (files, directories and links), each
    /// directory's entries in canonical-name order after the directory
    /// (tsgo `MapFS.Entries`).
    pub fn entries(&self) -> Vec<Entry> {
        fn visit(node: &Node, out: &mut Vec<Entry>) {
            if let Kind::Directory(entries) = &node.kind {
                for entry in entries.values() {
                    out.push(entry.to_entry());
                    visit(entry, out);
                }
            }
        }
        let mut out = Vec::new();
        visit(&self.read_lock(), &mut out);
        out
    }

    fn read_lock(&self) -> RwLockReadGuard<'_, Node> {
        self.root.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_lock(&self) -> RwLockWriteGuard<'_, Node> {
        self.root.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// The canonical spelling of a name or path (tsgo
    /// `tspath.GetCanonicalFileName`).
    fn fold(&self, name: &str) -> String {
        if self.case_sensitive {
            name.to_owned()
        } else {
            to_file_name_lower_case(name)
        }
    }

    /// The route of a normalized path's own components, no link followed.
    fn literal_route(&self, normal: &str) -> Route {
        components(normal)
            .map(|name| Box::from(self.fold(name)))
            .collect()
    }

    /// Walks a normalized path from the root, following the links in every
    /// component but the last and, when `follow_last`, in the last one. A
    /// file where a directory is needed is an error.
    fn lookup(&self, root: &Node, normal: &str, follow_last: bool) -> io::Result<Lookup> {
        let mut pending = components(normal)
            .map(str::to_owned)
            .collect::<VecDeque<_>>();
        let mut route: Route = Vec::new();
        let mut link = None;
        let mut hops = 0;
        while let Some(name) = pending.pop_front() {
            let Some(Kind::Directory(entries)) = root.get(&route).map(|node| &node.kind) else {
                unreachable!("a walk stands in a directory");
            };
            let key: Box<str> = Box::from(self.fold(&name));
            let Some(node) = entries.get(&key) else {
                let mut rest = vec![name];
                rest.extend(pending);
                return Ok(Lookup::Missing {
                    parent: route,
                    rest,
                    link,
                });
            };
            match &node.kind {
                Kind::Symlink(target) if follow_last || !pending.is_empty() => {
                    hops += 1;
                    if hops > MAX_LINK_HOPS {
                        return Err(io::Error::other(format!(
                            "{normal:?}: too many levels of symbolic links"
                        )));
                    }
                    link = Some(Link {
                        path: Arc::clone(&node.path),
                        target: Arc::clone(target),
                    });
                    let mut next = components(target)
                        .map(str::to_owned)
                        .collect::<VecDeque<_>>();
                    next.extend(pending);
                    pending = next;
                    route.clear();
                }
                Kind::File(_) | Kind::Symlink(_) if !pending.is_empty() => {
                    return Err(not_a_directory(&node.path));
                }
                _ => route.push(key),
            }
        }
        Ok(Lookup::Found(route))
    }

    /// tsgo `MapFS.mkdirAll`: walks the path a prefix at a time; a prefix
    /// that a link leads elsewhere restarts the walk from the directory the
    /// link reaches, and the missing directories are created in order.
    fn create_dir_all_in(&self, root: &mut Node, normal: &str) -> io::Result<()> {
        if let Ok(Lookup::Found(route)) = self.lookup(root, normal, true) {
            let node = root.get(&route).expect("found");
            return if node.is_dir() {
                Ok(())
            } else {
                Err(mkdir_not_a_directory(normal))
            };
        }
        let mut path = normal.to_owned();
        let mut restarts = 0;
        'walk: loop {
            let names = components(&path).map(str::to_owned).collect::<Vec<_>>();
            let mut missing = Vec::new();
            for end in 1..=names.len() {
                let prefix = join(names[..end].iter().map(String::as_str));
                match self.lookup(root, &prefix, true)? {
                    Lookup::Found(route) => {
                        let node = root.get(&route).expect("found");
                        if !node.is_dir() {
                            return Err(mkdir_not_a_directory(&node.path));
                        }
                        if route != self.literal_route(&prefix) {
                            restarts += 1;
                            if restarts > MAX_LINK_HOPS {
                                return Err(io::Error::other(format!(
                                    "{normal:?}: too many levels of symbolic links"
                                )));
                            }
                            let rest = names[end..].join("/");
                            path = if rest.is_empty() {
                                node.path.to_string()
                            } else {
                                child_path(&node.path, &rest)
                            };
                            continue 'walk;
                        }
                    }
                    Lookup::Missing {
                        link: Some(link), ..
                    } => return Err(broken_link(&link)),
                    Lookup::Missing { link: None, .. } => missing.push(prefix),
                }
            }
            for directory in missing {
                let route = self.literal_route(&directory);
                let (name, parent_route) = route.split_last().expect("below the root");
                let modified = self.clock.now();
                root.entries_mut(parent_route)
                    .expect("the parent was found or created")
                    .insert(name.clone(), Node::directory(directory, modified));
            }
            return Ok(());
        }
    }

    /// tsgo `MapFS.WriteFile`/`AppendFile`: the parent must be a directory;
    /// the file, or a link's missing target, is (re)written with the
    /// spelling `normal`.
    fn put(
        &self,
        root: &mut Node,
        normal: &str,
        contents: impl FnOnce(Option<&Arc<[u8]>>) -> Arc<[u8]>,
        operation: &str,
    ) -> io::Result<()> {
        let directory = parent(normal);
        if directory != "/" && directory != normal {
            match self.lookup(root, directory, true) {
                Ok(Lookup::Found(route)) => {
                    if !root.get(&route).expect("found").is_dir() {
                        return Err(io::Error::new(
                            io::ErrorKind::NotADirectory,
                            format!(
                                "{operation} {normal:?}: parent path exists but is not a directory"
                            ),
                        ));
                    }
                }
                Ok(Lookup::Missing {
                    link: Some(link), ..
                }) => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{operation} {normal:?}: {}", broken_link(&link)),
                    ))
                }
                Ok(Lookup::Missing { link: None, .. }) => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("{operation} {normal:?}: file does not exist"),
                    ))
                }
                Err(error) => return Err(error),
            }
        }
        let modified;
        match self.lookup(root, normal, true)? {
            Lookup::Found(route) => {
                let node = root.get_mut(&route).filter(|_| !route.is_empty());
                let Some(node) = node else {
                    return Err(not_a_regular_file(operation, normal));
                };
                let Kind::File(existing) = &node.kind else {
                    return Err(not_a_regular_file(operation, normal));
                };
                let contents = contents(Some(existing));
                modified = self.clock.now();
                *node = Node {
                    path: Arc::from(normal),
                    modified,
                    kind: Kind::File(contents),
                };
            }
            Lookup::Missing {
                parent: mut route,
                rest,
                ..
            } => {
                // A link to a missing entry: the file is created where the
                // link leads, with the directories on the way.
                let (name, directories) = rest.split_last().expect("a missing name");
                modified = self.clock.now();
                for directory in directories {
                    let base = Arc::clone(&root.get(&route).expect("walked").path);
                    let key: Box<str> = Box::from(self.fold(directory));
                    root.entries_mut(&route)
                        .expect("walked directory")
                        .entry(key.clone())
                        .or_insert_with(|| Node::directory(child_path(&base, directory), modified));
                    route.push(key);
                }
                root.entries_mut(&route).expect("walked directory").insert(
                    Box::from(self.fold(name)),
                    Node {
                        path: Arc::from(normal),
                        modified,
                        kind: Kind::File(contents(None)),
                    },
                );
            }
        }
        Ok(())
    }
}

impl FileSystem for MemFs {
    fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        let root = self.read_lock();
        match self.lookup(&root, &normalize(path)?, true)? {
            Lookup::Found(route) => match &root.get(&route).expect("found").kind {
                Kind::File(contents) => Ok(contents.to_vec()),
                _ => Err(io::Error::new(
                    io::ErrorKind::IsADirectory,
                    format!("read {path:?}: is a directory"),
                )),
            },
            Lookup::Missing { .. } => Err(not_found(path)),
        }
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        let root = self.read_lock();
        match self.lookup(&root, &normalize(path)?, true)? {
            Lookup::Found(route) => Ok(root.get(&route).expect("found").metadata()),
            Lookup::Missing { .. } => Err(not_found(path)),
        }
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        let root = self.read_lock();
        match self.lookup(&root, &normalize(path)?, true)? {
            Lookup::Found(route) => match &root.get(&route).expect("found").kind {
                Kind::Directory(entries) => Ok(entries
                    .values()
                    .map(|node| DirEntry::new(node.name(), node.file_type()))
                    .collect()),
                _ => Err(not_a_directory(path)),
            },
            Lookup::Missing { .. } => Err(not_found(path)),
        }
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        let normal = normalize(path)?;
        let root = self.read_lock();
        match self.lookup(&root, &normal, true)? {
            Lookup::Found(route) if route.is_empty() => Ok(normal),
            Lookup::Found(route) => Ok(root.get(&route).expect("found").path.to_string()),
            Lookup::Missing { .. } => Err(not_found(path)),
        }
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let normal = normalize(path)?;
        let mut root = self.write_lock();
        self.put(&mut root, &normal, |_| Arc::from(contents), "write")
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        let normal = normalize(path)?;
        let mut root = self.write_lock();
        self.put(
            &mut root,
            &normal,
            |existing| {
                let mut combined = existing.map_or_else(Vec::new, |existing| existing.to_vec());
                combined.extend_from_slice(contents);
                Arc::from(combined)
            },
            "append",
        )
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        let normal = normalize(path)?;
        let mut root = self.write_lock();
        self.create_dir_all_in(&mut root, &normal)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        let route = self.literal_route(&normalize(path)?);
        if let Some((name, directory)) = route.split_last() {
            if let Some(entries) = self.write_lock().entries_mut(directory) {
                entries.remove(name);
            }
        }
        Ok(())
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        let route = self.literal_route(&normalize(path)?);
        let mut root = self.write_lock();
        match root.get_mut(&route).filter(|_| !route.is_empty()) {
            Some(node) => {
                node.modified = modified;
                Ok(())
            }
            None => Err(not_found(path)),
        }
    }
}

/// tsgo `comparePathsByParts`: component by component while both paths
/// have one more, then the remainders as strings.
fn compare_paths_by_parts(mut left: &str, mut right: &str) -> Ordering {
    loop {
        match (left.split_once('/'), right.split_once('/')) {
            (Some((left_head, left_rest)), Some((right_head, right_rest))) => {
                match left_head.cmp(right_head) {
                    Ordering::Equal => {
                        left = left_rest;
                        right = right_rest;
                    }
                    order => return order,
                }
            }
            _ => return left.cmp(right),
        }
    }
}

/// A seed path is absolute and normalized, and every path of one file
/// system is rooted alike.
fn check_seed_path(path: &str, rooted_at_slash: &mut Option<bool>) -> io::Result<()> {
    if normalize(path)? != path {
        return Err(invalid_input(format!("non-normalized path {path:?}")));
    }
    let at_slash = path.starts_with('/');
    match rooted_at_slash {
        Some(existing) if *existing != at_slash => {
            Err(invalid_input("mixed posix and windows paths".to_owned()))
        }
        _ => {
            *rooted_at_slash = Some(at_slash);
            Ok(())
        }
    }
}

fn child_path(directory: &str, name: &str) -> String {
    if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

fn not_found(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{path:?}: file does not exist"),
    )
}

fn not_a_directory(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotADirectory,
        format!("{path:?}: path exists but is not a directory"),
    )
}

fn mkdir_not_a_directory(path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotADirectory,
        format!("mkdir {path:?}: path exists but is not a directory"),
    )
}

fn not_a_regular_file(operation: &str, path: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::IsADirectory,
        format!("{operation} {path:?}: path exists but is not a regular file"),
    )
}

fn broken_link(link: &Link) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("broken symlink {:?} -> {:?}", link.path, link.target),
    )
}

fn invalid_input(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
