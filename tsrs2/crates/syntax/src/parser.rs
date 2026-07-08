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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum ParsingContext {
    SourceElements = 0,
    BlockStatements = 1,
    SwitchClauses = 2,
    SwitchClauseStatements = 3,
    TypeMembers = 4,
    ClassMembers = 5,
    EnumMembers = 6,
    HeritageClauseElement = 7,
    VariableDeclarations = 8,
    ObjectBindingElements = 9,
    ArrayBindingElements = 10,
    ArgumentExpressions = 11,
    ObjectLiteralMembers = 12,
    JsxAttributes = 13,
    JsxChildren = 14,
    ArrayLiteralMembers = 15,
    Parameters = 16,
    JSDocParameters = 17,
    RestProperties = 18,
    TypeParameters = 19,
    TypeArguments = 20,
    TupleElementTypes = 21,
    HeritageClauses = 22,
    ImportOrExportSpecifiers = 23,
    ImportAttributes = 24,
    JSDocComment = 25,
}

impl ParsingContext {
    const ALL: [Self; 26] = [
        Self::SourceElements,
        Self::BlockStatements,
        Self::SwitchClauses,
        Self::SwitchClauseStatements,
        Self::TypeMembers,
        Self::ClassMembers,
        Self::EnumMembers,
        Self::HeritageClauseElement,
        Self::VariableDeclarations,
        Self::ObjectBindingElements,
        Self::ArrayBindingElements,
        Self::ArgumentExpressions,
        Self::ObjectLiteralMembers,
        Self::JsxAttributes,
        Self::JsxChildren,
        Self::ArrayLiteralMembers,
        Self::Parameters,
        Self::JSDocParameters,
        Self::RestProperties,
        Self::TypeParameters,
        Self::TypeArguments,
        Self::TupleElementTypes,
        Self::HeritageClauses,
        Self::ImportOrExportSpecifiers,
        Self::ImportAttributes,
        Self::JSDocComment,
    ];

    const fn bit(self) -> u32 {
        1_u32 << self as u8
    }
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

    fn parse_list(
        &mut self,
        context: ParsingContext,
        mut parse_element: impl FnMut(&mut Self) -> Option<NodeId>,
    ) -> crate::NodeArrayId {
        let saved_context = self.parsing_context;
        self.parsing_context |= context.bit();
        let mut list = Vec::new();
        let list_pos = self.node_pos();

        while !self.is_list_terminator(context) {
            if self.is_list_element(context, false) {
                if let Some(element) = parse_element(self) {
                    list.push(element);
                    continue;
                }
            }
            if self.abort_parsing_list_or_move_to_next_token(context) {
                break;
            }
        }

        self.parsing_context = saved_context;
        self.arena
            .alloc_array(list, list_pos, self.node_pos(), false)
    }

    fn parse_delimited_list(
        &mut self,
        context: ParsingContext,
        mut parse_element: impl FnMut(&mut Self) -> Option<NodeId>,
        consider_semicolon_as_delimiter: bool,
    ) -> crate::NodeArrayId {
        let saved_context = self.parsing_context;
        self.parsing_context |= context.bit();
        let mut list = Vec::new();
        let list_pos = self.node_pos();
        let mut comma_start = None;

        loop {
            if self.is_list_element(context, false) {
                let start_pos = self.scanner.full_start_pos();
                let Some(element) = parse_element(self) else {
                    self.parsing_context = saved_context;
                    return self.arena.empty_array(list_pos);
                };
                list.push(element);

                comma_start = Some(self.scanner.token_start());
                if self.parse_optional(SyntaxKind::CommaToken) {
                    continue;
                }

                comma_start = None;
                if self.is_list_terminator(context) {
                    break;
                }

                self.parse_expected(
                    SyntaxKind::CommaToken,
                    self.expected_comma_diagnostic(context),
                );
                if consider_semicolon_as_delimiter
                    && self.token() == SyntaxKind::SemicolonToken
                    && !self.scanner.has_preceding_line_break()
                {
                    self.next_token();
                }
                if start_pos == self.scanner.full_start_pos() {
                    self.next_token();
                }
                continue;
            }

            if self.is_list_terminator(context) {
                break;
            }
            if self.abort_parsing_list_or_move_to_next_token(context) {
                break;
            }
        }

        self.parsing_context = saved_context;
        self.arena
            .alloc_array(list, list_pos, self.node_pos(), comma_start.is_some())
    }

