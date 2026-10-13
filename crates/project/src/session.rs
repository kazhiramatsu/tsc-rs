//! tsgo `project.Session` (session.go): the language server's projects. The
//! editor's notifications are kept as pending file changes over the open
//! documents; an open applies them at once, a request for a document
//! applies the pending ones (tsgo `getSnapshot`), and a close or a config's
//! watch event schedules an update in the background, each into a new
//! snapshot the session adopts.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, RwLock};
use std::time::Instant;

use tsc_host::go_time::duration_string;
use tsc_host::vfs::FileSystem;
use tsc_ls::lsutil::UserPreferences;
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, LanguageKind,
    TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::content_mapper::host::Spawner;
use tsc_program::uri::uri_to_file_name;
use tsc_syntax::ScriptKind;

use crate::background::{Cancellation, Queue};
use crate::client::{Client, Logger};
use crate::fs::{FileChangeSummary, Paths};
use crate::overlay::{FileChange, FileChangeKind, OverlayFs, Overlays};
use crate::project::Project;
use crate::snapshot::{ProjectError, SessionOptions, Snapshot, SnapshotChange, SnapshotHost};

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
        lock(&self.inner.pending_file_changes).push(FileChange {
            version,
            changes,
            ..FileChange::new(FileChangeKind::Change, uri.clone())
        });
    }

    /// tsgo `DidSaveFile`: pending until the next update.
    pub fn did_save_file(&self, uri: &DocumentUri) {
        lock(&self.inner.pending_file_changes)
            .push(FileChange::new(FileChangeKind::Save, uri.clone()));
    }

    /// tsgo `DidChangeWatchedFiles`: pending until the next update, which a
    /// change of a config the snapshot keeps schedules.
    pub fn did_change_watched_files(&self, changes: &[FileEvent]) {
        let snapshot = self.snapshot();
        let mut has_config_change = false;
        let changes = changes
            .iter()
            .filter_map(|change| {
                let kind = match change.r#type {
                    FileChangeType::CREATED => FileChangeKind::WatchCreate,
                    FileChangeType::CHANGED => FileChangeKind::WatchChange,
                    FileChangeType::DELETED => FileChangeKind::WatchDelete,
                    _ => return None,
                };
                if !has_config_change {
                    has_config_change = document_file_name(&change.uri)
                        .is_some_and(|file_name| snapshot.is_config_tracked(&file_name));
                }
                Some(FileChange::new(kind, change.uri.clone()))
            })
            .collect::<Vec<_>>();
        lock(&self.inner.pending_file_changes).extend(changes);
        if has_config_change {
            self.inner.schedule_snapshot_update();
        }
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

    /// tsgo `WaitForBackgroundTasks`.
    pub fn wait_for_background_tasks(&self) {
        self.inner.background.wait();
    }

    /// tsgo `Close`: the scheduled update cancelled, the background work
    /// ended, and the content mappers' processes with it.
    pub fn close(&self) {
        let inner = &self.inner;
        inner.cancel_scheduled_snapshot_update();
        inner.background.close();
        let _update = lock(&inner.snapshot_update);
        inner.host.close();
    }
}

impl Inner {
    fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.snapshot.read().unwrap_or_else(PoisonError::into_inner))
    }

    /// tsgo `UpdateSnapshot`: the current snapshot with `change`, adopted.
    fn update_snapshot(
        &self,
        overlays: Overlays,
        change: SnapshotChange<'_>,
    ) -> Result<Arc<Snapshot>, ProjectError> {
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
        *current = Arc::clone(&snapshot);
        Ok(snapshot)
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
