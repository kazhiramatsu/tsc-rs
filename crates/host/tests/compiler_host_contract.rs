use std::path::{Path, PathBuf};

use tsc_diagnostics::JsString;
use tsc_host::{CompilerHost, HostError, HostErrorKind, HostOperation, MemoryCompilerHost};

#[test]
fn memory_host_preserves_js_path_identity_in_reads_listings_and_failures() {
    let names = [
        vec![0xd800],
        vec![0xd801],
        vec![0xdc00],
        vec![0xfffd],
        vec![0xe000],
        vec![0xd83d, 0xde00],
    ];
    for case_sensitive in [true, false] {
        let mut builder = MemoryCompilerHost::builder("/Work").case_sensitive(case_sensitive);
        let mut paths = Vec::new();
        for (index, units) in names.iter().enumerate() {
            let mut path = JsString::from("/Work/");
            path.push_js(JsString::from_code_units(units).as_js());
            path.push_str("/Leaf.TS");
            builder = builder.file_js(path.clone(), vec![index as u8]);
            paths.push(path);
        }
        let first_error = HostError::new_js(
            HostErrorKind::PermissionDenied,
            HostOperation::FileExists,
            Some(paths[0].as_js()),
            "first",
        );
        let second_error = HostError::new_js(
            HostErrorKind::PermissionDenied,
            HostOperation::FileExists,
            Some(paths[1].as_js()),
            "second",
        );
        let host = builder
            .failure(first_error.clone())
            .failure(second_error.clone())
            .realpath_js(paths[0].clone(), paths[1].clone())
            .build()
            .unwrap();
        for (index, path) in paths.iter().enumerate() {
            assert_eq!(
                host.read_file_js(path.as_js()).unwrap(),
                Some(vec![index as u8])
            );
            let parent = path.as_js().strip_suffix("/Leaf.TS").unwrap();
            assert_eq!(host.read_directory_js(parent).unwrap(), [path.clone()]);
            assert!(host.directory_exists_js(parent).unwrap());
            if !case_sensitive {
                let folded = tsc_host::to_file_name_lower_case_js(path.as_js());
                assert_eq!(
                    host.read_file_js(folded.as_js()).unwrap(),
                    Some(vec![index as u8])
                );
            }
        }
        let expected_directories = [0, 1, 5, 2, 4, 3]
            .into_iter()
            .map(|index| {
                paths[index]
                    .as_js()
                    .strip_suffix("/Leaf.TS")
                    .unwrap()
                    .to_owned()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            host.get_directories_js("/Work".into()).unwrap(),
            expected_directories
        );
        assert_eq!(host.file_exists_js(paths[0].as_js()), Err(first_error));
        assert_eq!(host.file_exists_js(paths[1].as_js()), Err(second_error));
        assert_eq!(
            host.realpath_js(paths[0].as_js()).unwrap(),
            Some(paths[1].clone())
        );
    }
}

fn host_with_tree(case_sensitive: bool) -> MemoryCompilerHost {
    MemoryCompilerHost::builder("/Work")
        .case_sensitive(case_sensitive)
        .file("/Work/empty.ts", Vec::new())
        .file("/Work/src/a.ts", b"export const a = 1;".to_vec())
        .directory("/Work/types")
        .build()
        .unwrap()
}

#[test]
fn keeps_empty_files_distinct_from_missing_entries() {
    let host = host_with_tree(true);
    assert_eq!(host.current_directory().unwrap(), Path::new("/Work"));
    assert_eq!(
        host.read_file(Path::new("/Work/empty.ts")).unwrap(),
        Some(Vec::new())
    );
    assert_eq!(host.read_file(Path::new("/Work/missing.ts")).unwrap(), None);
    assert!(host.file_exists(Path::new("/Work/empty.ts")).unwrap());
    assert!(!host.file_exists(Path::new("/Work/missing.ts")).unwrap());
    assert!(host.directory_exists(Path::new("/Work/src")).unwrap());
    assert!(!host.directory_exists(Path::new("/Work/missing")).unwrap());
}

#[test]
fn lists_immediate_files_and_directories_deterministically() {
    let host = host_with_tree(true);
    assert_eq!(
        host.read_directory(Path::new("/Work")).unwrap(),
        [
            PathBuf::from("/Work/empty.ts"),
            PathBuf::from("/Work/src"),
            PathBuf::from("/Work/types"),
        ]
    );
    assert_eq!(
        host.read_directory(Path::new("/Work/src")).unwrap(),
        [PathBuf::from("/Work/src/a.ts")]
    );
    assert_eq!(
        host.get_directories(Path::new("/Work")).unwrap(),
        [PathBuf::from("/Work/src"), PathBuf::from("/Work/types")]
    );
    assert!(host
        .get_directories(Path::new("/Work/src"))
        .unwrap()
        .is_empty());
    assert!(host
        .read_directory(Path::new("/Work/missing"))
        .unwrap()
        .is_empty());
}

#[test]
fn directory_listing_uses_javascript_utf16_display_name_order() {
    for case_sensitive in [true, false] {
        let host = MemoryCompilerHost::builder("/Work")
            .case_sensitive(case_sensitive)
            .file("/Work/B.ts", Vec::new())
            .file("/Work/a.ts", Vec::new())
            .file("/Work/\u{10000}.ts", Vec::new())
            .file("/Work/\u{e000}.ts", Vec::new())
            .directory("/Work/B-dir")
            .directory("/Work/a-dir")
            .directory("/Work/\u{10000}-dir")
            .directory("/Work/\u{e000}-dir")
            .build()
            .unwrap();

        assert_eq!(
            host.read_directory(Path::new("/Work")).unwrap(),
            [
                PathBuf::from("/Work/B-dir"),
                PathBuf::from("/Work/B.ts"),
                PathBuf::from("/Work/a-dir"),
                PathBuf::from("/Work/a.ts"),
                PathBuf::from("/Work/\u{10000}-dir"),
                PathBuf::from("/Work/\u{10000}.ts"),
                PathBuf::from("/Work/\u{e000}-dir"),
                PathBuf::from("/Work/\u{e000}.ts"),
            ]
        );
        assert_eq!(
            host.get_directories(Path::new("/Work")).unwrap(),
            [
                PathBuf::from("/Work/B-dir"),
                PathBuf::from("/Work/a-dir"),
                PathBuf::from("/Work/\u{10000}-dir"),
                PathBuf::from("/Work/\u{e000}-dir"),
            ]
        );
    }
}

#[test]
fn case_profile_controls_identity_without_normalizing_paths() {
    let insensitive = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/Ä/İıß/FILE.TS", b"bytes".to_vec())
        .file("/Work/src/../literal.ts", b"literal".to_vec())
        .build()
        .unwrap();
    assert!(insensitive
        .file_exists(Path::new("/work/ä/İıß/file.ts"))
        .unwrap());
    assert!(!insensitive
        .file_exists(Path::new("/work/ä/iıss/file.ts"))
        .unwrap());
    assert!(insensitive
        .file_exists(Path::new("/work/src/../literal.ts"))
        .unwrap());
    assert!(!insensitive
        .file_exists(Path::new("/work/literal.ts"))
        .unwrap());

    let sensitive = host_with_tree(true);
    assert!(!sensitive.file_exists(Path::new("/work/empty.ts")).unwrap());
}

#[test]
fn incompatible_case_folded_collisions_fail_closed() {
    let error = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/A.ts", b"first".to_vec())
        .file("/work/a.ts", b"second".to_vec())
        .build()
        .unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::IdentityConflict);
    assert_eq!(error.operation(), HostOperation::BuildMemoryHost);

    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .file("/Work/A.ts", b"same".to_vec())
        .file("/work/a.ts", b"same".to_vec())
        .build()
        .unwrap();
    assert_eq!(
        host.read_directory(Path::new("/work")).unwrap(),
        [PathBuf::from("/Work/A.ts")]
    );
}

