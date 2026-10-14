//! tsgo `ls/autoimport/registry.go`: the auto-import registry a snapshot
//! keeps — per project, the index of its program's exports — and how a
//! snapshot's changes make a bucket dirty and a request for a file rebuild
//! it. A bucket stays usable for a request from the only file edited since
//! it was built: an edit inside a file seldom changes what that file can
//! import.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use tsc_checker::program::ProgramFileId;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_program::PreparedProgram;

use super::index::Index;
use crate::lsutil::UserPreferences;
use crate::utilities::path_key;

/// tsgo `newProgramStructure`.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
enum NewProgramStructure {
    #[default]
    False,
    SameFileNames,
    DifferentFileNames,
}

/// tsgo `bucketBuildPreferences`: the preferences a bucket's build read.
#[derive(Clone, Debug, Default)]
struct BuildPreferences {
    file_exclude_patterns: Vec<String>,
    entrypoint_directory_search: Option<bool>,
}

impl BuildPreferences {
    fn of(preferences: &UserPreferences) -> Self {
        Self {
            file_exclude_patterns: preferences
                .auto_import_file_exclude_patterns
                .clone()
                .unwrap_or_default(),
            entrypoint_directory_search: preferences.auto_import_entrypoint_directory_search,
        }
    }

    /// tsgo `Equal`: the same patterns in any order.
    fn equals(&self, other: &Self) -> bool {
        let sorted = |patterns: &[String]| {
            let mut patterns = patterns.to_vec();
            patterns.sort();
            patterns
        };
        sorted(&self.file_exclude_patterns) == sorted(&other.file_exclude_patterns)
            && self.entrypoint_directory_search == other.entrypoint_directory_search
    }
}

/// tsgo `BucketState`.
#[derive(Clone, Debug, Default)]
struct BucketState {
    /// The file edited last, if any (ignored with `multiple_files_dirty`).
    dirty_file: Option<String>,
    multiple_files_dirty: bool,
    new_program_structure: NewProgramStructure,
    build_preferences: BuildPreferences,
}

impl BucketState {
    /// tsgo `possiblyNeedsRebuildForFile`.
    fn possibly_needs_rebuild_for_file(&self, file: &str, preferences: &UserPreferences) -> bool {
        self.new_program_structure > NewProgramStructure::False
            || self.has_dirty_file_besides(file)
            || !self
                .build_preferences
                .equals(&BuildPreferences::of(preferences))
    }

    /// tsgo `hasDirtyFileBesides`.
    fn has_dirty_file_besides(&self, file: &str) -> bool {
        self.multiple_files_dirty
            || self
                .dirty_file
                .as_deref()
                .is_some_and(|dirty| dirty != file)
    }
}

/// tsgo `RegistryBucket` of a project.
#[derive(Clone, Debug, Default)]
pub(crate) struct RegistryBucket {
    state: BucketState,
    /// The paths of the files the bucket indexed.
    paths: Arc<BTreeSet<String>>,
    pub(crate) index: Arc<Index>,
}

impl RegistryBucket {
    /// tsgo `newRegistryBucket`: dirty, so the first request builds it.
    fn dirty() -> Self {
        Self {
            state: BucketState {
                multiple_files_dirty: true,
                new_program_structure: NewProgramStructure::DifferentFileNames,
                ..BucketState::default()
            },
            ..Self::default()
        }
    }

    /// tsgo `markProjectFileDirty`.
    fn mark_project_file_dirty(&mut self, file: &str) {
        if self.state.has_dirty_file_besides(file) {
            self.state.multiple_files_dirty = true;
        } else {
            self.state.dirty_file = Some(file.to_owned());
        }
    }
}

/// The module specifiers computed for an importing file, by target path
/// (tsgo's `specifierCache` entry; empty for none).
pub(crate) type SpecifierCache = Mutex<HashMap<String, String>>;

/// tsgo `Registry`.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    user_preferences: UserPreferences,
    projects: BTreeMap<String, RegistryBucket>,
    specifier_cache: BTreeMap<String, Arc<SpecifierCache>>,
}

/// tsgo `RegistryChange`: what a snapshot's update changed, by path.
#[derive(Clone, Debug, Default)]
pub struct RegistryChange {
    /// The file to prepare auto-imports for: its path and name.
    pub requested_file: Option<(String, String)>,
    /// The open files, by path.
    pub open_files: BTreeMap<String, String>,
    /// The paths of the files changed, created and deleted.
    pub changed: BTreeSet<String>,
    pub created: BTreeSet<String>,
    pub deleted: BTreeSet<String>,
    /// The projects whose program was rebuilt: whether with other files.
    pub rebuilt_programs: BTreeMap<String, bool>,
    pub user_preferences: Option<UserPreferences>,
}

