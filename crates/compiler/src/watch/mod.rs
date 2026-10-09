//! Watch mode (tsgo `execute/watcher.go`): `tsc --watch` builds the program,
//! watches the directories it depends on, and builds again when a change
//! arrives, reusing the previous build's incremental state. `tsc -b --watch`
//! is the build orchestrator's watch ([`BuildWatcher`], tsgo
//! `build/orchestrator.go`).
//!
//! A cycle ([`Watcher::do_cycle`]) takes the changes the watches reported,
//! checks whether the configuration files changed, decides whether the
//! changes concern the program (a file it read or probed, a file its
//! `include` could match, a directory it watches), and if so builds again
//! and reconciles the watched directories. Each build is the command's
//! compilation over the previous build's state, which a watch run keeps in
//! memory whether or not the program is incremental.
//!
//! The directories are watched through a [`WatchBackend`]: the process
//! watches the operating system's file system (the `notify` crate), a test
//! harness supplies its own (tsgo `MockWatchBackend`), and so can a
//! WebAssembly embedding.

mod manager;
#[cfg(not(target_family = "wasm"))]
mod native;
mod tracking;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use tsc_diagnostics::{gen, Diagnostic, DiagnosticMessage, JsStr, MessageChain};
use tsc_program::LibraryCatalog;

use crate::cli::{self, CliError, CommandBudgets, WatchTarget};
use crate::incremental::WatchState;
use crate::locale::Locale;
use crate::system::{CommandLineTesting, ProgramReport, System};

pub(crate) use manager::Backend;
pub(crate) use manager::{can_watch_directory, contains_path, DirWatchSet};
use manager::{directory_path, resolve_desired_dirs, WatchManager};
pub use manager::{
    WatchBackend, WatchEvent, WatchEventKind, WatchEvents, WatchHandle, WatchRequest,
};
pub(crate) use tracking::TrackingHost;

/// tsgo's debounce of a watch's events: a cycle starts once no change
/// arrived for this long...
const QUIET: Duration = Duration::from_millis(50);
/// ...or this long after the first change.
const LONGEST: Duration = Duration::from_millis(500);

/// tsgo `CreateWatchStatusReporter`: the screen is cleared before a
/// compilation starts (unless `preserveWatchOutput` or the statistics
/// options keep it), then the time and the message.
pub(crate) struct WatchStatus<'a> {
    pub(crate) system: &'a dyn System,
    pub(crate) testing: Option<&'a dyn CommandLineTesting>,
    pub(crate) pretty: bool,
    pub(crate) locale: Locale,
    /// `preserveWatchOutput`, `diagnostics` or `extendedDiagnostics`.
    pub(crate) keeps_screen: bool,
}

impl WatchStatus<'_> {
    pub(crate) fn report(&self, message: &'static DiagnosticMessage, args: &[String]) {
        let mut output = String::new();
        if let Some(testing) = self.testing {
            testing.on_watch_status_report_start(&mut output);
        }
        let clears = (message.code == gen::Starting_compilation_in_watch_mode.code
            || message.code == gen::File_change_detected_Starting_incremental_compilation.code)
            && !self.keeps_screen;
        if clears {
            output.push_str("\x1b[2J\x1b[3J\x1b[H");
        }
        let text = MessageChain::new(message, args)
            .text_in(self.locale.messages())
            .to_string_lossy()
            .into_owned();
        let time = crate::build::status_time(self.system);
        if self.pretty {
            output.push_str(&format!("[\x1b[90m{time}\x1b[0m] {text}\n\n"));
        } else {
            output.push_str(&format!("{time} - {text}\n\n"));
        }
        if let Some(testing) = self.testing {
            testing.on_watch_status_report_end(&mut output);
        }
        self.system.write_output(&output);
    }

    /// `Found N errors. Watching for file changes.`
    pub(crate) fn report_found_errors(&self, count: usize) {
        if count == 1 {
            self.report(&gen::Found_1_error_Watching_for_file_changes, &[]);
        } else {
            self.report(
                &gen::Found_0_errors_Watching_for_file_changes,
                &[count.to_string()],
            );
        }
    }
}

