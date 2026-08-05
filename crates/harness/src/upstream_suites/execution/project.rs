use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;
use tsc_host::{CompilerHost, FsCompilerHost, HostError, HostErrorKind, HostOperation};
use tsc_program::{
    load_program, CompilerConfigHost, CompilerOptions, ConfigFilePattern, ConfigHostError,
    ConfigHostOperation, ConfigParseHost, ConfigRootPlan, ConfigRootPlanRequest, LibraryCatalog,
    PreparedProgram, ProgramLoadLimits, ProgramOptions,
};

use super::{
    absolute_config_pattern, compare_utf16, config_base_paths, error, file_extension_is_exact,
    glob_matches, join_config_path, normalize_virtual_path, ProjectExecutionPlan, ProjectModule,
    ProjectMount, ProjectRootSelection,
};
use crate::HarnessResult;

const PROJECT_RUNNER_DEFAULT_LIBRARY: &str = "lib.es5.d.ts";
const NODE_MODULES_SEARCH_CASES: &[&str] = &[
    "nodeModulesImportHigher.json",
    "nodeModulesMaxDepthExceeded.json",
    "nodeModulesMaxDepthIncreased.json",
];

/// Focused no-emit projection of one pinned `NodeModulesSearch` project case.
///
/// The upstream project runner always emits and verifies output baselines.
/// This result deliberately stops at the owned H0 loader boundary, so its
/// manifest case remains `not-run` until emit and full baseline comparison are
/// implemented. `effective_compiler_options` contains the loader-owned subset
/// of the runner/config option projection plus the mandatory H0 `noEmit=true`
/// adapter.
#[derive(Clone, Debug)]
pub struct ProjectConfigProgram {
    pub config_root_plan: Arc<ConfigRootPlan>,
    pub root_names: Arc<[PathBuf]>,
    pub effective_compiler_options: CompilerOptions,
    pub effective_program_options: ProgramOptions,
    pub prepared_program: PreparedProgram,
}

/// Parse and load the six `NodeModulesSearch` CommonJS/AMD variants through
/// the same descriptor-existing-options then config path used by the pinned
/// TypeScript project runner for the loader-facing option subset.
///
/// `projectsRunner.ts` supplies its existing compiler options to
/// `parseJsonSourceFileConfigFileContent`; TypeScript's `extend(first,
/// second)` copies `first` last, so runner/descriptor values win conflicts.
/// The project host also selects `lib.es5.d.ts` independently of `target`.
/// Both facts are retained without fabricating a raw `lib` option.
pub fn load_node_modules_search_project(
    workspace: &Path,
    plan: &ProjectExecutionPlan,
    limits: ProgramLoadLimits,
) -> HarnessResult<ProjectConfigProgram> {
    if !NODE_MODULES_SEARCH_CASES.contains(&plan.fixture.source.relative_path.as_ref()) {
        return Err(error(format!(
            "focused NodeModulesSearch executor does not own project descriptor {:?}",
            plan.fixture.source.relative_path
        )));
    }
    if plan.fixture.project_root.as_ref() != "tests/cases/projects/NodeModulesSearch" {
        return Err(error(format!(
            "NodeModulesSearch descriptor {:?} has unexpected project root {:?}",
            plan.fixture.source.relative_path, plan.fixture.project_root
        )));
    }
    let ProjectRootSelection::ProjectConfig {
        resolved_config_path,
        ..
    } = &plan.fixture.root_selection
    else {
        return Err(error(format!(
            "NodeModulesSearch descriptor {:?} is not project-config selected",
            plan.fixture.source.relative_path
        )));
    };

    let library_directory = normalize_existing_directory(
        &workspace.join("vendor/typescript-6.0.3/lib"),
        "pinned TypeScript library directory",
    )?;

    let host = MountedProjectHost::new(
        workspace,
        Arc::clone(&plan.fixture.mount),
        Arc::clone(&plan.fixture.current_directory),
        library_directory.clone(),
    )?;
    let config_text = host
        .mounted_file(resolved_config_path)
        .ok_or_else(|| {
            error(format!(
                "NodeModulesSearch config {resolved_config_path:?} is absent from the verified mount"
            ))
        })?
        .source
        .decoded
        .to_string();
    let config_root_plan = Arc::new(
        tsc_program::parse_config_root_plan(
            &host,
            ConfigRootPlanRequest {
                file_name: resolved_config_path.to_string(),
                text: config_text,
                base_path: plan.fixture.current_directory.to_string(),
            },
        )
        .map_err(|parse_error| {
            error(format!(
                "failed to parse NodeModulesSearch config {resolved_config_path:?}: {parse_error}"
            ))
        })?,
    );
    if config_root_plan.diagnostics().next().is_some()
        || !config_root_plan.option_diagnostics().is_empty()
    {
        return Err(error(format!(
            "focused NodeModulesSearch config {resolved_config_path:?} produced diagnostics"
        )));
    }

    let mut compiler_options = config_root_plan
        .module_resolution_options()
        .compiler_options()
        .clone();
    apply_project_runner_existing_options(plan, &mut compiler_options)?;
    // H0 is an intentionally mandatory no-emit boundary. This is the sole
    // execution adapter that differs from the emitting upstream runner.
    compiler_options.no_emit = Some(true);

    let program_options = config_root_plan
        .module_resolution_options()
        .program_options()
        .clone()
        // projectsRunner omits parseJsonSourceFileConfigFileContent's optional
        // configFileName argument, leaving options.configFilePath undefined.
        .without_config_file_path()
        .with_default_library_file_name(PROJECT_RUNNER_DEFAULT_LIBRARY);
    let root_names = config_root_plan
        .file_names()
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let library_catalog = LibraryCatalog::typescript_6_0_3(library_directory);
    let prepared_program = load_program(
        &host,
        &root_names,
        compiler_options.clone(),
        program_options.clone(),
        &library_catalog,
        limits,
    )
    .map_err(|load_error| {
        error(format!(
            "failed to load NodeModulesSearch project {:?}: {load_error}",
            plan.fixture.source.relative_path
        ))
    })?;

    Ok(ProjectConfigProgram {
        config_root_plan,
        root_names: Arc::from(root_names),
        effective_compiler_options: compiler_options,
        effective_program_options: program_options,
        prepared_program,
    })
}

