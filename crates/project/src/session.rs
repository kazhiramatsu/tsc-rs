//! tsgo `project.Session` (session.go): the language server's projects. The
//! editor's notifications are kept as pending file changes over the open
//! documents; an open applies them at once, a request for a document
//! applies the pending ones (tsgo `getSnapshot`), and a close or a config's
//! watch event schedules an update in the background, each into a new
//! snapshot the session adopts.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use tsc_diagnostics::DiagnosticList;
use tsc_host::go_time::duration_string;
use tsc_host::vfs::FileSystem;
use tsc_ls::lsconv::{diagnostic_to_lsp, DiagnosticFile, DiagnosticOptions};
use tsc_ls::lsutil::UserPreferences;
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, LanguageKind, PublishDiagnosticsParams,
    TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::content_mapper::host::Spawner;
use tsc_program::uri::uri_to_file_name;
use tsc_program::ConfigOptionBag;
use tsc_syntax::ScriptKind;

use crate::background::{Cancellation, Queue};
use crate::client::{Client, Logger};
use crate::fs::{FileChangeSummary, Paths};
use crate::id::ProjectKind;
use crate::overlay::{FileChange, FileChangeKind, OverlayFs, Overlays};
use crate::project::{ProgramUpdateKind, Project};
use crate::snapshot::{ProjectError, SessionOptions, Snapshot, SnapshotChange, SnapshotHost};
use crate::watch::{glob_string, WatchRegistry, WatchedFiles};

/// The user's preferences (tsgo's `initialUserPreferences`,
/// `workspaceUserPreferences` and `pendingUserConfigChanges`).
struct UserConfig {
    initial: UserPreferences,
    workspace: UserPreferences,
    pending: bool,
}

/// tsgo's scheduled snapshot update (`scheduledSnapshotUpdateCancel` and
/// `scheduledSnapshotUpdateGeneration`).
#[derive(Default)]
struct ScheduledUpdate {
    cancellation: Option<Cancellation>,
    generation: u64,
}

/// tsgo `Session`.
pub struct Session {
    inner: Arc<Inner>,
}

struct Inner {
    host: SnapshotHost,
    client: Option<Arc<dyn Client>>,
    logger: Option<Arc<dyn Logger>>,
    /// The open documents (tsgo `fs`).
    fs: Mutex<OverlayFs>,
    snapshot: RwLock<Arc<Snapshot>>,
    /// tsgo `snapshotUpdateMu`: one snapshot update at a time.
    snapshot_update: Mutex<()>,
    pending_file_changes: Mutex<Vec<FileChange>>,
    user_config: Mutex<UserConfig>,
    /// tsgo `backgroundQueue`.
    background: Arc<Queue>,
    scheduled_update: Mutex<ScheduledUpdate>,
    /// tsgo's scheduled diagnostics refresh (`diagnosticsRefreshCancel`
    /// and `diagnosticsRefreshGeneration`).
    diagnostics_refresh: Mutex<ScheduledUpdate>,
    /// tsgo `globalDiagPublishPending`.
    global_diagnostics_publish_pending: AtomicBool,
    /// tsgo `watches`: the globs registered with the editor.
    watches: WatchRegistry,
    idle_clean: Mutex<IdleClean>,
}

/// tsgo `idleCacheCleanDelay`.
const IDLE_CACHE_CLEAN_DELAY: Duration = Duration::from_secs(30);

/// tsgo's idle cache clean timer (`idleCacheCleanTimer`, its wait group and
/// `idleCacheCleanClosed`).
#[derive(Default)]
struct IdleClean {
    cancellation: Option<Cancellation>,
    generation: u64,
    closed: bool,
    timer: Option<std::thread::JoinHandle<()>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Session {
    /// tsgo `NewSession`.
    pub fn new(
        options: SessionOptions,
        fs: Arc<dyn FileSystem>,
        client: Option<Arc<dyn Client>>,
        logger: Option<Arc<dyn Logger>>,
        spawner: Option<Arc<dyn Spawner>>,
    ) -> Self {
        let position_encoding = options.position_encoding.clone();
        let paths = Paths {
            current_directory: options.current_directory.clone(),
            case_sensitive: fs.case_sensitive(),
        };
        let host = SnapshotHost::new(options, Arc::clone(&fs), spawner);
        let snapshot = Arc::new(host.new_root_snapshot());
        Self {
            inner: Arc::new(Inner {
                host,
                client,
                logger,
                fs: Mutex::new(OverlayFs::new(fs, paths, position_encoding)),
                snapshot: RwLock::new(snapshot),
                snapshot_update: Mutex::new(()),
                pending_file_changes: Mutex::default(),
                user_config: Mutex::new(UserConfig {
                    initial: UserPreferences::new_default(),
                    workspace: UserPreferences::new_default(),
                    pending: false,
                }),
                background: Arc::default(),
                scheduled_update: Mutex::default(),
                diagnostics_refresh: Mutex::default(),
                global_diagnostics_publish_pending: AtomicBool::new(false),
                watches: WatchRegistry::default(),
                idle_clean: Mutex::default(),
            }),
        }
    }

    /// The session's snapshot host.
    pub fn host(&self) -> &SnapshotHost {
        &self.inner.host
    }

    /// tsgo `Snapshot`: the current snapshot.
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.inner.snapshot()
    }

