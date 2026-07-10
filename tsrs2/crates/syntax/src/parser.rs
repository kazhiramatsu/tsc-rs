#![allow(dead_code)]

use crate::arena::NodeArena;
use crate::for_each_child::for_each_child;
use crate::nodes::{
    ArrayBindingPatternData, ArrayLiteralExpressionData, ArrayTypeData, ArrowFunctionData,
    AsExpressionData, AwaitExpressionData, BigIntLiteralData, BinaryExpressionData,
    BindingElementData, BlockData, BreakStatementData, CallExpressionData, CallSignatureData,
    CaseBlockData, CaseClauseData, CatchClauseData, ClassDeclarationData, ClassExpressionData,
    ClassStaticBlockDeclarationData, ComputedPropertyNameData, ConditionalExpressionData,
    ConditionalTypeData, ConstructSignatureData, ConstructorData, ConstructorTypeData,
    ContinueStatementData, DebuggerStatementData, DecoratorData, DefaultClauseData,
    DeleteExpressionData, DoStatementData, ElementAccessExpressionData, EmptyStatementData,
    EnumDeclarationData, EnumMemberData, ExportAssignmentData, ExportDeclarationData,
    ExportSpecifierData, ExpressionStatementData, ExpressionWithTypeArgumentsData,
    ExternalModuleReferenceData, ForInStatementData, ForOfStatementData, ForStatementData,
    FunctionDeclarationData, FunctionExpressionData, FunctionTypeData, GetAccessorData,
    HeritageClauseData, IdentifierData, IfStatementData, ImportAttributeData, ImportAttributesData,
    ImportClauseData, ImportDeclarationData, ImportEqualsDeclarationData, ImportSpecifierData,
    ImportTypeData, IndexSignatureData, IndexedAccessTypeData, InferTypeData,
    InterfaceDeclarationData, IntersectionTypeData, JSDocFunctionTypeData,
    JSDocNonNullableTypeData, JSDocNullableTypeData, JSDocOptionalTypeData, JSDocVariadicTypeData,
    JsxAttributeData, JsxAttributesData, JsxClosingElementData, JsxElementData, JsxExpressionData,
    JsxFragmentData, JsxNamespacedNameData, JsxOpeningElementData, JsxSelfClosingElementData,
    JsxSpreadAttributeData, JsxTextData, LabeledStatementData, LiteralTypeData, MappedTypeData,
    MetaPropertyData, MethodDeclarationData, MethodSignatureData, ModuleBlockData,
    ModuleDeclarationData, NamedExportsData, NamedImportsData, NamedTupleMemberData,
    NamespaceExportData, NamespaceExportDeclarationData, NamespaceImportData, NewExpressionData,
    NoSubstitutionTemplateLiteralData, NodeData, NodeId, NodePayload, NonNullExpressionData,
    NumericLiteralData, ObjectBindingPatternData, ObjectLiteralExpressionData,
    OmittedExpressionData, OptionalTypeData, ParameterData, ParenthesizedExpressionData,
    ParenthesizedTypeData, PostfixUnaryExpressionData, PrefixUnaryExpressionData,
    PrivateIdentifierData, PropertyAccessExpressionData, PropertyAssignmentData,
    PropertyDeclarationData, PropertySignatureData, QualifiedNameData,
    RegularExpressionLiteralData, RestTypeData, ReturnStatementData, SatisfiesExpressionData,
    SetAccessorData, ShorthandPropertyAssignmentData, SourceFileData, SpreadAssignmentData,
    SpreadElementData, StringLiteralData, SwitchStatementData, TaggedTemplateExpressionData,
    TemplateExpressionData, TemplateHeadData, TemplateLiteralTypeData, TemplateLiteralTypeSpanData,
    TemplateMiddleData, TemplateSpanData, TemplateTailData, ThrowStatementData, TryStatementData,
    TupleTypeData, TypeAliasDeclarationData, TypeAssertionExpressionData, TypeLiteralData,
    TypeOfExpressionData, TypeOperatorData, TypeParameterData, TypePredicateData, TypeQueryData,
    TypeReferenceData, UnionTypeData, VariableDeclarationData, VariableDeclarationListData,
    VariableStatementData, VoidExpressionData, WhileStatementData, WithStatementData,
    YieldExpressionData,
};
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
    not_parenthesized_arrow: std::collections::HashSet<usize>,
}

