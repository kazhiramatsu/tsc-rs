//! tsgo's `internal/ipc` and `internal/jsonrpc` (19dadef8): the messages an
//! API connection exchanges, the protocols that frame them and the
//! [`Conn`] that runs a [`Handler`] over a protocol.

use std::fmt;

mod conn;
pub mod jsonrpc;
mod timing;

pub use conn::{Conn, Mode};

/// tsgo `jsonrpc.CodeInternalError`, the code of every error response the
/// connection writes.
pub const CODE_INTERNAL_ERROR: i32 = -32603;

/// tsgo `jsonrpc.ID`: a message's ID, a string or an integer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Id {
    String(String),
    Int(i32),
}

impl fmt::Display for Id {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(text) => formatter.write_str(text),
            Self::Int(number) => write!(formatter, "{number}"),
        }
    }
}

/// tsgo `jsonrpc.ResponseError`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
}

impl ResponseError {
    /// An error response with tsgo's code for a failed request.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: CODE_INTERNAL_ERROR,
            message: message.into(),
        }
    }
}

/// tsgo `jsonrpc.Message`: a request, a notification or a response, with
/// its parameters or result kept as JSON text.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Message {
    pub id: Option<Id>,
    pub method: String,
    pub params: Vec<u8>,
    pub result: Vec<u8>,
    pub error: Option<ResponseError>,
}

impl Message {
    /// A request: an ID and a method.
    pub fn is_request(&self) -> bool {
        self.id.is_some() && !self.method.is_empty()
    }

    /// A notification: a method without an ID.
    pub fn is_notification(&self) -> bool {
        self.id.is_none() && !self.method.is_empty()
    }

    /// A response: an ID without a method.
    pub fn is_response(&self) -> bool {
        self.id.is_some() && self.method.is_empty()
    }
}

/// A request's result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Payload {
    /// JSON text.
    Json(String),
    /// tsgo `RawBinary`: bytes the MessagePack protocol sends as they are.
    Binary(Vec<u8>),
}

impl Payload {
    /// `null`.
    pub fn null() -> Self {
        Self::Json("null".to_owned())
    }
}

/// A failed read, write or exchange, with tsgo's message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error(String);

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

/// tsgo `ipc.Protocol`: how messages are read and written.
pub trait Protocol: Send {
    /// The next message; nothing at the end of the input.
    fn read_message(&mut self) -> Result<Option<Message>, Error>;
    fn write_request(&mut self, id: &Id, method: &str, params: &str) -> Result<(), Error>;
    fn write_notification(&mut self, method: &str, params: &str) -> Result<(), Error>;
    fn write_response(&mut self, id: &Id, result: &Payload) -> Result<(), Error>;
    fn write_error(&mut self, id: &Id, error: &ResponseError) -> Result<(), Error>;
}

/// tsgo `ipc.Handler`: what answers a connection's requests.
pub trait Handler: Send + Sync {
    /// The request's result, or the message of its error response.
    fn handle_request(&self, method: &str, params: &[u8]) -> Result<Payload, String>;
    fn handle_notification(&self, method: &str, params: &[u8]);
}

/// A panic's message, as Go's `%v` of the recovered value.
pub(crate) fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "a panic without a message".to_owned()
    }
}