    /// tsgo `Config`: the user's preferences.
    pub fn config(&self) -> UserPreferences {
        lock(&self.inner.user_config).workspace.clone()
    }

    /// tsgo `Configure`: the user's new preferences, applied at the next
    /// update; the client's locale follows them, and the editor refreshes
    /// its inlay hints and code lenses when their preferences changed.
    pub fn configure(&self, config: UserPreferences) {
        let old = {
            let mut user_config = lock(&self.inner.user_config);
            user_config.pending = true;
            std::mem::replace(&mut user_config.workspace, config.clone())
        };
        let Some(client) = &self.inner.client else {
            return;
        };
        if !config.locale.is_empty() {
            client.set_locale(&config.locale);
        }
        if old.inlay_hints_preferences != config.inlay_hints_preferences {
            let _ = client.refresh_inlay_hints();
        }
        if old.code_lens_user_preferences != config.code_lens_user_preferences {
            let _ = client.refresh_code_lens();
        }
        // tsgo `refreshDiagnosticsIfNeeded` and `refreshATAIfNeeded`.
        if old.custom_config_file_name != config.custom_config_file_name
            || old.report_style_checks_as_warnings != config.report_style_checks_as_warnings
            || old.validate_enabled != config.validate_enabled
            || (old.is_ata_disabled() && !config.is_ata_disabled())
        {
            self.schedule_diagnostics_refresh();
        }
    }

    /// tsgo `ScheduleDiagnosticsRefresh`: the editor pulls its diagnostics
    /// again after the debounce delay, unless another refresh comes first.
    pub fn schedule_diagnostics_refresh(&self) {
        self.inner
            .schedule_diagnostics_refresh(self.inner.host.options().debounce_delay);
    }

    /// tsgo `InitializeWithUserConfig`.
    pub fn initialize_with_user_config(&self, config: UserPreferences) {
        lock(&self.inner.user_config).initial = config.clone();
        self.configure(config);
    }

    /// The user's preferences when the session started.
    pub fn initial_config(&self) -> UserPreferences {
        lock(&self.inner.user_config).initial.clone()
    }

