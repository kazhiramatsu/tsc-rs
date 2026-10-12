//! The API server's connection (tsgo `ipc.SyncConn` and `ipc.AsyncConn` as
//! the API runs them) over the shared messages and protocols of
//! [`tsc_host::ipc`].

mod conn;
mod timing;

pub use conn::{Conn, Mode};
pub use tsc_host::ipc::{
    jsonrpc, panic_message, Error, Handler, Id, Message, Payload, Protocol, ResponseError,
    CODE_INTERNAL_ERROR,
};
