use serde_json::{json, Value};
use tsc_syntax::{for_each_child, NodeData, NodeId, ParseOptions, SourceFile};
use tsc_types::ScriptTarget;

fn fragments(source: &SourceFile, id: NodeId, output: &mut Vec<Value>) {
    let node = source.arena.node(id);
    let literal = match &node.data {
        NodeData::NoSubstitutionTemplateLiteral(data) => Some((&data.text, &data.raw_text)),
        NodeData::TemplateHead(data) => Some((&data.text, &data.raw_text)),
        NodeData::TemplateMiddle(data) => Some((&data.text, &data.raw_text)),
        NodeData::TemplateTail(data) => Some((&data.text, &data.raw_text)),
        _ => None,
    };
    if let Some((text, raw)) = literal {
        output.push(json!({
            "kind": node.kind as u16,
            "pos": source.positions().byte_to_utf16(node.pos).unwrap(),
            "end": source.positions().byte_to_utf16(node.end).unwrap(),
            "value_utf16": text.to_utf16(),
            "raw_text": raw,
            "template_flags": node.template_flags,
        }));
    } else {
        assert_eq!(node.template_flags, 0, "template flags on {:?}", node.kind);
    }
    for_each_child(&source.arena, node, |child| {
        fragments(source, child, output);
        false
    });
}

#[test]
fn parser_template_flags_match_repeated_typescript_observations() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/utf16-template-flags.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let source = tsc_syntax::parse_source_file(
            "main.ts",
            case["source"].as_str().unwrap(),
            ParseOptions {
                script_target: ScriptTarget::from_bits(fixture["target"].as_i64().unwrap() as i32),
                ..ParseOptions::default()
            },
            None,
        );
        let mut values = Vec::new();
        fragments(&source, source.root, &mut values);
        let diagnostics = source.parse_diagnostics.iter().map(|diagnostic| {
            assert!(diagnostic.message.next.is_empty());
            json!({
                "code": diagnostic.code(), "start": diagnostic.start, "length": diagnostic.length,
                "message_utf16": diagnostic.message.text.to_utf16(),
            })
        }).collect::<Vec<_>>();
        // The fixture's transform observations belong to the emitter comparison;
        // this syntax target qualifies only the explicitly separate syntax fields.
        assert_eq!(
            json!({"fragments": values, "diagnostics": diagnostics}),
            case["expected"]["syntax"],
            "{}",
            case["case_id"]
        );
    }
}
