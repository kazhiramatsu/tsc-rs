//! The configs a snapshot has parsed (tsgo `ConfigFileRegistry`,
//! configfileregistry.go and configfileregistrybuilder.go): one entry per
//! config path with its parse, the projects that use it and the configs that
//! extend it. A parse is redone lazily, when a project acquires an entry a
//! file change has marked.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use tsc_diagnostics::{JsStr, JsString};
use tsc_host::vfs::VfsCompilerHost;
use tsc_program::{
    command_line_option_bag, parse_config_root_plan_with_cache,
    parse_config_root_plan_with_command_line, CompilerConfigHost, ConfigExtendedCache,
    ConfigRootPlan, ConfigRootPlanRequest, JsonValue,
};

use crate::fs::{FileChangeSummary, SnapshotFsBuilder, SourceFs};
use crate::id::ProjectId;
use crate::snapshot::ProjectError;

/// What an entry's parse needs before use (tsgo `PendingReload`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingReload {
    None,
    /// The include patterns are matched again.
    FileNames,
    /// The config is parsed again.
    Full,
}

/// One config (tsgo `configFileEntry`).
#[derive(Clone, Debug)]
pub(crate) struct ConfigFileEntry {
    file_name: String,
    pending_reload: PendingReload,
    /// The parse; none when the config cannot be read.
    plan: Option<Arc<ConfigRootPlan>>,
    retaining_projects: BTreeSet<ProjectId>,
    /// The open files whose default-project search reached this config (by
    /// path).
    retaining_open_files: BTreeSet<String>,
    /// The configs that extend this one (by path).
    retaining_configs: BTreeSet<String>,
    /// A project's config, whose roots a created file can add to (tsgo's
    /// `rootFilesWatch`, absent for an entry only extended).
    matches_root_files: bool,
}

impl ConfigFileEntry {
    /// tsgo `newConfigFileEntry`: parsed at its first use.
    fn new(file_name: &str) -> Self {
        Self {
            file_name: file_name.to_owned(),
            pending_reload: PendingReload::Full,
            plan: None,
            retaining_projects: BTreeSet::new(),
            retaining_open_files: BTreeSet::new(),
            retaining_configs: BTreeSet::new(),
            matches_root_files: true,
        }
    }
}

type Configs = Arc<BTreeMap<String, Arc<ConfigFileEntry>>>;

/// tsgo `configFileNames`: an open file's nearest config, and for each
/// config on the way up the config above it (none when there is none).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ConfigFileNames {
    nearest: Option<String>,
    ancestors: BTreeMap<String, Option<String>>,
}

type ConfigFileNameCache = Arc<BTreeMap<String, ConfigFileNames>>;

/// A snapshot's configs (tsgo `ConfigFileRegistry`).
#[derive(Clone, Debug, Default)]
pub(crate) struct ConfigFileRegistry {
    configs: Configs,
    /// The config names of the open files, by path.
    config_file_names: ConfigFileNameCache,
    /// tsgo `customConfigFileName`: the user's config file name, looked
    /// for before `tsconfig.json` and `jsconfig.json`.
    custom_config_file_name: String,
}

/// What a file change did to the configs (tsgo `changeFileResult`): the
/// projects using a changed config, and the open files whose config names
/// may have changed.
#[derive(Debug, Default)]
pub(crate) struct ChangeFileResult {
    pub(crate) affected_projects: BTreeSet<ProjectId>,
    pub(crate) affected_files: BTreeSet<String>,
}

impl ConfigFileRegistry {
    /// tsgo `GetConfig`: the parse of the config at `path`.
    pub(crate) fn config(&self, path: &str) -> Option<&Arc<ConfigRootPlan>> {
        self.configs.get(path)?.plan.as_ref()
    }

    /// tsgo `customConfigFileName`.
    pub(crate) fn custom_config_file_name(&self) -> &str {
        &self.custom_config_file_name
    }

    /// tsgo `GetConfigFileName`: an open file's nearest config.
    pub(crate) fn config_file_name(&self, path: &str) -> Option<&str> {
        self.config_file_names.get(path)?.nearest.as_deref()
    }

