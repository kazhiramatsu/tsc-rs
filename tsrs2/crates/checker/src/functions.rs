//! M4 5.5f: the §8 functions/await/yield band — the fn-expression
//! trio (checkFunctionExpressionOrObjectLiteralMethod + the contextual
//! parameter/return assignment + the deferred body pass),
//! getReturnTypeFromBody with its aggregators and Promise wrappers,
//! the await family's error paths (the probe half landed errorNode-less
//! at 5.5e in operators.rs), and the yield grammar slice.
//!
//! Stage seams live where the extraction doc (§0/§8) puts them —
//! the [FLOW M5] pair retired at 6.6c (functionHasImplicitReturn and
//! checkAllCodePathsInNonVoidFunctionReturnOrThrow are real; the
//! 2355/2366/2534/7030 band is live); [ITER 5.8] generator bodies and
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
use crate::narrow::TypePredicateKind;
use crate::state::{CheckResult2, CheckerState, Unsupported};
use crate::structural::SignatureKind;
use tsrs2_diags::gen as diagnostics;
use tsrs2_diags::DiagnosticMessage;

pub(crate) const FUNCTION_FLAGS_GENERATOR: u32 = 1;
pub(crate) const FUNCTION_FLAGS_ASYNC: u32 = 2;
pub(crate) use crate::contextual::FUNCTION_FLAGS_INVALID;

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
            let name = self.name_of_node(node);
            self.check_collisions_for_declaration_name(node, name);
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
                            canonical_signature_cache: None,
                            base_signature_cache: None,
                            composite_kind: None,
                            composite_signatures: None,
                            optional_call_signature_cache: (None, None),
                            isolated_signature_kind: Some(SignatureKind::Call),
                            isolated_signature_type: None,
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
    /// instantiation) stay a named Unsupported until 7.4 — no
    /// production producer sets Inferential before then, and the
    /// 79174 mapper instantiation is live for Some contexts (7.1).
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
                    let inference_context = self.get_inference_context(node);
                    // 79166-79172 (live at 7.4): Inferential mode first
                    // infers from the ANNOTATED parameters/return, then
                    // a generic-rest contextual signature instantiates
                    // through the NON-fixing mapper (fresh inferences
                    // stay unfixed for the re-run).
                    let mut instantiated_contextual_signature: Option<crate::state::SignatureId> =
                        None;
                    if check_mode.intersects(CheckMode::INFERENTIAL) {
                        let context = inference_context
                            .expect("Inferential check mode implies an inference context (79167)");
                        self.infer_from_annotated_parameters_and_return(
                            signature,
                            contextual_signature,
                            context,
                        )?;
                        let rest_type = self.get_effective_rest_type(contextual_signature)?;
                        if rest_type.is_some_and(|rest_type| {
                            self.tables
                                .flags_of(rest_type)
                                .intersects(TypeFlags::TYPE_PARAMETER)
                        }) {
                            let non_fixing_mapper =
                                self.inference_context(context).non_fixing_mapper;
                            instantiated_contextual_signature = Some(self.instantiate_signature(
                                contextual_signature,
                                non_fixing_mapper,
                                false,
                            )?);
                        }
                    }
                    // 79174: otherwise instantiate through the
                    // context's fixing mapper when a context is in
                    // scope.
                    let instantiated_contextual_signature = match instantiated_contextual_signature
                    {
                        Some(instantiated) => instantiated,
                        None => match inference_context {
                            Some(context) => {
                                let mapper = self.inference_context(context).mapper;
                                self.instantiate_signature(contextual_signature, mapper, false)?
                            }
                            None => contextual_signature,
                        },
                    };
                    self.assign_contextual_parameter_types(
                        signature,
                        instantiated_contextual_signature,
                    )?;
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
                // 79184-79187 (live at 7.4): a WIDER non-context-
                // sensitive function still feeds its annotated
                // parameters/return into the inference context.
                if check_mode.intersects(CheckMode::INFERENTIAL) {
                    let context = self
                        .get_inference_context(node)
                        .expect("Inferential check mode implies an inference context (79185)");
                    self.infer_from_annotated_parameters_and_return(
                        signature,
                        contextual_signature,
                        context,
                    )?;
                }
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
            self.seal_signature_return_type(signature, return_type);
        }
        self.check_signature_declaration(node)
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
        self.check_all_code_paths_in_non_void_function_return_or_throw(node, return_type)?;
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
                // 79207-79210: a truthiness gate — an undefined
                // unwrap skips the return-expression relation.
                if let Some(return_or_promised_type) =
                    self.unwrap_return_type(return_type, function_flags)?
                {
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
        }
        Ok(())
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
    /// tsc-port: inferFromAnnotatedParametersAndReturn @6.0.3
    /// tsc-hash: f2ea611b9352712aab08666e375e665345ab679d5e59c236ad63344eb60db4eb
    /// tsc-span: _tsc.js:78351-78373
    ///
    /// tsc reads `inferenceContext.inferences` unchecked — both call
    /// sites (79168/79186) sit behind the Inferential guard, which
    /// only Some-context pushes produce.
    fn infer_from_annotated_parameters_and_return(
        &mut self,
        signature: crate::state::SignatureId,
        context_signature: crate::state::SignatureId,
        inference_context: crate::inference::InferenceContextId,
    ) -> CheckResult2<()> {
        let parameters = self.signature_of(signature).parameters.clone();
        let has_rest = self
            .signature_of(signature)
            .flags
            .intersects(SignatureFlags::HAS_REST_PARAMETER);
        let len = parameters.len() - usize::from(has_rest);
        for (index, &parameter) in parameters.iter().enumerate().take(len) {
            let declaration = self
                .binder
                .symbol(parameter)
                .value_declaration
                .expect("own-signature parameters carry their declaration (78354)");
            if let Some(type_node) = self.effective_type_annotation_node(declaration) {
                let annotated = self.get_type_from_type_node(type_node)?;
                let is_optional = self.is_optional_declaration(declaration);
                let source =
                    self.tables
                        .add_optionality(annotated, /*is_property*/ false, is_optional);
                let target = self.get_type_at_position(context_signature, index)?;
                let inferences = self.inference_context(inference_context).inferences.clone();
                self.infer_types(
                    &inferences,
                    source,
                    target,
                    tsrs2_types::InferencePriority::NONE,
                    false,
                )?;
            }
        }
        let return_type_node = self
            .signature_of(signature)
            .declaration
            .and_then(|declaration| self.effective_return_type_node(declaration));
        if let Some(return_type_node) = return_type_node {
            let source = self.get_type_from_type_node(return_type_node)?;
            let target = self.get_return_type_of_signature(context_signature)?;
            let inferences = self.inference_context(inference_context).inferences.clone();
            self.infer_types(
                &inferences,
                source,
                target,
                tsrs2_types::InferencePriority::NONE,
                false,
            )?;
        }
        Ok(())
    }

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
        self.links.set_symbol_type_contextual(
            self.speculation_depth,
            parameter,
            LinkSlot::Resolved(ty),
        );
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
                        self.links.set_symbol_type_contextual(
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
                // tsc writes unguarded: the compute above can fill the
                // slot with a circularity scar that this write repairs
                // (sanctioned overwrite; see links.rs).
                self.links.overwrite_symbol_type_for_binding_element(
                    self.speculation_depth,
                    symbol,
                    ty,
                );
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
        let mut yield_type: Option<TypeId> = None;
        let mut next_type: Option<TypeId> = None;
        let mut fallback_return_type = self.tables.intrinsics.void;
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
            // 78779-78786: return aggregation (no early exits — the
            // never fallback rides fallbackReturnType) + yield/next
            // aggregation.
            return_type = None;
            match self.check_and_aggregate_return_expression_types(func, check_mode)? {
                None => fallback_return_type = self.tables.intrinsics.never,
                Some(types) => {
                    if !types.is_empty() {
                        return_type =
                            Some(self.get_union_type_ex(&types, UnionReduction::Subtype)?);
                    }
                }
            }
            let (yield_types, next_types) =
                self.check_and_aggregate_yield_operand_types(func, check_mode)?;
            yield_type = if yield_types.is_empty() {
                None
            } else {
                Some(self.get_union_type_ex(&yield_types, UnionReduction::Subtype)?)
            };
            next_type = if next_types.is_empty() {
                None
            } else {
                Some(self.get_intersection_type(
                    &next_types,
                    tsrs2_types::IntersectionFlags::default(),
                )?)
            };
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
                        // 78799: `unwrapReturnType(...) || voidType`.
                        let unwrapped = self
                            .unwrap_return_type(contextual, function_flags)?
                            .unwrap_or(self.tables.intrinsics.void);
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
        if return_type.is_some() || yield_type.is_some() || next_type.is_some() {
            // reportErrorsFromWidening (78807-78810).
            if let Some(current) = yield_type {
                self.report_errors_from_widening(
                    func,
                    current,
                    Some(tsrs2_types::WideningKind::GENERATOR_YIELD),
                )?;
            }
            if let Some(current) = return_type {
                self.report_errors_from_widening(
                    func,
                    current,
                    Some(tsrs2_types::WideningKind::FUNCTION_RETURN),
                )?;
            }
            if let Some(current) = next_type {
                self.report_errors_from_widening(
                    func,
                    current,
                    Some(tsrs2_types::WideningKind::GENERATOR_NEXT),
                )?;
            }
            let any_unit = return_type.is_some_and(|t| self.is_unit_type(t))
                || yield_type.is_some_and(|t| self.is_unit_type(t))
                || next_type.is_some_and(|t| self.is_unit_type(t));
            if any_unit {
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
                                return_type
                            }
                        } else {
                            let signature_return =
                                self.get_return_type_of_signature(contextual_signature)?;
                            // 78815-78817: /*contextFlags*/ void 0.
                            self.instantiate_contextual_type_for_node(Some(signature_return), func)?
                        }
                    }
                };
                if is_generator {
                    yield_type = self
                        .get_widened_literal_like_type_for_contextual_iteration_type_if_needed(
                            yield_type,
                            contextual_type,
                            tsrs2_types::IterationTypeKind::YIELD,
                            is_async,
                        )?;
                    return_type = self
                        .get_widened_literal_like_type_for_contextual_iteration_type_if_needed(
                            return_type,
                            contextual_type,
                            tsrs2_types::IterationTypeKind::RETURN,
                            is_async,
                        )?;
                    next_type = self
                        .get_widened_literal_like_type_for_contextual_iteration_type_if_needed(
                            next_type,
                            contextual_type,
                            tsrs2_types::IterationTypeKind::NEXT,
                            is_async,
                        )?;
                } else {
                    return_type = self
                        .get_widened_literal_like_type_for_contextual_return_type_if_needed(
                            return_type,
                            contextual_type,
                            is_async,
                        )?;
                }
            }
            // Final getWidenedType (78827-78829).
            if let Some(current) = yield_type {
                yield_type = Some(self.get_widened_type(current)?);
            }
            if let Some(current) = return_type {
                return_type = Some(self.get_widened_type(current)?);
            }
            if let Some(current) = next_type {
                next_type = Some(self.get_widened_type(current)?);
            }
        }
        if is_generator {
            // 78832-78838.
            let yield_type = yield_type.unwrap_or(self.tables.intrinsics.never);
            let return_type = return_type.unwrap_or(fallback_return_type);
            let next_type = match next_type {
                Some(next_type) => next_type,
                None => self
                    .get_contextual_iteration_type(tsrs2_types::IterationTypeKind::NEXT, func)?
                    .unwrap_or(self.tables.intrinsics.unknown),
            };
            return self.create_generator_type(yield_type, return_type, next_type, is_async);
        }
        let final_return = return_type.unwrap_or(fallback_return_type);
        if is_async {
            self.create_promise_type(final_return)
        } else {
            Ok(final_return)
        }
    }

    /// tsc-port: checkAndAggregateYieldOperandTypes @6.0.3
    /// tsc-hash: a16b8857b4be707079b5be83bee1da62028d63253024b55124974a04795b1afd
    /// tsc-span: _tsc.js:78874-78902
    fn check_and_aggregate_yield_operand_types(
        &mut self,
        func: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<(Vec<TypeId>, Vec<TypeId>)> {
        let mut yield_types: Vec<TypeId> = Vec::new();
        let mut next_types: Vec<TypeId> = Vec::new();
        let is_async = self.get_function_flags(func) & FUNCTION_FLAGS_ASYNC != 0;
        let source = self.binder.source_of_node(func);
        let Some(body) = node_util::body_of(source, func) else {
            return Ok((yield_types, next_types));
        };
        let inner_mode =
            CheckMode::from_bits(check_mode.bits() & !CheckMode::SKIP_GENERIC_FUNCTIONS.bits());
        for yield_expression in self.collect_yield_expressions(body) {
            let (expression, asterisk_token) = match self.data_of(yield_expression) {
                NodeData::YieldExpression(data) => (data.expression, data.asterisk_token),
                _ => (None, None),
            };
            let mut yield_expression_type = match expression {
                Some(expression) => self.check_expression(expression, inner_mode)?,
                None => self.tables.intrinsics.undefined_widening,
            };
            if let Some(expression) = expression {
                if self.is_const_context(expression)? {
                    yield_expression_type = self
                        .tables
                        .get_regular_type_of_literal_type(yield_expression_type);
                }
            }
            let any = self.tables.intrinsics.any;
            let yielded = self.get_yielded_type_of_yield_expression(
                yield_expression,
                yield_expression_type,
                any,
                is_async,
            )?;
            if let Some(yielded) = yielded {
                if !yield_types.contains(&yielded) {
                    yield_types.push(yielded);
                }
            }
            let next_type = if asterisk_token.is_some() {
                let use_ = if is_async {
                    tsrs2_types::IterationUse::ASYNC_YIELD_STAR
                } else {
                    tsrs2_types::IterationUse::YIELD_STAR
                };
                let iteration_types =
                    self.get_iteration_types_of_iterable(yield_expression_type, use_, expression)?;
                iteration_types.map(|types| types.next_type)
            } else {
                self.get_contextual_type(yield_expression, tsrs2_types::ContextFlags::NONE)?
            };
            if let Some(next_type) = next_type {
                if !next_types.contains(&next_type) {
                    next_types.push(next_type);
                }
            }
        }
        Ok((yield_types, next_types))
    }

    /// tsc-port: getYieldedTypeOfYieldExpression @6.0.3
    /// tsc-hash: d4f6c9a1ee088d42e9287c8ff580ac4a7782234ad201e2ac7af0c0e11c1b6978
    /// tsc-span: _tsc.js:78903-78911
    ///
    /// `None` = tsc's undefined (the async getAwaitedType tail).
    fn get_yielded_type_of_yield_expression(
        &mut self,
        node: NodeId,
        expression_type: TypeId,
        sent_type: TypeId,
        is_async: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let (expression, asterisk_token) = match self.data_of(node) {
            NodeData::YieldExpression(data) => (data.expression, data.asterisk_token),
            _ => (None, None),
        };
        let error_node = expression.unwrap_or(node);
        let yielded_type = if asterisk_token.is_some() {
            let use_ = if is_async {
                tsrs2_types::IterationUse::ASYNC_YIELD_STAR
            } else {
                tsrs2_types::IterationUse::YIELD_STAR
            };
            self.check_iterated_type_or_element_type(
                use_,
                expression_type,
                sent_type,
                Some(error_node),
            )?
        } else {
            expression_type
        };
        if !is_async {
            return Ok(Some(yielded_type));
        }
        self.get_awaited_type_with_error(
            yielded_type,
            Some((
                error_node,
                if asterisk_token.is_some() {
                    &diagnostics::Type_of_iterated_elements_of_a_yield_operand_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member
                } else {
                    &diagnostics::Type_of_yield_operand_in_an_async_generator_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member
                },
            )),
        )
    }

    /// forEachYieldExpression (14300-14328) specialized to an ORDERED
    /// collector (source order matters for the aggregation unions'
    /// error attribution): the yield arm recurses into its operand
    /// AFTER the visit; declaration kinds stop; function-likes
    /// contribute only computed property names; type nodes stop.
    fn collect_yield_expressions(&self, body: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut worklist = vec![body];
        while let Some(node) = worklist.pop() {
            match self.kind_of(node) {
                SyntaxKind::YieldExpression => {
                    out.push(node);
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
                        let mut children = Vec::new();
                        tsrs2_syntax::for_each_child(
                            &source.arena,
                            source.arena.node(node),
                            |child| {
                                children.push(child);
                                false
                            },
                        );
                        // LIFO worklist: reversed push keeps source order.
                        for &child in children.iter().rev() {
                            worklist.push(child);
                        }
                    }
                }
            }
        }
        out
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
        let mut has_return_with_no_expression = self.function_has_implicit_return(func)?;
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
    /// The Identifier arm consumes the real assignment-marking family
    /// (isSymbolAssigned, live since 6.2); the binding-pattern arm
    /// went live at 6.6f (isSomeSymbolAssigned landed with that
    /// family).
    pub(crate) fn is_constant_reference(&mut self, node: NodeId) -> CheckResult2<bool> {
        match self.kind_of(node) {
            SyntaxKind::ThisKeyword => Ok(true),
            SyntaxKind::Identifier => {
                // 70379: a type-query `this` identifier falls out of
                // tsc's switch (the !isThisInTypeQuery guard) — not a
                // constant reference.
                if self.is_this_in_type_query(node) {
                    return Ok(false);
                }
                let Some(symbol) = self.get_resolved_symbol(node)? else {
                    return Ok(false);
                };
                if self.is_constant_variable(symbol) {
                    return Ok(true);
                }
                if self.is_parameter_or_mutable_local_variable(symbol)
                    && !self.is_symbol_assigned(symbol)?
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
                // 70385-70387 (LIVE since 6.6f — isSomeSymbolAssigned
                // landed with the definite-assignment family): a
                // parameter/catch-variable pattern is constant iff no
                // member symbol is ever assigned; a variable pattern
                // iff the declaration is const-like.
                let Some(parent) = self.parent_of(node) else {
                    return Ok(false);
                };
                let source = self.binder.source_of_node(parent);
                let root_declaration = node_util::get_root_declaration(source, parent);
                let root_kind = self.kind_of(root_declaration);
                let is_catch_clause_variable = root_kind == SyntaxKind::VariableDeclaration
                    && self
                        .parent_of(root_declaration)
                        .is_some_and(|declaration_parent| {
                            self.kind_of(declaration_parent) == SyntaxKind::CatchClause
                        });
                if root_kind == SyntaxKind::Parameter || is_catch_clause_variable {
                    return Ok(!self.is_some_symbol_assigned(root_declaration)?);
                }
                Ok(root_kind == SyntaxKind::VariableDeclaration
                    && self.is_var_const_like(root_declaration))
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

    /// tsc-port: getGlobalPromiseLikeType @6.0.3
    /// tsc-hash: ac8c0f35caff10d3e34cda093925f7dcaf5f40347ee803b6e363b54b6bef3aa2
    /// tsc-span: _tsc.js:60758-60765
    pub(crate) fn get_global_promise_like_type(
        &mut self,
        report_errors: bool,
    ) -> CheckResult2<Option<TypeId>> {
        if let Some(memo) = self.deferred_global_promise_like_type {
            return Ok((memo != self.empty_generic_type).then_some(memo));
        }
        let symbol = self.get_global_type_symbol("PromiseLike", report_errors);
        if symbol.is_none() && !report_errors {
            return Ok(None);
        }
        let resolved = self.get_type_of_global_symbol(symbol, 1)?;
        self.deferred_global_promise_like_type = Some(resolved);
        Ok((resolved != self.empty_generic_type).then_some(resolved))
    }

    /// tsc-port: createPromiseLikeType @6.0.3
    /// tsc-hash: 815667493c5a737be799570a3aef5987952582bafb15ee6d73fb814860b16c3c
    /// tsc-span: _tsc.js:78713-78723
    pub(crate) fn create_promise_like_type(
        &mut self,
        promised_type: TypeId,
    ) -> CheckResult2<TypeId> {
        let global_promise_like = self.get_global_promise_like_type(/*report_errors*/ true)?;
        let Some(global_promise_like) = global_promise_like else {
            return Ok(self.tables.intrinsics.unknown);
        };
        let unwrapped = self.unwrap_awaited_type(promised_type)?;
        let awaited = self
            .get_awaited_type_no_alias(unwrapped, None)?
            .unwrap_or(self.tables.intrinsics.unknown);
        Ok(self
            .tables
            .create_type_reference(global_promise_like, &[awaited]))
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
    /// `None` = tsc's undefined (only the async-generator awaited tail
    /// produces it, 84508); the missing-iteration-type arm returns
    /// errorType EXPLICITLY, and the async non-generator arm carries
    /// its own `|| errorType` belt — callers each keep their distinct
    /// undefined handling (`?? returnType`, truthiness skip, ||
    /// voidType).
    pub(crate) fn unwrap_return_type(
        &mut self,
        return_type: TypeId,
        function_flags: u32,
    ) -> CheckResult2<Option<TypeId>> {
        let is_generator = function_flags & FUNCTION_FLAGS_GENERATOR != 0;
        let is_async = function_flags & FUNCTION_FLAGS_ASYNC != 0;
        if is_generator {
            let return_iteration_type = self.get_iteration_type_of_generator_function_return_type(
                tsrs2_types::IterationTypeKind::RETURN,
                return_type,
                is_async,
            )?;
            let Some(return_iteration_type) = return_iteration_type else {
                return Ok(Some(self.tables.intrinsics.error));
            };
            if is_async {
                let unwrapped = self.unwrap_awaited_type(return_iteration_type)?;
                return self.get_awaited_type_no_alias(unwrapped, None);
            }
            return Ok(Some(return_iteration_type));
        }
        if is_async {
            let awaited = self.get_awaited_type_no_alias(return_type, None)?;
            return Ok(Some(awaited.unwrap_or(self.tables.intrinsics.error)));
        }
        Ok(Some(return_type))
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
        // 6.6f: syntax-probe gate → flag-exact containment for the
        // failed-return face. SUBTREE probe (6.6 review): a compound
        // operand (`return { a: u }` / `return [u]`) inherits a
        // seam-reverted descendant's wideness into its own type, so
        // the old subtree gate's coverage keeps its strength here.
        if let Some(effective) = effective_expr {
            if self.flow_answer_is_seam_reverted_within(effective)
                && !self.is_type_assignable_to(unwrapped_expr_type, unwrapped_return_type)?
            {
                return Err(Unsupported::new(
                    "failed return over a seam-reverted flow answer \
                     (unported narrowing dependency, M6/M8 seam)",
                ));
            }
        }
        // checkTypeAssignableToAndOptionallyElaborate — elaboration
        // first (the Step-12 idiom): a literal return operand that
        // reports an inner member/element row suppresses the outer
        // head. tsc passes the EFFECTIVE check node (84585-84587) —
        // outer parens AND satisfies strip off before elaborateError,
        // so `return ([1] satisfies [number])` still elaborates the
        // array literal (the entry arms alone never strip satisfies).
        let elaborated = match effective_expr {
            Some(effective) => {
                !self.is_type_assignable_to(unwrapped_expr_type, unwrapped_return_type)?
                    && self
                        .elaborate_literal_assignment(
                            effective,
                            unwrapped_return_type,
                            Some(&diagnostics::Type_0_is_not_assignable_to_type_1),
                        )?
                        .reported()
            }
            None => false,
        };
        if !elaborated {
            self.check_type_assignable_to(
                unwrapped_expr_type,
                unwrapped_return_type,
                error_node,
                &diagnostics::Type_0_is_not_assignable_to_type_1,
            )?;
        }
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
    /// resolution, 5.8d) and fall through as non-CommonJS; the
    /// default arm's 1378/2854 pair is LIVE for sub-ES2017 targets
    /// and non-ES2022-family module options (review find, PR #5).
    /// Since 5.8a `await using` declaration LISTS route here too —
    /// every message selects on isAwaitExpression(node) like tsc
    /// (the 2865-2868 family for lists).
    pub(crate) fn check_await_grammar(&mut self, node: NodeId) -> CheckResult2<bool> {
        let is_await_expression = self.kind_of(node) == SyntaxKind::AwaitExpression;
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
                if is_await_expression {
                    &diagnostics::await_expression_cannot_be_used_inside_a_class_static_block
                } else {
                    &diagnostics::await_using_statements_cannot_be_used_inside_a_class_static_block
                },
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
                            if is_await_expression {
                                &diagnostics::await_expressions_are_only_allowed_at_the_top_level_of_a_file_when_that_file_is_a_module_but_this_file_has_no_imports_or_exports_Consider_adding_an_empty_export_to_make_this_file_a_module
                            } else {
                                &diagnostics::await_using_statements_are_only_allowed_at_the_top_level_of_a_file_when_that_file_is_a_module_but_this_file_has_no_imports_or_exports_Consider_adding_an_empty_export_to_make_this_file_a_module
                            },
                            &[],
                        );
                        has_error = true;
                    }
                    // moduleKind ladder (79357-79383): Node16/18/20/
                    // NodeNext arms need impliedNodeFormat (module
                    // resolution, 5.8d) — treated as the non-CommonJS
                    // fallthrough (true-CJS node-flavor 2856-family
                    // rows stay FN); the default arm is LIVE for
                    // sub-ES2017 targets and non-ES2022-family module
                    // options (review find, PR #5: 1378/2854).
                    let module_kind = self.options.emit_module_kind();
                    let target_ok =
                        self.options.emit_script_target() >= tsrs2_types::ScriptTarget::ES2017;
                    let ladder_ok = match module_kind {
                        100 | 101 | 102 | 199 => target_ok,
                        7 | 99 | 200 | 4 => target_ok,
                        _ => false,
                    };
                    if !ladder_ok {
                        self.error_at_span(
                            span,
                            node,
                            if is_await_expression {
                                &diagnostics::Top_level_await_expressions_are_only_allowed_when_the_module_option_is_set_to_es2022_esnext_system_node16_node18_node20_nodenext_or_preserve_and_the_target_option_is_set_to_es2017_or_higher
                            } else {
                                &diagnostics::Top_level_await_using_statements_are_only_allowed_when_the_module_option_is_set_to_es2022_esnext_system_node16_node18_node20_nodenext_or_preserve_and_the_target_option_is_set_to_es2017_or_higher
                            },
                            &[],
                        );
                        has_error = true;
                    }
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
                    if is_await_expression {
                        &diagnostics::await_expressions_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules
                    } else {
                        &diagnostics::await_using_statements_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules
                    },
                    &[],
                    related,
                );
                has_error = true;
            }
        }
        if is_await_expression && self.is_in_parameter_initializer_before_containing_function(node)
        {
            self.error_at(
                Some(node),
                &diagnostics::await_expressions_cannot_be_used_in_a_parameter_initializer,
                &[],
            );
            has_error = true;
        }
        Ok(has_error)
    }

    // ---- yield ----

    /// tsc-port: checkYieldExpression @6.0.3
    /// tsc-hash: 0b3a8949d463bc687dfc4e9cfdba344821a310459bbf326e88bcbf681ad78c13
    /// tsc-span: _tsc.js:80447-80512
    ///
    /// Grammar closure runs eager (the 5.4 addLazyDiagnostic
    /// decision), as does the noImplicitAny 7057 closure. Emit-helper
    /// probes are importHelpers-gated (no-op).
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
        let is_async = function_flags & FUNCTION_FLAGS_ASYNC != 0;
        let (expression, asterisk_token) = match self.data_of(node) {
            NodeData::YieldExpression(data) => (data.expression, data.asterisk_token),
            _ => (None, None),
        };
        let mut return_type = self.get_return_type_from_annotation(func)?;
        if let Some(current) = return_type {
            if self.tables.flags_of(current).intersects(TypeFlags::UNION) {
                return_type = Some(self.filter_type_with(current, |state, t| {
                    state.check_generator_instantiation_assignability_to_return_type(
                        t,
                        function_flags,
                        /*error_node*/ None,
                    )
                })?);
            }
        }
        let iteration_types = match return_type {
            Some(return_type) => {
                self.get_iteration_types_of_generator_function_return_type(return_type, is_async)?
            }
            None => None,
        };
        let any = self.tables.intrinsics.any;
        let signature_yield_type = iteration_types.map(|types| types.yield_type).unwrap_or(any);
        let signature_next_type = iteration_types.map(|types| types.next_type).unwrap_or(any);
        let yield_expression_type = match expression {
            Some(expression) => self.check_expression(expression, CheckMode::NORMAL)?,
            None => self.tables.intrinsics.undefined_widening,
        };
        let yielded_type = self.get_yielded_type_of_yield_expression(
            node,
            yield_expression_type,
            signature_next_type,
            is_async,
        )?;
        if return_type.is_some() {
            if let Some(yielded_type) = yielded_type {
                // checkTypeAssignableToAndOptionallyElaborate —
                // head-only slice; errorNode = expression || node.
                self.check_type_assignable_to(
                    yielded_type,
                    signature_yield_type,
                    Some(expression.unwrap_or(node)),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
            }
        }
        if asterisk_token.is_some() {
            let use_ = if is_async {
                tsrs2_types::IterationUse::ASYNC_YIELD_STAR
            } else {
                tsrs2_types::IterationUse::YIELD_STAR
            };
            let iterated = self.get_iteration_type_of_iterable(
                use_,
                tsrs2_types::IterationTypeKind::RETURN,
                yield_expression_type,
                expression,
            )?;
            return Ok(iterated.unwrap_or(any));
        }
        if let Some(return_type) = return_type {
            let next = self.get_iteration_type_of_generator_function_return_type(
                tsrs2_types::IterationTypeKind::NEXT,
                return_type,
                is_async,
            )?;
            return Ok(next.unwrap_or(any));
        }
        let contextual_next =
            self.get_contextual_iteration_type(tsrs2_types::IterationTypeKind::NEXT, func)?;
        if let Some(contextual_next) = contextual_next {
            return Ok(contextual_next);
        }
        // The noImplicitAny 7057 closure (eager identity).
        if self
            .options
            .strict_option_value(self.options.no_implicit_any)
            && !self.expression_result_is_unused(node)
        {
            let contextual_type =
                self.get_contextual_type(node, tsrs2_types::ContextFlags::NONE)?;
            let contextual_is_any =
                contextual_type.is_some_and(|t| self.tables.flags_of(t).intersects(TypeFlags::ANY));
            if contextual_type.is_none() || contextual_is_any {
                self.error_at(
                    Some(node),
                    &diagnostics::yield_expression_implicitly_results_in_an_any_type_because_its_containing_generator_lacks_a_return_type_annotation,
                    &[],
                );
            }
        }
        Ok(any)
    }

    /// tsc-port: expressionResultIsUnused @6.0.3
    /// tsc-hash: 57c8d4656b6d1338c146dde5d8bb7d44bf64c2a678b8cb84fa7affe6fcb14595
    /// tsc-span: _tsc.js:19091-19114
    ///
    /// CommaListExpression is transform-synthesized — parse trees
    /// carry comma sequences as BinaryExpression, so only that arm is
    /// live here.
    fn expression_result_is_unused(&self, node: NodeId) -> bool {
        let mut node = node;
        loop {
            let Some(parent) = self.parent_of(node) else {
                return false;
            };
            match self.kind_of(parent) {
                SyntaxKind::ParenthesizedExpression => {
                    node = parent;
                }
                SyntaxKind::ExpressionStatement | SyntaxKind::VoidExpression => {
                    return true;
                }
                SyntaxKind::ForStatement => {
                    let (initializer, incrementor) = match self.data_of(parent) {
                        NodeData::ForStatement(data) => (data.initializer, data.incrementor),
                        _ => (None, None),
                    };
                    return initializer == Some(node) || incrementor == Some(node);
                }
                SyntaxKind::BinaryExpression => {
                    let NodeData::BinaryExpression(data) = self.data_of(parent) else {
                        return false;
                    };
                    let is_comma = data
                        .operator_token
                        .is_some_and(|token| self.kind_of(token) == SyntaxKind::CommaToken);
                    if !is_comma {
                        return false;
                    }
                    if data.left == Some(node) {
                        return true;
                    }
                    node = parent;
                }
                _ => return false,
            }
        }
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

    // ---- §5 member/function declaration drivers (5.8b) ----

    /// tsc-port: checkParameter @6.0.3
    /// tsc-hash: 1efd72b631aa59c1a5c9b853f95ad970dd43af2b03a95cb20024b2da103444da
    /// tsc-span: _tsc.js:81170-81205
    ///
    /// checkGrammarModifiers rides the M7-stub hook; the
    /// erasableSyntaxOnly row is option-absent (dead, §13 audit).
    pub(crate) fn check_parameter(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_modifiers(node);
        self.check_variable_like_declaration(node)?;
        let Some(func) = self.get_containing_function(node) else {
            return Err(Unsupported::new(
                "parameter outside a function (parse recovery)",
            ));
        };
        let source = self.binder.source_of_node(node);
        let func_kind = self.kind_of(func);
        let func_body = node_util::body_of(self.binder.source_of_node(func), func);
        let (name, dot_dot_dot_token, question_token, initializer) = match self.data_of(node) {
            NodeData::Parameter(data) => (
                data.name,
                data.dot_dot_dot_token,
                data.question_token,
                data.initializer,
            ),
            _ => (None, None, None, None),
        };
        if node_util::has_syntactic_modifier(
            source,
            node,
            tsrs2_types::ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
        ) {
            if !(func_kind == SyntaxKind::Constructor && func_body.is_some()) {
                self.error_at(
                    Some(node),
                    &diagnostics::A_parameter_property_is_only_allowed_in_a_constructor_implementation,
                    &[],
                );
            }
            if func_kind == SyntaxKind::Constructor {
                if let Some(name) = name {
                    if self.kind_of(name) == SyntaxKind::Identifier
                        && self.identifier_text_of(name) == Some("constructor")
                    {
                        self.error_at(
                            Some(name),
                            &diagnostics::constructor_cannot_be_used_as_a_parameter_property_name,
                            &[],
                        );
                    }
                }
            }
        }
        let name_is_pattern = name.is_some_and(|name| {
            node_util::is_binding_pattern(self.binder.source_of_node(name), name)
        });
        if initializer.is_none()
            && question_token.is_some()
            && name_is_pattern
            && func_body.is_some()
        {
            self.error_at(
                Some(node),
                &diagnostics::A_binding_pattern_parameter_cannot_be_optional_in_an_implementation_signature,
                &[],
            );
        }
        if let Some(name) = name {
            if self.kind_of(name) == SyntaxKind::Identifier {
                let text = self.identifier_text_of(name).map(str::to_owned);
                if matches!(text.as_deref(), Some("this") | Some("new")) {
                    let parameters = match self.data_of(func) {
                        NodeData::FunctionDeclaration(data) => data.parameters,
                        NodeData::FunctionExpression(data) => data.parameters,
                        NodeData::ArrowFunction(data) => data.parameters,
                        NodeData::MethodDeclaration(data) => data.parameters,
                        NodeData::MethodSignature(data) => data.parameters,
                        NodeData::Constructor(data) => data.parameters,
                        NodeData::GetAccessor(data) => data.parameters,
                        NodeData::SetAccessor(data) => data.parameters,
                        NodeData::CallSignature(data) => data.parameters,
                        NodeData::ConstructSignature(data) => data.parameters,
                        NodeData::FunctionType(data) => data.parameters,
                        NodeData::ConstructorType(data) => data.parameters,
                        NodeData::IndexSignature(data) => data.parameters,
                        _ => None,
                    };
                    if self.nodes_of(parameters).first() != Some(&node) {
                        self.error_at(
                            Some(node),
                            &diagnostics::A_0_parameter_must_be_the_first_parameter,
                            &[text.as_deref().unwrap_or("")],
                        );
                    }
                    if matches!(
                        func_kind,
                        SyntaxKind::Constructor
                            | SyntaxKind::ConstructSignature
                            | SyntaxKind::ConstructorType
                    ) {
                        self.error_at(
                            Some(node),
                            &diagnostics::A_constructor_cannot_have_a_this_parameter,
                            &[],
                        );
                    }
                    if func_kind == SyntaxKind::ArrowFunction {
                        self.error_at(
                            Some(node),
                            &diagnostics::An_arrow_function_cannot_have_a_this_parameter,
                            &[],
                        );
                    }
                    if matches!(func_kind, SyntaxKind::GetAccessor | SyntaxKind::SetAccessor) {
                        self.error_at(
                            Some(node),
                            &diagnostics::get_and_set_accessors_cannot_declare_this_parameters,
                            &[],
                        );
                    }
                }
            }
        }
        if dot_dot_dot_token.is_some() && !name_is_pattern {
            let symbol = self.get_symbol_of_declaration(node)?;
            let raw = self.get_type_of_symbol(symbol)?;
            let reduced = self.get_reduced_type(raw)?;
            let any_readonly_array = self.any_readonly_array_type()?;
            if !self.is_type_assignable_to(reduced, any_readonly_array)? {
                self.error_at(
                    Some(node),
                    &diagnostics::A_rest_parameter_must_be_of_an_array_type,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: checkSignatureDeclaration @6.0.3
    /// tsc-hash: c740ef163bdd457d25dd1f9a18e6dde5f7cb24f26cdba99764015af155055c19
    /// tsc-span: _tsc.js:81289-81355
    ///
    /// Emit-helper probes are importHelpers-gated (no-op);
    /// checkUnmatchedJSDocParameters is JS-only; the JSDoc type-tag
    /// return-location indirection is JS-only (returnTypeErrorLocation
    /// === returnTypeNode in TS files);
    /// registerForUnusedIdentifiersCheck is M7-inert. The lazy tail
    /// runs eager (the 5.4 addLazyDiagnostic decision).
    pub(crate) fn check_signature_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        let kind = self.kind_of(node);
        if kind == SyntaxKind::IndexSignature {
            self.check_grammar_index_signature(node)?;
        } else if matches!(
            kind,
            SyntaxKind::FunctionType
                | SyntaxKind::FunctionDeclaration
                | SyntaxKind::ConstructorType
                | SyntaxKind::CallSignature
                | SyntaxKind::Constructor
                | SyntaxKind::ConstructSignature
        ) {
            self.check_grammar_function_like_declaration(node)?;
        }
        let (type_parameters, parameters, type_node) = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => {
                (data.type_parameters, data.parameters, data.r#type)
            }
            NodeData::FunctionExpression(data) => {
                (data.type_parameters, data.parameters, data.r#type)
            }
            NodeData::ArrowFunction(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::MethodDeclaration(data) => {
                (data.type_parameters, data.parameters, data.r#type)
            }
            NodeData::MethodSignature(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::Constructor(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::GetAccessor(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::SetAccessor(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::CallSignature(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::ConstructSignature(data) => {
                (data.type_parameters, data.parameters, data.r#type)
            }
            NodeData::FunctionType(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::ConstructorType(data) => (data.type_parameters, data.parameters, data.r#type),
            NodeData::IndexSignature(data) => (data.type_parameters, data.parameters, data.r#type),
            _ => (None, None, None),
        };
        let type_parameter_nodes = self.nodes_of(type_parameters);
        self.check_type_parameters(&type_parameter_nodes)?;
        // forEach(node.parameters, checkParameter) — DIRECT calls with
        // per-parameter Err containment (the checkTypeParameters
        // precedent: one out-of-slice parameter must not silence its
        // siblings).
        for parameter in self.nodes_of(parameters) {
            let _ = self.check_parameter(parameter);
        }
        if type_node.is_some() {
            self.check_source_element(type_node);
        }
        self.check_collision_with_arguments_in_generated_code(node);
        let return_type_node = type_node;
        if self
            .options
            .strict_option_value(self.options.no_implicit_any)
            && return_type_node.is_none()
        {
            match kind {
                SyntaxKind::ConstructSignature => {
                    self.error_at(
                        Some(node),
                        &diagnostics::Construct_signature_which_lacks_return_type_annotation_implicitly_has_an_any_return_type,
                        &[],
                    );
                }
                SyntaxKind::CallSignature => {
                    self.error_at(
                        Some(node),
                        &diagnostics::Call_signature_which_lacks_return_type_annotation_implicitly_has_an_any_return_type,
                        &[],
                    );
                }
                _ => {}
            }
        }
        if let Some(return_type_node) = return_type_node {
            let function_flags = self.get_function_flags(node);
            if function_flags & (FUNCTION_FLAGS_INVALID | FUNCTION_FLAGS_GENERATOR)
                == FUNCTION_FLAGS_GENERATOR
            {
                let return_type = self.get_type_from_type_node(return_type_node)?;
                if return_type == self.tables.intrinsics.void {
                    self.error_at(
                        Some(return_type_node),
                        &diagnostics::A_generator_cannot_have_a_void_type_annotation,
                        &[],
                    );
                } else {
                    self.check_generator_instantiation_assignability_to_return_type(
                        return_type,
                        function_flags,
                        Some(return_type_node),
                    )?;
                }
            } else if function_flags & (FUNCTION_FLAGS_ASYNC | FUNCTION_FLAGS_GENERATOR)
                == FUNCTION_FLAGS_ASYNC
            {
                self.check_async_function_return_type(node, return_type_node)?;
            }
        }
        Ok(())
    }

    /// tsc-port: checkCollisionWithArgumentsInGeneratedCode @6.0.3
    /// tsc-hash: 2a65d820227a75f185de001f3818cd4a5b17061d5288875c90b314b5a89e8b16
    /// tsc-span: _tsc.js:83229-83238
    ///
    /// Dead at target >= ES2015; LIVE for the ES5 fixture matrix. The
    /// row is errorSkippedOn("noEmit").
    fn check_collision_with_arguments_in_generated_code(&mut self, node: NodeId) {
        if self.options.emit_script_target() >= tsrs2_types::ScriptTarget::ES2015 {
            return;
        }
        let source = self.binder.source_of_node(node);
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
        let parameter_nodes = self.nodes_of(parameters);
        let has_rest = parameter_nodes.iter().any(|&parameter| {
            matches!(
                self.data_of(parameter),
                NodeData::Parameter(data) if data.dot_dot_dot_token.is_some()
            )
        });
        if !has_rest
            || NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT)
            || node_util::body_of(source, node).is_none()
        {
            return;
        }
        for parameter in parameter_nodes {
            let name = match self.data_of(parameter) {
                NodeData::Parameter(data) => data.name,
                _ => None,
            };
            let Some(name) = name else { continue };
            if self.kind_of(name) == SyntaxKind::Identifier
                && self.identifier_text_of(name) == Some("arguments")
            {
                self.error_skipped_on_no_emit(
                    Some(name),
                    &diagnostics::Duplicate_identifier_arguments_Compiler_uses_arguments_to_initialize_rest_parameters,
                    &[],
                );
            }
        }
    }

    /// tsc-port: checkAsyncFunctionReturnType @6.0.3
    /// tsc-hash: 284148376bf3f6e79a983118a7b503673819d16045bbc500d5ada6a482e2b2aa
    /// tsc-span: _tsc.js:82498-82579
    ///
    /// returnTypeNode === returnTypeErrorLocation in TS files, so
    /// reportErrorForInvalidReturnType reduces to a plain error at the
    /// return-type node. markLinkedReferences is emit-only (no-op).
    /// The ES5 relation's errorInfo chain is location-equal here —
    /// tsc's closure answers undefined — so the head carries alone.
    fn check_async_function_return_type(
        &mut self,
        node: NodeId,
        return_type_node: NodeId,
    ) -> CheckResult2<()> {
        let return_type = self.get_type_from_type_node(return_type_node)?;
        if self.options.emit_script_target() >= tsrs2_types::ScriptTarget::ES2015 {
            if return_type == self.tables.intrinsics.error {
                return Ok(());
            }
            let global_promise_type = self.get_global_promise_type(/*report_errors*/ true)?;
            if let Some(global_promise_type) = global_promise_type {
                if !self.is_reference_to_type(return_type, global_promise_type) {
                    let awaited = self.get_awaited_type_no_alias(return_type, None)?;
                    let display =
                        self.type_to_string_slice(awaited.unwrap_or(self.tables.intrinsics.void))?;
                    self.error_at(
                        Some(return_type_node),
                        &diagnostics::The_return_type_of_an_async_function_or_method_must_be_the_global_Promise_T_type_Did_you_mean_to_write_Promise_0,
                        &[&display],
                    );
                    return Ok(());
                }
            }
        } else {
            if return_type == self.tables.intrinsics.error {
                return Ok(());
            }
            let promise_constructor_name = self.get_entity_name_from_type_node(return_type_node);
            let Some(promise_constructor_name) = promise_constructor_name else {
                let display = self.type_to_string_slice(return_type)?;
                self.error_at(
                    Some(return_type_node),
                    &diagnostics::Type_0_is_not_a_valid_async_function_return_type_in_ES5_because_it_does_not_refer_to_a_Promise_compatible_constructor_value,
                    &[&display],
                );
                return Ok(());
            };
            let promise_constructor_symbol = self.resolve_entity_name(
                promise_constructor_name,
                SymbolFlags::VALUE,
                /*ignore_errors*/ true,
                None,
            )?;
            let promise_constructor_type = match promise_constructor_symbol {
                Some(symbol) => self.get_type_of_symbol(symbol)?,
                None => self.tables.intrinsics.error,
            };
            let entity_text = node_util::declaration_name_to_string(
                self.binder.source_of_node(promise_constructor_name),
                Some(promise_constructor_name),
            );
            if promise_constructor_type == self.tables.intrinsics.error {
                let is_plain_promise = self.kind_of(promise_constructor_name)
                    == SyntaxKind::Identifier
                    && self.identifier_text_of(promise_constructor_name) == Some("Promise")
                    && {
                        let target = self.tables.reference_target(return_type);
                        self.get_global_promise_type(/*report_errors*/ false)? == Some(target)
                    };
                if is_plain_promise {
                    self.error_at(
                        Some(return_type_node),
                        &diagnostics::An_async_function_or_method_in_ES5_requires_the_Promise_constructor_Make_sure_you_have_a_declaration_for_the_Promise_constructor_or_include_ES2015_in_your_lib_option,
                        &[],
                    );
                } else {
                    self.error_at(
                        Some(return_type_node),
                        &diagnostics::Type_0_is_not_a_valid_async_function_return_type_in_ES5_because_it_does_not_refer_to_a_Promise_compatible_constructor_value,
                        &[&entity_text],
                    );
                }
                return Ok(());
            }
            let global_promise_constructor_like =
                self.get_global_promise_constructor_like_type(/*report_errors*/ true)?;
            if global_promise_constructor_like == self.empty_object_type {
                self.error_at(
                    Some(return_type_node),
                    &diagnostics::Type_0_is_not_a_valid_async_function_return_type_in_ES5_because_it_does_not_refer_to_a_Promise_compatible_constructor_value,
                    &[&entity_text],
                );
                return Ok(());
            }
            if !self.check_type_assignable_to(
                promise_constructor_type,
                global_promise_constructor_like,
                Some(return_type_node),
                &diagnostics::Type_0_is_not_a_valid_async_function_return_type_in_ES5_because_it_does_not_refer_to_a_Promise_compatible_constructor_value,
            )? {
                return Ok(());
            }
            // The locals collision row (82559-82563).
            let root_name = self.get_first_identifier(promise_constructor_name);
            let root_text = self
                .identifier_text_of(root_name)
                .map(str::to_owned)
                .unwrap_or_default();
            let colliding = self
                .binder
                .locals_of(node)
                .and_then(|locals| locals.get(root_text.as_str()).copied());
            if let Some(colliding) = colliding {
                let colliding = self.get_merged_symbol(colliding);
                if self
                    .binder
                    .symbol(colliding)
                    .flags
                    .intersects(SymbolFlags::VALUE)
                {
                    let value_declaration = self.binder.symbol(colliding).value_declaration;
                    self.error_at(
                        value_declaration,
                        &diagnostics::Duplicate_identifier_0_Compiler_uses_declaration_1_to_support_async_functions,
                        &[&root_text, &entity_text],
                    );
                    return Ok(());
                }
            }
        }
        self.check_awaited_type(
            return_type,
            /*with_alias*/ false,
            node,
            &diagnostics::The_return_type_of_an_async_function_must_either_be_a_valid_promise_or_must_not_contain_a_callable_then_member,
        )?;
        Ok(())
    }

    /// getEntityNameFromTypeNode (14623-14635), TS arms only.
    fn get_entity_name_from_type_node(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::TypeReference(data) => data.type_name,
            NodeData::ExpressionWithTypeArguments(data) => data
                .expression
                .filter(|&expression| self.is_entity_name_expression(expression)),
            _ => matches!(
                self.kind_of(node),
                SyntaxKind::Identifier | SyntaxKind::QualifiedName
            )
            .then_some(node),
        }
    }

    /// getFirstIdentifier (the leftmost name of an entity chain).
    fn get_first_identifier(&self, name: NodeId) -> NodeId {
        let mut current = name;
        loop {
            match self.data_of(current) {
                NodeData::QualifiedName(data) => match data.left {
                    Some(left) => current = left,
                    None => return current,
                },
                NodeData::PropertyAccessExpression(data) => match data.expression {
                    Some(expression) => current = expression,
                    None => return current,
                },
                _ => return current,
            }
        }
    }

    /// hasBindableName (57638-57640): !hasDynamicName ||
    /// hasLateBindableName (AST half + the memoizing type half).
    /// tsc-port: hasBindableName @6.0.3
    /// tsc-hash: 45fe3426c08797ee297d285a79977d446d2cda77badbe86fcca00450c58d1c3b
    /// tsc-span: _tsc.js:57643-57645
    pub(crate) fn has_bindable_name(&mut self, node: NodeId) -> CheckResult2<bool> {
        let source = self.binder.source_of_node(node);
        if !node_util::has_dynamic_name(source, node) {
            return Ok(true);
        }
        if !self.has_late_bindable_ast_name(node) {
            return Ok(false);
        }
        let name = self
            .name_of_node(node)
            .expect("dynamic names are present names");
        if self.kind_of(name) != SyntaxKind::ComputedPropertyName {
            // Element-access declaration names are JS-only shapes.
            return Ok(false);
        }
        let name_type = self.check_computed_property_name(name)?;
        Ok(self.property_name_from_type_usable(name_type).is_some())
    }

    /// tsc-port: functionHasImplicitReturn @6.0.3
    /// tsc-hash: 82639fc96cdd05a5d0f6cec8552ebe828c87898536a91ec4ca88e3d2f606eec1
    /// tsc-span: _tsc.js:78956-78958
    ///
    /// REAL since 6.6: `func.endFlowNode && isReachableFlowNode(...)`.
    /// The binder records node_end_flow under the SAME bind-time
    /// verdict tsc uses to SET endFlowNode (containers.rs — presence
    /// here ≡ the old HAS_IMPLICIT_RETURN flag read), and the 6.6 walk
    /// adds the checker-side refinements the M4-era flag could not
    /// see: exhaustive switches and never-returning calls
    /// (getEffectsSignature).
    pub(crate) fn function_has_implicit_return(&mut self, func: NodeId) -> CheckResult2<bool> {
        let file = self.binder.file_index_of_node(func);
        let Some(&end_flow) = self.binder.file(file).node_end_flow.get(&func) else {
            return Ok(false);
        };
        self.is_reachable_flow_node(file, end_flow)
    }

    /// tsc-port: checkAllCodePathsInNonVoidFunctionReturnOrThrow @6.0.3
    /// tsc-hash: 608b7fb1571a161314fb3aa82e2817f347b15e39244a275b890104c5423e2dc6
    /// tsc-span: _tsc.js:79075-79108
    ///
    /// Eager (addLazyDiagnostic identity). The 79087-79095 arms are an
    /// else-if LADDER: an arm that ERRORS ends the chain, an arm whose
    /// CONDITION fails falls through to the trailing noImplicitReturns
    /// arm (79096) — a declared undefined-including return type under
    /// noImplicitReturns still reaches the 7030 face.
    pub(crate) fn check_all_code_paths_in_non_void_function_return_or_throw(
        &mut self,
        func: NodeId,
        return_type: Option<TypeId>,
    ) -> CheckResult2<()> {
        let function_flags = self.get_function_flags(func);
        let ty = match return_type {
            Some(return_type) => self.unwrap_return_type(return_type, function_flags)?,
            None => None,
        };
        if let Some(ty) = ty {
            if self.maybe_type_of_kind(ty, TypeFlags::VOID)
                || self.tables.flags_of(ty).intersects(TypeFlags::from_bits(
                    TypeFlags::ANY.bits() | TypeFlags::UNDEFINED.bits(),
                ))
            {
                return Ok(());
            }
        }
        let source = self.binder.source_of_node(func);
        let body = node_util::body_of(source, func);
        if self.kind_of(func) == SyntaxKind::MethodSignature
            || body.is_none()
            || body.is_some_and(|body| self.kind_of(body) != SyntaxKind::Block)
            || !self.function_has_implicit_return(func)?
        {
            return Ok(());
        }
        let has_explicit_return =
            NodeFlags::from_bits(self.node_flags(func)).intersects(NodeFlags::HAS_EXPLICIT_RETURN);
        let error_node = self.type_annotation_of(func).unwrap_or(func);
        if let Some(ty) = ty {
            if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
                self.error_at(
                    Some(error_node),
                    &diagnostics::A_function_returning_never_cannot_have_a_reachable_end_point,
                    &[],
                );
                return Ok(());
            }
            if !has_explicit_return {
                self.error_at(
                    Some(error_node),
                    &diagnostics::A_function_whose_declared_type_is_neither_undefined_void_nor_any_must_return_a_value,
                    &[],
                );
                return Ok(());
            }
            let strict_null_checks = self
                .options
                .strict_option_value(self.options.strict_null_checks);
            if strict_null_checks {
                let undefined_type = self.tables.intrinsics.undefined;
                if !self.is_type_assignable_to(undefined_type, ty)? {
                    self.error_at(
                        Some(error_node),
                        &diagnostics::Function_lacks_ending_return_statement_and_return_type_does_not_include_undefined,
                        &[],
                    );
                    return Ok(());
                }
            }
        }
        if self.options.no_implicit_returns == Some(true) {
            if ty.is_none() {
                if !has_explicit_return {
                    return Ok(());
                }
                let signature = self.get_signature_from_declaration(func)?;
                let inferred_return_type = self.get_return_type_of_signature(signature)?;
                if self
                    .is_unwrapped_return_type_undefined_void_or_any(func, inferred_return_type)?
                {
                    return Ok(());
                }
            }
            self.error_at(
                Some(error_node),
                &diagnostics::Not_all_code_paths_return_a_value,
                &[],
            );
        }
        Ok(())
    }

    /// tsc-port: isUnwrappedReturnTypeUndefinedVoidOrAny @6.0.3
    /// tsc-hash: 770b8189436fbaf4a6f320823cc2d13bfe7bcb8b367af9e97c5e4cae4cfcc480
    /// tsc-span: _tsc.js:84512-84515
    pub(crate) fn is_unwrapped_return_type_undefined_void_or_any(
        &mut self,
        func: NodeId,
        return_type: TypeId,
    ) -> CheckResult2<bool> {
        let function_flags = self.get_function_flags(func);
        let ty = self.unwrap_return_type(return_type, function_flags)?;
        Ok(ty.is_some_and(|ty| {
            self.maybe_type_of_kind(ty, TypeFlags::VOID)
                || self.tables.flags_of(ty).intersects(TypeFlags::from_bits(
                    TypeFlags::ANY.bits() | TypeFlags::UNDEFINED.bits(),
                ))
        }))
    }

    /// tsc-port: checkFunctionOrMethodDeclaration @6.0.3
    /// tsc-hash: 2fbb401c655a6f56f2d5b09c206d2a1f2ef7791c58986f4371ad680c7021f07f
    /// tsc-span: _tsc.js:82899-82941
    ///
    /// FUNCTION DECLARATION BODIES DRIVE EAGERLY here (the 5.5f
    /// deferred path covers fn EXPRESSIONS only); the JSDoc type-tag
    /// arm is JS-only. The lazy tail runs eager.
    pub(crate) fn check_function_or_method_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        self.check_decorators(node)?;
        self.check_signature_declaration(node)?;
        let function_flags = self.get_function_flags(node);
        let name = self.name_of_node(node);
        if let Some(name) = name {
            if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                self.check_computed_property_name(name)?;
            }
        }
        if self.has_bindable_name(node)? {
            let symbol = self.get_symbol_of_declaration(node)?;
            let local_symbol = self
                .binder
                .file(self.binder.file_index_of_node(node))
                .node_local_symbol
                .get(&node)
                .copied()
                .unwrap_or(symbol);
            let node_kind = self.kind_of(node);
            let first_declaration = self
                .binder
                .symbol(local_symbol)
                .declarations
                .iter()
                .copied()
                .find(|&declaration| self.kind_of(declaration) == node_kind);
            if first_declaration == Some(node) {
                self.check_function_or_constructor_symbol(local_symbol)?;
            }
            if self.binder.symbol(symbol).parent.is_some() {
                self.check_function_or_constructor_symbol(symbol)?;
            }
        }
        let body = if self.kind_of(node) == SyntaxKind::MethodSignature {
            None
        } else {
            node_util::body_of(self.binder.source_of_node(node), node)
        };
        self.check_source_element(body);
        let annotated_return = self.get_return_type_from_annotation(node)?;
        self.check_all_code_paths_in_non_void_function_return_or_throw(node, annotated_return)?;
        // The lazy tail (eager identity).
        if self.type_annotation_of(node).is_none() {
            let body_missing = body.is_none()
                || node_util::node_is_missing(self.binder.source_of_node(node), body);
            if body_missing && !self.is_private_within_ambient(node) {
                let any = self.tables.intrinsics.any;
                self.report_implicit_any(node, any, None)?;
            }
            if function_flags & FUNCTION_FLAGS_GENERATOR != 0
                && body.is_some_and(|body| {
                    !node_util::node_is_missing(self.binder.source_of_node(node), Some(body))
                })
            {
                // FORCING demand: yield aggregation through the
                // signature's return type.
                let signature = self.get_signature_from_declaration(node)?;
                self.get_return_type_of_signature(signature)?;
            }
        }
        Ok(())
    }

    /// tsc-port: isPrivateWithinAmbient @6.0.3
    /// tsc-hash: ef8656cf63ef4572437e31c6e94657e5d7347469b8186a82faf2194ccf706b59
    /// tsc-span: _tsc.js:82010-82012
    pub(crate) fn is_private_within_ambient(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        (node_util::has_syntactic_modifier(source, node, tsrs2_types::ModifierFlags::PRIVATE)
            || self.is_private_identifier_class_element(node))
            && NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT)
    }

    /// tsc-port: checkFunctionDeclaration @6.0.3
    /// tsc-hash: 956771632475180065203b669105ea260e3170c06f61bfcf41c9ecc36edfd888
    /// tsc-span: _tsc.js:82784-82791
    pub(crate) fn check_function_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_function_or_method_declaration(node)?;
        self.check_grammar_for_generator(node);
        let name = self.name_of_node(node);
        self.check_collisions_for_declaration_name(node, name);
        Ok(())
    }

    /// tsc-port: checkMethodDeclaration @6.0.3
    /// tsc-hash: 7b7dfb1af0fea107714f252f92aa8c55ed9a6fb974e7043caf434e427c83ab0f
    /// tsc-span: _tsc.js:81522-81535
    pub(crate) fn check_method_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        if !self.check_grammar_method(node)? {
            if let Some(name) = self.name_of_node(node) {
                self.check_grammar_computed_property_name(name);
            }
        }
        let is_method_declaration = self.kind_of(node) == SyntaxKind::MethodDeclaration;
        let name = self.name_of_node(node);
        if is_method_declaration {
            let asterisk = matches!(
                self.data_of(node),
                NodeData::MethodDeclaration(data) if data.asterisk_token.is_some()
            );
            if asterisk {
                if let Some(name) = name {
                    if self.kind_of(name) == SyntaxKind::Identifier
                        && self.identifier_text_of(name) == Some("constructor")
                    {
                        self.error_at(
                            Some(name),
                            &diagnostics::Class_constructor_may_not_be_a_generator,
                            &[],
                        );
                    }
                }
            }
        }
        self.check_function_or_method_declaration(node)?;
        let source = self.binder.source_of_node(node);
        let body = node_util::body_of(source, node);
        if node_util::has_syntactic_modifier(source, node, tsrs2_types::ModifierFlags::ABSTRACT)
            && is_method_declaration
            && body.is_some()
        {
            let display = name
                .map(|name| {
                    node_util::declaration_name_to_string(
                        self.binder.source_of_node(name),
                        Some(name),
                    )
                })
                .unwrap_or_default();
            self.error_at(
                Some(node),
                &diagnostics::Method_0_cannot_have_an_implementation_because_it_is_marked_abstract,
                &[&display],
            );
        }
        if let Some(name) = name {
            if self.kind_of(name) == SyntaxKind::PrivateIdentifier
                && node_util::get_containing_class(source, node).is_none()
            {
                self.error_at(
                    Some(node),
                    &diagnostics::Private_identifiers_are_not_allowed_outside_class_bodies,
                    &[],
                );
            }
        }
        self.set_node_links_for_private_identifier_scope(node);
        Ok(())
    }

    /// tsc-port: checkPropertyDeclaration @6.0.3
    /// tsc-hash: fe84062922bd92bdd3e7794e8bec2e7a757490511cb4989fad1b96fecae1d88d
    /// tsc-span: _tsc.js:81508-81515
    pub(crate) fn check_property_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        let grammar_reported = {
            let modifiers_reported = self.check_grammar_modifiers(node);
            modifiers_reported || self.check_grammar_property(node)?
        };
        if !grammar_reported {
            if let Some(name) = self.name_of_node(node) {
                self.check_grammar_computed_property_name(name);
            }
        }
        self.check_variable_like_declaration(node)?;
        self.set_node_links_for_private_identifier_scope(node);
        let source = self.binder.source_of_node(node);
        let initializer = match self.data_of(node) {
            NodeData::PropertyDeclaration(data) => data.initializer,
            _ => None,
        };
        if node_util::has_syntactic_modifier(source, node, tsrs2_types::ModifierFlags::ABSTRACT)
            && self.kind_of(node) == SyntaxKind::PropertyDeclaration
            && initializer.is_some()
        {
            let display = self
                .name_of_node(node)
                .map(|name| {
                    node_util::declaration_name_to_string(
                        self.binder.source_of_node(name),
                        Some(name),
                    )
                })
                .unwrap_or_default();
            self.error_at(
                Some(node),
                &diagnostics::Property_0_cannot_have_an_initializer_because_it_is_marked_abstract,
                &[&display],
            );
        }
        Ok(())
    }

    /// tsc-port: checkPropertySignature @6.0.3
    /// tsc-hash: 5ec7037fde36394f480b2002581b97e85c333f095d83c9015ff7ca1b5d281b3e
    /// tsc-span: _tsc.js:81516-81521
    pub(crate) fn check_property_signature(&mut self, node: NodeId) -> CheckResult2<()> {
        if let Some(name) = self.name_of_node(node) {
            if self.kind_of(name) == SyntaxKind::PrivateIdentifier {
                self.error_at(
                    Some(node),
                    &diagnostics::Private_identifiers_are_not_allowed_outside_class_bodies,
                    &[],
                );
            }
        }
        self.check_property_declaration(node)
    }

    /// tsc-port: setNodeLinksForPrivateIdentifierScope @6.0.3
    /// tsc-hash: a3f5ba27fc2fb97d9c9987c0a09b74d9301ff572b74a4a018ebb222c74367b42
    /// tsc-span: _tsc.js:81536-81551
    ///
    /// The class-expression-in-iteration arm sets emit-only flags
    /// (BlockScopedBindingInLoop / LoopWithCapturedBlockScopedBinding)
    /// — elided; no check-path reads them.
    fn set_node_links_for_private_identifier_scope(&mut self, node: NodeId) {
        let Some(name) = self.name_of_node(node) else {
            return;
        };
        if self.kind_of(name) != SyntaxKind::PrivateIdentifier {
            return;
        }
        // LanguageFeatureMinimumTarget.PrivateNamesAndClassStaticBlocks
        // = ClassAndClassElementDecorators = ES2022.
        if self.options.emit_script_target() < tsrs2_types::ScriptTarget::ES2022
            || !self.options.use_define_for_class_fields_effective()
        {
            let mut lexical_scope = self.get_enclosing_block_scope_container(node);
            while let Some(scope) = lexical_scope {
                self.links.or_node_check_flags(
                    self.speculation_depth,
                    scope,
                    tsrs2_types::NodeCheckFlags::CONTAINS_CLASS_WITH_PRIVATE_IDENTIFIERS,
                );
                lexical_scope = self.get_enclosing_block_scope_container(scope);
            }
        }
    }

    /// tsc-port: checkClassStaticBlockDeclaration @6.0.3
    /// tsc-hash: c69edb58bee2e5b97158cbe060be6384119f295fac411294c48ff6272658951c
    /// tsc-span: _tsc.js:81552-81555
    pub(crate) fn check_class_static_block_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        self.check_grammar_modifiers(node);
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        for child in children {
            self.check_source_element(Some(child));
        }
        Ok(())
    }

    /// tsc-port: checkConstructorDeclaration @6.0.3
    /// tsc-hash: 191587bffa4356eeeff6ed0796a6b497c9bb677692431871115655f54a277508
    /// tsc-span: _tsc.js:81556-81611
    /// (covers superCallIsRootLevelInConstructor 81612-81615 +
    /// nodeImmediatelyReferencesSuperOrThis 81616-81624 +
    /// findFirstSuperCall 72321-72323)
    ///
    /// captureLexicalThis is emit-only (no-op); the lazy tail runs
    /// eager. emitStandardClassFields makes the root-level band dead
    /// at the default target and LIVE for low-@target fixtures.
    pub(crate) fn check_constructor_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_signature_declaration(node)?;
        if !self.check_grammar_constructor_type_parameters(node) {
            self.check_grammar_constructor_type_annotation(node);
        }
        let body = match self.data_of(node) {
            NodeData::Constructor(data) => data.body,
            _ => None,
        };
        self.check_source_element(body);
        let symbol = self.get_symbol_of_declaration(node)?;
        let first_declaration = self.get_declaration_of_kind(symbol, SyntaxKind::Constructor);
        if first_declaration == Some(node) {
            self.check_function_or_constructor_symbol(symbol)?;
        }
        let source = self.binder.source_of_node(node);
        if node_util::node_is_missing(source, body) {
            return Ok(());
        }
        let body = body.expect("nodeIsMissing answered false");
        // The lazy tail (eager identity).
        let Some(containing_class) = self.parent_of(node) else {
            return Ok(());
        };
        if self
            .get_class_extends_heritage_element(containing_class)
            .is_some()
        {
            let class_extends_null = self.class_declaration_extends_null(containing_class)?;
            let super_call = self.find_first_super_call(body);
            if let Some(super_call) = super_call {
                if class_extends_null {
                    self.error_at(
                        Some(super_call),
                        &diagnostics::A_constructor_cannot_contain_a_super_call_when_its_class_extends_null,
                        &[],
                    );
                }
                let super_call_should_be_root_level = !self.options.emit_standard_class_fields()
                    && (self.class_has_initialized_instance_or_private_member(containing_class)
                        || self.constructor_has_parameter_property(node));
                if super_call_should_be_root_level {
                    if !self.super_call_is_root_level_in_constructor(super_call, body) {
                        self.error_at(
                            Some(super_call),
                            &diagnostics::A_super_call_must_be_a_root_level_statement_within_a_constructor_of_a_derived_class_that_contains_initialized_properties_parameter_properties_or_private_identifiers,
                            &[],
                        );
                    } else {
                        let statements = match self.data_of(body) {
                            NodeData::Block(data) => self.nodes_of(data.statements),
                            _ => Vec::new(),
                        };
                        let mut super_call_statement = None;
                        for statement in statements {
                            if self.kind_of(statement) == SyntaxKind::ExpressionStatement {
                                let expression = match self.data_of(statement) {
                                    NodeData::ExpressionStatement(data) => data.expression,
                                    _ => None,
                                };
                                if let Some(expression) = expression {
                                    let skipped = self.skip_outer_expressions(
                                        expression,
                                        crate::operators::OuterExpressionKinds::ALL,
                                    );
                                    if self.is_super_call(skipped) {
                                        super_call_statement = Some(statement);
                                        break;
                                    }
                                }
                            }
                            if self.node_immediately_references_super_or_this(statement) {
                                break;
                            }
                        }
                        if super_call_statement.is_none() {
                            self.error_at(
                                Some(node),
                                &diagnostics::A_super_call_must_be_the_first_statement_in_the_constructor_to_refer_to_super_or_this_when_a_derived_class_contains_initialized_properties_parameter_properties_or_private_identifiers,
                                &[],
                            );
                        }
                    }
                }
            } else if !class_extends_null {
                self.error_at(
                    Some(node),
                    &diagnostics::Constructors_for_derived_classes_must_contain_a_super_call,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// isInstancePropertyWithInitializerOrPrivateIdentifierProperty
    /// over the class members (81570-81575).
    fn class_has_initialized_instance_or_private_member(&self, class_node: NodeId) -> bool {
        let members = match self.data_of(class_node) {
            NodeData::ClassDeclaration(data) => data.members,
            NodeData::ClassExpression(data) => data.members,
            _ => None,
        };
        let source = self.binder.source_of_node(class_node);
        self.nodes_of(members).iter().any(|&member| {
            let _ = source;
            if self.is_private_identifier_class_element(member) {
                return true;
            }
            matches!(
                self.data_of(member),
                NodeData::PropertyDeclaration(data)
                    if data.initializer.is_some()
                        && !node_util::has_syntactic_modifier(
                            source,
                            member,
                            tsrs2_types::ModifierFlags::STATIC,
                        )
            )
        })
    }

    /// `some(node.parameters, p => hasSyntacticModifier(p, 31))`.
    fn constructor_has_parameter_property(&self, node: NodeId) -> bool {
        let parameters = match self.data_of(node) {
            NodeData::Constructor(data) => data.parameters,
            _ => None,
        };
        let source = self.binder.source_of_node(node);
        self.nodes_of(parameters).iter().any(|&parameter| {
            node_util::has_syntactic_modifier(
                source,
                parameter,
                tsrs2_types::ModifierFlags::PARAMETER_PROPERTY_MODIFIER,
            )
        })
    }

    /// findFirstSuperCall (72321-72323): forEachChild walk stopping at
    /// function-like boundaries.
    fn find_first_super_call(&self, node: NodeId) -> Option<NodeId> {
        if self.is_super_call(node) {
            return Some(node);
        }
        if node_util::is_function_like_kind(self.kind_of(node)) {
            return None;
        }
        let source = self.binder.source_of_node(node);
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
            .into_iter()
            .find_map(|child| self.find_first_super_call(child))
    }

    /// isPrivateIdentifierClassElementDeclaration (11944-11946).
    fn is_private_identifier_class_element(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::PropertyDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
        ) && self
            .name_of_node(node)
            .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
    }

    /// isThisContainerOrFunctionBlock (14505-14526).
    fn is_this_container_or_function_block(&self, node: NodeId) -> bool {
        match self.kind_of(node) {
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::PropertyDeclaration => true,
            SyntaxKind::Block => matches!(
                self.parent_of(node).map(|parent| self.kind_of(parent)),
                Some(SyntaxKind::Constructor)
                    | Some(SyntaxKind::MethodDeclaration)
                    | Some(SyntaxKind::GetAccessor)
                    | Some(SyntaxKind::SetAccessor)
            ),
            _ => false,
        }
    }

    /// isSuperCall: a CallExpression over the `super` keyword.
    fn is_super_call(&self, node: NodeId) -> bool {
        matches!(
            self.data_of(node),
            NodeData::CallExpression(data)
                if data.expression.is_some_and(
                    |expression| self.kind_of(expression) == SyntaxKind::SuperKeyword
                )
        )
    }

    /// superCallIsRootLevelInConstructor (81612-81615).
    fn super_call_is_root_level_in_constructor(&self, super_call: NodeId, body: NodeId) -> bool {
        let mut parent = self.parent_of(super_call);
        while let Some(current) = parent {
            if self.kind_of(current) == SyntaxKind::ParenthesizedExpression {
                parent = self.parent_of(current);
            } else {
                return self.kind_of(current) == SyntaxKind::ExpressionStatement
                    && self.parent_of(current) == Some(body);
            }
        }
        false
    }

    /// nodeImmediatelyReferencesSuperOrThis (81616-81624).
    fn node_immediately_references_super_or_this(&self, node: NodeId) -> bool {
        let kind = self.kind_of(node);
        if kind == SyntaxKind::SuperKeyword || kind == SyntaxKind::ThisKeyword {
            return true;
        }
        let source = self.binder.source_of_node(node);
        let _ = source;
        if self.is_this_container_or_function_block(node) {
            return false;
        }
        let mut children = Vec::new();
        tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
            children.push(child);
            false
        });
        children
            .into_iter()
            .any(|child| self.node_immediately_references_super_or_this(child))
    }

    /// tsc-port: checkAccessorDeclaration @6.0.3
    /// tsc-hash: 90af5f06939b919ab242980dd27109e1cf9c92f8c58c8789120f88283b016300
    /// tsc-span: _tsc.js:81625-81669
    ///
    /// The lazy block runs BEFORE the body (eager identity preserves
    /// tsc's diagnostic order); the getter/setter pair rows latch on
    /// the getter's TypeChecked NodeCheckFlags bit.
    pub(crate) fn check_accessor_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        let name = self.name_of_node(node);
        let source = self.binder.source_of_node(node);
        if let Some(name) = name {
            if self.kind_of(name) == SyntaxKind::Identifier
                && self.identifier_text_of(name) == Some("constructor")
                && node_util::get_containing_class(source, node)
                    == self.parent_of(node).filter(|&parent| {
                        matches!(
                            self.kind_of(parent),
                            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                        )
                    })
                && self.parent_of(node).is_some_and(|parent| {
                    matches!(
                        self.kind_of(parent),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                })
            {
                self.error_at(
                    Some(name),
                    &diagnostics::Class_constructor_may_not_be_an_accessor,
                    &[],
                );
            }
        }
        // The lazy diagnostics block (eager identity).
        let grammar_reported = {
            let fn_like = self.check_grammar_function_like_declaration(node)?;
            fn_like || self.check_grammar_accessor(node)?
        };
        if !grammar_reported {
            if let Some(name) = name {
                self.check_grammar_computed_property_name(name);
            }
        }
        self.check_decorators(node)?;
        self.check_signature_declaration(node)?;
        let is_get = self.kind_of(node) == SyntaxKind::GetAccessor;
        let body = node_util::body_of(source, node);
        if is_get {
            let flags = NodeFlags::from_bits(self.node_flags(node));
            if !flags.intersects(NodeFlags::AMBIENT)
                && body.is_some()
                && flags.intersects(NodeFlags::HAS_IMPLICIT_RETURN)
                && !flags.intersects(NodeFlags::HAS_EXPLICIT_RETURN)
            {
                self.error_at(name, &diagnostics::A_get_accessor_must_return_a_value, &[]);
            }
        }
        if let Some(name) = name {
            if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                self.check_computed_property_name(name)?;
            }
        }
        if self.has_bindable_name(node)? {
            let symbol = self.get_symbol_of_declaration(node)?;
            let getter = self.get_declaration_of_kind(symbol, SyntaxKind::GetAccessor);
            let setter = self.get_declaration_of_kind(symbol, SyntaxKind::SetAccessor);
            if let (Some(getter), Some(setter)) = (getter, setter) {
                let getter_checked = self
                    .links
                    .node(getter)
                    .check_flags
                    .intersects(tsrs2_types::NodeCheckFlags::TYPE_CHECKED);
                if !getter_checked {
                    self.links.or_node_check_flags(
                        self.speculation_depth,
                        getter,
                        tsrs2_types::NodeCheckFlags::TYPE_CHECKED,
                    );
                    let getter_source = self.binder.source_of_node(getter);
                    let setter_source = self.binder.source_of_node(setter);
                    let getter_flags =
                        node_util::get_syntactic_modifier_flags(getter_source, getter);
                    let setter_flags =
                        node_util::get_syntactic_modifier_flags(setter_source, setter);
                    let getter_name = self.name_of_node(getter);
                    let setter_name = self.name_of_node(setter);
                    if getter_flags.intersects(tsrs2_types::ModifierFlags::ABSTRACT)
                        != setter_flags.intersects(tsrs2_types::ModifierFlags::ABSTRACT)
                    {
                        self.error_at(
                            getter_name,
                            &diagnostics::Accessors_must_both_be_abstract_or_non_abstract,
                            &[],
                        );
                        self.error_at(
                            setter_name,
                            &diagnostics::Accessors_must_both_be_abstract_or_non_abstract,
                            &[],
                        );
                    }
                    let getter_less_accessible = (getter_flags
                        .intersects(tsrs2_types::ModifierFlags::PROTECTED)
                        && !setter_flags.intersects(tsrs2_types::ModifierFlags::from_bits(
                            tsrs2_types::ModifierFlags::PROTECTED.bits()
                                | tsrs2_types::ModifierFlags::PRIVATE.bits(),
                        )))
                        || (getter_flags.intersects(tsrs2_types::ModifierFlags::PRIVATE)
                            && !setter_flags.intersects(tsrs2_types::ModifierFlags::PRIVATE));
                    if getter_less_accessible {
                        self.error_at(
                            getter_name,
                            &diagnostics::A_get_accessor_must_be_at_least_as_accessible_as_the_setter,
                            &[],
                        );
                        self.error_at(
                            setter_name,
                            &diagnostics::A_get_accessor_must_be_at_least_as_accessible_as_the_setter,
                            &[],
                        );
                    }
                }
            }
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        let return_type = self.get_type_of_accessors(symbol)?;
        if is_get {
            self.check_all_code_paths_in_non_void_function_return_or_throw(
                node,
                Some(return_type),
            )?;
        }
        self.check_source_element(body);
        self.set_node_links_for_private_identifier_scope(node);
        Ok(())
    }

    /// tsc-port: checkMissingDeclaration @6.0.3
    /// tsc-hash: 452c55461683d3ab7d952ca2d6f80d4f55796de22a8e268b0309db7b876edaef
    /// tsc-span: _tsc.js:81670-81672
    pub(crate) fn check_missing_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_decorators(node)
    }

    /// tsc-port: getEffectiveDeclarationFlags @6.0.3
    /// tsc-hash: 6fa156d5b6aaa7dbafa694411f531167459eb0f991bd2472eb01f79800c1ad74
    /// tsc-span: _tsc.js:82013-82023
    ///
    /// The global-scope-augmentation exemption inspects module blocks
    /// — constructible only under `declare global` (5.8d modules);
    /// until then the ambient-export inference treats them like any
    /// other export-context container (divergence bounded to
    /// declare-global fixtures, recorded FN class).
    fn get_effective_declaration_flags(
        &self,
        node: NodeId,
        flags_to_check: tsrs2_types::ModifierFlags,
    ) -> tsrs2_types::ModifierFlags {
        let source = self.binder.source_of_node(node);
        let mut flags = node_util::get_combined_modifier_flags(source, node);
        let parent_kind = self.parent_of(node).map(|parent| self.kind_of(parent));
        if !matches!(
            parent_kind,
            Some(SyntaxKind::InterfaceDeclaration)
                | Some(SyntaxKind::ClassDeclaration)
                | Some(SyntaxKind::ClassExpression)
        ) && NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT)
        {
            let container = self.get_enclosing_container(node);
            let in_export_context = container.is_some_and(|container| {
                NodeFlags::from_bits(self.node_flags(container))
                    .intersects(NodeFlags::EXPORT_CONTEXT)
            });
            if in_export_context && !flags.intersects(tsrs2_types::ModifierFlags::AMBIENT) {
                flags = tsrs2_types::ModifierFlags::from_bits(
                    flags.bits() | tsrs2_types::ModifierFlags::EXPORT.bits(),
                );
            }
            flags = tsrs2_types::ModifierFlags::from_bits(
                flags.bits() | tsrs2_types::ModifierFlags::AMBIENT.bits(),
            );
        }
        tsrs2_types::ModifierFlags::from_bits(flags.bits() & flags_to_check.bits())
    }

    /// getEnclosingContainer (13841-13843): the nearest ancestor with
    /// the IsContainer bit.
    fn get_enclosing_container(&self, node: NodeId) -> Option<NodeId> {
        let source = self.binder.source_of_node(node);
        let mut current = node_util::parent_of(source, node);
        while let Some(candidate) = current {
            if tsrs2_binder::containers::get_container_flags(source, candidate)
                .intersects(tsrs2_binder::containers::ContainerFlags::IS_CONTAINER)
            {
                return Some(candidate);
            }
            current = node_util::parent_of(source, candidate);
        }
        None
    }

    /// tsc-port: checkFunctionOrConstructorSymbol @6.0.3
    /// tsc-hash: 694b31e3daa15dfa22f631c3a9d695ec5e24d8b301a85016f05603ca0e94503f
    /// tsc-span: _tsc.js:82024-82026
    ///
    /// The worker (checkFunctionOrConstructorSymbolWorker 82027-82214,
    /// hash 8732298f571dde47010af0ac55f03cf7156c4564b2f4682904e35ecc13
    /// dbdf19) runs eager per the addLazyDiagnostic decision; JSDoc
    /// overload tags are JS-only.
    pub(crate) fn check_function_or_constructor_symbol(
        &mut self,
        symbol: SymbolId,
    ) -> CheckResult2<()> {
        let flags_to_check = tsrs2_types::ModifierFlags::from_bits(
            tsrs2_types::ModifierFlags::EXPORT.bits()
                | tsrs2_types::ModifierFlags::AMBIENT.bits()
                | tsrs2_types::ModifierFlags::PRIVATE.bits()
                | tsrs2_types::ModifierFlags::PROTECTED.bits()
                | tsrs2_types::ModifierFlags::ABSTRACT.bits(),
        );
        let mut some_node_flags = tsrs2_types::ModifierFlags::NONE;
        let mut all_node_flags = flags_to_check;
        let mut some_have_question_token = false;
        let mut all_have_question_token = true;
        let mut has_overloads = false;
        let mut body_declaration: Option<NodeId> = None;
        let mut last_seen_non_ambient: Option<NodeId> = None;
        let mut previous_declaration: Option<NodeId> = None;
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let is_constructor = self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::CONSTRUCTOR);
        // Parse-recovery gate: under parse errors our recovery tree's
        // declaration/body boundaries diverge from tsc's, and the
        // body-accounting rows (2389/2391/2392/2393-family) key on
        // exactly those boundaries — contain rather than misreport.
        // (tsc emits these as plain error() even in errored files; the
        // divergence is OUR tree, not the suppression discipline.)
        if declarations
            .iter()
            .any(|&declaration| self.has_parse_diagnostics(declaration))
        {
            return Err(Unsupported::new(
                "overload band over a parse-recovery tree (declaration boundaries diverge)",
            ));
        }
        let mut duplicate_function_declaration = false;
        let mut multiple_constructor_implementation = false;
        let mut has_non_ambient_class = false;
        let mut function_declarations: Vec<NodeId> = Vec::new();
        for &node in &declarations {
            let in_ambient_context =
                NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT);
            let parent_kind = self.parent_of(node).map(|parent| self.kind_of(parent));
            let in_ambient_context_or_interface = matches!(
                parent_kind,
                Some(SyntaxKind::InterfaceDeclaration) | Some(SyntaxKind::TypeLiteral)
            ) || in_ambient_context;
            if in_ambient_context_or_interface {
                previous_declaration = None;
            }
            let node_kind = self.kind_of(node);
            if matches!(
                node_kind,
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) && !in_ambient_context
            {
                has_non_ambient_class = true;
            }
            if matches!(
                node_kind,
                SyntaxKind::FunctionDeclaration
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::MethodSignature
                    | SyntaxKind::Constructor
            ) {
                function_declarations.push(node);
                let current_node_flags = self.get_effective_declaration_flags(node, flags_to_check);
                some_node_flags = tsrs2_types::ModifierFlags::from_bits(
                    some_node_flags.bits() | current_node_flags.bits(),
                );
                all_node_flags = tsrs2_types::ModifierFlags::from_bits(
                    all_node_flags.bits() & current_node_flags.bits(),
                );
                let question = self.has_question_token(node);
                some_have_question_token |= question;
                all_have_question_token &= question;
                let source = self.binder.source_of_node(node);
                let body = node_util::body_of(source, node);
                let body_is_present =
                    body.is_some_and(|body| !node_util::node_is_missing(source, Some(body)));
                if body_is_present && body_declaration.is_some() {
                    if is_constructor {
                        multiple_constructor_implementation = true;
                    } else {
                        duplicate_function_declaration = true;
                    }
                } else if let Some(previous) = previous_declaration {
                    if self.parent_of(previous) == self.parent_of(node) {
                        let previous_end = self
                            .binder
                            .source_of_node(previous)
                            .arena
                            .node(previous)
                            .end;
                        let node_pos = source.arena.node(node).pos;
                        if previous_end != node_pos {
                            self.report_implementation_expected_error(previous, is_constructor)?;
                        }
                    }
                }
                if body_is_present {
                    if body_declaration.is_none() {
                        body_declaration = Some(node);
                    }
                } else {
                    has_overloads = true;
                }
                previous_declaration = Some(node);
                if !in_ambient_context_or_interface {
                    last_seen_non_ambient = Some(node);
                }
            }
        }
        if multiple_constructor_implementation {
            for &declaration in &function_declarations {
                self.error_at(
                    Some(declaration),
                    &diagnostics::Multiple_constructor_implementations_are_not_allowed,
                    &[],
                );
            }
        }
        if duplicate_function_declaration {
            for &declaration in &function_declarations {
                let error_node = self.name_of_node(declaration).unwrap_or(declaration);
                self.error_at(
                    Some(error_node),
                    &diagnostics::Duplicate_function_implementation,
                    &[],
                );
            }
        }
        if has_non_ambient_class
            && !is_constructor
            && self
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::FUNCTION)
        {
            let related: Vec<tsrs2_diags::RelatedInfo> = declarations
                .iter()
                .filter(|&&d| self.kind_of(d) == SyntaxKind::ClassDeclaration)
                .map(|&d| {
                    self.related_info_for_node(
                        d,
                        &diagnostics::Consider_adding_a_declare_modifier_to_this_class,
                        &[],
                    )
                })
                .collect();
            let symbol_name = self.symbol_display_name(symbol);
            for &declaration in &declarations {
                let diagnostic = match self.kind_of(declaration) {
                    SyntaxKind::ClassDeclaration => {
                        Some(&diagnostics::Class_declaration_cannot_implement_overload_list_for_0)
                    }
                    SyntaxKind::FunctionDeclaration => Some(
                        &diagnostics::Function_with_bodies_can_only_merge_with_classes_that_are_ambient,
                    ),
                    _ => None,
                };
                if let Some(diagnostic) = diagnostic {
                    let error_node = self.name_of_node(declaration).unwrap_or(declaration);
                    self.error_at_with_related(
                        Some(error_node),
                        diagnostic,
                        &[&symbol_name],
                        related.clone(),
                    );
                }
            }
        }
        if let Some(last) = last_seen_non_ambient {
            let source = self.binder.source_of_node(last);
            let body_missing = match node_util::body_of(source, last) {
                Some(body) => node_util::node_is_missing(source, Some(body)),
                None => true,
            };
            if body_missing
                && !node_util::has_syntactic_modifier(
                    source,
                    last,
                    tsrs2_types::ModifierFlags::ABSTRACT,
                )
                && !self.has_question_token(last)
            {
                self.report_implementation_expected_error(last, is_constructor)?;
            }
        }
        if has_overloads {
            self.check_flag_agreement_between_overloads(
                &declarations,
                body_declaration,
                flags_to_check,
                some_node_flags,
                all_node_flags,
            );
            self.check_question_token_agreement_between_overloads(
                &declarations,
                body_declaration,
                some_have_question_token,
                all_have_question_token,
            );
            if let Some(body_declaration) = body_declaration {
                let signatures = self.get_signatures_of_symbol(Some(symbol))?;
                let body_signature = self.get_signature_from_declaration(body_declaration)?;
                for signature in signatures {
                    if !self
                        .is_implementation_compatible_with_overload(body_signature, signature)?
                    {
                        let error_node = self.signature_of(signature).declaration;
                        let related = self.related_info_for_node(
                            body_declaration,
                            &diagnostics::The_implementation_signature_is_declared_here,
                            &[],
                        );
                        self.error_at_with_related(
                            error_node,
                            &diagnostics::This_overload_signature_is_not_compatible_with_its_implementation_signature,
                            &[],
                            vec![related],
                        );
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// reportImplementationExpectedError (82076-82123, the worker's
    /// inner fn).
    fn report_implementation_expected_error(
        &mut self,
        node: NodeId,
        is_constructor: bool,
    ) -> CheckResult2<()> {
        let source = self.binder.source_of_node(node);
        let name = self.name_of_node(node);
        if let Some(name) = name {
            if node_util::node_is_missing(source, Some(name)) {
                return Ok(());
            }
        }
        let parent = self.parent_of(node);
        let subsequent_node = parent.and_then(|parent| {
            let mut seen = false;
            let mut found = None;
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(parent), |child| {
                if seen {
                    found = Some(child);
                    return true;
                }
                seen = child == node;
                false
            });
            found
        });
        let node_end = source.arena.node(node).end;
        if let Some(subsequent) = subsequent_node {
            if source.arena.node(subsequent).pos == node_end
                && self.kind_of(subsequent) == self.kind_of(node)
            {
                let subsequent_name = self.name_of_node(subsequent);
                let error_node = subsequent_name.unwrap_or(subsequent);
                if let (Some(name), Some(subsequent_name)) = (name, subsequent_name) {
                    let both_private = self.kind_of(name) == SyntaxKind::PrivateIdentifier
                        && self.kind_of(subsequent_name) == SyntaxKind::PrivateIdentifier
                        && self.identifier_text_of(name)
                            == self.identifier_text_of(subsequent_name);
                    let both_computed_identical = self.kind_of(name)
                        == SyntaxKind::ComputedPropertyName
                        && self.kind_of(subsequent_name) == SyntaxKind::ComputedPropertyName
                        && {
                            let left = self.check_computed_property_name(name)?;
                            let right = self.check_computed_property_name(subsequent_name)?;
                            self.is_type_identical_to(left, right)?
                        };
                    let both_literal = node_util::is_property_name_literal(source, name)
                        && node_util::is_property_name_literal(source, subsequent_name)
                        && node_util::get_escaped_text_of_identifier_or_literal(source, name)
                            == node_util::get_escaped_text_of_identifier_or_literal(
                                source,
                                subsequent_name,
                            );
                    if both_private || both_computed_identical || both_literal {
                        let is_method = matches!(
                            self.kind_of(node),
                            SyntaxKind::MethodDeclaration | SyntaxKind::MethodSignature
                        );
                        let node_static = node_util::has_syntactic_modifier(
                            source,
                            node,
                            tsrs2_types::ModifierFlags::STATIC,
                        );
                        let subsequent_static = node_util::has_syntactic_modifier(
                            source,
                            subsequent,
                            tsrs2_types::ModifierFlags::STATIC,
                        );
                        if is_method && node_static != subsequent_static {
                            let diagnostic = if node_static {
                                &diagnostics::Function_overload_must_be_static
                            } else {
                                &diagnostics::Function_overload_must_not_be_static
                            };
                            self.error_at(Some(error_node), diagnostic, &[]);
                        }
                        return Ok(());
                    }
                }
                let subsequent_body = node_util::body_of(source, subsequent);
                if subsequent_body
                    .is_some_and(|body| !node_util::node_is_missing(source, Some(body)))
                {
                    let display = name
                        .map(|name| node_util::declaration_name_to_string(source, Some(name)))
                        .unwrap_or_default();
                    self.error_at(
                        Some(error_node),
                        &diagnostics::Function_implementation_name_must_be_0,
                        &[&display],
                    );
                    return Ok(());
                }
            }
        }
        let error_node = name.unwrap_or(node);
        if is_constructor {
            self.error_at(
                Some(error_node),
                &diagnostics::Constructor_implementation_is_missing,
                &[],
            );
        } else if node_util::has_syntactic_modifier(
            source,
            node,
            tsrs2_types::ModifierFlags::ABSTRACT,
        ) {
            self.error_at(
                Some(error_node),
                &diagnostics::All_declarations_of_an_abstract_method_must_be_consecutive,
                &[],
            );
        } else {
            self.error_at(
                Some(error_node),
                &diagnostics::Function_implementation_is_missing_or_not_immediately_following_the_declaration,
                &[],
            );
        }
        Ok(())
    }

    /// getCanonicalOverload (82028-82031).
    fn get_canonical_overload(
        &self,
        overloads: &[NodeId],
        implementation: Option<NodeId>,
    ) -> NodeId {
        let shares_container = implementation.is_some_and(|implementation| {
            overloads
                .first()
                .is_some_and(|&first| self.parent_of(implementation) == self.parent_of(first))
        });
        if shares_container {
            implementation.expect("shares_container implies present")
        } else {
            overloads[0]
        }
    }

    /// checkFlagAgreementBetweenOverloads (82032-82055).
    fn check_flag_agreement_between_overloads(
        &mut self,
        overloads: &[NodeId],
        implementation: Option<NodeId>,
        flags_to_check: tsrs2_types::ModifierFlags,
        some_overload_flags: tsrs2_types::ModifierFlags,
        all_overload_flags: tsrs2_types::ModifierFlags,
    ) {
        let some_but_not_all = some_overload_flags.bits() ^ all_overload_flags.bits();
        if some_but_not_all == 0 {
            return;
        }
        let canonical = self.get_canonical_overload(overloads, implementation);
        let canonical_flags = self.get_effective_declaration_flags(canonical, flags_to_check);
        // group(overloads, fileName) — first-seen file order.
        let mut file_order: Vec<usize> = Vec::new();
        let mut groups: std::collections::HashMap<usize, Vec<NodeId>> =
            std::collections::HashMap::new();
        for &overload in overloads {
            let file = self.binder.file_index_of_node(overload);
            if !groups.contains_key(&file) {
                file_order.push(file);
            }
            groups.entry(file).or_default().push(overload);
        }
        for file in file_order {
            let overloads_in_file = groups[&file].clone();
            let canonical_for_file =
                self.get_canonical_overload(&overloads_in_file, implementation);
            let canonical_flags_for_file =
                self.get_effective_declaration_flags(canonical_for_file, flags_to_check);
            for &o in &overloads_in_file {
                let flags = self.get_effective_declaration_flags(o, flags_to_check);
                let deviation = flags.bits() ^ canonical_flags.bits();
                let deviation_in_file = flags.bits() ^ canonical_flags_for_file.bits();
                let error_node = self.name_of_node(o);
                if deviation_in_file & tsrs2_types::ModifierFlags::EXPORT.bits() != 0 {
                    self.error_at(
                        error_node,
                        &diagnostics::Overload_signatures_must_all_be_exported_or_non_exported,
                        &[],
                    );
                } else if deviation_in_file & tsrs2_types::ModifierFlags::AMBIENT.bits() != 0 {
                    self.error_at(
                        error_node,
                        &diagnostics::Overload_signatures_must_all_be_ambient_or_non_ambient,
                        &[],
                    );
                } else if deviation
                    & (tsrs2_types::ModifierFlags::PRIVATE.bits()
                        | tsrs2_types::ModifierFlags::PROTECTED.bits())
                    != 0
                {
                    self.error_at(
                        error_node.or(Some(o)),
                        &diagnostics::Overload_signatures_must_all_be_public_private_or_protected,
                        &[],
                    );
                } else if deviation & tsrs2_types::ModifierFlags::ABSTRACT.bits() != 0 {
                    self.error_at(
                        error_node,
                        &diagnostics::Overload_signatures_must_all_be_abstract_or_non_abstract,
                        &[],
                    );
                }
            }
        }
    }

    /// checkQuestionTokenAgreementBetweenOverloads (82056-82066).
    fn check_question_token_agreement_between_overloads(
        &mut self,
        overloads: &[NodeId],
        implementation: Option<NodeId>,
        some_have_question_token: bool,
        all_have_question_token: bool,
    ) {
        if some_have_question_token == all_have_question_token {
            return;
        }
        let canonical = self.get_canonical_overload(overloads, implementation);
        let canonical_has_question_token = self.has_question_token(canonical);
        for &o in overloads {
            if self.has_question_token(o) != canonical_has_question_token {
                let error_node = self.name_of_node(o);
                self.error_at(
                    error_node,
                    &diagnostics::Overload_signatures_must_all_be_optional_or_required,
                    &[],
                );
            }
        }
    }

    /// tsc-port: checkExportsOnMergedDeclarations @6.0.3
    /// tsc-hash: a2c41fdcf1788e5486ad20834c57dbdf8c55e52950e0c24022870e3510559398
    /// tsc-span: _tsc.js:82215-82217
    ///
    /// The worker (82218-82311, hash 27bf5a2713904b795ff58d00bb0b7709
    /// eeed5818fb779c3dc557db69107ac931) runs eager; JSDoc tag arms
    /// are JS-only.
    pub(crate) fn check_exports_on_merged_declarations(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<()> {
        let local_symbol = self
            .binder
            .file(self.binder.file_index_of_node(node))
            .node_local_symbol
            .get(&node)
            .copied();
        let symbol = match local_symbol {
            Some(local) => local,
            None => {
                let symbol = self.get_symbol_of_declaration(node)?;
                if self.binder.symbol(symbol).export_symbol.is_none() {
                    return Ok(());
                }
                symbol
            }
        };
        if self.get_declaration_of_kind(symbol, self.kind_of(node)) != Some(node) {
            return Ok(());
        }
        let mut exported_spaces = 0u32;
        let mut non_exported_spaces = 0u32;
        let mut default_exported_spaces = 0u32;
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let export_default = tsrs2_types::ModifierFlags::from_bits(
            tsrs2_types::ModifierFlags::EXPORT.bits() | tsrs2_types::ModifierFlags::DEFAULT.bits(),
        );
        for &d in &declarations {
            let spaces = self.get_declaration_spaces(d)?;
            let flags = self.get_effective_declaration_flags(d, export_default);
            if flags.intersects(tsrs2_types::ModifierFlags::EXPORT) {
                if flags.intersects(tsrs2_types::ModifierFlags::DEFAULT) {
                    default_exported_spaces |= spaces;
                } else {
                    exported_spaces |= spaces;
                }
            } else {
                non_exported_spaces |= spaces;
            }
        }
        let non_default_exported = exported_spaces | non_exported_spaces;
        let common_exports_and_locals = exported_spaces & non_exported_spaces;
        let common_default_and_non_default = default_exported_spaces & non_default_exported;
        if common_exports_and_locals != 0 || common_default_and_non_default != 0 {
            for &d in &declarations {
                let spaces = self.get_declaration_spaces(d)?;
                let name = self.name_of_node(d);
                let display = name
                    .map(|name| {
                        node_util::declaration_name_to_string(
                            self.binder.source_of_node(name),
                            Some(name),
                        )
                    })
                    .unwrap_or_default();
                if spaces & common_default_and_non_default != 0 {
                    self.error_at(
                        name,
                        &diagnostics::Merged_declaration_0_cannot_include_a_default_export_declaration_Consider_adding_a_separate_export_default_0_declaration_instead,
                        &[&display],
                    );
                } else if spaces & common_exports_and_locals != 0 {
                    self.error_at(
                        name,
                        &diagnostics::Individual_declarations_in_merged_declaration_0_must_be_all_exported_or_all_local,
                        &[&display],
                    );
                }
            }
        }
        Ok(())
    }

    /// getDeclarationSpaces (82259-82310, the worker's inner fn).
    /// The JSDoc typedef/callback/enum-tag rows are JS-band; alias
    /// targets resolve through resolveAlias and union their target
    /// declarations' spaces (M4 5.9d).
    fn get_declaration_spaces(&mut self, decl: NodeId) -> CheckResult2<u32> {
        const EXPORT_VALUE: u32 = 1;
        const EXPORT_TYPE: u32 = 2;
        const EXPORT_NAMESPACE: u32 = 4;
        match self.kind_of(decl) {
            SyntaxKind::InterfaceDeclaration | SyntaxKind::TypeAliasDeclaration => Ok(EXPORT_TYPE),
            SyntaxKind::ModuleDeclaration => {
                let source = self.binder.source_of_node(decl);
                let instantiated = node_util::is_ambient_module(source, decl) || {
                    let mut visited = std::collections::HashMap::new();
                    tsrs2_binder::containers::get_module_instance_state(source, decl, &mut visited)
                        != tsrs2_binder::containers::ModuleInstanceState::NonInstantiated
                };
                Ok(if instantiated {
                    EXPORT_NAMESPACE | EXPORT_VALUE
                } else {
                    EXPORT_NAMESPACE
                })
            }
            SyntaxKind::ClassDeclaration | SyntaxKind::EnumDeclaration | SyntaxKind::EnumMember => {
                Ok(EXPORT_TYPE | EXPORT_VALUE)
            }
            SyntaxKind::SourceFile => Ok(EXPORT_TYPE | EXPORT_VALUE | EXPORT_NAMESPACE),
            SyntaxKind::ExportAssignment | SyntaxKind::BinaryExpression => {
                let expression = match self.data_of(decl) {
                    NodeData::ExportAssignment(data) => data.expression,
                    NodeData::BinaryExpression(data) => data.right,
                    _ => None,
                };
                let is_entity =
                    expression.is_some_and(|expression| self.is_entity_name_expression(expression));
                if !is_entity {
                    return Ok(EXPORT_VALUE);
                }
                // tsc reassigns d = expression and falls through into
                // the alias arm.
                self.declaration_spaces_of_alias_target(
                    expression.expect("entity check implies Some"),
                )
            }
            SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::ImportClause => self.declaration_spaces_of_alias_target(decl),
            SyntaxKind::VariableDeclaration
            | SyntaxKind::BindingElement
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::Identifier => Ok(EXPORT_VALUE),
            SyntaxKind::MethodSignature | SyntaxKind::PropertySignature => Ok(EXPORT_TYPE),
            _ => Err(Unsupported::new(
                "getDeclarationSpaces unexpected declaration kind (Debug.failBadSyntaxKind, parse recovery)",
            )),
        }
    }

    /// getDeclarationSpaces' shared alias arm (82287-82295): the
    /// resolved alias target's declarations union their spaces.
    fn declaration_spaces_of_alias_target(&mut self, d: NodeId) -> CheckResult2<u32> {
        let symbol = self.get_symbol_of_declaration(d)?;
        let target = self.resolve_alias(symbol)?;
        let declarations = self.binder.symbol(target).declarations.clone();
        let mut result = 0u32;
        for declaration in declarations {
            result |= self.get_declaration_spaces(declaration)?;
        }
        Ok(result)
    }

    /// tsc-port: checkTypePredicate @6.0.3
    /// tsc-hash: a9cf25130ba9b91453a845c17be5e5baea7d97023a327b90cc68d9b070610950
    /// tsc-span: _tsc.js:81206-81253
    /// (covers getTypePredicateParent 81254-81268)
    ///
    /// The 2677 assignability face carries tsc's leadingError as a
    /// containingMessageChain, and checkTypeRelatedTo wraps errorInfo
    /// under that chain UNCONDITIONALLY (64890-64896) — every relation
    /// failure path (generic 2322, no-common-properties 2559, missing
    /// property, readonly 4104) lands the SAME outer code 2677 at
    /// node.type with no arguments; the varying part is only the
    /// elided T2 chain tail. So the head-only slice needs no display
    /// rendering and none of check_type_assignable_to's head
    /// overrides here.
    pub(crate) fn check_type_predicate(&mut self, node: NodeId) -> CheckResult2<()> {
        // getTypePredicateParent (81254-81268): the seven signature
        // kinds whose return-type slot may carry a predicate.
        let parent = self.parent_of(node).filter(|&parent| {
            let parent_type = match self.data_of(parent) {
                NodeData::ArrowFunction(data) => data.r#type,
                NodeData::CallSignature(data) => data.r#type,
                NodeData::FunctionDeclaration(data) => data.r#type,
                NodeData::FunctionExpression(data) => data.r#type,
                NodeData::FunctionType(data) => data.r#type,
                NodeData::MethodDeclaration(data) => data.r#type,
                NodeData::MethodSignature(data) => data.r#type,
                _ => None,
            };
            parent_type == Some(node)
        });
        let Some(parent) = parent else {
            self.error_at(
                Some(node),
                &diagnostics::A_type_predicate_is_only_allowed_in_return_type_position_for_functions_and_methods,
                &[],
            );
            return Ok(());
        };
        let signature = self.get_signature_from_declaration(parent)?;
        let Some(type_predicate) = self.get_type_predicate_of_signature(signature)? else {
            return Ok(());
        };
        let (parameter_name_node, type_node) = match self.data_of(node) {
            NodeData::TypePredicate(data) => (data.parameter_name, data.r#type),
            _ => (None, None),
        };
        self.check_source_element(type_node);
        if matches!(
            type_predicate.kind,
            TypePredicateKind::This | TypePredicateKind::AssertsThis
        ) {
            return Ok(());
        }
        if type_predicate.parameter_index >= 0 {
            let index = type_predicate.parameter_index as usize;
            let signature_data = self.signature_of(signature);
            let has_rest = signature_data
                .flags
                .intersects(SignatureFlags::HAS_REST_PARAMETER);
            let is_last = index == signature_data.parameters.len() - 1;
            // The declared signature's own parameter list produced
            // parameter_index (createTypePredicateFromTypePredicateNode
            // findIndex), so the index is always in bounds here.
            let parameter = signature_data.parameters[index];
            if has_rest && is_last {
                self.error_at(
                    parameter_name_node,
                    &diagnostics::A_type_predicate_cannot_reference_a_rest_parameter,
                    &[],
                );
            } else if let Some(predicate_type) = type_predicate.ty {
                let parameter_type = self.get_type_of_symbol(parameter)?;
                if !self.is_type_assignable_to(predicate_type, parameter_type)? {
                    self.error_at(
                        type_node,
                        &diagnostics::A_type_predicate_s_type_must_be_assignable_to_its_parameter_s_type,
                        &[],
                    );
                }
            }
        } else if let Some(parameter_name_node) = parameter_name_node {
            let predicate_variable_name = type_predicate.parameter_name.clone().unwrap_or_default();
            let parameters = match self.data_of(parent) {
                NodeData::ArrowFunction(data) => data.parameters,
                NodeData::CallSignature(data) => data.parameters,
                NodeData::FunctionDeclaration(data) => data.parameters,
                NodeData::FunctionExpression(data) => data.parameters,
                NodeData::FunctionType(data) => data.parameters,
                NodeData::MethodDeclaration(data) => data.parameters,
                NodeData::MethodSignature(data) => data.parameters,
                _ => None,
            };
            let mut has_reported_error = false;
            for parameter in self.nodes_of(parameters) {
                let name = match self.data_of(parameter) {
                    NodeData::Parameter(data) => data.name,
                    _ => None,
                };
                let Some(name) = name else { continue };
                if matches!(
                    self.kind_of(name),
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                ) && self.check_if_type_predicate_variable_is_declared_in_binding_pattern(
                    name,
                    parameter_name_node,
                    &predicate_variable_name,
                ) {
                    has_reported_error = true;
                    break;
                }
            }
            if !has_reported_error {
                self.error_at(
                    Some(parameter_name_node),
                    &diagnostics::Cannot_find_parameter_0,
                    &[&predicate_variable_name],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: checkIfTypePredicateVariableIsDeclaredInBindingPattern @6.0.3
    /// tsc-hash: dd7f9d7772f812ac15d0104b5950068275a485f8a3a4a6837c844502a5a94a8a
    /// tsc-span: _tsc.js:81269-81288
    fn check_if_type_predicate_variable_is_declared_in_binding_pattern(
        &mut self,
        pattern: NodeId,
        predicate_variable_node: NodeId,
        predicate_variable_name: &str,
    ) -> bool {
        let elements = match self.data_of(pattern) {
            NodeData::ObjectBindingPattern(data) => data.elements,
            NodeData::ArrayBindingPattern(data) => data.elements,
            _ => None,
        };
        for element in self.nodes_of(elements) {
            if self.kind_of(element) == SyntaxKind::OmittedExpression {
                continue;
            }
            let name = match self.data_of(element) {
                NodeData::BindingElement(data) => data.name,
                _ => None,
            };
            let Some(name) = name else { continue };
            match self.kind_of(name) {
                SyntaxKind::Identifier => {
                    if self.escaped_text_of(Some(name)) == Some(predicate_variable_name) {
                        self.error_at(
                            Some(predicate_variable_node),
                            &diagnostics::A_type_predicate_cannot_reference_element_0_in_a_binding_pattern,
                            &[predicate_variable_name],
                        );
                        return true;
                    }
                }
                SyntaxKind::ArrayBindingPattern | SyntaxKind::ObjectBindingPattern => {
                    if self.check_if_type_predicate_variable_is_declared_in_binding_pattern(
                        name,
                        predicate_variable_node,
                        predicate_variable_name,
                    ) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    // ---- grammar (fn-like declaration band) ----

    /// tsc-port: checkGrammarFunctionLikeDeclaration @6.0.3
    /// tsc-hash: c9f0c6623f0bb6fdff9dac448b23075eff0bf20954d1f4acd38520ea0a84081f
    /// tsc-span: _tsc.js:89466-89469
    ///
    /// checkGrammarModifiers heads tsc's `||` ladder — a modifier
    /// grammar error suppresses EVERY follower (type-parameter list,
    /// parameter list, arrow 1200, use-strict 1347). The would-report
    /// skeleton supplies the verdict; the modifier row itself stays
    /// the M7 FN.
    pub(crate) fn check_grammar_function_like_declaration(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<bool> {
        if self.check_grammar_modifiers_would_report(node) {
            return Ok(true);
        }
        let type_parameters = match self.data_of(node) {
            NodeData::FunctionExpression(data) => data.type_parameters,
            NodeData::ArrowFunction(data) => data.type_parameters,
            NodeData::MethodDeclaration(data) => data.type_parameters,
            NodeData::MethodSignature(data) => data.type_parameters,
            NodeData::FunctionDeclaration(data) => data.type_parameters,
            NodeData::GetAccessor(data) => data.type_parameters,
            NodeData::SetAccessor(data) => data.type_parameters,
            NodeData::Constructor(data) => data.type_parameters,
            NodeData::CallSignature(data) => data.type_parameters,
            NodeData::ConstructSignature(data) => data.type_parameters,
            NodeData::FunctionType(data) => data.type_parameters,
            NodeData::ConstructorType(data) => data.type_parameters,
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

    /// tsc-port: checkGrammarForInvalidQuestionMark @6.0.3
    /// tsc-hash: ecbb4add9b568ce8b186de774851c5795ea842ace6c2c2fa9541e70f8545b8c0
    /// tsc-span: _tsc.js:89631-89633
    fn check_grammar_for_invalid_question_mark(
        &mut self,
        question_token: Option<NodeId>,
        message: &'static tsrs2_diags::DiagnosticMessage,
    ) -> bool {
        match question_token {
            Some(token) => self.grammar_error_on_node(token, message, &[]),
            None => false,
        }
    }

    /// tsc-port: checkGrammarForInvalidExclamationToken @6.0.3
    /// tsc-hash: 11b6e35393603887b71a5bea8e64697e42ca22bd858b3fe12d6d2bfe92d8ad4c
    /// tsc-span: _tsc.js:89634-89636
    fn check_grammar_for_invalid_exclamation_token(
        &mut self,
        exclamation_token: Option<NodeId>,
        message: &'static tsrs2_diags::DiagnosticMessage,
    ) -> bool {
        match exclamation_token {
            Some(token) => self.grammar_error_on_node(token, message, &[]),
            None => false,
        }
    }

    /// tsc-port: checkGrammarForInvalidDynamicName @6.0.3
    /// tsc-hash: 122fcd605b02d6e016546ac05ae47fd79514d7a92c12d6f24c061bc8a74c49a1
    /// tsc-span: _tsc.js:89938-89942
    /// (covers isNonBindableDynamicName 57646-57648)
    ///
    /// isLateBindableName's TYPE half runs through the memoizing
    /// checkComputedPropertyName; element-access names are JS-only
    /// declaration shapes (unreachable from TS member names).
    fn check_grammar_for_invalid_dynamic_name(
        &mut self,
        name: NodeId,
        message: &'static tsrs2_diags::DiagnosticMessage,
    ) -> CheckResult2<bool> {
        let source = self.binder.source_of_node(name);
        if !node_util::is_dynamic_name(source, name) {
            return Ok(false);
        }
        // isLateBindableName: AST half (entity-name expression) + type
        // half (literal/unique-symbol name type).
        let expression = match self.data_of(name) {
            NodeData::ComputedPropertyName(data) => data.expression,
            _ => None,
        };
        let is_late_bindable = match expression {
            Some(expression) if self.is_entity_name_expression(expression) => {
                let name_type = self.check_computed_property_name(name)?;
                self.property_name_from_type_usable(name_type).is_some()
            }
            _ => false,
        };
        if is_late_bindable {
            return Ok(false);
        }
        // The tsc guard also requires the non-entity-name shape:
        // `!isEntityNameExpression(node.expression)` — an entity-name
        // computed name whose TYPE half failed is dynamic but exempt.
        if expression.is_some_and(|expression| self.is_entity_name_expression(expression)) {
            return Ok(false);
        }
        Ok(self.grammar_error_on_node(name, message, &[]))
    }

    /// tsc-port: checkGrammarMethod @6.0.3
    /// tsc-hash: b527241eef4a0e9f52c7f1aba21a0c2d994426c4e9b937a0b184941346158068
    /// tsc-span: _tsc.js:89943-89977
    pub(crate) fn check_grammar_method(&mut self, node: NodeId) -> CheckResult2<bool> {
        if self.check_grammar_function_like_declaration(node)? {
            return Ok(true);
        }
        let parent = self.parent_of(node);
        let parent_kind = parent.map(|parent| self.kind_of(parent));
        let (name, question_token, exclamation_token, body, modifiers) = match self.data_of(node) {
            NodeData::MethodDeclaration(data) => (
                data.name,
                data.question_token,
                data.exclamation_token,
                data.body,
                data.modifiers,
            ),
            NodeData::MethodSignature(data) => {
                (data.name, data.question_token, None, None, data.modifiers)
            }
            _ => (None, None, None, None, None),
        };
        if self.kind_of(node) == SyntaxKind::MethodDeclaration {
            if parent_kind == Some(SyntaxKind::ObjectLiteralExpression) {
                let modifier_nodes = self.nodes_of(modifiers);
                let only_async = modifier_nodes.len() == 1
                    && modifier_nodes
                        .first()
                        .is_some_and(|&m| self.kind_of(m) == SyntaxKind::AsyncKeyword);
                if !modifier_nodes.is_empty() && !only_async {
                    return Ok(self.grammar_error_on_first_token(
                        node,
                        &diagnostics::Modifiers_cannot_appear_here,
                        &[],
                    ));
                }
                if self.check_grammar_for_invalid_question_mark(
                    question_token,
                    &diagnostics::An_object_member_cannot_be_declared_optional,
                ) {
                    return Ok(true);
                }
                if self.check_grammar_for_invalid_exclamation_token(
                    exclamation_token,
                    &diagnostics::A_definite_assignment_assertion_is_not_permitted_in_this_context,
                ) {
                    return Ok(true);
                }
                if body.is_none() {
                    let end = self.binder.source_of_node(node).arena.node(node).end;
                    return Ok(self.grammar_error_at_pos(
                        node,
                        end.saturating_sub(1),
                        1,
                        &diagnostics::_0_expected,
                        &["{"],
                    ));
                }
            }
            if self.check_grammar_for_generator(node) {
                return Ok(true);
            }
        }
        let Some(name) = name else {
            return Ok(false);
        };
        if matches!(
            parent_kind,
            Some(SyntaxKind::ClassDeclaration) | Some(SyntaxKind::ClassExpression)
        ) {
            if self.options.emit_script_target() < tsrs2_types::ScriptTarget::ES2015
                && self.kind_of(name) == SyntaxKind::PrivateIdentifier
            {
                return Ok(self.grammar_error_on_node(
                    name,
                    &diagnostics::Private_identifiers_are_only_available_when_targeting_ECMAScript_2015_and_higher,
                    &[],
                ));
            }
            if NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT) {
                return self.check_grammar_for_invalid_dynamic_name(
                    name,
                    &diagnostics::A_computed_property_name_in_an_ambient_context_must_refer_to_an_expression_whose_type_is_a_literal_type_or_a_unique_symbol_type,
                );
            }
            if self.kind_of(node) == SyntaxKind::MethodDeclaration && body.is_none() {
                return self.check_grammar_for_invalid_dynamic_name(
                    name,
                    &diagnostics::A_computed_property_name_in_a_method_overload_must_refer_to_an_expression_whose_type_is_a_literal_type_or_a_unique_symbol_type,
                );
            }
        } else if parent_kind == Some(SyntaxKind::InterfaceDeclaration) {
            return self.check_grammar_for_invalid_dynamic_name(
                name,
                &diagnostics::A_computed_property_name_in_an_interface_must_refer_to_an_expression_whose_type_is_a_literal_type_or_a_unique_symbol_type,
            );
        } else if parent_kind == Some(SyntaxKind::TypeLiteral) {
            return self.check_grammar_for_invalid_dynamic_name(
                name,
                &diagnostics::A_computed_property_name_in_a_type_literal_must_refer_to_an_expression_whose_type_is_a_literal_type_or_a_unique_symbol_type,
            );
        }
        Ok(false)
    }

    /// tsc-port: checkGrammarProperty @6.0.3
    /// tsc-hash: 96ee5a1ca0e98d408af24ce8c6f1d49184809528811cdd2d86069fdba2ee41f7
    /// tsc-span: _tsc.js:90262-90306
    pub(crate) fn check_grammar_property(&mut self, node: NodeId) -> CheckResult2<bool> {
        let (name, question_token, exclamation_token, type_node, initializer) =
            match self.data_of(node) {
                NodeData::PropertyDeclaration(data) => (
                    data.name,
                    data.question_token,
                    data.exclamation_token,
                    data.r#type,
                    data.initializer,
                ),
                NodeData::PropertySignature(data) => (
                    data.name,
                    data.question_token,
                    None,
                    data.r#type,
                    data.initializer,
                ),
                _ => (None, None, None, None, None),
            };
        let _ = question_token;
        let Some(name) = name else {
            return Ok(false);
        };
        let parent = self.parent_of(node);
        let parent_kind = parent.map(|parent| self.kind_of(parent));
        // The mapped-type `in`-name row (90263-90265) targets
        // node.parent.members[0] — reachable only through parse
        // recovery of `{ [K in T]: ... }` inside a class/interface.
        if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
            if let NodeData::ComputedPropertyName(data) = self.data_of(name) {
                if let Some(expression) = data.expression {
                    if self.kind_of(expression) == SyntaxKind::BinaryExpression {
                        let is_in = matches!(
                            self.data_of(expression),
                            NodeData::BinaryExpression(bin)
                                if bin.operator_token.is_some_and(
                                    |token| self.kind_of(token) == SyntaxKind::InKeyword
                                )
                        );
                        if is_in {
                            let first_member = parent.and_then(|parent| {
                                let members = match self.data_of(parent) {
                                    NodeData::ClassDeclaration(data) => data.members,
                                    NodeData::ClassExpression(data) => data.members,
                                    NodeData::InterfaceDeclaration(data) => data.members,
                                    NodeData::TypeLiteral(data) => data.members,
                                    _ => None,
                                };
                                self.nodes_of(members).first().copied()
                            });
                            if let Some(first_member) = first_member {
                                return Ok(self.grammar_error_on_node(
                                    first_member,
                                    &diagnostics::A_mapped_type_may_not_declare_properties_or_methods,
                                    &[],
                                ));
                            }
                        }
                    }
                }
            }
        }
        if matches!(
            parent_kind,
            Some(SyntaxKind::ClassDeclaration) | Some(SyntaxKind::ClassExpression)
        ) {
            let string_constructor_name = self.kind_of(name) == SyntaxKind::StringLiteral
                && matches!(
                    self.data_of(name),
                    NodeData::StringLiteral(data) if data.text == "constructor"
                );
            if string_constructor_name {
                return Ok(self.grammar_error_on_node(
                    name,
                    &diagnostics::Classes_may_not_have_a_field_named_constructor,
                    &[],
                ));
            }
            if self.check_grammar_for_invalid_dynamic_name(
                name,
                &diagnostics::A_computed_property_name_in_a_class_property_declaration_must_have_a_simple_literal_type_or_a_unique_symbol_type,
            )? {
                return Ok(true);
            }
            let source = self.binder.source_of_node(node);
            if self.options.emit_script_target() < tsrs2_types::ScriptTarget::ES2015 {
                if self.kind_of(name) == SyntaxKind::PrivateIdentifier {
                    return Ok(self.grammar_error_on_node(
                        name,
                        &diagnostics::Private_identifiers_are_only_available_when_targeting_ECMAScript_2015_and_higher,
                        &[],
                    ));
                }
                if node_util::is_auto_accessor_property_declaration(source, node)
                    && !NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT)
                {
                    return Ok(self.grammar_error_on_node(
                        name,
                        &diagnostics::Properties_with_the_accessor_modifier_are_only_available_when_targeting_ECMAScript_2015_and_higher,
                        &[],
                    ));
                }
            }
            if node_util::is_auto_accessor_property_declaration(source, node)
                && self.check_grammar_for_invalid_question_mark(
                    question_token,
                    &diagnostics::An_accessor_property_cannot_be_declared_optional,
                )
            {
                return Ok(true);
            }
        } else if parent_kind == Some(SyntaxKind::InterfaceDeclaration) {
            if self.check_grammar_for_invalid_dynamic_name(
                name,
                &diagnostics::A_computed_property_name_in_an_interface_must_refer_to_an_expression_whose_type_is_a_literal_type_or_a_unique_symbol_type,
            )? {
                return Ok(true);
            }
            if let Some(initializer) = initializer {
                return Ok(self.grammar_error_on_node(
                    initializer,
                    &diagnostics::An_interface_property_cannot_have_an_initializer,
                    &[],
                ));
            }
        } else if parent_kind == Some(SyntaxKind::TypeLiteral) {
            if self.check_grammar_for_invalid_dynamic_name(
                name,
                &diagnostics::A_computed_property_name_in_a_type_literal_must_refer_to_an_expression_whose_type_is_a_literal_type_or_a_unique_symbol_type,
            )? {
                return Ok(true);
            }
            if let Some(initializer) = initializer {
                return Ok(self.grammar_error_on_node(
                    initializer,
                    &diagnostics::A_type_literal_property_cannot_have_an_initializer,
                    &[],
                ));
            }
        }
        if NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT) {
            // tsc reaches this through `!checkGrammarModifiers(node) &&
            // !checkGrammarProperty(node)` — a decorator on an ambient
            // property reports 1206 in checkGrammarModifiers (M7 stub
            // here) and SHORT-CIRCUITS this row; contain that shape
            // rather than fabricate 1039 beside tsc's 1206.
            let has_decorator = {
                let modifiers = match self.data_of(node) {
                    NodeData::PropertyDeclaration(data) => data.modifiers,
                    NodeData::PropertySignature(data) => data.modifiers,
                    _ => None,
                };
                self.nodes_of(modifiers)
                    .iter()
                    .any(|&modifier| self.kind_of(modifier) == SyntaxKind::Decorator)
            };
            if has_decorator {
                return Err(Unsupported::new(
                    "ambient-initializer row behind checkGrammarModifiers' decorator rows (M7)",
                ));
            }
            self.check_ambient_initializer(node)?;
        }
        if self.kind_of(node) == SyntaxKind::PropertyDeclaration {
            if let Some(exclamation_token) = exclamation_token {
                let source = self.binder.source_of_node(node);
                let is_class_parent = matches!(
                    parent_kind,
                    Some(SyntaxKind::ClassDeclaration) | Some(SyntaxKind::ClassExpression)
                );
                let is_static = node_util::has_syntactic_modifier(
                    source,
                    node,
                    tsrs2_types::ModifierFlags::STATIC,
                );
                let has_abstract = node_util::has_syntactic_modifier(
                    source,
                    node,
                    tsrs2_types::ModifierFlags::ABSTRACT,
                );
                let ambient =
                    NodeFlags::from_bits(self.node_flags(node)).intersects(NodeFlags::AMBIENT);
                if !is_class_parent
                    || type_node.is_none()
                    || initializer.is_some()
                    || ambient
                    || is_static
                    || has_abstract
                {
                    let message = if initializer.is_some() {
                        &diagnostics::Declarations_with_initializers_cannot_also_have_definite_assignment_assertions
                    } else if type_node.is_none() {
                        &diagnostics::Declarations_with_definite_assignment_assertions_must_also_have_type_annotations
                    } else {
                        &diagnostics::A_definite_assignment_assertion_is_not_permitted_in_this_context
                    };
                    return Ok(self.grammar_error_on_node(exclamation_token, message, &[]));
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: checkGrammarAccessor @6.0.3
    /// tsc-hash: 450d5e7cd9f84c9e80f00b3a4cb4181f267550bf59e6691590ac5a1e8346d4a8
    /// tsc-span: _tsc.js:89843-89885
    /// (covers doesAccessorHaveCorrectParameterCount 89886-89888 +
    /// getAccessorThisParameter 89889-89893)
    pub(crate) fn check_grammar_accessor(&mut self, accessor: NodeId) -> CheckResult2<bool> {
        let is_get = self.kind_of(accessor) == SyntaxKind::GetAccessor;
        let (name, type_parameters, parameters, type_node, body) = match self.data_of(accessor) {
            NodeData::GetAccessor(data) => (
                data.name,
                data.type_parameters,
                data.parameters,
                data.r#type,
                data.body,
            ),
            NodeData::SetAccessor(data) => (
                data.name,
                data.type_parameters,
                data.parameters,
                data.r#type,
                data.body,
            ),
            _ => (None, None, None, None, None),
        };
        let Some(name) = name else {
            return Ok(false);
        };
        let parent_kind = self.parent_of(accessor).map(|parent| self.kind_of(parent));
        // tsc's parser stamps the Ambient NODE flag from a `declare`
        // member modifier; ours reaches it through the modifier — read
        // both faces.
        let ambient = NodeFlags::from_bits(self.node_flags(accessor))
            .intersects(NodeFlags::AMBIENT)
            || node_util::has_syntactic_modifier(
                self.binder.source_of_node(accessor),
                accessor,
                tsrs2_types::ModifierFlags::AMBIENT,
            );
        let in_type_container = matches!(
            parent_kind,
            Some(SyntaxKind::TypeLiteral) | Some(SyntaxKind::InterfaceDeclaration)
        );
        let source = self.binder.source_of_node(accessor);
        let has_abstract = node_util::has_syntactic_modifier(
            source,
            accessor,
            tsrs2_types::ModifierFlags::ABSTRACT,
        );
        if !ambient && !in_type_container {
            if self.options.emit_script_target() < tsrs2_types::ScriptTarget::ES2015
                && self.kind_of(name) == SyntaxKind::PrivateIdentifier
            {
                return Ok(self.grammar_error_on_node(
                    name,
                    &diagnostics::Private_identifiers_are_only_available_when_targeting_ECMAScript_2015_and_higher,
                    &[],
                ));
            }
            if body.is_none() && !has_abstract {
                let end = source.arena.node(accessor).end;
                return Ok(self.grammar_error_at_pos(
                    accessor,
                    end.saturating_sub(1),
                    1,
                    &diagnostics::_0_expected,
                    &["{"],
                ));
            }
        }
        if let Some(body) = body {
            if has_abstract {
                return Ok(self.grammar_error_on_node(
                    accessor,
                    &diagnostics::An_abstract_accessor_cannot_have_an_implementation,
                    &[],
                ));
            }
            if in_type_container {
                return Ok(self.grammar_error_on_node(
                    body,
                    &diagnostics::An_implementation_cannot_be_declared_in_ambient_contexts,
                    &[],
                ));
            }
        }
        if type_parameters.is_some() {
            return Ok(self.grammar_error_on_node(
                name,
                &diagnostics::An_accessor_cannot_have_type_parameters,
                &[],
            ));
        }
        let parameter_nodes = self.nodes_of(parameters);
        let this_parameter = self.get_accessor_this_parameter(accessor, &parameter_nodes, is_get);
        let correct_count =
            this_parameter.is_some() || parameter_nodes.len() == if is_get { 0 } else { 1 };
        if !correct_count {
            return Ok(self.grammar_error_on_node(
                name,
                if is_get {
                    &diagnostics::A_get_accessor_cannot_have_parameters
                } else {
                    &diagnostics::A_set_accessor_must_have_exactly_one_parameter
                },
                &[],
            ));
        }
        if !is_get {
            if let Some(type_node) = type_node {
                let _ = type_node;
                return Ok(self.grammar_error_on_node(
                    name,
                    &diagnostics::A_set_accessor_cannot_have_a_return_type_annotation,
                    &[],
                ));
            }
            // getSetAccessorValueParameter: skip a leading `this`.
            let value_parameter = if this_parameter.is_some() {
                parameter_nodes.get(1).copied()
            } else {
                parameter_nodes.first().copied()
            };
            if let Some(parameter) = value_parameter {
                let (dot_dot_dot_token, parameter_question_token, parameter_initializer) =
                    match self.data_of(parameter) {
                        NodeData::Parameter(data) => (
                            data.dot_dot_dot_token,
                            data.question_token,
                            data.initializer,
                        ),
                        _ => (None, None, None),
                    };
                if let Some(dot_dot_dot_token) = dot_dot_dot_token {
                    return Ok(self.grammar_error_on_node(
                        dot_dot_dot_token,
                        &diagnostics::A_set_accessor_cannot_have_rest_parameter,
                        &[],
                    ));
                }
                if let Some(parameter_question_token) = parameter_question_token {
                    return Ok(self.grammar_error_on_node(
                        parameter_question_token,
                        &diagnostics::A_set_accessor_cannot_have_an_optional_parameter,
                        &[],
                    ));
                }
                if parameter_initializer.is_some() {
                    return Ok(self.grammar_error_on_node(
                        name,
                        &diagnostics::A_set_accessor_parameter_cannot_have_an_initializer,
                        &[],
                    ));
                }
            }
        }
        Ok(false)
    }

    /// getAccessorThisParameter (89889-89893): only when the count
    /// matches get=1/set=2 exactly does the leading `this` parameter
    /// count as one.
    fn get_accessor_this_parameter(
        &self,
        _accessor: NodeId,
        parameters: &[NodeId],
        is_get: bool,
    ) -> Option<NodeId> {
        if parameters.len() != if is_get { 1 } else { 2 } {
            return None;
        }
        let first = *parameters.first()?;
        let name = match self.data_of(first) {
            NodeData::Parameter(data) => data.name?,
            _ => return None,
        };
        (self.kind_of(name) == SyntaxKind::Identifier
            && self.identifier_text_of(name) == Some("this"))
        .then_some(first)
    }

    /// tsc-port: checkGrammarIndexSignature @6.0.3
    /// tsc-hash: 2856d1455aeec93ef47683be871684d5298f850b1d5b3361f0477c67511d9c26
    /// tsc-span: _tsc.js:89525-89527
    /// (covers checkGrammarIndexSignatureParameters 89488-89524)
    pub(crate) fn check_grammar_index_signature(&mut self, node: NodeId) -> CheckResult2<bool> {
        self.check_grammar_modifiers(node);
        let (parameters_array, type_node) = match self.data_of(node) {
            NodeData::IndexSignature(data) => (data.parameters, data.r#type),
            _ => (None, None),
        };
        let parameters = self.nodes_of(parameters_array);
        let parameter = parameters.first().copied();
        if parameters.len() != 1 {
            return Ok(match parameter {
                Some(parameter) => {
                    let parameter_name = self.name_of_node(parameter).unwrap_or(parameter);
                    self.grammar_error_on_node(
                        parameter_name,
                        &diagnostics::An_index_signature_must_have_exactly_one_parameter,
                        &[],
                    )
                }
                None => self.grammar_error_on_node(
                    node,
                    &diagnostics::An_index_signature_must_have_exactly_one_parameter,
                    &[],
                ),
            });
        }
        self.check_grammar_for_disallowed_trailing_comma(
            parameters_array,
            &diagnostics::An_index_signature_cannot_have_a_trailing_comma,
        );
        let parameter = parameter.expect("length checked above");
        let (dot_dot_dot_token, parameter_name, question_token, initializer, parameter_type) =
            match self.data_of(parameter) {
                NodeData::Parameter(data) => (
                    data.dot_dot_dot_token,
                    data.name,
                    data.question_token,
                    data.initializer,
                    data.r#type,
                ),
                _ => (None, None, None, None, None),
            };
        let error_name = parameter_name.unwrap_or(parameter);
        if let Some(dot_dot_dot_token) = dot_dot_dot_token {
            return Ok(self.grammar_error_on_node(
                dot_dot_dot_token,
                &diagnostics::An_index_signature_cannot_have_a_rest_parameter,
                &[],
            ));
        }
        let source = self.binder.source_of_node(parameter);
        // hasEffectiveModifiers = getEffectiveModifierFlags != None
        // (16922); JSDoc modifiers are JS-only, so the syntactic
        // flags ARE the effective flags here.
        if node_util::get_syntactic_modifier_flags(source, parameter)
            != tsrs2_types::ModifierFlags::NONE
        {
            return Ok(self.grammar_error_on_node(
                error_name,
                &diagnostics::An_index_signature_parameter_cannot_have_an_accessibility_modifier,
                &[],
            ));
        }
        if let Some(question_token) = question_token {
            return Ok(self.grammar_error_on_node(
                question_token,
                &diagnostics::An_index_signature_parameter_cannot_have_a_question_mark,
                &[],
            ));
        }
        if initializer.is_some() {
            return Ok(self.grammar_error_on_node(
                error_name,
                &diagnostics::An_index_signature_parameter_cannot_have_an_initializer,
                &[],
            ));
        }
        let Some(parameter_type) = parameter_type else {
            return Ok(self.grammar_error_on_node(
                error_name,
                &diagnostics::An_index_signature_parameter_must_have_a_type_annotation,
                &[],
            ));
        };
        let ty = self.get_type_from_type_node(parameter_type)?;
        let literal_or_unique = self.some_type(ty, |state, t| {
            state.tables.flags_of(t).intersects(TypeFlags::from_bits(
                TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE.bits(),
            ))
        });
        if literal_or_unique || self.is_generic_type(ty)? {
            return Ok(self.grammar_error_on_node(
                error_name,
                &diagnostics::An_index_signature_parameter_type_cannot_be_a_literal_type_or_generic_type_Consider_using_a_mapped_object_type_instead,
                &[],
            ));
        }
        let mut every_valid_index_key = true;
        for member in self.union_members_or_self(ty) {
            if !self.is_valid_index_key_type(member)? {
                every_valid_index_key = false;
                break;
            }
        }
        if !every_valid_index_key {
            return Ok(self.grammar_error_on_node(
                error_name,
                &diagnostics::An_index_signature_parameter_type_must_be_string_number_symbol_or_a_template_literal_type,
                &[],
            ));
        }
        if type_node.is_none() {
            return Ok(self.grammar_error_on_node(
                node,
                &diagnostics::An_index_signature_must_have_a_type_annotation,
                &[],
            ));
        }
        Ok(false)
    }

    /// tsc-port: checkGrammarConstructorTypeParameters @6.0.3
    /// tsc-hash: 86991bda286301e58591072483ca8f83b143fdff37cdd172320489b4336e5785
    /// tsc-span: _tsc.js:90248-90255
    ///
    /// The JSDoc type-parameter arm is JS-only (elided).
    pub(crate) fn check_grammar_constructor_type_parameters(&mut self, node: NodeId) -> bool {
        let type_parameters = match self.data_of(node) {
            NodeData::Constructor(data) => data.type_parameters,
            _ => None,
        };
        let Some(type_parameters) = type_parameters else {
            return false;
        };
        let source = self.binder.source_of_node(node);
        let array = source.arena.node_array(type_parameters);
        let (range_pos, range_end) = (array.pos as usize, array.end as usize);
        let pos = if range_pos == range_end {
            range_pos
        } else {
            tsrs2_syntax::skip_trivia(&source.text, range_pos)
        };
        self.grammar_error_at_pos(
            node,
            pos as u32,
            range_end.saturating_sub(pos) as u32,
            &diagnostics::Type_parameters_cannot_appear_on_a_constructor_declaration,
            &[],
        )
    }

    /// tsc-port: checkGrammarConstructorTypeAnnotation @6.0.3
    /// tsc-hash: 8ec606d58f086731a46ab06d3c090b5488fe0b3589ec21678e9a14c0145fa26f
    /// tsc-span: _tsc.js:90256-90261
    pub(crate) fn check_grammar_constructor_type_annotation(&mut self, node: NodeId) -> bool {
        let type_node = match self.data_of(node) {
            NodeData::Constructor(data) => data.r#type,
            _ => None,
        };
        match type_node {
            Some(type_node) => self.grammar_error_on_node(
                type_node,
                &diagnostics::Type_annotation_cannot_appear_on_a_constructor_declaration,
                &[],
            ),
            None => false,
        }
    }

    /// tsc-port: checkGrammarTypeParameterList @6.0.3
    /// tsc-hash: b38e4c29614faf900e080dc6c95f69705bbeab6d9fba40536784f9a966c176e3
    /// tsc-span: _tsc.js:89407-89414
    pub(crate) fn check_grammar_type_parameter_list(
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
        // 89447: languageVersion >= ES2016 only.
        if self.options.emit_script_target() < tsrs2_types::ScriptTarget::ES2016 {
            return false;
        }
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
            let expression = statement_data.expression?;
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

    /// getEffectiveReturnTypeNode (16768, the non-JSDoc face):
    /// `node.type` of ANY signature declaration — tsc reads the slot
    /// generically, so every kind that carries one answers (the
    /// FunctionType/signature-member arms were missing until 6.6c:
    /// a signature whose .declaration is a TYPE node — a never-typed
    /// callable parameter — hid its annotation from the effects
    /// consult, the 2366-FP face the f12 pin holds).
    pub(crate) fn effective_return_type_node(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::FunctionExpression(data) => data.r#type,
            NodeData::ArrowFunction(data) => data.r#type,
            NodeData::MethodDeclaration(data) => data.r#type,
            NodeData::FunctionDeclaration(data) => data.r#type,
            NodeData::GetAccessor(data) => data.r#type,
            NodeData::SetAccessor(data) => data.r#type,
            NodeData::Constructor(data) => data.r#type,
            NodeData::FunctionType(data) => data.r#type,
            NodeData::ConstructorType(data) => data.r#type,
            NodeData::CallSignature(data) => data.r#type,
            NodeData::ConstructSignature(data) => data.r#type,
            NodeData::MethodSignature(data) => data.r#type,
            NodeData::IndexSignature(data) => data.r#type,
            _ => None,
        }
    }

    /// tsc-port: widenTypeInferredFromInitializer @6.0.3
    /// tsc-hash: fe71fba645cae40dc0d8f96f156ac4f0795719f4d9cbeb9760e31c161f9cea30
    /// tsc-span: _tsc.js:80690-80702
    ///
    /// The Constant/readonly guard keeps the literal (5.6: routed
    /// through getWidenedLiteralTypeForInitializer so isDeclaration-
    /// Readonly participates).
    pub(crate) fn widen_type_inferred_from_initializer(
        &mut self,
        declaration: NodeId,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        let widened = self.get_widened_literal_type_for_initializer(declaration, ty)?;
        if self.is_in_js_file(declaration) {
            if self.is_empty_literal_type(widened) {
                let any = self.tables.intrinsics.any;
                self.report_implicit_any(declaration, any, None)?;
                return Ok(any);
            }
            if self.is_empty_array_literal_type(widened)? {
                let any_array = self.any_array_type()?;
                self.report_implicit_any(declaration, any_array, None)?;
                return Ok(any_array);
            }
        }
        Ok(widened)
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
    pub(crate) fn is_parameter_or_mutable_local_variable(&self, symbol: SymbolId) -> bool {
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

    /// tsc-port: isEffectiveExternalModule @6.0.3
    /// tsc-hash: faeb969f19783953861ac4d20410d7c78928ef45e6ec39f2f27231d2d0acfc33
    /// tsc-span: _tsc.js:13756-13758
    ///
    /// The commonJsModuleIndicator half is JS-only; TS files answer by
    /// the parser's external-module indicator.
    pub(crate) fn is_effective_external_module(&self, node: NodeId) -> bool {
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
        if !self.diagnostics.contains(&diagnostic) {
            self.diagnostics.push(diagnostic);
        }
    }

    /// tsc-port: grammarErrorAtPos @6.0.3
    /// tsc-hash: 6fbd3a708a6c4276e6337b6db010e4a4dcb92cd0d236abf9f538680414e2603e
    /// tsc-span: _tsc.js:90224-90231
    ///
    /// Explicit-span grammar error, gated on the file having NO parse
    /// diagnostics. Span arguments are UTF-16 units.
    pub(crate) fn grammar_error_at_pos(
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

    /// tsrs-native: createAnonymousType(symbol, emptySymbols,
    /// [signature], [], []) shorthand — the returnOnlyType shape
    /// (79138); tsc inlines the call at each site.
    pub(crate) fn create_single_signature_anonymous_type(
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
    /// Array patterns run the §4 iteration protocol (5.8b).
    /// getFlowTypeOfDestructuring is live since 6.4b (flow.rs).
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
                ty = self.get_flow_type_of_destructuring(declaration, declared)?;
            }
        } else {
            // 55984-55996: the array-pattern arm — Destructuring use,
            // PossiblyOutOfBounds only for non-rest elements.
            let NodeData::BindingElement(data) = self.data_of(declaration).clone() else {
                return Err(Unsupported::new(
                    "malformed binding element (parse recovery)",
                ));
            };
            let use_ = if data.dot_dot_dot_token.is_some() {
                tsrs2_types::IterationUse::DESTRUCTURING
            } else {
                tsrs2_types::IterationUse::from_bits(
                    tsrs2_types::IterationUse::DESTRUCTURING.bits()
                        | tsrs2_types::IterationUse::POSSIBLY_OUT_OF_BOUNDS.bits(),
                )
            };
            let undefined_type = self.tables.intrinsics.undefined;
            let element_type = self.check_iterated_type_or_element_type(
                use_,
                parent_type,
                undefined_type,
                Some(pattern),
            )?;
            let elements = match self.data_of(pattern) {
                NodeData::ArrayBindingPattern(pattern_data) => self.nodes_of(pattern_data.elements),
                _ => Vec::new(),
            };
            let index = elements
                .iter()
                .position(|&element| element == declaration)
                .ok_or_else(|| {
                    Unsupported::new("binding element outside its pattern (parse recovery)")
                })?;
            if data.dot_dot_dot_token.is_some() {
                let base_constraint = self.map_type(
                    parent_type,
                    &mut |state, t| {
                        Ok(Some(
                            if state
                                .tables
                                .flags_of(t)
                                .intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE)
                            {
                                state.get_base_constraint_or_type(t)?
                            } else {
                                t
                            },
                        ))
                    },
                    false,
                )?;
                let base_constraint = base_constraint.expect("mapper never returns None");
                let all_tuples =
                    self.every_type(base_constraint, |state, t| state.tables.is_tuple_type(t));
                ty = if all_tuples {
                    let sliced = self.map_type(
                        base_constraint,
                        &mut |state, t| state.slice_tuple_type(t, index, 0).map(Some),
                        false,
                    )?;
                    sliced.expect("mapper never returns None")
                } else {
                    self.create_array_type(element_type, false)?
                };
            } else if self.is_array_like_type(parent_type)? {
                let index_type = self.tables.get_number_literal_type(index as f64);
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
                        data.name,
                        None,
                        None,
                    )?
                    .unwrap_or(self.tables.intrinsics.error);
                ty = self.get_flow_type_of_destructuring(declaration, declared)?;
            } else {
                ty = element_type;
            }
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
    pub(crate) fn get_non_undefined_type(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
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

    /// tsc-port: getTypeOfInitializer @6.0.3
    /// tsc-hash: a634e86b085e2c5bdf1ddba28241453f81b9b4c70d742c9a589fbe2b54d6dafc
    /// tsc-span: _tsc.js:69889-69892
    pub(crate) fn get_type_of_initializer(&mut self, node: NodeId) -> CheckResult2<TypeId> {
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
    #[test]
    fn setter_return_annotation_feeds_the_bare_return_7030() {
        // getEffectiveReturnTypeNode reads a set accessor's parsed
        // (grammatically-illegal, 1095) annotation generically
        // (16768) — the bare-return face still consults it (6.6
        // review D2; oracle-pinned vs vendored tsc 6.0.3 noLib).
        let options = CompilerOptions {
            no_implicit_returns: Some(true),
            strict_null_checks: Some(false),
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_rows_with(
                "class C { set p(v: number): number { if (v) { return; } } }\n",
                &options
            ),
            [(1095, 14, 1), (7030, 46, 6)]
        );
    }

    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        checked_rows_with(text, &CompilerOptions::default())
    }

    fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], options, |state| {
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

    // ---- checkTypePredicate tail (M5 close; rows oracle-pinned vs
    // vendored tsc 6.0.3 noLib per shape, 2026-07-19) ----

    #[test]
    fn type_predicate_type_must_be_assignable_to_its_parameter() {
        // 2677 at node.type; the chain tail is elided (T2).
        assert_eq!(
            checked_rows("function f(x: string): x is number {\n    return true;\n}\n"),
            [(2677, 28, 6)]
        );
        // The containingMessageChain wrap (64890-64896) folds the
        // no-common-properties face under the SAME 2677 head — never
        // a bare 2559.
        assert_eq!(
            checked_rows("declare function w(x: { a(): void }): x is { b?: number };\n"),
            [(2677, 43, 14)]
        );
        // Width subtyping runs predicate→parameter: extra predicate
        // members are fine.
        assert_eq!(
            checked_rows("declare function m(x: { a: number }): x is { a: number, b: number };\n"),
            []
        );
        assert_eq!(
            checked_rows("declare function ok(x: number | string): x is string;\n"),
            []
        );
        // asserts-identifier predicates take the same tail; a bare
        // asserts (no type) checks nothing.
        assert_eq!(
            checked_rows("declare function a1(x: string): asserts x is number;\n"),
            [(2677, 45, 6)]
        );
        assert_eq!(
            checked_rows("declare function a2(x: string): asserts x;\n"),
            []
        );
        // This/AssertsThis kinds skip the identifier tail entirely.
        assert_eq!(
            checked_rows("class C { m(): this is C { return true; } }\n"),
            []
        );
        // MethodSignature and FunctionType parents reach the same
        // check (getTypePredicateParent kinds).
        assert_eq!(
            checked_rows("interface I { p(x: string): x is number; }\n"),
            [(2677, 33, 6)]
        );
        assert_eq!(
            checked_rows("let ft: (x: string) => x is number;\n"),
            [(2677, 28, 6)]
        );
    }

    #[test]
    fn type_predicate_parameter_reference_errors() {
        // 1229: the predicate references the rest parameter itself.
        assert_eq!(
            checked_rows("declare function b4(...a: any[]): a is number;\n"),
            [(1229, 34, 1)]
        );
        // A rest parameter elsewhere in the list doesn't gate the
        // named parameter's assignability face.
        assert_eq!(
            checked_rows("declare function r(x: string, ...rest: any[]): x is number;\n"),
            [(2677, 52, 6)]
        );
        // 1225: no parameter of that name.
        assert_eq!(
            checked_rows("declare function h(y: string): x is number;\n"),
            [(1225, 31, 1)]
        );
        // 1230: the name lives inside a binding pattern (object,
        // nested, and the no-match fallback to 1225).
        assert_eq!(
            checked_rows("declare function b5({ a, b, p1 }: any, p2: any): p1 is number;\n"),
            [(1230, 49, 2)]
        );
        assert_eq!(
            checked_rows("declare function b7({ a, c: { p1 } }: any, p2: any): p1 is number;\n"),
            [(1230, 53, 2)]
        );
        assert_eq!(
            checked_rows("declare function b8({ a, b }: any, p2: any): q is number;\n"),
            [(1225, 45, 1)]
        );
    }

    // ---- implicit returns (6.6c; rows oracle-pinned vs vendored
    // tsc 6.0.3 noLib per shape, 2026-07-19) ----

    #[test]
    fn reachable_end_in_non_void_function_reports_the_ladder() {
        // 2355: declared non-void, no explicit return, end reachable.
        assert_eq!(checked_rows("function f(): number { }\n"), [(2355, 14, 6)]);
        // 2534: declared never with a reachable end point.
        assert_eq!(checked_rows("function f(): never { }\n"), [(2534, 14, 5)]);
        // 2366: strictNullChecks (TS6 default-on) + explicit return
        // present but end still reachable.
        assert_eq!(
            checked_rows("function f(x: boolean): number { if (x) return 1; }\n"),
            [(2366, 24, 6)]
        );
        // A throw-terminated body has an unreachable end — clean.
        assert_eq!(checked_rows("function f(): number { throw 1; }\n"), []);
    }

    #[test]
    fn reachability_refinements_suppress_implicit_return_reports() {
        // The three checker-side refinements the bind-time flag could
        // not see (the retired [FLOW M5] switch/call gate's faces).
        // Never-returning call (getEffectsSignature):
        assert_eq!(
            checked_rows("declare function fail(): never;\nfunction f(): number { fail(); }\n"),
            []
        );
        // Exhaustive switch (SwitchClause clauseStart==clauseEnd +
        // isExhaustiveSwitchStatement):
        assert_eq!(
            checked_rows(
                "function f(x: 1 | 2): number { switch (x) { case 1: return 1; case 2: return 2; } }\n"
            ),
            []
        );
        // Non-exhaustive control: the suppression must NOT over-fire.
        assert_eq!(
            checked_rows("function f(x: 1 | 2): number { switch (x) { case 1: return 1; } }\n"),
            [(2366, 22, 6)]
        );
        // asserts-false argument (isFalseExpression):
        assert_eq!(
            checked_rows(
                "declare function assert(v: boolean): asserts v;\nfunction f(): number { assert(false); }\n"
            ),
            []
        );
    }

    #[test]
    fn no_implicit_returns_arms_report_7030() {
        let options = CompilerOptions {
            no_implicit_returns: Some(true),
            ..CompilerOptions::default()
        };
        // Annotation-less: inferred return type is non-void with an
        // explicit return elsewhere (79096's !type block).
        assert_eq!(
            checked_rows_with("function f(x: boolean) { if (x) return 1; }\n", &options),
            [(7030, 9, 1)]
        );
        // Declared undefined-including type still reaches the trailing
        // arm (the else-if LADDER: the snc arm's condition fails —
        // undefined IS assignable — and falls through, 79087-79096).
        assert_eq!(
            checked_rows_with(
                "function f(x: boolean): number | undefined { if (x) return 1; }\n",
                &options
            ),
            [(7030, 24, 18)]
        );
        // checkReturnStatement's bare-`return;` face (84546) — only
        // reachable with strictNullChecks off.
        let snc_off = CompilerOptions {
            no_implicit_returns: Some(true),
            strict_null_checks: Some(false),
            ..CompilerOptions::default()
        };
        assert_eq!(
            checked_rows_with(
                "function f(x: boolean): number { if (x) { return; } return 1; }\n",
                &snc_off
            ),
            [(7030, 42, 6)]
        );
    }

    #[test]
    fn never_typed_callable_parameter_suppresses_implicit_return() {
        // The effects consult must see the FunctionTYPE declaration's
        // never annotation (getEffectiveReturnTypeNode reads `.type`
        // on every signature-declaration kind — the FunctionType arm
        // was the 6.6c FP face, neverReturningFunctions1 f12).
        assert_eq!(
            checked_rows(
                "function f12(x: number, fail: (message?: string) => never): number {\n    if (x >= 0) return x;\n    fail(\"negative number\");\n    x;\n}\n"
            ),
            []
        );
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
        // Both oracle rows since the A3 wiring: the grammar 1047 and
        // checkSignatureDeclaration's 2370 (`number[] | undefined`
        // fails the readonly-array relation) — the recorded FN is
        // resolved.
        assert_eq!(
            checked_rows("(function (...rest?: number[]) {});\n"),
            [(1047, 18, 1), (2370, 11, 18)]
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
        // Oracle rows exactly: 2697 (untyped thenable await needs a
        // declared Promise) + 2339 @94 (x.bad → number). The 2339 row
        // recovered when getQuickTypeOfExpression's await arm went
        // live (the initializer used to contain the whole element).
        assert_eq!(
            checked_rows(
                "declare const p: { then(cb: (v: number) => void): void };\n(async () => { const x = await p; x.bad; });\n"
            ),
            [(2697, 59, 41), (2339, 94, 3)]
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

    #[test]
    fn method_modifier_error_suppresses_type_parameter_grammar() {
        // m4-review S7 (oracle: vendored tsc 6.0.3, noLib, strict,
        // 2026-07-19): tsc 1031 @10 (the M7-FN modifier row) + 1183
        // @30 — the declare verdict heads the `||` ladder and
        // suppresses the empty-type-parameter-list 1098 the port
        // reported pre-fix.
        assert_eq!(
            checked_rows("class C { declare m<>(): void {} }\n"),
            [(1183, 30, 1)]
        );
    }

    // ---- m4-review A3: checkSignatureDeclaration on expression
    // forms (oracle: vendored tsc 6.0.3, noLib, strict, 2026-07-19).
    // The contextual once-path used to end in a no-op stub, so every
    // signature-declaration row was FN for fn-exprs/arrows/obj-methods.

    #[test]
    fn arrow_type_predicate_unassignable_reports_2677() {
        assert_eq!(
            checked_rows("const p = (x: number): x is string => typeof x === \"string\";\n"),
            [(2677, 28, 6)]
        );
    }

    #[test]
    fn generator_function_expression_void_annotation_reports_2505() {
        assert_eq!(
            checked_rows("const g = function* (): void {};\n"),
            [(2505, 24, 4)]
        );
    }

    #[test]
    fn async_arrow_non_promise_annotation_reports_1064() {
        assert_eq!(
            checked_rows(
                "interface Promise<T> { p: T }\ndeclare const a: any;\nconst h = async (): number => a;\n"
            ),
            [(1064, 72, 6)]
        );
    }

    #[test]
    fn arrow_non_array_rest_parameter_reports_2370() {
        assert_eq!(
            checked_rows(
                "interface Array<T> { length: number }\ninterface ReadonlyArray<T> { length: number }\ninterface ConcatArray<T> { length: number }\nconst f = (...r: number) => r;\n"
            ),
            [(2370, 139, 12)]
        );
    }

    // ---- m4-review A2: obj-literal accessors defer to the whole
    // checkAccessorDeclaration (oracle: vendored tsc 6.0.3, noLib,
    // strict, 2026-07-19). The subset route checked signature +
    // accessor types but never entered the body.

    #[test]
    fn obj_literal_getter_body_is_checked() {
        assert_eq!(
            checked_rows(
                "const o = {\n    get x() {\n        let a: number = \"s\";\n        return 1;\n    },\n};\n"
            ),
            [(2322, 38, 1)]
        );
    }

    #[test]
    fn obj_literal_accessor_grammar_and_setter_body_rows() {
        assert_eq!(
            checked_rows(
                "const o = {\n    get x(this: void, extra: number) {\n        return 1;\n    },\n    set y(v: string) {\n        let b: string = 123;\n    },\n};\n"
            ),
            [(1054, 20, 1), (2784, 22, 10), (2322, 111, 1)]
        );
    }
}
