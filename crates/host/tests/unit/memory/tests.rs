use crate::to_file_name_lower_case;

#[test]
fn vendored_file_name_fold_preserves_special_code_points() {
    assert_eq!(
        to_file_name_lower_case("/W/Ä/İıß/FILE.TS"),
        "/w/ä/İıß/file.ts"
    );
    assert_eq!(to_file_name_lower_case("/W/ẞ/ΣΟΣ.TS"), "/w/ß/σος.ts");
}

#[test]
fn file_name_fold_matches_tsgo_case_insensitive_keys() {
    // tspath.ToFileNameLowerCase, with PathKey.CaseInsensitiveKey's
    // encoded-dynamic-name exception. Unicode's contextual sigma behavior
    // is pinned separately above to preserve this host's existing profile.
    for (input, expected) in [
        ("", ""),
        ("/w/already-lower_09.ts", "/w/already-lower_09.ts"),
        ("C:\\WORK\\A-Z_09.TS", "c:\\work\\a-z_09.ts"),
        ("ABCDEFGHIJKLMNOPQRSTUVWXYZ", "abcdefghijklmnopqrstuvwxyz"),
        (
            " !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~\t\r\n",
            " !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~\t\r\n",
        ),
        ("/W/Ä/İıß/FILE.TS", "/w/ä/İıß/file.ts"),
        ("/I/İ/ı/I\u{0307}/ẞ/ß.TS", "/i/İ/ı/i\u{0307}/ß/ß.ts"),
        ("^/~ts-uri~/ABC/FILE.TS", "^/~ts-uri~/ABC/FILE.TS"),
        ("^/~ts-uri~/Ä/İ/FILE.TS", "^/~ts-uri~/Ä/İ/FILE.TS"),
        ("^/~TS-URI~/FILE.TS", "^/~ts-uri~/file.ts"),
    ] {
        assert_eq!(to_file_name_lower_case(input), expected, "{input:?}");
        assert_eq!(
            crate::to_file_name_lower_case_js(input.into()),
            tsc_diagnostics::JsString::from(expected),
            "{input:?}"
        );
    }
}

#[test]
fn file_name_fold_preserves_lone_surrogates_and_dynamic_names() {
    use tsc_diagnostics::JsString;
    let path = JsString::from_code_units(&[b'A' as u16, 0xd800, b'Z' as u16, 0xdc00]);
    assert_eq!(
        crate::to_file_name_lower_case_js(path.as_js()).to_utf16(),
        [b'a' as u16, 0xd800, b'z' as u16, 0xdc00]
    );
    let mut dynamic = JsString::from("^/~ts-uri~/ABC/");
    dynamic.push_code_unit(0xd800);
    assert_eq!(crate::to_file_name_lower_case_js(dynamic.as_js()), dynamic);
}
