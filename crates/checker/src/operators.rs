//! M4 5.5e: the operator band (extraction doc §7) — the generic binary
//! trampoline + checkBinaryExpression machine, the nullish-coalescing
//! grammar/semantics probes, the truthiness classifiers, and (in later
//! slices of 5.5e) the checkBinaryLikeExpressionWorker operator arms,
//! destructuring assignment family, unary band, conditional/template,
//! assertions/satisfies, instantiation expressions, meta-properties and
//! the instanceof/in slices.
//!
//! The trampoline shape is MANDATORY (extraction §7): tsc walks nested
//! binary chains iteratively over parallel state/node stacks with ONE
//! shared user-state record — a recursive checkBinary would overflow on
//! the same deep chains tsc handles, and the checkExpression call ORDER
//! (left-to-right, one eager call per non-binary operand) is observable
//! through type interning.

use tsc_binder::node_util;
use tsc_syntax::{NodeData, NodeId, SyntaxKind};
use tsc_types::{
    CheckMode, IterationUse, LiteralValue, SymbolFlags, SymbolId, TypeData, TypeFacts, TypeFlags,
    TypeId,
};

use crate::state::{CheckResult, CheckerState};
use crate::structural::SignatureKind;

/// tsc OuterExpressionKinds (isOuterExpression 27561): the checker
/// consumers use All (63), Assertions|Parentheses (39), and the sign
/// bit to retain JSDoc type-assertion parentheses.
#[derive(Clone, Copy)]
pub(crate) struct OuterExpressionKinds(pub i32);

impl OuterExpressionKinds {
    pub(crate) const PARENTHESES: Self = Self(1);
    pub(crate) const TYPE_ASSERTIONS: Self = Self(2);
    pub(crate) const NON_NULL_ASSERTIONS: Self = Self(4);
    // PartiallyEmittedExpression nodes never parse (transform-only
    // kind) — the bit stays for the ALL mask arithmetic.
    #[allow(dead_code)]
    pub(crate) const PARTIALLY_EMITTED_EXPRESSIONS: Self = Self(8);
    pub(crate) const EXPRESSIONS_WITH_TYPE_ARGUMENTS: Self = Self(16);
    pub(crate) const SATISFIES: Self = Self(32);
    pub(crate) const ASSERTIONS: Self = Self(Self::TYPE_ASSERTIONS.0 | Self::NON_NULL_ASSERTIONS.0);
    pub(crate) const ALL: Self = Self(63);
    pub(crate) const EXCLUDE_JSDOC_TYPE_ASSERTIONS: Self = Self(i32::MIN);

    fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }
}

/// tsc SyntacticNullishnessSemantics / SyntacticTruthySemantics
/// (anonymous const enums at their classifiers): Always=1, Never=2,
/// Sometimes=3; the conditional arms BIT-OR the two branches.
const SEMANTICS_ALWAYS: u8 = 1;
const SEMANTICS_NEVER: u8 = 2;
const SEMANTICS_SOMETIMES: u8 = 3;

/// tokenToString for operator tokens — every kind this band passes is
/// in the generated textToToken reverse map.
fn token_text(kind: SyntaxKind) -> &'static str {
    tsc_syntax::tokens::token_to_string(kind).expect("operator tokens have token text")
}

/// createCheckBinaryExpression's user state (79827-79832): ONE record
/// shared by every frame of a trampoline run. leftType lives at
/// typeStack[stackIndex], lastResult at typeStack[stackIndex + 1].
struct BinaryCheckState {
    check_mode: CheckMode,
    skip: bool,
    stack_index: usize,
    type_stack: Vec<Option<TypeId>>,
}

impl BinaryCheckState {
    fn left_type(&self) -> Option<TypeId> {
        self.type_stack.get(self.stack_index).copied().flatten()
    }

    fn set_left_type(&mut self, ty: Option<TypeId>) {
        self.ensure(self.stack_index);
        let index = self.stack_index;
        self.type_stack[index] = ty;
    }

    fn last_result(&self) -> Option<TypeId> {
        self.type_stack.get(self.stack_index + 1).copied().flatten()
    }

    fn set_last_result(&mut self, ty: Option<TypeId>) {
        self.ensure(self.stack_index + 1);
        let index = self.stack_index + 1;
        self.type_stack[index] = ty;
    }

    fn ensure(&mut self, index: usize) {
        if self.type_stack.len() <= index {
            self.type_stack.resize(index + 1, None);
        }
    }
}

/// The trampoline's per-frame progression (BinaryExpressionState
/// 27945-28047). The checker machine defines every hook, so nextState
/// walks Enter → Left → Operator → Right → Exit linearly.
#[derive(Clone, Copy, PartialEq)]
enum BinaryState {
    Enter,
    Left,
    Operator,
    Right,
    Exit,
}

impl<'a> CheckerState<'a> {
    /// The binary node's (left, operatorToken, right). tsc's parser
    /// fills a missing operand with a missing identifier; a synthetic
    /// recovery tree may instead omit one of the slots, so consumers
    /// choose the recovery result appropriate to their return type.
    fn binary_parts(&self, node: NodeId) -> Option<(NodeId, NodeId, NodeId)> {
        let NodeData::BinaryExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        match (data.left, data.operator_token, data.right) {
            (Some(left), Some(op), Some(right)) => Some((left, op, right)),
            _ => None,
        }
    }

    fn operator_kind(&self, operator_token: NodeId) -> SyntaxKind {
        self.kind_of(operator_token)
    }

