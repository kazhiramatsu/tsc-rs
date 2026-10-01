//! H2.6a / m-1 focused suite (packet §8): witness replays against the
//! frozen W-H2.6A artifact plus generator algorithm contracts. The frozen
//! oracle bytes are the entire byte expectation; nothing here authors an
//! expected map by hand. Registered as a child module of `source_map.rs`
//! (crate-internal reach; runs in the lib test target).

use serde_json::Value;
use tsc_diagnostics::JsStr;

use super::{SourceMapGenerator, SourceMappingFields};

/// Match canonical name identity, Unicode case folding and JSON's lone-unit
/// escaping through the real source-map generator.
#[test]
fn source_map_js_values_match_typescript() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-source-map-values.json")).unwrap();
    let js = |value: &Value| {
        tsc_diagnostics::JsString::from_code_units(
            &value
                .as_array()
                .unwrap()
                .iter()
                .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
                .collect::<Vec<_>>(),
        )
    };
    for case in fixture["maps"].as_array().unwrap() {
        let mut generator = SourceMapGenerator::new(
            js(&case["file"]),
            js(&case["source_root"]),
            js(&case["directory"]),
            js(&case["cwd"]),
            case["case_sensitive"].as_bool().unwrap(),
        );
        for (index, source) in case["sources"].as_array().unwrap().iter().enumerate() {
            let source = js(source);
            let source_index = generator.add_source(source.as_js());
            assert_eq!(
                source_index as u64,
                case["source_indices"][index].as_u64().unwrap(),
                "{}",
                case["case_id"]
            );
            let content = js(&case["contents"][index]);
            generator.set_source_content(source_index, Some(content.as_js()));
            generator.add_mapping(
                index as u32,
                0,
                Some(SourceMappingFields {
                    source_index,
                    source_line: index as u32,
                    source_character: 2,
                }),
                None,
            );
        }
        let expected_raw: Vec<_> = case["raw_sources"]
            .as_array()
            .unwrap()
            .iter()
            .map(js)
            .collect();
        assert_eq!(generator.raw_sources(), expected_raw, "{}", case["case_id"]);
        assert_eq!(
            generator.to_json_string(),
            case["json"].as_str().unwrap(),
            "{}",
            case["case_id"]
        );
    }
}

#[test]
fn map_url_rejects_unpaired_units_without_losing_the_original_value() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../fixtures/utf16-source-map-values.json")).unwrap();
    let lane = crate::MapLaneInputs {
        current_directory: "/project".into(),
        common_source_directory: "/project/".into(),
        use_case_sensitive_source_keys: true,
    };
    for case in fixture["uris"].as_array().unwrap() {
        let input = tsc_diagnostics::JsString::from_code_units(
            &case["input"]
                .as_array()
                .unwrap()
                .iter()
                .map(|unit| u16::try_from(unit.as_u64().unwrap()).unwrap())
                .collect::<Vec<_>>(),
        );
        let actual = crate::source_mapping_url(
            &lane,
            &Default::default(),
            "{}",
            "/project/output.js".into(),
            Some(input.as_js()),
            "/project/input.ts".into(),
        );
        if let Some(expected) = case["url"].as_str() {
            assert_eq!(actual.unwrap(), expected, "{input:?}");
        } else {
            let error = actual.unwrap_err();
            assert_eq!(
                error.to_string(),
                case["error"]["message"].as_str().unwrap()
            );
            assert!(
                matches!(error, crate::EmitFailure::MalformedSourceMapUrl { path } if path == input)
            );
        }
    }
}