/// tsc Tristate: the arrow-function lookahead verdict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Tristate {
    False,
    True,
    Unknown,
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
            not_parenthesized_arrow: std::collections::HashSet::new(),
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

    /// tsc parseErrorAt.
    fn parse_error_at(
        &mut self,
        start: usize,
        end: usize,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) {
        self.parse_error_at_position(start, end.saturating_sub(start), message, args);
    }

    /// tsc parseErrorAtRange.
    fn parse_error_at_range(
        &mut self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) {
        let (pos, end) = {
            let node = self.arena.node(node);
            (node.pos as usize, node.end as usize)
        };
        self.parse_error_at(pos, end, message, args);
    }

    /// tsc getTextOfNodeFromSourceText (includeTrivia=false).
    fn text_of_node(&self, node: NodeId) -> String {
        let node = self.arena.node(node);
        let end = node.end as usize;
        let start = crate::scanner::skip_trivia(self.scanner.text(), node.pos as usize).min(end);
        self.scanner.text()[start..end].to_owned()
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

    /// tsc scanJsxText.
    fn scan_jsx_text(&mut self) -> SyntaxKind {
        let token = self.scanner.scan_jsx_token(true);
        self.drain_scanner_errors();
        token
    }

    /// tsc parser-side reScanJsxToken (allowMultilineJsxText=true).
    fn re_scan_jsx_token(&mut self) -> SyntaxKind {
        let token = self.scanner.re_scan_jsx_token(true);
        self.drain_scanner_errors();
        token
    }

    /// tsc scanJsxIdentifier.
    fn scan_jsx_identifier(&mut self) -> SyntaxKind {
        let token = self.scanner.scan_jsx_identifier();
        self.drain_scanner_errors();
        token
    }

    /// tsc scanJsxAttributeValue.
    fn scan_jsx_attribute_value(&mut self) -> SyntaxKind {
        let token = self.scanner.scan_jsx_attribute_value();
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

    /// tsc parseExpected with shouldAdvance=false: the JSX productions check
    /// the token and report, then pick the follow-up scan mode themselves.
    fn parse_expected_without_advancing(
        &mut self,
        kind: SyntaxKind,
        message: Option<&'static DiagnosticMessage>,
    ) -> bool {
        if self.token() == kind {
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

    /// tsc createMissingNode: reportAtCurrentPosition=true errors at the
    /// token FULL start with length 0; otherwise at the current token span.
    fn create_missing_node(
        &mut self,
        kind: SyntaxKind,
        report_at_current_position: bool,
        message: Option<&'static DiagnosticMessage>,
        args: &[&str],
    ) -> NodeId {
        if report_at_current_position {
            if let Some(message) = message {
                self.parse_error_at_position(self.scanner.full_start_pos(), 0, message, args);
            }
        } else if let Some(message) = message {
            self.parse_error_at_current_token(message, args);
        }

        let pos = self.scanner.token_start();
        let id = self.arena.alloc_missing(kind, pos);
        self.finish_node_at(id, pos, pos)
    }

    /// tsc createIdentifier: the real-identifier path is parse_identifier;
    /// this is the private-identifier/missing tail. (The Unknown-token
    /// reScanInvalidIdentifier retry is not surfaced by the scanner yet.)
    fn create_identifier_node(
        &mut self,
        is_identifier: bool,
        diagnostic_message: Option<&'static DiagnosticMessage>,
        private_identifier_diagnostic_message: Option<&'static DiagnosticMessage>,
    ) -> NodeId {
        if is_identifier {
            return self.parse_identifier();
        }
        if self.token() == SyntaxKind::PrivateIdentifier {
            self.parse_error_at_current_token(
                private_identifier_diagnostic_message
                    .unwrap_or(&gen::Private_identifiers_are_not_allowed_outside_class_bodies),
                &[],
            );
            return self.create_identifier_node(true, None, None);
        }
        let report_at_current_position = self.token() == SyntaxKind::EndOfFileToken;
        let is_reserved_word = self.token().value() >= SyntaxKind::FirstReservedWord.value()
            && self.token().value() <= SyntaxKind::LastReservedWord.value();
        let msg_arg = self.current_token_text();
        let default_message = if is_reserved_word {
            &gen::Identifier_expected_0_is_a_reserved_word_that_cannot_be_used_here
        } else {
            &gen::Identifier_expected
        };
        self.create_missing_node(
            SyntaxKind::Identifier,
            report_at_current_position,
            Some(diagnostic_message.unwrap_or(default_message)),
            &[&msg_arg],
        )
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
        let context_flags = self.context_flags;
        let parsing_context = self.parsing_context;
        let result = f(self);
        self.scanner.restore(scanner_state);
        self.parse_diagnostics.truncate(diagnostics_len);
        self.parse_error_before_next_finished_node = parse_error_before_next_finished_node;
        self.context_flags = context_flags;
        self.parsing_context = parsing_context;
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
                let start_pos = self.scanner.full_start_pos();
                if let Some(element) = parse_element(self) {
                    list.push(element);
                    if start_pos == self.scanner.full_start_pos() {
                        self.next_token();
                    }
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
        parse_element: impl FnMut(&mut Self) -> Option<NodeId>,
        consider_semicolon_as_delimiter: bool,
    ) -> crate::NodeArrayId {
        self.parse_delimited_list_worker(context, parse_element, consider_semicolon_as_delimiter)
            .expect("non-speculative element parsers always yield an element")
    }

    /// tsc parseDelimitedList: `None` when an element parser aborts,
    /// which only speculative parsers (parseParameterForSpeculation) do.
    fn parse_delimited_list_worker(
        &mut self,
        context: ParsingContext,
        mut parse_element: impl FnMut(&mut Self) -> Option<NodeId>,
        consider_semicolon_as_delimiter: bool,
    ) -> Option<crate::NodeArrayId> {
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
                    return None;
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
        Some(
            self.arena
                .alloc_array(list, list_pos, self.node_pos(), comma_start.is_some()),
        )
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

    /// tsc isIdentifier: contextual keywords count, reserved words do not.
    fn is_identifier(&self) -> bool {
        if self.token() == SyntaxKind::Identifier {
            return true;
        }
        if self.token() == SyntaxKind::YieldKeyword && self.in_yield_context() {
            return false;
        }
        if self.token() == SyntaxKind::AwaitKeyword && self.in_await_context() {
            return false;
        }
        self.token().value() > SyntaxKind::LastReservedWord.value()
    }

    fn is_identifier_or_keyword_or_literal(&self) -> bool {
        token_is_identifier_or_keyword(self.token())
            || matches!(
                self.token(),
                SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral | SyntaxKind::StringLiteral
            )
    }

    /// tsc isTypeMemberStart. Advances the scanner; call under look_ahead.
    fn is_type_member_start(&mut self) -> bool {
        if matches!(
            self.token(),
            SyntaxKind::OpenParenToken
                | SyntaxKind::LessThanToken
                | SyntaxKind::GetKeyword
                | SyntaxKind::SetKeyword
        ) {
            return true;
        }
        let mut id_token = false;
        while self.is_modifier_kind(self.token()) {
            id_token = true;
            self.next_token();
        }
        if self.token() == SyntaxKind::OpenBracketToken {
            return true;
        }
        if self.is_literal_property_name() {
            id_token = true;
            self.next_token();
        }
        if id_token {
            return matches!(
                self.token(),
                SyntaxKind::OpenParenToken
                    | SyntaxKind::LessThanToken
                    | SyntaxKind::QuestionToken
                    | SyntaxKind::ColonToken
                    | SyntaxKind::CommaToken
            ) || self.can_parse_semicolon();
        }
        false
    }

    /// tsc isClassMemberStart. Advances the scanner; call under look_ahead.
    fn is_class_member_start(&mut self) -> bool {
        let mut id_token = None;
        if self.token() == SyntaxKind::AtToken {
            return true;
        }
        while self.is_modifier_kind(self.token()) {
            id_token = Some(self.token());
            if is_class_member_modifier(self.token()) {
                return true;
            }
            self.next_token();
        }
        if self.token() == SyntaxKind::AsteriskToken {
            return true;
        }
        if self.is_literal_property_name() {
            id_token = Some(self.token());
            self.next_token();
        }
        if self.token() == SyntaxKind::OpenBracketToken {
            return true;
        }
        if let Some(id_token) = id_token {
            if !is_keyword(id_token)
                || matches!(id_token, SyntaxKind::SetKeyword | SyntaxKind::GetKeyword)
            {
                return true;
            }
            match self.token() {
                SyntaxKind::OpenParenToken
                | SyntaxKind::LessThanToken
                | SyntaxKind::ExclamationToken
                | SyntaxKind::ColonToken
                | SyntaxKind::EqualsToken
                | SyntaxKind::QuestionToken => true,
                _ => self.can_parse_semicolon(),
            }
        } else {
            false
        }
    }

    fn is_start_of_parameter(&mut self, is_jsdoc_parameter: bool) -> bool {
        self.token() == SyntaxKind::DotDotDotToken
            || self.is_binding_identifier_or_private_identifier_or_pattern()
            || self.is_modifier_kind(self.token())
            || self.token() == SyntaxKind::AtToken
            || self.is_start_of_type(!is_jsdoc_parameter)
    }

    fn is_start_of_type(&mut self, in_start_of_parameter: bool) -> bool {
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
            SyntaxKind::MinusToken => {
                !in_start_of_parameter
                    && self.look_ahead(|parser| parser.next_token_is_numeric_or_big_int_literal())
            }
            SyntaxKind::OpenParenToken => {
                !in_start_of_parameter
                    && self.look_ahead(|parser| parser.is_start_of_parenthesized_or_function_type())
            }
            _ => self.is_identifier(),
        }
    }

    fn is_start_of_parenthesized_or_function_type(&mut self) -> bool {
        self.next_token();
        self.token() == SyntaxKind::CloseParenToken
            || self.is_start_of_parameter(false)
            || self.is_start_of_type(false)
    }

    fn next_token_is_numeric_or_big_int_literal(&mut self) -> bool {
        self.next_token();
        matches!(
            self.token(),
            SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
        )
    }

    fn is_start_of_left_hand_side_expression(&mut self) -> bool {
        if self.token() == SyntaxKind::ImportKeyword {
            return self.look_ahead(|parser| parser.next_token_is_open_paren_or_less_than_or_dot());
        }
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

    fn next_token_is_open_paren_or_less_than_or_dot(&mut self) -> bool {
        matches!(
            self.next_token(),
            SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken | SyntaxKind::DotToken
        )
    }

    fn is_start_of_expression(&mut self) -> bool {
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
        ) || self.is_binary_operator()
            || self.is_identifier()
    }

    /// tsc isBinaryOperator: precedence-driven, with the DisallowIn guard —
    /// NOT the FirstBinaryOperator..LastBinaryOperator kind range, which
    /// also spans `?`, `:`, `=>`, and `@`.
    fn is_binary_operator(&self) -> bool {
        if self.in_disallow_in_context() && self.token() == SyntaxKind::InKeyword {
            return false;
        }
        get_binary_operator_precedence(self.token()) > 0
    }

    fn is_start_of_statement(&mut self) -> bool {
        match self.token() {
            SyntaxKind::ImportKeyword => {
                self.is_start_of_declaration()
                    || self
                        .look_ahead(|parser| parser.next_token_is_open_paren_or_less_than_or_dot())
            }
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
            | SyntaxKind::FinallyKeyword => true,
            SyntaxKind::ConstKeyword | SyntaxKind::ExportKeyword => self.is_start_of_declaration(),
            SyntaxKind::AsyncKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword
            | SyntaxKind::TypeKeyword
            | SyntaxKind::GlobalKeyword
            | SyntaxKind::DeferKeyword => true,
            SyntaxKind::AccessorKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::ReadonlyKeyword => {
                self.is_start_of_declaration()
                    || !self.look_ahead(|parser| {
                        parser.next_token_is_identifier_or_keyword_on_same_line()
                    })
            }
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
                | SyntaxKind::DefaultKeyword
                | SyntaxKind::ExportKeyword
                | SyntaxKind::InKeyword
                | SyntaxKind::PrivateKeyword
                | SyntaxKind::ProtectedKeyword
                | SyntaxKind::PublicKeyword
                | SyntaxKind::ReadonlyKeyword
                | SyntaxKind::StaticKeyword
                | SyntaxKind::OutKeyword
                | SyntaxKind::OverrideKeyword
        )
    }

    fn parse_statement(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::SemicolonToken => self.parse_empty_statement(),
            SyntaxKind::OpenBraceToken => self.parse_block(false, None),
            SyntaxKind::VarKeyword => self.parse_variable_statement(self.node_pos(), None),
            SyntaxKind::LetKeyword if self.is_let_declaration() => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::AwaitKeyword if self.is_await_using_declaration() => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::UsingKeyword if self.is_using_declaration() => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::FunctionKeyword => self.parse_function_declaration(self.node_pos(), None),
            SyntaxKind::ClassKeyword => self.parse_class_declaration(self.node_pos(), None),
            SyntaxKind::IfKeyword => self.parse_if_statement(),
            SyntaxKind::DoKeyword => self.parse_do_statement(),
            SyntaxKind::WhileKeyword => self.parse_while_statement(),
            SyntaxKind::ForKeyword => self.parse_for_or_for_in_or_for_of_statement(),
            SyntaxKind::ContinueKeyword => self.parse_break_or_continue_statement(false),
            SyntaxKind::BreakKeyword => self.parse_break_or_continue_statement(true),
            SyntaxKind::ReturnKeyword => self.parse_return_statement(),
            SyntaxKind::WithKeyword => self.parse_with_statement(),
            SyntaxKind::SwitchKeyword => self.parse_switch_statement(),
            SyntaxKind::ThrowKeyword => self.parse_throw_statement(),
            SyntaxKind::TryKeyword | SyntaxKind::CatchKeyword | SyntaxKind::FinallyKeyword => {
                self.parse_try_statement()
            }
            SyntaxKind::DebuggerKeyword => self.parse_debugger_statement(),
            SyntaxKind::AtToken => self.parse_declaration(),
            SyntaxKind::AsyncKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::TypeKeyword
            | SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword
            | SyntaxKind::DeclareKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::EnumKeyword
            | SyntaxKind::ExportKeyword
            | SyntaxKind::ImportKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::PublicKeyword
            | SyntaxKind::AbstractKeyword
            | SyntaxKind::AccessorKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::GlobalKeyword
                if self.is_start_of_declaration() =>
            {
                self.parse_declaration()
            }
            _ => self.parse_expression_or_labeled_statement(),
        }
    }

    /// tsc parseDeclaration.
    fn parse_declaration(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers(true, false, false);
        if self.modifiers_contain(modifiers, SyntaxKind::DeclareKeyword) {
            self.mark_modifiers_ambient(modifiers);
            self.do_in_context(NodeFlags::AMBIENT, NodeFlags::NONE, |parser| {
                parser.parse_declaration_worker(pos, modifiers)
            })
        } else {
            self.parse_declaration_worker(pos, modifiers)
        }
    }

    fn mark_modifiers_ambient(&mut self, modifiers: Option<crate::NodeArrayId>) {
        if let Some(list) = modifiers {
            let nodes = self.arena.node_array(list).nodes.clone();
            for modifier in nodes {
                self.arena.node_mut(modifier).flags |= NodeFlags::AMBIENT.bits();
            }
        }
    }

    fn parse_declaration_worker(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        match self.token() {
            SyntaxKind::VarKeyword
            | SyntaxKind::LetKeyword
            | SyntaxKind::ConstKeyword
            | SyntaxKind::UsingKeyword => {
                return self.parse_variable_statement(pos, modifiers);
            }
            SyntaxKind::AwaitKeyword if self.is_await_using_declaration() => {
                return self.parse_variable_statement(pos, modifiers);
            }
            SyntaxKind::FunctionKeyword => return self.parse_function_declaration(pos, modifiers),
            SyntaxKind::ClassKeyword => return self.parse_class_declaration(pos, modifiers),
            SyntaxKind::InterfaceKeyword => {
                return self.parse_interface_declaration(pos, modifiers);
            }
            SyntaxKind::TypeKeyword => return self.parse_type_alias_declaration(pos, modifiers),
            SyntaxKind::EnumKeyword => return self.parse_enum_declaration(pos, modifiers),
            SyntaxKind::GlobalKeyword
            | SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword => {
                return self.parse_module_declaration(pos, modifiers);
            }
            SyntaxKind::ImportKeyword => {
                return self.parse_import_declaration_or_import_equals_declaration(pos, modifiers);
            }
            SyntaxKind::ExportKeyword => {
                self.next_token();
                return match self.token() {
                    SyntaxKind::DefaultKeyword | SyntaxKind::EqualsToken => {
                        self.parse_export_assignment(pos, modifiers)
                    }
                    SyntaxKind::AsKeyword => {
                        self.parse_namespace_export_declaration(pos, modifiers)
                    }
                    _ => self.parse_export_declaration(pos, modifiers),
                };
            }
            _ => {}
        }
        // tsc returns undefined when nothing was parsed and there are no
        // modifiers; isStartOfDeclaration makes that unreachable, so always
        // recover with the MissingDeclaration node.
        let missing = self.create_missing_node(
            SyntaxKind::MissingDeclaration,
            true,
            Some(&gen::Declaration_expected),
            &[],
        );
        let node = self.arena.node_mut(missing);
        node.pos = pos as u32;
        if let NodeData::MissingDeclaration(data) = &mut node.data {
            data.modifiers = modifiers;
        }
        missing
    }

    fn parse_function_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let saved_await_context = self.in_await_context();
        self.parse_expected(SyntaxKind::FunctionKeyword, None);
        let asterisk_token = self.parse_optional_token(SyntaxKind::AsteriskToken);
        // `export default function` may omit the name.
        let name = if self.modifiers_contain(modifiers, SyntaxKind::DefaultKeyword) {
            if self.is_binding_identifier() {
                Some(self.parse_binding_identifier())
            } else {
                None
            }
        } else {
            Some(self.parse_binding_identifier())
        };
        let is_generator = asterisk_token.is_some();
        let is_async = self.modifiers_contain(modifiers, SyntaxKind::AsyncKeyword);
        let type_parameters = self.parse_type_parameters();
        if self.modifiers_contain(modifiers, SyntaxKind::ExportKeyword) {
            self.set_await_context(true);
        }
        let parameters = self.parse_parameters(is_generator, is_async);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(
            is_generator,
            is_async,
            false,
            Some(&gen::or_expected),
        );
        self.set_await_context(saved_await_context);
        self.finish_node_data(
            NodeData::FunctionDeclaration(FunctionDeclarationData {
                modifiers,
                asterisk_token,
                name,
                r#type,
                type_parameters,
                parameters: Some(parameters),
                body,
            }),
            pos,
        )
    }

    fn parse_constructor_name(&mut self) -> bool {
        if self.token() == SyntaxKind::ConstructorKeyword {
            return self.parse_expected(SyntaxKind::ConstructorKeyword, None);
        }
        if self.token() == SyntaxKind::StringLiteral
            && self.look_ahead(|parser| parser.next_token()) == SyntaxKind::OpenParenToken
        {
            return self.try_parse(|parser| {
                let literal = parser.parse_string_literal();
                match &parser.arena.node(literal).data {
                    NodeData::StringLiteral(data) => data.text == "constructor",
                    _ => false,
                }
            });
        }
        false
    }

    fn try_parse_constructor_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> Option<NodeId> {
        self.try_parse(|parser| {
            if !parser.parse_constructor_name() {
                return None;
            }
            let type_parameters = parser.parse_type_parameters();
            let parameters = parser.parse_parameters(false, false);
            let r#type = parser.parse_return_type(SyntaxKind::ColonToken, false);
            let body = parser.parse_function_block_or_semicolon(
                false,
                false,
                false,
                Some(&gen::or_expected),
            );
            Some(parser.finish_node_data(
                NodeData::Constructor(ConstructorData {
                    modifiers,
                    name: None,
                    r#type,
                    type_parameters,
                    parameters: Some(parameters),
                    body,
                }),
                pos,
            ))
        })
    }

    fn parse_property_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        name: NodeId,
        question_token: Option<NodeId>,
    ) -> NodeId {
        let exclamation_token =
            if question_token.is_none() && !self.scanner.has_preceding_line_break() {
                self.parse_optional_token(SyntaxKind::ExclamationToken)
            } else {
                None
            };
        let r#type = self.parse_type_annotation();
        let initializer = self.do_in_context(
            NodeFlags::NONE,
            NodeFlags::from_bits(
                NodeFlags::YIELD_CONTEXT.bits()
                    | NodeFlags::AWAIT_CONTEXT.bits()
                    | NodeFlags::DISALLOW_IN_CONTEXT.bits(),
            ),
            |parser| parser.parse_initializer(),
        );
        self.parse_semicolon_after_property_name(name, r#type, initializer);
        self.finish_node_data(
            NodeData::PropertyDeclaration(PropertyDeclarationData {
                modifiers,
                name: Some(name),
                question_token,
                exclamation_token,
                r#type,
                initializer,
            }),
            pos,
        )
    }

    fn parse_property_or_method_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let asterisk_token = self.parse_optional_token(SyntaxKind::AsteriskToken);
        let name = self.parse_property_name();
        let question_token = self.parse_optional_token(SyntaxKind::QuestionToken);
        if asterisk_token.is_some()
            || matches!(
                self.token(),
                SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken
            )
        {
            return self.parse_method_declaration(
                pos,
                modifiers,
                asterisk_token,
                name,
                question_token,
                None,
                Some(&gen::or_expected),
            );
        }
        self.parse_property_declaration(pos, modifiers, name, question_token)
    }

    fn parse_class_static_block_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::StaticKeyword, None);
        let body = self.parse_class_static_block_body();
        self.finish_node_data(
            NodeData::ClassStaticBlockDeclaration(ClassStaticBlockDeclarationData {
                modifiers,
                body: Some(body),
            }),
            pos,
        )
    }

    fn parse_class_static_block_body(&mut self) -> NodeId {
        let saved_yield_context = self.in_yield_context();
        let saved_await_context = self.in_await_context();
        self.set_yield_context(false);
        self.set_await_context(true);
        let body = self.parse_block(false, None);
        self.set_yield_context(saved_yield_context);
        self.set_await_context(saved_await_context);
        body
    }

    fn parse_class_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.token() == SyntaxKind::SemicolonToken {
            self.next_token();
            return self.finish_kind_only_node(SyntaxKind::SemicolonClassElement, pos);
        }
        let modifiers = self.parse_modifiers(true, true, true);
        if self.token() == SyntaxKind::StaticKeyword
            && self.look_ahead(|parser| parser.next_token_is_open_brace())
        {
            return self.parse_class_static_block_declaration(pos, modifiers);
        }
        if self.parse_contextual_modifier(SyntaxKind::GetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, SyntaxKind::GetAccessor, false);
        }
        if self.parse_contextual_modifier(SyntaxKind::SetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, SyntaxKind::SetAccessor, false);
        }
        if self.token() == SyntaxKind::ConstructorKeyword
            || self.token() == SyntaxKind::StringLiteral
        {
            if let Some(constructor) = self.try_parse_constructor_declaration(pos, modifiers) {
                return constructor;
            }
        }
        if self.is_index_signature() {
            return self.parse_index_signature_declaration(pos, modifiers);
        }
        if token_is_identifier_or_keyword(self.token())
            || matches!(
                self.token(),
                SyntaxKind::StringLiteral
                    | SyntaxKind::NumericLiteral
                    | SyntaxKind::BigIntLiteral
                    | SyntaxKind::AsteriskToken
                    | SyntaxKind::OpenBracketToken
            )
        {
            if self.modifiers_contain(modifiers, SyntaxKind::DeclareKeyword) {
                self.mark_modifiers_ambient(modifiers);
                return self.do_in_context(NodeFlags::AMBIENT, NodeFlags::NONE, |parser| {
                    parser.parse_property_or_method_declaration(pos, modifiers)
                });
            }
            return self.parse_property_or_method_declaration(pos, modifiers);
        }
        if modifiers.is_some() {
            let name = self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Declaration_expected),
                &[],
            );
            return self.parse_property_declaration(pos, modifiers, name, None);
        }
        // tsc Debug.fail: isClassMemberStart vetted the lookahead.
        debug_assert!(
            false,
            "should not have attempted to parse class member declaration"
        );
        let name = self.create_missing_node(
            SyntaxKind::Identifier,
            true,
            Some(&gen::Declaration_expected),
            &[],
        );
        self.parse_property_declaration(pos, None, name, None)
    }

    fn parse_class_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_class_declaration_or_expression(pos, None, false)
    }

    fn parse_class_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_class_declaration_or_expression(pos, modifiers, true)
    }

    fn parse_class_declaration_or_expression(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        is_declaration: bool,
    ) -> NodeId {
        let saved_await_context = self.in_await_context();
        self.parse_expected(SyntaxKind::ClassKeyword, None);
        let name = self.parse_name_of_class_declaration_or_expression();
        let type_parameters = self.parse_type_parameters();
        if self.modifiers_contain(modifiers, SyntaxKind::ExportKeyword) {
            self.set_await_context(true);
        }
        let heritage_clauses = self.parse_heritage_clauses();
        let members = if self.parse_expected(SyntaxKind::OpenBraceToken, None) {
            let members = self.parse_class_members();
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
            members
        } else {
            self.arena.missing_array(self.node_pos())
        };
        self.set_await_context(saved_await_context);
        let data = if is_declaration {
            NodeData::ClassDeclaration(ClassDeclarationData {
                modifiers,
                name,
                type_parameters,
                heritage_clauses,
                members: Some(members),
            })
        } else {
            NodeData::ClassExpression(ClassExpressionData {
                modifiers,
                name,
                type_parameters,
                heritage_clauses,
                members: Some(members),
            })
        };
        self.finish_node_data(data, pos)
    }

    fn parse_name_of_class_declaration_or_expression(&mut self) -> Option<NodeId> {
        if self.is_binding_identifier() && !self.is_implements_clause() {
            Some(self.parse_identifier())
        } else {
            None
        }
    }

    fn is_implements_clause(&mut self) -> bool {
        self.token() == SyntaxKind::ImplementsKeyword
            && self.look_ahead(|parser| {
                parser.next_token();
                token_is_identifier_or_keyword(parser.token())
            })
    }

    fn parse_heritage_clauses(&mut self) -> Option<crate::NodeArrayId> {
        if self.is_heritage_clause() {
            Some(self.parse_list(ParsingContext::HeritageClauses, |parser| {
                Some(parser.parse_heritage_clause())
            }))
        } else {
            None
        }
    }

    /// The clause keyword (extends vs implements) is recoverable from the
    /// source range; HeritageClauseData does not store it.
    fn parse_heritage_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        debug_assert!(matches!(
            self.token(),
            SyntaxKind::ExtendsKeyword | SyntaxKind::ImplementsKeyword
        ));
        self.next_token();
        let types = self.parse_delimited_list(
            ParsingContext::HeritageClauseElement,
            |parser| Some(parser.parse_expression_with_type_arguments()),
            false,
        );
        self.finish_node_data(
            NodeData::HeritageClause(HeritageClauseData { types: Some(types) }),
            pos,
        )
    }

    fn parse_expression_with_type_arguments(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_left_hand_side_expression_or_higher();
        if self.arena.node(expression).kind == SyntaxKind::ExpressionWithTypeArguments {
            return expression;
        }
        let type_arguments = self.try_parse_type_arguments();
        self.finish_node_data(
            NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                expression: Some(expression),
                type_arguments,
            }),
            pos,
        )
    }

    fn parse_class_members(&mut self) -> crate::NodeArrayId {
        self.parse_list(ParsingContext::ClassMembers, |parser| {
            Some(parser.parse_class_element())
        })
    }

    fn parse_interface_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::InterfaceKeyword, None);
        let name = self.parse_identifier_or_missing();
        let type_parameters = self.parse_type_parameters();
        let heritage_clauses = self.parse_heritage_clauses();
        let members = self.parse_object_type_members();
        self.finish_node_data(
            NodeData::InterfaceDeclaration(InterfaceDeclarationData {
                modifiers,
                name: Some(name),
                type_parameters,
                heritage_clauses,
                members: Some(members),
            }),
            pos,
        )
    }

    fn parse_type_alias_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::TypeKeyword, None);
        if self.scanner.has_preceding_line_break() {
            self.parse_error_at_current_token(&gen::Line_break_not_permitted_here, &[]);
        }
        let name = self.parse_identifier_or_missing();
        let type_parameters = self.parse_type_parameters();
        self.parse_expected(SyntaxKind::EqualsToken, None);
        let r#type = if self.token() == SyntaxKind::IntrinsicKeyword {
            match self.try_parse(|parser| parser.parse_keyword_and_no_dot()) {
                Some(node) => node,
                None => self.parse_type(),
            }
        } else {
            self.parse_type()
        };
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::TypeAliasDeclaration(TypeAliasDeclarationData {
                modifiers,
                name: Some(name),
                r#type: Some(r#type),
                type_parameters,
            }),
            pos,
        )
    }

    fn parse_enum_member(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_property_name();
        let initializer = self.allow_in(|parser| parser.parse_initializer());
        self.finish_node_data(
            NodeData::EnumMember(EnumMemberData {
                name: Some(name),
                initializer,
            }),
            pos,
        )
    }

    fn parse_enum_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::EnumKeyword, None);
        let name = self.parse_identifier_or_missing();
        let members = if self.parse_expected(SyntaxKind::OpenBraceToken, None) {
            let members = self.do_in_context(
                NodeFlags::NONE,
                NodeFlags::from_bits(
                    NodeFlags::YIELD_CONTEXT.bits() | NodeFlags::AWAIT_CONTEXT.bits(),
                ),
                |parser| {
                    parser.parse_delimited_list(
                        ParsingContext::EnumMembers,
                        |parser| Some(parser.parse_enum_member()),
                        false,
                    )
                },
            );
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
            members
        } else {
            self.arena.missing_array(self.node_pos())
        };
        self.finish_node_data(
            NodeData::EnumDeclaration(EnumDeclarationData {
                modifiers,
                name: Some(name),
                members: Some(members),
            }),
            pos,
        )
    }

    fn parse_module_block(&mut self) -> NodeId {
        let pos = self.node_pos();
        let statements = if self.parse_expected(SyntaxKind::OpenBraceToken, None) {
            let statements = self.parse_list(ParsingContext::BlockStatements, |parser| {
                Some(parser.parse_statement())
            });
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
            statements
        } else {
            self.arena.missing_array(self.node_pos())
        };
        self.finish_node_data(
            NodeData::ModuleBlock(ModuleBlockData {
                statements: Some(statements),
            }),
            pos,
        )
    }

    /// tsc parseModuleOrNamespaceDeclaration: `namespace a.b.c` desugars into
    /// nested module declarations via the recursive dotted-name walk.
    fn parse_module_or_namespace_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        flags: NodeFlags,
    ) -> NodeId {
        let namespace_flag = flags.bits() & NodeFlags::NAMESPACE.bits();
        let name = if flags.bits() & NodeFlags::NESTED_NAMESPACE.bits() != 0 {
            self.parse_identifier_name(None)
        } else {
            self.parse_identifier_or_missing()
        };
        let body = if self.parse_optional(SyntaxKind::DotToken) {
            let nested_pos = self.node_pos();
            let nested_flags =
                NodeFlags::from_bits(NodeFlags::NESTED_NAMESPACE.bits() | namespace_flag);
            self.parse_module_or_namespace_declaration(nested_pos, None, nested_flags)
        } else {
            self.parse_module_block()
        };
        self.finish_node_with_flags(
            NodeData::ModuleDeclaration(ModuleDeclarationData {
                modifiers,
                name: Some(name),
                body: Some(body),
            }),
            pos,
            flags,
        )
    }

    fn parse_ambient_external_module_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let mut flags = NodeFlags::NONE;
        let name = if self.token() == SyntaxKind::GlobalKeyword {
            flags |= NodeFlags::GLOBAL_AUGMENTATION;
            self.parse_identifier_or_missing()
        } else {
            self.parse_string_literal()
        };
        let body = if self.token() == SyntaxKind::OpenBraceToken {
            Some(self.parse_module_block())
        } else {
            self.parse_semicolon();
            None
        };
        self.finish_node_with_flags(
            NodeData::ModuleDeclaration(ModuleDeclarationData {
                modifiers,
                name: Some(name),
                body,
            }),
            pos,
            flags,
        )
    }

    fn parse_module_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        if self.token() == SyntaxKind::GlobalKeyword {
            return self.parse_ambient_external_module_declaration(pos, modifiers);
        }
        let mut flags = NodeFlags::NONE;
        if self.parse_optional(SyntaxKind::NamespaceKeyword) {
            flags |= NodeFlags::NAMESPACE;
        } else {
            self.parse_expected(SyntaxKind::ModuleKeyword, None);
            if self.token() == SyntaxKind::StringLiteral {
                return self.parse_ambient_external_module_declaration(pos, modifiers);
            }
        }
        self.parse_module_or_namespace_declaration(pos, modifiers, flags)
    }

    fn is_external_module_reference(&mut self) -> bool {
        self.token() == SyntaxKind::RequireKeyword
            && self.look_ahead(|parser| parser.next_token_is_open_paren())
    }

    fn parse_namespace_export_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::AsKeyword, None);
        self.parse_expected(SyntaxKind::NamespaceKeyword, None);
        let name = self.parse_identifier_or_missing();
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::NamespaceExportDeclaration(NamespaceExportDeclarationData {
                modifiers,
                name: Some(name),
            }),
            pos,
        )
    }

    fn identifier_text_is(&self, id: Option<NodeId>, text: &str) -> bool {
        id.is_some_and(|id| match &self.arena.node(id).data {
            NodeData::Identifier(data) => data.escaped_text == text,
            _ => false,
        })
    }

    /// tsc parseImportDeclarationOrImportEqualsDeclaration. The type-only and
    /// defer phase modifiers steer the grammar but have no node-data slot yet.
    fn parse_import_declaration_or_import_equals_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::ImportKeyword, None);
        let after_import_pos = self.scanner.full_start_pos();
        let mut identifier = if self.is_identifier() {
            Some(self.parse_identifier())
        } else {
            None
        };
        let mut is_type_only = false;
        let mut is_defer_phase = false;
        if self.identifier_text_is(identifier, "type")
            && (self.token() != SyntaxKind::FromKeyword
                || self.is_identifier()
                    && self
                        .look_ahead(|parser| parser.next_token_is_from_keyword_or_equals_token()))
            && (self.is_identifier()
                || self.token_after_import_definitely_produces_import_declaration())
        {
            is_type_only = true;
            identifier = if self.is_identifier() {
                Some(self.parse_identifier())
            } else {
                None
            };
        } else if self.identifier_text_is(identifier, "defer")
            && (if self.token() == SyntaxKind::FromKeyword {
                !self.look_ahead(|parser| parser.next_token_is_string_literal())
            } else {
                self.token() != SyntaxKind::CommaToken && self.token() != SyntaxKind::EqualsToken
            })
        {
            is_defer_phase = true;
            identifier = if self.is_identifier() {
                Some(self.parse_identifier())
            } else {
                None
            };
        }
        if let Some(identifier) = identifier {
            if !self.token_after_imported_identifier_definitely_produces_import_declaration()
                && !is_defer_phase
            {
                return self.parse_import_equals_declaration(pos, modifiers, identifier);
            }
        }
        let import_clause =
            self.try_parse_import_clause(identifier, after_import_pos, is_type_only);
        let module_specifier = self.parse_module_specifier();
        let attributes = self.try_parse_import_attributes();
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::ImportDeclaration(ImportDeclarationData {
                modifiers,
                import_clause,
                module_specifier: Some(module_specifier),
                attributes,
            }),
            pos,
        )
    }

    fn next_token_is_string_literal(&mut self) -> bool {
        self.next_token() == SyntaxKind::StringLiteral
    }

    fn next_token_is_from_keyword_or_equals_token(&mut self) -> bool {
        self.next_token();
        matches!(
            self.token(),
            SyntaxKind::FromKeyword | SyntaxKind::EqualsToken
        )
    }

    fn token_after_import_definitely_produces_import_declaration(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::AsteriskToken | SyntaxKind::OpenBraceToken
        )
    }

    fn token_after_imported_identifier_definitely_produces_import_declaration(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::CommaToken | SyntaxKind::FromKeyword
        )
    }

    fn try_parse_import_clause(
        &mut self,
        identifier: Option<NodeId>,
        pos: usize,
        is_type_only: bool,
    ) -> Option<NodeId> {
        if identifier.is_some()
            || matches!(
                self.token(),
                SyntaxKind::AsteriskToken | SyntaxKind::OpenBraceToken
            )
        {
            let import_clause = self.parse_import_clause(identifier, pos, is_type_only);
            self.parse_expected(SyntaxKind::FromKeyword, None);
            Some(import_clause)
        } else {
            None
        }
    }

    fn parse_import_clause(
        &mut self,
        identifier: Option<NodeId>,
        pos: usize,
        is_type_only: bool,
    ) -> NodeId {
        let named_bindings = if identifier.is_none() || self.parse_optional(SyntaxKind::CommaToken)
        {
            Some(if self.token() == SyntaxKind::AsteriskToken {
                self.parse_namespace_import()
            } else {
                self.parse_named_imports_or_exports(true)
            })
        } else {
            None
        };
        self.finish_node_data(
            NodeData::ImportClause(ImportClauseData {
                is_type_only,
                name: identifier,
                named_bindings,
            }),
            pos,
        )
    }

    /// tsc parseImportEqualsDeclaration (isTypeOnly has no node-data slot).
    fn parse_import_equals_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        identifier: NodeId,
    ) -> NodeId {
        self.parse_expected(SyntaxKind::EqualsToken, None);
        let module_reference = self.parse_module_reference();
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::ImportEqualsDeclaration(ImportEqualsDeclarationData {
                modifiers,
                name: Some(identifier),
                module_reference: Some(module_reference),
            }),
            pos,
        )
    }

    fn parse_module_reference(&mut self) -> NodeId {
        if self.is_external_module_reference() {
            self.parse_external_module_reference()
        } else {
            self.parse_entity_name(false, None)
        }
    }

    fn parse_external_module_reference(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::RequireKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = self.parse_module_specifier();
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        self.finish_node_data(
            NodeData::ExternalModuleReference(ExternalModuleReferenceData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_module_specifier(&mut self) -> NodeId {
        if self.token() == SyntaxKind::StringLiteral {
            self.parse_string_literal()
        } else {
            self.parse_expression()
        }
    }

    fn parse_namespace_import(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::AsteriskToken, None);
        self.parse_expected(SyntaxKind::AsKeyword, None);
        let name = self.parse_identifier_or_missing();
        self.finish_node_data(
            NodeData::NamespaceImport(NamespaceImportData { name: Some(name) }),
            pos,
        )
    }

    fn can_parse_module_export_name(&self) -> bool {
        token_is_identifier_or_keyword(self.token()) || self.token() == SyntaxKind::StringLiteral
    }

    fn parse_module_export_name_plain(&mut self) -> NodeId {
        if self.token() == SyntaxKind::StringLiteral {
            self.parse_string_literal()
        } else {
            self.parse_identifier_name(None)
        }
    }

    fn parse_module_export_name_checked(
        &mut self,
        check_identifier_is_keyword: &mut bool,
        check_identifier_start: &mut usize,
        check_identifier_end: &mut usize,
    ) -> NodeId {
        if self.token() == SyntaxKind::StringLiteral {
            return self.parse_string_literal();
        }
        *check_identifier_is_keyword = is_keyword(self.token()) && !self.is_identifier();
        *check_identifier_start = self.scanner.token_start();
        *check_identifier_end = self.scanner.pos();
        self.parse_identifier_name(None)
    }

    fn parse_named_imports_or_exports(&mut self, is_imports: bool) -> NodeId {
        let pos = self.node_pos();
        let elements = self.parse_bracketed_list(
            ParsingContext::ImportOrExportSpecifiers,
            |parser| Some(parser.parse_import_or_export_specifier(is_imports)),
            SyntaxKind::OpenBraceToken,
            SyntaxKind::CloseBraceToken,
        );
        let data = if is_imports {
            NodeData::NamedImports(NamedImportsData {
                elements: Some(elements),
            })
        } else {
            NodeData::NamedExports(NamedExportsData {
                elements: Some(elements),
            })
        };
        self.finish_node_data(data, pos)
    }

    /// tsc parseImportOrExportSpecifier.
    fn parse_import_or_export_specifier(&mut self, is_import_specifier: bool) -> NodeId {
        let pos = self.node_pos();
        let mut check_identifier_is_keyword = is_keyword(self.token()) && !self.is_identifier();
        let mut check_identifier_start = self.scanner.token_start();
        let mut check_identifier_end = self.scanner.pos();
        let mut is_type_only = false;
        let mut property_name: Option<NodeId> = None;
        let mut can_parse_as_keyword = true;
        let mut name = self.parse_module_export_name_plain();
        if self.arena.node(name).kind == SyntaxKind::Identifier
            && self.identifier_text_is(Some(name), "type")
        {
            if self.token() == SyntaxKind::AsKeyword {
                // `{ type as ... }`: which of type/as is the name depends on
                // what follows the second `as`, if any.
                let first_as = self.parse_identifier_name(None);
                if self.token() == SyntaxKind::AsKeyword {
                    let second_as = self.parse_identifier_name(None);
                    if self.can_parse_module_export_name() {
                        is_type_only = true;
                        property_name = Some(first_as);
                        name = self.parse_module_export_name_checked(
                            &mut check_identifier_is_keyword,
                            &mut check_identifier_start,
                            &mut check_identifier_end,
                        );
                        can_parse_as_keyword = false;
                    } else {
                        property_name = Some(name);
                        name = second_as;
                        can_parse_as_keyword = false;
                    }
                } else if self.can_parse_module_export_name() {
                    property_name = Some(name);
                    can_parse_as_keyword = false;
                    name = self.parse_module_export_name_checked(
                        &mut check_identifier_is_keyword,
                        &mut check_identifier_start,
                        &mut check_identifier_end,
                    );
                } else {
                    is_type_only = true;
                    name = first_as;
                }
            } else if self.can_parse_module_export_name() {
                is_type_only = true;
                name = self.parse_module_export_name_checked(
                    &mut check_identifier_is_keyword,
                    &mut check_identifier_start,
                    &mut check_identifier_end,
                );
            }
        }
        if can_parse_as_keyword && self.token() == SyntaxKind::AsKeyword {
            property_name = Some(name);
            self.parse_expected(SyntaxKind::AsKeyword, None);
            name = self.parse_module_export_name_checked(
                &mut check_identifier_is_keyword,
                &mut check_identifier_start,
                &mut check_identifier_end,
            );
        }
        if is_import_specifier {
            if self.arena.node(name).kind != SyntaxKind::Identifier {
                let name_pos = self.arena.node(name).pos as usize;
                let name_end = self.arena.node(name).end as usize;
                let start = crate::scanner::skip_trivia(self.scanner.text(), name_pos);
                self.parse_error_at_position(
                    start,
                    name_end - start,
                    &gen::Identifier_expected,
                    &[],
                );
                let missing = self.create_missing_node(SyntaxKind::Identifier, false, None, &[]);
                let missing_node = self.arena.node_mut(missing);
                missing_node.pos = name_pos as u32;
                missing_node.end = name_pos as u32;
                name = missing;
            } else if check_identifier_is_keyword {
                self.parse_error_at_position(
                    check_identifier_start,
                    check_identifier_end - check_identifier_start,
                    &gen::Identifier_expected,
                    &[],
                );
            }
        }
        let data = if is_import_specifier {
            NodeData::ImportSpecifier(ImportSpecifierData {
                is_type_only,
                property_name,
                name: Some(name),
            })
        } else {
            NodeData::ExportSpecifier(ExportSpecifierData {
                is_type_only,
                property_name,
                name: Some(name),
            })
        };
        self.finish_node_data(data, pos)
    }

    fn parse_namespace_export(&mut self, pos: usize) -> NodeId {
        let name = self.parse_module_export_name_plain();
        self.finish_node_data(
            NodeData::NamespaceExport(NamespaceExportData { name: Some(name) }),
            pos,
        )
    }

    /// tsc parseExportDeclaration (isTypeOnly has no node-data slot).
    fn parse_export_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let saved_await_context = self.in_await_context();
        self.set_await_context(true);
        let mut export_clause = None;
        let mut module_specifier = None;
        let mut attributes = None;
        let is_type_only = self.parse_optional(SyntaxKind::TypeKeyword);
        let namespace_export_pos = self.node_pos();
        if self.parse_optional(SyntaxKind::AsteriskToken) {
            if self.parse_optional(SyntaxKind::AsKeyword) {
                export_clause = Some(self.parse_namespace_export(namespace_export_pos));
            }
            self.parse_expected(SyntaxKind::FromKeyword, None);
            module_specifier = Some(self.parse_module_specifier());
        } else {
            export_clause = Some(self.parse_named_imports_or_exports(false));
            if self.token() == SyntaxKind::FromKeyword
                || self.token() == SyntaxKind::StringLiteral
                    && !self.scanner.has_preceding_line_break()
            {
                self.parse_expected(SyntaxKind::FromKeyword, None);
                module_specifier = Some(self.parse_module_specifier());
            }
        }
        if module_specifier.is_some()
            && matches!(
                self.token(),
                SyntaxKind::WithKeyword | SyntaxKind::AssertKeyword
            )
            && !self.scanner.has_preceding_line_break()
        {
            attributes = Some(self.parse_import_attributes(self.token(), false));
        }
        self.parse_semicolon();
        self.set_await_context(saved_await_context);
        self.finish_node_data(
            NodeData::ExportDeclaration(ExportDeclarationData {
                is_type_only,
                modifiers,
                export_clause,
                module_specifier,
                attributes,
            }),
            pos,
        )
    }

    /// tsc parseExportAssignment.
    fn parse_export_assignment(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let saved_await_context = self.in_await_context();
        self.set_await_context(true);
        let is_export_equals = if self.parse_optional(SyntaxKind::EqualsToken) {
            Some(true)
        } else {
            self.parse_expected(SyntaxKind::DefaultKeyword, None);
            None
        };
        let expression = self.parse_assignment_expression_or_higher(true);
        self.parse_semicolon();
        self.set_await_context(saved_await_context);
        self.finish_node_data(
            NodeData::ExportAssignment(ExportAssignmentData {
                is_export_equals,
                modifiers,
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn try_parse_import_attributes(&mut self) -> Option<NodeId> {
        if self.token() == SyntaxKind::WithKeyword
            || self.token() == SyntaxKind::AssertKeyword && !self.scanner.has_preceding_line_break()
        {
            Some(self.parse_import_attributes(self.token(), false))
        } else {
            None
        }
    }

    fn parse_empty_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::SemicolonToken, None);
        self.finish_node_data(NodeData::EmptyStatement(EmptyStatementData {}), pos)
    }

    fn parse_block(
        &mut self,
        ignore_missing_open_brace: bool,
        diagnostic_message: Option<&'static DiagnosticMessage>,
    ) -> NodeId {
        let pos = self.node_pos();
        let open_brace_parsed = self.parse_expected(SyntaxKind::OpenBraceToken, diagnostic_message);
        let statements = if open_brace_parsed || ignore_missing_open_brace {
            let statements = self.parse_list(ParsingContext::BlockStatements, |parser| {
                Some(parser.parse_statement())
            });
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
            statements
        } else {
            self.arena.empty_array(self.node_pos())
        };

        let block = self.finish_node_data(
            NodeData::Block(BlockData {
                statements: Some(statements),
            }),
            pos,
        );

        if self.token() == SyntaxKind::EqualsToken {
            self.parse_error_at_current_token(
                &gen::Declaration_or_statement_expected_This_follows_a_block_of_statements_so_if_you_intended_to_write_a_destructuring_assignment_you_might_need_to_wrap_the_whole_assignment_in_parentheses,
                &[],
            );
            self.next_token();
        }

        block
    }

    fn parse_if_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::IfKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        let then_statement = self.parse_statement();
        let else_statement = if self.parse_optional(SyntaxKind::ElseKeyword) {
            Some(self.parse_statement())
        } else {
            None
        };
        self.finish_node_data(
            NodeData::IfStatement(IfStatementData {
                expression: Some(expression),
                then_statement: Some(then_statement),
                else_statement,
            }),
            pos,
        )
    }

    fn parse_do_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::DoKeyword, None);
        let statement = self.parse_statement();
        self.parse_expected(SyntaxKind::WhileKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        self.parse_optional(SyntaxKind::SemicolonToken);
        self.finish_node_data(
            NodeData::DoStatement(DoStatementData {
                statement: Some(statement),
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_while_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::WhileKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        let statement = self.parse_statement();
        self.finish_node_data(
            NodeData::WhileStatement(WhileStatementData {
                expression: Some(expression),
                statement: Some(statement),
            }),
            pos,
        )
    }

    fn parse_for_or_for_in_or_for_of_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::ForKeyword, None);
        let await_modifier = self.parse_optional_token(SyntaxKind::AwaitKeyword);
        self.parse_expected(SyntaxKind::OpenParenToken, None);

        let initializer = if self.token() == SyntaxKind::SemicolonToken {
            None
        } else if self.is_variable_statement_start() {
            Some(self.parse_variable_declaration_list(true))
        } else {
            Some(self.disallow_in(|parser| parser.parse_expression()))
        };

        let is_for_of = if await_modifier.is_some() {
            self.parse_expected(SyntaxKind::OfKeyword, None);
            true
        } else {
            self.parse_optional(SyntaxKind::OfKeyword)
        };

        if is_for_of {
            let expression =
                self.allow_in(|parser| parser.parse_assignment_expression_or_higher(true));
            self.parse_expected(SyntaxKind::CloseParenToken, None);
            let statement = self.parse_statement();
            return self.finish_node_data(
                NodeData::ForOfStatement(ForOfStatementData {
                    await_modifier,
                    initializer,
                    expression: Some(expression),
                    statement: Some(statement),
                }),
                pos,
            );
        }

        if self.parse_optional(SyntaxKind::InKeyword) {
            let expression = self.allow_in(|parser| parser.parse_expression());
            self.parse_expected(SyntaxKind::CloseParenToken, None);
            let statement = self.parse_statement();
            return self.finish_node_data(
                NodeData::ForInStatement(ForInStatementData {
                    initializer,
                    expression: Some(expression),
                    statement: Some(statement),
                }),
                pos,
            );
        }

        self.parse_expected(SyntaxKind::SemicolonToken, None);
        let condition = if !matches!(
            self.token(),
            SyntaxKind::SemicolonToken | SyntaxKind::CloseParenToken
        ) {
            Some(self.allow_in(|parser| parser.parse_expression()))
        } else {
            None
        };
        self.parse_expected(SyntaxKind::SemicolonToken, None);
        let incrementor = if self.token() != SyntaxKind::CloseParenToken {
            Some(self.allow_in(|parser| parser.parse_expression()))
        } else {
            None
        };
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        let statement = self.parse_statement();
        self.finish_node_data(
            NodeData::ForStatement(ForStatementData {
                initializer,
                condition,
                incrementor,
                statement: Some(statement),
            }),
            pos,
        )
    }

    fn parse_break_or_continue_statement(&mut self, is_break: bool) -> NodeId {
        let pos = self.node_pos();
        let keyword = if is_break {
            SyntaxKind::BreakKeyword
        } else {
            SyntaxKind::ContinueKeyword
        };
        self.parse_expected(keyword, None);
        let label = if self.can_parse_semicolon() {
            None
        } else {
            Some(self.parse_identifier())
        };
        self.parse_semicolon();

        if is_break {
            self.finish_node_data(NodeData::BreakStatement(BreakStatementData { label }), pos)
        } else {
            self.finish_node_data(
                NodeData::ContinueStatement(ContinueStatementData { label }),
                pos,
            )
        }
    }

    fn parse_return_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::ReturnKeyword, None);
        let expression = if self.can_parse_semicolon() {
            None
        } else {
            Some(self.allow_in(|parser| parser.parse_expression()))
        };
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::ReturnStatement(ReturnStatementData { expression }),
            pos,
        )
    }

    fn parse_with_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::WithKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        let statement =
            self.do_in_context(NodeFlags::IN_WITH_STATEMENT, NodeFlags::NONE, |parser| {
                parser.parse_statement()
            });
        self.finish_node_data(
            NodeData::WithStatement(WithStatementData {
                expression: Some(expression),
                statement: Some(statement),
            }),
            pos,
        )
    }

    fn parse_switch_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::SwitchKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        let case_block = self.parse_case_block();
        self.finish_node_data(
            NodeData::SwitchStatement(SwitchStatementData {
                expression: Some(expression),
                case_block: Some(case_block),
            }),
            pos,
        )
    }

    fn parse_case_block(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBraceToken, None);
        let clauses = self.parse_list(ParsingContext::SwitchClauses, |parser| {
            Some(parser.parse_case_or_default_clause())
        });
        self.parse_expected(SyntaxKind::CloseBraceToken, None);
        self.finish_node_data(
            NodeData::CaseBlock(CaseBlockData {
                clauses: Some(clauses),
            }),
            pos,
        )
    }

    fn parse_case_or_default_clause(&mut self) -> NodeId {
        if self.token() == SyntaxKind::CaseKeyword {
            self.parse_case_clause()
        } else {
            self.parse_default_clause()
        }
    }

    fn parse_case_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::CaseKeyword, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::ColonToken, None);
        let statements = self.parse_list(ParsingContext::SwitchClauseStatements, |parser| {
            Some(parser.parse_statement())
        });
        self.finish_node_data(
            NodeData::CaseClause(CaseClauseData {
                expression: Some(expression),
                statements: Some(statements),
            }),
            pos,
        )
    }

    fn parse_default_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::DefaultKeyword, None);
        self.parse_expected(SyntaxKind::ColonToken, None);
        let statements = self.parse_list(ParsingContext::SwitchClauseStatements, |parser| {
            Some(parser.parse_statement())
        });
        self.finish_node_data(
            NodeData::DefaultClause(DefaultClauseData {
                statements: Some(statements),
            }),
            pos,
        )
    }

    fn parse_throw_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::ThrowKeyword, None);
        let expression = if self.scanner.has_preceding_line_break() {
            // tsc: an ASI'd throw gets an EMPTY identifier without a parse
            // error here; the missing-semicolon path reports instead.
            let pos = self.node_pos();
            let id = self.arena.alloc_node(
                NodeData::Identifier(IdentifierData {
                    escaped_text: String::new(),
                    text: String::new(),
                }),
                pos,
                pos,
                NodeFlags::NONE,
            );
            self.finish_node(id, pos)
        } else {
            self.allow_in(|parser| parser.parse_expression())
        };
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::ThrowStatement(ThrowStatementData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_try_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::TryKeyword, None);
        let try_block = self.parse_block(false, None);
        let catch_clause = if self.token() == SyntaxKind::CatchKeyword {
            Some(self.parse_catch_clause())
        } else {
            None
        };
        let finally_block = if catch_clause.is_none() || self.token() == SyntaxKind::FinallyKeyword
        {
            self.parse_expected(
                SyntaxKind::FinallyKeyword,
                Some(&gen::catch_or_finally_expected),
            );
            Some(self.parse_block(false, None))
        } else {
            None
        };
        self.finish_node_data(
            NodeData::TryStatement(TryStatementData {
                try_block: Some(try_block),
                catch_clause,
                finally_block,
            }),
            pos,
        )
    }

    fn parse_catch_clause(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::CatchKeyword, None);
        let variable_declaration = if self.parse_optional(SyntaxKind::OpenParenToken) {
            let declaration = self.parse_variable_declaration(false);
            self.parse_expected(SyntaxKind::CloseParenToken, None);
            Some(declaration)
        } else {
            None
        };
        let block = self.parse_block(false, None);
        self.finish_node_data(
            NodeData::CatchClause(CatchClauseData {
                variable_declaration,
                block: Some(block),
            }),
            pos,
        )
    }

    fn parse_debugger_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::DebuggerKeyword, None);
        self.parse_semicolon();
        self.finish_node_data(NodeData::DebuggerStatement(DebuggerStatementData {}), pos)
    }

    fn parse_expression_or_labeled_statement(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.allow_in(|parser| parser.parse_expression());
        if self.is_identifier_node(expression) && self.parse_optional(SyntaxKind::ColonToken) {
            let statement = self.parse_statement();
            return self.finish_node_data(
                NodeData::LabeledStatement(LabeledStatementData {
                    label: Some(expression),
                    statement: Some(statement),
                }),
                pos,
            );
        }
        if !self.try_parse_semicolon() {
            self.parse_error_for_missing_semicolon_after(expression);
        }
        self.finish_node_data(
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_variable_statement(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let declaration_list = self.parse_variable_declaration_list(false);
        self.parse_semicolon();
        self.finish_node_data(
            NodeData::VariableStatement(VariableStatementData {
                modifiers,
                declaration_list: Some(declaration_list),
            }),
            pos,
        )
    }

    fn parse_variable_declaration_list(&mut self, in_for_statement_initializer: bool) -> NodeId {
        let pos = self.node_pos();
        let mut flags = NodeFlags::NONE;
        match self.token() {
            SyntaxKind::VarKeyword => {}
            SyntaxKind::LetKeyword => flags |= NodeFlags::LET,
            SyntaxKind::ConstKeyword => flags |= NodeFlags::CONST,
            SyntaxKind::UsingKeyword => flags |= NodeFlags::USING,
            SyntaxKind::AwaitKeyword if self.is_await_using_declaration() => {
                flags |= NodeFlags::AWAIT_USING;
                self.next_token();
            }
            _ => {}
        }
        self.next_token();

        let declarations = if self.token() == SyntaxKind::OfKeyword
            && self.look_ahead(|parser| {
                parser.next_token();
                parser.is_identifier() && parser.next_token() == SyntaxKind::CloseParenToken
            }) {
            self.arena.empty_array(self.node_pos())
        } else if in_for_statement_initializer {
            self.disallow_in(|parser| {
                parser.parse_delimited_list(
                    ParsingContext::VariableDeclarations,
                    |parser| Some(parser.parse_variable_declaration(false)),
                    false,
                )
            })
        } else {
            self.parse_delimited_list(
                ParsingContext::VariableDeclarations,
                |parser| Some(parser.parse_variable_declaration(true)),
                false,
            )
        };

        self.finish_node_with_flags(
            NodeData::VariableDeclarationList(VariableDeclarationListData {
                declarations: Some(declarations),
            }),
            pos,
            flags,
        )
    }

    fn parse_variable_declaration(&mut self, allow_exclamation: bool) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_identifier_or_pattern();
        let exclamation_token = if allow_exclamation
            && self.arena.node(name).kind == SyntaxKind::Identifier
            && self.token() == SyntaxKind::ExclamationToken
            && !self.scanner.has_preceding_line_break()
        {
            Some(self.parse_token_node())
        } else {
            None
        };
        let r#type = self.parse_type_annotation();
        let initializer = if matches!(self.token(), SyntaxKind::InKeyword | SyntaxKind::OfKeyword) {
            None
        } else {
            self.parse_initializer()
        };
        self.finish_node_data(
            NodeData::VariableDeclaration(VariableDeclarationData {
                name: Some(name),
                exclamation_token,
                r#type,
                initializer,
            }),
            pos,
        )
    }

    fn parse_identifier_or_pattern(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::OpenBracketToken => self.parse_array_binding_pattern(),
            SyntaxKind::OpenBraceToken => self.parse_object_binding_pattern(),
            _ => self.parse_binding_identifier(),
        }
    }

    fn parse_array_binding_pattern(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBracketToken, None);
        let elements = self.allow_in(|parser| {
            parser.parse_delimited_list(
                ParsingContext::ArrayBindingElements,
                |parser| Some(parser.parse_array_binding_element()),
                false,
            )
        });
        self.parse_expected(SyntaxKind::CloseBracketToken, None);
        self.finish_node_data(
            NodeData::ArrayBindingPattern(ArrayBindingPatternData {
                elements: Some(elements),
            }),
            pos,
        )
    }

    /// tsc parseArrayBindingElement.
    fn parse_array_binding_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.token() == SyntaxKind::CommaToken {
            return self
                .finish_node_data(NodeData::OmittedExpression(OmittedExpressionData {}), pos);
        }
        let dot_dot_dot_token = self.parse_optional_token(SyntaxKind::DotDotDotToken);
        let name = self.parse_identifier_or_pattern();
        let initializer = self.parse_initializer();
        self.finish_node_data(
            NodeData::BindingElement(BindingElementData {
                dot_dot_dot_token,
                property_name: None,
                name: Some(name),
                initializer,
            }),
            pos,
        )
    }

    fn parse_object_binding_pattern(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBraceToken, None);
        let elements = self.allow_in(|parser| {
            parser.parse_delimited_list(
                ParsingContext::ObjectBindingElements,
                |parser| Some(parser.parse_object_binding_element()),
                false,
            )
        });
        self.parse_expected(SyntaxKind::CloseBraceToken, None);
        self.finish_node_data(
            NodeData::ObjectBindingPattern(ObjectBindingPatternData {
                elements: Some(elements),
            }),
            pos,
        )
    }

    /// tsc parseObjectBindingElement.
    fn parse_object_binding_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        let dot_dot_dot_token = self.parse_optional_token(SyntaxKind::DotDotDotToken);
        let token_is_identifier = self.is_binding_identifier();
        let mut property_name = Some(self.parse_property_name());
        let name = if token_is_identifier && self.token() != SyntaxKind::ColonToken {
            property_name.take().expect("property name was just parsed")
        } else {
            self.parse_expected(SyntaxKind::ColonToken, None);
            self.parse_identifier_or_pattern()
        };
        let initializer = self.parse_initializer();
        self.finish_node_data(
            NodeData::BindingElement(BindingElementData {
                dot_dot_dot_token,
                property_name,
                name: Some(name),
                initializer,
            }),
            pos,
        )
    }

    /// tsc parseBindingIdentifier.
    fn parse_binding_identifier(&mut self) -> NodeId {
        let is_binding_identifier = self.is_binding_identifier();
        self.create_identifier_node(is_binding_identifier, None, None)
    }

    fn parse_identifier(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::Identifier(IdentifierData {
                escaped_text: text.clone(),
                text,
            }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_private_identifier(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::PrivateIdentifier(PrivateIdentifierData {
                escaped_text: text.clone(),
                text,
            }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_expression(&mut self) -> NodeId {
        let save_decorator_context = self.in_decorator_context();
        if save_decorator_context {
            self.set_decorator_context(false);
        }
        let pos = self.node_pos();
        let mut expr = self.parse_assignment_expression_or_higher(true);
        while let Some(operator_token) = self.parse_optional_token(SyntaxKind::CommaToken) {
            let right = self.parse_assignment_expression_or_higher(true);
            expr = self.make_binary_expression(expr, operator_token, right, pos);
        }
        if save_decorator_context {
            self.set_decorator_context(true);
        }
        expr
    }

    fn parse_assignment_expression_or_higher(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> NodeId {
        if self.is_yield_expression() {
            return self.parse_yield_expression();
        }
        let arrow_expression = self
            .try_parse_parenthesized_arrow_function_expression(allow_return_type_in_arrow_function)
            .or_else(|| {
                self.try_parse_async_simple_arrow_function_expression(
                    allow_return_type_in_arrow_function,
                )
            });
        if let Some(arrow_expression) = arrow_expression {
            return arrow_expression;
        }
        let pos = self.node_pos();
        let expr = self.parse_binary_expression_or_higher(LOWEST_OPERATOR_PRECEDENCE);
        if self.arena.node(expr).kind == SyntaxKind::Identifier
            && self.token() == SyntaxKind::EqualsGreaterThanToken
        {
            return self.parse_simple_arrow_function_expression(
                pos,
                expr,
                allow_return_type_in_arrow_function,
                None,
            );
        }
        if is_left_hand_side_expression_kind(self.arena.node(expr).kind)
            && is_assignment_operator(self.scanner.re_scan_greater_token())
        {
            let operator_token = self.parse_token_node();
            let right =
                self.parse_assignment_expression_or_higher(allow_return_type_in_arrow_function);
            return self.make_binary_expression(expr, operator_token, right, pos);
        }
        self.parse_conditional_expression_rest(expr, pos, allow_return_type_in_arrow_function)
    }

    fn parse_conditional_expression_rest(
        &mut self,
        left_operand: NodeId,
        pos: usize,
        allow_return_type_in_arrow_function: bool,
    ) -> NodeId {
        let Some(question_token) = self.parse_optional_token(SyntaxKind::QuestionToken) else {
            return left_operand;
        };
        let when_true = self.do_in_context(
            NodeFlags::NONE,
            NodeFlags::DISALLOW_IN_CONTEXT | NodeFlags::DECORATOR_CONTEXT,
            |parser| parser.parse_assignment_expression_or_higher(false),
        );
        let colon_token = self.parse_expected_token(SyntaxKind::ColonToken, None);
        let when_false = if self.node_is_present(colon_token) {
            self.parse_assignment_expression_or_higher(allow_return_type_in_arrow_function)
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                false,
                Some(&gen::_0_expected),
                &[&token_to_string(SyntaxKind::ColonToken)],
            )
        };
        self.finish_node_data(
            NodeData::ConditionalExpression(ConditionalExpressionData {
                condition: Some(left_operand),
                question_token: Some(question_token),
                when_true: Some(when_true),
                colon_token: Some(colon_token),
                when_false: Some(when_false),
            }),
            pos,
        )
    }

    fn try_parse_parenthesized_arrow_function_expression(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        match self.is_parenthesized_arrow_function_expression() {
            Tristate::False => None,
            Tristate::True => self.parse_parenthesized_arrow_function_expression(true, true),
            Tristate::Unknown => self.try_parse(|parser| {
                parser.parse_possible_parenthesized_arrow_function_expression(
                    allow_return_type_in_arrow_function,
                )
            }),
        }
    }

    fn is_parenthesized_arrow_function_expression(&mut self) -> Tristate {
        if matches!(
            self.token(),
            SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken | SyntaxKind::AsyncKeyword
        ) {
            return self
                .look_ahead(|parser| parser.is_parenthesized_arrow_function_expression_worker());
        }
        if self.token() == SyntaxKind::EqualsGreaterThanToken {
            // ERROR RECOVERY TWEAK: "a, => b" — treat => as a (bad) arrow.
            return Tristate::True;
        }
        Tristate::False
    }

    fn is_parenthesized_arrow_function_expression_worker(&mut self) -> Tristate {
        if self.token() == SyntaxKind::AsyncKeyword {
            self.next_token();
            if self.scanner.has_preceding_line_break() {
                return Tristate::False;
            }
            if !matches!(
                self.token(),
                SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken
            ) {
                return Tristate::False;
            }
        }

        let first = self.token();
        let second = self.next_token();

        if first == SyntaxKind::OpenParenToken {
            if second == SyntaxKind::CloseParenToken {
                let third = self.next_token();
                return match third {
                    SyntaxKind::EqualsGreaterThanToken
                    | SyntaxKind::ColonToken
                    | SyntaxKind::OpenBraceToken => Tristate::True,
                    _ => Tristate::False,
                };
            }
            if matches!(
                second,
                SyntaxKind::OpenBracketToken | SyntaxKind::OpenBraceToken
            ) {
                return Tristate::Unknown;
            }
            if second == SyntaxKind::DotDotDotToken {
                return Tristate::True;
            }
            if self.is_modifier_kind(second)
                && second != SyntaxKind::AsyncKeyword
                && self.look_ahead(|parser| {
                    parser.next_token();
                    parser.is_identifier()
                })
            {
                if self.next_token() == SyntaxKind::AsKeyword {
                    return Tristate::False;
                }
                return Tristate::True;
            }
            if !self.is_identifier() && second != SyntaxKind::ThisKeyword {
                return Tristate::False;
            }
            match self.next_token() {
                SyntaxKind::ColonToken => Tristate::True,
                SyntaxKind::QuestionToken => {
                    self.next_token();
                    if matches!(
                        self.token(),
                        SyntaxKind::ColonToken
                            | SyntaxKind::CommaToken
                            | SyntaxKind::EqualsToken
                            | SyntaxKind::CloseParenToken
                    ) {
                        return Tristate::True;
                    }
                    Tristate::False
                }
                SyntaxKind::CommaToken | SyntaxKind::EqualsToken | SyntaxKind::CloseParenToken => {
                    Tristate::Unknown
                }
                _ => Tristate::False,
            }
        } else {
            debug_assert_eq!(first, SyntaxKind::LessThanToken);
            if !self.is_identifier() && self.token() != SyntaxKind::ConstKeyword {
                return Tristate::False;
            }
            if self.language_variant == LanguageVariant::Jsx {
                let is_arrow_function_in_jsx = self.look_ahead(|parser| {
                    parser.parse_optional(SyntaxKind::ConstKeyword);
                    let third = parser.next_token();
                    if third == SyntaxKind::ExtendsKeyword {
                        let fourth = parser.next_token();
                        !matches!(
                            fourth,
                            SyntaxKind::EqualsToken
                                | SyntaxKind::GreaterThanToken
                                | SyntaxKind::SlashToken
                        )
                    } else {
                        matches!(third, SyntaxKind::CommaToken | SyntaxKind::EqualsToken)
                    }
                });
                if is_arrow_function_in_jsx {
                    return Tristate::True;
                }
                return Tristate::False;
            }
            Tristate::Unknown
        }
    }

    fn parse_possible_parenthesized_arrow_function_expression(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        let token_pos = self.scanner.token_start();
        if self.not_parenthesized_arrow.contains(&token_pos) {
            return None;
        }
        let result = self.parse_parenthesized_arrow_function_expression(
            false,
            allow_return_type_in_arrow_function,
        );
        if result.is_none() {
            self.not_parenthesized_arrow.insert(token_pos);
        }
        result
    }

    fn try_parse_async_simple_arrow_function_expression(
        &mut self,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        if self.token() == SyntaxKind::AsyncKeyword
            && self.look_ahead(|parser| parser.is_un_parenthesized_async_arrow_function_worker())
                == Tristate::True
        {
            let pos = self.node_pos();
            let async_modifier = self.parse_modifiers_for_arrow_function();
            let expr = self.parse_binary_expression_or_higher(LOWEST_OPERATOR_PRECEDENCE);
            return Some(self.parse_simple_arrow_function_expression(
                pos,
                expr,
                allow_return_type_in_arrow_function,
                async_modifier,
            ));
        }
        None
    }

    fn is_un_parenthesized_async_arrow_function_worker(&mut self) -> Tristate {
        if self.token() == SyntaxKind::AsyncKeyword {
            self.next_token();
            if self.scanner.has_preceding_line_break()
                || self.token() == SyntaxKind::EqualsGreaterThanToken
            {
                return Tristate::False;
            }
            let expr = self.parse_binary_expression_or_higher(LOWEST_OPERATOR_PRECEDENCE);
            if !self.scanner.has_preceding_line_break()
                && self.arena.node(expr).kind == SyntaxKind::Identifier
                && self.token() == SyntaxKind::EqualsGreaterThanToken
            {
                return Tristate::True;
            }
        }
        Tristate::False
    }

    /// tsc parseContextualModifier.
    fn parse_contextual_modifier(&mut self, t: SyntaxKind) -> bool {
        self.token() == t && self.try_parse(|parser| parser.next_token_can_follow_modifier())
    }

    fn next_token_is_on_same_line_and_can_follow_modifier(&mut self) -> bool {
        self.next_token();
        if self.scanner.has_preceding_line_break() {
            return false;
        }
        self.can_follow_modifier()
    }

    fn next_token_can_follow_modifier(&mut self) -> bool {
        match self.token() {
            SyntaxKind::ConstKeyword => self.next_token() == SyntaxKind::EnumKeyword,
            SyntaxKind::ExportKeyword => {
                self.next_token();
                if self.token() == SyntaxKind::DefaultKeyword {
                    return self
                        .look_ahead(|parser| parser.next_token_can_follow_default_keyword());
                }
                if self.token() == SyntaxKind::TypeKeyword {
                    return self
                        .look_ahead(|parser| parser.next_token_can_follow_export_modifier());
                }
                self.can_follow_export_modifier()
            }
            SyntaxKind::DefaultKeyword => self.next_token_can_follow_default_keyword(),
            SyntaxKind::StaticKeyword => {
                self.next_token();
                self.can_follow_modifier()
            }
            SyntaxKind::GetKeyword | SyntaxKind::SetKeyword => {
                self.next_token();
                self.can_follow_get_or_set_keyword()
            }
            _ => self.next_token_is_on_same_line_and_can_follow_modifier(),
        }
    }

    fn can_follow_export_modifier(&self) -> bool {
        self.token() == SyntaxKind::AtToken
            || self.token() != SyntaxKind::AsteriskToken
                && self.token() != SyntaxKind::AsKeyword
                && self.token() != SyntaxKind::OpenBraceToken
                && self.can_follow_modifier()
    }

    fn next_token_can_follow_export_modifier(&mut self) -> bool {
        self.next_token();
        self.can_follow_export_modifier()
    }

    fn parse_any_contextual_modifier(&mut self) -> bool {
        self.is_modifier_kind(self.token())
            && self.try_parse(|parser| parser.next_token_can_follow_modifier())
    }

    fn can_follow_modifier(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::OpenBracketToken
                | SyntaxKind::OpenBraceToken
                | SyntaxKind::AsteriskToken
                | SyntaxKind::DotDotDotToken
        ) || self.is_literal_property_name()
    }

    fn can_follow_get_or_set_keyword(&self) -> bool {
        self.token() == SyntaxKind::OpenBracketToken || self.is_literal_property_name()
    }

    fn next_token_can_follow_default_keyword(&mut self) -> bool {
        self.next_token();
        match self.token() {
            SyntaxKind::ClassKeyword
            | SyntaxKind::FunctionKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::AtToken => true,
            SyntaxKind::AbstractKeyword => {
                self.look_ahead(|parser| parser.next_token_is_class_keyword_on_same_line())
            }
            SyntaxKind::AsyncKeyword => {
                self.look_ahead(|parser| parser.next_token_is_function_keyword_on_same_line())
            }
            _ => false,
        }
    }

    fn next_token_is_class_keyword_on_same_line(&mut self) -> bool {
        self.next_token();
        self.token() == SyntaxKind::ClassKeyword && !self.scanner.has_preceding_line_break()
    }

    fn next_token_is_function_keyword_on_same_line(&mut self) -> bool {
        self.next_token();
        self.token() == SyntaxKind::FunctionKeyword && !self.scanner.has_preceding_line_break()
    }

    fn next_token_is_open_brace(&mut self) -> bool {
        self.next_token() == SyntaxKind::OpenBraceToken
    }

    /// tsc parseDecoratorExpression.
    fn parse_decorator_expression(&mut self) -> NodeId {
        if self.in_await_context() && self.token() == SyntaxKind::AwaitKeyword {
            // `@await x` inside an async body: `await` cannot be an
            // identifier here, so a missing identifier heads the chain.
            let pos = self.node_pos();
            let is_identifier = self.is_identifier();
            let await_expression =
                self.create_identifier_node(is_identifier, Some(&gen::Expression_expected), None);
            self.next_token();
            let member_expression = self.parse_member_expression_rest(pos, await_expression, true);
            return self.parse_call_expression_rest(pos, member_expression);
        }
        self.parse_left_hand_side_expression_or_higher()
    }

    fn try_parse_decorator(&mut self) -> Option<NodeId> {
        let pos = self.node_pos();
        if !self.parse_optional(SyntaxKind::AtToken) {
            return None;
        }
        let expression =
            self.do_in_context(NodeFlags::DECORATOR_CONTEXT, NodeFlags::NONE, |parser| {
                parser.parse_decorator_expression()
            });
        Some(self.finish_node_data(
            NodeData::Decorator(DecoratorData {
                expression: Some(expression),
            }),
            pos,
        ))
    }

    /// tsc tryParseModifier.
    fn try_parse_modifier(
        &mut self,
        has_seen_static_modifier: bool,
        permit_const_as_modifier: bool,
        stop_on_start_of_class_static_block: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        let kind = self.token();
        if self.token() == SyntaxKind::ConstKeyword && permit_const_as_modifier {
            if !self.try_parse(|parser| parser.next_token_is_on_same_line_and_can_follow_modifier())
            {
                return None;
            }
        } else {
            if self.token() == SyntaxKind::StaticKeyword
                && (has_seen_static_modifier
                    || stop_on_start_of_class_static_block
                        && self.look_ahead(|parser| parser.next_token_is_open_brace()))
            {
                return None;
            }
            if !self.parse_any_contextual_modifier() {
                return None;
            }
        }
        // The modifier token itself has already been consumed above.
        let id = self
            .arena
            .alloc_token(kind, pos, self.scanner.full_start_pos(), NodeFlags::NONE);
        Some(self.finish_node(id, pos))
    }

    /// tsc parseModifiers: decorators and modifiers interleave in one list
    /// (leading decorators, modifiers, then the trailing-decorator recovery).
    fn parse_modifiers(
        &mut self,
        allow_decorators: bool,
        permit_const_as_modifier: bool,
        stop_on_start_of_class_static_block: bool,
    ) -> Option<crate::NodeArrayId> {
        let pos = self.node_pos();
        let mut list = Vec::new();
        let mut has_seen_static_modifier = false;
        let mut has_leading_modifier = false;
        let mut has_trailing_decorator = false;

        if allow_decorators && self.token() == SyntaxKind::AtToken {
            while let Some(decorator) = self.try_parse_decorator() {
                list.push(decorator);
            }
        }
        while let Some(modifier) = self.try_parse_modifier(
            has_seen_static_modifier,
            permit_const_as_modifier,
            stop_on_start_of_class_static_block,
        ) {
            if self.arena.node(modifier).kind == SyntaxKind::StaticKeyword {
                has_seen_static_modifier = true;
            }
            list.push(modifier);
            has_leading_modifier = true;
        }
        if has_leading_modifier && allow_decorators && self.token() == SyntaxKind::AtToken {
            while let Some(decorator) = self.try_parse_decorator() {
                list.push(decorator);
                has_trailing_decorator = true;
            }
        }
        if has_trailing_decorator {
            while let Some(modifier) = self.try_parse_modifier(
                has_seen_static_modifier,
                permit_const_as_modifier,
                stop_on_start_of_class_static_block,
            ) {
                if self.arena.node(modifier).kind == SyntaxKind::StaticKeyword {
                    has_seen_static_modifier = true;
                }
                list.push(modifier);
            }
        }
        if list.is_empty() {
            None
        } else {
            Some(self.arena.alloc_array(list, pos, self.node_pos(), false))
        }
    }

    fn parse_modifiers_for_arrow_function(&mut self) -> Option<crate::NodeArrayId> {
        if self.token() != SyntaxKind::AsyncKeyword {
            return None;
        }
        let pos = self.node_pos();
        let modifier = self.parse_token_node();
        let end = self.arena.node(modifier).end as usize;
        Some(self.arena.alloc_array(vec![modifier], pos, end, false))
    }

    fn parse_parenthesized_arrow_function_expression(
        &mut self,
        allow_ambiguity: bool,
        allow_return_type_in_arrow_function: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers_for_arrow_function();
        let is_async = modifiers.is_some();
        let type_parameters = self.parse_type_parameters();

        let parameters: crate::NodeArrayId;
        if !self.parse_expected(SyntaxKind::OpenParenToken, None) {
            if !allow_ambiguity {
                return None;
            }
            // tsc createMissingList
            parameters = self.arena.missing_array(self.node_pos());
        } else {
            match self.parse_parameters_worker(false, is_async, allow_ambiguity) {
                Some(list) => parameters = list,
                None if !allow_ambiguity => return None,
                None => parameters = self.arena.empty_array(self.node_pos()),
            }
            if !self.parse_expected(SyntaxKind::CloseParenToken, None) && !allow_ambiguity {
                return None;
            }
        }

        let has_return_colon = self.token() == SyntaxKind::ColonToken;
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        if let Some(return_type) = r#type {
            if !allow_ambiguity && self.type_has_arrow_function_blocking_parse_error(return_type) {
                return None;
            }
        }
        // A JSDoc function type as the (unwrapped) return type means the `=>`
        // belongs to that type, not to an arrow body.
        let mut unwrapped_type = r#type;
        while let Some(current) = unwrapped_type {
            match &self.arena.node(current).data {
                NodeData::ParenthesizedType(data) => unwrapped_type = data.r#type,
                _ => break,
            }
        }
        let has_jsdoc_function_type = unwrapped_type
            .is_some_and(|current| self.arena.node(current).kind == SyntaxKind::JSDocFunctionType);
        if !allow_ambiguity
            && self.token() != SyntaxKind::EqualsGreaterThanToken
            && (has_jsdoc_function_type || self.token() != SyntaxKind::OpenBraceToken)
        {
            return None;
        }

        let last_token = self.token();
        let equals_greater_than_token =
            self.parse_expected_token(SyntaxKind::EqualsGreaterThanToken, None);
        let body = if matches!(
            last_token,
            SyntaxKind::EqualsGreaterThanToken | SyntaxKind::OpenBraceToken
        ) {
            self.parse_arrow_function_expression_body(is_async, allow_return_type_in_arrow_function)
        } else {
            let is_identifier = self.is_identifier();
            self.create_identifier_node(is_identifier, None, None)
        };

        // `a ? (b): c => d` — inside a conditional's whenTrue an arrow with a
        // return type must be followed by the conditional's own colon.
        if !allow_return_type_in_arrow_function
            && has_return_colon
            && self.token() != SyntaxKind::ColonToken
        {
            return None;
        }

        Some(self.finish_node_data(
            NodeData::ArrowFunction(ArrowFunctionData {
                modifiers,
                r#type,
                type_parameters,
                parameters: Some(parameters),
                equals_greater_than_token: Some(equals_greater_than_token),
                body: Some(body),
            }),
            pos,
        ))
    }

    fn parse_simple_arrow_function_expression(
        &mut self,
        pos: usize,
        identifier: NodeId,
        allow_return_type_in_arrow_function: bool,
        async_modifier: Option<crate::NodeArrayId>,
    ) -> NodeId {
        debug_assert_eq!(self.token(), SyntaxKind::EqualsGreaterThanToken);
        let identifier_pos = self.arena.node(identifier).pos as usize;
        let parameter = self.finish_node_data(
            NodeData::Parameter(ParameterData {
                modifiers: None,
                dot_dot_dot_token: None,
                name: Some(identifier),
                question_token: None,
                r#type: None,
                initializer: None,
            }),
            identifier_pos,
        );
        let parameter_node = self.arena.node(parameter);
        let (parameter_pos, parameter_end) =
            (parameter_node.pos as usize, parameter_node.end as usize);
        let parameters =
            self.arena
                .alloc_array(vec![parameter], parameter_pos, parameter_end, false);
        let equals_greater_than_token =
            self.parse_expected_token(SyntaxKind::EqualsGreaterThanToken, None);
        let body = self.parse_arrow_function_expression_body(
            async_modifier.is_some(),
            allow_return_type_in_arrow_function,
        );
        self.finish_node_data(
            NodeData::ArrowFunction(ArrowFunctionData {
                modifiers: async_modifier,
                r#type: None,
                type_parameters: None,
                parameters: Some(parameters),
                equals_greater_than_token: Some(equals_greater_than_token),
                body: Some(body),
            }),
            pos,
        )
    }

    fn parse_arrow_function_expression_body(
        &mut self,
        is_async: bool,
        allow_return_type_in_arrow_function: bool,
    ) -> NodeId {
        if self.token() == SyntaxKind::OpenBraceToken {
            return self.parse_function_block(false, is_async, false, None);
        }
        if self.token() != SyntaxKind::SemicolonToken
            && self.token() != SyntaxKind::FunctionKeyword
            && self.token() != SyntaxKind::ClassKeyword
            && self.is_start_of_statement()
            && !self.is_start_of_expression_statement()
        {
            return self.parse_function_block(false, is_async, true, None);
        }
        let saved_yield_context = self.in_yield_context();
        self.set_yield_context(false);
        let node = if is_async {
            self.do_in_context(NodeFlags::AWAIT_CONTEXT, NodeFlags::NONE, |parser| {
                parser.parse_assignment_expression_or_higher(allow_return_type_in_arrow_function)
            })
        } else {
            self.do_in_context(NodeFlags::NONE, NodeFlags::AWAIT_CONTEXT, |parser| {
                parser.parse_assignment_expression_or_higher(allow_return_type_in_arrow_function)
            })
        };
        self.set_yield_context(saved_yield_context);
        node
    }

    fn is_start_of_expression_statement(&mut self) -> bool {
        self.token() != SyntaxKind::OpenBraceToken
            && self.token() != SyntaxKind::FunctionKeyword
            && self.token() != SyntaxKind::ClassKeyword
            && self.token() != SyntaxKind::AtToken
            && self.is_start_of_expression()
    }

    fn parse_binary_expression_or_higher(&mut self, precedence: i32) -> NodeId {
        let pos = self.node_pos();
        let left_operand = self.parse_unary_expression_or_higher();
        self.parse_binary_expression_rest(precedence, left_operand, pos)
    }

    fn parse_binary_expression_rest(
        &mut self,
        precedence: i32,
        mut left_operand: NodeId,
        pos: usize,
    ) -> NodeId {
        loop {
            self.scanner.re_scan_greater_token();
            let new_precedence = get_binary_operator_precedence(self.token());
            let consume_current_operator = if self.token() == SyntaxKind::AsteriskAsteriskToken {
                new_precedence >= precedence
            } else {
                new_precedence > precedence
            };
            if !consume_current_operator {
                break;
            }
            if self.token() == SyntaxKind::InKeyword && self.in_disallow_in_context() {
                break;
            }
            if matches!(
                self.token(),
                SyntaxKind::AsKeyword | SyntaxKind::SatisfiesKeyword
            ) {
                if self.scanner.has_preceding_line_break() {
                    break;
                }
                let keyword_kind = self.token();
                self.next_token();
                let r#type = self.parse_type();
                left_operand = if keyword_kind == SyntaxKind::SatisfiesKeyword {
                    self.make_satisfies_expression(left_operand, r#type)
                } else {
                    self.make_as_expression(left_operand, r#type)
                };
            } else {
                let operator_token = self.parse_token_node();
                let right = self.parse_binary_expression_or_higher(new_precedence);
                left_operand =
                    self.make_binary_expression(left_operand, operator_token, right, pos);
            }
        }
        left_operand
    }

    fn make_binary_expression(
        &mut self,
        left: NodeId,
        operator_token: NodeId,
        right: NodeId,
        pos: usize,
    ) -> NodeId {
        self.finish_node_data(
            NodeData::BinaryExpression(BinaryExpressionData {
                left: Some(left),
                operator_token: Some(operator_token),
                right: Some(right),
            }),
            pos,
        )
    }

    fn make_as_expression(&mut self, left: NodeId, r#type: NodeId) -> NodeId {
        let pos = self.arena.node(left).pos as usize;
        self.finish_node_data(
            NodeData::AsExpression(AsExpressionData {
                expression: Some(left),
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn make_satisfies_expression(&mut self, left: NodeId, r#type: NodeId) -> NodeId {
        let pos = self.arena.node(left).pos as usize;
        self.finish_node_data(
            NodeData::SatisfiesExpression(SatisfiesExpressionData {
                expression: Some(left),
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn parse_unary_expression_or_higher(&mut self) -> NodeId {
        if self.is_update_expression() {
            let pos = self.node_pos();
            let update_expression = self.parse_update_expression();
            if self.token() == SyntaxKind::AsteriskAsteriskToken {
                let precedence = get_binary_operator_precedence(self.token());
                return self.parse_binary_expression_rest(precedence, update_expression, pos);
            }
            return update_expression;
        }

        let unary_operator = self.token();
        let start = self.scanner.token_start();
        let simple_unary_expression = self.parse_simple_unary_expression();
        if self.token() == SyntaxKind::AsteriskAsteriskToken {
            let node = self.arena.node(simple_unary_expression);
            let end = node.end as usize;
            if node.kind == SyntaxKind::TypeAssertionExpression {
                self.parse_error_at_position(
                    start,
                    end.saturating_sub(start),
                    &gen::A_type_assertion_expression_is_not_allowed_in_the_left_hand_side_of_an_exponentiation_expression_Consider_enclosing_the_expression_in_parentheses,
                    &[],
                );
            } else {
                self.parse_error_at_position(
                    start,
                    end.saturating_sub(start),
                    &gen::An_unary_expression_with_the_0_operator_is_not_allowed_in_the_left_hand_side_of_an_exponentiation_expression_Consider_enclosing_the_expression_in_parentheses,
                    &[&token_to_string(unary_operator)],
                );
            }
        }
        simple_unary_expression
    }

    fn is_update_expression(&self) -> bool {
        match self.token() {
            SyntaxKind::PlusToken
            | SyntaxKind::MinusToken
            | SyntaxKind::TildeToken
            | SyntaxKind::ExclamationToken
            | SyntaxKind::DeleteKeyword
            | SyntaxKind::TypeOfKeyword
            | SyntaxKind::VoidKeyword
            | SyntaxKind::AwaitKeyword => false,
            SyntaxKind::LessThanToken => self.language_variant == LanguageVariant::Jsx,
            _ => true,
        }
    }

    fn parse_simple_unary_expression(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::PlusToken
            | SyntaxKind::MinusToken
            | SyntaxKind::TildeToken
            | SyntaxKind::ExclamationToken => self.parse_prefix_unary_expression(),
            SyntaxKind::DeleteKeyword => self.parse_delete_expression(),
            SyntaxKind::TypeOfKeyword => self.parse_type_of_expression(),
            SyntaxKind::VoidKeyword => self.parse_void_expression(),
            SyntaxKind::LessThanToken => {
                if self.language_variant == LanguageVariant::Jsx {
                    return self.parse_jsx_element_or_self_closing_element_or_fragment(
                        true, None, None, true,
                    );
                }
                self.parse_type_assertion()
            }
            SyntaxKind::AwaitKeyword if self.is_await_expression() => self.parse_await_expression(),
            _ => self.parse_update_expression(),
        }
    }

    fn parse_type_assertion(&mut self) -> NodeId {
        debug_assert!(self.language_variant != LanguageVariant::Jsx);
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::LessThanToken, None);
        let r#type = self.parse_type();
        self.parse_expected(SyntaxKind::GreaterThanToken, None);
        let expression = self.parse_simple_unary_expression();
        self.finish_node_data(
            NodeData::TypeAssertionExpression(TypeAssertionExpressionData {
                r#type: Some(r#type),
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_prefix_unary_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let operand = self.parse_simple_unary_expression();
        self.finish_node_data(
            NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                operand: Some(operand),
            }),
            pos,
        )
    }

    fn parse_delete_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let expression = self.parse_simple_unary_expression();
        self.finish_node_data(
            NodeData::DeleteExpression(DeleteExpressionData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_type_of_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let expression = self.parse_simple_unary_expression();
        self.finish_node_data(
            NodeData::TypeOfExpression(TypeOfExpressionData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_void_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let expression = self.parse_simple_unary_expression();
        self.finish_node_data(
            NodeData::VoidExpression(VoidExpressionData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_await_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let expression = self.parse_simple_unary_expression();
        self.finish_node_data(
            NodeData::AwaitExpression(AwaitExpressionData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_update_expression(&mut self) -> NodeId {
        if matches!(
            self.token(),
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) {
            let pos = self.node_pos();
            self.next_token();
            let operand = self.parse_left_hand_side_expression_or_higher();
            return self.finish_node_data(
                NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                    operand: Some(operand),
                }),
                pos,
            );
        } else if self.language_variant == LanguageVariant::Jsx
            && self.token() == SyntaxKind::LessThanToken
            && self
                .look_ahead(|parser| parser.next_token_is_identifier_or_keyword_or_greater_than())
        {
            return self
                .parse_jsx_element_or_self_closing_element_or_fragment(true, None, None, false);
        }

        let expression = self.parse_left_hand_side_expression_or_higher();
        if matches!(
            self.token(),
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
        ) && !self.scanner.has_preceding_line_break()
        {
            self.next_token();
            return self.finish_node_data(
                NodeData::PostfixUnaryExpression(PostfixUnaryExpressionData {
                    operand: Some(expression),
                }),
                self.arena.node(expression).pos as usize,
            );
        }
        expression
    }

    /// tsc parseJsxElementOrSelfClosingElementOrFragment.
    fn parse_jsx_element_or_self_closing_element_or_fragment(
        &mut self,
        in_expression_context: bool,
        top_invalid_node_position: Option<usize>,
        opening_tag: Option<NodeId>,
        must_be_unary: bool,
    ) -> NodeId {
        let pos = self.node_pos();
        let opening = self
            .parse_jsx_opening_or_self_closing_element_or_opening_fragment(in_expression_context);
        let result = match self.arena.node(opening).kind {
            SyntaxKind::JsxOpeningElement => {
                let opening_tag_name = self.jsx_opening_element_tag_name(opening);
                let mut children = self.parse_jsx_children(opening);
                let closing_element;

                // The last child owns a closing tag that actually matches OUR
                // opening tag: give the child a synthetic empty closing element
                // and adopt its closing element as ours.
                let last_child = self.arena.node_array(children).nodes.last().copied();
                let rebalance = last_child.and_then(|last| {
                    let (last_opening, last_children, last_closing) = {
                        let data = self.arena.node(last).data.as_jsx_element()?;
                        (data.opening_element?, data.children?, data.closing_element?)
                    };
                    let last_open_tag = self.jsx_opening_element_tag_name(last_opening);
                    let last_close_tag = self.jsx_closing_element_tag_name(last_closing);
                    if !self.tag_names_are_equivalent(last_open_tag, last_close_tag)
                        && self.tag_names_are_equivalent(opening_tag_name, last_close_tag)
                    {
                        Some((last_opening, last_children, last_closing))
                    } else {
                        None
                    }
                });
                if let Some((last_opening, last_children, last_closing)) = rebalance {
                    let end = self.arena.node_array(last_children).end as usize;
                    let empty_identifier = self.arena.alloc_node(
                        NodeData::Identifier(IdentifierData {
                            escaped_text: String::new(),
                            text: String::new(),
                        }),
                        end,
                        end,
                        NodeFlags::NONE,
                    );
                    let empty_identifier = self.finish_node_at(empty_identifier, end, end);
                    let synthetic_closing = self.arena.alloc_node(
                        NodeData::JsxClosingElement(JsxClosingElementData {
                            tag_name: Some(empty_identifier),
                        }),
                        end,
                        end,
                        NodeFlags::NONE,
                    );
                    let synthetic_closing = self.finish_node_at(synthetic_closing, end, end);
                    let new_last_pos = self.arena.node(last_opening).pos as usize;
                    let new_last = self.arena.alloc_node(
                        NodeData::JsxElement(JsxElementData {
                            opening_element: Some(last_opening),
                            children: Some(last_children),
                            closing_element: Some(synthetic_closing),
                        }),
                        new_last_pos,
                        end,
                        NodeFlags::NONE,
                    );
                    let new_last = self.finish_node_at(new_last, new_last_pos, end);

                    let (mut nodes, children_pos) = {
                        let array = self.arena.node_array(children);
                        (array.nodes.clone(), array.pos as usize)
                    };
                    *nodes
                        .last_mut()
                        .expect("rebalance only fires on a non-empty child list") = new_last;
                    children = self.arena.alloc_array(nodes, children_pos, end, false);
                    closing_element = last_closing;
                } else {
                    closing_element =
                        self.parse_jsx_closing_element(opening, in_expression_context);
                    let closing_tag_name = self.jsx_closing_element_tag_name(closing_element);
                    if !self.tag_names_are_equivalent(opening_tag_name, closing_tag_name) {
                        let outer_tag_matches = opening_tag.is_some_and(|outer| {
                            self.arena.node(outer).kind == SyntaxKind::JsxOpeningElement
                                && self.tag_names_are_equivalent(
                                    closing_tag_name,
                                    self.jsx_opening_element_tag_name(outer),
                                )
                        });
                        if let Some(opening_tag_name) = opening_tag_name {
                            let opening_tag_text = self.text_of_node(opening_tag_name);
                            if outer_tag_matches {
                                self.parse_error_at_range(
                                    opening_tag_name,
                                    &gen::JSX_element_0_has_no_corresponding_closing_tag,
                                    &[&opening_tag_text],
                                );
                            } else if let Some(closing_tag_name) = closing_tag_name {
                                self.parse_error_at_range(
                                    closing_tag_name,
                                    &gen::Expected_corresponding_JSX_closing_tag_for_0,
                                    &[&opening_tag_text],
                                );
                            }
                        }
                    }
                }
                self.finish_node_data(
                    NodeData::JsxElement(JsxElementData {
                        opening_element: Some(opening),
                        children: Some(children),
                        closing_element: Some(closing_element),
                    }),
                    pos,
                )
            }
            SyntaxKind::JsxOpeningFragment => {
                let children = self.parse_jsx_children(opening);
                let closing_fragment = self.parse_jsx_closing_fragment(in_expression_context);
                self.finish_node_data(
                    NodeData::JsxFragment(JsxFragmentData {
                        opening_fragment: Some(opening),
                        children: Some(children),
                        closing_fragment: Some(closing_fragment),
                    }),
                    pos,
                )
            }
            _ => {
                debug_assert_eq!(
                    self.arena.node(opening).kind,
                    SyntaxKind::JsxSelfClosingElement
                );
                opening
            }
        };
        // A sibling element at the top level: parse it and glue both together
        // with a synthetic comma so only one error is reported.
        if !must_be_unary && in_expression_context && self.token() == SyntaxKind::LessThanToken {
            let top_bad_pos =
                top_invalid_node_position.unwrap_or(self.arena.node(result).pos as usize);
            let invalid_element = self.try_parse(|parser| {
                Some(
                    parser.parse_jsx_element_or_self_closing_element_or_fragment(
                        true,
                        Some(top_bad_pos),
                        None,
                        false,
                    ),
                )
            });
            if let Some(invalid_element) = invalid_element {
                let operator_token =
                    self.create_missing_node(SyntaxKind::CommaToken, false, None, &[]);
                let invalid_pos = self.arena.node(invalid_element).pos;
                let invalid_end = self.arena.node(invalid_element).end as usize;
                {
                    let operator = self.arena.node_mut(operator_token);
                    operator.pos = invalid_pos;
                    operator.end = invalid_pos;
                }
                let start = crate::scanner::skip_trivia(self.scanner.text(), top_bad_pos);
                self.parse_error_at(
                    start,
                    invalid_end,
                    &gen::JSX_expressions_must_have_one_parent_element,
                    &[],
                );
                return self.make_binary_expression(result, operator_token, invalid_element, pos);
            }
        }
        result
    }

    /// tsc parseJsxText. The containsOnlyTriviaWhiteSpaces flag has no slot;
    /// the JsxText/JsxTextAllWhiteSpaces distinction is parse-and-drop.
    fn parse_jsx_text(&mut self) -> NodeId {
        let pos = self.node_pos();
        let text = self.scanner.token_value().to_owned();
        self.scan_jsx_text();
        self.finish_node_data(NodeData::JsxText(JsxTextData { text }), pos)
    }

    /// tsc parseJsxChild.
    fn parse_jsx_child(&mut self, opening_tag: NodeId, token: SyntaxKind) -> Option<NodeId> {
        match token {
            SyntaxKind::EndOfFileToken => {
                if self.arena.node(opening_tag).kind == SyntaxKind::JsxOpeningFragment {
                    self.parse_error_at_range(
                        opening_tag,
                        &gen::JSX_fragment_has_no_corresponding_closing_tag,
                        &[],
                    );
                } else if let Some(tag) = self.jsx_opening_element_tag_name(opening_tag) {
                    let (tag_pos, tag_end) = {
                        let node = self.arena.node(tag);
                        (node.pos as usize, node.end as usize)
                    };
                    let start =
                        crate::scanner::skip_trivia(self.scanner.text(), tag_pos).min(tag_end);
                    let tag_text = self.text_of_node(tag);
                    self.parse_error_at(
                        start,
                        tag_end,
                        &gen::JSX_element_0_has_no_corresponding_closing_tag,
                        &[&tag_text],
                    );
                }
                None
            }
            SyntaxKind::LessThanSlashToken | SyntaxKind::ConflictMarkerTrivia => None,
            SyntaxKind::JsxText | SyntaxKind::JsxTextAllWhiteSpaces => Some(self.parse_jsx_text()),
            SyntaxKind::OpenBraceToken => self.parse_jsx_expression(false),
            SyntaxKind::LessThanToken => {
                Some(self.parse_jsx_element_or_self_closing_element_or_fragment(
                    false,
                    None,
                    Some(opening_tag),
                    false,
                ))
            }
            _ => unreachable!("parseJsxChild: unexpected token {token:?}"),
        }
    }

    /// tsc parseJsxChildren.
    fn parse_jsx_children(&mut self, opening_tag: NodeId) -> crate::NodeArrayId {
        let mut list = Vec::new();
        let list_pos = self.node_pos();
        let save_parsing_context = self.parsing_context;
        self.parsing_context |= ParsingContext::JsxChildren.bit();
        loop {
            let token = self.re_scan_jsx_token();
            let Some(child) = self.parse_jsx_child(opening_tag, token) else {
                break;
            };
            list.push(child);
            if self.arena.node(opening_tag).kind == SyntaxKind::JsxOpeningElement {
                if let Some((child_open_tag, child_close_tag)) = self.jsx_element_tag_names(child) {
                    let opening_tag_name = self.jsx_opening_element_tag_name(opening_tag);
                    if !self.tag_names_are_equivalent(child_open_tag, child_close_tag)
                        && self.tag_names_are_equivalent(opening_tag_name, child_close_tag)
                    {
                        break;
                    }
                }
            }
        }
        self.parsing_context = save_parsing_context;
        self.arena
            .alloc_array(list, list_pos, self.node_pos(), false)
    }

    /// tsc parseJsxAttributes.
    fn parse_jsx_attributes(&mut self) -> NodeId {
        let pos = self.node_pos();
        let properties = self.parse_list(ParsingContext::JsxAttributes, |parser| {
            Some(parser.parse_jsx_attribute())
        });
        self.finish_node_data(
            NodeData::JsxAttributes(JsxAttributesData {
                properties: Some(properties),
            }),
            pos,
        )
    }

    /// tsc parseJsxOpeningOrSelfClosingElementOrOpeningFragment.
    fn parse_jsx_opening_or_self_closing_element_or_opening_fragment(
        &mut self,
        in_expression_context: bool,
    ) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::LessThanToken, None);
        if self.token() == SyntaxKind::GreaterThanToken {
            self.scan_jsx_text();
            let id = self.arena.alloc_token(
                SyntaxKind::JsxOpeningFragment,
                pos,
                self.scanner.full_start_pos(),
                NodeFlags::NONE,
            );
            return self.finish_node(id, pos);
        }
        let tag_name = self.parse_jsx_element_name();
        // tsc gates on the JavaScriptFile context flag; this port is TS-only.
        let type_arguments = self.try_parse_type_arguments();
        let attributes = self.parse_jsx_attributes();
        if self.token() == SyntaxKind::GreaterThanToken {
            self.scan_jsx_text();
            self.finish_node_data(
                NodeData::JsxOpeningElement(JsxOpeningElementData {
                    tag_name: Some(tag_name),
                    type_arguments,
                    attributes: Some(attributes),
                }),
                pos,
            )
        } else {
            self.parse_expected(SyntaxKind::SlashToken, None);
            if self.parse_expected_without_advancing(SyntaxKind::GreaterThanToken, None) {
                if in_expression_context {
                    self.next_token();
                } else {
                    self.scan_jsx_text();
                }
            }
            self.finish_node_data(
                NodeData::JsxSelfClosingElement(JsxSelfClosingElementData {
                    tag_name: Some(tag_name),
                    type_arguments,
                    attributes: Some(attributes),
                }),
                pos,
            )
        }
    }

    /// tsc parseJsxElementName.
    fn parse_jsx_element_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        let initial = self.parse_jsx_tag_name();
        if self.arena.node(initial).kind == SyntaxKind::JsxNamespacedName {
            return initial;
        }
        let mut expression = initial;
        while self.parse_optional(SyntaxKind::DotToken) {
            let name = self.parse_right_side_of_dot(true, false, false);
            expression = self.finish_node_data(
                NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                    expression: Some(expression),
                    question_dot_token: None,
                    name: Some(name),
                }),
                pos,
            );
        }
        expression
    }

    /// tsc parseJsxTagName.
    fn parse_jsx_tag_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.scan_jsx_identifier();
        let is_this = self.token() == SyntaxKind::ThisKeyword;
        let tag_name = self.parse_identifier_name_error_on_unicode_escape_sequence();
        if self.parse_optional(SyntaxKind::ColonToken) {
            self.scan_jsx_identifier();
            let name = self.parse_identifier_name_error_on_unicode_escape_sequence();
            return self.finish_node_data(
                NodeData::JsxNamespacedName(JsxNamespacedNameData {
                    namespace: Some(tag_name),
                    name: Some(name),
                }),
                pos,
            );
        }
        if is_this {
            let id = self.arena.alloc_token(
                SyntaxKind::ThisKeyword,
                pos,
                self.scanner.full_start_pos(),
                NodeFlags::NONE,
            );
            self.finish_node(id, pos)
        } else {
            tag_name
        }
    }

    /// tsc parseJsxExpression.
    fn parse_jsx_expression(&mut self, in_expression_context: bool) -> Option<NodeId> {
        let pos = self.node_pos();
        if !self.parse_expected(SyntaxKind::OpenBraceToken, None) {
            return None;
        }
        let mut dot_dot_dot_token = None;
        let mut expression = None;
        if self.token() != SyntaxKind::CloseBraceToken {
            if !in_expression_context {
                dot_dot_dot_token = self.parse_optional_token(SyntaxKind::DotDotDotToken);
            }
            expression = Some(self.parse_expression());
        }
        if in_expression_context {
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
        } else if self.parse_expected_without_advancing(SyntaxKind::CloseBraceToken, None) {
            self.scan_jsx_text();
        }
        Some(self.finish_node_data(
            NodeData::JsxExpression(JsxExpressionData {
                dot_dot_dot_token,
                expression,
            }),
            pos,
        ))
    }

    /// tsc parseJsxAttribute.
    fn parse_jsx_attribute(&mut self) -> NodeId {
        if self.token() == SyntaxKind::OpenBraceToken {
            return self.parse_jsx_spread_attribute();
        }
        let pos = self.node_pos();
        let name = self.parse_jsx_attribute_name();
        let initializer = self.parse_jsx_attribute_value();
        self.finish_node_data(
            NodeData::JsxAttribute(JsxAttributeData {
                name: Some(name),
                initializer,
            }),
            pos,
        )
    }

    /// tsc parseJsxAttributeValue.
    fn parse_jsx_attribute_value(&mut self) -> Option<NodeId> {
        if self.token() == SyntaxKind::EqualsToken {
            if self.scan_jsx_attribute_value() == SyntaxKind::StringLiteral {
                return Some(self.parse_string_literal());
            }
            if self.token() == SyntaxKind::OpenBraceToken {
                return self.parse_jsx_expression(true);
            }
            if self.token() == SyntaxKind::LessThanToken {
                return Some(self.parse_jsx_element_or_self_closing_element_or_fragment(
                    true, None, None, false,
                ));
            }
            self.parse_error_at_current_token(&gen::or_JSX_element_expected, &[]);
        }
        None
    }

    /// tsc parseJsxAttributeName.
    fn parse_jsx_attribute_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.scan_jsx_identifier();
        let attr_name = self.parse_identifier_name_error_on_unicode_escape_sequence();
        if self.parse_optional(SyntaxKind::ColonToken) {
            self.scan_jsx_identifier();
            let name = self.parse_identifier_name_error_on_unicode_escape_sequence();
            return self.finish_node_data(
                NodeData::JsxNamespacedName(JsxNamespacedNameData {
                    namespace: Some(attr_name),
                    name: Some(name),
                }),
                pos,
            );
        }
        attr_name
    }

    /// tsc parseJsxSpreadAttribute.
    fn parse_jsx_spread_attribute(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBraceToken, None);
        self.parse_expected(SyntaxKind::DotDotDotToken, None);
        let expression = self.parse_expression();
        self.parse_expected(SyntaxKind::CloseBraceToken, None);
        self.finish_node_data(
            NodeData::JsxSpreadAttribute(JsxSpreadAttributeData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    /// tsc parseJsxClosingElement.
    fn parse_jsx_closing_element(&mut self, open: NodeId, in_expression_context: bool) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::LessThanSlashToken, None);
        let tag_name = self.parse_jsx_element_name();
        if self.parse_expected_without_advancing(SyntaxKind::GreaterThanToken, None) {
            let open_tag_name = self.jsx_opening_element_tag_name(open);
            if in_expression_context
                || !self.tag_names_are_equivalent(open_tag_name, Some(tag_name))
            {
                self.next_token();
            } else {
                self.scan_jsx_text();
            }
        }
        self.finish_node_data(
            NodeData::JsxClosingElement(JsxClosingElementData {
                tag_name: Some(tag_name),
            }),
            pos,
        )
    }

    /// tsc parseJsxClosingFragment.
    fn parse_jsx_closing_fragment(&mut self, in_expression_context: bool) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::LessThanSlashToken, None);
        if self.parse_expected_without_advancing(
            SyntaxKind::GreaterThanToken,
            Some(&gen::Expected_corresponding_closing_tag_for_JSX_fragment),
        ) {
            if in_expression_context {
                self.next_token();
            } else {
                self.scan_jsx_text();
            }
        }
        let id = self.arena.alloc_token(
            SyntaxKind::JsxClosingFragment,
            pos,
            self.scanner.full_start_pos(),
            NodeFlags::NONE,
        );
        self.finish_node(id, pos)
    }

    /// tsc parseIdentifierNameErrorOnUnicodeEscapeSequence.
    fn parse_identifier_name_error_on_unicode_escape_sequence(&mut self) -> NodeId {
        if self.scanner.has_unicode_escape() || self.scanner.has_extended_unicode_escape() {
            self.parse_error_at_current_token(
                &gen::Unicode_escape_sequence_cannot_appear_here,
                &[],
            );
        }
        self.parse_identifier_name(None)
    }

    /// tsc tagNamesAreEquivalent.
    fn tag_names_are_equivalent(&self, lhs: Option<NodeId>, rhs: Option<NodeId>) -> bool {
        let (Some(lhs), Some(rhs)) = (lhs, rhs) else {
            return false;
        };
        let lhs_node = self.arena.node(lhs);
        let rhs_node = self.arena.node(rhs);
        if lhs_node.kind != rhs_node.kind {
            return false;
        }
        if lhs_node.kind == SyntaxKind::ThisKeyword {
            return true;
        }
        match (&lhs_node.data, &rhs_node.data) {
            (NodeData::Identifier(lhs), NodeData::Identifier(rhs)) => {
                lhs.escaped_text == rhs.escaped_text
            }
            (NodeData::JsxNamespacedName(lhs), NodeData::JsxNamespacedName(rhs)) => {
                self.identifier_escaped_text(lhs.namespace)
                    == self.identifier_escaped_text(rhs.namespace)
                    && self.identifier_escaped_text(lhs.name)
                        == self.identifier_escaped_text(rhs.name)
            }
            (NodeData::PropertyAccessExpression(lhs), NodeData::PropertyAccessExpression(rhs)) => {
                self.identifier_escaped_text(lhs.name) == self.identifier_escaped_text(rhs.name)
                    && self.tag_names_are_equivalent(lhs.expression, rhs.expression)
            }
            _ => false,
        }
    }

    fn identifier_escaped_text(&self, id: Option<NodeId>) -> Option<&str> {
        match &self.arena.node(id?).data {
            NodeData::Identifier(data) => Some(&data.escaped_text),
            _ => None,
        }
    }

    fn jsx_opening_element_tag_name(&self, node: NodeId) -> Option<NodeId> {
        match &self.arena.node(node).data {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            _ => None,
        }
    }

    fn jsx_closing_element_tag_name(&self, node: NodeId) -> Option<NodeId> {
        self.arena
            .node(node)
            .data
            .as_jsx_closing_element()
            .and_then(|data| data.tag_name)
    }

    /// (openingElement.tagName, closingElement.tagName) when `node` is a JsxElement.
    fn jsx_element_tag_names(&self, node: NodeId) -> Option<(Option<NodeId>, Option<NodeId>)> {
        let data = self.arena.node(node).data.as_jsx_element()?;
        let open_tag = data
            .opening_element
            .and_then(|open| self.jsx_opening_element_tag_name(open));
        let close_tag = data
            .closing_element
            .and_then(|close| self.jsx_closing_element_tag_name(close));
        Some((open_tag, close_tag))
    }

    fn is_await_expression(&mut self) -> bool {
        self.token() == SyntaxKind::AwaitKeyword
            && (self.in_await_context()
                || self.look_ahead(|parser| {
                    parser.next_token();
                    parser.is_identifier_or_keyword_or_literal()
                        && !parser.scanner.has_preceding_line_break()
                }))
    }

    fn is_yield_expression(&mut self) -> bool {
        self.token() == SyntaxKind::YieldKeyword
            && (self.in_yield_context()
                || self.look_ahead(|parser| {
                    parser.next_token();
                    parser.is_identifier_or_keyword_or_literal()
                        && !parser.scanner.has_preceding_line_break()
                }))
    }

    fn parse_yield_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let (asterisk_token, expression) = if !self.scanner.has_preceding_line_break()
            && (self.token() == SyntaxKind::AsteriskToken || self.is_start_of_expression())
        {
            let asterisk_token = self.parse_optional_token(SyntaxKind::AsteriskToken);
            let expression = self.parse_assignment_expression_or_higher(true);
            (asterisk_token, Some(expression))
        } else {
            (None, None)
        };
        self.finish_node_data(
            NodeData::YieldExpression(YieldExpressionData {
                asterisk_token,
                expression,
            }),
            pos,
        )
    }

    /// tsc parseSuperExpression.
    fn parse_super_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let mut expression = self.parse_token_node();
        if self.token() == SyntaxKind::LessThanToken {
            let start_pos = self.node_pos();
            let type_arguments =
                self.try_parse(|parser| parser.try_parse_type_arguments_in_expression());
            if let Some(type_arguments) = type_arguments {
                let end = self.node_pos();
                self.parse_error_at(start_pos, end, &gen::super_may_not_use_type_arguments, &[]);
                if !self.is_template_start_of_tagged_template() {
                    expression = self.finish_node_data(
                        NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                            expression: Some(expression),
                            type_arguments: Some(type_arguments),
                        }),
                        pos,
                    );
                }
            }
        }
        if matches!(
            self.token(),
            SyntaxKind::OpenParenToken | SyntaxKind::DotToken | SyntaxKind::OpenBracketToken
        ) {
            return expression;
        }
        self.parse_expected_token(
            SyntaxKind::DotToken,
            Some(&gen::super_must_be_followed_by_an_argument_list_or_member_access),
        );
        let name = self.parse_right_side_of_dot(true, true, true);
        self.finish_node_data(
            NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                expression: Some(expression),
                question_dot_token: None,
                name: Some(name),
            }),
            pos,
        )
    }

    fn parse_left_hand_side_expression_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = if self.token() == SyntaxKind::ImportKeyword {
            if self.look_ahead(|parser| parser.next_token_is_open_paren_or_less_than()) {
                // tsc sets PossiblyContainsDynamicImport (no sourceFlags slot).
                self.parse_token_node()
            } else if self.look_ahead(|parser| parser.next_token_is_dot()) {
                // tsc createMetaProperty(ImportKeyword, ...); the keywordToken
                // and the defer/ImportMeta sourceFlags have no slots.
                self.next_token();
                self.next_token();
                let name = self.parse_identifier_name(None);
                self.finish_node_data(
                    NodeData::MetaProperty(MetaPropertyData { name: Some(name) }),
                    pos,
                )
            } else {
                self.parse_member_expression_or_higher()
            }
        } else if self.token() == SyntaxKind::SuperKeyword {
            self.parse_super_expression()
        } else {
            self.parse_member_expression_or_higher()
        };
        self.parse_call_expression_rest(pos, expression)
    }

    fn next_token_is_dot(&mut self) -> bool {
        self.next_token() == SyntaxKind::DotToken
    }

    fn parse_member_expression_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_primary_expression();
        self.parse_member_expression_rest(pos, expression, true)
    }

    fn parse_primary_expression(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::ThisKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword => self.parse_token_node(),
            SyntaxKind::Identifier => self.parse_identifier(),
            SyntaxKind::PrivateIdentifier => self.parse_private_identifier(),
            SyntaxKind::StringLiteral => self.parse_string_literal(),
            SyntaxKind::NumericLiteral => self.parse_numeric_literal(),
            SyntaxKind::BigIntLiteral => self.parse_big_int_literal(),
            SyntaxKind::NoSubstitutionTemplateLiteral => {
                // tsc: an untagged template with invalid parts re-scans WITH
                // error reporting (the initial backtick scan is silent).
                if self.scanner.token_flags_are_invalid() {
                    self.re_scan_template_token(false);
                }
                self.parse_no_substitution_template_literal()
            }
            SyntaxKind::FunctionKeyword => self.parse_function_expression(),
            SyntaxKind::ClassKeyword => self.parse_class_expression(),
            SyntaxKind::NewKeyword => self.parse_new_expression_stub(),
            SyntaxKind::SlashToken | SyntaxKind::SlashEqualsToken => {
                if self.scanner.re_scan_slash_token(false) == SyntaxKind::RegularExpressionLiteral {
                    self.drain_scanner_errors();
                    self.parse_regular_expression_literal()
                } else {
                    let is_identifier = self.is_identifier();
                    self.create_identifier_node(
                        is_identifier,
                        Some(&gen::Expression_expected),
                        None,
                    )
                }
            }
            SyntaxKind::TemplateHead => self.parse_template_expression(false),
            SyntaxKind::OpenParenToken => self.parse_parenthesized_expression(),
            SyntaxKind::OpenBracketToken => self.parse_array_literal_expression(),
            SyntaxKind::OpenBraceToken => self.parse_object_literal_expression(),
            SyntaxKind::AsyncKeyword
                if self.look_ahead(|parser| {
                    parser.next_token() == SyntaxKind::FunctionKeyword
                        && !parser.scanner.has_preceding_line_break()
                }) =>
            {
                self.parse_function_expression()
            }
            SyntaxKind::AtToken => self.parse_decorated_expression(),
            _ => {
                let is_identifier = self.is_identifier();
                self.create_identifier_node(is_identifier, Some(&gen::Expression_expected), None)
            }
        }
    }

    /// tsc parseDecoratedExpression: `@dec class {}` in expression position.
    fn parse_decorated_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers(true, false, false);
        if self.token() == SyntaxKind::ClassKeyword {
            return self.parse_class_declaration_or_expression(pos, modifiers, false);
        }
        let missing = self.create_missing_node(
            SyntaxKind::MissingDeclaration,
            true,
            Some(&gen::Expression_expected),
            &[],
        );
        let node = self.arena.node_mut(missing);
        node.pos = pos as u32;
        if let NodeData::MissingDeclaration(data) = &mut node.data {
            data.modifiers = modifiers;
        }
        missing
    }

    fn parse_member_expression_rest(
        &mut self,
        pos: usize,
        mut expression: NodeId,
        allow_optional_chain: bool,
    ) -> NodeId {
        loop {
            let mut question_dot_token = None;
            let is_property_access = if allow_optional_chain
                && self.is_start_of_optional_property_or_element_access_chain()
            {
                question_dot_token = self.parse_optional_token(SyntaxKind::QuestionDotToken);
                self.is_identifier()
            } else {
                self.parse_optional(SyntaxKind::DotToken)
            };

            if is_property_access {
                expression =
                    self.parse_property_access_expression_rest(pos, expression, question_dot_token);
                continue;
            }

            // tsc: in the Decorator context `[` is not element access — it
            // could start a ComputedPropertyName.
            if !self.in_decorator_context() && self.parse_optional(SyntaxKind::OpenBracketToken) {
                expression =
                    self.parse_element_access_expression_rest(pos, expression, question_dot_token);
                continue;
            }

            if self.is_template_start_of_tagged_template() {
                let (tag, type_arguments) = if question_dot_token.is_none() {
                    self.split_expression_with_type_arguments(expression)
                } else {
                    (expression, None)
                };
                expression =
                    self.parse_tagged_template_rest(pos, tag, question_dot_token, type_arguments);
                continue;
            }

            if question_dot_token.is_none() {
                if self.token() == SyntaxKind::ExclamationToken
                    && !self.scanner.has_preceding_line_break()
                {
                    self.next_token();
                    expression = self.finish_node_data(
                        NodeData::NonNullExpression(NonNullExpressionData {
                            expression: Some(expression),
                        }),
                        pos,
                    );
                    continue;
                }

                if let Some(type_arguments) = self.try_parse_type_arguments_in_expression() {
                    expression = self.finish_node_data(
                        NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
                            expression: Some(expression),
                            type_arguments: Some(type_arguments),
                        }),
                        pos,
                    );
                    continue;
                }
            }

            return expression;
        }
    }

    fn parse_call_expression_rest(&mut self, pos: usize, mut expression: NodeId) -> NodeId {
        loop {
            expression = self.parse_member_expression_rest(pos, expression, true);

            let question_dot_token = self.parse_optional_token(SyntaxKind::QuestionDotToken);
            let type_arguments = if question_dot_token.is_some() {
                self.try_parse_type_arguments_in_expression()
            } else {
                None
            };

            if question_dot_token.is_some() && self.is_template_start_of_tagged_template() {
                expression = self.parse_tagged_template_rest(
                    pos,
                    expression,
                    question_dot_token,
                    type_arguments,
                );
                continue;
            }

            if type_arguments.is_some() || self.token() == SyntaxKind::OpenParenToken {
                let (callee, expression_type_arguments) = if question_dot_token.is_none()
                    && self.arena.node(expression).kind == SyntaxKind::ExpressionWithTypeArguments
                {
                    self.split_expression_with_type_arguments(expression)
                } else {
                    (expression, None)
                };
                let arguments = self.parse_argument_list();
                expression = self.finish_node_data(
                    NodeData::CallExpression(CallExpressionData {
                        expression: Some(callee),
                        question_dot_token,
                        type_arguments: type_arguments.or(expression_type_arguments),
                        arguments: Some(arguments),
                    }),
                    pos,
                );
                continue;
            }

            if let Some(question_dot_token) = question_dot_token {
                let name = self.create_missing_node(
                    SyntaxKind::Identifier,
                    false,
                    Some(&gen::Identifier_expected),
                    &[],
                );
                expression = self.finish_node_data(
                    NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                        expression: Some(expression),
                        question_dot_token: Some(question_dot_token),
                        name: Some(name),
                    }),
                    pos,
                );
            }

            break;
        }

        expression
    }

    fn parse_property_access_expression_rest(
        &mut self,
        pos: usize,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
    ) -> NodeId {
        let name = self.parse_right_side_of_dot(true, true, true);
        self.finish_node_data(
            NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                expression: Some(expression),
                question_dot_token,
                name: Some(name),
            }),
            pos,
        )
    }

    fn parse_element_access_expression_rest(
        &mut self,
        pos: usize,
        expression: NodeId,
        question_dot_token: Option<NodeId>,
    ) -> NodeId {
        let argument_expression = if self.token() == SyntaxKind::CloseBracketToken {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::An_element_access_expression_should_take_an_argument),
                &[],
            )
        } else {
            self.allow_in(|parser| parser.parse_expression())
        };
        self.parse_expected(SyntaxKind::CloseBracketToken, None);
        self.finish_node_data(
            NodeData::ElementAccessExpression(ElementAccessExpressionData {
                expression: Some(expression),
                question_dot_token,
                argument_expression: Some(argument_expression),
            }),
            pos,
        )
    }

    fn parse_tagged_template_rest(
        &mut self,
        pos: usize,
        tag: NodeId,
        question_dot_token: Option<NodeId>,
        type_arguments: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let template = if self.token() == SyntaxKind::NoSubstitutionTemplateLiteral {
            self.scanner.re_scan_template_token(true);
            self.drain_scanner_errors();
            self.parse_no_substitution_template_literal()
        } else {
            self.parse_template_expression(true)
        };
        self.finish_node_data(
            NodeData::TaggedTemplateExpression(TaggedTemplateExpressionData {
                tag: Some(tag),
                question_dot_token,
                type_arguments,
                template: Some(template),
            }),
            pos,
        )
    }

    fn parse_argument_list(&mut self) -> crate::NodeArrayId {
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let arguments = self.parse_delimited_list(
            ParsingContext::ArgumentExpressions,
            |parser| Some(parser.parse_argument_or_array_literal_element()),
            false,
        );
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        arguments
    }

    fn try_parse_type_arguments_in_expression(&mut self) -> Option<crate::NodeArrayId> {
        self.try_parse(|parser| parser.parse_type_arguments_in_expression())
    }

    fn parse_type_arguments_in_expression(&mut self) -> Option<crate::NodeArrayId> {
        if self.scanner.re_scan_less_than_token() != SyntaxKind::LessThanToken {
            return None;
        }
        self.next_token();
        let type_arguments = self.parse_delimited_list(
            ParsingContext::TypeArguments,
            |parser| Some(parser.parse_type()),
            false,
        );
        if self.scanner.re_scan_greater_token() != SyntaxKind::GreaterThanToken {
            return None;
        }
        self.next_token();

        self.can_follow_type_arguments_in_expression()
            .then_some(type_arguments)
    }

    fn can_follow_type_arguments_in_expression(&mut self) -> bool {
        match self.token() {
            SyntaxKind::OpenParenToken
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateHead => true,
            SyntaxKind::LessThanToken
            | SyntaxKind::GreaterThanToken
            | SyntaxKind::PlusToken
            | SyntaxKind::MinusToken => false,
            _ => {
                self.scanner.has_preceding_line_break()
                    || self.is_binary_operator()
                    || !self.is_start_of_expression()
            }
        }
    }

    fn is_start_of_optional_property_or_element_access_chain(&mut self) -> bool {
        self.token() == SyntaxKind::QuestionDotToken
            && self.look_ahead(|parser| {
                parser.next_token();
                parser.is_identifier()
                    || parser.token() == SyntaxKind::OpenBracketToken
                    || parser.is_template_start_of_tagged_template()
            })
    }

    fn is_template_start_of_tagged_template(&self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::NoSubstitutionTemplateLiteral | SyntaxKind::TemplateHead
        )
    }

    /// tsc parseRightSideOfDot.
    fn parse_right_side_of_dot(
        &mut self,
        allow_identifier_names: bool,
        allow_private_identifiers: bool,
        allow_unicode_escape_sequence_in_identifier_name: bool,
    ) -> NodeId {
        // A keyword on a fresh line followed by another identifier likely
        // starts the next construct; give the dot a missing name instead.
        if self.scanner.has_preceding_line_break() && token_is_identifier_or_keyword(self.token()) {
            let matches_pattern =
                self.look_ahead(|parser| parser.next_token_is_identifier_or_keyword_on_same_line());
            if matches_pattern {
                return self.create_missing_node(
                    SyntaxKind::Identifier,
                    true,
                    Some(&gen::Identifier_expected),
                    &[],
                );
            }
        }
        if self.token() == SyntaxKind::PrivateIdentifier {
            let node = self.parse_private_identifier();
            return if allow_private_identifiers {
                node
            } else {
                self.create_missing_node(
                    SyntaxKind::Identifier,
                    true,
                    Some(&gen::Identifier_expected),
                    &[],
                )
            };
        }
        if allow_identifier_names {
            return if allow_unicode_escape_sequence_in_identifier_name {
                self.parse_identifier_name(None)
            } else {
                self.parse_identifier_name_error_on_unicode_escape_sequence()
            };
        }
        self.parse_identifier_or_missing()
    }

    /// tsc parseIdentifier: an identifier, or a missing node with an error.
    fn parse_identifier_or_missing(&mut self) -> NodeId {
        let is_identifier = self.is_identifier();
        self.create_identifier_node(is_identifier, None, None)
    }

    /// tsc parseIdentifierName: reserved words allowed.
    fn parse_identifier_name(&mut self, message: Option<&'static DiagnosticMessage>) -> NodeId {
        let is_identifier = token_is_identifier_or_keyword(self.token());
        self.create_identifier_node(is_identifier, message, None)
    }

    /// tsc parseEntityName.
    fn parse_entity_name(
        &mut self,
        allow_reserved_words: bool,
        diagnostic: Option<&'static DiagnosticMessage>,
    ) -> NodeId {
        let pos = self.node_pos();
        let mut entity = if allow_reserved_words {
            self.parse_identifier_name(diagnostic)
        } else {
            let is_identifier = self.is_identifier();
            self.create_identifier_node(is_identifier, diagnostic, None)
        };
        while self.parse_optional(SyntaxKind::DotToken) {
            if self.token() == SyntaxKind::LessThanToken {
                // The entity is followed by type arguments; the caller
                // (parseTypeReference et al.) picks them up.
                break;
            }
            let right = self.parse_right_side_of_dot(allow_reserved_words, false, true);
            entity = self.finish_node_data(
                NodeData::QualifiedName(QualifiedNameData {
                    left: Some(entity),
                    right: Some(right),
                }),
                pos,
            );
        }
        entity
    }

    fn parse_entity_name_of_type_reference(&mut self) -> NodeId {
        self.parse_entity_name(true, Some(&gen::Type_expected))
    }

    fn next_token_is_identifier_or_keyword_on_same_line(&mut self) -> bool {
        self.next_token();
        token_is_identifier_or_keyword(self.token()) && !self.scanner.has_preceding_line_break()
    }

    fn next_token_is_identifier_or_keyword_or_greater_than(&mut self) -> bool {
        self.next_token();
        token_is_identifier_or_keyword(self.token()) || self.token() == SyntaxKind::GreaterThanToken
    }

    fn expression_with_type_arguments_parts(
        &self,
        expression: NodeId,
    ) -> Option<(NodeId, crate::NodeArrayId)> {
        self.arena
            .node(expression)
            .data
            .as_expression_with_type_arguments()
            .and_then(|data| Some((data.expression?, data.type_arguments?)))
    }

    fn split_expression_with_type_arguments(
        &self,
        expression: NodeId,
    ) -> (NodeId, Option<crate::NodeArrayId>) {
        self.expression_with_type_arguments_parts(expression)
            .map(|(expression, type_arguments)| (expression, Some(type_arguments)))
            .unwrap_or((expression, None))
    }

    fn parse_parenthesized_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let expression = if self.token() == SyntaxKind::CloseParenToken {
            self.create_identifier_node(false, Some(&gen::Expression_expected), None)
        } else {
            self.allow_in(|parser| parser.parse_expression())
        };
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        self.finish_node_data(
            NodeData::ParenthesizedExpression(ParenthesizedExpressionData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_new_expression_stub(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::NewKeyword, None);
        if self.parse_optional(SyntaxKind::DotToken) {
            let name = self.parse_identifier();
            return self.finish_node_data(
                NodeData::MetaProperty(MetaPropertyData { name: Some(name) }),
                pos,
            );
        }

        let expression = if self.is_start_of_left_hand_side_expression() {
            let expression_pos = self.node_pos();
            let expression = self.parse_primary_expression();
            let expression = self.parse_member_expression_rest(expression_pos, expression, false);
            Some(expression)
        } else {
            {
                let is_identifier = self.is_identifier();
                Some(self.create_identifier_node(
                    is_identifier,
                    Some(&gen::Expression_expected),
                    None,
                ))
            }
        };
        let (expression, type_arguments) = expression
            .map(|expression| self.split_expression_with_type_arguments(expression))
            .map(|(expression, type_arguments)| (Some(expression), type_arguments))
            .unwrap_or((None, None));
        let arguments = if self.token() == SyntaxKind::OpenParenToken {
            Some(self.parse_argument_list())
        } else {
            None
        };
        self.finish_node_data(
            NodeData::NewExpression(NewExpressionData {
                expression,
                question_dot_token: None,
                type_arguments,
                arguments,
            }),
            pos,
        )
    }

    fn parse_array_literal_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBracketToken, None);
        let elements = self.parse_delimited_list(
            ParsingContext::ArrayLiteralMembers,
            |parser| Some(parser.parse_argument_or_array_literal_element()),
            false,
        );
        self.parse_expected(SyntaxKind::CloseBracketToken, None);
        self.finish_node_data(
            NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                elements: Some(elements),
            }),
            pos,
        )
    }

    fn parse_object_literal_expression(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBraceToken, None);
        let properties = self.parse_delimited_list(
            ParsingContext::ObjectLiteralMembers,
            |parser| Some(parser.parse_object_literal_element()),
            true,
        );
        self.parse_expected(SyntaxKind::CloseBraceToken, None);
        self.finish_node_data(
            NodeData::ObjectLiteralExpression(ObjectLiteralExpressionData {
                properties: Some(properties),
            }),
            pos,
        )
    }

    fn parse_argument_or_array_literal_element(&mut self) -> NodeId {
        if self.token() == SyntaxKind::DotDotDotToken {
            return self.parse_spread_element();
        }
        if self.token() == SyntaxKind::CommaToken {
            let pos = self.node_pos();
            return self
                .finish_node_data(NodeData::OmittedExpression(OmittedExpressionData {}), pos);
        }
        self.parse_assignment_expression_or_higher(true)
    }

    fn parse_spread_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::DotDotDotToken, None);
        let expression = self.parse_assignment_expression_or_higher(true);
        self.finish_node_data(
            NodeData::SpreadElement(SpreadElementData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    fn parse_object_literal_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.parse_optional(SyntaxKind::DotDotDotToken) {
            let expression = self.parse_assignment_expression_or_higher(true);
            return self.finish_node_data(
                NodeData::SpreadAssignment(SpreadAssignmentData {
                    expression: Some(expression),
                }),
                pos,
            );
        }

        let modifiers = self.parse_modifiers(true, false, false);
        if self.parse_contextual_modifier(SyntaxKind::GetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, SyntaxKind::GetAccessor, false);
        }
        if self.parse_contextual_modifier(SyntaxKind::SetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, SyntaxKind::SetAccessor, false);
        }

        let asterisk_token = self.parse_optional_token(SyntaxKind::AsteriskToken);
        let token_is_identifier = self.is_identifier();
        let name = self.parse_property_name();
        let question_token = self.parse_optional_token(SyntaxKind::QuestionToken);
        let exclamation_token = self.parse_optional_token(SyntaxKind::ExclamationToken);

        if asterisk_token.is_some()
            || matches!(
                self.token(),
                SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken
            )
        {
            return self.parse_method_declaration(
                pos,
                modifiers,
                asterisk_token,
                name,
                question_token,
                exclamation_token,
                None,
            );
        }

        if token_is_identifier && self.token() != SyntaxKind::ColonToken {
            let equals_token = self.parse_optional_token(SyntaxKind::EqualsToken);
            let object_assignment_initializer = if equals_token.is_some() {
                Some(self.allow_in(|parser| parser.parse_assignment_expression_or_higher(true)))
            } else {
                None
            };
            return self.finish_node_data(
                NodeData::ShorthandPropertyAssignment(ShorthandPropertyAssignmentData {
                    modifiers,
                    name: Some(name),
                    question_token,
                    exclamation_token,
                    equals_token,
                    object_assignment_initializer,
                }),
                pos,
            );
        }

        self.parse_expected(SyntaxKind::ColonToken, None);
        let initializer =
            self.allow_in(|parser| parser.parse_assignment_expression_or_higher(true));
        self.finish_node_data(
            NodeData::PropertyAssignment(PropertyAssignmentData {
                modifiers,
                name: Some(name),
                question_token,
                exclamation_token,
                initializer: Some(initializer),
            }),
            pos,
        )
    }

    fn parse_property_name(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::OpenBracketToken => self.parse_computed_property_name(),
            SyntaxKind::StringLiteral => self.parse_string_literal(),
            SyntaxKind::NumericLiteral => self.parse_numeric_literal(),
            SyntaxKind::BigIntLiteral => self.parse_big_int_literal(),
            SyntaxKind::PrivateIdentifier => self.parse_private_identifier(),
            _ => self.parse_identifier_name(None),
        }
    }

    fn parse_computed_property_name(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBracketToken, None);
        let expression = self.allow_in(|parser| parser.parse_expression());
        self.parse_expected(SyntaxKind::CloseBracketToken, None);
        self.finish_node_data(
            NodeData::ComputedPropertyName(ComputedPropertyNameData {
                expression: Some(expression),
            }),
            pos,
        )
    }

    /// tsc parseMethodDeclaration (its 8-arg shape is kept verbatim).
    #[allow(clippy::too_many_arguments)]
    fn parse_method_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        asterisk_token: Option<NodeId>,
        name: NodeId,
        question_token: Option<NodeId>,
        exclamation_token: Option<NodeId>,
        diagnostic_message: Option<&'static DiagnosticMessage>,
    ) -> NodeId {
        let is_generator = asterisk_token.is_some();
        let is_async = self.modifiers_contain(modifiers, SyntaxKind::AsyncKeyword);
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(is_generator, is_async);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(
            is_generator,
            is_async,
            false,
            diagnostic_message,
        );
        self.finish_node_data(
            NodeData::MethodDeclaration(MethodDeclarationData {
                modifiers,
                asterisk_token,
                name: Some(name),
                question_token,
                exclamation_token,
                r#type,
                type_parameters,
                parameters: Some(parameters),
                body,
            }),
            pos,
        )
    }

    fn modifiers_contain(&self, modifiers: Option<crate::NodeArrayId>, kind: SyntaxKind) -> bool {
        modifiers.is_some_and(|list| {
            self.arena
                .node_array(list)
                .nodes
                .iter()
                .any(|&modifier| self.arena.node(modifier).kind == kind)
        })
    }

    fn is_parameter_name_start(&self) -> bool {
        self.is_binding_identifier()
            || matches!(
                self.token(),
                SyntaxKind::OpenBracketToken | SyntaxKind::OpenBraceToken
            )
    }

    fn parse_parameter(&mut self, in_outer_await_context: bool) -> NodeId {
        self.parse_parameter_worker(in_outer_await_context, true)
            .expect("allow_ambiguity parameter always yields")
    }

    fn parse_parameter_for_speculation(&mut self, in_outer_await_context: bool) -> Option<NodeId> {
        self.parse_parameter_worker(in_outer_await_context, false)
    }

    fn parse_parameter_worker(
        &mut self,
        in_outer_await_context: bool,
        allow_ambiguity: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        // Modifier parsing runs in the enclosing function's await context.
        let modifiers = if in_outer_await_context {
            self.do_in_context(NodeFlags::AWAIT_CONTEXT, NodeFlags::NONE, |parser| {
                parser.parse_modifiers(true, false, false)
            })
        } else {
            self.do_in_context(NodeFlags::NONE, NodeFlags::AWAIT_CONTEXT, |parser| {
                parser.parse_modifiers(true, false, false)
            })
        };

        if self.token() == SyntaxKind::ThisKeyword {
            let name = self.parse_identifier();
            let r#type = self.parse_type_annotation();
            if let Some(list) = modifiers {
                if let Some(&modifier) = self.arena.node_array(list).nodes.first() {
                    let node = self.arena.node(modifier);
                    let (start, length) = (node.pos as usize, (node.end - node.pos) as usize);
                    self.parse_error_at_position(
                        start,
                        length,
                        &gen::Neither_decorators_nor_modifiers_may_be_applied_to_this_parameters,
                        &[],
                    );
                }
            }
            return Some(self.finish_node_data(
                NodeData::Parameter(ParameterData {
                    modifiers,
                    dot_dot_dot_token: None,
                    name: Some(name),
                    question_token: None,
                    r#type,
                    initializer: None,
                }),
                pos,
            ));
        }

        let dot_dot_dot_token = self.parse_optional_token(SyntaxKind::DotDotDotToken);
        if !allow_ambiguity && !self.is_parameter_name_start() {
            return None;
        }
        let name = self.parse_name_of_parameter(modifiers.is_some());
        let question_token = self.parse_optional_token(SyntaxKind::QuestionToken);
        let r#type = self.parse_type_annotation();
        let initializer = self.parse_initializer();
        Some(self.finish_node_data(
            NodeData::Parameter(ParameterData {
                modifiers,
                dot_dot_dot_token,
                name: Some(name),
                question_token,
                r#type,
                initializer,
            }),
            pos,
        ))
    }

    fn parse_name_of_parameter(&mut self, has_modifiers: bool) -> NodeId {
        let name = self.parse_identifier_or_pattern();
        let name_node = self.arena.node(name);
        if name_node.pos == name_node.end && !has_modifiers && self.is_modifier_kind(self.token()) {
            // A modifier alone ("void foo(private)") would loop forever;
            // consume it so the list makes progress (tsc parseNameOfParameter).
            self.next_token();
        }
        name
    }

    fn parse_parameters_worker(
        &mut self,
        yield_context: bool,
        await_context: bool,
        allow_ambiguity: bool,
    ) -> Option<crate::NodeArrayId> {
        let saved_yield_context = self.in_yield_context();
        let saved_await_context = self.in_await_context();
        self.set_yield_context(yield_context);
        self.set_await_context(await_context);
        let parameters = self.parse_delimited_list_worker(
            ParsingContext::Parameters,
            |parser| {
                if allow_ambiguity {
                    Some(parser.parse_parameter(saved_await_context))
                } else {
                    parser.parse_parameter_for_speculation(saved_await_context)
                }
            },
            false,
        );
        self.set_yield_context(saved_yield_context);
        self.set_await_context(saved_await_context);
        parameters
    }

    fn parse_parameters(&mut self, yield_context: bool, await_context: bool) -> crate::NodeArrayId {
        if !self.parse_expected(SyntaxKind::OpenParenToken, None) {
            // tsc createMissingList
            return self.arena.missing_array(self.node_pos());
        }
        let parameters = self
            .parse_parameters_worker(yield_context, await_context, true)
            .expect("allow_ambiguity parameter list always yields");
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        parameters
    }

    fn parse_return_type(&mut self, return_token: SyntaxKind, is_type: bool) -> Option<NodeId> {
        if self.should_parse_return_type(return_token, is_type) {
            Some(self.allow_conditional_types_and(|parser| parser.parse_type_or_type_predicate()))
        } else {
            None
        }
    }

    fn should_parse_return_type(&mut self, return_token: SyntaxKind, is_type: bool) -> bool {
        if return_token == SyntaxKind::EqualsGreaterThanToken {
            self.parse_expected(return_token, None);
            true
        } else if self.parse_optional(SyntaxKind::ColonToken) {
            true
        } else if is_type && self.token() == SyntaxKind::EqualsGreaterThanToken {
            self.parse_error_at_current_token(
                &gen::_0_expected,
                &[&token_to_string(SyntaxKind::ColonToken)],
            );
            self.next_token();
            true
        } else {
            false
        }
    }

    fn parse_function_block(
        &mut self,
        is_generator: bool,
        is_async: bool,
        ignore_missing_open_brace: bool,
        diagnostic_message: Option<&'static DiagnosticMessage>,
    ) -> NodeId {
        let (set, clear) = context_flags_for_function_body(is_generator, is_async);
        let save_decorator_context = self.in_decorator_context();
        if save_decorator_context {
            self.set_decorator_context(false);
        }
        let block = self.do_in_context(set, clear, |parser| {
            parser.parse_block(ignore_missing_open_brace, diagnostic_message)
        });
        if save_decorator_context {
            self.set_decorator_context(true);
        }
        block
    }

    /// tsc parseFunctionExpression.
    fn parse_function_expression(&mut self) -> NodeId {
        let saved_decorator_context = self.in_decorator_context();
        self.set_decorator_context(false);
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers(false, false, false);
        self.parse_expected(SyntaxKind::FunctionKeyword, None);
        let asterisk_token = self.parse_optional_token(SyntaxKind::AsteriskToken);
        let is_generator = asterisk_token.is_some();
        let is_async = self.modifiers_contain(modifiers, SyntaxKind::AsyncKeyword);
        let name = if is_generator && is_async {
            self.do_in_context(
                NodeFlags::YIELD_CONTEXT | NodeFlags::AWAIT_CONTEXT,
                NodeFlags::NONE,
                |parser| parser.parse_optional_binding_identifier(),
            )
        } else if is_generator {
            self.do_in_context(NodeFlags::YIELD_CONTEXT, NodeFlags::NONE, |parser| {
                parser.parse_optional_binding_identifier()
            })
        } else if is_async {
            self.do_in_context(NodeFlags::AWAIT_CONTEXT, NodeFlags::NONE, |parser| {
                parser.parse_optional_binding_identifier()
            })
        } else {
            self.parse_optional_binding_identifier()
        };
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(is_generator, is_async);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        let body = Some(self.parse_function_block(is_generator, is_async, false, None));
        self.set_decorator_context(saved_decorator_context);
        self.finish_node_data(
            NodeData::FunctionExpression(FunctionExpressionData {
                modifiers,
                asterisk_token,
                name,
                r#type,
                type_parameters,
                parameters: Some(parameters),
                body,
            }),
            pos,
        )
    }

    /// tsc parseOptionalBindingIdentifier.
    fn parse_optional_binding_identifier(&mut self) -> Option<NodeId> {
        if self.is_binding_identifier() {
            Some(self.parse_binding_identifier())
        } else {
            None
        }
    }

    fn parse_template_expression(&mut self, is_tagged_template: bool) -> NodeId {
        let pos = self.node_pos();
        let head = self.parse_template_head(is_tagged_template);
        let spans_pos = self.node_pos();
        let mut spans = Vec::new();
        loop {
            let span = self.parse_template_span(is_tagged_template);
            let literal = self
                .arena
                .node(span)
                .data
                .as_template_span()
                .and_then(|data| data.literal)
                .map(|literal| self.arena.node(literal).kind);
            spans.push(span);
            if literal != Some(SyntaxKind::TemplateMiddle) {
                break;
            }
        }
        let template_spans = self
            .arena
            .alloc_array(spans, spans_pos, self.node_pos(), false);
        self.finish_node_data(
            NodeData::TemplateExpression(TemplateExpressionData {
                head: Some(head),
                template_spans: Some(template_spans),
            }),
            pos,
        )
    }

    fn parse_template_span(&mut self, is_tagged_template: bool) -> NodeId {
        let pos = self.node_pos();
        let expression = self.allow_in(|parser| parser.parse_expression());
        let literal = self.parse_literal_of_template_span(is_tagged_template);
        self.finish_node_data(
            NodeData::TemplateSpan(TemplateSpanData {
                expression: Some(expression),
                literal: Some(literal),
            }),
            pos,
        )
    }

    fn parse_literal_of_template_span(&mut self, is_tagged_template: bool) -> NodeId {
        if self.token() == SyntaxKind::CloseBraceToken {
            self.scanner.re_scan_template_token(is_tagged_template);
            self.drain_scanner_errors();
            self.parse_template_middle_or_tail()
        } else {
            self.create_missing_node(
                SyntaxKind::TemplateTail,
                false,
                Some(&gen::_0_expected),
                &["}"],
            )
        }
    }

    /// tsc parseTemplateHead: an untagged template with invalid parts
    /// re-scans WITH error reporting (the initial backtick scan is silent).
    fn parse_template_head(&mut self, is_tagged_template: bool) -> NodeId {
        if !is_tagged_template && self.scanner.token_flags_are_invalid() {
            self.re_scan_template_token(false);
        }
        self.parse_template_fragment(SyntaxKind::TemplateHead)
    }

    /// tsc parser-side reScanTemplateToken.
    fn re_scan_template_token(&mut self, is_tagged_template: bool) -> SyntaxKind {
        let token = self.scanner.re_scan_template_token(is_tagged_template);
        self.drain_scanner_errors();
        token
    }

    fn parse_template_middle_or_tail(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::TemplateMiddle => self.parse_template_fragment(SyntaxKind::TemplateMiddle),
            SyntaxKind::TemplateTail => self.parse_template_fragment(SyntaxKind::TemplateTail),
            _ => self.create_missing_node(
                SyntaxKind::TemplateTail,
                false,
                Some(&gen::_0_expected),
                &["}"],
            ),
        }
    }

    fn parse_template_fragment(&mut self, kind: SyntaxKind) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let data = match kind {
            SyntaxKind::TemplateHead => NodeData::TemplateHead(TemplateHeadData {
                text,
                raw_text: None,
            }),
            SyntaxKind::TemplateMiddle => NodeData::TemplateMiddle(TemplateMiddleData {
                text,
                raw_text: None,
            }),
            SyntaxKind::TemplateTail => NodeData::TemplateTail(TemplateTailData {
                text,
                raw_text: None,
            }),
            _ => unreachable!("template fragment kind"),
        };
        let id = self.arena.alloc_node(data, pos, end, NodeFlags::NONE);
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_string_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::StringLiteral(StringLiteralData { text }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_numeric_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::NumericLiteral(NumericLiteralData { text }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_big_int_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::BigIntLiteral(BigIntLiteralData { text }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_regular_expression_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::RegularExpressionLiteral(RegularExpressionLiteralData { text }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_no_substitution_template_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let end = self.scanner.pos();
        let text = self.current_token_text();
        let id = self.arena.alloc_node(
            NodeData::NoSubstitutionTemplateLiteral(NoSubstitutionTemplateLiteralData { text }),
            pos,
            end,
            NodeFlags::NONE,
        );
        self.next_token();
        self.finish_node_at(id, pos, end)
    }

    fn parse_type_annotation(&mut self) -> Option<NodeId> {
        if self.parse_optional(SyntaxKind::ColonToken) {
            Some(self.parse_type())
        } else {
            None
        }
    }

    /// tsc parseBracketedList.
    fn parse_bracketed_list(
        &mut self,
        context: ParsingContext,
        parse_element: impl FnMut(&mut Self) -> Option<NodeId>,
        open: SyntaxKind,
        close: SyntaxKind,
    ) -> crate::NodeArrayId {
        if self.parse_expected(open, None) {
            let result = self.parse_delimited_list(context, parse_element, false);
            self.parse_expected(close, None);
            result
        } else {
            self.arena.missing_array(self.node_pos())
        }
    }

    /// Kind-only nodes (ThisType, JSDoc sentinel types) carry no data.
    fn finish_kind_only_node(&mut self, kind: SyntaxKind, pos: usize) -> NodeId {
        let id = self
            .arena
            .alloc_token(kind, pos, self.scanner.full_start_pos(), NodeFlags::NONE);
        self.finish_node(id, pos)
    }

    fn parse_template_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let head = self.parse_template_head(false);
        let spans_pos = self.node_pos();
        let mut spans = Vec::new();
        loop {
            let span = self.parse_template_type_span();
            let literal_kind = match &self.arena.node(span).data {
                NodeData::TemplateLiteralTypeSpan(data) => {
                    data.literal.map(|literal| self.arena.node(literal).kind)
                }
                _ => None,
            };
            spans.push(span);
            if literal_kind != Some(SyntaxKind::TemplateMiddle) {
                break;
            }
        }
        let template_spans = self
            .arena
            .alloc_array(spans, spans_pos, self.node_pos(), false);
        self.finish_node_data(
            NodeData::TemplateLiteralType(TemplateLiteralTypeData {
                readonly_pos: 0.0,
                readonly_end: 0.0,
                readonly_kind: SyntaxKind::TemplateLiteralType,
                readonly_flags: NodeId::default(),
                readonly_parent: NodeId::default(),
                readonly_head: NodePayload::String(String::new()),
                readonly_template_spans: crate::NodeArrayId::default(),
                head: Some(head),
                template_spans: Some(template_spans),
            }),
            pos,
        )
    }

    fn parse_template_type_span(&mut self) -> NodeId {
        let pos = self.node_pos();
        let r#type = self.parse_type();
        let literal = self.parse_literal_of_template_span(false);
        self.finish_node_data(
            NodeData::TemplateLiteralTypeSpan(TemplateLiteralTypeSpanData {
                r#type: Some(r#type),
                literal: Some(literal),
            }),
            pos,
        )
    }

    fn parse_type_arguments_of_type_reference(&mut self) -> Option<crate::NodeArrayId> {
        if !self.scanner.has_preceding_line_break()
            && self.scanner.re_scan_less_than_token() == SyntaxKind::LessThanToken
        {
            Some(self.parse_bracketed_list(
                ParsingContext::TypeArguments,
                |parser| Some(parser.parse_type()),
                SyntaxKind::LessThanToken,
                SyntaxKind::GreaterThanToken,
            ))
        } else {
            None
        }
    }

    /// tsc tryParseTypeArguments (not speculative: `<` commits the list).
    fn try_parse_type_arguments(&mut self) -> Option<crate::NodeArrayId> {
        if self.token() == SyntaxKind::LessThanToken {
            Some(self.parse_bracketed_list(
                ParsingContext::TypeArguments,
                |parser| Some(parser.parse_type()),
                SyntaxKind::LessThanToken,
                SyntaxKind::GreaterThanToken,
            ))
        } else {
            None
        }
    }

    fn parse_type_reference(&mut self) -> NodeId {
        let pos = self.node_pos();
        let type_name = self.parse_entity_name_of_type_reference();
        let type_arguments = self.parse_type_arguments_of_type_reference();
        self.finish_node_data(
            NodeData::TypeReference(TypeReferenceData {
                type_name: Some(type_name),
                type_arguments,
            }),
            pos,
        )
    }

    fn type_has_arrow_function_blocking_parse_error(&self, id: NodeId) -> bool {
        let node = self.arena.node(id);
        match &node.data {
            NodeData::TypeReference(data) => data
                .type_name
                .is_none_or(|type_name| !self.node_is_present(type_name)),
            NodeData::FunctionType(FunctionTypeData {
                parameters, r#type, ..
            })
            | NodeData::ConstructorType(ConstructorTypeData {
                parameters, r#type, ..
            }) => {
                parameters.is_some_and(|list| self.arena.node_array(list).is_missing_list)
                    || r#type.is_some_and(|t| self.type_has_arrow_function_blocking_parse_error(t))
            }
            NodeData::ParenthesizedType(data) => data
                .r#type
                .is_some_and(|t| self.type_has_arrow_function_blocking_parse_error(t)),
            _ => false,
        }
    }

    fn parse_this_type_predicate(&mut self, lhs: NodeId) -> NodeId {
        self.next_token();
        let pos = self.arena.node(lhs).pos as usize;
        let r#type = self.parse_type();
        self.finish_node_data(
            NodeData::TypePredicate(TypePredicateData {
                asserts_modifier: None,
                parameter_name: Some(lhs),
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn parse_this_type_node(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        self.finish_kind_only_node(SyntaxKind::ThisType, pos)
    }

    fn parse_jsdoc_all_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        self.finish_kind_only_node(SyntaxKind::JSDocAllType, pos)
    }

    fn parse_jsdoc_non_nullable_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        let r#type = self.parse_non_array_type();
        self.finish_node_data(
            NodeData::JSDocNonNullableType(JSDocNonNullableTypeData {
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn parse_jsdoc_unknown_or_nullable_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.next_token();
        if matches!(
            self.token(),
            SyntaxKind::CommaToken
                | SyntaxKind::CloseBraceToken
                | SyntaxKind::CloseParenToken
                | SyntaxKind::GreaterThanToken
                | SyntaxKind::EqualsToken
                | SyntaxKind::BarToken
        ) {
            self.finish_kind_only_node(SyntaxKind::JSDocUnknownType, pos)
        } else {
            let r#type = self.parse_type();
            self.finish_node_data(
                NodeData::JSDocNullableType(JSDocNullableTypeData {
                    r#type: Some(r#type),
                }),
                pos,
            )
        }
    }

    fn next_token_is_open_paren(&mut self) -> bool {
        self.next_token() == SyntaxKind::OpenParenToken
    }

    fn parse_jsdoc_function_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.try_parse(|parser| parser.next_token_is_open_paren()) {
            let parameters = self.parse_jsdoc_function_parameters();
            let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
            return self.finish_node_data(
                NodeData::JSDocFunctionType(JSDocFunctionTypeData {
                    parameters: Some(parameters),
                    r#type,
                }),
                pos,
            );
        }
        // `function` used as a plain type name.
        let type_name = self.parse_identifier_name(None);
        self.finish_node_data(
            NodeData::TypeReference(TypeReferenceData {
                type_name: Some(type_name),
                type_arguments: None,
            }),
            pos,
        )
    }

    /// tsc parseParameters with SignatureFlags.Type | SignatureFlags.JSDoc.
    fn parse_jsdoc_function_parameters(&mut self) -> crate::NodeArrayId {
        if !self.parse_expected(SyntaxKind::OpenParenToken, None) {
            return self.arena.missing_array(self.node_pos());
        }
        let saved_yield_context = self.in_yield_context();
        let saved_await_context = self.in_await_context();
        self.set_yield_context(false);
        self.set_await_context(false);
        let parameters = self.parse_delimited_list(
            ParsingContext::JSDocParameters,
            |parser| Some(parser.parse_jsdoc_parameter()),
            false,
        );
        self.set_yield_context(saved_yield_context);
        self.set_await_context(saved_await_context);
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        parameters
    }

    fn parse_jsdoc_parameter(&mut self) -> NodeId {
        let pos = self.node_pos();
        let mut name = None;
        if matches!(
            self.token(),
            SyntaxKind::ThisKeyword | SyntaxKind::NewKeyword
        ) {
            name = Some(self.parse_identifier_name(None));
            self.parse_expected(SyntaxKind::ColonToken, None);
        }
        let r#type = self.parse_jsdoc_type();
        self.finish_node_data(
            NodeData::Parameter(ParameterData {
                modifiers: None,
                dot_dot_dot_token: None,
                name,
                question_token: None,
                r#type: Some(r#type),
                initializer: None,
            }),
            pos,
        )
    }

    /// tsc parseJSDocType minus the JSDoc-comment-only namepath branch and
    /// the setSkipJsDocLeadingAsterisks scanner mode (both unreachable from
    /// type positions in source text).
    fn parse_jsdoc_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let has_dot_dot_dot = self.parse_optional(SyntaxKind::DotDotDotToken);
        let mut r#type = self.parse_type_or_type_predicate();
        if has_dot_dot_dot {
            r#type = self.finish_node_data(
                NodeData::JSDocVariadicType(JSDocVariadicTypeData {
                    r#type: Some(r#type),
                }),
                pos,
            );
        }
        if self.token() == SyntaxKind::EqualsToken {
            self.next_token();
            return self.finish_node_data(
                NodeData::JSDocOptionalType(JSDocOptionalTypeData {
                    r#type: Some(r#type),
                }),
                pos,
            );
        }
        r#type
    }

    fn parse_type_query(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::TypeOfKeyword, None);
        let expr_name = self.parse_entity_name(true, None);
        let type_arguments = if !self.scanner.has_preceding_line_break() {
            self.try_parse_type_arguments()
        } else {
            None
        };
        self.finish_node_data(
            NodeData::TypeQuery(TypeQueryData {
                expr_name: Some(expr_name),
                type_arguments,
            }),
            pos,
        )
    }

    fn parse_type_parameter(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers(false, true, false);
        let name = self.parse_identifier_or_missing();
        let mut constraint = None;
        let mut expression = None;
        if self.parse_optional(SyntaxKind::ExtendsKeyword) {
            // A type acts as the constraint; anything else is a bogus
            // expression kept for error reporting (tsc parseTypeParameter).
            if self.is_start_of_type(false) || !self.is_start_of_expression() {
                constraint = Some(self.parse_type());
            } else {
                expression = Some(self.parse_unary_expression_or_higher());
            }
        }
        let default_type = if self.parse_optional(SyntaxKind::EqualsToken) {
            Some(self.parse_type())
        } else {
            None
        };
        self.finish_node_data(
            NodeData::TypeParameter(TypeParameterData {
                modifiers,
                name: Some(name),
                constraint,
                r#default: default_type,
                expression,
            }),
            pos,
        )
    }

    fn parse_type_parameters(&mut self) -> Option<crate::NodeArrayId> {
        if self.token() == SyntaxKind::LessThanToken {
            Some(self.parse_bracketed_list(
                ParsingContext::TypeParameters,
                |parser| Some(parser.parse_type_parameter()),
                SyntaxKind::LessThanToken,
                SyntaxKind::GreaterThanToken,
            ))
        } else {
            None
        }
    }

    fn parse_type_member_semicolon(&mut self) {
        if self.parse_optional(SyntaxKind::CommaToken) {
            return;
        }
        self.parse_semicolon();
    }

    fn parse_signature_member(&mut self, kind: SyntaxKind) -> NodeId {
        let pos = self.node_pos();
        if kind == SyntaxKind::ConstructSignature {
            self.parse_expected(SyntaxKind::NewKeyword, None);
        }
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(false, false);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, true);
        self.parse_type_member_semicolon();
        let data = if kind == SyntaxKind::CallSignature {
            NodeData::CallSignature(CallSignatureData {
                r#type,
                type_parameters,
                parameters: Some(parameters),
            })
        } else {
            NodeData::ConstructSignature(ConstructSignatureData {
                r#type,
                type_parameters,
                parameters: Some(parameters),
            })
        };
        self.finish_node_data(data, pos)
    }

    fn is_index_signature(&mut self) -> bool {
        self.token() == SyntaxKind::OpenBracketToken
            && self.look_ahead(|parser| parser.is_unambiguously_index_signature())
    }

    fn is_unambiguously_index_signature(&mut self) -> bool {
        self.next_token();
        if matches!(
            self.token(),
            SyntaxKind::DotDotDotToken | SyntaxKind::CloseBracketToken
        ) {
            return true;
        }
        if self.is_modifier_kind(self.token()) {
            self.next_token();
            if self.is_identifier() {
                return true;
            }
        } else if !self.is_identifier() {
            return false;
        } else {
            self.next_token();
        }
        if matches!(
            self.token(),
            SyntaxKind::ColonToken | SyntaxKind::CommaToken
        ) {
            return true;
        }
        if self.token() != SyntaxKind::QuestionToken {
            return false;
        }
        self.next_token();
        matches!(
            self.token(),
            SyntaxKind::ColonToken | SyntaxKind::CommaToken | SyntaxKind::CloseBracketToken
        )
    }

    fn parse_index_signature_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let parameters = self.parse_bracketed_list(
            ParsingContext::Parameters,
            |parser| Some(parser.parse_parameter(false)),
            SyntaxKind::OpenBracketToken,
            SyntaxKind::CloseBracketToken,
        );
        let r#type = self.parse_type_annotation();
        self.parse_type_member_semicolon();
        self.finish_node_data(
            NodeData::IndexSignature(IndexSignatureData {
                modifiers,
                r#type,
                type_parameters: None,
                parameters: Some(parameters),
            }),
            pos,
        )
    }

    fn parse_property_or_method_signature(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        let name = self.parse_property_name();
        let question_token = self.parse_optional_token(SyntaxKind::QuestionToken);
        let data = if matches!(
            self.token(),
            SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken
        ) {
            let type_parameters = self.parse_type_parameters();
            let parameters = self.parse_parameters(false, false);
            let r#type = self.parse_return_type(SyntaxKind::ColonToken, true);
            NodeData::MethodSignature(MethodSignatureData {
                modifiers,
                name: Some(name),
                question_token,
                r#type,
                type_parameters,
                parameters: Some(parameters),
            })
        } else {
            let r#type = self.parse_type_annotation();
            // An initializer is a grammar error the checker reports; the
            // parser still consumes it (tsc parsePropertyOrMethodSignature).
            let initializer = if self.token() == SyntaxKind::EqualsToken {
                self.parse_initializer()
            } else {
                None
            };
            NodeData::PropertySignature(PropertySignatureData {
                modifiers,
                name: Some(name),
                question_token,
                r#type,
                initializer,
            })
        };
        self.parse_type_member_semicolon();
        self.finish_node_data(data, pos)
    }

    fn parse_type_member(&mut self) -> NodeId {
        if matches!(
            self.token(),
            SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken
        ) {
            return self.parse_signature_member(SyntaxKind::CallSignature);
        }
        if self.token() == SyntaxKind::NewKeyword
            && self.look_ahead(|parser| parser.next_token_is_open_paren_or_less_than())
        {
            return self.parse_signature_member(SyntaxKind::ConstructSignature);
        }
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers(false, false, false);
        if self.parse_contextual_modifier(SyntaxKind::GetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, SyntaxKind::GetAccessor, true);
        }
        if self.parse_contextual_modifier(SyntaxKind::SetKeyword) {
            return self.parse_accessor_declaration(pos, modifiers, SyntaxKind::SetAccessor, true);
        }
        if self.is_index_signature() {
            return self.parse_index_signature_declaration(pos, modifiers);
        }
        self.parse_property_or_method_signature(pos, modifiers)
    }

    fn next_token_is_open_paren_or_less_than(&mut self) -> bool {
        matches!(
            self.next_token(),
            SyntaxKind::OpenParenToken | SyntaxKind::LessThanToken
        )
    }

    /// tsc parseAccessorDeclaration; `in_type_member` mirrors
    /// SignatureFlags.Type (no body expected, type-member semicolons).
    fn parse_accessor_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        kind: SyntaxKind,
        in_type_member: bool,
    ) -> NodeId {
        let name = self.parse_property_name();
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(false, false);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(false, false, in_type_member, None);
        let data = if kind == SyntaxKind::GetAccessor {
            NodeData::GetAccessor(GetAccessorData {
                modifiers,
                name: Some(name),
                r#type,
                type_parameters,
                parameters: Some(parameters),
                body,
            })
        } else {
            NodeData::SetAccessor(SetAccessorData {
                modifiers,
                name: Some(name),
                r#type,
                type_parameters,
                parameters: Some(parameters),
                body,
            })
        };
        self.finish_node_data(data, pos)
    }

    /// tsc parseFunctionBlockOrSemicolon.
    fn parse_function_block_or_semicolon(
        &mut self,
        is_generator: bool,
        is_async: bool,
        in_type_member: bool,
        diagnostic_message: Option<&'static DiagnosticMessage>,
    ) -> Option<NodeId> {
        if self.token() != SyntaxKind::OpenBraceToken {
            if in_type_member {
                self.parse_type_member_semicolon();
                return None;
            }
            if self.can_parse_semicolon() {
                self.parse_semicolon();
                return None;
            }
        }
        Some(self.parse_function_block(is_generator, is_async, false, diagnostic_message))
    }

    fn parse_type_literal(&mut self) -> NodeId {
        let pos = self.node_pos();
        let members = self.parse_object_type_members();
        self.finish_node_data(
            NodeData::TypeLiteral(TypeLiteralData {
                members: Some(members),
            }),
            pos,
        )
    }

    fn parse_object_type_members(&mut self) -> crate::NodeArrayId {
        if self.parse_expected(SyntaxKind::OpenBraceToken, None) {
            let members = self.parse_list(ParsingContext::TypeMembers, |parser| {
                Some(parser.parse_type_member())
            });
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
            members
        } else {
            self.arena.missing_array(self.node_pos())
        }
    }

    fn is_start_of_mapped_type(&mut self) -> bool {
        self.next_token();
        if matches!(self.token(), SyntaxKind::PlusToken | SyntaxKind::MinusToken) {
            return self.next_token() == SyntaxKind::ReadonlyKeyword;
        }
        if self.token() == SyntaxKind::ReadonlyKeyword {
            self.next_token();
        }
        self.token() == SyntaxKind::OpenBracketToken
            && self.next_token_is_identifier()
            && self.next_token() == SyntaxKind::InKeyword
    }

    fn next_token_is_identifier(&mut self) -> bool {
        self.next_token();
        self.is_identifier()
    }

    fn parse_mapped_type_parameter(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_identifier_name(None);
        self.parse_expected(SyntaxKind::InKeyword, None);
        let r#type = self.parse_type();
        self.finish_node_data(
            NodeData::TypeParameter(TypeParameterData {
                modifiers: None,
                name: Some(name),
                constraint: Some(r#type),
                r#default: None,
                expression: None,
            }),
            pos,
        )
    }

    fn parse_mapped_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenBraceToken, None);
        let mut readonly_token = None;
        if matches!(
            self.token(),
            SyntaxKind::ReadonlyKeyword | SyntaxKind::PlusToken | SyntaxKind::MinusToken
        ) {
            let token = self.parse_token_node();
            readonly_token = Some(token);
            if self.arena.node(token).kind != SyntaxKind::ReadonlyKeyword {
                self.parse_expected(SyntaxKind::ReadonlyKeyword, None);
            }
        }
        self.parse_expected(SyntaxKind::OpenBracketToken, None);
        let type_parameter = self.parse_mapped_type_parameter();
        let name_type = if self.parse_optional(SyntaxKind::AsKeyword) {
            Some(self.parse_type())
        } else {
            None
        };
        self.parse_expected(SyntaxKind::CloseBracketToken, None);
        let mut question_token = None;
        if matches!(
            self.token(),
            SyntaxKind::QuestionToken | SyntaxKind::PlusToken | SyntaxKind::MinusToken
        ) {
            let token = self.parse_token_node();
            question_token = Some(token);
            if self.arena.node(token).kind != SyntaxKind::QuestionToken {
                self.parse_expected(SyntaxKind::QuestionToken, None);
            }
        }
        let r#type = self.parse_type_annotation();
        self.parse_semicolon();
        let members = self.parse_list(ParsingContext::TypeMembers, |parser| {
            Some(parser.parse_type_member())
        });
        self.parse_expected(SyntaxKind::CloseBraceToken, None);
        self.finish_node_data(
            NodeData::MappedType(MappedTypeData {
                readonly_token,
                type_parameter: Some(type_parameter),
                r#type,
                name_type,
                question_token,
                members: Some(members),
            }),
            pos,
        )
    }

    fn parse_tuple_element_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        if self.parse_optional(SyntaxKind::DotDotDotToken) {
            let r#type = self.parse_type();
            return self.finish_node_data(
                NodeData::RestType(RestTypeData {
                    r#type: Some(r#type),
                }),
                pos,
            );
        }
        let r#type = self.parse_type();
        // Postfix `T?` (JSDocNullableType whose pos matches its inner type)
        // becomes an optional tuple element with the same range and flags.
        let rewrap = match &self.arena.node(r#type).data {
            NodeData::JSDocNullableType(data) => data
                .r#type
                .filter(|inner| self.arena.node(r#type).pos == self.arena.node(*inner).pos),
            _ => None,
        };
        if let Some(inner) = rewrap {
            let node = self.arena.node(r#type);
            let (node_pos, node_end, node_flags) =
                (node.pos as usize, node.end as usize, node.flags);
            return self.arena.alloc_node(
                NodeData::OptionalType(OptionalTypeData {
                    r#type: Some(inner),
                }),
                node_pos,
                node_end,
                NodeFlags::from_bits(node_flags),
            );
        }
        r#type
    }

    fn is_next_token_colon_or_question_colon(&mut self) -> bool {
        self.next_token() == SyntaxKind::ColonToken
            || self.token() == SyntaxKind::QuestionToken
                && self.next_token() == SyntaxKind::ColonToken
    }

    fn is_tuple_element_name(&mut self) -> bool {
        if self.token() == SyntaxKind::DotDotDotToken {
            return token_is_identifier_or_keyword(self.next_token())
                && self.is_next_token_colon_or_question_colon();
        }
        token_is_identifier_or_keyword(self.token()) && self.is_next_token_colon_or_question_colon()
    }

    fn parse_tuple_element_name_or_tuple_element_type(&mut self) -> NodeId {
        if self.look_ahead(|parser| parser.is_tuple_element_name()) {
            let pos = self.node_pos();
            let dot_dot_dot_token = self.parse_optional_token(SyntaxKind::DotDotDotToken);
            let name = self.parse_identifier_name(None);
            let question_token = self.parse_optional_token(SyntaxKind::QuestionToken);
            self.parse_expected(SyntaxKind::ColonToken, None);
            let r#type = self.parse_tuple_element_type();
            return self.finish_node_data(
                NodeData::NamedTupleMember(NamedTupleMemberData {
                    dot_dot_dot_token,
                    name: Some(name),
                    question_token,
                    r#type: Some(r#type),
                }),
                pos,
            );
        }
        self.parse_tuple_element_type()
    }

    fn parse_tuple_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let elements = self.parse_bracketed_list(
            ParsingContext::TupleElementTypes,
            |parser| Some(parser.parse_tuple_element_name_or_tuple_element_type()),
            SyntaxKind::OpenBracketToken,
            SyntaxKind::CloseBracketToken,
        );
        self.finish_node_data(
            NodeData::TupleType(TupleTypeData {
                elements: Some(elements),
            }),
            pos,
        )
    }

    fn parse_parenthesized_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let r#type = self.parse_type();
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        self.finish_node_data(
            NodeData::ParenthesizedType(ParenthesizedTypeData {
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn parse_modifiers_for_constructor_type(&mut self) -> Option<crate::NodeArrayId> {
        if self.token() != SyntaxKind::AbstractKeyword {
            return None;
        }
        let pos = self.node_pos();
        let modifier = self.parse_token_node();
        let end = self.arena.node(modifier).end as usize;
        Some(self.arena.alloc_array(vec![modifier], pos, end, false))
    }

    fn parse_function_or_constructor_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let modifiers = self.parse_modifiers_for_constructor_type();
        let is_constructor_type = self.parse_optional(SyntaxKind::NewKeyword);
        debug_assert!(
            modifiers.is_none() || is_constructor_type,
            "Per isStartOfFunctionOrConstructorType, a function type cannot have modifiers."
        );
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(false, false);
        let r#type = self.parse_return_type(SyntaxKind::EqualsGreaterThanToken, false);
        let data = if is_constructor_type {
            NodeData::ConstructorType(ConstructorTypeData {
                modifiers,
                r#type,
                type_parameters,
                parameters: Some(parameters),
            })
        } else {
            NodeData::FunctionType(FunctionTypeData {
                modifiers,
                r#type,
                type_parameters,
                parameters: Some(parameters),
            })
        };
        self.finish_node_data(data, pos)
    }

    fn parse_keyword_and_no_dot(&mut self) -> Option<NodeId> {
        let node = self.parse_token_node();
        if self.token() == SyntaxKind::DotToken {
            None
        } else {
            Some(node)
        }
    }

    fn parse_literal_type_node(&mut self, negative: bool) -> NodeId {
        let pos = self.node_pos();
        if negative {
            self.next_token();
        }
        let mut expression = match self.token() {
            SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword | SyntaxKind::NullKeyword => {
                self.parse_token_node()
            }
            SyntaxKind::StringLiteral => self.parse_string_literal(),
            SyntaxKind::NumericLiteral => self.parse_numeric_literal(),
            SyntaxKind::BigIntLiteral => self.parse_big_int_literal(),
            SyntaxKind::NoSubstitutionTemplateLiteral => {
                self.parse_no_substitution_template_literal()
            }
            // parseLiteralTypeNode is only entered on literal starts.
            _ => self.parse_token_node(),
        };
        if negative {
            expression = self.finish_node_data(
                NodeData::PrefixUnaryExpression(PrefixUnaryExpressionData {
                    operand: Some(expression),
                }),
                pos,
            );
        }
        self.finish_node_data(
            NodeData::LiteralType(LiteralTypeData {
                literal: Some(expression),
            }),
            pos,
        )
    }

    fn is_start_of_type_of_import_type(&mut self) -> bool {
        self.next_token();
        self.token() == SyntaxKind::ImportKeyword
    }

    /// tsc parseImportType. Source flags (PossiblyContainsDynamicImport) and
    /// the isTypeOf marker have no home in the current node data yet.
    fn parse_import_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let _is_type_of = self.parse_optional(SyntaxKind::TypeOfKeyword);
        self.parse_expected(SyntaxKind::ImportKeyword, None);
        self.parse_expected(SyntaxKind::OpenParenToken, None);
        let argument = self.parse_type();
        let mut attributes = None;
        if self.parse_optional(SyntaxKind::CommaToken) {
            self.parse_expected(SyntaxKind::OpenBraceToken, None);
            if matches!(
                self.token(),
                SyntaxKind::WithKeyword | SyntaxKind::AssertKeyword
            ) {
                self.next_token();
            } else {
                self.parse_error_at_current_token(
                    &gen::_0_expected,
                    &[&token_to_string(SyntaxKind::WithKeyword)],
                );
            }
            self.parse_expected(SyntaxKind::ColonToken, None);
            attributes = Some(self.parse_import_attributes(self.token(), true));
            self.parse_optional(SyntaxKind::CommaToken);
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
        }
        self.parse_expected(SyntaxKind::CloseParenToken, None);
        let qualifier = if self.parse_optional(SyntaxKind::DotToken) {
            Some(self.parse_entity_name_of_type_reference())
        } else {
            None
        };
        let type_arguments = self.parse_type_arguments_of_type_reference();
        self.finish_node_data(
            NodeData::ImportType(ImportTypeData {
                argument: Some(argument),
                attributes,
                qualifier,
                type_arguments,
            }),
            pos,
        )
    }

    /// tsc parseImportAttributes.
    fn parse_import_attributes(&mut self, keyword: SyntaxKind, skip_keyword: bool) -> NodeId {
        let pos = self.node_pos();
        if !skip_keyword {
            self.parse_expected(keyword, None);
        }
        let elements = if self.parse_expected(SyntaxKind::OpenBraceToken, None) {
            let elements = self.parse_delimited_list(
                ParsingContext::ImportAttributes,
                |parser| Some(parser.parse_import_attribute()),
                true,
            );
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
            elements
        } else {
            self.arena.empty_array(self.node_pos())
        };
        self.finish_node_data(
            NodeData::ImportAttributes(ImportAttributesData {
                elements: Some(elements),
            }),
            pos,
        )
    }

    fn parse_import_attribute(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = if token_is_identifier_or_keyword(self.token()) {
            self.parse_identifier_name(None)
        } else {
            self.parse_string_literal()
        };
        self.parse_expected(SyntaxKind::ColonToken, None);
        let value = self.parse_assignment_expression_or_higher(true);
        self.finish_node_data(
            NodeData::ImportAttribute(ImportAttributeData {
                name: Some(name),
                value: Some(value),
            }),
            pos,
        )
    }

    fn parse_non_array_type(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::AnyKeyword
            | SyntaxKind::UnknownKeyword
            | SyntaxKind::StringKeyword
            | SyntaxKind::NumberKeyword
            | SyntaxKind::BigIntKeyword
            | SyntaxKind::SymbolKeyword
            | SyntaxKind::BooleanKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::NeverKeyword
            | SyntaxKind::ObjectKeyword => {
                match self.try_parse(|parser| parser.parse_keyword_and_no_dot()) {
                    Some(node) => node,
                    None => self.parse_type_reference(),
                }
            }
            SyntaxKind::AsteriskEqualsToken => {
                // `*=` in a type position is a JSDoc all-type followed by `=`.
                self.scanner.re_scan_asterisk_equals_token();
                self.parse_jsdoc_all_type()
            }
            SyntaxKind::AsteriskToken => self.parse_jsdoc_all_type(),
            SyntaxKind::QuestionQuestionToken => {
                // `??` splits into `?` heading a JSDoc unknown/nullable type.
                self.scanner.re_scan_question_token();
                self.parse_jsdoc_unknown_or_nullable_type()
            }
            SyntaxKind::QuestionToken => self.parse_jsdoc_unknown_or_nullable_type(),
            SyntaxKind::FunctionKeyword => self.parse_jsdoc_function_type(),
            SyntaxKind::ExclamationToken => self.parse_jsdoc_non_nullable_type(),
            SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword => self.parse_literal_type_node(false),
            SyntaxKind::MinusToken => {
                if self.look_ahead(|parser| parser.next_token_is_numeric_or_big_int_literal()) {
                    self.parse_literal_type_node(true)
                } else {
                    self.parse_type_reference()
                }
            }
            SyntaxKind::VoidKeyword => self.parse_token_node(),
            SyntaxKind::ThisKeyword => {
                let this_keyword = self.parse_this_type_node();
                if self.token() == SyntaxKind::IsKeyword && !self.scanner.has_preceding_line_break()
                {
                    self.parse_this_type_predicate(this_keyword)
                } else {
                    this_keyword
                }
            }
            SyntaxKind::TypeOfKeyword => {
                if self.look_ahead(|parser| parser.is_start_of_type_of_import_type()) {
                    self.parse_import_type()
                } else {
                    self.parse_type_query()
                }
            }
            SyntaxKind::OpenBraceToken => {
                if self.look_ahead(|parser| parser.is_start_of_mapped_type()) {
                    self.parse_mapped_type()
                } else {
                    self.parse_type_literal()
                }
            }
            SyntaxKind::OpenBracketToken => self.parse_tuple_type(),
            SyntaxKind::OpenParenToken => self.parse_parenthesized_type(),
            SyntaxKind::ImportKeyword => self.parse_import_type(),
            SyntaxKind::AssertsKeyword => {
                if self
                    .look_ahead(|parser| parser.next_token_is_identifier_or_keyword_on_same_line())
                {
                    self.parse_asserts_type_predicate()
                } else {
                    self.parse_type_reference()
                }
            }
            SyntaxKind::TemplateHead => self.parse_template_type(),
            _ => self.parse_type_reference(),
        }
    }

    fn parse_postfix_type_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let mut r#type = self.parse_non_array_type();
        while !self.scanner.has_preceding_line_break() {
            match self.token() {
                SyntaxKind::ExclamationToken => {
                    self.next_token();
                    r#type = self.finish_node_data(
                        NodeData::JSDocNonNullableType(JSDocNonNullableTypeData {
                            r#type: Some(r#type),
                        }),
                        pos,
                    );
                }
                SyntaxKind::QuestionToken => {
                    // A `?` that begins a conditional type's branches is not
                    // a postfix nullable marker.
                    if self.look_ahead(|parser| parser.next_token_is_start_of_type()) {
                        return r#type;
                    }
                    self.next_token();
                    r#type = self.finish_node_data(
                        NodeData::JSDocNullableType(JSDocNullableTypeData {
                            r#type: Some(r#type),
                        }),
                        pos,
                    );
                }
                SyntaxKind::OpenBracketToken => {
                    self.parse_expected(SyntaxKind::OpenBracketToken, None);
                    if self.is_start_of_type(false) {
                        let index_type = self.parse_type();
                        self.parse_expected(SyntaxKind::CloseBracketToken, None);
                        r#type = self.finish_node_data(
                            NodeData::IndexedAccessType(IndexedAccessTypeData {
                                object_type: Some(r#type),
                                index_type: Some(index_type),
                            }),
                            pos,
                        );
                    } else {
                        self.parse_expected(SyntaxKind::CloseBracketToken, None);
                        r#type = self.finish_node_data(
                            NodeData::ArrayType(ArrayTypeData {
                                element_type: Some(r#type),
                            }),
                            pos,
                        );
                    }
                }
                _ => return r#type,
            }
        }
        r#type
    }

    fn next_token_is_start_of_type(&mut self) -> bool {
        self.next_token();
        self.is_start_of_type(false)
    }

    /// The operator token kind is recoverable from the source range; the
    /// node data only stores the operand (same convention as
    /// PrefixUnaryExpression).
    fn parse_type_operator(&mut self, operator: SyntaxKind) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(operator, None);
        let r#type = self.parse_type_operator_or_higher();
        self.finish_node_data(
            NodeData::TypeOperator(TypeOperatorData {
                r#type: Some(r#type),
            }),
            pos,
        )
    }

    fn try_parse_constraint_of_infer_type(&mut self) -> Option<NodeId> {
        if self.parse_optional(SyntaxKind::ExtendsKeyword) {
            let constraint = self.disallow_conditional_types_and(|parser| parser.parse_type());
            if self.in_disallow_conditional_types_context()
                || self.token() != SyntaxKind::QuestionToken
            {
                return Some(constraint);
            }
        }
        None
    }

    fn parse_type_parameter_of_infer_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        let name = self.parse_identifier_or_missing();
        let constraint = self.try_parse(|parser| parser.try_parse_constraint_of_infer_type());
        self.finish_node_data(
            NodeData::TypeParameter(TypeParameterData {
                modifiers: None,
                name: Some(name),
                constraint,
                r#default: None,
                expression: None,
            }),
            pos,
        )
    }

    fn parse_infer_type(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::InferKeyword, None);
        let type_parameter = self.parse_type_parameter_of_infer_type();
        self.finish_node_data(
            NodeData::InferType(InferTypeData {
                type_parameter: Some(type_parameter),
            }),
            pos,
        )
    }

    fn parse_type_operator_or_higher(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::KeyOfKeyword | SyntaxKind::UniqueKeyword | SyntaxKind::ReadonlyKeyword => {
                self.parse_type_operator(self.token())
            }
            SyntaxKind::InferKeyword => self.parse_infer_type(),
            _ => self.allow_conditional_types_and(|parser| parser.parse_postfix_type_or_higher()),
        }
    }

    fn parse_function_or_constructor_type_to_error(
        &mut self,
        is_in_union_type: bool,
    ) -> Option<NodeId> {
        if !self.is_start_of_function_type_or_constructor_type() {
            return None;
        }
        let r#type = self.parse_function_or_constructor_type();
        let message: &'static DiagnosticMessage = if self.arena.node(r#type).kind
            == SyntaxKind::FunctionType
        {
            if is_in_union_type {
                &gen::Function_type_notation_must_be_parenthesized_when_used_in_a_union_type
            } else {
                &gen::Function_type_notation_must_be_parenthesized_when_used_in_an_intersection_type
            }
        } else if is_in_union_type {
            &gen::Constructor_type_notation_must_be_parenthesized_when_used_in_a_union_type
        } else {
            &gen::Constructor_type_notation_must_be_parenthesized_when_used_in_an_intersection_type
        };
        let node = self.arena.node(r#type);
        let (start, length) = (node.pos as usize, (node.end - node.pos) as usize);
        self.parse_error_at_position(start, length, message, &[]);
        Some(r#type)
    }

    fn parse_union_or_intersection_type(
        &mut self,
        operator: SyntaxKind,
        parse_constituent_type: fn(&mut Self) -> NodeId,
        create_type_node: fn(crate::NodeArrayId) -> NodeData,
    ) -> NodeId {
        let pos = self.node_pos();
        let is_union_type = operator == SyntaxKind::BarToken;
        let has_leading_operator = self.parse_optional(operator);
        let mut r#type = if has_leading_operator {
            self.parse_function_or_constructor_type_to_error(is_union_type)
                .unwrap_or_else(|| parse_constituent_type(self))
        } else {
            parse_constituent_type(self)
        };
        if self.token() == operator || has_leading_operator {
            let mut types = vec![r#type];
            while self.parse_optional(operator) {
                let next = self
                    .parse_function_or_constructor_type_to_error(is_union_type)
                    .unwrap_or_else(|| parse_constituent_type(self));
                types.push(next);
            }
            let array = self.arena.alloc_array(types, pos, self.node_pos(), false);
            r#type = self.finish_node_data(create_type_node(array), pos);
        }
        r#type
    }

    fn parse_intersection_type_or_higher(&mut self) -> NodeId {
        self.parse_union_or_intersection_type(
            SyntaxKind::AmpersandToken,
            Self::parse_type_operator_or_higher,
            |types| NodeData::IntersectionType(IntersectionTypeData { types: Some(types) }),
        )
    }

    fn parse_union_type_or_higher(&mut self) -> NodeId {
        self.parse_union_or_intersection_type(
            SyntaxKind::BarToken,
            Self::parse_intersection_type_or_higher,
            |types| NodeData::UnionType(UnionTypeData { types: Some(types) }),
        )
    }

    fn next_token_is_new_keyword(&mut self) -> bool {
        self.next_token();
        self.token() == SyntaxKind::NewKeyword
    }

    fn is_start_of_function_type_or_constructor_type(&mut self) -> bool {
        if self.token() == SyntaxKind::LessThanToken {
            return true;
        }
        if self.token() == SyntaxKind::OpenParenToken
            && self.look_ahead(|parser| parser.is_unambiguously_start_of_function_type())
        {
            return true;
        }
        self.token() == SyntaxKind::NewKeyword
            || self.token() == SyntaxKind::AbstractKeyword
                && self.look_ahead(|parser| parser.next_token_is_new_keyword())
    }

    fn skip_parameter_start(&mut self) -> bool {
        if self.is_modifier_kind(self.token()) {
            self.parse_modifiers(false, false, false);
        }
        if self.is_identifier() || self.token() == SyntaxKind::ThisKeyword {
            self.next_token();
            return true;
        }
        if matches!(
            self.token(),
            SyntaxKind::OpenBracketToken | SyntaxKind::OpenBraceToken
        ) {
            let previous_error_count = self.parse_diagnostics.len();
            self.parse_identifier_or_pattern();
            return previous_error_count == self.parse_diagnostics.len();
        }
        false
    }

    fn is_unambiguously_start_of_function_type(&mut self) -> bool {
        self.next_token();
        if matches!(
            self.token(),
            SyntaxKind::CloseParenToken | SyntaxKind::DotDotDotToken
        ) {
            return true;
        }
        if self.skip_parameter_start() {
            if matches!(
                self.token(),
                SyntaxKind::ColonToken
                    | SyntaxKind::CommaToken
                    | SyntaxKind::QuestionToken
                    | SyntaxKind::EqualsToken
            ) {
                return true;
            }
            if self.token() == SyntaxKind::CloseParenToken {
                self.next_token();
                if self.token() == SyntaxKind::EqualsGreaterThanToken {
                    return true;
                }
            }
        }
        false
    }

    fn parse_type_or_type_predicate(&mut self) -> NodeId {
        let pos = self.node_pos();
        let type_predicate_variable = if self.is_identifier() {
            self.try_parse(|parser| parser.parse_type_predicate_prefix())
        } else {
            None
        };
        let r#type = self.parse_type();
        if let Some(parameter_name) = type_predicate_variable {
            self.finish_node_data(
                NodeData::TypePredicate(TypePredicateData {
                    asserts_modifier: None,
                    parameter_name: Some(parameter_name),
                    r#type: Some(r#type),
                }),
                pos,
            )
        } else {
            r#type
        }
    }

    fn parse_type_predicate_prefix(&mut self) -> Option<NodeId> {
        let id = self.parse_identifier();
        if self.token() == SyntaxKind::IsKeyword && !self.scanner.has_preceding_line_break() {
            self.next_token();
            Some(id)
        } else {
            None
        }
    }

    fn parse_asserts_type_predicate(&mut self) -> NodeId {
        let pos = self.node_pos();
        let asserts_modifier = self.parse_expected_token(SyntaxKind::AssertsKeyword, None);
        let parameter_name = if self.token() == SyntaxKind::ThisKeyword {
            self.parse_this_type_node()
        } else {
            self.parse_identifier_or_missing()
        };
        let r#type = if self.parse_optional(SyntaxKind::IsKeyword) {
            Some(self.parse_type())
        } else {
            None
        };
        self.finish_node_data(
            NodeData::TypePredicate(TypePredicateData {
                asserts_modifier: Some(asserts_modifier),
                parameter_name: Some(parameter_name),
                r#type,
            }),
            pos,
        )
    }

    fn parse_type(&mut self) -> NodeId {
        if self.context_flags.bits() & NodeFlags::TYPE_EXCLUDES_FLAGS.bits() != 0 {
            return self.do_in_context(NodeFlags::NONE, NodeFlags::TYPE_EXCLUDES_FLAGS, |parser| {
                parser.parse_type()
            });
        }
        if self.is_start_of_function_type_or_constructor_type() {
            return self.parse_function_or_constructor_type();
        }
        let pos = self.node_pos();
        let r#type = self.parse_union_type_or_higher();
        if !self.in_disallow_conditional_types_context()
            && !self.scanner.has_preceding_line_break()
            && self.parse_optional(SyntaxKind::ExtendsKeyword)
        {
            // The extends type is never itself conditional; its branches are.
            let extends_type = self.disallow_conditional_types_and(|parser| parser.parse_type());
            self.parse_expected(SyntaxKind::QuestionToken, None);
            let true_type = self.allow_conditional_types_and(|parser| parser.parse_type());
            self.parse_expected(SyntaxKind::ColonToken, None);
            let false_type = self.allow_conditional_types_and(|parser| parser.parse_type());
            return self.finish_node_data(
                NodeData::ConditionalType(ConditionalTypeData {
                    check_type: Some(r#type),
                    extends_type: Some(extends_type),
                    true_type: Some(true_type),
                    false_type: Some(false_type),
                }),
                pos,
            );
        }
        r#type
    }

    fn allow_conditional_types_and<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.do_in_context(
            NodeFlags::NONE,
            NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT,
            f,
        )
    }

    fn disallow_conditional_types_and<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.do_in_context(
            NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT,
            NodeFlags::NONE,
            f,
        )
    }

    fn in_disallow_conditional_types_context(&self) -> bool {
        self.context_flags.bits() & NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT.bits() != 0
    }

    fn parse_initializer(&mut self) -> Option<NodeId> {
        if self.parse_optional(SyntaxKind::EqualsToken) {
            Some(self.parse_assignment_expression_or_higher(true))
        } else {
            None
        }
    }

    fn parse_semicolon(&mut self) {
        if !self.try_parse_semicolon() {
            self.parse_error_at_current_token(&gen::_0_expected, &[";"]);
        }
    }

    /// tsc parseErrorForMissingSemicolonAfter.
    fn parse_error_for_missing_semicolon_after(&mut self, node: NodeId) {
        if self.arena.node(node).kind == SyntaxKind::TaggedTemplateExpression {
            let template = match &self.arena.node(node).data {
                NodeData::TaggedTemplateExpression(data) => data.template,
                _ => None,
            };
            if let Some(template) = template {
                let start = crate::scanner::skip_trivia(
                    self.scanner.text(),
                    self.arena.node(template).pos as usize,
                );
                let end = self.arena.node(template).end as usize;
                self.parse_error_at_position(
                    start,
                    end - start,
                    &gen::Module_declaration_names_may_only_use_or_quoted_strings,
                    &[],
                );
            }
            return;
        }
        let expression_text = match &self.arena.node(node).data {
            NodeData::Identifier(data) => Some(data.text.clone()),
            _ => None,
        };
        let Some(expression_text) = expression_text.filter(|text| is_identifier_text(text)) else {
            self.parse_error_at_current_token(
                &gen::_0_expected,
                &[&token_to_string(SyntaxKind::SemicolonToken)],
            );
            return;
        };
        let pos =
            crate::scanner::skip_trivia(self.scanner.text(), self.arena.node(node).pos as usize);
        let node_end = self.arena.node(node).end as usize;
        match expression_text.as_str() {
            "const" | "let" | "var" => {
                self.parse_error_at_position(
                    pos,
                    node_end - pos,
                    &gen::Variable_declaration_not_allowed_at_this_location,
                    &[],
                );
                return;
            }
            "declare" => {
                // A `declare` heading an unsupported construct recovers
                // silently; the checker owns the follow-up errors.
                return;
            }
            "interface" => {
                self.parse_error_for_invalid_name(
                    &gen::Interface_name_cannot_be_0,
                    &gen::Interface_must_be_given_a_name,
                    SyntaxKind::OpenBraceToken,
                );
                return;
            }
            "is" => {
                let token_start = self.scanner.token_start();
                self.parse_error_at_position(
                    pos,
                    token_start - pos,
                    &gen::A_type_predicate_is_only_allowed_in_return_type_position_for_functions_and_methods,
                    &[],
                );
                return;
            }
            "module" | "namespace" => {
                self.parse_error_for_invalid_name(
                    &gen::Namespace_name_cannot_be_0,
                    &gen::Namespace_must_be_given_a_name,
                    SyntaxKind::OpenBraceToken,
                );
                return;
            }
            "type" => {
                self.parse_error_for_invalid_name(
                    &gen::Type_alias_name_cannot_be_0,
                    &gen::Type_alias_must_be_given_a_name,
                    SyntaxKind::EqualsToken,
                );
                return;
            }
            _ => {}
        }
        let suggestion = get_spelling_suggestion(&expression_text, VIABLE_KEYWORD_SUGGESTIONS)
            .map(str::to_owned)
            .or_else(|| get_space_suggestion(&expression_text));
        if let Some(suggestion) = suggestion {
            self.parse_error_at_position(
                pos,
                node_end - pos,
                &gen::Unknown_keyword_or_identifier_Did_you_mean_0,
                &[&suggestion],
            );
            return;
        }
        if self.token() == SyntaxKind::Unknown {
            return;
        }
        self.parse_error_at_position(
            pos,
            node_end - pos,
            &gen::Unexpected_keyword_or_identifier,
            &[],
        );
    }

    fn parse_error_for_invalid_name(
        &mut self,
        name_diagnostic: &'static DiagnosticMessage,
        blank_diagnostic: &'static DiagnosticMessage,
        token_if_blank_name: SyntaxKind,
    ) {
        if self.token() == token_if_blank_name {
            self.parse_error_at_current_token(blank_diagnostic, &[]);
        } else {
            let name = self.current_token_text();
            self.parse_error_at_current_token(name_diagnostic, &[&name]);
        }
    }

    /// tsc parseSemicolonAfterPropertyName.
    fn parse_semicolon_after_property_name(
        &mut self,
        name: NodeId,
        r#type: Option<NodeId>,
        initializer: Option<NodeId>,
    ) {
        if self.token() == SyntaxKind::AtToken && !self.scanner.has_preceding_line_break() {
            self.parse_error_at_current_token(
                &gen::Decorators_must_precede_the_name_and_all_keywords_of_property_declarations,
                &[],
            );
            return;
        }
        if self.token() == SyntaxKind::OpenParenToken {
            self.parse_error_at_current_token(
                &gen::Cannot_start_a_function_call_in_a_type_annotation,
                &[],
            );
            self.next_token();
            return;
        }
        if r#type.is_some() && !self.can_parse_semicolon() {
            if initializer.is_some() {
                self.parse_error_at_current_token(
                    &gen::_0_expected,
                    &[&token_to_string(SyntaxKind::SemicolonToken)],
                );
            } else {
                self.parse_error_at_current_token(&gen::Expected_for_property_initializer, &[]);
            }
            return;
        }
        if self.try_parse_semicolon() {
            return;
        }
        if initializer.is_some() {
            self.parse_error_at_current_token(
                &gen::_0_expected,
                &[&token_to_string(SyntaxKind::SemicolonToken)],
            );
            return;
        }
        self.parse_error_for_missing_semicolon_after(name);
    }

    fn try_parse_semicolon(&mut self) -> bool {
        if self.parse_optional(SyntaxKind::SemicolonToken) {
            true
        } else {
            self.can_parse_semicolon()
        }
    }

    /// tsc parseExpectedToken.
    fn parse_expected_token(
        &mut self,
        kind: SyntaxKind,
        message: Option<&'static DiagnosticMessage>,
    ) -> NodeId {
        match self.parse_optional_token(kind) {
            Some(token) => token,
            None => {
                let message = message.unwrap_or(&gen::_0_expected);
                self.create_missing_node(kind, false, Some(message), &[&token_to_string(kind)])
            }
        }
    }

    fn node_is_present(&self, id: NodeId) -> bool {
        let node = self.arena.node(id);
        node.pos != node.end || node.kind == SyntaxKind::EndOfFileToken
    }

    fn parse_optional_token(&mut self, kind: SyntaxKind) -> Option<NodeId> {
        if self.token() == kind {
            Some(self.parse_token_node())
        } else {
            None
        }
    }

    fn allow_in<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.do_in_context(NodeFlags::NONE, NodeFlags::DISALLOW_IN_CONTEXT, f)
    }

    fn disallow_in<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.do_in_context(NodeFlags::DISALLOW_IN_CONTEXT, NodeFlags::NONE, f)
    }

    fn in_disallow_in_context(&self) -> bool {
        self.context_flags.contains(NodeFlags::DISALLOW_IN_CONTEXT)
    }

    fn in_decorator_context(&self) -> bool {
        self.context_flags.contains(NodeFlags::DECORATOR_CONTEXT)
    }

    fn set_context_flag(&mut self, value: bool, flag: NodeFlags) {
        self.context_flags = if value {
            self.context_flags | flag
        } else {
            NodeFlags::from_bits(self.context_flags.bits() & !flag.bits())
        };
    }

    fn set_decorator_context(&mut self, value: bool) {
        self.set_context_flag(value, NodeFlags::DECORATOR_CONTEXT);
    }

    fn set_yield_context(&mut self, value: bool) {
        self.set_context_flag(value, NodeFlags::YIELD_CONTEXT);
    }

    fn set_await_context(&mut self, value: bool) {
        self.set_context_flag(value, NodeFlags::AWAIT_CONTEXT);
    }

    fn in_await_context(&self) -> bool {
        self.context_flags.contains(NodeFlags::AWAIT_CONTEXT)
    }

    fn in_yield_context(&self) -> bool {
        self.context_flags.contains(NodeFlags::YIELD_CONTEXT)
    }

    /// The tsc parseForOrForInOrForOfStatement initializer condition: keyword
    /// declarations, `using` with the disallow-of lookahead, `await using`.
    fn is_variable_statement_start(&mut self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::VarKeyword | SyntaxKind::LetKeyword | SyntaxKind::ConstKeyword
        ) || (self.token() == SyntaxKind::UsingKeyword
            && self.look_ahead(|parser| {
                parser.next_token_is_binding_identifier_or_start_of_destructuring_on_same_line(true)
            }))
            || (self.token() == SyntaxKind::AwaitKeyword
                && self.look_ahead(|parser| {
                    parser
                        .next_token_is_using_keyword_then_binding_identifier_or_start_of_object_destructuring_on_same_line(false)
                }))
    }

    fn is_let_declaration(&mut self) -> bool {
        self.look_ahead(|parser| {
            parser.next_token();
            parser.is_binding_identifier_or_private_identifier_or_pattern()
        })
    }

    /// tsc nextTokenIsBindingIdentifierOrStartOfDestructuringOnSameLine.
    fn next_token_is_binding_identifier_or_start_of_destructuring_on_same_line(
        &mut self,
        disallow_of: bool,
    ) -> bool {
        self.next_token();
        if disallow_of && self.token() == SyntaxKind::OfKeyword {
            return self
                .look_ahead(|parser| parser.next_token_is_equals_or_semicolon_or_colon_token());
        }
        (self.is_binding_identifier() || self.token() == SyntaxKind::OpenBraceToken)
            && !self.scanner.has_preceding_line_break()
    }

    fn next_token_is_equals_or_semicolon_or_colon_token(&mut self) -> bool {
        self.next_token();
        matches!(
            self.token(),
            SyntaxKind::EqualsToken | SyntaxKind::SemicolonToken | SyntaxKind::ColonToken
        )
    }

    /// tsc nextTokenIsUsingKeywordThenBindingIdentifierOrStartOfObjectDestructuringOnSameLine.
    fn next_token_is_using_keyword_then_binding_identifier_or_start_of_object_destructuring_on_same_line(
        &mut self,
        disallow_of: bool,
    ) -> bool {
        if self.next_token() == SyntaxKind::UsingKeyword {
            return self.next_token_is_binding_identifier_or_start_of_destructuring_on_same_line(
                disallow_of,
            );
        }
        false
    }

    /// tsc isUsingDeclaration; callers guard on token() == UsingKeyword.
    fn is_using_declaration(&mut self) -> bool {
        self.look_ahead(|parser| {
            parser.next_token_is_binding_identifier_or_start_of_destructuring_on_same_line(false)
        })
    }

    /// tsc isAwaitUsingDeclaration; callers guard on token() == AwaitKeyword.
    fn is_await_using_declaration(&mut self) -> bool {
        self.look_ahead(|parser| {
            parser
                .next_token_is_using_keyword_then_binding_identifier_or_start_of_object_destructuring_on_same_line(false)
        })
    }

    fn is_start_of_declaration(&mut self) -> bool {
        self.look_ahead(|parser| parser.is_declaration())
    }

    fn is_declaration(&mut self) -> bool {
        loop {
            match self.token() {
                SyntaxKind::VarKeyword
                | SyntaxKind::LetKeyword
                | SyntaxKind::ConstKeyword
                | SyntaxKind::FunctionKeyword
                | SyntaxKind::ClassKeyword
                | SyntaxKind::EnumKeyword => return true,
                SyntaxKind::UsingKeyword => return self.is_using_declaration(),
                SyntaxKind::AwaitKeyword => return self.is_await_using_declaration(),
                // 'declare'/'module'/'namespace'/'interface'/'type' are legal
                // identifiers; only a same-line identifier after them commits
                // to the declaration reading (tsc isDeclaration).
                SyntaxKind::InterfaceKeyword
                | SyntaxKind::TypeKeyword
                | SyntaxKind::DeferKeyword => {
                    return self.next_token_is_identifier_on_same_line();
                }
                SyntaxKind::ModuleKeyword | SyntaxKind::NamespaceKeyword => {
                    return self.next_token_is_identifier_or_string_literal_on_same_line();
                }
                SyntaxKind::AbstractKeyword
                | SyntaxKind::AccessorKeyword
                | SyntaxKind::AsyncKeyword
                | SyntaxKind::DeclareKeyword
                | SyntaxKind::PrivateKeyword
                | SyntaxKind::ProtectedKeyword
                | SyntaxKind::PublicKeyword
                | SyntaxKind::ReadonlyKeyword => {
                    let previous_token = self.token();
                    self.next_token();
                    if self.scanner.has_preceding_line_break() {
                        return false;
                    }
                    if previous_token == SyntaxKind::DeclareKeyword
                        && self.token() == SyntaxKind::TypeKeyword
                    {
                        return true;
                    }
                }
                SyntaxKind::GlobalKeyword => {
                    self.next_token();
                    return matches!(
                        self.token(),
                        SyntaxKind::OpenBraceToken
                            | SyntaxKind::Identifier
                            | SyntaxKind::ExportKeyword
                    );
                }
                SyntaxKind::ImportKeyword => {
                    self.next_token();
                    return matches!(
                        self.token(),
                        SyntaxKind::DeferKeyword
                            | SyntaxKind::StringLiteral
                            | SyntaxKind::AsteriskToken
                            | SyntaxKind::OpenBraceToken
                    ) || token_is_identifier_or_keyword(self.token());
                }
                SyntaxKind::ExportKeyword => {
                    let mut current_token = self.next_token();
                    if current_token == SyntaxKind::TypeKeyword {
                        current_token = self.look_ahead(|parser| parser.next_token());
                    }
                    if matches!(
                        current_token,
                        SyntaxKind::EqualsToken
                            | SyntaxKind::AsteriskToken
                            | SyntaxKind::OpenBraceToken
                            | SyntaxKind::DefaultKeyword
                            | SyntaxKind::AsKeyword
                            | SyntaxKind::AtToken
                    ) {
                        return true;
                    }
                }
                SyntaxKind::StaticKeyword => {
                    self.next_token();
                }
                _ => return false,
            }
        }
    }

    fn next_token_is_identifier_on_same_line(&mut self) -> bool {
        self.next_token();
        !self.scanner.has_preceding_line_break() && self.is_identifier()
    }

    fn next_token_is_identifier_or_string_literal_on_same_line(&mut self) -> bool {
        self.next_token();
        !self.scanner.has_preceding_line_break()
            && (self.is_identifier() || self.token() == SyntaxKind::StringLiteral)
    }

    fn is_identifier_node(&self, id: NodeId) -> bool {
        self.arena.node(id).kind == SyntaxKind::Identifier
    }

    fn current_token_text(&self) -> String {
        if self.scanner.token_value().is_empty() {
            token_to_string(self.token())
        } else {
            self.scanner.token_value().to_owned()
        }
    }

    /// tsc isFileProbablyExternalModule: the first statement that is an
    /// import/export form or carries an export modifier, else import.meta.
    fn is_file_probably_external_module(&self, statements: crate::NodeArrayId) -> Option<NodeId> {
        let statements = &self.arena.node_array(statements).nodes;
        statements
            .iter()
            .copied()
            .find(|statement| self.is_an_external_module_indicator_node(*statement))
            .or_else(|| {
                statements
                    .iter()
                    .copied()
                    .find_map(|statement| self.walk_tree_for_import_meta(statement))
            })
    }

    fn is_an_external_module_indicator_node(&self, id: NodeId) -> bool {
        let node = self.arena.node(id);
        match &node.data {
            NodeData::ImportEqualsDeclaration(data) => {
                data.module_reference.is_some_and(|reference| {
                    self.arena.node(reference).kind == SyntaxKind::ExternalModuleReference
                })
            }
            NodeData::ImportDeclaration(_)
            | NodeData::ExportAssignment(_)
            | NodeData::ExportDeclaration(_) => true,
            _ => self.statement_modifiers(id).is_some_and(|modifiers| {
                self.arena
                    .node_array(modifiers)
                    .nodes
                    .iter()
                    .any(|modifier| self.arena.node(*modifier).kind == SyntaxKind::ExportKeyword)
            }),
        }
    }

    fn statement_modifiers(&self, id: NodeId) -> Option<crate::NodeArrayId> {
        match &self.arena.node(id).data {
            NodeData::FunctionDeclaration(data) => data.modifiers,
            NodeData::ClassDeclaration(data) => data.modifiers,
            NodeData::VariableStatement(data) => data.modifiers,
            NodeData::InterfaceDeclaration(data) => data.modifiers,
            NodeData::TypeAliasDeclaration(data) => data.modifiers,
            NodeData::EnumDeclaration(data) => data.modifiers,
            NodeData::ModuleDeclaration(data) => data.modifiers,
            NodeData::ImportEqualsDeclaration(data) => data.modifiers,
            NodeData::MissingDeclaration(data) => data.modifiers,
            NodeData::NamespaceExportDeclaration(data) => data.modifiers,
            _ => None,
        }
    }

    /// tsc walkTreeForImportMeta / isImportMeta. MetaProperty carries no
    /// keywordToken slot, so the leading source token disambiguates
    /// `import.meta` from `new.target`.
    fn walk_tree_for_import_meta(&self, id: NodeId) -> Option<NodeId> {
        let node = self.arena.node(id);
        if node.kind == SyntaxKind::MetaProperty {
            let token_start = crate::scanner::skip_trivia(self.scanner.text(), node.pos as usize);
            let is_import = self.scanner.text()[token_start..].starts_with("import");
            let is_meta = match &node.data {
                NodeData::MetaProperty(data) => data.name.is_some_and(|name| {
                    matches!(&self.arena.node(name).data, NodeData::Identifier(data) if data.escaped_text == "meta")
                }),
                _ => false,
            };
            if is_import && is_meta {
                return Some(id);
            }
        }
        let mut children = Vec::new();
        for_each_child(&self.arena, node, |child| {
            children.push(child);
            false
        });
        children
            .into_iter()
            .find_map(|child| self.walk_tree_for_import_meta(child))
    }

    /// The tsc sourceFile.transformFlags & ContainsPossibleTopLevelAwait
    /// guard: a statement contributes unless its own kind strips the flag on
    /// outward propagation (FunctionExcludes).
    fn statements_possibly_contain_top_level_await(&self, statements: crate::NodeArrayId) -> bool {
        self.arena
            .node_array(statements)
            .nodes
            .iter()
            .any(|statement| {
                self.arena.node(*statement).kind != SyntaxKind::FunctionDeclaration
                    && self.statement_contains_possible_top_level_await(*statement)
            })
    }

    /// tsc containsPossibleTopLevelAwait (statement-level).
    fn statement_contains_possible_top_level_await(&self, id: NodeId) -> bool {
        !NodeFlags::from_bits(self.arena.node(id).flags).contains(NodeFlags::AWAIT_CONTEXT)
            && self.subtree_contains_possible_top_level_await(id)
    }

    /// Simulates TransformFlags.ContainsPossibleTopLevelAwait: the only
    /// source is an identifier spelled `await` (factory createIdentifier);
    /// function-like factories strip the flag from their BODY propagation;
    /// enum/module/import= factories clear it on the whole node.
    fn subtree_contains_possible_top_level_await(&self, id: NodeId) -> bool {
        let node = self.arena.node(id);
        let body = match &node.data {
            NodeData::Identifier(data) => return data.escaped_text == "await",
            NodeData::EnumDeclaration(_)
            | NodeData::ModuleDeclaration(_)
            | NodeData::ImportEqualsDeclaration(_)
            | NodeData::ImportDeclaration(_)
            | NodeData::ImportClause(_)
            | NodeData::NamespaceImport(_)
            | NodeData::NamespaceExport(_)
            | NodeData::NamedImports(_)
            | NodeData::ImportSpecifier(_)
            | NodeData::ExportAssignment(_)
            | NodeData::ExportDeclaration(_)
            | NodeData::NamedExports(_)
            | NodeData::ExportSpecifier(_)
            | NodeData::ExternalModuleReference(_) => return false,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::ArrowFunction(data) => data.body,
            NodeData::FunctionDeclaration(data) => data.body,
            _ => None,
        };
        let mut children = Vec::new();
        for_each_child(&self.arena, node, |child| {
            children.push(child);
            false
        });
        children
            .into_iter()
            .filter(|child| Some(*child) != body)
            .any(|child| self.subtree_contains_possible_top_level_await(child))
    }

    /// tsc reparseTopLevelAwait: maximal runs of possible-await statements
    /// re-parse from their start in the Await context; parse diagnostics in
    /// the re-parsed ranges are replaced by the re-parse output.
    fn reparse_top_level_await(&mut self, statements_id: crate::NodeArrayId) -> crate::NodeArrayId {
        let (old_statements, array_pos, array_end) = {
            let array = self.arena.node_array(statements_id);
            (array.nodes.clone(), array.pos as usize, array.end as usize)
        };
        let saved_diagnostics = std::mem::take(&mut self.parse_diagnostics);
        let mut statements: Vec<NodeId> = Vec::new();

        let mut pos: Option<usize> = Some(0);
        let mut start = self.find_statement_with_await(&old_statements, 0);
        while let Some(run_start) = start {
            let keep_from = pos.expect("a run exists only while the cursor does");
            statements.extend_from_slice(&old_statements[keep_from..run_start]);
            pos = self.find_statement_without_await(&old_statements, run_start);

            // Keep the diagnostics of the untouched range.
            let prev_pos = self.to_utf16(self.arena.node(old_statements[keep_from]).pos as usize);
            let next_pos = self.to_utf16(self.arena.node(old_statements[run_start]).pos as usize);
            if let Some(diag_start) = saved_diagnostics
                .iter()
                .position(|diagnostic| diagnostic.start.is_some_and(|start| start >= prev_pos))
            {
                let diag_end = saved_diagnostics[diag_start..]
                    .iter()
                    .position(|diagnostic| diagnostic.start.is_some_and(|start| start >= next_pos))
                    .map(|offset| diag_start + offset)
                    .unwrap_or(saved_diagnostics.len());
                self.parse_diagnostics
                    .extend_from_slice(&saved_diagnostics[diag_start..diag_end]);
            }

            // Re-parse in the Await context (tsc SpeculationKind.Reparse:
            // diagnostics kept, scanner state restored afterwards).
            let scanner_state = self.scanner.save();
            let saved_context_flags = self.context_flags;
            self.context_flags =
                NodeFlags::from_bits(self.context_flags.bits() | NodeFlags::AWAIT_CONTEXT.bits());
            self.scanner
                .reset_token_state(self.arena.node(old_statements[run_start]).pos as usize);
            self.next_token();
            while self.token() != SyntaxKind::EndOfFileToken {
                let start_pos = self.scanner.full_start_pos();
                let statement = self.parse_statement();
                statements.push(statement);
                if start_pos == self.scanner.full_start_pos() {
                    self.next_token();
                }
                if let Some(cursor) = pos {
                    let non_await_pos = self.arena.node(old_statements[cursor]).pos;
                    let statement_end = self.arena.node(statement).end;
                    if statement_end == non_await_pos {
                        break;
                    }
                    if statement_end > non_await_pos {
                        pos = self.find_statement_without_await(&old_statements, cursor + 1);
                    }
                }
            }
            self.context_flags = saved_context_flags;
            self.scanner.restore(scanner_state);

            start = pos.and_then(|cursor| self.find_statement_with_await(&old_statements, cursor));
        }
        if let Some(keep_from) = pos {
            statements.extend_from_slice(&old_statements[keep_from..]);
            let prev_pos = self.to_utf16(self.arena.node(old_statements[keep_from]).pos as usize);
            if let Some(diag_start) = saved_diagnostics
                .iter()
                .position(|diagnostic| diagnostic.start.is_some_and(|start| start >= prev_pos))
            {
                self.parse_diagnostics
                    .extend_from_slice(&saved_diagnostics[diag_start..]);
            }
        }

        self.arena
            .alloc_array(statements, array_pos, array_end, false)
    }

    fn find_statement_with_await(&self, statements: &[NodeId], from: usize) -> Option<usize> {
        (from..statements.len())
            .find(|index| self.statement_contains_possible_top_level_await(statements[*index]))
    }

    fn find_statement_without_await(&self, statements: &[NodeId], from: usize) -> Option<usize> {
        (from..statements.len())
            .find(|index| !self.statement_contains_possible_top_level_await(statements[*index]))
    }

    fn finish_node_data(&mut self, data: NodeData, pos: usize) -> NodeId {
        self.finish_node_with_flags(data, pos, NodeFlags::NONE)
    }

    fn finish_node_with_flags(&mut self, data: NodeData, pos: usize, flags: NodeFlags) -> NodeId {
        let id = self
            .arena
            .alloc_node(data, pos, self.scanner.full_start_pos(), flags);
        self.finish_node(id, pos)
    }

    fn finish(
        mut self,
        statements: crate::NodeArrayId,
        end_of_file_token: NodeId,
    ) -> FinishedParse {
        let eof_end = self.arena.node(end_of_file_token).end as usize;
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
    let mut statements = parser.parse_list(ParsingContext::SourceElements, |parser| {
        Some(parser.parse_statement())
    });
    debug_assert_eq!(parser.token(), SyntaxKind::EndOfFileToken);
    let end_of_file_token = parser.parse_token_node();

    // tsc createSourceFile2: a module whose statements possibly contain
    // top-level await re-parses those statements in the Await context.
    if !parser.is_declaration_file
        && parser
            .is_file_probably_external_module(statements)
            .is_some()
        && parser.statements_possibly_contain_top_level_await(statements)
    {
        statements = parser.reparse_top_level_await(statements);
    }
    let external_module_indicator = parser.is_file_probably_external_module(statements);

    let finished = parser.finish(statements, end_of_file_token);
    SourceFile {
        file_name: finished.file_name,
        text,
        language_variant: finished.language_variant,
        is_declaration_file: finished.is_declaration_file,
        line_map: finished.line_map,
        arena: finished.arena,
        root: finished.root,
        external_module_indicator,
        parse_diagnostics: finished.parse_diagnostics,
    }
}

