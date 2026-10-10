//! `tsc -b` (tsgo execute/build): the build orchestrator over the project
//! graph. Every project named on the command line and every project it
//! references is a task; the tasks are ordered upstream first; each one is
//! classified up to date or not (`getUpToDateStatus`), built through the
//! ordinary project pipeline with its old build info, pseudo-built by
//! touching its outputs, skipped, or cleaned. The port follows
//! `orchestrator.go`, `buildtask.go`, `uptodatestatus.go` and `host.go`;
//! watch mode and parallel builders are not implemented (the projects are
//! built one after the other, which is the order tsgo reports in).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use tsc_diagnostics::{gen, Diagnostic, JsStr, JsString, MessageChain};
use tsc_host::vfs::FileSystem;
use tsc_host::CompilerHost;
use tsc_incremental::{compute_hash_with_text, is_default_library_name, BuildInfo};
use tsc_program::{
    build_info_file_name_in_build_mode, canonical_emit_path, command_line_option_bag,
    decode_host_text, output_file_names, parse_build_command_line,
    parse_config_root_plan_with_command_line, resolve_config_file_name_of_project_reference,
    CompilerConfigHost, CompilerOptions, ConfigExtendedCache, ConfigOptionBag, ConfigRootPlan,
    ConfigRootPlanRequest, LibraryCatalog, ParsedBuildCommandLine,
};

use crate::cli::{
    relative_file_name, render_diagnostics, run_config_for_build, BuildProjectRun, CliError,
    CliOutput, CliRoute, DiagnosticSourceMap,
};
use crate::statistics::{ProjectCounts, Statistics};
use crate::system::{CommandLineTesting, System};

/// The parsed `tsc -b` command line (tsgo `ParsedBuildCommandLine`).
#[derive(Clone, Debug, Default)]
pub(crate) struct BuildCommand {
    /// The projects as written (`.` when none was named).
    pub(crate) projects: Vec<String>,
    pub(crate) verbose: bool,
    pub(crate) dry: bool,
    pub(crate) force: bool,
    pub(crate) clean: bool,
    pub(crate) stop_build_on_errors: bool,
    /// `--watch`: the orchestrator stays and builds again on changes.
    pub(crate) watch: bool,
    /// The compiler options of the command line, merged over every
    /// project's (tsgo parses every project with them as the existing ones).
    pub(crate) command_line: ConfigOptionBag,
}

impl BuildCommand {
    /// tsgo `ParsedBuildCommandLine`'s parts the orchestrator reads.
    pub(crate) fn from_parsed(
        parsed: &ParsedBuildCommandLine,
        current_directory: JsStr<'_>,
    ) -> Self {
        Self {
            projects: parsed.projects.clone(),
            verbose: parsed.build_bool("verbose"),
            dry: parsed.build_bool("dry"),
            force: parsed.build_bool("force"),
            clean: parsed.build_bool("clean"),
            stop_build_on_errors: parsed.build_bool("stopBuildOnErrors"),
            watch: parsed.option_bool("watch") == Some(true),
            command_line: command_line_option_bag(&parsed.options, current_directory),
        }
    }
}

/// tsgo `ExitStatusProjectReferenceCycle_OutputsSkipped`.
pub(crate) const EXIT_PROJECT_REFERENCE_CYCLE: i32 = 4;

/// The filesystem the orchestrator reads times from and touches (tsgo
/// `incremental.Host`: `GetMTime`, `SetMTime`, and `vfs.FS.Remove`).
pub(crate) trait BuildFileSystem {
    fn modified_time(&self, path: &str) -> Option<SystemTime>;
    fn set_modified_time(&self, path: &str, time: SystemTime) -> Result<(), String>;
    fn remove_file(&self, path: &str) -> Result<(), String>;
    fn file_exists(&self, path: &str) -> bool;
}

impl BuildFileSystem for dyn FileSystem + '_ {
    fn modified_time(&self, path: &str) -> Option<SystemTime> {
        self.metadata(path).ok().map(|metadata| metadata.modified())
    }

    fn set_modified_time(&self, path: &str, time: SystemTime) -> Result<(), String> {
        self.set_modified(path, time)
            .map_err(|error| error.to_string())
    }

    fn remove_file(&self, path: &str) -> Result<(), String> {
        self.remove(path).map_err(|error| error.to_string())
    }

    fn file_exists(&self, path: &str) -> bool {
        self.is_file(path)
    }
}

/// tsgo `upToDateStatusType`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StatusKind {
    ConfigFileNotFound,
    BuildErrors,
    UpstreamErrors,
    UpToDate,
    UpToDateWithUpstreamTypes,
    UpToDateWithInputFileText,
    InputFileMissing,
    OutputMissing,
    InputFileNewer,
    OutOfDateBuildInfoWithPendingEmit,
    OutOfDateBuildInfoWithErrors,
    OutOfDateOptions,
    OutOfDateRoots,
    TsVersionOutputOfDate,
    ForceBuild,
    Solution,
}

impl StatusKind {
    fn is_error(self) -> bool {
        matches!(
            self,
            Self::ConfigFileNotFound | Self::BuildErrors | Self::UpstreamErrors
        )
    }

    fn is_pseudo_build(self) -> bool {
        matches!(
            self,
            Self::UpToDateWithUpstreamTypes | Self::UpToDateWithInputFileText
        )
    }
}

/// A file and its modification time (tsgo `fileAndTime`; `None` is the
/// zero time).
#[derive(Clone, Debug, Default)]
struct FileAndTime {
    file: String,
    time: Option<SystemTime>,
}

/// tsgo `upToDateStatus.data`.
#[derive(Clone, Debug)]
enum StatusData {
    None,
    /// A file name (an input, an output, the build info) or a version.
    Text(String),
    /// tsgo `inputOutputName`.
    InputOutput {
        input: String,
        output: String,
    },
    /// tsgo `inputOutputFileAndTime`.
    Times {
        input: FileAndTime,
        output: FileAndTime,
    },
    /// tsgo `upstreamErrors`.
    Upstream {
        reference: String,
        has_upstream_errors: bool,
    },
}

#[derive(Clone, Debug)]
struct UpToDateStatus {
    kind: StatusKind,
    data: StatusData,
}

impl UpToDateStatus {
    fn new(kind: StatusKind) -> Self {
        Self {
            kind,
            data: StatusData::None,
        }
    }

