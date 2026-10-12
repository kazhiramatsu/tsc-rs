//! tsgo `ipc.AsyncConn` (conn_async.go): a JSON-RPC connection whose calls
//! may come from any thread while one thread runs its read loop; a response
//! goes to the call that waits for its ID.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use super::jsonrpc::{MessageReader, MessageWriter};
use super::{panic_message, Error, Handler, Id, Message, ResponseError, CODE_INTERNAL_ERROR};

/// tsgo `ipc.ErrConnClosed`.
const CONN_CLOSED: &str = "ipc: connection closed";

/// Why a [`AsyncConn::call`] has no result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallError {
    /// No response came within the call's time limit (tsgo: the call's
    /// context ended).
    TimedOut,
    /// The request could not be written, the peer answered with an error,
    /// or the connection stopped (with tsgo's message).
    Failed(Error),
}

impl std::fmt::Display for CallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("context deadline exceeded"),
            Self::Failed(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CallError {}

#[derive(Default)]
struct Pending {
    calls: HashMap<String, mpsc::Sender<Message>>,
    /// tsgo `AsyncConn.terminal`: why the connection stopped.
    terminal: Option<String>,
    /// tsgo `AsyncConn.hasCause`.
    has_cause: bool,
}

impl Pending {
    /// tsgo `recordTerminalErrorLocked`: the connection is closed, with the
    /// first cause recorded; whether this cause was the first.
    fn record_terminal(&mut self, cause: Option<&str>) -> bool {
        match (&mut self.terminal, cause) {
            (None, None) => {
                self.terminal = Some(CONN_CLOSED.to_owned());
                false
            }
            (None, Some(cause)) => {
                self.terminal = Some(format!("{CONN_CLOSED}\n{cause}"));
                self.has_cause = true;
                true
            }
            (Some(terminal), Some(cause)) if !self.has_cause => {
                terminal.push('\n');
                terminal.push_str(cause);
                self.has_cause = true;
                true
            }
            _ => false,
        }
    }
}

/// tsgo `ipc.AsyncConn`. One thread runs [`Self::run`] over the reading half
/// of the transport; [`Self::call`] and [`Self::notify`] may be used from any
/// thread meanwhile. tsgo handles each incoming request on a goroutine of its
/// own; here the read loop handles it before reading on, which only orders
/// the answers of a handler that answers at once (as the content mapper's,
/// which rejects every request).
pub struct AsyncConn<W: Write> {
    writer: Mutex<MessageWriter<W>>,
    handler: Box<dyn Handler>,
    sequence: AtomicU64,
    pending: Mutex<Pending>,
}

impl<W: Write> AsyncConn<W> {
    pub fn new(writer: W, handler: Box<dyn Handler>) -> Self {
        Self {
            writer: Mutex::new(MessageWriter::new(writer)),
            handler,
            sequence: AtomicU64::new(0),
            pending: Mutex::new(Pending::default()),
        }
    }

    /// tsgo `Run`: reads the peer's messages until its input ends or fails,
    /// then fails the calls still waiting with the reason.
    pub fn run<R: Read>(&self, reader: &mut MessageReader<R>) -> Result<(), Error> {
        let result = self.read_loop(reader);
        let mut pending = self.lock_pending();
        pending.record_terminal(result.as_ref().err().map(Error::message));
        // Dropping a sender ends its call's wait.
        pending.calls.clear();
        result
    }

    fn read_loop<R: Read>(&self, reader: &mut MessageReader<R>) -> Result<(), Error> {
        while let Some(message) = reader.read_message()? {
            if message.is_response() {
                self.handle_response(message);
            } else if message.is_request() {
                self.handle_request(&message)?;
            } else if message.is_notification() {
                self.handler
                    .handle_notification(&message.method, &message.params);
            }
        }
        Ok(())
    }

    /// tsgo `handleResponse`: the waiting call takes the response; one no
    /// call waits for is dropped.
    fn handle_response(&self, message: Message) {
        let Some(id) = message.id.as_ref().map(id_key) else {
            return;
        };
        let sender = self.lock_pending().calls.remove(&id);
        if let Some(sender) = sender {
            let _ = sender.send(message);
        }
    }

    /// tsgo `handleRequest`: the handler's result or error, and a panic's
    /// message as an error response.
    fn handle_request(&self, message: &Message) -> Result<(), Error> {
        let id = message.id.as_ref().expect("a request has an ID");
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.handler
                .handle_request(&message.method, &message.params)
        }));
        let mut writer = self.lock_writer();
        let written = match outcome {
            Ok(Ok(result)) => writer.write_response(id, &result),
            Ok(Err(message)) => writer.write_error(
                id,
                &ResponseError {
                    code: CODE_INTERNAL_ERROR,
                    message,
                },
            ),
            Err(panic) => writer.write_error(
                id,
                &ResponseError {
                    code: CODE_INTERNAL_ERROR,
                    message: format!("panic: {}", panic_message(panic.as_ref())),
                },
            ),
        };
        written.map_err(|error| Error::new(format!("ipc: failed to write response: {error}")))
    }

    /// tsgo `Call`: sends a request with the next `api<n>` ID and waits for
    /// its response, at most `timeout` when one is given.
    pub fn call(
        &self,
        method: &str,
        params: &str,
        timeout: Option<Duration>,
    ) -> Result<Vec<u8>, CallError> {
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let id = Id::String(format!("api{sequence}"));
        let key = id_key(&id);
        let (sender, receiver) = mpsc::channel();
        {
            let mut pending = self.lock_pending();
            if let Some(terminal) = &pending.terminal {
                return Err(CallError::Failed(Error::new(terminal.clone())));
            }
            pending.calls.insert(key.clone(), sender);
        }
        let response = (|| {
            self.lock_writer()
                .write_request(&id, method, params)
                .map_err(CallError::Failed)?;
            let received = match timeout {
                Some(timeout) => receiver.recv_timeout(timeout),
                None => receiver.recv().map_err(|_| RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(message) => Ok(message),
                Err(RecvTimeoutError::Timeout) => Err(CallError::TimedOut),
                Err(RecvTimeoutError::Disconnected) => Err(CallError::Failed(Error::new(
                    self.lock_pending()
                        .terminal
                        .clone()
                        .unwrap_or_else(|| CONN_CLOSED.to_owned()),
                ))),
            }
        })();
        self.lock_pending().calls.remove(&key);
        let message = response?;
        if let Some(error) = message.error {
            return Err(CallError::Failed(Error::new(format!(
                "ipc: remote error [{}]: {}",
                error.code, error.message
            ))));
        }
        Ok(message.result)
    }

    /// tsgo `Notify`: sends a notification, unless the connection stopped.
    pub fn notify(&self, method: &str, params: &str) -> Result<(), Error> {
        if let Some(terminal) = &self.lock_pending().terminal {
            return Err(Error::new(terminal.clone()));
        }
        self.lock_writer().write_notification(method, params)
    }

    fn lock_pending(&self) -> MutexGuard<'_, Pending> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_writer(&self) -> MutexGuard<'_, MessageWriter<W>> {
        self.writer.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// A response's ID as the key its call waits under.
fn id_key(id: &Id) -> String {
    match id {
        Id::String(text) => format!("s{text}"),
        Id::Int(number) => format!("i{number}"),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/ipc/conn_async_tests.rs"]
mod conn_async_tests;
