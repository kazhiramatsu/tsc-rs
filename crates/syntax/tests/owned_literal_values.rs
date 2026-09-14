use serde_json::{json, Value};
use tsc_syntax::{for_each_child, NodeData, NodeId, ParseOptions, SourceFile};
use tsc_types::ScriptTarget;

fn literal_values(source: &SourceFile, id: NodeId, values: &mut Vec<Value>) {
    let node = source.arena.node(id);
    let literal = match &node.data {
        NodeData::StringLiteral(data) => Some((&data.text, None)),
        NodeData::NoSubstitutionTemplateLiteral(data) => {
            Some((&data.text, data.raw_text.as_deref()))
        }
        NodeData::TemplateHead(data) => Some((&data.text, data.raw_text.as_deref())),
        NodeData::TemplateMiddle(data) => Some((&data.text, data.raw_text.as_deref())),
        NodeData::TemplateTail(data) => Some((&data.text, data.raw_text.as_deref())),
        _ => None,
    };
    if let Some((text, raw)) = literal {
        values.push(json!({
            "kind": node.kind as u16,
            "pos": source.positions().byte_to_utf16(node.pos).unwrap(),
            "end": source.positions().byte_to_utf16(node.end).unwrap(),
            "value_utf16": text.to_utf16(),
            "raw_text": raw,
        }));
    }
    for_each_child(&source.arena, node, |child| {
        literal_values(source, child, values);
        false
    });
}

#[test]
fn parser_owned_values_match_repeated_typescript_observations() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/utf16-owned-literal-values.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let source = tsc_syntax::parse_source_file(
            "main.ts",
            case["source"].as_str().unwrap(),
            ParseOptions {
                script_target: ScriptTarget::from_bits(case["target"].as_i64().unwrap() as i32),
                ..ParseOptions::default()
            },
            None,
        );
        let mut literals = Vec::new();
        literal_values(&source, source.root, &mut literals);
        let diagnostics = source.parse_diagnostics.iter().map(|diagnostic| {
            assert!(diagnostic.message.next.is_empty());
            json!({
                "code": diagnostic.code(), "start": diagnostic.start, "length": diagnostic.length,
                "message_utf16": diagnostic.message.text.to_utf16(),
            })
        }).collect::<Vec<_>>();
        assert_eq!(
            json!({"literals": literals, "diagnostics": diagnostics}),
            case["expected"],
            "{}",
            case["case_id"]
        );
    }
}
