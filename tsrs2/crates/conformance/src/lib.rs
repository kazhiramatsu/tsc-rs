#![forbid(unsafe_code)]

use tsrs2_checker::{check_program, CompilerOptions};

pub fn run_empty_engine_smoke() -> usize {
    check_program(&[], &CompilerOptions::default())
        .diagnostics
        .len()
}
