#![allow(dead_code)]

use crate::arena::NodeArena;
use crate::nodes::{NodeData, NodeId, SourceFileData};
use crate::scanner::{LanguageVariant, Scanner};
use crate::{SourceFile, SyntaxKind};
use tsrs2_diags::MessageChain;
use tsrs2_diags::{compute_line_map, gen, Diagnostic, DiagnosticList, DiagnosticMessage, LineMap};
use tsrs2_types::NodeFlags;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseOptions {
    pub language_variant: LanguageVariant,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            language_variant: LanguageVariant::Standard,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SyntaxCursor {
    _private: (),
}

trait ParserTruthy {
    fn is_truthy(&self) -> bool;
}

impl ParserTruthy for bool {
    fn is_truthy(&self) -> bool {
        *self
    }
}

impl<T> ParserTruthy for Option<T> {
    fn is_truthy(&self) -> bool {
        self.is_some()
    }
}

impl ParserTruthy for SyntaxKind {
    fn is_truthy(&self) -> bool {
        *self != SyntaxKind::Unknown
    }
}

struct Parser<'text> {
    scanner: Scanner<'text>,
    arena: NodeArena,
    file_name: String,
    language_variant: LanguageVariant,
    is_declaration_file: bool,
    line_map: LineMap,
    context_flags: NodeFlags,
    parse_diagnostics: DiagnosticList,
    parse_error_before_next_finished_node: bool,
    parsing_context: u32,
}

struct FinishedParse {
    file_name: String,
    language_variant: LanguageVariant,
    is_declaration_file: bool,
    line_map: LineMap,
    arena: NodeArena,
    root: NodeId,
    parse_diagnostics: DiagnosticList,
}

impl<'text> Parser<'text> {
    fn new(file_name: String, text: &'text str, language_variant: LanguageVariant) -> Self {
        let is_declaration_file = file_name.ends_with(".d.ts");
        Self {
            scanner: Scanner::new(text, language_variant),
            arena: NodeArena::new(),
            file_name,
            language_variant,
            is_declaration_file,
            line_map: compute_line_map(text),
            context_flags: NodeFlags::NONE,
            parse_diagnostics: Vec::new(),
            parse_error_before_next_finished_node: false,
            parsing_context: 0,
        }
    }

    fn parse_error_at_position(
        &mut self,
        start: usize,
        length: usize,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) {
        let args = args.iter().map(|arg| (*arg).to_owned()).collect();
        self.push_parse_diagnostic(start, length, message, args);
        self.parse_error_before_next_finished_node = true;
    }

    fn parse_error_at_current_token(&mut self, message: &'static DiagnosticMessage, args: &[&str]) {
        self.parse_error_at_position(
            self.scanner.token_start(),
            self.scanner.pos() - self.scanner.token_start(),
            message,
            args,
        );
    }

    fn drain_scanner_errors(&mut self) {
        for error in self.scanner.take_errors() {
            self.push_parse_diagnostic(error.start, error.length, error.message, error.args);
            self.parse_error_before_next_finished_node = true;
        }
    }

    fn token(&self) -> SyntaxKind {
        self.scanner.token()
    }

    fn next_token(&mut self) -> SyntaxKind {
        let token = self.scanner.scan();
        self.drain_scanner_errors();
        token
    }

    fn node_pos(&self) -> usize {
        self.scanner.full_start_pos()
    }

    fn finish_node(&mut self, id: NodeId, pos: usize) -> NodeId {
        self.finish_node_at(id, pos, self.scanner.full_start_pos())
    }

    fn parse_expected(
        &mut self,
        kind: SyntaxKind,
        message: Option<&'static DiagnosticMessage>,
    ) -> bool {
        if self.token() == kind {
            self.next_token();
            return true;
        }

        if let Some(message) = message {
            self.parse_error_at_current_token(message, &[]);
        } else {
            self.parse_error_at_current_token(&gen::_0_expected, &[&token_to_string(kind)]);
        }
        false
    }

    fn parse_optional(&mut self, kind: SyntaxKind) -> bool {
        if self.token() == kind {
            self.next_token();
            true
        } else {
            false
        }
    }