#[test]
fn sensitive_profile_keeps_case_distinct_and_wrong_kinds_absent() {
    let host = MemoryCompilerHost::builder("/Work")
        .file("/Work/A.ts", b"upper".to_vec())
        .file("/Work/a.ts", b"lower".to_vec())
        .directory("/Work/dir")
        .build()
        .unwrap();
    assert_eq!(
        host.read_directory(Path::new("/Work")).unwrap(),
        [
            PathBuf::from("/Work/A.ts"),
            PathBuf::from("/Work/a.ts"),
            PathBuf::from("/Work/dir"),
        ]
    );
    assert_eq!(
        host.read_file(Path::new("/Work/A.ts")).unwrap(),
        Some(b"upper".to_vec())
    );
    assert_eq!(
        host.read_file(Path::new("/Work/a.ts")).unwrap(),
        Some(b"lower".to_vec())
    );
    assert!(!host.directory_exists(Path::new("/Work/A.ts")).unwrap());
    assert!(!host.file_exists(Path::new("/Work/dir")).unwrap());
    assert_eq!(host.read_file(Path::new("/Work/dir")).unwrap(), None);
}

#[test]
fn explicit_realpaths_do_not_replace_lexical_identity() {
    let host = MemoryCompilerHost::builder("/Work")
        .file("/Work/actual/a.ts", b"same".to_vec())
        .file("/Work/link/a.ts", b"same".to_vec())
        .realpath("/Work/link/a.ts", "/Work/actual/a.ts")
        .build()
        .unwrap();
    assert_eq!(
        host.realpath(Path::new("/Work/link/a.ts")).unwrap(),
        Some(PathBuf::from("/Work/actual/a.ts"))
    );
    assert_eq!(
        host.realpath(Path::new("/Work/actual/a.ts")).unwrap(),
        Some(PathBuf::from("/Work/actual/a.ts"))
    );
    assert_eq!(host.realpath(Path::new("/Work/missing.ts")).unwrap(), None);
}

