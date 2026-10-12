//! tsgo `contentmapper` hostimpl.go: the host that runs the configured
//! content mappers. Each mapper identity runs one process, shared by every
//! project that uses it; a project is opened in the mapper lazily, and a
//! transform request turns a file's content into virtual TypeScript.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::value::RawValue;
use tsc_host::ipc::jsonrpc::MessageReader;
use tsc_host::ipc::{AsyncConn, CallError, Handler, Payload};

use super::errors::{
    InitializeError, InitializeErrorKind, ProjectErrorKind, ProjectFailure, TransformFailure,
};
use super::protocol::{
    decode_transform_result, CloseProjectParams, InitializeParams, InitializeResult,
    OpenProjectParams, OpenProjectResult, PositionEncoding, TransformOutput, TransformParams,
    METHOD_CLOSE_PROJECT, METHOD_INITIALIZE, METHOD_OPEN_PROJECT, METHOD_TRANSFORM,
};
use super::{Mapper, Project};
use crate::go_json::GoJson;

/// tsgo `initializeTimeoutSeconds`.
pub const INITIALIZE_TIMEOUT_SECONDS: u64 = 5;

/// A spawned mapper process: its standard output to read, its standard
/// input to write, and control over it.
pub struct MapperProcess {
    pub stdout: Box<dyn Read + Send>,
    pub stdin: Box<dyn Write + Send>,
    pub control: Box<dyn ProcessControl>,
}

/// What the host needs of a running mapper process.
pub trait ProcessControl: Send + Sync {
    /// Tears the process down; later calls do nothing.
    fn close(&self);
    /// The exit code, once the process has exited.
    fn exit_code(&self) -> Option<i32>;
}

/// tsgo `Spawner`: starts a mapper's command in its package directory,
/// sending its standard error to `stderr`. Production hosts start a
/// process; tests serve the protocol in-process.
pub trait Spawner: Send + Sync {
    fn spawn(
        &self,
        command: &[String],
        dir: &str,
        stderr: Box<dyn Write + Send>,
    ) -> Result<MapperProcess, String>;
}

/// tsgo `Logger`: receives protocol and process output as whole lines.
pub type Logger = Arc<dyn Fn(&str) + Send + Sync>;

/// tsgo `Request`: one content-mapped file to transform.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    pub file_name: &'a str,
    pub content: &'a str,
}

/// tsgo `ProjectSpec`: the project configuration a project's mappers see.
#[derive(Debug, Eq, PartialEq)]
pub struct ProjectSpec {
    /// The absolute project configuration file, or empty without one.
    pub config_file_name: String,
    pub mappers: Vec<Arc<Mapper>>,
    /// The project's effective compiler options, as tsgo marshals them.
    pub compiler_options: Arc<GoJson>,
}

/// tsgo `OptionPathSegment`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OptionPathSegment {
    Property(String),
    Index(usize),
}

/// tsgo `OptionDiagnostic`: a mapper's diagnostic about its options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OptionDiagnostic {
    pub mapper: Arc<Mapper>,
    pub path: Vec<OptionPathSegment>,
    pub source: String,
    pub code: i32,
    pub message_text: String,
}

/// tsgo `OperationTiming`: how often an operation ran and for how long.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OperationTiming {
    pub count: u64,
    pub duration: Duration,
}

impl OperationTiming {
    fn since(self, previous: Self) -> Self {
        Self {
            count: self.count - self.count.min(previous.count),
            duration: self.duration.saturating_sub(previous.duration),
        }
    }
}

/// tsgo `MapperTimings`: one mapper identity's process and protocol work.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MapperTimings {
    pub spawn: OperationTiming,
    pub initialize: OperationTiming,
    pub open_project: OperationTiming,
    pub close_project: OperationTiming,
    pub transform: OperationTiming,
}

/// tsgo `Timings`: by mapper identity, and the wall time some request was
/// in flight.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Timings {
    pub mappers: HashMap<String, MapperTimings>,
    pub request_wait: Duration,
}

impl Timings {
    /// tsgo `Since`: the work after `previous`.
    pub fn since(&self, previous: &Timings) -> Timings {
        Timings {
            mappers: self
                .mappers
                .iter()
                .map(|(identity, current)| {
                    let before = previous.mappers.get(identity).copied().unwrap_or_default();
                    (
                        identity.clone(),
                        MapperTimings {
                            spawn: current.spawn.since(before.spawn),
                            initialize: current.initialize.since(before.initialize),
                            open_project: current.open_project.since(before.open_project),
                            close_project: current.close_project.since(before.close_project),
                            transform: current.transform.since(before.transform),
                        },
                    )
                })
                .collect(),
            request_wait: self.request_wait.saturating_sub(previous.request_wait),
        }
    }
}