/// tsc Parser.parseJsonText. The JavaScriptFile/JsonFile context flags are
/// not stamped (nothing consumes them yet).
pub fn parse_json_text(file_name: String, text: String) -> SourceFile {
    let mut parser = Parser::new(file_name, &text, LanguageVariant::Standard);
    parser.next_token();
    let pos = parser.node_pos();

    let (statements, end_of_file_token) = if parser.token() == SyntaxKind::EndOfFileToken {
        let statements = parser.arena.alloc_array(Vec::new(), pos, pos, false);
        let end_of_file_token = parser.parse_token_node();
        (statements, end_of_file_token)
    } else {
        let mut expressions = Vec::new();
        while parser.token() != SyntaxKind::EndOfFileToken {
            let expression = match parser.token() {
                SyntaxKind::OpenBracketToken => parser.parse_array_literal_expression(),
                SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword | SyntaxKind::NullKeyword => {
                    parser.parse_token_node()
                }
                SyntaxKind::MinusToken => {
                    if parser.look_ahead(|parser| {
                        parser.next_token() == SyntaxKind::NumericLiteral
                            && parser.next_token() != SyntaxKind::ColonToken
                    }) {
                        parser.parse_prefix_unary_expression()
                    } else {
                        parser.parse_object_literal_expression()
                    }
                }
                SyntaxKind::NumericLiteral
                    if parser
                        .look_ahead(|parser| parser.next_token() != SyntaxKind::ColonToken) =>
                {
                    parser.parse_numeric_literal()
                }
                SyntaxKind::StringLiteral
                    if parser
                        .look_ahead(|parser| parser.next_token() != SyntaxKind::ColonToken) =>
                {
                    parser.parse_string_literal()
                }
                _ => parser.parse_object_literal_expression(),
            };

            if expressions.is_empty() && parser.token() != SyntaxKind::EndOfFileToken {
                parser.parse_error_at_current_token(&gen::Unexpected_token, &[]);
            }
            expressions.push(expression);
        }

        let expression = if expressions.len() > 1 {
            let expressions_end = parser.node_pos();
            let elements = parser
                .arena
                .alloc_array(expressions, pos, expressions_end, false);
            parser.finish_node_data(
                NodeData::ArrayLiteralExpression(ArrayLiteralExpressionData {
                    elements: Some(elements),
                }),
                pos,
            )
        } else {
            expressions[0]
        };
        let statement = parser.finish_node_data(
            NodeData::ExpressionStatement(ExpressionStatementData {
                expression: Some(expression),
            }),
            pos,
        );
        let statements = parser
            .arena
            .alloc_array(vec![statement], pos, parser.node_pos(), false);
        let end_of_file_token =
            parser.parse_expected_token(SyntaxKind::EndOfFileToken, Some(&gen::Unexpected_token));
        (statements, end_of_file_token)
    };

    let finished = parser.finish(statements, end_of_file_token);
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

fn is_assignment_operator(kind: SyntaxKind) -> bool {
    kind.value() >= SyntaxKind::FirstAssignment.value()
        && kind.value() <= SyntaxKind::LastAssignment.value()
}

/// tsc isLeftHandSideExpressionKind: the only left sides an assignment
/// operator may bind to; otherwise `=` is left for the outer context.
fn is_left_hand_side_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::NewExpression
            | SyntaxKind::CallExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxFragment
            | SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ParenthesizedExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::ClassExpression
            | SyntaxKind::FunctionExpression
            | SyntaxKind::Identifier
            | SyntaxKind::PrivateIdentifier
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateExpression
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::ThisKeyword
            | SyntaxKind::TrueKeyword
            | SyntaxKind::SuperKeyword
            | SyntaxKind::NonNullExpression
            | SyntaxKind::ExpressionWithTypeArguments
            | SyntaxKind::MetaProperty
            | SyntaxKind::ImportKeyword
            | SyntaxKind::MissingDeclaration
    )
}

