use serde_json::Value;
use tsc_diagnostics::PositionIndex;
use tsc_emitter::{
    create_printer, create_text_writer, transform_nodes, NewLineKind, PrintRequest, PrinterOptions,
    SourceBytePosition, SourceUtf16Location, SourceUtf16Position, TransformArena, TransformRoot,
};
use tsc_syntax::parse_source_file;

const ORACLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h1-printer-foundation.v1.json"
));

fn oracle() -> Value {
    serde_json::from_slice(ORACLE).expect("H1.2 printer oracle is valid JSON")
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} must be a string"))
}

fn number(value: &Value, key: &str) -> u32 {
    u32::try_from(
        value[key]
            .as_u64()
            .unwrap_or_else(|| panic!("{key} must be an unsigned integer")),
    )
    .unwrap_or_else(|_| panic!("{key} must fit in u32"))
}

fn flag(value: &Value, key: &str) -> bool {
    value[key]
        .as_bool()
        .unwrap_or_else(|| panic!("{key} must be a boolean"))
}

fn new_line(value: &str) -> NewLineKind {
    match value {
        "lf" => NewLineKind::LineFeed,
        "crlf" => NewLineKind::CarriageReturnLineFeed,
        other => panic!("unknown oracle newline {other}"),
    }
}

#[test]
fn text_writer_matches_vendored_unicode_and_newline_steps() {
    let oracle = oracle();
    for case in oracle["writer_cases"].as_array().unwrap() {
        let case_id = text(case, "id");
        let mut writer = create_text_writer(new_line(text(case, "new_line")));
        for step in case["steps"].as_array().unwrap() {
            let operation = &step["operation"];
            match text(operation, "kind") {
                "write" => writer.write(text(operation, "text")),
                "rawWrite" => writer.raw_write(text(operation, "text")),
                "writeLiteral" => writer.write_literal(text(operation, "text")),
                "writeKeyword" => writer.write_keyword(text(operation, "text")),
                "writeComment" => writer.write_comment(text(operation, "text")),
                "writeSpace" => writer.write_space(text(operation, "text")),
                "writeLine" => writer.write_line(flag(operation, "force")),
                "increaseIndent" => writer.increase_indent(),
                "decreaseIndent" => writer.decrease_indent(),
                "clear" => writer.clear(),
                kind => panic!("unknown writer operation {kind}"),
            }

            let state = &step["state"];
            let context = format!("{case_id} step {}", number(step, "index"));
            assert_eq!(writer.text(), text(state, "text"), "{context} text");
            assert_eq!(
                writer.text().len(),
                number(state, "text_utf8_bytes") as usize,
                "{context} UTF-8 byte length"
            );
            assert_eq!(
                writer.text_position().value(),
                number(state, "text_position_utf16"),
                "{context} UTF-16 text position"
            );
            assert_eq!(writer.line(), number(state, "line"), "{context} line");
            assert_eq!(
                writer.column(),
                number(state, "column_utf16"),
                "{context} UTF-16 column"
            );
            assert_eq!(writer.indent(), number(state, "indent"), "{context} indent");
            assert_eq!(
                writer.is_at_start_of_line(),
                flag(state, "at_start_of_line"),
                "{context} line-start state"
            );
            assert_eq!(
                writer.has_trailing_comment(),
                flag(state, "has_trailing_comment"),
                "{context} trailing-comment state"
            );
            assert_eq!(
                writer.has_trailing_whitespace(),
                flag(state, "has_trailing_whitespace"),
                "{context} trailing-whitespace state"
            );
        }
    }
}

#[test]
fn source_byte_to_utf16_locations_match_vendored_line_maps() {
    let oracle = oracle();
    for case in oracle["source_position_cases"].as_array().unwrap() {
        let case_id = text(case, "id");
        let source_text = text(case, "text");
        let positions = PositionIndex::new_static(source_text);
        assert_eq!(positions.byte_len(), number(case, "text_utf8_bytes"));
        assert_eq!(positions.utf16_len(), number(case, "text_utf16_units"));

        for expected in case["positions"].as_array().unwrap() {
            let label = text(expected, "label");
            let byte =
                SourceBytePosition::new(number(expected, "source_byte_position"), &positions)
                    .unwrap_or_else(|error| panic!("{case_id}/{label}: {error}"));
            let utf16 = SourceUtf16Position::from_byte(byte, &positions).unwrap();
            let location = SourceUtf16Location::from_byte(byte, &positions).unwrap();
            assert_eq!(
                utf16.value(),
                number(expected, "source_utf16_position"),
                "{case_id}/{label} UTF-16 position"
            );
            assert_eq!(
                location.line(),
                number(expected, "line"),
                "{case_id}/{label} line"
            );
            assert_eq!(
                location.column(),
                number(expected, "column_utf16"),
                "{case_id}/{label} UTF-16 column"
            );
        }
    }
}

#[test]
fn whole_source_printer_coordinates_match_vendored_printer() {
    let oracle = oracle();
    for case in oracle["printer_cases"].as_array().unwrap() {
        let case_id = text(case, "id");
        let source_text = text(case, "source");
        let parsed = parse_source_file(
            text(case, "file_name"),
            source_text,
            Default::default(),
            None,
        );
        assert!(
            parsed.parse_diagnostics.is_empty(),
            "{case_id} parses cleanly"
        );
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        let mut result = transform_nodes(
            arena,
            vec![TransformRoot::SourceFile(source)],
            Vec::new(),
            false,
        )
        .expect("identity transformation");
        let mut printer = create_printer(PrinterOptions::new(new_line(text(case, "new_line"))));
        // h2-6a-m-2 §5: the identity arm's token hook-event assertions
        // are deleted with the seam (the arm records nothing under the
        // new model and is unreachable in compiler emit); the vendored
        // byte/position oracle below is unchanged.
        let printed = printer
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap_or_else(|error| panic!("{case_id}: {error}"));

        assert_eq!(printed.text(), text(case, "output"), "{case_id} output");
        assert_eq!(
            printed.end().position().value(),
            number(case, "output_utf16_units"),
            "{case_id} output end"
        );
        assert_eq!(
            printed.end().line(),
            number(&case["output_end"], "line"),
            "{case_id} output line"
        );
        assert_eq!(
            printed.end().column(),
            number(&case["output_end"], "column_utf16"),
            "{case_id} output column"
        );
    }
}
