//! tsgo `lsp.Server` (`internal/lsp/server.go`): the language server's
//! loops and its lifecycle requests.
//!
//! Three loops run as in tsgo. The read loop answers `initialize` before the
//! server is initialized (and `ServerNotInitialized` to any other request),
//! routes the client's responses to the server's pending requests, cancels on
//! `$/cancelRequest`, and queues every other message. The dispatch loop runs
//! each handler's synchronous part in order; a handler may return work that
//! runs on its own thread. The write loop sends the outgoing messages.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tsc_compiler::locale::Locale;
use tsc_compiler::DEFAULT_LOAD_LIMITS;
use tsc_host::ipc::jsonrpc::{Reader, Writer};
use tsc_host::vfs::FileSystem;
use tsc_ls::lsutil::{parse_user_preferences, UserPreferences};
use tsc_ls::{LanguageService, LsError};
use tsc_lsproto::message::{decode_result, to_raw};
use tsc_lsproto::{
    self as lsproto, decode_params, method, parse_message, ErrorCode, HasTextDocumentUri, Id,
    LspError, Message, NoParams, Notification, Request, RequestMessage, ResponseError,
    ResponseMessage,
};
use tsc_program::{ConfigOptionBag, LibraryCatalog};
use tsc_project::{Project, Session, SessionOptions, Snapshot};

use crate::gofmt;
use crate::logger::{is_valid_log_verbosity, Logger};
use crate::queue::DynamicQueue;

mod cross_project;

use cross_project::CrossProjectStart;
use tsc_ls::SearchPosition;

/// Go's JSON encoder nests at most this deep (`jsontext`'s maximum depth).
const MAX_JSON_DEPTH: usize = 10_000;

/// The configuration sections the server asks for and watches.
const CONFIGURATION_SECTIONS: [&str; 4] = ["js/ts", "typescript", "javascript", "editor"];

/// tsgo `ServerOptions`.
pub struct ServerOptions {
    /// Where the server logs before the client starts initializing it.
    pub stderr: Box<dyn Write + Send>,
    /// The directory the server runs in (tsgo `Cwd`), the session's until
    /// the client names a workspace.
    pub cwd: String,
    /// tsgo `SetParentProcessID`: watches the process id the client gives
    /// in `initialize`.
    pub set_parent_process_id: Option<Box<dyn Fn(i32) + Send + Sync>>,
    /// tsgo `FS`: the files the session reads.
    pub fs: Arc<dyn FileSystem>,
    /// tsgo `DefaultLibraryPath`: the directory of the standard library.
    pub default_library_path: String,
}

/// Why a handler did not answer (tsgo's handler errors: `io.EOF` for
/// `exit`, `context.Canceled`, and the others).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The client sent `exit`.
    Exit,
    /// The request was cancelled.
    Cancelled,
    /// tsgo `ls.ErrNeedsAutoImports`: the auto-import requests' handler
    /// prepares the registry and runs the request again.
    NeedsAutoImports,
    Error(LspError),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exit => formatter.write_str("EOF"),
            Self::Cancelled => formatter.write_str("context canceled"),
            Self::NeedsAutoImports => formatter.write_str("completion list needs auto imports"),
            Self::Error(error) => formatter.write_str(&error.message),
        }
    }
}

impl From<LspError> for Failure {
    fn from(error: LspError) -> Self {
        Self::Error(error)
    }
}

impl From<ErrorCode> for Failure {
    fn from(code: ErrorCode) -> Self {
        Self::Error(code.into())
    }
}

/// What a request runs in (tsgo's request context): its cancellation, the
/// locale and its id.
#[derive(Clone)]
pub struct RequestContext {
    cancelled: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    /// tsgo `context.WithTimeout`'s deadline.
    deadline: Option<Instant>,
    pub locale: Locale,
    pub request_id: Option<Id>,
}

impl RequestContext {
    /// tsgo `ctx.Err() != nil`: the request was cancelled or the server
    /// stopped.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst) || self.stopped.load(Ordering::SeqCst)
    }

    /// tsgo `context.DeadlineExceeded`.
    fn deadline_exceeded(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    /// tsgo's context check: a cancelled request ends.
    pub fn check(&self) -> Result<(), Failure> {
        if self.is_cancelled() {
            return Err(Failure::Cancelled);
        }
        Ok(())
    }
}

/// A handler's work after its synchronous part (tsgo's `asyncWork`).
pub type AsyncWork = Box<dyn FnOnce() -> Result<(), Failure> + Send>;

type Handler = Box<
    dyn Fn(&Arc<Server>, &RequestContext, &RequestMessage) -> Result<Option<AsyncWork>, Failure>
        + Send
        + Sync,
>;

/// Why the server stopped (tsgo's first error in `Run`'s group): the end of
/// the client's input ends it without an error.
enum StopReason {
    EndOfInput,
    Error(String),
}

/// What the client told the server when it initialized it.
#[derive(Default)]
struct ServerState {
    initialize_params: Option<lsproto::InitializeParams>,
    initialization_options: lsproto::InitializationOptions,
    client_capabilities: lsproto::ResolvedClientCapabilities,
    position_encoding: lsproto::PositionEncodingKind,
    locale: Locale,
    init_locale: Locale,
    flake_logging: lsproto::DiagnosticFlakeLogLevel,
    watch_enabled: bool,
    telemetry_enabled: bool,
    current_directory: String,
    /// tsgo `compilerOptionsForInferredProjects`.
    compiler_options_for_inferred_projects: Option<Arc<ConfigOptionBag>>,
}

/// tsgo `Server`.
pub struct Server {
    requests: DynamicQueue<RequestMessage>,
    outgoing: DynamicQueue<Message>,
    pending_client_requests: Mutex<HashMap<Id, Arc<AtomicBool>>>,
    pending_server_requests: Mutex<HashMap<Id, mpsc::Sender<ResponseMessage>>>,
    client_seq: AtomicI32,
    pub(crate) init_started: AtomicBool,
    /// tsgo `session`: created by `initialized`.
    session: OnceLock<Session>,
    stopped: Arc<AtomicBool>,
    stop_reason: Mutex<Option<StopReason>>,
    state: Mutex<ServerState>,
    pub(crate) logger: Logger,
    init_complete: (Mutex<bool>, Condvar),
    pub(crate) stderr: Mutex<Box<dyn Write + Send>>,
    cwd: String,
    set_parent_process_id: Option<Box<dyn Fn(i32) + Send + Sync>>,
    fs: Arc<dyn FileSystem>,
    default_library_path: String,
    /// tsgo `watchers`: the IDs of the watches the client registered.
    watchers: Mutex<HashSet<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Server {
    /// tsgo `NewServer`.
    pub fn new(options: ServerOptions) -> Arc<Self> {
        Arc::new(Self {
            requests: DynamicQueue::new(),
            outgoing: DynamicQueue::new(),
            pending_client_requests: Mutex::default(),
            pending_server_requests: Mutex::default(),
            client_seq: AtomicI32::new(0),
            init_started: AtomicBool::new(false),
            session: OnceLock::new(),
            stopped: Arc::new(AtomicBool::new(false)),
            stop_reason: Mutex::new(None),
            state: Mutex::default(),
            logger: Logger::default(),
            init_complete: (Mutex::new(false), Condvar::new()),
            stderr: Mutex::new(options.stderr),
            cwd: options.cwd,
            set_parent_process_id: options.set_parent_process_id,
            fs: options.fs,
            default_library_path: options.default_library_path,
            watchers: Mutex::default(),
        })
    }

    /// tsgo `Run`: serves the client until its input ends (no error), it
    /// sends `exit` or the server is stopped (`context canceled`), or a
    /// loop fails.
    pub fn run(
        self: &Arc<Self>,
        input: Box<dyn Read + Send>,
        output: Box<dyn Write + Send>,
    ) -> Result<(), String> {
        let writer = {
            let server = Arc::clone(self);
            std::thread::Builder::new()
                .name("tsc-rs-lsp-write".to_owned())
                .spawn(move || {
                    if let Err(error) = server.write_loop(output) {
                        server.stop_with(StopReason::Error(error));
                    }
                })
                .map_err(|error| error.to_string())?
        };
        // tsgo does not wait for the read loop either: it blocks on the
        // input and cannot be cancelled.
        let reader = Arc::clone(self);
        std::thread::Builder::new()
            .name("tsc-rs-lsp-read".to_owned())
            .spawn(move || {
                let reason = match reader.read_loop(input) {
                    Ok(()) => StopReason::EndOfInput,
                    Err(error) => StopReason::Error(error),
                };
                reader.stop_with(reason);
            })
            .map_err(|error| error.to_string())?;
        self.dispatch_loop();
        self.stop();
        let _ = writer.join();
        match lock(&self.stop_reason).take() {
            Some(StopReason::Error(error)) => Err(error),
            _ => Ok(()),
        }
    }

    /// Whether the server stopped (tsgo's cancelled lifecycle context).
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }

    /// tsgo's cancelled lifecycle context: every loop ends.
    pub fn stop(&self) {
        self.stop_with(StopReason::Error("context canceled".to_owned()));
    }

    fn stop_with(&self, reason: StopReason) {
        lock(&self.stop_reason).get_or_insert(reason);
        self.stopped.store(true, Ordering::SeqCst);
        self.requests.close();
        self.outgoing.close();
        let (complete, ready) = &self.init_complete;
        drop(lock(complete));
        ready.notify_all();
    }

