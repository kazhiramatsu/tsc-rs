
use super::*;

#[test]
fn virtual_normalization_keeps_scalar_corpus_semantics_and_utf16_identity() {
    for (base, path) in [
        ("/work", "a.ts"),
        (r"\work\src", r"..\a.ts"),
        ("/work", "/../../a.ts"),
        ("relative", "a.ts"),
        ("/work", "bad\0.ts"),
        ("/work/", "./a//b/../c.ts"),
    ] {
        let scalar = super::super::normalize_virtual_path(base, path);
        let js = normalize_virtual(base.into(), path.into());
        match (scalar, js) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual.as_js(), expected.as_str()),
            (Err(_), Err(_)) => {}
            outcomes => panic!("normalization boundary changed: {outcomes:?}"),
        }
    }
    let mut paths = Vec::new();
    for unit in [0xd800, 0xd801, 0xdc00, 0xfffd] {
        let mut input = JsString::from("x/../");
        input.push_js(JsString::from_code_units(&[unit]).as_js());
        input.push_str(".ts");
        let result = normalize_virtual(r"\work\src".into(), input.as_js()).unwrap();
        let mut expected = "/work/src/".encode_utf16().collect::<Vec<_>>();
        expected.push(unit);
        expected.extend(".ts".encode_utf16());
        assert_eq!(result.to_utf16(), expected);
        assert!(paths.iter().all(|previous| previous != &result));
        paths.push(result);
    }
}
