#![forbid(unsafe_code)]

pub mod arena;
mod chars;
pub mod for_each_child;
mod keywords;
pub mod kind;
pub mod nodes;
mod parser;
pub mod scanner;

use tsrs2_diags::{DiagnosticList, LineMap};

pub use arena::NodeArena;
pub use for_each_child::{for_each_child, NodeLookup};
pub use kind::SyntaxKind;
pub use nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId, NodePayload, SourceFileData};
pub use parser::{ParseOptions, SyntaxCursor};
pub use scanner::{scan_tokens, LanguageVariant, TokenRecord};

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
    options: ParseOptions,
    cursor: Option<&SyntaxCursor>,
) -> SourceFile {
    parser::parse_source_file(file_name.into(), text.into(), options, cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_file_creates_root_and_eof_nodes() {
        let source = parse_source_file("a.ts", "", ParseOptions::default(), None);

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
