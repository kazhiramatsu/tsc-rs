//! tsgo `projecttestutil` for the session tests: a session over a
//! case-insensitive file system with the embedded standard library, and the
//! documents, edits and programs the tests use.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tsc_compiler::locale::Locale;
use tsc_compiler::system::{BundledFs, EMBEDDED_LIBRARY_DIRECTORY};
use tsc_compiler::{BoundDocument, DEFAULT_LOAD_LIMITS};
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, FileSystemWatcher, LanguageKind, Position,
    PositionEncodingKind, PublishDiagnosticsParams, Range, TextDocumentContentChangePartial,
    TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::LibraryCatalog;
use tsc_project::{Client, ProjectId, ProjectProgram, Session, SessionOptions};

/// tsgo `projecttestutil.ClientMock`: the watch calls the session made.
#[derive(Default)]
pub struct ClientMock {
    pub watch_files_calls: Mutex<Vec<(String, Vec<FileSystemWatcher>)>>,
    pub unwatch_files_calls: Mutex<Vec<String>>,
    pub publish_diagnostics_calls: Mutex<Vec<PublishDiagnosticsParams>>,
    pub refresh_diagnostics_calls: Mutex<usize>,
    pub refresh_code_lens_calls: Mutex<usize>,
    pub refresh_inlay_hints_calls: Mutex<usize>,
    pub set_locale_calls: Mutex<Vec<String>>,
    /// tsgo `RegisterContentMapperExtensionsCalls`.
    pub register_content_mapper_extensions_calls: Mutex<Vec<Vec<String>>>,
    /// The telemetry events the session sent.
    pub telemetry_calls: Mutex<Vec<tsc_lsproto::TelemetryEvent>>,
    /// tsgo `SetLocaleFunc`.
    #[allow(clippy::type_complexity)]
    pub set_locale_func: Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>,
    /// tsgo `GetLocaleFunc`: the locale [`Client::locale`] answers (the
    /// default without one).
    #[allow(clippy::type_complexity)]
    pub locale_func: Mutex<Option<Box<dyn Fn() -> Locale + Send + Sync>>>,
}

impl ClientMock {
    pub fn watch_files_calls(&self) -> Vec<(String, Vec<FileSystemWatcher>)> {
        self.watch_files_calls.lock().unwrap().clone()
    }

    pub fn unwatch_files_calls(&self) -> Vec<String> {
        self.unwatch_files_calls.lock().unwrap().clone()
    }

    pub fn publish_diagnostics_calls(&self) -> Vec<PublishDiagnosticsParams> {
        self.publish_diagnostics_calls.lock().unwrap().clone()
    }

    pub fn refresh_diagnostics_calls(&self) -> usize {
        *self.refresh_diagnostics_calls.lock().unwrap()
    }

    pub fn refresh_code_lens_calls(&self) -> usize {
        *self.refresh_code_lens_calls.lock().unwrap()
    }

    pub fn refresh_inlay_hints_calls(&self) -> usize {
        *self.refresh_inlay_hints_calls.lock().unwrap()
    }

    pub fn set_locale_calls(&self) -> Vec<String> {
        self.set_locale_calls.lock().unwrap().clone()
    }

    pub fn register_content_mapper_extensions_calls(&self) -> Vec<Vec<String>> {
        self.register_content_mapper_extensions_calls
            .lock()
            .unwrap()
            .clone()
    }
}

impl Client for ClientMock {
    fn publish_diagnostics(&self, params: PublishDiagnosticsParams) -> Result<(), String> {
        self.publish_diagnostics_calls.lock().unwrap().push(params);
        Ok(())
    }

    fn watch_files(&self, id: &str, watchers: Vec<FileSystemWatcher>) -> Result<(), String> {
        self.watch_files_calls
            .lock()
            .unwrap()
            .push((id.to_owned(), watchers));
        Ok(())
    }

    fn send_telemetry(&self, event: tsc_lsproto::TelemetryEvent) -> Result<(), String> {
        self.telemetry_calls.lock().unwrap().push(event);
        Ok(())
    }

