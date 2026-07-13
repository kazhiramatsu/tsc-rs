//! M4 5.5f: the §8 functions/await/yield band — the fn-expression
//! trio (checkFunctionExpressionOrObjectLiteralMethod + the contextual
//! parameter/return assignment + the deferred body pass),
//! getReturnTypeFromBody with its aggregators and Promise wrappers,
//! the await family's error paths (the probe half landed errorNode-less
//! at 5.5e in operators.rs), and the yield grammar slice.
//!
//! Stage seams live where the extraction doc (§0/§8) puts them:
//! [FLOW M5] functionHasImplicitReturn → false;
//! checkAllCodePathsInNonVoidFunctionReturnOrThrow → no-op (FN on
//! 2355/2366/2534/7030); [ITER 5.8] generator bodies and
//! yield aggregation escape Unsupported; [INFER M6] the Inferential
//! checkMode arms are dead (no producer sets the bit at M4).

use tsrs2_binder::node_util;
use tsrs2_binder::SymbolId;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckMode, NodeCheckFlags, NodeFlags, ObjectFlags, SignatureFlags, SymbolFlags, TypeFlags,
    TypeId, UnionReduction,
};

use crate::links::LinkSlot;
use crate::state::{CheckResult2, CheckerState, Unsupported};
use crate::structural::SignatureKind;
use tsrs2_diags::gen as diagnostics;
use tsrs2_diags::DiagnosticMessage;

pub(crate) const FUNCTION_FLAGS_GENERATOR: u32 = 1;
pub(crate) const FUNCTION_FLAGS_ASYNC: u32 = 2;

impl<'a> CheckerState<'a> {
    // ---- the trio ----

    /// tsc-port: checkFunctionExpressionOrObjectLiteralMethod @6.0.3
    /// tsc-hash: a8dbe3f5163f7970481f0e51339e47005c069d0c756dd4b4abc8952313936c7c
    /// tsc-span: _tsc.js:79109-79151
    pub(crate) fn check_function_expression_or_object_literal_method(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        debug_assert!(
            self.kind_of(node) != SyntaxKind::MethodDeclaration
                || self.is_object_literal_method(node)
        );
        self.check_node_deferred(node);
        if self.kind_of(node) == SyntaxKind::FunctionExpression {
            self.check_collisions_for_declaration_name(node);
        }
        if check_mode.intersects(CheckMode::SKIP_CONTEXT_SENSITIVE)
            && self.is_context_sensitive(node)
        {
            if self.effective_return_type_node(node).is_none()
                && !self.has_context_sensitive_parameters(node)
            {
                let contextual_signature = self.get_contextual_signature(node)?;
                if let Some(contextual_signature) = contextual_signature {
                    let contextual_return =
                        self.get_return_type_of_signature(contextual_signature)?;
                    if self.could_contain_type_variables(contextual_return) {
                        if let Some(cached) = self.links.node(node).context_free_type.resolved() {
                            return Ok(cached);
                        }
                        let return_type = self.get_return_type_from_body(node, check_mode)?;
                        let return_only_signature = crate::state::Signature {
                            declaration: None,
                            flags: SignatureFlags::IS_NON_INFERRABLE,
                            type_parameters: None,
                            parameters: Vec::new(),
                            this_parameter: None,
                            min_argument_count: 0,
                            resolved_return_type: LinkSlot::Resolved(return_type),
                            from_method: false,
                            target: None,
                            mapper: None,
                            instantiations: std::collections::HashMap::new(),
                            erased_signature_cache: None,
                            composite_kind: None,
                            composite_signatures: None,
                            optional_call_signature_cache: (None, None),
                        };
                        let signature = self.alloc_signature(return_only_signature);
                        let symbol = self.node_symbol(node);
                        let return_only_type =
                            self.create_single_signature_anonymous_type(symbol, signature);
                        let object_flags = self.tables.object_flags_of(return_only_type)
                            | ObjectFlags::NON_INFERRABLE_TYPE;
                        self.tables.type_mut(return_only_type).object_flags = object_flags;
                        self.links.set_node_context_free_type(
                            self.speculation_depth,
                            node,
                            LinkSlot::Resolved(return_only_type),
                        );
                        return Ok(return_only_type);
                    }
                }
            }
            return Ok(self.any_function_type);
        }
        let has_grammar_error = self.check_grammar_function_like_declaration(node)?;
        if !has_grammar_error && self.kind_of(node) == SyntaxKind::FunctionExpression {
            self.check_grammar_for_generator(node);
        }
        self.contextually_check_function_expression_or_object_literal_method(node, check_mode)?;
        let symbol = self.get_symbol_of_declaration(node)?;
        self.get_type_of_symbol(symbol)
    }

