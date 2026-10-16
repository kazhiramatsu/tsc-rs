//! tsgo `ls/autoimport/registry.go`: the auto-import registry a snapshot
//! keeps — per project, the index of its program's exports; per directory
//! with a `node_modules`, the index of the exports of the packages there —
//! and how a snapshot's changes make a bucket dirty and a request for a
//! file rebuild it. A bucket stays usable for a request from the only file
//! edited since it was built: an edit inside a file seldom changes what
//! that file can import.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use tsc_checker::program::ProgramFileId;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_host::CompilerHost;
use tsc_program::{ModuleResolver, PreparedProgram, ResolvedEntrypoint};
use tsc_types::JsStr;

use super::alias_resolver::extract_from_resolved_files;
use super::index::Index;
use super::node_modules::{
    discover_bucket_packages, extract_package, get_package_names_in_node_modules,
    install_extractions, resolve_from, DiscoveredPackage, ExtractionInputs, FailedSource,
    PackageExtraction,
};
use super::package_json::{add_package_json_dependencies, PackageJsonInfo};
use crate::lsutil::UserPreferences;
use crate::utilities::path_key;

/// tsgo `knownRecursiveSearchPackages`: packages imported by deep paths often
/// enough that every directory of them is searched for entrypoints.
const KNOWN_RECURSIVE_SEARCH_PACKAGES: [&str; 47] = [
    "@material-ui/core",
    "@material-ui/icons",
    "@sap/cds",
    "@testing-library/react-native",
    "ajv",
    "asap",
    "async",
    "aws-sdk",
    "braintree-web",
    "core-js",
    "core-js-pure",
    "crypto-js",
    "cypress-mochawesome-reporter",
    "dd-trace",
    "dumi",
    "dva",
    "egg-mock",
    "electron-log",
    "es-abstract",
    "es6-promise",
    "eslint-config-taro",
    "expo",
    "expo-router",
    "flow-remove-types",
    "gatsby",
    "glamor",
    "gluegun",
    "graphology-indices",
    "graphology-traversal",
    "graphology-utils",
    "jest-expo",
    "lodash",
    "lodash-es",
    "moment",
    "mz",
    "next",
    "pdfjs-dist",
    "protobufjs",
    "react-app-polyfill",
    "react-dev-utils",
    "react-devtools-inline",
    "recast",
    "semver",
    "stylelint-config-html",
    "umi",
    "web3-provider-engine",
    "webpack",
];

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
pub(super) struct BucketState {
    /// The file edited last, if any (ignored with `multiple_files_dirty`;
    /// node_modules buckets use `dirty_packages`).
    dirty_file: Option<String>,
    multiple_files_dirty: bool,
    new_program_structure: NewProgramStructure,
    build_preferences: BuildPreferences,
    /// tsgo `dirtyPackages`: the packages of a node_modules bucket to
    /// extract again (a local workspace package's file changed).
    dirty_packages: Option<BTreeSet<String>>,
    /// tsgo `recursiveSearchPackages`: the packages whose every directory
    /// the bucket's build searched; none when all were.
    recursive_search_packages: Option<BTreeSet<String>>,
}

impl BucketState {
    /// tsgo `possiblyNeedsRebuildForFile`.
    fn possibly_needs_rebuild_for_file(&self, file: &str, preferences: &UserPreferences) -> bool {
        self.new_program_structure > NewProgramStructure::False
            || self.has_dirty_file_besides(file)
            || !self
                .build_preferences
                .equals(&BuildPreferences::of(preferences))
            || self
                .dirty_packages
                .as_ref()
                .is_some_and(|packages| !packages.is_empty())
    }

    /// tsgo `hasDirtyFileBesides`.
    fn has_dirty_file_besides(&self, file: &str) -> bool {
        self.multiple_files_dirty
            || self
                .dirty_file
                .as_deref()
                .is_some_and(|dirty| dirty != file)
    }

    /// tsgo `DirtyPackages`.
    fn dirty_packages(&self) -> Option<&BTreeSet<String>> {
        if self.multiple_files_dirty {
            return None;
        }
        self.dirty_packages.as_ref()
    }
}

/// tsgo `recursiveSearchSubset`: whether a build that searched `current`
/// (none: every package) covers the packages `target` needs searched.
fn recursive_search_subset(
    target: Option<&BTreeSet<String>>,
    current: Option<&BTreeSet<String>>,
) -> bool {
    match (target, current) {
        // Target wants all packages searched — only satisfied if current is also all.
        (None, current) => current.is_none(),
        // Current searched all packages, so any concrete target is satisfied.
        (Some(_), None) => true,
        (Some(target), Some(current)) => target.is_subset(current),
    }
}

/// tsgo `RegistryBucket`.
#[derive(Clone, Debug, Default)]
pub(crate) struct RegistryBucket {
    pub(super) state: BucketState,
    /// tsgo `Paths`: the paths the bucket indexed with their package names:
    /// in a project bucket every file's (no package); in a node_modules
    /// bucket, the files of its local workspace packages.
    pub(super) paths: Arc<BTreeMap<String, String>>,
    /// tsgo `PackageFiles`: every package directory of a node_modules
    /// bucket, with the paths and names of the files indexed for it (none
    /// when the package was not indexed).
    pub(super) package_files: Arc<BTreeMap<String, Option<BTreeMap<String, String>>>>,
    /// tsgo `ResolvedPackageNames`: the packages a project's program imports.
    pub(super) resolved_package_names: Option<Arc<BTreeSet<String>>>,
    /// tsgo `DependencyNames`: the packages a node_modules bucket includes,
    /// none for every package of the directory.
    pub(super) dependency_names: Option<Arc<BTreeSet<String>>>,
    /// tsgo `AmbientModuleNames`: the files of a node_modules bucket declaring
    /// each ambient module.
    pub(super) ambient_module_names: Arc<BTreeMap<String, Vec<String>>>,
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

