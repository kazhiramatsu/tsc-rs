//! A snapshot build's projects (tsgo `ProjectCollectionBuilder`,
//! projectcollectionbuilder.go): the base snapshot's projects, changed by
//! file changes and an API request. A project is copied out of the base
//! snapshot the first time the build changes it, so what the build leaves
//! alone stays shared.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use tsc_compiler::LiveProgram;
use tsc_diagnostics::JsStr;
use tsc_host::vfs::VfsCompilerHost;
use tsc_program::{
    load_emitting_program, load_program, load_project_config_program,
    resolve_config_file_name_of_project_reference, resolve_project_references_with, CanonicalPath,
    ConfigOptionBag, ConfigRootPlan, ContentMapperProjectHandle,
};

use crate::config::{ConfigFileRegistry, ConfigFileRegistryBuilder};
use crate::fs::{FileChangeSummary, SeenFiles, SnapshotFsBuilder, SourceFs};
use crate::id::{ProjectId, ProjectKind};
use crate::project::{
    default_project_from_program_inclusion, inferred_project_roots, CommandLine, ProgramRoots,
    ProgramUpdateKind, Project, ProjectProgram,
};
use crate::snapshot::{
    ApiSnapshotRequest, ApiState, CreateProgramRequest, OpenedFile, ProjectCollection,
    ProjectError, SnapshotHost,
};
use crate::watch::{
    path_globs, resolution_lookup_globs, ContentMapperWatch, WatchedFiles, WATCH_ALL,
};
use tsc_syntax::ScriptKind;

type Projects = Arc<BTreeMap<ProjectId, Arc<Project>>>;

/// Whether a search may create projects and build their programs (tsgo
/// `projectLoadKind`).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LoadKind {
    Find,
    Create,
}

/// What a default-project search found (tsgo `searchResult`): the project,
/// and the configs on the way to it, which a clean-up keeps.
#[derive(Default)]
struct SearchResult {
    project: Option<ProjectId>,
    retain: BTreeSet<String>,
}

/// The kind of a file change (tsgo `lsproto.FileChangeType`).
#[derive(Clone, Copy, Eq, PartialEq)]
enum FileChange {
    Changed,
    Created,
    Deleted,
}

pub(crate) struct ProjectCollectionBuilder<'a> {
    host: &'a SnapshotHost,
    fs: &'a SnapshotFsBuilder,
    snapshot_id: u64,
    base: &'a Arc<ProjectCollection>,
    configs: ConfigFileRegistryBuilder<'a>,
    configured: Projects,
    synthetic: Projects,
    inferred: Option<Arc<Project>>,
    /// tsgo `fileDefaultProjects`: the default projects this build found.
    file_default_projects: BTreeMap<String, ProjectId>,
    api_state: ApiState,
    created_programs: Vec<ProjectId>,
    /// tsgo `openFilesChanged`: the editor opened or closed a file.
    open_files_changed: bool,
    /// tsgo `programStructureChanged`: a change may move files between
    /// projects.
    program_structure_changed: bool,
    /// tsgo `defaultProjectsInvalidated`: every open file's default project
    /// is searched for again.
    default_projects_invalidated: bool,
    /// tsgo `compilerOptionsForInferredProjects`.
    compiler_options_for_inferred_projects: Option<Arc<ConfigOptionBag>>,
}

impl<'a> ProjectCollectionBuilder<'a> {
    pub(crate) fn new(
        host: &'a SnapshotHost,
        fs: &'a SnapshotFsBuilder,
        snapshot_id: u64,
        base: &'a Arc<ProjectCollection>,
        configs: ConfigFileRegistryBuilder<'a>,
        compiler_options_for_inferred_projects: Option<Arc<ConfigOptionBag>>,
    ) -> Self {
        Self {
            host,
            fs,
            snapshot_id,
            base,
            configs,
            configured: Arc::clone(&base.configured),
            synthetic: Arc::clone(&base.synthetic),
            inferred: base.inferred.clone(),
            file_default_projects: BTreeMap::new(),
            api_state: base.api_state.clone(),
            created_programs: Vec::new(),
            open_files_changed: !fs.overlays().keys().eq(base.open_files.iter()),
            program_structure_changed: false,
            default_projects_invalidated: false,
            compiler_options_for_inferred_projects,
        }
    }

    /// tsgo `isOpenFile`.
    fn is_open_file(&self, path: &str) -> bool {
        self.fs.is_open_file(path)
    }

    /// tsgo `Finalize`: the base snapshot's collection when nothing changed.
    pub(crate) fn finish(self) -> (Arc<ProjectCollection>, ConfigFileRegistry, Vec<ProjectId>) {
        let configs = self.configs.finish();
        let same_inferred = match (&self.inferred, &self.base.inferred) {
            (Some(inferred), Some(base)) => Arc::ptr_eq(inferred, base),
            (None, None) => true,
            _ => false,
        };
        let projects = if Arc::ptr_eq(&self.configured, &self.base.configured)
            && Arc::ptr_eq(&self.synthetic, &self.base.synthetic)
            && same_inferred
            && !self.open_files_changed
            && self.file_default_projects == self.base.file_default_projects
            && self.api_state == self.base.api_state
        {
            Arc::clone(self.base)
        } else {
            Arc::new(ProjectCollection {
                configured: self.configured,
                synthetic: self.synthetic,
                inferred: self.inferred,
                open_files: if self.open_files_changed {
                    self.fs.overlays().keys().cloned().collect()
                } else {
                    self.base.open_files.clone()
                },
                file_default_projects: self.file_default_projects,
                api_state: self.api_state,
            })
        };
        (projects, configs, self.created_programs)
    }

    fn projects_of(&mut self, id: &ProjectId) -> &mut Projects {
        match id.kind() {
            Some(ProjectKind::Synthetic) => &mut self.synthetic,
            _ => &mut self.configured,
        }
    }

    fn project(&self, id: &ProjectId) -> Option<&Arc<Project>> {
        match id.kind() {
            Some(ProjectKind::Inferred) => self.inferred.as_ref(),
            Some(ProjectKind::Synthetic) => self.synthetic.get(id),
            _ => self.configured.get(id),
        }
    }

    fn insert(&mut self, project: Project) {
        let id = project.id.clone();
        if id.kind() == Some(ProjectKind::Inferred) {
            self.inferred = Some(Arc::new(project));
        } else {
            Arc::make_mut(self.projects_of(&id)).insert(id, Arc::new(project));
        }
    }

    /// Change the project, copied out of the snapshots that share it the
    /// first time (tsgo's dirty map `Change`).
    fn change(&mut self, id: &ProjectId, change: impl FnOnce(&mut Project)) {
        let entry = match id.kind() {
            Some(ProjectKind::Inferred) => self.inferred.as_mut(),
            _ => Arc::make_mut(self.projects_of(id)).get_mut(id),
        }
        .unwrap_or_else(|| panic!("project {id} not found"));
        if Arc::strong_count(entry) > 1 {
            *entry = Arc::new(entry.cloned_for_change());
        }
        change(Arc::get_mut(entry).expect("the project is not shared"));
    }

    /// tsgo `forEachProject`: the configured projects, the synthetic
    /// programs, then the inferred project.
    fn project_ids(&self) -> Vec<ProjectId> {
        self.configured
            .keys()
            .chain(self.synthetic.keys())
            .chain(self.inferred.as_ref().map(|project| &project.id))
            .cloned()
            .collect()
    }

