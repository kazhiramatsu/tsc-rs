//! Lossless, deterministic execution inputs for the pinned TypeScript suites.
//!
//! The expansion manifest records the complete case inventory without copying
//! source text into a large JSON artifact. This module joins that inventory
//! back to the pinned corpus. Source bytes are verified for every recorded
//! path, decoded once per Git blob, and shared by every matrix variant.
//!
//! Compiler config files are parsed once through the program-owned H0.5 root
//! planner. The harness supplies the fixed compiler corpus's `harnessIO`
//! virtual-host observations, then retains TypeScript's original-unit stable
//! membership partition instead of substituting `ParsedCommandLine.fileNames`
//! order. This adapter is not the future general filesystem `matchFiles` host.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{json, Value};
use tsc_host::{
    to_file_name_lower_case, CompilerHost, FsCompilerHost, HostError, MemoryCompilerHost,
};
use tsc_program::{
    load_emitting_program, load_program, parse_config_root_plan, CompilerConfigHost,
    CompilerOptionNumber, CompilerOptions, ConfigFilePattern, ConfigHostError, ConfigHostOperation,
    ConfigOptionValueState, ConfigParseHost, ConfigRootPlan, ConfigRootPlanRequest, LibraryCatalog,
    ModuleSuffix, PreparedProgram, ProgramLoadLimits, ProgramOptions, ProgramPath,
};

use super::compiler::{
    expand_configurations, extract_compiler_settings, is_config_file_name, make_units_from_test,
    ParsedUnit,
};
use super::{
    collect_suite_paths, decode_source, error, git_blob_sha1, path_from_posix,
    read_recorded_manifest, sha256_hex, CaseConfiguration, CompilerFixtureExpansion,
    ExecutionState, ExpansionManifest, OrderedSetting, ProjectInputFiles, ProjectModule,
    SourceEncoding, SourceInventoryEntry, SuiteName, UnitContent, VIRTUAL_SOURCE_ROOT,
};
use crate::HarnessResult;

mod project;

pub use project::{
    load_node_modules_search_project, load_project_emit, load_project_emit_with_option_floor,
    load_project_no_emit, ProjectConfigProgram, ProjectNoEmitProgram,
};

/// Build the same bounded no-emit [`PreparedProgram`] that the compiler
/// runner would hand to `createProgram` for one recorded compiler case.
///
/// The upstream compiler runner normally continues into emit and baseline
/// comparison. H0 owns the source/config/loader boundary only, so this
/// adapter deliberately stops at the owned program and keeps that distinction
/// explicit in its name and return type.
pub fn load_compiler_no_emit(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    limits: ProgramLoadLimits,
) -> HarnessResult<PreparedProgram> {
    load_compiler_program(
        workspace,
        plan,
        limits,
        CompilerProgramMode::NoEmit,
        EmitOptionFloor::Established,
    )
}

/// Build the profile-admitted emitting [`PreparedProgram`] for one pinned
/// compiler-runner case.
///
/// The caller still owns classification and exact output comparison. This
/// adapter only reconstructs the same verified VFS, root order, and effective
/// options as [`load_compiler_no_emit`] before selecting the distinct emitting
/// loader.
/// Which map-family options the fixture-settings projection admits.
///
/// The established H2.5g/H2.5h acceptance floor DROPS the map family (both
/// sides of those frozen bands are mapless); the H2.6a band projects only
/// `sourceMap` (h2-6a-ca-2 — the ca-1 oracle observed WITH maps), and the
/// H2.6b band projects the complete map family. Every other entry point keeps
/// its existing floor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EmitOptionFloor {
    Established,
    SourceMap,
    MapFamily,
    DeclarationFamily,
}

pub fn load_compiler_emit(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    limits: ProgramLoadLimits,
) -> HarnessResult<PreparedProgram> {
    load_compiler_program(
        workspace,
        plan,
        limits,
        CompilerProgramMode::Emit,
        EmitOptionFloor::Established,
    )
}

/// [`load_compiler_emit`] with an explicit settings floor (the H2.6a/H2.6b
/// acceptance lanes).
pub fn load_compiler_emit_with_option_floor(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    limits: ProgramLoadLimits,
    floor: EmitOptionFloor,
) -> HarnessResult<PreparedProgram> {
    load_compiler_program(workspace, plan, limits, CompilerProgramMode::Emit, floor)
}

/// Reconstruct one qualification-owned compiler-runner VFS without depending
/// on which pinned suite originally supplied the fixture.
///
/// H2 source-reachability evidence carries the exact merged harness settings,
/// root order, current directory, and verified virtual bytes. This adapter
/// applies the same option projection and read-only TypeScript library mount
/// as [`load_compiler_emit`]. Config discovery, links, and symlinks are
/// deliberately absent from this protocol; a slice must first disposition
/// those host behaviors before using a richer execution route.
pub fn load_qualified_compiler_emit(
    workspace: &Path,
    current_directory: &str,
    files: &[(PathBuf, Vec<u8>)],
    roots: &[PathBuf],
    settings: &[(String, String)],
    limits: ProgramLoadLimits,
) -> HarnessResult<PreparedProgram> {
    load_qualified_compiler_emit_with_option_floor(
        workspace,
        current_directory,
        files,
        roots,
        settings,
        limits,
        EmitOptionFloor::Established,
    )
}