    fn is_list_terminator(&mut self, context: ParsingContext) -> bool {
        if self.token() == SyntaxKind::EndOfFileToken {
            return true;
        }

        match context {
            ParsingContext::BlockStatements
            | ParsingContext::SwitchClauses
            | ParsingContext::TypeMembers
            | ParsingContext::ClassMembers
            | ParsingContext::EnumMembers
            | ParsingContext::ObjectLiteralMembers
            | ParsingContext::ObjectBindingElements
            | ParsingContext::ImportOrExportSpecifiers
            | ParsingContext::ImportAttributes => self.token() == SyntaxKind::CloseBraceToken,
            ParsingContext::SwitchClauseStatements => matches!(
                self.token(),
                SyntaxKind::CloseBraceToken | SyntaxKind::CaseKeyword | SyntaxKind::DefaultKeyword
            ),
            ParsingContext::HeritageClauseElement => matches!(
                self.token(),
                SyntaxKind::OpenBraceToken
                    | SyntaxKind::ExtendsKeyword
                    | SyntaxKind::ImplementsKeyword
            ),
            ParsingContext::VariableDeclarations => self.is_variable_declarator_list_terminator(),
            ParsingContext::TypeParameters => matches!(
                self.token(),
                SyntaxKind::GreaterThanToken
                    | SyntaxKind::OpenParenToken
                    | SyntaxKind::OpenBraceToken
                    | SyntaxKind::ExtendsKeyword
                    | SyntaxKind::ImplementsKeyword
            ),
            ParsingContext::ArgumentExpressions => {
                matches!(
                    self.token(),
                    SyntaxKind::CloseParenToken | SyntaxKind::SemicolonToken
                )
            }
            ParsingContext::ArrayLiteralMembers
            | ParsingContext::TupleElementTypes
            | ParsingContext::ArrayBindingElements => self.token() == SyntaxKind::CloseBracketToken,
            ParsingContext::JSDocParameters
            | ParsingContext::Parameters
            | ParsingContext::RestProperties => {
                matches!(
                    self.token(),
                    SyntaxKind::CloseParenToken | SyntaxKind::CloseBracketToken
                )
            }
            ParsingContext::TypeArguments => self.token() != SyntaxKind::CommaToken,
            ParsingContext::HeritageClauses => {
                matches!(
                    self.token(),
                    SyntaxKind::OpenBraceToken | SyntaxKind::CloseBraceToken
                )
            }
            ParsingContext::JsxAttributes => {
                matches!(
                    self.token(),
                    SyntaxKind::GreaterThanToken | SyntaxKind::SlashToken
                )
            }
            ParsingContext::JsxChildren => {
                self.token() == SyntaxKind::LessThanToken
                    && self.look_ahead(|parser| {
                        parser.next_token();
                        parser.token() == SyntaxKind::SlashToken
                    })
            }
            ParsingContext::SourceElements | ParsingContext::JSDocComment => false,
        }
    }

