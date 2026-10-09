//! A project and its program (tsgo `project.Project`, project.go).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};

use tsc_compiler::LiveProgram;
use tsc_diagnostics::{Diagnostic, JsStr};
use tsc_program::{
    CanonicalPath, CompilerOptions, ConfigRootPlan, PreparedProgram, ProgramOptions,
};

use crate::fs::SeenFiles;
use crate::id::{ProjectId, ProjectKind};

/// How a project's program was last built (tsgo `ProgramUpdateKind`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ProgramUpdateKind {
    /// Not built by the build that made this snapshot.
    #[default]
    None,
    /// The previous program reused after a one-file change.
    Cloned,
    /// Built again with the same files.
    SameFileNames,
    /// Built with other files, or for the first time.
    NewFiles,
}

/// What a project's program is built from (tsgo `ParsedCommandLine`).
#[derive(Clone, Debug)]
pub enum CommandLine {
    /// A configured project's config, as parsed.
    Config(Arc<ConfigRootPlan>),
    /// A synthetic program's roots and options.
    Roots(Arc<ProgramRoots>),
}

impl CommandLine {
    /// tsgo `FileNames`: the root files.
    pub fn file_names(&self) -> Vec<String> {
        match self {
            Self::Config(plan) => plan
                .file_names()
                .iter()
                .map(|file| file.to_string_lossy().into_owned())
                .collect(),
            Self::Roots(roots) => roots.root_file_names.clone(),
        }
    }

    pub fn compiler_options(&self) -> &CompilerOptions {
        match self {
            Self::Config(plan) => plan.compiler_options(),
            Self::Roots(roots) => &roots.compiler_options,
        }
    }

    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Config(left), Self::Config(right)) => Arc::ptr_eq(left, right),
            (Self::Roots(left), Self::Roots(right)) => Arc::ptr_eq(left, right),
            _ => false,
        }
    }
}

/// The root files and options of a program without a config (tsgo
/// `NewParsedCommandLine`): a synthetic program's.
#[derive(Clone, Debug, PartialEq)]
pub struct ProgramRoots {
    /// Absolute file names, in the client's order.
    pub root_file_names: Vec<String>,
    pub compiler_options: CompilerOptions,
    pub program_options: ProgramOptions,
    /// tsgo `CommandLine.Errors`: rows the client reports as the program's
    /// config file parsing diagnostics.
    pub config_file_parsing_diagnostics: Vec<Diagnostic>,
}

/// The inferred project's roots and options (tsgo `NewInferredProject`'s
/// defaults: `allowJs`, `module: esnext`, `moduleResolution: bundler`, the
/// latest standard target, `jsx: react-jsx`, `allowImportingTsExtensions`,
/// `strictNullChecks`, `strictFunctionTypes`, `sourceMap`,
/// `allowNonTsExtensions` and `resolveJsonModule`).
pub(crate) fn inferred_project_roots(root_file_names: Vec<String>) -> ProgramRoots {
    ProgramRoots {
        root_file_names,
        compiler_options: CompilerOptions {
            allow_js: true,
            allow_js_specified: Some(true),
            module: Some(99),
            module_resolution: Some(100),
            target: Some(13),
            jsx: Some(4),
            allow_importing_ts_extensions: Some(true),
            strict_null_checks: Some(true),
            strict_function_types: Some(true),
            source_map: Some(true),
            allow_non_ts_extensions: Some(true),
            resolve_json_module: Some(true),
            ..CompilerOptions::default()
        },
        program_options: ProgramOptions::default(),
        config_file_parsing_diagnostics: Vec::new(),
    }
}

/// A project's program: the prepared program and its checker, which one
/// caller uses at a time.
pub struct ProjectProgram {
    prepared: Arc<PreparedProgram>,
    live: Mutex<LiveProgram>,
}

impl ProjectProgram {
    pub(crate) fn new(live: LiveProgram) -> Self {
        Self {
            prepared: live.shared_prepared(),
            live: Mutex::new(live),
        }
    }

    pub fn prepared(&self) -> &PreparedProgram {
        &self.prepared
    }

