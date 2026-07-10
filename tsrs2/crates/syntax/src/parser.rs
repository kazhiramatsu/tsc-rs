#![allow(dead_code)]

use crate::arena::NodeArena;
use crate::nodes::{
    ArrayBindingPatternData, ArrayLiteralExpressionData, ArrayTypeData, ArrowFunctionData,
    AsExpressionData, AwaitExpressionData, BigIntLiteralData, BinaryExpressionData,
    BindingElementData, BlockData, BreakStatementData, CallExpressionData, CallSignatureData,
    CaseBlockData, CaseClauseData, CatchClauseData, ClassExpressionData, ComputedPropertyNameData,
    ConditionalExpressionData, ConditionalTypeData, ConstructSignatureData, ConstructorTypeData,
    ContinueStatementData, DebuggerStatementData, DefaultClauseData, DeleteExpressionData,
    DoStatementData, ElementAccessExpressionData, EmptyStatementData, ExpressionStatementData,
    ExpressionWithTypeArgumentsData, ForInStatementData, ForOfStatementData, ForStatementData,
    FunctionExpressionData, FunctionTypeData, GetAccessorData, IdentifierData, IfStatementData,
    ImportAttributeData, ImportAttributesData, ImportTypeData, IndexSignatureData,
    IndexedAccessTypeData, InferTypeData, IntersectionTypeData, JSDocFunctionTypeData,
    JSDocNonNullableTypeData, JSDocNullableTypeData, JSDocOptionalTypeData, JSDocVariadicTypeData,
    LabeledStatementData, LiteralTypeData, MappedTypeData, MetaPropertyData, MethodDeclarationData,
    MethodSignatureData, MissingDeclarationData, NamedTupleMemberData, NewExpressionData,
    NoSubstitutionTemplateLiteralData, NodeData, NodeId, NodePayload, NonNullExpressionData,
    NumericLiteralData, ObjectBindingPatternData, ObjectLiteralExpressionData,
    OmittedExpressionData, OptionalTypeData, ParameterData, ParenthesizedExpressionData,
    ParenthesizedTypeData, PostfixUnaryExpressionData, PrefixUnaryExpressionData,
    PrivateIdentifierData, PropertyAccessExpressionData, PropertyAssignmentData,
    PropertySignatureData, QualifiedNameData, RegularExpressionLiteralData, RestTypeData,
    ReturnStatementData, SatisfiesExpressionData, SetAccessorData, ShorthandPropertyAssignmentData,
    SourceFileData, SpreadAssignmentData, SpreadElementData, StringLiteralData,
    SwitchStatementData, TaggedTemplateExpressionData, TemplateExpressionData, TemplateHeadData,
    TemplateLiteralTypeData, TemplateLiteralTypeSpanData, TemplateMiddleData, TemplateSpanData,
    TemplateTailData, ThrowStatementData, TryStatementData, TupleTypeData,
    TypeAssertionExpressionData, TypeLiteralData, TypeOfExpressionData, TypeOperatorData,
    TypeParameterData, TypePredicateData, TypeQueryData, TypeReferenceData, UnionTypeData,
    VariableDeclarationData, VariableDeclarationListData, VariableStatementData,
    VoidExpressionData, WhileStatementData, WithStatementData, YieldExpressionData,
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
        ) || is_binary_operator(self.token())
            || self.is_identifier()
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
            SyntaxKind::VarKeyword | SyntaxKind::ConstKeyword => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::LetKeyword if self.is_let_declaration() => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::AwaitKeyword if self.is_await_using_declaration() => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::UsingKeyword if self.is_using_declaration() => {
                self.parse_variable_statement(self.node_pos(), None)
            }
            SyntaxKind::FunctionKeyword | SyntaxKind::ClassKeyword => {
                self.parse_unported_declaration_statement(self.node_pos(), None)
            }
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
            SyntaxKind::AtToken
            | SyntaxKind::AsyncKeyword
            | SyntaxKind::InterfaceKeyword
            | SyntaxKind::TypeKeyword
            | SyntaxKind::ModuleKeyword
            | SyntaxKind::NamespaceKeyword
            | SyntaxKind::DeclareKeyword
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
                self.parse_unported_declaration_statement(self.node_pos(), None)
            }
            _ => self.parse_expression_or_labeled_statement(),
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
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Expression_expected),
                &[],
            )
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
        self.parse_semicolon();
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

    fn parse_array_binding_element(&mut self) -> NodeId {
        if self.token() == SyntaxKind::CommaToken {
            let pos = self.node_pos();
            return self
                .finish_node_data(NodeData::OmittedExpression(OmittedExpressionData {}), pos);
        }
        self.parse_binding_element()
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

    fn parse_object_binding_element(&mut self) -> NodeId {
        self.parse_binding_element()
    }

    fn parse_binding_element(&mut self) -> NodeId {
        let pos = self.node_pos();
        let dot_dot_dot_token = self.parse_optional_token(SyntaxKind::DotDotDotToken);
        let first_name = self.parse_binding_identifier();
        let (property_name, name) = if self.parse_optional(SyntaxKind::ColonToken) {
            (Some(first_name), self.parse_identifier_or_pattern())
        } else {
            (None, first_name)
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

    fn parse_binding_identifier(&mut self) -> NodeId {
        if self.token() == SyntaxKind::PrivateIdentifier {
            return self.parse_private_identifier();
        }
        if self.is_binding_identifier() {
            self.parse_identifier()
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Identifier_expected),
                &[],
            )
        }
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
        let colon_token = self.parse_expected_token(SyntaxKind::ColonToken);
        let when_false = if self.node_is_present(colon_token) {
            self.parse_assignment_expression_or_higher(allow_return_type_in_arrow_function)
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
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

    /// tsc parseModifiers. Decorators (`allow_decorators`) land with 2.6.
    fn parse_modifiers(
        &mut self,
        _allow_decorators: bool,
        permit_const_as_modifier: bool,
        stop_on_start_of_class_static_block: bool,
    ) -> Option<crate::NodeArrayId> {
        let pos = self.node_pos();
        let mut list = Vec::new();
        let mut has_seen_static_modifier = false;
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
            self.parse_expected_token(SyntaxKind::EqualsGreaterThanToken);
        let body = if matches!(
            last_token,
            SyntaxKind::EqualsGreaterThanToken | SyntaxKind::OpenBraceToken
        ) {
            self.parse_arrow_function_expression_body(is_async, allow_return_type_in_arrow_function)
        } else if self.is_identifier() {
            self.parse_identifier()
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Identifier_expected),
                &[],
            )
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
            self.parse_expected_token(SyntaxKind::EqualsGreaterThanToken);
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
            SyntaxKind::LessThanToken if self.language_variant != LanguageVariant::Jsx => {
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

    fn parse_left_hand_side_expression_or_higher(&mut self) -> NodeId {
        let pos = self.node_pos();
        let expression = self.parse_member_expression_or_higher();
        self.parse_call_expression_rest(pos, expression)
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
                self.parse_no_substitution_template_literal()
            }
            SyntaxKind::FunctionKeyword => self.parse_function_expression_stub(None, false),
            SyntaxKind::ClassKeyword => self.parse_class_expression_stub(),
            SyntaxKind::NewKeyword => self.parse_new_expression_stub(),
            SyntaxKind::SlashToken | SyntaxKind::SlashEqualsToken => {
                if self.scanner.re_scan_slash_token(false) == SyntaxKind::RegularExpressionLiteral {
                    self.drain_scanner_errors();
                    self.parse_regular_expression_literal()
                } else {
                    self.create_missing_node(
                        SyntaxKind::Identifier,
                        true,
                        Some(&gen::Expression_expected),
                        &[],
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
                let pos = self.node_pos();
                self.next_token();
                self.parse_function_expression_stub(Some(pos), true)
            }
            kind if token_is_identifier_or_keyword(kind) => self.parse_identifier(),
            _ => self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Expression_expected),
                &[],
            ),
        }
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

            if self.parse_optional(SyntaxKind::OpenBracketToken) {
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
        let name = self.parse_right_side_of_dot(true, true);
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
                    || is_binary_operator(self.token())
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

    /// tsc parseRightSideOfDot; the allowUnicodeEscapeSequenceInIdentifierName
    /// error is not ported (scanner does not surface escape flags yet).
    fn parse_right_side_of_dot(
        &mut self,
        allow_identifier_names: bool,
        allow_private_identifiers: bool,
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
            return self.parse_identifier_name(None);
        }
        self.parse_identifier_or_missing()
    }

    /// tsc parseIdentifier: an identifier, or a missing node with an error.
    fn parse_identifier_or_missing(&mut self) -> NodeId {
        if self.is_identifier() {
            self.parse_identifier()
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Identifier_expected),
                &[],
            )
        }
    }

    /// tsc parseIdentifierName: reserved words allowed.
    fn parse_identifier_name(&mut self, message: Option<&'static DiagnosticMessage>) -> NodeId {
        if token_is_identifier_or_keyword(self.token()) {
            self.parse_identifier()
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                message.or(Some(&gen::Identifier_expected)),
                &[],
            )
        }
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
        } else if self.is_identifier() {
            self.parse_identifier()
        } else {
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                diagnostic.or(Some(&gen::Identifier_expected)),
                &[],
            )
        };
        while self.parse_optional(SyntaxKind::DotToken) {
            if self.token() == SyntaxKind::LessThanToken {
                // The entity is followed by type arguments; the caller
                // (parseTypeReference et al.) picks them up.
                break;
            }
            let right = self.parse_right_side_of_dot(allow_reserved_words, false);
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
            self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Expression_expected),
                &[],
            )
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
            Some(self.create_missing_node(
                SyntaxKind::Identifier,
                true,
                Some(&gen::Expression_expected),
                &[],
            ))
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
            _ => self.parse_identifier(),
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

    /// tsc parseMethodDeclaration.
    fn parse_method_declaration(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
        asterisk_token: Option<NodeId>,
        name: NodeId,
        question_token: Option<NodeId>,
        exclamation_token: Option<NodeId>,
    ) -> NodeId {
        let is_generator = asterisk_token.is_some();
        let is_async = self.modifiers_contain(modifiers, SyntaxKind::AsyncKeyword);
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(is_generator, is_async);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        let body = self.parse_function_block_or_semicolon(is_generator, is_async, false);
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
        _in_outer_await_context: bool,
        allow_ambiguity: bool,
    ) -> Option<NodeId> {
        let pos = self.node_pos();
        // parseModifiers (accessibility/decorators) lands with stage 2.6;
        // the in/out-of-await-context wrap only affects modifier parsing.
        let modifiers: Option<crate::NodeArrayId> = None;

        if self.token() == SyntaxKind::ThisKeyword {
            let name = self.parse_identifier();
            let r#type = self.parse_type_annotation();
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

    fn parse_function_expression_stub(
        &mut self,
        forced_pos: Option<usize>,
        is_async: bool,
    ) -> NodeId {
        let pos = forced_pos.unwrap_or_else(|| self.node_pos());
        self.parse_expected(SyntaxKind::FunctionKeyword, None);
        let asterisk_token = self.parse_optional_token(SyntaxKind::AsteriskToken);
        let is_generator = asterisk_token.is_some();
        let name = if self.is_binding_identifier() {
            Some(self.parse_binding_identifier())
        } else {
            None
        };
        let type_parameters = self.parse_type_parameters();
        let parameters = self.parse_parameters(is_generator, is_async);
        let r#type = self.parse_return_type(SyntaxKind::ColonToken, false);
        let body = Some(self.parse_function_block(is_generator, is_async, false, None));
        self.finish_node_data(
            NodeData::FunctionExpression(FunctionExpressionData {
                modifiers: None,
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

    fn parse_class_expression_stub(&mut self) -> NodeId {
        let pos = self.node_pos();
        self.parse_expected(SyntaxKind::ClassKeyword, None);
        let name = if self.is_binding_identifier() {
            Some(self.parse_binding_identifier())
        } else {
            None
        };
        while !matches!(
            self.token(),
            SyntaxKind::OpenBraceToken | SyntaxKind::EndOfFileToken
        ) {
            self.next_token();
        }
        let members_pos = self.node_pos();
        if self.parse_optional(SyntaxKind::OpenBraceToken) {
            self.skip_balanced_until(SyntaxKind::CloseBraceToken);
            self.parse_expected(SyntaxKind::CloseBraceToken, None);
        }
        let members = self.arena.empty_array(members_pos);
        self.finish_node_data(
            NodeData::ClassExpression(ClassExpressionData {
                modifiers: None,
                name,
                type_parameters: None,
                heritage_clauses: None,
                members: Some(members),
            }),
            pos,
        )
    }

    fn parse_template_expression(&mut self, is_tagged_template: bool) -> NodeId {
        let pos = self.node_pos();
        let head = self.parse_template_head();
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
                true,
                Some(&gen::_0_expected),
                &["}"],
            )
        }
    }

    fn parse_template_head(&mut self) -> NodeId {
        self.parse_template_fragment(SyntaxKind::TemplateHead)
    }

    fn parse_template_middle_or_tail(&mut self) -> NodeId {
        match self.token() {
            SyntaxKind::TemplateMiddle => self.parse_template_fragment(SyntaxKind::TemplateMiddle),
            SyntaxKind::TemplateTail => self.parse_template_fragment(SyntaxKind::TemplateTail),
            _ => self.create_missing_node(
                SyntaxKind::TemplateTail,
                true,
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
        let head = self.parse_template_head();
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
        let body = self.parse_function_block_or_semicolon(false, false, in_type_member);
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
        Some(self.parse_function_block(is_generator, is_async, false, None))
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
            attributes = Some(self.parse_import_attributes_body());
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

    /// tsc parseImportAttributes with skipKeyword=true.
    fn parse_import_attributes_body(&mut self) -> NodeId {
        let pos = self.node_pos();
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
        let asserts_modifier = self.parse_expected_token(SyntaxKind::AssertsKeyword);
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

    fn try_parse_semicolon(&mut self) -> bool {
        if self.parse_optional(SyntaxKind::SemicolonToken) {
            true
        } else {
            self.can_parse_semicolon()
        }
    }

    fn parse_expected_token(&mut self, kind: SyntaxKind) -> NodeId {
        match self.parse_optional_token(kind) {
            Some(token) => token,
            None => self.create_missing_node(kind, true, None, &[]),
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

    fn parse_unported_declaration_statement(
        &mut self,
        pos: usize,
        modifiers: Option<crate::NodeArrayId>,
    ) -> NodeId {
        self.skip_unported_declaration();
        self.finish_node_data(
            NodeData::MissingDeclaration(MissingDeclarationData { modifiers }),
            pos,
        )
    }

    fn skip_unported_declaration(&mut self) {
        if self.token() == SyntaxKind::ImportKeyword {
            self.skip_unported_import_declaration();
            return;
        }

        let mut brace_depth = 0usize;
        let mut paren_depth = 0usize;
        let mut bracket_depth = 0usize;
        loop {
            match self.token() {
                SyntaxKind::EndOfFileToken => break,
                SyntaxKind::SemicolonToken
                    if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 =>
                {
                    self.next_token();
                    break;
                }
                SyntaxKind::OpenParenToken => {
                    paren_depth += 1;
                    self.next_token();
                }
                SyntaxKind::CloseParenToken if paren_depth > 0 => {
                    paren_depth -= 1;
                    self.next_token();
                }
                SyntaxKind::OpenBracketToken => {
                    bracket_depth += 1;
                    self.next_token();
                }
                SyntaxKind::CloseBracketToken if bracket_depth > 0 => {
                    bracket_depth -= 1;
                    self.next_token();
                }
                SyntaxKind::OpenBraceToken => {
                    brace_depth += 1;
                    self.next_token();
                }
                SyntaxKind::CloseBraceToken => {
                    if brace_depth == 0 {
                        break;
                    }
                    brace_depth -= 1;
                    self.next_token();
                    if brace_depth == 0 && paren_depth == 0 && bracket_depth == 0 {
                        break;
                    }
                }
                _ => {
                    self.next_token();
                }
            }
        }
    }

    fn skip_unported_import_declaration(&mut self) {
        loop {
            match self.token() {
                SyntaxKind::EndOfFileToken => break,
                SyntaxKind::SemicolonToken => {
                    self.next_token();
                    break;
                }
                _ => {
                    self.next_token();
                }
            }
        }
    }

    fn skip_postfix_expression_rest(&mut self) {
        loop {
            match self.token() {
                SyntaxKind::DotToken | SyntaxKind::QuestionDotToken => {
                    self.next_token();
                    if self.token() == SyntaxKind::OpenParenToken {
                        self.next_token();
                        self.skip_balanced_until(SyntaxKind::CloseParenToken);
                        self.parse_expected(SyntaxKind::CloseParenToken, None);
                    } else if self.token() == SyntaxKind::OpenBracketToken {
                        self.next_token();
                        self.skip_balanced_until(SyntaxKind::CloseBracketToken);
                        self.parse_expected(SyntaxKind::CloseBracketToken, None);
                    } else if self.token() != SyntaxKind::EndOfFileToken {
                        self.next_token();
                    }
                }
                SyntaxKind::OpenBracketToken => {
                    self.next_token();
                    self.skip_balanced_until(SyntaxKind::CloseBracketToken);
                    self.parse_expected(SyntaxKind::CloseBracketToken, None);
                }
                SyntaxKind::OpenParenToken => {
                    self.next_token();
                    self.skip_balanced_until(SyntaxKind::CloseParenToken);
                    self.parse_expected(SyntaxKind::CloseParenToken, None);
                }
                _ => break,
            }
        }
    }

    fn skip_balanced_until(&mut self, close: SyntaxKind) {
        let mut stack = Vec::new();
        while self.token() != SyntaxKind::EndOfFileToken {
            if stack.is_empty() && self.token() == close {
                break;
            }

            match self.token() {
                SyntaxKind::OpenBraceToken => stack.push(SyntaxKind::CloseBraceToken),
                SyntaxKind::OpenBracketToken => stack.push(SyntaxKind::CloseBracketToken),
                SyntaxKind::OpenParenToken => stack.push(SyntaxKind::CloseParenToken),
                SyntaxKind::CloseBraceToken
                | SyntaxKind::CloseBracketToken
                | SyntaxKind::CloseParenToken
                    if stack.last().copied() == Some(self.token()) =>
                {
                    stack.pop();
                }
                _ => {}
            }
            self.next_token();
        }
    }

    fn skip_until_method_body_or_delimiter(&mut self) {
        let mut stack = Vec::new();
        while self.token() != SyntaxKind::EndOfFileToken {
            if stack.is_empty()
                && matches!(
                    self.token(),
                    SyntaxKind::OpenParenToken
                        | SyntaxKind::OpenBraceToken
                        | SyntaxKind::CommaToken
                        | SyntaxKind::CloseBraceToken
                )
            {
                break;
            }

            match self.token() {
                SyntaxKind::LessThanToken => stack.push(SyntaxKind::GreaterThanToken),
                SyntaxKind::OpenBracketToken => stack.push(SyntaxKind::CloseBracketToken),
                SyntaxKind::OpenParenToken => stack.push(SyntaxKind::CloseParenToken),
                SyntaxKind::OpenBraceToken => stack.push(SyntaxKind::CloseBraceToken),
                SyntaxKind::GreaterThanToken
                | SyntaxKind::CloseBracketToken
                | SyntaxKind::CloseParenToken
                | SyntaxKind::CloseBraceToken
                    if stack.last().copied() == Some(self.token()) =>
                {
                    stack.pop();
                }
                _ => {}
            }
            self.next_token();
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

    fn is_variable_statement_start(&mut self) -> bool {
        matches!(
            self.token(),
            SyntaxKind::VarKeyword | SyntaxKind::LetKeyword | SyntaxKind::ConstKeyword
        ) || self.is_using_declaration()
            || self.is_await_using_declaration()
    }

    fn is_let_declaration(&mut self) -> bool {
        self.look_ahead(|parser| {
            parser.next_token();
            parser.is_binding_identifier_or_private_identifier_or_pattern()
        })
    }

    fn is_using_declaration(&mut self) -> bool {
        self.look_ahead(|parser| {
            parser.next_token();
            !parser.scanner.has_preceding_line_break()
                && (parser.is_binding_identifier()
                    || matches!(
                        parser.token(),
                        SyntaxKind::OpenBraceToken | SyntaxKind::OpenBracketToken
                    ))
        })
    }

    fn is_await_using_declaration(&mut self) -> bool {
        self.look_ahead(|parser| {
            parser.next_token() == SyntaxKind::UsingKeyword
                && !parser.scanner.has_preceding_line_break()
                && {
                    parser.next_token();
                    !parser.scanner.has_preceding_line_break()
                        && (parser.is_binding_identifier()
                            || matches!(parser.token(), SyntaxKind::OpenBraceToken))
                }
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
    let statements = parser.parse_list(ParsingContext::SourceElements, |parser| {
        Some(parser.parse_statement())
    });
    debug_assert_eq!(parser.token(), SyntaxKind::EndOfFileToken);
    let end_of_file_token = parser.parse_token_node();
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

fn is_binary_operator(kind: SyntaxKind) -> bool {
    kind.value() >= SyntaxKind::FirstBinaryOperator.value()
        && kind.value() <= SyntaxKind::LastBinaryOperator.value()
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
    fn parse_source_file_skips_unported_import_and_declare_shapes() {
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
}