    /// tsgo `GetAncestorConfigFileName`: the config above
    /// `config_file_name` an open file's search found.
    pub(crate) fn ancestor_config_file_name(
        &self,
        path: &str,
        config_file_name: &str,
    ) -> Option<&str> {
        self.config_file_names
            .get(path)?
            .ancestors
            .get(config_file_name)?
            .as_deref()
    }

    /// tsgo `collectConfiguredContentMappers`: the extensions of every
    /// parse's content mappers, sorted, each once.
    pub(crate) fn content_mapper_extensions(&self) -> Vec<String> {
        let mut extensions = self
            .configs
            .values()
            .filter_map(|entry| entry.plan.as_deref())
            .flat_map(|plan| plan.content_mapper_extensions().map(str::to_owned))
            .collect::<Vec<_>>();
        extensions.sort();
        extensions.dedup();
        extensions
    }
}

/// The configs of a snapshot build (tsgo `configFileRegistryBuilder`).
pub(crate) struct ConfigFileRegistryBuilder<'a> {
    fs: &'a SnapshotFsBuilder,
    base: Configs,
    configs: Configs,
    config_file_names: ConfigFileNameCache,
    custom_config_file_name: String,
    custom_config_file_name_changed: bool,
    /// The session's `run_external_code`.
    run_external_code: bool,
}

impl<'a> ConfigFileRegistryBuilder<'a> {
    /// `custom_config_file_name` is the user's preference for this build.
    pub(crate) fn new(
        fs: &'a SnapshotFsBuilder,
        base: &ConfigFileRegistry,
        run_external_code: bool,
        custom_config_file_name: &str,
    ) -> Self {
        Self {
            fs,
            base: Arc::clone(&base.configs),
            configs: Arc::clone(&base.configs),
            config_file_names: Arc::clone(&base.config_file_names),
            custom_config_file_name: custom_config_file_name.to_owned(),
            custom_config_file_name_changed: custom_config_file_name
                != base.custom_config_file_name,
            run_external_code,
        }
    }

    pub(crate) fn finish(self) -> ConfigFileRegistry {
        ConfigFileRegistry {
            configs: if Arc::ptr_eq(&self.configs, &self.base) {
                self.base
            } else {
                self.configs
            },
            config_file_names: self.config_file_names,
            custom_config_file_name: self.custom_config_file_name,
        }
    }

    /// The entry at `path` to change, cloned out of the base snapshot first.
    fn entry_mut(&mut self, path: &str) -> Option<&mut ConfigFileEntry> {
        Arc::make_mut(&mut self.configs)
            .get_mut(path)
            .map(Arc::make_mut)
    }

    /// tsgo `acquireConfigForProject`: the parse of the config at `path`,
    /// parsed now when it is new or marked; `project` retains the entry until
    /// it releases it.
    pub(crate) fn acquire_config_for_project(
        &mut self,
        file_name: &str,
        path: &str,
        project: &ProjectId,
    ) -> Result<Option<Arc<ConfigRootPlan>>, ProjectError> {
        if !self.configs.contains_key(path) {
            Arc::make_mut(&mut self.configs)
                .insert(path.to_owned(), Arc::new(ConfigFileEntry::new(file_name)));
        }
        let entry = &self.configs[path];
        if !entry.retaining_projects.contains(project)
            || entry.pending_reload != PendingReload::None
        {
            self.entry_mut(path)
                .expect("the entry was stored above")
                .retaining_projects
                .insert(project.clone());
            self.reload_if_needed(path)?;
        }
        Ok(self.configs[path].plan.clone())
    }