    /// tsgo `DidOpenFile`: the document's overlay, applied at once with the
    /// pending changes; the document's default project is loaded.
    pub fn did_open_file(
        &self,
        uri: &DocumentUri,
        version: i32,
        content: &str,
        language_kind: &LanguageKind,
    ) -> Result<(), ProjectError> {
        let inner = &self.inner;
        inner.schedule_idle_cache_clean();
        inner.cancel_scheduled_snapshot_update();
        let _update = lock(&inner.snapshot_update);
        let (changes, overlays) = {
            let mut pending = lock(&inner.pending_file_changes);
            pending.push(FileChange {
                kind: FileChangeKind::Open,
                uri: uri.clone(),
                version,
                content: content.to_owned(),
                language_kind: language_kind.clone(),
                changes: Vec::new(),
            });
            inner.flush_changes_locked(&mut pending)
        };
        inner
            .update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    documents: document_file_name(uri).into_iter().collect(),
                    ..SnapshotChange::default()
                },
            )
            .map(|_| ())
    }

    /// tsgo `DidCloseFile`: pending, with an update scheduled.
    pub fn did_close_file(&self, uri: &DocumentUri) {
        self.inner.schedule_idle_cache_clean();
        lock(&self.inner.pending_file_changes)
            .push(FileChange::new(FileChangeKind::Close, uri.clone()));
        self.inner.schedule_snapshot_update();
    }

    /// tsgo `DidChangeFile`: pending until the next update.
    pub fn did_change_file(
        &self,
        uri: &DocumentUri,
        version: i32,
        changes: Vec<TextDocumentContentChangePartialOrWholeDocument>,
    ) {
        self.inner.schedule_idle_cache_clean();
        lock(&self.inner.pending_file_changes).push(FileChange {
            version,
            changes,
            ..FileChange::new(FileChangeKind::Change, uri.clone())
        });
        // A content-mapped file's dependents are refreshed at once (the
        // client re-pulls only the documents it syncs); an ordinary edit
        // cancels a pending refresh (tsgo `isContentMapperFile`).
        let file_name = document_file_name(uri).unwrap_or_default();
        let extensions = self.snapshot().content_mapper_extensions();
        if extensions
            .iter()
            .any(|extension| file_name.ends_with(extension.as_str()))
        {
            self.inner.schedule_diagnostics_refresh(Duration::ZERO);
        } else {
            self.inner.cancel_diagnostics_refresh();
        }
    }

    /// tsgo `DidSaveFile`: pending until the next update.
    pub fn did_save_file(&self, uri: &DocumentUri) {
        self.inner.schedule_idle_cache_clean();
        lock(&self.inner.pending_file_changes)
            .push(FileChange::new(FileChangeKind::Save, uri.clone()));
    }

    /// tsgo `DidChangeWatchedFiles`: pending until the next update, which a
    /// change of a config the snapshot keeps schedules.
    pub fn did_change_watched_files(&self, changes: &[FileEvent]) {
        let snapshot = self.snapshot();
        let mut has_config_change = false;
        let mut has_relevant_change = false;
        let extensions = snapshot.content_mapper_extensions();
        let watched = snapshot.content_mapper_watched_paths();
        let changes = changes
            .iter()
            .filter_map(|change| {
                let kind = match change.r#type {
                    FileChangeType::CREATED => FileChangeKind::WatchCreate,
                    FileChangeType::CHANGED => FileChangeKind::WatchChange,
                    FileChangeType::DELETED => FileChangeKind::WatchDelete,
                    _ => return None,
                };
                let file_name = document_file_name(&change.uri).unwrap_or_default();
                if !has_config_change {
                    has_config_change = snapshot.is_config_tracked(&file_name);
                }
                if !has_relevant_change {
                    has_relevant_change =
                        self.is_relevant_change(&snapshot, &file_name, kind, &extensions, &watched);
                }
                Some(FileChange::new(kind, change.uri.clone()))
            })
            .collect::<Vec<_>>();
        lock(&self.inner.pending_file_changes).extend(changes);
        // Only the paths that can affect a program refresh the editor's
        // diagnostics.
        if has_relevant_change {
            self.schedule_diagnostics_refresh();
        }
        // Config diagnostics are pushed on snapshot updates, not pulled.
        if has_config_change {
            self.inner.schedule_snapshot_update();
        }
        self.inner.schedule_idle_cache_clean();
    }

    /// tsgo `DidChangeWatchedFiles`' relevance: a content mapper's watched
    /// file, a file of a relevant extension (or a content mapper's), or an
    /// extensionless path that is a directory (for a deletion, one the
    /// snapshot read files below or has open documents in, or within
    /// `node_modules`).
    fn is_relevant_change(
        &self,
        snapshot: &Snapshot,
        file_name: &str,
        kind: FileChangeKind,
        extensions: &[String],
        watched: &BTreeSet<String>,
    ) -> bool {
        let path = snapshot.to_path(file_name);
        let path = path.trim_end_matches('/');
        if watched.contains(path) {
            return true;
        }
        let base = path.rsplit('/').next().unwrap_or(path);
        let Some(index) = base.rfind('.') else {
            if kind != FileChangeKind::WatchDelete {
                return lock(&self.inner.fs).directory_exists(file_name);
            }
            return snapshot.has_files_within(path)
                || path.ends_with("/node_modules")
                || path.contains("/node_modules/");
        };
        let extension = &base[index..];
        matches!(
            extension,
            ".js" | ".jsx" | ".mjs" | ".cjs" | ".ts" | ".tsx" | ".mts" | ".cts" | ".json"
        ) || extensions
            .iter()
            .any(|mapped| path.ends_with(mapped.as_str()))
    }

    /// tsgo `getSnapshot` for documents: the pending changes applied, and
    /// otherwise an update only when a document's default project is not
    /// loaded or is dirty.
    pub fn get_snapshot(&self, documents: &[DocumentUri]) -> Result<Arc<Snapshot>, ProjectError> {
        let inner = &self.inner;
        let _update = lock(&inner.snapshot_update);
        inner.cancel_scheduled_snapshot_update();
        let (changes, overlays, new_config) = inner.flush_changes();
        let documents = documents
            .iter()
            .filter_map(document_file_name)
            .collect::<Vec<_>>();
        if !changes.is_empty() || new_config.is_some() {
            return inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    new_config,
                    documents,
                    ..SnapshotChange::default()
                },
            );
        }
        let snapshot = inner.snapshot();
        let stale = documents.iter().any(|document| {
            snapshot
                .default_project(document)
                .is_none_or(|project| project.dirty)
        });
        if !stale {
            return Ok(snapshot);
        }
        inner.update_snapshot(
            overlays,
            SnapshotChange {
                documents,
                ..SnapshotChange::default()
            },
        )
    }

    /// tsgo `getSnapshotAndDefaultProject` (what `GetLanguageService`
    /// stands on): the snapshot for the document and its default project.
    pub fn get_snapshot_and_default_project(
        &self,
        uri: &DocumentUri,
    ) -> Result<(Arc<Snapshot>, Arc<Project>), DocumentProjectError> {
        let snapshot = self
            .get_snapshot(std::slice::from_ref(uri))
            .map_err(DocumentProjectError::Update)?;
        let file_name = document_file_name(uri).unwrap_or_default();
        if let Some(project) = snapshot.default_project(&file_name) {
            return Ok((Arc::clone(&snapshot), Arc::clone(project)));
        }
        let kind = match snapshot.overlay(&file_name) {
            Some(overlay) => Some(overlay.kind()),
            None => snapshot
                .read_file(&file_name)
                .map(|_| ScriptKind::from_file_name(&file_name)),
        };
        Err(DocumentProjectError::NoProject {
            uri: uri.as_str().to_owned(),
            unknown_script_kind: kind == Some(ScriptKind::Unknown),
        })
    }

    /// tsgo `EnqueuePublishGlobalDiagnostics`: after a language service
    /// request, the configs' diagnostics again in the background when
    /// checking found new global diagnostics.
    pub fn enqueue_publish_global_diagnostics(&self) {
        let inner = &self.inner;
        if !inner.host.options().push_diagnostics_enabled
            || self.config().validate_enabled == Some(false)
        {
            return;
        }
        if inner
            .global_diagnostics_publish_pending
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let background = Arc::clone(inner);
            inner
                .background
                .enqueue(&Cancellation::default(), move |_| {
                    background.publish_global_diagnostics();
                });
        }
    }

    /// tsgo `DidChangeCompilerOptionsForInferredProjects`: the inferred
    /// projects take `options` (tsgo's language server sets them for its
    /// tests).
    pub fn did_change_compiler_options_for_inferred_projects(&self, options: Arc<ConfigOptionBag>) {
        let overlays = lock(&self.inner.fs).overlays();
        let _ = self.inner.update_snapshot(
            overlays,
            SnapshotChange {
                compiler_options_for_inferred_projects: Some(options),
                ..SnapshotChange::default()
            },
        );
    }

    /// tsgo `WaitForBackgroundTasks`: the idle clean cancelled, the
    /// background work done.
    pub fn wait_for_background_tasks(&self) {
        self.inner.cancel_idle_cache_clean();
        self.inner.background.wait();
    }

    /// tsgo `Close`: the scheduled update cancelled, the background work
    /// ended, and the content mappers' processes with it.
    pub fn close(&self) {
        let inner = &self.inner;
        inner.cancel_scheduled_snapshot_update();
        inner.cancel_diagnostics_refresh();
        inner.close_idle_cache_clean();
        inner.background.close();
        let _update = lock(&inner.snapshot_update);
        inner.host.close();
    }
}

