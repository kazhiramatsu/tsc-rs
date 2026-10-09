//! The encoder against tsgo's own encodings of the sources in
//! `fixtures/tsgo/`: each `<file>.hex` holds tsgo's
//! `encoder.EncodeSourceFile` of `<file>` named `/<file>`
//! (`scripts/api_encoder_dump.py fixtures` writes them at the vendored
//! commit). Each source exercises shapes where tsgo's parser builds a tree
//! tsc's does not (heritage TypeReferences, a nested namespace's implicit
//! `export`, JSDoc comment lists, array binding holes, `A#b` link names),
//! tsgo's flags and literal token flags, and the module references its
//! parser records.

mod common;

use std::path::Path;

fn decode_hex(text: &str) -> Vec<u8> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    digits
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn encodings_match_tsgo() {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tsgo");
    let mut sources: Vec<_> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension != "hex"))
        .collect();
    sources.sort();
    assert!(sources.len() >= 12, "fixtures: {sources:?}");
    let mut failures = Vec::new();
    for source in &sources {
        let file_name = source.file_name().unwrap().to_str().unwrap();
        let text = std::fs::read_to_string(source).unwrap();
        let expected = decode_hex(
            &std::fs::read_to_string(source.with_file_name(format!("{file_name}.hex"))).unwrap(),
        );
        let actual = common::encode(&format!("/{file_name}"), &text);
        if actual != expected {
            let (category, detail) = common::difference(&actual, &expected);
            failures.push(format!("{file_name}: {category}\n    {detail}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