    /// tsgo `markNodeModulesDirty`: a package to extract again, or the whole
    /// bucket for none.
    fn mark_node_modules_dirty(&mut self, package_name: &str) {
        if self.state.multiple_files_dirty {
            return;
        }
        if package_name.is_empty() {
            self.state.multiple_files_dirty = true;
            return;
        }
        self.state
            .dirty_packages
            .get_or_insert_with(BTreeSet::new)
            .insert(package_name.to_owned());
    }
}

/// tsgo `directory`: a directory above an open file.
#[derive(Clone, Debug)]
pub(crate) struct Directory {
    pub(super) name: String,
    pub(super) package_json: Arc<PackageJsonInfo>,
    has_node_modules: bool,
}

/// The module specifiers computed for an importing file, by target path
/// (tsgo's `specifierCache` entry; empty for none).
pub(crate) type SpecifierCache = Mutex<HashMap<String, String>>;

/// tsgo `Registry`.
#[derive(Clone, Debug, Default)]
pub struct Registry {
    user_preferences: UserPreferences,
    /// The directories above the open files, by path.
    directories: BTreeMap<String, Directory>,
    /// The node_modules buckets, by the path of the directory holding the
    /// `node_modules` directory.
    node_modules: BTreeMap<String, RegistryBucket>,
    projects: BTreeMap<String, RegistryBucket>,
    unique_package_count: usize,
    /// tsgo `entrypoints`: the entrypoints of the node_modules buckets' files,
    /// by path.
    entrypoints: BTreeMap<String, Vec<Arc<ResolvedEntrypoint>>>,
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

/// What a project's program tells the registry (tsgo's queries of
/// `compiler.Program` in `updateIndexes`).
#[derive(Clone, Debug, Default)]
pub struct ProgramPackageFacts {
    /// tsgo `getResolvedPackageNames`.
    pub resolved_package_names: BTreeSet<String>,
    /// tsgo `DeepImportPackageNames`.
    pub deep_import_package_names: BTreeSet<String>,
    /// tsgo `addProjectReferenceOutputMappings`: each output .d.ts path of
    /// the program's project references, with its source's name.
    pub project_reference_outputs: Vec<(String, String)>,
}

/// What the registry asks of the snapshot it is built in (tsgo
/// `RegistryCloneHost`).
pub trait RegistryCloneHost: Sync {
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
    /// tsgo `GetProgramForProject`'s facts, if the project has a program.
    fn program_package_facts(&self, project: &str) -> Option<ProgramPackageFacts>;
    /// tsgo `FS()`: the snapshot's files, its current directory the
    /// session's.
    fn fs(&self) -> &(dyn CompilerHost + Sync);
    /// tsgo's parse cache: the documents the session's programs share.
    fn documents(&self) -> Option<&tsc_compiler::DocumentRegistry>;
    /// tsgo `ToPath`.
    fn path_of(&self, file_name: &str) -> String;
}

/// A bucket built for a request (tsgo `bucketBuildResult`).
struct BucketBuildResult {
    target: BucketTarget,
    /// The path of the directory the bucket's lookups start from.
    resolution_path: String,
    bucket: RegistryBucket,
    entrypoints: BTreeMap<String, Vec<Arc<ResolvedEntrypoint>>>,
    removed_entrypoint_paths: Vec<String>,
    failed_sources: BTreeMap<String, FailedSource>,
    /// None for a project bucket.
    failed_targets: Option<BTreeSet<String>>,
}

enum BucketTarget {
    NodeModules(String),
    Project(String),
}

/// A node_modules bucket a request builds (tsgo `nodeModulesBucketTask`).
struct NodeModulesTask {
    directory_path: String,
    directory_name: String,
    dependency_names: Option<BTreeSet<String>>,
    /// For granular updates: the packages to extract again.
    dirty_packages: Option<BTreeSet<String>>,
    directory_package_names: BTreeSet<String>,
    discovered: Vec<DiscoveredPackage>,
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
    pub fn is_prepared_for_importing_file(
        &self,
        path: &str,
        project: &str,
        preferences: &UserPreferences,
    ) -> bool {
        let Some(project_bucket) = self.projects.get(project) else {
            return false;
        };
        if project_bucket
            .state
            .possibly_needs_rebuild_for_file(path, preferences)
        {
            return false;
        }
        let mut directory = parent_path(path);
        loop {
            if self.node_modules.get(directory).is_some_and(|bucket| {
                bucket
                    .state
                    .possibly_needs_rebuild_for_file(path, preferences)
            }) {
                return false;
            }
            let parent = parent_path(directory);
            if parent == directory {
                return true;
            }
            directory = parent;
        }
    }

    /// The bucket of a project.
    pub(crate) fn project_bucket(&self, project: &str) -> Option<&RegistryBucket> {
        self.projects.get(project)
    }

    /// The node_modules bucket of the directory at `path`.
    pub(crate) fn node_modules_bucket(&self, path: &str) -> Option<&RegistryBucket> {
        self.node_modules.get(path)
    }

    /// The directory at `path`, if it is above an open file.
    pub(crate) fn directory(&self, path: &str) -> Option<&Directory> {
        self.directories.get(path)
    }

    /// The entrypoints of the node_modules buckets' file at `path`.
    pub(crate) fn entrypoints(&self, path: &str) -> Option<&[Arc<ResolvedEntrypoint>]> {
        self.entrypoints.get(path).map(Vec::as_slice)
    }

