//! tsgo `api/session.go` (19dadef8): the session that answers an API
//! client's requests over snapshots of tsgo's project system
//! ([`tsc_project`]). The methods ported so far are `echo`, `ping`,
//! `initialize`, `batchRequests`, `createSnapshot`, `updateSnapshot`,
//! `release` and `getDefaultProjectForFile`, and
//! `getCurrentLanguageServerSnapshot` answers as tsgo's standalone session
//! does; tsgo's other methods answer that they are not implemented yet.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use base64::Engine;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::value::RawValue;
use tsc_compiler::BoundDocument;
use tsc_host::vfs::FileSystem;
use tsc_program::go_json::{
    compiler_options_json, struct_json, GoJson, COMPILER_OPTIONS_FIELDS, TYPE_ACQUISITION_FIELDS,
};
use tsc_program::{ConfigProjectReference, JsonValue};
use tsc_project::{
    ApiSnapshotRequest, CommandLine, CreateProgramRequest, FileChangeSummary, Project, ProjectId,
    ProjectKind, ProjectProgram, ReconfigureProgramRequest, SessionOptions, Snapshot, SnapshotHost,
};

use crate::ipc::{panic_message, Handler, Payload};
use crate::proto::{
    BatchRequest, BatchRequestsParams, ConfigFileResponse, CreateProgramOptions,
    CreateSnapshotParams, CreateSnapshotResponse, DiagnosticResponse, DocumentIdentifier,
    EnsurePrograms, FileNotifications, GetDefaultProjectForFileParams, InitializeResponse,
    OpenedFileOperationResult, ProjectFileChanges, ProjectReference, ProjectResponse,
    ReleaseParams, SnapshotChanges, SnapshotId, SnapshotOperationResponse, SnapshotRequestChanges,
    SourceFileResponse, UpdateSnapshotParams,
};

/// tsgo `ErrInvalidRequest`.
pub const INVALID_REQUEST: &str = "api: invalid request";
/// tsgo `ErrClientError`.
pub const CLIENT_ERROR: &str = "api: client error";

/// tsgo `DefaultMaxResponseBytesPerPage`: room for base64 expansion below
/// V8's longest string.
pub const DEFAULT_MAX_RESPONSE_BYTES_PER_PAGE: usize = 300_000_000;

/// tsgo `sessionIDCounter`.
static SESSION_IDS: AtomicU64 = AtomicU64::new(0);

fn client_error(message: impl std::fmt::Display) -> String {
    format!("{CLIENT_ERROR}: {message}")
}

fn invalid_request(message: impl std::fmt::Display) -> String {
    format!("{INVALID_REQUEST}: {message}")
}

fn json<T: Serialize>(value: &T) -> Payload {
    Payload::Json(serde_json::to_string(value).expect("a response serializes"))
}

fn raw_json(text: String) -> Box<RawValue> {
    RawValue::from_string(text).expect("tsgo's JSON form is valid JSON")
}

/// The projects and files a snapshot has open through this session (tsgo
/// `snapshotOpenState`), by path.
#[derive(Clone, Debug, Default)]
struct OpenState {
    projects: BTreeSet<String>,
    files: BTreeSet<String>,
}

/// tsgo `snapshotData`: a snapshot the client holds, with the number of
/// times it was handed out.
struct SnapshotData {
    snapshot: Arc<Snapshot>,
    ref_count: usize,
    open: OpenState,
}

/// tsgo `api.Session`.
pub struct Session {
    id: String,
    host: SnapshotHost,
    /// MessagePack sends binary responses as they are (tsgo
    /// `UseBinaryResponses`).
    use_binary_responses: bool,
    snapshots: Mutex<BTreeMap<SnapshotId, SnapshotData>>,
    /// The responses of a batch that did not fit its first page, by
    /// continuation token.
    batch_pages: Mutex<BTreeMap<String, Vec<String>>>,
    next_batch_page: AtomicU64,
}

impl Session {
    /// tsgo `NewStandaloneSession`: a session with its own snapshot host.
    pub fn new(
        options: SessionOptions,
        fs: Arc<dyn FileSystem>,
        use_binary_responses: bool,
    ) -> Self {
        let id = SESSION_IDS.fetch_add(1, Ordering::Relaxed) + 1;
        Self {
            id: format!("api-session-{id}"),
            host: SnapshotHost::new(options, fs),
            use_binary_responses,
            snapshots: Mutex::default(),
            batch_pages: Mutex::default(),
            next_batch_page: AtomicU64::new(0),
        }
    }

