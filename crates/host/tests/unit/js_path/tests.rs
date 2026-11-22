use std::borrow::Cow;
use std::path::{Path, PathBuf};

use tsc_diagnostics::{JsStr, JsString};

use super::filesystem_path;
use crate::{HostErrorKind, HostOperation};

/// The conversion without the ASCII borrow: the text validated as UTF-8, or
/// its lossy spelling when it has an unpaired surrogate.
fn by_validation(path: JsStr<'_>) -> Cow<'_, Path> {
    match path.as_str() {
        Some(path) => Cow::Borrowed(Path::new(path)),
        None => Cow::Owned(PathBuf::from(path.to_string_lossy().into_owned())),
    }
}

#[test]
fn an_ascii_query_borrows_the_same_native_path_as_the_validated_text() {
    for text in [
        "/project/src/a.ts",
        "/Users/someone/dev/node_modules/@types/node/package.json",
        "relative/./path/../a.d.ts",
        "C:\\project\\a.ts",
        "/",
        "a",
        "/project/caf\u{e9}/a.ts",
        "/project/\u{1f600}/a.ts",
    ] {
        let path = JsStr::from(text);
        let native = filesystem_path(path, HostOperation::FileExists).unwrap();
        assert_eq!(native, by_validation(path), "{text:?}");
        assert!(matches!(native, Cow::Borrowed(_)), "{text:?}");
        assert_eq!(native.as_os_str().as_encoded_bytes(), text.as_bytes());
    }
}

#[test]
fn a_query_with_an_unpaired_surrogate_keeps_its_lossy_spelling() {
    let mut text = JsString::from("/project/");
    text.push_code_unit(0xd800);
    text.push_str("/a.ts");
    let native = filesystem_path(text.as_js(), HostOperation::FileExists).unwrap();
    assert_eq!(native, by_validation(text.as_js()));
    assert!(matches!(native, Cow::Owned(_)));
    assert_eq!(native.to_str(), Some("/project/\u{fffd}/a.ts"));
}

#[test]
fn an_empty_query_and_one_with_a_nul_are_refused_before_any_conversion() {
    for text in ["", "/project/a\0.ts"] {
        let error = filesystem_path(JsStr::from(text), HostOperation::FileExists).unwrap_err();
        assert_eq!(error.kind(), HostErrorKind::InvalidInput, "{text:?}");
    }
}