    fn parse_token_node(&mut self) -> NodeId {
        let kind = self.token();
        let pos = self.scanner.full_start_pos();
        let id = self
            .arena
            .alloc_token(kind, pos, self.scanner.pos(), NodeFlags::NONE);
        self.next_token();
        self.finish_node(id, pos)
    }

    fn create_missing_node(
        &mut self,
        kind: SyntaxKind,
        report: bool,
        message: Option<&'static DiagnosticMessage>,
        args: &[&str],
    ) -> NodeId {
        if report {
            if let Some(message) = message {
                self.parse_error_at_current_token(message, args);
            } else {
                self.parse_error_at_current_token(&gen::_0_expected, &[&token_to_string(kind)]);
            }
        }

        let pos = self.scanner.token_start();
        let id = self.arena.alloc_missing(kind, pos);
        self.finish_node_at(id, pos, pos)
    }

    fn do_in_context<R>(
        &mut self,
        set: NodeFlags,
        clear: NodeFlags,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved = self.context_flags;
        self.context_flags =
            NodeFlags::from_bits((self.context_flags | set).bits() & !clear.bits());
        let result = f(self);
        self.context_flags = saved;
        result
    }

    fn try_parse<R: ParserTruthy>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let scanner_state = self.scanner.save();
        let diagnostics_len = self.parse_diagnostics.len();
        let parse_error_before_next_finished_node = self.parse_error_before_next_finished_node;
        let context_flags = self.context_flags;
        let parsing_context = self.parsing_context;

        let result = f(self);
        if !result.is_truthy() {
            self.scanner.restore(scanner_state);
            self.parse_diagnostics.truncate(diagnostics_len);
            self.parse_error_before_next_finished_node = parse_error_before_next_finished_node;
            self.context_flags = context_flags;
            self.parsing_context = parsing_context;
        }
        result
    }

    fn look_ahead<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let scanner_state = self.scanner.save();
        let diagnostics_len = self.parse_diagnostics.len();
        let parse_error_before_next_finished_node = self.parse_error_before_next_finished_node;
        let result = f(self);
        self.scanner.restore(scanner_state);
        self.parse_diagnostics.truncate(diagnostics_len);
        self.parse_error_before_next_finished_node = parse_error_before_next_finished_node;
        result
    }

    fn finish(mut self) -> FinishedParse {
        let statements = self.arena.empty_array(0);
        let eof_pos = self.scanner.full_start_pos();
        let eof_end = self.scanner.pos();
        let end_of_file_token = self.arena.alloc_token(
            SyntaxKind::EndOfFileToken,
            eof_pos,
            eof_end,
            NodeFlags::NONE,
        );
        let root = self.arena.alloc_node(
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements),
                end_of_file_token: Some(end_of_file_token),
            }),
            0,
            eof_end,
            NodeFlags::NONE,
        );
        self.arena.finalize_tree(root);

        FinishedParse {
            file_name: self.file_name,
            language_variant: self.language_variant,
            is_declaration_file: self.is_declaration_file,
            line_map: self.line_map,
            arena: self.arena,
            root,
            parse_diagnostics: self.parse_diagnostics,
        }
    }

    fn finish_node_at(&mut self, id: NodeId, pos: usize, end: usize) -> NodeId {
        let node = self.arena.node_mut(id);
        node.pos = pos as u32;
        node.end = end as u32;
        node.flags |= (self.context_flags & NodeFlags::CONTEXT_FLAGS).bits();
        if self.parse_error_before_next_finished_node {
            self.parse_error_before_next_finished_node = false;
            node.flags |= NodeFlags::THIS_NODE_HAS_ERROR.bits();
        }
        id
    }

    fn push_parse_diagnostic(
        &mut self,
        start: usize,
        length: usize,
        message: &'static DiagnosticMessage,
        args: Vec<String>,
    ) {
        let start_utf16 = self.to_utf16(start);
        if self
            .parse_diagnostics
            .last()
            .is_none_or(|last| last.start != Some(start_utf16))
        {
            let end_utf16 = self.to_utf16(start.saturating_add(length));
            self.parse_diagnostics.push(Diagnostic::new(
                Some(self.file_name.clone()),
                Some(start_utf16),
                Some(end_utf16.saturating_sub(start_utf16)),
                MessageChain::new(message, &args),
            ));
        }
    }

    fn to_utf16(&self, byte_offset: usize) -> u32 {
        self.line_map
            .byte_to_utf16
            .get(byte_offset)
            .copied()
            .unwrap_or_else(|| {
                self.line_map
                    .byte_to_utf16
                    .last()
                    .copied()
                    .expect("line map always contains EOF")
            })
    }
}

