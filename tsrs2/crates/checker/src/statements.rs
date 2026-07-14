//! M4 5.8a: the statement band — §2 variable band (grammar workers,
//! checkVariableLikeDeclaration, the collisions band, the renamed-
//! binding drain) + §3 control statements (truthiness kit, loops,
//! switch/return/try). Extraction doc:
//! docs/design/greenfield/m4-58-statement-extraction.md.
//!
//! Slice seams (each greppable, owner in parens):
//! - checkDecorators — the class/decorator band (5.8c §10) registers
//!   deferrals; variable/binding-element/catch declarations cannot
//!   carry decorators, so the hook is a named stub here.
//! - checkExportsOnMergedDeclarations — §5 overload band (5.8b).
//! - checkIteratedTypeOrElementType — §4 iteration protocol (5.8b);
//!   the ArrayBindingPattern widened-type arm and
//!   checkRightHandSideOfForOf escape with it.
//! - checkExternalEmitHelpers (88907) — importHelpers-gated; the
//!   option is absent from CompilerOptions, so every call site is a
//!   verified no-op (§13 options audit) and the calls are elided with
//!   per-site notes.
//! - checkCollisionWithArgumentsInGeneratedCode (83229) — its only
//!   caller is checkSignatureDeclarationDiagnostics (81315), a §5
//!   worker; the fn ports with its caller in 5.8b.
//! - checkAliasSymbol require-alias arm —
//!   isVariableDeclarationInitializedToBareOrAccessedRequire shapes
//!   are JS-only (M2 3.4c residual); TS-file variable symbols never
//!   carry the Alias flag here.

use tsrs2_binder::node_util;
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage, MessageChain};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{CheckMode, ModifierFlags, NodeFlags, SymbolFlags, TypeFlags, TypeId};

use crate::state::{CheckResult2, CheckerState, Unsupported};

impl<'a> CheckerState<'a> {
    // ---- §2 drivers ----

