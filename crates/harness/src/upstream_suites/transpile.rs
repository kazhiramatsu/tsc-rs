//! The native transpile suite (`tsc/internal/testrunner/transpile_runner.go`):
//! the case files under `tests/cases/transpile`, their units, configurations
//! and options, and the reference baselines the runner writes for them.
//!
//! A case is split into units as the compiler runner splits it, but its
//! configurations vary only by [`TRANSPILE_VARY_BY`] and its options start
//! empty: the transpile runner applies none of the compiler runner's harness
//! defaults and no skip rules.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use tsc_program::{CompilerOptions, ProgramOptions};

use super::compiler::make_units_from_test;
use super::execution::apply_compiler_settings;
use super::native::{
    configurations_varying_by, extract_settings, NativeConfiguration, NativeProfile,
};
use super::{decode_source, error, VIRTUAL_SOURCE_ROOT};
use crate::HarnessResult;

/// `transpileVaryBy`.
pub const TRANSPILE_VARY_BY: [&str; 3] = ["declarationmap", "sourcemap", "inlinesourcemap"];

/// The harness-only directives (`harnessCommandLineOptions`) a transpile case
/// may carry besides compiler options; only `reportDiagnostics` changes what
/// the transpile runner does.
const HARNESS_DIRECTIVES: [&str; 14] = [
    "usecasesensitivefilenames",
    "baselinefile",
    "includebuiltfile",
    "filename",
    "libfiles",
    "noimplicitreferences",
    "currentdirectory",
    "symlink",
    "link",
    "notypesandsymbols",
    "fullemitpaths",
    "reportdiagnostics",
    "capturesuggestions",
    "typescriptversion",
];

/// One `@filename` unit of a transpile case.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TranspileUnit {
    pub name: String,
    pub content: String,
}

/// One configuration of a transpile case.
#[derive(Clone, Debug)]
pub struct TranspileConfiguration {
    /// The case's file name without its extension, plus
    /// `(<configuration>)` when the configuration has a name
    /// (`formatTranspileConfigurationName`).
    pub configured_name: String,
    /// The options `SetOptionsFromTestConfig` sets on empty options.
    pub compiler_options: CompilerOptions,
    /// `HarnessOptions.ReportDiagnostics`.
    pub report_diagnostics: bool,
    /// `options.Pretty`: the diagnostics are rendered as a pretty error
    /// baseline.
    pub pretty: bool,
}

/// One case file: `tests/cases/transpile/<relative_path>`.
#[derive(Clone, Debug)]
pub struct TranspileCase {
    pub relative_path: String,
    /// `tspath.GetAnyExtensionFromPath(fileName, nil, false)`.
    pub extension: String,
    pub units: Vec<TranspileUnit>,
    pub configurations: Vec<TranspileConfiguration>,
}

impl NativeProfile {
    pub fn transpile_cases_root(&self) -> PathBuf {
        self.upstream_root()
            .join("tsc/testdata/tests/cases/transpile")
    }

    pub fn transpile_baselines_root(&self) -> PathBuf {
        self.upstream_root()
            .join("tsc/testdata/baselines/reference/transpile")
    }

    /// Every case file (`transpileBaselineRegex` is `\.[cm]?[tj]sx?$`),
    /// relative to the cases root and sorted.
    pub fn transpile_case_paths(&self) -> HarnessResult<Vec<String>> {
        let root = self.transpile_cases_root();
        let mut paths = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(directory) = stack.pop() {
            let entries = fs::read_dir(&directory)
                .map_err(|source| error(format!("{}: {source}", directory.display())))?;
            for entry in entries {
                let entry = entry.map_err(|source| error(source.to_string()))?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let relative = path
                    .strip_prefix(&root)
                    .expect("walked under the cases root")
                    .to_string_lossy()
                    .replace('\\', "/");
                if is_transpile_case_name(&relative) {
                    paths.push(relative);
                }
            }
        }
        paths.sort();
        Ok(paths)
    }

