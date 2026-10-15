//! tsgo `project.Session` (session.go): the language server's projects. The
//! editor's notifications are kept as pending file changes over the open
//! documents; an open applies them at once, a request for a document
//! applies the pending ones (tsgo `getSnapshot`), and a close or a config's
//! watch event schedules an update in the background, each into a new
//! snapshot the session adopts.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::{Duration, Instant};

use tsc_compiler::locale::Locale;
use tsc_diagnostics::DiagnosticList;
use tsc_host::go_time::duration_string;
use tsc_host::vfs::FileSystem;
use tsc_ls::lsconv::{diagnostic_to_lsp, DiagnosticOptions, TextScript};
use tsc_ls::lsutil::UserPreferences;
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, LanguageKind, PublishDiagnosticsParams,
    TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::content_mapper::host::Spawner;
use tsc_program::uri::uri_to_file_name;
use tsc_program::ConfigOptionBag;
use tsc_syntax::ScriptKind;

use crate::ata::{NpmExecutor, TypingsInstallRequest, TypingsInstaller};
use crate::background::{Cancellation, Queue};
use crate::client::{Client, Logger};
use crate::fs::{FileChangeSummary, Paths};
use crate::id::{ProjectId, ProjectKind};
use crate::overlay::{FileChange, FileChangeKind, OverlayFs, Overlays};
use crate::project::{CommandLine, ProgramUpdateKind, Project};
use crate::snapshot::ContentMapperContributions;
use crate::snapshot::{
    ApiSnapshotRequest, AtaStateChange, ProjectError, ProjectTreeRequest, SessionOptions, Snapshot,
    SnapshotChange, SnapshotHost,
};
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

/// tsgo `ResourceRequest`: what a request needs brought up to date in the
/// snapshot.
#[derive(Default)]
struct ResourceRequest<'a> {
    /// tsgo `Documents`: the default projects of these, an inferred one for
    /// a file without.
    documents: &'a [DocumentUri],
    /// tsgo `ConfiguredProjectDocuments`: the configured projects of these.
    configured_project_documents: &'a [DocumentUri],
    /// tsgo `Projects`.
    projects: Vec<ProjectId>,
    /// tsgo `ProjectTree`.
    project_tree: Option<ProjectTreeRequest>,
}

/// tsgo `Session`. A clone is the same session (an API session over the
/// language server holds it).
#[derive(Clone)]
pub struct Session {
    inner: Arc<Inner>,
}

