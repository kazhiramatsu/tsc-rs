//! tsgo `projecttestutil` for the session tests (session.rs,
//! collection_builder.rs): a session over a case-insensitive file system,
//! and the documents, edits and programs the tests use.

#![allow(dead_code)]

use std::sync::Arc;

use tsc_compiler::BoundDocument;
use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsproto::{
    DocumentUri, FileChangeType, FileEvent, LanguageKind, Position, PositionEncodingKind, Range,
    TextDocumentContentChangePartial, TextDocumentContentChangePartialOrWholeDocument,
};
use tsc_program::{LibraryCatalog, ProgramLoadLimits};
use tsc_project::{ProjectId, ProjectProgram, Session, SessionOptions};

/// tsgo `projecttestutil.Setup`: a case-insensitive file system.
pub fn setup(files: &[(&str, &str)]) -> (Session, Arc<MemFs>) {
    let fs = Arc::new(
        MemFs::from_entries(
            files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), Seed::file(text))),
            false,
            Arc::new(SystemClock),
        )
        .expect("build the file system"),
    );
    let session = Session::new(
        SessionOptions {
            current_directory: "/".to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1("/typescript/lib"),
            load_limits: ProgramLoadLimits::new(1_000, 10_000, 64, 1 << 20, 1 << 24),
            run_external_code: false,
            position_encoding: PositionEncodingKind::UTF8,
            watch_enabled: true,
            logging_enabled: true,
            push_diagnostics_enabled: true,
            debounce_delay: std::time::Duration::ZERO,
        },
        Arc::clone(&fs) as Arc<dyn FileSystem>,
        None,
        None,
        None,
    );
    (session, fs)
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