    /// tsgo `InitComplete`: waits until `initialized` is handled (or the
    /// server stopped).
    pub fn wait_init_complete(&self) {
        let (complete, ready) = &self.init_complete;
        let mut done = lock(complete);
        while !*done && !self.stopped.load(Ordering::SeqCst) {
            done = ready
                .wait_timeout(done, Duration::from_millis(50))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    fn read_loop(self: &Arc<Self>, input: Box<dyn Read + Send>) -> Result<(), String> {
        let mut reader = Reader::new(input);
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                return Err("context canceled".to_owned());
            }
            let data = match reader.read() {
                Ok(Some(data)) => data,
                Ok(None) => return Ok(()),
                Err(error) => return Err(error.message().to_owned()),
            };
            let message = match parse_message(&data) {
                Ok(message) => message,
                Err(error) => {
                    self.send_error(None, &Failure::Error(error))?;
                    continue;
                }
            };
            let initialized = lock(&self.state).initialize_params.is_some();
            if !initialized {
                if let Message::Request(request) = &message {
                    if request.method == method::INITIALIZE {
                        let params = match decode_params::<lsproto::InitializeParams>(request) {
                            Ok(params) => params,
                            Err(error) => {
                                self.send_error(request.id.clone(), &Failure::Error(error))?;
                                continue;
                            }
                        };
                        let response = self
                            .handle_initialize(params)
                            .map_err(|error| error.to_string())?;
                        self.send_result(request.id.clone(), &response)?;
                    } else {
                        self.send_error(
                            request.id.clone(),
                            &Failure::from(ErrorCode::SERVER_NOT_INITIALIZED),
                        )?;
                    }
                    continue;
                }
            }
            match message {
                Message::Response(response) => {
                    let Some(id) = response.id.clone() else {
                        continue;
                    };
                    if let Some(pending) = lock(&self.pending_server_requests).remove(&id) {
                        let _ = pending.send(response);
                    }
                }
                Message::Request(request) | Message::Notification(request) => {
                    if request.method == method::CANCEL_REQUEST {
                        if let Ok(params) = decode_params::<lsproto::CancelParams>(&request) {
                            self.cancel_request(&params.id);
                        }
                    } else if !self.requests.put(request) {
                        return Err("context canceled".to_owned());
                    }
                }
            }
        }
    }

    /// tsgo `cancelRequest`: only a request the dispatch loop has taken.
    fn cancel_request(&self, id: &lsproto::IntegerOrString) {
        let id = Id::from_integer_or_string(id);
        if let Some(cancelled) = lock(&self.pending_client_requests).remove(&id) {
            cancelled.store(true, Ordering::SeqCst);
        }
    }

    fn dispatch_loop(self: &Arc<Self>) {
        while let Some(request) = self.requests.get() {
            let cancelled = Arc::new(AtomicBool::new(false));
            if let Some(id) = &request.id {
                lock(&self.pending_client_requests).insert(id.clone(), Arc::clone(&cancelled));
            }
            let context = RequestContext {
                cancelled,
                stopped: Arc::clone(&self.stopped),
                deadline: None,
                locale: self.locale(),
                request_id: request.id.clone(),
            };
            match self.handle_request_or_notification(&context, &request) {
                Err(failure) => {
                    self.handle_failure(request.id.clone(), failure);
                    self.remove_request(&request);
                }
                Ok(Some(work)) => {
                    let server = Arc::clone(self);
                    let spawned = std::thread::Builder::new()
                        .name("tsc-rs-lsp-request".to_owned())
                        .stack_size(tsc_program::WORKER_STACK_BYTES)
                        .spawn(move || {
                            if let Err(failure) = work() {
                                server.handle_failure(request.id.clone(), failure);
                            }
                            server.remove_request(&request);
                        });
                    if let Err(error) = spawned {
                        self.logger
                            .error(self, &format!("cannot start a request thread: {error}"));
                    }
                }
                Ok(None) => self.remove_request(&request),
            }
        }
    }

    /// tsgo's `handleError` in the dispatch loop.
    fn handle_failure(&self, id: Option<Id>, failure: Failure) {
        let failure = match failure {
            Failure::Exit => return self.stop(),
            Failure::Cancelled => Failure::from(ErrorCode::REQUEST_CANCELLED),
            failure => failure,
        };
        if self.send_error(id, &failure).is_err() {
            self.stop();
        }
    }

    fn remove_request(&self, request: &RequestMessage) {
        if let Some(id) = &request.id {
            lock(&self.pending_client_requests).remove(id);
        }
    }

    fn write_loop(&self, output: Box<dyn Write + Send>) -> Result<(), String> {
        let mut writer = Writer::new(output);
        while let Some(message) = self.outgoing.get() {
            let data = serde_json::to_vec(&message)
                .map_err(|error| error.to_string())
                .and_then(|data| {
                    if exceeds_depth(&data, MAX_JSON_DEPTH) {
                        return Err("jsontext: exceeded max depth".to_owned());
                    }
                    Ok(data)
                });
            let data = match data {
                Ok(data) => data,
                Err(error) => {
                    let error = format!("failed to marshal message: {error}");
                    // tsgo `messageMarshalError`: a response that cannot be
                    // written fails only its request.
                    if let Message::Response(ResponseMessage {
                        id: Some(id),
                        error: None,
                        ..
                    }) = &message
                    {
                        self.logger.error(
                            self,
                            &format!("failed to marshal response for request {id}: {error}"),
                        );
                        self.send_error(Some(id.clone()), &LspError::internal(error).into())?;
                        continue;
                    }
                    return Err(format!("failed to write message: {error}"));
                }
            };
            writer
                .write(&data)
                .map_err(|error| format!("failed to write message: {}", error.message()))?;
        }
        Ok(())
    }

    /// tsgo `handleRequestOrNotification`: the handler's synchronous part,
    /// its log, and its work to run after.
    fn handle_request_or_notification(
        self: &Arc<Self>,
        context: &RequestContext,
        request: &RequestMessage,
    ) -> Result<Option<AsyncWork>, Failure> {
        let Some(handler) = handlers().get(request.method.as_str()) else {
            self.logger
                .warn(self, &format!("unknown method '{}'", request.method));
            if request.id.is_some() {
                self.send_error(
                    request.id.clone(),
                    &Failure::from(ErrorCode::INVALID_REQUEST),
                )
                .map_err(LspError::internal)?;
            }
            return Ok(None);
        };
        let start = Instant::now();
        let id = request
            .id
            .as_ref()
            .map(|id| format!(" ({id})"))
            .unwrap_or_default();
        match handler(self, context, request) {
            Err(failure) => {
                self.logger.error(
                    self,
                    &format!("error handling method '{}'{id}: {failure}", request.method),
                );
                Err(failure)
            }
            Ok(Some(work)) => {
                let server = Arc::clone(self);
                let method = request.method.clone();
                Ok(Some(Box::new(move || {
                    let result = work();
                    let elapsed = tsc_host::go_time::duration_string(start.elapsed());
                    if result.is_err() {
                        server.logger.info(
                            &server,
                            &format!("error handling method '{method}'{id} in {elapsed}"),
                        );
                    } else if !server.logger.is_tracing() {
                        server.logger.info(
                            &server,
                            &format!("handled method '{method}'{id} in {elapsed}"),
                        );
                    }
                    result
                })))
            }
            Ok(None) => {
                if !self.logger.is_tracing() {
                    let elapsed = tsc_host::go_time::duration_string(start.elapsed());
                    self.logger.info(
                        self,
                        &format!("handled method '{}'{id} in {elapsed}", request.method),
                    );
                }
                Ok(None)
            }
        }
    }

    // -- sending -------------------------------------------------------------

    /// tsgo `send`: queues a message; an error once the server stopped.
    pub fn send(&self, message: Message) -> Result<(), String> {
        if self.outgoing.put(message) {
            Ok(())
        } else {
            Err("context canceled".to_owned())
        }
    }

    /// tsgo `sendResult`.
    pub fn send_result<T: Serialize>(&self, id: Option<Id>, result: &T) -> Result<(), String> {
        let result =
            to_raw(result).map_err(|error| format!("failed to marshal message: {error}"))?;
        self.send(Message::Response(ResponseMessage {
            id,
            result: Some(result),
            error: None,
        }))
    }

    /// tsgo `sendError`: a notification gets no response, except for a
    /// message that could not be read.
    fn send_error(&self, id: Option<Id>, failure: &Failure) -> Result<(), String> {
        let error = match failure {
            Failure::Error(error) => error.clone(),
            other => LspError::internal(other.to_string()),
        };
        if id.is_none() && error.code != ErrorCode::INVALID_REQUEST {
            self.logger
                .error(self, &format!("error handling notification: {error}"));
            return Ok(());
        }
        self.send(Message::Response(ResponseMessage {
            id,
            result: None,
            error: Some(ResponseError {
                code: error.code.0,
                message: error.message,
                data: None,
            }),
        }))
    }

    /// tsgo `sendNotification`.
    pub fn send_notification<N: Notification>(&self, params: &N::Params) -> Result<(), String>
    where
        N::Params: 'static,
    {
        let message =
            RequestMessage::new(None, N::METHOD, params).map_err(|error| error.to_string())?;
        self.send(Message::Notification(message))
    }

    fn next_client_request_id(&self) -> Id {
        Id::String(format!(
            "ts{}",
            self.client_seq.fetch_add(1, Ordering::SeqCst) + 1
        ))
    }

