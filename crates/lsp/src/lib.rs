//! tsgo's language server (`internal/lsp`): the server's loops and
//! lifecycle (`server`) and its command line (`cli`). The protocol's types
//! are `tsc_lsproto`.

pub mod capabilities;
pub mod cli;
pub mod content_mappers;
pub mod gofmt;
pub mod logger;
pub mod progress;
pub mod queue;
pub mod server;

pub use server::{Server, ServerOptions};