impl Inner {
    fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.snapshot.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// tsgo `UpdateSnapshot`: the current snapshot with `change`, adopted;
    /// the watches follow it in the background.
    fn update_snapshot(
        self: &Arc<Self>,
        overlays: Overlays,
        change: SnapshotChange<'_>,
    ) -> Result<Arc<Snapshot>, ProjectError> {
        let (old, snapshot) = {
            let mut current = self
                .snapshot
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            let snapshot = Arc::new(self.host.clone_snapshot(
                &current,
                SnapshotChange {
                    overlays: Some(overlays),
                    ..change
                },
            )?);
            (
                std::mem::replace(&mut *current, Arc::clone(&snapshot)),
                snapshot,
            )
        };
        // tsgo's adoption task: the logs, the watches, then the configs'
        // diagnostics.
        let inner = Arc::clone(self);
        let new = Arc::clone(&snapshot);
        self.background.enqueue(&Cancellation::default(), move |_| {
            inner.log(|| {
                format!(
                    "Adopted snapshot {} (parent {}) as current session snapshot (replacing {})",
                    new.id(),
                    new.parent_id(),
                    old.id()
                )
            });
            if inner.logging_enabled() {
                inner.log_project_changes(&old, &new);
            }
            inner.log(String::new);
            if inner.host.options().watch_enabled {
                if let Err(error) = inner.update_watches(&old, &new) {
                    inner.log(|| error);
                }
            }
            inner.publish_program_diagnostics(&old, &new);
        });
        Ok(snapshot)
    }

    /// tsgo `publishProgramDiagnostics`: the config diagnostics of each
    /// configured project with an open file, published when its program was
    /// built anew (not a one-file clone) or its files were opened again, and
    /// cleared when it went away, its files were closed or the user turned
    /// validation off.
    fn publish_program_diagnostics(&self, old: &Arc<Snapshot>, new: &Arc<Snapshot>) {
        if !self.host.options().push_diagnostics_enabled {
            return;
        }
        let configured = |project: &Project| project.kind == ProjectKind::Configured;
        let config_file_name =
            |project: &Project| project.config_file_name.clone().unwrap_or_default();
        if new.user_preferences().validate_enabled == Some(false) {
            if old.user_preferences().validate_enabled == Some(false) {
                return;
            }
            let old_open = old.open_configured_projects();
            for project in old.projects() {
                if configured(project) && old_open.contains(&project.id) {
                    self.publish_project_diagnostics(old, &config_file_name(project), Vec::new());
                }
            }
            return;
        }
        let should_publish = |project: &Project| {
            configured(project)
                && project.program.is_some()
                && project.program_last_update == new.id()
                && matches!(
                    project.program_update_kind,
                    ProgramUpdateKind::SameFileNames | ProgramUpdateKind::NewFiles
                )
        };
        let old_open = old.open_configured_projects();
        let new_open = new.open_configured_projects();
        for project in new.projects() {
            if old.project(&project.id).is_none()
                && should_publish(project)
                && new_open.contains(&project.id)
            {
                self.publish_project_diagnostics(
                    new,
                    &config_file_name(project),
                    project.project_diagnostics(),
                );
            }
        }
        for project in old.projects() {
            match new.project(&project.id) {
                None if configured(project) => {
                    self.publish_project_diagnostics(old, &config_file_name(project), Vec::new())
                }
                Some(new_project)
                    if !Arc::ptr_eq(project, new_project)
                        && should_publish(new_project)
                        && new_open.contains(&new_project.id) =>
                {
                    self.publish_project_diagnostics(
                        new,
                        &config_file_name(new_project),
                        new_project.project_diagnostics(),
                    );
                }
                _ => {}
            }
        }
        // The projects whose open files changed without a new program.
        for new_project in new.projects().filter(|project| configured(project)) {
            let Some(old_project) = old.project(&new_project.id) else {
                continue;
            };
            let new_has_open_files = new_open.contains(&new_project.id);
            let old_has_open_files = old_open.contains(&new_project.id);
            if new_has_open_files
                && !old_has_open_files
                && (Arc::ptr_eq(old_project, new_project) || !should_publish(new_project))
            {
                self.publish_project_diagnostics(
                    new,
                    &config_file_name(new_project),
                    new_project.project_diagnostics(),
                );
            } else if !new_has_open_files && old_has_open_files {
                self.publish_project_diagnostics(new, &config_file_name(new_project), Vec::new());
            }
        }
    }