    /// tsc-port: contextuallyCheckFunctionExpressionOrObjectLiteralMethod @6.0.3
    /// tsc-hash: 18461360013004586013aa6b48a5e86af57c46c1f75cd258508ae9ca50e26a39
    /// tsc-span: _tsc.js:79152-79193
    ///
    /// The ContextChecked once-flag DOUBLE-CHECK is transcribed:
    /// getContextualSignature may re-enter this function (through
    /// checkExpression of a discriminant or a contextual force), so
    /// tsc reads the flag once before and once after the signature
    /// query. The `checkMode & Inferential` arms
    /// (inferFromAnnotatedParametersAndReturn + nonFixingMapper
    /// instantiation) are [INFER M6]-dead: no M4 producer sets the
    /// bit, and getInferenceContext answers None, so
    /// instantiatedContextualSignature always resolves to the
    /// contextual signature itself.
    pub(crate) fn contextually_check_function_expression_or_object_literal_method(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<()> {
        if self
            .links
            .node(node)
            .check_flags
            .intersects(NodeCheckFlags::CONTEXT_CHECKED)
        {
            return Ok(());
        }
        let contextual_signature = self.get_contextual_signature(node)?;
        if self
            .links
            .node(node)
            .check_flags
            .intersects(NodeCheckFlags::CONTEXT_CHECKED)
        {
            return Ok(());
        }
        self.links.or_node_check_flags(
            self.speculation_depth,
            node,
            NodeCheckFlags::CONTEXT_CHECKED,
        );
        let symbol = self.get_symbol_of_declaration(node)?;
        let ty = self.get_type_of_symbol(symbol)?;
        let signature = self
            .get_signatures_of_type(ty, SignatureKind::Call)?
            .first()
            .copied();
        let Some(signature) = signature else {
            return Ok(());
        };
        if self.is_context_sensitive(node) {
            match contextual_signature {
                Some(contextual_signature) => {
                    // getInferenceContext → None ([INFER M6]); the
                    // Inferential-mode infer/instantiate arms are dead
                    // and instantiatedContextualSignature is the
                    // contextual signature unchanged.
                    let inference_context = self.get_inference_context(node);
                    if let Some(context) = inference_context {
                        match context {}
                    }
                    self.assign_contextual_parameter_types(signature, contextual_signature)?;
                }
                None => {
                    self.assign_non_contextual_parameter_types(signature)?;
                }
            }
        } else if let Some(contextual_signature) = contextual_signature {
            let has_type_parameters = match self.data_of(node) {
                NodeData::FunctionExpression(data) => data.type_parameters.is_some(),
                NodeData::ArrowFunction(data) => data.type_parameters.is_some(),
                NodeData::MethodDeclaration(data) => data.type_parameters.is_some(),
                _ => false,
            };
            let contextual_parameter_count =
                self.signature_of(contextual_signature).parameters.len();
            let own_parameter_count = self.parameters_of_function(node).len();
            if !has_type_parameters && contextual_parameter_count > own_parameter_count {
                // The body is Inferential-mode-only
                // (inferFromAnnotatedParametersAndReturn) — dead at M4.
            }
        }
        if contextual_signature.is_some()
            && self.get_return_type_from_annotation(node)?.is_none()
            && self
                .signature_of(signature)
                .resolved_return_type
                .resolved()
                .is_none()
        {
            let return_type = self.get_return_type_from_body(node, check_mode)?;
            if self
                .signature_of(signature)
                .resolved_return_type
                .resolved()
                .is_none()
            {
                self.signatures[signature.0 as usize].resolved_return_type =
                    LinkSlot::Resolved(return_type);
            }
        }
        // checkSignatureDeclaration (86971) — 5.8-DECL no-op hook:
        // its grammar walks and 2378/1057-family live with the
        // declaration checkers.
        self.check_signature_declaration_stub(node);
        Ok(())
    }

    /// tsc-port: checkFunctionExpressionOrObjectLiteralMethodDeferred @6.0.3
    /// tsc-hash: 62df6fd08b379a9891dc3019b29fa59ff85ec5b9c203fbdcee9fdb3b1bc695b6
    /// tsc-span: _tsc.js:79194-79213
    ///
    /// Block bodies route through checkSourceElement, whose statement
    /// arms are 5.8 stubs — the deferred pass drives nothing extra for
    /// them until 5.8 (their Unsupported is contained per-element by
    /// check_source_element). Expression bodies run the
    /// checkReturnExpression tail live.
    pub(crate) fn check_function_expression_or_object_literal_method_deferred(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        debug_assert!(
            self.kind_of(node) != SyntaxKind::MethodDeclaration
                || self.is_object_literal_method(node)
        );
        let function_flags = self.get_function_flags(node);
        let return_type = self.get_return_type_from_annotation(node)?;
        self.check_all_code_paths_in_non_void_function_return_or_throw(node, return_type);
        let body = node_util::body_of(self.binder.source_of_node(node), node);
        let Some(body) = body else {
            return Ok(());
        };
        if self.effective_return_type_node(node).is_none() {
            let signature = self.get_signature_from_declaration(node)?;
            self.get_return_type_of_signature(signature)?;
        }
        if self.kind_of(body) == SyntaxKind::Block {
            self.check_source_element(Some(body));
        } else {
            let expr_type = self.check_expression(body, CheckMode::NORMAL)?;
            if let Some(return_type) = return_type {
                let return_or_promised_type =
                    self.unwrap_return_type(return_type, function_flags)?;
                self.check_return_expression(
                    node,
                    return_or_promised_type,
                    body,
                    Some(body),
                    expr_type,
                    false,
                )?;
            }
        }
        Ok(())
    }

    /// checkCollisionsForDeclarationName (83356-83370) — the
    /// require/exports/globalPromise/name-collision walk is emit-facing
    /// bookkeeping plus 5.8-band diagnostics (2441/1212-family via
    /// checkCollision*); no-op hook until 5.8.
    pub(crate) fn check_collisions_for_declaration_name(&mut self, _node: NodeId) {}

    /// checkSignatureDeclaration (86971) — 5.8-DECL no-op hook.
    fn check_signature_declaration_stub(&mut self, _node: NodeId) {}

    /// tsc-port: checkAllCodePathsInNonVoidFunctionReturnOrThrow @6.0.3
    /// tsc-hash: 608b7fb1571a161314fb3aa82e2817f347b15e39244a275b890104c5423e2dc6
    /// tsc-span: _tsc.js:79075-79108
    ///
    /// [FLOW M5] — the whole lazy closure reads
    /// functionHasImplicitReturn/isReachableFlowNode; stubbed out
    /// entirely per the extraction doc §0 (FN on 2355/2366/2534/7030
    /// until M5).
    fn check_all_code_paths_in_non_void_function_return_or_throw(
        &mut self,
        _func: NodeId,
        _return_type: Option<TypeId>,
    ) {
    }

    // ---- contextual parameter assignment ----

    /// tsc-port: assignContextualParameterTypes @6.0.3
    /// tsc-hash: c3c4ff940b94bd7bb8317686f1860e54934cb43c0bce6248b309c0b1cf72b855
    /// tsc-span: _tsc.js:78374-78417
    fn assign_contextual_parameter_types(
        &mut self,
        signature: crate::state::SignatureId,
        context: crate::state::SignatureId,
    ) -> CheckResult2<()> {
        if self.signature_of(context).type_parameters.is_some() {
            if self.signature_of(signature).type_parameters.is_none() {
                let context_type_parameters = self.signature_of(context).type_parameters.clone();
                self.signatures[signature.0 as usize].type_parameters = context_type_parameters;
            } else {
                return Ok(());
            }
        }
        if let Some(context_this) = self.signature_of(context).this_parameter {
            let parameter = self.signature_of(signature).this_parameter;
            let needs_assignment = match parameter {
                None => true,
                Some(parameter) => match self.binder.symbol(parameter).value_declaration {
                    Some(declaration) => self.effective_type_annotation_node(declaration).is_none(),
                    None => false,
                },
            };
            if needs_assignment {
                if parameter.is_none() {
                    let created = self.create_symbol_without_type(context_this);
                    self.signatures[signature.0 as usize].this_parameter = Some(created);
                }
                let this_type = self.get_type_of_symbol(context_this)?;
                let this_parameter = self
                    .signature_of(signature)
                    .this_parameter
                    .expect("assigned above");
                self.assign_parameter_type(this_parameter, Some(this_type))?;
            }
        }
        let has_rest = self
            .signature_of(signature)
            .flags
            .intersects(SignatureFlags::HAS_REST_PARAMETER);
        let len = self.signature_of(signature).parameters.len() - usize::from(has_rest);
        for i in 0..len {
            let parameter = self.signature_of(signature).parameters[i];
            let declaration = self.binder.symbol(parameter).value_declaration;
            let Some(declaration) = declaration else {
                continue;
            };
            if self.effective_type_annotation_node(declaration).is_none() {
                let mut ty = self.try_get_type_at_position(context, i)?;
                if let Some(current) = ty {
                    if self.initializer_of(declaration).is_some() {
                        let initializer_type = self.check_declaration_initializer(
                            declaration,
                            CheckMode::NORMAL,
                            None,
                        )?;
                        if !self.is_type_assignable_to(initializer_type, current)? {
                            let widened = self.widen_type_inferred_from_initializer(
                                declaration,
                                initializer_type,
                            )?;
                            if self.is_type_assignable_to(current, widened)? {
                                ty = Some(widened);
                            }
                        }
                    }
                }
                self.assign_parameter_type(parameter, ty)?;
            }
        }
        if has_rest {
            let parameter = *self
                .signature_of(signature)
                .parameters
                .last()
                .expect("rest flag implies a parameter");
            let needs_assignment = match self.binder.symbol(parameter).value_declaration {
                Some(declaration) => self.effective_type_annotation_node(declaration).is_none(),
                None => self
                    .get_check_flags(parameter)
                    .intersects(tsrs2_types::CheckFlags::DEFERRED_TYPE),
            };
            if needs_assignment {
                let contextual_parameter_type =
                    self.get_rest_type_at_position(context, len, false)?;
                self.assign_parameter_type(parameter, Some(contextual_parameter_type))?;
            }
        }
        Ok(())
    }

    /// tsc-port: assignNonContextualParameterTypes @6.0.3
    /// tsc-hash: 44abc1cf2e72182c295966e0b728eb120f09f047e0c7da327405b665ac388f3b
    /// tsc-span: _tsc.js:78418-78425
    fn assign_non_contextual_parameter_types(
        &mut self,
        signature: crate::state::SignatureId,
    ) -> CheckResult2<()> {
        if let Some(this_parameter) = self.signature_of(signature).this_parameter {
            self.assign_parameter_type(this_parameter, None)?;
        }
        let parameters = self.signature_of(signature).parameters.clone();
        for parameter in parameters {
            self.assign_parameter_type(parameter, None)?;
        }
        Ok(())
    }

    /// tsc-port: assignParameterType @6.0.3
    /// tsc-hash: f9efd414e96c65328f6cd6ad1f9d098787fa7c6244a1ada60544ee99bee060e6
    /// tsc-span: _tsc.js:78426-78450
    ///
    /// The contextual-less fallback is getWidenedTypeForVariableLike-
    /// Declaration(declaration, reportErrors=true) — the full 5.6
    /// chain: annotation, initializer inference + widening, or the
    /// implicit-any tail with its 7006-family report.
    fn assign_parameter_type(
        &mut self,
        parameter: SymbolId,
        contextual_type: Option<TypeId>,
    ) -> CheckResult2<()> {
        if let Some(existing) = self.links.symbol(parameter).type_of_symbol.resolved() {
            if let Some(contextual_type) = contextual_type {
                assert_eq!(
                    existing, contextual_type,
                    "Parameter symbol already has a cached type which differs from newly assigned type"
                );
            }
            return Ok(());
        }
        let declaration = self.binder.symbol(parameter).value_declaration;
        let base = match contextual_type {
            Some(contextual_type) => contextual_type,
            None => match declaration {
                Some(declaration) => self.get_widened_type_for_variable_like_declaration(
                    declaration,
                    /*report_errors*/ true,
                )?,
                None => self.get_type_of_symbol(parameter)?,
            },
        };
        let is_optional = declaration.is_some_and(|declaration| {
            self.initializer_of(declaration).is_none()
                && matches!(
                    self.data_of(declaration),
                    NodeData::Parameter(data) if data.question_token.is_some()
                )
        });
        let mut ty = self.tables.add_optionality(base, false, is_optional);
        self.links
            .set_symbol_type(self.speculation_depth, parameter, LinkSlot::Resolved(ty));
        if let Some(declaration) = declaration {
            let name = match self.data_of(declaration) {
                NodeData::Parameter(data) => data.name,
                NodeData::BindingElement(data) => data.name,
                _ => None,
            };
            if let Some(name) = name {
                if self.kind_of(name) != SyntaxKind::Identifier {
                    if ty == self.tables.intrinsics.unknown {
                        ty = self.get_type_from_binding_pattern(name, false, false)?;
                        self.links.set_symbol_type(
                            self.speculation_depth,
                            parameter,
                            LinkSlot::Resolved(ty),
                        );
                    }
                    self.assign_binding_element_types(name, ty)?;
                }
            }
        }
        Ok(())
    }

    /// tsc-port: assignBindingElementTypes @6.0.3
    /// tsc-hash: af5b07d61441384b942c4e0e5a478d8fdcf25921dff2daae68e0ff34ba6d11a3
    /// tsc-span: _tsc.js:78451-78467
    fn assign_binding_element_types(
        &mut self,
        pattern: NodeId,
        parent_type: TypeId,
    ) -> CheckResult2<()> {
        let elements = match self.data_of(pattern) {
            NodeData::ObjectBindingPattern(data) => data.elements,
            NodeData::ArrayBindingPattern(data) => data.elements,
            _ => None,
        };
        for element in self.nodes_of(elements) {
            if self.kind_of(element) == SyntaxKind::OmittedExpression {
                continue;
            }
            let ty = self.get_binding_element_type_from_parent_type(
                element,
                parent_type,
                /*no_tuple_bounds_check*/ false,
            )?;
            let name = match self.data_of(element) {
                NodeData::BindingElement(data) => data.name,
                _ => None,
            };
            let Some(name) = name else { continue };
            if self.kind_of(name) == SyntaxKind::Identifier {
                let symbol = self.get_symbol_of_declaration(element)?;
                self.links
                    .set_symbol_type(self.speculation_depth, symbol, LinkSlot::Resolved(ty));
            } else {
                self.assign_binding_element_types(name, ty)?;
            }
        }
        Ok(())
    }

    /// createSymbolWithType's no-type face (78368-78373 half): clone
    /// flags/name/READONLY check-flag, leave the links type vacant for
    /// assignParameterType's once-write.
    fn create_symbol_without_type(&mut self, source: SymbolId) -> SymbolId {
        let source_flags = self.symbol_flags(source);
        let name = self.binder.symbol(source).escaped_name.clone();
        let symbol = self.binder.create_symbol(source_flags, name);
        let readonly = tsrs2_types::CheckFlags::from_bits(
            self.get_check_flags(source).bits() & tsrs2_types::CheckFlags::READONLY.bits(),
        );
        self.links
            .set_symbol_check_flags(self.speculation_depth, symbol, readonly);
        symbol
    }

    // ---- getReturnTypeFromBody + aggregators ----

    /// tsc-port: getReturnTypeFromBody @6.0.3
    /// tsc-hash: 3ea543a40c2fea0856b01ae96456c722222cf0a46b633de952569f737279c3f4
    /// tsc-span: _tsc.js:78752-78841
    ///
    /// Generator bodies escape whole ([ITER 5.8]: yield aggregation +
    /// createGeneratorType), so of the widening tail only the
    /// FunctionReturn rows run: reportErrorsFromWidening + the
    /// literal-level contextual widening + the final getWidenedType.
    pub(crate) fn get_return_type_from_body(
        &mut self,
        func: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(func);
        let Some(body) = node_util::body_of(source, func) else {
            return Ok(self.tables.intrinsics.error);
        };
        let function_flags = self.get_function_flags(func);
        let is_async = function_flags & FUNCTION_FLAGS_ASYNC != 0;
        let is_generator = function_flags & FUNCTION_FLAGS_GENERATOR != 0;
        let mut return_type: Option<TypeId>;
        let fallback_return_type = self.tables.intrinsics.void;
        if self.kind_of(body) != SyntaxKind::Block {
            let inner_mode =
                CheckMode::from_bits(check_mode.bits() & !CheckMode::SKIP_GENERIC_FUNCTIONS.bits());
            let mut ty = self.check_expression_cached(body, inner_mode)?;
            if self.is_const_context(body)? {
                ty = self.tables.get_regular_type_of_literal_type(ty);
            }
            if is_async {
                let checked = self.check_awaited_type(
                    ty,
                    /*with_alias*/ false,
                    func,
                    &diagnostics::The_return_type_of_an_async_function_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member,
                )?;
                ty = self.unwrap_awaited_type(checked)?;
            }
            return_type = Some(ty);
        } else if is_generator {
            return Err(Unsupported::new(
                "generator body return inference ([ITER] yield aggregation, 5.8)",
            ));
        } else {
            let types = self.check_and_aggregate_return_expression_types(func, check_mode)?;
            let Some(types) = types else {
                let never = self.tables.intrinsics.never;
                return if is_async {
                    self.create_promise_return_type(func, never)
                } else {
                    Ok(never)
                };
            };
            if types.is_empty() {
                let contextual_return_type =
                    self.get_contextual_return_type(func, tsrs2_types::ContextFlags::NONE)?;
                let undefined_preferred = match contextual_return_type {
                    Some(contextual) => {
                        let unwrapped = self.unwrap_return_type(contextual, function_flags)?;
                        self.some_type(unwrapped, |state, t| {
                            state.tables.flags_of(t).intersects(TypeFlags::UNDEFINED)
                        })
                    }
                    None => false,
                };
                let return_type = if undefined_preferred {
                    self.tables.intrinsics.undefined
                } else {
                    self.tables.intrinsics.void
                };
                return if is_async {
                    self.create_promise_return_type(func, return_type)
                } else {
                    Ok(return_type)
                };
            }
            return_type = Some(self.get_union_type_ex(&types, UnionReduction::Subtype)?);
        }
        if let Some(current) = return_type {
            // reportErrorsFromWidening (78807-78810): generator bodies
            // escape whole above, so only the FunctionReturn row runs.
            self.report_errors_from_widening(
                func,
                current,
                Some(tsrs2_types::WideningKind::FUNCTION_RETURN),
            )?;
            if self.is_unit_type(current) {
                let contextual_signature =
                    self.get_contextual_signature_for_function_like_declaration(func)?;
                let contextual_type = match contextual_signature {
                    None => None,
                    Some(contextual_signature) => {
                        let own = self.get_signature_from_declaration(func)?;
                        if contextual_signature == own {
                            if is_generator {
                                None
                            } else {
                                Some(current)
                            }
                        } else {
                            let signature_return =
                                self.get_return_type_of_signature(contextual_signature)?;
                            self.instantiate_contextual_type(Some(signature_return), func)?
                        }
                    }
                };
                return_type = self
                    .get_widened_literal_like_type_for_contextual_return_type_if_needed(
                        Some(current),
                        contextual_type,
                        is_async,
                    )?;
            }
            // Final getWidenedType (78827-78829).
            if let Some(current) = return_type {
                return_type = Some(self.get_widened_type(current)?);
            }
        }
        let final_return = return_type.unwrap_or(fallback_return_type);
        if is_generator {
            unreachable!("generator arm escaped above");
        }
        if is_async {
            self.create_promise_type(final_return)
        } else {
            Ok(final_return)
        }
    }

    /// tsc-port: functionHasImplicitReturn @6.0.3
    /// tsc-hash: 82639fc96cdd05a5d0f6cec8552ebe828c87898536a91ec4ca88e3d2f606eec1
    /// tsc-span: _tsc.js:78956-78958
    ///
    /// [FLOW M5] stub: endFlowNode reachability answers false until
    /// the flow graph lands — implicit-return undefined unions are FN
    /// (divergent fn types stay contained behind the T2 signature
    /// display).
    fn function_has_implicit_return_stub(&self, _func: NodeId) -> bool {
        false
    }

    /// tsc-port: checkAndAggregateReturnExpressionTypes @6.0.3
    /// tsc-hash: 69b5d219762f77c14a66f98a7981ba6bfa0ee5411becccc95ddf59efef3e609e
    /// tsc-span: _tsc.js:78959-79008
    fn check_and_aggregate_return_expression_types(
        &mut self,
        func: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<Option<Vec<TypeId>>> {
        let function_flags = self.get_function_flags(func);
        let mut aggregated_types: Vec<TypeId> = Vec::new();
        let mut has_return_with_no_expression = self.function_has_implicit_return_stub(func);
        let mut has_return_of_type_never = false;
        let source = self.binder.source_of_node(func);
        let body = node_util::body_of(source, func).expect("callers checked the body");
        let return_statements = self.collect_return_statements(body);
        for return_statement in return_statements {
            let expression = match self.data_of(return_statement) {
                NodeData::ReturnStatement(data) => data.expression,
                _ => None,
            };
            let Some(mut expr) = expression else {
                has_return_with_no_expression = true;
                continue;
            };
            expr = node_util::skip_parentheses_pub(self.binder.source_of_node(expr), expr);
            if function_flags & FUNCTION_FLAGS_ASYNC != 0
                && self.kind_of(expr) == SyntaxKind::AwaitExpression
            {
                if let NodeData::AwaitExpression(data) = self.data_of(expr) {
                    if let Some(operand) = data.expression {
                        expr = node_util::skip_parentheses_pub(
                            self.binder.source_of_node(operand),
                            operand,
                        );
                    }
                }
            }
            // The self-recursive `return f()` skip (78979): the callee
            // resolves quietly; isConstantReference's Identifier arm
            // needs only the resolved symbol + declaration shape.
            if self.is_self_recursive_call_return(expr, func)? {
                has_return_of_type_never = true;
                continue;
            }
            let inner_mode =
                CheckMode::from_bits(check_mode.bits() & !CheckMode::SKIP_GENERIC_FUNCTIONS.bits());
            let mut ty = self.check_expression_cached(expr, inner_mode)?;
            if function_flags & FUNCTION_FLAGS_ASYNC != 0 {
                let checked = self.check_awaited_type(
                    ty,
                    /*with_alias*/ false,
                    func,
                    &diagnostics::The_return_type_of_an_async_function_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member,
                )?;
                ty = self.unwrap_awaited_type(checked)?;
            }
            if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
                has_return_of_type_never = true;
            }
            let pushed = if self.is_const_context(expr)? {
                self.tables.get_regular_type_of_literal_type(ty)
            } else {
                ty
            };
            if !aggregated_types.contains(&pushed) {
                aggregated_types.push(pushed);
            }
        }
        if aggregated_types.is_empty()
            && !has_return_with_no_expression
            && (has_return_of_type_never
                || Self::may_return_never(
                    self.kind_of(func),
                    self.parent_of(func).map(|parent| self.kind_of(parent)),
                ))
        {
            return Ok(None);
        }
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        if strict_null_checks && !aggregated_types.is_empty() && has_return_with_no_expression {
            // isJSConstructor is JS-only — the guard reduces to the
            // undefined push.
            let undefined = self.tables.intrinsics.undefined;
            if !aggregated_types.contains(&undefined) {
                aggregated_types.push(undefined);
            }
        }
        Ok(Some(aggregated_types))
    }

    /// tsc-port: mayReturnNever @6.0.3
    /// tsc-hash: 4fc9783ea8faf0fad211da1a03cf192825fefe1281becd1758b6d016e17f86ed
    /// tsc-span: _tsc.js:79009-79019
    fn may_return_never(kind: SyntaxKind, parent_kind: Option<SyntaxKind>) -> bool {
        match kind {
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction => true,
            SyntaxKind::MethodDeclaration => {
                parent_kind == Some(SyntaxKind::ObjectLiteralExpression)
            }
            _ => false,
        }
    }

    /// forEachReturnStatement (14275-14295) specialized to a collector:
    /// visits ReturnStatements without descending into nested
    /// function-likes (the walker recurses only through statement
    /// constructs).
    fn collect_return_statements(&self, body: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut worklist = vec![body];
        while let Some(node) = worklist.pop() {
            match self.kind_of(node) {
                SyntaxKind::ReturnStatement => out.push(node),
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
                    let mut children = Vec::new();
                    tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                        children.push(child);
                        false
                    });
                    // LIFO worklist: reversed push keeps source order.
                    for &child in children.iter().rev() {
                        worklist.push(child);
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The `return f()` self-recursion probe (78979): CallExpression
    /// over a plain identifier callee whose cached type's symbol is
    /// the function's own merged symbol, unless the function is a
    /// fn-expression/arrow whose reference is non-constant.
    fn is_self_recursive_call_return(&mut self, expr: NodeId, func: NodeId) -> CheckResult2<bool> {
        if self.kind_of(expr) != SyntaxKind::CallExpression {
            return Ok(false);
        }
        let callee = match self.data_of(expr) {
            NodeData::CallExpression(data) => data.expression,
            _ => None,
        };
        let Some(callee) = callee else {
            return Ok(false);
        };
        if self.kind_of(callee) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let callee_type = self.check_expression_cached(callee, CheckMode::NORMAL)?;
        let callee_symbol = self.tables.type_of(callee_type).symbol;
        let func_symbol = self.node_symbol(func);
        let Some(func_symbol) = func_symbol else {
            return Ok(false);
        };
        let func_symbol = self.get_merged_symbol(func_symbol);
        if callee_symbol != Some(func_symbol) {
            return Ok(false);
        }
        let value_declaration = self.binder.symbol(func_symbol).value_declaration;
        let is_fn_expression = value_declaration
            .is_some_and(|declaration| self.is_function_expression_or_arrow_function(declaration));
        if !is_fn_expression {
            return Ok(true);
        }
        self.is_constant_reference(callee)
    }

    /// tsc-port: isConstantReference @6.0.3
    /// tsc-hash: 63298ed7776bb8e07259b2d8bb0051c1ce8c9e2f1e4be5517b1c15c6eca65e81
    /// tsc-span: _tsc.js:70374-70393
    ///
    /// The Identifier arm's isSymbolAssigned read is [FLOW M5]-adjacent
    /// (post-assignment analysis); the 5.5a definite-assignment stub
    /// (false = never assigned) keeps parameters/mutable locals
    /// constant, matching tsc whenever the symbol is in fact never
    /// written between declaration and use. The binding-pattern arm's
    /// consumers are destructured declarations (5.6/5.8) — escape.
    fn is_constant_reference(&mut self, node: NodeId) -> CheckResult2<bool> {
        match self.kind_of(node) {
            SyntaxKind::ThisKeyword => Ok(true),
            SyntaxKind::Identifier => {
                let Some(symbol) = self.get_resolved_symbol(node) else {
                    return Ok(false);
                };
                if self.is_constant_variable(symbol) {
                    return Ok(true);
                }
                if self.is_parameter_or_mutable_local_variable(symbol)
                    && !self.is_symbol_assigned_definitely_stub(symbol)
                {
                    return Ok(true);
                }
                Ok(self
                    .binder
                    .symbol(symbol)
                    .value_declaration
                    .is_some_and(|declaration| {
                        self.kind_of(declaration) == SyntaxKind::FunctionExpression
                    }))
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression => {
                let expression = match self.data_of(node) {
                    NodeData::PropertyAccessExpression(data) => data.expression,
                    NodeData::ElementAccessExpression(data) => data.expression,
                    _ => None,
                };
                let Some(expression) = expression else {
                    return Ok(false);
                };
                if !self.is_constant_reference(expression)? {
                    return Ok(false);
                }
                let resolved = self
                    .links
                    .node(node)
                    .resolved_symbol
                    .resolved()
                    .unwrap_or(self.unknown_symbol);
                Ok(self.is_readonly_symbol(resolved))
            }
            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern => {
                Err(Unsupported::new(
                    "isConstantReference binding-pattern arm (destructured declarations, 5.6/5.8)",
                ))
            }
            _ => Ok(false),
        }
    }

    // ---- Promise wrappers ----

    /// tsc-port: createPromiseType @6.0.3
    /// tsc-hash: 62d5888e1a605f7c047eac534909be55e9ca148580d6d49efa7f4fa25b2d73bc
    /// tsc-span: _tsc.js:78702-78712
    pub(crate) fn create_promise_type(&mut self, promised_type: TypeId) -> CheckResult2<TypeId> {
        let global_promise = self.get_global_promise_type(/*report_errors*/ true)?;
        let Some(global_promise) = global_promise else {
            return Ok(self.tables.intrinsics.unknown);
        };
        let unwrapped = self.unwrap_awaited_type(promised_type)?;
        let awaited = self
            .get_awaited_type_no_alias(unwrapped, None)?
            .unwrap_or(self.tables.intrinsics.unknown);
        Ok(self
            .tables
            .create_type_reference(global_promise, &[awaited]))
    }

    /// tsc-port: createPromiseReturnType @6.0.3
    /// tsc-hash: 8deae93ccfc7675a8ab2ff4fbfba4c0490efc173ad3b4fc8a82b609a35d1ed4b
    /// tsc-span: _tsc.js:78724-78742
    ///
    /// The isImportCall diagnostics selections ride along (2711/2712);
    /// import-call callers land at 5.7 but the selection costs nothing
    /// and keeps the transcription exact.
    pub(crate) fn create_promise_return_type(
        &mut self,
        func: NodeId,
        promised_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let promise_type = self.create_promise_type(promised_type)?;
        if promise_type == self.tables.intrinsics.unknown {
            let is_import_call = self.is_import_call(func);
            self.error_at(
                Some(func),
                if is_import_call {
                    &diagnostics::A_dynamic_import_call_returns_a_Promise_Make_sure_you_have_a_declaration_for_Promise_or_include_ES2015_in_your_lib_option
                } else {
                    &diagnostics::An_async_function_or_method_must_return_a_Promise_Make_sure_you_have_a_declaration_for_Promise_or_include_ES2015_in_your_lib_option
                },
                &[],
            );
            return Ok(self.tables.intrinsics.error);
        }
        if self
            .get_global_promise_constructor_symbol(/*report_errors*/ true)?
            .is_none()
        {
            let is_import_call = self.is_import_call(func);
            self.error_at(
                Some(func),
                if is_import_call {
                    &diagnostics::A_dynamic_import_call_in_ES5_requires_the_Promise_constructor_Make_sure_you_have_a_declaration_for_the_Promise_constructor_or_include_ES2015_in_your_lib_option
                } else {
                    &diagnostics::An_async_function_or_method_in_ES5_requires_the_Promise_constructor_Make_sure_you_have_a_declaration_for_the_Promise_constructor_or_include_ES2015_in_your_lib_option
                },
                &[],
            );
        }
        Ok(promise_type)
    }

    // ---- return-expression checking (deferred tail) ----

    /// tsc-port: unwrapReturnType @6.0.3
    /// tsc-hash: 7a90be08264ca0cb4c797f22e1cad0fb1af1c7ce9586adc56ec473229938554c
    /// tsc-span: _tsc.js:84500-84511
    ///
    /// The generator arm reads getIterationTypeOfGeneratorFunction-
    /// ReturnType — [ITER 5.8] escape.
    pub(crate) fn unwrap_return_type(
        &mut self,
        return_type: TypeId,
        function_flags: u32,
    ) -> CheckResult2<TypeId> {
        let is_generator = function_flags & FUNCTION_FLAGS_GENERATOR != 0;
        let is_async = function_flags & FUNCTION_FLAGS_ASYNC != 0;
        if is_generator {
            return Err(Unsupported::new(
                "unwrapReturnType generator arm ([ITER] iteration types, 5.8)",
            ));
        }
        if is_async {
            let awaited = self.get_awaited_type_no_alias(return_type, None)?;
            return Ok(awaited.unwrap_or(self.tables.intrinsics.error));
        }
        Ok(return_type)
    }

    /// tsc-port: checkReturnExpression @6.0.3
    /// tsc-hash: 91aacbdf8bc774dfdf833a4737c749c29ebdda40f369569b5b26d9e555172a2d
    /// tsc-span: _tsc.js:84550-84588
    pub(crate) fn check_return_expression(
        &mut self,
        container: NodeId,
        unwrapped_return_type: TypeId,
        node: NodeId,
        expr: Option<NodeId>,
        expr_type: TypeId,
        in_conditional_expression: bool,
    ) -> CheckResult2<()> {
        let function_flags = self.get_function_flags(container);
        if let Some(expr) = expr {
            let unwrapped_expr =
                node_util::skip_parentheses_pub(self.binder.source_of_node(expr), expr);
            if self.kind_of(unwrapped_expr) == SyntaxKind::ConditionalExpression {
                let (when_true, when_false) = match self.data_of(unwrapped_expr) {
                    NodeData::ConditionalExpression(data) => (data.when_true, data.when_false),
                    _ => (None, None),
                };
                if let (Some(when_true), Some(when_false)) = (when_true, when_false) {
                    let true_type = self.check_expression(when_true, CheckMode::NORMAL)?;
                    self.check_return_expression(
                        container,
                        unwrapped_return_type,
                        node,
                        Some(when_true),
                        true_type,
                        true,
                    )?;
                    let false_type = self.check_expression(when_false, CheckMode::NORMAL)?;
                    self.check_return_expression(
                        container,
                        unwrapped_return_type,
                        node,
                        Some(when_false),
                        false_type,
                        true,
                    )?;
                    return Ok(());
                }
            }
        }
        let in_return_statement = self.kind_of(node) == SyntaxKind::ReturnStatement;
        let unwrapped_expr_type = if function_flags & FUNCTION_FLAGS_ASYNC != 0 {
            self.check_awaited_type(
                expr_type,
                /*with_alias*/ false,
                node,
                &diagnostics::The_return_type_of_an_async_function_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member,
            )?
        } else {
            expr_type
        };
        let effective_expr = expr.map(|expr| self.get_effective_check_node(expr));
        let error_node = if in_return_statement && !in_conditional_expression {
            Some(node)
        } else {
            effective_expr
        };
        // checkTypeAssignableToAndOptionallyElaborate — the 5.4
        // head-only slice; `effective_expr` feeds only the elided
        // elaboration tail.
        self.check_type_assignable_to(
            unwrapped_expr_type,
            unwrapped_return_type,
            error_node,
            &diagnostics::Type_0_is_not_assignable_to_type_1,
        )?;
        Ok(())
    }

    // ---- await family (error paths; probes landed at 5.5e) ----

    /// tsc-port: checkAwaitExpression @6.0.3
    /// tsc-hash: dffa747ca459ea1e3cdedd8c594aeea8e4af018c55772af87c27b1144d42732f
    /// tsc-span: _tsc.js:79408-79426
    ///
    /// addLazyDiagnostic = eager (5.4 decision) — the grammar closure
    /// runs inline. The no-op-await 80007 tail is suggestion-category
    /// (unmodeled band) — skipped.
    pub(crate) fn check_await_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_await_grammar(node)?;
        let expression = match self.data_of(node) {
            NodeData::AwaitExpression(data) => data.expression,
            _ => None,
        };
        let expression =
            expression.ok_or_else(|| Unsupported::new("await without operand (parse recovery)"))?;
        let operand_type = self.check_expression(expression, CheckMode::NORMAL)?;
        let awaited_type = self.check_awaited_type(
            operand_type,
            /*with_alias*/ true,
            node,
            &diagnostics::Type_of_await_operand_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member,
        )?;
        Ok(awaited_type)
    }

    /// tsc-port: checkAwaitGrammar @6.0.3
    /// tsc-hash: 3f453fc0953b59e17f03bbf0551b5beaa858c2f36b5ad9cc9e808cdeaed030d5
    /// tsc-span: _tsc.js:79338-79407
    ///
    /// ES2025 target + ESNext-family module default: the moduleKind
    /// ladder's Node16..NodeNext arms need impliedNodeFormat (module
    /// resolution, 5.8) — under the conformance option mapping the
    /// reachable rows are the non-module 1375/2853 pair and the
    /// nested 1308/2852 (+related 1356) pair; the target/module 1378/
    /// 2854 arms are dead at ES2017+, transcription kept for the
    /// ladder's fallthrough shape. `await using` declarations reach
    /// the checker at 5.8 — the isAwaitExpression selections are all
    /// true here.
    pub(crate) fn check_await_grammar(&mut self, node: NodeId) -> CheckResult2<bool> {
        let mut has_error = false;
        let container = {
            // getContainingFunctionOrClassStaticBlock (14612).
            let mut current = self.parent_of(node);
            loop {
                match current {
                    Some(n)
                        if node_util::is_function_like_kind(self.kind_of(n))
                            || self.kind_of(n) == SyntaxKind::ClassStaticBlockDeclaration =>
                    {
                        break Some(n)
                    }
                    Some(n) => current = self.parent_of(n),
                    None => break None,
                }
            }
        };
        if container.is_some_and(|container| {
            self.kind_of(container) == SyntaxKind::ClassStaticBlockDeclaration
        }) {
            self.error_at(
                Some(node),
                &diagnostics::await_expression_cannot_be_used_inside_a_class_static_block,
                &[],
            );
            has_error = true;
        } else if !NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AWAIT_CONTEXT)
        {
            if node_util::is_in_top_level_context(self.binder.source_of_node(node), node) {
                if !self.has_parse_diagnostics(node) {
                    let span = self.span_of_token_at_node_pos(node);
                    if !self.is_effective_external_module(node) {
                        self.error_at_span(
                            span,
                            node,
                            &diagnostics::await_expressions_are_only_allowed_at_the_top_level_of_a_file_when_that_file_is_a_module_but_this_file_has_no_imports_or_exports_Consider_adding_an_empty_export_to_make_this_file_a_module,
                            &[],
                        );
                        has_error = true;
                    }
                    // moduleKind ladder: ESNext-family + ES2025 target
                    // → break (no 1378/2854); Node16..NodeNext need
                    // impliedNodeFormat (5.8) — the conformance
                    // mapping never selects them at this stage.
                }
            } else if !self.has_parse_diagnostics(node) {
                let span = self.span_of_token_at_node_pos(node);
                let related = container
                    .filter(|&container| {
                        self.kind_of(container) != SyntaxKind::Constructor
                            && self.get_function_flags(container) & FUNCTION_FLAGS_ASYNC == 0
                    })
                    .map(|container| {
                        self.related_info_for_node(
                            container,
                            &diagnostics::Did_you_mean_to_mark_this_function_as_async,
                            &[],
                        )
                    })
                    .into_iter()
                    .collect();
                self.error_at_span_with_related(
                    span,
                    node,
                    &diagnostics::await_expressions_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules,
                    &[],
                    related,
                );
                has_error = true;
            }
        }
        if self.is_in_parameter_initializer_before_containing_function(node) {
            self.error_at(
                Some(node),
                &diagnostics::await_expressions_cannot_be_used_in_a_parameter_initializer,
                &[],
            );
            has_error = true;
        }
        Ok(has_error)
    }

    // ---- yield (grammar + non-generator arm; [ITER] escape) ----

    /// tsc-port: checkYieldExpression @6.0.3
    /// tsc-hash: 0b3a8949d463bc687dfc4e9cfdba344821a310459bbf326e88bcbf681ad78c13
    /// tsc-span: _tsc.js:80447-80512
    ///
    /// 5.5 slice per §8: grammar closure (eager) + the non-generator
    /// anyType arm. Generator containers need the iteration-types
    /// protocol ([ITER 5.8]) — escape; per-element containment keeps
    /// the enclosing file checked.
    pub(crate) fn check_yield_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_yield_expression_grammar(node);
        let func = self.get_containing_function(node);
        let Some(func) = func else {
            return Ok(self.tables.intrinsics.any);
        };
        let function_flags = self.get_function_flags(func);
        if function_flags & FUNCTION_FLAGS_GENERATOR == 0 {
            return Ok(self.tables.intrinsics.any);
        }
        Err(Unsupported::new(
            "checkYieldExpression generator arm ([ITER] iteration types, 5.8)",
        ))
    }

    /// checkYieldExpressionGrammar (80505-80511, the lazy closure —
    /// eager here per the 5.4 addLazyDiagnostic decision).
    fn check_yield_expression_grammar(&mut self, node: NodeId) {
        if !NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::YIELD_CONTEXT) {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::A_yield_expression_is_only_allowed_in_a_generator_body,
                &[],
            );
        }
        if self.is_in_parameter_initializer_before_containing_function(node) {
            self.error_at(
                Some(node),
                &diagnostics::yield_expressions_cannot_be_used_in_a_parameter_initializer,
                &[],
            );
        }
    }

    // ---- grammar (fn-like declaration band) ----

    /// tsc-port: checkGrammarFunctionLikeDeclaration @6.0.3
    /// tsc-hash: c9f0c6623f0bb6fdff9dac448b23075eff0bf20954d1f4acd38520ea0a84081f
    /// tsc-span: _tsc.js:89466-89469
    ///
    /// checkGrammarModifiers rides the existing M7-stub hook (fn
    /// expressions admit only `async`; other modifiers are parse
    /// errors, so the gate cannot misfire). The arrow-function 1200
    /// walk and the use-strict 1347 walk port live below.
    pub(crate) fn check_grammar_function_like_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<bool> {
        self.check_grammar_modifiers(node);
        let type_parameters = match self.data_of(node) {
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::GetAccessor(data) => data.type_parameters,
            NodeData::SetAccessor(data) => data.type_parameters,
            _ => None,
        };
        if self.check_grammar_type_parameter_list(node, type_parameters) {
            return Ok(true);
        }
        if self.check_grammar_parameter_list(node)? {
            return Ok(true);
        }
        if self.check_grammar_arrow_function(node) {
            return Ok(true);
        }
        if node_util::is_function_like_declaration_kind(self.kind_of(node))
            && self.check_grammar_for_use_strict_simple_parameter_list(node)
        {
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: checkGrammarForGenerator @6.0.3
    /// tsc-hash: 4ce9ce97b68d631a6ce5a066e0849566b1b7004c16330d9cee00b5531d6fbd65
    /// tsc-span: _tsc.js:89618-89630
    pub(crate) fn check_grammar_for_generator(&mut self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let Some(asterisk) = node_util::asterisk_token_of(source, node) else {
            return false;
        };
        debug_assert!(matches!(
            self.kind_of(node),
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::MethodDeclaration
        ));
        if NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT) {
            return self.grammar_error_on_node(
                asterisk,
                &diagnostics::Generators_are_not_allowed_in_an_ambient_context,
                &[],
            );
        }
        if node_util::body_of(source, node).is_none() {
            return self.grammar_error_on_node(
                asterisk,
                &diagnostics::An_overload_signature_cannot_be_declared_as_a_generator,
                &[],
            );
        }
        false
    }

    /// tsc-port: checkGrammarTypeParameterList @6.0.3
    /// tsc-hash: b38e4c29614faf900e080dc6c95f69705bbeab6d9fba40536784f9a966c176e3
    /// tsc-span: _tsc.js:89407-89414
    fn check_grammar_type_parameter_list(
        &mut self,
        node: NodeId,
        type_parameters: Option<tsrs2_syntax::NodeArrayId>,
    ) -> bool {
        let Some(array_id) = type_parameters else {
            return false;
        };
        let source = self.binder.source_of_node(node);
        let array = source.arena.node_array(array_id);
        if !array.nodes.is_empty() {
            return false;
        }
        if !source.parse_diagnostics.is_empty() {
            return false;
        }
        let start_byte = array.pos as usize - "<".len();
        let end_byte = tsrs2_syntax::skip_trivia(&source.text, array.end as usize) + ">".len();
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start = to_utf16(start_byte);
        let length = to_utf16(end_byte) - start;
        self.grammar_error_at_pos(
            node,
            start,
            length,
            &diagnostics::Type_parameter_list_cannot_be_empty,
            &[],
        )
    }

    /// tsc-port: checkGrammarParameterList @6.0.3
    /// tsc-hash: 09bfdcd3a387a1c410d86c288e0a97aa1ef3fc7241a296c0a0cdcfb9f11d0c99
    /// tsc-span: _tsc.js:89415-89442
    fn check_grammar_parameter_list(&mut self, node: NodeId) -> CheckResult2<bool> {
        let parameters = self.parameters_of_function(node);
        let mut seen_optional_parameter = false;
        let parameter_count = parameters.len();
        for (i, &parameter) in parameters.iter().enumerate() {
            let NodeData::Parameter(data) = self.data_of(parameter).clone() else {
                continue;
            };
            if let Some(dot_dot_dot) = data.dot_dot_dot_token {
                if i != parameter_count - 1 {
                    return Ok(self.grammar_error_on_node(
                        dot_dot_dot,
                        &diagnostics::A_rest_parameter_must_be_last_in_a_parameter_list,
                        &[],
                    ));
                }
                if !NodeFlags::from_bits(self.node_flags(parameter)).intersects(NodeFlags::AMBIENT)
                {
                    let array = self.parameter_array_of_function(node);
                    if let Some(array) = array {
                        self.check_grammar_for_disallowed_trailing_comma(
                            Some(array),
                            &diagnostics::A_rest_parameter_or_binding_pattern_may_not_have_a_trailing_comma,
                        );
                    }
                }
                if let Some(question) = data.question_token {
                    return Ok(self.grammar_error_on_node(
                        question,
                        &diagnostics::A_rest_parameter_cannot_be_optional,
                        &[],
                    ));
                }
                if data.initializer.is_some() {
                    let name = data.name.unwrap_or(parameter);
                    return Ok(self.grammar_error_on_node(
                        name,
                        &diagnostics::A_rest_parameter_cannot_have_an_initializer,
                        &[],
                    ));
                }
            } else if data.question_token.is_some() {
                seen_optional_parameter = true;
                if data.initializer.is_some() {
                    let name = data.name.unwrap_or(parameter);
                    return Ok(self.grammar_error_on_node(
                        name,
                        &diagnostics::Parameter_cannot_have_question_mark_and_initializer,
                        &[],
                    ));
                }
            } else if seen_optional_parameter && data.initializer.is_none() {
                let name = data.name.unwrap_or(parameter);
                return Ok(self.grammar_error_on_node(
                    name,
                    &diagnostics::A_required_parameter_cannot_follow_an_optional_parameter,
                    &[],
                ));
            }
        }
        Ok(false)
    }

    /// tsc-port: checkGrammarArrowFunction @6.0.3
    /// tsc-hash: 8f7f56e65b9b90db72cf6b6e3c6f9c82bfb1d7b237ba81842c439fed4c211365
    /// tsc-span: _tsc.js:89474-89488
    ///
    /// The mts/cts 1219-note arm needs fileExtensionIsOneOf on module
    /// extensions — dead in the .ts conformance surface, elided with
    /// this note.
    fn check_grammar_arrow_function(&mut self, node: NodeId) -> bool {
        if self.kind_of(node) != SyntaxKind::ArrowFunction {
            return false;
        }
        let NodeData::ArrowFunction(data) = self.data_of(node).clone() else {
            return false;
        };
        let Some(arrow) = data.equals_greater_than_token else {
            return false;
        };
        let source = self.binder.source_of_node(node);
        let arrow_record = source.arena.node(arrow);
        let line_of = |byte: u32| -> usize {
            match source.line_map.line_starts.binary_search(&byte) {
                Ok(line) => line,
                Err(insertion) => insertion - 1,
            }
        };
        let start_line = line_of(arrow_record.pos);
        let end_line = line_of(arrow_record.end);
        if start_line != end_line {
            return self.grammar_error_on_node(
                arrow,
                &diagnostics::Line_terminator_not_permitted_before_arrow,
                &[],
            );
        }
        false
    }

    /// tsc-port: checkGrammarForUseStrictSimpleParameterList @6.0.3
    /// tsc-hash: 58ed76f80b7673e04ac7687637d47c898b542816212961bbc35f9c375b620d27
    /// tsc-span: _tsc.js:89446-89465
    ///
    /// languageVersion is ES2025 — the ES2016 gate is always open.
    fn check_grammar_for_use_strict_simple_parameter_list(&mut self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let body = node_util::body_of(source, node);
        let Some(body) = body else {
            return false;
        };
        if self.kind_of(body) != SyntaxKind::Block {
            return false;
        }
        let Some(use_strict) = self.find_use_strict_prologue(body) else {
            return false;
        };
        let parameters = self.parameters_of_function(node);
        let non_simple: Vec<NodeId> = parameters
            .into_iter()
            .filter(|&parameter| match self.data_of(parameter) {
                NodeData::Parameter(data) => {
                    data.initializer.is_some()
                        || data.dot_dot_dot_token.is_some()
                        || data.name.is_some_and(|name| {
                            matches!(
                                self.kind_of(name),
                                SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                            )
                        })
                }
                _ => false,
            })
            .collect();
        if non_simple.is_empty() {
            return false;
        }
        for &parameter in &non_simple {
            let related = vec![self.related_info_for_node(
                use_strict,
                &diagnostics::use_strict_directive_used_here,
                &[],
            )];
            self.error_at_with_related(
                Some(parameter),
                &diagnostics::This_parameter_is_not_allowed_with_use_strict_directive,
                &[],
                related,
            );
        }
        let related: Vec<_> = non_simple
            .iter()
            .enumerate()
            .map(|(index, &parameter)| {
                if index == 0 {
                    self.related_info_for_node(
                        parameter,
                        &diagnostics::Non_simple_parameter_declared_here,
                        &[],
                    )
                } else {
                    self.related_info_for_node(parameter, &diagnostics::and_here, &[])
                }
            })
            .collect();
        self.error_at_with_related(
            Some(use_strict),
            &diagnostics::use_strict_directive_cannot_be_used_with_non_simple_parameter_list,
            &[],
            related,
        );
        true
    }

    /// findUseStrictPrologue (89439-89445): leading ExpressionStatement
    /// string-literal prologue scan.
    fn find_use_strict_prologue(&self, body: NodeId) -> Option<NodeId> {
        let NodeData::Block(data) = self.data_of(body) else {
            return None;
        };
        for statement in self.nodes_of(data.statements) {
            if self.kind_of(statement) != SyntaxKind::ExpressionStatement {
                return None;
            }
            let NodeData::ExpressionStatement(statement_data) = self.data_of(statement) else {
                return None;
            };
            let Some(expression) = statement_data.expression else {
                return None;
            };
            if self.kind_of(expression) != SyntaxKind::StringLiteral {
                return None;
            }
            let NodeData::StringLiteral(literal) = self.data_of(expression) else {
                return None;
            };
            if literal.text == "use strict" {
                return Some(statement);
            }
        }
        None
    }
    // ---- band-local helpers ----

    /// getEffectiveReturnTypeNode (the non-JSDoc face): the declared
    /// return annotation of a function-like declaration.
    pub(crate) fn effective_return_type_node(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            _ => None,
        }
    }

    /// tsc-port: widenTypeInferredFromInitializer @6.0.3
    /// tsc-hash: fe71fba645cae40dc0d8f96f156ac4f0795719f4d9cbeb9760e31c161f9cea30
    /// tsc-span: _tsc.js:80690-80702
    ///
    /// The Constant/readonly guard keeps the literal (5.6: routed
    /// through getWidenedLiteralTypeForInitializer so isDeclaration-
    /// Readonly participates). The isInJSFile empty-literal arms
    /// change the RESULT type (anyType/anyArrayType) even when the
    /// checkJs report gate is off — escape rather than diverge.
    pub(crate) fn widen_type_inferred_from_initializer(
        &mut self,
        declaration: NodeId,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        if self.is_in_js_file(declaration) {
            return Err(Unsupported::new(
                "widenTypeInferredFromInitializer JS empty-literal arms ([JSDOC])",
            ));
        }
        self.get_widened_literal_type_for_initializer(declaration, ty)
    }

    /// The parameter NodeArray of a function-like (for trailing-comma
    /// grammar spans).
    fn parameter_array_of_function(&self, node: NodeId) -> Option<tsrs2_syntax::NodeArrayId> {
        match self.data_of(node) {
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            _ => None,
        }
    }

    /// tsc-port: isParameterOrMutableLocalVariable @6.0.3
    /// tsc-hash: 1164e87c02e8624766e950b63d9d8c46443d3ff7cc03e910f6eb1a4ffc30e492
    /// tsc-span: _tsc.js:71595-71598
    ///
    /// tsc-port: isMutableLocalVariableDeclaration @6.0.3
    /// tsc-hash: 30bf18a9c81fd230aec4c144496955920925daa53ca0b099edfff2ff459b9d6b
    /// tsc-span: _tsc.js:71599-71601
    fn is_parameter_or_mutable_local_variable(&self, symbol: SymbolId) -> bool {
        let Some(declaration) = self.binder.symbol(symbol).value_declaration else {
            return false;
        };
        let source = self.binder.source_of_node(declaration);
        let root = node_util::get_root_declaration(source, declaration);
        match self.kind_of(root) {
            SyntaxKind::Parameter => true,
            SyntaxKind::VariableDeclaration => {
                let parent = self.parent_of(root);
                let is_catch =
                    parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::CatchClause);
                if is_catch {
                    return true;
                }
                // isMutableLocalVariableDeclaration: `let` list, not
                // exported, and not a global-file top-level statement.
                const LET: i32 = 1;
                let let_list = parent.is_some_and(|parent| self.node_flags(parent) & LET != 0);
                if !let_list {
                    return false;
                }
                let exported = node_util::get_combined_modifier_flags(source, root)
                    .intersects(tsrs2_types::ModifierFlags::EXPORT);
                if exported {
                    return false;
                }
                let statement = parent.and_then(|parent| self.parent_of(parent));
                let global_top_level = statement.is_some_and(|statement| {
                    self.kind_of(statement) == SyntaxKind::VariableStatement
                        && self.parent_of(statement).is_some_and(|container| {
                            self.kind_of(container) == SyntaxKind::SourceFile
                                && source.external_module_indicator.is_none()
                        })
                });
                !global_top_level
            }
            _ => false,
        }
    }

    /// isEffectiveExternalModule: the commonJsModuleIndicator half is
    /// JS-only; TS files answer by the parser's external-module
    /// indicator.
    fn is_effective_external_module(&self, node: NodeId) -> bool {
        self.binder
            .source_of_node(node)
            .external_module_indicator
            .is_some()
    }

    /// tsc-port: getEffectiveCheckNode @6.0.3
    /// tsc-hash: 85134a2e08a5f051fe697dc3459ffe7803efbcb52897f62a803de959f131644d
    /// tsc-span: _tsc.js:76190-76193
    ///
    /// skipOuterExpressions(Parentheses | Satisfies) — the two kinds
    /// interleave in any order.
    pub(crate) fn get_effective_check_node(&self, argument: NodeId) -> NodeId {
        let mut node = argument;
        loop {
            match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => {
                    let Some(expression) = data.expression else {
                        return node;
                    };
                    node = expression;
                }
                NodeData::SatisfiesExpression(data) => {
                    let Some(expression) = data.expression else {
                        return node;
                    };
                    node = expression;
                }
                _ => return node,
            }
        }
    }

    /// The token-span diagnostic shape shared by checkAwaitGrammar's
    /// createFileDiagnostic sites (span = token at node.pos).
    fn span_of_token_at_node_pos(&self, node: NodeId) -> (u32, u32) {
        let source = self.binder.source_of_node(node);
        let pos = source.arena.node(node).pos as usize;
        let (start, end) = node_util::get_span_of_token_at_position(source, pos);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start_utf16 = to_utf16(start);
        (start_utf16, to_utf16(end).saturating_sub(start_utf16))
    }

    fn error_at_span(
        &mut self,
        span: (u32, u32),
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) {
        self.error_at_span_with_related(span, node, message, args, Vec::new());
    }

    fn error_at_span_with_related(
        &mut self,
        span: (u32, u32),
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
        related: Vec<tsrs2_diags::RelatedInfo>,
    ) {
        let source = self.binder.source_of_node(node);
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let mut diagnostic = tsrs2_diags::Diagnostic::new(
            Some(source.file_name.clone()),
            Some(span.0),
            Some(span.1),
            tsrs2_diags::MessageChain::new(message, &args),
        );
        for info in related {
            diagnostic.related.push(info);
        }
        if !self
            .diagnostics
            .iter()
            .any(|existing| *existing == diagnostic)
        {
            self.diagnostics.push(diagnostic);
        }
    }

    /// tsc grammarErrorAtPos (90243): explicit-span grammar error,
    /// gated on the file having NO parse diagnostics.
    fn grammar_error_at_pos(
        &mut self,
        node: NodeId,
        start: u32,
        length: u32,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> bool {
        if self.has_parse_diagnostics(node) {
            return false;
        }
        self.error_at_span((start, length), node, message, args);
        true
    }

    /// createAnonymousType(symbol, emptySymbols, [signature], [], []):
    /// the returnOnlyType shape (79138).
    fn create_single_signature_anonymous_type(
        &mut self,
        symbol: Option<SymbolId>,
        signature: crate::state::SignatureId,
    ) -> TypeId {
        let id = self.create_resolved_empty_anonymous_type(symbol);
        let members = self
            .links
            .ty(id)
            .resolved_members
            .resolved()
            .expect("created resolved above");
        self.members_mut(members).call_signatures.push(signature);
        id
    }

    /// tsc-port: getGlobalPromiseType @6.0.3
    /// tsc-hash: 5671539aa2bade85fdd8c114bf4741539f9fbc9b403c00897ee66be19962df06
    /// tsc-span: _tsc.js:60750-60757
    pub(crate) fn get_global_promise_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(memo) = self.deferred_global_promise_type {
            return Ok((memo != self.empty_generic_type).then_some(memo));
        }
        let symbol = self.get_global_type_symbol("Promise", report_errors);
        if symbol.is_none() && !report_errors {
            return Ok(None);
        }
        let resolved = self.get_type_of_global_symbol(symbol, 1)?;
        self.deferred_global_promise_type = Some(resolved);
        Ok((resolved != self.empty_generic_type).then_some(resolved))
    }

    /// tsc-port: getGlobalPromiseConstructorSymbol @6.0.3
    /// tsc-hash: 40dd0097149011100b9db68105bde360ed986c349536d3e106c8633a99ff2cb5
    /// tsc-span: _tsc.js:60766-60768
    pub(crate) fn get_global_promise_constructor_symbol(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<Option<SymbolId>> {
        if let Some(memo) = self.deferred_global_promise_constructor_symbol {
            return Ok(memo);
        }
        let symbol = self.get_global_symbol(
            "Promise",
            SymbolFlags::VALUE,
            report_errors.then_some(&diagnostics::Cannot_find_global_value_0),
        );
        if symbol.is_some() || report_errors {
            self.deferred_global_promise_constructor_symbol = Some(symbol);
        }
        Ok(symbol)
    }

    /// tsc-port: getBindingElementTypeFromParentType @6.0.3
    /// tsc-hash: c9233c1ae6f500780146135e92ef33cbbb833c61acdb248037680798e79c0a0e
    /// tsc-span: _tsc.js:55952-56005
    ///
    /// The array-pattern arm needs checkIteratedTypeOrElementType
    /// ([ITER 5.8]) — escapes; object patterns run live.
    /// getFlowTypeOfDestructuring is the 5.5e [FLOW M5] identity stub.
    pub(crate) fn get_binding_element_type_from_parent_type(
        &mut self,
        declaration: NodeId,
        parent_type: TypeId,
        no_tuple_bounds_check: bool,
    ) -> CheckResult2<TypeId> {
        let mut parent_type = parent_type;
        if self.tables.flags_of(parent_type).intersects(TypeFlags::ANY) {
            return Ok(parent_type);
        }
        let pattern = self.parent_of(declaration).ok_or_else(|| {
            Unsupported::new("binding element without a pattern (parse recovery)")
        })?;
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let is_ambient_parameter = NodeFlags::from_bits(self.node_flags(declaration))
            .intersects(NodeFlags::AMBIENT)
            && node_util::is_part_of_parameter_declaration(
                self.binder.source_of_node(declaration),
                declaration,
            );
        if strict_null_checks && is_ambient_parameter {
            parent_type = self.get_non_nullable_type(parent_type)?;
        } else if strict_null_checks {
            let grandparent = self.parent_of(pattern);
            let grandparent_initializer =
                grandparent.and_then(|grandparent| self.initializer_of(grandparent));
            if let Some(initializer) = grandparent_initializer {
                let initializer_type = self.get_type_of_initializer(initializer)?;
                if !self.has_type_facts(initializer_type, tsrs2_types::TypeFacts::EQ_UNDEFINED)? {
                    parent_type = self
                        .get_type_with_facts(parent_type, tsrs2_types::TypeFacts::NE_UNDEFINED)?;
                }
            }
        }
        let ty;
        if self.kind_of(pattern) == SyntaxKind::ObjectBindingPattern {
            let NodeData::BindingElement(data) = self.data_of(declaration).clone() else {
                return Err(Unsupported::new(
                    "malformed binding element (parse recovery)",
                ));
            };
            if data.dot_dot_dot_token.is_some() {
                parent_type = self.get_reduced_type(parent_type)?;
                if self
                    .tables
                    .flags_of(parent_type)
                    .intersects(TypeFlags::UNKNOWN)
                    || !self.is_valid_spread_type(parent_type)?
                {
                    self.error_at(
                        Some(declaration),
                        &diagnostics::Rest_types_may_only_be_created_from_object_types,
                        &[],
                    );
                    return Ok(self.tables.intrinsics.error);
                }
                let elements = match self.data_of(pattern) {
                    NodeData::ObjectBindingPattern(pattern_data) => pattern_data.elements,
                    _ => None,
                };
                let mut literal_members = Vec::new();
                for element in self.nodes_of(elements) {
                    let NodeData::BindingElement(element_data) = self.data_of(element) else {
                        continue;
                    };
                    if element_data.dot_dot_dot_token.is_none() {
                        if let Some(name) = element_data.property_name.or(element_data.name) {
                            literal_members.push(name);
                        }
                    }
                }
                let symbol = self.node_symbol(declaration);
                ty = self.get_rest_type(parent_type, &literal_members, symbol)?;
            } else {
                let name = data.property_name.or(data.name).ok_or_else(|| {
                    Unsupported::new("binding element without a name (parse recovery)")
                })?;
                let index_type = self.get_literal_type_from_property_name(name)?;
                let access_flags = tsrs2_types::AccessFlags::EXPRESSION_POSITION
                    | if no_tuple_bounds_check || self.has_default_value(declaration) {
                        tsrs2_types::AccessFlags::ALLOW_MISSING
                    } else {
                        tsrs2_types::AccessFlags::NONE
                    };
                let declared = self
                    .get_indexed_access_type_or_undefined(
                        parent_type,
                        index_type,
                        access_flags,
                        Some(name),
                        None,
                        None,
                    )?
                    .unwrap_or(self.tables.intrinsics.error);
                ty = self.get_flow_type_of_destructuring(declaration, declared);
            }
        } else {
            return Err(Unsupported::new(
                "array binding pattern element type (checkIteratedTypeOrElementType [ITER], 5.8)",
            ));
        }
        if self.initializer_of(declaration).is_none() {
            return Ok(ty);
        }
        let source = self.binder.source_of_node(declaration);
        let walked = node_util::walk_up_binding_elements_and_patterns(source, declaration);
        let walked_annotation = walked
            .map(|walked| self.effective_type_annotation_node(walked))
            .unwrap_or(None);
        if walked_annotation.is_some() {
            if !strict_null_checks {
                return Ok(ty);
            }
            let initializer_type =
                self.check_declaration_initializer(declaration, CheckMode::NORMAL, None)?;
            if !self.has_type_facts(initializer_type, tsrs2_types::TypeFacts::IS_UNDEFINED)? {
                return self.get_non_undefined_type(ty);
            }
            return Ok(ty);
        }
        let non_undefined = self.get_non_undefined_type(ty)?;
        let initializer_type =
            self.check_declaration_initializer(declaration, CheckMode::NORMAL, None)?;
        let union =
            self.get_union_type_ex(&[non_undefined, initializer_type], UnionReduction::Subtype)?;
        self.widen_type_inferred_from_initializer(declaration, union)
    }

    /// tsc-port: getNonUndefinedType @6.0.3
    /// tsc-hash: 5f29daa4407d5acb7d4db0db7a6a9828b6446830de42150fee571104eabf68ba
    /// tsc-span: _tsc.js:55888-55891
    fn get_non_undefined_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        let has_generic_undefined_constraint = self.some_type_result(ty, |state, t| {
            // isGenericTypeWithUndefinedConstraint (55885-55887).
            if !state.tables.flags_of(t).intersects(TypeFlags::INSTANTIABLE) {
                return Ok(false);
            }
            let constraint = state
                .get_base_constraint_of_type(t)?
                .unwrap_or(state.tables.intrinsics.unknown);
            Ok(state.maybe_type_of_kind(constraint, TypeFlags::UNDEFINED))
        })?;
        let type_or_constraint = if has_generic_undefined_constraint {
            self.map_type(
                ty,
                &mut |state, t| {
                    if state.tables.flags_of(t).intersects(TypeFlags::INSTANTIABLE) {
                        state.get_base_constraint_or_type(t).map(Some)
                    } else {
                        Ok(Some(t))
                    }
                },
                false,
            )?
            .expect("mapper is total")
        } else {
            ty
        };
        self.get_type_with_facts(type_or_constraint, tsrs2_types::TypeFacts::NE_UNDEFINED)
    }

    /// getTypeOfInitializer (69889): the links resolvedType cache or a
    /// fresh getTypeOfExpression.
    fn get_type_of_initializer(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(resolved) = self.links.node(node).resolved_type.resolved() {
            return Ok(resolved);
        }
        self.get_type_of_expression(node)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;

    /// Driver-level fixture check (operators.rs idiom): oracle-pinned
    /// rows (tsc 6.0.3, noLib, options {}) — scratchpad p.ts probes,
    /// 2026-07-13. Suggestion-band rows (6133/80007) are unmodeled and
    /// absent throughout; null-span global 2318 rows are file-less and
    /// filtered by the harness.
    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            state
                .diagnostics
                .iter()
                .filter(|diag| diag.file_name.is_some())
                .map(|diag| {
                    (
                        diag.code(),
                        diag.start.unwrap_or(u32::MAX),
                        diag.length.unwrap_or(u32::MAX),
                    )
                })
                .collect()
        })
    }

    // ---- fn-expression bodies (deferred pass) ----

    #[test]
    fn function_expression_block_bodies_check_deferred() {
        assert_eq!(
            checked_rows("(function () { \"x\".foo; });\n"),
            [(2339, 19, 3)]
        );
    }

    #[test]
    fn function_declaration_signature_infers_from_body() {
        // h : () => string via getSignatureFromDeclaration +
        // getReturnTypeFromBody — unlocks the operator band on
        // function declarations (5.5e FN row).
        assert_eq!(
            checked_rows("function h() { return \"s\"; }\nh * 2;\n"),
            [(2362, 29, 1)]
        );
    }

    #[test]
    fn contextual_signature_types_unannotated_parameters() {
        assert_eq!(
            checked_rows("declare let cb: (n: number) => void;\ncb = (x) => { x.foo; };\n"),
            [(2339, 53, 3)]
        );
    }

    #[test]
    fn getter_bodies_infer_through_get_type_of_accessors() {
        // The 3417-band un-escape: "s" widens to string (unit-type
        // contextual widening, no contextual signature).
        assert_eq!(
            checked_rows("({ get g() { return \"s\" } }).g.bad;\n"),
            [(2339, 31, 3)]
        );
    }

    // ---- parameter-list grammar ----

    #[test]
    fn required_after_optional_reports_1016() {
        assert_eq!(
            checked_rows("(function (a?: number, b: string) {});\n"),
            [(1016, 23, 1)]
        );
    }

    #[test]
    fn optional_rest_reports_1047() {
        // Oracle also shows 2370 (rest must be array) from
        // checkSignatureDeclaration — the 5.8-DECL hook (recorded FN).
        assert_eq!(
            checked_rows("(function (...rest?: number[]) {});\n"),
            [(1047, 18, 1)]
        );
    }

    #[test]
    fn use_strict_with_non_simple_parameters_reports_1346_1347() {
        assert_eq!(
            checked_rows("(function (a = 2) { \"use strict\"; });\n"),
            [(1346, 11, 5), (1347, 20, 13)]
        );
    }

    // ---- await / yield grammar ----

    #[test]
    fn top_level_await_in_non_module_reports_1375() {
        assert_eq!(checked_rows("await 1;\n"), [(1375, 0, 5)]);
    }

    #[test]
    fn await_inside_plain_function_expression_reports_1308() {
        // related 1356 (mark the function async) rides on the 1308 row.
        assert_eq!(
            checked_rows("(function f2() { return await 2; });\n"),
            [(1308, 24, 5)]
        );
    }

    #[test]
    fn yield_outside_generator_reports_1163() {
        assert_eq!(
            checked_rows("(function () { yield 5; });\n"),
            [(1163, 15, 5)]
        );
    }

    // ---- await family error paths ----

    #[test]
    fn non_callable_then_callback_reports_1320() {
        // { then(cb: number): void } is thenable but its callback is
        // not callable — getAwaitedTypeNoAlias' thenable tail.
        assert_eq!(
            checked_rows(
                "declare const r: { then(cb: number): void };\n(async () => { await r; });\n"
            ),
            [(2697, 46, 24), (1320, 60, 7)]
        );
    }

    #[test]
    fn self_referential_thenable_reports_1062() {
        assert_eq!(
            checked_rows(
                "type T = { then(cb: (v: T) => void): void };\ndeclare const s: T;\n(async () => { await s; });\n"
            ),
            [(2697, 66, 24), (1062, 80, 7)]
        );
    }

    #[test]
    fn union_self_referential_thenable_reports_1062() {
        assert_eq!(
            checked_rows(
                "type U = number | { then(cb: (v: U) => void): void };\ndeclare const u: U;\n(async () => { await u; });\n"
            ),
            [(2697, 75, 24), (1062, 89, 7)]
        );
    }

    #[test]
    fn custom_thenable_awaits_to_its_promised_type() {
        // Oracle adds 2339 @94 (x.bad → number) — the `const x`
        // initializer typing is the 5.6 band, so that row is a
        // recorded FN here; the 2697 proves the awaited walk ran.
        assert_eq!(
            checked_rows(
                "declare const p: { then(cb: (v: number) => void): void };\n(async () => { const x = await p; x.bad; });\n"
            ),
            [(2697, 59, 41)]
        );
    }

    #[test]
    fn async_block_body_without_promise_reports_2697() {
        assert_eq!(
            checked_rows(
                "declare const th: { then: number };\ndeclare let r: () => void;\nr = async () => { await th; };\n"
            ),
            [(2697, 67, 25)]
        );
    }
}