#[derive(Clone, Copy)]
enum Operation {
    Spawn,
    Initialize,
    OpenProject,
    CloseProject,
    Transform,
}

/// tsgo `timingCollector`.
#[derive(Default)]
struct TimingCollector {
    state: Mutex<TimingState>,
}

#[derive(Default)]
struct TimingState {
    mappers: HashMap<String, MapperTimings>,
    active_requests: u64,
    request_wait_start: Option<Instant>,
    request_wait_elapsed: Duration,
}

impl TimingCollector {
    fn lock(&self) -> MutexGuard<'_, TimingState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record(&self, identity: &str, operation: Operation, start: Instant) {
        let elapsed = start.elapsed();
        let mut state = self.lock();
        let timings = state.mappers.entry(identity.to_owned()).or_default();
        let timing = match operation {
            Operation::Spawn => &mut timings.spawn,
            Operation::Initialize => &mut timings.initialize,
            Operation::OpenProject => &mut timings.open_project,
            Operation::CloseProject => &mut timings.close_project,
            Operation::Transform => &mut timings.transform,
        };
        timing.count += 1;
        timing.duration += elapsed;
    }

    /// tsgo `startRequest`.
    fn start_request(&self) -> Instant {
        let mut state = self.lock();
        if state.active_requests == 0 {
            state.request_wait_start = Some(Instant::now());
        }
        state.active_requests += 1;
        Instant::now()
    }

    /// tsgo `finishRequest`.
    fn finish_request(&self, identity: &str, operation: Operation, start: Instant) {
        self.record(identity, operation, start);
        let mut state = self.lock();
        state.active_requests -= 1;
        if state.active_requests == 0 {
            if let Some(started) = state.request_wait_start.take() {
                state.request_wait_elapsed += started.elapsed();
            }
        }
    }

    fn snapshot(&self) -> Timings {
        let state = self.lock();
        let mut request_wait = state.request_wait_elapsed;
        if state.active_requests != 0 {
            if let Some(started) = state.request_wait_start {
                request_wait += started.elapsed();
            }
        }
        Timings {
            mappers: state.mappers.clone(),
            request_wait,
        }
    }
}

/// tsgo `rejectHandler`: the protocol is driven by the host; a request from
/// the mapper is a protocol violation.
struct RejectHandler;

impl Handler for RejectHandler {
    fn handle_request(&self, method: &str, _params: &[u8]) -> Result<Payload, String> {
        Err(format!(
            "content mapper sent an unexpected request: {method}"
        ))
    }

    fn handle_notification(&self, _method: &str, _params: &[u8]) {}
}

/// A mapper's running connection.
struct Connection {
    conn: AsyncConn<Box<dyn Write + Send>>,
    control: Box<dyn ProcessControl>,
}

/// tsgo `mapperConn`: one mapper identity's connection, or why it failed to
/// start (kept, so a broken mapper is not started again), and the projects
/// and acquisitions that retain it.
#[derive(Default)]
struct MapperConn {
    connection: Option<Arc<Connection>>,
    failure: Option<InitializeError>,
    encoding: Option<PositionEncoding>,
    diagnostic_source: String,
    refs: usize,
}

/// tsgo `projectEntry`.
struct ProjectEntry {
    mapper: Arc<Mapper>,
    spec: Arc<ProjectSpec>,
    opened: bool,
    config_identity: String,
    watched_files: Vec<String>,
    option_diagnostics: Vec<OptionDiagnostic>,
}

/// tsgo `projectLease`'s shared part: a project spec's entries, by the
/// mapper object (its address, as tsgo keys them by pointer).
struct Lease {
    mappers: Vec<Arc<Mapper>>,
    entries: Vec<(usize, String)>,
    refs: usize,
}

fn mapper_key(mapper: &Arc<Mapper>) -> usize {
    Arc::as_ptr(mapper) as usize
}

#[derive(Default)]
struct State {
    closed: bool,
    conns: HashMap<String, MapperConn>,
    projects: HashMap<String, ProjectEntry>,
    leases: HashMap<String, Lease>,
    next_project_id: u64,
}

struct HostInner {
    spawner: Arc<dyn Spawner>,
    logger: Option<Logger>,
    locale: String,
    timing: TimingCollector,
    state: Mutex<State>,
}

/// tsgo `contentmapper.Host`.
#[derive(Clone)]
pub struct Host(Arc<HostInner>);

impl Host {
    /// tsgo `NewHost`: mappers run through `spawner`, initialized with
    /// `locale` (empty for the default).
    pub fn new(spawner: Arc<dyn Spawner>, locale: impl Into<String>) -> Self {
        Self::with_logger(spawner, locale, None)
    }