pub fn parse_source_file(
    file_name: String,
    text: String,
    options: ParseOptions,
    _cursor: Option<&SyntaxCursor>,
) -> SourceFile {
    let mut parser = Parser::new(file_name, &text, options.language_variant);
    parser.next_token();
    while parser.token() != SyntaxKind::EndOfFileToken {
        parser.next_token();
    }
    let finished = parser.finish();
    SourceFile {
        file_name: finished.file_name,
        text,
        language_variant: finished.language_variant,
        is_declaration_file: finished.is_declaration_file,
        line_map: finished.line_map,
        arena: finished.arena,
        root: finished.root,
        external_module_indicator: None,
        parse_diagnostics: finished.parse_diagnostics,
    }
}

fn token_to_string(kind: SyntaxKind) -> String {
    format!("{kind:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_source_file_drains_scanner_errors() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "\"unterminated".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert_eq!(source.parse_diagnostics.len(), 1);
        assert_eq!(source.parse_diagnostics[0].code(), 1002);
        assert_eq!(source.parse_diagnostics[0].start, Some(13));
        assert_eq!(source.parse_diagnostics[0].length, Some(0));
    }

    #[test]
    fn same_start_dedup_and_finish_node_error_transfer() {
        let mut parser = Parser::new("a.ts".to_owned(), "", LanguageVariant::Standard);
        parser.next_token();

        parser.parse_error_at_position(0, 0, &gen::Identifier_expected, &[]);
        parser.parse_error_at_position(0, 0, &gen::Unexpected_token, &[]);
        let first = parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
        let second = parser.create_missing_node(SyntaxKind::Identifier, false, None, &[]);

        assert_eq!(parser.parse_diagnostics.len(), 1);
        assert!(NodeFlags::from_bits(parser.arena.node(first).flags)
            .contains(NodeFlags::THIS_NODE_HAS_ERROR));
        assert!(!NodeFlags::from_bits(parser.arena.node(second).flags)
            .contains(NodeFlags::THIS_NODE_HAS_ERROR));
    }

    #[test]
    fn parse_token_node_consumes_current_token() {
        let mut parser = Parser::new("a.ts".to_owned(), ";", LanguageVariant::Standard);
        parser.next_token();

        let token = parser.parse_token_node();

        assert_eq!(parser.arena.node(token).kind, SyntaxKind::SemicolonToken);
        assert_eq!(parser.token(), SyntaxKind::EndOfFileToken);
    }

    #[test]
    fn expected_optional_context_and_speculation_restore_parser_state() {
        let mut parser = Parser::new("a.ts".to_owned(), ";x", LanguageVariant::Standard);
        parser.next_token();

        assert!(parser.parse_optional(SyntaxKind::SemicolonToken));
        assert_eq!(parser.node_pos(), 1);
        assert!(!parser.parse_expected(SyntaxKind::CommaToken, None));
        assert_eq!(parser.parse_diagnostics.len(), 1);

        let context_node =
            parser.do_in_context(NodeFlags::AWAIT_CONTEXT, NodeFlags::NONE, |parser| {
                parser.create_missing_node(SyntaxKind::Identifier, false, None, &[])
            });
        assert!(NodeFlags::from_bits(parser.arena.node(context_node).flags)
            .contains(NodeFlags::AWAIT_CONTEXT));

        let result: Option<NodeId> = parser.try_parse(|parser| {
            parser.parse_error_at_current_token(&gen::Unexpected_token, &[]);
            parser.next_token();
            None
        });
        assert!(result.is_none());
        assert_eq!(parser.token(), SyntaxKind::Identifier);
        assert_eq!(parser.parse_diagnostics.len(), 1);

        let lookahead = parser.look_ahead(|parser| {
            parser.next_token();
            parser.token()
        });
        assert_eq!(lookahead, SyntaxKind::EndOfFileToken);
        assert_eq!(parser.token(), SyntaxKind::Identifier);
    }
}