    fn is_list_element(&mut self, context: ParsingContext, in_error_recovery: bool) -> bool {
        match context {
            ParsingContext::SourceElements
            | ParsingContext::BlockStatements
            | ParsingContext::SwitchClauseStatements => {
                !(self.token() == SyntaxKind::SemicolonToken && in_error_recovery)
                    && self.is_start_of_statement()
            }
            ParsingContext::SwitchClauses => {
                matches!(
                    self.token(),
                    SyntaxKind::CaseKeyword | SyntaxKind::DefaultKeyword
                )
            }
            ParsingContext::TypeMembers => self.look_ahead(|parser| parser.is_type_member_start()),
            ParsingContext::ClassMembers => {
                self.look_ahead(|parser| parser.is_class_member_start())
                    || self.token() == SyntaxKind::SemicolonToken && !in_error_recovery
            }
            ParsingContext::EnumMembers => {
                self.token() == SyntaxKind::OpenBracketToken || self.is_literal_property_name()
            }
            ParsingContext::ObjectLiteralMembers => {
                matches!(
                    self.token(),
                    SyntaxKind::OpenBracketToken
                        | SyntaxKind::AsteriskToken
                        | SyntaxKind::DotDotDotToken
                        | SyntaxKind::DotToken
                ) || self.is_literal_property_name()
            }
            ParsingContext::RestProperties => self.is_literal_property_name(),
            ParsingContext::ObjectBindingElements => {
                matches!(
                    self.token(),
                    SyntaxKind::OpenBracketToken | SyntaxKind::DotDotDotToken
                ) || self.is_literal_property_name()
            }
            ParsingContext::ImportAttributes => self.is_import_attribute_name(),
            ParsingContext::HeritageClauseElement => {
                if self.token() == SyntaxKind::OpenBraceToken {
                    return self
                        .look_ahead(|parser| parser.is_valid_heritage_clause_object_literal());
                }
                if !in_error_recovery {
                    self.is_start_of_left_hand_side_expression()
                        && !self.is_heritage_clause_extends_or_implements_keyword()
                } else {
                    self.is_identifier() && !self.is_heritage_clause_extends_or_implements_keyword()
                }
            }
            ParsingContext::VariableDeclarations => {
                self.is_binding_identifier_or_private_identifier_or_pattern()
            }
            ParsingContext::ArrayBindingElements => {
                matches!(
                    self.token(),
                    SyntaxKind::CommaToken | SyntaxKind::DotDotDotToken
                ) || self.is_binding_identifier_or_private_identifier_or_pattern()
            }
            ParsingContext::TypeParameters => {
                matches!(
                    self.token(),
                    SyntaxKind::InKeyword | SyntaxKind::ConstKeyword
                ) || self.is_identifier()
            }
            ParsingContext::ArrayLiteralMembers => {
                matches!(self.token(), SyntaxKind::CommaToken | SyntaxKind::DotToken)
                    || self.token() == SyntaxKind::DotDotDotToken
                    || self.is_start_of_expression()
            }
            ParsingContext::ArgumentExpressions => {
                self.token() == SyntaxKind::DotDotDotToken || self.is_start_of_expression()
            }
            ParsingContext::Parameters => self.is_start_of_parameter(false),
            ParsingContext::JSDocParameters => self.is_start_of_parameter(true),
            ParsingContext::TypeArguments | ParsingContext::TupleElementTypes => {
                self.token() == SyntaxKind::CommaToken || self.is_start_of_type(false)
            }
            ParsingContext::HeritageClauses => self.is_heritage_clause(),
            ParsingContext::ImportOrExportSpecifiers => {
                if self.token() == SyntaxKind::FromKeyword
                    && self.look_ahead(|parser| {
                        parser.next_token();
                        parser.token() == SyntaxKind::StringLiteral
                    })
                {
                    return false;
                }
                self.token() == SyntaxKind::StringLiteral
                    || token_is_identifier_or_keyword(self.token())
            }
            ParsingContext::JsxAttributes => {
                token_is_identifier_or_keyword(self.token())
                    || self.token() == SyntaxKind::OpenBraceToken
            }
            ParsingContext::JsxChildren | ParsingContext::JSDocComment => true,
        }
    }

    fn abort_parsing_list_or_move_to_next_token(&mut self, context: ParsingContext) -> bool {
        self.parsing_context_error(context);
        if self.is_in_some_parsing_context() {
            return true;
        }
        self.next_token();
        false
    }

    fn is_in_some_parsing_context(&mut self) -> bool {
        for context in ParsingContext::ALL {
            if self.parsing_context & context.bit() != 0
                && (self.is_list_element(context, true) || self.is_list_terminator(context))
            {
                return true;
            }
        }
        false
    }

