//! Snapshots and their host (tsgo `SnapshotHost` and `Snapshot`,
//! snapshothost.go and snapshot.go).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tsc_compiler::DocumentRegistry;
use tsc_diagnostics::Diagnostic;
use tsc_host::vfs::FileSystem;
use tsc_program::content_mapper::host::{Host as ContentMapperHost, Spawner};
use tsc_program::content_mapper::source::ContentMappedCache;
use tsc_program::content_mapper::Mapper;
use tsc_program::{
    ConfigOptionBag, ConfigParseError, ConfigProjectReference, ConfigRootPlan, LibraryCatalog,
    ProgramLoadLimits,
};

use crate::builder::ProjectCollectionBuilder;
use crate::config::{ConfigFileRegistry, ConfigFileRegistryBuilder};
use crate::fs::{FileChangeSummary, Paths, SnapshotFs, SnapshotFsBuilder};
use crate::id::{ProjectId, ProjectKind};
use crate::overlay::{Overlay, OverlayLayer, Overlays};
use crate::project::{default_project_from_program_inclusion, ProgramUpdateKind, Project};
use tsc_ls::autoimport::{ProgramPackageFacts, Registry, RegistryChange, RegistryCloneHost};
use tsc_ls::lsconv::{Converters, LspLineMap};
use tsc_ls::lsutil::UserPreferences;
use tsc_lsproto::PositionEncodingKind;

/// What a session's projects are built with (tsgo `SessionOptions`).
#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub current_directory: String,
    /// The default libraries (tsgo `DefaultLibraryPath`).
    pub library_catalog: LibraryCatalog,
    pub load_limits: ProgramLoadLimits,
    /// tsgo `RunExternalCode`: the configs' content mappers may run their
    /// processes (the API server's `--runExternalCode`).
    pub run_external_code: bool,
    /// tsgo `PositionEncoding`: how the editor counts characters.
    pub position_encoding: tsc_lsproto::PositionEncodingKind,
    /// tsgo `WatchEnabled`: the session registers file watchers.
    pub watch_enabled: bool,
    /// tsgo `LoggingEnabled`.
    pub logging_enabled: bool,
    /// tsgo `PushDiagnosticsEnabled`: config errors are published.
    pub push_diagnostics_enabled: bool,
    /// tsgo `DebounceDelay`: how long a scheduled update waits for more
    /// events.
    pub debounce_delay: std::time::Duration,
    /// The client's capabilities (tsgo's `GetClientCapabilities` of the
    /// session's background context): relative watch patterns, the
    /// diagnostics the client takes.
    pub client_capabilities: tsc_lsproto::ResolvedClientCapabilities,
}

impl SessionOptions {
    /// tsgo's `relativePatternSupport` of watched files.
    pub(crate) fn relative_pattern_support(&self) -> bool {
        self.client_capabilities
            .workspace
            .did_change_watched_files
            .relative_pattern_support
    }
}

/// A failed snapshot update (tsgo's API errors), with tsgo's message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectError {
    message: String,
}

impl ProjectError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProjectError {}

/// A referenced config's path that the reference graph cannot name.
impl From<ConfigParseError> for ProjectError {
    fn from(error: ConfigParseError) -> Self {
        Self::new(format!("cannot resolve the project references: {error}"))
    }
}

/// A synthetic program to create (tsgo `APICreateProgramRequest`).
#[derive(Clone, Debug, Default)]
pub struct CreateProgramRequest {
    /// Absolute file names, in order.
    pub root_file_names: Vec<String>,
    /// tsgo `CompilerOptions`, in the form tsgo's API takes them
    /// ([`tsc_program::go_json::compiler_options_bag`]).
    pub options: ConfigOptionBag,
    pub config_file_parsing_diagnostics: Vec<Diagnostic>,
    /// The projects the program references (tsgo `ProjectReferences`), by
    /// config path.
    pub project_references: Vec<ConfigProjectReference>,
    /// The API's module resolution for the program (tsgo
    /// `CreateProgramOptions.ModuleResolver`).
    pub module_resolution: Option<tsc_program::ModuleResolutionOverrideHandle>,
}

/// A synthetic program to configure again (tsgo
/// `APIReconfigureProgramRequest`).
#[derive(Clone, Debug)]
pub struct ReconfigureProgramRequest {
    pub program_id: ProjectId,
    pub program: CreateProgramRequest,
}

