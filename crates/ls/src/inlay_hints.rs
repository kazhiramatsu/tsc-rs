//! tsgo `ls/inlay_hints.go`: a range's inlay hints (`textDocument/inlayHint`)
//! — the inferred types of variables, properties, parameters and returns,
//! the values of enum members, and the parameter names of arguments.

use crate::lsconv::Script;
use tsc_binder::node_util;
use tsc_binder::SymbolId;
use tsc_checker::exports::INLAY_HINT_NODE_BUILDER_FLAGS;
use tsc_checker::state::CheckResult;
use tsc_diagnostics::spanmap::{Fidelity, FEATURE_INLAY_HINTS};
use tsc_emitter::{TransformArena, TransformNode, TransformNodeArray};
use tsc_lsproto::{
    InlayHint, InlayHintKind, InlayHintLabelPart, InlayHintParams, InlayHintResponse,
    InlayHintsOrNull, Location, StringOrInlayHintLabelParts,
};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{ElementFlags, SignatureFlags, SymbolFlags, TypeData, TypeFlags, TypeId};

use crate::astnav::{Found, Navigator};
use crate::language_service::{LanguageService, LsError};
use crate::lsutil::{IncludeInlayParameterNameHints, InlayHintsPreferences, QuotePreference};
use crate::request::Request;
use crate::tree::View;
use crate::utilities::script_of;

impl LanguageService<'_> {
    /// tsgo `ProvideInlayHint`: the hints of each projection of the range.
    pub fn provide_inlay_hints(
        &mut self,
        params: &InlayHintParams,
    ) -> Result<InlayHintResponse, LsError> {
        let preferences = self.user_preferences_ref().inlay_hints_preferences.clone();
        if !is_any_inlay_hint_enabled(&preferences) {
            return Ok(InlayHintsOrNull::Null);
        }
        let file = self.file(&params.text_document.uri);
        let user_preferences = self.user_preferences_ref().clone();
        let hints = self
            .with_program_request(false, |request| {
                let quote_preference = crate::lsutil::get_quote_preference(
                    request.state.binder.source(file),
                    &user_preferences,
                );
                let mut result = Vec::new();
                for (projection, span) in
                    request.range_projections_intersecting(file, &params.range, FEATURE_INLAY_HINTS)
                {
                    let mut hints = InlayHints {
                        request,
                        file: projection,
                        span: (span.start, span.end),
                        preferences: &preferences,
                        quote_preference,
                        result: Vec::new(),
                    };
                    let root = hints.source().root;
                    hints.visit(root)?;
                    result.extend(hints.result);
                }
                Ok(result)
            })?
            .unwrap_or_default();
        Ok(InlayHintsOrNull::InlayHints(hints))
    }
}

/// tsgo `inlayHintState`.
struct InlayHints<'r, 's, 'a, 'p> {
    request: &'r mut Request<'s, 'a>,
    file: usize,
    span: (usize, usize),
    preferences: &'p InlayHintsPreferences,
    quote_preference: QuotePreference,
    result: Vec<InlayHint>,
}

/// tsgo `parameterInfo`.
struct ParameterInfo {
    parameter: NodeId,
    name: String,
    is_rest_parameter: bool,
}

impl InlayHints<'_, '_, '_, '_> {
    fn source(&self) -> &SourceFile {
        self.request.state.binder.source(self.file)
    }

    fn kind(&self, node: NodeId) -> SyntaxKind {
        self.request.state.binder.node_record(node).kind
    }

    fn data(&self, node: NodeId) -> &NodeData {
        &self.request.state.binder.node_record(node).data
    }

