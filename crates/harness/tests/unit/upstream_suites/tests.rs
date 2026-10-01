use super::*;

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