    fn with_text(kind: StatusKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            data: StatusData::Text(text.into()),
        }
    }

    fn input_output(kind: StatusKind, input: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            kind,
            data: StatusData::InputOutput {
                input: input.into(),
                output: output.into(),
            },
        }
    }

    /// tsgo `inputOutputFileAndTime`.
    fn times(&self) -> Option<(&FileAndTime, &FileAndTime)> {
        match &self.data {
            StatusData::Times { input, output } => Some((input, output)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BuildKind {
    None,
    Pseudo,
    Program,
}

/// tsgo `buildInfoEntry`: a project's build info as read (or as just
/// written), by its canonical path, with the time it was written and the
/// time its declaration outputs last changed.
#[derive(Clone, Debug)]
struct BuildInfoEntry {
    info: Option<Arc<BuildInfo>>,
    path: String,
    mtime: Option<SystemTime>,
    dts_time: Option<SystemTime>,
}

/// tsgo `BuildTask`.
struct BuildTask {
    /// The normalized absolute config file name.
    config: String,
    /// `None`: the config file does not exist.
    plan: Option<ConfigRootPlan>,
    /// The effective compiler options (the config's with the command line's
    /// applied), when the config exists.
    options: Option<CompilerOptions>,
    /// (task, index of the reference in the config) of each upstream.
    upstream: Vec<(usize, usize)>,
    status: Option<UpToDateStatus>,
    build_info: Option<BuildInfoEntry>,
    errors: Vec<Diagnostic>,
    /// The task's report (tsgo `taskResult.builder`).
    output: String,
    exit_status: i32,
    build_kind: BuildKind,
    files_to_delete: Vec<String>,
    /// The project's statistics, when its options asked for them.
    statistics: Option<Statistics>,
    /// tsgo `pending`: the task builds (or reports its status) this cycle.
    pending: bool,
    /// tsgo `isInitialCycle`: the task was made by the first graph and has
    /// not been built yet.
    initial_cycle: bool,
    /// tsgo `dirty`: its configuration changed; the next graph parses it
    /// again.
    dirty: bool,
    /// A watch's downstream tasks (tsgo `downStream`).
    downstream: Vec<usize>,
    /// The package.json files its program looked up (tsgo `packageJsons`).
    package_jsons: Vec<String>,
}

impl BuildTask {
    fn new(config: String, plan: Option<ConfigRootPlan>, initial_cycle: bool) -> Self {
        let options = plan.as_ref().map(|plan| plan.compiler_options().clone());
        Self {
            config,
            plan,
            options,
            upstream: Vec::new(),
            status: None,
            build_info: None,
            errors: Vec::new(),
            output: String::new(),
            exit_status: 0,
            build_kind: BuildKind::None,
            files_to_delete: Vec::new(),
            statistics: None,
            pending: true,
            initial_cycle,
            dirty: false,
            downstream: Vec::new(),
            package_jsons: Vec::new(),
        }
    }

    /// tsgo `resetStatus`.
    fn reset_status(&mut self) {
        self.status = None;
        self.pending = true;
        self.errors.clear();
    }

    /// tsgo `buildOrCleanProject`: a fresh result for the cycle.
    fn reset_result(&mut self) {
        self.output.clear();
        self.exit_status = 0;
        self.build_kind = BuildKind::None;
        self.files_to_delete.clear();
        self.statistics = None;
    }

    fn plan_and_options(&self) -> Option<(&ConfigRootPlan, &CompilerOptions)> {
        Some((self.plan.as_ref()?, self.options.as_ref()?))
    }

    fn status(&self) -> &UpToDateStatus {
        self.status
            .as_ref()
            .expect("the status is computed before use")
    }
}

/// `tsc -b` over one command line. It owns what its builds read, so a
/// watch (`tsc -b --watch`) keeps it from one cycle to the next.
pub(crate) struct Orchestrator<'a> {
    /// The host of the current cycle (tsgo clears its caches between
    /// cycles; a watch takes a new one).
    host: Box<dyn CompilerHost + 'a>,
    fs: &'a dyn FileSystem,
    system: &'a dyn System,
    testing: Option<&'a dyn CommandLineTesting>,
    locale: crate::locale::Locale,
    catalog: LibraryCatalog,
    command: BuildCommand,
    /// Normalized, forward slashes.
    current_directory: String,
    current_directory_path: PathBuf,
    case_sensitive: bool,
    pretty: bool,
    tasks: Vec<BuildTask>,
    by_path: BTreeMap<String, usize>,
    /// Upstream first (tsgo `order`).
    order: Vec<usize>,
    /// The graph's errors (a cycle).
    errors: Vec<Diagnostic>,
    /// tsgo `host.mTimes`: the first observed time of each canonical path.
    mtimes: HashMap<String, Option<SystemTime>>,
    config_cache: ConfigExtendedCache,
    /// The texts of the configs parsed and the projects' diagnosed files,
    /// for the reports and the summary.
    sources: DiagnosticSourceMap,
    /// tsgo `graphGenerated`: a later graph reuses the tasks.
    graph_generated: bool,
}

/// What an orchestrator keeps from one run to the next (tsgo's keeps its
/// tasks), apart from the system it runs over: the API keeps it between
/// requests and attaches the system for each.
pub(crate) struct OrchestratorState {
    locale: crate::locale::Locale,
    catalog: LibraryCatalog,
    command: BuildCommand,
    current_directory: String,
    current_directory_path: PathBuf,
    case_sensitive: bool,
    pretty: bool,
    tasks: Vec<BuildTask>,
    by_path: BTreeMap<String, usize>,
    order: Vec<usize>,
    errors: Vec<Diagnostic>,
    mtimes: HashMap<String, Option<SystemTime>>,
    config_cache: ConfigExtendedCache,
    sources: DiagnosticSourceMap,
    graph_generated: bool,
}

impl<'a> Orchestrator<'a> {
    /// The orchestrator without its system ([`OrchestratorState`]).
    pub(crate) fn into_state(self) -> OrchestratorState {
        OrchestratorState {
            locale: self.locale,
            catalog: self.catalog,
            command: self.command,
            current_directory: self.current_directory,
            current_directory_path: self.current_directory_path,
            case_sensitive: self.case_sensitive,
            pretty: self.pretty,
            tasks: self.tasks,
            by_path: self.by_path,
            order: self.order,
            errors: self.errors,
            mtimes: self.mtimes,
            config_cache: self.config_cache,
            sources: self.sources,
            graph_generated: self.graph_generated,
        }
    }

    /// An orchestrator over `system` with a kept state; its host is new
    /// (tsgo's host caches last one cycle).
    pub(crate) fn with_state(
        system: &'a dyn System,
        testing: Option<&'a dyn CommandLineTesting>,
        state: OrchestratorState,
    ) -> Self {
        Self {
            host: system.compiler_host(),
            fs: system.fs(),
            system,
            testing,
            locale: state.locale,
            catalog: state.catalog,
            command: state.command,
            current_directory: state.current_directory,
            current_directory_path: state.current_directory_path,
            case_sensitive: state.case_sensitive,
            pretty: state.pretty,
            tasks: state.tasks,
            by_path: state.by_path,
            order: state.order,
            errors: state.errors,
            mtimes: state.mtimes,
            config_cache: state.config_cache,
            sources: state.sources,
            graph_generated: state.graph_generated,
        }
    }

    pub(crate) fn new(
        system: &'a dyn System,
        testing: Option<&'a dyn CommandLineTesting>,
        catalog: LibraryCatalog,
        command: BuildCommand,
        current_directory: PathBuf,
        pretty: bool,
    ) -> Self {
        let host = system.compiler_host();
        let case_sensitive = host.use_case_sensitive_file_names();
        let directory = current_directory
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
            .to_owned();
        Self {
            host,
            fs: system.fs(),
            system,
            testing,
            locale: crate::locale::Locale::English,
            catalog,
            command,
            current_directory: if directory.is_empty() {
                "/".to_owned()
            } else {
                directory
            },
            current_directory_path: current_directory,
            case_sensitive,
            pretty,
            tasks: Vec::new(),
            by_path: BTreeMap::new(),
            order: Vec::new(),
            errors: Vec::new(),
            mtimes: HashMap::new(),
            config_cache: ConfigExtendedCache::default(),
            sources: DiagnosticSourceMap::new(),
            graph_generated: false,
        }
    }

    /// tsgo `toPath`: the canonical form of a file name.
    fn to_path(&self, file_name: &str) -> String {
        canonical_emit_path(
            file_name.into(),
            self.current_directory.as_str().into(),
            self.case_sensitive,
        )
        .to_string_lossy()
        .into_owned()
    }

    /// tsgo `GetNormalizedAbsolutePath(file, directory)`.
    fn absolute(&self, file_name: &str, directory: &str) -> String {
        tsc_program::normalize_absolute_js_path_lexical(file_name.into(), Some(directory.into()))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| file_name.to_owned())
    }

    /// tsgo `relativeFileName`.
    fn relative(&self, file_name: &str) -> String {
        relative_file_name(file_name, &self.current_directory, self.case_sensitive)
            .to_string_lossy()
            .into_owned()
    }

    /// tsgo `host.GetMTime`: the modification time, observed once per
    /// path (`None` when the file does not exist).
    fn mtime(&mut self, file_name: &str) -> Option<SystemTime> {
        let path = self.to_path(file_name);
        if let Some(time) = self.mtimes.get(&path) {
            return *time;
        }
        let time = self.fs.modified_time(file_name);
        self.mtimes.insert(path, time);
        time
    }

    // ----- the graph -------------------------------------------------------

    /// tsgo `ResolvedProjectPaths`: the config file of each project named
    /// on the command line.
    fn resolved_project_paths(&self) -> Vec<String> {
        self.command
            .projects
            .iter()
            .map(|project| {
                let absolute = self.absolute(project, &self.current_directory);
                resolve_config_file_name_of_project_reference(absolute.as_str().into())
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    /// tsgo `createBuildTasks`: a task per config reachable from the
    /// projects, each config parsed once. A later graph (a watch's) keeps
    /// the old task of a config that did not change, and the build info of
    /// one that did.
    fn create_build_tasks(
        &mut self,
        configs: &[String],
        old: &mut BTreeMap<String, BuildTask>,
        initial: bool,
    ) {
        for config in configs {
            let path = self.to_path(config);
            if self.by_path.contains_key(&path) {
                continue;
            }
            let mut task = match old.remove(&path) {
                Some(mut existing) if !existing.dirty => {
                    existing.upstream.clear();
                    existing.downstream.clear();
                    existing
                }
                existing => {
                    let plan = self.parse_config(config);
                    let mut task = BuildTask::new(config.clone(), plan, initial);
                    task.build_info = existing.and_then(|existing| existing.build_info);
                    task
                }
            };
            task.config = config.clone();
            let references = task
                .plan
                .as_ref()
                .map(resolved_reference_paths)
                .unwrap_or_default();
            self.tasks.push(task);
            self.by_path.insert(path, self.tasks.len() - 1);
            self.create_build_tasks(&references, old, initial);
        }
    }

    /// tsgo `GetResolvedProjectReference`: the config parsed with the
    /// command line's options, or `None` when the file does not exist.
    fn parse_config(&mut self, config: &str) -> Option<ConfigRootPlan> {
        let bytes = self.host.read_file_js(config.into()).ok()??;
        let text = decode_host_text(bytes).ok()?;
        let adapter = CompilerConfigHost::new(&*self.host);
        let plan = parse_config_root_plan_with_command_line(
            &adapter,
            ConfigRootPlanRequest {
                file_name: JsString::from(config),
                text,
                base_path: JsString::from(self.current_directory.as_str()),
            },
            &self.command.command_line,
            &mut self.config_cache,
        )
        .ok()?
        .into_build_mode();
        for source in plan.extended_sources() {
            self.sources
                .insert(source.file_name.clone(), Arc::clone(source.snapshot()));
        }
        self.sources.insert(
            plan.source().file_name.clone(),
            Arc::clone(plan.source().snapshot()),
        );
        Some(plan)
    }

    /// tsgo `setupBuildTask`: the dependency order and the cycle check.
    fn setup_build_task(
        &mut self,
        config: &str,
        downstream: Option<usize>,
        in_circular_context: bool,
        completed: &mut BTreeSet<usize>,
        analyzing: &mut BTreeSet<usize>,
        circularity_stack: &mut Vec<String>,
    ) -> Option<usize> {
        let path = self.to_path(config);
        let task = *self.by_path.get(&path).expect("every config has a task");
        if !completed.contains(&task) {
            if analyzing.contains(&task) {
                if !in_circular_context {
                    self.errors.push(Diagnostic::new(
                        None,
                        None,
                        None,
                        MessageChain::new(
                            &gen::Project_references_may_not_form_a_circular_graph_Cycle_detected_0,
                            &[circularity_stack.join("\n")],
                        ),
                    ));
                }
                return None;
            }
            analyzing.insert(task);
            circularity_stack.push(config.to_owned());
            let references: Vec<(String, bool)> = self.tasks[task]
                .plan
                .as_ref()
                .map(|plan| {
                    plan.project_references()
                        .unwrap_or_default()
                        .iter()
                        .map(|reference| {
                            (
                                resolve_config_file_name_of_project_reference(
                                    reference.path.as_js(),
                                )
                                .to_string_lossy()
                                .into_owned(),
                                reference.circular == Some(true),
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (index, (reference, circular)) in references.iter().enumerate() {
                let upstream = self.setup_build_task(
                    reference,
                    Some(task),
                    in_circular_context || *circular,
                    completed,
                    analyzing,
                    circularity_stack,
                );
                if let Some(upstream) = upstream {
                    self.tasks[task].upstream.push((upstream, index));
                }
            }
            circularity_stack.pop();
            completed.insert(task);
            self.order.push(task);
        }
        if self.command.watch {
            if let Some(downstream) = downstream {
                self.tasks[task].downstream.push(downstream);
            }
        }
        Some(task)
    }

    /// tsgo `GenerateGraph` (and `GenerateGraphReusingOldTasks` once a
    /// graph exists).
    fn generate_graph(&mut self) {
        let initial = !self.graph_generated;
        let mut old = BTreeMap::new();
        for task in std::mem::take(&mut self.tasks) {
            old.insert(self.to_path(&task.config), task);
        }
        self.by_path.clear();
        self.order.clear();
        self.errors.clear();
        let projects = self.resolved_project_paths();
        self.create_build_tasks(&projects, &mut old, initial);
        let mut completed = BTreeSet::new();
        let mut analyzing = BTreeSet::new();
        let mut stack = Vec::new();
        for project in &projects {
            self.setup_build_task(
                project,
                None,
                false,
                &mut completed,
                &mut analyzing,
                &mut stack,
            );
        }
        self.graph_generated = true;
    }

    // ----- reporting -------------------------------------------------------

    /// tsgo `CreateBuilderStatusReporter`: a status line with the time
    /// (`HH:MM:SS AM - message` plainly, the time in grey brackets when
    /// pretty), followed by a blank line.
    fn status_line(&self, message: MessageChain) -> String {
        let text = message
            .text_in(self.locale.messages())
            .to_string_lossy()
            .into_owned();
        let time = status_time(self.system);
        let mut line = String::new();
        if let Some(testing) = self.testing {
            testing.on_build_status_report_start(&mut line);
        }
        if self.pretty {
            line.push_str(&format!("[\x1b[90m{time}\x1b[0m] {text}\n\n"));
        } else {
            line.push_str(&format!("{time} - {text}\n\n"));
        }
        if let Some(testing) = self.testing {
            testing.on_build_status_report_end(&mut line);
        }
        line
    }

    /// tsgo `CreateDiagnosticReporter` for a file-less diagnostic.
    fn diagnostic_text(&self, diagnostic: &Diagnostic) -> Result<String, CliError> {
        render_diagnostics(
            crate::cli::Format {
                current_directory: &self.current_directory_path,
                case_sensitive: self.case_sensitive,
                pretty: self.pretty,
                locale: self.locale,
            },
            &self.sources,
            std::slice::from_ref(diagnostic),
            false,
        )
    }

    fn report_task_diagnostic(
        &mut self,
        task: usize,
        diagnostic: Diagnostic,
    ) -> Result<(), CliError> {
        let text = self.diagnostic_text(&diagnostic)?;
        let task = &mut self.tasks[task];
        task.output.push_str(&text);
        task.errors.push(diagnostic);
        Ok(())
    }

    fn report_task_status(&mut self, task: usize, message: MessageChain) {
        let line = self.status_line(message);
        self.tasks[task].output.push_str(&line);
    }

    // ----- the build -------------------------------------------------------

    /// tsgo `Orchestrator.Start` (`tsc -b`), or the command line's clean.
    pub(crate) fn run(mut self, route: &mut CliRoute<'_>) -> Result<CliOutput, CliError> {
        if self.command.clean {
            self.generate_graph();
            return self.clean_command_line();
        }
        let result = self.start("", false, route)?;
        let mut stdout = result.stdout;
        if self.pretty {
            stdout.push_str(&render_error_summary(
                &self.current_directory_path,
                &self.sources,
                &result.errors,
                self.locale,
            )?);
        }
        stdout.push_str(&self.aggregate_statistics());
        Ok(CliOutput::new(stdout, result.status))
    }

    /// tsgo `start`: the graph (reusing the old tasks once there is one),
    /// the order of `project` (every project when empty), without the
    /// project itself for its references, and its build.
    fn start(
        &mut self,
        project: &str,
        only_references: bool,
        route: &mut CliRoute<'_>,
    ) -> Result<OrchestratorResult, CliError> {
        self.generate_graph();
        let Some(mut order) = self.build_order_for(project) else {
            return Ok(OrchestratorResult::status(EXIT_INVALID_PROJECT));
        };
        if only_references && self.errors.is_empty() {
            if project.is_empty() {
                return Ok(OrchestratorResult::status(EXIT_INVALID_PROJECT));
            }
            order.pop();
        }
        self.build_or_clean_order(&order, route)
    }

    /// tsgo `recheckAllProjects`: the projects of `project`'s order are
    /// checked and their configurations parsed again; the time cache and
    /// the caches start over.
    fn recheck_all_projects(&mut self, project: &str) {
        if !self.graph_generated {
            return;
        }
        let Some(order) = self.build_order_for(project) else {
            return;
        };
        for task in order {
            self.tasks[task].reset_status();
            self.reset_config(task);
        }
        self.mtimes.clear();
        self.reset_caches();
    }

    /// tsgo `getBuildOrderFor`: `project` and the projects upstream of it,
    /// in the order (every project when empty); `None` for a project the
    /// graph does not have.
    fn build_order_for(&self, project: &str) -> Option<Vec<usize>> {
        if project.is_empty() {
            return Some(self.order.clone());
        }
        let absolute = self.absolute(project, &self.current_directory);
        let config = resolve_config_file_name_of_project_reference(absolute.as_str().into())
            .to_string_lossy()
            .into_owned();
        let target = *self.by_path.get(&self.to_path(&config))?;
        let mut projects = BTreeSet::new();
        let mut pending = vec![target];
        while let Some(task) = pending.pop() {
            if projects.insert(task) {
                pending.extend(
                    self.tasks[task]
                        .upstream
                        .iter()
                        .map(|&(upstream, _)| upstream),
                );
            }
        }
        Some(
            self.order
                .iter()
                .copied()
                .filter(|task| projects.contains(task))
                .collect(),
        )
    }

    /// Every project's build (tsgo `buildOrClean`), as a watch cycle runs it.
    pub(crate) fn build_all(
        &mut self,
        route: &mut CliRoute<'_>,
    ) -> Result<OrchestratorResult, CliError> {
        let order = self.order.clone();
        self.build_or_clean_order(&order, route)
    }

    /// tsgo `buildOrCleanOrder` up to its report: the projects in `order`,
    /// their reports, the exit status, the errors reported (a task that is
    /// not pending reports its errors again) and the statistics.
    fn build_or_clean_order(
        &mut self,
        order: &[usize],
        route: &mut CliRoute<'_>,
    ) -> Result<OrchestratorResult, CliError> {
        let mut stdout = String::new();
        if self.command.verbose {
            let listed = order
                .iter()
                .map(|&task| format!("\r\n    * {}", self.relative(&self.tasks[task].config)))
                .collect::<String>();
            stdout.push_str(
                &self.status_line(MessageChain::new(&gen::Projects_in_this_build_0, &[listed])),
            );
        }
        let mut result = OrchestratorResult::default();
        if self.errors.is_empty() {
            result.statistics.projects = order.len();
            for &task in order {
                self.tasks[task].reset_result();
                self.build_project(task, route)?;
            }
            for &task in order {
                let task_state = &mut self.tasks[task];
                stdout.push_str(&task_state.output);
                result.status = result.status.max(task_state.exit_status);
                result.errors.extend(task_state.errors.iter().cloned());
                match task_state.build_kind {
                    BuildKind::Program => result.statistics.projects_built += 1,
                    BuildKind::Pseudo => result.statistics.timestamp_updates += 1,
                    _ => {}
                }
                debug_assert!(task_state.files_to_delete.is_empty());
            }
        } else {
            // Circularity errors prevent any project from being built.
            result.status = EXIT_PROJECT_REFERENCE_CYCLE;
            for diagnostic in &self.errors {
                stdout.push_str(&self.diagnostic_text(diagnostic)?);
            }
            result.errors.extend(self.errors.iter().cloned());
        }
        result.stdout = stdout;
        Ok(result)
    }

    /// tsgo `reportWithFilesToDelete`: the build's aggregate statistics
    /// under the command line's `--diagnostics` or `--extendedDiagnostics`.
    fn aggregate_statistics(&self) -> String {
        let requested = ["diagnostics", "extendedDiagnostics"].iter().any(|name| {
            matches!(
                self.command.command_line.typed_value_state(name),
                tsc_program::ConfigOptionValueState::Value(value) if value.as_bool() == Some(true)
            )
        });
        if !requested {
            return String::new();
        }
        let mut statistics = Statistics {
            projects: Some(ProjectCounts {
                in_scope: self.order.len(),
                built: self
                    .order
                    .iter()
                    .filter(|&&task| matches!(self.tasks[task].build_kind, BuildKind::Program))
                    .count(),
                timestamp_updates: self
                    .order
                    .iter()
                    .filter(|&&task| matches!(self.tasks[task].build_kind, BuildKind::Pseudo))
                    .count(),
            }),
            ..Statistics::default()
        };
        for &task in &self.order {
            if let Some(project) = &self.tasks[task].statistics {
                statistics.aggregate(project);
            }
        }
        statistics.total_time = self.system.since_start();
        statistics.report(self.testing)
    }

    /// tsgo `BuildTask.buildProject`: a pending task gets its status and
    /// is built or settled; one that is not pending (a watch's later cycle)
    /// reports its errors again.
    fn build_project(&mut self, task: usize, route: &mut CliRoute<'_>) -> Result<(), CliError> {
        if self.tasks[task].pending {
            let status = self.up_to_date_status(task);
            self.tasks[task].status = Some(status);
            self.report_up_to_date_status(task);
            if self.handle_status_that_doesnt_require_build(task)? {
                let config_diagnostics = self.tasks[task]
                    .plan
                    .as_ref()
                    .map(|plan| plan.diagnostics().cloned().collect::<Vec<_>>())
                    .unwrap_or_default();
                for diagnostic in config_diagnostics {
                    self.report_task_diagnostic(task, diagnostic)?;
                }
                if !self.tasks[task].errors.is_empty() {
                    self.tasks[task].exit_status = 1;
                }
            } else {
                let has_changed_dts_file = self.compile_and_emit(task, route)?;
                self.update_downstream(task, has_changed_dts_file);
            }
        } else if !self.tasks[task].errors.is_empty() {
            self.report_up_to_date_status(task);
            for diagnostic in self.tasks[task].errors.clone() {
                let text = self.diagnostic_text(&diagnostic)?;
                self.tasks[task].output.push_str(&text);
            }
        }
        // tsgo `unblockDownstream`.
        self.tasks[task].pending = false;
        self.tasks[task].initial_cycle = false;
        Ok(())
    }

    /// tsgo `updateDownstream`: in a watch's later cycle, a built project
    /// tells its downstream projects whether its declarations changed, and
    /// makes them pending.
    fn update_downstream(&mut self, task: usize, has_changed_dts_file: bool) {
        if self.tasks[task].initial_cycle {
            return;
        }
        if self.command.stop_build_on_errors && self.tasks[task].status().kind.is_error() {
            return;
        }
        let config = self.tasks[task].config.clone();
        let path = self.to_path(&config);
        for downstream in self.tasks[task].downstream.clone() {
            let status = self.tasks[downstream].status.clone();
            if let Some(status) = status {
                match status.kind {
                    StatusKind::UpToDate if !has_changed_dts_file => {
                        self.tasks[downstream].status = Some(UpToDateStatus {
                            kind: StatusKind::UpToDateWithUpstreamTypes,
                            data: status.data,
                        });
                    }
                    StatusKind::UpToDate
                    | StatusKind::UpToDateWithUpstreamTypes
                    | StatusKind::UpToDateWithInputFileText => {
                        if has_changed_dts_file {
                            self.tasks[downstream].status = Some(UpToDateStatus::input_output(
                                StatusKind::InputFileNewer,
                                config.clone(),
                                oldest_output_file_name(&status),
                            ));
                        }
                    }
                    StatusKind::UpstreamErrors => {
                        if let StatusData::Upstream { reference, .. } = &status.data {
                            let reference = resolve_config_file_name_of_project_reference(
                                reference.as_str().into(),
                            )
                            .to_string_lossy()
                            .into_owned();
                            if self.to_path(&reference) == path {
                                self.tasks[downstream].reset_status();
                            }
                        }
                    }
                    _ => {}
                }
            }
            self.tasks[downstream].pending = true;
        }
    }

    /// tsgo `handleStatusThatDoesntRequireBuild`: `true` when the project
    /// is not built.
    fn handle_status_that_doesnt_require_build(&mut self, task: usize) -> Result<bool, CliError> {
        let config = self.tasks[task].config.clone();
        let status = self.tasks[task].status().clone();
        match status.kind {
            StatusKind::UpToDate => {
                if self.command.dry {
                    self.report_task_status(
                        task,
                        MessageChain::new(&gen::Project_0_is_up_to_date, &[config]),
                    );
                }
                return Ok(true);
            }
            StatusKind::UpstreamErrors => {
                if let StatusData::Upstream {
                    reference,
                    has_upstream_errors,
                } = &status.data
                {
                    if self.command.verbose {
                        let message = if *has_upstream_errors {
                            &gen::Skipping_build_of_project_0_because_its_dependency_1_was_not_built
                        } else {
                            &gen::Skipping_build_of_project_0_because_its_dependency_1_has_errors
                        };
                        let message = MessageChain::new(
                            message,
                            &[self.relative(&config), self.relative(reference)],
                        );
                        self.report_task_status(task, message);
                    }
                }
                return Ok(true);
            }
            StatusKind::Solution => return Ok(true),
            StatusKind::ConfigFileNotFound => {
                let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(&gen::File_0_not_found, &[config]),
                );
                self.report_task_diagnostic(task, diagnostic)?;
                return Ok(true);
            }
            _ => {}
        }
        if status.kind.is_pseudo_build() {
            if self.command.dry {
                self.report_task_status(
                    task,
                    MessageChain::new(
                        &gen::A_non_dry_build_would_update_timestamps_for_output_of_project_0,
                        &[config],
                    ),
                );
                self.tasks[task].status = Some(UpToDateStatus::new(StatusKind::UpToDate));
                return Ok(true);
            }
            self.update_time_stamps(task, &[], &gen::Updating_output_timestamps_of_project_0);
            let data = status.data.clone();
            self.tasks[task].status = Some(UpToDateStatus {
                kind: StatusKind::UpToDate,
                data,
            });
            self.tasks[task].build_kind = BuildKind::Pseudo;
            return Ok(true);
        }
        if self.command.dry {
            self.report_task_status(
                task,
                MessageChain::new(&gen::A_non_dry_build_would_build_project_0, &[config]),
            );
            self.tasks[task].status = Some(UpToDateStatus::new(StatusKind::UpToDate));
            return Ok(true);
        }
        Ok(false)
    }

    /// tsgo `compileAndEmit`: the project built through the ordinary
    /// pipeline with its old build info, then its unchanged outputs
    /// touched.
    fn compile_and_emit(
        &mut self,
        task: usize,
        route: &mut CliRoute<'_>,
    ) -> Result<bool, CliError> {
        self.tasks[task].errors.clear();
        let config = self.tasks[task].config.clone();
        if self.command.verbose {
            let message = MessageChain::new(&gen::Building_project_0, &[self.relative(&config)]);
            self.report_task_status(task, message);
        }
        let (plan, options) = {
            let task = &self.tasks[task];
            let (plan, options) = task
                .plan_and_options()
                .expect("a project without a config is never built");
            (plan.clone(), options.clone())
        };
        let old_info = if self.command.force {
            None
        } else {
            self.tasks[task]
                .build_info
                .as_ref()
                .and_then(|entry| entry.info.clone())
        };
        // tsgo restores the time of a declaration file written although
        // only its map changed: the times before the build.
        let composite = options.composite == Some(true);
        let outputs = self.output_names(task);
        let declaration_times: HashMap<String, Option<SystemTime>> = if composite {
            outputs
                .iter()
                .filter(|output| is_declaration_output(output))
                .map(|output| (output.clone(), self.fs.modified_time(output)))
                .collect()
        } else {
            HashMap::new()
        };
        let run: BuildProjectRun = run_config_for_build(
            &*self.host,
            &self.current_directory_path,
            &self.catalog,
            &plan,
            old_info.as_deref(),
            route,
        )?;
        // tsgo `writeFile`: a watch keeps the time it wrote an output of a
        // project without build info state, and the build info's.
        let build_info_name = self.build_info_name(task);
        let mut build_info_written_at = None;
        for (file, time) in &run.write_times {
            if build_info_name.as_deref() == Some(file.as_str()) {
                build_info_written_at = Some(*time);
            } else if self.stores_output_time_stamps(task) {
                self.store_mtime(file, *time);
            }
        }
        // tsgo `OnEmittedFiles` updates the build's time cache of the files
        // a test harness stamped.
        for (file, time) in &run.stamped {
            let path = self.to_path(file);
            if let Some(cached) = self.mtimes.get_mut(&path) {
                *cached = Some(*time);
            }
        }
        for output in &run.declarations_differing_only_in_map {
            if let Some(Some(time)) = declaration_times.get(output) {
                let _ = self.fs.set_modified_time(output, *time);
            }
        }
        self.sources.extend(
            run.sources
                .iter()
                .map(|(name, text)| (name.clone(), Arc::clone(text))),
        );
        let emitted = run.emitted_files.clone();
        let status_kind = self.tasks[task].status().kind;
        {
            let entry = &mut self.tasks[task];
            entry.output.push_str(&run.stdout);
            entry.exit_status = run.exit_code;
            entry.errors.extend(run.diagnostics.iter().cloned());
            entry.statistics = run.statistics;
            entry.package_jsons = run.package_json_lookups.clone();
        }
        if let Some(name) = &build_info_name {
            if emitted.iter().any(|file| file == name) {
                self.on_build_info_emit(
                    task,
                    name,
                    run.has_changed_dts_file,
                    build_info_written_at,
                );
            }
        }
        if (options.no_emit_on_error != Some(true) || run.diagnostics.is_empty())
            && (!emitted.is_empty() || status_kind != StatusKind::OutOfDateBuildInfoWithErrors)
        {
            self.update_time_stamps(
                task,
                &emitted,
                &gen::Updating_unchanged_output_timestamps_of_project_0,
            );
        }
        self.tasks[task].build_kind = BuildKind::Program;
        if run.exit_code == 1 || run.exit_code == 2 {
            self.tasks[task].status = Some(UpToDateStatus::new(StatusKind::BuildErrors));
        } else {
            let oldest = emitted
                .first()
                .cloned()
                .or_else(|| outputs.first().cloned())
                .unwrap_or_default();
            self.tasks[task].status = Some(UpToDateStatus::with_text(StatusKind::UpToDate, oldest));
        }
        Ok(run.has_changed_dts_file)
    }

    /// tsgo `onBuildInfoEmit`: the entry of the build info just written.
    fn on_build_info_emit(
        &mut self,
        task: usize,
        file_name: &str,
        has_changed_dts_file: bool,
        written_at: Option<SystemTime>,
    ) {
        let now = written_at.unwrap_or_else(|| self.system.now());
        // The build info just written, from the file system itself: a
        // host may still hold what it saw before the build (tsgo keeps the
        // object it wrote).
        let info = self
            .fs
            .read(file_name)
            .ok()
            .and_then(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                BuildInfo::from_json(text.strip_prefix('\u{feff}').unwrap_or(&text))
            })
            .map(Arc::new);
        let previous = self.tasks[task].build_info.as_ref();
        let dts_time = if has_changed_dts_file {
            Some(now)
        } else {
            previous.and_then(|entry| entry.dts_time)
        };
        let path = self.to_path(file_name);
        self.tasks[task].build_info = Some(BuildInfoEntry {
            info,
            path,
            mtime: Some(now),
            dts_time,
        });
    }

    /// tsgo `updateTimeStamps`: the outputs not written by this build get
    /// the current time (only the build info of an incremental or `noEmit`
    /// project).
    fn update_time_stamps(
        &mut self,
        task: usize,
        emitted: &[String],
        message: &'static tsc_diagnostics::DiagnosticMessage,
    ) {
        let config = self.tasks[task].config.clone();
        let Some(options) = self.tasks[task].options.clone() else {
            return;
        };
        let now = self.system.now();
        let build_info_name = self.build_info_name(task);
        let mut files = Vec::new();
        if options.no_emit != Some(true) && !tsc_incremental::options::is_incremental(&options) {
            files.extend(self.output_names(task));
        }
        files.extend(build_info_name.clone());
        let mut reported = false;
        for file in files {
            if emitted.contains(&file) {
                continue;
            }
            if !reported && self.command.verbose {
                let relative = self.relative(&config);
                self.report_task_status(task, MessageChain::new(message, &[relative]));
                reported = true;
            }
            if self.fs.set_modified_time(&file, now).is_ok() {
                if build_info_name.as_deref() == Some(file.as_str()) {
                    if let Some(entry) = &mut self.tasks[task].build_info {
                        entry.mtime = Some(now);
                    }
                } else if self.stores_output_time_stamps(task) {
                    self.store_mtime(&file, now);
                }
            }
        }
    }

    // ----- clean -----------------------------------------------------------

    /// tsgo `Orchestrator.clean`.
    /// The command line's clean (tsgo `buildOrCleanOrder` with every
    /// project cleaned by `cleanProject`): a missing configuration fails
    /// it, and only a dry run lists the files.
    fn clean_command_line(mut self) -> Result<CliOutput, CliError> {
        let mut stdout = String::new();
        if !self.errors.is_empty() {
            for diagnostic in &self.errors {
                stdout.push_str(&self.diagnostic_text(diagnostic)?);
            }
            if self.pretty {
                stdout.push_str(&render_error_summary(
                    &self.current_directory_path,
                    &self.sources,
                    &self.errors,
                    self.locale,
                )?);
            }
            return Ok(CliOutput::new(stdout, EXIT_PROJECT_REFERENCE_CYCLE));
        }
        let order = self.order.clone();
        let pass = self.clean_order(&order)?;
        stdout.push_str(&pass.stdout);
        if self.pretty {
            stdout.push_str(&render_error_summary(
                &self.current_directory_path,
                &self.sources,
                &pass.errors,
                self.locale,
            )?);
        }
        if self.command.dry && !pass.files_to_delete.is_empty() {
            let listed = pass
                .files_to_delete
                .iter()
                .map(|file| format!("\r\n * {file}"))
                .collect::<String>();
            stdout.push_str(&self.status_line(MessageChain::new(
                &gen::A_non_dry_build_would_delete_the_following_files_0,
                &[listed],
            )));
        }
        let exit_status = if pass.missing_config { 1 } else { 0 };
        Ok(CliOutput::new(stdout, exit_status))
    }

    /// tsgo `Orchestrator.clean` (the API's `Clean` and `CleanReferences`):
    /// the order of `project` (every project when empty), without its
    /// last project for the references, cleaned over the configurations
    /// last parsed; every file deleted or to delete is listed.
    fn clean(
        &mut self,
        project: &str,
        only_references: bool,
    ) -> Result<OrchestratorResult, CliError> {
        if !self.graph_generated {
            self.generate_graph();
        }
        if !self.errors.is_empty() {
            return Ok(OrchestratorResult {
                status: EXIT_PROJECT_REFERENCE_CYCLE,
                errors: self.errors.clone(),
                ..OrchestratorResult::default()
            });
        }
        let Some(mut order) = self.build_order_for(project) else {
            return Ok(OrchestratorResult::status(EXIT_INVALID_PROJECT));
        };
        if only_references {
            order.pop();
        }
        let pass = self.clean_order(&order)?;
        Ok(OrchestratorResult {
            stdout: pass.stdout,
            status: 0,
            errors: pass.errors,
            statistics: BuildStatistics {
                projects: order.len(),
                ..BuildStatistics::default()
            },
            files_to_delete: pass.files_to_delete,
        })
    }

    /// The outputs and build info of the projects in `order` that are not
    /// their inputs, deleted (under `--dry`, only listed); a project whose
    /// outputs went is checked again (tsgo `cleanProject` and `clean`).
    fn clean_order(&mut self, order: &[usize]) -> Result<CleanPass, CliError> {
        let mut pass = CleanPass::default();
        for &task in order {
            if self.tasks[task].plan.is_none() {
                let diagnostic = Diagnostic::new(
                    None,
                    None,
                    None,
                    MessageChain::new(&gen::File_0_not_found, &[self.tasks[task].config.clone()]),
                );
                pass.stdout.push_str(&self.diagnostic_text(&diagnostic)?);
                pass.errors.push(diagnostic);
                pass.missing_config = true;
                continue;
            }
            let inputs: BTreeSet<String> = self.tasks[task]
                .plan
                .as_ref()
                .map(|plan| {
                    plan.file_names()
                        .iter()
                        .map(|name| self.to_path(&name.to_string_lossy()))
                        .collect()
                })
                .unwrap_or_default();
            let mut outputs = self.output_names(task);
            outputs.extend(self.build_info_name(task));
            let mut deleted = false;
            for output in outputs {
                if inputs.contains(&self.to_path(&output)) || !self.fs.file_exists(&output) {
                    continue;
                }
                pass.files_to_delete.push(output.clone());
                if self.command.dry {
                    continue;
                }
                match self.fs.remove_file(&output) {
                    Ok(()) => deleted = true,
                    Err(_) => {
                        let diagnostic = Diagnostic::new(
                            None,
                            None,
                            None,
                            MessageChain::new(&gen::Failed_to_delete_file_0, &[output]),
                        );
                        pass.stdout.push_str(&self.diagnostic_text(&diagnostic)?);
                        pass.errors.push(diagnostic);
                    }
                }
            }
            if deleted {
                self.tasks[task].reset_status();
                self.tasks[task].build_info = None;
            }
        }
        Ok(pass)
    }

    // ----- names -----------------------------------------------------------

    /// tsgo `GetOutputFileNames` of the task's project.
    fn output_names(&self, task: usize) -> Vec<String> {
        let Some((plan, options)) = self.tasks[task].plan_and_options() else {
            return Vec::new();
        };
        output_file_names(
            options,
            Some(plan.config_file_name()),
            plan.file_names(),
            self.current_directory.as_str().into(),
            self.case_sensitive,
        )
        .iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
    }

    /// tsgo `GetBuildInfoFileName` of the task's project under the build.
    fn build_info_name(&self, task: usize) -> Option<String> {
        let (plan, options) = self.tasks[task].plan_and_options()?;
        build_info_file_name_in_build_mode(
            options,
            Some(plan.config_file_name()),
            self.current_directory.as_str().into(),
            self.case_sensitive,
        )
        .map(|name| name.to_string_lossy().into_owned())
    }

    /// tsgo `loadOrStoreBuildInfo`: the build info read once.
    fn load_build_info(&mut self, task: usize, build_info_name: &str) -> BuildInfoEntry {
        let path = self.to_path(build_info_name);
        if let Some(entry) = &self.tasks[task].build_info {
            if entry.path == path {
                return entry.clone();
            }
        }
        let info = self
            .host
            .read_file_js(build_info_name.into())
            .ok()
            .flatten()
            .and_then(|bytes| {
                let text = String::from_utf8_lossy(&bytes);
                BuildInfo::from_json(text.strip_prefix('\u{feff}').unwrap_or(&text))
            })
            .map(Arc::new);
        let mtime = if info.is_some() {
            self.mtime(build_info_name)
        } else {
            None
        };
        let entry = BuildInfoEntry {
            info,
            path,
            mtime,
            dts_time: None,
        };
        self.tasks[task].build_info = Some(entry.clone());
        entry
    }

    /// tsgo `getLatestChangedDtsMTime` of an upstream task.
    fn latest_changed_dts_mtime(&mut self, task: usize) -> Option<SystemTime> {
        let entry = self.tasks[task].build_info.clone()?;
        if let Some(time) = entry.dts_time {
            return Some(time);
        }
        let latest = entry
            .info
            .as_ref()
            .map(|info| info.latest_changed_dts_file.clone())
            .unwrap_or_default();
        let directory = directory_of(&entry.path);
        let file = self.absolute(&latest, &directory);
        let time = self.mtime(&file);
        if let Some(entry) = &mut self.tasks[task].build_info {
            entry.dts_time = time;
        }
        time
    }

    // ----- the up-to-date status --------------------------------------------

    /// The version of a file's current text (tsgo `ComputeHash`).
    fn current_version(&self, file_name: &str) -> Option<String> {
        let bytes = self.host.read_file_js(file_name.into()).ok()??;
        let text = decode_host_text(bytes).ok()?;
        Some(compute_hash_with_text(&text, self.testing.is_some()))
    }

    /// tsgo `getUpToDateStatus`.
    fn up_to_date_status(&mut self, task: usize) -> UpToDateStatus {
        if let Some(status) = &self.tasks[task].status {
            return status.clone();
        }
        let Some((plan, options)) = self.tasks[task]
            .plan_and_options()
            .map(|(plan, options)| (plan.clone(), options.clone()))
        else {
            return UpToDateStatus::new(StatusKind::ConfigFileNotFound);
        };
        if plan.file_names().is_empty() && plan.project_references().is_some() {
            return UpToDateStatus::new(StatusKind::Solution);
        }
        let config = self.tasks[task].config.clone();
        let upstream = self.tasks[task].upstream.clone();
        let references: Vec<String> = plan
            .project_references()
            .unwrap_or_default()
            .iter()
            .map(|reference| reference.path.to_string_lossy().into_owned())
            .collect();
        for &(upstream_task, reference) in &upstream {
            if self.command.stop_build_on_errors
                && self.tasks[upstream_task].status().kind.is_error()
            {
                return UpToDateStatus {
                    kind: StatusKind::UpstreamErrors,
                    data: StatusData::Upstream {
                        reference: references.get(reference).cloned().unwrap_or_default(),
                        has_upstream_errors: self.tasks[upstream_task].status().kind
                            == StatusKind::UpstreamErrors,
                    },
                };
            }
        }
        if self.command.force {
            return UpToDateStatus::new(StatusKind::ForceBuild);
        }

        // The build info.
        let build_info_path = self
            .build_info_name(task)
            .expect("a config file names a build info under tsc -b");
        let build_info_directory = directory_of(&build_info_path);
        let entry = self.load_build_info(task, &build_info_path);
        let Some(info) = entry.info.clone() else {
            return UpToDateStatus::with_text(StatusKind::OutputMissing, build_info_path);
        };
        if !info.is_valid_version() {
            return UpToDateStatus::with_text(
                StatusKind::TsVersionOutputOfDate,
                info.version.clone(),
            );
        }
        let no_check = options.no_check == Some(true);
        if info.errors || (!no_check && (info.semantic_errors || info.check_pending)) {
            return UpToDateStatus::with_text(
                StatusKind::OutOfDateBuildInfoWithErrors,
                build_info_path,
            );
        }
        let incremental = tsc_incremental::options::is_incremental(&options);
        let emit_declarations =
            options.declaration == Some(true) || options.composite == Some(true);
        if incremental {
            if !info.is_incremental() {
                return UpToDateStatus::with_text(StatusKind::OutOfDateOptions, build_info_path);
            }
            if (emit_declarations && !info.emit_diagnostics_per_file.is_empty())
                || (!no_check
                    && (!info.change_file_set.is_empty()
                        || !info.semantic_diagnostics_per_file.is_empty()))
            {
                return UpToDateStatus::with_text(
                    StatusKind::OutOfDateBuildInfoWithErrors,
                    build_info_path,
                );
            }
            if options.no_emit != Some(true)
                && (!info.change_file_set.is_empty()
                    || !info.affected_files_pending_emit.is_empty())
            {
                return UpToDateStatus::with_text(
                    StatusKind::OutOfDateBuildInfoWithPendingEmit,
                    build_info_path,
                );
            }
            let absolute = |name: &str| self.absolute(name, &build_info_directory);
            if info.is_emit_pending(&options, &absolute) {
                return UpToDateStatus::with_text(StatusKind::OutOfDateOptions, build_info_path);
            }
        }

        let mut input_text_unchanged = false;
        let mut oldest_output = FileAndTime {
            file: build_info_path.clone(),
            time: entry.mtime,
        };
        let mut newest_input = FileAndTime::default();
        let mut seen_roots: BTreeSet<String> = BTreeSet::new();
        let canonical = |name: &str, directory: &str| {
            let absolute = self.absolute(name, directory);
            self.to_path(&absolute)
        };
        let reader = info.root_info_reader(&build_info_directory, &canonical);
        let file_names: Vec<String> = plan
            .file_names()
            .iter()
            .map(|name| name.to_string_lossy().into_owned())
            .collect();
        for input in &file_names {
            let Some(input_time) = self.mtime(input) else {
                return UpToDateStatus::with_text(StatusKind::InputFileMissing, input.clone());
            };
            let input_path = self.to_path(input);
            if after(Some(input_time), oldest_output.time) {
                let mut version = String::new();
                let mut current_version = None;
                if info.is_incremental() {
                    if let Some((Some(file_info), resolved)) = reader.file_info(&input_path) {
                        if !file_info.version().is_empty() {
                            version = file_info.version().to_owned();
                            current_version = self.current_version(resolved);
                            if current_version.as_deref() == Some(version.as_str()) {
                                input_text_unchanged = true;
                            }
                        }
                    }
                }
                if version.is_empty() || current_version.as_deref() != Some(version.as_str()) {
                    return UpToDateStatus::input_output(
                        StatusKind::InputFileNewer,
                        input.clone(),
                        build_info_path.clone(),
                    );
                }
            }
            if after(Some(input_time), newest_input.time) {
                newest_input = FileAndTime {
                    file: input.clone(),
                    time: Some(input_time),
                };
            }
            seen_roots.insert(input_path);
        }
        for root in reader.roots() {
            if !seen_roots.contains(root) {
                return UpToDateStatus::input_output(
                    StatusKind::OutOfDateRoots,
                    root.to_owned(),
                    build_info_path.clone(),
                );
            }
        }
        if info.is_incremental() {
            let resolved_roots: BTreeSet<String> = reader
                .roots()
                .filter_map(|root| {
                    reader
                        .file_info(root)
                        .map(|(_, resolved)| resolved.to_owned())
                })
                .collect();
            for (index, file_info) in info.file_infos.iter().enumerate() {
                let Some(name) = info.file_names.get(index) else {
                    continue;
                };
                // Lib files bundled with the compiler change only with its
                // version, which the build info's version verified.
                if is_default_library_name(name) {
                    continue;
                }
                let input = self.absolute(name, &build_info_directory);
                let input_path = self.to_path(&input);
                if seen_roots.contains(&input_path) || resolved_roots.contains(&input_path) {
                    continue;
                }
                let Some(input_time) = self.mtime(&input) else {
                    // A file of the program is missing (a dependency was
                    // removed).
                    return UpToDateStatus::with_text(StatusKind::InputFileMissing, input);
                };
                if after(Some(input_time), oldest_output.time) {
                    let version = file_info.version();
                    let current_version = if version.is_empty() {
                        None
                    } else {
                        self.current_version(&input)
                    };
                    if version.is_empty() || current_version.as_deref() != Some(version) {
                        return UpToDateStatus::input_output(
                            StatusKind::InputFileNewer,
                            input,
                            build_info_path.clone(),
                        );
                    }
                    input_text_unchanged = true;
                }
            }
        }

        if !incremental {
            for output in self.output_names(task) {
                let Some(output_time) = self.mtime(&output) else {
                    return UpToDateStatus::with_text(StatusKind::OutputMissing, output);
                };
                if before(Some(output_time), newest_input.time) {
                    return UpToDateStatus::input_output(
                        StatusKind::InputFileNewer,
                        newest_input.file.clone(),
                        output,
                    );
                }
                if before(Some(output_time), oldest_output.time) {
                    oldest_output = FileAndTime {
                        file: output,
                        time: Some(output_time),
                    };
                }
            }
        }

        let mut reference_dts_unchanged = false;
        for &(upstream_task, reference) in &upstream {
            let upstream_status = self.tasks[upstream_task].status().clone();
            if upstream_status.kind == StatusKind::Solution {
                continue;
            }
            // An upstream whose newest input is older than our oldest
            // output cannot make us out of date.
            if let Some((input, _)) = upstream_status.times() {
                if input.time.is_some() && before(input.time, oldest_output.time) {
                    continue;
                }
            }
            let reference_path = references.get(reference).cloned().unwrap_or_default();
            if self.has_conflicting_build_info(task, upstream_task) {
                return UpToDateStatus::input_output(
                    StatusKind::InputFileNewer,
                    reference_path,
                    oldest_output.file.clone(),
                );
            }
            // Only the declaration files of the upstream changed, and we
            // built after them: a pseudo build suffices.
            let newest_dts_change = self.latest_changed_dts_mtime(upstream_task);
            if newest_dts_change.is_some() && before(newest_dts_change, oldest_output.time) {
                reference_dts_unchanged = true;
                continue;
            }
            return UpToDateStatus::input_output(
                StatusKind::InputFileNewer,
                reference_path,
                oldest_output.file.clone(),
            );
        }

        let mut inputs_to_check = vec![config.clone()];
        inputs_to_check.extend(
            plan.extended_source_files()
                .iter()
                .map(|name| name.to_string_lossy().into_owned()),
        );
        for input in inputs_to_check {
            let input_time = self.mtime(&input);
            if after(input_time, oldest_output.time) {
                return UpToDateStatus::input_output(
                    StatusKind::InputFileNewer,
                    input,
                    oldest_output.file.clone(),
                );
            }
        }
        for package_json in &info.package_jsons {
            let package_json = self.absolute(package_json, &build_info_directory);
            let Some(time) = self.mtime(&package_json) else {
                return UpToDateStatus::with_text(StatusKind::InputFileMissing, package_json);
            };
            if after(Some(time), oldest_output.time) {
                return UpToDateStatus::input_output(
                    StatusKind::InputFileNewer,
                    package_json,
                    oldest_output.file.clone(),
                );
            }
        }
        for package_json in &info.missing_package_jsons {
            let package_json = self.absolute(package_json, &build_info_directory);
            if self.mtime(&package_json).is_some() {
                return UpToDateStatus::input_output(
                    StatusKind::InputFileNewer,
                    package_json,
                    oldest_output.file.clone(),
                );
            }
        }

        UpToDateStatus {
            kind: if reference_dts_unchanged {
                StatusKind::UpToDateWithUpstreamTypes
            } else if input_text_unchanged {
                StatusKind::UpToDateWithInputFileText
            } else {
                StatusKind::UpToDate
            },
            data: StatusData::Times {
                input: newest_input,
                output: oldest_output,
            },
        }
    }

    /// tsgo `hasConflictingBuildInfo`: the task and its upstream share a
    /// build info file.
    fn has_conflicting_build_info(&self, task: usize, upstream: usize) -> bool {
        match (
            &self.tasks[task].build_info,
            &self.tasks[upstream].build_info,
        ) {
            (Some(mine), Some(theirs)) => mine.path == theirs.path,
            _ => false,
        }
    }

    /// tsgo `reportUpToDateStatus` (verbose only).
    fn report_up_to_date_status(&mut self, task: usize) {
        if !self.command.verbose {
            return;
        }
        let config = self.relative(&self.tasks[task].config.clone());
        let status = self.tasks[task].status().clone();
        let text = |data: &StatusData| match data {
            StatusData::Text(text) => text.clone(),
            _ => String::new(),
        };
        let message = match status.kind {
            StatusKind::ConfigFileNotFound => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_config_file_does_not_exist,
                &[config],
            ),
            StatusKind::UpstreamErrors => {
                let StatusData::Upstream {
                    reference,
                    has_upstream_errors,
                } = &status.data
                else {
                    return;
                };
                let message = if *has_upstream_errors {
                    &gen::Project_0_can_t_be_built_because_its_dependency_1_was_not_built
                } else {
                    &gen::Project_0_can_t_be_built_because_its_dependency_1_has_errors
                };
                MessageChain::new(message, &[config, self.relative(reference)])
            }
            StatusKind::BuildErrors => {
                MessageChain::new(&gen::Project_0_is_out_of_date_because_it_has_errors, &[config])
            }
            StatusKind::UpToDate => {
                // A project that was built in this run reports nothing more.
                let Some((input, output)) = status.times() else {
                    return;
                };
                MessageChain::new(
                    &gen::Project_0_is_up_to_date_because_newest_input_1_is_older_than_output_2,
                    &[config, self.relative(&input.file), self.relative(&output.file)],
                )
            }
            StatusKind::UpToDateWithUpstreamTypes => MessageChain::new(
                &gen::Project_0_is_up_to_date_with_d_ts_files_from_its_dependencies,
                &[config],
            ),
            StatusKind::UpToDateWithInputFileText => MessageChain::new(
                &gen::Project_0_is_up_to_date_but_needs_to_update_timestamps_of_output_files_that_are_older_than_input_files,
                &[config],
            ),
            StatusKind::InputFileMissing => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_input_1_does_not_exist,
                &[config, self.relative(&text(&status.data))],
            ),
            StatusKind::OutputMissing => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_output_file_1_does_not_exist,
                &[config, self.relative(&text(&status.data))],
            ),
            StatusKind::InputFileNewer => {
                let StatusData::InputOutput { input, output } = &status.data else {
                    return;
                };
                MessageChain::new(
                    &gen::Project_0_is_out_of_date_because_output_1_is_older_than_input_2,
                    &[config, self.relative(output), self.relative(input)],
                )
            }
            StatusKind::OutOfDateBuildInfoWithPendingEmit => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_buildinfo_file_1_indicates_that_some_of_the_changes_were_not_emitted,
                &[config, self.relative(&text(&status.data))],
            ),
            StatusKind::OutOfDateBuildInfoWithErrors => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_buildinfo_file_1_indicates_that_program_needs_to_report_errors,
                &[config, self.relative(&text(&status.data))],
            ),
            StatusKind::OutOfDateOptions => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_buildinfo_file_1_indicates_there_is_change_in_compilerOptions,
                &[config, self.relative(&text(&status.data))],
            ),
            StatusKind::OutOfDateRoots => {
                let StatusData::InputOutput { input, output } = &status.data else {
                    return;
                };
                MessageChain::new(
                    &gen::Project_0_is_out_of_date_because_buildinfo_file_1_indicates_that_file_2_was_root_file_of_compilation_but_not_any_more,
                    &[config, self.relative(output), self.relative(input)],
                )
            }
            StatusKind::TsVersionOutputOfDate => MessageChain::new(
                &gen::Project_0_is_out_of_date_because_output_for_it_was_generated_with_version_1_that_differs_with_current_version_2,
                &[
                    config,
                    text(&status.data),
                    tsc_incremental::VERSION.to_owned(),
                ],
            ),
            StatusKind::ForceBuild => {
                MessageChain::new(&gen::Project_0_is_being_forcibly_rebuilt, &[config])
            }
            StatusKind::Solution => return,
        };
        self.report_task_status(task, message);
    }
}

// ----- watch (tsgo's Orchestrator as a watcher) ----------------------------

impl<'a> Orchestrator<'a> {
    pub(crate) fn set_locale(&mut self, locale: crate::locale::Locale) {
        self.locale = locale;
    }

    /// The command line's options keep the screen (tsgo's watch status
    /// reporter reads the command line's compiler options).
    pub(crate) fn keeps_screen(&self) -> bool {
        ["preserveWatchOutput", "diagnostics", "extendedDiagnostics"]
            .iter()
            .any(|name| {
                matches!(
                    self.command.command_line.typed_value_state(name),
                    tsc_program::ConfigOptionValueState::Value(value) if value.as_bool() == Some(true)
                )
            })
    }

    pub(crate) fn case_sensitive(&self) -> bool {
        self.case_sensitive
    }

    /// The first graph (tsgo `GenerateGraph` in `start`).
    pub(crate) fn generate_initial_graph(&mut self) {
        self.generate_graph();
    }

    /// tsgo `GenerateGraphReusingOldTasks`.
    pub(crate) fn regenerate_graph(&mut self) {
        self.generate_graph();
    }

    /// tsgo `aggregate statistics` of a watch cycle's report.
    pub(crate) fn statistics_report(&self) -> String {
        self.aggregate_statistics()
    }

    /// tsgo `canUpdateJsDtsOutputTimestamps`.
    fn can_update_js_dts_output_timestamps(&self, task: usize) -> bool {
        self.tasks[task].options.as_ref().is_some_and(|options| {
            options.no_emit != Some(true) && !tsc_incremental::options::is_incremental(options)
        })
    }

    /// tsgo `storeOutputTimeStamp`: a watch keeps the times it wrote the
    /// outputs of a project without build info state.
    fn stores_output_time_stamps(&self, task: usize) -> bool {
        self.command.watch
            && self.tasks[task]
                .options
                .as_ref()
                .is_some_and(|options| !tsc_incremental::options::is_incremental(options))
    }

    /// tsgo `host.storeMTime`.
    fn store_mtime(&mut self, file_name: &str, time: SystemTime) {
        let path = self.to_path(file_name);
        self.mtimes.insert(path, Some(time));
    }

    /// tsgo `updateWatch`: the next cycle's time cache starts empty but for
    /// the outputs of a project that touches them, whose stored times stay.
    pub(crate) fn update_watch(&mut self) {
        let old = std::mem::take(&mut self.mtimes);
        for position in 0..self.order.len() {
            let task = self.order[position];
            if !self.can_update_js_dts_output_timestamps(task) {
                continue;
            }
            for output in self.output_names(task) {
                let path = self.to_path(&output);
                if let Some(time) = old.get(&path) {
                    self.mtimes.insert(path, *time);
                }
            }
        }
    }

    /// tsgo `resetCaches`: what lasts one cycle (the host's caches and the
    /// extended configurations).
    pub(crate) fn reset_caches(&mut self) {
        self.host = self.system.compiler_host();
        self.config_cache = ConfigExtendedCache::default();
    }

    /// tsgo `resetConfig`: the next graph parses the configuration again.
    fn reset_config(&mut self, task: usize) {
        self.tasks[task].dirty = true;
    }

    /// An overflow (tsgo `DoCycle`): every configuration is parsed again.
    pub(crate) fn reset_all_configs(&mut self) {
        for position in 0..self.order.len() {
            let task = self.order[position];
            self.reset_config(task);
        }
    }

    /// tsgo `checkTasksForEventChanges`: which projects the changes concern
    /// (`(needs a new graph, needs a build)`).
    pub(crate) fn check_tasks_for_event_changes(
        &mut self,
        changed: &BTreeMap<String, crate::watch::WatchEventKind>,
        under_watch: &dyn Fn(&str) -> bool,
    ) -> (bool, bool) {
        let normalized: BTreeMap<String, crate::watch::WatchEventKind> = changed
            .iter()
            .map(|(path, kind)| (self.to_path(path), *kind))
            .collect();
        let mut needs_config_update = false;
        let mut needs_update = false;
        for position in 0..self.order.len() {
            let task = self.order[position];
            let config = self.tasks[task].config.clone();
            if normalized.contains_key(&self.to_path(&config)) {
                self.reset_config(task);
                needs_config_update = true;
                needs_update = true;
                continue;
            }
            let Some(plan) = self.tasks[task].plan.clone() else {
                continue;
            };
            if plan
                .extended_source_files()
                .iter()
                .any(|file| normalized.contains_key(&self.to_path(&file.to_string_lossy())))
            {
                self.reset_config(task);
                needs_config_update = true;
                needs_update = true;
                continue;
            }
            let roots: BTreeSet<String> = plan
                .file_names()
                .iter()
                .map(|file| self.to_path(&file.to_string_lossy()))
                .collect();
            let mut root_changed = false;
            if roots.iter().any(|root| normalized.contains_key(root)) {
                self.tasks[task].reset_status();
                needs_update = true;
                root_changed = true;
            }
            if !root_changed
                && (self.build_info_inputs_changed(task, &roots, &normalized)
                    || self.tasks[task].package_jsons.iter().any(|package_json| {
                        self.package_json_lookup_changed(package_json, &normalized)
                    }))
            {
                self.tasks[task].reset_status();
                needs_update = true;
            }
            // tsgo `ReloadFileNamesOfParsedCommandLine`: the include
            // patterns may match other files now.
            if !plan.wildcard_directories().is_empty() {
                if let Some(reloaded) = self.parse_config(&config) {
                    if reloaded.file_names() != plan.file_names() {
                        let mut plan = plan;
                        plan = plan.with_reloaded_file_names(reloaded);
                        self.tasks[task].plan = Some(plan);
                        self.tasks[task].reset_status();
                        needs_update = true;
                    }
                }
            }
        }
        if !needs_update {
            let directory_changed = changed
                .keys()
                .any(|path| self.fs.is_dir(path) && under_watch(path));
            if directory_changed {
                for position in 0..self.order.len() {
                    let task = self.order[position];
                    self.tasks[task].reset_status();
                }
                needs_update = true;
            }
        }
        (needs_config_update, needs_update)
    }

    /// Whether a file the build info lists (and the roots do not), or a
    /// package.json it looked up, changed.
    fn build_info_inputs_changed(
        &self,
        task: usize,
        roots: &BTreeSet<String>,
        changed: &BTreeMap<String, crate::watch::WatchEventKind>,
    ) -> bool {
        let Some(entry) = &self.tasks[task].build_info else {
            return false;
        };
        let Some(info) = &entry.info else {
            return false;
        };
        let directory = directory_of(&entry.path);
        for file_name in &info.file_names {
            let path = self.to_path(&self.resolve_build_info_file_name(file_name, &directory));
            if roots.contains(&path) {
                continue;
            }
            if changed.contains_key(&path) {
                return true;
            }
        }
        info.package_jsons
            .iter()
            .chain(&info.missing_package_jsons)
            .any(|package_json| {
                self.package_json_lookup_changed(&self.absolute(package_json, &directory), changed)
            })
    }

    /// tsgo `packageJsonLookupChanged`: the file changed, or a directory
    /// holding it was deleted.
    fn package_json_lookup_changed(
        &self,
        package_json: &str,
        changed: &BTreeMap<String, crate::watch::WatchEventKind>,
    ) -> bool {
        let path = self.to_path(package_json);
        if changed.contains_key(&path) {
            return true;
        }
        changed.iter().any(|(changed_path, kind)| {
            *kind == crate::watch::WatchEventKind::Delete
                && crate::watch::contains_path(changed_path, &path, self.case_sensitive)
        })
    }

    /// tsgo `resolveBuildInfoFileName`: a default library's bare name is in
    /// the library directory; any other name is relative to the build info.
    fn resolve_build_info_file_name(&self, file_name: &str, build_info_directory: &str) -> String {
        if tsc_incremental::is_default_library_name(file_name) {
            format!(
                "{}/{file_name}",
                self.system.default_library_path().trim_end_matches('/')
            )
        } else {
            self.absolute(file_name, build_info_directory)
        }
    }

    /// The real path of a file or directory, or the name itself.
    fn realpath(&self, file_name: &str) -> String {
        self.fs
            .canonicalize(file_name)
            .unwrap_or_else(|_| file_name.to_owned())
    }

    /// tsgo `computeDesiredWatches` before `ResolveDesiredDirs`.
    pub(crate) fn desired_watches(&self, set: &mut crate::watch::DirWatchSet) {
        let watchable = |set: &crate::watch::DirWatchSet, directory: &str| {
            !set.covered(directory) && crate::watch::can_watch_directory(directory)
        };
        for &task in &self.order {
            let task_state = &self.tasks[task];
            set.set(&self.realpath(&directory_of(&task_state.config)), false);
            let Some(plan) = &task_state.plan else {
                continue;
            };
            for file in plan.extended_source_files() {
                let real = self.realpath(&file.to_string_lossy());
                set.set(&directory_of(&real), false);
            }
            for directory in plan.wildcard_directories() {
                set.set(
                    &self.realpath(&directory.path.to_string_lossy()),
                    directory.recursive,
                );
            }
            for file in plan.file_names() {
                let absolute = self.absolute(&file.to_string_lossy(), &self.current_directory);
                let directory = directory_of(&absolute);
                if watchable(set, &directory) {
                    set.set(&directory, false);
                }
            }
            if let Some(entry) = &task_state.build_info {
                if let Some(info) = &entry.info {
                    let build_info_directory = directory_of(&entry.path);
                    let roots: BTreeSet<String> = plan
                        .file_names()
                        .iter()
                        .map(|file| self.to_path(&file.to_string_lossy()))
                        .collect();
                    for file_name in &info.file_names {
                        let absolute = self.realpath(
                            &self.resolve_build_info_file_name(file_name, &build_info_directory),
                        );
                        if roots.contains(&self.to_path(&absolute)) {
                            continue;
                        }
                        let directory = directory_of(&absolute);
                        if watchable(set, &directory) {
                            set.set(&directory, false);
                        }
                    }
                    for package_json in info.package_jsons.iter().chain(&info.missing_package_jsons)
                    {
                        let absolute = self.absolute(package_json, &build_info_directory);
                        add_package_json_watch_dirs(set, &absolute);
                    }
                }
            }
            for package_json in &task_state.package_jsons {
                add_package_json_watch_dirs(set, package_json);
            }
        }
    }
}

/// tsgo `addPackageJsonWatchDirs`: the package.json's directory, and in a
/// node_modules tree every directory up to the one holding node_modules.
fn add_package_json_watch_dirs(set: &mut crate::watch::DirWatchSet, package_json: &str) {
    let add = |set: &mut crate::watch::DirWatchSet, directory: &str| {
        if !set.covered(directory) && crate::watch::can_watch_directory(directory) {
            set.set(directory, false);
        }
    };
    let directory = directory_of(package_json);
    let mut directories = vec![directory.clone()];
    let mut found_node_modules = false;
    let mut current = directory.clone();
    loop {
        let parent = directory_of(&current);
        if parent.is_empty() || parent == current {
            break;
        }
        directories.push(parent.clone());
        if parent.rsplit('/').next() == Some("node_modules") {
            found_node_modules = true;
            let grandparent = directory_of(&parent);
            if !grandparent.is_empty() && grandparent != parent {
                directories.push(grandparent);
            }
            break;
        }
        current = parent;
    }
    if !found_node_modules {
        add(set, &directory);
        return;
    }
    for directory in directories {
        add(set, &directory);
    }
}

/// tsgo `ExitStatusInvalidProject_OutputsSkipped`: a project the graph
/// does not have.
pub(crate) const EXIT_INVALID_PROJECT: i32 = 3;

/// One build or clean of the projects in an order (tsgo
/// `OrchestratorResult`).
#[derive(Debug, Default)]
pub struct OrchestratorResult {
    /// The report (the command line prints it; the API discards it).
    pub(crate) stdout: String,
    /// tsgo `Result.Status`.
    pub status: i32,
    /// The errors reported (tsgo `Errors`).
    pub errors: Vec<Diagnostic>,
    pub statistics: BuildStatistics,
    /// The files a clean deleted, or would delete (tsgo `FilesToDelete`).
    pub files_to_delete: Vec<String>,
}

impl OrchestratorResult {
    fn status(status: i32) -> Self {
        Self {
            status,
            ..Self::default()
        }
    }
}

/// tsgo `Statistics`' project counts: the projects in the order, those
/// built, those whose outputs' times were updated.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BuildStatistics {
    pub projects: usize,
    pub projects_built: usize,
    pub timestamp_updates: usize,
}

