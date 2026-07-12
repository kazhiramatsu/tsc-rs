//! The constant-expression evaluator (M4 5.3b): tsc's shared
//! `createEvaluator` (19382) instantiated with the checker's
//! entity-name/element-access arms, plus `computeEnumMemberValues`
//! and the declared-before-use walk those arms lean on. Everything
//! here feeds enum declared types (annotate.rs) and enum relations
//! (relate.rs isEnumTypeRelatedTo).
//!
//! The declared-before-use walk gained its checkExpression-era arms at
//! 5.5a (TDZ band: class + binding-element declarations, the IIFE /
//! property-initializer / decorator usage sub-arms). Still escaped:
//! property and parameter-property DECLARATIONS plus the static-block
//! property-initialization probe — the 2729 consumer band (5.5d).

use tsrs2_binder::{node_util, SymbolId};
use tsrs2_diags::gen as diagnostics;
use tsrs2_syntax::{NodeData, NodeId, SyntaxKind};
use tsrs2_types::{ModifierFlags, NodeFlags, SymbolFlags};

use crate::state::{CheckResult2, CheckerState, Unsupported};

/// tsc EvaluatorResult.value: string | number | undefined (pseudo
/// bigints never flow — the evaluator has no bigint arms).
#[derive(Clone, Debug, PartialEq)]
pub enum EvalValue {
    Str(String),
    Num(f64),
}

/// tsc-port: evaluatorResult @6.0.3
/// tsc-hash: 306471c9c9700df5c41a9bc47881a08d5b2a3d26ffca8eed84fad7887c79ea23
/// tsc-span: _tsc.js:19379-19381
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluatorResult {
    pub value: Option<EvalValue>,
    pub is_syntactically_string: bool,
    pub resolved_other_files: bool,
    pub has_external_references: bool,
}

pub fn evaluator_result(
    value: Option<EvalValue>,
    is_syntactically_string: bool,
    resolved_other_files: bool,
    has_external_references: bool,
) -> EvaluatorResult {
    EvaluatorResult {
        value,
        is_syntactically_string,
        resolved_other_files,
        has_external_references,
    }
}

fn undefined_result() -> EvaluatorResult {
    evaluator_result(None, false, false, false)
}

impl<'a> CheckerState<'a> {
    /// tsc-port: getEnumMemberValue @6.0.3
    /// tsc-hash: a5c901df4f0a1434d5bc7598d85a54b6cd18243641e6438d89475370d5ea6250
    /// tsc-span: _tsc.js:88231-88237
    pub(crate) fn get_enum_member_value(
        &mut self,
        member: NodeId,
    ) -> CheckResult2<EvaluatorResult> {
        let parent = self
            .parent_of(member)
            .expect("enum members hang off their enum declaration");
        self.compute_enum_member_values(parent)?;
        Ok(self
            .links
            .node(member)
            .enum_member_value
            .unwrap_or_else(undefined_result))
    }

    /// tsc-port: computeEnumMemberValues @6.0.3
    /// tsc-hash: aa6ddfa2dacf97c4acae1099dbe06490dbba2a30c4f0f2018624008e82a55862
    /// tsc-span: _tsc.js:85580-85593
    ///
    /// tsc sets the EnumValuesComputed flag up front and writes each
    /// member's links entry as the loop advances — same-enum backward
    /// references re-enter through getEnumMemberValue, see the flag,
    /// and read the already-written slot. On Unsupported unwind the
    /// flag REVERTS (tsc cannot fail here) so a later query recomputes;
    /// already-written member slots are reused, not rewritten.
    fn compute_enum_member_values(&mut self, node: NodeId) -> CheckResult2<()> {
        if self.links.node(node).enum_values_computed {
            return Ok(());
        }
        self.links
            .set_node_enum_values_computed(self.speculation_depth, node);
        let members = match self.data_of(node) {
            NodeData::EnumDeclaration(data) => self.nodes_of(data.members),
            _ => unreachable!("computeEnumMemberValues callers pass enum declarations"),
        };
        let mut auto_value = Some(0f64);
        let mut previous: Option<NodeId> = None;
        for member in members {
            let outcome = if let Some(existing) = self.links.node(member).enum_member_value {
                Ok(existing)
            } else {
                self.compute_enum_member_value(member, auto_value, previous)
            };
            let result = match outcome {
                Ok(result) => result,
                Err(err) => {
                    self.links
                        .revert_node_enum_values_computed(self.speculation_depth, node);
                    return Err(err);
                }
            };
            if self.links.node(member).enum_member_value.is_none() {
                self.links.set_node_enum_member_value(
                    self.speculation_depth,
                    member,
                    result.clone(),
                );
            }
            auto_value = match result.value {
                Some(EvalValue::Num(value)) => Some(value + 1.0),
                _ => None,
            };
            previous = Some(member);
        }
        Ok(())
    }

    /// tsc-port: computeEnumMemberValue @6.0.3
    /// tsc-hash: 4461717e7939639f1770e6a37065ac5cdc7d0efe08159150566d0d3be3cbaa66
    /// tsc-span: _tsc.js:85594-85631
    ///
    /// The isolatedModules arm (85623-85630, diagnostic 18058) is
    /// elided: the option is unmodeled (default off), so the arm never
    /// fires in an oracle-default run.
    fn compute_enum_member_value(
        &mut self,
        member: NodeId,
        auto_value: Option<f64>,
        _previous: Option<NodeId>,
    ) -> CheckResult2<EvaluatorResult> {
        let (name, initializer) = match self.data_of(member) {
            NodeData::EnumMember(data) => (data.name, data.initializer),
            _ => unreachable!("enum member lists hold enum members"),
        };
        let name = name.ok_or_else(|| Unsupported::new("enum member with missing name"))?;
        if self.is_computed_non_literal_name(name) {
            self.error_at(
                Some(name),
                &diagnostics::Computed_property_names_are_not_allowed_in_enums,
                &[],
            );
        } else if self.kind_of(name) == SyntaxKind::BigIntLiteral {
            self.error_at(
                Some(name),
                &diagnostics::An_enum_member_cannot_have_a_numeric_name,
                &[],
            );
        } else {
            let text = self.get_text_of_property_name(name)?;
            if is_numeric_literal_name(&text) && !is_infinity_or_nan_string(&text) {
                self.error_at(
                    Some(name),
                    &diagnostics::An_enum_member_cannot_have_a_numeric_name,
                    &[],
                );
            }
        }
        if initializer.is_some() {
            return self.compute_constant_enum_member_value(member);
        }
        let parent = self
            .parent_of(member)
            .expect("enum members hang off their enum declaration");
        if self.node_flags(parent) & NodeFlags::AMBIENT.bits() != 0 && !self.is_enum_const(parent)
        {
            return Ok(undefined_result());
        }
        let Some(auto_value) = auto_value else {
            self.error_at(
                Some(name),
                &diagnostics::Enum_member_must_have_initializer,
                &[],
            );
            return Ok(undefined_result());
        };
        Ok(evaluator_result(
            Some(EvalValue::Num(auto_value)),
            false,
            false,
            false,
        ))
    }

