//! tsgo `project/watch.go`: the files a snapshot asks the editor to watch,
//! as globs over the workspace and recursive watches of the directories
//! outside it, and the registrations the session keeps of them, counted by
//! glob.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use tsc_diagnostics::JsStr;
use tsc_lsproto::{
    FileSystemWatcher, PatternOrRelativePattern, RelativePattern, Uri, WatchKind,
    WorkspaceFolderOrURI,
};

/// tsgo `minWatchLocationDepth`.
const MIN_WATCH_LOCATION_DEPTH: usize = 2;

/// tsgo's `WatchKindCreate | WatchKindChange | WatchKindDelete`.
pub(crate) const WATCH_ALL: WatchKind = WatchKind(7);

/// tsgo's process-wide `watcherID`.
static WATCHER_ID: AtomicU64 = AtomicU64::new(0);

fn next_watcher_id() -> u64 {
    WATCHER_ID.fetch_add(1, Ordering::SeqCst) + 1
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// tsgo `PatternsAndIgnored`: globs inside the workspace, directories
/// outside it, and the directories too shallow to watch.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PatternsAndIgnored {
    pub(crate) directories_outside_workspace: Vec<String>,
    pub(crate) patterns_inside_workspace: Vec<String>,
    pub(crate) ignored: BTreeSet<String>,
}

/// tsgo `Watchers`: a watch's ID and its watchers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watchers {
    pub id: String,
    pub workspace_watchers: Vec<FileSystemWatcher>,
    pub outside_workspace_watchers: Vec<FileSystemWatcher>,
    pub ignored_paths: BTreeSet<String>,
}

type ComputeGlobPatterns<T> = Arc<dyn Fn(&T) -> PatternsAndIgnored + Send + Sync>;

/// A watch's watchers once computed.
#[derive(Debug)]
struct Computed {
    workspace: Vec<FileSystemWatcher>,
    outside: Vec<FileSystemWatcher>,
    ignored: BTreeSet<String>,
    id: u64,
}

/// tsgo `WatchedFiles[T]`: the files of `input` to watch, computed when
/// first asked; a watch whose watchers changed from the ones it was cloned
/// with takes a new ID.
pub(crate) struct WatchedFiles<T> {
    name: String,
    watch_kind: WatchKind,
    has_relative_pattern_capability: bool,
    compute_glob_patterns: ComputeGlobPatterns<T>,
    input: T,
    /// The watchers of the watch this one was cloned from (tsgo copies them
    /// into the clone).
    inherited: (Vec<FileSystemWatcher>, Vec<FileSystemWatcher>),
    id: u64,
    computed: OnceLock<Computed>,
}

impl<T> std::fmt::Debug for WatchedFiles<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WatchedFiles")
            .field("name", &self.name)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<T: Default> WatchedFiles<T> {
    /// tsgo `NewWatchedFiles`.
    pub(crate) fn new(
        name: impl Into<String>,
        watch_kind: WatchKind,
        has_relative_pattern_capability: bool,
        compute_glob_patterns: impl Fn(&T) -> PatternsAndIgnored + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            watch_kind,
            has_relative_pattern_capability,
            compute_glob_patterns: Arc::new(compute_glob_patterns),
            input: T::default(),
            inherited: (Vec::new(), Vec::new()),
            id: next_watcher_id(),
            computed: OnceLock::new(),
        }
    }
}