/// What [`Orchestrator::clean_order`] did.
#[derive(Default)]
struct CleanPass {
    stdout: String,
    errors: Vec<Diagnostic>,
    files_to_delete: Vec<String>,
    /// A project's configuration was missing (the command line fails).
    missing_config: bool,
}

/// tsgo `oldestOutputFileName` of an up-to-date or pseudo-build status.
fn oldest_output_file_name(status: &UpToDateStatus) -> String {
    match &status.data {
        StatusData::Times { output, .. } => output.file.clone(),
        StatusData::InputOutput { output, .. } => output.clone(),
        StatusData::Text(text) => text.clone(),
        StatusData::None | StatusData::Upstream { .. } => String::new(),
    }
}

/// The `buildOptions` of the API's `createBuildOrchestrator` (tsgo
/// `core.BuildOptions`, which replace the parsed ones). The port builds the
/// projects one after the other, so `builders` has nothing to set.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ApiBuildOptions {
    pub dry: bool,
    pub force: bool,
    pub verbose: bool,
    pub stop_build_on_errors: bool,
    pub clean: bool,
}

/// tsgo's API build orchestrator (`build.NewOrchestrator` over the API's
/// build system): it builds and cleans the projects of its root names and
/// keeps its tasks from one request to the next.
pub struct ApiOrchestrator {
    system: Arc<dyn System>,
    budgets: crate::cli::CommandBudgets,
    /// Out only while a call runs.
    state: Option<OrchestratorState>,
}