/// tsc OperatorPrecedence.Lowest (Comma). `getBinaryOperatorPrecedence`
/// never returns it, so the Pratt loop always consumes the first operator.
const LOWEST_OPERATOR_PRECEDENCE: i32 = 0;

/// tsc getBinaryOperatorPrecedence; -1 for non-operators (loop exits).
/// In TS 6.0 Coalesce == LogicalOR == 5.
fn get_binary_operator_precedence(kind: SyntaxKind) -> i32 {
    match kind {
        SyntaxKind::QuestionQuestionToken => 5,
        SyntaxKind::BarBarToken => 5,
        SyntaxKind::AmpersandAmpersandToken => 6,
        SyntaxKind::BarToken => 7,
        SyntaxKind::CaretToken => 8,
        SyntaxKind::AmpersandToken => 9,
        SyntaxKind::EqualsEqualsToken
        | SyntaxKind::ExclamationEqualsToken
        | SyntaxKind::EqualsEqualsEqualsToken
        | SyntaxKind::ExclamationEqualsEqualsToken => 10,
        SyntaxKind::LessThanToken
        | SyntaxKind::GreaterThanToken
        | SyntaxKind::LessThanEqualsToken
        | SyntaxKind::GreaterThanEqualsToken
        | SyntaxKind::InstanceOfKeyword
        | SyntaxKind::InKeyword
        | SyntaxKind::AsKeyword
        | SyntaxKind::SatisfiesKeyword => 11,
        SyntaxKind::LessThanLessThanToken
        | SyntaxKind::GreaterThanGreaterThanToken
        | SyntaxKind::GreaterThanGreaterThanGreaterThanToken => 12,
        SyntaxKind::PlusToken | SyntaxKind::MinusToken => 13,
        SyntaxKind::AsteriskToken | SyntaxKind::SlashToken | SyntaxKind::PercentToken => 14,
        SyntaxKind::AsteriskAsteriskToken => 15,
        _ => -1,
    }
}

