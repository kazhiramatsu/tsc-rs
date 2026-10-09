//! Check a tsconfig.json without emitting files and return diagnostics as JSON.
//!
//! The Rust API is experimental and still under development; its interfaces
//! may change incompatibly.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;

use serde_json::{json, Value};
use tsc_compiler::ProgramSession;
use tsc_diagnostics::{Diagnostic, DiagnosticCategory, JsString, MessageChain, TextSnapshot};
use tsc_host::FsCompilerHost;
use tsc_program::{
    decode_host_text, is_non_fatal_option_diagnostic, load_config_program_with_no_emit_override,
    parse_config_root_plan, CompilerConfigHost, ConfigRootPlanRequest, LibraryCatalog,
    ProgramLoadLimits,
};

type SourceTexts = BTreeMap<JsString, Arc<TextSnapshot>>;

fn main() -> ExitCode {
    match run() {
        Ok(has_errors) => ExitCode::from(u8::from(has_errors)),
        Err(error) => {
            eprintln!("Type check could not complete: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, Box<dyn Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: type_check <tsconfig.json> <typescript-lib-directory>".into());
    }
    let (diagnostics, sources) = check_project(Path::new(&args[0]), Path::new(&args[1]))?;

    // A completed check can still contain TypeScript errors. These are data,
    // separate from I/O, unsupported options, or compiler execution failures.
    let has_errors = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.category() == DiagnosticCategory::Error);
    let report = json!({
        "has_errors": has_errors,
        "diagnostics": diagnostics.iter()
            .map(|diagnostic| diagnostic_json(diagnostic, &sources))
            .collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(has_errors)
}

fn check_project(
    config_file: &Path,
    library_directory: &Path,
) -> Result<(Vec<Diagnostic>, SourceTexts), Box<dyn Error>> {
    let host = FsCompilerHost::from_process()?;
    let config_file = std::path::absolute(config_file)?;
    let config_name = config_file.to_str().ok_or("config path must be Unicode")?;
    let base_path = config_file
        .parent()
        .and_then(Path::to_str)
        .ok_or("config directory must be Unicode")?;
    let plan = parse_config_root_plan(
        &CompilerConfigHost::new(&host),
        ConfigRootPlanRequest {
            file_name: config_name.into(),
            text: decode_host_text(std::fs::read(&config_file)?)?,
            base_path: base_path.into(),
        },
    )?;

    let mut sources = SourceTexts::new();
    for source in std::iter::once(plan.source()).chain(plan.extended_sources()) {
        sources.insert(source.file_name.clone(), Arc::clone(source.snapshot()));
    }

    // The Rust API reads standard library declarations from this directory.
    let libraries = LibraryCatalog::typescript_7_1(std::path::absolute(library_directory)?);
    let limits = ProgramLoadLimits::new(
        1_000_000,         // source files
        2_000_000,         // import/reference edges
        256,               // dependency depth
        64 * 1024 * 1024,  // bytes per source file
        512 * 1024 * 1024, // total source bytes
    );
    // Override noEmit even when the config omits it or sets it to false.
    let prepared = load_config_program_with_no_emit_override(&host, &plan, &libraries, limits)?;

    // Retain source snapshots to translate diagnostic offsets after run()
    // consumes the prepared program. Arc cloning does not copy source text.
    for source in prepared.source_files() {
        sources.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }
    for source in prepared.auxiliary_files() {
        sources.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }

    let outcome = ProgramSession::new(prepared).run()?;
    // Options such as deprecations can be reported without preventing a check.
    let mut diagnostics = plan
        .option_diagnostics()
        .iter()
        .filter(|diagnostic| is_non_fatal_option_diagnostic(diagnostic))
        .cloned()
        .collect::<Vec<_>>();
    diagnostics.extend(outcome.into_diagnostics());
    Ok((diagnostics, sources))
}

fn diagnostic_json(diagnostic: &Diagnostic, sources: &SourceTexts) -> Value {
    let position = diagnostic.file_name.as_ref().and_then(|file| {
        sources
            .get(file)?
            .positions()
            .line_and_character_utf16(diagnostic.start?)
    });
    let mut message = String::new();
    append_message(&diagnostic.message, 0, &mut message);
    json!({
        "code": diagnostic.code(),
        "category": diagnostic.category().name(),
        "message": message,
        "file": diagnostic.file_name.as_ref().map(|name| name.to_string_lossy()),
        "start": diagnostic.start,
        "length": diagnostic.length,
        "line": position.map(|position| position.line + 1),
        "column": position.map(|position| position.character + 1),
    })
}

fn append_message(chain: &MessageChain, depth: usize, output: &mut String) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(&"  ".repeat(depth));
    output.push_str(&chain.text.to_string_lossy());
    for child in &chain.next {
        append_message(child, depth + 1, output);
    }
}
