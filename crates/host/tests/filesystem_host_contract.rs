use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tsc_host::{CompilerHost, FsCompilerHost, HostErrorKind, HostOperation, MemoryCompilerHost};

static NEXT_TEMP_TREE: AtomicU64 = AtomicU64::new(0);

struct TempTree {
    root: PathBuf,
}

impl TempTree {
    fn new() -> Self {
        loop {
            let sequence = NEXT_TEMP_TREE.fetch_add(1, Ordering::Relaxed);
            let candidate = std::env::temp_dir()
                .join(format!("tsc-rs-fs-host-{}-{sequence}", std::process::id()));
            match fs::create_dir(&candidate) {
                Ok(()) => {
                    #[cfg(windows)]
                    let root = dunce::canonicalize(candidate).expect("physicalize temp tree root");
                    #[cfg(not(windows))]
                    let root = fs::canonicalize(candidate).expect("physicalize temp tree root");
                    return Self { root };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create temp tree: {error}"),
            }
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            if !std::thread::panicking() {
                panic!("remove temp tree {}: {error}", self.root.display());
            }
        }
    }
}

fn native_case_profile() -> bool {
    FsCompilerHost::from_process()
        .expect("construct process filesystem host")
        .use_case_sensitive_file_names()
}

fn assert_same_observation(filesystem: &dyn CompilerHost, memory: &dyn CompilerHost, path: &Path) {
    assert_eq!(filesystem.read_file(path), memory.read_file(path));
    assert_eq!(filesystem.file_exists(path), memory.file_exists(path));
    assert_eq!(
        filesystem.directory_exists(path),
        memory.directory_exists(path)
    );
    assert_eq!(filesystem.read_directory(path), memory.read_directory(path));
    assert_eq!(filesystem.realpath(path), memory.realpath(path));
}

#[test]
fn filesystem_and_memory_hosts_observe_the_same_logical_tree() {
    let tree = TempTree::new();
    fs::create_dir(tree.path("empty-dir")).unwrap();
    fs::create_dir(tree.path("src")).unwrap();
    fs::write(tree.path("empty.ts"), []).unwrap();
    fs::write(tree.path("src/a.ts"), b"export const a = 1;").unwrap();

    let case_sensitive = native_case_profile();
    let filesystem = FsCompilerHost::new(tree.root(), case_sensitive).unwrap();
    let memory = MemoryCompilerHost::builder(tree.root())
        .case_sensitive(case_sensitive)
        .directory(tree.path("empty-dir"))
        .file(tree.path("empty.ts"), Vec::new())
        .file(tree.path("src/a.ts"), b"export const a = 1;".to_vec())
        .build()
        .unwrap();

    assert_eq!(filesystem.current_directory().unwrap(), tree.root());
    assert_eq!(filesystem.current_directory(), memory.current_directory());
    assert_eq!(
        filesystem.use_case_sensitive_file_names(),
        memory.use_case_sensitive_file_names()
    );
    for path in [
        tree.root().to_path_buf(),
        tree.path("empty-dir"),
        tree.path("empty.ts"),
        tree.path("src"),
        tree.path("src/a.ts"),
        tree.path("missing.ts"),
    ] {
        assert_same_observation(&filesystem, &memory, &path);
    }
}

#[test]
fn filesystem_host_preserves_source_bytes_without_decoding() {
    let tree = TempTree::new();
    let utf8_bom = [0xef, 0xbb, 0xbf, b'l', b'e', b't'];
    let utf16_le = [0xff, 0xfe, b'l', 0, b'e', 0, b't', 0];
    let utf16_be = [0xfe, 0xff, 0, b'l', 0, b'e', 0, b't'];
    let invalid_utf8 = [0xf0, 0x28, 0x8c, 0x28];
    for (name, bytes) in [
        ("utf8-bom.ts", utf8_bom.as_slice()),
        ("utf16-le.ts", utf16_le.as_slice()),
        ("utf16-be.ts", utf16_be.as_slice()),
        ("invalid-utf8.ts", invalid_utf8.as_slice()),
    ] {
        fs::write(tree.path(name), bytes).unwrap();
    }

    let filesystem = FsCompilerHost::new(tree.root(), native_case_profile()).unwrap();
    for (name, bytes) in [
        ("utf8-bom.ts", utf8_bom.as_slice()),
        ("utf16-le.ts", utf16_le.as_slice()),
        ("utf16-be.ts", utf16_be.as_slice()),
        ("invalid-utf8.ts", invalid_utf8.as_slice()),
    ] {
        assert_eq!(
            filesystem.read_file(&tree.path(name)).unwrap(),
            Some(bytes.to_vec())
        );
    }
}

#[test]
fn filesystem_case_profile_matches_native_lookup() {
    let tree = TempTree::new();
    fs::write(tree.path("MiXeD.ts"), b"bytes").unwrap();

    let process_host = FsCompilerHost::from_process().unwrap();
    assert_eq!(
        process_host.current_directory().unwrap(),
        std::env::current_dir().unwrap()
    );
    #[cfg(windows)]
    assert!(!process_host.use_case_sensitive_file_names());
    #[cfg(not(windows))]
    {
        let executable = std::env::current_exe().unwrap();
        let swapped = PathBuf::from(
            executable
                .to_str()
                .unwrap()
                .chars()
                .map(|character| {
                    if character.is_ascii_lowercase() {
                        character.to_ascii_uppercase()
                    } else if character.is_ascii_uppercase() {
                        character.to_ascii_lowercase()
                    } else {
                        character
                    }
                })
                .collect::<String>(),
        );
        let detected = match fs::metadata(swapped) {
            Ok(_) => false,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                true
            }
            Err(error) => panic!("inspect swapped executable path: {error}"),
        };
        assert_eq!(process_host.use_case_sensitive_file_names(), detected);
    }

