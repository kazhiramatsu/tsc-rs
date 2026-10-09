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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_diagnostics::{
    format_diagnostic_with_color_and_context, sort_and_dedupe_diagnostic_indices_with_context,
    write_error_summary_text, Diagnostic, FormatDiagnosticsHost, MessageChain, TextSnapshot,
};
use tsc_diagnostics::{gen, JsStr, JsString};
use tsc_host::{CompilerHost, HostError};
use tsc_incremental::{BuildInfo, OldState};
use tsc_program::{
    command_line_option_bag, command_line_program_inputs, decode_host_text, load_config_program,
    load_config_program_with_no_emit_override, load_emitting_config_program, load_emitting_program,
    load_program, parse_build_command_line, parse_command_line,
    parse_config_root_plan_with_command_line, CompilerConfigHost, CompilerOptions,
    ConfigExtendedCache, ConfigOptionBag, ConfigParseError, ConfigProgramLoadError, ConfigRootPlan,
    ConfigRootPlanRequest, LibraryCatalog, PreparedProgramMode, ProgramLoadLimits, ProgramOptions,
    WorkerBudget,
};

use crate::build::{self, BuildCommand};
use crate::help::Help;
use crate::incremental::WatchState;
use crate::locale::Locale;
use crate::show_config::ShowConfig;
use crate::statistics::Statistics;
use crate::system::{
    CommandLineTesting, NativeSystem, ProgramReport, System, SystemEmitFileSystem,
};
use crate::watch::{BuildWatcher, CommandWatcher, Watcher};
use crate::{CheckerBudget, EmitFileSystem, FsOutputSink, NoEmitWorkCounters, ProgramSession};
use tsc_types::tracing::{Args as TraceArgs, Phase as TracePhase, Tracing};

const EXIT_SUCCESS: i32 = 0;
const EXIT_COMMAND_LINE: i32 = 1;
const EXIT_DIAGNOSTIC: i32 = 2;
const EXIT_FAILURE: i32 = 2;
const CONFIG_FILE_NAME: &str = "tsconfig.json";
/// The vendored TypeScript profile whose standard libraries the executable
/// embeds and whose behavior it follows; `--version` reports it.
/// The version the command reports, tsgo's `core.Version()` at the vendored
/// profile.
pub const TYPESCRIPT_VERSION: &str = tsc_types::TYPESCRIPT_VERSION;
pub(crate) type DiagnosticSourceMap = BTreeMap<JsString, Arc<TextSnapshot>>;
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

fn cli_worker_budget(system: &dyn System) -> WorkerBudget {
    match system
        .env_var(WORKERS_ENV)
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

fn cli_checker_budget(system: &dyn System) -> CheckerBudget {
    match system
        .env_var(CHECKERS_ENV)
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

/// The command's own parallelism options (tsgo `singleThreaded` and
/// `checkers`, process options of the command line): one worker and one
/// checker, or that many checkers.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CommandBudgets {
    single_threaded: bool,
    checkers: Option<std::num::NonZeroUsize>,
}

impl CommandBudgets {
    fn of(single_threaded: Option<bool>, checkers: Option<f64>) -> Self {
        Self {
            single_threaded: single_threaded == Some(true),
            checkers: checkers
                .filter(|count| count.is_finite() && *count >= 1.0)
                .and_then(|count| std::num::NonZeroUsize::new(count as usize)),
        }
    }

    /// tsgo `Program.SingleThreaded`: no worker threads.
    fn worker_budget(self, system: &dyn System) -> WorkerBudget {
        if self.single_threaded {
            WorkerBudget::serial()
        } else {
            cli_worker_budget(system)
        }
    }

    /// tsgo `newCheckerPool`: one checker when single-threaded, else
    /// `--checkers`, else the process's budget.
    fn checker_budget(self, system: &dyn System) -> CheckerBudget {
        if self.single_threaded {
            CheckerBudget::serial()
                .with_leaked_states(true)
                .with_order_replay(tsc_checker::order_replay_requested())
        } else if let Some(checkers) = self.checkers {
            CheckerBudget::new(checkers)
                .with_leaked_states(true)
                .with_order_replay(tsc_checker::order_replay_requested())
        } else {
            cli_checker_budget(system)
        }
    }
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
    Host(String),
    Config(String),
    Load(String),
    Driver(String),
    Render(String),
}

pub(crate) struct CliRoute<'a> {
    pub(crate) system: &'a dyn System,
    pub(crate) testing: Option<&'a dyn CommandLineTesting>,
    pub(crate) pretty: bool,
    pub(crate) locale: Locale,
    pub(crate) output_filesystem: &'a mut dyn EmitFileSystem,
    /// The time the configuration took to parse (tsgo `ConfigTime`).
    pub(crate) config_time: std::time::Duration,
    /// `--singleThreaded` and `--checkers`.
    pub(crate) budgets: CommandBudgets,
    /// The project's `--generateTrace` session while it runs.
    pub(crate) tracing: Option<Arc<Tracing>>,
    /// A build watch's run: the time of each write is taken as the file is
    /// written (tsgo `Sys.Now()` in `writeFile`), before a harness stamps it.
    pub(crate) write_times: bool,
}

impl CliRoute<'_> {
    pub(crate) fn worker_budget(&self) -> WorkerBudget {
        self.budgets.worker_budget(self.system)
    }

    pub(crate) fn checker_budget(&self) -> CheckerBudget {
        self.budgets.checker_budget(self.system)
    }

    /// The program load limits with the run's worker budget.
    fn limits(&self) -> ProgramLoadLimits {
        DEFAULT_LIMITS.with_workers(self.worker_budget())
    }

    /// How this run writes diagnostics from `current_directory`.
    fn format<'p>(&self, current_directory: &'p Path) -> Format<'p> {
        Format {
            current_directory,
            case_sensitive: self.system.fs().case_sensitive(),
            pretty: self.pretty,
            locale: self.locale,
        }
    }
}

/// How diagnostics are written (tsgo `getFormatOptsOfSys` with
/// `shouldBePretty`): relative to the current directory under the file
/// system's case profile, plainly or with colors and context.
#[derive(Clone, Copy)]
pub(crate) struct Format<'a> {
    pub(crate) current_directory: &'a Path,
    pub(crate) case_sensitive: bool,
    pub(crate) pretty: bool,
    /// The language of the messages (tsgo `FormattingOptions.Locale`).
    pub(crate) locale: Locale,
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Host(detail) => write!(formatter, "filesystem host failure: {detail}"),
            Self::Config(detail) => write!(formatter, "config failure: {detail}"),
            Self::Load(detail) => write!(formatter, "program construction failure: {detail}"),
            Self::Driver(detail) => write!(formatter, "compiler failure: {detail}"),
            Self::Render(detail) => write!(formatter, "diagnostic rendering failure: {detail}"),
        }
    }
}

impl Error for CliError {}