/// What an API request asks of a snapshot update (tsgo
/// `APISnapshotRequest`).
#[derive(Clone, Default)]
pub struct ApiSnapshotRequest {
    /// Config file names (absolute) whose projects to open.
    pub open_projects: BTreeSet<String>,
    /// Config file names (absolute) whose projects to close.
    pub close_projects: BTreeSet<String>,
    pub create_programs: Vec<CreateProgramRequest>,
    pub reconfigure_programs: Vec<ReconfigureProgramRequest>,
    pub remove_programs: BTreeSet<ProjectId>,
    /// Projects whose programs to bring up to date (never created here).
    pub ensure_programs: BTreeSet<ProjectId>,
    pub ensure_all_programs: bool,
    /// Files (absolute names) to open, each placed in its default project
    /// (tsgo `OpenFiles`). An empty set, unlike none, still asks for the
    /// clean-up an open does.
    pub open_files: Option<BTreeSet<String>>,
    /// Files (absolute names) to close (tsgo `CloseFiles`).
    pub close_files: Option<BTreeSet<String>>,
    /// Files (absolute names) whose default project to bring up to date,
    /// created when missing (tsgo `EnsureFiles`; the API ensures every file
    /// it opens).
    pub ensure_files: BTreeSet<String>,
    /// A file system that replaces the host's for this snapshot and its
    /// updates (tsgo's request file system).
    pub file_system: Option<Arc<dyn FileSystem>>,
    /// The file system replaces everything read before.
    pub replace_file_system: bool,
    /// The language service's preferences from now on (tsgo
    /// `UserPreferences`, the snapshot's `newConfig`).
    pub user_preferences: Option<UserPreferences>,
}

/// What a snapshot update takes (tsgo `SnapshotChange` with the overlays
/// `Clone` gets).
#[derive(Clone, Default)]
pub struct SnapshotChange<'a> {
    pub file_changes: FileChangeSummary,
    /// The open documents; the base snapshot's when none.
    pub overlays: Option<Overlays>,
    /// tsgo `ResourceRequest.Documents`: files (absolute names) whose default
    /// project to bring up to date, an inferred one for a file without.
    pub documents: Vec<String>,
    /// tsgo `ConfiguredProjectDocuments`: files whose configured projects
    /// to load, if they have one.
    pub configured_project_documents: Vec<String>,
    /// tsgo `newConfig`: the user's preferences, when they changed.
    pub new_config: Option<UserPreferences>,
    pub api_request: Option<&'a ApiSnapshotRequest>,
    /// tsgo `cleanFileCache`: the cached files no project's build looked at
    /// are dropped (the session's idle clean).
    pub clean_file_cache: bool,
    /// tsgo `compilerOptionsForInferredProjects`: the inferred projects'
    /// options, when they changed.
    pub compiler_options_for_inferred_projects: Option<Arc<ConfigOptionBag>>,
    /// tsgo `ResourceRequest.AutoImports`: the file whose auto-imports to
    /// prepare.
    pub auto_imports: Option<String>,
    /// tsgo `ResourceRequest.Projects`: projects whose programs to bring up
    /// to date.
    pub projects: Vec<ProjectId>,
    /// tsgo `ResourceRequest.ProjectTree`: project trees to load.
    pub project_tree: Option<ProjectTreeRequest>,
    /// tsgo `contentMapperContributions`: the editor's mappers for inferred
    /// projects, when they changed.
    pub content_mapper_contributions: Option<ContentMapperContributions>,
}

/// tsgo `ContentMapperContributions`: the content mappers an editor's
/// extensions contribute for inferred projects (configured projects never
/// take them), and the extensions they claim.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContentMapperContributions {
    pub mappers: Vec<Arc<Mapper>>,
    pub extensions: Vec<String>,
}

/// tsgo `ProjectTreeRequest`: the project trees to load — every one, or the
/// trees of the projects that may reference some projects.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProjectTreeRequest {
    /// tsgo `referencedProjects`: the referenced projects' config paths;
    /// none for every tree.
    referenced_projects: Option<BTreeSet<String>>,
}

impl ProjectTreeRequest {
    /// Every project tree.
    pub fn all() -> Self {
        Self::default()
    }

    /// The trees of the projects that may reference these projects (by
    /// their IDs, the config paths of configured ones).
    pub fn referencing(projects: BTreeSet<String>) -> Self {
        Self {
            referenced_projects: Some(projects),
        }
    }

    /// tsgo `IsAllProjects`.
    pub(crate) fn is_all_projects(&self) -> bool {
        self.referenced_projects.is_none()
    }

    /// tsgo `IsProjectReferenced`.
    pub(crate) fn is_project_referenced(&self, path: &str) -> bool {
        self.referenced_projects
            .as_ref()
            .is_some_and(|projects| projects.contains(path))
    }
}

/// The API's open state (tsgo `APIState`): the projects and files opened
/// through the API, with their open counts, by path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ApiState {
    pub(crate) open_projects: BTreeMap<String, usize>,
    pub(crate) open_files: BTreeMap<String, OpenedFile>,
}

/// A file opened through the API (tsgo `apiOpenedFile`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct OpenedFile {
    pub(crate) file_name: String,
    pub(crate) ref_count: usize,
}

type Projects = Arc<BTreeMap<ProjectId, Arc<Project>>>;

/// A snapshot's projects (tsgo `ProjectCollection`).
#[derive(Clone, Default)]
pub(crate) struct ProjectCollection {
    pub(crate) configured: Projects,
    pub(crate) synthetic: Projects,
    pub(crate) inferred: Option<Arc<Project>>,
    /// tsgo `openFiles`: the paths of the editor's open files.
    pub(crate) open_files: BTreeSet<String>,
    /// tsgo `fileDefaultProjects`: the default projects the build that made
    /// the snapshot found, by file path.
    pub(crate) file_default_projects: BTreeMap<String, ProjectId>,
    pub(crate) api_state: ApiState,
}

