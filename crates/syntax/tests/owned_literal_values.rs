use serde_json::{json, Value};
use tsc_syntax::{for_each_child, NodeData, NodeId, ParseOptions, SourceFile};

fn literal_values(source: &SourceFile, id: NodeId, values: &mut Vec<Value>) {
    let node = source.arena.node(id);
    let literal = match node.data() {
        NodeData::StringLiteral(data) => Some((data.text, None)),
        NodeData::NoSubstitutionTemplateLiteral(data) => Some((data.text, data.raw_text)),
        NodeData::TemplateHead(data) => Some((data.text, data.raw_text)),
        NodeData::TemplateMiddle(data) => Some((data.text, data.raw_text)),
        NodeData::TemplateTail(data) => Some((data.text, data.raw_text)),
        _ => None,
    };
    if let Some((text, raw)) = literal {
        let text = tsc_types::JsStr::from_canonical_bytes(source.arena.text(text));
        let raw = raw.map(|raw| source.arena.text_str(raw).expect("a UTF-8 raw text"));
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
            ParseOptions::default(),
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