/// Runs a command line in the process (its current directory, file system
/// and environment) and returns what it wrote.
pub fn run_cli(args: &[String]) -> CliOutput {
    let system = match NativeSystem::from_process() {
        Ok(system) => system,
        Err(error) => {
            return CliOutput {
                stdout: String::new(),
                stderr: format!("tsc-rs: {}\n", host_error(error)),
                exit_code: EXIT_FAILURE,
                work_counters: NoEmitWorkCounters::default(),
            }
        }
    };
    let mut watcher = None;
    let (exit_code, work_counters) = run(&system, args, None, &mut watcher);
    if let Some(watcher) = &mut watcher {
        // tsgo `RunLoop`: the watch writes each cycle's output as it ends
        // and runs until the process is stopped.
        let mut flush = || {
            use std::io::Write;
            let (stdout, stderr) = system.take_output();
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(stdout.as_bytes());
            let _ = out.flush();
            let mut err = std::io::stderr().lock();
            let _ = err.write_all(stderr.as_bytes());
            let _ = err.flush();
        };
        watcher.run(&mut flush);
    }
    let (stdout, stderr) = system.take_output();
    CliOutput {
        stdout,
        stderr,
        exit_code,
        work_counters,
    }
}

/// A command line's outcome (tsgo `CommandLineResult`): its exit status
/// and, for `--watch`, the watch it started (its first build has run).
pub struct CommandLineResult<'a> {
    pub status: i32,
    pub watcher: Option<CommandWatcher<'a>>,
}

/// Runs a command line over `system` (tsgo `execute.CommandLine`): the
/// output goes to the system, and the exit status is returned (tsgo
/// `ExitStatus`: 0 success, 1 diagnostics with the outputs skipped, 2
/// diagnostics with the outputs generated, 4 a project reference cycle).
/// `testing` observes the run as tsgo's test harness does.
pub fn execute_command_line<'a>(
    system: &'a dyn System,
    args: &[String],
    testing: Option<&'a dyn CommandLineTesting>,
) -> CommandLineResult<'a> {
    let mut watcher = None;
    let status = run(system, args, testing, &mut watcher).0;
    CommandLineResult { status, watcher }
}

fn run<'a>(
    system: &'a dyn System,
    args: &[String],
    testing: Option<&'a dyn CommandLineTesting>,
    watcher: &mut Option<CommandWatcher<'a>>,
) -> (i32, NoEmitWorkCounters) {
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
    let result = execute(system, testing, args, watcher);
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
        Ok(output) => {
            system.write_output(&output.stdout);
            system.write_error(&output.stderr);
            (output.exit_code, output.work_counters)
        }
        Err(error) => {
            system.write_error(&format!("tsc-rs: {error}\n"));
            (EXIT_FAILURE, NoEmitWorkCounters::default())
        }
    }
}

fn execute<'a>(
    system: &'a dyn System,
    testing: Option<&'a dyn CommandLineTesting>,
    args: &[String],
    watcher: &mut Option<CommandWatcher<'a>>,
) -> Result<CliOutput, CliError> {
    // tsgo CommandLine (execute/tsc.go): the build command when the first
    // argument is -b/--b/-build/--build.
    if let Some(first) = args.first() {
        if matches!(
            first.to_ascii_lowercase().as_str(),
            "-b" | "--b" | "-build" | "--build"
        ) {
            return execute_build(system, testing, &args[1..], watcher);
        }
    }
    let prologue_started = std::time::Instant::now();
    let host = system.compiler_host();
    let host = &*host;
    let current_directory = PathBuf::from(system.current_directory());
    let current_directory_js = JsString::from(system.current_directory());
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
    let pretty = parsed
        .option_bool("pretty")
        .unwrap_or_else(|| default_pretty(system));
    let locale = command_line_locale(parsed.option_string("locale"));
    let format = Format {
        current_directory: &current_directory,
        case_sensitive: host.use_case_sensitive_file_names(),
        pretty,
        locale,
    };
    if !parsed.errors.is_empty() {
        return rendered_diagnostics_with_exit(
            format,
            &BTreeMap::new(),
            &parsed.errors,
            EXIT_COMMAND_LINE,
        );
    }
    // tsgo createColors follows the environment and the terminal, not
    // --pretty.
    let help = Help::new(system, default_pretty(system), locale, TYPESCRIPT_VERSION);
    let all = parsed.option_bool("all") == Some(true);
    if parsed.option_bool("init") == Some(true) {
        return write_config_file(system, &help, format, &parsed.options);
    }
    if parsed.option_bool("version") == Some(true) {
        return Ok(CliOutput::new(help.version(), EXIT_SUCCESS));
    }
    if parsed.option_bool("help") == Some(true) || all {
        return Ok(CliOutput::new(help.help(all), EXIT_SUCCESS));
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
            format,
            &BTreeMap::new(),
            &[diagnostic],
            EXIT_COMMAND_LINE,
        );
    }
    let watch = parsed.option_bool("watch") == Some(true);
    let budgets = CommandBudgets::of(
        parsed.option_bool("singleThreaded"),
        parsed
            .option_value("checkers")
            .and_then(|value| value.as_f64()),
    );
    let mut output_filesystem =
        SystemEmitFileSystem::new(system.fs(), budgets.worker_budget(system).max_workers() > 1);
    let mut route = CliRoute {
        system,
        testing,
        pretty,
        locale,
        output_filesystem: &mut output_filesystem,
        config_time: std::time::Duration::ZERO,
        budgets,
        tracing: None,
        write_times: false,
    };
    let catalog = LibraryCatalog::typescript_7_1(Path::new(system.default_library_path()));
    // tsgo wraps the command line's options as `compilerOptions` and merges
    // them over the config's; explicit files take them as the program's.
    let command_line = command_line_option_bag(&parsed.options, current_directory_js.as_js());
    let files = parsed
        .file_names
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let ignore_config = parsed.option_bool("ignoreConfig") == Some(true);
    let show_config = parsed.option_bool("showConfig") == Some(true);
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
                format,
                &BTreeMap::new(),
                &[diagnostic],
                EXIT_COMMAND_LINE,
            );
        }
        // tsgo `tspath.NormalizePath(Project)`: `D:\\work` is `D:/work`.
        let project = PathBuf::from(
            tsc_program::normalize_path(project)
                .to_string_lossy()
                .into_owned(),
        );
        let config_file = match resolve_project_file(host, &current_directory, &project)? {
            Ok(config_file) => config_file,
            Err(ProjectFileError::MissingPath(path)) => {
                let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(&gen::The_specified_path_does_not_exist_0, &[path]),
                );
                return rendered_diagnostics_with_exit(
                    format,
                    &BTreeMap::new(),
                    &[diagnostic],
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
                    format,
                    &BTreeMap::new(),
                    &[diagnostic],
                    EXIT_COMMAND_LINE,
                );
            }
        };
        let config_started = std::time::Instant::now();
        let config_clock = system.now();
        let (plan, source_texts) =
            parse_config_file(host, &current_directory, &config_file, &command_line)?;
        route.config_time = elapsed_since(system, config_clock);
        tsc_types::trace::mark("cli: project config plan", config_started);
        if show_config {
            return Ok(show_config_of_plan(&plan, case_sensitive));
        }
        if watch {
            *watcher = Some(CommandWatcher::Program(Box::new(start_watch(
                system,
                testing,
                pretty,
                locale,
                budgets,
                &current_directory,
                catalog,
                WatchTarget::Config {
                    file: config_file,
                    command_line,
                    plan: Box::new(plan),
                    source_texts,
                },
            ))));
            return Ok(CliOutput::new(String::new(), EXIT_SUCCESS));
        }
        return execute_config(
            host,
            &current_directory,
            &catalog,
            &plan,
            source_texts,
            &mut route,
        );
    }

    if !files.is_empty() {
        if !ignore_config && find_config_file(host, &current_directory)?.is_some() {
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
                format,
                &source_texts,
                &[diagnostic],
                EXIT_COMMAND_LINE,
            );
        }
        let (options, program_options) = command_line_program_inputs(
            &command_line,
            current_directory_js.as_js(),
            case_sensitive,
        )
        .map_err(config_error)?;
        if show_config {
            // Without a config file tsgo writes paths from a tsconfig.json
            // in the current directory.
            let absolute =
                |path: &str| normalized_absolute_path(&current_directory, Path::new(path));
            let show = ShowConfig {
                options: &command_line,
                config_file: absolute("tsconfig.json"),
                file_names: parsed
                    .file_names
                    .iter()
                    .map(|file| absolute(file))
                    .collect(),
                plan: None,
                case_sensitive,
            };
            return Ok(CliOutput::new(show.to_json(), EXIT_SUCCESS));
        }
        if watch {
            *watcher = Some(CommandWatcher::Program(Box::new(start_watch(
                system,
                testing,
                pretty,
                locale,
                budgets,
                &current_directory,
                catalog,
                WatchTarget::Files {
                    roots: files,
                    options: Box::new(options),
                    program_options: Box::new(program_options),
                },
            ))));
            return Ok(CliOutput::new(String::new(), EXIT_SUCCESS));
        }
        // Keep the caller's spelling for root-file diagnostics. The program
        // loader normalizes these against the host cwd for identity and I/O,
        // while TypeScript reports a missing explicit root as it was written
        // on the command line (for example `missing.ts`, not its absolute
        // cwd-expanded path).
        return execute_explicit_files(
            host,
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
    // Without a config file tsgo prints its version and help (or, under
    // --showConfig, that it found none).
    let Some(config_file) = find_config_file(host, &current_directory)? else {
        if parsed.option_bool("showConfig") == Some(true) {
            let diagnostic = Diagnostic::new(
                None,
                None,
                None,
                MessageChain::new(
                    &gen::Cannot_find_a_tsconfig_json_file_at_the_current_directory_0,
                    &[normalized_absolute_path(&current_directory, Path::new("."))],
                ),
            );
            return rendered_diagnostics_with_exit(
                route.format(&current_directory),
                &BTreeMap::new(),
                &[diagnostic],
                EXIT_COMMAND_LINE,
            );
        }
        let mut stdout = help.version();
        stdout.push_str(&help.help(all));
        return Ok(CliOutput::new(stdout, EXIT_COMMAND_LINE));
    };
    let config_clock = system.now();
    let (plan, source_texts) =
        parse_config_file(host, &current_directory, &config_file, &command_line)?;
    route.config_time = elapsed_since(system, config_clock);
    if show_config {
        return Ok(show_config_of_plan(&plan, case_sensitive));
    }
    if watch {
        *watcher = Some(CommandWatcher::Program(Box::new(start_watch(
            system,
            testing,
            pretty,
            locale,
            budgets,
            &current_directory,
            catalog,
            WatchTarget::Config {
                file: config_file,
                command_line,
                plan: Box::new(plan),
                source_texts,
            },
        ))));
        return Ok(CliOutput::new(String::new(), EXIT_SUCCESS));
    }
    execute_config(
        host,
        &current_directory,
        &catalog,
        &plan,
        source_texts,
        &mut route,
    )
}

