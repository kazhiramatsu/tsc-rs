//! tsgo's session tests that look inside a snapshot (`snapshot_test.go`).

use std::sync::Arc;

use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsproto::{DocumentUri, LanguageKind, PositionEncodingKind};
use tsc_program::{LibraryCatalog, ProgramLoadLimits};

use super::Session;
use crate::snapshot::SessionOptions;

fn file_system(files: &[(&str, &str)]) -> Arc<MemFs> {
    Arc::new(
        MemFs::from_entries(
            files
                .iter()
                .map(|(name, text)| ((*name).to_owned(), Seed::file(text))),
            false,
            Arc::new(SystemClock),
        )
        .expect("build the file system"),
    )
}

/// tsgo `projecttestutil.Setup`, over `fs`.
fn setup(fs: &Arc<MemFs>) -> Session {
    Session::new(
        SessionOptions {
            current_directory: "/".to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1("/typescript/lib"),
            load_limits: ProgramLoadLimits::new(1_000, 10_000, 64, 1 << 20, 1 << 24),
            run_external_code: false,
            position_encoding: PositionEncodingKind::UTF8,
            watch_enabled: true,
            logging_enabled: true,
            telemetry_enabled: false,
            push_diagnostics_enabled: true,
            debounce_delay: std::time::Duration::ZERO,
            client_capabilities: Default::default(),
            typings_location: None,
        },
        Arc::clone(fs) as Arc<dyn FileSystem>,
        None,
        None,
        None,
        None,
    )
}

fn open(session: &Session, uri: &str, text: &str) {
    session
        .did_open_file(&DocumentUri::from(uri), 1, text, &LanguageKind::TYPE_SCRIPT)
        .expect("open the document");
}

#[test]
fn cached_disk_files_are_cleaned_up() {
    // "cached disk files are cleaned up": closing p1's only open file and
    // opening an unrelated one closes p1 and drops the files only it read.
    let files = [
        ("/home/projects/TS/p1/tsconfig.json", "{}"),
        (
            "/home/projects/TS/p1/index.ts",
            "import { a } from './a'; console.log(a);",
        ),
        ("/home/projects/TS/p1/a.ts", "export const a = 1;"),
        ("/home/projects/TS/p2/tsconfig.json", "{}"),
        (
            "/home/projects/TS/p2/index.ts",
            "import { b } from './b'; console.log(b);",
        ),
        ("/home/projects/TS/p2/b.ts", "export const b = 2;"),
    ];
    let session = setup(&file_system(&files));
    open(&session, "file:///home/projects/TS/p1/index.ts", files[1].1);
    open(&session, "file:///home/projects/TS/p2/index.ts", files[4].1);
    let cached = |path: &str| session.snapshot().fs.files.contains_key(path);
    assert!(cached("/home/projects/ts/p1/a.ts"));
    assert!(cached("/home/projects/ts/p2/b.ts"));

    session.did_close_file(&DocumentUri::from("file:///home/projects/TS/p1/index.ts"));
    open(&session, "untitled:Untitled-1", "");
    assert!(!cached("/home/projects/ts/p1/a.ts"));
    assert!(cached("/home/projects/ts/p2/b.ts"));
}

#[test]
fn an_unchanged_watch_change_keeps_the_program() {
    // "no-op watch change does not rebuild program": a watch change of a
    // file whose disk text is the same keeps the program; a real change
    // rebuilds it.
    let files = [
        ("/home/projects/TS/p1/tsconfig.json", "{}"),
        (
            "/home/projects/TS/p1/index.ts",
            "import { a } from './a'; console.log(a);",
        ),
        ("/home/projects/TS/p1/a.ts", "export const a = 1;"),
    ];
    let fs = file_system(&files);
    let session = setup(&fs);
    let index = DocumentUri::from("file:///home/projects/TS/p1/index.ts");
    open(&session, index.as_str(), files[1].1);
    let program = |session: &Session| {
        let (_, project) = session
            .get_snapshot_and_default_project(&index)
            .expect("the project of index.ts");
        Arc::clone(project.program().expect("the program"))
    };
    let before = program(&session);
    let change = [tsc_lsproto::FileEvent {
        uri: DocumentUri::from("file:///home/projects/TS/p1/a.ts"),
        r#type: tsc_lsproto::FileChangeType::CHANGED,
    }];

    session.did_change_watched_files(&change);
    assert!(Arc::ptr_eq(&program(&session), &before));

    fs.write("/home/projects/TS/p1/a.ts", b"export const a = 2;")
        .expect("write a.ts");
    session.did_change_watched_files(&change);
    assert!(!Arc::ptr_eq(&program(&session), &before));
}
