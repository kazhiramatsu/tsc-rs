//! Conformance against a native (TypeScript 7.x) profile's reference
//! baselines; see `docs/design/greenfield/slices/conformance-ts71/README.md`.
//!
//! Every case is expanded with the native runner's own rules
//! (`tsc_harness::upstream_suites::native`). A configuration the native runner
//! executes is lane A: tsc-rs checks it through the Program path and the
//! diagnostics the native runner collects are compared with the profile's
//! `.errors.txt` (an absent file means none), then its emit with the `.js`
//! and `.js.map` baselines. A configuration the runner's
//! `SkipUnsupportedCompilerOptions` leaves out (an option TypeScript 7
//! removed: ES5, UMD and System, node10 and classic, `baseUrl`,
//! `esModuleInterop: false`, `alwaysStrict: false`) is skipped, and one it
//! never produces baselines for (`skippedTests`, an unknown directive, the
//! fatal AMD and `outFile`) is not run.

mod emit_baseline;
mod errors_baseline;

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use emit_baseline::{Emission, EmittedFile, MapOptions};
use errors_baseline::remove_test_path_prefixes;
use serde::Serialize;
use sha2::Digest as _;
use tsc_compiler::{
    CheckerBudget, DriverError, MemoryOutputSink, NativeHarnessCollection, PreparedProgramMode,
    ProgramSession,
};
use tsc_diagnostics::{Diagnostic, PositionIndex};
use tsc_harness::upstream_suites::execution::{
    load_native_compiler_program, load_native_declaration_program, native_compiler_fixture,
    native_compiler_plan, normalize_compiler_fixture_path, read_test_library,
    CompilerExecutionPlan, CompilerRootSelection,
};
use tsc_harness::upstream_suites::native::{
    expand_case, NativeCase, NativeConfiguration, NativeProfile, NativeSkip, NativeSuite,
};
use tsc_harness::upstream_suites::OrderedSetting;
use tsc_program::ProgramLoadLimits;

/// One diagnostic as the native error baseline's summary line records it.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct BaselineDiagnostic {
    /// `None` for a global diagnostic; `lib.x.d.ts` for a masked library file.
    pub file: Option<String>,
    /// 1-based line and UTF-16 column; `None` when masked or global.
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub code: u32,
    pub category: String,
    /// The first line of the flattened message.
    pub text: String,
}

/// Parse the summary block of an `.errors.txt` (`WriteFormatDiagnostics`):
/// one `[<file>(<line>,<col>): ]<category> TS<code>: <text>` header per
/// diagnostic, chain lines indented below it, ending at the first blank line.
pub fn parse_errors_baseline(text: &str) -> Vec<BaselineDiagnostic> {
    if text.contains("\x1b[") {
        return parse_pretty_errors_baseline(text);
    }
    let mut diagnostics = Vec::new();
    for line in text.split("\r\n") {
        if line.is_empty() {
            break;
        }
        if line.starts_with(' ') {
            continue;
        }
        if let Some(diagnostic) = parse_summary_line(line) {
            diagnostics.push(diagnostic);
        }
    }
    diagnostics
}

/// A `@pretty` baseline (`FormatDiagnosticsWithColorAndContext`): ANSI
/// colors, one `[<file>:<line>:<col> - ]<category> TS<code>: <text>` header
/// per diagnostic followed by its code frame, chain lines and related rows
/// (all indented or framed, so only the headers parse), and a trailing
/// `Found N errors` summary. Blank lines separate diagnostics here.
fn parse_pretty_errors_baseline(text: &str) -> Vec<BaselineDiagnostic> {
    let plain = strip_ansi(text);
    plain
        .split('\n')
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.is_empty() && !line.starts_with(' '))
        .filter_map(parse_pretty_header)
        .collect()
}

fn strip_ansi(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("\x1b[") {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find(|c: char| c.is_ascii_alphabetic())
            .map_or(after.len(), |end| end + 1);
        rest = &after[end..];
    }
    plain.push_str(rest);
    plain
}

fn parse_pretty_header(line: &str) -> Option<BaselineDiagnostic> {
    const CATEGORIES: [&str; 4] = ["error", "warning", "suggestion", "message"];
    let (location, rest) = match line.split_once(" - ") {
        Some((location, rest))
            if CATEGORIES
                .iter()
                .any(|category| rest.starts_with(&format!("{category} TS"))) =>
        {
            (Some(location), rest)
        }
        _ if CATEGORIES
            .iter()
            .any(|category| line.starts_with(&format!("{category} TS"))) =>
        {
            (None, line)
        }
        _ => return None,
    };
    let (category, rest) = rest.split_once(" TS")?;
    let (code, text) = rest.split_once(": ")?;
    let (file, line_number, column) = match location {
        None => (None, None, None),
        Some(location) => {
            let (file_and_line, column) = location.rsplit_once(':')?;
            let (file, line_number) = file_and_line.rsplit_once(':')?;
            (
                Some(file.to_owned()),
                line_number.parse().ok(),
                column.parse().ok(),
            )
        }
    };
    Some(BaselineDiagnostic {
        file,
        line: line_number,
        column,
        code: code.parse().ok()?,
        category: category.to_owned(),
        text: text.to_owned(),
    })
}

fn parse_summary_line(line: &str) -> Option<BaselineDiagnostic> {
    const CATEGORIES: [&str; 4] = ["error", "warning", "suggestion", "message"];
    let (location, rest) = if CATEGORIES
        .iter()
        .any(|category| line.starts_with(&format!("{category} TS")))
    {
        (None, line)
    } else {
        let marker = CATEGORIES
            .iter()
            .filter_map(|category| line.find(&format!("): {category} TS")))
            .min()?;
        let open = line[..marker].rfind('(')?;
        (
            Some((&line[..open], &line[open + 1..marker])),
            &line[marker + 3..],
        )
    };
    let (category, rest) = rest.split_once(" TS")?;
    let (code, text) = rest.split_once(": ")?;
    let (file, line_number, column) = match location {
        None => (None, None, None),
        Some((file, position)) => {
            let (line_number, column) = position.split_once(',')?;
            (
                Some(file.to_owned()),
                line_number.parse().ok(),
                column.parse().ok(),
            )
        }
    };
    Some(BaselineDiagnostic {
        file,
        line: line_number,
        column,
        code: code.parse().ok()?,
        category: category.to_owned(),
        text: text.to_owned(),
    })
}