    /// tsc-port: checkVariableStatement @6.0.3
    /// tsc-hash: e7419e774165d50c50d16fcfd9254d411142d9a3d29ee577d86f5c34cfaacea6
    /// tsc-span: _tsc.js:83618-83621
    ///
    /// checkGrammarModifiers stays the M7-stub hook — its false return
    /// feeds the && chain so the grammar workers sit in tsc's slots.
    pub(crate) fn check_variable_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::VariableStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(declaration_list) = data.declaration_list else {
            return Err(Unsupported::new("VariableStatement recovery node"));
        };
        // checkGrammarModifiers gate approximation: in a position
        // where block declarations are disallowed, ANY modifier on a
        // variable statement is illegal — tsc's modifier grammar
        // reports 1184-family and short-circuits the whole ladder.
        // The M7-stub returns false, so mirror that face here (the
        // 1184 row itself stays the M7 FN).
        let has_modifiers = matches!(
            self.data_of(node),
            NodeData::VariableStatement(data) if data.modifiers.is_some()
        );
        let list_block_scope = self.node_flags(declaration_list) & NodeFlags::BLOCK_SCOPED.bits();
        let modifiers_would_report = (has_modifiers
            && self
                .parent_of(node)
                .is_some_and(|parent| !self.allow_block_declarations(parent)))
            // `declare using` / `declare await using`: the modifier
            // grammar owns the report (1044-family) and short-circuits
            // the ladder — our ambient row would double-fire (the
            // usingDeclarations.13 pin).
            || (node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::AMBIENT,
            ) && (list_block_scope == NodeFlags::USING.bits()
                || list_block_scope == NodeFlags::AWAIT_USING.bits()));
        if !modifiers_would_report
            && !self.check_grammar_modifiers(node)
            && !self.check_grammar_variable_declaration_list(declaration_list)?
        {
            self.check_grammar_for_disallowed_block_scoped_variable_statement(
                node,
                declaration_list,
            );
        }
        self.check_variable_declaration_list(declaration_list)
    }

    /// tsc-port: checkVariableDeclarationList @6.0.3
    /// tsc-hash: b77f7ceb5cc6e0fd5717ec5b97459be4ad38ae3d30017e9a23b183a6f0372d14
    /// tsc-span: _tsc.js:83611-83617
    ///
    /// The using/await-using checkExternalEmitHelpers probe
    /// (languageVersion < ESNext) is an importHelpers-gated no-op
    /// (module note) — elided.
    pub(crate) fn check_variable_declaration_list(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::VariableDeclarationList(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        for declaration in self.nodes_of(data.declarations) {
            self.check_source_element(Some(declaration));
        }
        Ok(())
    }

    /// tsc-port: checkVariableDeclaration @6.0.3
    /// tsc-hash: 7d5852063394fdbf5fe79c0a3a578f405b9813a93ffdb0239ea1dcacfb018e16
    /// tsc-span: _tsc.js:83600-83606
    ///
    /// The tracing push/pop pair is elided (no tracing host).
    pub(crate) fn check_variable_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_variable_declaration(node)?;
        self.check_variable_like_declaration(node)
    }

    /// tsc-port: checkBindingElement @6.0.3
    /// tsc-hash: 37fff77c1e97bd2677b90b959f8d1e0530bcc626ddcef907c7529629e2c727b7
    /// tsc-span: _tsc.js:83607-83610
    pub(crate) fn check_binding_element(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_binding_element(node);
        self.check_variable_like_declaration(node)
    }

    // ---- §2 checkVariableLikeDeclaration (the core) ----

    /// tsc-port: checkVariableLikeDeclaration @6.0.3
    /// tsc-hash: ef6e3ea5a5a3e0ce00cc9cdc2ea39764fee1747945ac03d7fb05d4435bd88854
    /// tsc-span: _tsc.js:83403-83574
    ///
    /// 5.8a callers: VariableDeclaration / BindingElement / the catch
    /// variable (checkTryStatement). Parameter (§5, 5.8b) and
    /// Property (§6, 5.8c) route here when their bands land — the
    /// kind-guarded arms below are already transcribed for them.
    /// Elisions, each with its owner note: checkDecorators (5.8c),
    /// the two checkExternalEmitHelpers probes (module note), the
    /// JS require-alias arm (module note), and the JS object-literal
    /// initializer exemption (isJSObjectLiteralInitializer — JS files
    /// route through the plain-JS allowlist, constant-false here).
    pub(crate) fn check_variable_like_declaration(&mut self, node: NodeId) -> CheckResult2<()> {
        let node_kind = self.kind_of(node);
        let is_binding_element = node_kind == SyntaxKind::BindingElement;
        // Step 1: checkDecorators — named stub (5.8c §10).
        self.source_element_stub("checkDecorators", "5.8c")?;
        // Step 2: force the annotation subtree (type-node arms §11).
        if !is_binding_element {
            let annotation = self.type_annotation_of(node);
            self.check_source_element(annotation);
        }
        // Step 3: recovery — no name, nothing to check.
        let Some(name) = self.name_of_node(node) else {
            return Ok(());
        };
        // Step 4: computed names (Property* callers, 5.8b/c).
        if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
            self.check_computed_property_name(name)?;
            if let Some(initializer) = self.only_expression_initializer_of(node) {
                self.check_expression_cached(initializer, CheckMode::NORMAL)?;
            }
        }
        if is_binding_element {
            let NodeData::BindingElement(data) = self.data_of(node) else {
                unreachable!("kind/data agree");
            };
            let (dot_dot_dot, property_name) = (data.dot_dot_dot_token, data.property_name);
            // Renamed signature-parameter bindings: record for the
            // end-of-worker drain; the early return is semantic.
            if property_name.is_some()
                && self.kind_of(name) == SyntaxKind::Identifier
                && node_util::is_part_of_parameter_declaration(
                    self.binder.source_of_node(node),
                    node,
                )
                && self.containing_function_body_is_missing(node)
            {
                self.potential_unused_renamed_binding_elements_in_types
                    .push(node);
                return Ok(());
            }
            // (Object-rest emit-helper probe elided — module note.)
            if let Some(property_name) = property_name {
                if self.kind_of(property_name) == SyntaxKind::ComputedPropertyName {
                    self.check_computed_property_name(property_name)?;
                }
            }
            let pattern = self.parent_of(node).expect("binding element has a pattern");
            let parent = self
                .parent_of(pattern)
                .expect("binding pattern has a declaration");
            let parent_check_mode = if dot_dot_dot.is_some() {
                CheckMode::REST_BINDING_ELEMENT
            } else {
                CheckMode::NORMAL
            };
            let parent_type =
                self.get_type_for_binding_element_parent(parent, parent_check_mode)?;
            let effective_name = property_name.unwrap_or(name);
            if let Some(parent_type) = parent_type {
                if !node_util::is_binding_pattern(
                    self.binder.source_of_node(effective_name),
                    effective_name,
                ) {
                    let expr_type = self.get_literal_type_from_property_name(effective_name)?;
                    if let Some(name_text) = self.property_name_from_type_usable(expr_type) {
                        if let Some(property) =
                            self.get_property_of_type_full(parent_type, &name_text)?
                        {
                            self.mark_property_as_referenced(
                                property, /*node_for_check_write_only*/ None,
                                /*is_self_type_access*/ false,
                            );
                            let parent_initializer_is_super = self
                                .only_expression_initializer_of(parent)
                                .is_some_and(|initializer| {
                                    self.kind_of(initializer) == SyntaxKind::SuperKeyword
                                });
                            self.check_property_accessibility(
                                node,
                                parent_initializer_is_super,
                                /*writing*/ false,
                                parent_type,
                                property,
                                /*report_error*/ true,
                            )?;
                        }
                    }
                }
            }
        }
        // Step 6: recurse into pattern elements. (The array-pattern
        // downlevelIteration emit-helper probe is elided — module
        // note.)
        let name_is_pattern = node_util::is_binding_pattern(self.binder.source_of_node(name), name);
        if name_is_pattern {
            for element in self.binding_pattern_elements(name) {
                self.check_source_element(Some(element));
            }
        }
        // Step 7: initializer on a signature-only parameter.
        if self.initializer_of_node(node).is_some()
            && node_util::is_part_of_parameter_declaration(self.binder.source_of_node(node), node)
            && self.containing_function_body_is_missing(node)
        {
            self.error_at(
                Some(node),
                &diagnostics::A_parameter_initializer_is_only_allowed_in_a_function_or_constructor_implementation,
                &[],
            );
            return Ok(());
        }
        // Step 8: binding-pattern names check their initializer
        // against the widened pattern type, then return.
        if name_is_pattern {
            if self.is_in_ambient_or_type_node(node) {
                return Ok(());
            }
            let parent_parent_kind = self
                .parent_of(node)
                .and_then(|parent| self.parent_of(parent))
                .map(|grandparent| self.kind_of(grandparent));
            let initializer = self.only_expression_initializer_of(node);
            let need_check_initializer =
                initializer.is_some() && parent_parent_kind != Some(SyntaxKind::ForInStatement);
            let need_check_widened_type = self
                .binding_pattern_elements(name)
                .iter()
                .all(|&element| self.kind_of(element) == SyntaxKind::OmittedExpression);
            if need_check_initializer || need_check_widened_type {
                let widened_type =
                    self.get_widened_type_for_variable_like_declaration(node, false)?;
                let strict_null_checks = self
                    .options
                    .strict_option_value(self.options.strict_null_checks);
                if need_check_initializer {
                    let initializer = initializer.expect("checked above");
                    let initializer_type =
                        self.check_expression_cached(initializer, CheckMode::NORMAL)?;
                    if strict_null_checks && need_check_widened_type {
                        self.check_non_null_non_void_type(initializer_type, node)?;
                    } else {
                        // checkTypeAssignableToAndOptionallyElaborate
                        // — head-only slice; the widened type is
                        // recomputed like tsc.
                        let target =
                            self.get_widened_type_for_variable_like_declaration(node, false)?;
                        self.declaration_initializer_flow_gate(
                            initializer,
                            initializer_type,
                            target,
                        )?;
                        self.check_type_assignable_to(
                            initializer_type,
                            target,
                            Some(node),
                            &diagnostics::Type_0_is_not_assignable_to_type_1,
                        )?;
                    }
                }
                if need_check_widened_type {
                    if self.kind_of(name) == SyntaxKind::ArrayBindingPattern {
                        return Err(Unsupported::new(
                            "checkIteratedTypeOrElementType ([ITER] §4, 5.8b)",
                        ));
                    } else if strict_null_checks {
                        self.check_non_null_non_void_type(widened_type, node)?;
                    }
                }
            }
            return Ok(());
        }
        // Step 9: the JS require-alias arm is elided (module note);
        // TS-file variable symbols never carry the Alias flag here.
        let symbol = self.get_symbol_of_declaration(node)?;
        // Step 10: bigint property names (Property* callers).
        if self.kind_of(name) == SyntaxKind::BigIntLiteral {
            self.error_at(
                Some(name),
                &diagnostics::A_bigint_literal_cannot_be_used_as_a_property_name,
                &[],
            );
        }
        // Step 11.
        let ty = {
            let raw = self.get_type_of_symbol(symbol)?;
            self.convert_auto_to_any(raw)
        };
        let value_declaration = self.binder.symbol(symbol).value_declaration;
        let parent_parent_kind = self
            .parent_of(node)
            .and_then(|parent| self.parent_of(parent))
            .map(|grandparent| self.kind_of(grandparent));
        if value_declaration == Some(node) {
            // Step 12: the value declaration's own initializer row.
            let initializer = self.only_expression_initializer_of(node);
            if let Some(initializer) = initializer {
                if parent_parent_kind != Some(SyntaxKind::ForInStatement) {
                    let initializer_type =
                        self.check_expression_cached(initializer, CheckMode::NORMAL)?;
                    // THE annotated-declaration 2322 row: errorNode =
                    // node (getErrorSpanForNode's VariableDeclaration
                    // arm reports at the NAME span — pinned).
                    self.declaration_initializer_flow_gate(initializer, initializer_type, ty)?;
                    self.check_type_assignable_to(
                        initializer_type,
                        ty,
                        Some(node),
                        &diagnostics::Type_0_is_not_assignable_to_type_1,
                    )?;
                    let block_scope_kind =
                        node_util::get_combined_node_flags(self.binder.source_of_node(node), node)
                            .bits()
                            & NodeFlags::BLOCK_SCOPED.bits();
                    if block_scope_kind == NodeFlags::AWAIT_USING.bits() {
                        let async_disposable =
                            self.get_global_async_disposable_type(/*report_errors*/ true)?;
                        let disposable =
                            self.get_global_disposable_type(/*report_errors*/ true)?;
                        if async_disposable != self.empty_object_type
                            && disposable != self.empty_object_type
                        {
                            let members = [
                                async_disposable,
                                disposable,
                                self.tables.intrinsics.null,
                                self.tables.intrinsics.undefined,
                            ];
                            let optional_disposable = self.get_union_type_ex(
                                &members,
                                tsrs2_types::UnionReduction::Literal,
                            )?;
                            let widened = self.widen_type_for_variable_like_declaration(
                                Some(initializer_type),
                                node,
                                /*report_errors*/ false,
                            )?;
                            self.check_type_assignable_to(
                                widened,
                                optional_disposable,
                                Some(initializer),
                                &diagnostics::The_initializer_of_an_await_using_declaration_must_be_either_an_object_with_a_Symbol_asyncDispose_or_Symbol_dispose_method_or_be_null_or_undefined,
                            )?;
                        }
                    } else if block_scope_kind == NodeFlags::USING.bits() {
                        let disposable =
                            self.get_global_disposable_type(/*report_errors*/ true)?;
                        if disposable != self.empty_object_type {
                            let members = [
                                disposable,
                                self.tables.intrinsics.null,
                                self.tables.intrinsics.undefined,
                            ];
                            let optional_disposable = self.get_union_type_ex(
                                &members,
                                tsrs2_types::UnionReduction::Literal,
                            )?;
                            let widened = self.widen_type_for_variable_like_declaration(
                                Some(initializer_type),
                                node,
                                /*report_errors*/ false,
                            )?;
                            self.check_type_assignable_to(
                                widened,
                                optional_disposable,
                                Some(initializer),
                                &diagnostics::The_initializer_of_a_using_declaration_must_be_either_an_object_with_a_Symbol_dispose_method_or_be_null_or_undefined,
                            )?;
                        }
                    }
                }
            }
            let declarations = self.binder.symbol(symbol).declarations.clone();
            if declarations.len() > 1
                && declarations.iter().any(|&d| {
                    d != node
                        && self.is_variable_like_declaration_kind(d)
                        && !self.are_declaration_flags_identical(d, node)
                })
            {
                let display = self.declaration_name_display(name);
                self.error_at(
                    Some(name),
                    &diagnostics::All_declarations_of_0_must_have_identical_modifiers,
                    &[&display],
                );
            }
        } else {
            // Step 13: the merged-declaration face.
            let declaration_type = {
                let widened = self.get_widened_type_for_variable_like_declaration(node, false)?;
                self.convert_auto_to_any(widened)
            };
            let error_type = self.tables.intrinsics.error;
            // [JSDOC] gate: a merged declaration living in a JS file
            // takes its type from @type tags (unmodeled) — comparing
            // against our annotation-less read fabricates 2403/2717.
            let any_js_declaration =
                self.binder
                    .symbol(symbol)
                    .declarations
                    .iter()
                    .any(|&declaration| {
                        crate::is_js_file_name(&self.binder.source_of_node(declaration).file_name)
                    });
            if any_js_declaration {
                return Err(Unsupported::new(
                    "merged declaration typed from a JS file (@type tags [JSDOC], M8 checkJs band)",
                ));
            }
            if ty != error_type
                && declaration_type != error_type
                && !self.is_type_identical_to(ty, declaration_type)?
                && !self
                    .binder
                    .symbol(symbol)
                    .flags
                    .intersects(SymbolFlags::ASSIGNMENT)
            {
                self.error_next_variable_or_property_declaration_must_have_same_type(
                    value_declaration,
                    ty,
                    node,
                    declaration_type,
                )?;
            }
            if let Some(initializer) = self.only_expression_initializer_of(node) {
                let initializer_type =
                    self.check_expression_cached(initializer, CheckMode::NORMAL)?;
                self.declaration_initializer_flow_gate(
                    initializer,
                    initializer_type,
                    declaration_type,
                )?;
                self.check_type_assignable_to(
                    initializer_type,
                    declaration_type,
                    Some(node),
                    &diagnostics::Type_0_is_not_assignable_to_type_1,
                )?;
            }
            if let Some(value_declaration) = value_declaration {
                if !self.are_declaration_flags_identical(node, value_declaration) {
                    let display = self.declaration_name_display(name);
                    self.error_at(
                        Some(name),
                        &diagnostics::All_declarations_of_0_must_have_identical_modifiers,
                        &[&display],
                    );
                }
            }
        }
        // Step 14: tail, non-property kinds only.
        if node_kind != SyntaxKind::PropertyDeclaration
            && node_kind != SyntaxKind::PropertySignature
        {
            self.source_element_stub("checkExportsOnMergedDeclarations", "5.8b")?;
            if node_kind == SyntaxKind::VariableDeclaration
                || node_kind == SyntaxKind::BindingElement
            {
                self.check_var_declared_names_not_shadowed(node)?;
            }
            self.check_collisions_for_declaration_name(node, Some(name));
        }
        Ok(())
    }

    /// tsc-port: errorNextVariableOrPropertyDeclarationMustHaveSameType @6.0.3
    /// tsc-hash: 88f4f009d322e9b123f200a2f7950e6651705fc29f014dc1c3d52c98957d8678
    /// tsc-span: _tsc.js:83575-83589
    ///
    /// Display band (risk §14.4): an unrenderable type unwinds
    /// Unsupported and the whole report escapes — never a partial
    /// render.
    fn error_next_variable_or_property_declaration_must_have_same_type(
        &mut self,
        first_declaration: Option<NodeId>,
        first_type: TypeId,
        next_declaration: NodeId,
        next_type: TypeId,
    ) -> CheckResult2<()> {
        let next_declaration_name = self.name_of_node(next_declaration);
        let message = if matches!(
            self.kind_of(next_declaration),
            SyntaxKind::PropertyDeclaration | SyntaxKind::PropertySignature
        ) {
            &diagnostics::Subsequent_property_declarations_must_have_the_same_type_Property_0_must_be_of_type_1_but_here_has_type_2
        } else {
            &diagnostics::Subsequent_variable_declarations_must_have_the_same_type_Variable_0_must_be_of_type_1_but_here_has_type_2
        };
        let decl_name = match next_declaration_name {
            Some(name) => self.declaration_name_display(name),
            None => "(Missing)".to_owned(),
        };
        let first_text = self.type_to_string_slice(first_type)?;
        let next_text = self.type_to_string_slice(next_type)?;
        let related = first_declaration
            .map(|declaration| {
                self.related_info_for_node(
                    declaration,
                    &diagnostics::_0_was_also_declared_here,
                    &[&decl_name],
                )
            })
            .into_iter()
            .collect();
        self.error_at_with_related(
            next_declaration_name.or(Some(next_declaration)),
            message,
            &[&decl_name, &first_text, &next_text],
            related,
        );
        Ok(())
    }

    /// tsc-port: areDeclarationFlagsIdentical @6.0.3
    /// tsc-hash: 2600092f6534f49b9a6cb18aa592f623cc7ee70b66d616eacc30c07802bced12
    /// tsc-span: _tsc.js:83590-83599
    fn are_declaration_flags_identical(&self, left: NodeId, right: NodeId) -> bool {
        let (left_kind, right_kind) = (self.kind_of(left), self.kind_of(right));
        if left_kind == SyntaxKind::Parameter && right_kind == SyntaxKind::VariableDeclaration
            || left_kind == SyntaxKind::VariableDeclaration && right_kind == SyntaxKind::Parameter
        {
            return true;
        }
        if self.has_question_token(left) != self.has_question_token(right) {
            return false;
        }
        let interesting = ModifierFlags::PRIVATE.bits()
            | ModifierFlags::PROTECTED.bits()
            | ModifierFlags::ASYNC.bits()
            | ModifierFlags::ABSTRACT.bits()
            | ModifierFlags::READONLY.bits()
            | ModifierFlags::STATIC.bits();
        // getSelectedEffectiveModifierFlags: effective == syntactic in
        // TS files (JSDoc modifiers are the JS residual).
        let left_flags =
            node_util::get_syntactic_modifier_flags(self.binder.source_of_node(left), left).bits()
                & interesting;
        let right_flags =
            node_util::get_syntactic_modifier_flags(self.binder.source_of_node(right), right)
                .bits()
                & interesting;
        left_flags == right_flags
    }

    /// tsc-port: convertAutoToAny @6.0.3
    /// tsc-hash: a9e79a9777e78396f826a6c1016b49f68afd58a183500c99f293dbf849b6f35f
    /// tsc-span: _tsc.js:83400-83402
    ///
    /// [FLOW M5]: the 5.6 AUTO arm already answers anyType at the
    /// declared-type level (autoType/autoArrayType never surface), so
    /// the twin is an identity — kept in slot for the M5 wiring.
    fn convert_auto_to_any(&self, ty: TypeId) -> TypeId {
        ty
    }

    /// tsc-port: checkVarDeclaredNamesNotShadowed @6.0.3
    /// tsc-hash: 57e3c0212046b4cd336bb4f34f4e79e57b807638cda2bb54225f0e6ad2115e89
    /// tsc-span: _tsc.js:83371-83399
    fn check_var_declared_names_not_shadowed(&mut self, node: NodeId) -> CheckResult2<()> {
        let source = self.binder.source_of_node(node);
        if node_util::get_combined_node_flags(source, node).intersects(NodeFlags::BLOCK_SCOPED)
            || node_util::is_part_of_parameter_declaration(source, node)
        {
            return Ok(());
        }
        let symbol = self.get_symbol_of_declaration(node)?;
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::FUNCTION_SCOPED_VARIABLE)
        {
            return Ok(());
        }
        let Some(name) = self.name_of_node(node) else {
            return Ok(());
        };
        let Some(name_text) = self.identifier_text_of(name).map(str::to_owned) else {
            return Ok(());
        };
        let local = self.resolve_name(
            Some(node),
            &name_text,
            SymbolFlags::VARIABLE,
            /*name_not_found_message*/ None,
            /*is_use*/ false,
            /*exclude_globals*/ false,
        );
        let Some(local) = local else {
            return Ok(());
        };
        if local == symbol
            || !self
                .binder
                .symbol(local)
                .flags
                .intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
        {
            return Ok(());
        }
        // getDeclarationNodeFlagsFromSymbol: combined flags of the
        // value declaration.
        let Some(local_declaration) = self.binder.symbol(local).value_declaration else {
            return Ok(());
        };
        let local_source = self.binder.source_of_node(local_declaration);
        if !node_util::get_combined_node_flags(local_source, local_declaration)
            .intersects(NodeFlags::BLOCK_SCOPED)
        {
            return Ok(());
        }
        let var_decl_list =
            self.ancestor_of_kind(local_declaration, SyntaxKind::VariableDeclarationList);
        let Some(var_decl_list) = var_decl_list else {
            return Ok(());
        };
        let list_parent = self.parent_of(var_decl_list);
        let container = match list_parent {
            Some(parent) if self.kind_of(parent) == SyntaxKind::VariableStatement => {
                self.parent_of(parent)
            }
            _ => None,
        };
        let names_share_scope = container.is_some_and(|container| match self.kind_of(container) {
            SyntaxKind::Block => self
                .parent_of(container)
                .is_some_and(|parent| node_util::is_function_like_kind(self.kind_of(parent))),
            SyntaxKind::ModuleBlock | SyntaxKind::ModuleDeclaration | SyntaxKind::SourceFile => {
                true
            }
            _ => false,
        });
        if !names_share_scope {
            let display = self.symbol_display_name(local);
            self.error_at(
                Some(node),
                &diagnostics::Cannot_initialize_outer_scoped_variable_0_in_the_same_scope_as_block_scoped_declaration_1,
                &[&display, &display],
            );
        }
        Ok(())
    }

    // ---- §2 grammar workers ----

    /// tsc-port: checkGrammarVariableDeclarationList @6.0.3
    /// tsc-hash: fa30e2731847fc9a016c79a464c82f37b355c4b7f83ed17ec6afbd44c00194e6
    /// tsc-span: _tsc.js:90130-90162
    pub(crate) fn check_grammar_variable_declaration_list(
        &mut self,
        declaration_list: NodeId,
    ) -> CheckResult2<bool> {
        let NodeData::VariableDeclarationList(data) = self.data_of(declaration_list) else {
            unreachable!("kind/data agree");
        };
        let declarations = data.declarations;
        if self.check_grammar_for_disallowed_trailing_comma(
            declarations,
            &diagnostics::Trailing_comma_not_allowed,
        ) {
            return Ok(true);
        }
        let Some(declarations_array) = declarations else {
            return Ok(false);
        };
        let (array_nodes_empty, array_pos, array_end) = {
            let source = self.binder.source_of_node(declaration_list);
            let array = source.arena.node_array(declarations_array);
            (
                array.nodes.is_empty(),
                array.pos as usize,
                array.end as usize,
            )
        };
        if array_nodes_empty {
            let start = self.utf16_position(declaration_list, array_pos);
            let end = self.utf16_position(declaration_list, array_end);
            return Ok(self.grammar_error_at_pos(
                declaration_list,
                start,
                end.saturating_sub(start),
                &diagnostics::Variable_declaration_list_cannot_be_empty,
                &[],
            ));
        }
        let block_scope_flags = self.node_flags(declaration_list) & NodeFlags::BLOCK_SCOPED.bits();
        if block_scope_flags == NodeFlags::USING.bits()
            || block_scope_flags == NodeFlags::AWAIT_USING.bits()
        {
            let is_using = block_scope_flags == NodeFlags::USING.bits();
            let parent = self.parent_of(declaration_list);
            if parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ForInStatement) {
                return Ok(self.grammar_error_on_node(
                    declaration_list,
                    if is_using {
                        &diagnostics::The_left_hand_side_of_a_for_in_statement_cannot_be_a_using_declaration
                    } else {
                        &diagnostics::The_left_hand_side_of_a_for_in_statement_cannot_be_an_await_using_declaration
                    },
                    &[],
                ));
            }
            let grandparent = parent.and_then(|parent| self.parent_of(parent));
            if parent.is_some_and(|parent| self.kind_of(parent) == SyntaxKind::VariableStatement)
                && grandparent.is_some_and(|grandparent| {
                    matches!(
                        self.kind_of(grandparent),
                        SyntaxKind::CaseClause | SyntaxKind::DefaultClause
                    )
                })
            {
                return Ok(self.grammar_error_on_node(
                    declaration_list,
                    if is_using {
                        &diagnostics::using_declarations_are_not_allowed_in_case_or_default_clauses_unless_contained_within_a_block
                    } else {
                        &diagnostics::await_using_declarations_are_not_allowed_in_case_or_default_clauses_unless_contained_within_a_block
                    },
                    &[],
                ));
            }
            if self.node_flags(declaration_list) & NodeFlags::AMBIENT.bits() != 0 {
                return Ok(self.grammar_error_on_node(
                    declaration_list,
                    if is_using {
                        &diagnostics::using_declarations_are_not_allowed_in_ambient_contexts
                    } else {
                        &diagnostics::await_using_declarations_are_not_allowed_in_ambient_contexts
                    },
                    &[],
                ));
            }
            if !is_using {
                return self.check_await_grammar(declaration_list);
            }
        }
        Ok(false)
    }

    /// tsc-port: allowBlockDeclarations @6.0.3
    /// tsc-hash: 98fc5c16b54b669191dffc47e7f4d54910c31f6285f14bf70c13927b6cd6c675
    /// tsc-span: _tsc.js:90163-90177
    fn allow_block_declarations(&self, parent: NodeId) -> bool {
        match self.kind_of(parent) {
            SyntaxKind::IfStatement
            | SyntaxKind::DoStatement
            | SyntaxKind::WhileStatement
            | SyntaxKind::WithStatement
            | SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement => false,
            SyntaxKind::LabeledStatement => self
                .parent_of(parent)
                .is_none_or(|grandparent| self.allow_block_declarations(grandparent)),
            _ => true,
        }
    }

    /// tsc-port: checkGrammarForDisallowedBlockScopedVariableStatement @6.0.3
    /// tsc-hash: db51d94da15743b3bb9952c67c99b470afbb88ab880bc97ed914819c26b391fa
    /// tsc-span: _tsc.js:90178-90186
    ///
    /// Plain error() channel — NOT parse-diagnostics-gated (risk
    /// §14.11).
    fn check_grammar_for_disallowed_block_scoped_variable_statement(
        &mut self,
        node: NodeId,
        declaration_list: NodeId,
    ) {
        let Some(parent) = self.parent_of(node) else {
            return;
        };
        if self.allow_block_declarations(parent) {
            return;
        }
        let source = self.binder.source_of_node(declaration_list);
        let block_scope_kind = node_util::get_combined_node_flags(source, declaration_list).bits()
            & NodeFlags::BLOCK_SCOPED.bits();
        if block_scope_kind == 0 {
            return;
        }
        let keyword = if block_scope_kind == NodeFlags::LET.bits() {
            "let"
        } else if block_scope_kind == NodeFlags::CONST.bits() {
            "const"
        } else if block_scope_kind == NodeFlags::USING.bits() {
            "using"
        } else {
            "await using"
        };
        self.error_at(
            Some(node),
            &diagnostics::_0_declarations_can_only_be_declared_inside_a_block,
            &[keyword],
        );
    }

    /// tsc-port: checkGrammarVariableDeclaration @6.0.3
    /// tsc-hash: b9957e90211a97064ed1f557b7648e68bfd5f77c0cebb3b53ca6c9d77b193903
    /// tsc-span: _tsc.js:90063-90099
    fn check_grammar_variable_declaration(&mut self, node: NodeId) -> CheckResult2<bool> {
        let NodeData::VariableDeclaration(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (name, exclamation_token, annotation, initializer) = (
            data.name,
            data.exclamation_token,
            data.r#type,
            data.initializer,
        );
        let Some(name) = name else {
            return Ok(false);
        };
        let source = self.binder.source_of_node(node);
        let node_flags = node_util::get_combined_node_flags(source, node);
        let block_scope_kind = node_flags.bits() & NodeFlags::BLOCK_SCOPED.bits();
        let name_is_pattern = node_util::is_binding_pattern(source, name);
        if name_is_pattern {
            if block_scope_kind == NodeFlags::AWAIT_USING.bits() {
                return Ok(self.grammar_error_on_node(
                    node,
                    &diagnostics::_0_declarations_may_not_have_binding_patterns,
                    &["await using"],
                ));
            }
            if block_scope_kind == NodeFlags::USING.bits() {
                return Ok(self.grammar_error_on_node(
                    node,
                    &diagnostics::_0_declarations_may_not_have_binding_patterns,
                    &["using"],
                ));
            }
        }
        let parent_parent_kind = self
            .parent_of(node)
            .and_then(|parent| self.parent_of(parent))
            .map(|grandparent| self.kind_of(grandparent));
        let in_for_in_or_of = matches!(
            parent_parent_kind,
            Some(SyntaxKind::ForInStatement) | Some(SyntaxKind::ForOfStatement)
        );
        if !in_for_in_or_of {
            if node_flags.intersects(NodeFlags::AMBIENT) {
                if self.check_ambient_initializer(node)? {
                    return Ok(true);
                }
            } else if initializer.is_none() {
                if name_is_pattern
                    && !self.parent_of(node).is_some_and(|parent| {
                        node_util::is_binding_pattern(self.binder.source_of_node(parent), parent)
                    })
                {
                    return Ok(self.grammar_error_on_node(
                        node,
                        &diagnostics::A_destructuring_declaration_must_have_an_initializer,
                        &[],
                    ));
                }
                let keyword = if block_scope_kind == NodeFlags::AWAIT_USING.bits() {
                    Some("await using")
                } else if block_scope_kind == NodeFlags::USING.bits() {
                    Some("using")
                } else if block_scope_kind == NodeFlags::CONST.bits() {
                    Some("const")
                } else {
                    None
                };
                if let Some(keyword) = keyword {
                    return Ok(self.grammar_error_on_node(
                        node,
                        &diagnostics::_0_declarations_must_be_initialized,
                        &[keyword],
                    ));
                }
            }
        }
        if let Some(exclamation_token) = exclamation_token {
            let parent_parent_is_variable_statement =
                parent_parent_kind == Some(SyntaxKind::VariableStatement);
            if !parent_parent_is_variable_statement
                || annotation.is_none()
                || initializer.is_some()
                || node_flags.intersects(NodeFlags::AMBIENT)
            {
                let message = if initializer.is_some() {
                    &diagnostics::Declarations_with_initializers_cannot_also_have_definite_assignment_assertions
                } else if annotation.is_none() {
                    &diagnostics::Declarations_with_definite_assignment_assertions_must_also_have_type_annotations
                } else {
                    &diagnostics::A_definite_assignment_assertion_is_not_permitted_in_this_context
                };
                return Ok(self.grammar_error_on_node(exclamation_token, message, &[]));
            }
        }
        // host.getEmitModuleFormatOfFile < System: impliedNodeFormat
        // is unported, so the per-file format reduces to the computed
        // module kind.
        if self.options.emit_module_kind() < 4 {
            let parent_parent_ambient = self
                .parent_of(node)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|grandparent| {
                    self.node_flags(grandparent) & NodeFlags::AMBIENT.bits() != 0
                });
            let parent_parent_exported = self
                .parent_of(node)
                .and_then(|parent| self.parent_of(parent))
                .is_some_and(|grandparent| {
                    node_util::has_syntactic_modifier(
                        self.binder.source_of_node(grandparent),
                        grandparent,
                        ModifierFlags::EXPORT,
                    )
                });
            if !parent_parent_ambient && parent_parent_exported {
                self.check_es_module_marker(name);
            }
        }
        if block_scope_kind != 0 {
            return Ok(self.check_grammar_name_in_let_or_const_declarations(name));
        }
        Ok(false)
    }

    /// tsc-port: checkAmbientInitializer @6.0.3
    /// tsc-hash: 0fb8e6d9740ef01fa649156af905db37144d09b43fc20b0516dbaea79a377df8
    /// tsc-span: _tsc.js:90049-90062
    fn check_ambient_initializer(&mut self, node: NodeId) -> CheckResult2<bool> {
        let Some(initializer) = self.initializer_of_node(node) else {
            return Ok(false);
        };
        let is_invalid = !(self.is_string_or_number_literal_expression(initializer)
            || self.is_simple_literal_enum_reference(initializer)?
            || matches!(
                self.kind_of(initializer),
                SyntaxKind::TrueKeyword | SyntaxKind::FalseKeyword
            )
            || self.is_bigint_literal_expression(initializer));
        // isDeclarationReadonly || isVariableDeclaration && isVarConstLike.
        let source = self.binder.source_of_node(node);
        let is_readonly = node_util::get_combined_modifier_flags(source, node)
            .intersects(ModifierFlags::READONLY)
            && !self.parent_of(node).is_some_and(|parent| {
                node_util::is_parameter_property_declaration(source, node, parent)
            });
        let block_scope_kind = node_util::get_combined_node_flags(source, node).bits()
            & NodeFlags::BLOCK_SCOPED.bits();
        let is_const_like = self.kind_of(node) == SyntaxKind::VariableDeclaration
            && (block_scope_kind == NodeFlags::CONST.bits()
                || block_scope_kind == NodeFlags::USING.bits()
                || block_scope_kind == NodeFlags::AWAIT_USING.bits());
        let annotation = self.type_annotation_of(node);
        if (is_readonly || is_const_like) && annotation.is_none() {
            if is_invalid {
                return Ok(self.grammar_error_on_node(
                    initializer,
                    &diagnostics::A_const_initializer_in_an_ambient_context_must_be_a_string_or_numeric_literal_or_literal_enum_reference,
                    &[],
                ));
            }
        } else {
            return Ok(self.grammar_error_on_node(
                initializer,
                &diagnostics::Initializers_are_not_allowed_in_ambient_contexts,
                &[],
            ));
        }
        Ok(false)
    }

    /// tsc-port: isStringOrNumberLiteralExpression @6.0.3
    /// tsc-hash: 302c26bbfe874ca754d59bcbdcfd227ccd15fb744ff739a73f84785b21ee57c0
    /// tsc-span: _tsc.js:90038-90040
    fn is_string_or_number_literal_expression(&self, expr: NodeId) -> bool {
        let source = self.binder.source_of_node(expr);
        if node_util::is_string_or_numeric_literal_like(source, expr) {
            return true;
        }
        matches!(self.data_of(expr), NodeData::PrefixUnaryExpression(data)
        if data.operator == SyntaxKind::MinusToken
            && data.operand.is_some_and(|operand| {
                self.kind_of(operand) == SyntaxKind::NumericLiteral
            }))
    }

    /// tsc-port: isBigIntLiteralExpression @6.0.3
    /// tsc-hash: 55a28aa8106d98b7ea259f9c075e82db13c7d2e4c2679f2b17e4d5b7640e116a
    /// tsc-span: _tsc.js:90041-90043
    fn is_bigint_literal_expression(&self, expr: NodeId) -> bool {
        if self.kind_of(expr) == SyntaxKind::BigIntLiteral {
            return true;
        }
        matches!(self.data_of(expr), NodeData::PrefixUnaryExpression(data)
        if data.operator == SyntaxKind::MinusToken
            && data.operand.is_some_and(|operand| {
                self.kind_of(operand) == SyntaxKind::BigIntLiteral
            }))
    }

    /// tsc-port: isSimpleLiteralEnumReference @6.0.3
    /// tsc-hash: c7fb47c14b4aca9e85ec9377bc7ebfed18e76fb43010736aa990069ff07f347b
    /// tsc-span: _tsc.js:90044-90048
    fn is_simple_literal_enum_reference(&mut self, expr: NodeId) -> CheckResult2<bool> {
        let source = self.binder.source_of_node(expr);
        let shape_matches = match self.data_of(expr) {
            NodeData::PropertyAccessExpression(data) => data
                .expression
                .is_some_and(|inner| node_util::is_entity_name_expression(source, inner)),
            NodeData::ElementAccessExpression(data) => {
                data.argument_expression
                    .is_some_and(|argument| self.is_string_or_number_literal_expression(argument))
                    && data
                        .expression
                        .is_some_and(|inner| node_util::is_entity_name_expression(source, inner))
            }
            _ => false,
        };
        if !shape_matches {
            return Ok(false);
        }
        let ty = self.check_expression_cached(expr, CheckMode::NORMAL)?;
        Ok(self.tables.flags_of(ty).intersects(TypeFlags::ENUM_LIKE))
    }

    /// tsc-port: checkESModuleMarker @6.0.3
    /// tsc-hash: e678444e57587f3692d1684590e4458f29815fdaf4976a215173bef5f5463975
    /// tsc-span: _tsc.js:90100-90114
    fn check_es_module_marker(&mut self, name: NodeId) -> bool {
        if self.kind_of(name) == SyntaxKind::Identifier {
            let is_marker = self.identifier_text_of(name).is_some_and(|text| {
                tsrs2_binder::unescape_leading_underscores(text) == "__esModule"
            });
            if is_marker {
                return self.grammar_error_on_node_skipped_on(
                    name,
                    &diagnostics::Identifier_expected_esModule_is_reserved_as_an_exported_marker_when_transforming_ECMAScript_modules,
                    &[],
                );
            }
        } else {
            // tsc returns on the FIRST non-omitted element.
            for element in self.binding_pattern_elements(name) {
                if self.kind_of(element) != SyntaxKind::OmittedExpression {
                    if let Some(element_name) = self.name_of_node(element) {
                        return self.check_es_module_marker(element_name);
                    }
                    return false;
                }
            }
        }
        false
    }

    /// tsc-port: grammarErrorOnNodeSkippedOn @6.0.3
    /// tsc-hash: a99a58ded4a296b6549a019fb418a7db0d2bc13be8e28d763abc144ce54b72f0
    /// tsc-span: _tsc.js:90232-90239
    ///
    /// The hasParseDiagnostics gate around errorSkippedOn — "noEmit"
    /// is the only key (state.rs error_skipped_on_no_emit).
    fn grammar_error_on_node_skipped_on(
        &mut self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> bool {
        if self.has_parse_diagnostics(node) {
            return false;
        }
        self.error_skipped_on_no_emit(Some(node), message, args);
        true
    }

    /// tsc-port: checkGrammarNameInLetOrConstDeclarations @6.0.3
    /// tsc-hash: 6f743554c0a026b4b6d6cff8b77e5d5631e9fe4f8c3001cc123e0cc0b4ed42ac
    /// tsc-span: _tsc.js:90115-90129
    fn check_grammar_name_in_let_or_const_declarations(&mut self, name: NodeId) -> bool {
        if self.kind_of(name) == SyntaxKind::Identifier {
            if self.identifier_text_of(name) == Some("let") {
                return self.grammar_error_on_node(
                    name,
                    &diagnostics::let_is_not_allowed_to_be_used_as_a_name_in_let_or_const_declarations,
                    &[],
                );
            }
        } else {
            for element in self.binding_pattern_elements(name) {
                if self.kind_of(element) != SyntaxKind::OmittedExpression {
                    if let Some(element_name) = self.name_of_node(element) {
                        self.check_grammar_name_in_let_or_const_declarations(element_name);
                    }
                }
            }
        }
        false
    }

    /// tsc-port: checkGrammarBindingElement @6.0.3
    /// tsc-hash: 362d42c73508058edac981bf53b16ca8ef9efe987c1ac5ebef4d56ac2bc0c132
    /// tsc-span: _tsc.js:90023-90037
    fn check_grammar_binding_element(&mut self, node: NodeId) -> bool {
        let NodeData::BindingElement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (dot_dot_dot, property_name, name, initializer) = (
            data.dot_dot_dot_token,
            data.property_name,
            data.name,
            data.initializer,
        );
        if dot_dot_dot.is_some() {
            let parent_elements =
                self.parent_of(node)
                    .and_then(|parent| match self.data_of(parent) {
                        NodeData::ObjectBindingPattern(data) => data.elements,
                        NodeData::ArrayBindingPattern(data) => data.elements,
                        _ => None,
                    });
            if let Some(elements) = parent_elements {
                let element_nodes = self.nodes_of(Some(elements));
                if element_nodes.last() != Some(&node) {
                    return self.grammar_error_on_node(
                        node,
                        &diagnostics::A_rest_element_must_be_last_in_a_destructuring_pattern,
                        &[],
                    );
                }
                self.check_grammar_for_disallowed_trailing_comma(
                    Some(elements),
                    &diagnostics::A_rest_parameter_or_binding_pattern_may_not_have_a_trailing_comma,
                );
                if property_name.is_some() {
                    if let Some(name) = name {
                        return self.grammar_error_on_node(
                            name,
                            &diagnostics::A_rest_element_cannot_have_a_property_name,
                            &[],
                        );
                    }
                }
            }
        }
        if dot_dot_dot.is_some() {
            if let Some(initializer) = initializer {
                let initializer_pos = {
                    let source = self.binder.source_of_node(initializer);
                    source.arena.node(initializer).pos as usize
                };
                let start = self.utf16_position(node, initializer_pos.saturating_sub(1));
                return self.grammar_error_at_pos(
                    node,
                    start,
                    1,
                    &diagnostics::A_rest_element_cannot_have_an_initializer,
                    &[],
                );
            }
        }
        false
    }

    // ---- §2 collisions band ----
    // (checkGrammarForDisallowedTrailingComma reuses the operators.rs
    // port — same worker, 5.5e slice.)

    /// tsc-port: needCollisionCheckForIdentifier @6.0.3
    /// tsc-hash: 2b501c1d60ba012afde766e0afb362478c9dc950bfa3dfaa4fcd7da1b419911a
    /// tsc-span: _tsc.js:83239-83259
    fn need_collision_check_for_identifier(
        &self,
        node: NodeId,
        identifier: Option<NodeId>,
        name: &str,
    ) -> bool {
        let Some(identifier) = identifier else {
            return false;
        };
        if self.identifier_text_of(identifier) != Some(name) {
            return false;
        }
        let kind = self.kind_of(node);
        if matches!(
            kind,
            SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::PropertyAssignment
        ) {
            return false;
        }
        if self.node_flags(node) & NodeFlags::AMBIENT.bits() != 0 {
            return false;
        }
        // Type-only import declarations never collide (5.8d re-audits
        // the ImportSpecifier grandparent face with §9).
        // ImportEqualsDeclaration.isTypeOnly is unmodeled in the node
        // data — the whole kind skips (FN-only: real import-equals
        // collisions under sub-ES2015 module formats stay silent;
        // reporting without the type-only gate would fabricate rows on
        // `import type x = require(...)`, an FP).
        let is_type_only = match self.data_of(node) {
            NodeData::ImportClause(data) => data.is_type_only,
            NodeData::ImportEqualsDeclaration(_) => true,
            NodeData::ImportSpecifier(data) => {
                data.is_type_only
                    || self
                        .parent_of(node)
                        .and_then(|named| self.parent_of(named))
                        .is_some_and(|clause| {
                            matches!(self.data_of(clause), NodeData::ImportClause(data)
                                if data.is_type_only)
                        })
            }
            _ => false,
        };
        if is_type_only {
            return false;
        }
        let source = self.binder.source_of_node(node);
        let root = node_util::get_root_declaration(source, node);
        if self.kind_of(root) == SyntaxKind::Parameter {
            let body = self
                .parent_of(root)
                .and_then(|function| node_util::body_of(source, function));
            if node_util::node_is_missing(source, body) {
                return false;
            }
        }
        true
    }

    /// tsc-port: getDeclarationContainer @6.0.3
    /// tsc-hash: 3d4b993da842ea191877ffad47fb0c8045a3d1086066350235a4992e74413283
    /// tsc-span: _tsc.js:55784-55798
    fn get_declaration_container(&self, node: NodeId) -> Option<NodeId> {
        let source = self.binder.source_of_node(node);
        let root = node_util::get_root_declaration(source, node);
        let mut current = Some(root);
        while let Some(candidate) = current {
            match self.kind_of(candidate) {
                SyntaxKind::VariableDeclaration
                | SyntaxKind::VariableDeclarationList
                | SyntaxKind::ImportSpecifier
                | SyntaxKind::NamedImports
                | SyntaxKind::NamespaceImport
                | SyntaxKind::ImportClause => current = self.parent_of(candidate),
                _ => return self.parent_of(candidate),
            }
        }
        None
    }

    /// tsc-port: checkCollisionWithRequireExportsInGeneratedCode @6.0.3
    /// tsc-hash: 7585d27c2665e3531b6d8cfdc18e43f7a636fbebcab66fdf7dccbbfdedbd1277
    /// tsc-span: _tsc.js:83288-83302
    ///
    /// host.getEmitModuleFormatOfFile reduces to the computed module
    /// kind (impliedNodeFormat unported) — live for @module:commonjs/
    /// amd/umd/system fixtures.
    fn check_collision_with_require_exports_in_generated_code(
        &mut self,
        node: NodeId,
        name: Option<NodeId>,
    ) {
        if self.options.emit_module_kind() >= 5 {
            return;
        }
        if !self.need_collision_check_for_identifier(node, name, "require")
            && !self.need_collision_check_for_identifier(node, name, "exports")
        {
            return;
        }
        let name = name.expect("collision check implies a name");
        if self.kind_of(node) == SyntaxKind::ModuleDeclaration
            && self.module_instance_state_of(node)
                != tsrs2_binder::containers::ModuleInstanceState::Instantiated
        {
            return;
        }
        let parent = self.get_declaration_container(node);
        if let Some(parent) = parent {
            if self.kind_of(parent) == SyntaxKind::SourceFile
                && self.binder.is_external_or_common_js_module_of_node(parent)
            {
                let display = self.declaration_name_display(name);
                self.error_skipped_on_no_emit(
                    Some(name),
                    &diagnostics::Duplicate_identifier_0_Compiler_reserves_name_1_in_top_level_scope_of_a_module,
                    &[&display, &display],
                );
            }
        }
    }

    /// tsc-port: checkCollisionWithGlobalPromiseInGeneratedCode @6.0.3
    /// tsc-hash: 8e1a4a58e52e623f7f78f69a5cb6d784585246de232bb50d40e8c25d93f629ae
    /// tsc-span: _tsc.js:83303-83314
    ///
    /// languageVersion is the mapped @target — live for the ES5/
    /// ES2015/ES2016 fixture band.
    fn check_collision_with_global_promise_in_generated_code(
        &mut self,
        node: NodeId,
        name: Option<NodeId>,
    ) {
        if self.options.emit_script_target() >= tsrs2_types::ScriptTarget::ES2017 {
            return;
        }
        if !self.need_collision_check_for_identifier(node, name, "Promise") {
            return;
        }
        let name = name.expect("collision check implies a name");
        if self.kind_of(node) == SyntaxKind::ModuleDeclaration
            && self.module_instance_state_of(node)
                != tsrs2_binder::containers::ModuleInstanceState::Instantiated
        {
            return;
        }
        let parent = self.get_declaration_container(node);
        if let Some(parent) = parent {
            if self.kind_of(parent) == SyntaxKind::SourceFile
                && self.binder.is_external_or_common_js_module_of_node(parent)
                && self.node_flags(parent) & NodeFlags::HAS_ASYNC_FUNCTIONS.bits() != 0
            {
                let display = self.declaration_name_display(name);
                self.error_skipped_on_no_emit(
                    Some(name),
                    &diagnostics::Duplicate_identifier_0_Compiler_reserves_name_1_in_top_level_scope_of_a_module_containing_async_functions,
                    &[&display, &display],
                );
            }
        }
    }

    /// tsc-port: recordPotentialCollisionWithWeakMapSetInGeneratedCode @6.0.3
    /// tsc-hash: ded0b7a69791e06183aa4547440ffc3929adc95b6fd126df5bdbf506765763e0
    /// tsc-span: _tsc.js:83315-83319
    fn record_potential_collision_with_weak_map_set_in_generated_code(
        &mut self,
        node: NodeId,
        name: Option<NodeId>,
    ) {
        if self.options.emit_script_target() <= tsrs2_types::ScriptTarget::ES2021
            && (self.need_collision_check_for_identifier(node, name, "WeakMap")
                || self.need_collision_check_for_identifier(node, name, "WeakSet"))
        {
            self.potential_weak_map_set_collisions.push(node);
        }
    }

    /// tsc-port: checkWeakMapSetCollision @6.0.3
    /// tsc-hash: 663b9c8e55a84c4b6a8e057ec63434875d6d2c616a27af09a959cf7adad75ba2
    /// tsc-span: _tsc.js:83320-83326
    ///
    /// The ContainsClassWithPrivateIdentifiers pusher lands with the
    /// class band (§6, 5.8c) — until then the drain runs empty-gated.
    pub(crate) fn check_weak_map_set_collision(&mut self, node: NodeId) {
        let Some(enclosing) = self.get_enclosing_block_scope_container(node) else {
            return;
        };
        if self
            .links
            .node(enclosing)
            .check_flags
            .intersects(tsrs2_types::NodeCheckFlags::CONTAINS_CLASS_WITH_PRIVATE_IDENTIFIERS)
        {
            let Some(name) = self.name_of_node(node) else {
                return;
            };
            let Some(text) = self.identifier_text_of(name).map(str::to_owned) else {
                return;
            };
            self.error_skipped_on_no_emit(
                Some(node),
                &diagnostics::Compiler_reserves_name_0_when_emitting_private_identifier_downlevel,
                &[&text],
            );
        }
    }

    /// tsc-port: recordPotentialCollisionWithReflectInGeneratedCode @6.0.3
    /// tsc-hash: c2971c5dee0a28f522a080c72e97f03bc8b6486144cedd5f4a4456972571190f
    /// tsc-span: _tsc.js:83327-83331
    fn record_potential_collision_with_reflect_in_generated_code(
        &mut self,
        node: NodeId,
        name: Option<NodeId>,
    ) {
        let target = self.options.emit_script_target();
        if name.is_some()
            && target >= tsrs2_types::ScriptTarget::ES2015
            && target <= tsrs2_types::ScriptTarget::ES2021
            && self.need_collision_check_for_identifier(node, name, "Reflect")
        {
            self.potential_reflect_collisions.push(node);
        }
    }

    /// tsc-port: checkReflectCollision @6.0.3
    /// tsc-hash: 41e1b79eb5d7042ba44760680048e464daea7e51b5dcce1f6852611a982f4d10
    /// tsc-span: _tsc.js:83332-83355
    ///
    /// The ContainsSuperPropertyInStaticInitializer pusher is the §6
    /// static-initializer super check (5.8c) — drain runs empty-gated.
    pub(crate) fn check_reflect_collision(&mut self, node: NodeId) {
        let mut has_collision = false;
        let contains_super = |state: &Self, candidate: NodeId| {
            state.links.node(candidate).check_flags.intersects(
                tsrs2_types::NodeCheckFlags::CONTAINS_SUPER_PROPERTY_IN_STATIC_INITIALIZER,
            )
        };
        match self.kind_of(node) {
            SyntaxKind::ClassExpression => {
                let members = match self.data_of(node) {
                    NodeData::ClassExpression(data) => data.members,
                    _ => None,
                };
                for member in self.nodes_of(members) {
                    if contains_super(self, member) {
                        has_collision = true;
                        break;
                    }
                }
            }
            SyntaxKind::FunctionExpression => {
                has_collision = contains_super(self, node);
            }
            _ => {
                if let Some(container) = self.get_enclosing_block_scope_container(node) {
                    has_collision = contains_super(self, container);
                }
            }
        }
        if has_collision {
            let Some(name) = self.name_of_node(node) else {
                return;
            };
            let display = self.declaration_name_display(name);
            self.error_skipped_on_no_emit(
                Some(node),
                &diagnostics::Duplicate_identifier_0_Compiler_reserves_name_1_when_emitting_super_references_in_static_initializers,
                &[&display, "Reflect"],
            );
        }
    }

    /// tsc-port: checkCollisionsForDeclarationName @6.0.3
    /// tsc-hash: f7f1b0b6e14c00ea54e1ac7fa0972467356023c05f33deb90f426148e0406b3d
    /// tsc-span: _tsc.js:83356-83370
    pub(crate) fn check_collisions_for_declaration_name(
        &mut self,
        node: NodeId,
        name: Option<NodeId>,
    ) {
        let Some(name) = name else {
            return;
        };
        self.check_collision_with_require_exports_in_generated_code(node, Some(name));
        self.check_collision_with_global_promise_in_generated_code(node, Some(name));
        self.record_potential_collision_with_weak_map_set_in_generated_code(node, Some(name));
        self.record_potential_collision_with_reflect_in_generated_code(node, Some(name));
        match self.kind_of(node) {
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                self.check_type_name_is_reserved(name, &diagnostics::Class_name_cannot_be_0);
                if self.node_flags(node) & NodeFlags::AMBIENT.bits() == 0 {
                    self.check_class_name_collision_with_object(name);
                }
            }
            SyntaxKind::EnumDeclaration => {
                self.check_type_name_is_reserved(name, &diagnostics::Enum_name_cannot_be_0);
            }
            _ => {}
        }
    }

    /// tsc-port: checkClassNameCollisionWithObject @6.0.3
    /// tsc-hash: bb5b14d2ca4a41c446f41c4569b2f7d5769904ea87ef5a7d801d8061ab8a3173
    /// tsc-span: _tsc.js:84787-84791
    fn check_class_name_collision_with_object(&mut self, name: NodeId) {
        if self.identifier_text_of(name) != Some("Object") {
            return;
        }
        let module_kind = self.options.emit_module_kind();
        if module_kind < 5 {
            let module_name = match module_kind {
                0 => "None",
                1 => "CommonJS",
                2 => "AMD",
                3 => "UMD",
                4 => "System",
                _ => unreachable!("module kinds below ES2015"),
            };
            self.error_at(
                Some(name),
                &diagnostics::Class_name_cannot_be_Object_when_targeting_ES5_and_above_with_module_0,
                &[module_name],
            );
        }
    }

    /// tsc-port: checkIfThisIsCapturedInEnclosingScope @6.0.3
    /// tsc-hash: a0778475684c83db348bd50b2c16d14e346dfcfaa6406d40bfd9cb831a6bf8ab
    /// tsc-span: _tsc.js:83260-83273
    ///
    /// The CaptureThis pusher is a downlevel-emit path (checkThis
    /// Expression at target<ES2015) — no pusher exists yet, the drain
    /// runs empty-gated (§0 note).
    pub(crate) fn check_if_this_is_captured_in_enclosing_scope(&mut self, node: NodeId) {
        let mut current = Some(node);
        while let Some(candidate) = current {
            if self
                .links
                .node(candidate)
                .check_flags
                .intersects(tsrs2_types::NodeCheckFlags::CAPTURE_THIS)
            {
                let is_declaration = self.kind_of(node) != SyntaxKind::Identifier;
                if is_declaration {
                    let name = self.name_of_node(node);
                    self.error_at(
                        name.or(Some(node)),
                        &diagnostics::Duplicate_identifier_this_Compiler_uses_variable_declaration_this_to_capture_this_reference,
                        &[],
                    );
                } else {
                    self.error_at(
                        Some(node),
                        &diagnostics::Expression_resolves_to_variable_declaration_this_that_compiler_uses_to_capture_this_reference,
                        &[],
                    );
                }
                return;
            }
            current = self.parent_of(candidate);
        }
    }

    /// tsc-port: checkIfNewTargetIsCapturedInEnclosingScope @6.0.3
    /// tsc-hash: 091969b9deafea358ca65dafeda11096ed7d035c773af41e4e2c9f090d6052fa
    /// tsc-span: _tsc.js:83274-83287
    ///
    /// CaptureNewTarget pusher: downlevel-emit path — drain runs
    /// empty-gated (§0 note).
    pub(crate) fn check_if_new_target_is_captured_in_enclosing_scope(&mut self, node: NodeId) {
        let mut current = Some(node);
        while let Some(candidate) = current {
            if self
                .links
                .node(candidate)
                .check_flags
                .intersects(tsrs2_types::NodeCheckFlags::CAPTURE_NEW_TARGET)
            {
                let is_declaration = self.kind_of(node) != SyntaxKind::Identifier;
                if is_declaration {
                    let name = self.name_of_node(node);
                    self.error_at(
                        name.or(Some(node)),
                        &diagnostics::Duplicate_identifier_newTarget_Compiler_uses_variable_declaration_newTarget_to_capture_new_target_meta_property_reference,
                        &[],
                    );
                } else {
                    self.error_at(
                        Some(node),
                        &diagnostics::Expression_resolves_to_variable_declaration_newTarget_that_compiler_uses_to_capture_new_target_meta_property_reference,
                        &[],
                    );
                }
                return;
            }
            current = self.parent_of(candidate);
        }
    }

    /// tsc-port: checkPotentialUncheckedRenamedBindingElementsInTypes @6.0.3
    /// tsc-hash: d19fef7ffa3fbfd8ff4eadcecd861fe8b1cd46745bbe946ed3d4b8ff6ae879d0
    /// tsc-span: _tsc.js:83180-83196
    ///
    /// The isReferenced gate (risk §14.16) reads the resolver-written
    /// SymbolLinks bit (resolve.rs 19767-twin).
    pub(crate) fn check_potential_unchecked_renamed_binding_elements_in_types(&mut self) {
        let recorded = self
            .potential_unused_renamed_binding_elements_in_types
            .clone();
        for node in recorded {
            let Ok(symbol) = self.get_symbol_of_declaration(node) else {
                continue;
            };
            if self.links.symbol(symbol).is_referenced {
                continue;
            }
            let source = self.binder.source_of_node(node);
            let Some(wrapping) = node_util::walk_up_binding_elements_and_patterns(source, node)
            else {
                continue;
            };
            let (name, property_name) = match self.data_of(node) {
                NodeData::BindingElement(data) => (data.name, data.property_name),
                _ => (None, None),
            };
            let (Some(name), Some(property_name)) = (name, property_name) else {
                continue;
            };
            let name_display = self.declaration_name_display(name);
            let property_display = self.declaration_name_display(property_name);
            let mut diagnostic = self.diagnostic_for_node(
                name,
                &diagnostics::_0_is_an_unused_renaming_of_1_Did_you_intend_to_use_it_as_a_type_annotation,
                &[&name_display, &property_display],
            );
            let wrapping_annotation = self.type_annotation_of(wrapping);
            if wrapping_annotation.is_none() {
                let source = self.binder.source_of_node(wrapping);
                let end_utf16 =
                    self.utf16_position(wrapping, source.arena.node(wrapping).end as usize);
                diagnostic.related.push(tsrs2_diags::RelatedInfo {
                    file_name: Some(source.file_name.clone()),
                    start: Some(end_utf16),
                    length: Some(0),
                    message: MessageChain::new(
                        &diagnostics::We_can_only_write_a_type_for_0_by_adding_a_type_for_the_entire_parameter_here,
                        std::slice::from_ref(&property_display),
                    ),
                });
            }
            self.push_error_diagnostic(diagnostic);
        }
    }

    /// [FLOW M5] declaration-row gate (the 5.5e second-face pattern):
    /// tsc relates the FLOW type of the initializer — a failed verdict
    /// over DECLARED types is tsc-clean whenever any narrowable
    /// reference feeds the initializer (assignment/guard narrowing).
    /// Contain those reports; M5 removes the gate.
    fn declaration_initializer_flow_gate(
        &mut self,
        initializer: NodeId,
        initializer_type: TypeId,
        target: TypeId,
    ) -> CheckResult2<()> {
        if self.is_type_assignable_to(initializer_type, target)? {
            return Ok(());
        }
        if self.subtree_mentions_narrowable_reference(initializer) {
            return Err(Unsupported::new(
                "[FLOW M5] failed declaration initializer over a narrowable reference",
            ));
        }
        Ok(())
    }

    /// Any descendant (or the node itself) is a narrowable reference —
    /// the broad FP=0-side probe feeding the declaration-row gate.
    fn subtree_mentions_narrowable_reference(&self, root: NodeId) -> bool {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let source = self.binder.source_of_node(node);
            if node_util::is_narrowable_reference(source, node) {
                return true;
            }
            let mut children = Vec::new();
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
                children.push(child);
                false
            });
            stack.extend(children);
        }
        false
    }

    // ---- shared small helpers ----

    /// tsc-port: hasOnlyExpressionInitializer @6.0.3
    /// tsc-hash: 56782551f781d46f793251ce5a948ea4bfc97074c0912263c4f57f9969f36c3a
    /// tsc-span: _tsc.js:12545-12557
    fn only_expression_initializer_of(&self, node: NodeId) -> Option<NodeId> {
        match self.kind_of(node) {
            SyntaxKind::VariableDeclaration
            | SyntaxKind::Parameter
            | SyntaxKind::BindingElement
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertyAssignment
            | SyntaxKind::EnumMember => self.initializer_of_node(node),
            _ => None,
        }
    }

    /// The declaration-kind initializer field (getEffectiveInitializer
    /// reduces to node.initializer in TS files).
    fn initializer_of_node(&self, node: NodeId) -> Option<NodeId> {
        match self.data_of(node) {
            NodeData::VariableDeclaration(data) => data.initializer,
            NodeData::Parameter(data) => data.initializer,
            NodeData::BindingElement(data) => data.initializer,
            NodeData::PropertyDeclaration(data) => data.initializer,
            NodeData::PropertyAssignment(data) => data.initializer,
            NodeData::EnumMember(data) => data.initializer,
            _ => None,
        }
    }

    /// tsc-port: isVariableLike @6.0.3
    /// tsc-hash: 9fa304f6057247becbdf6555219a6fdc01a2f305bfd1babebbbfe887d5d03180
    /// tsc-span: _tsc.js:14350-14365
    fn is_variable_like_declaration_kind(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::BindingElement
                | SyntaxKind::EnumMember
                | SyntaxKind::Parameter
                | SyntaxKind::PropertyAssignment
                | SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::ShorthandPropertyAssignment
                | SyntaxKind::VariableDeclaration
        )
    }

    /// tsc hasQuestionToken — the declaration kinds carrying one.
    fn has_question_token(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::Parameter(data) => data.question_token.is_some(),
            NodeData::PropertyDeclaration(data) => data.question_token.is_some(),
            NodeData::PropertySignature(data) => data.question_token.is_some(),
            NodeData::MethodDeclaration(data) => data.question_token.is_some(),
            NodeData::MethodSignature(data) => data.question_token.is_some(),
            NodeData::ShorthandPropertyAssignment(data) => data.question_token.is_some(),
            NodeData::PropertyAssignment(data) => data.question_token.is_some(),
            _ => false,
        }
    }

    /// nodeIsMissing(getContainingFunction(node).body) — the shared
    /// signature-only-parameter probe.
    fn containing_function_body_is_missing(&self, node: NodeId) -> bool {
        let Some(function) = self.get_containing_function(node) else {
            return false;
        };
        let source = self.binder.source_of_node(function);
        node_util::node_is_missing(source, node_util::body_of(source, function))
    }

    /// The binding-pattern element list (Object/Array patterns).
    fn binding_pattern_elements(&self, pattern: NodeId) -> Vec<NodeId> {
        match self.data_of(pattern) {
            NodeData::ObjectBindingPattern(data) => self.nodes_of(data.elements),
            NodeData::ArrayBindingPattern(data) => self.nodes_of(data.elements),
            _ => Vec::new(),
        }
    }

    /// declarationNameToString: source text of the name node.
    fn declaration_name_display(&self, name: NodeId) -> String {
        let source = self.binder.source_of_node(name);
        node_util::declaration_name_to_string(source, Some(name))
    }

    /// getAncestor(node, kind): the parent-chain probe.
    fn ancestor_of_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(candidate) = current {
            if self.kind_of(candidate) == kind {
                return Some(candidate);
            }
            current = self.parent_of(candidate);
        }
        None
    }

    /// Byte offset → UTF-16 diagnostic position for the file owning
    /// `node`.
    fn utf16_position(&self, node: NodeId, byte: usize) -> u32 {
        let source = self.binder.source_of_node(node);
        source
            .line_map
            .byte_to_utf16
            .get(byte)
            .copied()
            .unwrap_or(byte as u32)
    }

    /// tsc getContainingFunctionOrClassStaticBlock (14612): nearest
    /// function-like or class-static-block ancestor.
    fn get_containing_function_or_class_static_block(&self, node: NodeId) -> Option<NodeId> {
        let mut current = self.parent_of(node);
        while let Some(candidate) = current {
            if node_util::is_function_like_kind(self.kind_of(candidate))
                || self.kind_of(candidate) == SyntaxKind::ClassStaticBlockDeclaration
            {
                return Some(candidate);
            }
            current = self.parent_of(candidate);
        }
        None
    }

    /// getModuleInstanceState through the binder's cached walk.
    fn module_instance_state_of(
        &self,
        node: NodeId,
    ) -> tsrs2_binder::containers::ModuleInstanceState {
        let source = self.binder.source_of_node(node);
        let mut visited = std::collections::HashMap::new();
        tsrs2_binder::containers::get_module_instance_state(source, node, &mut visited)
    }
}

