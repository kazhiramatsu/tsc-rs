//! The TypeScript 7.1 suites outside the compiler runner, compared with the
//! profile's reference baselines byte for byte as `baseline.Run` compares
//! them (docs/design/greenfield/slices/ts71-suites/README.md). The
//! `suites-ts71` binary and `scripts/suites_ts71.py` drive [`run`].
//!
//! transpile (`testrunner/transpile_runner.go`): every configuration of a
//! case writes `<name>.js` unless it sets `emitDeclarationOnly` and
//! `<name>.d.ts` when it sets `declaration`. A baseline lists the units,
//! then for each unit the output of `transpile.TranspileModule` (or
//! `TranspileDeclaration`), its source map and, when there are any, the
//! diagnostics as an error baseline.
//!
//! tsoptions and config (`tsoptions/commandlineparser_test.go`,
//! `tsoptions/tsconfigparsing_test.go`): the command-line and tsconfig
//! parsing baselines of the Go tests' tables ([`tsoptions`], [`tsconfig`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tsc_compiler::transpile::{
    transpile_declaration, transpile_module, TranspileError, TranspileOptions,
};
use tsc_diagnostics::compare_diagnostics;
use tsc_harness::upstream_suites::native::NativeProfile;
use tsc_harness::upstream_suites::transpile::{TranspileCase, TranspileConfiguration};

use super::errors_baseline::{self, InputFile};
use super::{declaration_emit_extension, output_extension, panic_text, CASE_STACK_BYTES};

mod go_json;
mod tables;
mod tsc;
mod tsconfig;
mod tsoptions;

/// The suites [`run`] knows.
pub const SUITES: [&str; 7] = [
    "config",
    "transpile",
    "tsbuild",
    "tsbuildWatch",
    "tsc",
    "tscWatch",
    "tsoptions",
];

/// How one produced (or expected) baseline compares with its reference.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuiteOutcome {
    /// Byte for byte equal to the reference.
    Full,
    /// Different from the reference, or a baseline only one side has.
    Mismatch,
    /// The case could not be run (a harness or transpile failure, a panic).
    HarnessError,
}

#[derive(Clone, Debug, Serialize)]
pub struct SuiteResult {
    pub suite: &'static str,
    /// The baseline's name under `baselines/reference/<suite>/`.
    pub baseline: String,
    pub outcome: SuiteOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SuiteRunOptions {
    pub profile: String,
    /// Run only the cases whose path contains this text.
    pub filter: Option<String>,
    /// Write every produced baseline that differs from its reference under
    /// `<dump>/<suite>/`.
    pub dump: Option<PathBuf>,
}

/// Runs every case of every suite and returns one result per baseline, in
/// suite and name order.
pub fn run(workspace: &Path, options: &SuiteRunOptions) -> Result<Vec<SuiteResult>, String> {
    let profile =
        NativeProfile::load(workspace, &options.profile).map_err(|error| error.to_string())?;
    let mut results = run_transpile(&profile, options)?;
    let reference = profile
        .upstream_root()
        .join("tsc/testdata/baselines/reference");
    results.extend(run_rendered(
        "tsoptions",
        &reference.join("tsoptions"),
        tsoptions::cases()
            .into_iter()
            .map(|case| {
                let baseline = case.baseline.clone();
                (
                    baseline,
                    Box::new(move || Ok(tsoptions::render(&case))) as Render,
                )
            })
            .collect(),
        options,
    )?);
    results.extend(run_rendered(
        "config",
        &reference.join("config"),
        tsconfig::cases()
            .into_iter()
            .map(|case| {
                let baseline = case.baseline.clone();
                (
                    baseline,
                    Box::new(move || tsconfig::render(&case)) as Render,
                )
            })
            .collect(),
        options,
    )?);
    let libraries = std::sync::Arc::new(library_file_names(
        &profile.upstream_root().join("tsc/internal/bundled/libs"),
    )?);
    let scenarios = tsc::cases(workspace, &options.profile)?;
    for suite in ["tsc", "tsbuild", "tscWatch", "tsbuildWatch"] {
        let cases = scenarios
            .iter()
            .filter(|scenario| tsc::suite_and_name(scenario).0 == suite)
            .map(|scenario| {
                let name = tsc::suite_and_name(scenario).1.to_owned();
                let scenario = scenario.clone();
                let libraries = std::sync::Arc::clone(&libraries);
                (
                    name,
                    Box::new(move || tsc::render(&scenario, &libraries)) as Render,
                )
            })
            .collect();
        let suite: &'static str = match suite {
            "tsc" => "tsc",
            "tsbuild" => "tsbuild",
            "tscWatch" => "tscWatch",
            _ => "tsbuildWatch",
        };
        results.extend(run_rendered(suite, &reference.join(suite), cases, options)?);
    }
    results
        .sort_by(|left, right| (left.suite, &left.baseline).cmp(&(right.suite, &right.baseline)));
    Ok(results)
}

