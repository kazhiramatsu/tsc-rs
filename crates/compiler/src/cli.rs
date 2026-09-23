//! Small, fail-closed command-line driver for the admitted compiler surface.
//!
//! The driver intentionally owns process concerns (argument selection,
//! current-directory discovery, diagnostic rendering, and exit status) while
//! [`tsc_program`] owns config conversion and program construction. Unsupported
//! flags and infrastructure failures return exit status 2. TypeScript's
//! no-emit program diagnostics return status 2 as well (the vendored driver
//! reports `DiagnosticsPresent_OutputsGenerated` because its no-emit emit
//! boundary is not marked skipped); command-line selection diagnostics such as
//! TS5112 retain status 1.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_diagnostics::{
    format_diagnostics_with_context, sort_and_dedupe_diagnostic_indices_with_context, Diagnostic,
    FormatDiagnosticsHost, MessageChain, TextSnapshot,
};
use tsc_diagnostics::{gen, JsStr, JsString};
use tsc_host::{CompilerHost, FsCompilerHost, HostError, ParallelSourceReader};
use tsc_program::{
    decode_host_text, is_non_fatal_option_diagnostic, load_config_program,
    load_config_program_with_no_emit_override,
    load_emitting_config_program_with_no_emit_override_and_overrides,
    load_emitting_config_program_with_overrides, load_emitting_program, load_program,
    parse_config_root_plan_with_cache, CompilerConfigHost, CompilerOptions,
    ConfigEmitOptionOverrides, ConfigExtendedCache, ConfigParseError, ConfigProgramLoadError,
    ConfigRootPlan, ConfigRootPlanRequest, LibraryCatalog, PreparedProgramMode, ProgramLoadLimits,
    ProgramOptions, WorkerBudget,
};

use crate::no_emit_canary::NoEmitCanary;
use crate::{
    CheckerBudget, EmitFileSystem, FsOutputSink, H2ActivityCounters, NoEmitActivityCounters,
    NoEmitWorkCounters, ProgramSession,
};

mod embedded_libraries {
    include!(concat!(env!("OUT_DIR"), "/typescript_6_0_3_libraries.rs"));
}

const EXIT_SUCCESS: i32 = 0;
const EXIT_COMMAND_LINE: i32 = 1;
const EXIT_DIAGNOSTIC: i32 = 2;
const EXIT_FAILURE: i32 = 2;
const CONFIG_FILE_NAME: &str = "tsconfig.json";
const TYPESCRIPT_VERSION: &str = "6.0.3";
type DiagnosticSourceMap = BTreeMap<JsString, Arc<TextSnapshot>>;
const DEFAULT_LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(
    1_000_000,
    2_000_000,
    256,
    64 * 1024 * 1024,
    512 * 1024 * 1024,
);

/// tsrs-native diagnostic control of the CLI's worker budget (see
/// [`WorkerBudget`]): `TSRS_WORKERS=<positive integer>` pins the budget
/// (clamped to the module cap), any other value or an unset variable selects
/// [`WorkerBudget::automatic`]. This is not a command-line option; every
/// budget produces identical diagnostics and output, so the variable exists
/// only for reproducible serial/worker-count measurements.
const WORKERS_ENV: &str = "TSRS_WORKERS";

fn cli_worker_budget() -> WorkerBudget {
    match std::env::var(WORKERS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<std::num::NonZeroUsize>().ok())
    {
        Some(workers) => WorkerBudget::new(workers),
        None => WorkerBudget::automatic(),
    }
}

/// tsrs-native diagnostic control of the CLI's checker budget (see
/// [`CheckerBudget`]): `TSRS_CHECKERS=<positive integer>` pins that many
/// checker states for the whole-Program check and emit (clamped to the module
/// cap and to the file count); `1` is the serial reference checker. Any other
/// value or an unset variable selects [`CheckerBudget::automatic`]. Not a
/// command-line option. One checker is the exact serial mode; a wider budget
/// keeps the sharded result unless `TSRS_ORDER_REPLAY=1` requests the serial
/// replay of an order-consuming run (see `CheckerBudget::with_order_replay`).
const CHECKERS_ENV: &str = "TSRS_CHECKERS";

fn cli_checker_budget() -> CheckerBudget {
    match std::env::var(CHECKERS_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<std::num::NonZeroUsize>().ok())
    {
        Some(checkers) => CheckerBudget::new(checkers),
        None => CheckerBudget::automatic(),
    }
    // The CLI process exits right after publishing: dropping the checker
    // states would only delay that.
    .with_leaked_states(true)
    .with_order_replay(tsc_checker::order_replay_requested())
}

/// The CLI's program load limits with its worker budget.
fn cli_limits() -> ProgramLoadLimits {
    DEFAULT_LIMITS.with_workers(cli_worker_budget())
}

/// Result of one CLI invocation. The binary writes the two streams and exits
/// with [`exit_code`](Self::exit_code); tests and embeddings can inspect the
/// result without spawning a child process.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliOutput {
    stdout: String,
    stderr: String,
    exit_code: i32,
    work_counters: NoEmitWorkCounters,
    no_emit_activity: NoEmitActivityCounters,
    h2_activity: H2ActivityCounters,
}

impl CliOutput {
    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    pub const fn exit_code(&self) -> i32 {
        self.exit_code
    }

    /// Program-session work is zero for version/usage/host failures that do
    /// not reach parsing. Qualification consumers use this accessor without
    /// changing the binary's stdout/stderr contract.
    pub const fn work_counters(&self) -> NoEmitWorkCounters {
        self.work_counters
    }

    /// H1 constructor/output-write observations for this CLI execution.
    pub const fn no_emit_activity(&self) -> NoEmitActivityCounters {
        self.no_emit_activity
    }

    /// H1 positive wiring counts plus one zero-until-admitted counter for
    /// every H2 runtime slice.
    pub const fn h2_activity(&self) -> H2ActivityCounters {
        self.h2_activity
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CliError {
    Usage(String),
    Host(String),
    Config(String),
    Load(String),
    Driver(String),
    Render(String),
}

struct CliRoute<'a> {
    pretty: bool,
    canary: &'a mut NoEmitCanary,
    output_filesystem: &'a mut dyn EmitFileSystem,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(detail) => write!(formatter, "{detail}"),
            Self::Host(detail) => write!(formatter, "filesystem host failure: {detail}"),
            Self::Config(detail) => write!(formatter, "config failure: {detail}"),
            Self::Load(detail) => write!(formatter, "program construction failure: {detail}"),
            Self::Driver(detail) => write!(formatter, "compiler failure: {detail}"),
            Self::Render(detail) => write!(formatter, "diagnostic rendering failure: {detail}"),
        }
    }
}

impl Error for CliError {}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CommandLine {
    project: Option<PathBuf>,
    files: Vec<PathBuf>,
    compiler_options: CompilerOptions,
    no_lib: Option<bool>,
    ignore_config: bool,
    pretty: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ConfigCommandLineOverrides {
    no_emit: Option<bool>,
    emit: ConfigEmitOptionOverrides,
}

#[derive(Default)]
struct NativeEmitFileSystem;

impl tsc_emitter::SharedEmitFileSystem for NativeEmitFileSystem {
    fn write_file(&self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString> {
        // This is the actual filesystem boundary; compiler path keys stay JS.
        let native = path.to_string_lossy();
        let native_path = Path::new(native.as_ref());
        fs::write(native_path, bytes).map_err(|error| stable_io_message(&error, "open", path))
    }

    fn create_directory(&self, path: JsStr<'_>) -> Result<(), JsString> {
        // This is the actual filesystem boundary; compiler path keys stay JS.
        let native = path.to_string_lossy();
        let native_path = Path::new(native.as_ref());
        match fs::create_dir(native_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && native_path.is_dir() => {
                Ok(())
            }
            Err(error) => Err(stable_io_message(&error, "mkdir", path)),
        }
    }

    fn directory_exists(&self, path: JsStr<'_>) -> bool {
        // This is the actual filesystem boundary; compiler path keys stay JS.
        let native = path.to_string_lossy();
        let native_path = Path::new(native.as_ref());
        native_path.is_dir()
    }
}

impl EmitFileSystem for NativeEmitFileSystem {
    fn shared(&self) -> Option<&dyn tsc_emitter::SharedEmitFileSystem> {
        Some(self)
    }

