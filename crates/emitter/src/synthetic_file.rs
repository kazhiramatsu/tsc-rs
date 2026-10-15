//! tsgo `printer/syntheticfile.go`: a node printed for insertion, with its
//! positions (and its descendants') assigned from where the printer wrote
//! them (`ChangeTrackerWriter.AssignPositionsToNode`), wrapped in a
//! synthetic source file the formatter can read.

use tsc_binder::node_util;
use tsc_syntax::{
    for_each_child, ExternalModuleIndicatorOptions, LanguageVariant, NodeArena, NodeData, NodeId,
    ScriptKind, SourceFile, SourceFileData, SyntaxKind,
};
use tsc_types::NodeFlags;

use crate::writer::{PositionKey, PrintPositions};
use crate::{
    create_printer, transform_nodes, NewLineKind, PrintRequest, PrinterError, PrinterOptions,
    SourceFileTextMode, StandaloneWriter, TransformArena, TransformNode, TransformNodeArray,
    TransformationResult,
};

/// A node printed and positioned for insertion.
pub struct PositionedNode {
    /// The printed text, its trailing new line trimmed.
    pub text: String,
    /// A snippet print's escapes of the text it wrote that holds `$`, as
    /// edits of `text` (tsgo `snippetEmitTextWriter`).
    pub snippet_escapes: Vec<crate::writer::SnippetEscape>,
    /// The synthetic source file of the text, whose only statement is the
    /// positioned node.
    pub file: SourceFile,
    /// The positioned node in `file`.
    pub node: NodeId,
}

/// How [`print_and_position`] prints: tsgo `PrintAndPositionNode`'s new
/// line and indent size, whether it has the node's source file (whose
/// comments and lines it then reads), and the language variant of the
/// synthetic file.
#[derive(Clone, Copy, Debug)]
pub struct PositionedPrint {
    pub new_line: NewLineKind,
    pub indent_size: usize,
    pub with_source_file: bool,
    pub language_variant: LanguageVariant,
    /// tsgo's snippet printer (`createSnippetPrinter`) of a program of this
    /// target (or of none): no comments, the printer's defaults otherwise,
    /// and the escapes of `$`; else the change tracker's printer.
    pub snippet: Option<Option<tsc_types::ScriptTarget>>,
}

/// tsgo `PrintAndPositionNode` then `CreateSyntheticSourceFile`: `node`
/// printed as TypeScript with tsgo's change tracker printer, its positions
/// assigned from the print to a copy of its subtree.
pub fn print_and_position(
    transformation: &mut TransformationResult<'_>,
    node: TransformNode,
    print: PositionedPrint,
) -> Result<PositionedNode, PrinterError> {
    let (options, writer) = match print.snippet {
        Some(target) => {
            let options = PrinterOptions::new(print.new_line)
                .with_remove_comments(true)
                .with_declaration_syntax(true)
                .with_source_file_text_mode(SourceFileTextMode::Canonical);
            (
                match target {
                    Some(target) => options.with_target(target),
                    None => options,
                },
                StandaloneWriter::SnippetChangeTracker(print.indent_size),
            )
        }
        // tsgo reads comments from the current source file: a node of none
        // has no comments to print.
        None => (
            PrinterOptions::new(print.new_line)
                .with_remove_comments(!print.with_source_file)
                .with_declaration_syntax(true)
                .with_never_ascii_escape(true)
                .with_terminate_unterminated_literals(true)
                .with_preserve_source_newlines(true)
                .with_source_file_text_mode(SourceFileTextMode::Canonical),
            StandaloneWriter::ChangeTracker(print.indent_size),
        ),
    };
    let printed = create_printer(options).print(
        transformation,
        PrintRequest::StandaloneNode { node, writer },
        None,
    )?;
    let text = printed
        .text()
        .strip_suffix(print.new_line.text())
        .unwrap_or(printed.text())
        .to_owned();
    let positions = printed.positions().expect("a change tracker print");
    let source = transformation.arena().source(node.source())?.syntax();
    let (arena, root) = assign_positions_to_node(&source.arena, node, positions);
    let file = create_synthetic_source_file(arena, root, text.clone(), print.language_variant);
    Ok(PositionedNode {
        snippet_escapes: printed.snippet_escapes().to_vec(),
        text,
        file,
        node: root,
    })
}

/// [`print_and_position`] of a tree decoded from the API, which has no
/// source file. `single_quoted` are the string literals to print in single
/// quotes.
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
    print_and_position(
        &mut transformation,
        TransformNode::new(source_id, node),
        PositionedPrint {
            new_line,
            indent_size,
            with_source_file: false,
            language_variant,
            snippet: None,
        },
    )
}

/// tsgo `AssignPositionsToNode`: a copy of the subtree with each node and
/// node list at the positions its print began and ended (zero for one not
/// printed), and each child's parent its node. A modifier list is tsgo's
/// new `ModifierList` of the visited modifiers, which has no position; a
/// copied node has no JSDoc (tsgo keeps a node's JSDoc in its file).
fn assign_positions_to_node(
    source: &NodeArena,
    node: TransformNode,
    positions: &PrintPositions,
) -> (NodeArena, NodeId) {
    let mut copy = source.copy_subtree(node.node());
    let mut modifiers = Vec::new();
    for &(old, new) in &copy.nodes {
        let key = PositionKey::Node(TransformNode::new(node.source(), old));
        let record = copy.arena.node_mut(new);
        record.pos = positions.pos(key) as u32;
        record.end = positions.end(key) as u32;
        record.js_doc = None;
        modifiers.extend(node_util::modifiers_of_data(&record.data));
    }
    for &(old, new) in &copy.arrays {
        let (pos, end) = if modifiers.contains(&new) {
            (u32::MAX, u32::MAX)
        } else {
            let key = PositionKey::List(TransformNodeArray::new(node.source(), old));
            (positions.pos(key) as u32, positions.end(key) as u32)
        };
        let record = copy.arena.node_array_mut(new);
        record.pos = pos;
        record.end = end;
    }
    for &(_, parent) in &copy.nodes {
        let mut children = Vec::new();
        for_each_child(&copy.arena, copy.arena.node(parent), |child| {
            children.push(child);
            false
        });
        for child in children {
            copy.arena.node_mut(child).parent = Some(parent);
        }
    }
    (copy.arena, copy.root)
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
