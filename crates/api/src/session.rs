//! tsgo `api/session.go` (19dadef8): the session that answers an API
//! client's requests over snapshots of tsgo's project system
//! ([`tsc_project`]). The methods ported so far are `echo`, `ping`,
//! `initialize`, `batchRequests`, `createSnapshot`, `updateSnapshot`,
//! `release`, `getDefaultProjectForFile`, the command line and config
//! requests (`parseCommandLine`, `readConfigFile`,
//! `parseJsonConfigFileContent`, `parseConfigFile`), the source files of a
//! client (`createSourceFile`, `createSourceFileFromFile`,
//! `releaseSourceFile`) and the transpile requests, and
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
use tsc_compiler::transpile::{transpile_declaration, transpile_module};
use tsc_compiler::BoundDocument;
use tsc_diagnostics::{gen, Diagnostic, JsStr, JsString, MessageChain};
use tsc_host::vfs::{FileSystem, VfsCompilerHost};
use tsc_program::go_json::{
    compiler_options_json, struct_json, GoJson, COMPILER_OPTIONS_FIELDS, TYPE_ACQUISITION_FIELDS,
};
use tsc_program::{
    command_line_compiler_options, command_line_program_inputs, command_line_raw,
    parse_command_line, parse_config_file_text_to_json, parse_config_root_plan, CompilerConfigHost,
    ConfigProjectReference, ConfigRootPlan, ConfigRootPlanRequest, JsonValue,
};
use tsc_project::{
    ApiSnapshotRequest, CommandLine, CreateProgramRequest, FileChangeSummary, Project, ProjectId,
    ProjectKind, ProjectProgram, ReconfigureProgramRequest, SessionOptions, Snapshot, SnapshotHost,
};

use crate::encoder::{
    encode_source_file, ScriptKind, SourceFileFacts, HEADER_OFFSET_SOURCE_FILE_LEASE,
};
use crate::ipc::{panic_message, Handler, Payload};
use crate::proto::{
    BatchRequest, BatchRequestsParams, ConfigFileParams, ConfigFileResponse, CreateProgramOptions,
    CreateSnapshotParams, CreateSnapshotResponse, CreateSourceFileFromFileParams,
    CreateSourceFileOptions, CreateSourceFileParams, DiagnosticResponse, DocumentIdentifier,
    EnsurePrograms, FileNotifications, GetDefaultProjectForFileParams, InitializeResponse,
    OpenedFileOperationResult, ParseCommandLineParams, ParseJsonConfigFileContentParams,
    ProjectFileChanges, ProjectReference, ProjectResponse, ReadConfigFileResponse, ReleaseParams,
    ReleaseSourceFileParams, SnapshotChanges, SnapshotId, SnapshotOperationResponse,
    SnapshotRequestChanges, SourceFileResponse, TranspileFromFileParams, TranspileOptions,
    TranspileOutputResponse, TranspileParams, UpdateSnapshotParams,
};
use crate::references::collect_external_module_references;
use crate::request_fs::{RequestFileSystem, SnapshotFileSystem};

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
/// times it was handed out and the request file system it reads, which its
/// updates read too.
struct SnapshotData {
    snapshot: Arc<Snapshot>,
    ref_count: usize,
    open: OpenState,
    file_system: Option<Arc<RequestFileSystem>>,
}