    fn write_file(&mut self, path: JsStr<'_>, bytes: &[u8]) -> Result<(), JsString> {
        // This is the actual filesystem boundary; compiler path keys stay JS.
        let native = path.to_string_lossy();
        let native_path = Path::new(native.as_ref());
        fs::write(native_path, bytes).map_err(|error| stable_io_message(&error, "open", path))
    }

    fn create_directory(&mut self, path: JsStr<'_>) -> Result<(), JsString> {
        // This is the actual filesystem boundary; compiler path keys stay JS.
        let native = path.to_string_lossy();
        let native_path = Path::new(native.as_ref());
        match fs::create_dir(native_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists && native_path.is_dir() => {
                Ok(())
            }
            Err(error) => Err(stable_io_message(&error, "mkdir", path)),
        }
    }

    fn directory_exists(&mut self, path: JsStr<'_>) -> bool {
        // This is the actual filesystem boundary; compiler path keys stay JS.
        let native = path.to_string_lossy();
        let native_path = Path::new(native.as_ref());
        native_path.is_dir()
    }
}

fn stable_io_message(error: &io::Error, operation: &str, path: JsStr<'_>) -> JsString {
    #[cfg(unix)]
    let known = match error.raw_os_error() {
        Some(2) => Some(("ENOENT", "no such file or directory")),
        Some(13) => Some(("EACCES", "permission denied")),
        Some(17) => Some(("EEXIST", "file already exists")),
        Some(20) => Some(("ENOTDIR", "not a directory")),
        Some(21) => Some(("EISDIR", "illegal operation on a directory")),
        Some(28) => Some(("ENOSPC", "no space left on device")),
        Some(30) => Some(("EROFS", "read-only file system")),
        _ => None,
    };
    #[cfg(not(unix))]
    let known: Option<(&str, &str)> = None;

    if let Some((code, detail)) = known {
        {
            let mut message = JsString::from(format!("{code}: {detail}, {operation} '"));
            message.push_js(path);
            message.push_str("'");
            message
        }
    } else {
        error.to_string().into()
    }
}

/// Production CLI host with an immutable, binary-owned TypeScript 6.0.3
/// standard-library directory. User/config/package paths retain ordinary
/// filesystem semantics; only exact immediate children of this private
/// directory are intercepted.
#[derive(Clone, Debug)]
struct CliCompilerHost {
    filesystem: FsCompilerHost,
    library_directory: PathBuf,
}

impl CliCompilerHost {
    fn new(filesystem: FsCompilerHost, current_directory: &Path) -> Self {
        Self {
            filesystem,
            library_directory: current_directory
                .join(".tsc-rs-embedded-569177652966bd52")
                .join(TYPESCRIPT_VERSION)
                .join("lib"),
        }
    }

    fn library_directory(&self) -> &Path {
        &self.library_directory
    }

    fn embedded_file_name<'a>(&self, path: &'a Path) -> Option<&'a str> {
        (path.parent() == Some(self.library_directory.as_path()))
            .then(|| path.file_name().and_then(|name| name.to_str()))
            .flatten()
    }

    fn embedded_bytes(&self, path: &Path) -> Option<&'static [u8]> {
        let name = self.embedded_file_name(path)?;
        embedded_libraries::TYPESCRIPT_6_0_3_LIBRARIES
            .binary_search_by_key(&name, |(candidate, _)| *candidate)
            .ok()
            .map(|index| embedded_libraries::TYPESCRIPT_6_0_3_LIBRARIES[index].1)
    }
}

impl CompilerHost for CliCompilerHost {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.filesystem.current_directory_js()
    }
    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        if let Some(path) = path
            .as_str()
            .map(Path::new)
            .filter(|path| self.embedded_file_name(path).is_some())
        {
            return Ok(self.embedded_bytes(path).map(<[u8]>::to_vec));
        }
        self.filesystem.read_file_js(path)
    }
    fn file_size_hint_js(&self, path: JsStr<'_>) -> Result<Option<u64>, HostError> {
        if let Some(path) = path
            .as_str()
            .map(Path::new)
            .filter(|path| self.embedded_file_name(path).is_some())
        {
            return Ok(self.embedded_bytes(path).map(|bytes| bytes.len() as u64));
        }
        self.filesystem.file_size_hint_js(path)
    }
    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if let Some(path) = path
            .as_str()
            .map(Path::new)
            .filter(|path| self.embedded_file_name(path).is_some())
        {
            return Ok(self.embedded_bytes(path).is_some());
        }
        self.filesystem.file_exists_js(path)
    }
    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(true);
        }
        self.filesystem.directory_exists_js(path)
    }
    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(embedded_libraries::TYPESCRIPT_6_0_3_LIBRARIES
                .iter()
                .map(|(name, _)| {
                    let mut entry = path.to_owned();
                    entry.push_str("/");
                    entry.push_str(name);
                    entry
                })
                .collect());
        }
        self.filesystem.read_directory_js(path)
    }
    fn read_directory_listing_js(
        &self,
        path: JsStr<'_>,
    ) -> Result<Vec<tsc_host::DirectoryListingEntry>, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(embedded_libraries::TYPESCRIPT_6_0_3_LIBRARIES
                .iter()
                .map(|(name, _)| {
                    let mut entry = path.to_owned();
                    entry.push_str("/");
                    entry.push_str(name);
                    tsc_host::DirectoryListingEntry {
                        path: entry,
                        kind: tsc_host::DirectoryListingKind::File,
                        symlink: false,
                    }
                })
                .collect());
        }
        self.filesystem.read_directory_listing_js(path)
    }
    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        if path
            .as_str()
            .is_some_and(|path| Path::new(path) == self.library_directory)
        {
            return Ok(Vec::new());
        }
        self.filesystem.get_directories_js(path)
    }
    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        if let Some(native) = path.as_str().map(Path::new) {
            if native == self.library_directory || self.embedded_bytes(native).is_some() {
                return Ok(Some(path.to_owned()));
            }
            if self.embedded_file_name(native).is_some() {
                return Ok(None);
            }
        }
        self.filesystem.realpath_js(path)
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.filesystem.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.filesystem.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        if self.embedded_file_name(path).is_some() {
            return Ok(self.embedded_bytes(path).map(<[u8]>::to_vec));
        }
        self.filesystem.read_file(path)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        if self.embedded_file_name(path).is_some() {
            return Ok(self.embedded_bytes(path).is_some());
        }
        self.filesystem.file_exists(path)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        if path == self.library_directory {
            return Ok(true);
        }
        self.filesystem.directory_exists(path)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if path == self.library_directory {
            return Ok(embedded_libraries::TYPESCRIPT_6_0_3_LIBRARIES
                .iter()
                .map(|(name, _)| path.join(name))
                .collect());
        }
        self.filesystem.read_directory(path)
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if path == self.library_directory {
            return Ok(Vec::new());
        }
        self.filesystem.get_directories(path)
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        if path == self.library_directory || self.embedded_bytes(path).is_some() {
            return Ok(Some(path.to_path_buf()));
        }
        if self.embedded_file_name(path).is_some() {
            return Ok(None);
        }
        self.filesystem.realpath(path)
    }

    /// Embedded library bytes are immutable; everything else delegates to the
    /// filesystem host's own answer.
    fn permits_source_read_ahead(&self) -> bool {
        self.filesystem.permits_source_read_ahead()
    }

    fn parallel_source_reader(&self) -> Option<&(dyn ParallelSourceReader + Sync)> {
        self.filesystem
            .parallel_source_reader()
            .is_some()
            .then_some(self as &(dyn ParallelSourceReader + Sync))
    }

    fn parallel_resolution_host(&self) -> Option<&(dyn CompilerHost + Sync)> {
        self.filesystem
            .parallel_resolution_host()
            .is_some()
            .then_some(self as &(dyn CompilerHost + Sync))
    }
}