impl ProjectCollection {
    /// tsgo `ProjectCollection.GetDefaultProject`: the default project the
    /// build found, else the configured project (by ID) whose program has
    /// the file, preferring one that has it directly; several that have it
    /// directly are settled by the open file's config names, else the first;
    /// else the inferred project when it has the file.
    pub(crate) fn default_project(
        &self,
        path: &str,
        configs: &ConfigFileRegistry,
        paths: &Paths,
    ) -> Option<&Arc<Project>> {
        if let Some(id) = self.file_default_projects.get(path) {
            return match id.kind() {
                Some(ProjectKind::Inferred) => self.inferred.as_ref(),
                _ => self.configured.get(id),
            };
        }
        let (project, several_direct) =
            default_project_from_program_inclusion(self.configured.values(), path);
        if several_direct {
            if let Some(project) = self.find_default_configured_project(path, configs, paths) {
                return Some(project);
            }
        }
        project.or_else(|| {
            self.inferred
                .as_ref()
                .filter(|project| project.contains_file(path))
        })
    }

    /// tsgo `ProjectCollection.findDefaultConfiguredProject`: from an open
    /// file's nearest config, a breadth-first search through the loaded
    /// projects' references for one that has the file directly, then from
    /// the configs above.
    fn find_default_configured_project(
        &self,
        path: &str,
        configs: &ConfigFileRegistry,
        paths: &Paths,
    ) -> Option<&Arc<Project>> {
        let config_file_name = configs.config_file_name(path)?;
        let mut visited = BTreeSet::new();
        self.find_default_configured_project_worker(
            path,
            config_file_name,
            &mut visited,
            None,
            configs,
            paths,
        )
    }

    fn find_default_configured_project_worker<'c>(
        &'c self,
        path: &str,
        config_file_name: &str,
        visited: &mut BTreeSet<ProjectId>,
        fallback: Option<&'c Arc<Project>>,
        configs: &ConfigFileRegistry,
        paths: &Paths,
    ) -> Option<&'c Arc<Project>> {
        let start = ProjectId::configured(&paths.to_path(config_file_name))
            .and_then(|id| self.configured.get(&id))?;
        let mut level = vec![start];
        let mut first_result = None;
        while !level.is_empty() {
            let mut next = Vec::new();
            for project in level {
                if !visited.insert(project.id.clone()) {
                    continue;
                }
                if project.contains_file(path) {
                    if !project.is_source_from_project_reference(path) {
                        return Some(project);
                    }
                    first_result = first_result.or(Some(project));
                }
                // A referenced project is searched only when it is loaded
                // (`disableReferencedProjectLoad` may leave it out).
                if let Some(command_line) = &project.command_line {
                    for reference in command_line.project_references() {
                        let name = tsc_program::resolve_config_file_name_of_project_reference(
                            reference.path.as_js(),
                        )
                        .to_string_lossy()
                        .into_owned();
                        if let Some(referenced) = ProjectId::configured(&paths.to_path(&name))
                            .and_then(|id| self.configured.get(&id))
                        {
                            next.push(referenced);
                        }
                    }
                }
            }
            level = next;
        }
        let fallback = fallback.or(first_result);
        // tsgo looks the option up on the open file's own path, which holds
        // no config.
        if configs.config(path).is_some_and(|config| {
            config.options().option_bool("disableSolutionSearching") == Some(true)
        }) {
            return fallback;
        }
        match configs.ancestor_config_file_name(path, config_file_name) {
            Some(ancestor) => self.find_default_configured_project_worker(
                path, ancestor, visited, fallback, configs, paths,
            ),
            None => fallback,
        }
    }
}

/// tsgo `autoImportRegistryCloneHost`: the new snapshot's projects and
/// files for the registry's update.
struct AutoImportHost<'a> {
    projects: &'a ProjectCollection,
    configs: &'a ConfigFileRegistry,
    paths: &'a Paths,
    /// tsgo `autoImportBuilderFS`: the snapshot build's files.
    fs: tsc_host::vfs::VfsCompilerHost<crate::fs::SourceFs<'a>>,
}

impl AutoImportHost<'_> {
    fn project(&self, id: &str) -> Option<&Arc<Project>> {
        self.projects
            .configured
            .values()
            .chain(self.projects.synthetic.values())
            .chain(self.projects.inferred.iter())
            .find(|project| project.id.as_str() == id)
    }
}