/// Replay TS mapping positions to isolate bundle path/options and URL workers.
/// These are helper facets; the Bundle printer owns position/registration tests.
#[test]
fn h2_7e_bundle_declaration_map_path_lanes_match_typescript() {
    let fixture: Value = serde_json::from_slice(include_bytes!(
        "../../fixtures/bundle-declaration-map-paths.json"
    ))
    .unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 10);
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["case_id"].as_str().unwrap();
        let raw_options = &case["options"];
        let options = tsc_types::CompilerOptions {
            declaration_map: raw_options["declarationMap"].as_bool(),
            source_map: raw_options["sourceMap"].as_bool(),
            inline_source_map: raw_options["inlineSourceMap"].as_bool(),
            inline_sources: raw_options["inlineSources"].as_bool(),
            source_root: raw_options["sourceRoot"]
                .as_str()
                .map(tsc_diagnostics::JsString::from),
            map_root: raw_options["mapRoot"]
                .as_str()
                .map(tsc_diagnostics::JsString::from),
            ..Default::default()
        };
        let observed = &case["typescript_observation"];
        let writes = observed["writes"].as_array().unwrap();
        let map_path = JsStr::from(writes[0]["path"].as_str().unwrap());
        let declaration_path = JsStr::from(writes[1]["path"].as_str().unwrap());
        let map = &observed["emit_result"]["source_maps"][0];
        let frozen = map["source_map_json"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(frozen).unwrap();
        let lane = crate::MapLaneInputs {
            common_source_directory: observed["common_source_directory"].as_str().unwrap().into(),
            current_directory: case["current_directory"].as_str().unwrap().into(),
            use_case_sensitive_source_keys: case["use_case_sensitive_file_names"]
                .as_bool()
                .unwrap(),
        };
        for _ in 0..2 {
            let inputs = crate::declaration_bundle_map_recording_inputs_for(
                &lane,
                &options,
                declaration_path,
            );
            assert!(
                !inputs.inline_sources,
                "declaration options omit inlineSources"
            );
            let mut generator = SourceMapGenerator::new(
                &inputs.file,
                &inputs.source_root,
                &inputs.sources_directory_path,
                &inputs.current_directory,
                inputs.use_case_sensitive_source_keys,
            );
            for source in map["input_source_file_names"].as_array().unwrap() {
                generator.add_source(source.as_str().unwrap());
            }
            for segment in decode_mappings(parsed["mappings"].as_str().unwrap()) {
                generator.add_mapping(
                    segment.generated_line,
                    segment.generated_character,
                    segment
                        .source
                        .map(
                            |(source_index, source_line, source_character)| SourceMappingFields {
                                source_index,
                                source_line,
                                source_character,
                            },
                        ),
                    segment.name,
                );
            }
            assert_eq!(
                generator.to_json_string(),
                frozen,
                "{id}: complete path-lane map replay"
            );
            let url = crate::execute::source_mapping_url_for_output(
                &lane,
                &crate::declaration_map::map_options(&options),
                frozen,
                declaration_path,
                Some(map_path),
                None,
            )
            .unwrap();
            let text = String::from_utf8(decode_base64(
                writes[1]["callback_utf8_base64"].as_str().unwrap(),
            ))
            .unwrap();
            let utf16 = text.encode_utf16().collect::<Vec<_>>();
            let position = writes[1]["data_source_map_url_pos"].as_u64().unwrap() as usize;
            let comment = String::from_utf16(&utf16[position..]).unwrap();
            assert_eq!(
                Some(url.as_str()),
                comment.strip_prefix("//# sourceMappingURL="),
                "{id}: Bundle URL"
            );
        }
    }
}

/// RFC 4648 standard-alphabet decoder (the comment-scope witness suite's
/// local-decoder precedent; a workspace dependency is not worth twenty
/// lines).
fn decode_base64(text: &str) -> Vec<u8> {
    fn value(byte: u8) -> u32 {
        match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("unexpected base64 byte {other}"),
        }
    }
    let bytes: Vec<u8> = text.bytes().filter(|&byte| byte != b'=').collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4 + 3);
    for chunk in bytes.chunks(4) {
        let mut accumulator = 0u32;
        for &byte in chunk {
            accumulator = (accumulator << 6) | value(byte);
        }
        accumulator <<= 6 * (4 - chunk.len());
        let emitted = match chunk.len() {
            4 => 3,
            3 => 2,
            2 => 1,
            other => panic!("dangling base64 chunk of {other}"),
        };
        for index in 0..emitted {
            out.push(((accumulator >> (16 - 8 * index)) & 0xff) as u8);
        }
    }
    out
}

/// Test-only VLQ/mappings decoder — the round-trip instrument. The
/// production module deliberately carries no decoder (h2-6a.md §4.2).
#[derive(Clone, Copy, Debug)]
struct Segment {
    generated_line: u32,
    generated_character: u32,
    source: Option<(u32, u32, u32)>,
    name: Option<u32>,
}

fn decode_vlq(bytes: &[u8], cursor: &mut usize) -> i64 {
    let mut shift = 0u32;
    let mut raw = 0i64;
    loop {
        let byte = bytes[*cursor];
        *cursor += 1;
        let digit = i64::from(match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            other => panic!("invalid VLQ byte {other}"),
        });
        raw |= (digit & 31) << shift;
        if digit & 32 == 0 {
            break;
        }
        shift += 5;
    }
    if raw & 1 == 1 {
        -(raw >> 1)
    } else {
        raw >> 1
    }
}

