use super::*;
use crate::{transform_nodes, TransformArena, TransformRoot, TransformationContext, Transformer};
use tsc_syntax::parse_source_file;

fn fixture() -> (
    TransformArena,
    TransformSourceId,
    TransformNode,
    TransformNode,
) {
    let file = parse_source_file("event.ts", "f(g);", Default::default(), None);
    let parent = file
        .arena
        .node_ids()
        .find(|&id| file.arena.node(id).kind == SyntaxKind::CallExpression)
        .unwrap();
    let trigger = file
        .arena
        .node_ids()
        .find(|&id| {
            file.arena
                .node(id)
                .as_identifier()
                .is_some_and(|data| data.text() == "g")
        })
        .unwrap();
    let mut arena = TransformArena::new();
    let source = arena.add_source(&file, None);
    let parent = arena.node_ref(source, parent).unwrap();
    let trigger = arena.node_ref(source, trigger).unwrap();
    (arena, source, parent, trigger)
}

fn canonical_printer() -> Printer {
    create_printer(
        PrinterOptions::new(NewLineKind::LineFeed)
            .with_source_file_text_mode(SourceFileTextMode::Canonical),
    )
}

fn comment(text: &str) -> SyntheticComment {
    SyntheticComment::new(SyntheticCommentKind::MultiLine, text, false, false)
}

struct ChildChangesParent {
    parent: TransformNode,
    trigger: TransformNode,
    changed: bool,
}

impl Transformer for ChildChangesParent {
    fn name(&self) -> &'static str {
        "childChangesParent"
    }
    fn initialize(&mut self, context: &mut TransformationContext) -> Result<(), TransformError> {
        context.enable_substitution(SyntaxKind::Identifier)
    }
    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        _: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if node == self.trigger && !self.changed {
            self.changed = true;
            context
                .arena_mut()?
                .metadata_mut(self.parent)
                .add_trailing_comment(comment("late"));
        }
        Ok(node)
    }
}

#[test]
fn child_substitution_can_add_metadata_to_an_initially_empty_parent() {
    let (arena, source, parent, trigger) = fixture();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![Box::new(ChildChangesParent {
            parent,
            trigger,
            changed: false,
        })],
        false,
    )
    .unwrap();
    assert!(result.arena().metadata(parent).is_none());
    let printed = canonical_printer()
        .print(&mut result, PrintRequest::SourceFile(source), None)
        .unwrap();
    assert_eq!(printed.text(), "f(g) /*late*/;\n");
    assert_eq!(
        result
            .arena()
            .metadata(parent)
            .unwrap()
            .trailing_comments()
            .len(),
        1
    );
}

#[test]
fn zero_emit_flags_do_not_hide_present_synthetic_comment_metadata() {
    let (mut arena, source, parent, _) = fixture();
    let metadata = arena.metadata_mut(parent);
    metadata.add_leading_comment(comment("head"));
    metadata.add_trailing_comment(comment("tail"));
    assert_eq!(metadata.flags(), EmitFlags::NONE);
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    assert!(result.has_inert_print_hooks());
    assert_eq!(
        canonical_printer()
            .print(&mut result, PrintRequest::SourceFile(source), None)
            .unwrap()
            .text(),
        "/*head*/ f(g) /*tail*/;\n"
    );
}

fn recording() -> crate::SourceMapRecordingInputs {
    crate::SourceMapRecordingInputs {
        file: "event.js".into(),
        source_root: "".into(),
        sources_directory_path: "/".into(),
        current_directory: "/".into(),
        use_case_sensitive_source_keys: true,
        inline_sources: false,
        original_positions: None,
    }
}

#[test]
fn recording_presence_belongs_to_each_actual_writer_bracket() {
    let (arena, source, _, _) = fixture();
    let mut result = transform_nodes(
        arena,
        vec![TransformRoot::SourceFile(source)],
        vec![],
        false,
    )
    .unwrap();
    let mut printer = canonical_printer();
    for records_maps in [true, false, true] {
        let printed = printer
            .print(
                &mut result,
                PrintRequest::SourceFile(source),
                records_maps.then(recording),
            )
            .unwrap();
        assert_eq!(printed.text(), "f(g);\n");
        let map = printed.into_source_map();
        assert_eq!(map.is_some(), records_maps);
        if let Some(mut map) = map {
            let json = map.to_json_string();
            assert!(json.contains("\"mappings\":"));
            assert!(!json.contains("\"mappings\":\"\""), "{json}");
        }
    }
}

#[test]
fn observation_bits_preserve_all_emit_flag_bits_and_unknown_fallbacks() {
    for bits in [0, 1, 1 << 31, u32::MAX] {
        let flags = EmitFlags::from_bits(bits);
        for present in [false, true] {
            for inert in [false, true] {
                let facts = NodePrintFacts::fresh(flags, present, inert);
                assert_eq!(facts.flags().bits(), bits);
                assert_eq!(facts.may_have_leading_metadata(), present);
                assert_eq!(facts.may_have_trailing_metadata(), present || !inert);
            }
        }
        let facts = NodePrintFacts::unknown(flags);
        assert_eq!(facts.flags().bits(), bits);
        assert!(facts.may_have_leading_metadata() && facts.may_have_trailing_metadata());
    }
}