/// What the registry asks of the snapshot it is built in (tsgo
/// `RegistryCloneHost`).
pub trait RegistryCloneHost {
    /// tsgo `GetDefaultProject`: the ID of a file's default project.
    fn default_project(&self, path: &str) -> Option<String>;
    /// tsgo `buildProjectBucket`: the paths of the files indexed and the
    /// index of the project's program's exports, if it has a program.
    fn build_project_bucket(
        &self,
        project: &str,
        preferences: &UserPreferences,
    ) -> Option<(BTreeSet<String>, Index)>;
    /// The paths of the program's files a project bucket indexes (tsgo
    /// `hasNewNonNodeModulesFiles` reads them).
    fn indexed_file_paths(&self, project: &str, preferences: &UserPreferences) -> Vec<String>;
}

impl Registry {
    /// tsgo `NewRegistry`.
    pub fn new(user_preferences: UserPreferences) -> Self {
        Self {
            user_preferences,
            ..Self::default()
        }
    }

    /// tsgo `IsPreparedForImportingFile`.
    pub(crate) fn is_prepared_for_importing_file(
        &self,
        path: &str,
        project: &str,
        preferences: &UserPreferences,
    ) -> bool {
        self.projects.get(project).is_some_and(|bucket| {
            !bucket
                .state
                .possibly_needs_rebuild_for_file(path, preferences)
        })
    }

    /// The bucket of a project.
    pub(crate) fn project_bucket(&self, project: &str) -> Option<&RegistryBucket> {
        self.projects.get(project)
    }

    /// The module specifiers computed for an importing file.
    pub(crate) fn specifier_cache(&self, path: &str) -> Option<&Arc<SpecifierCache>> {
        self.specifier_cache.get(path)
    }

    /// tsgo `Clone`: the registry after a snapshot's change, its requested
    /// file's project bucket rebuilt when needed.
    pub fn clone_with(&self, change: &RegistryChange, host: &dyn RegistryCloneHost) -> Self {
        let mut builder = self.clone();
        if let Some(preferences) = &change.user_preferences {
            if preferences.auto_import_specifier_exclude_regexes
                != builder
                    .user_preferences
                    .auto_import_specifier_exclude_regexes
            {
                builder.specifier_cache.clear();
            }
            builder.user_preferences = preferences.clone();
        }
        builder.update_bucket_existence(change, host);
        builder.mark_buckets_dirty(change);
        if let Some((requested, _)) = &change.requested_file {
            builder.update_indexes(requested, host);
        }
        builder
    }

    /// tsgo `updateBucketAndDirectoryExistence` for project buckets: one
    /// for the default project of each open file and of the requested file.
    fn update_bucket_existence(&mut self, change: &RegistryChange, host: &dyn RegistryCloneHost) {
        let mut needed_projects = BTreeSet::new();
        for path in change.open_files.keys() {
            if let Some(project) = host.default_project(path) {
                needed_projects.insert(project);
            }
            self.specifier_cache.entry(path.clone()).or_default();
        }
        if let Some((requested, _)) = &change.requested_file {
            if let Some(project) = host.default_project(requested) {
                needed_projects.insert(project);
            }
            self.specifier_cache.entry(requested.clone()).or_default();
        }
        let requested = change.requested_file.as_ref().map(|(path, _)| path);
        self.specifier_cache
            .retain(|path, _| change.open_files.contains_key(path) || Some(path) == requested);
        self.projects
            .retain(|project, _| needed_projects.contains(project));
        for project in needed_projects {
            self.projects
                .entry(project)
                .or_insert_with(RegistryBucket::dirty);
        }
    }

    /// tsgo `markBucketsDirty` for project buckets.
    fn mark_buckets_dirty(&mut self, change: &RegistryChange) {
        // Mark new program structures
        for (project, &new_file_names) in &change.rebuilt_programs {
            if let Some(bucket) = self.projects.get_mut(project) {
                bucket.state.new_program_structure = if new_file_names {
                    NewProgramStructure::DifferentFileNames
                } else {
                    NewProgramStructure::SameFileNames
                };
            }
        }
        // For projects, mark the bucket dirty if the bucket contains the file directly.
        // Any other significant change, like a created failed lookup location, is
        // handled by newProgramStructure.
        for paths in [&change.created, &change.deleted, &change.changed] {
            for path in paths {
                for bucket in self.projects.values_mut() {
                    if !bucket.state.multiple_files_dirty && bucket.paths.contains(path) {
                        bucket.mark_project_file_dirty(path);
                    }
                }
            }
        }
    }

