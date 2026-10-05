//! The native (TypeScript 7.x, Go) compiler-test harness rules at a vendored
//! commit: case enumeration, directive extraction, configuration expansion and
//! naming, and the per-configuration skip rules.
//!
//! Ported from `tsc/internal/testrunner/{compiler_runner,test_case_parser}.go`
//! and `tsc/internal/testutil/harnessutil/harnessutil.go` at the commit named by
//! the profile's `manifest.json` (written by `scripts/vendor_typescript_native.py`).
//! Units are split by `compiler::make_units_from_test` as Go's
//! `ParseTestFilesAndSymlinksWithOptions` splits them (a unit without content
//! lines is an empty file).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::HarnessResult;

use super::{compiler, decode_source, error};

pub const NATIVE_VENDOR_ROOT: &str = "vendor/typescript-native";
pub const MAX_VARIATIONS: usize = 25;

/// `getCompilerVaryByMap` (compiler_runner.go): every option that is not
/// command-line-only, is boolean or enum, and has an `Affects*` flag, plus
/// `noEmit` and `isolatedModules`; lower-cased.
pub const VARY_BY: [&str; 72] = [
    "allowarbitraryextensions",
    "allowimportingtsextensions",
    "allowjs",
    "allowsyntheticdefaultimports",
    "allowumdglobalaccess",
    "allowunreachablecode",
    "allowunusedlabels",
    "alwaysstrict",
    "assumechangesonlyaffectdirectdependencies",
    "checkjs",
    "composite",
    "declaration",
    "declarationmap",
    "deduplicatepackages",
    "disablesizelimit",
    "downleveliteration",
    "emitbom",
    "emitdeclarationonly",
    "emitdecoratormetadata",
    "erasablesyntaxonly",
    "esmoduleinterop",
    "exactoptionalpropertytypes",
    "experimentaldecorators",
    "forceconsistentcasinginfilenames",
    "importhelpers",
    "inlinesourcemap",
    "inlinesources",
    "isolateddeclarations",
    "isolatedmodules",
    "jsx",
    "libreplacement",
    "module",
    "moduledetection",
    "moduleresolution",
    "newline",
    "noemit",
    "noemithelpers",
    "noemitonerror",
    "noerrortruncation",
    "nofallthroughcasesinswitch",
    "noimplicitany",
    "noimplicitoverride",
    "noimplicitreturns",
    "noimplicitthis",
    "nolib",
    "nopropertyaccessfromindexsignature",
    "noresolve",
    "nouncheckedindexedaccess",
    "nouncheckedsideeffectimports",
    "nounusedlocals",
    "nounusedparameters",
    "preserveconstenums",
    "removecomments",
    "resolvejsonmodule",
    "resolvepackagejsonexports",
    "resolvepackagejsonimports",
    "rewriterelativeimportextensions",
    "skipdefaultlibcheck",
    "skiplibcheck",
    "sourcemap",
    "stabletypeordering",
    "strict",
    "strictbindcallapply",
    "strictbuiltiniteratorreturn",
    "strictfunctiontypes",
    "strictnullchecks",
    "strictpropertyinitialization",
    "stripinternal",
    "target",
    "usedefineforclassfields",
    "useunknownincatchvariables",
    "verbatimmodulesyntax",
];

/// The enum options that can vary, with their `enummaps.go` keys in map order
/// and the normalized value each key parses to.
const ENUM_OPTIONS: [(&str, &[(&str, i32)]); 6] = [
    (
        "target",
        &[
            ("es5", 1),
            ("es6", 2),
            ("es2015", 2),
            ("es2016", 3),
            ("es2017", 4),
            ("es2018", 5),
            ("es2019", 6),
            ("es2020", 7),
            ("es2021", 8),
            ("es2022", 9),
            ("es2023", 10),
            ("es2024", 11),
            ("es2025", 12),
            ("es2026", 13),
            ("esnext", 99),
        ],
    ),
    (
        "module",
        &[
            ("commonjs", 1),
            ("amd", 2),
            ("system", 4),
            ("umd", 3),
            ("es6", 5),
            ("es2015", 5),
            ("es2020", 6),
            ("es2022", 7),
            ("esnext", 99),
            ("node16", 100),
            ("node18", 101),
            ("node20", 102),
            ("nodenext", 199),
            ("preserve", 200),
        ],
    ),
    (
        "moduleresolution",
        &[
            ("node16", 3),
            ("nodenext", 99),
            ("bundler", 100),
            ("classic", 1),
            ("node", 2),
            ("node10", 2),
        ],
    ),
    (
        "moduledetection",
        &[("auto", 1), ("legacy", 2), ("force", 3)],
    ),
    (
        "jsx",
        &[
            ("preserve", 1),
            ("react-native", 3),
            ("react-jsx", 4),
            ("react-jsxdev", 5),
            ("react", 2),
        ],
    ),
    ("newline", &[("crlf", 0), ("lf", 1)]),
];