    fn parsing_context_error(&mut self, context: ParsingContext) {
        match context {
            ParsingContext::SourceElements => {
                if self.token() == SyntaxKind::DefaultKeyword {
                    self.parse_error_at_current_token(
                        &gen::_0_expected,
                        &[&token_to_string(SyntaxKind::ExportKeyword)],
                    );
                } else {
                    self.parse_error_at_current_token(&gen::Declaration_or_statement_expected, &[]);
                }
            }
            ParsingContext::BlockStatements => {
                self.parse_error_at_current_token(&gen::Declaration_or_statement_expected, &[]);
            }
            ParsingContext::SwitchClauses => {
                self.parse_error_at_current_token(&gen::case_or_default_expected, &[]);
            }
            ParsingContext::SwitchClauseStatements => {
                self.parse_error_at_current_token(&gen::Statement_expected, &[]);
            }
            ParsingContext::RestProperties | ParsingContext::TypeMembers => {
                self.parse_error_at_current_token(&gen::Property_or_signature_expected, &[]);
            }
            ParsingContext::ClassMembers => {
                self.parse_error_at_current_token(
                    &gen::Unexpected_token_A_constructor_method_accessor_or_property_was_expected,
                    &[],
                );
            }
            ParsingContext::EnumMembers => {
                self.parse_error_at_current_token(&gen::Enum_member_expected, &[]);
            }
            ParsingContext::HeritageClauseElement => {
                self.parse_error_at_current_token(&gen::Expression_expected, &[]);
            }
            ParsingContext::VariableDeclarations => {
                if is_keyword(self.token()) {
                    self.parse_error_at_current_token(
                        &gen::_0_is_not_allowed_as_a_variable_declaration_name,
                        &[&token_to_string(self.token())],
                    );
                } else {
                    self.parse_error_at_current_token(&gen::Variable_declaration_expected, &[]);
                }
            }
            ParsingContext::ObjectBindingElements => {
                self.parse_error_at_current_token(
                    &gen::Property_destructuring_pattern_expected,
                    &[],
                );
            }
            ParsingContext::ArrayBindingElements => {
                self.parse_error_at_current_token(
                    &gen::Array_element_destructuring_pattern_expected,
                    &[],
                );
            }
            ParsingContext::ArgumentExpressions => {
                self.parse_error_at_current_token(&gen::Argument_expression_expected, &[]);
            }
            ParsingContext::ObjectLiteralMembers => {
                self.parse_error_at_current_token(&gen::Property_assignment_expected, &[]);
            }
            ParsingContext::ArrayLiteralMembers => {
                self.parse_error_at_current_token(&gen::Expression_or_comma_expected, &[]);
            }
            ParsingContext::JSDocParameters => {
                self.parse_error_at_current_token(&gen::Parameter_declaration_expected, &[]);
            }
            ParsingContext::Parameters => {
                if is_keyword(self.token()) {
                    self.parse_error_at_current_token(
                        &gen::_0_is_not_allowed_as_a_parameter_name,
                        &[&token_to_string(self.token())],
                    );
                } else {
                    self.parse_error_at_current_token(&gen::Parameter_declaration_expected, &[]);
                }
            }
            ParsingContext::TypeParameters => {
                self.parse_error_at_current_token(&gen::Type_parameter_declaration_expected, &[]);
            }
            ParsingContext::TypeArguments => {
                self.parse_error_at_current_token(&gen::Type_argument_expected, &[]);
            }
            ParsingContext::TupleElementTypes => {
                self.parse_error_at_current_token(&gen::Type_expected, &[]);
            }
            ParsingContext::HeritageClauses => {
                self.parse_error_at_current_token(&gen::Unexpected_token_expected, &[]);
            }
            ParsingContext::ImportOrExportSpecifiers => {
                if self.token() == SyntaxKind::FromKeyword {
                    self.parse_error_at_current_token(&gen::_0_expected, &["}"]);
                } else {
                    self.parse_error_at_current_token(&gen::Identifier_expected, &[]);
                }
            }
            ParsingContext::JsxAttributes
            | ParsingContext::JsxChildren
            | ParsingContext::JSDocComment => {
                self.parse_error_at_current_token(&gen::Identifier_expected, &[]);
            }
            ParsingContext::ImportAttributes => {
                self.parse_error_at_current_token(&gen::Identifier_or_string_literal_expected, &[]);
            }
        }
    }