impl RegistryCloneHost for AutoImportHost<'_> {
    fn default_project(&self, path: &str) -> Option<String> {
        self.projects
            .default_project(path, self.configs, self.paths)
            .map(|project| project.id.as_str().to_owned())
    }

    fn build_project_bucket(
        &self,
        project: &str,
        preferences: &UserPreferences,
    ) -> Option<(BTreeSet<String>, tsc_ls::autoimport::Index)> {
        let program = self.project(project)?.program.as_ref()?;
        program
            .with_live(|live| {
                live.with_query_checker(|state| {
                    tsc_ls::autoimport::build_project_bucket(
                        state,
                        program.prepared(),
                        preferences,
                        &self.fs,
                    )
                })
            })?
            .ok()
    }

    fn program_package_facts(&self, project: &str) -> Option<ProgramPackageFacts> {
        let program = self.project(project)?.program.as_ref()?;
        program.with_live(|live| {
            live.with_query_checker(|state| {
                tsc_ls::autoimport::program_package_facts(state, program.prepared())
            })
        })
    }

    fn fs(&self) -> &dyn tsc_host::CompilerHost {
        &self.fs
    }

    fn path_of(&self, file_name: &str) -> String {
        self.paths.to_path(file_name)
    }

    fn indexed_file_paths(&self, project: &str, _preferences: &UserPreferences) -> Vec<String> {
        let Some(program) = self
            .project(project)
            .and_then(|project| project.program.as_ref())
        else {
            return Vec::new();
        };
        program
            .with_live(|live| {
                live.with_query_checker(|state| {
                    tsc_ls::autoimport::indexed_file_paths(state, program.prepared())
                })
            })
            .unwrap_or_default()
    }
}

/// An immutable state of the projects (tsgo `Snapshot`).
pub struct Snapshot {
    id: u64,
    parent_id: u64,
    pub(crate) fs: SnapshotFs,
    pub(crate) projects: Arc<ProjectCollection>,
    pub(crate) configs: ConfigFileRegistry,
    /// tsgo `userPreferences`.
    user_preferences: UserPreferences,
    /// tsgo `compilerOptionsForInferredProjects`: the options a new inferred
    /// project takes, its defaults when none.
    compiler_options_for_inferred_projects: Option<Arc<ConfigOptionBag>>,
    file_system_override: bool,
    created_programs: Vec<ProjectId>,
    /// tsgo `AutoImports`: the auto-import registry.
    auto_imports: Arc<Registry>,
    /// tsgo `inferredProjectContentMappers` and
    /// `inferredProjectContentMapperExtensions`: the editor's contributions.
    inferred_project_content_mappers: ContentMapperContributions,
}