/// `skippedTests` (compiler_runner.go): API samples that needed the built
/// `typescript.d.ts`, and tests whose options were removed and no longer parse.
pub const SKIPPED_TESTS: [&str; 42] = [
    "APILibCheck.ts",
    "APISample_Watch.ts",
    "APISample_WatchWithDefaults.ts",
    "APISample_WatchWithOwnWatchHost.ts",
    "APISample_compile.ts",
    "APISample_jsdoc.ts",
    "APISample_linter.ts",
    "APISample_parseConfig.ts",
    "APISample_transform.ts",
    "APISample_watcher.ts",
    "preserveUnusedImports.ts",
    "noCrashWithVerbatimModuleSyntaxAndImportsNotUsedAsValues.ts",
    "verbatimModuleSyntaxCompat.ts",
    "verbatimModuleSyntaxCompat2.ts",
    "verbatimModuleSyntaxCompat3.ts",
    "verbatimModuleSyntaxCompat4.ts",
    "preserveValueImports.ts",
    "preserveValueImports_importsNotUsedAsValues.ts",
    "preserveValueImports_errors.ts",
    "preserveValueImports_mixedImports.ts",
    "preserveValueImports_module.ts",
    "importsNotUsedAsValues_error.ts",
    "alwaysStrictNoImplicitUseStrict.ts",
    "nonPrimitiveIndexingWithForInSupressError.ts",
    "parameterInitializerBeforeDestructuringEmit.ts",
    "mappedTypeUnionConstraintInferences.ts",
    "lateBoundConstraintTypeChecksCorrectly.ts",
    "keyofDoesntContainSymbols.ts",
    "noStrictGenericChecks.ts",
    "noImplicitUseStrict_umd.ts",
    "noImplicitUseStrict_system.ts",
    "noImplicitUseStrict_es6.ts",
    "noImplicitUseStrict_commonjs.ts",
    "noImplicitAnyIndexingSuppressed.ts",
    "excessPropertyErrorsSuppressed.ts",
    "moduleNoneDynamicImport.ts",
    "moduleNoneErrors.ts",
    "noErrorUsingImportExportModuleAugmentationInDeclarationFile1.ts",
    "noErrorUsingImportExportModuleAugmentationInDeclarationFile2.ts",
    "noErrorUsingImportExportModuleAugmentationInDeclarationFile3.ts",
    "requireOfJsonFileWithModuleEmitNone.ts",
    "requireOfJsonFileWithModuleNodeResolutionEmitNone.ts",
];