impl ApiOrchestrator {
    /// tsgo `handleCreateBuildOrchestrator`: the root names parsed as a
    /// build command line over `system`, with the given build and compiler
    /// options in place of the parsed ones.
    pub fn new(
        system: Arc<dyn System>,
        catalog: LibraryCatalog,
        root_names: &[String],
        build_options: Option<ApiBuildOptions>,
        compiler_options: Option<ConfigOptionBag>,
    ) -> Self {
        let current_directory = system.current_directory().to_owned();
        let host = system.compiler_host();
        let read_response_file = |path: JsStr<'_>| {
            host.read_file_js(path)
                .ok()
                .flatten()
                .and_then(|bytes| decode_host_text(bytes).ok())
        };
        let parsed = parse_build_command_line(
            root_names,
            JsStr::from_str(&current_directory),
            host.use_case_sensitive_file_names(),
            &read_response_file,
        );
        let mut command = BuildCommand::from_parsed(&parsed, JsStr::from_str(&current_directory));
        if let Some(options) = build_options {
            command.dry = options.dry;
            command.force = options.force;
            command.verbose = options.verbose;
            command.stop_build_on_errors = options.stop_build_on_errors;
            command.clean = options.clean;
        }
        if let Some(options) = compiler_options {
            command.command_line = options;
        }
        let budgets = crate::cli::CommandBudgets::of(
            parsed.option_bool("singleThreaded"),
            parsed
                .option_value("checkers")
                .and_then(|value| value.as_f64()),
        );
        drop(host);
        let state = Orchestrator::new(
            &*system,
            None,
            catalog,
            command,
            PathBuf::from(&current_directory),
            false,
        )
        .into_state();
        Self {
            system,
            budgets,
            state: Some(state),
        }
    }

    /// tsgo `Orchestrator.Build`: `project` (every project when empty)
    /// and its upstream projects, checked again and built.
    pub fn build(&mut self, project: &str) -> Result<OrchestratorResult, String> {
        self.call(|orchestrator, route| {
            orchestrator.recheck_all_projects(project);
            orchestrator.start(project, false, route)
        })
    }

    /// tsgo `Orchestrator.BuildReferences`: as [`build`](Self::build),
    /// without the project itself.
    pub fn build_references(&mut self, project: &str) -> Result<OrchestratorResult, String> {
        self.call(|orchestrator, route| {
            orchestrator.recheck_all_projects(project);
            orchestrator.start(project, true, route)
        })
    }

    /// tsgo `Orchestrator.Clean`.
    pub fn clean(&mut self, project: &str) -> Result<OrchestratorResult, String> {
        self.call(|orchestrator, _| orchestrator.clean(project, false))
    }

    /// tsgo `Orchestrator.CleanReferences`.
    pub fn clean_references(&mut self, project: &str) -> Result<OrchestratorResult, String> {
        self.call(|orchestrator, _| orchestrator.clean(project, true))
    }

    /// One call over the system, the state put back after it.
    fn call(
        &mut self,
        run: impl FnOnce(
            &mut Orchestrator<'_>,
            &mut CliRoute<'_>,
        ) -> Result<OrchestratorResult, CliError>,
    ) -> Result<OrchestratorResult, String> {
        let state = self
            .state
            .take()
            .ok_or_else(|| "build orchestrator lost its state in an earlier call".to_owned())?;
        let mut orchestrator = Orchestrator::with_state(&*self.system, None, state);
        let result = crate::cli::with_build_route(
            &*self.system,
            None,
            false,
            crate::locale::Locale::English,
            self.budgets,
            false,
            |route| run(&mut orchestrator, route),
        );
        self.state = Some(orchestrator.into_state());
        result.map_err(|error| error.to_string())
    }
}