    /// tsgo `publishProjectDiagnostics`: the diagnostics of a config (none
    /// when the user turned validation off) in the client's locale.
    /// tsgo `publishGlobalDiagnostics`.
    fn publish_global_diagnostics(&self) {
        self.global_diagnostics_publish_pending
            .store(false, Ordering::SeqCst);
        let snapshot = Arc::clone(&self.snapshot.read().unwrap_or_else(PoisonError::into_inner));
        for project in snapshot.projects() {
            if project.kind != ProjectKind::Configured {
                continue;
            }
            let Some(program) = project.program() else {
                continue;
            };
            if program.take_new_global_diagnostics() {
                self.publish_project_diagnostics(
                    &snapshot,
                    project.config_file_name.as_deref().unwrap_or_default(),
                    project.project_diagnostics(),
                );
            }
        }
    }

    fn publish_project_diagnostics(
        &self,
        snapshot: &Arc<Snapshot>,
        config_file_name: &str,
        diagnostics: DiagnosticList,
    ) {
        let Some(client) = &self.client else {
            return;
        };
        let diagnostics = if lock(&self.user_config).workspace.validate_enabled == Some(false) {
            Vec::new()
        } else {
            diagnostics
        };
        // The texts of the files the diagnostics are in.
        let texts = diagnostics
            .iter()
            .flat_map(|diagnostic| {
                diagnostic.file_name.iter().chain(
                    diagnostic
                        .related
                        .iter()
                        .filter_map(|related| related.file_name.as_ref()),
                )
            })
            .map(|file_name| file_name.to_string_lossy().into_owned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|file_name| {
                let text = tsc_program::decode_host_text(snapshot.read_file(&file_name)?).ok()?;
                Some((file_name, text))
            })
            .collect::<BTreeMap<_, _>>();
        let converters = snapshot.converters(self.host.options().position_encoding.clone());
        let options = DiagnosticOptions::push(&self.host.options().client_capabilities);
        let locale = client.locale();
        let file = |file_name: &str| {
            texts
                .get_key_value(file_name)
                .map(|(file_name, text)| DiagnosticFile {
                    file_name: file_name.clone(),
                    text,
                })
        };
        let diagnostics = diagnostics
            .iter()
            .map(|diagnostic| {
                diagnostic_to_lsp(&converters, diagnostic, &options, locale.messages(), &file)
            })
            .collect();
        let params = PublishDiagnosticsParams {
            uri: DocumentUri(tsc_program::uri::file_name_to_document_uri(
                config_file_name,
            )),
            version: None,
            diagnostics,
        };
        if let Err(error) = client.publish_diagnostics(params) {
            self.log(|| format!("Error publishing diagnostics: {error}"));
        }
    }

    /// tsgo `logProjectChanges`: each project added or built with new
    /// files, each removed, then the cache statistics when one was.
    fn log_project_changes(&self, old: &Snapshot, new: &Snapshot) {
        let verbose = self
            .logger
            .as_ref()
            .is_some_and(|logger| logger.is_verbose());
        let mut logged = false;
        let mut log_project = |project: &Project| {
            self.log(|| print_project(project, verbose));
            logged = true;
        };
        for project in new.projects() {
            if old.project(&project.id).is_none() {
                log_project(project);
            }
        }
        for project in old.projects() {
            match new.project(&project.id) {
                None => self.log(|| format!("\nProject '{}' removed\n{HR}", project.id)),
                Some(new_project)
                    if !Arc::ptr_eq(project, new_project)
                        && new_project.program_update_kind == ProgramUpdateKind::NewFiles =>
                {
                    log_project(new_project);
                }
                Some(_) => {}
            }
        }
        if logged || verbose {
            let [open, cached, aliases, projects, configs] = new.cache_statistics();
            self.log(|| "\n======== Cache Statistics ========".to_owned());
            self.log(|| format!("Open file count:   {open:6}"));
            self.log(|| format!("Cached disk files: {cached:6}"));
            self.log(|| format!("Realpath aliases:  {aliases:6}"));
            self.log(|| format!("Project count:     {projects:6}"));
            self.log(|| format!("Config count:      {configs:6}"));
        }
    }