    /// tsc-port: computeConstantEnumMemberValue @6.0.3
    /// tsc-hash: f90d1dc614b5f3fe4a1cac8bd8470fe49f5a423b9eae5130d8c6c59e5070480a
    /// tsc-span: _tsc.js:85632-85657
    ///
    /// The isolatedModules string arm (85646-85651, diagnostic 18055)
    /// is elided with the option. The non-constant fallback
    /// (85654: checkTypeAssignableTo over checkExpression) is live
    /// since 5.5e.
    fn compute_constant_enum_member_value(
        &mut self,
        member: NodeId,
    ) -> CheckResult2<EvaluatorResult> {
        let parent = self
            .parent_of(member)
            .expect("enum members hang off their enum declaration");
        let is_const_enum = self.is_enum_const(parent);
        let initializer = match self.data_of(member) {
            NodeData::EnumMember(data) => data
                .initializer
                .expect("computeConstantEnumMemberValue callers check the initializer"),
            _ => unreachable!("enum member lists hold enum members"),
        };
        let result = self.evaluate(initializer, Some(member))?;
        match &result.value {
            Some(value) => {
                if is_const_enum {
                    if let EvalValue::Num(number) = value {
                        if !number.is_finite() {
                            self.error_at(
                                Some(initializer),
                                if number.is_nan() {
                                    &diagnostics::const_enum_member_initializer_was_evaluated_to_disallowed_value_NaN
                                } else {
                                    &diagnostics::const_enum_member_initializer_was_evaluated_to_a_non_finite_value
                                },
                                &[],
                            );
                        }
                    }
                }
            }
            None => {
                if is_const_enum {
                    self.error_at(
                        Some(initializer),
                        &diagnostics::const_enum_member_initializers_must_be_constant_expressions,
                        &[],
                    );
                } else if self.node_flags(parent) & NodeFlags::AMBIENT.bits() != 0 {
                    self.error_at(
                        Some(initializer),
                        &diagnostics::In_ambient_enum_declarations_member_initializer_must_be_constant_expression,
                        &[],
                    );
                } else {
                    // 85654: checkTypeAssignableTo(checkExpression(
                    // initializer), numberType, initializer, 2553-head).
                    let source =
                        self.check_expression(initializer, tsrs2_types::CheckMode::NORMAL)?;
                    let number = self.tables.intrinsics.number;
                    self.check_type_assignable_to(
                        source,
                        number,
                        Some(initializer),
                        &diagnostics::Type_0_is_not_assignable_to_type_1_as_required_for_computed_enum_member_values,
                    )?;
                }
            }
        }
        Ok(result)
    }

    /// tsc-port: evaluate @6.0.3
    /// tsc-hash: cf8c7011795daf924318d70c10b46875e0671880a842c17716c1ab33dcfb2a9a
    /// tsc-span: _tsc.js:19383-19475
    pub(crate) fn evaluate(
        &mut self,
        expr: NodeId,
        location: Option<NodeId>,
    ) -> CheckResult2<EvaluatorResult> {
        let expr = self.skip_parentheses(expr);
        match self.data_of(expr).clone() {
            NodeData::PrefixUnaryExpression(data) => {
                let operand = data
                    .operand
                    .ok_or_else(|| Unsupported::new("prefix expression with missing operand"))?;
                let result = self.evaluate(operand, location)?;
                if let Some(EvalValue::Num(value)) = result.value {
                    let mapped = match data.operator {
                        SyntaxKind::PlusToken => Some(value),
                        SyntaxKind::MinusToken => Some(-value),
                        SyntaxKind::TildeToken => Some(!(js_to_int32(value)) as f64),
                        _ => None,
                    };
                    if let Some(mapped) = mapped {
                        return Ok(evaluator_result(
                            Some(EvalValue::Num(mapped)),
                            false,
                            result.resolved_other_files,
                            result.has_external_references,
                        ));
                    }
                }
                Ok(evaluator_result(
                    None,
                    false,
                    result.resolved_other_files,
                    result.has_external_references,
                ))
            }
            NodeData::BinaryExpression(data) => {
                let (left_node, operator_token, right_node) =
                    match (data.left, data.operator_token, data.right) {
                        (Some(left), Some(operator), Some(right)) => (left, operator, right),
                        _ => {
                            return Err(Unsupported::new(
                                "binary expression with missing pieces",
                            ))
                        }
                    };
                let operator = self.kind_of(operator_token);
                let left = self.evaluate(left_node, location)?;
                let right = self.evaluate(right_node, location)?;
                let is_syntactically_string = (left.is_syntactically_string
                    || right.is_syntactically_string)
                    && operator == SyntaxKind::PlusToken;
                let resolved_other_files = left.resolved_other_files || right.resolved_other_files;
                let has_external_references =
                    left.has_external_references || right.has_external_references;
                if let (Some(EvalValue::Num(l)), Some(EvalValue::Num(r))) =
                    (&left.value, &right.value)
                {
                    let (l, r) = (*l, *r);
                    let mapped = match operator {
                        SyntaxKind::BarToken => Some((js_to_int32(l) | js_to_int32(r)) as f64),
                        SyntaxKind::AmpersandToken => {
                            Some((js_to_int32(l) & js_to_int32(r)) as f64)
                        }
                        SyntaxKind::GreaterThanGreaterThanToken => {
                            Some((js_to_int32(l) >> (js_to_uint32(r) & 31)) as f64)
                        }
                        SyntaxKind::GreaterThanGreaterThanGreaterThanToken => {
                            Some((js_to_uint32(l) >> (js_to_uint32(r) & 31)) as f64)
                        }
                        SyntaxKind::LessThanLessThanToken => {
                            Some((js_to_int32(l) << (js_to_uint32(r) & 31)) as f64)
                        }
                        SyntaxKind::CaretToken => Some((js_to_int32(l) ^ js_to_int32(r)) as f64),
                        SyntaxKind::AsteriskToken => Some(l * r),
                        SyntaxKind::SlashToken => Some(l / r),
                        SyntaxKind::PlusToken => Some(l + r),
                        SyntaxKind::MinusToken => Some(l - r),
                        SyntaxKind::PercentToken => Some(l % r),
                        SyntaxKind::AsteriskAsteriskToken => Some(js_pow(l, r)),
                        _ => None,
                    };
                    if let Some(mapped) = mapped {
                        return Ok(evaluator_result(
                            Some(EvalValue::Num(mapped)),
                            is_syntactically_string,
                            resolved_other_files,
                            has_external_references,
                        ));
                    }
                } else if operator == SyntaxKind::PlusToken {
                    let concat = |value: &EvalValue| -> String {
                        match value {
                            EvalValue::Str(text) => text.clone(),
                            EvalValue::Num(number) => {
                                tsrs2_types::js_number_to_string(*number)
                            }
                        }
                    };
                    if let (Some(l), Some(r)) = (&left.value, &right.value) {
                        return Ok(evaluator_result(
                            Some(EvalValue::Str(format!("{}{}", concat(l), concat(r)))),
                            is_syntactically_string,
                            resolved_other_files,
                            has_external_references,
                        ));
                    }
                }
                Ok(evaluator_result(
                    None,
                    is_syntactically_string,
                    resolved_other_files,
                    has_external_references,
                ))
            }
            NodeData::StringLiteral(data) => Ok(evaluator_result(
                Some(EvalValue::Str(data.text.clone())),
                true,
                false,
                false,
            )),
            NodeData::NoSubstitutionTemplateLiteral(data) => Ok(evaluator_result(
                Some(EvalValue::Str(data.text.clone())),
                true,
                false,
                false,
            )),
            NodeData::TemplateExpression(_) => self.evaluate_template_expression(expr, location),
            NodeData::NumericLiteral(data) => {
                let value = crate::annotate::parse_numeric_literal_text(&data.text)?;
                Ok(evaluator_result(
                    Some(EvalValue::Num(value)),
                    false,
                    false,
                    false,
                ))
            }
            NodeData::Identifier(_) => self.evaluate_entity_name_expression(expr, location),
            NodeData::PropertyAccessExpression(_) => {
                if self.is_entity_name_expression(expr) {
                    return self.evaluate_entity_name_expression(expr, location);
                }
                Ok(undefined_result())
            }
            NodeData::ElementAccessExpression(_) => {
                self.evaluate_element_access_expression(expr, location)
            }
            _ => Ok(undefined_result()),
        }
    }

