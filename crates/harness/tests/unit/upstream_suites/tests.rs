use super::*;

#[test]
fn test_sha1_matches_standard_and_git_blob_vectors() {
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    assert_eq!(
        git_blob_sha1(b""),
        "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"
    );
}

#[test]
fn test_decode_source_matches_typescript_bom_dispatch() {
    assert_eq!(
        decode_source(b"\xef\xbb\xbfhello"),
        (SourceEncoding::Utf8Bom, "hello".to_owned())
    );
    assert_eq!(
        decode_source(b"\xff\xfeh\0i\0x"),
        (SourceEncoding::Utf16Le, "hi".to_owned())
    );
    assert_eq!(
        decode_source(b"\xfe\xff\0h\0ix"),
        (SourceEncoding::Utf16Be, "hi".to_owned())
    );
    assert_eq!(
        decode_source(b"a\xffb"),
        (SourceEncoding::Utf8, "a\u{fffd}b".to_owned())
    );
}

#[test]
fn test_project_input_resolution_is_lexical_and_confined() {
    assert_eq!(
        resolve_project_input(
            "tests/cases/projects/ReferenceResolution/src/ts/foo",
            "../../../bar/bar.ts"
        )
        .unwrap(),
        "ReferenceResolution/bar/bar.ts"
    );
    assert!(resolve_project_input("tests/cases/projects/a", "../../escape.ts").is_err());
}