    fn logging_enabled(&self) -> bool {
        self.logger.is_some() && self.host.options().logging_enabled
    }

    /// tsgo `updateWatches`: the watches of the configs and projects the
    /// new snapshot added, changed or removed, and those whose registration
    /// failed before.
    fn update_watches(&self, old: &Snapshot, new: &Snapshot) -> Result<(), String> {
        let start = Instant::now();
        let mut errors = Vec::new();
        let old_configs = old.configs.root_files_watches().collect::<BTreeMap<_, _>>();
        let new_configs = new.configs.root_files_watches().collect::<BTreeMap<_, _>>();
        let id = |watch: Option<&Arc<WatchedFiles<_>>>| watch.map(|watch| watch.id());
        for (path, new_watch) in &new_configs {
            if !old_configs.contains_key(path) {
                errors.extend(self.update_watch(None, new_watch.map(Arc::as_ref)));
            }
        }
        for (path, old_watch) in &old_configs {
            match new_configs.get(path) {
                Some(new_watch) if id(*old_watch) != id(*new_watch) => errors.extend(
                    self.update_watch(old_watch.map(Arc::as_ref), new_watch.map(Arc::as_ref)),
                ),
                Some(_) => {}
                None => errors.extend(self.update_watch(old_watch.map(Arc::as_ref), None)),
            }
        }
        // Retry the config watches whose registration failed.
        for (path, new_watch) in &new_configs {
            if let (Some(old_watch), Some(new_watch)) = (old_configs.get(path), new_watch) {
                if id(*old_watch) == Some(new_watch.id())
                    && self.watches.is_pending(&new_watch.id())
                {
                    errors.extend(self.update_watch(None, Some(new_watch.as_ref())));
                }
            }
        }
        let old_projects = old
            .projects()
            .map(|project| (&project.id, project))
            .collect::<BTreeMap<_, _>>();
        let new_projects = new
            .projects()
            .map(|project| (&project.id, project))
            .collect::<BTreeMap<_, _>>();
        for project in new.projects() {
            if !old_projects.contains_key(&project.id) {
                errors.extend(self.update_watch(None, project.program_files_watch.as_deref()));
                errors.extend(self.update_watch(None, project.content_mapper_watch.as_deref()));
            }
        }
        for project in old.projects() {
            let Some(new_project) = new_projects.get(&project.id) else {
                errors.extend(self.update_watch(project.program_files_watch.as_deref(), None));
                errors.extend(self.update_watch(project.content_mapper_watch.as_deref(), None));
                continue;
            };
            if Arc::ptr_eq(project, new_project) {
                continue;
            }
            errors.extend(self.update_changed_watch(
                project.program_files_watch.as_deref(),
                new_project.program_files_watch.as_deref(),
            ));
            errors.extend(self.update_changed_watch(
                project.content_mapper_watch.as_deref(),
                new_project.content_mapper_watch.as_deref(),
            ));
        }
        if !errors.is_empty() {
            return Err(format!("errors updating watches: [{}]", errors.join(" ")));
        }
        self.log(|| format!("Updated watches in {}", duration_string(start.elapsed())));
        Ok(())
    }

    /// tsgo's per-watch step of `updateWatches` for a changed project: the
    /// watch again when its ID changed, or when its registration failed.
    fn update_changed_watch<T>(
        &self,
        old: Option<&WatchedFiles<T>>,
        new: Option<&WatchedFiles<T>>,
    ) -> Vec<String> {
        let old_id = old.map(WatchedFiles::id).unwrap_or_default();
        let new_id = new.map(WatchedFiles::id).unwrap_or_default();
        if old_id != new_id {
            return self.update_watch(old, new);
        }
        if self.watches.is_pending(&new_id) {
            return self.update_watch(None, new);
        }
        Vec::new()
    }

