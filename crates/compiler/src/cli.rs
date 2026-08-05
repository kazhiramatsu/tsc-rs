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
use std::env;
use std::error::Error;
use std::fmt;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use tsc_diagnostics::gen;
use tsc_diagnostics::{
    compute_line_starts, format_diagnostics_with_context, get_line_and_character_of_position,
    sort_and_dedupe_diagnostic_indices_with_context, Diagnostic, FormatDiagnosticsHost,
    MessageChain,
};
use tsc_host::{CompilerHost, FsCompilerHost, HostError};
use tsc_program::{
    decode_host_text, is_non_fatal_option_diagnostic, load_config_program,
    load_config_program_with_no_emit_override, load_program, parse_config_root_plan,
    CompilerConfigHost, CompilerOptions, ConfigParseError, ConfigProgramLoadError, ConfigRootPlan,
    ConfigRootPlanRequest, LibraryCatalog, ProgramLoadLimits, ProgramOptions,
};

use crate::ProgramSession;

const EXIT_SUCCESS: i32 = 0;
const EXIT_COMMAND_LINE: i32 = 1;
const EXIT_DIAGNOSTIC: i32 = 2;
const EXIT_FAILURE: i32 = 2;
const CONFIG_FILE_NAME: &str = "tsconfig.json";
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

/// Execute the bounded H0 command-line surface.
pub fn run_cli(args: &[String]) -> CliOutput {
    match execute(args) {
        Ok(output) => output,
        Err(error) => CliOutput {
            stdout: String::new(),
            stderr: format!("tsc-rs: {error}\n"),
            exit_code: EXIT_FAILURE,
        },
    }
}