    /// The module specifiers computed for an importing file.
    pub(crate) fn specifier_cache(&self, path: &str) -> Option<&Arc<SpecifierCache>> {
        self.specifier_cache.get(path)
    }

    /// tsgo `Clone`: the registry after a snapshot's change, its requested
    /// file's buckets rebuilt when needed.
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
        builder.update_bucket_and_directory_existence(change, host);
        builder.mark_buckets_dirty(change);
        if change.requested_file.is_some() {
            builder.update_indexes(change, host);
        }
        builder
    }

    /// tsgo `updateBucketAndDirectoryExistence`: a project bucket for the
    /// default project of each open file and of the requested file, and a
    /// directory for each directory above them, with a node_modules bucket
    /// where it has a `node_modules` directory.
    fn update_bucket_and_directory_existence(
        &mut self,
        change: &RegistryChange,
        host: &dyn RegistryCloneHost,
    ) {
        let mut needed_projects = BTreeSet::new();
        let mut needed_directories: BTreeMap<String, String> = BTreeMap::new();
        let mut add_needed_directories = |path: &str, file_name: &str| {
            if tsc_program::is_encoded_dynamic_file_name(file_name) {
                return;
            }
            let mut directory_name = tsc_program::get_directory_path(JsStr::from_str(file_name))
                .to_string_lossy()
                .into_owned();
            let mut directory_path = parent_path(path).to_owned();
            loop {
                if needed_directories.contains_key(&directory_path) {
                    break;
                }
                needed_directories.insert(directory_path.clone(), directory_name.clone());
                let parent_path = parent_path(&directory_path).to_owned();
                let parent_name = tsc_program::get_directory_path(JsStr::from_str(&directory_name))
                    .to_string_lossy()
                    .into_owned();
                if parent_path == directory_path || parent_name == directory_name {
                    break;
                }
                directory_path = parent_path;
                directory_name = parent_name;
            }
        };
        for (path, file_name) in &change.open_files {
            if let Some(project) = host.default_project(path) {
                needed_projects.insert(project);
            }
            add_needed_directories(path, file_name);
            self.specifier_cache.entry(path.clone()).or_default();
        }
        if let Some((requested, requested_name)) = &change.requested_file {
            add_needed_directories(requested, requested_name);
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

        let fs = host.fs();
        let node_modules_exists = |directory_name: &str| {
            fs.directory_exists_js(JsStr::from_str(&join(directory_name, "node_modules")))
                .unwrap_or(false)
        };
        let package_json_changed = |directory_name: &str| {
            let path = host.path_of(&join(directory_name, "package.json"));
            change.changed.contains(&path)
                || change.deleted.contains(&path)
                || change.created.contains(&path)
        };
        let removed: Vec<String> = self
            .directories
            .keys()
            .filter(|path| !needed_directories.contains_key(*path))
            .cloned()
            .collect();
        for path in removed {
            // Have and don't need
            self.directories.remove(&path);
            self.node_modules.remove(&path);
        }
        for (path, name) in needed_directories {
            let has_node_modules = node_modules_exists(&name);
            match self.directories.get_mut(&path) {
                Some(directory) => {
                    let package_json_changed = package_json_changed(&name);
                    if !package_json_changed && directory.has_node_modules == has_node_modules {
                        continue;
                    }
                    directory.package_json =
                        Arc::new(PackageJsonInfo::read(fs, &join(&name, "package.json")));
                    directory.has_node_modules = has_node_modules;
                }
                None => {
                    // Need and don't have
                    self.directories.insert(
                        path.clone(),
                        Directory {
                            package_json: Arc::new(PackageJsonInfo::read(
                                fs,
                                &join(&name, "package.json"),
                            )),
                            name,
                            has_node_modules,
                        },
                    );
                }
            }
            if has_node_modules {
                self.node_modules
                    .entry(path)
                    .or_insert_with(RegistryBucket::dirty);
            } else {
                self.node_modules.remove(&path);
            }
        }
    }

    /// tsgo `markBucketsDirty`.
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
        // Mark files dirty, bailing out if all buckets already have multiple files dirty
        let mut clean_node_modules: BTreeSet<String> = self
            .node_modules
            .iter()
            .filter(|(_, bucket)| !bucket.state.multiple_files_dirty)
            .map(|(path, _)| path.clone())
            .collect();
        let mut clean_projects: BTreeSet<String> = self
            .projects
            .iter()
            .filter(|(_, bucket)| !bucket.state.multiple_files_dirty)
            .map(|(project, _)| project.clone())
            .collect();
        for paths in [&change.created, &change.deleted, &change.changed] {
            if clean_node_modules.is_empty() && clean_projects.is_empty() {
                return;
            }
            for path in paths {
                if !clean_node_modules.is_empty() {
                    // For node_modules, mark the bucket dirty if anything changes in the
                    // directory. The path could be either a symlink path (containing
                    // /node_modules/) or a realpath (for symlinked project references).
                    // Both are recorded in Paths for granular updates.
                    if let Some(directory_path) = split_at_node_modules(path) {
                        if clean_node_modules.contains(directory_path) {
                            let bucket = self
                                .node_modules
                                .get_mut(directory_path)
                                .expect("a clean bucket");
                            // Look up the package name for granular updates
                            let package_name = bucket.paths.get(path).cloned().unwrap_or_default();
                            bucket.mark_node_modules_dirty(&package_name);
                            if bucket.state.multiple_files_dirty {
                                clean_node_modules.remove(directory_path);
                            }
                        }
                    } else {
                        // Check if this path (possibly a realpath of a workspace package) is
                        // in any bucket's Paths. This handles local workspace packages where
                        // the realpath doesn't contain /node_modules/.
                        for directory_path in clean_node_modules.clone() {
                            let bucket = self
                                .node_modules
                                .get_mut(&directory_path)
                                .expect("a clean bucket");
                            if let Some(package_name) = bucket.paths.get(path).cloned() {
                                // Use the package name for granular updates
                                bucket.mark_node_modules_dirty(&package_name);
                                if bucket.state.multiple_files_dirty {
                                    clean_node_modules.remove(&directory_path);
                                }
                            }
                        }
                    }
                }
                // For projects, mark the bucket dirty if the bucket contains the file directly.
                // Any other significant change, like a created failed lookup location, is
                // handled by newProgramStructure.
                for project in clean_projects.clone() {
                    let bucket = self.projects.get_mut(&project).expect("a clean bucket");
                    if bucket.paths.contains_key(path) {
                        // Project buckets don't use package-based granular updates
                        bucket.mark_project_file_dirty(path);
                        if bucket.state.multiple_files_dirty {
                            clean_projects.remove(&project);
                        }
                    }
                }
            }
        }
    }