    /// tsgo `HandleAPIRequest` (projectcollectionbuilder.go:186-377) for
    /// projects and synthetic programs.
    pub(crate) fn handle_api_request(
        &mut self,
        request: &ApiSnapshotRequest,
    ) -> Result<(), ProjectError> {
        let paths = self.host.paths().clone();
        let mut projects_to_close = BTreeSet::new();
        for file_name in &request.close_projects {
            let path = paths.to_path(file_name);
            match self.api_state.open_projects.get(&path).copied() {
                Some(count) if count > 1 => {
                    self.api_state.open_projects.insert(path, count - 1);
                }
                Some(_) => {
                    self.api_state.open_projects.remove(&path);
                    projects_to_close.insert(path);
                }
                None => {}
            }
        }

        for file_name in &request.open_projects {
            let path = paths.to_path(file_name);
            let id = self.find_or_create_configured_project(&paths.absolute(file_name), &path);
            *self
                .api_state
                .open_projects
                .entry(path.clone())
                .or_default() += 1;
            // A project opened again in the same request stays open.
            projects_to_close.remove(&path);
            self.update_program(&id)?;
        }

        self.handle_opened_files(request)?;

        for path in projects_to_close {
            if let Some(id) = ProjectId::configured(&path) {
                if self.configured.contains_key(&id) {
                    self.delete_project(&id);
                }
            }
        }

        self.place_opened_files(request)?;

        let removed = request
            .remove_programs
            .iter()
            .map(ProjectId::canonical)
            .collect::<BTreeSet<_>>();
        let mut reconfigured = BTreeSet::new();
        for program in &request.reconfigure_programs {
            let id = program.program_id.canonical();
            if !reconfigured.insert(id.clone()) {
                return Err(ProjectError::new(format!(
                    "synthetic program reconfigured more than once: {id}"
                )));
            }
            if removed.contains(&id) {
                return Err(ProjectError::new(format!(
                    "synthetic program cannot be reconfigured and removed: {id}"
                )));
            }
            if !self.synthetic.contains_key(&id) {
                return Err(ProjectError::new(format!(
                    "synthetic program not found for reconfiguration: {id}"
                )));
            }
        }
        for id in &removed {
            if !self.synthetic.contains_key(id) {
                return Err(ProjectError::new(format!(
                    "synthetic program not found for removal: {id}"
                )));
            }
            self.delete_project(id);
        }

        let mut created = Vec::with_capacity(request.create_programs.len());
        for program in &request.create_programs {
            let id = self.next_synthetic_project_id();
            self.update_or_create_synthetic_project(&id, program)?;
            created.push(id);
        }
        let mut updated = created.clone();
        for program in &request.reconfigure_programs {
            let id = program.program_id.canonical();
            self.update_or_create_synthetic_project(&id, &program.program)?;
            updated.push(id);
        }
        for id in &updated {
            if self.project(id).is_some_and(|project| project.dirty) {
                self.update_program(id)?;
            }
        }
        self.created_programs = created;

        self.ensure_files(request)?;
        for id in &request.ensure_programs {
            // tsgo `DidRequestProject`: a program is brought up to date, never
            // created.
            let id = id.canonical();
            if self.project(&id).is_some() {
                self.update_program(&id)?;
            }
        }
        if request.ensure_all_programs {
            for id in self.project_ids() {
                if self.project(&id).is_some() {
                    self.update_program(&id)?;
                }
            }
        }
        Ok(())
    }

    /// tsgo `findOrCreateProject` with `projectLoadKindCreate`.
    fn find_or_create_configured_project(&mut self, file_name: &str, path: &str) -> ProjectId {
        let id = ProjectId::configured(path)
            .unwrap_or_else(|| panic!("invalid configured project ID: {path}"));
        if !self.configured.contains_key(&id) {
            let directory = tsc_program::get_directory_path(JsStr::from_str(file_name))
                .to_string_lossy()
                .into_owned();
            self.insert(self.with_watches(Project::new_configured(file_name, path, directory)));
        }
        id
    }

    /// tsgo `nextSyntheticProjectID`: the lowest free number.
    fn next_synthetic_project_id(&self) -> ProjectId {
        (1..)
            .map(ProjectId::synthetic)
            .find(|id| !self.synthetic.contains_key(id))
            .expect("a synthetic project ID is free")
    }

    /// tsgo `updateOrCreateSyntheticProject`: a new synthetic project, or the
    /// project's roots and options replaced when they differ.
    fn update_or_create_synthetic_project(
        &mut self,
        id: &ProjectId,
        program: &CreateProgramRequest,
    ) -> Result<(), ProjectError> {
        let mut roots = ProgramRoots::new(
            program.root_file_names.clone(),
            program.options.clone(),
            program.config_file_parsing_diagnostics.clone(),
            program.project_references.clone(),
            self.host.paths(),
        )?;
        if let Some(resolution) = &program.module_resolution {
            roots.program_options = roots
                .program_options
                .with_module_resolution_override(resolution.clone());
        }
        let Some(project) = self.synthetic.get(id) else {
            let current_directory = self.host.options().current_directory.clone();
            self.insert(self.with_watches(Project::new_synthetic(
                id.clone(),
                current_directory,
                Arc::new(roots),
            )));
            return Ok(());
        };
        let changed = !matches!(
            &project.command_line,
            Some(CommandLine::Roots(current)) if **current == roots
        );
        if changed {
            self.change(id, |project| {
                project.set_command_line(CommandLine::Roots(Arc::new(roots)));
            });
        }
        Ok(())
    }

    /// tsgo `updateProgram`: a configured project takes its config again (a
    /// config that cannot be read deletes the project); a project whose
    /// command line changed or that is dirty gets a new program. Whether the
    /// program's files changed is returned.
    fn update_program(&mut self, id: &ProjectId) -> Result<bool, ProjectError> {
        let Some(project) = self.project(id).cloned() else {
            return Ok(false);
        };
        let mut update = false;
        if project.kind == ProjectKind::Configured {
            let file_name = project.config_file_name.clone().unwrap_or_default();
            let path = project.config_file_path.clone().unwrap_or_default();
            match self
                .configs
                .acquire_config_for_project(&file_name, &path, id)?
            {
                None => {
                    self.delete_project(id);
                    return Ok(true);
                }
                Some(plan) => {
                    let command_line = CommandLine::Config(plan);
                    if !project.has_command_line(&command_line) {
                        update = true;
                        self.change(id, |project| project.set_command_line(command_line));
                    }
                }
            }
        }
        let project = Arc::clone(self.project(id).expect("the project is loaded"));
        if !update && !project.dirty {
            return Ok(false);
        }
        // tsgo `CreateProgram`: a program dirty from one changed file, with
        // the same command line, takes the file read again in place
        // (`UpdateProgram`); it keeps what the previous build read.
        if let (false, Some(path), Some(old)) = (update, &project.dirty_file_path, &project.program)
        {
            if let Some(program) = self.reuse_program(old, path)? {
                let (watched, watch) = self.content_mapper_watch(&project, &program);
                let snapshot_id = self.snapshot_id;
                self.change(id, |project| {
                    project.program = Some(Arc::new(program));
                    project.program_update_kind = ProgramUpdateKind::Cloned;
                    project.program_last_update = snapshot_id;
                    project.dirty = false;
                    project.dirty_file_path = None;
                    project.content_mapper_watched_files = Some(Arc::new(watched));
                    project.content_mapper_watch = watch;
                });
                return Ok(false);
            }
        }
        let (program, seen) = self.create_program(&project)?;
        let (watched, content_mapper_watch) = self.content_mapper_watch(&project, &program);
        let update_kind = match &project.program {
            Some(old) if old.has_same_file_names(&program) => ProgramUpdateKind::SameFileNames,
            _ => ProgramUpdateKind::NewFiles,
        };
        // tsgo `releaseDroppedProjectReferences`.
        if let Some(old) = &project.program {
            for path in old.references().difference(program.references()) {
                self.configs.release_config_for_project(path, id);
            }
        }
        let snapshot_id = self.snapshot_id;
        self.change(id, |project| {
            project.program = Some(Arc::new(program));
            project.program_update_kind = update_kind;
            project.program_last_update = snapshot_id;
            // tsgo `CloneWatchers`: the build's files, when they changed.
            if update_kind == ProgramUpdateKind::NewFiles {
                project.program_files_watch = project
                    .program_files_watch
                    .as_ref()
                    .map(|watch| Arc::new(watch.clone_with(seen.files().clone())));
            }
            project.seen = Arc::new(seen);
            project.dirty = false;
            project.dirty_file_path = None;
            project.content_mapper_watched_files = Some(Arc::new(watched));
            project.content_mapper_watch = content_mapper_watch;
        });
        Ok(update_kind == ProgramUpdateKind::NewFiles)
    }