/// Directive names `SetOptionsFromTestConfig` accepts: the 7.x option
/// declarations plus the harness additions and the harness-only options.
/// `typescriptVersion` is accepted and ignored. Any other name is fatal.
const KNOWN_DIRECTIVES: [&str; 141] = [
    "all",
    "allowarbitraryextensions",
    "allowimportingtsextensions",
    "allowjs",
    "allownontsextensions",
    "allowsyntheticdefaultimports",
    "allowumdglobalaccess",
    "allowunreachablecode",
    "allowunusedlabels",
    "alwaysstrict",
    "assumechangesonlyaffectdirectdependencies",
    "baselinefile",
    "baseurl",
    "capturesuggestions",
    "checkers",
    "checkjs",
    "composite",
    "currentdirectory",
    "customconditions",
    "declaration",
    "declarationdir",
    "declarationmap",
    "deduplicatepackages",
    "diagnostics",
    "disablereferencedprojectload",
    "disablesizelimit",
    "disablesolutionsearching",
    "disablesourceofprojectreferenceredirect",
    "downleveliteration",
    "emitbom",
    "emitdeclarationonly",
    "emitdecoratormetadata",
    "erasablesyntaxonly",
    "esmoduleinterop",
    "exactoptionalpropertytypes",
    "experimentaldecorators",
    "explainfiles",
    "extendeddiagnostics",
    "filename",
    "forceconsistentcasinginfilenames",
    "fullemitpaths",
    "generatecpuprofile",
    "generatetrace",
    "help",
    "ignoreconfig",
    "ignoredeprecations",
    "importhelpers",
    "includebuiltfile",
    "incremental",
    "init",
    "inlinesourcemap",
    "inlinesources",
    "isolateddeclarations",
    "isolatedmodules",
    "jsx",
    "jsxfactory",
    "jsxfragmentfactory",
    "jsximportsource",
    "lib",
    "libfiles",
    "libreplacement",
    "link",
    "listemittedfiles",
    "listfiles",
    "listfilesonly",
    "locale",
    "maproot",
    "maxnodemodulejsdepth",
    "module",
    "moduledetection",
    "moduleresolution",
    "modulesuffixes",
    "newline",
    "nocheck",
    "noemit",
    "noemithelpers",
    "noemitonerror",
    "noerrortruncation",
    "nofallthroughcasesinswitch",
    "noimplicitany",
    "noimplicitoverride",
    "noimplicitreferences",
    "noimplicitreturns",
    "noimplicitthis",
    "nolib",
    "nopropertyaccessfromindexsignature",
    "noresolve",
    "notypesandsymbols",
    "nouncheckedindexedaccess",
    "nouncheckedsideeffectimports",
    "nounusedlocals",
    "nounusedparameters",
    "outdir",
    "outfile",
    "paths",
    "plugins",
    "pprofdir",
    "preserveconstenums",
    "preservesymlinks",
    "preservewatchoutput",
    "pretty",
    "project",
    "quiet",
    "reactnamespace",
    "removecomments",
    "reportdiagnostics",
    "resolvejsonmodule",
    "resolvepackagejsonexports",
    "resolvepackagejsonimports",
    "rewriterelativeimportextensions",
    "rootdir",
    "rootdirs",
    "runexternalcode",
    "showconfig",
    "singlethreaded",
    "skipdefaultlibcheck",
    "skiplibcheck",
    "sourcemap",
    "sourceroot",
    "stabletypeordering",
    "strict",
    "strictbindcallapply",
    "strictbuiltiniteratorreturn",
    "strictfunctiontypes",
    "strictnullchecks",
    "strictpropertyinitialization",
    "stripinternal",
    "suppressoutputpathcheck",
    "symlink",
    "target",
    "traceresolution",
    "tsbuildinfofile",
    "typeroots",
    "types",
    "typescriptversion",
    "usecasesensitivefilenames",
    "usedefineforclassfields",
    "useunknownincatchvariables",
    "verbatimmodulesyntax",
    "version",
    "watch",
];

/// A vendored native profile: its manifest and upstream tree.
#[derive(Clone, Debug)]
pub struct NativeProfile {
    workspace: PathBuf,
    pub manifest: NativeManifest,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NativeManifest {
    pub schema: u32,
    pub repository: String,
    pub commit: String,
    pub profile: String,
    pub vendored_root: String,
    pub sets: Vec<NativeSet>,
    pub baseline_names: NativeBaselineNames,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NativeSet {
    pub path: String,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub git_tree_sha1: Option<String>,
    /// The blob id of a set that vendors one file.
    #[serde(default)]
    pub git_blob_sha1: Option<String>,
    pub blob_inventory_sha256: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NativeBaselineNames {
    pub path: String,
    pub entries: u64,
    pub sha256: String,
}

/// The two compiler-baseline suites.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum NativeSuite {
    Compiler,
    Conformance,
}

impl NativeSuite {
    pub const ALL: [Self; 2] = [Self::Compiler, Self::Conformance];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Compiler => "compiler",
            Self::Conformance => "conformance",
        }
    }
}