/// tsc's viableKeywordSuggestions: keyword texts longer than two characters,
/// in textToKeywordObj order (ties in getSpellingSuggestion keep the first).
const VIABLE_KEYWORD_SUGGESTIONS: &[&str] = &[
    "abstract",
    "accessor",
    "any",
    "asserts",
    "assert",
    "bigint",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "const",
    "debugger",
    "declare",
    "default",
    "defer",
    "delete",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "get",
    "implements",
    "import",
    "infer",
    "instanceof",
    "interface",
    "intrinsic",
    "keyof",
    "let",
    "module",
    "namespace",
    "never",
    "new",
    "null",
    "number",
    "object",
    "package",
    "private",
    "protected",
    "public",
    "override",
    "out",
    "readonly",
    "require",
    "global",
    "return",
    "satisfies",
    "set",
    "static",
    "string",
    "super",
    "switch",
    "symbol",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "unique",
    "unknown",
    "using",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "async",
    "await",
];

/// tsc isClassMemberModifier.
fn is_class_member_modifier(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PublicKeyword
            | SyntaxKind::PrivateKeyword
            | SyntaxKind::ProtectedKeyword
            | SyntaxKind::ReadonlyKeyword
            | SyntaxKind::StaticKeyword
            | SyntaxKind::OverrideKeyword
            | SyntaxKind::AccessorKeyword
    )
}

