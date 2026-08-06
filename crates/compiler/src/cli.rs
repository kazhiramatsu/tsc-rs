//! Small, fail-closed H0 command-line driver.
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
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tsc_diagnostics::gen;
use tsc_diagnostics::{
    format_diagnostics_with_context, sort_and_dedupe_diagnostic_indices_with_context, Diagnostic,
    FormatDiagnosticsHost, MessageChain, TextSnapshot,
};
use tsc_host::{CompilerHost, FsCompilerHost, HostError};
use tsc_program::{
    decode_host_text, is_non_fatal_option_diagnostic, load_config_program,
    load_config_program_with_no_emit_override, load_program, parse_config_root_plan,
    CompilerConfigHost, CompilerOptions, ConfigParseError, ConfigProgramLoadError, ConfigRootPlan,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

use crate::{NoEmitWorkCounters, ProgramSession};

mod embedded_libraries {
    include!(concat!(env!("OUT_DIR"), "/typescript_6_0_3_libraries.rs"));
}

const EXIT_SUCCESS: i32 = 0;
const EXIT_COMMAND_LINE: i32 = 1;
const EXIT_DIAGNOSTIC: i32 = 2;
const EXIT_FAILURE: i32 = 2;
const CONFIG_FILE_NAME: &str = "tsconfig.json";
const TYPESCRIPT_VERSION: &str = "6.0.3";
type DiagnosticSourceMap = BTreeMap<String, Arc<TextSnapshot>>;
const DEFAULT_LIMITS: ProgramLoadLimits = ProgramLoadLimits::new(
    1_000_000,
    2_000_000,
    256,
    64 * 1024 * 1024,
    512 * 1024 * 1024,
);

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
enum CliError {
    Usage(String),
    Host(String),
    Config(String),
    Load(String),
    Driver(String),
    Render(String),
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
    no_emit: bool,
    ignore_config: bool,
    pretty: Option<bool>,
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
}

/// Execute the bounded H0 command-line surface.
pub fn run_cli(args: &[String]) -> CliOutput {
    match execute(args) {
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
    let command_line = parse_arguments(args)?;
    if args.iter().any(|arg| arg == "--version") {
        return Ok(CliOutput {
            stdout: format!("Version {TYPESCRIPT_VERSION}\n"),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters: NoEmitWorkCounters::default(),
        });
    }

    let filesystem = FsCompilerHost::from_process().map_err(host_error)?;
    let pretty = command_line.pretty.unwrap_or_else(default_pretty);
    let current_directory = filesystem.current_directory().map_err(host_error)?;
    let host = CliCompilerHost::new(filesystem, &current_directory);
    let catalog = LibraryCatalog::typescript_6_0_3(host.library_directory());

    if let Some(project) = command_line.project {
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
        let requested = absolutize(&current_directory, &project);
        let config_display = if requested == config_file {
            project
        } else {
            project.join(CONFIG_FILE_NAME)
        };
        let (plan, source_texts) = parse_config_file(
            &host,
            &current_directory,
            &config_file,
            Some(&config_display),
        )?;
        return execute_config(
            &host,
            &current_directory,
            &catalog,
            &plan,
            source_texts,
            command_line.no_emit,
            pretty,
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
        if !command_line.no_emit {
            return Err(CliError::Usage(
                "explicit source files require --noEmit; H0 never invokes an emitter".to_owned(),
            ));
        }
        // Keep the caller's spelling for root-file diagnostics. The program
        // loader normalizes these against the host cwd for identity and I/O,
        // while TypeScript reports a missing explicit root as it was written
        // on the command line (for example `missing.ts`, not its absolute
        // cwd-expanded path).
        let roots = command_line.files;
        return execute_explicit_files(&host, &current_directory, &catalog, &roots, pretty);
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
        command_line.no_emit,
        pretty,
    )
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
                if !value {
                    return Err(CliError::Usage(
                        "--noEmit=false is outside the mandatory no-emit driver".to_owned(),
                    ));
                }
                command_line.no_emit = true;
                index = next_index;
            }
            "--noEmit=false" => {
                return Err(CliError::Usage(
                    "--noEmit=false is outside the mandatory no-emit driver".to_owned(),
                ));
            }
            "--ignoreConfig" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.ignore_config = value;
                index = next_index;
            }
            "--pretty" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.pretty = Some(value);
                index = next_index;
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

fn execute_config(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    plan: &ConfigRootPlan,
    mut source_texts: DiagnosticSourceMap,
    no_emit_override: bool,
    pretty: bool,
) -> Result<CliOutput, CliError> {
    for source in plan.extended_sources() {
        source_texts.insert(source.file_name.clone(), Arc::clone(source.snapshot()));
    }
    source_texts.insert(
        plan.source().file_name.clone(),
        Arc::clone(plan.source().snapshot()),
    );
    let prepared = if no_emit_override {
        load_config_program_with_no_emit_override(host, plan, catalog, DEFAULT_LIMITS)
    } else {
        load_config_program(host, plan, catalog, DEFAULT_LIMITS)
    };
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(ConfigProgramLoadError::Diagnostics { config, options }) => {
            let mut diagnostics = config;
            diagnostics.extend(options);
            return rendered_diagnostics(current_directory, &source_texts, &diagnostics, pretty);
        }
        Err(ConfigProgramLoadError::NoEmitRequired { value }) => {
            return Err(CliError::Load(format!(
                "compilerOptions.noEmit must be true (observed {value:?}); pass --noEmit to override"
            )));
        }
        Err(ConfigProgramLoadError::Program(error)) => {
            return Err(CliError::Load(error.to_string()))
        }
    };
    for source in prepared.source_files() {
        source_texts.insert(
            source.path().display().display().to_string(),
            Arc::clone(source.snapshot()),
        );
    }
    for source in prepared.auxiliary_files() {
        source_texts.insert(
            source.path().display().display().to_string(),
            Arc::clone(source.snapshot()),
        );
    }
    let option_diagnostics = plan
        .option_diagnostics()
        .iter()
        .filter(|diagnostic| is_non_fatal_option_diagnostic(diagnostic))
        .cloned()
        .collect::<Vec<_>>();
    execute_prepared(
        current_directory,
        source_texts,
        prepared,
        &option_diagnostics,
        pretty,
    )
}

fn execute_explicit_files(
    host: &dyn CompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    roots: &[PathBuf],
    pretty: bool,
) -> Result<CliOutput, CliError> {
    let options = CompilerOptions {
        no_emit: Some(true),
        ..CompilerOptions::default()
    };
    let prepared = load_program(
        host,
        roots,
        options,
        ProgramOptions::default(),
        catalog,
        DEFAULT_LIMITS,
    )
    .map_err(|error| CliError::Load(error.to_string()))?;
    let mut source_texts = BTreeMap::new();
    for source in prepared.source_files() {
        source_texts.insert(
            source.path().display().display().to_string(),
            Arc::clone(source.snapshot()),
        );
    }
    execute_prepared(current_directory, source_texts, prepared, &[], pretty)
}

fn execute_prepared(
    current_directory: &Path,
    source_texts: DiagnosticSourceMap,
    prepared: tsc_program::PreparedProgram,
    additional_diagnostics: &[Diagnostic],
    pretty: bool,
) -> Result<CliOutput, CliError> {
    let outcome = ProgramSession::new(prepared)
        .run()
        .map_err(|error| CliError::Driver(error.to_string()))?;
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
        diagnostics.extend(outcome.semantic_diagnostics().iter().cloned());
    }
    let work_counters = outcome.work_counters();
    rendered_diagnostics_with_work(
        current_directory,
        &source_texts,
        &diagnostics,
        pretty,
        work_counters,
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
    )
}

