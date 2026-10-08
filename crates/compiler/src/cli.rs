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
    format_diagnostic_with_color_and_context, sort_and_dedupe_diagnostic_indices_with_context,
    write_error_summary_text, Diagnostic, FormatDiagnosticsHost, MessageChain, TextSnapshot,
};
use tsc_diagnostics::{gen, JsStr, JsString};
use tsc_host::{CompilerHost, FsCompilerHost, HostError, ParallelSourceReader};
use tsc_incremental::{BuildInfo, OldState};
use tsc_program::{
    command_line_option_bag, command_line_program_inputs, decode_host_text,
    is_non_fatal_option_diagnostic, load_config_program, load_config_program_with_no_emit_override,
    load_emitting_config_program, load_emitting_program, load_program, parse_build_command_line,
    parse_command_line, parse_config_root_plan_with_command_line, CompilerConfigHost,
    CompilerOptions, ConfigExtendedCache, ConfigOptionBag, ConfigParseError,
    ConfigProgramLoadError, ConfigRootPlan, ConfigRootPlanRequest, LibraryCatalog,
    PreparedProgramMode, ProgramLoadLimits, ProgramOptions, WorkerBudget,
};

use crate::build::{self, BuildCommand};
use crate::{CheckerBudget, EmitFileSystem, FsOutputSink, NoEmitWorkCounters, ProgramSession};

mod embedded_libraries {
    include!(concat!(env!("OUT_DIR"), "/embedded_libraries.rs"));
}

const EXIT_SUCCESS: i32 = 0;
const EXIT_COMMAND_LINE: i32 = 1;
const EXIT_DIAGNOSTIC: i32 = 2;
const EXIT_FAILURE: i32 = 2;
const CONFIG_FILE_NAME: &str = "tsconfig.json";
/// The vendored TypeScript profile whose standard libraries the executable
/// embeds and whose behavior it follows; `--version` reports it.
const EMBEDDED_LIBRARY_PROFILE: &str = "7.1.0-dev-19dadef8";
pub(crate) const TYPESCRIPT_VERSION: &str = EMBEDDED_LIBRARY_PROFILE;
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
}