fn normalize_existing_directory(path: &Path, label: &str) -> HarnessResult<PathBuf> {
    if !path.is_absolute() {
        return Err(error(format!("{label} {path:?} must be absolute")));
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    return Err(error(format!(
                        "{label} {path:?} escapes its filesystem root"
                    )));
                }
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    let metadata = std::fs::metadata(&normalized).map_err(|source_error| {
        error(format!(
            "failed to inspect {label} {normalized:?}: {source_error}"
        ))
    })?;
    if !metadata.is_dir() {
        return Err(error(format!("{label} {normalized:?} is not a directory")));
    }
    Ok(normalized)
}

fn apply_project_runner_existing_options(
    plan: &ProjectExecutionPlan,
    options: &mut CompilerOptions,
) -> HarnessResult<()> {
    // createCompilerOptions initializes these before descriptor options.
    options.no_error_truncation = Some(false);
    options.module_resolution = Some(1); // Classic
    options.module = Some(match plan.module_variant {
        ProjectModule::Commonjs => 1,
        ProjectModule::Amd => 2,
    });

    // The descriptor loop only copies recognized command/compiler options.
    // Keep this focused adapter fail-closed on the exact option spellings used
    // by the three pinned descriptors.
    for property in plan.fixture.properties.iter() {
        match property.name.as_ref() {
            "module" => {
                options.module = Some(project_named_i32(&property.value, "module")?);
            }
            "moduleResolution" => {
                options.module_resolution =
                    Some(project_named_i32(&property.value, "moduleResolution")?);
            }
            "declaration" => {
                if property.value.as_bool() != Some(false) {
                    return Err(error(
                        "focused NodeModulesSearch executor requires declaration=false",
                    ));
                }
            }
            "scenario" | "projectRoot" | "baselineCheck" | "project" => {}
            other => {
                return Err(error(format!(
                    "focused NodeModulesSearch descriptor contains unsupported property {other:?}"
                )));
            }
        }
    }
    Ok(())
}

