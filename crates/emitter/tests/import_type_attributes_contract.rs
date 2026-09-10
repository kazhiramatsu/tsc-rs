//! Complete direct attribute observations, including focused emit-hook order.
use std::{cell::RefCell, rc::Rc};
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitFlags, EmitHint, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, StandaloneWriter, TransformArena, TransformError,
    TransformNode, TransformNodeArray, TransformRoot, TransformationContext, Transformer,
};
use tsc_syntax::{parse_source_file, NodeData, SyntaxKind};

fn raw_position(arena: &TransformArena, node: TransformNode, value: u32) -> i64 {
    if value == u32::MAX {
        -1
    } else {
        i64::from(
            arena
                .source(node.source())
                .unwrap()
                .syntax()
                .positions()
                .byte_to_utf16(value)
                .unwrap(),
        )
    }
}

fn node_state(arena: &TransformArena, node: TransformNode) -> serde_json::Value {
    let record = arena.node(node).unwrap();
    let metadata = arena.metadata(node);
    serde_json::json!({"kind":record.kind as u16, "pos":raw_position(arena,node,record.pos),
        "end":raw_position(arena,node,record.end), "flags":record.flags,
        "emit_flags":metadata.map_or(EmitFlags::NONE, |m|m.flags()).bits(),
        "original_present":metadata.and_then(|m|m.original()).is_some()})
}

struct AttributeHooks {
    events: Rc<RefCell<Vec<serde_json::Value>>>,
    replacement: Option<TransformNode>,
}

impl AttributeHooks {
    fn record(
        &self,
        context: &TransformationContext,
        phase: &str,
        hint: EmitHint,
        node: TransformNode,
    ) {
        self.events
            .borrow_mut()
            .push(serde_json::json!({"phase":phase,
            "hint":format!("{hint:?}"), "node":node_state(context.arena(),node)}));
    }
}

impl Transformer for AttributeHooks {
    fn name(&self) -> &'static str {
        "direct-attribute-hooks"
    }
    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_substitution(SyntaxKind::ImportAttributes)?;
        context.enable_emit_notification(SyntaxKind::ImportAttributes)
    }
    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.record(context, "substitute", hint, node);
        Ok(self.replacement.unwrap_or(node))
    }
    fn before_emit_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        self.record(context, "before", hint, node);
        Ok(())
    }
    fn after_emit_node(
        &mut self,
        context: &TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        self.record(context, "after", hint, node);
        Ok(())
    }
}