    /// tsgo `updateIndexes` for the requested file's project bucket.
    fn update_indexes(&mut self, requested: &str, host: &dyn RegistryCloneHost) {
        let Some(project) = host.default_project(requested) else {
            return;
        };
        let preferences = self.user_preferences.clone();
        let Some(bucket) = self.projects.get_mut(&project) else {
            return;
        };
        let mut should_rebuild = bucket.state.has_dirty_file_besides(requested)
            || !bucket
                .state
                .build_preferences
                .equals(&BuildPreferences::of(&preferences));
        if !should_rebuild && bucket.state.new_program_structure > NewProgramStructure::False {
            // tsgo `hasNewNonNodeModulesFiles` (the resolved package names
            // are the node_modules buckets', L5c).
            let has_new_files = bucket.state.new_program_structure
                == NewProgramStructure::DifferentFileNames
                && host
                    .indexed_file_paths(&project, &preferences)
                    .iter()
                    .any(|path| !bucket.paths.contains(path));
            if has_new_files {
                should_rebuild = true;
            } else {
                bucket.state.new_program_structure = NewProgramStructure::False;
            }
        }
        if should_rebuild {
            if let Some((paths, index)) = host.build_project_bucket(&project, &preferences) {
                *bucket = RegistryBucket {
                    state: BucketState {
                        build_preferences: BuildPreferences::of(&preferences),
                        ..BucketState::default()
                    },
                    paths: Arc::new(paths),
                    index: Arc::new(index),
                };
            }
        }
    }
}

/// The program's files that are no default library (tsgo `isIgnoredFile`;
/// no global typings without ATA) and are not under `node_modules`.
fn non_node_modules_files(state: &CheckerState<'_>) -> Vec<usize> {
    (0..state.binder.file_count())
        .filter(|&file| {
            let facts = state
                .binder
                .file_facts(ProgramFileId::from_raw(file as u32));
            !facts.is_default_library()
                && !contains_lowercase_directory_sequence(
                    &state.binder.source(file).file_name.to_string_lossy(),
                    "/node_modules/",
                )
        })
        .collect()
}

/// The paths of the program's files that are not under `node_modules`
/// (tsgo `hasNewNonNodeModulesFiles` compares them with a bucket's).
pub fn indexed_file_paths(state: &CheckerState<'_>, prepared: &PreparedProgram) -> Vec<String> {
    non_node_modules_files(state)
        .into_iter()
        .map(|file| {
            path_key(
                prepared,
                &state.binder.source(file).file_name.to_string_lossy(),
            )
        })
        .collect()
}

/// tsgo `buildProjectBucket`: the exports of the program's files, indexed,
/// with the paths of the files. A file the preferences exclude, or one the
/// node_modules buckets own (through a link in a `node_modules` directory)
/// is left out.
pub fn build_project_bucket(
    state: &mut CheckerState<'_>,
    prepared: &PreparedProgram,
    preferences: &UserPreferences,
) -> CheckResult<(BTreeSet<String>, Index)> {
    let path_of = |file_name: &str| path_key(prepared, file_name);
    let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
    let file_exclude_patterns = preferences
        .auto_import_file_exclude_patterns
        .as_deref()
        .and_then(|patterns| {
            tsc_program::vfsmatch::SpecMatcher::new(
                patterns,
                "",
                tsc_program::vfsmatch::Usage::Exclude,
                case_sensitive,
            )
        });
    let project_root_path = path_of(&prepared.current_directory().display().to_string_lossy());
    let symlinks = KnownSymlinks::of(prepared, &path_of);
    let mut paths = BTreeSet::new();
    let mut index = Index::default();
    for file in non_node_modules_files(state) {
        let file_name = state
            .binder
            .source(file)
            .file_name
            .to_string_lossy()
            .into_owned();
        if file_exclude_patterns
            .as_ref()
            .is_some_and(|patterns| patterns.match_string(&file_name))
        {
            continue;
        }
        let path = path_of(&file_name);
        // Ordinary node_modules files are owned by node_modules buckets.
        if symlinks.has_symlink_to_node_modules(&path, &project_root_path) {
            continue;
        }
        paths.insert(path);
        let mut extractor = super::Extractor {
            checker: state,
            path_of: &path_of,
            package_name: String::new(),
        };
        for export in extractor.extract_from_file(file)? {
            index.insert_as_words(Arc::new(export));
        }
    }
    Ok((paths, index))
}

