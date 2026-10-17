//! tsgo `ls/signaturehelp.go`: the signatures of the call around a
//! position (a call, `new`, tagged template or JSX element; type arguments
//! being written; a function whose contextual signature is known) with the
//! active signature and parameter (`textDocument/signatureHelp`).

use crate::jsdoc::LinkLocations;
use tsc_checker::state::{CheckResult, SignatureId, SignatureKind};
use tsc_checker::{DisplayPart, NodeBuilder};
use tsc_diagnostics::spanmap::FEATURE_SIGNATURE_HELP;
use tsc_lsproto::{
    MarkupContent, MarkupKind, ParameterInformation, SignatureHelp, SignatureHelpContext,
    SignatureHelpOrNull, SignatureHelpParams, SignatureHelpResponse, SignatureHelpTriggerKind,
    SignatureInformation, StringOrMarkupContent, StringOrTuple, UintegerOrNull,
    VSClassifiedTextElement, VSClassifiedTextRun,
};
use tsc_syntax::{skip_trivia, NodeArrayId, NodeData, NodeId, SyntaxKind, TokenScanner};
use tsc_types::{CheckFlags, ContextFlags, ElementFlags, InternalSymbolName, SymbolId, TypeData};

use crate::completions::{is_template_literal_kind, Completions};
use crate::hover::classification_for_symbol;
use crate::language_service::{LanguageService, LsError};
use crate::request::Request;
use crate::symbols::get_declaration_name;
use crate::tree::{Property, View};
use crate::utilities::{is_in_comment, touching, Touching};

/// tsgo's signature help trigger reasons (VS Code's `toTsTriggerReason`).
#[derive(Clone, Copy, Eq, PartialEq)]
enum TriggerReason {
    None,
    Invoked,
    CharacterTyped,
    Retriggered,
}

/// tsgo `invocation`.
#[derive(Clone, Copy)]
pub(crate) enum Invocation {
    /// tsgo `callInvocation`: a call, `new`, tagged template or JSX element.
    Call(NodeId),
    /// tsgo `typeArgsInvocation`: the identifier whose type arguments are
    /// being written.
    TypeArgs(NodeId),
    /// tsgo `contextualInvocation`: a function's contextual signature, the
    /// token the search started at (the enclosing declaration for printing)
    /// and the signature's symbol.
    Contextual {
        signature: SignatureId,
        node: Touching,
        symbol: Option<SymbolId>,
    },
}

/// tsgo `argumentListInfo`.
#[derive(Clone)]
pub(crate) struct ArgumentListInfo {
    pub(crate) is_type_parameter_list: bool,
    pub(crate) invocation: Invocation,
    /// The span's position and end (tsgo `core.TextRange`; a tagged
    /// template's and a JSX element's end is its length, as tsgo builds it).
    arguments_span: (u32, u32),
    pub(crate) argument_index: usize,
    /// The apparent number of arguments.
    pub(crate) argument_count: usize,
}

/// tsgo `argumentOrParameterListInfo`.
struct ArgumentOrParameterListInfo {
    list: Option<NodeArrayId>,
    argument_index: usize,
    argument_count: usize,
    arguments_span: (u32, u32),
}

/// tsgo `contextualSignatureLocationInfo`.
struct ContextualSignatureLocationInfo {
    contextual_type: tsc_types::TypeId,
    argument_index: usize,
    argument_count: usize,
    arguments_span: (u32, u32),
}

/// tsgo `CandidateOrTypeInfo`.
enum CandidateOrTypeInfo {
    Candidates {
        candidates: Vec<SignatureId>,
        resolved_signature: SignatureId,
    },
    Type(SymbolId),
}

/// tsgo `signatureHelpParameter`.
#[derive(Clone)]
struct SignatureHelpParameter {
    info: ParameterInformation,
    is_rest: bool,
}

/// tsgo signature help's `signatureInformation`.
struct SignatureItem {
    label: String,
    documentation: Option<String>,
    parameters: Vec<SignatureHelpParameter>,
    is_variadic: bool,
    runs: Vec<VSClassifiedTextRun>,
}

/// tsgo `signatureHelpItemInfo`.
struct ItemInfo {
    is_variadic: bool,
    parameters: Vec<SignatureHelpParameter>,
    writer: DisplayParts,
}

/// tsgo `displayPartsWriter`: the text, and its classified runs for a
/// client with Visual Studio extensions.
#[derive(Clone)]
struct DisplayParts {
    text: String,
    runs: Vec<VSClassifiedTextRun>,
    vs_capability: bool,
}

impl DisplayParts {
    fn new(vs_capability: bool) -> Self {
        Self {
            text: String::new(),
            runs: Vec::new(),
            vs_capability,
        }
    }

    /// tsgo `addRun`.
    fn add_run(&mut self, classification: &str, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.vs_capability {
            self.runs.push(VSClassifiedTextRun {
                classification_type_name: classification.to_owned(),
                text: text.to_owned(),
                marker_tag_type: None,
                style: 0,
                _vs_type: Default::default(),
            });
        }
        self.text.push_str(text);
    }

    fn write(&mut self, text: &str) {
        self.add_run("text", text);
    }

    fn write_punctuation(&mut self, text: &str) {
        self.add_run("punctuation", text);
    }

    /// tsgo `WriteFrom`.
    fn write_from(&mut self, other: &DisplayParts) {
        self.text.push_str(&other.text);
        if self.vs_capability {
            self.runs.extend(other.runs.iter().cloned());
        }
    }

    /// A printer's display parts written in.
    fn write_parts(&mut self, parts: &[DisplayPart]) {
        for part in parts {
            self.add_run(part.classification, &part.text);
        }
    }
}

impl LanguageService<'_> {
    /// tsgo `ProvideSignatureHelp`: the help of the first single-segment
    /// projection of the position that has one.
    pub fn provide_signature_help(
        &mut self,
        params: &SignatureHelpParams,
    ) -> Result<SignatureHelpResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let help = self.with_program_request(false, |request| {
            for projection in
                request.position_projections(file, &params.position, FEATURE_SIGNATURE_HELP)
            {
                if !projection.fidelity.is_single_segment() {
                    continue;
                }
                if let Some(help) = request.signature_help_items(
                    projection.file,
                    projection.position,
                    params.context.as_ref(),
                )? {
                    return Ok(Some(help));
                }
            }
            Ok(None)
        })?;
        Ok(match help.flatten() {
            Some(help) => SignatureHelpOrNull::SignatureHelp(help),
            None => SignatureHelpOrNull::Null,
        })
    }
}