    /// tsgo `acquireConfigForFile`: the parse of the config at `path`,
    /// parsed now when it is new or marked; an open file at `file_path`
    /// retains the entry until it closes. An entry nothing retains goes at
    /// the next [`Self::cleanup`].
    pub(crate) fn acquire_config_for_file(
        &mut self,
        file_name: &str,
        path: &str,
        file_path: &str,
    ) -> Result<Option<Arc<ConfigRootPlan>>, ProjectError> {
        if !self.configs.contains_key(path) {
            Arc::make_mut(&mut self.configs)
                .insert(path.to_owned(), Arc::new(ConfigFileEntry::new(file_name)));
        }
        if self.fs.is_open_file(file_path)
            && !self.configs[path].retaining_open_files.contains(file_path)
        {
            self.entry_mut(path)
                .expect("the entry was stored above")
                .retaining_open_files
                .insert(file_path.to_owned());
        }
        self.reload_if_needed(path)?;
        Ok(self.configs[path].plan.clone())
    }

    /// The stored parse of the config at `path`, not parsed again (tsgo
    /// `findOrAcquireConfigForFile` with `projectLoadKindFind`).
    pub(crate) fn find_config(&self, path: &str) -> Option<Arc<ConfigRootPlan>> {
        self.configs.get(path)?.plan.clone()
    }

    /// tsgo `Cleanup`: drop the entries nothing retains.
    pub(crate) fn cleanup(&mut self) {
        let unretained = self
            .configs
            .iter()
            .filter(|(_, entry)| {
                entry.retaining_projects.is_empty()
                    && entry.retaining_open_files.is_empty()
                    && entry.retaining_configs.is_empty()
            })
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        if !unretained.is_empty() {
            let configs = Arc::make_mut(&mut self.configs);
            for path in unretained {
                configs.remove(&path);
            }
        }
    }

    /// tsgo `releaseConfigForProject`: the entry stays until [`Self::cleanup`].
    pub(crate) fn release_config_for_project(&mut self, path: &str, project: &ProjectId) {
        if self
            .configs
            .get(path)
            .is_some_and(|entry| entry.retaining_projects.contains(project))
        {
            self.entry_mut(path)
                .expect("the entry exists")
                .retaining_projects
                .remove(project);
        }
    }

    /// tsgo `reloadIfNeeded`.
    fn reload_if_needed(&mut self, path: &str) -> Result<(), ProjectError> {
        let entry = &self.configs[path];
        let file_name = entry.file_name.clone();
        let old_plan = entry.plan.clone();
        let plan = match (entry.pending_reload, &old_plan) {
            (PendingReload::None, _) => return Ok(()),
            // tsgo `ReloadFileNamesOfParsedCommandLine`: the include patterns
            // matched again, the parse otherwise kept (its errors included).
            (PendingReload::FileNames, Some(old)) => self
                .parse(&file_name)?
                .map(|reloaded| Arc::new((**old).clone().with_reloaded_file_names(reloaded))),
            (PendingReload::FileNames | PendingReload::Full, _) => {
                self.parse(&file_name)?.map(Arc::new)
            }
        };
        let full = entry.pending_reload == PendingReload::Full;
        let entry = self.entry_mut(path).expect("the entry exists");
        entry.plan = plan.clone();
        entry.pending_reload = PendingReload::None;
        if full {
            self.update_extending_configs(path, plan.as_deref(), old_plan.as_deref());
        }
        Ok(())
    }

    /// tsgo `GetParsedCommandLineOfConfigFilePath`: none when the config
    /// cannot be read.
    fn parse(&self, file_name: &str) -> Result<Option<ConfigRootPlan>, ProjectError> {
        let Some(file) = self.fs.get(file_name) else {
            return Ok(None);
        };
        let text = tsc_program::decode_host_text(file.content.to_vec())
            .map_err(|error| ProjectError::new(format!("cannot decode {file_name}: {error}")))?;
        let paths = self.fs.paths();
        let host = VfsCompilerHost::new(SourceFs::new(self.fs), paths.current_directory.clone());
        let config_host = CompilerConfigHost::new(&host);
        let request = ConfigRootPlanRequest {
            file_name: JsString::from(paths.absolute(file_name).as_str()),
            text,
            base_path: JsString::from(paths.current_directory.as_str()),
        };
        // tsgo `reloadIfNeeded`: when the session runs external code, the
        // parse has `runExternalCode` as an existing option, so a config's
        // content mappers register as they do with the command line flag.
        let parsed = if self.run_external_code {
            parse_config_root_plan_with_command_line(
                &config_host,
                request,
                &command_line_option_bag(
                    &[("runExternalCode".to_owned(), JsonValue::Bool(true))],
                    JsStr::from_str(&paths.current_directory),
                ),
                &mut ConfigExtendedCache::default(),
            )
        } else {
            parse_config_root_plan_with_cache(
                &config_host,
                request,
                &mut ConfigExtendedCache::default(),
            )
        };
        parsed
            .map(Some)
            .map_err(|error| ProjectError::new(format!("cannot parse {file_name}: {error}")))
    }