    fn expected_comma_diagnostic(
        &self,
        context: ParsingContext,
    ) -> Option<&'static DiagnosticMessage> {
        if context == ParsingContext::EnumMembers {
            Some(&gen::An_enum_member_name_must_be_followed_by_a_or)
        } else {
            None
        }
    }

    fn is_variable_declarator_list_terminator(&self) -> bool {
        self.can_parse_semicolon()
            || matches!(self.token(), SyntaxKind::InKeyword | SyntaxKind::OfKeyword)
            || self.token() == SyntaxKind::EqualsGreaterThanToken
    }

    fn can_parse_semicolon(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::SemicolonToken | SyntaxKind::CloseBraceToken | SyntaxKind::EndOfFileToken
        ) || self.scanner.has_preceding_line_break()
    }

    fn is_literal_property_name(&self) -> bool {
        token_is_identifier_or_keyword(self.token())
            || matches!(
                self.token(),
                SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
            )
    }

    fn is_import_attribute_name(&self) -> bool {
        token_is_identifier_or_keyword(self.token()) || self.token() == SyntaxKind::StringLiteral
    }

    fn is_valid_heritage_clause_object_literal(&mut self) -> bool {
        debug_assert_eq!(self.token(), SyntaxKind::OpenBraceToken);
        if self.next_token() == SyntaxKind::CloseBraceToken {
            let next = self.next_token();
            matches!(
                next,
                SyntaxKind::CommaToken
                    | SyntaxKind::OpenBraceToken
                    | SyntaxKind::ExtendsKeyword
                    | SyntaxKind::ImplementsKeyword
            )
        } else {
            true
        }
    }

    fn is_heritage_clause_extends_or_implements_keyword(&mut self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::ExtendsKeyword | SyntaxKind::ImplementsKeyword
        ) && self.look_ahead(|parser| {
            parser.next_token();
            parser.is_start_of_expression()
        })
    }

    fn is_heritage_clause(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::ExtendsKeyword | SyntaxKind::ImplementsKeyword
        )
    }

    fn is_binding_identifier_or_private_identifier_or_pattern(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::OpenBraceToken
                | SyntaxKind::OpenBracketToken
                | SyntaxKind::PrivateIdentifier
        ) || self.is_binding_identifier()
    }

    fn is_binding_identifier(&self) -> bool {
        self.token() == SyntaxKind::Identifier
            || self.token().value() > SyntaxKind::LastReservedWord.value()
    }

    fn is_identifier(&self) -> bool {
        token_is_identifier_or_keyword(self.token())
    }

    fn is_type_member_start(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::OpenParenToken
                | SyntaxKind::LessThanToken
                | SyntaxKind::OpenBracketToken
                | SyntaxKind::AsteriskToken
                | SyntaxKind::DotDotDotToken
                | SyntaxKind::QuestionToken
        ) || self.is_literal_property_name()
            || self.is_modifier_kind(self.token())
    }

    fn is_class_member_start(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::OpenBracketToken
                | SyntaxKind::AsteriskToken
                | SyntaxKind::DotDotDotToken
                | SyntaxKind::AtToken
                | SyntaxKind::StaticKeyword
                | SyntaxKind::AccessorKeyword
        ) || self.is_literal_property_name()
            || self.is_modifier_kind(self.token())
    }

    fn is_start_of_parameter(&self, is_jsdoc_parameter: bool) -> bool {
        self.token() == SyntaxKind::DotDotDotToken
            || self.is_binding_identifier_or_private_identifier_or_pattern()
            || self.is_modifier_kind(self.token())
            || self.token() == SyntaxKind::AtToken
            || self.is_start_of_type(!is_jsdoc_parameter)
    }

    fn is_start_of_type(&self, in_start_of_parameter: bool) -> bool {
        match self.token() {
            SyntaxKind::AnyKeyword
            | SyntaxKind::UnknownKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::UniqueKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::ThisKeyword
            | SyntaxKind::TypeOfKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::OpenBraceToken
            | SyntaxKind::OpenBracketToken
            | SyntaxKind::LessThanToken
            | SyntaxKind::BarToken
            | SyntaxKind::AmpersandToken
            | SyntaxKind::NewKeyword
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::ObjectKeyword
            | SyntaxKind::AsteriskToken
            | SyntaxKind::QuestionToken
            | SyntaxKind::ExclamationToken
            | SyntaxKind::DotDotDotToken
            | SyntaxKind::InferKeyword
            | SyntaxKind::ImportKeyword
            | SyntaxKind::AssertsKeyword
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead => true,
            SyntaxKind::FunctionKeyword => !in_start_of_parameter,
            SyntaxKind::MinusToken => !in_start_of_parameter,
            SyntaxKind::OpenParenToken => !in_start_of_parameter,
            _ => self.is_identifier(),
        }
    }

    fn is_start_of_left_hand_side_expression(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::ThisKeyword
                | SyntaxKind::SuperKeyword
                | SyntaxKind::NullKeyword
                | SyntaxKind::TrueKeyword
                | SyntaxKind::FalseKeyword
                | SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::StringLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::TemplateHead
                | SyntaxKind::OpenParenToken
                | SyntaxKind::OpenBracketToken
                | SyntaxKind::OpenBraceToken
                | SyntaxKind::FunctionKeyword
                | SyntaxKind::ClassKeyword
                | SyntaxKind::NewKeyword
                | SyntaxKind::SlashToken
                | SyntaxKind::SlashEqualsToken
        ) || self.is_identifier()
    }

    fn is_start_of_expression(&self) -> bool {
        if self.is_start_of_left_hand_side_expression() {
            return true;
        }

        matches!(
            self.token(),
            SyntaxKind::PlusToken
                | SyntaxKind::MinusToken
                | SyntaxKind::TildeToken
                | SyntaxKind::ExclamationToken
                | SyntaxKind::DeleteKeyword
                | SyntaxKind::TypeOfKeyword
                | SyntaxKind::VoidKeyword
                | SyntaxKind::PlusPlusToken
                | SyntaxKind::MinusMinusToken
                | SyntaxKind::LessThanToken
                | SyntaxKind::AwaitKeyword
                | SyntaxKind::YieldKeyword
                | SyntaxKind::PrivateIdentifier
                | SyntaxKind::AtToken
        ) || is_binary_operator(self.token())
            || self.is_identifier()
    }

    fn is_start_of_statement(&self) -> bool {
        match self.token() {
            SyntaxKind::AtToken
            | SyntaxKind::SemicolonToken
            | SyntaxKind::OpenBraceToken
            | SyntaxKind::VarKeyword
            | SyntaxKind::LetKeyword
            | SyntaxKind::UsingKeyword
            | SyntaxKind::FunctionKeyword
            | SyntaxKind::ClassKeyword
            | SyntaxKind::EnumKeyword
            | SyntaxKind::IfKeyword
            | SyntaxKind::DoKeyword
            | SyntaxKind::WhileKeyword
            | SyntaxKind::ForKeyword
            | SyntaxKind::ContinueKeyword
            | SyntaxKind::BreakKeyword
            | SyntaxKind::ReturnKeyword
            | SyntaxKind::WithKeyword
            | SyntaxKind::SwitchKeyword
            | SyntaxKind::ThrowKeyword
            | SyntaxKind::TryKeyword
            | SyntaxKind::DebuggerKeyword
            | SyntaxKind::CatchKeyword
            | SyntaxKind::FinallyKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::ExportKeyword
            | SyntaxKind::AsyncKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword
            | SyntaxKind::TypeKeyword
            | SyntaxKind::GlobalKeyword
            | SyntaxKind::DeferKeyword => true,
            _ => self.is_start_of_expression(),
        }
    }

    fn is_modifier_kind(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::AbstractKeyword
                | SyntaxKind::AccessorKeyword
                | SyntaxKind::AsyncKeyword
                | SyntaxKind::ConstKeyword
                | SyntaxKind::DeclareKeyword
                | SyntaxKind::ExportKeyword
                | SyntaxKind::InKeyword
                | SyntaxKind::PrivateKeyword
                | SyntaxKind::ProtectedKeyword
                | SyntaxKind::PublicKeyword
                | SyntaxKind::ReadonlyKeyword
                | SyntaxKind::StaticKeyword
                | SyntaxKind::OverrideKeyword
        )
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