    /// tsgo `NewHostWithOptions`: with protocol and process logging.
    pub fn with_logger(
        spawner: Arc<dyn Spawner>,
        locale: impl Into<String>,
        logger: Option<Logger>,
    ) -> Self {
        Self(Arc::new(HostInner {
            spawner,
            logger,
            locale: locale.into(),
            timing: TimingCollector::default(),
            state: Mutex::new(State::default()),
        }))
    }

    /// tsgo `Timings`: the work so far.
    pub fn timings(&self) -> Timings {
        self.0.timing.snapshot()
    }

    /// tsgo `Project`: a retained view of `spec`. Specs with the same
    /// configuration file name, compiler options and mappers (the same
    /// objects) share their mapper projects. Nothing after [`Self::close`].
    pub fn project(&self, spec: Arc<ProjectSpec>) -> Option<ProjectView> {
        let key = spec_key(&spec);
        let mut state = self.lock();
        if state.closed {
            return None;
        }
        if let Some(lease) = state.leases.get_mut(&key) {
            lease.refs += 1;
            return Some(ProjectView::new(self, key));
        }
        let mut entries = Vec::with_capacity(spec.mappers.len());
        for mapper in &spec.mappers {
            let identity = mapper.identity();
            let handle = format!("{identity}:{}", state.next_project_id);
            state.next_project_id += 1;
            state.projects.insert(
                handle.clone(),
                ProjectEntry {
                    mapper: Arc::clone(mapper),
                    spec: Arc::clone(&spec),
                    opened: false,
                    config_identity: String::new(),
                    watched_files: Vec::new(),
                    option_diagnostics: Vec::new(),
                },
            );
            state.conns.entry(identity).or_default().refs += 1;
            entries.push((mapper_key(mapper), handle));
        }
        state.leases.insert(
            key.clone(),
            Lease {
                mappers: spec.mappers.clone(),
                entries,
                refs: 1,
            },
        );
        Some(ProjectView::new(self, key))
    }

    /// tsgo `Acquire`: retains the processes of these mappers (without
    /// starting them) until the returned guard is dropped.
    pub fn acquire(&self, mappers: &[Arc<Mapper>]) -> Acquisition {
        let mut identities = Vec::new();
        let mut state = self.lock();
        if !state.closed {
            for mapper in mappers {
                let identity = mapper.identity();
                if identities.contains(&identity) {
                    continue;
                }
                state.conns.entry(identity.clone()).or_default().refs += 1;
                identities.push(identity);
            }
        }
        Acquisition {
            host: self.clone(),
            identities,
        }
    }

    /// tsgo `Transform`: transforms in a short-lived project with default
    /// compiler options.
    pub fn transform(
        &self,
        mapper: &Arc<Mapper>,
        request: Request<'_>,
    ) -> Result<TransformOutput, TransformFailure> {
        let Some(project) = self.project(Arc::new(ProjectSpec {
            config_file_name: String::new(),
            mappers: vec![Arc::clone(mapper)],
            compiler_options: Arc::new(GoJson::Object(Vec::new())),
        })) else {
            return Err(TransformFailure::Host(
                "content mapper project is closed".to_owned(),
            ));
        };
        project.transform(mapper, request)
    }