    /// tsgo `sendClientRequest`: a request to the client, waiting for its
    /// response. The read loop routes the response, so a handler's
    /// synchronous part may wait for one.
    pub fn send_client_request<R: Request>(
        &self,
        context: &RequestContext,
        params: &R::Params,
    ) -> Result<R::Response, Failure>
    where
        R::Params: 'static,
    {
        let id = self.next_client_request_id();
        let request = RequestMessage::new(Some(id.clone()), R::METHOD, params)
            .map_err(|error| LspError::internal(error.to_string()))?;
        let (sender, receiver) = mpsc::channel();
        lock(&self.pending_server_requests).insert(id.clone(), sender);
        let response = self.send(Message::Request(request)).and_then(|()| loop {
            match receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(response) => break Ok(response),
                Err(mpsc::RecvTimeoutError::Timeout) if context.deadline_exceeded() => {
                    break Err("context deadline exceeded".to_owned())
                }
                Err(mpsc::RecvTimeoutError::Timeout) if !context.is_cancelled() => {}
                Err(_) => break Err("context canceled".to_owned()),
            }
        });
        lock(&self.pending_server_requests).remove(&id);
        let response = match response {
            Ok(response) => response,
            Err(_) if context.is_cancelled() => return Err(Failure::Cancelled),
            Err(error) => return Err(LspError::internal(error).into()),
        };
        if let Some(error) = response.error {
            return Err(LspError::internal(format!("request failed: {error}")).into());
        }
        decode_result::<R::Response>(response.result.as_ref())
            .map_err(|error| LspError::internal(error).into())
    }

    /// tsgo `sendClientRequestFireAndForget`: the response is ignored.
    pub fn send_client_request_fire_and_forget<R: Request>(
        &self,
        params: &R::Params,
    ) -> Result<(), String>
    where
        R::Params: 'static,
    {
        let request = RequestMessage::new(Some(self.next_client_request_id()), R::METHOD, params)
            .map_err(|error| error.to_string())?;
        self.send(Message::Request(request))
    }

    // -- state ---------------------------------------------------------------

    /// tsgo `GetLocale`.
    pub fn locale(&self) -> Locale {
        lock(&self.state).locale
    }

    /// The position encoding the client and the server agreed on.
    pub fn position_encoding(&self) -> lsproto::PositionEncodingKind {
        lock(&self.state).position_encoding.clone()
    }

    /// The resolved capabilities of the client.
    pub fn client_capabilities(&self) -> lsproto::ResolvedClientCapabilities {
        lock(&self.state).client_capabilities.clone()
    }

    /// The session's directory: the workspace's, or the server's.
    pub fn current_directory(&self) -> String {
        lock(&self.state).current_directory.clone()
    }

    /// The preferences the client configured (once it initialized).
    pub fn user_preferences(&self) -> Option<UserPreferences> {
        self.session().map(Session::config)
    }

    /// tsgo `session`: the projects, once `initialized` created them.
    /// tsgo `SetCompilerOptionsForInferredProjects`: the inferred projects'
    /// options, which its tests set before initializing the server.
    pub fn set_compiler_options_for_inferred_projects(&self, options: ConfigOptionBag) {
        let options = Arc::new(options);
        lock(&self.state).compiler_options_for_inferred_projects = Some(Arc::clone(&options));
        if let Some(session) = self.session.get() {
            session.did_change_compiler_options_for_inferred_projects(options);
        }
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.get()
    }

    /// The session, which every handler but `initialize`'s and
    /// `initialized`'s has.
    /// tsgo `NewLanguageService` over the project's program in the
    /// snapshot, with the snapshot's converters and preferences and the
    /// request's locale.
    fn with_language_service<T>(
        &self,
        context: &RequestContext,
        snapshot: &Arc<Snapshot>,
        project: &Project,
        handle: impl FnOnce(&mut LanguageService<'_>) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        let Some(program) = project.program() else {
            return Err(
                LspError::internal(format!("project {} has no program", project.id())).into(),
            );
        };
        let converters = snapshot.converters(self.position_encoding());
        let preferences = snapshot.user_preferences().clone();
        let capabilities = self.client_capabilities();
        program.with_live(|live| {
            let mut service = LanguageService::new(
                live,
                snapshot.as_ref(),
                &converters,
                &preferences,
                &capabilities,
                context.locale.messages(),
            )
            .with_project_id(project.id().as_str());
            handle(&mut service)
        })
    }

    fn initialized_session(&self) -> &Session {
        self.session
            .get()
            .expect("the handlers run once the session exists")
    }

    // -- lifecycle handlers ----------------------------------------------------

    /// tsgo `handleInitialize`.
    fn handle_initialize(
        &self,
        params: lsproto::InitializeParams,
    ) -> Result<lsproto::InitializeResponse, Failure> {
        let mut state = lock(&self.state);
        if state.initialize_params.is_some() {
            return Err(ErrorCode::INVALID_REQUEST.into());
        }
        self.init_started.store(true, Ordering::SeqCst);
        // The spec types initializationOptions as nullable; both null and an
        // absent value are empty options.
        state.initialization_options = match &params.initialization_options {
            Some(lsproto::InitializationOptionsOrNull::InitializationOptions(options)) => {
                options.clone()
            }
            _ => lsproto::InitializationOptions::default(),
        };
        if let Some(verbosity) = state.initialization_options.log_verbosity {
            if is_valid_log_verbosity(verbosity) {
                self.logger.set_verbosity(verbosity);
            }
        }
        if let Some(level) = state.initialization_options.track_flaky_diagnostics {
            state.flake_logging = level;
        }
        state.client_capabilities = params.capabilities.resolve();
        let capabilities = marshal_indent(&state.client_capabilities);
        drop(state);
        self.logger.info(
            self,
            &format!("Resolved client capabilities: {capabilities}"),
        );
        let mut state = lock(&self.state);
        state.position_encoding = if state
            .client_capabilities
            .general
            .position_encodings
            .contains(&lsproto::PositionEncodingKind::UTF8)
        {
            lsproto::PositionEncodingKind::UTF8
        } else {
            lsproto::PositionEncodingKind::UTF16
        };
        if let Some(locale) = &params.locale {
            state.locale = Locale::parse(locale).unwrap_or_default();
        }
        state.init_locale = state.locale;
        if let (Some(watch), lsproto::IntegerOrNull::Integer(process_id)) =
            (&self.set_parent_process_id, &params.process_id)
        {
            watch(*process_id);
        }
        let response = crate::capabilities::initialize_result(
            &state.position_encoding,
            &state.client_capabilities,
        );
        state.initialize_params = Some(params);
        Ok(response)
    }

    /// tsgo `handleInitialized`: the session's directory and the client's
    /// configuration.
    fn handle_initialized(
        self: &Arc<Self>,
        context: &RequestContext,
        _params: lsproto::InitializedParams,
    ) -> Result<(), Failure> {
        let (
            dynamic_watch_registration,
            telemetry,
            disable_push_diagnostics,
            run_external_code,
            workspace_folders,
            root_uri,
            root_path,
        ) = {
            let state = lock(&self.state);
            let params = state.initialize_params.as_ref();
            let folders = params.and_then(|params| match &params.workspace_folders {
                Some(lsproto::WorkspaceFoldersOrNull::WorkspaceFolders(folders)) => {
                    Some(folders.clone())
                }
                _ => None,
            });
            let root_uri = params.and_then(|params| match &params.root_uri {
                lsproto::DocumentUriOrNull::DocumentUri(uri) => Some(uri.clone()),
                _ => None,
            });
            let root_path = params.and_then(|params| match &params.root_path {
                Some(lsproto::StringOrNull::String(path)) => Some(path.clone()),
                _ => None,
            });
            (
                state
                    .client_capabilities
                    .workspace
                    .did_change_watched_files
                    .dynamic_registration,
                state
                    .initialization_options
                    .enable_telemetry
                    .unwrap_or_default(),
                state
                    .initialization_options
                    .disable_push_diagnostics
                    .unwrap_or_default(),
                state
                    .initialization_options
                    .run_external_code
                    .unwrap_or_default(),
                folders.filter(|_| state.client_capabilities.workspace.workspace_folders),
                root_uri,
                root_path,
            )
        };
        if dynamic_watch_registration {
            self.logger.log(
                self,
                "file watching: using LSP client-side watching (client supports dynamic registration)",
            );
            lock(&self.state).watch_enabled = true;
        } else {
            self.logger.log(
                self,
                "file watching: disabled (client lacks dynamic watch registration and builtin watcher backend is not fast-recursive)",
            );
        }
        let file_name = |uri: &str| {
            tsc_program::uri::uri_to_file_name(uri)
                .ok()
                .filter(|name| !name.is_empty())
        };
        let mut cwd = self.cwd.clone();
        if let Some(folders) = workspace_folders.filter(|folders| folders.len() == 1) {
            if let Some(name) = file_name(&folders[0].uri.0) {
                cwd = name;
            }
        } else if let Some(uri) = root_uri {
            if let Some(name) = file_name(uri.as_str()) {
                cwd = name;
            }
        } else if let Some(path) = root_path {
            // tsgo `tspath.PathIsAbsolute`.
            if tsc_program::path_root_parts(tsc_diagnostics::JsStr::from_str(&path)).is_some() {
                cwd = tsc_program::uri::rooted_path_from_absolute(&path);
            }
        }
        let (position_encoding, watch_enabled, client_capabilities) = {
            let mut state = lock(&self.state);
            state.telemetry_enabled = telemetry;
            state.current_directory = cwd.clone();
            (
                state.position_encoding.clone(),
                state.watch_enabled,
                state.client_capabilities.clone(),
            )
        };
        let host = Arc::new(SessionHost(Arc::downgrade(self)));
        let session = Session::new(
            SessionOptions {
                current_directory: cwd,
                library_catalog: LibraryCatalog::typescript_7_1(Path::new(
                    &self.default_library_path,
                )),
                load_limits: DEFAULT_LOAD_LIMITS,
                run_external_code,
                position_encoding,
                watch_enabled,
                logging_enabled: true,
                push_diagnostics_enabled: !disable_push_diagnostics,
                debounce_delay: Duration::from_millis(500),
                client_capabilities,
            },
            Arc::clone(&self.fs),
            Some(Arc::clone(&host) as Arc<dyn tsc_project::Client>),
            Some(host as Arc<dyn tsc_project::Logger>),
            None,
        );
        if self.session.set(session).is_err() {
            return Err(ErrorCode::INVALID_REQUEST.into());
        }
        let preferences = self.request_configuration(context)?;
        self.initialized_session()
            .initialize_with_user_config(preferences);
        let registration = lsproto::RegistrationParams {
            registrations: vec![lsproto::Registration {
                id: "typescript-config-watch-id".to_owned(),
                register_options: Some(lsproto::RegisterOptions::WorkspaceDidChangeConfiguration(
                    lsproto::DidChangeConfigurationRegistrationOptions {
                        section: Some(lsproto::StringOrStrings::Strings(
                            CONFIGURATION_SECTIONS.map(str::to_owned).to_vec(),
                        )),
                    },
                )),
            }],
        };
        self.send_client_request::<lsproto::ClientRegisterCapabilityInfo>(context, &registration)
            .map_err(|failure| {
                prefix_failure(failure, "failed to register configuration change watcher")
            })?;
        let options = lock(&self.state)
            .compiler_options_for_inferred_projects
            .clone();
        if let Some(options) = options {
            self.initialized_session()
                .did_change_compiler_options_for_inferred_projects(options);
        }
        let (complete, ready) = &self.init_complete;
        *lock(complete) = true;
        ready.notify_all();
        Ok(())
    }

    /// tsgo `RequestConfiguration`: the preferences of the `js/ts`,
    /// `typescript`, `javascript` and `editor` sections from
    /// `workspace/configuration`, or without the configuration capability of
    /// the initialization options' preferences as `js/ts`.
    fn request_configuration(&self, context: &RequestContext) -> Result<UserPreferences, Failure> {
        let (configuration, preferences) = {
            let state = lock(&self.state);
            (
                state.client_capabilities.workspace.configuration,
                state
                    .initialization_options
                    .user_preferences
                    .clone()
                    .filter(|preferences| !preferences.is_null()),
            )
        };
        let mut sections = serde_json::Map::new();
        if !configuration {
            if let Some(preferences) = preferences {
                self.logger.log(
                    self,
                    &format!(
                        "received formatting options from initialization: {}\n{}",
                        gofmt::type_name(&preferences),
                        gofmt::format_any(&preferences)
                    ),
                );
                if preferences.is_object() {
                    sections.insert("js/ts".to_owned(), preferences);
                    return Ok(parse_user_preferences(&sections));
                }
            }
            return Ok(UserPreferences::new_default());
        }
        let items = CONFIGURATION_SECTIONS
            .iter()
            .map(|section| lsproto::ConfigurationItem {
                section: Some((*section).to_owned()),
                ..lsproto::ConfigurationItem::default()
            })
            .collect();
        let configs = self
            .send_client_request::<lsproto::WorkspaceConfigurationInfo>(
                context,
                &lsproto::ConfigurationParams { items },
            )
            .map_err(|failure| prefix_failure(failure, "configure request failed"))?;
        for (section, config) in CONFIGURATION_SECTIONS.iter().zip(configs) {
            sections.insert((*section).to_owned(), config);
        }
        let section = |name: &str| gofmt::format_any(sections.get(name).unwrap_or(&Value::Null));
        self.logger.log(
            self,
            &format!(
                "received options from workspace/configuration request:\njs/ts: {}\n\ntypescript: {}\n\njavascript: {}\n\neditor: {}\n",
                section("js/ts"),
                section("typescript"),
                section("javascript"),
                section("editor")
            ),
        );
        Ok(parse_user_preferences(&sections))
    }

    /// tsgo `handleDidChangeWorkspaceConfiguration`: the preferences of the
    /// settings the client sent (an object).
    fn handle_did_change_workspace_configuration(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::DidChangeConfigurationParams,
    ) -> Result<(), Failure> {
        if let Value::Object(settings) = &params.settings {
            self.initialized_session()
                .configure(parse_user_preferences(settings));
        }
        Ok(())
    }

    /// tsgo `handleDocumentDiagnostic` (without the flaky diagnostics
    /// logging, which reruns the request after an emit).
    fn handle_document_diagnostic(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DocumentDiagnosticParams,
    ) -> Result<lsproto::DocumentDiagnosticResponse, Failure> {
        service
            .provide_diagnostics(&params.text_document.uri)
            .map_err(language_service_failure)
    }

    /// tsgo `handleHover`.
    fn handle_hover(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::HoverParams,
    ) -> Result<lsproto::HoverResponse, Failure> {
        let hover = service
            .provide_hover(&params)
            .map_err(language_service_failure)?;
        Ok(match hover {
            Some(hover) => lsproto::HoverOrNull::Hover(hover),
            None => lsproto::HoverOrNull::Null,
        })
    }

    /// tsgo `handleDocumentFormat`.
    fn handle_document_format(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DocumentFormattingParams,
    ) -> Result<lsproto::DocumentFormattingResponse, Failure> {
        service
            .provide_format_document(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleDocumentRangeFormat`.
    fn handle_document_range_format(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DocumentRangeFormattingParams,
    ) -> Result<lsproto::DocumentRangeFormattingResponse, Failure> {
        service
            .provide_format_document_range(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleCodeAction`.
    fn handle_code_action(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::CodeActionParams,
    ) -> Result<lsproto::CodeActionResponse, Failure> {
        service
            .provide_code_actions(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleDocumentOnTypeFormat`.
    fn handle_document_on_type_format(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DocumentOnTypeFormattingParams,
    ) -> Result<lsproto::DocumentOnTypeFormattingResponse, Failure> {
        service
            .provide_format_document_on_type(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleCompletion`.
    fn handle_completion(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::CompletionParams,
    ) -> Result<lsproto::CompletionResponse, Failure> {
        service
            .provide_completion(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleCompletionItemResolve`: the item's file's default
    /// project's language service resolves it, in the synchronous part
    /// (tsgo's `registerRequestHandler`). A panic answers tsgo's `recover`
    /// error.
    fn handle_completion_item_resolve(
        self: &Arc<Self>,
        context: &RequestContext,
        params: lsproto::CompletionItem,
        _request: &RequestMessage,
    ) -> Result<lsproto::CompletionResolveResponse, Failure> {
        let Some(data) = params.data.as_ref() else {
            return Err(LspError::internal("completion item data is nil").into());
        };
        let file_name = &data.file_name;
        let rooted = file_name.starts_with(['/', '\\'])
            || file_name.starts_with("^/")
            || file_name.contains("://")
            || file_name.as_bytes().get(1) == Some(&b':')
                && file_name.as_bytes()[0].is_ascii_alphabetic();
        if !rooted {
            return Err(
                LspError::internal("completion item data fileName must be absolute").into(),
            );
        }
        let uri = lsproto::DocumentUri(tsc_program::uri::file_name_to_document_uri(file_name));
        let (snapshot, project) = self
            .initialized_session()
            .get_snapshot_and_default_project(&uri)
            .map_err(|error| LspError::internal(error.to_string()))?;
        let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.with_language_service(context, &snapshot, &project, |service| {
                service
                    .resolve_completion_item(params)
                    .map_err(language_service_failure)
            })
        }));
        match response {
            Ok(response) => response,
            Err(panic) => {
                let detail = panic
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                    .unwrap_or_default();
                Err(LspError::wrap(
                    ErrorCode::INTERNAL_ERROR,
                    format!(
                        "panic handling request {}: {detail}",
                        lsproto::CompletionItemResolveInfo::METHOD
                    ),
                )
                .into())
            }
        }
    }

    /// tsgo `handleSelectionRange`.
    fn handle_selection_range(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::SelectionRangeParams,
    ) -> Result<lsproto::SelectionRangeResponse, Failure> {
        service
            .provide_selection_ranges(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleFoldingRange`.
    fn handle_folding_range(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::FoldingRangeParams,
    ) -> Result<lsproto::FoldingRangeResponse, Failure> {
        service
            .provide_folding_ranges(&params.text_document.uri)
            .map_err(language_service_failure)
    }

    /// tsgo `handleDocumentSymbol`.
    fn handle_document_symbol(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DocumentSymbolParams,
    ) -> Result<lsproto::DocumentSymbolResponse, Failure> {
        service
            .provide_document_symbols(&params.text_document.uri)
            .map_err(language_service_failure)
    }

    /// tsgo `handleSignatureHelp`.
    fn handle_signature_help(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::SignatureHelpParams,
    ) -> Result<lsproto::SignatureHelpResponse, Failure> {
        service
            .provide_signature_help(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `handleDefinition`.
    fn handle_definition(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DefinitionParams,
    ) -> Result<lsproto::DefinitionResponse, Failure> {
        service
            .provide_definition(&params.text_document.uri, &params.position)
            .map_err(language_service_failure)
    }

    /// tsgo `handleTypeDefinition`.
    fn handle_type_definition(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::TypeDefinitionParams,
    ) -> Result<lsproto::TypeDefinitionResponse, Failure> {
        service
            .provide_type_definition(&params.text_document.uri, &params.position)
            .map_err(language_service_failure)
    }

    /// tsgo `ProvideReferences` across projects.
    fn handle_references(
        self: &Arc<Self>,
        context: &RequestContext,
        start: &CrossProjectStart,
        params: lsproto::ReferenceParams,
    ) -> Result<lsproto::ReferencesResponse, Failure> {
        let include_declaration = params.context.include_declaration;
        let response = self.search_across_projects(
            context,
            start,
            &params.text_document.uri,
            &params.position,
            SearchPosition::Lsp(params.position.clone()),
            |service, uri, at, with_definition| {
                service.search_references(uri, at, include_declaration, with_definition)
            },
            tsc_ls::combine_references,
        )?;
        Ok(response.unwrap_or(lsproto::LocationsOrNull::Null))
    }

    /// tsgo `handlePrepareRename`: the span and placeholder, else the
    /// reason as a failed request.
    fn handle_prepare_rename(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::PrepareRenameParams,
    ) -> Result<lsproto::PrepareRenameResponse, Failure> {
        let info = service
            .get_rename_info("", &params.text_document.uri, &params.position)
            .map_err(language_service_failure)?;
        if !info.can_rename {
            // tsgo `userFacingRequestFailedError`.
            return Err(LspError {
                code: ErrorCode::REQUEST_FAILED,
                message: info.localized_error_message,
            }
            .into());
        }
        Ok(
            lsproto::RangeOrPrepareRenamePlaceholderOrPrepareRenameDefaultBehaviorOrNull::PrepareRenamePlaceholder(
                lsproto::PrepareRenamePlaceholder {
                    range: info.trigger_span,
                    placeholder: info.display_name,
                },
            ),
        )
    }

    /// tsgo `handleRename`: a module specifier's rename renames its file —
    /// the file's rename for a client that sends `workspace/willRenameFiles`
    /// for it, else the file's rename with its edits.
    fn handle_rename(
        self: &Arc<Self>,
        context: &RequestContext,
        params: lsproto::RenameParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::RenameResponse, Failure> {
        let start = self
            .cross_project_start(&params.text_document.uri)
            .map_err(|error| LspError::internal(error.to_string()))?;
        let info = self.with_language_service(
            context,
            &start.snapshot,
            &start.default_project,
            |service| {
                service
                    .get_rename_info(
                        &params.new_name,
                        &params.text_document.uri,
                        &params.position,
                    )
                    .map_err(language_service_failure)
            },
        )?;
        if info.can_rename && !info.file_to_rename.is_empty() {
            let old_uri = lsproto::DocumentUri(tsc_program::uri::file_name_to_document_uri(
                &info.file_to_rename,
            ));
            let new_uri = lsproto::DocumentUri(tsc_program::uri::file_name_to_document_uri(
                &info.new_file_name,
            ));
            // We send a `willRenameFiles` request if the client allows;
            // otherwise we directly compute the edits for renaming the file.
            if self
                .client_capabilities()
                .workspace
                .file_operations
                .will_rename
            {
                return Ok(lsproto::WorkspaceEditOrNull::WorkspaceEdit(
                    lsproto::WorkspaceEdit {
                        document_changes: Some(vec![
                        lsproto::TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile::RenameFile(
                            lsproto::RenameFile {
                                kind: Default::default(),
                                annotation_id: None,
                                old_uri,
                                new_uri,
                                options: None,
                            },
                        ),
                    ]),
                        ..lsproto::WorkspaceEdit::default()
                    },
                ));
            }
            let params = lsproto::RenameFilesParams {
                files: vec![lsproto::FileRename { old_uri, new_uri }],
            };
            return self.will_rename_files(context, &params, true);
        }
        let new_name = &params.new_name;
        let response = self.search_across_projects(
            context,
            &start,
            &params.text_document.uri,
            &params.position,
            SearchPosition::Lsp(params.position.clone()),
            |service, uri, at, with_definition| {
                let SearchPosition::Lsp(position) = at else {
                    unreachable!("a rename is at a document position");
                };
                service.search_rename(uri, position, new_name, with_definition)
            },
            tsc_ls::combine_rename,
        )?;
        Ok(response.unwrap_or(lsproto::WorkspaceEditOrNull::Null))
    }

    /// tsgo `handleWillRenameFiles`.
    fn handle_will_rename_files(
        self: &Arc<Self>,
        context: &RequestContext,
        params: lsproto::RenameFilesParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::WillRenameFilesResponse, Failure> {
        self.will_rename_files(context, &params, false)
    }

    /// tsgo `handleWillRenameFilesWorker`: the edits of each language
    /// service project (every project tree loaded) for each rename, each
    /// file's rename and each edit once; with `send_rename_file` (a rename
    /// for a client without `willRenameFiles`), the renames themselves
    /// after them.
    fn will_rename_files(
        &self,
        context: &RequestContext,
        params: &lsproto::RenameFilesParams,
        send_rename_file: bool,
    ) -> Result<lsproto::WillRenameFilesResponse, Failure> {
        type Change = lsproto::TextDocumentEditOrCreateFileOrRenameFileOrDeleteFile;
        if params.files.is_empty() {
            return Ok(lsproto::WorkspaceEditOrNull::Null);
        }
        let uris: Vec<lsproto::DocumentUri> = params
            .files
            .iter()
            .map(|file| file.old_uri.clone())
            .collect();
        let snapshot = self
            .initialized_session()
            .get_snapshot_loading_project_tree(&uris)
            .map_err(|error| LspError::internal(error.to_string()))?;
        let mut seen_edits: HashMap<(String, [u32; 4]), String> = HashMap::new();
        let mut seen_renames: HashSet<String> = HashSet::new();
        let mut document_changes: Vec<Change> = Vec::new();
        for project in snapshot.language_service_projects() {
            if project.program().is_none() {
                continue;
            }
            for file in &params.files {
                let changes =
                    self.with_language_service(context, &snapshot, project, |service| {
                        service
                            .get_edits_for_file_rename(&file.old_uri, &file.new_uri)
                            .map_err(language_service_failure)
                    })?;
                for change in changes {
                    match change {
                        Change::RenameFile(rename) => {
                            if seen_renames.insert(rename.old_uri.0.clone()) {
                                document_changes.push(Change::RenameFile(rename));
                            }
                        }
                        Change::TextDocumentEdit(edit) => {
                            let uri = edit.text_document.uri.0.clone();
                            let mut deduped = Vec::new();
                            for text_edit in edit.edits {
                                if let lsproto::TextEditOrAnnotatedTextEditOrSnippetTextEdit::TextEdit(
                                    text_edit,
                                ) = &text_edit
                                {
                                    let range = &text_edit.range;
                                    let key = (
                                        uri.clone(),
                                        [
                                            range.start.line,
                                            range.start.character,
                                            range.end.line,
                                            range.end.character,
                                        ],
                                    );
                                    if seen_edits.get(&key) == Some(&text_edit.new_text) {
                                        continue;
                                    }
                                    seen_edits.insert(key, text_edit.new_text.clone());
                                }
                                deduped.push(text_edit);
                            }
                            if !deduped.is_empty() {
                                document_changes.push(Change::TextDocumentEdit(
                                    lsproto::TextDocumentEdit {
                                        text_document: edit.text_document,
                                        edits: deduped,
                                    },
                                ));
                            }
                        }
                        other => document_changes.push(other),
                    }
                }
            }
        }
        if send_rename_file {
            for file in &params.files {
                document_changes.push(Change::RenameFile(lsproto::RenameFile {
                    kind: Default::default(),
                    annotation_id: None,
                    old_uri: file.old_uri.clone(),
                    new_uri: file.new_uri.clone(),
                    options: None,
                }));
            }
        }
        if document_changes.is_empty() {
            return Ok(lsproto::WorkspaceEditOrNull::Null);
        }
        if self
            .client_capabilities()
            .workspace
            .workspace_edit
            .document_changes
        {
            return Ok(lsproto::WorkspaceEditOrNull::WorkspaceEdit(
                lsproto::WorkspaceEdit {
                    document_changes: Some(document_changes),
                    ..lsproto::WorkspaceEdit::default()
                },
            ));
        }
        let mut changes: BTreeMap<lsproto::DocumentUri, Vec<lsproto::TextEdit>> = BTreeMap::new();
        for change in document_changes {
            if let Change::TextDocumentEdit(edit) = change {
                for text_edit in edit.edits {
                    if let lsproto::TextEditOrAnnotatedTextEditOrSnippetTextEdit::TextEdit(
                        text_edit,
                    ) = text_edit
                    {
                        changes
                            .entry(edit.text_document.uri.clone())
                            .or_default()
                            .push(text_edit);
                    }
                }
            }
        }
        Ok(lsproto::WorkspaceEditOrNull::WorkspaceEdit(
            lsproto::WorkspaceEdit {
                changes: Some(changes),
                ..lsproto::WorkspaceEdit::default()
            },
        ))
    }

    /// tsgo `handleWorkspaceSymbol`: the symbols of the document's projects
    /// with the `currentProject` scope, else of every project (each project
    /// tree loaded).
    fn handle_workspace_symbol(
        self: &Arc<Self>,
        context: &RequestContext,
        params: lsproto::WorkspaceSymbolParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::WorkspaceSymbolResponse, Failure> {
        let session = self.initialized_session();
        let current_project = params.text_document.as_ref().filter(|_| {
            session.config().workspace_symbols_scope
                == tsc_ls::lsutil::WorkspaceSymbolsScope::CURRENT_PROJECT
        });
        let (snapshot, projects) = match current_project {
            Some(document) => {
                let snapshot = session
                    .get_snapshot(std::slice::from_ref(&document.uri))
                    .map_err(|error| LspError::internal(error.to_string()))?;
                let file_name =
                    tsc_program::uri::uri_to_file_name(document.uri.as_str()).unwrap_or_default();
                let projects: Vec<Arc<Project>> = snapshot
                    .projects_containing_file(&file_name)
                    .into_iter()
                    .cloned()
                    .collect();
                (snapshot, projects)
            }
            None => {
                let snapshot = session
                    .get_snapshot_loading_project_tree(&[])
                    .map_err(|error| LspError::internal(error.to_string()))?;
                let projects: Vec<Arc<Project>> =
                    snapshot.language_service_projects().cloned().collect();
                (snapshot, projects)
            }
        };
        let mut matches = Vec::new();
        for project in &projects {
            if project.program().is_none() {
                continue;
            }
            matches.push(
                self.with_language_service(context, &snapshot, project, |service| {
                    service
                        .workspace_symbol_matches(&params.query)
                        .map_err(language_service_failure)
                })?,
            );
        }
        Ok(tsc_ls::workspace_symbols(matches))
    }

    /// tsgo `handleSourceDefinition`.
    fn handle_source_definition(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::TextDocumentPositionParams,
    ) -> Result<lsproto::CustomTextDocumentSourceDefinitionResponse, Failure> {
        service
            .provide_source_definition(&params.text_document.uri, &params.position)
            .map_err(language_service_failure)
    }

    /// tsgo `handlePrepareCallHierarchy`.
    fn handle_prepare_call_hierarchy(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::CallHierarchyPrepareParams,
    ) -> Result<lsproto::CallHierarchyPrepareResponse, Failure> {
        service
            .provide_prepare_call_hierarchy(&params.text_document.uri, &params.position)
            .map_err(language_service_failure)
    }

    /// tsgo `handleCallHierarchyIncomingCalls`: the calls into each of the
    /// item's declarations across projects (tsgo
    /// `ProvideCallHierarchyIncomingCalls`).
    fn handle_call_hierarchy_incoming_calls(
        self: &Arc<Self>,
        context: &RequestContext,
        params: lsproto::CallHierarchyIncomingCallsParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::CallHierarchyIncomingCallsResponse, Failure> {
        let start = self
            .cross_project_start(&params.item.uri)
            .map_err(|error| LspError::internal(error.to_string()))?;
        let targets = self.with_language_service(
            context,
            &start.snapshot,
            &start.default_project,
            |service| {
                service
                    .incoming_call_targets(&params.item)
                    .map_err(language_service_failure)
            },
        )?;
        let mut responses = Vec::new();
        for target in targets {
            responses.extend(self.search_across_projects(
                context,
                &start,
                &target.uri,
                &target.position,
                SearchPosition::Lsp(target.position.clone()),
                |service, uri, at, with_definition| {
                    service.search_incoming_calls(uri, at, with_definition)
                },
                tsc_ls::combine_incoming_calls,
            )?);
        }
        Ok(tsc_ls::incoming_calls_of_declarations(responses))
    }

    /// tsgo `handleCallHierarchyOutgoingCalls`.
    fn handle_call_hierarchy_outgoing_calls(
        self: &Arc<Self>,
        context: &RequestContext,
        params: lsproto::CallHierarchyOutgoingCallsParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::CallHierarchyOutgoingCallsResponse, Failure> {
        let (snapshot, project) = self
            .initialized_session()
            .get_snapshot_and_default_project(&params.item.uri)
            .map_err(|error| LspError::internal(error.to_string()))?;
        self.with_language_service(context, &snapshot, &project, |service| {
            service
                .provide_call_hierarchy_outgoing_calls(&params.item)
                .map_err(language_service_failure)
        })
    }

    /// tsgo `handleCodeLens`.
    fn handle_code_lens(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::CodeLensParams,
    ) -> Result<lsproto::CodeLensResponse, Failure> {
        service
            .provide_code_lenses(&params.text_document.uri)
            .map_err(language_service_failure)
    }

    /// tsgo `handleCodeLensResolve`: a lens of a file the session no longer
    /// has (one from an older snapshot) answers `ContentModified`.
    fn handle_code_lens_resolve(
        self: &Arc<Self>,
        context: &RequestContext,
        code_lens: lsproto::CodeLens,
        _request: &RequestMessage,
    ) -> Result<lsproto::CodeLensResolveResponse, Failure> {
        let uri = code_lens
            .data
            .as_ref()
            .map(|data| data.uri.clone())
            .unwrap_or_default();
        let Ok(start) = self.cross_project_start(&uri) else {
            return Err(ErrorCode::CONTENT_MODIFIED.into());
        };
        let command_name = lock(&self.state)
            .initialization_options
            .code_lens_show_locations_command_name
            .clone();
        // tsgo `ResolveCodeLens`: the default project's search at the lens's
        // data position, the other projects' at the start of its range.
        let data = code_lens.data.clone().expect("a code lens's data");
        let default_at = SearchPosition::Offset(data.position as u32);
        let position = &code_lens.range.start;
        let locations = if data.kind == lsproto::CodeLensKind::REFERENCES {
            // Don't include the declaration in the references count.
            let response = self.search_across_projects(
                context,
                &start,
                &uri,
                position,
                default_at,
                |service, uri, at, with_definition| {
                    service.search_references(uri, at, false, with_definition)
                },
                tsc_ls::combine_references,
            )?;
            match response {
                Some(lsproto::LocationsOrNull::Locations(locations)) => locations,
                _ => Vec::new(),
            }
        } else if data.kind == lsproto::CodeLensKind::IMPLEMENTATIONS {
            // Locations only, without the lens's own node.
            let response = self.search_across_projects(
                context,
                &start,
                &uri,
                position,
                default_at,
                |service, uri, at, with_definition| {
                    service.search_implementations(uri, at, true, with_definition)
                },
                tsc_ls::combine_implementations,
            )?;
            match response {
                Some(lsproto::LocationOrLocationsOrDefinitionLinksOrNull::Locations(locations)) => {
                    locations
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
        };
        Ok(tsc_ls::code_lens_with_command(
            context.locale.messages(),
            code_lens,
            locations,
            command_name.as_deref(),
        ))
    }

    /// tsgo `handleDocumentHighlight`.
    fn handle_document_highlight(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::DocumentHighlightParams,
    ) -> Result<lsproto::DocumentHighlightResponse, Failure> {
        service
            .provide_document_highlights(&params.text_document.uri, &params.position)
            .map_err(language_service_failure)
    }

    /// tsgo `handleMultiDocumentHighlight`.
    fn handle_multi_document_highlight(
        self: &Arc<Self>,
        _context: &RequestContext,
        service: &mut LanguageService<'_>,
        params: lsproto::MultiDocumentHighlightParams,
    ) -> Result<lsproto::CustomMultiDocumentHighlightResponse, Failure> {
        service
            .provide_multi_document_highlights(&params)
            .map_err(language_service_failure)
    }

    /// tsgo `ProvideVSReferences` across projects.
    fn handle_vs_references(
        self: &Arc<Self>,
        context: &RequestContext,
        start: &CrossProjectStart,
        params: lsproto::ReferenceParams,
    ) -> Result<lsproto::VSReferencesResponse, Failure> {
        let response = self.search_across_projects(
            context,
            start,
            &params.text_document.uri,
            &params.position,
            SearchPosition::Lsp(params.position.clone()),
            |service, uri, at, with_definition| {
                let SearchPosition::Lsp(position) = at else {
                    unreachable!("a references search is at a document position");
                };
                service.search_vs_references(uri, position, with_definition)
            },
            tsc_ls::combine_vs_references,
        )?;
        Ok(response.unwrap_or(lsproto::VSReferenceItemsOrNull::Null))
    }

    /// tsgo `ProvideImplementations` across projects.
    fn handle_implementation(
        self: &Arc<Self>,
        context: &RequestContext,
        start: &CrossProjectStart,
        params: lsproto::ImplementationParams,
    ) -> Result<lsproto::ImplementationResponse, Failure> {
        let response = self.search_across_projects(
            context,
            start,
            &params.text_document.uri,
            &params.position,
            SearchPosition::Lsp(params.position.clone()),
            |service, uri, at, with_definition| {
                service.search_implementations(uri, at, false, with_definition)
            },
            tsc_ls::combine_implementations,
        )?;
        Ok(response.unwrap_or(lsproto::LocationOrLocationsOrDefinitionLinksOrNull::Null))
    }

    /// tsgo `handleShutdown`.
    fn handle_shutdown(
        self: &Arc<Self>,
        _context: &RequestContext,
        _params: NoParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::ShutdownResponse, Failure> {
        self.initialized_session().close();
        Ok(lsproto::Null)
    }

    // -- document handlers ---------------------------------------------------

    /// tsgo `handleDidOpen`.
    fn handle_did_open(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::DidOpenTextDocumentParams,
    ) -> Result<(), Failure> {
        let document = params.text_document;
        self.initialized_session()
            .did_open_file(
                &document.uri,
                document.version,
                &document.text,
                &document.language_id,
            )
            .map_err(|error| LspError::internal(error.to_string()).into())
    }

    /// tsgo `handleDidChange`.
    fn handle_did_change(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::DidChangeTextDocumentParams,
    ) -> Result<(), Failure> {
        self.initialized_session().did_change_file(
            &params.text_document.uri,
            params.text_document.version,
            params.content_changes,
        );
        Ok(())
    }

    /// tsgo `handleDidSave`.
    fn handle_did_save(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::DidSaveTextDocumentParams,
    ) -> Result<(), Failure> {
        self.initialized_session()
            .did_save_file(&params.text_document.uri);
        Ok(())
    }

    /// tsgo `handleDidClose`.
    fn handle_did_close(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::DidCloseTextDocumentParams,
    ) -> Result<(), Failure> {
        self.initialized_session()
            .did_close_file(&params.text_document.uri);
        Ok(())
    }

    /// tsgo `handleDidChangeWatchedFiles`.
    fn handle_did_change_watched_files(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::DidChangeWatchedFilesParams,
    ) -> Result<(), Failure> {
        self.initialized_session()
            .did_change_watched_files(&params.changes);
        Ok(())
    }

    // -- the session's client ------------------------------------------------

    /// A context of the server's own request to the client, with tsgo's
    /// `watchRequestTimeout` (one second).
    fn watch_request_context(&self) -> RequestContext {
        RequestContext {
            cancelled: Arc::new(AtomicBool::new(false)),
            stopped: Arc::clone(&self.stopped),
            deadline: Some(Instant::now() + Duration::from_secs(1)),
            locale: self.locale(),
            request_id: None,
        }
    }

    /// tsgo `WatchFiles`: `client/registerCapability` of the watchers.
    fn watch_files(
        &self,
        id: &str,
        watchers: Vec<lsproto::FileSystemWatcher>,
    ) -> Result<(), String> {
        let registration = lsproto::RegistrationParams {
            registrations: vec![lsproto::Registration {
                id: id.to_owned(),
                register_options: Some(lsproto::RegisterOptions::WorkspaceDidChangeWatchedFiles(
                    lsproto::DidChangeWatchedFilesRegistrationOptions { watchers },
                )),
            }],
        };
        self.send_client_request::<lsproto::ClientRegisterCapabilityInfo>(
            &self.watch_request_context(),
            &registration,
        )
        .map_err(|failure| format!("failed to register file watcher: {failure}"))?;
        lock(&self.watchers).insert(id.to_owned());
        Ok(())
    }

    /// tsgo `UnwatchFiles`: `client/unregisterCapability` of a registered
    /// watch.
    fn unwatch_files(&self, id: &str) -> Result<(), String> {
        if !lock(&self.watchers).contains(id) {
            return Err(format!("no file watcher exists with ID {id}"));
        }
        let unregistration = lsproto::UnregistrationParams {
            unregisterations: vec![lsproto::Unregistration {
                id: id.to_owned(),
                method: lsproto::WorkspaceDidChangeWatchedFilesInfo::METHOD.to_owned(),
            }],
        };
        self.send_client_request::<lsproto::ClientUnregisterCapabilityInfo>(
            &self.watch_request_context(),
            &unregistration,
        )
        .map_err(|failure| format!("failed to unregister file watcher: {failure}"))?;
        lock(&self.watchers).remove(id);
        Ok(())
    }

    /// tsgo `RefreshDiagnostics`: fire and forget, as the client answers
    /// null and a slow client must not hold the server.
    fn refresh_diagnostics(&self) -> Result<(), String> {
        if !self
            .client_capabilities()
            .workspace
            .diagnostics
            .refresh_support
        {
            return Ok(());
        }
        self.send_client_request_fire_and_forget::<lsproto::WorkspaceDiagnosticRefreshInfo>(
            &NoParams,
        )
        .map_err(|error| format!("failed to refresh diagnostics: {error}"))
    }

    /// tsgo `RefreshInlayHints`.
    fn refresh_inlay_hints(&self) -> Result<(), String> {
        if !self
            .client_capabilities()
            .workspace
            .inlay_hint
            .refresh_support
        {
            return Ok(());
        }
        self.send_client_request_fire_and_forget::<lsproto::WorkspaceInlayHintRefreshInfo>(
            &NoParams,
        )
        .map_err(|error| format!("failed to refresh inlay hints: {error}"))
    }

    /// tsgo `RefreshCodeLens`.
    fn refresh_code_lens(&self) -> Result<(), String> {
        if !self
            .client_capabilities()
            .workspace
            .code_lens
            .refresh_support
        {
            return Ok(());
        }
        self.send_client_request_fire_and_forget::<lsproto::WorkspaceCodeLensRefreshInfo>(&NoParams)
            .map_err(|error| format!("failed to refresh code lens: {error}"))
    }

    /// tsgo `SetLocale`: `auto` is the locale `initialize` gave.
    fn set_locale(&self, locale: &str) {
        let mut state = lock(&self.state);
        let locale = if locale == "auto" {
            state.init_locale
        } else {
            match Locale::parse(locale) {
                Some(locale) => locale,
                None => return,
            }
        };
        state.locale = locale;
    }

    /// tsgo `handleSetLogVerbosity`.
    fn handle_set_log_verbosity(
        self: &Arc<Self>,
        _context: &RequestContext,
        params: lsproto::SetLogVerbosityParams,
    ) -> Result<(), Failure> {
        if !is_valid_log_verbosity(params.verbosity) {
            return Err(LspError::wrap(
                ErrorCode::INVALID_PARAMS,
                format!("invalid log verbosity {}", params.verbosity.0),
            )
            .into());
        }
        self.logger.set_verbosity(params.verbosity);
        Ok(())
    }
}

/// tsgo `fmt.Errorf("<context>: %w", err)`.
fn prefix_failure(failure: Failure, context: &str) -> Failure {
    match failure {
        Failure::Error(error) => Failure::Error(LspError {
            code: error.code,
            message: format!("{context}: {}", error.message),
        }),
        failure => failure,
    }
}

/// Go's `json.MarshalIndent(value, "", "\t")`.
fn marshal_indent<T: Serialize>(value: &T) -> String {
    let mut data = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b"\t");
    let mut serializer = serde_json::Serializer::with_formatter(&mut data, formatter);
    if value.serialize(&mut serializer).is_err() {
        return String::new();
    }
    String::from_utf8(data).unwrap_or_default()
}

/// Whether JSON text nests objects and arrays deeper than `limit`.
fn exceeds_depth(data: &[u8], limit: usize) -> bool {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in data {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// tsgo `handlers()`: each method's handler.
fn handlers() -> &'static HashMap<&'static str, Handler> {
    static HANDLERS: OnceLock<HashMap<&'static str, Handler>> = OnceLock::new();
    HANDLERS.get_or_init(|| {
        let mut handlers: HashMap<&'static str, Handler> = HashMap::new();
        register_request::<lsproto::InitializeInfo>(&mut handlers, |server, _, params, _| {
            server.handle_initialize(params)
        });
        register_notification::<lsproto::InitializedInfo>(
            &mut handlers,
            Server::handle_initialized,
        );
        register_request::<lsproto::ShutdownInfo>(&mut handlers, Server::handle_shutdown);
        register_notification::<lsproto::ExitInfo>(&mut handlers, |_, _, _| Err(Failure::Exit));
        register_notification::<lsproto::WorkspaceDidChangeConfigurationInfo>(
            &mut handlers,
            Server::handle_did_change_workspace_configuration,
        );
        register_notification::<lsproto::TextDocumentDidOpenInfo>(
            &mut handlers,
            Server::handle_did_open,
        );
        register_notification::<lsproto::TextDocumentDidChangeInfo>(
            &mut handlers,
            Server::handle_did_change,
        );
        register_notification::<lsproto::TextDocumentDidSaveInfo>(
            &mut handlers,
            Server::handle_did_save,
        );
        register_notification::<lsproto::TextDocumentDidCloseInfo>(
            &mut handlers,
            Server::handle_did_close,
        );
        register_notification::<lsproto::WorkspaceDidChangeWatchedFilesInfo>(
            &mut handlers,
            Server::handle_did_change_watched_files,
        );
        register_notification::<lsproto::SetTraceInfo>(&mut handlers, |_, _, _| Ok(()));
        register_language_service_document_request::<lsproto::TextDocumentDiagnosticInfo>(
            &mut handlers,
            Server::handle_document_diagnostic,
        );
        register_language_service_document_request::<lsproto::TextDocumentHoverInfo>(
            &mut handlers,
            Server::handle_hover,
        );
        register_language_service_with_auto_imports_request::<lsproto::TextDocumentCompletionInfo>(
            &mut handlers,
            Server::handle_completion,
        );
        register_request::<lsproto::CompletionItemResolveInfo>(
            &mut handlers,
            Server::handle_completion_item_resolve,
        );
        register_language_service_document_request::<lsproto::TextDocumentDefinitionInfo>(
            &mut handlers,
            Server::handle_definition,
        );
        register_language_service_document_request::<lsproto::TextDocumentTypeDefinitionInfo>(
            &mut handlers,
            Server::handle_type_definition,
        );
        register_language_service_document_request::<lsproto::TextDocumentSignatureHelpInfo>(
            &mut handlers,
            Server::handle_signature_help,
        );
        register_language_service_document_request::<lsproto::TextDocumentDocumentSymbolInfo>(
            &mut handlers,
            Server::handle_document_symbol,
        );
        register_language_service_document_request::<lsproto::TextDocumentFoldingRangeInfo>(
            &mut handlers,
            Server::handle_folding_range,
        );
        register_language_service_document_request::<lsproto::TextDocumentSelectionRangeInfo>(
            &mut handlers,
            Server::handle_selection_range,
        );
        register_language_service_document_request::<lsproto::TextDocumentPrepareRenameInfo>(
            &mut handlers,
            Server::handle_prepare_rename,
        );
        register_request::<lsproto::TextDocumentRenameInfo>(&mut handlers, Server::handle_rename);
        register_request::<lsproto::WorkspaceSymbolInfo>(
            &mut handlers,
            Server::handle_workspace_symbol,
        );
        register_language_service_document_request::<lsproto::CustomTextDocumentSourceDefinitionInfo>(
            &mut handlers,
            Server::handle_source_definition,
        );
        register_language_service_document_request::<lsproto::TextDocumentPrepareCallHierarchyInfo>(
            &mut handlers,
            Server::handle_prepare_call_hierarchy,
        );
        register_request::<lsproto::CallHierarchyIncomingCallsInfo>(
            &mut handlers,
            Server::handle_call_hierarchy_incoming_calls,
        );
        register_request::<lsproto::CallHierarchyOutgoingCallsInfo>(
            &mut handlers,
            Server::handle_call_hierarchy_outgoing_calls,
        );
        register_language_service_document_request::<lsproto::TextDocumentCodeLensInfo>(
            &mut handlers,
            Server::handle_code_lens,
        );
        register_request::<lsproto::CodeLensResolveInfo>(
            &mut handlers,
            Server::handle_code_lens_resolve,
        );
        register_request::<lsproto::WorkspaceWillRenameFilesInfo>(
            &mut handlers,
            Server::handle_will_rename_files,
        );
        register_language_service_document_request::<lsproto::TextDocumentDocumentHighlightInfo>(
            &mut handlers,
            Server::handle_document_highlight,
        );
        register_language_service_document_request::<
            lsproto::CustomTextDocumentMultiDocumentHighlightInfo,
        >(&mut handlers, Server::handle_multi_document_highlight);
        register_multi_project_reference_request::<lsproto::TextDocumentReferencesInfo>(
            &mut handlers,
            Server::handle_references,
        );
        register_multi_project_reference_request::<lsproto::TextDocumentVSReferencesInfo>(
            &mut handlers,
            Server::handle_vs_references,
        );
        register_multi_project_reference_request::<lsproto::TextDocumentImplementationInfo>(
            &mut handlers,
            Server::handle_implementation,
        );
        register_language_service_document_request::<lsproto::TextDocumentFormattingInfo>(
            &mut handlers,
            Server::handle_document_format,
        );
        register_language_service_document_request::<lsproto::TextDocumentRangeFormattingInfo>(
            &mut handlers,
            Server::handle_document_range_format,
        );
        register_language_service_document_request::<lsproto::TextDocumentOnTypeFormattingInfo>(
            &mut handlers,
            Server::handle_document_on_type_format,
        );
        register_language_service_with_auto_imports_request::<lsproto::TextDocumentCodeActionInfo>(
            &mut handlers,
            Server::handle_code_action,
        );
        register_notification::<lsproto::CustomSetLogVerbosityInfo>(
            &mut handlers,
            Server::handle_set_log_verbosity,
        );
        handlers
    })
}

/// A request's handler: its params to its response.
type RequestHandler<R> = fn(
    &Arc<Server>,
    &RequestContext,
    <R as Request>::Params,
    &RequestMessage,
) -> Result<<R as Request>::Response, Failure>;

/// A notification's handler.
type NotificationHandler<N> =
    fn(&Arc<Server>, &RequestContext, <N as Notification>::Params) -> Result<(), Failure>;

/// tsgo `registerRequestHandler`: decodes the params, runs `handle` and
/// answers, all in the synchronous part.
fn register_request<R: Request + 'static>(
    handlers: &mut HashMap<&'static str, Handler>,
    handle: RequestHandler<R>,
) where
    R::Params: 'static,
{
    handlers.insert(
        R::METHOD,
        Box::new(move |server, context, request| {
            if server.session().is_none() && R::METHOD != method::INITIALIZE {
                return Err(ErrorCode::SERVER_NOT_INITIALIZED.into());
            }
            let params = decode_params::<R::Params>(request)?;
            let response = handle(server, context, params, request)?;
            context.check()?;
            server
                .send_result(request.id.clone(), &response)
                .map_err(LspError::internal)?;
            Ok(None)
        }),
    );
}

/// A language service request's handler.
type LanguageServiceHandler<R> = fn(
    &Arc<Server>,
    &RequestContext,
    &mut LanguageService<'_>,
    <R as Request>::Params,
) -> Result<<R as Request>::Response, Failure>;

/// tsgo `registerLanguageServiceDocumentRequestHandler`: the document's
/// default project in the synchronous part; the handler on its program
/// after, then the configs' diagnostics again if checking found new global
/// ones, then the answer. A panic answers tsgo's `recover` error.
fn register_language_service_document_request<R: Request + 'static>(
    handlers: &mut HashMap<&'static str, Handler>,
    handle: LanguageServiceHandler<R>,
) where
    R::Params: HasTextDocumentUri + Send + 'static,
{
    handlers.insert(
        R::METHOD,
        Box::new(move |server, context, request| {
            if server.session().is_none() {
                return Err(ErrorCode::SERVER_NOT_INITIALIZED.into());
            }
            let params = decode_params::<R::Params>(request)?;
            let (snapshot, project) = server
                .initialized_session()
                .get_snapshot_and_default_project(params.text_document_uri())
                .map_err(|error| LspError::internal(error.to_string()))?;
            let server = Arc::clone(server);
            let context = context.clone();
            let id = request.id.clone();
            Ok(Some(Box::new(move || {
                let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    server.with_language_service(&context, &snapshot, &project, |service| {
                        handle(&server, &context, service, params)
                    })
                }));
                server
                    .initialized_session()
                    .enqueue_publish_global_diagnostics();
                let response = match response {
                    Ok(response) => response?,
                    Err(panic) => {
                        let detail = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                            .unwrap_or_default();
                        return Err(LspError::wrap(
                            ErrorCode::INTERNAL_ERROR,
                            format!("panic handling request {}: {detail}", R::METHOD),
                        )
                        .into());
                    }
                };
                context.check()?;
                server
                    .send_result(id, &response)
                    .map_err(LspError::internal)?;
                Ok(())
            })))
        }),
    );
}

/// A cross-project request's handler.
type MultiProjectHandler<R> = fn(
    &Arc<Server>,
    &RequestContext,
    &CrossProjectStart,
    <R as Request>::Params,
) -> Result<<R as Request>::Response, Failure>;

/// tsgo `registerMultiProjectReferenceRequestHandler`: the document's
/// default project and the projects with it in the synchronous part; the
/// search across projects after, then the answer. A panic answers tsgo's
/// `recover` error.
fn register_multi_project_reference_request<R: Request + 'static>(
    handlers: &mut HashMap<&'static str, Handler>,
    handle: MultiProjectHandler<R>,
) where
    R::Params: HasTextDocumentUri + Send + 'static,
{
    handlers.insert(
        R::METHOD,
        Box::new(move |server, context, request| {
            if server.session().is_none() {
                return Err(ErrorCode::SERVER_NOT_INITIALIZED.into());
            }
            let params = decode_params::<R::Params>(request)?;
            let start = server
                .cross_project_start(params.text_document_uri())
                .map_err(|error| LspError::internal(error.to_string()))?;
            let server = Arc::clone(server);
            let context = context.clone();
            let id = request.id.clone();
            Ok(Some(Box::new(move || {
                let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle(&server, &context, &start, params)
                }));
                let response = match response {
                    Ok(response) => response?,
                    Err(panic) => {
                        let detail = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                            .unwrap_or_default();
                        return Err(LspError::wrap(
                            ErrorCode::INTERNAL_ERROR,
                            format!("panic handling request {}: {detail}", R::METHOD),
                        )
                        .into());
                    }
                };
                context.check()?;
                server
                    .send_result(id, &response)
                    .map_err(LspError::internal)?;
                Ok(())
            })))
        }),
    );
}

/// tsgo `registerLanguageServiceWithAutoImportsRequestHandler`: as a
/// document request, but a handler that needs auto-imports runs again on a
/// snapshot prepared for the document's auto-imports (without the configs'
/// diagnostics after it).
fn register_language_service_with_auto_imports_request<R: Request + 'static>(
    handlers: &mut HashMap<&'static str, Handler>,
    handle: LanguageServiceHandler<R>,
) where
    R::Params: HasTextDocumentUri + Clone + Send + 'static,
{
    handlers.insert(
        R::METHOD,
        Box::new(move |server, context, request| {
            if server.session().is_none() {
                return Err(ErrorCode::SERVER_NOT_INITIALIZED.into());
            }
            let params = decode_params::<R::Params>(request)?;
            let (snapshot, project) = server
                .initialized_session()
                .get_snapshot_and_default_project(params.text_document_uri())
                .map_err(|error| LspError::internal(error.to_string()))?;
            let server = Arc::clone(server);
            let context = context.clone();
            let id = request.id.clone();
            Ok(Some(Box::new(move || {
                let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let response =
                        server.with_language_service(&context, &snapshot, &project, |service| {
                            handle(&server, &context, service, params.clone())
                        });
                    if !matches!(response, Err(Failure::NeedsAutoImports)) {
                        return response;
                    }
                    let (snapshot, project) = server
                        .initialized_session()
                        .get_snapshot_with_auto_imports(&snapshot, params.text_document_uri())
                        .map_err(|error| LspError::internal(error.to_string()))?;
                    context.check()?;
                    let response =
                        server.with_language_service(&context, &snapshot, &project, |service| {
                            handle(&server, &context, service, params)
                        });
                    if matches!(response, Err(Failure::NeedsAutoImports)) {
                        panic!(
                            "{} returned ErrNeedsAutoImports even after enabling auto imports",
                            R::METHOD
                        );
                    }
                    response
                }));
                let response = match response {
                    Ok(response) => response?,
                    Err(panic) => {
                        let detail = panic
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
                            .unwrap_or_default();
                        return Err(LspError::wrap(
                            ErrorCode::INTERNAL_ERROR,
                            format!("panic handling request {}: {detail}", R::METHOD),
                        )
                        .into());
                    }
                };
                context.check()?;
                server
                    .send_result(id, &response)
                    .map_err(LspError::internal)?;
                Ok(())
            })))
        }),
    );
}

/// tsgo `registerNotificationHandler`.
fn register_notification<N: Notification + 'static>(
    handlers: &mut HashMap<&'static str, Handler>,
    handle: NotificationHandler<N>,
) where
    N::Params: 'static,
{
    handlers.insert(
        N::METHOD,
        Box::new(move |server, context, request| {
            if server.session().is_none() && N::METHOD != method::INITIALIZED {
                return Err(ErrorCode::SERVER_NOT_INITIALIZED.into());
            }
            let params = decode_params::<N::Params>(request)?;
            handle(server, context, params)?;
            context.check()?;
            Ok(None)
        }),
    );
}

/// A language service failure: an internal error.
fn language_service_failure(error: LsError) -> Failure {
    if error.is_needs_auto_imports() {
        return Failure::NeedsAutoImports;
    }
    LspError::internal(error.0).into()
}

/// The server as the session's client and logger (tsgo's `Client: s` and
/// `Logger: s.logger`), without keeping the server alive.
struct SessionHost(Weak<Server>);

impl tsc_project::Client for SessionHost {
    /// tsgo `PublishDiagnostics`.
    fn publish_diagnostics(&self, params: lsproto::PublishDiagnosticsParams) -> Result<(), String> {
        self.0.upgrade().map_or(Ok(()), |server| {
            server.send_notification::<lsproto::TextDocumentPublishDiagnosticsInfo>(&params)
        })
    }

    fn watch_files(
        &self,
        id: &str,
        watchers: Vec<lsproto::FileSystemWatcher>,
    ) -> Result<(), String> {
        self.0
            .upgrade()
            .map_or(Ok(()), |server| server.watch_files(id, watchers))
    }

    fn unwatch_files(&self, id: &str) -> Result<(), String> {
        self.0
            .upgrade()
            .map_or(Ok(()), |server| server.unwatch_files(id))
    }

    fn refresh_diagnostics(&self) -> Result<(), String> {
        self.0
            .upgrade()
            .map_or(Ok(()), |server| server.refresh_diagnostics())
    }

    fn refresh_inlay_hints(&self) -> Result<(), String> {
        self.0
            .upgrade()
            .map_or(Ok(()), |server| server.refresh_inlay_hints())
    }

    fn refresh_code_lens(&self) -> Result<(), String> {
        self.0
            .upgrade()
            .map_or(Ok(()), |server| server.refresh_code_lens())
    }

    fn set_locale(&self, locale: &str) {
        if let Some(server) = self.0.upgrade() {
            server.set_locale(locale);
        }
    }

    fn locale(&self) -> Locale {
        self.0
            .upgrade()
            .map(|server| server.locale())
            .unwrap_or_default()
    }
}

impl tsc_project::Logger for SessionHost {
    fn log(&self, message: &str) {
        if let Some(server) = self.0.upgrade() {
            server.logger.log(&server, message);
        }
    }

    fn is_verbose(&self) -> bool {
        self.0
            .upgrade()
            .is_some_and(|server| server.logger.is_verbose())
    }
}