/// The watch a command started (tsgo `tsc.Watcher`): a compilation's or a
/// build's.
pub enum CommandWatcher<'a> {
    Program(Box<Watcher<'a>>),
    Build(Box<BuildWatcher<'a>>),
}

impl CommandWatcher<'_> {
    /// One cycle over the changes reported since the last.
    pub fn do_cycle(&mut self) {
        match self {
            Self::Program(watcher) => watcher.do_cycle(),
            Self::Build(watcher) => watcher.do_cycle(),
        }
    }

    /// Runs the watch until the process ends.
    pub fn run(&mut self, flush: &mut dyn FnMut()) -> ! {
        match self {
            Self::Program(watcher) => watcher.run(flush),
            Self::Build(watcher) => watcher.run(flush),
        }
    }
}

/// tsgo's choice of a watch's backend: the harness's when it has one; the
/// operating system's for the process.
fn watch_backend<'a>(testing: Option<&'a dyn CommandLineTesting>) -> Option<Backend<'a>> {
    match testing.and_then(CommandLineTesting::watch_backend) {
        Some(backend) => Some(Backend::Borrowed(backend)),
        None if testing.is_none() => native_backend(),
        None => None,
    }
}

/// `tsc -b --watch`: tsgo's build `Orchestrator` as the command's watcher.
/// Each cycle builds the projects the changes concern (their configs,
/// roots, the other files their build info lists, the package.json files
/// they looked up, or a watched directory), over the tasks' state kept
/// from the last cycle.
pub struct BuildWatcher<'a> {
    orchestrator: crate::build::Orchestrator<'a>,
    manager: WatchManager<'a>,
    system: &'a dyn System,
    testing: Option<&'a dyn CommandLineTesting>,
    pretty: bool,
    locale: Locale,
    budgets: CommandBudgets,
}

impl<'a> BuildWatcher<'a> {
    /// tsgo `Orchestrator.start` with `--watch`: the first build, then the
    /// watches. The first build's exit status comes back with the watcher.
    pub(crate) fn start(
        orchestrator: crate::build::Orchestrator<'a>,
        system: &'a dyn System,
        testing: Option<&'a dyn CommandLineTesting>,
        pretty: bool,
        locale: Locale,
        budgets: CommandBudgets,
    ) -> (Self, i32) {
        let mut watcher = Self {
            orchestrator,
            manager: WatchManager::new(watch_backend(testing)),
            system,
            testing,
            pretty,
            locale,
            budgets,
        };
        watcher
            .status()
            .report(&gen::Starting_compilation_in_watch_mode, &[]);
        watcher.orchestrator.generate_initial_graph();
        let status = watcher.build();
        watcher.watch();
        (watcher, status)
    }

    fn status(&self) -> WatchStatus<'a> {
        WatchStatus {
            system: self.system,
            testing: self.testing,
            pretty: self.pretty,
            locale: self.locale,
            keeps_screen: self.orchestrator.keeps_screen(),
        }
    }

    /// tsgo `buildOrClean` and its report: the projects' output, then
    /// `Found N errors. Watching for file changes.` and the statistics.
    fn build(&mut self) -> i32 {
        let orchestrator = &mut self.orchestrator;
        let result = cli::with_build_route(
            self.system,
            self.testing,
            self.pretty,
            self.locale,
            self.budgets,
            |route| orchestrator.build_order(route),
        );
        match result {
            Ok(cycle) => {
                self.system.write_output(&cycle.stdout);
                self.status().report_found_errors(cycle.errors.len());
                self.system
                    .write_output(&self.orchestrator.statistics_report());
                cycle.exit_status
            }
            Err(error) => {
                self.system.write_error(&format!("tsc-rs: {error}\n"));
                1
            }
        }
    }

    /// tsgo `Watch` (and the end of `DoCycle`): the time cache moves on,
    /// the watches follow the projects, and the cycle's caches go.
    fn watch(&mut self) {
        self.orchestrator.update_watch();
        let mut set = DirWatchSet::new(self.orchestrator.case_sensitive());
        self.orchestrator.desired_watches(&mut set);
        let fs = self.system.fs();
        let desired =
            resolve_desired_dirs(&set.into_directories(), &|directory| fs.is_dir(directory));
        if let Err(error) = self.manager.reconcile(&desired) {
            self.system.write_output(&format!("{error}\n"));
            self.manager.force_overflow();
        }
        self.orchestrator.reset_caches();
    }

    /// tsgo `Orchestrator.DoCycle`.
    pub fn do_cycle(&mut self) {
        let events = self.manager.drain_events();
        if events.is_empty() {
            return;
        }
        let (needs_config_update, needs_update) = if events.overflow {
            self.orchestrator.reset_all_configs();
            (true, true)
        } else {
            let case_sensitive = self.orchestrator.case_sensitive();
            let manager = &self.manager;
            self.orchestrator
                .check_tasks_for_event_changes(&events.changed, &|path| {
                    manager.is_path_under_watch(path, case_sensitive)
                })
        };
        if !needs_update {
            self.orchestrator.reset_caches();
            return;
        }
        self.status().report(
            &gen::File_change_detected_Starting_incremental_compilation,
            &[],
        );
        if needs_config_update {
            self.orchestrator.regenerate_graph();
        }
        self.build();
        self.watch();
    }

    /// Runs the watch until the process ends.
    pub fn run(&mut self, flush: &mut dyn FnMut()) -> ! {
        flush();
        loop {
            self.manager.wait_for_events(QUIET, LONGEST);
            self.do_cycle();
            flush();
        }
    }
}