/// tsgo's snapshot as the language service's host: its file system for
/// module specifier completions (tsgo `s.fs.fs`, the open documents left
/// out).
impl tsc_ls::Host for Snapshot {
    /// tsgo `Snapshot.ReadDirectory` (`vfsmatch.ReadDirectory`).
    fn read_directory(
        &self,
        path: &str,
        extensions: &[&str],
        excludes: &[&str],
        includes: &[&str],
        depth: Option<usize>,
    ) -> Vec<String> {
        use tsc_program::ConfigParseHost;
        let host = tsc_host::vfs::VfsCompilerHost::new(
            &*self.fs.base,
            self.fs.paths().current_directory.clone(),
        );
        let patterns = |patterns: &[&str]| {
            patterns
                .iter()
                .map(|&pattern| tsc_diagnostics::JsString::from(pattern))
                .collect::<Vec<_>>()
        };
        let (excludes, includes) = (patterns(excludes), patterns(includes));
        tsc_program::CompilerConfigHost::new(&host)
            .read_directory(
                tsc_diagnostics::JsStr::from_str(path),
                extensions,
                Some(&excludes),
                Some(&includes),
                depth,
            )
            .map(|files| {
                files
                    .iter()
                    .map(|file| file.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// tsgo `Snapshot.GetDirectories`.
    fn get_directories(&self, path: &str) -> Vec<String> {
        self.fs.base.accessible_entries(path).directories
    }

    /// tsgo `Snapshot.DirectoryExists`.
    fn directory_exists(&self, path: &str) -> bool {
        self.fs.base.is_dir(path)
    }

    /// tsgo `Snapshot.FileExists`.
    fn file_exists(&self, path: &str) -> bool {
        self.fs.base.is_file(path)
    }

    /// tsgo `AutoImportRegistry`.
    fn auto_import_registry(&self) -> Option<&Registry> {
        Some(&self.auto_imports)
    }

    /// The parse of the config the snapshot keeps for `config_file_name`.
    fn config_plan(&self, config_file_name: &str) -> Option<&ConfigRootPlan> {
        self.config(config_file_name).map(|plan| &**plan)
    }

    /// The snapshot's file system (the open documents over it) as a
    /// compiler host.
    fn compiler_host(&self) -> Option<Box<dyn tsc_host::CompilerHost + '_>> {
        Some(Box::new(tsc_host::vfs::VfsCompilerHost::new(
            &*self.fs.base,
            self.fs.paths().current_directory.clone(),
        )))
    }
}

impl tsc_emitter::source_mapper::SourceMapperHost for Snapshot {
    fn use_case_sensitive_file_names(&self) -> bool {
        self.fs.paths().case_sensitive
    }

    /// tsgo `Snapshot.ReadFile`: the file's text as the snapshot reads it.
    fn read_file(&self, file_name: &str) -> Option<String> {
        let file = self.fs.file(file_name)?;
        Some(
            tsc_program::decode_host_text(file.content.to_vec())
                .unwrap_or_else(|_| String::from_utf8_lossy(&file.content).into_owned()),
        )
    }
}

impl Snapshot {
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The ID of the snapshot this one was cloned from.
    pub fn parent_id(&self) -> u64 {
        self.parent_id
    }

    /// tsgo `Projects()`: the configured projects, then the synthetic
    /// programs, each by ID, then the inferred project.
    pub fn projects(&self) -> impl Iterator<Item = &Arc<Project>> {
        self.projects
            .configured
            .values()
            .chain(self.projects.synthetic.values())
            .chain(self.projects.inferred.iter())
    }

    /// tsgo `GetProject`.
    pub fn project(&self, id: &ProjectId) -> Option<&Arc<Project>> {
        let id = id.canonical();
        match id.kind() {
            Some(ProjectKind::Inferred) => self.projects.inferred.as_ref(),
            _ => self
                .projects
                .configured
                .get(&id)
                .or_else(|| self.projects.synthetic.get(&id)),
        }
    }

    pub fn inferred_project(&self) -> Option<&Arc<Project>> {
        self.projects.inferred.as_ref()
    }

    /// tsgo `LanguageServiceProjects`: the configured projects by ID, then
    /// the inferred project (the API's synthetic programs take no part in
    /// the language service's searches over projects).
    pub fn language_service_projects(&self) -> impl Iterator<Item = &Arc<Project>> {
        self.projects
            .configured
            .values()
            .chain(self.projects.inferred.iter())
    }

    /// tsgo `GetDefaultProject`: the project a file belongs to.
    pub fn default_project(&self, file_name: &str) -> Option<&Arc<Project>> {
        self.projects.default_project(
            &self.fs.paths().to_path(file_name),
            &self.configs,
            self.fs.paths(),
        )
    }

    /// tsgo's `cacheDirectories[path]` or `hasOverlayWithin(path)`: whether
    /// the snapshot read files below `path` or has open documents there.
    pub(crate) fn has_files_within(&self, path: &str) -> bool {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        self.fs
            .files
            .range(prefix.clone()..)
            .next()
            .is_some_and(|(cached, _)| cached.starts_with(&prefix))
            || self
                .fs
                .overlays
                .range(prefix.clone()..)
                .next()
                .is_some_and(|(open, _)| open.starts_with(&prefix))
    }

    /// tsgo `contentMapperWatchState`'s files, by path.
    pub(crate) fn content_mapper_watched_paths(&self) -> BTreeSet<String> {
        self.content_mapper_watched_files()
    }

    /// The path of `file_name` in the snapshot's file system.
    pub(crate) fn to_path(&self, file_name: &str) -> String {
        self.fs.paths().to_path(file_name)
    }

    /// tsgo `logCacheStats`' counts: the open documents, the cached disk
    /// files, the realpath aliases, the projects and the configs.
    pub(crate) fn cache_statistics(&self) -> [usize; 5] {
        [
            self.fs.overlays.len(),
            self.fs.files.len(),
            self.fs.aliases.len(),
            self.projects().count(),
            self.configs.len(),
        ]
    }

    /// tsgo `GetOpenConfiguredProjects`: the configured projects with an
    /// open file, the default project of each open file or else every
    /// configured project that has it.
    pub fn open_configured_projects(&self) -> BTreeSet<ProjectId> {
        let mut projects = BTreeSet::new();
        for (path, overlay) in self.fs.overlays.iter() {
            if let Some(project) = self.default_project(overlay.file_name()) {
                if project.kind == ProjectKind::Configured {
                    projects.insert(project.id.clone());
                    continue;
                }
            }
            for project in self.projects.configured.values() {
                if project.contains_file(path) {
                    projects.insert(project.id.clone());
                }
            }
        }
        projects
    }

    /// tsgo `GetLanguageServiceProjectsContainingFile`: the configured
    /// projects whose programs have the file, then the inferred project.
    /// tsgo `Project.HasFile`: whether the project's program has the file.
    pub fn project_has_file(&self, project: &Project, file_name: &str) -> bool {
        project.contains_file(&self.fs.paths().to_path(file_name))
    }

    pub fn projects_containing_file(&self, file_name: &str) -> Vec<&Arc<Project>> {
        let path = self.fs.paths().to_path(file_name);
        self.projects
            .configured
            .values()
            .chain(self.projects.inferred.iter())
            .filter(|project| project.contains_file(&path))
            .collect()
    }

    /// The synthetic programs the update created, in request order (tsgo
    /// `CreatedPrograms`).
    pub fn created_programs(&self) -> &[ProjectId] {
        &self.created_programs
    }

    /// tsgo `UserPreferences`: the user's preferences for the snapshot.
    pub fn user_preferences(&self) -> &UserPreferences {
        &self.user_preferences
    }

    /// tsgo `isOpenFile`: whether the editor has the file open.
    pub fn is_open_file(&self, file_name: &str) -> bool {
        self.fs
            .overlays
            .contains_key(&self.fs.paths().to_path(file_name))
    }

    /// The editor's open document at `file_name`.
    pub fn overlay(&self, file_name: &str) -> Option<&Arc<Overlay>> {
        self.fs.overlays.get(&self.fs.paths().to_path(file_name))
    }

    /// The open documents by path.
    pub fn overlays(&self) -> &Overlays {
        &self.fs.overlays
    }

    /// Whether the snapshot reads an API request's file system.
    pub fn has_file_system_override(&self) -> bool {
        self.file_system_override
    }

    /// tsgo `LSPLineMap`: the protocol line starts of a file the snapshot
    /// reads (an open document's own).
    pub fn lsp_line_map(&self, file_name: &str) -> Option<Arc<LspLineMap>> {
        if let Some(overlay) = self.fs.overlays.get(&self.fs.paths().to_path(file_name)) {
            return Some(overlay.line_map());
        }
        self.fs.file(file_name).map(|file| file.line_map())
    }

    /// tsgo `Snapshot.Converters`: positions in the snapshot's files.
    pub fn converters(self: &Arc<Self>, position_encoding: PositionEncodingKind) -> Converters {
        let snapshot = Arc::clone(self);
        Converters::new(position_encoding, move |file_name| {
            snapshot
                .lsp_line_map(file_name)
                .unwrap_or_else(|| Arc::new(LspLineMap::new("")))
        })
    }

    /// tsgo `Snapshot.ReadFile`: the file as the snapshot read it, or as its
    /// file system holds it.
    pub fn read_file(&self, file_name: &str) -> Option<Vec<u8>> {
        self.fs.read(file_name)
    }

    /// The snapshot's files as a file system (tsgo's snapshot as the host of
    /// a resolver).
    pub fn file_system(&self) -> Arc<dyn FileSystem> {
        Arc::new(crate::fs::SnapshotView {
            fs: self.fs.clone(),
        })
    }

    /// tsgo `ConfigFileRegistry.isTracked`: whether the snapshot keeps an
    /// entry for the config at `file_name`.
    pub fn is_config_tracked(&self, file_name: &str) -> bool {
        self.configs.is_tracked(&self.fs.paths().to_path(file_name))
    }

    /// tsgo `ConfigFileRegistry.GetConfig`: the parse of the config the
    /// snapshot keeps for `file_name`.
    pub fn config(&self, file_name: &str) -> Option<&Arc<ConfigRootPlan>> {
        self.configs.config(&self.fs.paths().to_path(file_name))
    }

    /// tsgo `ContentMapperExtensions` (`contentMapperWatchState`): the
    /// extensions of the content mappers of every config the snapshot keeps
    /// and of the editor's contributions, sorted, each once.
    pub fn content_mapper_extensions(&self) -> Vec<String> {
        let mut extensions = self.configs.content_mapper_extensions();
        extensions.extend(
            self.inferred_project_content_mappers
                .extensions
                .iter()
                .cloned(),
        );
        extensions.sort();
        extensions.dedup();
        extensions
    }

    /// tsgo `contentMapperWatchState`'s files: the paths the projects'
    /// content mappers watch, whose changes the next snapshot keeps.
    fn content_mapper_watched_files(&self) -> BTreeSet<String> {
        self.projects()
            .filter_map(|project| project.content_mapper_watched_files.as_deref())
            .flatten()
            .cloned()
            .collect()
    }
}

/// The services shared by a session's snapshots (tsgo `SnapshotHost`).
pub struct SnapshotHost {
    options: SessionOptions,
    fs: Arc<dyn FileSystem>,
    paths: Paths,
    snapshot_id: AtomicU64,
    /// tsgo `parseCache`: the parsed and bound sources the snapshots'
    /// programs share.
    documents: DocumentRegistry,
    /// tsgo `contentMapperHost`: the content mappers' processes the
    /// programs share.
    content_mapper_host: Option<ContentMapperHost>,
    /// tsgo `contentMappedParseCache`: the content mappers' outputs the
    /// programs share.
    content_mapped_cache: Arc<ContentMappedCache>,
}

impl SnapshotHost {
    /// tsgo `NewSnapshotHost`. The content mapper host runs the mappers
    /// through `spawner` under `run_external_code` (tsgo
    /// `newContentMapperHost`); without either, a config's content mappers
    /// do not register.
    /// tsgo `NewSnapshotHost` (with tsgo `newContentMapperHost`: the
    /// mappers run under `RunExternalCode`, initialized in `locale`, the
    /// client's).
    pub fn new(
        options: SessionOptions,
        fs: Arc<dyn FileSystem>,
        spawner: Option<Arc<dyn Spawner>>,
        locale: &str,
        content_mapper_logger: Option<tsc_program::content_mapper::host::Logger>,
    ) -> Self {
        let paths = Paths {
            current_directory: options.current_directory.clone(),
            case_sensitive: fs.case_sensitive(),
        };
        let content_mapper_host = spawner
            .filter(|_| options.run_external_code)
            .map(|spawner| ContentMapperHost::with_logger(spawner, locale, content_mapper_logger));
        Self {
            options,
            fs,
            paths,
            snapshot_id: AtomicU64::new(0),
            documents: DocumentRegistry::new(),
            content_mapper_host,
            content_mapped_cache: Arc::default(),
        }
    }

    /// The content mappers' host, under `run_external_code` with a spawner.
    pub(crate) fn content_mapper_host(&self) -> Option<&ContentMapperHost> {
        self.content_mapper_host.as_ref()
    }

    /// The content mappers' outputs the programs share (tsgo's
    /// content-mapped parse cache).
    pub fn content_mapped_cache(&self) -> &Arc<ContentMappedCache> {
        &self.content_mapped_cache
    }

    /// tsgo `SnapshotHost.Close`: the content mappers' processes end.
    pub fn close(&self) {
        if let Some(host) = &self.content_mapper_host {
            host.close();
        }
    }

    pub fn options(&self) -> &SessionOptions {
        &self.options
    }

    /// The documents the programs share (tsgo's parse cache).
    pub fn documents(&self) -> &DocumentRegistry {
        &self.documents
    }

    /// tsgo `NewRootSnapshot`: an empty snapshot with ID 0.
    pub fn new_root_snapshot(&self) -> Snapshot {
        Snapshot {
            id: 0,
            parent_id: 0,
            fs: SnapshotFs::new(Arc::clone(&self.fs), self.paths.clone()),
            projects: Arc::default(),
            configs: ConfigFileRegistry::default(),
            user_preferences: UserPreferences::new_default(),
            compiler_options_for_inferred_projects: None,
            file_system_override: false,
            created_programs: Vec::new(),
            auto_imports: Arc::new(Registry::new(UserPreferences::new_default())),
            inferred_project_content_mappers: ContentMapperContributions::default(),
        }
    }

    /// tsgo `CloneSnapshot` (`Snapshot.Clone`, snapshot.go:431-727): the
    /// snapshot that `base` becomes with `change`. A failed API request
    /// yields its error and no snapshot.
    pub fn clone_snapshot(
        &self,
        base: &Snapshot,
        change: SnapshotChange<'_>,
    ) -> Result<Snapshot, ProjectError> {
        let SnapshotChange {
            mut file_changes,
            overlays,
            documents,
            configured_project_documents,
            new_config,
            api_request: request,
            clean_file_cache,
            compiler_options_for_inferred_projects: new_inferred_options,
            auto_imports: prepare_auto_imports,
            projects: requested_projects,
            project_tree,
            content_mapper_contributions,
        } = change;
        let inferred_project_content_mappers = content_mapper_contributions
            .clone()
            .unwrap_or_else(|| base.inferred_project_content_mappers.clone());
        let overlays = overlays.unwrap_or_else(|| Arc::clone(&base.fs.overlays));
        // tsgo `CloneSnapshot`: an API request's preferences are the new
        // configuration.
        let new_config =
            new_config.or_else(|| request.and_then(|request| request.user_preferences.clone()));
        // The documents and mapped outputs of the programs that are gone
        // (tsgo releases a program's files when its snapshot goes).
        self.documents.purge();
        self.content_mapped_cache.purge();
        let file_system = request.and_then(|request| request.file_system.clone());
        let file_system_override = file_system.is_some();
        // A total replacement, and a return from a request's file system to
        // the host's, keep nothing read before.
        if request.is_some_and(|request| request.replace_file_system)
            || (base.file_system_override && !file_system_override)
        {
            file_changes.invalidate_all = true;
        }
        // tsgo `layerOverlayFileSystem`: the open documents over the
        // snapshot's file system.
        let file_system = file_system.unwrap_or_else(|| Arc::clone(&self.fs));
        let file_system: Arc<dyn FileSystem> = if overlays.is_empty() {
            file_system
        } else {
            Arc::new(OverlayLayer::new(
                file_system,
                self.paths.clone(),
                Arc::clone(&overlays),
            ))
        };
        let fs = SnapshotFsBuilder::new(
            file_system,
            Arc::clone(&base.fs.files),
            Arc::clone(&base.fs.aliases),
            self.paths.clone(),
            overlays,
        );
        // tsgo: the extensions of the base's mappers, those of the new
        // contributions in place of the base's when they changed.
        let content_mapper_extensions = match &content_mapper_contributions {
            None => base.content_mapper_extensions(),
            Some(contributions) => {
                let mut extensions = base.configs.content_mapper_extensions();
                extensions.extend(contributions.extensions.iter().cloned());
                extensions
            }
        };
        let file_changes = fs.process_file_changes(
            file_changes,
            &base.fs,
            &content_mapper_extensions,
            &base.content_mapper_watched_files(),
        );
        let compiler_options_for_inferred_projects = new_inferred_options
            .clone()
            .or_else(|| base.compiler_options_for_inferred_projects.clone());
        let id = self.snapshot_id.fetch_add(1, Ordering::Relaxed) + 1;
        // tsgo `Clone`: the custom config file name of the new preferences.
        let custom_config_file_name = new_config.as_ref().map_or_else(
            || base.configs.custom_config_file_name().to_owned(),
            |config| config.custom_config_file_name.clone(),
        );
        let (projects, configs, created_programs) = {
            let configs = ConfigFileRegistryBuilder::new(
                &fs,
                &base.configs,
                self.options.run_external_code,
                self.options.relative_pattern_support(),
                &custom_config_file_name,
            );
            let mut builder = ProjectCollectionBuilder::new(
                self,
                &fs,
                id,
                &base.projects,
                configs,
                compiler_options_for_inferred_projects.clone(),
                &inferred_project_content_mappers,
            );
            builder.did_change_custom_config_file_name();
            if let Some(options) = &new_inferred_options {
                builder.did_change_compiler_options_for_inferred_projects(options);
            }
            if content_mapper_contributions.is_some() {
                builder.did_change_content_mapper_contributions()?;
            }
            if let Some(config) = &new_config {
                builder.did_change_user_preferences(&base.user_preferences, config);
            }
            if !file_changes.is_empty() {
                builder.did_change_files(&file_changes)?;
            }
            if let Some(request) = request {
                builder.handle_api_request(request)?;
            }
            for document in &documents {
                builder.did_request_file(document, false)?;
            }
            for document in &configured_project_documents {
                builder.did_request_file(document, true)?;
            }
            for id in &requested_projects {
                builder.did_request_project(id)?;
            }
            if let Some(request) = &project_tree {
                builder.did_request_project_trees(request)?;
            }
            builder.finish()
        };
        // tsgo `Clone`: after an open, a close or a deletion, the files no
        // project's last build read go, when a program was built (not
        // reused) here.
        let built = |project: &Arc<Project>| {
            project.program_last_update == id
                && project.program_update_kind != ProgramUpdateKind::Cloned
        };
        let all_projects = || {
            projects
                .configured
                .values()
                .chain(projects.synthetic.values())
                .chain(projects.inferred.iter())
        };
        // tsgo: on an open, a close, a deletion or when asked; the files a
        // build looked at change only when a program was built anew.
        let should_clean_file_cache = clean_file_cache
            || file_changes.opened.is_some()
            || file_changes.reopened.is_some()
            || !file_changes.closed.is_empty()
            || !file_changes.deleted.is_empty();
        if should_clean_file_cache && (clean_file_cache || all_projects().any(built)) {
            fs.retain_files(|path| all_projects().any(|project| project.seen.seen_file(path)));
        }
        // tsgo `Clone`: the auto-import registry after the change, prepared
        // for the requested file.
        let auto_imports = {
            let to_path = |file_name: &String| self.paths.to_path(file_name);
            let change = RegistryChange {
                requested_file: prepare_auto_imports
                    .map(|file_name| (self.paths.to_path(&file_name), file_name)),
                open_files: fs
                    .overlays()
                    .iter()
                    .map(|(path, overlay)| (path.clone(), overlay.file_name().to_owned()))
                    .collect(),
                changed: file_changes.changed.iter().map(to_path).collect(),
                created: file_changes.created.iter().map(to_path).collect(),
                deleted: file_changes.deleted.iter().map(to_path).collect(),
                rebuilt_programs: all_projects()
                    .filter(|project| built(project))
                    .map(|project| {
                        (
                            project.id.as_str().to_owned(),
                            project.program_update_kind == ProgramUpdateKind::NewFiles,
                        )
                    })
                    .collect(),
                user_preferences: new_config.clone(),
            };
            let host = AutoImportHost {
                projects: &projects,
                configs: &configs,
                paths: &self.paths,
                fs: tsc_host::vfs::VfsCompilerHost::new(
                    crate::fs::SourceFs::new(&fs),
                    self.paths.current_directory.clone(),
                ),
            };
            Arc::new(base.auto_imports.clone_with(&change, &host))
        };
        Ok(Snapshot {
            id,
            parent_id: base.id,
            fs: fs.finish(),
            projects,
            configs,
            user_preferences: new_config.unwrap_or_else(|| base.user_preferences.clone()),
            compiler_options_for_inferred_projects,
            file_system_override,
            created_programs,
            auto_imports,
            inferred_project_content_mappers,
        })
    }

    /// tsgo `GetNormalizedAbsolutePath` against the session's current
    /// directory.
    pub fn absolute_file_name(&self, file_name: &str) -> String {
        self.paths.absolute(file_name)
    }

    /// tsgo `tspath.ToPath` against the session's current directory and
    /// file system.
    pub fn to_path(&self, file_name: &str) -> String {
        self.paths.to_path(file_name)
    }

    /// Whether the session's file system tells upper from lower case.
    pub fn case_sensitive(&self) -> bool {
        self.paths.case_sensitive
    }

    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }
}