    /// tsgo `updateWatch`: the new watch's globs registered (those another
    /// watch has already are counted), the old one's released and
    /// unregistered when no watch has them.
    fn update_watch<T>(
        &self,
        old: Option<&WatchedFiles<T>>,
        new: Option<&WatchedFiles<T>>,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        let Some(client) = &self.client else {
            return errors;
        };
        if let Some(new) = new {
            let watchers = new.watchers();
            let all = watchers
                .workspace_watchers
                .iter()
                .chain(&watchers.outside_workspace_watchers)
                .collect::<Vec<_>>();
            if !all.is_empty() {
                let mut acquired = Vec::new();
                for (index, watcher) in all.iter().enumerate() {
                    let id = format!("{}.{index}", watchers.id);
                    if self.watches.acquire(watcher, &id) {
                        acquired.push((id, (*watcher).clone()));
                    }
                }
                let mut watch_errors = Vec::new();
                for (id, watcher) in &acquired {
                    match client.watch_files(id, vec![watcher.clone()]) {
                        Err(error) => watch_errors.push(error),
                        Ok(()) => {
                            self.log(|| {
                                if old.is_none() {
                                    format!("Added new watch: {id}")
                                } else {
                                    format!("Updated watch: {id}")
                                }
                            });
                            self.log(|| format!("\t{}", glob_string(watcher)));
                            self.log(String::new);
                        }
                    }
                }
                if watch_errors.is_empty() {
                    self.watches.clear_pending(&watchers.id);
                } else {
                    // Every glob acquired here is released, to register
                    // again at the retry.
                    for (_, watcher) in &acquired {
                        self.watches.release(watcher);
                    }
                    self.watches.mark_pending(&watchers.id);
                    errors.extend(watch_errors);
                }
                if !watchers.ignored_paths.is_empty() {
                    self.log(|| {
                        format!(
                            "{} paths ineligible for watching",
                            watchers.ignored_paths.len()
                        )
                    });
                }
            }
        }
        if let Some(old) = old {
            let watchers = old.watchers();
            let removed = watchers
                .workspace_watchers
                .iter()
                .chain(&watchers.outside_workspace_watchers)
                .filter_map(|watcher| self.watches.release(watcher))
                .collect::<Vec<_>>();
            for id in removed {
                match client.unwatch_files(&id) {
                    Err(error) => errors.push(error),
                    Ok(()) if new.is_none() => self.log(|| format!("Removed watch: {id}")),
                    Ok(()) => {}
                }
            }
        }
        errors
    }

    /// tsgo `ScheduleSnapshotUpdate`: the pending changes applied in the
    /// background after the debounce delay, unless another update comes
    /// first.
    fn schedule_snapshot_update(self: &Arc<Self>) {
        let mut scheduled = lock(&self.scheduled_update);
        if let Some(cancellation) = scheduled.cancellation.take() {
            cancellation.cancel();
        }
        let cancellation = Cancellation::default();
        scheduled.generation += 1;
        let generation = scheduled.generation;
        scheduled.cancellation = Some(cancellation.clone());
        let inner = Arc::clone(self);
        self.background.enqueue(&cancellation, move |cancellation| {
            if !cancellation.sleep(inner.host.options().debounce_delay) {
                return;
            }
            {
                let mut scheduled = lock(&inner.scheduled_update);
                if scheduled.generation != generation {
                    return;
                }
                scheduled.cancellation = None;
            }
            let _update = lock(&inner.snapshot_update);
            let (changes, overlays, new_config) = inner.flush_changes();
            if changes.is_empty() && new_config.is_none() {
                return;
            }
            // tsgo's background update has no caller to answer; a failed
            // update leaves the snapshot as it was.
            let _ = inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    new_config,
                    ..SnapshotChange::default()
                },
            );
        });
    }

    /// tsgo `scheduleDiagnosticsRefresh`: the editor's `RefreshDiagnostics`
    /// in the background after `delay`, unless another refresh comes first.
    fn schedule_diagnostics_refresh(self: &Arc<Self>, delay: Duration) {
        let mut scheduled = lock(&self.diagnostics_refresh);
        match scheduled.cancellation.take() {
            Some(cancellation) => {
                cancellation.cancel();
                self.log(|| "Delaying scheduled diagnostics refresh...".to_owned());
            }
            None => self.log(|| "Scheduling new diagnostics refresh...".to_owned()),
        }
        let cancellation = Cancellation::default();
        scheduled.generation += 1;
        let generation = scheduled.generation;
        scheduled.cancellation = Some(cancellation.clone());
        let inner = Arc::clone(self);
        self.background.enqueue(&cancellation, move |cancellation| {
            if !delay.is_zero() && !cancellation.sleep(delay) || cancellation.is_cancelled() {
                return;
            }
            {
                let mut scheduled = lock(&inner.diagnostics_refresh);
                if scheduled.generation != generation {
                    return;
                }
                scheduled.cancellation = None;
            }
            inner.log(|| "Running scheduled diagnostics refresh".to_owned());
            if let Some(client) = &inner.client {
                if let Err(error) = client.refresh_diagnostics() {
                    inner.log(|| format!("Error refreshing diagnostics: {error}"));
                }
            }
        });
    }

    /// tsgo `cancelDiagnosticsRefresh`.
    fn cancel_diagnostics_refresh(&self) {
        let mut scheduled = lock(&self.diagnostics_refresh);
        if let Some(cancellation) = scheduled.cancellation.take() {
            cancellation.cancel();
            self.log(|| "Canceled scheduled diagnostics refresh".to_owned());
            scheduled.generation += 1;
        }
    }

    /// tsgo `scheduleIdleCacheClean`: after the editor has been quiet for
    /// [`IDLE_CACHE_CLEAN_DELAY`], the pending changes applied with the
    /// cached files no build looked at dropped.
    fn schedule_idle_cache_clean(self: &Arc<Self>) {
        let mut idle = lock(&self.idle_clean);
        if idle.closed {
            return;
        }
        if let Some(cancellation) = idle.cancellation.take() {
            cancellation.cancel();
        }
        let cancellation = Cancellation::default();
        idle.generation += 1;
        let generation = idle.generation;
        idle.cancellation = Some(cancellation.clone());
        let inner = Arc::clone(self);
        idle.timer = Some(std::thread::spawn(move || {
            if !cancellation.sleep(IDLE_CACHE_CLEAN_DELAY) {
                return;
            }
            {
                let mut idle = lock(&inner.idle_clean);
                if idle.generation != generation {
                    return;
                }
                idle.cancellation = None;
            }
            let _update = lock(&inner.snapshot_update);
            inner.cancel_scheduled_snapshot_update();
            let (changes, overlays, new_config) = inner.flush_changes();
            let _ = inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    new_config,
                    clean_file_cache: true,
                    ..SnapshotChange::default()
                },
            );
        }));
    }

    /// tsgo `cancelIdleCacheClean`.
    fn cancel_idle_cache_clean(&self) {
        let mut idle = lock(&self.idle_clean);
        if let Some(cancellation) = idle.cancellation.take() {
            cancellation.cancel();
            idle.generation += 1;
        }
    }

    /// tsgo `closeIdleCacheClean`: no clean after this, and a running one
    /// done.
    fn close_idle_cache_clean(&self) {
        let timer = {
            let mut idle = lock(&self.idle_clean);
            idle.closed = true;
            if let Some(cancellation) = idle.cancellation.take() {
                cancellation.cancel();
                idle.generation += 1;
            }
            idle.timer.take()
        };
        if let Some(timer) = timer {
            let _ = timer.join();
        }
    }

    /// tsgo `cancelScheduledSnapshotUpdate`.
    fn cancel_scheduled_snapshot_update(&self) {
        let mut scheduled = lock(&self.scheduled_update);
        if let Some(cancellation) = scheduled.cancellation.take() {
            cancellation.cancel();
            scheduled.generation += 1;
        }
    }

    /// tsgo `flushChanges`: the pending file changes applied to the open
    /// documents, and the user's preferences when they changed.
    fn flush_changes(&self) -> (FileChangeSummary, Overlays, Option<UserPreferences>) {
        let (changes, overlays) = {
            let mut pending = lock(&self.pending_file_changes);
            self.flush_changes_locked(&mut pending)
        };
        let mut user_config = lock(&self.user_config);
        let new_config = user_config.pending.then(|| user_config.workspace.clone());
        user_config.pending = false;
        (changes, overlays, new_config)
    }

    /// tsgo `flushChangesLocked`.
    fn flush_changes_locked(&self, pending: &mut Vec<FileChange>) -> (FileChangeSummary, Overlays) {
        let mut fs = lock(&self.fs);
        if pending.is_empty() {
            return (FileChangeSummary::default(), fs.overlays());
        }
        let start = Instant::now();
        let count = pending.len();
        let processed = fs.process_changes(std::mem::take(pending));
        self.log(|| {
            format!(
                "Processed {count} file changes in {}",
                duration_string(start.elapsed())
            )
        });
        processed
    }

    /// tsgo's `if s.options.LoggingEnabled { s.logger.Log(...) }`.
    fn log(&self, message: impl FnOnce() -> String) {
        if let Some(logger) = self
            .logger
            .as_ref()
            .filter(|_| self.host.options().logging_enabled)
        {
            logger.log(&message());
        }
    }
}

