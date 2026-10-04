//! Execution inputs of the native (TypeScript 7.x) compiler-runner cases: the
//! fixture's units mounted on an in-memory host under the virtual root, the
//! options its directives project, and the Program the runner loads.
//!
//! Compiler config files are parsed once through the program-owned root
//! planner. The harness reproduces `harnessIO`'s virtual host for them, then
//! retains TypeScript's original-unit stable membership partition instead of
//! substituting `ParsedCommandLine.fileNames` order. This adapter is not the
//! general filesystem `matchFiles` host.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Value};
use tsc_diagnostics::{JsStr, JsString};
use tsc_host::{CompilerHost, FsCompilerHost, HostError, MemoryCompilerHost};
use tsc_program::{
    load_emitting_program, load_program, parse_config_root_plan, CompilerOptionNumber,
    CompilerOptions, ConfigFilePattern, ConfigHostError, ConfigHostOperation,
    ConfigOptionValueState, ConfigParseHost, ConfigRootPlan, ConfigRootPlanRequest, LibraryCatalog,
    ModuleSuffix, PreparedProgram, ProgramLoadLimits, ProgramOptions, ProgramPath,
};

use super::compiler::{
    extract_compiler_settings, is_config_file_name, make_units_from_test, ParsedUnit,
};
use super::{error, OrderedSetting, SourceEncoding, VIRTUAL_SOURCE_ROOT};
use crate::HarnessResult;

mod js_paths;

/// The compiler-runner fixture of one native (TypeScript 7.x) case: units,
/// links and the tsconfig root plan with Strada's unit semantics, which Go's
/// runner keeps. Configurations are supplied per plan by
/// [`native_compiler_plan`].
pub fn native_compiler_fixture(
    profile: &super::native::NativeProfile,
    case: &super::native::NativeCase,
) -> HarnessResult<Arc<CompilerFixtureInput>> {
    let workspace_path = profile.cases_root(case.suite).join(&case.relative_path);
    let raw: Arc<[u8]> = fs::read(&workspace_path)
        .map_err(|source| error(format!("{}: {source}", workspace_path.display())))?
        .into();
    let (encoding, decoded) = super::decode_source(&raw);
    let upstream_path = format!(
        "tsc/testdata/tests/cases/{}/{}",
        case.suite.name(),
        case.relative_path
    );
    let settings = extract_compiler_settings(&decoded);
    let (parsed_units, links) = make_units_from_test(&decoded, &upstream_path)?;
    let config_offset = parsed_units
        .iter()
        .position(|unit| is_config_file_name(&unit.name));
    let source = Arc::new(VerifiedSource {
        index: 0,
        relative_path: Arc::from(case.relative_path.as_str()),
        upstream_path: Arc::from(upstream_path.as_str()),
        workspace_path: Arc::new(workspace_path),
        git_blob_sha1: Arc::from(""),
        raw,
        encoding,
        decoded: Arc::from(decoded.as_str()),
    });
    compiler_fixture_from_parts(source, settings, parsed_units, config_offset, links).map(Arc::new)
}

/// The execution plan of one native configuration: the fixture's settings in
/// source order and spelling, with the configuration's values (Go lower-cases
/// the names and trims one trailing `;`), and the shared root rules.
pub fn native_compiler_plan(
    fixture: Arc<CompilerFixtureInput>,
    configuration: &super::native::NativeConfiguration,
) -> HarnessResult<CompilerExecutionPlan> {
    let effective_settings = fixture
        .settings
        .iter()
        .map(|setting| OrderedSetting {
            name: setting.name.clone(),
            value: configuration
                .settings
                .get(&setting.name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| setting.value.clone()),
        })
        .collect::<Vec<_>>();
    let current_directory = compiler_current_directory(&effective_settings)?;
    let use_case_sensitive_file_names = compiler_case_sensitivity(&effective_settings);
    let root_selection = compiler_root_selection(&fixture, &effective_settings)?;
    let name: Arc<str> = Arc::from(configuration.name.as_str());
    Ok(CompilerExecutionPlan {
        fixture,
        variant: CompilerVariant {
            configuration_index: 0,
            key: Arc::clone(&name),
            description: Arc::clone(&name),
            upstream_name: name,
            overrides: Arc::from([]),
        },
        effective_settings: Arc::from(effective_settings),
        current_directory: Arc::from(current_directory),
        use_case_sensitive_file_names,
        root_selection,
    })
}

/// Every file under a native profile's `tests/lib` with its path relative to
/// it (`testLibFolderMap` in harnessutil), in path order.
pub fn read_test_library(root: &Path) -> HarnessResult<Vec<(String, Arc<str>)>> {
    let mut files = Vec::new();
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory)
            .map_err(|source| error(format!("{}: {source}", directory.display())))?;
        for entry in entries {
            let path = entry.map_err(|source| error(source.to_string()))?.path();
            if path.is_dir() {
                directories.push(path);
                continue;
            }
            let text = fs::read_to_string(&path)
                .map_err(|source| error(format!("{}: {source}", path.display())))?;
            let relative = path
                .strip_prefix(root)
                .expect("walked under the test library")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, Arc::from(text)));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

/// Load a native plan the way the native runner builds its Program: every
/// option as configured (no implied `noEmit`; a configuration that sets
/// `noEmit` loads through the no-emit loader, every other one through the
/// emitting loader, whose Program reports the output-path checks), the
/// harness defaults, and the runner's file system (`CompileFilesEx`): a later
/// unit of the same path replaces an earlier one, and `test_library` (the
/// profile's `tests/lib`) is mounted at `/.lib` when a root file mentions
/// `/.lib/`. `standard_library` is the profile's `lib.*.d.ts` catalog.
pub fn load_native_compiler_program(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    limits: ProgramLoadLimits,
    test_library: &Path,
    standard_library: &Path,
) -> HarnessResult<PreparedProgram> {
    let current_directory = plan.current_directory.as_ref();
    let (vfs_write_order, root_units, other_units) = match &plan.root_selection {
        CompilerRootSelection::Explicit {
            vfs_write_order,
            root_units,
            other_units,
            ..
        }
        | CompilerRootSelection::Config {
            vfs_write_order,
            root_units,
            other_units,
            ..
        } => (vfs_write_order, root_units, other_units),
    };
    let unit = |unit_id: &CompilerUnitId| {
        plan.fixture
            .units
            .get(unit_id.0 as usize)
            .ok_or_else(|| error("compiler VFS unit is out of bounds"))
    };
    // The native runner writes `toBeCompiled` and then `otherFiles` into one
    // map, so a later unit of the same path replaces an earlier one.
    let mut native_contents = HashMap::<String, Arc<str>>::new();
    for unit_id in root_units.iter().chain(other_units.iter()) {
        let unit = unit(unit_id)?;
        if let Some(content) = &unit.content {
            let path = normalize_compiler_fixture_path(current_directory, unit.name.as_ref())?;
            native_contents.insert(path, Arc::clone(content));
        }
    }
    let mut files = Vec::<(String, Arc<str>)>::new();
    let mut written = HashSet::<String>::new();
    for unit_id in vfs_write_order.iter() {
        let unit = unit(unit_id)?;
        let path = normalize_compiler_fixture_path(current_directory, unit.name.as_ref())?;
        let Some(content) = unit.content.as_ref() else {
            continue;
        };
        if !written.insert(path.clone()) {
            continue;
        }
        let content = native_contents.get(&path).unwrap_or(content);
        files.push((path, Arc::clone(content)));
    }
    let mentions_test_library = root_units.iter().any(|unit_id| {
        unit(unit_id).is_ok_and(|unit| {
            unit.content
                .as_deref()
                .is_some_and(|content| content.contains("/.lib/"))
        })
    });
    let roots = compiler_root_paths(plan)?;
    load_native_program_files(
        workspace,
        plan,
        files,
        mentions_test_library,
        roots,
        limits,
        test_library,
        standard_library,
    )
}