fn is_identifier_text(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    crate::chars::is_identifier_start(first) && chars.all(crate::chars::is_identifier_part)
}

/// tsc getSpellingSuggestion.
fn get_spelling_suggestion<'a>(name: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let name_chars: Vec<char> = name.chars().collect();
    let maximum_length_difference = 2.max((0.34 * name_chars.len() as f64).floor() as usize);
    let mut best_distance = (name_chars.len() as f64 * 0.4).floor() + 1.0;
    let mut best_candidate = None;
    for &candidate in candidates {
        let candidate_chars: Vec<char> = candidate.chars().collect();
        if candidate_chars.len().abs_diff(name_chars.len()) > maximum_length_difference {
            continue;
        }
        if candidate == name {
            continue;
        }
        if candidate_chars.len() < 3 && candidate.to_lowercase() != name.to_lowercase() {
            continue;
        }
        let Some(distance) =
            levenshtein_with_max(&name_chars, &candidate_chars, best_distance - 0.1)
        else {
            continue;
        };
        debug_assert!(distance < best_distance);
        best_distance = distance;
        best_candidate = Some(candidate);
    }
    best_candidate
}

/// tsc levenshteinWithMax (char-swap cost 0.1, substitution 2, in/del 1).
fn levenshtein_with_max(s1: &[char], s2: &[char], max: f64) -> Option<f64> {
    let mut previous: Vec<f64> = (0..=s2.len()).map(|i| i as f64).collect();
    let mut current: Vec<f64> = vec![0.0; s2.len() + 1];
    let big = max + 0.01;
    for i in 1..=s1.len() {
        let c1 = s1[i - 1];
        let min_j = ((i as f64 - max).max(1.0)).ceil() as usize;
        let max_j = ((max + i as f64).min(s2.len() as f64)).floor() as usize;
        current[0] = i as f64;
        let mut col_min = i as f64;
        for entry in current.iter_mut().take(min_j).skip(1) {
            *entry = big;
        }
        for j in min_j..=max_j {
            let substitution_distance = if s1[i - 1].to_lowercase().eq(s2[j - 1].to_lowercase()) {
                previous[j - 1] + 0.1
            } else {
                previous[j - 1] + 2.0
            };
            let dist = if c1 == s2[j - 1] {
                previous[j - 1]
            } else {
                (previous[j] + 1.0)
                    .min(current[j - 1] + 1.0)
                    .min(substitution_distance)
            };
            current[j] = dist;
            col_min = col_min.min(dist);
        }
        for entry in current.iter_mut().take(s2.len() + 1).skip(max_j + 1) {
            *entry = big;
        }
        if col_min > max {
            return None;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let res = previous[s2.len()];
    if res > max {
        None
    } else {
        Some(res)
    }
}

/// tsc getSpaceSuggestion.
fn get_space_suggestion(expression_text: &str) -> Option<String> {
    for &keyword in VIABLE_KEYWORD_SUGGESTIONS {
        if expression_text.len() > keyword.len() + 2 && expression_text.starts_with(keyword) {
            return Some(format!("{} {}", keyword, &expression_text[keyword.len()..]));
        }
    }
    None
}

fn context_flags_for_function_body(is_generator: bool, is_async: bool) -> (NodeFlags, NodeFlags) {
    let mut set = NodeFlags::NONE;
    let mut clear = NodeFlags::NONE;
    if is_generator {
        set |= NodeFlags::YIELD_CONTEXT;
    } else {
        clear |= NodeFlags::YIELD_CONTEXT;
    }
    if is_async {
        set |= NodeFlags::AWAIT_CONTEXT;
    } else {
        clear |= NodeFlags::AWAIT_CONTEXT;
    }
    (set, clear)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_tsx(text: &str) -> SourceFile {
        parse_source_file(
            "a.tsx".to_owned(),
            text.to_owned(),
            ParseOptions {
                language_variant: LanguageVariant::Jsx,
            },
            None,
        )
    }

    fn first_initializer(source: &SourceFile) -> NodeId {
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        let statement = source
            .arena
            .node(statements.nodes[0])
            .data
            .as_variable_statement()
            .expect("variable statement");
        let list = source
            .arena
            .node(statement.declaration_list.expect("declaration list"))
            .data
            .as_variable_declaration_list()
            .expect("declaration list data")
            .declarations
            .expect("declarations");
        let declaration = source
            .arena
            .node(source.arena.node_array(list).nodes[0])
            .data
            .as_variable_declaration()
            .expect("variable declaration");
        declaration.initializer.expect("initializer")
    }

    fn diagnostic_pins(source: &SourceFile) -> Vec<(u32, Option<u32>, Option<u32>)> {
        source
            .parse_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code(), diagnostic.start, diagnostic.length))
            .collect()
    }

    #[test]
    fn jsx_element_attributes_and_children_oracle_pins() {
        let source = parse_tsx("const a = <div className=\"x\" {...props}>hello{world}</div>;");
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );

        let element = source
            .arena
            .node(first_initializer(&source))
            .data
            .as_jsx_element()
            .expect("jsx element");
        let opening = source
            .arena
            .node(element.opening_element.expect("opening"))
            .data
            .as_jsx_opening_element()
            .expect("opening element");
        let attributes = source
            .arena
            .node(opening.attributes.expect("attributes"))
            .data
            .as_jsx_attributes()
            .expect("attributes data")
            .properties
            .expect("properties");
        let attribute_kinds: Vec<_> = source
            .arena
            .node_array(attributes)
            .nodes
            .iter()
            .map(|id| source.arena.node(*id).kind)
            .collect();
        assert_eq!(
            attribute_kinds,
            [SyntaxKind::JsxAttribute, SyntaxKind::JsxSpreadAttribute]
        );

        let child_kinds: Vec<_> = source
            .arena
            .node_array(element.children.expect("children"))
            .nodes
            .iter()
            .map(|id| source.arena.node(*id).kind)
            .collect();
        assert_eq!(
            child_kinds,
            [SyntaxKind::JsxText, SyntaxKind::JsxExpression]
        );
    }

    #[test]
    fn jsx_fragment_oracle_pins() {
        let source = parse_tsx("const b = <>text{1}<br/></>;");
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );

        let fragment = source
            .arena
            .node(first_initializer(&source))
            .data
            .as_jsx_fragment()
            .expect("jsx fragment");
        assert_eq!(
            source
                .arena
                .node(fragment.opening_fragment.expect("opening fragment"))
                .kind,
            SyntaxKind::JsxOpeningFragment
        );
        assert_eq!(
            source
                .arena
                .node(fragment.closing_fragment.expect("closing fragment"))
                .kind,
            SyntaxKind::JsxClosingFragment
        );
        let child_kinds: Vec<_> = source
            .arena
            .node_array(fragment.children.expect("children"))
            .nodes
            .iter()
            .map(|id| source.arena.node(*id).kind)
            .collect();
        assert_eq!(
            child_kinds,
            [
                SyntaxKind::JsxText,
                SyntaxKind::JsxExpression,
                SyntaxKind::JsxSelfClosingElement
            ]
        );
    }

    #[test]
    fn jsx_tag_and_attribute_name_shapes() {
        let source = parse_tsx("const c = <Foo.Bar a:b=\"1\" this-prop={2} />;");
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );

        let element = source
            .arena
            .node(first_initializer(&source))
            .data
            .as_jsx_self_closing_element()
            .expect("self-closing element");
        assert_eq!(
            source.arena.node(element.tag_name.expect("tag name")).kind,
            SyntaxKind::PropertyAccessExpression
        );
        let attributes = source
            .arena
            .node(element.attributes.expect("attributes"))
            .data
            .as_jsx_attributes()
            .expect("attributes data")
            .properties
            .expect("properties");
        let attribute_nodes = &source.arena.node_array(attributes).nodes;
        let namespaced = source
            .arena
            .node(attribute_nodes[0])
            .data
            .as_jsx_attribute()
            .expect("first attribute");
        assert_eq!(
            source.arena.node(namespaced.name.expect("name")).kind,
            SyntaxKind::JsxNamespacedName
        );
        let dashed = source
            .arena
            .node(attribute_nodes[1])
            .data
            .as_jsx_attribute()
            .expect("second attribute");
        let dashed_name = source
            .arena
            .node(dashed.name.expect("name"))
            .data
            .as_identifier()
            .expect("identifier name");
        assert_eq!(dashed_name.escaped_text, "this-prop");
    }

    #[test]
    fn jsx_this_tag_name() {
        let source = parse_tsx("function g() { return <this.Component />; }");
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
    }

    #[test]
    fn jsx_closing_tag_mismatch_oracle_pins() {
        let source = parse_tsx("const d = <div></span>;");
        assert_eq!(diagnostic_pins(&source), [(17002, Some(17), Some(4))]);
    }

    #[test]
    fn jsx_sibling_elements_glued_with_synthetic_comma() {
        let source = parse_tsx("const e = <div/><span/>;");
        assert_eq!(diagnostic_pins(&source), [(2657, Some(10), Some(13))]);
        assert_eq!(
            source.arena.node(first_initializer(&source)).kind,
            SyntaxKind::BinaryExpression
        );
    }

    #[test]
    fn jsx_rebalances_closing_tag_owned_by_outer_element() {
        let source = parse_tsx("const f = <div><b>text</div>;");
        assert_eq!(diagnostic_pins(&source), [(17008, Some(16), Some(1))]);

        let outer = source
            .arena
            .node(first_initializer(&source))
            .data
            .as_jsx_element()
            .expect("outer element");
        let children = source.arena.node_array(outer.children.expect("children"));
        let inner = source
            .arena
            .node(*children.nodes.last().expect("inner child"))
            .data
            .as_jsx_element()
            .expect("inner element");
        let synthetic_closing = source
            .arena
            .node(inner.closing_element.expect("synthetic closing"));
        assert_eq!(synthetic_closing.pos, synthetic_closing.end);
    }

    #[test]
    fn jsx_unclosed_element_at_eof_oracle_pins() {
        let source = parse_tsx("const h = <div>");
        assert_eq!(
            diagnostic_pins(&source),
            [(17008, Some(11), Some(3)), (1005, Some(15), Some(0))]
        );
    }

    #[test]
    fn for_of_expression_initializer_stays_an_expression() {
        // tsc: `for (x of a)` initializer is an Identifier, not a
        // VariableDeclarationList (the using-declaration lookahead must not
        // fire on `x of`).
        let source = parse_source_file(
            "a.ts".to_owned(),
            "declare var x: string, a: string[]; for (x of a) { }".to_owned(),
            ParseOptions::default(),
            None,
        );
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        let for_of = source
            .arena
            .node(statements.nodes[1])
            .data
            .as_for_of_statement()
            .expect("for-of statement");
        assert_eq!(
            source
                .arena
                .node(for_of.initializer.expect("initializer"))
                .kind,
            SyntaxKind::Identifier
        );
    }

    #[test]
    fn using_with_bracket_is_an_expression_statement() {
        // tsc: `using [a] = null` is element-access assignment, not a using
        // declaration (the lookahead accepts identifiers and `{` only).
        let source = parse_source_file(
            "a.ts".to_owned(),
            "declare var using: any[], a: number; function f() { using [a] = null; }".to_owned(),
            ParseOptions::default(),
            None,
        );
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        let function = source
            .arena
            .node(statements.nodes[1])
            .data
            .as_function_declaration()
            .expect("function declaration");
        let body = source
            .arena
            .node(function.body.expect("body"))
            .data
            .as_block()
            .expect("function body");
        let body_statements = source
            .arena
            .node_array(body.statements.expect("body statements"));
        assert_eq!(
            source.arena.node(body_statements.nodes[0]).kind,
            SyntaxKind::ExpressionStatement
        );
    }

    #[test]
    fn export_before_bare_identifier_reports_declaration_expected() {
        // tsc: `export i` is not a statement start (isStartOfStatement
        // ExportKeyword arm → isStartOfDeclaration false), so the list
        // machinery reports 1128 at `export`, not 1434.
        let source = parse_source_file(
            "a.ts".to_owned(),
            "declare module \"*.foo\" {\n  export i\n".to_owned(),
            ParseOptions::default(),
            None,
        );
        let pins: Vec<(u32, u32, u32)> = source
            .parse_diagnostics
            .iter()
            .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
            .collect();
        assert_eq!(pins, [(1128, 27, 6), (1005, 36, 0)]);
    }

    #[test]
    fn binding_patterns_support_computed_names_and_nesting() {
        // tsc parses both clean: computed property names in object binding
        // elements, nested patterns in array binding elements.
        let source = parse_source_file(
            "a.ts".to_owned(),
            "declare var f: any; let [{ [f(1)]: x } = f(0)] = []; let [[a], { b: [c] }] = f;"
                .to_owned(),
            ParseOptions::default(),
            None,
        );
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
    }

    #[test]
    fn parse_json_text_oracle_pins() {
        // Pins collected from ts.parseJsonText (vendor 6.0.3).
        type JsonPin = (&'static str, &'static [(u32, u32, u32)], Option<SyntaxKind>);
        let cases: &[JsonPin] = &[
            (
                "{ \"name\": \"p\", \"exports\": { \".\": \"./i.js\" } }",
                &[],
                Some(SyntaxKind::ObjectLiteralExpression),
            ),
            ("-5", &[], Some(SyntaxKind::PrefixUnaryExpression)),
            (
                "1 2",
                &[(1012, 2, 1)],
                Some(SyntaxKind::ArrayLiteralExpression),
            ),
            ("", &[], None),
            ("\"hello\"", &[], Some(SyntaxKind::StringLiteral)),
            (
                "[1, true, null]",
                &[],
                Some(SyntaxKind::ArrayLiteralExpression),
            ),
            // Unquoted keys and trailing commas are checker errors, not
            // parse errors.
            (
                "{ name: 1, }",
                &[],
                Some(SyntaxKind::ObjectLiteralExpression),
            ),
        ];
        for (text, diagnostics, expression_kind) in cases {
            let source = parse_json_text("a.json".to_owned(), (*text).to_owned());
            let pins: Vec<(u32, u32, u32)> = source
                .parse_diagnostics
                .iter()
                .map(|d| (d.code(), d.start.unwrap_or(0), d.length.unwrap_or(0)))
                .collect();
            assert_eq!(&pins, diagnostics, "diagnostics for {text:?}");

            let root = source
                .arena
                .node(source.root)
                .data
                .as_source_file()
                .expect("source file root");
            let statements = source
                .arena
                .node_array(root.statements.expect("statements"));
            match expression_kind {
                None => assert!(statements.nodes.is_empty(), "statements for {text:?}"),
                Some(kind) => {
                    let statement = source
                        .arena
                        .node(statements.nodes[0])
                        .data
                        .as_expression_statement()
                        .expect("expression statement");
                    assert_eq!(
                        source
                            .arena
                            .node(statement.expression.expect("expression"))
                            .kind,
                        *kind,
                        "expression kind for {text:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn type_assertion_still_parses_in_standard_variant() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "const e = <string>x;".to_owned(),
            ParseOptions::default(),
            None,
        );
        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
    }

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
    fn parse_source_file_builds_statement_tree() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let x = 1; const y = 2; if (x) { debugger; }".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        assert_eq!(statements.nodes.len(), 3);

        let variable_statement = source.arena.node(statements.nodes[0]);
        let NodeData::VariableStatement(variable_statement_data) = &variable_statement.data else {
            panic!("expected variable statement");
        };
        let declaration_list = variable_statement_data
            .declaration_list
            .expect("declaration list");
        assert!(
            NodeFlags::from_bits(source.arena.node(declaration_list).flags)
                .contains(NodeFlags::LET)
        );
        let declaration_list_data = source
            .arena
            .node(declaration_list)
            .data
            .as_variable_declaration_list()
            .expect("variable declaration list");
        let declarations = source
            .arena
            .node_array(declaration_list_data.declarations.expect("declarations"));
        assert_eq!(declarations.nodes.len(), 1);
        let declaration = source
            .arena
            .node(declarations.nodes[0])
            .data
            .as_variable_declaration()
            .expect("variable declaration");
        assert_eq!(
            source.arena.node(declaration.name.expect("name")).kind,
            SyntaxKind::Identifier
        );
        assert_eq!(
            source
                .arena
                .node(declaration.initializer.expect("initializer"))
                .kind,
            SyntaxKind::NumericLiteral
        );

        let const_statement = source.arena.node(statements.nodes[1]);
        let NodeData::VariableStatement(const_statement_data) = &const_statement.data else {
            panic!("expected const variable statement");
        };
        let const_declaration_list = const_statement_data
            .declaration_list
            .expect("const declaration list");
        assert!(
            NodeFlags::from_bits(source.arena.node(const_declaration_list).flags)
                .contains(NodeFlags::CONST)
        );

        let if_statement = source
            .arena
            .node(statements.nodes[2])
            .data
            .as_if_statement()
            .expect("if statement");
        let then_block = source
            .arena
            .node(if_statement.then_statement.expect("then statement"))
            .data
            .as_block()
            .expect("then block");
        let block_statements = source
            .arena
            .node_array(then_block.statements.expect("block statements"));
        assert_eq!(block_statements.nodes.len(), 1);
        assert_eq!(
            source.arena.node(block_statements.nodes[0]).kind,
            SyntaxKind::DebuggerStatement
        );
    }

    #[test]
    fn parse_import_and_ambient_function_declarations() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "import {foo, baz} from \"foobarbaz\";\nfoo(baz);\ndeclare function fn7(x, y?, ...z);\ndeclare function fn9(...q: {}[]);\n".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        let kinds: Vec<SyntaxKind> = statements
            .nodes
            .iter()
            .map(|&statement| source.arena.node(statement).kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::ImportDeclaration,
                SyntaxKind::ExpressionStatement,
                SyntaxKind::FunctionDeclaration,
                SyntaxKind::FunctionDeclaration,
            ]
        );
        let NodeData::FunctionDeclaration(ambient) = &source.arena.node(statements.nodes[2]).data
        else {
            panic!("expected function declaration");
        };
        assert!(ambient.modifiers.is_some());
        assert!(ambient.body.is_none());
    }

    #[test]
    fn parse_primary_expression_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "const arr = [1,,...x]; const obj = {a: 1, b, ...c, [d.e]: 2}; new.target; /x/g; const t = `a${b}c`;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        assert_eq!(statements.nodes.len(), 5);

        let initializer = |statement: NodeId| -> NodeId {
            let variable_statement = source
                .arena
                .node(statement)
                .data
                .as_variable_statement()
                .expect("variable statement");
            let declaration_list = source
                .arena
                .node(
                    variable_statement
                        .declaration_list
                        .expect("declaration list"),
                )
                .data
                .as_variable_declaration_list()
                .expect("declaration list data");
            let declarations = source
                .arena
                .node_array(declaration_list.declarations.expect("declarations"));
            source
                .arena
                .node(declarations.nodes[0])
                .data
                .as_variable_declaration()
                .expect("declaration")
                .initializer
                .expect("initializer")
        };

        let arr = initializer(statements.nodes[0]);
        let arr_data = source
            .arena
            .node(arr)
            .data
            .as_array_literal_expression()
            .expect("array literal");
        let arr_elements = source
            .arena
            .node_array(arr_data.elements.expect("array elements"));
        assert_eq!(
            arr_elements
                .nodes
                .iter()
                .map(|id| source.arena.node(*id).kind)
                .collect::<Vec<_>>(),
            vec![
                SyntaxKind::NumericLiteral,
                SyntaxKind::OmittedExpression,
                SyntaxKind::SpreadElement,
            ]
        );

        let obj = initializer(statements.nodes[1]);
        let obj_data = source
            .arena
            .node(obj)
            .data
            .as_object_literal_expression()
            .expect("object literal");
        let properties = source
            .arena
            .node_array(obj_data.properties.expect("properties"));
        assert_eq!(
            properties
                .nodes
                .iter()
                .map(|id| source.arena.node(*id).kind)
                .collect::<Vec<_>>(),
            vec![
                SyntaxKind::PropertyAssignment,
                SyntaxKind::ShorthandPropertyAssignment,
                SyntaxKind::SpreadAssignment,
                SyntaxKind::PropertyAssignment,
            ]
        );
        let computed_property = source
            .arena
            .node(properties.nodes[3])
            .data
            .as_property_assignment()
            .expect("computed property assignment")
            .name
            .expect("computed name");
        assert_eq!(
            source.arena.node(computed_property).kind,
            SyntaxKind::ComputedPropertyName
        );

        let new_target = source
            .arena
            .node(statements.nodes[2])
            .data
            .as_expression_statement()
            .expect("new.target statement")
            .expression
            .expect("new.target expression");
        assert_eq!(source.arena.node(new_target).kind, SyntaxKind::MetaProperty);

        let regex = source
            .arena
            .node(statements.nodes[3])
            .data
            .as_expression_statement()
            .expect("regex statement")
            .expression
            .expect("regex expression");
        assert_eq!(
            source.arena.node(regex).kind,
            SyntaxKind::RegularExpressionLiteral
        );

        let template = initializer(statements.nodes[4]);
        assert_eq!(
            source.arena.node(template).kind,
            SyntaxKind::TemplateExpression
        );
    }

    #[test]
    fn parse_member_and_call_expression_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "foo.bar(1, ...xs); obj?.prop?.[key]?.(arg); tag<T>`x${y}z`; new Foo<T>(arg); x!.y;"
                .to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        assert_eq!(statements.nodes.len(), 5);

        let call_statement = source
            .arena
            .node(statements.nodes[0])
            .data
            .as_expression_statement()
            .expect("call statement");
        let call = source
            .arena
            .node(call_statement.expression.expect("call expression"))
            .data
            .as_call_expression()
            .expect("call expression");
        assert_eq!(
            source.arena.node(call.expression.expect("callee")).kind,
            SyntaxKind::PropertyAccessExpression
        );
        let call_arguments = source
            .arena
            .node_array(call.arguments.expect("call arguments"));
        assert_eq!(call_arguments.nodes.len(), 2);
        assert_eq!(
            source.arena.node(call_arguments.nodes[1]).kind,
            SyntaxKind::SpreadElement
        );

        let optional_call_statement = source
            .arena
            .node(statements.nodes[1])
            .data
            .as_expression_statement()
            .expect("optional call statement");
        let optional_call = source
            .arena
            .node(optional_call_statement.expression.expect("optional call"))
            .data
            .as_call_expression()
            .expect("optional call expression");
        assert!(optional_call.question_dot_token.is_some());
        assert_eq!(
            source
                .arena
                .node(optional_call.expression.expect("optional callee"))
                .kind,
            SyntaxKind::ElementAccessExpression
        );

        let tagged_statement = source
            .arena
            .node(statements.nodes[2])
            .data
            .as_expression_statement()
            .expect("tagged template statement");
        let tagged = source
            .arena
            .node(tagged_statement.expression.expect("tagged template"))
            .data
            .as_tagged_template_expression()
            .expect("tagged template expression");
        assert!(tagged.type_arguments.is_some());
        assert_eq!(
            source.arena.node(tagged.template.expect("template")).kind,
            SyntaxKind::TemplateExpression
        );

        let new_statement = source
            .arena
            .node(statements.nodes[3])
            .data
            .as_expression_statement()
            .expect("new expression statement");
        let new_expression = source
            .arena
            .node(new_statement.expression.expect("new expression"))
            .data
            .as_new_expression()
            .expect("new expression");
        assert!(new_expression.type_arguments.is_some());
        assert_eq!(
            source
                .arena
                .node_array(new_expression.arguments.expect("new arguments"))
                .nodes
                .len(),
            1
        );

        let non_null_statement = source
            .arena
            .node(statements.nodes[4])
            .data
            .as_expression_statement()
            .expect("non-null statement");
        let property_access = source
            .arena
            .node(non_null_statement.expression.expect("property access"))
            .data
            .as_property_access_expression()
            .expect("property access expression");
        assert_eq!(
            source
                .arena
                .node(property_access.expression.expect("non-null base"))
                .kind,
            SyntaxKind::NonNullExpression
        );
    }

    #[test]
    fn parse_unary_update_await_and_yield_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "++a; b--; delete obj.x; typeof y; void z; await q; const g = function*(){ yield; yield* q; }; const h = async function(){ await q; };".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        assert_eq!(statements.nodes.len(), 8);

        let expression_statement_expression = |index: usize| -> NodeId {
            source
                .arena
                .node(statements.nodes[index])
                .data
                .as_expression_statement()
                .expect("expression statement")
                .expression
                .expect("expression")
        };

        assert_eq!(
            source.arena.node(expression_statement_expression(0)).kind,
            SyntaxKind::PrefixUnaryExpression
        );
        assert_eq!(
            source.arena.node(expression_statement_expression(1)).kind,
            SyntaxKind::PostfixUnaryExpression
        );
        assert_eq!(
            source.arena.node(expression_statement_expression(2)).kind,
            SyntaxKind::DeleteExpression
        );
        assert_eq!(
            source.arena.node(expression_statement_expression(3)).kind,
            SyntaxKind::TypeOfExpression
        );
        assert_eq!(
            source.arena.node(expression_statement_expression(4)).kind,
            SyntaxKind::VoidExpression
        );
        assert_eq!(
            source.arena.node(expression_statement_expression(5)).kind,
            SyntaxKind::AwaitExpression
        );

        let variable_initializer = |index: usize| -> NodeId {
            let variable_statement = source
                .arena
                .node(statements.nodes[index])
                .data
                .as_variable_statement()
                .expect("variable statement");
            let declaration_list = source
                .arena
                .node(
                    variable_statement
                        .declaration_list
                        .expect("declaration list"),
                )
                .data
                .as_variable_declaration_list()
                .expect("declaration list data");
            let declarations = source
                .arena
                .node_array(declaration_list.declarations.expect("declarations"));
            source
                .arena
                .node(declarations.nodes[0])
                .data
                .as_variable_declaration()
                .expect("declaration")
                .initializer
                .expect("initializer")
        };

        let generator = source
            .arena
            .node(variable_initializer(6))
            .data
            .as_function_expression()
            .expect("generator function expression");
        let generator_body = source
            .arena
            .node(generator.body.expect("generator body"))
            .data
            .as_block()
            .expect("generator body block");
        let generator_statements = source
            .arena
            .node_array(generator_body.statements.expect("generator statements"));
        assert_eq!(generator_statements.nodes.len(), 2);
        for statement in &generator_statements.nodes {
            let expression = source
                .arena
                .node(*statement)
                .data
                .as_expression_statement()
                .expect("yield expression statement")
                .expression
                .expect("yield expression");
            assert_eq!(
                source.arena.node(expression).kind,
                SyntaxKind::YieldExpression
            );
        }

        let async_function = source
            .arena
            .node(variable_initializer(7))
            .data
            .as_function_expression()
            .expect("async function expression");
        let async_body = source
            .arena
            .node(async_function.body.expect("async body"))
            .data
            .as_block()
            .expect("async body block");
        let async_statements = source
            .arena
            .node_array(async_body.statements.expect("async statements"));
        let await_expression = source
            .arena
            .node(async_statements.nodes[0])
            .data
            .as_expression_statement()
            .expect("await expression statement")
            .expression
            .expect("await expression");
        assert_eq!(
            source.arena.node(await_expression).kind,
            SyntaxKind::AwaitExpression
        );
    }

    fn expression_statements(source: &SourceFile) -> Vec<NodeId> {
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        statements
            .nodes
            .iter()
            .map(|&statement| {
                source
                    .arena
                    .node(statement)
                    .data
                    .as_expression_statement()
                    .expect("expression statement")
                    .expression
                    .expect("expression")
            })
            .collect()
    }

    fn binary_parts(source: &SourceFile, id: NodeId) -> (NodeId, SyntaxKind, NodeId) {
        let binary = source
            .arena
            .node(id)
            .data
            .as_binary_expression()
            .expect("binary expression");
        (
            binary.left.expect("left"),
            source
                .arena
                .node(binary.operator_token.expect("operator token"))
                .kind,
            binary.right.expect("right"),
        )
    }

    #[test]
    fn parse_binary_expression_precedence_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "1 + 2 * 3; 2 ** 3 ** 4; a >> b >>> c; x, y;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        assert_eq!(expressions.len(), 4);

        let (_, plus, multiply) = binary_parts(&source, expressions[0]);
        assert_eq!(plus, SyntaxKind::PlusToken);
        let (_, asterisk, _) = binary_parts(&source, multiply);
        assert_eq!(asterisk, SyntaxKind::AsteriskToken);

        let (base, outer_exponent, tower) = binary_parts(&source, expressions[1]);
        assert_eq!(outer_exponent, SyntaxKind::AsteriskAsteriskToken);
        assert_eq!(source.arena.node(base).kind, SyntaxKind::NumericLiteral);
        let (_, inner_exponent, _) = binary_parts(&source, tower);
        assert_eq!(inner_exponent, SyntaxKind::AsteriskAsteriskToken);

        let (shift, unsigned_shift, _) = binary_parts(&source, expressions[2]);
        assert_eq!(
            unsigned_shift,
            SyntaxKind::GreaterThanGreaterThanGreaterThanToken
        );
        let (_, signed_shift, _) = binary_parts(&source, shift);
        assert_eq!(signed_shift, SyntaxKind::GreaterThanGreaterThanToken);

        let (_, comma, _) = binary_parts(&source, expressions[3]);
        assert_eq!(comma, SyntaxKind::CommaToken);
    }

    #[test]
    fn parse_relational_chain_not_type_arguments() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "a < b > c;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        let (less, greater, _) = binary_parts(&source, expressions[0]);
        assert_eq!(greater, SyntaxKind::GreaterThanToken);
        let (_, less_operator, _) = binary_parts(&source, less);
        assert_eq!(less_operator, SyntaxKind::LessThanToken);
    }

    #[test]
    fn parse_as_satisfies_and_type_assertion() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "x as T; y satisfies U; <T>z;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        assert_eq!(
            source.arena.node(expressions[0]).kind,
            SyntaxKind::AsExpression
        );
        assert_eq!(
            source.arena.node(expressions[1]).kind,
            SyntaxKind::SatisfiesExpression
        );
        assert_eq!(
            source.arena.node(expressions[2]).kind,
            SyntaxKind::TypeAssertionExpression
        );
    }

    #[test]
    fn as_on_new_line_breaks_binary_loop() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "x\nas;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        assert_eq!(expressions.len(), 2);
        assert_eq!(
            source.arena.node(expressions[0]).kind,
            SyntaxKind::Identifier
        );
        assert_eq!(
            source.arena.node(expressions[1]).kind,
            SyntaxKind::Identifier
        );
    }

    #[test]
    fn unary_left_of_exponent_reports_17006_but_still_parses() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "-x ** 2;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert_eq!(source.parse_diagnostics.len(), 1);
        assert_eq!(source.parse_diagnostics[0].code(), 17006);
        let expressions = expression_statements(&source);
        let (negated, exponent, _) = binary_parts(&source, expressions[0]);
        assert_eq!(exponent, SyntaxKind::AsteriskAsteriskToken);
        assert_eq!(
            source.arena.node(negated).kind,
            SyntaxKind::PrefixUnaryExpression
        );
    }

    #[test]
    fn parse_assignment_right_associative_and_rescanned_operator() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "a = b = c; x >>= y;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);

        let (left, equals, chained) = binary_parts(&source, expressions[0]);
        assert_eq!(equals, SyntaxKind::EqualsToken);
        assert_eq!(source.arena.node(left).kind, SyntaxKind::Identifier);
        let (_, inner_equals, _) = binary_parts(&source, chained);
        assert_eq!(inner_equals, SyntaxKind::EqualsToken);

        let (_, shift_assign, _) = binary_parts(&source, expressions[1]);
        assert_eq!(shift_assign, SyntaxKind::GreaterThanGreaterThanEqualsToken);
    }

    #[test]
    fn assignment_to_non_lhs_leaves_equals_for_outer_context() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "a + b = c;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(!source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        let first = source
            .arena
            .node(statements.nodes[0])
            .data
            .as_expression_statement()
            .expect("expression statement")
            .expression
            .expect("expression");
        let (_, plus, _) = binary_parts(&source, first);
        assert_eq!(plus, SyntaxKind::PlusToken);
    }

    #[test]
    fn parse_conditional_expression_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "a ? b : c ? d : e;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        let conditional = source
            .arena
            .node(expressions[0])
            .data
            .as_conditional_expression()
            .expect("conditional expression");
        let when_false = conditional.when_false.expect("when false");
        assert_eq!(
            source.arena.node(when_false).kind,
            SyntaxKind::ConditionalExpression
        );
    }

    #[test]
    fn conditional_missing_colon_recovers_with_missing_when_false() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "a ? b;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(!source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        let conditional = source
            .arena
            .node(expressions[0])
            .data
            .as_conditional_expression()
            .expect("conditional expression");
        let colon = conditional.colon_token.expect("colon token");
        let colon_node = source.arena.node(colon);
        assert_eq!(colon_node.pos, colon_node.end);
        let when_false = conditional.when_false.expect("when false");
        let when_false_node = source.arena.node(when_false);
        assert_eq!(when_false_node.pos, when_false_node.end);
    }

    #[test]
    fn parse_arrow_function_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "x => x; (a, b) => a; () => 1; (...xs) => xs; (a) => { return a; };".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        assert_eq!(expressions.len(), 5);

        for &expression in &expressions {
            assert_eq!(
                source.arena.node(expression).kind,
                SyntaxKind::ArrowFunction
            );
        }

        let simple = source
            .arena
            .node(expressions[0])
            .data
            .as_arrow_function()
            .expect("simple arrow");
        let simple_parameters = source
            .arena
            .node_array(simple.parameters.expect("parameters"));
        assert_eq!(simple_parameters.nodes.len(), 1);
        assert!(simple.equals_greater_than_token.is_some());

        let two_parameters = source
            .arena
            .node(expressions[1])
            .data
            .as_arrow_function()
            .expect("two-parameter arrow");
        assert_eq!(
            source
                .arena
                .node_array(two_parameters.parameters.expect("parameters"))
                .nodes
                .len(),
            2
        );

        let rest = source
            .arena
            .node(expressions[3])
            .data
            .as_arrow_function()
            .expect("rest arrow");
        let rest_parameters = source
            .arena
            .node_array(rest.parameters.expect("parameters"));
        let rest_parameter = source
            .arena
            .node(rest_parameters.nodes[0])
            .data
            .as_parameter()
            .expect("rest parameter");
        assert!(rest_parameter.dot_dot_dot_token.is_some());

        let block_body = source
            .arena
            .node(expressions[4])
            .data
            .as_arrow_function()
            .expect("block-body arrow");
        assert_eq!(
            source.arena.node(block_body.body.expect("body")).kind,
            SyntaxKind::Block
        );
    }

    #[test]
    fn parse_async_arrow_and_line_break_asi() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "async x => x; async (a) => a; async\ny => y;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        assert_eq!(expressions.len(), 4);

        for &expression in &expressions[..2] {
            let arrow = source
                .arena
                .node(expression)
                .data
                .as_arrow_function()
                .expect("async arrow");
            assert!(arrow.modifiers.is_some());
        }

        assert_eq!(
            source.arena.node(expressions[2]).kind,
            SyntaxKind::Identifier
        );
        assert_eq!(
            source.arena.node(expressions[3]).kind,
            SyntaxKind::ArrowFunction
        );
    }

    #[test]
    fn parenthesized_expression_not_mistaken_for_arrow() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "(a, b); (a);".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        for &expression in &expressions {
            assert_eq!(
                source.arena.node(expression).kind,
                SyntaxKind::ParenthesizedExpression
            );
        }
    }

    #[test]
    fn conditional_when_true_rejects_arrow_return_type() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "a ? (b): c => d;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        let conditional = source
            .arena
            .node(expressions[0])
            .data
            .as_conditional_expression()
            .expect("conditional expression");
        assert_eq!(
            source
                .arena
                .node(conditional.when_true.expect("when true"))
                .kind,
            SyntaxKind::ParenthesizedExpression
        );
        assert_eq!(
            source
                .arena
                .node(conditional.when_false.expect("when false"))
                .kind,
            SyntaxKind::ArrowFunction
        );
    }

    #[test]
    fn function_expression_parses_real_parameters() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "(function (this: T, a, b = 1) { return a; });".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let expressions = expression_statements(&source);
        let parenthesized = source
            .arena
            .node(expressions[0])
            .data
            .as_parenthesized_expression()
            .expect("parenthesized expression");
        let function = source
            .arena
            .node(parenthesized.expression.expect("function expression"))
            .data
            .as_function_expression()
            .expect("function expression");
        let parameters = source
            .arena
            .node_array(function.parameters.expect("parameters"));
        assert_eq!(parameters.nodes.len(), 3);
        let default_parameter = source
            .arena
            .node(parameters.nodes[2])
            .data
            .as_parameter()
            .expect("defaulted parameter");
        assert!(default_parameter.initializer.is_some());
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

    fn variable_types(source: &SourceFile) -> Vec<NodeId> {
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));
        statements
            .nodes
            .iter()
            .map(|&statement| {
                let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
                    panic!("expected variable statement");
                };
                let list = source
                    .arena
                    .node(data.declaration_list.expect("declaration list"))
                    .data
                    .as_variable_declaration_list()
                    .expect("variable declaration list");
                let declarations = source
                    .arena
                    .node_array(list.declarations.expect("declarations"));
                source
                    .arena
                    .node(declarations.nodes[0])
                    .data
                    .as_variable_declaration()
                    .expect("variable declaration")
                    .r#type
                    .expect("type annotation")
            })
            .collect()
    }

    #[test]
    fn parse_type_reference_and_postfix_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let a: string; let b: Array<number>; let c: ns.Entity<string>[]; let d: A[\"k\"]; let e: string!; let f: ?string;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let types = variable_types(&source);
        assert_eq!(source.arena.node(types[0]).kind, SyntaxKind::StringKeyword);

        let NodeData::TypeReference(array_ref) = &source.arena.node(types[1]).data else {
            panic!("expected type reference");
        };
        assert!(array_ref.type_arguments.is_some());

        let NodeData::ArrayType(array) = &source.arena.node(types[2]).data else {
            panic!("expected array type");
        };
        let NodeData::TypeReference(qualified) =
            &source.arena.node(array.element_type.expect("element")).data
        else {
            panic!("expected type reference element");
        };
        assert_eq!(
            source
                .arena
                .node(qualified.type_name.expect("type name"))
                .kind,
            SyntaxKind::QualifiedName
        );

        assert_eq!(
            source.arena.node(types[3]).kind,
            SyntaxKind::IndexedAccessType
        );
        assert_eq!(
            source.arena.node(types[4]).kind,
            SyntaxKind::JSDocNonNullableType
        );
        assert_eq!(
            source.arena.node(types[5]).kind,
            SyntaxKind::JSDocNullableType
        );
    }

    #[test]
    fn parse_union_intersection_and_type_operators() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let a: A | B & C; let b: keyof A; let c: readonly string[]; let d: unique symbol;"
                .to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let types = variable_types(&source);
        let NodeData::UnionType(union) = &source.arena.node(types[0]).data else {
            panic!("expected union type");
        };
        let members = source.arena.node_array(union.types.expect("types"));
        assert_eq!(members.nodes.len(), 2);
        assert_eq!(
            source.arena.node(members.nodes[1]).kind,
            SyntaxKind::IntersectionType
        );
        assert_eq!(source.arena.node(types[1]).kind, SyntaxKind::TypeOperator);
        let NodeData::TypeOperator(readonly_array) = &source.arena.node(types[2]).data else {
            panic!("expected type operator");
        };
        assert_eq!(
            source
                .arena
                .node(readonly_array.r#type.expect("operand"))
                .kind,
            SyntaxKind::ArrayType
        );
        assert_eq!(source.arena.node(types[3]).kind, SyntaxKind::TypeOperator);
    }

    #[test]
    fn parse_object_type_member_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let o: { a: string; readonly b?: number, m<T>(x: T): T; (x: number): void; new (): any; [k: string]: any; get p(): number; set p(v); };".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let types = variable_types(&source);
        let NodeData::TypeLiteral(literal) = &source.arena.node(types[0]).data else {
            panic!("expected type literal");
        };
        let members = source.arena.node_array(literal.members.expect("members"));
        let kinds: Vec<SyntaxKind> = members
            .nodes
            .iter()
            .map(|&member| source.arena.node(member).kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::PropertySignature,
                SyntaxKind::PropertySignature,
                SyntaxKind::MethodSignature,
                SyntaxKind::CallSignature,
                SyntaxKind::ConstructSignature,
                SyntaxKind::IndexSignature,
                SyntaxKind::GetAccessor,
                SyntaxKind::SetAccessor,
            ]
        );
        let NodeData::PropertySignature(readonly_property) =
            &source.arena.node(members.nodes[1]).data
        else {
            panic!("expected property signature");
        };
        assert!(readonly_property.modifiers.is_some());
        assert!(readonly_property.question_token.is_some());
    }

    #[test]
    fn parse_tuple_function_and_constructor_types() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let t: [string, number?, ...boolean[], name: string]; let f: (a: string) => void; let g: new () => any; let h: abstract new () => any;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let types = variable_types(&source);
        let NodeData::TupleType(tuple) = &source.arena.node(types[0]).data else {
            panic!("expected tuple type");
        };
        let elements = source.arena.node_array(tuple.elements.expect("elements"));
        let kinds: Vec<SyntaxKind> = elements
            .nodes
            .iter()
            .map(|&element| source.arena.node(element).kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::StringKeyword,
                SyntaxKind::OptionalType,
                SyntaxKind::RestType,
                SyntaxKind::NamedTupleMember,
            ]
        );
        assert_eq!(source.arena.node(types[1]).kind, SyntaxKind::FunctionType);
        assert_eq!(
            source.arena.node(types[2]).kind,
            SyntaxKind::ConstructorType
        );
        let NodeData::ConstructorType(abstract_ctor) = &source.arena.node(types[3]).data else {
            panic!("expected constructor type");
        };
        assert!(abstract_ctor.modifiers.is_some());
    }

    #[test]
    fn parse_conditional_infer_typeof_and_import_types() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let a: T extends U ? V : W; let b: T extends infer U extends X ? U : never; let c: typeof ns.entity; let d: import(\"m\").T<U>; let e: typeof import(\"m\");".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let types = variable_types(&source);
        assert_eq!(
            source.arena.node(types[0]).kind,
            SyntaxKind::ConditionalType
        );
        let NodeData::ConditionalType(conditional) = &source.arena.node(types[1]).data else {
            panic!("expected conditional type");
        };
        let NodeData::InferType(infer) = &source
            .arena
            .node(conditional.extends_type.expect("extends type"))
            .data
        else {
            panic!("expected infer type");
        };
        let NodeData::TypeParameter(infer_parameter) = &source
            .arena
            .node(infer.type_parameter.expect("type parameter"))
            .data
        else {
            panic!("expected type parameter");
        };
        assert!(infer_parameter.constraint.is_some());

        let NodeData::TypeQuery(query) = &source.arena.node(types[2]).data else {
            panic!("expected type query");
        };
        assert_eq!(
            source.arena.node(query.expr_name.expect("expr name")).kind,
            SyntaxKind::QualifiedName
        );

        let NodeData::ImportType(import_type) = &source.arena.node(types[3]).data else {
            panic!("expected import type");
        };
        assert!(import_type.qualifier.is_some());
        assert!(import_type.type_arguments.is_some());
        assert_eq!(source.arena.node(types[4]).kind, SyntaxKind::ImportType);
    }

    #[test]
    fn parse_mapped_and_template_literal_types() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let m: { readonly [K in keyof T as `get${K}`]?: T[K]; }; let t: `a${T}b`;".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let types = variable_types(&source);
        let NodeData::MappedType(mapped) = &source.arena.node(types[0]).data else {
            panic!("expected mapped type");
        };
        assert!(mapped.readonly_token.is_some());
        assert!(mapped.name_type.is_some());
        assert!(mapped.question_token.is_some());
        assert_eq!(
            source.arena.node(mapped.r#type.expect("type")).kind,
            SyntaxKind::IndexedAccessType
        );

        let NodeData::TemplateLiteralType(template) = &source.arena.node(types[1]).data else {
            panic!("expected template literal type");
        };
        let spans = source
            .arena
            .node_array(template.template_spans.expect("spans"));
        assert_eq!(spans.nodes.len(), 1);
        assert_eq!(
            source.arena.node(spans.nodes[0]).kind,
            SyntaxKind::TemplateLiteralTypeSpan
        );
    }

    #[test]
    fn parse_type_predicates_in_return_types() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "const f = function (x): x is string { return true; }; const g = function (x): asserts x is string {}; let h: { isC(): this is C; };".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));

        let function_return_type = |statement: NodeId| {
            let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
                panic!("expected variable statement");
            };
            let list = source
                .arena
                .node(data.declaration_list.expect("list"))
                .data
                .as_variable_declaration_list()
                .expect("declaration list");
            let declarations = source
                .arena
                .node_array(list.declarations.expect("declarations"));
            let declaration = source
                .arena
                .node(declarations.nodes[0])
                .data
                .as_variable_declaration()
                .expect("variable declaration");
            let NodeData::FunctionExpression(function) = &source
                .arena
                .node(declaration.initializer.expect("initializer"))
                .data
            else {
                panic!("expected function expression");
            };
            function.r#type.expect("return type")
        };

        let predicate = function_return_type(statements.nodes[0]);
        let NodeData::TypePredicate(data) = &source.arena.node(predicate).data else {
            panic!("expected type predicate");
        };
        assert!(data.asserts_modifier.is_none());

        let asserts_predicate = function_return_type(statements.nodes[1]);
        let NodeData::TypePredicate(asserts_data) = &source.arena.node(asserts_predicate).data
        else {
            panic!("expected asserts predicate");
        };
        assert!(asserts_data.asserts_modifier.is_some());

        let NodeData::VariableStatement(h_statement) = &source.arena.node(statements.nodes[2]).data
        else {
            panic!("expected variable statement");
        };
        let h_list = source
            .arena
            .node(h_statement.declaration_list.expect("list"))
            .data
            .as_variable_declaration_list()
            .expect("declaration list");
        let h_declarations = source
            .arena
            .node_array(h_list.declarations.expect("declarations"));
        let h_type = source
            .arena
            .node(h_declarations.nodes[0])
            .data
            .as_variable_declaration()
            .expect("variable declaration")
            .r#type
            .expect("type annotation");
        let NodeData::TypeLiteral(literal) = &source.arena.node(h_type).data else {
            panic!("expected type literal");
        };
        let members = source.arena.node_array(literal.members.expect("members"));
        let NodeData::MethodSignature(method) = &source.arena.node(members.nodes[0]).data else {
            panic!("expected method signature");
        };
        let NodeData::TypePredicate(this_predicate) =
            &source.arena.node(method.r#type.expect("return type")).data
        else {
            panic!("expected this predicate");
        };
        assert_eq!(
            source
                .arena
                .node(this_predicate.parameter_name.expect("parameter name"))
                .kind,
            SyntaxKind::ThisType
        );
    }

    #[test]
    fn parse_generic_arrow_type_assertion_and_object_accessors() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "const f = <T>(x: T): T => x; const v = <Foo<string>>bar; const o = { get x() { return 1; }, set x(v) {}, async m<T>(a: T) { return a; } };".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(source.parse_diagnostics.is_empty());
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        let statements = source
            .arena
            .node_array(root.statements.expect("statements"));

        let initializer = |statement: NodeId| {
            let NodeData::VariableStatement(data) = &source.arena.node(statement).data else {
                panic!("expected variable statement");
            };
            let list = source
                .arena
                .node(data.declaration_list.expect("list"))
                .data
                .as_variable_declaration_list()
                .expect("declaration list");
            let declarations = source
                .arena
                .node_array(list.declarations.expect("declarations"));
            source
                .arena
                .node(declarations.nodes[0])
                .data
                .as_variable_declaration()
                .expect("variable declaration")
                .initializer
                .expect("initializer")
        };

        let NodeData::ArrowFunction(arrow) =
            &source.arena.node(initializer(statements.nodes[0])).data
        else {
            panic!("expected arrow function");
        };
        assert!(arrow.type_parameters.is_some());
        assert!(arrow.r#type.is_some());

        let NodeData::TypeAssertionExpression(assertion) =
            &source.arena.node(initializer(statements.nodes[1])).data
        else {
            panic!("expected type assertion");
        };
        let NodeData::TypeReference(assertion_type) = &source
            .arena
            .node(assertion.r#type.expect("assertion type"))
            .data
        else {
            panic!("expected type reference");
        };
        assert!(assertion_type.type_arguments.is_some());

        let NodeData::ObjectLiteralExpression(object) =
            &source.arena.node(initializer(statements.nodes[2])).data
        else {
            panic!("expected object literal");
        };
        let properties = source
            .arena
            .node_array(object.properties.expect("properties"));
        let kinds: Vec<SyntaxKind> = properties
            .nodes
            .iter()
            .map(|&property| source.arena.node(property).kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                SyntaxKind::GetAccessor,
                SyntaxKind::SetAccessor,
                SyntaxKind::MethodDeclaration,
            ]
        );
        let NodeData::MethodDeclaration(method) = &source.arena.node(properties.nodes[2]).data
        else {
            panic!("expected method declaration");
        };
        assert!(method.modifiers.is_some());
        assert!(method.type_parameters.is_some());
    }

    #[test]
    fn union_function_type_error_and_type_expected_recovery() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "let x: A | () => void; let y: ;".to_owned(),
            ParseOptions::default(),
            None,
        );

        let codes: Vec<u32> = source
            .parse_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        assert_eq!(
            codes,
            vec![
                gen::Function_type_notation_must_be_parenthesized_when_used_in_a_union_type.code,
                gen::Type_expected.code,
            ]
        );

        let types = variable_types(&source);
        let NodeData::UnionType(union) = &source.arena.node(types[0]).data else {
            panic!("expected union type");
        };
        let members = source.arena.node_array(union.types.expect("types"));
        assert_eq!(
            source.arena.node(members.nodes[1]).kind,
            SyntaxKind::FunctionType
        );
    }

    fn statement_kinds(source: &SourceFile) -> Vec<SyntaxKind> {
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        source
            .arena
            .node_array(root.statements.expect("statements"))
            .nodes
            .iter()
            .map(|&statement| source.arena.node(statement).kind)
            .collect()
    }

    fn statement_nodes(source: &SourceFile) -> Vec<NodeId> {
        let root = source
            .arena
            .node(source.root)
            .data
            .as_source_file()
            .expect("source file root");
        source
            .arena
            .node_array(root.statements.expect("statements"))
            .nodes
            .clone()
    }

    #[test]
    fn parse_class_declaration_shapes() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "@dec export abstract class C<T> extends B<T> implements I, J {\n  constructor(private readonly x: number) { super(); }\n  static { C.count = 0; }\n  #secret = 1;\n  declare readonly f: string;\n  get p(): number { return 1; }\n  set p(v) {}\n  static async *m<U>(u: U): Promise<U> { return u; }\n  [k: string]: any;\n  ;\n}".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        let statements = statement_nodes(&source);
        let NodeData::ClassDeclaration(class) = &source.arena.node(statements[0]).data else {
            panic!("expected class declaration");
        };
        assert!(class.modifiers.is_some());
        assert!(class.type_parameters.is_some());
        let heritage = source
            .arena
            .node_array(class.heritage_clauses.expect("heritage clauses"));
        assert_eq!(heritage.nodes.len(), 2);
        let modifier_kinds: Vec<SyntaxKind> = source
            .arena
            .node_array(class.modifiers.expect("modifiers"))
            .nodes
            .iter()
            .map(|&modifier| source.arena.node(modifier).kind)
            .collect();
        assert_eq!(
            modifier_kinds,
            vec![
                SyntaxKind::Decorator,
                SyntaxKind::ExportKeyword,
                SyntaxKind::AbstractKeyword,
            ]
        );
        let member_kinds: Vec<SyntaxKind> = source
            .arena
            .node_array(class.members.expect("members"))
            .nodes
            .iter()
            .map(|&member| source.arena.node(member).kind)
            .collect();
        assert_eq!(
            member_kinds,
            vec![
                SyntaxKind::Constructor,
                SyntaxKind::ClassStaticBlockDeclaration,
                SyntaxKind::PropertyDeclaration,
                SyntaxKind::PropertyDeclaration,
                SyntaxKind::GetAccessor,
                SyntaxKind::SetAccessor,
                SyntaxKind::MethodDeclaration,
                SyntaxKind::IndexSignature,
                SyntaxKind::SemicolonClassElement,
            ]
        );
    }

    #[test]
    fn parse_interface_type_alias_and_enum() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "interface I<T> extends A, B<T> { a: string; }\ntype Alias<T> = T | null;\ntype Str = intrinsic;\nconst enum E { A, B = 2, \"c\" = 3 }".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        assert_eq!(
            statement_kinds(&source),
            vec![
                SyntaxKind::InterfaceDeclaration,
                SyntaxKind::TypeAliasDeclaration,
                SyntaxKind::TypeAliasDeclaration,
                SyntaxKind::EnumDeclaration,
            ]
        );
        let statements = statement_nodes(&source);
        let NodeData::TypeAliasDeclaration(intrinsic_alias) =
            &source.arena.node(statements[2]).data
        else {
            panic!("expected type alias");
        };
        assert_eq!(
            source
                .arena
                .node(intrinsic_alias.r#type.expect("type"))
                .kind,
            SyntaxKind::IntrinsicKeyword
        );
        let NodeData::EnumDeclaration(enum_declaration) = &source.arena.node(statements[3]).data
        else {
            panic!("expected enum declaration");
        };
        assert!(enum_declaration.modifiers.is_some());
        assert_eq!(
            source
                .arena
                .node_array(enum_declaration.members.expect("members"))
                .nodes
                .len(),
            3
        );
    }

    #[test]
    fn parse_namespace_and_ambient_modules() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "namespace a.b { export const x = 1; }\ndeclare module \"m\" { let y: number; }\ndeclare global { interface Window {} }\nmodule Simple { }".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        let statements = statement_nodes(&source);
        assert_eq!(
            statement_kinds(&source),
            vec![
                SyntaxKind::ModuleDeclaration,
                SyntaxKind::ModuleDeclaration,
                SyntaxKind::ModuleDeclaration,
                SyntaxKind::ModuleDeclaration,
            ]
        );
        // namespace a.b desugars into a nested module declaration.
        let NodeData::ModuleDeclaration(outer) = &source.arena.node(statements[0]).data else {
            panic!("expected module declaration");
        };
        let body = outer.body.expect("body");
        assert_eq!(source.arena.node(body).kind, SyntaxKind::ModuleDeclaration);
        assert!(NodeFlags::from_bits(source.arena.node(body).flags)
            .contains(NodeFlags::NESTED_NAMESPACE));
        let NodeData::ModuleDeclaration(global) = &source.arena.node(statements[2]).data else {
            panic!("expected global augmentation");
        };
        assert!(NodeFlags::from_bits(source.arena.node(statements[2]).flags)
            .contains(NodeFlags::GLOBAL_AUGMENTATION));
        assert!(global.body.is_some());
    }

    #[test]
    fn parse_import_and_export_forms() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "import d, { e as f, type g } from \"m\";\nimport * as ns from \"m\";\nimport type { A } from \"m\";\nimport eq = require(\"m\");\nexport * as everything from \"m\";\nexport { a as b };\nexport default 42;\nexport = eq;\nexport as namespace NS;\nimport \"side-effect\";".to_owned(),
            ParseOptions::default(),
            None,
        );

        assert!(
            source.parse_diagnostics.is_empty(),
            "{:?}",
            source.parse_diagnostics
        );
        assert_eq!(
            statement_kinds(&source),
            vec![
                SyntaxKind::ImportDeclaration,
                SyntaxKind::ImportDeclaration,
                SyntaxKind::ImportDeclaration,
                SyntaxKind::ImportEqualsDeclaration,
                SyntaxKind::ExportDeclaration,
                SyntaxKind::ExportDeclaration,
                SyntaxKind::ExportAssignment,
                SyntaxKind::ExportAssignment,
                SyntaxKind::NamespaceExportDeclaration,
                SyntaxKind::ImportDeclaration,
            ]
        );
        let statements = statement_nodes(&source);
        let NodeData::ImportDeclaration(first) = &source.arena.node(statements[0]).data else {
            panic!("expected import declaration");
        };
        let NodeData::ImportClause(clause) = &source
            .arena
            .node(first.import_clause.expect("import clause"))
            .data
        else {
            panic!("expected import clause");
        };
        assert!(clause.name.is_some());
        let NodeData::NamedImports(named) = &source
            .arena
            .node(clause.named_bindings.expect("named bindings"))
            .data
        else {
            panic!("expected named imports");
        };
        assert_eq!(
            source
                .arena
                .node_array(named.elements.expect("elements"))
                .nodes
                .len(),
            2
        );
        let NodeData::ImportEqualsDeclaration(equals) = &source.arena.node(statements[3]).data
        else {
            panic!("expected import equals");
        };
        assert_eq!(
            source
                .arena
                .node(equals.module_reference.expect("module reference"))
                .kind,
            SyntaxKind::ExternalModuleReference
        );
        let NodeData::ImportDeclaration(side_effect) = &source.arena.node(statements[9]).data
        else {
            panic!("expected side-effect import");
        };
        assert!(side_effect.import_clause.is_none());
    }

    #[test]
    fn missing_semicolon_reports_spelling_suggestions() {
        let source = parse_source_file(
            "a.ts".to_owned(),
            "interfaz Foo {}\nvar x = 1;\nnamespacefoo Bar {}".to_owned(),
            ParseOptions::default(),
            None,
        );

        let codes: Vec<u32> = source
            .parse_diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code())
            .collect();
        // `interfaz` levenshteins to `interface`; `namespacefoo` splits into
        // `namespace foo` via the space suggestion.
        assert!(
            codes
                .iter()
                .filter(|&&code| code == gen::Unknown_keyword_or_identifier_Did_you_mean_0.code)
                .count()
                >= 2,
            "{:?}",
            source.parse_diagnostics
        );
    }
}