fn execute_build<'a>(
    system: &'a dyn System,
    testing: Option<&'a dyn CommandLineTesting>,
    args: &[String],
    watcher: &mut Option<CommandWatcher<'a>>,
) -> Result<CliOutput, CliError> {
    let host = system.compiler_host();
    let host = &*host;
    let current_directory = PathBuf::from(system.current_directory());
    let current_directory_js = JsString::from(system.current_directory());
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
    let pretty = parsed
        .option_bool("pretty")
        .unwrap_or_else(|| default_pretty(system));
    let locale = command_line_locale(parsed.option_string("locale"));
    let format = Format {
        current_directory: &current_directory,
        case_sensitive: host.use_case_sensitive_file_names(),
        pretty,
        locale,
    };
    if !parsed.errors.is_empty() {
        return rendered_diagnostics_with_exit(
            format,
            &BTreeMap::new(),
            &parsed.errors,
            EXIT_COMMAND_LINE,
        );
    }
    if parsed.option_bool("help") == Some(true) {
        let help = Help::new(system, default_pretty(system), locale, TYPESCRIPT_VERSION);
        let mut stdout = help.version();
        stdout.push_str(&help.build_help());
        return Ok(CliOutput::new(stdout, EXIT_SUCCESS));
    }
    let budgets = CommandBudgets::of(
        parsed.option_bool("singleThreaded"),
        parsed
            .option_value("checkers")
            .and_then(|value| value.as_f64()),
    );
    let mut output_filesystem =
        SystemEmitFileSystem::new(system.fs(), budgets.worker_budget(system).max_workers() > 1);
    let mut route = CliRoute {
        system,
        testing,
        pretty,
        locale,
        output_filesystem: &mut output_filesystem,
        config_time: std::time::Duration::ZERO,
        budgets,
        tracing: None,
        write_times: false,
    };
    let catalog = LibraryCatalog::typescript_7_1(Path::new(system.default_library_path()));
    let command = BuildCommand {
        projects: parsed.projects.clone(),
        verbose: parsed.build_bool("verbose"),
        dry: parsed.build_bool("dry"),
        force: parsed.build_bool("force"),
        clean: parsed.build_bool("clean"),
        stop_build_on_errors: parsed.build_bool("stopBuildOnErrors"),
        watch: parsed.option_bool("watch") == Some(true),
        command_line: command_line_option_bag(&parsed.options, current_directory_js.as_js()),
    };
    if command.watch {
        // tsgo `Orchestrator.start` with `--watch`: the first build and the
        // watches; the status is the first build's.
        let mut orchestrator =
            build::Orchestrator::new(system, testing, catalog, command, current_directory, pretty);
        orchestrator.set_locale(locale);
        let (build_watcher, status) =
            BuildWatcher::start(orchestrator, system, testing, pretty, locale, budgets);
        *watcher = Some(CommandWatcher::Build(Box::new(build_watcher)));
        return Ok(CliOutput::new(String::new(), status));
    }
    build::run_build(current_directory, catalog, command, &mut route)
}

