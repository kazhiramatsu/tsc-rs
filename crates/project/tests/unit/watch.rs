//! tsgo `project/watch_test.go`, and `tspath`'s common parent directories
//! as the watches use them.

use std::collections::BTreeMap;

use tsc_lsproto::{PatternOrRelativePattern, WatchKind, WorkspaceFolderOrURI};

use super::*;

#[test]
fn path_components_for_watching_group_os_roots() {
    // `TestGetPathComponentsForWatching`.
    let cases: [(&str, &[&str]); 8] = [
        ("/project", &["/", "project"]),
        ("C:/project", &["C:/", "project"]),
        (
            "//server/share/project/tsconfig.json",
            &["//server/share", "project", "tsconfig.json"],
        ),
        ("C:/Users", &["C:/Users"]),
        ("C:/Users/andrew/project", &["C:/Users/andrew", "project"]),
        ("/home", &["/home"]),
        ("/home/andrew/project", &["/home/andrew", "project"]),
        ("/home/andrew", &["/home/andrew"]),
    ];
    for (path, expected) in cases {
        assert_eq!(path_components_for_watching(path), expected, "{path}");
    }
}

fn files(case_sensitive: bool, names: &[&str]) -> BTreeMap<String, String> {
    names
        .iter()
        .map(|name| {
            let path = if case_sensitive {
                (*name).to_owned()
            } else {
                tsc_host::to_file_name_lower_case(name)
            };
            (path, (*name).to_owned())
        })
        .collect()
}

#[test]
fn an_external_directory_keeps_its_spelling() {
    // `TestResolutionLookupWatcherPreservesDirectorySpelling`.
    let result = resolution_lookup_globs(
        "/workspace",
        "/lib",
        "/current",
        false,
        &files(false, &["/External/Dir/file.ts"]),
    );
    assert_eq!(result.directories_outside_workspace, ["/External/Dir"]);
    let watcher = new_recursive_directory_watcher(
        &result.directories_outside_workspace[0],
        WatchKind::CREATE,
        true,
    );
    let PatternOrRelativePattern::RelativePattern(relative) = watcher.glob_pattern else {
        panic!("a relative pattern");
    };
    let WorkspaceFolderOrURI::URI(uri) = relative.base_uri else {
        panic!("a URI base");
    };
    assert_eq!(uri.0, "file:///External/Dir");
}

#[test]
fn a_node_modules_directory_keeps_its_spelling() {
    // `TestResolutionLookupWatcherPreservesNodeModulesSpelling`.
    let result = resolution_lookup_globs(
        "/workspace",
        "/lib",
        "/current",
        false,
        &files(false, &["/External/Node_Modules/pkg/index.d.ts"]),
    );
    assert_eq!(
        result.patterns_inside_workspace,
        ["/External/Node_Modules/**/*"]
    );
}

#[test]
fn included_directories_keep_their_spelling() {
    // `TestResolutionLookupWatcherPreservesIncludedDirectorySpelling`.
    let result = resolution_lookup_globs(
        "/Workspace",
        "/Lib",
        "/Project",
        false,
        &files(
            false,
            &[
                "/Workspace/src/index.ts",
                "/Project/src/index.ts",
                "/Lib/lib.d.ts",
            ],
        ),
    );
    assert_eq!(
        result.patterns_inside_workspace,
        ["/Workspace/**/*", "/Project/**/*", "/Lib/**/*"]
    );
}

#[test]
fn external_directories_aggregate_by_the_hosts_case() {
    // `TestResolutionLookupWatcherAggregatesUsingHostCaseSensitivity`.
    let names = ["/External/Lib/src/a.ts", "/external/LIB/test/b.ts"];
    let sensitive =
        resolution_lookup_globs("/Workspace", "/Lib", "/Project", true, &files(true, &names));
    assert_eq!(
        sensitive.directories_outside_workspace,
        ["/External/Lib/src", "/external/LIB/test"]
    );
    let insensitive = resolution_lookup_globs(
        "/Workspace",
        "/Lib",
        "/Project",
        false,
        &files(false, &names),
    );
    assert_eq!(insensitive.directories_outside_workspace.len(), 1);
    let directory = &insensitive.directories_outside_workspace[0];
    assert!(directory == "/External/Lib" || directory == "/external/LIB");
}