/// The program's known links (tsgo `KnownSymlinks`) by real path: each
/// linked file's links, each linked directory's.
struct KnownSymlinks {
    files_by_realpath: BTreeMap<String, Vec<String>>,
    directories_by_realpath: BTreeMap<String, Vec<String>>,
}

impl KnownSymlinks {
    /// tsgo `GetSymlinkCache`'s maps by real path, keyed by path.
    fn of(prepared: &PreparedProgram, path_of: &dyn Fn(&str) -> String) -> Self {
        let facts = tsc_program::discover_symlink_facts(prepared);
        let by_realpath = |pairs: Vec<(tsc_types::JsString, tsc_types::JsString)>| {
            let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for (real, symlink) in pairs {
                let real = path_of(&real.to_string_lossy());
                map.entry(remove_trailing_separator(&real).to_owned())
                    .or_default()
                    .push(symlink.to_string_lossy().into_owned());
            }
            map
        };
        Self {
            files_by_realpath: by_realpath(facts.files),
            directories_by_realpath: by_realpath(facts.directories),
        }
    }

    /// tsgo `hasSymlinkToNodeModules`: whether a file outside the project's
    /// directory, or one of its directories, has a link in a `node_modules`
    /// directory (the node_modules bucket indexes it there).
    fn has_symlink_to_node_modules(&self, file_path: &str, project_root_path: &str) -> bool {
        // Keep files inside this project indexed in project buckets even if they are
        // reachable through a node_modules symlink from elsewhere.
        if contains_path(project_root_path, file_path) {
            return false;
        }
        let in_node_modules = |links: &Vec<String>| {
            links
                .iter()
                .any(|link| contains_lowercase_directory_sequence(link, "/node_modules/"))
        };
        // First check if the file itself has a symlink to node_modules
        if self
            .files_by_realpath
            .get(file_path)
            .is_some_and(in_node_modules)
        {
            return true;
        }
        // Fall back to checking ancestor directories
        let mut directory = file_path;
        loop {
            if self
                .directories_by_realpath
                .get(directory)
                .is_some_and(in_node_modules)
            {
                return true;
            }
            let parent = parent_path(directory);
            if parent == directory {
                return false;
            }
            directory = parent;
        }
    }
}

/// tsgo `ContainsLowercaseDirectorySequence`: the sequence in the path,
/// ignoring the path's case.
fn contains_lowercase_directory_sequence(path: &str, sequence: &str) -> bool {
    path.to_lowercase().contains(sequence)
}

/// tsgo `PathKey.ContainsPath`: `child` is `parent` or inside it.
fn contains_path(parent: &str, child: &str) -> bool {
    if parent.is_empty() {
        return false;
    }
    let root_length = |path: &str| {
        tsc_program::path_root_parts(tsc_types::JsStr::from_str(path))
            .map_or(0, |(root, _)| root.as_bytes().len())
    };
    let (parent_root, parent_rest) = parent.split_at(root_length(parent));
    let (child_root, child_rest) = child.split_at(root_length(child));
    if !parent_root.eq_ignore_ascii_case(child_root) {
        return false;
    }
    parent_rest == child_rest
        || child_rest.len() > parent_rest.len()
            && child_rest.starts_with(parent_rest)
            && (parent_rest.is_empty()
                || parent_rest.ends_with('/')
                || child_rest.as_bytes()[parent_rest.len()] == b'/')
}

/// tsgo `PathKey.Parent`: the path's directory (a root's is itself).
fn parent_path(path: &str) -> &str {
    let root_length = tsc_program::path_root_parts(tsc_types::JsStr::from_str(path))
        .map_or(0, |(root, _)| root.as_bytes().len());
    let path = remove_trailing_separator(path);
    match path[root_length.min(path.len())..].rfind('/') {
        Some(index) => &path[..root_length + index],
        None => &path[..root_length.min(path.len())],
    }
}

/// tsgo `RemoveTrailingDirectorySeparator`.
fn remove_trailing_separator(path: &str) -> &str {
    if path.len() > 1 && path.ends_with('/') {
        &path[..path.len() - 1]
    } else {
        path
    }
}