/// The native runner's compile of a configuration's emitted declaration
/// files (`compileDeclarationFiles` in `tsbaseline/js_emit_baseline.go`, which
/// calls `CompileFilesEx`): a file system of only `inputs` and then `others`
/// (a later file of the same path replaces an earlier one), the
/// configuration's options and links, and every input but a `.json` one as a
/// root. Paths are normalized absolute fixture paths.
#[allow(clippy::too_many_arguments)]
pub fn load_native_declaration_program(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    inputs: &[(String, String)],
    others: &[(String, String)],
    limits: ProgramLoadLimits,
    test_library: &Path,
    standard_library: &Path,
) -> HarnessResult<PreparedProgram> {
    let mut contents = HashMap::<&str, &str>::new();
    for (path, content) in inputs.iter().chain(others) {
        contents.insert(path, content);
    }
    let mut files = Vec::<(String, Arc<str>)>::new();
    let mut written = HashSet::<&str>::new();
    for (path, _) in inputs.iter().chain(others) {
        if written.insert(path) {
            files.push((path.clone(), Arc::from(contents[path.as_str()])));
        }
    }
    let mentions_test_library = inputs.iter().any(|(_, content)| content.contains("/.lib/"));
    let roots = inputs
        .iter()
        .filter(|(path, _)| !path.ends_with(".json"))
        .map(|(path, _)| PathBuf::from(path))
        .collect();
    load_native_program_files(
        workspace,
        plan,
        files,
        mentions_test_library,
        roots,
        limits,
        test_library,
        standard_library,
    )
}

/// The runner's file system (`files` in write order, the test library when
/// mentioned, and the fixture's links) and the Program over `roots` with the
/// plan's options and the harness defaults.
#[allow(clippy::too_many_arguments)]
fn load_native_program_files(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    files: Vec<(String, Arc<str>)>,
    mentions_test_library: bool,
    roots: Vec<PathBuf>,
    limits: ProgramLoadLimits,
    test_library: &Path,
    standard_library: &Path,
) -> HarnessResult<PreparedProgram> {
    let current_directory = plan.current_directory.as_ref();
    let mut host_builder = MemoryCompilerHost::builder(current_directory)
        .case_sensitive(plan.use_case_sensitive_file_names);
    let mut source_paths = HashMap::<String, Arc<str>>::new();
    for (path, content) in files {
        host_builder = host_builder.file(&path, content.as_bytes().to_vec());
        source_paths.insert(path, content);
    }
    if mentions_test_library {
        for (relative, content) in read_test_library(test_library)? {
            if let std::collections::hash_map::Entry::Vacant(entry) =
                source_paths.entry(format!("/.lib/{relative}"))
            {
                host_builder = host_builder.file(entry.key(), content.as_bytes().to_vec());
                entry.insert(content);
            }
        }
    }

    // The compiler runner's VFS presents document/global symlinks through
    // `realpath`. MemoryCompilerHost requires both spellings to exist before
    // accepting that identity override, so publish a byte-identical alias
    // for every file a link makes visible: an exact-file link publishes its
    // target; a DIRECTORY link publishes every descendant of its target
    // under the link-relative suffix (upstream's vfs mounts the target
    // directory node at the link path, so `fileExists(link/sub/file)` holds,
    // `realpath` resolves to the physical file, and module resolution sees
    // package directories such as `node_modules/package-a/{package.json,
    // index.d.ts}` — `_tsc.js:41239-41246` accepts each aliased file and
    // `:40047-40054` keeps both the original and the real spelling).
    // Descendants published by other links count too (a link into a
    // directory that itself contains links), so the expansion repeats until
    // no alias is added; every alias resolves through earlier overrides to
    // its physical file.
    let symlink_operations = plan
        .fixture
        .global_symlinks
        .iter()
        .chain(
            plan.fixture
                .units
                .iter()
                .flat_map(|unit| unit.document_symlinks.iter()),
        )
        .collect::<Vec<_>>();
    let mut realpath_overrides: BTreeMap<String, String> = BTreeMap::new();
    let resolve_physical = |overrides: &BTreeMap<String, String>, path: &str| -> String {
        let mut current = path.to_owned();
        for _ in 0..symlink_operations.len().max(1) {
            match overrides.get(&current) {
                Some(next) if *next != current => current = next.clone(),
                _ => break,
            }
        }
        current
    };
    let mut expanded = true;
    let mut passes = 0usize;
    while expanded && passes <= symlink_operations.len() {
        expanded = false;
        passes += 1;
        for operation in &symlink_operations {
            let target = operation.normalized_target.as_ref();
            let link = operation.normalized_link_path.as_ref();
            let mut aliases: Vec<(String, String)> = Vec::new();
            if source_paths.contains_key(target) {
                aliases.push((link.to_owned(), target.to_owned()));
            } else {
                let prefix = format!("{}/", target.trim_end_matches('/'));
                let link_prefix = link.trim_end_matches('/');
                let mut descendants = source_paths.keys().collect::<Vec<_>>();
                descendants.sort();
                for descendant in descendants {
                    if let Some(suffix) = descendant.strip_prefix(&prefix) {
                        aliases.push((format!("{link_prefix}/{suffix}"), descendant.clone()));
                    }
                }
            }
            for (alias, physical) in aliases {
                let physical = resolve_physical(&realpath_overrides, &physical);
                if alias == physical {
                    continue;
                }
                if !source_paths.contains_key(&alias) {
                    let Some(content) = source_paths.get(&physical).cloned() else {
                        continue;
                    };
                    host_builder = host_builder.file(alias.as_str(), content.as_bytes().to_vec());
                    source_paths.insert(alias.clone(), content);
                    expanded = true;
                }
                if realpath_overrides.get(&alias) != Some(&physical) {
                    realpath_overrides.insert(alias, physical);
                    expanded = true;
                }
            }
        }
    }
    for (link, target) in realpath_overrides {
        host_builder = host_builder.realpath(PathBuf::from(link), PathBuf::from(target));
    }
    for directory in compiler_vfs_trailing_directory_aliases(
        source_paths.keys().map(|path| Path::new(path.as_str())),
    )? {
        host_builder = host_builder.directory(directory);
    }
    let fixture_host = host_builder.build().map_err(|host_error| {
        error(format!(
            "failed to build compiler fixture host for {:?}: {host_error}",
            plan.fixture.source.relative_path
        ))
    })?;
    let library_directory = standard_library.to_path_buf();
    let host = CompilerSuiteHost::new(
        workspace,
        fixture_host,
        library_directory.clone(),
        plan.use_case_sensitive_file_names,
    )?;

    let (mut compiler_options, program_options) = plan_compiler_options(plan)?;
    // CompilerBaselineRunner normalizes these harness-only defaults
    // after config and directive projection. They are not command-line
    // defaults: in particular, its absent `newLine` becomes CRLF even when
    // the host running this adapter normally uses LF. Retain that distinction
    // here so an emitting compiler-suite plan observes the upstream baseline
    // bytes while the production CLI continues to use its own host default.
    compiler_options.new_line.get_or_insert(0);
    compiler_options.no_error_truncation = Some(true);
    let catalog = LibraryCatalog::typescript_7_1(library_directory);
    let loaded = if compiler_options.no_emit == Some(true) {
        load_program(
            &host,
            &roots,
            compiler_options,
            program_options,
            &catalog,
            limits,
        )
    } else {
        load_emitting_program(
            &host,
            &roots,
            compiler_options,
            program_options,
            &catalog,
            limits,
        )
    };
    loaded.map_err(|load_error| {
        error(format!(
            "failed to load compiler fixture {:?} ({:?}): {load_error}",
            plan.fixture.source.relative_path, plan.variant.key
        ))
    })
}

