use std::collections::BTreeMap;

use super::*;

fn mappings(text: &str) -> (Vec<Mapping>, Option<String>, usize) {
    let mut decoder = MappingsDecoder::new(text);
    let mut decoded = Vec::new();
    while let Some(mapping) = decoder.next() {
        decoded.push(mapping);
    }
    (decoded, decoder.error().map(str::to_owned), decoder.pos())
}

#[test]
fn decodes_segments_lines_and_names() {
    let (decoded, error, pos) = mappings("AAAA,SAASA;AACA");
    assert_eq!(error, None);
    assert_eq!(pos, 15);
    let span = |gl, gc, si, sl, sc, ni| Mapping {
        generated_line: gl,
        generated_character: gc,
        source_index: si,
        source_line: sl,
        source_character: sc,
        name_index: ni,
    };
    assert_eq!(
        decoded,
        vec![
            span(0, 0, 0, 0, 0, MISSING),
            span(0, 9, 0, 0, 9, 0),
            span(1, 0, 0, 1, 9, MISSING),
        ]
    );
    assert!(!span(0, 2, MISSING, MISSING, MISSING, MISSING).is_source_mapping());
}

#[test]
fn decoder_reports_invalid_characters() {
    let (decoded, error, _) = mappings("AAAA,!");
    assert_eq!(decoded.len(), 1);
    assert_eq!(error.as_deref(), Some("Invalid character in VLQ"));
}

#[test]
fn line_starts_follow_every_ecma_line_break() {
    assert_eq!(compute_ecma_line_starts(""), vec![0]);
    assert_eq!(
        compute_ecma_line_starts("a\r\nb\nc\u{2028}d"),
        vec![0, 3, 5, 9]
    );
    assert_eq!(compute_ecma_line_starts("x\n"), vec![0, 2]);
}

#[test]
fn utf16_columns_map_to_byte_positions() {
    let text = "a\u{1d4b3}b";
    let starts = compute_ecma_line_starts(text);
    assert_eq!(position_of_line_and_utf16_character(&starts, 0, 1, text), 1);
    assert_eq!(position_of_line_and_utf16_character(&starts, 0, 2, text), 5);
    assert_eq!(position_of_line_and_utf16_character(&starts, 0, 3, text), 5);
    assert_eq!(position_of_line_and_utf16_character(&starts, 0, 9, text), 6);
    assert_eq!(position_of_line_and_utf16_character(&starts, 7, 0, text), 0);
}

#[test]
fn source_mapping_url_is_read_from_the_last_lines() {
    let text = "var x;\n//# sourceMappingURL=a.js.map\n\n";
    assert_eq!(
        try_get_source_mapping_url(text, &compute_ecma_line_starts(text)),
        "a.js.map"
    );
    let text = "var x;\n//# sourceMappingURL=a.js.map\nvar y;";
    assert_eq!(
        try_get_source_mapping_url(text, &compute_ecma_line_starts(text)),
        ""
    );
}

struct Files(BTreeMap<&'static str, &'static str>);

impl SourceMapperHost for Files {
    fn use_case_sensitive_file_names(&self) -> bool {
        true
    }

    fn read_file(&self, file_name: &str) -> Option<String> {
        self.0.get(file_name).map(|text| (*text).to_owned())
    }
}

/// tsgo's mapper over a declaration map: a declaration's position maps to
/// the closest mapping's source position, and back.
#[test]
fn declaration_positions_map_to_their_sources() {
    let files = Files(BTreeMap::from([
        (
            "/dist/index.d.ts",
            "export declare function f(): void;\n//# sourceMappingURL=index.d.ts.map",
        ),
        (
            "/dist/index.d.ts.map",
            r#"{"version":3,"file":"index.d.ts","sourceRoot":"","sources":["../src/index.ts"],"names":[],"mappings":"AAAA,wBAAgB,CAAC"}"#,
        ),
        ("/src/index.ts", "export function f() {}"),
    ]));
    let mapper = get_document_position_mapper(&files, "/dist/index.d.ts").expect("a mapper");
    // `f` of the declaration (column 24) is `f` of the source (column 16).
    assert_eq!(
        mapper.source_position(24),
        Some(DocumentPosition {
            file_name: "/src/index.ts".to_owned(),
            pos: 16,
        })
    );
    assert_eq!(
        mapper.generated_position("/src/index.ts", 16),
        Some(DocumentPosition {
            file_name: "/dist/index.d.ts".to_owned(),
            pos: 24,
        })
    );
    assert_eq!(mapper.source_position(1000), None);
}

#[test]
fn inline_base64_maps_are_read_and_inlined_sources_are_not() {
    let map = r#"{"version":3,"file":"a.d.ts","sources":["a.ts"],"mappings":"AAAA"}"#;
    let encoded = encode_base64(map);
    let text: &'static str = Box::leak(
        format!("declare const a: 1;\n//# sourceMappingURL=data:application/json;base64,{encoded}")
            .into_boxed_str(),
    );
    let files = Files(BTreeMap::from([
        ("/a.d.ts", text),
        ("/a.ts", "const a = 1;"),
    ]));
    let mapper = get_document_position_mapper(&files, "/a.d.ts").expect("an inline map");
    assert_eq!(
        mapper.source_position(0),
        Some(DocumentPosition {
            file_name: "/a.ts".to_owned(),
            pos: 0,
        })
    );
    let files = Files(BTreeMap::from([
        ("/b.d.ts", "declare const b: 1;"),
        (
            "/b.d.ts.map",
            r#"{"version":3,"file":"b.d.ts","sources":["b.ts"],"sourcesContent":["const b = 1;"],"mappings":"AAAA"}"#,
        ),
    ]));
    assert!(get_document_position_mapper(&files, "/b.d.ts").is_none());
}

fn encode_base64(text: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let bytes = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let joined = u32::from(bytes[0]) << 16 | u32::from(bytes[1]) << 8 | u32::from(bytes[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(joined >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