    /// tsgo `ID`.
    pub fn id(&self) -> &str {
        &self.id
    }

    fn current_directory(&self) -> &str {
        &self.host.options().current_directory
    }

    /// tsgo `HandleRequest`: the method's result or its error's text.
    pub fn handle(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        match method {
            "echo" => {
                return Ok(if self.use_binary_responses {
                    Payload::Binary(params.to_vec())
                } else {
                    // Go writes a JSON value compacted.
                    Payload::Json(compact_json(&String::from_utf8_lossy(params)))
                });
            }
            "ping" => return Ok(json(&"pong")),
            _ => {}
        }
        match method {
            "batchRequests" => {
                let params = parse::<BatchRequestsParams>("BatchRequestsParams", params)?;
                self.handle_batch_requests(&params).map(Payload::Json)
            }
            "release" => {
                let params = parse::<ReleaseParams>("ReleaseParams", params)?;
                self.handle_release(&params).map(|()| json(&true))
            }
            "initialize" => Ok(json(&InitializeResponse {
                use_case_sensitive_file_names: self.host.case_sensitive(),
                current_directory: self.current_directory().to_owned(),
            })),
            "createSnapshot" => {
                let params = parse::<CreateSnapshotParams>("CreateSnapshotParams", params)?;
                self.handle_create_snapshot(&params)
                    .map(|response| json(&response))
            }
            "updateSnapshot" => {
                let params = parse::<UpdateSnapshotParams>("UpdateSnapshotParams", params)?;
                self.handle_update_snapshot(params)
                    .map(|response| json(&response))
            }
            "getDefaultProjectForFile" => {
                let params = parse::<GetDefaultProjectForFileParams>(
                    "GetDefaultProjectForFileParams",
                    params,
                )?;
                self.handle_get_default_project_for_file(&params)
                    .map(|response| json(&response))
            }
            // A standalone session has no language server to share.
            "getCurrentLanguageServerSnapshot" => Err(client_error(
                "getCurrentLanguageServerSnapshot requires an LSP-connected API session",
            )),
            #[cfg(test)]
            "panicForTest" => panic!("test panic"),
            _ if TSGO_METHODS.binary_search(&method).is_ok() => {
                Err(invalid_request(format!("{method} is not implemented yet")))
            }
            _ => Err(invalid_request(format!("unknown API method {method:?}"))),
        }
    }