impl ParallelSourceReader for CliCompilerHost {
    fn read_source_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        CompilerHost::read_file_js(self, path)
    }
}

/// Execute the bounded H0/H1 command-line surface.
pub fn run_cli(args: &[String]) -> CliOutput {
    // tsc's command line never requests suggestion diagnostics, so the
    // unused-identifier suggestion pass (checkUnusedIdentifiers behind
    // getSuggestionDiagnostics) is skipped; noUnusedLocals /
    // noUnusedParameters errors still run.
    tsc_checker::set_unused_identifier_suggestions(false);
    // executeCommandLine's host parses JSDoc with defaultJSDocParsingMode
    // (ParseForTypeErrors, _tsc.js:132784, 132867): TS/TSX comments only
    // when they contain @see or @link, JS/JSX comments always.
    tsc_program::set_default_js_doc_parsing_mode(crate::JSDocParsingMode::ParseForTypeErrors);
    let mut no_emit_canary = NoEmitCanary::new();
    let execute_started = std::time::Instant::now();
    let result = execute(args, &mut no_emit_canary);
    tsc_types::trace::mark("cli: execute", execute_started);
    // Measurement builds only (`perf-counters` feature): aggregate counters
    // are written to the sidecar file named by TSRS_PERF_COUNTERS (one
    // `name=value` or `name=unwired` line each); stdout/stderr stay exactly
    // the ordinary CLI streams. Never compiled into candidate binaries.
    #[cfg(feature = "perf-counters")]
    if let Some(path) = std::env::var_os("TSRS_PERF_COUNTERS") {
        let mut report = String::new();
        for (name, value) in tsc_types::perf::snapshot() {
            report.push_str(name);
            report.push('=');
            match value {
                Some(value) => report.push_str(&value.to_string()),
                None => report.push_str("unwired"),
            }
            report.push('\n');
        }
        // A failed sidecar write must not change the CLI outcome.
        let _ = std::fs::write(path, report);
    }
    match result {
        Ok(output) => output,
        Err(error) => CliOutput {
            stdout: String::new(),
            stderr: format!("tsc-rs: {error}\n"),
            exit_code: EXIT_FAILURE,
            work_counters: NoEmitWorkCounters::default(),
            no_emit_activity: NoEmitActivityCounters,
            h2_activity: H2ActivityCounters::default(),
        },
    }
}

fn execute(args: &[String], no_emit_canary: &mut NoEmitCanary) -> Result<CliOutput, CliError> {
    let prologue_started = std::time::Instant::now();
    let command_line = parse_arguments(args)?;
    if args.iter().any(|arg| arg == "--version") {
        return Ok(CliOutput {
            stdout: format!("Version {TYPESCRIPT_VERSION}\n"),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters: NoEmitWorkCounters::default(),
            no_emit_activity: NoEmitActivityCounters,
            h2_activity: H2ActivityCounters::default(),
        });
    }

    let filesystem = FsCompilerHost::from_process().map_err(host_error)?;
    let pretty = command_line.pretty.unwrap_or_else(default_pretty);
    let mut output_filesystem = NativeEmitFileSystem;
    let mut route = CliRoute {
        pretty,
        canary: no_emit_canary,
        output_filesystem: &mut output_filesystem,
    };
    let current_directory = filesystem.current_directory().map_err(host_error)?;
    let host = CliCompilerHost::new(filesystem, &current_directory);
    let catalog = LibraryCatalog::typescript_6_0_3(host.library_directory());
    tsc_types::trace::mark("cli: arguments, host, catalog", prologue_started);

    if let Some(project) = command_line.project.as_ref() {
        let config_file = match resolve_project_file(&host, &current_directory, project)? {
            Ok(config_file) => config_file,
            Err(ProjectFileError::MissingPath(path)) => {
                let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(&gen::The_specified_path_does_not_exist_0, &[path]),
                );
                return rendered_diagnostics_with_exit(
                    &current_directory,
                    &BTreeMap::new(),
                    &[diagnostic],
                    pretty,
                    EXIT_COMMAND_LINE,
                );
            }
            Err(ProjectFileError::MissingConfig(directory)) => {
                let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(
                        &gen::Cannot_find_a_tsconfig_json_file_at_the_specified_directory_0,
                        &[directory],
                    ),
                );
                return rendered_diagnostics_with_exit(
                    &current_directory,
                    &BTreeMap::new(),
                    &[diagnostic],
                    pretty,
                    EXIT_COMMAND_LINE,
                );
            }
        };
        let requested = absolutize(&current_directory, project);
        let config_display = if requested == config_file {
            project.clone()
        } else {
            project.join(CONFIG_FILE_NAME)
        };
        let config_started = std::time::Instant::now();
        let (plan, source_texts) = parse_config_file(
            &host,
            &current_directory,
            &config_file,
            Some(&config_display),
        )?;
        tsc_types::trace::mark("cli: project config plan", config_started);
        return execute_config(
            &host,
            &current_directory,
            &catalog,
            &plan,
            source_texts,
            config_command_line_overrides(&command_line),
            &mut route,
        );
    }

    if !command_line.files.is_empty() {
        if !command_line.ignore_config && find_config_file(&host, &current_directory)?.is_some() {
            let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(
                        &gen::tsconfig_json_is_present_but_will_not_be_loaded_if_files_are_specified_on_commandline_Use_ignoreConfig_to_skip_this_error,
                        &[],
                    ),
                );
            // TS5112 is fileless: the config is discovered with
            // `fileExists`, but TypeScript does not read or parse it
            // before rejecting explicit roots. Keep this branch free of
            // a second host read and of source-text ownership.
            let source_texts = BTreeMap::new();
            return rendered_diagnostics_with_exit(
                &current_directory,
                &source_texts,
                &[diagnostic],
                pretty,
                EXIT_COMMAND_LINE,
            );
        }
        // Keep the caller's spelling for root-file diagnostics. The program
        // loader normalizes these against the host cwd for identity and I/O,
        // while TypeScript reports a missing explicit root as it was written
        // on the command line (for example `missing.ts`, not its absolute
        // cwd-expanded path).
        return execute_explicit_files(
            &host,
            &current_directory,
            &catalog,
            &command_line.files,
            &command_line.compiler_options,
            command_line.no_lib,
            &mut route,
        );
    }

    if command_line.ignore_config {
        return Err(CliError::Usage(
            "--ignoreConfig requires explicit source files or -p".to_owned(),
        ));
    }

    let config_file = find_config_file(&host, &current_directory)?.ok_or_else(|| {
        CliError::Usage(format!(
            "cannot find {CONFIG_FILE_NAME} from {}",
            current_directory.display()
        ))
    })?;
    let (plan, source_texts) = parse_config_file(&host, &current_directory, &config_file, None)?;
    execute_config(
        &host,
        &current_directory,
        &catalog,
        &plan,
        source_texts,
        config_command_line_overrides(&command_line),
        &mut route,
    )
}

fn config_command_line_overrides(command_line: &CommandLine) -> ConfigCommandLineOverrides {
    let options = &command_line.compiler_options;
    ConfigCommandLineOverrides {
        no_emit: options.no_emit,
        emit: ConfigEmitOptionOverrides {
            target: options.target,
            module: options.module,
            use_define_for_class_fields: options.use_define_for_class_fields,
            no_emit_on_error: options.no_emit_on_error,
            emit_bom: options.emit_bom,
            new_line: options.new_line,
            list_emitted_files: options.list_emitted_files,
            no_lib: command_line.no_lib,
        },
    }
}