/// One test file: `tsc/testdata/tests/cases/<suite>/<relative_path>`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct NativeCase {
    pub suite: NativeSuite,
    pub relative_path: String,
}

impl NativeCase {
    pub fn file_name(&self) -> &str {
        self.relative_path
            .rsplit('/')
            .next()
            .expect("split yields a last element")
    }
}

impl NativeProfile {
    pub fn load(workspace: &Path, profile: &str) -> HarnessResult<Self> {
        let path = workspace
            .join(NATIVE_VENDOR_ROOT)
            .join(profile)
            .join("manifest.json");
        let text = fs::read_to_string(&path)
            .map_err(|source| error(format!("{}: {source}", path.display())))?;
        let manifest: NativeManifest = serde_json::from_str(&text)
            .map_err(|source| error(format!("{}: {source}", path.display())))?;
        Ok(Self {
            workspace: workspace.to_path_buf(),
            manifest,
        })
    }

    pub fn upstream_root(&self) -> PathBuf {
        self.workspace.join(&self.manifest.vendored_root)
    }

    pub fn cases_root(&self, suite: NativeSuite) -> PathBuf {
        self.upstream_root()
            .join("tsc/testdata/tests/cases")
            .join(suite.name())
    }

    /// `tests/lib`, which the runner mounts at `/.lib`.
    pub fn test_library_root(&self) -> PathBuf {
        self.upstream_root().join("tsc/testdata/tests/lib")
    }

    /// The standard libraries the native compiler embeds
    /// (`tsc/internal/bundled/libs`): the profile's `lib.*.d.ts` catalog.
    pub fn bundled_libraries_root(&self) -> PathBuf {
        self.upstream_root().join("tsc/internal/bundled/libs")
    }

    /// The profile's diagnostic message catalog
    /// (`tsc/internal/diagnostics/diagnosticMessages.json`), the source of
    /// `crates/diagnostics/src/gen.rs`.
    pub fn diagnostic_messages_path(&self) -> PathBuf {
        self.upstream_root()
            .join("tsc/internal/diagnostics/diagnosticMessages.json")
    }

    /// Every `.ts`/`.tsx` file of both suites, as `TestLocal` enumerates them
    /// (`compilerBaselineRegex` is `\.tsx?$`), sorted by suite and path.
    pub fn cases(&self) -> HarnessResult<Vec<NativeCase>> {
        let mut cases = Vec::new();
        for suite in NativeSuite::ALL {
            let root = self.cases_root(suite);
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
                    let name = path.to_string_lossy();
                    if !(name.ends_with(".ts") || name.ends_with(".tsx")) {
                        continue;
                    }
                    let relative = path
                        .strip_prefix(&root)
                        .expect("walked under the suite root")
                        .to_string_lossy()
                        .replace('\\', "/");
                    cases.push(NativeCase {
                        suite,
                        relative_path: relative,
                    });
                }
            }
        }
        cases.sort();
        Ok(cases)
    }

    pub fn read_case(&self, case: &NativeCase) -> HarnessResult<String> {
        let path = self.cases_root(case.suite).join(&case.relative_path);
        let raw =
            fs::read(&path).map_err(|source| error(format!("{}: {source}", path.display())))?;
        Ok(decode_source(&raw).1)
    }

    /// The upstream-relative names of every compiler and conformance
    /// reference baseline, of every kind.
    pub fn baseline_names(&self) -> HarnessResult<Vec<String>> {
        let path = self
            .workspace
            .join(NATIVE_VENDOR_ROOT)
            .join(&self.manifest.profile)
            .join(&self.manifest.baseline_names.path);
        let text = fs::read_to_string(&path)
            .map_err(|source| error(format!("{}: {source}", path.display())))?;
        Ok(text.lines().map(str::to_owned).collect())
    }

    pub fn errors_baseline_path(&self, suite: NativeSuite, stem: &str) -> PathBuf {
        self.baseline_path(suite, &format!("{stem}.errors.txt"))
    }

    /// The JavaScript emit baseline (`DoJSEmitBaseline`): the inputs and the
    /// emitted JavaScript and declaration files of one configuration.
    pub fn js_baseline_path(&self, suite: NativeSuite, stem: &str) -> PathBuf {
        self.baseline_path(suite, &format!("{stem}.js"))
    }

    /// The raw source maps of one configuration (`DoSourcemapBaseline`).
    pub fn js_map_baseline_path(&self, suite: NativeSuite, stem: &str) -> PathBuf {
        self.baseline_path(suite, &format!("{stem}.js.map"))
    }

    /// The source-map record of one configuration (`DoSourcemapRecordBaseline`).
    pub fn sourcemap_baseline_path(&self, suite: NativeSuite, stem: &str) -> PathBuf {
        self.baseline_path(suite, &format!("{stem}.sourcemap.txt"))
    }

    fn baseline_path(&self, suite: NativeSuite, file_name: &str) -> PathBuf {
        self.upstream_root()
            .join("tsc/testdata/baselines/reference")
            .join(suite.name())
            .join(file_name)
    }
}