/// The upstream compiler runner's virtual filesystem treats a directory path
/// with or without its final separator as one directory. MemoryCompilerHost
/// intentionally preserves exact host queries, so the adapter materializes
/// the second spelling instead of weakening the host or resolver contracts.
fn compiler_vfs_trailing_directory_aliases<'path>(
    paths: impl IntoIterator<Item = &'path Path>,
) -> HarnessResult<BTreeSet<PathBuf>> {
    let mut aliases = BTreeSet::new();
    for path in paths {
        for directory in path.ancestors().skip(1) {
            if directory == Path::new("/") {
                continue;
            }
            let text = directory.to_str().ok_or_else(|| {
                error(format!(
                    "compiler VFS directory is not valid Unicode: {directory:?}"
                ))
            })?;
            aliases.insert(PathBuf::from(format!("{text}/")));
        }
    }
    Ok(aliases)
}

/// Compiler-runner VFS with the exact vendored library directory mounted
/// read-only from the workspace. Fixture paths remain entirely in memory;
/// only catalog-owned absolute paths can reach the filesystem host.
#[derive(Debug)]
struct CompilerSuiteHost {
    fixture: MemoryCompilerHost,
    libraries: FsCompilerHost,
    library_directory: PathBuf,
    library_directory_js: JsString,
}

impl CompilerSuiteHost {
    fn new(
        workspace: &Path,
        fixture: MemoryCompilerHost,
        library_directory: PathBuf,
        case_sensitive: bool,
    ) -> HarnessResult<Self> {
        let library_directory = fs::canonicalize(&library_directory).map_err(|source| {
            error(format!(
                "failed to canonicalize compiler library mount {library_directory:?}: {source}"
            ))
        })?;
        let libraries = FsCompilerHost::new(workspace, case_sensitive).map_err(|source| {
            error(format!(
                "failed to construct compiler library filesystem host: {source}"
            ))
        })?;
        let library_directory_js = library_directory
            .to_str()
            .ok_or_else(|| error("native library mount is not Unicode"))?
            .into();
        Ok(Self {
            library_directory_js,
            fixture,
            libraries,
            library_directory,
        })
    }

    fn is_library_path(&self, path: &Path) -> bool {
        path.is_absolute()
            && path.starts_with(&self.library_directory)
            && !path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
    }
}

impl CompilerHost for CompilerSuiteHost {
    fn current_directory_js(&self) -> Result<JsString, HostError> {
        self.fixture.current_directory_js()
    }
    fn read_file_js(&self, path: JsStr<'_>) -> Result<Option<Vec<u8>>, HostError> {
        if js_paths::library_query(path, self.library_directory_js.as_js()) {
            self.libraries.read_file_js(path)
        } else {
            self.fixture.read_file_js(path)
        }
    }
    fn file_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if js_paths::library_query(path, self.library_directory_js.as_js()) {
            self.libraries.file_exists_js(path)
        } else {
            self.fixture.file_exists_js(path)
        }
    }
    fn directory_exists_js(&self, path: JsStr<'_>) -> Result<bool, HostError> {
        if js_paths::library_query(path, self.library_directory_js.as_js()) {
            self.libraries.directory_exists_js(path)
        } else {
            self.fixture.directory_exists_js(path)
        }
    }
    fn read_directory_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        if js_paths::library_query(path, self.library_directory_js.as_js()) {
            self.libraries.read_directory_js(path)
        } else {
            self.fixture.read_directory_js(path)
        }
    }
    fn get_directories_js(&self, path: JsStr<'_>) -> Result<Vec<JsString>, HostError> {
        if js_paths::library_query(path, self.library_directory_js.as_js()) {
            self.libraries.get_directories_js(path)
        } else {
            self.fixture.get_directories_js(path)
        }
    }
    fn realpath_js(&self, path: JsStr<'_>) -> Result<Option<JsString>, HostError> {
        if js_paths::library_query(path, self.library_directory_js.as_js()) {
            self.libraries.realpath_js(path)
        } else {
            self.fixture.realpath_js(path)
        }
    }

    fn current_directory(&self) -> Result<PathBuf, HostError> {
        self.fixture.current_directory()
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        self.fixture.use_case_sensitive_file_names()
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        if self.is_library_path(path) {
            self.libraries.read_file(path)
        } else {
            self.fixture.read_file(path)
        }
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        if self.is_library_path(path) {
            self.libraries.file_exists(path)
        } else {
            self.fixture.file_exists(path)
        }
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        if self.is_library_path(path) {
            self.libraries.directory_exists(path)
        } else {
            self.fixture.directory_exists(path)
        }
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if self.is_library_path(path) {
            self.libraries.read_directory(path)
        } else {
            self.fixture.read_directory(path)
        }
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if self.is_library_path(path) {
            self.libraries.get_directories(path)
        } else {
            self.fixture.get_directories(path)
        }
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        if self.is_library_path(path) {
            self.libraries.realpath(path)
        } else {
            self.fixture.realpath(path)
        }
    }
}

fn compiler_root_paths(plan: &CompilerExecutionPlan) -> HarnessResult<Vec<PathBuf>> {
    let units = match &plan.root_selection {
        CompilerRootSelection::Explicit {
            program_root_units, ..
        }
        | CompilerRootSelection::Config {
            program_root_units, ..
        } => program_root_units,
    };
    units
        .iter()
        .map(|id| {
            let unit = plan
                .fixture
                .units
                .get(id.0 as usize)
                .ok_or_else(|| error("compiler root unit is out of bounds"))?;
            normalize_compiler_fixture_path(plan.current_directory.as_ref(), unit.name.as_ref())
                .map(PathBuf::from)
        })
        .collect()
}

fn plan_compiler_options(
    plan: &CompilerExecutionPlan,
) -> HarnessResult<(CompilerOptions, ProgramOptions)> {
    let (compiler_options, mut program_options) = project_compiler_options(
        &plan.fixture,
        &plan.effective_settings,
        plan.current_directory.as_ref(),
    )?;

    if plan.fixture.config_root_plan.is_some() {
        // CompilerBaselineRunner passes the parsed config source through to
        // createProgram after applying harness settings. The Program must
        // therefore verify the effective values while retaining config syntax
        // solely as diagnostic provenance.
        program_options = program_options.with_program_owned_config_option_diagnostics();
    }
    Ok((compiler_options, program_options))
}

fn project_compiler_options(
    fixture: &CompilerFixtureInput,
    settings: &[OrderedSetting],
    current_directory: &str,
) -> HarnessResult<(CompilerOptions, ProgramOptions)> {
    let (mut compiler_options, mut program_options, config_has_explicit_allow_js) = fixture
        .config_root_plan
        .as_ref()
        .map(|config| {
            (
                config.compiler_options().clone(),
                config.program_options().clone(),
                matches!(
                    config.options().typed_value_state("allowJs"),
                    ConfigOptionValueState::Value(value) if value.is_boolean()
                ),
            )
        })
        .unwrap_or_else(|| (CompilerOptions::default(), ProgramOptions::default(), false));

    // CompilerBaselineRunner's effective-options contract defaults this to
    // true only when the config did not supply a value. Fixture settings are
    // applied below and retain the final override.
    compiler_options.skip_default_lib_check.get_or_insert(true);
    apply_compiler_settings(
        &mut compiler_options,
        &mut program_options,
        current_directory,
        settings
            .iter()
            .map(|setting| (setting.name.as_str(), setting.value.as_str())),
        config_has_explicit_allow_js,
    )?;
    Ok((compiler_options, program_options))
}

/// Apply an ordered compiler-runner setting layer, then materialize tsc's
/// computed `allowJs` value once at the boundary. `CompilerOptions::allow_js`
/// is the effective Rust value; retaining whether a lower layer explicitly
/// supplied `allowJs` here prevents `checkJs` from overriding an explicit
/// false while still giving an absent `allowJs` the tsc default.
fn apply_compiler_settings<'setting>(
    compiler_options: &mut CompilerOptions,
    program_options: &mut ProgramOptions,
    current_directory: &str,
    settings: impl IntoIterator<Item = (&'setting str, &'setting str)>,
    lower_layer_has_explicit_allow_js: bool,
) -> HarnessResult<()> {
    let mut has_explicit_allow_js = lower_layer_has_explicit_allow_js;
    for (name, value) in settings {
        let key = CompilerFixtureOptionKey::new(name);
        has_explicit_allow_js |= key.as_str() == "allowjs";
        apply_compiler_setting(
            compiler_options,
            program_options,
            current_directory,
            name,
            value,
        )?;
    }
    if !has_explicit_allow_js {
        compiler_options.allow_js = compiler_options.check_js.unwrap_or(false);
    }
    Ok(())
}