fn parse_arguments(args: &[String]) -> Result<CommandLine, CliError> {
    let mut command_line = CommandLine {
        pretty: None,
        ..CommandLine::default()
    };
    let mut index = 0usize;
    let mut end_options = false;
    while index < args.len() {
        let argument = &args[index];
        if end_options {
            command_line.files.push(PathBuf::from(argument));
            index += 1;
            continue;
        }
        match argument.as_str() {
            "--" => {
                end_options = true;
                index += 1;
            }
            "--version" | "-v" => {
                index += 1;
            }
            "--noEmit" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.compiler_options.no_emit = Some(value);
                index = next_index;
            }
            value if value.starts_with("--noEmit=") => {
                command_line.compiler_options.no_emit = Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "--target" => {
                let (value, next_index) = required_option_value(args, index)?;
                command_line.compiler_options.target = Some(parse_target(value)?);
                index = next_index;
            }
            value if value.starts_with("--target=") => {
                command_line.compiler_options.target =
                    Some(parse_target(inline_value(value, "--target")?)?);
                index += 1;
            }
            "--module" => {
                let (value, next_index) = required_option_value(args, index)?;
                command_line.compiler_options.module = Some(parse_module(value)?);
                index = next_index;
            }
            value if value.starts_with("--module=") => {
                command_line.compiler_options.module =
                    Some(parse_module(inline_value(value, "--module")?)?);
                index += 1;
            }
            "--newLine" => {
                let (value, next_index) = required_option_value(args, index)?;
                command_line.compiler_options.new_line = Some(parse_new_line(value)?);
                index = next_index;
            }
            value if value.starts_with("--newLine=") => {
                command_line.compiler_options.new_line =
                    Some(parse_new_line(inline_value(value, "--newLine")?)?);
                index += 1;
            }
            "--listEmittedFiles" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.compiler_options.list_emitted_files = Some(value);
                index = next_index;
            }
            value if value.starts_with("--listEmittedFiles=") => {
                command_line.compiler_options.list_emitted_files =
                    Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "--emitBOM" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.compiler_options.emit_bom = Some(value);
                index = next_index;
            }
            value if value.starts_with("--emitBOM=") => {
                command_line.compiler_options.emit_bom = Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "--noEmitOnError" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.compiler_options.no_emit_on_error = Some(value);
                index = next_index;
            }
            value if value.starts_with("--noEmitOnError=") => {
                command_line.compiler_options.no_emit_on_error = Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "--useDefineForClassFields" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.compiler_options.use_define_for_class_fields = Some(value);
                index = next_index;
            }
            value if value.starts_with("--useDefineForClassFields=") => {
                command_line.compiler_options.use_define_for_class_fields =
                    Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "--noLib" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.no_lib = Some(value);
                index = next_index;
            }
            value if value.starts_with("--noLib=") => {
                command_line.no_lib = Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "--ignoreConfig" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.ignore_config = value;
                index = next_index;
            }
            value if value.starts_with("--ignoreConfig=") => {
                command_line.ignore_config = parse_inline_boolean(value)?;
                index += 1;
            }
            "--pretty" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.pretty = Some(value);
                index = next_index;
            }
            value if value.starts_with("--pretty=") => {
                command_line.pretty = Some(parse_inline_boolean(value)?);
                index += 1;
            }
            "-p" | "--project" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CliError::Usage(format!("{argument} expects a config file or directory"))
                })?;
                if value.starts_with('-') {
                    return Err(CliError::Usage(format!(
                        "{argument} expects a config file or directory, got {value:?}"
                    )));
                }
                if command_line.project.replace(PathBuf::from(value)).is_some() {
                    return Err(CliError::Usage(
                        "the project option may be specified only once".to_owned(),
                    ));
                }
                index += 2;
            }
            value if value.starts_with("-p=") || value.starts_with("--project=") => {
                let (_, value) = value.split_once('=').expect("project option has an equals");
                if value.is_empty() {
                    return Err(CliError::Usage(
                        "the project option requires a config file or directory".to_owned(),
                    ));
                }
                if command_line.project.replace(PathBuf::from(value)).is_some() {
                    return Err(CliError::Usage(
                        "the project option may be specified only once".to_owned(),
                    ));
                }
                index += 1;
            }
            value if value.starts_with('-') => {
                return Err(CliError::Usage(format!("unsupported option {value:?}")));
            }
            value => {
                command_line.files.push(PathBuf::from(value));
                index += 1;
            }
        }
    }
    if command_line.project.is_some() && !command_line.files.is_empty() {
        return Err(CliError::Usage(
            "project selection cannot be combined with explicit source files".to_owned(),
        ));
    }
    Ok(command_line)
}

/// TypeScript's command-line parser consumes a separate `true`/`false` token
/// for boolean switches. Keep the no-emit surface compatible while leaving
/// arbitrary following paths available as explicit roots.
fn consume_boolean_value(args: &[String], index: usize, default: bool) -> (bool, usize) {
    match args.get(index + 1).map(String::as_str) {
        Some("true") => (true, index + 2),
        Some("false") => (false, index + 2),
        _ => (default, index + 1),
    }
}

fn required_option_value(args: &[String], index: usize) -> Result<(&str, usize), CliError> {
    let option = &args[index];
    let value = args
        .get(index + 1)
        .ok_or_else(|| CliError::Usage(format!("{option} expects a value")))?;
    if value.starts_with('-') {
        return Err(CliError::Usage(format!(
            "{option} expects a value, got {value:?}"
        )));
    }
    Ok((value, index + 2))
}

fn inline_value<'a>(argument: &'a str, option: &str) -> Result<&'a str, CliError> {
    let (_, value) = argument
        .split_once('=')
        .expect("caller selected an equals-form option");
    if value.is_empty() {
        Err(CliError::Usage(format!("{option} expects a value")))
    } else {
        Ok(value)
    }
}

fn parse_inline_boolean(argument: &str) -> Result<bool, CliError> {
    let (option, value) = argument
        .split_once('=')
        .expect("caller selected an equals-form boolean option");
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(CliError::Usage(format!(
            "{option} expects 'true' or 'false', got {value:?}"
        ))),
    }
}

fn parse_target(value: &str) -> Result<i32, CliError> {
    // Retain the existing CLI alias; all TypeScript spellings come from the
    // same catalog as tsconfig conversion.
    if value.eq_ignore_ascii_case("latest") {
        return Ok(99);
    }
    parse_named_emit_option("target", value)
}

fn parse_module(value: &str) -> Result<i32, CliError> {
    parse_named_emit_option("module", value)
}

fn parse_named_emit_option(name: &str, value: &str) -> Result<i32, CliError> {
    tsc_program::compiler_option_declaration(name)
        .and_then(|option| option.value_kind().named_value(value))
        .ok_or_else(|| CliError::Usage(format!("--{name} has an unknown value {value:?}")))
}

fn parse_new_line(value: &str) -> Result<i32, CliError> {
    if value.eq_ignore_ascii_case("crlf") {
        Ok(0)
    } else if value.eq_ignore_ascii_case("lf") {
        Ok(1)
    } else {
        Err(CliError::Usage(format!(
            "--newLine expects 'crlf' or 'lf', got {value:?}"
        )))
    }
}

