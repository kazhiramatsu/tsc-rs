//! tsgo `api/requestfilesystem/{requestfilesystem,pathtree,filechanges}_test.go`
//! (19dadef8). Not ported: the case that counts the language server's
//! overlay reads (a standalone session has no overlays), and Go `FileInfo`
//! details without a Rust counterpart (mode bits, the info's identity); a
//! file change names a file, not its URI.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tsc_host::vfs::{DirEntry, Entries, FileSystem, FileType, MemFs, Metadata, Seed, SystemClock};
use tsc_project::FileChangeSummary;

use super::path_tree::{compose, Entry, Fallback, PathNode, RequestDirectory, RequestFile};
use super::*;

/// tsgo `vfstest.FromMap`: files, and links (`link:<target>`).
fn host(files: &[(&str, &str)], case_sensitive: bool) -> Arc<MemFs> {
    Arc::new(
        MemFs::from_entries(
            files
                .iter()
                .map(|(name, text)| match text.strip_prefix("link:") {
                    Some(target) => (*name, Seed::symlink(target)),
                    None => (*name, Seed::file(*text)),
                }),
            case_sensitive,
            Arc::new(SystemClock),
        )
        .expect("build the host"),
    )
}

/// tsgo `trackingvfs.FS`: the paths read through it.
struct Tracking {
    inner: Arc<dyn FileSystem>,
    seen: Mutex<BTreeSet<String>>,
}

impl Tracking {
    fn new(inner: Arc<dyn FileSystem>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            seen: Mutex::default(),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn see(&self, path: &str) {
        self.lock().insert(path.to_owned());
    }

    fn has(&self, path: &str) -> bool {
        self.lock().contains(path)
    }

    fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn clear(&self) {
        self.lock().clear();
    }

    fn forget(&self, path: &str) {
        self.lock().remove(path);
    }
}

impl FileSystem for Tracking {
    fn case_sensitive(&self) -> bool {
        self.inner.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.see(path);
        self.inner.read(path)
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        self.see(path);
        self.inner.metadata(path)
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.see(path);
        self.inner.read_dir(path)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.see(path);
        self.inner.canonicalize(path)
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.inner.write(path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.inner.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.inner.create_dir_all(path)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.inner.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.inner.set_modified(path, modified)
    }

    fn is_file(&self, path: &str) -> bool {
        self.see(path);
        self.inner.is_file(path)
    }

    fn is_dir(&self, path: &str) -> bool {
        self.see(path);
        self.inner.is_dir(path)
    }

    fn accessible_entries(&self, path: &str) -> Entries {
        self.see(path);
        self.inner.accessible_entries(path)
    }
}

/// A request file system's parameters.
#[derive(Clone, Default)]
struct Request(RequestFileSystemParams);

impl Request {
    fn kind(kind: &str) -> Self {
        Self(RequestFileSystemParams {
            kind: kind.to_owned(),
            ..RequestFileSystemParams::default()
        })
    }

    fn full() -> Self {
        Self::kind("full")
    }

    fn layer() -> Self {
        Self::kind("layer")
    }

    fn file(mut self, name: &str, text: &str) -> Self {
        self.0.files.insert(name.to_owned(), text.to_owned());
        self
    }

    fn listing(mut self, name: &str, files: &[&str], directories: &[&str]) -> Self {
        self.0.directories.insert(
            name.to_owned(),
            RequestDirectoryEntries {
                files: files.iter().map(|file| (*file).to_owned()).collect(),
                directories: directories.iter().map(|dir| (*dir).to_owned()).collect(),
            },
        );
        self
    }

    fn link(mut self, name: &str, target: &str, host: bool) -> Self {
        self.0.symlinks.insert(
            name.to_owned(),
            RequestSymlinkParams {
                target: target.to_owned(),
                host,
            },
        );
        self
    }

