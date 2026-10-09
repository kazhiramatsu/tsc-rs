//! tsgo's `snapshotfs_test.go` on the API path: the realpath aliases of
//! files read through `node_modules` links (`TestRealpathAliasLifecycle`),
//! the directory listing that keeps cached files, the snapshot's reads, and
//! the file cache clean-up after a deletion.

use super::*;
use std::sync::Arc;
use tsc_host::vfs::{MemFs, Seed, SystemClock};

fn memory(entries: &[(&str, Seed)]) -> Arc<MemFs> {
    Arc::new(
        MemFs::from_entries(
            entries
                .iter()
                .map(|(name, seed)| ((*name).to_owned(), seed.clone())),
            true,
            Arc::new(SystemClock),
        )
        .expect("build the file system"),
    )
}

fn paths() -> Paths {
    Paths {
        current_directory: "/".to_owned(),
        case_sensitive: true,
    }
}

fn empty(fs: &Arc<MemFs>) -> SnapshotFs {
    SnapshotFs::new(Arc::clone(fs) as Arc<dyn FileSystem>, paths())
}

fn builder(fs: &Arc<MemFs>, base: &SnapshotFs) -> SnapshotFsBuilder {
    SnapshotFsBuilder::new(
        Arc::clone(fs) as Arc<dyn FileSystem>,
        Arc::clone(&base.files),
        Arc::clone(&base.aliases),
        paths(),
    )
}

