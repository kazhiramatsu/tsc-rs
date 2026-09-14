use serde_json::{json, Value};
use tsc_syntax::{for_each_child, NodeData, ParseOptions};
use tsc_types::ScriptTarget;

use crate::{TransformArena, TransformFlags, TransformNode, TransformSourceId};

fn fragments(
    arena: &TransformArena,
    source: TransformSourceId,
    node: TransformNode,
    output: &mut Vec<TransformNode>,
) {
    let record = arena.node(node).unwrap();
    if matches!(
        record.data,
        NodeData::NoSubstitutionTemplateLiteral(_)
            | NodeData::TemplateHead(_)
            | NodeData::TemplateMiddle(_)
            | NodeData::TemplateTail(_)
    ) {
        output.push(node);
    }
    let syntax = arena.source(source).unwrap().syntax();
    for_each_child(&syntax.arena, record, |child| {
        fragments(arena, source, TransformNode::new(source, child), output);
        false
    });
}

#[test]
fn template_facets_follow_parser_flags_and_survive_factory_copies() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../syntax/tests/fixtures/utf16-template-flags.json"
    ))
    .unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let parsed = tsc_syntax::parse_source_file(
            "main.ts",
            case["source"].as_str().unwrap(),
            ParseOptions {
                script_target: ScriptTarget::ES2017,
                ..ParseOptions::default()
            },
            None,
        );
        let empty = tsc_syntax::parse_source_file("other.ts", "", Default::default(), None);
        let mut arena = TransformArena::new();
        let source = arena.add_source(&parsed, None);
        let other_source = arena.add_source(&empty, None);
        super::initialize_transform_flags(&mut arena, source).unwrap();
        let root = arena.root(source).unwrap();
        let expected = &case["expected"]["transforms"];
        // Other source facets are outside this contract; the ES2018 visitor
        // gate must reflect the template bits anywhere in the source subtree.
        assert_eq!(
            arena
                .transform_flags(root)
                .contains(TransformFlags::CONTAINS_ES_2018),
            expected["root_flags"].as_i64().unwrap() & 128 != 0,
            "{}",
            case["case_id"]
        );
        let mut nodes = Vec::new();
        fragments(&arena, source, root, &mut nodes);
        let observations = nodes
            .iter()
            .map(|&node| {
                let record = arena.node(node).unwrap();
                json!({"kind": record.kind as u16,
                "pos": parsed.positions().byte_to_utf16(record.pos).unwrap(),
                "end": parsed.positions().byte_to_utf16(record.end).unwrap(),
                "flags": arena.transform_flags(node).bits()})
            })
            .collect::<Vec<_>>();
        assert_eq!(
            json!(observations),
            expected["fragments"],
            "{}",
            case["case_id"]
        );
        for node in nodes {
            let template_flags = arena.node(node).unwrap().template_flags;
            let transform_flags = arena.transform_flags(node);
            let clone = arena.factory().clone_node(node).unwrap();
            let foreign_clone = arena
                .factory()
                .clone_node_to_source(node, other_source)
                .unwrap();
            for cloned in [clone, foreign_clone] {
                assert_eq!(arena.node(cloned).unwrap().template_flags, template_flags);
                assert_eq!(arena.transform_flags(cloned), transform_flags);
            }
        }
    }
}