/// tsgo's `hr` under a project in the logs.
const HR: &str = "-----------------------------------------------";

/// tsgo `Project.print`: the project and its file count (and, verbose, the
/// files).
fn print_project(project: &Project, write_file_names: bool) -> String {
    let mut text = format!("\nProject '{}'\n", project.id);
    match &project.program {
        None => text.push_str("\tFiles (0) NoProgram\n"),
        Some(program) => {
            let files = program.prepared().source_files();
            text.push_str(&format!("\tFiles ({})\n", files.len()));
            if write_file_names {
                for file in files {
                    text.push_str("\t\t");
                    text.push_str(&file.path().display().to_string_lossy());
                    text.push('\n');
                }
            }
        }
    }
    text.push_str(HR);
    text
}

/// tsgo's errors for a document's default project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DocumentProjectError {
    /// The snapshot update failed.
    Update(ProjectError),
    /// No project has the document; tsgo
    /// `ErrNoProjectForUnknownScriptKind` when the file's kind is unknown.
    NoProject {
        uri: String,
        unknown_script_kind: bool,
    },
}

impl fmt::Display for DocumentProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Update(error) => error.fmt(formatter),
            Self::NoProject {
                uri,
                unknown_script_kind: true,
            } => write!(
                formatter,
                "no project for unknown script kind: no project found for URI {uri}"
            ),
            Self::NoProject { uri, .. } => write!(formatter, "no project found for URI {uri}"),
        }
    }
}

impl std::error::Error for DocumentProjectError {}

/// A document's file name; a URI that names none has none.
fn document_file_name(uri: &DocumentUri) -> Option<String> {
    uri_to_file_name(uri.as_str()).ok()
}

#[cfg(test)]
#[path = "../tests/unit/session.rs"]
mod tests;