    /// tsc-port: evaluateTemplateExpression @6.0.3
    /// tsc-hash: 246e9f296c1bbc64e52c6295a39da58da7c30d09539f86774ede4074fad6bd5a
    /// tsc-span: _tsc.js:19476-19502
    fn evaluate_template_expression(
        &mut self,
        expr: NodeId,
        location: Option<NodeId>,
    ) -> CheckResult2<EvaluatorResult> {
        let (head, spans) = match self.data_of(expr) {
            NodeData::TemplateExpression(data) => (data.head, self.nodes_of(data.template_spans)),
            _ => unreachable!("template expression callers check the kind"),
        };
        let head = head.ok_or_else(|| Unsupported::new("template expression missing head"))?;
        let mut result = match self.data_of(head) {
            NodeData::TemplateHead(data) => data.text.clone(),
            _ => return Err(Unsupported::new("template expression with non-head head")),
        };
        let mut resolved_other_files = false;
        let mut has_external_references = false;
        for span in spans {
            let (expression, literal) = match self.data_of(span) {
                NodeData::TemplateSpan(data) => (data.expression, data.literal),
                _ => unreachable!("template spans hold template spans"),
            };
            let expression = expression
                .ok_or_else(|| Unsupported::new("template span with missing expression"))?;
            let span_result = self.evaluate(expression, location)?;
            let Some(value) = span_result.value else {
                return Ok(evaluator_result(None, true, false, false));
            };
            match value {
                EvalValue::Str(text) => result.push_str(&text),
                EvalValue::Num(number) => {
                    result.push_str(&tsrs2_types::js_number_to_string(number))
                }
            }
            let literal =
                literal.ok_or_else(|| Unsupported::new("template span with missing literal"))?;
            match self.data_of(literal) {
                NodeData::TemplateMiddle(data) => result.push_str(&data.text),
                NodeData::TemplateTail(data) => result.push_str(&data.text),
                _ => return Err(Unsupported::new("template span with non-template literal")),
            }
            resolved_other_files = resolved_other_files || span_result.resolved_other_files;
            has_external_references = has_external_references || span_result.has_external_references;
        }
        Ok(evaluator_result(
            Some(EvalValue::Str(result)),
            true,
            resolved_other_files,
            has_external_references,
        ))
    }

