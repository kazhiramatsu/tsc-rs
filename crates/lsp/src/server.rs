//! tsgo `lsp.Server` (`internal/lsp/server.go`): the language server's
//! loops and its lifecycle requests.
//!
//! Three loops run as in tsgo. The read loop answers `initialize` before the
//! server is initialized (and `ServerNotInitialized` to any other request),
//! routes the client's responses to the server's pending requests, cancels on
//! `$/cancelRequest`, and queues every other message. The dispatch loop runs
//! each handler's synchronous part in order; a handler may return work that
//! runs on its own thread. The write loop sends the outgoing messages.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tsc_compiler::locale::Locale;
use tsc_host::ipc::jsonrpc::{Reader, Writer};
use tsc_ls::lsutil::{parse_user_preferences, UserPreferences};
use tsc_lsproto::message::{decode_result, to_raw};
use tsc_lsproto::{
    self as lsproto, decode_params, method, parse_message, ErrorCode, Id, LspError, Message,
    NoParams, Notification, Request, RequestMessage, ResponseError, ResponseMessage,
};

use crate::gofmt;
use crate::logger::{is_valid_log_verbosity, Logger};
use crate::queue::DynamicQueue;

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
}

/// Why a handler did not answer (tsgo's handler errors: `io.EOF` for
/// `exit`, `context.Canceled`, and the others).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The client sent `exit`.
    Exit,
    /// The request was cancelled.
    Cancelled,
    Error(LspError),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exit => formatter.write_str("EOF"),
            Self::Cancelled => formatter.write_str("context canceled"),
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
    pub locale: Locale,
    pub request_id: Option<Id>,
}

impl RequestContext {
    /// tsgo `ctx.Err() != nil`: the request was cancelled or the server
    /// stopped.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst) || self.stopped.load(Ordering::SeqCst)
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
    /// The preferences the client configured, which the session takes.
    user_preferences: Option<UserPreferences>,
}

/// tsgo `Server`.
pub struct Server {
    requests: DynamicQueue<RequestMessage>,
    outgoing: DynamicQueue<Message>,
    pending_client_requests: Mutex<HashMap<Id, Arc<AtomicBool>>>,
    pending_server_requests: Mutex<HashMap<Id, mpsc::Sender<ResponseMessage>>>,
    client_seq: AtomicI32,
    pub(crate) init_started: AtomicBool,
    /// tsgo `session != nil`: set when `initialized` creates the session.
    session_created: AtomicBool,
    stopped: Arc<AtomicBool>,
    stop_reason: Mutex<Option<StopReason>>,
    state: Mutex<ServerState>,
    pub(crate) logger: Logger,
    init_complete: (Mutex<bool>, Condvar),
    pub(crate) stderr: Mutex<Box<dyn Write + Send>>,
    cwd: String,
    set_parent_process_id: Option<Box<dyn Fn(i32) + Send + Sync>>,
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
            session_created: AtomicBool::new(false),
            stopped: Arc::new(AtomicBool::new(false)),
            stop_reason: Mutex::new(None),
            state: Mutex::default(),
            logger: Logger::default(),
            init_complete: (Mutex::new(false), Condvar::new()),
            stderr: Mutex::new(options.stderr),
            cwd: options.cwd,
            set_parent_process_id: options.set_parent_process_id,
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
                    let elapsed = gofmt::format_duration(start.elapsed());
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
                    let elapsed = gofmt::format_duration(start.elapsed());
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
        lock(&self.state).user_preferences.clone()
    }

    fn session_created(&self) -> bool {
        self.session_created.load(Ordering::SeqCst)
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
        let (dynamic_watch_registration, telemetry, workspace_folders, root_uri, root_path) = {
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
        {
            let mut state = lock(&self.state);
            state.telemetry_enabled = telemetry;
            state.current_directory = cwd;
        }
        self.session_created.store(true, Ordering::SeqCst);
        let preferences = self.request_configuration(context)?;
        lock(&self.state).user_preferences = Some(preferences);
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
            lock(&self.state).user_preferences = Some(parse_user_preferences(settings));
        }
        Ok(())
    }

    /// tsgo `handleShutdown`.
    fn handle_shutdown(
        self: &Arc<Self>,
        _context: &RequestContext,
        _params: NoParams,
        _request: &RequestMessage,
    ) -> Result<lsproto::ShutdownResponse, Failure> {
        Ok(lsproto::Null)
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
        register_notification::<lsproto::SetTraceInfo>(&mut handlers, |_, _, _| Ok(()));
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
            if !server.session_created() && R::METHOD != method::INITIALIZE {
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
            if !server.session_created() && N::METHOD != method::INITIALIZED {
                return Err(ErrorCode::SERVER_NOT_INITIALIZED.into());
            }
            let params = decode_params::<N::Params>(request)?;
            handle(server, context, params)?;
            context.check()?;
            Ok(None)
        }),
    );
}
