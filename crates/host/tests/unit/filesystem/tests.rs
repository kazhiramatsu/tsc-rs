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
