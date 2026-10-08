//! Single-file JavaScript and declaration emit: TypeScript 7.1's
//! `transpile.TranspileModule` and `transpile.TranspileDeclaration`.
//!
//! tsgo-port: transpileWorker @7.1 (transpile/transpile.go:118-258)
//!
//! Both entries reach one worker: it clears the options that do not apply to
//! a single file, forces `noCheck`, `isolatedModules` (unless
//! `verbatimModuleSyntax`), `noResolve`, `suppressOutputPathCheck` and
//! `allowNonTsExtensions`, roots the input at `/` in an in-memory file system
//! (with a barebones default library for declarations), creates a Program
//! that skips module resolution and runs the ordinary emitter. The Rust
//! adapter does the same over the production loader, checker and emitter;
//! the session's emit route ([`EmitRouteKind`]) admits the forced options.

use std::path::PathBuf;

use tsc_diagnostics::{DiagnosticList, JsString};
use tsc_emitter::{EmitArtifactKind, EmitRouteKind, MemoryOutputSink};
use tsc_host::MemoryCompilerHost;
use tsc_program::{
    load_emitting_program, CompilerOptions, LibraryCatalog, ProgramLoadError, ProgramLoadLimits,
    ProgramOptions,
};

use crate::{DriverError, ProgramSession};

/// `barebonesLibContent`: the default library of declaration transpilation.
pub const BAREBONES_LIB_CONTENT: &str = "interface Boolean {}
interface Function {}
interface CallableFunction {}
interface NewableFunction {}
interface IArguments {}
interface Number {}
interface Object {}
interface RegExp {}
interface String {}
interface Array<T> { length: number; [n: number]: T; }
interface SymbolConstructor {
    (desc?: string | number): symbol;
    for(name: string): symbol;
    readonly toStringTag: symbol;
}
declare var Symbol: SymbolConstructor;
interface Symbol {
    readonly [Symbol.toStringTag]: string;
}";

/// `inputDirectory`: the current directory the input is rooted at.
const INPUT_DIRECTORY: &str = "/";
/// `libDirectory`: where the barebones default library lives.
const LIB_DIRECTORY: &str = "/lib";

/// `transpile.Options`.
#[derive(Clone, Debug, Default)]
pub struct TranspileOptions {
    /// The base compiler options (`nil`: empty options). The worker clears
    /// and forces some of them; see the module documentation.
    pub compiler_options: Option<CompilerOptions>,
    /// The name of the input file; empty or absent means `module.ts`, or
    /// `module.tsx` when `jsx` is set.
    pub file_name: Option<String>,
    /// Whether the syntactic and option diagnostics join the result. The
    /// diagnostics of the emit itself always do.
    pub report_diagnostics: bool,
}

/// `transpile.Output`.
#[derive(Clone, Debug)]
pub struct TranspileOutput {
    pub output_text: String,
    /// Located in the input file by its rooted name (`/<file name>`).
    pub diagnostics: DiagnosticList,
    /// The source map, when the options asked for a separate one.
    pub source_map_text: Option<String>,
}

/// The worker's assertions (`debug.Assert`) and the refusals of the Rust
/// loader and session.
#[derive(Debug)]
pub enum TranspileError {
    /// `debug.Assert(hasOutputText, "Output generation failed")`.
    OutputGenerationFailed {
        writes: Vec<(JsString, EmitArtifactKind)>,
    },
    /// "Unexpected multiple outputs" / "Unexpected multiple source map
    /// outputs".
    MultipleOutputs {
        writes: Vec<(JsString, EmitArtifactKind)>,
    },
    /// Rust-only: the virtual host rejected the input path.
    Host(String),
    /// Rust-only: the loader refused the input.
    Program(Box<ProgramLoadError>),
    /// Rust-only: the session or the emitter refused the request.
    Driver(Box<DriverError>),
}

impl std::fmt::Display for TranspileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutputGenerationFailed { .. } => formatter.write_str("Output generation failed"),
            Self::MultipleOutputs { .. } => formatter.write_str("Unexpected multiple outputs"),
            Self::Host(detail) => write!(formatter, "virtual host rejected the input: {detail}"),
            Self::Program(error) => write!(formatter, "program load failed: {error}"),
            Self::Driver(error) => write!(formatter, "driver failed: {error}"),
        }
    }
}

impl std::error::Error for TranspileError {}

/// tsgo-port: TranspileModule @7.1 (transpile/transpile.go:93-95)
pub fn transpile_module(
    input: &str,
    options: &TranspileOptions,
) -> Result<TranspileOutput, TranspileError> {
    transpile_worker(input, options, false)
}

/// tsgo-port: TranspileDeclaration @7.1 (transpile/transpile.go:114-116)
pub fn transpile_declaration(
    input: &str,
    options: &TranspileOptions,
) -> Result<TranspileOutput, TranspileError> {
    transpile_worker(input, options, true)
}