    /// tsgo `handleBatchRequests`: each request in order, the encoded
    /// responses paged by size.
    fn handle_batch_requests(&self, params: &BatchRequestsParams) -> Result<String, String> {
        let max_bytes = usize::try_from(params.max_response_bytes_per_page)
            .ok()
            .filter(|max| *max > 0)
            .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES_PER_PAGE);
        if !params.continuation_token.is_empty() {
            let page = self
                .lock_batch_pages()
                .remove(&params.continuation_token)
                .ok_or_else(|| client_error("invalid batch continuation token"))?;
            return Ok(self.paginate_batch_responses(page, max_bytes));
        }
        let responses = params
            .requests
            .iter()
            .map(|request| self.handle_batch_request(request))
            .collect();
        Ok(self.paginate_batch_responses(responses, max_bytes))
    }

    /// tsgo `paginateBatchResponses`: as many responses as fit the page
    /// (always one), the rest kept for the continuation token.
    fn paginate_batch_responses(&self, mut responses: Vec<String>, max_bytes: usize) -> String {
        let mut encoded_length = r#"{"responses":[]}"#.len();
        let mut page_length = 0;
        for response in &responses {
            let additional = response.len() + usize::from(page_length > 0);
            if page_length > 0 && encoded_length + additional > max_bytes {
                break;
            }
            encoded_length += additional;
            page_length += 1;
        }
        if page_length == responses.len() {
            return format!(r#"{{"responses":[{}]}}"#, responses.join(","));
        }
        let token = format!(
            "{}-{}",
            self.id,
            self.next_batch_page.fetch_add(1, Ordering::Relaxed) + 1
        );
        let continuation_length = r#","continuationToken":"""#.len() + token.len();
        while page_length > 1 && encoded_length + continuation_length > max_bytes {
            encoded_length -= responses[page_length - 1].len() + 1;
            page_length -= 1;
        }
        let remaining = responses.split_off(page_length);
        self.lock_batch_pages().insert(token.clone(), remaining);
        format!(
            r#"{{"responses":[{}],"continuationToken":{}}}"#,
            responses.join(","),
            serde_json::to_string(&token).expect("a string serializes")
        )
    }

    /// tsgo `handleBatchRequest`: one request's encoded `BatchResponse`; a
    /// panic becomes the request's error.
    fn handle_batch_request(&self, request: &BatchRequest) -> String {
        let method = request.method.as_str();
        let (result, error) = if method == "batchRequests" {
            (
                None,
                Some(invalid_request("batchRequests cannot be nested")),
            )
        } else {
            let params = request
                .params
                .as_ref()
                .map_or(&b""[..], |params| params.get().as_bytes());
            match catch_unwind(AssertUnwindSafe(|| self.handle(method, params))) {
                Ok(Ok(payload)) => (Some(payload), None),
                Ok(Err(error)) => (None, Some(error)),
                Err(panic) => (
                    None,
                    Some(format!("panic: {}", panic_message(panic.as_ref()))),
                ),
            }
        };
        let result = match result {
            None => "null".to_owned(),
            Some(Payload::Json(text)) => text,
            // A binary source file travels as base64 in a batch; other
            // bytes are a JSON byte string (base64), as Go writes them.
            Some(Payload::Binary(bytes)) if is_source_file_response_method(method) => {
                serde_json::to_string(&SourceFileResponse {
                    data: base64::engine::general_purpose::STANDARD.encode(bytes),
                })
                .expect("a response serializes")
            }
            Some(Payload::Binary(bytes)) => {
                serde_json::to_string(&base64::engine::general_purpose::STANDARD.encode(bytes))
                    .expect("a string serializes")
            }
        };
        let mut response = format!(
            r#"{{"method":{},"result":{result}"#,
            serde_json::to_string(method).expect("a string serializes")
        );
        if let Some(error) = error.filter(|error| !error.is_empty()) {
            response.push_str(r#","error":"#);
            response.push_str(&serde_json::to_string(&error).expect("a string serializes"));
        }
        response.push('}');
        response
    }

    /// tsgo `handleRelease`.
    fn handle_release(&self, params: &ReleaseParams) -> Result<(), String> {
        if params.snapshot == 0 {
            return Err(client_error("empty handle"));
        }
        let released = {
            let mut snapshots = self.lock_snapshots();
            let data = snapshots
                .get_mut(&params.snapshot)
                .ok_or_else(|| client_error(format!("snapshot {} not found", params.snapshot)))?;
            data.ref_count -= 1;
            if data.ref_count == 0 {
                snapshots.remove(&params.snapshot)
            } else {
                None
            }
        };
        // The snapshot's programs go outside the lock.
        drop(released);
        Ok(())
    }

    /// tsgo `handleCreateSnapshot`: a snapshot made from an empty one.
    fn handle_create_snapshot(
        &self,
        params: &CreateSnapshotParams,
    ) -> Result<CreateSnapshotResponse, String> {
        reject_file_system(params)?;
        let mut request = self.to_api_snapshot_request(&params.changes)?;
        let open = self.reconcile_snapshot_opens(&mut request, OpenState::default());
        let file_changes = self.to_file_change_summary(params.file_notifications.as_ref())?;
        let root = self.host.new_root_snapshot();
        let snapshot = self
            .host
            .clone_snapshot(&root, file_changes, Some(&request))
            .map_err(|error| client_error(format!("failed to create snapshot: {error}")))?;
        let response = self.create_snapshot_response(&snapshot, None, &params.changes)?;
        self.register_snapshot(snapshot, open);
        Ok(response)
    }

    /// tsgo `handleUpdateSnapshot`: a snapshot made from one the client
    /// holds.
    fn handle_update_snapshot(
        &self,
        params: UpdateSnapshotParams,
    ) -> Result<CreateSnapshotResponse, String> {
        let (base, base_open) = {
            let snapshots = self.lock_snapshots();
            let data = snapshots
                .get(&params.snapshot)
                .ok_or_else(|| client_error(format!("snapshot {} not found", params.snapshot)))?;
            (Arc::clone(&data.snapshot), data.open.clone())
        };
        let changes = params.changes.unwrap_or_default();
        reject_file_system(&changes)?;
        let mut request = self.to_api_snapshot_request(&changes.changes)?;
        let open = self.reconcile_snapshot_opens(&mut request, base_open);
        let file_changes = self.to_file_change_summary(changes.file_notifications.as_ref())?;
        let snapshot = self
            .host
            .clone_snapshot(&base, file_changes, Some(&request))
            .map_err(|error| client_error(format!("failed to update snapshot: {error}")))?;
        let response = self.create_snapshot_response(&snapshot, Some(&base), &changes.changes)?;
        self.register_snapshot(snapshot, open);
        Ok(response)
    }

    /// tsgo `handleGetDefaultProjectForFile`: the file's default project, or
    /// `null` when no project has it.
    fn handle_get_default_project_for_file(
        &self,
        params: &GetDefaultProjectForFileParams,
    ) -> Result<Option<ProjectResponse>, String> {
        let snapshot = self.snapshot(params.snapshot)?;
        let file_name = self.absolute_file_name(&params.file)?;
        Ok(snapshot
            .default_project(&file_name)
            .map(|project| project_response(&snapshot, project)))
    }

    fn snapshot(&self, handle: SnapshotId) -> Result<Arc<Snapshot>, String> {
        self.lock_snapshots()
            .get(&handle)
            .map(|data| Arc::clone(&data.snapshot))
            .ok_or_else(|| client_error(format!("snapshot {handle} not found")))
    }

    /// tsgo `registerSnapshot`.
    fn register_snapshot(&self, snapshot: Snapshot, open: OpenState) {
        self.lock_snapshots().insert(
            snapshot.id(),
            SnapshotData {
                snapshot: Arc::new(snapshot),
                ref_count: 1,
                open,
            },
        );
    }

    fn absolute_file_name(&self, document: &DocumentIdentifier) -> Result<String, String> {
        document
            .to_absolute_file_name(self.current_directory())
            .map_err(client_error)
    }

    /// tsgo `toAPISnapshotRequest`: the request's documents as absolute
    /// file names, its programs validated.
    fn to_api_snapshot_request(
        &self,
        changes: &SnapshotRequestChanges,
    ) -> Result<ApiSnapshotRequest, String> {
        let mut request = ApiSnapshotRequest::default();
        for project in &changes.open_projects {
            let config_file_name = self.absolute_file_name(project)?;
            let Some(id) = ProjectId::configured(&self.host.to_path(&config_file_name)) else {
                return Err(client_error(format!(
                    "invalid configured project ID: {config_file_name}"
                )));
            };
            request.ensure_programs.insert(id);
            request.open_projects.insert(config_file_name);
        }
        for project in &changes.close_projects {
            let path = self.host.to_path(&self.absolute_file_name(project)?);
            request.close_projects.insert(path);
        }
        let mut open_files = BTreeMap::new();
        for file in changes.open_files.iter().flatten() {
            let file_name = self.absolute_file_name(file)?;
            open_files
                .entry(self.host.to_path(&file_name))
                .or_insert(file_name);
        }
        if !open_files.is_empty() {
            request.ensure_files = open_files.values().cloned().collect();
            request.open_files = Some(open_files.into_values().collect());
        }
        if !changes.close_files.is_empty() {
            let mut close_files = BTreeSet::new();
            for file in &changes.close_files {
                close_files.insert(self.host.to_path(&self.absolute_file_name(file)?));
            }
            request.close_files = Some(close_files);
        }
        for (index, program) in changes.create_programs.iter().flatten().enumerate() {
            let Some(program) = program else {
                return Err(client_error(format!(
                    "createPrograms[{index}] must not be null"
                )));
            };
            request.create_programs.push(self.create_program_request(
                &program.root_files,
                &program.compiler_options.0,
                program.options.as_ref(),
            )?);
        }
        let mut reconfigured = BTreeSet::new();
        for (index, program) in changes.reconfigure_programs.iter().enumerate() {
            let Some(program) = program else {
                return Err(client_error(format!(
                    "reconfigurePrograms[{index}] must not be null"
                )));
            };
            let program_id = ProjectId::new(program.id.0.as_str());
            if !reconfigured.insert(program_id.clone()) {
                return Err(client_error(format!(
                    "synthetic program reconfigured more than once: {program_id}"
                )));
            }
            request
                .reconfigure_programs
                .push(ReconfigureProgramRequest {
                    program_id,
                    program: self.create_program_request(
                        &program.root_files,
                        &program.compiler_options.0,
                        program.options.as_ref(),
                    )?,
                });
        }
        for program in &changes.remove_programs {
            let id = ProjectId::new(program.0.as_str());
            if reconfigured.contains(&id) {
                return Err(client_error(format!(
                    "synthetic program cannot be reconfigured and removed: {id}"
                )));
            }
            request.remove_programs.insert(id);
        }
        match &changes.ensure_programs {
            Some(EnsurePrograms::All) => request.ensure_all_programs = true,
            Some(EnsurePrograms::Projects(projects)) => request
                .ensure_programs
                .extend(projects.iter().map(|id| ProjectId::new(id.as_str()))),
            None => {}
        }
        Ok(request)
    }

    fn create_program_request(
        &self,
        root_files: &[DocumentIdentifier],
        options: &tsc_program::ConfigOptionBag,
        program_options: Option<&CreateProgramOptions>,
    ) -> Result<CreateProgramRequest, String> {
        let mut request = CreateProgramRequest {
            root_file_names: root_files
                .iter()
                .map(|file| self.absolute_file_name(file))
                .collect::<Result<_, _>>()?,
            options: options.clone(),
            ..CreateProgramRequest::default()
        };
        if let Some(program_options) = program_options {
            if program_options.module_resolver != 0 {
                return Err(client_error(format!(
                    "module resolver {} not found",
                    program_options.module_resolver
                )));
            }
            request.project_references = program_options
                .project_references
                .iter()
                .map(|reference| ConfigProjectReference {
                    path: reference.path.as_str().into(),
                    original_path: reference.original_path.as_str().into(),
                    prepend: None,
                    circular: reference.circular.then_some(true),
                })
                .collect();
            request.config_file_parsing_diagnostics = program_options
                .config_file_parsing_diagnostics
                .iter()
                .map(DiagnosticResponse::to_diagnostic)
                .collect();
        }
        Ok(request)
    }

    /// tsgo `reconcileSnapshotOpens`: a project or file the base already has
    /// open through this session is not opened again, and one it does not
    /// have open is not closed.
    fn reconcile_snapshot_opens(
        &self,
        request: &mut ApiSnapshotRequest,
        base: OpenState,
    ) -> OpenState {
        let mut state = base;
        request
            .close_projects
            .retain(|path| state.projects.remove(path));
        let open_projects = std::mem::take(&mut request.open_projects);
        for config_file_name in open_projects {
            if state.projects.insert(self.host.to_path(&config_file_name)) {
                request.open_projects.insert(config_file_name);
            }
        }
        if let Some(close_files) = &mut request.close_files {
            close_files.retain(|path| state.files.remove(path));
        }
        if let Some(open_files) = &mut request.open_files {
            let names = std::mem::take(open_files);
            for file_name in names {
                if state.files.insert(self.host.to_path(&file_name)) {
                    open_files.insert(file_name);
                }
            }
        }
        state
    }

    /// tsgo `toFileChangeSummary`: the documents as absolute file names.
    fn to_file_change_summary(
        &self,
        notifications: Option<&FileNotifications>,
    ) -> Result<FileChangeSummary, String> {
        let Some(notifications) = notifications else {
            return Ok(FileChangeSummary::default());
        };
        if notifications.invalidate_all {
            return Ok(FileChangeSummary {
                invalidate_all: true,
                ..FileChangeSummary::default()
            });
        }
        let names = |documents: &[DocumentIdentifier]| {
            documents
                .iter()
                .map(|document| self.absolute_file_name(document))
                .collect::<Result<BTreeSet<_>, _>>()
        };
        Ok(FileChangeSummary {
            changed: names(&notifications.changed)?,
            created: names(&notifications.created)?,
            deleted: names(&notifications.deleted)?,
            invalidate_all: false,
        })
    }

    /// tsgo `createSnapshotResponse`: every project without a base, else the
    /// projects added (in the new snapshot's order) and replaced (in the
    /// base's order), with the files that changed.
    fn create_snapshot_response(
        &self,
        snapshot: &Snapshot,
        base: Option<&Snapshot>,
        request: &SnapshotRequestChanges,
    ) -> Result<CreateSnapshotResponse, String> {
        let operation = self.create_snapshot_operation_response(snapshot, request)?;
        let loaded = |project: &&Arc<Project>| project.command_line().is_some();
        let Some(base) = base else {
            return Ok(CreateSnapshotResponse {
                snapshot: snapshot.id(),
                projects: snapshot
                    .projects()
                    .filter(loaded)
                    .map(|project| project_response(snapshot, project))
                    .collect(),
                changes: None,
                operation,
            });
        };
        let mut projects = snapshot
            .projects()
            .filter(|project| base.project(project.id()).is_none())
            .filter(loaded)
            .map(|project| project_response(snapshot, project))
            .collect::<Vec<_>>();
        for old in base.projects() {
            if let Some(new) = snapshot.project(old.id()) {
                if !Arc::ptr_eq(old, new) && new.command_line().is_some() {
                    projects.push(project_response(snapshot, new));
                }
            }
        }
        Ok(CreateSnapshotResponse {
            snapshot: snapshot.id(),
            projects,
            changes: Some(self.compute_snapshot_changes(base, snapshot)),
            operation,
        })
    }

    /// tsgo `createSnapshotOperationResponse`: the programs the request
    /// created and the default project of each file it opened, when it asked
    /// for them.
    fn create_snapshot_operation_response(
        &self,
        snapshot: &Snapshot,
        request: &SnapshotRequestChanges,
    ) -> Result<SnapshotOperationResponse, String> {
        let mut operation = SnapshotOperationResponse::default();
        if let Some(create_programs) = &request.create_programs {
            let created = snapshot.created_programs();
            if created.len() != create_programs.len() {
                return Err("created program result count does not match request".to_owned());
            }
            operation.created_programs = Some(created.iter().map(ToString::to_string).collect());
        }
        if let Some(open_files) = &request.open_files {
            let mut results = Vec::with_capacity(open_files.len());
            for file in open_files {
                let file_name = self.absolute_file_name(file)?;
                let project = snapshot
                    .default_project(&file_name)
                    .ok_or_else(|| format!("no project found for opened file {file_name}"))?;
                results.push(OpenedFileOperationResult {
                    project: project.id().to_string(),
                });
            }
            operation.opened_files = Some(results);
        }
        Ok(operation)
    }

    /// tsgo `computeSnapshotChanges`: the base's projects that are gone, and
    /// for each replaced project with another program, the files its program
    /// no longer has and the files whose parse changed.
    fn compute_snapshot_changes(&self, base: &Snapshot, snapshot: &Snapshot) -> SnapshotChanges {
        let mut changes = SnapshotChanges::default();
        for old in base.projects() {
            let Some(new) = snapshot.project(old.id()) else {
                changes.removed_projects.push(old.id().to_string());
                continue;
            };
            let same_program = match (old.program(), new.program()) {
                (Some(old), Some(new)) => Arc::ptr_eq(old, new),
                (None, None) => true,
                _ => false,
            };
            if Arc::ptr_eq(old, new) || same_program {
                continue;
            }
            let old_files = old
                .program()
                .map(|program| self.program_documents(program))
                .unwrap_or_default();
            let new_files = new
                .program()
                .map(|program| self.program_documents(program))
                .unwrap_or_default();
            let mut project_changes = ProjectFileChanges::default();
            for (path, document) in &old_files {
                match new_files.get(path) {
                    None => project_changes.deleted_files.push(path.clone()),
                    Some(new_document) if !Arc::ptr_eq(document, new_document) => {
                        project_changes.changed_files.push(path.clone());
                    }
                    Some(_) => {}
                }
            }
            if !project_changes.changed_files.is_empty()
                || !project_changes.deleted_files.is_empty()
            {
                changes
                    .changed_projects
                    .insert(new.id().to_string(), project_changes);
            }
        }
        changes
    }

    /// tsgo `FilesByPath`: each file of the program, libraries included,
    /// with its parse.
    fn program_documents(&self, program: &ProjectProgram) -> BTreeMap<String, Arc<BoundDocument>> {
        program.with_live(|live| {
            (0..live.file_count())
                .filter_map(|index| {
                    let file_name = live.file_name(index)?.to_string_lossy().into_owned();
                    let document = Arc::clone(live.document(index)?);
                    Some((self.host.to_path(&file_name), document))
                })
                .collect()
        })
    }

    fn lock_snapshots(&self) -> MutexGuard<'_, BTreeMap<SnapshotId, SnapshotData>> {
        self.snapshots
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_batch_pages(&self) -> MutexGuard<'_, BTreeMap<String, Vec<String>>> {
        self.batch_pages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Handler for Session {
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String> {
        self.handle(method, params)
    }

    /// tsgo's session ignores notifications.
    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// tsgo `unmarshallerFor`: the parameters (their zero value for `null`), or
/// the request's error.
fn parse<T: DeserializeOwned + Default>(type_name: &str, params: &[u8]) -> Result<T, String> {
    serde_json::from_slice::<Option<T>>(params)
        .map(Option::unwrap_or_default)
        .map_err(|error| invalid_request(format!("failed to unmarshal *api.{type_name}: {error}")))
}

/// JSON text without the whitespace between its tokens.
fn compact_json(text: &str) -> String {
    let mut compact = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in text.chars() {
        if in_string {
            compact.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if !matches!(character, ' ' | '\t' | '\n' | '\r') {
            in_string = character == '"';
            compact.push(character);
        }
    }
    compact
}

/// A request's file system (tsgo `requestfilesystem`) is not ported yet.
fn reject_file_system(params: &CreateSnapshotParams) -> Result<(), String> {
    match &params.file_system {
        Some(_) => Err(client_error("a request's file system is not supported yet")),
        None => Ok(()),
    }
}

/// tsgo `isSourceFileResponseMethod`.
fn is_source_file_response_method(method: &str) -> bool {
    matches!(
        method,
        "createSourceFile"
            | "createSourceFileFromFile"
            | "getSourceFile"
            | "getConfigSourceFile"
            | "typeToTypeNode"
            | "signatureToSignatureDeclaration"
    )
}

/// tsgo `NewProjectResponse`.
fn project_response(snapshot: &Snapshot, project: &Project) -> ProjectResponse {
    let command_line = project
        .command_line()
        .expect("a project in a response has its command line");
    let text_of = |file_name: &str| {
        snapshot
            .read_file(file_name)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    };
    let (options, type_acquisition, compile_on_save, raw, errors) = match command_line {
        CommandLine::Config(plan) => (
            compiler_options_json(plan),
            Some(struct_json(
                TYPE_ACQUISITION_FIELDS,
                plan.type_acquisition_option_bag(),
            )),
            // tsgo sets it for every config: the raw value when it is a
            // boolean, else false.
            Some(matches!(
                plan.compile_on_save(),
                Some(JsonValue::Bool(true))
            )),
            Some(GoJson::from_json(plan.raw())),
            plan.errors(),
        ),
        CommandLine::Roots(roots) => (
            struct_json(COMPILER_OPTIONS_FIELDS, &roots.options),
            None,
            None,
            None,
            roots.config_file_parsing_diagnostics.as_slice(),
        ),
    };
    let options = raw_json(options.compact());
    let file_names = command_line.file_names();
    ProjectResponse {
        id: project.id().to_string(),
        config_file_name: match project.kind() {
            ProjectKind::Configured => project.config_file_name().unwrap_or_default().to_owned(),
            ProjectKind::Inferred | ProjectKind::Synthetic => String::new(),
        },
        current_directory: project.current_directory().to_owned(),
        dirty: project.is_dirty(),
        parsed_command_line: ConfigFileResponse {
            file_names: file_names.clone(),
            options: options.clone(),
            project_references: command_line
                .project_references()
                .iter()
                .map(|reference| ProjectReference {
                    path: reference.path.to_string_lossy().into_owned(),
                    original_path: reference.original_path.to_string_lossy().into_owned(),
                    circular: reference.circular == Some(true),
                })
                .collect(),
            type_acquisition: type_acquisition.map(|value| raw_json(value.compact())),
            compile_on_save,
            raw: raw.map(|value| raw_json(value.compact())),
            errors: errors
                .iter()
                .map(|diagnostic| DiagnosticResponse::new(diagnostic, &text_of))
                .collect(),
        },
        root_files: file_names,
        compiler_options: options,
    }
}

/// tsgo's API methods (the `Method` constants of api/proto.go), sorted: a
/// method among them that is not ported yet is not implemented, any other
/// is unknown.
const TSGO_METHODS: &[&str] = &[
    "batchRequests",
    "build",
    "buildReferences",
    "cleanBuild",
    "cleanReferences",
    "createBuildOrchestrator",
    "createModuleResolver",
    "createSnapshot",
    "createSourceFile",
    "createSourceFileFromFile",
    "disposeBuildOrchestrator",
    "emit",
    "emitToString",
    "formatNodeForInsertion",
    "getAliasSymbolOfType",
    "getAliasTypeArgumentsOfType",
    "getAliasedSymbol",
    "getAnyType",
    "getApparentPropertiesOfType",
    "getApparentType",
    "getAwaitedType",
    "getBaseConstraintOfType",
    "getBaseTypeOfLiteralType",
    "getBaseTypeOfType",
    "getBaseTypes",
    "getBigIntType",
    "getBindDiagnostics",
    "getBooleanType",
    "getCheckTypeOfType",
    "getCompletionsAtPosition",
    "getConfigFileNames",
    "getConfigFileParsingDiagnostics",
    "getConfigSourceFile",
    "getConstantValue",
    "getConstraintOfType",
    "getConstraintOfTypeParameter",
    "getConstraintTypeOfMappedType",
    "getContextualType",
    "getContextualTypeForArgument",
    "getCurrentLanguageServerSnapshot",
    "getDeclarationDiagnostics",
    "getDeclarationEmit",
    "getDeclaredTypeOfSymbol",
    "getDefaultFromTypeParameter",
    "getDefaultProjectForFile",
    "getDocumentationComment",
    "getESSymbolType",
    "getExportSpecifierLocalTargetSymbol",
    "getExportSymbolOfSymbol",
    "getExportSymbolOfSymbolForChecker",
    "getExportsOfModule",
    "getExportsOfSymbol",
    "getExtendsTypeOfType",
    "getFalseTypeOfConditionalType",
    "getFreshTypeOfType",
    "getFullyQualifiedName",
    "getGlobalDiagnostics",
    "getImmediateAliasedSymbol",
    "getImportAdderEdits",
    "getIndexInfoOfType",
    "getIndexInfosOfType",
    "getIndexTypeOfType",
    "getJavaScriptEmit",
    "getJsDocTags",
    "getLocalTypeParametersOfType",
    "getMemberInModuleExports",
    "getMembersOfSymbol",
    "getModeForResolutionAtIndex",
    "getModeForUsageLocation",
    "getNameTypeOfMappedType",
    "getNeverType",
    "getNonMissingTypeOfSymbol",
    "getNonNullableType",
    "getNonPrimitiveType",
    "getNullType",
    "getNumberType",
    "getObjectTypeOfType",
    "getOuterTypeParametersOfType",
    "getParameterType",
    "getParametersOfSignature",
    "getParentOfSymbol",
    "getProgramDiagnostics",
    "getPropertiesOfType",
    "getPropertyOfType",
    "getReducedType",
    "getReferencedSymbolsForNode",
    "getReferencesToSymbolInFile",
    "getRegularTypeOfType",
    "getResolvedModule",
    "getResolvedModuleFromModuleSpecifier",
    "getResolvedSignature",
    "getResolvedTypeReferenceDirective",
    "getResolvedTypeReferenceDirectiveFromTypeReferenceDirective",
    "getRestTypeOfSignature",
    "getReturnTypeOfSignature",
    "getSemanticDiagnostics",
    "getShorthandAssignmentValueSymbol",
    "getSignatureFromDeclaration",
    "getSignatureUsages",
    "getSignaturesOfType",
    "getSourceFile",
    "getSourceFileMetadata",
    "getSourceFileNames",
    "getStringType",
    "getSuggestionDiagnostics",
    "getSymbolAtLocation",
    "getSymbolAtPosition",
    "getSymbolOfSourceFile",
    "getSymbolOfType",
    "getSymbolsAtLocations",
    "getSymbolsAtPositions",
    "getSymbolsInScope",
    "getSymbolsOfSourceFiles",
    "getSyntacticDiagnostics",
    "getTargetOfSignature",
    "getTargetOfType",
    "getTargetSymbol",
    "getTemplateTypeOfMappedType",
    "getThisParameterOfSignature",
    "getThisTypeOfType",
    "getTrueTypeOfConditionalType",
    "getTypeArguments",
    "getTypeAtLocation",
    "getTypeAtLocations",
    "getTypeAtPosition",
    "getTypeFromTypeNode",
    "getTypeOfPropertyOfType",
    "getTypeOfSymbol",
    "getTypeOfSymbolAtLocation",
    "getTypeParameterAtPosition",
    "getTypeParameterOfMappedType",
    "getTypeParametersOfSignature",
    "getTypeParametersOfType",
    "getTypePredicateOfSignature",
    "getTypesAtPositions",
    "getTypesOfSymbols",
    "getTypesOfType",
    "getUndefinedType",
    "getUnknownType",
    "getVoidType",
    "getWellKnownSignatures",
    "getWellKnownSymbols",
    "getWidenedType",
    "initialize",
    "isArrayLikeType",
    "isArrayType",
    "isContextSensitive",
    "isReadonlySymbol",
    "isTypeAssignableTo",
    "parseCommandLine",
    "parseConfigFile",
    "parseJsonConfigFileContent",
    "printNode",
    "readConfigFile",
    "release",
    "releaseModuleResolver",
    "releaseSourceFile",
    "resolveModuleName",
    "resolveName",
    "saveHeapProfile",
    "signatureToSignatureDeclaration",
    "startCPUProfile",
    "stopCPUProfile",
    "transpileDeclaration",
    "transpileDeclarationFromFile",
    "transpileModule",
    "transpileModuleFromFile",
    "typeToString",
    "typeToTypeNode",
    "updateSnapshot",
];

#[cfg(test)]
#[path = "../tests/unit/session.rs"]
mod tests;
