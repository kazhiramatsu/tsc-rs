//! M4 5.5a: expression checking — the driver + leaf arms.
//!
//! checkExpression (80960) / checkExpressionWorker (81011) ported with
//! the FULL kind dispatch in tsc switch order; arms whose workers land
//! in a later 5.5 slice (b contextual / c literals / d access+facts /
//! e operators / f functions+await+JSX) or a later stage (5.7 calls,
//! 5.8 declarations) are named Unsupported escapes (grep
//! `expression_stub`) — NEVER unreachable!(): expression statements
//! route arbitrary fixture code through here, and per-element
//! containment (check_source_element) turns each escape into an honest
//! FN for that statement only.
//!
//! Stage-boundary stubs introduced here (m4-55-expression-extraction.md
//! §0):
//! - [FLOW → M5] `get_flow_type_of_reference_stub` — the ONE seam M5
//!   replaces (getFlowTypeOfReference 70394 returns the declared type).
//!   With it: isSymbolAssignedDefinitely/isPastLastAssignment answer
//!   their no-marking defaults, the flow-container widening loop
//!   (72193) is stubbed out, and checkIdentifier's auto-type (7034/
//!   7005) and 2454 arms self-deactivate (flowType == declared type).
//! - [FACTS → 5.5d] the initialType ladder (72198) is dead under the
//!   [FLOW] stub — getFlowTypeOfReference ignores `initial` — so
//!   removeOptionalityFromDeclaredType/getOptionalType ride with the
//!   facts classifier at 5.5d rather than being half-ported here.
//! - [CONTEXT] the getContextualType consumers
//!   (hasContextualTypeWithNoGenericTypes, getContextualThisParameterType)
//!   are live since 5.5b (contextual.rs owns the band).

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::{gen as diagnostics, DiagnosticMessage, MessageChain, RelatedInfo};
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{
    CheckMode, ModifierFlags, NodeCheckFlags, ObjectFlags, PseudoBigInt, ScriptTarget,
    SymbolFlags, TypeData, TypeFlags, TypeId,
};

use crate::state::{CheckerState, CheckResult2, Unsupported};

/// tsc AssignmentKind (15579 band): None / Definite / Compound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AssignmentKind {
    None,
    Definite,
    Compound,
}

/// tsc AccessKind (17458 band): Read / Write / ReadWrite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AccessKind {
    Read,
    Write,
    ReadWrite,
}

/// findAncestor callback verdicts (tsc's boolean | "quit").
pub(crate) enum Ancestor {
    Yes,
    No,
    Quit,
}

impl<'a> CheckerState<'a> {
    /// tsc findAncestor (12299): walk node.parent upward until the
    /// callback answers Yes (return that node) or Quit (return None).
    pub(crate) fn find_ancestor(
        &self,
        start: Option<NodeId>,
        mut callback: impl FnMut(&Self, NodeId) -> Ancestor,
    ) -> Option<NodeId> {
        let mut current = start;
        while let Some(node) = current {
            match callback(self, node) {
                Ancestor::Yes => return Some(node),
                Ancestor::Quit => return None,
                Ancestor::No => current = self.parent_of(node),
            }
        }
        None
    }

    // ---- the driver (checkExpression band) ----