    /// tsgo `updateExtendingConfigs`: the configs a parse extends are kept
    /// with the extending config's path, so their changes reach it.
    fn update_extending_configs(
        &mut self,
        extending: &str,
        new: Option<&ConfigRootPlan>,
        old: Option<&ConfigRootPlan>,
    ) {
        let paths = self.fs.paths().clone();
        let extended = |plan: Option<&ConfigRootPlan>| {
            plan.map_or_else(Vec::new, |plan| {
                plan.extended_source_files()
                    .iter()
                    .map(|file| file.to_string_lossy().into_owned())
                    .collect()
            })
        };
        let new_files = extended(new);
        let new_paths = new_files
            .iter()
            .map(|file| paths.to_path(file))
            .collect::<BTreeSet<_>>();
        for file_name in &new_files {
            let path = paths.to_path(file_name);
            match self.configs.get(&path) {
                Some(entry) if entry.retaining_configs.contains(extending) => {}
                Some(_) => {
                    self.entry_mut(&path)
                        .expect("the entry exists")
                        .retaining_configs
                        .insert(extending.to_owned());
                }
                None => {
                    Arc::make_mut(&mut self.configs).insert(
                        path,
                        Arc::new(ConfigFileEntry {
                            retaining_configs: BTreeSet::from([extending.to_owned()]),
                            matches_root_files: false,
                            ..ConfigFileEntry::new(file_name)
                        }),
                    );
                }
            }
        }
        for file_name in extended(old) {
            let path = paths.to_path(&file_name);
            if !new_paths.contains(&path)
                && self
                    .configs
                    .get(&path)
                    .is_some_and(|entry| entry.retaining_configs.contains(extending))
            {
                self.entry_mut(&path)
                    .expect("the entry exists")
                    .retaining_configs
                    .remove(extending);
            }
        }
    }