/// `tsc -b` over the command line.
pub(crate) fn run_build(
    current_directory: PathBuf,
    catalog: LibraryCatalog,
    command: BuildCommand,
    route: &mut CliRoute<'_>,
) -> Result<CliOutput, CliError> {
    let mut orchestrator = Orchestrator::new(
        route.system,
        route.testing,
        catalog,
        command,
        current_directory,
        route.pretty,
    );
    orchestrator.locale = route.locale;
    orchestrator.run(route)
}

/// tsgo `ResolvedProjectReferencePaths`: the config file of every
/// reference of the plan.
fn resolved_reference_paths(plan: &ConfigRootPlan) -> Vec<String> {
    plan.project_references()
        .unwrap_or_default()
        .iter()
        .map(|reference| {
            resolve_config_file_name_of_project_reference(reference.path.as_js())
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn is_declaration_output(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.ends_with(".d.ts") || lower.ends_with(".d.mts") || lower.ends_with(".d.cts")
}

fn directory_of(path: &str) -> String {
    match path.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(index) => path[..index].to_owned(),
        None => String::new(),
    }
}

/// `a.After(b)` over tsgo times, where `None` is the zero time.
fn after(a: Option<SystemTime>, b: Option<SystemTime>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        (None, _) => false,
    }
}

/// `a.Before(b)` over tsgo times, where `None` is the zero time.
fn before(a: Option<SystemTime>, b: Option<SystemTime>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a < b,
        (None, Some(_)) => true,
        (_, None) => false,
    }
}

