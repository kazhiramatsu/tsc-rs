use std::io;

use super::map_io_error;
use crate::{HostErrorKind, HostOperation};

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
use super::is_incomplete_windows_namespace_ancestor;

#[test]
fn maps_stable_io_error_classes() {
    for (source, expected) in [
        (
            io::ErrorKind::PermissionDenied,
            HostErrorKind::PermissionDenied,
        ),
        (io::ErrorKind::InvalidInput, HostErrorKind::InvalidInput),
        (io::ErrorKind::InvalidData, HostErrorKind::InvalidData),
        (io::ErrorKind::OutOfMemory, HostErrorKind::ResourceLimit),
        (io::ErrorKind::StorageFull, HostErrorKind::ResourceLimit),
        (io::ErrorKind::FileTooLarge, HostErrorKind::ResourceLimit),
        (io::ErrorKind::QuotaExceeded, HostErrorKind::ResourceLimit),
        (io::ErrorKind::Other, HostErrorKind::Other),
    ] {
        let error = map_io_error(io::Error::from(source), HostOperation::ReadFile, None);
        assert_eq!(error.kind(), expected);
        assert_eq!(error.operation(), HostOperation::ReadFile);
    }
}

#[test]
fn io_error_retains_requested_js_identity_and_native_error_context() {
    use std::path::PathBuf;
    use tsc_diagnostics::JsString;

    let mut requested = JsString::from("/work/");
    requested.push_code_unit(0xd800);
    let native = PathBuf::from("/work/\u{fffd}");
    let observed = native.join("leaf.ts");
    let error = map_io_error(
        io::Error::from(io::ErrorKind::PermissionDenied),
        HostOperation::ReadDirectory,
        Some(observed.clone()),
    );
    let error = super::retain_query_path(error, requested.as_js(), &native);
    let mut expected = requested;
    expected.push(std::path::MAIN_SEPARATOR);
    expected.push_str("leaf.ts");
    assert_eq!(error.js_path(), Some(expected.as_js()));
    assert_eq!(error.path(), Some(observed.as_path()));
    assert_eq!(error.kind(), HostErrorKind::PermissionDenied);
    assert_eq!(error.operation(), HostOperation::ReadDirectory);
}

#[cfg(windows)]
#[test]
fn recognizes_only_incomplete_windows_namespace_ancestors() {
    assert!(is_incomplete_windows_namespace_ancestor(Path::new(
        "//?/C:"
    )));
    assert!(is_incomplete_windows_namespace_ancestor(Path::new("//?/")));
    assert!(is_incomplete_windows_namespace_ancestor(Path::new(
        r"\\.\VolumeName"
    )));
    assert!(!is_incomplete_windows_namespace_ancestor(Path::new(
        "//?/C:/"
    )));
    assert!(!is_incomplete_windows_namespace_ancestor(Path::new(
        "//?/C:/work"
    )));
    assert!(!is_incomplete_windows_namespace_ancestor(Path::new(
        "C:/work"
    )));
}

#[cfg(windows)]
#[test]
fn removes_only_verbatim_disk_realpath_prefixes() {
    use std::path::PathBuf;

    use super::normalize_windows_realpath;

    assert_eq!(
        normalize_windows_realpath(PathBuf::from(r"\\?\C:\work\a.ts")),
        PathBuf::from(r"C:\work\a.ts")
    );
    assert_eq!(
        normalize_windows_realpath(PathBuf::from(r"\\?\UNC\server\share\a.ts")),
        PathBuf::from(r"\\?\UNC\server\share\a.ts")
    );
    assert_eq!(
        normalize_windows_realpath(PathBuf::from(r"\\?\Volume{1234}\a.ts")),
        PathBuf::from(r"\\?\Volume{1234}\a.ts")
    );
}