    /// tsgo `updateProgram`'s content mapper watch: the manifests of the
    /// command line's mapper packages, and the files its dynamic mappers
    /// watch when the program has a mapped file (a mapper no file uses is
    /// not opened).
    /// The paths of those files, and the project's content mapper watch of
    /// them (tsgo `contentMapperWatch.Clone`).
    fn content_mapper_watch(
        &self,
        project: &Project,
        program: &ProjectProgram,
    ) -> (BTreeSet<String>, Option<Arc<ContentMapperWatch>>) {
        let mut files = match &project.command_line {
            Some(CommandLine::Config(plan)) => plan.content_mapper_manifest_files(),
            _ => Vec::new(),
        };
        let prepared = program.prepared();
        if prepared
            .source_files()
            .iter()
            .any(|source| source.content_mapped().is_some())
        {
            if let Some(mapper_project) = prepared.program_options().content_mapper_project() {
                files.extend(mapper_project.0.watched_files().unwrap_or_default());
            }
        }
        files.sort();
        files.dedup();
        let paths = self.host.paths();
        let watched = files.iter().map(|file| paths.to_path(file)).collect();
        let watch = project
            .content_mapper_watch
            .as_ref()
            .map(|watch| Arc::new(watch.clone_with(files)));
        (watched, watch)
    }

    /// tsgo `NewProject`'s watches: the program's files (the workspace, the
    /// project's directory, the library and the directories its build
    /// looked in) and the content mappers' files.
    fn with_watches(&self, mut project: Project) -> Project {
        let options = self.host.options();
        let paths = self.host.paths();
        let case_sensitive = paths.case_sensitive;
        let relative_pattern_support = options.relative_pattern_support();
        let workspace = paths.current_directory.clone();
        let library = options
            .library_catalog
            .directory()
            .to_string_lossy()
            .into_owned();
        let project_directory = project.current_directory.clone();
        project.program_files_watch = Some(Arc::new(WatchedFiles::new(
            format!("program files for {}", project.id),
            WATCH_ALL,
            relative_pattern_support,
            move |files: &BTreeMap<String, String>| {
                resolution_lookup_globs(
                    &workspace,
                    &library,
                    &project_directory,
                    case_sensitive,
                    files,
                )
            },
        )));
        let workspace = paths.current_directory.clone();
        project.content_mapper_watch = Some(Arc::new(WatchedFiles::new(
            format!("content mapper configuration files for {}", project.id),
            WATCH_ALL,
            relative_pattern_support,
            move |files: &Vec<String>| path_globs(&workspace, case_sensitive, files),
        )));
        project
    }

    /// tsgo `refreshContentMapperProjectForChanges`: a project whose content
    /// mapper watch has a changed file, or every project past the excessive
    /// change threshold, refreshes its program's mapper project and builds
    /// its program again.
    fn refresh_content_mapper_project_for_changes(
        &mut self,
        id: &ProjectId,
        changes: &[&[String]],
        refresh_all: bool,
    ) {
        let Some(project) = self.project(id) else {
            return;
        };
        let (Some(program), Some(watched)) =
            (&project.program, &project.content_mapper_watched_files)
        else {
            return;
        };
        if !refresh_all
            && !changes
                .iter()
                .flat_map(|paths| paths.iter())
                .any(|path| watched.contains(path))
        {
            return;
        }
        if let Some(mapper_project) = program
            .prepared()
            .program_options()
            .content_mapper_project()
        {
            let _ = mapper_project.0.refresh();
        }
        self.change(id, |project| {
            project.dirty = true;
            project.dirty_file_path = None;
        });
    }

    /// tsgo `UpdateProgram`'s reuse: `old` with the file at `path` read
    /// again, when the file can be replaced in place (see
    /// [`LiveProgram::reuse`]).
    fn reuse_program(
        &self,
        old: &ProjectProgram,
        path: &str,
    ) -> Result<Option<ProjectProgram>, ProjectError> {
        let Ok(canonical) = CanonicalPath::from_js_normalized(JsStr::from_str(path)) else {
            return Ok(None);
        };
        let prepared = old.prepared();
        let Some(source) = prepared
            .source_id(&canonical)
            .and_then(|id| prepared.source_file(id))
        else {
            return Ok(None);
        };
        // tsgo's parse cache key holds the file handle's kind: an open
        // document whose language changed is parsed anew.
        if self.fs.overlays().get(path).is_some_and(|overlay| {
            overlay.kind() != ScriptKind::Unknown && overlay.kind() != source.script_kind()
        }) {
            return Ok(None);
        }
        let file_name = source.path().display().to_string_lossy().into_owned();
        let Some(text) = self
            .fs
            .get(&file_name)
            .and_then(|file| tsc_program::decode_host_text(file.content.to_vec()).ok())
        else {
            return Ok(None);
        };
        // tsgo's new compiler host: a content mapper's file runs through
        // its mapper again over the snapshot's files.
        let host = VfsCompilerHost::new(
            SourceFs::new(self.fs),
            self.host.paths().current_directory.clone(),
        );
        match LiveProgram::reuse(prepared, &canonical, text, &host, self.host.documents()) {
            Some(Ok(live)) => Ok(Some(ProjectProgram::new(live, old.references().clone()))),
            Some(Err(error)) => Err(ProjectError::new(format!(
                "cannot reuse the program for {file_name}: {error}"
            ))),
            None => Ok(None),
        }
    }