fn execute_config(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    plan: &ConfigRootPlan,
    mut source_texts: DiagnosticSourceMap,
    overrides: ConfigCommandLineOverrides,
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    for source in plan.extended_sources() {
        source_texts.insert(source.file_name.clone(), Arc::clone(source.snapshot()));
    }
    source_texts.insert(
        plan.source().file_name.clone(),
        Arc::clone(plan.source().snapshot()),
    );
    let effective_no_emit = overrides
        .no_emit
        .unwrap_or_else(|| plan.compiler_options().no_emit == Some(true));
    if effective_no_emit && !overrides.emit.is_empty() {
        return Err(CliError::Usage(
            "emit-profile command-line overrides are unavailable on the preserved --noEmit route"
                .to_owned(),
        ));
    }
    let limits = cli_limits();
    let load_started = std::time::Instant::now();
    let prepared = match overrides.no_emit {
        Some(true) => load_config_program_with_no_emit_override(host, plan, catalog, limits),
        Some(false) => load_emitting_config_program_with_no_emit_override_and_overrides(
            host,
            plan,
            catalog,
            limits,
            overrides.emit,
        ),
        None if plan.compiler_options().no_emit == Some(true) => {
            load_config_program(host, plan, catalog, limits)
        }
        None => {
            load_emitting_config_program_with_overrides(host, plan, catalog, limits, overrides.emit)
        }
    };
    tsc_types::trace::mark("load program", load_started);
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(ConfigProgramLoadError::Diagnostics { config, options }) => {
            let mut diagnostics = config;
            diagnostics.extend(options);
            return rendered_diagnostics(
                current_directory,
                &source_texts,
                &diagnostics,
                route.pretty,
            );
        }
        Err(ConfigProgramLoadError::NoEmitRequired { value }) => {
            return Err(CliError::Load(format!(
                "compilerOptions.noEmit must be true (observed {value:?}); pass --noEmit to override"
            )));
        }
        Err(ConfigProgramLoadError::EmitRequired { value }) => {
            return Err(CliError::Load(format!(
                "compilerOptions.noEmit must be absent or false for emission (observed {value:?}); pass --noEmit=false to override"
            )));
        }
        Err(ConfigProgramLoadError::Program(error)) => {
            return Err(CliError::Load(error.to_string()))
        }
    };
    for source in prepared.source_files() {
        source_texts.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }
    for source in prepared.auxiliary_files() {
        source_texts.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }
    let option_diagnostics = plan
        .option_diagnostics()
        .iter()
        // Emitting config programs validate effective options themselves so
        // noEmitOnError sees them. Only the no-emit route retains plan ownership.
        .filter(|_| prepared.mode() == PreparedProgramMode::NoEmit)
        .filter(|diagnostic| is_non_fatal_option_diagnostic(diagnostic))
        .cloned()
        .collect::<Vec<_>>();
    execute_prepared(
        current_directory,
        source_texts,
        prepared,
        &option_diagnostics,
        route,
    )
}

fn execute_explicit_files(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    roots: &[PathBuf],
    compiler_options: &CompilerOptions,
    no_lib: Option<bool>,
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    let options = compiler_options.clone();
    let program_options = no_lib
        .map(|value| ProgramOptions::default().with_no_lib(value))
        .unwrap_or_default();
    let limits = cli_limits();
    let prepared = if options.no_emit == Some(true) {
        load_program(host, roots, options, program_options, catalog, limits)
    } else {
        load_emitting_program(host, roots, options, program_options, catalog, limits)
    }
    .map_err(|error| CliError::Load(error.to_string()))?;
    let mut source_texts = BTreeMap::new();
    for source in prepared.source_files() {
        source_texts.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }
    execute_prepared(current_directory, source_texts, prepared, &[], route)
}

fn execute_prepared(
    current_directory: &Path,
    source_texts: DiagnosticSourceMap,
    prepared: tsc_program::PreparedProgram,
    additional_diagnostics: &[Diagnostic],
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    if prepared.mode() == PreparedProgramMode::Emit {
        return execute_emitting_prepared(
            current_directory,
            source_texts,
            prepared,
            additional_diagnostics,
            route,
        );
    }
    let session_started = std::time::Instant::now();
    // tsc emitFilesAndReportErrors (_tsc.js:129433-129440): a --noEmit
    // command with getEmitDeclarations(options) reports the declaration
    // diagnostics after the semantic pass, only while nothing beyond the
    // config-file parsing diagnostics was reported. The command session runs
    // that getter over its own checker sessions
    // (`ProgramSession::run_no_emit_command`).
    let outcome = ProgramSession::new(prepared)
        .with_worker_budget(cli_worker_budget())
        .with_checker_budget(cli_checker_budget())
        .with_leaked_program(true)
        .run_with_no_emit_canary(
            false,
            tsc_checker::LibraryPrefixCompletion::Complete,
            true,
            route.canary,
        )
        .map_err(|error| CliError::Driver(error.to_string()))?;
    tsc_types::trace::mark("check session", session_started);
    tsc_checker::line_profile::write_report();
    // Config-owned non-fatal option rows are supplied separately from the
    // prepared program. Insert them at the same bucket boundary as
    // `getOptionsDiagnostics`, before global and semantic rows; appending
    // them after `into_diagnostics` would make TS5107 appear after semantic
    // diagnostics and would violate the command-line ordering contract.
    let mut diagnostics = Vec::new();
    diagnostics.extend(outcome.config_diagnostics().iter().cloned());
    diagnostics.extend(outcome.syntactic_diagnostics().iter().cloned());
    if outcome.syntactic_diagnostics().is_empty() {
        diagnostics.extend(outcome.options_diagnostics().iter().cloned());
        diagnostics.extend(additional_diagnostics.iter().cloned());
        diagnostics.extend(outcome.global_diagnostics().iter().cloned());
        if outcome.options_diagnostics().is_empty()
            && additional_diagnostics.is_empty()
            && outcome.global_diagnostics().is_empty()
        {
            diagnostics.extend(outcome.semantic_diagnostics().iter().cloned());
        }
    }
    // The command's own option rows (`additional_diagnostics`) close the
    // declaration gate as well; the session could not see them.
    if diagnostics.len() == outcome.config_diagnostics().len() {
        diagnostics.extend(outcome.declaration_diagnostics().iter().cloned());
    }
    let work_counters = outcome.work_counters();
    let no_emit_activity = outcome.no_emit_activity();
    let render_started = std::time::Instant::now();
    let rendered = rendered_diagnostics_with_work(
        current_directory,
        &source_texts,
        &diagnostics,
        route.pretty,
        work_counters,
        no_emit_activity,
    );
    tsc_types::trace::mark("cli: render diagnostics", render_started);
    rendered
}

/// Shared command producer for real CLI execution and scoped Program emits.
pub(crate) fn emit_command_status(
    current_directory: JsStr<'_>,
    emit: &crate::EmitOutcome,
    diagnostics: &[Diagnostic],
) -> (Vec<JsString>, i32) {
    let status_writes = emit
        .emitted_files()
        .unwrap_or_default()
        .iter()
        .map(|path| {
            let absolute = tsc_program::canonical_emit_path(path.as_js(), current_directory, true);
            {
                let mut status = JsString::from("TSFILE: ");
                status.push_js(absolute.as_js());
                status
            }
        })
        .collect::<Vec<_>>();

    // tsc-port: emitFilesAndReportErrorsAndGetExitStatus @6.0.3
    // tsc-hash: accac089a63c276079dd3309c69c617169dac0a0578c1551c8ea8a273d22bb78
    // tsc-span: _tsc.js:129468-129485
    let exit_code = if emit.emit_skipped() && !diagnostics.is_empty() {
        EXIT_COMMAND_LINE
    } else if !diagnostics.is_empty() {
        EXIT_DIAGNOSTIC
    } else {
        EXIT_SUCCESS
    };
    (status_writes, exit_code)
}

fn execute_emitting_prepared(
    current_directory: &Path,
    source_texts: DiagnosticSourceMap,
    prepared: tsc_program::PreparedProgram,
    additional_diagnostics: &[Diagnostic],
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    // The real filesystem is stateless: its artifacts are written on the
    // worker budget; an injected (observing) filesystem keeps ordered writes.
    let write_workers = cli_worker_budget().max_workers();
    let mut shared_sink;
    let mut ordered_sink;
    let sink: &mut dyn tsc_emitter::OutputSink =
        if let Some(shared) = route.output_filesystem.shared() {
            shared_sink = tsc_emitter::SharedFsOutputSink::new(shared, write_workers);
            &mut shared_sink
        } else {
            ordered_sink = FsOutputSink::new(route.output_filesystem);
            &mut ordered_sink
        };
    let session_started = std::time::Instant::now();
    let outcome = ProgramSession::new(prepared)
        .with_worker_budget(cli_worker_budget())
        .with_checker_budget(cli_checker_budget())
        .with_leaked_program(true)
        .emit_for_cli(sink)
        .map_err(|error| CliError::Driver(error.to_string()))?;
    tsc_types::trace::mark("check + emit session", session_started);
    tsc_checker::line_profile::write_report();

    let (emit, diagnostics, work_counters) = outcome.into_reported(additional_diagnostics);

    let (status_writes, exit_code) = emit_command_status(
        current_directory
            .to_str()
            .expect("prepared CLI cwd is Unicode")
            .into(),
        &emit,
        &diagnostics,
    );
    rendered_diagnostics_with_exit_work_status_and_h2(
        current_directory,
        &source_texts,
        &diagnostics,
        route.pretty,
        exit_code,
        work_counters,
        NoEmitActivityCounters,
        emit.h2_activity(),
        &status_writes,
    )
}

