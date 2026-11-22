use super::*;

/// The form the backward pass replaced: the base name by two splits, then a
/// search for the dot.
fn by_splits(path: JsStr<'_>) -> bool {
    let basename = path.split_ascii(b'/').next_back().unwrap_or(path);
    let basename = basename.split_ascii(b'\\').next_back().unwrap_or(basename);
    basename.contains(".")
}

#[test]
fn the_extension_test_reads_the_last_component_alone() {
    // Every string of up to six characters over both separators, the dot, a
    // letter and a two-byte scalar: each separator order, leading and
    // trailing dots, empty components.
    let alphabet = ["/", "\\", ".", "a", "\u{e9}"];
    let mut texts = vec![String::new()];
    let mut start = 0;
    for _ in 0..6 {
        let end = texts.len();
        for index in start..end {
            for piece in alphabet {
                let mut text = texts[index].clone();
                text.push_str(piece);
                texts.push(text);
            }
        }
        start = end;
    }
    assert_eq!(texts.len(), 19_531);
    for text in &texts {
        let path = JsStr::from(text.as_str());
        assert_eq!(path_has_extension(path), by_splits(path), "{text:?}");
    }
    for (path, expected) in [
        ("/project/src/a.ts", true),
        ("/project/src.d/Makefile", false),
        ("C:\\project\\a.ts", true),
        ("C:\\project.d\\a", false),
        ("/project/.hidden", true),
        ("/project/a.", true),
        ("a", false),
        ("", false),
    ] {
        assert_eq!(path_has_extension(JsStr::from(path)), expected, "{path:?}");
    }
}
