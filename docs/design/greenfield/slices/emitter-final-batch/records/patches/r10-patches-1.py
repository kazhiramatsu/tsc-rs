#!/usr/bin/env python3
"""r10 patches, part 1 (checker + emitter causes attributed from the plan-base census).

Run from the tree root: python3 r10-patches-1.py . — each anchor asserts uniqueness.
"""
import sys, os
root = sys.argv[1] if len(sys.argv) > 1 else "."

def patch(path, pairs):
    p = os.path.join(root, path)
    s = open(p).read()
    for old, new in pairs:
        if s.count(old) == 0 and s.count(new) == 1:
            continue  # already applied
        n = s.count(old)
        assert n == 1, (path, n, old[:100])
        s = s.replace(old, new)
    open(p, "w").write(s)
    print("patched", path)

# ---- EF7-ENUM-ISOLATED (checker): const enum used before its declaration under isolatedModules
patch("crates/checker/src/resolve.rs", [(
'''        } else {
            debug_assert!(flags.intersects(SymbolFlags::CONST_ENUM));
            // getIsolatedModules(compilerOptions): option unmodeled ⇒
            // false ⇒ no message.
            None
        };''',
'''        } else {
            debug_assert!(flags.intersects(SymbolFlags::CONST_ENUM));
            // getIsolatedModules(compilerOptions) = isolatedModules ||
            // verbatimModuleSyntax (48468-48470; EF7-ENUM-ISOLATED).
            (self.options.isolated_modules == Some(true)
                || self.options.verbatim_module_syntax == Some(true))
            .then_some(&diagnostics::Enum_0_used_before_its_declaration)
        };''')])

# ---- EF7-ENUM-18056 (checker): enum member following a non-literal numeric member
patch("crates/checker/src/evaluate.rs", [(
'''    /// The isolatedModules arm (85623-85630, diagnostic 18058) is
    /// elided: the option is unmodeled (default off), so the arm never
    /// fires in an oracle-default run.
    fn compute_enum_member_value(
        &mut self,
        member: NodeId,
        auto_value: Option<f64>,
        _previous: Option<NodeId>,
    ) -> CheckResult<EvaluatorResult> {''',
'''    /// The isolatedModules arm (85621-85630, diagnostic 18056) is live
    /// since the ordinary emit route admits `isolatedModules` /
    /// `verbatimModuleSyntax` (EF7-ENUM-18056).
    fn compute_enum_member_value(
        &mut self,
        member: NodeId,
        auto_value: Option<f64>,
        previous: Option<NodeId>,
    ) -> CheckResult<EvaluatorResult> {'''),
(
'''            return Ok(undefined_result());
        };
        Ok(evaluator_result(
            Some(EvalValue::Num(auto_value)),
            false,
            false,
            false,
        ))''',
'''            return Ok(undefined_result());
        };
        // 85621-85630: `getIsolatedModules(compilerOptions) &&
        // previous?.initializer` → TS18056 unless the previous member's
        // value is a number resolved without other files.
        let previous_with_initializer = previous.filter(|&previous| {
            matches!(
                self.data_of(previous),
                NodeData::EnumMember(data) if data.initializer.is_some()
            )
        });
        if let Some(previous) = previous_with_initializer {
            if self.options.isolated_modules == Some(true)
                || self.options.verbatim_module_syntax == Some(true)
            {
                let numeric_local = self
                    .links
                    .node(previous)
                    .enum_member_value
                    .as_ref()
                    .is_some_and(|value| {
                        matches!(value.value, Some(EvalValue::Num(_))) && !value.resolved_other_files
                    });
                if !numeric_local {
                    self.error_at(
                        Some(name),
                        &diagnostics::Enum_member_following_a_non_literal_numeric_member_must_have_an_initializer_when_isolatedModules_is_enabled,
                        &[],
                    );
                }
            }
        }
        Ok(evaluator_result(
            Some(EvalValue::Num(auto_value)),
            false,
            false,
            false,
        ))''')])

# ---- EF7-JSDOC-CHECK-NODE (checker): getEffectiveCheckNode keeps a JSDoc type assertion in JS files
patch("crates/checker/src/functions.rs", [(
'''    /// skipOuterExpressions(Parentheses | Satisfies) — the two kinds
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
                }''',
'''    /// skipOuterExpressions(Parentheses | Satisfies) — the two kinds
    /// interleave in any order. In a JavaScript file the flags add
    /// `ExcludeJSDocTypeAssertion`: a parenthesized expression carrying a
    /// JSDoc `@type` cast is the check node itself (EF7-JSDOC-CHECK-NODE).
    pub(crate) fn get_effective_check_node(&self, argument: NodeId) -> NodeId {
        let exclude_jsdoc_type_assertions = self.is_in_js_file(argument);
        let mut node = argument;
        loop {
            match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => {
                    if exclude_jsdoc_type_assertions
                        && node_util::is_jsdoc_type_assertion(
                            self.binder.source_of_node(node),
                            node,
                        )
                    {
                        return node;
                    }
                    let Some(expression) = data.expression else {
                        return node;
                    };
                    node = expression;
                }''')])