impl<T> WatchedFiles<T> {
    /// tsgo `Watchers`.
    pub(crate) fn watchers(&self) -> Watchers {
        let computed = self.computed.get_or_init(|| {
            let result = (self.compute_glob_patterns)(&self.input);
            let mut globs = result.patterns_inside_workspace;
            globs.sort();
            globs.dedup();
            let mut changed = false;
            let same_workspace = self.inherited.0.len() == globs.len()
                && self
                    .inherited
                    .0
                    .iter()
                    .zip(&globs)
                    .all(|(watcher, glob)| glob_string(watcher) == *glob);
            let workspace = if same_workspace {
                self.inherited.0.clone()
            } else {
                changed = true;
                globs
                    .iter()
                    .map(|glob| FileSystemWatcher {
                        glob_pattern: PatternOrRelativePattern::Pattern(glob.clone()),
                        kind: Some(self.watch_kind),
                    })
                    .collect()
            };
            let mut directories = result.directories_outside_workspace;
            directories.sort();
            directories.dedup();
            let same_outside = self.inherited.1.len() == directories.len()
                && self
                    .inherited
                    .1
                    .iter()
                    .zip(&directories)
                    .all(|(watcher, directory)| {
                        glob_string(watcher)
                            == recursive_directory_glob_pattern(
                                directory,
                                self.has_relative_pattern_capability,
                            )
                    });
            let outside = if same_outside {
                self.inherited.1.clone()
            } else {
                changed = true;
                directories
                    .iter()
                    .map(|directory| {
                        new_recursive_directory_watcher(
                            directory,
                            self.watch_kind,
                            self.has_relative_pattern_capability,
                        )
                    })
                    .collect()
            };
            Computed {
                workspace,
                outside,
                ignored: result.ignored,
                id: if changed { next_watcher_id() } else { self.id },
            }
        });
        Watchers {
            id: format!("{} watcher {}", self.name, computed.id),
            workspace_watchers: computed.workspace.clone(),
            outside_workspace_watchers: computed.outside.clone(),
            ignored_paths: computed.ignored.clone(),
        }
    }

    /// tsgo `ID`.
    pub(crate) fn id(&self) -> String {
        self.watchers().id
    }

    /// tsgo `Clone`: the watch of `input`, starting from this one's
    /// watchers (and without an ID of its own until they change).
    pub(crate) fn clone_with(&self, input: T) -> Self {
        let inherited = match self.computed.get() {
            Some(computed) => (computed.workspace.clone(), computed.outside.clone()),
            None => self.inherited.clone(),
        };
        Self {
            name: self.name.clone(),
            watch_kind: self.watch_kind,
            has_relative_pattern_capability: self.has_relative_pattern_capability,
            compute_glob_patterns: Arc::clone(&self.compute_glob_patterns),
            input,
            inherited,
            id: 0,
            computed: OnceLock::new(),
        }
    }
}

/// tsgo's `programFilesWatch`: of the files a build looked at (path to
/// file name).
pub(crate) type ProgramFilesWatch = WatchedFiles<BTreeMap<String, String>>;

/// tsgo's `contentMapperWatch`: of the content mappers' files.
pub(crate) type ContentMapperWatch = WatchedFiles<Vec<String>>;

/// tsgo `fileSystemWatcherGlobString`.
pub(crate) fn glob_string(watcher: &FileSystemWatcher) -> String {
    match &watcher.glob_pattern {
        PatternOrRelativePattern::Pattern(pattern) => pattern.clone(),
        PatternOrRelativePattern::RelativePattern(relative) => {
            format!("{}/{}", base_uri(relative), relative.pattern)
        }
    }
}

fn base_uri(relative: &RelativePattern) -> &str {
    match &relative.base_uri {
        WorkspaceFolderOrURI::URI(uri) => &uri.0,
        WorkspaceFolderOrURI::WorkspaceFolder(_) => {
            panic!("workspace folder-based relative patterns not implemented")
        }
    }
}

/// tsgo `fileSystemWatcherKey`: a watcher's glob and kind.
type WatcherKey = (String, u32);

fn watcher_key(watcher: &FileSystemWatcher) -> WatcherKey {
    (glob_string(watcher), watcher.kind.unwrap_or(WATCH_ALL).0)
}

#[derive(Default)]
struct RegistryState {
    entries: HashMap<WatcherKey, (usize, String)>,
    pending: HashSet<String>,
}