/// Canonical lookup key for a compiler-runner setting.
///
/// TypeScript's compiler test harness resolves option declarations without
/// regard to ASCII case. Keeping that policy at this boundary avoids growing
/// spelling aliases throughout the projection below and gives newly admitted
/// canonical options the same lookup semantics automatically.
#[derive(Clone, Debug, Eq, PartialEq)]
struct CompilerFixtureOptionKey(Box<str>);

impl CompilerFixtureOptionKey {
    fn new(raw_name: &str) -> Self {
        Self(raw_name.to_ascii_lowercase().into_boxed_str())
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

fn apply_compiler_setting(
    compiler_options: &mut CompilerOptions,
    program_options: &mut ProgramOptions,
    current_directory: &str,
    name: &str,
    value: &str,
) -> HarnessResult<()> {
    let key = CompilerFixtureOptionKey::new(name);
    let boolean = || parse_compiler_bool(name, value);
    match key.as_str() {
        "allowjs" => compiler_options.allow_js = boolean()?,
        "checkjs" => compiler_options.check_js = Some(boolean()?),
        "forceconsistentcasinginfilenames" => {
            compiler_options.force_consistent_casing_in_file_names = Some(boolean()?)
        }
        "maxnodemodulejsdepth" => {
            compiler_options.max_node_module_js_depth = Some(CompilerOptionNumber::new(
                value.parse::<f64>().map_err(|_| {
                    error(format!(
                        "compiler option {name:?} is not a number: {value:?}"
                    ))
                })?,
            ))
        }
        "experimentaldecorators" => compiler_options.experimental_decorators = boolean()?,
        "emitdecoratormetadata" => compiler_options.emit_decorator_metadata = Some(boolean()?),
        "target" => compiler_options.target = Some(parse_target(value)?),
        "module" => compiler_options.module = Some(parse_module(value)?),
        "moduleresolution" => {
            compiler_options.module_resolution = Some(parse_module_resolution(value)?)
        }
        "moduledetection" => {
            compiler_options.module_detection = Some(parse_module_detection(value)?)
        }
        "jsx" => compiler_options.jsx = Some(parse_jsx(value)?),
        "noemit" => compiler_options.no_emit = Some(boolean()?),
        // tsgo's harness parses it as a compiler option (harnessutil.go:317-333).
        "suppressoutputpathcheck" => compiler_options.suppress_output_path_check = Some(boolean()?),
        "noresolve" => compiler_options.no_resolve = Some(boolean()?),
        "erasablesyntaxonly" => compiler_options.erasable_syntax_only = Some(boolean()?),
        "nolib" => *program_options = program_options.clone().with_no_lib(boolean()?),
        "preservesymlinks" => {
            *program_options = program_options.clone().with_preserve_symlinks(boolean()?)
        }
        "lib" => {
            compiler_options.lib = Some(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_ascii_lowercase)
                    .collect(),
            )
        }
        "libreplacement" => compiler_options.lib_replacement = Some(boolean()?),
        "types" => {
            *program_options = program_options.clone().with_types(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(JsString::from)
                    .collect(),
            )
        }
        "typeroots" => {
            *program_options = program_options
                .clone()
                .with_type_roots(parse_virtual_program_paths(current_directory, value, name)?)
        }
        "strict" => compiler_options.strict = Some(boolean()?),
        "strictnullchecks" => compiler_options.strict_null_checks = Some(boolean()?),
        "strictfunctiontypes" => compiler_options.strict_function_types = Some(boolean()?),
        "noimplicitany" => compiler_options.no_implicit_any = Some(boolean()?),
        "noimplicitthis" => compiler_options.no_implicit_this = Some(boolean()?),
        "noimplicitoverride" => compiler_options.no_implicit_override = Some(boolean()?),
        "strictbindcallapply" => compiler_options.strict_bind_call_apply = Some(boolean()?),
        "exactoptionalpropertytypes" => {
            compiler_options.exact_optional_property_types = Some(boolean()?)
        }
        "nofallthroughcasesinswitch" => {
            compiler_options.no_fallthrough_cases_in_switch = Some(boolean()?)
        }
        "noimplicitreturns" => compiler_options.no_implicit_returns = Some(boolean()?),
        "nounusedlocals" => compiler_options.no_unused_locals = Some(boolean()?),
        "nounusedparameters" => compiler_options.no_unused_parameters = Some(boolean()?),
        "allowunreachablecode" => compiler_options.allow_unreachable_code = Some(boolean()?),
        "allowunusedlabels" => compiler_options.allow_unused_labels = Some(boolean()?),
        "nouncheckedindexedaccess" => {
            compiler_options.no_unchecked_indexed_access = Some(boolean()?)
        }
        "nopropertyaccessfromindexsignature" => {
            compiler_options.no_property_access_from_index_signature = Some(boolean()?)
        }
        "nouncheckedsideeffectimports" => {
            compiler_options.no_unchecked_side_effect_imports = Some(boolean()?)
        }
        "strictpropertyinitialization" => {
            compiler_options.strict_property_initialization = Some(boolean()?)
        }
        "usedefineforclassfields" => {
            compiler_options.use_define_for_class_fields = Some(boolean()?)
        }
        "useunknownincatchvariables" => {
            compiler_options.use_unknown_in_catch_variables = Some(boolean()?)
        }
        "alwaysstrict" => compiler_options.always_strict = Some(boolean()?),
        "noerrortruncation" => compiler_options.no_error_truncation = Some(boolean()?),
        "importhelpers" => compiler_options.import_helpers = Some(boolean()?),
        "downleveliteration" => compiler_options.downlevel_iteration = Some(boolean()?),
        "strictbuiltiniteratorreturn" => {
            compiler_options.strict_builtin_iterator_return = Some(boolean()?)
        }
        "modulesuffixes" => {
            compiler_options.module_suffixes = Some(
                value
                    .split(',')
                    .map(|entry| ModuleSuffix::Value(entry.into()))
                    .collect(),
            )
        }
        "resolvepackagejsonexports" => {
            compiler_options.resolve_package_json_exports = Some(boolean()?)
        }
        "resolvepackagejsonimports" => {
            compiler_options.resolve_package_json_imports = Some(boolean()?)
        }
        "customconditions" => {
            compiler_options.custom_conditions = Some(
                value
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(JsString::from)
                    .collect(),
            )
        }
        "nodtsresolution" => compiler_options.no_dts_resolution = Some(boolean()?),
        "allowarbitraryextensions" => {
            compiler_options.allow_arbitrary_extensions = Some(boolean()?)
        }
        "allowimportingtsextensions" => {
            compiler_options.allow_importing_ts_extensions = Some(boolean()?)
        }
        "rewriterelativeimportextensions" => {
            compiler_options.rewrite_relative_import_extensions = Some(boolean()?)
        }
        "resolvejsonmodule" => compiler_options.resolve_json_module = Some(boolean()?),
        "skiplibcheck" => compiler_options.skip_lib_check = Some(boolean()?),
        "skipdefaultlibcheck" => compiler_options.skip_default_lib_check = Some(boolean()?),
        "esmoduleinterop" => compiler_options.es_module_interop = Some(boolean()?),
        "allowsyntheticdefaultimports" => {
            compiler_options.allow_synthetic_default_imports = Some(boolean()?)
        }
        "preserveconstenums" => compiler_options.preserve_const_enums = Some(boolean()?),
        "isolatedmodules" => compiler_options.isolated_modules = Some(boolean()?),
        "verbatimmodulesyntax" => compiler_options.verbatim_module_syntax = Some(boolean()?),
        "allowumdglobalaccess" => compiler_options.allow_umd_global_access = Some(boolean()?),
        "baseurl" => compiler_options.base_url = Some(value.to_owned().into()),
        "jsxfactory" => compiler_options.jsx_factory = Some(value.to_owned().into()),
        "jsxfragmentfactory" => {
            compiler_options.jsx_fragment_factory = Some(value.to_owned().into())
        }
        "jsximportsource" => compiler_options.jsx_import_source = Some(value.to_owned().into()),
        "reactnamespace" => compiler_options.react_namespace = Some(value.to_owned().into()),
        "ignoredeprecations" => {
            compiler_options.ignore_deprecations = Some(value.to_owned().into())
        }
        "newline" => {
            compiler_options.new_line = Some(match value.to_ascii_lowercase().as_str() {
                "crlf" => 0,
                "lf" => 1,
                _ => {
                    return Err(error(format!(
                        "unsupported compiler setting newLine={value:?}"
                    )))
                }
            })
        }
        "removecomments" => compiler_options.remove_comments = Some(boolean()?),
        "declaration" => compiler_options.declaration = Some(boolean()?),
        "composite" => compiler_options.composite = Some(boolean()?),
        "isolateddeclarations" => compiler_options.isolated_declarations = Some(boolean()?),
        // The flag gates upstream handleNoEmitOptions (_tsc.js:125636-125663):
        // dropping it silently leaves every harness execution unblocked, which
        // hides the blocked-emit diagnostic set the observations record
        // (H2.5h CA-2b).
        "noemitonerror" => compiler_options.no_emit_on_error = Some(boolean()?),
        "sourcemap" => compiler_options.source_map = Some(boolean()?),
        "inlinesourcemap" => compiler_options.inline_source_map = Some(boolean()?),
        "inlinesources" => compiler_options.inline_sources = Some(boolean()?),
        "sourceroot" => compiler_options.source_root = Some(value.to_owned().into()),
        "maproot" => compiler_options.map_root = Some(value.to_owned().into()),
        "emitbom" => compiler_options.emit_bom = Some(boolean()?),
        "emitdeclarationonly" => compiler_options.emit_declaration_only = Some(boolean()?),
        "declarationmap" => compiler_options.declaration_map = Some(boolean()?),
        "outfile" => compiler_options.out_file = Some(value.to_owned().into()),
        "outdir" => compiler_options.out_dir = Some(value.to_owned().into()),
        "noemithelpers" => compiler_options.no_emit_helpers = Some(boolean()?),
        // The native runner builds the whole Program with these options
        // (`CompileFilesEx` makes the paths absolute).
        "rootdir" | "declarationdir" | "tsbuildinfofile" => {
            let path = Some(normalize_virtual_path(current_directory, value)?.into());
            match key.as_str() {
                "rootdir" => compiler_options.root_dir = path,
                "declarationdir" => compiler_options.declaration_dir = path,
                _ => compiler_options.ts_build_info_file = path,
            }
        }
        "stripinternal" => compiler_options.strip_internal = Some(boolean()?),
        "nocheck" => compiler_options.no_check = Some(boolean()?),
        "incremental" => compiler_options.incremental = Some(boolean()?),
        "assumechangesonlyaffectdirectdependencies"
        | "disablesizelimit"
        | "out"
        | "pretty"
        | "traceresolution"
        | "listfilesonly"
        | "capturesuggestions"
        | "fullemitpaths"
        | "typescriptversion"
        | "stabletypeordering"
        | "notypesandsymbols"
        | "noimplicitreferences"
        | "currentdirectory"
        | "usecasesensitivefilenames"
        | "filename"
        | "link"
        | "symlink" => {}
        _ => {
            return Err(error(format!(
                "unsupported compiler fixture option {name:?}"
            )))
        }
    }
    Ok(())
}

fn parse_virtual_program_paths(
    current_directory: &str,
    value: &str,
    option_name: &str,
) -> HarnessResult<Vec<ProgramPath>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let normalized = normalize_virtual_path(current_directory, entry)?;
            ProgramPath::from_trusted_parts(&normalized, &normalized).map_err(|path_error| {
                error(format!(
                    "compiler option {option_name:?} contains invalid path {entry:?}: {path_error}"
                ))
            })
        })
        .collect()
}

