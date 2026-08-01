use std::path::{Path, PathBuf};

use tsc_program::{PreparationErrorKind, PreparationOperation, ProgramPath};

#[test]
fn display_and_canonical_paths_remain_distinct() {
    let path = ProgramPath::from_trusted_parts("/Work/src/../A.ts", "/work/a.ts").unwrap();

    assert_eq!(path.display(), Path::new("/Work/src/../A.ts"));
    assert_eq!(path.canonical().as_path(), Path::new("/work/a.ts"));

    let (display, canonical) = path.into_parts();
    assert_eq!(display, PathBuf::from("/Work/src/../A.ts"));
    assert_eq!(canonical.as_path(), Path::new("/work/a.ts"));
}

#[test]
fn supplied_canonical_identity_is_not_replaced_by_lexical_or_realpath_guessing() {
    let lexical =
        ProgramPath::from_trusted_parts("/Work/link/../link/a.ts", "/work/link/a.ts").unwrap();
    let physical =
        ProgramPath::from_trusted_parts("/Work/actual/a.ts", "/work/actual/a.ts").unwrap();

    assert_ne!(lexical.canonical(), physical.canonical());
    assert_eq!(lexical.display(), Path::new("/Work/link/../link/a.ts"));
}

#[test]
fn trusted_directory_unc_and_url_identities_are_not_rejected_by_guessed_grammar() {
    for (display, canonical) in [
        ("/Work/", "/work/"),
        ("//Server/Share/", "//server/share/"),
        ("file:///Work/", "file:///work/"),
    ] {
        let path = ProgramPath::from_trusted_parts(display, canonical).unwrap();
        assert_eq!(path.display(), Path::new(display));
        assert_eq!(path.canonical().as_path(), Path::new(canonical));
    }
}

#[test]
fn empty_and_nul_paths_fail_closed() {
    for result in [
        ProgramPath::from_trusted_parts("", "/work/a.ts"),
        ProgramPath::from_trusted_parts("/Work/a.ts", ""),
        ProgramPath::from_trusted_parts("/Work/a\0.ts", "/work/a.ts"),
        ProgramPath::from_trusted_parts("/Work/a.ts", "/work/a\0.ts"),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), PreparationErrorKind::InvalidInput);
        assert_eq!(error.operation(), PreparationOperation::CreateProgramPath);
    }
}

#[cfg(unix)]
#[test]
fn non_unicode_display_and_canonical_paths_fail_closed() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let invalid = PathBuf::from(OsString::from_vec(vec![b'/', 0xff]));
    for result in [
        ProgramPath::from_trusted_parts(invalid.clone(), "/work/a.ts"),
        ProgramPath::from_trusted_parts("/Work/a.ts", invalid.clone()),
    ] {
        let error = result.unwrap_err();
        assert_eq!(error.kind(), PreparationErrorKind::InvalidInput);
        assert_eq!(error.operation(), PreparationOperation::CreateProgramPath);
        assert_eq!(error.path(), Some(invalid.as_path()));
    }
}