    /// tsc-port: evaluateEntityNameExpression @6.0.3
    /// tsc-hash: 75e23fe35aacb7b8ab9a5a25c919e6390d0a076daa4e29550a7a9922a51e8b02
    /// tsc-span: _tsc.js:85658-85715
    fn evaluate_entity_name_expression(
        &mut self,
        expr: NodeId,
        location: Option<NodeId>,
    ) -> CheckResult2<EvaluatorResult> {
        let Some(symbol) =
            self.resolve_entity_name(expr, SymbolFlags::VALUE, /*ignore_errors*/ true, None)
        else {
            return Ok(undefined_result());
        };
        if self.kind_of(expr) == SyntaxKind::Identifier {
            if let Some(text) = self.identifier_text_of(expr).map(str::to_owned) {
                if is_infinity_or_nan_string(&text)
                    && Some(symbol)
                        == self.get_global_symbol(&text, SymbolFlags::VALUE, /*diagnostic*/ None)
                {
                    let value = if text == "NaN" {
                        f64::NAN
                    } else if text == "Infinity" {
                        f64::INFINITY
                    } else {
                        f64::NEG_INFINITY
                    };
                    return Ok(evaluator_result(
                        Some(EvalValue::Num(value)),
                        false,
                        false,
                        false,
                    ));
                }
            }
        }
        let flags = self.binder.symbol(symbol).flags;
        if flags.intersects(SymbolFlags::ENUM_MEMBER) {
            return match location {
                Some(location) => self.evaluate_enum_member(expr, symbol, location),
                None => {
                    let declaration = self
                        .binder
                        .symbol(symbol)
                        .value_declaration
                        .expect("enum member symbols have value declarations");
                    self.get_enum_member_value(declaration)
                }
            };
        }
        if self.is_constant_variable(symbol) {
            let declaration = self.binder.symbol(symbol).value_declaration;
            if let Some(declaration) = declaration {
                if let NodeData::VariableDeclaration(data) = self.data_of(declaration).clone() {
                    if data.r#type.is_none() && data.initializer.is_some() {
                        let gate = match location {
                            None => true,
                            Some(location) => {
                                declaration != location
                                    && self.is_block_scoped_name_declared_before_use(
                                        declaration,
                                        location,
                                    )?
                            }
                        };
                        if gate {
                            let result =
                                self.evaluate(data.initializer.unwrap(), Some(declaration))?;
                            if let Some(location) = location {
                                if self.binder.file_index_of_node(location)
                                    != self.binder.file_index_of_node(declaration)
                                {
                                    return Ok(evaluator_result(
                                        result.value,
                                        /*is_syntactically_string*/ false,
                                        /*resolved_other_files*/ true,
                                        /*has_external_references*/ true,
                                    ));
                                }
                            }
                            return Ok(evaluator_result(
                                result.value,
                                result.is_syntactically_string,
                                result.resolved_other_files,
                                /*has_external_references*/ true,
                            ));
                        }
                    }
                }
            }
        }
        Ok(undefined_result())
    }

    /// tsc-port: evaluateElementAccessExpression @6.0.3
    /// tsc-hash: 45c3d37b5754f457a65e319c30fac0b69bf242d5a01e8b65ea5d3b6188afbfa7
    /// tsc-span: _tsc.js:85716-85738
    fn evaluate_element_access_expression(
        &mut self,
        expr: NodeId,
        location: Option<NodeId>,
    ) -> CheckResult2<EvaluatorResult> {
        let (root, argument) = match self.data_of(expr) {
            NodeData::ElementAccessExpression(data) => (data.expression, data.argument_expression),
            _ => unreachable!("element access callers check the kind"),
        };
        let (Some(root), Some(argument)) = (root, argument) else {
            return Ok(undefined_result());
        };
        let argument_text = match self.data_of(argument) {
            NodeData::StringLiteral(data) => data.text.clone(),
            NodeData::NoSubstitutionTemplateLiteral(data) => data.text.clone(),
            _ => return Ok(undefined_result()),
        };
        if !self.is_entity_name_expression(root) {
            return Ok(undefined_result());
        }
        let Some(root_symbol) =
            self.resolve_entity_name(root, SymbolFlags::VALUE, /*ignore_errors*/ true, None)
        else {
            return Ok(undefined_result());
        };
        if !self
            .binder
            .symbol(root_symbol)
            .flags
            .intersects(SymbolFlags::ENUM)
        {
            return Ok(undefined_result());
        }
        let name = tsrs2_binder::escape_leading_underscores(&argument_text);
        let member = self
            .binder
            .symbol(root_symbol)
            .exports
            .get(name.as_str())
            .copied();
        let Some(member) = member else {
            return Ok(undefined_result());
        };
        match location {
            Some(location) => self.evaluate_enum_member(expr, member, location),
            None => {
                let declaration = self
                    .binder
                    .symbol(member)
                    .value_declaration
                    .expect("enum member symbols have value declarations");
                self.get_enum_member_value(declaration)
            }
        }
    }

    /// tsc-port: evaluateEnumMember @6.0.3
    /// tsc-hash: 75d8099578f3d4b54b9efb87a6002d034d00ddbb151bb1a67325644ea23538c8
    /// tsc-span: _tsc.js:85739-85766
    fn evaluate_enum_member(
        &mut self,
        expr: NodeId,
        symbol: SymbolId,
        location: NodeId,
    ) -> CheckResult2<EvaluatorResult> {
        let declaration = self.binder.symbol(symbol).value_declaration;
        let Some(declaration) = declaration.filter(|&declaration| declaration != location) else {
            let display = self.symbol_display_name(symbol);
            self.error_at(
                Some(expr),
                &diagnostics::Property_0_is_used_before_being_assigned,
                &[&display],
            );
            return Ok(undefined_result());
        };
        if !self.is_block_scoped_name_declared_before_use(declaration, location)? {
            self.error_at(
                Some(expr),
                &diagnostics::A_member_initializer_in_a_enum_declaration_cannot_reference_members_declared_after_it_including_members_defined_in_other_enums,
                &[],
            );
            return Ok(evaluator_result(
                Some(EvalValue::Num(0.0)),
                false,
                false,
                false,
            ));
        }
        let value = self.get_enum_member_value(declaration)?;
        if self.parent_of(location) != self.parent_of(declaration) {
            return Ok(evaluator_result(
                value.value,
                value.is_syntactically_string,
                value.resolved_other_files,
                /*has_external_references*/ true,
            ));
        }
        Ok(value)
    }

    // ---- declared-before-use ----

    /// tsc-port: isBlockScopedNameDeclaredBeforeUse @6.0.3
    /// tsc-hash: 1c9c6287164fe185037750316b773c9939ba584ee6b7ae231c97e77861eb4479
    /// tsc-span: _tsc.js:47930-48089
    ///
    /// Evaluator slice. The cross-file arm reduces to `true`: the
    /// moduleKind conjunct needs an externalModuleIndicator either way
    /// and `!compilerOptions.outFile` is always true (outFile
    /// unmodeled), so the tsc disjunction short-circuits. The JSDoc
    /// usage-flag test is elided (no JSDoc nodes parse). Declaration
    /// kinds the evaluator cannot produce (BindingElement, classes,
    /// property/parameter-property declarations) escape.
    pub(crate) fn is_block_scoped_name_declared_before_use(
        &mut self,
        declaration: NodeId,
        usage: NodeId,
    ) -> CheckResult2<bool> {
        if self.binder.file_index_of_node(declaration) != self.binder.file_index_of_node(usage) {
            return Ok(true);
        }
        if self.is_in_type_query(usage) || self.is_in_ambient_or_type_node(usage) {
            return Ok(true);
        }
        let declaration_pos = self.pos_of(declaration);
        let usage_pos = self.pos_of(usage);
        if declaration_pos <= usage_pos && !self.is_unrealized_this_property_use(declaration, usage)
        {
            return match self.kind_of(declaration) {
                SyntaxKind::BindingElement => {
                    // 47949-47955: same-pattern uses compare binding
                    // elements positionally; other uses recurse on the
                    // enclosing VariableDeclaration.
                    let error_binding_element =
                        self.get_ancestor_of_kind_inclusive(usage, SyntaxKind::BindingElement);
                    if let Some(error_binding_element) = error_binding_element {
                        return Ok(error_binding_element != declaration
                            || self.pos_of(declaration) < self.pos_of(error_binding_element));
                    }
                    let variable_declaration = self
                        .get_ancestor_of_kind_inclusive(
                            declaration,
                            SyntaxKind::VariableDeclaration,
                        )
                        .ok_or_else(|| {
                            Unsupported::new(
                                "binding element outside a variable declaration \
                                 (parse recovery)",
                            )
                        })?;
                    self.is_block_scoped_name_declared_before_use(variable_declaration, usage)
                }
                SyntaxKind::VariableDeclaration => Ok(!self
                    .is_immediately_used_in_initializer_of_block_scoped_variable(
                        declaration,
                        usage,
                    )?),
                SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                    // 47957-47968: a same-position-band use is legal
                    // unless it sits inside the class's own computed
                    // property names or (standard) decorators.
                    Ok(self.class_use_before_declaration_is_legal(declaration, usage))
                }
                SyntaxKind::PropertyDeclaration => Err(Unsupported::new(
                    "property declared-before-use (2729 band, 5.5d)",
                )),
                SyntaxKind::Parameter => Err(Unsupported::new(
                    "parameter-property declared-before-use (2729 band, 5.5d)",
                )),
                _ => Ok(true),
            };
        }
        let usage_parent = self.parent_of(usage);
        if let Some(parent) = usage_parent {
            if self.kind_of(parent) == SyntaxKind::ExportSpecifier {
                return Ok(true);
            }
            if let NodeData::ExportAssignment(data) = self.data_of(parent) {
                if data.is_export_equals == Some(true) {
                    return Ok(true);
                }
            }
        }
        if let NodeData::ExportAssignment(data) = self.data_of(usage) {
            if data.is_export_equals == Some(true) {
                return Ok(true);
            }
        }
        if self.is_used_in_function_or_instance_property(usage, declaration)? {
            // 48046-48056: the emitStandardClassFields sub-arm requires
            // a property/parameter-property declaration — the kinds
            // above already escape, so this reduces to `true`.
            return Ok(true);
        }
        Ok(false)
    }

    /// The isClassLike arm of isBlockScopedNameDeclaredBeforeUse
    /// (47957-47968). legacyDecorators == experimentalDecorators, so
    /// the decorator disjuncts are live under the DEFAULT options.
    fn class_use_before_declaration_is_legal(&self, declaration: NodeId, usage: NodeId) -> bool {
        let legacy_decorators = self.options.experimental_decorators;
        let container = {
            let mut current = Some(usage);
            let mut found = None;
            while let Some(n) = current {
                if n == declaration {
                    break;
                }
                let hit = if self.kind_of(n) == SyntaxKind::ComputedPropertyName {
                    self.parent_of(n)
                        .and_then(|parent| self.parent_of(parent))
                        .is_some_and(|grand| grand == declaration)
                } else if !legacy_decorators && self.kind_of(n) == SyntaxKind::Decorator {
                    self.parent_of(n).is_some_and(|parent| {
                        if parent == declaration {
                            return true;
                        }
                        let grand = self.parent_of(parent);
                        match self.kind_of(parent) {
                            SyntaxKind::MethodDeclaration
                            | SyntaxKind::GetAccessor
                            | SyntaxKind::SetAccessor
                            | SyntaxKind::PropertyDeclaration => {
                                grand.is_some_and(|grand| grand == declaration)
                            }
                            SyntaxKind::Parameter => grand
                                .and_then(|grand| self.parent_of(grand))
                                .is_some_and(|great| great == declaration),
                            _ => false,
                        }
                    })
                } else {
                    false
                };
                if hit {
                    found = Some(n);
                    break;
                }
                current = self.parent_of(n);
            }
            found
        };
        let Some(container) = container else {
            return true;
        };
        if !legacy_decorators && self.kind_of(container) == SyntaxKind::Decorator {
            // Legal only when the use is wrapped in a non-IIFE
            // function between it and the decorator.
            let mut current = Some(usage);
            while let Some(n) = current {
                if n == container {
                    return false;
                }
                if node_util::is_function_like_kind(self.kind_of(n))
                    && self.get_immediately_invoked_function_expression(n).is_none()
                {
                    return true;
                }
                current = self.parent_of(n);
            }
            return false;
        }
        false
    }

    /// tsc getContainingClass (14487).
    fn containing_class_of(&self, node: NodeId) -> Option<NodeId> {
        node_util::get_containing_class(self.binder.source_of_node(node), node)
    }

    /// tsc getAncestor (12654) — nearest ancestor OR SELF of `kind`.
    fn get_ancestor_of_kind_inclusive(&self, node: NodeId, kind: SyntaxKind) -> Option<NodeId> {
        let mut current = Some(node);
        while let Some(n) = current {
            if self.kind_of(n) == kind {
                return Some(n);
            }
            current = self.parent_of(n);
        }
        None
    }

    /// The 47947 pos-guard tail: `isPropertyDeclaration(declaration) &&
    /// isThisProperty(usage.parent) && !declaration.initializer &&
    /// !declaration.exclamationToken`.
    fn is_unrealized_this_property_use(&self, declaration: NodeId, usage: NodeId) -> bool {
        let NodeData::PropertyDeclaration(data) = self.data_of(declaration) else {
            return false;
        };
        if data.initializer.is_some() || data.exclamation_token.is_some() {
            return false;
        }
        let Some(parent) = self.parent_of(usage) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::PropertyAccessExpression(access) => access
                .expression
                .is_some_and(|expression| self.kind_of(expression) == SyntaxKind::ThisKeyword),
            _ => false,
        }
    }

    /// tsc-port: isBlockScopedNameDeclaredBeforeUse.isImmediatelyUsedInInitializerOfBlockScopedVariable @6.0.3
    /// tsc-hash: 1c9c6287164fe185037750316b773c9939ba584ee6b7ae231c97e77861eb4479
    /// tsc-span: _tsc.js:47930-48089
    fn is_immediately_used_in_initializer_of_block_scoped_variable(
        &mut self,
        declaration: NodeId,
        usage: NodeId,
    ) -> CheckResult2<bool> {
        let decl_container = self.get_enclosing_block_scope_container(declaration);
        let parent = self.parent_of(declaration);
        let grandparent = parent.and_then(|parent| self.parent_of(parent));
        let Some(grandparent) = grandparent else {
            return Ok(false);
        };
        match self.kind_of(grandparent) {
            SyntaxKind::VariableStatement
            | SyntaxKind::ForStatement
            | SyntaxKind::ForOfStatement => {
                if self.is_same_scope_descendent_of(usage, Some(declaration), decl_container)? {
                    return Ok(true);
                }
            }
            _ => {}
        }
        match self.data_of(grandparent) {
            NodeData::ForInStatement(data) => {
                self.is_same_scope_descendent_of(usage, data.expression, decl_container)
            }
            NodeData::ForOfStatement(data) => {
                self.is_same_scope_descendent_of(usage, data.expression, decl_container)
            }
            _ => Ok(false),
        }
    }

    /// tsc-port: isBlockScopedNameDeclaredBeforeUse.isUsedInFunctionOrInstanceProperty @6.0.3
    /// tsc-hash: 1c9c6287164fe185037750316b773c9939ba584ee6b7ae231c97e77861eb4479
    /// tsc-span: _tsc.js:47930-48089
    ///
    /// Worker walk with the class-property and decorator arms escaped
    /// (evaluator usages are enum members and variable declarations,
    /// which never sit under property initializers or decorators).
    fn is_used_in_function_or_instance_property(
        &mut self,
        usage: NodeId,
        declaration: NodeId,
    ) -> CheckResult2<bool> {
        let decl_container = self.get_enclosing_block_scope_container(declaration);
        let mut current = Some(usage);
        while let Some(node) = current {
            if Some(node) == decl_container {
                return Ok(false);
            }
            if node_util::is_function_like_kind(self.kind_of(node)) {
                // findAncestor callback `!getIIFE(current)`: an IIFE
                // keeps climbing (its body runs immediately), any other
                // function-like defers the use — hit.
                if self.get_immediately_invoked_function_expression(node).is_none() {
                    return Ok(true);
                }
                current = self.parent_of(node);
                continue;
            }
            if self.kind_of(node) == SyntaxKind::ClassStaticBlockDeclaration {
                // findAncestor callback: `true` stops the walk with a
                // hit, `false` keeps climbing.
                if self.pos_of(declaration) < self.pos_of(usage) {
                    return Ok(true);
                }
                current = self.parent_of(node);
                continue;
            }
            if let Some(parent) = self.parent_of(node) {
                if let NodeData::PropertyDeclaration(data) = self.data_of(parent) {
                    if data.initializer == Some(node) {
                        if self.has_static_modifier(parent) {
                            if self.kind_of(declaration) == SyntaxKind::MethodDeclaration {
                                return Ok(true);
                            }
                            if self.kind_of(declaration) == SyntaxKind::PropertyDeclaration
                                && self.containing_class_of(usage)
                                    == self.containing_class_of(declaration)
                            {
                                // isPropertyInitializedInStaticBlocks
                                // needs getTypeOfSymbol — the 2729
                                // consumer band (5.5d); TDZ
                                // declarations are never properties.
                                return Err(Unsupported::new(
                                    "static-block property-initialization probe (2729 band, 5.5d)",
                                ));
                            }
                        } else {
                            let is_declaration_instance_property = self.kind_of(declaration)
                                == SyntaxKind::PropertyDeclaration
                                && !self.has_static_modifier(declaration);
                            if !is_declaration_instance_property
                                || self.containing_class_of(usage)
                                    != self.containing_class_of(declaration)
                            {
                                return Ok(true);
                            }
                        }
                    }
                }
                if let NodeData::Decorator(data) = self.data_of(parent) {
                    if data.expression == Some(node) {
                        // Recursion restarts the worker from the
                        // decorated declaration's container; a false
                        // answer QUITS the whole walk (tsc "quit").
                        if let Some(decorated) = self.parent_of(parent) {
                            match self.kind_of(decorated) {
                                SyntaxKind::Parameter => {
                                    let restart = self
                                        .parent_of(decorated)
                                        .and_then(|method| self.parent_of(method));
                                    return match restart {
                                        Some(restart) => self
                                            .is_used_in_function_or_instance_property(
                                                restart,
                                                declaration,
                                            ),
                                        None => Ok(false),
                                    };
                                }
                                SyntaxKind::MethodDeclaration => {
                                    let restart = self.parent_of(decorated);
                                    return match restart {
                                        Some(restart) => self
                                            .is_used_in_function_or_instance_property(
                                                restart,
                                                declaration,
                                            ),
                                        None => Ok(false),
                                    };
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            current = self.parent_of(node);
        }
        Ok(false)
    }

    /// tsc-port: isSameScopeDescendentOf @6.0.3
    /// tsc-hash: b6ffa36eb5f3ba52f245efbfc9dbd16db3995cc03e410d56a558c4efcbb3640f
    /// tsc-span: _tsc.js:48478-48480
    fn is_same_scope_descendent_of(
        &mut self,
        initial: NodeId,
        parent: Option<NodeId>,
        stop_at: Option<NodeId>,
    ) -> CheckResult2<bool> {
        let Some(parent) = parent else {
            return Ok(false);
        };
        let mut current = Some(initial);
        while let Some(node) = current {
            if node == parent {
                return Ok(true);
            }
            if Some(node) == stop_at {
                return Ok(false);
            }
            if node_util::is_function_like_kind(self.kind_of(node)) {
                match self.get_immediately_invoked_function_expression(node) {
                    None => return Ok(false),
                    Some(_) => {
                        if self.function_flags_async_generator(node) {
                            return Ok(false);
                        }
                    }
                }
            }
            current = self.parent_of(node);
        }
        Ok(false)
    }

    /// tsc-port: getImmediatelyInvokedFunctionExpression @6.0.3
    /// tsc-hash: 376116f5822935a0b930eb122871cf6a305990b92a0e03ce126f83914ed84686
    /// tsc-span: _tsc.js:14595-14607
    fn get_immediately_invoked_function_expression(&self, func: NodeId) -> Option<NodeId> {
        if !matches!(
            self.kind_of(func),
            SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
        ) {
            return None;
        }
        let mut prev = func;
        let mut parent = self.parent_of(func)?;
        while self.kind_of(parent) == SyntaxKind::ParenthesizedExpression {
            prev = parent;
            parent = self.parent_of(parent)?;
        }
        match self.data_of(parent) {
            NodeData::CallExpression(data) if data.expression == Some(prev) => Some(parent),
            _ => None,
        }
    }

    /// tsc-port: getFunctionFlags @6.0.3
    /// tsc-hash: e20bbca2eb9fbae4b851ad4664eb23bae2901c534f2b9b1eec097be1dc56c3fb
    /// tsc-span: _tsc.js:15810-15833
    ///
    /// `getFunctionFlags(n) & AsyncGenerator` — only the Async |
    /// Generator bits matter to isSameScopeDescendentOf.
    fn function_flags_async_generator(&self, node: NodeId) -> bool {
        let (asterisk, has_async) = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => (data.asterisk_token.is_some(), true),
            NodeData::FunctionExpression(data) => (data.asterisk_token.is_some(), true),
            NodeData::MethodDeclaration(data) => (data.asterisk_token.is_some(), true),
            NodeData::ArrowFunction(_) => (false, true),
            _ => (false, false),
        };
        if !has_async {
            return asterisk;
        }
        asterisk
            || node_util::has_syntactic_modifier(
                self.binder.source_of_node(node),
                node,
                ModifierFlags::ASYNC,
            )
    }

    /// tsc-port: getEnclosingBlockScopeContainer @6.0.3
    /// tsc-hash: 50444054506d87acb188cbcd3ed441a6c57e41352eda843ae6f0840bbbb1cc07
    /// tsc-span: _tsc.js:13844-13846
    pub(crate) fn get_enclosing_block_scope_container(&self, node: NodeId) -> Option<NodeId> {
        let mut current = self.parent_of(node);
        while let Some(candidate) = current {
            let parent = self.parent_of(candidate);
            if self.is_block_scope(candidate, parent) {
                return Some(candidate);
            }
            current = parent;
        }
        None
    }

    /// tsc-port: isBlockScope @6.0.3
    /// tsc-hash: 0f16e21e5d49b19429a2c7da58f19407ff27166aa2072a90ef6837af5b6f5d05
    /// tsc-span: _tsc.js:13786-13809
    fn is_block_scope(&self, node: NodeId, parent: Option<NodeId>) -> bool {
        match self.kind_of(node) {
            SyntaxKind::SourceFile
            | SyntaxKind::CaseBlock
            | SyntaxKind::CatchClause
            | SyntaxKind::ModuleDeclaration
            | SyntaxKind::ForStatement
            | SyntaxKind::ForInStatement
            | SyntaxKind::ForOfStatement
            | SyntaxKind::Constructor
            | SyntaxKind::MethodDeclaration
            | SyntaxKind::GetAccessor
            | SyntaxKind::SetAccessor
            | SyntaxKind::FunctionDeclaration
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::PropertyDeclaration
            | SyntaxKind::ClassStaticBlockDeclaration => true,
            SyntaxKind::Block => !parent.is_some_and(|parent| {
                node_util::is_function_like_kind(self.kind_of(parent))
                    || self.kind_of(parent) == SyntaxKind::ClassStaticBlockDeclaration
            }),
            _ => false,
        }
    }

    /// tsc-port: isInTypeQuery @6.0.3
    /// tsc-hash: 8785d47c907901c016e44b54eafcb54081198d3feba0af1af3d51531d273d52d
    /// tsc-span: _tsc.js:16701-16706
    pub(crate) fn is_in_type_query(&self, node: NodeId) -> bool {
        let mut current = Some(node);
        while let Some(node) = current {
            match self.kind_of(node) {
                SyntaxKind::TypeQuery => return true,
                SyntaxKind::Identifier | SyntaxKind::QualifiedName => {}
                _ => return false,
            }
            current = self.parent_of(node);
        }
        false
    }

    /// tsc-port: isInAmbientOrTypeNode @6.0.3
    /// tsc-hash: bcea35c9b9ab2de5c35c6aaea990ce25eb0171c7cf514a166a40c0c2675d60b2
    /// tsc-span: _tsc.js:69404-69406
    pub(crate) fn is_in_ambient_or_type_node(&self, node: NodeId) -> bool {
        if self.node_flags(node) & NodeFlags::AMBIENT.bits() != 0 {
            return true;
        }
        let mut current = Some(node);
        while let Some(node) = current {
            if matches!(
                self.kind_of(node),
                SyntaxKind::InterfaceDeclaration
                    | SyntaxKind::TypeAliasDeclaration
                    | SyntaxKind::TypeLiteral
            ) {
                return true;
            }
            current = self.parent_of(node);
        }
        false
    }

    // ---- small shared helpers ----

    pub(crate) fn pos_of(&self, node: NodeId) -> u32 {
        self.binder.source_of_node(node).arena.node(node).pos
    }

    /// tsc-port: skipParentheses @6.0.3
    /// tsc-hash: 57477e009374b3ffadffee5b4db7695a3c33fc1710ed92ce3c06ab51d147e7f3
    /// tsc-span: _tsc.js:15661-15664
    ///
    /// OuterExpressionKinds.Parentheses only — the evaluator never
    /// passes excludeJSDocTypeAssertions.
    fn skip_parentheses(&self, node: NodeId) -> NodeId {
        let mut node = node;
        while let NodeData::ParenthesizedExpression(data) = self.data_of(node) {
            match data.expression {
                Some(expression) => node = expression,
                None => break,
            }
        }
        node
    }

    /// tsc-port: isComputedNonLiteralName @6.0.3
    /// tsc-hash: 3d7ec42dcf0260b3223c227752413c3bb90ef31f7e40edbbf523205e2cde53ea
    /// tsc-span: _tsc.js:13860-13862
    pub(crate) fn is_computed_non_literal_name(&self, name: NodeId) -> bool {
        match self.data_of(name) {
            NodeData::ComputedPropertyName(data) => !data.expression.is_some_and(|expression| {
                node_util::is_string_or_numeric_literal_like(
                    self.binder.source_of_node(expression),
                    expression,
                )
            }),
            _ => false,
        }
    }

    /// tsc-port: getTextOfPropertyName @6.0.3
    /// tsc-hash: b6a070f28394bc21fdf62f528c96dee4a10060472d748602fd0bba75be70e0cd
    /// tsc-span: _tsc.js:13883-13885
    fn get_text_of_property_name(&self, name: NodeId) -> CheckResult2<String> {
        let source = self.binder.source_of_node(name);
        if let NodeData::ComputedPropertyName(data) = self.data_of(name) {
            let expression = data
                .expression
                .ok_or_else(|| Unsupported::new("computed property name missing expression"))?;
            return node_util::get_escaped_text_of_identifier_or_literal(source, expression)
                .ok_or_else(|| {
                    Unsupported::new("non-literal computed property name text (5.5)")
                });
        }
        node_util::get_escaped_text_of_identifier_or_literal(source, name)
            .ok_or_else(|| Unsupported::new("property name shape without literal text (5.5)"))
    }

    /// tsc-port: isEnumConst @6.0.3
    /// tsc-hash: 4f77dd246f3d44f2c2d865ed0dc45bba90d3db5bfef3641db124643e7b77e6a2
    /// tsc-span: _tsc.js:14125-14127
    pub(crate) fn is_enum_const(&self, node: NodeId) -> bool {
        node_util::get_combined_modifier_flags(self.binder.source_of_node(node), node)
            .intersects(ModifierFlags::CONST)
    }

    /// tsc-port: isConstantVariable @6.0.3
    /// tsc-hash: 54585a38cdbdd065d6cae1a89a5ef4a96623269d5c0203ae1f825e741edf7eda
    /// tsc-span: _tsc.js:71592-71594
    ///
    /// getDeclarationNodeFlagsFromSymbol = valueDeclaration COMBINED
    /// node flags (Const lives on the VariableDeclarationList);
    /// NodeFlags.Constant = Const | Using.
    fn is_constant_variable(&self, symbol: SymbolId) -> bool {
        let data = self.binder.symbol(symbol);
        if !data.flags.intersects(SymbolFlags::VARIABLE) {
            return false;
        }
        let Some(declaration) = data.value_declaration else {
            return false;
        };
        let combined = node_util::get_combined_node_flags(
            self.binder.source_of_node(declaration),
            declaration,
        );
        combined.intersects(NodeFlags::from_bits(
            NodeFlags::CONST.bits() | NodeFlags::USING.bits(),
        ))
    }

    /// tsc-port: getDeclarationOfKind @6.0.3
    /// tsc-hash: d34933434824a0ff76b3eb034566feb42e6054c05caf812831c45ba8aed59e3c
    /// tsc-span: _tsc.js:12642-12652
    pub(crate) fn get_declaration_of_kind(
        &self,
        symbol: SymbolId,
        kind: SyntaxKind,
    ) -> Option<NodeId> {
        self.binder
            .symbol(symbol)
            .declarations
            .iter()
            .copied()
            .find(|&declaration| self.kind_of(declaration) == kind)
    }
}

/// tsc-port: isNumericLiteralName @6.0.3
/// tsc-hash: 792c3a97db611b31a75c5d2ee921c6788c6a6ccbfbd6a3240b943e3f98205802
/// tsc-span: _tsc.js:19205-19207
///
/// `(+name).toString() === name` — JS ToNumber over the name string,
/// round-tripped through Number#toString.
fn is_numeric_literal_name(name: &str) -> bool {
    tsrs2_types::js_number_to_string(js_string_to_number(name)) == name
}

/// tsc-port: isInfinityOrNaNString @6.0.3
/// tsc-hash: 0e62fa3d1eedc96edada87bf439e9b1e08da6114503b430fd3b76fcc9ec063ef
/// tsc-span: _tsc.js:19196-19198
fn is_infinity_or_nan_string(name: &str) -> bool {
    name == "Infinity" || name == "-Infinity" || name == "NaN"
}

/// JS unary `+string` (ToNumber on a string): empty/whitespace → 0,
/// 0x/0o/0b integer forms, signed decimal/Infinity forms, else NaN.
/// Rust's f64 parser accepts "inf"/"infinity"/"nan" spellings that JS
/// rejects, so those pass through the explicit special-form table.
fn js_string_to_number(text: &str) -> f64 {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return 0.0;
    }
    match trimmed {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(digits) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        return u128::from_str_radix(digits, 16).map_or(f64::NAN, |value| value as f64);
    }
    if let Some(digits) = trimmed.strip_prefix("0o").or_else(|| trimmed.strip_prefix("0O")) {
        return u128::from_str_radix(digits, 8).map_or(f64::NAN, |value| value as f64);
    }
    if let Some(digits) = trimmed.strip_prefix("0b").or_else(|| trimmed.strip_prefix("0B")) {
        return u128::from_str_radix(digits, 2).map_or(f64::NAN, |value| value as f64);
    }
    if trimmed
        .chars()
        .any(|c| !matches!(c, '0'..='9' | '.' | 'e' | 'E' | '+' | '-'))
    {
        return f64::NAN;
    }
    trimmed.parse::<f64>().unwrap_or(f64::NAN)
}

/// ECMAScript ToInt32 (used by `| & ^ ~ << >>`).
fn js_to_int32(value: f64) -> i32 {
    js_to_uint32(value) as i32
}

/// ECMAScript ToUint32 (used by `>>>` and shift counts).
fn js_to_uint32(value: f64) -> u32 {
    if !value.is_finite() || value == 0.0 {
        return 0;
    }
    let truncated = value.trunc();
    let modulo = truncated.rem_euclid(4294967296.0);
    modulo as u32
}

/// JS `**` (Number::exponentiate) deviates from IEEE `pow`: a NaN
/// exponent and |base| = 1 with an infinite exponent are NaN.
fn js_pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() {
        return f64::NAN;
    }
    if exponent.is_infinite() && base.abs() == 1.0 {
        return f64::NAN;
    }
    base.powf(exponent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_to_int32_wraps_like_ecmascript() {
        assert_eq!(js_to_int32(0.0), 0);
        assert_eq!(js_to_int32(-0.0), 0);
        assert_eq!(js_to_int32(3.9), 3);
        assert_eq!(js_to_int32(-3.9), -3);
        assert_eq!(js_to_int32(4294967296.0), 0);
        assert_eq!(js_to_int32(4294967295.0), -1);
        assert_eq!(js_to_int32(2147483648.0), -2147483648);
        assert_eq!(js_to_int32(f64::NAN), 0);
        assert_eq!(js_to_int32(f64::INFINITY), 0);
    }

    #[test]
    fn js_pow_matches_ecmascript_special_cases() {
        assert!(js_pow(1.0, f64::INFINITY).is_nan());
        assert!(js_pow(-1.0, f64::NEG_INFINITY).is_nan());
        assert!(js_pow(2.0, f64::NAN).is_nan());
        assert_eq!(js_pow(f64::NAN, 0.0), 1.0);
        assert_eq!(js_pow(2.0, 10.0), 1024.0);
    }

    #[test]
    fn numeric_literal_names_round_trip() {
        assert!(is_numeric_literal_name("0"));
        assert!(is_numeric_literal_name("10"));
        assert!(is_numeric_literal_name("1.5"));
        assert!(is_numeric_literal_name("-1"));
        assert!(is_numeric_literal_name("Infinity"));
        assert!(is_numeric_literal_name("NaN"));
        assert!(!is_numeric_literal_name("1.0"));
        assert!(!is_numeric_literal_name("01"));
        assert!(!is_numeric_literal_name("1e2")); // "100" round-trip
        assert!(!is_numeric_literal_name("A"));
        assert!(!is_numeric_literal_name(""));
    }
}