impl CliOutput {
    pub(crate) fn new(stdout: String, exit_code: i32) -> Self {
        Self {
            stdout,
            stderr: String::new(),
            exit_code,
            work_counters: NoEmitWorkCounters::default(),
        }
    }

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CliError {
    Usage(String),
    Host(String),
    Config(String),
    Load(String),
    Driver(String),
    Render(String),
}

pub(crate) struct CliRoute<'a> {
    pub(crate) pretty: bool,
    pub(crate) output_filesystem: &'a mut dyn EmitFileSystem,
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

/// The directory of the embedded standard library: tsgo's bundled
/// library path (`bundled:///libs`, internal/bundled/embed.go), a URL that
/// no filesystem path equals. Diagnostics and `--listFiles` name a library
/// file `bundled:///libs/lib.dom.d.ts`, as tsgo does, and such a name sorts
/// after every absolute path, which orders the diagnostics as tsgo's.
const EMBEDDED_LIBRARY_DIRECTORY: &str = "bundled:///libs";

/// Production CLI host with an immutable, binary-owned TypeScript 7.1
/// standard-library directory. User/config/package paths retain ordinary
/// filesystem semantics; only exact immediate children of this private
/// directory are intercepted.
#[derive(Clone, Debug)]
struct CliCompilerHost {
    filesystem: FsCompilerHost,
    library_directory: PathBuf,
}

impl CliCompilerHost {
    fn new(filesystem: FsCompilerHost) -> Self {
        Self {
            filesystem,
            library_directory: PathBuf::from(EMBEDDED_LIBRARY_DIRECTORY),
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
        embedded_libraries::EMBEDDED_LIBRARIES
            .binary_search_by_key(&name, |(candidate, _)| *candidate)
            .ok()
            .map(|index| embedded_libraries::EMBEDDED_LIBRARIES[index].1)
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
            return Ok(embedded_libraries::EMBEDDED_LIBRARIES
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
            return Ok(embedded_libraries::EMBEDDED_LIBRARIES
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
            return Ok(embedded_libraries::EMBEDDED_LIBRARIES
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
    let execute_started = std::time::Instant::now();
    let result = execute(args);
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
        let _ = std::fs::write(&path, report);
        let mut sites = String::new();
        for (site, count) in tsc_types::perf::name_sites() {
            sites.push_str(&format!("{count} {site}\n"));
        }
        let mut sites_path = path.clone();
        sites_path.push(".sites");
        let _ = std::fs::write(sites_path, sites);
    }
    match result {
        Ok(output) => output,
        Err(error) => CliOutput {
            stdout: String::new(),
            stderr: format!("tsc-rs: {error}\n"),
            exit_code: EXIT_FAILURE,
            work_counters: NoEmitWorkCounters::default(),
        },
    }
}

fn execute(args: &[String]) -> Result<CliOutput, CliError> {
    // tsgo CommandLine (execute/tsc.go): the build command when the first
    // argument is -b/--b/-build/--build.
    if let Some(first) = args.first() {
        if matches!(
            first.to_ascii_lowercase().as_str(),
            "-b" | "--b" | "-build" | "--build"
        ) {
            return execute_build(&args[1..]);
        }
    }
    let prologue_started = std::time::Instant::now();
    let filesystem = FsCompilerHost::from_process().map_err(host_error)?;
    let current_directory = filesystem.current_directory().map_err(host_error)?;
    let current_directory_js = JsString::from(
        current_directory
            .to_str()
            .ok_or_else(|| CliError::Host("current directory is not Unicode".to_owned()))?,
    );
    let host = CliCompilerHost::new(filesystem);
    let case_sensitive = host.use_case_sensitive_file_names();
    // tsgo ParseCommandLine: the options, the file names and the errors.
    let read_response_file = |path: JsStr<'_>| {
        host.read_file_js(path)
            .ok()
            .flatten()
            .and_then(|bytes| decode_host_text(bytes).ok())
    };
    let parsed = parse_command_line(
        args,
        current_directory_js.as_js(),
        case_sensitive,
        &read_response_file,
    );
    let pretty = parsed.option_bool("pretty").unwrap_or_else(default_pretty);
    if !parsed.errors.is_empty() {
        return rendered_diagnostics_with_exit(
            &current_directory,
            &BTreeMap::new(),
            &parsed.errors,
            pretty,
            EXIT_COMMAND_LINE,
        );
    }
    if parsed.option_bool("init") == Some(true) {
        return Err(CliError::Usage(
            "unsupported option \"--init\" (tsconfig.json generation)".to_owned(),
        ));
    }
    if parsed.option_bool("version") == Some(true) {
        return Ok(CliOutput {
            stdout: format!("Version {TYPESCRIPT_VERSION}\n"),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters: NoEmitWorkCounters::default(),
        });
    }
    if parsed.option_bool("help") == Some(true) || parsed.option_bool("all") == Some(true) {
        return Err(CliError::Usage(
            "unsupported option \"--help\" (the README describes the options)".to_owned(),
        ));
    }
    if parsed.option_bool("watch") == Some(true)
        && parsed.option_bool("listFilesOnly") == Some(true)
    {
        let diagnostic = Diagnostic::new(
            None,
            None,
            None,
            MessageChain::new(
                &gen::Options_0_and_1_cannot_be_combined,
                &["watch".to_owned(), "listFilesOnly".to_owned()],
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
    if parsed.option_bool("watch") == Some(true) {
        return Err(CliError::Usage(
            "unsupported option \"--watch\" (watch mode)".to_owned(),
        ));
    }
    if parsed.option_bool("showConfig") == Some(true) {
        return Err(CliError::Usage(
            "unsupported option \"--showConfig\"".to_owned(),
        ));
    }
    if let Some(locale) = parsed.option_string("locale") {
        if !locale
            .as_str()
            .is_some_and(|locale| locale.to_ascii_lowercase().starts_with("en"))
        {
            return Err(CliError::Usage(format!(
                "unsupported option \"--locale {}\" (only English messages exist)",
                locale.to_string_lossy()
            )));
        }
    }
    let mut output_filesystem = NativeEmitFileSystem;
    let mut route = CliRoute {
        pretty,
        output_filesystem: &mut output_filesystem,
    };
    let catalog = LibraryCatalog::typescript_7_1(host.library_directory());
    // tsgo wraps the command line's options as `compilerOptions` and merges
    // them over the config's; explicit files take them as the program's.
    let command_line = command_line_option_bag(&parsed.options, current_directory_js.as_js());
    let files = parsed
        .file_names
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let ignore_config = parsed.option_bool("ignoreConfig") == Some(true);
    tsc_types::trace::mark("cli: arguments, host, catalog", prologue_started);

    if let Some(project) = parsed.option_string("project") {
        if !files.is_empty() {
            let diagnostic = Diagnostic::new(
                None,
                None,
                None,
                MessageChain::new(
                    &gen::Option_project_cannot_be_mixed_with_source_files_on_a_command_line,
                    &[] as &[String],
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
        let project = PathBuf::from(project.to_string_lossy().into_owned());
        let config_file = match resolve_project_file(&host, &current_directory, &project)? {
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
            Err(ProjectFileError::MissingConfig(config_file)) => {
                let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(
                        &gen::Cannot_find_a_tsconfig_json_file_at_the_current_directory_0,
                        &[config_file],
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
        let config_started = std::time::Instant::now();
        let (plan, source_texts) =
            parse_config_file(&host, &current_directory, &config_file, &command_line)?;
        tsc_types::trace::mark("cli: project config plan", config_started);
        return execute_config(
            &host,
            &current_directory,
            &catalog,
            &plan,
            source_texts,
            &mut route,
        );
    }

    if !files.is_empty() {
        if !ignore_config && find_config_file(&host, &current_directory)?.is_some() {
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
        let (options, program_options) = command_line_program_inputs(
            &command_line,
            current_directory_js.as_js(),
            case_sensitive,
        )
        .map_err(config_error)?;
        // Keep the caller's spelling for root-file diagnostics. The program
        // loader normalizes these against the host cwd for identity and I/O,
        // while TypeScript reports a missing explicit root as it was written
        // on the command line (for example `missing.ts`, not its absolute
        // cwd-expanded path).
        return execute_explicit_files(
            &host,
            &current_directory,
            &catalog,
            &files,
            options,
            program_options,
            &mut route,
        );
    }

    // tsgo searches tsconfig.json upward from the current directory (even
    // under --ignoreConfig when no file is named) and prints its version and
    // help when there is none.
    let config_file = find_config_file(&host, &current_directory)?.ok_or_else(|| {
        CliError::Usage(format!(
            "cannot find {CONFIG_FILE_NAME} from {}",
            current_directory.display()
        ))
    })?;
    let (plan, source_texts) =
        parse_config_file(&host, &current_directory, &config_file, &command_line)?;
    execute_config(
        &host,
        &current_directory,
        &catalog,
        &plan,
        source_texts,
        &mut route,
    )
}

fn execute_build(args: &[String]) -> Result<CliOutput, CliError> {
    let filesystem = FsCompilerHost::from_process().map_err(host_error)?;
    let current_directory = filesystem.current_directory().map_err(host_error)?;
    let current_directory_js = JsString::from(
        current_directory
            .to_str()
            .ok_or_else(|| CliError::Host("current directory is not Unicode".to_owned()))?,
    );
    let host = CliCompilerHost::new(filesystem);
    let read_response_file = |path: JsStr<'_>| {
        host.read_file_js(path)
            .ok()
            .flatten()
            .and_then(|bytes| decode_host_text(bytes).ok())
    };
    // tsgo ParseBuildCommandLine: the build options, the common compiler
    // options (applied to every project), the projects and the errors.
    let parsed = parse_build_command_line(
        args,
        current_directory_js.as_js(),
        host.use_case_sensitive_file_names(),
        &read_response_file,
    );
    let pretty = parsed.option_bool("pretty").unwrap_or_else(default_pretty);
    if !parsed.errors.is_empty() {
        return rendered_diagnostics_with_exit(
            &current_directory,
            &BTreeMap::new(),
            &parsed.errors,
            pretty,
            EXIT_COMMAND_LINE,
        );
    }
    if parsed.option_bool("help") == Some(true) {
        return Err(CliError::Usage(
            "unsupported option \"--help\" (the README describes the options)".to_owned(),
        ));
    }
    if parsed.option_bool("watch") == Some(true) {
        return Err(CliError::Usage(
            "unsupported option \"--watch\" (watch mode)".to_owned(),
        ));
    }
    let mut output_filesystem = NativeEmitFileSystem;
    let mut route = CliRoute {
        pretty,
        output_filesystem: &mut output_filesystem,
    };
    let catalog = LibraryCatalog::typescript_7_1(host.library_directory());
    let command = BuildCommand {
        projects: parsed.projects.clone(),
        verbose: parsed.build_bool("verbose"),
        dry: parsed.build_bool("dry"),
        force: parsed.build_bool("force"),
        clean: parsed.build_bool("clean"),
        stop_build_on_errors: parsed.build_bool("stopBuildOnErrors"),
        command_line: command_line_option_bag(&parsed.options, current_directory_js.as_js()),
    };
    build::run_build(&host, &current_directory, &catalog, &command, &mut route)
}

/// What one project's run produced, for the command (`-p`) and for a
/// build (`tsc -b`, which buffers the output and reads the emit's facts).
pub(crate) struct BuildProjectRun {
    pub(crate) stdout: String,
    pub(crate) exit_code: i32,
    /// The diagnostics reported (tsgo `taskResult`'s errors).
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// The files written, as absolute normalized paths (the build info
    /// among them when it was written).
    pub(crate) emitted_files: Vec<String>,
    pub(crate) has_changed_dts_file: bool,
    pub(crate) declarations_differing_only_in_map: Vec<String>,
}

/// How a project is run: for `tsc -b` the session knows it is a build and
/// the pretty error summary is left to the orchestrator.
#[derive(Clone, Copy)]
struct ProjectRunMode {
    build: bool,
    summary: bool,
}

impl ProjectRunMode {
    const COMMAND: Self = Self {
        build: false,
        summary: true,
    };
    const BUILD: Self = Self {
        build: true,
        summary: false,
    };
}

/// The old build info a project's run reuses: read by the command, or
/// handed over by the build (`None` under `--force`).
enum OldBuildInfoSource<'a> {
    Read,
    Given(Option<&'a BuildInfo>),
}

/// tsgo `compileAndEmit`'s program run of one project of a build.
pub(crate) fn run_config_for_build(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    plan: &ConfigRootPlan,
    old: Option<&BuildInfo>,
    route: &mut CliRoute<'_>,
) -> Result<BuildProjectRun, CliError> {
    let mut source_texts = BTreeMap::new();
    source_texts.insert(
        plan.config_file_name().to_owned(),
        Arc::clone(plan.source().snapshot()),
    );
    run_config(
        host,
        current_directory,
        catalog,
        plan,
        source_texts,
        route,
        ProjectRunMode::BUILD,
        OldBuildInfoSource::Given(old),
    )
}

fn execute_config(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    plan: &ConfigRootPlan,
    source_texts: DiagnosticSourceMap,
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    run_config(
        host,
        current_directory,
        catalog,
        plan,
        source_texts,
        route,
        ProjectRunMode::COMMAND,
        OldBuildInfoSource::Read,
    )
    .map(|run| CliOutput::new(run.stdout, run.exit_code))
}

#[allow(clippy::too_many_arguments)]
fn run_config(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    plan: &ConfigRootPlan,
    mut source_texts: DiagnosticSourceMap,
    route: &mut CliRoute<'_>,
    mode: ProjectRunMode,
    old_source: OldBuildInfoSource<'_>,
) -> Result<BuildProjectRun, CliError> {
    for source in plan.extended_sources() {
        source_texts.insert(source.file_name.clone(), Arc::clone(source.snapshot()));
    }
    source_texts.insert(
        plan.source().file_name.clone(),
        Arc::clone(plan.source().snapshot()),
    );
    // The plan's options are the config's with the command line's merged
    // over them. tsgo runs no emit under --listFilesOnly: the no-emit route.
    let limits = cli_limits();
    let load_started = std::time::Instant::now();
    let prepared = if plan.compiler_options().list_files_only == Some(true) {
        load_config_program_with_no_emit_override(host, plan, catalog, limits)
    } else if plan.compiler_options().no_emit == Some(true) {
        load_config_program(host, plan, catalog, limits)
    } else {
        load_emitting_config_program(host, plan, catalog, limits)
    };
    tsc_types::trace::mark("load program", load_started);
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(ConfigProgramLoadError::Diagnostics { config, options }) => {
            let mut diagnostics = config;
            diagnostics.extend(options);
            let stdout = render_diagnostics(
                current_directory,
                &source_texts,
                &diagnostics,
                route.pretty,
                mode.summary,
            )?;
            return Ok(BuildProjectRun {
                stdout,
                exit_code: if diagnostics.is_empty() {
                    EXIT_SUCCESS
                } else {
                    EXIT_DIAGNOSTIC
                },
                diagnostics,
                emitted_files: Vec::new(),
                has_changed_dts_file: false,
                declarations_differing_only_in_map: Vec::new(),
            });
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
    // tsgo ReadBuildInfoProgram: the command reads the old build info of an
    // incremental program before it compiles; a build hands it over.
    let default_library_directory = catalog.directory().to_string_lossy();
    let old_build_info = match old_source {
        OldBuildInfoSource::Read => {
            crate::incremental::read_old_build_info(host, &prepared, &default_library_directory)
        }
        OldBuildInfoSource::Given(info) => info.and_then(|info| {
            crate::incremental::old_state_of(info, &prepared, &default_library_directory)
        }),
    };
    execute_prepared(
        current_directory,
        source_texts,
        prepared,
        &option_diagnostics,
        route,
        old_build_info,
        mode,
    )
}

fn execute_explicit_files(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    roots: &[PathBuf],
    options: CompilerOptions,
    program_options: ProgramOptions,
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    let limits = cli_limits();
    // tsgo runs no emit under --listFilesOnly: the no-emit route (whose
    // loader requires an explicit noEmit).
    let prepared = if options.no_emit == Some(true) || options.list_files_only == Some(true) {
        let mut options = options;
        options.no_emit = Some(true);
        load_program(host, roots, options, program_options, catalog, limits)
    } else {
        load_emitting_program(host, roots, options, program_options, catalog, limits)
    }
    .map_err(|error| CliError::Load(error.to_string()))?;
    // tsgo prints the resolution trace while it creates the Program, before
    // any listing or diagnostic.
    let resolution_trace: String = prepared
        .resolution_trace()
        .iter()
        .map(|line| format!("{line}\n"))
        .collect();
    let mut source_texts = BTreeMap::new();
    for source in prepared.source_files() {
        source_texts.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }
    let old_build_info = crate::incremental::read_old_build_info(
        host,
        &prepared,
        &catalog.directory().to_string_lossy(),
    );
    execute_prepared(
        current_directory,
        source_texts,
        prepared,
        &[],
        route,
        old_build_info,
        ProjectRunMode::COMMAND,
    )
    .map(|run| CliOutput::new(resolution_trace + &run.stdout, run.exit_code))
}

fn execute_prepared(
    current_directory: &Path,
    source_texts: DiagnosticSourceMap,
    prepared: tsc_program::PreparedProgram,
    additional_diagnostics: &[Diagnostic],
    route: &mut CliRoute<'_>,
    old_build_info: Option<OldState>,
    mode: ProjectRunMode,
) -> Result<BuildProjectRun, CliError> {
    if prepared.mode() == PreparedProgramMode::Emit {
        return execute_emitting_prepared(
            current_directory,
            source_texts,
            prepared,
            additional_diagnostics,
            route,
            old_build_info,
            mode,
        );
    }
    let session_started = std::time::Instant::now();
    let list_emitted_files = prepared.compiler_options().list_emitted_files == Some(true);
    let list_files_only = prepared.compiler_options().list_files_only == Some(true);
    let listing = listing_lines(&prepared, current_directory);
    // tsc emitFilesAndReportErrors (_tsc.js:129433-129440): a --noEmit
    // command with getEmitDeclarations(options) reports the declaration
    // diagnostics after the semantic pass, only while nothing beyond the
    // config-file parsing diagnostics was reported. The command session runs
    // that getter over its own checker sessions
    // (`ProgramSession::run_no_emit_command`).
    let session = ProgramSession::new(prepared)
        .with_worker_budget(cli_worker_budget())
        .with_checker_budget(cli_checker_budget())
        .with_leaked_program(true)
        .with_command_options_diagnostics(!additional_diagnostics.is_empty())
        .with_build_mode(mode.build)
        .with_old_build_info(old_build_info);
    // tsgo EmitFilesAndReportErrors runs no emit under --listFilesOnly, so no
    // build info is written either.
    let session = if list_files_only {
        session.with_list_files_only(true)
    } else {
        session.with_command_build_info()
    };
    let outcome = session
        .run_no_emit_pass(false, tsc_checker::LibraryPrefixCompletion::Complete, true)
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
    // tsgo emitBuildInfo: an incremental program's --noEmit command writes
    // its build info; the file joins the emitted-file listing, a failure to
    // write it (TS5033) joins the diagnostics.
    let mut status_writes = Vec::new();
    let mut emitted_files = Vec::new();
    if let Some(document) = outcome.build_info() {
        let mut sink = FsOutputSink::new(route.output_filesystem);
        match crate::incremental::write_build_info(&mut sink, document) {
            Some(failure) => {
                // The command's list is in tsgo's sorted order; a file-less
                // row sorts before the located rows, by code.
                let position = diagnostics
                    .iter()
                    .position(|diagnostic| {
                        diagnostic.file_name.is_some() || diagnostic.code() > failure.code()
                    })
                    .unwrap_or(diagnostics.len());
                diagnostics.insert(position, failure);
            }
            None => {
                let absolute = tsc_program::canonical_emit_path(
                    document.file_name.as_js(),
                    current_directory
                        .to_str()
                        .expect("prepared CLI cwd is Unicode")
                        .into(),
                    true,
                );
                if list_emitted_files {
                    let mut status = JsString::from("TSFILE: ");
                    status.push_js(absolute.as_js());
                    status_writes.push(status);
                }
                emitted_files.push(absolute.to_string_lossy().into_owned());
            }
        }
    }
    status_writes.extend(listing);
    let work_counters = outcome.work_counters();
    let render_started = std::time::Instant::now();
    let rendered = rendered_diagnostics_with_exit_work_status_and_summary(
        current_directory,
        &source_texts,
        &diagnostics,
        route.pretty,
        // tsgo EmitFilesAndReportErrors: --listFilesOnly skips the emit, so
        // its diagnostics report the outputs as skipped.
        if list_files_only {
            EXIT_COMMAND_LINE
        } else {
            EXIT_DIAGNOSTIC
        },
        work_counters,
        &status_writes,
        mode.summary,
    );
    tsc_types::trace::mark("cli: render diagnostics", render_started);
    let output = rendered?;
    Ok(BuildProjectRun {
        exit_code: output.exit_code,
        stdout: output.stdout,
        diagnostics,
        emitted_files,
        has_changed_dts_file: false,
        declarations_differing_only_in_map: Vec::new(),
    })
}

/// tsgo execute/tsc/emit.go `listFiles`: after the `TSFILE:` lines,
/// `--explainFiles` explains every file of the program (`Program.ExplainFiles`:
/// the name relative to the current directory, then three spaces and each
/// explanation), else `--listFiles`/`--listFilesOnly` lists every file's name.
fn listing_lines(
    prepared: &tsc_program::PreparedProgram,
    current_directory: &Path,
) -> Vec<JsString> {
    let options = prepared.compiler_options();
    if options.explain_files == Some(true) {
        let cwd: JsStr<'_> = current_directory
            .to_str()
            .expect("prepared CLI cwd is Unicode")
            .into();
        prepared
            .explain_files(cwd)
            .into_iter()
            .flat_map(|(name, lines)| {
                std::iter::once(name).chain(lines.into_iter().map(|line| {
                    let mut indented = JsString::from("   ");
                    indented.push_js(line.as_js());
                    indented
                }))
            })
            .collect()
    } else if options.list_files == Some(true) || options.list_files_only == Some(true) {
        prepared
            .source_files()
            .iter()
            .map(|source| source.path().display().to_owned())
            .collect()
    } else {
        Vec::new()
    }
}

/// Shared command producer for real CLI execution and scoped Program emits.
/// `list_emitted_files`: the `TSFILE:` lines are printed (a build collects
/// the emitted files whether or not the option asks for the listing).
pub(crate) fn emit_command_status(
    current_directory: JsStr<'_>,
    emit: &crate::EmitOutcome,
    diagnostics: &[Diagnostic],
    list_emitted_files: bool,
) -> (Vec<JsString>, i32) {
    let status_writes = emit
        .emitted_files()
        .filter(|_| list_emitted_files)
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
    old_build_info: Option<OldState>,
    mode: ProjectRunMode,
) -> Result<BuildProjectRun, CliError> {
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
    let list_emitted_files = prepared.compiler_options().list_emitted_files == Some(true);
    let listing = listing_lines(&prepared, current_directory);
    let session_started = std::time::Instant::now();
    let outcome = ProgramSession::new(prepared)
        .with_worker_budget(cli_worker_budget())
        .with_checker_budget(cli_checker_budget())
        .with_leaked_program(true)
        .with_build_mode(mode.build)
        .with_old_build_info(old_build_info)
        .emit_for_cli(sink)
        .map_err(|error| CliError::Driver(error.to_string()))?;
    tsc_types::trace::mark("check + emit session", session_started);
    tsc_checker::line_profile::write_report();

    let build_emit = outcome.build_emit.clone();
    let (emit, diagnostics, work_counters) = outcome.into_reported(additional_diagnostics);

    let cwd: JsStr<'_> = current_directory
        .to_str()
        .expect("prepared CLI cwd is Unicode")
        .into();
    let (mut status_writes, exit_code) =
        emit_command_status(cwd, &emit, &diagnostics, list_emitted_files);
    status_writes.extend(listing);
    let emitted_files = emit
        .emitted_files()
        .unwrap_or_default()
        .iter()
        .map(|path| {
            tsc_program::canonical_emit_path(path.as_js(), cwd, true)
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    let output = rendered_diagnostics_with_exit_work_status_and_summary(
        current_directory,
        &source_texts,
        &diagnostics,
        route.pretty,
        exit_code,
        work_counters,
        &status_writes,
        mode.summary,
    )?;
    Ok(BuildProjectRun {
        exit_code: output.exit_code,
        stdout: output.stdout,
        diagnostics: diagnostics.to_vec(),
        emitted_files,
        has_changed_dts_file: build_emit.has_changed_dts_file,
        declarations_differing_only_in_map: build_emit
            .declarations_differing_only_in_map
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    })
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
    )
}

fn rendered_diagnostics_with_exit_and_work(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
    work_counters: NoEmitWorkCounters,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_work_and_status(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        exit_code,
        work_counters,
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
    status_writes: &[JsString],
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_work_status_and_summary(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        exit_code,
        work_counters,
        status_writes,
        true,
    )
}

/// The diagnostics as the command prints them (sorted, deduplicated, plain
/// or pretty), without the status lines and, when `summary`, with the
/// pretty error summary.
pub(crate) fn render_diagnostics(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    summary: bool,
) -> Result<String, CliError> {
    let output = rendered_diagnostics_with_exit_work_status_and_summary(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        EXIT_DIAGNOSTIC,
        NoEmitWorkCounters::default(),
        &[],
        summary,
    )?;
    Ok(output.stdout)
}

/// tsgo `CreateReportErrorSummary`'s text (the pretty reporter's
/// `Found N errors…`) over the given diagnostics.
pub(crate) fn render_error_summary_text(
    current_directory: &Path,
    diagnostics: &[Diagnostic],
) -> Result<String, CliError> {
    let current_directory = current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let source_texts = BTreeMap::new();
    let host = FormatDiagnosticsHost::from_js_snapshots(current_directory.into(), &source_texts);
    let summarized: Vec<Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| !is_command_line_selection_diagnostic(diagnostic.code()))
        .cloned()
        .collect();
    write_error_summary_text(&summarized, &host, "\n")
        .map(|text| text.to_string_lossy().into_owned())
        .map_err(|error| CliError::Render(error.to_string()))
}

#[allow(clippy::too_many_arguments)]
fn rendered_diagnostics_with_exit_work_status_and_summary(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
    work_counters: NoEmitWorkCounters,
    status_writes: &[JsString],
    summary: bool,
) -> Result<CliOutput, CliError> {
    if diagnostics.is_empty() && status_writes.is_empty() {
        return Ok(CliOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters,
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
        });
    }
    let current_directory = current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let host = FormatDiagnosticsHost::from_js_snapshots(current_directory.into(), source_texts);
    let text = if pretty {
        // tsgo's pretty reporter: each diagnostic with its context, then the
        // error summary (CreateDiagnosticReporter, CreateReportErrorSummary).
        let selected: Vec<Diagnostic> =
            sort_and_dedupe_diagnostic_indices_with_context(diagnostics, &host)
                .into_iter()
                .map(|index| diagnostics[index].clone())
                .collect();
        let render =
            |error: tsc_diagnostics::FormatDiagnosticsError| CliError::Render(error.to_string());
        let mut text = JsString::new();
        for diagnostic in &selected {
            format_diagnostic_with_color_and_context(&mut text, diagnostic, &host, "\n")
                .map_err(render)?;
            text.push('\n');
        }
        append_status_writes(&mut text, status_writes);
        // The configuration-file selection errors end the run before any
        // summary is written; a build summarizes every project at its end.
        if summary {
            let summarized: Vec<Diagnostic> = selected
                .into_iter()
                .filter(|diagnostic| !is_command_line_selection_diagnostic(diagnostic.code()))
                .collect();
            text.push_js(
                write_error_summary_text(&summarized, &host, "\n")
                    .map_err(render)?
                    .as_js(),
            );
        }
        text.to_string_lossy().into_owned()
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
    })
}

fn append_status_writes(output: &mut JsString, status_writes: &[JsString]) {
    for status in status_writes {
        output.push_js(status.as_js());
        output.push('\n');
    }
}

fn is_command_line_selection_diagnostic(code: u32) -> bool {
    matches!(code, 5058 | 5081 | 5112)
}

/// tsgo `defaultIsPretty`: FORCE_COLOR decides, then NO_COLOR and a dumb
/// terminal turn colors off; otherwise colors follow a terminal stdout.
fn default_pretty() -> bool {
    if let Some(force_color) = std::env::var_os("FORCE_COLOR") {
        return matches!(force_color.to_str(), Some("" | "1" | "2" | "3" | "true"));
    }
    if std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        return false;
    }
    if std::env::var_os("TERM").is_some_and(|value| value == "dumb") {
        return false;
    }
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
pub(crate) fn relative_file_name<'p>(
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

/// tsgo names the config by its normalized absolute path
/// (GetParsedCommandLineOfConfigFile, tsoptions/tsconfigparsing.go:2071), and
/// diagnostics print it relative to the current directory.
/// The config's plan with the command line's options merged over its own
/// (tsgo GetParsedCommandLineOfConfigFile).
fn parse_config_file(
    host: &dyn CompilerHost,
    current_directory: &Path,
    config_file: &Path,
    command_line: &ConfigOptionBag,
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
    let display_file_name = tsc_program::normalize_path(JsStr::from_str(
        absolutize(current_directory, config_file)
            .to_str()
            .ok_or_else(|| CliError::Config("config path is not Unicode".to_owned()))?,
    ));
    let base_path = current_directory
        .to_str()
        .ok_or_else(|| CliError::Config("current directory is not Unicode".to_owned()))?;
    let adapter = CompilerConfigHost::new(host);
    let plan = parse_config_root_plan_with_command_line(
        &adapter,
        ConfigRootPlanRequest {
            file_name: display_file_name.clone(),
            text,
            base_path: base_path.into(),
        },
        command_line,
        &mut ConfigExtendedCache::default(),
    )
    .map_err(config_error)?;
    let mut source_texts = BTreeMap::new();
    source_texts.insert(display_file_name, Arc::clone(plan.source().snapshot()));
    Ok((plan, source_texts))
}

/// A `--project` that names nothing to load, by the normalized absolute
/// path tsgo reports (tsc.go:165-180).
enum ProjectFileError {
    /// No file or directory at the path (TS5058).
    MissingPath(String),
    /// A directory without `tsconfig.json`: the file's path (TS5081).
    MissingConfig(String),
}

fn resolve_project_file(
    host: &dyn CompilerHost,
    current_directory: &Path,
    project: &Path,
) -> Result<Result<PathBuf, ProjectFileError>, CliError> {
    let requested = normalized_absolute_path(current_directory, project);
    let project = absolutize(current_directory, project);
    if host.directory_exists(&project).map_err(host_error)? {
        let config_file = project.join(CONFIG_FILE_NAME);
        if host.file_exists(&config_file).map_err(host_error)? {
            return Ok(Ok(config_file));
        }
        let separator = if requested.ends_with('/') { "" } else { "/" };
        return Ok(Err(ProjectFileError::MissingConfig(format!(
            "{requested}{separator}{CONFIG_FILE_NAME}"
        ))));
    }
    if !host.file_exists(&project).map_err(host_error)? {
        return Ok(Err(ProjectFileError::MissingPath(requested)));
    }
    Ok(Ok(project))
}

/// tspath.NormalizePath of the absolute path: forward slashes, with `.` and
/// `..` resolved.
fn normalized_absolute_path(current_directory: &Path, path: &Path) -> String {
    let text = absolutize(current_directory, path)
        .to_string_lossy()
        .replace('\\', "/");
    let rooted = text.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for component in text.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            component => parts.push(component),
        }
    }
    let joined = parts.join("/");
    if rooted {
        format!("/{joined}")
    } else {
        joined
    }
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