/// Compiler-test directives retain their trimmed text through
/// `TestCaseParser.makeUnitsFromTest`. The harness boolean converter then
/// recognizes only the case-insensitive `true` lexeme; every other value is
/// false. In particular, a historical directive such as `true;` is not
/// normalized by removing the semicolon.
///
/// tsc-port: equateStringsCaseInsensitive @6.0.3 (harness boolean-arm comparison)
/// tsc-hash: 1798be4a0411df11d02a3c1ab582f840d2c3d2bae6a48dc4803dabc1155e485c
/// tsc-span: _tsc.js:905-907
fn parse_compiler_bool(_name: &str, value: &str) -> HarnessResult<bool> {
    Ok(value.eq_ignore_ascii_case("true"))
}

fn parse_target(value: &str) -> HarnessResult<i32> {
    match value.to_ascii_lowercase().as_str() {
        "es3" => Ok(0),
        "es5" => Ok(1),
        "es6" | "es2015" => Ok(2),
        "es2016" => Ok(3),
        "es2017" => Ok(4),
        "es2018" => Ok(5),
        "es2019" => Ok(6),
        "es2020" => Ok(7),
        "es2021" => Ok(8),
        "es2022" => Ok(9),
        "es2023" => Ok(10),
        "es2024" => Ok(11),
        "es2025" => Ok(12),
        "es2026" => Ok(13),
        "esnext" => Ok(99),
        _ => Err(error(format!("unsupported compiler target {value:?}"))),
    }
}

fn parse_module(value: &str) -> HarnessResult<i32> {
    match value.to_ascii_lowercase().as_str() {
        "none" => Ok(0),
        "commonjs" => Ok(1),
        "amd" => Ok(2),
        "umd" => Ok(3),
        "system" => Ok(4),
        "es6" | "es2015" => Ok(5),
        "es2020" => Ok(6),
        "es2022" => Ok(7),
        "esnext" => Ok(99),
        "node16" => Ok(100),
        "node18" => Ok(101),
        "node20" => Ok(102),
        "nodenext" => Ok(199),
        "preserve" => Ok(200),
        _ => Err(error(format!("unsupported compiler module {value:?}"))),
    }
}

fn parse_module_resolution(value: &str) -> HarnessResult<i32> {
    match value.to_ascii_lowercase().as_str() {
        "classic" => Ok(1),
        "node" | "node10" => Ok(2),
        "node16" => Ok(3),
        "nodenext" => Ok(99),
        "bundler" => Ok(100),
        _ => Err(error(format!(
            "unsupported compiler moduleResolution {value:?}"
        ))),
    }
}

fn parse_module_detection(value: &str) -> HarnessResult<i32> {
    match value.to_ascii_lowercase().as_str() {
        "legacy" => Ok(1),
        "auto" => Ok(2),
        "force" => Ok(3),
        _ => Err(error(format!(
            "unsupported compiler moduleDetection {value:?}"
        ))),
    }
}

fn parse_jsx(value: &str) -> HarnessResult<i32> {
    match value.to_ascii_lowercase().as_str() {
        "preserve" => Ok(1),
        "react" => Ok(2),
        "react-native" => Ok(3),
        "react-jsx" => Ok(4),
        "react-jsxdev" => Ok(5),
        _ => Err(error(format!("unsupported compiler jsx {value:?}"))),
    }
}

/// Content and identity for one source path in the vendored corpus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSource {
    pub index: u32,
    pub relative_path: Arc<str>,
    pub upstream_path: Arc<str>,
    pub workspace_path: Arc<PathBuf>,
    pub git_blob_sha1: Arc<str>,
    pub raw: Arc<[u8]>,
    pub encoding: SourceEncoding,
    pub decoded: Arc<str>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CompilerUnitId(pub u32);