/// [`load_qualified_compiler_emit`] with an explicit settings floor (the
/// H2.6a/H2.6b acceptance lanes).
#[allow(clippy::too_many_arguments)]
pub fn load_qualified_compiler_emit_with_option_floor(
    workspace: &Path,
    current_directory: &str,
    files: &[(PathBuf, Vec<u8>)],
    roots: &[PathBuf],
    settings: &[(String, String)],
    limits: ProgramLoadLimits,
    floor: EmitOptionFloor,
) -> HarnessResult<PreparedProgram> {
    if files.is_empty() || roots.is_empty() {
        return Err(error(
            "qualified compiler input must contain files and roots",
        ));
    }
    let mut host_builder = MemoryCompilerHost::builder(current_directory).case_sensitive(true);
    let mut unique_paths = HashSet::with_capacity(files.len());
    for (file_name, bytes) in files {
        if !file_name.is_absolute() || !unique_paths.insert(file_name.clone()) {
            return Err(error(format!(
                "qualified compiler input has invalid or duplicate file {file_name:?}"
            )));
        }
        host_builder = host_builder.file(file_name, bytes.clone());
    }
    for root in roots {
        if !root.is_absolute() || !unique_paths.contains(root) {
            return Err(error(format!(
                "qualified compiler root is absent from the VFS: {root:?}"
            )));
        }
    }
    for directory in
        compiler_vfs_trailing_directory_aliases(unique_paths.iter().map(PathBuf::as_path))?
    {
        host_builder = host_builder.directory(directory);
    }
    let fixture_host = host_builder.build().map_err(|host_error| {
        error(format!(
            "failed to build qualified compiler fixture host: {host_error}"
        ))
    })?;
    let virtual_config_paths = files
        .iter()
        .filter_map(|(path, _)| {
            path.to_str()
                .filter(|path| is_config_file_name(path))
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    if virtual_config_paths.len() > 1 {
        return Err(error(format!(
            "qualified compiler input has multiple virtual config files: {virtual_config_paths:?}"
        )));
    }
    let config_root_plan = virtual_config_paths
        .first()
        .map(|config_path| {
            let config_host = CompilerConfigHost::new(&fixture_host);
            let text = config_host
                .read_file(config_path)
                .map_err(|parse_error| {
                    error(format!(
                        "failed to read qualified compiler virtual config {config_path:?}: {parse_error}"
                    ))
                })?
                .ok_or_else(|| {
                    error(format!(
                        "qualified compiler virtual config is absent from the VFS: {config_path:?}"
                    ))
                })?;
            parse_config_root_plan(
                &config_host,
                ConfigRootPlanRequest {
                    file_name: config_path.clone(),
                    text,
                    base_path: current_directory.to_owned(),
                },
            )
            .map_err(|parse_error| {
                error(format!(
                    "failed to parse qualified compiler virtual config {config_path:?}: {parse_error}"
                ))
            })
        })
        .transpose()?;
    let library_directory = workspace.join("vendor/typescript-6.0.3/lib");
    let host = CompilerSuiteHost::new(workspace, fixture_host, library_directory.clone(), true)?;

    let config_has_explicit_allow_js = config_root_plan.as_ref().is_some_and(|config| {
        matches!(
            config.options().typed_value_state("allowJs"),
            ConfigOptionValueState::Value(value) if value.is_boolean()
        )
    });
    let (mut compiler_options, mut program_options) = config_root_plan
        .as_ref()
        .map(|config| {
            let mut compiler_options = config.compiler_options().clone();
            apply_emit_option_floor_to_config(&mut compiler_options, floor);
            (
                compiler_options,
                config
                    .program_options()
                    .clone()
                    .with_program_owned_config_option_diagnostics(),
            )
        })
        .unwrap_or_else(|| (CompilerOptions::default(), ProgramOptions::default()));
    // H2's qualification host starts from this harness default before
    // applying fixture settings. An explicit config or directive value wins.
    compiler_options.skip_default_lib_check.get_or_insert(true);
    apply_compiler_settings(
        &mut compiler_options,
        &mut program_options,
        current_directory,
        settings
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
        config_has_explicit_allow_js,
        floor,
    )?;
    if floor == EmitOptionFloor::DeclarationFamily {
        compiler_options.list_emitted_files = Some(true);
    }
    compiler_options.new_line.get_or_insert(0);
    compiler_options.no_error_truncation = Some(true);
    let catalog = LibraryCatalog::typescript_6_0_3(library_directory);
    load_emitting_program(
        &host,
        roots,
        compiler_options,
        program_options,
        &catalog,
        limits,
    )
    .map_err(|load_error| {
        error(format!(
            "failed to load qualified compiler fixture: {load_error}"
        ))
    })
}

/// Apply the same admission floor to options originating in a virtual config
/// that [`apply_compiler_setting`] applies to compiler-runner directives.
/// Config parsing remains responsible for typed conversion and path rebasing;
/// this projection only removes options that the selected floor still drops.
fn apply_emit_option_floor_to_config(options: &mut CompilerOptions, floor: EmitOptionFloor) {
    if !matches!(
        floor,
        EmitOptionFloor::SourceMap
            | EmitOptionFloor::MapFamily
            | EmitOptionFloor::DeclarationFamily
    ) {
        options.source_map = None;
    }
    if !matches!(
        floor,
        EmitOptionFloor::MapFamily | EmitOptionFloor::DeclarationFamily
    ) {
        options.inline_source_map = None;
        options.inline_sources = None;
        options.source_root = None;
        options.map_root = None;
        options.emit_bom = None;
    }
    if floor != EmitOptionFloor::DeclarationFamily {
        options.emit_declaration_only = None;
    }

    options.no_emit_helpers = None;
    options.declaration_map = None;
    options.out_file = None;
    options.out_dir = None;
    options.declaration_dir = None;
    options.incremental = None;
    options.assume_changes_only_affect_direct_dependencies = None;
    options.strip_internal = None;
    options.out = None;
    options.root_dir = None;
    options.ts_build_info_file = None;
    options.stable_type_ordering = None;
    options.no_check = None;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompilerProgramMode {
    NoEmit,
    Emit,
}

fn load_compiler_program(
    workspace: &Path,
    plan: &CompilerExecutionPlan,
    limits: ProgramLoadLimits,
    mode: CompilerProgramMode,
    floor: EmitOptionFloor,
) -> HarnessResult<PreparedProgram> {
    let current_directory = plan.current_directory.as_ref();
    let mut host_builder = MemoryCompilerHost::builder(current_directory)
        .case_sensitive(plan.use_case_sensitive_file_names);
    let mut source_paths = HashMap::<String, Arc<str>>::new();

    let vfs_write_order = match &plan.root_selection {
        CompilerRootSelection::Explicit {
            vfs_write_order, ..
        }
        | CompilerRootSelection::Config {
            vfs_write_order, ..
        } => vfs_write_order,
    };
    for unit_id in vfs_write_order.iter() {
        let unit = plan
            .fixture
            .units
            .get(unit_id.0 as usize)
            .ok_or_else(|| error("compiler VFS unit is out of bounds"))?;
        let path = normalize_compiler_fixture_path(current_directory, unit.name.as_ref())?;
        let Some(content) = unit.content.as_ref() else {
            continue;
        };
        host_builder = host_builder.file(&path, content.as_bytes().to_vec());
        source_paths.insert(path, Arc::clone(content));
    }

    // The compiler runner's VFS presents document/global symlinks through
    // `realpath`. MemoryCompilerHost requires both spellings to exist before
    // accepting that identity override, so publish a byte-identical alias
    // only for a link whose target has content.
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
    let mut realpath_overrides = Vec::new();
    for operation in symlink_operations {
        let Some(target_content) = source_paths.get(operation.normalized_target.as_ref()) else {
            continue;
        };
        if !source_paths.contains_key(operation.normalized_link_path.as_ref()) {
            host_builder = host_builder.file(
                operation.normalized_link_path.as_ref(),
                target_content.as_bytes().to_vec(),
            );
            source_paths.insert(
                operation.normalized_link_path.to_string(),
                Arc::clone(target_content),
            );
        }
        realpath_overrides.push((
            PathBuf::from(operation.normalized_link_path.as_ref()),
            PathBuf::from(operation.normalized_target.as_ref()),
        ));
    }
    for (link, target) in realpath_overrides {
        host_builder = host_builder.realpath(link, target);
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
    let library_directory = workspace.join("vendor/typescript-6.0.3/lib");
    let host = CompilerSuiteHost::new(
        workspace,
        fixture_host,
        library_directory.clone(),
        plan.use_case_sensitive_file_names,
    )?;

    let (mut compiler_options, program_options) = plan_compiler_options(plan, floor)?;
    // CompilerBaselineRunner normalizes these harness-only defaults
    // after config and directive projection. They are not command-line
    // defaults: in particular, its absent `newLine` becomes CRLF even when
    // the host running this adapter normally uses LF. Retain that distinction
    // here so an emitting compiler-suite plan observes the upstream baseline
    // bytes while the production CLI continues to use its own host default.
    compiler_options.new_line.get_or_insert(0);
    compiler_options.no_error_truncation = Some(true);
    if mode == CompilerProgramMode::NoEmit {
        compiler_options.no_emit = Some(true);
    }
    let roots = compiler_root_paths(plan)?;
    let catalog = LibraryCatalog::typescript_6_0_3(library_directory);
    let loaded = match mode {
        CompilerProgramMode::NoEmit => load_program(
            &host,
            &roots,
            compiler_options,
            program_options,
            &catalog,
            limits,
        ),
        CompilerProgramMode::Emit => load_emitting_program(
            &host,
            &roots,
            compiler_options,
            program_options,
            &catalog,
            limits,
        ),
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
        Ok(Self {
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
    floor: EmitOptionFloor,
) -> HarnessResult<(CompilerOptions, ProgramOptions)> {
    let (compiler_options, mut program_options) = project_compiler_options(
        &plan.fixture,
        &plan.effective_settings,
        plan.current_directory.as_ref(),
        floor,
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
    floor: EmitOptionFloor,
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
        floor,
    )?;
    if floor == EmitOptionFloor::DeclarationFamily {
        compiler_options.list_emitted_files = Some(true);
    }
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
    floor: EmitOptionFloor,
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
            floor,
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

/// Compiler-runner metadata that controls baseline comparison rather than a
/// [`CompilerOptions`] or [`ProgramOptions`] value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CompilerBaselineMetadata {
    SuppressOutputPathCheck,
}

impl CompilerBaselineMetadata {
    fn lookup(key: &CompilerFixtureOptionKey) -> Option<Self> {
        match key.as_str() {
            "suppressoutputpathcheck" => Some(Self::SuppressOutputPathCheck),
            _ => None,
        }
    }

    fn validate(self, raw_name: &str, value: &str) -> HarnessResult<()> {
        match self {
            Self::SuppressOutputPathCheck => {
                parse_compiler_bool(raw_name, value)?;
            }
        }
        Ok(())
    }
}

fn apply_compiler_setting(
    compiler_options: &mut CompilerOptions,
    program_options: &mut ProgramOptions,
    current_directory: &str,
    name: &str,
    value: &str,
    floor: EmitOptionFloor,
) -> HarnessResult<()> {
    let key = CompilerFixtureOptionKey::new(name);
    if let Some(metadata) = CompilerBaselineMetadata::lookup(&key) {
        return metadata.validate(name, value);
    }
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
                    .map(str::to_owned)
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
        "noimplicitusestrict" => compiler_options.no_implicit_use_strict = Some(boolean()?),
        "keyofstringsonly" => compiler_options.keyof_strings_only = Some(boolean()?),
        "suppressexcesspropertyerrors" => {
            compiler_options.suppress_excess_property_errors = Some(boolean()?)
        }
        "suppressimplicitanyindexerrors" => {
            compiler_options.suppress_implicit_any_index_errors = Some(boolean()?)
        }
        "nostrictgenericchecks" => compiler_options.no_strict_generic_checks = Some(boolean()?),
        "preservevalueimports" => compiler_options.preserve_value_imports = Some(boolean()?),
        "importsnotusedasvalues" => {
            compiler_options.imports_not_used_as_values =
                Some(match value.to_ascii_lowercase().as_str() {
                    "remove" => 0,
                    "preserve" => 1,
                    "error" => 2,
                    _ => value.parse::<i32>().map_err(|_| {
                        error(format!(
                            "compiler option importsNotUsedAsValues has invalid value {value:?}"
                        ))
                    })?,
                })
        }
        "charset" => compiler_options.charset = Some(value.to_owned()),
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
                    .map(|entry| ModuleSuffix::Value(entry.to_owned()))
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
                    .map(str::to_owned)
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
        "baseurl" => compiler_options.base_url = Some(value.to_owned()),
        "jsxfactory" => compiler_options.jsx_factory = Some(value.to_owned()),
        "jsxfragmentfactory" => compiler_options.jsx_fragment_factory = Some(value.to_owned()),
        "jsximportsource" => compiler_options.jsx_import_source = Some(value.to_owned()),
        "reactnamespace" => compiler_options.react_namespace = Some(value.to_owned()),
        "ignoredeprecations" => compiler_options.ignore_deprecations = Some(value.to_owned()),
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
        // `sourcemap` projects on the H2.6a and H2.6b floors and stays
        // dropped on the established floor (the frozen 5g/5h bands are
        // mapless on both sides).
        "sourcemap" => {
            if matches!(
                floor,
                EmitOptionFloor::SourceMap
                    | EmitOptionFloor::MapFamily
                    | EmitOptionFloor::DeclarationFamily
            ) {
                compiler_options.source_map = Some(boolean()?);
            }
        }
        "inlinesourcemap" => {
            if matches!(
                floor,
                EmitOptionFloor::MapFamily | EmitOptionFloor::DeclarationFamily
            ) {
                compiler_options.inline_source_map = Some(boolean()?);
            }
        }
        "inlinesources" => {
            if matches!(
                floor,
                EmitOptionFloor::MapFamily | EmitOptionFloor::DeclarationFamily
            ) {
                compiler_options.inline_sources = Some(boolean()?);
            }
        }
        "sourceroot" => {
            if matches!(
                floor,
                EmitOptionFloor::MapFamily | EmitOptionFloor::DeclarationFamily
            ) {
                compiler_options.source_root = Some(value.to_owned());
            }
        }
        "maproot" => {
            if matches!(
                floor,
                EmitOptionFloor::MapFamily | EmitOptionFloor::DeclarationFamily
            ) {
                compiler_options.map_root = Some(value.to_owned());
            }
        }
        // W5 K21: the production emitter honors emitBOM on the JavaScript
        // write's byte-order-mark flag (execute.rs). The breadth floor
        // projects it so the observation's BOM facet is comparable; the
        // 5g/5h/6a floors keep the historical drop.
        "emitbom" => {
            if matches!(
                floor,
                EmitOptionFloor::MapFamily | EmitOptionFloor::DeclarationFamily
            ) {
                compiler_options.emit_bom = Some(boolean()?);
            }
        }
        "emitdeclarationonly" => {
            if floor == EmitOptionFloor::DeclarationFamily {
                compiler_options.emit_declaration_only = Some(boolean()?);
            }
        }
        "noemithelpers"
        | "declarationmap"
        | "outdir"
        | "declarationdir"
        | "incremental"
        | "assumechangesonlyaffectdirectdependencies"
        | "stripinternal"
        | "disablesizelimit"
        | "out"
        | "outfile"
        | "rootdir"
        | "tsbuildinfofile"
        | "pretty"
        | "traceresolution"
        | "listfilesonly"
        | "capturesuggestions"
        | "fullemitpaths"
        | "typescriptversion"
        | "stabletypeordering"
        | "notypesandsymbols"
        | "noimplicitreferences"
        | "nocheck"
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

/// A fully verified, canonically ordered set of execution plans.
#[derive(Clone, Debug)]
pub struct UpstreamExecutionCorpus {
    pub manifest: Arc<ExpansionManifest>,
    pub plans: Arc<[UpstreamExecutionPlan]>,
    pub cache_stats: SourceCacheStats,
}

/// One entry in the recorded 7,908-case execution order.
#[derive(Clone, Debug)]
pub struct UpstreamExecutionPlan {
    pub provenance: CaseProvenance,
    pub input: UpstreamExecutionInput,
}

#[derive(Clone, Debug)]
pub enum UpstreamExecutionInput {
    Compiler(CompilerExecutionPlan),
    Project(ProjectExecutionPlan),
}

#[derive(Clone, Debug)]
pub struct CaseProvenance {
    /// Stable manifest offset for deterministic sharding and result assembly.
    pub case_index: u32,
    pub case_id: Arc<str>,
    pub suite: SuiteName,
    pub source_index: u32,
    pub source_path: Arc<str>,
    pub upstream_path: Arc<str>,
    pub git_blob_sha1: Arc<str>,
    pub source_commit: &'static str,
    pub initial_execution_state: ExecutionState,
}

/// Content and identity for one source path in the pinned corpus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSource {
    pub index: u32,
    pub suite: SuiteName,
    pub relative_path: Arc<str>,
    pub upstream_path: Arc<str>,
    pub workspace_path: Arc<PathBuf>,
    pub git_blob_sha1: Arc<str>,
    pub raw: Arc<[u8]>,
    pub encoding: SourceEncoding,
    pub decoded: Arc<str>,
}

/// Observable work avoided by the blob-keyed source cache.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceCacheStats {
    pub verified_source_paths: usize,
    pub verified_source_bytes: u64,
    pub unique_raw_blobs: usize,
    pub reused_raw_blobs: usize,
    pub decode_requests: usize,
    pub unique_decoded_blobs: usize,
    pub reused_decoded_blobs: usize,
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
    pub configurations: Arc<[super::CompilerConfiguration]>,
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

#[derive(Clone, Debug)]
pub struct ProjectFixtureInput {
    pub source: Arc<VerifiedSource>,
    /// Raw source is retained because it is the ultimate lossless descriptor.
    pub descriptor_raw: Arc<[u8]>,
    pub descriptor_text: Arc<str>,
    /// Top-level JavaScript property order, including case-distinct keys.
    pub properties: Arc<[OrderedJsonProperty]>,
    pub scenario: Arc<str>,
    pub project_root: Arc<str>,
    pub input_files: ProjectInputFiles,
    pub current_directory: Arc<str>,
    pub mount: Arc<ProjectMount>,
    pub root_selection: ProjectRootSelection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OrderedJsonProperty {
    pub name: Arc<str>,
    pub value: Value,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectMount {
    pub workspace_path: Arc<PathBuf>,
    pub virtual_path: Arc<str>,
    pub case_sensitive: bool,
    pub read_only: bool,
    /// Every file below the pinned `tests/cases/projects` tree. The source
    /// cache verifies each path and decodes each distinct Git blob once before
    /// the mount is published; all project matrix variants share these owners.
    pub files: Arc<[ProjectMountFile]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectMountFile {
    pub source: Arc<VerifiedSource>,
    pub virtual_path: Arc<str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectRootSelection {
    Explicit {
        /// Raw descriptor order, spelling, and duplicates are retained.
        input_names: Arc<[Arc<str>]>,
    },
    ProjectConfig {
        raw_project: Arc<str>,
        config_file_name: Arc<str>,
        resolved_config_path: Arc<str>,
    },
    DiscoverConfig,
}

#[derive(Clone, Debug)]
pub struct ProjectExecutionPlan {
    pub fixture: Arc<ProjectFixtureInput>,
    /// Module variant identity remains separate from a descriptor-level module
    /// property, which the next option-projection stage may apply afterwards.
    pub module_variant: ProjectModule,
    pub baseline_folder: Arc<str>,
    pub descriptor_module_override: Option<Value>,
}

/// Verify the recorded corpus and construct every compiler/project plan in its
/// canonical manifest order.
pub fn load_recorded_execution_plans(workspace: &Path) -> HarnessResult<UpstreamExecutionCorpus> {
    let (manifest, _) = read_recorded_manifest(workspace)?;
    let manifest = Arc::new(manifest);
    let mut cache = SourceCache::load(workspace, &manifest)?;

    let mut compiler_fixtures = HashMap::with_capacity(manifest.compiler_fixtures.len());
    for fixture in &manifest.compiler_fixtures {
        let source = cache.decoded_source(&manifest, fixture.source)?;
        let input = Arc::new(build_compiler_fixture(fixture, source)?);
        if compiler_fixtures.insert(fixture.source, input).is_some() {
            return Err(error(format!(
                "duplicate compiler fixture source index {}",
                fixture.source
            )));
        }
    }

    let project_mount_files = manifest
        .sources
        .iter()
        .enumerate()
        .filter(|(_, source)| source.suite == SuiteName::Projects)
        .map(|(index, _)| {
            let source = cache.decoded_source(
                &manifest,
                u32::try_from(index).map_err(|_| error("project mount source index overflow"))?,
            )?;
            let virtual_path =
                join_posix("/.src/tests/cases/projects", source.relative_path.as_ref());
            Ok(ProjectMountFile {
                source,
                virtual_path: Arc::from(virtual_path),
            })
        })
        .collect::<HarnessResult<Vec<_>>>()?;
    let tests_mount = Arc::new(ProjectMount {
        workspace_path: Arc::new(workspace.join("ts-tests/tests")),
        virtual_path: Arc::from("/.src/tests"),
        case_sensitive: true,
        read_only: true,
        files: Arc::from(project_mount_files),
    });
    let mut project_fixtures = HashMap::with_capacity(manifest.project_fixtures.len());
    for fixture in &manifest.project_fixtures {
        let source = cache.decoded_source(&manifest, fixture.source)?;
        let input = Arc::new(build_project_fixture(
            fixture,
            source,
            Arc::clone(&tests_mount),
        )?);
        if project_fixtures.insert(fixture.source, input).is_some() {
            return Err(error(format!(
                "duplicate project fixture source index {}",
                fixture.source
            )));
        }
    }

    let mut plans = Vec::with_capacity(manifest.cases.len());
    for (case_index, case) in manifest.cases.iter().enumerate() {
        let input = match &case.configuration {
            CaseConfiguration::Compiler { configuration } => {
                let fixture = compiler_fixtures.get(&case.source).ok_or_else(|| {
                    error(format!("compiler case {:?} has no fixture input", case.id))
                })?;
                UpstreamExecutionInput::Compiler(build_compiler_plan(
                    Arc::clone(fixture),
                    *configuration,
                )?)
            }
            CaseConfiguration::Project {
                module,
                baseline_folder,
            } => {
                let fixture = project_fixtures.get(&case.source).ok_or_else(|| {
                    error(format!("project case {:?} has no fixture input", case.id))
                })?;
                UpstreamExecutionInput::Project(build_project_plan(
                    Arc::clone(fixture),
                    *module,
                    baseline_folder,
                ))
            }
        };
        let verified_source = match &input {
            UpstreamExecutionInput::Compiler(plan) => &plan.fixture.source,
            UpstreamExecutionInput::Project(plan) => &plan.fixture.source,
        };
        let provenance = CaseProvenance {
            case_index: u32::try_from(case_index)
                .map_err(|_| error("upstream execution case index overflow"))?,
            case_id: Arc::from(case.id.as_str()),
            suite: case.suite,
            source_index: case.source,
            source_path: Arc::clone(&verified_source.relative_path),
            upstream_path: Arc::clone(&verified_source.upstream_path),
            git_blob_sha1: Arc::clone(&verified_source.git_blob_sha1),
            source_commit: super::SOURCE_COMMIT,
            initial_execution_state: case.initial_execution_state,
        };
        plans.push(UpstreamExecutionPlan { provenance, input });
    }

    Ok(UpstreamExecutionCorpus {
        manifest,
        plans: Arc::from(plans),
        cache_stats: cache.stats,
    })
}

struct SourceCache {
    workspace_paths: Vec<Arc<PathBuf>>,
    raw_sources: Vec<Arc<[u8]>>,
    raw_by_blob: HashMap<String, Arc<[u8]>>,
    decoded_by_blob: HashMap<String, (SourceEncoding, Arc<str>)>,
    stats: SourceCacheStats,
}

impl SourceCache {
    fn load(workspace: &Path, manifest: &ExpansionManifest) -> HarnessResult<Self> {
        verify_suite_path_sets(workspace, manifest)?;

        let mut cache = Self {
            workspace_paths: Vec::with_capacity(manifest.sources.len()),
            raw_sources: Vec::with_capacity(manifest.sources.len()),
            raw_by_blob: HashMap::new(),
            decoded_by_blob: HashMap::new(),
            stats: SourceCacheStats::default(),
        };

        for source in &manifest.sources {
            let suite = suite_identity(manifest, source.suite)?;
            let path = workspace
                .join(&suite.vendored_path)
                .join(path_from_posix(&source.path)?);
            let raw = fs::read(&path).map_err(|source_error| {
                error(format!(
                    "failed to read pinned execution source {}: {source_error}",
                    path.display()
                ))
            })?;
            verify_source_bytes(source, &path, &raw)?;

            cache.stats.verified_source_paths += 1;
            cache.stats.verified_source_bytes = cache
                .stats
                .verified_source_bytes
                .checked_add(raw.len() as u64)
                .ok_or_else(|| error("verified source byte count overflow"))?;

            let shared = if let Some(existing) = cache.raw_by_blob.get(&source.git_blob_sha1) {
                if existing.as_ref() != raw.as_slice() {
                    return Err(error(format!(
                        "Git blob collision while loading {}",
                        path.display()
                    )));
                }
                cache.stats.reused_raw_blobs += 1;
                Arc::clone(existing)
            } else {
                let shared: Arc<[u8]> = Arc::from(raw);
                cache
                    .raw_by_blob
                    .insert(source.git_blob_sha1.clone(), Arc::clone(&shared));
                shared
            };
            cache.workspace_paths.push(Arc::new(path));
            cache.raw_sources.push(shared);
        }
        cache.stats.unique_raw_blobs = cache.raw_by_blob.len();
        Ok(cache)
    }

    fn decoded_source(
        &mut self,
        manifest: &ExpansionManifest,
        index: u32,
    ) -> HarnessResult<Arc<VerifiedSource>> {
        let source = source_entry(manifest, index)?;
        let raw = self
            .raw_sources
            .get(index as usize)
            .cloned()
            .ok_or_else(|| error(format!("source index {index} was not loaded")))?;
        let workspace_path = self
            .workspace_paths
            .get(index as usize)
            .cloned()
            .ok_or_else(|| error(format!("source index {index} has no workspace path")))?;
        self.stats.decode_requests += 1;
        let (encoding, decoded) =
            if let Some((encoding, decoded)) = self.decoded_by_blob.get(&source.git_blob_sha1) {
                self.stats.reused_decoded_blobs += 1;
                (*encoding, Arc::clone(decoded))
            } else {
                let (encoding, decoded) = decode_source(raw.as_ref());
                let decoded: Arc<str> = Arc::from(decoded);
                self.decoded_by_blob.insert(
                    source.git_blob_sha1.clone(),
                    (encoding, Arc::clone(&decoded)),
                );
                (encoding, decoded)
            };
        self.stats.unique_decoded_blobs = self.decoded_by_blob.len();

        let suite = suite_identity(manifest, source.suite)?;
        Ok(Arc::new(VerifiedSource {
            index,
            suite: source.suite,
            relative_path: Arc::from(source.path.as_str()),
            upstream_path: Arc::from(join_posix(&suite.source_path, &source.path)),
            workspace_path,
            git_blob_sha1: Arc::from(source.git_blob_sha1.as_str()),
            raw,
            encoding,
            decoded,
        }))
    }
}

fn verify_suite_path_sets(workspace: &Path, manifest: &ExpansionManifest) -> HarnessResult<()> {
    for suite in &manifest.corpus_pin.suites {
        let root = workspace.join(&suite.vendored_path);
        let actual = collect_suite_paths(&root)?;
        let expected = manifest
            .sources
            .iter()
            .filter(|source| source.suite == suite.name)
            .map(|source| source.path.clone())
            .collect::<Vec<_>>();
        if actual != expected {
            return Err(error(format!(
                "execution corpus path set for {} no longer matches the recorded manifest",
                suite.name.as_str()
            )));
        }
    }
    Ok(())
}

fn verify_source_bytes(
    source: &SourceInventoryEntry,
    path: &Path,
    raw: &[u8],
) -> HarnessResult<()> {
    if raw.len() as u64 != source.bytes
        || sha256_hex(raw) != source.sha256
        || git_blob_sha1(raw) != source.git_blob_sha1
    {
        return Err(error(format!(
            "execution source {} does not match its recorded byte and blob identity",
            path.display()
        )));
    }
    Ok(())
}

fn build_compiler_fixture(
    recorded: &CompilerFixtureExpansion,
    source: Arc<VerifiedSource>,
) -> HarnessResult<CompilerFixtureInput> {
    if source.encoding != recorded.encoding
        || source.decoded.len() as u64 != recorded.decoded_utf8_bytes
        || sha256_hex(source.decoded.as_bytes()) != recorded.decoded_sha256
    {
        return Err(error(format!(
            "decoded compiler fixture {:?} does not match the manifest",
            source.relative_path
        )));
    }
    let settings = extract_compiler_settings(&source.decoded);
    if settings != recorded.settings {
        return Err(error(format!(
            "compiler settings for {:?} no longer match the manifest",
            source.relative_path
        )));
    }
    let configurations = expand_configurations(source.relative_path.as_ref(), &settings)?;
    if configurations != recorded.configurations {
        return Err(error(format!(
            "compiler configurations for {:?} no longer match the source settings",
            source.relative_path
        )));
    }
    let (parsed_units, links) =
        make_units_from_test(&source.decoded, source.upstream_path.as_ref())?;
    let config_offset = parsed_units
        .iter()
        .position(|unit| is_config_file_name(&unit.name));
    verify_parsed_units(recorded, &parsed_units, config_offset, &source)?;
    if links != recorded.links {
        return Err(error(format!(
            "compiler @link directives for {:?} no longer match the manifest",
            source.relative_path
        )));
    }

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
                    file_name: unit.name.to_string(),
                    text: text.to_owned(),
                    base_path: VIRTUAL_SOURCE_ROOT.to_owned(),
                },
            );
            let config_host_log = Arc::from(host.into_log());
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
        configurations: Arc::from(recorded.configurations.clone()),
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

fn verify_parsed_units(
    recorded: &CompilerFixtureExpansion,
    parsed: &[ParsedUnit],
    config_offset: Option<usize>,
    source: &VerifiedSource,
) -> HarnessResult<()> {
    let mut normal_offset = 0;
    for (index, unit) in parsed.iter().enumerate() {
        let expected = if Some(index) == config_offset {
            recorded.virtual_config.as_ref()
        } else {
            let expected = recorded.normal_units.get(normal_offset);
            normal_offset += 1;
            expected
        }
        .ok_or_else(|| {
            error(format!(
                "compiler unit occurrence {index} for {:?} is absent from the manifest",
                source.relative_path
            ))
        })?;
        if unit.name != expected.name || unit.file_options != expected.file_options {
            return Err(error(format!(
                "compiler unit occurrence {index} for {:?} no longer matches the manifest",
                source.relative_path
            )));
        }
        let actual_content = match &unit.content {
            Some(content) => UnitContent::Present {
                utf8_bytes: content.len() as u64,
                sha256: sha256_hex(content.as_bytes()),
            },
            None => UnitContent::Missing,
        };
        if actual_content != expected.content {
            return Err(error(format!(
                "compiler unit content {index} for {:?} no longer matches the manifest",
                source.relative_path
            )));
        }
        let symlinks = exact_setting(&unit.file_options, "symlink")
            .filter(|value| !value.is_empty())
            .map(|value| value.split(',').map(js_trim).collect::<Vec<_>>())
            .unwrap_or_default();
        if symlinks != expected.document_symlinks {
            return Err(error(format!(
                "compiler document symlinks {index} for {:?} no longer match the manifest",
                source.relative_path
            )));
        }
    }
    if normal_offset != recorded.normal_units.len()
        || config_offset.is_some() != recorded.virtual_config.is_some()
    {
        return Err(error(format!(
            "compiler unit partition for {:?} no longer matches the manifest",
            source.relative_path
        )));
    }
    Ok(())
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
    log: RefCell<Vec<Value>>,
}

impl<'a> CompilerFixtureConfigHost<'a> {
    fn new(units: &'a [CompilerUnitInput]) -> Self {
        Self {
            units,
            log: RefCell::new(Vec::new()),
        }
    }

    fn into_log(self) -> Vec<Value> {
        self.log.into_inner()
    }
}

impl ConfigParseHost for CompilerFixtureConfigHost<'_> {
    fn use_case_sensitive_file_names(&self) -> bool {
        false
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        let key = to_file_name_lower_case(path);
        let result = self
            .units
            .iter()
            .any(|unit| to_file_name_lower_case(unit.name.as_ref()) == key);
        self.log.borrow_mut().push(json!({
            "operation": "file_exists",
            "path": path,
            "result": result,
        }));
        Ok(result)
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        let key = to_file_name_lower_case(path);
        let result = self.units.iter().find_map(|unit| {
            (to_file_name_lower_case(unit.name.as_ref()) == key)
                .then(|| unit.content.as_deref().map(str::to_owned))
                .flatten()
        });
        self.log.borrow_mut().push(json!({
            "operation": "read_file",
            "path": path,
            "result": if result.is_some() { "text" } else { "missing" },
        }));
        Ok(result)
    }

    fn read_directory(
        &self,
        directory: &str,
        extensions: &[&str],
        excludes: Option<&[String]>,
        includes: Option<&[String]>,
        depth: Option<usize>,
    ) -> Result<Vec<String>, ConfigHostError> {
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
        for base_path in config_base_paths(directory, includes) {
            visit_config_directory(
                self.units,
                directory,
                &base_path,
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
        self.log.borrow_mut().push(json!({
            "operation": "read_directory",
            "directory": directory,
            "extensions": extensions,
            "excludes": excludes,
            "includes": includes,
            "depth": depth,
            "result": result,
        }));
        Ok(result)
    }
}

fn config_base_paths(directory: &str, includes: &[String]) -> Vec<String> {
    let mut include_bases = includes
        .iter()
        .map(|include| {
            let absolute = absolute_config_pattern(directory, include);
            let wildcard = absolute.find(['*', '?']);
            match wildcard {
                Some(wildcard) => absolute[..wildcard].rfind('/').map_or_else(
                    || directory.to_owned(),
                    |index| absolute[..index].to_owned(),
                ),
                None if path_component_has_extension(&absolute) => absolute.rfind('/').map_or_else(
                    || directory.to_owned(),
                    |index| absolute[..index].to_owned(),
                ),
                None => absolute,
            }
        })
        .collect::<Vec<_>>();
    include_bases.sort_by(|left, right| {
        compare_utf16(
            &to_file_name_lower_case(left),
            &to_file_name_lower_case(right),
        )
    });
    let mut bases = vec![directory.to_owned()];
    for include_base in include_bases {
        if bases
            .iter()
            .all(|base| !config_path_contains(base, &include_base))
        {
            bases.push(include_base);
        }
    }
    bases
}

fn config_path_contains(parent: &str, child: &str) -> bool {
    let parent = to_file_name_lower_case(parent.trim_end_matches('/'));
    let child = to_file_name_lower_case(child.trim_end_matches('/'));
    child == parent
        || child
            .strip_prefix(&parent)
            .is_some_and(|tail| tail.starts_with('/'))
}

#[allow(clippy::too_many_arguments)]
fn visit_config_directory(
    units: &[CompilerUnitInput],
    base_directory: &str,
    directory: &str,
    extensions: &[&str],
    excludes: Option<&[String]>,
    includes: &[String],
    include_patterns: &[Option<ConfigFilePattern>],
    depth: Option<usize>,
    visited: &mut HashSet<String>,
    buckets: &mut [Vec<String>],
) -> Result<(), ConfigHostError> {
    let directory_key = to_file_name_lower_case(directory);
    if !visited.insert(directory_key.clone()) {
        return Ok(());
    }
    let mut files = Vec::new();
    let mut directories = Vec::new();
    for unit in units {
        let normalized = normalize_compiler_unit_path(unit.name.as_ref()).map_err(|error| {
            ConfigHostError::new(
                ConfigHostOperation::ReadDirectory,
                unit.name.as_ref(),
                error.to_string(),
            )
        })?;
        // harnessIO's virtual ParseConfigHost intentionally uses a raw folded
        // string prefix here rather than a path-component containment check.
        // Preserve that observable quirk for compiler-runner compatibility.
        if !to_file_name_lower_case(&normalized).starts_with(&directory_key) {
            continue;
        }
        let Some(mut tail) = normalized.get(directory.len()..) else {
            continue;
        };
        if let Some(stripped) = tail.strip_prefix('/') {
            tail = stripped;
        }
        if let Some(separator) = tail.find('/') {
            let child = &tail[..separator];
            if !child.is_empty() && !directories.iter().any(|entry| entry == child) {
                directories.push(child.to_owned());
            }
        } else if !tail.is_empty() {
            files.push(tail.to_owned());
        }
    }
    files.sort_by(|left, right| compare_utf16(left, right));
    for file in files {
        let path = join_config_path(directory, &file);
        if !extensions
            .iter()
            .any(|extension| file_extension_is_exact(&path, extension))
            || excludes.is_some_and(|patterns| {
                patterns
                    .iter()
                    .any(|pattern| config_exclude_matches(base_directory, pattern, &path))
            })
        {
            continue;
        }
        let include_index = if includes.is_empty() {
            Some(0)
        } else {
            include_patterns.iter().position(|pattern| {
                pattern
                    .as_ref()
                    .is_some_and(|pattern| pattern.matches(&path))
            })
        };
        if let Some(include_index) = include_index {
            buckets[include_index].push(path);
        }
    }

    if depth == Some(1) {
        return Ok(());
    }
    let child_depth = depth.map(|depth| depth.saturating_sub(1));
    directories.sort_by(|left, right| compare_utf16(left, right));
    for child in directories {
        let path = join_config_path(directory, &child);
        if excludes.is_some_and(|patterns| {
            patterns
                .iter()
                .any(|pattern| config_exclude_matches(base_directory, pattern, &path))
        }) {
            continue;
        }
        visit_config_directory(
            units,
            base_directory,
            &path,
            extensions,
            excludes,
            includes,
            include_patterns,
            child_depth,
            visited,
            buckets,
        )?;
    }
    Ok(())
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

fn file_extension_is_exact(path: &str, extension: &str) -> bool {
    path.len() > extension.len() && path.ends_with(extension)
}

fn config_exclude_matches(base_directory: &str, pattern: &str, path: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = absolute_config_pattern(base_directory, pattern);
    if pattern.contains(['*', '?']) {
        return glob_matches(&pattern, path, false);
    }
    let pattern = pattern.trim_end_matches('/');
    let folded_pattern = to_file_name_lower_case(pattern);
    let folded_path = to_file_name_lower_case(path);
    folded_path == folded_pattern
        || folded_path
            .strip_prefix(&folded_pattern)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn absolute_config_pattern(base_directory: &str, pattern: &str) -> String {
    let pattern = pattern.replace('\\', "/");
    if compiler_fixture_root_parts(&pattern).is_some() {
        return normalize_compiler_rooted_path(&pattern).unwrap_or(pattern);
    }
    let combined = join_config_path(base_directory, &pattern);
    normalize_compiler_rooted_path(&combined).unwrap_or(combined)
}

fn path_component_has_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|base| base.contains('.'))
}

fn glob_matches(pattern: &str, text: &str, case_sensitive: bool) -> bool {
    let (pattern, text) = if case_sensitive {
        (pattern.to_owned(), text.to_owned())
    } else {
        (
            to_file_name_lower_case(pattern),
            to_file_name_lower_case(text),
        )
    };
    let pattern = pattern.chars().collect::<Vec<_>>();
    let text = text.chars().collect::<Vec<_>>();
    let mut memo = vec![vec![None; text.len() + 1]; pattern.len() + 1];

    fn matches(
        pattern: &[char],
        text: &[char],
        pattern_index: usize,
        text_index: usize,
        memo: &mut [Vec<Option<bool>>],
    ) -> bool {
        if let Some(result) = memo[pattern_index][text_index] {
            return result;
        }
        let result = if pattern_index == pattern.len() {
            text_index == text.len()
        } else if pattern[pattern_index] == '*' {
            let double = pattern.get(pattern_index + 1) == Some(&'*');
            let after_star = pattern_index + if double { 2 } else { 1 };
            if double && pattern.get(after_star) == Some(&'/') {
                matches(pattern, text, after_star + 1, text_index, memo)
                    || (text_index < text.len()
                        && matches(pattern, text, pattern_index, text_index + 1, memo))
            } else {
                matches(pattern, text, after_star, text_index, memo)
                    || (text_index < text.len()
                        && (double || text[text_index] != '/')
                        && matches(pattern, text, pattern_index, text_index + 1, memo))
            }
        } else if text_index < text.len()
            && ((pattern[pattern_index] == '?' && text[text_index] != '/')
                || pattern[pattern_index] == text[text_index])
        {
            matches(pattern, text, pattern_index + 1, text_index + 1, memo)
        } else {
            false
        };
        memo[pattern_index][text_index] = Some(result);
        result
    }

    matches(&pattern, &text, 0, 0, &mut memo)
}

fn compare_utf16(left: &str, right: &str) -> std::cmp::Ordering {
    left.encode_utf16().cmp(right.encode_utf16())
}

fn build_compiler_plan(
    fixture: Arc<CompilerFixtureInput>,
    configuration_index: u32,
) -> HarnessResult<CompilerExecutionPlan> {
    let recorded = fixture.source.index;
    let manifest_configuration = fixture
        .configurations
        .get(configuration_index as usize)
        .ok_or_else(|| {
            error(format!(
                "compiler source index {recorded} has no configuration {configuration_index}"
            ))
        })?
        .clone();
    let effective_settings = if manifest_configuration.settings.is_empty() {
        Arc::clone(&fixture.settings)
    } else {
        Arc::from(merge_ordered_settings(
            fixture.settings.as_ref(),
            &manifest_configuration.settings,
        ))
    };
    let current_directory = compiler_current_directory(&effective_settings)?;
    let use_case_sensitive_file_names = compiler_case_sensitivity(&effective_settings);
    let allow_js = compiler_root_allow_js(&fixture, &effective_settings)?;
    let root_selection = compiler_root_selection(&fixture, &effective_settings, allow_js)?;

    Ok(CompilerExecutionPlan {
        fixture,
        variant: CompilerVariant {
            configuration_index,
            key: Arc::from(manifest_configuration.variant.as_str()),
            description: Arc::from(manifest_configuration.description.as_str()),
            upstream_name: Arc::from(manifest_configuration.upstream_name.as_str()),
            overrides: Arc::from(manifest_configuration.settings.clone()),
        },
        effective_settings,
        current_directory: Arc::from(current_directory),
        use_case_sensitive_file_names,
        root_selection,
    })
}

fn compiler_root_selection(
    fixture: &CompilerFixtureInput,
    settings: &[OrderedSetting],
    allow_js: bool,
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
                .any(|file_name| file_name == &normalized)
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
            allow_js,
        ))
    } else {
        Ok(explicit_compiler_roots(
            fixture,
            CompilerExplicitRootReason::AllUnits,
            candidates,
            Vec::new(),
            allow_js,
        ))
    }
}

fn explicit_compiler_roots(
    fixture: &CompilerFixtureInput,
    reason: CompilerExplicitRootReason,
    root_units: Vec<CompilerUnitId>,
    other_units: Vec<CompilerUnitId>,
    allow_js: bool,
) -> CompilerRootSelection {
    let vfs_write_order = root_units
        .iter()
        .chain(&other_units)
        .copied()
        .collect::<Vec<_>>();
    let program_root_units = supported_compiler_roots(fixture, &root_units, allow_js);
    CompilerRootSelection::Explicit {
        reason,
        root_units: Arc::from(root_units),
        other_units: Arc::from(other_units),
        vfs_write_order: Arc::from(vfs_write_order),
        program_root_units: Arc::from(program_root_units),
    }
}

/// tsc `isSupportedSourceFileName` followed by CompilerBaselineRunner's
/// explicit JSON-root exclusion. The comparison is deliberately
/// case-sensitive, matching `fileExtensionIs`; host case sensitivity affects
/// path identity, not recognized source-extension spelling.
fn supported_compiler_roots(
    fixture: &CompilerFixtureInput,
    root_units: &[CompilerUnitId],
    allow_js: bool,
) -> Vec<CompilerUnitId> {
    const TS_EXTENSIONS: [&str; 7] = [".ts", ".tsx", ".d.ts", ".cts", ".d.cts", ".mts", ".d.mts"];
    const JS_EXTENSIONS: [&str; 4] = [".js", ".jsx", ".mjs", ".cjs"];

    root_units
        .iter()
        .copied()
        .filter(|id| {
            fixture.units.get(id.0 as usize).is_some_and(|unit| {
                let name = unit.name.as_ref();
                !file_extension_is(name, ".json")
                    && (TS_EXTENSIONS
                        .iter()
                        .any(|extension| file_extension_is(name, extension))
                        || allow_js
                            && JS_EXTENSIONS
                                .iter()
                                .any(|extension| file_extension_is(name, extension)))
            })
        })
        .collect()
}

/// Compute only the option dependency needed during root selection without
/// eagerly validating unrelated fixture options. This mirrors tsc's
/// `_computedOptions.allowJs`: an explicit `allowJs` wins, otherwise `checkJs`
/// supplies the value.
fn compiler_root_allow_js(
    fixture: &CompilerFixtureInput,
    settings: &[OrderedSetting],
) -> HarnessResult<bool> {
    let (mut allow_js, mut check_js, mut has_explicit_allow_js) = fixture
        .config_root_plan
        .as_ref()
        .map(|config| {
            (
                config.compiler_options().allow_js,
                config.compiler_options().check_js,
                matches!(
                    config.options().typed_value_state("allowJs"),
                    ConfigOptionValueState::Value(value) if value.is_boolean()
                ),
            )
        })
        .unwrap_or((false, None, false));
    for setting in settings {
        match CompilerFixtureOptionKey::new(&setting.name).as_str() {
            "allowjs" => {
                allow_js = parse_compiler_bool(&setting.name, &setting.value)?;
                has_explicit_allow_js = true;
            }
            "checkjs" => check_js = Some(parse_compiler_bool(&setting.name, &setting.value)?),
            _ => {}
        }
    }
    Ok(if has_explicit_allow_js {
        allow_js
    } else {
        check_js.unwrap_or(false)
    })
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

fn build_project_fixture(
    recorded: &super::ProjectFixtureExpansion,
    source: Arc<VerifiedSource>,
    tests_mount: Arc<ProjectMount>,
) -> HarnessResult<ProjectFixtureInput> {
    if source.encoding != recorded.encoding {
        return Err(error(format!(
            "project descriptor {:?} encoding no longer matches the manifest",
            source.relative_path
        )));
    }
    let properties = parse_ordered_json_object(&source.decoded, source.relative_path.as_ref())?;
    let scenario =
        required_ordered_string(&properties, "scenario", source.relative_path.as_ref())?.to_owned();
    let project_root =
        required_ordered_string(&properties, "projectRoot", source.relative_path.as_ref())?
            .to_owned();
    if scenario != recorded.scenario || project_root != recorded.project_root {
        return Err(error(format!(
            "project descriptor {:?} identity no longer matches the manifest",
            source.relative_path
        )));
    }
    verify_project_inputs(
        &properties,
        &recorded.input_files,
        source.relative_path.as_ref(),
    )?;
    let current_directory = normalize_virtual_path(VIRTUAL_SOURCE_ROOT, &project_root)?;
    let root_selection =
        project_root_selection(&properties, &recorded.input_files, &current_directory)?;
    Ok(ProjectFixtureInput {
        descriptor_raw: Arc::clone(&source.raw),
        descriptor_text: Arc::clone(&source.decoded),
        properties: Arc::from(
            properties
                .into_iter()
                .map(|(name, value)| OrderedJsonProperty {
                    name: Arc::from(name),
                    value,
                })
                .collect::<Vec<_>>(),
        ),
        scenario: Arc::from(scenario),
        project_root: Arc::from(project_root),
        input_files: recorded.input_files.clone(),
        current_directory: Arc::from(current_directory),
        mount: tests_mount,
        root_selection,
        source,
    })
}

fn build_project_plan(
    fixture: Arc<ProjectFixtureInput>,
    module: ProjectModule,
    baseline_folder: &str,
) -> ProjectExecutionPlan {
    let descriptor_module_override = fixture
        .properties
        .iter()
        .filter(|property| property.name.as_ref() == "module")
        .map(|property| property.value.clone())
        .next_back();
    ProjectExecutionPlan {
        fixture,
        module_variant: module,
        baseline_folder: Arc::from(baseline_folder),
        descriptor_module_override,
    }
}

fn project_root_selection(
    properties: &[(String, Value)],
    inputs: &ProjectInputFiles,
    current_directory: &str,
) -> HarnessResult<ProjectRootSelection> {
    let raw_project = properties
        .iter()
        .filter(|(name, _)| name == "project")
        .filter_map(|(_, value)| value.as_str())
        .next_back()
        .filter(|value| !value.is_empty());
    if let Some(project) = raw_project {
        if matches!(inputs, ProjectInputFiles::Present { inputs } if !inputs.is_empty()) {
            return Err(error(
                "project descriptor cannot combine a project option with explicit input files",
            ));
        }
        let config_file_name = normalize_posix_path(&join_posix(project, "tsconfig.json"), false)?;
        let resolved_config_path = normalize_virtual_path(current_directory, &config_file_name)?;
        return Ok(ProjectRootSelection::ProjectConfig {
            raw_project: Arc::from(project),
            config_file_name: Arc::from(config_file_name),
            resolved_config_path: Arc::from(resolved_config_path),
        });
    }
    match inputs {
        ProjectInputFiles::Present { inputs } if !inputs.is_empty() => {
            Ok(ProjectRootSelection::Explicit {
                input_names: Arc::from(
                    inputs
                        .iter()
                        .map(|input| Arc::from(input.path.as_str()))
                        .collect::<Vec<_>>(),
                ),
            })
        }
        ProjectInputFiles::Absent | ProjectInputFiles::Present { .. } => {
            Ok(ProjectRootSelection::DiscoverConfig)
        }
    }
}

fn verify_project_inputs(
    properties: &[(String, Value)],
    recorded: &ProjectInputFiles,
    fixture_path: &str,
) -> HarnessResult<()> {
    let value = properties
        .iter()
        .find(|(name, _)| name == "inputFiles")
        .map(|(_, value)| value);
    match (value, recorded) {
        (None, ProjectInputFiles::Absent) => Ok(()),
        (Some(Value::Array(values)), ProjectInputFiles::Present { inputs }) => {
            let actual = values
                .iter()
                .map(|value| {
                    value.as_str().ok_or_else(|| {
                        error(format!(
                            "project descriptor {fixture_path:?} inputFiles entries must be strings"
                        ))
                    })
                })
                .collect::<HarnessResult<Vec<_>>>()?;
            if actual
                == inputs
                    .iter()
                    .map(|input| input.path.as_str())
                    .collect::<Vec<_>>()
            {
                Ok(())
            } else {
                Err(error(format!(
                    "project descriptor {fixture_path:?} inputs no longer match the manifest"
                )))
            }
        }
        _ => Err(error(format!(
            "project descriptor {fixture_path:?} inputFiles state no longer matches the manifest"
        ))),
    }
}

#[derive(Debug)]
struct OrderedObject(Vec<(String, Value)>);

impl<'de> Deserialize<'de> for OrderedObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedObjectVisitor;

        impl<'de> Visitor<'de> for OrderedObjectVisitor {
            type Value = OrderedObject;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a JSON object")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut properties = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some((name, value)) = map.next_entry::<String, Value>()? {
                    if let Some((_, existing)) = properties
                        .iter_mut()
                        .find(|(existing, _)| existing == &name)
                    {
                        *existing = value;
                    } else {
                        properties.push((name, value));
                    }
                }
                Ok(OrderedObject(properties))
            }
        }

        deserializer.deserialize_map(OrderedObjectVisitor)
    }
}

fn parse_ordered_json_object(text: &str, path: &str) -> HarnessResult<Vec<(String, Value)>> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let object = OrderedObject::deserialize(&mut deserializer).map_err(|source| {
        error(format!(
            "project descriptor {path:?} is invalid JSON: {source}"
        ))
    })?;
    deserializer.end().map_err(|source| {
        error(format!(
            "project descriptor {path:?} has trailing JSON content: {source}"
        ))
    })?;
    Ok(object.0)
}

fn required_ordered_string<'a>(
    properties: &'a [(String, Value)],
    name: &str,
    path: &str,
) -> HarnessResult<&'a str> {
    properties
        .iter()
        .find(|(candidate, _)| candidate == name)
        .and_then(|(_, value)| value.as_str())
        .ok_or_else(|| {
            error(format!(
                "project descriptor {path:?} field {name:?} must be a string"
            ))
        })
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

fn merge_ordered_settings(
    base: &[OrderedSetting],
    overrides: &[OrderedSetting],
) -> Vec<OrderedSetting> {
    let mut result = base.to_vec();
    for setting in overrides {
        if let Some(existing) = result
            .iter_mut()
            .find(|existing| existing.name == setting.name)
        {
            existing.value.clone_from(&setting.value);
        } else {
            result.push(setting.clone());
        }
    }
    result
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

fn source_entry(manifest: &ExpansionManifest, index: u32) -> HarnessResult<&SourceInventoryEntry> {
    manifest.sources.get(index as usize).ok_or_else(|| {
        error(format!(
            "execution plan references missing source index {index}"
        ))
    })
}

fn suite_identity(
    manifest: &ExpansionManifest,
    suite: SuiteName,
) -> HarnessResult<&super::CorpusSuiteIdentity> {
    manifest
        .corpus_pin
        .suites
        .iter()
        .find(|identity| identity.name == suite)
        .ok_or_else(|| error(format!("manifest has no {} suite identity", suite.as_str())))
}

fn join_posix(left: &str, right: &str) -> String {
    if left.is_empty() {
        right.to_owned()
    } else if right.is_empty() {
        left.to_owned()
    } else {
        format!(
            "{}/{}",
            left.trim_end_matches('/'),
            right.trim_start_matches('/')
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/upstream_suites/execution_tests.rs"]
mod tests;