impl Request<'_, '_> {
    /// tsgo `GetSignatureHelpItems`.
    fn signature_help_items(
        &mut self,
        file: usize,
        position: u32,
        context: Option<&SignatureHelpContext>,
    ) -> CheckResult<Option<SignatureHelp>> {
        let mut cx = Completions::new(self, file, position);
        // Decide whether to show signature help.
        let Some(starting_token) = cx.preceding_token(position) else {
            // We are at the beginning of the file.
            return Ok(None);
        };
        let reason = trigger_reason(context);
        // Only need to be careful if the user typed a character and
        // signature help wasn't showing.
        let only_use_syntactic_owners = reason == TriggerReason::CharacterTyped;
        // Bail out quickly in the middle of a string or comment, don't
        // provide signature help unless the user explicitly requested it.
        if only_use_syntactic_owners
            && (cx.is_in_string(position, Some(starting_token))
                || is_in_comment(&cx.navigator, position, cx.found(starting_token)))
        {
            return Ok(None);
        }
        let is_manually_invoked = reason == TriggerReason::Invoked;
        let Some(argument_info) =
            cx.containing_argument_info(starting_token, is_manually_invoked, position)?
        else {
            return Ok(None);
        };
        // Extra syntactic and semantic filtering of signature help.
        match cx.candidate_or_type_info(
            &argument_info,
            starting_token,
            only_use_syntactic_owners,
        )? {
            Some(CandidateOrTypeInfo::Candidates {
                candidates,
                resolved_signature,
            }) => cx.create_signature_help_items(
                &candidates,
                resolved_signature,
                &argument_info,
                file,
                only_use_syntactic_owners,
            ),
            Some(CandidateOrTypeInfo::Type(symbol)) => {
                cx.create_type_help_items(symbol, &argument_info)
            }
            // For JS files, try a fallback that searches all source files
            // for declarations with matching names that have call
            // signatures. This is a heuristic for untyped JS code.
            None if cx.is_source_file_js() => cx.create_js_signature_help_items(&argument_info),
            None => Ok(None),
        }
    }
}

/// tsgo's trigger reason of a signature help context (VS Code's
/// `toTsTriggerReason`): none without a context.
fn trigger_reason(context: Option<&SignatureHelpContext>) -> TriggerReason {
    let Some(context) = context else {
        return TriggerReason::None;
    };
    match context.trigger_kind {
        SignatureHelpTriggerKind::TRIGGER_CHARACTER if context.trigger_character.is_some() => {
            if context.is_retrigger {
                TriggerReason::Retriggered
            } else {
                TriggerReason::CharacterTyped
            }
        }
        SignatureHelpTriggerKind::CONTENT_CHANGE => {
            if context.is_retrigger {
                TriggerReason::Retriggered
            } else {
                TriggerReason::CharacterTyped
            }
        }
        _ => TriggerReason::Invoked,
    }
}

/// tsgo's internal symbol names (`InternalSymbolNamePrefix`), which tsc-rs
/// writes with two leading underscores (a user's name has a third).
fn is_internal_symbol_name(name: &str) -> bool {
    name.starts_with("__") && !name.starts_with("___")
}