    /// tsgo `CreateProgram` (`NewProgram` over the build's files): the
    /// program and what its build looked for.
    fn create_program(
        &mut self,
        project: &Project,
    ) -> Result<(ProjectProgram, SeenFiles), ProjectError> {
        let options = self.host.options();
        let paths = self.host.paths().clone();
        let source = SourceFs::tracking(self.fs);
        let failed = |error: &dyn std::fmt::Display| {
            ProjectError::new(format!(
                "cannot create the program of {}: {error}",
                project.id
            ))
        };
        let Some(command_line) = project.command_line.as_ref() else {
            return Err(failed(&"the project has no command line"));
        };
        // tsgo's host `GetResolvedProjectReference`: each referenced config
        // is the project's from the config registry, and a file the program
        // read.
        let mut references = BTreeSet::new();
        let resolved = if command_line.project_references().is_empty() {
            None
        } else {
            let configs = &mut self.configs;
            let resolved = resolve_project_references_with(
                command_line.config_file_name(),
                command_line.project_references(),
                JsStr::from_str(&project.current_directory),
                paths.case_sensitive,
                &mut |config_file_name| {
                    let file_name = config_file_name.to_string_lossy().into_owned();
                    let path = paths.to_path(&file_name);
                    source.track_file(path.clone(), file_name.clone());
                    references.insert(path.clone());
                    configs.acquire_config_for_project(&file_name, &path, &project.id)
                },
            )?;
            Some(Arc::new(resolved))
        };
        let host = VfsCompilerHost::new(&source, project.current_directory.clone());
        let document_script_kinds = self.document_script_kinds();
        let prepared = match command_line {
            CommandLine::Config(plan) => load_project_config_program(
                &host,
                &Arc::new(
                    (*self.with_content_mapper_project(plan))
                        .clone()
                        .with_document_script_kinds(document_script_kinds),
                ),
                resolved,
                plan.options()
                    .option_bool("disableSourceOfProjectReferenceRedirect")
                    != Some(true),
                &options.library_catalog,
                options.load_limits,
            )
            .map_err(|error| failed(&format!("{error:?}")))?,
            CommandLine::Roots(roots) => {
                let root_names = roots
                    .root_file_names
                    .iter()
                    .map(PathBuf::from)
                    .collect::<Vec<_>>();
                let load = if roots.compiler_options.no_emit == Some(true) {
                    load_program
                } else {
                    load_emitting_program
                };
                let mut program_options = roots
                    .program_options
                    .clone()
                    .with_document_script_kinds(document_script_kinds);
                if let Some(resolved) = resolved {
                    program_options = program_options
                        .with_project_references(resolved)
                        .with_project_reference_sources(true);
                }
                load(
                    &host,
                    &root_names,
                    roots.compiler_options.clone(),
                    program_options,
                    &options.library_catalog,
                    options.load_limits,
                )
                .map_err(|error| failed(&error))?
            }
        };
        drop(host);
        let live = LiveProgram::with_documents(prepared, self.host.documents())
            .map_err(|error| failed(&error))?;
        Ok((ProjectProgram::new(live, references), source.into_seen()))
    }

    /// tsgo's file handles' kinds (`FileHandle.Kind`): the languages of the
    /// open documents that give one, by path.
    fn document_script_kinds(&self) -> Arc<BTreeMap<String, ScriptKind>> {
        Arc::new(
            self.fs
                .overlays()
                .iter()
                .filter(|(_, overlay)| overlay.kind() != ScriptKind::Unknown)
                .map(|(path, overlay)| (path.clone(), overlay.kind()))
                .collect(),
        )
    }

    /// tsgo compiler host `ContentMapperProject`: the config's content
    /// mappers (it has them under the session's `run_external_code`) run in
    /// a project of the session's host, which the program's loader
    /// transforms through, taking the outputs the session's programs hold
    /// (tsgo's content-mapped parse cache). The program and those reused
    /// from it keep the project until no snapshot holds them (tsgo
    /// `Snapshot.dispose`).
    fn with_content_mapper_project(&self, plan: &Arc<ConfigRootPlan>) -> Arc<ConfigRootPlan> {
        let project = self.host.content_mapper_host().and_then(|host| {
            let view = host.project(Arc::clone(plan.program_options().content_mapper_spec()?))?;
            Some(view.with_content_mapped_cache(Arc::clone(self.host.content_mapped_cache())))
        });
        match project {
            Some(project) => Arc::new(
                (**plan)
                    .clone()
                    .with_content_mapper_project(ContentMapperProjectHandle(Arc::new(project))),
            ),
            None => Arc::clone(plan),
        }
    }

    /// tsgo `deleteProject`: a configured project releases its config.
    fn delete_project(&mut self, id: &ProjectId) {
        let Some(project) = self.project(id).cloned() else {
            return;
        };
        if let Some(program) = &project.program {
            for path in program.references() {
                self.configs.release_config_for_project(path, id);
            }
        }
        if let Some(path) = &project.config_file_path {
            self.configs.release_config_for_project(path, id);
        }
        if id.kind() == Some(ProjectKind::Inferred) {
            // tsgo `deleteInferredProject`.
            self.inferred = None;
        } else {
            Arc::make_mut(self.projects_of(id)).remove(id);
        }
    }

    /// Steps 3, 4 and 7 of tsgo `HandleAPIRequest`: the open counts of the
    /// files the request closes and opens, then the default projects of the
    /// newly opened files (a file without one goes to the inferred project),
    /// and the clean-up of the configured projects nothing needs.
    fn handle_opened_files(&mut self, request: &ApiSnapshotRequest) -> Result<(), ProjectError> {
        let paths = self.host.paths().clone();
        for file_name in request.close_files.iter().flatten() {
            let path = paths.to_path(file_name);
            match self.api_state.open_files.get_mut(&path) {
                Some(file) if file.ref_count > 1 => file.ref_count -= 1,
                Some(_) => {
                    self.api_state.open_files.remove(&path);
                }
                None => {}
            }
        }
        for file_name in request.open_files.iter().flatten() {
            let file = self
                .api_state
                .open_files
                .entry(paths.to_path(file_name))
                .or_insert_with(|| OpenedFile {
                    file_name: file_name.clone(),
                    ref_count: 0,
                });
            file.file_name = file_name.clone();
            file.ref_count += 1;
        }
        Ok(())
    }

    /// Step 7 of tsgo `HandleAPIRequest`: place the newly opened files.
    fn place_opened_files(&mut self, request: &ApiSnapshotRequest) -> Result<(), ProjectError> {
        let paths = self.host.paths().clone();
        if let Some(open_files) = &request.open_files {
            let mut retain = BTreeSet::new();
            let mut ensure_inferred_project = false;
            for file_name in open_files {
                let path = paths.to_path(file_name);
                let result = self.find_or_create_default_configured_project_for_file(
                    file_name,
                    &path,
                    LoadKind::Create,
                )?;
                retain.extend(result.retain);
                if result.project.is_none() {
                    if !self.is_supported_in_inferred_project(file_name) {
                        return Err(ProjectError::new(format!(
                            "no project found for opened file: {file_name}"
                        )));
                    }
                    ensure_inferred_project = true;
                }
            }
            self.cleanup_configured_projects(Some(&retain))?;
            if ensure_inferred_project && self.inferred.is_some() {
                self.update_program(&ProjectId::inferred())?;
            }
        } else if request.close_files.is_some() {
            self.cleanup_configured_projects(None)?;
        }
        Ok(())
    }

    /// Step 12 of tsgo `HandleAPIRequest`: each file's default project is
    /// brought up to date (`didRequestFile`), and a file must have one.
    fn ensure_files(&mut self, request: &ApiSnapshotRequest) -> Result<(), ProjectError> {
        let paths = self.host.paths().clone();
        for file_name in &request.ensure_files {
            let path = paths.to_path(file_name);
            // tsgo `didRequestFile` for a file no editor has open.
            let result = self.find_or_create_default_configured_project_for_file(
                file_name,
                &path,
                LoadKind::Create,
            )?;
            if result.project.is_none() {
                self.ensure_inferred_project_includes_closed_file(file_name)?;
            }
            if self.find_default_project(file_name, &path)?.is_none() {
                return Err(ProjectError::new(format!(
                    "no project found for opened file: {file_name}"
                )));
            }
        }
        Ok(())
    }

