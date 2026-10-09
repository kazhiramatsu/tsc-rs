//! tsgo `api/requestfilesystem/pathtree.go`: a request file system's
//! entries by path, each node keyed by its full path under its parent, with
//! what a missing entry falls back to. Composing a layer over a tree shares
//! every node the layer does not touch.

use std::collections::BTreeMap;
use std::sync::Arc;

use tsc_host::vfs::Entries;

use super::paths::{
    base_file_name, canonical_file_name, directory_path, ensure_trailing_separator,
};

/// tsgo `requestFallback`: whether a path not in the tree is read from the
/// base file system.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Fallback {
    /// As the nearest ancestor says.
    #[default]
    Inherit,
    Allowed,
    /// Missing, whatever the base file system holds.
    Missing,
}

/// tsgo `requestFile`.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RequestFile {
    pub(crate) file_name: String,
    pub(crate) content: String,
}

/// tsgo `requestSymlink`: a link and its absolute target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RequestSymlink {
    pub(crate) link_name: String,
    pub(crate) target: String,
    pub(crate) host: bool,
}

/// tsgo `requestDirectory`: a directory, with its complete listing when
/// the request gave one.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct RequestDirectory {
    pub(crate) directory_name: String,
    pub(crate) listing: Option<Entries>,
}

/// tsgo `requestEntry`.
#[derive(Clone, Debug)]
pub(crate) enum Entry {
    File(Arc<RequestFile>),
    Symlink(Arc<RequestSymlink>),
    Directory(Arc<RequestDirectory>),
}

/// tsgo `requestPathNode`.
#[derive(Clone, Debug, Default)]
pub(crate) struct PathNode {
    pub(crate) entry: Option<Entry>,
    pub(crate) fallback: Fallback,
    pub(crate) children: BTreeMap<String, Arc<PathNode>>,
    pub(crate) has_symlinks: bool,
}

/// tsgo `requestPathAncestors`: the path's ancestors from its root, ending
/// with the path.
pub(crate) fn path_ancestors(path: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut path = path.to_owned();
    loop {
        let parent = directory_path(&path);
        let done = parent == path;
        paths.push(path);
        if done {
            break;
        }
        path = parent;
    }
    paths.reverse();
    paths
}

/// tsgo `requestPathContains`.
pub(crate) fn path_contains(parent: &str, path: &str) -> bool {
    path == parent || path.starts_with(&ensure_trailing_separator(parent))
}

impl PathNode {
    /// tsgo `replacesSubtree`: a file or a link hides what was below it.
    pub(crate) fn replaces_subtree(&self) -> bool {
        matches!(self.entry, Some(Entry::File(_) | Entry::Symlink(_)))
    }

    /// tsgo `ensure`: the node of `path`, created with its ancestors.
    pub(crate) fn ensure(&mut self, path: &str) -> &mut PathNode {
        let mut node = self;
        for ancestor in path_ancestors(path) {
            node = Arc::make_mut(node.children.entry(ancestor).or_default());
        }
        node
    }

    /// tsgo `lookup`: the node of `path`, if any, and its fallback.
    pub(crate) fn lookup(&self, path: &str) -> (Option<&PathNode>, Fallback) {
        let mut fallback = Fallback::Inherit;
        let mut node = Some(self);
        for ancestor in path_ancestors(path) {
            let Some(current) = node else {
                break;
            };
            if current.fallback != Fallback::Inherit {
                fallback = current.fallback;
            }
            node = current.children.get(&ancestor).map(Arc::as_ref);
        }
        if let Some(node) = node {
            if node.fallback != Fallback::Inherit {
                fallback = node.fallback;
            }
        }
        (node, fallback)
    }

    /// tsgo `walkSymlinks`.
    pub(crate) fn walk_symlinks(&self, visit: &mut impl FnMut(&str, &Arc<RequestSymlink>)) {
        if !self.has_symlinks {
            return;
        }
        for (path, child) in &self.children {
            if let Some(Entry::Symlink(symlink)) = &child.entry {
                visit(path, symlink);
            }
            child.walk_symlinks(visit);
        }
    }

    /// tsgo `entries`: a directory's listing, given or derived from its
    /// files and directories.
    pub(crate) fn entries(&self) -> Option<Entries> {
        let Some(Entry::Directory(directory)) = &self.entry else {
            return None;
        };
        if let Some(listing) = &directory.listing {
            return Some(listing.clone());
        }
        let mut entries = Entries::default();
        for child in self.children.values() {
            match &child.entry {
                Some(Entry::File(file)) => entries.files.push(base_file_name(&file.file_name)),
                Some(Entry::Directory(directory)) => entries
                    .directories
                    .push(base_file_name(&directory.directory_name)),
                Some(Entry::Symlink(_)) | None => {}
            }
        }
        entries.files.sort();
        entries.directories.sort();
        Some(entries)
    }

    /// tsgo `firstSymlink`: the first link among the path's ancestors (the
    /// path included), by path.
    pub(crate) fn first_symlink(&self, path: &str) -> Option<(String, Arc<RequestSymlink>)> {
        let mut node = self;
        for ancestor in path_ancestors(path) {
            node = node.children.get(&ancestor)?;
            if let Some(Entry::Symlink(symlink)) = &node.entry {
                return Some((ancestor, Arc::clone(symlink)));
            }
        }
        None
    }

