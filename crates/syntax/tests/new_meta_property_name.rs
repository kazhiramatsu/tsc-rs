//! `new.<name>` meta-property names follow tsc parseIdentifierName: keywords
//! are accepted, any other token yields a missing identifier plus TS1003, and
//! a string/template token whose value carries an unpaired surrogate must
//! neither panic nor fabricate an identifier from the literal value.
use serde_json::{json, Value};
use tsc_syntax::{for_each_child, LanguageVariant, NodeData, ParseOptions, SyntaxKind};

#[test]
fn new_meta_property_names_follow_parse_identifier_name() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/utf16-new-meta-property-name.json")).unwrap();
    assert_eq!(fixture["repetitions"], 2);
    for case in fixture["cases"].as_array().unwrap() {
        let case_id = case["case_id"].as_str().unwrap();
        let file_name = case["file_name"].as_str().unwrap();
        let parsed = tsc_syntax::parse_source_file(
            file_name,
            case["source"].as_str().unwrap(),
            ParseOptions {
                javascript_file: file_name.ends_with(".js"),
                language_variant: if file_name.ends_with(".tsx") {
                    LanguageVariant::Jsx
                } else {
                    LanguageVariant::Standard
                },
                ..ParseOptions::default()
            },
            None,
        );
        let positions = parsed.positions();
        let utf16 = |byte: u32| positions.byte_to_utf16(byte).unwrap();
        let diagnostics = parsed
            .parse_diagnostics
            .iter()
            .map(|diagnostic| {
                assert!(diagnostic.message.next.is_empty());
                json!({
                    "code": diagnostic.code(), "start": diagnostic.start, "length": diagnostic.length,
                    "message_utf16": diagnostic.message.text.to_utf16(),
                })
            })
            .collect::<Vec<_>>();
        let mut meta_properties = Vec::new();
        let mut new_expressions = 0usize;
        let mut stack = vec![parsed.root];
        while let Some(id) = stack.pop() {
            let node = parsed.arena.node(id);
            if node.kind == SyntaxKind::NewExpression {
                new_expressions += 1;
            }
            if let NodeData::MetaProperty(data) = &node.data {
                assert_eq!(data.keyword_token, SyntaxKind::NewKeyword, "{case_id}");
                let name = parsed.arena.node(data.name.expect("meta property name"));
                let NodeData::Identifier(identifier) = &name.data else {
                    panic!("{case_id}: meta property name is not an identifier");
                };
                meta_properties.push(json!({
                    "pos": utf16(node.pos), "end": utf16(node.end),
                    "name_pos": utf16(name.pos), "name_end": utf16(name.end),
                    "name_utf16": identifier.text.encode_utf16().collect::<Vec<u16>>(),
                    "name_missing": name.pos == name.end,
                }));
            }
            let mut children = Vec::new();
            for_each_child(&parsed.arena, node, |child| {
                children.push(child);
                false
            });
            // Depth-first in source order.
            stack.extend(children.into_iter().rev());
        }
        assert_eq!(
            json!({
                "diagnostics": diagnostics,
                "meta_properties": meta_properties,
                "new_expressions": new_expressions,
            }),
            case["expected"],
            "{case_id}"
        );
    }
}