/// tsgo's status time: the system's local time as Go's `03:04:05 PM`.
pub(crate) fn status_time(system: &dyn System) -> String {
    let now = system
        .now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    format_status_time(now + local_time_offset(system, now))
}

/// The seconds of the day as `HH:MM:SS AM`.
fn format_status_time(local_seconds: i64) -> String {
    let seconds = local_seconds.rem_euclid(86_400);
    let (hour, minute, second) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    let (hour, meridiem) = match hour {
        0 => (12, "AM"),
        hour @ 1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        hour => (hour - 12, "PM"),
    };
    format!("{hour:02}:{minute:02}:{second:02} {meridiem}")
}

/// The local time zone's offset from UTC at `now`, from the zone database
/// (`TZ`, else `/etc/localtime`); UTC when it cannot be read.
fn local_time_offset(system: &dyn System, now: i64) -> i64 {
    let zone = system.env_var("TZ").map(|zone| {
        let zone = zone.strip_prefix(':').unwrap_or(&zone).to_owned();
        if zone.starts_with('/') {
            zone
        } else {
            format!("/usr/share/zoneinfo/{zone}")
        }
    });
    let path = zone.unwrap_or_else(|| "/etc/localtime".to_owned());
    fs::read(path)
        .ok()
        .and_then(|bytes| tzif_offset(&bytes, now))
        .unwrap_or(0)
}

