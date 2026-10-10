//! tsgo `api/session.go` (19dadef8): the session that answers an API
//! client's requests over snapshots of tsgo's project system
//! ([`tsc_project`]). The methods ported so far are `echo`, `ping`,
//! `initialize`, `batchRequests`, `createSnapshot`, `updateSnapshot`,
//! `release`, `getDefaultProjectForFile`, the command line and config
//! requests (`parseCommandLine`, `readConfigFile`,
//! `parseJsonConfigFileContent`, `parseConfigFile`), the source files of a
//! client (`createSourceFile`, `createSourceFileFromFile`,
//! `releaseSourceFile`), the transpile requests, the program's information
//! (`getSourceFile`, the resolved modules and their modes, the config
//! files), the module resolvers and, in `crate::checker`, the symbol and
//! type queries of the checker; `getCurrentLanguageServerSnapshot` answers
//! as tsgo's standalone session does, and tsgo's other methods answer that
//! they are not implemented yet.

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
use tsc_program::{
    CanonicalPath, ModuleResolution, PackageId, PackageJsonType, ResolutionKey, ResolutionMode,
    ResolutionOutcome, TypeReferenceResolution, TypeReferenceResolutionKey,
};
use tsc_project::{
    ApiSnapshotRequest, CommandLine, CreateProgramRequest, FileChangeSummary, Project, ProjectId,
    ProjectKind, ProjectProgram, ReconfigureProgramRequest, SessionOptions, Snapshot, SnapshotHost,
};
use tsc_syntax::{NodeData, NodeId, SyntaxKind};