    /// tsgo `updateIndexes`: the requested file's node_modules buckets and
    /// project bucket rebuilt as needed.
    fn update_indexes(&mut self, change: &RegistryChange, host: &dyn RegistryCloneHost) {
        let Some((requested, _)) = &change.requested_file else {
            return;
        };
        let Some(project_id) = host.default_project(requested) else {
            return;
        };
        let fs = host.fs();
        let path_of = |file_name: &str| host.path_of(file_name);
        let current_directory = fs
            .current_directory_js()
            .map(|directory| directory.to_string_lossy().into_owned())
            .unwrap_or_default();
        let preferences = self.user_preferences.clone();

        // Compute resolved package names and project reference output mappings for all
        // projects upfront: a node_modules directory can be used by multiple projects.
        let mut all_resolved_package_names: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut project_reference_outputs: BTreeMap<String, String> = BTreeMap::new();
        // Packages with implicit deep imports (subpath imports in packages without
        // exports) need recursive directory search to discover all auto-importable
        // files, even when the preference is disabled.
        let mut all_deep_import_packages = BTreeSet::new();
        for project in self.projects.keys() {
            if let Some(facts) = host.program_package_facts(project) {
                for (output, source) in facts.project_reference_outputs {
                    // Only add if not already present (first program wins)
                    project_reference_outputs.entry(output).or_insert(source);
                }
                all_deep_import_packages.extend(facts.deep_import_package_names);
                all_resolved_package_names.insert(project.clone(), facts.resolved_package_names);
            }
        }
        let file_exclude_patterns = preferences
            .auto_import_file_exclude_patterns
            .as_deref()
            .and_then(|patterns| {
                tsc_program::vfsmatch::SpecMatcher::new(
                    patterns,
                    "",
                    tsc_program::vfsmatch::Usage::Exclude,
                    fs.use_case_sensitive_file_names(),
                )
            });
        // Which packages need recursive directory search for this build; none
        // means all (the preference is enabled).
        let mut target_recursive_packages = (preferences.auto_import_entrypoint_directory_search
            != Some(true))
        .then_some(all_deep_import_packages);

        // --- Collect node_modules tasks ---
        let mut tasks = Vec::new();
        let mut directory_path = requested.as_str();
        loop {
            if let Some(bucket) = self.node_modules.get(directory_path) {
                let directory_name = self.directories[directory_path].name.clone();
                let dependencies = self.compute_dependencies_for_node_modules_directory(
                    change,
                    &all_resolved_package_names,
                    directory_path,
                );
                let needs_full_rebuild = bucket.state.multiple_files_dirty
                    || bucket.dependency_names.as_deref() != dependencies.as_ref()
                    || !bucket
                        .state
                        .build_preferences
                        .equals(&BuildPreferences::of(&preferences))
                    || !recursive_search_subset(
                        target_recursive_packages.as_ref(),
                        bucket.state.recursive_search_packages.as_ref(),
                    );
                let dirty_packages = bucket.state.dirty_packages().cloned();
                if needs_full_rebuild {
                    tasks.push(NodeModulesTask {
                        directory_path: directory_path.to_owned(),
                        directory_name,
                        dependency_names: dependencies,
                        dirty_packages: None,
                        directory_package_names: BTreeSet::new(),
                        discovered: Vec::new(),
                    });
                } else if dirty_packages
                    .as_ref()
                    .is_some_and(|packages| !packages.is_empty())
                {
                    tasks.push(NodeModulesTask {
                        directory_path: directory_path.to_owned(),
                        directory_name,
                        dependency_names: dependencies,
                        dirty_packages,
                        directory_package_names: BTreeSet::new(),
                        discovered: Vec::new(),
                    });
                }
            }
            let parent = parent_path(directory_path);
            if parent == directory_path {
                break;
            }
            directory_path = parent;
        }

        // --- Phase 1: Discovery ---
        for task in &mut tasks {
            let package_names = match &task.dirty_packages {
                Some(dirty) => dirty.clone(),
                None => {
                    task.directory_package_names = get_package_names_in_node_modules(
                        &join(&task.directory_name, "node_modules"),
                        fs,
                    );
                    task.dependency_names
                        .clone()
                        .unwrap_or_else(|| task.directory_package_names.clone())
                }
            };
            task.discovered = discover_bucket_packages(
                &package_names,
                &task.directory_name,
                fs,
                &current_directory,
                &path_of,
            );
        }

        // --- Phase 2: Extraction (once per real package directory) ---
        // Extract from main packages first. If a main package has no TypeScript
        // entrypoints, fall back to extracting from @types in a second pass. Packages
        // with no main package extract directly from @types in the primary pass.
        let inputs = ExtractionInputs {
            host: fs,
            documents: host.documents(),
            path_of: &path_of,
            project_reference_outputs: &project_reference_outputs,
            file_exclude_patterns: file_exclude_patterns.as_ref(),
        };
        // tsgo extracts each unique package on its work group.
        let workers = tsc_program::WorkerBudget::automatic();
        let extract_all = |jobs: Vec<(String, &PackageJsonInfo, &str, bool)>| {
            workers.map_ordered(
                jobs,
                |_| 1,
                |(realpath, package_json, package_name, enable_directory_search)| {
                    let result = extract_package(
                        &inputs,
                        package_json,
                        package_name,
                        enable_directory_search,
                    );
                    (realpath, result)
                },
            )
        };
        let mut seen = BTreeSet::new();
        let mut extraction_cache: BTreeMap<String, PackageExtraction> = BTreeMap::new();
        let mut types_fallback_candidates = Vec::new();
        let mut jobs = Vec::new();
        for task in &tasks {
            for package in &task.discovered {
                if let Some(realpath) = &package.realpath {
                    if seen.insert(realpath.clone()) {
                        let enable_directory_search = target_recursive_packages
                            .as_ref()
                            .is_none_or(|target| target.contains(&package.package_name))
                            || KNOWN_RECURSIVE_SEARCH_PACKAGES
                                .contains(&package.package_name.as_str());
                        // Record actual directory-searched packages so the stored set
                        // reflects reality for rebuild detection.
                        if enable_directory_search {
                            if let Some(target) = &mut target_recursive_packages {
                                target.insert(package.package_name.clone());
                            }
                        }
                        jobs.push((
                            realpath.clone(),
                            &*package.package_json,
                            package.package_name.as_str(),
                            enable_directory_search,
                        ));
                    }
                    if package.types_realpath.is_some() {
                        types_fallback_candidates.push(package);
                    }
                } else if let (Some(types_realpath), Some(types_package_json)) =
                    (&package.types_realpath, &package.types_package_json)
                {
                    if seen.insert(types_realpath.clone()) {
                        // @types packages always get directory search
                        if let Some(target) = &mut target_recursive_packages {
                            target.insert(package.package_name.clone());
                        }
                        jobs.push((
                            types_realpath.clone(),
                            &**types_package_json,
                            package.package_name.as_str(),
                            true,
                        ));
                    }
                }
            }
        }
        for (realpath, result) in extract_all(jobs) {
            if let Some(result) = result {
                extraction_cache.insert(realpath, result);
            }
        }
        // For packages whose main extraction yielded nothing, fall back to @types.
        let mut jobs = Vec::new();
        for package in types_fallback_candidates {
            let (Some(realpath), Some(types_realpath), Some(types_package_json)) = (
                &package.realpath,
                &package.types_realpath,
                &package.types_package_json,
            ) else {
                continue;
            };
            if extraction_cache.contains_key(realpath) || seen.contains(types_realpath) {
                continue;
            }
            seen.insert(types_realpath.clone());
            // @types fallback packages always get directory search
            if let Some(target) = &mut target_recursive_packages {
                target.insert(package.package_name.clone());
            }
            jobs.push((
                types_realpath.clone(),
                &**types_package_json,
                package.package_name.as_str(),
                true,
            ));
        }
        for (realpath, result) in extract_all(jobs) {
            if let Some(result) = result {
                extraction_cache.insert(realpath, result);
            }
        }
        self.unique_package_count = seen.len();

        // --- Phase 3: Bucket building ---
        let mut results = Vec::new();
        for task in &tasks {
            let result = if let Some(dirty_packages) = &task.dirty_packages {
                self.update_node_modules_bucket(
                    task,
                    dirty_packages,
                    &extraction_cache,
                    target_recursive_packages.as_ref(),
                    &preferences,
                    &path_of,
                )
            } else {
                self.build_node_modules_bucket(
                    task,
                    &extraction_cache,
                    target_recursive_packages.as_ref(),
                    &preferences,
                )
            };
            results.push(result);
        }

        // Project bucket (no cross-bucket dedup needed).
        if let Some(bucket) = self.projects.get_mut(&project_id) {
            let resolved_package_names = all_resolved_package_names.get(&project_id);
            let mut should_rebuild = bucket.state.has_dirty_file_besides(requested)
                || !bucket
                    .state
                    .build_preferences
                    .equals(&BuildPreferences::of(&preferences));
            if !should_rebuild && bucket.state.new_program_structure > NewProgramStructure::False {
                // tsgo `hasNewNonNodeModulesFiles`.
                let has_new_files = bucket.state.new_program_structure
                    == NewProgramStructure::DifferentFileNames
                    && host
                        .indexed_file_paths(&project_id, &preferences)
                        .iter()
                        .any(|path| !bucket.paths.contains_key(path));
                if bucket.resolved_package_names.as_deref() != resolved_package_names
                    || has_new_files
                {
                    should_rebuild = true;
                } else {
                    bucket.state.new_program_structure = NewProgramStructure::False;
                }
            }
            if should_rebuild {
                if let Some((paths, index)) = host.build_project_bucket(&project_id, &preferences) {
                    results.push(BucketBuildResult {
                        target: BucketTarget::Project(project_id.clone()),
                        resolution_path: path_of(&current_directory),
                        bucket: RegistryBucket {
                            state: BucketState {
                                build_preferences: BuildPreferences::of(&preferences),
                                ..BucketState::default()
                            },
                            paths: Arc::new(
                                paths
                                    .into_iter()
                                    .map(|path| (path, String::new()))
                                    .collect(),
                            ),
                            resolved_package_names: resolved_package_names
                                .map(|names| Arc::new(names.clone())),
                            index: Arc::new(index),
                            ..RegistryBucket::default()
                        },
                        entrypoints: BTreeMap::new(),
                        removed_entrypoint_paths: Vec::new(),
                        failed_sources: BTreeMap::new(),
                        failed_targets: None,
                    });
                }
            }
        }

        let mut second_pass = Vec::new();
        for result in results {
            for path in &result.removed_entrypoint_paths {
                self.entrypoints.remove(path);
            }
            self.entrypoints.extend(result.entrypoints);
            match result.target {
                BucketTarget::NodeModules(directory_path) => {
                    if let Some(failed_targets) = result.failed_targets {
                        second_pass.push((
                            directory_path.clone(),
                            result.resolution_path,
                            result.failed_sources,
                            failed_targets,
                        ));
                    }
                    self.node_modules.insert(directory_path, result.bucket);
                }
                BucketTarget::Project(project) => {
                    self.projects.insert(project, result.bucket);
                }
            }
        }

        // If we failed to resolve any alias exports by ending up at a non-relative module
        // specifier that didn't resolve to another package, it's probably an ambient module
        // declared in another package. We recorded these failures, along with the name of
        // every ambient module declared elsewhere, so we can do a second pass on the failed
        // files, this time including the ambient modules declarations that were missing the
        // first time. Example: node_modules/fs-extra/index.d.ts is simply `export * from "fs"`,
        // but when trying to resolve the `export *`, we don't know where "fs" is declared.
        for (directory_path, resolution_path, failed_sources, failed_targets) in second_pass {
            let mut root_files: Vec<String> = Vec::new();
            for target in &failed_targets {
                for file_name in self.resolve_ambient_module_name(target, &resolution_path) {
                    if !root_files.contains(&file_name) {
                        root_files.push(file_name);
                    }
                }
            }
            if root_files.is_empty() {
                continue;
            }
            let options = tsc_program::CompilerOptions::default();
            let Ok(mut resolver) = ModuleResolver::new(fs, &options) else {
                continue;
            };
            let source_file_names: Vec<String> = failed_sources
                .values()
                .map(|source| source.file_name.clone())
                .collect();
            let realpath = |file_name: &str| super::node_modules::realpath(fs, file_name);
            let Some((_, exports)) = extract_from_resolved_files(
                fs,
                host.documents(),
                &mut resolver,
                &root_files,
                &source_file_names,
                &path_of,
                /* record_failures */ false,
                |alias, resolver| {
                    let mut exports = Vec::new();
                    for (path, source) in &failed_sources {
                        let Some(index) = alias.files.iter().position(|(_, file)| file == path)
                        else {
                            continue;
                        };
                        let mut resolve_module = |name: &str, containing_file: &str| {
                            resolve_from(resolver, name, containing_file)
                        };
                        let extracted = alias.with_checker(|checker| {
                            let mut extractor = super::Extractor {
                                checker,
                                path_of: &path_of,
                                package_name: source.package_name.clone(),
                                realpath: Some(&realpath),
                                resolve_module: Some(&mut resolve_module),
                            };
                            extractor.extract_from_file(index).unwrap_or_default()
                        });
                        exports.extend(extracted.unwrap_or_default());
                    }
                    exports
                },
            ) else {
                continue;
            };
            if let Some(bucket) = self.node_modules.get_mut(&directory_path) {
                let index = Arc::make_mut(&mut bucket.index);
                for export in exports {
                    index.insert_as_words(Arc::new(export));
                }
            }
        }
    }