/// tsgo `api.Session`.
pub struct Session {
    id: String,
    host: SnapshotHost,
    /// The session's file system (tsgo `FS()`), under a request's.
    fs: Arc<dyn FileSystem>,
    /// MessagePack sends binary responses as they are (tsgo
    /// `UseBinaryResponses`).
    use_binary_responses: bool,
    snapshots: Mutex<BTreeMap<SnapshotId, SnapshotData>>,
    /// The responses of a batch that did not fit its first page, by
    /// continuation token.
    batch_pages: Mutex<BTreeMap<String, Vec<String>>>,
    next_batch_page: AtomicU64,
    /// The source files the client holds (tsgo `sourceFileLeases`), by
    /// lease.
    source_file_leases: Mutex<BTreeSet<u64>>,
    next_source_file_lease: AtomicU64,
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
            host: SnapshotHost::new(options, Arc::clone(&fs)),
            fs,
            use_binary_responses,
            snapshots: Mutex::default(),
            batch_pages: Mutex::default(),
            next_batch_page: AtomicU64::new(0),
            source_file_leases: Mutex::default(),
            next_source_file_lease: AtomicU64::new(0),
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
            "parseCommandLine" => {
                let params = parse::<ParseCommandLineParams>("ParseCommandLineParams", params)?;
                Ok(json(&self.handle_parse_command_line(&params)))
            }
            "readConfigFile" => {
                let params = parse::<ConfigFileParams>("ReadConfigFileParams", params)?;
                self.handle_read_config_file(&params)
                    .map(|response| json(&response))
            }
            "parseJsonConfigFileContent" => {
                let params = parse::<ParseJsonConfigFileContentParams>(
                    "ParseJsonConfigFileContentParams",
                    params,
                )?;
                self.handle_parse_json_config_file_content(&params)
                    .map(|response| json(&response))
            }
            "parseConfigFile" => {
                let params = parse::<ConfigFileParams>("ParseConfigFileParams", params)?;
                self.handle_parse_config_file(&params)
                    .map(|response| json(&response))
            }
            "createSourceFile" => {
                let params = parse::<CreateSourceFileParams>("CreateSourceFileParams", params)?;
                self.create_source_file(&params.file_name, &params.source_text, params.options)
            }
            "createSourceFileFromFile" => {
                let params = parse::<CreateSourceFileFromFileParams>(
                    "CreateSourceFileFromFileParams",
                    params,
                )?;
                let file_name = self.host.absolute_file_name(&params.file_name);
                let text = self
                    .read_file_text(&file_name)
                    .ok_or_else(|| client_error(format!("could not read file {file_name:?}")))?;
                self.create_source_file(&file_name, &text, params.options)
            }
            "releaseSourceFile" => {
                let params = parse::<ReleaseSourceFileParams>("ReleaseSourceFileParams", params)?;
                self.handle_release_source_file(&params)
                    .map(|()| json(&true))
            }
            "transpileModule" | "transpileDeclaration" => {
                let params = parse::<TranspileParams>("TranspileParams", params)?;
                self.transpile(
                    &params.input,
                    &params.options,
                    method == "transpileDeclaration",
                )
                .map(|response| json(&response))
            }
            "transpileModuleFromFile" | "transpileDeclarationFromFile" => {
                let params = parse::<TranspileFromFileParams>("TranspileFromFileParams", params)?;
                let file_name = self.host.absolute_file_name(&params.file_name);
                let input = self
                    .read_file_text(&file_name)
                    .ok_or_else(|| client_error(format!("could not read file {file_name:?}")))?;
                let options = TranspileOptions {
                    file_name,
                    ..params.options
                };
                self.transpile(&input, &options, method == "transpileDeclarationFromFile")
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

    /// tsgo `snapshotHost.FS().ReadFile`: a file of the session's file
    /// system, decoded.
    fn read_file_text(&self, file_name: &str) -> Option<String> {
        self.fs
            .read(file_name)
            .ok()
            .and_then(|bytes| tsc_program::decode_host_text(bytes).ok())
    }

    /// A host over the session's file system, for config parsing (tsgo
    /// passes the snapshot host as the `ParseConfigHost`).
    fn config_host(&self) -> VfsCompilerHost<&dyn FileSystem> {
        VfsCompilerHost::new(&*self.fs, self.current_directory())
    }

    /// tsgo `handleParseCommandLine`: `tsoptions.ParseCommandLine` over the
    /// session's file system (for response files).
    fn handle_parse_command_line(&self, params: &ParseCommandLineParams) -> ConfigFileResponse {
        let current_directory = JsStr::from_str(self.current_directory());
        let read_file = |name: JsStr<'_>| self.read_file_text(&name.to_string_lossy());
        let parsed = parse_command_line(
            &params.command_line,
            current_directory,
            self.host.case_sensitive(),
            &read_file,
        );
        let options = command_line_compiler_options(&parsed.options, current_directory);
        let raw = GoJson::from_json(&command_line_raw(&parsed.options));
        ConfigFileResponse {
            file_names: parsed.file_names,
            options: raw_json(struct_json(COMPILER_OPTIONS_FIELDS, &options).compact()),
            project_references: Vec::new(),
            type_acquisition: None,
            compile_on_save: None,
            raw: Some(raw_json(raw.compact())),
            errors: parsed
                .errors
                .iter()
                .map(|diagnostic| DiagnosticResponse::new(diagnostic, &|_| None))
                .collect(),
        }
    }

    /// tsgo `handleReadConfigFile`: the config's JSON and its first error.
    fn handle_read_config_file(
        &self,
        params: &ConfigFileParams,
    ) -> Result<ReadConfigFileResponse, String> {
        let file_name = self.absolute_file_name(&params.file)?;
        let Some(text) = self.read_file_text(&file_name) else {
            return Ok(ReadConfigFileResponse {
                config: raw_json("{}".to_owned()),
                error: Some(DiagnosticResponse::new(
                    &Diagnostic::new(
                        None,
                        None,
                        None,
                        MessageChain::new(&gen::Cannot_read_file_0, &[file_name]),
                    ),
                    &|_| None,
                )),
            });
        };
        let (config, errors) = parse_config_file_text_to_json(file_name.as_str(), text.as_str())
            .map_err(|error| error.to_string())?;
        let text_of = |name: &str| (name == file_name).then(|| text.clone());
        Ok(ReadConfigFileResponse {
            config: raw_json(GoJson::from_json(&config).compact()),
            error: errors
                .first()
                .map(|error| DiagnosticResponse::new(error, &text_of)),
        })
    }

    /// tsgo `handleParseJsonConfigFileContent`: a config's JSON value,
    /// parsed in the config directory or as the named config. tsgo parses
    /// the value itself; tsc-rs parses its JSON text, so its errors lose
    /// the locations the value does not have.
    fn handle_parse_json_config_file_content(
        &self,
        params: &ParseJsonConfigFileContentParams,
    ) -> Result<ConfigFileResponse, String> {
        let (file_name, base_path) = match (&params.config_directory, &params.config_file_name) {
            (Some(directory), None) => (String::new(), self.host.absolute_file_name(directory)),
            (None, Some(file)) => (
                self.absolute_file_name(file)?,
                self.current_directory().to_owned(),
            ),
            _ => {
                return Err(client_error(
                    "exactly one of configDirectory or configFileName is required",
                ))
            }
        };
        // tsgo takes a value that is not an object as an empty one.
        let text = match &params.json {
            serde_json::Value::Object(_) => params.json.to_string(),
            _ => "{}".to_owned(),
        };
        let host = self.config_host();
        let plan = parse_config_root_plan(
            &CompilerConfigHost::new(&host),
            ConfigRootPlanRequest {
                file_name: JsString::from(file_name.as_str()),
                text,
                base_path: JsString::from(base_path.as_str()),
            },
        )
        .map_err(|error| error.to_string())?;
        let mut response = config_file_response(&plan, &|_| None);
        response.errors = plan
            .errors()
            .iter()
            .map(|error| {
                let mut error = error.clone();
                error.file_name = None;
                error.start = None;
                error.length = None;
                error.related.clear();
                DiagnosticResponse::new(&error, &|_| None)
            })
            .collect();
        Ok(response)
    }

    /// tsgo `handleParseConfigFile`: the config file's parse.
    fn handle_parse_config_file(
        &self,
        params: &ConfigFileParams,
    ) -> Result<ConfigFileResponse, String> {
        let file_name = self.absolute_file_name(&params.file)?;
        let text = self
            .read_file_text(&file_name)
            .ok_or_else(|| client_error(format!("could not read file {file_name:?}")))?;
        let host = self.config_host();
        let plan = parse_config_root_plan(
            &CompilerConfigHost::new(&host),
            ConfigRootPlanRequest {
                file_name: JsString::from(file_name.as_str()),
                text,
                base_path: JsString::from(self.current_directory()),
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(config_file_response(&plan, &|name| {
            self.read_file_text(name)
        }))
    }

    /// tsgo `createSourceFile`: the file parsed and bound as tsgo's parse
    /// cache does, encoded with a new lease.
    fn create_source_file(
        &self,
        file_name: &str,
        source_text: &str,
        options: CreateSourceFileOptions,
    ) -> Result<Payload, String> {
        // tsgo `EnsureScriptKindFromFileName`: a name without a known
        // extension is TypeScript.
        let script_kind = match options.script_kind {
            0 => match ScriptKind::from_file_name(file_name) {
                ScriptKind::Unknown => ScriptKind::Ts,
                kind => kind,
            },
            kind => ScriptKind::from_number(kind)
                .ok_or_else(|| client_error(format!("invalid scriptKind {kind}")))?,
        };
        let file_name = self.host.absolute_file_name(file_name);
        let file = crate::parse_source_file(&file_name, source_text, script_kind);
        let options = tsc_types::CompilerOptions::default();
        let bind_data = tsc_binder::bind_source_file(&file, &options).into_bind_data();
        let references = collect_external_module_references(&file);
        let path = self.host.to_path(&file_name);
        let (mut data, _) = encode_source_file(
            &file,
            &SourceFileFacts {
                path: Some(&path),
                script_kind,
                imports: &references.imports,
                module_augmentations: &references.module_augmentations,
                ambient_module_names: &references.ambient_module_names,
                bind_data: Some(&bind_data),
                ..SourceFileFacts::default()
            },
        );
        let lease = self.next_source_file_lease.fetch_add(1, Ordering::Relaxed) + 1;
        data[HEADER_OFFSET_SOURCE_FILE_LEASE..HEADER_OFFSET_SOURCE_FILE_LEASE + 8]
            .copy_from_slice(&lease.to_le_bytes());
        self.source_file_leases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(lease);
        Ok(if self.use_binary_responses {
            Payload::Binary(data)
        } else {
            json(&SourceFileResponse {
                data: base64::engine::general_purpose::STANDARD.encode(data),
            })
        })
    }

    /// tsgo `handleReleaseSourceFile`.
    fn handle_release_source_file(&self, params: &ReleaseSourceFileParams) -> Result<(), String> {
        if params.lease == 0 {
            return Err(client_error("empty source file lease"));
        }
        if !self
            .source_file_leases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&params.lease)
        {
            return Err(client_error(format!(
                "source file lease {} not found",
                params.lease
            )));
        }
        Ok(())
    }

    /// tsgo `transpileOutput`: `transpile.TranspileModule` or
    /// `TranspileDeclaration` of `input`.
    fn transpile(
        &self,
        input: &str,
        options: &TranspileOptions,
        declaration: bool,
    ) -> Result<TranspileOutputResponse, String> {
        let (compiler_options, _) = command_line_program_inputs(
            &options.compiler_options.0,
            JsStr::from_str(self.current_directory()),
            self.host.case_sensitive(),
        )
        .map_err(|error| format!("{error:?}"))?;
        let options = tsc_compiler::transpile::TranspileOptions {
            compiler_options: Some(compiler_options),
            file_name: (!options.file_name.is_empty()).then(|| options.file_name.clone()),
            report_diagnostics: options.report_diagnostics,
        };
        let output = if declaration {
            transpile_declaration(input, &options)
        } else {
            transpile_module(input, &options)
        }
        .map_err(|error| error.to_string())?;
        let text_of = |_: &str| Some(input.to_owned());
        Ok(TranspileOutputResponse {
            output_text: output.output_text,
            diagnostics: output
                .diagnostics
                .iter()
                .map(|diagnostic| DiagnosticResponse::new(diagnostic, &text_of))
                .collect(),
            source_map_text: output.source_map_text.unwrap_or_default(),
        })
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
        let mut request = self.to_api_snapshot_request(&params.changes)?;
        let open = self.reconcile_snapshot_opens(&mut request, OpenState::default());
        let mut file_changes = self.to_file_change_summary(params.file_notifications.as_ref())?;
        let mut file_system = None;
        if let Some(supplied) = &params.file_system {
            file_system = Some(
                RequestFileSystem::new_for_update(
                    supplied,
                    &SnapshotFileSystem::Host(Arc::clone(&self.fs)),
                    self.current_directory(),
                    &mut file_changes,
                )
                .map_err(client_error)?,
            );
            request.replace_file_system = supplied.is_full();
        }
        let file_changes = read_through(&mut request, file_changes, file_system.as_ref());
        let root = self.host.new_root_snapshot();
        let snapshot = self
            .host
            .clone_snapshot(&root, file_changes, Some(&request))
            .map_err(|error| client_error(format!("failed to create snapshot: {error}")))?;
        let response = self.create_snapshot_response(&snapshot, None, &params.changes)?;
        self.register_snapshot(snapshot, open, file_system);
        Ok(response)
    }

    /// tsgo `handleUpdateSnapshot`: a snapshot made from one the client
    /// holds.
    fn handle_update_snapshot(
        &self,
        params: UpdateSnapshotParams,
    ) -> Result<CreateSnapshotResponse, String> {
        let (base, base_open, mut file_system) = {
            let snapshots = self.lock_snapshots();
            let data = snapshots
                .get(&params.snapshot)
                .ok_or_else(|| client_error(format!("snapshot {} not found", params.snapshot)))?;
            (
                Arc::clone(&data.snapshot),
                data.open.clone(),
                data.file_system.clone(),
            )
        };
        let changes = params.changes.unwrap_or_default();
        let mut request = self.to_api_snapshot_request(&changes.changes)?;
        let open = self.reconcile_snapshot_opens(&mut request, base_open);
        let mut file_changes = self.to_file_change_summary(changes.file_notifications.as_ref())?;
        if let Some(supplied) = &changes.file_system {
            let base_file_system = match &file_system {
                Some(request_fs) => SnapshotFileSystem::Request(Arc::clone(request_fs)),
                None => SnapshotFileSystem::Host(Arc::clone(&self.fs)),
            };
            file_system = Some(
                RequestFileSystem::new_for_update(
                    supplied,
                    &base_file_system,
                    self.current_directory(),
                    &mut file_changes,
                )
                .map_err(client_error)?,
            );
        }
        request.replace_file_system = changes
            .file_system
            .as_ref()
            .is_some_and(|supplied| supplied.is_full());
        let file_changes = read_through(&mut request, file_changes, file_system.as_ref());
        let snapshot = self
            .host
            .clone_snapshot(&base, file_changes, Some(&request))
            .map_err(|error| client_error(format!("failed to update snapshot: {error}")))?;
        let response = self.create_snapshot_response(&snapshot, Some(&base), &changes.changes)?;
        self.register_snapshot(snapshot, open, file_system);
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
    fn register_snapshot(
        &self,
        snapshot: Snapshot,
        open: OpenState,
        file_system: Option<Arc<RequestFileSystem>>,
    ) {
        self.lock_snapshots().insert(
            snapshot.id(),
            SnapshotData {
                snapshot: Arc::new(snapshot),
                ref_count: 1,
                open,
                file_system,
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

/// A snapshot read through a request's file system: the request carries
/// it, and the file changes reach the files' other names through its links
/// (tsgo's snapshot expands them with `ExpandFileChanges`).
fn read_through(
    request: &mut ApiSnapshotRequest,
    file_changes: FileChangeSummary,
    file_system: Option<&Arc<RequestFileSystem>>,
) -> FileChangeSummary {
    match file_system {
        Some(file_system) => {
            request.file_system = Some(Arc::clone(file_system) as Arc<dyn FileSystem>);
            file_system.expand_file_changes(file_changes)
        }
        None => file_changes,
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

/// tsgo `NewConfigFileResponse` of a config's parse.
fn config_file_response(
    plan: &ConfigRootPlan,
    text_of: &dyn Fn(&str) -> Option<String>,
) -> ConfigFileResponse {
    ConfigFileResponse {
        file_names: plan
            .file_names()
            .iter()
            .map(|file| file.to_string_lossy().into_owned())
            .collect(),
        options: raw_json(compiler_options_json(plan).compact()),
        project_references: plan
            .project_references()
            .unwrap_or_default()
            .iter()
            .map(project_reference)
            .collect(),
        type_acquisition: Some(raw_json(
            struct_json(TYPE_ACQUISITION_FIELDS, plan.type_acquisition_option_bag()).compact(),
        )),
        // tsgo sets it for every config: the raw value when it is a
        // boolean, else false.
        compile_on_save: Some(matches!(
            plan.compile_on_save(),
            Some(JsonValue::Bool(true))
        )),
        raw: Some(raw_json(GoJson::from_json(plan.raw()).compact())),
        errors: plan
            .errors()
            .iter()
            .map(|diagnostic| DiagnosticResponse::new(diagnostic, text_of))
            .collect(),
    }
}

fn project_reference(reference: &ConfigProjectReference) -> ProjectReference {
    ProjectReference {
        path: reference.path.to_string_lossy().into_owned(),
        original_path: reference.original_path.to_string_lossy().into_owned(),
        circular: reference.circular == Some(true),
    }
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
    let parsed_command_line = match command_line {
        CommandLine::Config(plan) => config_file_response(plan, &text_of),
        CommandLine::Roots(roots) => ConfigFileResponse {
            file_names: roots.root_file_names.clone(),
            options: raw_json(struct_json(COMPILER_OPTIONS_FIELDS, &roots.options).compact()),
            project_references: roots
                .project_references
                .iter()
                .map(project_reference)
                .collect(),
            type_acquisition: None,
            compile_on_save: None,
            raw: None,
            errors: roots
                .config_file_parsing_diagnostics
                .iter()
                .map(|diagnostic| DiagnosticResponse::new(diagnostic, &text_of))
                .collect(),
        },
    };
    ProjectResponse {
        id: project.id().to_string(),
        config_file_name: match project.kind() {
            ProjectKind::Configured => project.config_file_name().unwrap_or_default().to_owned(),
            ProjectKind::Inferred | ProjectKind::Synthetic => String::new(),
        },
        current_directory: project.current_directory().to_owned(),
        dirty: project.is_dirty(),
        root_files: parsed_command_line.file_names.clone(),
        compiler_options: parsed_command_line.options.clone(),
        parsed_command_line,
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