    fn progress_start(
        &self,
        _message: &'static tsc_diagnostics::DiagnosticMessage,
        _args: &[String],
    ) {
    }

    fn progress_finish(
        &self,
        _message: &'static tsc_diagnostics::DiagnosticMessage,
        _args: &[String],
    ) {
    }

    fn register_content_mapper_extensions(&self, extensions: &[String]) -> Result<(), String> {
        self.register_content_mapper_extensions_calls
            .lock()
            .unwrap()
            .push(extensions.to_vec());
        Ok(())
    }

    fn unwatch_files(&self, id: &str) -> Result<(), String> {
        self.unwatch_files_calls.lock().unwrap().push(id.to_owned());
        Ok(())
    }

    fn refresh_diagnostics(&self) -> Result<(), String> {
        *self.refresh_diagnostics_calls.lock().unwrap() += 1;
        Ok(())
    }

    fn refresh_inlay_hints(&self) -> Result<(), String> {
        *self.refresh_inlay_hints_calls.lock().unwrap() += 1;
        Ok(())
    }

    fn refresh_code_lens(&self) -> Result<(), String> {
        *self.refresh_code_lens_calls.lock().unwrap() += 1;
        Ok(())
    }

    fn set_locale(&self, locale: &str) {
        self.set_locale_calls
            .lock()
            .unwrap()
            .push(locale.to_owned());
        if let Some(set_locale) = &*self.set_locale_func.lock().unwrap() {
            set_locale(locale);
        }
    }

    fn locale(&self) -> Locale {
        self.locale_func
            .lock()
            .unwrap()
            .as_ref()
            .map_or_else(Locale::default, |locale| locale())
    }
}

/// tsgo `projecttestutil.Setup`: a case-insensitive file system.
pub fn setup(files: &[(&str, &str)]) -> (Session, Arc<MemFs>) {
    let (session, fs, _) = setup_with_client(files);
    (session, fs)
}

/// [`setup`] with tsgo's client mock.
pub fn setup_with_client(files: &[(&str, &str)]) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    setup_entries(
        files
            .iter()
            .map(|(name, text)| ((*name).to_owned(), Seed::file(text)))
            .collect(),
        "/",
    )
}

/// tsgo `projecttestutil.SetupWithOptions`: entries (files and links) and
/// the workspace's directory.
pub fn setup_entries(
    entries: Vec<(String, Seed)>,
    workspace: &str,
) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    setup_session(entries, workspace, None)
}

/// [`setup_entries`] with tsgo's `RunExternalCode` and the content mappers'
/// processes started by `spawner`.
pub fn setup_with_spawner(
    entries: Vec<(String, Seed)>,
    workspace: &str,
    spawner: Arc<dyn tsc_program::content_mapper::host::Spawner>,
) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    setup_session(entries, workspace, Some(spawner))
}

/// [`setup_with_spawner`] with the client's relative watch patterns (tsgo
/// `RelativePatternSupport`).
pub fn setup_with_spawner_and_relative_patterns(
    entries: Vec<(String, Seed)>,
    workspace: &str,
    spawner: Arc<dyn tsc_program::content_mapper::host::Spawner>,
) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    setup_session_with(entries, workspace, Some(spawner), |options| {
        options
            .client_capabilities
            .workspace
            .did_change_watched_files
            .relative_pattern_support = true;
    })
}

/// [`setup_entries`] with tsgo's `TelemetryEnabled`.
pub fn setup_with_telemetry(
    entries: Vec<(String, Seed)>,
    workspace: &str,
) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    setup_session_with(entries, workspace, None, |options| {
        options.telemetry_enabled = true;
    })
}

fn setup_session(
    entries: Vec<(String, Seed)>,
    workspace: &str,
    spawner: Option<Arc<dyn tsc_program::content_mapper::host::Spawner>>,
) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    setup_session_with(entries, workspace, spawner, |_| {})
}