    /// Run `query` over the program's diagnostics and checker.
    pub fn with_live<T>(&self, query: impl FnOnce(&mut LiveProgram) -> T) -> T {
        query(&mut self.live.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// tsgo `GetSourceFileByPath(path) != nil`.
    pub(crate) fn contains_file(&self, path: &str) -> bool {
        let Ok(path) = CanonicalPath::from_js_normalized(JsStr::from_str(path)) else {
            return false;
        };
        self.prepared.source_id(&path).is_some()
            || self
                .prepared
                .package_redirect_files()
                .iter()
                .any(|file| *file.path.canonical() == path)
    }

    /// tsgo `HasSameFileNames`: the same files by path, spelled the same,
    /// and the same deduplicated package copies.
    pub(crate) fn has_same_file_names(&self, other: &Self) -> bool {
        fn files(prepared: &PreparedProgram) -> (BTreeMap<String, String>, BTreeSet<String>) {
            let sources = prepared
                .source_files()
                .iter()
                .map(|source| {
                    let path = source.path();
                    (
                        path.canonical().as_js().to_string_lossy().into_owned(),
                        path.display().to_string_lossy().into_owned(),
                    )
                })
                .collect();
            let redirects = prepared
                .package_redirect_files()
                .iter()
                .map(|file| file.path.canonical().as_js().to_string_lossy().into_owned())
                .collect();
            (sources, redirects)
        }
        files(&self.prepared) == files(&other.prepared)
    }
}

/// A project (tsgo `Project`): a configured project, a synthetic program or
/// the inferred project, with the program built for it.
#[derive(Clone)]
pub struct Project {
    pub(crate) kind: ProjectKind,
    pub(crate) id: ProjectId,
    pub(crate) current_directory: String,
    pub(crate) config_file_name: Option<String>,
    pub(crate) config_file_path: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) dirty_file_path: Option<String>,
    pub(crate) command_line: Option<CommandLine>,
    pub(crate) program: Option<Arc<ProjectProgram>>,
    pub(crate) program_update_kind: ProgramUpdateKind,
    /// The ID of the snapshot whose build made the program.
    pub(crate) program_last_update: u64,
    pub(crate) seen: Arc<SeenFiles>,
}

impl Project {
    /// tsgo `NewProject`: dirty, so the first update builds a program.
    fn new(id: ProjectId, kind: ProjectKind, current_directory: String) -> Self {
        Self {
            kind,
            id,
            current_directory,
            config_file_name: None,
            config_file_path: None,
            dirty: true,
            dirty_file_path: None,
            command_line: None,
            program: None,
            program_update_kind: ProgramUpdateKind::None,
            program_last_update: 0,
            seen: Arc::default(),
        }
    }

    /// tsgo `NewConfiguredProject`.
    pub(crate) fn new_configured(file_name: &str, path: &str, current_directory: String) -> Self {
        let id = ProjectId::configured(path)
            .unwrap_or_else(|| panic!("invalid configured project ID: {path}"));
        let mut project = Self::new(id, ProjectKind::Configured, current_directory);
        project.config_file_name = Some(file_name.to_owned());
        project.config_file_path = Some(path.to_owned());
        project
    }

    /// tsgo `NewInferredProject` with the session's default options (no
    /// `compilerOptionsForInferredProjects` reaches the API's sessions).
    pub(crate) fn new_inferred(current_directory: String, root_file_names: Vec<String>) -> Self {
        let mut project = Self::new(
            ProjectId::inferred(),
            ProjectKind::Inferred,
            current_directory,
        );
        project.command_line = Some(CommandLine::Roots(Arc::new(inferred_project_roots(
            root_file_names,
        ))));
        project
    }

    /// tsgo `newSyntheticProject`.
    pub(crate) fn new_synthetic(
        id: ProjectId,
        current_directory: String,
        roots: Arc<ProgramRoots>,
    ) -> Self {
        let mut project = Self::new(id, ProjectKind::Synthetic, current_directory);
        project.command_line = Some(CommandLine::Roots(roots));
        project
    }

    /// tsgo `Project.Clone`: a copy to change in a build; the copy was not
    /// built by it.
    pub(crate) fn cloned_for_change(&self) -> Self {
        Self {
            program_update_kind: ProgramUpdateKind::None,
            ..self.clone()
        }
    }

    /// tsgo `SetCommandLine`: a new command line always needs a full build.
    pub(crate) fn set_command_line(&mut self, command_line: CommandLine) {
        self.command_line = Some(command_line);
        self.dirty = true;
        self.dirty_file_path = None;
    }

    pub(crate) fn has_command_line(&self, command_line: &CommandLine) -> bool {
        self.command_line
            .as_ref()
            .is_some_and(|current| current.same(command_line))
    }

    /// tsgo `containsFile`.
    pub(crate) fn contains_file(&self, path: &str) -> bool {
        self.program
            .as_ref()
            .is_some_and(|program| program.contains_file(path))
    }

    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    pub fn kind(&self) -> ProjectKind {
        self.kind
    }

    pub fn current_directory(&self) -> &str {
        &self.current_directory
    }

    /// A configured project's config file name.
    pub fn config_file_name(&self) -> Option<&str> {
        self.config_file_name.as_deref()
    }

    /// tsgo `IsDirty`: a change the program does not reflect yet.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn command_line(&self) -> Option<&CommandLine> {
        self.command_line.as_ref()
    }

    /// tsgo `CommandLine.FileNames()`.
    pub fn root_file_names(&self) -> Vec<String> {
        self.command_line
            .as_ref()
            .map_or_else(Vec::new, CommandLine::file_names)
    }

    pub fn program(&self) -> Option<&Arc<ProjectProgram>> {
        self.program.as_ref()
    }

    pub fn program_update_kind(&self) -> ProgramUpdateKind {
        self.program_update_kind
    }

    /// The ID of the snapshot whose build made the program.
    pub fn program_last_update(&self) -> u64 {
        self.program_last_update
    }
}
