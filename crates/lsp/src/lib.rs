//! tsgo's language server (`internal/lsp`): the protocol's types
//! (`lsproto`), the server's loops and lifecycle (`server`) and its command
//! line (`cli`).

pub mod capabilities;
pub mod cli;
pub mod gofmt;
pub mod logger;
pub mod lsproto;
pub mod queue;
pub mod server;

pub use server::{Server, ServerOptions};