fn decode_mappings(mappings: &str) -> Vec<Segment> {
    let bytes = mappings.as_bytes();
    let mut segments = Vec::new();
    let mut line = 0u32;
    let mut generated_character = 0i64;
    let mut source_index = 0i64;
    let mut source_line = 0i64;
    let mut source_character = 0i64;
    let mut name_index = 0i64;
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b';' => {
                line += 1;
                generated_character = 0;
                cursor += 1;
            }
            b',' => cursor += 1,
            _ => {
                generated_character += decode_vlq(bytes, &mut cursor);
                let mut source = None;
                let mut name = None;
                if cursor < bytes.len() && bytes[cursor] != b',' && bytes[cursor] != b';' {
                    source_index += decode_vlq(bytes, &mut cursor);
                    source_line += decode_vlq(bytes, &mut cursor);
                    source_character += decode_vlq(bytes, &mut cursor);
                    source = Some((
                        u32::try_from(source_index).expect("source index"),
                        u32::try_from(source_line).expect("source line"),
                        u32::try_from(source_character).expect("source character"),
                    ));
                    if cursor < bytes.len() && bytes[cursor] != b',' && bytes[cursor] != b';' {
                        name_index += decode_vlq(bytes, &mut cursor);
                        name = Some(u32::try_from(name_index).expect("name index"));
                    }
                }
                segments.push(Segment {
                    generated_line: line,
                    generated_character: u32::try_from(generated_character)
                        .expect("generated character"),
                    source,
                    name,
                });
            }
        }
    }
    segments
}

fn mappings_of(generator: &mut SourceMapGenerator) -> String {
    let json = generator.to_json_string();
    let parsed: Value = serde_json::from_str(&json).expect("generator JSON");
    parsed["mappings"].as_str().expect("mappings").to_owned()
}

fn source(
    source_index: u32,
    source_line: u32,
    source_character: u32,
) -> Option<SourceMappingFields> {
    Some(SourceMappingFields {
        source_index,
        source_line,
        source_character,
    })
}

#[test]
fn identical_position_readds_collapse() {
    let mut generator = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    generator.add_source("/project/input.ts");
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    assert_eq!(mappings_of(&mut generator), "AAAA");
}

#[test]
fn same_position_source_advance_overwrites_pending() {
    // A non-backtracking same-position re-add replaces the pending
    // mapping without committing (upstream addMapping's else branch).
    let mut generator = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    generator.add_source("/project/input.ts");
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    generator.add_mapping(0, 0, source(0, 0, 4), None);
    assert_eq!(mappings_of(&mut generator), "AAAI");
}

#[test]
fn backtracking_source_position_commits_prior_mapping() {
    let mut generator = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    generator.add_source("/project/input.ts");
    generator.add_mapping(0, 0, source(0, 0, 4), None);
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    // Same generated position, source character walks backwards: the
    // first mapping commits, then the second commits at the same
    // generated position with a negative source-character delta.
    assert_eq!(mappings_of(&mut generator), "AAAI,AAAJ");
}

#[test]
fn line_advances_emit_semicolon_runs_and_reset_character_base() {
    let mut generator = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    generator.add_source("/project/input.ts");
    generator.add_mapping(0, 5, source(0, 0, 5), None);
    generator.add_mapping(2, 5, source(0, 2, 5), None);
    // Line 0 char 5 = "KAAK"; two line advances = ";;", character base
    // resets so char 5 re-encodes as +5, source deltas continue running.
    assert_eq!(mappings_of(&mut generator), "KAAK;;KAEA");
}

#[test]
fn sourceless_mapping_after_sourced_commits_intact() {
    let mut generator = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    generator.add_source("/project/input.ts");
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    generator.add_mapping(0, 4, None, None);
    assert_eq!(mappings_of(&mut generator), "AAAA,I");
}

#[test]
fn vlq_spec_vectors() {
    fn encoded(value: i64) -> String {
        let mut generator = SourceMapGenerator::new("o.js", "", "/p", "/p", true);
        if value >= 0 {
            generator.add_mapping(0, u32::try_from(value).expect("vector"), None, None);
            mappings_of(&mut generator)
        } else {
            // Negative deltas require a source walk: encode |value| then
            // backtrack the source character by |value|.
            generator.add_source("/p/i.ts");
            generator.add_mapping(
                0,
                0,
                source(0, 0, u32::try_from(-value).expect("vector")),
                None,
            );
            generator.add_mapping(0, 1, source(0, 0, 0), None);
            let mappings = mappings_of(&mut generator);
            mappings
                .split(',')
                .nth(1)
                .expect("second segment")
                .to_owned()
        }
    }
    assert_eq!(encoded(0), "A");
    assert_eq!(encoded(1), "C");
    assert_eq!(encoded(2), "E");
    assert_eq!(encoded(15), "e");
    assert_eq!(encoded(16), "gB");
    assert_eq!(encoded(31), "+B");
    assert_eq!(encoded(32), "gC");
    assert_eq!(encoded(511), "+f");
    assert_eq!(encoded(512), "ggB");
    // Negative deltas via the backtracking lane: value encodes the
    // generated-character advance (+1 = "C") then the source triple with
    // sourceCharacter delta -N; the last VLQ field carries the sign bit.
    assert_eq!(encoded(-1), "CAAD");
    assert_eq!(encoded(-2), "CAAF");
    assert_eq!(encoded(-16), "CAAhB");
}