use crate::checker::Registry;
use crate::encoder::{
    build_node_index_table, encode_source_file, ScriptKind, SourceFileFacts,
    HEADER_OFFSET_SOURCE_FILE_LEASE,
};
use crate::ipc::{panic_message, Conn, Handler, Payload};
use crate::module_resolution::{
    module_kind_name, resolution_mode as api_resolution_mode, Chain, InFlight,
    ModuleResolverRegistration, ProgramModuleResolution, ResolutionState,
    ResolveModuleNameCallbackParams, StaticResolutions,
};
use crate::proto::{
    BatchRequest, BatchRequestsParams, ConfigFileParams, ConfigFileResponse,
    CreateModuleResolverParams, CreateProgramOptions, CreateSnapshotParams, CreateSnapshotResponse,
    CreateSourceFileFromFileParams, CreateSourceFileOptions, CreateSourceFileParams,
    DiagnosticResponse, DocumentIdentifier, EnsurePrograms, FileNotifications,
    GetDefaultProjectForFileParams, GetModeForResolutionAtIndexParams,
    GetModeForUsageLocationParams, GetResolvedModuleFromModuleSpecifierParams,
    GetResolvedModuleParams, GetResolvedTypeReferenceDirectiveFromReferenceParams,
    GetResolvedTypeReferenceDirectiveParams, GetSourceFileParams, InitializeResponse,
    OpenedFileOperationResult, PackageIdResponse, ParseCommandLineParams,
    ParseJsonConfigFileContentParams, ProjectFileChanges, ProjectParams, ProjectReference,
    ProjectResponse, ReadConfigFileResponse, ReleaseModuleResolverParams, ReleaseParams,
    ReleaseSourceFileParams, ResolveModuleNameParams, ResolveModuleNameResult,
    ResolvedModuleResponse, ResolvedTypeReferenceDirectiveResponse, SnapshotChanges, SnapshotId,
    SnapshotOperationResponse, SnapshotRequestChanges, SourceFileMetadata, SourceFileResponse,
    TranspileFromFileParams, TranspileOptions, TranspileOutputResponse, TranspileParams,
    UpdateSnapshotParams,
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

pub(crate) fn client_error(message: impl std::fmt::Display) -> String {
    format!("{CLIENT_ERROR}: {message}")
}

fn invalid_request(message: impl std::fmt::Display) -> String {
    format!("{INVALID_REQUEST}: {message}")
}

pub(crate) fn json<T: Serialize>(value: &T) -> Payload {
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
    /// The symbols, types and signatures handed out in the snapshot.
    registry: Arc<Mutex<Registry>>,
}

/// A client's module resolver and the resolution its programs share.
type ModuleResolverEntry = (
    Arc<ModuleResolverRegistration>,
    tsc_program::ModuleResolutionOverrideHandle,
);

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
    /// The client's module resolvers (tsgo `moduleResolvers`), by ID, with
    /// the resolution their programs share (a program with the same
    /// resolver is the same program).
    module_resolvers: Mutex<BTreeMap<u64, ModuleResolverEntry>>,
    next_module_resolver: AtomicU64,
    /// What the resolvers' resolutions share.
    resolution: Arc<ResolutionState>,
    /// The last symbol number handed out (tsgo `ast.GetSymbolId`'s counter).
    pub(crate) next_symbol_id: AtomicU64,
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
            module_resolvers: Mutex::default(),
            next_module_resolver: AtomicU64::new(0),
            resolution: Arc::default(),
            next_symbol_id: AtomicU64::new(0),
        }
    }

    /// tsgo `ID`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The connection the client's module resolver callbacks go through
    /// (tsgo `Session.conn`).
    pub fn set_connection(&self, conn: &Arc<Conn>) {
        self.resolution.set_connection(conn);
    }

    fn current_directory(&self) -> &str {
        &self.host.options().current_directory
    }

    /// The session's snapshot host.
    pub(crate) fn host(&self) -> &SnapshotHost {
        &self.host
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
            "getSourceFile" => {
                let params = parse::<GetSourceFileParams>("GetSourceFileParams", params)?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                Ok(
                    match self.source_file(&program, &document_file_name(&params.file)?) {
                        Some(file) => self.encode_program_file(&file),
                        None => self.no_source_file(),
                    },
                )
            }
            "getSourceFileNames" => {
                let params = parse::<ProjectParams>("GetSourceFileNamesParams", params)?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                Ok(json(&program.with_live(|live| {
                    (0..live.file_count())
                        .filter_map(|index| live.file_name(index))
                        .map(|name| name.to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                })))
            }
            "getSourceFileMetadata" => {
                let params = parse::<GetSourceFileParams>("GetSourceFileParams", params)?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                Ok(json(
                    &self
                        .source_file(&program, &document_file_name(&params.file)?)
                        .and_then(|file| source_file_metadata(&program, &file)),
                ))
            }
            "getConfigFileNames" => {
                let params = parse::<ProjectParams>("GetProjectDiagnosticsParams", params)?;
                let (project, _) = self.program(params.snapshot, &params.project)?;
                // tsgo answers a program without a config with a nil slice,
                // which Go writes as `[]`.
                Ok(json(&match project.command_line() {
                    Some(CommandLine::Config(plan)) => {
                        std::iter::once(plan.config_file_name().to_owned())
                            .chain(plan.extended_source_files().iter().cloned())
                            .map(|name| name.to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                }))
            }
            "getConfigSourceFile" => {
                let params = parse::<GetSourceFileParams>("GetSourceFileParams", params)?;
                let (project, program) = self.program(params.snapshot, &params.project)?;
                self.handle_get_config_source_file(&project, &program, &params)
            }
            "getModeForUsageLocation" => {
                let params = parse::<GetModeForUsageLocationParams>(
                    "GetModeForUsageLocationParams",
                    params,
                )?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                self.required_source_file(&program, &params.file)?;
                let (_, usage) = self.resolve_node_handle(&program, &params.usage)?;
                if !is_string_literal_like(&program, usage) {
                    return Err(client_error("usage must be a StringLiteralLike node"));
                }
                Ok(json(&mode_for_usage_location(&program, usage)))
            }
            "getModeForResolutionAtIndex" => {
                let params = parse::<GetModeForResolutionAtIndexParams>(
                    "GetModeForResolutionAtIndexParams",
                    params,
                )?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                let file = self.required_source_file(&program, &params.file)?;
                let references = collect_external_module_references(file.document.source());
                let source = file.document.source();
                let usages =
                    references
                        .imports
                        .iter()
                        .chain(references.module_augmentations.iter().filter(|&&name| {
                            source.arena.node(name).kind == SyntaxKind::StringLiteral
                        }))
                        .copied()
                        .collect::<Vec<_>>();
                let usage = usize::try_from(params.index)
                    .ok()
                    .and_then(|index| usages.get(index).copied())
                    .ok_or_else(|| client_error("invalid resolution index"))?;
                Ok(json(&mode_for_usage_location(&program, usage)))
            }
            "getResolvedModule" => {
                let params = parse::<GetResolvedModuleParams>("GetResolvedModuleParams", params)?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                let file = self.required_source_file(&program, &params.file)?;
                Ok(json(&resolved_module(
                    &program,
                    &file,
                    &params.module_name,
                    params.mode,
                )))
            }
            "getResolvedModuleFromModuleSpecifier" => {
                let params = parse::<GetResolvedModuleFromModuleSpecifierParams>(
                    "GetResolvedModuleFromModuleSpecifierParams",
                    params,
                )?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                let (file, specifier) =
                    self.resolve_node_handle(&program, &params.module_specifier)?;
                if !is_string_literal_like(&program, specifier) {
                    return Err(client_error(
                        "moduleSpecifier must be a StringLiteralLike node",
                    ));
                }
                let file = match &params.source_file {
                    Some(source_file) => self.required_source_file(&program, source_file)?,
                    None => file,
                };
                let text = literal_text(file.document.source(), specifier)
                    .or_else(|| {
                        self.resolve_node_handle(&program, &params.module_specifier)
                            .ok()
                            .and_then(|(own, _)| literal_text(own.document.source(), specifier))
                    })
                    .unwrap_or_default();
                let mode = mode_for_usage_location(&program, specifier);
                Ok(json(&resolved_module(&program, &file, &text, mode)))
            }
            "getResolvedTypeReferenceDirective" => {
                let params = parse::<GetResolvedTypeReferenceDirectiveParams>(
                    "GetResolvedTypeReferenceDirectiveParams",
                    params,
                )?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                let file = self.required_source_file(&program, &params.file)?;
                Ok(json(&resolved_type_reference_directive(
                    &program,
                    &file,
                    &params.type_directive_name,
                    params.mode,
                )))
            }
            "getResolvedTypeReferenceDirectiveFromTypeReferenceDirective" => {
                let params = parse::<GetResolvedTypeReferenceDirectiveFromReferenceParams>(
                    "GetResolvedTypeReferenceDirectiveFromReferenceParams",
                    params,
                )?;
                let (_, program) = self.program(params.snapshot, &params.project)?;
                let file = self.required_source_file(&program, &params.source_file)?;
                let mode = match params.resolution_mode {
                    0 => program
                        .with_live(|live| {
                            live.with_checker(|checker| {
                                checker.api_default_resolution_mode_for_file(
                                    file.document.source().root,
                                )
                            })
                        })
                        .unwrap_or(0),
                    mode => mode,
                };
                Ok(json(&resolved_type_reference_directive(
                    &program,
                    &file,
                    &params.type_directive_name,
                    mode,
                )))
            }
            "createModuleResolver" => {
                let params =
                    parse::<CreateModuleResolverParams>("CreateModuleResolverParams", params)?;
                self.handle_create_module_resolver(&params)
                    .map(|id| json(&id))
            }
            "releaseModuleResolver" => {
                let params =
                    parse::<ReleaseModuleResolverParams>("ReleaseModuleResolverParams", params)?;
                if self
                    .lock_module_resolvers()
                    .remove(&params.resolver)
                    .is_none()
                {
                    return Err(client_error(format!(
                        "module resolver {} not found",
                        params.resolver
                    )));
                }
                Ok(json(&()))
            }
            "resolveModuleName" => {
                let params = parse::<ResolveModuleNameParams>("ResolveModuleNameParams", params)?;
                self.handle_resolve_module_name(&params)
                    .map(|result| json(&result))
            }
            // A standalone session has no language server to share.
            "getCurrentLanguageServerSnapshot" => Err(client_error(
                "getCurrentLanguageServerSnapshot requires an LSP-connected API session",
            )),
            #[cfg(test)]
            "panicForTest" => panic!("test panic"),
            _ => {
                if let Some(result) = self.handle_checker_request(method, params) {
                    return result;
                }
                if TSGO_METHODS.binary_search(&method).is_ok() {
                    Err(invalid_request(format!("{method} is not implemented yet")))
                } else {
                    Err(invalid_request(format!("unknown API method {method:?}")))
                }
            }
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
        Ok(self.source_file_payload(data))
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

    /// tsgo `getProgram`: the project of a snapshot the client holds, and
    /// its program.
    pub(crate) fn program(
        &self,
        snapshot: SnapshotId,
        project: &str,
    ) -> Result<(Arc<Project>, Arc<ProjectProgram>), String> {
        let snapshot = self.snapshot(snapshot)?;
        let found = snapshot
            .project(&ProjectId::new(project))
            .ok_or_else(|| client_error(format!("project {project} not found")))?;
        let program = found
            .program()
            .cloned()
            .ok_or_else(|| client_error("project has no program"))?;
        Ok((Arc::clone(found), program))
    }

    /// tsgo `Program.GetSourceFile`: the file named `file_name` against the
    /// program's current directory, by path.
    fn source_file(&self, program: &ProjectProgram, file_name: &str) -> Option<ProgramFile> {
        let absolute = tsc_program::get_normalized_absolute_path(
            JsStr::from_str(file_name),
            program.prepared().current_directory().display(),
        );
        self.source_file_by_path(program, &self.host.to_path(&absolute.to_string_lossy()))
    }

    /// tsgo `Program.GetSourceFileByPath`.
    fn source_file_by_path(&self, program: &ProjectProgram, path: &str) -> Option<ProgramFile> {
        program.with_live(|live| {
            (0..live.file_count()).find_map(|index| {
                let file_name = live.file_name(index)?.to_string_lossy().into_owned();
                if self.host.to_path(&file_name) != path {
                    return None;
                }
                Some(ProgramFile {
                    document: Arc::clone(live.document(index)?),
                    path: path.to_owned(),
                    file_name,
                })
            })
        })
    }

    /// tsgo `resolveOptionalSourceFile` with a file: the program's file, or
    /// the client's error.
    pub(crate) fn required_source_file(
        &self,
        program: &ProjectProgram,
        file: &DocumentIdentifier,
    ) -> Result<ProgramFile, String> {
        self.source_file(program, &document_file_name(file)?)
            .ok_or_else(|| {
                client_error(format!(
                    "source file not found: {}",
                    if file.uri.is_empty() {
                        &file.file_name
                    } else {
                        &file.uri
                    }
                ))
            })
    }

    /// tsgo `resolveNodeHandle`: the node of a handle `index.kind.path`, by
    /// its index in the encoding of the program's file at `path`.
    fn resolve_node_handle(
        &self,
        program: &ProjectProgram,
        handle: &str,
    ) -> Result<(ProgramFile, NodeId), String> {
        let invalid = || client_error(format!("invalid node handle {handle:?}"));
        let (index, rest) = handle.split_once('.').ok_or_else(invalid)?;
        let (_, path) = rest.split_once('.').ok_or_else(invalid)?;
        let index = go_parse_uint32(index)
            .map_err(|error| client_error(format!("invalid node handle {handle:?}: {error}")))?;
        let stale = || {
            client_error(format!(
                "node handle {handle:?} could not be resolved (file may not be loaded or handle may be stale)"
            ))
        };
        let file = self.source_file_by_path(program, path).ok_or_else(stale)?;
        let node = build_node_index_table(file.document.source())
            .nodes()
            .get(index as usize)
            .copied()
            .flatten()
            .ok_or_else(stale)?;
        Ok((file, node))
    }

    /// tsgo `encodeSourceFileResponse` of a program's file.
    fn encode_program_file(&self, file: &ProgramFile) -> Payload {
        let source = file.document.source();
        let references = collect_external_module_references(source);
        let (data, _) = encode_source_file(
            source,
            &SourceFileFacts {
                path: Some(&file.path),
                script_kind: script_kind_of(&file.file_name),
                imports: &references.imports,
                module_augmentations: &references.module_augmentations,
                ambient_module_names: &references.ambient_module_names,
                bind_data: Some(&file.document.data),
                ..SourceFileFacts::default()
            },
        );
        self.source_file_payload(data)
    }

    /// A source file response: MessagePack sends the encoding, JSON its
    /// base64.
    pub(crate) fn source_file_payload(&self, data: Vec<u8>) -> Payload {
        if self.use_binary_responses {
            Payload::Binary(data)
        } else {
            json(&SourceFileResponse {
                data: base64::engine::general_purpose::STANDARD.encode(data),
            })
        }
    }

    /// tsgo `encodeSourceFileResponse(nil)`: no bytes, or JSON `null`.
    fn no_source_file(&self) -> Payload {
        if self.use_binary_responses {
            Payload::Binary(Vec::new())
        } else {
            json(&())
        }
    }

    /// tsgo `handleGetConfigSourceFile`: the project's config file or one
    /// it extends, parsed as a tsconfig source.
    fn handle_get_config_source_file(
        &self,
        project: &Project,
        program: &ProjectProgram,
        params: &GetSourceFileParams,
    ) -> Result<Payload, String> {
        let Some(CommandLine::Config(plan)) = project.command_line() else {
            return Ok(self.no_source_file());
        };
        let absolute = tsc_program::get_normalized_absolute_path(
            JsStr::from_str(&document_file_name(&params.file)?),
            program.prepared().current_directory().display(),
        );
        let requested = self.host.to_path(&absolute.to_string_lossy());
        let root = plan.config_file_name().to_string_lossy().into_owned();
        let (file_name, text) = if self.host.to_path(&root) == requested {
            (root, plan.source().text().to_owned())
        } else {
            let Some(file_name) = plan
                .extended_source_files()
                .iter()
                .map(|name| name.to_string_lossy().into_owned())
                .find(|name| self.host.to_path(name) == requested)
            else {
                return Ok(self.no_source_file());
            };
            let snapshot = self.snapshot(params.snapshot)?;
            let Some(text) = snapshot
                .read_file(&file_name)
                .and_then(|bytes| tsc_program::decode_host_text(bytes).ok())
            else {
                return Ok(self.no_source_file());
            };
            (file_name, text)
        };
        // tsgo `NewTsconfigSourceFileFromFilePath`: a JSON source the parse
        // cache did not hash.
        let source = tsc_syntax::parse_json_source_text_from_snapshot(
            file_name.as_str(),
            tsc_diagnostics::TextSnapshot::new(text, Default::default()),
        );
        let (data, _) = encode_source_file(
            &source,
            &SourceFileFacts {
                path: Some(&requested),
                script_kind: ScriptKind::Json,
                unhashed: true,
                ..SourceFileFacts::default()
            },
        );
        Ok(self.source_file_payload(data))
    }

    fn lock_module_resolvers(&self) -> MutexGuard<'_, BTreeMap<u64, ModuleResolverEntry>> {
        self.module_resolvers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn resolution_paths(&self) -> crate::module_resolution::Paths {
        crate::module_resolution::Paths {
            current_directory: self.current_directory().to_owned(),
            case_sensitive: self.host.case_sensitive(),
        }
    }

    /// A request starts building a snapshot over `file_system` (the request's,
    /// else the session's): the context its resolver callbacks run in.
    fn begin_snapshot(&self, file_system: Option<&Arc<RequestFileSystem>>) {
        self.resolution.begin(InFlight {
            context: 0,
            fs: file_system.map_or_else(
                || Arc::clone(&self.fs),
                |file_system| Arc::clone(file_system) as Arc<dyn FileSystem>,
            ),
        });
    }

    /// tsgo `handleCreateModuleResolver`.
    fn handle_create_module_resolver(
        &self,
        params: &CreateModuleResolverParams,
    ) -> Result<u64, String> {
        let paths = self.resolution_paths();
        let resolutions = params
            .module_resolutions
            .as_ref()
            .map(|spec| StaticResolutions::compile(spec, &paths))
            .transpose()
            .map_err(client_error)?;
        let (compiler_options, program_options) = command_line_program_inputs(
            &params.compiler_options.0,
            JsStr::from_str(self.current_directory()),
            self.host.case_sensitive(),
        )
        .map_err(|error| format!("{error:?}"))?;
        let id = self.next_module_resolver.fetch_add(1, Ordering::Relaxed) + 1;
        let registration = Arc::new(ModuleResolverRegistration {
            compiler_options,
            program_options,
            resolutions,
            callback: params.resolve_module_name_callback.clone(),
        });
        let program =
            tsc_program::ModuleResolutionOverrideHandle(Arc::new(ProgramModuleResolution {
                registration: Arc::clone(&registration),
                state: Arc::clone(&self.resolution),
                paths,
            }));
        self.lock_module_resolvers()
            .insert(id, (registration, program));
        Ok(id)
    }

    /// tsgo `handleResolveModuleName`.
    fn handle_resolve_module_name(
        &self,
        params: &ResolveModuleNameParams,
    ) -> Result<ResolveModuleNameResult, String> {
        if params.module_name.is_empty() {
            return Err(client_error("moduleName is empty"));
        }
        let (registration, _) = self
            .lock_module_resolvers()
            .get(&params.resolver)
            .cloned()
            .ok_or_else(|| {
                client_error(format!("module resolver {} not found", params.resolver))
            })?;
        let mode = match params.resolution_mode {
            None => ResolutionMode::Unspecified,
            Some(mode) => api_resolution_mode(mode).ok_or_else(|| {
                client_error(format!("invalid resolutionMode {}", module_kind_name(mode)))
            })?,
        };
        let containing_directory = tsc_program::get_normalized_absolute_path(
            JsStr::from_str(&self.absolute_file_name(&params.containing_directory)?),
            JsStr::from_str(self.current_directory()),
        )
        .to_string_lossy()
        .into_owned();
        if params.snapshot != 0 && params.in_progress_snapshot != 0 {
            return Err(client_error(
                "snapshot and inProgressSnapshot are mutually exclusive",
            ));
        }
        let fs: Arc<dyn FileSystem> = if params.in_progress_snapshot != 0 {
            match self.resolution.in_flight() {
                Some(in_flight) if in_flight.context == params.in_progress_snapshot => in_flight.fs,
                _ => {
                    return Err(client_error(format!(
                        "in-progress snapshot {} not found",
                        params.in_progress_snapshot
                    )))
                }
            }
        } else if params.snapshot != 0 {
            // tsgo resolves over the snapshot, which keeps what it read.
            self.snapshot(params.snapshot)?.file_system()
        } else {
            Arc::clone(&self.fs)
        };
        let (module, trace) =
            match registration.chain(&params.module_name, &containing_directory, mode) {
                Chain::Answered(module) => (module.map(|module| *module), Vec::new()),
                Chain::Default(registration) if !registration.callback.is_empty() => {
                    let callback = ResolveModuleNameCallbackParams {
                        module_name: params.module_name.clone(),
                        containing_directory: containing_directory.clone(),
                        resolution_mode: match mode {
                            ResolutionMode::Unspecified => 0,
                            ResolutionMode::CommonJs => 1,
                            ResolutionMode::EsNext => 99,
                        },
                        snapshot: (params.snapshot != 0).then_some(params.snapshot),
                        in_progress_snapshot: (params.in_progress_snapshot != 0)
                            .then_some(params.in_progress_snapshot),
                    };
                    (
                        self.resolution.call_resolver(
                            &registration.callback,
                            &callback,
                            &self.resolution_paths(),
                        )?,
                        Vec::new(),
                    )
                }
                Chain::Default(registration) => {
                    // tsgo `module.NewResolver` over the snapshot's file system
                    // with the resolver's options.
                    let host = VfsCompilerHost::new(fs, self.current_directory());
                    let mut resolver = tsc_program::ModuleResolver::new_with_program_options(
                        &host,
                        &registration.compiler_options,
                        &registration.program_options,
                    )
                    .map_err(|error| error.to_string())?;
                    let resolution = resolver
                        .resolve_from_directory(
                            JsStr::from_str(&containing_directory),
                            JsStr::from_str(&params.module_name),
                            mode,
                        )
                        .map_err(|error| error.to_string())?;
                    let trace = resolution.trace().iter().map(|line| line.text()).collect();
                    let module = match resolution.outcome() {
                        ResolutionOutcome::Resolved(module) => Some(module.clone()),
                        ResolutionOutcome::NotFound => None,
                    };
                    (module, trace)
                }
            };
        Ok(ResolveModuleNameResult {
            resolved_module: module.as_ref().map(host_resolved_module_response),
            trace,
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
        self.begin_snapshot(file_system.as_ref());
        let snapshot = self
            .host
            .clone_snapshot(&root, file_changes, Some(&request));
        let resolution_error = self.resolution.end();
        // A failed resolver callback fails the snapshot, as tsgo's clone
        // returns the program's error.
        let snapshot = snapshot
            .map_err(|error| error.to_string())
            .and_then(|snapshot| resolution_error.map_or(Ok(snapshot), Err))
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
        self.begin_snapshot(file_system.as_ref());
        let snapshot = self
            .host
            .clone_snapshot(&base, file_changes, Some(&request));
        let resolution_error = self.resolution.end();
        let snapshot = snapshot
            .map_err(|error| error.to_string())
            .and_then(|snapshot| resolution_error.map_or(Ok(snapshot), Err))
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

    /// The registry of a snapshot the client holds.
    pub(crate) fn snapshot_registry(
        &self,
        handle: SnapshotId,
    ) -> Result<Arc<Mutex<Registry>>, String> {
        self.lock_snapshots()
            .get(&handle)
            .map(|data| Arc::clone(&data.registry))
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
                registry: Arc::default(),
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
                // tsgo `moduleResolverFactory`.
                let (registration, program) = self
                    .lock_module_resolvers()
                    .get(&program_options.module_resolver)
                    .cloned()
                    .ok_or_else(|| {
                        client_error(format!(
                            "module resolver {} not found",
                            program_options.module_resolver
                        ))
                    })?;
                if !registration.callback.is_empty() && self.resolution.connection().is_none() {
                    return Err(client_error("API connection is not initialized"));
                }
                request.module_resolution = Some(program);
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
pub(crate) fn parse<T: DeserializeOwned + Default>(
    type_name: &str,
    params: &[u8],
) -> Result<T, String> {
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

/// A file of a program the API asks about.
pub(crate) struct ProgramFile {
    pub(crate) document: Arc<BoundDocument>,
    /// The file name as the program spells it.
    file_name: String,
    path: String,
}

/// tsgo `DocumentIdentifier.ToFileName`: a URI's file name, or the file name
/// as given.
fn document_file_name(document: &DocumentIdentifier) -> Result<String, String> {
    if document.uri.is_empty() {
        Ok(document.file_name.clone())
    } else {
        crate::proto::uri_to_file_name(&document.uri).map_err(client_error)
    }
}

/// tsgo `core.GetScriptKindFromFileName`, a name without a known extension
/// being TypeScript (`EnsureScriptKind`).
fn script_kind_of(file_name: &str) -> ScriptKind {
    match ScriptKind::from_file_name(file_name) {
        ScriptKind::Unknown => ScriptKind::Ts,
        kind => kind,
    }
}

/// Go's `strconv.ParseUint(text, 10, 32)` and the text of its error.
pub(crate) fn go_parse_uint32(text: &str) -> Result<u32, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!(
            "strconv.ParseUint: parsing {text:?}: invalid syntax"
        ));
    }
    text.parse::<u32>()
        .map_err(|_| format!("strconv.ParseUint: parsing {text:?}: value out of range"))
}

fn is_string_literal_like(program: &ProjectProgram, node: NodeId) -> bool {
    program
        .with_live(|live| {
            live.with_checker(|checker| {
                matches!(
                    checker.binder.source_of_node(node).arena.node(node).kind,
                    SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
                )
            })
        })
        .unwrap_or(false)
}

/// The text of a string literal of `source`.
fn literal_text(source: &tsc_syntax::SourceFile, node: NodeId) -> Option<String> {
    if !source.arena.contains_node(node) {
        return None;
    }
    match &source.arena.node(node).data {
        NodeData::StringLiteral(literal) => Some(literal.text.to_string_lossy().into_owned()),
        NodeData::NoSubstitutionTemplateLiteral(literal) => {
            Some(literal.text.to_string_lossy().into_owned())
        }
        _ => None,
    }
}

/// tsgo `Program.GetModeForUsageLocation`.
fn mode_for_usage_location(program: &ProjectProgram, usage: NodeId) -> u32 {
    program
        .with_live(|live| live.with_checker(|checker| checker.api_mode_for_usage_location(usage)))
        .unwrap_or(0)
}

/// tsgo's `core.ResolutionMode` number as the resolution tables key it.
fn resolution_mode(mode: u32) -> Option<ResolutionMode> {
    match mode {
        0 => Some(ResolutionMode::Unspecified),
        1 => Some(ResolutionMode::CommonJs),
        99 => Some(ResolutionMode::EsNext),
        _ => None,
    }
}

fn package_id(package_id: Option<&PackageId>) -> Option<PackageIdResponse> {
    package_id.map(|id| PackageIdResponse {
        name: id.name().to_string_lossy().into_owned(),
        sub_module_name: id.submodule_name().to_string_lossy().into_owned(),
        version: id.version().to_string_lossy().into_owned(),
        peer_dependencies: id
            .peer_dependencies()
            .map(|peers| peers.to_string_lossy().into_owned())
            .unwrap_or_default(),
    })
}

/// tsgo `Program.GetResolvedModule` and `newResolvedModuleResponse`.
fn resolved_module(
    program: &ProjectProgram,
    file: &ProgramFile,
    module_name: &str,
    mode: u32,
) -> Option<ResolvedModuleResponse> {
    let key = ResolutionKey::new(
        CanonicalPath::from_js_normalized(JsStr::from_str(&file.path)).ok()?,
        JsStr::from_str(module_name).to_owned(),
        resolution_mode(mode)?,
    );
    let resolution: &ModuleResolution =
        program.prepared().resolutions().require_module(&key).ok()?;
    let ResolutionOutcome::Resolved(module) = resolution.outcome() else {
        return None;
    };
    Some(ResolvedModuleResponse {
        resolved_file_name: module
            .target()
            .resolved_file()
            .display()
            .to_string_lossy()
            .into_owned(),
        original_path: module
            .original_path()
            .map(|path| path.display().to_string_lossy().into_owned())
            .unwrap_or_default(),
        extension: module.extension().as_js().to_string_lossy().into_owned(),
        resolved_using_ts_extension: module.resolved_using_ts_extension(),
        resolved_using_extra_extensions: false,
        package_id: package_id(module.package_id()),
        is_external_library_import: module.is_external_library_import(),
        alternate_result: resolution
            .alternate_result()
            .map(|path| path.display().to_string_lossy().into_owned())
            .unwrap_or_default(),
    })
}

/// tsgo `newResolvedModuleResponse` of a resolver's module.
fn host_resolved_module_response(
    module: &tsc_program::HostResolvedModule,
) -> ResolvedModuleResponse {
    ResolvedModuleResponse {
        resolved_file_name: module
            .resolved_file()
            .display()
            .to_string_lossy()
            .into_owned(),
        original_path: module
            .original_path()
            .map(|path| path.display().to_string_lossy().into_owned())
            .unwrap_or_default(),
        extension: module.extension().as_js().to_string_lossy().into_owned(),
        resolved_using_ts_extension: module.resolved_using_ts_extension(),
        resolved_using_extra_extensions: false,
        package_id: package_id(module.package_id()),
        is_external_library_import: module.is_external_library_import(),
        alternate_result: module
            .alternate_result()
            .map(|path| path.display().to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// tsgo `Program.GetResolvedTypeReferenceDirective` and
/// `newResolvedTypeReferenceDirectiveResponse`.
fn resolved_type_reference_directive(
    program: &ProjectProgram,
    file: &ProgramFile,
    name: &str,
    mode: u32,
) -> Option<ResolvedTypeReferenceDirectiveResponse> {
    let key = TypeReferenceResolutionKey::source(
        CanonicalPath::from_js_normalized(JsStr::from_str(&file.path)).ok()?,
        JsStr::from_str(name).to_owned(),
        resolution_mode(mode)?,
    );
    let resolution: &TypeReferenceResolution = program
        .prepared()
        .resolutions()
        .require_type_reference(&key)
        .ok()?;
    let ResolutionOutcome::Resolved(directive) = resolution.outcome() else {
        return None;
    };
    Some(ResolvedTypeReferenceDirectiveResponse {
        primary: directive.primary(),
        resolved_file_name: directive.target().display().to_string_lossy().into_owned(),
        original_path: directive
            .original_path()
            .map(|path| path.display().to_string_lossy().into_owned())
            .unwrap_or_default(),
        package_id: package_id(directive.package_id()),
        is_external_library_import: directive.is_external_library_import(),
    })
}

/// tsgo `handleGetSourceFileMetadata`: the program's facts of a file.
fn source_file_metadata(
    program: &ProjectProgram,
    file: &ProgramFile,
) -> Option<SourceFileMetadata> {
    let prepared = program.prepared();
    let id = prepared
        .source_id(&CanonicalPath::from_js_normalized(JsStr::from_str(&file.path)).ok()?)?;
    let source = prepared.source_file(id)?;
    let package = source
        .package_scope()
        .and_then(|scope| prepared.package(scope));
    // tsgo `loadSourceFileMetaData` keeps the `type` field for a file whose
    // extension leaves its format open under node16 to nodenext resolution,
    // and for a file in node_modules.
    let resolution_kind = prepared.compiler_options().emit_module_resolution_kind();
    let format_open = ![".mts", ".cts", ".mjs", ".cjs"]
        .iter()
        .any(|extension| file.file_name.ends_with(extension));
    let keeps_type = format_open && (3..=99).contains(&resolution_kind)
        || file.file_name.contains("/node_modules/");
    let package_json_type = match package
        .filter(|_| keeps_type)
        .map(|package| package.module_type())
    {
        Some(PackageJsonType::Module) => "module".to_owned(),
        Some(PackageJsonType::CommonJs) => "commonjs".to_owned(),
        // tsgo keeps any other string of the `type` field.
        Some(PackageJsonType::Other) => package
            .and_then(|package| serde_json::from_str::<serde_json::Value>(package.text()).ok())
            .and_then(|json| json.get("type")?.as_str().map(str::to_owned))
            .unwrap_or_default(),
        Some(PackageJsonType::Unspecified) | None => String::new(),
    };
    Some(SourceFileMetadata {
        is_default_library: prepared.library_files().contains(&id),
        is_from_external_library: source.found_searching_node_modules(),
        package_json_type,
        package_json_directory: package
            .map(|package| {
                tsc_program::get_directory_path(package.package_json().display())
                    .to_string_lossy()
                    .into_owned()
            })
            .unwrap_or_default(),
        implied_node_format: match source.implied_node_format() {
            Some(ResolutionMode::CommonJs) => 1,
            Some(ResolutionMode::EsNext) => 99,
            Some(ResolutionMode::Unspecified) | None => 0,
        },
    })
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
