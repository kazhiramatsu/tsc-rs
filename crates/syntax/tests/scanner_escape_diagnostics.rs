//! Scanner escape diagnostics after the implementation-review fix round
//! (A-7, lane A2 F1–F3): end of text after a backslash inside a tagged
//! template, extended escapes whose value overflows, and identifier escapes
//! that are peeked but not consumed. The fixture is pinned TypeScript's parse
//! diagnostics, identifier texts in source order and statement counts.
use serde_json::{json, Value};
use tsc_syntax::{for_each_child, NodeData, ParseOptions};

#[test]
fn scanner_escape_diagnostics_follow_typescript() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/utf16-scanner-escape-diagnostics.json"
    ))
    .unwrap();
    assert_eq!(fixture["repetitions"], 2);
    assert_eq!(fixture["typescript"], "6.0.3");
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let case_id = case["case_id"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let parsed = tsc_syntax::parse_source_file("a.ts", source, ParseOptions::default(), None);
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
        let mut identifiers = Vec::new();
        let mut stack = vec![parsed.root];
        while let Some(id) = stack.pop() {
            let node = parsed.arena.node(id);
            let text = match &node.data {
                NodeData::Identifier(identifier) => Some(identifier.text.as_str()),
                NodeData::PrivateIdentifier(identifier) => Some(identifier.text.as_str()),
                _ => None,
            };
            if let Some(text) = text {
                identifiers.push(json!({
                    "text_utf16": text.encode_utf16().collect::<Vec<u16>>(),
                    "pos": utf16(node.pos), "end": utf16(node.end),
                    "missing": node.pos == node.end,
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
        let NodeData::SourceFile(root) = &parsed.arena.node(parsed.root).data else {
            panic!("{case_id}: root is not a source file");
        };
        let statements = root.statements.map_or(0, |statements| {
            parsed.arena.node_array(statements).nodes.len()
        });
        let actual = json!({
            "diagnostics": diagnostics,
            "identifiers": identifiers,
            "statements": statements,
        });
        if actual != case["expected"] {
            failures.push(format!(
                "{case_id}:\n  expected {}\n  actual   {}",
                case["expected"], actual
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "scanner escape divergences:\n{}",
        failures.join("\n")
    );
}
