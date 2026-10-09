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
    parse_config_root_plan_with_cache, CompilerConfigHost, ConfigExtendedCache, ConfigRootPlan,
    ConfigRootPlanRequest,
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
    /// The configs that extend this one (by path).
    retaining_configs: BTreeSet<String>,
    /// A project's config, whose roots a created file can add to (tsgo's
    /// `rootFilesWatch`, absent for an entry only extended).
    matches_root_files: bool,
}

type Configs = Arc<BTreeMap<String, Arc<ConfigFileEntry>>>;

/// A snapshot's configs (tsgo `ConfigFileRegistry`).
#[derive(Clone, Debug, Default)]
pub(crate) struct ConfigFileRegistry {
    configs: Configs,
}

impl ConfigFileRegistry {
    /// tsgo `GetConfig`: the parse of the config at `path`.
    pub(crate) fn config(&self, path: &str) -> Option<&Arc<ConfigRootPlan>> {
        self.configs.get(path)?.plan.as_ref()
    }
}

/// The configs of a snapshot build (tsgo `configFileRegistryBuilder`).
pub(crate) struct ConfigFileRegistryBuilder<'a> {
    fs: &'a SnapshotFsBuilder,
    base: Configs,
    configs: Configs,
}

impl<'a> ConfigFileRegistryBuilder<'a> {
    pub(crate) fn new(fs: &'a SnapshotFsBuilder, base: &ConfigFileRegistry) -> Self {
        Self {
            fs,
            base: Arc::clone(&base.configs),
            configs: Arc::clone(&base.configs),
        }
    }

    pub(crate) fn finish(self) -> ConfigFileRegistry {
        ConfigFileRegistry {
            configs: if Arc::ptr_eq(&self.configs, &self.base) {
                self.base
            } else {
                self.configs
            },
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
            Arc::make_mut(&mut self.configs).insert(
                path.to_owned(),
                Arc::new(ConfigFileEntry {
                    file_name: file_name.to_owned(),
                    pending_reload: PendingReload::Full,
                    plan: None,
                    retaining_projects: BTreeSet::new(),
                    retaining_configs: BTreeSet::new(),
                    matches_root_files: true,
                }),
            );
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

    /// tsgo `acquireConfigForFile` for a file the API opened (no editor's
    /// open file retains the entry): the parse of the config at `path`,
    /// parsed now when it is new or marked. An entry nothing retains goes at
    /// the next [`Self::cleanup`].
    pub(crate) fn acquire_config_for_file(
        &mut self,
        file_name: &str,
        path: &str,
    ) -> Result<Option<Arc<ConfigRootPlan>>, ProjectError> {
        if !self.configs.contains_key(path) {
            Arc::make_mut(&mut self.configs).insert(
                path.to_owned(),
                Arc::new(ConfigFileEntry {
                    file_name: file_name.to_owned(),
                    pending_reload: PendingReload::Full,
                    plan: None,
                    retaining_projects: BTreeSet::new(),
                    retaining_configs: BTreeSet::new(),
                    matches_root_files: true,
                }),
            );
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
                entry.retaining_projects.is_empty() && entry.retaining_configs.is_empty()
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
        parse_config_root_plan_with_cache(
            &CompilerConfigHost::new(&host),
            ConfigRootPlanRequest {
                file_name: JsString::from(paths.absolute(file_name).as_str()),
                text,
                base_path: JsString::from(paths.current_directory.as_str()),
            },
            &mut ConfigExtendedCache::default(),
        )
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
                            file_name: file_name.clone(),
                            pending_reload: PendingReload::Full,
                            plan: None,
                            retaining_projects: BTreeSet::new(),
                            retaining_configs: BTreeSet::from([extending.to_owned()]),
                            matches_root_files: false,
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

    /// tsgo `configFileRegistryBuilder.DidChangeFiles` without open files:
    /// the configs a change concerns are marked for a reload, and the
    /// projects retaining them are returned.
    pub(crate) fn did_change_files(&mut self, changes: &FileChangeSummary) -> BTreeSet<ProjectId> {
        if changes.invalidate_all {
            return self.invalidate_cache();
        }
        let paths = self.fs.paths().clone();
        let mut affected = BTreeSet::new();
        let has_excessive_changes = changes.has_excessive_watch_events();
        let ignored = |file_name: &str| {
            file_name.contains("/node_modules/.")
                || file_name.contains("/.git")
                || file_name.contains(".#")
        };
        let mut created = BTreeMap::new();
        let mut deleted = BTreeMap::new();
        let mut touched = BTreeSet::new();
        for file_name in changes.changed.iter().filter(|name| !ignored(name)) {
            touched.insert(paths.to_path(file_name));
        }
        for file_name in changes.deleted.iter().filter(|name| !ignored(name)) {
            let path = paths.to_path(file_name);
            deleted.insert(path.clone(), file_name.clone());
            touched.insert(path);
        }
        for file_name in changes.created.iter().filter(|name| !ignored(name)) {
            let path = paths.to_path(file_name);
            created.insert(path.clone(), file_name.clone());
            touched.insert(path);
        }

        // Changes to the configs themselves and to the configs they extend.
        for path in &touched {
            let Some(entry) = self.configs.get(path) else {
                continue;
            };
            if has_excessive_changes {
                return self.invalidate_cache();
            }
            let extending = entry.retaining_configs.clone();
            affected.extend(self.handle_config_change(path));
            for extending in extending {
                if self.configs.contains_key(&extending) {
                    affected.extend(self.handle_config_change(&extending));
                }
            }
            // A config is not also a root file.
            created.remove(path);
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
                affected.extend(self.mark_file_names_reload(&config));
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
                affected.extend(self.mark_file_names_reload(&config));
            }
        }
        affected
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
    /// files matched again when its text is unchanged); every retaining
    /// project is affected.
    fn invalidate_cache(&mut self) -> BTreeSet<ProjectId> {
        let mut affected = BTreeSet::new();
        let paths = self.configs.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            let entry = &self.configs[&path];
            affected.extend(entry.retaining_projects.iter().cloned());
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
        affected
    }
}