fn rendered_diagnostics(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_work(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        NoEmitWorkCounters::default(),
        NoEmitActivityCounters,
    )
}

fn rendered_diagnostics_with_work(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    work_counters: NoEmitWorkCounters,
    no_emit_activity: NoEmitActivityCounters,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_and_work(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        EXIT_DIAGNOSTIC,
        work_counters,
        no_emit_activity,
    )
}

fn rendered_diagnostics_with_exit(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_and_work(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        exit_code,
        NoEmitWorkCounters::default(),
        NoEmitActivityCounters,
    )
}

fn rendered_diagnostics_with_exit_and_work(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
    work_counters: NoEmitWorkCounters,
    no_emit_activity: NoEmitActivityCounters,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_work_and_status(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        exit_code,
        work_counters,
        no_emit_activity,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
fn rendered_diagnostics_with_exit_work_and_status(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
    work_counters: NoEmitWorkCounters,
    no_emit_activity: NoEmitActivityCounters,
    status_writes: &[JsString],
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_work_status_and_h2(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        exit_code,
        work_counters,
        no_emit_activity,
        H2ActivityCounters::default(),
        status_writes,
    )
}

#[allow(clippy::too_many_arguments)]
fn rendered_diagnostics_with_exit_work_status_and_h2(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
    work_counters: NoEmitWorkCounters,
    no_emit_activity: NoEmitActivityCounters,
    h2_activity: H2ActivityCounters,
    status_writes: &[JsString],
) -> Result<CliOutput, CliError> {
    if diagnostics.is_empty() && status_writes.is_empty() {
        return Ok(CliOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters,
            no_emit_activity,
            h2_activity,
        });
    }
    if diagnostics.is_empty() {
        let mut stdout = JsString::new();
        append_status_writes(&mut stdout, status_writes);
        return Ok(CliOutput {
            stdout: stdout.to_string_lossy().into_owned(),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters,
            no_emit_activity,
            h2_activity,
        });
    }
    let current_directory = current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let host = FormatDiagnosticsHost::from_js_snapshots(current_directory.into(), source_texts);
    let text = if pretty {
        let mut text = format_diagnostics_with_context(diagnostics, &host)
            .map_err(|error| CliError::Render(error.to_string()))?;
        append_status_writes(&mut text, status_writes);
        append_pretty_error_summary(
            &mut text,
            diagnostics,
            &host,
            source_texts,
            current_directory,
        );
        colorize_pretty_output(text.to_string_lossy().as_ref())
    } else {
        let mut text =
            format_plain_diagnostics(diagnostics, &host, source_texts, current_directory)
                .map_err(|error| CliError::Render(error.to_string()))?;
        append_status_writes(&mut text, status_writes);
        text.to_string_lossy().into_owned()
    };
    Ok(CliOutput {
        stdout: text,
        stderr: String::new(),
        exit_code,
        work_counters,
        no_emit_activity,
        h2_activity,
    })
}

fn append_status_writes(output: &mut JsString, status_writes: &[JsString]) {
    for status in status_writes {
        output.push_js(status.as_js());
        output.push('\n');
    }
}

/// Append the command-line reporter's contextual error summary. Plain output
/// intentionally omits this block, matching TypeScript's non-pretty reporter.
/// The per-file counts are derived from the same sorted/deduplicated view used
/// by the formatter, so the summary cannot count an occurrence which was not
/// printed above.
fn append_pretty_error_summary(
    output: &mut JsString,
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
    source_texts: &DiagnosticSourceMap,
    current_directory: &str,
) {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let mut file_counts = BTreeMap::<JsString, (usize, u32)>::new();
    let mut total = 0usize;
    for index in indices {
        let diagnostic = &diagnostics[index];
        if diagnostic.category().name() != "error"
            || is_command_line_selection_diagnostic(diagnostic.code())
        {
            continue;
        }
        let file_name = diagnostic.file_name.as_ref().map(JsString::as_js);
        let Some(file_name) = file_name else {
            total += 1;
            continue;
        };
        total += 1;
        let display_name = relative_file_name(
            file_name,
            current_directory,
            process_case_sensitive_file_names(),
        );
        let line = diagnostic
            .start
            .and_then(|start| {
                source_texts
                    .get(file_name.as_bytes())
                    .or_else(|| {
                        let normalized = normalize_slashes(file_name);
                        source_texts
                            .iter()
                            .find(|(candidate, _)| normalize_slashes(*candidate) == normalized)
                            .map(|(_, text)| text)
                    })
                    .and_then(|snapshot| {
                        snapshot
                            .positions()
                            .line_and_character_utf16(start)
                            .map(|location| location.line + 1)
                    })
            })
            .unwrap_or(1);
        file_counts
            .entry(display_name)
            .and_modify(|entry| {
                entry.0 += 1;
                entry.1 = entry.1.min(line);
            })
            .or_insert((1, line));
    }
    if total == 0 {
        return;
    }

    output.push_str("\n\n");
    let noun = if total == 1 { "error" } else { "errors" };
    match (total, file_counts.len()) {
        (1, 0) => output.push_str("Found 1 error.\n"),
        (1, 1) => {
            let (file, (_, line)) = file_counts.iter().next().expect("one file exists");
            output.push_str("Found 1 error in ");
            output.push_js(file.as_js());
            output.push_str(&format!(":{line}\n"));
        }
        (_, 0) => output.push_str(&format!("Found {total} {noun}.\n")),
        (_, 1) => {
            let (file, (_, line)) = file_counts.iter().next().expect("one file exists");
            output.push_str(&format!(
                "Found {total} {noun} in the same file, starting at: "
            ));
            output.push_js(file.as_js());
            output.push_str(&format!(":{line}\n"));
        }
        (_, file_count) => {
            output.push_str(&format!("Found {total} {noun} in {file_count} files.\n\n"));
            output.push_str("Errors  Files\n");
            let mut files: Vec<_> = file_counts.into_iter().collect();
            files.sort_by(|a, b| a.0.cmp_utf16(b.0.as_js()));
            for (file, (count, line)) in files {
                output.push_str(&format!("{count:>6}  "));
                output.push_js(file.as_js());
                output.push_str(&format!(":{line}\n"));
            }
        }
    }
    output.push('\n');
}

const ANSI_RESET: &str = "\u{1b}[0m";
const ANSI_GRAY: &str = "\u{1b}[90m";
const ANSI_CYAN: &str = "\u{1b}[96m";
const ANSI_YELLOW: &str = "\u{1b}[93m";
const ANSI_RED: &str = "\u{1b}[91m";
const ANSI_REVERSE: &str = "\u{1b}[7m";

/// Add the ANSI layer owned by TypeScript's pretty command-line reporter.
///
/// The shared diagnostics renderer intentionally remains color-free because
/// its output is also consumed by conformance and JSONL adapters. CLI pretty
/// output applies the small, stable ANSI vocabulary after the common text and
/// context layout has been selected, which keeps plain and pretty sorting
/// byte-identical apart from styling.
fn colorize_pretty_output(input: &str) -> String {
    let mut output = String::with_capacity(input.len() + input.len() / 2);
    let mut context = None;
    let mut previous_fileless_diagnostic = false;
    let mut previous_context_line = false;
    for line in input.split_inclusive('\n') {
        let (line, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |line| (line, "\n"));
        if let Some(colored) = colorize_header(line) {
            if previous_fileless_diagnostic || previous_context_line {
                output.push('\n');
            }
            output.push_str(&colored);
            output.push_str(newline);
            context = category_context_color(line).map(|color| (color, 0));
            previous_fileless_diagnostic = false;
            previous_context_line = false;
        } else if let Some(colored) = colorize_related_location(line) {
            output.push_str(&colored);
            output.push_str(newline);
            context = Some((ANSI_CYAN, 4));
            previous_fileless_diagnostic = false;
            previous_context_line = false;
        } else if let Some(colored) = colorize_fileless_diagnostic(line) {
            output.push_str(&colored);
            output.push_str(newline);
            context = None;
            previous_fileless_diagnostic = true;
            previous_context_line = false;
        } else if line.starts_with("Found ") {
            output.push_str(&colorize_summary(line));
            output.push_str(newline);
            context = None;
            previous_fileless_diagnostic = false;
            previous_context_line = false;
        } else if let Some((squiggle_color, indent)) = context {
            output.push_str(&colorize_context_line(line, squiggle_color, indent));
            output.push_str(newline);
            previous_fileless_diagnostic = false;
            previous_context_line = true;
        } else {
            output.push_str(line);
            output.push_str(newline);
            previous_fileless_diagnostic = false;
            previous_context_line = false;
        }
    }
    output
}

fn category_context_color(line: &str) -> Option<&'static str> {
    let (_, detail) = line.split_once(" - ")?;
    let (category, _) = detail.split_once(" TS")?;
    match category {
        "error" => Some(ANSI_RED),
        "warning" => Some(ANSI_YELLOW),
        "suggestion" => Some("\u{1b}[92m"),
        "message" => Some(ANSI_CYAN),
        _ => None,
    }
}

