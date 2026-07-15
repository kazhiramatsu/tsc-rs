#![forbid(unsafe_code)]

pub mod arena;
mod chars;
pub mod for_each_child;
mod keywords;
pub mod kind;
pub mod nodes;
mod parser;
pub mod scanner;
pub mod tokens;

use tsrs2_diags::{DiagnosticList, LineMap};

pub use arena::NodeArena;
pub use for_each_child::{for_each_child, NodeLookup};
pub use kind::SyntaxKind;
pub use nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId, NodePayload, SourceFileData};
pub use parser::{ParseOptions, SyntaxCursor};
pub use scanner::{
    is_js_whitespace, is_line_break, is_whitespace_like, js_trim_start, scan_tokens, skip_trivia,
    CommentDirective, CommentDirectiveKind, LanguageVariant, TokenRecord,
};

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
    /// tsc SourceFile.commentDirectives: scanner-collected
    /// `@ts-expect-error`/`@ts-ignore` markers, in scan order (byte
    /// offsets; see CommentDirective).
    pub comment_directives: Vec<CommentDirective>,
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

/// tsc parseJsonText: .json inputs parse as a single JSON value expression.
pub fn parse_json_text(file_name: impl Into<String>, text: impl Into<String>) -> SourceFile {
    parser::parse_json_text(file_name.into(), text.into())
}

/// tsc stringToToken for keyword lookup (identifierToKeywordKind path):
/// Some only for keyword kinds.
pub fn keyword_kind(text: &str) -> Option<SyntaxKind> {
    keywords::keyword_kind(text)
}

/// tsc-port: escapeLeadingUnderscores @6.0.3
/// tsc-hash: 86d7f97e898c96c6de2e47109d4583e4446ba8a518842f34d0d3cd4aa1b0b3c4
/// tsc-span: _tsc.js:11438-11440
///
/// A name beginning with two underscores gains ONE more, so user
/// `__proto__` cannot collide with internal symbol names (`__call`
/// etc. are stored unescaped). The factory applies this to every
/// Identifier/PrivateIdentifier escapedText. The charCodeAt checks are
/// byte checks: `_` is ASCII, so a multi-byte first char never matches.
pub fn escape_leading_underscores(name: &str) -> String {
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'_' && bytes[1] == b'_' {
        format!("_{name}")
    } else {
        name.to_owned()
    }
}

/// tsc-port: unescapeLeadingUnderscores @6.0.3
/// tsc-hash: e8294a1e4ef10b8ca2bcce06045e22adab6689e46b655acf51bacc3810ef5271
/// tsc-span: _tsc.js:11441-11444
///
/// Display-time inverse: exactly three leading underscores drop one.
pub fn unescape_leading_underscores(name: &str) -> &str {
    let bytes = name.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'_' && bytes[1] == b'_' && bytes[2] == b'_' {
        &name[1..]
    } else {
        name
    }
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