/// `extractCompilerSettings` (test_case_parser.go): every match of
/// `(?m)^//\s*@(\w+)\s*:\s*([^\r\n]*)` in the whole file, left to right and
/// without overlap (the leading `\s*` may cross a line break). Names are
/// lower-cased, the last occurrence wins, and the value is trimmed and loses
/// one trailing `;`.
pub fn extract_settings(content: &str) -> BTreeMap<String, String> {
    let mut settings = BTreeMap::new();
    let bytes = content.as_bytes();
    let mut line_start = 0;
    let mut resume = 0;
    loop {
        if line_start >= resume {
            if let Some((name, value, end)) = match_directive(bytes, line_start) {
                settings.insert(name, value);
                resume = end;
            }
        }
        match content[line_start..].find('\n') {
            Some(offset) => line_start += offset + 1,
            None => break,
        }
    }
    settings
}

/// RE2's `\s` in Go regexps: `[\t\n\f\r ]`.
fn is_re2_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

fn match_directive(bytes: &[u8], start: usize) -> Option<(String, String, usize)> {
    let mut at = start;
    if bytes.get(at..at + 2)? != b"//" {
        return None;
    }
    at += 2;
    while bytes.get(at).is_some_and(|&byte| is_re2_space(byte)) {
        at += 1;
    }
    if bytes.get(at) != Some(&b'@') {
        return None;
    }
    at += 1;
    let name_start = at;
    while bytes
        .get(at)
        .is_some_and(|&byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        at += 1;
    }
    if at == name_start {
        return None;
    }
    let name = std::str::from_utf8(&bytes[name_start..at])
        .expect("ASCII word characters")
        .to_ascii_lowercase();
    while bytes.get(at).is_some_and(|&byte| is_re2_space(byte)) {
        at += 1;
    }
    if bytes.get(at) != Some(&b':') {
        return None;
    }
    at += 1;
    while bytes.get(at).is_some_and(|&byte| is_re2_space(byte)) {
        at += 1;
    }
    let value_start = at;
    while bytes
        .get(at)
        .is_some_and(|&byte| byte != b'\r' && byte != b'\n')
    {
        at += 1;
    }
    let value = String::from_utf8_lossy(&bytes[value_start..at]);
    let value = value.trim();
    let value = value.strip_suffix(';').unwrap_or(value);
    Some((name, value.to_owned(), at))
}

/// One configuration of a case: its name (empty when nothing varies) and
/// the settings it applies, keyed by lower-cased directive name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeConfiguration {
    pub name: String,
    pub settings: BTreeMap<String, String>,
}