/// The UTC offset at `now` recorded by a TZif file (RFC 8536): the type of
/// the last transition at or before `now`, else the first standard type.
/// Later than the last transition the last type applies (the footer's rule
/// is not evaluated; the database lists transitions decades ahead).
fn tzif_offset(bytes: &[u8], now: i64) -> Option<i64> {
    fn be32(bytes: &[u8], at: usize) -> Option<i64> {
        Some(i64::from(i32::from_be_bytes(
            bytes.get(at..at + 4)?.try_into().ok()?,
        )))
    }
    fn be64(bytes: &[u8], at: usize) -> Option<i64> {
        Some(i64::from_be_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
    }
    struct Header {
        is_ut: usize,
        is_std: usize,
        leap: usize,
        time: usize,
        types: usize,
        chars: usize,
    }
    fn header(bytes: &[u8], at: usize) -> Option<(Header, u8)> {
        if bytes.get(at..at + 4)? != b"TZif" {
            return None;
        }
        let version = *bytes.get(at + 4)?;
        let count = |index: usize| be32(bytes, at + 20 + index * 4).map(|count| count as usize);
        Some((
            Header {
                is_ut: count(0)?,
                is_std: count(1)?,
                leap: count(2)?,
                time: count(3)?,
                types: count(4)?,
                chars: count(5)?,
            },
            version,
        ))
    }
    let (first, version) = header(bytes, 0)?;
    let (start, header, time_size) = if version >= b'2' {
        let v1_size = first.time * 5
            + first.types * 6
            + first.chars
            + first.leap * 8
            + first.is_std
            + first.is_ut;
        let second_header = 44 + v1_size;
        let (second, _) = header(bytes, second_header)?;
        (second_header + 44, second, 8usize)
    } else {
        (44, first, 4usize)
    };
    let times = start;
    let indices = times + header.time * time_size;
    let types = indices + header.time;
    if header.types == 0 {
        return None;
    }
    let type_offset = |index: usize| -> Option<(i64, bool)> {
        if index >= header.types {
            return None;
        }
        let at = types + index * 6;
        Some((be32(bytes, at)?, *bytes.get(at + 4)? != 0))
    };
    let mut selected = None;
    for transition in 0..header.time {
        let at = times + transition * time_size;
        let when = if time_size == 8 {
            be64(bytes, at)?
        } else {
            be32(bytes, at)?
        };
        if when > now {
            break;
        }
        selected = Some(usize::from(*bytes.get(indices + transition)?));
    }
    let index = match selected {
        Some(index) => index,
        None => (0..header.types)
            .find(|index| type_offset(*index).is_some_and(|(_, dst)| !dst))
            .unwrap_or(0),
    };
    type_offset(index).map(|(offset, _)| offset)
}

/// tsgo `CreateReportErrorSummary`'s text for the whole build (pretty only).
fn render_error_summary(
    current_directory: &Path,
    sources: &DiagnosticSourceMap,
    errors: &[Diagnostic],
    locale: crate::locale::Locale,
) -> Result<String, CliError> {
    crate::cli::render_error_summary_text(current_directory, sources, errors, locale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_comparisons_treat_none_as_the_zero_time() {
        let t1 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        let t2 = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(2);
        assert!(after(Some(t2), Some(t1)));
        assert!(!after(Some(t1), Some(t2)));
        assert!(after(Some(t1), None));
        assert!(!after(None, Some(t1)));
        assert!(!after(None, None));
        assert!(before(Some(t1), Some(t2)));
        assert!(before(None, Some(t1)));
        assert!(!before(Some(t1), None));
        assert!(!before(None, None));
    }

    #[test]
    fn status_time_has_gos_shape() {
        let system = crate::system::NativeSystem::from_process().expect("the process system");
        let time = status_time(&system);
        assert_eq!(time.len(), 11, "{time}");
        assert!(time.ends_with(" AM") || time.ends_with(" PM"), "{time}");
        assert_eq!(&time[2..3], ":");
        assert_eq!(&time[5..6], ":");
        assert_eq!(format_status_time(0), "12:00:00 AM");
        assert_eq!(format_status_time(12 * 3600 + 5), "12:00:05 PM");
        assert_eq!(format_status_time(13 * 3600 + 4 * 60 + 5), "01:04:05 PM");
        assert_eq!(format_status_time(-1), "11:59:59 PM");
    }

    /// A TZif v2 file with one type (UTC+9, no transitions) and one with a
    /// transition to UTC+10 at second 1000.
    fn tzif(transitions: &[(i64, u8)], types: &[(i32, bool)]) -> Vec<u8> {
        fn block(transitions: &[(i64, u8)], types: &[(i32, bool)], wide: bool) -> Vec<u8> {
            let mut out = b"TZif2".to_vec();
            out.extend([0u8; 15]);
            for count in [0u32, 0, 0, transitions.len() as u32, types.len() as u32, 0] {
                out.extend(count.to_be_bytes());
            }
            for (when, _) in transitions {
                if wide {
                    out.extend(when.to_be_bytes());
                } else {
                    out.extend((*when as i32).to_be_bytes());
                }
            }
            out.extend(transitions.iter().map(|(_, index)| *index));
            for (offset, dst) in types {
                out.extend(offset.to_be_bytes());
                out.push(u8::from(*dst));
                out.push(0);
            }
            out
        }
        let mut out = block(transitions, types, false);
        out.extend(block(transitions, types, true));
        out.extend(b"\nJST-9\n");
        out
    }

    #[test]
    fn tzif_offset_follows_the_transitions() {
        let fixed = tzif(&[], &[(9 * 3600, false)]);
        assert_eq!(tzif_offset(&fixed, 0), Some(9 * 3600));
        assert_eq!(tzif_offset(&fixed, 1_800_000_000), Some(9 * 3600));
        let shifting = tzif(&[(1000, 1)], &[(9 * 3600, false), (10 * 3600, true)]);
        assert_eq!(tzif_offset(&shifting, 999), Some(9 * 3600));
        assert_eq!(tzif_offset(&shifting, 1000), Some(10 * 3600));
        assert_eq!(tzif_offset(b"not a zone", 0), None);
    }

    #[test]
    fn directory_of_handles_the_root() {
        assert_eq!(directory_of("/a/b.json"), "/a");
        assert_eq!(directory_of("/b.json"), "/");
        assert_eq!(directory_of("b.json"), "");
    }
}