fn setup_session_with(
    entries: Vec<(String, Seed)>,
    workspace: &str,
    spawner: Option<Arc<dyn tsc_program::content_mapper::host::Spawner>>,
    configure: impl FnOnce(&mut SessionOptions),
) -> (Session, Arc<MemFs>, Arc<ClientMock>) {
    // tsgo `GetSessionInitOptions`: every session installs typings (with
    // the npm mock and no packages).
    let (session, fs, client, _) = setup_session_full(
        entries,
        workspace,
        spawner,
        |options| {
            options.typings_location = Some(TEST_TYPINGS_LOCATION.to_owned());
            configure(options);
        },
        Some(TypingsInstallerOptions::default()),
    );
    (session, fs, client)
}

/// tsgo `projecttestutil.TestTypingsLocation`.
pub const TEST_TYPINGS_LOCATION: &str = "/home/src/Library/Caches/typescript";

/// tsgo `projecttestutil.TypingsInstallerOptions`: the registry's packages
/// (with tsgo's test versions) and the `@types` files npm installs.
#[derive(Clone, Default)]
pub struct TypingsInstallerOptions {
    pub types_registry: Vec<String>,
    pub package_to_file: BTreeMap<String, String>,
}

/// tsgo `NpmExecutorMock` with `SetupNpmExecutorForTypingsInstaller`: npm
/// writes the registry and the `@types` files into the file system.
pub struct NpmExecutorMock {
    fs: Arc<MemFs>,
    options: TypingsInstallerOptions,
    calls: Mutex<Vec<(String, Vec<String>)>>,
}

impl NpmExecutorMock {
    /// tsgo `NpmInstallCalls`: each call's directory and arguments.
    pub fn npm_install_calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

/// tsgo `TypesRegistryConfig`.
fn types_registry_config() -> serde_json::Value {
    serde_json::json!({
        "latest": "1.3.0",
        "ts2.0": "1.0.0",
        "ts2.1": "1.0.0",
        "ts2.2": "1.2.0",
        "ts2.3": "1.3.0",
        "ts2.4": "1.3.0",
        "ts2.5": "1.3.0",
        "ts2.6": "1.3.0",
        "ts2.7": "1.3.0",
    })
}

impl tsc_project::ata::NpmExecutor for NpmExecutorMock {
    fn npm_install(&self, cwd: &str, args: &[String]) -> Result<Vec<u8>, String> {
        self.calls
            .lock()
            .unwrap()
            .push((cwd.to_owned(), args.to_vec()));
        if args.len() < 3 {
            return Err(format!("unexpected npm install: {cwd} {args:?}"));
        }
        if args.len() == 3 && args[2] == "types-registry@latest" {
            let mut entries = serde_json::Map::new();
            for name in self
                .options
                .types_registry
                .iter()
                .chain(self.options.package_to_file.keys())
            {
                entries.insert(name.clone(), types_registry_config());
            }
            let text = serde_json::json!({ "entries": entries }).to_string();
            return self
                .fs
                .write_creating_dirs(
                    &format!("{cwd}/node_modules/types-registry/index.json"),
                    text.as_bytes(),
                )
                .map(|()| Vec::new())
                .map_err(|error| error.to_string());
        }
        let end = args
            .iter()
            .skip(2)
            .position(|arg| arg.starts_with("--"))
            .map_or(args.len(), |index| index + 2);
        for package in &args[2..end] {
            // `@types/<name>@<version>`.
            let package = match package.rfind('@') {
                Some(index) if index > 6 => &package[..index],
                _ => package.as_str(),
            };
            let name = &package["@types/".len()..];
            let content = self
                .options
                .package_to_file
                .get(name)
                .ok_or_else(|| format!("content not provided for {name}"))?;
            self.fs
                .write_creating_dirs(
                    &format!("{cwd}/node_modules/@types/{name}/index.d.ts"),
                    content.as_bytes(),
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(Vec::new())
    }
}

/// tsgo `projecttestutil.SetupWithTypingsInstaller`: a session that installs
/// typings in [`TEST_TYPINGS_LOCATION`] with tsgo's npm mock.
pub fn setup_with_typings_installer(
    files: &[(&str, &str)],
    typings: TypingsInstallerOptions,
) -> (Session, Arc<MemFs>, Arc<ClientMock>, Arc<NpmExecutorMock>) {
    setup_with_options_and_typings_installer(files, |_| {}, typings)
}

/// tsgo `projecttestutil.SetupWithOptionsAndTypingsInstaller`.
pub fn setup_with_options_and_typings_installer(
    files: &[(&str, &str)],
    configure: impl FnOnce(&mut SessionOptions),
    typings: TypingsInstallerOptions,
) -> (Session, Arc<MemFs>, Arc<ClientMock>, Arc<NpmExecutorMock>) {
    let entries = files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), Seed::file(text)))
        .collect();
    let (session, fs, client, npm) = setup_session_full(
        entries,
        "/",
        None,
        |options| {
            options.typings_location = Some(TEST_TYPINGS_LOCATION.to_owned());
            configure(options);
        },
        Some(typings),
    );
    (session, fs, client, npm.expect("the npm mock"))
}