#[test]
fn json_escaper_matches_serde_reference() {
    // The production serializer never uses serde; serde_json is the
    // second witness for the escaper only (packet §8.4).
    let samples = [
        "plain".to_owned(),
        "with \"quotes\" and \\ backslash".to_owned(),
        "controls \u{0008}\u{0009}\u{000A}\u{000C}\u{000D} end".to_owned(),
        "other controls \u{0000}\u{0001}\u{000B}\u{001F} end".to_owned(),
        "del \u{007F} and separators \u{2028}\u{2029} raw".to_owned(),
        "non-ASCII \u{00E9}\u{30C6}\u{30B9}\u{30C8} raw".to_owned(),
    ];
    for sample in samples {
        let mut ours = String::new();
        super::push_json_string(&mut ours, &sample);
        let reference = serde_json::to_string(&sample).expect("serde string");
        assert_eq!(ours, reference, "escaper diverged for {sample:?}");
    }
}

#[test]
fn serialization_shape_key_order_and_sources_content() {
    let mut generator = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    generator.add_source("/project/input.ts");
    generator.add_mapping(0, 0, source(0, 0, 0), None);
    assert_eq!(
        generator.to_json_string(),
        "{\"version\":3,\"file\":\"out.js\",\"sourceRoot\":\"\",\"sources\":[\"input.ts\"],\"names\":[],\"mappings\":\"AAAA\"}"
    );
    // sourcesContent joins as the LAST key with null holes when set.
    let mut with_content = SourceMapGenerator::new("out.js", "", "/project", "/project", true);
    let first = with_content.add_source("/project/a.ts");
    let second = with_content.add_source("/project/b.ts");
    assert_eq!((first, second), (0, 1));
    with_content.set_source_content(1, Some("const b = 1;".into()));
    with_content.set_source_content(0, None);
    assert_eq!(
        with_content.to_json_string(),
        "{\"version\":3,\"file\":\"out.js\",\"sourceRoot\":\"\",\"sources\":[\"a.ts\",\"b.ts\"],\"names\":[],\"mappings\":\"\",\"sourcesContent\":[null,\"const b = 1;\"]}"
    );
}

#[test]
fn add_source_dedupes_on_relative_key_and_keeps_raw_order() {
    let mut generator = SourceMapGenerator::new("out.js", "", "/project/out", "/project", true);
    let first = generator.add_source("/project/src/nested/input.ts");
    let again = generator.add_source("/project/src/nested/input.ts");
    let second = generator.add_source("/project/other.ts");
    assert_eq!((first, again, second), (0, 0, 1));
    let json = generator.to_json_string();
    let parsed: Value = serde_json::from_str(&json).expect("JSON");
    let sources: Vec<&str> = parsed["sources"]
        .as_array()
        .expect("sources")
        .iter()
        .map(|value| value.as_str().expect("source"))
        .collect();
    assert_eq!(sources, ["../src/nested/input.ts", "../other.ts"]);
    let raw: Vec<&str> = generator
        .raw_sources()
        .iter()
        .map(|name| name.as_str().expect("scalar fixture source filename"))
        .collect();
    assert_eq!(raw, ["/project/src/nested/input.ts", "/project/other.ts"]);
}

#[test]
fn relativizer_cross_root_uses_file_url_prefix() {
    // Zero shared components (different roots): the absolute `to`
    // components come back with the file:// prefix on a slash root.
    let mut generator = SourceMapGenerator::new("out.js", "", "c:/work/out", "c:/work", true);
    generator.add_source("/other/root/input.ts");
    let json = generator.to_json_string();
    let parsed: Value = serde_json::from_str(&json).expect("JSON");
    assert_eq!(
        parsed["sources"][0].as_str().expect("source"),
        "file:///other/root/input.ts"
    );
}

// ---------------------------------------------------------------------------
// §8.3/§8.4 recording contracts (h2-6a-m-2): flag-gate and source-switch
// behavior asserted as segment presence/absence THROUGH the generator —
// no hand-authored map bytes. Fixtures carry a `1_0` numeric-separator
// literal: the emission plan's literal-rewrite lane marks the ancestor
// chain structured, so the identity print pipelines every node exactly
// as the production transform output does (the parsed-tree fast path
// would otherwise stop recursion above the observed records).
// ---------------------------------------------------------------------------

