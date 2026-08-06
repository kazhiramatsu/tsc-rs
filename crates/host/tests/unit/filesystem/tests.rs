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
