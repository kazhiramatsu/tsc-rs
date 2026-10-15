//! tsgo `projecttestutil` for the session tests: a session over a
//! case-insensitive file system with the embedded standard library, and the
//! documents, edits and programs the tests use.

#![allow(dead_code)]

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
    let client = Arc::new(ClientMock::default());
    let fs = Arc::new(
        MemFs::from_entries(entries, false, Arc::new(SystemClock)).expect("build the file system"),
    );
    let mut options = SessionOptions {
        current_directory: workspace.to_owned(),
        library_catalog: LibraryCatalog::typescript_7_1(EMBEDDED_LIBRARY_DIRECTORY),
        load_limits: DEFAULT_LOAD_LIMITS,
        run_external_code: spawner.is_some(),
        position_encoding: PositionEncodingKind::UTF8,
        watch_enabled: true,
        logging_enabled: true,
        push_diagnostics_enabled: true,
        debounce_delay: std::time::Duration::ZERO,
        client_capabilities: Default::default(),
    };
    configure(&mut options);
    let session = Session::new(
        options,
        // tsgo `bundled.WrapFS`: the embedded standard library.
        Arc::new(BundledFs::new(Arc::clone(&fs))) as Arc<dyn FileSystem>,
        Some(Arc::clone(&client) as Arc<dyn Client>),
        None,
        spawner,
    );
    (session, fs, client)
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