fn rendered_diagnostics_with_work(
    current_directory: &Path,
    source_texts: &DiagnosticSourceMap,
    diagnostics: &[Diagnostic],
    pretty: bool,
    work_counters: NoEmitWorkCounters,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit_and_work(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        EXIT_DIAGNOSTIC,
        work_counters,
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
    if diagnostics.is_empty() {
        return Ok(CliOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
            work_counters,
        });
    }
    let current_directory = current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let host = FormatDiagnosticsHost::from_snapshots(current_directory, source_texts);
    let text = if pretty {
        let mut text = format_diagnostics_with_context(diagnostics, &host)
            .map_err(|error| CliError::Render(error.to_string()))?;
        append_pretty_error_summary(
            &mut text,
            diagnostics,
            &host,
            source_texts,
            current_directory,
        );
        colorize_pretty_output(&text)
    } else {
        format_plain_diagnostics(diagnostics, &host, source_texts, current_directory)
            .map_err(|error| CliError::Render(error.to_string()))?
    };
    Ok(CliOutput {
        stdout: text,
        stderr: String::new(),
        exit_code,
        work_counters,
    })
}

/// Append the command-line reporter's contextual error summary. Plain output
/// intentionally omits this block, matching TypeScript's non-pretty reporter.
/// The per-file counts are derived from the same sorted/deduplicated view used
/// by the formatter, so the summary cannot count an occurrence which was not
/// printed above.
fn append_pretty_error_summary(
    output: &mut String,
    diagnostics: &[Diagnostic],
    host: &FormatDiagnosticsHost<'_>,
    source_texts: &DiagnosticSourceMap,
    current_directory: &str,
) {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let mut file_counts = BTreeMap::<String, (usize, u32)>::new();
    let mut total = 0usize;
    for index in indices {
        let diagnostic = &diagnostics[index];
        if diagnostic.category().name() != "error"
            || is_command_line_selection_diagnostic(diagnostic.code())
        {
            continue;
        }
        let file_name = diagnostic.file_name.as_deref();
        let Some(file_name) = file_name else {
            total += 1;
            continue;
        };
        total += 1;
        let display_name = relative_file_name(file_name, current_directory);
        let line = diagnostic
            .start
            .and_then(|start| {
                source_texts
                    .get(file_name)
                    .or_else(|| {
                        let normalized = normalize_slashes(file_name);
                        source_texts
                            .iter()
                            .find(|(candidate, _)| normalize_slashes(candidate) == normalized)
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
            output.push_str(&format!("Found 1 error in {file}:{line}\n"));
        }
        (_, 0) => output.push_str(&format!("Found {total} {noun}.\n")),
        (_, 1) => {
            let (file, (_, line)) = file_counts.iter().next().expect("one file exists");
            output.push_str(&format!(
                "Found {total} {noun} in the same file, starting at: {file}:{line}\n"
            ));
        }
        (_, file_count) => {
            output.push_str(&format!("Found {total} {noun} in {file_count} files.\n\n"));
            output.push_str("Errors  Files\n");
            for (file, (count, line)) in file_counts {
                output.push_str(&format!("{count:>6}  {file}:{line}\n"));
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
) -> Result<String, String> {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let mut output = String::new();
    for index in indices {
        let diagnostic = &diagnostics[index];
        if let Some(file_name) = diagnostic.file_name.as_deref() {
            let snapshot = source_texts
                .get(file_name)
                .or_else(|| {
                    let normalized = normalize_slashes(file_name);
                    source_texts
                        .iter()
                        .find(|(candidate, _)| normalize_slashes(candidate) == normalized)
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
            output.push_str(&format!(
                "{}({},{}): ",
                relative_file_name(file_name, current_directory),
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

fn append_plain_message(message: &MessageChain, indent: usize, output: &mut String) {
    if indent != 0 {
        output.push('\n');
        output.push_str(&"  ".repeat(indent));
    }
    output.push_str(&message.text);
    for child in &message.next {
        append_plain_message(child, indent + 1, output);
    }
}

fn relative_file_name(file_name: &str, current_directory: &str) -> String {
    let file_name = normalize_slashes(file_name);
    let normalized_current_directory = normalize_slashes(current_directory);
    let current_directory = normalized_current_directory.trim_end_matches('/');
    if current_directory.is_empty() {
        return file_name;
    }
    if file_name == current_directory {
        return ".".to_owned();
    }
    if let Some(suffix) = file_name.strip_prefix(current_directory) {
        if let Some(suffix) = suffix.strip_prefix('/') {
            return suffix.to_owned();
        }
    }
    file_name
}

fn normalize_slashes(path: &str) -> String {
    path.replace('\\', "/")
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
    let plan = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: display_file_name.to_owned(),
            text,
            base_path: base_path.to_owned(),
        },
    )
    .map_err(config_error)?;
    let mut source_texts = BTreeMap::new();
    source_texts.insert(
        display_file_name.to_owned(),
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