    /// tsgo `configFileRegistryBuilder.DidChangeFiles`: the configs a change
    /// concerns are marked for a reload, closed files release their configs,
    /// a created or deleted config forgets the config names of the open
    /// files below it; the projects retaining a marked config and those open
    /// files are returned.
    pub(crate) fn did_change_files(&mut self, changes: &FileChangeSummary) -> ChangeFileResult {
        if changes.invalidate_all {
            return self.invalidate_cache();
        }
        let paths = self.fs.paths().clone();
        let mut result = ChangeFileResult::default();
        let has_excessive_changes = changes.has_excessive_watch_events()
            && changes.includes_watch_change_outside_node_modules;
        let ignored = |file_name: &str| {
            file_name.contains("/node_modules/.")
                || file_name.contains("/.git")
                || file_name.contains(".#")
        };
        let mut created = BTreeMap::new();
        let mut deleted = BTreeMap::new();
        let mut touched = BTreeSet::new();
        let mut created_or_deleted_configs = BTreeSet::new();
        let mut note = |file_name: &str, touched: &mut BTreeSet<String>| {
            let path = paths.to_path(file_name);
            if self.is_config_base_name(path.rsplit('/').next().unwrap_or_default()) {
                created_or_deleted_configs.insert(path.clone());
            }
            touched.insert(path.clone());
            path
        };
        for file_name in changes.changed.iter().filter(|name| !ignored(name)) {
            note(file_name, &mut touched);
        }
        for file_name in changes.deleted.iter().filter(|name| !ignored(name)) {
            let path = note(file_name, &mut touched);
            deleted.insert(path, file_name.clone());
        }
        for file_name in changes.created.iter().filter(|name| !ignored(name)) {
            let path = note(file_name, &mut touched);
            created.insert(path, file_name.clone());
        }

        for file_name in &changes.closed {
            self.did_close_file(&paths.to_path(file_name));
        }

        // Changes to the configs themselves and to the configs they extend,
        // and to the manifests of their content mappers' packages.
        for path in &touched {
            let Some(entry) = self.configs.get(path) else {
                if path.ends_with("/package.json") {
                    let declaring = self
                        .configs
                        .iter()
                        .filter(|(_, entry)| {
                            entry.plan.as_ref().is_some_and(|plan| {
                                plan.content_mapper_manifest_files()
                                    .iter()
                                    .any(|manifest| paths.to_path(manifest) == *path)
                            })
                        })
                        .map(|(config, _)| config.clone())
                        .collect::<Vec<_>>();
                    for config in declaring {
                        result
                            .affected_projects
                            .extend(self.handle_config_change(&config));
                    }
                }
                continue;
            };
            if has_excessive_changes {
                return self.invalidate_cache();
            }
            let extending = entry.retaining_configs.clone();
            result
                .affected_projects
                .extend(self.handle_config_change(path));
            for extending in extending {
                if self.configs.contains_key(&extending) {
                    result
                        .affected_projects
                        .extend(self.handle_config_change(&extending));
                }
            }
            // A config is not also a root file.
            created.remove(path);
        }

        // A config created or deleted (or changed) above an open file may
        // change the file's nearest config.
        for path in &created_or_deleted_configs {
            if has_excessive_changes {
                return self.invalidate_cache();
            }
            let directory = format!("{}/", path_parent(path).trim_end_matches('/'));
            let below = self
                .config_file_names
                .keys()
                .filter(|open| open.starts_with(&directory))
                .cloned()
                .collect::<Vec<_>>();
            if !below.is_empty() {
                let names = Arc::make_mut(&mut self.config_file_names);
                for open in below {
                    names.remove(&open);
                    result.affected_files.insert(open);
                }
            }
        }

        // A deleted root file that only a wildcard included.
        for (path, file_name) in &deleted {
            let marked = self
                .configs
                .iter()
                .filter(|(_, entry)| {
                    entry.pending_reload == PendingReload::None
                        && entry.plan.as_ref().is_some_and(|plan| {
                            plan.file_names()
                                .iter()
                                .any(|root| paths.to_path(&root.to_string_lossy()) == *path)
                                && !plan
                                    .lists_file(JsStr::from_str(file_name), paths.case_sensitive)
                        })
                })
                .map(|(config, _)| config.clone())
                .collect::<Vec<_>>();
            for config in marked {
                if has_excessive_changes {
                    return self.invalidate_cache();
                }
                result
                    .affected_projects
                    .extend(self.mark_file_names_reload(&config));
            }
        }

        // A created file that a config's include patterns may match.
        if !created.is_empty() {
            let marked = self
                .configs
                .iter()
                .filter(|(_, entry)| {
                    entry.matches_root_files
                        && entry.pending_reload == PendingReload::None
                        && entry.plan.as_ref().is_some_and(|plan| {
                            created.iter().any(|(path, file_name)| {
                                plan.possibly_matches_file_name(
                                    JsStr::from_str(file_name),
                                    paths.case_sensitive,
                                ) || (plan.possibly_matches_directory_name(
                                    JsStr::from_str(path),
                                    paths.case_sensitive,
                                ) && self.fs.is_dir(file_name))
                            })
                        })
                })
                .map(|(config, _)| config.clone())
                .collect::<Vec<_>>();
            for config in marked {
                if has_excessive_changes {
                    return self.invalidate_cache();
                }
                result
                    .affected_projects
                    .extend(self.mark_file_names_reload(&config));
            }
        }
        result
    }