/// `GetFileBasedTestConfigurations` (harnessutil.go). No configuration at all
/// means the case runs once, unnamed, with no settings.
pub fn configurations(
    settings: &BTreeMap<String, String>,
) -> HarnessResult<Vec<NativeConfiguration>> {
    let mut dimensions: Vec<(String, Vec<String>)> = Vec::new();
    let mut variation_count = 1usize;
    let mut fixed = BTreeMap::new();
    for (option, value) in settings {
        if !VARY_BY.contains(&option.as_str()) {
            fixed.insert(option.clone(), value.clone());
            continue;
        }
        let Some(entries) = split_option_values(option, value)? else {
            continue;
        };
        if entries.len() > 1 {
            variation_count *= entries.len();
            if variation_count > MAX_VARIATIONS {
                return Err(error(
                    "Provided test options exceeded the maximum number of variations",
                ));
            }
            dimensions.push((option.clone(), entries));
        } else if let Some(entry) = entries.into_iter().next() {
            fixed.insert(option.clone(), entry);
        }
    }
    if dimensions.is_empty() {
        if fixed.is_empty() {
            return Ok(Vec::new());
        }
        return Ok(vec![NativeConfiguration {
            name: String::new(),
            settings: fixed,
        }]);
    }
    let mut varying = vec![BTreeMap::new()];
    for (option, entries) in &dimensions {
        varying = varying
            .into_iter()
            .flat_map(|state: BTreeMap<String, String>| {
                entries.iter().map(move |entry| {
                    let mut next = state.clone();
                    next.insert(option.clone(), entry.clone());
                    next
                })
            })
            .collect();
    }
    Ok(varying
        .into_iter()
        .map(|varying| {
            let name = varying
                .iter()
                .map(|(key, value)| format!("{key}={}", value.to_lowercase()))
                .collect::<Vec<_>>()
                .join(",");
            let mut settings = varying;
            for (key, value) in &fixed {
                settings.insert(key.clone(), value.clone());
            }
            NativeConfiguration { name, settings }
        })
        .collect())
}

fn enum_values(option: &str) -> Option<&'static [(&'static str, i32)]> {
    ENUM_OPTIONS
        .iter()
        .find(|(name, _)| *name == option)
        .map(|(_, values)| *values)
}

/// The normalized value of `value` for a varying option, or `None` when the
/// option does not recognize it.
fn normalized_value(option: &str, value: &str) -> Option<i32> {
    let lower = value.to_ascii_lowercase();
    match enum_values(option) {
        Some(values) => values
            .iter()
            .find(|(key, _)| *key == lower)
            .map(|(_, value)| *value),
        None => match lower.as_str() {
            "true" => Some(1),
            "false" => Some(0),
            _ => None,
        },
    }
}

/// `splitOptionValues` (harnessutil.go): the distinct values of a varying
/// option, keeping the first spelling of each normalized value; `*` adds
/// every value and `-x`/`!x` removes one. `None` when the value selects
/// nothing at all.
fn split_option_values(option: &str, value: &str) -> HarnessResult<Option<Vec<String>>> {
    if value.is_empty() {
        return Ok(None);
    }
    let mut star = false;
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part == "*" {
            star = true;
        } else if let Some(excluded) = part.strip_prefix('-').or_else(|| part.strip_prefix('!')) {
            excludes.push(excluded);
        } else {
            includes.push(part);
        }
    }
    if includes.is_empty() && !star && excludes.is_empty() {
        return Ok(None);
    }
    let mut variations: Vec<(i32, String)> = Vec::new();
    let mut add = |spelling: &str| -> HarnessResult<()> {
        let normalized = normalized_value(option, spelling)
            .ok_or_else(|| error(format!("Unknown value '{spelling}' for option '{option}'")))?;
        if !variations
            .iter()
            .any(|(existing, _)| *existing == normalized)
        {
            variations.push((normalized, spelling.to_owned()));
        }
        Ok(())
    };
    for include in &includes {
        add(include)?;
    }
    if star {
        match enum_values(option) {
            Some(values) => {
                for (key, _) in values {
                    add(key)?;
                }
            }
            None => {
                add("true")?;
                add("false")?;
            }
        }
    }
    for exclude in excludes {
        if let Some(normalized) = normalized_value(option, exclude) {
            variations.retain(|(existing, _)| *existing != normalized);
        }
    }
    if variations.is_empty() {
        return Err(error(format!(
            "Variations in test option '@{option}' resulted in an empty set."
        )));
    }
    Ok(Some(
        variations
            .into_iter()
            .map(|(_, spelling)| spelling)
            .collect(),
    ))
}

/// The stem shared by a configuration's baselines: the file name without its
/// extension, plus `(<configuration>)` when the configuration has a name.
pub fn baseline_stem(file_name: &str, configuration: &str) -> String {
    let extensionless = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    if configuration.is_empty() {
        extensionless.to_owned()
    } else {
        format!("{extensionless}({configuration})")
    }
}