impl Completions<'_, '_, '_> {
    /// tsgo `getContainingArgumentInfo`: the innermost invocation around
    /// the token whose arguments hold the position (or end at it), else the
    /// innermost one; a contextual signature at once.
    fn containing_argument_info(
        &mut self,
        node: Touching,
        is_manually_invoked: bool,
        position: u32,
    ) -> CheckResult<Option<ArgumentListInfo>> {
        let mut first_argument_info = None;
        let mut current = Some(node);
        while let Some(n) = current {
            let kind = self.kind(n);
            if kind == SyntaxKind::SourceFile || (!is_manually_invoked && kind == SyntaxKind::Block)
            {
                break;
            }
            if let Some(argument_info) =
                self.immediately_containing_argument_or_contextual_parameter_info(n, position)?
            {
                // For contextual invocations (e.g., arrow functions with
                // contextual types), always return immediately without
                // checking the position.
                if matches!(argument_info.invocation, Invocation::Contextual { .. }) {
                    return Ok(Some(argument_info));
                }
                let (start, end) = argument_info.arguments_span;
                if first_argument_info.is_none() {
                    first_argument_info = Some(argument_info.clone());
                }
                // The end boundary of an argument list keeps the innermost
                // call; a span that contains the position is the answer.
                if end == position || (start <= position && position < end) {
                    return Ok(Some(argument_info));
                }
            }
            current = self.parent(n).map(Touching::Node);
        }
        Ok(first_argument_info)
    }

    /// tsgo `getImmediatelyContainingArgumentOrContextualParameterInfo`.
    fn immediately_containing_argument_or_contextual_parameter_info(
        &mut self,
        node: Touching,
        position: u32,
    ) -> CheckResult<Option<ArgumentListInfo>> {
        match self.try_get_parameter_info(node)? {
            Some(info) => Ok(Some(info)),
            None => self.immediately_containing_argument_info(node, position),
        }
    }

    /// tsgo `getImmediatelyContainingArgumentInfo`: the invocation whose
    /// argument (or type argument, template piece or JSX attributes) the
    /// node is (completions read it too).
    pub(crate) fn immediately_containing_argument_info(
        &mut self,
        node: Touching,
        position: u32,
    ) -> CheckResult<Option<ArgumentListInfo>> {
        let Some(parent) = self.parent(node) else {
            return Ok(None);
        };
        let parent_kind = self.node_kind(parent);
        let kind = self.kind(node);
        if matches!(
            parent_kind,
            SyntaxKind::CallExpression | SyntaxKind::NewExpression
        ) {
            // Whether the token introduces a list, ends one (no help), or is
            // buried inside one: find out if the node is an argument, a
            // type argument, or neither.
            let Some(info) = self.argument_or_parameter_list_info(node)? else {
                return Ok(None);
            };
            let is_type_parameter_list = match (self.type_argument_list_of(parent), info.list) {
                (Some(type_arguments), Some(list)) => {
                    self.list_range(type_arguments).0 == self.list_range(list).0
                }
                _ => false,
            };
            return Ok(Some(ArgumentListInfo {
                is_type_parameter_list,
                invocation: Invocation::Call(parent),
                arguments_span: info.arguments_span,
                argument_index: info.argument_index,
                argument_count: info.argument_count,
            }));
        }
        if kind == SyntaxKind::NoSubstitutionTemplateLiteral
            && parent_kind == SyntaxKind::TaggedTemplateExpression
        {
            // Check if we're actually inside the template; otherwise we'll
            // fall out and return none.
            if self.is_inside_template_literal(node, position) {
                return Ok(Some(self.argument_list_info_for_template(parent, 0)));
            }
            return Ok(None);
        }
        if kind == SyntaxKind::TemplateHead
            && self
                .parent_of(parent)
                .is_some_and(|tag| self.node_kind(tag) == SyntaxKind::TaggedTemplateExpression)
        {
            let tag = self.parent_of(parent).expect("a tagged template");
            let argument_index = if self.is_inside_template_literal(node, position) {
                0
            } else {
                1
            };
            return Ok(Some(
                self.argument_list_info_for_template(tag, argument_index),
            ));
        }
        if parent_kind == SyntaxKind::TemplateSpan {
            let template = self.parent_of(parent);
            let tag = template.and_then(|template| self.parent_of(template));
            if let (Some(template), Some(tag)) = (template, tag) {
                if self.node_kind(tag) == SyntaxKind::TaggedTemplateExpression {
                    // If we're just after a template tail, don't show
                    // signature help.
                    if kind == SyntaxKind::TemplateTail
                        && !self.is_inside_template_literal(node, position)
                    {
                        return Ok(None);
                    }
                    let NodeData::TemplateExpression(data) = self.data(template) else {
                        return Ok(None);
                    };
                    let span_index = self
                        .node_list(data.template_spans)
                        .iter()
                        .position(|&span| span == parent)
                        .unwrap_or(0);
                    let argument_index =
                        self.argument_index_for_template_piece(span_index, node, position);
                    return Ok(Some(
                        self.argument_list_info_for_template(tag, argument_index),
                    ));
                }
            }
        }
        if matches!(
            parent_kind,
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
        ) {
            // A JSX opening element or self-closing element; whether its tag
            // resolves to a function component is up to the candidates.
            let Some(attributes) = self.jsx_attributes_of(parent) else {
                return Ok(None);
            };
            let record = self.record(attributes);
            let attribute_span_start = record.pos;
            let attribute_span_end = skip_trivia(self.text(), record.end as usize) as u32;
            return Ok(Some(ArgumentListInfo {
                is_type_parameter_list: false,
                invocation: Invocation::Call(parent),
                arguments_span: (
                    attribute_span_start,
                    attribute_span_end.wrapping_sub(attribute_span_start),
                ),
                argument_index: 0,
                argument_count: 1,
            }));
        }
        if let Some((called, type_argument_count)) = self.possible_type_arguments_info(Some(node)) {
            let called_pos = self.record(called).pos;
            return Ok(Some(ArgumentListInfo {
                is_type_parameter_list: true,
                invocation: Invocation::TypeArgs(called),
                arguments_span: (called_pos, self.end(node)),
                argument_index: type_argument_count,
                argument_count: type_argument_count + 1,
            }));
        }
        Ok(None)
    }

    /// tsgo `getArgumentIndexForTemplatePiece`: the argument a template
    /// piece stands for — the strings array inside a literal, else the
    /// substitution after it (the next one just right of a literal).
    fn argument_index_for_template_piece(
        &self,
        span_index: usize,
        node: Touching,
        position: u32,
    ) -> usize {
        if is_template_literal_kind(self.kind(node)) {
            if self.is_inside_template_literal(node, position) {
                return 0;
            }
            return span_index + 2;
        }
        span_index + 1
    }

    /// tsgo `isInsideTemplateLiteral`.
    fn is_inside_template_literal(&self, node: Touching, position: u32) -> bool {
        is_template_literal_kind(self.kind(node))
            && (self.start(node) < position && position < self.end(node)
                || self.is_unterminated_literal(node) && position == self.end(node))
    }

    /// tsgo `getArgumentListInfoForTemplate`: the strings array and each
    /// substitution are the tag's arguments.
    fn argument_list_info_for_template(
        &self,
        tag: NodeId,
        argument_index: usize,
    ) -> ArgumentListInfo {
        let template = match self.data(tag) {
            NodeData::TaggedTemplateExpression(data) => data.template,
            _ => None,
        };
        let argument_count = match template.map(|template| self.data(template)) {
            Some(NodeData::TemplateExpression(data)) => {
                self.node_list(data.template_spans).len() + 1
            }
            _ => 1,
        };
        ArgumentListInfo {
            is_type_parameter_list: false,
            invocation: Invocation::Call(tag),
            argument_index,
            argument_count,
            arguments_span: self.applicable_range_for_tagged_template(template),
        }
    }

    /// tsgo `getApplicableRangeForTaggedTemplate`: from the template's start
    /// to its end (past the trivia after a missing last literal), as tsgo
    /// builds it (its end is its length).
    fn applicable_range_for_tagged_template(&self, template: Option<NodeId>) -> (u32, u32) {
        let Some(template) = template else {
            return (0, 0);
        };
        let start = self.node_start(template);
        let mut end = self.record(template).end;
        // A template without a tail: include the trivia up to the next
        // token, in case the user is about to type a middle or a tail.
        if let NodeData::TemplateExpression(data) = self.data(template) {
            if let Some(&last_span) = self.node_list(data.template_spans).last() {
                let literal = match self.data(last_span) {
                    NodeData::TemplateSpan(span) => span.literal,
                    _ => None,
                };
                if literal.is_none_or(|literal| {
                    let record = self.record(literal);
                    record.end == record.pos
                }) {
                    end = skip_trivia(self.text(), end as usize) as u32;
                }
            }
        }
        (start, end.wrapping_sub(start))
    }

    /// tsgo `tryGetParameterInfo`: the contextual signature of the function
    /// whose parameter list (or the parenthesized expression or binary
    /// expression standing for one) the token is in.
    fn try_get_parameter_info(
        &mut self,
        starting_token: Touching,
    ) -> CheckResult<Option<ArgumentListInfo>> {
        let Some(node) = self.adjusted_node(starting_token) else {
            return Ok(None);
        };
        let Some(info) = self.contextual_signature_location_info(node)? else {
            return Ok(None);
        };
        // For an optional function.
        let state = &mut *self.request.state;
        let non_nullable_contextual_type = state.get_non_nullable_type(info.contextual_type)?;
        let Some(symbol) = state.tables.type_of(non_nullable_contextual_type).symbol else {
            return Ok(None);
        };
        let signatures =
            state.get_signatures_of_type(non_nullable_contextual_type, SignatureKind::Call)?;
        let Some(&signature) = signatures.last() else {
            return Ok(None);
        };
        Ok(Some(ArgumentListInfo {
            is_type_parameter_list: false,
            invocation: Invocation::Contextual {
                signature,
                node: starting_token,
                symbol: self.choose_better_symbol(symbol),
            },
            arguments_span: info.arguments_span,
            argument_index: info.argument_index,
            argument_count: info.argument_count,
        }))
    }

    /// tsgo `chooseBetterSymbol`: for a type literal's symbol, the symbol of
    /// what declares its function type.
    fn choose_better_symbol(&self, symbol: SymbolId) -> Option<SymbolId> {
        let data = self.request.state.binder.symbol(symbol);
        if data.escaped_name.as_js() != InternalSymbolName::TYPE {
            return Some(symbol);
        }
        for &declaration in &data.declarations {
            let kind = self.node_kind(declaration);
            if kind == SyntaxKind::FunctionType {
                if let Some(parent) = self.parent_of(declaration) {
                    if crate::utilities::can_have_symbol(self.node_kind(parent)) {
                        return self.request.state.binder.node_symbol(parent);
                    }
                    // tsgo's reparser makes a property signature of a
                    // `@property`/`@param` tag and a type alias of a
                    // `@typedef`, each with the tag's type as a clone whose
                    // parent is that declaration: the function type written
                    // in the tag names the property or the alias (webpack's
                    // `isEvaluation(phase)` under `@type {ImportPhaseUtils}`,
                    // `getExportsFromData` under `@type {GetExportsFromDataFn}`).
                    if self.node_kind(parent) == SyntaxKind::JSDocTypeExpression {
                        if let Some(tag) = self.parent_of(parent).filter(|&tag| {
                            matches!(
                                self.node_kind(tag),
                                SyntaxKind::JSDocPropertyTag
                                    | SyntaxKind::JSDocParameterTag
                                    | SyntaxKind::JSDocTypedefTag
                            )
                        }) {
                            return self.request.state.binder.node_symbol(tag);
                        }
                    }
                }
            }
            // A `@callback`'s signature is the function type of the alias
            // tsgo reparses the tag into; the tag is that declaration here.
            if kind == SyntaxKind::JSDocSignature {
                if let Some(tag) = self
                    .parent_of(declaration)
                    .filter(|&tag| self.node_kind(tag) == SyntaxKind::JSDocCallbackTag)
                {
                    return self.request.state.binder.node_symbol(tag);
                }
            }
        }
        Some(symbol)
    }

    /// tsgo `getAdjustedNode`: an open paren or comma itself, else the
    /// parameter the token is in.
    fn adjusted_node(&self, node: Touching) -> Option<Touching> {
        match self.kind(node) {
            SyntaxKind::OpenParenToken | SyntaxKind::CommaToken => Some(node),
            _ => {
                let mut current = self.parent(node);
                while let Some(ancestor) = current {
                    if self.node_kind(ancestor) == SyntaxKind::Parameter {
                        return Some(Touching::Node(ancestor));
                    }
                    current = self.parent_of(ancestor);
                }
                None
            }
        }
    }

    /// tsgo `getContextualSignatureLocationInfo`: a parameter list's (or a
    /// parenthesized expression's or binary expression's) contextual type
    /// and the parameter at the node.
    fn contextual_signature_location_info(
        &mut self,
        node: Touching,
    ) -> CheckResult<Option<ContextualSignatureLocationInfo>> {
        let Some(parent) = self.parent(node) else {
            return Ok(None);
        };
        match self.node_kind(parent) {
            SyntaxKind::ParenthesizedExpression
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction => {
                let Some(info) = self.argument_or_parameter_list_info(node)? else {
                    return Ok(None);
                };
                let is_method = self.node_kind(parent) == SyntaxKind::MethodDeclaration;
                let state = &mut *self.request.state;
                let contextual_type = if is_method {
                    state.get_contextual_type_for_object_literal_element(
                        parent,
                        ContextFlags::NONE,
                    )?
                } else {
                    state.get_contextual_type(parent, ContextFlags::NONE)?
                };
                Ok(
                    contextual_type.map(|contextual_type| ContextualSignatureLocationInfo {
                        contextual_type,
                        argument_index: info.argument_index,
                        argument_count: info.argument_count,
                        arguments_span: info.arguments_span,
                    }),
                )
            }
            SyntaxKind::BinaryExpression => {
                let highest_binary = self.highest_binary(parent);
                let contextual_type = self
                    .request
                    .state
                    .get_contextual_type(highest_binary, ContextFlags::NONE)?;
                if self.kind(node) == SyntaxKind::OpenParenToken {
                    return Ok(None);
                }
                let argument_index = self.count_binary_expression_parameters(parent) - 1;
                let argument_count = self.count_binary_expression_parameters(highest_binary);
                let record = self.record(parent);
                Ok(
                    contextual_type.map(|contextual_type| ContextualSignatureLocationInfo {
                        contextual_type,
                        argument_index,
                        argument_count,
                        arguments_span: (record.pos, record.end),
                    }),
                )
            }
            _ => Ok(None),
        }
    }

    /// tsgo `getHighestBinary`.
    fn highest_binary(&self, binary: NodeId) -> NodeId {
        match self.parent_of(binary) {
            Some(parent) if self.node_kind(parent) == SyntaxKind::BinaryExpression => {
                self.highest_binary(parent)
            }
            _ => binary,
        }
    }

    /// tsgo `countBinaryExpressionParameters`.
    fn count_binary_expression_parameters(&self, binary: NodeId) -> usize {
        match self.data(binary) {
            NodeData::BinaryExpression(data)
                if data
                    .left
                    .is_some_and(|left| self.node_kind(left) == SyntaxKind::BinaryExpression) =>
            {
                self.count_binary_expression_parameters(data.left.expect("a left operand")) + 1
            }
            _ => 2,
        }
    }

    /// tsgo `getArgumentOrParameterListInfo`.
    fn argument_or_parameter_list_info(
        &mut self,
        node: Touching,
    ) -> CheckResult<Option<ArgumentOrParameterListInfo>> {
        let Some((list, argument_index)) = self.argument_or_parameter_list_and_index(node)? else {
            return Ok(None);
        };
        let argument_count = self.argument_count(node, list)?;
        let arguments_span = self.applicable_span_for_arguments(list, node);
        Ok(Some(ArgumentOrParameterListInfo {
            list,
            argument_index,
            argument_count,
            arguments_span,
        }))
    }

    /// tsgo `getApplicableSpanForArguments`: the list's full start to the
    /// end of the trivia after it (after the open token for no list), at
    /// least one position wide.
    fn applicable_span_for_arguments(
        &self,
        list: Option<NodeArrayId>,
        node: Touching,
    ) -> (u32, u32) {
        let (start, end) = match list {
            Some(list) => {
                let (pos, end) = self.list_range(list);
                (pos, skip_trivia(self.text(), end as usize) as u32)
            }
            // The user has just opened a list with no arguments.
            None => {
                let end = self.end(node);
                (end, skip_trivia(self.text(), end as usize) as u32)
            }
        };
        (start, ensure_minimum_span_size(start, end))
    }

    /// tsgo `getArgumentOrParameterListAndIndex`: the list after an open
    /// token (at its first item), else the list holding the node and its
    /// index in it.
    fn argument_or_parameter_list_and_index(
        &mut self,
        node: Touching,
    ) -> CheckResult<Option<(Option<NodeArrayId>, usize)>> {
        if matches!(
            self.kind(node),
            SyntaxKind::LessThanToken | SyntaxKind::OpenParenToken
        ) {
            // The list that starts right after the token; the user has just
            // opened it, so this is item 0.
            let list = self
                .parent(node)
                .and_then(|parent| self.child_list_that_starts_with_opener_token(parent, node));
            return Ok(Some((list, 0)));
        }
        // None outside the parent's argument or type argument list: right
        // of the close paren, between the type arguments and the arguments,
        // on the call's target or on `new`.
        let Some(list) = self.find_containing_list(node) else {
            return Ok(None);
        };
        Ok(Some((Some(list), self.argument_index(node, Some(list))?)))
    }

    /// tsgo `getChildListThatStartsWithOpenerToken`.
    fn child_list_that_starts_with_opener_token(
        &self,
        parent: NodeId,
        opener: Touching,
    ) -> Option<NodeArrayId> {
        let (type_arguments, arguments) = match self.data(parent) {
            NodeData::CallExpression(data) => (data.type_arguments, data.arguments),
            NodeData::NewExpression(data) => (data.type_arguments, data.arguments),
            _ => return None,
        };
        if self.kind(opener) == SyntaxKind::LessThanToken {
            type_arguments
        } else {
            arguments
        }
    }

    /// tsgo `findContainingList`: the NodeList of the node's parent that
    /// holds the node (a list element, or a comma between two).
    fn find_containing_list(&self, node: Touching) -> Option<NodeArrayId> {
        let parent = self.parent(node)?;
        let (pos, end) = (self.pos(node), self.end(node));
        let mut found = None;
        let tree = self.navigator.tree();
        tree.properties(self.record(parent), |property| {
            if let Property::List(Some(list))
            | Property::Modifiers(Some(list))
            | Property::HeritageTypes(Some(list)) = property
            {
                let (list_pos, list_end) = self.list_range(list);
                if list_pos <= pos && list_end >= end {
                    found = Some(list);
                }
            }
        });
        found
    }

    /// tsgo `getArgumentIndex`.
    fn argument_index(&mut self, node: Touching, list: Option<NodeArrayId>) -> CheckResult<usize> {
        let tokens = self.tokens_of_list(list, self.parent(node));
        self.argument_index_or_count(&tokens, Some(node))
    }

    /// tsgo `getArgumentCount`.
    fn argument_count(&mut self, node: Touching, list: Option<NodeArrayId>) -> CheckResult<usize> {
        let tokens = self.tokens_of_list(list, self.parent(node));
        self.argument_index_or_count(&tokens, None)
    }

    /// tsgo `getArgumentIndexOrCount`: the index of the argument at the node
    /// (a spread of a tuple counts its required elements), or the apparent
    /// number of arguments (one more after a trailing comma).
    fn argument_index_or_count(
        &mut self,
        arguments: &[Touching],
        node: Option<Touching>,
    ) -> CheckResult<usize> {
        let mut argument_index = 0;
        let mut skip_comma = false;
        for &argument in arguments {
            if node == Some(argument) {
                if !skip_comma && self.kind(argument) == SyntaxKind::CommaToken {
                    argument_index += 1;
                }
                return Ok(argument_index);
            }
            if self.kind(argument) == SyntaxKind::SpreadElement {
                argument_index += self.spread_element_count(argument)?;
                skip_comma = true;
                continue;
            }
            if self.kind(argument) != SyntaxKind::CommaToken {
                argument_index += 1;
                skip_comma = true;
                continue;
            }
            if skip_comma {
                skip_comma = false;
                continue;
            }
            argument_index += 1;
        }
        if node.is_some() {
            return Ok(argument_index);
        }
        // `f(a,)` has one child less than it has arguments.
        if arguments
            .last()
            .is_some_and(|&last| self.kind(last) == SyntaxKind::CommaToken)
        {
            return Ok(argument_index + 1);
        }
        Ok(argument_index)
    }

    /// tsgo `getSpreadElementCount`: a spread tuple's elements before its
    /// first optional one (its fixed length without one).
    fn spread_element_count(&mut self, spread: Touching) -> CheckResult<usize> {
        let Touching::Node(spread) = spread else {
            return Ok(0);
        };
        let NodeData::SpreadElement(data) = self.data(spread) else {
            return Ok(0);
        };
        let Some(expression) = data.expression else {
            return Ok(0);
        };
        let state = &mut *self.request.state;
        let spread_type = state.get_type_at_location(expression)?;
        if !state.tables.is_tuple_type(spread_type) {
            return Ok(0);
        }
        let target = state.tables.reference_target(spread_type);
        let TypeData::TupleTarget(tuple) = &state.tables.type_of(target).data else {
            return Ok(0);
        };
        if tuple.fixed_length == 0 {
            return Ok(0);
        }
        Ok(tuple
            .element_flags
            .iter()
            .position(|flags| !flags.intersects(ElementFlags::REQUIRED))
            .unwrap_or(tuple.fixed_length))
    }

    /// tsgo `getTokenFromNodeList`: a list's elements and the tokens (commas)
    /// between them, as the parent's children.
    fn tokens_of_list(&self, list: Option<NodeArrayId>, parent: Option<NodeId>) -> Vec<Touching> {
        let (Some(list), Some(parent)) = (list, parent) else {
            return Vec::new();
        };
        let (pos, end) = self.list_range(list);
        let nodes = self.node_list(Some(list));
        let mut left = pos;
        let mut index = 0;
        let mut tokens = Vec::new();
        while left < end {
            if let Some(&node) = nodes
                .get(index)
                .filter(|&&node| self.record(node).pos == left)
            {
                tokens.push(Touching::Node(node));
                left = self.record(node).end;
                index += 1;
            } else {
                let scanner = TokenScanner::new(self.source, left as usize);
                let token_end = scanner.token_end() as u32;
                tokens.push(Touching::Token {
                    kind: scanner.token(),
                    pos: scanner.token_full_start() as u32,
                    end: token_end,
                    parent,
                });
                left = token_end;
            }
        }
        tokens
    }

    /// The type arguments of a call or `new` expression.
    fn type_argument_list_of(&self, node: NodeId) -> Option<NodeArrayId> {
        match self.data(node) {
            NodeData::CallExpression(data) => data.type_arguments,
            NodeData::NewExpression(data) => data.type_arguments,
            _ => None,
        }
    }

    /// A JSX opening or self-closing element's attributes.
    fn jsx_attributes_of(&self, node: NodeId) -> Option<NodeId> {
        match self.data(node) {
            NodeData::JsxOpeningElement(data) => data.attributes,
            NodeData::JsxSelfClosingElement(data) => data.attributes,
            _ => None,
        }
    }

    /// tsgo `getCandidateOrTypeInfo`: the candidates of the invocation (a
    /// call's from the checker; type arguments' from the generic
    /// signatures, else the symbol called), none for a call the token does
    /// not syntactically belong to when only syntactic owners count.
    fn candidate_or_type_info(
        &mut self,
        info: &ArgumentListInfo,
        starting_token: Touching,
        only_use_syntactic_owners: bool,
    ) -> CheckResult<Option<CandidateOrTypeInfo>> {
        match info.invocation {
            Invocation::Call(node) => {
                if only_use_syntactic_owners && !self.is_syntactic_owner(starting_token, node) {
                    return Ok(None);
                }
                let (resolved_signature, candidates) = self
                    .request
                    .state
                    .get_resolved_signature_for_signature_help(node, info.argument_count)?;
                if candidates.is_empty() {
                    return Ok(None);
                }
                Ok(Some(CandidateOrTypeInfo::Candidates {
                    candidates,
                    resolved_signature,
                }))
            }
            Invocation::TypeArgs(called) => {
                let container = if self.node_kind(called) == SyntaxKind::Identifier {
                    self.parent_of(called).unwrap_or(called)
                } else {
                    called
                };
                if only_use_syntactic_owners
                    && !self.contains_preceding_token(starting_token, container)
                {
                    return Ok(None);
                }
                let candidates = self.possible_generic_signatures(called, info.argument_count)?;
                if let Some(&first) = candidates.first() {
                    return Ok(Some(CandidateOrTypeInfo::Candidates {
                        candidates,
                        resolved_signature: first,
                    }));
                }
                // None for an unresolved symbol.
                Ok(self
                    .request
                    .state
                    .get_symbol_at_location(called)?
                    .map(CandidateOrTypeInfo::Type))
            }
            Invocation::Contextual { signature, .. } => Ok(Some(CandidateOrTypeInfo::Candidates {
                candidates: vec![signature],
                resolved_signature: signature,
            })),
        }
    }

    /// tsgo `isSyntacticOwner`: an open paren or comma among the call's
    /// children, or a `<` before the call's expression.
    fn is_syntactic_owner(&self, starting_token: Touching, node: NodeId) -> bool {
        if !matches!(
            self.node_kind(node),
            SyntaxKind::CallExpression | SyntaxKind::NewExpression
        ) {
            return false;
        }
        match self.kind(starting_token) {
            SyntaxKind::OpenParenToken | SyntaxKind::CommaToken => self
                .navigator
                .children_from_non_js_doc(View::Node(node))
                .into_iter()
                .filter_map(touching)
                .any(|child| child == starting_token),
            SyntaxKind::LessThanToken => {
                let expression = match self.data(node) {
                    NodeData::CallExpression(data) => data.expression,
                    NodeData::NewExpression(data) => data.expression,
                    _ => None,
                };
                expression.is_some_and(|expression| {
                    self.contains_preceding_token(starting_token, expression)
                })
            }
            _ => false,
        }
    }

    /// tsgo `containsPrecedingToken`: whether the token before the starting
    /// token (searched from its parent up, past missing nodes) is in the
    /// container.
    fn contains_preceding_token(&self, starting_token: Touching, container: NodeId) -> bool {
        let pos = self.pos(starting_token);
        let mut current_parent = self.parent(starting_token);
        while let Some(parent) = current_parent {
            if let Some(preceding) = self
                .navigator
                .preceding_token_from(pos, Some(View::Node(parent)), true)
                .and_then(touching)
            {
                let record = self.record(container);
                return record.pos <= self.pos(preceding) && record.end >= self.end(preceding);
            }
            current_parent = self.parent_of(parent);
        }
        false
    }

    /// tsgo `createSignatureHelpItems`: each candidate's items (one for each
    /// list of its expanded parameters), the resolved signature's (the first
    /// variadic one, or one with enough parameters) active.
    fn create_signature_help_items(
        &mut self,
        candidates: &[SignatureId],
        resolved_signature: SignatureId,
        argument_info: &ArgumentListInfo,
        file: usize,
        use_full_prefix: bool,
    ) -> CheckResult<Option<SignatureHelp>> {
        let capabilities = self.request.capabilities;
        let documentation_format = capabilities
            .text_document
            .signature_help
            .signature_information
            .documentation_format
            .first()
            .cloned()
            .unwrap_or(MarkupKind::PLAIN_TEXT);
        let vs_capability = capabilities._vs_supports_visual_studio_extensions;
        let enclosing_declaration =
            self.enclosing_declaration_of_invocation(argument_info.invocation);
        let call_target_symbol = match argument_info.invocation {
            Invocation::Contextual { symbol, .. } => symbol,
            _ => {
                let expression = self.expression_of_invocation(argument_info.invocation);
                let mut symbol = match expression {
                    Some(expression) => self.request.state.get_symbol_at_location(expression)?,
                    None => None,
                };
                if symbol.is_none() && use_full_prefix {
                    if let Some(declaration) = self
                        .request
                        .state
                        .signature_of(resolved_signature)
                        .declaration
                    {
                        symbol = self.request.state.binder.node_symbol(declaration);
                    }
                }
                symbol
            }
        };
        // A contextual signature of an anonymous inline function type has an
        // internal name: the signature has no prefix.
        let mut call_target_display = String::new();
        if let Some(symbol) = call_target_symbol {
            let name = self.request.state.binder.symbol(symbol).escaped_name;
            if !is_internal_symbol_name(&name.as_js().to_string_lossy()) {
                call_target_display = if use_full_prefix {
                    let root = self.request.state.binder.source(file).root;
                    self.request.state.symbol_to_string_with_flags(
                        symbol,
                        Some(root),
                        tsc_types::SymbolFlags::NONE,
                        tsc_checker::SymbolFormatFlags::USE_ALIAS_DEFINED_OUTSIDE_CURRENT_SCOPE,
                    )?
                } else {
                    self.request.symbol_to_string(symbol)?
                };
            }
        }
        let mut items = Vec::with_capacity(candidates.len());
        for &candidate in candidates {
            items.push(self.signature_help_item(
                candidate,
                argument_info.is_type_parameter_list,
                &call_target_display,
                call_target_symbol,
                enclosing_declaration,
                documentation_format == MarkupKind::MARKDOWN,
                vs_capability,
            )?);
        }
        let mut selected_item_index = 0;
        let mut item_seen = 0;
        for (index, item) in items.iter().enumerate() {
            if candidates[index] == resolved_signature {
                selected_item_index = item_seen;
                if item.len() > 1 {
                    for (count, signature) in item.iter().enumerate() {
                        if signature.is_variadic
                            || signature.parameters.len() >= argument_info.argument_count
                        {
                            selected_item_index = item_seen + count;
                            break;
                        }
                    }
                }
            }
            item_seen += item.len();
        }
        let flattened: Vec<SignatureItem> = items.into_iter().flatten().collect();
        if flattened.is_empty() {
            return Ok(None);
        }
        let signature_information = &capabilities
            .text_document
            .signature_help
            .signature_information;
        let supports_per_signature_active_parameter =
            signature_information.active_parameter_support;
        let supports_null_active_parameter = signature_information.no_active_parameter_support;
        let active_parameter = (!supports_per_signature_active_parameter).then(|| {
            compute_active_parameter(
                &flattened[selected_item_index],
                argument_info.argument_index,
                supports_null_active_parameter,
            )
        });
        let signatures = flattened
            .iter()
            .map(|item| SignatureInformation {
                label: item.label.clone(),
                documentation: item.documentation.as_ref().map(|documentation| {
                    StringOrMarkupContent::MarkupContent(MarkupContent {
                        kind: documentation_format.clone(),
                        value: documentation.clone(),
                    })
                }),
                parameters: Some(
                    item.parameters
                        .iter()
                        .map(|parameter| parameter.info.clone())
                        .collect(),
                ),
                active_parameter: if supports_per_signature_active_parameter {
                    compute_active_parameter(
                        item,
                        argument_info.argument_index,
                        supports_null_active_parameter,
                    )
                } else {
                    None
                },
                _vs_colorized_label: (!item.runs.is_empty()).then(|| VSClassifiedTextElement {
                    runs: item.runs.clone(),
                    ..VSClassifiedTextElement::default()
                }),
            })
            .collect();
        Ok(Some(SignatureHelp {
            signatures,
            active_signature: Some(selected_item_index as u32),
            active_parameter: active_parameter.flatten(),
        }))
    }

    /// tsgo `getEnclosingDeclarationFromInvocation`: the call, the
    /// identifier called, or (a contextual signature's) the node the search
    /// started at.
    fn enclosing_declaration_of_invocation(&self, invocation: Invocation) -> Option<NodeId> {
        match invocation {
            Invocation::Call(node) => Some(node),
            Invocation::TypeArgs(called) => Some(called),
            Invocation::Contextual { node, .. } => match node {
                Touching::Node(node) => Some(node),
                Touching::Token { parent, .. } => Some(parent),
            },
        }
    }

    /// tsgo `getExpressionFromInvocation`: what a call invokes (tsgo
    /// `GetInvokedExpression`), or the identifier called with type
    /// arguments.
    fn expression_of_invocation(&self, invocation: Invocation) -> Option<NodeId> {
        match invocation {
            Invocation::Call(node) => self.request.invoked_expression(node),
            Invocation::TypeArgs(called) => Some(called),
            Invocation::Contextual { .. } => None,
        }
    }

    /// tsgo `getSignatureHelpItem`: an item for each list of the candidate's
    /// expanded parameters, labeled by the call target, the (type)
    /// parameters and the return type, documented by its declaration's
    /// comment.
    #[allow(clippy::too_many_arguments)]
    fn signature_help_item(
        &mut self,
        candidate: SignatureId,
        is_type_parameter_list: bool,
        call_target_display: &str,
        call_target_symbol: Option<SymbolId>,
        enclosing_declaration: Option<NodeId>,
        markdown: bool,
        vs_capability: bool,
    ) -> CheckResult<Vec<SignatureItem>> {
        let infos = if is_type_parameter_list {
            self.item_info_for_type_parameters(candidate, enclosing_declaration, vs_capability)?
        } else {
            self.item_info_for_parameters(
                candidate,
                enclosing_declaration,
                markdown,
                vs_capability,
            )?
        };
        let suffix =
            self.return_type_to_display_parts(candidate, enclosing_declaration, vs_capability)?;
        // The documentation of the signature's declaration.
        let declaration = self.request.state.signature_of(candidate).declaration;
        let mut documentation = None;
        if let Some(declaration) = declaration {
            let text = self.request.documentation_from_declaration(
                None,
                Some(declaration),
                Touching::Node(declaration),
                markdown,
                true,
                LinkLocations::Feature(FEATURE_SIGNATURE_HELP),
            )?;
            if !text.is_empty() {
                documentation = Some(text);
            }
        }
        let mut result = Vec::with_capacity(infos.len());
        for info in infos {
            let mut label = DisplayParts::new(vs_capability);
            if !call_target_display.is_empty() {
                let classification = call_target_symbol.map_or("text", |symbol| {
                    classification_for_symbol(self.request.state, symbol)
                });
                label.add_run(classification, call_target_display);
            }
            label.write_from(&info.writer);
            label.write_from(&suffix);
            result.push(SignatureItem {
                label: label.text,
                documentation: documentation.clone(),
                parameters: info.parameters,
                is_variadic: info.is_variadic,
                runs: label.runs,
            });
        }
        Ok(result)
    }

    /// tsgo `returnTypeToDisplayParts`: `: ` and the signature's type
    /// predicate or return type.
    fn return_type_to_display_parts(
        &mut self,
        candidate: SignatureId,
        enclosing_declaration: Option<NodeId>,
        vs_capability: bool,
    ) -> CheckResult<DisplayParts> {
        let mut parts = DisplayParts::new(vs_capability);
        parts.write_punctuation(": ");
        let state = &mut *self.request.state;
        if let Some(predicate) = state.get_type_predicate_of_signature(candidate)? {
            parts.write(&printed(state.type_predicate_to_string(&predicate)));
            return Ok(parts);
        }
        let return_type = state.get_return_type_of_signature(candidate)?;
        match state.signature_help_type(return_type, enclosing_declaration) {
            Ok(Some(type_parts)) => parts.write_parts(&type_parts),
            _ => parts.write(&printed(state.type_to_string_with_flags(
                return_type,
                None,
                tsc_checker::exports::TYPE_FORMAT_DEFAULT,
                None,
            ))),
        }
        Ok(parts)
    }

    /// tsgo `itemInfoForTypeParameters`: the candidate's type parameters as
    /// the parameters, labeled `<T, U>(a: T, b: U)` for each list of its
    /// expanded parameters.
    fn item_info_for_type_parameters(
        &mut self,
        candidate: SignatureId,
        enclosing_declaration: Option<NodeId>,
        vs_capability: bool,
    ) -> CheckResult<Vec<ItemInfo>> {
        // tsgo prints the item's labels with a node builder of its own.
        let mut builder = NodeBuilder::default();
        let signature = self.request.state.signature_of(candidate).clone();
        let type_parameters = match signature.target {
            Some(target) => self
                .request
                .state
                .signature_of(target)
                .type_parameters
                .clone(),
            None => signature.type_parameters.clone(),
        }
        .unwrap_or_default();
        let signature_help_type_parameters: Vec<SignatureHelpParameter> = type_parameters
            .iter()
            .map(|&type_parameter| {
                self.type_parameter_parameter(&mut builder, type_parameter, enclosing_declaration)
            })
            .collect();
        // The type parameters' label.
        let mut parts = DisplayParts::new(vs_capability);
        parts.write_punctuation("<");
        for (index, type_parameter) in signature_help_type_parameters.iter().enumerate() {
            if index > 0 {
                parts.write_punctuation(", ");
            }
            parts.add_run("type parameter name", &label_of(&type_parameter.info));
        }
        parts.write_punctuation(">");
        // The parameters' label, like `(a: string, b: number)`; the type
        // parameters are the item's parameters.
        let lists = self
            .request
            .state
            .get_expanded_parameters(candidate, false)?;
        if !lists.is_empty() {
            parts.write_punctuation("(");
        }
        let mut result = Vec::with_capacity(lists.len());
        for list in lists {
            let mut list_parts = parts.clone();
            for (index, &parameter) in list.iter().enumerate() {
                if index > 0 {
                    list_parts.write_punctuation(", ");
                }
                let parameter_parts =
                    self.parameter_parts(&mut builder, parameter, enclosing_declaration);
                list_parts.write_parts(&parameter_parts);
            }
            list_parts.write_punctuation(")");
            result.push(ItemInfo {
                is_variadic: false,
                parameters: signature_help_type_parameters.clone(),
                writer: list_parts,
            });
        }
        Ok(result)
    }

    /// tsgo `itemInfoForParameters`: labeled `<T>(a: T, b: number)` for each
    /// list of the candidate's expanded parameters.
    fn item_info_for_parameters(
        &mut self,
        candidate: SignatureId,
        enclosing_declaration: Option<NodeId>,
        markdown: bool,
        vs_capability: bool,
    ) -> CheckResult<Vec<ItemInfo>> {
        // tsgo prints the item's labels with a node builder of its own.
        let mut builder = NodeBuilder::default();
        let signature = self.request.state.signature_of(candidate).clone();
        let type_parameters = signature.type_parameters.clone().unwrap_or_default();
        let signature_help_type_parameters: Vec<SignatureHelpParameter> = type_parameters
            .iter()
            .map(|&type_parameter| {
                self.type_parameter_parameter(&mut builder, type_parameter, enclosing_declaration)
            })
            .collect();
        // The type parameters' label, like `<T, U>`.
        let mut parts = DisplayParts::new(vs_capability);
        if !signature_help_type_parameters.is_empty() {
            parts.write_punctuation("<");
            for (index, type_parameter) in signature_help_type_parameters.iter().enumerate() {
                if index > 0 {
                    parts.write_punctuation(", ");
                }
                parts.add_run("type parameter name", &label_of(&type_parameter.info));
            }
            parts.write_punctuation(">");
        }
        // The parameters' label, like `(a: string, b: number)`.
        let lists = self
            .request
            .state
            .get_expanded_parameters(candidate, false)?;
        if !lists.is_empty() {
            parts.write_punctuation("(");
        }
        let has_effective_rest_parameter =
            self.request.state.has_effective_rest_parameter(candidate)?;
        let single_list = lists.len() == 1;
        let mut result = Vec::with_capacity(lists.len());
        for list in lists {
            let is_variadic = has_effective_rest_parameter
                && (single_list
                    || list.last().is_some_and(|&last| {
                        self.request
                            .state
                            .symbol_check_flags(last)
                            .intersects(CheckFlags::REST_PARAMETER)
                    }));
            let mut list_parts = parts.clone();
            let mut parameters = Vec::with_capacity(list.len());
            for (index, &parameter) in list.iter().enumerate() {
                if index > 0 {
                    list_parts.write_punctuation(", ");
                }
                let parameter_parts =
                    self.parameter_parts(&mut builder, parameter, enclosing_declaration);
                let label: String = parameter_parts
                    .iter()
                    .map(|part| part.text.as_str())
                    .collect();
                list_parts.write_parts(&parameter_parts);
                parameters.push(self.parameter_from_label(parameter, label, markdown)?);
            }
            list_parts.write_punctuation(")");
            result.push(ItemInfo {
                is_variadic,
                parameters,
                writer: list_parts,
            });
        }
        Ok(result)
    }

    /// A parameter's declaration as signature help prints it (tsgo
    /// `SymbolToParameterDeclaration` written into a display parts writer).
    fn parameter_parts(
        &mut self,
        builder: &mut NodeBuilder,
        parameter: SymbolId,
        enclosing_declaration: Option<NodeId>,
    ) -> Vec<DisplayPart> {
        self.request
            .state
            .with_node_builder(builder, |state| {
                state.signature_help_parameter(parameter, enclosing_declaration)
            })
            .unwrap_or_default()
    }

    /// tsgo `createSignatureHelpParameterFromLabel`: a parameter labeled
    /// `label`, documented by its declaration's comment.
    fn parameter_from_label(
        &mut self,
        parameter: SymbolId,
        label: String,
        markdown: bool,
    ) -> CheckResult<SignatureHelpParameter> {
        let check_flags = self.request.state.symbol_check_flags(parameter);
        let is_rest = check_flags.intersects(CheckFlags::REST_PARAMETER);
        let mut documentation = None;
        if let Some(declaration) = self
            .request
            .state
            .binder
            .symbol(parameter)
            .value_declaration
        {
            let text = self.request.documentation_from_declaration(
                None,
                Some(declaration),
                Touching::Node(declaration),
                markdown,
                true,
                LinkLocations::Feature(FEATURE_SIGNATURE_HELP),
            )?;
            if !text.is_empty() {
                documentation = Some(StringOrMarkupContent::MarkupContent(MarkupContent {
                    kind: if markdown {
                        MarkupKind::MARKDOWN
                    } else {
                        MarkupKind::PLAIN_TEXT
                    },
                    value: text,
                }));
            }
        }
        Ok(SignatureHelpParameter {
            info: ParameterInformation {
                label: StringOrTuple::String(label),
                documentation,
            },
            is_rest,
        })
    }

    /// tsgo `createSignatureHelpParameterForTypeParameter`.
    fn type_parameter_parameter(
        &mut self,
        builder: &mut NodeBuilder,
        type_parameter: tsc_types::TypeId,
        enclosing_declaration: Option<NodeId>,
    ) -> SignatureHelpParameter {
        let label = self
            .request
            .state
            .with_node_builder(builder, |state| {
                state.signature_help_type_parameter(type_parameter, enclosing_declaration)
            })
            .unwrap_or_default();
        SignatureHelpParameter {
            info: ParameterInformation {
                label: StringOrTuple::String(label),
                documentation: None,
            },
            is_rest: false,
        }
    }

    /// tsgo `createTypeHelpItems`: a generic type's type parameters while its
    /// type arguments are written (no signature has them).
    fn create_type_help_items(
        &mut self,
        symbol: SymbolId,
        argument_info: &ArgumentListInfo,
    ) -> CheckResult<Option<SignatureHelp>> {
        let type_parameters = self
            .request
            .state
            .get_local_type_parameters_of_class_or_interface_or_type_alias(symbol);
        if type_parameters.is_empty() {
            return Ok(None);
        }
        let enclosing_declaration =
            self.enclosing_declaration_of_invocation(argument_info.invocation);
        // tsgo `getTypeHelpItem` prints the type parameters with a node
        // builder of its own.
        let mut builder = NodeBuilder::default();
        let parameters: Vec<SignatureHelpParameter> = type_parameters
            .iter()
            .map(|&type_parameter| {
                self.type_parameter_parameter(&mut builder, type_parameter, enclosing_declaration)
            })
            .collect();
        // The label: the symbol's name and its type parameters.
        let mut label = self.request.symbol_to_string(symbol)?;
        label.push('<');
        for (index, parameter) in parameters.iter().enumerate() {
            if index > 0 {
                label.push_str(", ");
            }
            label.push_str(&label_of(&parameter.info));
        }
        label.push('>');
        let supports_per_signature_active_parameter = self
            .request
            .capabilities
            .text_document
            .signature_help
            .signature_information
            .active_parameter_support;
        let active_parameter = (!parameters.is_empty()).then_some(UintegerOrNull::Uinteger(
            argument_info.argument_index as u32,
        ));
        Ok(Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: None,
                parameters: Some(
                    parameters
                        .into_iter()
                        .map(|parameter| parameter.info)
                        .collect(),
                ),
                active_parameter: active_parameter
                    .clone()
                    .filter(|_| supports_per_signature_active_parameter),
                _vs_colorized_label: None,
            }],
            active_signature: Some(0),
            active_parameter: active_parameter.filter(|_| !supports_per_signature_active_parameter),
        }))
    }

    /// tsgo `createJSSignatureHelpItems`: for a property access call in
    /// JavaScript the checker cannot resolve, the call signatures of the
    /// first declaration of that name in the program's files.
    fn create_js_signature_help_items(
        &mut self,
        argument_info: &ArgumentListInfo,
    ) -> CheckResult<Option<SignatureHelp>> {
        if matches!(argument_info.invocation, Invocation::Contextual { .. }) {
            return Ok(None);
        }
        // Some symbol with the call expression's name that has call
        // signatures.
        let Some(expression) = self.expression_of_invocation(argument_info.invocation) else {
            return Ok(None);
        };
        let NodeData::PropertyAccessExpression(access) = self.data(expression) else {
            return Ok(None);
        };
        let Some(name) = access
            .name
            .and_then(|name| tsc_binder::node_util::id_text(self.source_of(name), name))
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        if name.is_empty() {
            return Ok(None);
        }
        for file in 0..self.request.state.binder.file_count() {
            let root = self.request.state.binder.source(file).root;
            if let Some(help) =
                self.signature_help_from_named_declarations(file, root, &name, argument_info)?
            {
                return Ok(Some(help));
            }
        }
        Ok(None)
    }

    /// tsgo `findSignatureHelpFromNamedDeclarations`: the first declaration
    /// named `name` (in the file's tree order) whose type has call
    /// signatures.
    fn signature_help_from_named_declarations(
        &mut self,
        file: usize,
        node: NodeId,
        name: &str,
        argument_info: &ArgumentListInfo,
    ) -> CheckResult<Option<SignatureHelp>> {
        let source = self.request.state.binder.source(file);
        if get_declaration_name(source, node) == name {
            if let Some(symbol) = self.request.state.binder.node_symbol(node) {
                let ty = self
                    .request
                    .state
                    .get_type_of_symbol_at_location(symbol, Some(node))?;
                let call_signatures = self
                    .request
                    .state
                    .get_signatures_of_type(ty, SignatureKind::Call)?;
                if let Some(&first) = call_signatures.first() {
                    if let Some(help) = self.create_signature_help_items(
                        &call_signatures,
                        first,
                        argument_info,
                        file,
                        true,
                    )? {
                        return Ok(Some(help));
                    }
                }
            }
        }
        for child in self.request.children(node) {
            if let Some(help) =
                self.signature_help_from_named_declarations(file, child, name, argument_info)?
            {
                return Ok(Some(help));
            }
        }
        Ok(None)
    }
}

