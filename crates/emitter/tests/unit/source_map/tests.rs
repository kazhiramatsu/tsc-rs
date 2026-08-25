//! H2.6a / m-1 focused suite (packet §8): witness replays against the
//! frozen W-H2.6A artifact plus generator algorithm contracts. The frozen
//! oracle bytes are the entire byte expectation; nothing here authors an
//! expected map by hand. Registered as a child module of `source_map.rs`
//! (crate-internal reach; runs in the lib test target).

use serde_json::Value;

use super::{SourceMapGenerator, SourceMappingFields};

const WITNESSES: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ratchets/h2-6a-witnesses.v1.json"
));

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

fn artifact() -> Value {
    serde_json::from_slice(WITNESSES).expect("witness artifact JSON")
}

fn write_path<'a>(observation: &'a Value, suffix: &str) -> &'a str {
    observation["writes"]
        .as_array()
        .expect("writes array")
        .iter()
        .map(|write| write["path"].as_str().expect("write path"))
        .find(|path| path.ends_with(suffix))
        .unwrap_or_else(|| panic!("no write ending in {suffix}"))
}

fn map_callback_text(observation: &Value) -> String {
    let map_write = observation["writes"]
        .as_array()
        .expect("writes array")
        .iter()
        .find(|write| {
            write["path"]
                .as_str()
                .expect("write path")
                .ends_with(".js.map")
        })
        .expect("map write");
    String::from_utf8(decode_base64(
        map_write["callback_utf8_base64"]
            .as_str()
            .expect("map write base64"),
    ))
    .expect("map write UTF-8")
}

/// Replays one frozen case through the generator alone: raw sources in
/// registration order, then every decoded mapping segment, byte-comparing
/// the serialization against the frozen `.js.map` write and the
/// `emitResult.sourceMaps` copy (one-toJSON-authority).
fn replay(case: &Value, source_root: &str, sources_directory_path: &str) {
    let case_id = case["case_id"].as_str().expect("case id");
    let observation = &case["observation"];
    let frozen = map_callback_text(observation);
    let parsed: Value = serde_json::from_str(&frozen).expect("frozen map JSON");
    let js_path = write_path(observation, ".js");
    let file = js_path
        .rsplit('/')
        .next()
        .expect("js write path has a basename");
    let raw_sources: Vec<&str> = observation["source_maps"][0]["input_source_file_names"]
        .as_array()
        .expect("input source file names")
        .iter()
        .map(|name| name.as_str().expect("source file name"))
        .collect();
    let mut generator = SourceMapGenerator::new(
        file,
        source_root,
        sources_directory_path,
        "/project",
        /* use_case_sensitive_source_keys */ true,
    );
    for raw in &raw_sources {
        generator.add_source(raw);
    }
    for segment in decode_mappings(parsed["mappings"].as_str().expect("mappings string")) {
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
    let produced = generator.to_json_string();
    assert_eq!(
        produced, frozen,
        "{case_id}: replay diverged from the map write"
    );
    assert_eq!(
        produced,
        observation["source_maps"][0]["source_map_json"]
            .as_str()
            .expect("source_map_json"),
        "{case_id}: replay diverged from emitResult.sourceMaps"
    );
    let expected_raw: Vec<&str> = raw_sources.clone();
    let recorded: Vec<&str> = generator
        .raw_sources()
        .iter()
        .map(|name| name.as_ref())
        .collect();
    assert_eq!(
        recorded, expected_raw,
        "{case_id}: raw source registration order"
    );
}

#[test]
fn witness_replays_are_byte_equal() {
    let artifact = artifact();
    let mut replayed = 0usize;
    for family in artifact["families"].as_array().expect("families") {
        for case in family["cases"].as_array().expect("cases") {
            let observation = &case["observation"];
            let has_map = observation["writes"]
                .as_array()
                .expect("writes")
                .iter()
                .any(|write| {
                    write["path"]
                        .as_str()
                        .expect("write path")
                        .ends_with(".js.map")
                });
            let lane = case["rust_lane"].as_str().expect("rust lane");
            if lane == "parity" && has_map {
                let js_path = write_path(observation, ".js");
                let directory = &js_path[..js_path.rfind('/').expect("js path directory")];
                replay(case, "", directory);
                replayed += 1;
            } else if case["case_id"] == "boundary-controls--control-sourceroot" {
                // Upstream getSourceMapDirectory: a set sourceRoot selects
                // host.getCommonSourceDirectory() (_tsc.js:116813) — the
                // single-file program's common directory is /project.
                replay(case, "src/", "/project");
                replayed += 1;
            }
        }
    }
    assert_eq!(
        replayed, 25,
        "replay census changed (24 parity maps + 1 control)"
    );
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
    with_content.set_source_content(1, Some("const b = 1;"));
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
        .map(|name| name.as_ref())
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
    let parsed = parse_source_file("/a.ts", source_text, Default::default(), None);
    let NodeData::SourceFile(source_file) = &parsed.arena.node(parsed.root).data else {
        panic!("source file root");
    };
    let statement_ids = parsed
        .arena
        .node_array(source_file.statements.expect("top-level statements"))
        .nodes
        .clone();
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
    let printed = create_printer(PrinterOptions::new(NewLineKind::LineFeed))
        .print(
            &mut result,
            PrintRequest::SourceFile(source),
            Some(SourceMapRecordingInputs {
                file: "a.js".into(),
                source_root: "".into(),
                sources_directory_path: "/".into(),
                current_directory: "/".into(),
                use_case_sensitive_source_keys: true,
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

/// §8.4: a `source_map_range` naming a second source registers it in
/// the generator and records the node's boundaries against it, while
/// unrelated records stay on the printed source (the source-switch
/// control; the full multi-source lane stays H2.6b).
#[test]
fn a_source_map_range_naming_a_second_source_registers_and_records_against_it() {
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
    assert_eq!(sources, ["a.ts", "b.ts"]);
    let positions = source_positions(&segments);
    // The switched node's boundaries record against source index 1.
    assert!(positions.contains(&(1, 0, 0)));
    assert!(positions.contains(&(1, 0, 13)));
    // Unrelated records stay on the printed source.
    assert!(positions.contains(&(0, 0, 4)));
    assert!(positions.contains(&(0, 0, 0)));
}
