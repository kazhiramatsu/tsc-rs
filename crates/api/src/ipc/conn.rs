//! tsgo `ipc.SyncConn` and `ipc.AsyncConn` (conn_sync.go, conn_async.go):
//! requests handled where they are read, one at a time, and calls to the
//! client that read the client's messages until their response arrives.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use super::timing::{
    server_timing_snapshot, TimingCollector, METHOD_GET_SERVER_TIMING, METHOD_RESET_SERVER_TIMING,
};
use super::{panic_message, Error, Handler, Id, Message, Payload, Protocol, ResponseError};

/// tsgo `ipc.ErrConnClosed`.
const CONN_CLOSED: &str = "ipc: connection closed";

/// Which of tsgo's connections a [`Conn`] is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    /// tsgo `SyncConn` (MessagePack): a call's ID is its method, and a
    /// response no call waits for is an error.
    Sync,
    /// tsgo `AsyncConn` (JSON-RPC) with its requests handled one at a time
    /// (a client matches responses by ID, so the order does not change the
    /// results): a call's ID is `api<n>`, a response no call waits for is
    /// dropped, and once the connection stops, calls and notifications fail
    /// with its cause.
    Async,
}

/// A connection to an API client (tsgo `ipc.Conn`).
pub struct Conn {
    protocol: Mutex<Box<dyn Protocol>>,
    handler: std::sync::Arc<dyn Handler>,
    mode: Mode,
    timing: Option<TimingCollector>,
    sequence: AtomicU64,
    /// tsgo `AsyncConn.terminal`: why an asynchronous connection stopped.
    terminal: Mutex<Option<String>>,
}

impl Conn {
    pub fn new(
        protocol: Box<dyn Protocol>,
        handler: std::sync::Arc<dyn Handler>,
        mode: Mode,
    ) -> Self {
        Self {
            protocol: Mutex::new(protocol),
            handler,
            mode,
            timing: None,
            sequence: AtomicU64::new(0),
            terminal: Mutex::new(None),
        }
    }

    /// tsgo `SetCollectTiming`: whether each request's processing time is
    /// collected for `getServerTiming`.
    pub fn set_collect_timing(&mut self, enabled: bool) {
        self.timing = enabled.then(TimingCollector::default);
    }

    /// tsgo `Run`: handles the client's messages until the input ends.
    pub fn run(&self) -> Result<(), Error> {
        let result = self.read_loop();
        if self.mode == Mode::Async {
            let mut terminal = self.lock_terminal();
            *terminal = Some(match &result {
                Ok(()) => CONN_CLOSED.to_owned(),
                Err(error) => format!("{CONN_CLOSED}\n{error}"),
            });
        }
        result
    }

    fn read_loop(&self) -> Result<(), Error> {
        loop {
            let Some(message) = self.lock_protocol().read_message()? else {
                return Ok(());
            };
            if message.is_request() {
                self.handle_request(&message)?;
            } else if message.is_notification() {
                self.handler
                    .handle_notification(&message.method, &message.params);
            } else if self.mode == Mode::Sync {
                // Responses are read where a call waits for them.
                return Err(Error::new(
                    "ipc: unexpected response message in sync connection",
                ));
            }
        }
    }

    fn handle_request(&self, message: &Message) -> Result<(), Error> {
        let id = message.id.as_ref().expect("a request has an ID");
        // The timing meta-requests are answered here and not recorded.
        match message.method.as_str() {
            METHOD_GET_SERVER_TIMING => {
                let snapshot = server_timing_snapshot(self.timing.as_ref()).to_json();
                return self
                    .lock_protocol()
                    .write_response(id, &Payload::Json(snapshot))
                    .map_err(|error| {
                        Error::new(format!(
                            "ipc: failed to write server timing response: {error}"
                        ))
                    });
            }
            METHOD_RESET_SERVER_TIMING => {
                if let Some(timing) = &self.timing {
                    timing.reset();
                }
                return self
                    .lock_protocol()
                    .write_response(id, &Payload::null())
                    .map_err(|error| {
                        Error::new(format!(
                            "ipc: failed to write reset server timing response: {error}"
                        ))
                    });
            }
            _ => {}
        }
        let start = self.timing.as_ref().map(|_| Instant::now());
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.handler
                .handle_request(&message.method, &message.params)
        }));
        let result = match outcome {
            Ok(result) => result,
            Err(panic) => {
                let panic = panic_message(panic.as_ref());
                return self
                    .lock_protocol()
                    .write_error(id, &ResponseError::internal(format!("panic: {panic}")))
                    .map_err(|error| {
                        Error::new(format!(
                            "ipc: failed to write panic error response: {error} (original panic: {panic})"
                        ))
                    });
            }
        };
        if let (Some(timing), Some(start)) = (&self.timing, start) {
            timing.record(&message.method, start.elapsed());
        }
        let mut protocol = self.lock_protocol();
        match result {
            Ok(payload) => protocol.write_response(id, &payload),
            Err(message) => protocol.write_error(id, &ResponseError::internal(message)),
        }
        .map_err(|error| Error::new(format!("ipc: failed to write response: {error}")))
    }

    /// tsgo `Call`: sends a request to the client and waits for its
    /// response, handling the client's requests and notifications that come
    /// first.
    pub fn call(&self, method: &str, params: &str) -> Result<Vec<u8>, Error> {
        self.check_open()?;
        let id = match self.mode {
            Mode::Sync => Id::String(method.to_owned()),
            Mode::Async => Id::String(format!(
                "api{}",
                self.sequence.fetch_add(1, Ordering::Relaxed) + 1
            )),
        };
        let mut protocol = self.lock_protocol();
        protocol.write_request(&id, method, params)?;
        loop {
            let Some(message) = protocol.read_message()? else {
                return Err(Error::new(match self.mode {
                    Mode::Sync => "EOF",
                    Mode::Async => CONN_CLOSED,
                }));
            };
            if message.is_response() && message.id.as_ref() == Some(&id) {
                if let Some(error) = message.error {
                    return Err(Error::new(format!(
                        "ipc: remote error [{}]: {}",
                        error.code, error.message
                    )));
                }
                return Ok(message.result);
            }
            if message.is_request() {
                // A synchronous client callback may make a nested request.
                drop(protocol);
                self.handle_request(&message)?;
                protocol = self.lock_protocol();
            } else if message.is_notification() {
                drop(protocol);
                self.handler
                    .handle_notification(&message.method, &message.params);
                protocol = self.lock_protocol();
            } else if self.mode == Mode::Sync {
                return Err(Error::new(format!(
                    "ipc: unexpected message while waiting for {method:?} response"
                )));
            }
        }
    }

    /// tsgo `Notify`: sends a notification to the client.
    pub fn notify(&self, method: &str, params: &str) -> Result<(), Error> {
        self.check_open()?;
        self.lock_protocol().write_notification(method, params)
    }

    fn check_open(&self) -> Result<(), Error> {
        match &*self.lock_terminal() {
            Some(terminal) => Err(Error::new(terminal.clone())),
            None => Ok(()),
        }
    }

    fn lock_protocol(&self) -> MutexGuard<'_, Box<dyn Protocol>> {
        self.protocol.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_terminal(&self) -> MutexGuard<'_, Option<String>> {
        self.terminal.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