fn project_named_i32(value: &Value, option: &str) -> HarnessResult<i32> {
    if let Some(value) = value.as_i64() {
        return i32::try_from(value)
            .map_err(|_| error(format!("project option {option:?} is outside i32")));
    }
    let value = value.as_str().ok_or_else(|| {
        error(format!(
            "project option {option:?} must be a string or integer"
        ))
    })?;
    match (option, value.to_ascii_lowercase().as_str()) {
        ("module", "commonjs") => Ok(1),
        ("module", "amd") => Ok(2),
        ("moduleResolution", "classic") => Ok(1),
        ("moduleResolution", "node" | "node10") => Ok(2),
        _ => Err(error(format!(
            "focused NodeModulesSearch executor does not own {option}={value:?}"
        ))),
    }
}

#[derive(Debug)]
struct MountedProjectHost {
    filesystem: FsCompilerHost,
    mount: Arc<ProjectMount>,
    current_directory: Arc<str>,
    library_directory: PathBuf,
}

impl MountedProjectHost {
    fn new(
        workspace: &Path,
        mount: Arc<ProjectMount>,
        current_directory: Arc<str>,
        library_directory: PathBuf,
    ) -> HarnessResult<Self> {
        if !mount.case_sensitive || !mount.read_only {
            return Err(error(
                "project executor requires the pinned case-sensitive read-only mount",
            ));
        }
        if mount.workspace_path.as_ref() != &workspace.join("ts-tests/tests") {
            return Err(error(format!(
                "project mount {:?} does not belong to workspace {:?}",
                mount.workspace_path, workspace
            )));
        }
        let filesystem = FsCompilerHost::new(workspace, true).map_err(|host_error| {
            error(format!(
                "failed to create project filesystem host: {host_error}"
            ))
        })?;
        Ok(Self {
            filesystem,
            mount,
            current_directory,
            library_directory,
        })
    }

    fn normalized_query(&self, path: &str, operation: HostOperation) -> Result<String, HostError> {
        normalize_virtual_path(self.current_directory.as_ref(), path).map_err(|path_error| {
            HostError::new(
                HostErrorKind::InvalidInput,
                operation,
                Some(PathBuf::from(path)),
                path_error.to_string(),
            )
        })
    }

    fn mounted_file(&self, path: &str) -> Option<&super::ProjectMountFile> {
        let normalized = self.normalized_query(path, HostOperation::ReadFile).ok()?;
        self.mount
            .files
            .iter()
            .find(|file| file.virtual_path.as_ref() == normalized)
    }

    fn mounted_entry_exists(
        &self,
        path: &str,
        operation: HostOperation,
    ) -> Result<bool, HostError> {
        let normalized = self.normalized_query(path, operation)?;
        if self
            .mount
            .files
            .iter()
            .any(|file| file.virtual_path.as_ref() == normalized)
        {
            return Ok(true);
        }
        let prefix = format!("{}/", normalized.trim_end_matches('/'));
        Ok(self
            .mount
            .files
            .iter()
            .any(|file| file.virtual_path.starts_with(&prefix)))
    }

    fn path_is_in_mount(&self, path: &str) -> bool {
        let root = self.mount.virtual_path.trim_end_matches('/');
        path == root
            || path
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('/'))
    }

    fn path_is_in_library(&self, path: &Path) -> bool {
        path.is_absolute()
            && path.starts_with(&self.library_directory)
            && !path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::CurDir | std::path::Component::ParentDir
                )
            })
    }

    fn mounted_directory_entries(&self, path: &str) -> Result<Vec<PathBuf>, HostError> {
        let normalized = self.normalized_query(path, HostOperation::ReadDirectory)?;
        if !self.path_is_in_mount(&normalized)
            || !self.mounted_entry_exists(&normalized, HostOperation::ReadDirectory)?
        {
            return Ok(Vec::new());
        }
        let prefix = format!("{}/", normalized.trim_end_matches('/'));
        let mut names = Vec::new();
        for file in self.mount.files.iter() {
            let Some(tail) = file.virtual_path.strip_prefix(&prefix) else {
                continue;
            };
            let name = tail.split('/').next().unwrap_or_default();
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
        names.sort_by(|left, right| compare_utf16(left, right));
        Ok(names
            .into_iter()
            .map(|name| PathBuf::from(join_config_path(&normalized, name)))
            .collect())
    }

    fn host_error(
        operation: ConfigHostOperation,
        path: &str,
        source: HostError,
    ) -> ConfigHostError {
        ConfigHostError::new(operation, path, source.to_string())
    }
}