use tsc_program::SourceFileId;
use tsc_syntax::{parse_source_file, NodeData};

use crate::{
    create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest, PrinterOptions,
    SourceMapRange, SourceRange, TransformArena, TransformNode, TransformRoot, TransformSourceId,
};

use super::SourceMapRecordingInputs;

/// Parse `input.ts`, apply `mutate` to the parsed nodes, identity-print
/// with a recording, and return the map's sources and decoded segments.
fn print_recorded(
    source_text: &str,
    mutate: impl FnOnce(&mut TransformArena, TransformSourceId, &[TransformNode]),
) -> (Vec<String>, Vec<Segment>) {
    print_recorded_with_options(
        source_text,
        PrinterOptions::new(NewLineKind::LineFeed),
        mutate,
    )
}

fn print_recorded_with_options(
    source_text: &str,
    options: PrinterOptions,
    mutate: impl FnOnce(&mut TransformArena, TransformSourceId, &[TransformNode]),
) -> (Vec<String>, Vec<Segment>) {
    let parsed = parse_source_file("/a.ts", source_text, Default::default(), None);
    let NodeData::SourceFile(source_file) = &parsed.arena.node(parsed.root).data else {
        panic!("source file root");
    };
    let statement_ids = parsed
        .arena
        .node_array(source_file.statements.expect("top-level statements"))
        .nodes
        .to_vec();
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, Some(SourceFileId::from_raw(0)));
    let statements = statement_ids
        .into_iter()
        .map(|id| arena.node_ref(source, id).expect("top-level statement"))
        .collect::<Vec<_>>();
    mutate(&mut arena, source, &statements);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity transformation");
    let printed = create_printer(options)
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            Some(SourceMapRecordingInputs {
                file: "a.js".into(),
                source_root: "".into(),
                sources_directory_path: "/".into(),
                current_directory: "/".into(),
                use_case_sensitive_source_keys: true,
                inline_sources: false,
            }),
        )
        .expect("recorded identity print");
    let json = printed
        .source_map()
        .expect("recorded print returns a map")
        .clone()
        .to_json_string();
    let parsed_json: Value = serde_json::from_str(&json).expect("map JSON");
    let sources = parsed_json["sources"]
        .as_array()
        .expect("sources array")
        .iter()
        .map(|value| value.as_str().expect("source entry").to_owned())
        .collect();
    let segments = decode_mappings(parsed_json["mappings"].as_str().expect("mappings"));
    (sources, segments)
}

fn source_positions(segments: &[Segment]) -> Vec<(u32, u32, u32)> {
    segments
        .iter()
        .filter_map(|segment| segment.source)
        .collect()
}

fn replace_with_statement(
    arena: &mut TransformArena,
    source: TransformSourceId,
    statement: TransformNode,
) {
    let root = arena.root(source).unwrap();
    let NodeData::SourceFile(mut data) = arena.node(root).unwrap().data.clone() else {
        panic!("source file root");
    };
    data.statements = Some(
        arena
            .factory()
            .create_node_array(source, vec![statement])
            .unwrap()
            .array(),
    );
    let flags = arena.transform_flags(root);
    let updated = arena
        .factory()
        .update_node(root, NodeData::SourceFile(data), flags)
        .unwrap();
    arena.replace_root(source, updated).unwrap();
}

fn replace_with_eof_block(
    arena: &mut TransformArena,
    source: TransformSourceId,
    anchor: u32,
) -> TransformNode {
    let statements = arena
        .factory()
        .create_node_array(source, Vec::new())
        .unwrap();
    arena
        .factory()
        .set_node_array_text_range(statements, anchor, anchor)
        .unwrap();
    let block = arena
        .factory()
        .create_node(
            source,
            NodeData::Block(tsc_syntax::nodes::BlockData {
                statements: Some(statements.array()),
            }),
            crate::TransformFlags::NONE,
        )
        .unwrap();
    arena.factory().set_multi_line(block, true).unwrap();
    replace_with_statement(arena, source, block);
    block
}

#[test]
fn generated_close_brace_at_eof_records_both_sides_in_utf16() {
    for (text, anchor, line, column) in [
        ("x;\n", 2, 1, 0),
        ("x;\r\n", 2, 1, 0),
        ("x;", 2, 0, 2),
        ("//😀", 0, 0, 4),
        ("x; /*😀*/", 2, 0, 9),
        ("x; /*😀*/\n", 2, 1, 0),
    ] {
        // Keep source trivia for locating EOF, but isolate the brace's
        // records from maps for comments emitted separately by SourceFile.
        let options = PrinterOptions::new(NewLineKind::LineFeed).with_remove_comments(true);
        let (_, segments) = print_recorded_with_options(text, options, |arena, source, _| {
            replace_with_eof_block(arena, source, anchor);
        });
        assert_eq!(
            source_positions(&segments),
            [(0, line, column), (0, line, column + 1)],
            "{text:?}"
        );
        assert_eq!(
            segments
                .iter()
                .map(|segment| (segment.generated_line, segment.generated_character))
                .collect::<Vec<_>>(),
            [(1, 0), (1, 1)],
            "{text:?}: close brace generated positions",
        );
    }
}