/// The names of the profile's standard library files.
fn library_file_names(directory: &Path) -> Result<Vec<String>, String> {
    let mut names = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| name.ends_with(".d.ts"))
        .collect::<Vec<_>>();
    names.sort();
    Ok(names)
}

/// A baseline's renderer.
type Render = Box<dyn FnOnce() -> Result<String, String> + Send>;

/// Renders each `(baseline, render)` of a table-driven suite and compares it
/// with its reference.
fn run_rendered(
    suite: &'static str,
    reference_root: &Path,
    cases: Vec<(String, Render)>,
    options: &SuiteRunOptions,
) -> Result<Vec<SuiteResult>, String> {
    let mut results = Vec::new();
    let mut produced = BTreeSet::new();
    for (baseline, render) in cases {
        if options
            .filter
            .as_deref()
            .is_some_and(|filter| !baseline.contains(filter))
        {
            continue;
        }
        produced.insert(baseline.clone());
        let actual = run_on_large_stack(render);
        results.push(compare(
            suite,
            baseline,
            actual,
            reference_root,
            options.dump.as_deref(),
        ));
    }
    if options.filter.is_none() {
        unproduced_references(suite, reference_root, &produced, &mut results)?;
    }
    Ok(results)
}

/// References no test of the pinned commit writes (left from an earlier
/// test): `tsc_test.go`'s color tests name no `adds color when FORCE_COLOR
/// is set` case. They are not compared.
const ORPHAN_REFERENCES: &[(&str, &str)] =
    &[("tsc", "commandLine/adds-color-when-FORCE_COLOR-is-set.js")];

/// Every reference under `root` (by its path relative to it) that no case
/// produced: a full run reports it as a mismatch.
fn unproduced_references(
    suite: &'static str,
    root: &Path,
    produced: &BTreeSet<String>,
    results: &mut Vec<SuiteResult>,
) -> Result<(), String> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .strip_prefix(root)
                .expect("walked under the reference root")
                .to_string_lossy()
                .replace('\\', "/");
            let orphan = ORPHAN_REFERENCES
                .iter()
                .any(|(orphan_suite, orphan)| *orphan_suite == suite && *orphan == name);
            if !produced.contains(&name) && !orphan {
                results.push(SuiteResult {
                    suite,
                    baseline: name,
                    outcome: SuiteOutcome::Mismatch,
                    detail: Some("tsc-rs does not produce this baseline".to_owned()),
                });
            }
        }
    }
    Ok(())
}

/// `TranspileBaselineRunner.RunTests`.
fn run_transpile(
    profile: &NativeProfile,
    options: &SuiteRunOptions,
) -> Result<Vec<SuiteResult>, String> {
    let reference_root = profile.transpile_baselines_root();
    let paths = profile
        .transpile_case_paths()
        .map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    let mut produced = BTreeSet::new();
    for path in &paths {
        if options
            .filter
            .as_deref()
            .is_some_and(|filter| !path.contains(filter))
        {
            continue;
        }
        let case = match profile.transpile_case(path) {
            Ok(case) => case,
            Err(error) => {
                results.push(SuiteResult {
                    suite: "transpile",
                    baseline: path.clone(),
                    outcome: SuiteOutcome::HarnessError,
                    detail: Some(error.to_string()),
                });
                continue;
            }
        };
        for configuration in &case.configurations {
            let options_of = &configuration.compiler_options;
            let mut kinds = Vec::new();
            if options_of.emit_declaration_only != Some(true) {
                kinds.push(false);
            }
            if options_of.declaration == Some(true) {
                kinds.push(true);
            }
            for declaration in kinds {
                let baseline = baseline_name(&case, configuration, declaration);
                produced.insert(baseline.clone());
                let actual = run_on_large_stack({
                    let case = case.clone();
                    let configuration = configuration.clone();
                    move || transpile_baseline(&case, &configuration, declaration)
                });
                results.push(compare(
                    "transpile",
                    baseline,
                    actual,
                    &reference_root,
                    options.dump.as_deref(),
                ));
            }
        }
    }
    // A reference no configuration produced (only for a full run: a filter
    // leaves cases out).
    if options.filter.is_none() {
        unproduced_references("transpile", &reference_root, &produced, &mut results)?;
    }
    Ok(results)
}