#[derive(Clone, Debug)]
pub struct CompilerFixtureInput {
    pub source: Arc<VerifiedSource>,
    /// Original unit occurrence order, including the virtual config file.
    pub units: Arc<[CompilerUnitInput]>,
    pub config_unit: Option<CompilerUnitId>,
    /// Program-owned config parse/root plan, materialized once per fixture and
    /// shared by all configuration variants.
    pub config_root_plan: Option<Arc<ConfigRootPlan>>,
    /// Exact ParseConfigHost call order observed while materializing the
    /// shared config plan. The frozen compiler oracle carries the same trace;
    /// retaining it here makes the full 103-fixture qualification independent
    /// of the later program-loader host.
    pub config_host_log: Arc<[Value]>,
    pub settings: Arc<[OrderedSetting]>,
    /// Lossless `@link` directive order before JavaScript object assignment.
    pub global_symlink_directives: Arc<[CompilerSymlinkOperation]>,
    /// Effective FileSet order. Repeated normalized link keys replace their
    /// target without moving the key's first insertion position.
    pub global_symlinks: Arc<[CompilerSymlinkOperation]>,
}

#[derive(Clone, Debug)]
pub struct CompilerUnitInput {
    pub id: CompilerUnitId,
    pub name: Arc<str>,
    /// `None` preserves JavaScript `undefined` for an intermediate empty unit.
    pub content: Option<Arc<str>>,
    pub file_options: Arc<[OrderedSetting]>,
    pub original_fixture_path: Arc<str>,
    /// TypeScript 6.0.3 initializes this array but does not populate it here.
    pub references: Arc<[Arc<str>]>,
    pub document_symlinks: Arc<[CompilerSymlinkOperation]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerSymlinkPhase {
    Document,
    Global,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompilerSymlinkOperation {
    pub phase: CompilerSymlinkPhase,
    /// For document symlinks this is the unit path; for global `@link` it is
    /// the directive's target spelling.
    pub raw_target: Arc<str>,
    pub raw_link_path: Arc<str>,
    pub anchor: Arc<str>,
    pub normalized_target: Arc<str>,
    pub normalized_link_path: Arc<str>,
}

#[derive(Clone, Debug)]
pub struct CompilerExecutionPlan {
    pub fixture: Arc<CompilerFixtureInput>,
    pub variant: CompilerVariant,
    /// Object-spread result in JavaScript property order. Exact-case duplicate
    /// keys replace their value without moving their original position.
    pub effective_settings: Arc<[OrderedSetting]>,
    pub current_directory: Arc<str>,
    pub use_case_sensitive_file_names: bool,
    pub root_selection: CompilerRootSelection,
}

#[derive(Clone, Debug)]
pub struct CompilerVariant {
    pub configuration_index: u32,
    pub key: Arc<str>,
    pub description: Arc<str>,
    pub upstream_name: Arc<str>,
    pub overrides: Arc<[OrderedSetting]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompilerExplicitRootReason {
    AllUnits,
    LastUnitImplicitReferences,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompilerRootSelection {
    Explicit {
        reason: CompilerExplicitRootReason,
        root_units: Arc<[CompilerUnitId]>,
        other_units: Arc<[CompilerUnitId]>,
        vfs_write_order: Arc<[CompilerUnitId]>,
        /// Roots with TypeScript's exact lowercase `.json` extension removed
        /// at the final `createProgram` argument filtering step.
        program_root_units: Arc<[CompilerUnitId]>,
    },
    Config {
        config_unit: CompilerUnitId,
        /// The config parse host sees every original occurrence, including the
        /// config unit itself.
        config_host_units: Arc<[CompilerUnitId]>,
        /// Stable membership partition in original unit occurrence order,
        /// deliberately not `ParsedCommandLine.fileNames` order.
        root_units: Arc<[CompilerUnitId]>,
        other_units: Arc<[CompilerUnitId]>,
        vfs_write_order: Arc<[CompilerUnitId]>,
        program_root_units: Arc<[CompilerUnitId]>,
    },
}

/// Units, tsconfig root plan and links of one compiler-runner fixture.
fn compiler_fixture_from_parts(
    source: Arc<VerifiedSource>,
    settings: Vec<OrderedSetting>,
    parsed_units: Vec<super::compiler::ParsedUnit>,
    config_offset: Option<usize>,
    links: Vec<super::CompilerLink>,
) -> HarnessResult<CompilerFixtureInput> {
    let current_directory = compiler_current_directory(&settings)?;
    let original_fixture_path = Arc::clone(&source.upstream_path);
    let units = parsed_units
        .into_iter()
        .enumerate()
        .map(|(index, unit)| {
            build_compiler_unit(
                CompilerUnitId(index as u32),
                unit,
                Arc::clone(&original_fixture_path),
                &current_directory,
            )
        })
        .collect::<HarnessResult<Vec<_>>>()?;
    let units: Arc<[CompilerUnitInput]> = Arc::from(units);
    let config_unit = config_offset.map(|index| CompilerUnitId(index as u32));
    let (config_root_plan, config_host_log) = match config_unit {
        Some(config_unit) => {
            let unit = units.get(config_unit.0 as usize).ok_or_else(|| {
                error(format!(
                    "compiler config unit for {:?} is out of bounds",
                    source.relative_path
                ))
            })?;
            let text = unit.content.as_deref().ok_or_else(|| {
                error(format!(
                    "compiler config unit for {:?} has missing content",
                    source.relative_path
                ))
            })?;
            let host = CompilerFixtureConfigHost::new(&units);
            let parsed = parse_config_root_plan(
                &host,
                ConfigRootPlanRequest {
                    file_name: unit.name.as_ref().into(),
                    text: text.to_owned(),
                    base_path: VIRTUAL_SOURCE_ROOT.into(),
                },
            );
            let config_host_log = Arc::from(host.into_log()?);
            let config_root_plan = parsed
                .map_err(|parse_error| {
                    error(format!(
                        "failed to plan compiler config for {:?}: {parse_error}",
                        source.relative_path
                    ))
                })?
                .into();
            (Some(config_root_plan), config_host_log)
        }
        None => (None, Arc::from([])),
    };
    let global_symlink_directives = links
        .into_iter()
        .map(|link| {
            let anchor: Arc<str> = Arc::from(VIRTUAL_SOURCE_ROOT);
            Ok(CompilerSymlinkOperation {
                phase: CompilerSymlinkPhase::Global,
                raw_target: Arc::from(link.target.as_str()),
                raw_link_path: Arc::from(link.link_path.as_str()),
                normalized_target: Arc::from(normalize_compiler_fixture_path(
                    anchor.as_ref(),
                    &link.target,
                )?),
                normalized_link_path: Arc::from(normalize_compiler_fixture_path(
                    anchor.as_ref(),
                    &link.link_path,
                )?),
                anchor,
            })
        })
        .collect::<HarnessResult<Vec<_>>>()?;
    let global_symlinks = effective_global_symlinks(&global_symlink_directives);

    Ok(CompilerFixtureInput {
        source,
        units,
        config_unit,
        config_root_plan,
        config_host_log,
        settings: Arc::from(settings),
        global_symlink_directives: Arc::from(global_symlink_directives),
        global_symlinks: Arc::from(global_symlinks),
    })
}

fn effective_global_symlinks(
    directives: &[CompilerSymlinkOperation],
) -> Vec<CompilerSymlinkOperation> {
    let mut effective: Vec<CompilerSymlinkOperation> = Vec::with_capacity(directives.len());
    for directive in directives {
        if let Some(existing) = effective
            .iter_mut()
            .find(|existing| existing.normalized_link_path == directive.normalized_link_path)
        {
            existing.clone_from(directive);
        } else {
            effective.push(directive.clone());
        }
    }
    effective
}

fn build_compiler_unit(
    id: CompilerUnitId,
    unit: ParsedUnit,
    original_fixture_path: Arc<str>,
    current_directory: &str,
) -> HarnessResult<CompilerUnitInput> {
    let name: Arc<str> = Arc::from(unit.name);
    let document_symlinks = exact_setting(&unit.file_options, "symlink")
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .split(',')
                .map(js_trim)
                .map(|link_path| {
                    let anchor: Arc<str> = Arc::from(current_directory);
                    Ok(CompilerSymlinkOperation {
                        phase: CompilerSymlinkPhase::Document,
                        raw_target: Arc::clone(&name),
                        raw_link_path: Arc::from(link_path),
                        normalized_target: Arc::from(normalize_compiler_fixture_path(
                            current_directory,
                            name.as_ref(),
                        )?),
                        normalized_link_path: Arc::from(normalize_compiler_fixture_path(
                            current_directory,
                            link_path,
                        )?),
                        anchor,
                    })
                })
                .collect::<HarnessResult<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(CompilerUnitInput {
        id,
        name,
        content: unit.content.map(Arc::from),
        file_options: Arc::from(unit.file_options),
        original_fixture_path,
        references: Arc::from([]),
        document_symlinks: Arc::from(document_symlinks),
    })
}

/// Adapter for `harnessIO.makeUnitsFromTest`'s fixed compiler-fixture units.
/// Config parsing is always case-insensitive and observes every original unit,
/// including the selected config occurrence. `fileExists`/`readFile` compare
/// raw unit spellings, while `readDirectory` normalizes each occurrence under
/// `/.src` before wildcard matching.
struct CompilerFixtureConfigHost<'a> {
    units: &'a [CompilerUnitInput],
    log: RefCell<Vec<tsc_program::JsonValue>>,
}

impl<'a> CompilerFixtureConfigHost<'a> {
    fn new(units: &'a [CompilerUnitInput]) -> Self {
        Self {
            units,
            log: RefCell::new(Vec::new()),
        }
    }

    fn into_log(self) -> HarnessResult<Vec<Value>> {
        // The frozen corpus observer uses serde values. Preserve the complete
        // JS trace through host execution; reject an unrepresentable observer
        // result explicitly rather than projecting any query or lookup key.
        self.log
            .into_inner()
            .iter()
            .map(js_paths::scalar_json_observation)
            .collect()
    }
}

impl ConfigParseHost for CompilerFixtureConfigHost<'_> {
    fn use_case_sensitive_file_names(&self) -> bool {
        false
    }

    fn file_exists(&self, path: JsStr<'_>) -> Result<bool, ConfigHostError> {
        let key = tsc_host::to_file_name_lower_case_js(path);
        let result = self
            .units
            .iter()
            .any(|unit| tsc_host::to_file_name_lower_case_js(unit.name.as_ref().into()) == key);
        self.log.borrow_mut().push(js_paths::log_entry([
            ("operation", js_paths::json_string("file_exists".into())),
            ("path", js_paths::json_string(path)),
            ("result", tsc_program::JsonValue::Bool(result)),
        ]));
        Ok(result)
    }

    fn read_file(&self, path: JsStr<'_>) -> Result<Option<String>, ConfigHostError> {
        let key = tsc_host::to_file_name_lower_case_js(path);
        let result = self.units.iter().find_map(|unit| {
            (tsc_host::to_file_name_lower_case_js(unit.name.as_ref().into()) == key)
                .then(|| unit.content.as_deref().map(str::to_owned))
                .flatten()
        });
        self.log.borrow_mut().push(js_paths::log_entry([
            ("operation", js_paths::json_string("read_file".into())),
            ("path", js_paths::json_string(path)),
            (
                "result",
                js_paths::json_string(if result.is_some() {
                    "text".into()
                } else {
                    "missing".into()
                }),
            ),
        ]));
        Ok(result)
    }

    fn read_directory(
        &self,
        directory: JsStr<'_>,
        extensions: &[&str],
        excludes: Option<&[JsString]>,
        includes: Option<&[JsString]>,
        depth: Option<usize>,
    ) -> Result<Vec<JsString>, ConfigHostError> {
        let includes = includes.unwrap_or(&[]);
        let include_patterns = includes
            .iter()
            .map(|include| {
                ConfigFilePattern::new(include, directory, /* case_sensitive */ false).map_err(
                    |detail| {
                        ConfigHostError::new(
                            ConfigHostOperation::ReadDirectory,
                            directory,
                            format!("invalid compiler-fixture include {include:?}: {detail}"),
                        )
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut buckets = vec![Vec::new(); includes.len().max(1)];
        let mut visited = HashSet::new();
        for base_path in js_paths::base_paths(directory, includes) {
            js_paths::visit_directory(
                self.units,
                directory,
                base_path.as_js(),
                extensions,
                excludes,
                includes,
                &include_patterns,
                depth,
                &mut visited,
                &mut buckets,
            )?;
        }
        let result = buckets.into_iter().flatten().collect::<Vec<_>>();
        self.log.borrow_mut().push(js_paths::log_entry([
            ("operation", js_paths::json_string("read_directory".into())),
            ("directory", js_paths::json_string(directory)),
            ("extensions", json!(extensions).into()),
            (
                "excludes",
                excludes.map_or(tsc_program::JsonValue::Null, js_paths::json_strings),
            ),
            ("includes", js_paths::json_strings(includes)),
            ("depth", json!(depth).into()),
            ("result", js_paths::json_strings(&result)),
        ]));
        Ok(result)
    }
}

fn join_config_path(directory: &str, name: &str) -> String {
    if directory.ends_with('/') {
        format!("{directory}{name}")
    } else {
        format!("{directory}/{name}")
    }
}

fn normalize_compiler_unit_path(path: &str) -> HarnessResult<String> {
    normalize_compiler_fixture_path(VIRTUAL_SOURCE_ROOT, path)
}

/// Resolve one compiler-runner fixture spelling with TypeScript's rooted-path
/// policy. Unlike the project-suite virtual namespace, compiler fixtures may
/// name files on a synthetic Windows drive even when their current directory
/// is the POSIX `/.src` mount.
///
/// tsc-port: getNormalizedAbsolutePath @6.0.3
/// tsc-hash: b61f74b787ba34aece216809c77bbf6f46565bc1f0a0af082110aacbe0bf9b0c
/// tsc-span: _tsc.js:5493-5567
/// tsc-port: simpleNormalizePath @6.0.3
/// tsc-hash: 1b1c1e16f323aede30aef78eaa9ab10df07777d696b878d6bac40df2f7515ac7
/// tsc-span: _tsc.js:5577-5592
fn normalize_compiler_fixture_path(base: &str, path: &str) -> HarnessResult<String> {
    let path = path.replace('\\', "/");
    let combined = if compiler_fixture_root_parts(&path).is_some() {
        path
    } else {
        join_config_path(base, &path)
    };
    normalize_compiler_rooted_path(&combined)
}

fn normalize_compiler_rooted_path(path: &str) -> HarnessResult<String> {
    if path.contains('\0') {
        return Err(error(format!(
            "compiler virtual path contains NUL: {path:?}"
        )));
    }
    let Some((root, tail)) = compiler_fixture_root_parts(path) else {
        return Err(error(format!(
            "compiler virtual path is not rooted: {path:?}"
        )));
    };
    let mut components = Vec::new();
    for component in tail.split('/') {
        match component {
            "" | "." => {}
            ".." => {
                components.pop();
            }
            component => components.push(component),
        }
    }
    if components.is_empty() {
        return Ok(root.to_owned());
    }
    Ok(format!("{root}{}", components.join("/")))
}

/// Split the roots admitted by TypeScript's `getEncodedRootLength` for the
/// compiler-runner's disk-path namespace. Keeping this lexical avoids making
/// a synthetic Windows drive or UNC share depend on the host OS running the
/// Rust harness.
fn compiler_fixture_root_parts(path: &str) -> Option<(&str, &str)> {
    if let Some(server_and_tail) = path.strip_prefix("//") {
        return match server_and_tail.find('/') {
            Some(separator) => {
                let root_end = 2 + separator + 1;
                Some((&path[..root_end], &path[root_end..]))
            }
            None => Some((path, "")),
        };
    }
    if let Some(tail) = path.strip_prefix('/') {
        return Some(("/", tail));
    }
    let bytes = path.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':') {
        if bytes.get(2) == Some(&b'/') {
            return Some((&path[..3], &path[3..]));
        }
        if bytes.len() == 2 {
            return Some((path, ""));
        }
    }
    None
}

fn compiler_root_selection(
    fixture: &CompilerFixtureInput,
    settings: &[OrderedSetting],
) -> HarnessResult<CompilerRootSelection> {
    let candidates = fixture
        .units
        .iter()
        .filter(|unit| Some(unit.id) != fixture.config_unit)
        .map(|unit| unit.id)
        .collect::<Vec<_>>();
    if let Some(config_unit) = fixture.config_unit {
        let config_plan = fixture.config_root_plan.as_ref().ok_or_else(|| {
            error(format!(
                "compiler source index {} has a config unit but no config root plan",
                fixture.source.index
            ))
        })?;
        let mut root_units = Vec::new();
        let mut other_units = Vec::new();
        for id in candidates {
            let unit = fixture
                .units
                .get(id.0 as usize)
                .ok_or_else(|| error("compiler config candidate unit is out of bounds"))?;
            let normalized = normalize_compiler_unit_path(unit.name.as_ref())?;
            if config_plan
                .file_names()
                .iter()
                .any(|file_name| file_name.as_js() == normalized.as_str())
            {
                root_units.push(id);
            } else {
                other_units.push(id);
            }
        }
        let vfs_write_order = root_units
            .iter()
            .chain(&other_units)
            .copied()
            .collect::<Vec<_>>();
        let program_root_units = json_filtered_roots(fixture, &root_units);
        return Ok(CompilerRootSelection::Config {
            config_unit,
            config_host_units: Arc::from(
                fixture.units.iter().map(|unit| unit.id).collect::<Vec<_>>(),
            ),
            root_units: Arc::from(root_units),
            other_units: Arc::from(other_units),
            vfs_write_order: Arc::from(vfs_write_order),
            program_root_units: Arc::from(program_root_units),
        });
    }
    let last = candidates
        .last()
        .copied()
        .ok_or_else(|| error("compiler fixture has no normal unit"))?;
    let last_unit = fixture
        .units
        .get(last.0 as usize)
        .ok_or_else(|| error("compiler fixture last unit is out of bounds"))?;
    let last_content = last_unit.content.as_deref().unwrap_or_default();
    let implicit_references = exact_setting(settings, "noImplicitReferences")
        .is_some_and(|value| !value.is_empty())
        || last_content.contains("require(")
        || contains_reference_path(last_content);
    if implicit_references {
        let other_units = candidates
            .into_iter()
            .filter(|id| {
                fixture
                    .units
                    .get(id.0 as usize)
                    .is_some_and(|unit| unit.name != last_unit.name)
            })
            .collect::<Vec<_>>();
        Ok(explicit_compiler_roots(
            fixture,
            CompilerExplicitRootReason::LastUnitImplicitReferences,
            vec![last],
            other_units,
        ))
    } else {
        Ok(explicit_compiler_roots(
            fixture,
            CompilerExplicitRootReason::AllUnits,
            candidates,
            Vec::new(),
        ))
    }
}

fn explicit_compiler_roots(
    fixture: &CompilerFixtureInput,
    reason: CompilerExplicitRootReason,
    root_units: Vec<CompilerUnitId>,
    other_units: Vec<CompilerUnitId>,
) -> CompilerRootSelection {
    let vfs_write_order = root_units
        .iter()
        .chain(&other_units)
        .copied()
        .collect::<Vec<_>>();
    let program_root_units = supported_compiler_roots(fixture, &root_units);
    CompilerRootSelection::Explicit {
        reason,
        root_units: Arc::from(root_units),
        other_units: Arc::from(other_units),
        vfs_write_order: Arc::from(vfs_write_order),
        program_root_units: Arc::from(program_root_units),
    }
}

/// The native runner's root list (`harnessutil.CompileFilesEx`): every unit
/// to be compiled except `.json` and `.tsbuildinfo` files becomes a program
/// root, whatever its extension, so a JavaScript root without `allowJs` or a
/// root with an unsupported extension reaches the program and reports
/// TS6504/TS6054 there (tsc's harness filtered them out first). The
/// comparison is deliberately case-sensitive, matching `fileExtensionIs`.
fn supported_compiler_roots(
    fixture: &CompilerFixtureInput,
    root_units: &[CompilerUnitId],
) -> Vec<CompilerUnitId> {
    root_units
        .iter()
        .copied()
        .filter(|id| {
            fixture.units.get(id.0 as usize).is_some_and(|unit| {
                let name = unit.name.as_ref();
                !file_extension_is(name, ".json") && !file_extension_is(name, ".tsbuildinfo")
            })
        })
        .collect()
}

fn file_extension_is(path: &str, extension: &str) -> bool {
    path.len() > extension.len() && path.ends_with(extension)
}

fn json_filtered_roots(
    fixture: &CompilerFixtureInput,
    root_units: &[CompilerUnitId],
) -> Vec<CompilerUnitId> {
    root_units
        .iter()
        .copied()
        .filter(|id| {
            fixture.units.get(id.0 as usize).is_some_and(|unit| {
                unit.name.len() <= ".json".len() || !unit.name.ends_with(".json")
            })
        })
        .collect()
}

fn compiler_current_directory(settings: &[OrderedSetting]) -> HarnessResult<String> {
    exact_setting(settings, "currentDirectory")
        .map(|value| normalize_virtual_path(VIRTUAL_SOURCE_ROOT, value))
        .transpose()
        .map(|value| value.unwrap_or_else(|| VIRTUAL_SOURCE_ROOT.to_owned()))
}

fn compiler_case_sensitivity(settings: &[OrderedSetting]) -> bool {
    settings
        .iter()
        .filter(|setting| {
            setting
                .name
                .eq_ignore_ascii_case("useCaseSensitiveFileNames")
        })
        .map(|setting| setting.value.eq_ignore_ascii_case("true"))
        .next_back()
        .unwrap_or(true)
}

fn exact_setting<'a>(settings: &'a [OrderedSetting], name: &str) -> Option<&'a str> {
    settings
        .iter()
        .find(|setting| setting.name == name)
        .map(|setting| setting.value.as_str())
}

fn contains_reference_path(text: &str) -> bool {
    text.match_indices("reference").any(|(index, _)| {
        let suffix = &text[index + "reference".len()..];
        let mut chars = suffix.chars();
        chars.next().is_some_and(is_js_whitespace) && chars.as_str().starts_with("path")
    })
}

fn is_js_whitespace(ch: char) -> bool {
    matches!(
        ch,
        '\u{0009}'
            | '\u{000a}'
            | '\u{000b}'
            | '\u{000c}'
            | '\u{000d}'
            | '\u{0020}'
            | '\u{00a0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn js_trim(value: &str) -> &str {
    value.trim_matches(is_js_whitespace)
}

fn normalize_virtual_path(base: &str, path: &str) -> HarnessResult<String> {
    let path = path.replace('\\', "/");
    let combined = if path.starts_with('/') {
        path
    } else if base.ends_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    };
    normalize_posix_path(&combined, true)
}

fn normalize_posix_path(path: &str, require_absolute: bool) -> HarnessResult<String> {
    if path.contains('\0') {
        return Err(error(format!("virtual path contains NUL: {path:?}")));
    }
    let path = path.replace('\\', "/");
    let absolute = path.starts_with('/');
    if require_absolute && !absolute {
        return Err(error(format!("virtual path is not absolute: {path:?}")));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            part => parts.push(part),
        }
    }
    let body = parts.join("/");
    if absolute {
        Ok(if body.is_empty() {
            "/".to_owned()
        } else {
            format!("/{body}")
        })
    } else {
        Ok(if body.is_empty() {
            ".".to_owned()
        } else {
            body
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/upstream_suites/execution_tests.rs"]
mod tests;