/// [`setup_with_spawner`] with tsgo's typings installer options and session
/// options (`GetSessionInitOptions` with both).
pub fn setup_with_spawner_and_typings(
    entries: Vec<(String, Seed)>,
    workspace: &str,
    spawner: Arc<dyn tsc_program::content_mapper::host::Spawner>,
    configure: impl FnOnce(&mut SessionOptions),
    typings: TypingsInstallerOptions,
) -> (Session, Arc<MemFs>, Arc<ClientMock>, Arc<NpmExecutorMock>) {
    let (session, fs, client, npm) = setup_session_full(
        entries,
        workspace,
        Some(spawner),
        |options| {
            options.typings_location = Some(TEST_TYPINGS_LOCATION.to_owned());
            configure(options);
        },
        Some(typings),
    );
    (session, fs, client, npm.expect("the npm mock"))
}

#[allow(clippy::type_complexity)]
fn setup_session_full(
    entries: Vec<(String, Seed)>,
    workspace: &str,
    spawner: Option<Arc<dyn tsc_program::content_mapper::host::Spawner>>,
    configure: impl FnOnce(&mut SessionOptions),
    typings: Option<TypingsInstallerOptions>,
) -> (
    Session,
    Arc<MemFs>,
    Arc<ClientMock>,
    Option<Arc<NpmExecutorMock>>,
) {
    let client = Arc::new(ClientMock::default());
    let fs = Arc::new(
        MemFs::from_entries(entries, false, Arc::new(SystemClock)).expect("build the file system"),
    );
    let npm = typings.map(|options| {
        Arc::new(NpmExecutorMock {
            fs: Arc::clone(&fs),
            options,
            calls: Mutex::default(),
        })
    });
    let mut options = SessionOptions {
        current_directory: workspace.to_owned(),
        library_catalog: LibraryCatalog::typescript_7_1(EMBEDDED_LIBRARY_DIRECTORY),
        load_limits: DEFAULT_LOAD_LIMITS,
        run_external_code: spawner.is_some(),
        position_encoding: PositionEncodingKind::UTF8,
        watch_enabled: true,
        logging_enabled: true,
        telemetry_enabled: false,
        push_diagnostics_enabled: true,
        debounce_delay: std::time::Duration::ZERO,
        client_capabilities: Default::default(),
        typings_location: None,
    };
    configure(&mut options);
    let session = Session::new(
        options,
        // tsgo `bundled.WrapFS`: the embedded standard library.
        Arc::new(BundledFs::new(Arc::clone(&fs))) as Arc<dyn FileSystem>,
        Some(Arc::clone(&client) as Arc<dyn Client>),
        None,
        spawner,
        npm.clone()
            .map(|npm| npm as Arc<dyn tsc_project::ata::NpmExecutor>),
    );
    (session, fs, client, npm)
}

pub fn uri(uri: &str) -> DocumentUri {
    DocumentUri::from(uri)
}