    /// tsgo `didCloseFile`: a closed file forgets its config names and
    /// releases its configs.
    fn did_close_file(&mut self, path: &str) {
        if path.starts_with("^/") {
            return;
        }
        if self.config_file_names.contains_key(path) {
            Arc::make_mut(&mut self.config_file_names).remove(path);
        }
        let retaining = self
            .configs
            .iter()
            .filter(|(_, entry)| entry.retaining_open_files.contains(path))
            .map(|(config, _)| config.clone())
            .collect::<Vec<_>>();
        for config in retaining {
            self.entry_mut(&config)
                .expect("the entry exists")
                .retaining_open_files
                .remove(path);
        }
    }

    /// tsgo `DidChangeCustomConfigFileName`: a new custom config file name
    /// forgets every open file's config names.
    pub(crate) fn did_change_custom_config_file_name(&mut self) -> bool {
        if !self.custom_config_file_name_changed {
            return false;
        }
        self.config_file_names = Arc::default();
        true
    }

    /// tsgo `isConfigBaseName`.
    fn is_config_base_name(&self, base_name: &str) -> bool {
        base_name == "tsconfig.json"
            || base_name == "jsconfig.json"
            || (!self.custom_config_file_name.is_empty()
                && base_name == self.custom_config_file_name)
    }