    /// tsc-port: createBinaryExpressionTrampoline @6.0.3 (specialized
    /// to the checker machine — the only instance at 5.5)
    /// tsc-hash: 241b9f9315e662617830e0f95f69e9b2d3c0df3c7a76fd3fe4e5caf9a5c34ccb
    /// tsc-span: _tsc.js:28048-28071
    ///
    /// tsc-port: createCheckBinaryExpression @6.0.3
    /// tsc-hash: bbbf9bef171632bd6b6df91a43e42605d3d42ed493a65f50b2334d38af83c3ff
    /// tsc-span: _tsc.js:79810-79935
    ///
    /// A checker abort anywhere in the walk unwinds the whole binary
    /// expression, matching recursive containment in the other bands.
    pub(crate) fn check_binary_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let mut state_stack: Vec<BinaryState> = vec![BinaryState::Enter];
        let mut node_stack: Vec<NodeId> = vec![node];
        let mut user: Option<BinaryCheckState> = None;
        let mut stack_index: usize = 0;
        loop {
            let frame_node = node_stack[stack_index];
            match state_stack[stack_index] {
                BinaryState::Enter => {
                    state_stack[stack_index] = BinaryState::Left;
                    self.binary_on_enter(frame_node, &mut user, check_mode)?;
                }
                BinaryState::Left => {
                    // Advance THEN maybe-push (BinaryExpressionState.left).
                    state_stack[stack_index] = BinaryState::Operator;
                    let Some((left, _, _)) = self.binary_parts(frame_node) else {
                        return Ok(self.tables.intrinsics.error);
                    };
                    let state = user.as_mut().expect("onEnter ran");
                    if !state.skip {
                        if let Some(next) = self.binary_maybe_check_expression(&mut user, left)? {
                            stack_index += 1;
                            state_stack.truncate(stack_index);
                            node_stack.truncate(stack_index);
                            state_stack.push(BinaryState::Enter);
                            node_stack.push(next);
                        }
                    }
                }
                BinaryState::Operator => {
                    state_stack[stack_index] = BinaryState::Right;
                    self.binary_on_operator(frame_node, user.as_mut().expect("onEnter ran"))?;
                }
                BinaryState::Right => {
                    state_stack[stack_index] = BinaryState::Exit;
                    let Some((_, _, right)) = self.binary_parts(frame_node) else {
                        return Ok(self.tables.intrinsics.error);
                    };
                    let state = user.as_mut().expect("onEnter ran");
                    if !state.skip {
                        if let Some(next) = self.binary_maybe_check_expression(&mut user, right)? {
                            stack_index += 1;
                            state_stack.truncate(stack_index);
                            node_stack.truncate(stack_index);
                            state_stack.push(BinaryState::Enter);
                            node_stack.push(next);
                        }
                    }
                }
                BinaryState::Exit => {
                    let result =
                        self.binary_on_exit(frame_node, user.as_mut().expect("onEnter ran"))?;
                    if stack_index > 0 {
                        stack_index -= 1;
                        // foldState: setLastResult(state, result).
                        user.as_mut()
                            .expect("shared record")
                            .set_last_result(Some(result));
                    } else {
                        return Ok(result);
                    }
                }
            }
        }
    }

    /// onEnter (79817-79850): re-entry bumps the shared record; a JS
    /// expando initializer checks only its right side; `=` with an
    /// object/array-literal LHS routes to the destructuring family and
    /// skips the operand walk.
    fn binary_on_enter(
        &mut self,
        node: NodeId,
        user: &mut Option<BinaryCheckState>,
        check_mode: CheckMode,
    ) -> CheckResult<()> {
        match user {
            Some(state) => {
                state.stack_index += 1;
                state.skip = false;
                state.set_left_type(None);
                state.set_last_result(None);
            }
            None => {
                *user = Some(BinaryCheckState {
                    check_mode,
                    skip: false,
                    stack_index: 0,
                    type_stack: vec![None, None],
                });
            }
        }
        let Some((left, operator_token, right)) = self.binary_parts(node) else {
            let state = user.as_mut().expect("created above");
            state.skip = true;
            state.set_last_result(Some(self.tables.intrinsics.error));
            return Ok(());
        };
        if self.is_in_js_file(node)
            && tsc_binder::assignment::get_assigned_expando_initializer(
                self.binder.source_of_node(node),
                node,
            )
            .is_some()
        {
            let result = self.check_expression(right, check_mode)?;
            let state = user.as_mut().expect("created above");
            state.skip = true;
            state.set_last_result(Some(result));
            return Ok(());
        }
        self.check_nullish_coalesce_operands(node)?;
        let operator = self.operator_kind(operator_token);
        if operator == SyntaxKind::EqualsToken
            && matches!(
                self.kind_of(left),
                SyntaxKind::ObjectLiteralExpression | SyntaxKind::ArrayLiteralExpression
            )
        {
            let state = user.as_mut().expect("created above");
            state.skip = true;
            let check_mode = state.check_mode;
            let right_type = self.check_expression(right, check_mode)?;
            let right_is_this = self.kind_of(right) == SyntaxKind::ThisKeyword;
            let result =
                self.check_destructuring_assignment(left, right_type, check_mode, right_is_this)?;
            user.as_mut()
                .expect("created above")
                .set_last_result(Some(result));
        }
        Ok(())
    }

    /// onOperator (79857-79878): stash leftType, then the logical-band
    /// probes — the `&&`/if-parent truthy-callable check and the
    /// binary-logical truthiness classifier.
    fn binary_on_operator(
        &mut self,
        node: NodeId,
        state: &mut BinaryCheckState,
    ) -> CheckResult<()> {
        if state.skip {
            return Ok(());
        }
        let left_type = state
            .last_result()
            .expect("left operand checked before operator");
        state.set_left_type(Some(left_type));
        state.set_last_result(None);
        let Some((left, operator_token, _)) = self.binary_parts(node) else {
            state.skip = true;
            state.set_last_result(Some(self.tables.intrinsics.error));
            return Ok(());
        };
        let operator = self.operator_kind(operator_token);
        if node_util::is_logical_or_coalescing_binary_operator(operator) {
            let mut parent = self.parent_of(node);
            while let Some(p) = parent {
                let is_lifting = self.kind_of(p) == SyntaxKind::ParenthesizedExpression
                    || node_util::is_logical_or_coalescing_binary_expression(
                        self.binder.source_of_node(p),
                        p,
                    );
                if !is_lifting {
                    break;
                }
                parent = self.parent_of(p);
            }
            let if_parent = parent.filter(|&p| self.kind_of(p) == SyntaxKind::IfStatement);
            if operator == SyntaxKind::AmpersandAmpersandToken || if_parent.is_some() {
                let body = if_parent.and_then(|p| match self.data_of(p) {
                    NodeData::IfStatement(data) => data.then_statement,
                    _ => None,
                });
                self.check_testing_known_truthy_callable_or_awaitable_or_enum_member_type(
                    left, left_type, body,
                )?;
            }
            if matches!(
                operator,
                SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
            ) {
                self.check_truthiness_of_type(left_type, left)?;
            }
        }
        Ok(())
    }

    /// onExit (79884-79906): skip frames pass lastResult through;
    /// checked frames fold operand types through the worker.
    fn binary_on_exit(
        &mut self,
        node: NodeId,
        state: &mut BinaryCheckState,
    ) -> CheckResult<TypeId> {
        let result = if state.skip {
            state.last_result().expect("skip frames stash their result")
        } else {
            let left_type = state.left_type().expect("operator stashed leftType");
            let right_type = state
                .last_result()
                .expect("right operand checked before exit");
            match self.binary_parts(node) {
                Some((left, operator_token, right)) => self.check_binary_like_expression_worker(
                    left,
                    operator_token,
                    right,
                    left_type,
                    right_type,
                    state.check_mode,
                    Some(node),
                )?,
                None => self.tables.intrinsics.error,
            }
        };
        state.skip = false;
        state.set_left_type(None);
        state.set_last_result(None);
        state.stack_index = state.stack_index.wrapping_sub(1);
        Ok(result)
    }

    /// maybeCheckExpression (79917-79922): the ONLY recursion into
    /// checkExpression — binary children become frames instead.
    fn binary_maybe_check_expression(
        &mut self,
        user: &mut Option<BinaryCheckState>,
        node: NodeId,
    ) -> CheckResult<Option<NodeId>> {
        if self.kind_of(node) == SyntaxKind::BinaryExpression {
            if self.binary_parts(node).is_some() {
                return Ok(Some(node));
            }
            user.as_mut()
                .expect("onEnter ran")
                .set_last_result(Some(self.tables.intrinsics.error));
            return Ok(None);
        }
        let check_mode = user.as_ref().expect("onEnter ran").check_mode;
        let ty = self.check_expression(node, check_mode)?;
        user.as_mut()
            .expect("onEnter ran")
            .set_last_result(Some(ty));
        Ok(None)
    }

    /// tsc-port: checkBinaryLikeExpression @6.0.3
    /// tsc-hash: 96659818f3d32c4fb7489aee39a9b5497cfb883882893272654c766e3c32bb9a
    /// tsc-span: _tsc.js:80009-80022
    ///
    /// The non-trampoline entry for synthetic `name = initializer`
    /// pairs (destructuring defaults): operands are checked eagerly
    /// (they are never deep chains).
    pub(crate) fn check_binary_like_expression(
        &mut self,
        left: NodeId,
        operator_token: NodeId,
        right: NodeId,
        check_mode: CheckMode,
        error_node: Option<NodeId>,
    ) -> CheckResult<TypeId> {
        let operator = self.operator_kind(operator_token);
        if operator == SyntaxKind::EqualsToken
            && matches!(
                self.kind_of(left),
                SyntaxKind::ObjectLiteralExpression | SyntaxKind::ArrayLiteralExpression
            )
        {
            let right_type = self.check_expression(right, check_mode)?;
            let right_is_this = self.kind_of(right) == SyntaxKind::ThisKeyword;
            return self.check_destructuring_assignment(
                left,
                right_type,
                check_mode,
                right_is_this,
            );
        }
        // isBinaryLogicalOperator (80015): && and || ONLY — a `??`
        // left is checked plainly (no truthiness classification).
        let left_type = if matches!(
            operator,
            SyntaxKind::AmpersandAmpersandToken | SyntaxKind::BarBarToken
        ) {
            self.check_truthiness_expression(left, check_mode)?
        } else {
            self.check_expression(left, check_mode)?
        };
        let right_type = self.check_expression(right, check_mode)?;
        self.check_binary_like_expression_worker(
            left,
            operator_token,
            right,
            left_type,
            right_type,
            check_mode,
            error_node,
        )
    }

    /// tsc-port: checkBinaryLikeExpressionWorker @6.0.3
    /// tsc-hash: f9d5dc4fc061ee0c46e1fd2bb1db949b13e33384d59d0b90dbc3d10bcf7784ce
    /// tsc-span: _tsc.js:80023-80262
    #[allow(clippy::too_many_arguments)]
    fn check_binary_like_expression_worker(
        &mut self,
        left: NodeId,
        operator_token: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        check_mode: CheckMode,
        error_node: Option<NodeId>,
    ) -> CheckResult<TypeId> {
        let operator = self.operator_kind(operator_token);
        let silent_never = self.tables.intrinsics.silent_never;
        match operator {
            SyntaxKind::AsteriskToken
            | SyntaxKind::AsteriskAsteriskToken
            | SyntaxKind::AsteriskEqualsToken
            | SyntaxKind::AsteriskAsteriskEqualsToken
            | SyntaxKind::SlashToken
            | SyntaxKind::SlashEqualsToken
            | SyntaxKind::PercentToken
            | SyntaxKind::PercentEqualsToken
            | SyntaxKind::MinusToken
            | SyntaxKind::MinusEqualsToken
            | SyntaxKind::LessThanLessThanToken
            | SyntaxKind::LessThanLessThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanToken
            | SyntaxKind::GreaterThanGreaterThanEqualsToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanToken
            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
            | SyntaxKind::BarToken
            | SyntaxKind::BarEqualsToken
            | SyntaxKind::CaretToken
            | SyntaxKind::CaretEqualsToken
            | SyntaxKind::AmpersandToken
            | SyntaxKind::AmpersandEqualsToken => {
                if left_type == silent_never || right_type == silent_never {
                    return Ok(silent_never);
                }
                let left_type = self.check_non_null_type(left_type, left)?;
                let right_type = self.check_non_null_type(right_type, right)?;
                let suggested_operator = if self
                    .tables
                    .flags_of(left_type)
                    .intersects(TypeFlags::BOOLEAN_LIKE)
                    && self
                        .tables
                        .flags_of(right_type)
                        .intersects(TypeFlags::BOOLEAN_LIKE)
                {
                    Self::get_suggested_boolean_operator(operator)
                } else {
                    None
                };
                if let Some(suggested) = suggested_operator {
                    let err_node = error_node.unwrap_or(operator_token);
                    self.error_at(
                        Some(err_node),
                        &tsc_diagnostics::gen::The_0_operator_is_not_allowed_for_boolean_types_Consider_using_1_instead,
                        &[token_text(operator), token_text(suggested)],
                    );
                    return Ok(self.tables.intrinsics.number);
                }
                let left_ok = self.check_arithmetic_operand_type(
                    left,
                    left_type,
                    &tsc_diagnostics::gen::The_left_hand_side_of_an_arithmetic_operation_must_be_of_type_any_number_bigint_or_an_enum_type,
                    true,
                )?;
                let right_ok = self.check_arithmetic_operand_type(
                    right,
                    right_type,
                    &tsc_diagnostics::gen::The_right_hand_side_of_an_arithmetic_operation_must_be_of_type_any_number_bigint_or_an_enum_type,
                    true,
                )?;
                let result_type;
                if (self.is_type_assignable_to_kind(left_type, TypeFlags::ANY_OR_UNKNOWN, false)?
                    && self.is_type_assignable_to_kind(
                        right_type,
                        TypeFlags::ANY_OR_UNKNOWN,
                        false,
                    )?)
                    // Or, if neither could be bigint, implicit
                    // coercion results in a number result.
                    || !(self.maybe_type_of_kind(left_type, TypeFlags::BIG_INT_LIKE)
                        || self.maybe_type_of_kind(right_type, TypeFlags::BIG_INT_LIKE))
                {
                    result_type = self.tables.intrinsics.number;
                } else if self.both_are_bigint_like(left_type, right_type)? {
                    match operator {
                        SyntaxKind::GreaterThanGreaterThanGreaterThanToken
                        | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken => {
                            self.report_operator_error(
                                operator_token,
                                left,
                                right,
                                left_type,
                                right_type,
                                error_node,
                                None,
                            )?;
                        }
                        SyntaxKind::AsteriskAsteriskToken
                        | SyntaxKind::AsteriskAsteriskEqualsToken => {
                            if self.options.emit_script_target().bits()
                                < tsc_types::ScriptTarget::ES2016.bits()
                            {
                                self.error_at(
                                    error_node,
                                    &tsc_diagnostics::gen::Exponentiation_cannot_be_performed_on_bigint_values_unless_the_target_option_is_set_to_es2016_or_later,
                                    &[],
                                );
                            }
                        }
                        _ => {}
                    }
                    result_type = self.tables.intrinsics.bigint;
                } else {
                    self.report_operator_error(
                        operator_token,
                        left,
                        right,
                        left_type,
                        right_type,
                        error_node,
                        Some(&mut |state, l, r| state.both_are_bigint_like(l, r)),
                    )?;
                    result_type = self.tables.intrinsics.error;
                }
                if left_ok && right_ok {
                    self.check_assignment_operator(
                        left,
                        operator_token,
                        right,
                        left_type,
                        result_type,
                    )?;
                    if matches!(
                        operator,
                        SyntaxKind::LessThanLessThanToken
                            | SyntaxKind::LessThanLessThanEqualsToken
                            | SyntaxKind::GreaterThanGreaterThanToken
                            | SyntaxKind::GreaterThanGreaterThanEqualsToken
                            | SyntaxKind::GreaterThanGreaterThanGreaterThanToken
                            | SyntaxKind::GreaterThanGreaterThanGreaterThanEqualsToken
                    ) {
                        self.check_shift_simplification(
                            left,
                            operator,
                            right,
                            error_node,
                            operator_token,
                        )?;
                    }
                }
                Ok(result_type)
            }
            SyntaxKind::PlusToken | SyntaxKind::PlusEqualsToken => {
                if left_type == silent_never || right_type == silent_never {
                    return Ok(silent_never);
                }
                let (left_type, right_type) =
                    if !self.is_type_assignable_to_kind(left_type, TypeFlags::STRING_LIKE, false)?
                        && !self.is_type_assignable_to_kind(
                            right_type,
                            TypeFlags::STRING_LIKE,
                            false,
                        )?
                    {
                        (
                            self.check_non_null_type(left_type, left)?,
                            self.check_non_null_type(right_type, right)?,
                        )
                    } else {
                        (left_type, right_type)
                    };
                let mut result_type: Option<TypeId> = None;
                if self.is_type_assignable_to_kind(left_type, TypeFlags::NUMBER_LIKE, true)?
                    && self.is_type_assignable_to_kind(right_type, TypeFlags::NUMBER_LIKE, true)?
                {
                    result_type = Some(self.tables.intrinsics.number);
                } else if self.is_type_assignable_to_kind(
                    left_type,
                    TypeFlags::BIG_INT_LIKE,
                    true,
                )? && self.is_type_assignable_to_kind(
                    right_type,
                    TypeFlags::BIG_INT_LIKE,
                    true,
                )? {
                    result_type = Some(self.tables.intrinsics.bigint);
                } else if self.is_type_assignable_to_kind(
                    left_type,
                    TypeFlags::STRING_LIKE,
                    true,
                )? || self.is_type_assignable_to_kind(
                    right_type,
                    TypeFlags::STRING_LIKE,
                    true,
                )? {
                    result_type = Some(self.tables.intrinsics.string);
                } else if self.tables.flags_of(left_type).intersects(TypeFlags::ANY)
                    || self.tables.flags_of(right_type).intersects(TypeFlags::ANY)
                {
                    let error = self.tables.intrinsics.error;
                    result_type = Some(if left_type == error || right_type == error {
                        error
                    } else {
                        self.tables.intrinsics.any
                    });
                }
                if let Some(result) = result_type {
                    if !self.check_for_disallowed_es_symbol_operand(
                        left, right, left_type, right_type, operator,
                    )? {
                        return Ok(result);
                    }
                }
                let Some(result_type) = result_type else {
                    let close_enough = TypeFlags::from_bits(
                        TypeFlags::NUMBER_LIKE.bits()
                            | TypeFlags::BIG_INT_LIKE.bits()
                            | TypeFlags::STRING_LIKE.bits()
                            | TypeFlags::ANY_OR_UNKNOWN.bits(),
                    );
                    self.report_operator_error(
                        operator_token,
                        left,
                        right,
                        left_type,
                        right_type,
                        error_node,
                        Some(&mut |state, l, r| {
                            Ok(state.is_type_assignable_to_kind(l, close_enough, false)?
                                && state.is_type_assignable_to_kind(r, close_enough, false)?)
                        }),
                    )?;
                    return Ok(self.tables.intrinsics.any);
                };
                if operator == SyntaxKind::PlusEqualsToken {
                    self.check_assignment_operator(
                        left,
                        operator_token,
                        right,
                        left_type,
                        result_type,
                    )?;
                }
                Ok(result_type)
            }
            SyntaxKind::LessThanToken
            | SyntaxKind::GreaterThanToken
            | SyntaxKind::LessThanEqualsToken
            | SyntaxKind::GreaterThanEqualsToken => {
                if self.check_for_disallowed_es_symbol_operand(
                    left, right, left_type, right_type, operator,
                )? {
                    let left_nonnull = self.check_non_null_type(left_type, left)?;
                    let left_cmp =
                        self.get_base_type_of_literal_type_for_comparison(left_nonnull)?;
                    let right_nonnull = self.check_non_null_type(right_type, right)?;
                    let right_cmp =
                        self.get_base_type_of_literal_type_for_comparison(right_nonnull)?;
                    self.report_operator_error_unless(
                        operator_token,
                        left,
                        right,
                        left_cmp,
                        right_cmp,
                        error_node,
                        &mut |state, l, r| {
                            if state.tables.flags_of(l).intersects(TypeFlags::ANY)
                                || state.tables.flags_of(r).intersects(TypeFlags::ANY)
                            {
                                return Ok(true);
                            }
                            let number_or_bigint = state.tables.intrinsics.number_or_bigint;
                            let left_number = state.is_type_assignable_to(l, number_or_bigint)?;
                            let right_number = state.is_type_assignable_to(r, number_or_bigint)?;
                            Ok(left_number && right_number
                                || !left_number
                                    && !right_number
                                    && state.are_types_comparable(l, r)?)
                        },
                    )?;
                }
                Ok(self.tables.intrinsics.boolean)
            }
            SyntaxKind::EqualsEqualsToken
            | SyntaxKind::ExclamationEqualsToken
            | SyntaxKind::EqualsEqualsEqualsToken
            | SyntaxKind::ExclamationEqualsEqualsToken => {
                if !check_mode.intersects(CheckMode::TYPE_ONLY) {
                    // In JavaScript, tsc reports the object-reference
                    // warning only for strict equality; TypeScript
                    // reports all four equality operators.
                    if (self.is_literal_expression_of_object(left)
                        || self.is_literal_expression_of_object(right))
                        && (!self.is_in_js_file(left)
                            || matches!(
                                operator,
                                SyntaxKind::EqualsEqualsEqualsToken
                                    | SyntaxKind::ExclamationEqualsEqualsToken
                            ))
                    {
                        let eq = matches!(
                            operator,
                            SyntaxKind::EqualsEqualsToken | SyntaxKind::EqualsEqualsEqualsToken
                        );
                        self.error_at(
                            error_node.or(Some(operator_token)),
                            &tsc_diagnostics::gen::This_condition_will_always_return_0_since_JavaScript_compares_objects_by_reference_not_value,
                            &[if eq { "false" } else { "true" }],
                        );
                    }
                    self.check_nan_equality(error_node, operator_token, left, right)?;
                    // The comparability verdict itself is tsc-faithful
                    // (the intersection relation arms landed 5.9b), but
                    // 2367 POSITIONS depend on narrowing: a failed
                    // comparison narrows the operands of the later
                    // comparisons in the chain, so without flow
                    // analysis every repeat reports where tsc stays
                    // silent (equalityWithIntersectionTypes01 pins the
                    // suppressed repeats). (The intersection-operand
                    // containment retired at 6.6f: operands carry
                    // real flow-narrowed types since 6.4.)
                    self.report_operator_error_unless(
                        operator_token,
                        left,
                        right,
                        left_type,
                        right_type,
                        error_node,
                        &mut |state, l, r| {
                            Ok(state.is_type_equality_comparable_to(l, r)?
                                || state.is_type_equality_comparable_to(r, l)?)
                        },
                    )?;
                }
                Ok(self.tables.intrinsics.boolean)
            }
            SyntaxKind::InstanceOfKeyword => {
                self.check_instance_of_expression(left, right, left_type, right_type, check_mode)
            }
            SyntaxKind::InKeyword => self.check_in_expression(left, right, left_type, right_type),
            SyntaxKind::AmpersandAmpersandToken | SyntaxKind::AmpersandAmpersandEqualsToken => {
                let result_type = if self.has_type_facts(left_type, TypeFacts::TRUTHY)? {
                    let strict_null_checks = self
                        .options
                        .strict_option_value(self.options.strict_null_checks);
                    // VERBATIM QUIRK (80211): the non-strict arm takes
                    // the falsy part of the RIGHT type's literal base.
                    let falsy_source = if strict_null_checks {
                        left_type
                    } else {
                        self.get_base_type_of_literal_type(right_type)?
                    };
                    let falsy = self.extract_definitely_falsy_types(falsy_source)?;
                    self.get_union_type_ex(
                        &[falsy, right_type],
                        tsc_types::UnionReduction::Literal,
                    )?
                } else {
                    left_type
                };
                if operator == SyntaxKind::AmpersandAmpersandEqualsToken {
                    self.check_assignment_operator(
                        left,
                        operator_token,
                        right,
                        left_type,
                        right_type,
                    )?;
                }
                Ok(result_type)
            }
            SyntaxKind::BarBarToken | SyntaxKind::BarBarEqualsToken => {
                let result_type = if self.has_type_facts(left_type, TypeFacts::FALSY)? {
                    let non_falsy = self.remove_definitely_falsy_types(left_type)?;
                    let non_nullable = self.get_non_nullable_type(non_falsy)?;
                    self.get_union_type_ex(
                        &[non_nullable, right_type],
                        tsc_types::UnionReduction::Subtype,
                    )?
                } else {
                    left_type
                };
                if operator == SyntaxKind::BarBarEqualsToken {
                    self.check_assignment_operator(
                        left,
                        operator_token,
                        right,
                        left_type,
                        right_type,
                    )?;
                }
                Ok(result_type)
            }
            SyntaxKind::QuestionQuestionToken | SyntaxKind::QuestionQuestionEqualsToken => {
                let result_type =
                    if self.has_type_facts(left_type, TypeFacts::EQ_UNDEFINED_OR_NULL)? {
                        let non_nullable = self.get_non_nullable_type(left_type)?;
                        self.get_union_type_ex(
                            &[non_nullable, right_type],
                            tsc_types::UnionReduction::Subtype,
                        )?
                    } else {
                        left_type
                    };
                if operator == SyntaxKind::QuestionQuestionEqualsToken {
                    self.check_assignment_operator(
                        left,
                        operator_token,
                        right,
                        left_type,
                        right_type,
                    )?;
                }
                Ok(result_type)
            }
            SyntaxKind::EqualsToken => {
                let declaration_kind = self
                    .parent_of(left)
                    .filter(|&parent| self.kind_of(parent) == SyntaxKind::BinaryExpression)
                    .map_or(tsc_binder::AssignmentDeclarationKind::None, |assignment| {
                        tsc_binder::get_assignment_declaration_kind(
                            self.binder.source_of_node(assignment),
                            assignment,
                        )
                    });
                self.check_assignment_declaration(declaration_kind, right_type)?;
                if self.is_assignment_declaration_worker(declaration_kind, left, right) {
                    let right_is_object = self
                        .tables
                        .flags_of(right_type)
                        .intersects(TypeFlags::OBJECT);
                    let object_can_supply_assignment_declaration = right_is_object
                        && (matches!(
                            declaration_kind,
                            tsc_binder::AssignmentDeclarationKind::ModuleExports
                                | tsc_binder::AssignmentDeclarationKind::Prototype
                        ) || self.is_empty_object_type(right_type)?
                            || self.is_function_object_type(right_type)?
                            || self
                                .tables
                                .object_flags_of(right_type)
                                .intersects(tsc_types::ObjectFlags::CLASS));
                    if !object_can_supply_assignment_declaration {
                        self.check_assignment_operator(
                            left,
                            operator_token,
                            right,
                            left_type,
                            right_type,
                        )?;
                    }
                    Ok(left_type)
                } else {
                    self.check_assignment_operator(
                        left,
                        operator_token,
                        right,
                        left_type,
                        right_type,
                    )?;
                    Ok(right_type)
                }
            }
            SyntaxKind::CommaToken => {
                if !self.options.allow_unreachable_code.unwrap_or(false)
                    && self.is_side_effect_free(left)
                    && !self.is_indirect_call_comma(left)
                    && !self.comma_left_inside_jsx_2657_span(left)
                {
                    self.error_at(
                        Some(left),
                        &tsc_diagnostics::gen::Left_side_of_comma_operator_is_unused_and_has_no_side_effects,
                        &[],
                    );
                }
                Ok(right_type)
            }
            _ => unreachable!("parser operator domain is closed (tsc Debug.fail)"),
        }
    }

    /// tsc-port: checkAssignmentDeclaration @6.0.3
    /// tsc-hash: 1daea1c4ab15e44151d23835b48f2a4c89eb86dae278cbc47b5753c6eae555b8
    /// tsc-span: _tsc.js:80273-80295
    fn check_assignment_declaration(
        &mut self,
        kind: tsc_binder::AssignmentDeclarationKind,
        right_type: TypeId,
    ) -> CheckResult<()> {
        if kind != tsc_binder::AssignmentDeclarationKind::ModuleExports {
            return Ok(());
        }
        for property in self.get_properties_of_object_type_owned(right_type)? {
            let property_type = self.get_type_of_symbol(property)?;
            let Some(property_type_symbol) = self.tables.type_of(property_type).symbol else {
                continue;
            };
            if !self
                .binder
                .symbol(property_type_symbol)
                .flags
                .intersects(SymbolFlags::CLASS)
            {
                continue;
            }
            let name = self.binder.symbol(property).escaped_name.clone();
            let location = self.binder.symbol(property).value_declaration;
            let Some(symbol) =
                self.resolve_name(location, &name, SymbolFlags::TYPE, None, false, false)?
            else {
                continue;
            };
            if !self
                .binder
                .symbol(symbol)
                .declarations
                .iter()
                .any(|&declaration| self.kind_of(declaration) == SyntaxKind::JSDocTypedefTag)
            {
                continue;
            }
            let display = tsc_binder::unescape_leading_underscores(&name);
            self.add_duplicate_declaration_errors_for_symbols(
                symbol,
                &tsc_diagnostics::gen::Duplicate_identifier_0,
                display,
                property,
            );
            self.add_duplicate_declaration_errors_for_symbols(
                property,
                &tsc_diagnostics::gen::Duplicate_identifier_0,
                display,
                symbol,
            );
        }
        Ok(())
    }

    /// tsc-port: isAssignmentDeclaration @6.0.3
    /// tsc-hash: 3ef6382442624f2ab23f2a5e2bff71731c55316e24582fa8ca6aeed23e68efd0
    /// tsc-span: _tsc.js:80350-80365
    fn is_assignment_declaration_worker(
        &self,
        kind: tsc_binder::AssignmentDeclarationKind,
        left: NodeId,
        right: NodeId,
    ) -> bool {
        match kind {
            tsc_binder::AssignmentDeclarationKind::ModuleExports => true,
            tsc_binder::AssignmentDeclarationKind::ExportsProperty
            | tsc_binder::AssignmentDeclarationKind::Property
            | tsc_binder::AssignmentDeclarationKind::Prototype
            | tsc_binder::AssignmentDeclarationKind::PrototypeProperty
            | tsc_binder::AssignmentDeclarationKind::ThisProperty => {
                let Some(symbol) = self.node_symbol(left) else {
                    return false;
                };
                let symbol = self.get_merged_symbol(symbol);
                let Some(initializer) = tsc_binder::assignment::get_assigned_expando_initializer(
                    self.binder.source_of_node(right),
                    right,
                ) else {
                    return false;
                };
                self.kind_of(initializer) == SyntaxKind::ObjectLiteralExpression
                    && !self.binder.symbol(symbol).exports.is_empty()
            }
            _ => false,
        }
    }

    /// tsc-port: checkInstanceOfExpression @6.0.3
    /// tsc-hash: 8b4bc390c2cbc6af55fcefe572164921124693cbde399348bb7a321ce672bef0
    /// tsc-span: _tsc.js:79558-79577
    ///
    /// The binary resolves a SIGNATURE (resolveInstanceofExpression,
    /// calls.rs): callable [Symbol.hasInstance] methods run the full
    /// resolveCall protocol (2860 failure head on the ladder); no
    /// signatures and not Function-subtype → 2359 + errorCall; every
    /// other shape is anySignature. The 2861 boolean check runs on the
    /// resolved signature's return type — anySignature (any) and
    /// errorCall (errorType) both pass silently.
    fn check_instance_of_expression(
        &mut self,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let silent_never = self.tables.intrinsics.silent_never;
        if left_type == silent_never || right_type == silent_never {
            return Ok(silent_never);
        }
        if !self.tables.flags_of(left_type).intersects(TypeFlags::ANY)
            && self.all_types_assignable_to_kind(left_type, TypeFlags::PRIMITIVE)?
        {
            self.error_at(
                Some(left),
                &tsc_diagnostics::gen::The_left_hand_side_of_an_instanceof_expression_must_be_of_type_any_an_object_type_or_a_type_parameter,
                &[],
            );
        }
        let Some(binary) = self
            .parent_of(left)
            .filter(|&parent| self.kind_of(parent) == SyntaxKind::BinaryExpression)
        else {
            // A parsed instanceof always owns both operands. For a
            // detached synthetic recovery operand there is no call
            // node on which to resolve Symbol.hasInstance, so retain
            // the operator's boolean result after the LHS check above.
            return Ok(self.tables.intrinsics.boolean);
        };
        debug_assert_eq!(self.kind_of(binary), SyntaxKind::BinaryExpression);
        let signature = self.get_resolved_signature(binary, check_mode)?;
        if signature == self.resolving_signature {
            // 79568-79570: M6-dead (SkipGenericFunctions producer).
            return Ok(silent_never);
        }
        let return_type = self.get_return_type_of_signature(signature)?;
        let boolean = self.tables.intrinsics.boolean;
        self.check_type_assignable_to(
            return_type,
            boolean,
            Some(right),
            &tsc_diagnostics::gen::An_object_s_Symbol_hasInstance_method_must_return_a_boolean_value_for_it_to_be_used_on_the_right_hand_side_of_an_instanceof_expression,
        )?;
        Ok(boolean)
    }

    /// tsc-port: getSymbolHasInstanceMethodOfObjectType @6.0.3
    /// tsc-hash: bbccaf986fe1a5455266aea00e7514929bfb20bab61dcd969d44420b84375bdb
    /// tsc-span: _tsc.js:79546-79557
    pub(crate) fn get_symbol_has_instance_method_of_object_type(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let has_instance_property_name =
            self.get_property_name_for_known_symbol_name("hasInstance")?;
        if !self.all_types_assignable_to_kind(ty, TypeFlags::NON_PRIMITIVE)? {
            return Ok(None);
        }
        let property = self.get_property_of_type_full(ty, &has_instance_property_name)?;
        let Some(property) = property else {
            return Ok(None);
        };
        let property_type = self.get_type_of_symbol(property)?;
        if !self
            .get_signatures_of_type(property_type, SignatureKind::Call)?
            .is_empty()
        {
            return Ok(Some(property_type));
        }
        Ok(None)
    }

    /// tsc-port: getPropertyNameForKnownSymbolName @6.0.3
    /// tsc-hash: 339a60369eedfaca440c106b17ebeff12f80aaa9c1c7301c470264254ba9e4a9
    /// tsc-span: _tsc.js:84219-84226
    ///
    /// The global Symbol constructor's unique-symbol property name
    /// (the late-bound `__@name@<id>` form) with the `__@name` noLib
    /// fallback. Shared: instanceof (`hasInstance`) + the §4 iterable
    /// slow path (`iterator`/`asyncIterator`).
    pub(crate) fn get_property_name_for_known_symbol_name(
        &mut self,
        symbol_name: &str,
    ) -> CheckResult<String> {
        let ctor = self.get_global_symbol("Symbol", SymbolFlags::VALUE, None);
        if let Some(ctor) = ctor {
            let ctor_type = self.get_type_of_symbol(ctor)?;
            let unique_type = self.get_type_of_property_of_type(ctor_type, symbol_name)?;
            if let Some(unique_type) = unique_type {
                if let Some(name) = self.property_name_from_type_usable(unique_type) {
                    return Ok(name);
                }
            }
        }
        Ok(format!("__@{symbol_name}"))
    }

    /// tsc-port: checkInExpression @6.0.3
    /// tsc-hash: a43ca6414d13b970e9f636bfc327fc6d95e70bc7f1bb5029037629d0e68dd54f
    /// tsc-span: _tsc.js:79591-79608
    ///
    /// Both operand rows are plain 2322 (2360/2361 do not exist in
    /// 6.0.3); the ClassPrivateFieldIn emit-helper rows are
    /// importHelpers-gated (no-op).
    fn check_in_expression(
        &mut self,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
    ) -> CheckResult<TypeId> {
        let silent_never = self.tables.intrinsics.silent_never;
        if left_type == silent_never || right_type == silent_never {
            return Ok(silent_never);
        }
        if self.kind_of(left) == SyntaxKind::PrivateIdentifier {
            let unresolved = self.links.node(left).resolved_symbol.resolved().is_none();
            if unresolved && self.get_containing_class_of(left).is_some() {
                // isUncheckedJSSuggestion is false in TS files.
                self.report_nonexistent_property(left, right_type, false)?;
            }
        } else {
            let nonnull = self.check_non_null_type(left_type, left)?;
            let string_number_symbol = self.tables.intrinsics.string_number_symbol;
            self.check_type_assignable_to(
                nonnull,
                string_number_symbol,
                Some(left),
                &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
            )?;
        }
        let nonnull_right = self.check_non_null_type(right_type, right)?;
        let non_primitive = self.tables.intrinsics.non_primitive;
        if self.check_type_assignable_to(
            nonnull_right,
            non_primitive,
            Some(right),
            &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
        )? && self.has_empty_object_intersection(right_type)?
        {
            let display = self.type_to_string_slice(right_type)?;
            self.error_at(
                Some(right),
                &tsc_diagnostics::gen::Type_0_may_represent_a_primitive_value_which_is_not_permitted_as_the_right_operand_of_the_in_operator,
                &[&display],
            );
        }
        Ok(self.tables.intrinsics.boolean)
    }

    /// tsc-port: hasEmptyObjectIntersection @6.0.3
    /// tsc-hash: daa5bda4229ae1742f39d860b11948d4b03840cf711d82dc9abf2faee639901c
    /// tsc-span: _tsc.js:79588-79590
    fn has_empty_object_intersection(&mut self, ty: TypeId) -> CheckResult<bool> {
        let unknown_empty = self.unknown_empty_object_type;
        self.some_type_result(ty, |state, t| {
            if t == unknown_empty {
                return Ok(true);
            }
            if !state.tables.flags_of(t).intersects(TypeFlags::INTERSECTION) {
                return Ok(false);
            }
            let base = state.get_base_constraint_or_type(t)?;
            state.is_empty_anonymous_object_type(base)
        })
    }

    /// bothAreBigIntLike (80264-80266).
    fn both_are_bigint_like(&mut self, left: TypeId, right: TypeId) -> CheckResult<bool> {
        Ok(
            self.is_type_assignable_to_kind(left, TypeFlags::BIG_INT_LIKE, false)?
                && self.is_type_assignable_to_kind(right, TypeFlags::BIG_INT_LIKE, false)?,
        )
    }

    /// The shift-simplification row (80098-80119): evaluate the RHS;
    /// |value| >= 32 elevates to 6807 INSIDE an enum member and is a
    /// suggestion (unmodeled band — skipped) everywhere else.
    fn check_shift_simplification(
        &mut self,
        left: NodeId,
        operator: SyntaxKind,
        right: NodeId,
        error_node: Option<NodeId>,
        operator_token: NodeId,
    ) -> CheckResult<()> {
        let rhs_eval = self.evaluate(right, None)?;
        let Some(crate::evaluate::EvalValue::Num(value)) = rhs_eval.value else {
            return Ok(());
        };
        // tsc's guard is `Math.abs(value) >= 32` (80100), FALSE for
        // NaN — a NaN shift amount reports nothing (m4-review OP-4;
        // the is_nan arm keeps that semantics in the inverted form).
        if value.abs() < 32.0 || value.is_nan() {
            return Ok(());
        }
        let is_enum_member = self
            .parent_of(right)
            .and_then(|p| self.parent_of(p))
            .map(|grandparent| {
                let walked = self.walk_up_parenthesized_expressions(grandparent);
                self.kind_of(walked) == SyntaxKind::EnumMember
            })
            .unwrap_or(false);
        // errorOrSuggestion: only the error flavor is modeled
        // (suggestion band unmodeled, like 80008).
        if is_enum_member {
            let left_text = self.text_of_node(left)?;
            let simplified = tsc_types::tables::js_number_to_string(value % 32.0);
            self.error_at(
                Some(error_node.unwrap_or(operator_token)),
                &tsc_diagnostics::gen::This_operation_can_be_simplified_This_shift_is_identical_to_0_1_2,
                &[&left_text, token_text(operator), &simplified],
            );
        }
        Ok(())
    }

    /// walkUpParenthesizedExpressions.
    /// tsc-port: walkUpParenthesizedExpressions @6.0.3
    /// tsc-hash: c7b9f97b1b14b29d991fe1746460648f8be06ba6f691484635c722c829279769
    /// tsc-span: _tsc.js:15646-15648
    pub(crate) fn walk_up_parenthesized_expressions(&self, mut node: NodeId) -> NodeId {
        while self.kind_of(node) == SyntaxKind::ParenthesizedExpression {
            match self.parent_of(node) {
                Some(parent) => node = parent,
                None => break,
            }
        }
        node
    }

    /// getTextOfNode: the source text of the node's span (left trivia
    /// skipped, like getSourceTextOfNodeFromSourceFile). JS
    /// `substring` SWAPS its arguments when begin > end — reachable
    /// for zero-width recovery nodes whose leading-trivia skip runs
    /// past their end; replicate rather than panic.
    /// tsc-port: getTextOfNode @6.0.3
    /// tsc-hash: cb4037b9d0e93b0fc01341a22e946e09da6115b8db5b2861f21b458f2add269f
    /// tsc-span: _tsc.js:13045-13047
    ///
    /// tsc-port: getSourceTextOfNodeFromSourceFile @6.0.3
    /// tsc-hash: c2d2a4cbb43eaea54f3c334de2dcafd9e23d75c8c0268997ea504de46f9b16c2
    /// tsc-span: _tsc.js:13017-13019
    pub(crate) fn text_of_node(&self, node: NodeId) -> CheckResult<String> {
        let source = self.binder.source_of_node(node);
        let raw = source.arena.node(node);
        let start = tsc_syntax::skip_trivia(&source.text, raw.pos as usize);
        let end = raw.end as usize;
        let (begin, finish) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        Ok(source.text[begin..finish].to_owned())
    }

    /// isIndirectCall (80278-80283) on the comma binary's parent
    /// shape: `(0, f)()` / `(0, obj.f)()` / `(0, eval)(...)`.
    fn is_indirect_call_comma(&self, left: NodeId) -> bool {
        let Some(binary) = self.parent_of(left) else {
            return false;
        };
        let Some(paren) = self.parent_of(binary) else {
            return false;
        };
        if self.kind_of(paren) != SyntaxKind::ParenthesizedExpression {
            return false;
        }
        let left_is_zero = self.kind_of(left) == SyntaxKind::NumericLiteral
            && matches!(self.data_of(left), NodeData::NumericLiteral(data) if data.text == "0");
        if !left_is_zero {
            return false;
        }
        let Some(grandparent) = self.parent_of(paren) else {
            return false;
        };
        let call_shape = match self.data_of(grandparent) {
            NodeData::CallExpression(data) => data.expression == Some(paren),
            _ => self.kind_of(grandparent) == SyntaxKind::TaggedTemplateExpression,
        };
        if !call_shape {
            return false;
        }
        let Some((_, _, right)) = self.binary_parts(binary) else {
            return false;
        };
        matches!(
            self.kind_of(right),
            SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
        ) || (self.kind_of(right) == SyntaxKind::Identifier
            && self.identifier_text_of(right) == Some("eval"))
    }

    /// The comma arm's JSX guard (80268-80277): a parse-level 2657
    /// (JSX expressions must have one parent element) covering the
    /// left operand's start suppresses 2695.
    fn comma_left_inside_jsx_2657_span(&self, left: NodeId) -> bool {
        let source = self.binder.source_of_node(left);
        let raw = source.arena.node(left);
        let start = tsc_syntax::skip_trivia(&source.text, raw.pos as usize) as u32;
        source.parse_diagnostics.iter().any(|diag| {
            let (Some(diag_start), Some(length)) = (diag.start, diag.length) else {
                return false;
            };
            diag.code() == 2657 && diag_start <= start && start <= diag_start + length
        })
    }

    /// tsc-port: isSideEffectFree @6.0.3
    /// tsc-hash: c75519b46affe50fc14f09341223c8535a7a0803cd37bc3cf0ff0a28e19890d7
    /// tsc-span: _tsc.js:79754-79800
    fn is_side_effect_free(&self, node: NodeId) -> bool {
        let node = node_util::skip_parentheses_pub(self.binder.source_of_node(node), node);
        match self.kind_of(node) {
            SyntaxKind::Identifier
            | SyntaxKind::StringLiteral
            | SyntaxKind::RegularExpressionLiteral
            | SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::TemplateExpression
            | SyntaxKind::NoSubstitutionTemplateLiteral
            | SyntaxKind::NumericLiteral
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::TrueKeyword
            | SyntaxKind::FalseKeyword
            | SyntaxKind::NullKeyword
            | SyntaxKind::UndefinedKeyword
            | SyntaxKind::FunctionExpression
            | SyntaxKind::ClassExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::TypeOfExpression
            | SyntaxKind::NonNullExpression
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::JsxElement => true,
            SyntaxKind::ConditionalExpression => match self.data_of(node) {
                NodeData::ConditionalExpression(data) => match (data.when_true, data.when_false) {
                    (Some(t), Some(f)) => {
                        self.is_side_effect_free(t) && self.is_side_effect_free(f)
                    }
                    _ => false,
                },
                _ => false,
            },
            SyntaxKind::BinaryExpression => match self.binary_parts(node) {
                Some((left, operator_token, right)) => {
                    if node_util::is_assignment_operator(self.operator_kind(operator_token)) {
                        false
                    } else {
                        self.is_side_effect_free(left) && self.is_side_effect_free(right)
                    }
                }
                None => false,
            },
            SyntaxKind::PrefixUnaryExpression | SyntaxKind::PostfixUnaryExpression => {
                let operator = match self.data_of(node) {
                    NodeData::PrefixUnaryExpression(data) => Some(data.operator),
                    NodeData::PostfixUnaryExpression(data) => Some(data.operator),
                    _ => None,
                };
                matches!(
                    operator,
                    Some(
                        SyntaxKind::ExclamationToken
                            | SyntaxKind::PlusToken
                            | SyntaxKind::MinusToken
                            | SyntaxKind::TildeToken
                    )
                )
            }
            // VoidExpression (explicit opt-out) and the assertion
            // kinds (not SEF, but can produce useful type warnings)
            // fall through to false with everything else.
            _ => false,
        }
    }

    /// isLiteralExpressionOfObject.
    fn is_literal_expression_of_object(&self, node: NodeId) -> bool {
        matches!(
            self.kind_of(node),
            SyntaxKind::ObjectLiteralExpression
                | SyntaxKind::ArrayLiteralExpression
                | SyntaxKind::RegularExpressionLiteral
                | SyntaxKind::FunctionExpression
                | SyntaxKind::ClassExpression
        )
    }

    /// checkForDisallowedESSymbolOperand (80284-80291): reports 2469
    /// on the offending operand, returns operand-legality.
    fn check_for_disallowed_es_symbol_operand(
        &mut self,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        operator: SyntaxKind,
    ) -> CheckResult<bool> {
        let offending = if self
            .maybe_type_of_kind_considering_base_constraint(left_type, TypeFlags::ES_SYMBOL_LIKE)?
        {
            Some(left)
        } else if self
            .maybe_type_of_kind_considering_base_constraint(right_type, TypeFlags::ES_SYMBOL_LIKE)?
        {
            Some(right)
        } else {
            None
        };
        if let Some(operand) = offending {
            self.error_at(
                Some(operand),
                &tsc_diagnostics::gen::The_0_operator_cannot_be_applied_to_type_symbol,
                &[token_text(operator)],
            );
            return Ok(false);
        }
        Ok(true)
    }

    /// getSuggestedBooleanOperator (80292-80305).
    fn get_suggested_boolean_operator(operator: SyntaxKind) -> Option<SyntaxKind> {
        match operator {
            SyntaxKind::BarToken | SyntaxKind::BarEqualsToken => Some(SyntaxKind::BarBarToken),
            SyntaxKind::CaretToken | SyntaxKind::CaretEqualsToken => {
                Some(SyntaxKind::ExclamationEqualsEqualsToken)
            }
            SyntaxKind::AmpersandToken | SyntaxKind::AmpersandEqualsToken => {
                Some(SyntaxKind::AmpersandAmpersandToken)
            }
            _ => None,
        }
    }

    /// tsc-port: checkArithmeticOperandType @6.0.3
    /// tsc-hash: c76d834d763a10ee5c562a6251dcb8947313cdbd82b1e9c46efbb79577207a1e
    /// tsc-span: _tsc.js:79214-79225
    fn check_arithmetic_operand_type(
        &mut self,
        operand: NodeId,
        ty: TypeId,
        diagnostic: &'static tsc_diagnostics::DiagnosticMessage,
        is_await_valid: bool,
    ) -> CheckResult<bool> {
        let number_or_bigint = self.tables.intrinsics.number_or_bigint;
        if !self.is_type_assignable_to(ty, number_or_bigint)? {
            let awaited = if is_await_valid {
                self.get_awaited_type_of_promise(ty)?
            } else {
                None
            };
            let maybe_missing_await = match awaited {
                Some(awaited) => self.is_type_assignable_to(awaited, number_or_bigint)?,
                None => false,
            };
            self.error_and_maybe_suggest_await(operand, maybe_missing_await, diagnostic, &[]);
            return Ok(false);
        }
        Ok(true)
    }

    fn report_primitive_module_exports_property_assignment(
        &mut self,
        access_expression: NodeId,
    ) -> CheckResult<()> {
        if self.kind_of(access_expression) != SyntaxKind::PropertyAccessExpression
            || !self.is_in_js_file(access_expression)
            || !node_util::is_assignment_target(
                self.binder.source_of_node(access_expression),
                access_expression,
            )
        {
            return Ok(());
        }
        let NodeData::PropertyAccessExpression(access) = self.data_of(access_expression) else {
            return Ok(());
        };
        let (Some(receiver), Some(name)) = (access.expression, access.name) else {
            return Ok(());
        };
        if !tsc_binder::assignment::is_module_exports_access_expression(
            self.binder.source_of_node(receiver),
            receiver,
        ) {
            return Ok(());
        }

        // The receiver expression is temporarily errorType while its
        // assignment LHS is being checked. The source-file export=
        // symbol owns the completed replacement type.
        let receiver_type = self.get_type_of_expression(receiver)?;
        let mut receiver_type = self.get_widened_type(receiver_type)?;
        if receiver_type == self.tables.intrinsics.error {
            let export_equals = {
                let source = self.binder.source_of_node(access_expression);
                self.binder
                    .node_symbol(source.root)
                    .map(|symbol| self.get_merged_symbol(symbol))
                    .and_then(|symbol| {
                        self.binder
                            .symbol(symbol)
                            .exports
                            .get(tsc_types::InternalSymbolName::EXPORT_EQUALS)
                            .copied()
                    })
            };
            if let Some(export_equals) = export_equals {
                receiver_type = self.get_type_of_symbol(export_equals)?;
                receiver_type = self.get_widened_type(receiver_type)?;
            }
        }
        if !self
            .tables
            .flags_of(receiver_type)
            .intersects(TypeFlags::PRIMITIVE)
        {
            return Ok(());
        }
        self.report_nonexistent_property(name, receiver_type, false)
    }

    fn check_assignment_operator(
        &mut self,
        left: NodeId,
        operator_token: NodeId,
        right: NodeId,
        left_type: TypeId,
        value_type: TypeId,
    ) -> CheckResult<()> {
        let operator = self.operator_kind(operator_token);
        if !node_util::is_assignment_operator(operator) {
            return Ok(());
        }
        if operator == SyntaxKind::EqualsToken {
            self.report_primitive_module_exports_property_assignment(left)?;
        }
        let mut assignee_type = left_type;
        if Self::is_compound_assignment(operator)
            && self.kind_of(left) == SyntaxKind::PropertyAccessExpression
        {
            assignee_type = self.check_property_access_expression(left, CheckMode::NORMAL, true)?;
        }
        if self.check_reference_expression(
            left,
            &tsc_diagnostics::gen::The_left_hand_side_of_an_assignment_expression_must_be_a_variable_or_a_property_access,
            &tsc_diagnostics::gen::The_left_hand_side_of_an_assignment_expression_may_not_be_an_optional_property_access,
        ) {
            let mut head_message: Option<&'static tsc_diagnostics::DiagnosticMessage> = None;
            if self.tables.exact_optional_property_types
                && self.kind_of(left) == SyntaxKind::PropertyAccessExpression
                && self.maybe_type_of_kind(value_type, TypeFlags::UNDEFINED)
            {
                let (receiver, name_text) = match self.data_of(left) {
                    NodeData::PropertyAccessExpression(data) => {
                        let name_text = data
                            .name
                            .and_then(|name| self.identifier_text_of(name))
                            .map(str::to_owned);
                        (data.expression, name_text)
                    }
                    _ => (None, None),
                };
                if let (Some(receiver), Some(name_text)) = (receiver, name_text) {
                    let receiver_type = self.get_type_of_expression(receiver)?;
                    let target = self.get_type_of_property_of_type(receiver_type, &name_text)?;
                    if self.is_exact_optional_property_mismatch(Some(value_type), target)? {
                        head_message = Some(&tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1_with_exactOptionalPropertyTypes_true_Consider_adding_undefined_to_the_type_of_the_target);
                    }
                }
            }
            // checkTypeAssignableToAndOptionallyElaborate(valueType,
            // assigneeType, errorNode=LEFT, expr=RIGHT) — elaboration
            // first (the Step-12 idiom): a literal RIGHT operand that
            // reports an inner member/element row suppresses the
            // LEFT-anchored head.
            let elaborated = !self.is_type_assignable_to(value_type, assignee_type)?
                && self.elaborate_literal_assignment(
                    right,
                    assignee_type,
                    Some(&tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1),
                )?
                .reported();
            if !elaborated {
                self.check_type_assignable_to(
                    value_type,
                    assignee_type,
                    Some(left),
                    head_message.unwrap_or(&tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1),
                )?;
            }
        }
        Ok(())
    }

    /// isCompoundAssignment: FirstCompoundAssignment(+=) through
    /// LastCompoundAssignment(??=).
    fn is_compound_assignment(operator: SyntaxKind) -> bool {
        operator.value() >= SyntaxKind::FirstCompoundAssignment.value()
            && operator.value() <= SyntaxKind::LastCompoundAssignment.value()
    }

    /// tsc-port: isExactOptionalPropertyMismatch @6.0.3
    /// tsc-hash: 31db751b7f65924eb86b2f3088de7b4bf9a0a35a6e41eb948c2e175edb5ee573
    /// tsc-span: _tsc.js:67771-67773
    pub(crate) fn is_exact_optional_property_mismatch(
        &mut self,
        source: Option<TypeId>,
        target: Option<TypeId>,
    ) -> CheckResult<bool> {
        let (Some(source), Some(target)) = (source, target) else {
            return Ok(false);
        };
        Ok(self.maybe_type_of_kind(source, TypeFlags::UNDEFINED)
            && self.contains_missing_type(target))
    }

    /// tsc-port: containsMissingType @6.0.3
    /// tsc-hash: 7e0d6e0ae9e6bc78feeacabc1f160a8e8020b593453d55d682dd1df22ab6b6fb
    /// tsc-span: _tsc.js:67886-67888
    pub(crate) fn contains_missing_type(&self, ty: TypeId) -> bool {
        let missing = self.tables.intrinsics.missing;
        ty == missing
            || (self.tables.flags_of(ty).intersects(TypeFlags::UNION)
                && match &self.tables.type_of(ty).data {
                    TypeData::Union { types, .. } => types.first() == Some(&missing),
                    _ => false,
                })
    }

    /// tsc-port: checkReferenceExpression @6.0.3
    /// tsc-hash: 6cf20850f5cdb8cee4f92d124671a8b425d3f520569f86af47faf596138c5ff0
    /// tsc-span: _tsc.js:79291-79302
    pub(crate) fn check_reference_expression(
        &mut self,
        expr: NodeId,
        invalid_reference_message: &'static tsc_diagnostics::DiagnosticMessage,
        invalid_optional_chain_message: &'static tsc_diagnostics::DiagnosticMessage,
    ) -> bool {
        let node = self.skip_outer_expressions(
            expr,
            OuterExpressionKinds(
                OuterExpressionKinds::ASSERTIONS.0 | OuterExpressionKinds::PARENTHESES.0,
            ),
        );
        if self.kind_of(node) != SyntaxKind::Identifier
            && !matches!(
                self.kind_of(node),
                SyntaxKind::PropertyAccessExpression | SyntaxKind::ElementAccessExpression
            )
        {
            self.error_at(Some(expr), invalid_reference_message, &[]);
            return false;
        }
        let source = self.binder.source_of_node(node);
        if node_util::node_flags(source, node).intersects(tsc_types::NodeFlags::OPTIONAL_CHAIN) {
            self.error_at(Some(expr), invalid_optional_chain_message, &[]);
            return false;
        }
        true
    }

    /// reportOperatorErrorUnless (80332-80338).
    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    fn report_operator_error_unless(
        &mut self,
        operator_token: NodeId,
        left: NodeId,
        right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        error_node: Option<NodeId>,
        types_are_compatible: &mut dyn FnMut(&mut Self, TypeId, TypeId) -> CheckResult<bool>,
    ) -> CheckResult<bool> {
        if !types_are_compatible(self, left_type, right_type)? {
            self.report_operator_error(
                operator_token,
                left,
                right,
                left_type,
                right_type,
                error_node,
                Some(types_are_compatible),
            )?;
            return Ok(true);
        }
        Ok(false)
    }

    /// tsc-port: reportOperatorError @6.0.3
    /// tsc-hash: 4cd1ff851ea94f2471ba78b4e315d9474715205ac4cf4ea0cc0ec1ee30b900a8
    /// tsc-span: _tsc.js:80374-80397
    #[allow(clippy::type_complexity)]
    #[allow(clippy::too_many_arguments)]
    fn report_operator_error(
        &mut self,
        operator_token: NodeId,
        _left: NodeId,
        _right: NodeId,
        left_type: TypeId,
        right_type: TypeId,
        error_node: Option<NodeId>,
        mut is_related: Option<&mut dyn FnMut(&mut Self, TypeId, TypeId) -> CheckResult<bool>>,
    ) -> CheckResult<()> {
        let err_node = error_node.unwrap_or(operator_token);
        let mut would_work_with_await = false;
        if let Some(is_related) = is_related.as_deref_mut() {
            let awaited_left = self.get_awaited_type_no_alias(left_type, None)?;
            let awaited_right = self.get_awaited_type_no_alias(right_type, None)?;
            would_work_with_await = !(awaited_left == Some(left_type)
                && awaited_right == Some(right_type))
                && awaited_left.is_some()
                && awaited_right.is_some()
                && is_related(self, awaited_left.unwrap(), awaited_right.unwrap())?;
        }
        let mut effective_left = left_type;
        let mut effective_right = right_type;
        if !would_work_with_await {
            if let Some(is_related) = is_related {
                let (l, r) = self.get_base_types_if_unrelated(left_type, right_type, is_related)?;
                effective_left = l;
                effective_right = r;
            }
        }
        let (left_str, right_str) =
            self.get_type_names_for_error_display(effective_left, effective_right)?;
        if !self.try_give_better_primary_error(
            operator_token,
            err_node,
            would_work_with_await,
            &left_str,
            &right_str,
        ) {
            let operator = self.operator_kind(operator_token);
            self.error_and_maybe_suggest_await(
                err_node,
                would_work_with_await,
                &tsc_diagnostics::gen::Operator_0_cannot_be_applied_to_types_1_and_2,
                &[token_text(operator), &left_str, &right_str],
            );
        }
        Ok(())
    }

    /// tryGiveBetterPrimaryError (80398-80412): equality flavors
    /// upgrade to 2367.
    fn try_give_better_primary_error(
        &mut self,
        operator_token: NodeId,
        err_node: NodeId,
        maybe_missing_await: bool,
        left_str: &str,
        right_str: &str,
    ) -> bool {
        match self.operator_kind(operator_token) {
            SyntaxKind::EqualsEqualsEqualsToken
            | SyntaxKind::EqualsEqualsToken
            | SyntaxKind::ExclamationEqualsEqualsToken
            | SyntaxKind::ExclamationEqualsToken => {
                self.error_and_maybe_suggest_await(
                    err_node,
                    maybe_missing_await,
                    &tsc_diagnostics::gen::This_comparison_appears_to_be_unintentional_because_the_types_0_and_1_have_no_overlap,
                    &[left_str, right_str],
                );
                true
            }
            _ => false,
        }
    }

    /// tsc-port: checkNaNEquality @6.0.3
    /// tsc-hash: 8a6362bd3cf2bcbdc1b91d1132901b7729db356d3571fa4630418f97ca1be139
    /// tsc-span: _tsc.js:80413-80428
    fn check_nan_equality(
        &mut self,
        error_node: Option<NodeId>,
        operator_token: NodeId,
        left: NodeId,
        right: NodeId,
    ) -> CheckResult<()> {
        let left_skipped = node_util::skip_parentheses_pub(self.binder.source_of_node(left), left);
        let right_skipped =
            node_util::skip_parentheses_pub(self.binder.source_of_node(right), right);
        let is_left_nan = self.is_global_nan(left_skipped)?;
        let is_right_nan = self.is_global_nan(right_skipped)?;
        if !is_left_nan && !is_right_nan {
            return Ok(());
        }
        let operator = self.operator_kind(operator_token);
        let eq = matches!(
            operator,
            SyntaxKind::EqualsEqualsEqualsToken | SyntaxKind::EqualsEqualsToken
        );
        let verdict_token = if eq {
            SyntaxKind::FalseKeyword
        } else {
            SyntaxKind::TrueKeyword
        };
        if is_left_nan && is_right_nan {
            self.error_at(
                error_node.or(Some(operator_token)),
                &tsc_diagnostics::gen::This_condition_will_always_return_0,
                &[token_text(verdict_token)],
            );
            return Ok(());
        }
        let operator_string = if matches!(
            operator,
            SyntaxKind::ExclamationEqualsEqualsToken | SyntaxKind::ExclamationEqualsToken
        ) {
            token_text(SyntaxKind::ExclamationToken)
        } else {
            ""
        };
        let location = if is_left_nan { right } else { left };
        let expression =
            node_util::skip_parentheses_pub(self.binder.source_of_node(location), location);
        let suggestion_target = if self.is_entity_name_expression(expression) {
            self.entity_name_to_string(expression)?
        } else {
            "...".to_owned()
        };
        let related = self.related_info_for_node(
            location,
            &tsc_diagnostics::gen::Did_you_mean_0,
            &[&format!(
                "{operator_string}Number.isNaN({suggestion_target})"
            )],
        );
        self.error_at_with_related(
            error_node.or(Some(operator_token)),
            &tsc_diagnostics::gen::This_condition_will_always_return_0,
            &[token_text(verdict_token)],
            vec![related],
        );
        Ok(())
    }

    /// isGlobalNaN (80429-80435).
    fn is_global_nan(&mut self, expr: NodeId) -> CheckResult<bool> {
        if self.kind_of(expr) != SyntaxKind::Identifier
            || self.identifier_text_of(expr) != Some("NaN")
        {
            return Ok(false);
        }
        let global_nan = self.get_global_symbol("NaN", SymbolFlags::VALUE, None);
        let Some(global_nan) = global_nan else {
            return Ok(false);
        };
        Ok(self.get_resolved_symbol(expr)? == Some(global_nan))
    }

    /// tsc-port: isTypeEqualityComparableTo @6.0.3
    /// tsc-hash: 433b6784f163c0230b9d6321a252e42ea3e01c77e36e7d1c598ec78f9afbaeb0
    /// tsc-span: _tsc.js:79807-79809
    pub(crate) fn is_type_equality_comparable_to(
        &mut self,
        source: TypeId,
        target: TypeId,
    ) -> CheckResult<bool> {
        if self.tables.flags_of(target).intersects(TypeFlags::NULLABLE) {
            return Ok(true);
        }
        self.is_type_comparable_to(source, target)
    }

    /// tsc-port: areTypesComparable @6.0.3
    /// tsc-hash: 2a23b9204e724b986d5cc84fa5d232d29fc9f42520c62a9bd97b241dc5eea2dc
    /// tsc-span: _tsc.js:63928-63930
    pub(crate) fn are_types_comparable(
        &mut self,
        type1: TypeId,
        type2: TypeId,
    ) -> CheckResult<bool> {
        Ok(
            self.is_type_comparable_to(type1, type2)?
                || self.is_type_comparable_to(type2, type1)?,
        )
    }

    /// tsc-port: getBaseTypesIfUnrelated @6.0.3
    /// tsc-hash: c357cbb950428b8072fd55f3d23c516c066ecc3d57e753cdee3be2ea3713b047
    /// tsc-span: _tsc.js:80436-80446
    #[allow(clippy::type_complexity)]
    fn get_base_types_if_unrelated(
        &mut self,
        left_type: TypeId,
        right_type: TypeId,
        is_related: &mut dyn FnMut(&mut Self, TypeId, TypeId) -> CheckResult<bool>,
    ) -> CheckResult<(TypeId, TypeId)> {
        let left_base = self.get_base_type_of_literal_type(left_type)?;
        let right_base = self.get_base_type_of_literal_type(right_type)?;
        if !is_related(self, left_base, right_base)? {
            return Ok((left_base, right_base));
        }
        Ok((left_type, right_type))
    }

    /// tsc-port: getTypeNamesForErrorDisplay @6.0.3
    /// tsc-hash: 40a0bc0eba39778afa87e7a1acdc421c12ecc5e0d0c117c72327676c61922597
    /// tsc-span: _tsc.js:50748-50756
    ///
    /// symbolValueDeclarationIsContextSensitive passes the value
    /// declaration as enclosingDeclaration for non-context-sensitive
    /// expression symbols — which arms the display slice's
    /// annotation-reuse channel (9.3b2 probes: fn-expression sources
    /// keep annotation spellings where declare-let twins render
    /// structurally; the prior "qualification concern only" note here
    /// was the stale-justification pattern again). Equal renderings
    /// retry through getTypeNameForErrorDisplay
    /// (UseFullyQualifiedType, 50757-50764) and the retried texts are
    /// used EVEN IF STILL EQUAL — same-type operands print
    /// `'null' and 'null'` (oracle-probed); tsc has no third fallback.
    fn get_type_names_for_error_display(
        &mut self,
        left: TypeId,
        right: TypeId,
    ) -> CheckResult<(String, String)> {
        let left_str = self.type_to_string_slice_with_error_enclosing(left)?;
        let right_str = self.type_to_string_slice_with_error_enclosing(right)?;
        if left_str == right_str {
            return Ok((
                self.get_type_name_for_error_display(left)?,
                self.get_type_name_for_error_display(right)?,
            ));
        }
        Ok((left_str, right_str))
    }

    // ---- definitely-falsy extraction (67839-67847) ----

    /// tsc-port: extractDefinitelyFalsyTypes @6.0.3
    /// tsc-hash: f57a216a7b9ce73a9b3eb9dc19b54926d6d150ef855a23a0c8def9a235cad4fc
    /// tsc-span: _tsc.js:67842-67844
    fn extract_definitely_falsy_types(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        let mapped = self.map_type(
            ty,
            &mut |state, t| state.get_definitely_falsy_part_of_type(t).map(Some),
            false,
        )?;
        Ok(mapped.expect("mapper never returns None"))
    }

    /// tsc-port: getDefinitelyFalsyPartOfType @6.0.3
    /// tsc-hash: 50b94f17558db01a13dc4a585cbf848a4f6933362aebcb51aff221ab5a523796
    /// tsc-span: _tsc.js:67845-67847
    fn get_definitely_falsy_part_of_type(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::STRING) {
            return Ok(self.tables.get_string_literal_type(""));
        }
        if flags.intersects(TypeFlags::NUMBER) {
            return Ok(self.tables.get_number_literal_type(0.0));
        }
        if flags.intersects(TypeFlags::BIG_INT) {
            return Ok(self
                .tables
                .get_bigint_literal_type(tsc_types::PseudoBigInt {
                    negative: false,
                    base10_value: "0".to_owned(),
                }));
        }
        let intrinsics = &self.tables.intrinsics;
        let is_self_falsy = ty == intrinsics.false_regular
            || ty == intrinsics.false_fresh
            || flags.intersects(TypeFlags::from_bits(
                TypeFlags::VOID.bits()
                    | TypeFlags::UNDEFINED.bits()
                    | TypeFlags::NULL.bits()
                    | TypeFlags::ANY_OR_UNKNOWN.bits(),
            ))
            || match &self.tables.type_of(ty).data {
                TypeData::Literal { value } => match value {
                    LiteralValue::String(text) => {
                        flags.intersects(TypeFlags::STRING_LITERAL) && text.is_empty()
                    }
                    LiteralValue::Number(value) => {
                        flags.intersects(TypeFlags::NUMBER_LITERAL) && *value == 0.0
                    }
                    LiteralValue::BigInt(value) => {
                        flags.intersects(TypeFlags::BIG_INT_LITERAL) && value.base10_value == "0"
                    }
                },
                _ => false,
            };
        Ok(if is_self_falsy {
            ty
        } else {
            self.tables.intrinsics.never
        })
    }

    /// tsc-port: checkDestructuringAssignment @6.0.3
    /// tsc-hash: a0405c5ba783516cc8f2bf817e30364e90ab508f785d5dcaa442789459735f7d
    /// tsc-span: _tsc.js:79716-79742
    pub(crate) fn check_destructuring_assignment(
        &mut self,
        expr_or_assignment: NodeId,
        source_type: TypeId,
        check_mode: CheckMode,
        right_is_this: bool,
    ) -> CheckResult<TypeId> {
        let mut source_type = source_type;
        let mut target = expr_or_assignment;
        if self.kind_of(expr_or_assignment) == SyntaxKind::ShorthandPropertyAssignment {
            let (name, equals_token, initializer) = match self.data_of(expr_or_assignment) {
                NodeData::ShorthandPropertyAssignment(data) => (
                    data.name,
                    data.equals_token,
                    data.object_assignment_initializer,
                ),
                _ => (None, None, None),
            };
            let Some(name) = name else {
                // The parser normally installs a missing identifier.
                // An omitted synthetic slot has no assignment target
                // left to check; preserve the source-side result.
                return Ok(source_type);
            };
            if let Some(initializer) = initializer {
                let strict_null_checks = self
                    .options
                    .strict_option_value(self.options.strict_null_checks);
                if strict_null_checks {
                    // checkExpression runs with NO mode here (79720).
                    let initializer_type = self.check_expression(initializer, CheckMode::NORMAL)?;
                    if !self.has_type_facts(initializer_type, TypeFacts::IS_UNDEFINED)? {
                        source_type =
                            self.get_type_with_facts(source_type, TypeFacts::NE_UNDEFINED)?;
                    }
                }
                if let Some(equals_token) = equals_token {
                    self.check_binary_like_expression(
                        name,
                        equals_token,
                        initializer,
                        check_mode,
                        None,
                    )?;
                }
            }
            target = name;
        }
        if self.kind_of(target) == SyntaxKind::BinaryExpression {
            let Some((left, operator_token, _)) = self.binary_parts(target) else {
                return Ok(source_type);
            };
            if self.operator_kind(operator_token) == SyntaxKind::EqualsToken {
                self.check_binary_expression(target, check_mode)?;
                target = left;
                if self
                    .options
                    .strict_option_value(self.options.strict_null_checks)
                {
                    source_type = self.get_type_with_facts(source_type, TypeFacts::NE_UNDEFINED)?;
                }
            }
        }
        match self.kind_of(target) {
            SyntaxKind::ObjectLiteralExpression => {
                self.check_object_literal_assignment(target, source_type, right_is_this)
            }
            SyntaxKind::ArrayLiteralExpression => {
                self.check_array_literal_assignment(target, source_type, check_mode)
            }
            _ => self.check_reference_assignment(target, source_type, check_mode),
        }
    }

    /// tsc-port: checkObjectLiteralAssignment @6.0.3
    /// tsc-hash: a6ba71eb73a08ca8cd66e2f7189e89f3b121aed04b12fc4bd109e6971b28dabb
    /// tsc-span: _tsc.js:79609-79618
    fn check_object_literal_assignment(
        &mut self,
        node: NodeId,
        source_type: TypeId,
        right_is_this: bool,
    ) -> CheckResult<TypeId> {
        let (properties, properties_array) = match self.data_of(node) {
            NodeData::ObjectLiteralExpression(data) => {
                (self.nodes_of(data.properties), data.properties)
            }
            _ => unreachable!("kind/data agree"),
        };
        let strict_null_checks = self
            .options
            .strict_option_value(self.options.strict_null_checks);
        if strict_null_checks && properties.is_empty() {
            return self.check_non_null_type(source_type, node);
        }
        for index in 0..properties.len() {
            self.check_object_literal_destructuring_property_assignment(
                source_type,
                index,
                &properties,
                properties_array,
                right_is_this,
            )?;
        }
        Ok(source_type)
    }

    /// tsc-port: checkObjectLiteralDestructuringPropertyAssignment @6.0.3
    /// tsc-hash: 23abed05f2a1cd6909230e50cff0ff0fe51d816decbc52346c01aee660f610d5
    /// tsc-span: _tsc.js:79619-79666
    ///
    /// The ObjectSpreadRest emit-helper row is importHelpers-gated
    /// (a no-op without the option — unmodeled).
    fn check_object_literal_destructuring_property_assignment(
        &mut self,
        object_literal_type: TypeId,
        property_index: usize,
        properties: &[NodeId],
        properties_array: Option<tsc_syntax::NodeArrayId>,
        right_is_this: bool,
    ) -> CheckResult<()> {
        let property = properties[property_index];
        match self.kind_of(property) {
            SyntaxKind::PropertyAssignment | SyntaxKind::ShorthandPropertyAssignment => {
                let (name, initializer) = match self.data_of(property) {
                    NodeData::PropertyAssignment(data) => (data.name, data.initializer),
                    NodeData::ShorthandPropertyAssignment(data) => (data.name, None),
                    _ => (None, None),
                };
                let Some(name) = name else {
                    // The parse diagnostic owns this malformed
                    // property; there is no key to index in the
                    // source object type.
                    return Ok(());
                };
                let expr_type = self.get_literal_type_from_property_name(name)?;
                if let Some(text) = self.property_name_from_type_usable(expr_type) {
                    let prop = self.get_property_of_type_full(object_literal_type, &text)?;
                    if let Some(prop) = prop {
                        self.mark_property_as_referenced(prop, Some(property), right_is_this);
                        self.check_property_accessibility(
                            property,
                            /*is_super*/ false,
                            /*writing*/ true,
                            object_literal_type,
                            prop,
                            /*report_error*/ true,
                        )?;
                    }
                }
                let access_flags = tsc_types::AccessFlags::from_bits(
                    tsc_types::AccessFlags::EXPRESSION_POSITION.bits()
                        | if self.has_default_value(property) {
                            tsc_types::AccessFlags::ALLOW_MISSING.bits()
                        } else {
                            0
                        },
                );
                let element_type = self.get_indexed_access_type(
                    object_literal_type,
                    expr_type,
                    access_flags,
                    Some(name),
                    None,
                    None,
                )?;
                let ty = self.get_flow_type_of_destructuring(property, element_type)?;
                let target = if self.kind_of(property) == SyntaxKind::ShorthandPropertyAssignment {
                    property
                } else {
                    let Some(initializer) = initializer else {
                        return Ok(());
                    };
                    initializer
                };
                self.check_destructuring_assignment(target, ty, CheckMode::NORMAL, false)?;
                Ok(())
            }
            SyntaxKind::SpreadAssignment => {
                if property_index < properties.len() - 1 {
                    self.error_at(
                        Some(property),
                        &tsc_diagnostics::gen::A_rest_element_must_be_last_in_a_destructuring_pattern,
                        &[],
                    );
                    return Ok(());
                }
                let expression = match self.data_of(property) {
                    NodeData::SpreadAssignment(data) => data.expression,
                    _ => None,
                };
                let Some(expression) = expression else {
                    return Ok(());
                };
                let mut non_rest_names: Vec<NodeId> = Vec::new();
                for &other in properties {
                    if self.kind_of(other) != SyntaxKind::SpreadAssignment {
                        let name = match self.data_of(other) {
                            NodeData::PropertyAssignment(data) => data.name,
                            NodeData::ShorthandPropertyAssignment(data) => data.name,
                            NodeData::MethodDeclaration(data) => data.name,
                            NodeData::GetAccessor(data) => data.name,
                            NodeData::SetAccessor(data) => data.name,
                            _ => None,
                        };
                        if let Some(name) = name {
                            non_rest_names.push(name);
                        }
                    }
                }
                let symbol = self.tables.type_of(object_literal_type).symbol;
                let rest_type = self.get_rest_type(object_literal_type, &non_rest_names, symbol)?;
                self.check_grammar_for_disallowed_trailing_comma(
                    properties_array,
                    &tsc_diagnostics::gen::A_rest_parameter_or_binding_pattern_may_not_have_a_trailing_comma,
                );
                self.check_destructuring_assignment(
                    expression,
                    rest_type,
                    CheckMode::NORMAL,
                    false,
                )?;
                Ok(())
            }
            _ => {
                self.error_at(
                    Some(property),
                    &tsc_diagnostics::gen::Property_assignment_expected,
                    &[],
                );
                Ok(())
            }
        }
    }

    /// tsc-port: checkArrayLiteralAssignment @6.0.3
    /// tsc-hash: dd986f2465f4953c7dcc207c5ee398ca69a859a73dc40a9e9d9bbbc4d16d20df
    /// tsc-span: _tsc.js:79667-79682
    ///
    /// The DestructuringAssignment emit-helper row is
    /// importHelpers-gated (no-op).
    fn check_array_literal_assignment(
        &mut self,
        node: NodeId,
        source_type: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let elements: Vec<NodeId> = match self.data_of(node) {
            NodeData::ArrayLiteralExpression(data) => self.nodes_of(data.elements),
            _ => unreachable!("kind/data agree"),
        };
        let undefined_type = self.tables.intrinsics.undefined;
        let possibly_out_of_bounds_type = self.check_iterated_type_or_element_type(
            IterationUse::from_bits(
                IterationUse::DESTRUCTURING.bits() | IterationUse::POSSIBLY_OUT_OF_BOUNDS.bits(),
            ),
            source_type,
            undefined_type,
            Some(node),
        )?;
        let mut in_bounds_type = if self.options.no_unchecked_indexed_access.unwrap_or(false) {
            None
        } else {
            Some(possibly_out_of_bounds_type)
        };
        for index in 0..elements.len() {
            let mut element_type = possibly_out_of_bounds_type;
            if self.kind_of(elements[index]) == SyntaxKind::SpreadElement {
                element_type = match in_bounds_type {
                    Some(ty) => ty,
                    None => {
                        let computed = self.check_iterated_type_or_element_type(
                            IterationUse::DESTRUCTURING,
                            source_type,
                            undefined_type,
                            Some(node),
                        )?;
                        in_bounds_type = Some(computed);
                        computed
                    }
                };
            }
            self.check_array_literal_destructuring_element_assignment(
                node,
                source_type,
                index,
                &elements,
                element_type,
                check_mode,
            )?;
        }
        Ok(source_type)
    }

    /// tsc-port: checkArrayLiteralDestructuringElementAssignment @6.0.3
    /// tsc-hash: c045a7a8c14edb78210b7ec68d31046445fe0a03d7ba0632ebda0c7d5c66ec17
    /// tsc-span: _tsc.js:79683-79715
    ///
    /// The synthetic access node (createSyntheticExpression 76289) is
    /// threaded as the ELEMENT node itself plus the synthetic_access
    /// flag: spans match (the synthetic copies the element's range)
    /// and the flag keeps every access-node kind probe off, exactly
    /// like tsc's SyntheticExpression kind (M4 5.9d).
    fn check_array_literal_destructuring_element_assignment(
        &mut self,
        node: NodeId,
        source_type: TypeId,
        element_index: usize,
        elements: &[NodeId],
        iterated_element_type: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult<()> {
        let element = elements[element_index];
        if self.kind_of(element) == SyntaxKind::OmittedExpression {
            return Ok(());
        }
        if self.kind_of(element) != SyntaxKind::SpreadElement {
            if !self.is_array_like_type(source_type)? {
                // 79696: non-array-like sources hand every element the
                // iterated type.
                self.check_destructuring_assignment(
                    element,
                    iterated_element_type,
                    check_mode,
                    false,
                )?;
                return Ok(());
            }
            let index_type = self.tables.get_number_literal_type(element_index as f64);
            let access_flags = tsc_types::AccessFlags::from_bits(
                tsc_types::AccessFlags::EXPRESSION_POSITION.bits()
                    | if self.has_default_value(element) {
                        tsc_types::AccessFlags::ALLOW_MISSING.bits()
                    } else {
                        0
                    },
            );
            // createSyntheticExpression(element, indexType): the
            // element node carries the span; the synthetic flag keeps
            // the access-node kind probes off (an element that IS an
            // element access must not take the real access arms).
            let element_type = self
                .get_indexed_access_type_or_undefined_ex(
                    source_type,
                    index_type,
                    access_flags,
                    Some(element),
                    None,
                    None,
                    /*synthetic_access*/ true,
                )?
                .unwrap_or(self.tables.intrinsics.error);
            let assigned_type = if self.has_default_value(element) {
                self.get_type_with_facts(element_type, TypeFacts::NE_UNDEFINED)?
            } else {
                element_type
            };
            let ty = self.get_flow_type_of_destructuring(element, assigned_type)?;
            self.check_destructuring_assignment(element, ty, check_mode, false)?;
            return Ok(());
        }
        if element_index < elements.len() - 1 {
            self.error_at(
                Some(element),
                &tsc_diagnostics::gen::A_rest_element_must_be_last_in_a_destructuring_pattern,
                &[],
            );
            return Ok(());
        }
        let rest_expression = match self.data_of(element) {
            NodeData::SpreadElement(data) => data.expression,
            _ => None,
        };
        let Some(rest_expression) = rest_expression else {
            return Ok(());
        };
        if self.kind_of(rest_expression) == SyntaxKind::BinaryExpression {
            if let Some((_, operator_token, _)) = self.binary_parts(rest_expression) {
                if self.operator_kind(operator_token) == SyntaxKind::EqualsToken {
                    self.error_at(
                        Some(operator_token),
                        &tsc_diagnostics::gen::A_rest_element_cannot_have_an_initializer,
                        &[],
                    );
                    return Ok(());
                }
            }
        }
        let elements_array = match self.data_of(node) {
            NodeData::ArrayLiteralExpression(data) => data.elements,
            _ => None,
        };
        self.check_grammar_for_disallowed_trailing_comma(
            elements_array,
            &tsc_diagnostics::gen::A_rest_parameter_or_binding_pattern_may_not_have_a_trailing_comma,
        );
        let all_tuples = self.every_type(source_type, |state, t| state.tables.is_tuple_type(t));
        let rest_type = if all_tuples {
            let sliced = self.map_type(
                source_type,
                &mut |state, t| state.slice_tuple_type(t, element_index, 0).map(Some),
                false,
            )?;
            sliced.expect("mapper never returns None")
        } else {
            // 79707: createArrayType(elementType) — the iterated
            // element type feeds non-tuple rests since 5.8b.
            self.create_array_type(iterated_element_type, false)?
        };
        self.check_destructuring_assignment(rest_expression, rest_type, check_mode, false)?;
        Ok(())
    }

    /// tsc-port: checkReferenceAssignment @6.0.3
    /// tsc-hash: a8df092688dc76db0fc8fc2c03d0d9d01f0ae09de7aeb8808a9ef9a33e7ad1c6
    /// tsc-span: _tsc.js:79743-79753
    ///
    /// The ClassPrivateFieldSet emit-helper row is importHelpers-gated
    /// (no-op).
    fn check_reference_assignment(
        &mut self,
        target: NodeId,
        source_type: TypeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let target_type = self.check_expression(target, check_mode)?;
        let parent_is_spread = self
            .parent_of(target)
            .is_some_and(|p| self.kind_of(p) == SyntaxKind::SpreadAssignment);
        let (reference_message, optional_message) = if parent_is_spread {
            (
                &tsc_diagnostics::gen::The_target_of_an_object_rest_assignment_must_be_a_variable_or_a_property_access,
                &tsc_diagnostics::gen::The_target_of_an_object_rest_assignment_may_not_be_an_optional_property_access,
            )
        } else {
            (
                &tsc_diagnostics::gen::The_left_hand_side_of_an_assignment_expression_must_be_a_variable_or_a_property_access,
                &tsc_diagnostics::gen::The_left_hand_side_of_an_assignment_expression_may_not_be_an_optional_property_access,
            )
        };
        if self.check_reference_expression(target, reference_message, optional_message) {
            self.check_type_assignable_to(
                source_type,
                target_type,
                Some(target),
                &tsc_diagnostics::gen::Type_0_is_not_assignable_to_type_1,
            )?;
        }
        Ok(source_type)
    }

    // ---- assertions / satisfies / instantiation expressions / meta ----

    /// tsc-port: checkAssertion @6.0.3
    /// tsc-hash: 9e116ec9fcef81cb8897434648907a824839cd020365db6116b40e0a92a7b435
    /// tsc-span: _tsc.js:77863-77876
    ///
    /// The erasableSyntaxOnly 1294 row is unmodeled (no harness
    /// directive sets the option, so the arm is unreachable in every
    /// gate).
    pub(crate) fn check_assertion(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        if self.kind_of(node) == SyntaxKind::TypeAssertionExpression {
            let file_name = self.binder.source_of_node(node).file_name.clone();
            if file_name.ends_with(".cts") || file_name.ends_with(".mts") {
                self.grammar_error_on_node(
                    node,
                    &tsc_diagnostics::gen::This_syntax_is_reserved_in_files_with_the_mts_or_cts_extension_Use_an_as_expression_instead,
                    &[],
                );
            }
        }
        self.check_assertion_worker(node, check_mode)
    }

    /// tsc-port: checkAssertionWorker @6.0.3
    /// tsc-hash: 4e26da5a8bfd5e25ade6ed74a9c0f354580d933097740b60c4fb75e87c35aa78
    /// tsc-span: _tsc.js:77908-77922
    pub(crate) fn check_assertion_worker(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let Some((type_node, expression)) = self.assertion_type_and_expression(node) else {
            return Ok(self.tables.intrinsics.error);
        };
        let expr_type = self.check_expression(expression, check_mode)?;
        if self.is_const_type_reference(type_node) {
            if !self.is_valid_const_assertion_argument(expression)? {
                self.error_at(
                    Some(expression),
                    &tsc_diagnostics::gen::A_const_assertion_can_only_be_applied_to_references_to_enum_members_or_string_number_boolean_array_or_object_literals,
                    &[],
                );
            }
            return Ok(self.tables.get_regular_type_of_literal_type(expr_type));
        }
        self.links
            .set_node_assertion_expression_type(self.speculation_depth, node, expr_type);
        self.check_source_element(Some(type_node));
        self.check_node_deferred(node);
        self.get_type_from_type_node(type_node)
    }

    /// tsc-port: getAssertionTypeAndExpression @6.0.3
    /// tsc-hash: b2792f4495599e3031324c0f4e51ab1bb93f7dca6c972ac946d482e96ae7d83c
    /// tsc-span: _tsc.js:77923-77938
    fn assertion_type_and_expression(&mut self, node: NodeId) -> Option<(NodeId, NodeId)> {
        let (type_node, expression) = match self.data_of(node) {
            NodeData::AsExpression(data) => (data.r#type, data.expression),
            NodeData::TypeAssertionExpression(data) => (data.r#type, data.expression),
            NodeData::ParenthesizedExpression(data) => {
                (self.jsdoc_type_assertion_type_node(node), data.expression)
            }
            _ => (None, None),
        };
        match (type_node, expression) {
            (Some(type_node), Some(expression)) => Some((type_node, expression)),
            _ => None,
        }
    }

    /// isConstTypeReference (11848).
    fn is_const_type_reference(&self, node: NodeId) -> bool {
        match self.data_of(node) {
            NodeData::TypeReference(data) => {
                data.type_arguments.is_none()
                    && data.type_name.is_some_and(|name| {
                        self.kind_of(name) == SyntaxKind::Identifier
                            && self.identifier_text_of(name) == Some("const")
                    })
            }
            _ => false,
        }
    }

    /// tsc-port: checkAssertionDeferred @6.0.3
    /// tsc-hash: d94786f9e7e2377bfb6fa517be200d2b838cbdabd73b17c13ce9985e5b022f34
    /// tsc-span: _tsc.js:77939-77954
    ///
    /// The 2352 report displays the UNWIDENED exprType (risk #2,
    /// oracle-proven 2026-07-12); the comparable probe reads the
    /// widened one.
    pub(crate) fn check_assertion_deferred(&mut self, node: NodeId) -> CheckResult<()> {
        let Some((type_node, _)) = self.assertion_type_and_expression(node) else {
            return Ok(());
        };
        let err_node = if self.kind_of(node) == SyntaxKind::ParenthesizedExpression {
            type_node
        } else {
            node
        };
        let stashed = self
            .links
            .node(node)
            .assertion_expression_type
            .expect("every deferred assertion keeps its stashed operand type across trials");
        let base = self.get_base_type_of_literal_type(stashed)?;
        let expr_type = self.get_regular_type_of_object_literal(base)?;
        let target_type = self.get_type_from_type_node(type_node)?;
        if target_type == self.tables.intrinsics.error {
            return Ok(());
        }
        // addLazyDiagnostic = eager identity (5.4).
        let widened = self.get_widened_type(expr_type)?;
        if !self.is_type_comparable_to(target_type, widened)? {
            self.check_type_comparable_to(
                expr_type,
                target_type,
                Some(err_node),
                &tsc_diagnostics::gen::Conversion_of_type_0_to_type_1_may_be_a_mistake_because_neither_type_sufficiently_overlaps_with_the_other_If_this_was_intentional_convert_the_expression_to_unknown_first,
            )?;
        }
        Ok(())
    }

    /// tsc-port: checkSatisfiesExpression @6.0.3
    /// tsc-hash: b04e473039c86379b77528a49b256683a1b25b9522a507d9866493733ebc9058
    /// tsc-span: _tsc.js:78047-78050
    pub(crate) fn check_satisfies_expression(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let (type_node, expression) = match self.data_of(node) {
            NodeData::SatisfiesExpression(data) => (data.r#type, data.expression),
            _ => (None, None),
        };
        let (Some(type_node), Some(expression)) = (type_node, expression) else {
            return Ok(self.tables.intrinsics.error);
        };
        self.check_source_element(Some(type_node));
        self.check_satisfies_expression_worker(expression, type_node)
    }

    /// tsc-port: checkSatisfiesExpressionWorker @6.0.3
    /// tsc-hash: 69efa22f5ce65ba834ce27bba8e4cab3bfedb94a4646cfa0944324f6f62d989e
    /// tsc-span: _tsc.js:78051-78060
    pub(crate) fn check_satisfies_expression_worker(
        &mut self,
        expression: NodeId,
        target: NodeId,
    ) -> CheckResult<TypeId> {
        let diagnostics_before_expression = self.diagnostics.len();
        let expr_type = self.check_expression(expression, CheckMode::NORMAL)?;
        let target_type = self.get_type_from_type_node(target)?;
        if target_type == self.tables.intrinsics.error {
            return Ok(target_type);
        }
        let error_node = self.parent_of(target).and_then(|start| {
            let mut current = Some(start);
            while let Some(node) = current {
                if matches!(
                    self.kind_of(node),
                    SyntaxKind::SatisfiesExpression | SyntaxKind::JSDocSatisfiesTag
                ) {
                    return Some(node);
                }
                current = self.parent_of(node);
            }
            None
        });
        // Contextual literal checking can already have emitted the
        // elementwise row before the shared optional-elaboration entry
        // is reached. Keep that existing de-duplication boundary, then
        // let the generic dependency own every elaboration arm and the
        // ordinary normalized relation fallback.
        if self.elaborated_satisfies_expressions.contains(&expression) {
            return Ok(expr_type);
        }
        let mut operand = expression;
        while let NodeData::ParenthesizedExpression(data) = self.data_of(operand) {
            match data.expression {
                Some(inner) => operand = inner,
                None => break,
            }
        }
        if self.literal_operand_reported_since(operand, diagnostics_before_expression)
            && matches!(
                self.data_of(operand),
                NodeData::ObjectLiteralExpression(_) | NodeData::ArrayLiteralExpression(_)
            )
        {
            self.elaborated_satisfies_expressions.insert(expression);
            return Ok(expr_type);
        }
        self.check_type_assignable_to_and_optionally_elaborate(
            expr_type,
            target_type,
            error_node,
            expression,
            &tsc_diagnostics::gen::Type_0_does_not_satisfy_the_expected_type_1,
        )?;
        Ok(expr_type)
    }

    /// Whether contextual checking already emitted a diagnostic inside
    /// this literal operand. Relation probes may lazily emit file-less
    /// missing-global diagnostics, so a raw diagnostic-count comparison
    /// would incorrectly treat those as successful literal elaboration.
    fn literal_operand_reported_since(&self, operand: NodeId, before: usize) -> bool {
        let source = self.binder.source_of_node(operand);
        let node = source.arena.node(operand);
        let start_byte = tsc_syntax::skip_trivia(&source.text, node.pos as usize);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start = to_utf16(start_byte);
        let end = to_utf16(node.end as usize);
        self.diagnostics[before..].iter().any(|diagnostic| {
            diagnostic.file_name.as_deref() == Some(source.file_name.as_str())
                && diagnostic.start.is_some_and(|diagnostic_start| {
                    start <= diagnostic_start && diagnostic_start < end
                })
        })
    }

    /// tsc-port: checkExpressionWithTypeArguments @6.0.3
    /// tsc-hash: 861f746ba696ba63aaaaba441683dfe5277e0e8b885a209edf4f4c5b9ea23f1a
    /// tsc-span: _tsc.js:77963-77974
    pub(crate) fn check_expression_with_type_arguments(
        &mut self,
        node: NodeId,
    ) -> CheckResult<TypeId> {
        self.check_grammar_expression_with_type_arguments(node);
        let (expression, type_arguments) = match self.data_of(node) {
            NodeData::ExpressionWithTypeArguments(data) => (data.expression, data.type_arguments),
            // The TypeQuery flavor (`typeof f<number>`) routes through
            // getTypeFromTypeNode, not this arm.
            _ => (None, None),
        };
        let Some(expression) = expression else {
            return Ok(self.tables.intrinsics.error);
        };
        for argument in self.nodes_of(type_arguments) {
            self.check_source_element(Some(argument));
        }
        if self.kind_of(node) == SyntaxKind::ExpressionWithTypeArguments {
            let parent = self
                .parent_of(node)
                .map(|p| self.walk_up_parenthesized_expressions(p));
            if let Some(parent) = parent {
                if self.kind_of(parent) == SyntaxKind::BinaryExpression {
                    if let Some((_, operator_token, right)) = self.binary_parts(parent) {
                        if self.operator_kind(operator_token) == SyntaxKind::InstanceOfKeyword
                            && self.is_node_descendant_of(node, right)
                        {
                            self.error_at(
                                Some(node),
                                &tsc_diagnostics::gen::The_right_hand_side_of_an_instanceof_expression_must_not_be_an_instantiation_expression,
                                &[],
                            );
                        }
                    }
                }
            }
        }
        let expr_type = self.check_expression(expression, CheckMode::NORMAL)?;
        self.get_instantiation_expression_type(expr_type, node, type_arguments)
    }

    /// tsc-port: getInstantiationExpressionType @6.0.3
    /// tsc-hash: f4a1390bbeb0115467dd9705247f0eef523b5c55910f73917feb8de92a755dc8
    /// tsc-span: _tsc.js:77975-78046
    pub(crate) fn get_instantiation_expression_type(
        &mut self,
        expr_type: TypeId,
        node: NodeId,
        type_arguments: Option<tsc_syntax::NodeArrayId>,
    ) -> CheckResult<TypeId> {
        let argument_nodes = self.nodes_of(type_arguments);
        if expr_type == self.tables.intrinsics.silent_never
            || expr_type == self.tables.intrinsics.error
            || argument_nodes.is_empty()
        {
            return Ok(expr_type);
        }
        if let Some(map) = &self.links.node(node).instantiation_expression_types {
            if let Some(&cached) = map.get(&expr_type) {
                return Ok(cached);
            }
        }
        let mut has_some_applicable_signature = false;
        let mut non_applicable_type: Option<TypeId> = None;
        let result = self.get_instantiated_type(
            expr_type,
            node,
            &argument_nodes,
            &mut has_some_applicable_signature,
            &mut non_applicable_type,
        )?;
        // STORE BEFORE ERROR (77987).
        self.links.set_node_instantiation_expression_type(
            self.speculation_depth,
            node,
            expr_type,
            result,
        );
        let error_type = if has_some_applicable_signature {
            non_applicable_type
        } else {
            Some(expr_type)
        };
        if let Some(error_type) = error_type {
            let display = self.type_to_string_slice(error_type)?;
            self.error_at_node_array_range(
                node,
                type_arguments,
                &tsc_diagnostics::gen::Type_0_has_no_signatures_for_which_the_type_argument_list_is_applicable,
                &[&display],
            );
        }
        Ok(result)
    }

    /// getInstantiatedType (77992-78030): per-part instantiation with
    /// the two accumulator flags threaded through.
    fn get_instantiated_type(
        &mut self,
        ty: TypeId,
        node: NodeId,
        argument_nodes: &[NodeId],
        has_some_applicable_signature: &mut bool,
        non_applicable_type: &mut Option<TypeId>,
    ) -> CheckResult<TypeId> {
        let mut has_signatures = false;
        let mut has_applicable_signature = false;
        let result = self.get_instantiated_type_part(
            ty,
            node,
            argument_nodes,
            has_some_applicable_signature,
            non_applicable_type,
            &mut has_signatures,
            &mut has_applicable_signature,
        )?;
        *has_some_applicable_signature |= has_applicable_signature;
        if has_signatures && !has_applicable_signature {
            non_applicable_type.get_or_insert(ty);
        }
        Ok(result)
    }

    #[allow(clippy::too_many_arguments)]
    fn get_instantiated_type_part(
        &mut self,
        ty: TypeId,
        node: NodeId,
        argument_nodes: &[NodeId],
        has_some_applicable_signature: &mut bool,
        non_applicable_type: &mut Option<TypeId>,
        has_signatures: &mut bool,
        has_applicable_signature: &mut bool,
    ) -> CheckResult<TypeId> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::OBJECT) {
            let members_id = self.resolve_structured_type_members(ty)?;
            let (call_signatures, construct_signatures, members, properties, index_infos) = {
                let resolved = &self.members[members_id.0 as usize];
                (
                    resolved.call_signatures.clone(),
                    resolved.construct_signatures.clone(),
                    resolved.members.clone(),
                    resolved.properties.clone(),
                    resolved.index_infos.clone(),
                )
            };
            let instantiated_call =
                self.get_instantiated_signatures(&call_signatures, argument_nodes)?;
            let instantiated_construct =
                self.get_instantiated_signatures(&construct_signatures, argument_nodes)?;
            *has_signatures |= !call_signatures.is_empty() || !construct_signatures.is_empty();
            *has_applicable_signature |=
                !instantiated_call.is_empty() || !instantiated_construct.is_empty();
            if instantiated_call != call_signatures
                || instantiated_construct != construct_signatures
            {
                let symbol = self
                    .binder
                    .create_symbol(SymbolFlags::NONE, "__instantiationExpression".to_owned());
                let result = self.tables.create_type(TypeFlags::OBJECT, TypeData::Object);
                self.tables.type_mut(result).object_flags = tsc_types::ObjectFlags::ANONYMOUS
                    | tsc_types::ObjectFlags::INSTANTIATION_EXPRESSION_TYPE;
                self.tables.type_mut(result).symbol = Some(symbol);
                let members_id = self.alloc_members(crate::state::ResolvedMembers {
                    members,
                    properties,
                    call_signatures: instantiated_call,
                    construct_signatures: instantiated_construct,
                    index_infos,
                });
                self.links
                    .set_fresh_type_members(result, crate::links::LinkSlot::Resolved(members_id));
                // tsc stamps result.node = node (77999): the same
                // type.node field deferred references ride, so it
                // shares the deferred_node slot —
                // getObjectTypeInstantiation selects it as the
                // declaration (63464) and instantiateAnonymousType
                // propagates it to Instantiated copies (63649-63651).
                self.links
                    .set_fresh_type_deferred_reference_links(result, node, None);
                return Ok(result);
            }
            return Ok(ty);
        }
        if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE) {
            if let Some(constraint) = self.get_base_constraint_of_type(ty)? {
                let instantiated = self.get_instantiated_type_part(
                    constraint,
                    node,
                    argument_nodes,
                    has_some_applicable_signature,
                    non_applicable_type,
                    has_signatures,
                    has_applicable_signature,
                )?;
                if instantiated != constraint {
                    return Ok(instantiated);
                }
            }
            return Ok(ty);
        }
        if flags.intersects(TypeFlags::UNION) {
            let mapped = self.map_type(
                ty,
                &mut |state, t| {
                    state
                        .get_instantiated_type(
                            t,
                            node,
                            argument_nodes,
                            has_some_applicable_signature,
                            non_applicable_type,
                        )
                        .map(Some)
                },
                false,
            )?;
            return Ok(mapped.expect("mapper never returns None"));
        }
        if flags.intersects(TypeFlags::INTERSECTION) {
            let parts = match &self.tables.type_of(ty).data {
                TypeData::Intersection { types } => types.to_vec(),
                _ => unreachable!("intersection flag implies payload"),
            };
            let mut mapped = Vec::with_capacity(parts.len());
            let mut changed = false;
            for part in parts {
                let instantiated = self.get_instantiated_type_part(
                    part,
                    node,
                    argument_nodes,
                    has_some_applicable_signature,
                    non_applicable_type,
                    has_signatures,
                    has_applicable_signature,
                )?;
                changed |= instantiated != part;
                mapped.push(instantiated);
            }
            if changed {
                return self.get_intersection_type(&mapped, tsc_types::IntersectionFlags::NONE);
            }
            return Ok(ty);
        }
        Ok(ty)
    }

    /// getInstantiatedSignatures (78031-78045): arity filter + 2344
    /// constraint check (reportErrors=true) + instantiation.
    fn get_instantiated_signatures(
        &mut self,
        signatures: &[crate::state::SignatureId],
        argument_nodes: &[NodeId],
    ) -> CheckResult<Vec<crate::state::SignatureId>> {
        let mut applicable = Vec::new();
        for &signature in signatures {
            if self.signatures[signature.0 as usize]
                .type_parameters
                .is_none()
            {
                continue;
            }
            if self.has_correct_type_argument_arity(signature, argument_nodes) {
                applicable.push(signature);
            }
        }
        let mut result = Vec::with_capacity(applicable.len());
        for signature in applicable {
            let type_argument_types =
                self.check_type_arguments(signature, argument_nodes, true, None)?;
            result.push(match type_argument_types {
                Some(types) => {
                    self.get_signature_instantiation(signature, Some(&types), false, None)?
                }
                None => signature,
            });
        }
        Ok(result)
    }

    /// createDiagnosticForNodeArray over the typeArguments range: the
    /// 2635 span covers `<` .. `>`.
    fn error_at_node_array_range(
        &mut self,
        node: NodeId,
        array: Option<tsc_syntax::NodeArrayId>,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        args: &[&str],
    ) {
        let Some(array) = array else {
            self.error_at(Some(node), message, args);
            return;
        };
        let source = self.binder.source_of_node(node);
        let array = source.arena.node_array(array);
        let start_byte = tsc_syntax::skip_trivia(&source.text, array.pos as usize);
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start = to_utf16(start_byte);
        let end = to_utf16(array.end as usize);
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let diagnostic = tsc_diagnostics::Diagnostic::new(
            Some(source.file_name.clone()),
            Some(start),
            Some(end.saturating_sub(start)),
            tsc_diagnostics::MessageChain::new(message, &args),
        );
        self.push_error_diagnostic(diagnostic);
    }

    /// tsc-port: checkGrammarExpressionWithTypeArguments @6.0.3
    /// tsc-hash: 8ce018b7777277c725b0efb48645cf9915b3fd4f177967af6c7c0f67aa52f09c
    /// tsc-span: _tsc.js:89557-89562
    pub(crate) fn check_grammar_expression_with_type_arguments(&mut self, node: NodeId) -> bool {
        let (expression, type_arguments) = match self.data_of(node) {
            NodeData::ExpressionWithTypeArguments(data) => (data.expression, data.type_arguments),
            // The TypeQuery face (`typeof f<...>`) arrives from
            // getTypeFromTypeQueryNode: isExpressionWithTypeArguments
            // gates the import arm off, but checkGrammarTypeArguments
            // still runs over its list (89562).
            NodeData::TypeQuery(data) => (None, data.type_arguments),
            _ => (None, None),
        };
        if let (Some(expression), Some(_)) = (expression, type_arguments) {
            if self.kind_of(expression) == SyntaxKind::ImportKeyword {
                return self.grammar_error_on_node(
                    node,
                    &tsc_diagnostics::gen::This_use_of_import_is_invalid_import_calls_can_be_written_but_they_must_have_parentheses_and_cannot_have_type_arguments,
                    &[],
                );
            }
        }
        self.check_grammar_type_arguments(node, type_arguments)
    }

    /// checkGrammarTypeArguments (89537-89539).
    /// tsc-port: checkGrammarTypeArguments @6.0.3
    /// tsc-hash: 0ec184a91fd848949484577ec95c93edaedd1d12bcd1e82b5bfa2fca64b94c92
    /// tsc-span: _tsc.js:89537-89539
    pub(crate) fn check_grammar_type_arguments(
        &mut self,
        node: NodeId,
        type_arguments: Option<tsc_syntax::NodeArrayId>,
    ) -> bool {
        self.check_grammar_for_disallowed_trailing_comma(
            type_arguments,
            &tsc_diagnostics::gen::Trailing_comma_not_allowed,
        ) || self.check_grammar_for_at_least_one_type_argument(node, type_arguments)
    }

    /// checkGrammarForAtLeastOneTypeArgument (89528-89536).
    fn check_grammar_for_at_least_one_type_argument(
        &mut self,
        node: NodeId,
        type_arguments: Option<tsc_syntax::NodeArrayId>,
    ) -> bool {
        let Some(array_id) = type_arguments else {
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
        let end_byte = tsc_syntax::skip_trivia(&source.text, array.end as usize) + ">".len();
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start = to_utf16(start_byte);
        let end = to_utf16(end_byte);
        let diagnostic = tsc_diagnostics::Diagnostic::new(
            Some(source.file_name.clone()),
            Some(start),
            Some(end.saturating_sub(start)),
            tsc_diagnostics::MessageChain::new(
                &tsc_diagnostics::gen::Type_argument_list_cannot_be_empty,
                &[],
            ),
        );
        self.push_error_diagnostic(diagnostic);
        true
    }

    /// tsc-port: checkMetaProperty @6.0.3
    /// tsc-hash: 40d3c927b0108cab1d0dafd089583cd07e99905eaa55e4f0723b1bae3626150c
    /// tsc-span: _tsc.js:78061-78075
    ///
    /// MetaProperty carries no keywordToken slot — the leading source
    /// token disambiguates (parser convention).
    pub(crate) fn check_meta_property(&mut self, node: NodeId) -> CheckResult<TypeId> {
        if self.meta_property_name_text(node).is_none() {
            return Ok(self.tables.intrinsics.error);
        }
        self.check_grammar_meta_property(node)?;
        if self.meta_property_is_new(node) {
            return self.check_new_target_meta_property(node);
        }
        // checkMetaProperty returns errorType after the grammar check
        // for both the valid call-form and invalid bare import.defer;
        // it must not fall through to import.meta's module diagnostic.
        if self.meta_property_name_text(node).as_deref() == Some("defer") {
            return Ok(self.tables.intrinsics.error);
        }
        self.check_import_meta_property(node)
    }

    /// tsc-port: checkImportMetaProperty @6.0.3
    /// tsc-hash: 3938f818e61319328c4132e426bb9c8a42165afb64b01a38b68d2482c2ec3bab
    /// tsc-span: _tsc.js:78099-78110
    ///
    /// The PossiblyContainsImportMeta Debug.assert is a parser-flag
    /// internal invariant (elided).
    fn check_import_meta_property(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let module_kind = self.options.emit_module_kind();
        if (100..=199).contains(&module_kind) {
            if self.implied_node_format_for_file(node)
                == Some(crate::modules::ModuleResolutionMode::CommonJs)
            {
                self.error_at(
                    Some(node),
                    &tsc_diagnostics::gen::The_import_meta_meta_property_is_not_allowed_in_files_which_will_build_into_CommonJS_output,
                    &[],
                );
            }
        } else if module_kind < 6 && module_kind != 4 {
            self.error_at(
                Some(node),
                &tsc_diagnostics::gen::The_import_meta_meta_property_is_only_allowed_when_the_module_option_is_es2020_es2022_esnext_system_node16_node18_node20_or_nodenext,
                &[],
            );
        }
        if self.meta_property_name_text(node).as_deref() == Some("meta") {
            self.get_global_import_meta_type()
        } else {
            Ok(self.tables.intrinsics.error)
        }
    }

    /// The keywordToken read: `new.target` vs `import.meta` via the
    /// leading token (parser convention — no keywordToken slot).
    /// tsrs-native: parser-representation adapter; tsc reads the
    /// MetaProperty keywordToken field directly.
    pub(crate) fn meta_property_is_new(&self, node: NodeId) -> bool {
        let source = self.binder.source_of_node(node);
        let raw = source.arena.node(node);
        let start = tsc_syntax::skip_trivia(&source.text, raw.pos as usize);
        source.text[start..].starts_with("new")
    }

    fn meta_property_name_text(&self, node: NodeId) -> Option<String> {
        match self.data_of(node) {
            NodeData::MetaProperty(data) => data
                .name
                .and_then(|name| self.identifier_text_of(name))
                .map(str::to_owned),
            _ => None,
        }
    }

    /// checkGrammarMetaProperty (90187-90212): the 17012-family plus
    /// the `import.defer` callee shape.
    fn check_grammar_meta_property(&mut self, node: NodeId) -> CheckResult<()> {
        let Some(name_text) = self.meta_property_name_text(node) else {
            // Parsed meta-properties always have either their parsed
            // identifier or a missing identifier. A synthetic absent
            // name has no token span for this grammar diagnostic.
            return Ok(());
        };
        let name = match self.data_of(node) {
            NodeData::MetaProperty(data) => data.name.expect("name text read above"),
            _ => unreachable!("kind/data agree"),
        };
        if self.meta_property_is_new(node) {
            if name_text != "target" {
                self.grammar_error_on_node(
                    name,
                    &tsc_diagnostics::gen::_0_is_not_a_valid_meta_property_for_keyword_1_Did_you_mean_2,
                    &[&name_text, "new", "target"],
                );
            }
            return Ok(());
        }
        if name_text != "meta" {
            let is_callee = self.parent_of(node).is_some_and(|p| {
                matches!(self.data_of(p), NodeData::CallExpression(data) if data.expression == Some(node))
            });
            if name_text == "defer" {
                if !is_callee {
                    // grammarErrorAtPos(node, node.end, 0, `_0_expected`, "(")
                    let source = self.binder.source_of_node(node);
                    if source.parse_diagnostics.is_empty() {
                        let raw = source.arena.node(node);
                        let to_utf16 = |byte: usize| -> u32 {
                            source
                                .line_map
                                .byte_to_utf16
                                .get(byte)
                                .copied()
                                .unwrap_or(byte as u32)
                        };
                        let pos = to_utf16(raw.end as usize);
                        let diagnostic = tsc_diagnostics::Diagnostic::new(
                            Some(source.file_name.clone()),
                            Some(pos),
                            Some(0),
                            tsc_diagnostics::MessageChain::new(
                                &tsc_diagnostics::gen::_0_expected,
                                &["(".to_owned()],
                            ),
                        );
                        self.push_error_diagnostic(diagnostic);
                    }
                }
            } else if is_callee {
                self.grammar_error_on_node(
                    name,
                    &tsc_diagnostics::gen::_0_is_not_a_valid_meta_property_for_keyword_import_Did_you_mean_meta_or_defer,
                    &[&name_text],
                );
            } else {
                self.grammar_error_on_node(
                    name,
                    &tsc_diagnostics::gen::_0_is_not_a_valid_meta_property_for_keyword_1_Did_you_mean_2,
                    &[&name_text, "import", "meta"],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: checkNewTargetMetaProperty @6.0.3
    /// tsc-hash: b7eaa9a363187065d52804e093029188b18cde68a6e5dee0d7bdefeaea290093
    /// tsc-span: _tsc.js:78086-78098
    fn check_new_target_meta_property(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let source = self.binder.source_of_node(node);
        let container = node_util::get_this_container(source, node, false).filter(|&c| {
            matches!(
                self.kind_of(c),
                SyntaxKind::Constructor
                    | SyntaxKind::FunctionDeclaration
                    | SyntaxKind::FunctionExpression
            )
        });
        let Some(container) = container else {
            self.error_at(
                Some(node),
                &tsc_diagnostics::gen::Meta_property_0_is_only_allowed_in_the_body_of_a_function_declaration_function_expression_or_constructor,
                &["new.target"],
            );
            return Ok(self.tables.intrinsics.error);
        };
        let symbol = if self.kind_of(container) == SyntaxKind::Constructor {
            let class = self
                .parent_of(container)
                .expect("tree invariant: constructors always have a class parent");
            self.get_symbol_of_declaration(class)?
        } else {
            self.get_symbol_of_declaration(container)?
        };
        self.get_type_of_symbol(symbol)
    }

    // ---- conditional / template (80513-80556) ----

    /// tsc-port: checkConditionalExpression @6.0.3
    /// tsc-hash: ceb2c1e06c09b5c195ae1ccb64041b5f8dc9d77a8a218724366bccd1faa167aa
    /// tsc-span: _tsc.js:80513-80519
    pub(crate) fn check_conditional_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let NodeData::ConditionalExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let (condition, when_true, when_false) =
            match (data.condition, data.when_true, data.when_false) {
                (Some(c), Some(t), Some(f)) => (c, t, f),
                _ => return Ok(self.tables.intrinsics.error),
            };
        let condition_type = self.check_truthiness_expression(condition, check_mode)?;
        self.check_testing_known_truthy_callable_or_awaitable_or_enum_member_type(
            condition,
            condition_type,
            Some(when_true),
        )?;
        let type1 = self.check_expression(when_true, check_mode)?;
        let type2 = self.check_expression(when_false, check_mode)?;
        self.get_union_type_ex(&[type1, type2], tsc_types::UnionReduction::Subtype)
    }

    /// tsc-port: checkTemplateExpression @6.0.3
    /// tsc-hash: 010e13efd30265d55b738b043b54b63944f83779906702a943ea6d125bc3cd5b
    /// tsc-span: _tsc.js:80524-80546
    ///
    /// VERBATIM QUIRK: `const evaluated = … && evaluate(node).value;
    /// if (evaluated)` — an empty-string evaluation is FALSY and falls
    /// through to the contextual arms instead of returning the fresh
    /// literal.
    pub(crate) fn check_template_expression(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let NodeData::TemplateExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(head) = data.head else {
            return Ok(self.tables.intrinsics.error);
        };
        let spans = self.nodes_of(data.template_spans);
        let mut texts: Vec<tsc_types::TemplateText> = Vec::with_capacity(spans.len() + 1);
        match self.data_of(head) {
            NodeData::TemplateHead(data) => {
                texts.push(tsc_types::TemplateText::from_utf16(
                    &tsc_syntax::template_text_utf16(&data.text, data.raw_text.as_deref()),
                ));
            }
            _ => unreachable!("parser invariant: template heads carry TemplateHead data"),
        }
        let mut types: Vec<TypeId> = Vec::with_capacity(spans.len());
        for span in spans {
            let (expression, literal) = match self.data_of(span) {
                NodeData::TemplateSpan(data) => (data.expression, data.literal),
                _ => (None, None),
            };
            let Some(expression) = expression else {
                return Ok(self.tables.intrinsics.error);
            };
            let ty = self.check_expression(expression, CheckMode::NORMAL)?;
            if self.maybe_type_of_kind_considering_base_constraint(ty, TypeFlags::ES_SYMBOL_LIKE)? {
                self.error_at(
                    Some(expression),
                    &tsc_diagnostics::gen::Implicit_conversion_of_a_symbol_to_a_string_will_fail_at_runtime_Consider_wrapping_this_expression_in_String,
                    &[],
                );
            }
            let Some(literal) = literal else {
                return Ok(self.tables.intrinsics.error);
            };
            match self.data_of(literal) {
                NodeData::TemplateMiddle(data) => {
                    texts.push(tsc_types::TemplateText::from_utf16(
                        &tsc_syntax::template_text_utf16(&data.text, data.raw_text.as_deref()),
                    ));
                }
                NodeData::TemplateTail(data) => {
                    texts.push(tsc_types::TemplateText::from_utf16(
                        &tsc_syntax::template_text_utf16(&data.text, data.raw_text.as_deref()),
                    ));
                }
                _ => unreachable!(
                    "parser invariant: span literals are TemplateMiddle/TemplateTail (missing shape included)"
                ),
            }
            let template_constraint = self.tables.intrinsics.template_constraint;
            types.push(if self.is_type_assignable_to(ty, template_constraint)? {
                ty
            } else {
                self.tables.intrinsics.string
            });
        }
        let tagged = self
            .parent_of(node)
            .is_some_and(|p| self.kind_of(p) == SyntaxKind::TaggedTemplateExpression);
        if !tagged {
            let evaluated = self.evaluate(node, None)?;
            if let Some(crate::evaluate::EvalValue::Str(text)) = evaluated.value {
                if !text.is_empty() {
                    let literal = self.tables.get_string_literal_type(&text);
                    return Ok(self.tables.get_fresh_type_of_literal_type(literal));
                }
            }
        }
        let contextual_template = {
            let contextual = self
                .get_contextual_type(node, tsc_types::ContextFlags::NONE)?
                .unwrap_or(self.tables.intrinsics.unknown);
            self.some_type_result(contextual, |state, t| {
                state.is_template_literal_contextual_type(t)
            })?
        };
        if self.is_const_context(node)?
            || self.is_template_literal_context(node)
            || contextual_template
        {
            return Ok(self.get_template_literal_type_from_texts(&texts, &types));
        }
        Ok(self.tables.intrinsics.string)
    }

    /// isTemplateLiteralContext (80520-80523).
    fn is_template_literal_context(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        match self.data_of(parent) {
            NodeData::ParenthesizedExpression(_) => self.is_template_literal_context(parent),
            NodeData::ElementAccessExpression(data) => data.argument_expression == Some(node),
            _ => false,
        }
    }

    /// isTemplateLiteralContextualType (80547-80552).
    fn is_template_literal_contextual_type(&mut self, ty: TypeId) -> CheckResult<bool> {
        let flags = self.tables.flags_of(ty);
        if flags.intersects(TypeFlags::from_bits(
            TypeFlags::STRING_LITERAL.bits() | TypeFlags::TEMPLATE_LITERAL.bits(),
        )) {
            return Ok(true);
        }
        if flags.intersects(TypeFlags::INSTANTIABLE_NON_PRIMITIVE) {
            let base = self
                .get_base_constraint_of_type(ty)?
                .unwrap_or(self.tables.intrinsics.unknown);
            return Ok(self.maybe_type_of_kind(base, TypeFlags::STRING_LIKE));
        }
        Ok(false)
    }

    // ---- unary band (79303-79527) ----

    /// tsc-port: checkPrefixUnaryExpression @6.0.3
    /// tsc-hash: ce2d703ad735a596dffafae5428d11e31154787b20d3ebb480d5c3dfa6a5a23f
    /// tsc-span: _tsc.js:79427-79481
    ///
    /// The fresh-literal negation arms run FIRST (before the operator
    /// switch): -numeric, +numeric, -bigint — there is NO +bigint arm.
    pub(crate) fn check_prefix_unary_expression(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let NodeData::PrefixUnaryExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let operator = data.operator;
        let Some(operand) = data.operand else {
            return Ok(self.tables.intrinsics.error);
        };
        let operand_type = self.check_expression(operand, CheckMode::NORMAL)?;
        let silent_never = self.tables.intrinsics.silent_never;
        if operand_type == silent_never {
            return Ok(silent_never);
        }
        match self.kind_of(operand) {
            SyntaxKind::NumericLiteral => {
                let text = match self.data_of(operand) {
                    NodeData::NumericLiteral(data) => data.text.clone(),
                    _ => unreachable!("kind/data agree"),
                };
                match operator {
                    SyntaxKind::MinusToken => {
                        let value = crate::annotate::parse_numeric_literal_text(&text)?;
                        let literal = self.tables.get_number_literal_type(-value);
                        return Ok(self.tables.get_fresh_type_of_literal_type(literal));
                    }
                    SyntaxKind::PlusToken => {
                        let value = crate::annotate::parse_numeric_literal_text(&text)?;
                        let literal = self.tables.get_number_literal_type(value);
                        return Ok(self.tables.get_fresh_type_of_literal_type(literal));
                    }
                    _ => {}
                }
            }
            SyntaxKind::BigIntLiteral => {
                if operator == SyntaxKind::MinusToken {
                    let text = match self.data_of(operand) {
                        NodeData::BigIntLiteral(data) => data.text.clone(),
                        _ => unreachable!("kind/data agree"),
                    };
                    let parsed = crate::expr::parse_pseudo_big_int(&text)?;
                    let literal = self
                        .tables
                        .get_bigint_literal_type(tsc_types::PseudoBigInt {
                            negative: true,
                            base10_value: parsed.base10_value,
                        });
                    return Ok(self.tables.get_fresh_type_of_literal_type(literal));
                }
            }
            _ => {}
        }
        match operator {
            SyntaxKind::PlusToken | SyntaxKind::MinusToken | SyntaxKind::TildeToken => {
                self.check_non_null_type(operand_type, operand)?;
                if self.maybe_type_of_kind_considering_base_constraint(
                    operand_type,
                    TypeFlags::ES_SYMBOL_LIKE,
                )? {
                    self.error_at(
                        Some(operand),
                        &tsc_diagnostics::gen::The_0_operator_cannot_be_applied_to_type_symbol,
                        &[token_text(operator)],
                    );
                }
                if operator == SyntaxKind::PlusToken {
                    if self.maybe_type_of_kind_considering_base_constraint(
                        operand_type,
                        TypeFlags::BIG_INT_LIKE,
                    )? {
                        let base = self.get_base_type_of_literal_type(operand_type)?;
                        let display = self.type_to_string_slice(base)?;
                        self.error_at(
                            Some(operand),
                            &tsc_diagnostics::gen::Operator_0_cannot_be_applied_to_type_1,
                            &[token_text(operator), &display],
                        );
                    }
                    return Ok(self.tables.intrinsics.number);
                }
                self.get_unary_result_type(operand_type)
            }
            SyntaxKind::ExclamationToken => {
                self.check_truthiness_of_type(operand_type, operand)?;
                let facts = self.get_type_facts(
                    operand_type,
                    TypeFacts::from_bits(TypeFacts::TRUTHY.bits() | TypeFacts::FALSY.bits()),
                )?;
                Ok(if facts == TypeFacts::TRUTHY {
                    self.tables.intrinsics.false_fresh
                } else if facts == TypeFacts::FALSY {
                    self.tables.intrinsics.true_fresh
                } else {
                    self.tables.intrinsics.boolean
                })
            }
            SyntaxKind::PlusPlusToken | SyntaxKind::MinusMinusToken => {
                let nonnull = self.check_non_null_type(operand_type, operand)?;
                let ok = self.check_arithmetic_operand_type(
                    operand,
                    nonnull,
                    &tsc_diagnostics::gen::An_arithmetic_operand_must_be_of_type_any_number_bigint_or_an_enum_type,
                    false,
                )?;
                if ok {
                    self.check_reference_expression(
                        operand,
                        &tsc_diagnostics::gen::The_operand_of_an_increment_or_decrement_operator_must_be_a_variable_or_a_property_access,
                        &tsc_diagnostics::gen::The_operand_of_an_increment_or_decrement_operator_may_not_be_an_optional_property_access,
                    );
                }
                self.get_unary_result_type(operand_type)
            }
            _ => Ok(self.tables.intrinsics.error),
        }
    }

    /// tsc-port: checkPostfixUnaryExpression @6.0.3
    /// tsc-hash: 4dc8a03f4704bc2594e5fbf7558593fe7d3760e79b9523ea96c16038dad68e7b
    /// tsc-span: _tsc.js:79482-79500
    pub(crate) fn check_postfix_unary_expression(&mut self, node: NodeId) -> CheckResult<TypeId> {
        let NodeData::PostfixUnaryExpression(data) = self.data_of(node) else {
            unreachable!("kind/data agree");
        };
        let Some(operand) = data.operand else {
            return Ok(self.tables.intrinsics.error);
        };
        let operand_type = self.check_expression(operand, CheckMode::NORMAL)?;
        let silent_never = self.tables.intrinsics.silent_never;
        if operand_type == silent_never {
            return Ok(silent_never);
        }
        let nonnull = self.check_non_null_type(operand_type, operand)?;
        let ok = self.check_arithmetic_operand_type(
            operand,
            nonnull,
            &tsc_diagnostics::gen::An_arithmetic_operand_must_be_of_type_any_number_bigint_or_an_enum_type,
            false,
        )?;
        if ok {
            self.check_reference_expression(
                operand,
                &tsc_diagnostics::gen::The_operand_of_an_increment_or_decrement_operator_must_be_a_variable_or_a_property_access,
                &tsc_diagnostics::gen::The_operand_of_an_increment_or_decrement_operator_may_not_be_an_optional_property_access,
            );
        }
        self.get_unary_result_type(operand_type)
    }

    /// tsc-port: getUnaryResultType @6.0.3
    /// tsc-hash: f024c6fb24dcb5c257b1e877dadb2996ccbb789df9dd13d1924f4cd6753a7ad6
    /// tsc-span: _tsc.js:79501-79508
    fn get_unary_result_type(&mut self, operand_type: TypeId) -> CheckResult<TypeId> {
        if self.maybe_type_of_kind(operand_type, TypeFlags::BIG_INT_LIKE) {
            let mixes_number =
                self.is_type_assignable_to_kind(operand_type, TypeFlags::ANY_OR_UNKNOWN, false)?
                    || self.maybe_type_of_kind(operand_type, TypeFlags::NUMBER_LIKE);
            return Ok(if mixes_number {
                self.tables.intrinsics.number_or_bigint
            } else {
                self.tables.intrinsics.bigint
            });
        }
        Ok(self.tables.intrinsics.number)
    }

    /// tsc-port: checkGrammarForDisallowedTrailingComma @6.0.3
    /// tsc-hash: a5d0b4c4a4810896c9476e5225fd51689524651d31235eee9e117723808a263c
    /// tsc-span: _tsc.js:89401-89408
    ///
    /// grammarErrorAtPos(list.end - 1, 1, message), parse-diagnostic
    /// suppressed like every grammar row.
    pub(crate) fn check_grammar_for_disallowed_trailing_comma(
        &mut self,
        list: Option<tsc_syntax::NodeArrayId>,
        message: &'static tsc_diagnostics::DiagnosticMessage,
    ) -> bool {
        let Some(list) = list else {
            return false;
        };
        let Some(current) = self.current_node else {
            return false;
        };
        let source = self.binder.source_of_node(current);
        let array = source.arena.node_array(list);
        if !array.has_trailing_comma {
            return false;
        }
        if !source.parse_diagnostics.is_empty() {
            return false;
        }
        let to_utf16 = |byte: usize| -> u32 {
            source
                .line_map
                .byte_to_utf16
                .get(byte)
                .copied()
                .unwrap_or(byte as u32)
        };
        let start = to_utf16(array.end as usize - 1);
        let end = to_utf16(array.end as usize);
        let diagnostic = tsc_diagnostics::Diagnostic::new(
            Some(source.file_name.clone()),
            Some(start),
            Some(end.saturating_sub(start)),
            tsc_diagnostics::MessageChain::new(message, &[]),
        );
        self.push_error_diagnostic(diagnostic);
        true
    }

    /// tsc-port: getRestType @6.0.3
    /// tsc-hash: c1ed5a63f503979fe12cc660c82e604821fc5a6abc7ca14c6f6170843b0aa350
    /// tsc-span: _tsc.js:55841-55884
    pub(crate) fn get_rest_type(
        &mut self,
        source: TypeId,
        properties: &[NodeId],
        symbol: Option<SymbolId>,
    ) -> CheckResult<TypeId> {
        let source = self.tables.filter_type(source, |tables, t| {
            !tables.flags_of(t).intersects(TypeFlags::NULLABLE)
        });
        if self.tables.flags_of(source).intersects(TypeFlags::NEVER) {
            return Ok(self.empty_object_type);
        }
        if self.tables.flags_of(source).intersects(TypeFlags::UNION) {
            let mapped = self.map_type(
                source,
                &mut |state, t| state.get_rest_type(t, properties, symbol).map(Some),
                false,
            )?;
            return Ok(mapped.expect("mapper never returns None"));
        }
        let mut key_types = Vec::with_capacity(properties.len());
        for &name in properties {
            key_types.push(self.get_literal_type_from_property_name(name)?);
        }
        let mut omit_key_type =
            self.get_union_type_ex(&key_types, tsc_types::UnionReduction::Literal)?;
        let mut spreadable_properties: Vec<SymbolId> = Vec::new();
        let mut unspreadable_to_rest_keys: Vec<TypeId> = Vec::new();
        for prop in self.get_properties_of_type(source)? {
            let literal_type_from_property = self.get_literal_type_from_property(
                prop,
                TypeFlags::STRING_OR_NUMBER_LITERAL_OR_UNIQUE,
                false,
            )?;
            let omitted = self.is_type_assignable_to(literal_type_from_property, omit_key_type)?;
            let private_or_protected = self
                .get_declaration_modifier_flags_from_symbol(prop)
                .intersects(tsc_types::ModifierFlags::from_bits(
                    tsc_types::ModifierFlags::PRIVATE.bits()
                        | tsc_types::ModifierFlags::PROTECTED.bits(),
                ));
            if !omitted && !private_or_protected && self.is_spreadable_property(prop) {
                spreadable_properties.push(prop);
            } else {
                unspreadable_to_rest_keys.push(literal_type_from_property);
            }
        }
        if self.is_generic_object_type_state(source)?
            || self.is_generic_index_type_state(omit_key_type)?
        {
            if !unspreadable_to_rest_keys.is_empty() {
                let mut all = vec![omit_key_type];
                all.extend(unspreadable_to_rest_keys);
                omit_key_type = self.get_union_type_ex(&all, tsc_types::UnionReduction::Literal)?;
            }
            if self
                .tables
                .flags_of(omit_key_type)
                .intersects(TypeFlags::NEVER)
            {
                return Ok(source);
            }
            let omit_alias = self.get_global_omit_symbol()?;
            let Some(omit_alias) = omit_alias else {
                return Ok(self.tables.intrinsics.error);
            };
            return self.get_type_alias_instantiation(
                omit_alias,
                Some(&[source, omit_key_type]),
                None,
                None,
            );
        }
        let mut members = tsc_binder::SymbolTable::default();
        let mut result_properties = Vec::with_capacity(spreadable_properties.len());
        for prop in spreadable_properties {
            let spread = self.get_spread_symbol(prop, /*readonly*/ false)?;
            members.insert(self.binder.symbol(prop).escaped_name.clone(), spread);
            result_properties.push(spread);
        }
        let index_infos = self.get_index_infos_of_type(source)?;
        let result = self.make_resolved_anonymous_type(
            symbol,
            members,
            result_properties,
            index_infos,
            tsc_types::ObjectFlags::OBJECT_REST_TYPE,
        );
        Ok(result)
    }

    /// tsc-port: getGlobalOmitSymbol @6.0.3
    /// tsc-hash: eef048565108e47aa85500fb2a7275defd8d8a63cd9193179c79610e42230b80
    /// tsc-span: _tsc.js:60917-60926
    ///
    /// Miss AND wrong-arity (2317 in the shared worker) memoize
    /// unknownSymbol.
    fn get_global_omit_symbol(&mut self) -> CheckResult<Option<SymbolId>> {
        if self.deferred_global_omit_symbol.is_none() {
            let resolved =
                self.get_global_type_alias_symbol("Omit", 2, /*report_errors*/ true)?;
            self.deferred_global_omit_symbol = Some(Some(resolved.unwrap_or(self.unknown_symbol)));
        }
        let memo = self
            .deferred_global_omit_symbol
            .expect("filled above")
            .expect("memo holds symbol-or-unknown");
        Ok((memo != self.unknown_symbol).then_some(memo))
    }

    // ---- nullish-coalescing operand probes ----

    /// tsc-port: checkNullishCoalesceOperands @6.0.3
    /// tsc-hash: e65c9ba2aeddc35f3a8478118a2c0c72ce71ad0b94badd51ad543970dc1c18fa
    /// tsc-span: _tsc.js:79936-79957
    ///
    /// The 5076 mixing rows are grammarErrorOnNode (SUPPRESSED in
    /// files with parse diagnostics — the 5.5d suppression class).
    fn check_nullish_coalesce_operands(&mut self, node: NodeId) -> CheckResult<()> {
        let Some((left, operator_token, right)) = self.binary_parts(node) else {
            return Ok(());
        };
        if self.operator_kind(operator_token) != SyntaxKind::QuestionQuestionToken {
            return Ok(());
        }
        let parent = self.parent_of(node);
        if let Some(parent) = parent.filter(|&p| self.kind_of(p) == SyntaxKind::BinaryExpression) {
            if let Some((parent_left, parent_op, _)) = self.binary_parts(parent) {
                if self.kind_of(parent_left) == SyntaxKind::BinaryExpression
                    && self.operator_kind(parent_op) == SyntaxKind::BarBarToken
                {
                    let qq = tsc_syntax::tokens::token_to_string(SyntaxKind::QuestionQuestionToken)
                        .expect("?? has token text");
                    let bar = tsc_syntax::tokens::token_to_string(self.operator_kind(parent_op))
                        .expect("|| has token text");
                    self.grammar_error_on_node(
                        parent_left,
                        &tsc_diagnostics::gen::_0_and_1_operations_cannot_be_mixed_without_parentheses,
                        &[qq, bar],
                    );
                }
            }
        } else if self.kind_of(left) == SyntaxKind::BinaryExpression {
            if let Some((_, left_op, _)) = self.binary_parts(left) {
                let left_operator = self.operator_kind(left_op);
                if matches!(
                    left_operator,
                    SyntaxKind::BarBarToken | SyntaxKind::AmpersandAmpersandToken
                ) {
                    let op = tsc_syntax::tokens::token_to_string(left_operator)
                        .expect("logical operators have token text");
                    let qq = tsc_syntax::tokens::token_to_string(SyntaxKind::QuestionQuestionToken)
                        .expect("?? has token text");
                    self.grammar_error_on_node(
                        left,
                        &tsc_diagnostics::gen::_0_and_1_operations_cannot_be_mixed_without_parentheses,
                        &[op, qq],
                    );
                }
            }
        } else if self.kind_of(right) == SyntaxKind::BinaryExpression {
            if let Some((_, right_op, _)) = self.binary_parts(right) {
                if self.operator_kind(right_op) == SyntaxKind::AmpersandAmpersandToken {
                    let qq = tsc_syntax::tokens::token_to_string(SyntaxKind::QuestionQuestionToken)
                        .expect("?? has token text");
                    let amp =
                        tsc_syntax::tokens::token_to_string(SyntaxKind::AmpersandAmpersandToken)
                            .expect("&& has token text");
                    self.grammar_error_on_node(
                        right,
                        &tsc_diagnostics::gen::_0_and_1_operations_cannot_be_mixed_without_parentheses,
                        &[qq, amp],
                    );
                }
            }
        }
        self.check_nullish_coalesce_operand_left(node)
    }

    /// tsc-port: checkNullishCoalesceOperandLeft @6.0.3
    /// tsc-hash: 251fcefd2d46564907d3f01a0b31b24fb40a95b124e8c1dc54c124872419a0aa
    /// tsc-span: _tsc.js:79958-79968
    fn check_nullish_coalesce_operand_left(&mut self, node: NodeId) -> CheckResult<()> {
        let Some((left, _, _)) = self.binary_parts(node) else {
            return Ok(());
        };
        let left_target = self.skip_outer_expressions(left, OuterExpressionKinds::ALL);
        let nullish_semantics = self.get_syntactic_nullishness_semantics(left_target)?;
        if nullish_semantics != SEMANTICS_SOMETIMES {
            if nullish_semantics == SEMANTICS_ALWAYS {
                self.error_at(
                    Some(left_target),
                    &tsc_diagnostics::gen::This_expression_is_always_nullish,
                    &[],
                );
            } else {
                self.error_at(
                    Some(left_target),
                    &tsc_diagnostics::gen::Right_operand_of_is_unreachable_because_the_left_operand_is_never_nullish,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// tsc-port: getSyntacticNullishnessSemantics @6.0.3
    /// tsc-hash: 40f713def8fb8b2bbf4ded4fd4e6a1bda376c3fc02fe576af13ca9335ce94f2e
    /// tsc-span: _tsc.js:79969-80008
    fn get_syntactic_nullishness_semantics(&mut self, node: NodeId) -> CheckResult<u8> {
        let node = self.skip_outer_expressions(node, OuterExpressionKinds::ALL);
        Ok(match self.kind_of(node) {
            SyntaxKind::AwaitExpression
            | SyntaxKind::CallExpression
            | SyntaxKind::TaggedTemplateExpression
            | SyntaxKind::ElementAccessExpression
            | SyntaxKind::MetaProperty
            | SyntaxKind::NewExpression
            | SyntaxKind::PropertyAccessExpression
            | SyntaxKind::YieldExpression
            | SyntaxKind::ThisKeyword => SEMANTICS_SOMETIMES,
            SyntaxKind::BinaryExpression => {
                let Some((_, operator_token, right)) = self.binary_parts(node) else {
                    return Ok(SEMANTICS_SOMETIMES);
                };
                match self.operator_kind(operator_token) {
                    SyntaxKind::BarBarToken
                    | SyntaxKind::BarBarEqualsToken
                    | SyntaxKind::AmpersandAmpersandToken
                    | SyntaxKind::AmpersandAmpersandEqualsToken => SEMANTICS_SOMETIMES,
                    // For these operator kinds, the right operand is
                    // effectively controlling.
                    SyntaxKind::CommaToken
                    | SyntaxKind::EqualsToken
                    | SyntaxKind::QuestionQuestionToken
                    | SyntaxKind::QuestionQuestionEqualsToken => {
                        self.get_syntactic_nullishness_semantics(right)?
                    }
                    _ => SEMANTICS_NEVER,
                }
            }
            SyntaxKind::ConditionalExpression => {
                let NodeData::ConditionalExpression(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let (when_true, when_false) = match (data.when_true, data.when_false) {
                    (Some(t), Some(f)) => (t, f),
                    _ => return Ok(SEMANTICS_SOMETIMES),
                };
                self.get_syntactic_nullishness_semantics(when_true)?
                    | self.get_syntactic_nullishness_semantics(when_false)?
            }
            SyntaxKind::NullKeyword => SEMANTICS_ALWAYS,
            SyntaxKind::Identifier => {
                if self.is_undefined_identifier_resolving_to_global(node)? {
                    SEMANTICS_ALWAYS
                } else {
                    SEMANTICS_SOMETIMES
                }
            }
            _ => SEMANTICS_NEVER,
        })
    }

    /// getResolvedSymbol(node) === undefinedSymbol (79999): the global
    /// `undefined` identifier test the nullish/truthy classifiers
    /// share.
    fn is_undefined_identifier_resolving_to_global(&mut self, node: NodeId) -> CheckResult<bool> {
        if self.identifier_text_of(node) != Some("undefined") {
            return Ok(false);
        }
        Ok(match self.get_resolved_symbol(node)? {
            Some(symbol) => symbol == self.undefined_symbol,
            None => false,
        })
    }

    /// tsc-port: skipOuterExpressions @6.0.3
    /// tsc-hash: 8b1eff7c004dde6bbe6b5940ba064195f1aea6668ca5d8b1f4a69bf9cec4dec1
    /// tsc-span: _tsc.js:27582-27587
    ///
    pub(crate) fn skip_outer_expressions(
        &self,
        mut node: NodeId,
        kinds: OuterExpressionKinds,
    ) -> NodeId {
        loop {
            let is_outer = match self.kind_of(node) {
                SyntaxKind::ParenthesizedExpression => {
                    if kinds.intersects(OuterExpressionKinds::EXCLUDE_JSDOC_TYPE_ASSERTIONS)
                        && node_util::is_jsdoc_type_assertion(
                            self.binder.source_of_node(node),
                            node,
                        )
                    {
                        false
                    } else {
                        kinds.intersects(OuterExpressionKinds::PARENTHESES)
                    }
                }
                SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression => {
                    kinds.intersects(OuterExpressionKinds::TYPE_ASSERTIONS)
                }
                SyntaxKind::SatisfiesExpression => kinds.intersects(OuterExpressionKinds(
                    OuterExpressionKinds::TYPE_ASSERTIONS.0 | OuterExpressionKinds::SATISFIES.0,
                )),
                SyntaxKind::ExpressionWithTypeArguments => {
                    kinds.intersects(OuterExpressionKinds::EXPRESSIONS_WITH_TYPE_ARGUMENTS)
                }
                SyntaxKind::NonNullExpression => {
                    kinds.intersects(OuterExpressionKinds::NON_NULL_ASSERTIONS)
                }
                _ => false,
            };
            if !is_outer {
                return node;
            }
            let inner = match self.data_of(node) {
                NodeData::ParenthesizedExpression(data) => data.expression,
                NodeData::TypeAssertionExpression(data) => data.expression,
                NodeData::AsExpression(data) => data.expression,
                NodeData::SatisfiesExpression(data) => data.expression,
                NodeData::ExpressionWithTypeArguments(data) => data.expression,
                NodeData::NonNullExpression(data) => data.expression,
                _ => None,
            };
            match inner {
                Some(inner) => node = inner,
                None => return node,
            }
        }
    }

    // ---- truthiness classifiers ----

    /// tsc-port: checkTruthinessExpression @6.0.3
    /// tsc-hash: 6b88ebed5cc1e6e290bfc7dbba0dd35e621e812f3bfe5a169993efde46aca71f
    /// tsc-span: _tsc.js:83792-83794
    pub(crate) fn check_truthiness_expression(
        &mut self,
        node: NodeId,
        check_mode: CheckMode,
    ) -> CheckResult<TypeId> {
        let ty = self.check_expression(node, check_mode)?;
        self.check_truthiness_of_type(ty, node)
    }

    /// tsc-port: checkTruthinessOfType @6.0.3
    /// tsc-hash: 6d0e0ab49571d21302779a7632807717a64c56f453ee40768afa6defe4444917
    /// tsc-span: _tsc.js:83748-83761
    pub(crate) fn check_truthiness_of_type(
        &mut self,
        ty: TypeId,
        node: NodeId,
    ) -> CheckResult<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::VOID) {
            self.error_at(
                Some(node),
                &tsc_diagnostics::gen::An_expression_of_type_void_cannot_be_tested_for_truthiness,
                &[],
            );
        } else {
            let semantics = self.get_syntactic_truthy_semantics(node)?;
            if semantics != SEMANTICS_SOMETIMES {
                let message = if semantics == SEMANTICS_ALWAYS {
                    &tsc_diagnostics::gen::This_kind_of_expression_is_always_truthy
                } else {
                    &tsc_diagnostics::gen::This_kind_of_expression_is_always_falsy
                };
                self.error_at(Some(node), message, &[]);
            }
        }
        Ok(ty)
    }

    /// tsc-port: getSyntacticTruthySemantics @6.0.3
    /// tsc-hash: 844fd4aace0569dd48862b6399cd00a3af6507e8a9ebcb9903be662792f5de04
    /// tsc-span: _tsc.js:83762-83791
    fn get_syntactic_truthy_semantics(&mut self, node: NodeId) -> CheckResult<u8> {
        let node = self.skip_outer_expressions(node, OuterExpressionKinds::ALL);
        Ok(match self.kind_of(node) {
            SyntaxKind::NumericLiteral => {
                let NodeData::NumericLiteral(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                if data.text == "0" || data.text == "1" {
                    SEMANTICS_SOMETIMES
                } else {
                    SEMANTICS_ALWAYS
                }
            }
            SyntaxKind::ArrayLiteralExpression
            | SyntaxKind::ArrowFunction
            | SyntaxKind::BigIntLiteral
            | SyntaxKind::ClassExpression
            | SyntaxKind::FunctionExpression
            | SyntaxKind::JsxElement
            | SyntaxKind::JsxSelfClosingElement
            | SyntaxKind::ObjectLiteralExpression
            | SyntaxKind::RegularExpressionLiteral => SEMANTICS_ALWAYS,
            SyntaxKind::VoidExpression | SyntaxKind::NullKeyword => SEMANTICS_NEVER,
            SyntaxKind::NoSubstitutionTemplateLiteral | SyntaxKind::StringLiteral => {
                let text_empty = match self.data_of(node) {
                    NodeData::StringLiteral(data) => data.text.is_empty(),
                    NodeData::NoSubstitutionTemplateLiteral(data) => data.text.is_empty(),
                    _ => unreachable!("kind/data agree"),
                };
                if text_empty {
                    SEMANTICS_NEVER
                } else {
                    SEMANTICS_ALWAYS
                }
            }
            SyntaxKind::ConditionalExpression => {
                let NodeData::ConditionalExpression(data) = self.data_of(node) else {
                    unreachable!("kind/data agree");
                };
                let (when_true, when_false) = match (data.when_true, data.when_false) {
                    (Some(t), Some(f)) => (t, f),
                    _ => return Ok(SEMANTICS_SOMETIMES),
                };
                self.get_syntactic_truthy_semantics(when_true)?
                    | self.get_syntactic_truthy_semantics(when_false)?
            }
            SyntaxKind::Identifier => {
                if self.is_undefined_identifier_resolving_to_global(node)? {
                    SEMANTICS_NEVER
                } else {
                    SEMANTICS_SOMETIMES
                }
            }
            _ => SEMANTICS_SOMETIMES,
        })
    }

    // ---- the truthy-callable/awaitable/enum-member condition check ----

    /// tsc-port: checkTestingKnownTruthyCallableOrAwaitableOrEnumMemberType @6.0.3
    /// tsc-hash: 81f77aa2139edada681d85860134e49f620840fd95c4a2b1d31a575b6b3aeb14
    /// tsc-span: _tsc.js:83636-83689
    ///
    /// 2845 enum-literal condition, 2774 always-defined function,
    /// 2801 always-defined promise. strictNullChecks-only.
    pub(crate) fn check_testing_known_truthy_callable_or_awaitable_or_enum_member_type(
        &mut self,
        cond_expr: NodeId,
        cond_type: TypeId,
        body: Option<NodeId>,
    ) -> CheckResult<()> {
        if !self
            .options
            .strict_option_value(self.options.strict_null_checks)
        {
            return Ok(());
        }
        self.truthy_callable_both_helper(cond_expr, cond_type, body)
    }

    /// bothHelper (83639-83646): the condition plus every `||`/`??`
    /// left successively.
    fn truthy_callable_both_helper(
        &mut self,
        walk: NodeId,
        cond_type: TypeId,
        body: Option<NodeId>,
    ) -> CheckResult<()> {
        let source = self.binder.source_of_node(walk);
        let mut cond_expr2 = node_util::skip_parentheses_pub(source, walk);
        self.truthy_callable_helper(cond_expr2, cond_type, body)?;
        while self.kind_of(cond_expr2) == SyntaxKind::BinaryExpression {
            let Some((left, operator_token, _)) = self.binary_parts(cond_expr2) else {
                break;
            };
            if !matches!(
                self.operator_kind(operator_token),
                SyntaxKind::BarBarToken | SyntaxKind::QuestionQuestionToken
            ) {
                break;
            }
            cond_expr2 = node_util::skip_parentheses_pub(self.binder.source_of_node(left), left);
            self.truthy_callable_helper(cond_expr2, cond_type, body)?;
        }
        Ok(())
    }

    /// helper (83647-83689). `location === condExpr2` reuses condType —
    /// including every walked `||`/`??` LEFT from bothHelper's loop
    /// (83657 has no other condition); only a logical binary's right
    /// side re-checks its expression.
    fn truthy_callable_helper(
        &mut self,
        cond_expr2: NodeId,
        cond_type: TypeId,
        body: Option<NodeId>,
    ) -> CheckResult<()> {
        let source = self.binder.source_of_node(cond_expr2);
        let location = if node_util::is_logical_or_coalescing_binary_expression(source, cond_expr2)
        {
            match self.binary_parts(cond_expr2) {
                Some((_, _, right)) => {
                    node_util::skip_parentheses_pub(self.binder.source_of_node(right), right)
                }
                None => return Ok(()),
            }
        } else {
            cond_expr2
        };
        // isModuleExportsAccessExpression: `module.exports` — a JS
        // shape; TS files never bind `module` this way, and JS files
        // are band-gated. The test reduces to false.
        let location_source = self.binder.source_of_node(location);
        if node_util::is_logical_or_coalescing_binary_expression(location_source, location) {
            return self.truthy_callable_both_helper(location, cond_type, body);
        }
        let ty = if location == cond_expr2 {
            cond_type
        } else {
            self.check_expression(location, CheckMode::NORMAL)?
        };
        if self.tables.flags_of(ty).intersects(TypeFlags::ENUM_LITERAL)
            && self.kind_of(location) == SyntaxKind::PropertyAccessExpression
        {
            let NodeData::PropertyAccessExpression(data) = self.data_of(location) else {
                unreachable!("kind/data agree");
            };
            let receiver = data
                .expression
                .expect("parser invariant: PropertyAccess expression always parsed");
            let receiver_symbol = self.links.node(receiver).resolved_symbol.resolved();
            let receiver_is_enum = receiver_symbol.is_some_and(|symbol| {
                symbol != self.unknown_symbol
                    && self
                        .binder
                        .symbol(symbol)
                        .flags
                        .intersects(SymbolFlags::ENUM)
            });
            if receiver_is_enum {
                let truthy = self.enum_literal_type_is_truthy(ty)?;
                self.error_at(
                    Some(location),
                    &tsc_diagnostics::gen::This_condition_will_always_return_0,
                    &[if truthy { "true" } else { "false" }],
                );
                return Ok(());
            }
        }
        let is_property_expression_cast = self.kind_of(location)
            == SyntaxKind::PropertyAccessExpression
            && match self.data_of(location) {
                NodeData::PropertyAccessExpression(data) => data
                    .expression
                    .is_some_and(|expr| self.is_type_assertion_kind(expr)),
                _ => false,
            };
        if !self.has_type_facts(ty, TypeFacts::TRUTHY)? || is_property_expression_cast {
            return Ok(());
        }
        let call_signatures = self.get_signatures_of_type(ty, SignatureKind::Call)?;
        let is_promise = self.get_awaited_type_of_promise(ty)?.is_some();
        if call_signatures.is_empty() && !is_promise {
            return Ok(());
        }
        let tested_node = match self.kind_of(location) {
            SyntaxKind::Identifier => Some(location),
            SyntaxKind::PropertyAccessExpression => match self.data_of(location) {
                NodeData::PropertyAccessExpression(data) => data.name,
                _ => None,
            },
            _ => None,
        };
        let tested_symbol = match tested_node {
            Some(node) => self.get_symbol_at_location_for_condition_walker(node)?,
            None => None,
        };
        if tested_symbol.is_none() && !is_promise {
            return Ok(());
        }
        let mut is_used = false;
        if let Some(symbol) = tested_symbol {
            if let Some(parent) = self.parent_of(cond_expr2) {
                if self.kind_of(parent) == SyntaxKind::BinaryExpression
                    && self.is_symbol_used_in_binary_expression_chain(parent, symbol)?
                {
                    is_used = true;
                }
            }
            if !is_used {
                if let (Some(body), Some(tested_node)) = (body, tested_node) {
                    if self.is_symbol_used_in_condition_body(
                        cond_expr2,
                        body,
                        tested_node,
                        symbol,
                    )? {
                        is_used = true;
                    }
                }
            }
        }
        if !is_used {
            if is_promise {
                let display = self.get_type_name_for_error_display(ty)?;
                self.error_and_maybe_suggest_await(
                    location,
                    true,
                    &tsc_diagnostics::gen::This_condition_will_always_return_true_since_this_0_is_always_defined,
                    &[&display],
                );
            } else {
                self.error_at(
                    Some(location),
                    &tsc_diagnostics::gen::This_condition_will_always_return_true_since_this_function_is_always_defined_Did_you_mean_to_call_it_instead,
                    &[],
                );
            }
        }
        Ok(())
    }

    /// The EnumLiteral truthiness read (83657: `!!type.value`).
    fn enum_literal_type_is_truthy(&self, ty: TypeId) -> CheckResult<bool> {
        match &self.tables.type_of(ty).data {
            TypeData::Literal { value } => Ok(match value {
                LiteralValue::String(text) => !text.is_empty(),
                LiteralValue::Number(value) => *value != 0.0,
                LiteralValue::BigInt(value) => value.base10_value != "0",
            }),
            // 83657 reads `.value` off the type: non-Literal payloads
            // (opaque computed enum members) carry none, and
            // `!!undefined` is false.
            _ => Ok(false),
        }
    }

    /// isTypeAssertion (skipParentheses + kind test) on a receiver.
    fn is_type_assertion_kind(&self, node: NodeId) -> bool {
        let node = node_util::skip_parentheses_pub(self.binder.source_of_node(node), node);
        matches!(
            self.kind_of(node),
            SyntaxKind::TypeAssertionExpression | SyntaxKind::AsExpression
        )
    }

    /// tsc-port: isSymbolUsedInConditionBody @6.0.3
    /// tsc-hash: 1541546be6d8c30f10ece7c5ffcbe6081f3c203c57a712880fd5ab17ad1ecaf8
    /// tsc-span: _tsc.js:83690-83721
    fn is_symbol_used_in_condition_body(
        &mut self,
        expr: NodeId,
        body: NodeId,
        tested_node: NodeId,
        tested_symbol: SymbolId,
    ) -> CheckResult<bool> {
        // forEachChild(body, check): seed with the body's children.
        let mut worklist = Vec::new();
        self.push_children(body, &mut worklist);
        while let Some(child) = worklist.pop() {
            if self.kind_of(child) == SyntaxKind::Identifier {
                let child_symbol = self.get_symbol_at_location_for_condition_walker(child)?;
                if child_symbol == Some(tested_symbol) {
                    if self.kind_of(expr) == SyntaxKind::Identifier
                        || (self.kind_of(tested_node) == SyntaxKind::Identifier
                            && self
                                .parent_of(tested_node)
                                .is_some_and(|p| self.kind_of(p) == SyntaxKind::BinaryExpression))
                    {
                        return Ok(true);
                    }
                    // Walk the access chains outward comparing
                    // symbols level by level.
                    let mut tested_expression = self.parent_of(tested_node);
                    let mut child_expression = self.parent_of(child);
                    while let (Some(tested), Some(child_expr)) =
                        (tested_expression, child_expression)
                    {
                        let tested_kind = self.kind_of(tested);
                        let child_kind = self.kind_of(child_expr);
                        if (tested_kind == SyntaxKind::Identifier
                            && child_kind == SyntaxKind::Identifier)
                            || (tested_kind == SyntaxKind::ThisKeyword
                                && child_kind == SyntaxKind::ThisKeyword)
                        {
                            let tested_symbol_here =
                                self.get_symbol_at_location_for_condition_walker(tested)?;
                            let child_symbol_here =
                                self.get_symbol_at_location_for_condition_walker(child_expr)?;
                            if tested_symbol_here.is_some()
                                && tested_symbol_here == child_symbol_here
                            {
                                return Ok(true);
                            }
                            break;
                        } else if tested_kind == SyntaxKind::PropertyAccessExpression
                            && child_kind == SyntaxKind::PropertyAccessExpression
                        {
                            let (tested_name, tested_receiver) = match self.data_of(tested) {
                                NodeData::PropertyAccessExpression(data) => {
                                    (data.name, data.expression)
                                }
                                _ => (None, None),
                            };
                            let (child_name, child_receiver) = match self.data_of(child_expr) {
                                NodeData::PropertyAccessExpression(data) => {
                                    (data.name, data.expression)
                                }
                                _ => (None, None),
                            };
                            let (Some(tested_name), Some(child_name)) = (tested_name, child_name)
                            else {
                                break;
                            };
                            let tested_name_symbol =
                                self.get_symbol_at_location_for_condition_walker(tested_name)?;
                            let child_name_symbol =
                                self.get_symbol_at_location_for_condition_walker(child_name)?;
                            if tested_name_symbol != child_name_symbol {
                                break;
                            }
                            child_expression = child_receiver;
                            tested_expression = tested_receiver;
                            if child_expression.is_none() || tested_expression.is_none() {
                                break;
                            }
                        } else if tested_kind == SyntaxKind::CallExpression
                            && child_kind == SyntaxKind::CallExpression
                        {
                            child_expression = match self.data_of(child_expr) {
                                NodeData::CallExpression(data) => data.expression,
                                _ => None,
                            };
                            tested_expression = match self.data_of(tested) {
                                NodeData::CallExpression(data) => data.expression,
                                _ => None,
                            };
                            if child_expression.is_none() || tested_expression.is_none() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
            }
            self.push_children(child, &mut worklist);
        }
        Ok(false)
    }

    /// tsc-port: isSymbolUsedInBinaryExpressionChain @6.0.3
    /// tsc-hash: ab03de0ed8f8d8f701e3b6d9e6978c943a987435e8603f1fe54728c4df46f024
    /// tsc-span: _tsc.js:83722-83739
    fn is_symbol_used_in_binary_expression_chain(
        &mut self,
        node: NodeId,
        tested_symbol: SymbolId,
    ) -> CheckResult<bool> {
        let mut current = Some(node);
        while let Some(node) = current {
            if self.kind_of(node) != SyntaxKind::BinaryExpression {
                break;
            }
            let Some((_, operator_token, right)) = self.binary_parts(node) else {
                break;
            };
            if self.operator_kind(operator_token) != SyntaxKind::AmpersandAmpersandToken {
                break;
            }
            // forEachChild(node.right, visit): the CHILDREN of the
            // right operand — a bare identifier right has none, so
            // `ff && ff` still reports (oracle-pinned).
            let mut worklist = Vec::new();
            self.push_children(right, &mut worklist);
            while let Some(child) = worklist.pop() {
                if self.kind_of(child) == SyntaxKind::Identifier {
                    let symbol = self.get_symbol_at_location_for_condition_walker(child)?;
                    if symbol == Some(tested_symbol) {
                        return Ok(true);
                    }
                }
                self.push_children(child, &mut worklist);
            }
            current = self.parent_of(node);
        }
        Ok(false)
    }

    /// forEachChild for the walkers: pushes every child of `node`.
    fn push_children(&self, node: NodeId, worklist: &mut Vec<NodeId>) {
        let source = self.binder.source_of_node(node);
        let raw = source.arena.node(node);
        tsc_syntax::for_each_child(&source.arena, raw, |child| {
            worklist.push(child);
            false
        });
    }

    /// getSymbolAtLocation (87531), the slice the condition walkers
    /// reach: declaration names return the declaration's symbol;
    /// identifiers lifted through their access chains resolve quietly
    /// (ignoreErrors) as values; property accesses use the resolved
    /// links, force-checking once when unset — exactly tsc's
    /// getSymbolOfNameOrPropertyAccessExpression (87317) forcing.
    /// Deviations (documented): the alias-symbol dontResolveAlias
    /// nuance collapses (both comparison sides resolve consistently,
    /// verdicts match); the getApplicableIndexSymbol tail is absent
    /// (index-signature symbol identity — no constructor for it yet),
    /// so index-signature conditions stay None and the caller's
    /// `!testedSymbol` early-return suppresses (FN-safe).
    fn get_symbol_at_location_for_condition_walker(
        &mut self,
        node: NodeId,
    ) -> CheckResult<Option<SymbolId>> {
        if self.is_declaration_name_for_walker(node) {
            let parent = self.parent_of(node).expect("declaration name has a parent");
            return Ok(self.node_symbol(parent).map(|s| self.get_merged_symbol(s)));
        }
        // while isRightSideOfQualifiedNameOrPropertyAccess (87357).
        let mut name = node;
        while let Some(parent) = self.parent_of(name) {
            let lifted = match self.data_of(parent) {
                NodeData::QualifiedName(data) => data.right == Some(name),
                NodeData::PropertyAccessExpression(data) => data.name == Some(name),
                _ => false,
            };
            if !lifted {
                break;
            }
            name = parent;
        }
        match self.kind_of(name) {
            SyntaxKind::Identifier => {
                if !self.is_expression_node_for_walker(name) {
                    return Ok(None);
                }
                let text = match self.identifier_text_of(name) {
                    Some(text) => text.to_owned(),
                    None => return Ok(None),
                };
                self.resolve_name(
                    Some(name),
                    &text,
                    SymbolFlags::VALUE | SymbolFlags::EXPORT_VALUE,
                    None,
                    true,
                    false,
                )
            }
            SyntaxKind::PropertyAccessExpression | SyntaxKind::QualifiedName => {
                if let Some(cached) = self.links.node(name).resolved_symbol.resolved() {
                    return Ok((cached != self.unknown_symbol).then_some(cached));
                }
                if self.kind_of(name) == SyntaxKind::PropertyAccessExpression {
                    self.check_property_access_expression(name, CheckMode::NORMAL, false)?;
                } else {
                    self.check_expression(name, CheckMode::NORMAL)?;
                }
                let resolved = self.links.node(name).resolved_symbol.resolved();
                Ok(resolved.filter(|&s| s != self.unknown_symbol))
            }
            SyntaxKind::ThisKeyword => {
                // 87574-87589: the function-like container's explicit
                // this-parameter symbol wins; otherwise the
                // isInExpressionContext arm reads
                // checkExpression(this).symbol — realized quietly as
                // tryGetThisTypeAt's type symbol (the walker's chains
                // only reach expression-position `this`, where
                // checkThisExpression reduces to tryGetThisTypeAt; its
                // error band cannot fire on a receiver whose property
                // resolved). The ThisType fallthrough is type-position
                // only — unreachable here.
                let Some(container) =
                    crate::expr::get_this_container_full(self, name, false, false)
                else {
                    return Ok(None);
                };
                if node_util::is_function_like_kind(self.kind_of(container)) {
                    let signature = self.get_signature_from_declaration(container)?;
                    if let Some(this_parameter) = self.signature_of(signature).this_parameter {
                        return Ok(Some(this_parameter));
                    }
                }
                let ty = self.try_get_this_type_at(name, true, container)?;
                Ok(ty.and_then(|ty| self.tables.type_of(ty).symbol))
            }
            _ => Ok(None),
        }
    }

    /// isDeclarationName (15679): parent is a declaration whose `name`
    /// slot IS this node. The walker only needs the named-declaration
    /// kinds a function/if body can contain.
    fn is_declaration_name_for_walker(&self, node: NodeId) -> bool {
        let Some(parent) = self.parent_of(node) else {
            return false;
        };
        let name = match self.data_of(parent) {
            NodeData::VariableDeclaration(data) => data.name,
            NodeData::FunctionDeclaration(data) => data.name,
            NodeData::ClassDeclaration(data) => data.name,
            NodeData::ClassExpression(data) => data.name,
            NodeData::FunctionExpression(data) => data.name,
            NodeData::InterfaceDeclaration(data) => data.name,
            NodeData::TypeAliasDeclaration(data) => data.name,
            NodeData::EnumDeclaration(data) => data.name,
            NodeData::EnumMember(data) => data.name,
            NodeData::ModuleDeclaration(data) => data.name,
            NodeData::Parameter(data) => data.name,
            NodeData::BindingElement(data) => data.name,
            NodeData::PropertyDeclaration(data) => data.name,
            NodeData::PropertyAssignment(data) => data.name,
            NodeData::ShorthandPropertyAssignment(data) => data.name,
            NodeData::MethodDeclaration(data) => data.name,
            NodeData::GetAccessor(data) => data.name,
            NodeData::SetAccessor(data) => data.name,
            NodeData::PropertySignature(data) => data.name,
            NodeData::MethodSignature(data) => data.name,
            _ => None,
        };
        name == Some(node)
    }

    /// isExpressionNode (12279 subset): the walker only distinguishes
    /// expression identifiers from type/declaration positions; a
    /// conservative parent test — identifiers under type nodes resolve
    /// as values in tsc's walker too (quietly, via the Value meaning),
    /// so mis-classifying toward "expression" only re-runs the same
    /// quiet resolution.
    fn is_expression_node_for_walker(&self, _node: NodeId) -> bool {
        true
    }

    // ---- the await PROBE family (getAwaitedType, errorNode-less) ----
    //
    // The await band proper (checkAwaitExpression + its grammar) is
    // 5.5f; these type-level walkers are pulled forward because the
    // operator band's error paths probe them on EVERY operator error
    // (reportOperatorError's wouldWorkWithAwait, the arithmetic 2362/
    // 2363 + 2773-related shaping, the truthy-callable 2801 promise
    // arm). errorNode is threaded as an Option so 5.5f lifts in place;
    // every 5.5e call site passes None.

    /// tsc-port: getAwaitedTypeOfPromise @6.0.3
    /// tsc-hash: 774f80c22975e7f9446ebcd3d7fbb53d2dbc633d52b24159db79d9d2d73a6cc2
    /// tsc-span: _tsc.js:82312-82315
    pub(crate) fn get_awaited_type_of_promise(
        &mut self,
        ty: TypeId,
    ) -> CheckResult<Option<TypeId>> {
        let promised = self.get_promised_type_of_promise(ty)?;
        match promised {
            Some(promised) => self.get_awaited_type_probe(promised),
            None => Ok(None),
        }
    }

    /// tsc-port: getAwaitedType @6.0.3
    /// tsc-hash: d8eeb1013e9cbe31e08ea52051232be9c5a3e8d5256dcca7824e9aee88fdaf9a
    /// tsc-span: _tsc.js:82431-82434
    pub(crate) fn get_awaited_type_probe(&mut self, ty: TypeId) -> CheckResult<Option<TypeId>> {
        let awaited = self.get_awaited_type_no_alias(ty, None)?;
        match awaited {
            Some(awaited) => Ok(Some(self.create_awaited_type_if_needed(awaited)?)),
            None => Ok(None),
        }
    }

    /// tsc-port: getAwaitedTypeNoAlias @6.0.3
    /// tsc-hash: bd45efe2c145824e7c6806099aee1e2eb7286d058912356c61200b090e432a2f
    /// tsc-span: _tsc.js:82435-82497
    ///
    /// Error paths live since 5.5f: the circularity arms report 1062
    /// at the caller's errorNode; the thenable tail reports the
    /// caller's head message (1320/1058-family). When
    /// getPromisedTypeOfPromise rejected every `then` signature on
    /// its explicit-this relation, tsc appends the 2684 this-context
    /// row under that head in one message chain.
    pub(crate) fn get_awaited_type_no_alias(
        &mut self,
        ty: TypeId,
        error_info: Option<(NodeId, &'static tsc_diagnostics::DiagnosticMessage)>,
    ) -> CheckResult<Option<TypeId>> {
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY) {
            return Ok(Some(ty));
        }
        if self.is_awaited_type_instantiation(ty)? {
            return Ok(Some(ty));
        }
        if let Some(cached) = self.links.ty(ty).awaited_type_of_type {
            return Ok(Some(cached));
        }
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            if self.awaited_type_stack.contains(&ty) {
                if let Some((error_node, _)) = error_info {
                    self.error_at(
                        Some(error_node),
                        &tsc_diagnostics::gen::Type_is_referenced_directly_or_indirectly_in_the_fulfillment_callback_of_its_own_then_method,
                        &[],
                    );
                }
                return Ok(None);
            }
            self.awaited_type_stack.push(ty);
            let mapped = self.map_type(
                ty,
                &mut |state, t| state.get_awaited_type_no_alias(t, error_info),
                false,
            );
            self.awaited_type_stack.pop();
            let mapped = mapped?;
            if let Some(mapped) = mapped {
                self.links
                    .set_type_awaited_type_of_type(self.speculation_depth, ty, mapped);
            }
            return Ok(mapped);
        }
        if self.is_awaited_type_needed(ty)? {
            self.links
                .set_type_awaited_type_of_type(self.speculation_depth, ty, ty);
            return Ok(Some(ty));
        }
        let (promised, this_type_for_error) =
            self.get_promised_type_of_promise_with_this_error(ty, None)?;
        if let Some(promised) = promised {
            if ty == promised || self.awaited_type_stack.contains(&promised) {
                if let Some((error_node, _)) = error_info {
                    self.error_at(
                        Some(error_node),
                        &tsc_diagnostics::gen::Type_is_referenced_directly_or_indirectly_in_the_fulfillment_callback_of_its_own_then_method,
                        &[],
                    );
                }
                return Ok(None);
            }
            self.awaited_type_stack.push(ty);
            let awaited = self.get_awaited_type_no_alias(promised, error_info);
            self.awaited_type_stack.pop();
            let awaited = awaited?;
            let Some(awaited) = awaited else {
                return Ok(None);
            };
            self.links
                .set_type_awaited_type_of_type(self.speculation_depth, ty, awaited);
            return Ok(Some(awaited));
        }
        if self.is_thenable_type(ty)? {
            if let Some((error_node, message)) = error_info {
                // getAwaitedTypeNoAlias 82485-82490:
                // chainDiagnosticMessages first creates the optional
                // 2684 detail, then prepends the caller's head.
                let detail = match this_type_for_error {
                    Some(this_type) => {
                        let type_text = self.type_to_string_slice(ty)?;
                        let this_text = self.type_to_string_slice(this_type)?;
                        vec![tsc_diagnostics::MessageChain::new(
                            &tsc_diagnostics::gen::The_this_context_of_type_0_is_not_assignable_to_method_s_this_of_type_1,
                            &[type_text, this_text],
                        )]
                    }
                    None => Vec::new(),
                };
                let chain = tsc_diagnostics::MessageChain::new(message, &[]).with_next(detail);
                let span = self.diag_span_of_node(error_node);
                let diagnostic = self.diagnostic_at_span(&span, chain);
                self.push_error_diagnostic(diagnostic);
            }
            return Ok(None);
        }
        self.links
            .set_type_awaited_type_of_type(self.speculation_depth, ty, ty);
        Ok(Some(ty))
    }

    /// tsc-port: isThenableType @6.0.3
    /// tsc-hash: 7033cc1e546eea275b2217b9a8c036f86d9bddb043b74b0415337d9acaddc9b7
    /// tsc-span: _tsc.js:82381-82388
    fn is_thenable_type(&mut self, ty: TypeId) -> CheckResult<bool> {
        let base = self.get_base_constraint_or_type(ty)?;
        if self.all_types_assignable_to_kind(
            base,
            TypeFlags::from_bits(TypeFlags::PRIMITIVE.bits() | TypeFlags::NEVER.bits()),
        )? {
            return Ok(false);
        }
        let then_function = self.get_type_of_property_of_type(ty, "then")?;
        let Some(then_function) = then_function else {
            return Ok(false);
        };
        let non_nullable =
            self.get_type_with_facts(then_function, TypeFacts::NE_UNDEFINED_OR_NULL)?;
        Ok(!self
            .get_signatures_of_type(non_nullable, SignatureKind::Call)?
            .is_empty())
    }

    /// tsc-port: isAwaitedTypeInstantiation @6.0.3
    /// tsc-hash: f22c1e99c236e2e1a59b280c5c9f89b036d4f2c6738c10fd0e2b15ceb80b15b0
    /// tsc-span: _tsc.js:82389-82398
    fn is_awaited_type_instantiation(&mut self, ty: TypeId) -> CheckResult<bool> {
        if !self.tables.flags_of(ty).intersects(TypeFlags::CONDITIONAL) {
            return Ok(false);
        }
        let awaited_symbol = self.get_global_awaited_symbol(false)?;
        let type_record = self.tables.type_of(ty);
        Ok(awaited_symbol.is_some()
            && type_record.alias_symbol == awaited_symbol
            && type_record
                .alias_type_arguments
                .as_ref()
                .is_some_and(|args| args.len() == 1))
    }

    /// tsc-port: unwrapAwaitedType @6.0.3
    /// tsc-hash: fa90c22f836ab976b33c8e42530722f711f185e19b9d01f759c86b64009a0ab6
    /// tsc-span: _tsc.js:82399-82401
    pub(crate) fn unwrap_awaited_type(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        if self.tables.flags_of(ty).intersects(TypeFlags::UNION) {
            let unwrapped = self.map_type(
                ty,
                &mut |state, t| state.unwrap_awaited_type(t).map(Some),
                false,
            )?;
            return Ok(unwrapped.expect("mapper never returns None"));
        }
        if self.is_awaited_type_instantiation(ty)? {
            let args = self
                .tables
                .type_of(ty)
                .alias_type_arguments
                .as_ref()
                .expect("isAwaitedTypeInstantiation checked the arguments");
            return Ok(args[0]);
        }
        Ok(ty)
    }

    /// tsc-port: isAwaitedTypeNeeded @6.0.3
    /// tsc-hash: 1f0d4ad0cfdddfcebe1865ffccfd2b0e1cd9e6fd4be0b2b82428c18fcef429f2
    /// tsc-span: _tsc.js:82402-82414
    fn is_awaited_type_needed(&mut self, ty: TypeId) -> CheckResult<bool> {
        if self.tables.flags_of(ty).intersects(TypeFlags::ANY)
            || self.is_awaited_type_instantiation(ty)?
        {
            return Ok(false);
        }
        if self.is_generic_object_type_state(ty)? {
            let base_constraint = self.get_base_constraint_of_type(ty)?;
            match base_constraint {
                Some(base) => {
                    if self
                        .tables
                        .flags_of(base)
                        .intersects(TypeFlags::ANY_OR_UNKNOWN)
                        || self.is_empty_object_type(base)?
                        || self.some_type_result(base, |state, t| state.is_thenable_type(t))?
                    {
                        return Ok(true);
                    }
                }
                None => {
                    if self.maybe_type_of_kind(ty, TypeFlags::TYPE_VARIABLE) {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    /// tsc-port: tryCreateAwaitedType @6.0.3
    /// tsc-hash: b1665dc715c2a81a96b4d19b9c3069ce59aa3fe0ef05cc554dbcaf9b606172ee
    /// tsc-span: _tsc.js:82415-82422
    fn try_create_awaited_type(&mut self, ty: TypeId) -> CheckResult<Option<TypeId>> {
        let awaited_symbol = self.get_global_awaited_symbol(true)?;
        if let Some(alias) = awaited_symbol {
            let unwrapped = self.unwrap_awaited_type(ty)?;
            return self
                .get_type_alias_instantiation(alias, Some(&[unwrapped]), None, None)
                .map(Some);
        }
        Ok(None)
    }

    /// tsc-port: createAwaitedTypeIfNeeded @6.0.3
    /// tsc-hash: 977cff8639a10834be336a33f914662ce5ca40826185d9460051489e3ec4cc33
    /// tsc-span: _tsc.js:82423-82430
    fn create_awaited_type_if_needed(&mut self, ty: TypeId) -> CheckResult<TypeId> {
        if self.is_awaited_type_needed(ty)? {
            if let Some(awaited) = self.try_create_awaited_type(ty)? {
                return Ok(awaited);
            }
        }
        Ok(ty)
    }

    /// tsc-port: getAwaitedType @6.0.3
    /// tsc-hash: d8eeb1013e9cbe31e08ea52051232be9c5a3e8d5256dcca7824e9aee88fdaf9a
    /// tsc-span: _tsc.js:82431-82434
    pub(crate) fn get_awaited_type_with_error(
        &mut self,
        ty: TypeId,
        error_info: Option<(NodeId, &'static tsc_diagnostics::DiagnosticMessage)>,
    ) -> CheckResult<Option<TypeId>> {
        let awaited = self.get_awaited_type_no_alias(ty, error_info)?;
        match awaited {
            Some(awaited) => Ok(Some(self.create_awaited_type_if_needed(awaited)?)),
            None => Ok(None),
        }
    }

    /// tsc-port: checkAwaitedType @6.0.3
    /// tsc-hash: 8963cbea36a471c1085379489f08bf1e81995dc88bdaac2bcfbf350bf630b88d
    /// tsc-span: _tsc.js:82377-82380
    pub(crate) fn check_awaited_type(
        &mut self,
        ty: TypeId,
        with_alias: bool,
        error_node: NodeId,
        message: &'static tsc_diagnostics::DiagnosticMessage,
    ) -> CheckResult<TypeId> {
        let awaited = if with_alias {
            self.get_awaited_type_with_error(ty, Some((error_node, message)))?
        } else {
            self.get_awaited_type_no_alias(ty, Some((error_node, message)))?
        };
        Ok(awaited.unwrap_or(self.tables.intrinsics.error))
    }

    /// tsc-port: getGlobalAwaitedSymbol @6.0.3
    /// tsc-hash: df4954ba20473c54fe47bb63db8cac683e38200a39002fabc594ece4dab81c7b
    /// tsc-span: _tsc.js:60927-60935
    ///
    /// getGlobalTypeAliasSymbol's arity probe (60936-60950) rides
    /// along: an Awaited alias with the wrong arity errors on ITS
    /// declaration (2317) — only constructible with a user-shadowed
    /// lib, so the arm escapes rather than half-rendering.
    pub(crate) fn get_global_awaited_symbol(
        &mut self,
        report_errors: bool,
    ) -> CheckResult<Option<SymbolId>> {
        if let Some(memo) = self.deferred_global_awaited_symbol {
            return Ok(memo.filter(|&s| s != self.unknown_symbol));
        }
        // Miss/wrong-arity memoize unknownSymbol only under
        // reportErrors (`|| (reportErrors2 ? unknownSymbol : void 0)`)
        // — the silent flavor retries per call.
        let resolved = self.get_global_type_alias_symbol("Awaited", 1, report_errors)?;
        match resolved {
            Some(symbol) => {
                self.deferred_global_awaited_symbol = Some(Some(symbol));
                Ok(Some(symbol))
            }
            None => {
                if report_errors {
                    let unknown = self.unknown_symbol;
                    self.deferred_global_awaited_symbol = Some(Some(unknown));
                }
                Ok(None)
            }
        }
    }

    // ---- operator-error display + await-hint plumbing ----

    /// tsc-port: maybeTypeOfKindConsideringBaseConstraint @6.0.3
    /// tsc-hash: ccf1aee732579a0f923c8d015f4c6723afeb2840bc92161e2305045257985500
    /// tsc-span: _tsc.js:79509-79515
    pub(crate) fn maybe_type_of_kind_considering_base_constraint(
        &mut self,
        ty: TypeId,
        kind: TypeFlags,
    ) -> CheckResult<bool> {
        if self.maybe_type_of_kind(ty, kind) {
            return Ok(true);
        }
        let base_constraint = self.get_base_constraint_or_type(ty)?;
        Ok(self.maybe_type_of_kind(base_constraint, kind))
    }

    /// tsc-port: errorAndMaybeSuggestAwait @6.0.3
    /// tsc-hash: 3d46b5e2d0f18a3f11f54b24c8e72153ecce9a18a04b43163bba0d3740e0c688
    /// tsc-span: _tsc.js:75323-75330
    pub(crate) fn error_and_maybe_suggest_await(
        &mut self,
        location: NodeId,
        maybe_missing_await: bool,
        message: &'static tsc_diagnostics::DiagnosticMessage,
        args: &[&str],
    ) -> usize {
        if maybe_missing_await {
            let related = self.related_info_for_node(
                location,
                &tsc_diagnostics::gen::Did_you_forget_to_use_await,
                &[],
            );
            self.error_at_with_related(Some(location), message, args, vec![related])
        } else {
            self.error_at(Some(location), message, args)
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/operators/tests.rs"]
mod tests;