/// tsgo `watchRegistry`: the registered globs, counted by the watches
/// that have them, and the watches whose registration failed.
#[derive(Default)]
pub(crate) struct WatchRegistry {
    state: Mutex<RegistryState>,
}

impl WatchRegistry {
    /// tsgo `Acquire`: whether the watcher is new (to register).
    pub(crate) fn acquire(&self, watcher: &FileSystemWatcher, id: &str) -> bool {
        let mut state = lock(&self.state);
        let entry = state
            .entries
            .entry(watcher_key(watcher))
            .or_insert_with(|| (0, id.to_owned()));
        entry.0 += 1;
        entry.0 == 1
    }

    /// tsgo `Release`: the ID to unregister when no watch has the watcher.
    pub(crate) fn release(&self, watcher: &FileSystemWatcher) -> Option<String> {
        let mut state = lock(&self.state);
        let key = watcher_key(watcher);
        let entry = state.entries.get_mut(&key)?;
        if entry.0 <= 1 {
            return state.entries.remove(&key).map(|(_, id)| id);
        }
        entry.0 -= 1;
        None
    }

    /// tsgo `MarkPending`.
    pub(crate) fn mark_pending(&self, id: &str) {
        lock(&self.state).pending.insert(id.to_owned());
    }

    /// tsgo `ClearPending`.
    pub(crate) fn clear_pending(&self, id: &str) {
        lock(&self.state).pending.remove(id);
    }

    /// tsgo `IsPending`.
    pub(crate) fn is_pending(&self, id: &str) -> bool {
        lock(&self.state).pending.contains(id)
    }
}

/// tsgo `getRecursiveGlobPattern`.
pub(crate) fn recursive_glob_pattern(directory: &str) -> String {
    format!("{}/**/*", directory.strip_suffix('/').unwrap_or(directory))
}

/// tsgo `recursiveDirectoryGlobPattern`.
fn recursive_directory_glob_pattern(directory: &str, use_relative_pattern: bool) -> String {
    if use_relative_pattern {
        return format!(
            "{}/**/*",
            tsc_program::uri::file_name_to_document_uri(directory)
        );
    }
    recursive_glob_pattern(directory)
}

/// tsgo `newRecursiveDirectoryWatcher`.
pub(crate) fn new_recursive_directory_watcher(
    directory: &str,
    kind: WatchKind,
    use_relative_pattern: bool,
) -> FileSystemWatcher {
    let glob_pattern = if use_relative_pattern {
        PatternOrRelativePattern::RelativePattern(RelativePattern {
            base_uri: WorkspaceFolderOrURI::URI(Uri(tsc_program::uri::file_name_to_document_uri(
                directory,
            ))),
            pattern: "**/*".to_owned(),
        })
    } else {
        PatternOrRelativePattern::Pattern(recursive_glob_pattern(directory))
    };
    FileSystemWatcher {
        glob_pattern,
        kind: Some(kind),
    }
}

/// tsgo `GetPathComponents`: the root, then each component.
pub(crate) fn path_components(path: &str) -> Vec<String> {
    let (root, rest) = match tsc_program::path_root_parts(JsStr::from_str(path)) {
        Some((root, rest)) => (
            root.to_string_lossy().into_owned(),
            rest.to_string_lossy().into_owned(),
        ),
        None => (String::new(), path.to_owned()),
    };
    let mut components = vec![root];
    components.extend(
        rest.split('/')
            .filter(|component| !component.is_empty())
            .map(str::to_owned),
    );
    components
}

/// tsgo `GetPathFromPathComponents`.
fn path_from_components(components: &[String]) -> String {
    let Some((root, rest)) = components.split_first() else {
        return String::new();
    };
    let root = if root.is_empty() || root.ends_with('/') {
        root.clone()
    } else {
        format!("{root}/")
    };
    format!("{root}{}", rest.join("/"))
}

