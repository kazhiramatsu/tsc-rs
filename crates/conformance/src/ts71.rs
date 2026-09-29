//! Conformance against a native (TypeScript 7.x) profile's reference
//! baselines; see `docs/design/greenfield/slices/conformance-ts71/README.md`.
//!
//! Every case is expanded with the native runner's own rules
//! (`tsc_harness::upstream_suites::native`). A configuration the native runner
//! executes, and that uses no option TypeScript 6.0 deprecated, is lane A:
//! tsc-rs checks it through the Program path and the diagnostics the native
//! runner collects are compared with the profile's `.errors.txt` (an absent
//! file means none). Every other configuration is lane B: tsc-rs keeps the
//! 6.0.3 behavior for the deprecated options, and those cases stay with the
//! 6.0.3 conformance goldens for now.

mod errors_baseline;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use errors_baseline::remove_test_path_prefixes;
use serde::Serialize;
use tsc_compiler::{NativeHarnessCollection, ProgramSession};
use tsc_diagnostics::{Diagnostic, PositionIndex};
use tsc_harness::upstream_suites::execution::{
    load_native_compiler_program, native_compiler_fixture, native_compiler_plan,
    CompilerExecutionPlan, CompilerRootSelection,
};
use tsc_harness::upstream_suites::native::{
    expand_case, NativeCase, NativeConfiguration, NativeProfile, NativeSkip, NativeSuite,
};
use tsc_program::ProgramLoadLimits;

/// Options TypeScript 6.0 deprecated and 7.x removed; a configuration using
/// any of them is lane B. Checked on the effective directive values.
fn deprecated_option(configuration: &NativeConfiguration) -> Option<&'static str> {
    let value = |name: &str| {
        configuration
            .settings
            .get(name)
            .map(|value| value.trim().to_ascii_lowercase())
    };
    let set = |name: &str| value(name).is_some_and(|value| !value.is_empty());
    if matches!(value("target").as_deref(), Some("es3" | "es5")) {
        return Some("target=es5");
    }
    if matches!(
        value("module").as_deref(),
        Some("none" | "amd" | "umd" | "system")
    ) {
        return Some("module");
    }
    if matches!(
        value("moduleresolution").as_deref(),
        Some("node" | "node10" | "classic")
    ) {
        return Some("moduleResolution");
    }
    for (name, reason) in [
        ("outfile", "outFile"),
        ("out", "out"),
        ("baseurl", "baseUrl"),
        ("downleveliteration", "downlevelIteration"),
        ("charset", "charset"),
        ("importsnotusedasvalues", "importsNotUsedAsValues"),
        ("keyofstringsonly", "keyofStringsOnly"),
        ("noimplicitusestrict", "noImplicitUseStrict"),
        ("nostrictgenericchecks", "noStrictGenericChecks"),
        ("preservevalueimports", "preserveValueImports"),
        (
            "suppressexcesspropertyerrors",
            "suppressExcessPropertyErrors",
        ),
        (
            "suppressimplicitanyindexerrors",
            "suppressImplicitAnyIndexErrors",
        ),
    ] {
        if set(name) {
            return Some(reason);
        }
    }
    for (name, reason) in [
        ("esmoduleinterop", "esModuleInterop=false"),
        (
            "allowsyntheticdefaultimports",
            "allowSyntheticDefaultImports=false",
        ),
        ("alwaysstrict", "alwaysStrict=false"),
    ] {
        if value(name).as_deref() == Some("false") {
            return Some(reason);
        }
    }
    None
}

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
    },
    /// Lane A, but tsc-rs could not build or check the Program.
    HarnessError { reason: String },
    /// Lane B: an option TypeScript 6.0 deprecated.
    Deprecated { option: String },
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
    /// Write the rendered error baseline of every lane-A configuration that
    /// differs from its reference to `<dump>/<suite>/<stem>.errors.txt` (an
    /// empty file when tsc-rs reports nothing), for diffing.
    pub dump: Option<PathBuf>,
}

