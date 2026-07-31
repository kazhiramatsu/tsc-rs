#![forbid(unsafe_code)]

pub mod classify;
pub mod compare;
mod error;
pub mod evaluate;
pub mod model;
pub mod normalize;
pub mod preflight;
pub mod schema;

pub use error::{FoundationError, FoundationResult};

use tsrs2_checker::{check_program, CompilerOptions, InputFile};

pub fn smoke_generated_source(source: &str) -> usize {
    let files = [InputFile {
        name: "main.ts".to_string(),
        text: source.to_string(),
    }];

    check_program(&files, &CompilerOptions::default())
        .diagnostics
        .len()
}