# ---- EF7-SYMBOL-WRITTEN-FACE (checker): symbolToString faces in the abstract-property message
patch("crates/checker/src/access.rs", [(
'''                        let prop_name = self.symbol_display_name(prop);
                        let class_name = self.symbol_display_name(parent_symbol);
                        self.error_at_js(
                            Some(error_node),
                            &tsc_diagnostics::gen::Abstract_property_0_in_class_1_cannot_be_accessed_in_the_constructor,''',
'''                        // 74904: symbolToString(prop), symbolToString(parentSymbol)
                        // — the written faces (an anonymous class expression
                        // assigned to `const Foo` prints `Foo`, not `__class`).
                        let prop_name = self.symbol_name_as_written_slice(prop);
                        let class_name = self.symbol_name_as_written_slice(parent_symbol);
                        self.error_at_js(
                            Some(error_node),
                            &tsc_diagnostics::gen::Abstract_property_0_in_class_1_cannot_be_accessed_in_the_constructor,''')])

# ---- EF7-ASYNC-ARROW-LEXICAL-THIS (emitter classifier): createArrowFunction 22709-22710
patch("crates/emitter/src/builtins.rs", [(
'''        NodeData::MethodDeclaration(data) => Ok(function_like_facet_flags(
            arena,
            source,
            data.asterisk_token,
            data.modifiers,
        )?),
        _ => Ok(TransformFlags::NONE),
    }
}''',
'''        NodeData::MethodDeclaration(data) => Ok(function_like_facet_flags(
            arena,
            source,
            data.asterisk_token,
            data.modifiers,
        )?),
        // createArrowFunction 22709-22710: an async arrow captures the lexical
        // `this` for `__awaiter(this, …)` (`ContainsES2017 | ContainsLexicalThis`),
        // so class-fields counts it as a static-initializer `this` reference
        // (EF7-ASYNC-ARROW-LEXICAL-THIS).
        NodeData::ArrowFunction(data) => {
            let facets = function_like_facet_flags(arena, source, None, data.modifiers)?;
            Ok(if facets.contains(TransformFlags::CONTAINS_ES_2017) {
                facets | TransformFlags::CONTAINS_LEXICAL_THIS
            } else {
                facets
            })
        }
        _ => Ok(TransformFlags::NONE),
    }
}'''),
# ---- EF7-VERBATIM-EXPORT-ASSIGNMENT (TypeScript pass): visitExportAssignment keeps the node under verbatimModuleSyntax
(
'''                NodeData::ExportAssignment(mut data) => {
                    if self
                        .resolver
                        .is_value_alias_declaration(self.resolver_node(original)?)?
                    {''',
'''                NodeData::ExportAssignment(mut data) => {
                    // visitExportAssignment: `compilerOptions.verbatimModuleSyntax ||
                    // resolver.isValueAliasDeclaration(node)` (EF7-VERBATIM-EXPORT-ASSIGNMENT).
                    if self.verbatim_module_syntax
                        || self
                            .resolver
                            .is_value_alias_declaration(self.resolver_node(original)?)?
                    {'''),
# ---- EF7-HELPERS-IMPORT-PROLOGUE (ES module): copyPrologue also copies the custom prologue (hoisted declarations)
(
'''        if !is_prologue {
            break;
        }
        offset += 1;
    }
    statements.insert(offset, declaration);''',
'''        if !is_prologue {
            break;
        }
        offset += 1;
    }
    // `copyCustomPrologue`: statements flagged `EmitFlags.CustomPrologue`
    // (variable and function declarations materialized by an earlier pass's
    // lexical environment) stay ahead of the helpers import
    // (EF7-HELPERS-IMPORT-PROLOGUE).
    while offset < statements.len()
        && context
            .arena()
            .metadata(statements[offset])
            .is_some_and(|metadata| metadata.flags().contains(EmitFlags::CUSTOM_PROLOGUE))
    {
        offset += 1;
    }
    statements.insert(offset, declaration);''')])

