//! The in-memory file system against tsgo's `vfstest` tests
//! (internal/vfs/vfstest/vfstest_test.go) and the adapters around it.

use std::io;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tsc_diagnostics::{JsStr, JsString};

use super::*;
use crate::CompilerHost;

fn start() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

fn clock() -> Arc<dyn Clock> {
    Arc::new(SteppingClock::new(start(), Duration::from_secs(1)))
}

fn seconds(time: SystemTime) -> u64 {
    time.duration_since(start())
        .expect("after the start")
        .as_secs()
}

fn fs_of(entries: &[(&str, Seed)], case_sensitive: bool) -> MemFs {
    MemFs::from_entries(
        entries.iter().map(|(path, seed)| (*path, seed.clone())),
        case_sensitive,
        clock(),
    )
    .expect("valid entries")
}

fn text(fs: &MemFs, path: &str) -> String {
    String::from_utf8(fs.read(path).expect(path)).expect("UTF-8")
}

fn names(entries: Vec<DirEntry>) -> Vec<String> {
    entries
        .into_iter()
        .map(|entry| entry.name().to_owned())
        .collect()
}

#[test]
fn normalize_resolves_segments_separators_and_drive_roots() {
    assert_eq!(normalize("/a/./b/../c/").unwrap(), "/a/c");
    assert_eq!(normalize("/").unwrap(), "/");
    assert_eq!(normalize("/..").unwrap(), "/");
    assert_eq!(normalize("c:\\Work\\x.ts").unwrap(), "c:/Work/x.ts");
    assert_eq!(normalize("C:").unwrap(), "C:/");
    assert_eq!(
        normalize("relative/x.ts").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        normalize("bundled:///libs/lib.d.ts").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
}

#[test]
fn insensitive_names_fold_and_keep_their_spelling() {
    for spelling in ["foo", "Foo"] {
        let upper = spelling == "Foo";
        let bar = |name: &str| {
            if upper {
                name.to_uppercase()
            } else {
                name.to_owned()
            }
        };
        let fs = fs_of(
            &[
                (
                    &format!("/{spelling}/{}/baz", bar("bar")),
                    Seed::file("bar"),
                ),
                (
                    &format!("/{spelling}/{}2/baz2", bar("bar")),
                    Seed::file("bar"),
                ),
                (
                    &format!("/{spelling}/{}3/baz3", bar("bar")),
                    Seed::file("bar"),
                ),
            ],
            false,
        );
        for query in ["/foo/bar/baz", "/Foo/Bar/Baz", "/FOO/BAR/BAZ"] {
            assert_eq!(text(&fs, query), "bar");
            assert!(fs.is_file(query));
            assert_eq!(
                fs.canonicalize(query).unwrap(),
                format!("/{spelling}/{}/baz", bar("bar"))
            );
        }
        for query in ["/foo", "/Foo"] {
            assert_eq!(
                names(fs.read_dir(query).unwrap()),
                [
                    bar("bar"),
                    format!("{}2", bar("bar")),
                    format!("{}3", bar("bar"))
                ]
            );
        }
        let missing = fs.canonicalize("/Does/Not/Exist").unwrap_err();
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert!(missing.to_string().contains("file does not exist"));
        assert!(fs.metadata("/does/not/exist").is_err());
    }
}

#[test]
fn sensitive_names_are_distinct() {
    let fs = fs_of(
        &[
            ("/foo/bar/baz", Seed::file("bar")),
            ("/Foo/Bar/Baz", Seed::file("BAR")),
        ],
        true,
    );
    assert_eq!(text(&fs, "/foo/bar/baz"), "bar");
    assert_eq!(text(&fs, "/Foo/Bar/Baz"), "BAR");
    assert!(fs.read("/FOO/BAR/BAZ").is_err());
    assert_eq!(names(fs.read_dir("/").unwrap()), ["Foo", "foo"]);
}

#[test]
fn duplicate_canonical_paths_are_rejected_only_when_names_fold() {
    let entries = [
        ("/foo/bar/baz", Seed::file("bar")),
        ("/Foo/Bar/Baz", Seed::file("bar")),
    ];
    assert!(MemFs::from_entries(entries.clone(), true, clock()).is_ok());
    let error = MemFs::from_entries(entries, false, clock()).unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"duplicate path: "/Foo/Bar/Baz" and "/foo/bar/baz" have the same canonical path"#
    );
}