/// A `tsc --watch` run (tsgo `execute.Watcher`).
pub struct Watcher<'a> {
    system: &'a dyn System,
    testing: Option<&'a dyn CommandLineTesting>,
    pretty: bool,
    locale: Locale,
    budgets: CommandBudgets,
    current_directory: PathBuf,
    catalog: LibraryCatalog,
    target: WatchTarget,
    manager: WatchManager<'a>,
    /// The previous build's incremental state.
    state: Option<WatchState>,
    /// The previous build's program, as the harness last saw it.
    report: Option<ProgramReport>,
    /// Every path the previous build depended on, by its canonical name.
    seen: BTreeSet<String>,
    /// The configuration files and their modification times.
    config_mtimes: BTreeMap<String, Option<SystemTime>>,
    config_modified: bool,
    config_has_errors: bool,
}

impl<'a> Watcher<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        system: &'a dyn System,
        testing: Option<&'a dyn CommandLineTesting>,
        pretty: bool,
        locale: Locale,
        budgets: CommandBudgets,
        current_directory: PathBuf,
        catalog: LibraryCatalog,
        target: WatchTarget,
    ) -> Self {
        // tsgo `createWatcher`: the harness's backend when it has one; the
        // run starts the operating system's at its start otherwise.
        let backend = watch_backend(testing);
        let config_mtimes = target
            .config_files()
            .into_iter()
            .map(|file| (file, None))
            .collect();
        Self {
            system,
            testing,
            pretty,
            locale,
            budgets,
            current_directory,
            catalog,
            target,
            manager: WatchManager::new(backend),
            state: None,
            report: None,
            seen: BTreeSet::new(),
            config_mtimes,
            config_modified: false,
            config_has_errors: false,
        }
    }

    /// tsgo `Watcher.start`: the first build.
    pub(crate) fn start(&mut self) {
        self.report_status(&gen::Starting_compilation_in_watch_mode, &[]);
        if self.build().is_err() {
            self.manager.force_overflow();
        }
    }

    /// Runs the watch until the process ends: waits for changes, lets them
    /// settle, and runs a cycle, passing each cycle's output to `flush`.
    pub fn run(&mut self, flush: &mut dyn FnMut()) -> ! {
        flush();
        loop {
            self.manager.wait_for_events(QUIET, LONGEST);
            self.do_cycle();
            flush();
        }
    }

    /// tsgo `Watcher.DoCycle`: one cycle over the changes reported since the
    /// last.
    pub fn do_cycle(&mut self) {
        let events = self.manager.drain_events();
        let has_events = !events.is_empty();
        if self.recheck_config(false) {
            return;
        }
        // tsgo decides here, too, between its single-file rebuild and a full
        // one; a build here is always the full one (the include patterns
        // matched again, every dependency recorded).
        let quiet = if has_events && !events.overflow {
            !self.config_modified && !self.is_relevant_change(&events.changed)
        } else {
            !has_events && !self.config_modified
        };
        if quiet {
            self.replay_program();
            return;
        }
        self.report_status(
            &gen::File_change_detected_Starting_incremental_compilation,
            &[],
        );
        if self.build().is_err() {
            self.manager.force_overflow();
        }
    }

    /// A cycle without a build reports the previous program again (tsgo
    /// calls `OnProgram` with it).
    fn replay_program(&self) {
        if let (Some(testing), Some(report)) = (self.testing, &self.report) {
            testing.on_program(report);
        }
    }

    /// tsgo `doBuild`: builds over the previous state with a host that
    /// records what the program depends on, then watches it.
    fn build(&mut self) -> Result<(), ()> {
        if self.target.has_wildcard_directories() {
            // tsgo `ReloadFileNamesOfParsedCommandLine`: a full build
            // expands the include patterns again (a build here is always
            // tsgo's full build).
            if let Err(error) = self.target.reload_file_names(self.system) {
                self.system.write_error(&format!("tsc-rs: {error}\n"));
                return Err(());
            }
        }
        let host = self.system.compiler_host();
        let tracking = TrackingHost::new(&*host);
        for directory in self.target.wildcard_directories().keys() {
            tracking.add(directory);
        }
        for file in self.target.config_files() {
            tracking.add(&file);
        }
        let result = cli::watch_build(
            self.system,
            self.testing,
            self.pretty,
            self.locale,
            self.budgets,
            &tracking,
            &self.current_directory,
            &self.catalog,
            &self.target,
            self.state.as_ref(),
        );
        let run = match result {
            Ok(run) => run,
            Err(error) => {
                self.system.write_error(&format!("tsc-rs: {error}\n"));
                return Err(());
            }
        };
        self.system.write_output(&run.stdout);
        if let Some(state) = run.watch_state {
            self.state = Some(state);
        }
        self.report = run.program_report;
        let seen = tracking.seen();
        self.seen = seen.iter().map(|path| self.canonical(path)).collect();
        self.refresh_config_mtimes();
        let reconciled = self.reconcile_watches(&seen);
        self.config_modified = false;
        self.report_found_errors(run.error_count);
        reconciled
    }

    /// tsgo's "Found N errors. Watching for file changes." status.
    fn report_found_errors(&self, count: usize) {
        self.status().report_found_errors(count);
    }

    fn report_status(&self, message: &'static DiagnosticMessage, args: &[String]) {
        self.status().report(message, args);
    }

    fn status(&self) -> WatchStatus<'a> {
        let options = self.target.options();
        WatchStatus {
            system: self.system,
            testing: self.testing,
            pretty: self.pretty,
            locale: self.locale,
            keeps_screen: options.preserve_watch_output == Some(true)
                || options.extended_diagnostics == Some(true)
                || options.diagnostics == Some(true),
        }
    }

    /// tsgo `recheckTsConfig`: when a configuration file changed (or the
    /// last parse failed), parse it again; a configuration that cannot be
    /// read stops the cycle with its error.
    fn recheck_config(&mut self, force: bool) -> bool {
        if !self.target.is_config() {
            return false;
        }
        if !force && !self.config_has_errors && !self.config_mtimes.is_empty() {
            let changed = self.config_mtimes.iter().any(|(file, old)| {
                let now = self.modified(file);
                match old {
                    None => now.is_some(),
                    Some(old) => now != Some(*old),
                }
            });
            if !changed {
                return false;
            }
        }
        match self.target.reparse(self.system) {
            Ok(changed) => {
                if self.config_has_errors {
                    self.config_modified = true;
                }
                self.config_has_errors = false;
                if changed {
                    self.config_modified = true;
                }
                self.config_mtimes = self
                    .target
                    .config_files()
                    .into_iter()
                    .map(|file| (file, None))
                    .collect();
                false
            }
            Err(ConfigError::Diagnostics(diagnostics)) => {
                let text = cli::render_watch_diagnostics(
                    self.system,
                    self.pretty,
                    self.locale,
                    &self.current_directory,
                    &diagnostics,
                )
                .unwrap_or_default();
                self.system.write_output(&text);
                self.config_has_errors = true;
                self.report_found_errors(diagnostics.len());
                true
            }
            Err(ConfigError::Fatal(error)) => {
                self.system.write_error(&format!("tsc-rs: {error}\n"));
                self.config_has_errors = true;
                true
            }
        }
    }

    fn refresh_config_mtimes(&mut self) {
        let files = self.target.config_files();
        self.config_mtimes = files
            .into_iter()
            .map(|file| {
                let modified = self.modified(&file);
                (file, modified)
            })
            .collect();
    }

    fn modified(&self, path: &str) -> Option<SystemTime> {
        self.system
            .fs()
            .metadata(path)
            .ok()
            .map(|metadata| metadata.modified())
    }

    fn directory_exists(&self, path: &str) -> bool {
        self.system.fs().is_dir(path)
    }

    fn case_sensitive(&self) -> bool {
        self.system.fs().case_sensitive()
    }

    /// tsgo `tspath.ToPath`.
    fn canonical(&self, path: &str) -> String {
        let absolute = cli::normalized_absolute(&self.current_directory, path);
        if self.case_sensitive() {
            absolute
        } else {
            tsc_host::to_file_name_lower_case(&absolute)
        }
    }

    /// tsgo `isRelevantChange`.
    fn is_relevant_change(&self, changed: &BTreeMap<String, WatchEventKind>) -> bool {
        changed.keys().any(|path| {
            let key = self.canonical(path);
            self.seen.contains(&key)
                || self.possibly_matches_file_name(path)
                || self.possibly_matches_directory_name(path)
                || (self.directory_exists(path)
                    && self
                        .manager
                        .is_path_under_watch(path, self.case_sensitive()))
        })
    }

    /// tsgo `PossiblyMatchesFileName` of the configuration.
    fn possibly_matches_file_name(&self, path: &str) -> bool {
        self.target.plan().is_some_and(|plan| {
            plan.possibly_matches_file_name(
                JsStr::from_str(&cli::normalized_absolute(&self.current_directory, path)),
                self.case_sensitive(),
            )
        })
    }

    /// tsgo `PossiblyMatchesDirectoryName` of the configuration.
    fn possibly_matches_directory_name(&self, path: &str) -> bool {
        self.target.plan().is_some_and(|plan| {
            plan.possibly_matches_directory_name(
                JsStr::from_str(&cli::normalized_absolute(&self.current_directory, path)),
                self.case_sensitive(),
            )
        })
    }

    /// tsgo `computeDesiredWatches` and `ReconcileWatches`.
    fn reconcile_watches(&mut self, seen: &[String]) -> Result<(), ()> {
        let fs = self.system.fs();
        let exists = |path: &str| fs.is_dir(path);
        let realpath = |path: &str| fs.canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let mut desired = BTreeMap::new();
        for (directory, recursive) in self.target.wildcard_directories() {
            desired.insert(realpath(&directory), recursive);
        }
        if !self.target.is_config() && desired.is_empty() {
            desired.insert(realpath(&self.current_directory.to_string_lossy()), false);
        }
        for file in self.target.config_files() {
            desired
                .entry(directory_path(&realpath(&file)))
                .or_insert(false);
        }
        if !self.target.is_config() {
            for file in self.target.file_names() {
                let absolute = cli::normalized_absolute(&self.current_directory, &file);
                desired
                    .entry(directory_path(&realpath(&absolute)))
                    .or_insert(false);
            }
        }
        let resolved = resolve_desired_dirs(&desired, &exists);
        let mut coverage = DirWatchSet::new(self.case_sensitive());
        for (directory, recursive) in &resolved {
            coverage.set(directory, *recursive);
        }
        for path in seen {
            let directory = directory_path(path);
            if !coverage.covered(&directory) && can_watch_directory(&directory) {
                coverage.set(&directory, false);
            }
        }
        let desired = resolve_desired_dirs(&coverage.into_directories(), &exists);
        if let Err(error) = self.manager.reconcile(&desired) {
            self.system.write_output(&format!("{error}\n"));
            return Err(());
        }
        Ok(())
    }
}

/// Why a configuration could not be parsed again.
pub(crate) enum ConfigError {
    /// tsgo's unrecoverable configuration errors (TS5083: the file cannot
    /// be read).
    Diagnostics(Vec<Diagnostic>),
    Fatal(CliError),
}

#[cfg(not(target_family = "wasm"))]
fn native_backend<'a>() -> Option<Backend<'a>> {
    Some(Backend::Owned(Box::new(native::NativeWatchBackend::new())))
}

#[cfg(target_family = "wasm")]
fn native_backend<'a>() -> Option<Backend<'a>> {
    None
}