    /// tsc-port: checkExpression @6.0.3
    /// tsc-hash: b56997759c77785af8c96e94324267893636cd83bfdc656d059ef139e4cd71ac
    /// tsc-span: _tsc.js:80960-80974
    ///
    /// The tracing pushes are elided. instantiationCount resets here —
    /// the third and last reset point (state.rs note closes).
    pub(crate) fn check_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        self.check_expression_with_force_tuple(node, check_mode, false)
    }

    pub(crate) fn check_expression_with_force_tuple(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
        force_tuple: bool,
    ) -> CheckResult2<TypeId> {
        let save_current_node = self.current_node;
        self.current_node = Some(node);
        self.instantiation_count = 0;
        let result = (|| {
            let uninstantiated = self.check_expression_worker(node, check_mode, force_tuple)?;
            let ty = self.instantiate_type_with_single_generic_call_signature(
                node,
                uninstantiated,
                check_mode,
            )?;
            if self.is_const_enum_object_type(ty) {
                self.check_const_enum_access(node, ty);
            }
            Ok(ty)
        })();
        self.current_node = save_current_node;
        result
    }

    /// tsc-port: instantiateTypeWithSingleGenericCallSignature @6.0.3 (5.5a gate slice)
    /// tsc-hash: 3fb3555fc3869377868f1a5f6f9d67871b494538220dd568554ee287b7e2882a
    /// tsc-span: _tsc.js:80751-80815
    ///
    /// The step-1 gate `checkMode & (Inferential | SkipGenericFunctions)`
    /// is UNREACHABLE at M4: the producible CheckMode set is
    /// {0, 1, 4, 32, 64} (extraction doc §2 audit — no producer sets
    /// bits 2 or 8 until M6), so the wrapper reduces to the identity.
    fn instantiate_type_with_single_generic_call_signature(
        &mut self,
        _node: NodeId,
        ty: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        if check_mode
            .intersects(CheckMode::INFERENTIAL | CheckMode::SKIP_GENERIC_FUNCTIONS)
        {
            unreachable!(
                "Inferential/SkipGenericFunctions have no producer until M6 \
                 (CheckMode audit, m4-55-expression-extraction.md §2)"
            );
        }
        Ok(ty)
    }

    /// tsc-port: isConstEnumObjectType @6.0.3
    /// tsc-hash: c84eab899298aa71e2408a793e0d60443b4265951883ed920daf1235d21e5dac
    /// tsc-span: _tsc.js:79540-79542
    pub(crate) fn is_const_enum_object_type(&self, ty: TypeId) -> bool {
        if !self
            .tables
            .object_flags_of(ty)
            .intersects(ObjectFlags::ANONYMOUS)
        {
            return false;
        }
        let Some(symbol) = self.tables.type_of(ty).symbol else {
            return false;
        };
        self.binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::CONST_ENUM)
    }

    /// tsc-port: checkConstEnumAccess @6.0.3 (2475 slice)
    /// tsc-hash: 6146127432061a4f34628e317f8d73bc8070bfb61cd3ce4e32c8fe76f1569690
    /// tsc-span: _tsc.js:80975-80999
    ///
    /// The 2748 ambient-const-enum arm is gated on `isolatedModules ||
    /// (verbatimModuleSyntax && ...)` — both options are absent from
    /// CompilerOptions, so the whole second block is constant-false
    /// and elided (its resolveName probe and redirect chase with it).
    /// Note tsc does NOT early-return after the 2475 emission.
    fn check_const_enum_access(&mut self, node: NodeId, _ty: TypeId) {
        let parent = self.parent_of(node);
        let ok = parent.is_some_and(|parent| match self.data_of(parent) {
            NodeData::PropertyAccessExpression(data) => data.expression == Some(node),
            NodeData::ElementAccessExpression(data) => data.expression == Some(node),
            NodeData::TypeQuery(data) => data.expr_name == Some(node),
            NodeData::ExportSpecifier(_) => true,
            _ => false,
        }) || (matches!(
            self.kind_of(node),
            SyntaxKind::Identifier | SyntaxKind::QualifiedName
        ) && self.is_in_right_side_of_import_or_export_assignment(node));
        if !ok {
            self.error_at(
                Some(node),
                &diagnostics::const_enums_can_only_be_used_in_property_or_index_access_expressions_or_the_right_hand_side_of_an_import_declaration_or_export_assignment_or_type_query,
                &[],
            );
        }
    }

    /// tsc-port: isInRightSideOfImportOrExportAssignment @6.0.3
    /// (getLeftSideOfImportEqualsOrExportAssignment folded in)
    /// tsc-hash: fd9f4f517974459a76ad5fcb8a9489ce11f1e433ef38af8c6911aec2efc24e10
    /// tsc-span: _tsc.js:87252-87266
    fn is_in_right_side_of_import_or_export_assignment(&self, node: NodeId) -> bool {
        let mut node = node;
        while let Some(parent) = self.parent_of(node) {
            if self.kind_of(parent) != SyntaxKind::QualifiedName {
                break;
            }
            node = parent;
        }
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::ImportEqualsDeclaration(data) => data.module_reference == Some(node),
            NodeData::ExportAssignment(data) => data.expression == Some(node),
            _ => false,
        }
    }

    /// One Unsupported escape per not-yet-landed checkExpressionWorker
    /// arm; the tsc worker name + owning slice make each disposition
    /// greppable. Never unreachable!(): fixture code reaches every arm.
    fn expression_stub(&self, worker: &str, owner: &str) -> CheckResult2<TypeId> {
        Err(Unsupported::new(format!(
            "{worker} (expression band, lands at {owner})"
        )))
    }

    /// tsc-port: checkExpressionWorker @6.0.3
    /// tsc-hash: 74f2718002f80385323e72b43d81bac59101667dab48845c89e6e2f508b19d62
    /// tsc-span: _tsc.js:81011-81127
    ///
    /// The cancellationToken pre-check is elided (no cancellation).
    fn check_expression_worker(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
        force_tuple: bool,
    ) -> CheckResult2<TypeId> {
        match self.kind_of(node) {
            SyntaxKind::Identifier => self.check_identifier(node, check_mode),
            SyntaxKind::PrivateIdentifier => self.check_private_identifier_expression(node),
            SyntaxKind::ThisKeyword => self.check_this_expression(node),
            SyntaxKind::SuperKeyword => self.check_super_expression(node),
            SyntaxKind::NullKeyword => Ok(self.tables.intrinsics.null_widening),
            SyntaxKind::NoSubstitutionTemplateLiteral | SyntaxKind::StringLiteral => {
                // hasSkipDirectInferenceFlag → blockedStringType: the
                // links flag is written only by inference (M6); no
                // producer exists, so the fresh-literal arm is the
                // whole behavior.
                let text = match self.data_of(node) {
                    NodeData::StringLiteral(data) => data.text.clone(),
                    NodeData::NoSubstitutionTemplateLiteral(data) => data.text.clone(),
                    _ => unreachable!("kind/data agree"),
                };
                let literal = self.tables.get_string_literal_type(&text);
                Ok(self.tables.get_fresh_type_of_literal_type(literal))
            }
            SyntaxKind::NumericLiteral => {
                // checkGrammarNumericLiteral (90342) emits only the
                // 80008-family (suggestion band, unmodeled) — skipped.
                let NodeData::NumericLiteral(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let value = crate::annotate::parse_numeric_literal_text(&data.text)?;
                let literal = self.tables.get_number_literal_type(value);
                Ok(self.tables.get_fresh_type_of_literal_type(literal))
            }
            SyntaxKind::BigIntLiteral => {
                self.check_grammar_big_int_literal(node);
                let NodeData::BigIntLiteral(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let value = parse_pseudo_big_int(&data.text)?;
                let literal = self.tables.get_bigint_literal_type(value);
                Ok(self.tables.get_fresh_type_of_literal_type(literal))
            }
            SyntaxKind::TrueKeyword => Ok(self.tables.intrinsics.true_fresh),
            SyntaxKind::FalseKeyword => Ok(self.tables.intrinsics.false_fresh),
            SyntaxKind::TemplateExpression => {
                self.check_template_expression(node)
            }
            SyntaxKind::RegularExpressionLiteral => {
                self.check_regular_expression_literal(node)
            }
            SyntaxKind::ArrayLiteralExpression => {
                self.check_array_literal(node, check_mode, force_tuple)
            }
            SyntaxKind::ObjectLiteralExpression => self.check_object_literal(node, check_mode),
            SyntaxKind::PropertyAccessExpression => {
                self.check_property_access_expression(node, check_mode, /*write_only*/ false)
            }
            SyntaxKind::QualifiedName => self.check_qualified_name(node, check_mode),
            SyntaxKind::ElementAccessExpression => self.check_indexed_access(node, check_mode),
            SyntaxKind::CallExpression => {
                if self.is_import_call(node) {
                    return self.expression_stub("checkImportCallExpression", "5.7b");
                }
                // `import.defer(...)`: the deferred dynamic-import
                // flavor rides the import-call band (checkMetaProperty
                // answers errorType for the bare `import.defer`, which
                // must not leak into untyped-call arg checking).
                let defer_call = matches!(self.data_of(node), NodeData::CallExpression(data)
                    if data.expression.is_some_and(|expression| {
                        self.kind_of(expression) == SyntaxKind::MetaProperty
                            && !self.meta_property_is_new(expression)
                    }));
                if defer_call {
                    return self.expression_stub(
                        "checkImportCallExpression (import.defer)",
                        "5.7b",
                    );
                }
                self.check_call_expression(node, check_mode)
            }
            SyntaxKind::NewExpression => self.check_call_expression(node, check_mode),
            SyntaxKind::TaggedTemplateExpression => {
                self.expression_stub("checkTaggedTemplateExpression", "5.7b")
            }
            SyntaxKind::ParenthesizedExpression => {
                self.check_parenthesized_expression(node, check_mode)
            }
            SyntaxKind::ClassExpression => {
                // checkClassExpression (84972) calls
                // checkClassLikeDeclaration EAGERLY — heritage/member
                // checks are one unit, so the whole arm escapes until
                // 5.8 (extraction doc §8).
                self.expression_stub("checkClassExpression", "5.8")
            }
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction => {
                self.check_function_expression_or_object_literal_method(node, check_mode)
            }
            SyntaxKind::TypeOfExpression => self.check_type_of_expression(node),
            SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression => {
                self.check_assertion(node, check_mode)
            }
            SyntaxKind::NonNullExpression => self.check_non_null_assertion(node),
            SyntaxKind::ExpressionWithTypeArguments => {
                self.check_expression_with_type_arguments(node)
            }
            SyntaxKind::SatisfiesExpression => self.check_satisfies_expression(node),
            SyntaxKind::MetaProperty => self.check_meta_property(node),
            SyntaxKind::DeleteExpression => self.check_delete_expression(node),
            SyntaxKind::VoidExpression => self.check_void_expression(node),
            SyntaxKind::AwaitExpression => self.check_await_expression(node),
            SyntaxKind::PrefixUnaryExpression => self.check_prefix_unary_expression(node),
            SyntaxKind::PostfixUnaryExpression => self.check_postfix_unary_expression(node),
            SyntaxKind::BinaryExpression => self.check_binary_expression(node, check_mode),
            SyntaxKind::ConditionalExpression => {
                self.check_conditional_expression(node, check_mode)
            }
            SyntaxKind::SpreadElement => {
                self.expression_stub("checkSpreadExpression ([ITER])", "5.5c")
            }
            SyntaxKind::OmittedExpression => Ok(self.tables.intrinsics.undefined_widening),
            SyntaxKind::YieldExpression => self.check_yield_expression(node),
            SyntaxKind::SyntheticExpression => unreachable!(
                "SyntheticExpression is checker-synthesized (5.5e destructuring); \
                 parsed trees never contain one"
            ),
            SyntaxKind::JsxExpression => self.check_jsx_expression(node),
            SyntaxKind::JsxElement => self.check_jsx_element(node),
            SyntaxKind::JsxSelfClosingElement => {
                self.check_jsx_self_closing_element(node)
            }
            SyntaxKind::JsxFragment => self.check_jsx_fragment(node),
            SyntaxKind::JsxAttributes => self.check_jsx_attributes_stub(),
            SyntaxKind::JsxOpeningElement => {
                unreachable!("Shouldn't ever directly check a JsxOpeningElement")
            }
            _ => Ok(self.tables.intrinsics.error),
        }
    }

    /// tsc isImportCall (16097): CallExpression whose expression is the
    /// `import` keyword.
    pub(crate) fn is_import_call(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::CallExpression(data) => data
                .expression
                .is_some_and(|expression| self.kind_of(expression) == SyntaxKind::ImportKeyword),
            _ => false,
        }
    }

    /// tsc-port: checkParenthesizedExpression @6.0.3
    /// tsc-hash: e09e928d312103d37dda4c13512d952ea898028cf66871324eb8b78f15db7144
    /// tsc-span: _tsc.js:81000-81010
    ///
    /// The JSDoc satisfies/type-assertion arms are elided project-wide
    /// (no JSDoc nodes parse).
    fn check_parenthesized_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let NodeData::ParenthesizedExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data.expression.ok_or_else(|| {
            Unsupported::new("parenthesized expression without operand (parse recovery)")
        })?;
        self.check_expression(expression, check_mode)
    }

    // ---- literal leaves ----

    /// tsc-port: checkGrammarBigIntLiteral @6.0.3
    /// tsc-hash: 82b45798947b25279e188e4bdc4b43e37c16aabe6d25911cfcd0f822020d73a1
    /// tsc-span: _tsc.js:90358-90368
    ///
    /// The boolean result is ignored by the caller (worker arm).
    fn check_grammar_big_int_literal(&mut self, node: NodeId) {
        let parent = self.parent_of(node);
        let literal_type = parent.is_some_and(|parent| {
            self.kind_of(parent) == SyntaxKind::LiteralType
                || (self.kind_of(parent) == SyntaxKind::PrefixUnaryExpression
                    && self
                        .parent_of(parent)
                        .is_some_and(|grand| self.kind_of(grand) == SyntaxKind::LiteralType))
        });
        if !literal_type {
            let ambient = self.node_flags(node) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0;
            if !ambient && self.options.emit_script_target() < ScriptTarget::ES2020 {
                self.grammar_error_on_node(
                    node,
                    &diagnostics::BigInt_literals_are_not_available_when_targeting_lower_than_ES2020,
                    &[],
                );
            }
        }
    }

    /// tsc-port: checkRegularExpressionLiteral @6.0.3 (once-flag slice)
    /// tsc-hash: e3904ad22a4b7597eead67ad00b86377b9ca72c3975bc95f5577df577a518be9
    /// tsc-span: _tsc.js:73931-73938
    ///
    /// The lazy checkGrammarRegularExpressionLiteral body (the regex
    /// validator, 1501-family) is an elided slice — the once-flag
    /// (TypeChecked on the literal's links) is wired so the validator
    /// drops in behind it; until then annotation-flag errors are FN.
    fn check_regular_expression_literal(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if !self
            .links
            .node(node)
            .check_flags
            .intersects(NodeCheckFlags::TYPE_CHECKED)
        {
            self.links.or_node_check_flags(
                self.speculation_depth,
                node,
                NodeCheckFlags::TYPE_CHECKED,
            );
            // checkGrammarRegularExpressionLiteral: elided (ledger).
        }
        self.global_regexp_type()
    }

    /// tsc grammarErrorOnNode (90253): node-span grammar error, gated
    /// on the file having NO parse diagnostics.
    pub(crate) fn grammar_error_on_node(
        &mut self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> bool {
        if self.has_parse_diagnostics(node) {
            return false;
        }
        self.error_at(Some(node), message, args);
        true
    }

    /// tsc grammarErrorOnFirstToken (90211): first-token-span grammar
    /// error, gated on the file having NO parse diagnostics.
    pub(crate) fn grammar_error_on_first_token(
        &mut self,
        node: NodeId,
        message: &'static DiagnosticMessage,
        args: &[&str],
    ) -> bool {
        if self.has_parse_diagnostics(node) {
            return false;
        }
        let source = self.binder.source_of_node(node);
        let pos = source.arena.node(node).pos as usize;
        // (start, end) byte offsets — the helper returns the token's
        // end, not a length.
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
        let end_utf16 = to_utf16(end);
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let diagnostic = tsrs2_diags::Diagnostic::new(
            Some(source.file_name.clone()),
            Some(start_utf16),
            Some(end_utf16.saturating_sub(start_utf16)),
            MessageChain::new(message, &args),
        );
        if !self.diagnostics.iter().any(|existing| *existing == diagnostic) {
            self.diagnostics.push(diagnostic);
        }
        true
    }

    /// tsc hasParseDiagnostics (47555).
    pub(crate) fn has_parse_diagnostics(&self, node: NodeId) -> bool {
        !self
            .binder
            .source_of_node(node)
            .parse_diagnostics
            .is_empty()
    }

    // ---- identifier band ----

    /// tsc isThisInTypeQuery (16707).
    pub(crate) fn is_this_in_type_query(&self, node: NodeId) -> bool {
        if !self.is_this_identifier(node) {
            return false;
        }
        let mut node = node;
        while let Some(parent) = self.parent_of(node) {
            let NodeData::QualifiedName(data) = self.data_of(parent) else {
                break;
            };
            if data.left != Some(node) {
                break;
            }
            node = parent;
        }
        self.parent_of(node)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::TypeQuery)
    }

    /// tsc-port: checkIdentifier @6.0.3
    /// tsc-hash: 9fe9fcc0b033367436dd525613db28f5705a42f8428a97215ce75bef6e985875
    /// tsc-span: _tsc.js:72126-72213
    ///
    /// Elisions/stubs, each per the extraction doc §3:
    /// - shouldMarkIdentifierAliasReferenced → markLinkedReferences:
    ///   alias-reference marking is emit/unused bookkeeping (5.8/M7) —
    ///   whole call elided.
    /// - contextualBindingPatterns membership: the stack is pushed only
    ///   by getTypeFromBindingPattern (5.5b); empty until then, so the
    ///   nonInferrableAnyType arm cannot fire — ported over the state
    ///   field that 5.5b starts pushing.
    /// - the flow-container widening loop (72193) needs
    ///   isPastLastAssignment ([FLOW] M5) — stubbed out, flowContainer
    ///   keeps its computed value.
    /// - isNeverInitialized reads isSymbolAssignedDefinitely — the M5
    ///   no-marking default (lastAssignmentPos unset → false) is what
    ///   the stub answers.
    /// - initialType (72198): dead under the [FLOW] stub (module note);
    ///   the auto-type arm (7034/7005) and the 2454 arm are ported
    ///   verbatim and self-deactivate (flowType == type).
    pub(crate) fn check_identifier(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        if self.is_this_in_type_query(node) {
            return self.check_this_expression(node);
        }
        let Some(symbol) = self.get_resolved_symbol(node) else {
            return Ok(self.tables.intrinsics.error);
        };
        self.check_identifier_calculate_node_check_flags(node, symbol);
        if symbol == self.arguments_symbol {
            if self.is_in_property_initializer_or_class_static_block(node, true) {
                return Ok(self.tables.intrinsics.error);
            }
            return self.arguments_symbol_type();
        }
        let local_or_export_symbol = self.get_export_symbol_of_value_symbol_if_exported(symbol);
        let mut declaration = self
            .binder
            .symbol(local_or_export_symbol)
            .value_declaration;
        let immediate_declaration = declaration;
        if let Some(decl) = declaration {
            if self.kind_of(decl) == SyntaxKind::BindingElement {
                let pattern = self.parent_of(decl);
                if pattern.is_some_and(|pattern| {
                    self.contextual_binding_patterns.contains(&pattern)
                }) && self
                    .find_ancestor(Some(node), |state, ancestor| {
                        if Some(ancestor) == pattern {
                            Ancestor::Yes
                        } else {
                            let _ = state;
                            Ancestor::No
                        }
                    })
                    .is_some()
                {
                    return Ok(self.tables.intrinsics.non_inferrable_any);
                }
            }
        }
        let mut ty = self.get_narrowed_type_of_symbol(local_or_export_symbol, node)?;
        let assignment_kind = self.get_assignment_target_kind(node);
        if assignment_kind != AssignmentKind::None {
            let local_flags = self.binder.symbol(local_or_export_symbol).flags;
            if !local_flags.intersects(SymbolFlags::VARIABLE)
                && !(self.is_in_js_file(node) && local_flags.intersects(SymbolFlags::VALUE_MODULE))
            {
                let message = if local_flags.intersects(SymbolFlags::ENUM) {
                    &diagnostics::Cannot_assign_to_0_because_it_is_an_enum
                } else if local_flags.intersects(SymbolFlags::CLASS) {
                    &diagnostics::Cannot_assign_to_0_because_it_is_a_class
                } else if local_flags.intersects(SymbolFlags::MODULE) {
                    &diagnostics::Cannot_assign_to_0_because_it_is_a_namespace
                } else if local_flags.intersects(SymbolFlags::FUNCTION) {
                    &diagnostics::Cannot_assign_to_0_because_it_is_a_function
                } else if local_flags.intersects(SymbolFlags::ALIAS) {
                    &diagnostics::Cannot_assign_to_0_because_it_is_an_import
                } else {
                    &diagnostics::Cannot_assign_to_0_because_it_is_not_a_variable
                };
                let display = self.symbol_display_name(symbol);
                self.error_at(Some(node), message, &[&display]);
                return Ok(self.tables.intrinsics.error);
            }
            if self.is_readonly_symbol(local_or_export_symbol) {
                let display = self.symbol_display_name(symbol);
                if local_flags.intersects(SymbolFlags::VARIABLE) {
                    self.error_at(
                        Some(node),
                        &diagnostics::Cannot_assign_to_0_because_it_is_a_constant,
                        &[&display],
                    );
                } else {
                    self.error_at(
                        Some(node),
                        &diagnostics::Cannot_assign_to_0_because_it_is_a_read_only_property,
                        &[&display],
                    );
                }
                return Ok(self.tables.intrinsics.error);
            }
        }
        let local_flags = self.binder.symbol(local_or_export_symbol).flags;
        let is_alias = local_flags.intersects(SymbolFlags::ALIAS);
        if local_flags.intersects(SymbolFlags::VARIABLE) {
            if assignment_kind == AssignmentKind::Definite {
                return Ok(if self.is_in_compound_like_assignment(node) {
                    self.get_base_type_of_literal_type(ty)?
                } else {
                    ty
                });
            }
        } else if is_alias {
            declaration = self.get_declaration_of_alias_symbol(symbol);
        } else {
            return Ok(ty);
        }
        let Some(declaration) = declaration else {
            return Ok(ty);
        };
        ty = self.get_narrowable_type_for_reference(ty, node, check_mode)?;
        let source = self.binder.source_of_node(declaration);
        let is_parameter = self.kind_of(node_util::get_root_declaration(source, declaration))
            == SyntaxKind::Parameter;
        let declaration_container = self.get_control_flow_container(declaration);
        let flow_container = self.get_control_flow_container(node);
        let is_outer_variable = flow_container != declaration_container;
        // [FLOW M5] the widening loop (72193) walking flowContainer
        // outward for captured const/past-last-assignment locals is
        // stubbed out — it only changes which container the flow
        // analysis starts from, meaningless until M5 narrows.
        let is_spread_destructuring_assignment_target = {
            let parent = self.parent_of(node);
            let grand = parent.and_then(|parent| self.parent_of(parent));
            parent.is_some_and(|parent| {
                self.kind_of(parent) == SyntaxKind::SpreadAssignment
            }) && grand.is_some_and(|grand| self.is_destructuring_assignment_target(grand))
        };
        let is_module_exports = self
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::MODULE_EXPORTS);
        let type_is_automatic =
            ty == self.tables.intrinsics.auto || self.is_auto_array_type(ty);
        let is_automatic_type_in_non_null = type_is_automatic
            && self.parent_of(node).is_some_and(|parent| {
                self.kind_of(parent) == SyntaxKind::NonNullExpression
            });
        let is_never_initialized = immediate_declaration.is_some_and(|decl| {
            if self.kind_of(decl) != SyntaxKind::VariableDeclaration {
                return false;
            }
            let NodeData::VariableDeclaration(data) = self.data_of(decl) else {
                return false;
            };
            let for_in_or_of = self
                .parent_of(decl)
                .and_then(|list| self.parent_of(list))
                .is_some_and(|grand| {
                    matches!(
                        self.kind_of(grand),
                        SyntaxKind::ForInStatement | SyntaxKind::ForOfStatement
                    )
                });
            !for_in_or_of
                && data.initializer.is_none()
                && data.exclamation_token.is_none()
                && self.is_mutable_local_variable_declaration(decl)
                && !self.is_symbol_assigned_definitely_stub(symbol)
        });
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        let assume_initialized = is_parameter
            || is_alias
            || (is_outer_variable && !is_never_initialized)
            || is_spread_destructuring_assignment_target
            || is_module_exports
            || self.is_same_scoped_binding_element(node, declaration)
            || (!type_is_automatic
                && (!strict_null_checks
                    || self
                        .tables
                        .flags_of(ty)
                        .intersects(TypeFlags::ANY_OR_UNKNOWN | TypeFlags::VOID)
                    || self.is_in_type_query(node)
                    || self.is_in_ambient_or_type_node(node)
                    || self.parent_of(node).is_some_and(|parent| {
                        self.kind_of(parent) == SyntaxKind::ExportSpecifier
                    })))
            || self.parent_of(node).is_some_and(|parent| {
                self.kind_of(parent) == SyntaxKind::NonNullExpression
            })
            || (self.kind_of(declaration) == SyntaxKind::VariableDeclaration
                && matches!(
                    self.data_of(declaration),
                    NodeData::VariableDeclaration(data) if data.exclamation_token.is_some()
                ))
            || self.node_flags(declaration) & tsrs2_types::NodeFlags::AMBIENT.bits() != 0;
        // [FACTS 5.5d] initialType (72198): dead under the [FLOW] stub
        // (getFlowTypeOfReference ignores `initial`); the verbatim
        // removeOptionalityFromDeclaredType/getOptionalType ladder
        // rides with the facts classifier.
        let flow_type = if is_automatic_type_in_non_null {
            // getNonNullableType of the stubbed flow type — the [FACTS]
            // strip; auto is unreachable until 5.6 evolving types, so
            // this arm cannot fire yet.
            unreachable!("autoType has no producer until 5.6 evolving types")
        } else {
            self.get_flow_type_of_reference_stub(node, ty, ty, flow_container)
        };
        if type_is_automatic {
            // The 7034/7005 auto-type arm: no producer assigns
            // auto/autoArrayType to a symbol until 5.6.
            unreachable!("autoType has no producer until 5.6 evolving types");
        } else if !assume_initialized
            && !self.contains_undefined_type(ty)
            && self.contains_undefined_type(flow_type)
        {
            // 2454 — self-deactivated under the [FLOW] stub
            // (flowType == type); M5 activates.
            let display = self.symbol_display_name(symbol);
            self.error_at(
                Some(node),
                &diagnostics::Variable_0_is_used_before_being_assigned,
                &[&display],
            );
            return Ok(ty);
        }
        Ok(if assignment_kind != AssignmentKind::None {
            self.get_base_type_of_literal_type(flow_type)?
        } else {
            flow_type
        })
    }

    /// THE M5 SEAM — tsc getFlowTypeOfReference (70394). M5 replaces
    /// exactly this function; every 5.5 call site threads (reference,
    /// declared, initial, flowContainer) so the swap is local.
    pub(crate) fn get_flow_type_of_reference_stub(
        &mut self,
        _reference: NodeId,
        declared: TypeId,
        _initial: TypeId,
        _flow_container: Option<NodeId>,
    ) -> TypeId {
        declared
    }

    /// [FLOW M5] isSymbolAssignedDefinitely (71644): needs the
    /// assignment-marking pass (symbol.lastAssignmentPos); with no
    /// marking, lastAssignmentPos is never set and the answer is the
    /// unmarked default.
    pub(crate) fn is_symbol_assigned_definitely_stub(&self, _symbol: SymbolId) -> bool {
        false
    }

    /// tsc-port: checkIdentifierCalculateNodeCheckFlags @6.0.3
    /// tsc-hash: 4950eecd5ccf25b955631bc3837bfda9ab85d640abbc29f84414e7a17115a3c8
    /// tsc-span: _tsc.js:72063-72125
    ///
    /// The deprecation-suggestion block (resolveAliasWithDeprecationCheck
    /// → addDeprecatedSuggestion) is suggestion-band — elided; its alias
    /// resolution has no other effect here.
    fn check_identifier_calculate_node_check_flags(&mut self, node: NodeId, symbol: SymbolId) {
        if self.is_this_in_type_query(node) {
            return;
        }
        if symbol == self.arguments_symbol {
            if self.is_in_property_initializer_or_class_static_block(node, true) {
                self.error_at(
                    Some(node),
                    &diagnostics::arguments_cannot_be_referenced_in_property_initializers_or_class_static_initialization_blocks,
                    &[],
                );
                return;
            }
            let mut container = self.get_containing_function(node);
            if let Some(first_container) = container {
                if self.options.emit_script_target() < ScriptTarget::ES2015 {
                    if self.kind_of(first_container) == SyntaxKind::ArrowFunction {
                        self.error_at(
                            Some(node),
                            &diagnostics::The_arguments_object_cannot_be_referenced_in_an_arrow_function_in_ES5_Consider_using_a_standard_function_expression,
                            &[],
                        );
                    } else if node_util::has_syntactic_modifier(
                        self.binder.source_of_node(first_container),
                        first_container,
                        ModifierFlags::ASYNC,
                    ) {
                        self.error_at(
                            Some(node),
                            &diagnostics::The_arguments_object_cannot_be_referenced_in_an_async_function_or_method_in_ES5_Consider_using_a_standard_function_or_method,
                            &[],
                        );
                    }
                }
                self.links.or_node_check_flags(
                    self.speculation_depth,
                    first_container,
                    NodeCheckFlags::CAPTURE_ARGUMENTS,
                );
                while let Some(current) = container {
                    if self.kind_of(current) != SyntaxKind::ArrowFunction {
                        break;
                    }
                    container = self.get_containing_function(current);
                    if let Some(outer) = container {
                        self.links.or_node_check_flags(
                            self.speculation_depth,
                            outer,
                            NodeCheckFlags::CAPTURE_ARGUMENTS,
                        );
                    }
                }
            }
            return;
        }
        let local_or_export_symbol = self.get_export_symbol_of_value_symbol_if_exported(symbol);
        let declaration = self.binder.symbol(local_or_export_symbol).value_declaration;
        if let Some(declaration) = declaration {
            if self
                .binder
                .symbol(local_or_export_symbol)
                .flags
                .intersects(SymbolFlags::CLASS)
            {
                let is_class_like = matches!(
                    self.kind_of(declaration),
                    SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                );
                let declaration_name = node_util::get_name_of_declaration(
                    self.binder.source_of_node(declaration),
                    declaration,
                );
                if is_class_like && declaration_name != Some(node) {
                    let source = self.binder.source_of_node(node);
                    let mut container =
                        node_util::get_this_container(source, node, false);
                    while let Some(current) = container {
                        if self.kind_of(current) == SyntaxKind::SourceFile
                            || self.parent_of(current) == Some(declaration)
                        {
                            break;
                        }
                        container = node_util::get_this_container(
                            self.binder.source_of_node(current),
                            current,
                            false,
                        );
                    }
                    if let Some(current) = container {
                        if self.kind_of(current) != SyntaxKind::SourceFile {
                            self.links.or_node_check_flags(
                                self.speculation_depth,
                                declaration,
                                NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE,
                            );
                            self.links.or_node_check_flags(
                                self.speculation_depth,
                                current,
                                NodeCheckFlags::CONTAINS_CONSTRUCTOR_REFERENCE,
                            );
                            self.links.or_node_check_flags(
                                self.speculation_depth,
                                node,
                                NodeCheckFlags::CONSTRUCTOR_REFERENCE,
                            );
                        }
                    }
                }
            }
        }
        self.check_nested_block_scoped_binding(node, symbol);
    }

    /// tsc-port: checkNestedBlockScopedBinding @6.0.3
    /// tsc-hash: f102e00417a0f87fce613db2ff0aab8819f26bb9955bd6173a39314342f2404c
    /// tsc-span: _tsc.js:72250-72290
    ///
    /// languageVersion >= ES2015 exits immediately; with the default
    /// target (ES2025) the ES5 loop-capture NodeCheckFlags bookkeeping
    /// below the gate only runs for down-level fixtures. The flags are
    /// inert emit-era bookkeeping either way (extraction doc §3).
    fn check_nested_block_scoped_binding(&mut self, node: NodeId, symbol: SymbolId) {
        if self.options.emit_script_target() >= ScriptTarget::ES2015 {
            return;
        }
        let flags = self.binder.symbol(symbol).flags;
        if !flags.intersects(SymbolFlags::BLOCK_SCOPED_VARIABLE | SymbolFlags::CLASS) {
            return;
        }
        let Some(value_declaration) = self.binder.symbol(symbol).value_declaration else {
            return;
        };
        if self.kind_of(value_declaration) == SyntaxKind::SourceFile {
            return;
        }
        if self
            .parent_of(value_declaration)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::CatchClause)
        {
            return;
        }
        let Some(container) = self.get_enclosing_block_scope_container(value_declaration) else {
            return;
        };
        let is_captured =
            self.is_inside_function_or_instance_property_initializer(node, container);
        let enclosing_iteration_statement = self.get_enclosing_iteration_statement(container);
        if let Some(iteration) = enclosing_iteration_statement {
            if is_captured {
                let mut captures_in_loop_body = true;
                if self.kind_of(container) == SyntaxKind::ForStatement {
                    let var_decl_list = self.get_ancestor_of_kind(
                        value_declaration,
                        SyntaxKind::VariableDeclarationList,
                    );
                    if var_decl_list
                        .and_then(|list| self.parent_of(list))
                        .is_some_and(|parent| parent == container)
                    {
                        if let Some(part) = self
                            .get_part_of_for_statement_containing_node(
                                self.parent_of(node).unwrap_or(node),
                                container,
                            )
                        {
                            self.links.or_node_check_flags(
                                self.speculation_depth,
                                part,
                                NodeCheckFlags::CONTAINS_CAPTURED_BLOCK_SCOPE_BINDING,
                            );
                            // links.capturedBlockScopeBindings pushIfUnique:
                            // consumed only by emit (isBindingCapturedByNode)
                            // — the list itself is elided with the emitter.
                            let initializer = match self.data_of(container) {
                                NodeData::ForStatement(data) => data.initializer,
                                _ => None,
                            };
                            if Some(part) == initializer {
                                captures_in_loop_body = false;
                            }
                        }
                    }
                }
                if captures_in_loop_body {
                    self.links.or_node_check_flags(
                        self.speculation_depth,
                        iteration,
                        NodeCheckFlags::LOOP_WITH_CAPTURED_BLOCK_SCOPED_BINDING,
                    );
                }
            }
            if self.kind_of(container) == SyntaxKind::ForStatement {
                let var_decl_list = self
                    .get_ancestor_of_kind(value_declaration, SyntaxKind::VariableDeclarationList);
                if var_decl_list
                    .and_then(|list| self.parent_of(list))
                    .is_some_and(|parent| parent == container)
                    && self.is_assigned_in_body_of_for_statement(node, container)
                {
                    self.links.or_node_check_flags(
                        self.speculation_depth,
                        value_declaration,
                        NodeCheckFlags::NEEDS_LOOP_OUT_PARAMETER,
                    );
                }
            }
            self.links.or_node_check_flags(
                self.speculation_depth,
                value_declaration,
                NodeCheckFlags::BLOCK_SCOPED_BINDING_IN_LOOP,
            );
        }
        if is_captured {
            self.links.or_node_check_flags(
                self.speculation_depth,
                value_declaration,
                NodeCheckFlags::CAPTURED_BLOCK_SCOPED_BINDING,
            );
        }
    }

    /// tsc isInsideFunctionOrInstancePropertyInitializer (72237).
    fn is_inside_function_or_instance_property_initializer(
        &self,
        node: NodeId,
        threshold: NodeId,
    ) -> bool {
        self.find_ancestor(Some(node), |state, n| {
            if n == threshold {
                return Ancestor::Quit;
            }
            if node_util::is_function_like_kind(state.kind_of(n)) {
                return Ancestor::Yes;
            }
            if let Some(parent) = state.parent_of(n) {
                if state.kind_of(parent) == SyntaxKind::PropertyDeclaration
                    && !state.has_static_modifier(parent)
                {
                    if let NodeData::PropertyDeclaration(data) = state.data_of(parent) {
                        if data.initializer == Some(n) {
                            return Ancestor::Yes;
                        }
                    }
                }
            }
            Ancestor::No
        })
        .is_some()
    }

    /// tsc getPartOfForStatementContainingNode (72241).
    fn get_part_of_for_statement_containing_node(
        &self,
        node: NodeId,
        container: NodeId,
    ) -> Option<NodeId> {
        let NodeData::ForStatement(data) = self.data_of(container) else {
            return None;
        };
        let (initializer, condition, incrementor, statement) =
            (data.initializer, data.condition, data.incrementor, data.statement);
        self.find_ancestor(Some(node), |_, n| {
            if n == container {
                Ancestor::Quit
            } else if Some(n) == initializer
                || Some(n) == condition
                || Some(n) == incrementor
                || Some(n) == statement
            {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        })
    }

    /// tsc getEnclosingIterationStatement (72245).
    fn get_enclosing_iteration_statement(&self, node: NodeId) -> Option<NodeId> {
        self.find_ancestor(Some(node), |state, n| {
            let kind = state.kind_of(n);
            let starts_new_lexical_environment = matches!(
                kind,
                SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
                    | SyntaxKind::ArrowFunction
                    | SyntaxKind::MethodDeclaration
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::Constructor
                    | SyntaxKind::ModuleDeclaration
                    | SyntaxKind::SourceFile
            );
            if starts_new_lexical_environment {
                return Ancestor::Quit;
            }
            if matches!(
                kind,
                SyntaxKind::ForStatement
                    | SyntaxKind::ForInStatement
                    | SyntaxKind::ForOfStatement
                    | SyntaxKind::DoStatement
                    | SyntaxKind::WhileStatement
            ) {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        })
    }

    /// tsc isAssignedInBodyOfForStatement (72318).
    fn is_assigned_in_body_of_for_statement(&self, node: NodeId, container: NodeId) -> bool {
        let mut current = node;
        while self
            .parent_of(current)
            .is_some_and(|parent| self.kind_of(parent) == SyntaxKind::ParenthesizedExpression)
        {
            current = self.parent_of(current).expect("checked above");
        }
        let is_assigned = if self.get_assignment_target(current).is_some() {
            // NB tsc uses isAssignmentTarget here, which is broader
            // than the ++/-- test below.
            true
        } else if let Some(parent) = self.parent_of(current) {
            match self.data_of(parent) {
                NodeData::PrefixUnaryExpression(data) => matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ),
                NodeData::PostfixUnaryExpression(data) => matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ),
                _ => false,
            }
        } else {
            false
        };
        if !is_assigned {
            return false;
        }
        let statement = match self.data_of(container) {
            NodeData::ForStatement(data) => data.statement,
            _ => None,
        };
        self.find_ancestor(Some(current), |_, n| {
            if n == container {
                Ancestor::Quit
            } else if Some(n) == statement {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        })
        .is_some()
    }

    /// tsc getAncestor (12654): nearest ancestor (or self) of `kind`.
    fn get_ancestor_of_kind(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(n) = current {
            if self.kind_of(n) == kind {
                return Some(n);
            }
            current = self.parent_of(n);
        }
        None
    }

    /// tsc isInPropertyInitializerOrClassStaticBlock (75388).
    pub(crate) fn is_in_property_initializer_or_class_static_block(
        &self,
        node: NodeId,
        ignore_arrow_functions: bool,
    ) -> bool {
        self.find_ancestor(Some(node), |state, n| match state.kind_of(n) {
            SyntaxKind::PropertyDeclaration | SyntaxKind::ClassStaticBlockDeclaration => {
                Ancestor::Yes
            }
            SyntaxKind::TypeQuery | SyntaxKind::JsxClosingElement => Ancestor::Quit,
            SyntaxKind::ArrowFunction => {
                if ignore_arrow_functions {
                    Ancestor::No
                } else {
                    Ancestor::Quit
                }
            }
            SyntaxKind::Block => {
                let parent_is_non_arrow_function_like = state
                    .parent_of(n)
                    .is_some_and(|parent| {
                        node_util::is_function_like_declaration_kind(state.kind_of(parent))
                            && state.kind_of(parent) != SyntaxKind::ArrowFunction
                    });
                if parent_is_non_arrow_function_like {
                    Ancestor::Quit
                } else {
                    Ancestor::No
                }
            }
            _ => Ancestor::No,
        })
        .is_some()
    }

    /// tsc getContainingFunction (14438): findAncestor(parent,
    /// isFunctionLike).
    pub(crate) fn get_containing_function(&self, node: NodeId) -> Option<NodeId> {
        self.find_ancestor(self.parent_of(node), |state, n| {
            if node_util::is_function_like_kind(state.kind_of(n)) {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        })
    }

    /// tsc getExportSymbolOfValueSymbolIfExported (47707).
    pub(crate) fn get_export_symbol_of_value_symbol_if_exported(
        &self,
        symbol: SymbolId,
    ) -> SymbolId {
        let data = self.binder.symbol(symbol);
        let target = if data.flags.intersects(SymbolFlags::EXPORT_VALUE) {
            data.export_symbol.unwrap_or(symbol)
        } else {
            symbol
        };
        self.get_merged_symbol(target)
    }

    /// tsc-port: getNarrowedTypeOfSymbol @6.0.3 (5.5a stub slice)
    /// tsc-hash: 22f3776b5ae1c8cd1ecef7799b03eb16ccc169f3bfe6b062b5bad3d2bce43ce9
    /// tsc-span: _tsc.js:72001-72062
    ///
    /// Both special arms — the dependent-destructuring union narrowing
    /// (InCheckIdentifier + getFlowTypeOfReference over the pattern)
    /// and the context-sensitive rest-parameter slice — are [FLOW]/M6
    /// machinery reading location.flowNode; the extraction doc stubs
    /// the whole function to plain getTypeOfSymbol until M5.
    fn get_narrowed_type_of_symbol(
        &mut self,
        symbol: SymbolId,
        _location: NodeId,
    ) -> CheckResult2<TypeId> {
        self.get_type_of_symbol(symbol)
    }

    /// tsc-port: getNarrowableTypeForReference @6.0.3
    /// tsc-hash: 08613f8018f28889de94abc11ac1bde0cf82fcae244ea2813d7674cf36969b91
    /// tsc-span: _tsc.js:71640-71646
    ///
    /// isNoInferType is constant-false (NoInfer substitution types are
    /// unconstructible until M8).
    pub(crate) fn get_narrowable_type_for_reference(
        &mut self,
        ty: TypeId,
        reference: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let substitute_constraints = !check_mode.intersects(CheckMode::INFERENTIAL)
            && self.some_type_result(ty, |state, t| state.is_generic_type_with_union_constraint(t))?
            && (self.is_constraint_position(ty, reference)?
                || self.has_contextual_type_with_no_generic_types(reference, check_mode)?);
        if substitute_constraints {
            self.map_type_result(ty, |state, t| state.get_base_constraint_or_type(t))
        } else {
            Ok(ty)
        }
    }

    /// tsc isGenericTypeWithUnionConstraint (71624).
    fn is_generic_type_with_union_constraint(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INTERSECTION) {
            let types = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies payload"),
            };
            for t in types {
                if self.is_generic_type_with_union_constraint(t)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if !flags.intersects(TypeFlags::INSTANTIABLE) {
            return Ok(false);
        }
        let constraint = self.get_base_constraint_or_type(ty)?;
        Ok(self
            .tables
            .flags_of(constraint)
            .intersects(TypeFlags::NULLABLE | TypeFlags::UNION))
    }

    /// tsc-port: isConstraintPosition @6.0.3
    /// tsc-hash: f157bad0ed0eb3a05505b0a87007863fcea00ba1805259d020f9b9563ab0ff9b
    /// tsc-span: _tsc.js:71622-71625
    fn is_constraint_position(&mut self, ty: TypeId, node: NodeId) -> CheckResult2<bool> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(false);
        };
        match self.data_of(parent) {
            NodeData::PropertyAccessExpression(_) | NodeData::QualifiedName(_) => Ok(true),
            NodeData::CallExpression(data) => Ok(data.expression == Some(node)),
            NodeData::NewExpression(data) => Ok(data.expression == Some(node)),
            NodeData::ElementAccessExpression(data) => {
                if data.expression != Some(node) {
                    return Ok(false);
                }
                let Some(argument) = data.argument_expression else {
                    return Ok(true);
                };
                let non_nullable_generic = self.some_type_result(ty, |state, t| {
                    state.is_generic_type_without_nullable_constraint(t)
                })?;
                if !non_nullable_generic {
                    return Ok(true);
                }
                let argument_type = self.get_type_of_expression(argument)?;
                Ok(!self.tables.is_generic_index_type(argument_type))
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: isGenericTypeWithoutNullableConstraint @6.0.3
    /// tsc-hash: 5e9566c70397e3995a0a17e8a59433d2ccc586d686371177dc4dc6279017cb50
    /// tsc-span: _tsc.js:71629-71631
    fn is_generic_type_without_nullable_constraint(&mut self, ty: TypeId) -> CheckResult2<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::INTERSECTION) {
            let types = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies payload"),
            };
            for t in types {
                if self.is_generic_type_without_nullable_constraint(t)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if !flags.intersects(TypeFlags::INSTANTIABLE) {
            return Ok(false);
        }
        let constraint = self.get_base_constraint_or_type(ty)?;
        Ok(!self.maybe_type_of_kind(constraint, TypeFlags::NULLABLE))
    }

    /// tsc-port: hasContextualTypeWithNoGenericTypes @6.0.3
    /// tsc-hash: 738c9447519370ba32d416ca51dd945895c9b2af11f53fe73d2e215b04dab1f2
    /// tsc-span: _tsc.js:71632-71641
    fn has_contextual_type_with_no_generic_types(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<bool> {
        if !matches!(
            self.kind_of(node),
            SyntaxKind::Identifier
                | SyntaxKind::PropertyAccessExpression
                | SyntaxKind::ElementAccessExpression
        ) {
            return Ok(false);
        }
        if let Some(parent) = self.parent_of(node) {
            let tag_name = match self.data_of(parent) {
                NodeData::JsxOpeningElement(data) => data.tag_name,
                NodeData::JsxSelfClosingElement(data) => data.tag_name,
                _ => None,
            };
            if tag_name == Some(node) {
                return Ok(false);
            }
        }
        let contextual_type = if check_mode.intersects(CheckMode::REST_BINDING_ELEMENT) {
            self.get_contextual_type(node, tsrs2_types::ContextFlags::SKIP_BINDING_PATTERNS)?
        } else {
            self.get_contextual_type(node, tsrs2_types::ContextFlags::NONE)?
        };
        Ok(contextual_type.is_some_and(|t| !self.tables.is_generic_type(t)))
    }

    /// someType (66550) with a fallible predicate (the constraints.rs
    /// twin takes an infallible one).
    pub(crate) fn some_type_result(
        &mut self,
        ty: TypeId,
        mut predicate: impl FnMut(&mut Self, TypeId) -> CheckResult2<bool>,
    ) -> CheckResult2<bool> {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let types = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types.to_vec(),
                _ => unreachable!("union flag implies payload"),
            };
            for t in types {
                if predicate(self, t)? {
                    return Ok(true);
                }
            }
            Ok(false)
        } else {
            predicate(self, ty)
        }
    }

    /// mapType (70036) with a fallible mapper — the no-alias, no-
    /// distribution-over-union-of-unions form checkIdentifier needs
    /// (getBaseConstraintOrType never returns a union-of-unions here).
    pub(crate) fn map_type_result(
        &mut self,
        ty: TypeId,
        mut mapper: impl FnMut(&mut Self, TypeId) -> CheckResult2<TypeId>,
    ) -> CheckResult2<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::NEVER) {
            return Ok(ty);
        }
        if !self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            return mapper(self, ty);
        }
        let types = match &self.tables.type_of(ty).data {
            TypeData::Union { types, .. } => types.to_vec(),
            _ => unreachable!("union flag implies payload"),
        };
        let mut mapped = Vec::with_capacity(types.len());
        let mut changed = false;
        for t in &types {
            let result = mapper(self, *t)?;
            changed |= result != *t;
            mapped.push(result);
        }
        if !changed {
            return Ok(ty);
        }
        self.get_union_type_ex(&mapped, tsrs2_types::UnionReduction::Literal)
    }

    /// tsc getControlFlowContainer (71477).
    pub(crate) fn get_control_flow_container(&self, node: NodeId) -> Option<NodeId> {
        self.find_ancestor(self.parent_of(node), |state, n| {
            let kind = state.kind_of(n);
            let is_container = (node_util::is_function_like_kind(kind)
                && node_util::get_immediately_invoked_function_expression(
                    state.binder.source_of_node(n),
                    n,
                )
                .is_none())
                || kind == SyntaxKind::ModuleBlock
                || kind == SyntaxKind::SourceFile
                || kind == SyntaxKind::PropertyDeclaration;
            if is_container {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        })
    }

    /// tsc isMutableLocalVariableDeclaration (71599).
    fn is_mutable_local_variable_declaration(&self, declaration: NodeId) -> bool {
        let Some(list) = self.parent_of(declaration) else {
            return false;
        };
        if self.node_flags(list) & tsrs2_types::NodeFlags::LET.bits() == 0 {
            return false;
        }
        let source = self.binder.source_of_node(declaration);
        if node_util::get_combined_modifier_flags(source, declaration)
            .intersects(ModifierFlags::EXPORT)
        {
            return false;
        }
        if let Some(statement) = self.parent_of(list) {
            if self.kind_of(statement) == SyntaxKind::VariableStatement {
                if let Some(container) = self.parent_of(statement) {
                    if self.kind_of(container) == SyntaxKind::SourceFile
                        && !self.binder.is_external_or_common_js_module_of_node(container)
                    {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// tsc isSameScopedBindingElement (72218).
    fn is_same_scoped_binding_element(&self, node: NodeId, declaration: NodeId) -> bool {
        if self.kind_of(declaration) != SyntaxKind::BindingElement {
            return false;
        }
        let binding_element = self.find_ancestor(Some(node), |state, n| {
            if state.kind_of(n) == SyntaxKind::BindingElement {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        });
        let Some(binding_element) = binding_element else {
            return false;
        };
        let source = self.binder.source_of_node(binding_element);
        let declaration_source = self.binder.source_of_node(declaration);
        node_util::get_root_declaration(source, binding_element)
            == node_util::get_root_declaration(declaration_source, declaration)
    }

    /// tsc getDeclarationOfAliasSymbol (48500): findLast over the
    /// declarations with isAliasSymbolDeclaration. The JS arms
    /// (assignment-declaration/require shapes) are the M2 3.4c residual
    /// — constant-false in TS files.
    fn get_declaration_of_alias_symbol(&self, symbol: SymbolId) -> Option<NodeId> {
        let declarations = self.binder.symbol(symbol).declarations.clone();
        declarations
            .into_iter()
            .rev()
            .find(|&declaration| self.is_alias_symbol_declaration(declaration))
    }

    /// tsc isAliasSymbolDeclaration (48503), TS arms.
    fn is_alias_symbol_declaration(&self, node: NodeId) -> bool {
        match self.kind_of(node) {
            SyntaxKind::ImportEqualsDeclaration
            | SyntaxKind::NamespaceExportDeclaration
            | SyntaxKind::NamespaceImport
            | SyntaxKind::NamespaceExport
            | SyntaxKind::ImportSpecifier
            | SyntaxKind::ExportSpecifier => true,
            SyntaxKind::ImportClause => match self.data_of(node) {
                NodeData::ImportClause(data) => data.name.is_some(),
                _ => false,
            },
            SyntaxKind::ExportAssignment => {
                // exportAssignmentIsAlias: the expression is an alias-
                // able entity name / class / function expression.
                match self.data_of(node) {
                    NodeData::ExportAssignment(data) => data.expression.is_some_and(|expression| {
                        self.is_entity_name_expression(expression)
                            || matches!(
                                self.kind_of(expression),
                                SyntaxKind::ClassExpression | SyntaxKind::FunctionExpression
                            )
                    }),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    // ---- assignment-target classification ----

    /// tsc getAssignmentTarget (15536): the enclosing assignment-like
    /// node when `node` is (part of) its target.
    pub(crate) fn get_assignment_target(&self, node: NodeId) -> Option<NodeId> {
        let mut node = node;
        let mut parent = self.parent_of(node)?;
        loop {
            match self.data_of(parent) {
                NodeData::BinaryExpression(data) => {
                    let operator = data
                        .operator_token
                        .map(|token| self.kind_of(token))
                        .unwrap_or(SyntaxKind::Unknown);
                    return (node_util::is_assignment_operator(operator)
                        && data.left == Some(node))
                    .then_some(parent);
                }
                NodeData::PrefixUnaryExpression(data) => {
                    return matches!(
                        data.operator,
                        SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                    )
                    .then_some(parent);
                }
                NodeData::PostfixUnaryExpression(data) => {
                    return matches!(
                        data.operator,
                        SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                    )
                    .then_some(parent);
                }
                NodeData::ForInStatement(data) => {
                    return (data.initializer == Some(node)).then_some(parent);
                }
                NodeData::ForOfStatement(data) => {
                    return (data.initializer == Some(node)).then_some(parent);
                }
                NodeData::ParenthesizedExpression(_)
                | NodeData::ArrayLiteralExpression(_)
                | NodeData::SpreadElement(_)
                | NodeData::NonNullExpression(_) => {
                    node = parent;
                }
                NodeData::SpreadAssignment(_) => {
                    node = self.parent_of(parent)?;
                }
                NodeData::ShorthandPropertyAssignment(data) => {
                    if data.name != Some(node) {
                        return None;
                    }
                    node = self.parent_of(parent)?;
                }
                NodeData::PropertyAssignment(data) => {
                    if data.name == Some(node) {
                        return None;
                    }
                    node = self.parent_of(parent)?;
                }
                _ => return None,
            }
            parent = self.parent_of(node)?;
        }
    }

    /// tsc getAssignmentTargetKind (15580).
    pub(crate) fn get_assignment_target_kind(&self, node: NodeId) -> AssignmentKind {
        let Some(target) = self.get_assignment_target(node) else {
            return AssignmentKind::None;
        };
        match self.data_of(target) {
            NodeData::BinaryExpression(data) => {
                let operator = data
                    .operator_token
                    .map(|token| self.kind_of(token))
                    .unwrap_or(SyntaxKind::Unknown);
                if operator == SyntaxKind::EqualsToken
                    || node_util::is_logical_or_coalescing_assignment_operator(operator)
                {
                    AssignmentKind::Definite
                } else {
                    AssignmentKind::Compound
                }
            }
            NodeData::PrefixUnaryExpression(_) | NodeData::PostfixUnaryExpression(_) => {
                AssignmentKind::Compound
            }
            NodeData::ForInStatement(_) | NodeData::ForOfStatement(_) => AssignmentKind::Definite,
            _ => unreachable!("getAssignmentTarget returns assignment-like nodes only"),
        }
    }

    /// tsc isDestructuringAssignmentTarget (76226-adjacent usage site):
    /// `parent.parent` participates in a destructuring assignment.
    fn is_destructuring_assignment_target(&self, parent: NodeId) -> bool {
        let Some(grand) = self.parent_of(parent) else {
            return false;
        };
        match self.data_of(grand) {
            NodeData::BinaryExpression(data) => data.left == Some(parent),
            NodeData::ForOfStatement(data) => data.initializer == Some(parent),
            _ => false,
        }
    }

    /// tsc isCompoundLikeAssignment + isInCompoundLikeAssignment
    /// (15600-15611).
    fn is_in_compound_like_assignment(&self, node: NodeId) -> bool {
        let Some(target) = self.get_assignment_target(node) else {
            return false;
        };
        let NodeData::BinaryExpression(data) = self.data_of(target) else {
            return false;
        };
        let operator = data
            .operator_token
            .map(|token| self.kind_of(token))
            .unwrap_or(SyntaxKind::Unknown);
        if operator != SyntaxKind::EqualsToken {
            return false;
        }
        let Some(right) = data.right else {
            return false;
        };
        let source = self.binder.source_of_node(target);
        let right = node_util::skip_parentheses_pub(source, right);
        let NodeData::BinaryExpression(right_data) = self.data_of(right) else {
            return false;
        };
        let right_operator = right_data
            .operator_token
            .map(|token| self.kind_of(token))
            .unwrap_or(SyntaxKind::Unknown);
        is_shift_operator_or_higher(right_operator)
    }

    /// tsc containsUndefinedType (64663).
    pub(crate) fn contains_undefined_type(&self, ty: TypeId) -> bool {
        let candidate = if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } => types[0],
                _ => unreachable!("union flag implies payload"),
            }
        } else {
            ty
        };
        self.tables
            .flags_of(candidate)
            .intersects(TypeFlags::UNDEFINED)
    }

    /// globals.rs auto_array_type is lazily minted; identity-test
    /// without forcing it (an unminted auto-array cannot equal `ty`).
    fn is_auto_array_type(&self, _ty: TypeId) -> bool {
        // autoArrayType has no producer until 5.6 evolving types; the
        // memoized global is never minted by 5.5a paths, so the
        // identity test is constant-false.
        false
    }

    // ---- access kind (isWriteOnlyAccess for getResolvedSymbol) ----

    /// tsc accessKind (17465).
    pub(crate) fn access_kind(&self, node: NodeId) -> AccessKind {
        let Some(parent) = self.parent_of(node) else {
            return AccessKind::Read;
        };
        match self.data_of(parent) {
            NodeData::ParenthesizedExpression(_) => self.access_kind(parent),
            NodeData::PrefixUnaryExpression(data) => {
                if matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ) {
                    AccessKind::ReadWrite
                } else {
                    AccessKind::Read
                }
            }
            NodeData::PostfixUnaryExpression(data) => {
                if matches!(
                    data.operator,
                    SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken
                ) {
                    AccessKind::ReadWrite
                } else {
                    AccessKind::Read
                }
            }
            NodeData::BinaryExpression(data) => {
                let operator = data
                    .operator_token
                    .map(|token| self.kind_of(token))
                    .unwrap_or(SyntaxKind::Unknown);
                if data.left == Some(node) && node_util::is_assignment_operator(operator) {
                    if operator == SyntaxKind::EqualsToken {
                        AccessKind::Write
                    } else {
                        AccessKind::ReadWrite
                    }
                } else {
                    AccessKind::Read
                }
            }
            NodeData::PropertyAccessExpression(data) => {
                if data.name != Some(node) {
                    AccessKind::Read
                } else {
                    self.access_kind(parent)
                }
            }
            NodeData::PropertyAssignment(data) => {
                let parent_access = self
                    .parent_of(parent)
                    .map(|grand| self.access_kind(grand))
                    .unwrap_or(AccessKind::Read);
                if data.name == Some(node) {
                    reverse_access_kind(parent_access)
                } else {
                    parent_access
                }
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                if data.object_assignment_initializer == Some(node) {
                    AccessKind::Read
                } else {
                    self.parent_of(parent)
                        .map(|grand| self.access_kind(grand))
                        .unwrap_or(AccessKind::Read)
                }
            }
            NodeData::ArrayLiteralExpression(_) => self.access_kind(parent),
            NodeData::ForInStatement(data) => {
                if data.initializer == Some(node) {
                    AccessKind::Write
                } else {
                    AccessKind::Read
                }
            }
            NodeData::ForOfStatement(data) => {
                if data.initializer == Some(node) {
                    AccessKind::Write
                } else {
                    AccessKind::Read
                }
            }
            _ => AccessKind::Read,
        }
    }

    /// tsc isWriteOnlyAccess (17459).
    pub(crate) fn is_write_only_access(&self, node: NodeId) -> bool {
        self.access_kind(node) == AccessKind::Write
    }

    // ---- this / super ----

    /// tsc-port: checkThisExpression @6.0.3
    /// tsc-hash: 4bb1d606ce7add8c32a4814bc6aa54920c45065ab7d23907e88804c33da36d27
    /// tsc-span: _tsc.js:72348-72421
    ///
    /// checkThisBeforeSuper's isPostSuperFlowNode is [FLOW] M5 — the
    /// stub answers "past super" so 17009 stays FN (extraction doc §0).
    pub(crate) fn check_this_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let is_node_in_type_query = self.is_in_type_query(node);
        let mut container = get_this_container_full(self, node, true, true)
            .ok_or_else(|| Unsupported::new("this outside any container (parse recovery)"))?;
        let mut captured_by_arrow_function = false;
        let mut this_in_computed_property_name = false;
        if self.kind_of(container) == SyntaxKind::Constructor {
            self.check_this_before_super_stub(node, container);
        }
        loop {
            if self.kind_of(container) == SyntaxKind::ArrowFunction {
                container = get_this_container_full(
                    self,
                    container,
                    false,
                    !this_in_computed_property_name,
                )
                .ok_or_else(|| {
                    Unsupported::new("this container walk escaped the tree (parse recovery)")
                })?;
                captured_by_arrow_function = true;
            }
            if self.kind_of(container) == SyntaxKind::ComputedPropertyName {
                container =
                    get_this_container_full(self, container, !captured_by_arrow_function, false)
                        .ok_or_else(|| {
                            Unsupported::new(
                                "this container walk escaped the tree (parse recovery)",
                            )
                        })?;
                this_in_computed_property_name = true;
                continue;
            }
            break;
        }
        self.check_this_in_static_class_field_initializer_in_decorated_class(node, container);
        if this_in_computed_property_name {
            self.error_at(
                Some(node),
                &diagnostics::this_cannot_be_referenced_in_a_computed_property_name,
                &[],
            );
        } else {
            match self.kind_of(container) {
                SyntaxKind::ModuleDeclaration => {
                    self.error_at(
                        Some(node),
                        &diagnostics::this_cannot_be_referenced_in_a_module_or_namespace_body,
                        &[],
                    );
                }
                SyntaxKind::EnumDeclaration => {
                    self.error_at(
                        Some(node),
                        &diagnostics::this_cannot_be_referenced_in_current_location,
                        &[],
                    );
                }
                _ => {}
            }
        }
        if !is_node_in_type_query
            && captured_by_arrow_function
            && self.options.emit_script_target() < ScriptTarget::ES2015
        {
            self.capture_lexical_this(node, container);
        }
        let ty = self.try_get_this_type_at(node, true, container)?;
        if self
            .options
            .strict_option_value(self.options.no_implicit_this)
        {
            let global_this_type = self.get_type_of_symbol(self.global_this_symbol)?;
            if ty == Some(global_this_type) && captured_by_arrow_function {
                self.error_at(
                    Some(node),
                    &diagnostics::The_containing_arrow_function_captures_the_global_value_of_this,
                    &[],
                );
            } else if ty.is_none() {
                let index = self.error_at(
                    Some(node),
                    &diagnostics::this_implicitly_has_type_any_because_it_does_not_have_a_type_annotation,
                    &[],
                );
                if self.kind_of(container) != SyntaxKind::SourceFile {
                    let outside_this = self.try_get_this_type_at_default(container)?;
                    if outside_this.is_some_and(|outside| outside != global_this_type) {
                        let related = self.create_error(
                            Some(container),
                            &diagnostics::An_outer_value_of_this_is_shadowed_by_this_container,
                            &[],
                        );
                        self.diagnostics[index].related.push(RelatedInfo {
                            file_name: related.file_name,
                            start: related.start,
                            length: related.length,
                            message: related.message,
                        });
                    }
                }
            }
        }
        Ok(ty.unwrap_or(self.tables.intrinsics.any))
    }

    /// tsc-port: tryGetThisTypeAt @6.0.3
    /// tsc-hash: 2cd361c26acb6a3601a6e0b7fa0726104aaea4313c035468241b49e1d53661ab
    /// tsc-span: _tsc.js:72422-72463
    ///
    /// Elisions: the JS arms (getTypeForThisExpressionFromJSDoc,
    /// getClassNameFromPrototypeMethod, isJSConstructor, commonjs
    /// SourceFile arm) per the plain-JS band; the
    /// getContextualThisParameterType fallback is live (5.5b).
    fn try_get_this_type_at(
        &mut self,
        node: NodeId,
        include_global_this: bool,
        container: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        if node_util::is_function_like_kind(self.kind_of(container))
            && (!self.is_in_parameter_initializer_before_containing_function(node)
                || self.get_this_parameter_of_declaration(container).is_some())
        {
            let this_type = match self.get_this_type_of_declaration(container)? {
                Some(this_type) => Some(this_type),
                None => self.get_contextual_this_parameter_type(container)?,
            };
            if let Some(this_type) = this_type {
                return Ok(Some(self.get_flow_type_of_reference_stub(
                    node, this_type, this_type, None,
                )));
            }
        }
        if let Some(parent) = self.parent_of(container) {
            if matches!(
                self.kind_of(parent),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
            ) {
                let symbol = self.node_symbol(parent).ok_or_else(|| {
                    Unsupported::new("class without a bound symbol (parse recovery)")
                })?;
                let symbol = self.get_merged_symbol(symbol);
                let ty = if self.has_static_modifier(container) {
                    self.get_type_of_symbol(symbol)?
                } else {
                    let declared = self.get_declared_type_of_class_or_interface(symbol)?;
                    match &self.tables.type_of(declared).data {
                        TypeData::GenericType { this_type, .. } => *this_type,
                        _ => {
                            return Err(Unsupported::new(
                                "this type of a mid-cycle declared-type shell",
                            ))
                        }
                    }
                };
                return Ok(Some(self.get_flow_type_of_reference_stub(node, ty, ty, None)));
            }
        }
        if self.kind_of(container) == SyntaxKind::SourceFile {
            // commonJsModuleIndicator arm: JS band (no CJS indicator in
            // TS files).
            if self
                .binder
                .is_external_or_common_js_module_of_node(container)
            {
                return Ok(Some(self.tables.intrinsics.undefined));
            }
            if include_global_this {
                return Ok(Some(self.get_type_of_symbol(self.global_this_symbol)?));
            }
        }
        Ok(None)
    }

    /// tryGetThisTypeAt's default-argument form (container defaults to
    /// getThisContainer(node, false, false)).
    fn try_get_this_type_at_default(&mut self, node: NodeId) -> CheckResult2<Option<TypeId>> {
        let Some(container) = get_this_container_full(self, node, false, false) else {
            return Ok(None);
        };
        self.try_get_this_type_at(node, true, container)
    }

    /// tsc getThisTypeOfDeclaration (63160): the declared this-
    /// parameter type of a function-like's signature.
    fn get_this_type_of_declaration(&mut self, declaration: NodeId) -> CheckResult2<Option<TypeId>> {
        let signature = self.get_signature_from_declaration(declaration)?;
        let Some(this_parameter) = self.signature_of(signature).this_parameter else {
            return Ok(None);
        };
        Ok(Some(self.get_type_of_symbol(this_parameter)?))
    }

    /// tsc getThisParameter-ish declaration probe: the first parameter
    /// when it is a `this` identifier.
    fn get_this_parameter_of_declaration(&self, declaration: NodeId) -> Option<NodeId> {
        let parameters = match self.data_of(declaration) {
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
            _ => return None,
        };
        let first = self.nodes_of(parameters).first().copied()?;
        let NodeData::Parameter(data) = self.data_of(first) else {
            return None;
        };
        let name = data.name?;
        self.is_this_identifier(name).then_some(first)
    }

    /// tsc isInParameterInitializerBeforeContainingFunction (73797).
    pub(crate) fn is_in_parameter_initializer_before_containing_function(&self, node: NodeId) -> bool {
        let mut in_binding_initializer = false;
        let mut current = self.parent_of(node);
        while let Some(n) = current {
            if node_util::is_function_like_kind(self.kind_of(n)) {
                return false;
            }
            if self.kind_of(n) == SyntaxKind::Parameter && !in_binding_initializer {
                return true;
            }
            if self.kind_of(n) == SyntaxKind::BindingElement {
                if let NodeData::BindingElement(data) = self.data_of(n) {
                    if data.initializer.is_some() {
                        in_binding_initializer = true;
                    }
                }
            }
            current = self.parent_of(n);
        }
        false
    }

    /// tsc captureLexicalThis (72346).
    fn capture_lexical_this(&mut self, node: NodeId, container: NodeId) {
        self.links.or_node_check_flags(
            self.speculation_depth,
            node,
            NodeCheckFlags::LEXICAL_THIS,
        );
        if matches!(
            self.kind_of(container),
            SyntaxKind::PropertyDeclaration | SyntaxKind::Constructor
        ) {
            if let Some(class_node) = self.parent_of(container) {
                self.links.or_node_check_flags(
                    self.speculation_depth,
                    class_node,
                    NodeCheckFlags::CAPTURE_THIS,
                );
            }
        } else {
            self.links.or_node_check_flags(
                self.speculation_depth,
                container,
                NodeCheckFlags::CAPTURE_THIS,
            );
        }
    }

    /// [FLOW M5] checkThisBeforeSuper (72340): needs isPostSuperFlowNode
    /// over the binder's flow graph — 17009/17011 are FN until M5.
    fn check_this_before_super_stub(&mut self, _node: NodeId, _container: NodeId) {}

    /// tsc checkThisInStaticClassFieldInitializerInDecoratedClass
    /// (72344) → 2816. legacyDecorators == experimentalDecorators.
    fn check_this_in_static_class_field_initializer_in_decorated_class(
        &mut self,
        this_expression: NodeId,
        container: NodeId,
    ) {
        if self.kind_of(container) != SyntaxKind::PropertyDeclaration
            || !self.has_static_modifier(container)
            || !self.options.experimental_decorators
        {
            return;
        }
        let NodeData::PropertyDeclaration(data) = self.data_of(container) else {
            return;
        };
        let Some(initializer) = data.initializer else {
            return;
        };
        let source = self.binder.source_of_node(this_expression);
        let this_pos = source.arena.node(this_expression).pos;
        let initializer_range = {
            let source = self.binder.source_of_node(initializer);
            let node = source.arena.node(initializer);
            (node.pos, node.end)
        };
        let contains = initializer_range.0 <= this_pos && this_pos <= initializer_range.1;
        let parent_has_decorators = self.parent_of(container).is_some_and(|class| {
            node_util::modifiers_of(self.binder.source_of_node(class), class)
                .map(|modifiers| {
                    self.nodes_of(Some(modifiers))
                        .iter()
                        .any(|&modifier| self.kind_of(modifier) == SyntaxKind::Decorator)
                })
                .unwrap_or(false)
        });
        if contains && parent_has_decorators {
            self.error_at(
                Some(this_expression),
                &diagnostics::Cannot_use_this_in_a_static_property_initializer_of_a_decorated_class,
                &[],
            );
        }
    }

    /// tsc classDeclarationExtendsNull (72336).
    fn class_declaration_extends_null(&mut self, class_declaration: NodeId) -> CheckResult2<bool> {
        let symbol = self.node_symbol(class_declaration).ok_or_else(|| {
            Unsupported::new("class without a bound symbol (parse recovery)")
        })?;
        let symbol = self.get_merged_symbol(symbol);
        let class_instance_type = self.get_declared_type_of_class_or_interface(symbol)?;
        let base_constructor_type =
            self.get_base_constructor_type_of_class(class_instance_type)?;
        Ok(base_constructor_type == self.tables.intrinsics.null_widening)
    }

    /// tsc getClassExtendsHeritageElement (14700-adjacent): the first
    /// extends-clause type node of a class-like declaration.
    fn get_class_extends_heritage_element(&self, node: NodeId) -> Option<NodeId> {
        let heritage = match self.data_of(node) {
            NodeData::ClassDeclaration(data) => data.heritage_clauses,
            NodeData::ClassExpression(data) => data.heritage_clauses,
            _ => return None,
        };
        for clause in self.nodes_of(heritage) {
            if self.heritage_clause_is_extends(clause) {
                let NodeData::HeritageClause(data) = self.data_of(clause) else {
                    continue;
                };
                return self.nodes_of(data.types).first().copied();
            }
        }
        None
    }

    /// tsc-port: checkSuperExpression @6.0.3
    /// tsc-hash: 1ae95b07cee37ec0b0386cf047154d3231182898c2cd277558053b83c446f5be
    /// tsc-span: _tsc.js:72509-72611
    ///
    /// checkThisBeforeSuper (17011) rides the [FLOW] M5 stub. The
    /// NodeCheckFlags writes happen BEFORE the later error returns —
    /// tsc order preserved.
    pub(crate) fn check_super_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let is_call_expression = self.parent_of(node).is_some_and(|parent| {
            matches!(self.data_of(parent), NodeData::CallExpression(data)
                if data.expression == Some(node))
        });
        let immediate_container = self.get_super_container(node, true);
        let mut container = immediate_container;
        let mut need_to_capture_lexical_this = false;
        let mut in_async_function = false;
        if !is_call_expression {
            while let Some(current) = container {
                if self.kind_of(current) != SyntaxKind::ArrowFunction {
                    break;
                }
                if node_util::has_syntactic_modifier(
                    self.binder.source_of_node(current),
                    current,
                    ModifierFlags::ASYNC,
                ) {
                    in_async_function = true;
                }
                container = self.get_super_container(current, true);
                need_to_capture_lexical_this =
                    self.options.emit_script_target() < ScriptTarget::ES2015;
            }
            if let Some(current) = container {
                if node_util::has_syntactic_modifier(
                    self.binder.source_of_node(current),
                    current,
                    ModifierFlags::ASYNC,
                ) {
                    in_async_function = true;
                }
            }
        }
        let legal = container
            .is_some_and(|container| self.is_legal_usage_of_super_expression(container, is_call_expression));
        if !legal {
            let current = self.find_ancestor(Some(node), |state, n| {
                if Some(n) == container {
                    Ancestor::Quit
                } else if state.kind_of(n) == SyntaxKind::ComputedPropertyName {
                    Ancestor::Yes
                } else {
                    Ancestor::No
                }
            });
            if current.is_some() {
                self.error_at(
                    Some(node),
                    &diagnostics::super_cannot_be_referenced_in_a_computed_property_name,
                    &[],
                );
            } else if is_call_expression {
                self.error_at(
                    Some(node),
                    &diagnostics::Super_calls_are_not_permitted_outside_constructors_or_in_nested_functions_inside_constructors,
                    &[],
                );
            } else if !container.is_some_and(|container| {
                self.parent_of(container).is_some_and(|parent| {
                    matches!(
                        self.kind_of(parent),
                        SyntaxKind::ClassDeclaration
                            | SyntaxKind::ClassExpression
                            | SyntaxKind::ObjectLiteralExpression
                    )
                })
            }) {
                self.error_at(
                    Some(node),
                    &diagnostics::super_can_only_be_referenced_in_members_of_derived_classes_or_object_literal_expressions,
                    &[],
                );
            } else {
                self.error_at(
                    Some(node),
                    &diagnostics::super_property_access_is_permitted_only_in_a_constructor_member_function_or_member_accessor_of_a_derived_class,
                    &[],
                );
            }
            return Ok(self.tables.intrinsics.error);
        }
        let container = container.expect("legality implies a container");
        let immediate_container = immediate_container.expect("legality implies a container");
        if !is_call_expression && self.kind_of(immediate_container) == SyntaxKind::Constructor {
            self.check_this_before_super_stub(node, container);
        }
        let node_check_flag = if self.has_static_modifier(container) || is_call_expression {
            // The ES2015..ES2021 static-initializer
            // ContainsSuperPropertyInStaticInitializer walk.
            if !is_call_expression {
                let language_version = self.options.emit_script_target();
                if language_version >= ScriptTarget::ES2015
                    && language_version <= ScriptTarget::ES2021
                    && (self.kind_of(container) == SyntaxKind::PropertyDeclaration
                        || self.kind_of(container) == SyntaxKind::ClassStaticBlockDeclaration)
                {
                    let mut scope = self.parent_of(node);
                    while let Some(current) = scope {
                        let current_scope =
                            self.get_enclosing_block_scope_container(current);
                        let Some(current_scope) = current_scope else { break };
                        if self.kind_of(current_scope) != SyntaxKind::SourceFile
                            || self
                                .binder
                                .is_external_or_common_js_module_of_node(current_scope)
                        {
                            self.links.or_node_check_flags(
                                self.speculation_depth,
                                current_scope,
                                NodeCheckFlags::CONTAINS_SUPER_PROPERTY_IN_STATIC_INITIALIZER,
                            );
                        }
                        scope = self.parent_of(current_scope);
                    }
                }
            }
            NodeCheckFlags::SUPER_STATIC
        } else {
            NodeCheckFlags::SUPER_INSTANCE
        };
        self.links
            .or_node_check_flags(self.speculation_depth, node, node_check_flag);
        if self.kind_of(container) == SyntaxKind::MethodDeclaration && in_async_function {
            let parent_is_super_property_assignment =
                self.parent_of(node).is_some_and(|parent| {
                    let is_super_property = matches!(
                        self.data_of(parent),
                        NodeData::PropertyAccessExpression(data)
                            if data.expression == Some(node)
                    ) || matches!(
                        self.data_of(parent),
                        NodeData::ElementAccessExpression(data)
                            if data.expression == Some(node)
                    );
                    is_super_property && self.get_assignment_target(parent).is_some()
                });
            let flag = if parent_is_super_property_assignment {
                NodeCheckFlags::METHOD_WITH_SUPER_PROPERTY_ASSIGNMENT_IN_ASYNC
            } else {
                NodeCheckFlags::METHOD_WITH_SUPER_PROPERTY_ACCESS_IN_ASYNC
            };
            self.links
                .or_node_check_flags(self.speculation_depth, container, flag);
        }
        if need_to_capture_lexical_this {
            let parent = self.parent_of(node).expect("super has a parent");
            self.capture_lexical_this(parent, container);
        }
        let container_parent = self
            .parent_of(container)
            .expect("legality implies a class/object-literal parent");
        if self.kind_of(container_parent) == SyntaxKind::ObjectLiteralExpression {
            if self.options.emit_script_target() < ScriptTarget::ES2015 {
                self.error_at(
                    Some(node),
                    &diagnostics::super_is_only_allowed_in_members_of_object_literal_expressions_when_option_target_is_ES2015_or_higher,
                    &[],
                );
                return Ok(self.tables.intrinsics.error);
            }
            return Ok(self.tables.intrinsics.any);
        }
        let class_like_declaration = container_parent;
        if self
            .get_class_extends_heritage_element(class_like_declaration)
            .is_none()
        {
            self.error_at(
                Some(node),
                &diagnostics::super_can_only_be_referenced_in_a_derived_class,
                &[],
            );
            return Ok(self.tables.intrinsics.error);
        }
        if self.class_declaration_extends_null(class_like_declaration)? {
            return Ok(if is_call_expression {
                self.tables.intrinsics.error
            } else {
                self.tables.intrinsics.null_widening
            });
        }
        let class_symbol = self
            .node_symbol(class_like_declaration)
            .ok_or_else(|| Unsupported::new("class without a bound symbol (parse recovery)"))?;
        let class_symbol = self.get_merged_symbol(class_symbol);
        let class_type = self.get_declared_type_of_class_or_interface(class_symbol)?;
        let base_types = self.get_base_types(class_type)?;
        let Some(&base_class_type) = base_types.first() else {
            return Ok(self.tables.intrinsics.error);
        };
        if self.kind_of(container) == SyntaxKind::Constructor
            && self.is_in_constructor_argument_initializer(node, container)
        {
            self.error_at(
                Some(node),
                &diagnostics::super_cannot_be_referenced_in_constructor_arguments,
                &[],
            );
            return Ok(self.tables.intrinsics.error);
        }
        if node_check_flag == NodeCheckFlags::SUPER_STATIC {
            self.get_base_constructor_type_of_class(class_type)
        } else {
            let this_type = match &self.tables.type_of(class_type).data {
                TypeData::GenericType { this_type, .. } => Some(*this_type),
                _ => None,
            };
            self.get_type_with_this_argument(base_class_type, this_type, false)
        }
    }

    /// tsc isLegalUsageOfSuperExpression (72597, closure).
    fn is_legal_usage_of_super_expression(
        &self,
        container: NodeId,
        is_call_expression: bool,
    ) -> bool {
        if is_call_expression {
            return self.kind_of(container) == SyntaxKind::Constructor;
        }
        let parent_is_class_or_object_literal =
            self.parent_of(container).is_some_and(|parent| {
                matches!(
                    self.kind_of(parent),
                    SyntaxKind::ClassDeclaration
                        | SyntaxKind::ClassExpression
                        | SyntaxKind::ObjectLiteralExpression
                )
            });
        if !parent_is_class_or_object_literal {
            return false;
        }
        if self.has_static_modifier(container) {
            matches!(
                self.kind_of(container),
                SyntaxKind::MethodDeclaration
                    | SyntaxKind::MethodSignature
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::PropertyDeclaration
                    | SyntaxKind::ClassStaticBlockDeclaration
            )
        } else {
            matches!(
                self.kind_of(container),
                SyntaxKind::MethodDeclaration
                    | SyntaxKind::MethodSignature
                    | SyntaxKind::GetAccessor
                    | SyntaxKind::SetAccessor
                    | SyntaxKind::PropertyDeclaration
                    | SyntaxKind::PropertySignature
                    | SyntaxKind::Constructor
            )
        }
    }

    /// tsc getSuperContainer (14559).
    fn get_super_container(&self, node: NodeId, stop_on_functions: bool) -> Option<NodeId> {
        let mut node = node;
        loop {
            node = self.parent_of(node)?;
            match self.kind_of(node) {
                SyntaxKind::ComputedPropertyName => {
                    node = self.parent_of(node)?;
                }
                SyntaxKind::FunctionDeclaration
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ArrowFunction => {
                    if stop_on_functions {
                        return Some(node);
                    }
                }
                SyntaxKind::PropertyDeclaration
                | SyntaxKind::PropertySignature
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::Constructor
                | SyntaxKind::GetAccessor
                | SyntaxKind::SetAccessor
                | SyntaxKind::ClassStaticBlockDeclaration => return Some(node),
                SyntaxKind::Decorator => {
                    let parent = self.parent_of(node)?;
                    if self.kind_of(parent) == SyntaxKind::Parameter {
                        if let Some(grand) = self.parent_of(parent) {
                            if is_class_element_kind(self.kind_of(grand)) {
                                node = grand;
                            }
                        }
                    } else if is_class_element_kind(self.kind_of(parent)) {
                        node = parent;
                    }
                }
                _ => {}
            }
        }
    }

    /// tsc isInConstructorArgumentInitializer (72505).
    fn is_in_constructor_argument_initializer(
        &self,
        node: NodeId,
        constructor_decl: NodeId,
    ) -> bool {
        self.find_ancestor(Some(node), |state, n| {
            if node_util::is_function_like_declaration_kind(state.kind_of(n)) {
                Ancestor::Quit
            } else if state.kind_of(n) == SyntaxKind::Parameter
                && state.parent_of(n) == Some(constructor_decl)
            {
                Ancestor::Yes
            } else {
                Ancestor::No
            }
        })
        .is_some()
    }

    // ---- typeof / void / delete ----

    /// tsc-port: checkTypeOfExpression @6.0.3
    /// tsc-hash: b41e256afa2f300fde4b285ca2d74af5a00128c1b8de59c4dbfb532ba07014ff
    /// tsc-span: _tsc.js:79330-79333
    fn check_type_of_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::TypeOfExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data
            .expression
            .ok_or_else(|| Unsupported::new("typeof without operand (parse recovery)"))?;
        self.check_expression(expression, CheckMode::NORMAL)?;
        Ok(self.typeof_type)
    }

    /// tsc-port: checkVoidExpression @6.0.3
    /// tsc-hash: af5b716bc8ec3c313ff11a944409326de865e42509505c9146a6faea88d1a8e8
    /// tsc-span: _tsc.js:79334-79337
    fn check_void_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        self.check_node_deferred(node);
        Ok(self.tables.intrinsics.undefined_widening)
    }

    /// tsc-port: checkDeleteExpression @6.0.3 (+ checkDeleteExpressionMustBeOptional)
    /// tsc-hash: da8de39c65dbe5aab4eba7b07f889bb425243eacb06038ec7d75b229020de3c6
    /// tsc-span: _tsc.js:79303-79329
    ///
    /// The 2704/2790 tail reads links.resolvedSymbol of the ACCESS
    /// expression — stamped only by the 5.5d property/element workers,
    /// whose arms escape first; the facts-classifier escape below is
    /// therefore unreachable until 5.5d un-stubs them together.
    fn check_delete_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        let NodeData::DeleteExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let expression = data
            .expression
            .ok_or_else(|| Unsupported::new("delete without operand (parse recovery)"))?;
        self.check_expression(expression, CheckMode::NORMAL)?;
        let source = self.binder.source_of_node(expression);
        let expr = node_util::skip_parentheses_pub(source, expression);
        let is_access = matches!(
            self.kind_of(expr),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        );
        if !is_access {
            self.error_at(
                Some(expr),
                &diagnostics::The_operand_of_a_delete_operator_must_be_a_property_reference,
                &[],
            );
            return Ok(self.tables.intrinsics.boolean);
        }
        if let NodeData::PropertyAccessExpression(data) = self.data_of(expr) {
            if data
                .name
                .is_some_and(|name| self.kind_of(name) == SyntaxKind::PrivateIdentifier)
            {
                self.error_at(
                    Some(expr),
                    &diagnostics::The_operand_of_a_delete_operator_cannot_be_a_private_identifier,
                    &[],
                );
            }
        }
        let resolved = self.links.node(expr).resolved_symbol.resolved();
        if let Some(resolved) = resolved {
            let symbol = self.get_export_symbol_of_value_symbol_if_exported(resolved);
            if self.is_readonly_symbol(symbol) {
                self.error_at(
                    Some(expr),
                    &diagnostics::The_operand_of_a_delete_operator_cannot_be_a_read_only_property,
                    &[],
                );
            } else {
                // checkDeleteExpressionMustBeOptional (79325): the 2790
                // strictNullChecks arm reads hasTypeFacts(IsUndefined)
                // — [FACTS] 5.5d, unreachable today (module note).
                return Err(Unsupported::new(
                    "checkDeleteExpressionMustBeOptional (facts classifier, 5.5d)",
                ));
            }
        }
        Ok(self.tables.intrinsics.boolean)
    }
}

// ---- M4 5.5b: the contextual driver band (L80551-80959) ----

impl<'a> CheckerState<'a> {
    /// tsc-port: getContextNode @6.0.3
    /// tsc-hash: 4e869b910490be7520efcdf5dda6d7de54949d285ad0a09fa342806e5ec5cf7e
    /// tsc-span: _tsc.js:80551-80556
    fn get_context_node(&self, node: NodeId) -> NodeId {
        if self.kind_of(node) == SyntaxKind::JsxAttributes {
            let parent = self.parent_of(node).expect("attributes have an element");
            if self.kind_of(parent) != SyntaxKind::JsxSelfClosingElement {
                return self.parent_of(parent).expect("opening element has a parent");
            }
        }
        node
    }

    /// tsc-port: checkExpressionWithContextualType @6.0.3
    /// tsc-hash: ce6d50e4b09ba21d3b8b1caca81f0a8e01957b7acd2195417c37b205adb69cc5
    /// tsc-span: _tsc.js:80557-80579
    ///
    /// 5.5 consumer: checkDeclarationInitializer only (argument
    /// checking is 5.7). The inference-context parameter is None until
    /// M6 (the intraExpressionInferenceSites reset rides with it), so
    /// checkMode never gains Inferential here.
    pub(crate) fn check_expression_with_contextual_type(
        &mut self,
        node: NodeId,
        contextual_type: TypeId,
        inference_context: Option<crate::contextual::InferenceContextPlaceholder>,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let context_node = self.get_context_node(node);
        self.push_contextual_type(context_node, Some(contextual_type), false);
        self.push_inference_context(context_node, inference_context);
        let result = (|state: &mut Self| -> CheckResult2<TypeId> {
            let ty = state.check_expression(node, check_mode | CheckMode::CONTEXTUAL)?;
            if state.maybe_type_of_kind(ty, TypeFlags::LITERAL) {
                let instantiated = state
                    .instantiate_contextual_type_for_node(Some(contextual_type), node)?;
                if state.is_literal_of_contextual_type(ty, instantiated)? {
                    return Ok(state.tables.get_regular_type_of_literal_type(ty));
                }
            }
            Ok(ty)
        })(self);
        self.pop_inference_context();
        self.pop_contextual_type();
        result
    }

    /// tsc-port: checkExpressionCached @6.0.3
    /// tsc-hash: d53b8def69286cea6beb2fdadf985dcd3c6d0dec3ef171f10eda495f50485178
    /// tsc-span: _tsc.js:80580-80595
    ///
    /// The flowLoopStart/flowTypeCache save-reset-restore is the M5
    /// fixpoint shape, wired now (both fields are dormant until M5).
    /// Unlike tsc's unconditional `links.resolvedType = ...`, a
    /// re-entrant inner resolution that already filled the slot wins
    /// the CACHE while this call still returns its own result — the
    /// two computations agree while flow state is dormant.
    pub(crate) fn check_expression_cached(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        if !check_mode.is_empty() {
            return self.check_expression(node, check_mode);
        }
        if let Some(cached) = self.links.node(node).resolved_type.resolved() {
            return Ok(cached);
        }
        let save_flow_loop_start = self.flow_loop_start;
        let save_flow_type_cache = self.flow_type_cache.take();
        self.flow_loop_start = self.flow_loop_count;
        let result = self.check_expression(node, check_mode);
        self.flow_type_cache = save_flow_type_cache;
        self.flow_loop_start = save_flow_loop_start;
        let ty = result?;
        if self.links.node(node).resolved_type.resolved().is_none() {
            self.links.set_node_resolved_type(
                self.speculation_depth,
                node,
                crate::links::LinkSlot::Resolved(ty),
            );
        }
        Ok(ty)
    }

    /// tsc-port: isTypeAssertion @6.0.3
    /// tsc-hash: 666d5c2cff0b5ac6e459a721e3c1251084f05b323b3384bd6d6b2d355d3be53e
    /// tsc-span: _tsc.js:80596-80603
    ///
    /// The JSDoc-type-assertion half is [JSDOC] (invisible — no JSDoc
    /// parse), so both skipParentheses forms collapse to the plain one.
    fn is_type_assertion_expr(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let node = node_util::skip_parentheses_pub(source, node);
        matches!(
            self.kind_of(node),
            SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression
        )
    }

    /// tsc-port: checkDeclarationInitializer @6.0.3
    /// tsc-hash: 140897d2a5fd50d78e8cfdcf90087bec70318e099c79f4d2acef4c49302c902d
    /// tsc-span: _tsc.js:80604-80628
    ///
    /// The JSDoc satisfies arm is [JSDOC]; getEffectiveInitializer's
    /// JS `x || y` unwrapping likewise, so the TS read is the plain
    /// `.initializer`.
    pub(crate) fn check_declaration_initializer(
        &mut self,
        declaration: NodeId,
        check_mode: CheckMode,
        contextual_type: Option<TypeId>,
    ) -> CheckResult2<TypeId> {
        let initializer = self
            .initializer_of(declaration)
            .expect("checkDeclarationInitializer callers guarantee an initializer");
        let ty = match self.get_quick_type_of_expression(initializer)? {
            Some(quick) => quick,
            None => match contextual_type {
                Some(contextual_type) => self.check_expression_with_contextual_type(
                    initializer,
                    contextual_type,
                    /*inference_context*/ None,
                    check_mode,
                )?,
                None => self.check_expression_cached(initializer, check_mode)?,
            },
        };
        let source = self.binder.source_of_node(declaration);
        let walk_target = if self.kind_of(declaration) == SyntaxKind::BindingElement {
            node_util::walk_up_binding_elements_and_patterns(source, declaration)
        } else {
            Some(declaration)
        };
        if walk_target.is_some_and(|t| self.kind_of(t) == SyntaxKind::Parameter) {
            let name = match self.data_of(declaration) {
                NodeData::Parameter(data) => data.name,
                NodeData::BindingElement(data) => data.name,
                NodeData::VariableDeclaration(data) => data.name,
                _ => None,
            };
            if let Some(name) = name {
                if self.kind_of(name) == SyntaxKind::ObjectBindingPattern
                    && self.is_object_literal_type(ty)
                {
                    return self.pad_object_literal_type(ty, name);
                }
                if self.kind_of(name) == SyntaxKind::ArrayBindingPattern
                    && self.tables.is_tuple_type(ty)
                {
                    return self.pad_tuple_type(ty, name);
                }
            }
        }
        Ok(ty)
    }


    /// tsc-port: padObjectLiteralType @6.0.3
    /// tsc-hash: 27d1ecfe9de206dd54dee7b938427800afbe679cccfdb1b16ffd99b872488dff
    /// tsc-span: _tsc.js:80629-80660
    fn pad_object_literal_type(&mut self, ty: TypeId, pattern: NodeId) -> CheckResult2<TypeId> {
        let elements = match self.data_of(pattern) {
            NodeData::ObjectBindingPattern(data) => data.elements,
            _ => None,
        };
        let elements: Vec<NodeId> = self.nodes_of(elements);
        let mut missing_elements: Vec<NodeId> = Vec::new();
        for &e in &elements {
            let has_initializer = matches!(
                self.data_of(e),
                NodeData::BindingElement(data) if data.initializer.is_some()
            );
            if !has_initializer {
                continue;
            }
            if let Some(name) = self.property_name_from_binding_element(e)? {
                if self.get_property_of_type_full(ty, &name)?.is_none() {
                    missing_elements.push(e);
                }
            }
        }
        if missing_elements.is_empty() {
            return Ok(ty);
        }
        let mut members = tsrs2_binder::SymbolTable::default();
        let mut properties: Vec<SymbolId> = Vec::new();
        for prop in self.get_properties_of_object_type_owned(ty)? {
            let name = self.binder.symbol(prop).escaped_name.clone();
            members.insert(name, prop);
            properties.push(prop);
        }
        for e in missing_elements {
            let name = self
                .property_name_from_binding_element(e)?
                .expect("filtered above");
            let symbol = self
                .binder
                .create_symbol(SymbolFlags::PROPERTY | SymbolFlags::OPTIONAL, name.clone());
            let element_type = self.get_type_from_binding_element(
                e,
                /*include_pattern_in_type*/ false,
                /*report_errors*/ false,
            )?;
            self.links.set_symbol_type(
                self.speculation_depth,
                symbol,
                crate::links::LinkSlot::Resolved(element_type),
            );
            members.insert(name, symbol);
            properties.push(symbol);
        }
        let index_infos = self.get_index_infos_of_type(ty)?;
        let symbol = self.tables.type_of(ty).symbol;
        let source_object_flags = self.tables.object_flags_of(ty);
        let id = self
            .tables
            .create_type(TypeFlags::OBJECT, tsrs2_types::TypeData::Object);
        self.tables.type_mut(id).object_flags = source_object_flags;
        self.tables.type_mut(id).symbol = symbol;
        let members_id = self.alloc_members(crate::state::ResolvedMembers {
            members,
            properties,
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_infos,
        });
        self.links.set_type_members(
            self.speculation_depth,
            id,
            crate::links::LinkSlot::Resolved(members_id),
        );
        Ok(id)
    }

    /// tsc-port: getPropertyNameFromBindingElement @6.0.3
    /// tsc-hash: 0384b54e818653d7ffc1e37a749d326641ed2996e361ac13190e37260ff67a66
    /// tsc-span: _tsc.js:80661-80664
    fn property_name_from_binding_element(
        &mut self,
        e: NodeId,
    ) -> CheckResult2<Option<String>> {
        let NodeData::BindingElement(data) = self.data_of(e) else {
            return Ok(None);
        };
        let Some(name) = data.property_name.or(data.name) else {
            return Ok(None);
        };
        let expr_type = self.get_literal_type_from_property_name(name)?;
        Ok(self.property_name_from_type_usable(expr_type))
    }

    /// tsc-port: padTupleType @6.0.3
    /// tsc-hash: 072c6834957ff8855543ae6c258dae1f0ba439e9587350619e9228b3cab1cd21
    /// tsc-span: _tsc.js:80665-80689
    ///
    /// The reportImplicitAny call on defaultless extra elements is
    /// [WIDEN → 5.6] — the anyType padding still happens; the 7006-band
    /// diagnostic is a recorded FN until then.
    fn pad_tuple_type(&mut self, ty: TypeId, pattern: NodeId) -> CheckResult2<TypeId> {
        let target = self.tables.reference_target(ty);
        let (combined_variable, arity, mut element_flags, readonly) =
            match &self.tables.type_of(target).data {
                tsrs2_types::TypeData::TupleTarget(data) => (
                    data.combined_flags
                        .intersects(tsrs2_types::ElementFlags::VARIABLE),
                    data.element_flags.len(),
                    data.element_flags.to_vec(),
                    data.readonly,
                ),
                _ => unreachable!("tuple type has a tuple target"),
            };
        let elements = match self.data_of(pattern) {
            NodeData::ArrayBindingPattern(data) => data.elements,
            _ => None,
        };
        let pattern_elements: Vec<NodeId> = self.nodes_of(elements);
        if combined_variable || arity >= pattern_elements.len() {
            return Ok(ty);
        }
        let mut element_types = self.get_type_arguments(ty)?;
        for i in arity..pattern_elements.len() {
            let e = pattern_elements[i];
            let is_rest_binding = matches!(
                self.data_of(e),
                NodeData::BindingElement(data) if data.dot_dot_dot_token.is_some()
            );
            if i < pattern_elements.len() - 1 || !is_rest_binding {
                let omitted = self.kind_of(e) == SyntaxKind::OmittedExpression;
                let has_default = !omitted && self.has_default_value(e);
                element_types.push(if has_default {
                    self.get_type_from_binding_element(
                        e,
                        /*include_pattern_in_type*/ false,
                        /*report_errors*/ false,
                    )?
                } else {
                    self.tables.intrinsics.any
                });
                element_flags.push(tsrs2_types::ElementFlags::OPTIONAL);
                if !omitted && !has_default {
                    let any = self.tables.intrinsics.any;
                    self.report_implicit_any(e, any, /*widening_kind*/ None)?;
                }
            }
        }
        self.create_tuple_type_forced(&element_types, Some(&element_flags), readonly, None)
    }

    /// tsc-port: hasDefaultValue @6.0.3
    /// tsc-hash: 8010e25693bfa7169cbb17e357fcb9dcfee994840774795a344514971afb439c
    /// tsc-span: _tsc.js:73949-73951
    pub(crate) fn has_default_value(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::BindingElement(data) => data.initializer.is_some(),
            NodeData::PropertyAssignment(data) => {
                data.initializer.is_some_and(|i| self.has_default_value(i))
            }
            NodeData::ShorthandPropertyAssignment(data) => {
                data.object_assignment_initializer.is_some()
            }
            NodeData::BinaryExpression(data) => data
                .operator_token
                .is_some_and(|op| self.kind_of(op) == SyntaxKind::EqualsToken),
            _ => false,
        }
    }

    /// tsc-port: getWidenedLiteralTypeForInitializer @6.0.3
    /// tsc-hash: c9f5c6c4ff4faa5cfff78b116e0f94e15a488c1f99bbe49e75e7026cd72b799e
    /// tsc-span: _tsc.js:80703-80705
    pub(crate) fn get_widened_literal_type_for_initializer(
        &mut self,
        declaration: NodeId,
        ty: TypeId,
    ) -> CheckResult2<TypeId> {
        let source = self.binder.source_of_node(declaration);
        let constant = node_util::get_combined_node_flags(source, declaration).bits()
            & tsrs2_types::NodeFlags::CONSTANT.bits()
            != 0;
        if constant || self.is_declaration_readonly(declaration) {
            Ok(ty)
        } else {
            self.get_widened_literal_type(ty)
        }
    }

    /// tsc isDeclarationReadonly (14128-14130): combined readonly
    /// modifier, excluding parameter properties.
    fn is_declaration_readonly(&self, declaration: NodeId) -> bool {
        let source = self.binder.source_of_node(declaration);
        node_util::has_syntactic_modifier(source, declaration, ModifierFlags::READONLY)
            && !(self.kind_of(declaration) == SyntaxKind::Parameter
                && self
                    .parent_of(declaration)
                    .is_some_and(|p| self.kind_of(p) == SyntaxKind::Constructor))
    }

    /// tsc-port: isLiteralOfContextualType @6.0.3
    /// tsc-hash: 57a645f8ae702bd309c5fcb54f5581f17a7db5cae1a16b366ab92375745ebf4f
    /// tsc-span: _tsc.js:80706-80719
    pub(crate) fn is_literal_of_contextual_type(
        &mut self,
        candidate_type: TypeId,
        contextual_type: Option<TypeId>,
    ) -> CheckResult2<bool> {
        let Some(contextual_type) = contextual_type else {
            return Ok(false);
        };
        let flags = self.tables.flags_of(contextual_type);
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let types = match &self.tables.type_of(contextual_type).data {
                TypeData::Union { types, .. } | TypeData::Intersection { types } => {
                    types.to_vec()
                }
                _ => unreachable!("union/intersection flag implies payload"),
            };
            for t in types {
                if self.is_literal_of_contextual_type(candidate_type, Some(t))? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE) {
            let constraint = self
                .get_base_constraint_of_type(contextual_type)?
                .unwrap_or(self.tables.intrinsics.unknown);
            return Ok((self.maybe_type_of_kind(constraint, TypeFlags::STRING)
                && self.maybe_type_of_kind(candidate_type, TypeFlags::STRING_LITERAL))
                || (self.maybe_type_of_kind(constraint, TypeFlags::NUMBER)
                    && self.maybe_type_of_kind(candidate_type, TypeFlags::NUMBER_LITERAL))
                || (self.maybe_type_of_kind(constraint, TypeFlags::BIG_INT)
                    && self.maybe_type_of_kind(candidate_type, TypeFlags::BIG_INT_LITERAL))
                || (self.maybe_type_of_kind(constraint, TypeFlags::ES_SYMBOL)
                    && self.maybe_type_of_kind(candidate_type, TypeFlags::UNIQUE_ES_SYMBOL))
                || self.is_literal_of_contextual_type(candidate_type, Some(constraint))?);
        }
        Ok((flags.intersects(
            TypeFlags::STRING_LITERAL
                | TypeFlags::INDEX
                | TypeFlags::TEMPLATE_LITERAL
                | TypeFlags::STRING_MAPPING,
        ) && self.maybe_type_of_kind(candidate_type, TypeFlags::STRING_LITERAL))
            || (flags.intersects(TypeFlags::NUMBER_LITERAL)
                && self.maybe_type_of_kind(candidate_type, TypeFlags::NUMBER_LITERAL))
            || (flags.intersects(TypeFlags::BIG_INT_LITERAL)
                && self.maybe_type_of_kind(candidate_type, TypeFlags::BIG_INT_LITERAL))
            || (flags.intersects(TypeFlags::BOOLEAN_LITERAL)
                && self.maybe_type_of_kind(candidate_type, TypeFlags::BOOLEAN_LITERAL))
            || (flags.intersects(TypeFlags::UNIQUE_ES_SYMBOL)
                && self.maybe_type_of_kind(candidate_type, TypeFlags::UNIQUE_ES_SYMBOL)))
    }

    /// tsc-port: isConstContext @6.0.3
    /// tsc-hash: ca0d9cc55a6a71da6f654ad57a0669325df6cca1e15082aa35dc2e8cf89db960
    /// tsc-span: _tsc.js:80720-80723
    ///
    /// The JSDoc-type-assertion disjunct is [JSDOC]; the
    /// isConstTypeVariable disjunct reads the contextual type and so
    /// only fires once const type parameters are constructible.
    pub(crate) fn is_const_context(&mut self, node: NodeId) -> CheckResult2<bool> {
        let Some(parent) = self.parent_of(node) else {
            return Ok(false);
        };
        let assertion_type = match self.data_of(parent) {
            NodeData::AsExpression(data) => data.r#type,
            NodeData::TypeAssertionExpression(data) => data.r#type,
            _ => None,
        };
        if let Some(assertion_type) = assertion_type {
            if self.is_const_type_reference_node(assertion_type) {
                return Ok(true);
            }
        }
        if self.is_valid_const_assertion_argument(node)? {
            let contextual = self.get_contextual_type(node, tsrs2_types::ContextFlags::NONE)?;
            if self.is_const_type_variable(contextual, 0) {
                return Ok(true);
            }
        }
        match self.kind_of(parent) {
            SyntaxKind::ParenthesizedExpression
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::SpreadElement => self.is_const_context(parent),
            SyntaxKind::PropertyAssignment
            | SyntaxKind::ShorthandPropertyAssignment
            | SyntaxKind::TemplateSpan => {
                let grandparent = self.parent_of(parent).expect("member has a container");
                self.is_const_context(grandparent)
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: isValidConstAssertionArgument @6.0.3
    /// tsc-hash: ac9960346501b930bbf39f79520b454dffdd55a38673ecea6208adc031e4fafc
    /// tsc-span: _tsc.js:77877-77906
    pub(crate) fn is_valid_const_assertion_argument(&mut self, node: NodeId) -> CheckResult2<bool> {
        match self.kind_of(node) {
            SyntaxKind::StringLiteral
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::TemplateExpression => Ok(true),
            SyntaxKind::ParenthesizedExpression => match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => match data.expression {
                    Some(expression) => self.is_valid_const_assertion_argument(expression),
                    None => Ok(false),
                },
                _ => Ok(false),
            },
            SyntaxKind::PrefixUnaryExpression => {
                let NodeData::PrefixUnaryExpression(data) = self.data_of(node) else {
                    return Ok(false);
                };
                let Some(operand) = data.operand else {
                    return Ok(false);
                };
                let operand_kind = self.kind_of(operand);
                Ok(match data.operator {
                    SyntaxKind::MinusToken => matches!(
                        operand_kind,
                        SyntaxKind::NumericLiteral | SyntaxKind::BigIntLiteral
                    ),
                    SyntaxKind::PlusToken => operand_kind == SyntaxKind::NumericLiteral,
                    _ => false,
                })
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
                let source = self.binder.source_of_node(node);
                let expr = node_util::skip_parentheses_pub(source, expression);
                let symbol = if self.is_entity_name_expression(expr) {
                    self.resolve_entity_name(
                        expr,
                        SymbolFlags::VALUE,
                        /*ignore_errors*/ true,
                        /*location*/ None,
                    )
                } else {
                    None
                };
                Ok(symbol.is_some_and(|s| {
                    self.symbol_flags(s).intersects(SymbolFlags::ENUM)
                }))
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: isConstTypeVariable @6.0.3
    /// tsc-hash: 93aac203ec4e0f4544652a04da816cd276af64c5b68e7b55573d80139bd3f6cc
    /// tsc-span: _tsc.js:58794-58797
    ///
    /// The Conditional/Mapped arms ride M8 (unconstructible); the
    /// Substitution/IndexedAccess/generic-tuple arms port over the
    /// existing TypeData kinds.
    pub(crate) fn is_const_type_variable(&mut self, ty: Option<TypeId>, depth: u32) -> bool {
        let Some(ty) = ty else {
            return false;
        };
        if depth >= 5 {
            return false;
        }
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::TYPE_PARAMETER) {
            let symbol = self.tables.type_of(ty).symbol;
            return symbol.is_some_and(|s| {
                self.binder.symbol(s).declarations.iter().any(|&d| {
                    node_util::has_syntactic_modifier(
                        self.binder.source_of_node(d),
                        d,
                        ModifierFlags::CONST,
                    )
                })
            });
        }
        if flags.intersects(TypeFlags::UNION_OR_INTERSECTION) {
            let types = match &self.tables.type_of(ty).data {
                TypeData::Union { types, .. } | TypeData::Intersection { types } => {
                    types.to_vec()
                }
                _ => return false,
            };
            return types
                .into_iter()
                .any(|t| self.is_const_type_variable(Some(t), depth));
        }
        if flags.intersects(TypeFlags::INDEXED_ACCESS) {
            let object_type = match &self.tables.type_of(ty).data {
                TypeData::IndexedAccess { object_type, .. } => Some(*object_type),
                _ => None,
            };
            return self.is_const_type_variable(object_type, depth + 1);
        }
        if self.tables.is_tuple_type(ty) {
            let target = self.tables.reference_target(ty);
            let element_flags: Vec<tsrs2_types::ElementFlags> =
                match &self.tables.type_of(target).data {
                    TypeData::TupleTarget(data) => data.element_flags.to_vec(),
                    _ => return false,
                };
            let Ok(type_arguments) = self.get_type_arguments(ty) else {
                return false;
            };
            return type_arguments.iter().enumerate().any(|(i, &t)| {
                element_flags
                    .get(i)
                    .is_some_and(|f| f.intersects(tsrs2_types::ElementFlags::VARIADIC))
                    && self.is_const_type_variable(Some(t), depth)
            });
        }
        false
    }

    /// tsc-port: checkExpressionForMutableLocation @6.0.3
    /// tsc-hash: d85d9ab8a5bee0dadc479cbc618ddf99398666c7c84f75d18023afe56186d6e3
    /// tsc-span: _tsc.js:80724-80736
    ///
    /// isCommonJsExportedExpression is [JSDOC] (JS-only, constant
    /// false in TS). Live consumers: the 5.5c literals band.
    pub(crate) fn check_expression_for_mutable_location(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
        force_tuple: bool,
    ) -> CheckResult2<TypeId> {
        let ty = self.check_expression_with_force_tuple(node, check_mode, force_tuple)?;
        if self.is_const_context(node)? {
            return Ok(self.tables.get_regular_type_of_literal_type(ty));
        }
        if self.is_type_assertion_expr(node) {
            return Ok(ty);
        }
        let contextual = self.get_contextual_type(node, tsrs2_types::ContextFlags::NONE)?;
        let instantiated = self.instantiate_contextual_type_for_node(contextual, node)?;
        self.get_widened_literal_like_type_for_contextual_type(ty, instantiated)
    }

    /// tsc-port: checkPropertyAssignment @6.0.3
    /// tsc-hash: e3a8819d69458527c7bb6cff8a6451b0195b7fc3c63b1c2fae5cb1a41bc7f242
    /// tsc-span: _tsc.js:80737-80742
    pub(crate) fn check_property_assignment(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        let (name, initializer) = match self.data_of(node) {
            NodeData::PropertyAssignment(data) => (data.name, data.initializer),
            _ => (None, None),
        };
        if let Some(name) = name {
            if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                self.check_computed_property_name(name)?;
            }
        }
        let initializer = initializer.ok_or_else(|| {
            Unsupported::new("property assignment without initializer (parse recovery)")
        })?;
        self.check_expression_for_mutable_location(initializer, check_mode, false)
    }

    /// tsc-port: checkObjectLiteralMethod @6.0.3
    /// tsc-hash: 52451fa471d9dde73e875e1db438439f637100b04997f3405fc87170de59b386
    /// tsc-span: _tsc.js:80743-80750
    ///
    /// checkGrammarMethod (89943) is an elided slice (M7 modifier
    /// band). instantiateTypeWithSingleGenericCallSignature already
    /// rides as the checkExpression wrapper's 5.5a gate slice —
    /// invoked here directly, matching tsc's explicit tail call.
    pub(crate) fn check_object_literal_method(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult2<TypeId> {
        // checkGrammarMethod(node): elided slice.
        if let Some(name) = match self.data_of(node) {
            NodeData::MethodDeclaration(data) => data.name,
            _ => None,
        } {
            if self.kind_of(name) == SyntaxKind::ComputedPropertyName {
                self.check_computed_property_name(name)?;
            }
        }
        let uninstantiated =
            self.check_function_expression_or_object_literal_method(node, check_mode)?;
        self.instantiate_type_with_single_generic_call_signature(node, uninstantiated, check_mode)
    }

    /// tsc-port: getReturnTypeOfSingleNonGenericCallSignature @6.0.3
    /// tsc-hash: e303b67bb9b2a56c491ea265d4a9dcec19c2fd5a6b9b4a44f5972612ee007e1d
    /// tsc-span: _tsc.js:80883-80888
    #[allow(dead_code)] // consumer: getQuickTypeOfExpression's call arm (5.5d/5.7)
    fn get_return_type_of_single_non_generic_call_signature(
        &mut self,
        func_type: TypeId,
    ) -> CheckResult2<Option<TypeId>> {
        let Some(signature) = self.get_single_call_signature(func_type)? else {
            return Ok(None);
        };
        if self.signature_of(signature).type_parameters.is_some() {
            return Ok(None);
        }
        self.get_return_type_of_signature(signature).map(Some)
    }

    /// tsc-port: getTypeOfExpression @6.0.3
    /// tsc-hash: bf5088bf74890be8eef71547b6179bb73d0390c2e8eb1719ba83b7f8af0e027f
    /// tsc-span: _tsc.js:80895-80914
    ///
    /// The TypeCached/flowTypeCache fast path and the
    /// flowInvocationCount-gated cache write are M5 shape: the counter
    /// never moves at M4 (no flow analysis), so the write arm is
    /// grep-ably dormant, and the node-flag half of the fast path is
    /// elided — the cache map itself already encodes membership.
    pub(crate) fn get_type_of_expression(&mut self, node: NodeId) -> CheckResult2<TypeId> {
        if let Some(quick) = self.get_quick_type_of_expression(node)? {
            return Ok(quick);
        }
        if let Some(cache) = &self.flow_type_cache {
            if let Some(&cached) = cache.get(&node) {
                return Ok(cached);
            }
        }
        let start_invocation_count = self.flow_invocation_count;
        let ty = self.check_expression(node, CheckMode::TYPE_ONLY)?;
        if self.flow_invocation_count != start_invocation_count {
            let cache = self.flow_type_cache.get_or_insert_with(Default::default);
            cache.insert(node, ty);
        }
        Ok(ty)
    }

    /// tsc-port: getQuickTypeOfExpression @6.0.3
    /// tsc-hash: 12fa64ad550e27bc242ab7af46766acf39032fe3d50d50699bdb2a0a251a5c57
    /// tsc-span: _tsc.js:80915-80944
    ///
    /// The JSDoc-assertion arm is [JSDOC]. The await arm needs
    /// getAwaitedType ([ASYNC → 5.5f]) and the call arm needs
    /// checkNonNullExpression ([FACTS → 5.5d]) / the resolved-signature
    /// machinery — both escape; their fallback (full checkExpression)
    /// escapes on the same nodes today, so containment is unchanged.
    pub(crate) fn get_quick_type_of_expression(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<Option<TypeId>> {
        let source = self.binder.source_of_node(node);
        let expr = node_util::skip_parentheses_pub(source, node);
        match self.kind_of(expr) {
            SyntaxKind::AwaitExpression => {
                return Err(Unsupported::new(
                    "getQuickTypeOfExpression await arm (getAwaitedType, 5.5f)",
                ));
            }
            SyntaxKind::CallExpression => {
                let callee = match self.data_of(expr) {
                    NodeData::CallExpression(data) => data.expression,
                    _ => None,
                };
                let super_call =
                    callee.is_some_and(|c| self.kind_of(c) == SyntaxKind::SuperKeyword);
                if !super_call {
                    return Err(Unsupported::new(
                        "getQuickTypeOfExpression call arm (checkNonNullExpression, 5.5d/5.7)",
                    ));
                }
            }
            SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression => {
                let type_node = match self.data_of(expr) {
                    NodeData::AsExpression(data) => data.r#type,
                    NodeData::TypeAssertionExpression(data) => data.r#type,
                    _ => None,
                };
                if let Some(type_node) = type_node {
                    if !self.is_const_type_reference_node(type_node) {
                        return Ok(Some(self.get_type_from_type_node(type_node)?));
                    }
                }
            }
            _ => {}
        }
        let is_literal_like = matches!(
            self.kind_of(node),
            SyntaxKind::NumericLiteral
                | SyntaxKind::BigIntLiteral
                | SyntaxKind::StringLiteral
                | SyntaxKind::RegularExpressionLiteral
                | SyntaxKind::NoSubstitutionTemplateLiteral
                | SyntaxKind::TrueKeyword
                | SyntaxKind::FalseKeyword
        );
        if is_literal_like {
            return self.check_expression(node, CheckMode::NORMAL).map(Some);
        }
        Ok(None)
    }

    /// tsc-port: getContextFreeTypeOfExpression @6.0.3
    /// tsc-hash: 5748dd239c3dcf528e706cf0dad9a33ba5139a8ff058f9e270bec304bac605a1
    /// tsc-span: _tsc.js:80945-80959
    pub(crate) fn get_context_free_type_of_expression(
        &mut self,
        node: NodeId,
    ) -> CheckResult2<TypeId> {
        if let Some(cached) = self.links.node(node).context_free_type.resolved() {
            return Ok(cached);
        }
        self.push_contextual_type(node, Some(self.tables.intrinsics.any), false);
        let result = self.check_expression(node, CheckMode::SKIP_CONTEXT_SENSITIVE);
        self.pop_contextual_type();
        let ty = result?;
        if self.links.node(node).context_free_type.resolved().is_none() {
            self.links.set_node_context_free_type(
                self.speculation_depth,
                node,
                crate::links::LinkSlot::Resolved(ty),
            );
        }
        Ok(ty)
    }

}

/// tsc getThisContainer (14459) with BOTH parameters — the binder's
/// two-parameter M2 port predates includeClassComputedPropertyName;
/// checkThisExpression's interlocked toggling loop needs the full
/// form (a class-hosted computed property name is its own container).
pub(crate) fn get_this_container_full(
    state: &CheckerState,
    node: NodeId,
    include_arrow_functions: bool,
    include_class_computed_property_name: bool,
) -> Option<NodeId> {
    let mut node = node;
    loop {
        node = state.parent_of(node)?;
        match state.kind_of(node) {
            SyntaxKind::ComputedPropertyName => {
                let parent = state.parent_of(node)?;
                let grand = state.parent_of(parent)?;
                if include_class_computed_property_name
                    && matches!(
                        state.kind_of(grand),
                        SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression
                    )
                {
                    return Some(node);
                }
                node = grand;
            }
            SyntaxKind::Decorator => {
                let parent = state.parent_of(node)?;
                if state.kind_of(parent) == SyntaxKind::Parameter {
                    if let Some(grand) = state.parent_of(parent) {
                        if is_class_element_kind(state.kind_of(grand)) {
                            node = grand;
                        }
                    }
                } else if is_class_element_kind(state.kind_of(parent)) {
                    node = parent;
                }
            }
            SyntaxKind::ArrowFunction if !include_arrow_functions => {}
            SyntaxKind::ArrowFunction
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::PropertySignature
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::MethodSignature
            | SyntaxKind::Constructor
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::CallSignature
            | SyntaxKind::ConstructSignature
            | SyntaxKind::IndexSignature
            | SyntaxKind::EnumDeclaration
            | SyntaxKind::SourceFile => return Some(node),
            _ => {}
        }
    }
}

/// tsc isClassElement kinds.
fn is_class_element_kind(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::Constructor
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::IndexSignature
            | SyntaxKind::ClassStaticBlockDeclaration
            | SyntaxKind::SemicolonClassElement
    )
}

/// tsc reverseAccessKind (17495).
fn reverse_access_kind(kind: AccessKind) -> AccessKind {
    match kind {
        AccessKind::Read => AccessKind::Write,
        AccessKind::Write => AccessKind::Read,
        AccessKind::ReadWrite => AccessKind::ReadWrite,
    }
}

/// tsc isShiftOperatorOrHigher (isCompoundLikeAssignment's RHS test):
/// every binary operator with shift precedence or higher.
fn is_shift_operator_or_higher(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::LessThanLessThanToken
            | SyntaxKind::GreaterThanGreaterThanToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanToken
            | SyntaxKind::PlusToken
            | SyntaxKind::MinusToken
            | SyntaxKind::AsteriskToken
            | SyntaxKind::SlashToken
            | SyntaxKind::PercentToken
            | SyntaxKind::AsteriskAsteriskToken
    )
}

/// tsc-port: parsePseudoBigInt @6.0.3
/// tsc-hash: 551740fd60e2c53fae1321cfb28848686cb2d80348a71fb7f72c35eff31a8664
/// tsc-span: _tsc.js:18909-18964
///
/// Binary/octal/hex forms convert through the segment div-10 loop;
/// decimal strips leading zeros. Scanner-invalid text (parse recovery)
/// escapes.
pub(crate) fn parse_pseudo_big_int(text: &str) -> CheckResult2<PseudoBigInt> {
    let bytes = text.as_bytes();
    let log2_base = match bytes.get(1) {
        Some(b'b') | Some(b'B') => Some(1u32),
        Some(b'o') | Some(b'O') => Some(3u32),
        Some(b'x') | Some(b'X') => Some(4u32),
        _ => None,
    };
    let Some(log2_base) = log2_base else {
        // Decimal: strip the trailing `n` and leading zeros.
        let digits = text.strip_suffix('n').unwrap_or(text);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(Unsupported::new(format!(
                "unparsable bigint literal text {text:?} (parse recovery)"
            )));
        }
        let trimmed = digits.trim_start_matches('0');
        let base10_value = if trimmed.is_empty() { "0" } else { trimmed };
        return Ok(PseudoBigInt {
            negative: false,
            base10_value: base10_value.to_owned(),
        });
    };
    let start_index = 2usize;
    let end_index = text.len().saturating_sub(1);
    if end_index <= start_index {
        return Err(Unsupported::new(format!(
            "unparsable bigint literal text {text:?} (parse recovery)"
        )));
    }
    let bits_needed = (end_index - start_index) as u32 * log2_base;
    let mut segments =
        vec![0u32; (bits_needed >> 4) as usize + if bits_needed & 15 != 0 { 1 } else { 0 }];
    let mut bit_offset = 0u32;
    for i in (start_index..end_index).rev() {
        let segment = (bit_offset >> 4) as usize;
        let digit_char = bytes[i];
        let digit = match digit_char {
            b'0'..=b'9' => (digit_char - b'0') as u32,
            b'A'..=b'F' => 10 + (digit_char - b'A') as u32,
            b'a'..=b'f' => 10 + (digit_char - b'a') as u32,
            b'_' => {
                // Numeric separators never reach node.text (scanner
                // strips them); recovery-only.
                return Err(Unsupported::new(format!(
                    "unparsable bigint literal text {text:?} (parse recovery)"
                )));
            }
            _ => {
                return Err(Unsupported::new(format!(
                    "unparsable bigint literal text {text:?} (parse recovery)"
                )))
            }
        };
        let shifted_digit = digit << (bit_offset & 15);
        segments[segment] |= shifted_digit & 0xFFFF;
        let residual = shifted_digit >> 16;
        if residual != 0 {
            segments[segment + 1] |= residual;
        }
        bit_offset += log2_base;
    }
    let mut base10 = Vec::new();
    let mut first_nonzero_segment = segments.len() as isize - 1;
    let mut segments_remaining = true;
    while segments_remaining {
        let mut mod10 = 0u32;
        segments_remaining = false;
        for segment in (0..=first_nonzero_segment.max(0) as usize).rev() {
            let new_segment = (mod10 << 16) | segments[segment];
            let segment_value = new_segment / 10;
            segments[segment] = segment_value;
            mod10 = new_segment - segment_value * 10;
            if segment_value != 0 && !segments_remaining {
                first_nonzero_segment = segment as isize;
                segments_remaining = true;
            }
        }
        base10.push(b'0' + mod10 as u8);
    }
    base10.reverse();
    let base10_value = String::from_utf8(base10).expect("digits are ASCII");
    Ok(PseudoBigInt {
        negative: false,
        base10_value,
    })
}

#[cfg(test)]
mod tests {
    use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
    use tsrs2_types::{CheckMode, CompilerOptions};

    use crate::state::test_support::with_program_state;
    use crate::state::CheckerState;

    /// Driver-level fixture check (check.rs idiom): full
    /// check_source_file, checker-sink rows as (code, start, length).
    fn checked_rows(text: &str) -> Vec<(u32, u32, u32)> {
        checked_rows_with(text, &CompilerOptions::default())
    }

    fn checked_rows_with(text: &str, options: &CompilerOptions) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], options, |state| {
            state.check_source_file(0);
            rows(state)
        })
    }

    fn rows(state: &CheckerState) -> Vec<(u32, u32, u32)> {
        state
            .diagnostics
            .iter()
            // File-less program diagnostics (lazy missing-global 2318s
            // in no-lib fixtures) are excluded from per-file output.
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

    /// The first node of `kind` whose parent satisfies `parent_kind`
    /// (None = any parent) — for direct check_expression probes of
    /// arms the 5.5a driver cannot reach (assignment operands route
    /// through the 5.5e binary arm, super through 5.5d receivers).
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

    fn direct_expression_rows(text: &str, kind: SyntaxKind, parent: Option<SyntaxKind>) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            let node = find_node(state, kind, parent);
            let _ = state.check_expression(node, CheckMode::NORMAL);
            rows(state)
        })
    }

    // ---- driver forcing (checkExpressionStatement) — oracle-pinned ----

    #[test]
    fn expression_statements_force_identifier_resolution() {
        assert_eq!(checked_rows("missingName;\n"), [(2304, 0, 11)]);
    }

    #[test]
    fn resolved_identifiers_are_silent() {
        assert_eq!(checked_rows("let y: number = 1;\ny;\n"), []);
    }

    #[test]
    fn typeof_forces_its_operand() {
        assert_eq!(checked_rows("typeof missing;\n"), [(2304, 7, 7)]);
    }

    #[test]
    fn void_defers_then_checks_its_operand() {
        // checkVoidExpression registers the node; the deferred drain
        // checks the operand (checkDeferredNode's void arm).
        assert_eq!(checked_rows("void missing;\n"), [(2304, 5, 7)]);
    }

    #[test]
    fn delete_of_a_non_access_reports_2703() {
        // Oracle also reports 1102 (strict-mode delete on an
        // identifier) from the BINDER sink — merged at the program
        // layer, not part of the checker sink this asserts.
        assert_eq!(
            checked_rows("declare var x: number;\ndelete x;\n"),
            [(2703, 30, 1)]
        );
    }

    // ---- literals — oracle-pinned ----

    #[test]
    fn literal_statements_are_silent() {
        assert_eq!(checked_rows("\"abc\";\n123;\ntrue;\nfalse;\nnull;\n1n;\n"), []);
        // Regex forces the RegExp global: no-lib fixtures take the
        // one-shot file-less 2318, excluded from per-file rows.
        assert_eq!(checked_rows("/abc/;\n"), []);
    }

    #[test]
    fn bigint_below_es2020_reports_2737() {
        let options = CompilerOptions {
            target: Some(tsrs2_types::ScriptTarget::ES5.bits()),
            ..CompilerOptions::default()
        };
        assert_eq!(checked_rows_with("1n;\n", &options), [(2737, 0, 2)]);
    }

    #[test]
    fn hex_bigint_literals_convert_to_base10() {
        let parsed = super::parse_pseudo_big_int("0x10n").expect("hex bigint parses");
        assert_eq!(parsed.base10_value, "16");
        assert!(!parsed.negative);
        let parsed = super::parse_pseudo_big_int("0b1010n").expect("binary bigint parses");
        assert_eq!(parsed.base10_value, "10");
        let parsed = super::parse_pseudo_big_int("0o777n").expect("octal bigint parses");
        assert_eq!(parsed.base10_value, "511");
        let parsed =
            super::parse_pseudo_big_int("0xffffffffffffffffn").expect("wide hex bigint parses");
        assert_eq!(parsed.base10_value, "18446744073709551615");
        let parsed = super::parse_pseudo_big_int("000123n").expect("decimal strips zeros");
        assert_eq!(parsed.base10_value, "123");
    }

    // ---- TDZ (checkResolvedBlockScopedVariable) — oracle-pinned ----

    #[test]
    fn let_used_before_declaration_reports_2448_with_related() {
        with_program_state(
            &[("a.ts", "x;\nlet x: number = 1;\n")],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                // Oracle also reports 2454 (used before being
                // assigned) — flow analysis, M5 FN.
                assert_eq!(rows(state), [(2448, 0, 1)]);
                let diag = &state.diagnostics[0];
                assert_eq!(diag.related.len(), 1);
                assert_eq!(diag.related[0].message.code, 2728);
                assert_eq!(diag.related[0].start, Some(7));
                assert_eq!(diag.related[0].length, Some(1));
            },
        );
    }

    #[test]
    fn class_used_before_declaration_reports_2449() {
        assert_eq!(checked_rows("C;\nclass C {}\n"), [(2449, 0, 1)]);
    }

    #[test]
    fn enum_used_before_declaration_reports_2450() {
        assert_eq!(checked_rows("E;\nenum E { A }\n"), [(2450, 0, 1)]);
    }

    #[test]
    fn function_wrapped_tdz_use_is_legal() {
        // isUsedInFunctionOrInstanceProperty: a non-IIFE function
        // defers the use past the declaration.
        assert_eq!(
            checked_rows("function g(): void { x; }\nlet x: number = 1;\n"),
            []
        );
    }

    #[test]
    fn var_used_before_declaration_is_not_tdz() {
        // Oracle reports 2454 (flow, M5 FN); no TDZ for var.
        assert_eq!(checked_rows("v;\nvar v: number;\n"), []);
    }

    // ---- ambient statement grammar — oracle-pinned ----

    #[test]
    fn statements_in_ambient_contexts_report_1036_once_per_block() {
        // The DRIVER cannot reach ambient statements until
        // checkModuleDeclaration lands (5.8) — namespace bodies are an
        // honest FN band; this drives check_source_element directly at
        // the statements to pin the grammar port (oracle spans).
        let direct = |text: &str, count: usize| {
            with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
                let source = state.binder.source(0);
                let statements: Vec<NodeId> = source
                    .arena
                    .node_ids()
                    .filter(|&id| {
                        tsrs2_binder::node_util::kind_of(source, id)
                            == SyntaxKind::ExpressionStatement
                    })
                    .collect();
                assert_eq!(statements.len(), count);
                for statement in statements {
                    state.check_source_element(Some(statement));
                }
                rows(state)
            })
        };
        assert_eq!(direct("declare namespace N { 1; }\n", 1), [(1036, 22, 1)]);
        // Once-flag sits on the enclosing block: a second statement
        // stays silent.
        assert_eq!(direct("declare namespace N { 1; 2; }\n", 2), [(1036, 22, 1)]);
    }

    // ---- this / super — oracle-pinned ----

    #[test]
    fn this_in_namespace_body_reports_2331() {
        // Driver reachability arrives with checkModuleDeclaration
        // (5.8) — direct probe. Oracle also reports 2683
        // (noImplicitThis implicit-any this): its globalThisType probe
        // needs the VALUE_MODULE getTypeOfSymbol arm (5.8), so the
        // check abandons after 2331 (honest FN).
        let rows = direct_expression_rows(
            "namespace N { this; }\nexport {};\n",
            SyntaxKind::ThisKeyword,
            None,
        );
        assert_eq!(rows, [(2331, 14, 4)]);
    }

    #[test]
    fn super_without_extends_reports_2335() {
        let rows = direct_expression_rows(
            "class A { m(): void { super.x; } }\n",
            SyntaxKind::SuperKeyword,
            None,
        );
        assert_eq!(rows, [(2335, 22, 5)]);
    }

    #[test]
    fn super_outside_class_members_reports_2660() {
        let rows = direct_expression_rows(
            "function g(): void { super.x; }\n",
            SyntaxKind::SuperKeyword,
            None,
        );
        assert_eq!(rows, [(2660, 21, 5)]);
    }

    // ---- assignment-target mutability — oracle-pinned (direct
    // probes: assignments route through the 5.5e binary arm, so the
    // driver cannot reach these until the trampoline lands) ----

    fn assignment_lhs_rows(text: &str) -> Vec<(u32, u32, u32)> {
        with_program_state(&[("a.ts", text)], &CompilerOptions::default(), |state| {
            let source = state.binder.source(0);
            let node = source
                .arena
                .node_ids()
                .find(|&id| {
                    tsrs2_binder::node_util::kind_of(source, id) == SyntaxKind::Identifier
                        && tsrs2_binder::node_util::parent_of(source, id).is_some_and(|parent| {
                            matches!(
                                &source.arena.node(parent).data,
                                NodeData::BinaryExpression(data) if data.left == Some(id)
                            )
                        })
                })
                .expect("fixture contains an assignment LHS identifier");
            let _ = state.check_expression(node, CheckMode::NORMAL);
            rows(state)
        })
    }

    #[test]
    fn assigning_to_an_enum_reports_2628() {
        assert_eq!(assignment_lhs_rows("enum E { A }\nE = 1;\n"), [(2628, 13, 1)]);
    }

    #[test]
    fn assigning_to_a_class_reports_2629() {
        assert_eq!(assignment_lhs_rows("class C {}\nC = 1;\n"), [(2629, 11, 1)]);
    }

    #[test]
    fn assigning_to_a_function_reports_2630() {
        assert_eq!(
            assignment_lhs_rows("function f(): void {}\nf = 1;\n"),
            [(2630, 22, 1)]
        );
    }

    #[test]
    fn assigning_to_a_const_reports_2588() {
        assert_eq!(
            assignment_lhs_rows("const c: number = 1;\nc = 2;\n"),
            [(2588, 21, 1)]
        );
    }

    // ---- onFailedToResolveSymbol chain (5.5a slice) — oracle-pinned ----

    #[test]
    fn primitive_type_name_in_value_position_reports_2693() {
        assert_eq!(checked_rows("string;\n"), [(2693, 0, 6)]);
    }

    #[test]
    fn instance_member_near_miss_reports_2663() {
        // Method bodies are driver-unreachable until 5.8 — direct
        // probe on the body identifier.
        let rows = direct_expression_rows(
            "class C { foo: number = 1; m(): void { foo; } }\n",
            SyntaxKind::Identifier,
            Some(SyntaxKind::ExpressionStatement),
        );
        assert_eq!(rows, [(2663, 39, 3)]);
    }

    #[test]
    fn static_member_near_miss_reports_2662() {
        let rows = direct_expression_rows(
            "class C { static bar: number = 1; m(): void { bar; } }\n",
            SyntaxKind::Identifier,
            Some(SyntaxKind::ExpressionStatement),
        );
        assert_eq!(rows, [(2662, 46, 3)]);
    }

    #[test]
    fn primitive_name_inside_class_without_member_reports_2693() {
        let rows = direct_expression_rows(
            "class C { m(): void { string; } }\n",
            SyntaxKind::Identifier,
            Some(SyntaxKind::ExpressionStatement),
        );
        assert_eq!(rows, [(2693, 22, 6)]);
    }

    // ---- per-element containment ----

    #[test]
    fn out_of_slice_expressions_abandon_only_their_statement() {
        // Statement 2's binary WORKER arm is a 5.5e stub, but the
        // trampoline checks both operands first — the operand's 2304
        // (oracle-exact) lands before the escape contains the rest of
        // the statement. Statement 3 still checks.
        assert_eq!(
            checked_rows("let a: number = 1;\na + missingName;\nmissingName2;\n"),
            [(2304, 23, 11), (2304, 36, 12)]
        );
    }

    #[test]
    fn rechecking_is_idempotent() {
        with_program_state(
            &[("a.ts", "missingName;\nE;\nenum E { A }\n")],
            &CompilerOptions::default(),
            |state| {
                state.check_source_file(0);
                let first = rows(state);
                assert_eq!(first, [(2304, 0, 11), (2450, 13, 1)]);
                state.check_source_file(0);
                assert_eq!(rows(state), first);
            },
        );
    }
}