#[test]
fn a_cloned_watch_keeps_its_watchers_until_they_change() {
    // tsgo `WatchedFiles.Clone` and `Watchers`: a clone whose globs are the
    // same keeps the watchers (and has no ID of its own); new globs take a
    // new ID.
    let watch = WatchedFiles::new("test", WATCH_ALL, false, |globs: &Vec<String>| {
        PatternsAndIgnored {
            patterns_inside_workspace: globs.clone(),
            ..PatternsAndIgnored::default()
        }
    });
    let first = watch.clone_with(vec!["/a/**/*".to_owned()]);
    let watchers = first.watchers();
    assert_eq!(watchers.workspace_watchers.len(), 1);
    assert!(watchers.id.starts_with("test watcher "));
    assert_ne!(watchers.id, "test watcher 0");
    let same = first.clone_with(vec!["/a/**/*".to_owned()]);
    assert_eq!(same.watchers().id, "test watcher 0");
    assert_eq!(
        same.watchers().workspace_watchers,
        watchers.workspace_watchers
    );
    let other = same.clone_with(vec!["/b/**/*".to_owned()]);
    assert_ne!(other.id(), "test watcher 0");
}

#[test]
fn registrations_are_counted_by_glob() {
    let registry = WatchRegistry::default();
    let watcher = new_recursive_directory_watcher("/project", WATCH_ALL, false);
    assert!(registry.acquire(&watcher, "a.0"));
    assert!(!registry.acquire(&watcher, "b.0"));
    assert_eq!(registry.release(&watcher), None);
    assert_eq!(registry.release(&watcher).as_deref(), Some("a.0"));
    assert_eq!(registry.release(&watcher), None);
}

/// Directories, the case sensitivity, and tsgo's parents and ignored.
type Case<'a> = (&'a [&'a str], bool, &'a [&'a str], &'a [&'a str]);

#[test]
fn common_parents_are_tsgos() {
    // tsgo `GetCommonParentDirectories` with the watches' depth (2) and
    // `getPathComponentsForWatching`, as a Go probe printed them.
    let cases: [Case<'_>; 11] = [
        (&["/a/b/c", "/a/b/d", "/e/f"], true, &["/a/b", "/e/f"], &[]),
        (&["/a"], true, &["/a"], &[]),
        (&["/"], true, &[], &["/"]),
        (
            &["/home/user/x/y", "/home/user/x/z"],
            true,
            &["/home/user/x"],
            &[],
        ),
        (&["/a/b", "/c/d", "/a/e"], true, &["/a", "/c/d"], &[]),
        (&["/a/b/c", "/a/x"], true, &["/a"], &[]),
        (
            &["/External/Lib/src", "/external/LIB/test"],
            false,
            &["/External/Lib"],
            &[],
        ),
        (
            &["/External/Lib/src", "/external/LIB/test"],
            true,
            &["/External/Lib/src", "/external/LIB/test"],
            &[],
        ),
        (&["/a", "/b"], true, &["/a", "/b"], &[]),
        (
            &["/home/andrew", "/home/bob"],
            true,
            &[],
            &["/home/andrew", "/home/bob"],
        ),
        (
            &["C:/Users/andrew/x", "C:/Users/andrew/y", "D:/z/w"],
            false,
            &["C:/Users/andrew/x", "C:/Users/andrew/y", "D:/z/w"],
            &[],
        ),
    ];
    for (directories, case_sensitive, parents, ignored) in cases {
        let directories = directories
            .iter()
            .map(|directory| (*directory).to_owned())
            .collect::<Vec<_>>();
        let (actual, actual_ignored) = common_parent_directories(&directories, 2, case_sensitive);
        assert_eq!(actual, parents, "{directories:?}");
        assert_eq!(
            actual_ignored.into_iter().collect::<Vec<_>>(),
            ignored,
            "{directories:?}"
        );
    }
}

#[test]
fn containment_follows_the_case_and_the_roots() {
    assert!(contains_path("/a/b", "/a/b", true));
    assert!(contains_path("/a/b", "/a/b/c.ts", true));
    assert!(!contains_path("/a/b", "/a/bc.ts", true));
    assert!(!contains_path("/a/B", "/a/b/c.ts", true));
    assert!(contains_path("/a/B", "/a/b/c.ts", false));
    assert!(contains_path("/", "/a/b", true));
    assert!(contains_path("C:/a", "c:/a/b", true));
}
