//! A snapshot build's projects (tsgo `ProjectCollectionBuilder`,
//! projectcollectionbuilder.go): the base snapshot's projects, changed by
//! file changes and an API request. A project is copied out of the base
//! snapshot the first time the build changes it, so what the build leaves
//! alone stays shared.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;

use tsc_compiler::LiveProgram;
use tsc_host::vfs::VfsCompilerHost;
use tsc_program::{
    load_config_program, load_emitting_config_program, load_emitting_program, load_program,
    resolve_config_file_name_of_project_reference, ConfigRootPlan,
};

use crate::config::{ConfigFileRegistry, ConfigFileRegistryBuilder};
use crate::fs::{FileChangeSummary, SeenFiles, SnapshotFsBuilder, SourceFs};
use crate::id::{ProjectId, ProjectKind};
use crate::project::{
    inferred_project_roots, CommandLine, ProgramRoots, ProgramUpdateKind, Project, ProjectProgram,
};
use crate::snapshot::{
    ApiSnapshotRequest, ApiState, CreateProgramRequest, OpenedFile, ProjectCollection,
    ProjectError, SnapshotHost,
};

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
}

impl<'a> ProjectCollectionBuilder<'a> {
    pub(crate) fn new(
        host: &'a SnapshotHost,
        fs: &'a SnapshotFsBuilder,
        snapshot_id: u64,
        base: &'a Arc<ProjectCollection>,
        configs: ConfigFileRegistryBuilder<'a>,
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
        }
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
            && self.file_default_projects == self.base.file_default_projects
            && self.api_state == self.base.api_state
        {
            Arc::clone(self.base)
        } else {
            Arc::new(ProjectCollection {
                configured: self.configured,
                synthetic: self.synthetic,
                inferred: self.inferred,
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
            self.update_or_create_synthetic_project(&id, program);
            created.push(id);
        }
        let mut updated = created.clone();
        for program in &request.reconfigure_programs {
            let id = program.program_id.canonical();
            self.update_or_create_synthetic_project(&id, &program.program);
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
            let directory = file_name
                .rsplit_once('/')
                .map_or_else(String::new, |(directory, _)| directory.to_owned());
            self.insert(Project::new_configured(file_name, path, directory));
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
    ) {
        let roots = ProgramRoots {
            root_file_names: program.root_file_names.clone(),
            compiler_options: program.compiler_options.clone(),
            program_options: program.program_options.clone(),
            config_file_parsing_diagnostics: program.config_file_parsing_diagnostics.clone(),
        };
        let Some(project) = self.synthetic.get(id) else {
            let current_directory = self.host.options().current_directory.clone();
            self.insert(Project::new_synthetic(
                id.clone(),
                current_directory,
                Arc::new(roots),
            ));
            return;
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
        let (program, seen) = self.create_program(&project)?;
        let update_kind = match &project.program {
            Some(old) if old.has_same_file_names(&program) => ProgramUpdateKind::SameFileNames,
            _ => ProgramUpdateKind::NewFiles,
        };
        let snapshot_id = self.snapshot_id;
        self.change(id, |project| {
            project.program = Some(Arc::new(program));
            project.program_update_kind = update_kind;
            project.program_last_update = snapshot_id;
            project.seen = Arc::new(seen);
            project.dirty = false;
            project.dirty_file_path = None;
        });
        Ok(update_kind == ProgramUpdateKind::NewFiles)
    }

    /// tsgo `CreateProgram` (`NewProgram` over the build's files): the
    /// program and what its build looked for.
    fn create_program(
        &self,
        project: &Project,
    ) -> Result<(ProjectProgram, SeenFiles), ProjectError> {
        let options = self.host.options();
        let source = SourceFs::tracking(self.fs);
        let host = VfsCompilerHost::new(&source, project.current_directory.clone());
        let failed = |error: &dyn std::fmt::Display| {
            ProjectError::new(format!(
                "cannot create the program of {}: {error}",
                project.id
            ))
        };
        let prepared = match project.command_line.as_ref() {
            Some(CommandLine::Config(plan)) => if plan.compiler_options().no_emit == Some(true) {
                load_config_program(&host, plan, &options.library_catalog, options.load_limits)
            } else {
                load_emitting_config_program(
                    &host,
                    plan,
                    &options.library_catalog,
                    options.load_limits,
                )
            }
            .map_err(|error| failed(&format!("{error:?}")))?,
            Some(CommandLine::Roots(roots)) => {
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
                load(
                    &host,
                    &root_names,
                    roots.compiler_options.clone(),
                    roots.program_options.clone(),
                    &options.library_catalog,
                    options.load_limits,
                )
                .map_err(|error| failed(&error))?
            }
            None => return Err(failed(&"the project has no command line")),
        };
        drop(host);
        let live = LiveProgram::new(prepared).map_err(|error| failed(&error))?;
        Ok((ProjectProgram::new(live), source.into_seen()))
    }

    /// tsgo `deleteProject`: a configured project releases its config.
    fn delete_project(&mut self, id: &ProjectId) {
        if let Some(path) = self
            .project(id)
            .and_then(|project| project.config_file_path.clone())
        {
            self.configs.release_config_for_project(&path, id);
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
                    if !is_supported_in_inferred_project(file_name) {
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
        let Some(config_file_name) = self.config_file_name_for_file(file_name) else {
            return Ok(SearchResult::default());
        };
        let mut visited = BTreeSet::new();
        let result = self.find_or_create_default_configured_project_worker(
            path,
            &config_file_name,
            load_kind,
            &mut visited,
        )?;
        if let Some(project) = &result.project {
            self.file_default_projects
                .insert(path.to_owned(), project.clone());
        }
        Ok(result)
    }

    /// tsgo `getConfigFileNameForFile` for a file no editor has open
    /// (`computeConfigFileName`): the nearest `tsconfig.json`, else
    /// `jsconfig.json`, in the file's directory and above it, not past a
    /// `node_modules` directory.
    fn config_file_name_for_file(&self, file_name: &str) -> Option<String> {
        if file_name.starts_with("^/") {
            return None;
        }
        let mut directory = self.host.paths().absolute(file_name);
        loop {
            directory = match directory.rfind('/') {
                Some(0) if directory.len() > 1 => "/".to_owned(),
                Some(index) if index > 0 => directory[..index].to_owned(),
                _ => return None,
            };
            for config in ["tsconfig.json", "jsconfig.json"] {
                let candidate = format!("{}/{config}", directory.trim_end_matches('/'));
                if self.fs.file_exists(&candidate) {
                    return Some(candidate);
                }
            }
            if directory.ends_with("/node_modules") || directory == "/" {
                return None;
            }
        }
    }

    /// tsgo `findOrCreateDefaultConfiguredProjectWorker`: a breadth-first
    /// search from the config through the project references for a project
    /// whose program has the file. tsgo visits a level's configs in
    /// parallel and keeps the lowest one that has the file; visiting them in
    /// order finds the same one. A file the API opened has no search above
    /// the config (no solution search).
    fn find_or_create_default_configured_project_worker(
        &mut self,
        path: &str,
        config_file_name: &str,
        load_kind: LoadKind,
        visited: &mut BTreeSet<(String, LoadKind)>,
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
        let mut fallback = None;
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
                    if fallback.is_none() && level_fallback.is_none() {
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
            if fallback.is_none() {
                fallback = level_fallback;
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
        let result_names = result_path(found.or(fallback));
        let project = result_names
            .first()
            .and_then(|name| ProjectId::configured(&paths.to_path(name)))
            .filter(|id| self.configured.contains_key(id));
        let mut retain = result_names
            .iter()
            .map(|name| paths.to_path(name))
            .collect::<BTreeSet<_>>();
        if found.is_none() && project.is_none() {
            // Nothing found: the whole graph was walked, so everything
            // visited is kept.
            retain.extend(visited.iter().map(|(name, _)| paths.to_path(name)));
        }
        Ok(SearchResult { project, retain })
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
            LoadKind::Create => self
                .configs
                .acquire_config_for_file(config_file_name, &config_path)?,
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
        let contains = self
            .project(&id)
            .is_some_and(|project| project.contains_file(path));
        // A source of a referenced project is no direct inclusion; the port's
        // programs read the references' outputs, so every inclusion is direct.
        Ok((contains, contains))
    }

    /// tsgo `cleanupConfiguredProjects`: the configured projects no opened
    /// file, open project or `retain` needs are deleted; the opened files
    /// without a configured project are the inferred project's roots.
    fn cleanup_configured_projects(
        &mut self,
        retain: Option<&BTreeSet<String>>,
    ) -> Result<(), ProjectError> {
        let mut to_remove = self
            .configured
            .keys()
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let mut inferred_project_files = Vec::new();
        let opened = self
            .api_state
            .open_files
            .iter()
            .map(|(path, file)| (path.clone(), file.file_name.clone()))
            .collect::<Vec<_>>();
        for (path, file_name) in opened {
            match self.find_default_configured_project(&file_name, &path)? {
                // The references a program reads are kept too (P5-1b-3).
                Some(id) => {
                    to_remove.remove(id.as_str());
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
        let containing = self
            .configured
            .values()
            .filter(|project| project.contains_file(path))
            .map(|project| project.id.clone())
            .collect::<Vec<_>>();
        if containing.len() > 1 {
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
        Ok(containing.into_iter().next())
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

    /// tsgo `collectInferredProjectRoots` (`appendAPIOpenedInferredRoots`):
    /// the opened files without a default configured project.
    fn collect_inferred_project_roots(&mut self) -> Result<Vec<String>, ProjectError> {
        let opened = self
            .api_state
            .open_files
            .iter()
            .map(|(path, file)| (path.clone(), file.file_name.clone()))
            .collect::<Vec<_>>();
        let mut roots = Vec::new();
        for (path, file_name) in opened {
            if self
                .find_default_configured_project(&file_name, &path)?
                .is_none()
            {
                roots.push(file_name);
            }
        }
        Ok(roots)
    }

    /// tsgo `updateInferredProjectRoots` and `updateInferredProject`: no
    /// supported root deletes the inferred project; otherwise its sorted
    /// roots replace the old ones when they differ.
    fn update_inferred_project_roots(&mut self, roots: Vec<String>) {
        let mut roots = roots
            .into_iter()
            .filter(|root| is_supported_in_inferred_project(root))
            .collect::<Vec<_>>();
        if roots.is_empty() {
            if self.inferred.is_some() {
                self.delete_project(&ProjectId::inferred());
            }
            return;
        }
        roots.sort();
        let current_directory = self.host.options().current_directory.clone();
        let Some(project) = &self.inferred else {
            self.insert(Project::new_inferred(current_directory, roots));
            return;
        };
        if project.root_file_names() != roots {
            let roots = Arc::new(inferred_project_roots(roots));
            self.change(&ProjectId::inferred(), |project| {
                project.set_command_line(CommandLine::Roots(roots));
            });
        }
    }

    /// tsgo `DidChangeFiles` (projectcollectionbuilder.go:388-463) without
    /// open files: the configs a change concerns are marked, and a project
    /// whose program read a changed, deleted or created file becomes dirty.
    pub(crate) fn did_change_files(&mut self, changes: &FileChangeSummary) {
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

        for id in self.configs.did_change_files(changes) {
            // tsgo `markProjectsAffectedByConfigChanges`.
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
            if !deleted.is_empty() {
                self.mark_files_changed(&id, &deleted, FileChange::Deleted);
            }
            if !created.is_empty() {
                self.mark_files_changed(&id, &created, FileChange::Created);
            }
        }
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

/// tsgo `isSupportedInInferredProject` for a file no editor has open: a
/// dynamic name or one with a script kind (tsgo `GetScriptKindFromFileName`).
fn is_supported_in_inferred_project(file_name: &str) -> bool {
    if file_name.starts_with("^/") {
        return true;
    }
    file_name.rfind('.').is_some_and(|index| {
        matches!(
            file_name[index..].to_ascii_lowercase().as_str(),
            ".js" | ".cjs" | ".mjs" | ".jsx" | ".ts" | ".cts" | ".mts" | ".tsx" | ".json"
        )
    })
}