pub fn open(session: &Session, document: &str, text: &str) {
    open_as(session, document, text, LanguageKind::TYPE_SCRIPT);
}

pub fn open_as(session: &Session, document: &str, text: &str, language_kind: LanguageKind) {
    session
        .did_open_file(&uri(document), 1, text, &language_kind)
        .expect("open the document");
}

/// An edit replacing `[start, end)` of the first line.
pub fn edit(start: u32, end: u32, text: &str) -> TextDocumentContentChangePartialOrWholeDocument {
    TextDocumentContentChangePartialOrWholeDocument::Partial(TextDocumentContentChangePartial {
        range: Range {
            start: Position {
                line: 0,
                character: start,
            },
            end: Position {
                line: 0,
                character: end,
            },
        },
        text: text.to_owned(),
        ..TextDocumentContentChangePartial::default()
    })
}

pub fn watch_event(document: &str, kind: FileChangeType) -> FileEvent {
    FileEvent {
        uri: uri(document),
        r#type: kind,
    }
}

/// tsgo `GetLanguageService(...).GetProgram()`: the document's default
/// project's program.
pub fn program(session: &Session, document: &str) -> Arc<ProjectProgram> {
    let (_, project) = session
        .get_snapshot_and_default_project(&uri(document))
        .expect("the document's project");
    Arc::clone(project.program().expect("the project's program"))
}

/// tsgo `GetSourceFile(...).Text()`.
pub fn source_text(program: &ProjectProgram, file_name: &str) -> Option<String> {
    program
        .prepared()
        .source_files()
        .iter()
        .find(|source| source.path().display().to_string_lossy() == file_name)
        .map(|source| source.text().to_owned())
}

/// tsgo `GetSourceFile`: the program's parsed file, compared by identity.
pub fn document(program: &ProjectProgram, file_name: &str) -> Arc<BoundDocument> {
    program
        .with_live(|live| {
            live.file_index(file_name)
                .and_then(|index| live.document(index))
                .cloned()
        })
        .expect("the file's document")
}

pub fn configured(path: &str) -> ProjectId {
    ProjectId::configured(path).expect("a configured project ID")
}

/// tsgo `SessionUtils.WatchesFile`: whether a registered watcher's glob
/// matches the file (case-insensitively, as the tests' file system).
pub fn watches_file(client: &ClientMock, file_name: &str) -> bool {
    let file_name = tsc_host::to_file_name_lower_case(file_name);
    let matches = |pattern: &str, path: &str| match pattern.strip_suffix("**/*") {
        Some(prefix) => path.starts_with(prefix) && path.len() > prefix.len(),
        None => pattern == path,
    };
    client.watch_files_calls().iter().any(|(_, watchers)| {
        watchers.iter().any(|watcher| match &watcher.glob_pattern {
            tsc_lsproto::PatternOrRelativePattern::Pattern(pattern) => {
                matches(&tsc_host::to_file_name_lower_case(pattern), &file_name)
            }
            tsc_lsproto::PatternOrRelativePattern::RelativePattern(relative) => {
                let tsc_lsproto::WorkspaceFolderOrURI::URI(uri) = &relative.base_uri else {
                    return false;
                };
                let base = tsc_program::uri::uri_to_file_name(&uri.0).unwrap_or_default();
                let base = tsc_host::to_file_name_lower_case(base.trim_end_matches('/'));
                file_name
                    .strip_prefix(&format!("{base}/"))
                    .is_some_and(|relative_path| {
                        matches(
                            &tsc_host::to_file_name_lower_case(&relative.pattern),
                            relative_path,
                        )
                    })
            }
        })
    })
}

/// The semantic diagnostics of `file_name` in the document's program.
pub fn semantic_diagnostic_count(session: &Session, document: &str, file_name: &str) -> usize {
    program(session, document).with_live(|live| {
        let file = live.file_index(file_name).expect("the file");
        live.semantic_diagnostics(file)
            .expect("check the file")
            .len()
    })
}
