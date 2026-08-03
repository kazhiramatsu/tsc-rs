#![forbid(unsafe_code)]

pub mod arena;
mod chars;
pub mod for_each_child;
mod keywords;
pub mod kind;
pub mod nodes;
pub mod observable_fields;
mod parser;
pub mod regex;
mod regex_unicode;
pub mod scanner;
pub mod tokens;

use tsc_diagnostics::{DiagnosticList, LineMap};
use tsc_types::ScriptTarget;

pub use arena::NodeArena;
pub use for_each_child::{for_each_child, NodeLookup};
pub use kind::SyntaxKind;
pub use nodes::{
    JSDocComment, Node, NodeArray, NodeArrayId, NodeData, NodeId, NodePayload, SourceFileData,
};
pub use observable_fields::{for_each_observable_field, ObservableField};
pub use parser::{
    is_identifier_text, is_identifier_text_for_target, JSDocParsingMode, ParseOptions, SyntaxCursor,
};
pub use scanner::{
    is_js_whitespace, is_line_break, is_whitespace_like, js_trim_start, scan_big_int_string,
    scan_tokens, skip_trivia, template_text_utf16, BigIntStringScan, CommentDirective,
    CommentDirectiveKind, LanguageVariant, TokenRecord,
};

/// Resolution-mode override retained from a leading
/// `/// <reference types="...">` directive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TypeReferenceDirectiveResolutionMode {
    Import,
    Require,
}

/// Exact source-owned triple-slash `path` or `lib` reference observation.
///
/// `pos` and `end` are UTF-16 offsets covering only the selected attribute
/// value, matching the vendored `FileReference` contract. `preserve` records
/// only the exact `preserve="true"` pragma value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileReference {
    pub file_name: String,
    pub pos: u32,
    pub end: u32,
    pub preserve: bool,
}

/// Exact source-owned type-reference directive observation.
///
/// `pos` and `end` are UTF-16 offsets covering only the directive's `types`
/// value, matching the vendored `FileReference` contract and TS2688 span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeReferenceDirective {
    pub file_name: String,
    pub pos: u32,
    pub end: u32,
    pub resolution_mode: Option<TypeReferenceDirectiveResolutionMode>,
    /// Whether the directive explicitly requested `preserve="true"`.
    pub preserve: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceFile {
    pub file_name: String,
    pub text: String,
    /// tsc SourceFile.languageVersion: the effective target used by the
    /// parser and scanner for this file.
    pub language_version: ScriptTarget,
    pub language_variant: LanguageVariant,
    pub is_declaration_file: bool,
    /// tsc SourceFile.jsDocParsingMode.
    pub js_doc_parsing_mode: JSDocParsingMode,
    pub line_map: LineMap,
    pub arena: NodeArena,
    pub root: NodeId,
    pub external_module_indicator: Option<NodeId>,
    pub parse_diagnostics: DiagnosticList,
    /// tsc SourceFile.jsDocDiagnostics: diagnostics produced while
    /// parsing attached JSDoc. They are merged into bind/check diagnostics
    /// only for checked JavaScript files, never into syntactic diagnostics.
    pub js_doc_diagnostics: DiagnosticList,
    /// tsc SourceFile.referencedFiles, in leading pragma order.
    pub referenced_files: Vec<FileReference>,
    /// tsc SourceFile.typeReferenceDirectives, in leading pragma order.
    pub type_reference_directives: Vec<TypeReferenceDirective>,
    /// tsc SourceFile.libReferenceDirectives, in leading pragma order.
    pub lib_reference_directives: Vec<FileReference>,
    /// Whether leading multiline comments contain a recognized
    /// `@jsxImportSource` pragma.
    pub has_jsx_import_source_pragma: bool,
    /// Whether leading multiline comments contain a recognized `@jsxRuntime`
    /// pragma.
    pub has_jsx_runtime_pragma: bool,
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

/// `parse_json_text` with explicit arena bases for a multi-file program.
pub fn parse_json_text_with_bases(
    file_name: impl Into<String>,
    text: impl Into<String>,
    node_id_base: u32,
    node_array_id_base: u32,
) -> SourceFile {
    parser::parse_json_text_with_bases(
        file_name.into(),
        text.into(),
        node_id_base,
        node_array_id_base,
    )
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