    /// tsgo `computeConfigFileName`: the user's config file name in the
    /// directory of `file_name` or above, else the nearest `tsconfig.json`,
    /// else `jsconfig.json`, not past a `node_modules` directory. Searching
    /// above a config (`skip_search_in_directory_of_file`) skips its own
    /// directory, for a `jsconfig.json` both names.
    pub(crate) fn compute_config_file_name(
        &self,
        file_name: &str,
        skip_search_in_directory_of_file: bool,
    ) -> Option<String> {
        let paths = self.fs.paths();
        let directory_of = |path: &str| {
            tsc_program::get_directory_path(JsStr::from_str(path))
                .to_string_lossy()
                .into_owned()
        };
        let combine = |directory: &str, name: &str| {
            tsc_program::combine_paths(JsStr::from_str(directory), JsStr::from_str(name))
                .to_string_lossy()
                .into_owned()
        };
        let start = directory_of(&paths.absolute(file_name));
        let search = |mut visit: Box<dyn FnMut(&str) -> Option<String> + '_>| {
            let mut directory = start.clone();
            loop {
                if let Some(found) = visit(&directory) {
                    return Some(found);
                }
                if directory.rsplit('/').next() == Some("node_modules") {
                    return None;
                }
                let parent = directory_of(&directory);
                if parent == directory {
                    return None;
                }
                directory = parent;
            }
        };
        if !self.custom_config_file_name.is_empty() {
            let mut skip = skip_search_in_directory_of_file;
            let found = search(Box::new(|directory: &str| {
                if !skip {
                    let candidate = combine(directory, &self.custom_config_file_name);
                    if self.fs.file_exists(&candidate) {
                        return Some(candidate);
                    }
                }
                skip = false;
                None
            }));
            if found.is_some() {
                return found;
            }
        }
        let mut skip_tsconfig = skip_search_in_directory_of_file;
        let mut skip_jsconfig = skip_search_in_directory_of_file
            && file_name.rsplit('/').next() != Some("tsconfig.json");
        search(Box::new(|directory: &str| {
            for (skip, name) in [
                (skip_tsconfig, "tsconfig.json"),
                (skip_jsconfig, "jsconfig.json"),
            ] {
                if !skip {
                    let candidate = combine(directory, name);
                    if self.fs.file_exists(&candidate) {
                        return Some(candidate);
                    }
                }
            }
            skip_tsconfig = false;
            skip_jsconfig = false;
            None
        }))
    }

    /// tsgo `getConfigFileNameForFile`: an open file's nearest config is
    /// kept until a config above it is created or deleted.
    pub(crate) fn get_config_file_name_for_file(
        &mut self,
        file_name: &str,
        path: &str,
    ) -> Option<String> {
        if file_name.starts_with("^/") {
            return None;
        }
        if let Some(names) = self.config_file_names.get(path) {
            return names.nearest.clone();
        }
        let nearest = self.compute_config_file_name(file_name, false);
        if self.fs.is_open_file(path) {
            Arc::make_mut(&mut self.config_file_names).insert(
                path.to_owned(),
                ConfigFileNames {
                    nearest: nearest.clone(),
                    ancestors: BTreeMap::new(),
                },
            );
        }
        nearest
    }

    /// tsgo `forEachConfigFileNameFor`: an open file's nearest config and
    /// the configs above it the searches found, in order.
    pub(crate) fn config_file_names_for(&self, path: &str) -> Vec<String> {
        let mut names = Vec::new();
        if path.starts_with("^/") {
            return names;
        }
        let Some(entry) = self.config_file_names.get(path) else {
            return names;
        };
        let mut next = entry.nearest.clone();
        while let Some(config) = next {
            next = entry.ancestors.get(&config).cloned().flatten();
            let known = entry.ancestors.contains_key(&config);
            names.push(config);
            if !known {
                break;
            }
        }
        names
    }

    /// tsgo `getAncestorConfigFileName`: the config above `config_file_name`
    /// for an open file (kept with its config names).
    pub(crate) fn get_ancestor_config_file_name(
        &mut self,
        file_name: &str,
        path: &str,
        config_file_name: &str,
    ) -> Option<String> {
        if file_name.starts_with("^/") {
            return None;
        }
        let entry = self.config_file_names.get(path)?;
        if let Some(ancestor) = entry.ancestors.get(config_file_name) {
            return ancestor.clone();
        }
        let ancestor = self.compute_config_file_name(config_file_name, true);
        if self.fs.is_open_file(path) {
            Arc::make_mut(&mut self.config_file_names)
                .get_mut(path)
                .expect("the entry exists")
                .ancestors
                .insert(config_file_name.to_owned(), ancestor.clone());
        }
        ancestor
    }

    /// The root files of the config at `path` are matched again before its
    /// next use.
    fn mark_file_names_reload(&mut self, path: &str) -> BTreeSet<ProjectId> {
        let entry = self.entry_mut(path).expect("the entry exists");
        entry.pending_reload = PendingReload::FileNames;
        entry.retaining_projects.clone()
    }

    /// tsgo `handleConfigChange`: the config is parsed again before its next
    /// use; the projects it newly concerns are returned.
    fn handle_config_change(&mut self, path: &str) -> BTreeSet<ProjectId> {
        if self.configs[path].pending_reload == PendingReload::Full {
            return BTreeSet::new();
        }
        let entry = self.entry_mut(path).expect("the entry exists");
        entry.pending_reload = PendingReload::Full;
        entry.retaining_projects.clone()
    }

    /// tsgo `invalidateCache`: every config is parsed again (or its root
    /// files matched again when its text is unchanged), and every open file
    /// forgets its config names; every retaining project and every open file
    /// is affected.
    fn invalidate_cache(&mut self) -> ChangeFileResult {
        let mut result = ChangeFileResult {
            affected_files: self.config_file_names.keys().cloned().collect(),
            ..ChangeFileResult::default()
        };
        self.config_file_names = Arc::default();
        let paths = self.configs.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            let entry = &self.configs[&path];
            result
                .affected_projects
                .extend(entry.retaining_projects.iter().cloned());
            if entry.pending_reload == PendingReload::Full {
                continue;
            }
            let unchanged = entry.plan.as_ref().is_some_and(|plan| {
                self.fs.get(&entry.file_name).is_some_and(|file| {
                    tsc_program::decode_host_text(file.content.to_vec())
                        .is_ok_and(|text| plan.source().text() == text)
                })
            });
            self.entry_mut(&path)
                .expect("the entry exists")
                .pending_reload = if unchanged {
                PendingReload::FileNames
            } else {
                PendingReload::Full
            };
        }
        result
    }
}

/// tsgo `PathKey.Parent`.
fn path_parent(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(index) => &path[..index],
        None => path,
    }
}
