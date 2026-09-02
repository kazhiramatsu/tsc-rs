use tsc_syntax::{NodeData, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, TransformError, TransformFlags,
    TransformNode, TransformNodeArray, TransformationContext, UnsupportedEmitFeature,
};

use super::diagnostics::{can_produce_diagnostics, effective_modifier_flags, DiagnosticContext};
use super::state::VisitResult;
use super::tracker::materialize_effects;
use super::DeclarationTransformer;

impl DeclarationTransformer<'_> {
    /// tsc-port: filterBindingPatternInitializers @6.0.3
    /// tsc-hash: 8e453cf427400e9e12fb4126c79492e6fd9d0a5e0b38a9ce183cf2d933f41994
    /// tsc-span: _tsc.js:114615-114641
    pub(crate) fn filter_binding_pattern_initializers(
        &mut self,
        cx: &mut TransformationContext,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        match self.kind(cx, name)? {
            SyntaxKind::Identifier => Ok(name),
            SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern => {
                let elements = match &cx.arena().node(name)?.data {
                    NodeData::ArrayBindingPattern(data) => data.elements,
                    NodeData::ObjectBindingPattern(data) => data.elements,
                    _ => None,
                };
                let mut visited = Vec::new();
                if let Some(elements) =
                    elements.and_then(|array| cx.arena().node_array_ref(name.source(), array))
                {
                    for &element in &cx.arena().node_array(elements)?.nodes.clone() {
                        let element = TransformNode::new(name.source(), element);
                        if self.kind(cx, element)? == SyntaxKind::OmittedExpression {
                            visited.push(element);
                        } else {
                            visited.push(self.visit_binding_element(cx, element)?);
                        }
                    }
                }
                update_binding_pattern(cx, name, visited)
            }
            _ => Err(Self::contract(
                "filterBindingPatternInitializers received a non-binding name",
            )),
        }
    }

    /// tsc-port: visitBindingElement @6.0.3
    /// tsc-hash: 26901262dc5e47bde89ea1764e0e824f19878f4803d51e59b87f44e7b9040698
    /// tsc-span: _tsc.js:114625-114640
    pub(crate) fn visit_binding_element(
        &mut self,
        cx: &mut TransformationContext,
        element: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if self.kind(cx, element)? == SyntaxKind::OmittedExpression {
            return Ok(element);
        }
        let NodeData::BindingElement(data) = cx.arena().node(element)?.data.clone() else {
            return Err(Self::contract(
                "visitBindingElement received a non-binding element",
            ));
        };
        if let Some(property_name) = data
            .property_name
            .and_then(|node| cx.arena().node_ref(element.source(), node))
        {
            if let NodeData::ComputedPropertyName(computed) = &cx.arena().node(property_name)?.data
            {
                if let Some(expression) = computed
                    .expression
                    .and_then(|node| cx.arena().node_ref(element.source(), node))
                {
                    if self.is_entity_name_expression(cx, expression)? {
                        let enclosing = self.state()?.enclosing_declaration.ok_or_else(|| {
                            Self::contract("binding name has no enclosing declaration")
                        })?;
                        self.check_entity_name_visibility(cx, expression, enclosing)?;
                    }
                }
            }
        }
        let name = data
            .name
            .and_then(|node| cx.arena().node_ref(element.source(), node))
            .ok_or_else(|| Self::contract("binding element has no name"))?;
        let name = self.filter_binding_pattern_initializers(cx, name)?;
        let dot = data
            .dot_dot_dot_token
            .and_then(|node| cx.arena().node_ref(element.source(), node));
        let property = data
            .property_name
            .and_then(|node| cx.arena().node_ref(element.source(), node));
        cx.factory()?
            .update_binding_element(element, dot, property, name, None)
    }

    /// tsc-port: ensureParameter @6.0.3
    /// tsc-hash: 6fe1d4897058854f9e607716f7033a8d0e091ce997f48960b14ca9e990e901f9
    /// tsc-span: _tsc.js:114642-114666
    pub(crate) fn ensure_parameter(
        &mut self,
        cx: &mut TransformationContext,
        parameter: TransformNode,
        modifier_mask: ModifierFlags,
    ) -> Result<TransformNode, TransformError> {
        let saved_diag =
            if self.tracker.suppress_new_diagnostic_contexts {
                None
            } else {
                Some(self.tracker.replace_diagnostic_context(
                    cx.arena(),
                    DiagnosticContext::ForNode(parameter),
                )?)
            };
        let result = (|| {
            let NodeData::Parameter(data) = cx.arena().node(parameter)?.data.clone() else {
                return Err(Self::contract("ensureParameter received a non-parameter"));
            };
            let name = data
                .name
                .and_then(|node| cx.arena().node_ref(parameter.source(), node))
                .ok_or_else(|| Self::contract("parameter has no name"))?;
            let name = self.filter_binding_pattern_initializers(cx, name)?;
            let modifiers = mask_modifiers(cx, parameter, modifier_mask, ModifierFlags::NONE)?;
            let dot = data
                .dot_dot_dot_token
                .and_then(|node| cx.arena().node_ref(parameter.source(), node));
            let question = if self
                .resolver
                .is_optional_parameter(self.required_resolver_node(cx, parameter)?)?
            {
                match data
                    .question_token
                    .and_then(|node| cx.arena().node_ref(parameter.source(), node))
                {
                    Some(question) => Some(question),
                    None => Some(cx.factory()?.create_token(
                        parameter.source(),
                        SyntaxKind::QuestionToken,
                        TransformFlags::CONTAINS_TYPE_SCRIPT,
                    )?),
                }
            } else {
                None
            };
            let r#type = self.ensure_type(cx, parameter, true)?;
            let initializer = self.ensure_no_initializer(cx, parameter)?;
            cx.factory()?.update_parameter_declaration(
                parameter,
                modifiers,
                dot,
                name,
                question,
                r#type,
                initializer,
            )
        })();
        if let Some(saved) = saved_diag {
            self.tracker.restore_diagnostic_context(saved);
        }
        result
    }

    /// tsc-port: shouldPrintWithInitializer @6.0.3
    /// tsc-hash: 916dbad9da8df1a1419a931b2f5e09e44492f512d7f255c10bab54494c6d3c04
    /// tsc-span: _tsc.js:114667-114669
    pub(crate) fn should_print_with_initializer(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        if !can_have_literal_initializer(cx, node)? || initializer(cx, node)?.is_none() {
            return Ok(false);
        }
        Ok(self
            .resolver
            .is_literal_const_declaration(self.required_resolver_node(cx, node)?)?)
    }

    /// tsc-port: ensureNoInitializer @6.0.3
    /// tsc-hash: 0fbe7a578a59fdc3bbe5f2abf7125d0fb531a5548bbb78fa66e079e5419265a2
    /// tsc-span: _tsc.js:114670-114679
    pub(crate) fn ensure_no_initializer(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        if !self.should_print_with_initializer(cx, node)? {
            return Ok(None);
        }
        let initial = initializer(cx, node)?
            .ok_or_else(|| Self::contract("literal initializer predicate lost its initializer"))?;
        let unwrapped = unwrap_parenthesized(cx, initial)?;
        if !is_primitive_literal_value(cx, unwrapped)?
            && self.options.isolated_declarations == Some(true)
            && !current_source_is_js(cx, node.source())?
        {
            return Err(TransformError::Unsupported(
                UnsupportedEmitFeature::IsolatedDeclarations,
            ));
        }
        let resolver_node = self.required_resolver_node(cx, node)?;
        let target = self.state()?.current_source_file;
        let result = self.resolver.create_literal_const_value(
            cx.arena_mut()?,
            target,
            resolver_node,
            &mut self.tracker,
        );
        let effects = self.tracker.take_pending_effects();
        materialize_effects(cx, self.host, effects)?;
        let transformed = result.map_err(TransformError::from)?;
        Ok(Some(transformed))
    }

    /// tsc-port: ensureType @6.0.3
    /// tsc-hash: da5f04e5313775c6082c9208710e736a7db6e7d19ddcb8998988aef11c6164ab
    /// tsc-span: _tsc.js:114680-114712
    pub(crate) fn ensure_type(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
        ignore_private: bool,
    ) -> Result<Option<TransformNode>, TransformError> {
        if !ignore_private && self.has_effective_modifier(cx, node, ModifierFlags::PRIVATE)? {
            return Ok(None);
        }
        if self.should_print_with_initializer(cx, node)? {
            return Ok(None);
        }
        if !matches!(
            self.kind(cx, node)?,
            SyntaxKind::ExportAssignment | SyntaxKind::BindingElement
        ) {
            if let Some(explicit) = type_annotation(cx, node)? {
                let needs_undefined = if self.kind(cx, node)? == SyntaxKind::Parameter {
                    self.resolver.requires_adding_implicit_undefined(
                        self.required_resolver_node(cx, node)?,
                        self.state()?
                            .enclosing_declaration
                            .map(|enclosing| self.required_resolver_node(cx, enclosing))
                            .transpose()?,
                    )?
                } else {
                    false
                };
                if !needs_undefined {
                    return match self.visit_declaration_subtree(cx, explicit)? {
                        VisitResult::None => Ok(None),
                        VisitResult::Node(node) => Ok(Some(node)),
                        VisitResult::Nodes(_) => Err(Self::contract(
                            "type-node visitor returned a statement array",
                        )),
                    };
                }
            }
        }

        let old_error_name =
            std::mem::replace(&mut self.tracker.error_name_node, node_name(cx, node)?);
        let saved_diag = if self.tracker.suppress_new_diagnostic_contexts
            || !can_produce_diagnostics(self.kind(cx, node)?)
        {
            None
        } else {
            match self
                .tracker
                .replace_diagnostic_context(cx.arena(), DiagnosticContext::ForNode(node))
            {
                Ok(saved) => Some(saved),
                Err(error) => {
                    self.tracker.error_name_node = old_error_name;
                    return Err(error);
                }
            }
        };
        let result = (|| {
            let declaration = self.required_resolver_node(cx, node)?;
            let enclosing = self.current_enclosing_resolver_node(cx)?;
            let target = self.state()?.current_source_file;
            let out = if has_inferred_type(self.kind(cx, node)?) {
                self.resolver.create_type_of_declaration(
                    cx.arena_mut()?,
                    target,
                    declaration,
                    enclosing,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut self.tracker,
                )
            } else if is_function_like(self.kind(cx, node)?) {
                self.resolver.create_return_type_of_signature_declaration(
                    cx.arena_mut()?,
                    target,
                    declaration,
                    enclosing,
                    EmitNodeBuilderFlags::DECLARATION_EMIT,
                    EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                    &mut self.tracker,
                )
            } else {
                return Err(Self::contract("ensureType received an unhandled node kind"));
            }
            .map_err(TransformError::from);
            let effects = self.tracker.take_pending_effects();
            materialize_effects(cx, self.host, effects)?;
            let out = out?;
            match out {
                Some(node) => Ok(Some(node)),
                None => Ok(Some(
                    cx.factory()?
                        .create_keyword_type_node(node.source(), SyntaxKind::AnyKeyword)?,
                )),
            }
        })();
        self.tracker.error_name_node = old_error_name;
        if let Some(saved) = saved_diag {
            self.tracker.restore_diagnostic_context(saved);
        }
        result
    }

    /// tsc-port: updateParamsList @6.0.3
    /// tsc-hash: 997802071013ec7d85d78ab9faec2e6c746d997d72897f05468884daf37d358a
    /// tsc-span: _tsc.js:114754-114763
    pub(crate) fn update_params_list(
        &mut self,
        cx: &mut TransformationContext,
        owner: TransformNode,
        parameters: Option<tsc_syntax::NodeArrayId>,
        modifier_mask: ModifierFlags,
    ) -> Result<TransformNodeArray, TransformError> {
        if self.has_effective_modifier(cx, owner, ModifierFlags::PRIVATE)? {
            return cx.factory()?.create_node_array(owner.source(), Vec::new());
        }
        let mut updated = Vec::new();
        let mut has_trailing_comma = false;
        if let Some(parameters) =
            parameters.and_then(|array| cx.arena().node_array_ref(owner.source(), array))
        {
            has_trailing_comma = cx.arena().node_array(parameters)?.has_trailing_comma;
            for &parameter in &cx.arena().node_array(parameters)?.nodes.clone() {
                updated.push(self.ensure_parameter(
                    cx,
                    TransformNode::new(owner.source(), parameter),
                    modifier_mask,
                )?);
            }
        }
        cx.factory()?.create_node_array_with_trailing_comma(
            owner.source(),
            updated,
            has_trailing_comma,
        )
    }

    /// tsc-port: updateAccessorParamsList @6.0.3
    /// tsc-hash: 2d28717c51a39657a91a639c5c0c6b8bdb65c25240aec4abfb393602312bbbf2
    /// tsc-span: _tsc.js:114764-114792
    pub(crate) fn update_accessor_params_list(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        is_private: bool,
    ) -> Result<TransformNodeArray, TransformError> {
        let parameters = parameters(cx, input)?;
        let mut updated = Vec::new();
        if !is_private {
            if let Some(this_parameter) = parameters.iter().copied().find(|parameter| {
                node_name(cx, *parameter)
                    .ok()
                    .flatten()
                    .is_some_and(|name| identifier_text(cx, name).as_deref() == Some("this"))
            }) {
                updated.push(self.ensure_parameter(
                    cx,
                    this_parameter,
                    ModifierFlags::from_bits(
                        ModifierFlags::ALL.bits() ^ ModifierFlags::PUBLIC.bits(),
                    ),
                )?);
            }
        }
        if self.kind(cx, input)? == SyntaxKind::SetAccessor {
            let value = if is_private {
                None
            } else {
                parameters.into_iter().find(|parameter| {
                    node_name(cx, *parameter)
                        .ok()
                        .flatten()
                        .is_none_or(|name| identifier_text(cx, name).as_deref() != Some("this"))
                })
            };
            let value = match value {
                Some(value) => self.ensure_parameter(
                    cx,
                    value,
                    ModifierFlags::from_bits(
                        ModifierFlags::ALL.bits() ^ ModifierFlags::PUBLIC.bits(),
                    ),
                )?,
                None => {
                    let name = cx.factory()?.create_identifier(input.source(), "value")?;
                    cx.factory()?.create_parameter_declaration(
                        input.source(),
                        None,
                        None,
                        name,
                        None,
                        None,
                        None,
                    )?
                }
            };
            updated.push(value);
        }
        cx.factory()?.create_node_array(input.source(), updated)
    }

    /// tsc-port: ensureTypeParams @6.0.3
    /// tsc-hash: fe24e563b0cd9be3395047ca98f3eab59cf09a0e6f519c670470c2a42efdb704
    /// tsc-span: _tsc.js:114793-114795
    pub(crate) fn ensure_type_params(
        &mut self,
        cx: &mut TransformationContext,
        owner: TransformNode,
        parameters: Option<tsc_syntax::NodeArrayId>,
    ) -> Result<Option<TransformNodeArray>, TransformError> {
        if self.has_effective_modifier(cx, owner, ModifierFlags::PRIVATE)? {
            return Ok(None);
        }
        self.visit_type_node_array(cx, owner.source(), parameters, SyntaxKind::TypeParameter)
    }

    /// tsc-port: ensureModifiers @6.0.3
    /// tsc-hash: af4b842776153d6a061f36040229c72c99088b8db706c53fd6ff34561c8e625e
    /// tsc-span: _tsc.js:115769-115776
    pub(crate) fn ensure_modifiers(
        &self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<Option<TransformNodeArray>, TransformError> {
        let current = self.effective_modifier_flags(cx, node)?;
        let ensured = self.ensure_modifier_flags(cx, node)?;
        if current == ensured {
            return modifier_array(cx, node);
        }
        cx.factory()?
            .create_modifiers_from_modifier_flags(node.source(), ensured)
    }

    /// tsc-port: ensureModifierFlags @6.0.3
    /// tsc-hash: 49183e0c061b35643f66d3b9c8fd87c25a5c151536df8d5daa26b5b14f89eba9
    /// tsc-span: _tsc.js:115777-115786
    pub(crate) fn ensure_modifier_flags(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<ModifierFlags, TransformError> {
        let mask = ModifierFlags::from_bits(
            ModifierFlags::ALL.bits()
                ^ (ModifierFlags::PUBLIC.bits()
                    | ModifierFlags::ASYNC.bits()
                    | ModifierFlags::OVERRIDE.bits()),
        );
        let additions = if self.state()?.needs_declare && !is_always_type(cx, node)? {
            ModifierFlags::AMBIENT
        } else {
            ModifierFlags::NONE
        };
        let parent_is_file = self.parent(cx, node)?.is_some_and(|parent| {
            cx.arena()
                .node(parent)
                .is_ok_and(|node| node.kind == SyntaxKind::SourceFile)
        });
        let (mask, additions) = if !parent_is_file {
            (
                ModifierFlags::from_bits(mask.bits() ^ ModifierFlags::AMBIENT.bits()),
                ModifierFlags::NONE,
            )
        } else {
            debug_assert!(!self.state()?.is_bundled_emit);
            (mask, additions)
        };
        mask_modifier_flags(cx, node, mask, additions)
    }

    /// tsc-port: shouldStripInternal @6.0.3
    /// tsc-hash: 8d56548f2b699d81843826afaefff59717cef8a31e1d8a1adf665a150777f788
    /// tsc-span: _tsc.js:115760-115762
    pub(crate) fn should_strip_internal(
        &self,
        _cx: &TransformationContext,
        node: Option<TransformNode>,
    ) -> Result<bool, TransformError> {
        if self.options.strip_internal != Some(true) || node.is_none() {
            return Ok(false);
        }
        Err(TransformError::UnsupportedCompilerOption {
            option: "stripInternal",
            detail: "H2.7c owns declaration internal-tag filtering",
        })
    }

    fn effective_modifier_flags(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<ModifierFlags, TransformError> {
        let source = cx.arena().source(node.source())?.syntax();
        Ok(effective_modifier_flags(source, node.node()))
    }

    /// tsrs-native: declaration-local effective modifier predicate.
    pub(crate) fn has_effective_modifier(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
        flags: ModifierFlags,
    ) -> Result<bool, TransformError> {
        Ok(self.effective_modifier_flags(cx, node)?.intersects(flags))
    }

    /// tsrs-native: typed declaration-subtree node-array visitor shared by
    /// ensure and import-type rewriting.
    pub(crate) fn visit_type_node_array(
        &mut self,
        cx: &mut TransformationContext,
        source: crate::TransformSourceId,
        nodes: Option<tsc_syntax::NodeArrayId>,
        required_kind: SyntaxKind,
    ) -> Result<Option<TransformNodeArray>, TransformError> {
        let Some(original) = nodes.and_then(|array| cx.arena().node_array_ref(source, array))
        else {
            return Ok(None);
        };
        let mut output = Vec::new();
        for &node in &cx.arena().node_array(original)?.nodes.clone() {
            let node = TransformNode::new(source, node);
            match self.visit_declaration_subtree(cx, node)? {
                VisitResult::None => {}
                VisitResult::Node(node)
                    if required_kind == SyntaxKind::Unknown
                        || self.kind(cx, node)? == required_kind =>
                {
                    output.push(node)
                }
                VisitResult::Node(node) => {
                    return Err(TransformError::UnexpectedChildKind {
                        parent: required_kind,
                        field: "declaration type list",
                        actual: self.kind(cx, node)?,
                    })
                }
                VisitResult::Nodes(_) => {
                    return Err(Self::contract(
                        "type-node list visitor returned a statement array",
                    ))
                }
            }
        }
        Ok(Some(cx.factory()?.update_node_array(original, output)?))
    }

    /// tsrs-native: arena-aware entity-name expression predicate.
    pub(crate) fn is_entity_name_expression(
        &self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<bool, TransformError> {
        match self.kind(cx, node)? {
            SyntaxKind::Identifier => Ok(true),
            SyntaxKind::PropertyAccessExpression => {
                let NodeData::PropertyAccessExpression(data) = &cx.arena().node(node)?.data else {
                    return Ok(false);
                };
                if !data.name.is_some_and(|name| {
                    cx.arena()
                        .node_ref(node.source(), name)
                        .and_then(|name| cx.arena().node(name).ok())
                        .is_some_and(|name| name.kind == SyntaxKind::Identifier)
                }) {
                    return Ok(false);
                }
                let Some(expression) = data
                    .expression
                    .and_then(|id| cx.arena().node_ref(node.source(), id))
                else {
                    return Ok(false);
                };
                self.is_entity_name_expression(cx, expression)
            }
            _ => Ok(false),
        }
    }
}

/// tsc-port: maskModifiers @6.0.3
/// tsc-hash: d6513fa885a2cb14487fe3f3e32c817f9f74904e64d312aa19cae9b6b222fd8e
/// tsc-span: _tsc.js:115809-115811
pub(crate) fn mask_modifiers(
    cx: &mut TransformationContext,
    node: TransformNode,
    modifier_mask: ModifierFlags,
    modifier_additions: ModifierFlags,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let flags = mask_modifier_flags(cx, node, modifier_mask, modifier_additions)?;
    cx.factory()?
        .create_modifiers_from_modifier_flags(node.source(), flags)
}

/// tsc-port: maskModifierFlags @6.0.3
/// tsc-hash: f10ef4216ea687a648e15d7748f188a57c359ff0471ffde9b242b68329ac01ac
/// tsc-span: _tsc.js:115812-115821
pub(crate) fn mask_modifier_flags(
    cx: &TransformationContext,
    node: TransformNode,
    modifier_mask: ModifierFlags,
    modifier_additions: ModifierFlags,
) -> Result<ModifierFlags, TransformError> {
    let source = cx.arena().source(node.source())?.syntax();
    let mut bits = (effective_modifier_flags(source, node.node()).bits() & modifier_mask.bits())
        | modifier_additions.bits();
    if bits & ModifierFlags::DEFAULT.bits() != 0 && bits & ModifierFlags::EXPORT.bits() == 0 {
        bits ^= ModifierFlags::EXPORT.bits();
    }
    if bits & ModifierFlags::DEFAULT.bits() != 0 && bits & ModifierFlags::AMBIENT.bits() != 0 {
        bits ^= ModifierFlags::AMBIENT.bits();
    }
    Ok(ModifierFlags::from_bits(bits))
}

/// tsc-port: isAlwaysType @6.0.3
/// tsc-hash: 30d1579722d364cb201197ea471debc420ad5c50bd5ce7902e39c4f2e8b26fac
/// tsc-span: _tsc.js:115803-115808
pub(crate) fn is_always_type(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(cx.arena().node(node)?.kind == SyntaxKind::InterfaceDeclaration)
}

/// tsc-port: canHaveLiteralInitializer @6.0.3
/// tsc-hash: 684e2fac9f424e79077797b3c28484f6f6c43d731a5d460ed83727345035a1f5
/// tsc-span: _tsc.js:115822-115832
pub(crate) fn can_have_literal_initializer(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let source = cx.arena().source(node.source())?.syntax();
    Ok(match cx.arena().node(node)?.kind {
        SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature => {
            !effective_modifier_flags(source, node.node()).contains(ModifierFlags::PRIVATE)
        }
        SyntaxKind::Parameter | SyntaxKind::VariableDeclaration => true,
        _ => false,
    })
}

fn update_binding_pattern(
    cx: &mut TransformationContext,
    original: TransformNode,
    elements: Vec<TransformNode>,
) -> Result<TransformNode, TransformError> {
    // Dispatch through the typed binding-pattern update faces.
    let array = match &cx.arena().node(original)?.data {
        NodeData::ArrayBindingPattern(data) => data.elements,
        NodeData::ObjectBindingPattern(data) => data.elements,
        _ => None,
    };
    let elements = match array.and_then(|array| cx.arena().node_array_ref(original.source(), array))
    {
        Some(array) => cx.factory()?.update_node_array(array, elements)?,
        None => cx
            .factory()?
            .create_node_array(original.source(), elements)?,
    };
    let data = match cx.arena().node(original)?.data.clone() {
        NodeData::ArrayBindingPattern(mut data) => {
            data.elements = Some(elements.array());
            NodeData::ArrayBindingPattern(data)
        }
        NodeData::ObjectBindingPattern(mut data) => {
            data.elements = Some(elements.array());
            NodeData::ObjectBindingPattern(data)
        }
        _ => {
            return Err(DeclarationTransformer::contract(
                "binding pattern update kind mismatch",
            ))
        }
    };
    let flags = cx.arena().transform_flags(original);
    cx.factory()?.update_node(original, data, flags)
}

fn node_name(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let id = match &cx.arena().node(node)?.data {
        NodeData::Parameter(data) => data.name,
        NodeData::VariableDeclaration(data) => data.name,
        NodeData::BindingElement(data) => data.name,
        NodeData::PropertyDeclaration(data) => data.name,
        NodeData::PropertySignature(data) => data.name,
        NodeData::MethodDeclaration(data) => data.name,
        NodeData::MethodSignature(data) => data.name,
        NodeData::GetAccessor(data) => data.name,
        NodeData::SetAccessor(data) => data.name,
        NodeData::TypeParameter(data) => data.name,
        NodeData::FunctionDeclaration(data) => data.name,
        NodeData::ImportEqualsDeclaration(data) => data.name,
        _ => None,
    };
    Ok(id.and_then(|id| cx.arena().node_ref(node.source(), id)))
}

fn type_annotation(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let id = match &cx.arena().node(node)?.data {
        NodeData::Parameter(data) => data.r#type,
        NodeData::VariableDeclaration(data) => data.r#type,
        NodeData::PropertyDeclaration(data) => data.r#type,
        NodeData::PropertySignature(data) => data.r#type,
        NodeData::MethodDeclaration(data) => data.r#type,
        NodeData::MethodSignature(data) => data.r#type,
        NodeData::GetAccessor(data) => data.r#type,
        NodeData::SetAccessor(data) => data.r#type,
        NodeData::BindingElement(_) | NodeData::ExportAssignment(_) => None,
        _ => None,
    };
    Ok(id.and_then(|id| cx.arena().node_ref(node.source(), id)))
}

fn initializer(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let id = match &cx.arena().node(node)?.data {
        NodeData::Parameter(data) => data.initializer,
        NodeData::VariableDeclaration(data) => data.initializer,
        NodeData::PropertyDeclaration(data) => data.initializer,
        NodeData::PropertySignature(data) => data.initializer,
        NodeData::BindingElement(data) => data.initializer,
        _ => None,
    };
    Ok(id.and_then(|id| cx.arena().node_ref(node.source(), id)))
}

fn parameters(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let id = match &cx.arena().node(node)?.data {
        NodeData::GetAccessor(data) => data.parameters,
        NodeData::SetAccessor(data) => data.parameters,
        _ => None,
    };
    let Some(array) = id.and_then(|array| cx.arena().node_array_ref(node.source(), array)) else {
        return Ok(Vec::new());
    };
    Ok(cx
        .arena()
        .node_array(array)?
        .nodes
        .iter()
        .map(|&node_id| TransformNode::new(node.source(), node_id))
        .collect())
}

fn modifier_array(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNodeArray>, TransformError> {
    let modifiers = match &cx.arena().node(node)?.data {
        NodeData::TypeParameter(data) => data.modifiers,
        NodeData::Parameter(data) => data.modifiers,
        NodeData::PropertySignature(data) => data.modifiers,
        NodeData::PropertyDeclaration(data) => data.modifiers,
        NodeData::MethodSignature(data) => data.modifiers,
        NodeData::MethodDeclaration(data) => data.modifiers,
        NodeData::Constructor(data) => data.modifiers,
        NodeData::GetAccessor(data) => data.modifiers,
        NodeData::SetAccessor(data) => data.modifiers,
        NodeData::IndexSignature(data) => data.modifiers,
        NodeData::ConstructorType(data) => data.modifiers,
        NodeData::FunctionDeclaration(data) => data.modifiers,
        NodeData::ClassDeclaration(data) => data.modifiers,
        NodeData::InterfaceDeclaration(data) => data.modifiers,
        NodeData::TypeAliasDeclaration(data) => data.modifiers,
        NodeData::ModuleDeclaration(data) => data.modifiers,
        _ => None,
    };
    Ok(modifiers.and_then(|array| cx.arena().node_array_ref(node.source(), array)))
}

fn unwrap_parenthesized(
    cx: &TransformationContext,
    mut node: TransformNode,
) -> Result<TransformNode, TransformError> {
    while let NodeData::ParenthesizedExpression(data) = &cx.arena().node(node)?.data {
        let Some(expression) = data
            .expression
            .and_then(|id| cx.arena().node_ref(node.source(), id))
        else {
            break;
        };
        node = expression;
    }
    Ok(node)
}

fn is_primitive_literal_value(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    Ok(match cx.arena().node(node)?.kind {
        SyntaxKind::TrueKeyword
        | SyntaxKind::FalseKeyword
        | SyntaxKind::NumericLiteral
        | SyntaxKind::StringLiteral
        | SyntaxKind::NoSubstitutionTemplateLiteral
        | SyntaxKind::BigIntLiteral => true,
        SyntaxKind::PrefixUnaryExpression => {
            let NodeData::PrefixUnaryExpression(data) = &cx.arena().node(node)?.data else {
                return Ok(false);
            };
            let operand = data
                .operand
                .and_then(|id| cx.arena().node_ref(node.source(), id));
            matches!(
                (
                    data.operator,
                    operand
                        .map(|node| cx.arena().node(node).map(|node| node.kind))
                        .transpose()?,
                ),
                (
                    SyntaxKind::MinusToken,
                    Some(SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral),
                ) | (SyntaxKind::PlusToken, Some(SyntaxKind::NumericLiteral))
            )
        }
        _ => false,
    })
}

const fn has_inferred_type(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Parameter
            | SyntaxKind::PropertySignature
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::BinaryExpression
            | SyntaxKind::VariableDeclaration
            | SyntaxKind::ExportAssignment
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::JSDocParameterTag
            | SyntaxKind::JSDocPropertyTag
    )
}

/// tsrs-native: declaration visitor function-like kind predicate.
pub(crate) const fn is_function_like(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::MethodSignature
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::FunctionType
            | SyntaxKind::ConstructorType
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
    )
}

fn current_source_is_js(
    cx: &TransformationContext,
    source: crate::TransformSourceId,
) -> Result<bool, TransformError> {
    let file_name = &cx.arena().source(source)?.syntax().file_name;
    Ok(matches!(
        std::path::Path::new(file_name)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("js" | "jsx" | "mjs" | "cjs")
    ))
}

fn identifier_text(cx: &TransformationContext, node: TransformNode) -> Option<String> {
    match &cx.arena().node(node).ok()?.data {
        NodeData::Identifier(data) => Some(data.text.clone()),
        _ => None,
    }
}
