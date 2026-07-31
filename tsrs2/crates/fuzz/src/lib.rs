#![forbid(unsafe_code)]

pub mod preflight;

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