// ---- §3 control statements ----
// (The truthiness kit + the known-truthy-callable/awaitable/enum
// condition band live in operators.rs — the 5.5e conditional-
// expression face; §3's statement callers route there.)

impl<'a> CheckerState<'a> {
    /// tsc-port: checkIfStatement @6.0.3
    /// tsc-hash: 2b63e389f5aac26a193f562defbd10f1bf73b620ec6293432cd8ab8318954da4
    /// tsc-span: _tsc.js:83626-83635
    pub(crate) fn check_if_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::IfStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (expression, then_statement, else_statement) =
            (data.expression, data.then_statement, data.else_statement);
        let Some(expression) = expression else {
            return Err(Unsupported::new("IfStatement recovery node"));
        };
        let ty = self.check_truthiness_expression(expression, CheckMode::NORMAL)?;
        self.check_testing_known_truthy_callable_or_awaitable_or_enum_member_type(
            expression,
            ty,
            then_statement,
        )?;
        self.check_source_element(then_statement);
        if let Some(then_statement) = then_statement {
            if self.kind_of(then_statement) == SyntaxKind::EmptyStatement {
                self.error_at(
                    Some(then_statement),
                    &diagnostics::The_body_of_an_if_statement_cannot_be_the_empty_statement,
                    &[],
                );
            }
        }
        self.check_source_element(else_statement);
        Ok(())
    }

    /// tsc-port: checkDoStatement @6.0.3
    /// tsc-hash: 57686e3d150a3f2ac5f1e97af5f1337d8bbf1461eda3360d5c2808e93744e0b2
    /// tsc-span: _tsc.js:83738-83742
    pub(crate) fn check_do_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::DoStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (statement, expression) = (data.statement, data.expression);
        self.check_source_element(statement);
        if let Some(expression) = expression {
            self.check_truthiness_expression(expression, CheckMode::NORMAL)?;
        }
        Ok(())
    }

    /// tsc-port: checkWhileStatement @6.0.3
    /// tsc-hash: 241a76e376f4046953222221e540da949932dfe134639342bf04abed3629c625
    /// tsc-span: _tsc.js:83743-83747
    pub(crate) fn check_while_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::WhileStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (expression, statement) = (data.expression, data.statement);
        if let Some(expression) = expression {
            self.check_truthiness_expression(expression, CheckMode::NORMAL)?;
        }
        self.check_source_element(statement);
        Ok(())
    }

    /// tsc-port: checkForStatement @6.0.3
    /// tsc-hash: 856acc0f28558cdfa2066735ebf036025b3617b8443b9a2576c083024403149c
    /// tsc-span: _tsc.js:83799-83818
    ///
    /// registerForUnusedIdentifiersCheck is M7-inert.
    pub(crate) fn check_for_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::ForStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (initializer, condition, incrementor, statement) = (
            data.initializer,
            data.condition,
            data.incrementor,
            data.statement,
        );
        if !self.check_grammar_statement_in_ambient_context_reported(node) {
            if let Some(initializer) = initializer {
                if self.kind_of(initializer) == SyntaxKind::VariableDeclarationList {
                    self.check_grammar_variable_declaration_list(initializer)?;
                }
            }
        }
        if let Some(initializer) = initializer {
            if self.kind_of(initializer) == SyntaxKind::VariableDeclarationList {
                self.check_variable_declaration_list(initializer)?;
            } else {
                self.check_expression(initializer, CheckMode::NORMAL)?;
            }
        }
        if let Some(condition) = condition {
            self.check_truthiness_expression(condition, CheckMode::NORMAL)?;
        }
        if let Some(incrementor) = incrementor {
            self.check_expression(incrementor, CheckMode::NORMAL)?;
        }
        self.check_source_element(statement);
        Ok(())
    }

    /// tsc-port: checkForOfStatement @6.0.3
    /// tsc-hash: 08b449c328a80dc775d3d2a2a8ed70659946c2d654441989dbfd0a2a42f496ff
    /// tsc-span: _tsc.js:83819-83857
    ///
    /// ITERATION SEMANTICS ESCAPE to 5.8b (§15): the grammar rows and
    /// the declaration-list LHS are live; checkRightHandSideOfForOf
    /// and the expression-LHS arms that consume its type are silent
    /// stubs so the BODY stays checked (per-element containment would
    /// silence it). Emit-helper probes elided (module note).
    pub(crate) fn check_for_of_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_for_in_or_for_of_statement(node)?;
        let NodeData::ForOfStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (await_modifier, initializer, statement) =
            (data.await_modifier, data.initializer, data.statement);
        if let Some(await_modifier) = await_modifier {
            let container = self.get_containing_function_or_class_static_block(node);
            if container.is_some_and(|container| {
                self.kind_of(container) == SyntaxKind::ClassStaticBlockDeclaration
            }) {
                self.grammar_error_on_node(
                    await_modifier,
                    &diagnostics::for_await_loops_cannot_be_used_inside_a_class_static_block,
                    &[],
                );
            }
            // (else: the ForAwaitOf emit-helper probe — elided.)
        }
        // (downlevelIteration ForOf emit-helper probe — elided.)
        if let Some(initializer) = initializer {
            if self.kind_of(initializer) == SyntaxKind::VariableDeclarationList {
                self.check_variable_declaration_list(initializer)?;
            } else {
                // varExpr arm: iteratedType = checkRightHandSideOfForOf
                // FIRST in tsc; the protocol is §4 (5.8b), so the
                // destructuring/LHS-relation consumers stub with it.
                self.source_element_stub("checkRightHandSideOfForOf", "5.8b")?;
                if matches!(
                    self.kind_of(initializer),
                    SyntaxKind::ArrayLiteralExpression | SyntaxKind::ObjectLiteralExpression
                ) {
                    // checkDestructuringAssignment(varExpr,
                    // iteratedType || errorType) needs the iterated
                    // type — running it against a stand-in could
                    // fabricate rows; whole arm stubs (FN).
                    self.source_element_stub("checkDestructuringAssignment (for-of LHS)", "5.8b")?;
                } else {
                    let _left_type = self.check_expression(initializer, CheckMode::NORMAL)?;
                    self.check_reference_expression(
                        initializer,
                        &diagnostics::The_left_hand_side_of_a_for_of_statement_must_be_a_variable_or_a_property_access,
                        &diagnostics::The_left_hand_side_of_a_for_of_statement_may_not_be_an_optional_property_access,
                    );
                    // (iteratedType → leftType relation — 5.8b.)
                }
            }
        }
        self.check_source_element(statement);
        Ok(())
    }

    /// tsc-port: checkForInStatement @6.0.3
    /// tsc-hash: 61579064b6101c0d86e57e90f1c2cf86ab07a63e8c92f14f8af3b9d4c4fadca1
    /// tsc-span: _tsc.js:83858-83889
    pub(crate) fn check_for_in_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_for_in_or_for_of_statement(node)?;
        let NodeData::ForInStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (initializer, expression, statement) =
            (data.initializer, data.expression, data.statement);
        let Some(expression) = expression else {
            return Err(Unsupported::new("ForInStatement recovery node"));
        };
        let raw_right = self.check_expression(expression, CheckMode::NORMAL)?;
        let right_type = self.get_non_nullable_type_if_needed(raw_right)?;
        if let Some(initializer) = initializer {
            if self.kind_of(initializer) == SyntaxKind::VariableDeclarationList {
                let declarations = match self.data_of(initializer) {
                    NodeData::VariableDeclarationList(data) => self.nodes_of(data.declarations),
                    _ => Vec::new(),
                };
                if let Some(&variable) = declarations.first() {
                    if let Some(name) = self.name_of_node(variable) {
                        if node_util::is_binding_pattern(self.binder.source_of_node(name), name) {
                            self.error_at(
                                Some(name),
                                &diagnostics::The_left_hand_side_of_a_for_in_statement_cannot_be_a_destructuring_pattern,
                                &[],
                            );
                        }
                    }
                }
                self.check_variable_declaration_list(initializer)?;
            } else {
                let left_type = self.check_expression(initializer, CheckMode::NORMAL)?;
                if matches!(
                    self.kind_of(initializer),
                    SyntaxKind::ArrayLiteralExpression | SyntaxKind::ObjectLiteralExpression
                ) {
                    self.error_at(
                        Some(initializer),
                        &diagnostics::The_left_hand_side_of_a_for_in_statement_cannot_be_a_destructuring_pattern,
                        &[],
                    );
                } else {
                    let index_or_string = self.get_index_type_or_string(right_type)?;
                    if !self.is_type_assignable_to(index_or_string, left_type)? {
                        self.error_at(
                            Some(initializer),
                            &diagnostics::The_left_hand_side_of_a_for_in_statement_must_be_of_type_string_or_any,
                            &[],
                        );
                    } else {
                        self.check_reference_expression(
                            initializer,
                            &diagnostics::The_left_hand_side_of_a_for_in_statement_must_be_a_variable_or_a_property_access,
                            &diagnostics::The_left_hand_side_of_a_for_in_statement_may_not_be_an_optional_property_access,
                        );
                    }
                }
            }
        }
        if right_type == self.tables.intrinsics.never
            || !self.is_type_assignable_to_kind(
                right_type,
                TypeFlags::NON_PRIMITIVE | TypeFlags::INSTANTIABLE_NON_PRIMITIVE,
                /*strict*/ false,
            )?
        {
            let display = self.type_to_string_slice(right_type)?;
            self.error_at(
                Some(expression),
                &diagnostics::The_right_hand_side_of_a_for_in_statement_must_be_of_type_any_an_object_type_or_a_type_parameter_but_here_has_type_0,
                &[&display],
            );
        }
        self.check_source_element(statement);
        Ok(())
    }

    /// tsc-port: checkGrammarForInOrForOfStatement @6.0.3
    /// tsc-hash: 112c35d2e30e5202b18a4ba5e9304743e63fd5a059c5d4cae87ee0fefebde9f0
    /// tsc-span: _tsc.js:89761-89842
    ///
    /// The Node16..NodeNext moduleKind arms need impliedNodeFormat
    /// (module resolution, 5.8d) — treated as the non-CommonJS
    /// fallthrough (true-CJS node-flavor fixtures FN the 1432 row);
    /// same disposition as checkAwaitGrammar's ladder (functions.rs).
    fn check_grammar_for_in_or_for_of_statement(&mut self, node: NodeId) -> CheckResult2<bool> {
        if self.check_grammar_statement_in_ambient_context_reported(node) {
            return Ok(true);
        }
        let is_for_of = self.kind_of(node) == SyntaxKind::ForOfStatement;
        let (await_modifier, initializer) = match self.data_of(node) {
            NodeData::ForOfStatement(data) => (data.await_modifier, data.initializer),
            NodeData::ForInStatement(data) => (None, data.initializer),
            _ => unreachable!("kind/data agree"),
        };
        if is_for_of {
            if let Some(await_modifier) = await_modifier {
                if self.node_flags(node) & NodeFlags::AWAIT_CONTEXT.bits() == 0 {
                    let source = self.binder.source_of_node(node);
                    if node_util::is_in_top_level_context(source, node) {
                        if !self.has_parse_diagnostics(node) {
                            if !self.is_effective_external_module(node) {
                                self.error_at(
                                    Some(await_modifier),
                                    &diagnostics::for_await_loops_are_only_allowed_at_the_top_level_of_a_file_when_that_file_is_a_module_but_this_file_has_no_imports_or_exports_Consider_adding_an_empty_export_to_make_this_file_a_module,
                                    &[],
                                );
                            }
                            let module_kind = self.options.emit_module_kind();
                            let target_ok = self.options.emit_script_target()
                                >= tsrs2_types::ScriptTarget::ES2017;
                            let ladder_ok = match module_kind {
                                // Node16/18/20/NodeNext: impliedNodeFormat
                                // unported — non-CJS fallthrough.
                                100 | 101 | 102 | 199 => target_ok,
                                // ES2022/ESNext/Preserve/System.
                                7 | 99 | 200 | 4 => target_ok,
                                _ => false,
                            };
                            if !ladder_ok {
                                self.error_at(
                                    Some(await_modifier),
                                    &diagnostics::Top_level_for_await_loops_are_only_allowed_when_the_module_option_is_set_to_es2022_esnext_system_node16_node18_node20_nodenext_or_preserve_and_the_target_option_is_set_to_es2017_or_higher,
                                    &[],
                                );
                            }
                        }
                    } else if !self.has_parse_diagnostics(node) {
                        let func = self.get_containing_function(node);
                        let related = func
                            .filter(|&func| self.kind_of(func) != SyntaxKind::Constructor)
                            .map(|func| {
                                self.related_info_for_node(
                                    func,
                                    &diagnostics::Did_you_mean_to_mark_this_function_as_async,
                                    &[],
                                )
                            })
                            .into_iter()
                            .collect();
                        self.error_at_with_related(
                            Some(await_modifier),
                            &diagnostics::for_await_loops_are_only_allowed_within_async_functions_and_at_the_top_levels_of_modules,
                            &[],
                            related,
                        );
                        return Ok(true);
                    }
                }
            }
        }
        let Some(initializer) = initializer else {
            return Ok(false);
        };
        if is_for_of
            && self.node_flags(node) & NodeFlags::AWAIT_CONTEXT.bits() == 0
            && self.kind_of(initializer) == SyntaxKind::Identifier
            && self.identifier_text_of(initializer) == Some("async")
        {
            self.grammar_error_on_node(
                initializer,
                &diagnostics::The_left_hand_side_of_a_for_of_statement_may_not_be_async,
                &[],
            );
            return Ok(false);
        }
        if self.kind_of(initializer) == SyntaxKind::VariableDeclarationList
            && !self.check_grammar_variable_declaration_list(initializer)?
        {
            let declarations = match self.data_of(initializer) {
                NodeData::VariableDeclarationList(data) => self.nodes_of(data.declarations),
                _ => Vec::new(),
            };
            if declarations.is_empty() {
                return Ok(false);
            }
            if declarations.len() > 1 {
                let message = if self.kind_of(node) == SyntaxKind::ForInStatement {
                    &diagnostics::Only_a_single_variable_declaration_is_allowed_in_a_for_in_statement
                } else {
                    &diagnostics::Only_a_single_variable_declaration_is_allowed_in_a_for_of_statement
                };
                return Ok(self.grammar_error_on_first_token(declarations[1], message, &[]));
            }
            let first = declarations[0];
            if self.initializer_of_node(first).is_some() {
                let message = if self.kind_of(node) == SyntaxKind::ForInStatement {
                    &diagnostics::The_variable_declaration_of_a_for_in_statement_cannot_have_an_initializer
                } else {
                    &diagnostics::The_variable_declaration_of_a_for_of_statement_cannot_have_an_initializer
                };
                let name = self.name_of_node(first);
                return Ok(self.grammar_error_on_node(name.unwrap_or(first), message, &[]));
            }
            if self.type_annotation_of(first).is_some() {
                let message = if self.kind_of(node) == SyntaxKind::ForInStatement {
                    &diagnostics::The_left_hand_side_of_a_for_in_statement_cannot_use_a_type_annotation
                } else {
                    &diagnostics::The_left_hand_side_of_a_for_of_statement_cannot_use_a_type_annotation
                };
                return Ok(self.grammar_error_on_node(first, message, &[]));
            }
        }
        Ok(false)
    }

    /// checkGrammarStatementInAmbientContext as a REPORTING probe: the
    /// worker in check.rs returns (), but the §3 callers branch on
    /// "did the ambient arm report" — mirror tsc's boolean by
    /// re-testing the ambient condition.
    fn check_grammar_statement_in_ambient_context_reported(&mut self, node: NodeId) -> bool {
        let before = self.diagnostics.len();
        self.check_grammar_statement_in_ambient_context(node);
        // tsc returns true only when IT reported (or the once-latch
        // already fired for this context) — approximating with "a new
        // diagnostic appeared" keeps the && chains correct except for
        // the latched second statement, where tsc returns FALSE and
        // falls through to the guarded grammar checks. Re-test the
        // ambient flag for that case.
        if self.diagnostics.len() > before {
            return true;
        }
        false
    }

    /// tsc-port: checkBreakOrContinueStatement @6.0.3
    /// tsc-hash: 0b9c72fdae9697d6afec768c0ed6ac98460c39571f2911d288f078e4c353afed
    /// tsc-span: _tsc.js:84497-84499
    pub(crate) fn check_break_or_continue_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        if !self.check_grammar_statement_in_ambient_context_reported(node) {
            self.check_grammar_break_or_continue_statement(node);
        }
        Ok(())
    }

    /// tsc-port: checkGrammarBreakOrContinueStatement @6.0.3
    /// tsc-hash: 23f3be29200d64ed5a6497e3ead4bfa1cd9c39f532f41b58245bb3d2d30c8947
    /// tsc-span: _tsc.js:89978-90022
    fn check_grammar_break_or_continue_statement(&mut self, node: NodeId) -> bool {
        let label = match self.data_of(node) {
            NodeData::BreakStatement(data) => data.label,
            NodeData::ContinueStatement(data) => data.label,
            _ => None,
        };
        let label_text = label.and_then(|label| self.identifier_text_of(label).map(str::to_owned));
        let is_break = self.kind_of(node) == SyntaxKind::BreakStatement;
        let mut current = Some(node);
        while let Some(candidate) = current {
            let kind = self.kind_of(candidate);
            if node_util::is_function_like_kind(kind)
                || kind == SyntaxKind::ClassStaticBlockDeclaration
            {
                return self.grammar_error_on_node(
                    node,
                    &diagnostics::Jump_target_cannot_cross_function_boundary,
                    &[],
                );
            }
            match kind {
                SyntaxKind::LabeledStatement => {
                    let (current_label, current_statement) = match self.data_of(candidate) {
                        NodeData::LabeledStatement(data) => (data.label, data.statement),
                        _ => (None, None),
                    };
                    let labels_match = label.is_some()
                        && current_label
                            .and_then(|l| self.identifier_text_of(l).map(str::to_owned))
                            == label_text;
                    if labels_match {
                        let is_misplaced_continue = !is_break
                            && !current_statement.is_some_and(|statement| {
                                self.is_iteration_statement(statement, true)
                            });
                        if is_misplaced_continue {
                            return self.grammar_error_on_node(
                                node,
                                &diagnostics::A_continue_statement_can_only_jump_to_a_label_of_an_enclosing_iteration_statement,
                                &[],
                            );
                        }
                        return false;
                    }
                }
                SyntaxKind::SwitchStatement => {
                    if is_break && label.is_none() {
                        return false;
                    }
                }
                _ => {
                    if self.is_iteration_statement(candidate, false) && label.is_none() {
                        return false;
                    }
                }
            }
            current = self.parent_of(candidate);
        }
        if label.is_some() {
            let message = if is_break {
                &diagnostics::A_break_statement_can_only_jump_to_a_label_of_an_enclosing_statement
            } else {
                &diagnostics::A_continue_statement_can_only_jump_to_a_label_of_an_enclosing_iteration_statement
            };
            self.grammar_error_on_node(node, message, &[])
        } else {
            let message = if is_break {
                &diagnostics::A_break_statement_can_only_be_used_within_an_enclosing_iteration_or_switch_statement
            } else {
                &diagnostics::A_continue_statement_can_only_be_used_within_an_enclosing_iteration_statement
            };
            self.grammar_error_on_node(node, message, &[])
        }
    }

    /// tsc-port: isIterationStatement @6.0.3
    /// tsc-hash: e3c007a9658a14db85ec10eac1f2ddfc6778a433592040d024484dd5d7fe180d
    /// tsc-span: _tsc.js:12302-12314
    fn is_iteration_statement(&self, node: NodeId, look_in_labeled_statements: bool) -> bool {
        match self.kind_of(node) {
            SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::DoStatement
            | SyntaxKind::WhileStatement => true,
            SyntaxKind::LabeledStatement if look_in_labeled_statements => {
                match self.data_of(node) {
                    NodeData::LabeledStatement(data) => data
                        .statement
                        .is_some_and(|statement| self.is_iteration_statement(statement, true)),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// tsc-port: checkReturnStatement @6.0.3
    /// tsc-hash: c9a0f8abcefe176817b5c00491ec3ea7e140cff4d2eb807bc02a925559136718
    /// tsc-span: _tsc.js:84516-84549
    ///
    /// noImplicitReturns is absent from CompilerOptions — the
    /// Not_all_code_paths_return_a_value arm stays dead (§13 audit).
    /// unwrapReturnType's generator arm escapes [ITER] (5.8b).
    pub(crate) fn check_return_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        if self.check_grammar_statement_in_ambient_context_reported(node) {
            return Ok(());
        }
        let NodeData::ReturnStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression;
        let container = self.get_containing_function_or_class_static_block(node);
        if container.is_some_and(|container| {
            self.kind_of(container) == SyntaxKind::ClassStaticBlockDeclaration
        }) {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::A_return_statement_cannot_be_used_inside_a_class_static_block,
                &[],
            );
            return Ok(());
        }
        let Some(container) = container else {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::A_return_statement_can_only_be_used_within_a_function_body,
                &[],
            );
            return Ok(());
        };
        let signature = self.get_signature_from_declaration(container)?;
        let return_type = self.get_return_type_of_signature(signature)?;
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        if strict_null_checks
            || expression.is_some()
            || self
                .tables
                .flags_of(return_type)
                .intersects(TypeFlags::NEVER)
        {
            let expr_type = match expression {
                Some(expression) => self.check_expression_cached(expression, CheckMode::NORMAL)?,
                None => self.tables.intrinsics.undefined,
            };
            if self.kind_of(container) == SyntaxKind::SetAccessor {
                if expression.is_some() {
                    self.error_at(Some(node), &diagnostics::Setters_cannot_return_a_value, &[]);
                }
            } else if self.kind_of(container) == SyntaxKind::Constructor {
                if expression.is_some() {
                    // Both rows land in one baseline: the relation's
                    // own failure report plus the 2409 head.
                    if !self.check_type_assignable_to(
                        expr_type,
                        return_type,
                        Some(node),
                        &diagnostics::Type_0_is_not_assignable_to_type_1,
                    )? {
                        self.error_at(
                            Some(node),
                            &diagnostics::Return_type_of_constructor_signature_must_be_assignable_to_the_instance_type_of_the_class,
                            &[],
                        );
                    }
                }
            } else if self.get_return_type_from_annotation(container)?.is_some() {
                let function_flags = self.get_function_flags(container);
                let unwrapped_return_type = self.unwrap_return_type(return_type, function_flags)?;
                self.check_return_expression(
                    container,
                    unwrapped_return_type,
                    node,
                    expression,
                    expr_type,
                    /*in_conditional_expression*/ false,
                )?;
            }
        }
        // (noImplicitReturns arm — option absent, dead.)
        Ok(())
    }

    /// tsc-port: checkWithStatement @6.0.3
    /// tsc-hash: 0d7c572d00ec056a1c58e81966d25476549d8e0391fb797d6ae94f01152f2c9f
    /// tsc-span: _tsc.js:84589-84602
    ///
    /// node.statement is NOT checked — with bodies are unchecked.
    pub(crate) fn check_with_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        if !self.check_grammar_statement_in_ambient_context_reported(node)
            && self.node_flags(node) & NodeFlags::AWAIT_CONTEXT.bits() != 0
        {
            self.grammar_error_on_first_token(
                node,
                &diagnostics::with_statements_are_not_allowed_in_an_async_function_block,
                &[],
            );
        }
        let NodeData::WithStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (expression, statement) = (data.expression, data.statement);
        if let Some(expression) = expression {
            self.check_expression(expression, CheckMode::NORMAL)?;
        }
        if !self.has_parse_diagnostics(node) {
            let source = self.binder.source_of_node(node);
            let node_pos = source.arena.node(node).pos as usize;
            let (token_start, _token_end) =
                node_util::get_span_of_token_at_position(source, node_pos);
            let statement_pos = statement
                .map(|statement| source.arena.node(statement).pos as usize)
                .unwrap_or(token_start);
            let start = self.utf16_position(node, token_start);
            let end = self.utf16_position(node, statement_pos);
            self.grammar_error_at_pos(
                node,
                start,
                end.saturating_sub(start),
                &diagnostics::The_with_statement_is_not_supported_All_symbols_in_a_with_block_will_have_type_any,
                &[],
            );
        }
        Ok(())
    }

    /// tsc-port: checkSwitchStatement @6.0.3
    /// tsc-hash: 70c9af565a281e020c5512e951d427e675e7ebe8f53db1409becb949363d98d1
    /// tsc-span: _tsc.js:84603-84642
    ///
    /// noFallthroughCasesInSwitch is modeled but its arm needs M5's
    /// isReachableFlowNode — dead until then (§0 note). caseBlock
    /// locals registration is M7-inert.
    pub(crate) fn check_switch_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::SwitchStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (expression, case_block) = (data.expression, data.case_block);
        let Some(expression) = expression else {
            return Err(Unsupported::new("SwitchStatement recovery node"));
        };
        let mut first_default_clause: Option<NodeId> = None;
        let mut has_duplicate_default_clause = false;
        let expression_type = self.check_expression(expression, CheckMode::NORMAL)?;
        let clauses = case_block
            .map(|case_block| match self.data_of(case_block) {
                NodeData::CaseBlock(data) => self.nodes_of(data.clauses),
                _ => Vec::new(),
            })
            .unwrap_or_default();
        for clause in clauses {
            if self.kind_of(clause) == SyntaxKind::DefaultClause && !has_duplicate_default_clause {
                if first_default_clause.is_none() {
                    first_default_clause = Some(clause);
                } else {
                    self.grammar_error_on_node(
                        clause,
                        &diagnostics::A_default_clause_cannot_appear_more_than_once_in_a_switch_statement,
                        &[],
                    );
                    has_duplicate_default_clause = true;
                }
            }
            if self.kind_of(clause) == SyntaxKind::CaseClause {
                // addLazyDiagnostic = eager identity (5.4 decision).
                let clause_expression = match self.data_of(clause) {
                    NodeData::CaseClause(data) => data.expression,
                    _ => None,
                };
                if let Some(clause_expression) = clause_expression {
                    let case_type = self.check_expression(clause_expression, CheckMode::NORMAL)?;
                    if !self.is_type_equality_comparable_to(expression_type, case_type)? {
                        // ARG ORDER: the case type is the SOURCE.
                        self.check_type_comparable_to(
                            case_type,
                            expression_type,
                            Some(clause_expression),
                            &diagnostics::Type_0_is_not_comparable_to_type_1,
                        )?;
                    }
                }
            }
            let statements = match self.data_of(clause) {
                NodeData::CaseClause(data) => data.statements,
                NodeData::DefaultClause(data) => data.statements,
                _ => None,
            };
            for statement in self.nodes_of(statements) {
                self.check_source_element(Some(statement));
            }
            // (noFallthroughCasesInSwitch arm — M5 flow, dead.)
        }
        Ok(())
    }

    /// tsc-port: checkLabeledStatement @6.0.3
    /// tsc-hash: aede5f733f5c0fbdcac637a52f20f6eb05c2a37f0256ee708789b57bd91eeab8
    /// tsc-span: _tsc.js:84643-84660
    ///
    /// The Unused_label arm is suggestion-band under the absent
    /// allowUnusedLabels option (errorOrSuggestion isError only when
    /// the option is EXPLICIT false) — skipped with §0's note.
    pub(crate) fn check_labeled_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::LabeledStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (label, statement) = (data.label, data.statement);
        if !self.check_grammar_statement_in_ambient_context_reported(node) {
            if let Some(label) = label {
                let label_text = self.identifier_text_of(label).map(str::to_owned);
                let mut current = self.parent_of(node);
                while let Some(candidate) = current {
                    if node_util::is_function_like_kind(self.kind_of(candidate)) {
                        break;
                    }
                    if self.kind_of(candidate) == SyntaxKind::LabeledStatement {
                        let candidate_label = match self.data_of(candidate) {
                            NodeData::LabeledStatement(data) => data.label,
                            _ => None,
                        };
                        if candidate_label
                            .and_then(|l| self.identifier_text_of(l).map(str::to_owned))
                            == label_text
                        {
                            // getTextOfNode — source text of the label.
                            let display = self.declaration_name_display(label);
                            self.grammar_error_on_node(
                                label,
                                &diagnostics::Duplicate_label_0,
                                &[&display],
                            );
                            break;
                        }
                    }
                    current = self.parent_of(candidate);
                }
            }
        }
        // (Unused label — suggestion band, skipped.)
        self.check_source_element(statement);
        Ok(())
    }

    /// tsc-port: checkThrowStatement @6.0.3
    /// tsc-hash: 0290673363a1ea730a006aebe88afb8bdcd18f2e9cbef47902b8bba29fe69b83
    /// tsc-span: _tsc.js:84661-84670
    pub(crate) fn check_throw_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        let NodeData::ThrowStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression;
        if !self.check_grammar_statement_in_ambient_context_reported(node) {
            if let Some(expression) = expression {
                if self.kind_of(expression) == SyntaxKind::Identifier
                    && self.identifier_text_of(expression) == Some("")
                {
                    self.grammar_error_after_first_token(
                        node,
                        &diagnostics::Line_break_not_permitted_here,
                        &[],
                    );
                }
            }
        }
        if let Some(expression) = expression {
            self.check_expression(expression, CheckMode::NORMAL)?;
        }
        Ok(())
    }

    /// tsc-port: grammarErrorAfterFirstToken @6.0.3
    /// tsc-hash: 68bf53599ebc14930b089ab20b2e3947651507045aee3511e408a4131dadc274
    /// tsc-span: _tsc.js:90369-90384
    ///
    /// Zero-length span at the END of the node's first token.
    fn grammar_error_after_first_token(
        &mut self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> bool {
        if self.has_parse_diagnostics(node) {
            return false;
        }
        let source = self.binder.source_of_node(node);
        let node_pos = source.arena.node(node).pos as usize;
        let (_start, token_end) = node_util::get_span_of_token_at_position(source, node_pos);
        let end = self.utf16_position(node, token_end);
        self.grammar_error_at_pos(node, end, 0, message, args)
    }

    /// tsc-port: checkTryStatement @6.0.3
    /// tsc-hash: dc888b416e5fa93a08982931e11d7dc3fbe1db56e44418523d1918aaf4c6b5d0
    /// tsc-span: _tsc.js:84671-84704
    ///
    /// forEachKey over catch locals = symbol-table insertion order
    /// (IndexMap).
    pub(crate) fn check_try_statement(&mut self, node: NodeId) -> CheckResult2<()> {
        self.check_grammar_statement_in_ambient_context(node);
        let NodeData::TryStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (try_block, catch_clause, finally_block) =
            (data.try_block, data.catch_clause, data.finally_block);
        if let Some(try_block) = try_block {
            self.check_block(try_block)?;
        }
        if let Some(catch_clause) = catch_clause {
            let NodeData::CatchClause(clause_data) = self.data_of(catch_clause) else {
                unreachable!("kind/data agree");
            };
            let (variable_declaration, catch_block) =
                (clause_data.variable_declaration, clause_data.block);
            if let Some(declaration) = variable_declaration {
                // Per-declaration containment keeps the catch block
                // checked when the variable check escapes.
                let _ = self.check_variable_like_declaration(declaration);
                let type_node = self.type_annotation_of(declaration);
                if let Some(type_node) = type_node {
                    let ty = self.get_type_from_type_node(type_node)?;
                    if !self
                        .tables
                        .flags_of(ty)
                        .intersects(TypeFlags::ANY | TypeFlags::UNKNOWN)
                    {
                        self.grammar_error_on_first_token(
                            type_node,
                            &diagnostics::Catch_clause_variable_type_annotation_must_be_any_or_unknown_if_specified,
                            &[],
                        );
                    }
                } else if let Some(initializer) = self.initializer_of_node(declaration) {
                    self.grammar_error_on_first_token(
                        initializer,
                        &diagnostics::Catch_clause_variable_cannot_have_an_initializer,
                        &[],
                    );
                } else {
                    let block_locals = catch_block
                        .and_then(|block| self.binder.locals_of(block))
                        .cloned();
                    if let Some(block_locals) = block_locals {
                        let catch_locals = self
                            .binder
                            .locals_of(catch_clause)
                            .cloned()
                            .unwrap_or_default();
                        for caught_name in catch_locals.keys() {
                            if let Some(&block_local) = block_locals.get(caught_name) {
                                let block_symbol = self.binder.symbol(block_local);
                                if let Some(value_declaration) = block_symbol.value_declaration {
                                    if block_symbol
                                        .flags
                                        .intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE)
                                    {
                                        let display =
                                            tsrs2_binder::unescape_leading_underscores(caught_name)
                                                .to_owned();
                                        self.grammar_error_on_node(
                                            value_declaration,
                                            &diagnostics::Cannot_redeclare_identifier_0_in_catch_clause,
                                            &[&display],
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(catch_block) = catch_block {
                self.check_block(catch_block)?;
            }
        }
        if let Some(finally_block) = finally_block {
            self.check_block(finally_block)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::state::test_support::with_program_state;
    use crate::{check_program, CompilerOptions, InputFile};

    /// Checker-sink rows as (code, start, length) — noLib unit parity
    /// (scratchpad p1-p6 oracle probes, 2026-07-14).
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

    // ---- §2 variable band (oracle p1) ----

    #[test]
    fn declaration_initializer_2322_reports_at_the_name_span() {
        // getErrorSpanForNode's VariableDeclaration arm → the NAME.
        assert_eq!(checked_rows("const x: string = 1;\n"), [(2322, 6, 1)]);
    }

    #[test]
    fn subsequent_variable_declaration_reports_2403_with_related() {
        let rows = with_program_state(
            &[("a.ts", "var y: string;\nvar y: number;\n")],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                state
                    .diagnostics
                    .iter()
                    .map(|diag| {
                        (
                            diag.code(),
                            diag.start.unwrap_or(u32::MAX),
                            diag.length.unwrap_or(u32::MAX),
                            diag.message_text().to_owned(),
                            diag.related.len(),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        );
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(
            (rows[0].0, rows[0].1, rows[0].2, rows[0].4),
            (2403, 19, 1, 1)
        );
        // tsc's double space after the first sentence is load-bearing.
        assert_eq!(
            rows[0].3,
            "Subsequent variable declarations must have the same type.  Variable 'y' must be of type 'string', but here has type 'number'."
        );
    }

    #[test]
    fn renamed_signature_binding_2842_waits_on_the_parameter_arm() {
        // Oracle p5: tsc reports 2842 at `b` (offset 24) and the
        // isReferenced gate suppresses h2's `c`. The drain + gate are
        // live (worker tail, risk §14.16), but the PUSHER rides
        // checkParameter → checkVariableLikeDeclaration (§5, 5.8b) —
        // until then the row is a recorded FN. This pin flips to
        // [(2842, 24, 1)] when 5.8b lands.
        assert_eq!(
            checked_rows(
                "declare function h({ a: b }: { a: number }): void;\ndeclare function h2({ a: c }: { a: number }, d: typeof c): void;\n"
            ),
            []
        );
    }

    // ---- §2 collisions band under skippedOn(noEmit) (oracle p4) ----

    fn commonjs_rows(no_emit: Option<bool>) -> Vec<(u32, u32, u32)> {
        let options = CompilerOptions {
            module: Some(1),
            no_emit,
            ..CompilerOptions::default()
        };
        let result = check_program(
            &[InputFile {
                name: "a.ts".to_owned(),
                text: "export {};\nvar require: number;\n".to_owned(),
            }],
            &options,
        );
        result
            .diagnostics
            .iter()
            .map(|diag| {
                (
                    diag.code(),
                    diag.start.unwrap_or(u32::MAX),
                    diag.length.unwrap_or(u32::MAX),
                )
            })
            .collect()
    }

    #[test]
    fn require_collision_reports_2441_and_no_emit_filters_it() {
        assert_eq!(commonjs_rows(None), [(2441, 15, 7)]);
        assert_eq!(commonjs_rows(Some(true)), []);
    }

    // ---- §3 control statements (oracle p2/p3) ----

    #[test]
    fn condition_bands_report_2774_2873_1313() {
        assert_eq!(
            checked_rows("declare function f(): void;\nif (f) {}\nif (void 0) {}\nif (1) ;\n"),
            [(2774, 32, 1), (2873, 42, 6), (1313, 60, 1)]
        );
    }

    #[test]
    fn switch_case_2678_uses_the_case_type_as_source() {
        assert_eq!(
            checked_rows("switch (\"a\") { case 1: break; }\n"),
            [(2678, 20, 1)]
        );
    }

    #[test]
    fn catch_clause_block_scoped_shadow_reports_2492() {
        assert_eq!(
            checked_rows("try {} catch (q) { let q: number; }\n"),
            [(2492, 23, 1)]
        );
    }

    #[test]
    fn block_scoped_statement_in_do_body_reports_1156() {
        assert_eq!(
            checked_rows("if (2) { let z = 1; }\ndo let v = 1; while (0);\n"),
            [(2872, 4, 1), (1156, 25, 10)]
        );
    }

    // ---- §11 tuple type-node rows (oracle p6) ----

    #[test]
    fn tuple_element_order_rows_1266_1257_2574() {
        assert_eq!(
            checked_rows(
                "interface Array<T> { length: number }\ntype T1 = [...string[], number?];\ntype T2 = [number?, string];\ntype T3 = [...number, string];\n"
            ),
            [(1266, 62, 7), (1257, 92, 6), (2574, 112, 9)]
        );
    }
}