# ---- EF7-YIELD-PARENS (ES2017): the yield produced from `await` is parenthesized only where tsc's factory parenthesizer would
patch("crates/emitter/src/factory.rs", [
('''enum Associativity {''', '''pub(crate) enum Associativity {'''),
('''const PRECEDENCE_YIELD: i8 = 2;''', '''pub(crate) const PRECEDENCE_YIELD: i8 = 2;'''),
('''const fn binary_operator_associativity(operator: SyntaxKind) -> Associativity {''',
 '''pub(crate) const fn binary_operator_associativity(operator: SyntaxKind) -> Associativity {'''),
('''const fn binary_operator_precedence(operator: SyntaxKind) -> i8 {''',
 '''pub(crate) const fn binary_operator_precedence(operator: SyntaxKind) -> i8 {'''),
])
patch("crates/emitter/src/builtins/es2017.rs", [(
'''            NodeData::BinaryExpression(data) => {
                let operator = data
                    .operator_token
                    .map(|operator| self.context.arena().node(self.node(operator)))
                    .transpose()?
                    .map(|operator| operator.kind);
                let assignment = operator.is_some_and(|operator| {
                    operator.value() >= SyntaxKind::FirstAssignment.value()
                        && operator.value() <= SyntaxKind::LastAssignment.value()
                });
                !(assignment && data.right == Some(original.node()))
            }
            NodeData::PrefixUnaryExpression(_) | NodeData::PostfixUnaryExpression(_) => true,''',
'''            NodeData::BinaryExpression(data) => {
                // binaryOperandNeedsParentheses (`updateBinaryExpression` →
                // parenthesizeLeft/RightSideOfBinary): a yield operand
                // (precedence 2) is parenthesized when the operator binds
                // tighter, except as the right operand of a right-associative
                // operator (`x = yield y`, `x ** yield y`); the comma operator
                // binds looser and never parenthesizes (EF7-YIELD-PARENS).
                let Some(operator) = data
                    .operator_token
                    .map(|operator| self.context.arena().node(self.node(operator)))
                    .transpose()?
                    .map(|operator| operator.kind)
                else {
                    return Ok(false);
                };
                let is_right = data.right == Some(original.node());
                let operator_precedence = crate::factory::binary_operator_precedence(operator);
                crate::factory::PRECEDENCE_YIELD < operator_precedence
                    && !(is_right
                        && crate::factory::binary_operator_associativity(operator)
                            == crate::factory::Associativity::Right)
            }
            // parenthesizeOperandOfPrefixUnary / OfPostfixUnary: a yield is
            // not a unary expression.
            NodeData::PrefixUnaryExpression(_)
            | NodeData::PostfixUnaryExpression(_)
            | NodeData::TypeOfExpression(_)
            | NodeData::VoidExpression(_)
            | NodeData::DeleteExpression(_)
            | NodeData::AwaitExpression(_) => true,''')])

# ---- EF7-GENERATOR-LOOP-VARIABLE (generators): createLoopVariable is the `_i` family re-assigned per printer scope
patch("crates/emitter/src/builtins/generators.rs", [(
'''        let provisional = self
            .generated_bindings
            .allocate_loop_variable(/*reserve_in_nested_scopes*/ false);
        // Planned-authoritative: the finalize walk keeps the `_i`-family
        // spelling verbatim (the B-4 collision lattice is its owner's
        // concern; no B-3 fixture occupies the family).
        TargetBinding::allocate_planned(self.context, provisional)''',
'''        let provisional = self
            .generated_bindings
            .allocate_loop_variable(/*reserve_in_nested_scopes*/ false);
        // `createLoopVariable()`: the visit-time `_i` is provisional and the
        // finalize walk re-assigns it per printer scope (`makeTempVariableName
        // (TempFlags._i)` resets with every function's tempFlags), exactly like
        // the ES2015 for-of loop variable (EF7-GENERATOR-LOOP-VARIABLE).
        TargetBinding::allocate_planned_loop(self.context, provisional)''')])

# ---- EF7-OBJECT-LITERAL-INDENTED (ES2015): `A || (hasComputed = B)` short-circuits the assignment
patch("crates/emitter/src/builtins/es2015.rs", [(
'''            if contains_yield_in_async || is_computed {
                has_computed = is_computed;
                num_initial_properties = Some(index);
                break;
            }''',
'''            // `property.transformFlags & ContainsYield && hierarchyFacts &
            // AsyncFunctionBody || (hasComputed = isComputedPropertyName)`:
            // the assignment is skipped when the yield arm short-circuits, so
            // a yield-containing computed property under an async body does
            // not mark the initial literal `Indented`
            // (EF7-OBJECT-LITERAL-INDENTED).
            if contains_yield_in_async {
                num_initial_properties = Some(index);
                break;
            }
            if is_computed {
                has_computed = true;
                num_initial_properties = Some(index);
                break;
            }''')])
print("r10 part 1 applied")