    /// tsgo `Close`: tears every mapper process down.
    pub fn close(&self) {
        let connections = {
            let mut state = self.lock();
            state.closed = true;
            state.projects.clear();
            state.leases.clear();
            state
                .conns
                .drain()
                .filter_map(|(_, conn)| conn.connection)
                .collect::<Vec<_>>()
        };
        for connection in connections {
            connection.control.close();
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// tsgo `connForLocked`: the mapper identity's connection, started on
    /// first use.
    fn conn_for_locked(
        &self,
        state: &mut State,
        mapper: &Mapper,
    ) -> Result<(Arc<Connection>, PositionEncoding, String), TransformFailure> {
        if state.closed {
            return Err(TransformFailure::Host(
                "content mapper host is closed".to_owned(),
            ));
        }
        let entry = state.conns.entry(mapper.identity()).or_default();
        if let Some(failure) = &entry.failure {
            return Err(TransformFailure::Initialize(failure.clone()));
        }
        if let (Some(connection), Some(encoding)) = (&entry.connection, entry.encoding) {
            return Ok((
                Arc::clone(connection),
                encoding,
                entry.diagnostic_source.clone(),
            ));
        }
        match self.dial(mapper) {
            Ok((connection, encoding, source)) => {
                entry.connection = Some(Arc::clone(&connection));
                entry.encoding = Some(encoding);
                entry.diagnostic_source = source.clone();
                Ok((connection, encoding, source))
            }
            Err(failure) => {
                entry.failure = Some(failure.clone());
                Err(TransformFailure::Initialize(failure))
            }
        }
    }

    /// tsgo `NewHostWithOptions`'s dial: start the process, run its
    /// connection, and initialize it within the time limit.
    fn dial(
        &self,
        mapper: &Mapper,
    ) -> Result<(Arc<Connection>, PositionEncoding, String), InitializeError> {
        let mapper_name = mapper.diagnostic_name().to_owned();
        let initialize_error = |kind| InitializeError {
            kind,
            mapper_name: mapper_name.clone(),
        };
        let Some(command) = mapper
            .manifest
            .exec
            .as_ref()
            .filter(|exec| !exec.is_empty())
        else {
            return Err(initialize_error(InitializeErrorKind::Request {
                detail: format!(
                    "content mapper {:?} declares no command to run",
                    mapper.definition.package
                ),
            }));
        };
        let stderr: Box<dyn Write + Send> = match &self.0.logger {
            Some(logger) => Box::new(StderrLogger {
                prefix: format!("[content mapper: {mapper_name}] stderr: "),
                logger: Arc::clone(logger),
                pending: Vec::new(),
            }),
            None => Box::new(std::io::sink()),
        };
        let identity = mapper.identity();
        let spawn_start = Instant::now();
        let process = self
            .0
            .spawner
            .spawn(command, &mapper.package_directory, stderr);
        self.0
            .timing
            .record(&identity, Operation::Spawn, spawn_start);
        let process = process.map_err(|detail| {
            initialize_error(InitializeErrorKind::ProcessStart {
                command: command[0].clone(),
                detail,
            })
        })?;
        let (stdout, stdin): (Box<dyn Read + Send>, Box<dyn Write + Send>) = match &self.0.logger {
            Some(logger) => (
                Box::new(LoggingStream::new(
                    process.stdout,
                    format!("[content mapper: {mapper_name}] receive: "),
                    Arc::clone(logger),
                )),
                Box::new(LoggingStream::new(
                    process.stdin,
                    format!("[content mapper: {mapper_name}] send: "),
                    Arc::clone(logger),
                )),
            ),
            None => (process.stdout, process.stdin),
        };
        let connection = Arc::new(Connection {
            conn: AsyncConn::new(stdin, Box::new(RejectHandler)),
            control: process.control,
        });
        {
            let connection = Arc::clone(&connection);
            let _ = std::thread::Builder::new()
                .name("tsc-rs-content-mapper".to_owned())
                .spawn(move || {
                    let _ = connection.conn.run(&mut MessageReader::new(stdout));
                    connection.control.close();
                });
        }
        let params = serde_json::to_string(&InitializeParams {
            locale: &self.0.locale,
            position_encodings: [
                PositionEncoding::Utf8.as_str(),
                PositionEncoding::Utf16.as_str(),
            ],
        })
        .expect("initialize parameters serialize");
        let initialize_start = self.0.timing.start_request();
        let initialized = connection.conn.call(
            METHOD_INITIALIZE,
            &params,
            Some(Duration::from_secs(INITIALIZE_TIMEOUT_SECONDS)),
        );
        self.0
            .timing
            .finish_request(&identity, Operation::Initialize, initialize_start);
        let handshake = initialized
            .map_err(HandshakeFailure::Call)
            .and_then(|raw| handshake_result(&raw).map_err(HandshakeFailure::Initialize));
        match handshake {
            Ok((encoding, source)) => Ok((connection, encoding, source)),
            Err(failure) => {
                let exit_code = connection.control.exit_code();
                connection.control.close();
                Err(match (failure, exit_code) {
                    (HandshakeFailure::Initialize(kind), _) => initialize_error(kind),
                    (_, Some(exit_code)) => {
                        initialize_error(InitializeErrorKind::ProcessExit { exit_code })
                    }
                    (HandshakeFailure::Call(CallError::TimedOut), None) => {
                        initialize_error(InitializeErrorKind::NoResponse {
                            timeout_seconds: INITIALIZE_TIMEOUT_SECONDS,
                        })
                    }
                    (HandshakeFailure::Call(CallError::Failed(error)), None) => {
                        initialize_error(InitializeErrorKind::Request {
                            detail: error.to_string(),
                        })
                    }
                })
            }
        }
    }

    /// tsgo `openProjectLocked`.
    fn open_project_locked(&self, state: &mut State, handle: &str) -> Result<(), ProjectFailure> {
        let (mapper, spec) = match state.projects.get(handle) {
            Some(entry) if entry.opened => return Ok(()),
            Some(entry) => (Arc::clone(&entry.mapper), Arc::clone(&entry.spec)),
            None => return Ok(()),
        };
        let (connection, _, diagnostic_source) =
            self.conn_for_locked(state, &mapper)
                .map_err(|failure| match failure {
                    TransformFailure::Initialize(error) => ProjectFailure::Initialize(error),
                    failure => ProjectFailure::Request(failure.to_string()),
                })?;
        let compiler_options = RawValue::from_string(spec.compiler_options.compact())
            .expect("compiler options are JSON");
        let options = mapper
            .definition
            .options
            .as_deref()
            .map(|options| RawValue::from_string(options.to_owned()))
            .transpose()
            .map_err(|error| ProjectFailure::Request(error.to_string()))?;
        let params = serde_json::to_string(&OpenProjectParams {
            config_file_name: &spec.config_file_name,
            project_handle: handle,
            options: options.as_deref(),
            compiler_options: &compiler_options,
        })
        .expect("openProject parameters serialize");
        let identity = mapper.identity();
        let start = self.0.timing.start_request();
        let raw = connection.conn.call(METHOD_OPEN_PROJECT, &params, None);
        self.0
            .timing
            .finish_request(&identity, Operation::OpenProject, start);
        let raw = raw.map_err(|error| ProjectFailure::Request(error.to_string()))?;
        let malformed = ProjectFailure::Project(ProjectErrorKind::MalformedResponse);
        let result: OpenProjectResult =
            serde_json::from_slice(&raw).map_err(|_| malformed.clone())?;
        let dynamic = mapper.manifest.dynamic_config;
        if dynamic && result.config_identity.is_empty() {
            return Err(ProjectFailure::Project(
                ProjectErrorKind::MissingConfigIdentity,
            ));
        }
        if !dynamic && !result.config_identity.is_empty() {
            return Err(ProjectFailure::Project(
                ProjectErrorKind::UnexpectedConfigIdentity,
            ));
        }
        if !dynamic && !result.watched_files.is_empty() {
            return Err(ProjectFailure::Project(
                ProjectErrorKind::UnexpectedWatchedFiles,
            ));
        }
        // tsgo `TryRootedFilePathFromAbsolute`: absolute, not a URL, and
        // normalized.
        let mut watched_files = Vec::with_capacity(result.watched_files.len());
        for file in &result.watched_files {
            let file = tsc_diagnostics::JsStr::from(file.as_str());
            let rooted =
                crate::js_path::root_parts(file).is_some_and(|(root, _)| !root.contains("://"));
            let normalized = rooted
                .then(|| {
                    crate::module_resolution::normalize_absolute_js_path(file, None, true).ok()
                })
                .flatten();
            let Some(normalized) = normalized else {
                return Err(ProjectFailure::Project(
                    ProjectErrorKind::NonAbsoluteWatchedFile,
                ));
            };
            watched_files.push(normalized.to_string_lossy().into_owned());
        }
        let mut option_diagnostics = Vec::with_capacity(result.option_diagnostics.len());
        for diagnostic in result.option_diagnostics {
            let mut path = Vec::with_capacity(diagnostic.path.len());
            for segment in &diagnostic.path {
                let text = segment.get();
                path.push(if text.starts_with('"') {
                    OptionPathSegment::Property(
                        serde_json::from_str(text).map_err(|_| malformed.clone())?,
                    )
                } else {
                    OptionPathSegment::Index(
                        serde_json::from_str(text).map_err(|_| malformed.clone())?,
                    )
                });
            }
            option_diagnostics.push(OptionDiagnostic {
                mapper: Arc::clone(&mapper),
                path,
                source: diagnostic_source.clone(),
                code: diagnostic.code,
                message_text: diagnostic.message_text,
            });
        }
        let entry = state.projects.get_mut(handle).expect("checked above");
        entry.config_identity = result.config_identity;
        entry.watched_files = watched_files;
        entry.option_diagnostics = option_diagnostics;
        entry.opened = true;
        Ok(())
    }

    /// tsgo `release`: drops the processes no project or acquisition
    /// retains any more.
    fn release(&self, identities: &[String]) {
        let mut closing = Vec::new();
        {
            let mut state = self.lock();
            for identity in identities {
                let Some(conn) = state.conns.get_mut(identity) else {
                    continue;
                };
                conn.refs -= 1;
                if conn.refs == 0 {
                    if let Some(connection) = state
                        .conns
                        .remove(identity)
                        .and_then(|conn| conn.connection)
                    {
                        closing.push(connection);
                    }
                }
            }
        }
        for connection in closing {
            connection.control.close();
        }
    }
}

enum HandshakeFailure {
    Call(CallError),
    Initialize(InitializeErrorKind),
}

/// tsgo `handshake`'s checks of the initialize result.
fn handshake_result(raw: &[u8]) -> Result<(PositionEncoding, String), InitializeErrorKind> {
    let result: InitializeResult =
        serde_json::from_slice(raw).map_err(|error| InitializeErrorKind::InvalidResponse {
            detail: error.to_string(),
        })?;
    let Some(encoding) = PositionEncoding::parse(&result.position_encoding) else {
        return Err(InitializeErrorKind::PositionEncoding {
            encoding: result.position_encoding,
        });
    };
    let source = result.diagnostic_source;
    if source.trim().is_empty() {
        return Err(InitializeErrorKind::EmptyDiagnosticSource);
    }
    // tsgo: "typescript", "tsc" and every supported extension (with JSON),
    // in any case.
    const RESERVED: &[&str] = &[
        "typescript",
        "tsc",
        "ts",
        "tsx",
        "d.ts",
        "cts",
        "d.cts",
        "mts",
        "d.mts",
        "js",
        "jsx",
        "cjs",
        "mjs",
        "json",
    ];
    if RESERVED
        .iter()
        .any(|reserved| source.eq_ignore_ascii_case(reserved))
    {
        return Err(InitializeErrorKind::ReservedDiagnosticSource { source });
    }
    Ok((encoding, source))
}

/// tsgo `projectSpecKey`: the spec's configuration file name and the
/// identities of its compiler options and mappers.
fn spec_key(spec: &ProjectSpec) -> String {
    let mut key = format!(
        "{}\0{:p}",
        spec.config_file_name,
        Arc::as_ptr(&spec.compiler_options)
    );
    for mapper in &spec.mappers {
        key.push_str(&format!("\0{:p}", Arc::as_ptr(mapper)));
    }
    key
}

/// The JSON text of one of tsgo's `CompilerOptions` fields, when set.
pub(crate) fn option_json(options: &GoJson, name: &str) -> Option<String> {
    let GoJson::Object(fields) = options else {
        return None;
    };
    fields
        .iter()
        .find(|(field, _)| field == name)
        .map(|(_, value)| value.compact())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// tsgo's static identity: the mapper's identity and its transform identity.
fn static_identity(mapper: &Mapper, compiler_options: &GoJson) -> String {
    let declared = mapper.declared_options_json(|name| option_json(compiler_options, name));
    format!(
        "{}:{}",
        mapper.identity(),
        hex(&mapper.transform_identity(&declared))
    )
}

/// tsgo `combinedIdentity`: a dynamic mapper's identity with its project's
/// configuration identity.
fn combined_identity(mapper: &Mapper, config_identity: &str, compiler_options: &GoJson) -> String {
    let declared = mapper.declared_options_json(|name| option_json(compiler_options, name));
    let transform = mapper.transform_identity(&declared);
    let identity = mapper.identity();
    let options = mapper.definition.options.as_deref().unwrap_or_default();
    let mut buffer = Vec::new();
    buffer.extend_from_slice(identity.as_bytes());
    buffer.push(0);
    buffer.extend_from_slice(options.as_bytes());
    buffer.push(0);
    buffer.extend_from_slice(config_identity.as_bytes());
    buffer.push(0);
    buffer.extend_from_slice(&transform);
    let hash = xxhash_rust::xxh3::xxh3_128(&buffer).to_be_bytes();
    format!("{identity}:{}", hex(&hash))
}

/// tsgo `projectView` (a retained `projectLease`): released once, on
/// [`Project::close`] or when dropped.
pub struct ProjectView {
    host: Host,
    key: String,
    released: AtomicBool,
}

impl ProjectView {
    fn new(host: &Host, key: String) -> Self {
        Self {
            host: host.clone(),
            key,
            released: AtomicBool::new(false),
        }
    }

    fn handle_of(&self, state: &State, mapper: &Arc<Mapper>) -> Option<String> {
        let lease = state.leases.get(&self.key)?;
        lease
            .entries
            .iter()
            .find(|(key, _)| *key == mapper_key(mapper))
            .map(|(_, handle)| handle.clone())
    }

    fn entry_identity(&self, state: &mut State, handle: &str) -> Result<String, ProjectFailure> {
        let Some(entry) = state.projects.get(handle) else {
            return Ok(String::new());
        };
        let mapper = Arc::clone(&entry.mapper);
        let spec = Arc::clone(&entry.spec);
        if mapper.manifest.dynamic_config {
            self.host.open_project_locked(state, handle)?;
            let config_identity = state.projects[handle].config_identity.clone();
            return Ok(combined_identity(
                &mapper,
                &config_identity,
                &spec.compiler_options,
            ));
        }
        Ok(static_identity(&mapper, &spec.compiler_options))
    }
}

impl Project for ProjectView {
    /// tsgo `Transform`: opens the project in the mapper if needed, then
    /// transforms (outside the host's lock, so transforms run together).
    fn transform(
        &self,
        mapper: &Arc<Mapper>,
        request: Request<'_>,
    ) -> Result<TransformOutput, TransformFailure> {
        let (connection, encoding, source, handle) = {
            let mut state = self.host.lock();
            let Some(handle) = self
                .handle_of(&state, mapper)
                .filter(|handle| state.projects.contains_key(handle))
            else {
                return Err(TransformFailure::Host(
                    "content mapper project is closed".to_owned(),
                ));
            };
            self.host.open_project_locked(&mut state, &handle).map_err(
                |failure| match failure {
                    ProjectFailure::Initialize(error) => TransformFailure::Initialize(error),
                    failure => TransformFailure::Project(failure),
                },
            )?;
            let (connection, encoding, source) = self.host.conn_for_locked(&mut state, mapper)?;
            (connection, encoding, source, handle)
        };
        let params = serde_json::to_string(&TransformParams {
            file_name: request.file_name,
            content: request.content,
            project_handle: &handle,
        })
        .expect("transform parameters serialize");
        let identity = mapper.identity();
        let start = self.host.0.timing.start_request();
        let raw = connection.conn.call(METHOD_TRANSFORM, &params, None);
        self.host
            .0
            .timing
            .finish_request(&identity, Operation::Transform, start);
        let raw = raw.map_err(|error| TransformFailure::Request(error.to_string()))?;
        decode_transform_result(&raw, request.content, encoding, &source)
            .map_err(TransformFailure::Response)
    }

    /// tsgo `Identity`: the mapper's transform identity in this project
    /// (with its dynamic configuration's), or nothing for another mapper.
    fn identity(&self, mapper: &Arc<Mapper>) -> Result<String, ProjectFailure> {
        let mut state = self.host.lock();
        let Some(handle) = self.handle_of(&state, mapper) else {
            return Ok(String::new());
        };
        self.entry_identity(&mut state, &handle)
    }

    /// tsgo `Identities`: every mapper's transform identity, in the spec's
    /// order.
    fn identities(&self) -> Result<Vec<String>, ProjectFailure> {
        let mut state = self.host.lock();
        let Some(lease) = state.leases.get(&self.key) else {
            return Ok(Vec::new());
        };
        let handles: Vec<String> = lease
            .mappers
            .iter()
            .filter_map(|mapper| {
                lease
                    .entries
                    .iter()
                    .find(|(key, _)| *key == mapper_key(mapper))
                    .map(|(_, handle)| handle.clone())
            })
            .collect();
        let mut identities = Vec::with_capacity(handles.len());
        for handle in handles {
            if state.projects.contains_key(&handle) {
                identities.push(self.entry_identity(&mut state, &handle)?);
            }
        }
        Ok(identities)
    }

    /// tsgo `WatchedFiles`: the files dynamic mappers watch, sorted, once
    /// each.
    fn watched_files(&self) -> Result<Vec<String>, ProjectFailure> {
        let mut state = self.host.lock();
        let Some(lease) = state.leases.get(&self.key) else {
            return Ok(Vec::new());
        };
        let handles: Vec<String> = lease
            .entries
            .iter()
            .map(|(_, handle)| handle.clone())
            .collect();
        let mut files = Vec::new();
        for handle in handles {
            let Some(entry) = state.projects.get(&handle) else {
                continue;
            };
            if entry.mapper.manifest.dynamic_config {
                self.host.open_project_locked(&mut state, &handle)?;
            }
            files.extend(state.projects[&handle].watched_files.iter().cloned());
        }
        files.sort();
        files.dedup();
        Ok(files)
    }

    /// tsgo `Diagnostics`: the option diagnostics of the mapper projects
    /// opened so far, in the spec's order.
    fn diagnostics(&self) -> Vec<OptionDiagnostic> {
        let state = self.host.lock();
        let Some(lease) = state.leases.get(&self.key) else {
            return Vec::new();
        };
        lease
            .mappers
            .iter()
            .filter_map(|mapper| {
                lease
                    .entries
                    .iter()
                    .find(|(key, _)| *key == mapper_key(mapper))
            })
            .filter_map(|(_, handle)| state.projects.get(handle))
            .filter(|entry| entry.opened)
            .flat_map(|entry| entry.option_diagnostics.iter().cloned())
            .collect()
    }

    /// tsgo `Refresh`: closes the opened mapper projects, to be opened again
    /// on their next use.
    fn refresh(&self) -> Result<(), String> {
        let mut closing = Vec::new();
        {
            let mut state = self.host.lock();
            let Some(lease) = state.leases.get(&self.key) else {
                return Ok(());
            };
            let handles: Vec<String> = lease
                .entries
                .iter()
                .map(|(_, handle)| handle.clone())
                .collect();
            for handle in handles {
                let Some(entry) = state.projects.get_mut(&handle) else {
                    continue;
                };
                if !entry.opened {
                    continue;
                }
                entry.opened = false;
                let identity = entry.mapper.identity();
                if let Some(connection) = state
                    .conns
                    .get(&identity)
                    .and_then(|conn| conn.connection.clone())
                {
                    closing.push((identity, connection, handle));
                }
            }
        }
        let mut result = Ok(());
        for (identity, connection, handle) in closing {
            if let Err(error) = self.host.close_project(&identity, &connection, &handle) {
                result = Err(error);
            }
        }
        result
    }

    /// tsgo `Close`: releases this view; the last release closes the mapper
    /// projects and drops the processes nothing else retains.
    fn close(&self) -> Result<(), String> {
        if self.released.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        let mut closing = Vec::new();
        let mut identities = Vec::new();
        {
            let mut state = self.host.lock();
            let Some(lease) = state.leases.get_mut(&self.key) else {
                return Ok(());
            };
            lease.refs -= 1;
            if lease.refs != 0 {
                return Ok(());
            }
            let lease = state.leases.remove(&self.key).expect("found above");
            for (_, handle) in lease.entries {
                let Some(entry) = state.projects.remove(&handle) else {
                    continue;
                };
                let identity = entry.mapper.identity();
                if entry.opened {
                    if let Some(connection) = state
                        .conns
                        .get(&identity)
                        .and_then(|conn| conn.connection.clone())
                    {
                        closing.push((identity.clone(), connection, handle));
                    }
                }
                identities.push(identity);
            }
        }
        let mut result = Ok(());
        for (identity, connection, handle) in closing {
            if let Err(error) = self.host.close_project(&identity, &connection, &handle) {
                result = Err(error);
            }
        }
        self.host.release(&identities);
        result
    }
}

impl Drop for ProjectView {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

impl Host {
    /// tsgo `closeProject`.
    fn close_project(
        &self,
        identity: &str,
        connection: &Connection,
        handle: &str,
    ) -> Result<(), String> {
        let params = serde_json::to_string(&CloseProjectParams {
            project_handle: handle,
        })
        .expect("closeProject parameters serialize");
        let start = self.0.timing.start_request();
        let closed = connection.conn.call(METHOD_CLOSE_PROJECT, &params, None);
        self.0
            .timing
            .finish_request(identity, Operation::CloseProject, start);
        closed.map(|_| ()).map_err(|error| error.to_string())
    }
}

/// tsgo `Acquire`'s release function, on drop.
pub struct Acquisition {
    host: Host,
    identities: Vec<String>,
}

impl Drop for Acquisition {
    fn drop(&mut self) {
        self.host.release(&std::mem::take(&mut self.identities));
    }
}

/// tsgo `stderrLogger`: a mapper's standard error, line by line.
struct StderrLogger {
    prefix: String,
    logger: Logger,
    pending: Vec<u8>,
}

impl Write for StderrLogger {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(data);
        while let Some(index) = self.pending.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=index).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 1]);
            (self.logger)(&format!("{}{}", self.prefix, line.trim_end_matches('\r')));
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for StderrLogger {
    fn drop(&mut self) {
        if !self.pending.is_empty() {
            let line = String::from_utf8_lossy(&self.pending).into_owned();
            (self.logger)(&format!("{}{}", self.prefix, line.trim_end_matches('\r')));
        }
    }
}

/// tsgo `loggingProtocol`, at the stream: each framed message read or
/// written is logged as its JSON text.
struct LoggingStream<S> {
    stream: S,
    prefix: String,
    logger: Logger,
    buffer: Vec<u8>,
}

impl<S> LoggingStream<S> {
    fn new(stream: S, prefix: String, logger: Logger) -> Self {
        Self {
            stream,
            prefix,
            logger,
            buffer: Vec::new(),
        }
    }

    /// Logs every complete `Content-Length` frame in the buffer.
    fn log_frames(&mut self) {
        loop {
            let Some(header_end) = self
                .buffer
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
            else {
                return;
            };
            let header = String::from_utf8_lossy(&self.buffer[..header_end]);
            let Some(length) = header.lines().find_map(|line| {
                line.strip_prefix("Content-Length:")
                    .and_then(|length| length.trim().parse::<usize>().ok())
            }) else {
                self.buffer.clear();
                return;
            };
            let start = header_end + 4;
            if self.buffer.len() < start + length {
                return;
            }
            let message = String::from_utf8_lossy(&self.buffer[start..start + length]).into_owned();
            (self.logger)(&format!("{}{message}", self.prefix));
            self.buffer.drain(..start + length);
        }
    }
}

impl<S: Read> Read for LoggingStream<S> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let count = self.stream.read(buffer)?;
        self.buffer.extend_from_slice(&buffer[..count]);
        self.log_frames();
        Ok(count)
    }
}

impl<S: Write> Write for LoggingStream<S> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let count = self.stream.write(data)?;
        self.buffer.extend_from_slice(&data[..count]);
        self.log_frames();
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/content_mapper/host_tests.rs"]
mod host_tests;