#[test]
fn generated_eof_close_brace_keeps_token_suppression_and_override_precedence() {
    use tsc_syntax::SyntaxKind::CloseBraceToken;
    for (flags, expected) in [
        (EmitFlags::NO_TOKEN_LEADING_SOURCE_MAPS, vec![(0, 1, 1)]),
        (EmitFlags::NO_TOKEN_TRAILING_SOURCE_MAPS, vec![(0, 1, 0)]),
        (EmitFlags::NO_TOKEN_SOURCE_MAPS, vec![]),
        (EmitFlags::NO_NESTED_SOURCE_MAPS, vec![]),
    ] {
        let (_, segments) = print_recorded("x;\n", |arena, source, _| {
            let block = replace_with_eof_block(arena, source, 2);
            arena.metadata_mut(block).add_flags(flags);
        });
        assert_eq!(source_positions(&segments), expected, "{flags:?}");
    }
    for synthesized in [false, true] {
        let (_, segments) = print_recorded("x;\n", |arena, source, _| {
            let block = replace_with_eof_block(arena, source, 2);
            let range = if synthesized {
                SourceRange::Synthesized
            } else {
                SourceRange::from_raw(0, 1, arena.source(source).unwrap().syntax().positions())
                    .unwrap()
            };
            arena
                .metadata_mut(block)
                .set_token_source_map_range(CloseBraceToken, SourceMapRange::new(source, range));
        });
        let expected = if synthesized {
            vec![]
        } else {
            vec![(0, 0, 0), (0, 0, 1)]
        };
        assert_eq!(source_positions(&segments), expected);
    }
    let options =
        PrinterOptions::new(NewLineKind::LineFeed).with_omit_brace_source_map_positions(true);
    let (_, segments) = print_recorded_with_options("x;\n", options, |arena, source, _| {
        replace_with_eof_block(arena, source, 2);
    });
    assert!(source_positions(&segments).is_empty());
}

/// A token range from another file keeps its UTF-16 offsets (5..6 in b.ts,
/// past the emoji) and is located in a.ts: beyond its end, on the last line.
#[test]
fn generated_eof_close_brace_locates_a_foreign_token_range_in_the_printed_source() {
    let foreign = parse_source_file("/b.ts", "//😀\nx", Default::default(), None);
    let (sources, segments) = print_recorded("x;\n", |arena, source, _| {
        let second = arena.add_source(&foreign, Some(SourceFileId::from_raw(1)));
        let block = replace_with_eof_block(arena, source, 2);
        let range = SourceRange::from_raw(7, 8, arena.source(second).unwrap().syntax().positions())
            .unwrap();
        arena.metadata_mut(block).set_token_source_map_range(
            tsc_syntax::SyntaxKind::CloseBraceToken,
            SourceMapRange::new(second, range),
        );
    });
    assert_eq!(sources, ["a.ts"]);
    assert_eq!(source_positions(&segments), [(0, 1, 2), (0, 1, 3)]);
}

#[test]
fn synthetic_meta_properties_in_an_empty_source_have_no_token_maps() {
    for (keyword_token, name) in [
        (tsc_syntax::SyntaxKind::ImportKeyword, "meta"),
        (tsc_syntax::SyntaxKind::NewKeyword, "target"),
    ] {
        let (_, segments) = print_recorded("", |arena, source, _| {
            let name = arena.factory().create_identifier(source, name).unwrap();
            let meta = arena
                .factory()
                .create_node(
                    source,
                    NodeData::MetaProperty(tsc_syntax::nodes::MetaPropertyData {
                        keyword_token,
                        name: Some(name.node()),
                    }),
                    crate::TransformFlags::NONE,
                )
                .unwrap();
            let statement = arena
                .factory()
                .create_node(
                    source,
                    NodeData::ExpressionStatement(tsc_syntax::nodes::ExpressionStatementData {
                        expression: Some(meta.node()),
                    }),
                    crate::TransformFlags::NONE,
                )
                .unwrap();
            replace_with_statement(arena, source, statement);
        });
        assert!(
            segments.is_empty(),
            "{keyword_token:?}: synthetic tokens must not map"
        );
    }
}

