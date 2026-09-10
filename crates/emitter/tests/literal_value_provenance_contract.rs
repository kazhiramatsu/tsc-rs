use serde_json::{json, Value};
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, NewLineKind, PrintRequest,
    PrinterOptions, StandaloneWriter, TransformArena, TransformNode, TransformRoot,
    TransformSourceId,
};
use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};

#[derive(Clone, Copy)]
enum LiteralKind {
    String,
    Template,
}

fn units(value: &Value) -> Vec<u16> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u16::try_from(u.as_u64().unwrap()).unwrap())
        .collect()
}

fn make_literal(
    arena: &mut TransformArena,
    source: TransformSourceId,
    case: &Value,
    kind: LiteralKind,
    original: bool,
) -> TransformNode {
    let text = units(&case[if original { "origin_units" } else { "units" }]);
    match kind {
        LiteralKind::String => arena
            .factory()
            .create_string_literal_from_code_units(
                source,
                &text,
                case[if original {
                    "origin_single_quote"
                } else {
                    "single_quote"
                }]
                .as_bool()
                .unwrap(),
            )
            .unwrap(),
        LiteralKind::Template => {
            let kind = match case["kind"].as_str().unwrap() {
                "NoSubstitutionTemplateLiteral" => SyntaxKind::NoSubstitutionTemplateLiteral,
                "TemplateHead" => SyntaxKind::TemplateHead,
                "TemplateMiddle" => SyntaxKind::TemplateMiddle,
                "TemplateTail" => SyntaxKind::TemplateTail,
                other => panic!("unknown fixture template kind: {other}"),
            };
            let raw = &case[if original {
                "origin_raw_units"
            } else {
                "raw_units"
            }];
            let raw = (!raw.is_null()).then(|| units(raw));
            arena
                .factory()
                .create_template_literal_like_from_code_units(source, kind, &text, raw.as_deref())
                .unwrap()
        }
    }
}

fn state(
    arena: &TransformArena,
    node: TransformNode,
    origin: TransformNode,
    created: TransformNode,
    kind: LiteralKind,
) -> Value {
    let record = arena.node(node).unwrap();
    let metadata = arena.metadata(node);
    let properties = arena.literal_properties(node).unwrap();
    assert_eq!((record.pos, record.end), (u32::MAX, u32::MAX));
    let original = metadata
        .and_then(tsc_emitter::EmitMetadata::original)
        .map(|node| {
            if node == origin {
                "origin"
            } else if node == created {
                "created"
            } else {
                panic!("unexpected original identity")
            }
        });
    let mut state = json!({"kind": record.kind as u16, "pos": -1, "end": -1, "flags": record.flags,
        "transform_flags": arena.transform_flags(node).bits(), "emit_flags": metadata.map_or(0, |value| value.flags().bits()),
        "text_utf16": properties.javascript_string_value().unwrap().code_units(), "original": original});
    match kind {
        LiteralKind::String => {
            let text_source = properties.string_literal_text_source().map(|node| {
                let NodeData::Identifier(data) = &arena.node(node).unwrap().data else {
                    panic!("identifier text source")
                };
                data.text.clone()
            });
            state["single_quote"] = json!(properties.string_literal_single_quote().unwrap());
            state["text_source"] = json!(text_source);
        }
        LiteralKind::Template => {
            state["raw_text_utf16"] = json!(properties
                .raw_template_text()
                .map(|value| value.code_units()));
        }
    }
    state
}

fn check_fixture(bytes: &[u8], count: usize, kind: LiteralKind, description: &str) {
    let artifact: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), count);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let parsed = parse_source_file("main.ts", "", Default::default(), None);
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let origin = make_literal(&mut arena, source, case, kind, true);
                let created = make_literal(&mut arena, source, case, kind, false);
                if let LiteralKind::String = kind {
                    for (literal, property) in
                        [(origin, "origin_text_source"), (created, "text_source")]
                    {
                        if let Some(text) = case[property].as_str() {
                            let node = arena.factory().create_identifier(source, text).unwrap();
                            arena
                                .literal_properties_mut(literal)
                                .unwrap()
                                .set_string_literal_text_source(node);
                        }
                    }
                }
                let policy = case["policy"].as_str().unwrap();
                if policy == "node-no-ascii" {
                    arena
                        .metadata_mut(created)
                        .add_flags(EmitFlags::NO_ASCII_ESCAPING);
                }
                let operation = case["operation"].as_str().unwrap();
                if matches!(operation, "set-original" | "set-original-then-clone") {
                    arena.set_original_node(created, Some(origin)).unwrap();
                }
                let selected = if matches!(operation, "clone" | "set-original-then-clone") {
                    arena.factory().clone_node(created).unwrap()
                } else {
                    created
                };
                let tree_state = json!({"origin": state(&arena, origin, origin, created, kind),
                    "created": state(&arena, created, origin, created, kind),
                    "selected": state(&arena, selected, origin, created, kind)});
                let mut transformation = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    Vec::new(),
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::LineFeed)
                        .with_never_ascii_escape(policy == "printer-no-ascii"),
                )
                .print(
                    &mut transformation,
                    PrintRequest::StandaloneNode {
                        node: selected,
                        writer: StandaloneWriter::MultiLine,
                    },
                    None,
                )
                .unwrap();
                let actual = json!({"tree_state": tree_state, "text_utf16": printed.text_utf16().as_ref(),
                    "utf8_base64": base64_encode(printed.text().as_bytes()), "utf8_bytes": printed.text().len(),
                    "end_utf16": {"position": printed.end().position().value(), "line": printed.end().line(), "column": printed.end().column()}});
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
    assert!(failures.is_empty(), "{description} failures: {failures:?}");
}

#[test]
fn template_raw_provenance_matches_typescript() {
    check_fixture(
        include_bytes!("fixtures/template-raw-provenance.json"),
        480,
        LiteralKind::Template,
        "template raw provenance",
    );
}

#[test]
fn string_property_provenance_matches_typescript() {
    check_fixture(
        include_bytes!("fixtures/string-property-provenance.json"),
        60,
        LiteralKind::String,
        "string property provenance",
    );
}