    /// tsgo `computeDependenciesForNodeModulesDirectory`: the packages of the
    /// directory a bucket includes — the dependencies of the package.json
    /// files under it and the packages the programs import — or none for
    /// every package, when an open file under it has no package.json.
    fn compute_dependencies_for_node_modules_directory(
        &self,
        change: &RegistryChange,
        all_resolved_package_names: &BTreeMap<String, BTreeSet<String>>,
        directory_path: &str,
    ) -> Option<BTreeSet<String>> {
        // If any open files are in scope of this directory but not in scope of any
        // package.json, we need to add all packages in this node_modules directory.
        for path in change.open_files.keys() {
            if contains_path(directory_path, path)
                && self
                    .get_nearest_ancestor_directory_with_package_json(path)
                    .is_none()
            {
                return None;
            }
        }
        // Get all package.jsons that have this node_modules directory in their spine
        let mut dependencies = BTreeSet::new();
        for (path, directory) in &self.directories {
            if contains_path(directory_path, path) {
                if let Some(contents) = &directory.package_json.contents {
                    add_package_json_dependencies(contents, &mut dependencies);
                }
            }
        }
        // Add packages that are directly imported by programs but not listed in
        // package.json, so node_modules files are always in node_modules buckets.
        for names in all_resolved_package_names.values() {
            dependencies.extend(names.iter().cloned());
        }
        Some(dependencies)
    }