    /// tsgo `containsFileAncestor`: whether a file is a proper ancestor.
    pub(crate) fn contains_file_ancestor(&self, path: &str) -> bool {
        let mut node = self;
        for ancestor in path_ancestors(path) {
            let Some(child) = node.children.get(&ancestor) else {
                return false;
            };
            node = child;
            if ancestor != path && matches!(node.entry, Some(Entry::File(_))) {
                return true;
            }
        }
        false
    }
}

/// tsgo `composeRequestPaths`: `overlay` over `base`, a node of the overlay
/// with its own fallback, or a file or link, replacing the base's subtree.
pub(crate) fn compose(
    base: Option<&Arc<PathNode>>,
    overlay: Option<&Arc<PathNode>>,
    mut fallback: Fallback,
    case_sensitive: bool,
) -> Option<Arc<PathNode>> {
    let Some(overlay) = overlay else {
        return base.cloned();
    };
    let mut base = base;
    if overlay.fallback != Fallback::Inherit {
        fallback = overlay.fallback;
        base = None;
    }
    if overlay.replaces_subtree() {
        base = None;
    }
    let mut result = base.map(|base| (**base).clone()).unwrap_or_default();
    if overlay.fallback != Fallback::Inherit || overlay.replaces_subtree() {
        result.fallback = fallback;
    }
    let previous_listing = match &result.entry {
        Some(Entry::Directory(directory)) => Some(directory.listing.clone()),
        _ => None,
    };
    let overlay_directory = match &overlay.entry {
        Some(Entry::Directory(directory)) => Some(directory),
        _ => None,
    };
    if let Some(entry) = &overlay.entry {
        result.entry = Some(entry.clone());
        if let (Some(directory), Some(listing)) = (overlay_directory, &previous_listing) {
            if directory.listing.is_none() {
                result.entry = Some(Entry::Directory(Arc::new(RequestDirectory {
                    directory_name: directory.directory_name.clone(),
                    listing: listing.clone(),
                })));
            }
        }
    }
    for (path, child) in &overlay.children {
        let composed = compose(
            result.children.get(path),
            Some(child),
            fallback,
            case_sensitive,
        )
        .expect("an overlay child composes");
        result.children.insert(path.clone(), composed);
    }
    if let Some(Entry::Directory(directory)) = &result.entry {
        if let Some(listing) = &directory.listing {
            if overlay_directory.is_none_or(|directory| directory.listing.is_none()) {
                let mut entries = listing.clone();
                let equal = |left: &str, right: &str| {
                    canonical_file_name(left, case_sensitive)
                        == canonical_file_name(right, case_sensitive)
                };
                for (path, child) in &overlay.children {
                    let name = base_file_name(path);
                    if child.fallback == Fallback::Missing || child.replaces_subtree() {
                        entries.files.retain(|entry| !equal(entry, &name));
                        entries.directories.retain(|entry| !equal(entry, &name));
                        entries.symlinks.retain(|entry| !equal(entry, &name));
                    }
                    match &child.entry {
                        Some(Entry::File(file)) => {
                            entries = merge_entries(
                                &entries,
                                &Entries {
                                    files: vec![base_file_name(&file.file_name)],
                                    ..Entries::default()
                                },
                                &equal,
                            );
                        }
                        Some(Entry::Directory(directory)) => {
                            entries = merge_entries(
                                &entries,
                                &Entries {
                                    directories: vec![base_file_name(&directory.directory_name)],
                                    ..Entries::default()
                                },
                                &equal,
                            );
                        }
                        Some(Entry::Symlink(_)) | None => {}
                    }
                }
                result.entry = Some(Entry::Directory(Arc::new(RequestDirectory {
                    directory_name: directory.directory_name.clone(),
                    listing: Some(entries),
                })));
            }
        }
    }
    result.has_symlinks = matches!(result.entry, Some(Entry::Symlink(_)))
        || result.children.values().any(|child| child.has_symlinks);
    Some(Arc::new(result))
}

/// tsgo `mergeEntries`: `overlay`'s names over `base`'s; a name keeps the
/// overlay's kind and loses a link mark the overlay does not repeat.
pub(crate) fn merge_entries(
    base: &Entries,
    overlay: &Entries,
    equal: &dyn Fn(&str, &str) -> bool,
) -> Entries {
    let mut result = base.clone();
    let delete_symlink = |result: &mut Entries, name: &str| {
        result.symlinks.retain(|existing| !equal(existing, name));
    };
    for name in &overlay.files {
        result.directories.retain(|value| !equal(value, name));
        if !result.files.iter().any(|value| equal(value, name)) {
            result.files.push(name.clone());
        }
        delete_symlink(&mut result, name);
    }
    for name in &overlay.directories {
        result.files.retain(|value| !equal(value, name));
        if !result.directories.iter().any(|value| equal(value, name)) {
            result.directories.push(name.clone());
        }
        delete_symlink(&mut result, name);
    }
    result.symlinks.extend(overlay.symlinks.iter().cloned());
    result.files.sort();
    result.directories.sort();
    result
}