/// The route of one build of a watch (`tsc -b --watch`): what a build's
/// projects write and report through.
pub(crate) fn with_build_route<R>(
    system: &dyn System,
    testing: Option<&dyn CommandLineTesting>,
    pretty: bool,
    locale: Locale,
    budgets: CommandBudgets,
    build: impl FnOnce(&mut CliRoute<'_>) -> R,
) -> R {
    let mut output_filesystem =
        SystemEmitFileSystem::new(system.fs(), budgets.worker_budget(system).max_workers() > 1);
    let mut route = CliRoute {
        system,
        testing,
        pretty,
        locale,
        output_filesystem: &mut output_filesystem,
        config_time: std::time::Duration::ZERO,
        budgets,
        tracing: None,
        write_times: true,
    };
    build(&mut route)
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
    /// The files a test harness stamped after the emit, with their time
    /// (tsgo `OnEmittedFiles`).
    pub(crate) stamped: Vec<(String, std::time::SystemTime)>,
    /// A build watch's write times: one per file written, in order, taken
    /// before a harness stamps them (empty otherwise).
    pub(crate) write_times: Vec<(String, std::time::SystemTime)>,
    /// The texts the diagnostics were rendered from (a build summarizes
    /// them at its end).
    pub(crate) sources: DiagnosticSourceMap,
    /// The incremental program's files for a test harness.
    pub(crate) program_report: Option<Vec<crate::ProgramFileReport>>,
    /// The run's statistics, when its options ask for them (a build
    /// aggregates them).
    pub(crate) statistics: Option<Statistics>,
    /// A watch run's state for its next cycle.
    pub(crate) watch_state: Option<WatchState>,
    /// The package.json files the program looked up (tsgo
    /// `PackageJsonLookupPaths`; a build watches them).
    pub(crate) package_json_lookups: Vec<String>,
}

/// tsgo `PackageJsonLookupPaths`: every package.json the program looked
/// up, existing or not, sorted (a program without a configuration file has
/// none).
fn package_json_lookup_paths(prepared: &tsc_program::PreparedProgram) -> Vec<String> {
    if prepared.program_options().config_file_path().is_none() {
        return Vec::new();
    }
    let mut paths = prepared
        .package_json_probes()
        .iter()
        .map(|probe| probe.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// How a project is run: for `tsc -b` the session knows it is a build and
/// the pretty error summary is left to the orchestrator.
#[derive(Clone, Copy)]
struct ProjectRunMode {
    build: bool,
    summary: bool,
    /// A watch run's cycle (`--watch`): the state is kept for the next
    /// cycle and nothing is leaked.
    watch: bool,
}

impl ProjectRunMode {
    const COMMAND: Self = Self {
        build: false,
        summary: true,
        watch: false,
    };
    const BUILD: Self = Self {
        build: true,
        summary: false,
        watch: false,
    };
    const WATCH: Self = Self {
        build: false,
        summary: true,
        watch: true,
    };
}

/// The old build info a project's run reuses: read by the command, or
/// handed over by the build (`None` under `--force`).
enum OldBuildInfoSource<'a> {
    Read,
    Given(Option<&'a BuildInfo>),
    /// A watch run's cycle: the previous cycle's state, or the build info
    /// on disk for the first (tsgo `ReadBuildInfoProgram` at the watch's
    /// start).
    Watch(Option<&'a WatchState>),
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
    let limits = route.limits();
    let config_file_name = plan.config_file_name().to_string_lossy().into_owned();
    // tsgo `startTracingIfNeeded` (a command's compilation; never a build's).
    let tracing = (!mode.build && !mode.watch)
        .then(|| start_tracing(route, plan.compiler_options(), &config_file_name))
        .flatten();
    let create_program = begin_create_program(tracing.as_ref(), &config_file_name);
    let load_started = std::time::Instant::now();
    let load_clock = route.system.now();
    let prepared = if plan.compiler_options().list_files_only == Some(true) {
        load_config_program_with_no_emit_override(host, plan, catalog, limits)
    } else if plan.compiler_options().no_emit == Some(true) {
        load_config_program(host, plan, catalog, limits)
    } else {
        load_emitting_config_program(host, plan, catalog, limits)
    };
    tsc_types::trace::mark("load program", load_started);
    if let Ok(prepared) = &prepared {
        trace_loader_parses(tracing.as_ref(), prepared);
    }
    drop(create_program);
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(ConfigProgramLoadError::Diagnostics { config, options }) => {
            let mut diagnostics = config;
            diagnostics.extend(options);
            let stdout = render_diagnostics(
                route.format(current_directory),
                &source_texts,
                &diagnostics,
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
                stamped: Vec::new(),
                write_times: Vec::new(),
                sources: source_texts,
                program_report: None,
                statistics: None,
                watch_state: None,
                package_json_lookups: Vec::new(),
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
        // noEmitOnError sees them. Only the no-emit route retains plan
        // ownership, of every option diagnostic (tsgo's program diagnostics
        // never stop the program, the `paths` rows included).
        .filter(|_| prepared.mode() == PreparedProgramMode::NoEmit)
        .cloned()
        .collect::<Vec<_>>();
    // tsgo ReadBuildInfoProgram: the command reads the old build info of an
    // incremental program before it compiles; a build hands it over.
    let default_library_directory = catalog.directory().to_string_lossy();
    let old_build_info = old_state(host, &prepared, &default_library_directory, old_source);
    let config_file = plan.config_file_name().to_string_lossy().into_owned();
    let resolution_trace = resolution_trace_text(&prepared, route, !mode.build);
    let statistics = wants_statistics(prepared.compiler_options()).then(|| Statistics {
        config_time: route.config_time,
        parse_time: elapsed_since(route.system, load_clock),
        ..Statistics::of_program(&prepared)
    });
    let check_clock = route.system.now();
    route.tracing = tracing;
    let run = execute_prepared(
        current_directory,
        source_texts,
        prepared,
        &option_diagnostics,
        route,
        old_build_info,
        mode,
    );
    let tracing = route.tracing.take();
    let mut run = run?;
    run.stdout.insert_str(0, &resolution_trace);
    report_program(route, Some(config_file), &run);
    report_statistics(route, &mut run, statistics, check_clock);
    stop_tracing(route, tracing, &mut run.stdout);
    Ok(run)
}

/// tsgo `startTracingIfNeeded`: a `--generateTrace` session for the
/// compilation, deterministic under a test harness.
fn start_tracing(
    route: &CliRoute<'_>,
    options: &CompilerOptions,
    config_file_path: &str,
) -> Option<Arc<Tracing>> {
    let directory = options.generate_trace.as_ref()?.to_string_lossy();
    if directory.is_empty() {
        return None;
    }
    Some(Tracing::start(
        &directory,
        config_file_path,
        route.testing.is_some(),
    ))
}

/// tsgo `Program` creation's span (`createProgram`).
fn begin_create_program(
    tracing: Option<&Arc<Tracing>>,
    config_file_path: &str,
) -> Option<tsc_types::tracing::Span> {
    tracing.map(|tracing| {
        tracing.begin(
            TracePhase::Program,
            "createProgram",
            TraceArgs::new().with("configFilePath", config_file_path),
        )
    })
}

/// The `createSourceFile` spans of the loader's parses (on worker threads
/// before the trace saw them), in the order they started.
fn trace_loader_parses(tracing: Option<&Arc<Tracing>>, prepared: &tsc_program::PreparedProgram) {
    let Some(tracing) = tracing else {
        return;
    };
    let mut parses = prepared
        .source_files()
        .iter()
        .filter_map(|source| {
            let (start, end) = source.preparsed_syntax().parse_span()?;
            Some((
                start,
                end,
                source.path().display().to_string_lossy().into_owned(),
            ))
        })
        .collect::<Vec<_>>();
    parses.sort_by_key(|(start, _, _)| *start);
    for (start, end, path) in parses {
        tracing.record_span(
            TracePhase::Parse,
            "createSourceFile",
            &TraceArgs::new().with("path", path),
            start,
            end,
        );
    }
}

/// tsgo `stopTracing`: the session's files, written through the run's file
/// system; a failure is a warning in the output.
fn stop_tracing(route: &CliRoute<'_>, tracing: Option<Arc<Tracing>>, stdout: &mut String) {
    let Some(tracing) = tracing else {
        return;
    };
    for file in tracing.finish() {
        if let Err(error) = route
            .system
            .fs()
            .write_creating_dirs(&file.path, file.text.as_bytes())
        {
            stdout.push_str(&format!(
                "Warning: Failed to stop tracing: failed to write {}: {error}\n",
                file.path
            ));
            return;
        }
    }
}

/// The state a project's run starts from (see [`OldBuildInfoSource`]).
fn old_state(
    host: &dyn CompilerHost,
    prepared: &tsc_program::PreparedProgram,
    default_library_directory: &str,
    source: OldBuildInfoSource<'_>,
) -> Option<OldState> {
    match source {
        OldBuildInfoSource::Read | OldBuildInfoSource::Watch(None) => {
            crate::incremental::read_old_build_info(host, prepared, default_library_directory)
        }
        OldBuildInfoSource::Given(info) => info.and_then(|info| {
            crate::incremental::old_state_of(info, prepared, default_library_directory)
        }),
        OldBuildInfoSource::Watch(Some(state)) => Some(crate::incremental::old_state_for_watch(
            state,
            prepared,
            default_library_directory,
        )),
    }
}

/// tsgo `EmitAndReportStatistics`: under `--diagnostics` or
/// `--extendedDiagnostics` a compilation that ran prints its statistics
/// after its output.
fn wants_statistics(options: &CompilerOptions) -> bool {
    options.diagnostics == Some(true) || options.extended_diagnostics == Some(true)
}

fn elapsed_since(system: &dyn System, start: std::time::SystemTime) -> std::time::Duration {
    system.now().duration_since(start).unwrap_or_default()
}

fn report_statistics(
    route: &CliRoute<'_>,
    run: &mut BuildProjectRun,
    statistics: Option<Statistics>,
    check_clock: std::time::SystemTime,
) {
    let Some(mut statistics) = statistics else {
        return;
    };
    statistics.check_time = elapsed_since(route.system, check_clock);
    statistics.total_time = route.system.since_start();
    run.stdout.push_str(&statistics.report(route.testing));
    run.statistics = Some(statistics);
}

/// The `--traceResolution` lines of a program's creation, as tsgo prints
/// them while it creates the Program; `shared_output` when they go to the
/// command's own output (a build writes each project's to its buffer).
fn resolution_trace_text(
    prepared: &tsc_program::PreparedProgram,
    route: &CliRoute<'_>,
    shared_output: bool,
) -> String {
    let mut text = String::new();
    for line in prepared.resolution_trace() {
        let line = line.to_string();
        match route.testing {
            Some(testing) => testing.trace(&line, &mut text, shared_output),
            None => {
                text.push_str(&line);
                text.push('\n');
            }
        }
    }
    text
}

/// tsgo `testing.OnProgram` after an incremental program's run.
fn report_program(route: &CliRoute<'_>, config_file: Option<String>, run: &BuildProjectRun) {
    if let (Some(testing), Some(files)) = (route.testing, &run.program_report) {
        testing.on_program(&ProgramReport {
            config_file,
            files: files.clone(),
        });
    }
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
    run_explicit_files(
        host,
        current_directory,
        catalog,
        roots,
        options,
        program_options,
        route,
        ProjectRunMode::COMMAND,
        OldBuildInfoSource::Read,
    )
    .map(|run| CliOutput::new(run.stdout, run.exit_code))
}

/// tsgo's compilation of the files named on the command line.
#[allow(clippy::too_many_arguments)]
fn run_explicit_files(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    roots: &[PathBuf],
    options: CompilerOptions,
    program_options: ProgramOptions,
    route: &mut CliRoute<'_>,
    mode: ProjectRunMode,
    old_source: OldBuildInfoSource<'_>,
) -> Result<BuildProjectRun, CliError> {
    let limits = route.limits();
    let load_clock = route.system.now();
    let tracing = (!mode.watch)
        .then(|| start_tracing(route, &options, ""))
        .flatten();
    let create_program = begin_create_program(tracing.as_ref(), "");
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
    trace_loader_parses(tracing.as_ref(), &prepared);
    drop(create_program);
    // tsgo prints the resolution trace while it creates the Program, before
    // any listing or diagnostic.
    let resolution_trace = resolution_trace_text(&prepared, route, true);
    let mut source_texts = BTreeMap::new();
    for source in prepared.source_files() {
        source_texts.insert(
            source.path().display().to_owned(),
            Arc::clone(source.snapshot()),
        );
    }
    let old_build_info = old_state(
        host,
        &prepared,
        &catalog.directory().to_string_lossy(),
        old_source,
    );
    let statistics = wants_statistics(prepared.compiler_options()).then(|| Statistics {
        parse_time: elapsed_since(route.system, load_clock),
        ..Statistics::of_program(&prepared)
    });
    let check_clock = route.system.now();
    route.tracing = tracing;
    let run = execute_prepared(
        current_directory,
        source_texts,
        prepared,
        &[],
        route,
        old_build_info,
        mode,
    );
    let tracing = route.tracing.take();
    let mut run = run?;
    report_program(route, None, &run);
    report_statistics(route, &mut run, statistics, check_clock);
    stop_tracing(route, tracing, &mut run.stdout);
    run.stdout.insert_str(0, &resolution_trace);
    Ok(run)
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
    let package_json_lookups = package_json_lookup_paths(&prepared);
    // tsc emitFilesAndReportErrors (_tsc.js:129433-129440): a --noEmit
    // command with getEmitDeclarations(options) reports the declaration
    // diagnostics after the semantic pass, only while nothing beyond the
    // config-file parsing diagnostics was reported. The command session runs
    // that getter over its own checker sessions
    // (`ProgramSession::run_no_emit_command`).
    // A watch run keeps its process: nothing is leaked from one cycle to the
    // next.
    let session = ProgramSession::new(prepared)
        .with_worker_budget(route.worker_budget())
        .with_checker_budget(route.checker_budget().with_leaked_states(!mode.watch))
        .with_leaked_program(!mode.watch)
        .with_command_options_diagnostics(!additional_diagnostics.is_empty())
        .with_build_mode(mode.build)
        .with_old_build_info(old_build_info)
        .with_testing(route.testing.is_some())
        .with_tracing(route.tracing.clone())
        .with_watch(mode.watch);
    // tsgo EmitFilesAndReportErrors runs no emit under --listFilesOnly, so no
    // build info is written either; an incremental program is still created
    // from the old build info (its testing data reports the files).
    let session = session
        .with_list_files_only(list_files_only)
        .with_command_build_info();
    let mut outcome = session
        .run_no_emit_pass(false, tsc_checker::LibraryPrefixCompletion::Complete, true)
        .map_err(|error| CliError::Driver(error.to_string()))?;
    let watch_state = outcome.take_watch_state();
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
    let program_report = outcome.program_report().map(<[_]>::to_vec);
    // tsgo's emit of a --noEmit command (none under --listFilesOnly): an
    // incremental program writes its build info, any other program's
    // `Program.Emit` returns at once.
    if !list_files_only && outcome.build_info().is_none() {
        if let Some(tracing) = &route.tracing {
            tracing
                .begin(TracePhase::Emit, "emit", TraceArgs::new())
                .end();
        }
    }
    if let Some(document) = outcome.build_info().filter(|_| !list_files_only) {
        let _span = crate::trace_build_info(route.tracing.as_ref());
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
    let write_times = if route.write_times {
        emitted_files
            .iter()
            .map(|file| (file.clone(), route.system.now()))
            .collect()
    } else {
        Vec::new()
    };
    let stamped = route
        .testing
        .map(|testing| testing.on_emitted_files(&emitted_files))
        .unwrap_or_default();
    status_writes.extend(listing);
    let status_writes = between_list_file_markers(route, status_writes);
    let work_counters = outcome.work_counters();
    let render_started = std::time::Instant::now();
    let rendered = rendered_diagnostics_with_exit_work_status_and_summary(
        route.format(current_directory),
        &source_texts,
        &diagnostics,
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
        stamped,
        write_times,
        sources: source_texts,
        program_report,
        statistics: None,
        watch_state,
        package_json_lookups,
    })
}

/// tsgo `listFiles` under a test harness: the listed files between
/// `OnListFilesStart` and `OnListFilesEnd`, which the harness drops when
/// it compares an incremental run with a clean one.
fn between_list_file_markers(route: &CliRoute<'_>, lines: Vec<JsString>) -> Vec<JsString> {
    let Some(testing) = route.testing else {
        return lines;
    };
    let mut start = String::new();
    testing.on_list_files_start(&mut start);
    let mut end = String::new();
    testing.on_list_files_end(&mut end);
    start
        .lines()
        .map(JsString::from)
        .chain(lines)
        .chain(end.lines().map(JsString::from))
        .collect()
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
    let write_workers = route.worker_budget().max_workers();
    let (worker_budget, checker_budget) = (
        route.worker_budget(),
        route.checker_budget().with_leaked_states(!mode.watch),
    );
    let testing = route.testing.is_some();
    let tracing = route.tracing.clone();
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
    let package_json_lookups = package_json_lookup_paths(&prepared);
    let session_started = std::time::Instant::now();
    let mut outcome = ProgramSession::new(prepared)
        .with_worker_budget(worker_budget)
        .with_checker_budget(checker_budget)
        .with_leaked_program(!mode.watch)
        .with_build_mode(mode.build)
        .with_old_build_info(old_build_info)
        .with_testing(testing)
        .with_tracing(tracing)
        .with_watch(mode.watch)
        .emit_for_cli(sink)
        .map_err(|error| CliError::Driver(error.to_string()))?;
    tsc_types::trace::mark("check + emit session", session_started);
    tsc_checker::line_profile::write_report();

    let build_emit = outcome.build_emit.clone();
    let program_report = outcome.program_report.clone();
    let watch_state = outcome.watch_state.take();
    let (emit, diagnostics, work_counters) = outcome.into_reported(additional_diagnostics);

    let cwd: JsStr<'_> = current_directory
        .to_str()
        .expect("prepared CLI cwd is Unicode")
        .into();
    let (mut status_writes, exit_code) =
        emit_command_status(cwd, &emit, &diagnostics, list_emitted_files);
    status_writes.extend(listing);
    let status_writes = between_list_file_markers(route, status_writes);
    let emitted_files = emit
        .emitted_files()
        .unwrap_or_default()
        .iter()
        .map(|path| {
            tsc_program::canonical_emit_path(path.as_js(), cwd, true)
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<String>>();
    let write_times = if route.write_times {
        emitted_files
            .iter()
            .map(|file| (file.clone(), route.system.now()))
            .collect()
    } else {
        Vec::new()
    };
    let stamped = route
        .testing
        .map(|testing| testing.on_emitted_files(&emitted_files))
        .unwrap_or_default();
    let output = rendered_diagnostics_with_exit_work_status_and_summary(
        route.format(current_directory),
        &source_texts,
        &diagnostics,
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
        stamped,
        write_times,
        sources: source_texts,
        program_report,
        statistics: None,
        watch_state,
        package_json_lookups,
    })
}

/// The command line's own errors (tsgo `tscCompilation` reports each one and
/// stops; no error summary follows).
fn rendered_diagnostics_with_exit(
    format: Format<'_>,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    exit_code: i32,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_work_status_and_summary(
        format,
        source_texts,
        diagnostics,
        exit_code,
        NoEmitWorkCounters::default(),
        &[],
        false,
    )
}

/// The diagnostics as the command prints them (sorted, deduplicated, plain
/// or pretty), without the status lines and, when `summary`, with the
/// pretty error summary.
pub(crate) fn render_diagnostics(
    format: Format<'_>,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    summary: bool,
) -> Result<String, CliError> {
    let output = rendered_diagnostics_with_exit_work_status_and_summary(
        format,
        source_texts,
        diagnostics,
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
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    locale: Locale,
) -> Result<String, CliError> {
    let current_directory = current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let host = FormatDiagnosticsHost::from_js_snapshots(current_directory.into(), source_texts)
        .with_catalog(locale.messages());
    let summarized: Vec<Diagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| !is_command_line_selection_diagnostic(diagnostic.code()))
        .cloned()
        .collect();
    write_error_summary_text(&summarized, &host, "\n")
        .map(|text| text.to_string_lossy().into_owned())
        .map_err(|error| CliError::Render(error.to_string()))
}

fn rendered_diagnostics_with_exit_work_status_and_summary(
    format: Format<'_>,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
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
    let current_directory = format
        .current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let host = FormatDiagnosticsHost::from_js_snapshots(current_directory.into(), source_texts)
        .with_catalog(format.locale.messages());
    let text = if format.pretty {
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
        let mut text = format_plain_diagnostics(
            diagnostics,
            &host,
            source_texts,
            current_directory,
            format.case_sensitive,
            format.locale.messages(),
        )
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

/// tsgo `WriteConfigFile`: a `tsconfig.json` with the recommended settings
/// and the command line's options in the current directory, or TS5054 when
/// one is there. Either way the run succeeds.
fn write_config_file(
    system: &dyn System,
    help: &Help<'_>,
    format: Format<'_>,
    options: &[(String, tsc_program::JsonValue)],
) -> Result<CliOutput, CliError> {
    let file = tsc_host::vfs::normalize(&format!("{}/tsconfig.json", system.current_directory()))
        .map_err(|error| CliError::Host(error.to_string()))?;
    if system.fs().is_file(&file) {
        let diagnostic = Diagnostic::new(
            None,
            None,
            None,
            MessageChain::new(&gen::A_tsconfig_json_file_is_already_defined_at_0, &[file]),
        );
        return rendered_diagnostics_with_exit(
            format,
            &BTreeMap::new(),
            &[diagnostic],
            EXIT_SUCCESS,
        );
    }
    let text = crate::init::generate_tsconfig(options, format.locale.messages());
    // tsgo ignores a failed write.
    let _ = system.fs().write_creating_dirs(&file, text.as_bytes());
    let mut stdout = "\n".to_owned();
    stdout.push_str(&help.header("Created a new tsconfig.json"));
    stdout.push_str("You can learn more at https://aka.ms/tsconfig\n");
    Ok(CliOutput::new(stdout, EXIT_SUCCESS))
}

/// tsgo `showConfig` for a config file: its options in effect, root files,
/// references, specs and `compileOnSave`.
fn show_config_of_plan(plan: &ConfigRootPlan, case_sensitive: bool) -> CliOutput {
    let show = ShowConfig {
        options: plan.options(),
        config_file: plan.config_file_name().to_string_lossy().into_owned(),
        file_names: plan
            .file_names()
            .iter()
            .map(|file| file.to_string_lossy().into_owned())
            .collect(),
        plan: Some(plan),
        case_sensitive,
    };
    CliOutput::new(show.to_json(), EXIT_SUCCESS)
}

/// The language of `--locale` (tsgo `ParsedCommandLine.Locale`): English
/// unless a well-formed tag names a translation.
fn command_line_locale(tag: Option<JsStr<'_>>) -> Locale {
    tag.and_then(|tag| Locale::parse(&tag.to_string_lossy()))
        .unwrap_or_default()
}

/// tsgo `defaultIsPretty`: FORCE_COLOR decides, then NO_COLOR and a dumb
/// terminal turn colors off; otherwise colors follow a terminal stdout.
fn default_pretty(system: &dyn System) -> bool {
    if let Some(force_color) = system.env_var("FORCE_COLOR") {
        return matches!(force_color.as_str(), "" | "1" | "2" | "3" | "true");
    }
    if system
        .env_var("NO_COLOR")
        .is_some_and(|value| !value.is_empty())
    {
        return false;
    }
    if system.env_var("TERM").is_some_and(|value| value == "dumb") {
        return false;
    }
    system.output_is_terminal()
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
    case_sensitive: bool,
    catalog: Option<&dyn tsc_diagnostics::MessageCatalog>,
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
            output
                .push_js(relative_file_name(file_name, current_directory, case_sensitive).as_js());
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
        append_plain_message(&diagnostic.message, 0, &mut output, catalog);
        output.push('\n');
    }
    Ok(output)
}

fn append_plain_message(
    message: &MessageChain,
    indent: usize,
    output: &mut JsString,
    catalog: Option<&dyn tsc_diagnostics::MessageCatalog>,
) {
    if indent != 0 {
        output.push('\n');
        output.push_str(&"  ".repeat(indent));
    }
    output.push_js(message.text_in(catalog).as_js());
    for child in &message.next {
        append_plain_message(child, indent + 1, output, catalog);
    }
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
    // tspath `ConvertToRelativePath`: a path without a root (`/`, `c:/`, …)
    // is left as it is.
    if tsc_program::path_root_parts(file_name.as_js()).is_none() {
        return file_name;
    }
    let directory = normalize_slashes(current_directory);
    let reduce = |path: JsStr<'_>| -> Vec<JsString> {
        // getPathComponents + reducePathComponents: a root component and
        // the segments, with `.` dropped and `..` folded.
        let (root, rest) = tsc_program::path_root_parts(path).unwrap_or(("".into(), path));
        let mut components: Vec<JsString> = vec![root.to_owned()];
        for segment in rest.split_ascii(b'/') {
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
/// tspath `GetNormalizedAbsolutePath`.
fn normalized_absolute_path(current_directory: &Path, path: &Path) -> String {
    tsc_program::get_normalized_absolute_path(
        JsStr::from_str(&path.to_string_lossy()),
        JsStr::from_str(&current_directory.to_string_lossy()),
    )
    .to_string_lossy()
    .into_owned()
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

/// A path joined to the current directory unless it is rooted, as tspath
/// roots it (`/`, `c:/`, UNC and URL roots, on every platform).
fn absolutize(current_directory: &Path, path: &Path) -> PathBuf {
    let rooted = path
        .to_str()
        .is_some_and(|text| tsc_program::path_root_parts(JsStr::from_str(text)).is_some());
    if path.is_absolute() || rooted {
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

// ----- watch mode -----------------------------------------------------------

/// What a watch run compiles: a configuration (its file, the command line's
/// options over it and its parse), or files named on the command line.
pub(crate) enum WatchTarget {
    Config {
        file: PathBuf,
        command_line: ConfigOptionBag,
        plan: Box<ConfigRootPlan>,
        source_texts: DiagnosticSourceMap,
    },
    Files {
        roots: Vec<PathBuf>,
        options: Box<CompilerOptions>,
        program_options: Box<ProgramOptions>,
    },
}

impl WatchTarget {
    pub(crate) fn is_config(&self) -> bool {
        matches!(self, Self::Config { .. })
    }

    pub(crate) fn options(&self) -> &CompilerOptions {
        match self {
            Self::Config { plan, .. } => plan.compiler_options(),
            Self::Files { options, .. } => options,
        }
    }

    /// The configuration file and the files it extends (tsgo
    /// `configFilePaths`).
    pub(crate) fn config_files(&self) -> Vec<String> {
        match self {
            Self::Config { plan, .. } => std::iter::once(plan.config_file_name())
                .chain(plan.extended_source_files().iter().map(JsString::as_js))
                .map(|file| file.to_string_lossy().into_owned())
                .collect(),
            Self::Files { .. } => Vec::new(),
        }
    }

    /// The root files.
    pub(crate) fn file_names(&self) -> Vec<String> {
        match self {
            Self::Config { plan, .. } => plan
                .file_names()
                .iter()
                .map(|file| file.to_string_lossy().into_owned())
                .collect(),
            Self::Files { roots, .. } => roots
                .iter()
                .map(|root| root.to_string_lossy().into_owned())
                .collect(),
        }
    }

    /// The include specs naming one file (no wildcard, an extension), made
    /// absolute against the configuration's directory.
    pub(crate) fn literal_includes(&self) -> Vec<String> {
        let Self::Config { plan, .. } = self else {
            return Vec::new();
        };
        let directory = PathBuf::from(crate::watch::config_directory(
            &plan.config_file_name().to_string_lossy(),
        ));
        plan.include_specs()
            .iter()
            .map(|spec| spec.to_string_lossy().into_owned())
            .filter(|spec| {
                !spec.contains(['*', '?'])
                    && spec
                        .rsplit('/')
                        .next()
                        .is_some_and(|name| name.contains('.'))
            })
            .map(|spec| normalized_absolute(&directory, &spec))
            .collect()
    }

    /// tsgo `WildcardDirectories`: the include directories, recursive when
    /// a pattern reaches below them.
    pub(crate) fn wildcard_directories(&self) -> BTreeMap<String, bool> {
        match self {
            Self::Config { plan, .. } => plan
                .wildcard_directories()
                .iter()
                .map(|directory| {
                    (
                        directory.path.to_string_lossy().into_owned(),
                        directory.recursive,
                    )
                })
                .collect(),
            Self::Files { .. } => BTreeMap::new(),
        }
    }

    pub(crate) fn has_wildcard_directories(&self) -> bool {
        matches!(self, Self::Config { plan, .. } if !plan.wildcard_directories().is_empty())
    }

    /// tsgo `ReloadFileNamesOfParsedCommandLine`: the include patterns
    /// matched again (the configuration parsed again); whether the root
    /// files changed.
    pub(crate) fn reload_file_names(&mut self, system: &dyn System) -> Result<bool, CliError> {
        let Self::Config {
            file,
            command_line,
            plan,
            ..
        } = self
        else {
            return Ok(false);
        };
        let host = system.compiler_host();
        let current_directory = PathBuf::from(system.current_directory());
        let (reloaded, _) = parse_config_file(&*host, &current_directory, file, command_line)?;
        let changed = reloaded.file_names() != plan.file_names();
        **plan = (**plan).clone().with_reloaded_file_names(reloaded);
        Ok(changed)
    }

    /// tsgo `parseConfigFile` of `recheckTsConfig`: the configuration parsed
    /// again; whether its parse changed (tsgo compares the
    /// `ParsedConfig`s). A configuration that cannot be read is TS5083.
    pub(crate) fn reparse(
        &mut self,
        system: &dyn System,
    ) -> Result<bool, crate::watch::ConfigError> {
        let Self::Config {
            file,
            command_line,
            plan,
            source_texts,
        } = self
        else {
            return Ok(false);
        };
        let host = system.compiler_host();
        let current_directory = PathBuf::from(system.current_directory());
        if !host.file_exists(file).unwrap_or(false) {
            let path = normalized_absolute_path(&current_directory, file);
            return Err(crate::watch::ConfigError::Diagnostics(vec![
                Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(&gen::Cannot_read_file_0, &[path]),
                ),
            ]));
        }
        let (new_plan, new_texts) =
            parse_config_file(&*host, &current_directory, file, command_line)
                .map_err(crate::watch::ConfigError::Fatal)?;
        let changed = new_plan.compiler_options() != plan.compiler_options()
            || new_plan.file_names() != plan.file_names()
            || new_plan.include_specs() != plan.include_specs()
            || new_plan.exclude_specs() != plan.exclude_specs();
        **plan = new_plan;
        *source_texts = new_texts;
        Ok(changed)
    }
}

/// What a watch run's build produced.
pub(crate) struct WatchBuild {
    /// The diagnostics, the error summary and the listings.
    pub(crate) stdout: String,
    /// The diagnostics reported (tsgo counts them for its status).
    pub(crate) error_count: usize,
    pub(crate) watch_state: Option<WatchState>,
    pub(crate) program_report: Option<ProgramReport>,
}

/// One build of a watch run: the command's compilation of `target` over the
/// previous build's state, through `host` (which records what it touched).
#[allow(clippy::too_many_arguments)]
pub(crate) fn watch_build(
    system: &dyn System,
    testing: Option<&dyn CommandLineTesting>,
    pretty: bool,
    locale: Locale,
    budgets: CommandBudgets,
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    target: &WatchTarget,
    state: Option<&WatchState>,
) -> Result<WatchBuild, CliError> {
    let mut output_filesystem =
        SystemEmitFileSystem::new(system.fs(), budgets.worker_budget(system).max_workers() > 1);
    let mut route = CliRoute {
        system,
        testing,
        pretty,
        locale,
        output_filesystem: &mut output_filesystem,
        config_time: std::time::Duration::ZERO,
        budgets,
        tracing: None,
        write_times: false,
    };
    let (run, config_file) = match target {
        WatchTarget::Config {
            plan, source_texts, ..
        } => (
            run_config(
                host,
                current_directory,
                catalog,
                plan,
                source_texts.clone(),
                &mut route,
                ProjectRunMode::WATCH,
                OldBuildInfoSource::Watch(state),
            )?,
            Some(plan.config_file_name().to_string_lossy().into_owned()),
        ),
        WatchTarget::Files {
            roots,
            options,
            program_options,
        } => (
            run_explicit_files(
                host,
                current_directory,
                catalog,
                roots,
                (**options).clone(),
                (**program_options).clone(),
                &mut route,
                ProjectRunMode::WATCH,
                OldBuildInfoSource::Watch(state),
            )?,
            None,
        ),
    };
    Ok(WatchBuild {
        stdout: run.stdout,
        error_count: run.diagnostics.len(),
        watch_state: run.watch_state,
        program_report: run
            .program_report
            .map(|files| ProgramReport { config_file, files }),
    })
}

/// A watch run's own diagnostics (a configuration it cannot read), without
/// an error summary.
pub(crate) fn render_watch_diagnostics(
    system: &dyn System,
    pretty: bool,
    locale: Locale,
    current_directory: &Path,
    diagnostics: &[Diagnostic],
) -> Result<String, CliError> {
    render_diagnostics(
        Format {
            current_directory,
            case_sensitive: system.fs().case_sensitive(),
            pretty,
            locale,
        },
        &BTreeMap::new(),
        diagnostics,
        false,
    )
}

/// `path` absolute against `current_directory` and normalized.
pub(crate) fn normalized_absolute(current_directory: &Path, path: &str) -> String {
    normalized_absolute_path(current_directory, Path::new(path))
}

/// Starts a watch run (tsgo `createWatcher` and `Watcher.start`): the first
/// build is written to the system's output.
#[allow(clippy::too_many_arguments)]
fn start_watch<'a>(
    system: &'a dyn System,
    testing: Option<&'a dyn CommandLineTesting>,
    pretty: bool,
    locale: Locale,
    budgets: CommandBudgets,
    current_directory: &Path,
    catalog: LibraryCatalog,
    target: WatchTarget,
) -> Watcher<'a> {
    let mut watcher = Watcher::new(
        system,
        testing,
        pretty,
        locale,
        budgets,
        current_directory.to_path_buf(),
        catalog,
        target,
    );
    watcher.start();
    watcher
}