impl CompilerHost for MountedProjectHost {
    fn current_directory(&self) -> Result<PathBuf, HostError> {
        Ok(PathBuf::from(self.current_directory.as_ref()))
    }

    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, HostError> {
        if self.path_is_in_library(path) {
            return self.filesystem.read_file(path);
        }
        let text = path.to_str().ok_or_else(|| {
            HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::ReadFile,
                Some(path.to_path_buf()),
                "project host path is not Unicode",
            )
        })?;
        let normalized = self.normalized_query(text, HostOperation::ReadFile)?;
        if self.path_is_in_mount(&normalized) {
            return Ok(self
                .mount
                .files
                .iter()
                .find(|file| file.virtual_path.as_ref() == normalized)
                .map(|file| file.source.raw.to_vec()));
        }
        Ok(None)
    }

    fn file_exists(&self, path: &Path) -> Result<bool, HostError> {
        if self.path_is_in_library(path) {
            return self.filesystem.file_exists(path);
        }
        let text = path.to_str().ok_or_else(|| {
            HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::FileExists,
                Some(path.to_path_buf()),
                "project host path is not Unicode",
            )
        })?;
        let normalized = self.normalized_query(text, HostOperation::FileExists)?;
        if self.path_is_in_mount(&normalized) {
            return Ok(self
                .mount
                .files
                .iter()
                .any(|file| file.virtual_path.as_ref() == normalized));
        }
        Ok(false)
    }

    fn directory_exists(&self, path: &Path) -> Result<bool, HostError> {
        if self.path_is_in_library(path) {
            return self.filesystem.directory_exists(path);
        }
        let text = path.to_str().ok_or_else(|| {
            HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::DirectoryExists,
                Some(path.to_path_buf()),
                "project host path is not Unicode",
            )
        })?;
        let normalized = self.normalized_query(text, HostOperation::DirectoryExists)?;
        if self.path_is_in_mount(&normalized) {
            let prefix = format!("{}/", normalized.trim_end_matches('/'));
            return Ok(self
                .mount
                .files
                .iter()
                .any(|file| file.virtual_path.starts_with(&prefix)));
        }
        Ok(false)
    }

    fn read_directory(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if self.path_is_in_library(path) {
            return self.filesystem.read_directory(path);
        }
        let text = path.to_str().ok_or_else(|| {
            HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::ReadDirectory,
                Some(path.to_path_buf()),
                "project host path is not Unicode",
            )
        })?;
        let normalized = self.normalized_query(text, HostOperation::ReadDirectory)?;
        if self.path_is_in_mount(&normalized) {
            return self.mounted_directory_entries(&normalized);
        }
        Ok(Vec::new())
    }

    fn get_directories(&self, path: &Path) -> Result<Vec<PathBuf>, HostError> {
        if self.path_is_in_library(path) {
            return self.filesystem.get_directories(path);
        }
        CompilerHost::read_directory(self, path)?
            .into_iter()
            .filter_map(|entry| match CompilerHost::directory_exists(self, &entry) {
                Ok(true) => Some(Ok(entry)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    fn realpath(&self, path: &Path) -> Result<Option<PathBuf>, HostError> {
        if self.path_is_in_library(path) {
            return self.filesystem.realpath(path);
        }
        let text = path.to_str().ok_or_else(|| {
            HostError::new(
                HostErrorKind::InvalidInput,
                HostOperation::Realpath,
                Some(path.to_path_buf()),
                "project host path is not Unicode",
            )
        })?;
        let normalized = self.normalized_query(text, HostOperation::Realpath)?;
        if self.path_is_in_mount(&normalized) {
            return self
                .mounted_entry_exists(&normalized, HostOperation::Realpath)
                .map(|exists| exists.then(|| PathBuf::from(normalized)));
        }
        Ok(None)
    }
}

impl ConfigParseHost for MountedProjectHost {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn file_exists(&self, path: &str) -> Result<bool, ConfigHostError> {
        CompilerHost::file_exists(self, Path::new(path))
            .map_err(|source| Self::host_error(ConfigHostOperation::FileExists, path, source))
    }

    fn read_file(&self, path: &str) -> Result<Option<String>, ConfigHostError> {
        // Keep project config reads on the same BOM/UTF-16/invalid-UTF-8
        // boundary as the production CLI and ordinary filesystem programs.
        // The virtual mount still owns the raw bytes; this adapter only
        // centralizes their TypeScript-compatible text projection.
        ConfigParseHost::read_file(&CompilerConfigHost::new(self), path)
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
                ConfigFilePattern::new(include, directory, true).map_err(|detail| {
                    ConfigHostError::new(
                        ConfigHostOperation::ReadDirectory,
                        directory,
                        format!("invalid project include {include:?}: {detail}"),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut buckets = vec![Vec::new(); includes.len().max(1)];
        let mut visited = HashSet::new();
        for base_path in config_base_paths(directory, includes) {
            visit_project_config_directory(
                self,
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
        buckets
            .into_iter()
            .flatten()
            .map(|path| relative_posix_path(self.current_directory.as_ref(), &path))
            .collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn visit_project_config_directory(
    host: &MountedProjectHost,
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
    if !visited.insert(directory.to_owned()) {
        return Ok(());
    }
    let entries = CompilerHost::read_directory(host, Path::new(directory)).map_err(|source| {
        MountedProjectHost::host_error(ConfigHostOperation::ReadDirectory, directory, source)
    })?;
    let mut files = Vec::new();
    let mut directories = Vec::new();
    for entry in entries {
        let text = entry.to_str().ok_or_else(|| {
            ConfigHostError::new(
                ConfigHostOperation::ReadDirectory,
                directory,
                "project directory entry is not Unicode",
            )
        })?;
        if CompilerHost::directory_exists(host, &entry).map_err(|source| {
            MountedProjectHost::host_error(ConfigHostOperation::ReadDirectory, text, source)
        })? {
            directories.push(text.to_owned());
        } else {
            files.push(text.to_owned());
        }
    }
    files.sort_by(|left, right| compare_utf16(left, right));
    for path in files {
        if !extensions
            .iter()
            .any(|extension| file_extension_is_exact(&path, extension))
            || excludes.is_some_and(|patterns| {
                patterns
                    .iter()
                    .any(|pattern| project_exclude_matches(base_directory, pattern, &path))
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
    let child_depth = depth.map(|value| value.saturating_sub(1));
    directories.sort_by(|left, right| compare_utf16(left, right));
    for path in directories {
        if excludes.is_some_and(|patterns| {
            patterns
                .iter()
                .any(|pattern| project_exclude_matches(base_directory, pattern, &path))
        }) {
            continue;
        }
        visit_project_config_directory(
            host,
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

fn project_exclude_matches(base_directory: &str, pattern: &str, path: &str) -> bool {
    if pattern.is_empty() {
        return false;
    }
    let pattern = absolute_config_pattern(base_directory, pattern);
    if pattern.contains(['*', '?']) {
        return glob_matches(&pattern, path, true);
    }
    let pattern = pattern.trim_end_matches('/');
    path == pattern
        || path
            .strip_prefix(pattern)
            .is_some_and(|tail| tail.starts_with('/'))
}

fn relative_posix_path(base: &str, target: &str) -> Result<String, ConfigHostError> {
    let base = normalize_virtual_path("/", base).map_err(|path_error| {
        ConfigHostError::new(
            ConfigHostOperation::ReadDirectory,
            base,
            path_error.to_string(),
        )
    })?;
    let target = normalize_virtual_path("/", target).map_err(|path_error| {
        ConfigHostError::new(
            ConfigHostOperation::ReadDirectory,
            target,
            path_error.to_string(),
        )
    })?;
    let base_parts = base.trim_start_matches('/').split('/').collect::<Vec<_>>();
    let target_parts = target
        .trim_start_matches('/')
        .split('/')
        .collect::<Vec<_>>();
    let common = base_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    let mut result = Vec::new();
    result.extend(std::iter::repeat_n("..", base_parts.len() - common));
    result.extend(target_parts[common..].iter().copied());
    Ok(if result.is_empty() {
        ".".to_owned()
    } else {
        result.join("/")
    })
}