    /// The units and configurations of one case (`runTest`).
    pub fn transpile_case(&self, relative_path: &str) -> HarnessResult<TranspileCase> {
        let path = self.transpile_cases_root().join(relative_path);
        let raw =
            fs::read(&path).map_err(|source| error(format!("{}: {source}", path.display())))?;
        let content = decode_source(&raw).1;
        let settings = extract_settings(&content);
        let mut configurations = configurations_varying_by(&settings, &TRANSPILE_VARY_BY)?;
        if configurations.is_empty() {
            configurations.push(NativeConfiguration {
                name: String::new(),
                settings: settings.clone(),
            });
        }
        let base_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
        let extension = any_extension(base_name).to_owned();
        let just_name = &base_name[..base_name.len() - extension.len()];
        let (units, _) = make_units_from_test(&content, base_name)?;
        let units = units
            .into_iter()
            .map(|unit| TranspileUnit {
                name: unit.name,
                content: unit.content.unwrap_or_default(),
            })
            .collect();
        let configurations = configurations
            .into_iter()
            .map(|configuration| {
                let configured_name = if configuration.name.is_empty() {
                    just_name.to_owned()
                } else {
                    format!(
                        "{just_name}({})",
                        format_configuration_name(&configuration.name)
                    )
                };
                let (compiler_options, report_diagnostics) =
                    transpile_options(&configuration.settings)?;
                Ok(TranspileConfiguration {
                    configured_name,
                    compiler_options,
                    report_diagnostics,
                    pretty: is_true(&configuration.settings, "pretty"),
                })
            })
            .collect::<HarnessResult<Vec<_>>>()?;
        Ok(TranspileCase {
            relative_path: relative_path.to_owned(),
            extension,
            units,
            configurations,
        })
    }
}

/// `\.[cm]?[tj]sx?$`.
fn is_transpile_case_name(name: &str) -> bool {
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    let extension = extension
        .strip_prefix(['c', 'm'])
        .filter(|rest| !rest.is_empty())
        .unwrap_or(extension);
    matches!(extension, "ts" | "tsx" | "js" | "jsx")
}

/// `GetAnyExtensionFromPath(path, nil, false)`: the text from the base name's
/// last `.` (empty when there is none).
fn any_extension(base_name: &str) -> &str {
    base_name.rfind('.').map_or("", |dot| &base_name[dot..])
}

/// `formatTranspileConfigurationName`.
fn format_configuration_name(name: &str) -> String {
    name.replace("declarationmap=", "declarationMap=")
        .replace("inlinesourcemap=", "inlineSourceMap=")
        .replace("sourcemap=", "sourceMap=")
}

/// `SetOptionsFromTestConfig(t, config, &core.CompilerOptions{}, harnessOptions,
/// srcFolder, false)`: compiler options on empty options, and the
/// `reportDiagnostics` harness option. The Program-level options the harness
/// projection keeps apart (`noLib`, `types`, …) are forced or cleared by
/// transpilation, so they are dropped here.
fn transpile_options(
    settings: &BTreeMap<String, String>,
) -> HarnessResult<(CompilerOptions, bool)> {
    let mut compiler_options = CompilerOptions::default();
    let mut program_options = ProgramOptions::default();
    let report_diagnostics = is_true(settings, "reportdiagnostics");
    apply_compiler_settings(
        &mut compiler_options,
        &mut program_options,
        VIRTUAL_SOURCE_ROOT,
        settings
            .iter()
            .filter(|(name, _)| !HARNESS_DIRECTIVES.contains(&name.as_str()))
            .map(|(name, value)| (name.as_str(), value.as_str())),
        false,
    )?;
    Ok((compiler_options, report_diagnostics))
}

/// A boolean directive's value (`getOptionValue` accepts `true` in any case).
fn is_true(settings: &BTreeMap<String, String>, name: &str) -> bool {
    settings
        .get(name)
        .is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_names_follow_the_runner_regex() {
        for name in [
            "a.ts",
            "a.tsx",
            "a.mts",
            "a.cts",
            "a.js",
            "a.mjsx",
            "dir/a.cjs",
        ] {
            assert!(is_transpile_case_name(name), "{name}");
        }
        for name in ["a.d", "a.json", "a.mt", "a", "a.m"] {
            assert!(!is_transpile_case_name(name), "{name}");
        }
    }

    #[test]
    fn configuration_names_use_the_option_spelling() {
        assert_eq!(
            format_configuration_name("declarationmap=true"),
            "declarationMap=true"
        );
        assert_eq!(
            format_configuration_name("inlinesourcemap=false,sourcemap=true"),
            "inlineSourceMap=false,sourceMap=true"
        );
    }
}