#[test]
fn observation_lookup_checks_native_spelling_on_a_fingerprint_collision() {
    use super::{ObservationKey, ObservationLookup, ObservationQuery, Presence};
    use std::collections::HashMap;
    use std::path::Path;

    let first = ObservationQuery::new(Path::new("/work/a.ts"));
    let mut second = ObservationQuery::new(Path::new("/work/astral-\u{1f600}.ts"));
    second.fingerprint = first.fingerprint;
    let mut absent = ObservationQuery::new(Path::new("/work/missing.ts"));
    absent.fingerprint = first.fingerprint;
    let mut map = HashMap::<ObservationKey, Presence>::new();
    map.insert(first.into_owned(), Presence::File { len: 17 });
    map.insert(second.into_owned(), Presence::Directory);
    assert!(matches!(
        map.get(&first as &dyn ObservationLookup),
        Some(Presence::File { len: 17 })
    ));
    assert!(matches!(
        map.get(&second as &dyn ObservationLookup),
        Some(Presence::Directory)
    ));
    assert!(!map.contains_key(&absent as &dyn ObservationLookup));
    map.insert(first.into_owned(), Presence::Missing);
    assert_eq!(map.len(), 2);
    assert!(matches!(
        map.get(&first as &dyn ObservationLookup),
        Some(Presence::Missing)
    ));
    assert!(matches!(
        map.get(&second as &dyn ObservationLookup),
        Some(Presence::Directory)
    ));
}

#[test]
fn observation_table_preserves_path_equivalence_in_both_insertion_orders() {
    use super::{
        canonical_observation_fingerprint, lookup_observation, ObservationKey, ObservationQuery,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    let paths = [
        "",
        ".",
        "./",
        "./.",
        "a",
        "a/",
        "a//",
        "a/.",
        "./a",
        "./a/",
        "a/b",
        "a//b",
        "a/./b/",
        "a/../b",
        "a/..//b/",
        "/",
        "//",
        "/.",
        "/a/b",
        "/a//b",
        "/a/./b/",
        "/a/../b",
        "C:/work/a",
        "c:/work/a",
        "C:\\work\\a",
        "//server/share/a",
        "\\\\server\\share\\a",
        "//?/C:/a/../b",
        "\\\\?\\C:\\a\\..\\b",
    ];
    for reverse in [false, true] {
        let mut old = HashMap::<PathBuf, usize>::new();
        let mut new = HashMap::<ObservationKey, usize>::new();
        for index in 0..paths.len() {
            let at = if reverse {
                paths.len() - 1 - index
            } else {
                index
            };
            let path = Path::new(paths[at]);
            let query = ObservationQuery::new(path);
            let fingerprint = canonical_observation_fingerprint(path.as_os_str());
            new.insert(
                ObservationQuery {
                    fingerprint,
                    ..query
                }
                .into_owned(),
                at,
            );
            old.insert(path.to_owned(), at);
            assert_eq!(new.len(), old.len());
            for spelling in paths {
                let p = Path::new(spelling);
                assert_eq!(
                    lookup_observation(&new, ObservationQuery::new(p))
                        .ok()
                        .copied(),
                    old.get(p).copied(),
                    "{reverse} {index} {spelling:?}"
                );
            }
        }
    }
}

#[cfg(unix)]
#[test]
fn observation_table_keeps_non_unicode_components_lossless() {
    use super::{
        canonical_observation_fingerprint, lookup_observation, ObservationKey, ObservationQuery,
    };
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;
    let mut table = std::collections::HashMap::<ObservationKey, usize>::new();
    for (index, bytes) in [b"/work/\xff/a".as_slice(), b"/work/\xfe/a".as_slice()]
        .into_iter()
        .enumerate()
    {
        let path = Path::new(OsStr::from_bytes(bytes));
        let query = ObservationQuery::new(path);
        let fingerprint = canonical_observation_fingerprint(path.as_os_str());
        table.insert(
            ObservationQuery {
                fingerprint,
                ..query
            }
            .into_owned(),
            index,
        );
    }
    let alias = Path::new(OsStr::from_bytes(b"/work/\xff/./a/"));
    assert_eq!(
        lookup_observation(&table, ObservationQuery::new(alias)).ok(),
        Some(&0)
    );
}

#[cfg(target_pointer_width = "64")]
#[test]
fn cached_fingerprint_does_not_grow_the_owned_path_header() {
    assert_eq!(
        std::mem::size_of::<super::ObservationKey>(),
        std::mem::size_of::<std::path::PathBuf>()
    );
}