    /// tsgo `getNearestAncestorDirectoryWithPackageJson`.
    fn get_nearest_ancestor_directory_with_package_json(
        &self,
        file_path: &str,
    ) -> Option<&Directory> {
        let mut directory_path = parent_path(file_path);
        loop {
            if let Some(directory) = self
                .directories
                .get(directory_path)
                .filter(|directory| directory.package_json.exists())
            {
                return Some(directory);
            }
            let parent = parent_path(directory_path);
            if parent == directory_path {
                return None;
            }
            directory_path = parent;
        }
    }

    /// tsgo `resolveAmbientModuleName`: the files of the nearest node_modules
    /// bucket at or above `from_path` declaring the ambient module.
    fn resolve_ambient_module_name(&self, module_name: &str, from_path: &str) -> Vec<String> {
        let mut directory_path = from_path;
        loop {
            if let Some(file_names) = self
                .node_modules
                .get(directory_path)
                .and_then(|bucket| bucket.ambient_module_names.get(module_name))
            {
                return file_names.clone();
            }
            let parent = parent_path(directory_path);
            if parent == directory_path {
                return Vec::new();
            }
            directory_path = parent;
        }
    }

    /// tsgo `buildNodeModulesBucket`.
    fn build_node_modules_bucket(
        &self,
        task: &NodeModulesTask,
        extraction_cache: &BTreeMap<String, PackageExtraction>,
        recursive_search_packages: Option<&BTreeSet<String>>,
        preferences: &UserPreferences,
    ) -> BucketBuildResult {
        let extraction = install_extractions(&task.discovered, extraction_cache);
        // PackageFiles with all directory package names; indexed packages have
        // their files, unindexed packages none.
        let package_files = task
            .directory_package_names
            .iter()
            .map(|name| (name.clone(), extraction.package_files.get(name).cloned()))
            .collect();
        // Paths as reverse mapping from path to package name, only for local
        // workspace packages (eligible for granular updates).
        let mut paths = BTreeMap::new();
        for package_name in &extraction.workspace_packages {
            if let Some(files) = extraction.package_files.get(package_name) {
                for path in files.keys() {
                    paths.insert(path.clone(), package_name.clone());
                }
            }
        }
        let mut index = Index::default();
        for exports in extraction.exports.values() {
            for export in exports {
                index.insert_as_words(Arc::new(export.clone()));
            }
        }
        let entrypoints = entrypoints_by_path(&extraction.entrypoints);
        // For a full rebuild, all entrypoints belonging to the old bucket's packages
        // must be removed from the registry-level map.
        let mut removed_entrypoint_paths = Vec::new();
        if let Some(old) = self.node_modules.get(&task.directory_path) {
            for files in old.package_files.values().flatten() {
                for path in files.keys() {
                    if self.entrypoints.contains_key(path) {
                        removed_entrypoint_paths.push(path.clone());
                    }
                }
            }
        }
        BucketBuildResult {
            target: BucketTarget::NodeModules(task.directory_path.clone()),
            resolution_path: task.directory_path.clone(),
            bucket: RegistryBucket {
                state: BucketState {
                    build_preferences: BuildPreferences::of(preferences),
                    recursive_search_packages: recursive_search_packages.cloned(),
                    ..BucketState::default()
                },
                paths: Arc::new(paths),
                package_files: Arc::new(package_files),
                resolved_package_names: None,
                dependency_names: task.dependency_names.clone().map(Arc::new),
                ambient_module_names: Arc::new(extraction.ambient_module_names),
                index: Arc::new(index),
            },
            entrypoints,
            removed_entrypoint_paths,
            failed_sources: extraction.failed_sources,
            failed_targets: Some(extraction.failed_targets),
        }
    }