/// Why the native runner does not produce baselines for a configuration.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum NativeSkip {
    /// `skippedTests` names the file.
    SkipList,
    /// A directive the harness does not accept (`t.Fatalf`).
    UnknownDirective(String),
    /// `failOnUnsupportedCompilerOptions`: `module=AMD` or `outFile`.
    Fatal(&'static str),
    /// `SkipUnsupportedCompilerOptions`.
    Unsupported(&'static str),
}

/// The options that decide a configuration's disposition: the virtual
/// tsconfig's `compilerOptions` overlaid by the configuration's directives.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EffectiveOptions {
    values: BTreeMap<String, OptionValue>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum OptionValue {
    Text(String),
    Bool(bool),
}

impl EffectiveOptions {
    pub fn from_configuration(
        tsconfig: Option<&str>,
        configuration: &BTreeMap<String, String>,
    ) -> Self {
        let mut values = BTreeMap::new();
        if let Some(options) = tsconfig.and_then(tsconfig_compiler_options) {
            for (key, value) in options {
                let value = match value {
                    serde_json::Value::String(text) => OptionValue::Text(text),
                    serde_json::Value::Bool(flag) => OptionValue::Bool(flag),
                    _ => continue,
                };
                values.insert(key.to_ascii_lowercase(), value);
            }
        }
        for (key, value) in configuration {
            let parsed = match value.to_ascii_lowercase().as_str() {
                "true" => OptionValue::Bool(true),
                "false" => OptionValue::Bool(false),
                _ => OptionValue::Text(value.clone()),
            };
            values.insert(key.clone(), parsed);
        }
        Self { values }
    }

    fn text(&self, key: &str) -> Option<String> {
        match self.values.get(key)? {
            OptionValue::Text(text) => Some(text.to_ascii_lowercase()),
            OptionValue::Bool(_) => None,
        }
    }

    fn is_false(&self, key: &str) -> bool {
        self.values.get(key) == Some(&OptionValue::Bool(false))
    }

    /// The first rule of `failOnUnsupportedCompilerOptions` or
    /// `SkipUnsupportedCompilerOptions` that applies, in that order.
    pub fn unsupported(&self) -> Option<NativeSkip> {
        if self.text("module").as_deref() == Some("amd") {
            return Some(NativeSkip::Fatal("module=amd"));
        }
        if self.text("outfile").is_some_and(|path| !path.is_empty()) {
            return Some(NativeSkip::Fatal("outFile"));
        }
        let rule = match self.text("module").as_deref() {
            Some("umd") => Some("module=umd"),
            Some("system") => Some("module=system"),
            _ => None,
        }
        .or(match self.text("moduleresolution").as_deref() {
            Some("node" | "node10") => Some("moduleResolution=node10"),
            Some("classic") => Some("moduleResolution=classic"),
            _ => None,
        })
        .or(self
            .is_false("esmoduleinterop")
            .then_some("esModuleInterop=false"))
        .or(self
            .is_false("allowsyntheticdefaultimports")
            .then_some("allowSyntheticDefaultImports=false"))
        .or(self
            .text("baseurl")
            .is_some_and(|path| !path.is_empty())
            .then_some("baseUrl"))
        .or((self.text("target").as_deref() == Some("es5")).then_some("target=es5"))
        .or(self
            .is_false("alwaysstrict")
            .then_some("alwaysStrict=false"));
        rule.map(NativeSkip::Unsupported)
    }
}

/// The `compilerOptions` object of a tsconfig text: comments and trailing
/// commas are removed before parsing, as tsconfig parsing tolerates them.
fn tsconfig_compiler_options(text: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let json = strip_jsonc(text);
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    match value.get("compilerOptions")? {
        serde_json::Value::Object(options) => Some(options.clone()),
        _ => None,
    }
}

fn strip_jsonc(text: &str) -> String {
    let without_comments = strip_json_comments(text);
    strip_trailing_commas(&without_comments)
}

/// Copy `text` without `//` and `/* */` comments outside strings.
fn strip_json_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            index = copy_json_string(&chars, index, &mut out);
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'/') {
            while index < chars.len() && chars[index] != '\n' {
                index += 1;
            }
            continue;
        }
        if ch == '/' && chars.get(index + 1) == Some(&'*') {
            index += 2;
            while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                index += 1;
            }
            index += 2;
            continue;
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// Copy `text` without commas that only precede a closing `}` or `]`.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' {
            index = copy_json_string(&chars, index, &mut out);
            continue;
        }
        if ch == ',' {
            let mut next = index + 1;
            while next < chars.len() && chars[next].is_whitespace() {
                next += 1;
            }
            if matches!(chars.get(next), Some('}' | ']')) {
                index += 1;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// Copy the JSON string starting at `start` (a `"`); returns the index after it.
fn copy_json_string(chars: &[char], start: usize, out: &mut String) -> usize {
    out.push(chars[start]);
    let mut index = start + 1;
    while index < chars.len() {
        let ch = chars[index];
        out.push(ch);
        index += 1;
        if ch == '\\' && index < chars.len() {
            out.push(chars[index]);
            index += 1;
        } else if ch == '"' {
            break;
        }
    }
    index
}

/// A case after expansion: every configuration with its baseline stem and,
/// for the ones the native runner does not execute, the reason.
#[derive(Clone, Debug)]
pub struct NativeExpansion {
    pub case: NativeCase,
    pub configurations: Vec<(NativeConfiguration, String, Option<NativeSkip>)>,
}

/// Expand one case the way `TestLocal` does.
pub fn expand_case(profile: &NativeProfile, case: &NativeCase) -> HarnessResult<NativeExpansion> {
    let file_name = case.file_name();
    if SKIPPED_TESTS.contains(&file_name) {
        return Ok(NativeExpansion {
            case: case.clone(),
            configurations: vec![(
                NativeConfiguration {
                    name: String::new(),
                    settings: BTreeMap::new(),
                },
                baseline_stem(file_name, ""),
                Some(NativeSkip::SkipList),
            )],
        });
    }
    let content = profile.read_case(case)?;
    let settings = extract_settings(&content);
    let mut configurations = configurations(&settings)?;
    if configurations.is_empty() {
        configurations.push(NativeConfiguration {
            name: String::new(),
            settings: BTreeMap::new(),
        });
    }
    let unknown = settings
        .keys()
        .find(|name| !KNOWN_DIRECTIVES.contains(&name.as_str()))
        .cloned();
    let (units, _) = compiler::make_units_from_test(&content, file_name)?;
    let tsconfig = units
        .iter()
        .find(|unit| compiler::is_config_file_name(&unit.name))
        .and_then(|unit| unit.content.clone());
    let configurations = configurations
        .into_iter()
        .map(|configuration| {
            let stem = baseline_stem(file_name, &configuration.name);
            let skip = if let Some(name) = &unknown {
                Some(NativeSkip::UnknownDirective(name.clone()))
            } else {
                EffectiveOptions::from_configuration(tsconfig.as_deref(), &configuration.settings)
                    .unsupported()
            };
            (configuration, stem, skip)
        })
        .collect();
    Ok(NativeExpansion {
        case: case.clone(),
        configurations,
    })
}

/// The configuration stems present among `names` for one suite, keyed by
/// stem, with the baseline kinds found.
pub fn baseline_stems(names: &[String], suite: NativeSuite) -> BTreeMap<String, BTreeSet<String>> {
    const KINDS: [&str; 8] = [
        ".errors.txt",
        ".sourcemap.txt",
        ".contentmapper.txt",
        ".trace.json",
        ".js.map",
        ".symbols",
        ".types",
        ".js",
    ];
    let prefix = format!("tsc/testdata/baselines/reference/{}/", suite.name());
    let mut stems: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for name in names {
        let Some(file) = name.strip_prefix(&prefix) else {
            continue;
        };
        if let Some(kind) = KINDS.iter().find(|kind| file.ends_with(**kind)) {
            let stem = &file[..file.len() - kind.len()];
            stems
                .entry(stem.to_owned())
                .or_default()
                .insert((*kind).to_owned());
        }
    }
    stems
}

#[cfg(test)]
#[path = "../../tests/unit/upstream_suites_native/tests.rs"]
mod tests;