/// The options `transpileWorker` derives from the caller's
/// (transpile/transpile.go:119-166).
pub fn transpile_compiler_options(
    options: Option<&CompilerOptions>,
    declaration: bool,
) -> CompilerOptions {
    let mut options = options.cloned().unwrap_or_default();
    // Options that do not apply to single-file transpilation. `lib`,
    // `paths`, `rootDirs` and `types` are Program options in tsc-rs, which
    // the worker builds empty.
    options.incremental = None;
    options.declaration = None;
    options.emit_declaration_only = None;
    options.no_emit = None;
    options.lib = None;
    options.out_file = None;
    options.composite = None;
    options.ts_build_info_file = None;
    options.allow_importing_ts_extensions = None;
    options.no_emit_on_error = None;
    options.declaration_dir = None;
    if options.verbatim_module_syntax != Some(true) {
        options.isolated_modules = Some(true);
    }
    options.no_check = Some(true);
    options.no_resolve = Some(true);
    options.suppress_output_path_check = Some(true);
    options.allow_non_ts_extensions = Some(true);
    if declaration {
        options.declaration = Some(true);
        options.emit_declaration_only = Some(true);
        options.isolated_declarations = Some(true);
    } else {
        options.declaration = Some(false);
        options.declaration_map = Some(false);
        options.isolated_declarations = Some(false);
    }
    options
}

/// tsgo-port: transpileWorker @7.1 (transpile/transpile.go:118-258)
fn transpile_worker(
    input: &str,
    transpile_options: &TranspileOptions,
    declaration: bool,
) -> Result<TranspileOutput, TranspileError> {
    let options =
        transpile_compiler_options(transpile_options.compiler_options.as_ref(), declaration);

    let file_name = match transpile_options.file_name.as_deref() {
        Some(name) if !name.is_empty() => name,
        _ if options.jsx.is_some_and(|jsx| jsx != 0) => "module.tsx",
        _ => "module.ts",
    };
    let input_file_name = normalized_absolute_path(file_name);

    // The in-memory file system: the input and, for declarations, the
    // barebones library under the target's default library name.
    let catalog = LibraryCatalog::typescript_7_1(LIB_DIRECTORY);
    let mut host = MemoryCompilerHost::builder(INPUT_DIRECTORY)
        .case_sensitive(true)
        .file(input_file_name.clone(), input.as_bytes().to_vec());
    let mut program_options = ProgramOptions::default();
    if declaration {
        let library_file_name = catalog.default_file_name(&options);
        host = host.file(
            format!("{LIB_DIRECTORY}/{library_file_name}"),
            BAREBONES_LIB_CONTENT.as_bytes().to_vec(),
        );
        program_options = program_options
            .with_no_lib(false)
            .with_default_library_file_name(library_file_name);
    } else {
        program_options = program_options.with_no_lib(true);
    }
    let host = host
        .build()
        .map_err(|error| TranspileError::Host(format!("{error:?}")))?;
    let prepared = load_emitting_program(
        &host,
        &[PathBuf::from(&input_file_name)],
        options,
        program_options,
        &catalog,
        ProgramLoadLimits::new(4, 64, 4, 64 * 1024 * 1024, 64 * 1024 * 1024),
    )
    .map_err(|error| TranspileError::Program(Box::new(error)))?;

    let route = if declaration {
        EmitRouteKind::TranspileDeclaration
    } else {
        EmitRouteKind::TranspileJavaScript
    };
    let mut sink = MemoryOutputSink::new();
    let session = ProgramSession::new(prepared).with_emit_route(route);
    let outcome = if declaration {
        session.emit_forced_declarations_command_for_transpile(&mut sink)
    } else {
        session.emit_for_cli(&mut sink)
    }
    .map_err(|error| TranspileError::Driver(Box::new(error)))?;

    let mut diagnostics = DiagnosticList::new();
    if transpile_options.report_diagnostics {
        diagnostics.extend(outcome.syntactic_diagnostics.iter().cloned());
        diagnostics.extend(outcome.config_diagnostics.iter().cloned());
        diagnostics.extend(outcome.options_diagnostics.iter().cloned());
    }
    // The emit's own diagnostics (isolated declarations among them) always
    // join.
    diagnostics.extend(outcome.emit.diagnostics().iter().cloned());

    let writes = sink
        .writes()
        .iter()
        .map(|write| (write.path().to_owned(), write.kind()))
        .collect::<Vec<_>>();
    let mut output_text = None;
    let mut source_map_text = None;
    for write in sink.writes() {
        let slot = if write.path().ends_with(".map") {
            &mut source_map_text
        } else {
            &mut output_text
        };
        if slot.is_some() {
            return Err(TranspileError::MultipleOutputs { writes });
        }
        *slot = Some(write.callback_text().to_owned());
    }
    let Some(output_text) = output_text else {
        return Err(TranspileError::OutputGenerationFailed { writes });
    };
    Ok(TranspileOutput {
        output_text,
        diagnostics,
        source_map_text,
    })
}

/// `tspath.GetNormalizedAbsolutePath(fileName, "/")`.
fn normalized_absolute_path(file_name: &str) -> String {
    let rooted = if file_name.starts_with('/') {
        file_name.to_owned()
    } else {
        format!("{INPUT_DIRECTORY}{file_name}")
    };
    tsc_program::normalize_path(JsString::from(rooted.as_str()).as_js())
        .to_string_lossy()
        .into_owned()
}
