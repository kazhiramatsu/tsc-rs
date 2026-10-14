//! tsgo `printer/syntheticfile.go`: a node printed for insertion, with its
//! positions (and its descendants') assigned from where the printer wrote
//! them (`ChangeTrackerWriter.AssignPositionsToNode`), wrapped in a
//! synthetic source file the formatter can read.

use tsc_binder::node_util;
use tsc_syntax::{
    for_each_child, for_each_child_array, ExternalModuleIndicatorOptions, LanguageVariant,
    NodeArena, NodeData, NodeId, ScriptKind, SourceFile, SourceFileData, SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::writer::{PositionKey, PrintPositions};
use crate::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterError, PrinterOptions,
    SourceFileTextMode, StandaloneWriter, TransformArena, TransformNode, TransformNodeArray,
    TransformSourceId,
};

/// A node printed and positioned for insertion.
pub struct PositionedNode {
    /// The printed text, its trailing new line trimmed.
    pub text: String,
    /// The synthetic source file of the text, whose only statement is the
    /// positioned node.
    pub file: SourceFile,
    /// The positioned node in `file`.
    pub node: NodeId,
}

/// tsgo `PrintAndPositionNode` then `CreateSyntheticSourceFile`: `node` of
/// `source` printed as TypeScript of no source file (so without source
/// comments), its positions assigned from the print. `single_quoted` are
/// the string literals to print in single quotes.
pub fn print_and_position_node(
    source: &SourceFile,
    node: NodeId,
    single_quoted: &[NodeId],
    new_line: NewLineKind,
    indent_size: usize,
    language_variant: LanguageVariant,
) -> Result<PositionedNode, PrinterError> {
    let mut arena = TransformArena::new();
    let source_id = arena.add_source(source, None);
    arena
        .mark_single_quoted(source_id, single_quoted)
        .map_err(PrinterError::Transform)?;
    let mut transformation =
        transform_nodes(arena, Vec::new(), Vec::new(), true).map_err(PrinterError::Transform)?;
    let options = PrinterOptions::new(new_line)
        .with_remove_comments(true)
        .with_declaration_syntax(true)
        .with_never_ascii_escape(true)
        .with_terminate_unterminated_literals(true)
        .with_preserve_source_newlines(true)
        .with_source_file_text_mode(SourceFileTextMode::Canonical);
    let printed = create_printer(options).print(
        &mut transformation,
        PrintRequest::StandaloneNode {
            node: TransformNode::new(source_id, node),
            writer: StandaloneWriter::ChangeTracker(indent_size),
        },
        None,
    )?;
    let text = printed
        .text()
        .strip_suffix(new_line.text())
        .unwrap_or(printed.text())
        .to_owned();
    let positions = printed.positions().expect("a change tracker print");

    let mut arena = source.arena.detached_clone();
    assign_positions_to_node(&mut arena, source_id, positions, node);
    let file = create_synthetic_source_file(arena, node, text.clone(), language_variant);
    Ok(PositionedNode { text, file, node })
}

/// tsgo `AssignPositionsToNode`: each node and node list of the subtree at
/// the positions its print began and ended (zero for one not printed), and
/// each child's parent its node. A modifier list is tsgo's new
/// `ModifierList` of the visited modifiers, which has no position.
fn assign_positions_to_node(
    arena: &mut NodeArena,
    source: TransformSourceId,
    positions: &PrintPositions,
    node: NodeId,
) {
    let modifiers = node_util::modifiers_of_data(&arena.node(node).data);
    let mut children = Vec::new();
    for_each_child(arena, arena.node(node), |child| {
        children.push(child);
        false
    });
    let mut arrays = Vec::new();
    for_each_child_array(arena.node(node), |array| {
        arrays.push(array);
        false
    });
    for array in arrays {
        let (pos, end) = if Some(array) == modifiers {
            (u32::MAX, u32::MAX)
        } else {
            let key = PositionKey::List(TransformNodeArray::new(source, array));
            (positions.pos(key) as u32, positions.end(key) as u32)
        };
        let record = arena.node_array_mut(array);
        record.pos = pos;
        record.end = end;
    }
    let key = PositionKey::Node(TransformNode::new(source, node));
    let record = arena.node_mut(node);
    record.pos = positions.pos(key) as u32;
    record.end = positions.end(key) as u32;
    for child in children {
        arena.node_mut(child).parent = Some(node);
        assign_positions_to_node(arena, source, positions, child);
    }
}

/// tsgo `CreateSyntheticSourceFile`: a source file of `text` whose
/// statement list holds the positioned `node`.
fn create_synthetic_source_file(
    mut arena: NodeArena,
    node: NodeId,
    text: String,
    language_variant: LanguageVariant,
) -> SourceFile {
    let text_len = text.len();
    let eof = arena.alloc_token(
        SyntaxKind::EndOfFileToken,
        text_len,
        text_len,
        NodeFlags::NONE,
    );
    let (node_pos, node_end) = {
        let record = arena.node(node);
        (record.pos as usize, record.end as usize)
    };
    let statements = arena.alloc_array(&[node], node_pos, node_end, false);
    let root = arena.alloc_node(
        NodeData::SourceFile(SourceFileData {
            statements: Some(statements),
            end_of_file_token: Some(eof),
        }),
        0,
        text_len,
        NodeFlags::NONE,
    );
    arena.node_mut(node).parent = Some(root);
    arena.node_mut(eof).parent = Some(root);
    SourceFile::from_tree(
        "",
        Some(text),
        arena,
        root,
        language_variant,
        ScriptKind::Unknown,
        false,
        ExternalModuleIndicatorOptions::default(),
    )
}
