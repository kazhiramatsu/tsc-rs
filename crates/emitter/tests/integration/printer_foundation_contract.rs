use tsc_emitter::{
    create_printer, transform_nodes, DisabledSourceMapRecorder, NewLineKind, PrintRequest,
    PrinterError, PrinterOptions, SourceMapHookEvent, SourceMapHookPhase, SourceMapRecorder,
    TransformArena, TransformBundle, TransformRoot, UnsupportedEmitFeature,
};
use tsc_syntax::{parse_source_file, NodeData};

#[derive(Default)]
struct RecordingSourceMapHooks {
    events: Vec<SourceMapHookEvent>,
}

impl SourceMapRecorder for RecordingSourceMapHooks {
    fn enabled(&self) -> bool {
        true
    }

    fn record(&mut self, event: SourceMapHookEvent) {
        self.events.push(event);
    }
}

fn transformed(
    text: &str,
) -> (
    tsc_emitter::TransformationResult,
    tsc_emitter::TransformSourceId,
) {
    let parsed = parse_source_file("unicode.js", text, Default::default(), None);
    let mut arena = TransformArena::new();
    let source = arena.add_source(&parsed, None);
    let result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        Vec::new(),
        false,
    )
    .expect("identity transform");
    (result, source)
}

#[test]
fn whole_source_pipeline_preserves_text_and_observes_typed_map_hook_phases() {
    let text = "const astral = \"😀\";\nconst combining = \"e\u{301}\";\n";
    let (mut result, source) = transformed(text);
    let printer = create_printer(PrinterOptions::new(NewLineKind::LineFeed));
    let mut recorder = RecordingSourceMapHooks::default();
    let printed = printer
        .print(&mut result, PrintRequest::SourceFile(source), &mut recorder)
        .expect("whole-source print");

    assert_eq!(printed.text(), text);
    assert_eq!(
        printed.end().position().value(),
        u32::try_from(text.encode_utf16().count()).unwrap()
    );
    assert_eq!(printed.end().line(), 2);
    assert_eq!(printed.end().column(), 0);
    assert!(recorder
        .events
        .iter()
        .any(|event| event.phase() == SourceMapHookPhase::BeforeNode));
    assert!(recorder
        .events
        .iter()
        .any(|event| event.phase() == SourceMapHookPhase::AfterNode));
    assert!(recorder
        .events
        .iter()
        .any(|event| event.phase() == SourceMapHookPhase::BeforeToken));
    assert!(recorder
        .events
        .iter()
        .any(|event| event.phase() == SourceMapHookPhase::AfterToken));
    for event in &recorder.events {
        assert_eq!(event.source(), source);
        assert!(event.generated().position().value() <= printed.end().position().value());
    }
}

#[test]
fn disabled_recorder_uses_the_same_pipeline_and_dormant_roots_fail_typed() {
    let text = "export const value = 1;\n";
    let (mut result, source) = transformed(text);
    let printer = create_printer(PrinterOptions::default());
    let mut disabled = DisabledSourceMapRecorder;
    assert_eq!(
        printer
            .print(&mut result, PrintRequest::SourceFile(source), &mut disabled)
            .unwrap()
            .text(),
        text
    );

    let root = result.arena().root(source).unwrap();
    let statements = match &result.arena().node(root).unwrap().data {
        NodeData::SourceFile(data) => result
            .arena()
            .node_array_ref(source, data.statements.unwrap())
            .unwrap(),
        _ => unreachable!(),
    };
    assert_eq!(
        printer.print(
            &mut result,
            PrintRequest::StandaloneNode(root),
            &mut disabled
        ),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::StandaloneNodePrinting
        ))
    );
    assert_eq!(
        printer.print(
            &mut result,
            PrintRequest::JavaScriptMap(source),
            &mut disabled
        ),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::JavaScriptMap
        ))
    );
    assert_eq!(
        printer.print(
            &mut result,
            PrintRequest::NodeList(statements),
            &mut disabled
        ),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::NodeListPrinting
        ))
    );
    assert_eq!(
        printer.print(
            &mut result,
            PrintRequest::Bundle(TransformBundle::new(vec![source])),
            &mut disabled
        ),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::BundleRoot
        ))
    );
    assert_eq!(
        printer.print(
            &mut result,
            PrintRequest::Declaration(source),
            &mut disabled
        ),
        Err(PrinterError::Unsupported(
            UnsupportedEmitFeature::Declaration
        ))
    );
}