fn execute(args: &[String]) -> Result<CliOutput, CliError> {
    let command_line = parse_arguments(args)?;
    if args.iter().any(|arg| arg == "--version") {
        return Ok(CliOutput {
            stdout: format!("{}\n", env!("CARGO_PKG_VERSION")),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
        });
    }

    let host = FsCompilerHost::from_process().map_err(host_error)?;
    let pretty = command_line.pretty.unwrap_or_else(default_pretty);
    let current_directory = host.current_directory().map_err(host_error)?;
    let catalog = LibraryCatalog::typescript_6_0_3(library_directory(&current_directory));

    if let Some(project) = command_line.project {
        let config_file = resolve_project_file(&host, &current_directory, &project)?;
        let (plan, source_texts) = parse_config_file(&host, &config_file)?;
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
        let roots = command_line
            .files
            .into_iter()
            .map(|file| absolutize(&current_directory, &file))
            .collect::<Vec<_>>();
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
    let (plan, source_texts) = parse_config_file(&host, &config_file)?;
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
            "--noEmit=true" => {
                command_line.no_emit = true;
                index += 1;
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
            "--ignoreConfig=true" => {
                command_line.ignore_config = true;
                index += 1;
            }
            "--ignoreConfig=false" => {
                command_line.ignore_config = false;
                index += 1;
            }
            "--pretty" => {
                let (value, next_index) = consume_boolean_value(args, index, true);
                command_line.pretty = Some(value);
                index = next_index;
            }
            "--pretty=true" => {
                command_line.pretty = Some(true);
                index += 1;
            }
            "--pretty=false" => {
                command_line.pretty = Some(false);
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

fn execute_config(
    host: &FsCompilerHost,
    current_directory: &Path,
    catalog: &LibraryCatalog,
    plan: &ConfigRootPlan,
    mut source_texts: BTreeMap<String, String>,
    no_emit_override: bool,
    pretty: bool,
) -> Result<CliOutput, CliError> {
    for source in plan.extended_sources() {
        source_texts.insert(source.file_name.clone(), source.text.clone());
    }
    source_texts.insert(plan.source().file_name.clone(), plan.source().text.clone());
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
            source.text().to_owned(),
        );
    }
    for source in prepared.auxiliary_files() {
        source_texts.insert(
            source.path().display().display().to_string(),
            source.text().to_owned(),
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
    host: &FsCompilerHost,
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
            source.text().to_owned(),
        );
    }
    execute_prepared(current_directory, source_texts, prepared, &[], pretty)
}

fn execute_prepared(
    current_directory: &Path,
    source_texts: BTreeMap<String, String>,
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
    rendered_diagnostics(current_directory, &source_texts, &diagnostics, pretty)
}

fn rendered_diagnostics(
    current_directory: &Path,
    source_texts: &BTreeMap<String, String>,
    diagnostics: &[Diagnostic],
    pretty: bool,
) -> Result<CliOutput, CliError> {
    rendered_diagnostics_with_exit(
        current_directory,
        source_texts,
        diagnostics,
        pretty,
        EXIT_DIAGNOSTIC,
    )
}

fn rendered_diagnostics_with_exit(
    current_directory: &Path,
    source_texts: &BTreeMap<String, String>,
    diagnostics: &[Diagnostic],
    pretty: bool,
    exit_code: i32,
) -> Result<CliOutput, CliError> {
    if diagnostics.is_empty() {
        return Ok(CliOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: EXIT_SUCCESS,
        });
    }
    let current_directory = current_directory
        .to_str()
        .ok_or_else(|| CliError::Render("current directory is not Unicode".to_owned()))?;
    let host = FormatDiagnosticsHost::new(current_directory, source_texts);
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
    source_texts: &BTreeMap<String, String>,
    current_directory: &str,
) {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let mut file_counts = BTreeMap::<String, (usize, u32)>::new();
    let mut total = 0usize;
    for index in indices {
        let diagnostic = &diagnostics[index];
        if diagnostic.category().name() != "error" {
            continue;
        }
        total += 1;
        let Some(file_name) = diagnostic.file_name.as_deref() else {
            continue;
        };
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
                    .map(|text| {
                        get_line_and_character_of_position(&compute_line_starts(text), start).line
                            + 1
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
    match file_counts.len() {
        0 => output.push_str(&format!("Found {total} {noun}.\n")),
        1 => {
            let (file, (_, line)) = file_counts.iter().next().expect("one file exists");
            output.push_str(&format!("Found {total} {noun} in {file}:{line}\n"));
        }
        file_count => {
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
    let mut in_context = false;
    for line in input.split_inclusive('\n') {
        let (line, newline) = line
            .strip_suffix('\n')
            .map_or((line, ""), |line| (line, "\n"));
        if let Some(colored) = colorize_header(line) {
            output.push_str(&colored);
            output.push_str(newline);
            in_context = true;
        } else if line.starts_with("Found ") {
            output.push_str(&colorize_summary(line));
            output.push_str(newline);
            in_context = false;
        } else if in_context {
            output.push_str(&colorize_context_line(line));
            output.push_str(newline);
        } else {
            output.push_str(line);
            output.push_str(newline);
        }
    }
    output
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

fn colorize_context_line(line: &str) -> String {
    if line.is_empty() {
        return String::new();
    }
    if let Some(first_tilde) = line.find('~') {
        if line[first_tilde..].bytes().all(|byte| byte == b'~') {
            let (prefix, marks) = line.split_at(first_tilde);
            if let Some((first, rest)) = prefix.split_at_checked(1) {
                if let Some((plain, red_rest)) = rest.split_at_checked(1) {
                    return format!(
                        "{ANSI_REVERSE}{first}{ANSI_RESET}{plain}{ANSI_RED}{red_rest}{marks}{ANSI_RESET}"
                    );
                }
            }
        }
    }
    let digit_start = line.find(|character: char| character.is_ascii_digit());
    let Some(digit_start) = digit_start else {
        return line.to_owned();
    };
    let digit_end = line[digit_start..]
        .find(|character: char| !character.is_ascii_digit())
        .map_or(line.len(), |offset| digit_start + offset);
    if digit_end == digit_start || line[digit_end..].is_empty() {
        return line.to_owned();
    }
    format!(
        "{ANSI_REVERSE}{}{ANSI_RESET}{}",
        &line[..digit_end],
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
    source_texts: &BTreeMap<String, String>,
    current_directory: &str,
) -> Result<String, String> {
    let indices = sort_and_dedupe_diagnostic_indices_with_context(diagnostics, host);
    let mut output = String::new();
    for index in indices {
        let diagnostic = &diagnostics[index];
        if let Some(file_name) = diagnostic.file_name.as_deref() {
            let text = source_texts
                .get(file_name)
                .or_else(|| {
                    let normalized = normalize_slashes(file_name);
                    source_texts
                        .iter()
                        .find(|(candidate, _)| normalize_slashes(candidate) == normalized)
                        .map(|(_, text)| text)
                })
                .ok_or_else(|| {
                    format!("diagnostic source text is unavailable for {file_name:?}")
                })?;
            let line_starts = compute_line_starts(text);
            let position = diagnostic
                .start
                .ok_or_else(|| format!("diagnostic start is unavailable for {file_name:?}"))?;
            let position = position.min(*line_starts.last().unwrap_or(&0));
            let location = get_line_and_character_of_position(&line_starts, position);
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
    host: &FsCompilerHost,
    config_file: &Path,
) -> Result<(ConfigRootPlan, BTreeMap<String, String>), CliError> {
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
    let file_name = config_file
        .to_str()
        .ok_or_else(|| CliError::Config("config path is not Unicode".to_owned()))?;
    let base_path = config_file
        .parent()
        .and_then(Path::to_str)
        .ok_or_else(|| CliError::Config("config parent path is not Unicode".to_owned()))?;
    let adapter = CompilerConfigHost::new(host);
    let plan = parse_config_root_plan(
        &adapter,
        ConfigRootPlanRequest {
            file_name: file_name.to_owned(),
            text: text.clone(),
            base_path: base_path.to_owned(),
        },
    )
    .map_err(config_error)?;
    let mut source_texts = BTreeMap::new();
    source_texts.insert(file_name.to_owned(), text);
    Ok((plan, source_texts))
}

fn resolve_project_file(
    host: &FsCompilerHost,
    current_directory: &Path,
    project: &Path,
) -> Result<PathBuf, CliError> {
    let project = absolutize(current_directory, project);
    if host.directory_exists(&project).map_err(host_error)? {
        return Ok(project.join(CONFIG_FILE_NAME));
    }
    Ok(project)
}

fn find_config_file(
    host: &FsCompilerHost,
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

fn library_directory(current_directory: &Path) -> PathBuf {
    let local = current_directory.join("vendor/typescript-6.0.3/lib");
    if local.is_dir() {
        return local;
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/typescript-6.0.3/lib")
}

fn host_error(error: HostError) -> CliError {
    CliError::Host(error.to_string())
}

fn config_error(error: ConfigParseError) -> CliError {
    CliError::Config(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arguments: &[&str]) -> CliOutput {
        run_cli(
            &arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn argument_parser_rejects_emit_and_unknown_options() {
        assert_eq!(
            parse_arguments(&["--noEmit=false".to_owned()]),
            Err(CliError::Usage(
                "--noEmit=false is outside the mandatory no-emit driver".to_owned()
            ))
        );
        assert!(matches!(
            parse_arguments(&["--watch".to_owned()]),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn boolean_switches_consume_separate_values_without_turning_them_into_roots() {
        let parsed = parse_arguments(&[
            "--noEmit".to_owned(),
            "true".to_owned(),
            "--ignoreConfig".to_owned(),
            "true".to_owned(),
            "--pretty".to_owned(),
            "false".to_owned(),
            "main.ts".to_owned(),
        ])
        .expect("separate boolean values are accepted");
        assert!(parsed.no_emit);
        assert!(parsed.ignore_config);
        assert_eq!(parsed.pretty, Some(false));
        assert_eq!(parsed.files, [PathBuf::from("main.ts")]);
    }

    #[test]
    fn explicit_files_require_no_emit() {
        let output = run(&["missing.ts"]);
        assert_eq!(output.exit_code(), EXIT_FAILURE);
        assert!(output.stderr().contains("require --noEmit"));
    }

    #[test]
    fn version_is_available_without_a_filesystem_host() {
        let output = run(&["--version"]);
        assert_eq!(output.exit_code(), EXIT_SUCCESS);
        assert!(output.stderr().is_empty());
        assert_eq!(output.stdout().trim(), env!("CARGO_PKG_VERSION"));
    }
}