/// tsgo `ensureMinimumSpanSize`: an empty span takes one position (a span
/// holds its start but not its end).
fn ensure_minimum_span_size(start: u32, end: u32) -> u32 {
    if end <= start {
        start + 1
    } else {
        end
    }
}

/// tsgo `computeActiveParameter`: the argument's parameter (the last for a
/// trailing rest parameter; none or one past the end for a rest parameter
/// in the middle); omitted without parameters.
fn compute_active_parameter(
    signature: &SignatureItem,
    argument_index: usize,
    supports_null: bool,
) -> Option<UintegerOrNull> {
    let parameter_count = signature.parameters.len();
    if parameter_count == 0 {
        return None;
    }
    let mut active_parameter = argument_index as u32;
    if signature.is_variadic {
        let first_rest = signature
            .parameters
            .iter()
            .position(|parameter| parameter.is_rest);
        if first_rest.is_some_and(|first_rest| first_rest < parameter_count - 1) {
            // A rest parameter in the middle cannot be highlighted.
            if supports_null {
                return Some(UintegerOrNull::Null);
            }
            return Some(UintegerOrNull::Uinteger(parameter_count as u32));
        }
        // Clamp to the last parameter for a trailing rest parameter.
        active_parameter = active_parameter.min(parameter_count as u32 - 1);
    }
    Some(UintegerOrNull::Uinteger(active_parameter))
}

/// A parameter's label text.
fn label_of(info: &ParameterInformation) -> String {
    match &info.label {
        StringOrTuple::String(label) => label.clone(),
        StringOrTuple::Tuple(_) => String::new(),
    }
}

/// A printed text, empty when the printer failed.
fn printed(text: Result<String, String>) -> String {
    text.unwrap_or_default()
}