/// tsgo `reducePathComponents`.
fn reduce_path_components(components: Vec<String>) -> Vec<String> {
    let mut components = components.into_iter();
    let Some(first) = components.next() else {
        return Vec::new();
    };
    let mut reduced = vec![first];
    for component in components {
        match component.as_str() {
            "" | "." => continue,
            ".." if reduced.len() > 1 && reduced.last().is_some_and(|last| last != "..") => {
                reduced.pop();
                continue;
            }
            ".." if reduced.len() == 1 && !reduced[0].is_empty() => continue,
            _ => reduced.push(component),
        }
    }
    reduced
}

/// tsgo `getPathComponentsForWatching`: a UNC share, `C:/Users/<name>`
/// and `/home/<name>` count as roots.
pub(crate) fn path_components_for_watching(path: &str) -> Vec<String> {
    let components = path_components(path);
    let root_length = perceived_os_root_length_for_watching(&components);
    if root_length <= 1 {
        return components;
    }
    let root = components[1..root_length]
        .iter()
        .fold(components[0].clone(), |root, component| {
            tsc_program::combine_paths(JsStr::from_str(&root), JsStr::from_str(component))
                .to_string_lossy()
                .into_owned()
        });
    std::iter::once(root)
        .chain(components[root_length..].iter().cloned())
        .collect()
}

/// tsgo `perceivedOsRootLengthForWatching`.
fn perceived_os_root_length_for_watching(components: &[String]) -> usize {
    let length = components.len();
    if length <= 1 {
        return length;
    }
    if components[0].starts_with("//") {
        return 2;
    }
    let root = components[0].as_bytes();
    if root.len() == 3 && root[0].is_ascii_alphabetic() && root[1] == b':' && root[2] == b'/' {
        if components[1].eq_ignore_ascii_case("users") {
            return length.min(3);
        }
        return 1;
    }
    if components[1] == "home" {
        return length.min(3);
    }
    1
}

/// tsgo `tspath.GetCommonParentDirectories`: the directories to watch for
/// `directories` (their common parents, fanned out where they diverge
/// within `min_components`), and those too shallow to watch.
pub(crate) fn common_parent_directories(
    directories: &[String],
    min_components: usize,
    case_sensitive: bool,
) -> (Vec<String>, BTreeSet<String>) {
    assert!(min_components >= 1, "minComponents must be at least 1");
    if directories.is_empty() {
        return (Vec::new(), BTreeSet::new());
    }
    if let [directory] = directories {
        if reduce_path_components(path_components_for_watching(directory)).len() < min_components {
            return (Vec::new(), BTreeSet::from([directory.clone()]));
        }
        return (directories.to_vec(), BTreeSet::new());
    }
    let mut ignored = BTreeSet::new();
    let mut groups = Vec::new();
    for directory in directories {
        let components = reduce_path_components(path_components_for_watching(directory));
        if components.len() < min_components {
            ignored.insert(directory.clone());
        } else {
            groups.push(components);
        }
    }
    // tsgo `RootedDirectoryPathFromAbsolute(GetPathFromPathComponents(...))`.
    let parents = common_parents_worker(&groups, min_components, case_sensitive)
        .iter()
        .map(|components| {
            tsc_program::get_normalized_absolute_path(
                JsStr::from_str(&path_from_components(components)),
                JsStr::from_str(""),
            )
            .to_string_lossy()
            .into_owned()
        })
        .collect();
    (parents, ignored)
}

