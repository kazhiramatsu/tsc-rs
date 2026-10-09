//! Snapshots and their host (tsgo `SnapshotHost` and `Snapshot`,
//! snapshothost.go and snapshot.go).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tsc_compiler::DocumentRegistry;
use tsc_diagnostics::Diagnostic;
use tsc_host::vfs::FileSystem;
use tsc_program::{
    CompilerOptions, ConfigParseError, ConfigProjectReference, ConfigRootPlan, LibraryCatalog,
    ProgramLoadLimits, ProgramOptions,
};

use crate::builder::ProjectCollectionBuilder;
use crate::config::{ConfigFileRegistry, ConfigFileRegistryBuilder};
use crate::fs::{FileChangeSummary, Paths, SnapshotFs, SnapshotFsBuilder};
use crate::id::{ProjectId, ProjectKind};
use crate::project::{default_project_from_program_inclusion, Project};

/// What a session's projects are built with (tsgo `SessionOptions`).
#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub current_directory: String,
    /// The default libraries (tsgo `DefaultLibraryPath`).
    pub library_catalog: LibraryCatalog,
    pub load_limits: ProgramLoadLimits,
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
    pub compiler_options: CompilerOptions,
    pub program_options: ProgramOptions,
    pub config_file_parsing_diagnostics: Vec<Diagnostic>,
    /// The projects the program references (tsgo `ProjectReferences`), by
    /// config path.
    pub project_references: Vec<ConfigProjectReference>,
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
    /// tsgo `fileDefaultProjects`: the default projects the build that made
    /// the snapshot found, by file path.
    pub(crate) file_default_projects: BTreeMap<String, ProjectId>,
    pub(crate) api_state: ApiState,
}

impl ProjectCollection {
    /// tsgo `ProjectCollection.GetDefaultProject` for a file without an
    /// editor's config lookup: the default project the build found, else
    /// the configured project (by ID) whose program has the file, preferring
    /// one that has it directly (several that do leave the first: tsgo
    /// settles them from the config of an editor's open file only), else the
    /// inferred project when it has the file.
    pub(crate) fn default_project(&self, path: &str) -> Option<&Arc<Project>> {
        if let Some(id) = self.file_default_projects.get(path) {
            return match id.kind() {
                Some(ProjectKind::Inferred) => self.inferred.as_ref(),
                _ => self.configured.get(id),
            };
        }
        default_project_from_program_inclusion(self.configured.values(), path)
            .0
            .or_else(|| {
                self.inferred
                    .as_ref()
                    .filter(|project| project.contains_file(path))
            })
    }
}

/// An immutable state of the projects (tsgo `Snapshot`).
pub struct Snapshot {
    id: u64,
    parent_id: u64,
    pub(crate) fs: SnapshotFs,
    pub(crate) projects: Arc<ProjectCollection>,
    pub(crate) configs: ConfigFileRegistry,
    file_system_override: bool,
    created_programs: Vec<ProjectId>,
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

    /// tsgo `GetDefaultProject`: the project a file belongs to.
    pub fn default_project(&self, file_name: &str) -> Option<&Arc<Project>> {
        self.projects
            .default_project(&self.fs.paths().to_path(file_name))
    }

    /// The synthetic programs the update created, in request order (tsgo
    /// `CreatedPrograms`).
    pub fn created_programs(&self) -> &[ProjectId] {
        &self.created_programs
    }

    /// Whether the snapshot reads an API request's file system.
    pub fn has_file_system_override(&self) -> bool {
        self.file_system_override
    }

    /// tsgo `Snapshot.ReadFile`: the file as the snapshot read it, or as its
    /// file system holds it.
    pub fn read_file(&self, file_name: &str) -> Option<Vec<u8>> {
        self.fs.read(file_name)
    }

    /// tsgo `ConfigFileRegistry.GetConfig`: the parse of the config the
    /// snapshot keeps for `file_name`.
    pub fn config(&self, file_name: &str) -> Option<&Arc<ConfigRootPlan>> {
        self.configs.config(&self.fs.paths().to_path(file_name))
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
}

impl SnapshotHost {
    pub fn new(options: SessionOptions, fs: Arc<dyn FileSystem>) -> Self {
        let paths = Paths {
            current_directory: options.current_directory.clone(),
            case_sensitive: fs.case_sensitive(),
        };
        Self {
            options,
            fs,
            paths,
            snapshot_id: AtomicU64::new(0),
            documents: DocumentRegistry::new(),
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
            file_system_override: false,
            created_programs: Vec::new(),
        }
    }

    /// tsgo `CloneSnapshot` (`Snapshot.Clone`, snapshot.go:431-727): the
    /// snapshot that `base` becomes with `file_changes` and `request`.
    /// A failed request yields its error and no snapshot.
    pub fn clone_snapshot(
        &self,
        base: &Snapshot,
        mut file_changes: FileChangeSummary,
        request: Option<&ApiSnapshotRequest>,
    ) -> Result<Snapshot, ProjectError> {
        // The documents of the programs that are gone (tsgo releases a
        // program's files when its snapshot goes).
        self.documents.purge();
        let file_system = request.and_then(|request| request.file_system.clone());
        let file_system_override = file_system.is_some();
        // A total replacement, and a return from a request's file system to
        // the host's, keep nothing read before.
        if request.is_some_and(|request| request.replace_file_system)
            || (base.file_system_override && !file_system_override)
        {
            file_changes.invalidate_all = true;
        }
        let fs = SnapshotFsBuilder::new(
            file_system.unwrap_or_else(|| Arc::clone(&self.fs)),
            Arc::clone(&base.fs.files),
            self.paths.clone(),
        );
        let file_changes = fs.process_file_changes(file_changes);
        let id = self.snapshot_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (projects, configs, created_programs) = {
            let configs = ConfigFileRegistryBuilder::new(&fs, &base.configs);
            let mut builder = ProjectCollectionBuilder::new(self, &fs, id, &base.projects, configs);
            if !file_changes.is_empty() {
                builder.did_change_files(&file_changes);
            }
            if let Some(request) = request {
                builder.handle_api_request(request)?;
            }
            builder.finish()
        };
        Ok(Snapshot {
            id,
            parent_id: base.id,
            fs: fs.finish(),
            projects,
            configs,
            file_system_override,
            created_programs,
        })
    }

    pub(crate) fn paths(&self) -> &Paths {
        &self.paths
    }
}