fn colorize_related_location(line: &str) -> Option<String> {
    let location = line.strip_prefix("  ")?;
    let mut location_parts = location.rsplitn(3, ':');
    let character = location_parts.next()?;
    let line_number = location_parts.next()?;
    let file_name = location_parts.next()?;
    if file_name.is_empty()
        || line_number.is_empty()
        || character.is_empty()
        || !line_number.bytes().all(|byte| byte.is_ascii_digit())
        || !character.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(format!(
        "  {ANSI_CYAN}{file_name}{ANSI_RESET}:{ANSI_YELLOW}{line_number}{ANSI_RESET}:{ANSI_YELLOW}{character}{ANSI_RESET}"
    ))
}

fn colorize_fileless_diagnostic(line: &str) -> Option<String> {
    let (category, detail) = line.split_once(" TS")?;
    let color = match category {
        "error" if !is_command_line_selection_line(line) => ANSI_RED,
        "warning" if !is_command_line_selection_line(line) => ANSI_YELLOW,
        "suggestion" if !is_command_line_selection_line(line) => "\u{1b}[92m",
        "message" if !is_command_line_selection_line(line) => ANSI_CYAN,
        _ => return None,
    };
    let (code, message) = detail.split_once(": ")?;
    if code.is_empty() || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(format!(
        "{color}{category}{ANSI_RESET}{ANSI_GRAY} TS{code}: {ANSI_RESET}{message}"
    ))
}

fn is_command_line_selection_line(line: &str) -> bool {
    line.split_once(" TS")
        .and_then(|(_, detail)| detail.split_once(": "))
        .and_then(|(code, _)| code.parse::<u32>().ok())
        .is_some_and(is_command_line_selection_diagnostic)
}

fn is_command_line_selection_diagnostic(code: u32) -> bool {
    matches!(code, 5057 | 5058 | 5112)
}

fn colorize_header(line: &str) -> Option<String> {
    let (location, detail) = line.split_once(" - ")?;
    let mut location_parts = location.rsplitn(3, ':');
    let character = location_parts.next()?;
    let line_number = location_parts.next()?;
    let file_name = location_parts.next()?;
    if file_name.is_empty()
        || line_number.is_empty()
        || character.is_empty()
        || !line_number.bytes().all(|byte| byte.is_ascii_digit())
        || !character.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let (category, message) = detail.split_once(" TS")?;
    let category_color = match category {
        "error" => ANSI_RED,
        "warning" => ANSI_YELLOW,
        "suggestion" => "\u{1b}[92m",
        "message" => ANSI_CYAN,
        _ => return None,
    };
    let (code, message) = message.split_once(": ")?;
    Some(format!(
        "{ANSI_CYAN}{file_name}{ANSI_RESET}:{ANSI_YELLOW}{line_number}{ANSI_RESET}:{ANSI_YELLOW}{character}{ANSI_RESET} - {category_color}{category}{ANSI_RESET}{ANSI_GRAY} TS{code}: {ANSI_RESET}{message}"
    ))
}

fn colorize_context_line(line: &str, squiggle_color: &str, indent: usize) -> String {
    if line.is_empty() {
        return String::new();
    }
    if line.len() < indent || !line[..indent].bytes().all(|byte| byte == b' ') {
        return line.to_owned();
    }
    let (indent_text, context_line) = line.split_at(indent);
    if line.bytes().all(|byte| byte == b' ') {
        if let Some((first, rest)) = context_line.split_at_checked(1) {
            if let Some((plain, red_rest)) = rest.split_at_checked(1) {
                return format!(
                    "{indent_text}{ANSI_REVERSE}{first}{ANSI_RESET}{plain}{squiggle_color}{red_rest}{ANSI_RESET}"
                );
            }
        }
    }
    if let Some(first_tilde) = line.find('~') {
        if line[first_tilde..].bytes().all(|byte| byte == b'~') {
            let (prefix, marks) = line.split_at(first_tilde);
            let Some(prefix) = prefix.strip_prefix(indent_text) else {
                return line.to_owned();
            };
            if let Some((first, rest)) = prefix.split_at_checked(1) {
                if let Some((plain, red_rest)) = rest.split_at_checked(1) {
                    return format!(
                        "{indent_text}{ANSI_REVERSE}{first}{ANSI_RESET}{plain}{squiggle_color}{red_rest}{marks}{ANSI_RESET}"
                    );
                }
            }
        }
    }
    let gutter_start = context_line
        .bytes()
        .take_while(|byte| *byte == b' ')
        .count();
    let digit_start = indent + gutter_start;
    let Some(first) = line.as_bytes().get(digit_start) else {
        return line.to_owned();
    };
    if !first.is_ascii_digit() {
        return line.to_owned();
    }
    let digit_end = line[digit_start..]
        .find(|character: char| !character.is_ascii_digit())
        .map_or(line.len(), |offset| digit_start + offset);
    if digit_end == digit_start || line[digit_end..].is_empty() {
        return line.to_owned();
    }
    format!(
        "{}{ANSI_REVERSE}{}{ANSI_RESET}{}",
        indent_text,
        &line[indent..digit_end],
        &line[digit_end..]
    )
}

fn colorize_summary(line: &str) -> String {
    let Some((prefix, line_number)) = line.rsplit_once(':') else {
        return line.to_owned();
    };
    if line_number.is_empty() || !line_number.bytes().all(|byte| byte.is_ascii_digit()) {
        return line.to_owned();
    }
    format!("{prefix}{ANSI_GRAY}:{line_number}{ANSI_RESET}")
}

fn default_pretty() -> bool {
    std::io::stdout().is_terminal()
}

