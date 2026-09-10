use serde_json::{json, Value};
use tsc_emitter::{base64_encode, create_text_writer, NewLineKind, TextWriter};

fn writer(mode: &str) -> TextWriter {
    match mode {
        "lf" => create_text_writer(NewLineKind::LineFeed),
        "crlf" => create_text_writer(NewLineKind::CarriageReturnLineFeed),
        "single" => TextWriter::single_line(),
        _ => panic!("unknown source writer mode: {mode}"),
    }
}

fn capture(writer: &TextWriter) -> Value {
    json!({
        "text_utf16": writer.text_utf16().as_ref(),
        "utf8_base64": base64_encode(writer.text().as_bytes()),
        "utf8_bytes": writer.text().len(),
        "end_utf16": {"position": writer.text_position().value(), "line": writer.line(), "column": writer.column()},
        "indent": writer.indent(), "at_line_start": writer.is_at_start_of_line(),
        "trailing_comment": writer.has_trailing_comment(),
        "trailing_whitespace": writer.has_trailing_whitespace(),
    })
}

fn apply(writer: &mut TextWriter, action: &Value, prefer_utf8: bool) {
    let op = action["op"].as_str().unwrap();
    if let Some(units) = action["units"].as_array() {
        let units = units
            .iter()
            .map(|u| u16::try_from(u.as_u64().unwrap()).unwrap())
            .collect::<Vec<_>>();
        if let Ok(text) = String::from_utf16(&units) {
            if prefer_utf8 {
                match op {
                    "write" => writer.write(&text),
                    "rawWrite" => writer.raw_write(&text),
                    "writeLiteral" => writer.write_literal(&text),
                    "writeComment" => writer.write_comment(&text),
                    "writeKeyword" => writer.write_keyword(&text),
                    "writeOperator" => writer.write_operator(&text),
                    "writeParameter" => writer.write_parameter(&text),
                    "writeProperty" => writer.write_property(&text),
                    "writePunctuation" => writer.write_punctuation(&text),
                    "writeSpace" => writer.write_space(&text),
                    "writeStringLiteral" => writer.write_string_literal(&text),
                    "writeSymbol" => writer.write_symbol(&text),
                    "writeTrailingSemicolon" => writer.write_trailing_semicolon(&text),
                    _ => panic!("unknown source writer operation: {op}"),
                }
                return;
            }
        }
        match op {
            // These are aliases of the same `write` function in both pinned
            // source constructors. Their UTF16 route shares write_utf16.
            "write"
            | "writeKeyword"
            | "writeOperator"
            | "writeParameter"
            | "writeProperty"
            | "writePunctuation"
            | "writeSpace"
            | "writeSymbol"
            | "writeTrailingSemicolon" => writer.write_utf16(&units),
            "rawWrite" => writer.raw_write_utf16(&units),
            "writeLiteral" => writer.write_literal_utf16(&units),
            "writeComment" => writer.write_comment_utf16(&units),
            "writeStringLiteral" => writer.write_string_literal_utf16(&units),
            _ => panic!("unknown source writer operation: {op}"),
        }
    } else {
        match op {
            "writeLine" => writer.write_line(action["force"].as_bool().unwrap()),
            "increaseIndent" => writer.increase_indent(),
            "decreaseIndent" => writer.decrease_indent(),
            "clear" => writer.clear(),
            _ => panic!("unknown source writer operation: {op}"),
        }
    }
}

#[test]
fn utf16_writer_matches_typescript() {
    let artifact: Value =
        serde_json::from_slice(include_bytes!("fixtures/utf16-writer.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 48);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let mode = case["mode"].as_str().unwrap();
                let mut direct = writer(mode);
                let mut mixed = writer(mode);
                let mut steps = vec![capture(&direct)];
                for action in case["actions"].as_array().unwrap() {
                    apply(&mut direct, action, false);
                    apply(&mut mixed, action, true);
                    assert_eq!(
                        capture(&mixed),
                        capture(&direct),
                        "{id}: UTF8/UTF16 input routes"
                    );
                    let mut fork = direct.clone();
                    assert_eq!(fork, direct);
                    fork.clear();
                    assert_eq!(
                        capture(&mixed),
                        capture(&direct),
                        "{id}: clearing a clone changed its source"
                    );
                    steps.push(capture(&direct));
                }
                let actual = json!({"steps": steps});
                assert_eq!(
                    actual, case["typescript_observation"],
                    "{id} repetition {repetition}"
                );
            });
            if result.is_err() {
                failures.push(format!("{id} repetition {repetition}"));
            }
        }
    }
    assert!(failures.is_empty(), "utf16 writer failures: {failures:?}");
}

#[test]
fn writer_value_equality_survives_chunking_and_clone_clear() {
    for mode in ["lf", "crlf", "single"] {
        let mut together = writer(mode);
        together.write_utf16(&[0xd83d, 0xde00]);
        let mut separate = writer(mode);
        separate.write_utf16(&[0xd83d]);
        separate.write_utf16(&[]);
        separate.write_utf16(&[0xde00]);
        assert_eq!(together, separate);
        let fork = separate.clone();
        separate.clear();
        assert_eq!(together, fork);
        assert_ne!(fork, separate);

        let mut unpaired = writer(mode);
        unpaired.write_utf16(&[0xd800]);
        let mut replacement = writer(mode);
        replacement.write(unpaired.text());
        assert_eq!(unpaired.text(), replacement.text());
        assert_ne!(unpaired, replacement);
    }
}