    /// tsgo `findOrCreateDefaultConfiguredProjectForFile`: the default
    /// project found before in this build, or the search from the nearest
    /// `tsconfig.json`/`jsconfig.json` above the file.
    fn find_or_create_default_configured_project_for_file(
        &mut self,
        file_name: &str,
        path: &str,
        load_kind: LoadKind,
    ) -> Result<SearchResult, ProjectError> {
        if let Some(id) = self.file_default_projects.get(path) {
            if id.kind() == Some(ProjectKind::Inferred) {
                return Ok(SearchResult::default());
            }
            let project = self.configured.contains_key(id).then(|| id.clone());
            return Ok(SearchResult {
                project,
                retain: BTreeSet::new(),
            });
        }
        let Some(config_file_name) = self.configs.get_config_file_name_for_file(file_name, path)
        else {
            return Ok(SearchResult::default());
        };
        let mut visited = BTreeSet::new();
        let result = self.find_or_create_default_configured_project_worker(
            file_name,
            path,
            &config_file_name,
            load_kind,
            &mut visited,
            None,
        )?;
        if let Some(project) = &result.project {
            self.file_default_projects
                .insert(path.to_owned(), project.clone());
        }
        Ok(result)
    }

    /// tsgo `findOrCreateDefaultConfiguredProjectWorker`: a breadth-first
    /// search from the config through the project references for a project
    /// whose program has the file. tsgo visits a level's configs in
    /// parallel and keeps the lowest one that has the file; visiting them in
    /// order finds the same one. Without a direct inclusion, the search goes
    /// on from the config above (a solution's), which only an open file has
    /// a name for.
    fn find_or_create_default_configured_project_worker(
        &mut self,
        file_name: &str,
        path: &str,
        config_file_name: &str,
        load_kind: LoadKind,
        visited: &mut BTreeSet<(String, LoadKind)>,
        fallback: Option<SearchResult>,
    ) -> Result<SearchResult, ProjectError> {
        struct Job {
            config_file_name: String,
            load_kind: LoadKind,
            parent: Option<usize>,
        }
        let paths = self.host.paths().clone();
        let mut jobs = vec![Job {
            config_file_name: config_file_name.to_owned(),
            load_kind,
            parent: None,
        }];
        let mut level = vec![0];
        let mut configs: BTreeMap<String, Arc<ConfigRootPlan>> = BTreeMap::new();
        let mut found = None;
        // The first project with the file as a referenced project's source.
        let mut level_result = None;
        while !level.is_empty() && found.is_none() {
            // A find is dropped when the level also creates the same config.
            let creates = level
                .iter()
                .filter(|&&job| jobs[job].load_kind == LoadKind::Create)
                .map(|&job| jobs[job].config_file_name.clone())
                .collect::<BTreeSet<_>>();
            level.retain(|&job| {
                jobs[job].load_kind == LoadKind::Create
                    || !creates.contains(&jobs[job].config_file_name)
            });
            let mut level_fallback = None;
            let mut next = Vec::new();
            for &job in &level {
                let key = (jobs[job].config_file_name.clone(), jobs[job].load_kind);
                if !visited.insert(key) {
                    continue;
                }
                let name = jobs[job].config_file_name.clone();
                let kind = jobs[job].load_kind;
                let (is_result, stop) = self.visit_search_node(path, &name, kind, &mut configs)?;
                if is_result {
                    if stop {
                        found = Some(job);
                        break;
                    }
                    if level_result.is_none() && level_fallback.is_none() {
                        level_fallback = Some(job);
                    }
                }
                // The references of the config, created only when the
                // config allows (tsgo `disableReferencedProjectLoad`).
                if let Some(config) = configs.get(&paths.to_path(&name)) {
                    let reference_kind =
                        if config.options().option_bool("disableReferencedProjectLoad")
                            == Some(true)
                        {
                            LoadKind::Find
                        } else {
                            kind
                        };
                    for reference in config.project_references().unwrap_or_default() {
                        let reference_name =
                            resolve_config_file_name_of_project_reference(reference.path.as_js())
                                .to_string_lossy()
                                .into_owned();
                        next.push(Job {
                            config_file_name: reference_name,
                            load_kind: reference_kind,
                            parent: Some(job),
                        });
                    }
                }
            }
            if level_result.is_none() {
                level_result = level_fallback;
            }
            let mut seen = BTreeSet::new();
            level = Vec::new();
            for job in next {
                if seen.insert((job.config_file_name.clone(), job.load_kind)) {
                    jobs.push(job);
                    level.push(jobs.len() - 1);
                }
            }
        }
        let result_path = |mut job: Option<usize>| {
            let mut names = Vec::new();
            while let Some(index) = job {
                names.push(jobs[index].config_file_name.clone());
                job = jobs[index].parent;
            }
            names
        };
        let result_names = result_path(found.or(level_result));
        let project = result_names
            .first()
            .and_then(|name| ProjectId::configured(&paths.to_path(name)))
            .filter(|id| self.configured.contains_key(id));
        let retain = result_names
            .iter()
            .map(|name| paths.to_path(name))
            .collect::<BTreeSet<_>>();
        if found.is_some() {
            return Ok(SearchResult { project, retain });
        }
        // A project with the file as a source of a referenced project.
        let fallback = match project {
            Some(project) => Some(SearchResult {
                project: Some(project),
                retain,
            }),
            None => fallback,
        };
        let disable_solution_searching =
            configs
                .get(&paths.to_path(config_file_name))
                .is_some_and(|config| {
                    config.options().option_bool("disableSolutionSearching") == Some(true)
                });
        if disable_solution_searching {
            if let Some(fallback) = fallback {
                return Ok(fallback);
            }
        }
        if let Some(ancestor) =
            self.configs
                .get_ancestor_config_file_name(file_name, path, config_file_name)
        {
            return self.find_or_create_default_configured_project_worker(
                file_name, path, &ancestor, load_kind, visited, fallback,
            );
        }
        if let Some(fallback) = fallback {
            return Ok(fallback);
        }
        // Nothing found: the whole graph was walked, so everything visited
        // is kept.
        Ok(SearchResult {
            project: None,
            retain: visited
                .iter()
                .map(|(name, _)| paths.to_path(name))
                .collect(),
        })
    }

    /// One config of the default-project search: whether its project has the
    /// file, and whether directly (tsgo's search `visit`).
    fn visit_search_node(
        &mut self,
        path: &str,
        config_file_name: &str,
        load_kind: LoadKind,
        configs: &mut BTreeMap<String, Arc<ConfigRootPlan>>,
    ) -> Result<(bool, bool), ProjectError> {
        let paths = self.host.paths().clone();
        let config_path = paths.to_path(config_file_name);
        let config = match load_kind {
            LoadKind::Find => self.configs.find_config(&config_path),
            LoadKind::Create => {
                self.configs
                    .acquire_config_for_file(config_file_name, &config_path, path)?
            }
        };
        let Some(config) = config else {
            return Ok((false, false));
        };
        configs.insert(config_path.clone(), Arc::clone(&config));
        // A config without root files (a solution) only leads to its
        // references.
        if config.file_names().is_empty() {
            return Ok((false, false));
        }
        // A composite project lists every file it has.
        if config.compiler_options().composite == Some(true)
            && !config
                .file_names()
                .iter()
                .any(|file| paths.to_path(&file.to_string_lossy()) == path)
        {
            return Ok((false, false));
        }
        let id = match load_kind {
            LoadKind::Find => match ProjectId::configured(&config_path) {
                Some(id) if self.configured.contains_key(&id) => id,
                _ => return Ok((false, false)),
            },
            LoadKind::Create => {
                let id = self.find_or_create_configured_project(config_file_name, &config_path);
                self.update_program(&id)?;
                id
            }
        };
        let Some(project) = self
            .project(&id)
            .filter(|project| project.contains_file(path))
        else {
            return Ok((false, false));
        };
        // A source of a referenced project is no direct inclusion: the
        // search goes on to the project that has it directly.
        Ok((true, !project.is_source_from_project_reference(path)))
    }