/// A tsc-rs diagnostic in the baseline's terms.
fn baseline_view(
    diagnostic: &Diagnostic,
    texts: &BTreeMap<String, String>,
    indexes: &mut BTreeMap<String, PositionIndex>,
) -> BaselineDiagnostic {
    let text = remove_test_path_prefixes(&diagnostic.message.text.to_string_lossy());
    let category = diagnostic.category().name().to_owned();
    let code = diagnostic.code();
    let Some(file_name) = diagnostic.file_name.as_ref() else {
        return BaselineDiagnostic {
            file: None,
            line: None,
            column: None,
            code,
            category,
            text,
        };
    };
    let file_name = file_name.to_string_lossy();
    let Some(source) = texts.get(file_name.as_ref()) else {
        // Not a fixture unit: a standard library file, whose location the
        // baseline masks.
        let base = file_name
            .rsplit('/')
            .next()
            .unwrap_or(&file_name)
            .to_owned();
        return BaselineDiagnostic {
            file: Some(base),
            line: None,
            column: None,
            code,
            category,
            text,
        };
    };
    let (line, column) = match diagnostic.start {
        Some(start) => {
            let index = indexes
                .entry(file_name.to_string())
                .or_insert_with(|| PositionIndex::new_static(source));
            match index.line_and_character_utf16(start) {
                Some(position) => (Some(position.line + 1), Some(position.character + 1)),
                None => (None, None),
            }
        }
        None => (None, None),
    };
    BaselineDiagnostic {
        file: Some(remove_test_path_prefixes(&file_name)),
        line,
        column,
        code,
        category,
        text,
    }
}

/// The deepest tier on which two multisets of diagnostics agree.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Agreement {
    /// Not even the (file, line, column, code) multiset agrees.
    None,
    /// T0: file, line, column and code.
    Location,
    /// T1: plus category.
    Category,
    /// T2: plus the first line of the message.
    Text,
    /// T3: the whole `.errors.txt` is byte-identical (spans, message chains,
    /// related information and order), which is what the native test
    /// requires; for `@pretty`, with the pretty diagnostics and summary.
    Full,
}

/// Whether an emit baseline (`.js`, `.js.map`) matches its reference byte
/// for byte. `Full` also when neither side has one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum EmitAgreement {
    None,
    /// The native runner writes no baseline for the case (`skippedEmitTests`
    /// in `compiler_runner.go`: output order or contents depend on its
    /// concurrency), so there is nothing to compare.
    NotAssessed,
    Full,
}

/// `skippedEmitTests` (`compiler_runner.go`): the cases whose JavaScript
/// baseline the native runner skips.
const SKIPPED_EMIT_TESTS: [&str; 8] = [
    "filesEmittingIntoSameOutput.ts",
    "jsFileCompilationWithJsEmitPathSameAsInput.ts",
    "grammarErrors.ts",
    "jsFileCompilationEmitBlockedCorrectly.ts",
    "jsDeclarationsReexportAliasesEsModuleInterop.ts",
    "jsFileCompilationWithoutJsExtensions.ts",
    "typeOnlyMerge2.ts",
    "typeOnlyMerge3.ts",
];

/// Compare a rendered emit baseline with its reference: the agreement and,
/// for a mismatch, a short reason for the record.
fn emit_agreement(
    rendered: Option<&str>,
    expected: Option<&str>,
    error: Option<&str>,
) -> (EmitAgreement, Option<String>) {
    if let Some(error) = error {
        return (EmitAgreement::None, Some(error.to_owned()));
    }
    match (rendered, expected) {
        (None, None) => (EmitAgreement::Full, None),
        (Some(rendered), Some(expected)) if rendered == expected => (EmitAgreement::Full, None),
        (Some(_), None) => (EmitAgreement::None, Some("reference missing".to_owned())),
        (None, Some(_)) => (EmitAgreement::None, Some("output missing".to_owned())),
        (Some(rendered), Some(expected)) => {
            let line = rendered
                .split('\n')
                .zip(expected.split('\n'))
                .position(|(a, b)| a != b)
                .map_or_else(
                    || {
                        rendered
                            .split('\n')
                            .count()
                            .min(expected.split('\n').count())
                    },
                    |index| index + 1,
                );
            (EmitAgreement::None, Some(format!("differs at line {line}")))
        }
    }
}

