//! Successful callback order on all current printer pipeline entry paths.
use std::{cell::RefCell, rc::Rc};
use tsc_emitter::{
    base64_encode, create_printer, transform_nodes, EmitHint, NewLineKind, PrintRequest,
    PrinterOptions, SourceFileTextMode, StandaloneWriter, TransformArena, TransformBundle,
    TransformError, TransformNode, TransformNodeArray, TransformRoot, TransformationContext,
    Transformer,
};
use tsc_syntax::{parse_json_text, parse_source_file, NodeData, SyntaxKind};

struct PhaseHooks {
    events: Rc<RefCell<Vec<serde_json::Value>>>,
    identifiers: bool,
}
impl PhaseHooks {
    fn record(
        &self,
        context: &TransformationContext,
        phase: &str,
        hint: EmitHint,
        node: TransformNode,
    ) {
        let arena = context.arena();
        let record = arena.node(node).unwrap();
        let positions = arena.source(node.source()).unwrap().syntax().positions();
        let position = |value| {
            if value == u32::MAX {
                -1
            } else {
                i64::from(positions.byte_to_utf16(value).unwrap())
            }
        };
        self.events.borrow_mut().push(serde_json::json!({"phase":phase,"hint":format!("{hint:?}"),"kind":record.kind as u16,"pos":position(record.pos),"end":position(record.end)}));
    }
}
impl Transformer for PhaseHooks {
    fn name(&self) -> &'static str {
        "direct-phase-hooks"
    }
    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        for kind in [SyntaxKind::SourceFile, SyntaxKind::ExpressionStatement]
            .into_iter()
            .chain(self.identifiers.then_some(SyntaxKind::Identifier))
        {
            context.enable_substitution(kind)?;
            context.enable_emit_notification(kind)?;
        }
        Ok(())
    }
    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        self.record(context, "substitute", hint, node);
        Ok(node)
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
fn emit_pipeline_phases_matches_typescript() {
    let artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/emit-pipeline-phases.json")).unwrap();
    assert_eq!(artifact["typescript"], "6.0.3");
    assert_eq!(artifact["route"], "direct-factory-and-printer");
    assert_eq!(artifact["repetitions"], 2);
    let cases = artifact["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 8);
    let bundle_artifact: serde_json::Value =
        serde_json::from_slice(include_bytes!("fixtures/emit-pipeline-bundle.json")).unwrap();
    assert_eq!(bundle_artifact["typescript"], "6.0.3");
    assert_eq!(bundle_artifact["route"], "direct-factory-and-printer");
    assert_eq!(bundle_artifact["repetitions"], 2);
    let bundle_cases = bundle_artifact["cases"].as_array().unwrap();
    assert_eq!(bundle_cases.len(), 2);
    let mut failures = Vec::new();
    for case in cases.iter().chain(bundle_cases) {
        let id = case["case_id"].as_str().unwrap();
        for repetition in 0..2 {
            let outcome = std::panic::catch_unwind(|| {
                let route = case["route"].as_str().unwrap();
                let text = case["text"].as_str().unwrap();
                let mut parsed = if route == "json" {
                    parse_json_text("main.json", text)
                } else {
                    parse_source_file("main.ts", text, Default::default(), None)
                };
                // TS parseJsonText does not fix up parent references. Match
                // that owned input before the immutable emit source is mounted.
                if route == "json" {
                    for id in parsed.arena.node_base()..parsed.arena.node_end() {
                        parsed.arena.node_mut(tsc_syntax::NodeId(id)).parent = None;
                    }
                }
                let mut arena = TransformArena::new();
                let source = arena.add_source(&parsed, None);
                let root = arena.root(source).unwrap();
                let NodeData::SourceFile(data) = &arena.node(root).unwrap().data else {
                    panic!("source")
                };
                let statement = arena
                    .node_array(TransformNodeArray::new(source, data.statements.unwrap()))
                    .unwrap()
                    .nodes[0];
                let node = arena.node_ref(source, statement).unwrap();
                let events = Rc::new(RefCell::new(Vec::new()));
                let hooks = PhaseHooks {
                    events: Rc::clone(&events),
                    identifiers: route == "standalone",
                };
                let mut transformation = transform_nodes(
                    arena,
                    vec![if route == "bundle" {
                        TransformRoot::Bundle(TransformBundle::new(vec![source]))
                    } else {
                        TransformRoot::SourceFile(source)
                    }],
                    vec![Box::new(hooks)],
                    false,
                )
                .unwrap();
                let mode = if route == "preserved" {
                    SourceFileTextMode::PreserveUnchanged
                } else {
                    SourceFileTextMode::Canonical
                };
                let request = if route == "standalone" {
                    PrintRequest::StandaloneNode {
                        node,
                        writer: StandaloneWriter::MultiLine,
                    }
                } else if route == "bundle" {
                    let TransformRoot::Bundle(bundle) = transformation.roots()[0].clone() else {
                        panic!("bundle root")
                    };
                    PrintRequest::Bundle(bundle)
                } else {
                    PrintRequest::SourceFile(source)
                };
                let printed = create_printer(
                    PrinterOptions::new(NewLineKind::CarriageReturnLineFeed)
                        .with_source_file_text_mode(mode),
                )
                .print(&mut transformation, request, None)
                .unwrap();
                let actual = serde_json::json!({"events":*events.borrow(),"text":printed.text(),"utf8_base64":base64_encode(printed.text().as_bytes()),"utf8_bytes":printed.text().len(),
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
        "emit pipeline phases failures: {failures:?}"
    );
}