/// tsgo `getCommonParentsWorker`.
fn common_parents_worker(
    groups: &[Vec<String>],
    min_components: usize,
    case_sensitive: bool,
) -> Vec<Vec<String>> {
    let Some(first) = groups.first() else {
        return Vec::new();
    };
    let canonical = |component: &str| {
        if case_sensitive {
            component.to_owned()
        } else {
            tsc_host::to_file_name_lower_case(component)
        }
    };
    let max_depth = groups.iter().map(Vec::len).min().unwrap_or(0);
    for last_common in 0..max_depth {
        let candidate = canonical(&first[last_common]);
        if groups[1..]
            .iter()
            .all(|group| canonical(&group[last_common]) == candidate)
        {
            continue;
        }
        if last_common >= min_components {
            return vec![first[..last_common].to_vec()];
        }
        // Not enough components: fan out by the diverging component.
        let mut by_key: BTreeMap<String, (Vec<String>, Vec<Vec<String>>)> = BTreeMap::new();
        for group in groups {
            let entry = by_key
                .entry(canonical(&group[last_common]))
                .or_insert_with(|| (Vec::new(), Vec::new()));
            entry.0 = group[..=last_common].to_vec();
            entry.1.push(group[last_common + 1..].to_vec());
        }
        let mut result = Vec::new();
        for (head, tails) in by_key.into_values() {
            for sub in
                common_parents_worker(&tails, min_components - (last_common + 1), case_sensitive)
            {
                if sub.is_empty() {
                    result.push(head.clone());
                } else {
                    result.push([head.clone(), sub].concat());
                }
            }
        }
        return result;
    }
    vec![first[..max_depth].to_vec()]
}

/// tsgo `trimContainedPath`'s answer: whether `child` is `parent` or below
/// it (the roots compared without case unless a dynamic name is involved).
pub(crate) fn contains_path(parent: &str, child: &str, case_sensitive: bool) -> bool {
    if parent.is_empty() || child.is_empty() {
        return false;
    }
    let split = |path: &str| -> (String, String) {
        match tsc_program::path_root_parts(JsStr::from_str(path)) {
            Some((root, rest)) => (
                root.to_string_lossy().into_owned(),
                rest.to_string_lossy().into_owned(),
            ),
            None => (String::new(), path.to_owned()),
        }
    };
    let (mut parent_root, parent_rest) = split(parent);
    let (mut child_root, child_rest) = split(child);
    let dynamic = tsc_program::is_encoded_dynamic_file_name(parent)
        || tsc_program::is_encoded_dynamic_file_name(child);
    let case_sensitive = if dynamic {
        parent_root = parent_root.trim_end_matches('/').to_owned();
        child_root = child_root.trim_end_matches('/').to_owned();
        true
    } else {
        case_sensitive
    };
    let roots_equal = if dynamic {
        parent_root == child_root
    } else {
        parent_root.eq_ignore_ascii_case(&child_root)
            || tsc_host::to_file_name_lower_case(&parent_root)
                == tsc_host::to_file_name_lower_case(&child_root)
    };
    if !roots_equal {
        return false;
    }
    let (prefix, text) = if case_sensitive {
        (parent_rest, child_rest)
    } else {
        (
            tsc_host::to_file_name_lower_case(&parent_rest),
            tsc_host::to_file_name_lower_case(&child_rest),
        )
    };
    let Some(relative) = text.strip_prefix(prefix.as_str()) else {
        return false;
    };
    relative.is_empty() || parent.ends_with('/') || prefix.is_empty() || relative.starts_with('/')
}

