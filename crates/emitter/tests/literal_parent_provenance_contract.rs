//! Exact literal spelling for positioned, cloned and unparented input trees.
use serde_json::{json, Value};
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, JavaScriptString, NewLineKind, PrintRequest,
    PrinterOptions, StandaloneWriter, TransformArena, TransformNode, TransformRoot,
};
use tsc_syntax::{parse_json_text, parse_source_file, template_text_utf16, NodeData, NodeId};
use tsc_types::NodeFlags;

fn state(arena: &TransformArena, node: TransformNode) -> Value {
    let record = arena.node(node).unwrap();
    let positions = arena.source(node.source()).unwrap().syntax().positions();
    let position = |value| {
        if value == u32::MAX {
            -1
        } else {
            i64::from(positions.byte_to_utf16(value).unwrap())
        }
    };
    let metadata = arena.metadata(node);
    let NodeData::StringLiteral(data) = &record.data else {
        panic!("literal")
    };
    let value = metadata
        .and_then(|value| value.javascript_string_value())
        .map(|value| value.code_units().to_vec())
        .unwrap_or_else(|| data.text.encode_utf16().collect());
    let parent = record.parent.map(|id| {
        let parent = arena
            .node(arena.node_ref(node.source(), id).unwrap())
            .unwrap();
        json!({"kind":parent.kind as u16,"pos":position(parent.pos),"end":position(parent.end)})
    });
    json!({"kind":record.kind as u16,"pos":position(record.pos),"end":position(record.end),
        "flags":record.flags,"emit_flags":metadata.map_or(0,|value| value.flags().bits()),
        "original_present":metadata.is_some_and(|value| value.original().is_some()),
        "parent":parent,"value_utf16":value})
}

#[test]
fn literal_parent_provenance_matches_typescript() {
    let artifact: Value = serde_json::from_slice(include_bytes!(
        "fixtures/literal-parent-provenance-utf16.json"
    ))
    .unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-factory-and-printer");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 128);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let result = std::panic::catch_unwind(|| {
                let token = case["token"].as_str().unwrap();
                let mut parsed = if case["route"] == "json" {
                    parse_json_text("main.json", token)
                } else {
                    parse_source_file("main.ts", format!("{token};"), Default::default(), None)
                };
                let NodeData::SourceFile(data) = &parsed.arena.node(parsed.root).data else {
                    panic!("source")
                };
                let statement = parsed.arena.node_array(data.statements.unwrap()).nodes[0];
                let NodeData::ExpressionStatement(data) = &parsed.arena.node(statement).data else {
                    panic!("statement")
                };
                let literal = data.expression.unwrap();
                // Project createSourceFile(setParentNodes=false) / parseJsonText
                // onto the owned test input before mounting the immutable emit
                // source. NodeData child edges and all raw ranges stay intact.
                if !case["parents"].as_bool().unwrap() {
                    for id in parsed.arena.node_base()..parsed.arena.node_end() {
                        parsed.arena.node_mut(NodeId(id)).parent = None;
                    }
                }
                let operation = case["operation"].as_str().unwrap();
                if operation == "parsed-synthesized-flag" {
                    parsed.arena.node_mut(literal).flags |= NodeFlags::SYNTHESIZED.bits();
                }
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let original = arena.node_ref(source, literal).unwrap();
                // Node.text is a JavaScript UTF16 value. Its lossless native
                // representation is the existing JavaScriptString side channel;
                // derive it from this input's raw token, never expected output.
                let NodeData::StringLiteral(data) = &arena.node(original).unwrap().data else {
                    panic!("literal")
                };
                let units = template_text_utf16(&data.text, Some(&token[1..token.len() - 1]));
                arena
                    .metadata_mut(original)
                    .set_javascript_string_value(JavaScriptString::from_code_units(units));
                let node = match operation {
                    "parsed" | "parsed-synthesized-flag" => original,
                    "clone" | "clone-ranged" => {
                        let node = arena.factory().clone_node(original).unwrap();
                        if operation == "clone-ranged" {
                            arena.factory().set_text_range(node, original).unwrap();
                        }
                        node
                    }
                    _ => unreachable!(),
                };
                let tree_state = json!({"parsed":state(&arena,original),"node":state(&arena,node),"same_parsed":node==original});
                let mut transformation = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    Vec::new(),
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_never_ascii_escape(case["never_ascii_escape"].as_bool().unwrap()),
                )
                .print(
                    &mut transformation,
                    PrintRequest::StandaloneNode {
                        node,
                        writer: StandaloneWriter::MultiLine,
                    },
                    None,
                )
                .unwrap();
                let actual = json!({"tree_state":tree_state,"text_utf16":printed.text().encode_utf16().collect::<Vec<_>>(),"utf8_base64":base64_encode(printed.text().as_bytes()),"utf8_bytes":printed.text().len(),
                    "end_utf16":{"position":printed.end().position().value(),"line":printed.end().line(),"column":printed.end().column()}});
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
    assert!(
        failures.is_empty(),
        "literal parent provenance failures: {failures:?}"
    );
}