#[test]
fn a_token_overflow_before_eof_does_not_create_a_map_continuation() {
    let (_, segments) = print_recorded("x;", |arena, source, _| {
        let statement = arena
            .factory()
            .create_node(
                source,
                NodeData::DebuggerStatement(tsc_syntax::nodes::DebuggerStatementData {}),
                crate::TransformFlags::NONE,
            )
            .unwrap();
        let range = SourceRange::from_raw(0, 2, arena.source(source).unwrap().syntax().positions())
            .unwrap();
        arena
            .factory()
            .set_text_range_from_source_range(statement, source, range)
            .unwrap();
        arena
            .metadata_mut(statement)
            .add_flags(EmitFlags::NO_SOURCE_MAP);
        replace_with_statement(arena, source, statement);
    });
    assert!(source_positions(&segments).is_empty());
}

const FUNCTION_FIXTURE: &str = "function f() {\n    // note\n    return 1_0;\n}\n";

fn function_body_and_return(
    arena: &TransformArena,
    function: TransformNode,
) -> (TransformNode, TransformNode) {
    let NodeData::FunctionDeclaration(data) = &arena.node(function).expect("function").data else {
        panic!("function declaration fixture");
    };
    let body = arena
        .node_ref(function.source(), data.body.expect("function body"))
        .expect("body node");
    let NodeData::Block(block) = &arena.node(body).expect("body block").data else {
        panic!("block body");
    };
    let statement_array = arena
        .node_array_ref(
            function.source(),
            block.statements.expect("body statements"),
        )
        .expect("body statement array");
    let first = arena
        .node_array(statement_array)
        .expect("body statement nodes")
        .nodes[0];
    let return_statement = arena
        .node_ref(function.source(), first)
        .expect("return statement");
    (body, return_statement)
}

/// §8.3: `NO_NESTED_SOURCE_MAPS` on a subtree suppresses the node,
/// token, and comment records inside it while the flagged node's own
/// boundaries stay live (the F5 carrier).
#[test]
fn no_nested_source_maps_suppresses_node_token_and_comment_records_inside_the_subtree() {
    let (_, baseline) = print_recorded(FUNCTION_FIXTURE, |_, _, _| {});
    let baseline = source_positions(&baseline);
    // Inner records exist to suppress: the comment (line 1), the return
    // chain (line 2), and the body close-brace token (line 3 column 0).
    assert!(baseline.iter().any(|&(_, line, _)| line == 1));
    assert!(baseline.iter().any(|&(_, line, _)| line == 2));
    assert!(baseline.contains(&(0, 3, 0)));

    let (_, suppressed) = print_recorded(FUNCTION_FIXTURE, |arena, _, statements| {
        arena
            .metadata_mut(statements[0])
            .add_flags(EmitFlags::NO_NESTED_SOURCE_MAPS);
    });
    let suppressed = source_positions(&suppressed);
    assert!(!suppressed.is_empty());
    // Only the function's own Before (0:0) and After (3:1) survive.
    assert!(suppressed.contains(&(0, 0, 0)));
    assert!(suppressed.contains(&(0, 3, 1)));
    assert!(suppressed
        .iter()
        .all(|position| [(0, 0, 0), (0, 3, 1)].contains(position)));
}

/// §8.3: `NO_LEADING_SOURCE_MAP` / `NO_TRAILING_SOURCE_MAP` suppress
/// exactly one node-boundary side, and the pair together records
/// neither (the F4 flag pairing).
#[test]
fn leading_and_trailing_source_map_flags_suppress_exactly_one_side() {
    let (_, baseline) = print_recorded(FUNCTION_FIXTURE, |_, _, _| {});
    let baseline = source_positions(&baseline);
    // The return statement's own sides (2:4 Before, 2:15 After) and its
    // literal child (2:11 / 2:14) are distinct records.
    for expected in [(0, 2, 4), (0, 2, 15), (0, 2, 11), (0, 2, 14)] {
        assert!(baseline.contains(&expected), "missing {expected:?}");
    }

    let flag_return = |flags: EmitFlags| {
        let (_, segments) = print_recorded(FUNCTION_FIXTURE, |arena, _, statements| {
            let (_, return_statement) = function_body_and_return(arena, statements[0]);
            arena.metadata_mut(return_statement).add_flags(flags);
        });
        source_positions(&segments)
    };

    let no_leading = flag_return(EmitFlags::NO_LEADING_SOURCE_MAP);
    assert!(!no_leading.contains(&(0, 2, 4)));
    assert!(no_leading.contains(&(0, 2, 15)));
    assert!(no_leading.contains(&(0, 2, 11)));

    let no_trailing = flag_return(EmitFlags::NO_TRAILING_SOURCE_MAP);
    assert!(no_trailing.contains(&(0, 2, 4)));
    assert!(!no_trailing.contains(&(0, 2, 15)));
    assert!(no_trailing.contains(&(0, 2, 14)));

    let neither = flag_return(EmitFlags::NO_LEADING_SOURCE_MAP | EmitFlags::NO_TRAILING_SOURCE_MAP);
    assert!(!neither.contains(&(0, 2, 4)));
    assert!(!neither.contains(&(0, 2, 15)));
    assert!(neither.contains(&(0, 2, 11)));
    assert!(neither.contains(&(0, 2, 14)));
}