    fn removed(mut self, path: &str) -> Self {
        self.0.removed_paths.push(path.to_owned());
        self
    }
}

fn on_host(fs: Arc<impl FileSystem + 'static>) -> SnapshotFileSystem {
    SnapshotFileSystem::Host(fs as Arc<dyn FileSystem>)
}

fn on(fs: &Arc<RequestFileSystem>) -> SnapshotFileSystem {
    SnapshotFileSystem::Request(Arc::clone(fs))
}

/// tsgo `newRequestFileSystem` and `newLayeredRequestFileSystem`: both are
/// `NewForUpdate`.
fn try_over(
    request: &Request,
    base: &SnapshotFileSystem,
    current_directory: &str,
) -> Result<Arc<RequestFileSystem>, String> {
    RequestFileSystem::new_for_update(
        &request.0,
        base,
        current_directory,
        &mut FileChangeSummary::default(),
    )
}

fn over(request: &Request, base: &SnapshotFileSystem) -> Arc<RequestFileSystem> {
    try_over(request, base, "/").expect("build the request file system")
}

fn read(fs: &dyn FileSystem, path: &str) -> Option<String> {
    fs.read(path)
        .ok()
        .map(|bytes| String::from_utf8(bytes).expect("the test files are UTF-8"))
}

fn files(fs: &dyn FileSystem, path: &str) -> Vec<String> {
    fs.accessible_entries(path).files
}

fn directories(fs: &dyn FileSystem, path: &str) -> Vec<String> {
    fs.accessible_entries(path).directories
}

fn same_base(fs: &RequestFileSystem, base: &Arc<impl FileSystem + 'static>) -> bool {
    Arc::as_ptr(fs.base_file_system()).cast::<()>() == Arc::as_ptr(base).cast::<()>()
}

/// What a path looks like through a file system, for comparisons.
fn observe(fs: &dyn FileSystem, operation: &str, path: &str) -> String {
    match operation {
        "is_file" => fs.is_file(path).to_string(),
        "is_dir" => fs.is_dir(path).to_string(),
        "read" => format!("{:?}", read(fs, path)),
        "canonicalize" => format!("{:?}", fs.canonicalize(path).ok()),
        "accessible_entries" => format!("{:?}", fs.accessible_entries(path)),
        "metadata" => format!(
            "{:?}",
            fs.metadata(path).ok().map(|metadata| (
                metadata.file_type(),
                metadata.len(),
                metadata.modified()
            ))
        ),
        _ => unreachable!("{operation}"),
    }
}

/// tsgo `verifyCompactionWithoutHostReads`: a layer composed over `layer`
/// answers as it does for every path `layer` answers without the host.
fn verify_compaction_without_host_reads(
    layer: &Arc<RequestFileSystem>,
    tracking: &Tracking,
    paths: &[&str],
) {
    let compacted = over(&Request::layer(), &on(layer));
    for operation in [
        "is_file",
        "is_dir",
        "read",
        "canonicalize",
        "accessible_entries",
        "metadata",
    ] {
        for path in paths {
            tracking.clear();
            let expected = observe(layer.as_ref(), operation, path);
            if !tracking.is_empty() {
                continue;
            }
            let actual = observe(compacted.as_ref(), operation, path);
            assert!(tracking.is_empty(), "{operation}({path}) read the host");
            assert_eq!(actual, expected, "{operation}({path}) after compaction");
        }
    }
}

#[test]
fn layers_compact_a_request_file_system_base_eagerly() {
    // TestInitializeForUpdate: filesystem layers eagerly compact a request
    // filesystem base.
    let host_fs = host(&[], true);
    let base = over(
        &Request::full().file("/base.ts", "base"),
        &on_host(Arc::clone(&host_fs)),
    );
    let layered = over(&Request::layer().file("/layered.ts", "layered"), &on(&base));
    assert!(same_base(&layered, &host_fs));
    assert!(layered.is_full());
    assert!(layered.is_file("/base.ts"));
    assert!(layered.is_file("/layered.ts"));
}

#[test]
fn a_layer_over_the_host_reads_its_listing_without_the_host() {
    // TestInitializeForUpdate: filesystem layers over a host-backed snapshot.
    let tracking = Tracking::new(host(&[("/dir/host.ts", "host")], true));
    let fs = over(
        &Request::layer()
            .file("/dir/cached.ts", "cached")
            .listing("/dir", &["cached.ts"], &[]),
        &on_host(Arc::clone(&tracking)),
    );
    // Change generation may inspect the old directory; reading the
    // supplied complete listing itself must not fall back to the host.
    tracking.forget("/dir");
    assert_eq!(files(fs.as_ref(), "/dir"), ["cached.ts"]);
    assert!(!tracking.has("/dir"));
}

#[test]
fn a_full_file_system_starts_a_new_chain() {
    // TestInitializeForUpdate: memory starts a new chain.
    let host_fs = host(&[("/host.ts", "host")], true);
    let base = over(
        &Request::full().file("/base.ts", "base"),
        &on_host(Arc::clone(&host_fs)),
    );
    let fs = over(
        &Request::full().file("/replacement.ts", "replacement"),
        &on(&base),
    );
    assert!(same_base(&fs, &host_fs));
    assert!(!fs.is_file("/base.ts"));
}

/// tsgo `testCompleteDirectoryListing`.
fn complete_directory_listing(kind: &str, explicit: bool, replacement: (&[&str], &[&str])) {
    let tracking = Tracking::new(host(
        &[
            ("/dir/host.ts", "host"),
            ("/dir/host-dir/index.ts", "host child"),
        ],
        true,
    ));
    let mut request = Request::kind(kind)
        .file("/dir/base.ts", "base")
        .file("/dir/base-dir/index.ts", "base child");
    if explicit {
        request = request.listing("/dir", &["base.ts"], &["base-dir"]);
    }
    let base = over(&request, &on_host(Arc::clone(&tracking)));
    let layered = over(
        &Request::layer().listing("/dir", replacement.0, replacement.1),
        &on(&base),
    );
    // Omitting a listing in a later update still merges its derived entries
    // with the complete listing, without reopening host fallback.
    let next = over(
        &Request::layer()
            .file("/dir/added.ts", "added")
            .file("/dir/added-dir/index.ts", "added child"),
        &on(&layered),
    );
    tracking.clear();
    let verify = || {
        let entries = layered.accessible_entries("/dir");
        assert!(tracking.is_empty());
        assert_eq!(entries.files, replacement.0);
        assert_eq!(entries.directories, replacement.1);
        let entries = next.accessible_entries("/dir");
        assert!(tracking.is_empty());
        let mut expected_files = vec!["added.ts"];
        expected_files.extend(replacement.0);
        let mut expected_directories = vec!["added-dir"];
        expected_directories.extend(replacement.1);
        assert_eq!(entries.files, expected_files);
        assert_eq!(entries.directories, expected_directories);
    };
    verify();
    assert!(same_base(&layered, &tracking));
    assert!(same_base(&next, &tracking));
    verify();
}

#[test]
fn complete_directory_listings_replace_what_was_listed() {
    // TestRequestFileSystemCompleteDirectoryListings{Full,Layer}{Explicit,Derived}{Replacement,Empty}.
    for kind in ["full", "layer"] {
        for explicit in [true, false] {
            complete_directory_listing(kind, explicit, (&["replacement.ts"], &["replacement-dir"]));
            complete_directory_listing(kind, explicit, (&[], &[]));
        }
    }
}

#[test]
fn an_explicit_listing_keeps_its_order() {
    // TestRequestFileSystemPreservesExplicitDirectoryOrder.
    let fs = over(
        &Request::full()
            .file("/src/index.ts", "")
            .file("/src/foo.ts", "")
            .listing("/src", &["index.ts", "foo.ts"], &[]),
        &on_host(host(&[], true)),
    );
    assert_eq!(files(fs.as_ref(), "/src"), ["index.ts", "foo.ts"]);
}

#[test]
fn derived_listings_follow_the_hosts_case_sensitivity() {
    // TestRequestFileSystemDerivesDirectoryListingsWithHostCaseSensitivity.
    let request = Request::full()
        .file("C:/Repo/upper.ts", "upper")
        .file("c:/repo/lower.ts", "lower");
    let fs = try_over(&request, &on_host(host(&[], false)), "C:/Workspace").unwrap();
    assert_eq!(files(fs.as_ref(), "C:/REPO"), ["lower.ts", "upper.ts"]);
    assert_eq!(directories(fs.as_ref(), "C:/"), ["Repo", "Workspace"]);
    let fs = try_over(&request, &on_host(host(&[], true)), "C:/Workspace").unwrap();
    assert_eq!(files(fs.as_ref(), "C:/Repo"), ["upper.ts"]);
    assert_eq!(files(fs.as_ref(), "c:/repo"), ["lower.ts"]);
}

/// tsgo `symlinkReplacementOptions`.
struct SymlinkReplacement {
    kind: &'static str,
    host_target: bool,
    case_sensitive: bool,
    remove: bool,
    link_path: &'static str,
}

/// tsgo `testSymlinkReplacesDirectory`.
fn symlink_replaces_directory(options: &SymlinkReplacement) {
    let link_path = options.link_path;
    let remove = options.remove;
    let host_fs = host(
        &[
            ("/dir/removed/old.ts", "old host"),
            ("/dir/removed/child/old.ts", "old host child"),
            ("/dir/removed/sibling.ts", "old sibling"),
            ("/dir/removed-other/old.ts", "unrelated"),
            ("/target/new.ts", "host target"),
            ("/target/removed/new.ts", "host target"),
        ],
        options.case_sensitive,
    );
    let mut request = Request::kind(options.kind)
        .file("/dir/removed/cached.ts", "cached")
        .file("/dir/removed/child/cached.ts", "cached child")
        .file("/dir/removed-other/old.ts", "unrelated")
        .listing("/dir", &[], &["removed", "removed-other"])
        .listing("/dir/removed", &["cached.ts"], &["child"])
        .listing("/dir/removed/child", &["cached.ts"], &[]);
    let mut expected = "host target";
    if options.kind == "full" {
        request = request
            .file("/target/new.ts", "request target")
            .file("/target/removed/new.ts", "request target");
        if !options.host_target {
            expected = "request target";
        }
    }
    let base = over(&request, &on_host(Arc::clone(&host_fs)));
    let mut previous = Arc::clone(&base);
    if remove {
        let removed_path = if options.case_sensitive {
            "/dir/removed".to_owned()
        } else {
            "/dir/removed".to_uppercase()
        };
        previous = over(&Request::layer().removed(&removed_path), &on(&base));
    }
    let verify_previous = || {
        assert_eq!(previous.is_dir("/dir/removed"), !remove);
        assert_eq!(previous.is_file("/dir/removed/cached.ts"), !remove);
        if remove {
            assert!(!previous.is_file("/dir/removed/old.ts"));
            assert!(files(previous.as_ref(), "/dir/removed").is_empty());
            assert!(directories(previous.as_ref(), "/dir/removed").is_empty());
        } else {
            assert_eq!(files(previous.as_ref(), "/dir/removed"), ["cached.ts"]);
            assert_eq!(directories(previous.as_ref(), "/dir/removed"), ["child"]);
        }
    };
    verify_previous();
    let linked = over(
        &Request::layer().link(link_path, "/target", options.host_target),
        &on(&previous),
    );
    let verify_linked = |fs: &Arc<RequestFileSystem>| {
        assert!(same_base(fs, &host_fs));
        assert_eq!(fs.is_full(), options.kind == "full");
        assert!(fs.is_dir(link_path));
        assert!(!fs.is_file(link_path));
        for suffix in ["/new.ts", "/removed/new.ts"] {
            let file_name = format!("{link_path}{suffix}");
            assert!(fs.is_file(&file_name), "{file_name}");
            assert_eq!(read(fs.as_ref(), &file_name).as_deref(), Some(expected));
            let metadata = fs.metadata(&file_name).unwrap();
            assert!(metadata.is_file());
            assert_eq!(metadata.len(), expected.len() as u64);
            assert_eq!(
                fs.canonicalize(&file_name).unwrap(),
                format!("/target{suffix}")
            );
        }
        assert!(fs.metadata(link_path).unwrap().is_dir());
        assert_eq!(fs.canonicalize(link_path).unwrap(), "/target");
        assert_eq!(files(fs.as_ref(), link_path), ["new.ts"]);
        assert_eq!(directories(fs.as_ref(), link_path), ["removed"]);
        let parent = fs.accessible_entries(&paths::directory_path(link_path));
        let link_name = paths::base_file_name(link_path);
        assert!(parent.directories.contains(&link_name));
        assert!(parent.symlinks.contains(&link_name));
        assert!(!fs.is_file(&format!("{link_path}/old.ts")));
        assert!(!fs.is_file(&format!("{link_path}/cached.ts")));
        if link_path != "/dir" {
            assert!(fs.is_file("/dir/removed-other/old.ts"));
        }
        if remove {
            assert!(!fs.is_file("/dir/removed/sibling.ts"));
        }
        assert_eq!(
            files(fs.as_ref(), &format!("{link_path}/removed")),
            ["new.ts"]
        );
    };
    verify_linked(&linked);
    let next = over(&Request::layer(), &on(&linked));
    verify_linked(&next);
    let deleted = over(
        &Request::layer().removed(&format!("{link_path}/new.ts")),
        &on(&next),
    );
    assert!(!deleted.is_file(&format!("{link_path}/new.ts")));
    assert!(deleted.is_file(&format!("{link_path}/removed/new.ts")));
    assert!(files(deleted.as_ref(), link_path).is_empty());
    verify_linked(&linked);
    assert!(base.is_file("/dir/removed/cached.ts"));
    verify_previous();
}

#[test]
fn a_symlink_replaces_a_directory() {
    // TestRequestFileSystemSymlinkReplacesDirectory*.
    let cases = [
        ("full", false, true),
        ("full", true, true),
        ("layer", false, true),
        ("layer", true, false),
    ];
    for (kind, host_target, case_sensitive) in cases {
        for remove in [false, true] {
            for link_path in ["/dir/removed", "/dir", "/dir/removed/child"] {
                symlink_replaces_directory(&SymlinkReplacement {
                    kind,
                    host_target,
                    case_sensitive,
                    remove,
                    link_path,
                });
            }
        }
    }
}

/// tsgo `testObjectOverridesTombstone`.
fn object_overrides_tombstone(object: &str, inherited_link: bool, path: &str) {
    let tracking = Tracking::new(host(
        &[
            ("/dir/removed/old.ts", "old"),
            ("/dir/removed/child.ts", "old child"),
            ("/old/removed/old.ts", "old target"),
            ("/target/file.ts", "target"),
        ],
        true,
    ));
    let mut base_request = Request::layer();
    if inherited_link {
        base_request = base_request.link("/dir", "/old", false);
    }
    let base = over(&base_request, &on_host(Arc::clone(&tracking)));
    let removed = over(&Request::layer().removed("/dir/removed"), &on(&base));
    let request = match object {
        "file" => Request::layer().file(path, "new"),
        "directory" => Request::layer().listing(path, &[], &[]),
        "symlink" => Request::layer().link(path, "/target", false),
        "file-symlink" => Request::layer().link(path, "/target/file.ts", false),
        _ => unreachable!("{object}"),
    };
    let replaced = over(&request, &on(&removed));
    let is_file = object == "file" || object == "file-symlink";
    assert_eq!(replaced.is_file(path), is_file, "{object} {path}");
    assert_eq!(replaced.is_dir(path), !is_file, "{object} {path}");
    assert!(replaced.metadata(path).is_ok());
    let entries = replaced.accessible_entries(&paths::directory_path(path));
    let entry_name = paths::base_file_name(path);
    assert_eq!(entries.files.contains(&entry_name), is_file);
    assert_eq!(entries.directories.contains(&entry_name), !is_file);
    assert_eq!(
        entries.symlinks.contains(&entry_name),
        object == "symlink" || object == "file-symlink"
    );
    assert!(!replaced.is_file("/dir/removed/old.ts"));
    assert_eq!(read(replaced.as_ref(), "/dir/removed/old.ts"), None);
    assert!(replaced.metadata("/dir/removed/old.ts").is_err());
    if is_file {
        assert!(files(replaced.as_ref(), &format!("{path}/removed")).is_empty());
        assert!(directories(replaced.as_ref(), path).is_empty());
    }
    assert!(!removed.is_dir("/dir/removed"));
    let file = format!("{path}/file.ts");
    verify_compaction_without_host_reads(
        &replaced,
        &tracking,
        &[
            path,
            &file,
            "/dir",
            "/dir/removed",
            "/dir/removed/old.ts",
            "/target/file.ts",
        ],
    );
}

#[test]
fn a_new_object_overrides_a_tombstone() {
    // TestRequestFileSystem{File,Directory,Symlink,FileSymlink}Overrides{Parent,Same,Child}Tombstone[InheritedLink].
    for object in ["file", "directory", "symlink", "file-symlink"] {
        for inherited_link in [false, true] {
            for path in ["/dir", "/dir/removed", "/dir/removed/child"] {
                object_overrides_tombstone(object, inherited_link, path);
            }
        }
    }
}

/// tsgo `testReplacementPreservesCurrentRemoval`.
fn replacement_preserves_current_removal(kind: &str, removed_path: &str) {
    let host_files = [
        ("/dir/old.ts", "old host"),
        ("/target/keep.ts", "keep"),
        ("/target/blocked/gone.ts", "gone"),
    ];
    let host_fs = host(&host_files, true);
    let mut request = Request::kind(kind);
    if kind == "full" {
        for (name, text) in host_files {
            request = request.file(name, text);
        }
    }
    let base = over(&request, &on_host(host_fs));
    let removed = over(
        &Request::layer().removed("/dir").removed("/dir/blocked"),
        &on(&base),
    );
    let linked = over(
        &Request::layer()
            .link("/dir", "/target", kind == "layer")
            .removed(removed_path),
        &on(&removed),
    );
    assert!(linked.is_file("/dir/keep.ts"));
    assert!(!linked.is_file("/dir/old.ts"));
    assert!(!linked.is_file("/dir/blocked/gone.ts"));
    assert_eq!(read(linked.as_ref(), "/dir/blocked/gone.ts"), None);
    assert!(linked.metadata("/dir/blocked/gone.ts").is_err());
    assert!(files(linked.as_ref(), "/dir/blocked").is_empty());
    let deleted = over(&Request::layer().removed("/dir"), &on(&linked));
    assert!(!deleted.is_dir("/dir"));
    assert!(!deleted.is_file("/dir/keep.ts"));
    assert!(files(deleted.as_ref(), "/dir").is_empty());
    assert!(deleted.is_file("/target/keep.ts"));
    assert!(linked.is_file("/dir/keep.ts"));
}

#[test]
fn a_replacement_keeps_the_current_removals() {
    // TestRequestFileSystemReplacementPreservesCurrent{Full,Layer}{Alias,Target}{Directory,File}Removal.
    for kind in ["full", "layer"] {
        for removed_path in [
            "/dir/blocked",
            "/dir/blocked/gone.ts",
            "/target/blocked",
            "/target/blocked/gone.ts",
        ] {
            replacement_preserves_current_removal(kind, removed_path);
        }
    }
}

/// tsgo `testSameLayerRemoval`.
fn same_layer_removal(host_target: bool, removed_path: &str, form: &str) {
    let tracking = Tracking::new(host(&[("/target/file.ts", "host")], true));
    let base = over(&Request::full(), &on_host(Arc::clone(&tracking)));
    let request = Request::layer()
        .file("/target/file.ts", "request")
        .link("/links/pkg", "/target", host_target)
        .removed(removed_path);
    let fs = match form {
        "standalone" => over(&request, &on_host(Arc::clone(&tracking))),
        "layered" => over(&request, &on(&base)),
        _ => {
            let fs = over(&request, &on(&base));
            match form {
                "recompacted" => over(&Request::layer(), &on(&fs)),
                "compacted-input" => over(&request, &on(&fs)),
                _ => fs,
            }
        }
    };
    assert!(!fs.is_file("/links/pkg/file.ts"), "{removed_path} {form}");
    assert_eq!(read(fs.as_ref(), "/links/pkg/file.ts"), None);
    assert!(fs.metadata("/links/pkg/file.ts").is_err());
    assert_eq!(
        fs.canonicalize("/links/pkg/file.ts").unwrap(),
        "/links/pkg/file.ts"
    );
    assert!(files(fs.as_ref(), "/links/pkg").is_empty());
    let link_exists = removed_path == "/links/pkg/file.ts";
    assert_eq!(
        fs.is_dir("/links/pkg"),
        link_exists,
        "{removed_path} {form}"
    );
    let entries = fs.accessible_entries("/links");
    assert_eq!(entries.directories.contains(&"pkg".to_owned()), link_exists);
    assert_eq!(entries.symlinks.contains("pkg"), link_exists);
    assert!(fs.is_file("/target/file.ts"));
    verify_compaction_without_host_reads(
        &fs,
        &tracking,
        &[
            "/",
            "/links",
            "/links/pkg",
            "/links/pkg/file.ts",
            "/target",
            "/target/file.ts",
            "/missing",
        ],
    );
}

#[test]
fn a_removal_in_the_same_layer_blocks_its_links() {
    // TestRequestFileSystemSameLayerRemoval{Request,Host}{Ancestor,Link,Descendant}{Standalone,Layered,Compacted,Recompacted,CompactedInput}.
    for host_target in [false, true] {
        for removed_path in ["/links", "/links/pkg", "/links/pkg/file.ts"] {
            for form in [
                "standalone",
                "layered",
                "compacted",
                "recompacted",
                "compacted-input",
            ] {
                same_layer_removal(host_target, removed_path, form);
            }
        }
    }
}

/// tsgo `testRemovalExceptions`.
fn removal_exceptions(host_target: bool, remove_again: bool) {
    let host_fs = host(
        &[
            ("/dir/old.ts", "old"),
            ("/target/a.ts", "a"),
            ("/target/b.ts", "b"),
            ("/target/sub/c.ts", "c"),
        ],
        true,
    );
    let base = over(&Request::layer(), &on_host(host_fs));
    let removed = over(&Request::layer().removed("/dir"), &on(&base));
    let request = Request::layer()
        .link("/dir/pkg", "/target", host_target)
        .removed("/dir/pkg/b.ts");
    let layered = over(&request, &on(&removed));
    let compacted = over(&Request::layer(), &on(&layered));
    let input = over(&Request::layer(), &on(&compacted));
    let verify = |fs: &Arc<RequestFileSystem>| {
        let fs = if remove_again {
            over(&Request::layer().removed("/dir"), &on(fs))
        } else {
            Arc::clone(fs)
        };
        assert_eq!(fs.is_file("/dir/pkg/a.ts"), !remove_again);
        assert!(!fs.is_file("/dir/pkg/b.ts"));
        assert!(!fs.is_file("/dir/old.ts"));
        let entries = fs.accessible_entries("/dir/pkg");
        if remove_again {
            assert!(entries.files.is_empty());
            assert!(entries.directories.is_empty());
        } else {
            assert_eq!(entries.files, ["a.ts"]);
            assert_eq!(entries.directories, ["sub"]);
        }
    };
    verify(&layered);
    verify(&compacted);
    verify(&input);
    assert!(!removed.is_dir("/dir/pkg"));
    assert!(compacted.is_file("/dir/pkg/a.ts"));
}

#[test]
fn a_link_under_a_removed_directory_is_an_exception() {
    // TestRequestFileSystemRemovalExceptions{Request,Host}[RemovedAgain].
    for host_target in [false, true] {
        for remove_again in [false, true] {
            removal_exceptions(host_target, remove_again);
        }
    }
}

#[test]
fn compaction_keeps_the_host_fallback() {
    // TestRequestFileSystem: compaction preserves host fallback.
    let host_fs = host(&[("/host.ts", "host")], true);
    let base = over(&Request::layer(), &on_host(Arc::clone(&host_fs)));
    assert!(same_base(&base, &host_fs));
    assert!(!base.is_file("/created-after-base.ts"));
    host_fs.write("/created-after-base.ts", b"created").unwrap();
    let layered = over(&Request::layer().file("/layered.ts", "layered"), &on(&base));
    assert!(same_base(&layered, &host_fs));
    assert!(layered.is_file("/created-after-base.ts"));
    host_fs.remove("/created-after-base.ts").unwrap();
    assert!(!layered.is_file("/created-after-base.ts"));
    assert_eq!(read(layered.as_ref(), "/host.ts").as_deref(), Some("host"));
}

#[test]
fn a_full_file_system_is_total() {
    // TestRequestFileSystem: memory is total and never falls back.
    let tracking = Tracking::new(host(&[("/host.ts", "host")], true));
    let fs = over(
        &Request::full().file("/src/index.ts", "memory"),
        &on_host(Arc::clone(&tracking)),
    );
    assert_eq!(
        read(fs.as_ref(), "/src/index.ts").as_deref(),
        Some("memory")
    );
    assert!(fs.is_file("/src/index.ts"));
    assert!(fs.is_dir("/src"));
    assert_eq!(files(fs.as_ref(), "/src"), ["index.ts"]);
    assert_eq!(read(fs.as_ref(), "/host.ts"), None);
    assert!(!fs.is_file("/host.ts"));
    assert!(!tracking.has("/host.ts"));
}

#[test]
fn a_layer_answers_its_own_files_and_the_host_the_rest() {
    // TestRequestFileSystem: cache hits bypass the host and misses fall back.
    let tracking = Tracking::new(host(&[("/fallback.ts", "fallback")], true));
    let fs = over(
        &Request::layer()
            .file("/cached/index.ts", "cached")
            .listing("/cached", &["index.ts"], &[]),
        &on_host(Arc::clone(&tracking)),
    );
    tracking.clear();
    assert_eq!(
        read(fs.as_ref(), "/cached/index.ts").as_deref(),
        Some("cached")
    );
    assert!(fs.is_file("/cached/index.ts"));
    assert!(fs.is_dir("/cached"));
    assert_eq!(files(fs.as_ref(), "/cached"), ["index.ts"]);
    assert!(!tracking.has("/cached/index.ts"));
    assert!(!tracking.has("/cached"));
    assert_eq!(
        read(fs.as_ref(), "/fallback.ts").as_deref(),
        Some("fallback")
    );
    assert!(tracking.has("/fallback.ts"));
}

#[test]
fn a_full_file_system_as_a_layer_still_replaces_everything() {
    // TestRequestFileSystem: layered memory is a total replacement.
    let fs = over(
        &Request::full().file("/memory.ts", "memory"),
        &on_host(host(&[("/host.ts", "host")], true)),
    );
    assert_eq!(read(fs.as_ref(), "/memory.ts").as_deref(), Some("memory"));
    assert_eq!(read(fs.as_ref(), "/host.ts"), None);
}

#[test]
fn a_full_file_system_resolves_its_links() {
    // TestRequestFileSystem: memory resolves internal file and directory
    // symlinks.
    let tracking = Tracking::new(host(&[("/host.ts", "host")], true));
    let fs = over(
        &Request::full()
            .file(
                "/packages/pkg/index.d.ts",
                "export declare const value: number;",
            )
            .link("/project/node_modules/pkg", "../../../packages/pkg", false)
            .link("/project/pkg.d.ts", "../packages/pkg/index.d.ts", false),
        &on_host(Arc::clone(&tracking)),
    );
    let declaration = Some("export declare const value: number;");
    assert_eq!(
        read(fs.as_ref(), "/project/node_modules/pkg/index.d.ts").as_deref(),
        declaration
    );
    assert_eq!(
        read(fs.as_ref(), "/project/pkg.d.ts").as_deref(),
        declaration
    );
    assert_eq!(
        fs.canonicalize("/project/node_modules/pkg/index.d.ts")
            .unwrap(),
        "/packages/pkg/index.d.ts"
    );
    let entries = fs.accessible_entries("/project/node_modules");
    assert_eq!(entries.directories, ["pkg"]);
    assert!(entries.symlinks.contains("pkg"));
    let entries = fs.accessible_entries("/project");
    assert_eq!(entries.files, ["pkg.d.ts"]);
    assert!(entries.symlinks.contains("pkg.d.ts"));
    assert!(tracking.is_empty());
}

#[test]
fn a_layer_resolves_its_links_before_the_host() {
    // TestRequestFileSystem: cache resolves internal symlinks before the
    // host.
    let tracking = Tracking::new(host(&[("/packages/pkg/index.d.ts", "host content")], true));
    let fs = over(
        &Request::layer()
            .file("/packages/pkg/index.d.ts", "cached content")
            .listing("/project/node_modules", &[], &[])
            .link("/project/node_modules/pkg", "/packages/pkg", false),
        &on_host(Arc::clone(&tracking)),
    );
    tracking.clear();
    assert_eq!(
        read(fs.as_ref(), "/project/node_modules/pkg/index.d.ts").as_deref(),
        Some("cached content")
    );
    assert_eq!(
        fs.canonicalize("/project/node_modules/pkg/index.d.ts")
            .unwrap(),
        "/packages/pkg/index.d.ts"
    );
    let entries = fs.accessible_entries("/project/node_modules");
    assert_eq!(entries.directories, ["pkg"]);
    assert!(entries.symlinks.contains("pkg"));
    assert!(tracking.is_empty());
}

#[test]
fn a_layer_file_shadows_a_host_links_real_path() {
    // TestRequestFileSystem: cache file shadows underlying symlink realpath.
    let base = host(
        &[
            ("/project/node_modules/pkg", "link:/host/pkg"),
            ("/host/pkg/index.d.ts", "host content"),
        ],
        true,
    );
    let fs = over(
        &Request::layer().file("/project/node_modules/pkg/index.d.ts", "cached content"),
        &on_host(base),
    );
    assert_eq!(
        read(fs.as_ref(), "/project/node_modules/pkg/index.d.ts").as_deref(),
        Some("cached content")
    );
    assert_eq!(
        fs.canonicalize("/project/node_modules/pkg/index.d.ts")
            .unwrap(),
        "/project/node_modules/pkg/index.d.ts"
    );
}

#[test]
fn a_layer_adds_changes_and_blocks_removed_entries() {
    // TestRequestFileSystem: layered cache adds changes and blocks removed
    // entries.
    let base = over(
        &Request::full()
            .file("/keep.ts", "keep")
            .file("/change.ts", "old")
            .file("/remove.ts", "remove")
            .file("/removed-dir/gone.ts", "gone")
            .file("/becomes-file/child.ts", "child")
            .file("/becomes-directory.ts", "file"),
        &on_host(host(&[], true)),
    );
    let layered = over(
        &Request::layer()
            .file("/change.ts", "new")
            .file("/added.ts", "added")
            .file("/remove.ts", "replacement")
            .file("/removed-dir/replacement.ts", "replacement")
            .file("/becomes-file", "file")
            .file("/becomes-directory.ts/child.ts", "child")
            .listing(
                "/",
                &["added.ts", "becomes-file", "change.ts", "remove.ts"],
                &["becomes-directory.ts", "removed-dir"],
            )
            .removed("/remove.ts")
            .removed("/removed-dir"),
        &on(&base),
    );
    for (path, expected) in [
        ("/keep.ts", "keep"),
        ("/change.ts", "new"),
        ("/added.ts", "added"),
        ("/remove.ts", "replacement"),
        ("/removed-dir/replacement.ts", "replacement"),
        ("/becomes-file", "file"),
        ("/becomes-directory.ts/child.ts", "child"),
    ] {
        assert_eq!(
            read(layered.as_ref(), path).as_deref(),
            Some(expected),
            "{path}"
        );
    }
    assert!(layered.is_file("/remove.ts"));
    assert!(layered.is_dir("/removed-dir"));
    assert!(!layered.is_file("/removed-dir/gone.ts"));
    assert!(layered.metadata("/remove.ts").is_ok());
    assert!(layered.metadata("/removed-dir/replacement.ts").is_ok());
    assert_eq!(
        layered.canonicalize("/removed-dir/replacement.ts").unwrap(),
        "/removed-dir/replacement.ts"
    );
    assert!(layered.is_file("/becomes-file"));
    assert!(!layered.is_dir("/becomes-file"));
    assert!(!layered.is_file("/becomes-directory.ts"));
    assert!(layered.is_dir("/becomes-directory.ts"));
    assert_eq!(
        files(layered.as_ref(), "/"),
        ["added.ts", "becomes-file", "change.ts", "remove.ts"]
    );
    assert_eq!(
        directories(layered.as_ref(), "/"),
        ["becomes-directory.ts", "removed-dir"]
    );
}

#[test]
fn a_new_layer_overrides_the_targets_of_inherited_links() {
    // TestRequestFileSystem: new layers override targets of inherited
    // symlinks.
    let base = over(
        &Request::full()
            .file("/target/change.ts", "old")
            .file("/target/keep.ts", "keep")
            .file("/target/remove.ts", "remove")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let layered = over(
        &Request::layer()
            .file("/target/change.ts", "new")
            .file("/target/added.ts", "added")
            .removed("/target/remove.ts"),
        &on(&base),
    );
    assert_eq!(
        read(layered.as_ref(), "/link/change.ts").as_deref(),
        Some("new")
    );
    assert_eq!(
        read(layered.as_ref(), "/link/added.ts").as_deref(),
        Some("added")
    );
    assert_eq!(read(layered.as_ref(), "/link/remove.ts"), None);
    assert_eq!(
        files(layered.as_ref(), "/link"),
        ["added.ts", "change.ts", "keep.ts"]
    );
}

#[test]
fn an_alias_tombstone_wins_over_an_inherited_links_target() {
    // TestRequestFileSystem: alias tombstones take precedence over inherited
    // symlink targets.
    let base = over(
        &Request::full()
            .file("/target/file.ts", "old")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let layered = over(
        &Request::layer()
            .file("/target/file.ts", "new")
            .removed("/link"),
        &on(&base),
    );
    assert_eq!(read(layered.as_ref(), "/link/file.ts"), None);
    assert!(!layered.is_file("/link/file.ts"));
    assert!(!layered.is_dir("/link"));
    assert!(layered.metadata("/link/file.ts").is_err());
    assert!(files(layered.as_ref(), "/link").is_empty());
}

#[test]
fn an_alias_tombstone_wins_over_a_same_layer_links_target() {
    // TestRequestFileSystem: alias tombstones take precedence over
    // same-layer symlink targets.
    let fs = over(
        &Request::full()
            .file("/target/file.ts", "memory")
            .link("/link", "/target", false)
            .link("/host-link", "/host-target", true)
            .removed("/link/file.ts")
            .removed("/host-link/file.ts"),
        &on_host(host(&[("/host-target/file.ts", "host")], true)),
    );
    for path in ["/link/file.ts", "/host-link/file.ts"] {
        assert_eq!(read(fs.as_ref(), path), None, "{path}");
        assert!(!fs.is_file(path), "{path}");
        assert!(fs.metadata(path).is_err(), "{path}");
    }
    assert!(files(fs.as_ref(), "/link").is_empty());
    assert!(files(fs.as_ref(), "/host-link").is_empty());
}

#[test]
fn compaction_keeps_a_removal_through_an_inherited_link() {
    // TestRequestFileSystem: compaction preserves overlays addressed through
    // inherited symlinks.
    let base = over(
        &Request::full()
            .file("/target/remove.ts", "remove")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let layered = over(&Request::layer().removed("/link/remove.ts"), &on(&base));
    assert_eq!(read(layered.as_ref(), "/link/remove.ts"), None);
}

#[test]
fn compaction_removes_tombstones_from_explicit_listings() {
    // TestRequestFileSystem: compaction removes tombstones from explicit
    // listings.
    let base = over(
        &Request::full()
            .file("/dir/remove.ts", "remove")
            .listing("/dir", &["remove.ts"], &[]),
        &on_host(host(&[], true)),
    );
    let layered = over(&Request::layer().removed("/dir/remove.ts"), &on(&base));
    assert!(files(layered.as_ref(), "/dir").is_empty());
}

#[test]
fn a_path_removed_through_an_inherited_link_can_be_created_again() {
    // TestRequestFileSystem: compaction allows recreating a path removed
    // through an inherited symlink.
    let base = over(
        &Request::full()
            .file("/target/recreated.ts", "base")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let removed = over(&Request::layer().removed("/link/recreated.ts"), &on(&base));
    let recreated = over(
        &Request::layer().file("/link/recreated.ts", "recreated"),
        &on(&removed),
    );
    assert_eq!(
        read(recreated.as_ref(), "/link/recreated.ts").as_deref(),
        Some("recreated")
    );
}

#[test]
fn a_descendant_of_a_path_removed_through_an_inherited_link_can_be_created() {
    // TestRequestFileSystem: compaction allows recreating a descendant of a
    // path removed through an inherited symlink.
    let base = over(
        &Request::full()
            .file("/target/dir/existing.ts", "existing")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let removed = over(&Request::layer().removed("/link/dir"), &on(&base));
    let recreated = over(
        &Request::layer().file("/link/dir/recreated.ts", "recreated"),
        &on(&removed),
    );
    assert_eq!(
        read(recreated.as_ref(), "/link/dir/recreated.ts").as_deref(),
        Some("recreated")
    );
    assert!(!recreated.is_file("/link/dir/existing.ts"));
    assert_eq!(files(recreated.as_ref(), "/link/dir"), ["recreated.ts"]);
    assert_eq!(directories(recreated.as_ref(), "/link"), ["dir"]);
}

#[test]
fn a_file_replacing_a_link_targets_directory_lists_nothing() {
    // TestRequestFileSystem: files replacing inherited symlink target
    // directories have empty listings.
    let base = over(
        &Request::full()
            .file("/target/item/child.ts", "child")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let layered = over(&Request::layer().file("/target/item", "file"), &on(&base));
    assert!(layered.is_file("/link/item"));
    assert!(!layered.is_dir("/link/item"));
    assert!(files(layered.as_ref(), "/link/item").is_empty());
    assert!(directories(layered.as_ref(), "/link/item").is_empty());
}

#[test]
fn a_layer_tombstone_blocks_the_host() {
    // TestRequestFileSystem: cache tombstones block host hits.
    let tracking = Tracking::new(host(
        &[("/remove.ts", "host"), ("/removed-dir/gone.ts", "host")],
        true,
    ));
    let fs = over(
        &Request::layer()
            .removed("/remove.ts")
            .removed("/removed-dir"),
        &on_host(Arc::clone(&tracking)),
    );
    tracking.clear();
    assert!(!fs.is_file("/remove.ts"));
    assert!(!fs.is_dir("/removed-dir"));
    assert!(!fs.is_file("/removed-dir/gone.ts"));
    assert!(tracking.is_empty());
}

#[test]
fn compacted_layers_keep_the_host_fallback() {
    // TestRequestFileSystem: compacted filesystem layers retain host
    // fallback.
    let host_fs = host(
        &[
            ("/host.ts", "host"),
            ("/removed.ts", "host removed"),
            ("/sealed/host.ts", "hidden from listing"),
            ("/open/host.ts", "host listing"),
            ("/open/layer-listed.ts", "host listed"),
        ],
        true,
    );
    let base = over(
        &Request::layer()
            .file("/inherited.ts", "inherited")
            .file("/sealed/inherited.ts", "sealed inherited")
            .listing("/sealed", &["inherited.ts"], &[])
            .removed("/removed.ts"),
        &on_host(Arc::clone(&host_fs)),
    );
    let layered = over(
        &Request::layer()
            .file("/added.ts", "added")
            .file("/sealed/added.ts", "sealed added")
            .listing("/open", &["layer-listed.ts"], &[]),
        &on(&base),
    );
    assert!(same_base(&layered, &host_fs));
    assert!(!layered.is_full());
    for (path, expected) in [
        ("/host.ts", "host"),
        ("/inherited.ts", "inherited"),
        ("/added.ts", "added"),
    ] {
        assert_eq!(
            read(layered.as_ref(), path).as_deref(),
            Some(expected),
            "{path}"
        );
    }
    assert_eq!(read(layered.as_ref(), "/removed.ts"), None);
    assert_eq!(
        files(layered.as_ref(), "/sealed"),
        ["added.ts", "inherited.ts"]
    );
    assert_eq!(files(layered.as_ref(), "/open"), ["layer-listed.ts"]);
}

#[test]
fn a_layer_compacted_over_a_full_file_system_is_full() {
    // TestRequestFileSystem: compacting a filesystem layer over a full
    // filesystem produces a full filesystem.
    let host_fs = host(&[("/host.ts", "host")], true);
    let base = over(
        &Request::full()
            .file("/target/inherited.ts", "inherited")
            .listing("/target", &["inherited.ts"], &[])
            .link("/link", "/target", false),
        &on_host(Arc::clone(&host_fs)),
    );
    let layered = over(
        &Request::layer().file("/target/added.ts", "added"),
        &on(&base),
    );
    assert!(layered.is_full());
    assert!(same_base(&layered, &host_fs));
    for (path, expected) in [
        ("/link/inherited.ts", "inherited"),
        ("/link/added.ts", "added"),
    ] {
        assert_eq!(
            read(layered.as_ref(), path).as_deref(),
            Some(expected),
            "{path}"
        );
    }
    assert_eq!(read(layered.as_ref(), "/host.ts"), None);
}

#[test]
fn a_full_file_system_reaches_the_host_only_through_a_host_link() {
    // TestRequestFileSystem: memory routes explicit host symlinks to the
    // host only through the link.
    let tracking = Tracking::new(host(
        &[
            (
                "/host/node_modules/pkg/index.d.ts",
                "export declare const hostValue: string;",
            ),
            ("/host/outside.ts", "outside"),
        ],
        true,
    ));
    let fs = over(
        &Request::full()
            .file("/project/index.ts", r#"import { hostValue } from "pkg";"#)
            .link("/project/node_modules", "/host/node_modules", true),
        &on_host(Arc::clone(&tracking)),
    );
    assert_eq!(read(fs.as_ref(), "/host/outside.ts"), None);
    assert!(!tracking.has("/host/outside.ts"));
    assert_eq!(
        read(fs.as_ref(), "/project/node_modules/pkg/index.d.ts").as_deref(),
        Some("export declare const hostValue: string;")
    );
    assert!(tracking.has("/host/node_modules/pkg/index.d.ts"));
    assert_eq!(
        fs.canonicalize("/project/node_modules/pkg/index.d.ts")
            .unwrap(),
        "/host/node_modules/pkg/index.d.ts"
    );
    let entries = fs.accessible_entries("/project");
    assert_eq!(entries.directories, ["node_modules"]);
    assert!(entries.symlinks.contains("node_modules"));
}

#[test]
fn a_layers_host_link_bypasses_the_snapshots_base() {
    // TestRequestFileSystem: layered host symlinks bypass snapshot bases.
    let tracking = Tracking::new(host(&[("/host/pkg/index.d.ts", "host")], true));
    let base = over(
        &Request::full().file("/memory.ts", "memory"),
        &on_host(Arc::clone(&tracking)),
    );
    let layered = over(
        &Request::layer().link("/project/pkg", "/host/pkg", true),
        &on(&base),
    );
    assert_eq!(
        read(layered.as_ref(), "/project/pkg/index.d.ts").as_deref(),
        Some("host")
    );
    assert!(layered.is_file("/project/pkg/index.d.ts"));
    assert!(layered.is_dir("/project/pkg"));
    assert_eq!(files(layered.as_ref(), "/project/pkg"), ["index.d.ts"]);
    assert_eq!(
        layered.canonicalize("/project/pkg/index.d.ts").unwrap(),
        "/host/pkg/index.d.ts"
    );
    assert!(layered.metadata("/project/pkg/index.d.ts").is_ok());
    assert!(tracking.has("/host/pkg/index.d.ts"));
}

#[test]
fn an_inherited_host_link_bypasses_newer_entries_at_its_target() {
    // TestRequestFileSystem: inherited host symlinks bypass newer cache
    // entries at the target.
    let base = over(
        &Request::full().link("/link", "/host/pkg", true),
        &on_host(host(
            &[
                ("/host/pkg/host.ts", "host"),
                ("/host/pkg/removed.ts", "removed"),
            ],
            true,
        )),
    );
    let layered = over(
        &Request::layer()
            .removed("/link/removed.ts")
            .file("/host/pkg/host.ts", "cache")
            .file("/host/pkg/cache-only.ts", "cache only"),
        &on(&base),
    );
    assert_eq!(
        read(layered.as_ref(), "/link/host.ts").as_deref(),
        Some("host")
    );
    assert!(!layered.is_file("/link/cache-only.ts"));
    assert!(!layered.is_file("/link/removed.ts"));
    assert_eq!(layered.metadata("/link/host.ts").unwrap().len(), 4);
    assert_eq!(files(layered.as_ref(), "/link"), ["host.ts"]);
}

#[test]
fn colliding_canonical_paths_are_rejected() {
    // TestRequestFileSystem: canonical path collisions are rejected.
    let base = on_host(host(&[], false));
    let error = try_over(
        &Request::full()
            .file(r"C:\Repo\file.ts", "first")
            .file("c:/repo/file.ts", "second"),
        &base,
        r"C:\Workspace",
    )
    .err()
    .unwrap();
    assert!(
        error.contains("duplicate request filesystem file path"),
        "{error}"
    );
    let error = try_over(
        &Request::full()
            .listing(r"C:\Repo", &[], &[])
            .listing("c:/repo/.", &[], &[]),
        &base,
        r"C:\Workspace",
    )
    .err()
    .unwrap();
    assert!(
        error.contains("duplicate request filesystem directory path"),
        "{error}"
    );
    let error = try_over(
        &Request::full()
            .link(r"C:\Repo\link", r"C:\Target", false)
            .link("c:/repo/link", r"C:\Other", false),
        &base,
        r"C:\Workspace",
    )
    .err()
    .unwrap();
    assert!(
        error.contains("duplicate request filesystem symlink path"),
        "{error}"
    );
}

#[test]
fn a_link_cycle_is_missing() {
    // TestRequestFileSystem: symlink cycles are treated as missing.
    let tracking = Tracking::new(host(&[("/host.ts", "host")], true));
    let fs = over(
        &Request::full()
            .link("/a", "/b", false)
            .link("/b", "/a", false),
        &on_host(Arc::clone(&tracking)),
    );
    assert_eq!(read(fs.as_ref(), "/a/file.ts"), None);
    assert!(!fs.is_dir("/a"));
    assert_eq!(fs.canonicalize("/a").unwrap(), "/a");
    assert!(tracking.is_empty());
}

#[test]
fn a_relative_posix_link_target_starts_at_the_links_directory() {
    // TestRequestFileSystem: posix relative symlink targets resolve from the
    // link directory.
    let tracking = Tracking::new(host(&[], true));
    let fs = try_over(
        &Request::full()
            .file(
                "/packages/pkg/index.d.ts",
                "export declare const value: number;",
            )
            .link("/project/pkg", "../packages/pkg", false),
        &on_host(Arc::clone(&tracking)),
        r"C:\Workspace",
    )
    .unwrap();
    assert_eq!(
        read(fs.as_ref(), "/project/pkg/index.d.ts").as_deref(),
        Some("export declare const value: number;")
    );
    assert_eq!(
        fs.canonicalize("/project/pkg/index.d.ts").unwrap(),
        "/packages/pkg/index.d.ts"
    );
    assert!(tracking.is_empty());
}

#[test]
fn vscode_uri_paths_have_listings_links_and_tombstones() {
    // TestRequestFileSystem: vscode document URI paths support listings
    // symlinks and tombstones.
    let tracking = Tracking::new(host(&[], true));
    let root = "vscode-remote://ssh-remote+host/workspace";
    let fs = over(
        &Request::full()
            .file(&format!("{root}/src/index.ts"), "index")
            .file(&format!("{root}/packages/pkg/a.ts"), "package")
            .link(&format!("{root}/src/pkg"), "../packages/pkg", false)
            .removed(&format!("{root}/packages/pkg/removed.ts")),
        &on_host(Arc::clone(&tracking)),
    );
    assert_eq!(
        read(fs.as_ref(), &format!("{root}/src/index.ts")).as_deref(),
        Some("index")
    );
    assert_eq!(
        read(fs.as_ref(), &format!("{root}/src/pkg/a.ts")).as_deref(),
        Some("package")
    );
    assert_eq!(
        fs.canonicalize(&format!("{root}/src/pkg/a.ts")).unwrap(),
        format!("{root}/packages/pkg/a.ts")
    );
    assert_eq!(files(fs.as_ref(), &format!("{root}/src")), ["index.ts"]);
    assert_eq!(directories(fs.as_ref(), &format!("{root}/src")), ["pkg"]);
    assert!(!fs.is_file(&format!("{root}/src/pkg/removed.ts")));
    assert!(tracking.is_empty());
}

#[test]
fn windows_paths_resolve_links_without_case() {
    // TestRequestFileSystem: windows paths resolve symlinks case
    // insensitively.
    let tracking = Tracking::new(host(&[("C:/Host/outside.ts", "outside")], false));
    let fs = try_over(
        &Request::full()
            .file(
                r"C:\Repo\Packages\Pkg\Index.d.ts",
                "export declare const windowsValue: number;",
            )
            .listing(r"C:\Repo\Project\node_modules", &[], &["pkg"])
            .link(
                r"C:\Repo\Project\node_modules\PKG",
                r"..\..\Packages\Pkg",
                false,
            )
            .link(
                r"C:\Repo\Project\Current.d.ts",
                r"..\Packages\Pkg\Index.d.ts",
                false,
            ),
        &on_host(Arc::clone(&tracking)),
        r"C:\Workspace",
    )
    .unwrap();
    let declaration = Some("export declare const windowsValue: number;");
    assert_eq!(
        read(fs.as_ref(), r"c:\repo\project\NODE_MODULES\pkg\INDEX.D.TS").as_deref(),
        declaration
    );
    assert_eq!(
        read(fs.as_ref(), r"C:\REPO\PROJECT\current.d.ts").as_deref(),
        declaration
    );
    assert_eq!(
        fs.canonicalize(r"c:\repo\project\node_modules\pkg\index.d.ts")
            .unwrap(),
        "C:/Repo/Packages/Pkg/index.d.ts"
    );
    let entries = fs.accessible_entries(r"c:\REPO\project\NODE_MODULES");
    assert_eq!(entries.directories, ["PKG"]);
    assert!(entries.symlinks.contains("PKG"));
    assert!(tracking.is_empty());
}

#[test]
fn case_insensitive_link_matching_survives_a_change_of_byte_length() {
    // TestRequestFileSystem: case insensitive symlink matching handles
    // unicode byte length changes (the Kelvin sign lowercases to `k`).
    let fs = try_over(
        &Request::full().file("C:/Repo/target.ts", "target").link(
            "C:/Repo/\u{212A}",
            "C:/Repo/target.ts",
            false,
        ),
        &on_host(host(&[], false)),
        "C:/Repo",
    )
    .unwrap();
    assert_eq!(read(fs.as_ref(), "c:/repo/k").as_deref(), Some("target"));
}

#[test]
fn a_full_file_system_is_read_only() {
    // TestRequestFileSystem: full request filesystems are immutable after
    // eager compaction.
    let memory = over(
        &Request::full().file("/src/a.ts", "a"),
        &on_host(host(&[("/host.ts", "host")], true)),
    );
    let invalid =
        |result: io::Result<()>| result.unwrap_err().kind() == io::ErrorKind::InvalidInput;
    assert!(invalid(memory.write("/src/b.ts", b"b")));
    assert!(invalid(memory.append("/src/a.ts", b"b")));
    assert!(invalid(memory.remove("/src")));
    assert_eq!(read(memory.as_ref(), "/src/a.ts").as_deref(), Some("a"));
    let cache = over(&Request::layer(), &on(&memory));
    assert!(cache.is_full());
    assert!(invalid(cache.write("/written.ts", b"written")));
}

#[test]
fn a_layer_writes_through_to_the_host() {
    // TestRequestFileSystem: layer request filesystems write through after
    // eager compaction.
    let host_fs = host(&[], true);
    let base = over(&Request::layer(), &on_host(Arc::clone(&host_fs)));
    let cache = over(&Request::layer(), &on(&base));
    assert!(!cache.is_full());
    cache.write("/written.ts", b"written").unwrap();
    cache.append("/written.ts", b" appended").unwrap();
    assert_eq!(
        read(host_fs.as_ref(), "/written.ts").as_deref(),
        Some("written appended")
    );
    cache.remove("/written.ts").unwrap();
    assert!(!host_fs.is_file("/written.ts"));
}

/// A host that writes a file only as a whole: its plain write and its
/// directory creation fail, as a client's callbacks leave both to the
/// client's own write.
struct WholeWritesOnly(Arc<MemFs>);

impl FileSystem for WholeWritesOnly {
    fn case_sensitive(&self) -> bool {
        self.0.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.0.read(path)
    }

    fn metadata(&self, path: &str) -> io::Result<Metadata> {
        self.0.metadata(path)
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.0.read_dir(path)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.0.canonicalize(path)
    }

    fn write(&self, _path: &str, _contents: &[u8]) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.0.append(path, contents)
    }

    fn create_dir_all(&self, _path: &str) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.0.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.0.set_modified(path, modified)
    }

    fn write_creating_dirs(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.0.write_creating_dirs(path, contents)
    }
}

#[test]
fn a_layer_passes_a_whole_write_to_the_host() {
    // tsgo `requestFileSystem.WriteFile` is the host's `WriteFile`.
    let host_fs = host(&[], true);
    let base = over(
        &Request::layer(),
        &on_host(Arc::new(WholeWritesOnly(Arc::clone(&host_fs)))),
    );
    let cache = over(&Request::layer(), &on(&base));
    cache
        .write_creating_dirs("/out/dir/a.js", b"written")
        .unwrap();
    assert_eq!(
        read(host_fs.as_ref(), "/out/dir/a.js").as_deref(),
        Some("written")
    );
}

#[test]
fn a_layers_writes_follow_inherited_links() {
    // TestRequestFileSystem: cache mutations follow inherited request
    // symlinks.
    let host_fs = host(
        &[
            ("/target/write.ts", "target"),
            ("/target/append.ts", "target"),
            ("/target/remove.ts", "target"),
            ("/target/times.ts", "target"),
            ("/link/write.ts", "alias"),
            ("/link/append.ts", "alias"),
            ("/link/remove.ts", "alias"),
            ("/link/times.ts", "alias"),
        ],
        true,
    );
    let base = over(
        &Request::layer().link("/link", "/target", false),
        &on_host(Arc::clone(&host_fs)),
    );
    let cache = over(&Request::layer(), &on(&base));
    cache.write("/link/write.ts", b"written").unwrap();
    assert_eq!(
        read(host_fs.as_ref(), "/target/write.ts").as_deref(),
        Some("written")
    );
    cache.append("/link/append.ts", b" appended").unwrap();
    assert_eq!(
        read(host_fs.as_ref(), "/target/append.ts").as_deref(),
        Some("target appended")
    );
    cache.remove("/link/remove.ts").unwrap();
    assert!(!host_fs.is_file("/target/remove.ts"));
    let modified = UNIX_EPOCH + Duration::from_secs(123);
    cache.set_modified("/link/times.ts", modified).unwrap();
    assert_eq!(
        host_fs.metadata("/target/times.ts").unwrap().modified(),
        modified
    );
}

#[test]
fn windows_and_posix_roots_link_across_and_relatively() {
    // TestRequestFileSystem: mixed windows and posix roots support
    // cross-root and relative symlinks.
    let tracking = Tracking::new(host(
        &[(
            "C:/Host/node_modules/host-pkg/index.d.ts",
            "export declare const hostValue: boolean;",
        )],
        false,
    ));
    let fs = try_over(
        &Request::full()
            .file(
                r"C:\Repo\Packages\windows-pkg\index.d.ts",
                "export declare const windowsValue: number;",
            )
            .file(
                "/repo/packages/posix-pkg/index.d.ts",
                "export declare const posixValue: string;",
            )
            .link(
                r"C:\Repo\Project\node_modules\posix-pkg",
                "/repo/packages/posix-pkg",
                false,
            )
            .link(
                "/repo/project/node_modules/windows-pkg",
                r"C:\Repo\Packages\windows-pkg",
                false,
            )
            .link(
                r"C:\Repo\Project\windows-pkg.d.ts",
                r"..\Packages\windows-pkg\index.d.ts",
                false,
            )
            .link(
                r"C:\Repo\Project\node_modules\host-pkg",
                r"..\..\..\Host\node_modules\host-pkg",
                true,
            ),
        &on_host(Arc::clone(&tracking)),
        r"C:\Workspace",
    )
    .unwrap();
    assert_eq!(
        read(
            fs.as_ref(),
            r"c:\REPO\project\NODE_MODULES\POSIX-PKG\INDEX.D.TS"
        )
        .as_deref(),
        Some("export declare const posixValue: string;")
    );
    assert_eq!(
        read(
            fs.as_ref(),
            "/REPO/PROJECT/NODE_MODULES/WINDOWS-PKG/INDEX.D.TS"
        )
        .as_deref(),
        Some("export declare const windowsValue: number;")
    );
    assert_eq!(
        read(fs.as_ref(), r"c:\repo\project\WINDOWS-PKG.D.TS").as_deref(),
        Some("export declare const windowsValue: number;")
    );
    assert_eq!(
        read(
            fs.as_ref(),
            r"C:\Repo\Project\node_modules\HOST-PKG\index.d.ts"
        )
        .as_deref(),
        Some("export declare const hostValue: boolean;")
    );
    assert_eq!(
        fs.canonicalize(r"c:\repo\project\node_modules\posix-pkg\index.d.ts")
            .unwrap(),
        "/repo/packages/posix-pkg/index.d.ts"
    );
    assert_eq!(
        fs.canonicalize("/repo/project/node_modules/windows-pkg/index.d.ts")
            .unwrap(),
        "C:/Repo/Packages/windows-pkg/index.d.ts"
    );
    assert!(tracking.has("C:/Host/node_modules/host-pkg/index.d.ts"));
}

// filechanges_test.go

fn changes_over(request: &Request, base: &SnapshotFileSystem) -> FileChangeSummary {
    let mut summary = FileChangeSummary::default();
    RequestFileSystem::new_for_update(&request.0, base, "/", &mut summary).unwrap();
    summary
}

fn names(set: &BTreeSet<String>) -> Vec<&str> {
    set.iter().map(String::as_str).collect()
}

#[test]
fn file_changes_include_directory_tombstones() {
    // TestFileChangesIncludeDirectoryTombstones.
    let base = over(
        &Request::full()
            .file("/removed/nested/file.ts", "removed")
            .file("/replaced.ts", "old")
            .link("/alias", "/removed", false),
        &on_host(host(&[], true)),
    );
    let summary = changes_over(
        &Request::layer()
            .file("/replaced.ts", "new")
            .removed("removed")
            .removed("/missing")
            .removed("/replaced.ts"),
        &on(&base),
    );
    assert!(!summary.invalidate_all);
    assert_eq!(names(&summary.deleted), ["/alias", "/removed"]);
    assert_eq!(names(&summary.changed), ["/replaced.ts"]);
}

#[test]
fn file_changes_include_listings_and_links() {
    // TestFileChangesIncludeListingsAndSymlinks.
    let summary = changes_over(
        &Request::layer()
            .listing("/dir", &[], &[])
            .link("/link", "/target", false)
            .link("/new", "/host", true),
        &on_host(host(
            &[
                ("/dir/old.ts", "old listing"),
                ("/link/old.ts", "old target"),
            ],
            true,
        )),
    );
    assert!(!summary.invalidate_all);
    assert_eq!(names(&summary.deleted), ["/dir", "/link"]);
    assert_eq!(names(&summary.created), ["/dir", "/link", "/new"]);
}

#[test]
fn file_changes_include_recursive_link_aliases() {
    // TestFileChangesIncludeRecursiveSymlinkAliases.
    let base = over(
        &Request::full()
            .file("/dir/file.ts", "old")
            .link("/dir/link", "/dir", false),
        &on_host(host(&[], true)),
    );
    let summary = changes_over(&Request::layer().file("/dir/file.ts", "new"), &on(&base));
    assert_eq!(
        names(&summary.changed),
        ["/dir/file.ts", "/dir/link/file.ts"]
    );
    assert!(summary.created.is_empty());
}

#[test]
fn file_changes_include_root_link_aliases() {
    // TestFileChangesIncludeRootSymlinkAliases.
    let base = over(
        &Request::full()
            .file("/file.ts", "old")
            .link("/link", "/", false),
        &on_host(host(&[], true)),
    );
    assert_eq!(read(base.as_ref(), "/link/file.ts").as_deref(), Some("old"));
    let summary = changes_over(&Request::layer().file("/file.ts", "new"), &on(&base));
    assert_eq!(names(&summary.changed), ["/file.ts", "/link/file.ts"]);
    assert!(summary.created.is_empty());
}

#[test]
fn expanded_changes_reach_a_files_other_names() {
    // tsgo `ExpandFileChanges`, which the snapshot applies to a host's
    // notifications.
    let fs = over(
        &Request::full()
            .file("/target/file.ts", "text")
            .link("/link", "/target", false),
        &on_host(host(&[], true)),
    );
    let summary = fs.expand_file_changes(FileChangeSummary {
        changed: BTreeSet::from(["/target/file.ts".to_owned()]),
        ..FileChangeSummary::default()
    });
    assert_eq!(
        names(&summary.changed),
        ["/link/file.ts", "/target/file.ts"]
    );
}

// pathtree_test.go

fn symlink_entry(link_name: &str, target: &str) -> Option<Entry> {
    Some(Entry::Symlink(Arc::new(super::path_tree::RequestSymlink {
        link_name: link_name.to_owned(),
        target: target.to_owned(),
        host: false,
    })))
}

fn file_entry(file_name: &str, content: &str) -> Option<Entry> {
    Some(Entry::File(Arc::new(RequestFile {
        file_name: file_name.to_owned(),
        content: content.to_owned(),
    })))
}

fn directory_entry(directory_name: &str, listing: Option<&[&str]>) -> Option<Entry> {
    Some(Entry::Directory(Arc::new(RequestDirectory {
        directory_name: directory_name.to_owned(),
        listing: listing.map(|files| Entries {
            files: files.iter().map(|file| (*file).to_owned()).collect(),
            ..Entries::default()
        }),
    })))
}

fn composed(base: PathNode, layer: PathNode, case_sensitive: bool) -> Arc<PathNode> {
    compose(
        Some(&Arc::new(base)),
        Some(&Arc::new(layer)),
        Fallback::Allowed,
        case_sensitive,
    )
    .unwrap()
}

#[test]
fn a_child_overrides_an_inherited_removal() {
    // TestRequestPathTreeChildOverridesInheritedMissing.
    let mut base = PathNode::default();
    base.ensure("/dir").fallback = Fallback::Missing;
    let mut layer = PathNode::default();
    layer.ensure("/dir/pkg").entry = symlink_entry("/dir/pkg", "/target");
    let compacted = composed(base.clone(), layer, true);
    assert_eq!(compacted.lookup("/dir/pkg/file.ts").1, Fallback::Allowed);
    assert_eq!(compacted.lookup("/dir/other.ts").1, Fallback::Missing);
    assert_eq!(base.lookup("/dir/pkg/file.ts").1, Fallback::Missing);
}

#[test]
fn a_same_layer_removal_blocks_a_link() {
    // TestRequestPathTreeSameLayerMissingBlocksSymlink.
    let mut layer = PathNode::default();
    layer.ensure("/dir").fallback = Fallback::Missing;
    layer.ensure("/dir/pkg").entry = symlink_entry("/dir/pkg", "/target");
    let compacted = composed(PathNode::default(), layer, true);
    assert_eq!(compacted.lookup("/dir/pkg/file.ts").1, Fallback::Missing);
}

#[test]
fn a_directory_keeps_an_inherited_removal() {
    // TestRequestPathTreeDirectoryPreservesInheritedMissing.
    let mut base = PathNode::default();
    base.ensure("/dir").fallback = Fallback::Missing;
    let mut layer = PathNode::default();
    layer.ensure("/dir/new").entry = directory_entry("/dir/new", None);
    let compacted = composed(base, layer, true);
    let (node, fallback) = compacted.lookup("/dir/new");
    assert!(matches!(
        &node.unwrap().entry,
        Some(Entry::Directory(directory)) if directory.directory_name == "/dir/new"
    ));
    assert_eq!(fallback, Fallback::Missing);
    assert_eq!(compacted.lookup("/dir/new/old.ts").1, Fallback::Missing);
}

#[test]
fn a_file_replaces_a_subtree() {
    // TestRequestPathTreeFileReplacesSubtree.
    let mut base = PathNode::default();
    base.ensure("/dir").entry = directory_entry("/dir", Some(&["old.ts"]));
    base.ensure("/dir/old.ts").entry = file_entry("/dir/old.ts", "old");
    let mut layer = PathNode::default();
    layer.ensure("/dir").entry = file_entry("/dir", "new");
    let compacted = composed(base.clone(), layer, true);
    let node = compacted.lookup("/dir").0.unwrap();
    assert!(matches!(&node.entry, Some(Entry::File(file)) if file.content == "new"));
    assert!(node.children.is_empty());
    assert!(compacted.contains_file_ancestor("/dir/old.ts"));
    assert!(matches!(
        &base.lookup("/dir/old.ts").0.unwrap().entry,
        Some(Entry::File(file)) if file.content == "old"
    ));
}

#[test]
fn a_listing_replacement_keeps_the_files() {
    // TestRequestPathTreeListingReplacementDoesNotRemoveFiles.
    let tracking = Tracking::new(host(&[], true));
    let base = over(
        &Request::layer()
            .file("/dir/retained.ts", "retained")
            .listing("/dir", &["retained.ts"], &[]),
        &on_host(Arc::clone(&tracking)),
    );
    let compacted = over(&Request::layer().listing("/dir", &[], &[]), &on(&base));
    assert_eq!(
        read(compacted.as_ref(), "/dir/retained.ts").as_deref(),
        Some("retained")
    );
    assert!(files(compacted.as_ref(), "/dir").is_empty());
    assert_eq!(files(base.as_ref(), "/dir"), ["retained.ts"]);
    verify_compaction_without_host_reads(&compacted, &tracking, &["/dir", "/dir/retained.ts"]);
}

#[test]
fn composition_keeps_listing_snapshots() {
    // TestRequestPathTreeCompositionPreservesListingSnapshots.
    let mut base = PathNode::default();
    base.ensure("/dir").entry = directory_entry("/dir", Some(&["OLD.ts"]));
    base.ensure("/dir/old.ts").entry = file_entry("/dir/OLD.ts", "");
    let mut layer = PathNode::default();
    layer.ensure("/dir/old.ts").fallback = Fallback::Missing;
    layer.ensure("/dir/new.ts").entry = file_entry("/dir/new.ts", "");
    let compacted = composed(base.clone(), layer, false);
    let node = compacted.lookup("/dir").0.unwrap();
    assert!(matches!(
        &node.entry,
        Some(Entry::Directory(directory)) if directory.listing.as_ref().unwrap().files == ["new.ts"]
    ));
    assert!(matches!(
        &base.lookup("/dir").0.unwrap().entry,
        Some(Entry::Directory(directory)) if directory.listing.as_ref().unwrap().files == ["OLD.ts"]
    ));
    let next = compose(
        Some(&compacted),
        Some(&Arc::default()),
        Fallback::Allowed,
        false,
    )
    .unwrap();
    assert!(std::ptr::eq(next.lookup("/dir").0.unwrap(), node));
}

#[test]
fn a_file_wins_over_a_same_layer_link() {
    // TestRequestPathTreeFileTakesPrecedenceOverSameLayerSymlink.
    let fs = over(
        &Request::full()
            .file("/item", "file")
            .file("/target/file.ts", "target")
            .link("/item", "/target", false),
        &on_host(host(&[], true)),
    );
    assert_eq!(read(fs.as_ref(), "/item").as_deref(), Some("file"));
    assert!(!fs.is_dir("/item"));
    assert_eq!(fs.canonicalize("/item").unwrap(), "/item");
    assert!(matches!(
        fs.paths.lookup("/item").0.unwrap().entry,
        Some(Entry::File(_))
    ));
    assert!(!fs.paths.has_symlinks);
    assert_eq!(files(fs.as_ref(), "/"), ["item"]);
}

#[test]
fn a_directory_wins_over_a_same_layer_link() {
    // TestRequestPathTreeDirectoryTakesPrecedenceOverSameLayerSymlink.
    let fs = over(
        &Request::full()
            .file("/item/child.ts", "child")
            .file("/target.ts", "target")
            .listing("/item", &["child.ts"], &[])
            .link("/item", "/target.ts", false),
        &on_host(host(&[], true)),
    );
    assert!(fs.is_dir("/item"));
    assert!(!fs.is_file("/item"));
    assert_eq!(fs.canonicalize("/item").unwrap(), "/item");
    assert_eq!(
        read(fs.as_ref(), "/item/child.ts").as_deref(),
        Some("child")
    );
    assert!(matches!(
        fs.paths.lookup("/item").0.unwrap().entry,
        Some(Entry::Directory(_))
    ));
    assert!(!fs.paths.has_symlinks);
    assert_eq!(files(fs.as_ref(), "/item"), ["child.ts"]);
}

#[test]
fn a_link_wins_over_a_listing_hint() {
    // TestRequestPathTreeSymlinkTakesPrecedenceOverListingHint.
    let fs = over(
        &Request::full()
            .file("/target/file.ts", "target")
            .listing("/links", &[], &["pkg"])
            .link("/links/pkg", "/target", false),
        &on_host(host(&[], true)),
    );
    assert!(matches!(
        fs.paths.lookup("/links/pkg").0.unwrap().entry,
        Some(Entry::Symlink(_))
    ));
    assert!(fs.paths.has_symlinks);
    assert_eq!(
        read(fs.as_ref(), "/links/pkg/file.ts").as_deref(),
        Some("target")
    );
    assert_eq!(fs.canonicalize("/links/pkg").unwrap(), "/target");
    assert_eq!(
        fs.accessible_entries("/links"),
        Entries {
            files: Vec::new(),
            directories: vec!["pkg".to_owned()],
            symlinks: BTreeSet::from(["pkg".to_owned()]),
        }
    );
}

#[test]
fn a_file_wins_over_a_same_layer_directory() {
    // TestRequestPathTreeFileTakesPrecedenceOverSameLayerDirectory.
    let fs = over(
        &Request::full()
            .file("/item", "file")
            .listing("/item", &["listed.ts"], &[]),
        &on_host(host(&[], true)),
    );
    assert!(matches!(
        fs.paths.lookup("/item").0.unwrap().entry,
        Some(Entry::File(_))
    ));
    assert_eq!(read(fs.as_ref(), "/item").as_deref(), Some("file"));
    assert!(!fs.is_dir("/item"));
    assert!(files(fs.as_ref(), "/item").is_empty());
}

#[test]
fn a_file_and_a_directory_have_their_own_metadata() {
    // TestRequestPathTree{File,Directory}ProvidesStatAndDirEntry.
    let fs = over(
        &Request::full()
            .file("/dir/file.ts", "file content")
            .listing("/empty", &[], &[]),
        &on_host(host(&[], true)),
    );
    let metadata = fs.metadata("/dir/file.ts").unwrap();
    assert_eq!(metadata.file_type(), FileType::File);
    assert_eq!(metadata.len(), "file content".len() as u64);
    assert_eq!(metadata.modified(), UNIX_EPOCH);
    let metadata = fs.metadata("/empty").unwrap();
    assert_eq!(metadata.file_type(), FileType::Directory);
    assert_eq!(metadata.len(), 0);
}

#[test]
fn a_link_reports_its_targets_metadata() {
    // TestRequestPathTreeSymlinkReportsTargetMetadata.
    let fs = over(
        &Request::full()
            .file("/target/file.ts", "target content")
            .link("/link.ts", "/target/file.ts", false),
        &on_host(host(&[], true)),
    );
    let target = fs.metadata("/target/file.ts").unwrap();
    let link = fs.metadata("/link.ts").unwrap();
    assert_eq!(
        (link.file_type(), link.len(), link.modified()),
        (target.file_type(), target.len(), target.modified())
    );
}

#[test]
fn the_hosts_metadata_is_kept() {
    // TestRequestPathTreeStatPreservesHostMetadata.
    let host_fs = host(&[("/host.ts", "host content")], true);
    let modified = UNIX_EPOCH + Duration::from_secs(1_788_955_200);
    host_fs.set_modified("/host.ts", modified).unwrap();
    let fs = over(&Request::layer(), &on_host(host_fs));
    let metadata = fs.metadata("/host.ts").unwrap();
    assert_eq!(metadata.modified(), modified);
    assert_eq!(metadata.len(), "host content".len() as u64);
}

/// tsgo `requestTestHostMetadata` without metadata: a host that only says
/// what exists.
struct ExistenceOnly(Arc<MemFs>);

impl FileSystem for ExistenceOnly {
    fn case_sensitive(&self) -> bool {
        self.0.case_sensitive()
    }

    fn read(&self, path: &str) -> io::Result<Vec<u8>> {
        self.0.read(path)
    }

    fn metadata(&self, _path: &str) -> io::Result<Metadata> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn read_dir(&self, path: &str) -> io::Result<Vec<DirEntry>> {
        self.0.read_dir(path)
    }

    fn canonicalize(&self, path: &str) -> io::Result<String> {
        self.0.canonicalize(path)
    }

    fn write(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.0.write(path, contents)
    }

    fn append(&self, path: &str, contents: &[u8]) -> io::Result<()> {
        self.0.append(path, contents)
    }

    fn create_dir_all(&self, path: &str) -> io::Result<()> {
        self.0.create_dir_all(path)
    }

    fn remove(&self, path: &str) -> io::Result<()> {
        self.0.remove(path)
    }

    fn set_modified(&self, path: &str, modified: SystemTime) -> io::Result<()> {
        self.0.set_modified(path, modified)
    }

    fn is_file(&self, path: &str) -> bool {
        self.0.is_file(path)
    }

    fn is_dir(&self, path: &str) -> bool {
        self.0.is_dir(path)
    }
}

#[test]
fn metadata_falls_back_to_what_exists() {
    // TestRequestPathTreeStatSupportsExistenceOnlyHost.
    let host_fs = Arc::new(ExistenceOnly(host(
        &[("/dir/file.ts", "host content")],
        true,
    )));
    let fs = over(&Request::layer(), &on_host(host_fs));
    let file = fs.metadata("/dir/file.ts").unwrap();
    assert_eq!((file.file_type(), file.len()), (FileType::File, 0));
    let directory = fs.metadata("/dir").unwrap();
    assert_eq!(directory.file_type(), FileType::Directory);
    assert!(fs.metadata("/missing").is_err());
}

#[test]
fn an_unknown_kind_is_rejected() {
    let error = try_over(&Request::kind("memory"), &on_host(host(&[], true)), "/")
        .err()
        .unwrap();
    assert_eq!(error, r#"unknown request filesystem kind "memory""#);
}

#[test]
fn the_request_parameters_decode_tsgos_json() {
    let params: RequestFileSystemParams = serde_json::from_str(
        r#"{"kind":"layer","files":{"/a.ts":"a"},"directories":{"/d":{"files":["x"],"directories":null}},"symlinks":{"/l":{"target":"/t","host":true}},"removedPaths":["/gone"]}"#,
    )
    .unwrap();
    assert!(!params.is_full());
    assert_eq!(
        params.files,
        BTreeMap::from([("/a.ts".to_owned(), "a".to_owned())])
    );
    assert_eq!(params.directories["/d"].files, ["x"]);
    assert!(params.directories["/d"].directories.is_empty());
    assert!(params.symlinks["/l"].host);
    assert_eq!(params.removed_paths, ["/gone"]);
}