    let swapped = tree.path("mIxEd.TS");
    let case_sensitive = !swapped.exists();
    let filesystem = FsCompilerHost::new(tree.root(), case_sensitive).unwrap();
    let memory = MemoryCompilerHost::builder(tree.root())
        .case_sensitive(case_sensitive)
        .file(tree.path("MiXeD.ts"), b"bytes".to_vec())
        .build()
        .unwrap();

    assert_eq!(filesystem.file_exists(&swapped).unwrap(), !case_sensitive);
    assert_eq!(
        filesystem.file_exists(&swapped),
        memory.file_exists(&swapped)
    );
}

#[test]
fn invalid_filesystem_host_inputs_fail_closed() {
    let tree = TempTree::new();
    let relative = FsCompilerHost::new("relative", true).unwrap_err();
    assert_eq!(relative.kind(), HostErrorKind::InvalidInput);
    assert_eq!(relative.operation(), HostOperation::CurrentDirectory);

    fs::write(tree.path("file.ts"), b"bytes").unwrap();
    let wrong_kind = FsCompilerHost::new(tree.path("file.ts"), true).unwrap_err();
    assert_eq!(wrong_kind.kind(), HostErrorKind::InvalidInput);
    assert_eq!(wrong_kind.operation(), HostOperation::CurrentDirectory);

    let host = FsCompilerHost::new(tree.root(), true).unwrap();
    let nul = PathBuf::from(format!("{}\0file.ts", tree.root().display()));
    let error = host.read_file(&nul).unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidInput);
    assert_eq!(error.operation(), HostOperation::ReadFile);
    assert_eq!(error.path(), Some(nul.as_path()));
}

#[cfg(unix)]
#[test]
fn filesystem_host_follows_symlinks_and_rejects_inspection_failures() {
    use std::os::unix::fs::symlink;
    use std::os::unix::net::UnixListener;

    let tree = TempTree::new();
    fs::create_dir(tree.path("actual-dir")).unwrap();
    fs::write(tree.path("actual.ts"), b"target").unwrap();
    fs::write(tree.path("actual-dir/nested.ts"), b"nested").unwrap();
    symlink(tree.path("actual.ts"), tree.path("link.ts")).unwrap();
    symlink(tree.path("actual-dir"), tree.path("link-dir")).unwrap();
    symlink(tree.path("absent.ts"), tree.path("dangling.ts")).unwrap();
    let _socket = UnixListener::bind(tree.path("compiler.sock")).unwrap();

    let host = FsCompilerHost::new(tree.root(), true).unwrap();
    assert_eq!(
        host.read_file(&tree.path("link.ts")).unwrap(),
        Some(b"target".to_vec())
    );
    assert_eq!(
        host.realpath(&tree.path("link.ts")).unwrap(),
        Some(tree.path("actual.ts"))
    );
    assert_eq!(
        host.read_directory(&tree.path("link-dir")).unwrap(),
        [tree.path("link-dir/nested.ts")]
    );
    assert_eq!(host.realpath(&tree.path("dangling.ts")).unwrap(), None);
    assert!(!host.file_exists(&tree.path("dangling.ts")).unwrap());

    let entries = host.read_directory(tree.root()).unwrap();
    assert!(entries.contains(&tree.path("actual.ts")));
    assert!(entries.contains(&tree.path("actual-dir")));
    assert!(entries.contains(&tree.path("link.ts")));
    assert!(entries.contains(&tree.path("link-dir")));
    assert!(!entries.contains(&tree.path("dangling.ts")));
    assert!(!entries.contains(&tree.path("compiler.sock")));

    symlink(tree.path("loop"), tree.path("loop")).unwrap();
    let error = host.file_exists(&tree.path("loop")).unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::Other);
    assert_eq!(error.operation(), HostOperation::FileExists);
    assert_eq!(error.path(), Some(tree.path("loop").as_path()));
}

#[cfg(unix)]
#[test]
fn non_unicode_filesystem_queries_fail_closed() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let tree = TempTree::new();
    let host = FsCompilerHost::new(tree.root(), true).unwrap();
    let invalid_query = tree.root().join(OsString::from_vec(vec![0xff]));
    let error = host.read_file(&invalid_query).unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidInput);
    assert_eq!(error.operation(), HostOperation::ReadFile);
}

#[cfg(target_os = "linux")]
#[test]
fn non_unicode_directory_entries_fail_closed() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let tree = TempTree::new();
    let host = FsCompilerHost::new(tree.root(), true).unwrap();
    let invalid_query = tree.root().join(OsString::from_vec(vec![0xff]));
    fs::write(&invalid_query, b"bytes").unwrap();
    let error = host.read_directory(tree.root()).unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidData);
    assert_eq!(error.operation(), HostOperation::ReadDirectory);
    assert_eq!(error.path(), Some(invalid_query.as_path()));
}
