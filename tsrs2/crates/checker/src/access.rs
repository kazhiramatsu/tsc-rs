//! M4 5.5d: the access band's non-null core (extraction doc §6) — the
//! checkNonNullType family every property/element access routes
//! through, plus the optional-chain type plumbing (optionalType marker
//! singleton) and the `x!` assertion arms.
//!
//! Oracle-pinned surprises (scratchpad pins55d matrix, 2026-07-12):
//! facts ∌ void ⇒ a void/never receiver falls PAST the nullable report
//! into plain 2339; 18050 fires only for the LITERAL `null` keyword /
//! identifier `undefined` (nullable-TYPED idents take 18047/18048);
//! `(null).foo` → 2531 (parens defeat both the kind and entity tests);
//! `x!` NEVER reports — checkNonNullAssertion has no error path (§6
//! correction: checkNonNullNonVoidType's consumers are the 5.8
//! variable-declaration sites, not arm 236).

use tsrs2_binder::{node_util, SymbolTable};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckMode, ModifierFlags, NodeFlags, SymbolFlags, SymbolId, TypeFacts, TypeFlags, TypeId,
    UnionReduction,
};

use crate::state::{CheckResult2, CheckerState, Unsupported};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlowGuardCertainty {
    Yes,
    No,
    Unknown,
}