#[test]
fn an_entry_below_a_file_is_rejected() {
    let error = MemFs::from_entries(
        [
            ("/foo", Seed::file("bar")),
            ("/foo/oops", Seed::file("baz")),
        ],
        false,
        clock(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .starts_with(r#"failed to create intermediate directories for "/foo/oops""#));
}

#[test]
fn seeds_must_be_normalized_and_rooted_alike() {
    let error = MemFs::from_entries([("/a/../b", Seed::file(""))], true, clock()).unwrap_err();
    assert_eq!(error.to_string(), r#"non-normalized path "/a/../b""#);
    let error = MemFs::from_entries(
        [("/a", Seed::file("")), ("c:/b", Seed::file(""))],
        true,
        clock(),
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "mixed posix and windows paths");
}

#[test]
fn writes_replace_files_and_never_go_below_one() {
    let fs = MemFs::new(false, clock());
    fs.write_creating_dirs("/foo/bar/baz", b"hello, world")
        .unwrap();
    assert_eq!(text(&fs, "/foo/bar/baz"), "hello, world");
    fs.write_creating_dirs("/foo/bar/baz", b"goodbye, world")
        .unwrap();
    assert_eq!(text(&fs, "/foo/bar/baz"), "goodbye, world");
    let error = fs
        .write_creating_dirs("/foo/bar/baz/oops", b"goodbye, world")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        r#"mkdir "/foo/bar/baz": path exists but is not a directory"#
    );
    // A plain write needs its directory.
    assert_eq!(
        fs.write("/missing/x.ts", b"").unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
    fs.append("/foo/bar/baz", b"!").unwrap();
    assert_eq!(text(&fs, "/foo/bar/baz"), "goodbye, world!");
    fs.append("/foo/bar/new", b"x").unwrap();
    assert_eq!(text(&fs, "/foo/bar/new"), "x");
}

#[test]
fn removal_takes_directories_whole_and_ignores_missing_entries() {
    let fs = MemFs::new(false, clock());
    fs.write_creating_dirs("/foo/bar/file.ts", b"remove")
        .unwrap();
    assert!(fs.is_file("/foo/bar/file.ts"));
    fs.remove("/foo/bar/file.ts").unwrap();
    assert!(!fs.is_file("/foo/bar/file.ts"));

    fs.write_creating_dirs("/foo/bar/test/remove2.ts", b"remove2")
        .unwrap();
    assert!(fs.is_dir("/foo/bar/test"));
    fs.remove("/foo/bar/test").unwrap();
    assert!(!fs.is_file("/foo/bar/test/remove2.ts"));
    assert!(!fs.is_dir("/foo/bar/test"));

    fs.remove("/foo/bar/test").unwrap();
    fs.remove("/foo/bar/file.ts").unwrap();

    fs.write_creating_dirs("/foo/barbar", b"remove2").unwrap();
    fs.remove("/foo/bar").unwrap();
    assert!(fs.is_file("/foo/barbar"));
}

#[test]
fn concurrent_operations_keep_the_tree_consistent() {
    let fs = MemFs::new(false, clock());
    std::thread::scope(|scope| {
        for thread in 0..4 {
            let fs = &fs;
            scope.spawn(move || {
                for round in 0..2_000 {
                    match (round + thread) % 8 {
                        0 => {
                            let _ = fs.write_creating_dirs("/foo/bar/baz.txt", b"hello, world");
                        }
                        1 => {
                            let _ = fs.read("/foo/bar/baz.txt");
                        }
                        2 => {
                            let _ = fs.is_dir("/foo/bar");
                        }
                        3 => {
                            let _ = fs.is_file("/foo/bar");
                        }
                        4 => {
                            let _ = fs.is_file("/foo/bar/baz.txt");
                        }
                        5 => {
                            let _ = fs.accessible_entries("/foo/bar");
                        }
                        6 => {
                            let _ = fs.canonicalize("/foo/bar/baz.txt");
                        }
                        _ => {
                            let _ = fs.metadata("/foo/bar/baz.txt");
                        }
                    }
                }
            });
        }
    });
    assert_eq!(text(&fs, "/foo/bar/baz.txt"), "hello, world");
}

fn linked() -> MemFs {
    fs_of(
        &[
            ("/foo.ts", Seed::file("hello, world")),
            ("/symlink.ts", Seed::symlink("/foo.ts")),
            ("/some/dir/file.ts", Seed::file("hello, world")),
            ("/some/dirlink", Seed::symlink("/some/dir")),
            ("/a", Seed::symlink("/b")),
            ("/b", Seed::symlink("/c")),
            ("/c", Seed::symlink("/d")),
            ("/d/existing.ts", Seed::file("this is existing.ts")),
        ],
        false,
    )
}

#[test]
fn reads_follow_links_and_chains() {
    let fs = linked();
    assert_eq!(text(&fs, "/symlink.ts"), "hello, world");
    assert_eq!(text(&fs, "/some/dirlink/file.ts"), "hello, world");
    assert_eq!(text(&fs, "/a/existing.ts"), "this is existing.ts");

    assert_eq!(fs.canonicalize("/symlink.ts").unwrap(), "/foo.ts");
    assert_eq!(fs.canonicalize("/some/dirlink").unwrap(), "/some/dir");
    assert_eq!(
        fs.canonicalize("/some/dirlink/file.ts").unwrap(),
        "/some/dir/file.ts"
    );

    assert!(fs.is_file("/symlink.ts"));
    assert!(fs.is_file("/some/dirlink/file.ts"));
    assert!(fs.is_file("/a/existing.ts"));
    for directory in ["/some/dirlink", "/d", "/c", "/b", "/a"] {
        assert!(fs.is_dir(directory), "{directory}");
    }
    assert!(fs.symlink_metadata("/a").unwrap().is_symlink());
    assert_eq!(fs.read_link("/a").unwrap(), "/b");
}

#[test]
fn writes_go_through_links_and_keep_their_spelling() {
    let fs = fs_of(
        &[
            ("/some/dir/other.ts", Seed::file("NOTHING")),
            ("/other.ts", Seed::symlink("/some/dir/other.ts")),
            ("/some/dirlink", Seed::symlink("/some/dir")),
            ("/brokenlink", Seed::symlink("/does/not/exist")),
            ("/a", Seed::symlink("/b")),
            ("/b", Seed::symlink("/c")),
            ("/c", Seed::symlink("/d")),
            ("/d/existing.ts", Seed::file("hello, world")),
        ],
        false,
    );
    fs.write_creating_dirs("/some/dirlink/file.ts", b"hello, world")
        .unwrap();
    assert_eq!(text(&fs, "/some/dirlink/file.ts"), "hello, world");
    assert_eq!(text(&fs, "/some/dir/file.ts"), "hello, world");
    // The file is stored in the linked directory, spelled as written.
    assert_eq!(
        fs.canonicalize("/some/dir/file.ts").unwrap(),
        "/some/dirlink/file.ts"
    );

    fs.write_creating_dirs("/some/dirlink/file.ts", b"goodbye, world")
        .unwrap();
    assert_eq!(text(&fs, "/some/dirlink/file.ts"), "goodbye, world");

    fs.write_creating_dirs("/other.ts", b"hello, world")
        .unwrap();
    assert_eq!(text(&fs, "/other.ts"), "hello, world");
    assert_eq!(text(&fs, "/some/dir/other.ts"), "hello, world");

    assert_eq!(
        fs.write_creating_dirs("/some/dirlink", b"hello, world")
            .unwrap_err()
            .to_string(),
        r#"write "/some/dirlink": path exists but is not a regular file"#
    );
    for path in ["/brokenlink/file.ts", "/brokenlink/also/wrong/file.ts"] {
        assert_eq!(
            fs.write_creating_dirs(path, b"hello, world")
                .unwrap_err()
                .to_string(),
            r#"broken symlink "/brokenlink" -> "/does/not/exist""#
        );
    }
    // A link to a missing file is written at its target.
    fs.write_creating_dirs("/brokenlink", b"hello, world")
        .unwrap();
    assert_eq!(text(&fs, "/brokenlink"), "hello, world");
    assert_eq!(text(&fs, "/does/not/exist"), "hello, world");
}

#[test]
fn directories_are_created_through_link_chains() {
    let fs = fs_of(
        &[
            ("/a", Seed::symlink("/b")),
            ("/b", Seed::symlink("/c")),
            ("/c", Seed::symlink("/d")),
            ("/d/existing.ts", Seed::file("hello, world")),
        ],
        false,
    );
    fs.write_creating_dirs("/a/foo/bar/new.ts", b"this is new.ts")
        .unwrap();
    for path in [
        "/a/foo/bar/new.ts",
        "/b/foo/bar/new.ts",
        "/d/foo/bar/new.ts",
    ] {
        assert_eq!(text(&fs, path), "this is new.ts");
    }
    assert_eq!(fs.canonicalize("/a/foo").unwrap(), "/d/foo");

    let fs = fs_of(
        &[
            ("/a", Seed::symlink("/b")),
            ("/b", Seed::symlink("/c")),
            ("/c", Seed::symlink("/d")),
            ("/d", Seed::file("hello, world")),
        ],
        false,
    );
    assert_eq!(
        fs.write_creating_dirs("/a/foo/bar/new.ts", b"this is new.ts")
            .unwrap_err()
            .to_string(),
        r#"mkdir "/d": path exists but is not a directory"#
    );
}

#[test]
fn removing_a_link_leaves_its_target_and_links_survive_their_target() {
    let fs = fs_of(
        &[
            ("/some/dir/other.ts", Seed::file("NOTHING")),
            ("/other.ts", Seed::symlink("/some/dir/other.ts")),
            ("/some/dirlink", Seed::symlink("/some/dir")),
            ("/brokenlink", Seed::symlink("/does/not/exist")),
            ("/a", Seed::symlink("/b")),
            ("/b", Seed::symlink("/c")),
            ("/c", Seed::symlink("/d")),
            ("/d/existing.ts", Seed::file("hello, world")),
        ],
        false,
    );
    fs.remove("/a").unwrap();
    assert!(!fs.is_dir("/a"));
    assert!(fs.is_dir("/b"));
    assert!(fs.is_dir("/c"));
    assert!(fs.is_file("/d/existing.ts"));

    fs.remove("/d").unwrap();
    assert!(!fs.is_dir("/b"));
    assert!(!fs.is_dir("/c"));
    assert!(!fs.is_dir("/d"));
    assert!(!fs.is_file("/d/again.ts"));
    fs.write_creating_dirs("/d/again.ts", b"d exists again")
        .unwrap();
    assert!(fs.is_dir("/b"));
    assert!(fs.is_dir("/c"));
    assert_eq!(text(&fs, "/b/again.ts"), "d exists again");

    assert!(!fs.is_file("/brokenlink"));
    assert!(!fs.is_dir("/brokenlink"));
    fs.remove("/does/not/exist").unwrap();
    fs.write_creating_dirs("/does/not/exist", b"hello, world")
        .unwrap();
    assert!(fs.is_file("/brokenlink"));
    // Removal and times name the entry itself, not where a link leads.
    assert_eq!(
        fs.set_modified("/some/dirlink/other.ts", start())
            .unwrap_err()
            .kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn seeds_are_stamped_in_component_order_before_their_directories() {
    let fs = fs_of(
        &[
            ("/p/src/x.ts", Seed::file("x")),
            ("/p/src.ts", Seed::file("src")),
            ("/p/a-c", Seed::file("a-c")),
            ("/p/a/b", Seed::file("a/b")),
        ],
        true,
    );
    let stamp = |path: &str| seconds(fs.entry(path).expect(path).modified());
    // tsgo comparePathsByParts: `a-c` sorts before `a/b`, `src.ts` before
    // `src/x.ts`; then the directories, in the same order.
    assert_eq!(stamp("/p/a-c"), 1);
    assert_eq!(stamp("/p/a/b"), 2);
    assert_eq!(stamp("/p/src.ts"), 3);
    assert_eq!(stamp("/p/src/x.ts"), 4);
    assert_eq!(stamp("/p"), 5);
    assert_eq!(stamp("/p/a"), 6);
    assert_eq!(stamp("/p/src"), 7);
    fs.write("/p/src.ts", b"changed").unwrap();
    assert_eq!(stamp("/p/src.ts"), 8);
    fs.set_modified("/p/src.ts", start() + Duration::from_secs(100))
        .unwrap();
    assert_eq!(stamp("/p/src.ts"), 100);
    assert_eq!(fs.metadata("/p/src.ts").unwrap().len(), 7);
}

#[test]
fn entries_report_files_directories_and_links_with_their_spelling() {
    let fs = fs_of(
        &[
            ("/W/b.ts", Seed::file("b")),
            ("/W/link", Seed::symlink("/W/b.ts")),
        ],
        false,
    );
    let entries = fs
        .entries()
        .into_iter()
        .map(|entry| (entry.path().to_owned(), entry.contents().clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        [
            ("/W".to_owned(), EntryContents::Directory),
            (
                "/W/b.ts".to_owned(),
                EntryContents::File(Arc::from(&b"b"[..]))
            ),
            (
                "/W/link".to_owned(),
                EntryContents::Symlink("/W/b.ts".to_owned())
            ),
        ]
    );
}

#[test]
fn accessible_entries_follow_links_and_leave_out_broken_ones() {
    let fs = fs_of(
        &[
            ("/w/file.ts", Seed::file("")),
            ("/w/dir/x.ts", Seed::file("")),
            ("/w/filelink", Seed::symlink("/w/file.ts")),
            ("/w/dirlink", Seed::symlink("/w/dir")),
            ("/w/broken", Seed::symlink("/w/missing")),
        ],
        true,
    );
    let entries = fs.accessible_entries("/w");
    assert_eq!(entries.files, ["file.ts", "filelink"]);
    assert_eq!(entries.directories, ["dir", "dirlink"]);
    assert_eq!(
        entries.symlinks.into_iter().collect::<Vec<_>>(),
        ["dirlink", "filelink"]
    );
    assert_eq!(fs.accessible_entries("/missing"), Entries::default());
}

#[test]
fn drive_rooted_paths_work_like_slash_rooted_ones() {
    let fs = fs_of(
        &[
            ("c:/home/src/a.ts", Seed::file("a")),
            ("c:/home/src/link.ts", Seed::symlink("c:/home/src/a.ts")),
        ],
        false,
    );
    assert_eq!(text(&fs, "C:/Home/src/link.ts"), "a");
    assert_eq!(
        fs.canonicalize("c:/home/src/link.ts").unwrap(),
        "c:/home/src/a.ts"
    );
    fs.write_creating_dirs("c:/home/out/a.js", b"a").unwrap();
    assert_eq!(names(fs.read_dir("c:/home").unwrap()), ["out", "src"]);
}

#[test]
fn the_compiler_host_reads_what_the_file_system_holds() {
    let fs = fs_of(
        &[
            ("/work/b.ts", Seed::file("b")),
            ("/work/A.ts", Seed::file("A")),
            ("/work/sub/c.ts", Seed::file("c")),
            ("/work/link", Seed::symlink("/work/sub")),
            ("/work/broken", Seed::symlink("/work/missing")),
        ],
        true,
    );
    let host = VfsCompilerHost::new(&fs, "/work");
    let js = JsStr::from_str;
    assert_eq!(host.read_file_js(js("b.ts")).unwrap(), Some(b"b".to_vec()));
    assert_eq!(host.read_file_js(js("/work/missing.ts")).unwrap(), None);
    assert_eq!(host.read_file_js(js("/work/sub")).unwrap(), None);
    assert!(host.file_exists_js(js("/work/link/c.ts")).unwrap());
    assert!(host.directory_exists_js(js("/work/link")).unwrap());
    assert!(!host.file_exists_js(js("/work/broken")).unwrap());
    let listing = host.read_directory_listing_js(js("/work")).unwrap();
    assert_eq!(
        listing
            .iter()
            .map(|entry| (
                entry.path.to_string_lossy().into_owned(),
                entry.kind,
                entry.symlink
            ))
            .collect::<Vec<_>>(),
        [
            ("/work/A.ts".to_owned(), DirectoryListingKind::File, false),
            ("/work/b.ts".to_owned(), DirectoryListingKind::File, false),
            (
                "/work/link".to_owned(),
                DirectoryListingKind::Directory,
                true
            ),
            (
                "/work/sub".to_owned(),
                DirectoryListingKind::Directory,
                false
            ),
        ]
    );
    assert_eq!(
        host.get_directories_js(js("/work"))
            .unwrap()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["/work/link", "/work/sub"]
    );
    assert_eq!(
        host.realpath_js(js("/work/link/c.ts")).unwrap(),
        Some(JsString::from("/work/sub/c.ts"))
    );
    assert_eq!(host.realpath_js(js("/work/broken")).unwrap(), None);
    assert_eq!(
        host.current_directory_js().unwrap(),
        JsString::from("/work")
    );
}

#[test]
fn the_compiler_host_lists_files_then_directories_in_the_file_systems_order() {
    // tsgo vfsmatch matches a listing's files, then its directories, in the
    // order the file system gives them (a disk's is sorted by name).
    let fs = fs_of(
        &[
            ("/work/z.ts", Seed::file("z")),
            ("/work/m/c.ts", Seed::file("c")),
            ("/work/a.ts", Seed::file("a")),
        ],
        true,
    );
    let host = VfsCompilerHost::new(&fs, "/work");
    assert_eq!(
        host.read_directory_js(JsStr::from_str("/work"))
            .unwrap()
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["/work/a.ts", "/work/z.ts", "/work/m"]
    );
}

use crate::DirectoryListingKind;

/// tsgo `osvfs` writes with Go's `os` package: a failure is Go's
/// `*fs.PathError`, and `os.MkdirAll` names the path that is not a
/// directory (tsgo 19dadef8's TS5033 texts for the same layouts).
#[cfg(unix)]
#[test]
fn os_writes_report_go_path_errors() {
    let root = std::env::temp_dir().join(format!("tsc-rs-osfs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("out.js")).unwrap();
    std::fs::write(root.join("file"), b"").unwrap();
    let root_path = std::fs::canonicalize(&root).unwrap();
    let root_path = root_path.to_str().unwrap();
    let fs = OsFs::new(true);

    let error = fs
        .write_creating_dirs(&format!("{root_path}/out.js"), b"x")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("open {root_path}/out.js: is a directory")
    );
    let error = fs
        .write_creating_dirs(&format!("{root_path}/file/sub/a.js"), b"x")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("mkdir {root_path}/file: not a directory")
    );
    assert_eq!(error.kind(), io::ErrorKind::NotADirectory);
    fs.write_creating_dirs(&format!("{root_path}/new/dir/a.js"), b"x")
        .unwrap();
    assert_eq!(fs.read(&format!("{root_path}/new/dir/a.js")).unwrap(), b"x");
    std::fs::remove_dir_all(&root).unwrap();
}