struct Inner {
    /// Shared with the API sessions over the language server (tsgo
    /// `SnapshotHost`).
    host: Arc<SnapshotHost>,
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
    /// tsgo `seenProjects`: the projects whose information was sent.
    seen_projects: Mutex<HashSet<ProjectId>>,
    /// tsgo `registeredContentMapperExtensions` and
    /// `registeredContentMapperSnapshotID` (under
    /// `contentMapperRegistrationMu`).
    content_mapper_registration: Mutex<(Vec<String>, u64)>,
    /// tsgo `typingsInstaller`: with a typings location and npm.
    typings_installer: Option<Arc<TypingsInstaller>>,
    /// tsgo `pendingATAChanges`: the installed typings the next update takes.
    pending_ata_changes: Mutex<BTreeMap<ProjectId, AtaStateChange>>,
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
    /// tsgo `NewSession`: with a typings location and `npm_executor`, the
    /// session installs the typings its projects acquire.
    pub fn new(
        options: SessionOptions,
        fs: Arc<dyn FileSystem>,
        client: Option<Arc<dyn Client>>,
        logger: Option<Arc<dyn Logger>>,
        spawner: Option<Arc<dyn Spawner>>,
        npm_executor: Option<Arc<dyn NpmExecutor>>,
    ) -> Self {
        let typings_installer =
            options
                .typings_location
                .clone()
                .zip(npm_executor)
                .map(|(location, npm_executor)| {
                    Arc::new(TypingsInstaller::new(
                        location,
                        Arc::clone(&fs),
                        npm_executor,
                    ))
                });
        let position_encoding = options.position_encoding.clone();
        let paths = Paths {
            current_directory: options.current_directory.clone(),
            case_sensitive: fs.case_sensitive(),
        };
        let locale = client.as_ref().map_or("", |client| client.locale().tag());
        let content_mapper_logger = logger.clone().map(|logger| {
            let logger: tsc_program::content_mapper::host::Logger =
                Arc::new(move |line: &str| logger.content_mapper_log(line));
            logger
        });
        let host = SnapshotHost::new(
            options,
            Arc::clone(&fs),
            spawner,
            locale,
            content_mapper_logger,
        );
        let host = Arc::new(host);
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
                content_mapper_registration: Mutex::default(),
                seen_projects: Mutex::default(),
                typings_installer,
                pending_ata_changes: Mutex::default(),
            }),
        }
    }

    /// The session's snapshot host.
    pub fn host(&self) -> &Arc<SnapshotHost> {
        &self.inner.host
    }

    /// tsgo `FS`: the open documents over the host's file system, as they
    /// are now.
    pub fn fs(&self) -> Arc<dyn FileSystem> {
        Arc::new(lock(&self.inner.fs).layer())
    }

    /// tsgo `WithCurrentLocale`: the client's locale (the default without
    /// a client).
    pub fn locale(&self) -> Locale {
        self.inner
            .client
            .as_ref()
            .map_or_else(Locale::default, |client| client.locale())
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
            let old_locale = client.locale();
            client.set_locale(&config.locale);
            let new_locale = client.locale();
            if old_locale.tag() != new_locale.tag() {
                if let Some(host) = self.inner.host.content_mapper_host() {
                    host.set_locale(new_locale.tag());
                }
            }
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
        self.get_snapshot_for(ResourceRequest {
            documents,
            ..ResourceRequest::default()
        })
    }

    /// tsgo `GetLanguageServicesForDocumentsLoadingProjectTree`'s snapshot:
    /// the documents' default projects and every project tree loaded (its
    /// language service projects are the ones to search).
    pub fn get_snapshot_loading_project_tree(
        &self,
        documents: &[DocumentUri],
    ) -> Result<Arc<Snapshot>, ProjectError> {
        self.get_snapshot_for(ResourceRequest {
            documents,
            project_tree: Some(ProjectTreeRequest::all()),
            ..ResourceRequest::default()
        })
    }

    /// tsgo `WithSnapshotLoadingProjectTree`'s snapshot: the requested
    /// project trees loaded.
    pub fn get_snapshot_loading_project_trees(
        &self,
        request: ProjectTreeRequest,
    ) -> Result<Arc<Snapshot>, ProjectError> {
        self.get_snapshot_for(ResourceRequest {
            project_tree: Some(request),
            ..ResourceRequest::default()
        })
    }

    /// tsgo `GetProjectsForFile`: the snapshot with the file's configured
    /// projects loaded (no inferred project for a file without one), and
    /// the language service projects that have the file.
    pub fn get_projects_for_file(
        &self,
        uri: &DocumentUri,
    ) -> Result<(Arc<Snapshot>, Vec<Arc<Project>>), ProjectError> {
        let snapshot = self.get_snapshot_for(ResourceRequest {
            configured_project_documents: std::slice::from_ref(uri),
            ..ResourceRequest::default()
        })?;
        let file_name = document_file_name(uri).unwrap_or_default();
        let projects = snapshot
            .projects_containing_file(&file_name)
            .into_iter()
            .cloned()
            .collect();
        Ok((snapshot, projects))
    }

    /// tsgo `GetLanguageServiceForProjectWithFile`'s snapshot and project:
    /// the project's program brought up to date, and the project when it
    /// still has the file.
    pub fn get_project_with_file(
        &self,
        id: &ProjectId,
        uri: &DocumentUri,
    ) -> Result<(Arc<Snapshot>, Option<Arc<Project>>), ProjectError> {
        let snapshot = self.get_snapshot_for(ResourceRequest {
            projects: vec![id.clone()],
            ..ResourceRequest::default()
        })?;
        let file_name = document_file_name(uri).unwrap_or_default();
        let project = snapshot
            .project(id)
            .filter(|project| snapshot.project_has_file(project, &file_name))
            .cloned();
        Ok((snapshot, project))
    }

    /// tsgo `getSnapshot`: with pending changes, a new snapshot for the
    /// request; else one only for projects or project trees to bring up to
    /// date, a document whose default project is not loaded or is dirty,
    /// or a configured project document that is not open (or whose default
    /// project is not loaded or is dirty).
    fn get_snapshot_for(
        &self,
        request: ResourceRequest<'_>,
    ) -> Result<Arc<Snapshot>, ProjectError> {
        let inner = &self.inner;
        let _update = lock(&inner.snapshot_update);
        inner.cancel_scheduled_snapshot_update();
        let (changes, overlays, ata_changes, new_config) = inner.flush_changes();
        let file_names = |documents: &[DocumentUri]| {
            documents
                .iter()
                .filter_map(document_file_name)
                .collect::<Vec<_>>()
        };
        let documents = file_names(request.documents);
        let configured_project_documents = file_names(request.configured_project_documents);
        if !changes.is_empty() || !ata_changes.is_empty() || new_config.is_some() {
            return inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    ata_changes,
                    new_config,
                    documents,
                    configured_project_documents,
                    projects: request.projects,
                    project_tree: request.project_tree,
                    ..SnapshotChange::default()
                },
            );
        }
        let snapshot = inner.snapshot();
        let not_loaded_or_dirty = |document: &String| {
            snapshot
                .default_project(document)
                .is_none_or(|project| project.dirty)
        };
        let stale = !request.projects.is_empty()
            || request.project_tree.is_some()
            || documents.iter().any(not_loaded_or_dirty)
            || configured_project_documents
                .iter()
                .any(|document| !snapshot.is_open_file(document) || not_loaded_or_dirty(document));
        if !stale {
            return Ok(snapshot);
        }
        inner.update_snapshot(
            overlays,
            SnapshotChange {
                documents,
                configured_project_documents,
                projects: request.projects,
                project_tree: request.project_tree,
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

    /// tsgo `GetLanguageServiceWithAutoImports`: a snapshot derived from
    /// `base` with the document's auto-imports prepared (tsgo
    /// `CloneSnapshotWithAutoImports`) and the document's default project in
    /// it. The snapshot becomes the session's in the background when the
    /// session has not moved on from `base`.
    pub fn get_snapshot_with_auto_imports(
        &self,
        base: &Arc<Snapshot>,
        uri: &DocumentUri,
    ) -> Result<(Arc<Snapshot>, Arc<Project>), DocumentProjectError> {
        let file_name = document_file_name(uri).unwrap_or_default();
        let snapshot = Arc::new(
            self.inner
                .host
                .clone_snapshot(
                    base,
                    SnapshotChange {
                        documents: vec![file_name.clone()],
                        auto_imports: Some(file_name.clone()),
                        client: self.inner.client.as_deref(),
                        base_file_system: true,
                        ..SnapshotChange::default()
                    },
                )
                .map_err(DocumentProjectError::Update)?,
        );
        let Some(project) = snapshot.default_project(&file_name).cloned() else {
            return Err(DocumentProjectError::NoProject {
                uri: uri.as_str().to_owned(),
                unknown_script_kind: false,
            });
        };
        self.try_adopt_snapshot_in_background(base, &snapshot);
        Ok((snapshot, project))
    }

    /// tsgo `TryAdoptSnapshotInBackground`: `new`, derived from `base`,
    /// becomes the session's snapshot in the background unless the session
    /// has moved on from `base` by then.
    pub fn try_adopt_snapshot_in_background(&self, base: &Arc<Snapshot>, new: &Arc<Snapshot>) {
        let inner = Arc::clone(&self.inner);
        let base = Arc::clone(base);
        let new = Arc::clone(new);
        self.inner
            .background
            .enqueue(&Cancellation::default(), move |_| {
                inner.adopt_snapshot_change(&base, new);
            });
    }

    /// tsgo `APIUpdate`: an API session's request over the language
    /// server's state, with the pending changes, adopted as the session's
    /// snapshot. A failed request is rejected as a unit (nothing of it is
    /// adopted), but the changes already flushed still advance the
    /// session's snapshot.
    pub fn api_update(&self, request: &ApiSnapshotRequest) -> Result<Arc<Snapshot>, ProjectError> {
        let inner = &self.inner;
        let _update = lock(&inner.snapshot_update);
        inner.cancel_scheduled_snapshot_update();
        // tsgo flushes (and so drops) the pending preferences here too.
        let (file_changes, overlays, ata_changes, _) = inner.flush_changes();
        let result = inner.update_snapshot(
            Arc::clone(&overlays),
            SnapshotChange {
                file_changes: file_changes.clone(),
                ata_changes: ata_changes.clone(),
                api_request: Some(request),
                ..SnapshotChange::default()
            },
        );
        if result.is_err() && (!file_changes.is_empty() || !ata_changes.is_empty()) {
            let _ = inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes,
                    ata_changes,
                    ..SnapshotChange::default()
                },
            );
        }
        result
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

    /// tsgo `SetContentMapperContributions`: the editor's mappers replace
    /// the inferred projects' (configured projects never take them), and
    /// the configured projects of the open `documents` are found (a file
    /// of a contributed extension may belong to one). Nothing without
    /// `run_external_code`.
    pub fn set_content_mapper_contributions(
        &self,
        contributions: ContentMapperContributions,
        documents: &[DocumentUri],
    ) {
        let inner = &self.inner;
        if !inner.host.options().run_external_code {
            return;
        }
        inner.cancel_scheduled_snapshot_update();
        let snapshot = {
            let _update = lock(&inner.snapshot_update);
            let (file_changes, overlays) = {
                let mut pending = lock(&inner.pending_file_changes);
                inner.flush_changes_locked(&mut pending)
            };
            inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes,
                    content_mapper_contributions: Some(contributions),
                    configured_project_documents: documents
                        .iter()
                        .filter_map(document_file_name)
                        .collect(),
                    ..SnapshotChange::default()
                },
            )
        };
        if let Ok(snapshot) = snapshot {
            let _ = inner.update_content_mapper_registrations(&snapshot);
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

    /// tsgo `adoptSnapshotChange`: `new` becomes the session's snapshot when
    /// the session is still at `base`; otherwise it is dropped.
    fn adopt_snapshot_change(&self, base: &Arc<Snapshot>, new: Arc<Snapshot>) {
        let mut current = self
            .snapshot
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if Arc::ptr_eq(&current, base) {
            let old = std::mem::replace(&mut *current, Arc::clone(&new));
            drop(current);
            self.log(|| {
                format!(
                    "Adopted snapshot {} (parent {}) as current session snapshot (replacing {})",
                    new.id(),
                    new.parent_id(),
                    old.id()
                )
            });
        } else {
            let moved_on = current.id();
            drop(current);
            self.log(|| {
                format!(
                    "Discarded snapshot {} (parent {}); session has moved on to snapshot {}",
                    new.id(),
                    new.parent_id(),
                    moved_on
                )
            });
        }
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
                    client: self.client.as_deref(),
                    ..change
                },
            )?);
            (
                std::mem::replace(&mut *current, Arc::clone(&snapshot)),
                snapshot,
            )
        };
        // tsgo: the projects whose typings may have changed install them.
        if self.typings_installer.is_some() && !lock(&self.user_config).workspace.is_ata_disabled()
        {
            self.trigger_ata_for_updated_projects(&snapshot);
        }
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
            let _ = inner.update_content_mapper_registrations(&new);
            inner.publish_program_diagnostics(&old, &new);
            inner.send_project_info_telemetry_for_new_projects(&old, &new);
        });
        Ok(snapshot)
    }

    /// tsgo `triggerATAForUpdatedProjects`: each project whose typings may
    /// have changed installs them in the background.
    fn trigger_ata_for_updated_projects(self: &Arc<Self>, snapshot: &Snapshot) {
        for project in snapshot.projects() {
            if !project.should_trigger_ata(snapshot.id()) {
                continue;
            }
            let inner = Arc::clone(self);
            let project = Arc::clone(project);
            self.background.enqueue(&Cancellation::default(), move |_| {
                inner.install_project_typings(&project);
            });
        }
    }

    /// tsgo's ATA task: the project's typings installed (with the editor's
    /// progress); new ones wait for the next update, which the diagnostics
    /// refresh brings.
    fn install_project_typings(self: &Arc<Self>, project: &Project) {
        let (Some(installer), Some(typings_info)) =
            (&self.typings_installer, project.compute_typings_info())
        else {
            return;
        };
        let logging = self.logging_enabled();
        let mut logs = Vec::new();
        if logging {
            logs.push(format!("Triggering ATA for project {}", project.id));
        }
        let file_names = project.program.as_ref().map_or_else(Vec::new, |program| {
            program
                .prepared()
                .source_files()
                .iter()
                .map(|file| file.path().display().to_string_lossy().into_owned())
                .collect()
        });
        let display_name = project.display_name(&self.host.options().current_directory);
        if let Some(client) = &self.client {
            client.progress_start(
                &tsc_diagnostics::gen::Installing_types_for_0,
                std::slice::from_ref(&display_name),
            );
        }
        let fs = lock(&self.fs).layer();
        let result = installer.install_typings(TypingsInstallRequest {
            project_id: project.id.as_str(),
            typings_info: &typings_info,
            file_names: &file_names,
            project_root_path: &project.current_directory,
            fs: &fs,
            logger: &mut |line| {
                if logging {
                    logs.push(line);
                }
            },
        });
        if let Some(client) = &self.client {
            client.progress_finish(
                &tsc_diagnostics::gen::Installing_types_for_0,
                std::slice::from_ref(&display_name),
            );
        }
        match result {
            Err(error) => {
                if logging {
                    self.log(|| {
                        format!(
                            "ATA installation failed for project {}: {error}",
                            project.id
                        )
                    });
                    self.log(|| logs.join("\n"));
                }
            }
            Ok(result) if result.typings_files != project.typings_files => {
                if logging {
                    self.log(|| logs.join("\n"));
                }
                lock(&self.pending_ata_changes).insert(
                    project.id.clone(),
                    AtaStateChange {
                        typings_info,
                        typings_files: result.typings_files,
                        typings_files_to_watch: result.files_to_watch,
                        logs,
                    },
                );
                self.schedule_diagnostics_refresh(self.host.options().debounce_delay);
            }
            Ok(_) => {}
        }
    }

    /// tsgo `sendProjectInfoTelemetryForNewProjects`: the information of each
    /// project the snapshot added, once per project in the session.
    fn send_project_info_telemetry_for_new_projects(&self, old: &Snapshot, new: &Snapshot) {
        if !self.host.options().telemetry_enabled {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        for project in new.projects() {
            if old.project(&project.id).is_some() || lock(&self.seen_projects).contains(&project.id)
            {
                continue;
            }
            let Some(event) = project_info_telemetry(project) else {
                continue;
            };
            if let Err(error) = client.send_telemetry(event) {
                self.log(|| format!("Error sending project info telemetry: {error}"));
                continue;
            }
            lock(&self.seen_projects).insert(project.id.clone());
        }
    }

    /// tsgo `updateContentMapperRegistrations`: the editor registers the
    /// extensions of every loaded config's content mappers and of the
    /// editor's contributions, so their (otherwise unsupported) files flow
    /// to the session once a config that maps them is found. An older
    /// snapshot's task (they may finish out of order) changes nothing; a
    /// failure leaves the state for the next update to retry.
    fn update_content_mapper_registrations(&self, snapshot: &Snapshot) -> Result<(), String> {
        let Some(client) = &self.client else {
            return Ok(());
        };
        let extensions = snapshot.content_mapper_extensions();
        let mut registration = lock(&self.content_mapper_registration);
        let (registered, registered_snapshot) = &mut *registration;
        if snapshot.id() <= *registered_snapshot {
            return Ok(());
        }
        if extensions == *registered {
            *registered_snapshot = snapshot.id();
            return Ok(());
        }
        // The client replaces the prior registration wholesale (an empty set removes it).
        if let Err(error) = client.register_content_mapper_extensions(&extensions) {
            self.log(|| error.clone());
            return Err(error);
        }
        *registered = extensions;
        *registered_snapshot = snapshot.id();
        Ok(())
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
                .map(|(file_name, text)| TextScript::new(file_name.clone(), text))
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
                errors.extend(self.update_watch(None, project.typings_watch.as_deref()));
                errors.extend(self.update_watch(None, project.content_mapper_watch.as_deref()));
            }
        }
        for project in old.projects() {
            let Some(new_project) = new_projects.get(&project.id) else {
                errors.extend(self.update_watch(project.program_files_watch.as_deref(), None));
                errors.extend(self.update_watch(project.typings_watch.as_deref(), None));
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
                project.typings_watch.as_deref(),
                new_project.typings_watch.as_deref(),
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
            let (changes, overlays, ata_changes, new_config) = inner.flush_changes();
            if changes.is_empty() && ata_changes.is_empty() && new_config.is_none() {
                return;
            }
            // tsgo's background update has no caller to answer; a failed
            // update leaves the snapshot as it was.
            let _ = inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    ata_changes,
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
            let (changes, overlays, ata_changes, new_config) = inner.flush_changes();
            let _ = inner.update_snapshot(
                overlays,
                SnapshotChange {
                    file_changes: changes,
                    ata_changes,
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
    #[allow(clippy::type_complexity)]
    fn flush_changes(
        &self,
    ) -> (
        FileChangeSummary,
        Overlays,
        BTreeMap<ProjectId, AtaStateChange>,
        Option<UserPreferences>,
    ) {
        let mut pending = lock(&self.pending_file_changes);
        let ata_changes = std::mem::take(&mut *lock(&self.pending_ata_changes));
        let (changes, overlays) = self.flush_changes_locked(&mut pending);
        drop(pending);
        let mut user_config = lock(&self.user_config);
        let new_config = user_config.pending.then(|| user_config.workspace.clone());
        user_config.pending = false;
        (changes, overlays, ata_changes, new_config)
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

/// tsgo `collectProjectInfoTelemetry`: a project's kind, config file name
/// and shape, compiler options (booleans and enum names, no paths) and the
/// counts and sizes of its files by kind; none without a program.
fn project_info_telemetry(project: &Project) -> Option<tsc_lsproto::TelemetryEvent> {
    let program = project.program()?;
    let command_line = project.command_line()?;
    let options = command_line.compiler_options();
    let configured = project.kind() == ProjectKind::Configured;
    let config_file_name = project
        .config_file_name()
        .filter(|_| configured)
        .map(|name| name.rsplit('/').next().unwrap_or(name))
        .filter(|base| matches!(*base, "tsconfig.json" | "jsconfig.json"))
        .unwrap_or("other");
    let mut properties = BTreeMap::from([
        ("configFileName".to_owned(), config_file_name.to_owned()),
        (
            "projectType".to_owned(),
            if configured { "configured" } else { "inferred" }.to_owned(),
        ),
        (
            "version".to_owned(),
            tsc_types::TYPESCRIPT_VERSION.to_owned(),
        ),
    ]);
    // Same approach as Strada's convertCompilerOptionsForTelemetry: booleans and enum string names, no paths.
    let mut compiler_options = BTreeMap::new();
    let bag = command_line.options();
    for name in [
        "strict",
        "noImplicitAny",
        "noImplicitThis",
        "strictNullChecks",
        "strictFunctionTypes",
        "strictBindCallApply",
        "strictPropertyInitialization",
        "strictBuiltinIteratorReturn",
        "useUnknownInCatchVariables",
        "exactOptionalPropertyTypes",
        "allowJs",
        "checkJs",
        "noEmit",
        "declaration",
        "composite",
        "isolatedModules",
        "skipLibCheck",
        "incremental",
    ] {
        if let Some(value) = bag.option_bool(name) {
            compiler_options.insert(name, serde_json::Value::Bool(value));
        }
    }
    for (name, value) in [
        ("target", options.target.and_then(script_target_name)),
        ("module", options.module.and_then(module_kind_name)),
        (
            "moduleResolution",
            options.module_resolution.and_then(module_resolution_name),
        ),
        ("jsx", options.jsx.and_then(jsx_emit_name)),
    ] {
        if let Some(value) = value {
            compiler_options.insert(name, serde_json::Value::String(value.to_owned()));
        }
    }
    if let Ok(json) = serde_json::to_string(&compiler_options) {
        properties.insert("compilerOptions".to_owned(), json);
    }
    // Config file shape.
    if let CommandLine::Config(plan) = command_line {
        if let Some(raw) = plan.raw().as_object() {
            for key in ["extends", "files", "include", "exclude"] {
                properties.insert(key.to_owned(), raw.contains_key(key).to_string());
            }
        }
    }
    let mut measurements = tsc_lsproto::ProjectInfoTelemetryMeasurements::default();
    for source in program.prepared().source_files() {
        let size = source.text().len() as f64;
        match source.script_kind() {
            ScriptKind::Js => {
                measurements.js_file_count += 1.0;
                measurements.js_file_size += size;
            }
            ScriptKind::Jsx => {
                measurements.jsx_file_count += 1.0;
                measurements.jsx_file_size += size;
            }
            ScriptKind::Ts => {
                if tsc_program::is_declaration_file_name(&source.path().display().to_string_lossy())
                {
                    measurements.dts_file_count += 1.0;
                    measurements.dts_file_size += size;
                } else {
                    measurements.ts_file_count += 1.0;
                    measurements.ts_file_size += size;
                }
            }
            ScriptKind::Tsx => {
                measurements.tsx_file_count += 1.0;
                measurements.tsx_file_size += size;
            }
            _ => {}
        }
    }
    Some(tsc_lsproto::TelemetryEvent::ProjectInfoTelemetryEvent(
        tsc_lsproto::ProjectInfoTelemetryEvent {
            properties,
            measurements,
            ..tsc_lsproto::ProjectInfoTelemetryEvent::default()
        },
    ))
}

/// tsgo `ScriptTarget.String()`.
fn script_target_name(target: i32) -> Option<&'static str> {
    Some(match target {
        1 => "ES5",
        2 => "ES2015",
        3 => "ES2016",
        4 => "ES2017",
        5 => "ES2018",
        6 => "ES2019",
        7 => "ES2020",
        8 => "ES2021",
        9 => "ES2022",
        10 => "ES2023",
        11 => "ES2024",
        12 => "ES2025",
        13 => "ES2026",
        99 => "ESNext",
        100 => "JSON",
        _ => return None,
    })
}

/// tsgo `ModuleKind.String()`.
fn module_kind_name(module: i32) -> Option<&'static str> {
    Some(match module {
        1 => "CommonJS",
        2 => "AMD",
        3 => "UMD",
        4 => "System",
        5 => "ES2015",
        6 => "ES2020",
        7 => "ES2022",
        99 => "ESNext",
        100 => "Node16",
        101 => "Node18",
        102 => "Node20",
        199 => "NodeNext",
        200 => "Preserve",
        _ => return None,
    })
}

/// tsgo `ModuleResolutionKind.String()`.
fn module_resolution_name(kind: i32) -> Option<&'static str> {
    Some(match kind {
        1 => "Classic",
        2 => "Node10",
        3 => "Node16",
        99 => "NodeNext",
        100 => "Bundler",
        _ => return None,
    })
}

/// tsgo `JsxEmit.String()`.
fn jsx_emit_name(jsx: i32) -> Option<&'static str> {
    Some(match jsx {
        1 => "preserve",
        2 => "react",
        3 => "react-native",
        4 => "react-jsx",
        5 => "react-jsxdev",
        _ => return None,
    })
}