/// `runKind`'s baseline name: the configured name with the output (or
/// declaration) extension of the case's extension.
fn baseline_name(
    case: &TranspileCase,
    configuration: &TranspileConfiguration,
    declaration: bool,
) -> String {
    let name = format!("{}{}", configuration.configured_name, case.extension);
    let extension = if declaration {
        declaration_emit_extension(&name)
    } else {
        output_extension(&name, jsx_preserve(configuration)).to_owned()
    };
    format!("{}{extension}", configuration.configured_name)
}

fn jsx_preserve(configuration: &TranspileConfiguration) -> bool {
    configuration.compiler_options.jsx == Some(1)
}

/// tsgo-port: TranspileBaselineRunner.runKind @7.1 (testrunner/transpile_runner.go:103-168)
fn transpile_baseline(
    case: &TranspileCase,
    configuration: &TranspileConfiguration,
    declaration: bool,
) -> Result<String, String> {
    let mut result = String::new();
    for unit in &case.units {
        append_section(&mut result, &unit.name, &unit.content);
    }
    for unit in &case.units {
        let options = TranspileOptions {
            compiler_options: Some(configuration.compiler_options.clone()),
            file_name: Some(unit.name.clone()),
            report_diagnostics: configuration.report_diagnostics,
        };
        let output = if declaration {
            transpile_declaration(&unit.content, &options)
        } else {
            transpile_module(&unit.content, &options)
        }
        .map_err(|error: TranspileError| format!("{}: {error}", unit.name))?;

        let output_file_name = change_extension(
            &unit.name,
            &if declaration {
                declaration_emit_extension(&unit.name)
            } else {
                output_extension(&unit.name, jsx_preserve(configuration)).to_owned()
            },
        );
        append_section(&mut result, &output_file_name, &output.output_text);
        if let Some(map) = output
            .source_map_text
            .as_deref()
            .filter(|map| !map.is_empty())
        {
            append_section(&mut result, &format!("{output_file_name}.map"), map);
        }
        if !output.diagnostics.is_empty() {
            result.push_str("\r\n\r\n//// [Diagnostics reported]\r\n");
            let diagnostic_file_name = output.diagnostics[0].file_name.as_ref().map_or_else(
                || unit.name.clone(),
                |name| name.to_string_lossy().into_owned(),
            );
            let mut diagnostics = output.diagnostics.to_vec();
            diagnostics.sort_by(compare_diagnostics);
            let files = [InputFile {
                name: &diagnostic_file_name,
                content: &unit.content,
            }];
            let errors = errors_baseline::render(&diagnostics, &files, &[], configuration.pretty)
                .unwrap_or_default();
            result.push_str(&errors.replace(&diagnostic_file_name, &unit.name));
            if !result.ends_with('\n') {
                result.push_str("\r\n");
            }
        }
    }
    Ok(result)
}

/// `appendTranspileSection`.
fn append_section(result: &mut String, file_name: &str, content: &str) {
    result.push_str("//// [");
    result.push_str(file_name);
    result.push_str("] ////\r\n");
    result.push_str(content);
    if !content.ends_with('\n') {
        result.push_str("\r\n");
    }
}

