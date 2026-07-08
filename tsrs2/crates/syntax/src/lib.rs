#![forbid(unsafe_code)]

pub mod arena;
mod chars;
pub mod for_each_child;
mod keywords;
pub mod kind;
pub mod nodes;
pub mod scanner;

use tsrs2_diags::{compute_line_map, DiagnosticList, LineMap};

pub use arena::NodeArena;
pub use for_each_child::{for_each_child, NodeLookup};
pub use kind::SyntaxKind;
pub use nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId, NodePayload, SourceFileData};
pub use scanner::{scan_tokens, LanguageVariant, TokenRecord};
use tsrs2_types::NodeFlags;

#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    pub file_name: String,
    pub text: String,
    pub language_variant: LanguageVariant,
    pub is_declaration_file: bool,
    pub line_map: LineMap,
    pub arena: NodeArena,
    pub root: NodeId,
    pub external_module_indicator: Option<NodeId>,
    pub parse_diagnostics: DiagnosticList,
}

impl SourceFile {
    pub fn node_count(&self) -> usize {
        self.arena.len()
    }

    pub fn identifier_count(&self) -> usize {
        self.arena
            .nodes()
            .iter()
            .filter(|node| node.kind == SyntaxKind::Identifier)
            .count()
    }
}

pub fn parse_source_file(
    file_name: impl Into<String>,
    text: impl Into<String>,
    language_variant: LanguageVariant,
) -> SourceFile {
    let file_name = file_name.into();
    let text = text.into();
    let line_map = compute_line_map(&text);
    let is_declaration_file = file_name.ends_with(".d.ts");
    let eof_pos = text.len();

    let mut arena = NodeArena::new();
    let statements = arena.empty_array(0);
    let end_of_file_token = arena.alloc_token(
        SyntaxKind::EndOfFileToken,
        eof_pos,
        eof_pos,
        NodeFlags::NONE,
    );
    let root = arena.alloc_node(
        NodeData::SourceFile(SourceFileData {
            statements: Some(statements),
            end_of_file_token: Some(end_of_file_token),
        }),
        0,
        eof_pos,
        NodeFlags::NONE,
    );
    arena.finalize_tree(root);

    SourceFile {
        file_name,
        text,
        language_variant,
        is_declaration_file,
        line_map,
        arena,
        root,
        external_module_indicator: None,
        parse_diagnostics: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_file_creates_root_and_eof_nodes() {
        let source = parse_source_file("a.ts", "", LanguageVariant::Standard);

        assert_eq!(source.node_count(), 2);
        assert_eq!(source.identifier_count(), 0);
        assert_eq!(source.line_map.line_starts, vec![0]);
        assert_eq!(source.arena.node(source.root).kind, SyntaxKind::SourceFile);

        let data = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("root is a source file");
        let eof = data.end_of_file_token.expect("source file has EOF token");
        assert_eq!(source.arena.node(eof).kind, SyntaxKind::EndOfFileToken);
        assert_eq!(source.arena.node(eof).parent, Some(source.root));
    }
}