    /// tsgo `cleanupConfiguredProjects`: the configured projects no open
    /// file (the editor's, then the API's), open project or `retain` needs
    /// are deleted; an open file keeps its default project, the configs its
    /// program references, and (for an editor's open file) the projects of
    /// the configs above it. The open files without a configured project
    /// are the inferred project's roots.
    fn cleanup_configured_projects(
        &mut self,
        retain: Option<&BTreeSet<String>>,
    ) -> Result<(), ProjectError> {
        let paths = self.host.paths().clone();
        let mut to_remove = self
            .configured
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let mut inferred_project_files = Vec::new();
        let mut open_files = self
            .fs
            .overlays()
            .iter()
            .map(|(path, overlay)| (path.clone(), overlay.file_name().to_owned(), true))
            .collect::<Vec<_>>();
        open_files.extend(
            self.api_state
                .open_files
                .iter()
                .filter(|(path, _)| !self.is_open_file(path))
                .map(|(path, file)| (path.clone(), file.file_name.clone(), false)),
        );
        for (path, file_name, editor) in open_files {
            match self.find_default_configured_project(&file_name, &path)? {
                Some(id) => {
                    self.retain_configured_project_and_references(&id, &mut to_remove);
                    if editor {
                        // tsgo `retainDefaultConfiguredProject`: the projects
                        // of the configs above the file stay.
                        for config_file_name in self.configs.config_file_names_for(&path) {
                            let ancestor = ProjectId::configured(&paths.to_path(&config_file_name))
                                .filter(|ancestor| self.configured.contains_key(ancestor));
                            if let Some(ancestor) = ancestor {
                                self.retain_configured_project_and_references(
                                    &ancestor,
                                    &mut to_remove,
                                );
                            }
                        }
                    }
                }
                None => inferred_project_files.push(file_name),
            }
        }
        for path in to_remove {
            if retain.is_some_and(|retain| retain.contains(&path))
                || self.api_state.open_projects.contains_key(&path)
            {
                continue;
            }
            if let Some(id) = ProjectId::configured(&path) {
                if self.configured.contains_key(&id) {
                    self.delete_project(&id);
                }
            }
        }
        self.update_inferred_project_roots(inferred_project_files);
        self.configs.cleanup();
        Ok(())
    }

    /// tsgo `retainConfiguredProjectAndReferences`: the project and the
    /// projects of the configs its program references stay.
    fn retain_configured_project_and_references(
        &self,
        id: &ProjectId,
        to_remove: &mut BTreeSet<String>,
    ) {
        to_remove.remove(id.as_str());
        if let Some(program) = self.project(id).and_then(|project| project.program()) {
            for reference in program.references() {
                let configured = ProjectId::configured(reference)
                    .is_some_and(|reference| self.configured.contains_key(&reference));
                if configured {
                    to_remove.remove(reference);
                }
            }
        }
    }

    /// tsgo `findDefaultConfiguredProject` (builder): the default project
    /// found in this build, else the first configured project (by path)
    /// whose program has the file; several direct inclusions are settled by
    /// a search that creates nothing.
    fn find_default_configured_project(
        &mut self,
        file_name: &str,
        path: &str,
    ) -> Result<Option<ProjectId>, ProjectError> {
        if let Some(id) = self.file_default_projects.get(path) {
            if id.kind() == Some(ProjectKind::Configured) && self.configured.contains_key(id) {
                return Ok(Some(id.clone()));
            }
        }
        let (project, several_direct) =
            default_project_from_program_inclusion(self.configured.values(), path);
        let project = project.map(|project| project.id.clone());
        if several_direct {
            if let Some(project) = self
                .find_or_create_default_configured_project_for_file(
                    file_name,
                    path,
                    LoadKind::Find,
                )?
                .project
            {
                return Ok(Some(project));
            }
        }
        Ok(project)
    }

    /// tsgo `findDefaultProject`: the default configured project, else the
    /// inferred project when it has the file.
    fn find_default_project(
        &mut self,
        file_name: &str,
        path: &str,
    ) -> Result<Option<ProjectId>, ProjectError> {
        if let Some(id) = self.find_default_configured_project(file_name, path)? {
            return Ok(Some(id));
        }
        if self
            .file_default_projects
            .get(path)
            .is_some_and(|id| id.kind() == Some(ProjectKind::Inferred))
        {
            return Ok(self.inferred.as_ref().map(|project| project.id.clone()));
        }
        if self
            .inferred
            .as_ref()
            .is_some_and(|project| project.contains_file(path))
        {
            self.file_default_projects
                .insert(path.to_owned(), ProjectId::inferred());
            return Ok(Some(ProjectId::inferred()));
        }
        Ok(None)
    }

    /// tsgo `ensureInferredProjectIncludesClosedFile`: the inferred project's
    /// roots are the opened files without a configured project and this
    /// file (an opened file can so be a root twice, as in tsgo), and its
    /// program is brought up to date.
    fn ensure_inferred_project_includes_closed_file(
        &mut self,
        file_name: &str,
    ) -> Result<(), ProjectError> {
        let mut roots = self.collect_inferred_project_roots()?;
        roots.push(file_name.to_owned());
        self.update_inferred_project_roots(roots);
        if self.inferred.is_some() {
            self.update_program(&ProjectId::inferred())?;
        }
        Ok(())
    }

    /// tsgo `collectInferredProjectRoots` (with `appendAPIOpenedInferredRoots`):
    /// the editor's open files, then the API's, without a default configured
    /// project.
    fn collect_inferred_project_roots(&mut self) -> Result<Vec<String>, ProjectError> {
        let mut open_files = self
            .fs
            .overlays()
            .iter()
            .map(|(path, overlay)| (path.clone(), overlay.file_name().to_owned()))
            .collect::<Vec<_>>();
        open_files.extend(
            self.api_state
                .open_files
                .iter()
                .filter(|(path, _)| !self.is_open_file(path))
                .map(|(path, file)| (path.clone(), file.file_name.clone())),
        );
        let mut roots = Vec::new();
        for (path, file_name) in open_files {
            if self
                .find_default_configured_project(&file_name, &path)?
                .is_none()
            {
                roots.push(file_name);
            }
        }
        Ok(roots)
    }

    /// tsgo `updateInferredProjectRoots`: the supported roots with the
    /// session's options for inferred projects.
    fn update_inferred_project_roots(&mut self, roots: Vec<String>) {
        let roots = roots
            .into_iter()
            .filter(|root| self.is_supported_in_inferred_project(root))
            .collect::<Vec<_>>();
        let options = self.compiler_options_for_inferred_projects.clone();
        self.update_inferred_project(roots, options.as_deref());
    }