/// `tspath.ChangeExtension`: the first of `extensionsToRemove` the path ends
/// with is replaced by `extension`; a path with none stays as it is.
fn change_extension(path: &str, extension: &str) -> String {
    const EXTENSIONS_TO_REMOVE: [&str; 12] = [
        ".d.ts", ".d.mts", ".d.cts", ".mjs", ".mts", ".cjs", ".cts", ".ts", ".js", ".tsx", ".jsx",
        ".json",
    ];
    match EXTENSIONS_TO_REMOVE
        .iter()
        .find(|known| path.ends_with(*known))
    {
        Some(known) => format!("{}{extension}", &path[..path.len() - known.len()]),
        None => path.to_owned(),
    }
}

fn run_on_large_stack(
    operation: impl FnOnce() -> Result<String, String> + Send + 'static,
) -> Result<String, String> {
    std::thread::Builder::new()
        .stack_size(CASE_STACK_BYTES)
        .spawn(move || std::panic::catch_unwind(std::panic::AssertUnwindSafe(operation)))
        .map_err(|error| format!("could not spawn the case thread: {error}"))?
        .join()
        .unwrap_or_else(Err)
        .unwrap_or_else(|panic| Err(format!("panicked: {}", panic_text(&panic))))
}

fn compare(
    suite: &'static str,
    baseline: String,
    actual: Result<String, String>,
    reference_root: &Path,
    dump: Option<&Path>,
) -> SuiteResult {
    let actual = match actual {
        Ok(actual) => actual,
        Err(detail) => {
            return SuiteResult {
                suite,
                baseline,
                outcome: SuiteOutcome::HarnessError,
                detail: Some(detail),
            }
        }
    };
    let (outcome, detail) = match std::fs::read(reference_root.join(&baseline)) {
        Ok(expected) if expected == actual.as_bytes() => (SuiteOutcome::Full, None),
        Ok(_) => (SuiteOutcome::Mismatch, None),
        Err(_) => (
            SuiteOutcome::Mismatch,
            Some("the reference has no such baseline".to_owned()),
        ),
    };
    if outcome != SuiteOutcome::Full {
        if let Some(dump) = dump {
            let path = dump.join(suite).join(&baseline);
            if let Some(directory) = path.parent() {
                let _ = std::fs::create_dir_all(directory);
            }
            let _ = std::fs::write(path, &actual);
        }
    }
    SuiteResult {
        suite,
        baseline,
        outcome,
        detail,
    }
}

/// The counts of a run, per suite.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SuiteSummary {
    pub suite: &'static str,
    pub baselines: usize,
    pub full: usize,
    pub mismatch: usize,
    pub harness_error: usize,
}

pub fn summarize(results: &[SuiteResult]) -> Vec<SuiteSummary> {
    SUITES
        .iter()
        .map(|&suite| {
            let mut summary = SuiteSummary {
                suite,
                ..SuiteSummary::default()
            };
            for result in results.iter().filter(|result| result.suite == suite) {
                summary.baselines += 1;
                match result.outcome {
                    SuiteOutcome::Full => summary.full += 1,
                    SuiteOutcome::Mismatch => summary.mismatch += 1,
                    SuiteOutcome::HarnessError => summary.harness_error += 1,
                }
            }
            summary
        })
        .collect()
}

/// `target/suites-ts71/<profile>/report.json`.
pub fn report_path(workspace: &Path, profile: &str) -> PathBuf {
    workspace
        .join("target/suites-ts71")
        .join(profile)
        .join("report.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_end_with_a_line_break() {
        let mut result = String::new();
        append_section(&mut result, "a.ts", "x;");
        append_section(&mut result, "b.ts", "y;\n");
        assert_eq!(result, "//// [a.ts] ////\r\nx;\r\n//// [b.ts] ////\r\ny;\n");
    }

    #[test]
    fn extensions_change_from_the_known_one() {
        assert_eq!(
            change_extension("node_modules/pkg/index.ts", ".js"),
            "node_modules/pkg/index.js"
        );
        assert_eq!(change_extension("a.tsx", ".js"), "a.js");
        assert_eq!(change_extension("a.d.ts", ".js"), "a.js");
        assert_eq!(change_extension("a.mts", ".d.mts"), "a.d.mts");
        assert_eq!(change_extension("a.vue", ".js"), "a.vue");
    }
}
