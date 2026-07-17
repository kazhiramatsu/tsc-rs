//! M4 5.5b: contextual typing — the §4 band (L72612-73955 + stacks).
//!
//! The contextual stacks (pushContextualType 73569 family), the
//! getContextualType master switch (73471) with every arm helper, the
//! apparent/instantiate pair, object-literal discrimination, and the
//! contextual-signature family (73741-73891).
//!
//! Stage-boundary escapes in this module (m4-55-expression-extraction.md
//! §0/§4 dispositions):
//! - [CALLS → 5.7/5.8] argument, decorator, and tagged-template
//!   contextual arms are live, including the self-contained
//!   isImportCall arm.
//! - [ITER → 5.5f/5.8] yield-operand arm answers None; generator
//!   filters inside return-type arms escape.
//! - [ASYNC → 5.5f] awaited-family arms escape once a contextual type
//!   actually exists (the None fall-through matches tsc exactly).
//! - [JSX → 5.5f] all JSX arms escape.
//! - [INFER → M6] the inference stack exists but only `None` is ever
//!   pushed (`InferenceContextPlaceholder` is uninhabited), so the
//!   mapper branches of instantiateContextualType are provably
//!   identity.
//! - [JSDOC] JS-only arms (type tags, satisfies tags, expando kinds)
//!   follow the standing plain-JS policy: the TS-visible shape ports,
//!   JSDoc reads are invisible (we do not parse JSDoc) — FN in JS only.

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckFlags, ContextFlags, ModifierFlags, NodeFlags, ObjectFlags, SymbolFlags, TypeData,
    TypeFlags, TypeId, UnionReduction,
};

use crate::indexed::is_numeric_literal_name;
use crate::state::{CheckResult2, CheckerState, SignatureId, Unsupported};

/// [INFER → M6] The InferenceContext payload. Uninhabited: the stack
/// can only ever hold `None` until M6 replaces this with the real
/// struct, which makes every `if let Some(context)` branch reading it
/// provably dead (`match context {}`).
#[derive(Clone, Copy, Debug)]
pub(crate) enum InferenceContextPlaceholder {}

/// One lazy discriminator of discriminateTypeByDiscriminableItems'
/// contextual callers (73357/73391): tsc passes `[() => type, name]`
/// thunk pairs that run zero or more times INSIDE the include loop —
/// laziness is observable through type-creation order, so the port
/// keeps the thunk explicit.
pub(crate) enum ContextualDiscriminator {
    /// `() => getContextFreeTypeOfExpression(expr)` (73377).
    ContextFree(NodeId),
    /// `() => undefinedType` (73386 — the absent-optional-property row).
    Undefined,
    /// `() => trueType` (73403 — the initializer-less JSX attribute).
    True,
}

impl<'a> CheckerState<'a> {
    // ---- the contextual stacks (73557-73605) ----

    /// tsc-port: pushCachedContextualType @6.0.3
    /// tsc-hash: e77fbf48ca2c7b30e00b74c2d48b22b29cfda72963d4f48cb6ed1206cb123020
    /// tsc-span: _tsc.js:73557-73568
    pub(crate) fn push_cached_contextual_type(&mut self, node: NodeId) -> CheckResult2<()> {
        let ty = self.get_contextual_type(node, ContextFlags::NONE)?;
        self.push_contextual_type(node, ty, true);
        Ok(())
    }

    /// tsc-port: pushContextualType @6.0.3
    /// tsc-hash: e9481e5556ca402bb1aa016d75b51e04d1c95a2f2db293ef858682407b0e93f7
    /// tsc-span: _tsc.js:73569-73574
    pub(crate) fn push_contextual_type(
        &mut self,
        node: NodeId,
        ty: Option<TypeId>,
        is_cache: bool,
    ) {
        self.contextual_type_nodes.push(node);
        self.contextual_types.push(ty);
        self.contextual_is_cache.push(is_cache);
    }

    /// tsc-port: popContextualType @6.0.3
    /// tsc-hash: 750594d6d1c31565f912747eed06290bc008c0aade55c6b5d2ef2de7910c66c9
    /// tsc-span: _tsc.js:73575-73580
    pub(crate) fn pop_contextual_type(&mut self) {
        self.contextual_type_nodes.pop();
        self.contextual_types.pop();
        self.contextual_is_cache.pop();
    }

    /// tsc-port: findContextualNode @6.0.3
    /// tsc-hash: ee812153d4466d26d0df3c91de6dcf99bbc7463a34626c706c8d5b0084079c69
    /// tsc-span: _tsc.js:73581-73588
    fn find_contextual_node(&self, node: NodeId, include_caches: bool) -> Option<usize> {
        (0..self.contextual_type_nodes.len()).rev().find(|&i| {
            node == self.contextual_type_nodes[i]
                && (include_caches || !self.contextual_is_cache[i])
        })
    }

    /// tsc-port: pushInferenceContext @6.0.3
    /// tsc-hash: 1c1aad71d5c38c6182bcd8692e1a2c99cac193cd434c266f58529edd2bb99f60
    /// tsc-span: _tsc.js:73589-73593
    pub(crate) fn push_inference_context(
        &mut self,
        node: NodeId,
        inference_context: Option<InferenceContextPlaceholder>,
    ) {
        self.inference_context_nodes.push(node);
        self.inference_contexts.push(inference_context);
    }

    /// tsc-port: popInferenceContext @6.0.3
    /// tsc-hash: c7d9b99b05a405da519f3d88cdf9403e47b70831b8f1b8947b57b264142ccef8
    /// tsc-span: _tsc.js:73594-73598
    pub(crate) fn pop_inference_context(&mut self) {
        self.inference_context_nodes.pop();
        self.inference_contexts.pop();
    }

    /// tsc-port: getInferenceContext @6.0.3
    /// tsc-hash: d1fbcf383b083b4356013df0c6189cde575b9e69500cb420105609527cbae083
    /// tsc-span: _tsc.js:73599-73605
    ///
    /// Innermost node-descendant scan; the found value may itself be
    /// None (checkExpressionWithContextualType pushes None at 5.5) —
    /// and until M6 it always is.
    pub(crate) fn get_inference_context(
        &self,
        node: NodeId,
    ) -> Option<InferenceContextPlaceholder> {
        for i in (0..self.inference_context_nodes.len()).rev() {
            if self.is_node_descendant_of(node, self.inference_context_nodes[i]) {
                return self.inference_contexts[i];
            }
        }
        None
    }

    /// tsc getCachedType/setCachedType (47484-47490): the string-keyed
    /// side cache (`key ? cachedTypes.get(key) : undefined` — every
    /// 5.5b caller passes a non-empty key).
    pub(crate) fn get_cached_type(&self, key: &str) -> Option<TypeId> {
        self.cached_types.get(key).copied()
    }

    pub(crate) fn set_cached_type(&mut self, key: String, ty: TypeId) -> TypeId {
        self.cached_types.insert(key, ty);
        ty
    }

    // ---- this-parameter contextual family (72612-72686) ----

    /// tsc-port: getContainingObjectLiteral @6.0.3
    /// tsc-hash: 678e4b2a3582b8f0853db50ed4c8427e781abaf62d6011b4116f4ecf2760cff8
    /// tsc-span: _tsc.js:72612-72614
    fn get_containing_object_literal(&self, func: NodeId) -> Option<NodeId> {
        let parent = self.parent_of(func)?;
        match self.kind_of(func) {
            SyntaxKind::MethodDeclaration | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
                if self.kind_of(parent) == SyntaxKind::ObjectLiteralExpression =>
            {
                Some(parent)
            }
            SyntaxKind::FunctionExpression
                if self.kind_of(parent) == SyntaxKind::PropertyAssignment =>
            {
                self.parent_of(parent)
            }
            _ => None,
        }
    }