    /// tsgo `updateNodeModulesBucket`: a bucket with its dirty packages
    /// extracted again.
    fn update_node_modules_bucket(
        &self,
        task: &NodeModulesTask,
        dirty_packages: &BTreeSet<String>,
        extraction_cache: &BTreeMap<String, PackageExtraction>,
        recursive_search_packages: Option<&BTreeSet<String>>,
        preferences: &UserPreferences,
        path_of: &dyn Fn(&str) -> String,
    ) -> BucketBuildResult {
        let existing = &self.node_modules[&task.directory_path];
        let extraction = install_extractions(&task.discovered, extraction_cache);
        // Clone the existing index, excluding exports from dirty packages
        let mut index = existing
            .index
            .clone_filtered(|export| !dirty_packages.contains(&export.package_name));
        // Clone PackageFiles, removing dirty packages, adding the new extraction
        let mut package_files = (*existing.package_files).clone();
        for package_name in dirty_packages {
            package_files.remove(package_name);
        }
        for (package_name, files) in &extraction.package_files {
            package_files.insert(package_name.clone(), Some(files.clone()));
        }
        // Clone Paths, removing dirty package paths
        let mut paths: BTreeMap<String, String> = existing
            .paths
            .iter()
            .filter(|(_, package_name)| !dirty_packages.contains(*package_name))
            .map(|(path, package_name)| (path.clone(), package_name.clone()))
            .collect();
        // Add paths for newly extracted workspace packages
        for package_name in &extraction.workspace_packages {
            if let Some(files) = extraction.package_files.get(package_name) {
                for path in files.keys() {
                    paths.insert(path.clone(), package_name.clone());
                }
            }
        }
        // Clone AmbientModuleNames, removing dirty package entries
        let mut ambient_module_names: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (module_name, file_names) in existing.ambient_module_names.iter() {
            let filtered: Vec<String> = file_names
                .iter()
                .filter(|file_name| {
                    !existing
                        .paths
                        .get(&path_of(file_name))
                        .is_some_and(|package_name| dirty_packages.contains(package_name))
                })
                .cloned()
                .collect();
            if !filtered.is_empty() {
                ambient_module_names.insert(module_name.clone(), filtered);
            }
        }
        for (module_name, file_names) in &extraction.ambient_module_names {
            ambient_module_names
                .entry(module_name.clone())
                .or_default()
                .extend(file_names.iter().cloned());
        }
        // Entrypoint paths to remove from the registry-level map (paths belonging
        // to dirty packages)
        let removed_entrypoint_paths = self
            .entrypoints
            .keys()
            .filter(|path| {
                existing
                    .paths
                    .get(*path)
                    .is_some_and(|package_name| dirty_packages.contains(package_name))
            })
            .cloned()
            .collect();
        // Insert newly extracted exports into the index
        for exports in extraction.exports.values() {
            for export in exports {
                index.insert_as_words(Arc::new(export.clone()));
            }
        }
        BucketBuildResult {
            target: BucketTarget::NodeModules(task.directory_path.clone()),
            resolution_path: task.directory_path.clone(),
            bucket: RegistryBucket {
                state: BucketState {
                    build_preferences: BuildPreferences::of(preferences),
                    recursive_search_packages: recursive_search_packages.cloned(),
                    ..BucketState::default()
                },
                paths: Arc::new(paths),
                package_files: Arc::new(package_files),
                resolved_package_names: None,
                dependency_names: existing.dependency_names.clone(),
                ambient_module_names: Arc::new(ambient_module_names),
                index: Arc::new(index),
            },
            entrypoints: entrypoints_by_path(&extraction.entrypoints),
            removed_entrypoint_paths,
            failed_sources: extraction.failed_sources,
            failed_targets: Some(extraction.failed_targets),
        }
    }
}

