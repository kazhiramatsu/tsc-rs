//! tsgo `overlayfs_test.go`: the editor's and the watcher's notifications
//! reduced and applied (`TestProcessChanges`), and the open documents as a
//! file system (`TestOverlayFSFileSystem`).

use std::sync::Arc;

use tsc_host::vfs::{FileSystem, MemFs, Seed, SystemClock};
use tsc_lsproto::{
    DocumentUri, LanguageKind, Position, PositionEncodingKind, Range,
    TextDocumentContentChangePartial, TextDocumentContentChangePartialOrWholeDocument,
    TextDocumentContentChangeWholeDocument,
};
use tsc_syntax::ScriptKind;

use super::*;

const URI1: &str = "file:///test1.ts";
const URI2: &str = "file:///test2.ts";

fn memory(files: &[(&str, &str)]) -> Arc<MemFs> {
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

fn overlay_fs() -> OverlayFs {
    let host = memory(&[
        ("/test1.ts", "// existing content"),
        ("/test2.ts", "// existing content"),
        ("/script", "// extensionless content"),
    ]);
    OverlayFs::new(
        host,
        Paths {
            current_directory: "/".to_owned(),
            case_sensitive: false,
        },
        PositionEncodingKind::UTF16,
    )
}

fn uri(text: &str) -> DocumentUri {
    DocumentUri(text.to_owned())
}

fn open(uri_text: &str, version: i32, content: &str, language: &str) -> FileChange {
    FileChange {
        kind: FileChangeKind::Open,
        uri: uri(uri_text),
        version,
        content: content.to_owned(),
        language_kind: LanguageKind(language.to_owned().into()),
        changes: Vec::new(),
    }
}

fn change(
    uri_text: &str,
    version: i32,
    edits: Vec<TextDocumentContentChangePartialOrWholeDocument>,
) -> FileChange {
    FileChange {
        version,
        changes: edits,
        ..FileChange::new(FileChangeKind::Change, uri(uri_text))
    }
}

fn whole(text: &str) -> TextDocumentContentChangePartialOrWholeDocument {
    TextDocumentContentChangePartialOrWholeDocument::WholeDocument(
        TextDocumentContentChangeWholeDocument {
            text: text.to_owned(),
        },
    )
}

fn partial(
    start: (u32, u32),
    end: (u32, u32),
    text: &str,
) -> TextDocumentContentChangePartialOrWholeDocument {
    let position = |(line, character)| Position { line, character };
    TextDocumentContentChangePartialOrWholeDocument::Partial(TextDocumentContentChangePartial {
        range: Range {
            start: position(start),
            end: position(end),
        },
        text: text.to_owned(),
        ..TextDocumentContentChangePartial::default()
    })
}

fn just(kind: FileChangeKind, uri_text: &str) -> FileChange {
    FileChange::new(kind, uri(uri_text))
}

/// The open documents over the host's file system, as a file system.
fn layer(fs: &OverlayFs) -> OverlayLayer {
    OverlayLayer::new(Arc::clone(&fs.host), fs.paths.clone(), fs.overlays())
}

#[test]
#[should_panic(expected = "can only process one file open event at a time")]
fn multiple_opens_panic() {
    overlay_fs().process_changes(vec![
        open(URI1, 1, "const x = 1;", "typescript"),
        open(URI2, 1, "const y = 2;", "typescript"),
    ]);
}

#[test]
fn watch_create_then_delete_becomes_nothing() {
    let (result, _) = overlay_fs().process_changes(vec![
        just(FileChangeKind::WatchCreate, URI1),
        just(FileChangeKind::WatchDelete, URI1),
    ]);
    assert!(result.is_empty());
}

#[test]
fn watch_delete_then_create_becomes_change() {
    let (result, _) = overlay_fs().process_changes(vec![
        just(FileChangeKind::WatchDelete, URI1),
        just(FileChangeKind::WatchCreate, URI1),
    ]);
    assert!(result.created.is_empty());
    assert!(result.deleted.is_empty());
    assert!(result.changed.contains("/test1.ts"));
}

#[test]
fn multiple_watch_changes_deduplicated() {
    let (result, _) = overlay_fs().process_changes(vec![
        just(FileChangeKind::WatchChange, URI1),
        just(FileChangeKind::WatchChange, URI1),
        just(FileChangeKind::WatchChange, URI1),
    ]);
    assert_eq!(result.changed.len(), 1);
    assert!(result.changed.contains("/test1.ts"));
}

#[test]
fn save_marks_overlay_as_matching_disk() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open(URI1, 1, "const x = 1;", "typescript")]);
    let (result, _) = fs.process_changes(vec![just(FileChangeKind::Save, URI1)]);
    // Saves are not changes of a snapshot.
    assert!(result.is_empty());
    assert!(layer(&fs).overlay("/test1.ts").unwrap().matches_disk_text());
}