/// §8.3: `NO_TOKEN_SOURCE_MAPS` on the token's owner gates the token
/// lane while node-boundary records stay live.
#[test]
fn no_token_source_maps_gates_the_token_lane_only() {
    let (_, baseline) = print_recorded(FUNCTION_FIXTURE, |_, _, _| {});
    let baseline = source_positions(&baseline);
    assert!(baseline.contains(&(0, 3, 0)));

    let (_, gated) = print_recorded(FUNCTION_FIXTURE, |arena, _, statements| {
        let (body, _) = function_body_and_return(arena, statements[0]);
        arena
            .metadata_mut(body)
            .add_flags(EmitFlags::NO_TOKEN_SOURCE_MAPS);
    });
    let gated = source_positions(&gated);
    // The close-brace token Before (3:0) is gone; the function's own
    // After (3:1) and the return chain records remain.
    assert!(!gated.contains(&(0, 3, 0)));
    assert!(gated.contains(&(0, 3, 1)));
    assert!(gated.contains(&(0, 2, 4)));
}

/// §8.4: a `source_map_range` from a second source records the node's
/// boundaries through the PRINTED source. `emitSourcePos(sourceMapRange.source
/// || sourceMapSource, …)` (_tsc.js:121283-121301) switches only for a range
/// that carries a source, and 6.0.3 never creates one (createSourceMapSource
/// has no caller), so a reused node keeps its own offsets and the second
/// source is never registered.
#[test]
fn a_source_map_range_from_a_second_source_records_through_the_printed_source() {
    let switch_target = parse_source_file("/b.ts", "var beta = 2;\n", Default::default(), None);
    let (sources, segments) = print_recorded("var alpha = 1_0 + 2;\n", |arena, _, statements| {
        let second = arena.add_source(&switch_target, Some(SourceFileId::from_raw(1)));
        let positions_range = {
            let syntax = arena.source(second).expect("second source").syntax();
            SourceRange::from_raw(0, 13, syntax.positions()).expect("second-source range")
        };
        // Switch the initializer literal: its generated boundaries are
        // not shared with any sibling record, so the switched records
        // survive the generator's same-generated-position collapse (a
        // switched STATEMENT's Before would be overwritten by its own
        // first child at the same generated column).
        let NodeData::VariableStatement(statement) =
            &arena.node(statements[0]).expect("variable statement").data
        else {
            panic!("variable statement fixture");
        };
        let list = arena
            .node_ref(
                statements[0].source(),
                statement.declaration_list.expect("declaration list"),
            )
            .expect("list node");
        let NodeData::VariableDeclarationList(list_data) = &arena.node(list).expect("list").data
        else {
            panic!("declaration list fixture");
        };
        let declarations = arena
            .node_array_ref(list.source(), list_data.declarations.expect("declarations"))
            .expect("declaration array");
        let first = arena.node_array(declarations).expect("declarations").nodes[0];
        let declaration = arena
            .node_ref(list.source(), first)
            .expect("declaration node");
        let NodeData::VariableDeclaration(declaration_data) =
            &arena.node(declaration).expect("declaration").data
        else {
            panic!("variable declaration fixture");
        };
        let initializer = arena
            .node_ref(
                declaration.source(),
                declaration_data.initializer.expect("initializer"),
            )
            .expect("initializer node");
        let NodeData::BinaryExpression(sum) = &arena.node(initializer).expect("sum").data else {
            panic!("binary initializer fixture");
        };
        // The left operand: records after its After side exist at later
        // generated columns, so neither switched side is collapsed by
        // the generator's same-generated-position overwrite.
        let left = arena
            .node_ref(initializer.source(), sum.left.expect("left operand"))
            .expect("left operand node");
        arena
            .metadata_mut(left)
            .set_source_map_range(SourceMapRange::new(second, positions_range));
    });
    assert_eq!(sources, ["a.ts"]);
    let positions = source_positions(&segments);
    // The switched node's After is b.ts's offset 13 located in a.ts, a
    // column no a.ts token starts or ends at.
    assert!(positions.contains(&(0, 0, 13)));
    // Unrelated records stay where they were.
    assert!(positions.contains(&(0, 0, 4)));
    assert!(positions.contains(&(0, 0, 0)));
}