fn token_is_identifier_or_keyword(kind: SyntaxKind) -> bool {
    kind.value() >= SyntaxKind::Identifier.value()
}

fn is_keyword(kind: SyntaxKind) -> bool {
    kind.value() >= SyntaxKind::FirstKeyword.value()
        && kind.value() <= SyntaxKind::LastKeyword.value()
}

fn is_binary_operator(kind: SyntaxKind) -> bool {
    kind.value() >= SyntaxKind::FirstBinaryOperator.value()
        && kind.value() <= SyntaxKind::LastBinaryOperator.value()
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

    #[test]
    fn delimited_list_tracks_trailing_comma() {
        let mut parser = Parser::new("a.ts".to_owned(), "a,)", LanguageVariant::Standard);
        parser.next_token();

        let list = parser.parse_delimited_list(
            ParsingContext::ArgumentExpressions,
            |parser| Some(parser.parse_token_node()),
            false,
        );
        let list = parser.arena.node_array(list);

        assert_eq!(list.nodes.len(), 1);
        assert!(list.has_trailing_comma);
        assert_eq!(parser.token(), SyntaxKind::CloseParenToken);
    }

    #[test]
    fn delimited_list_reports_missing_commas_and_keeps_progressing() {
        let mut parser = Parser::new("a.ts".to_owned(), "a b)", LanguageVariant::Standard);
        parser.next_token();

        let list = parser.parse_delimited_list(
            ParsingContext::ArgumentExpressions,
            |parser| Some(parser.parse_token_node()),
            false,
        );
        let list = parser.arena.node_array(list);

        assert_eq!(list.nodes.len(), 2);
        assert!(!list.has_trailing_comma);
        assert_eq!(parser.token(), SyntaxKind::CloseParenToken);
        assert_eq!(parser.parse_diagnostics.len(), 1);
        assert_eq!(parser.parse_diagnostics[0].code(), 1005);
    }

    #[test]
    fn list_recovery_aborts_when_outer_context_can_consume_token() {
        let mut parser = Parser::new("a.ts".to_owned(), "}", LanguageVariant::Standard);
        parser.next_token();
        parser.parsing_context |= ParsingContext::BlockStatements.bit();

        let list = parser.parse_delimited_list(
            ParsingContext::ArgumentExpressions,
            |parser| Some(parser.parse_token_node()),
            false,
        );

        assert!(parser.arena.node_array(list).nodes.is_empty());
        assert_eq!(parser.token(), SyntaxKind::CloseBraceToken);
        assert_eq!(parser.parse_diagnostics.len(), 1);
        assert_eq!(parser.parse_diagnostics[0].code(), 1135);
    }

    #[test]
    fn parse_list_skips_unrecoverable_tokens() {
        let mut parser = Parser::new("a.ts".to_owned(), "x case", LanguageVariant::Standard);
        parser.next_token();

        let list = parser.parse_list(ParsingContext::SwitchClauses, |parser| {
            Some(parser.parse_token_node())
        });

        let list = parser.arena.node_array(list);
        assert_eq!(list.nodes.len(), 1);
        assert_eq!(
            parser.arena.node(list.nodes[0]).kind,
            SyntaxKind::CaseKeyword
        );
        assert_eq!(parser.token(), SyntaxKind::EndOfFileToken);
        assert_eq!(parser.parse_diagnostics.len(), 1);
        assert_eq!(parser.parse_diagnostics[0].code(), 1130);
    }
}