#[test]
fn open_falls_back_to_file_extension_for_unknown_language_kind() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open(
        "file:///test1.mts",
        1,
        "export const x = 1;",
        "mts",
    )]);
    assert_eq!(
        layer(&fs).overlay("/test1.mts").unwrap().kind(),
        ScriptKind::Ts
    );
}

#[test]
fn open_extensionless_file_preserves_unknown_script_kind() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open("file:///script", 1, "const x = 1;", "plaintext")]);
    assert_eq!(
        layer(&fs).overlay("/script").unwrap().kind(),
        ScriptKind::Unknown
    );
}

#[test]
fn watch_change_on_overlay_marks_as_not_matching_disk() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open(URI1, 1, "const x = 1;", "typescript")]);
    assert!(!layer(&fs).overlay("/test1.ts").unwrap().matches_disk_text());
    fs.process_changes(vec![just(FileChangeKind::Save, URI1)]);
    assert!(layer(&fs).overlay("/test1.ts").unwrap().matches_disk_text());
    fs.process_changes(vec![just(FileChangeKind::WatchChange, URI1)]);
    assert!(!layer(&fs).overlay("/test1.ts").unwrap().matches_disk_text());
}

#[test]
fn save_without_overlay_is_a_disk_change() {
    let (result, _) = overlay_fs().process_changes(vec![just(FileChangeKind::Save, URI1)]);
    assert!(result.changed.contains("/test1.ts"));
}

#[test]
fn close_and_change_without_overlay_do_nothing() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open(URI1, 1, "const x = 1;", "typescript")]);
    fs.process_changes(vec![just(FileChangeKind::Close, URI1)]);
    let (result, _) = fs.process_changes(vec![just(FileChangeKind::Close, URI1)]);
    assert!(result.is_empty());
    let (result, _) = fs.process_changes(vec![change(URI1, 2, vec![whole("const x = 1;")])]);
    assert!(result.is_empty());
}

#[test]
fn close_then_open_in_same_batch_marks_as_changed() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open(URI1, 1, "const x = 1;", "typescript")]);
    // Neovim reloads a file this way.
    let (result, _) = fs.process_changes(vec![
        just(FileChangeKind::Close, URI1),
        open(URI1, 0, "const x = 2;", "typescript"),
    ]);
    assert_eq!(result.opened, None);
    assert!(result.changed.contains("/test1.ts"));
    assert_eq!(
        layer(&fs).overlay("/test1.ts").unwrap().content(),
        "const x = 2;"
    );
}

/// Edits apply in order, each against the text the previous one left, at
/// UTF-16 positions.
#[test]
fn edits_apply_in_order() {
    let mut fs = overlay_fs();
    fs.process_changes(vec![open(URI1, 1, "let α = 1;\nlet b = 2;", "typescript")]);
    let (result, _) = fs.process_changes(vec![
        change(
            URI1,
            2,
            vec![partial((0, 4), (0, 5), "x"), partial((1, 8), (1, 9), "3")],
        ),
        change(URI1, 3, vec![partial((0, 0), (0, 0), "// edit\n")]),
    ]);
    assert!(result.changed.contains("/test1.ts"));
    let overlay = Arc::clone(layer(&fs).overlay("/test1.ts").unwrap());
    assert_eq!(overlay.content(), "// edit\nlet x = 1;\nlet b = 3;");
    assert_eq!(overlay.version(), 3);
    assert!(!overlay.matches_disk_text());
}

/// tsgo `TestOverlayFSFileSystem`.
#[test]
fn overlays_are_files_over_the_host() {
    let host = memory(&[("/virtual", "host file")]);
    let paths = Paths {
        current_directory: "/".to_owned(),
        case_sensitive: false,
    };
    let mut overlays = std::collections::BTreeMap::new();
    overlays.insert(
        "/virtual/nested/file.ts".to_owned(),
        Arc::new(Overlay::new(
            "/virtual/nested/file.ts",
            "overlay",
            1,
            ScriptKind::Ts,
        )),
    );
    let fs = OverlayLayer::new(host, paths, Arc::new(overlays));
    assert!(fs.is_dir("/virtual"));
    assert!(!fs.is_file("/virtual"));
    assert!(fs.metadata("/virtual").unwrap().is_dir());
    assert_eq!(fs.read("/virtual/nested/file.ts").unwrap(), b"overlay");
    let root = fs.accessible_entries("/");
    assert!(root.directories.iter().any(|name| name == "virtual"));
    assert!(!root.files.iter().any(|name| name == "virtual"));
}
