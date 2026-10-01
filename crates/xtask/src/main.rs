//! Workspace maintenance commands (`cargo xtask <command>`).
//!
//! `codegen diagnostics` regenerates `crates/diagnostics/src/gen.rs` from the
//! vendored TypeScript 7.1 diagnostic catalog; `codegen diagnostics-check`
//! verifies that the generated file is current.

#![deny(unsafe_code)]

mod codegen_common;
mod diagnostics_codegen;

fn main() {
    let mut args = std::env::args().skip(1);
    let result = match (args.next().as_deref(), args.next().as_deref(), args.next()) {
        (Some("codegen"), Some("diagnostics"), None) => diagnostics_codegen::run(false),
        (Some("codegen"), Some("diagnostics-check"), None) => diagnostics_codegen::run(true),
        _ => {
            eprintln!("usage: cargo xtask codegen diagnostics|diagnostics-check");
            std::process::exit(2);
        }
    };
    if let Err(error) = result {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