/// Format the command-line's non-contextual reporter.
///
/// TypeScript's plain reporter deliberately omits source excerpts and related
/// information. It still owns the same stable sort/dedup boundary as the
/// contextual reporter, so switching `--pretty` never changes which
/// diagnostic occurrence is retained.
fn format_plain_diagnostics(
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
    source_texts: &DiagnosticSourceMap,
    current_directory: &str,
) -> Result<JsString, String> {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let mut output = JsString::new();
    for index in indices {
        let diagnostic = &diagnostics[index];
        if let Some(file_name) = diagnostic.file_name.as_ref().map(JsString::as_js) {
            let snapshot = source_texts
                .get(file_name.as_bytes())
                .or_else(|| {
                    let normalized = normalize_slashes(file_name);
                    source_texts
                        .iter()
                        .find(|(candidate, _)| normalize_slashes(*candidate) == normalized)
                        .map(|(_, snapshot)| snapshot)
                })
                .ok_or_else(|| {
                    format!("diagnostic source text is unavailable for {file_name:?}")
                })?;
            let text = snapshot.text();
            let position = diagnostic
                .start
                .ok_or_else(|| format!("diagnostic start is unavailable for {file_name:?}"))?;
            // A one-line source has a final line start of zero; clamp to the
            // UTF-16 text length rather than to that line-start sentinel so
            // located config diagnostics near the end of the line retain
            // their column.
            let text_length = text.encode_utf16().count() as u32;
            let position = position.min(text_length);
            let location = snapshot
                .positions()
                .line_and_character_utf16(position)
                .expect("clamped diagnostic position has a source line");
            output.push_js(
                relative_file_name(
                    file_name,
                    current_directory,
                    process_case_sensitive_file_names(),
                )
                .as_js(),
            );
            output.push_str(&format!(
                "({},{}): ",
                location.line + 1,
                location.character + 1
            ));
        }
        output.push_str(diagnostic.category().name());
        output.push_str(" TS");
        output.push_str(&diagnostic.code().to_string());
        output.push_str(": ");
        append_plain_message(&diagnostic.message, 0, &mut output);
        output.push('\n');
    }
    Ok(output)
}

fn append_plain_message(message: &MessageChain, indent: usize, output: &mut JsString) {
    if indent != 0 {
        output.push('\n');
        output.push_str(&"  ".repeat(indent));
    }
    output.push_js(message.text.as_js());
    for child in &message.next {
        append_plain_message(child, indent + 1, output);
    }
}

/// The process filesystem's case sensitivity for diagnostic path rendering
/// (tsc's host.getCanonicalFileName), probed once.
fn process_case_sensitive_file_names() -> bool {
    static CASE_SENSITIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *CASE_SENSITIVE.get_or_init(|| {
        FsCompilerHost::from_process().map_or(true, |host| host.use_case_sensitive_file_names())
    })
}

/// tsc-port: convertToRelativePath / getPathComponentsRelativeTo @6.0.3
/// (formatDiagnostic's file name): a rooted file name is rendered relative
/// to the current directory, climbing with `..` when the file lies outside
/// it (`../font/google/index.d.ts`); components after the root compare
/// through the host's canonical (case-folded) spelling, the root itself
/// case-insensitively, and a file sharing no root with the directory keeps
/// its absolute spelling.
fn relative_file_name<'p>(
    file_name: impl Into<JsStr<'p>>,
    current_directory: &str,
    case_sensitive: bool,
) -> JsString {
    let file_name = normalize_slashes(file_name);
    if !file_name.as_js().starts_with("/") {
        return file_name;
    }
    let directory = normalize_slashes(current_directory);
    let reduce = |path: JsStr<'_>| -> Vec<JsString> {
        // getPathComponents + reducePathComponents: a root component and
        // the segments, with `.` dropped and `..` folded.
        let mut components: Vec<JsString> = vec!["/".into()];
        for segment in path.split_ascii(b'/') {
            if segment.is_empty() || segment == "." {
                continue;
            }
            if segment == ".." {
                if components.len() > 1 {
                    components.pop();
                }
                continue;
            }
            components.push(segment.to_owned());
        }
        components
    };
    let from = reduce(directory.as_js());
    let to = reduce(file_name.as_js());
    let canonical = |component: &JsString| -> JsString {
        if case_sensitive {
            component.clone()
        } else {
            tsc_host::to_file_name_lower_case_js(component.as_js())
        }
    };
    let mut start = 0;
    while start < from.len() && start < to.len() {
        let equal = if start == 0 {
            tsc_host::to_file_name_lower_case_js(from[start].as_js())
                == tsc_host::to_file_name_lower_case_js(to[start].as_js())
        } else {
            canonical(&from[start]) == canonical(&to[start])
        };
        if !equal {
            break;
        }
        start += 1;
    }
    if start == 0 {
        return file_name;
    }
    let mut relative = JsString::new();
    for _ in start..from.len() {
        relative.push_str("../");
    }
    for (index, component) in to[start..].iter().enumerate() {
        if index > 0 {
            relative.push_str("/");
        }
        relative.push_js(component.as_js());
    }
    relative
}

fn normalize_slashes<'p>(path: impl Into<JsStr<'p>>) -> JsString {
    super::normalize_source_slashes(path.into())
}

fn parse_config_file(
    host: &dyn CompilerHost,
    current_directory: &Path,
    config_file: &Path,
    display_file_name: Option<&Path>,
) -> Result<(ConfigRootPlan, DiagnosticSourceMap), CliError> {
    let bytes = host
        .read_file(config_file)
        .map_err(host_error)?
        .ok_or_else(|| {
            CliError::Config(format!(
                "config file does not exist: {}",
                config_file.display()
            ))
        })?;
    let text = decode_host_text(bytes).map_err(|error| CliError::Config(error.to_string()))?;
    let display_file_name = display_file_name
        .unwrap_or(config_file)
        .to_str()
        .ok_or_else(|| CliError::Config("config display path is not Unicode".to_owned()))?;
    let base_path = current_directory
        .to_str()
        .ok_or_else(|| CliError::Config("current directory is not Unicode".to_owned()))?;
    let adapter = CompilerConfigHost::new(host);
    let plan = parse_config_root_plan_with_cache(
        &adapter,
        ConfigRootPlanRequest {
            file_name: display_file_name.into(),
            text,
            base_path: base_path.into(),
        },
        &mut ConfigExtendedCache::default(),
    )
    .map_err(config_error)?
    .with_resolved_config_source_path();
    let mut source_texts = BTreeMap::new();
    source_texts.insert(
        display_file_name.into(),
        Arc::clone(plan.source().snapshot()),
    );
    Ok((plan, source_texts))
}

enum ProjectFileError {
    MissingPath(String),
    MissingConfig(String),
}

fn resolve_project_file(
    host: &dyn CompilerHost,
    current_directory: &Path,
    project: &Path,
) -> Result<Result<PathBuf, ProjectFileError>, CliError> {
    let requested = project.to_string_lossy().replace('\\', "/");
    let project = absolutize(current_directory, project);
    if host.directory_exists(&project).map_err(host_error)? {
        let config_file = project.join(CONFIG_FILE_NAME);
        if host.file_exists(&config_file).map_err(host_error)? {
            return Ok(Ok(config_file));
        }
        return Ok(Err(ProjectFileError::MissingConfig(requested)));
    }
    if !host.file_exists(&project).map_err(host_error)? {
        return Ok(Err(ProjectFileError::MissingPath(requested)));
    }
    Ok(Ok(project))
}

fn find_config_file(
    host: &dyn CompilerHost,
    current_directory: &Path,
) -> Result<Option<PathBuf>, CliError> {
    let mut directory = current_directory.to_path_buf();
    loop {
        let candidate = directory.join(CONFIG_FILE_NAME);
        if host.file_exists(&candidate).map_err(host_error)? {
            return Ok(Some(candidate));
        }
        if !directory.pop() {
            return Ok(None);
        }
    }
}

fn absolutize(current_directory: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        current_directory.join(path)
    }
}

fn host_error(error: HostError) -> CliError {
    CliError::Host(error.to_string())
}

fn config_error(error: ConfigParseError) -> CliError {
    CliError::Config(error.to_string())
}

#[cfg(test)]
#[path = "../tests/unit/cli/tests.rs"]
mod tests;