fn links(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn linked_library() -> Arc<MemFs> {
    memory(&[
        (
            "/project/node_modules/mylib",
            Seed::symlink("/packages/mylib"),
        ),
        (
            "/packages/mylib/package.json",
            Seed::file(r#"{"name": "mylib", "main": "index.js"}"#),
        ),
        (
            "/packages/mylib/index.d.ts",
            Seed::file("export declare const x: number;"),
        ),
        (
            "/project/node_modules/nolink/package.json",
            Seed::file(r#"{"name": "nolink"}"#),
        ),
    ])
}

#[test]
fn an_alias_is_recorded_for_a_file_read_through_a_node_modules_link() {
    // "alias recorded when reading symlinked node_modules file".
    let fs = linked_library();
    let build = builder(&fs, &empty(&fs));
    let file = build
        .get("/project/node_modules/mylib/package.json")
        .expect("the linked file");
    assert_eq!(&*file.content, br#"{"name": "mylib", "main": "index.js"}"#);
    assert!(build
        .get("/project/node_modules/nolink/package.json")
        .is_some());
    let snapshot = build.finish();
    assert_eq!(
        snapshot.aliases.get("/packages/mylib/package.json"),
        Some(&links(&["/project/node_modules/mylib/package.json"]))
    );
    assert_eq!(snapshot.aliases.len(), 1);
}

#[test]
fn no_alias_is_recorded_outside_node_modules() {
    // "no alias recorded for files outside node_modules".
    let fs = memory(&[
        ("/project/link", Seed::symlink("/elsewhere")),
        ("/elsewhere/index.ts", Seed::file("export const x = 1;")),
    ]);
    let build = builder(&fs, &empty(&fs));
    assert!(build.get("/project/link/index.ts").is_some());
    assert!(build.finish().aliases.is_empty());
}

#[test]
fn aliases_carry_over_to_the_next_snapshot() {
    // "aliases carried over across snapshots".
    let fs = linked_library();
    let build = builder(&fs, &empty(&fs));
    build.get("/project/node_modules/mylib/package.json");
    let first = build.finish();
    let second = builder(&fs, &first).finish();
    assert_eq!(
        second.aliases.get("/packages/mylib/package.json"),
        Some(&links(&["/project/node_modules/mylib/package.json"]))
    );
}

#[test]
fn a_deleted_file_is_no_longer_an_alias() {
    // "alias pruned when symlinked file is deleted": the real file goes,
    // and its deletion reaches the linked path.
    let fs = linked_library();
    let build = builder(&fs, &empty(&fs));
    build.get("/project/node_modules/mylib/package.json");
    build.get("/project/node_modules/mylib/index.d.ts");
    let first = build.finish();
    assert!(first.aliases.contains_key("/packages/mylib/index.d.ts"));

    fs.remove("/packages/mylib/index.d.ts")
        .expect("delete the file");
    let build = builder(&fs, &first);
    let changes = build.process_file_changes(
        FileChangeSummary {
            deleted: links(&["/packages/mylib/index.d.ts"]),
            ..FileChangeSummary::default()
        },
        &first,
    );
    assert!(changes
        .deleted
        .contains("/project/node_modules/mylib/index.d.ts"));
    let second = build.finish();
    assert!(second.aliases.contains_key("/packages/mylib/package.json"));
    assert!(!second.aliases.contains_key("/packages/mylib/index.d.ts"));
    assert!(!second
        .files
        .contains_key("/project/node_modules/mylib/index.d.ts"));
}

#[test]
fn links_to_one_real_file_are_aliases_until_each_goes() {
    // "multiple symlinks to same realpath" and "multiple symlinks pruned
    // individually".
    let fs = memory(&[
        ("/a/node_modules/mylib", Seed::symlink("/packages/mylib")),
        ("/b/node_modules/mylib", Seed::symlink("/packages/mylib")),
        (
            "/packages/mylib/package.json",
            Seed::file(r#"{"name": "mylib"}"#),
        ),
    ]);
    let build = builder(&fs, &empty(&fs));
    build.get("/a/node_modules/mylib/package.json");
    build.get("/b/node_modules/mylib/package.json");
    let first = build.finish();
    assert_eq!(
        first.aliases.get("/packages/mylib/package.json"),
        Some(&links(&[
            "/a/node_modules/mylib/package.json",
            "/b/node_modules/mylib/package.json",
        ]))
    );

    fs.remove("/b/node_modules/mylib").expect("delete the link");
    let build = builder(&fs, &first);
    build.process_file_changes(
        FileChangeSummary {
            deleted: links(&["/b/node_modules/mylib/package.json"]),
            ..FileChangeSummary::default()
        },
        &first,
    );
    let second = build.finish();
    assert_eq!(
        second.aliases.get("/packages/mylib/package.json"),
        Some(&links(&["/a/node_modules/mylib/package.json"]))
    );
    // The earlier snapshot keeps its aliases ("alias clone isolation").
    assert_eq!(first.aliases["/packages/mylib/package.json"].len(), 2);
}

#[test]
fn a_change_to_a_real_file_names_its_links() {
    // "expandRealpathAliases expands change events", "... delete events"
    // and "... is a no-op with no aliases".
    let fs = linked_library();
    let build = builder(&fs, &empty(&fs));
    build.get("/project/node_modules/mylib/package.json");
    let snapshot = build.finish();
    let expanded = snapshot.expand_realpath_aliases(FileChangeSummary {
        changed: links(&["/packages/mylib/package.json"]),
        deleted: links(&["/packages/mylib/package.json"]),
        ..FileChangeSummary::default()
    });
    let both = links(&[
        "/packages/mylib/package.json",
        "/project/node_modules/mylib/package.json",
    ]);
    assert_eq!(expanded.changed, both);
    assert_eq!(expanded.deleted, both);
    let unchanged = FileChangeSummary {
        changed: links(&["/some/file.ts"]),
        ..FileChangeSummary::default()
    };
    assert_eq!(
        empty(&fs).expand_realpath_aliases(unchanged.clone()),
        unchanged
    );
}

#[test]
fn a_change_to_a_real_file_reaches_the_file_read_through_its_link() {
    // "markDirtyFiles invalidates symlinked file via realpath event".
    let fs = linked_library();
    let build = builder(&fs, &empty(&fs));
    build.get("/project/node_modules/mylib/package.json");
    let first = build.finish();
    fs.write("/packages/mylib/package.json", br#"{"name": "mylib"}"#)
        .expect("write the file");
    let build = builder(&fs, &first);
    let changes = build.process_file_changes(
        FileChangeSummary {
            changed: links(&["/packages/mylib/package.json"]),
            ..FileChangeSummary::default()
        },
        &first,
    );
    assert!(changes
        .changed
        .contains("/project/node_modules/mylib/package.json"));
    let file = build
        .get("/project/node_modules/mylib/package.json")
        .expect("the linked file");
    assert_eq!(&*file.content, br#"{"name": "mylib"}"#);
    let second = build.finish();
    assert_eq!(
        &*second.files["/project/node_modules/mylib/package.json"].content,
        br#"{"name": "mylib"}"#
    );
}

#[test]
fn a_listing_keeps_the_files_the_snapshot_read() {
    // TestSnapshotFSBuilder "GetAccessibleEntries combines disk and
    // overlay", for a cached file: a file the snapshot read stays in the
    // directory's listing after it leaves the disk unannounced.
    let fs = memory(&[
        ("/p/a.ts", Seed::file("export const a = 1;")),
        ("/p/b.ts", Seed::file("export const b = 1;")),
        ("/p/sub/c.ts", Seed::file("export const c = 1;")),
    ]);
    let build = builder(&fs, &empty(&fs));
    build.get("/p/a.ts");
    fs.remove("/p/a.ts").expect("delete the file");
    let entries = build.accessible_entries("/p");
    assert_eq!(entries.files, ["a.ts", "b.ts"]);
    assert_eq!(entries.directories, ["sub"]);
}

#[test]
fn a_snapshot_reads_a_file_it_did_not_cache_once() {
    // TestSnapshotFS "GetFile reads from fs when not cached": a snapshot
    // reads such a file once (tsgo readFiles), whatever the disk holds later.
    let fs = memory(&[("/p/x.ts", Seed::file("export const x = 1;"))]);
    let snapshot = empty(&fs);
    let first = snapshot.file("/p/x.ts").expect("the file");
    fs.write("/p/x.ts", b"export const x = 2;")
        .expect("write the file");
    let again = snapshot.file("/p/x.ts").expect("the file");
    assert!(Arc::ptr_eq(&first, &again));
    assert!(snapshot.file("/p/missing.ts").is_none());
}

#[test]
fn a_deletion_drops_the_files_no_project_read() {
    // tsgo Clone's clean-up: after a deletion that builds a program again,
    // the cache keeps only the files a project's build read (the config the
    // registry parsed is not one of them; tsgo, probed).
    use crate::{ApiSnapshotRequest, ProjectId, SessionOptions, SnapshotHost};
    use tsc_program::{LibraryCatalog, ProgramLoadLimits};

    let fs = memory(&[
        (
            "/p/tsconfig.json",
            Seed::file(r#"{ "compilerOptions": { "noLib": true } }"#),
        ),
        ("/p/a.ts", Seed::file("export const a = 1;")),
        ("/p/b.ts", Seed::file("export const b = 1;")),
    ]);
    let host = SnapshotHost::new(
        SessionOptions {
            current_directory: "/".to_owned(),
            library_catalog: LibraryCatalog::typescript_7_1("/typescript/lib"),
            load_limits: ProgramLoadLimits::new(1_000, 10_000, 64, 1 << 20, 1 << 24),
        },
        Arc::clone(&fs) as Arc<dyn FileSystem>,
    );
    let id = ProjectId::configured("/p/tsconfig.json").expect("a configured ID");
    let opened = host
        .clone_snapshot(
            &host.new_root_snapshot(),
            FileChangeSummary::default(),
            Some(&ApiSnapshotRequest {
                open_projects: BTreeSet::from(["/p/tsconfig.json".to_owned()]),
                ensure_programs: BTreeSet::from([id.clone()]),
                ..ApiSnapshotRequest::default()
            }),
        )
        .expect("open the project");
    assert!(opened.fs.files.contains_key("/p/tsconfig.json"));

    fs.remove("/p/b.ts").expect("delete the file");
    let deleted = host
        .clone_snapshot(
            &opened,
            FileChangeSummary {
                deleted: links(&["/p/b.ts"]),
                ..FileChangeSummary::default()
            },
            Some(&ApiSnapshotRequest {
                ensure_programs: BTreeSet::from([id]),
                ..ApiSnapshotRequest::default()
            }),
        )
        .expect("delete the file");
    let cached = deleted.fs.files.keys().cloned().collect::<Vec<_>>();
    assert_eq!(cached, ["/p/a.ts"]);
}

#[test]
fn a_created_path_that_was_looked_for_and_missing_dirties() {
    // tsgo SeenFileOrMissingParentDirectory checks the path itself, then
    // each directory above it: a link created where the program looked for
    // a missing node_modules.
    let seen = SeenFiles {
        files: ["/project/index.ts".to_owned()].into(),
        missing_directories: ["/project/node_modules".to_owned()].into(),
    };
    assert!(seen.seen_file_or_missing_parent_directory("/project/node_modules"));
    assert!(seen.seen_file_or_missing_parent_directory("/project/node_modules/pkg/index.d.ts"));
    assert!(seen.seen_file_or_missing_parent_directory("/project/index.ts"));
    assert!(!seen.seen_file_or_missing_parent_directory("/project/other.ts"));
    assert!(!seen.seen_file_or_missing_parent_directory("/project"));
}