#[test]
fn import_type_attributes_matches_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/import-type-attributes.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-factory-and-printer");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 84);
    let mut failures = Vec::new();
    for case in cases {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let outcome = std::panic::catch_unwind(|| {
                let parsed = parse_source_file(
                    "main.ts",
                    case["text"].as_str().unwrap(),
                    Default::default(),
                    None,
                );
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let NodeData::SourceFile(data) =
                    &arena.node(arena.root(source).unwrap()).unwrap().data
                else {
                    panic!("source")
                };
                let statement = arena
                    .node_array(TransformNodeArray::new(source, data.statements.unwrap()))
                    .unwrap()
                    .nodes[0];
                let NodeData::TypeAliasDeclaration(data) = &arena
                    .node(arena.node_ref(source, statement).unwrap())
                    .unwrap()
                    .data
                else {
                    panic!("type alias")
                };
                let original_type = arena.node_ref(source, data.r#type.unwrap()).unwrap();
                let NodeData::ImportType(data) = arena.node(original_type).unwrap().data.clone()
                else {
                    panic!("import type")
                };
                let original = arena.node_ref(source, data.attributes.unwrap()).unwrap();
                let NodeData::ImportAttributes(attributes_data) =
                    arena.node(original).unwrap().data.clone()
                else {
                    panic!("attributes")
                };
                let original_array =
                    TransformNodeArray::new(source, attributes_data.elements.unwrap());
                let mode = case["mode"].as_str().unwrap();
                let attributes = match mode {
                    "parsed" => original,
                    "clone" => arena.factory().clone_node(original).unwrap(),
                    "created" | "ranged-created" => {
                        let node = arena
                            .factory()
                            .create_import_attributes(
                                source,
                                original_array,
                                attributes_data.multi_line,
                                Some(attributes_data.token),
                            )
                            .unwrap();
                        if mode == "ranged-created" {
                            arena.factory().set_text_range(node, original).unwrap();
                        }
                        node
                    }
                    _ => unreachable!(),
                };
                let flags =
                    EmitFlags::from_bits(u32::try_from(case["flags"].as_u64().unwrap()).unwrap());
                if !flags.is_empty() {
                    arena.metadata_mut(attributes).set_flags(flags);
                }
                let argument = arena.node_ref(source, data.argument.unwrap()).unwrap();
                let qualifier = data.qualifier.map(|id| arena.node_ref(source, id).unwrap());
                let type_arguments = data
                    .type_arguments
                    .map(|id| TransformNodeArray::new(source, id));
                let node = arena
                    .factory()
                    .update_import_type_node(
                        original_type,
                        argument,
                        Some(attributes),
                        qualifier,
                        type_arguments,
                        data.is_type_of,
                    )
                    .unwrap();
                let replacement = if case["replace"].as_bool().unwrap() {
                    let children = arena
                        .node_array(original_array)
                        .unwrap()
                        .nodes
                        .iter()
                        .rev()
                        .map(|id| arena.node_ref(source, *id).unwrap())
                        .collect();
                    let members = arena.factory().create_node_array(source, children).unwrap();
                    let replacement = arena
                        .factory()
                        .create_import_attributes(
                            source,
                            members,
                            Some(false),
                            Some(attributes_data.token),
                        )
                        .unwrap();
                    if !flags.is_empty() {
                        arena.metadata_mut(replacement).set_flags(flags);
                    }
                    Some(replacement)
                } else {
                    None
                };
                let attribute_state = |node| {
                    let NodeData::ImportAttributes(data) = &arena.node(node).unwrap().data else {
                        panic!("attributes")
                    };
                    let array = TransformNodeArray::new(source, data.elements.unwrap());
                    let record = arena.node_array(array).unwrap();
                    let elements = record
                        .nodes
                        .iter()
                        .map(|id| node_state(&arena, arena.node_ref(source, *id).unwrap()))
                        .collect::<Vec<_>>();
                    let mut state = node_state(&arena, node);
                    state["multi_line"] = serde_json::json!(data.multi_line);
                    state["token"] = serde_json::json!(data.token as u16);
                    state["members"] = serde_json::json!({"same_original":array==original_array,"pos":raw_position(&arena,node,record.pos),"end":raw_position(&arena,node,record.end),"has_trailing_comma":record.has_trailing_comma,"elements":elements});
                    state
                };
                let tree_state = serde_json::json!({"parent":node_state(&arena,node),"attributes":attribute_state(attributes),"replacement":replacement.map(attribute_state)});
                let events = Rc::new(RefCell::new(Vec::new()));
                let hooks = AttributeHooks {
                    events: Rc::clone(&events),
                    replacement,
                };
                let mut transformation = transform_nodes(
                    arena,
                    vec![TransformRoot::SourceFile(source)],
                    vec![Box::new(hooks)],
                    false,
                )
                .unwrap();
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_source_file_text_mode(SourceFileTextMode::Canonical)
                        .with_declaration_syntax(true)
                        .with_only_print_js_doc_style(
                            case["only_print_js_doc_style"].as_bool().unwrap_or(false),
                        )
                        .with_remove_comments(case["remove_comments"].as_bool().unwrap_or(false)),
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
                let actual = serde_json::json!({"tree_state":tree_state,"events":*events.borrow(),"text":printed.text(),"utf8_base64":base64_encode(printed.text().as_bytes()),"utf8_bytes":printed.text().len(),
                    "end_utf16":{"position":printed.end().position().value(),"line":printed.end().line(),"column":printed.end().column()}});
                assert_eq!(
                    actual, case["typescript_observation"],
                    "{id} repetition {repetition}"
                );
            });
            if outcome.is_err() {
                failures.push(format!("{id} repetition {repetition}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "import type attributes failures: {failures:?}"
    );
}