impl FlowGuardCertainty {
    fn may_narrow(self) -> bool {
        self != Self::No
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlowPredicateEffect {
    Argument(usize),
    This,
    None,
    Unknown,
}

impl FlowPredicateEffect {
    fn certainty(self) -> FlowGuardCertainty {
        match self {
            Self::Argument(_) | Self::This => FlowGuardCertainty::Yes,
            Self::None => FlowGuardCertainty::No,
            Self::Unknown => FlowGuardCertainty::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FlowReferenceRoot<'a> {
    text: &'a str,
    symbol: Option<SymbolId>,
    /// Aliases cannot participate in narrowing before their initializer
    /// has executed. Original reference roots use zero.
    available_from: u32,
    /// tsc inlines condition aliases only while `inlineLevel < 5`.
    /// Original roots are level zero; the first alias is level one.
    alias_depth: u8,
    /// A sibling binding from the same destructuring pattern. These
    /// only narrow dependently under a discriminant comparison/switch.
    dependent: bool,
}

impl<'a> CheckerState<'a> {
    /// tsc-port: entityNameToString @6.0.3
    /// tsc-hash: 1d98dd7fd01a30bb4a1bf4062755311f4a0d24d9d7254db4941dd58e0e1d4333
    /// tsc-span: _tsc.js:13886-13908
    ///
    /// The identifier arm renders SOURCE TEXT (escapes as written) for
    /// parsed nodes; the property-access recursion drops `?.` — which
    /// is why `x?.a`'s 18047 message says 'x.a' while the span covers
    /// the `?.` (oracle-pinned). JSDocMemberName/JsxNamespacedName
    /// arms escape (JSDoc unmodeled / JSX 5.5f).
    pub(crate) fn entity_name_to_string(&self, node: NodeId) -> CheckResult2<String> {
        match self.kind_of(node) {
            SyntaxKind::ThisKeyword => Ok("this".to_owned()),
            SyntaxKind::Identifier | SyntaxKind::PrivateIdentifier => {
                let source = self.binder.source_of_node(node);
                let raw = source.arena.node(node);
                if raw.end == raw.pos {
                    // getFullWidth == 0: parse-recovery synthesized —
                    // idText.
                    return Ok(self
                        .identifier_text_of(node)
                        .map(tsrs2_binder::unescape_leading_underscores)
                        .unwrap_or_default()
                        .to_owned());
                }
                let start = tsrs2_syntax::skip_trivia(&source.text, raw.pos as usize);
                Ok(source.text[start..raw.end as usize].to_owned())
            }
            SyntaxKind::QualifiedName => {
                let NodeData::QualifiedName(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let (Some(left), Some(right)) = (data.left, data.right) else {
                    return Err(Unsupported::new("entityNameToString on recovery node"));
                };
                Ok(format!(
                    "{}.{}",
                    self.entity_name_to_string(left)?,
                    self.entity_name_to_string(right)?
                ))
            }
            SyntaxKind::PropertyAccessExpression => {
                let NodeData::PropertyAccessExpression(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let (Some(expression), Some(name)) = (data.expression, data.name) else {
                    return Err(Unsupported::new("entityNameToString on recovery node"));
                };
                Ok(format!(
                    "{}.{}",
                    self.entity_name_to_string(expression)?,
                    self.entity_name_to_string(name)?
                ))
            }
            _ => Err(Unsupported::new(
                "entityNameToString beyond entity kinds (JSDoc/JSX, M8)",
            )),
        }
    }

    // ---- optional-chain type plumbing (the optionalType marker) ----

    /// tsc-port: getOptionalType @6.0.3
    /// tsc-hash: bb5a73a698a53842f916c77432005c59701edb3812c2a8886b2ff40155bcdc4b
    /// tsc-span: _tsc.js:67852-67856
    pub(crate) fn get_optional_type(
        &mut self,
        ty: TypeId,
        is_property: bool,
    ) -> CheckResult2<TypeId> {
        debug_assert!(self
            .options
            .strict_option_value(self.options.strict_null_checks));
        let missing_or_undefined = if is_property {
            self.tables.intrinsics.undefined_or_missing
        } else {
            self.tables.intrinsics.undefined
        };
        let already = ty == missing_or_undefined
            || self.tables.flags_of(ty).intersects(TypeFlags::UNION)
                && match &self.tables.type_of(ty).data {
                    tsrs2_types::TypeData::Union { types, .. } => {
                        types.first() == Some(&missing_or_undefined)
                    }
                    _ => unreachable!("union flag implies union data"),
                };
        if already {
            Ok(ty)
        } else {
            self.get_union_type_ex(&[ty, missing_or_undefined], UnionReduction::Literal)
        }
    }

    /// tsc-port: addOptionalTypeMarker @6.0.3
    /// tsc-hash: 2f1bc5d7263e537738cfae30bb751693cb7cc8a515df0c8e66238e37eb8b1170
    /// tsc-span: _tsc.js:67871-67879
    pub(crate) fn add_optional_type_marker(&mut self, ty: TypeId) -> CheckResult2<TypeId> {
        if self
            .options
            .strict_option_value(self.options.strict_null_checks)
        {
            let optional = self.tables.intrinsics.optional;
            self.get_union_type_ex(&[ty, optional], UnionReduction::Literal)
        } else {
            Ok(ty)
        }
    }

    pub(crate) fn remove_optional_type_marker(&mut self, ty: TypeId) -> TypeId {
        if self
            .options
            .strict_option_value(self.options.strict_null_checks)
        {
            // tsc removeType (70022-70024): filterType against the
            // marker singleton.
            let optional = self.tables.intrinsics.optional;
            self.tables.filter_type(ty, |_, t| t != optional)
        } else {
            ty
        }
    }

    pub(crate) fn propagate_optional_type_marker(
        &mut self,
        ty: TypeId,
        node: NodeId,
        was_optional: bool,
    ) -> CheckResult2<TypeId> {
        if !was_optional {
            return Ok(ty);
        }
        let source = self.binder.source_of_node(node);
        if node_util::is_outermost_optional_chain(source, node) {
            self.get_optional_type(ty, /*is_property*/ false)
        } else {
            self.add_optional_type_marker(ty)
        }
    }

    /// tsc-port: getOptionalExpressionType @6.0.3
    /// tsc-hash: ea259d5b5bbf80bf0a78c525248ab3300fbf0a062d7ff85d73969038ada5a26f
    /// tsc-span: _tsc.js:67880-67882
    pub(crate) fn get_optional_expression_type(
        &mut self,
        expr_type: TypeId,
        expression: NodeId,
    ) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(expression);
        if node_util::is_expression_of_optional_chain_root(source, expression) {
            self.get_non_nullable_type(expr_type)
        } else if node_util::is_optional_chain(source, expression) {
            Ok(self.remove_optional_type_marker(expr_type))
        } else {
            Ok(expr_type)
        }
    }

    // ---- the non-null core ----

    /// tsc-port: checkNonNullExpression @6.0.3
    /// tsc-hash: 6f60b907a131050516e46ca6b7498d1d1096d7a0b188dd2b549ae048f6aa4938
    /// tsc-span: _tsc.js:74990-74992
    ///
    /// isNullableType/getNonNullableTypeIfNeeded (74993-74998) have no
    /// live consumer yet (declaration/flow bands) — unported per the
    /// ledger-unreachability rule.
    pub(crate) fn check_non_null_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let ty = self.check_expression(node, CheckMode::NORMAL)?;
        self.check_non_null_type(ty, node)
    }

    /// tsc-port: reportObjectPossiblyNullOrUndefinedError @6.0.3
    /// tsc-hash: 79ff155e0f17eaa98158dc28a44288d034bf01fed59a8f3cf88a5a6ff17eb590
    /// tsc-span: _tsc.js:74999-75022
    ///
    /// Selection order is load-bearing: the NullKeyword arm precedes
    /// the length test (a paren-wrapped `null` misses BOTH the kind
    /// and entity tests → 2531); the `undefined` IDENTIFIER arm lives
    /// inside the <100 branch.
    fn report_object_possibly_null_or_undefined_error(
        &mut self,
        node: NodeId,
        facts: TypeFacts,
    ) -> CheckResult2<()> {
        let node_text = if self.is_entity_name_expression(node) {
            Some(self.entity_name_to_string(node)?)
        } else {
            None
        };
        if self.kind_of(node) == SyntaxKind::NullKeyword {
            self.error_at(
                Some(node),
                &tsrs2_diags::gen::The_value_0_cannot_be_used_here,
                &["null"],
            );
            return Ok(());
        }
        if let Some(text) = node_text {
            if text.encode_utf16().count() < 100 {
                if self.kind_of(node) == SyntaxKind::Identifier && text == "undefined" {
                    self.error_at(
                        Some(node),
                        &tsrs2_diags::gen::The_value_0_cannot_be_used_here,
                        &["undefined"],
                    );
                    return Ok(());
                }
                let message = if facts.intersects(TypeFacts::IS_UNDEFINED) {
                    if facts.intersects(TypeFacts::IS_NULL) {
                        &tsrs2_diags::gen::_0_is_possibly_null_or_undefined
                    } else {
                        &tsrs2_diags::gen::_0_is_possibly_undefined
                    }
                } else {
                    &tsrs2_diags::gen::_0_is_possibly_null
                };
                self.error_at(Some(node), message, &[&text]);
                return Ok(());
            }
        }
        let message = if facts.intersects(TypeFacts::IS_UNDEFINED) {
            if facts.intersects(TypeFacts::IS_NULL) {
                &tsrs2_diags::gen::Object_is_possibly_null_or_undefined
            } else {
                &tsrs2_diags::gen::Object_is_possibly_undefined
            }
        } else {
            &tsrs2_diags::gen::Object_is_possibly_null
        };
        self.error_at(Some(node), message, &[]);
        Ok(())
    }

    /// tsc-port: checkNonNullTypeWithReporter @6.0.3
    /// tsc-hash: 0754bf4b1a28b608ede342e0351520d32e9eb7561e782250fc9311d73108f497
    /// tsc-span: _tsc.js:75028-75050
    ///
    /// The Invoke reporter flavor (2721/2722/2723) is the [CALLS 5.7]
    /// consumer — the reporter parameter keeps its seam.
    /// [FLOW M5] containment gate: tsc runs these reports on the FLOW
    /// type; our stub answers the DECLARED type, so a narrowable
    /// receiver (identifier/this/property/element chains — the shapes
    /// getFlowTypeOfReference narrows) that tsc de-nulls via guards or
    /// assignments would FP here (conformance find:
    /// logicalAssignment11's `d ?? (d = ...); d.length`). Nullable and
    /// unknown verdicts on narrowable receivers therefore CONTAIN
    /// until M5; literal receivers (`null.foo`, `(null).foo`) cannot
    /// be narrowed and report exactly.
    pub(crate) fn receiver_may_be_flow_narrowed(&self, node: NodeId) -> bool {
        let mut core = node;
        loop {
            match self.data_of(core) {
                NodeData::ParenthesizedExpression(data) => match data.expression {
                    Some(inner) => core = inner,
                    None => break,
                },
                NodeData::NonNullExpression(data) => match data.expression {
                    Some(inner) => core = inner,
                    None => break,
                },
                _ => break,
            }
        }
        matches!(
            self.kind_of(core),
            SyntaxKind::Identifier
                | SyntaxKind::ThisKeyword
                | SyntaxKind::PropertyAccessExpression
                | SyntaxKind::ElementAccessExpression
                // Type-query entity names narrow too (tsc
                // getFlowTypeOfReference's QualifiedName face;
                // typeofThis pins `typeof this.a.b` under an if-guard,
                // M4 5.8d).
                | SyntaxKind::QualifiedName
        )
    }

    pub(crate) fn check_non_null_type_with_reporter(
        &mut self,
        ty: TypeId,
        node: NodeId,
        report_error: fn(&mut Self, NodeId, TypeFacts) -> CheckResult2<()>,
    ) -> CheckResult2<TypeId> {
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        if strict_null_checks && self.tables.flags_of(ty).intersects(TypeFlags::UNKNOWN) {
            if self.receiver_may_be_flow_narrowed(node) {
                return Err(Unsupported::new(
                    "[FLOW M5] unknown receiver on a narrowable reference",
                ));
            }
            if self.is_entity_name_expression(node) {
                let node_text = self.entity_name_to_string(node)?;
                if node_text.encode_utf16().count() < 100 {
                    self.error_at(
                        Some(node),
                        &tsrs2_diags::gen::_0_is_of_type_unknown,
                        &[&node_text],
                    );
                    return Ok(self.tables.intrinsics.error);
                }
            }
            self.error_at(
                Some(node),
                &tsrs2_diags::gen::Object_is_of_type_unknown,
                &[],
            );
            return Ok(self.tables.intrinsics.error);
        }
        let facts = self.get_type_facts(ty, TypeFacts::IS_UNDEFINED_OR_NULL)?;
        if facts.intersects(TypeFacts::IS_UNDEFINED_OR_NULL) {
            if self.receiver_may_be_flow_narrowed(node) {
                return Err(Unsupported::new(
                    "[FLOW M5] nullable receiver on a narrowable reference",
                ));
            }
            report_error(self, node, facts)?;
            let t = self.get_non_nullable_type(ty)?;
            return Ok(
                if self
                    .tables
                    .flags_of(t)
                    .intersects(TypeFlags::NULLABLE | TypeFlags::NEVER)
                {
                    self.tables.intrinsics.error
                } else {
                    t
                },
            );
        }
        Ok(ty)
    }

    /// tsrs-native: [FLOW M5] containment probe (no tsc counterpart —
    /// tsc consults real flow types). True when a narrowing construct
    /// in the enclosing function chain (a) mentions one of the
    /// reference's roots — the same BINDING, not just the same
    /// spelling — and (b) can REACH the reference along forward
    /// control flow (PR #6 review round 2: a guard AFTER the read, on
    /// a shadowed binding, or in a sibling if/conditional limb never
    /// suppresses a report). Constructs: if/while/do/for/switch/
    /// conditional conditions, `case` guards, a logical binary's left
    /// operand, and assignment targets. Guard-shaped `const`/`using`
    /// aliases (comparisons, logicals, `!`, type-predicate calls) and
    /// aliased/destructured discriminants (`const kind = obj.kind`,
    /// `const { kind } = obj`) extend the root set in source order, but
    /// an alias DEFINITION is not itself a guard — a reaching condition
    /// must consume it. Candidates are isolated by their nearest
    /// function boundary. M5 removes the probe with the gates it feeds.
    pub(crate) fn flow_guards_narrow_reference(
        &self,
        gate_node: NodeId,
        reference: NodeId,
    ) -> bool {
        self.flow_guards_narrow_reference_worker(gate_node, reference, false)
    }

    /// tsrs-native: narrow `typeof this` only for the `instanceof`
    /// face that can actually change its flow type. A mere truthiness
    /// test such as `if (this)` reaches the reference but leaves
    /// `typeof this` unchanged and must not suppress downstream
    /// relation errors.
    pub(crate) fn instanceof_guard_narrows_reference(
        &self,
        gate_node: NodeId,
        reference: NodeId,
    ) -> bool {
        self.flow_guards_narrow_reference_worker(gate_node, reference, true)
    }

    fn flow_guards_narrow_reference_worker(
        &self,
        gate_node: NodeId,
        reference: NodeId,
        require_instanceof: bool,
    ) -> bool {
        let source = self.binder.source_of_node(gate_node);
        let mut roots: Vec<FlowReferenceRoot<'_>> = Vec::new();
        let mut this_root = false;
        self.collect_reference_roots(reference, &mut roots, &mut this_root);
        if roots.is_empty() && !this_root {
            return false;
        }
        let mut gate_scopes = Vec::new();
        let mut cursor = Some(gate_node);
        while let Some(current) = cursor {
            if node_util::is_function_like_kind(self.kind_of(current)) {
                gate_scopes.push(Some(current));
            }
            cursor = self.parent_of(current);
        }
        // Guards in an enclosing function can narrow a captured
        // binding (especially lexical `this`) inside a nested arrow.
        // The source-file scope is the outermost link in that chain.
        gate_scopes.push(None);

        // Build a syntax-only index once per source. Grouping by the
        // nearest function boundary both excludes unrelated closures
        // and lets later failed diagnostics reuse the source walk.
        if !self
            .flow_containment_indexes
            .borrow()
            .contains_key(&source.root)
        {
            let mut index: crate::state::FlowContainmentIndex = Default::default();
            let mut worklist = vec![(source.root, None)];
            while let Some((current, enclosing_scope)) = worklist.pop() {
                let current_scope = if node_util::is_function_like_kind(self.kind_of(current)) {
                    Some(current)
                } else {
                    enclosing_scope
                };
                let candidates = index.entry(current_scope).or_default();
                match self.data_of(current) {
                    NodeData::VariableDeclaration(_) => {
                        candidates.alias_declarations.push(current);
                    }
                    NodeData::IfStatement(_)
                    | NodeData::WhileStatement(_)
                    | NodeData::DoStatement(_)
                    | NodeData::ForStatement(_)
                    | NodeData::SwitchStatement(_)
                    | NodeData::CaseClause(_)
                    | NodeData::ConditionalExpression(_)
                    | NodeData::BinaryExpression(_) => candidates.guard_nodes.push(current),
                    _ => {}
                }
                tsrs2_syntax::for_each_child(&source.arena, source.arena.node(current), |child| {
                    worklist.push((child, current_scope));
                    false
                });
            }
            let mut scopes: Vec<_> = index.keys().copied().collect();
            scopes.sort_unstable();
            for scope in scopes {
                let candidates = index
                    .get_mut(&scope)
                    .expect("scope was collected from this index");
                candidates
                    .alias_declarations
                    .sort_by_key(|&node| source.arena.node(node).pos);
            }
            self.flow_containment_indexes
                .borrow_mut()
                .insert(source.root, index);
        }
        let indexes = self.flow_containment_indexes.borrow();
        let Some(index) = indexes.get(&source.root) else {
            return false;
        };
        let scoped_candidates: Vec<_> = gate_scopes
            .iter()
            .filter_map(|scope| index.get(scope))
            .collect();
        if scoped_candidates.is_empty() {
            return false;
        }
        let mut alias_declarations: Vec<_> = scoped_candidates
            .iter()
            .flat_map(|candidates| candidates.alias_declarations.iter().copied())
            .collect();
        alias_declarations.sort_by_key(|&node| source.arena.node(node).pos);

        // Alias declarations are processed in source order: eligible
        // aliases are immutable and can only reference an earlier
        // constant, so no fixpoint scan is needed.
        // Pass 1 — root alias expansion: aliased and destructured
        // discriminants re-narrow their SOURCE (tsc's
        // controlFlowAliasing: `const kind = obj.kind;
        // if (kind === "a") obj.a`), so their binding names join the
        // root set. tsc's eligibility is intentionally narrow: a
        // const/using declaration, no explicit type, and at most five
        // inline alias edges.
        for current in alias_declarations {
            let NodeData::VariableDeclaration(data) = self.data_of(current) else {
                continue;
            };
            if data.r#type.is_some()
                || !node_util::get_combined_node_flags(source, current)
                    .intersects(NodeFlags::CONSTANT)
            {
                continue;
            }
            let (Some(name), Some(initializer)) = (data.name, data.initializer) else {
                continue;
            };
            let normalized = self.skip_flow_transparent_expression(initializer);
            let property_alias = matches!(
                self.data_of(normalized),
                NodeData::PropertyAccessExpression(_) | NodeData::ElementAccessExpression(_)
            );
            let destructuring_alias = matches!(
                self.data_of(name),
                NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_)
            );
            let guard_certainty = self.initializer_guard_certainty(initializer);
            let condition_alias = matches!(self.data_of(normalized), NodeData::Identifier(_))
                && self
                    .operand_root_depth_filtered(normalized, &roots, this_root, true)
                    .is_some();
            let source_depth = if guard_certainty.may_narrow() {
                self.guard_operand_root_depth(initializer, &roots, this_root)
            } else {
                self.operand_root_depth(initializer, &roots, this_root)
            };
            let Some(source_depth) = source_depth else {
                continue;
            };
            let alias_depth = source_depth.saturating_add(1);
            let dependent_source = !guard_certainty.may_narrow() && {
                let dependent_roots: Vec<_> = roots
                    .iter()
                    .filter(|root| root.dependent)
                    .copied()
                    .collect();
                let direct_roots: Vec<_> = roots
                    .iter()
                    .filter(|root| !root.dependent)
                    .copied()
                    .collect();
                self.operand_root_depth(initializer, &dependent_roots, false)
                    .is_some()
                    && self
                        .operand_root_depth(initializer, &direct_roots, this_root)
                        .is_none()
            };
            if alias_depth <= 5
                && (property_alias
                    || destructuring_alias
                    || guard_certainty.may_narrow()
                    || condition_alias)
            {
                self.push_alias_binding_roots(
                    current,
                    name,
                    alias_depth,
                    dependent_source,
                    &mut roots,
                );
            }
        }
        // Pass 2 — the guard scan proper.
        let direct_roots: Vec<_> = roots
            .iter()
            .filter(|root| !root.dependent)
            .copied()
            .collect();
        let dependent_roots: Vec<_> = roots
            .iter()
            .filter(|root| root.dependent)
            .copied()
            .collect();
        for &current in scoped_candidates
            .iter()
            .flat_map(|candidates| &candidates.guard_nodes)
        {
            let narrowing_operand = match self.data_of(current) {
                NodeData::IfStatement(data) => data.expression,
                NodeData::WhileStatement(data) => data.expression,
                NodeData::DoStatement(data) => data.expression,
                NodeData::ForStatement(data) => data.condition,
                NodeData::SwitchStatement(data) => data.expression,
                // A `case` guard narrows on its own (`switch (true) {
                // case typeof x === "s": ... }`) — the discriminant
                // arm above cannot see it.
                NodeData::CaseClause(data) => data.expression,
                NodeData::ConditionalExpression(data) => data.condition,
                NodeData::BinaryExpression(data) => {
                    let operator = data.operator_token.map(|token| self.kind_of(token));
                    let is_logical = matches!(
                        operator,
                        Some(
                            SyntaxKind::AmpersandAmpersandToken
                                | SyntaxKind::BarBarToken
                                | SyntaxKind::QuestionQuestionToken
                        )
                    );
                    let is_assignment = operator.is_some_and(|op| {
                        op == SyntaxKind::EqualsToken
                            || (op.value() >= SyntaxKind::FirstCompoundAssignment.value()
                                && op.value() <= SyntaxKind::LastCompoundAssignment.value())
                    });
                    if is_logical || is_assignment {
                        data.left
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(operand) = narrowing_operand {
                let direct_guard =
                    self.guard_operand_mentions_roots(operand, &direct_roots, this_root);
                let dependent_guard = !dependent_roots.is_empty()
                    && self.guard_operand_mentions_roots(operand, &dependent_roots, false)
                    && (self.kind_of(current) == SyntaxKind::SwitchStatement
                        || self.dependent_discriminant_comparison(operand, &dependent_roots));
                if (direct_guard || dependent_guard)
                    && (!require_instanceof
                        || self.guard_operand_has_instanceof_root(operand, &roots, this_root))
                    && self.guard_reaches_reference(current, operand, reference)
                {
                    return true;
                }
            }
        }
        false
    }

    /// Root keys under `reference`: every value-bearing identifier
    /// paired with its lexical binding, plus the `this` face. A
    /// property name is not a root by itself: `other.value` cannot
    /// narrow `obj.value` merely because both paths end in `value`.
    fn collect_reference_roots<'r>(
        &'r self,
        reference: NodeId,
        roots: &mut Vec<FlowReferenceRoot<'r>>,
        this_root: &mut bool,
    ) {
        let source = self.binder.source_of_node(reference);
        let push = |roots: &mut Vec<FlowReferenceRoot<'r>>, entry: FlowReferenceRoot<'r>| {
            if !roots.contains(&entry) {
                roots.push(entry);
            }
        };
        let mut worklist = vec![reference];
        while let Some(current) = worklist.pop() {
            match self.data_of(current) {
                NodeData::Identifier(data) => {
                    // `typeof this` parses its exprName as an
                    // identifier spelled `this` — same narrowing face
                    // as the keyword.
                    if data.escaped_text == "this" {
                        *this_root = true;
                    }
                    let symbol = self.resolve_lexical_value_symbol(current, &data.escaped_text);
                    push(
                        roots,
                        FlowReferenceRoot {
                            text: &data.escaped_text,
                            symbol,
                            available_from: 0,
                            alias_depth: 0,
                            dependent: false,
                        },
                    );
                    if let Some(symbol) = symbol {
                        self.push_destructuring_sibling_roots(symbol, roots);
                    }
                }
                NodeData::PropertyAccessExpression(data) => {
                    if let Some(expression) = data.expression {
                        worklist.push(expression);
                    }
                    continue;
                }
                _ => {
                    if self.kind_of(current) == SyntaxKind::ThisKeyword {
                        *this_root = true;
                    }
                }
            }
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(current), |child| {
                worklist.push(child);
                false
            });
        }
    }

    /// A dependent destructuring sibling is only a narrowing guard
    /// when it participates in an equality-style discriminant test.
    /// This rejects unrelated truthiness such as `{a, b}; if (a)
    /// b.missing`, while preserving `kind === "A"` correlations.
    fn dependent_discriminant_comparison(
        &self,
        operand: NodeId,
        roots: &[FlowReferenceRoot<'_>],
    ) -> bool {
        let source = self.binder.source_of_node(operand);
        let mut worklist = vec![operand];
        while let Some(current) = worklist.pop() {
            if let NodeData::BinaryExpression(data) = self.data_of(current) {
                let operator = data.operator_token.map(|token| self.kind_of(token));
                if matches!(
                    operator,
                    Some(
                        SyntaxKind::EqualsEqualsToken
                            | SyntaxKind::ExclamationEqualsToken
                            | SyntaxKind::EqualsEqualsEqualsToken
                            | SyntaxKind::ExclamationEqualsEqualsToken
                    )
                ) && [data.left, data.right]
                    .into_iter()
                    .flatten()
                    .any(|side| self.guard_operand_mentions_roots(side, roots, false))
                {
                    return true;
                }
            }
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(current), |child| {
                worklist.push(child);
                false
            });
        }
        false
    }

    fn push_destructuring_sibling_roots<'r>(
        &'r self,
        symbol: SymbolId,
        roots: &mut Vec<FlowReferenceRoot<'r>>,
    ) {
        let declarations = self.binder.symbol(symbol).declarations.clone();
        for declaration in declarations {
            if self.kind_of(declaration) != SyntaxKind::BindingElement {
                continue;
            }
            let mut top_pattern = None;
            let mut cursor = self.parent_of(declaration);
            while let Some(current) = cursor {
                match self.kind_of(current) {
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern => {
                        top_pattern = Some(current);
                        cursor = self.parent_of(current);
                    }
                    SyntaxKind::BindingElement => cursor = self.parent_of(current),
                    _ => break,
                }
            }
            let Some(top_pattern) = top_pattern else {
                continue;
            };
            // Correlated narrowing across destructured bindings is a
            // discriminated-UNION feature. A comparison on one field
            // of a concrete object does not change the type of its
            // siblings (`{ a: boolean, b: number }; a === true`).
            // Keep this containment gate closed unless the annotated
            // or inferred source has actually resolved to a union.
            let Some(owner) = self.parent_of(top_pattern) else {
                continue;
            };
            let source_type = self
                .type_annotation_of(owner)
                .and_then(|annotation| self.links.node(annotation).resolved_type.resolved())
                .or_else(|| match self.data_of(owner) {
                    NodeData::VariableDeclaration(data) => {
                        data.initializer.and_then(|initializer| {
                            self.links.node(initializer).resolved_type.resolved()
                        })
                    }
                    _ => None,
                });
            let Some(source_type) = source_type else {
                continue;
            };
            if !self.is_confirmed_union_flow_source(source_type) {
                continue;
            }
            let mut worklist = vec![top_pattern];
            while let Some(current) = worklist.pop() {
                match self.data_of(current) {
                    NodeData::ObjectBindingPattern(data) => {
                        worklist.extend(self.nodes_of(data.elements));
                    }
                    NodeData::ArrayBindingPattern(data) => {
                        worklist.extend(self.nodes_of(data.elements));
                    }
                    NodeData::BindingElement(data) => {
                        let Some(name) = data.name else { continue };
                        if let NodeData::Identifier(name_data) = self.data_of(name) {
                            let entry = FlowReferenceRoot {
                                text: &name_data.escaped_text,
                                symbol: self
                                    .resolve_lexical_value_symbol(name, &name_data.escaped_text),
                                available_from: 0,
                                alias_depth: 0,
                                dependent: true,
                            };
                            if !roots.contains(&entry) {
                                roots.push(entry);
                            }
                        } else {
                            worklist.push(name);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn is_confirmed_union_flow_source(&self, mut ty: TypeId) -> bool {
        for _ in 0..4 {
            let flags = self.tables.flags_of(ty);
            if flags.intersects(TypeFlags::UNION) {
                return true;
            }
            if !flags.intersects(TypeFlags::TYPE_PARAMETER) {
                return false;
            }
            let Some(constraint) = self.links.ty(ty).type_parameter_constraint.resolved() else {
                return false;
            };
            if constraint == ty {
                return false;
            }
            ty = constraint;
        }
        false
    }

    fn guard_operand_has_instanceof_root(
        &self,
        operand: NodeId,
        roots: &[FlowReferenceRoot<'_>],
        this_root: bool,
    ) -> bool {
        let source = self.binder.source_of_node(operand);
        let mut worklist = vec![operand];
        while let Some(current) = worklist.pop() {
            if let NodeData::BinaryExpression(data) = self.data_of(current) {
                let operator = data.operator_token.map(|token| self.kind_of(token));
                if operator == Some(SyntaxKind::InstanceOfKeyword)
                    && data.left.is_some_and(|left| {
                        self.guard_operand_mentions_roots(left, roots, this_root)
                    })
                {
                    return true;
                }
            }
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(current), |child| {
                worklist.push(child);
                false
            });
        }
        false
    }

    /// Candidate-side face of the root match: text equality, with
    /// binding identity allowed to BREAK the match only when both
    /// sides resolved — a guard on a shadowed binding is unrelated;
    /// an unresolved side stays text-matched, erring on the
    /// containment side.
    fn operand_root_depth(
        &self,
        operand: NodeId,
        roots: &[FlowReferenceRoot<'_>],
        this_root: bool,
    ) -> Option<u8> {
        self.operand_root_depth_filtered(operand, roots, this_root, false)
    }

    fn operand_root_depth_filtered(
        &self,
        operand: NodeId,
        roots: &[FlowReferenceRoot<'_>],
        this_root: bool,
        aliases_only: bool,
    ) -> Option<u8> {
        let source = self.binder.source_of_node(operand);
        let mut minimum_depth = None;
        let mut worklist = vec![operand];
        while let Some(current) = worklist.pop() {
            match self.data_of(current) {
                NodeData::Identifier(data) => {
                    let text = data.escaped_text.as_str();
                    if roots
                        .iter()
                        .any(|root| root.text == text && (!aliases_only || root.alias_depth > 0))
                    {
                        let symbol = self.resolve_lexical_value_symbol(current, text);
                        let position = source.arena.node(current).pos;
                        for root in roots.iter().filter(|root| {
                            root.text == text
                                && (!aliases_only || root.alias_depth > 0)
                                && position >= root.available_from
                                && match (root.symbol, symbol) {
                                    (Some(root), Some(candidate)) => root == candidate,
                                    _ => true,
                                }
                        }) {
                            minimum_depth =
                                Some(minimum_depth.map_or(root.alias_depth, |depth: u8| {
                                    depth.min(root.alias_depth)
                                }));
                        }
                    }
                }
                NodeData::PropertyAccessExpression(data) => {
                    if let Some(expression) = data.expression {
                        worklist.push(expression);
                    }
                    continue;
                }
                _ => {
                    if !aliases_only
                        && this_root
                        && self.kind_of(current) == SyntaxKind::ThisKeyword
                    {
                        minimum_depth = Some(0);
                    }
                }
            }
            tsrs2_syntax::for_each_child(&source.arena, source.arena.node(current), |child| {
                worklist.push(child);
                false
            });
        }
        minimum_depth
    }

    /// Immutable lexical VALUE lookup — the `&self` face that
    /// `resolve_name` cannot offer (it caches, reports, and allocates
    /// suggestion symbols). Walks `locals` up the parent chain, then
    /// the globals table. Misses (import aliases whose VALUE-flag
    /// chase is unported, class members, anything else) stay None,
    /// which the root match treats as "cannot distinguish".
    /// tsrs-native: the [FLOW M5] gate's raw lexical probe — the
    /// NO-alias-chase getSymbol flavor, deliberately frozen (PR #7
    /// hardening): an import-alias root stays None = "cannot
    /// distinguish", which CONTAINS. The tsc-shaped chase lives in
    /// get_symbol_in_table for the name resolvers.
    fn resolve_lexical_value_symbol(&self, at: NodeId, name: &str) -> Option<SymbolId> {
        let probe = |table: &SymbolTable| -> Option<SymbolId> {
            let &symbol = table.get(name)?;
            let symbol = self.get_merged_symbol(symbol);
            self.binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::VALUE)
                .then_some(symbol)
        };
        let mut location = Some(at);
        while let Some(current) = location {
            if let Some(table) = self.binder.locals_of(current) {
                if let Some(symbol) = probe(table) {
                    return Some(symbol);
                }
            }
            location = self.parent_of(current);
        }
        probe(&self.globals)
    }

    /// Strip expression wrappers that do not change whether an
    /// initializer/condition can carry a narrowing fact.
    fn skip_flow_transparent_expression(&self, mut expression: NodeId) -> NodeId {
        loop {
            let inner = match self.data_of(expression) {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::PartiallyEmittedExpression(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                _ => None,
            };
            let Some(inner) = inner else {
                return expression;
            };
            expression = inner;
        }
    }

    /// Guard-shaped initializer classifier for `const ok = <cond>`
    /// aliases. `Unknown` is distinct from a proven non-predicate:
    /// until M5 can query resolved signatures, indirect/member/imported
    /// callees stay on the containment side to preserve FP=0.
    fn initializer_guard_certainty(&self, initializer: NodeId) -> FlowGuardCertainty {
        let initializer = self.skip_flow_transparent_expression(initializer);
        match self.data_of(initializer) {
            NodeData::BinaryExpression(bin) => {
                let operator = bin.operator_token.map(|token| self.kind_of(token));
                if matches!(
                    operator,
                    Some(
                        SyntaxKind::EqualsEqualsToken
                            | SyntaxKind::ExclamationEqualsToken
                            | SyntaxKind::EqualsEqualsEqualsToken
                            | SyntaxKind::ExclamationEqualsEqualsToken
                            | SyntaxKind::InstanceOfKeyword
                            | SyntaxKind::InKeyword
                            | SyntaxKind::AmpersandAmpersandToken
                            | SyntaxKind::BarBarToken
                            | SyntaxKind::QuestionQuestionToken
                    )
                ) {
                    FlowGuardCertainty::Yes
                } else {
                    FlowGuardCertainty::No
                }
            }
            NodeData::PrefixUnaryExpression(unary) => {
                if unary.operator != SyntaxKind::ExclamationToken {
                    FlowGuardCertainty::No
                } else if let Some(operand) = unary.operand {
                    let operand = self.skip_flow_transparent_expression(operand);
                    if matches!(self.data_of(operand), NodeData::CallExpression(_)) {
                        self.call_predicate_certainty(operand)
                    } else {
                        FlowGuardCertainty::Yes
                    }
                } else {
                    FlowGuardCertainty::Unknown
                }
            }
            NodeData::CallExpression(_) => self.call_predicate_certainty(initializer),
            _ => FlowGuardCertainty::No,
        }
    }

    /// Does this guard operand contain a root in a position that can
    /// actually narrow it? In particular, an argument occurrence in a
    /// locally-known boolean (non-predicate) call is not a narrowing
    /// occurrence (`if (notPred(x)) ...`).
    fn guard_operand_mentions_roots(
        &self,
        operand: NodeId,
        roots: &[FlowReferenceRoot<'_>],
        this_root: bool,
    ) -> bool {
        self.guard_operand_root_depth(operand, roots, this_root)
            .is_some()
    }

    fn guard_operand_root_depth(
        &self,
        operand: NodeId,
        roots: &[FlowReferenceRoot<'_>],
        this_root: bool,
    ) -> Option<u8> {
        let operand = self.skip_flow_transparent_expression(operand);
        match self.data_of(operand) {
            NodeData::CallExpression(data) => match self.call_predicate_effect(operand) {
                FlowPredicateEffect::Argument(index) => self
                    .nodes_of(data.arguments)
                    .get(index)
                    .and_then(|&argument| self.operand_root_depth(argument, roots, this_root)),
                FlowPredicateEffect::This => data.expression.and_then(|callee| {
                    let callee = self.skip_flow_transparent_expression(callee);
                    match self.data_of(callee) {
                        NodeData::PropertyAccessExpression(data) => data.expression,
                        NodeData::ElementAccessExpression(data) => data.expression,
                        _ => None,
                    }
                    .and_then(|receiver| self.operand_root_depth(receiver, roots, this_root))
                }),
                FlowPredicateEffect::None => None,
                FlowPredicateEffect::Unknown => self.operand_root_depth(operand, roots, this_root),
            },
            NodeData::PrefixUnaryExpression(data)
                if data.operator == SyntaxKind::ExclamationToken =>
            {
                data.operand
                    .and_then(|operand| self.guard_operand_root_depth(operand, roots, this_root))
            }
            NodeData::BinaryExpression(data) => {
                let operator = data.operator_token.map(|token| self.kind_of(token));
                let guard_operator = matches!(
                    operator,
                    Some(
                        SyntaxKind::EqualsEqualsToken
                            | SyntaxKind::ExclamationEqualsToken
                            | SyntaxKind::EqualsEqualsEqualsToken
                            | SyntaxKind::ExclamationEqualsEqualsToken
                            | SyntaxKind::InstanceOfKeyword
                            | SyntaxKind::InKeyword
                            | SyntaxKind::AmpersandAmpersandToken
                            | SyntaxKind::BarBarToken
                            | SyntaxKind::QuestionQuestionToken
                    )
                );
                if !guard_operator {
                    return None;
                }
                [data.left, data.right]
                    .into_iter()
                    .flatten()
                    .filter_map(|child| self.guard_operand_root_depth(child, roots, this_root))
                    .min()
            }
            _ => self.operand_root_depth(operand, roots, this_root),
        }
    }

    /// Predicate certainty for a call expression. Only an explicitly
    /// known non-predicate returns `No`; unsupported member/type-alias/
    /// imported forms are `Unknown`, which contains rather than emits a
    /// false positive.
    fn call_predicate_certainty(&self, call: NodeId) -> FlowGuardCertainty {
        self.call_predicate_effect(call).certainty()
    }

    /// Predicate effect for the SELECTED call signature. Reading the
    /// already-populated call cache is important for overloads: unioning
    /// all declarations would let an unrelated predicate overload
    /// suppress a real diagnostic. If resolution is unavailable, only
    /// unanimous declaration effects are considered known.
    fn call_predicate_effect(&self, call: NodeId) -> FlowPredicateEffect {
        let NodeData::CallExpression(data) = self.data_of(call) else {
            return FlowPredicateEffect::None;
        };
        if let Some(signature) = self.links.node(call).resolved_signature.resolved() {
            return self
                .signature_of(signature)
                .declaration
                .map_or(FlowPredicateEffect::Unknown, |declaration| {
                    self.declaration_predicate_effect(declaration)
                });
        }
        let mut callee = data.expression;
        while let Some(current) = callee {
            let normalized = self.skip_flow_transparent_expression(current);
            if normalized != current {
                callee = Some(normalized);
            } else {
                break;
            }
        }
        let Some(callee) = callee else {
            return FlowPredicateEffect::Unknown;
        };
        let NodeData::Identifier(name) = self.data_of(callee) else {
            return FlowPredicateEffect::Unknown;
        };
        match self.resolve_lexical_value_symbol(callee, &name.escaped_text) {
            Some(symbol) => {
                let declarations = &self.binder.symbol(symbol).declarations;
                let mut effect = None;
                for &declaration in declarations {
                    let declaration_effect = self.declaration_predicate_effect(declaration);
                    match effect {
                        None => effect = Some(declaration_effect),
                        Some(previous) if previous == declaration_effect => {}
                        Some(_) => return FlowPredicateEffect::Unknown,
                    }
                }
                effect.unwrap_or(FlowPredicateEffect::Unknown)
            }
            None => FlowPredicateEffect::Unknown,
        }
    }

    /// Syntactic predicate probe for a resolved lexical declaration.
    /// A TypeReference/intersection/member-shaped callable is not proof
    /// of a non-predicate; it remains Unknown until signature resolution
    /// lands in M5.
    fn declaration_predicate_effect(&self, declaration: NodeId) -> FlowPredicateEffect {
        if let Some(annotation) = self.type_annotation_of(declaration) {
            if self.kind_of(annotation) == SyntaxKind::TypePredicate {
                return self.type_predicate_effect(declaration, annotation);
            }
            // On a function-like declaration the annotation is the
            // function's RETURN type. A returned predicate function is
            // not a predicate of this call's argument.
            if tsrs2_binder::node_util::is_function_like_kind(self.kind_of(declaration)) {
                return FlowPredicateEffect::None;
            }
            // On variables/parameters a FunctionType annotation is the
            // callable's own signature.
            if let NodeData::FunctionType(data) = self.data_of(annotation) {
                return data.r#type.map_or(FlowPredicateEffect::None, |ret| {
                    if self.kind_of(ret) == SyntaxKind::TypePredicate {
                        self.type_predicate_effect(annotation, ret)
                    } else {
                        FlowPredicateEffect::None
                    }
                });
            }
            if matches!(
                self.kind_of(annotation),
                SyntaxKind::TypeReference | SyntaxKind::IntersectionType | SyntaxKind::UnionType
            ) {
                return FlowPredicateEffect::Unknown;
            }
            if self.kind_of(annotation) == SyntaxKind::ParenthesizedType {
                return FlowPredicateEffect::Unknown;
            }
            return FlowPredicateEffect::None;
        }
        if let NodeData::VariableDeclaration(data) = self.data_of(declaration) {
            if let Some(initializer) = data.initializer {
                if let Some(ret) = self.type_annotation_of(initializer) {
                    return if self.kind_of(ret) == SyntaxKind::TypePredicate {
                        self.type_predicate_effect(initializer, ret)
                    } else {
                        FlowPredicateEffect::None
                    };
                }
                // TypeScript can infer predicates for function-shaped
                // initializers; without resolved signatures this is not
                // a proven non-predicate.
                if tsrs2_binder::node_util::is_function_like_kind(self.kind_of(initializer)) {
                    return FlowPredicateEffect::Unknown;
                }
            }
        }
        FlowPredicateEffect::Unknown
    }

    /// Map a syntactic `x is T`/`asserts x` predicate back to the call
    /// argument it affects. A `this is T` predicate targets the method
    /// receiver instead.
    fn type_predicate_effect(&self, declaration: NodeId, predicate: NodeId) -> FlowPredicateEffect {
        let NodeData::TypePredicate(data) = self.data_of(predicate) else {
            return FlowPredicateEffect::Unknown;
        };
        let Some(parameter_name) = data.parameter_name else {
            return FlowPredicateEffect::Unknown;
        };
        if matches!(
            self.kind_of(parameter_name),
            SyntaxKind::ThisKeyword | SyntaxKind::ThisType
        ) {
            return FlowPredicateEffect::This;
        }
        let Some(predicate_name) = self.identifier_text_of(parameter_name) else {
            return FlowPredicateEffect::Unknown;
        };
        let parameter_list = match self.data_of(declaration) {
            NodeData::FunctionDeclaration(data) => data.parameters,
            NodeData::FunctionExpression(data) => data.parameters,
            NodeData::ArrowFunction(data) => data.parameters,
            NodeData::MethodDeclaration(data) => data.parameters,
            NodeData::GetAccessor(data) => data.parameters,
            NodeData::SetAccessor(data) => data.parameters,
            NodeData::Constructor(data) => data.parameters,
            NodeData::FunctionType(data) => data.parameters,
            NodeData::ConstructorType(data) => data.parameters,
            NodeData::CallSignature(data) => data.parameters,
            NodeData::ConstructSignature(data) => data.parameters,
            NodeData::MethodSignature(data) => data.parameters,
            _ => None,
        };
        let mut argument_index = 0;
        for parameter in self.nodes_of(parameter_list) {
            let NodeData::Parameter(data) = self.data_of(parameter) else {
                continue;
            };
            let name = data.name.and_then(|name| self.identifier_text_of(name));
            if name == Some("this") {
                continue;
            }
            if name == Some(predicate_name) {
                return FlowPredicateEffect::Argument(argument_index);
            }
            argument_index += 1;
        }
        FlowPredicateEffect::Unknown
    }

    /// Alias-expansion push: the binding names a qualifying
    /// declaration introduces join the root set, keyed by their
    /// DECLARED symbol (`node_symbol`, merged) so a same-named shadow
    /// elsewhere stays unrelated. Returns whether anything new landed.
    fn push_alias_binding_roots<'r>(
        &'r self,
        declaration: NodeId,
        name: NodeId,
        alias_depth: u8,
        dependent: bool,
        roots: &mut Vec<FlowReferenceRoot<'r>>,
    ) -> bool {
        let source = self.binder.source_of_node(name);
        let available_from = source.arena.node(declaration).end;
        let mut grew = false;
        let mut push =
            |roots: &mut Vec<FlowReferenceRoot<'r>>, text: &'r str, declaring: NodeId| {
                let symbol = self
                    .binder
                    .node_symbol(declaring)
                    .map(|symbol| self.get_merged_symbol(symbol));
                let entry = FlowReferenceRoot {
                    text,
                    symbol,
                    available_from,
                    alias_depth,
                    dependent,
                };
                if !roots.contains(&entry) {
                    roots.push(entry);
                    grew = true;
                }
            };
        match self.data_of(name) {
            NodeData::Identifier(data) => push(roots, &data.escaped_text, declaration),
            NodeData::ObjectBindingPattern(_) | NodeData::ArrayBindingPattern(_) => {
                let mut worklist = vec![name];
                while let Some(current) = worklist.pop() {
                    if let NodeData::BindingElement(element) = self.data_of(current) {
                        if let Some(element_name) = element.name {
                            match self.data_of(element_name) {
                                NodeData::Identifier(data) => {
                                    push(roots, &data.escaped_text, current);
                                }
                                NodeData::ObjectBindingPattern(_)
                                | NodeData::ArrayBindingPattern(_) => {
                                    worklist.push(element_name);
                                }
                                _ => {}
                            }
                        }
                        continue;
                    }
                    tsrs2_syntax::for_each_child(
                        &source.arena,
                        source.arena.node(current),
                        |child| {
                            worklist.push(child);
                            false
                        },
                    );
                }
            }
            _ => {}
        }
        grew
    }

    /// The structural reach face: guard G (narrowing operand C) may
    /// retype reference R iff narrowed flow can arrive at R — via a
    /// shared iteration statement's back edge (order-free), from
    /// inside R itself (the guard participates in producing R's
    /// value), from a condition into its own limbs (R after C inside
    /// G — `if (x.missing) {}` reports), or forward into later code
    /// (G before R), except across sibling if/conditional limbs,
    /// which rejoin without the narrowing.
    fn guard_reaches_reference(&self, guard: NodeId, operand: NodeId, reference: NodeId) -> bool {
        let source = self.binder.source_of_node(guard);
        // R's inclusive ancestor path, deepest first.
        let mut reference_path = Vec::new();
        let mut cursor = Some(reference);
        while let Some(node) = cursor {
            reference_path.push(node);
            cursor = self.parent_of(node);
        }
        let reference_index: std::collections::HashMap<NodeId, usize> = reference_path
            .iter()
            .enumerate()
            .map(|(index, &node)| (node, index))
            .collect();
        // Lowest common ancestor, inclusive of G and R themselves;
        // remember each side's child under it.
        let mut guard_child = None;
        let mut guard_side = guard;
        let (lca, lca_index) = loop {
            if let Some(&index) = reference_index.get(&guard_side) {
                break (guard_side, index);
            }
            guard_child = Some(guard_side);
            match self.parent_of(guard_side) {
                Some(parent) => guard_side = parent,
                None => return false,
            }
        };
        let reference_child = (lca_index > 0).then(|| reference_path[lca_index - 1]);
        // 1) Back-edge waiver: a common enclosing iteration statement
        //    carries later guards to earlier reads.
        let mut cursor = Some(lca);
        while let Some(node) = cursor {
            if self.is_iteration_statement(node, false) {
                return true;
            }
            cursor = self.parent_of(node);
        }
        // 2) Guard inside the reference: `take(ok ? a : b)` — the
        //    conditional narrows its own branches.
        if lca == reference {
            return true;
        }
        // 3) R inside the guard construct: narrowed from the operand
        //    on — its limbs and trailing clauses, not the operand
        //    itself.
        if lca == guard {
            return source.arena.node(reference).pos >= source.arena.node(operand).end;
        }
        // 4) Disjoint: forward order only, minus sibling limbs.
        if source.arena.node(guard).end > source.arena.node(reference).pos {
            return false;
        }
        let (Some(guard_child), Some(reference_child)) = (guard_child, reference_child) else {
            return true;
        };
        match self.data_of(lca) {
            NodeData::IfStatement(data) => {
                let guard_then = data.then_statement == Some(guard_child);
                let guard_else = data.else_statement == Some(guard_child);
                let reference_then = data.then_statement == Some(reference_child);
                let reference_else = data.else_statement == Some(reference_child);
                !((guard_then && reference_else) || (guard_else && reference_then))
            }
            NodeData::ConditionalExpression(data) => {
                let guard_true = data.when_true == Some(guard_child);
                let guard_false = data.when_false == Some(guard_child);
                let reference_true = data.when_true == Some(reference_child);
                let reference_false = data.when_false == Some(reference_child);
                !((guard_true && reference_false) || (guard_false && reference_true))
            }
            NodeData::TryStatement(data) => {
                let regions = [data.try_block, data.catch_clause, data.finally_block];
                let guard_region = regions
                    .iter()
                    .position(|&region| region == Some(guard_child));
                let reference_region = regions
                    .iter()
                    .position(|&region| region == Some(reference_child));
                // Narrowing established in try/catch/finally does not
                // transfer into a sibling exception region. Within one
                // region the lower LCA would have handled ordinary flow.
                !matches!(
                    (guard_region, reference_region),
                    (Some(guard), Some(reference)) if guard != reference
                )
            }
            _ => true,
        }
    }

    pub(crate) fn check_non_null_type(&mut self, ty: TypeId, node: NodeId) -> CheckResult2<TypeId> {
        self.check_non_null_type_with_reporter(
            ty,
            node,
            Self::report_object_possibly_null_or_undefined_error,
        )
    }

    /// tsc-port: checkNonNullNonVoidType @6.0.3
    /// tsc-hash: 8a9444b51ee2d2fad8646f1fe67adeda802f49e5a4f62c88c2adaf356f41e9c8
    /// tsc-span: _tsc.js:75051-75068
    ///
    /// The 5.8a declaration-band consumer (checkVariableLikeDeclaration
    /// binding-pattern arms): `node` is the DECLARATION there, so the
    /// entity-name faces are transcribed but reachable only from the
    /// expression-side callers.
    pub(crate) fn check_non_null_non_void_type(
        &mut self,
        ty: TypeId,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        let non_null_type = self.check_non_null_type(ty, node)?;
        if self
            .tables
            .flags_of(non_null_type)
            .intersects(TypeFlags::VOID)
        {
            // [FLOW M5] narrowable-receiver containment — the same
            // gate check_non_null_type_with_reporter applies; a
            // guarded `this.x` chain tsc narrows past void/undefined
            // would FP here (typeofThis pins the typeof-query face).
            if self.receiver_may_be_flow_narrowed(node) {
                return Err(Unsupported::new(
                    "[FLOW M5] void receiver on a narrowable reference",
                ));
            }
            if self.is_entity_name_expression(node) {
                let node_text = self.entity_name_to_string(node)?;
                if self.kind_of(node) == SyntaxKind::Identifier && node_text == "undefined" {
                    self.error_at(
                        Some(node),
                        &tsrs2_diags::gen::The_value_0_cannot_be_used_here,
                        &[&node_text],
                    );
                    return Ok(non_null_type);
                }
                if node_text.encode_utf16().count() < 100 {
                    self.error_at(
                        Some(node),
                        &tsrs2_diags::gen::_0_is_possibly_undefined,
                        &[&node_text],
                    );
                    return Ok(non_null_type);
                }
            }
            self.error_at(
                Some(node),
                &tsrs2_diags::gen::Object_is_possibly_undefined,
                &[],
            );
        }
        Ok(non_null_type)
    }

    /// tsc-port: reportCannotInvokePossiblyNullOrUndefinedError @6.0.3
    /// tsc-hash: b3748b887956c0833de220ae247af85d956add778ef31dd09c491063e8aaf39b
    /// tsc-span: _tsc.js:75022-75027
    ///
    /// The Invoke reporter flavor (resolveCallExpression 77002) — the
    /// [FLOW M5] narrowable-receiver containment gate rides in
    /// check_non_null_type_with_reporter unchanged.
    pub(crate) fn report_cannot_invoke_possibly_null_or_undefined_error(
        &mut self,
        node: NodeId,
        facts: TypeFacts,
    ) -> CheckResult2<()> {
        let message = if facts.intersects(TypeFacts::IS_UNDEFINED) {
            if facts.intersects(TypeFacts::IS_NULL) {
                &tsrs2_diags::gen::Cannot_invoke_an_object_which_is_possibly_null_or_undefined
            } else {
                &tsrs2_diags::gen::Cannot_invoke_an_object_which_is_possibly_undefined
            }
        } else {
            &tsrs2_diags::gen::Cannot_invoke_an_object_which_is_possibly_null
        };
        self.error_at(Some(node), message, &[]);
        Ok(())
    }

    // ---- property accessibility (74871-74989) ----

    /// tsc-port: checkPropertyAccessibility @6.0.3
    /// tsc-hash: 618747038424684a935dd42963c96bfb8bf2661f6e8f1ba3685110747bcbc1e4
    /// tsc-span: _tsc.js:74871-74989
    ///
    /// checkPropertyAccessibility + checkPropertyAccessibilityAtLocation
    /// + getEnclosingClassFromThisParameter + getThisParameterFromNode-
    ///   Context + symbolHasNonMethodDeclaration, one block. Arm order
    ///   is the observable: super-ES5 2340 → super-abstract 2513 →
    ///   super-instance-field 2855 → abstract-in-ctor 2715 → non-public
    ///   fast-path → private 2341 → super-protected OK → protected
    ///   derivation 2445 → static OK → type-parameter constraint →
    ///   hasBaseType 2446.
    pub(crate) fn check_property_accessibility(
        &mut self,
        node: NodeId,
        is_super: bool,
        writing: bool,
        ty: TypeId,
        prop: SymbolId,
        report_error: bool,
    ) -> CheckResult2<bool> {
        let error_node = if !report_error {
            None
        } else {
            match self.kind_of(node) {
                SyntaxKind::QualifiedName => {
                    let NodeData::QualifiedName(data) = self.data_of(node) else {
                        unreachable!("kind/data agree");
                    };
                    data.right
                }
                SyntaxKind::ImportType => Some(node),
                SyntaxKind::BindingElement => {
                    let NodeData::BindingElement(data) = self.data_of(node) else {
                        unreachable!("kind/data agree");
                    };
                    data.property_name.or_else(|| self.name_of_node(node))
                }
                _ => self.name_of_node(node),
            }
        };
        self.check_property_accessibility_at_location(node, is_super, writing, ty, prop, error_node)
    }

    fn check_property_accessibility_at_location(
        &mut self,
        location: NodeId,
        is_super: bool,
        writing: bool,
        containing_type: TypeId,
        prop: SymbolId,
        error_node: Option<NodeId>,
    ) -> CheckResult2<bool> {
        let flags = self.get_declaration_modifier_flags_from_symbol_write(prop, writing);
        if is_super {
            if self.options.emit_script_target() < tsrs2_types::ScriptTarget::ES2015
                && self.symbol_has_non_method_declaration(prop)?
            {
                if let Some(error_node) = error_node {
                    self.error_at(
                        Some(error_node),
                        &tsrs2_diags::gen::Only_public_and_protected_methods_of_the_base_class_are_accessible_via_the_super_keyword,
                        &[],
                    );
                }
                return Ok(false);
            }
            if flags.intersects(ModifierFlags::ABSTRACT) {
                if let Some(error_node) = error_node {
                    let prop_name = self.symbol_display_name(prop);
                    let class_name = match self.get_declaring_class(prop)? {
                        Some(class) => self.type_to_string_slice(class)?,
                        None => {
                            return Err(Unsupported::new(
                                "abstract member without declaring class (recovery)",
                            ))
                        }
                    };
                    self.error_at(
                        Some(error_node),
                        &tsrs2_diags::gen::Abstract_method_0_in_class_1_cannot_be_accessed_via_super_expression,
                        &[&prop_name, &class_name],
                    );
                }
                return Ok(false);
            }
            if !flags.intersects(ModifierFlags::STATIC)
                && self
                    .binder
                    .symbol(prop)
                    .declarations
                    .iter()
                    .copied()
                    .any(|declaration| self.is_class_instance_property(declaration))
            {
                if let Some(error_node) = error_node {
                    let prop_name = self.symbol_display_name(prop);
                    self.error_at(
                        Some(error_node),
                        &tsrs2_diags::gen::Class_field_0_defined_by_the_parent_class_is_not_accessible_in_the_child_class_via_super,
                        &[&prop_name],
                    );
                }
                return Ok(false);
            }
        }
        if flags.intersects(ModifierFlags::ABSTRACT)
            && self.symbol_has_non_method_declaration(prop)?
            && (self.is_this_property(location)
                || self.is_this_initialized_object_binding_expression(location)
                || self.parent_of(location).is_some_and(|parent| {
                    self.kind_of(parent) == SyntaxKind::ObjectBindingPattern
                        && self
                            .parent_of(parent)
                            .is_some_and(|grand| self.is_this_initialized_declaration(grand))
                }))
        {
            if let Some(parent_symbol) = self.get_parent_of_symbol(prop) {
                if self
                    .binder
                    .symbol(parent_symbol)
                    .flags
                    .intersects(SymbolFlags::CLASS)
                    && self.is_node_used_during_class_initialization(location)
                {
                    if let Some(error_node) = error_node {
                        let prop_name = self.symbol_display_name(prop);
                        let class_name = self.symbol_display_name(parent_symbol);
                        self.error_at(
                            Some(error_node),
                            &tsrs2_diags::gen::Abstract_property_0_in_class_1_cannot_be_accessed_in_the_constructor,
                            &[&prop_name, &class_name],
                        );
                    }
                    return Ok(false);
                }
            }
        }
        if !flags.intersects(ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER) {
            return Ok(true);
        }
        if flags.intersects(ModifierFlags::PRIVATE) {
            let declaring_class_declaration = self
                .get_parent_of_symbol(prop)
                .and_then(|parent| self.get_class_like_declaration_of_symbol(parent));
            if !self.is_node_within_class(location, declaring_class_declaration) {
                if let Some(error_node) = error_node {
                    let prop_name = self.symbol_display_name(prop);
                    let class_name = match self.get_declaring_class(prop)? {
                        Some(class) => self.type_to_string_slice(class)?,
                        None => {
                            return Err(Unsupported::new(
                                "private member without declaring class (recovery)",
                            ))
                        }
                    };
                    self.error_at(
                        Some(error_node),
                        &tsrs2_diags::gen::Property_0_is_private_and_only_accessible_within_class_1,
                        &[&prop_name, &class_name],
                    );
                }
                return Ok(false);
            }
            return Ok(true);
        }
        if is_super {
            return Ok(true);
        }
        let mut enclosing_class: Option<TypeId> = None;
        let mut containing = Some(self.get_containing_class_of(location));
        while let Some(Some(class_declaration)) = containing {
            let class_symbol = self.get_symbol_of_declaration(class_declaration)?;
            let declared = self.get_declared_type_of_class_or_interface(class_symbol)?;
            if let Some(derived) =
                self.is_class_derived_from_declaring_classes(declared, prop, writing)?
            {
                enclosing_class = Some(derived);
                break;
            }
            containing = Some(self.get_containing_class_of(class_declaration));
        }
        if enclosing_class.is_none() {
            let mut this_class = self.get_enclosing_class_from_this_parameter(location)?;
            if let Some(candidate) = this_class {
                this_class =
                    self.is_class_derived_from_declaring_classes(candidate, prop, writing)?;
            }
            if flags.intersects(ModifierFlags::STATIC) || this_class.is_none() {
                if let Some(error_node) = error_node {
                    let prop_name = self.symbol_display_name(prop);
                    let class = match self.get_declaring_class(prop)? {
                        Some(class) => class,
                        None => containing_type,
                    };
                    let class_name = self.type_to_string_slice(class)?;
                    self.error_at(
                        Some(error_node),
                        &tsrs2_diags::gen::Property_0_is_protected_and_only_accessible_within_class_1_and_its_subclasses,
                        &[&prop_name, &class_name],
                    );
                }
                return Ok(false);
            }
            enclosing_class = this_class;
        }
        let enclosing_class = enclosing_class.expect("assigned above");
        if flags.intersects(ModifierFlags::STATIC) {
            return Ok(true);
        }
        let mut containing_type = Some(containing_type);
        if self
            .tables
            .flags_of(containing_type.expect("set above"))
            .intersects(TypeFlags::TYPE_PARAMETER)
        {
            let current = containing_type.expect("set above");
            let is_this_type = matches!(
                &self.tables.type_of(current).data,
                tsrs2_types::TypeData::TypeParameter {
                    is_this_type: true,
                    ..
                }
            );
            containing_type = if is_this_type {
                self.get_constraint_of_type_parameter(current)?
            } else {
                self.get_base_constraint_of_type(current)?
            };
        }
        let has_base = match containing_type {
            Some(containing) => self.has_base_type(containing, enclosing_class)?,
            None => false,
        };
        if !has_base {
            if let Some(error_node) = error_node {
                let prop_name = self.symbol_display_name(prop);
                let enclosing_name = self.type_to_string_slice(enclosing_class)?;
                let containing_name = match containing_type {
                    Some(containing) => self.type_to_string_slice(containing)?,
                    None => {
                        return Err(Unsupported::new(
                            "protected-instance report without containing type",
                        ))
                    }
                };
                self.error_at(
                    Some(error_node),
                    &tsrs2_diags::gen::Property_0_is_protected_and_only_accessible_through_an_instance_of_class_1_This_is_an_instance_of_class_2,
                    &[&prop_name, &enclosing_name, &containing_name],
                );
            }
            return Ok(false);
        }
        Ok(true)
    }

    fn get_enclosing_class_from_this_parameter(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let source = self.binder.source_of_node(node);
        let this_container =
            node_util::get_this_container(source, node, /*include_arrow_functions*/ false);
        let this_parameter = this_container
            .filter(|&container| {
                tsrs2_binder::node_util::is_function_like_kind(self.kind_of(container))
            })
            .and_then(|container| self.this_parameter_node_of(container));
        let mut this_type =
            match this_parameter.and_then(|parameter| self.type_annotation_of(parameter)) {
                Some(annotation) => Some(self.get_type_from_type_node(annotation)?),
                None => None,
            };
        if let Some(current) = this_type {
            if self
                .tables
                .flags_of(current)
                .intersects(TypeFlags::TYPE_PARAMETER)
            {
                this_type = self.get_constraint_of_type_parameter(current)?;
            }
        } else if let Some(container) = this_container {
            if tsrs2_binder::node_util::is_function_like_kind(self.kind_of(container)) {
                this_type = self.get_contextual_this_parameter_type(container)?;
            }
        }
        if let Some(this_type) = this_type {
            if self.tables.object_flags_of(this_type).intersects(
                tsrs2_types::ObjectFlags::CLASS_OR_INTERFACE | tsrs2_types::ObjectFlags::REFERENCE,
            ) {
                return Ok(Some(self.get_target_type(this_type)));
            }
        }
        Ok(None)
    }

    fn symbol_has_non_method_declaration(&mut self, symbol: SymbolId) -> CheckResult2<bool> {
        self.for_each_property_bool(symbol, &mut |state, prop| {
            Ok(!state
                .binder
                .symbol(prop)
                .flags
                .intersects(SymbolFlags::METHOD))
        })
    }

    /// tsc-port: forEachProperty @6.0.3
    /// tsc-hash: 128739109b1041971a4dc3005ddb74ae627cdcbc16248bde54194cc9fbe2d18b
    /// tsc-span: _tsc.js:67432-67443
    ///
    /// The boolean specialization (every live consumer here returns a
    /// truthiness verdict): Synthetic properties recurse through their
    /// containingType constituents.
    fn for_each_property_bool(
        &mut self,
        prop: SymbolId,
        callback: &mut dyn FnMut(&mut Self, SymbolId) -> CheckResult2<bool>,
    ) -> CheckResult2<bool> {
        if self
            .get_check_flags(prop)
            .intersects(tsrs2_types::CheckFlags::SYNTHETIC)
        {
            let containing = self
                .links
                .symbol(prop)
                .containing_type
                .expect("Synthetic check flag implies containing type");
            let name = self.binder.symbol(prop).escaped_name.clone();
            let constituents: Vec<TypeId> = match &self.tables.type_of(containing).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                tsrs2_types::TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("synthetic containing type is a union or intersection"),
            };
            for constituent in constituents {
                if let Some(member) = self.get_property_of_type_full(constituent, &name)? {
                    if self.for_each_property_bool(member, callback)? {
                        return Ok(true);
                    }
                }
            }
            return Ok(false);
        }
        callback(self, prop)
    }

    /// tsc-port: isClassDerivedFromDeclaringClasses @6.0.3
    /// tsc-hash: 2a68f84fe590f0c893a27d889c275ed89d9003e2f7c9ec3991e70d6942fde8cd
    /// tsc-span: _tsc.js:67462-67465
    fn is_class_derived_from_declaring_classes(
        &mut self,
        check_class: TypeId,
        prop: SymbolId,
        writing: bool,
    ) -> CheckResult2<Option<TypeId>> {
        let blocked = self.for_each_property_bool(prop, &mut |state, p| {
            if state
                .get_declaration_modifier_flags_from_symbol_write(p, writing)
                .intersects(ModifierFlags::PROTECTED)
            {
                match state.get_declaring_class(p)? {
                    Some(declaring) => Ok(!state.has_base_type(check_class, declaring)?),
                    None => Ok(true),
                }
            } else {
                Ok(false)
            }
        })?;
        Ok(if blocked { None } else { Some(check_class) })
    }

    /// tsc-port: forEachEnclosingClass @6.0.3
    /// tsc-hash: 473286bbde1a0ff408905a5d5f0833cd6c37d75ca6ba96c30b069679d9184ced
    /// tsc-span: _tsc.js:87230-87251
    pub(crate) fn get_containing_class_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::get_containing_class(self.binder.source_of_node(node), node)
    }

    fn is_node_used_during_class_initialization(&self, node: NodeId) -> bool {
        self.find_ancestor(Some(node), |state, element| {
            let kind = state.kind_of(element);
            let ctor_with_body = kind == SyntaxKind::Constructor
                && node_util::body_of(state.binder.source_of_node(element), element).is_some();
            if ctor_with_body || kind == SyntaxKind::PropertyDeclaration {
                crate::expr::Ancestor::Yes
            } else if matches!(
                kind,
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) || tsrs2_binder::node_util::is_function_like_declaration_kind(kind)
            {
                crate::expr::Ancestor::Quit
            } else {
                crate::expr::Ancestor::No
            }
        })
        .is_some()
    }

    pub(crate) fn is_node_within_class(
        &self,
        node: NodeId,
        class_declaration: Option<NodeId>,
    ) -> bool {
        let Some(class_declaration) = class_declaration else {
            return false;
        };
        let mut containing = self.get_containing_class_of(node);
        while let Some(class) = containing {
            if class == class_declaration {
                return true;
            }
            containing = self.get_containing_class_of(class);
        }
        false
    }

    /// tsc-port: getClassLikeDeclarationOfSymbol @6.0.3
    /// tsc-hash: acf690079471ca724f4c20e7f54ec1e0de2863874ad80b01bf0c9e3b25b0f18a
    /// tsc-span: _tsc.js:17548-17551
    pub(crate) fn get_class_like_declaration_of_symbol(&self, symbol: SymbolId) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| {
                matches!(
                    self.kind_of(declaration),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                )
            })
    }

    /// tsc-port: isClassInstanceProperty @6.0.3
    /// tsc-hash: 23e17175464d30e2bc154426a18df24b8c5ee1319551ad5bced0feff6ebcde79
    /// tsc-span: _tsc.js:12049-12058
    ///
    /// The isInJSFile expando arm is [JSDOC]-gated (plain-JS band);
    /// TS files take the class-parent property-declaration test.
    fn is_class_instance_property(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        matches!(
            self.kind_of(parent),
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
        ) && self.kind_of(node) == SyntaxKind::PropertyDeclaration
            && !tsrs2_binder::node_util::get_combined_modifier_flags(
                self.binder.source_of_node(node),
                node,
            )
            .intersects(ModifierFlags::ACCESSOR)
    }

    /// tsc-port: isThisProperty @6.0.3
    /// tsc-hash: 00057dc50395f95f80ad950b1020aa661aa6ce6404eed788d5235e78bc996404
    /// tsc-span: _tsc.js:14612-14622
    fn is_this_property(&self, node: NodeId) -> bool {
        let expression = match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data.expression,
            NodeData::ElementAccessExpression(data) => data.expression,
            _ => return false,
        };
        expression.is_some_and(|expression| self.kind_of(expression) == SyntaxKind::ThisKeyword)
    }

    fn is_this_initialized_declaration(&self, node: NodeId) -> bool {
        let NodeData::VariableDeclaration(data) = self.data_of(node) else {
            return false;
        };
        data.initializer
            .is_some_and(|initializer| self.kind_of(initializer) == SyntaxKind::ThisKeyword)
    }

    fn is_this_initialized_object_binding_expression(&self, node: NodeId) -> bool {
        if !matches!(
            self.kind_of(node),
            SyntaxKind::ShorthandPropertyAssignment | SyntaxKind::PropertyAssignment
        ) {
            return false;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        let Some(grand) = self.parent_of(parent) else {
            return false;
        };
        let NodeData::BinaryExpression(data) = self.data_of(grand) else {
            return false;
        };
        data.operator_token
            .is_some_and(|operator| self.kind_of(operator) == SyntaxKind::EqualsToken)
            && data
                .right
                .is_some_and(|right| self.kind_of(right) == SyntaxKind::ThisKeyword)
    }

    // ---- validity probes (no reporting) ----

    /// tsc-port: isValidPropertyAccessForCompletions @6.0.3
    /// tsc-hash: 949fafb835b56c4e7369a29f47988f34f591ce99edb4862c902bbd16a1575614
    /// tsc-span: _tsc.js:75644-75677
    pub(crate) fn is_valid_property_access_for_completions(
        &mut self,
        node: NodeId,
        ty: TypeId,
        property: SymbolId,
    ) -> CheckResult2<bool> {
        let is_super = self.kind_of(node) == SyntaxKind::PropertyAccessExpression
            && match self.data_of(node) {
                NodeData::PropertyAccessExpression(data) => data
                    .expression
                    .is_some_and(|expression| self.kind_of(expression) == SyntaxKind::SuperKeyword),
                _ => false,
            };
        self.is_property_accessible(node, is_super, /*is_write*/ false, ty, property)
    }

    fn is_property_accessible(
        &mut self,
        node: NodeId,
        is_super: bool,
        is_write: bool,
        containing_type: TypeId,
        property: SymbolId,
    ) -> CheckResult2<bool> {
        // tsc isTypeAny (type.flags & Any).
        if self
            .tables
            .flags_of(containing_type)
            .intersects(TypeFlags::ANY)
        {
            return Ok(true);
        }
        if let Some(value_declaration) = self.binder.symbol(property).value_declaration {
            let is_private_element = matches!(
                self.kind_of(value_declaration),
                SyntaxKind::PropertyDeclaration
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
            ) && self
                .name_of_node(value_declaration)
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier);
            if is_private_element {
                let decl_class = self.get_containing_class_of(value_declaration);
                let source = self.binder.source_of_node(node);
                return Ok(!node_util::is_optional_chain(source, node)
                    && decl_class.is_some_and(|class| {
                        self.find_ancestor(Some(node), |_, parent| {
                            if parent == class {
                                crate::expr::Ancestor::Yes
                            } else {
                                crate::expr::Ancestor::No
                            }
                        })
                        .is_some()
                    }));
            }
        }
        self.check_property_accessibility_at_location(
            node,
            is_super,
            is_write,
            containing_type,
            property,
            /*error_node*/ None,
        )
    }

    // ---- the `x!` assertion arms (worker arm 236) ----

    /// tsc-port: checkNonNullChain @6.0.3
    /// tsc-hash: 0e4e137714c21841cd24acb8f2e6ffe876248461b58de19752270b958adf6854
    /// tsc-span: _tsc.js:77955-77962
    ///
    /// NO error path (§6 corrected 2026-07-12): `x!` strips silently —
    /// checkNonNullNonVoidType's consumers are the 5.8 declaration
    /// sites. `x!` on void → never, no diagnostic (pinned).
    pub(crate) fn check_non_null_assertion(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::NonNullExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(expression) = data.expression else {
            return Err(Unsupported::new("NonNullExpression recovery node"));
        };
        let source = self.binder.source_of_node(node);
        if node_util::node_flags(source, node).intersects(NodeFlags::OPTIONAL_CHAIN) {
            // checkNonNullChain (77955-77959).
            let left_type = self.check_expression(expression, CheckMode::NORMAL)?;
            let non_optional_type = self.get_optional_expression_type(left_type, expression)?;
            let stripped = self.get_non_nullable_type(non_optional_type)?;
            return self.propagate_optional_type_marker(
                stripped,
                node,
                non_optional_type != left_type,
            );
        }
        let ty = self.check_expression(expression, CheckMode::NORMAL)?;
        self.get_non_nullable_type(ty)
    }
}

// ---- M4 5.5d: the property-access band (extraction doc §6) ----

impl<'a> CheckerState<'a> {
    /// tsc-port: checkPropertyAccessExpression @6.0.3
    /// tsc-hash: b901cf55eabb72cda82207bc6a0b1660873bd3ab99d58747182a5635ad675479
    /// tsc-span: _tsc.js:75069-75086
    pub(crate) fn check_property_access_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
        write_only: bool,
    ) -> CheckResult2<TypeId> {
        let NodeData::PropertyAccessExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (Some(expression), Some(name)) = (data.expression, data.name) else {
            return Err(Unsupported::new("PropertyAccessExpression recovery node"));
        };
        let source = self.binder.source_of_node(node);
        if node_util::node_flags(source, node).intersects(NodeFlags::OPTIONAL_CHAIN) {
            return self.check_property_access_chain(node, check_mode);
        }
        let left_type = self.check_non_null_expression(expression)?;
        self.check_property_access_expression_or_qualified_name(
            node, expression, left_type, name, check_mode, write_only,
        )
    }

    fn check_property_access_chain(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let NodeData::PropertyAccessExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (Some(expression), Some(name)) = (data.expression, data.name) else {
            return Err(Unsupported::new("PropertyAccessExpression recovery node"));
        };
        let left_type = self.check_expression(expression, CheckMode::NORMAL)?;
        let non_optional_type = self.get_optional_expression_type(left_type, expression)?;
        let checked = self.check_non_null_type(non_optional_type, expression)?;
        let access_type = self.check_property_access_expression_or_qualified_name(
            node, expression, checked, name, check_mode, /*write_only*/ false,
        )?;
        self.propagate_optional_type_marker(access_type, node, non_optional_type != left_type)
    }

    pub(crate) fn check_qualified_name(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let NodeData::QualifiedName(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (Some(left), Some(right)) = (data.left, data.right) else {
            return Err(Unsupported::new("QualifiedName recovery node"));
        };
        let source = self.binder.source_of_node(node);
        let left_type =
            if node_util::is_part_of_type_query(source, node) && self.is_this_identifier(left) {
                let this_type = self.check_this_expression(left)?;
                self.check_non_null_type(this_type, left)?
            } else {
                self.check_non_null_expression(left)?
            };
        self.check_property_access_expression_or_qualified_name(
            node, left, left_type, right, check_mode, /*write_only*/ false,
        )
    }

    fn is_method_access_for_call(&self, node: NodeId) -> bool {
        let mut node = node;
        while let Some(parent) = self.parent_of(node) {
            if self.kind_of(parent) == SyntaxKind::ParenthesizedExpression {
                node = parent;
            } else {
                break;
            }
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        matches!(
            self.kind_of(parent),
            SyntaxKind::CallExpression | SyntaxKind::NewExpression
        ) && match self.data_of(parent) {
            NodeData::CallExpression(data) => data.expression == Some(node),
            NodeData::NewExpression(data) => data.expression == Some(node),
            _ => false,
        }
    }

    // ---- private-identifier family ----

    /// tsc-port: lookupSymbolForPrivateIdentifierDeclaration @6.0.3
    /// tsc-hash: 04d25f857376660c473c5a36a0fb50e4842554a80d0452ddfb016b891250bd65
    /// tsc-span: _tsc.js:75087-75098
    ///
    /// getSymbolNameForPrivateIdentifier mangles `__#{classId}@{text}`;
    /// the bind-time classId is a binder-private counter, so the lookup
    /// matches by the `__#` prefix + `@{text}` suffix — exact, because
    /// one class cannot declare two privates with the same text.
    fn lookup_symbol_for_private_identifier_declaration(
        &mut self,
        prop_name: &str,
        location: NodeId,
    ) -> CheckResult2<Option<SymbolId>> {
        let mut containing = self.get_containing_class_excluding_class_decorators(location);
        while let Some(class) = containing {
            let Some(symbol) = self.binder.node_symbol(class) else {
                containing = self.get_containing_class_of(class);
                continue;
            };
            let suffix = format!("@{prop_name}");
            let found = {
                let symbol_data = self.binder.symbol(symbol);
                let in_members = symbol_data.members.iter().find_map(|(name, &member)| {
                    (name.starts_with("__#") && name.ends_with(&suffix)).then_some(member)
                });
                in_members.or_else(|| {
                    symbol_data.exports.iter().find_map(|(name, &member)| {
                        (name.starts_with("__#") && name.ends_with(&suffix)).then_some(member)
                    })
                })
            };
            if let Some(found) = found {
                return Ok(Some(found));
            }
            containing = self.get_containing_class_of(class);
        }
        Ok(None)
    }

    /// getContainingClassExcludingClassDecorators (17520-ish): like
    /// getContainingClass but a decorator's class does not contain its
    /// own decorator expressions.
    fn get_containing_class_excluding_class_decorators(&self, node: NodeId) -> Option<NodeId> {
        // Walk up past a class's OWN decorators: find the decorator
        // ancestor first; when its parent is class-like, start the
        // containing-class walk from the class's parent.
        let source = self.binder.source_of_node(node);
        let mut decorator_class: Option<NodeId> = None;
        let mut probe = node;
        while let Some(parent) = self.parent_of(probe) {
            if self.kind_of(probe) == SyntaxKind::Decorator
                && matches!(
                    self.kind_of(parent),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                )
            {
                decorator_class = Some(parent);
                break;
            }
            probe = parent;
        }
        let start = match decorator_class {
            Some(class) => self.parent_of(class)?,
            None => node,
        };
        node_util::get_containing_class(source, start)
    }

    /// tsc-port: checkPrivateIdentifierExpression @6.0.3
    /// tsc-hash: cf4b94ca769673162d6328f5aecd25f56b8057f143c75613b7deb065efd685c9
    /// tsc-span: _tsc.js:75099-75138
    ///
    /// checkGrammarPrivateIdentifierExpression's 18016/1005-family
    /// walks are parse-adjacent grammar; the expression-position 2304
    /// arm is the semantic slice (`#x` outside a class member access).
    pub(crate) fn check_private_identifier_expression(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        self.check_grammar_private_identifier_expression(node)?;
        let symbol = self.get_symbol_for_private_identifier_expression(node)?;
        if let Some(symbol) = symbol {
            self.mark_property_as_referenced(symbol, None, /*is_self_type_access*/ false);
        }
        Ok(self.tables.intrinsics.any)
    }

    fn check_grammar_private_identifier_expression(&mut self, priv_id: NodeId) -> CheckResult2<()> {
        let source = self.binder.source_of_node(priv_id);
        if node_util::get_containing_class(source, priv_id).is_none() {
            self.grammar_error_on_node(
                priv_id,
                &tsrs2_diags::gen::Private_identifiers_are_not_allowed_outside_class_bodies,
                &[],
            );
            return Ok(());
        }
        let parent_is_for_in = self
            .parent_of(priv_id)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ForInStatement);
        if !parent_is_for_in {
            if !node_util::is_expression_node(self.binder.source_of_node(priv_id), priv_id) {
                self.grammar_error_on_node(
                    priv_id,
                    &tsrs2_diags::gen::Private_identifiers_are_only_allowed_in_class_bodies_and_may_only_be_used_as_part_of_a_class_member_declaration_property_access_or_on_the_left_hand_side_of_an_in_expression,
                    &[],
                );
                return Ok(());
            }
            let is_in_operation = self.parent_of(priv_id).is_some_and(|parent| {
                let NodeData::BinaryExpression(data) = self.data_of(parent) else {
                    return false;
                };
                data.operator_token
                    .is_some_and(|op| self.kind_of(op) == SyntaxKind::InKeyword)
            });
            if self
                .get_symbol_for_private_identifier_expression(priv_id)?
                .is_none()
                && !is_in_operation
            {
                let text = self
                    .identifier_text_of(priv_id)
                    .map(tsrs2_binder::unescape_leading_underscores)
                    .unwrap_or_default()
                    .to_owned();
                self.grammar_error_on_node(
                    priv_id,
                    &tsrs2_diags::gen::Cannot_find_name_0,
                    &[&text],
                );
            }
        }
        Ok(())
    }

    fn get_symbol_for_private_identifier_expression(
        &mut self,
        priv_id: NodeId,
    ) -> CheckResult2<Option<SymbolId>> {
        if !node_util::is_expression_node(self.binder.source_of_node(priv_id), priv_id) {
            return Ok(None);
        }
        if let crate::links::LinkSlot::Resolved(symbol) = self.links.node(priv_id).resolved_symbol {
            return Ok(Some(symbol));
        }
        let name = self
            .identifier_text_of(priv_id)
            .map(str::to_owned)
            .unwrap_or_default();
        let symbol = self.lookup_symbol_for_private_identifier_declaration(&name, priv_id)?;
        if let Some(symbol) = symbol {
            self.links
                .set_node_resolved_symbol(self.speculation_depth, priv_id, symbol);
        }
        Ok(symbol)
    }

    fn get_private_identifier_property_of_type(
        &mut self,
        left_type: TypeId,
        lexically_scoped_identifier: SymbolId,
    ) -> CheckResult2<Option<SymbolId>> {
        let name = self
            .binder
            .symbol(lexically_scoped_identifier)
            .escaped_name
            .clone();
        self.get_property_of_type_full(left_type, &name)
    }

    /// tsc-port: checkPrivateIdentifierPropertyAccess @6.0.3
    /// tsc-hash: 88d07828ed4c3f6ad804ab8aa9a8b3ba39df1680fe4d40b43b04e977f00da6ee
    /// tsc-span: _tsc.js:75139-75172
    fn check_private_identifier_property_access(
        &mut self,
        left_type: TypeId,
        right: NodeId,
        lexically_scoped_identifier: Option<SymbolId>,
    ) -> CheckResult2<bool> {
        let mut property_on_type: Option<SymbolId> = None;
        let properties = self.get_properties_of_type(left_type)?;
        let right_text = self
            .identifier_text_of(right)
            .map(str::to_owned)
            .unwrap_or_default();
        for symbol in properties {
            let Some(decl) = self.binder.symbol(symbol).value_declaration else {
                continue;
            };
            let name_matches = self.name_of_node(decl).is_some_and(|name| {
                self.kind_of(name) == SyntaxKind::PrivateIdentifier
                    && self.identifier_text_of(name) == Some(right_text.as_str())
            });
            if name_matches {
                property_on_type = Some(symbol);
                break;
            }
        }
        let diag_name = tsrs2_binder::unescape_leading_underscores(&right_text).to_owned();
        if let Some(property_on_type) = property_on_type {
            let type_value_decl = self
                .binder
                .symbol(property_on_type)
                .value_declaration
                .expect("matched by value declaration above");
            let type_class = self
                .get_containing_class_of(type_value_decl)
                .expect("private members live in classes");
            if let Some(lexical_value_decl) = lexically_scoped_identifier
                .and_then(|symbol| self.binder.symbol(symbol).value_declaration)
            {
                let lexical_class = self
                    .get_containing_class_of(lexical_value_decl)
                    .expect("private members live in classes");
                let shadowed = self
                    .find_ancestor(Some(lexical_class), |_, n| {
                        if type_class == n {
                            crate::expr::Ancestor::Yes
                        } else {
                            crate::expr::Ancestor::No
                        }
                    })
                    .is_some();
                if shadowed {
                    let left_name = self.type_to_string_slice(left_type)?;
                    let index = self.error_at_with_related(
                        Some(right),
                        &tsrs2_diags::gen::The_property_0_cannot_be_accessed_on_type_1_within_this_class_because_it_is_shadowed_by_another_private_identifier_with_the_same_spelling,
                        &[&diag_name, &left_name],
                        vec![self.related_info_for_node(
                            lexical_value_decl,
                            &tsrs2_diags::gen::The_shadowing_declaration_of_0_is_defined_here,
                            &[&diag_name],
                        )],
                    );
                    let _ = index;
                    return Ok(true);
                }
            }
            let class_name_node = self.name_of_node(type_class);
            let class_display = match class_name_node {
                Some(name) => self.entity_name_to_string(name)?,
                None => "anonymous".to_owned(),
            };
            self.error_at(
                Some(right),
                &tsrs2_diags::gen::Property_0_is_not_accessible_outside_class_1_because_it_has_a_private_identifier,
                &[&diag_name, &class_display],
            );
            return Ok(true);
        }
        Ok(false)
    }

    /// JS assignment-declared members that our binder has not
    /// materialized yet. Match the receiver's lexical symbol, not its
    /// spelling: a nested `class C` must never open the outer `C`.
    fn container_has_unbound_js_member(&self, ty: TypeId, property_name: &str) -> bool {
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return false;
        };
        let symbol = self.get_merged_symbol(symbol);
        let declarations = self.binder.symbol(symbol).declarations.clone();
        let mut value_symbols = Vec::new();
        let mut push_value_symbol = |candidate: SymbolId| {
            let candidate = self.get_merged_symbol(candidate);
            if self
                .binder
                .symbol(candidate)
                .flags
                .intersects(SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE)
                && !value_symbols.contains(&candidate)
            {
                value_symbols.push(candidate);
            }
        };
        push_value_symbol(symbol);
        // Anonymous object/function types carry a synthetic symbol;
        // the receiver binding lives on a nearby declaration node.
        for &declaration in &declarations {
            let mut cursor = Some(declaration);
            for _ in 0..4 {
                let Some(current) = cursor else { break };
                if let Some(candidate) = self.node_symbol(current) {
                    push_value_symbol(candidate);
                }
                cursor = self.parent_of(current);
            }
        }
        if value_symbols.is_empty() {
            return false;
        }
        // Class value types are anonymous constructor objects; class
        // instances are CLASS targets or REFERENCE instantiations.
        // Static assignments must not open the instance side (or vice
        // versa) merely because both types carry the class symbol.
        let instance_side = !self
            .tables
            .object_flags_of(ty)
            .intersects(tsrs2_types::ObjectFlags::ANONYMOUS);
        declarations.iter().any(|&declaration| {
            let source = self.binder.source_of_node(declaration);
            if !crate::is_js_file_name(&source.file_name) {
                return false;
            }
            source.arena.node_ids().any(|node| {
                let NodeData::BinaryExpression(binary) = self.data_of(node) else {
                    return false;
                };
                if binary.operator_token.map(|token| self.kind_of(token))
                    != Some(SyntaxKind::EqualsToken)
                {
                    return false;
                }
                let Some(left) = binary.left else {
                    return false;
                };
                let NodeData::PropertyAccessExpression(access) = self.data_of(left) else {
                    return false;
                };
                if access.name.and_then(|name| self.identifier_text_of(name)) != Some(property_name)
                {
                    return false;
                }
                let Some(receiver) = access.expression else {
                    return false;
                };
                let receiver_symbol = match self.data_of(receiver) {
                    NodeData::Identifier(data) if !instance_side => {
                        self.resolve_lexical_value_symbol(receiver, &data.escaped_text)
                    }
                    NodeData::PropertyAccessExpression(prototype) => {
                        if !instance_side
                            || prototype
                                .name
                                .and_then(|name| self.identifier_text_of(name))
                                != Some("prototype")
                        {
                            None
                        } else {
                            prototype.expression.and_then(|base| {
                                let NodeData::Identifier(data) = self.data_of(base) else {
                                    return None;
                                };
                                self.resolve_lexical_value_symbol(base, &data.escaped_text)
                            })
                        }
                    }
                    _ => None,
                };
                receiver_symbol
                    .map(|receiver| self.get_merged_symbol(receiver))
                    .is_some_and(|receiver| value_symbols.contains(&receiver))
            })
        })
    }

    /// tsc-port: isThisPropertyAccessInConstructor @6.0.3
    /// tsc-hash: 5607d52a591e4970cd8b5ff02cb5ebe04d85bcf7ab6a5b16722bb6e6c4bc27be
    /// tsc-span: _tsc.js:75192-75200
    ///
    /// isConstructorDeclaredProperty keys on JS assignment-declared
    /// properties (M5/JSDOC); the TS-reachable half is
    /// isThisProperty && isAutoTypedProperty — a property declaration
    /// with neither annotation nor initializer under noImplicitAny.
    pub(crate) fn is_this_property_access_in_constructor(
        &mut self,
        node: NodeId,
        prop: SymbolId,
    ) -> CheckResult2<bool> {
        if !self.is_this_property(node) {
            return Ok(false);
        }
        let is_auto_typed = {
            let declaration = self.binder.symbol(prop).value_declaration;
            match declaration {
                Some(declaration)
                    if self.kind_of(declaration) == SyntaxKind::PropertyDeclaration =>
                {
                    let NodeData::PropertyDeclaration(data) = self.data_of(declaration) else {
                        unreachable!("kind/data agree");
                    };
                    data.r#type.is_none()
                        && data.initializer.is_none()
                        && self
                            .options
                            .strict_option_value(self.options.no_implicit_any)
                }
                _ => false,
            }
        };
        if !is_auto_typed {
            return Ok(false);
        }
        // getThisContainer(node, true, false) === getDeclaringConstructor(prop)
        let source = self.binder.source_of_node(node);
        let this_container =
            node_util::get_this_container(source, node, /*include_arrow_functions*/ true);
        let declaring_ctor = self
            .binder
            .symbol(prop)
            .declarations
            .iter()
            .copied()
            .find_map(|declaration| {
                let container = node_util::get_this_container(
                    self.binder.source_of_node(declaration),
                    declaration,
                    /*include_arrow_functions*/ false,
                )?;
                (self.kind_of(container) == SyntaxKind::Constructor).then_some(container)
            });
        Ok(this_container.is_some() && this_container == declaring_ctor)
    }

    /// Related-info construction (createDiagnosticForNode on a possibly
    /// OTHER file — related infos carry their own file).
    pub(crate) fn related_info_for_node(
        &self,
        node: NodeId,
        message: &'static tsrs2_diags::DiagnosticMessage,
        args: &[&str],
    ) -> tsrs2_diags::RelatedInfo {
        let source = self.binder.source_of_node(node);
        let (start, end) = tsrs2_binder::node_util::get_error_span_for_node(source, node);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let (start_utf16, end_utf16) = (to_utf16(start), to_utf16(end));
        tsrs2_diags::RelatedInfo {
            file_name: Some(source.file_name.clone()),
            start: Some(start_utf16),
            length: Some(end_utf16 - start_utf16),
            message: tsrs2_diags::MessageChain::new(
                message,
                &args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>(),
            ),
        }
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: checkPropertyAccessExpressionOrQualifiedName @6.0.3
    /// tsc-hash: cd288efe571bfca00aa5dea8cea66ae89c8a6d13c180bcf483606acc60c3ece3
    /// tsc-span: _tsc.js:75201-75322
    ///
    /// Elisions/dispositions, each FN-only or unobservable:
    /// - checkExternalEmitHelpers (emit-marking artifact);
    /// - markLinkedReferences (declaration-emit bookkeeping);
    /// - deprecation suggestions (addDeprecatedSuggestion — the
    ///   suggestion band rides JSDoc @deprecated, unmodeled);
    /// - isUncheckedJSSuggestion / isPlainJsFile arms gate on JS files
    ///   (plain-JS band);
    /// - getWidenedType is the 5.6 [WIDEN] identity (extraction §6).
    fn check_property_access_expression_or_qualified_name(
        &mut self,
        node: NodeId,
        left: NodeId,
        left_type: TypeId,
        right: NodeId,
        _check_mode: CheckMode,
        write_only: bool,
    ) -> CheckResult2<TypeId> {
        let parent_symbol = match self.links.node(left).resolved_symbol {
            crate::links::LinkSlot::Resolved(symbol) => Some(symbol),
            _ => None,
        };
        let assignment_kind = self.get_assignment_target_kind(node);
        let apparent_source = if assignment_kind != crate::expr::AssignmentKind::None
            || self.is_method_access_for_call(node)
        {
            self.get_widened_type(left_type)?
        } else {
            left_type
        };
        let apparent_type = self.get_apparent_type(apparent_source)?;
        let is_any_like = self
            .tables
            .flags_of(apparent_type)
            .intersects(TypeFlags::ANY)
            || apparent_type == self.tables.intrinsics.silent_never;
        let right_is_private = self.kind_of(right) == SyntaxKind::PrivateIdentifier;
        let right_text = self
            .identifier_text_of(right)
            .map(str::to_owned)
            .unwrap_or_default();
        let prop: Option<SymbolId>;
        if right_is_private {
            // Emit-helper gates skip (languageVersion probes are
            // checkExternalEmitHelpers bookkeeping).
            let lexically_scoped_symbol =
                self.lookup_symbol_for_private_identifier_declaration(&right_text, right)?;
            if assignment_kind != crate::expr::AssignmentKind::None {
                if let Some(scoped) = lexically_scoped_symbol {
                    if self
                        .binder
                        .symbol(scoped)
                        .value_declaration
                        .is_some_and(|decl| self.kind_of(decl) == SyntaxKind::MethodDeclaration)
                    {
                        let display =
                            tsrs2_binder::unescape_leading_underscores(&right_text).to_owned();
                        self.grammar_error_on_node(
                            right,
                            &tsrs2_diags::gen::Cannot_assign_to_private_method_0_Private_methods_are_not_writable,
                            &[&display],
                        );
                    }
                }
            }
            if is_any_like {
                if lexically_scoped_symbol.is_some() {
                    return Ok(if apparent_type == self.tables.intrinsics.error {
                        self.tables.intrinsics.error
                    } else {
                        apparent_type
                    });
                }
                if self
                    .get_containing_class_excluding_class_decorators(right)
                    .is_none()
                {
                    self.grammar_error_on_node(
                        right,
                        &tsrs2_diags::gen::Private_identifiers_are_not_allowed_outside_class_bodies,
                        &[],
                    );
                    return Ok(self.tables.intrinsics.any);
                }
            }
            prop = match lexically_scoped_symbol {
                Some(scoped) => self.get_private_identifier_property_of_type(left_type, scoped)?,
                None => None,
            };
            match prop {
                None => {
                    if self.check_private_identifier_property_access(
                        left_type,
                        right,
                        lexically_scoped_symbol,
                    )? {
                        return Ok(self.tables.intrinsics.error);
                    }
                    // Plain-JS 1111 arm is isPlainJsFile-gated.
                }
                Some(prop_symbol) => {
                    let flags = self.binder.symbol(prop_symbol).flags;
                    let is_setonly_accessor = flags.intersects(SymbolFlags::SET_ACCESSOR)
                        && !flags.intersects(SymbolFlags::GET_ACCESSOR);
                    if is_setonly_accessor
                        && assignment_kind != crate::expr::AssignmentKind::Definite
                    {
                        self.error_at(
                            Some(node),
                            &tsrs2_diags::gen::Private_accessor_was_defined_without_a_getter,
                            &[],
                        );
                    }
                }
            }
        } else {
            if is_any_like {
                return Ok(if apparent_type == self.tables.intrinsics.error {
                    self.tables.intrinsics.error
                } else {
                    apparent_type
                });
            }
            let skip_object_function_property_augment =
                self.is_const_enum_object_type(apparent_type);
            if skip_object_function_property_augment {
                return Err(Unsupported::new(
                    "const-enum object property lookup (skipObjectFunctionPropertyAugment — enum band residual)",
                ));
            }
            let include_type_only_members = self.kind_of(node) == SyntaxKind::QualifiedName;
            let _ = include_type_only_members; // 5.8 modules: typeOnlyExportStarMap is empty pre-5.8.
            prop = self.get_property_of_type_full(apparent_type, &right_text)?;
        }
        let prop_type: TypeId;
        if let Some(prop) = prop {
            // Deprecation-suggestion skips (unmodeled JSDoc band).
            self.check_property_not_used_before_declaration(prop, node, right)?;
            let self_type_access = self.is_self_type_access(left, parent_symbol)?;
            self.mark_property_as_referenced(prop, Some(node), self_type_access);
            self.links
                .set_node_resolved_symbol(self.speculation_depth, node, prop);
            let writing = self.is_write_access(node);
            let is_super = self.kind_of(left) == SyntaxKind::SuperKeyword;
            self.check_property_accessibility(
                node,
                is_super,
                writing,
                apparent_type,
                prop,
                /*report_error*/ true,
            )?;
            if self.is_assignment_to_readonly_entity(node, prop, assignment_kind)? {
                let display = tsrs2_binder::unescape_leading_underscores(&right_text).to_owned();
                self.error_at(
                    Some(right),
                    &tsrs2_diags::gen::Cannot_assign_to_0_because_it_is_a_read_only_property,
                    &[&display],
                );
                return Ok(self.tables.intrinsics.error);
            }
            if self.is_this_property_access_in_constructor(node, prop)? {
                // autoType routes into getFlowTypeOfProperty — the
                // [FLOW M5] surface; containment keeps the statement
                // honest instead of faking a narrowed type.
                return Err(Unsupported::new(
                    "this-property-in-constructor autoType (getFlowTypeOfProperty [FLOW M5])",
                ));
            }
            prop_type = if write_only || self.is_write_only_access(node) {
                self.get_write_type_of_symbol(prop)?
            } else {
                self.get_type_of_symbol(prop)?
            };
        } else {
            let use_index_info = !right_is_private
                && (assignment_kind == crate::expr::AssignmentKind::None
                    || !self.tables.is_generic_object_type(left_type)
                    || self.is_this_type_parameter(left_type));
            let index_info = if use_index_info {
                self.get_applicable_index_info_for_name_info(apparent_type, &right_text)?
            } else {
                None
            };
            let Some(index_info) = index_info else {
                // isUncheckedJSSuggestion: TS files → false (JS band).
                let is_unchecked_js = false;
                if self.is_js_literal_type(left_type)? {
                    return Ok(self.tables.intrinsics.any);
                }
                let left_symbol = self.tables.type_of(left_type).symbol;
                if left_symbol == Some(self.global_this_symbol) {
                    let exported = self.globals.get(right_text.as_str()).copied();
                    if let Some(exported) = exported {
                        if self
                            .binder
                            .symbol(exported)
                            .flags
                            .intersects(SymbolFlags::BLOCK_SCOPED)
                        {
                            let display =
                                tsrs2_binder::unescape_leading_underscores(&right_text).to_owned();
                            let type_name = self.type_to_string_slice(left_type)?;
                            self.error_at(
                                Some(right),
                                &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1,
                                &[&display, &type_name],
                            );
                        }
                    } else if self
                        .options
                        .strict_option_value(self.options.no_implicit_any)
                    {
                        let type_name = self.type_to_string_slice(left_type)?;
                        self.error_at(
                            Some(right),
                            &tsrs2_diags::gen::Element_implicitly_has_an_any_type_because_type_0_has_no_index_signature,
                            &[&type_name],
                        );
                    }
                    return Ok(self.tables.intrinsics.any);
                }
                if !right_text.is_empty()
                    && !self.check_and_report_error_for_extending_interface(node)?
                {
                    // [FLOW M5] guard gate: tsc resolves the property
                    // against the receiver's FLOW type — a miss on
                    // the DECLARED type of a narrowable receiver may
                    // be tsc-clean once a RELATED, REACHING guard
                    // narrows it (conformance FP:
                    // typeGuardOfFormIsType's `isC1(c) && c.p1` —
                    // exposed when 5.7 un-escaped guard calls). The
                    // reference-targeted probe replaced the old
                    // position-only limb test (PR #6 review round 2:
                    // `if (true) { x.missing }` reports). M5 removes
                    // the gate.
                    if self.receiver_may_be_flow_narrowed(left)
                        && self.flow_guards_narrow_reference(node, left)
                    {
                        return Err(Unsupported::new(
                            "[FLOW M5] property miss on a narrowable receiver in a guarded position",
                        ));
                    }
                    // JS assignment-declared members: tsc's binder
                    // turns `C.staticProp = 0` in a .js file into a
                    // static member declaration
                    // (bindSpecialPropertyAssignment) — a miss against
                    // a JS-declared container reflects our missing
                    // expando binding, not a real miss
                    // (inferringClassStaticMembersFromAssignments,
                    // 5.8e lift FP).
                    if self.container_has_unbound_js_member(left_type, &right_text) {
                        return Err(Unsupported::new(
                            "property miss on a JS-declared container (assignment-declaration binding, M8 checkJs band)",
                        ));
                    }
                    let report_target = if self.is_this_type_parameter(left_type) {
                        apparent_type
                    } else {
                        left_type
                    };
                    self.report_nonexistent_property(right, report_target, is_unchecked_js)?;
                }
                return Ok(self.tables.intrinsics.error);
            };
            let source = self.binder.source_of_node(node);
            if index_info.is_readonly
                && (node_util::is_assignment_target(source, node) || self.is_delete_target(node))
            {
                let type_name = self.type_to_string_slice(apparent_type)?;
                self.error_at(
                    Some(node),
                    &tsrs2_diags::gen::Index_signature_in_type_0_only_permits_reading,
                    &[&type_name],
                );
            }
            let mut index_prop_type = index_info.value_type;
            if self.options.no_unchecked_indexed_access == Some(true)
                && self.get_assignment_target_kind(node) != crate::expr::AssignmentKind::Definite
            {
                let missing = self.tables.intrinsics.missing;
                index_prop_type =
                    self.get_union_type_ex(&[index_prop_type, missing], UnionReduction::Literal)?;
            }
            if self.options.no_property_access_from_index_signature == Some(true)
                && self.kind_of(node) == SyntaxKind::PropertyAccessExpression
            {
                let display = tsrs2_binder::unescape_leading_underscores(&right_text).to_owned();
                self.error_at(
                    Some(right),
                    &tsrs2_diags::gen::Property_0_comes_from_an_index_signature_so_it_must_be_accessed_with_0,
                    &[&display],
                );
            }
            // index_info.declaration deprecation suggestion elided
            // (JSDoc band).
            prop_type = index_prop_type;
        }
        self.get_flow_type_of_access_expression(node, prop, prop_type, right)
    }

    pub(crate) fn is_delete_target(&self, node: NodeId) -> bool {
        // tsc isDeleteTarget (walkUpParenthesizedExpressions parent is
        // a DeleteExpression whose expression chain reaches node).
        let mut current = node;
        while let Some(parent) = self.parent_of(current) {
            match self.kind_of(parent) {
                SyntaxKind::ParenthesizedExpression => current = parent,
                SyntaxKind::DeleteExpression => return true,
                _ => return false,
            }
        }
        false
    }

    fn is_write_access(&self, node: NodeId) -> bool {
        !matches!(self.access_kind(node), crate::expr::AccessKind::Read)
    }

    /// tsc-port: getWriteTypeOfSymbol @6.0.3
    /// tsc-hash: 2c1e85a0c9e90a6b8acad4ad180acfd12a0afc3d8cc35e3216a643a6ab8f4380
    /// tsc-span: _tsc.js:56929-56944
    pub(crate) fn get_write_type_of_symbol(&mut self, symbol: SymbolId) -> CheckResult2<TypeId> {
        let check_flags = self.get_check_flags(symbol);
        if check_flags.intersects(tsrs2_types::CheckFlags::SYNTHETIC_PROPERTY) {
            if check_flags.intersects(tsrs2_types::CheckFlags::DEFERRED_TYPE) {
                return Err(Unsupported::new(
                    "getWriteTypeOfSymbolWithDeferredType (deferred synthetic members)",
                ));
            }
            if let crate::links::LinkSlot::Resolved(write_type) =
                self.links.symbol(symbol).write_type
            {
                return Ok(write_type);
            }
            return self.get_type_of_symbol(symbol);
        }
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(SymbolFlags::PROPERTY) {
            let ty = self.get_type_of_symbol(symbol)?;
            return Ok(self.remove_missing_type(ty, flags.intersects(SymbolFlags::OPTIONAL)));
        }
        if flags.intersects(SymbolFlags::ACCESSOR) {
            if check_flags.intersects(tsrs2_types::CheckFlags::INSTANTIATED) {
                return Err(Unsupported::new(
                    "getWriteTypeOfInstantiatedSymbol (instantiated accessor writes)",
                ));
            }
            return self.get_write_type_of_accessors(symbol);
        }
        self.get_type_of_symbol(symbol)
    }

    /// tsc-port: getFlowTypeOfAccessExpression @6.0.3
    /// tsc-hash: 89b88c4b38664b752c7090e6f02f3c71f568da24ad7dfa014ba9c7fa329b9aee
    /// tsc-span: _tsc.js:75339-75371
    ///
    /// [FLOW M5]: getFlowTypeOfReference is the 5.5a stub (declared
    /// type back), so the 2565 comparison self-deactivates (the stub
    /// cannot add `undefined`) — transcribed verbatim per §6.
    fn get_flow_type_of_access_expression(
        &mut self,
        node: NodeId,
        prop: Option<SymbolId>,
        prop_type: TypeId,
        error_node: NodeId,
    ) -> CheckResult2<TypeId> {
        let assignment_kind = self.get_assignment_target_kind(node);
        if assignment_kind == crate::expr::AssignmentKind::Definite {
            let is_optional = prop.is_some_and(|prop| {
                self.binder
                    .symbol(prop)
                    .flags
                    .intersects(SymbolFlags::OPTIONAL)
            });
            return Ok(self.remove_missing_type(prop_type, is_optional));
        }
        if let Some(prop_symbol) = prop {
            let flags = self.binder.symbol(prop_symbol).flags;
            let narrowable_kind = flags
                .intersects(SymbolFlags::VARIABLE | SymbolFlags::PROPERTY | SymbolFlags::ACCESSOR);
            let union_method = flags.intersects(SymbolFlags::METHOD)
                && self.tables.flags_of(prop_type).intersects(TypeFlags::UNION);
            // isDuplicatedCommonJSExport: JS band, false in TS.
            if !narrowable_kind && !union_method {
                return Ok(prop_type);
            }
        }
        if prop_type == self.tables.intrinsics.auto {
            return Err(Unsupported::new(
                "getFlowTypeOfProperty on autoType ([FLOW M5])",
            ));
        }
        let prop_type =
            self.get_narrowable_type_for_reference(prop_type, node, CheckMode::NORMAL)?;
        let mut assume_uninitialized = false;
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let strict_property_initialization = self
            .options
            .strict_option_value(self.options.strict_property_initialization);
        let node_is_access = matches!(
            self.kind_of(node),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        );
        let receiver_is_this = match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data
                .expression
                .is_some_and(|e| self.kind_of(e) == SyntaxKind::ThisKeyword),
            NodeData::ElementAccessExpression(data) => data
                .expression
                .is_some_and(|e| self.kind_of(e) == SyntaxKind::ThisKeyword),
            _ => false,
        };
        if strict_null_checks
            && strict_property_initialization
            && node_is_access
            && receiver_is_this
        {
            let declaration = prop.and_then(|prop| self.binder.symbol(prop).value_declaration);
            if let Some(declaration) = declaration {
                let is_property_without_initializer = self.kind_of(declaration)
                    == SyntaxKind::PropertyDeclaration
                    && match self.data_of(declaration) {
                        NodeData::PropertyDeclaration(data) => data.initializer.is_none(),
                        _ => false,
                    };
                if is_property_without_initializer {
                    let is_static = tsrs2_binder::node_util::get_combined_modifier_flags(
                        self.binder.source_of_node(declaration),
                        declaration,
                    )
                    .intersects(ModifierFlags::STATIC);
                    if !is_static {
                        let flow_container = self.get_control_flow_container(node);
                        let source = self.binder.source_of_node(declaration);
                        let is_ambient = node_util::node_flags(source, declaration)
                            .intersects(NodeFlags::AMBIENT);
                        if let Some(flow_container) = flow_container {
                            if self.kind_of(flow_container) == SyntaxKind::Constructor
                                && self.parent_of(flow_container) == self.parent_of(declaration)
                                && !is_ambient
                            {
                                assume_uninitialized = true;
                            }
                        }
                    }
                }
            }
        }
        // The JS assignment-declaration else-if arm requires
        // prop.valueDeclaration to be a PropertyAccessExpression —
        // impossible in TS files (JS band).
        let initial_type = if assume_uninitialized {
            self.get_optional_type(prop_type, /*is_property*/ false)?
        } else {
            prop_type
        };
        let flow_type = self.get_flow_type_of_reference_stub(node, prop_type, initial_type, None);
        if assume_uninitialized
            && !self.contains_undefined_type(prop_type)
            && self.contains_undefined_type(flow_type)
        {
            let display = match prop {
                Some(prop) => self.symbol_display_name(prop),
                None => String::new(),
            };
            self.error_at(
                Some(error_node),
                &tsrs2_diags::gen::Property_0_is_used_before_being_assigned,
                &[&display],
            );
            return Ok(prop_type);
        }
        if assignment_kind != crate::expr::AssignmentKind::None {
            self.get_base_type_of_literal_type(flow_type)
        } else {
            Ok(flow_type)
        }
    }
}

/// tsc-port: getScriptTargetFeatures @6.0.3 (full member table)
/// tsc-hash: 4caf0dbfd5f82ff6f32731df602469bcbc345272d4a232aef289c77293d3f659
/// tsc-span: _tsc.js:13062-13646
///
/// Mechanically extracted (type, lib, member) rows in source order —
/// getSuggestedLibForNonExistentProperty scans a type's lib entries in
/// order and returns the first lib containing the member.
static SCRIPT_TARGET_FEATURE_MEMBERS: &[(&str, &str, &str)] = &[
    ("Array", "es2015", "find"),
    ("Array", "es2015", "findIndex"),
    ("Array", "es2015", "fill"),
    ("Array", "es2015", "copyWithin"),
    ("Array", "es2015", "entries"),
    ("Array", "es2015", "keys"),
    ("Array", "es2015", "values"),
    ("Array", "es2016", "includes"),
    ("Array", "es2019", "flat"),
    ("Array", "es2019", "flatMap"),
    ("Array", "es2022", "at"),
    ("Array", "es2023", "findLastIndex"),
    ("Array", "es2023", "findLast"),
    ("Array", "es2023", "toReversed"),
    ("Array", "es2023", "toSorted"),
    ("Array", "es2023", "toSpliced"),
    ("Array", "es2023", "with"),
    ("ArrayBuffer", "es2024", "maxByteLength"),
    ("ArrayBuffer", "es2024", "resizable"),
    ("ArrayBuffer", "es2024", "resize"),
    ("ArrayBuffer", "es2024", "detached"),
    ("ArrayBuffer", "es2024", "transfer"),
    ("ArrayBuffer", "es2024", "transferToFixedLength"),
    ("Atomics", "es2017", "add"),
    ("Atomics", "es2017", "and"),
    ("Atomics", "es2017", "compareExchange"),
    ("Atomics", "es2017", "exchange"),
    ("Atomics", "es2017", "isLockFree"),
    ("Atomics", "es2017", "load"),
    ("Atomics", "es2017", "or"),
    ("Atomics", "es2017", "store"),
    ("Atomics", "es2017", "sub"),
    ("Atomics", "es2017", "wait"),
    ("Atomics", "es2017", "notify"),
    ("Atomics", "es2017", "xor"),
    ("Atomics", "es2024", "waitAsync"),
    ("Atomics", "esnext", "pause"),
    ("SharedArrayBuffer", "es2017", "byteLength"),
    ("SharedArrayBuffer", "es2017", "slice"),
    ("SharedArrayBuffer", "es2024", "growable"),
    ("SharedArrayBuffer", "es2024", "maxByteLength"),
    ("SharedArrayBuffer", "es2024", "grow"),
    ("RegExp", "es2015", "flags"),
    ("RegExp", "es2015", "sticky"),
    ("RegExp", "es2015", "unicode"),
    ("RegExp", "es2018", "dotAll"),
    ("RegExp", "es2024", "unicodeSets"),
    ("RegExpConstructor", "es2025", "escape"),
    ("Reflect", "es2015", "apply"),
    ("Reflect", "es2015", "construct"),
    ("Reflect", "es2015", "defineProperty"),
    ("Reflect", "es2015", "deleteProperty"),
    ("Reflect", "es2015", "get"),
    ("Reflect", "es2015", "getOwnPropertyDescriptor"),
    ("Reflect", "es2015", "getPrototypeOf"),
    ("Reflect", "es2015", "has"),
    ("Reflect", "es2015", "isExtensible"),
    ("Reflect", "es2015", "ownKeys"),
    ("Reflect", "es2015", "preventExtensions"),
    ("Reflect", "es2015", "set"),
    ("Reflect", "es2015", "setPrototypeOf"),
    ("ArrayConstructor", "es2015", "from"),
    ("ArrayConstructor", "es2015", "of"),
    ("ArrayConstructor", "esnext", "fromAsync"),
    ("ObjectConstructor", "es2015", "assign"),
    ("ObjectConstructor", "es2015", "getOwnPropertySymbols"),
    ("ObjectConstructor", "es2015", "keys"),
    ("ObjectConstructor", "es2015", "is"),
    ("ObjectConstructor", "es2015", "setPrototypeOf"),
    ("ObjectConstructor", "es2017", "values"),
    ("ObjectConstructor", "es2017", "entries"),
    ("ObjectConstructor", "es2017", "getOwnPropertyDescriptors"),
    ("ObjectConstructor", "es2019", "fromEntries"),
    ("ObjectConstructor", "es2022", "hasOwn"),
    ("ObjectConstructor", "es2024", "groupBy"),
    ("NumberConstructor", "es2015", "isFinite"),
    ("NumberConstructor", "es2015", "isInteger"),
    ("NumberConstructor", "es2015", "isNaN"),
    ("NumberConstructor", "es2015", "isSafeInteger"),
    ("NumberConstructor", "es2015", "parseFloat"),
    ("NumberConstructor", "es2015", "parseInt"),
    ("Math", "es2015", "clz32"),
    ("Math", "es2015", "imul"),
    ("Math", "es2015", "sign"),
    ("Math", "es2015", "log10"),
    ("Math", "es2015", "log2"),
    ("Math", "es2015", "log1p"),
    ("Math", "es2015", "expm1"),
    ("Math", "es2015", "cosh"),
    ("Math", "es2015", "sinh"),
    ("Math", "es2015", "tanh"),
    ("Math", "es2015", "acosh"),
    ("Math", "es2015", "asinh"),
    ("Math", "es2015", "atanh"),
    ("Math", "es2015", "hypot"),
    ("Math", "es2015", "trunc"),
    ("Math", "es2015", "fround"),
    ("Math", "es2015", "cbrt"),
    ("Math", "es2025", "f16round"),
    ("Map", "es2015", "entries"),
    ("Map", "es2015", "keys"),
    ("Map", "es2015", "values"),
    ("Map", "esnext", "getOrInsert"),
    ("Map", "esnext", "getOrInsertComputed"),
    ("MapConstructor", "es2024", "groupBy"),
    ("Set", "es2015", "entries"),
    ("Set", "es2015", "keys"),
    ("Set", "es2015", "values"),
    ("Set", "es2025", "union"),
    ("Set", "es2025", "intersection"),
    ("Set", "es2025", "difference"),
    ("Set", "es2025", "symmetricDifference"),
    ("Set", "es2025", "isSubsetOf"),
    ("Set", "es2025", "isSupersetOf"),
    ("Set", "es2025", "isDisjointFrom"),
    ("PromiseConstructor", "es2015", "all"),
    ("PromiseConstructor", "es2015", "race"),
    ("PromiseConstructor", "es2015", "reject"),
    ("PromiseConstructor", "es2015", "resolve"),
    ("PromiseConstructor", "es2020", "allSettled"),
    ("PromiseConstructor", "es2021", "any"),
    ("PromiseConstructor", "es2024", "withResolvers"),
    ("PromiseConstructor", "es2025", "try"),
    ("Symbol", "es2015", "for"),
    ("Symbol", "es2015", "keyFor"),
    ("Symbol", "es2019", "description"),
    ("WeakMap", "es2015", "entries"),
    ("WeakMap", "es2015", "keys"),
    ("WeakMap", "es2015", "values"),
    ("WeakMap", "esnext", "getOrInsert"),
    ("WeakMap", "esnext", "getOrInsertComputed"),
    ("WeakSet", "es2015", "entries"),
    ("WeakSet", "es2015", "keys"),
    ("WeakSet", "es2015", "values"),
    ("String", "es2015", "codePointAt"),
    ("String", "es2015", "includes"),
    ("String", "es2015", "endsWith"),
    ("String", "es2015", "normalize"),
    ("String", "es2015", "repeat"),
    ("String", "es2015", "startsWith"),
    ("String", "es2015", "anchor"),
    ("String", "es2015", "big"),
    ("String", "es2015", "blink"),
    ("String", "es2015", "bold"),
    ("String", "es2015", "fixed"),
    ("String", "es2015", "fontcolor"),
    ("String", "es2015", "fontsize"),
    ("String", "es2015", "italics"),
    ("String", "es2015", "link"),
    ("String", "es2015", "small"),
    ("String", "es2015", "strike"),
    ("String", "es2015", "sub"),
    ("String", "es2015", "sup"),
    ("String", "es2017", "padStart"),
    ("String", "es2017", "padEnd"),
    ("String", "es2019", "trimStart"),
    ("String", "es2019", "trimEnd"),
    ("String", "es2019", "trimLeft"),
    ("String", "es2019", "trimRight"),
    ("String", "es2020", "matchAll"),
    ("String", "es2021", "replaceAll"),
    ("String", "es2022", "at"),
    ("String", "es2024", "isWellFormed"),
    ("String", "es2024", "toWellFormed"),
    ("StringConstructor", "es2015", "fromCodePoint"),
    ("StringConstructor", "es2015", "raw"),
    ("DateTimeFormat", "es2017", "formatToParts"),
    ("Promise", "es2018", "finally"),
    ("RegExpMatchArray", "es2018", "groups"),
    ("RegExpExecArray", "es2018", "groups"),
    ("Intl", "es2018", "PluralRules"),
    ("Intl", "es2020", "RelativeTimeFormat"),
    ("Intl", "es2020", "Locale"),
    ("Intl", "es2020", "DisplayNames"),
    ("Intl", "es2021", "ListFormat"),
    ("Intl", "es2021", "DateTimeFormat"),
    ("Intl", "es2022", "Segmenter"),
    ("Intl", "es2025", "DurationFormat"),
    ("NumberFormat", "es2018", "formatToParts"),
    ("SymbolConstructor", "es2020", "matchAll"),
    ("SymbolConstructor", "esnext", "metadata"),
    ("SymbolConstructor", "esnext", "dispose"),
    ("SymbolConstructor", "esnext", "asyncDispose"),
    ("DataView", "es2020", "setBigInt64"),
    ("DataView", "es2020", "setBigUint64"),
    ("DataView", "es2020", "getBigInt64"),
    ("DataView", "es2020", "getBigUint64"),
    ("DataView", "es2025", "setFloat16"),
    ("DataView", "es2025", "getFloat16"),
    ("RelativeTimeFormat", "es2020", "format"),
    ("RelativeTimeFormat", "es2020", "formatToParts"),
    ("RelativeTimeFormat", "es2020", "resolvedOptions"),
    ("Int8Array", "es2022", "at"),
    ("Int8Array", "es2023", "findLastIndex"),
    ("Int8Array", "es2023", "findLast"),
    ("Int8Array", "es2023", "toReversed"),
    ("Int8Array", "es2023", "toSorted"),
    ("Int8Array", "es2023", "toSpliced"),
    ("Int8Array", "es2023", "with"),
    ("Uint8Array", "es2022", "at"),
    ("Uint8Array", "es2023", "findLastIndex"),
    ("Uint8Array", "es2023", "findLast"),
    ("Uint8Array", "es2023", "toReversed"),
    ("Uint8Array", "es2023", "toSorted"),
    ("Uint8Array", "es2023", "toSpliced"),
    ("Uint8Array", "es2023", "with"),
    ("Uint8Array", "esnext", "toBase64"),
    ("Uint8Array", "esnext", "setFromBase64"),
    ("Uint8Array", "esnext", "toHex"),
    ("Uint8Array", "esnext", "setFromHex"),
    ("Uint8ClampedArray", "es2022", "at"),
    ("Uint8ClampedArray", "es2023", "findLastIndex"),
    ("Uint8ClampedArray", "es2023", "findLast"),
    ("Uint8ClampedArray", "es2023", "toReversed"),
    ("Uint8ClampedArray", "es2023", "toSorted"),
    ("Uint8ClampedArray", "es2023", "toSpliced"),
    ("Uint8ClampedArray", "es2023", "with"),
    ("Int16Array", "es2022", "at"),
    ("Int16Array", "es2023", "findLastIndex"),
    ("Int16Array", "es2023", "findLast"),
    ("Int16Array", "es2023", "toReversed"),
    ("Int16Array", "es2023", "toSorted"),
    ("Int16Array", "es2023", "toSpliced"),
    ("Int16Array", "es2023", "with"),
    ("Uint16Array", "es2022", "at"),
    ("Uint16Array", "es2023", "findLastIndex"),
    ("Uint16Array", "es2023", "findLast"),
    ("Uint16Array", "es2023", "toReversed"),
    ("Uint16Array", "es2023", "toSorted"),
    ("Uint16Array", "es2023", "toSpliced"),
    ("Uint16Array", "es2023", "with"),
    ("Int32Array", "es2022", "at"),
    ("Int32Array", "es2023", "findLastIndex"),
    ("Int32Array", "es2023", "findLast"),
    ("Int32Array", "es2023", "toReversed"),
    ("Int32Array", "es2023", "toSorted"),
    ("Int32Array", "es2023", "toSpliced"),
    ("Int32Array", "es2023", "with"),
    ("Uint32Array", "es2022", "at"),
    ("Uint32Array", "es2023", "findLastIndex"),
    ("Uint32Array", "es2023", "findLast"),
    ("Uint32Array", "es2023", "toReversed"),
    ("Uint32Array", "es2023", "toSorted"),
    ("Uint32Array", "es2023", "toSpliced"),
    ("Uint32Array", "es2023", "with"),
    ("Float32Array", "es2022", "at"),
    ("Float32Array", "es2023", "findLastIndex"),
    ("Float32Array", "es2023", "findLast"),
    ("Float32Array", "es2023", "toReversed"),
    ("Float32Array", "es2023", "toSorted"),
    ("Float32Array", "es2023", "toSpliced"),
    ("Float32Array", "es2023", "with"),
    ("Float64Array", "es2022", "at"),
    ("Float64Array", "es2023", "findLastIndex"),
    ("Float64Array", "es2023", "findLast"),
    ("Float64Array", "es2023", "toReversed"),
    ("Float64Array", "es2023", "toSorted"),
    ("Float64Array", "es2023", "toSpliced"),
    ("Float64Array", "es2023", "with"),
    ("BigInt64Array", "es2022", "at"),
    ("BigInt64Array", "es2023", "findLastIndex"),
    ("BigInt64Array", "es2023", "findLast"),
    ("BigInt64Array", "es2023", "toReversed"),
    ("BigInt64Array", "es2023", "toSorted"),
    ("BigInt64Array", "es2023", "toSpliced"),
    ("BigInt64Array", "es2023", "with"),
    ("BigUint64Array", "es2022", "at"),
    ("BigUint64Array", "es2023", "findLastIndex"),
    ("BigUint64Array", "es2023", "findLast"),
    ("BigUint64Array", "es2023", "toReversed"),
    ("BigUint64Array", "es2023", "toSorted"),
    ("BigUint64Array", "es2023", "toSpliced"),
    ("BigUint64Array", "es2023", "with"),
    ("Error", "es2022", "cause"),
    ("ErrorConstructor", "esnext", "isError"),
    ("Uint8ArrayConstructor", "esnext", "fromBase64"),
    ("Uint8ArrayConstructor", "esnext", "fromHex"),
    ("Date", "esnext", "toTemporalInstant"),
];

impl<'a> CheckerState<'a> {
    /// tsc-port: markPropertyAsReferenced @6.0.3
    /// tsc-hash: 481c183fe00ed2e2b477f0a529928f3ae93bf03baa43c2685af7d970b7f36a95
    /// tsc-span: _tsc.js:75598-75621
    ///
    /// The isReferenced write is M7 unused-checks bookkeeping — inert
    /// until then, ported for the symbol-state parity.
    pub(crate) fn mark_property_as_referenced(
        &mut self,
        prop: SymbolId,
        node_for_check_write_only: Option<NodeId>,
        is_self_type_access: bool,
    ) {
        let flags = self.binder.symbol(prop).flags;
        if !flags.intersects(SymbolFlags::CLASS_MEMBER) {
            return;
        }
        let Some(value_declaration) = self.binder.symbol(prop).value_declaration else {
            return;
        };
        let has_private_modifier = tsrs2_binder::node_util::get_combined_modifier_flags(
            self.binder.source_of_node(value_declaration),
            value_declaration,
        )
        .intersects(ModifierFlags::PRIVATE);
        let has_private_identifier = self
            .name_of_node(value_declaration)
            .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier);
        if !has_private_modifier && !has_private_identifier {
            return;
        }
        if let Some(node) = node_for_check_write_only {
            if self.is_write_only_access(node) && !flags.intersects(SymbolFlags::SET_ACCESSOR) {
                return;
            }
        }
        if is_self_type_access {
            let containing_method = self.find_ancestor(node_for_check_write_only, |state, n| {
                if tsrs2_binder::node_util::is_function_like_declaration_kind(state.kind_of(n)) {
                    crate::expr::Ancestor::Yes
                } else {
                    crate::expr::Ancestor::No
                }
            });
            if let Some(method) = containing_method {
                if self.binder.node_symbol(method) == Some(prop) {
                    return;
                }
            }
        }
        let target = if self
            .get_check_flags(prop)
            .intersects(tsrs2_types::CheckFlags::INSTANTIATED)
        {
            self.links
                .symbol(prop)
                .target
                .expect("Instantiated check flag implies links.target")
        } else {
            prop
        };
        self.links
            .set_symbol_is_referenced(self.speculation_depth, target);
    }

    pub(crate) fn is_self_type_access(
        &mut self,
        name: NodeId,
        parent: Option<SymbolId>,
    ) -> CheckResult2<bool> {
        if self.kind_of(name) == SyntaxKind::ThisKeyword {
            return Ok(true);
        }
        let Some(parent) = parent else {
            return Ok(false);
        };
        if !self.is_entity_name_expression(name) {
            return Ok(false);
        }
        // getResolvedSymbol(getFirstIdentifier(name)).
        let mut first = name;
        while let NodeData::PropertyAccessExpression(data) = self.data_of(first) {
            let Some(expression) = data.expression else {
                return Ok(false);
            };
            first = expression;
        }
        if self.kind_of(first) != SyntaxKind::Identifier {
            return Ok(false);
        }
        Ok(self.get_resolved_symbol(first)? == Some(parent))
    }

    /// tsc-port: checkPropertyNotUsedBeforeDeclaration @6.0.3
    /// tsc-hash: fca1f7be706dc5144716725dbeb84a5085a8b48aba853272e3ccd86ea3ef6e46
    /// tsc-span: _tsc.js:75372-75415
    fn check_property_not_used_before_declaration(
        &mut self,
        prop: SymbolId,
        node: NodeId,
        right: NodeId,
    ) -> CheckResult2<()> {
        let Some(value_declaration) = self.binder.symbol(prop).value_declaration else {
            return Ok(());
        };
        let source = self.binder.source_of_node(node);
        if source.is_declaration_file {
            return Ok(());
        }
        let declaration_name = self
            .identifier_text_of(right)
            .map(tsrs2_binder::unescape_leading_underscores)
            .unwrap_or_default()
            .to_owned();
        let mut message: Option<&'static tsrs2_diags::DiagnosticMessage> = None;
        let is_optional_property = self.kind_of(value_declaration)
            == SyntaxKind::PropertyDeclaration
            && match self.data_of(value_declaration) {
                NodeData::PropertyDeclaration(data) => data.question_token.is_some(),
                _ => false,
            };
        let node_is_nested_access = matches!(
            self.kind_of(node),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) && match self.data_of(node) {
            NodeData::PropertyAccessExpression(data) => data.expression.is_some_and(|e| {
                matches!(
                    self.kind_of(e),
                    SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                )
            }),
            NodeData::ElementAccessExpression(data) => data.expression.is_some_and(|e| {
                matches!(
                    self.kind_of(e),
                    SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
                )
            }),
            _ => false,
        };
        let static_method_exemption = self.kind_of(value_declaration)
            == SyntaxKind::MethodDeclaration
            && tsrs2_binder::node_util::get_combined_modifier_flags(
                self.binder.source_of_node(value_declaration),
                value_declaration,
            )
            .intersects(ModifierFlags::STATIC);
        if self.is_in_property_initializer_or_class_static_block(node, false)
            && !is_optional_property
            && !node_is_nested_access
            && !self.is_block_scoped_name_declared_before_use(value_declaration, right)?
            && !static_method_exemption
            && (self.options.use_define_for_class_fields_effective()
                || !self.is_property_declared_in_ancestor_class(prop)?)
        {
            message = Some(&tsrs2_diags::gen::Property_0_is_used_before_its_initialization);
        } else if self.kind_of(value_declaration) == SyntaxKind::ClassDeclaration
            && self
                .parent_of(node)
                .is_some_and(|parent| self.kind_of(parent) != SyntaxKind::TypeReference)
            && !node_util::node_flags(
                self.binder.source_of_node(value_declaration),
                value_declaration,
            )
            .intersects(NodeFlags::AMBIENT)
            && !self.is_block_scoped_name_declared_before_use(value_declaration, right)?
        {
            message = Some(&tsrs2_diags::gen::Class_0_used_before_its_declaration);
        }
        if let Some(message) = message {
            let related = self.related_info_for_node(
                value_declaration,
                &tsrs2_diags::gen::_0_is_declared_here,
                &[&declaration_name],
            );
            self.error_at_with_related(Some(right), message, &[&declaration_name], vec![related]);
        }
        Ok(())
    }

    fn is_property_declared_in_ancestor_class(&mut self, prop: SymbolId) -> CheckResult2<bool> {
        let Some(parent) = self.binder.symbol(prop).parent else {
            return Ok(false);
        };
        if !self
            .binder
            .symbol(parent)
            .flags
            .intersects(SymbolFlags::CLASS)
        {
            return Ok(false);
        }
        let declared = self.get_declared_type_of_class_or_interface(parent)?;
        let base_types = self.get_base_types(declared)?;
        let Some(&first_base) = base_types.first() else {
            return Ok(false);
        };
        let name = self.binder.symbol(prop).escaped_name.clone();
        let super_property = self.get_property_of_type_full(first_base, &name)?;
        Ok(super_property
            .is_some_and(|symbol| self.binder.symbol(symbol).value_declaration.is_some()))
    }

    /// tsc-port: isAssignmentToReadonlyEntity @6.0.3
    /// tsc-hash: f196b7ce743620dc575f7d80b5bc9b15d8ea46595076bb03262ec224dde50d18
    /// tsc-span: _tsc.js:79256-79289
    ///
    /// The alias tail (NamespaceImport receiver) needs alias
    /// resolution (5.8) — a named escape keeps the statement honest.
    pub(crate) fn is_assignment_to_readonly_entity(
        &mut self,
        expr: NodeId,
        symbol: SymbolId,
        assignment_kind: crate::expr::AssignmentKind,
    ) -> CheckResult2<bool> {
        if assignment_kind == crate::expr::AssignmentKind::None {
            return Ok(false);
        }
        if self.is_readonly_symbol(symbol) {
            if self
                .binder
                .symbol(symbol)
                .flags
                .intersects(SymbolFlags::PROPERTY)
                && self.is_this_property(expr)
            {
                let ctor = self.get_control_flow_container(expr);
                let Some(ctor) = ctor.filter(|&c| self.kind_of(c) == SyntaxKind::Constructor)
                else {
                    return Ok(true);
                };
                if let Some(value_declaration) = self.binder.symbol(symbol).value_declaration {
                    let is_assignment_declaration =
                        self.kind_of(value_declaration) == SyntaxKind::BinaryExpression;
                    let is_local_property_declaration =
                        self.parent_of(ctor) == self.parent_of(value_declaration);
                    let is_local_parameter_property =
                        Some(ctor) == self.parent_of(value_declaration);
                    // The assignment-declaration flavors are JS-band
                    // (isBinaryExpression valueDeclaration) — dead in
                    // TS, transcribed for shape.
                    let is_writeable = is_local_property_declaration
                        || is_local_parameter_property
                        || is_assignment_declaration;
                    return Ok(!is_writeable);
                }
            }
            return Ok(true);
        }
        if matches!(
            self.kind_of(expr),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) {
            let receiver = match self.data_of(expr) {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                _ => None,
            };
            if let Some(mut node) = receiver {
                while self.kind_of(node) == SyntaxKind::ParenthesizedExpression {
                    let NodeData::ParenthesizedExpression(data) = self.data_of(node) else {
                        break;
                    };
                    let Some(inner) = data.expression else { break };
                    node = inner;
                }
                if self.kind_of(node) == SyntaxKind::Identifier {
                    if let crate::links::LinkSlot::Resolved(receiver_symbol) =
                        self.links.node(node).resolved_symbol
                    {
                        if self
                            .binder
                            .symbol(receiver_symbol)
                            .flags
                            .intersects(SymbolFlags::ALIAS)
                        {
                            return Err(Unsupported::new(
                                "readonly-entity alias receiver (getDeclarationOfAliasSymbol, 5.8)",
                            ));
                        }
                    }
                }
            }
        }
        Ok(false)
    }

    fn is_this_type_parameter(&self, ty: TypeId) -> bool {
        self.tables
            .flags_of(ty)
            .intersects(TypeFlags::TYPE_PARAMETER)
            && matches!(
                &self.tables.type_of(ty).data,
                tsrs2_types::TypeData::TypeParameter {
                    is_this_type: true,
                    ..
                }
            )
    }

    /// tsc-port: checkAndReportErrorForExtendingInterface @6.0.3
    /// tsc-hash: cfe6ae0a56109c62323d85e8a5b022405d71bb9999323d65b4c14ab4546b91d9
    /// tsc-span: _tsc.js:48255-48281
    pub(crate) fn check_and_report_error_for_extending_interface(
        &mut self,
        error_location: NodeId,
    ) -> CheckResult2<bool> {
        let Some(expression) = self.get_entity_name_for_extending_interface(error_location) else {
            return Ok(false);
        };
        if self
            .resolve_entity_name(expression, SymbolFlags::INTERFACE, true, None)?
            .is_some()
        {
            let text = self.entity_name_to_string(expression)?;
            self.error_at(
                Some(error_location),
                &tsrs2_diags::gen::Cannot_extend_an_interface_0_Did_you_mean_implements,
                &[&text],
            );
            return Ok(true);
        }
        Ok(false)
    }

    fn get_entity_name_for_extending_interface(&self, node: NodeId) -> Option<NodeId> {
        match self.kind_of(node) {
            SyntaxKind::Identifier | SyntaxKind::PropertyAccessExpression => self
                .parent_of(node)
                .and_then(|parent| self.get_entity_name_for_extending_interface(parent)),
            SyntaxKind::ExpressionWithTypeArguments => {
                let NodeData::ExpressionWithTypeArguments(data) = self.data_of(node) else {
                    return None;
                };
                let expression = data.expression?;
                self.is_entity_name_expression(expression)
                    .then_some(expression)
            }
            _ => None,
        }
    }

    // ---- reportNonexistentProperty + the suggestion helpers ----

    /// tsc-port: reportNonexistentProperty @6.0.3
    /// tsc-hash: 307d53095640744e1815787c33792c951a7a0c7fc169193870ec78298aca5f7c
    /// tsc-span: _tsc.js:75416-75470
    ///
    /// The unchecked-JS suggestion flavor (2568) is JS-band; TS always
    /// reports as an error. Element-side callers do NOT come through
    /// here (their 2551 has no related 2728 — oracle-pinned).
    pub(crate) fn report_nonexistent_property(
        &mut self,
        prop_node: NodeId,
        containing_type: TypeId,
        is_unchecked_js: bool,
    ) -> CheckResult2<()> {
        let cache_key = format!("{}|{}", containing_type.0, is_unchecked_js);
        if !self.links.insert_node_non_existent_prop_key(
            self.speculation_depth,
            prop_node,
            cache_key,
        ) {
            return Ok(());
        }
        let prop_name_raw = self
            .identifier_text_of(prop_node)
            .map(str::to_owned)
            .unwrap_or_default();
        let missing_property =
            tsrs2_binder::unescape_leading_underscores(&prop_name_raw).to_owned();
        let mut chain_tail: Vec<tsrs2_diags::MessageChain> = Vec::new();
        let flags = self.tables.flags_of(containing_type);
        if self.kind_of(prop_node) != SyntaxKind::PrivateIdentifier
            && flags.intersects(TypeFlags::UNION)
            && !flags.intersects(TypeFlags::PRIMITIVE)
        {
            let constituents: Vec<TypeId> = match &self.tables.type_of(containing_type).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            for subtype in constituents {
                let has_prop = self
                    .get_property_of_type_full(subtype, &prop_name_raw)?
                    .is_some();
                let has_index = has_prop
                    || self
                        .get_applicable_index_info_for_name_info(subtype, &prop_name_raw)?
                        .is_some();
                if !has_index {
                    let subtype_name = self.type_to_string_slice(subtype)?;
                    chain_tail.push(tsrs2_diags::MessageChain::new(
                        &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1,
                        &[missing_property.clone(), subtype_name],
                    ));
                    break;
                }
            }
        }
        let mut related: Option<tsrs2_diags::RelatedInfo> = None;
        let head: tsrs2_diags::MessageChain;
        if self.type_has_static_property(&prop_name_raw, containing_type)? {
            let type_name = self.type_to_string_slice(containing_type)?;
            let suggestion = format!("{type_name}.{missing_property}");
            head = tsrs2_diags::MessageChain::new(
                &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1_Did_you_mean_to_access_the_static_member_2_instead,
                &[missing_property.clone(), type_name, suggestion],
            );
        } else {
            let promised = self.get_promised_type_of_promise(containing_type)?;
            let promised_has_prop = match promised {
                Some(promised) => self
                    .get_property_of_type_full(promised, &prop_name_raw)?
                    .is_some(),
                None => false,
            };
            if promised_has_prop {
                let type_name = self.type_to_string_slice(containing_type)?;
                head = tsrs2_diags::MessageChain::new(
                    &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1,
                    &[missing_property.clone(), type_name],
                );
                related = Some(self.related_info_for_node(
                    prop_node,
                    &tsrs2_diags::gen::Did_you_forget_to_use_await,
                    &[],
                ));
            } else {
                let container = if self.is_empty_anonymous_type_literal(containing_type)? {
                    "{}".to_owned()
                } else {
                    self.type_to_string_slice(containing_type)?
                };
                let lib_suggestion = self.get_suggested_lib_for_non_existent_property(
                    &missing_property,
                    containing_type,
                )?;
                if let Some(lib) = lib_suggestion {
                    head = tsrs2_diags::MessageChain::new(
                        &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1_Do_you_need_to_change_your_target_library_Try_changing_the_lib_compiler_option_to_2_or_later,
                        &[missing_property.clone(), container, lib.to_owned()],
                    );
                } else {
                    let suggestion = self.get_suggested_symbol_for_nonexistent_property(
                        Some(prop_node),
                        &prop_name_raw,
                        containing_type,
                    )?;
                    if let Some(suggestion) = suggestion {
                        let suggested_name = tsrs2_binder::unescape_leading_underscores(
                            &self.binder.symbol(suggestion).escaped_name,
                        )
                        .to_owned();
                        head = tsrs2_diags::MessageChain::new(
                            &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1_Did_you_mean_2,
                            &[missing_property.clone(), container, suggested_name.clone()],
                        );
                        if let Some(value_declaration) =
                            self.binder.symbol(suggestion).value_declaration
                        {
                            related = Some(self.related_info_for_node(
                                value_declaration,
                                &tsrs2_diags::gen::_0_is_declared_here,
                                &[&suggested_name],
                            ));
                        }
                    } else if self.container_seems_to_be_empty_dom_element(containing_type)? {
                        head = tsrs2_diags::MessageChain::new(
                            &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1_Try_changing_the_lib_compiler_option_to_include_dom,
                            &[missing_property.clone(), container],
                        );
                    } else {
                        // chainDiagnosticMessages NESTS: the never-
                        // intersection row wraps the union-constituent
                        // row, and the 2339 head wraps that.
                        if let Some(mut elaborated) =
                            self.elaborate_never_intersection_row(containing_type)?
                        {
                            elaborated.next = std::mem::take(&mut chain_tail);
                            chain_tail = vec![elaborated];
                        }
                        head = tsrs2_diags::MessageChain::new(
                            &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1,
                            &[missing_property.clone(), container],
                        );
                    }
                }
            }
        }
        let mut diagnostic = self.diagnostic_for_node(
            prop_node,
            &tsrs2_diags::gen::Property_0_does_not_exist_on_type_1,
            &[],
        );
        diagnostic.message = head.with_next(chain_tail);
        if let Some(related) = related {
            diagnostic.related.push(related);
        }
        self.push_error_diagnostic(diagnostic);
        Ok(())
    }

    /// The one anonymous nodeBuilder spelling needed by the
    /// missing-property head. Keeping this local avoids making `{}` a
    /// generic relation-error display for unrelated synthesized empty
    /// object/array fallbacks.
    fn is_empty_anonymous_type_literal(&mut self, ty: TypeId) -> CheckResult2<bool> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::OBJECT)
            || !self
                .tables
                .object_flags_of(ty)
                .intersects(tsrs2_types::ObjectFlags::ANONYMOUS)
        {
            return Ok(false);
        }
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return Ok(false);
        };
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::TYPE_LITERAL)
        {
            return Ok(false);
        }
        let resolved = self.resolve_structured_type_members(ty)?;
        let members = self.members_of(resolved);
        Ok(members.properties.is_empty()
            && members.call_signatures.is_empty()
            && members.construct_signatures.is_empty()
            && members.index_infos.is_empty())
    }

    /// tsc-port: typeHasStaticProperty @6.0.3
    /// tsc-hash: 8e00c6c781a5ab8b88e17d268b78a78c33c5f1eaf1f73b428c996fd17def8621
    /// tsc-span: _tsc.js:75471-75500
    pub(crate) fn type_has_static_property(
        &mut self,
        prop_name: &str,
        containing_type: TypeId,
    ) -> CheckResult2<bool> {
        let Some(symbol) = self.tables.type_of(containing_type).symbol else {
            return Ok(false);
        };
        let symbol_type = self.get_type_of_symbol(symbol)?;
        let Some(prop) = self.get_property_of_type_full(symbol_type, prop_name)? else {
            return Ok(false);
        };
        let Some(value_declaration) = self.binder.symbol(prop).value_declaration else {
            return Ok(false);
        };
        Ok(tsrs2_binder::node_util::get_combined_modifier_flags(
            self.binder.source_of_node(value_declaration),
            value_declaration,
        )
        .intersects(ModifierFlags::STATIC))
    }

    fn get_suggested_lib_for_non_existent_property(
        &mut self,
        missing_property: &str,
        containing_type: TypeId,
    ) -> CheckResult2<Option<&'static str>> {
        let apparent = self.get_apparent_type(containing_type)?;
        let Some(container) = self.tables.type_of(apparent).symbol else {
            return Ok(None);
        };
        let container_name = self.binder.symbol(container).escaped_name.clone();
        let container_name = tsrs2_binder::unescape_leading_underscores(&container_name);
        Ok(SCRIPT_TARGET_FEATURE_MEMBERS
            .iter()
            .find(|(type_name, _, member)| {
                *type_name == container_name && *member == missing_property
            })
            .map(|(_, lib, _)| *lib))
    }

    fn container_seems_to_be_empty_dom_element(
        &mut self,
        containing_type: TypeId,
    ) -> CheckResult2<bool> {
        let Some(lib) = &self.options.lib else {
            return Ok(false);
        };
        if lib.iter().any(|entry| entry == "dom") {
            return Ok(false);
        }
        // everyContainedType(t => symbol name matches the DOM shape).
        let constituents: Vec<TypeId> = if self
            .tables
            .flags_of(containing_type)
            .intersects(TypeFlags::UNION)
        {
            match &self.tables.type_of(containing_type).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            }
        } else {
            vec![containing_type]
        };
        let dom_shape = |name: &str| {
            name == "EventTarget"
                || name == "Node"
                || name == "Element"
                || (name.starts_with("HTML") && name.ends_with("Element"))
        };
        for constituent in &constituents {
            let Some(symbol) = self.tables.type_of(*constituent).symbol else {
                return Ok(false);
            };
            let name = self.binder.symbol(symbol).escaped_name.clone();
            if !dom_shape(tsrs2_binder::unescape_leading_underscores(&name)) {
                return Ok(false);
            }
        }
        self.is_empty_object_type(containing_type)
    }

    /// tsc-port: getPromisedTypeOfPromise @6.0.3 (the reportNonexistent
    /// consumer slice — errorNode/thisTypeForErrorOut stay 5.5f)
    /// tsc-hash: 34400f2efd43255c842416a05dbe9f0b0e3f9f09d13162db7e61e10a0f59541f
    /// tsc-span: _tsc.js:82316-82376
    pub(crate) fn get_promised_type_of_promise(
        &mut self,
        ty: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        Ok(self
            .get_promised_type_of_promise_with_this_error(ty, None)?
            .0)
    }

    /// The full face: (promised, thisTypeForErrorOut). Error rows
    /// (1059 no-then / 2684 this-context / 1060 non-callable) fire
    /// only with an errorNode — the 5.5f awaited family passes None
    /// and consumes thisTypeForErrorOut; errorNode callers arrive at
    /// 5.8 (checkAsyncFunctionReturnType, for-await).
    pub(crate) fn get_promised_type_of_promise_with_this_error(
        &mut self,
        ty: TypeId,
        error_node: Option<NodeId>,
    ) -> CheckResult2<(Option<TypeId>, Option<TypeId>)> {
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok((None, None));
        }
        if let Some(cached) = self.links.ty(ty).promised_type_of_promise {
            return Ok((Some(cached), None));
        }
        let global_promise = self.get_global_type_or_undefined("Promise", 1)?;
        if let Some(global_promise) = global_promise {
            let is_reference = self
                .tables
                .object_flags_of(ty)
                .intersects(tsrs2_types::ObjectFlags::REFERENCE)
                && self.tables.reference_target(ty) == global_promise;
            if is_reference {
                let arguments = self.get_type_arguments(ty)?;
                if let Some(&first) = arguments.first() {
                    self.links
                        .set_type_promised_type_of_promise(self.speculation_depth, ty, first);
                    return Ok((Some(first), None));
                }
            }
        }
        let base_or_type = self.get_base_constraint_or_type(ty)?;
        if self
            .all_types_assignable_to_kind(base_or_type, TypeFlags::PRIMITIVE | TypeFlags::NEVER)?
        {
            return Ok((None, None));
        }
        let then_function = self.get_type_of_property_of_type(ty, "then")?;
        if then_function.is_some_and(|then_function| {
            self.tables
                .flags_of(then_function)
                .intersects(TypeFlags::ANY)
        }) {
            return Ok((None, None));
        }
        let then_signatures = match then_function {
            Some(then_function) => {
                self.get_signatures_of_type(then_function, crate::structural::SignatureKind::Call)?
            }
            None => Vec::new(),
        };
        if then_signatures.is_empty() {
            if let Some(error_node) = error_node {
                self.error_at(
                    Some(error_node),
                    &tsrs2_diags::gen::A_promise_must_have_a_then_method,
                    &[],
                );
            }
            return Ok((None, None));
        }
        let mut this_type_for_error: Option<TypeId> = None;
        let mut candidates = Vec::new();
        for &then_signature in &then_signatures {
            let this_type = self.get_this_type_of_signature(then_signature)?;
            match this_type {
                Some(this_type)
                    if this_type != self.tables.intrinsics.void
                        && !self.is_type_subtype_of(ty, this_type)? =>
                {
                    this_type_for_error = Some(this_type);
                }
                _ => candidates.push(then_signature),
            }
        }
        if candidates.is_empty() {
            let this_type =
                this_type_for_error.expect("no candidates implies a this-type rejection");
            if let Some(error_node) = error_node {
                let type_text = self.type_to_string_slice(ty)?;
                let this_text = self.type_to_string_slice(this_type)?;
                self.error_at(
                    Some(error_node),
                    &tsrs2_diags::gen::The_this_context_of_type_0_is_not_assignable_to_method_s_this_of_type_1,
                    &[&type_text, &this_text],
                );
            }
            return Ok((None, Some(this_type)));
        }
        let mut first_param_types = Vec::with_capacity(candidates.len());
        for &candidate in &candidates {
            first_param_types.push(self.get_type_of_first_parameter_of_signature(candidate)?);
        }
        let union = self.get_union_type_ex(&first_param_types, UnionReduction::Literal)?;
        let onfulfilled = self.get_type_with_facts(union, TypeFacts::NE_UNDEFINED_OR_NULL)?;
        if self.tables.flags_of(onfulfilled).intersects(TypeFlags::ANY) {
            return Ok((None, None));
        }
        let onfulfilled_signatures =
            self.get_signatures_of_type(onfulfilled, crate::structural::SignatureKind::Call)?;
        if onfulfilled_signatures.is_empty() {
            if let Some(error_node) = error_node {
                self.error_at(
                    Some(error_node),
                    &tsrs2_diags::gen::The_first_parameter_of_the_then_method_of_a_promise_must_be_a_callback,
                    &[],
                );
            }
            return Ok((None, None));
        }
        let mut value_types = Vec::with_capacity(onfulfilled_signatures.len());
        for &signature in &onfulfilled_signatures {
            value_types.push(self.get_type_of_first_parameter_of_signature(signature)?);
        }
        let promised = self.get_union_type_ex(&value_types, UnionReduction::Subtype)?;
        self.links
            .set_type_promised_type_of_promise(self.speculation_depth, ty, promised);
        Ok((Some(promised), None))
    }

    /// tsc-port: allTypesAssignableToKind @6.0.3
    /// tsc-hash: 25de6c560affc5674fb9d5ffc29722e879ac6d1ce99705bcee879c7b4e389d58
    /// tsc-span: _tsc.js:79537-79539
    pub(crate) fn all_types_assignable_to_kind(
        &mut self,
        source: TypeId,
        kind: TypeFlags,
    ) -> CheckResult2<bool> {
        if self.tables.flags_of(source).intersects(TypeFlags::UNION) {
            let constituents: Vec<TypeId> = match &self.tables.type_of(source).data {
                tsrs2_types::TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies union data"),
            };
            for constituent in constituents {
                if !self.all_types_assignable_to_kind(constituent, kind)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        self.is_type_assignable_to_kind(source, kind, /*strict*/ false)
    }

    /// getTypeOfFirstParameterOfSignature(WithFallback neverType).
    fn get_type_of_first_parameter_of_signature(
        &mut self,
        signature: crate::state::SignatureId,
    ) -> CheckResult2<TypeId> {
        if self.signature_of(signature).parameters.is_empty() {
            return Ok(self.tables.intrinsics.never);
        }
        self.get_type_at_position(signature, 0)
    }
}

impl<'a> CheckerState<'a> {
    /// tsc-port: checkIndexedAccess @6.0.3
    /// tsc-hash: da7de5599179770799ebfdf4f1d33878c672e95c9361e33df09c5ded59aa2afc
    /// tsc-span: _tsc.js:75711-75744
    ///
    /// Receiver widening is the 5.6 [WIDEN] identity; the index checks
    /// BEFORE the errorType bail (its diagnostics must land).
    pub(crate) fn check_indexed_access(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(node);
        if node_util::node_flags(source, node).intersects(NodeFlags::OPTIONAL_CHAIN) {
            return self.check_element_access_chain(node, check_mode);
        }
        let NodeData::ElementAccessExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(expression) = data.expression else {
            return Err(Unsupported::new("ElementAccessExpression recovery node"));
        };
        let expr_type = self.check_non_null_expression(expression)?;
        self.check_element_access_expression(node, expr_type, check_mode)
    }

    fn check_element_access_chain(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let NodeData::ElementAccessExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(expression) = data.expression else {
            return Err(Unsupported::new("ElementAccessExpression recovery node"));
        };
        let expr_type = self.check_expression(expression, CheckMode::NORMAL)?;
        let non_optional_type = self.get_optional_expression_type(expr_type, expression)?;
        let checked = self.check_non_null_type(non_optional_type, expression)?;
        let access_type = self.check_element_access_expression(node, checked, check_mode)?;
        self.propagate_optional_type_marker(access_type, node, non_optional_type != expr_type)
    }

    fn check_element_access_expression(
        &mut self,
        node: NodeId,
        expr_type: TypeId,
        _check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let NodeData::ElementAccessExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (Some(_expression), Some(index_expression)) =
            (data.expression, data.argument_expression)
        else {
            return Err(Unsupported::new("ElementAccessExpression recovery node"));
        };
        // checkElementAccessExpression receiver widening (75720):
        // assignment targets and method-call receivers read the
        // widened type.
        let object_type = if self.get_assignment_target_kind(node)
            != crate::expr::AssignmentKind::None
            || self.is_method_access_for_call(node)
        {
            self.get_widened_type(expr_type)?
        } else {
            expr_type
        };
        let index_type = self.check_expression(index_expression, CheckMode::NORMAL)?;
        if object_type == self.tables.intrinsics.error
            || object_type == self.tables.intrinsics.silent_never
        {
            return Ok(object_type);
        }
        if self.is_const_enum_object_type(object_type)
            && !matches!(
                self.kind_of(index_expression),
                SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
            )
        {
            self.error_at(
                Some(index_expression),
                &tsrs2_diags::gen::A_const_enum_member_can_only_be_accessed_using_a_string_literal,
                &[],
            );
            return Ok(self.tables.intrinsics.error);
        }
        let effective_index_type =
            if self.is_for_in_variable_for_numeric_property_names(index_expression)? {
                self.tables.intrinsics.number
            } else {
                index_type
            };
        let assignment_target_kind = self.get_assignment_target_kind(node);
        let access_flags = if assignment_target_kind == crate::expr::AssignmentKind::None {
            tsrs2_types::AccessFlags::EXPRESSION_POSITION
        } else {
            let mut bits = tsrs2_types::AccessFlags::WRITING.bits();
            if self.tables.is_generic_object_type(object_type)
                && !self.is_this_type_parameter(object_type)
            {
                bits |= tsrs2_types::AccessFlags::NO_INDEX_SIGNATURES.bits();
            }
            if assignment_target_kind == crate::expr::AssignmentKind::Compound {
                bits |= tsrs2_types::AccessFlags::EXPRESSION_POSITION.bits();
            }
            tsrs2_types::AccessFlags::from_bits(bits)
        };
        let indexed_access_type = self
            .get_indexed_access_type_or_undefined(
                object_type,
                effective_index_type,
                access_flags,
                Some(node),
                None,
                None,
            )?
            .unwrap_or(self.tables.intrinsics.error);
        let resolved_symbol = match self.links.node(node).resolved_symbol {
            crate::links::LinkSlot::Resolved(symbol) => Some(symbol),
            _ => None,
        };
        let flow_type = self.get_flow_type_of_access_expression(
            node,
            resolved_symbol,
            indexed_access_type,
            index_expression,
        )?;
        self.check_indexed_access_index_type(flow_type, node)
    }

    /// tsc-port: isForInVariableForNumericPropertyNames @6.0.3
    /// tsc-hash: 5c9cd3056bea1f4fbb01e9b4f28c93e0a990b94b409ab0b8eabb0afe855960f1
    /// tsc-span: _tsc.js:75678-75710
    fn is_for_in_variable_for_numeric_property_names(
        &mut self,
        expr: NodeId,
    ) -> CheckResult2<bool> {
        let mut e = expr;
        while self.kind_of(e) == SyntaxKind::ParenthesizedExpression {
            let NodeData::ParenthesizedExpression(data) = self.data_of(e) else {
                break;
            };
            let Some(inner) = data.expression else { break };
            e = inner;
        }
        if self.kind_of(e) != SyntaxKind::Identifier {
            return Ok(false);
        }
        let Some(symbol) = self.get_resolved_symbol(e)? else {
            return Ok(false);
        };
        if !self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::VARIABLE)
        {
            return Ok(false);
        }
        let mut child = expr;
        let mut node = self.parent_of(expr);
        while let Some(current) = node {
            if self.kind_of(current) == SyntaxKind::ForInStatement {
                let NodeData::ForInStatement(data) = self.data_of(current) else {
                    unreachable!("kind/data agree");
                };
                if data.statement == Some(child) {
                    let for_in_symbol = self.get_for_in_variable_symbol(current)?;
                    if for_in_symbol == Some(symbol) {
                        if let Some(expression) = data.expression {
                            let expression_type =
                                self.check_expression_cached(expression, CheckMode::NORMAL)?;
                            let infos = self.get_index_infos_of_type(expression_type)?;
                            let number = self.tables.intrinsics.number;
                            if infos.len() == 1 && infos.iter().any(|info| info.key_type == number)
                            {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
            child = current;
            node = self.parent_of(current);
        }
        Ok(false)
    }

    fn get_for_in_variable_symbol(&mut self, node: NodeId) -> CheckResult2<Option<SymbolId>> {
        let NodeData::ForInStatement(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(initializer) = data.initializer else {
            return Ok(None);
        };
        match self.kind_of(initializer) {
            SyntaxKind::VariableDeclarationList => {
                let NodeData::VariableDeclarationList(list) = self.data_of(initializer) else {
                    unreachable!("kind/data agree");
                };
                let first = list.declarations.and_then(|declarations| {
                    self.binder
                        .source_of_node(initializer)
                        .arena
                        .node_array(declarations)
                        .nodes
                        .first()
                        .copied()
                });
                if let Some(variable) = first {
                    let name_is_pattern = self.name_of_node(variable).is_some_and(|name| {
                        matches!(
                            self.kind_of(name),
                            SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                        )
                    });
                    if !name_is_pattern {
                        return Ok(Some(self.get_symbol_of_declaration(variable)?));
                    }
                }
                Ok(None)
            }
            SyntaxKind::Identifier => self.get_resolved_symbol(initializer),
            _ => Ok(None),
        }
    }

    /// tsc-port: checkIndexedAccessIndexType @6.0.3
    /// tsc-hash: a2cd8152c78bd2c6042737704036aee3897bd1e8fb54378add79f82572845461
    /// tsc-span: _tsc.js:81893-81919
    ///
    /// The mapped-readonly write arm (2542) gates on ObjectFlags::MAPPED
    /// — mapped types have no producer yet, so the modifiers read stays
    /// a named escape behind the dead gate.
    pub(crate) fn check_indexed_access_index_type(
        &mut self,
        ty: TypeId,
        access_node: NodeId,
    ) -> CheckResult2<TypeId> {
        if !self
            .tables
            .flags_of(ty)
            .intersects(TypeFlags::INDEXED_ACCESS)
        {
            return Ok(ty);
        }
        let (object_type, index_type) = match &self.tables.type_of(ty).data {
            tsrs2_types::TypeData::IndexedAccess {
                object_type,
                index_type,
                ..
            } => (*object_type, *index_type),
            _ => unreachable!("IndexedAccess flag implies payload"),
        };
        let number = self.tables.intrinsics.number;
        let has_number_index_info = self
            .get_index_infos_of_type(object_type)?
            .iter()
            .any(|info| info.key_type == number);
        let key_of_object = self.get_index_type(object_type, tsrs2_types::IndexFlags::NONE)?;
        let constituents = self.union_members_or_self(index_type);
        let mut every_assignable = true;
        for t in constituents {
            let ok = self.is_type_assignable_to(t, key_of_object)?
                || has_number_index_info && self.is_applicable_index_type(t, number)?;
            if !ok {
                every_assignable = false;
                break;
            }
        }
        if every_assignable {
            let source = self.binder.source_of_node(access_node);
            if self.kind_of(access_node) == SyntaxKind::ElementAccessExpression
                && node_util::is_assignment_target(source, access_node)
                && self
                    .tables
                    .object_flags_of(object_type)
                    .intersects(tsrs2_types::ObjectFlags::MAPPED)
            {
                return Err(Unsupported::new(
                    "mapped-readonly write 2542 (getMappedTypeModifiers — mapped family, M8)",
                ));
            }
            return Ok(ty);
        }
        if self.tables.is_generic_object_type(object_type) {
            if let Some(property_name) = self.property_name_from_type_usable(index_type) {
                let apparent = self.get_apparent_type(object_type)?;
                let apparent_members = self.union_members_or_self(apparent);
                let mut property_symbol = None;
                for t in apparent_members {
                    if let Some(found) = self.get_property_of_type_full(t, &property_name)? {
                        property_symbol = Some(found);
                        break;
                    }
                }
                if let Some(property_symbol) = property_symbol {
                    if self
                        .get_declaration_modifier_flags_from_symbol(property_symbol)
                        .intersects(ModifierFlags::NON_PUBLIC_ACCESSIBILITY_MODIFIER)
                    {
                        let display = tsrs2_binder::unescape_leading_underscores(&property_name);
                        self.error_at(
                            Some(access_node),
                            &tsrs2_diags::gen::Private_or_protected_member_0_cannot_be_accessed_on_a_type_parameter,
                            &[display],
                        );
                        return Ok(self.tables.intrinsics.error);
                    }
                }
            }
        }
        let index_display = self.type_to_string_slice(index_type)?;
        let object_display = self.type_to_string_slice(object_type)?;
        self.error_at(
            Some(access_node),
            &tsrs2_diags::gen::Type_0_cannot_be_used_to_index_type_1,
            &[&index_display, &object_display],
        );
        Ok(self.tables.intrinsics.error)
    }
}

#[cfg(test)]
mod tests {
    use tsrs2_types::CompilerOptions;

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Driver-level fixture check (literals.rs idiom): oracle-pinned
    /// rows (tsc 6.0.3, noLib, options {} unless stated) — scratchpad
    /// matrix-risk{1,4}.out, 2026-07-12.
    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            rows(state)
        })
    }

    fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
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
    }

    // ---- risk-#4 selection matrix ----

    #[test]
    fn nullable_union_receiver_contains_until_flow_lands() {
        // Oracle: (18047, 32, 1) + (2339, 34, 6). The receiver is a
        // narrowable identifier — the [FLOW M5] gate contains the
        // whole statement until narrowing exists. Restore the oracle
        // rows at M5.
        assert_eq!(
            checked_rows("declare const x: string | null;\nx.length;\n"),
            []
        );
    }

    #[test]
    fn undefined_union_receiver_contains_until_flow_lands() {
        // Oracle: (18048, 37, 1) + (2339, 39, 6) — [FLOW M5] gate.
        assert_eq!(
            checked_rows("declare const x: string | undefined;\nx.length;\n"),
            []
        );
    }

    #[test]
    fn both_nullable_receiver_contains_until_flow_lands() {
        // Oracle: (18049, 44, 1) + (2339, 46, 6) — [FLOW M5] gate.
        assert_eq!(
            checked_rows("declare const x: string | null | undefined;\nx.length;\n"),
            []
        );
    }

    #[test]
    fn unknown_receiver_contains_until_flow_lands() {
        // Oracle: (18046, 26, 1) — [FLOW M5] gate (typeof guards can
        // narrow unknown).
        assert_eq!(checked_rows("declare const x: unknown;\nx.length;\n"), []);
    }

    #[test]
    fn void_receiver_reports_plain_2339_on_void() {
        // facts ∌ void: NOT an 18048-family report (oracle-pinned).
        assert_eq!(
            checked_rows("declare const x: void;\nx.foo;\n"),
            [(2339, 25, 3)]
        );
    }

    #[test]
    fn never_receiver_reports_plain_2339() {
        assert_eq!(
            checked_rows("declare const x: never;\nx.foo;\n"),
            [(2339, 26, 3)]
        );
    }

    #[test]
    fn null_literal_receiver_reports_18050() {
        assert_eq!(checked_rows("null.foo;\n"), [(18050, 0, 4)]);
    }

    #[test]
    fn parenthesized_null_receiver_reports_2531() {
        // Parens defeat BOTH the NullKeyword kind test and the
        // entity-name test (oracle-pinned).
        assert_eq!(checked_rows("(null).foo;\n"), [(2531, 0, 6)]);
    }

    #[test]
    fn chained_entity_name_contains_until_flow_lands() {
        // Oracle: (18047, 46, 3) "'x.a' is possibly 'null'." —
        // property-access receivers narrow too ([FLOW M5] gate).
        assert_eq!(
            checked_rows("declare const x: { a: { b: number } | null };\nx.a.b;\n"),
            []
        );
    }

    #[test]
    fn optional_chain_root_silences_nullable_receiver() {
        assert_eq!(
            checked_rows("declare const x: { a: number } | null;\nx?.a;\n"),
            []
        );
    }

    #[test]
    fn optional_root_then_plain_link_contains_until_flow_lands() {
        // Oracle: (18047, 58, 4) — span includes the `?.`, message
        // renders 'x.a' (entityNameToString). [FLOW M5] gate.
        assert_eq!(
            checked_rows("declare const x: { a: { b: number } | null } | undefined;\nx?.a.b;\n"),
            []
        );
    }

    #[test]
    fn nonnull_assertion_strips_silently() {
        assert_eq!(
            checked_rows("declare const x: { a: number } | null;\nx!.a;\n"),
            []
        );
        // `x!` on void: silent, downstream never (oracle-pinned).
        assert_eq!(
            checked_rows("declare const x: void;\nx!.foo;\n"),
            [(2339, 26, 3)]
        );
    }

    // ---- risk-#1 spelling matrix (property side) ----

    #[test]
    fn insertion_typo_reports_2551_with_related_2728() {
        // Anonymous-receiver flavors stay contained until the
        // nodeBuilder display slice (T2) — interface receivers pin the
        // band (oracle re-probed 2026-07-12).
        let text = "interface O { hello: string }\ndeclare const o: O;\no.helo;\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (2551, Some(52), Some(4))
            );
            assert_eq!(
                diag.message_text(),
                "Property 'helo' does not exist on type 'O'. Did you mean 'hello'?"
            );
            assert_eq!(diag.related.len(), 1);
            assert_eq!(diag.related[0].message.code, 2728);
            assert_eq!(diag.related[0].start, Some(14));
            assert_eq!(diag.related[0].length, Some(5));
        });
    }

    #[test]
    fn substitution_typo_len3_gets_no_suggestion() {
        assert_eq!(
            checked_rows("interface O { abc: string }\ndeclare const o: O;\no.abd;\n"),
            [(2339, 50, 3)]
        );
    }

    #[test]
    fn substitution_typo_len5_suggests() {
        assert_eq!(
            checked_rows("interface O { world: string }\ndeclare const o: O;\no.worls;\n"),
            [(2551, 52, 5)]
        );
    }

    #[test]
    fn case_flip_suggests() {
        assert_eq!(
            checked_rows("interface O { hello: string }\ndeclare const o: O;\no.HELLO;\n"),
            [(2551, 52, 5)]
        );
    }

    #[test]
    fn short_candidate_needs_case_insensitive_match() {
        assert_eq!(
            checked_rows("interface O { ab: number }\ndeclare const o: O;\no.ax;\no.AB;\n"),
            [(2339, 49, 2), (2551, 55, 2)]
        );
    }

    #[test]
    fn union_chain_names_first_lacking_constituent() {
        let text = "interface A { a: number; c: string }\ninterface B { b: number; c: string }\ndeclare const o: A | B;\no.d;\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1);
            let diag = diags[0];
            assert_eq!((diag.code(), diag.start), (2339, Some(100)));
            assert_eq!(
                diag.message_text(),
                "Property 'd' does not exist on type 'A | B'."
            );
            assert_eq!(diag.message.next.len(), 1);
            assert_eq!(
                diag.message.next[0].text,
                "Property 'd' does not exist on type 'A'."
            );
        });
    }

    #[test]
    fn static_member_suggestion_reports_2576() {
        assert_eq!(
            checked_rows("class C { static s = 1; }\ndeclare const c: C;\nc.s;\n"),
            [(2576, 48, 1)]
        );
    }

    #[test]
    fn thenable_miss_gets_await_hint_2773() {
        let text = "interface P { then(cb: (x: { a: number }) => void): void }\ndeclare const p: P;\np.a;\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!((diag.code(), diag.start), (2339, Some(81)));
            assert_eq!(diag.related.len(), 1);
            assert_eq!(diag.related[0].message.code, 2773);
            assert_eq!(diag.related[0].start, Some(81));
        });
    }

    #[test]
    fn never_intersection_elaborates_reduction_reason() {
        let text = "interface A { a: 1 }\ninterface B { a: 2 }\ndeclare const o: A & B;\no.b;\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (2339, Some(68), Some(1))
            );
            assert_eq!(diag.message.next.len(), 1);
            assert_eq!(
                diag.message.next[0].text,
                "The intersection 'A & B' was reduced to 'never' because property 'a' has conflicting types in some constituents."
            );
        });
    }

    // ---- name-side suggestion budget (noLib burn) ----

    // ---- element-access ladder (risk-#1 order; oracle re-probed
    // with named receivers 2026-07-12) ----

    #[test]
    fn element_spelling_2551_has_no_related() {
        let text = "interface O { hello: number }\ndeclare const o: O;\no[\"helo\"];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (2551, Some(52), Some(6))
            );
            // The element-side flavor carries NO related 2728
            // (oracle-pinned asymmetry vs the property side).
            assert_eq!(diag.related.len(), 0);
        });
    }

    #[test]
    fn element_literal_miss_reports_7053_chain() {
        let text = "interface O { hello: number }\ndeclare const o: O;\no[\"xyz\"];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (7053, Some(50), Some(8))
            );
            assert_eq!(
                diag.message_text(),
                "Element implicitly has an 'any' type because expression of type '\"xyz\"' can't be used to index type 'O'."
            );
            assert_eq!(diag.message.next.len(), 1);
            assert_eq!(
                diag.message.next[0].text,
                "Property 'xyz' does not exist on type 'O'."
            );
        });
    }

    #[test]
    fn element_number_index_reports_7015_on_index_expression() {
        assert_eq!(
            checked_rows(
                "interface O { [n: number]: string }\ndeclare const o: O;\ndeclare const s: string;\no[s];\n"
            ),
            [(7015, 83, 1)]
        );
    }

    #[test]
    fn element_get_method_probe_reports_7052() {
        let text = "interface O { get(k: string): number }\ndeclare const o: O;\ndeclare const k: string;\no[k];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (7052, Some(84), Some(4))
            );
            assert!(diag
                .message_text()
                .ends_with("Did you mean to call 'o.get'?"));
        });
    }

    #[test]
    fn element_string_key_reports_7053_no_index_signature() {
        let text =
            "interface O { a: number }\ndeclare const o: O;\ndeclare const k: string;\no[k];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (7053, Some(71), Some(4))
            );
            assert_eq!(
                diag.message.next[0].text,
                "No index signature with a parameter of type 'string' was found on type 'O'."
            );
        });
    }

    #[test]
    fn element_static_member_reports_2576_with_bracket_text() {
        let text = "class C { static s = 1; }\ndeclare const c: C;\nc[\"s\"];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (2576, Some(46), Some(6))
            );
            assert!(
                diag.message_text()
                    .ends_with("Did you mean to access the static member 'C[\"s\"]' instead?"),
                "{}",
                diag.message_text()
            );
        });
    }

    #[test]
    fn element_union_receiver_reports_single_7053() {
        let text = "interface A { a: number }\ninterface B { b: number }\ndeclare const o: A | B;\ndeclare const k: string;\no[k];\n";
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            state.check_source_file(0);
            let diags: Vec<_> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .collect();
            assert_eq!(diags.len(), 1, "{diags:?}");
            let diag = diags[0];
            assert_eq!(
                (diag.code(), diag.start, diag.length),
                (7053, Some(101), Some(4))
            );
            assert_eq!(
                diag.message.next[0].text,
                "No index signature with a parameter of type 'string' was found on type 'A | B'."
            );
        });
    }

    #[test]
    fn tuple_out_of_range_contains_until_tuple_display_lands() {
        // Oracle: (2493, 37, 1) "Tuple type '[string, number]' of
        // length '2' has no element at index '5'." — the tuple
        // rendering is nodeBuilder (T2), so the statement CONTAINS
        // (honest FN). Flip to the oracle row when T2 lands.
        assert_eq!(
            checked_rows("declare const t: [string, number];\nt[5];\n"),
            []
        );
    }

    #[test]
    fn string_index_signature_hit_is_silent() {
        assert_eq!(
            checked_rows(
                "interface O { [k: string]: number }\ndeclare const o: O;\no[\"anything\"];\n"
            ),
            []
        );
    }

    #[test]
    fn nolib_burn_exhausts_name_suggestions() {
        // Bootstrap burns all 10 slots: near-miss names degrade to
        // plain 2304 (oracle-pinned; the LIB-LOADED 2552 flavor is
        // conformance-gated).
        assert_eq!(checked_rows("const hello = 1;\nhelo;\n"), [(2304, 17, 4)]);
    }

    #[test]
    fn strict_bind_call_apply_off_frees_two_slots() {
        // burn=8 ⇒ suggestions #9/#10 live, #11 degrades (the full
        // budget mechanics in one noLib pin).
        let options = CompilerOptions {
            strict_bind_call_apply: Some(false),
            ..CompilerOptions::default()
        };
        let text = "const hello = 1;\nconst world = 1;\nconst tiger = 1;\nhelo;\nworl;\ntige;\n";
        with_program_state(&[("a.ts", text)], &options, |state| {
            state.check_source_file(0);
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .map(|d| d.code())
                .collect();
            assert_eq!(codes, [2552, 2552, 2304]);
        });
    }

    #[test]
    fn guard_arm_2693_does_not_consume_budget() {
        // A guard-chain arm (the primitive-name 2693 flavor; the
        // 2662/2663 MissingPrefix arms need class-body checking, 5.8)
        // returns BEFORE the budget block — both later near-misses
        // still suggest (oracle-pinned under strictBindCallApply:false).
        let options = CompilerOptions {
            strict_bind_call_apply: Some(false),
            ..CompilerOptions::default()
        };
        let text = "const hello = 1;\nconst world = 1;\nstring;\nhelo;\nworl;\n";
        with_program_state(&[("a.ts", text)], &options, |state| {
            state.check_source_file(0);
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .map(|d| d.code())
                .collect();
            assert_eq!(codes, [2693, 2552, 2552]);
        });
    }

    #[test]
    fn no_suggestion_failure_still_consumes_budget() {
        let options = CompilerOptions {
            strict_bind_call_apply: Some(false),
            ..CompilerOptions::default()
        };
        let text = "const hello = 1;\nconst world = 1;\nxyzzy;\nhelo;\nworl;\n";
        with_program_state(&[("a.ts", text)], &options, |state| {
            state.check_source_file(0);
            let codes: Vec<u32> = state
                .diagnostics
                .iter()
                .filter(|d| d.file_name.is_some())
                .map(|d| d.code())
                .collect();
            assert_eq!(codes, [2304, 2552, 2304]);
        });
    }

    // ---- 5.7b: the 2729 declared-before-use band (scratchpad
    // pins/r{1,2}.ts, oracle-probed 2026-07-13) ----

    #[test]
    fn static_property_used_before_initialization_reports_2729() {
        assert_eq!(
            checked_rows("class C {\n    static a = C.b;\n    static b = 1;\n}\nC.a;\n"),
            [(2729, 27, 1)]
        );
    }

    #[test]
    fn instance_property_used_before_initialization_stays_contained() {
        // Oracle: 2729 at `b` (23+1) — LIVE since the VALUE_MODULE
        // getTypeOfSymbol arm landed (5.8d) un-contained the strict
        // noImplicitThis globalThis probe on the `this` receiver.
        assert_eq!(
            checked_rows("class E {\n    a = this.b;\n    b = 1;\n}\ndeclare const e: E;\ne.a;\n"),
            [(2729, 23, 1)]
        );
    }

    #[test]
    fn property_used_after_its_declaration_is_clean() {
        // The positional walk's other face: b precedes a (static so
        // the receiver stays this-free).
        assert_eq!(
            checked_rows("class G {\n    static b = 1;\n    static a = G.b;\n}\nG.a;\n"),
            []
        );
    }
}