/// A stack overflow aborts the process, so the threads that run cases reserve
/// far more than the CLI's checker threads: the corpus includes deeply nested
/// stress cases. The reservation is virtual.
const CASE_STACK_BYTES: usize = 256 << 20;

fn limits() -> ProgramLoadLimits {
    ProgramLoadLimits::new(512, 8_192, 64, 64 * 1024 * 1024, 256 * 1024 * 1024)
}

/// The fixture files in the order the native runner passes them to the
/// error baseline (`tsConfigFiles`, `toBeCompiled`, `otherFiles`), each by
/// its normalized absolute unit path with its text.
fn baseline_input_files(plan: &CompilerExecutionPlan) -> Vec<(String, String)> {
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
    config
        .iter()
        .chain(roots.iter())
        .chain(others.iter())
        .filter_map(|id| plan.fixture.units.iter().find(|unit| unit.id == *id))
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
        .collect()
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

fn run_lane_a(
    workspace: &Path,
    profile: &NativeProfile,
    suite: NativeSuite,
    (configuration, stem): (&NativeConfiguration, &str),
    plan: &CompilerExecutionPlan,
    dump: Option<&Path>,
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
    let prepared = match load_native_compiler_program(workspace, plan, limits(), &test_library) {
        Ok(prepared) => prepared,
        Err(error) => {
            return Outcome::HarnessError {
                reason: format!("load: {error}"),
            }
        }
    };
    let outcome = match ProgramSession::new(prepared).run_for_native_harness(collection) {
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
    if !pretty && (agreement == Agreement::Text || dump.is_some()) {
        let inputs: Vec<_> = files
            .iter()
            .map(|(name, content)| errors_baseline::InputFile { name, content })
            .collect();
        let rendered = errors_baseline::render(diagnostics, &inputs);
        if rendered == expected_text {
            agreement = Agreement::Full;
        } else if let Some(directory) = dump {
            let path = directory
                .join(suite.name())
                .join(format!("{stem}.errors.txt"));
            let written = std::fs::create_dir_all(directory.join(suite.name()))
                .and_then(|()| std::fs::write(&path, rendered.unwrap_or_default()));
            if let Err(error) = written {
                eprintln!("{}: {error}", path.display());
            }
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
    Outcome::Compared {
        agreement,
        expected: expected.len(),
        actual: actual.len(),
        missing,
        unexpected,
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
pub fn run_worker(workspace: &Path, profile: &str, cases_file: &Path) -> Result<(), String> {
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
fn run_case(
    workspace: &Path,
    profile: &NativeProfile,
    case: &NativeCase,
    leave_out: &[&str],
    dump: Option<&Path>,
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
        let outcome = if let Some(option) = deprecated_option(&configuration) {
            Outcome::Deprecated {
                option: option.to_owned(),
            }
        } else if let Some(skip) = skip {
            match skip {
                NativeSkip::Unsupported(rule) => Outcome::Deprecated {
                    option: rule.to_owned(),
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
                                    case.suite,
                                    (&configuration, &stem),
                                    &plan,
                                    dump,
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
    pub harness_errors: usize,
    pub deprecated: usize,
    pub not_run: usize,
}

pub fn summarize(results: &[ConfigurationResult]) -> Summary {
    let mut summary = Summary {
        configurations: results.len(),
        ..Summary::default()
    };
    for result in results {
        match &result.outcome {
            Outcome::Compared { agreement, .. } => {
                summary.lane_a += 1;
                match agreement {
                    Agreement::Full => summary.full += 1,
                    Agreement::Text => summary.text += 1,
                    Agreement::Category => summary.category += 1,
                    Agreement::Location => summary.location += 1,
                    Agreement::None => summary.mismatch += 1,
                }
            }
            Outcome::HarnessError { .. } => {
                summary.lane_a += 1;
                summary.harness_errors += 1;
            }
            Outcome::Deprecated { .. } => summary.deprecated += 1,
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