    fn name_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::name_field_of(self.request.state.binder.source_of_node(node), node)
    }

    fn list(&self, list: Option<NodeArrayId>) -> Vec<NodeId> {
        list.map_or_else(Vec::new, |list| {
            self.request.state.binder.node_array(list).nodes.to_vec()
        })
    }

    /// tsgo `GetStartOfNode(node, file, false)`.
    fn start(&self, node: NodeId) -> usize {
        let source = self.request.state.binder.source_of_node(node);
        Navigator::new(source).start_of_node(Found::Node(View::Node(node)), false) as usize
    }

    /// tsgo `visit`: a node's hints (by the preferences), then its children's;
    /// none in a type or outside the range.
    fn visit(&mut self, root: NodeId) -> CheckResult<()> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let record = self.request.state.binder.node_record(node);
            let (pos, end, kind) = (record.pos as usize, record.end as usize, record.kind);
            if end == pos {
                continue;
            }
            // tsgo `TextRange.Intersects`.
            if self.span.0.max(pos) > self.span.1.min(end) {
                continue;
            }
            if crate::completions::is_type_node_kind(kind)
                && kind != SyntaxKind::ExpressionWithTypeArguments
            {
                continue;
            }
            let preferences = self.preferences;
            if preferences.include_inlay_variable_type_hints == Some(true)
                && kind == SyntaxKind::VariableDeclaration
                || preferences.include_inlay_property_declaration_type_hints == Some(true)
                    && kind == SyntaxKind::PropertyDeclaration
            {
                self.visit_variable_like_declaration(node)?;
            } else if preferences.include_inlay_enum_member_value_hints == Some(true)
                && kind == SyntaxKind::EnumMember
            {
                self.visit_enum_member(node)?;
            } else if should_show_parameter_name_hints(preferences)
                && matches!(kind, SyntaxKind::CallExpression | SyntaxKind::NewExpression)
            {
                self.visit_call_or_new_expression(node)?;
            } else {
                if preferences.include_inlay_function_parameter_type_hints == Some(true)
                    && node_util::is_function_like_declaration_kind(kind)
                    && self.request.state.has_context_sensitive_parameters(node)
                {
                    self.visit_function_like_for_parameter_type(node)?;
                }
                if preferences.include_inlay_function_like_return_type_hints == Some(true)
                    && is_signature_supporting_return_annotation(kind)
                {
                    self.visit_function_declaration_like_for_return_type(node)?;
                }
            }
            let source = self.source();
            let mut children = Vec::new();
            tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            stack.extend(children.into_iter().rev());
        }
        Ok(())
    }

    /// tsgo `visitFunctionDeclarationLikeForReturnType`: the inferred return
    /// type (or type predicate) of a function with a body and no annotation
    /// (an arrow function's only with parentheses).
    fn visit_function_declaration_like_for_return_type(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult<()> {
        if self.kind(declaration) == SyntaxKind::ArrowFunction
            && Navigator::new(self.source())
                .child_of_kind(View::Node(declaration), SyntaxKind::OpenParenToken)
                .is_none()
        {
            return Ok(());
        }
        let source = self.request.state.binder.source_of_node(declaration);
        if node_util::type_node_of(source, declaration).is_some()
            || node_util::body_of(source, declaration).is_none()
        {
            return Ok(());
        }
        let signature = self
            .request
            .state
            .get_signature_from_declaration_checked(declaration)?;
        let position = self.type_annotation_position(declaration);
        if let Some(predicate) = self
            .request
            .state
            .get_type_predicate_of_signature(signature)?
        {
            if predicate.ty.is_some() {
                let parts = self.type_predicate_to_inlay_hint_parts(&predicate)?;
                self.add_type_hints(parts, position);
                return Ok(());
            }
        }
        let return_type = self.request.state.get_return_type_of_signature(signature)?;
        if self.is_module_reference_type(return_type) {
            return Ok(());
        }
        let parts = self.type_to_inlay_hint_parts(return_type)?;
        self.add_type_hints(parts, position);
        Ok(())
    }

    /// tsgo `visitCallOrNewExpression`: each argument's parameter name (a
    /// spread tuple's required elements counted), unless the argument names
    /// it already (as an identifier, a property access or a comment) — a
    /// rest parameter's first argument always.
    fn visit_call_or_new_expression(&mut self, expression: NodeId) -> CheckResult<()> {
        let arguments = match self.data(expression) {
            NodeData::CallExpression(data) => self.list(data.arguments),
            NodeData::NewExpression(data) => self.list(data.arguments),
            _ => Vec::new(),
        };
        if arguments.is_empty() {
            return Ok(());
        }
        let signature = self.request.state.get_resolved_signature(
            expression,
            None,
            tsc_types::CheckMode::NORMAL,
        )?;
        let mut signature_parameter_position = 0;
        for original_argument in arguments {
            let argument = node_util::skip_parentheses_pub(
                self.request.state.binder.source_of_node(original_argument),
                original_argument,
            );
            if should_show_literal_parameter_name_hints_only(self.preferences)
                && !self.is_hintable_literal(argument)
            {
                signature_parameter_position += 1;
                continue;
            }
            let mut spread_arguments = 0;
            if let NodeData::SpreadElement(spread) = self.data(argument) {
                if let Some(spread_expression) = spread.expression {
                    let state = &mut *self.request.state;
                    let spread_type = state.get_type_at_location(spread_expression)?;
                    if state.tables.is_tuple_type(spread_type) {
                        let target = state.tables.reference_target(spread_type);
                        if let TypeData::TupleTarget(tuple) = &state.tables.type_of(target).data {
                            if tuple.fixed_length == 0 {
                                continue;
                            }
                            let required = tuple
                                .element_flags
                                .iter()
                                .position(|flags| !flags.intersects(ElementFlags::REQUIRED))
                                .unwrap_or(tuple.fixed_length);
                            if required > 0 {
                                spread_arguments = required;
                            }
                        }
                    }
                }
            }
            let info = self
                .parameter_identifier_info_at_position(signature, signature_parameter_position)?;
            signature_parameter_position += if spread_arguments > 0 {
                spread_arguments
            } else {
                1
            };
            let Some(info) = info else {
                return Ok(());
            };
            let parameter_name_not_same_as_argument = self
                .preferences
                .include_inlay_parameter_name_hints_when_argument_matches_name
                == Some(true)
                || !self.identifier_or_access_expression_postfix_matches_parameter_name(
                    argument, &info.name,
                );
            if !parameter_name_not_same_as_argument && !info.is_rest_parameter {
                continue;
            }
            if self.leading_comments_contain_parameter_name(argument, &info.name) {
                continue;
            }
            let position = self.start(original_argument);
            self.add_parameter_hints(&info.name, info.parameter, position, info.is_rest_parameter);
        }
        Ok(())
    }

    /// tsgo `visitEnumMember`: an enum member's value without an initializer.
    fn visit_enum_member(&mut self, member: NodeId) -> CheckResult<()> {
        let source = self.request.state.binder.source_of_node(member);
        if node_util::initializer_of(source, member).is_some() {
            return Ok(());
        }
        if let Some(value) = self.request.state.get_constant_value(member)? {
            let text = match value {
                tsc_checker::exports::ConstantValue::String(text) => {
                    text.to_string_lossy().into_owned()
                }
                tsc_checker::exports::ConstantValue::Number(number) => {
                    tsc_types::tables::js_number_to_string(number)
                }
            };
            let end = self.request.state.binder.node_record(member).end as usize;
            self.add_enum_member_value_hints(&text, end);
        }
        Ok(())
    }

    /// tsgo `visitVariableLikeDeclaration`: the type of a variable (or
    /// property) without an annotation — one with an initializer, or a
    /// property whose type is not `any` —, not of a binding pattern, a
    /// variable not hintable, or a module, nor one that spells the name.
    fn visit_variable_like_declaration(&mut self, declaration: NodeId) -> CheckResult<()> {
        let source = self.request.state.binder.source_of_node(declaration);
        let kind = self.kind(declaration);
        let initializer = node_util::initializer_of(source, declaration);
        let name = self.name_of(declaration);
        let no_value = initializer.is_none()
            && !(kind == SyntaxKind::PropertyDeclaration && {
                let ty = self.request.state.get_type_at_location(declaration)?;
                !self
                    .request
                    .state
                    .tables
                    .flags_of(ty)
                    .intersects(TypeFlags::ANY)
            });
        if no_value
            || name.is_some_and(|name| {
                matches!(
                    self.kind(name),
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                )
            })
            || (kind == SyntaxKind::VariableDeclaration
                && !self.is_hintable_declaration(declaration))
        {
            return Ok(());
        }
        let source = self.request.state.binder.source_of_node(declaration);
        if node_util::type_node_of(source, declaration).is_some() {
            return Ok(());
        }
        let declaration_type = self.request.state.get_type_at_location(declaration)?;
        if self.is_module_reference_type(declaration_type) {
            return Ok(());
        }
        let parts = self.type_to_inlay_hint_parts(declaration_type)?;
        let Some(name) = name else {
            return Ok(());
        };
        let hint_text: String = parts.iter().map(|part| part.value.as_str()).collect();
        if self
            .preferences
            .include_inlay_variable_type_hints_when_type_matches_name
            != Some(true)
            && self.kind(name) != SyntaxKind::ComputedPropertyName
            && equate_string_case_insensitive(&self.node_text(name), &hint_text)
        {
            return Ok(());
        }
        let end = self.request.state.binder.node_record(name).end as usize;
        self.add_type_hints(parts, end);
        Ok(())
    }

    /// tsgo `visitFunctionLikeForParameterType`: the type of each hintable
    /// parameter without an annotation (`this` by the signature's).
    fn visit_function_like_for_parameter_type(&mut self, node: NodeId) -> CheckResult<()> {
        let signature = self
            .request
            .state
            .get_signature_from_declaration_checked(node)?;
        let parameters = crate::utilities::parameters_in(self.source(), node);
        let mut position = 0;
        for parameter in parameters {
            let is_this = self.is_this_parameter(parameter);
            if self.is_hintable_declaration(parameter) {
                let record = self.request.state.signature_of(signature);
                let symbol = if is_this {
                    record.this_parameter
                } else {
                    record.parameters.get(position).copied()
                };
                self.add_parameter_type_hint(parameter, symbol)?;
            }
            if is_this {
                continue;
            }
            position += 1;
        }
        Ok(())
    }

    /// tsgo `addParameterTypeHint`: after the name (or `?`) of a parameter
    /// without an annotation.
    fn add_parameter_type_hint(
        &mut self,
        node: NodeId,
        symbol: Option<SymbolId>,
    ) -> CheckResult<()> {
        let source = self.request.state.binder.source_of_node(node);
        if node_util::type_node_of(source, node).is_some() {
            return Ok(());
        }
        let Some(symbol) = symbol else {
            return Ok(());
        };
        let Some(parts) = self.parameter_declaration_type_hints(symbol)? else {
            return Ok(());
        };
        let NodeData::Parameter(data) = self.data(node) else {
            return Ok(());
        };
        let Some(after) = data.question_token.or(data.name) else {
            return Ok(());
        };
        let position = self.request.state.binder.node_record(after).end as usize;
        self.add_type_hints(parts, position);
        Ok(())
    }

    /// tsgo `getParameterDeclarationTypeHints`.
    fn parameter_declaration_type_hints(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult<Option<Vec<InlayHintLabelPart>>> {
        let Some(value_declaration) = self.request.state.binder.symbol(symbol).value_declaration
        else {
            return Ok(None);
        };
        if self.kind(value_declaration) != SyntaxKind::Parameter {
            return Ok(None);
        }
        let ty = self
            .request
            .state
            .get_type_of_symbol_at_location(symbol, Some(value_declaration))?;
        if self.is_module_reference_type(ty) {
            return Ok(None);
        }
        Ok(Some(self.type_to_inlay_hint_parts(ty)?))
    }

    /// tsgo `typeToInlayHintParts`: the type's node with the symbols its
    /// identifiers name, as label parts.
    fn type_to_inlay_hint_parts(&mut self, ty: TypeId) -> CheckResult<Vec<InlayHintLabelPart>> {
        let mut arena = TransformArena::new();
        let program_source = self.request.state.program_source_id(self.file);
        let target = arena.add_source(self.source(), Some(program_source));
        let built = self
            .request
            .state
            .type_to_type_node_in_arena(
                &mut arena,
                target,
                ty,
                None,
                INLAY_HINT_NODE_BUILDER_FLAGS,
                0,
            )
            .unwrap_or_else(|error| panic!("inlay hint type node: {error}"));
        let (node, identifiers) = built.expect("should always get typenode");
        Ok(self.inlay_hint_label_parts(&arena, node, &identifiers))
    }

    /// tsgo `typePredicateToInlayHintParts`.
    fn type_predicate_to_inlay_hint_parts(
        &mut self,
        predicate: &tsc_checker::narrow::TypePredicate,
    ) -> CheckResult<Vec<InlayHintLabelPart>> {
        let mut arena = TransformArena::new();
        let program_source = self.request.state.program_source_id(self.file);
        let target = arena.add_source(self.source(), Some(program_source));
        let built = self
            .request
            .state
            .type_predicate_to_type_predicate_node_in_arena(
                &mut arena,
                target,
                predicate,
                None,
                INLAY_HINT_NODE_BUILDER_FLAGS,
            )
            .unwrap_or_else(|error| panic!("inlay hint type predicate node: {error}"));
        let (node, identifiers) = built.expect("should always get typePredicateNode");
        Ok(self.inlay_hint_label_parts(&arena, node, &identifiers))
    }

    /// tsgo `addTypeHints`: `: ` and the parts, after a space.
    fn add_type_hints(&mut self, parts: Vec<InlayHintLabelPart>, position: usize) {
        let Some(position) = self.lsp_position(position) else {
            return;
        };
        let mut label = vec![label_part(": ")];
        label.extend(parts);
        self.result.push(InlayHint {
            position,
            label: StringOrInlayHintLabelParts::InlayHintLabelParts(label),
            kind: Some(InlayHintKind::TYPE),
            padding_left: Some(true),
            ..InlayHint::default()
        });
    }

    /// tsgo `addEnumMemberValueHints`.
    fn add_enum_member_value_hints(&mut self, text: &str, position: usize) {
        let Some(position) = self.lsp_position(position) else {
            return;
        };
        self.result.push(InlayHint {
            position,
            label: StringOrInlayHintLabelParts::String(format!("= {text}")),
            padding_left: Some(true),
            ..InlayHint::default()
        });
    }

    /// tsgo `addParameterHints`: the name (`...` before a rest parameter's)
    /// at its declaration, and `:`, before a space.
    fn add_parameter_hints(
        &mut self,
        text: &str,
        parameter: NodeId,
        position: usize,
        is_first_variadic_argument: bool,
    ) {
        let Some(position) = self.lsp_position(position) else {
            return;
        };
        let hint_text = format!(
            "{}{text}",
            if is_first_variadic_argument {
                "..."
            } else {
                ""
            }
        );
        let label = vec![
            self.node_display_part(hint_text, parameter),
            label_part(":"),
        ];
        self.result.push(InlayHint {
            position,
            label: StringOrInlayHintLabelParts::InlayHintLabelParts(label),
            kind: Some(InlayHintKind::PARAMETER),
            padding_right: Some(true),
            ..InlayHint::default()
        });
    }

    /// A hint's position, none where it does not map.
    fn lsp_position(&self, position: usize) -> Option<tsc_lsproto::Position> {
        let (position, fidelity) = self.request.converters.to_lsp_position_for_feature(
            &script_of(self.source()),
            position,
            FEATURE_INLAY_HINTS,
        );
        (fidelity != Fidelity::None).then_some(position)
    }

    /// tsgo `getNodeDisplayPart`: the text, with the location of the node.
    /// The location is an optional go-to target for the name. Only attach it when the name maps back to a
    /// single concrete span in the original text; an approximate or synthesized mapping would point the
    /// user somewhere wrong, so it is better to omit the target than to fabricate one.
    fn node_display_part(&self, text: String, node: NodeId) -> InlayHintLabelPart {
        let source = self.request.state.binder.source_of_node(node);
        let start = self.start(node);
        let end = self.request.state.binder.node_record(node).end as usize;
        let script = script_of(source);
        let (range, fidelity) = self.request.converters.to_lsp_range_for_feature(
            &script,
            start..end,
            FEATURE_INLAY_HINTS,
        );
        InlayHintLabelPart {
            value: text,
            location: fidelity.is_single_segment().then(|| Location {
                uri: tsc_lsproto::DocumentUri(tsc_program::uri::file_name_to_document_uri(
                    script.original_file_name(),
                )),
                range,
            }),
            ..InlayHintLabelPart::default()
        }
    }

    /// tsgo `getInlayHintLabelParts`: a built type node's text in parts, an
    /// identifier that names a declared symbol with the location of the
    /// declaration's name.
    fn inlay_hint_label_parts(
        &self,
        arena: &TransformArena,
        node: TransformNode,
        identifiers: &[(TransformNode, SymbolId)],
    ) -> Vec<InlayHintLabelPart> {
        let mut parts = Vec::new();
        LabelParts {
            hints: self,
            arena,
            identifiers,
            parts: &mut parts,
        }
        .visit(Some(node));
        parts
    }

    /// tsgo `getLiteralText`.
    fn literal_text(&self, data: &NodeData) -> String {
        match data {
            NodeData::StringLiteral(literal) => {
                let units = &literal.text.to_utf16();
                if self.quote_preference == QuotePreference::SINGLE {
                    format!(
                        "'{}'",
                        tsc_emitter::escape_string(units, tsc_emitter::LiteralQuote::Single)
                    )
                } else {
                    format!(
                        "\"{}\"",
                        tsc_emitter::escape_string(units, tsc_emitter::LiteralQuote::Double)
                    )
                }
            }
            NodeData::TemplateHead(literal) => {
                format!(
                    "`{}${{",
                    template_raw_text(&literal.raw_text, &literal.text)
                )
            }
            NodeData::TemplateMiddle(literal) => {
                format!(
                    "}}{}${{",
                    template_raw_text(&literal.raw_text, &literal.text)
                )
            }
            NodeData::TemplateTail(literal) => {
                format!("}}{}`", template_raw_text(&literal.raw_text, &literal.text))
            }
            _ => literal_node_text(data),
        }
    }

    /// tsgo `getParameterIdentifierInfoAtPosition`: the parameter at the
    /// position, or the rest parameter's (its tuple's labeled element's).
    fn parameter_identifier_info_at_position(
        &mut self,
        signature: tsc_checker::state::SignatureId,
        position: usize,
    ) -> CheckResult<Option<ParameterInfo>> {
        let record = self.request.state.signature_of(signature);
        let parameters = record.parameters.clone();
        let has_rest = record.flags.intersects(SignatureFlags::HAS_REST_PARAMETER);
        let parameter_count = parameters.len() - usize::from(has_rest);
        if position < parameter_count {
            let Some(identifier) = self.parameter_declaration_identifier(parameters[position])
            else {
                return Ok(None);
            };
            return Ok(Some(ParameterInfo {
                parameter: identifier,
                name: self.node_text(identifier),
                is_rest_parameter: false,
            }));
        }
        let rest = parameters.get(parameter_count).copied();
        let Some((rest, rest_identifier)) = rest.and_then(|rest| {
            self.parameter_declaration_identifier(rest)
                .map(|identifier| (rest, identifier))
        }) else {
            return Ok(None);
        };
        let state = &mut *self.request.state;
        let rest_type = state.get_type_of_symbol(rest)?;
        if state.tables.is_tuple_type(rest_type) {
            let target = state.tables.reference_target(rest_type);
            let labeled: Vec<Option<NodeId>> = match &state.tables.type_of(target).data {
                TypeData::TupleTarget(tuple) => match &tuple.labeled_element_declarations {
                    Some(names) => names.iter().map(|name| name.map(NodeId::new)).collect(),
                    None => vec![None; tuple.element_flags.len()],
                },
                _ => Vec::new(),
            };
            let index = position - parameter_count;
            if let Some(Some(element)) = labeled.get(index).copied() {
                let (name, is_rest) = match self.data(element) {
                    NodeData::NamedTupleMember(member) => {
                        (member.name, member.dot_dot_dot_token.is_some())
                    }
                    NodeData::Parameter(parameter) => {
                        (parameter.name, parameter.dot_dot_dot_token.is_some())
                    }
                    _ => (None, false),
                };
                if let Some(name) = name {
                    return Ok(Some(ParameterInfo {
                        parameter: name,
                        name: self.node_text(name),
                        is_rest_parameter: is_rest,
                    }));
                }
            }
            return Ok(None);
        }
        if position == parameter_count {
            let name = self
                .request
                .state
                .binder
                .symbol(rest)
                .escaped_name
                .unescape()
                .to_string_lossy()
                .into_owned();
            return Ok(Some(ParameterInfo {
                parameter: rest_identifier,
                name,
                is_rest_parameter: true,
            }));
        }
        Ok(None)
    }

    /// tsgo `getParameterDeclarationIdentifier`: the name of a parameter
    /// symbol's declaration, an identifier.
    fn parameter_declaration_identifier(&self, symbol: SymbolId) -> Option<NodeId> {
        let declaration = self.request.state.binder.symbol(symbol).value_declaration?;
        if self.kind(declaration) != SyntaxKind::Parameter {
            return None;
        }
        self.name_of(declaration)
            .filter(|&name| self.kind(name) == SyntaxKind::Identifier)
    }

    /// tsgo `identifierOrAccessExpressionPostfixMatchesParameterName`.
    fn identifier_or_access_expression_postfix_matches_parameter_name(
        &self,
        expression: NodeId,
        parameter_name: &str,
    ) -> bool {
        match self.data(expression) {
            NodeData::Identifier(_) => self.node_text(expression) == parameter_name,
            NodeData::PropertyAccessExpression(access) => access
                .name
                .is_some_and(|name| self.node_text(name) == parameter_name),
            _ => false,
        }
    }

    /// tsgo `leadingCommentsContainsParameterName`: a leading comment whose
    /// text (without space, slashes and stars) is the name.
    fn leading_comments_contain_parameter_name(&self, node: NodeId, name: &str) -> bool {
        let source = self.source();
        let jsx = source.language_variant == tsc_syntax::scanner::LanguageVariant::Jsx;
        if !crate::completions::is_identifier_text_of_variant(name, jsx) {
            return false;
        }
        if self.kind(node) == SyntaxKind::JsxText {
            return false;
        }
        let text = source.text();
        let pos = self.request.state.binder.node_record(node).pos as usize;
        tsc_syntax::comment_ranges(text, pos, false)
            .into_iter()
            .any(|(start, end)| {
                text[start..end].trim_matches(|c: char| c.is_whitespace() || c == '/' || c == '*')
                    == name
            })
    }

    /// tsgo `getTypeAnnotationPosition`: after the close paren, else the
    /// parameters.
    fn type_annotation_position(&self, declaration: NodeId) -> usize {
        let navigator = Navigator::new(self.source());
        if let Some(close) =
            navigator.child_of_kind(View::Node(declaration), SyntaxKind::CloseParenToken)
        {
            return navigator.end(close) as usize;
        }
        let source = self.source();
        crate::utilities::parameter_list_in(source, declaration).map_or(
            self.request.state.binder.node_record(declaration).end as usize,
            |list| source.arena.node_array(list).end as usize,
        )
    }

    /// tsgo `isHintableDeclaration`: not a parameter or `const` with a
    /// literal, `new`, object literal or assertion initializer.
    fn is_hintable_declaration(&self, node: NodeId) -> bool {
        let source = self.request.state.binder.source_of_node(node);
        let candidate = node_util::is_part_of_parameter_declaration(source, node)
            || self.kind(node) == SyntaxKind::VariableDeclaration && {
                // tsgo `ast.IsVarConst`: `const`, not `let` or `using`.
                let block_scope = node_util::get_combined_node_flags(source, node).bits()
                    & tsc_types::NodeFlags::BLOCK_SCOPED.bits();
                block_scope == tsc_types::NodeFlags::CONST.bits()
            };
        let Some(initializer) = node_util::initializer_of(source, node).filter(|_| candidate)
        else {
            return true;
        };
        let initializer = node_util::skip_parentheses_pub(source, initializer);
        !(self.is_hintable_literal(initializer)
            || matches!(
                self.kind(initializer),
                SyntaxKind::NewExpression
                    | SyntaxKind::ObjectLiteralExpression
                    | SyntaxKind::TypeAssertionExpression
                    | SyntaxKind::AsExpression
            ))
    }

    /// tsgo `isHintableLiteral`.
    fn is_hintable_literal(&self, node: NodeId) -> bool {
        match self.data(node) {
            NodeData::PrefixUnaryExpression(unary) => unary.operand.is_some_and(|operand| {
                is_literal_expression_kind(self.kind(operand))
                    || self.kind(operand) == SyntaxKind::Identifier
                        && is_infinity_or_nan_string(&self.node_text(operand))
            }),
            NodeData::Identifier(_) => {
                let name = self.node_text(node);
                name == "undefined" || is_infinity_or_nan_string(&name)
            }
            _ => {
                matches!(
                    self.kind(node),
                    SyntaxKind::TrueKeyword
                        | SyntaxKind::FalseKeyword
                        | SyntaxKind::NullKeyword
                        | SyntaxKind::NoSubstitutionTemplateLiteral
                        | SyntaxKind::TemplateExpression
                ) || is_literal_expression_kind(self.kind(node))
            }
        }
    }

    /// tsgo `ast.IsThisParameter`: a parameter named `this`.
    fn is_this_parameter(&self, parameter: NodeId) -> bool {
        self.name_of(parameter).is_some_and(
            |name| matches!(self.data(name), NodeData::Identifier(id) if id.escaped_text == "this"),
        )
    }

    /// tsgo `isModuleReferenceType`: the type of a module.
    fn is_module_reference_type(&self, ty: TypeId) -> bool {
        self.request
            .state
            .tables
            .type_of(ty)
            .symbol
            .is_some_and(|symbol| {
                self.request
                    .state
                    .symbol_flags(symbol)
                    .intersects(SymbolFlags::MODULE)
            })
    }

    /// tsgo `Node.Text`: an identifier's or literal's text.
    fn node_text(&self, node: NodeId) -> String {
        match self.data(node) {
            NodeData::Identifier(data) => {
                data.escaped_text.unescape().to_string_lossy().into_owned()
            }
            NodeData::PrivateIdentifier(data) => {
                data.escaped_text.unescape().to_string_lossy().into_owned()
            }
            data => literal_node_text(data),
        }
    }
}

/// tsgo `getInlayHintLabelParts`' walk over a built node.
struct LabelParts<'h, 'r, 's, 'a, 'p, 'x> {
    hints: &'h InlayHints<'r, 's, 'a, 'p>,
    arena: &'x TransformArena,
    identifiers: &'x [(TransformNode, SymbolId)],
    parts: &'x mut Vec<InlayHintLabelPart>,
}

impl LabelParts<'_, '_, '_, '_, '_, '_> {
    fn push(&mut self, value: &str) {
        self.parts.push(label_part(value));
    }

    fn child(&self, parent: TransformNode, child: Option<NodeId>) -> Option<TransformNode> {
        child.and_then(|child| self.arena.node_ref(parent.source(), child))
    }

    fn children(&self, parent: TransformNode, list: Option<NodeArrayId>) -> Vec<TransformNode> {
        let Some(list) = list else {
            return Vec::new();
        };
        let Ok(array) = self
            .arena
            .node_array(TransformNodeArray::new(parent.source(), list))
        else {
            return Vec::new();
        };
        array
            .nodes
            .iter()
            .filter_map(|&child| self.arena.node_ref(parent.source(), child))
            .collect()
    }

    /// tsgo `visitDisplayPartList`.
    fn visit_list(&mut self, nodes: Vec<TransformNode>, separator: &str) {
        for (index, node) in nodes.into_iter().enumerate() {
            if index > 0 {
                self.push(separator);
            }
            self.visit(Some(node));
        }
    }

    /// tsgo `visitParametersAndTypeParameters`.
    fn visit_parameters_and_type_parameters(
        &mut self,
        node: TransformNode,
        type_parameters: Option<NodeArrayId>,
        parameters: Option<NodeArrayId>,
    ) {
        let type_parameters = self.children(node, type_parameters);
        if !type_parameters.is_empty() {
            self.push("<");
            self.visit_list(type_parameters, ", ");
            self.push(">");
        }
        self.push("(");
        let parameters = self.children(node, parameters);
        self.visit_list(parameters, ", ");
        self.push(")");
    }

    /// tsgo `visitForDisplayParts`.
    fn visit(&mut self, node: Option<TransformNode>) {
        let Some(node) = node else {
            return;
        };
        let Ok(record) = self.arena.node(node) else {
            return;
        };
        let kind = record.kind;
        if let Some(token) = tsc_syntax::tokens::token_to_string(kind) {
            self.push(token);
            return;
        }
        if is_literal_expression_kind(kind) {
            let text = self.hints.literal_text(&record.data);
            self.parts.push(label_part(&text));
            return;
        }
        if kind == SyntaxKind::ThisType {
            self.push("this");
            return;
        }
        let data = record.data.clone();
        match data {
            NodeData::Identifier(identifier) => {
                let text = identifier
                    .escaped_text
                    .unescape()
                    .to_string_lossy()
                    .into_owned();
                let name = self
                    .identifiers
                    .iter()
                    .find(|(id, _)| *id == node)
                    .and_then(|&(_, symbol)| {
                        let first = *self
                            .hints
                            .request
                            .state
                            .binder
                            .symbol(symbol)
                            .declarations
                            .first()?;
                        node_util::get_name_of_declaration(
                            self.hints.request.state.binder.source_of_node(first),
                            first,
                        )
                    });
                match name {
                    Some(name) => {
                        let part = self.hints.node_display_part(text, name);
                        self.parts.push(part);
                    }
                    None => self.parts.push(label_part(&text)),
                }
            }
            NodeData::QualifiedName(name) => {
                self.visit(self.child(node, name.left));
                self.push(".");
                self.visit(self.child(node, name.right));
            }
            NodeData::TypePredicate(predicate) => {
                if predicate.asserts_modifier.is_some() {
                    self.push("asserts ");
                }
                self.visit(self.child(node, predicate.parameter_name));
                if predicate.r#type.is_some() {
                    self.push(" is ");
                    self.visit(self.child(node, predicate.r#type));
                }
            }
            NodeData::TypeReference(reference) => {
                self.visit(self.child(node, reference.type_name));
                let arguments = self.children(node, reference.type_arguments);
                if !arguments.is_empty() {
                    self.push("<");
                    self.visit_list(arguments, ",");
                    self.push(">");
                }
            }
            NodeData::TypeParameter(parameter) => {
                let modifiers = self.children(node, parameter.modifiers);
                if !modifiers.is_empty() {
                    self.visit_list(modifiers, "");
                }
                self.visit(self.child(node, parameter.name));
                if parameter.constraint.is_some() {
                    self.push(" extends ");
                    self.visit(self.child(node, parameter.constraint));
                }
                if parameter.default.is_some() {
                    self.push(" = ");
                    self.visit(self.child(node, parameter.default));
                }
            }
            NodeData::Parameter(parameter) => {
                let modifiers = self.children(node, parameter.modifiers);
                if !modifiers.is_empty() {
                    self.visit_list(modifiers, " ");
                }
                if parameter.dot_dot_dot_token.is_some() {
                    self.push("...");
                }
                self.visit(self.child(node, parameter.name));
                if parameter.question_token.is_some() {
                    self.push("?");
                }
                if parameter.r#type.is_some() {
                    self.push(": ");
                    self.visit(self.child(node, parameter.r#type));
                }
            }
            NodeData::ConstructorType(constructor) => {
                self.push("new ");
                self.visit_parameters_and_type_parameters(
                    node,
                    constructor.type_parameters,
                    constructor.parameters,
                );
                self.push(" => ");
                self.visit(self.child(node, constructor.r#type));
            }
            NodeData::TypeQuery(query) => {
                self.push("typeof ");
                self.visit(self.child(node, query.expr_name));
                let arguments = self.children(node, query.type_arguments);
                if !arguments.is_empty() {
                    self.push("<");
                    self.visit_list(arguments, ", ");
                    self.push(">");
                }
            }
            NodeData::TypeLiteral(literal) => {
                self.push("{");
                let members = self.children(node, literal.members);
                if !members.is_empty() {
                    self.push(" ");
                    self.visit_list(members, "; ");
                    self.push(" ");
                }
                self.push("}");
            }
            NodeData::ArrayType(array) => {
                self.visit(self.child(node, array.element_type));
                self.push("[]");
            }
            NodeData::TupleType(tuple) => {
                self.push("[");
                let elements = self.children(node, tuple.elements);
                self.visit_list(elements, ", ");
                self.push("]");
            }
            NodeData::NamedTupleMember(member) => {
                if member.dot_dot_dot_token.is_some() {
                    self.push("...");
                }
                self.visit(self.child(node, member.name));
                if member.question_token.is_some() {
                    self.push("?");
                }
                self.push(": ");
                self.visit(self.child(node, member.r#type));
            }
            NodeData::OptionalType(optional) => {
                self.visit(self.child(node, optional.r#type));
                self.push("?");
            }
            NodeData::RestType(rest) => {
                self.push("...");
                self.visit(self.child(node, rest.r#type));
            }
            NodeData::UnionType(union) => {
                let types = self.children(node, union.types);
                self.visit_list(types, " | ");
            }
            NodeData::IntersectionType(intersection) => {
                let types = self.children(node, intersection.types);
                self.visit_list(types, " & ");
            }
            NodeData::ConditionalType(conditional) => {
                self.visit(self.child(node, conditional.check_type));
                self.push(" extends ");
                self.visit(self.child(node, conditional.extends_type));
                self.push(" ? ");
                self.visit(self.child(node, conditional.true_type));
                self.push(" : ");
                self.visit(self.child(node, conditional.false_type));
            }
            NodeData::InferType(infer) => {
                self.push("infer ");
                self.visit(self.child(node, infer.type_parameter));
            }
            NodeData::ParenthesizedType(parenthesized) => {
                self.push("(");
                self.visit(self.child(node, parenthesized.r#type));
                self.push(")");
            }
            NodeData::TypeOperator(operator) => {
                self.push(tsc_syntax::tokens::token_to_string(operator.operator).unwrap_or(""));
                self.visit(self.child(node, operator.r#type));
            }
            NodeData::IndexedAccessType(access) => {
                self.visit(self.child(node, access.object_type));
                self.push("[");
                self.visit(self.child(node, access.index_type));
                self.push("]");
            }
            NodeData::MappedType(mapped) => {
                self.push("{ ");
                if let Some(readonly) = self.child(node, mapped.readonly_token) {
                    match self.arena.node(readonly).map(|record| record.kind) {
                        Ok(SyntaxKind::PlusToken) => self.push("+"),
                        Ok(SyntaxKind::MinusToken) => self.push("-"),
                        _ => {}
                    }
                    self.push("readonly ");
                }
                self.push("[");
                self.visit(self.child(node, mapped.type_parameter));
                if mapped.name_type.is_some() {
                    self.push(" as ");
                    self.visit(self.child(node, mapped.name_type));
                }
                self.push("]");
                if let Some(question) = self.child(node, mapped.question_token) {
                    match self.arena.node(question).map(|record| record.kind) {
                        Ok(SyntaxKind::PlusToken) => self.push("+"),
                        Ok(SyntaxKind::MinusToken) => self.push("-"),
                        _ => {}
                    }
                    self.push("?");
                }
                self.push(": ");
                if mapped.r#type.is_some() {
                    self.visit(self.child(node, mapped.r#type));
                }
                self.push("; }");
            }
            NodeData::LiteralType(literal) => self.visit(self.child(node, literal.literal)),
            NodeData::FunctionType(function) => {
                self.visit_parameters_and_type_parameters(
                    node,
                    function.type_parameters,
                    function.parameters,
                );
                self.push(" => ");
                self.visit(self.child(node, function.r#type));
            }
            NodeData::ImportType(import) => {
                if import.is_type_of {
                    self.push("typeof ");
                }
                self.push("import(");
                self.visit(self.child(node, import.argument));
                self.push(")");
                if import.qualifier.is_some() {
                    self.push(".");
                    self.visit(self.child(node, import.qualifier));
                }
                let arguments = self.children(node, import.type_arguments);
                if !arguments.is_empty() {
                    self.push("<");
                    self.visit_list(arguments, ", ");
                    self.push(">");
                }
            }
            NodeData::PropertySignature(property) => {
                let modifiers = self.children(node, property.modifiers);
                if !modifiers.is_empty() {
                    self.visit_list(modifiers, " ");
                    self.push(" ");
                }
                self.visit(self.child(node, property.name));
                if let Some(postfix) = self.child(node, property.question_token) {
                    if let Ok(record) = self.arena.node(postfix) {
                        self.push(tsc_syntax::tokens::token_to_string(record.kind).unwrap_or(""));
                    }
                }
                if property.r#type.is_some() {
                    self.push(": ");
                    self.visit(self.child(node, property.r#type));
                }
            }
            NodeData::IndexSignature(signature) => {
                self.push("[");
                let parameters = self.children(node, signature.parameters);
                self.visit_list(parameters, ", ");
                self.push("]");
                if signature.r#type.is_some() {
                    self.push(": ");
                    self.visit(self.child(node, signature.r#type));
                }
            }
            NodeData::MethodSignature(method) => {
                let modifiers = self.children(node, method.modifiers);
                if !modifiers.is_empty() {
                    self.visit_list(modifiers, " ");
                    self.push(" ");
                }
                self.visit(self.child(node, method.name));
                if let Some(postfix) = self.child(node, method.question_token) {
                    if let Ok(record) = self.arena.node(postfix) {
                        self.push(tsc_syntax::tokens::token_to_string(record.kind).unwrap_or(""));
                    }
                }
                self.visit_parameters_and_type_parameters(
                    node,
                    method.type_parameters,
                    method.parameters,
                );
                if method.r#type.is_some() {
                    self.push(": ");
                    self.visit(self.child(node, method.r#type));
                }
            }
            NodeData::CallSignature(signature) => {
                self.visit_parameters_and_type_parameters(
                    node,
                    signature.type_parameters,
                    signature.parameters,
                );
                if signature.r#type.is_some() {
                    self.push(": ");
                    self.visit(self.child(node, signature.r#type));
                }
            }
            NodeData::ConstructSignature(signature) => {
                self.push("new ");
                self.visit_parameters_and_type_parameters(
                    node,
                    signature.type_parameters,
                    signature.parameters,
                );
                if signature.r#type.is_some() {
                    self.push(": ");
                    self.visit(self.child(node, signature.r#type));
                }
            }
            NodeData::ArrayBindingPattern(pattern) => {
                self.push("[");
                let elements = self.children(node, pattern.elements);
                self.visit_list(elements, ", ");
                self.push("]");
            }
            NodeData::ObjectBindingPattern(pattern) => {
                self.push("{");
                let elements = self.children(node, pattern.elements);
                if !elements.is_empty() {
                    self.push(" ");
                    self.visit_list(elements, ", ");
                    self.push(" ");
                }
                self.push("}");
            }
            NodeData::BindingElement(element) => self.visit(self.child(node, element.name)),
            NodeData::PrefixUnaryExpression(unary) => {
                self.push(tsc_syntax::tokens::token_to_string(unary.operator).unwrap_or(""));
                self.visit(self.child(node, unary.operand));
            }
            NodeData::TemplateLiteralType(template) => {
                self.visit(self.child(node, template.head));
                for span in self.children(node, template.template_spans) {
                    self.visit(Some(span));
                }
            }
            NodeData::TemplateLiteralTypeSpan(span) => {
                self.visit(self.child(node, span.r#type));
                self.visit(self.child(node, span.literal));
            }
            NodeData::TemplateHead(_) | NodeData::TemplateMiddle(_) | NodeData::TemplateTail(_) => {
                let text = self.hints.literal_text(&data);
                self.parts.push(label_part(&text));
            }
            NodeData::ComputedPropertyName(computed) => {
                self.push("[");
                self.visit(self.child(node, computed.expression));
                self.push("]");
            }
            NodeData::PropertyAccessExpression(access) => {
                self.visit(self.child(node, access.expression));
                self.push(".");
                self.visit(self.child(node, access.name));
            }
            NodeData::ElementAccessExpression(access) => {
                self.visit(self.child(node, access.expression));
                self.push("[");
                self.visit(self.child(node, access.argument_expression));
                self.push("]");
            }
            // tsgo fails on any other kind (debug.FailBadSyntaxKind).
            _ => {}
        }
    }
}

fn label_part(value: &str) -> InlayHintLabelPart {
    InlayHintLabelPart {
        value: value.to_owned(),
        ..InlayHintLabelPart::default()
    }
}

/// A template literal part's raw text, else its text escaped for a
/// backtick.
fn template_raw_text(raw_text: &Option<String>, text: &tsc_types::JsString) -> String {
    match raw_text {
        Some(raw) if !raw.is_empty() => raw.clone(),
        _ => tsc_emitter::escape_string(&text.to_utf16(), tsc_emitter::LiteralQuote::Backtick),
    }
}

/// tsgo `Node.Text` of a literal.
fn literal_node_text(data: &NodeData) -> String {
    match data {
        NodeData::NumericLiteral(literal) => literal.text.clone(),
        NodeData::BigIntLiteral(literal) => literal.text.clone(),
        NodeData::StringLiteral(literal) => literal.text.to_string_lossy().into_owned(),
        NodeData::NoSubstitutionTemplateLiteral(literal) => {
            literal.text.to_string_lossy().into_owned()
        }
        NodeData::RegularExpressionLiteral(literal) => literal.text.clone(),
        _ => String::new(),
    }
}

/// tsgo `ast.IsLiteralExpression` (`IsLiteralKind`).
fn is_literal_expression_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::StringLiteral
            | SyntaxKind::JsxText
            | SyntaxKind::JsxTextAllWhiteSpaces
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
    )
}

/// tsgo `ast.IsInfinityOrNaNString`.
fn is_infinity_or_nan_string(name: &str) -> bool {
    matches!(name, "Infinity" | "-Infinity" | "NaN")
}

/// tsgo `stringutil.EquateStringCaseInsensitive`.
fn equate_string_case_insensitive(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b) || a.to_lowercase() == b.to_lowercase()
}

/// tsgo `isAnyInlayHintEnabled`.
fn is_any_inlay_hint_enabled(preferences: &InlayHintsPreferences) -> bool {
    preferences.include_inlay_parameter_name_hints != IncludeInlayParameterNameHints::NONE
        || preferences.include_inlay_function_parameter_type_hints == Some(true)
        || preferences.include_inlay_variable_type_hints == Some(true)
        || preferences.include_inlay_property_declaration_type_hints == Some(true)
        || preferences.include_inlay_function_like_return_type_hints == Some(true)
        || preferences.include_inlay_enum_member_value_hints == Some(true)
}

/// tsgo `shouldShowParameterNameHints`.
fn should_show_parameter_name_hints(preferences: &InlayHintsPreferences) -> bool {
    preferences.include_inlay_parameter_name_hints == IncludeInlayParameterNameHints::LITERALS
        || preferences.include_inlay_parameter_name_hints == IncludeInlayParameterNameHints::ALL
}

/// tsgo `shouldShowLiteralParameterNameHintsOnly`.
fn should_show_literal_parameter_name_hints_only(preferences: &InlayHintsPreferences) -> bool {
    preferences.include_inlay_parameter_name_hints == IncludeInlayParameterNameHints::LITERALS
}

/// tsgo `isSignatureSupportingReturnAnnotation`.
fn is_signature_supporting_return_annotation(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionExpression
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
    )
}
