#![forbid(unsafe_code)]

pub mod adapters;
pub mod classify;
pub mod cli;
pub mod compare;
mod error;
pub mod evaluate;
pub mod executor;
pub mod model;
pub mod normalize;
pub mod preflight;
pub mod process_session;
pub mod replay;
pub mod rust_worker;
pub mod schema;
pub mod worker_protocol;

pub use error::{FoundationError, FoundationResult};

use tsc_checker::{check_program, CompilerOptions, InputFile};

pub fn smoke_generated_source(source: &str) -> usize {
    let files = [InputFile {
        name: "main.ts".to_string(),
        text: source.to_string(),
    }];

    check_program(&files, &CompilerOptions::default())
        .diagnostics
        .len()
}