/// tsgo `createResolutionLookupGlobMapper`: the program files watch of a
/// project, from the files its build looked at (path to file name): the
/// workspace, the project's directory and the library directory
/// recursively when a file is below them, each `node_modules` with a file,
/// and the common parents of the other directories.
pub(crate) fn resolution_lookup_globs(
    workspace_directory: &str,
    library_directory: &str,
    project_directory: &str,
    case_sensitive: bool,
    files: &BTreeMap<String, String>,
) -> PatternsAndIgnored {
    let key = |path: &str| {
        if case_sensitive {
            path.to_owned()
        } else {
            tsc_host::to_file_name_lower_case(path)
        }
    };
    let workspace = key(workspace_directory);
    let project = key(project_directory);
    let library = key(library_directory);
    let parent = |path: &str| {
        tsc_program::get_directory_path(JsStr::from_str(path))
            .to_string_lossy()
            .into_owned()
    };
    let mut seen_directories = HashSet::new();
    let (mut include_workspace, mut include_root, mut include_library) = (false, false, false);
    let mut node_modules_directories = BTreeMap::new();
    let mut external_directories = BTreeMap::new();
    for (path, file_name) in files {
        if tsc_program::is_encoded_dynamic_file_name(path) || path.starts_with("^/") {
            continue;
        }
        if !seen_directories.insert(parent(path)) {
            continue;
        }
        if contains_path(&workspace, path, true) {
            include_workspace = true;
        } else if contains_path(&project, path, true) {
            include_root = true;
        } else if contains_path(&library, path, true) {
            include_library = true;
        } else if let Some(directory) = node_modules_directory(path) {
            let presentation = node_modules_directory_spelling(file_name, case_sensitive)
                .expect("canonical node_modules path did not have a presentation component");
            node_modules_directories.insert(directory, presentation);
        } else {
            external_directories.insert(parent(path), parent(file_name));
        }
    }
    let mut globs = Vec::new();
    if include_workspace {
        globs.push(recursive_glob_pattern(workspace_directory));
    }
    if include_root {
        globs.push(recursive_glob_pattern(project_directory));
    }
    if include_library {
        globs.push(recursive_glob_pattern(library_directory));
    }
    let mut node_modules_globs = node_modules_directories
        .values()
        .map(|directory| recursive_glob_pattern(directory))
        .collect::<Vec<_>>();
    node_modules_globs.sort();
    globs.extend(node_modules_globs);
    let mut result = PatternsAndIgnored {
        patterns_inside_workspace: globs,
        ..PatternsAndIgnored::default()
    };
    if !external_directories.is_empty() {
        let directories = external_directories.into_values().collect::<Vec<_>>();
        let (mut parents, ignored) =
            common_parent_directories(&directories, MIN_WATCH_LOCATION_DEPTH, case_sensitive);
        parents.sort();
        result.directories_outside_workspace = parents;
        result.ignored = ignored;
    }
    result
}

/// tsgo `SplitAtCanonicalComponent(path, "node_modules")`'s directory: the
/// path through its first `node_modules` component.
fn node_modules_directory(path: &str) -> Option<String> {
    let index = path
        .match_indices("/node_modules")
        .find(|(index, matched)| {
            let end = index + matched.len();
            end == path.len() || path.as_bytes()[end] == b'/'
        })?
        .0;
    Some(path[..index + "/node_modules".len()].to_owned())
}

/// tsgo `SplitFilePathAtComponent(fileName, "node_modules")`: the file
/// name's spelling of the directory through its first `node_modules`.
fn node_modules_directory_spelling(file_name: &str, case_sensitive: bool) -> Option<String> {
    let mut offset = 0;
    for component in file_name.split('/') {
        let end = offset + component.len();
        let matches = if case_sensitive {
            component == "node_modules"
        } else {
            component.eq_ignore_ascii_case("node_modules")
        };
        if matches && offset > 0 {
            return Some(file_name[..end].to_owned());
        }
        offset = end + 1;
    }
    None
}

/// tsgo `NewWatchedFilesForPaths`' globs: the files inside the workspace,
/// and the directories of the others.
pub(crate) fn path_globs(
    workspace_directory: &str,
    case_sensitive: bool,
    files: &[String],
) -> PatternsAndIgnored {
    let mut result = PatternsAndIgnored::default();
    for file_name in files {
        if contains_path(workspace_directory, file_name, case_sensitive) {
            result.patterns_inside_workspace.push(file_name.clone());
        } else {
            result.directories_outside_workspace.push(
                tsc_program::get_directory_path(JsStr::from_str(file_name))
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    result
}

#[cfg(test)]
#[path = "../tests/unit/watch.rs"]
mod tests;