#[test]
fn host_failures_are_never_converted_to_missing() {
    let denied = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadFile,
        Some(PathBuf::from("/Work/secret.ts")),
        "denied by test host",
    );
    let host = MemoryCompilerHost::builder("/Work")
        .file("/Work/secret.ts", b"secret".to_vec())
        .failure(denied.clone())
        .build()
        .unwrap();
    assert_eq!(host.read_file(Path::new("/Work/secret.ts")), Err(denied));
    assert!(host.file_exists(Path::new("/Work/secret.ts")).unwrap());
    assert_eq!(host.read_file(Path::new("/Work/missing.ts")).unwrap(), None);
}

#[test]
fn case_insensitive_directory_realpath_and_failures_use_canonical_identity() {
    let directory_failure = HostError::new(
        HostErrorKind::PermissionDenied,
        HostOperation::ReadDirectory,
        Some(PathBuf::from("/Work/Secret")),
        "denied by test host",
    );
    let host = MemoryCompilerHost::builder("/Work")
        .case_sensitive(false)
        .directory("/Work/Actual")
        .directory("/Work/Link")
        .directory("/Work/Secret")
        .realpath("/Work/Link", "/Work/Actual")
        .failure(directory_failure.clone())
        .build()
        .unwrap();
    assert!(host.directory_exists(Path::new("/work/actual")).unwrap());
    assert_eq!(
        host.realpath(Path::new("/work/link")).unwrap(),
        Some(PathBuf::from("/Work/Actual"))
    );
    assert_eq!(
        host.read_directory(Path::new("/work/secret")),
        Err(directory_failure)
    );
}

#[test]
fn malformed_failure_and_realpath_facts_fail_during_build() {
    let malformed = HostError::new(
        HostErrorKind::Other,
        HostOperation::ReadFile,
        None,
        "unaddressable failure",
    );
    let error = MemoryCompilerHost::builder("/Work")
        .failure(malformed)
        .build()
        .unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidData);
    assert_eq!(error.operation(), HostOperation::BuildMemoryHost);

    let error = MemoryCompilerHost::builder("/Work")
        .file("/Work/link", Vec::new())
        .directory("/Work/actual")
        .realpath("/Work/link", "/Work/actual")
        .build()
        .unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidData);
    assert_eq!(error.operation(), HostOperation::BuildMemoryHost);
}

#[test]
fn null_paths_fail_closed() {
    let nul = PathBuf::from("/Work/nul\0.ts");
    let error = MemoryCompilerHost::builder("/Work")
        .file(nul.clone(), Vec::new())
        .build()
        .unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidInput);
    assert_eq!(error.operation(), HostOperation::BuildMemoryHost);
    assert_eq!(error.path(), Some(nul.as_path()));

    let host = host_with_tree(true);
    let error = host.read_file(&nul).unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidInput);
    assert_eq!(error.operation(), HostOperation::ReadFile);
    assert_eq!(error.path(), Some(nul.as_path()));
}

#[cfg(unix)]
#[test]
fn non_unicode_paths_fail_closed() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let invalid = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
    let error = MemoryCompilerHost::builder("/Work")
        .file(invalid.clone(), Vec::new())
        .build()
        .unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidInput);
    assert_eq!(error.operation(), HostOperation::BuildMemoryHost);
    assert_eq!(error.path(), Some(invalid.as_path()));

    let host = host_with_tree(true);
    let error = host.read_file(&invalid).unwrap_err();
    assert_eq!(error.kind(), HostErrorKind::InvalidInput);
    assert_eq!(error.operation(), HostOperation::ReadFile);
    assert_eq!(error.path(), Some(invalid.as_path()));
}

#[test]
fn compiler_host_surface_is_read_only_and_object_safe() {
    fn inspect(host: &dyn CompilerHost) {
        assert!(host.use_case_sensitive_file_names());
        assert_eq!(host.current_directory().unwrap(), Path::new("/Work"));
    }

    let host = host_with_tree(true);
    inspect(&host);
}