fn agreement(expected: &[BaselineDiagnostic], actual: &[BaselineDiagnostic]) -> Agreement {
    fn sorted<T: Ord + Clone>(items: impl Iterator<Item = T>) -> Vec<T> {
        let mut items: Vec<T> = items.collect();
        items.sort();
        items
    }
    let location = |items: &[BaselineDiagnostic]| {
        sorted(
            items
                .iter()
                .map(|d| (d.file.clone(), d.line, d.column, d.code)),
        )
    };
    if location(expected) != location(actual) {
        return Agreement::None;
    }
    let category = |items: &[BaselineDiagnostic]| {
        sorted(
            items
                .iter()
                .map(|d| (d.file.clone(), d.line, d.column, d.code, d.category.clone())),
        )
    };
    if category(expected) != category(actual) {
        return Agreement::Location;
    }
    if sorted(expected.iter().cloned()) != sorted(actual.iter().cloned()) {
        return Agreement::Category;
    }
    Agreement::Text
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
// One result per configuration; the compared variant's size is immaterial.
#[allow(clippy::large_enum_variant)]
pub enum Outcome {
    /// Lane A, compared.
    Compared {
        agreement: Agreement,
        expected: usize,
        actual: usize,
        /// The first expected diagnostic tsc-rs did not report and the first
        /// reported one the baseline lacks, on the location tier.
        missing: Option<BaselineDiagnostic>,
        unexpected: Option<BaselineDiagnostic>,
        /// SHA-256 of tsc-rs's rendered error baseline (the empty string when
        /// it reports nothing). Two runs with the same digest produced the
        /// same diagnostics in the same order, whatever their tier.
        rendered_sha256: Option<String>,
        /// Whether tsc-rs's JavaScript emit baseline (the `.js` reference:
        /// the JavaScript and declaration files) matches byte for byte.
        emit: EmitAgreement,
        /// Why the emit differs: the first differing line, the missing side,
        /// a reference section the runner does not reproduce, or an emit
        /// error.
        emit_detail: Option<String>,
        /// SHA-256 of tsc-rs's rendered `.js` baseline (the empty string when
        /// it emitted nothing).
        emit_sha256: String,
        /// The same for the `.js.map` reference (the raw source maps).
        map: EmitAgreement,
        map_detail: Option<String>,
    },
    /// Lane A, but tsc-rs could not build or check the Program.
    HarnessError { reason: String },
    /// The native runner's `SkipUnsupportedCompilerOptions` skips it: an
    /// option TypeScript 7 removed.
    Skipped { rule: String },
    /// The native runner does not produce baselines for it.
    NotRun { reason: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfigurationResult {
    pub suite: &'static str,
    pub case: String,
    pub configuration: String,
    pub stem: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

pub struct RunOptions {
    pub profile: String,
    /// Keep the cases whose suite-relative path contains this text.
    pub filter: Option<String>,
    /// Keep only this `<suite>/<path>` case.
    pub case: Option<String>,
    pub threads: usize,
    /// Checkers per configuration. The conformance comparison runs on one
    /// checker, the exact reference; a sharded run is the parallel control
    /// that must agree with it up to member order.
    pub checkers: usize,
    /// Write the rendered error baseline of every lane-A configuration that
    /// differs from its reference to `<dump>/<suite>/<stem>.errors.txt` (an
    /// empty file when tsc-rs reports nothing), for diffing; likewise the
    /// `.js` and `.js.map` baselines that differ.
    pub dump: Option<PathBuf>,
}

/// A stack overflow aborts the process, so the threads that run cases reserve
/// far more than the CLI's checker threads: the corpus includes deeply nested
/// stress cases. The reservation is virtual.
const CASE_STACK_BYTES: usize = 256 << 20;

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(512, 8_192, 64, 64 * 1024 * 1024, 256 * 1024 * 1024)
}

/// Fixture units by their normalized absolute unit path, with their text.
type Units = Vec<(String, String)>;

/// The fixture units of the plan's groups (`tsConfigFiles`, `toBeCompiled`,
/// `otherFiles`).
fn plan_units(plan: &CompilerExecutionPlan) -> (Units, Units, Units) {
    let (config, roots, others) = match &plan.root_selection {
        CompilerRootSelection::Explicit {
            root_units,
            other_units,
            ..
        } => (None, root_units, other_units),
        CompilerRootSelection::Config {
            config_unit,
            root_units,
            other_units,
            ..
        } => (Some(*config_unit), root_units, other_units),
    };
    let unit = |id: &tsc_harness::upstream_suites::execution::CompilerUnitId| {
        plan.fixture
            .units
            .iter()
            .find(|unit| unit.id == *id)
            .map(|unit| {
                let content = unit.content.as_deref().unwrap_or_default().to_owned();
                (absolute(&plan.current_directory, &unit.name), content)
            })
    };
    (
        config.iter().filter_map(unit).collect(),
        roots.iter().filter_map(unit).collect(),
        others.iter().filter_map(unit).collect(),
    )
}

/// The fixture files in the order the native runner passes them to the
/// error baseline (`tsConfigFiles`, `toBeCompiled`, `otherFiles`).
fn baseline_input_files(plan: &CompilerExecutionPlan) -> Vec<(String, String)> {
    let (config, roots, others) = plan_units(plan);
    config.into_iter().chain(roots).chain(others).collect()
}

/// The sources the JavaScript emit baseline lists (`otherFiles`, then
/// `toBeCompiled`; no config file).
fn js_baseline_sources(plan: &CompilerExecutionPlan) -> Vec<(String, String)> {
    let (_, roots, others) = plan_units(plan);
    others.into_iter().chain(roots).collect()
}

/// What the declaration compile and the noCheck comparison of the emit
/// baseline read of the emitting Program (`result.Options` and
/// `result.Program` in `DoJSEmitBaseline`).
#[derive(Default)]
struct EmitFacts {
    declaration: bool,
    allow_js: bool,
    no_check: bool,
    no_emit: bool,
    no_emit_on_error: bool,
    /// The Program's current directory, which resolves relative output paths.
    current_directory: String,
    /// The normalized absolute `outDir`.
    out_dir: Option<String>,
    /// The normalized absolute `declarationDir`.
    declaration_dir: Option<String>,
    /// `jsx` is `preserve`, which keeps a `.jsx` or `.tsx` file's output a
    /// `.jsx` file (`GetOutputExtension`).
    jsx_preserve: bool,
    /// `Program.CommonSourceDirectory()`: empty or ending with `/`.
    common_source_directory: String,
    /// The normalized absolute paths of the Program's source files.
    source_paths: HashSet<String>,
    /// The same paths in Program order (`program.GetSourceFiles()`).
    source_order: Vec<String>,
}

impl EmitFacts {
    fn new(prepared: &tsc_program::PreparedProgram) -> Self {
        let options = prepared.compiler_options();
        let on = |value: Option<bool>| value == Some(true);
        let current_directory = prepared.current_directory().display();
        let config = prepared
            .program_options()
            .config_file_path()
            .map(|path| path.display());
        let case_sensitive = prepared.path_context().use_case_sensitive_file_names();
        // getCommonSourceDirectory over the files that may be emitted, as the
        // emitter computes it.
        let emitted: Vec<_> = prepared
            .source_files()
            .iter()
            .filter(|source| {
                tsc_program::source_file_may_be_emitted_for_options(
                    source.path().display(),
                    source.may_be_emitted(),
                    options,
                    config,
                    current_directory,
                    case_sensitive,
                )
            })
            .map(|source| source.path().display())
            .collect();
        let common_source_directory = tsc_program::common_source_directory(
            options,
            config,
            &emitted,
            current_directory,
            case_sensitive,
        );
        let current_directory = current_directory.to_string_lossy().into_owned();
        let source_order: Vec<_> = prepared
            .source_files()
            .iter()
            .map(|source| {
                absolute(
                    &current_directory,
                    &source.path().display().to_string_lossy(),
                )
            })
            .collect();
        Self {
            declaration: on(options.declaration),
            allow_js: options.allow_js,
            no_check: on(options.no_check),
            no_emit: on(options.no_emit),
            no_emit_on_error: on(options.no_emit_on_error),
            out_dir: options.out_dir.as_ref().map(|directory| {
                absolute(&current_directory, &directory.as_js().to_string_lossy())
            }),
            declaration_dir: options.declaration_dir.as_ref().map(|directory| {
                absolute(&current_directory, &directory.as_js().to_string_lossy())
            }),
            jsx_preserve: options.jsx == Some(1),
            common_source_directory: common_source_directory
                .as_js()
                .to_string_lossy()
                .into_owned(),
            source_paths: source_order.iter().cloned().collect(),
            source_order,
            current_directory,
        }
    }
}

/// `GetNormalizedAbsolutePath(path, currentDirectory)`, keeping a rooted
/// name such as `c:/app/main.ts` (`GetRootLength`).
fn absolute(current_directory: &str, path: &str) -> String {
    normalize_compiler_fixture_path(current_directory, path).unwrap_or_else(|_| path.to_owned())
}

/// What the native harness's second Program of a configuration produces.
struct SecondProgram {
    emission: Emission,
    map_options: MapOptions,
    facts: EmitFacts,
    /// The diagnostics the Program reports after its emit. `None` when the
    /// Program does not emit before its diagnostics are known (it cannot
    /// emit, `noEmitOnError` asks for them first, or the control runs it
    /// with several checkers): the first Program's diagnostics are then the
    /// second's.
    diagnostics: Option<Vec<Diagnostic>>,
    /// Why the emit failed, when the diagnostics could still be collected.
    emit_error: Option<String>,
}

/// Run the configuration as the native harness's second Program does
/// (`compileFilesWithHost`, harnessutil.go:673-688: `program.Emit`, then the
/// diagnostic getters), collecting the written files, the diagnostics
/// reported after the emit, the map options of the Program's effective
/// compiler options (the directives and the config file) and the facts the
/// emit baseline reads. The emit comes first: what it resolves through the
/// checker is resolved in emit order, and that is the order the reference
/// baselines were written in. Nothing is emitted for a Program that cannot
/// emit (`noEmit`).
fn emit_outputs(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    test_library: &Path,
    standard_library: &Path,
    budget: CheckerBudget,
    collection: NativeHarnessCollection,
) -> Result<SecondProgram, String> {
    let prepared =
        load_native_compiler_program(workspace, plan, limits(), test_library, standard_library)
            .map_err(|error| format!("load: {error}"))?;
    let options = prepared.compiler_options();
    let on = |value: Option<bool>| value == Some(true);
    let map_options = MapOptions {
        source_map: on(options.source_map),
        // GetAreDeclarationMapsEnabled: declaration maps need declarations.
        declaration_map: on(options.declaration_map)
            && (on(options.declaration) || on(options.composite)),
        inline_source_map: on(options.inline_source_map),
        no_emit_on_error: on(options.no_emit_on_error),
    };
    let facts = EmitFacts::new(&prepared);
    if prepared.mode() != PreparedProgramMode::Emit {
        return Ok(SecondProgram {
            emission: Emission::default(),
            map_options,
            facts,
            diagnostics: None,
            emit_error: None,
        });
    }
    let mut sink = MemoryOutputSink::new();
    let session = ProgramSession::new(prepared).with_checker_budget(budget);
    let (diagnostics, emit_error) = match session
        .emit_then_run_for_native_harness(collection, &mut sink)
        .map_err(|error| format!("emit: {error}"))?
    {
        Ok((outcome, emit)) => (
            Some(outcome.native_harness_diagnostics().to_vec()),
            emit.and_then(Result::err)
                .map(|error| format!("emit: {}", DriverError::Emit(error))),
        ),
        // The Program's emit asks for the diagnostics first, or the
        // budget has several checkers: the order of the first Program.
        Err(session) => {
            session
                .emit(&mut sink)
                .map_err(|error| format!("emit: {error}"))?;
            (None, None)
        }
    };
    let emission = if emit_error.is_some() {
        Emission::default()
    } else {
        harness_order(Emission::from_writes(sink.writes()), &facts)
    };
    Ok(SecondProgram {
        emission,
        map_options,
        facts,
        diagnostics,
        emit_error,
    })
}

/// Order the emitted files as the native harness's `newCompilationResult`
/// does (harnessutil.go:746-834): for each Program source file that is not
/// a declaration file, in Program order, the JavaScript, declaration and map
/// files at the paths `getOutputPath` computes, then the files no source
/// matched (a declaration under a `declarationDir` other than `outDir`, a
/// declaration map, an `outFile` bundle), sorted by name.
fn harness_order(emission: Emission, facts: &EmitFacts) -> Emission {
    let Emission { js, dts, maps } = emission;
    let mut js = HarnessGroup::new(js, facts);
    let mut dts = HarnessGroup::new(dts, facts);
    let mut maps = HarnessGroup::new(maps, facts);
    for source in &facts.source_order {
        if is_declaration_file_name(source) {
            continue;
        }
        let extension = output_extension(source, facts.jsx_preserve);
        js.take(harness_output_path(facts, source, extension));
        dts.take(harness_output_path(
            facts,
            source,
            &declaration_emit_extension(source),
        ));
        maps.take(harness_output_path(
            facts,
            source,
            &format!("{extension}.map"),
        ));
    }
    Emission {
        js: js.finish(),
        dts: dts.finish(),
        maps: maps.finish(),
    }
}

/// One output group of [`harness_order`]: the files a source claimed, in
/// claim order, and the files not yet claimed with their absolute names.
struct HarnessGroup {
    claimed: Vec<EmittedFile>,
    unclaimed: Vec<(String, EmittedFile)>,
}

impl HarnessGroup {
    fn new(files: Vec<EmittedFile>, facts: &EmitFacts) -> Self {
        let unclaimed = files
            .into_iter()
            .map(|file| (absolute(&facts.current_directory, &file.path), file))
            .collect();
        Self {
            claimed: Vec::new(),
            unclaimed,
        }
    }

    fn take(&mut self, path: Option<String>) {
        let Some(path) = path else {
            return;
        };
        if let Some(index) = self.unclaimed.iter().position(|(name, _)| *name == path) {
            let (_, file) = self.unclaimed.remove(index);
            self.claimed.push(file);
        }
    }

    fn finish(mut self) -> Vec<EmittedFile> {
        self.unclaimed
            .sort_by(|(left, _), (right, _)| left.cmp(right));
        self.claimed
            .extend(self.unclaimed.into_iter().map(|(_, file)| file));
        self.claimed
    }
}

/// The harness's `getOutputPath(path, ext)` (harnessutil.go:836-860): under
/// an output directory, the source's path relative to the common source
/// directory is placed under `outDir` (also for a declaration, whose
/// `declarationDir` only decides whether a directory applies). `None` when
/// the path leaves the common source directory (`..` segments, which no
/// written file name has) or keeps no known extension to change.
fn harness_output_path(facts: &EmitFacts, source: &str, extension: &str) -> Option<String> {
    let declaration = is_declaration_extension(extension);
    let directory = if declaration {
        facts.declaration_dir.as_ref().or(facts.out_dir.as_ref())
    } else {
        facts.out_dir.as_ref()
    };
    let mut path = source.to_owned();
    if directory.is_some() && !facts.common_source_directory.is_empty() {
        let relative = source.strip_prefix(&facts.common_source_directory)?;
        let out_dir = facts
            .out_dir
            .clone()
            .unwrap_or_else(|| facts.current_directory.clone());
        path = format!("{}/{relative}", out_dir.trim_end_matches('/'));
    }
    if extension == declaration_emit_extension(&path) {
        return Some(change_to_declaration_extension(&path));
    }
    const KNOWN: [&str; 12] = [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ];
    let known = KNOWN.iter().find(|known| path.ends_with(*known))?;
    Some(format!("{}{extension}", &path[..path.len() - known.len()]))
}

/// `outputpaths.GetOutputExtension`.
fn output_extension(path: &str, jsx_preserve: bool) -> &'static str {
    if path.ends_with(".json") {
        ".json"
    } else if jsx_preserve && (path.ends_with(".jsx") || path.ends_with(".tsx")) {
        ".jsx"
    } else if path.ends_with(".mts") || path.ends_with(".mjs") {
        ".mjs"
    } else if path.ends_with(".cts") || path.ends_with(".cjs") {
        ".cjs"
    } else {
        ".js"
    }
}

/// `tspath.GetDeclarationEmitExtensionForPath`.
fn declaration_emit_extension(path: &str) -> String {
    if path.ends_with(".mjs") || path.ends_with(".mts") {
        ".d.mts".to_owned()
    } else if path.ends_with(".cjs") || path.ends_with(".cts") {
        ".d.cts".to_owned()
    } else if [".ts", ".tsx", ".js", ".jsx"]
        .iter()
        .any(|extension| path.ends_with(extension))
    {
        ".d.ts".to_owned()
    } else {
        let base = path.rsplit('/').next().unwrap_or(path);
        match base.rfind('.') {
            Some(index) => format!(".d{}.ts", &base[index..]),
            None => ".d.ts".to_owned(),
        }
    }
}

/// The declaration extensions `getOutputPath` places under `declarationDir`.
fn is_declaration_extension(extension: &str) -> bool {
    matches!(extension, ".d.ts" | ".d.mts" | ".d.cts")
        || (extension.ends_with(".ts") && extension.contains(".d."))
}

/// The declaration files the native runner compiles again for one fixture
/// unit (`addDtsFile` in `prepareDeclarationCompilationContext`,
/// js_emit_baseline.go:223-237): a declaration or JSON unit as written, or
/// the emitted declaration of a TypeScript unit (a JavaScript one under
/// `allowJs`) the Program contains, unless already listed.
fn add_dts_file(
    (path, content): &(String, String),
    facts: &EmitFacts,
    emission: &Emission,
    inputs: &[(String, String)],
    others: &[(String, String)],
) -> Option<(String, String)> {
    if is_declaration_file_name(path) || path.ends_with(".json") {
        return Some((path.clone(), content.clone()));
    }
    let typescript = [".ts", ".tsx", ".mts", ".cts"]
        .iter()
        .any(|extension| path.ends_with(extension));
    let javascript = [".js", ".jsx", ".mjs", ".cjs"]
        .iter()
        .any(|extension| path.ends_with(extension));
    if !facts.source_paths.contains(path) || !(typescript || (javascript && facts.allow_js)) {
        return None;
    }
    // findResultCodeFile: the declaration path computed from `outDir` and
    // the common source directory (declarationDir is not consulted).
    let source_file_name = match &facts.out_dir {
        Some(out_dir) => {
            let relative = path.replacen(&facts.common_source_directory, "", 1);
            absolute("/", &format!("{out_dir}/{relative}"))
        }
        None => path.clone(),
    };
    let declaration = change_to_declaration_extension(&source_file_name);
    let file = emission
        .dts
        .iter()
        .find(|file| absolute(&facts.current_directory, &file.path) == declaration)?;
    let listed = |files: &[(String, String)]| files.iter().any(|(name, _)| *name == declaration);
    if listed(inputs) || listed(others) {
        return None;
    }
    let content = file
        .content
        .strip_prefix('\u{feff}')
        .unwrap_or(&file.content);
    Some((declaration, content.to_owned()))
}

/// `tspath.IsDeclarationFileName`: `.d.ts`, `.d.mts`, `.d.cts`, or a
/// `.d.<extension>.ts` name.
fn is_declaration_file_name(path: &str) -> bool {
    let base = path.rsplit('/').next().unwrap_or(path);
    [".d.ts", ".d.mts", ".d.cts"]
        .iter()
        .any(|extension| base.ends_with(extension))
        || (base.ends_with(".ts") && base.contains(".d."))
}

/// `outputpaths.ChangeToDeclarationExtension` without content mappers.
fn change_to_declaration_extension(path: &str) -> String {
    const EXTENSIONS: [&str; 10] = [
        ".d.ts", ".d.mts", ".d.cts", ".ts", ".tsx", ".mts", ".cts", ".js", ".jsx", ".json",
    ];
    const MORE: [&str; 2] = [".mjs", ".cjs"];
    let extension = EXTENSIONS
        .iter()
        .chain(MORE.iter())
        .find(|extension| path.ends_with(*extension));
    let stem = extension.map_or(path, |extension| &path[..path.len() - extension.len()]);
    let declaration = match extension.copied() {
        Some(".mts" | ".mjs") => ".d.mts",
        Some(".cts" | ".cjs") => ".d.cts",
        Some(".json") => ".d.json.ts",
        _ => ".d.ts",
    };
    format!("{stem}{declaration}")
}

/// The `[DtsFileErrors]` text: when the configuration requested
/// declarations, had no diagnostics and emitted some, its declaration files
/// are compiled again with the same options (`compileDeclarationFiles`), and
/// that compile's diagnostics are rendered over the config file and the
/// compiled declarations. Empty when there are none.
#[allow(clippy::too_many_arguments)]
fn declaration_file_errors(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    facts: &EmitFacts,
    emission: &Emission,
    had_diagnostics: bool,
    test_library: &Path,
    standard_library: &Path,
    budget: CheckerBudget,
    collection: NativeHarnessCollection,
) -> Result<String, String> {
    if !facts.declaration || had_diagnostics || emission.dts.is_empty() {
        return Ok(String::new());
    }
    let (config, roots, others) = plan_units(plan);
    let mut declaration_inputs = Vec::new();
    for unit in &roots {
        if let Some(file) = add_dts_file(unit, facts, emission, &declaration_inputs, &[]) {
            declaration_inputs.push(file);
        }
    }
    let mut declaration_others = Vec::new();
    for unit in &others {
        if let Some(file) = add_dts_file(
            unit,
            facts,
            emission,
            &declaration_inputs,
            &declaration_others,
        ) {
            declaration_others.push(file);
        }
    }
    let prepared = load_native_declaration_program(
        workspace,
        plan,
        &declaration_inputs,
        &declaration_others,
        limits(),
        test_library,
        standard_library,
    )
    .map_err(|error| format!("declaration load: {error}"))?;
    let outcome = ProgramSession::new(prepared)
        .with_checker_budget(budget)
        .run_for_native_harness(collection)
        .map_err(|error| format!("declaration check: {error}"))?;
    let diagnostics = outcome.native_harness_diagnostics();
    let files: Vec<_> = config
        .iter()
        .chain(&declaration_inputs)
        .chain(&declaration_others)
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    let library = if files.iter().any(|file| file.content.contains("/.lib/")) {
        read_test_library(test_library)
            .map(|library| {
                library
                    .into_iter()
                    .map(|(relative, content)| (format!("/.lib/{relative}"), content.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let library_inputs: Vec<_> = library
        .iter()
        .filter(|(name, _)| !files.iter().any(|file| file.name == name))
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    Ok(
        errors_baseline::render(diagnostics, &files, &library_inputs, false)
            .map(|errors| emit_baseline::dts_file_errors_section(&errors))
            .unwrap_or_default(),
    )
}

/// The noCheck comparison (`result.Repeat` with `noCheck`): the files only a
/// noCheck emit writes, or writes differently. tsgo's noCheck emit equals its
/// checked emit except where `noEmitOnError` stopped the checked emit after
/// diagnostics (the reference baselines show no other difference), so only
/// such a configuration is emitted again.
#[allow(clippy::too_many_arguments)]
fn no_check_comparison(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    facts: &EmitFacts,
    emission: &Emission,
    had_diagnostics: bool,
    test_library: &Path,
    standard_library: &Path,
    budget: CheckerBudget,
    full_emit_paths: bool,
) -> Result<String, String> {
    if facts.no_check || facts.no_emit || !(facts.no_emit_on_error && had_diagnostics) {
        return Ok(String::new());
    }
    let mut no_check_plan = plan.clone();
    let mut settings = plan.effective_settings.to_vec();
    settings.push(OrderedSetting {
        name: "noCheck".to_owned(),
        value: "true".to_owned(),
    });
    no_check_plan.effective_settings = settings.into();
    let no_check = emit_outputs(
        workspace,
        &no_check_plan,
        test_library,
        standard_library,
        budget,
        NativeHarnessCollection::default(),
    )
    .and_then(|second| match second.emit_error {
        Some(error) => Err(error),
        None => Ok(second.emission),
    })
    .map_err(|error| format!("noCheck {error}"))?;
    Ok(emit_baseline::no_check_sections(
        emission,
        &no_check,
        full_emit_paths,
    ))
}

fn sha256_hex(text: &str) -> String {
    format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
}

fn dump_file(directory: &Path, suite: NativeSuite, file_name: &str, content: &str) {
    let path = directory.join(suite.name()).join(file_name);
    let written = std::fs::create_dir_all(directory.join(suite.name()))
        .and_then(|()| std::fs::write(&path, content));
    if let Err(error) = written {
        eprintln!("{}: {error}", path.display());
    }
}

#[allow(clippy::too_many_arguments)]
fn run_lane_a(
    workspace: &Path,
    profile: &NativeProfile,
    (suite, case_path): (NativeSuite, &str),
    (configuration, stem): (&NativeConfiguration, &str),
    plan: &CompilerExecutionPlan,
    dump: Option<&Path>,
    checkers: usize,
) -> Outcome {
    let flag = |name: &str| {
        configuration
            .settings
            .get(name)
            .is_some_and(|value| value.eq_ignore_ascii_case("true"))
    };
    let pretty = flag("pretty");
    let collection = NativeHarnessCollection {
        capture_suggestions: flag("capturesuggestions"),
    };
    let test_library = profile.test_library_root();
    let standard_library = profile.bundled_libraries_root();
    let prepared = match load_native_compiler_program(
        workspace,
        plan,
        limits(),
        &test_library,
        &standard_library,
    ) {
        Ok(prepared) => prepared,
        Err(error) => {
            return Outcome::HarnessError {
                reason: format!("load: {error}"),
            }
        }
    };
    let budget = || {
        std::num::NonZeroUsize::new(checkers).map_or(CheckerBudget::serial(), CheckerBudget::new)
    };
    let outcome = match ProgramSession::new(prepared)
        .with_checker_budget(budget())
        .run_for_native_harness(collection)
    {
        Ok(outcome) => outcome,
        Err(error) => {
            return Outcome::HarnessError {
                reason: format!("check: {error}"),
            }
        }
    };
    let files = baseline_input_files(plan);
    // A fixture that mentions `/.lib/` compiles the profile's test library
    // too (the loader mounts it), and the native baseline locates rows and
    // related information in those files (`react18.d.ts:478:9`): their
    // texts feed the position index, without a section of their own.
    let library: Vec<(String, String)> = if files
        .iter()
        .any(|(_, content)| content.contains("/.lib/"))
    {
        read_test_library(&test_library)
            .map(|library| {
                library
                    .into_iter()
                    .map(|(relative, content)| (format!("/.lib/{relative}"), content.to_string()))
                    .filter(|(name, _)| !files.iter().any(|(existing, _)| existing == name))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let texts: BTreeMap<String, String> = files.iter().cloned().collect();
    let mut indexes = BTreeMap::new();
    // The native harness compiles a configuration twice
    // (`compileFilesWithHost`, harnessutil.go:647-712): the first Program
    // only reports its diagnostics, the second emits and then reports them,
    // and the errors baseline holds the second Program's. Should the two
    // counts differ, the harness keeps the shorter list and adds a row no
    // reference baseline contains.
    let (second, mut emit_error) = match emit_outputs(
        workspace,
        plan,
        &test_library,
        &standard_library,
        budget(),
        collection,
    ) {
        Ok(mut second) => {
            let error = second.emit_error.take();
            (second, error)
        }
        Err(error) => (
            SecondProgram {
                emission: Emission::default(),
                map_options: MapOptions::default(),
                facts: EmitFacts::default(),
                diagnostics: None,
                emit_error: None,
            },
            Some(error),
        ),
    };
    let first_diagnostics = outcome.native_harness_diagnostics();
    let (diagnostics, counts) = match second.diagnostics.as_deref() {
        Some(after_emit) if after_emit.len() == first_diagnostics.len() => (after_emit, None),
        Some(after_emit) => (
            if after_emit.len() < first_diagnostics.len() {
                after_emit
            } else {
                first_diagnostics
            },
            Some((first_diagnostics.len(), after_emit.len())),
        ),
        None => (first_diagnostics, None),
    };
    let actual: Vec<_> = diagnostics
        .iter()
        .map(|diagnostic| baseline_view(diagnostic, &texts, &mut indexes))
        .collect();
    let expected_text = std::fs::read(profile.errors_baseline_path(suite, stem))
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let expected = expected_text
        .as_deref()
        .map(parse_errors_baseline)
        .unwrap_or_default();
    let mut agreement = agreement(&expected, &actual);
    let inputs: Vec<_> = files
        .iter()
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    let library_inputs: Vec<_> = library
        .iter()
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    let mut rendered = errors_baseline::render(diagnostics, &inputs, &library_inputs, pretty);
    if let Some((before_emit, after_emit)) = counts {
        agreement = Agreement::None;
        // The harness's row lists the diagnostics only the longer list has.
        let (longer, shorter) = match second.diagnostics.as_deref() {
            Some(post) if post.len() > first_diagnostics.len() => (post, first_diagnostics),
            Some(post) => (first_diagnostics, post),
            None => (first_diagnostics, first_diagnostics),
        };
        let mut excess = String::new();
        for diagnostic in longer.iter().filter(|d| !shorter.contains(d)) {
            let view = baseline_view(diagnostic, &texts, &mut indexes);
            excess.push_str(&format!(
                "  {}({},{}): TS{}: {}\n",
                view.file.as_deref().unwrap_or_default(),
                view.line.unwrap_or_default(),
                view.column.unwrap_or_default(),
                view.code,
                view.text
            ));
        }
        rendered = Some(format!(
            "Pre-emit ({before_emit}) and post-emit ({after_emit}) diagnostic counts do not match!\nThe excess diagnostics are:\n{excess}{}",
            rendered.unwrap_or_default()
        ));
    }
    let rendered_sha256 = Some(format!(
        "{:x}",
        sha2::Sha256::digest(rendered.as_deref().unwrap_or_default().as_bytes())
    ));
    if agreement == Agreement::Text && rendered == expected_text {
        agreement = Agreement::Full;
    } else if let Some(directory) = dump.filter(|_| rendered != expected_text) {
        dump_file(
            directory,
            suite,
            &format!("{stem}.errors.txt"),
            rendered.as_deref().unwrap_or_default(),
        );
    }
    let key = |d: &BaselineDiagnostic| (d.file.clone(), d.line, d.column, d.code);
    let missing = expected
        .iter()
        .find(|d| !actual.iter().any(|a| key(a) == key(d)))
        .cloned();
    let unexpected = actual
        .iter()
        .find(|a| !expected.iter().any(|d| key(a) == key(d)))
        .cloned();

    // The emit of the second Program against the `.js` and `.js.map`
    // references.
    let (emission, map_options, facts) = (&second.emission, second.map_options, &second.facts);
    let full_emit_paths = flag("fullemitpaths");
    let mut sections = String::new();
    if emit_error.is_none() {
        let had_diagnostics = !diagnostics.is_empty();
        let extra = declaration_file_errors(
            workspace,
            plan,
            facts,
            emission,
            had_diagnostics,
            &test_library,
            &standard_library,
            budget(),
            collection,
        )
        .and_then(|errors| {
            no_check_comparison(
                workspace,
                plan,
                facts,
                emission,
                had_diagnostics,
                &test_library,
                &standard_library,
                budget(),
                full_emit_paths,
            )
            .map(|comparison| errors + &comparison)
        });
        match extra {
            Ok(extra) => sections = extra,
            Err(error) => emit_error = Some(error),
        }
    }
    let header = format!("tests/cases/{}/{case_path}", suite.name());
    let sources = js_baseline_sources(plan);
    let source_inputs: Vec<_> = sources
        .iter()
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    let rendered_js = emit_baseline::render_js(
        &header,
        &source_inputs,
        emission,
        full_emit_paths,
        &sections,
    );
    let expected_js = std::fs::read(profile.js_baseline_path(suite, stem))
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let file_name = case_path.rsplit('/').next().unwrap_or(case_path);
    let (emit, emit_detail) = if SKIPPED_EMIT_TESTS.contains(&file_name) {
        (
            EmitAgreement::NotAssessed,
            Some("the native runner skips this case's JavaScript baseline".to_owned()),
        )
    } else {
        emit_agreement(
            rendered_js.as_deref(),
            expected_js.as_deref(),
            emit_error.as_deref(),
        )
    };
    let emit_sha256 = sha256_hex(rendered_js.as_deref().unwrap_or_default());
    // createSourceMapPreviewLink finds a map's sources among the Program's
    // files in Program order (`result.Inputs()`, sourcemap_baseline.go:90-100),
    // where a file another one imports comes first.
    let program_inputs: Vec<_> = facts
        .source_order
        .iter()
        .filter_map(|path| {
            files
                .iter()
                .find(|(name, _)| absolute(&facts.current_directory, name) == *path)
                .map(|(_, content)| errors_baseline::InputFile {
                    name: path,
                    content,
                })
        })
        .collect();
    let rendered_map = emit_baseline::render_js_map(
        map_options,
        !diagnostics.is_empty(),
        emission,
        &program_inputs,
        full_emit_paths,
    );
    let expected_map = std::fs::read(profile.js_map_baseline_path(suite, stem))
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let (map, map_detail) = emit_agreement(
        rendered_map.as_deref(),
        expected_map.as_deref(),
        emit_error.as_deref(),
    );
    if let Some(directory) = dump {
        if emit == EmitAgreement::None {
            dump_file(
                directory,
                suite,
                &format!("{stem}.js"),
                rendered_js.as_deref().unwrap_or_default(),
            );
        }
        if map == EmitAgreement::None {
            dump_file(
                directory,
                suite,
                &format!("{stem}.js.map"),
                rendered_map.as_deref().unwrap_or_default(),
            );
        }
    }
    Outcome::Compared {
        agreement,
        expected: expected.len(),
        actual: actual.len(),
        missing,
        unexpected,
        rendered_sha256,
        emit,
        emit_detail,
        emit_sha256,
        map,
        map_detail,
    }
}

fn case_key(case: &NativeCase) -> String {
    format!("{}/{}", case.suite.name(), case.relative_path)
}

fn parse_case_key(key: &str) -> Result<NativeCase, String> {
    let (suite, relative_path) = key
        .split_once('/')
        .ok_or_else(|| format!("{key}: expected <suite>/<path>"))?;
    let suite = NativeSuite::ALL
        .into_iter()
        .find(|candidate| candidate.name() == suite)
        .ok_or_else(|| format!("{key}: unknown suite {suite}"))?;
    Ok(NativeCase {
        suite,
        relative_path: relative_path.to_owned(),
    })
}

fn selected_cases(
    profile: &NativeProfile,
    options: &RunOptions,
) -> Result<Vec<NativeCase>, String> {
    let mut cases = profile.cases().map_err(|e| e.to_string())?;
    if let Some(filter) = &options.filter {
        cases.retain(|case| case.relative_path.contains(filter.as_str()));
    }
    if let Some(only) = &options.case {
        cases.retain(|case| case_key(case) == *only);
    }
    Ok(cases)
}

/// The `<suite>/<path>` keys of the cases `options` selects.
pub fn list(workspace: &Path, options: &RunOptions) -> Result<Vec<String>, String> {
    let profile = NativeProfile::load(workspace, &options.profile).map_err(|e| e.to_string())?;
    Ok(selected_cases(&profile, options)?
        .iter()
        .map(case_key)
        .collect())
}

/// Expand, route and run the selected cases on threads of this process. A
/// case that overflows the stack aborts the whole process;
/// `scripts/conformance_ts71.py` runs the corpus in restartable
/// [`run_worker`] processes instead.
pub fn run(workspace: &Path, options: &RunOptions) -> Result<Vec<ConfigurationResult>, String> {
    let profile = NativeProfile::load(workspace, &options.profile).map_err(|e| e.to_string())?;
    let cases = selected_cases(&profile, options)?;
    let next = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..options.threads.max(1) {
            std::thread::Builder::new()
                .name("conformance-ts71".to_owned())
                .stack_size(CASE_STACK_BYTES)
                .spawn_scoped(scope, || loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(index) else {
                        break;
                    };
                    let mut case_results = Vec::new();
                    run_case(
                        workspace,
                        &profile,
                        case,
                        &[],
                        options.dump.as_deref(),
                        options.checkers,
                        &mut |_, _| {},
                        &mut |result| {
                            case_results.push(result);
                        },
                    );
                    results
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend(case_results);
                })
                .expect("spawn a conformance worker");
        }
    });
    let mut results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    results.sort_by(|a, b| (a.suite, &a.case, &a.stem).cmp(&(b.suite, &b.case, &b.stem)));
    Ok(results)
}

/// Worker mode for `scripts/conformance_ts71.py`, which restarts a worker
/// process after a configuration that aborts it (a stack overflow cannot
/// unwind) or never finishes. Runs the cases listed in `cases_file`, one
/// `<suite>/<path>` key per line followed by any tab-separated stems to leave
/// out, and reports each step on stdout: `case <key>` before a case,
/// `begin <json>` with the configuration and stem before a lane-A
/// configuration runs, and `result <json>` for every configuration.
pub fn run_worker(
    workspace: &Path,
    profile: &str,
    cases_file: &Path,
    checkers: usize,
) -> Result<(), String> {
    let profile = NativeProfile::load(workspace, profile).map_err(|e| e.to_string())?;
    let listed = std::fs::read_to_string(cases_file)
        .map_err(|e| format!("{}: {e}", cases_file.display()))?;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("conformance-ts71".to_owned())
            .stack_size(CASE_STACK_BYTES)
            .spawn_scoped(scope, || -> Result<(), String> {
                for line in listed.lines().filter(|line| !line.is_empty()) {
                    let mut fields = line.split('\t');
                    let key = fields.next().expect("split yields a first field");
                    let leave_out: Vec<&str> = fields.collect();
                    let case = parse_case_key(key)?;
                    report_line(&format!("case {key}"));
                    run_case(
                        workspace,
                        &profile,
                        &case,
                        &leave_out,
                        None,
                        checkers,
                        &mut |configuration, stem| {
                            let begin = serde_json::json!({
                                "configuration": configuration.name,
                                "stem": stem,
                            });
                            report_line(&format!("begin {begin}"));
                        },
                        &mut |result| {
                            let result = serde_json::to_string(&result).expect("result json");
                            report_line(&format!("result {result}"));
                        },
                    );
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?
            .join()
            .map_err(|panic| format!("worker panic: {}", panic_text(&panic)))?
    })
}

/// One stdout line in a single write, so a worker killed while reporting
/// leaves at most a truncated last line.
fn report_line(line: &str) {
    use std::io::Write as _;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| stdout.flush())
        .expect("write to stdout");
}

/// Expand one case and run its configurations in order, leaving out the
/// stems in `leave_out`. `starting` hears of each lane-A configuration just
/// before it runs, the only step that can overflow the stack or run away;
/// `finished` receives every configuration's result. See [`RunOptions::dump`].
#[allow(clippy::too_many_arguments)]
fn run_case(
    workspace: &Path,
    profile: &NativeProfile,
    case: &NativeCase,
    leave_out: &[&str],
    dump: Option<&Path>,
    checkers: usize,
    starting: &mut dyn FnMut(&NativeConfiguration, &str),
    finished: &mut dyn FnMut(ConfigurationResult),
) {
    let result = |configuration: &str, stem: &str, outcome: Outcome| ConfigurationResult {
        suite: case.suite.name(),
        case: case.relative_path.clone(),
        configuration: configuration.to_owned(),
        stem: stem.to_owned(),
        outcome,
    };
    let expansion = match expand_case(profile, case) {
        Ok(expansion) => expansion,
        Err(error) => {
            finished(result(
                "",
                "",
                Outcome::HarnessError {
                    reason: format!("expand: {error}"),
                },
            ));
            return;
        }
    };
    let fixture = std::panic::catch_unwind(|| native_compiler_fixture(profile, case));
    for (configuration, stem, skip) in expansion.configurations {
        if leave_out.contains(&stem.as_str()) {
            continue;
        }
        let outcome = if let Some(skip) = skip {
            match skip {
                NativeSkip::Unsupported(rule) => Outcome::Skipped {
                    rule: rule.to_owned(),
                },
                other => Outcome::NotRun {
                    reason: format!("{other:?}"),
                },
            }
        } else {
            match &fixture {
                Ok(Ok(fixture)) => {
                    match native_compiler_plan(std::sync::Arc::clone(fixture), &configuration) {
                        Ok(plan) => {
                            starting(&configuration, &stem);
                            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                run_lane_a(
                                    workspace,
                                    profile,
                                    (case.suite, &case.relative_path),
                                    (&configuration, &stem),
                                    &plan,
                                    dump,
                                    checkers,
                                )
                            }))
                            .unwrap_or_else(|panic| {
                                Outcome::HarnessError {
                                    reason: format!("panic: {}", panic_text(&panic)),
                                }
                            })
                        }
                        Err(error) => Outcome::HarnessError {
                            reason: format!("plan: {error}"),
                        },
                    }
                }
                Ok(Err(error)) => Outcome::HarnessError {
                    reason: format!("fixture: {error}"),
                },
                Err(panic) => Outcome::HarnessError {
                    reason: format!("fixture panic: {}", panic_text(panic)),
                },
            }
        };
        finished(result(&configuration.name, &stem, outcome));
    }
}

fn panic_text(panic: &Box<dyn std::any::Any + Send>) -> String {
    panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_else(|| "non-string panic".to_owned())
}

/// Counts for the summary line and the report.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub configurations: usize,
    pub lane_a: usize,
    pub full: usize,
    pub text: usize,
    pub category: usize,
    pub location: usize,
    pub mismatch: usize,
    /// Compared configurations whose `.js` emit baseline matches / differs /
    /// is not assessed (the native runner skips it).
    pub emit_full: usize,
    pub emit_mismatch: usize,
    pub emit_not_assessed: usize,
    /// The same for the `.js.map` baseline.
    pub map_full: usize,
    pub map_mismatch: usize,
    pub harness_errors: usize,
    pub skipped: usize,
    pub not_run: usize,
}

pub fn summarize(results: &[ConfigurationResult]) -> Summary {
    let mut summary = Summary {
        configurations: results.len(),
        ..Summary::default()
    };
    for result in results {
        match &result.outcome {
            Outcome::Compared {
                agreement,
                emit,
                map,
                ..
            } => {
                summary.lane_a += 1;
                match agreement {
                    Agreement::Full => summary.full += 1,
                    Agreement::Text => summary.text += 1,
                    Agreement::Category => summary.category += 1,
                    Agreement::Location => summary.location += 1,
                    Agreement::None => summary.mismatch += 1,
                }
                match emit {
                    EmitAgreement::Full => summary.emit_full += 1,
                    EmitAgreement::None => summary.emit_mismatch += 1,
                    EmitAgreement::NotAssessed => summary.emit_not_assessed += 1,
                }
                match map {
                    EmitAgreement::Full => summary.map_full += 1,
                    EmitAgreement::None | EmitAgreement::NotAssessed => {
                        summary.map_mismatch += 1;
                    }
                }
            }
            Outcome::HarnessError { .. } => {
                summary.lane_a += 1;
                summary.harness_errors += 1;
            }
            Outcome::Skipped { .. } => summary.skipped += 1,
            Outcome::NotRun { .. } => summary.not_run += 1,
        }
    }
    summary
}

/// Where the command writes its report.
pub fn report_path(workspace: &Path, profile: &str) -> PathBuf {
    workspace
        .join("target/conformance-ts71")
        .join(profile)
        .join("report.json")
}

#[cfg(test)]
#[path = "../tests/unit/ts71/tests.rs"]
mod tests;
