//! tsgo `projecttestutil` for the session tests (session.rs,
//! collection_builder.rs): a session over a case-insensitive file system,
//! and the documents, edits and programs the tests use.

#![allow(dead_code)]

use std::sync::{Arc, Mutex};

use tsc_compiler::locale::Locale;
use tsc_compiler::BoundDocument;
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, FileSystemWatcher, LanguageKind, Position,
    PositionEncodingKind, Range, TextDocumentContentChangePartial,
    TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::{LibraryCatalog, ProgramLoadLimits};
use tsc_project::{Client, ProjectId, ProjectProgram, Session, SessionOptions};

/// tsgo `projecttestutil.ClientMock`: the watch calls the session made.
#[derive(Default)]
pub struct ClientMock {
    pub watch_files_calls: Mutex<Vec<(String, Vec<FileSystemWatcher>)>>,
    pub unwatch_files_calls: Mutex<Vec<String>>,
}

impl ClientMock {
    pub fn watch_files_calls(&self) -> Vec<(String, Vec<FileSystemWatcher>)> {
        self.watch_files_calls.lock().unwrap().clone()
    }

    pub fn unwatch_files_calls(&self) -> Vec<String> {
        self.unwatch_files_calls.lock().unwrap().clone()
    }
}

impl Client for ClientMock {
    fn watch_files(&self, id: &str, watchers: Vec<FileSystemWatcher>) -> Result<(), String> {
        self.watch_files_calls
            .lock()
            .unwrap()
            .push((id.to_owned(), watchers));
        Ok(())
    }

    fn unwatch_files(&self, id: &str) -> Result<(), String> {
        self.unwatch_files_calls.lock().unwrap().push(id.to_owned());
        Ok(())
    }

    fn refresh_inlay_hints(&self) -> Result<(), String> {
        Ok(())
    }

    fn refresh_code_lens(&self) -> Result<(), String> {
        Ok(())
    }

    fn set_locale(&self, _locale: &str) {}

    fn locale(&self) -> Locale {
        Locale::default()
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
    let client = Arc::new(ClientMock::default());
    let fs = Arc::new(
        MemFs::from_entries(entries, false, Arc::new(SystemClock)).expect("build the file system"),
    );
    let session = Session::new(
        SessionOptions {
            current_directory: workspace.to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1("/typescript/lib"),
            load_limits: ProgramLoadLimits::new(1_000, 10_000, 64, 1 << 20, 1 << 24),
            run_external_code: false,
            position_encoding: PositionEncodingKind::UTF8,
            watch_enabled: true,
            logging_enabled: true,
            push_diagnostics_enabled: true,
            debounce_delay: std::time::Duration::ZERO,
            relative_pattern_support: false,
        },
        Arc::clone(&fs) as Arc<dyn FileSystem>,
        Some(Arc::clone(&client) as Arc<dyn Client>),
        None,
        None,
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