/// The entrypoints of a bucket's packages by the path of their files.
fn entrypoints_by_path(
    sets: &[Vec<Arc<ResolvedEntrypoint>>],
) -> BTreeMap<String, Vec<Arc<ResolvedEntrypoint>>> {
    let mut entrypoints: BTreeMap<String, Vec<Arc<ResolvedEntrypoint>>> = BTreeMap::new();
    for set in sets {
        for entrypoint in set {
            entrypoints
                .entry(entrypoint.resolved_path.clone())
                .or_default()
                .push(Arc::clone(entrypoint));
        }
    }
    entrypoints
}

/// The program's files the registry indexes: not a default library, not a
/// content mapper's supplemental output (tsgo `IsContentMapperSupplemental`:
/// no import's target), not under `node_modules` (tsgo
/// `hasNewNonNodeModulesFiles`).
fn non_node_modules_files(state: &CheckerState<'_>) -> Vec<usize> {
    registry_files(state)
        .filter(|&file| {
            !contains_lowercase_directory_sequence(
                &state.binder.source(file).file_name.to_string_lossy(),
                "/node_modules/",
            )
        })
        .collect()
}

/// The program's files but the default libraries and the content mappers'
/// supplemental outputs (tsgo `IsContentMapperSupplemental` and
/// `isIgnoredFile`).
fn registry_files<'s>(state: &'s CheckerState<'_>) -> impl Iterator<Item = usize> + 's {
    (0..state.binder.file_count()).filter(|&file| {
        let facts = state
            .binder
            .file_facts(ProgramFileId::from_raw(file as u32));
        let supplemental = state
            .binder
            .source(file)
            .content_mapper
            .as_ref()
            .is_some_and(|output| output.canonical.is_some());
        !facts.is_default_library() && !supplemental
    })
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
/// is left out. `fs` is the snapshot's files: a relative module
/// augmentation's name is resolved there with the default options.
pub fn build_project_bucket(
    state: &mut CheckerState<'_>,
    prepared: &PreparedProgram,
    preferences: &UserPreferences,
    fs: &dyn CompilerHost,
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
    let options = tsc_program::CompilerOptions::default();
    let mut resolver = ModuleResolver::new(fs, &options).ok();
    let mut resolve_module = |name: &str, containing_file: &str| {
        resolver
            .as_mut()
            .and_then(|resolver| resolve_from(resolver, name, containing_file))
    };
    let mut paths = BTreeSet::new();
    let mut index = Index::default();
    for file in registry_files(state).collect::<Vec<_>>() {
        let source = state.binder.source(file);
        let file_name = source.file_name.to_string_lossy().into_owned();
        if file_exclude_patterns
            .as_ref()
            .is_some_and(|patterns| patterns.match_string(&file_name))
        {
            continue;
        }
        let path = path_of(&file_name);
        // Ordinary node_modules files are owned by node_modules buckets. Content-mapped files are not
        // discovered by those buckets, but files already transformed in the Program can be indexed here.
        if source.content_mapper.is_none()
            && (contains_lowercase_directory_sequence(&file_name, "/node_modules/")
                || symlinks.has_symlink_to_node_modules(&path, &project_root_path))
        {
            continue;
        }
        paths.insert(path);
        let mut extractor = super::Extractor {
            checker: state,
            path_of: &path_of,
            package_name: String::new(),
            realpath: None,
            resolve_module: Some(&mut resolve_module),
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
pub(super) fn contains_lowercase_directory_sequence(path: &str, sequence: &str) -> bool {
    path.to_lowercase().contains(sequence)
}

/// tsgo `PathKey.ContainsPath`: `child` is `parent` or inside it.
pub(super) fn contains_path(parent: &str, child: &str) -> bool {
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
pub(super) fn parent_path(path: &str) -> &str {
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

/// tsgo `PathKey.SplitAtCanonicalComponent("node_modules")`: the path of the
/// directory holding the first `node_modules` component of a path.
fn split_at_node_modules(path: &str) -> Option<&str> {
    let root_length = tsc_program::path_root_parts(tsc_types::JsStr::from_str(path))
        .map_or(0, |(root, _)| root.as_bytes().len());
    if root_length == 0 {
        return None;
    }
    const NEEDLE: &str = "/node_modules";
    let mut offset = root_length - 1;
    loop {
        let index = offset + path[offset..].find(NEEDLE)?;
        let end = index + NEEDLE.len();
        if end == path.len() || path.as_bytes()[end] == b'/' {
            return Some(&path[..index.max(root_length)]);
        }
        offset = end;
    }
}

/// tsgo `RootedDirectoryPath.ResolveFile`/`ResolveDirectory`: a name in a
/// directory.
pub(super) fn join(directory: &str, name: &str) -> String {
    if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}
