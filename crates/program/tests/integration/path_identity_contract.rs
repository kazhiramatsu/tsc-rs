#[cfg(unix)]
use std::path::PathBuf;
use tsc_diagnostics::JsString;

use tsc_program::{PreparationErrorKind, PreparationOperation, ProgramPath};

#[test]
fn display_and_canonical_paths_remain_distinct() {
    let path = ProgramPath::from_trusted_parts("/Work/src/../A.ts", "/work/a.ts").unwrap();

    assert_eq!(path.display(), "/Work/src/../A.ts");
    assert_eq!(path.canonical().as_js(), "/work/a.ts");

    let (display, canonical) = path.into_parts();
    assert_eq!(display, "/Work/src/../A.ts");
    assert_eq!(canonical.as_js(), "/work/a.ts");
}

#[test]
fn supplied_canonical_identity_is_not_replaced_by_lexical_or_realpath_guessing() {
    let lexical =
        ProgramPath::from_trusted_parts("/Work/link/../link/a.ts", "/work/link/a.ts").unwrap();
    let physical =
        ProgramPath::from_trusted_parts("/Work/actual/a.ts", "/work/actual/a.ts").unwrap();

    assert_ne!(lexical.canonical(), physical.canonical());
    assert_eq!(lexical.display(), "/Work/link/../link/a.ts");
}

#[test]
fn trusted_directory_unc_and_url_identities_are_not_rejected_by_guessed_grammar() {
    for (display, canonical) in [
        ("/Work/", "/work/"),
        ("//Server/Share/", "//server/share/"),
        ("file:///Work/", "file:///work/"),
    ] {
        let path = ProgramPath::from_trusted_parts(display, canonical).unwrap();
        assert_eq!(path.display(), display);
        assert_eq!(path.canonical().as_js(), canonical);
    }
}

#[test]
fn compiler_path_identity_distinguishes_names_with_the_same_utf8_projection() {
    let paths = [0xd800, 0xd801, 0xfffd].map(|unit| {
        let mut value = JsString::from("/work/");
        value.push_code_unit(unit);
        value.push_str(".ts");
        ProgramPath::from_js_parts(value.as_js(), value.as_js()).unwrap()
    });
    let identities = paths
        .iter()
        .map(|path| path.canonical().clone())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(identities.len(), 3);
    for (path, unit) in paths.iter().zip([0xd800, 0xd801, 0xfffd]) {
        assert_eq!(path.display().code_units().nth(6), Some(unit));
        assert_eq!(path.display().to_string_lossy(), "/work/\u{fffd}.ts");
    }
    let scalar = ProgramPath::from_trusted_parts("/Work/A.ts", "/work/a.ts").unwrap();
    assert_eq!(
        scalar,
        ProgramPath::from_js_parts("/Work/A.ts".into(), "/work/a.ts".into()).unwrap()
    );
}

#[test]
fn output_directories_preserve_distinct_js_components() {
    use tsc_program::{
        canonical_emit_path, common_source_directory, source_file_path_in_new_directory,
        CompilerOptions,
    };
    let sources = [0xd800, 0xd801, 0xfffd].map(|unit| {
        let mut path = JsString::from("/work/");
        path.push_code_unit(unit);
        path.push_str("/a.ts");
        path
    });
    let paths = sources.iter().map(JsString::as_js).collect::<Vec<_>>();
    // tsc getCommonSourceDirectory (_tsc.js:116460-116475) appends the trailing
    // separator that getSourceFilePathInNewDir strips again; the inferred
    // directory itself (computeCommonSourceDirectoryOfFilenames) carries none.
    let common = common_source_directory(
        &CompilerOptions::default(),
        None,
        &paths,
        "/work".into(),
        true,
    );
    assert_eq!(common, "/work/");
    let outputs = sources
        .iter()
        .map(|path| {
            source_file_path_in_new_directory(
                path.as_js(),
                "dist".into(),
                common.as_js(),
                "/work".into(),
                true,
            )
        })
        .collect::<Vec<_>>();
    for (output, unit) in outputs.iter().zip([0xd800, 0xd801, 0xfffd]) {
        let mut expected = "dist/".encode_utf16().collect::<Vec<_>>();
        expected.push(unit);
        expected.extend("/a.ts".encode_utf16());
        assert_eq!(output.to_utf16(), expected);
    }
    let identities = outputs
        .iter()
        .map(|path| canonical_emit_path(path.as_js(), "/work".into(), false))
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(identities.len(), 3);
}

#[test]
fn lexical_path_errors_keep_the_original_js_query() {
    use tsc_program::{normalize_absolute_js_path_lexical, ResolutionErrorKind};
    let relative = JsString::from_code_units(&[0xd800, 0x2f, 0x61]);
    let error = normalize_absolute_js_path_lexical(relative.as_js(), None).unwrap_err();
    assert_eq!(error.kind(), ResolutionErrorKind::Canonicalization);
    assert_eq!(error.js_path(), Some(relative.as_js()));
    let normalized =
        normalize_absolute_js_path_lexical(relative.as_js(), Some("/work".into())).unwrap();
    assert_eq!(
        normalized.to_utf16(),
        [
            "/work/".encode_utf16().collect::<Vec<_>>(),
            relative.to_utf16()
        ]
        .concat()
    );
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
    let invalid = JsString::from_code_units(&[0x2f, 0xd800, 0]);
    let error = ProgramPath::from_js_parts(invalid.as_js(), "/work/a.ts".into()).unwrap_err();
    assert_eq!(error.kind(), PreparationErrorKind::InvalidInput);
    assert_eq!(error.js_path(), Some(invalid.as_js()));
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
