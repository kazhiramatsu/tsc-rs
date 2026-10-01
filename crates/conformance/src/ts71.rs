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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use emit_baseline::{Emission, MapOptions};
use errors_baseline::remove_test_path_prefixes;
use serde::Serialize;
use sha2::Digest as _;
use tsc_compiler::{
    CheckerBudget, MemoryOutputSink, NativeHarnessCollection, PreparedProgramMode, ProgramSession,
};
use tsc_diagnostics::{Diagnostic, PositionIndex};
use tsc_harness::upstream_suites::execution::{
    load_native_compiler_program, native_compiler_fixture, native_compiler_plan,
    CompilerExecutionPlan, CompilerRootSelection,
};
use tsc_harness::upstream_suites::native::{
    expand_case, NativeCase, NativeConfiguration, NativeProfile, NativeSkip, NativeSuite,
};
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
    /// requires. Not assessed for `@pretty` configurations.
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
            let detail = if expected.contains("//// [DtsFileErrors]") {
                "reference has DtsFileErrors".to_owned()
            } else if expected.contains("\n!!!! File ") {
                "reference has the noCheck comparison".to_owned()
            } else {
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
                format!("differs at line {line}")
            };
            (EmitAgreement::None, Some(detail))
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
        /// it reports nothing); `None` for a `@pretty` configuration. Two runs
        /// with the same digest produced the same diagnostics in the same
        /// order, whatever their tier.
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
                let name = unit.name.replace('\\', "/");
                let path = if name.starts_with('/') {
                    name
                } else {
                    format!("{}/{}", plan.current_directory.trim_end_matches('/'), name)
                };
                let content = unit.content.as_deref().unwrap_or_default().to_owned();
                (normalize(&path), content)
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

/// Emit the configuration as the native harness's second Program does
/// (`compileFilesWithHost`: `program.Emit` after the diagnostics Program),
/// collecting the written files, with the map options of the Program's
/// effective compiler options (the directives and the config file). Nothing
/// for a Program that cannot emit (`noEmit`).
fn emit_outputs(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    test_library: &Path,
    standard_library: &Path,
    budget: CheckerBudget,
) -> Result<(Emission, MapOptions), String> {
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
    if prepared.mode() != PreparedProgramMode::Emit {
        return Ok((Emission::default(), map_options));
    }
    let mut sink = MemoryOutputSink::new();
    ProgramSession::new(prepared)
        .with_checker_budget(budget)
        .emit(&mut sink)
        .map_err(|error| format!("emit: {error}"))?;
    Ok((Emission::from_writes(sink.writes()), map_options))
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

fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    format!("/{}", parts.join("/"))
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
        output_path_check: !flag("suppressoutputpathcheck"),
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
    let texts: BTreeMap<String, String> = files.iter().cloned().collect();
    let mut indexes = BTreeMap::new();
    let diagnostics = outcome.native_harness_diagnostics();
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
    let mut rendered_sha256 = None;
    if !pretty {
        let inputs: Vec<_> = files
            .iter()
            .map(|(name, content)| errors_baseline::InputFile { name, content })
            .collect();
        let rendered = errors_baseline::render(diagnostics, &inputs);
        rendered_sha256 = Some(format!(
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

    // The emit, as the native harness's second Program produces it, against
    // the `.js` and `.js.map` references.
    let (emission, map_options, emit_error) =
        match emit_outputs(workspace, plan, &test_library, &standard_library, budget()) {
            Ok((emission, map_options)) => (emission, map_options, None),
            Err(error) => (Emission::default(), MapOptions::default(), Some(error)),
        };
    let full_emit_paths = flag("fullemitpaths");
    let header = format!("tests/cases/{}/{case_path}", suite.name());
    let sources = js_baseline_sources(plan);
    let source_inputs: Vec<_> = sources
        .iter()
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    let rendered_js = emit_baseline::render_js(&header, &source_inputs, &emission, full_emit_paths);
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
    let all_inputs: Vec<_> = files
        .iter()
        .map(|(name, content)| errors_baseline::InputFile { name, content })
        .collect();
    let rendered_map = emit_baseline::render_js_map(
        map_options,
        !diagnostics.is_empty(),
        &emission,
        &all_inputs,
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
