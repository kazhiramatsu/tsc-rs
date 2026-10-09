//! A snapshot build's projects (tsgo `ProjectCollectionBuilder`,
//! projectcollectionbuilder.go): the base snapshot's projects, changed by
//! file changes and an API request. A project is copied out of the base
//! snapshot the first time the build changes it, so what the build leaves
//! alone stays shared.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use tsc_compiler::LiveProgram;
use tsc_host::vfs::VfsCompilerHost;
use tsc_program::{
    load_config_program, load_emitting_config_program, load_emitting_program, load_program,
};

use crate::config::{ConfigFileRegistry, ConfigFileRegistryBuilder};
use crate::fs::{FileChangeSummary, SeenFiles, SnapshotFsBuilder, SourceFs};
use crate::id::{ProjectId, ProjectKind};
use crate::project::{CommandLine, ProgramRoots, ProgramUpdateKind, Project, ProjectProgram};
use crate::snapshot::{
    ApiSnapshotRequest, ApiState, CreateProgramRequest, ProjectCollection, ProjectError,
    SnapshotHost,
};

type Projects = Arc<std::collections::BTreeMap<ProjectId, Arc<Project>>>;

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
            api_state: base.api_state.clone(),
            created_programs: Vec::new(),
        }
    }

    /// tsgo `Finalize`: the base snapshot's collection when nothing changed.
    pub(crate) fn finish(self) -> (Arc<ProjectCollection>, ConfigFileRegistry, Vec<ProjectId>) {
        let configs = self.configs.finish();
        let projects = if Arc::ptr_eq(&self.configured, &self.base.configured)
            && Arc::ptr_eq(&self.synthetic, &self.base.synthetic)
            && self.api_state == self.base.api_state
        {
            Arc::clone(self.base)
        } else {
            Arc::new(ProjectCollection {
                configured: self.configured,
                synthetic: self.synthetic,
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
            Some(ProjectKind::Synthetic) => self.synthetic.get(id),
            _ => self.configured.get(id),
        }
    }

    fn insert(&mut self, project: Project) {
        let id = project.id.clone();
        Arc::make_mut(self.projects_of(&id)).insert(id, Arc::new(project));
    }

    /// Change the project, copied out of the snapshots that share it the
    /// first time (tsgo's dirty map `Change`).
    fn change(&mut self, id: &ProjectId, change: impl FnOnce(&mut Project)) {
        let projects = Arc::make_mut(self.projects_of(id));
        let entry = projects
            .get_mut(id)
            .unwrap_or_else(|| panic!("project {id} not found"));
        if Arc::strong_count(entry) > 1 {
            *entry = Arc::new(entry.cloned_for_change());
        }
        change(Arc::get_mut(entry).expect("the project is not shared"));
    }

    fn project_ids(&self) -> Vec<ProjectId> {
        self.configured
            .keys()
            .chain(self.synthetic.keys())
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

        for path in projects_to_close {
            if let Some(id) = ProjectId::configured(&path) {
                if self.configured.contains_key(&id) {
                    self.delete_project(&id);
                }
            }
        }

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
        Arc::make_mut(self.projects_of(id)).remove(id);
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