    /// tsc-port: getThisTypeArgument @6.0.3
    /// tsc-hash: fbb2deb3a6643f344a59c239e50540598dffa1c9e24e26ffcf041bd0c460dc5e
    /// tsc-span: _tsc.js:72615-72617
    fn get_this_type_argument(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::REFERENCE)
        {
            return Ok(None);
        }
        let TypeData::Reference { target, .. } = self.tables.type_of(ty).data else {
            return Ok(None);
        };
        let global_this = self.global_this_type_alias()?;
        if global_this != Some(target) {
            return Ok(None);
        }
        Ok(self.get_type_arguments(ty)?.first().copied())
    }

    /// tsc-port: getThisTypeFromContextualType @6.0.3
    /// tsc-hash: 031f9a6c212f438d45d3b3a8b852a024108659a3993c61ac1a509326937d4717
    /// tsc-span: _tsc.js:72618-72622
    fn get_this_type_from_contextual_type(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        self.map_type(
            ty,
            &mut |state, t| {
                if state.tables.flags_of(t).intersects(TypeFlags::INTERSECTION) {
                    let types = match &state.tables.type_of(t).data {
                        TypeData::Intersection { types } => types.to_vec(),
                        _ => unreachable!("intersection flag implies payload"),
                    };
                    for member in types {
                        if let Some(this_arg) = state.get_this_type_argument(member)? {
                            return Ok(Some(this_arg));
                        }
                    }
                    Ok(None)
                } else {
                    state.get_this_type_argument(t)
                }
            },
            false,
        )
    }

    /// tsc-port: getThisTypeOfObjectLiteralFromContextualType @6.0.3
    /// tsc-hash: d2b676809580d76d6f9a3b1cc9dabb3e734eb96ba7c8a94b3804c4f17d4717f5
    /// tsc-span: _tsc.js:72623-72641
    fn get_this_type_of_object_literal_from_contextual_type(
        &mut self,
        containing_literal: NodeId,
        contextual_type: Option<TypeId>,
    ) -> CheckResult2<Option<TypeId>> {
        let mut literal = containing_literal;
        let mut ty = contextual_type;
        while let Some(current) = ty {
            if let Some(this_type) = self.get_this_type_from_contextual_type(current)? {
                return Ok(Some(this_type));
            }
            let parent = self.parent_of(literal).expect("literal has a parent");
            if self.kind_of(parent) != SyntaxKind::PropertyAssignment {
                break;
            }
            literal = self.parent_of(parent).expect("assignment has a parent");
            ty = self.get_apparent_type_of_contextual_type(literal, ContextFlags::NONE)?;
        }
        Ok(None)
    }

    /// tsc-port: getContextualThisParameterType @6.0.3
    /// tsc-hash: 427137868847758711d3501016d73efd44c6a073114c73a77b2cf5296a08eff3
    /// tsc-span: _tsc.js:72642-72686
    ///
    /// The commonJS-indicator arm is [JSDOC].
    pub(crate) fn get_contextual_this_parameter_type(
        &mut self,
        func: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.kind_of(func) == SyntaxKind::ArrowFunction {
            return Ok(None);
        }
        if self.is_context_sensitive_function_or_object_literal_method(func)? {
            if let Some(contextual_signature) = self.get_contextual_signature(func)? {
                if let Some(this_parameter) = self.signature_of(contextual_signature).this_parameter
                {
                    return Ok(Some(self.get_type_of_symbol(this_parameter)?));
                }
            }
        }
        let in_js = self.is_in_js_file(func);
        let no_implicit_this = self
            .options
            .strict_option_value(self.options.no_implicit_this);
        if no_implicit_this || in_js {
            if let Some(containing_literal) = self.get_containing_object_literal(func) {
                let contextual_type = self
                    .get_apparent_type_of_contextual_type(containing_literal, ContextFlags::NONE)?;
                let this_type = self.get_this_type_of_object_literal_from_contextual_type(
                    containing_literal,
                    contextual_type,
                )?;
                if let Some(this_type) = this_type {
                    // instantiateType(thisType, getMapperFromContext(
                    // getInferenceContext(...))) — [INFER → M6]: the
                    // context is always None, so the mapper is None and
                    // instantiateType is the identity read.
                    if let Some(context) = self.get_inference_context(containing_literal) {
                        match context {}
                    }
                    return Ok(Some(this_type));
                }
                let base = match contextual_type {
                    Some(contextual_type) => self.get_non_nullable_type(contextual_type)?,
                    None => self.check_expression_cached(
                        containing_literal,
                        tsrs2_types::CheckMode::NORMAL,
                    )?,
                };
                return Ok(Some(self.get_widened_type(base)?));
            }
            // walkUpParenthesizedExpressions (tsc 14434).
            let mut parent = self.parent_of(func);
            while let Some(current) = parent {
                if self.kind_of(current) != SyntaxKind::ParenthesizedExpression {
                    break;
                }
                parent = self.parent_of(current);
            }
            if let Some(parent) = parent {
                if let NodeData::BinaryExpression(data) = self.data_of(parent) {
                    let (left, operator) = (data.left, data.operator_token);
                    let is_assignment =
                        operator.is_some_and(|op| self.kind_of(op) == SyntaxKind::EqualsToken);
                    if is_assignment {
                        if let Some(target) = left {
                            if matches!(
                                self.kind_of(target),
                                SyntaxKind::PropertyAccessExpression
                                    | SyntaxKind::ElementAccessExpression
                            ) {
                                // The commonJsModuleIndicator arm is
                                // [JSDOC] (JS only) — invisible here.
                                let expression = match self.data_of(target) {
                                    NodeData::PropertyAccessExpression(data) => data.expression,
                                    NodeData::ElementAccessExpression(data) => data.expression,
                                    _ => None,
                                };
                                let Some(expression) = expression else {
                                    return Ok(None);
                                };
                                let checked = self.check_expression_cached(
                                    expression,
                                    tsrs2_types::CheckMode::NORMAL,
                                )?;
                                return Ok(Some(self.get_widened_type(checked)?));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: getContextuallyTypedParameterType @6.0.3
    /// tsc-hash: e63a5c7305b6cd22417fe6667a33caf49d1794694a7131a99263b174e1a08bde
    /// tsc-span: _tsc.js:72687-72719
    ///
    /// The IIFE arm parks anySignature on the call while the argument
    /// checks (links swap — re-entrant reads short-circuit); note the
    /// IIFE index is the RAW parameter position (no this-parameter
    /// shift, unlike the contextual-signature arm below).
    pub(crate) fn get_contextually_typed_parameter_type(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let func = self.parent_of(parameter).expect("parameter has a parent");
        if !self.is_context_sensitive_function_or_object_literal_method(func)? {
            return Ok(None);
        }
        let source = self.binder.source_of_node(func);
        let iife = node_util::get_immediately_invoked_function_expression(source, func);
        if let Some(iife) = iife {
            let has_arguments = matches!(self.data_of(iife), NodeData::CallExpression(data)
                if data.arguments.is_some());
            if has_arguments {
                let args = self.get_effective_call_arguments(iife)?;
                let parameters = self.parameters_of_function(func);
                let index_of_parameter = parameters
                    .iter()
                    .position(|&p| p == parameter)
                    .expect("parameter is in its own function's list");
                let is_rest = matches!(
                    self.data_of(parameter),
                    NodeData::Parameter(data) if data.dot_dot_dot_token.is_some()
                );
                if is_rest {
                    let any = self.tables.intrinsics.any;
                    return Ok(Some(self.get_spread_argument_type(
                        &args,
                        index_of_parameter,
                        args.len(),
                        any,
                        tsrs2_types::CheckMode::NORMAL,
                    )?));
                }
                let has_initializer = matches!(
                    self.data_of(parameter),
                    NodeData::Parameter(data) if data.initializer.is_some()
                );
                let cached = self.links.swap_node_resolved_signature_iife(
                    self.speculation_depth,
                    iife,
                    crate::links::LinkSlot::Resolved(self.any_signature),
                );
                let result = (|state: &mut Self| -> CheckResult2<Option<TypeId>> {
                    if index_of_parameter < args.len() {
                        let checked = state.check_effective_arg(
                            &args[index_of_parameter],
                            tsrs2_types::CheckMode::NORMAL,
                        )?;
                        Ok(Some(state.get_widened_literal_type(checked)?))
                    } else if has_initializer {
                        Ok(None)
                    } else {
                        Ok(Some(state.tables.intrinsics.undefined_widening))
                    }
                })(self);
                self.links
                    .swap_node_resolved_signature_iife(self.speculation_depth, iife, cached);
                return result;
            }
        }
        let Some(contextual_signature) = self.get_contextual_signature(func)? else {
            // [INFER M6] gate: a context-sensitive function that HAS a
            // contextual type but yields no contextual SIGNATURE is
            // undecidable pre-M6 — during resolveCall's sentinel
            // window the read sees resolvingSignature's empty list, and
            // deciding "no context" here caches implicit-any into the
            // parameter slots and fabricates 7006 (tsc assigns
            // contextual parameter types inside the pushed-context
            // window, assignContextualParameterTypes — M6 inference
            // machinery). A function with NO contextual type at all
            // keeps the None verdict — the genuine implicit-any face.
            if self
                .get_apparent_type_of_contextual_type(func, ContextFlags::SIGNATURE)?
                .is_some()
            {
                return Err(Unsupported::new(
                    "[INFER M6] context-sensitive parameter under an unresolved contextual signature",
                ));
            }
            return Ok(None);
        };
        let parameters = self.parameters_of_function(func);
        let index = parameters
            .iter()
            .position(|&p| p == parameter)
            .expect("parameter is in its own function's list")
            - usize::from(self.this_parameter_node_of(func).is_some());
        let is_rest = matches!(
            self.data_of(parameter),
            NodeData::Parameter(data) if data.dot_dot_dot_token.is_some()
        ) && parameters.last() == Some(&parameter);
        if is_rest {
            Ok(Some(self.get_rest_type_at_position(
                contextual_signature,
                index,
                /*readonly*/ false,
            )?))
        } else {
            self.try_get_type_at_position(contextual_signature, index)
        }
    }

    /// tsc-port: getContextualTypeForVariableLikeDeclaration @6.0.3
    /// tsc-hash: 0600ff219bea83d1d0d517f12fb743e5c806e8de147650ced5bd5568445ecb94
    /// tsc-span: _tsc.js:72720-72735
    ///
    /// The tryGetJSDocSatisfiesTypeNode fallback is [JSDOC].
    fn get_contextual_type_for_variable_like_declaration(
        &mut self,
        declaration: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(type_node) = self.effective_type_annotation_node(declaration) {
            return Ok(Some(self.get_type_from_type_node(type_node)?));
        }
        match self.kind_of(declaration) {
            SyntaxKind::Parameter => self.get_contextually_typed_parameter_type(declaration),
            SyntaxKind::BindingElement => {
                self.get_contextual_type_for_binding_element(declaration, context_flags)
            }
            SyntaxKind::PropertyDeclaration
                if node_util::has_syntactic_modifier(
                    self.binder.source_of_node(declaration),
                    declaration,
                    ModifierFlags::STATIC,
                ) =>
            {
                self.get_contextual_type_for_static_property_declaration(declaration, context_flags)
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: getContextualTypeForBindingElement @6.0.3
    /// tsc-hash: e5b2f2548742ed96aba69a7189d31af3eab726f5858cb54087547df5f61687ad
    /// tsc-span: _tsc.js:72736-72751
    fn get_contextual_type_for_binding_element(
        &mut self,
        declaration: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let pattern = self.parent_of(declaration).expect("element has a pattern");
        let parent = self.parent_of(pattern).expect("pattern has a declaration");
        let NodeData::BindingElement(data) = self.data_of(declaration) else {
            unreachable!("kind/data agree");
        };
        let (property_name, element_name, dot_dot_dot) = (
            data.property_name,
            data.name,
            data.dot_dot_dot_token.is_some(),
        );
        let name = property_name.or(element_name).expect("element has a name");
        let mut parent_type =
            self.get_contextual_type_for_variable_like_declaration(parent, context_flags)?;
        if parent_type.is_none()
            && self.kind_of(parent) != SyntaxKind::BindingElement
            && self.initializer_of(parent).is_some()
        {
            let check_mode = if dot_dot_dot {
                tsrs2_types::CheckMode::REST_BINDING_ELEMENT
            } else {
                tsrs2_types::CheckMode::NORMAL
            };
            parent_type = Some(self.check_declaration_initializer(parent, check_mode, None)?);
        }
        let Some(parent_type) = parent_type else {
            return Ok(None);
        };
        let source = self.binder.source_of_node(declaration);
        if node_util::is_binding_pattern(source, name) || self.is_computed_non_literal_name(name) {
            return Ok(None);
        }
        let parent_name = match self.data_of(parent) {
            NodeData::VariableDeclaration(data) => data.name,
            NodeData::Parameter(data) => data.name,
            NodeData::BindingElement(data) => data.name,
            NodeData::PropertyDeclaration(data) => data.name,
            _ => None,
        };
        if parent_name.is_some_and(|n| self.kind_of(n) == SyntaxKind::ArrayBindingPattern) {
            let elements = match self.data_of(pattern) {
                NodeData::ArrayBindingPattern(data) => data.elements,
                NodeData::ObjectBindingPattern(data) => data.elements,
                _ => None,
            };
            let Some(index) = self
                .nodes_of(elements)
                .iter()
                .position(|&e| e == declaration)
            else {
                return Ok(None);
            };
            return self.get_contextual_type_for_element_expression_simple(parent_type, index);
        }
        let name_type = self.get_literal_type_from_property_name(name)?;
        if let Some(text) = self.property_name_from_type_usable(name_type) {
            return self.get_type_of_property_of_type(parent_type, &text);
        }
        Ok(None)
    }

    /// tsc-port: getContextualTypeForStaticPropertyDeclaration @6.0.3
    /// tsc-hash: 8a200710e2f2f9229ce7191be72bc3cab15674b6a2b6b168ae14d6c444bb6ceb
    /// tsc-span: _tsc.js:72752-72756
    fn get_contextual_type_for_static_property_declaration(
        &mut self,
        declaration: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let parent = self.parent_of(declaration).expect("member has a class");
        if !node_util::is_expression_node(self.binder.source_of_node(parent), parent) {
            return Ok(None);
        }
        let Some(parent_type) = self.get_contextual_type(parent, context_flags)? else {
            return Ok(None);
        };
        let symbol = self.get_symbol_of_declaration(declaration)?;
        let name = self.binder.symbol(symbol).escaped_name.clone();
        self.get_type_of_property_of_contextual_type(parent_type, &name, None)
    }

    /// tsc-port: getContextualTypeForInitializerExpression @6.0.3
    /// tsc-hash: 7092a5af271f253b10d14f1b0df22964ffec38ecdc378d6bd69b9a2a5e73a308
    /// tsc-span: _tsc.js:72757-72775
    fn get_contextual_type_for_initializer_expression(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let declaration = self.parent_of(node).expect("initializer has a declaration");
        if self.initializer_of(declaration) == Some(node) {
            if let Some(result) =
                self.get_contextual_type_for_variable_like_declaration(declaration, context_flags)?
            {
                return Ok(Some(result));
            }
            if !context_flags.intersects(ContextFlags::SKIP_BINDING_PATTERNS) {
                let name = match self.data_of(declaration) {
                    NodeData::VariableDeclaration(data) => data.name,
                    NodeData::Parameter(data) => data.name,
                    NodeData::BindingElement(data) => data.name,
                    NodeData::PropertyDeclaration(data) => data.name,
                    _ => None,
                };
                if let Some(name) = name {
                    let source = self.binder.source_of_node(declaration);
                    if node_util::is_binding_pattern(source, name) {
                        let elements = match self.data_of(name) {
                            NodeData::ObjectBindingPattern(data) => data.elements,
                            NodeData::ArrayBindingPattern(data) => data.elements,
                            _ => None,
                        };
                        let has_elements = !self.nodes_of(elements).is_empty();
                        if has_elements {
                            return Ok(Some(self.get_type_from_binding_pattern(
                                name, /*include_pattern_in_type*/ true,
                                /*report_errors*/ false,
                            )?));
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsc-port: getContextualTypeForReturnExpression @6.0.3
    /// tsc-hash: 941b7545b7acc626e53e5111d6ec13ccfebc39f90fe0a2a9e463dea900536755
    /// tsc-span: _tsc.js:72776-72801
    ///
    /// Generator filter/iteration arms live since 5.8b (§4).
    fn get_contextual_type_for_return_expression(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(func) = self.get_containing_function(node) else {
            return Ok(None);
        };
        let Some(contextual_return_type) = self.get_contextual_return_type(func, context_flags)?
        else {
            return Ok(None);
        };
        let function_flags = self.get_function_flags(func);
        let mut contextual_return_type = contextual_return_type;
        if function_flags & FUNCTION_FLAGS_GENERATOR != 0 {
            // 72782-72791: union contextual types keep only the
            // constituents with a generator RETURN iteration type;
            // a miss on the whole answers None.
            let is_async_generator = function_flags & FUNCTION_FLAGS_ASYNC != 0;
            if self
                .tables
                .flags_of(contextual_return_type)
                .intersects(TypeFlags::UNION)
            {
                contextual_return_type =
                    self.filter_type_with(contextual_return_type, |state, t| {
                        Ok(state
                            .get_iteration_type_of_generator_function_return_type(
                                tsrs2_types::IterationTypeKind::RETURN,
                                t,
                                is_async_generator,
                            )?
                            .is_some())
                    })?;
            }
            let iteration_return_type = self.get_iteration_type_of_generator_function_return_type(
                tsrs2_types::IterationTypeKind::RETURN,
                contextual_return_type,
                is_async_generator,
            )?;
            let Some(iteration_return_type) = iteration_return_type else {
                return Ok(None);
            };
            contextual_return_type = iteration_return_type;
        }
        if function_flags & FUNCTION_FLAGS_ASYNC != 0 {
            // 72792-72795: awaited-or-promise-like of the contextual
            // return type.
            return self.awaited_or_promise_like_of(contextual_return_type);
        }
        Ok(Some(contextual_return_type))
    }

    /// The shared async-contextual shape (72793/72805): the awaited
    /// contextual type unioned with its PromiseLike wrap; an undefined
    /// awaited answers None. (The return arm's outer mapType is
    /// redundant — getAwaitedTypeNoAlias already distributes over
    /// unions internally.)
    fn awaited_or_promise_like_of(&mut self, ty: TypeId) -> CheckResult2<Option<TypeId>> {
        let awaited = self.get_awaited_type_no_alias(ty, None)?;
        let Some(awaited) = awaited else {
            return Ok(None);
        };
        let promise_like = self.create_promise_like_type(awaited)?;
        Ok(Some(self.get_union_type_ex(
            &[awaited, promise_like],
            tsrs2_types::UnionReduction::Literal,
        )?))
    }

    /// tsc-port: getContextualTypeForYieldOperand @6.0.3
    /// tsc-hash: 31ef78625fa63299495f6acde2319bebfbd69e0de6ba9aea6831bbc38a8f7115
    /// tsc-span: _tsc.js:72810-72848
    fn get_contextual_type_for_yield_operand(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(func) = self.get_containing_function(node) else {
            return Ok(None);
        };
        let function_flags = self.get_function_flags(func);
        let Some(mut contextual_return_type) =
            self.get_contextual_return_type(func, context_flags)?
        else {
            return Ok(None);
        };
        let is_async_generator = function_flags & FUNCTION_FLAGS_ASYNC != 0;
        let asterisk_token = match self.data_of(node) {
            NodeData::YieldExpression(data) => data.asterisk_token,
            _ => None,
        };
        if asterisk_token.is_none()
            && self
                .tables
                .flags_of(contextual_return_type)
                .intersects(TypeFlags::UNION)
        {
            contextual_return_type =
                self.filter_type_with(contextual_return_type, |state, t| {
                    Ok(state
                        .get_iteration_type_of_generator_function_return_type(
                            tsrs2_types::IterationTypeKind::RETURN,
                            t,
                            is_async_generator,
                        )?
                        .is_some())
                })?;
        }
        if asterisk_token.is_some() {
            let iteration_types = self.get_iteration_types_of_generator_function_return_type(
                contextual_return_type,
                is_async_generator,
            )?;
            let silent_never = self.tables.intrinsics.silent_never;
            let yield_type = iteration_types
                .map(|types| types.yield_type)
                .unwrap_or(silent_never);
            let return_type = self
                .get_contextual_type(node, context_flags)?
                .unwrap_or(silent_never);
            let next_type = iteration_types
                .map(|types| types.next_type)
                .unwrap_or(self.tables.intrinsics.unknown);
            let generator_type = self.create_generator_type(
                yield_type,
                return_type,
                next_type,
                /*is_async_generator*/ false,
            )?;
            if is_async_generator {
                let async_generator_type = self.create_generator_type(
                    yield_type,
                    return_type,
                    next_type,
                    /*is_async_generator*/ true,
                )?;
                return Ok(Some(self.get_union_type_ex(
                    &[generator_type, async_generator_type],
                    tsrs2_types::UnionReduction::Literal,
                )?));
            }
            return Ok(Some(generator_type));
        }
        self.get_iteration_type_of_generator_function_return_type(
            tsrs2_types::IterationTypeKind::YIELD,
            contextual_return_type,
            is_async_generator,
        )
    }

    /// tsc-port: getContextualIterationType @6.0.3
    /// tsc-hash: 126acfec7a3eab20bcff0beb031916075ebbb2a790f3c36b856b79f7ad90772a
    /// tsc-span: _tsc.js:72862-72873
    pub(crate) fn get_contextual_iteration_type(
        &mut self,
        kind: tsrs2_types::IterationTypeKind,
        function_decl: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let is_async = self.get_function_flags(function_decl) & FUNCTION_FLAGS_ASYNC != 0;
        let contextual_return_type =
            self.get_contextual_return_type(function_decl, ContextFlags::NONE)?;
        match contextual_return_type {
            Some(contextual_return_type) => self
                .get_iteration_type_of_generator_function_return_type(
                    kind,
                    contextual_return_type,
                    is_async,
                ),
            None => Ok(None),
        }
    }

    /// tsc-port: getContextualTypeForAwaitOperand @6.0.3
    /// tsc-hash: 5f290c56300cb643f37d3f452bd7a0b18c7234bef5c5bda9ff4d100454bce922
    /// tsc-span: _tsc.js:72802-72809
    fn get_contextual_type_for_await_operand(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(contextual_type) = self.get_contextual_type(node, context_flags)? else {
            return Ok(None);
        };
        self.awaited_or_promise_like_of(contextual_type)
    }

    /// tsc-port: getContextualReturnType @6.0.3
    /// tsc-hash: 6656f35e13ca42f182376bea5fca91873448108c174cb259f9fde363dece7113
    /// tsc-span: _tsc.js:72874-72905
    ///
    /// The async filter is live (5.5f); the generator filter since
    /// 5.8b. The annotation, contextual-signature and IIFE arms are
    /// live ([CALLS]-lite: the IIFE arm is just getContextualType of
    /// the call node).
    pub(crate) fn get_contextual_return_type(
        &mut self,
        function_decl: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(return_type) = self.get_return_type_from_annotation(function_decl)? {
            return Ok(Some(return_type));
        }
        if let Some(signature) =
            self.get_contextual_signature_for_function_like_declaration(function_decl)?
        {
            if !self.is_resolving_return_type_of_signature(signature) {
                let return_type = self.get_return_type_of_signature(signature)?;
                let function_flags = self.get_function_flags(function_decl);
                if function_flags & FUNCTION_FLAGS_GENERATOR != 0 {
                    // 72883-72892: keep any/unknown/void/instantiable
                    // constituents plus generator-instantiable ones.
                    let filtered = self.filter_type_with(return_type, |state, t| {
                        if state.tables.flags_of(t).intersects(
                            TypeFlags::ANY_OR_UNKNOWN
                                | TypeFlags::VOID
                                | TypeFlags::INSTANTIABLE_NON_PRIMITIVE,
                        ) {
                            return Ok(true);
                        }
                        state.check_generator_instantiation_assignability_to_return_type(
                            t,
                            function_flags,
                            /*error_node*/ None,
                        )
                    })?;
                    return Ok(Some(filtered));
                }
                if function_flags & FUNCTION_FLAGS_ASYNC != 0 {
                    // 72894-72898: keep the constituents that are
                    // any/unknown/void/instantiable or promises.
                    let filtered = self.filter_type_with(return_type, |state, t| {
                        if state.tables.flags_of(t).intersects(
                            TypeFlags::ANY_OR_UNKNOWN
                                | TypeFlags::VOID
                                | TypeFlags::INSTANTIABLE_NON_PRIMITIVE,
                        ) {
                            return Ok(true);
                        }
                        Ok(state.get_awaited_type_of_promise(t)?.is_some())
                    })?;
                    return Ok(Some(filtered));
                }
                return Ok(Some(return_type));
            }
        }
        let source = self.binder.source_of_node(function_decl);
        if let Some(iife) =
            node_util::get_immediately_invoked_function_expression(source, function_decl)
        {
            return self.get_contextual_type(iife, context_flags);
        }
        Ok(None)
    }

    /// tsc-port: getContextualTypeForDecorator @6.0.3
    /// tsc-hash: 14ed8418fd06a2894d182f1e99e2938447169457f6081bf5a7e9809141125e62
    /// tsc-span: _tsc.js:72925-72929
    fn get_contextual_type_for_decorator(
        &mut self,
        decorator: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(signature) = self.get_decorator_call_signature(decorator)? else {
            return Ok(None);
        };
        Ok(Some(self.get_or_create_type_from_signature(signature)?))
    }

    /// tsc-port: getContextualTypeForArgument @6.0.3
    /// tsc-hash: fd4575a68cf6d1f6f7455674078b3c307ffa8b684aa12747ef5ca66c3a85ecc7
    /// tsc-span: _tsc.js:72906-72910
    ///
    /// The effective-args recompute checks spread operands through the
    /// cached path exactly like tsc; the EffectiveArg::Node equality is
    /// tsc's args.indexOf.
    fn get_contextual_type_for_argument(
        &mut self,
        call_target: NodeId,
        arg: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let args = self.get_effective_call_arguments(call_target)?;
        let arg_index = args
            .iter()
            .position(|candidate| *candidate == crate::calls::EffectiveArg::Node(arg));
        match arg_index {
            None => Ok(None),
            Some(arg_index) => {
                self.get_contextual_type_for_argument_at_index(call_target, arg_index)
            }
        }
    }

    /// tsc-port: getContextualTypeForArgumentAtIndex @6.0.3
    /// tsc-hash: 6ce36cc4edfdefeba3dfd2a1aeef63dc139941ee14d9cf4886e7354ec145741f
    /// tsc-span: _tsc.js:72911-72924
    ///
    /// The SENTINEL VALVE (72918): while the call is mid-resolution
    /// (links Resolving), contextual reads see resolvingSignature —
    /// zero parameters, so getTypeAtPosition answers anyType — and
    /// never re-enter resolution. The import-call arm precedes the
    /// resolution read: the plain arguments list is the effective
    /// list (grammar caps import calls at two plain arguments).
    fn get_contextual_type_for_argument_at_index(
        &mut self,
        call_target: NodeId,
        arg_index: usize,
    ) -> CheckResult2<Option<TypeId>> {
        if self.is_import_call(call_target) {
            return Ok(Some(match arg_index {
                0 => self.tables.intrinsics.string,
                1 => self.get_global_import_call_options_type(/*report_errors*/ false)?,
                _ => self.tables.intrinsics.any,
            }));
        }
        let signature = if self
            .links
            .node(call_target)
            .resolved_signature
            .is_resolving()
        {
            self.resolving_signature
        } else {
            self.get_resolved_signature(call_target, tsrs2_types::CheckMode::NORMAL)?
        };
        // 72919-72921: JSX opening-likes answer the effective first
        // argument (the sentinel valve above still applies — a
        // mid-resolution read sees resolvingSignature's empty
        // parameter list, the unknown-fallback call-props face).
        if matches!(
            self.kind_of(call_target),
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement
        ) && arg_index == 0
        {
            return Ok(Some(self.get_effective_first_argument_for_jsx_signature(
                signature,
                call_target,
            )?));
        }
        let data = self.signature_of(signature);
        let has_rest = data
            .flags
            .intersects(tsrs2_types::SignatureFlags::HAS_REST_PARAMETER);
        let parameters = data.parameters.clone();
        if has_rest && !parameters.is_empty() && arg_index >= parameters.len() - 1 {
            let rest_index = parameters.len() - 1;
            let rest_type = self.get_type_of_symbol(parameters[rest_index])?;
            let literal = self
                .tables
                .get_number_literal_type((arg_index - rest_index) as f64);
            return Ok(Some(self.get_indexed_access_type(
                rest_type,
                literal,
                tsrs2_types::AccessFlags::CONTEXTUAL,
                None,
                None,
                None,
            )?));
        }
        Ok(Some(self.get_type_at_position(signature, arg_index)?))
    }

    /// tsc-port: getContextualTypeForSubstitutionExpression @6.0.3
    /// tsc-hash: 62ca4575e0c962280f0dab001970618308c877ac39e0db34c92eff453bb4de07
    /// tsc-span: _tsc.js:72929-72934
    ///
    /// Tagged templates route to getContextualTypeForArgument on the
    /// tagged parent; the untagged answer is tsc's own undefined.
    fn get_contextual_type_for_substitution_expression(
        &mut self,
        template: NodeId,
        substitution_expression: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let parent = self.parent_of(template).expect("template has a parent");
        if self.kind_of(parent) == SyntaxKind::TaggedTemplateExpression {
            return self.get_contextual_type_for_argument(parent, substitution_expression);
        }
        Ok(None)
    }

    /// tsc-port: getContextualTypeForBinaryOperand @6.0.3
    /// tsc-hash: bec959dd83fe56039a67a594889bdf6033231a6abba3666720924d37b00c8086
    /// tsc-span: _tsc.js:72935-72953
    fn get_contextual_type_for_binary_operand(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let binary = self.parent_of(node).expect("operand has a binary parent");
        let NodeData::BinaryExpression(data) = self.data_of(binary) else {
            unreachable!("kind/data agree");
        };
        let (left, operator_token, right) = (data.left, data.operator_token, data.right);
        let operator = operator_token.map(|t| self.kind_of(t));
        match operator {
            Some(
                SyntaxKind::EqualsToken
                | SyntaxKind::AmpersandAmpersandEqualsToken
                | SyntaxKind::BarBarEqualsToken
                | SyntaxKind::QuestionQuestionEqualsToken,
            ) => {
                if Some(node) == right {
                    self.get_contextual_type_for_assignment_declaration(binary)
                } else {
                    Ok(None)
                }
            }
            Some(SyntaxKind::BarBarToken | SyntaxKind::QuestionQuestionToken) => {
                // When an || expression has a contextual type, the RHS
                // takes it too, EXCEPT when the LHS type carries a
                // binding `pattern` or there is no context and the
                // expression is not a defaulted-expando initializer
                // ([JSDOC]: expando detection is JS-only, constant
                // false in TS) — those get the LHS type.
                let ty = self.get_contextual_type(binary, context_flags)?;
                if Some(node) == right {
                    let has_pattern = ty.is_some_and(|t| self.links.ty(t).pattern.is_some());
                    let no_context_non_expando =
                        ty.is_none() && !self.is_defaulted_expando_initializer(binary);
                    if has_pattern || no_context_non_expando {
                        let left = left.expect("binary has a left operand");
                        return Ok(Some(self.get_type_of_expression(left)?));
                    }
                }
                Ok(ty)
            }
            Some(SyntaxKind::AmpersandAmpersandToken | SyntaxKind::CommaToken) => {
                if Some(node) == right {
                    self.get_contextual_type(binary, context_flags)
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// tsc isDefaultedExpandoInitializer (15010-15013): expando
    /// detection reads getExpandoInitializer, whose every arm requires
    /// a JS file — constant false in TS; JS rides [JSDOC] (FN: the JS
    /// `var x = x || {}` shape keeps its contextual type where tsc
    /// switches to the LHS type).
    fn is_defaulted_expando_initializer(&self, node: NodeId) -> bool {
        let _ = node;
        false
    }

    /// tsc-port: getSymbolForExpression @6.0.3
    /// tsc-hash: cc13b2e3c5958fca3589c74248091883cb95c0d39a97794192a3ee6e0674a3d6
    /// tsc-span: _tsc.js:72955-72979
    fn get_symbol_for_expression(&mut self, e: NodeId) -> CheckResult2<Option<SymbolId>> {
        if let Some(symbol) = self.node_symbol(e) {
            return Ok(Some(symbol));
        }
        match self.data_of(e) {
            NodeData::Identifier(_) => self.get_resolved_symbol(e),
            NodeData::PropertyAccessExpression(data) => {
                let (expression, name) = (data.expression, data.name);
                let expression = expression.expect("access has an expression");
                let name = name.expect("access has a name");
                let lhs_type = self.get_type_of_expression(expression)?;
                if self.kind_of(name) == SyntaxKind::PrivateIdentifier {
                    // tryGetPrivateIdentifierPropertyOfType (72975-72978).
                    let Some(name_text) = self.identifier_text_of(name).map(str::to_owned)
                    else {
                        return Ok(None);
                    };
                    let lexically_scoped = self
                        .lookup_symbol_for_private_identifier_declaration(&name_text, name)?;
                    let Some(lexically_scoped) = lexically_scoped else {
                        return Ok(None);
                    };
                    return self.get_private_identifier_property_of_type(lhs_type, lexically_scoped);
                }
                let Some(name_text) = self.identifier_text_of(name).map(str::to_owned) else {
                    return Ok(None);
                };
                self.get_property_of_type_full(lhs_type, &name_text)
            }
            NodeData::ElementAccessExpression(data) => {
                let (expression, argument) = (data.expression, data.argument_expression);
                let expression = expression.expect("access has an expression");
                let Some(argument) = argument else {
                    return Ok(None);
                };
                let prop_type =
                    self.check_expression_cached(argument, tsrs2_types::CheckMode::NORMAL)?;
                let Some(name_text) = self.property_name_from_type_usable(prop_type) else {
                    return Ok(None);
                };
                let lhs_type = self.get_type_of_expression(expression)?;
                self.get_property_of_type_full(lhs_type, &name_text)
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: getContextualTypeForAssignmentDeclaration @6.0.3
    /// tsc-hash: 8ab1d325cd62ac8ae1977531d737fddfbc18d36415268f97afa6801f97b02eb2
    /// tsc-span: _tsc.js:72980-73052
    ///
    /// TS-visible kinds only: getAssignmentDeclarationKind maps every
    /// worker answer except Property(5) to None(0) outside JS files
    /// (15055-15058), so the ThisProperty/Exports/Prototype/
    /// ModuleExports arms are JS-only [JSDOC] — JS files escape whole.
    fn get_contextual_type_for_assignment_declaration(
        &mut self,
        binary_expression: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.is_in_js_file(binary_expression) {
            return Err(Unsupported::new(
                "getContextualTypeForAssignmentDeclaration JS kinds ([JSDOC] band, M8)",
            ));
        }
        let NodeData::BinaryExpression(data) = self.data_of(binary_expression) else {
            unreachable!("kind/data agree");
        };
        let left = data.left.expect("assignment has a left side");
        let kind = self.assignment_declaration_kind_ts(binary_expression);
        match kind {
            TsAssignmentDeclarationKind::None => {
                // The kind-0 head is shared with ThisProperty in tsc;
                // only the TS-reachable half ports (`this.x = e` maps
                // to kind 0 in TS files).
                let lhs_symbol = self.get_symbol_for_expression(left)?;
                let decl =
                    lhs_symbol.and_then(|symbol| self.binder.symbol(symbol).value_declaration);
                if let Some(decl) = decl {
                    if matches!(
                        self.kind_of(decl),
                        SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature
                    ) {
                        if let Some(annotation) = self.effective_type_annotation_node(decl) {
                            let annotated = self.get_type_from_type_node(annotation)?;
                            let mapper = self
                                .links
                                .symbol(lhs_symbol.expect("decl implies symbol"))
                                .mapper;
                            return Ok(Some(self.instantiate_type(annotated, mapper)?));
                        }
                        if self.kind_of(decl) == SyntaxKind::PropertyDeclaration {
                            let has_initializer = matches!(
                                self.data_of(decl),
                                NodeData::PropertyDeclaration(data) if data.initializer.is_some()
                            );
                            if has_initializer {
                                return Ok(Some(self.get_type_of_expression(left)?));
                            }
                        }
                        return Ok(None);
                    }
                }
                Ok(Some(self.get_type_of_expression(left)?))
            }
            TsAssignmentDeclarationKind::Property => {
                // isPossiblyAliasedThisProperty is JS-gated for kind 5
                // (73053-73063) — false in TS. `left.symbol` is a
                // declaration-site symbol; TS binary assignments never
                // declare, so the node-symbol arm answers None and the
                // getTypeOfExpression(left) arm is the TS shape.
                if self.node_symbol(left).is_none() {
                    return Ok(Some(self.get_type_of_expression(left)?));
                }
                let symbol = self.node_symbol(left).expect("guarded above");
                let Some(decl) = self.binder.symbol(symbol).value_declaration else {
                    return Ok(None);
                };
                if let Some(annotation) = self.effective_type_annotation_node(decl) {
                    return Ok(Some(self.get_type_from_type_node(annotation)?));
                }
                let NodeData::PropertyAccessExpression(access) = self.data_of(left) else {
                    // Element-access Property assignments carry the
                    // same tail; the identifier probe below only
                    // applies to property accesses in tsc, so fall
                    // through to the final arm.
                    return Ok(if decl == left {
                        None
                    } else {
                        Some(self.get_type_of_expression(left)?)
                    });
                };
                let lhs_expression = access.expression.expect("access has an expression");
                if let Some(id_text) = self.identifier_text_of(lhs_expression).map(str::to_owned) {
                    let parent_symbol =
                        self.resolve_value_name_no_report(lhs_expression, &id_text)?;
                    if let Some(parent_symbol) = parent_symbol {
                        let annotated = self
                            .binder
                            .symbol(parent_symbol)
                            .value_declaration
                            .and_then(|d| self.effective_type_annotation_node(d));
                        if let Some(annotated) = annotated {
                            let name_str = self.element_or_property_access_name(left);
                            if let Some(name_str) = name_str {
                                let annotated_type = self.get_type_from_type_node(annotated)?;
                                return self.get_type_of_property_of_contextual_type(
                                    annotated_type,
                                    &name_str,
                                    None,
                                );
                            }
                        }
                        return Ok(None);
                    }
                }
                Ok(if decl == left {
                    None
                } else {
                    Some(self.get_type_of_expression(left)?)
                })
            }
        }
    }

    /// tsc getAssignmentDeclarationKind (15055-15058) restricted to TS
    /// files: `special === Property || isInJSFile ? special : None` —
    /// callers gate JS files out, so only the worker's Property answer
    /// survives. The worker (15095-15117): binary `=` with an access
    /// LHS and a non-`void 0` RHS, classified by the LHS shape
    /// (15146-15177); the Object.defineProperty call kinds and the
    /// prototype/exports kinds all map to None in TS.
    fn assignment_declaration_kind_ts(&self, expr: NodeId) -> TsAssignmentDeclarationKind {
        let NodeData::BinaryExpression(data) = self.data_of(expr) else {
            return TsAssignmentDeclarationKind::None;
        };
        let (Some(left), Some(op), Some(right)) = (data.left, data.operator_token, data.right)
        else {
            return TsAssignmentDeclarationKind::None;
        };
        if self.kind_of(op) != SyntaxKind::EqualsToken
            || !matches!(
                self.kind_of(left),
                SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
            )
        {
            return TsAssignmentDeclarationKind::None;
        }
        if self.is_void_zero(self.get_right_most_assigned_expression(right)) {
            return TsAssignmentDeclarationKind::None;
        }
        // getAssignmentDeclarationPropertyAccessKind (15146): the
        // Property answer requires a bindable static name expression
        // LHS whose base chain is entity names (this-LHS is
        // ThisProperty → None in TS; module/exports kinds → None).
        let this_lhs = match self.data_of(left) {
            NodeData::PropertyAccessExpression(data) => data
                .expression
                .is_some_and(|e| self.kind_of(e) == SyntaxKind::ThisKeyword),
            NodeData::ElementAccessExpression(data) => data
                .expression
                .is_some_and(|e| self.kind_of(e) == SyntaxKind::ThisKeyword),
            _ => false,
        };
        if this_lhs {
            return TsAssignmentDeclarationKind::None;
        }
        let base = match self.data_of(left) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => None,
        };
        let Some(base) = base else {
            return TsAssignmentDeclarationKind::None;
        };
        if !self.is_bindable_static_name_expression(base, /*exclude_this*/ true) {
            return TsAssignmentDeclarationKind::None;
        }
        if self.is_prototype_access(base) {
            // PrototypeProperty — None in TS.
            return TsAssignmentDeclarationKind::None;
        }
        // The exports/module heads are None in TS regardless; the
        // remaining test is isBindableStaticNameExpression(lhs, true)
        // or a dynamic element access.
        if self.is_bindable_static_name_expression(left, /*exclude_this*/ true)
            || (self.kind_of(left) == SyntaxKind::ElementAccessExpression
                && node_util::has_dynamic_name(self.binder.source_of_node(left), left))
        {
            return TsAssignmentDeclarationKind::Property;
        }
        TsAssignmentDeclarationKind::None
    }

    /// tsc getRightMostAssignedExpression (15037-15046).
    fn get_right_most_assigned_expression(&self, mut node: NodeId) -> NodeId {
        loop {
            let NodeData::BinaryExpression(data) = self.data_of(node) else {
                return node;
            };
            let is_plain_assignment = data
                .operator_token
                .is_some_and(|op| self.kind_of(op) == SyntaxKind::EqualsToken);
            let Some(right) = data.right else {
                return node;
            };
            if !is_plain_assignment {
                return node;
            }
            node = right;
        }
    }

    /// tsc isVoidZero (15118-15120).
    fn is_void_zero(&self, node: NodeId) -> bool {
        let NodeData::VoidExpression(data) = self.data_of(node) else {
            return false;
        };
        data.expression.is_some_and(
            |e| matches!(self.data_of(e), NodeData::NumericLiteral(data) if data.text == "0"),
        )
    }

    /// tsc isBindableStaticNameExpression (15090-15092) =
    /// isEntityNameExpression ‖ isBindableStaticAccessExpression; the
    /// literal-like element-access recursion (15075-15089) folded in.
    fn is_bindable_static_name_expression(&self, node: NodeId, exclude_this: bool) -> bool {
        if self.is_entity_name_expression(node) {
            return true;
        }
        self.is_bindable_static_access_expression(node, exclude_this)
    }

    /// tsc isBindableStaticAccessExpression (15075-15082).
    fn is_bindable_static_access_expression(&self, node: NodeId, exclude_this: bool) -> bool {
        match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => {
                let expression = data.expression;
                let name = data.name;
                let this_base =
                    expression.is_some_and(|e| self.kind_of(e) == SyntaxKind::ThisKeyword);
                if !exclude_this && this_base {
                    return true;
                }
                name.is_some_and(|n| self.kind_of(n) == SyntaxKind::Identifier)
                    && expression.is_some_and(|e| {
                        self.is_bindable_static_name_expression(e, /*exclude_this*/ true)
                    })
            }
            NodeData::ElementAccessExpression(data) => {
                // isBindableStaticElementAccessExpression (15083-15089):
                // literal-like argument + this/entity/bindable base.
                let literal_like = data.argument_expression.is_some_and(|arg| {
                    matches!(
                        self.kind_of(arg),
                        SyntaxKind::StringLiteral | SyntaxKind::NumericLiteral
                    )
                });
                if !literal_like {
                    return false;
                }
                let Some(expression) = data.expression else {
                    return false;
                };
                let this_base = self.kind_of(expression) == SyntaxKind::ThisKeyword;
                (!exclude_this && this_base)
                    || self.is_entity_name_expression(expression)
                    || self.is_bindable_static_access_expression(expression, true)
            }
            _ => false,
        }
    }

    /// tsc isPrototypeAccess: bindable access whose name is
    /// "prototype".
    fn is_prototype_access(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) && self.element_or_property_access_name(node).as_deref() == Some("prototype")
    }

    /// tsc getElementOrPropertyAccessName (15134-15145): the identifier
    /// name or the string/numeric literal argument text (un-escaped).
    fn element_or_property_access_name(&self, node: NodeId) -> Option<String> {
        match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => {
                let name = data.name?;
                self.identifier_text_of(name).map(str::to_owned)
            }
            NodeData::ElementAccessExpression(data) => {
                let source = self.binder.source_of_node(node);
                let arg = node_util::skip_parentheses_pub(source, data.argument_expression?);
                match self.data_of(arg) {
                    NodeData::StringLiteral(data) => Some(data.text.clone()),
                    NodeData::NumericLiteral(data) => Some(data.text.clone()),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// resolveName(Value, no-report, isUse) — the kind-5 parent-symbol
    /// probe (73020-73030).
    fn resolve_value_name_no_report(
        &mut self,
        location: NodeId,
        name: &str,
    ) -> CheckResult2<Option<SymbolId>> {
        self.resolve_name(
            Some(location),
            name,
            SymbolFlags::VALUE,
            /*name_not_found_message*/ None,
            /*is_use*/ true,
            /*exclude_globals*/ false,
        )
    }
}

/// The TS-visible slice of tsc AssignmentDeclarationKind: everything
/// except Property maps to None outside JS files (15055-15058).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TsAssignmentDeclarationKind {
    None,
    Property,
}

/// tsc FunctionFlags (15807): Normal 0, Generator 1, Async 2,
/// Invalid 4.
pub(crate) const FUNCTION_FLAGS_GENERATOR: u32 = 1;
pub(crate) const FUNCTION_FLAGS_ASYNC: u32 = 2;
pub(crate) const FUNCTION_FLAGS_INVALID: u32 = 4;

impl<'a> CheckerState<'a> {
    /// tsc-port: getFunctionFlags @6.0.3
    /// tsc-hash: e20bbca2eb9fbae4b851ad4664eb23bae2901c534f2b9b1eec097be1dc56c3fb
    /// tsc-span: _tsc.js:15810-15833
    ///
    /// The generator/async switch covers FunctionDeclaration/
    /// FunctionExpression/MethodDeclaration/ArrowFunction; every other
    /// function-like kind (constructor, accessors, signatures) skips
    /// it but still takes the missing-body Invalid bit.
    pub(crate) fn get_function_flags(&self, node: NodeId) -> u32 {
        let mut flags = 0;
        let (asterisk_eligible, async_eligible) = match self.kind_of(node) {
            SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::MethodDeclaration => (true, true),
            SyntaxKind::ArrowFunction => (false, true),
            _ => (false, false),
        };
        if asterisk_eligible {
            let asterisk = match self.data_of(node) {
                NodeData::FunctionDeclaration(data) => data.asterisk_token.is_some(),
                NodeData::FunctionExpression(data) => data.asterisk_token.is_some(),
                NodeData::MethodDeclaration(data) => data.asterisk_token.is_some(),
                _ => false,
            };
            if asterisk {
                flags |= FUNCTION_FLAGS_GENERATOR;
            }
        }
        if async_eligible
            && node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::ASYNC,
            )
        {
            flags |= FUNCTION_FLAGS_ASYNC;
        }
        let body = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::ArrowFunction(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            _ => None,
        };
        if body.is_none() {
            flags |= FUNCTION_FLAGS_INVALID;
        }
        flags
    }

    // ---- property-of-contextual-type family (73112-73203) ----

    /// tsc-port: getTypeOfPropertyOfContextualType @6.0.3
    /// tsc-hash: ec4d63f5248309d0acfd4a38b4b9e2e26840d7b409f04390cd5d3333d849d0cd
    /// tsc-span: _tsc.js:73112-73162
    ///
    /// The generic-mapped-type arms ride is_generic_mapped_type (a
    /// constant-false stub until M8), so
    /// getIndexedMappedTypeSubstitutedTypeOfContextualType is
    /// unreachable — the escape below keeps the arm honest if M8's
    /// stub un-stubs first.
    pub(crate) fn get_type_of_property_of_contextual_type(
        &mut self,
        ty: TypeId,
        name: &str,
        name_type: Option<TypeId>,
    ) -> CheckResult2<Option<TypeId>> {
        let name = name.to_owned();
        self.map_type(
            ty,
            &mut |state, t| {
                if state.tables.flags_of(t).intersects(TypeFlags::INTERSECTION) {
                    let constituents = match &state.tables.type_of(t).data {
                        TypeData::Intersection { types } => types.to_vec(),
                        _ => unreachable!("intersection flag implies payload"),
                    };
                    let mut types: Vec<TypeId> = Vec::new();
                    let mut index_info_candidates: Vec<TypeId> = Vec::new();
                    let mut ignore_index_infos = false;
                    for constituent in constituents {
                        if !state
                            .tables
                            .flags_of(constituent)
                            .intersects(TypeFlags::OBJECT)
                        {
                            continue;
                        }
                        if state.is_generic_mapped_type_state(constituent) {
                            return Err(Unsupported::new(
                                "getIndexedMappedTypeSubstitutedTypeOfContextualType (mapped types, M8)",
                            ));
                        }
                        let property_type =
                            state.get_type_of_concrete_property_of_contextual_type(constituent, &name)?;
                        let Some(property_type) = property_type else {
                            if !ignore_index_infos {
                                index_info_candidates.push(constituent);
                            }
                            continue;
                        };
                        ignore_index_infos = true;
                        index_info_candidates.clear();
                        state.append_contextual_property_type_constituent(&mut types, Some(property_type));
                    }
                    for candidate in index_info_candidates {
                        let index_info_type = state
                            .get_type_from_index_infos_of_contextual_type(candidate, &name, name_type)?;
                        state.append_contextual_property_type_constituent(&mut types, index_info_type);
                    }
                    if types.is_empty() {
                        return Ok(None);
                    }
                    if types.len() == 1 {
                        return Ok(Some(types[0]));
                    }
                    return state
                        .get_intersection_type(&types, tsrs2_types::IntersectionFlags::NONE)
                        .map(Some);
                }
                if !state.tables.flags_of(t).intersects(TypeFlags::OBJECT) {
                    return Ok(None);
                }
                if state.is_generic_mapped_type_state(t) {
                    return Err(Unsupported::new(
                        "getIndexedMappedTypeSubstitutedTypeOfContextualType (mapped types, M8)",
                    ));
                }
                match state.get_type_of_concrete_property_of_contextual_type(t, &name)? {
                    Some(property_type) => {
                        // appendContextualPropertyTypeConstituent's
                        // any→unknown laundering applies on the
                        // intersection path only; the plain path
                        // returns the property type as-is.
                        Ok(Some(property_type))
                    }
                    None => state.get_type_from_index_infos_of_contextual_type(t, &name, name_type),
                }
            },
            true,
        )
    }

    /// tsc-port: appendContextualPropertyTypeConstituent @6.0.3
    /// tsc-hash: 48d80cfc2711af79365c5414a651c4f62ee3633b6bda09fd927b38b1f2233b86
    /// tsc-span: _tsc.js:73163-73165
    ///
    /// `type.flags & Any ? unknownType : type` — the any→unknown
    /// laundering that keeps a contextual `any` member from swallowing
    /// the whole intersection.
    fn append_contextual_property_type_constituent(
        &self,
        types: &mut Vec<TypeId>,
        ty: Option<TypeId>,
    ) {
        let Some(ty) = ty else { return };
        types.push(if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            self.tables.intrinsics.unknown
        } else {
            ty
        });
    }

    /// tsc-port: getTypeOfConcretePropertyOfContextualType @6.0.3
    /// tsc-hash: a43fe4c12bceede221999a4ac05100e280a4f8caab2099195e2bc86334d2afe8
    /// tsc-span: _tsc.js:73178-73184
    ///
    /// isCircularMappedProperty (73099-73101) reads CheckFlags::MAPPED
    /// — unconstructible until M8, so the probe reduces to the
    /// property read.
    fn get_type_of_concrete_property_of_contextual_type(
        &mut self,
        ty: TypeId,
        name: &str,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(prop) = self.get_property_of_type_full(ty, name)? else {
            return Ok(None);
        };
        if self.get_check_flags(prop).intersects(CheckFlags::MAPPED) {
            return Err(Unsupported::new(
                "isCircularMappedProperty (mapped-type properties, M8)",
            ));
        }
        let prop_type = self.get_type_of_symbol(prop)?;
        let optional = self.symbol_flags(prop).intersects(SymbolFlags::OPTIONAL);
        Ok(Some(self.remove_missing_type(prop_type, optional)))
    }

    /// tsc-port: getTypeFromIndexInfosOfContextualType @6.0.3
    /// tsc-hash: 7cc354d24fafe9c954bd57a3d377d49dfb4d52f364497d10d612f50d2d95c28d
    /// tsc-span: _tsc.js:73185-73203
    fn get_type_from_index_infos_of_contextual_type(
        &mut self,
        ty: TypeId,
        name: &str,
        name_type: Option<TypeId>,
    ) -> CheckResult2<Option<TypeId>> {
        if self.tables.is_tuple_type(ty) && is_numeric_literal_name(name) {
            let parsed = name.parse::<f64>().unwrap_or(-1.0);
            if parsed >= 0.0 {
                let target = self.tables.reference_target(ty);
                let fixed_length = match &self.tables.type_of(target).data {
                    TypeData::TupleTarget(data) => data.fixed_length,
                    _ => unreachable!("tuple type has a tuple target"),
                };
                let rest_type = self.get_element_type_of_slice_of_tuple_type(
                    ty,
                    fixed_length,
                    /*end_skip_count*/ 0,
                    /*writing*/ false,
                    /*no_reductions*/ true,
                )?;
                if let Some(rest_type) = rest_type {
                    return Ok(Some(rest_type));
                }
            }
        }
        let key_type = match name_type {
            Some(name_type) => name_type,
            None => self.tables.get_string_literal_type(name),
        };
        Ok(self
            .get_applicable_index_info(ty, key_type)?
            .map(|info| info.value_type))
    }

    // ---- object-literal / array / conditional arms (73204-73298) ----

    /// tsc-port: getContextualTypeForObjectLiteralMethod @6.0.3
    /// tsc-hash: b0d5c6ffb99e432ae37da5d75d23788e0911e7fb0dbe36e273c85a53772fbf44
    /// tsc-span: _tsc.js:73204-73210
    pub(crate) fn get_contextual_type_for_object_literal_method(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        debug_assert!(self.is_object_literal_method(node));
        if self.node_flags(node) & NodeFlags::IN_WITH_STATEMENT.bits() != 0 {
            return Ok(None);
        }
        self.get_contextual_type_for_object_literal_element(node, context_flags)
    }

    /// tsc-port: getContextualTypeForObjectLiteralElement @6.0.3
    /// tsc-hash: 897522d69bac932ae4d3e05850282a0b1c05b992bf49ada7afea8a6e96ae27d7
    /// tsc-span: _tsc.js:73211-73247
    fn get_contextual_type_for_object_literal_element(
        &mut self,
        element: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let object_literal = self.parent_of(element).expect("element has a literal");
        if self.kind_of(element) == SyntaxKind::PropertyAssignment {
            if let Some(property_assignment_type) =
                self.get_contextual_type_for_variable_like_declaration(element, context_flags)?
            {
                return Ok(Some(property_assignment_type));
            }
        }
        let Some(ty) = self.get_apparent_type_of_contextual_type(object_literal, context_flags)?
        else {
            return Ok(None);
        };
        let source = self.binder.source_of_node(element);
        if !node_util::has_dynamic_name(source, element) {
            // hasBindableName: static (non-dynamic) names bind through
            // the symbol; the late-bound half rides has_dynamic_name
            // below.
            let symbol = self.get_symbol_of_declaration(element)?;
            let name = self.binder.symbol(symbol).escaped_name.clone();
            let name_type = self.links.symbol(symbol).name_type;
            return self.get_type_of_property_of_contextual_type(ty, &name, name_type);
        }
        if let Some(name) = self.name_of_node(element) {
            if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                let NodeData::ComputedPropertyName(data) = self.data_of(name) else {
                    unreachable!("kind/data agree");
                };
                if let Some(expression) = data.expression {
                    let expr_type =
                        self.check_expression(expression, tsrs2_types::CheckMode::NORMAL)?;
                    if let Some(text) = self.property_name_from_type_usable(expr_type) {
                        if let Some(prop_type) =
                            self.get_type_of_property_of_contextual_type(ty, &text, None)?
                        {
                            return Ok(Some(prop_type));
                        }
                    }
                }
            }
        }
        if let Some(name) = self.name_of_node(element) {
            let name_type = self.get_literal_type_from_property_name(name)?;
            return self.map_type(
                ty,
                &mut |state, t| {
                    Ok(state
                        .get_applicable_index_info(t, name_type)?
                        .map(|info| info.value_type))
                },
                true,
            );
        }
        Ok(None)
    }

    /// tsc-port: getSpreadIndices @6.0.3
    /// tsc-hash: 80a2a871cfc376cb9f1e77a7807c8db11c9ebf9fa172ced4f9c8b903d256d03d
    /// tsc-span: _tsc.js:73248-73257
    fn get_spread_indices(&self, elements: &[NodeId]) -> (Option<u32>, Option<u32>) {
        let mut first = None;
        let mut last = None;
        for (i, &element) in elements.iter().enumerate() {
            if self.kind_of(element) == SyntaxKind::SpreadElement {
                first.get_or_insert(i as u32);
                last = Some(i as u32);
            }
        }
        (first, last)
    }

    /// The 2-argument form of getContextualTypeForElementExpression
    /// (binding-element recursion, 72745): length/spread bounds absent.
    fn get_contextual_type_for_element_expression_simple(
        &mut self,
        ty: TypeId,
        index: usize,
    ) -> CheckResult2<Option<TypeId>> {
        self.get_contextual_type_for_element_expression(Some(ty), index, None, None, None)
    }

    /// getSpreadArgumentType's tuple-rest contextual read (76029):
    /// (type, index, length) with no spread-index bookkeeping.
    pub(crate) fn get_contextual_type_for_element_expression_at(
        &mut self,
        ty: TypeId,
        index: usize,
        length: Option<usize>,
    ) -> CheckResult2<Option<TypeId>> {
        self.get_contextual_type_for_element_expression(Some(ty), index, length, None, None)
    }

    /// tsc-port: getContextualTypeForElementExpression @6.0.3
    /// tsc-hash: 411daddedf4c16bb0699c229832288924ebb6fc8e3370d3ca750c6503fa3ae99
    /// tsc-span: _tsc.js:73258-73294
    ///
    /// TRANSCRIBED QUIRKS (extraction doc §4): `elementFlags[index] &&
    /// 2` is a literal `&&` in the bundle — isOptional is truthy for
    /// EVERY nonzero element flag, not just Optional; and
    /// `!firstSpreadIndex` is a falsy test — an index-0 spread behaves
    /// like no spread on the numeric-name fast path.
    fn get_contextual_type_for_element_expression(
        &mut self,
        ty: Option<TypeId>,
        index: usize,
        length: Option<usize>,
        first_spread_index: Option<u32>,
        last_spread_index: Option<u32>,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(ty) = ty else {
            return Ok(None);
        };
        self.map_type(
            ty,
            &mut |state, t| {
                if state.tables.is_tuple_type(t) {
                    let target = state.tables.reference_target(t);
                    let (fixed_length, element_flags, combined_variable) =
                        match &state.tables.type_of(target).data {
                            TypeData::TupleTarget(data) => (
                                data.fixed_length,
                                data.element_flags.to_vec(),
                                data.combined_flags
                                    .intersects(tsrs2_types::ElementFlags::VARIABLE),
                            ),
                            _ => unreachable!("tuple type has a tuple target"),
                        };
                    if (first_spread_index.is_none()
                        || index < first_spread_index.unwrap() as usize)
                        && index < fixed_length
                    {
                        let element = state.get_type_arguments(t)?[index];
                        // QUIRK: literal `&&` — truthy for any nonzero
                        // flag word (transcribed verbatim).
                        let is_optional = !element_flags[index].is_empty();
                        return Ok(Some(state.remove_missing_type(element, is_optional)));
                    }
                    let offset = match (length, last_spread_index) {
                        (Some(length), None) => length - index,
                        (Some(length), Some(last)) if index > last as usize => length - index,
                        _ => 0,
                    };
                    let fixed_end_length = if offset > 0 && combined_variable {
                        crate::structural::end_element_count(
                            &element_flags,
                            tsrs2_types::ElementFlags::FIXED,
                        )
                    } else {
                        0
                    };
                    if offset > 0 && offset <= fixed_end_length {
                        // getTypeReferenceArity = the target's element
                        // count for tuples.
                        let type_arguments = state.get_type_arguments(t)?;
                        let arity = element_flags.len();
                        return Ok(Some(type_arguments[arity - offset]));
                    }
                    let end_skip_count = match (length, last_spread_index) {
                        (Some(length), Some(last)) => fixed_end_length.min(length - last as usize),
                        _ => fixed_end_length,
                    };
                    return state.get_element_type_of_slice_of_tuple_type(
                        t,
                        match first_spread_index {
                            None => fixed_length,
                            Some(first) => fixed_length.min(first as usize),
                        },
                        end_skip_count,
                        /*writing*/ false,
                        /*no_reductions*/ true,
                    );
                }
                // QUIRK: `!firstSpreadIndex` — falsy test; Some(0)
                // behaves like None (transcribed verbatim).
                let no_spread_before = match first_spread_index {
                    None | Some(0) => true,
                    Some(first) => index < first as usize,
                };
                if no_spread_before {
                    let name = index.to_string();
                    if let Some(prop_type) =
                        state.get_type_of_property_of_contextual_type(t, &name, None)?
                    {
                        return Ok(Some(prop_type));
                    }
                }
                // 73282-73289: the silent Element probe.
                let undefined_type = state.tables.intrinsics.undefined;
                state.get_iterated_type_or_element_type(
                    tsrs2_types::IterationUse::ELEMENT,
                    t,
                    undefined_type,
                    /*error_node*/ None,
                    /*check_assignability*/ false,
                )
            },
            true,
        )
    }

    /// tsc-port: getContextualTypeForConditionalOperand @6.0.3
    /// tsc-hash: ff8d50438ae5114b6116a15913a788b84a397fdc688cf892146053b13eb0e81c
    /// tsc-span: _tsc.js:73295-73298
    fn get_contextual_type_for_conditional_operand(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let conditional = self.parent_of(node).expect("operand has a conditional");
        let NodeData::ConditionalExpression(data) = self.data_of(conditional) else {
            unreachable!("kind/data agree");
        };
        if Some(node) == data.when_true || Some(node) == data.when_false {
            self.get_contextual_type(conditional, context_flags)
        } else {
            Ok(None)
        }
    }

    // ---- discrimination (73336-73390) ----

    /// tsc-port: isPossiblyDiscriminantValue @6.0.3
    /// tsc-hash: 3305c1bd73352b6f59ea64044a8707a658c129243f5fc1922e571a2e09d13472
    /// tsc-span: _tsc.js:73336-73356
    fn is_possibly_discriminant_value(&self, node: NodeId) -> bool {
        match self.kind_of(node) {
            SyntaxKind::StringLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::TemplateExpression
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::Identifier
            | SyntaxKind::UndefinedKeyword => true,
            SyntaxKind::PropertyAccessExpression => match self.data_of(node) {
                NodeData::PropertyAccessExpression(data) => data
                    .expression
                    .is_some_and(|e| self.is_possibly_discriminant_value(e)),
                _ => false,
            },
            SyntaxKind::ParenthesizedExpression => match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => data
                    .expression
                    .is_some_and(|e| self.is_possibly_discriminant_value(e)),
                _ => false,
            },
            SyntaxKind::JsxExpression => match self.data_of(node) {
                NodeData::JsxExpression(data) => match data.expression {
                    None => true,
                    Some(e) => self.is_possibly_discriminant_value(e),
                },
                _ => false,
            },
            _ => false,
        }
    }

    /// tsc-port: getMatchingUnionConstituentForObjectLiteral @6.0.3
    /// tsc-hash: db09013c68166b9ce42115f32f7faf9a1dbf4fca7f7700428f6df1bae9d6d1a0
    /// tsc-span: _tsc.js:69635-69640
    fn get_matching_union_constituent_for_object_literal(
        &mut self,
        union_type: TypeId,
        node: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(key_property_name) = self.get_key_property_name(union_type)? else {
            return Ok(None);
        };
        let properties = match self.data_of(node) {
            NodeData::ObjectLiteralExpression(data) => data.properties,
            _ => None,
        };
        let prop_node = self.nodes_of(properties).iter().copied().find(|&p| {
            self.node_symbol(p).is_some()
                && self.kind_of(p) == SyntaxKind::PropertyAssignment
                && self
                    .node_symbol(p)
                    .is_some_and(|s| self.binder.symbol(s).escaped_name == key_property_name)
                && matches!(
                    self.data_of(p),
                    NodeData::PropertyAssignment(data)
                        if data.initializer.is_some_and(|i| self.is_possibly_discriminant_value(i))
                )
        });
        let Some(prop_node) = prop_node else {
            return Ok(None);
        };
        let NodeData::PropertyAssignment(data) = self.data_of(prop_node) else {
            unreachable!("found above");
        };
        let initializer = data.initializer.expect("filtered above");
        let prop_type = self.get_context_free_type_of_expression(initializer)?;
        self.get_constituent_type_for_key_type(union_type, prop_type)
    }

    /// tsc-port: discriminateContextualTypeByObjectMembers @6.0.3
    /// tsc-hash: 0a13461fbb35ef1e3ab737b11ef90c8335c1ef340c1c7645547f52f876ef68fe
    /// tsc-span: _tsc.js:73357-73390
    fn discriminate_contextual_type_by_object_members(
        &mut self,
        node: NodeId,
        contextual_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let key = format!("D{},{}", node.0, contextual_type.0);
        if let Some(cached) = self.get_cached_type(&key) {
            return Ok(cached);
        }
        if let Some(matched) =
            self.get_matching_union_constituent_for_object_literal(contextual_type, node)?
        {
            return Ok(self.set_cached_type(key, matched));
        }
        let mut discriminators: Vec<(ContextualDiscriminator, String)> = Vec::new();
        let properties = match self.data_of(node) {
            NodeData::ObjectLiteralExpression(data) => data.properties,
            _ => None,
        };
        {
            for p in self.nodes_of(properties) {
                let Some(symbol) = self.node_symbol(p) else {
                    continue;
                };
                let escaped_name = self.binder.symbol(symbol).escaped_name.clone();
                match self.kind_of(p) {
                    SyntaxKind::PropertyAssignment => {
                        let NodeData::PropertyAssignment(data) = self.data_of(p) else {
                            unreachable!("kind/data agree");
                        };
                        let Some(initializer) = data.initializer else {
                            continue;
                        };
                        if self.is_possibly_discriminant_value(initializer)
                            && self.is_discriminant_property(contextual_type, &escaped_name)?
                        {
                            discriminators.push((
                                ContextualDiscriminator::ContextFree(initializer),
                                escaped_name,
                            ));
                        }
                    }
                    SyntaxKind::ShorthandPropertyAssignment => {
                        let NodeData::ShorthandPropertyAssignment(data) = self.data_of(p) else {
                            unreachable!("kind/data agree");
                        };
                        let Some(name) = data.name else { continue };
                        if self.is_discriminant_property(contextual_type, &escaped_name)? {
                            discriminators
                                .push((ContextualDiscriminator::ContextFree(name), escaped_name));
                        }
                    }
                    _ => {}
                }
            }
        }
        let node_members: Option<&tsrs2_binder::SymbolTable> = self
            .node_symbol(node)
            .map(|s| &self.binder.symbol(s).members);
        let has_members = node_members.is_some_and(|m| !m.is_empty());
        let mut absent_optional: Vec<String> = Vec::new();
        if has_members {
            for s in self.get_properties_of_type(contextual_type)? {
                if !self.symbol_flags(s).intersects(SymbolFlags::OPTIONAL) {
                    continue;
                }
                let name = self.binder.symbol(s).escaped_name.clone();
                let node_symbol = self.node_symbol(node).expect("has_members implies symbol");
                if self.binder.symbol(node_symbol).members.get(&name).is_some() {
                    continue;
                }
                if self.is_discriminant_property(contextual_type, &name)? {
                    absent_optional.push(name);
                }
            }
        }
        discriminators.extend(
            absent_optional
                .into_iter()
                .map(|name| (ContextualDiscriminator::Undefined, name)),
        );
        let discriminated = self.discriminate_type_by_discriminable_items_contextual(
            contextual_type,
            &discriminators,
        )?;
        Ok(self.set_cached_type(key, discriminated))
    }

    /// tsc-port: discriminateContextualTypeByJSXAttributes @6.0.3
    /// tsc-hash: 40883fde6fc48131a8496a52df4fa2e6e2bff0bda57fd9b057681462e6e2b643
    /// tsc-span: _tsc.js:73391-73423
    fn discriminate_contextual_type_by_jsx_attributes(
        &mut self,
        node: NodeId,
        contextual_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let key = format!("D{},{}", node.0, contextual_type.0);
        if let Some(cached) = self.get_cached_type(&key) {
            return Ok(cached);
        }
        let jsx_namespace = self.get_jsx_namespace_at(node)?;
        let jsx_children_property_name =
            self.get_jsx_element_children_property_name(jsx_namespace)?;
        let mut discriminators: Vec<(ContextualDiscriminator, String)> = Vec::new();
        let properties = match self.data_of(node) {
            NodeData::JsxAttributes(data) => data.properties,
            _ => None,
        };
        for p in self.nodes_of(properties) {
            if self.kind_of(p) != SyntaxKind::JsxAttribute {
                continue;
            }
            let Some(symbol) = self.node_symbol(p) else {
                continue;
            };
            let escaped_name = self.binder.symbol(symbol).escaped_name.clone();
            if !self.is_discriminant_property(contextual_type, &escaped_name)? {
                continue;
            }
            let initializer = match self.data_of(p) {
                NodeData::JsxAttribute(data) => data.initializer,
                _ => None,
            };
            match initializer {
                None => {
                    discriminators.push((ContextualDiscriminator::True, escaped_name));
                }
                Some(initializer) if self.is_possibly_discriminant_value(initializer) => {
                    discriminators.push((
                        ContextualDiscriminator::ContextFree(initializer),
                        escaped_name,
                    ));
                }
                Some(_) => {}
            }
        }
        let has_members = self
            .node_symbol(node)
            .is_some_and(|s| !self.binder.symbol(s).members.is_empty());
        let mut absent_optional: Vec<String> = Vec::new();
        if has_members {
            for s in self.get_properties_of_type(contextual_type)? {
                if !self.symbol_flags(s).intersects(SymbolFlags::OPTIONAL) {
                    continue;
                }
                let name = self.binder.symbol(s).escaped_name.clone();
                // 73411-73414: an absent `children` attribute does not
                // discriminate when the element HAS semantic children.
                if jsx_children_property_name.as_deref() == Some(name.as_str()) {
                    let element = self.parent_of(node).and_then(|p| self.parent_of(p));
                    let has_semantic_children = element.is_some_and(|element| {
                        matches!(self.data_of(element), NodeData::JsxElement(_))
                            && self
                                .nodes_of(match self.data_of(element) {
                                    NodeData::JsxElement(data) => data.children,
                                    _ => None,
                                })
                                .into_iter()
                                .any(|child| self.is_semantic_jsx_child(child))
                    });
                    if has_semantic_children {
                        continue;
                    }
                }
                let node_symbol = self.node_symbol(node).expect("has_members implies symbol");
                if self.binder.symbol(node_symbol).members.get(&name).is_some() {
                    continue;
                }
                if self.is_discriminant_property(contextual_type, &name)? {
                    absent_optional.push(name);
                }
            }
        }
        discriminators.extend(
            absent_optional
                .into_iter()
                .map(|name| (ContextualDiscriminator::Undefined, name)),
        );
        let discriminated = self.discriminate_type_by_discriminable_items_contextual(
            contextual_type,
            &discriminators,
        )?;
        Ok(self.set_cached_type(key, discriminated))
    }

    /// tsc-port: discriminateTypeByDiscriminableItems @6.0.3 (contextual form)
    /// tsc-hash: 1290115be563c6a5dbeaf88f7facf3ed9a8fed47250275f155ab82f834058cbd
    /// tsc-span: _tsc.js:67259-67284
    ///
    /// The contextual callers' form: lazy thunk discriminators
    /// (evaluated INSIDE the include loop, zero or more times — the
    /// timing is observable through type-creation order) and
    /// `related = isTypeAssignableTo`. The RelationChecker form
    /// (structural.rs) keeps the M3 relation-error shape.
    fn discriminate_type_by_discriminable_items_contextual(
        &mut self,
        target: TypeId,
        discriminators: &[(ContextualDiscriminator, String)],
    ) -> CheckResult2<TypeId> {
        let types = match &self.tables.type_of(target).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("discrimination over a union"),
        };
        // include: 0 = False, -1 = True, 3 = Maybe (tsc Ternary words).
        let mut include: Vec<i8> = Vec::with_capacity(types.len());
        for &t in &types {
            let excluded = self.tables.flags_of(t).intersects(TypeFlags::PRIMITIVE) || {
                let reduced = self.get_reduced_type(t)?;
                self.tables.flags_of(reduced).intersects(TypeFlags::NEVER)
            };
            include.push(if excluded { 0 } else { -1 });
        }
        for (get_discriminating_type, property_name) in discriminators {
            let mut matched = false;
            for i in 0..types.len() {
                if include[i] != 0 {
                    let target_type = self
                        .get_type_of_property_or_index_signature_of_type(types[i], property_name)?;
                    if let Some(target_type) = target_type {
                        let discriminating_type = match get_discriminating_type {
                            ContextualDiscriminator::ContextFree(expr) => {
                                self.get_context_free_type_of_expression(*expr)?
                            }
                            ContextualDiscriminator::Undefined => self.tables.intrinsics.undefined,
                            ContextualDiscriminator::True => self.tables.intrinsics.true_fresh,
                        };
                        let related = self.some_type_result(discriminating_type, |state, t| {
                            state.is_type_assignable_to(t, target_type)
                        })?;
                        if related {
                            matched = true;
                        } else {
                            include[i] = 3;
                        }
                    }
                }
            }
            for slot in include.iter_mut() {
                if *slot == 3 {
                    *slot = if matched { 0 } else { -1 };
                }
            }
        }
        let filtered = if include.contains(&0) {
            let kept: Vec<TypeId> = types
                .iter()
                .zip(include.iter())
                .filter(|&(_, &inc)| inc != 0)
                .map(|(&t, _)| t)
                .collect();
            self.get_union_type_ex(&kept, UnionReduction::None)?
        } else {
            target
        };
        Ok(
            if self.tables.flags_of(filtered).intersects(TypeFlags::NEVER) {
                target
            } else {
                filtered
            },
        )
    }

    // ---- apparent + instantiate (73424-73470) ----

    /// tsc-port: getApparentTypeOfContextualType @6.0.3
    /// tsc-hash: 853b2fb000998846727ef8d54ea4a04fec7672a5957b9f992597f34bab19093e
    /// tsc-span: _tsc.js:73424-73440
    ///
    /// Mapped types are NOT apparent-ified (the L73430 comment: eager
    /// evaluation would break per-position element contextual types) —
    /// the ObjectFlags::MAPPED guard is dormant until M8. The
    /// JSX-attributes discrimination twin is [JSX → 5.5f].
    pub(crate) fn get_apparent_type_of_contextual_type(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let contextual_type = if self.is_object_literal_method(node) {
            self.get_contextual_type_for_object_literal_method(node, context_flags)?
        } else {
            self.get_contextual_type(node, context_flags)?
        };
        let instantiated_type = self.instantiate_contextual_type(contextual_type, node)?;
        let Some(instantiated_type) = instantiated_type else {
            return Ok(None);
        };
        if context_flags.intersects(ContextFlags::NO_CONSTRAINTS)
            && self
                .tables
                .flags_of(instantiated_type)
                .intersects(TypeFlags::TYPE_VARIABLE)
        {
            return Ok(None);
        }
        let apparent_type = self.map_type(
            instantiated_type,
            &mut |state, t| {
                if state
                    .tables
                    .object_flags_of(t)
                    .intersects(ObjectFlags::MAPPED)
                {
                    Ok(Some(t))
                } else {
                    state.get_apparent_type(t).map(Some)
                }
            },
            true,
        )?;
        let Some(apparent_type) = apparent_type else {
            return Ok(None);
        };
        if self
            .tables
            .flags_of(apparent_type)
            .intersects(TypeFlags::UNION)
        {
            if self.kind_of(node) == SyntaxKind::ObjectLiteralExpression {
                return self
                    .discriminate_contextual_type_by_object_members(node, apparent_type)
                    .map(Some);
            }
            if self.kind_of(node) == SyntaxKind::JsxAttributes {
                return self
                    .discriminate_contextual_type_by_jsx_attributes(node, apparent_type)
                    .map(Some);
            }
        }
        Ok(Some(apparent_type))
    }

    /// tsc-port: instantiateContextualType @6.0.3
    /// tsc-hash: 5f836db3ed2a22d7adb310d466cfd9b1f501c80ff659ea2c66cc57c81754463c
    /// tsc-span: _tsc.js:73441-73458
    ///
    /// [INFER → M6]: both mapper branches (nonFixingMapper under
    /// ContextFlags::Signature, returnMapper) require a Some inference
    /// context — the stack only ever holds None until M6, so the
    /// structural port reduces to the identity read
    /// (instantiateInstantiableTypes 73459-73470 rides with M6).
    pub(crate) fn instantiate_contextual_type(
        &mut self,
        contextual_type: Option<TypeId>,
        node: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(ty) = contextual_type {
            if self.maybe_type_of_kind(ty, TypeFlags::INSTANTIABLE) {
                if let Some(context) = self.get_inference_context(node) {
                    match context {}
                }
            }
        }
        Ok(contextual_type)
    }

    /// The driver-band consumers (checkExpressionForMutableLocation
    /// 80728, checkExpressionWithContextualType 80571) instantiate with
    /// node-independent flags — this is the same identity read.
    pub(crate) fn instantiate_contextual_type_for_node(
        &mut self,
        contextual_type: Option<TypeId>,
        node: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        self.instantiate_contextual_type(contextual_type, node)
    }

    // ---- the master switch (73471-73556) ----

    /// tsc-port: getContextualType @6.0.3
    /// tsc-hash: 02e025b3e9c853ac1497ab5d7bd33875c5e78ff40065cf2b8f3febad76aa2316
    /// tsc-span: _tsc.js:73471-73556
    ///
    /// Arm dispositions per the extraction doc §4 table; the CALLS
    /// contextual arms are live through decorators. The yield-operand
    /// arm is live since 5.8b.
    pub(crate) fn get_contextual_type(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        if self.node_flags(node) & NodeFlags::IN_WITH_STATEMENT.bits() != 0 {
            return Ok(None);
        }
        if let Some(index) = self.find_contextual_node(node, context_flags.is_empty()) {
            return Ok(self.contextual_types[index]);
        }
        let Some(parent) = self.parent_of(node) else {
            return Ok(None);
        };
        match self.kind_of(parent) {
            SyntaxKind::VariableDeclaration
            | SyntaxKind::Parameter
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::BindingElement => {
                self.get_contextual_type_for_initializer_expression(node, context_flags)
            }
            SyntaxKind::ArrowFunction | SyntaxKind::ReturnStatement => {
                self.get_contextual_type_for_return_expression(node, context_flags)
            }
            SyntaxKind::YieldExpression => {
                self.get_contextual_type_for_yield_operand(parent, context_flags)
            }
            SyntaxKind::AwaitExpression => {
                self.get_contextual_type_for_await_operand(parent, context_flags)
            }
            SyntaxKind::CallExpression | SyntaxKind::NewExpression => {
                self.get_contextual_type_for_argument(parent, node)
            }
            SyntaxKind::Decorator => self.get_contextual_type_for_decorator(parent),
            SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression => {
                let type_node = match self.data_of(parent) {
                    NodeData::AsExpression(data) => data.r#type,
                    NodeData::TypeAssertionExpression(data) => data.r#type,
                    _ => unreachable!("kind/data agree"),
                };
                let Some(type_node) = type_node else {
                    return Ok(None);
                };
                if self.is_const_type_reference_node(type_node) {
                    self.get_contextual_type(parent, context_flags)
                } else {
                    Ok(Some(self.get_type_from_type_node(type_node)?))
                }
            }
            SyntaxKind::BinaryExpression => {
                self.get_contextual_type_for_binary_operand(node, context_flags)
            }
            SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment => {
                self.get_contextual_type_for_object_literal_element(parent, context_flags)
            }
            SyntaxKind::SpreadAssignment => {
                let literal = self.parent_of(parent).expect("spread has a literal");
                self.get_contextual_type(literal, context_flags)
            }
            SyntaxKind::ArrayLiteralExpression => {
                let array_literal = parent;
                let ty = self.get_apparent_type_of_contextual_type(array_literal, context_flags)?;
                let NodeData::ArrayLiteralExpression(data) = self.data_of(array_literal) else {
                    unreachable!("kind/data agree");
                };
                let elements = self.nodes_of(data.elements);
                let Some(element_index) = elements.iter().position(|&e| e == node) else {
                    return Ok(None);
                };
                let spread_indices = match self.links.node(array_literal).spread_indices {
                    Some(cached) => cached,
                    None => {
                        let computed = self.get_spread_indices(&elements);
                        self.links.set_node_spread_indices(
                            self.speculation_depth,
                            array_literal,
                            computed,
                        );
                        computed
                    }
                };
                self.get_contextual_type_for_element_expression(
                    ty,
                    element_index,
                    Some(elements.len()),
                    spread_indices.0,
                    spread_indices.1,
                )
            }
            SyntaxKind::ConditionalExpression => {
                self.get_contextual_type_for_conditional_operand(node, context_flags)
            }
            SyntaxKind::TemplateSpan => {
                let template = self.parent_of(parent).expect("span has a template");
                debug_assert_eq!(self.kind_of(template), SyntaxKind::TemplateExpression);
                self.get_contextual_type_for_substitution_expression(template, node)
            }
            SyntaxKind::ParenthesizedExpression => {
                if self.is_in_js_file(parent) {
                    // [JSDOC] the satisfies/type-tag reads are
                    // invisible (no JSDoc parse) — the recursion below
                    // is tsc's untagged path.
                    return Err(Unsupported::new(
                        "getContextualType parenthesized JSDoc arms ([JSDOC] band, M8)",
                    ));
                }
                self.get_contextual_type(parent, context_flags)
            }
            SyntaxKind::NonNullExpression => self.get_contextual_type(parent, context_flags),
            SyntaxKind::SatisfiesExpression => {
                let NodeData::SatisfiesExpression(data) = self.data_of(parent) else {
                    unreachable!("kind/data agree");
                };
                match data.r#type {
                    Some(type_node) => Ok(Some(self.get_type_from_type_node(type_node)?)),
                    None => Ok(None),
                }
            }
            SyntaxKind::ExportAssignment => {
                // tryGetTypeFromEffectiveTypeNode: export assignments
                // carry no annotation in TS ([JSDOC] covers the JS
                // read) — tsc's own answer is undefined.
                Ok(None)
            }
            SyntaxKind::JsxExpression => {
                self.get_contextual_type_for_jsx_expression(parent, context_flags)
            }
            SyntaxKind::JsxAttribute | SyntaxKind::JsxSpreadAttribute => {
                self.get_contextual_type_for_jsx_attribute(parent, context_flags)
            }
            SyntaxKind::JsxOpeningElement | SyntaxKind::JsxSelfClosingElement => {
                self.get_contextual_jsx_element_attributes_type(parent, context_flags)
            }
            SyntaxKind::ImportAttribute => self.get_contextual_import_attribute_type(parent),
            _ => Ok(None),
        }
    }

    /// tsc-port: getContextualTypeForJsxExpression @6.0.3
    /// tsc-hash: b10ba9dda2cd44d3442c390ed7f41f269e91d44a9ff50e382f6f991800e3bcc0
    /// tsc-span: _tsc.js:73321-73324
    fn get_contextual_type_for_jsx_expression(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(expr_parent) = self.parent_of(node) else {
            return Ok(None);
        };
        match self.kind_of(expr_parent) {
            SyntaxKind::JsxAttribute | SyntaxKind::JsxSpreadAttribute => {
                self.get_contextual_type(node, context_flags)
            }
            SyntaxKind::JsxElement => {
                self.get_contextual_type_for_child_jsx_expression(expr_parent, node, context_flags)
            }
            _ => Ok(None),
        }
    }

    /// tsc-port: getContextualTypeForChildJsxExpression @6.0.3
    /// tsc-hash: a7fc76a8ae4a98cf7ed01ce13f28f0b738a923c4badbe00ae8ffe0d84b347329
    /// tsc-span: _tsc.js:73299-73320
    fn get_contextual_type_for_child_jsx_expression(
        &mut self,
        node: NodeId,
        child: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        let attributes = match self.data_of(node) {
            NodeData::JsxElement(data) => data.opening_element,
            _ => None,
        }
        .and_then(|opening| match self.data_of(opening) {
            NodeData::JsxOpeningElement(data) => data.attributes,
            _ => None,
        });
        let Some(attributes) = attributes else {
            return Ok(None);
        };
        let attributes_type =
            self.get_apparent_type_of_contextual_type(attributes, context_flags)?;
        let jsx_namespace = self.get_jsx_namespace_at(node)?;
        let jsx_children_property_name =
            self.get_jsx_element_children_property_name(jsx_namespace)?;
        let Some(attributes_type) = attributes_type else {
            return Ok(None);
        };
        if self
            .tables
            .flags_of(attributes_type)
            .intersects(TypeFlags::ANY)
        {
            return Ok(None);
        }
        let Some(children_name) = jsx_children_property_name.filter(|name| !name.is_empty()) else {
            return Ok(None);
        };
        let children = match self.data_of(node) {
            NodeData::JsxElement(data) => data.children,
            _ => None,
        };
        let real_children: Vec<NodeId> = self
            .nodes_of(children)
            .into_iter()
            .filter(|&c| self.is_semantic_jsx_child(c))
            .collect();
        let child_index = real_children.iter().position(|&c| c == child);
        let child_field_type =
            self.get_type_of_property_of_contextual_type(attributes_type, &children_name, None)?;
        let Some(child_field_type) = child_field_type else {
            return Ok(None);
        };
        if real_children.len() == 1 {
            return Ok(Some(child_field_type));
        }
        let Some(child_index) = child_index else {
            return Ok(None);
        };
        self.map_type(
            child_field_type,
            &mut |state, t| {
                if state.is_array_like_type(t)? {
                    let index_type = state.tables.get_number_literal_type(child_index as f64);
                    Ok(Some(state.get_indexed_access_type(
                        t,
                        index_type,
                        tsrs2_types::AccessFlags::NONE,
                        None,
                        None,
                        None,
                    )?))
                } else {
                    Ok(Some(t))
                }
            },
            /*no_reductions*/ true,
        )
    }

    /// tsc-port: getContextualTypeForJsxAttribute @6.0.3
    /// tsc-hash: 234873420689f4e705c11dd2aef24f03350e3ea62a81f046f4ab3714f94f8bd8
    /// tsc-span: _tsc.js:73325-73335
    fn get_contextual_type_for_jsx_attribute(
        &mut self,
        attribute: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        if self.kind_of(attribute) == SyntaxKind::JsxAttribute {
            let Some(attributes) = self.parent_of(attribute) else {
                return Ok(None);
            };
            let attributes_type =
                self.get_apparent_type_of_contextual_type(attributes, context_flags)?;
            let Some(attributes_type) = attributes_type else {
                return Ok(None);
            };
            if self
                .tables
                .flags_of(attributes_type)
                .intersects(TypeFlags::ANY)
            {
                return Ok(None);
            }
            let name = match self.data_of(attribute) {
                NodeData::JsxAttribute(data) => {
                    data.name.map(|name| self.jsx_attribute_name_text(name))
                }
                _ => None,
            };
            let Some(name) = name else {
                return Ok(None);
            };
            return self.get_type_of_property_of_contextual_type(attributes_type, &name, None);
        }
        let Some(attributes) = self.parent_of(attribute) else {
            return Ok(None);
        };
        self.get_contextual_type(attributes, context_flags)
    }

    /// tsc-port: getContextualJsxElementAttributesType @6.0.3
    /// tsc-hash: 8dde288c7eaa5923dbecc2d5aa84d4cb3b7ca6c4226c4b8fe7d48d91146c349a
    /// tsc-span: _tsc.js:73635-73647
    fn get_contextual_jsx_element_attributes_type(
        &mut self,
        node: NodeId,
        context_flags: ContextFlags,
    ) -> CheckResult2<Option<TypeId>> {
        if self.kind_of(node) == SyntaxKind::JsxOpeningElement
            && context_flags != ContextFlags::IGNORE_NODE_INFERENCES
        {
            let Some(parent) = self.parent_of(node) else {
                return Ok(None);
            };
            if let Some(index) = self.find_contextual_node(parent, context_flags.is_empty()) {
                return Ok(self.contextual_types[index]);
            }
        }
        self.get_contextual_type_for_argument_at_index(node, 0)
    }

    /// tsc-port: getContextualImportAttributeType @6.0.3
    /// tsc-hash: 49559b4ce50d3b56699b5b2b948515aa71d6c772f280e485d834181feb43636c
    /// tsc-span: _tsc.js:73629-73634
    fn get_contextual_import_attribute_type(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let attributes_type = self.get_global_import_attributes_type()?;
        let Some(name) = self.get_name_from_import_attribute(node) else {
            return Ok(None);
        };
        self.get_type_of_property_of_contextual_type(attributes_type, &name, None)
    }

    /// tsc getNameFromImportAttribute (19376-19378).
    fn get_name_from_import_attribute(&self, node: NodeId) -> Option<String> {
        let NodeData::ImportAttribute(data) = self.data_of(node) else {
            return None;
        };
        let name = data.name?;
        match self.data_of(name) {
            NodeData::Identifier(data) => Some(data.escaped_text.clone()),
            NodeData::StringLiteral(data) => Some(data.text.clone()),
            _ => None,
        }
    }

    // ---- contextual signatures (73741-73891) ----

    /// tsc-port: getIntersectedSignatures @6.0.3
    /// tsc-hash: 2aabc138cbb777a1d365326f262d73eb2e6df4f3ea1c1b0293d859c6154f63da
    /// tsc-span: _tsc.js:73741-73746
    fn get_intersected_signatures(
        &mut self,
        signatures: &[SignatureId],
    ) -> CheckResult2<Option<SignatureId>> {
        if !self
            .options
            .strict_option_value(self.options.no_implicit_any)
        {
            return Ok(None);
        }
        // reduceLeft: left===right short-circuit, then the
        // type-parameter-identity gate before each combine.
        let mut left: Option<SignatureId> = None;
        for (index, &right) in signatures.iter().enumerate() {
            if index == 0 {
                left = Some(right);
                continue;
            }
            let Some(current) = left else {
                // A prior combine answered undefined — reduceLeft keeps
                // folding but every subsequent step propagates it.
                continue;
            };
            if current == right {
                continue;
            }
            if self.compare_type_parameters_identical(current, right)? {
                left = Some(self.combine_signatures_of_intersection_members(current, right)?);
            } else {
                left = None;
            }
        }
        Ok(left)
    }

    /// tsc-port: combineIntersectionThisParam @6.0.3
    /// tsc-hash: c17c91780307e643aa4cc5139d00f4ce5caa1e4069ce2695dc4c7041dbf994b3
    /// tsc-span: _tsc.js:73747-73753
    fn combine_intersection_this_param(
        &mut self,
        left: Option<SymbolId>,
        right: Option<SymbolId>,
        mapper: Option<crate::instantiate::MapperId>,
    ) -> CheckResult2<Option<SymbolId>> {
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(left.or(right));
        };
        let left_type = self.get_type_of_symbol(left)?;
        let right_type = self.get_type_of_symbol(right)?;
        let right_type = self.instantiate_type(right_type, mapper)?;
        let this_type =
            self.get_union_type_ex(&[left_type, right_type], UnionReduction::Literal)?;
        Ok(Some(self.create_symbol_with_type(left, this_type)))
    }

    /// tsc-port: combineIntersectionParameters @6.0.3
    /// tsc-hash: 36a7a6122080bf4f9c5feb744a1c61b2ae13f042000911b77d10bd44a9a600a9
    /// tsc-span: _tsc.js:73754-73795
    fn combine_intersection_parameters(
        &mut self,
        left: SignatureId,
        right: SignatureId,
        mapper: Option<crate::instantiate::MapperId>,
    ) -> CheckResult2<Vec<SymbolId>> {
        let left_count = self.get_parameter_count(left)?;
        let right_count = self.get_parameter_count(right)?;
        let (longest, shorter) = if left_count >= right_count {
            (left, right)
        } else {
            (right, left)
        };
        let longest_count = if longest == left {
            left_count
        } else {
            right_count
        };
        let either_has_effective_rest =
            self.has_effective_rest_parameter(left)? || self.has_effective_rest_parameter(right)?;
        let needs_extra_rest_element =
            either_has_effective_rest && !self.has_effective_rest_parameter(longest)?;
        let mut params: Vec<SymbolId> = Vec::with_capacity(longest_count + 1);
        for i in 0..longest_count {
            let mut longest_param_type = self
                .try_get_type_at_position(longest, i)?
                .expect("i < longest's parameter count");
            if longest == right {
                longest_param_type = self.instantiate_type(longest_param_type, mapper)?;
            }
            let mut shorter_param_type = self
                .try_get_type_at_position(shorter, i)?
                .unwrap_or(self.tables.intrinsics.unknown);
            if shorter == right {
                shorter_param_type = self.instantiate_type(shorter_param_type, mapper)?;
            }
            let union_param_type = self.get_union_type_ex(
                &[longest_param_type, shorter_param_type],
                UnionReduction::Literal,
            )?;
            let is_rest_param =
                either_has_effective_rest && !needs_extra_rest_element && i == longest_count - 1;
            let is_optional = i >= self.get_min_argument_count(left)?
                && i >= self.get_min_argument_count(right)?;
            let left_name = if i >= left_count {
                None
            } else {
                self.get_parameter_name_at_position(left, i)?
            };
            let right_name = if i >= right_count {
                None
            } else {
                self.get_parameter_name_at_position(right, i)?
            };
            let param_name = if left_name == right_name {
                left_name
            } else if left_name.is_none() {
                right_name
            } else if right_name.is_none() {
                left_name
            } else {
                None
            };
            let flags = SymbolFlags::FUNCTION_SCOPED_VARIABLE
                | if is_optional && !is_rest_param {
                    SymbolFlags::OPTIONAL
                } else {
                    SymbolFlags::from_bits(0)
                };
            let param_symbol = self
                .binder
                .create_symbol(flags, param_name.unwrap_or_else(|| format!("arg{i}")));
            let check_flags = if is_rest_param {
                CheckFlags::REST_PARAMETER
            } else if is_optional {
                CheckFlags::OPTIONAL_PARAMETER
            } else {
                CheckFlags::from_bits(0)
            };
            self.links
                .set_symbol_check_flags(self.speculation_depth, param_symbol, check_flags);
            let param_type = if is_rest_param {
                self.create_array_type(union_param_type, false)?
            } else {
                union_param_type
            };
            self.links.set_symbol_type(
                self.speculation_depth,
                param_symbol,
                crate::links::LinkSlot::Resolved(param_type),
            );
            params.push(param_symbol);
        }
        if needs_extra_rest_element {
            let rest_param_symbol = self
                .binder
                .create_symbol(SymbolFlags::FUNCTION_SCOPED_VARIABLE, "args".to_owned());
            self.links.set_symbol_check_flags(
                self.speculation_depth,
                rest_param_symbol,
                CheckFlags::REST_PARAMETER,
            );
            let element = self.get_type_at_position(shorter, longest_count)?;
            let mut rest_type = self.create_array_type(element, false)?;
            if shorter == right {
                rest_type = self.instantiate_type(rest_type, mapper)?;
            }
            self.links.set_symbol_type(
                self.speculation_depth,
                rest_param_symbol,
                crate::links::LinkSlot::Resolved(rest_type),
            );
            params.push(rest_param_symbol);
        }
        Ok(params)
    }

    /// tsc-port: combineSignaturesOfIntersectionMembers @6.0.3
    /// tsc-hash: 67882ee737f3b9059b0476ab7ee0cc8ceb564e69770cf60c750266090b262277
    /// tsc-span: _tsc.js:73796-73829
    fn combine_signatures_of_intersection_members(
        &mut self,
        left: SignatureId,
        right: SignatureId,
    ) -> CheckResult2<SignatureId> {
        let left_tps = self.signature_of(left).type_parameters.clone();
        let right_tps = self.signature_of(right).type_parameters.clone();
        let type_params = left_tps.clone().or(right_tps.clone());
        let param_mapper = match (left_tps.as_ref(), right_tps.as_ref()) {
            (Some(left_tps), Some(right_tps)) => {
                Some(self.create_type_mapper(right_tps.clone(), Some(left_tps.clone())))
            }
            _ => None,
        };
        let propagating = tsrs2_types::SignatureFlags::PROPAGATING_FLAGS.bits()
            & !tsrs2_types::SignatureFlags::HAS_REST_PARAMETER.bits();
        let mut flags = tsrs2_types::SignatureFlags::from_bits(
            (self.signature_of(left).flags.bits() | self.signature_of(right).flags.bits())
                & propagating,
        );
        let declaration = self.signature_of(left).declaration;
        let params = self.combine_intersection_parameters(left, right, param_mapper)?;
        let last_param_is_rest = params.last().is_some_and(|&p| {
            self.get_check_flags(p)
                .intersects(CheckFlags::REST_PARAMETER)
        });
        if last_param_is_rest {
            flags = tsrs2_types::SignatureFlags::from_bits(
                flags.bits() | tsrs2_types::SignatureFlags::HAS_REST_PARAMETER.bits(),
            );
        }
        let left_this = self.signature_of(left).this_parameter;
        let right_this = self.signature_of(right).this_parameter;
        let this_param =
            self.combine_intersection_this_param(left_this, right_this, param_mapper)?;
        let min_arg_count = self
            .signature_of(left)
            .min_argument_count
            .max(self.signature_of(right).min_argument_count);
        let left_composite_kind = self.signature_of(left).composite_kind;
        let left_composite_signatures = self.signature_of(left).composite_signatures.clone();
        let left_mapper = self.signature_of(left).mapper;
        let composite_signatures = {
            let mut list = match (left_composite_kind, &left_composite_signatures) {
                (Some(TypeFlags::INTERSECTION), Some(signatures)) => signatures.clone(),
                _ => vec![left],
            };
            list.push(right);
            list
        };
        let mapper = param_mapper.map(|param_mapper| {
            match (left_composite_kind, left_mapper, &left_composite_signatures) {
                (Some(TypeFlags::INTERSECTION), Some(left_mapper), Some(_)) => {
                    self.combine_type_mappers(Some(left_mapper), param_mapper)
                }
                _ => param_mapper,
            }
        });
        let result = crate::state::Signature {
            declaration,
            flags,
            type_parameters: type_params,
            parameters: params,
            this_parameter: this_param,
            min_argument_count: min_arg_count,
            resolved_return_type: crate::links::LinkSlot::Vacant,
            from_method: self.signature_of(left).from_method,
            target: None,
            mapper,
            instantiations: std::collections::HashMap::new(),
            erased_signature_cache: None,
            composite_kind: Some(TypeFlags::INTERSECTION),
            composite_signatures: Some(composite_signatures),
            optional_call_signature_cache: (None, None),
            isolated_signature_kind: self.signature_of(left).isolated_signature_kind,
            isolated_signature_type: None,
        };
        Ok(self.alloc_signature(result))
    }

    /// tsc-port: getContextualCallSignature @6.0.3
    /// tsc-hash: 17e0e8d83c070a82bca854b226fc75adaf259bf3d35ae1552509e5009eff905c
    /// tsc-span: _tsc.js:73830-73834
    fn get_contextual_call_signature(
        &mut self,
        ty: TypeId,
        node: NodeId,
    ) -> CheckResult2<Option<SignatureId>> {
        let signatures = self.get_signatures_of_type(ty, crate::structural::SignatureKind::Call)?;
        let mut applicable_by_arity: Vec<SignatureId> = Vec::new();
        for signature in signatures {
            if !self.is_arity_smaller(signature, node)? {
                applicable_by_arity.push(signature);
            }
        }
        if applicable_by_arity.len() == 1 {
            return Ok(Some(applicable_by_arity[0]));
        }
        self.get_intersected_signatures(&applicable_by_arity)
    }

    /// tsc-port: isAritySmaller @6.0.3
    /// tsc-hash: 76c4b60c9f043739ab3c66041377a58eb6371b46bb05d7489a1b2e13bc536ab5
    /// tsc-span: _tsc.js:73835-73847
    ///
    /// isJSDocOptionalParameter is [JSDOC] (constant false — no JSDoc
    /// parse).
    fn is_arity_smaller(&mut self, signature: SignatureId, target: NodeId) -> CheckResult2<bool> {
        let parameters = self.parameters_of_function(target);
        let mut target_parameter_count = 0usize;
        while target_parameter_count < parameters.len() {
            let param = parameters[target_parameter_count];
            let NodeData::Parameter(data) = self.data_of(param) else {
                break;
            };
            if data.initializer.is_some()
                || data.question_token.is_some()
                || data.dot_dot_dot_token.is_some()
            {
                break;
            }
            target_parameter_count += 1;
        }
        if !parameters.is_empty() && self.parameter_is_this_keyword(parameters[0]) {
            target_parameter_count -= 1;
        }
        Ok(!self.has_effective_rest_parameter(signature)?
            && self.get_parameter_count(signature)? < target_parameter_count)
    }

    /// tsc-port: getContextualSignatureForFunctionLikeDeclaration @6.0.3
    /// tsc-hash: 285e51fbe6399fa87d48553f4a02162ffe635e47bd6a1a60366d98ee5513914f
    /// tsc-span: _tsc.js:73848-73850
    pub(crate) fn get_contextual_signature_for_function_like_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<SignatureId>> {
        if self.is_function_expression_or_arrow_function(node)
            || self.is_object_literal_method(node)
        {
            self.get_contextual_signature(node)
        } else {
            Ok(None)
        }
    }

    /// tsc-port: getContextualSignature @6.0.3
    /// tsc-hash: c1e7196d7ccbc1f0d515ba769eba97076e82b2a9b2470335bff2d50f97a09950
    /// tsc-span: _tsc.js:73851-73891
    ///
    /// getSignatureOfTypeTag is [JSDOC] (no JSDoc parse — the JS
    /// type-tag answer is a recorded FN in JS files only).
    pub(crate) fn get_contextual_signature(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<SignatureId>> {
        debug_assert!(
            self.kind_of(node) != SyntaxKind::MethodDeclaration
                || self.is_object_literal_method(node)
        );
        let Some(ty) = self.get_apparent_type_of_contextual_type(node, ContextFlags::SIGNATURE)?
        else {
            return Ok(None);
        };
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            return self.get_contextual_call_signature(ty, node);
        }
        let types = match &self.tables.type_of(ty).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies payload"),
        };
        let mut signature_list: Vec<SignatureId> = Vec::new();
        for current in types {
            if let Some(signature) = self.get_contextual_call_signature(current, node)? {
                if signature_list.is_empty() {
                    signature_list.push(signature);
                } else if !self.compare_signatures_identical_at(
                    signature_list[0],
                    signature,
                    /*partial_match*/ false,
                    /*ignore_this_types*/ true,
                    /*ignore_return_types*/ true,
                )? {
                    return Ok(None);
                } else {
                    signature_list.push(signature);
                }
            }
        }
        Ok(match signature_list.len() {
            0 => None,
            1 => Some(signature_list[0]),
            _ => {
                let head = signature_list[0];
                Some(self.create_union_signature(head, signature_list))
            }
        })
    }

    // ---- isContextSensitive family (63832-63882 + walkers) ----
    // §12 lists the LIVE consumers (fn-expression trio) at 5.5f; the
    // family itself is a prerequisite of this stage's
    // getContextuallyTypedParameterType / getContextualThisParameterType
    // gates, so it ports here.

    /// tsc-port: isContextSensitive @6.0.3
    /// tsc-hash: 6f0633a32072d0c768c34aeb4ba53dd0f57863aba52fb8bcfbcf88699fe2d228
    /// tsc-span: _tsc.js:63832-63865
    pub(crate) fn is_context_sensitive(&self, node: NodeId) -> bool {
        debug_assert!(
            self.kind_of(node) != SyntaxKind::MethodDeclaration
                || self.is_object_literal_method(node)
        );
        match self.kind_of(node) {
            SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::FunctionDeclaration => {
                self.is_context_sensitive_function_like_declaration_syntactic(node)
            }
            SyntaxKind::ObjectLiteralExpression => match self.data_of(node) {
                NodeData::ObjectLiteralExpression(data) => self
                    .nodes_of(data.properties)
                    .iter()
                    .any(|&p| self.is_context_sensitive(p)),
                _ => false,
            },
            SyntaxKind::ArrayLiteralExpression => match self.data_of(node) {
                NodeData::ArrayLiteralExpression(data) => self
                    .nodes_of(data.elements)
                    .iter()
                    .any(|&e| self.is_context_sensitive(e)),
                _ => false,
            },
            SyntaxKind::ConditionalExpression => match self.data_of(node) {
                NodeData::ConditionalExpression(data) => {
                    data.when_true.is_some_and(|n| self.is_context_sensitive(n))
                        || data
                            .when_false
                            .is_some_and(|n| self.is_context_sensitive(n))
                }
                _ => false,
            },
            SyntaxKind::BinaryExpression => match self.data_of(node) {
                NodeData::BinaryExpression(data) => {
                    let is_or_coalesce = data.operator_token.is_some_and(|op| {
                        matches!(
                            self.kind_of(op),
                            SyntaxKind::BarBarToken | SyntaxKind::QuestionQuestionToken
                        )
                    });
                    is_or_coalesce
                        && (data.left.is_some_and(|n| self.is_context_sensitive(n))
                            || data.right.is_some_and(|n| self.is_context_sensitive(n)))
                }
                _ => false,
            },
            SyntaxKind::PropertyAssignment => match self.data_of(node) {
                NodeData::PropertyAssignment(data) => data
                    .initializer
                    .is_some_and(|n| self.is_context_sensitive(n)),
                _ => false,
            },
            SyntaxKind::ParenthesizedExpression => match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => data
                    .expression
                    .is_some_and(|n| self.is_context_sensitive(n)),
                _ => false,
            },
            SyntaxKind::JsxAttributes | SyntaxKind::JsxAttribute => {
                // [JSX → 5.5f] consumers; the walk itself is
                // syntactic, but JSX fixtures reach it only through
                // 5.5f arms — answering false here is tsc's answer for
                // attribute-less nodes and a recorded FN otherwise.
                false
            }
            SyntaxKind::JsxExpression | SyntaxKind::YieldExpression => {
                let expression = match self.data_of(node) {
                    NodeData::JsxExpression(data) => data.expression,
                    NodeData::YieldExpression(data) => data.expression,
                    _ => None,
                };
                expression.is_some_and(|e| self.is_context_sensitive(e))
            }
            _ => false,
        }
    }

    /// tsc-port: isContextSensitiveFunctionLikeDeclaration @6.0.3
    /// tsc-hash: 68d03bd21ee5d9b3da9872978c939e893d0a8d0747d16eadcb180229056012e4
    /// tsc-span: _tsc.js:63866-63868
    fn is_context_sensitive_function_like_declaration_syntactic(&self, node: NodeId) -> bool {
        self.has_context_sensitive_parameters(node)
            || self.has_context_sensitive_return_expression(node)
            || self.has_context_sensitive_yield_expression(node)
    }

    /// tsc-port: hasContextSensitiveParameters @6.0.3
    /// tsc-hash: 5e4f4b912d59ce18948174f79c715f7462acbc58aad580616e084403324c3e2a
    /// tsc-span: _tsc.js:19182-19196
    pub(crate) fn has_context_sensitive_parameters(&self, node: NodeId) -> bool {
        if self.function_type_parameters_of(node).is_some() {
            return false;
        }
        let parameters = self.parameters_of_function(node);
        if parameters
            .iter()
            .any(|&p| matches!(self.data_of(p), NodeData::Parameter(data) if data.r#type.is_none()))
        {
            return true;
        }
        if self.kind_of(node) != SyntaxKind::ArrowFunction {
            let this_less = parameters
                .first()
                .is_none_or(|&p| !self.parameter_is_this_keyword(p));
            if this_less {
                return self.node_flags(node) & NodeFlags::CONTAINS_THIS.bits() != 0;
            }
        }
        false
    }

    /// tsc-port: hasContextSensitiveReturnExpression @6.0.3
    /// tsc-hash: ebf895c7da74e8dc1ebdb5597e76ab4068ec8b16647e3c3d49a3b94910b13fda
    /// tsc-span: _tsc.js:63869-63877
    fn has_context_sensitive_return_expression(&self, node: NodeId) -> bool {
        if self.function_type_parameters_of(node).is_some() {
            return false;
        }
        let annotation = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            _ => None,
        };
        if annotation.is_some() {
            return false;
        }
        let body = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::ArrowFunction(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            _ => None,
        };
        let Some(body) = body else {
            return false;
        };
        if self.kind_of(body) != SyntaxKind::Block {
            return self.is_context_sensitive(body);
        }
        self.for_each_return_statement(body, &mut |state, statement| {
            let NodeData::ReturnStatement(data) = state.data_of(statement) else {
                return false;
            };
            data.expression
                .is_some_and(|e| state.is_context_sensitive(e))
        })
    }

    /// tsc-port: hasContextSensitiveYieldExpression @6.0.3
    /// tsc-hash: e86b90a13cdba775d429b1509dae01a73b969373a6ea8cbbc13ee5ba82563025
    /// tsc-span: _tsc.js:63878-63880
    fn has_context_sensitive_yield_expression(&self, node: NodeId) -> bool {
        if self.get_function_flags(node) & FUNCTION_FLAGS_GENERATOR == 0 {
            return false;
        }
        let body = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            _ => None,
        };
        let Some(body) = body else {
            return false;
        };
        self.for_each_yield_expression(body, &mut |state, y| state.is_context_sensitive(y))
    }

    /// tsc-port: isContextSensitiveFunctionOrObjectLiteralMethod @6.0.3
    /// tsc-hash: a8672101fe99aa49099d9bdab6c69f7e0d1471b677e7087133e419596786277b
    /// tsc-span: _tsc.js:63881-63883
    pub(crate) fn is_context_sensitive_function_or_object_literal_method(
        &mut self,
        func: NodeId,
    ) -> CheckResult2<bool> {
        Ok((self.is_function_expression_or_arrow_function(func)
            || self.is_object_literal_method(func))
            && self.is_context_sensitive_function_like_declaration_syntactic(func))
    }

    /// tsc-port: forEachReturnStatement @6.0.3
    /// tsc-hash: 7bd4a6c5fcb13a2ab5f46bc529fa21ff82a210bb2e0c753cc845dc96b0e53152
    /// tsc-span: _tsc.js:14275-14299
    ///
    /// Iterative worklist (M2 walker discipline — recursion on fixture
    /// depth overflows). The traversal ORDER differs from tsc's
    /// depth-first only in sibling scheduling; the callers here are
    /// existence tests, so any-order is observationally identical.
    fn for_each_return_statement(
        &self,
        body: NodeId,
        visitor: &mut dyn FnMut(&Self, NodeId) -> bool,
    ) -> bool {
        let mut worklist = vec![body];
        while let Some(node) = worklist.pop() {
            match self.kind_of(node) {
                SyntaxKind::ReturnStatement => {
                    if visitor(self, node) {
                        return true;
                    }
                }
                SyntaxKind::CaseBlock
                | SyntaxKind::Block
                | SyntaxKind::IfStatement
                | SyntaxKind::DoStatement
                | SyntaxKind::WhileStatement
                | SyntaxKind::ForStatement
                | SyntaxKind::ForInStatement
                | SyntaxKind::ForOfStatement
                | SyntaxKind::WithStatement
                | SyntaxKind::SwitchStatement
                | SyntaxKind::CaseClause
                | SyntaxKind::DefaultClause
                | SyntaxKind::LabeledStatement
                | SyntaxKind::TryStatement
                | SyntaxKind::CatchClause => {
                    let source = self.binder.source_of_node(node);
                    tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                        worklist.push(child);
                        false
                    });
                }
                _ => {}
            }
        }
        false
    }

    /// tsc-port: forEachYieldExpression @6.0.3
    /// tsc-hash: 0ca704472cf987e11146946e880d6fb7f769b0d610e4ab36a401bff70ba5f71e
    /// tsc-span: _tsc.js:14300-14328
    ///
    /// Same iterative worklist note as forEachReturnStatement. The
    /// yield arm recurses into the OPERAND after visiting; declaration
    /// kinds stop; function-likes contribute only computed property
    /// names; type nodes stop.
    fn for_each_yield_expression(
        &self,
        body: NodeId,
        visitor: &mut dyn FnMut(&Self, NodeId) -> bool,
    ) -> bool {
        let mut worklist = vec![body];
        while let Some(node) = worklist.pop() {
            match self.kind_of(node) {
                SyntaxKind::YieldExpression => {
                    if visitor(self, node) {
                        return true;
                    }
                    if let NodeData::YieldExpression(data) = self.data_of(node) {
                        if let Some(operand) = data.expression {
                            worklist.push(operand);
                        }
                    }
                }
                SyntaxKind::EnumDeclaration
                | SyntaxKind::InterfaceDeclaration
                | SyntaxKind::ModuleDeclaration
                | SyntaxKind::TypeAliasDeclaration => {}
                _ => {
                    let source = self.binder.source_of_node(node);
                    if node_util::is_function_like_kind(self.kind_of(node)) {
                        if let Some(name) = self.name_of_node(node) {
                            if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                                if let NodeData::ComputedPropertyName(data) = self.data_of(name) {
                                    if let Some(expression) = data.expression {
                                        worklist.push(expression);
                                    }
                                }
                            }
                        }
                    } else if !self.is_part_of_type_node(node) {
                        tsrs2_syntax::for_each_child(
                            &source.arena,
                            source.arena.node(node),
                            |child| {
                                worklist.push(child);
                                false
                            },
                        );
                    }
                }
            }
        }
        false
    }

    // ---- small structural predicates for this band ----

    /// tsc isObjectLiteralMethod (14407-14409).
    pub(crate) fn is_object_literal_method(&self, node: NodeId) -> bool {
        self.kind_of(node) == SyntaxKind::MethodDeclaration
            && self
                .parent_of(node)
                .is_some_and(|p| self.kind_of(p) == SyntaxKind::ObjectLiteralExpression)
    }

    /// tsc isFunctionExpressionOrArrowFunction.
    pub(crate) fn is_function_expression_or_arrow_function(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
        )
    }

    /// tsc parameterIsThisKeyword: parameter whose name is the `this`
    /// identifier.
    fn parameter_is_this_keyword(&self, parameter: NodeId) -> bool {
        matches!(
            self.data_of(parameter),
            NodeData::Parameter(data)
                if data.name.is_some_and(|n| self.identifier_text_of(n) == Some("this"))
        )
    }

    /// The declaration's parameter list (function-like kinds).
    pub(crate) fn parameters_of_function(&self, node: NodeId) -> Vec<NodeId> {
        let parameters = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            _ => None,
        };
        self.nodes_of(parameters)
    }

    /// The declaration's type-parameter list (getEffectiveTypeParameterDeclarations'
    /// TS half — JSDoc templates are [JSDOC]).
    fn function_type_parameters_of(&self, node: NodeId) -> Option<tsrs2_syntax::NodeArrayId> {
        match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            _ => None,
        }
    }

    /// getThisParameter's declaration read: the leading `this`
    /// parameter node if present.
    pub(crate) fn this_parameter_node_of(&self, func: NodeId) -> Option<NodeId> {
        let parameters = self.parameters_of_function(func);
        parameters
            .first()
            .copied()
            .filter(|&p| self.parameter_is_this_keyword(p))
    }

    /// tsc getEffectiveTypeAnnotationNode, TS half: the declaration's
    /// syntactic `.type` ([JSDOC] carries the JS annotations).
    pub(crate) fn effective_type_annotation_node(&self, declaration: NodeId) -> Option<NodeId> {
        // tsc getEffectiveTypeAnnotationNode is a kind-generic `.type`
        // read; the arms below are the declaration kinds that carry
        // one (function-like `.type` is the return annotation, per
        // tsc). Kinds without a type field answer None.
        match self.data_of(declaration) {
            NodeData::VariableDeclaration(data) => data.r#type,
            NodeData::Parameter(data) => data.r#type,
            NodeData::PropertyDeclaration(data) => data.r#type,
            NodeData::PropertySignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::IndexSignature(data) => data.r#type,
            NodeData::FunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            _ => None,
        }
    }

    /// tsc hasInitializer's `.initializer` read for the declaration
    /// kinds this band dispatches on.
    pub(crate) fn initializer_of(&self, declaration: NodeId) -> Option<NodeId> {
        match self.data_of(declaration) {
            NodeData::VariableDeclaration(data) => data.initializer,
            NodeData::Parameter(data) => data.initializer,
            NodeData::BindingElement(data) => data.initializer,
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            NodeData::EnumMember(data) => data.initializer,
            NodeData::JsxAttribute(data) => data.initializer,
            _ => None,
        }
    }

    /// tsc isConstTypeReference on a TYPE node: `const` type reference
    /// with no type arguments.
    pub(crate) fn is_const_type_reference_node(&self, type_node: NodeId) -> bool {
        let NodeData::TypeReference(data) = self.data_of(type_node) else {
            return false;
        };
        data.type_arguments.is_none()
            && data
                .type_name
                .is_some_and(|n| self.identifier_text_of(n) == Some("const"))
    }

    /// tsc-port: getReturnTypeFromAnnotation @6.0.3
    /// tsc-hash: 59a361ad1f8c3e47696f66ca558f78d32f511ea571477db96df219a130d55c5a
    /// tsc-span: _tsc.js:59842-59871
    ///
    /// The JSDoc signature/construct-signature/type-tag arms are
    /// [JSDOC]; the constructor arm returns the class declared type;
    /// the getter arm falls back to the setter's annotated parameter.
    pub(crate) fn get_return_type_from_annotation(
        &mut self,
        declaration: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        if self.kind_of(declaration) == SyntaxKind::Constructor {
            let class = self
                .parent_of(declaration)
                .expect("constructor has a class");
            let symbol = self.get_symbol_of_declaration(class)?;
            let symbol = self.get_merged_symbol(symbol);
            return self
                .get_declared_type_of_class_or_interface(symbol)
                .map(Some);
        }
        let type_node = match self.data_of(declaration) {
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(_) => None,
            _ => None,
        };
        if let Some(type_node) = type_node {
            return Ok(Some(self.get_type_from_type_node(type_node)?));
        }
        if self.kind_of(declaration) == SyntaxKind::GetAccessor {
            let source = self.binder.source_of_node(declaration);
            if !node_util::has_dynamic_name(source, declaration) {
                let symbol = self.get_symbol_of_declaration(declaration)?;
                let setter = self
                    .binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .copied()
                    .find(|&d| self.kind_of(d) == SyntaxKind::SetAccessor);
                if let Some(setter) = setter {
                    let annotated = self
                        .parameters_of_function(setter)
                        .first()
                        .and_then(|&p| self.effective_type_annotation_node(p));
                    if let Some(annotated) = annotated {
                        return Ok(Some(self.get_type_from_type_node(annotated)?));
                    }
                }
            }
        }
        // getReturnTypeOfTypeTag — [JSDOC].
        Ok(None)
    }

    /// tsc-port: isResolvingReturnTypeOfSignature @6.0.3
    /// tsc-hash: f78b4c461afdf8a424ff79b33e53c878d2498a8bf4e5e8006c5c0c25f15da54a
    /// tsc-span: _tsc.js:59872-59874
    fn is_resolving_return_type_of_signature(&self, signature: SignatureId) -> bool {
        let data = self.signature_of(signature);
        if let Some(composite) = &data.composite_signatures {
            if composite
                .iter()
                .any(|&s| self.is_resolving_return_type_of_signature(s))
            {
                return true;
            }
        }
        data.resolved_return_type.resolved().is_none()
            && self
                .find_resolution_cycle_start_index(
                    crate::state::ResolutionTarget::Signature(signature),
                    tsrs2_types::TypeSystemPropertyName::RESOLVED_RETURN_TYPE,
                )
                .is_some()
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_syntax::{NodeId, SyntaxKind};
    use tsrs2_types::{CompilerOptions, ContextFlags};

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// The first node of `kind` whose parent satisfies `parent_kind`
    /// (None = any parent) — direct get_contextual_type probes; the
    /// consuming checkers land in later 5.5 slices.
    fn find_node(
        state: &CheckerState,
        kind: SyntaxKind,
        parent_kind: Option<SyntaxKind>,
    ) -> NodeId {
        let source = state.binder.source(0);
        source
            .arena
            .node_ids()
            .find(|&id| {
                tsrs2_binder::node_util::kind_of(source, id) == kind
                    && parent_kind.is_none_or(|expected| {
                        tsrs2_binder::node_util::parent_of(source, id).is_some_and(|parent| {
                            tsrs2_binder::node_util::kind_of(source, parent) == expected
                        })
                    })
            })
            .expect("fixture contains the probe node")
    }

    #[test]
    fn initializer_takes_the_annotation_as_contextual_type() {
        with_program_state(
            &[("a.ts", "let x: number = 1;\n")],
            &CompilerOptions::default(),
            |state| {
                let initializer = find_node(
                    state,
                    SyntaxKind::NumericLiteral,
                    Some(SyntaxKind::VariableDeclaration),
                );
                let contextual = state
                    .get_contextual_type(initializer, ContextFlags::NONE)
                    .expect("in slice");
                assert_eq!(contextual, Some(state.tables.intrinsics.number));
            },
        );
    }

    #[test]
    fn conditional_operands_inherit_the_outer_context() {
        with_program_state(
            &[(
                "a.ts",
                "declare var b: boolean;\nlet x: number = b ? 1 : 2;\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let when_true = find_node(
                    state,
                    SyntaxKind::NumericLiteral,
                    Some(SyntaxKind::ConditionalExpression),
                );
                let contextual = state
                    .get_contextual_type(when_true, ContextFlags::NONE)
                    .expect("in slice");
                assert_eq!(contextual, Some(state.tables.intrinsics.number));
            },
        );
    }

    #[test]
    fn logical_or_rhs_without_context_takes_the_lhs_type() {
        with_program_state(
            &[("a.ts", "declare var s: string;\ns || \"fallback\";\n")],
            &CompilerOptions::default(),
            |state| {
                let rhs = find_node(
                    state,
                    SyntaxKind::StringLiteral,
                    Some(SyntaxKind::BinaryExpression),
                );
                let contextual = state
                    .get_contextual_type(rhs, ContextFlags::NONE)
                    .expect("in slice");
                assert_eq!(contextual, Some(state.tables.intrinsics.string));
            },
        );
    }

    #[test]
    fn binding_pattern_initializers_get_the_pattern_type() {
        // No annotation: the SkipBindingPatterns gate builds the type
        // FROM the pattern (includePatternInType) — `a = 1` becomes an
        // optional `a: number` member and the pattern link is stamped.
        with_program_state(
            &[("a.ts", "let { a = 1 } = { a: 2 };\n")],
            &CompilerOptions::default(),
            |state| {
                let literal = find_node(
                    state,
                    SyntaxKind::ObjectLiteralExpression,
                    Some(SyntaxKind::VariableDeclaration),
                );
                let contextual = state
                    .get_contextual_type(literal, ContextFlags::NONE)
                    .expect("in slice")
                    .expect("pattern contextual type");
                let pattern = find_node(state, SyntaxKind::ObjectBindingPattern, None);
                assert_eq!(state.links.ty(contextual).pattern, Some(pattern));
                let member = state
                    .get_type_of_property_of_type(contextual, "a")
                    .expect("in slice")
                    .expect("member a");
                // `a = 1` rides addOptionality: number | undefined
                // under the default strictNullChecks.
                let number = state.tables.intrinsics.number;
                let expected = state.tables.add_optionality(
                    number, /*is_property*/ false, /*is_optional*/ true,
                );
                assert_eq!(member, expected);
                // SkipBindingPatterns answers None instead.
                assert_eq!(
                    state
                        .get_contextual_type(literal, ContextFlags::SKIP_BINDING_PATTERNS)
                        .expect("in slice"),
                    None
                );
            },
        );
    }

    #[test]
    fn contextually_typed_parameter_reads_the_annotated_signature() {
        with_program_state(
            &[(
                "a.ts",
                "let f: (x?: string) => void = function (x = \"a\") {};\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let default_value = find_node(
                    state,
                    SyntaxKind::StringLiteral,
                    Some(SyntaxKind::Parameter),
                );
                let contextual = state
                    .get_contextual_type(default_value, ContextFlags::NONE)
                    .expect("in slice");
                // `x?: string` reads back as string | undefined under
                // the default strictNullChecks.
                let string = state.tables.intrinsics.string;
                let expected = state.tables.add_optionality(
                    string, /*is_property*/ false, /*is_optional*/ true,
                );
                assert_eq!(contextual, Some(expected));
            },
        );
    }

    #[test]
    fn object_literal_discrimination_picks_the_matching_constituent() {
        with_program_state(
            &[(
                "a.ts",
                "interface A { kind: \"a\"; x: number }\ninterface B { kind: \"b\"; y: string }\nlet v: A | B = { kind: \"a\", x: 1 };\n",
            )],
            &CompilerOptions::default(),
            |state| {
                let literal = find_node(
                    state,
                    SyntaxKind::ObjectLiteralExpression,
                    Some(SyntaxKind::VariableDeclaration),
                );
                let apparent = state
                    .get_apparent_type_of_contextual_type(literal, ContextFlags::NONE)
                    .expect("in slice")
                    .expect("discriminated type");
                let a_symbol = state
                    .resolve_file_scope_name("A", tsrs2_types::SymbolFlags::INTERFACE)
                    .expect("A resolves");
                let a_declared = state
                    .get_declared_type_of_class_or_interface(a_symbol)
                    .expect("in slice");
                assert_eq!(apparent, a_declared);
            },
        );
    }

    #[test]
    fn fresh_literals_widen_without_a_matching_context() {
        with_program_state(&[("a.ts", "")], &CompilerOptions::default(), |state| {
            let fresh = {
                let regular = state.tables.get_string_literal_type("a");
                state.tables.get_fresh_type_of_literal_type(regular)
            };
            let widened = state
                .get_widened_literal_like_type_for_contextual_type(fresh, None)
                .expect("in slice");
            assert_eq!(widened, state.tables.intrinsics.string);
            let regular = state.tables.get_regular_type_of_literal_type(fresh);
            let kept = state
                .get_widened_literal_like_type_for_contextual_type(fresh, Some(regular))
                .expect("in slice");
            assert_eq!(kept, regular);
        });
    }

    #[test]
    fn const_assertion_operands_are_const_contexts() {
        with_program_state(
            &[("a.ts", "let x = \"a\" as const;\n")],
            &CompilerOptions::default(),
            |state| {
                let operand = find_node(
                    state,
                    SyntaxKind::StringLiteral,
                    Some(SyntaxKind::AsExpression),
                );
                assert!(state.is_const_context(operand).expect("in slice"));
            },
        );
    }
}