    /// tsgo `DidChangeCompilerOptionsForInferredProjects`' step of `Clone`:
    /// the inferred project, if any, takes the new options.
    pub(crate) fn did_change_compiler_options_for_inferred_projects(
        &mut self,
        options: &ConfigOptionBag,
    ) {
        if let Some(project) = &self.inferred {
            let roots = project.root_file_names();
            self.update_inferred_project(roots, Some(options));
        }
    }

    /// tsgo `updateInferredProject` and `updateOrCreateInferredProject`: no
    /// root deletes the inferred project; otherwise its sorted roots and its
    /// options (the project's own when none are given) replace the old ones
    /// when they differ.
    fn update_inferred_project(
        &mut self,
        mut roots: Vec<String>,
        options: Option<&ConfigOptionBag>,
    ) {
        if roots.is_empty() {
            if self.inferred.is_some() {
                self.delete_project(&ProjectId::inferred());
            }
            return;
        }
        roots.sort();
        let current_directory = self.host.options().current_directory.clone();
        let Some(project) = &self.inferred else {
            let roots = inferred_project_roots(roots, options, self.host.paths());
            self.insert(self.with_watches(Project::new_inferred(current_directory, roots)));
            return;
        };
        let current = match project.command_line() {
            Some(CommandLine::Roots(roots)) => Some(&roots.options),
            _ => None,
        };
        let options = options.or(current);
        if project.root_file_names() != roots || options != current {
            let roots = Arc::new(inferred_project_roots(roots, options, self.host.paths()));
            self.change(&ProjectId::inferred(), |project| {
                project.set_command_line(CommandLine::Roots(roots));
            });
        }
    }

    /// tsgo `DidChangeFiles` (projectcollectionbuilder.go:397-472): the
    /// configs a change concerns are marked, a project whose program read a
    /// changed, deleted or created file becomes dirty, a closed file leaves
    /// the inferred project's roots, and an opened file gets its configured
    /// project (and the configs above it) while the projects no open file
    /// needs go.
    pub(crate) fn did_change_files(
        &mut self,
        changes: &FileChangeSummary,
    ) -> Result<(), ProjectError> {
        self.open_files_changed =
            self.open_files_changed || changes.opened.is_some() || !changes.closed.is_empty();
        let paths = self.host.paths().clone();
        let to_paths = |files: &BTreeSet<String>| {
            files
                .iter()
                .map(|file| paths.to_path(file))
                .collect::<Vec<_>>()
        };
        let changed = to_paths(&changes.changed);
        let deleted = to_paths(&changes.deleted);
        let created = to_paths(&changes.created);
        if self.host.content_mapper_host().is_some() {
            let refresh_all = changes.has_excessive_non_create_watch_events();
            for id in self.project_ids() {
                self.refresh_content_mapper_project_for_changes(
                    &id,
                    &[&changed, &deleted, &created],
                    refresh_all,
                );
            }
        }

        let config_changes = self.configs.did_change_files(changes);
        self.program_structure_changed =
            self.mark_projects_affected_by_config_changes(config_changes)?;

        let excessive = changes.has_excessive_non_create_watch_events();
        for id in self.project_ids() {
            if excessive {
                self.change(&id, |project| {
                    project.dirty = true;
                    project.dirty_file_path = None;
                });
                continue;
            }
            self.mark_files_changed(&id, &changed, FileChange::Changed);
            if id.kind() == Some(ProjectKind::Inferred) && !changes.closed.is_empty() {
                let closed = changes
                    .closed
                    .iter()
                    .map(|file_name| paths.to_path(file_name))
                    .collect::<BTreeSet<_>>();
                let roots = self
                    .project(&id)
                    .map(|project| project.root_file_names())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|root| !closed.contains(&paths.to_path(root)))
                    .collect();
                self.update_inferred_project_roots(roots);
            }
            if !deleted.is_empty() {
                self.mark_files_changed(&id, &deleted, FileChange::Deleted);
            }
            if !created.is_empty() {
                self.mark_files_changed(&id, &created, FileChange::Created);
            }
        }

        if let Some(file_name) = changes.opened.as_ref().or(changes.reopened.as_ref()) {
            let path = paths.to_path(file_name);
            let result = self.ensure_configured_project_and_ancestors_for_file(file_name, &path)?;
            self.cleanup_configured_projects(Some(&result.retain))?;
        }
        Ok(())
    }

    /// tsgo `markProjectsAffectedByConfigChanges`: the projects using a
    /// changed config become dirty, and the open files whose config names
    /// may have changed get their configured projects again; whether any
    /// did.
    fn mark_projects_affected_by_config_changes(
        &mut self,
        config_changes: crate::config::ChangeFileResult,
    ) -> Result<bool, ProjectError> {
        for id in config_changes.affected_projects {
            if self
                .project(&id)
                .is_some_and(|project| !project.dirty || project.dirty_file_path.is_some())
            {
                self.change(&id, |project| {
                    project.dirty = true;
                    project.dirty_file_path = None;
                });
            }
        }
        let mut has_changes = false;
        for path in config_changes.affected_files {
            let Some(overlay) = self.fs.overlays().get(&path).cloned() else {
                continue;
            };
            self.ensure_configured_project_and_ancestors_for_file(overlay.file_name(), &path)?;
            has_changes = true;
        }
        Ok(has_changes)
    }

    /// tsgo `DidChangeCustomConfigFileName`: every open file's default
    /// project is searched for again.
    pub(crate) fn did_change_custom_config_file_name(&mut self) {
        if !self.configs.did_change_custom_config_file_name() {
            return;
        }
        self.file_default_projects.clear();
        self.default_projects_invalidated = true;
        self.program_structure_changed = true;
    }

    /// tsgo `DidChangeUserPreferences`: a new locale makes every project
    /// dirty.
    pub(crate) fn did_change_user_preferences(
        &mut self,
        old: &tsc_ls::lsutil::UserPreferences,
        new: &tsc_ls::lsutil::UserPreferences,
    ) {
        if old.locale == new.locale {
            return;
        }
        for id in self.project_ids() {
            self.change(&id, |project| {
                project.dirty = true;
                project.dirty_file_path = None;
            });
        }
    }

    /// tsgo `DidRequestFile`: the projects of a file a request needs are
    /// loaded and brought up to date; a file no editor has open goes to the
    /// inferred project when no configured project has it (unless
    /// `configured_projects_only`).
    pub(crate) fn did_request_file(
        &mut self,
        file_name: &str,
        configured_projects_only: bool,
    ) -> Result<(), ProjectError> {
        let path = self.host.paths().to_path(file_name);
        if self.default_projects_invalidated {
            self.ensure_configured_project_and_ancestors_for_file(file_name, &path)?;
            if !self.is_open_file(&path) {
                return Ok(());
            }
        }
        if self.is_open_file(&path) {
            let mut has_changes = self.program_structure_changed;
            // The default project, if it can be found without updating
            // anything else.
            if let Some(id) = self.find_default_project(file_name, &path)? {
                has_changes = self.update_program(&id)? || has_changes;
                if self
                    .project(&id)
                    .is_some_and(|project| project.contains_file(&path))
                {
                    if has_changes {
                        self.cleanup_inferred_project()?;
                        if self.inferred.is_some() {
                            self.update_program(&ProjectId::inferred())?;
                        }
                    }
                    return Ok(());
                }
            }
            // Every known project brought up to date.
            for id in self.configured.keys().cloned().collect::<Vec<_>>() {
                has_changes = self.update_program(&id)? || has_changes;
            }
            if has_changes {
                // Files may move in or out of the inferred project.
                self.cleanup_inferred_project()?;
            }
            if self.inferred.is_some() {
                self.update_program(&ProjectId::inferred())?;
            }
        } else {
            let result = self.ensure_configured_project_and_ancestors_for_file(file_name, &path)?;
            if result.project.is_none() && !configured_projects_only {
                self.ensure_inferred_project_includes_closed_file(file_name)?;
            }
        }
        Ok(())
    }

    /// tsgo `ensureConfiguredProjectAndAncestorsForFile`: the file's default
    /// configured project, created when missing, and for an open file the
    /// projects of the configs above it.
    fn ensure_configured_project_and_ancestors_for_file(
        &mut self,
        file_name: &str,
        path: &str,
    ) -> Result<SearchResult, ProjectError> {
        let mut result = self.find_or_create_default_configured_project_for_file(
            file_name,
            path,
            LoadKind::Create,
        )?;
        if result.project.is_some() && self.is_open_file(path) {
            self.create_ancestor_tree(file_name, path, &mut result)?;
        }
        Ok(result)
    }

    /// tsgo `createAncestorTree`: the projects of the configs above a
    /// composite project, created to load later (a solution's), kept with
    /// the open file's result.
    fn create_ancestor_tree(
        &mut self,
        file_name: &str,
        path: &str,
        result: &mut SearchResult,
    ) -> Result<(), ProjectError> {
        let paths = self.host.paths().clone();
        let Some(mut id) = result.project.clone() else {
            return Ok(());
        };
        loop {
            let Some(project) = self.project(&id).cloned() else {
                return Ok(());
            };
            // Only a composite project searches for its solution.
            if let Some(command_line) = project.config_plan() {
                if command_line.compiler_options().composite != Some(true)
                    || command_line
                        .options()
                        .option_bool("disableSolutionSearching")
                        == Some(true)
                {
                    return Ok(());
                }
            }
            let Some(config_file_name) = project.config_file_name.clone() else {
                return Ok(());
            };
            let Some(ancestor_name) =
                self.configs
                    .get_ancestor_config_file_name(file_name, path, &config_file_name)
            else {
                return Ok(());
            };
            let ancestor_path = paths.to_path(&ancestor_name);
            let ancestor = self.find_or_create_configured_project(&ancestor_name, &ancestor_path);
            result.retain.insert(ancestor_path);
            // A new ancestor (not loaded yet) records this project as one it
            // may reference.
            let composite = project
                .config_plan()
                .is_none_or(|command_line| command_line.compiler_options().composite == Some(true));
            if self
                .project(&ancestor)
                .is_some_and(|ancestor| ancestor.command_line.is_none())
                && composite
            {
                let reference = project.config_file_path.clone();
                self.change(&ancestor, |ancestor| {
                    ancestor.set_potential_project_reference(reference);
                });
            }
            id = ancestor;
        }
    }

    /// tsgo `DidRequestProjectTrees` for every project: each configured
    /// project's program brought up to date with the projects it
    /// references, then the inferred project's (the configured projects may
    /// have taken or left its files).
    pub(crate) fn did_request_project_trees(&mut self) -> Result<(), ProjectError> {
        let mut seen = BTreeSet::new();
        for id in self.configured.keys().cloned().collect::<Vec<_>>() {
            if !self.configured.contains_key(&id) {
                continue;
            }
            self.update_program(&id)?;
            self.ensure_project_tree(&id, &mut seen)?;
        }
        self.cleanup_inferred_project()?;
        if self.inferred.is_some() {
            self.update_program(&ProjectId::inferred())?;
        }
        Ok(())
    }

    /// tsgo `ensureProjectTree`: the projects of the configs a project's
    /// program references, loaded and brought up to date, down the
    /// references (none below a project that disables referenced project
    /// loads).
    fn ensure_project_tree(
        &mut self,
        id: &ProjectId,
        seen: &mut BTreeSet<ProjectId>,
    ) -> Result<(), ProjectError> {
        if !seen.insert(id.clone()) {
            return Ok(());
        }
        let Some(project) = self.project(id).cloned() else {
            return Ok(());
        };
        let Some(program) = project.program.clone() else {
            return Ok(());
        };
        if project.config_plan().is_some_and(|plan| {
            plan.options().option_bool("disableReferencedProjectLoad") == Some(true)
        }) {
            return Ok(());
        }
        for path in program.references() {
            let Some(config) = self.configs.find_config(path) else {
                continue;
            };
            let file_name = config.config_file_name().to_string_lossy().into_owned();
            let child = self.find_or_create_configured_project(&file_name, path);
            self.update_program(&child)?;
            self.ensure_project_tree(&child, seen)?;
        }
        Ok(())
    }

    /// tsgo `cleanupInferredProject`.
    fn cleanup_inferred_project(&mut self) -> Result<(), ProjectError> {
        let roots = self.collect_inferred_project_roots()?;
        self.update_inferred_project_roots(roots);
        Ok(())
    }

    /// tsgo `markFilesChanged`: one changed file of the program is kept as
    /// the dirty file (a later build may reuse the program); a deletion, a
    /// `package.json`, a second file or a file the build only looked for
    /// needs a full build.
    fn mark_files_changed(&mut self, id: &ProjectId, paths: &[String], change: FileChange) {
        let Some(project) = self.project(id) else {
            return;
        };
        if project.program.is_none() || (project.dirty && project.dirty_file_path.is_none()) {
            return;
        }
        let mut dirty = false;
        let mut dirty_file_path = project.dirty_file_path.clone();
        for path in paths {
            if project.contains_file(path) {
                dirty = true;
                if change == FileChange::Deleted || path.rsplit('/').next() == Some("package.json")
                {
                    dirty_file_path = None;
                    break;
                }
                match &dirty_file_path {
                    None => dirty_file_path = Some(path.clone()),
                    Some(current) if current != path => {
                        dirty_file_path = None;
                        break;
                    }
                    Some(_) => {}
                }
            } else if match change {
                FileChange::Created => project.seen.seen_file_or_missing_parent_directory(path),
                FileChange::Changed | FileChange::Deleted => project.seen.seen_file(path),
            } {
                dirty = true;
                dirty_file_path = None;
                break;
            }
        }
        if dirty || project.dirty_file_path != dirty_file_path {
            self.change(id, |project| {
                project.dirty = true;
                project.dirty_file_path = dirty_file_path;
            });
        }
    }
}

impl ProjectCollectionBuilder<'_> {
    /// tsgo `isSupportedInInferredProject`: a dynamic name, one with a script
    /// kind (tsgo `GetScriptKindFromFileName`), or an open document without
    /// an extension.
    fn is_supported_in_inferred_project(&self, file_name: &str) -> bool {
        if file_name.starts_with("^/") {
            return true;
        }
        let extension = file_name
            .rsplit('/')
            .next()
            .and_then(|name| name.rfind('.').map(|index| &name[index..]));
        if extension.is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                ".js" | ".cjs" | ".mjs" | ".jsx" | ".ts" | ".cts" | ".mts" | ".tsx" | ".json"
            )
        }) {
            return true;
        }
        extension.is_none() && self.is_open_file(&self.host.paths().to_path(file_name))
    }
}
