//! Operator checking: unary / update / binary operators, the `+` overload,
//! comparison & comparability, and truthiness (`testable` / `falsy_part` /
//! `truthy_part`). Split out of `exprs.rs`.

use crate::ast::*;
use crate::binder::flags;
use crate::checker::exprs::expr_contains_optional_chain;
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TruthinessContext {
    Condition,
    LoopCondition,
    LogicalAndLeft,
    LogicalOrLeft,
    LogicalNotOperand,
}

impl TruthinessContext {
    fn allows_function_condition(self) -> bool {
        matches!(
            self,
            TruthinessContext::Condition | TruthinessContext::LogicalAndLeft
        )
    }

    fn allows_promise_condition(self) -> bool {
        matches!(
            self,
            TruthinessContext::Condition
                | TruthinessContext::LoopCondition
                | TruthinessContext::LogicalAndLeft
        )
    }
}

impl<'a> Checker<'a> {
    pub(crate) fn check_unary(&mut self, op: UnaryOp, operand: &'a Expr) -> TypeId {
        let t = self.check_expr(operand, None);
        match op {
            UnaryOp::Delete => {
                match operand {
                    Expr::PropAccess { obj, name, .. } => {
                        if name.name.starts_with('#') {
                            self.error_at(
                                operand.span(),
                                &gen::The_operand_of_a_delete_operator_cannot_be_a_private_identifier,
                                &[],
                            );
                        }
                        let obj_t = self
                            .caches
                            .expr_type_cache
                            .get(&(obj.as_ref() as *const Expr as usize))
                            .copied()
                            .unwrap_or_else(|| self.check_expr(obj, None));
                        if !self.types.is_any_or_error(obj_t) {
                            if let Some(p) = self.prop_info_of_type(obj_t, &name.name) {
                                if p.readonly {
                                    self.error_at(
                                        operand.span(),
                                        &gen::The_operand_of_a_delete_operator_cannot_be_a_read_only_property,
                                        &[],
                                    );
                                } else if !p.optional && self.options.strict_null_checks() {
                                    self.error_at(
                                        operand.span(),
                                        &gen::The_operand_of_a_delete_operator_must_be_optional,
                                        &[],
                                    );
                                }
                            }
                        }
                    }
                    Expr::ElemAccess { .. } => {}
                    _ => {
                        // strict mode (alwaysStrict via --strict)
                        if self.options.strict.unwrap_or(false) && matches!(operand, Expr::Ident(_))
                        {
                            self.error_at(
                                operand.span(),
                                &gen::delete_cannot_be_called_on_an_identifier_in_strict_mode,
                                &[],
                            );
                        }
                        self.error_at(
                            operand.span(),
                            &gen::The_operand_of_a_delete_operator_must_be_a_property_reference,
                            &[],
                        );
                    }
                }
                return self.types.boolean;
            }
            UnaryOp::Typeof => {
                let lits = [
                    "string",
                    "number",
                    "bigint",
                    "boolean",
                    "symbol",
                    "undefined",
                    "object",
                    "function",
                ];
                let members: Vec<TypeId> = lits.iter().map(|l| self.types.string_lit(l)).collect();
                self.types.union(members)
            }
            UnaryOp::Void => self.types.undefined,
            UnaryOp::Bang => {
                self.check_testable(operand, t, TruthinessContext::LogicalNotOperand);
                self.types.boolean
            }
            UnaryOp::Minus | UnaryOp::Plus | UnaryOp::Tilde => {
                let reg = self.types.regular(t);
                if matches!(self.types.kind(reg), TypeKind::EsSymbol) {
                    self.error_at(
                        operand.span(),
                        &gen::The_0_operator_cannot_be_applied_to_type_symbol,
                        &[unary_operator_text(op).to_string()],
                    );
                    return self.types.number;
                }
                if self.report_direct_unusable_value(operand) {
                    return self.types.number;
                }
                // tsc checkNonNullType on the operand: strict rejects any
                // nullish member; non-strict rejects only an EXACTLY nullish
                // operand (`let x; -x` reads flow type `undefined` → 18048)
                let t = self.check_operand_non_nullish(t, operand);
                if matches!(
                    self.types.kind(t),
                    TypeKind::Bigint | TypeKind::BigIntLit(_)
                ) {
                    self.types.bigint
                } else if op == UnaryOp::Minus {
                    // negative literal type for numeric literals
                    let r = self.types.regular(t);
                    if let TypeKind::NumLit(bits) = self.types.kind(r) {
                        let v = -f64::from_bits(*bits);
                        let lit = self.types.number_lit(v);
                        return self.types.fresh(lit);
                    }
                    self.types.number
                } else {
                    self.types.number
                }
            }
        }
    }

    pub(crate) fn check_update(&mut self, operand: &'a Expr, _span: Span) -> TypeId {
        let t = self.check_expr(operand, None);
        // assignment-target checks first (matches tsc order: 2588 before 2356)
        let is_ref_like = matches!(
            operand,
            Expr::Ident(_) | Expr::PropAccess { .. } | Expr::ElemAccess { .. } | Expr::Paren { .. }
        );
        if !is_ref_like {
            self.error_at(
                operand.span(),
                &gen::The_operand_of_an_increment_or_decrement_operator_must_be_a_variable_or_a_property_access,
                &[],
            );
            if matches!(operand, Expr::NullLit { .. }) {
                self.report_direct_unusable_value(operand);
            }
            return self.types.number;
        }
        if !self.check_reference_for_assignment(operand, true) {
            return self.types.number;
        }
        let ok = self.is_arithmetic_operand(t);
        if !ok {
            self.error_at(
                operand.span(),
                &gen::An_arithmetic_operand_must_be_of_type_any_number_bigint_or_an_enum_type,
                &[],
            );
        }
        self.types.number
    }

    fn contains_nullish_member(&mut self, t: TypeId) -> bool {
        self.types
            .union_members(t)
            .iter()
            .any(|&m| matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined))
    }

    fn is_arithmetic_operand(&mut self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::EnumType(_) | TypeKind::EnumMember(_) => {
                let (n, s) = self.enum_member_kinds_of(t);
                n && !s
            }
            TypeKind::Any
            | TypeKind::Error
            | TypeKind::Number
            | TypeKind::NumLit(_)
            | TypeKind::Bigint
            | TypeKind::BigIntLit(_) => true,
            TypeKind::Union(ms) => {
                let ms = ms.clone();
                ms.iter().all(|&m| self.is_arithmetic_operand(m))
            }
            TypeKind::Intersection(ms) => {
                // a numeric operand survives the intersection (`number & Brand`).
                let ms = ms.clone();
                ms.iter().any(|&m| self.is_arithmetic_operand(m))
            }
            _ => false,
        }
    }

    fn direct_unusable_value_name(&self, expr: &Expr) -> Option<&'static str> {
        match expr {
            Expr::NullLit { .. } => Some("null"),
            Expr::Ident(id)
                if id.name == "undefined"
                    && self.lookup_value(self.current_scope, "undefined").is_none() =>
            {
                Some("undefined")
            }
            _ => None,
        }
    }

    fn report_direct_unusable_value(&mut self, expr: &'a Expr) -> bool {
        let Some(value) = self.direct_unusable_value_name(expr) else {
            return false;
        };
        self.error_at(
            expr.span(),
            &gen::The_value_0_cannot_be_used_here,
            &[value.to_string()],
        );
        true
    }

    fn plus_string_like(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::String | TypeKind::StrLit(_) => true,
            TypeKind::Intersection(ms) => ms.iter().any(|&m| self.plus_string_like(m)),
            _ => false,
        }
    }

    fn plus_any_like(&self, t: TypeId) -> bool {
        matches!(self.types.kind(t), TypeKind::Any | TypeKind::Error)
    }

    /// returns false when the target isn't assignable (errors already emitted)
    fn check_reference_for_assignment(&mut self, target: &'a Expr, _for_update: bool) -> bool {
        match target {
            Expr::Ident(id) => {
                if id.name == "undefined" {
                    self.error_at(
                        id.span,
                        &gen::Cannot_assign_to_0_because_it_is_not_a_variable,
                        &[id.name.clone()],
                    );
                    return false;
                }
                if (id.name == "eval" || id.name == "arguments")
                    && self.options.strict.unwrap_or(false)
                {
                    self.error_at(
                        id.span,
                        &gen::Invalid_use_of_0_in_strict_mode,
                        &[id.name.clone()],
                    );
                }
                if let Some(sym) = self.lookup_value(self.current_scope, &id.name) {
                    let s = self.symbol(sym);
                    let f = s.flags;
                    if f & flags::CONST_VARIABLE != 0 {
                        self.error_at(
                            id.span,
                            &gen::Cannot_assign_to_0_because_it_is_a_constant,
                            &[id.name.clone()],
                        );
                        return false;
                    }
                    if f & flags::ALIAS != 0 {
                        self.error_at(
                            id.span,
                            &gen::Cannot_assign_to_0_because_it_is_an_import,
                            &[id.name.clone()],
                        );
                        return false;
                    }
                    if f & flags::CLASS != 0 {
                        self.error_at(
                            id.span,
                            &gen::Cannot_assign_to_0_because_it_is_a_class,
                            &[id.name.clone()],
                        );
                        return false;
                    }
                    if f & flags::ENUM != 0 {
                        self.error_at(
                            id.span,
                            &gen::Cannot_assign_to_0_because_it_is_an_enum,
                            &[id.name.clone()],
                        );
                        return false;
                    }
                    if f & flags::NAMESPACE != 0 {
                        self.error_at(
                            id.span,
                            &gen::Cannot_assign_to_0_because_it_is_a_namespace,
                            &[id.name.clone()],
                        );
                        return false;
                    }
                    if f & flags::FUNCTION != 0 {
                        self.error_at(
                            id.span,
                            &gen::Cannot_assign_to_0_because_it_is_a_function,
                            &[id.name.clone()],
                        );
                        return false;
                    }
                    self.symuse.assigned_symbols.insert(sym);
                }
                true
            }
            Expr::PropAccess { obj, name, .. } => {
                let obj_t = self.check_expr(obj, None);
                if let Some(pinfo) = self.prop_info_of_type(obj_t, &name.name) {
                    if pinfo.readonly {
                        self.error_at(
                            name.span,
                            &gen::Cannot_assign_to_0_because_it_is_a_read_only_property,
                            &[name.name.clone()],
                        );
                        // A read-only target is reported as TS2540 only; tsc does
                        // not additionally check the assigned value's type, so the
                        // caller must not run the assignability check.
                        return false;
                    }
                }
                true
            }
            Expr::ElemAccess { obj, index, .. } => {
                let obj_t = self.check_expr(obj, None);
                let idx_t = self.check_expr(index, None);
                let idx_r = self.types.regular(idx_t);
                // readonly tuple / readonly array elements cannot be assigned
                match self.types.kind(obj_t) {
                    TypeKind::ReadonlyTuple(_) => {
                        if let TypeKind::NumLit(bits) = self.types.kind(idx_r) {
                            let nm = crate::js_num::to_js_string(f64::from_bits(*bits));
                            self.error_at(
                                index.span(),
                                &gen::Cannot_assign_to_0_because_it_is_a_read_only_property,
                                &[nm],
                            );
                            return false;
                        }
                    }
                    TypeKind::ReadonlyArray(_) => {
                        let d = self.display_type(obj_t);
                        self.error_at(
                            target.span(),
                            &gen::Index_signature_in_type_0_only_permits_reading,
                            &[d],
                        );
                        return false;
                    }
                    _ => {}
                }
                // a named property wins over index signatures
                let prop_name = match self.types.kind(idx_r) {
                    TypeKind::StrLit(s) => Some(s.to_str_lossy().into_owned()),
                    _ => None,
                };
                let has_prop = prop_name
                    .as_ref()
                    .map(|n| self.prop_info_of_type(obj_t, n).is_some())
                    .unwrap_or(false);
                if !has_prop && !self.types.is_any_or_error(obj_t) {
                    let ap = self.apparent_type(obj_t);
                    if let Some(sid) = self.shape_of_type(ap) {
                        let infos = self.types.shape(sid).index_infos.clone();
                        if !infos.is_empty() && infos.iter().all(|i| i.readonly) {
                            let d = self.display_type(obj_t);
                            self.error_at(
                                target.span(),
                                &gen::Index_signature_in_type_0_only_permits_reading,
                                &[d],
                            );
                            return false;
                        }
                    }
                }
                true
            }
            Expr::Paren { inner, .. } => self.check_reference_for_assignment(inner, _for_update),
            // destructuring-assignment targets: `[a, b] = …`, `({x} = …)`. Each
            // element/property is itself an assignment target, so recurse. This
            // also marks the targets assigned (so they are not reported as
            // read-before-assignment or unused). Element-wise assignability of
            // the right-hand side is not yet checked.
            Expr::Array { elements, .. } => {
                let mut ok = true;
                for el in elements {
                    let mut tgt = el;
                    if let Expr::Binary {
                        op: BinOp::Assign,
                        left,
                        ..
                    } = tgt
                    {
                        tgt = left;
                    }
                    if let Expr::Spread { expr, .. } = tgt {
                        tgt = expr;
                    }
                    if matches!(tgt, Expr::Missing { .. }) {
                        continue;
                    }
                    ok &= self.check_reference_for_assignment(tgt, _for_update);
                }
                ok
            }
            Expr::Object { props, .. } => {
                let mut ok = true;
                for p in props {
                    match p {
                        ObjectProp::Shorthand { name, .. } => {
                            if let Some(sym) = self.lookup_value(self.current_scope, &name.name) {
                                self.symuse.assigned_symbols.insert(sym);
                            }
                        }
                        ObjectProp::Property { value, .. } => {
                            let mut tgt = value;
                            if let Expr::Binary {
                                op: BinOp::Assign,
                                left,
                                ..
                            } = tgt
                            {
                                tgt = left;
                            }
                            ok &= self.check_reference_for_assignment(tgt, _for_update);
                        }
                        ObjectProp::Spread { expr, .. } => {
                            ok &= self.check_reference_for_assignment(expr, _for_update);
                        }
                        ObjectProp::Method(m) => {
                            self.error_at(
                                m.span,
                                &gen::The_left_hand_side_of_an_assignment_expression_must_be_a_variable_or_a_property_access,
                                &[],
                            );
                            ok = false;
                        }
                    }
                }
                ok
            }
            _ => {
                self.error_at(
                    target.span(),
                    &gen::The_left_hand_side_of_an_assignment_expression_must_be_a_variable_or_a_property_access,
                    &[],
                );
                false
            }
        }
    }

    pub(crate) fn check_binary(&mut self, e: &'a Expr, ctx: Option<TypeId>) -> TypeId {
        let Expr::Binary {
            op,
            op_span,
            left,
            right,
            span,
        } = e
        else {
            unreachable!()
        };
        use BinOp::*;
        match op {
            Assign => {
                if expr_contains_optional_chain(left) {
                    let lt0 = self.check_expr(left, None);
                    let _ = lt0;
                    self.error_at(
                        left.span(),
                        &gen::The_left_hand_side_of_an_assignment_expression_may_not_be_an_optional_property_access,
                        &[],
                    );
                    return self.check_expr(right, None);
                }
                let ok = self.check_reference_for_assignment(left, false);
                if !ok {
                    return self.check_expr(right, None);
                }
                // A destructuring-assignment pattern (`[a, b] = …`, `({x} = …)`)
                // was validated element-wise by check_reference_for_assignment;
                // type-check the right-hand side on its own and skip the scalar
                // value/narrowing path below, which would mis-read the pattern as
                // a value expression.
                if matches!(&**left, Expr::Array { .. } | Expr::Object { .. }) {
                    return self.check_expr(right, None);
                }
                let lt = self.check_target_type(left);
                let rt = self.check_expr(right, Some(lt));
                // The assigned value must be assignable to the target's declared
                // type (`x = v` where `x: number` and `v: string` is TS2322).
                // `check_reference_for_assignment` already handled non-assignable
                // *targets* (const / readonly → TS2540/2588), so only run the
                // value check when the target itself was assignable. An object or
                // arrow right-hand side is contextually typed by `lt` above, which
                // makes its own shape win, so this relation check does not
                // double-report. tsc anchors the diagnostic at the left-hand side.
                if !self.types.is_any_or_error(lt) && !self.types.is_error(rt) {
                    self.check_assignable(rt, lt, left.span(), None, Some(right));
                }
                rt
            }
            AddAssign | SubAssign | MulAssign | DivAssign | ModAssign | ExpAssign | ShlAssign
            | ShrAssign | UShrAssign | AmpAssign | BarAssign | CaretAssign => {
                let ok = self.check_reference_for_assignment(left, false);
                let lt = self.check_target_type(left);
                // a compound assignment READS the target first: definite
                // assignment applies (tsc AssignmentKind.Compound); the
                // target bypasses the read seam, so check here
                self.da_check_compound_target(left);
                let rt = self.check_expr(right, None);
                let left_null_value = matches!(&**left, Expr::NullLit { .. })
                    && self.report_direct_unusable_value(left);
                if *op == AddAssign {
                    if self.options.strict_null_checks()
                        && !self.plus_string_like(lt)
                        && !self.plus_any_like(lt)
                        && !self.plus_any_like(rt)
                    {
                        self.report_direct_unusable_value(right);
                    }
                } else {
                    let right_unusable = self.report_direct_unusable_value(right);
                    if ok && !left_null_value && !self.is_arithmetic_operand(lt) {
                        self.error_at(left.span(), &gen::The_left_hand_side_of_an_arithmetic_operation_must_be_of_type_any_number_bigint_or_an_enum_type, &[]);
                    }
                    if !right_unusable && !self.is_arithmetic_operand(rt) {
                        self.error_at(right.span(), &gen::The_right_hand_side_of_an_arithmetic_operation_must_be_of_type_any_number_bigint_or_an_enum_type, &[]);
                    }
                }
                self.types.number
            }
            AmpAmpAssign | BarBarAssign | QuestionQuestionAssign => {
                self.check_reference_for_assignment(left, false);
                let lt = self.check_target_type(left);
                let rt = self.check_expr(right, Some(lt));
                rt
            }
            Comma => {
                let side_effect_free = matches!(
                    &**left,
                    Expr::Ident(_)
                        | Expr::NumLit { .. }
                        | Expr::StrLit { .. }
                        | Expr::BoolLit { .. }
                        | Expr::NullLit { .. }
                        | Expr::BigIntLit { .. }
                        | Expr::RegexLit { .. }
                );
                if side_effect_free {
                    self.error_at(
                        left.span(),
                        &gen::Left_side_of_comma_operator_is_unused_and_has_no_side_effects,
                        &[],
                    );
                }
                self.check_expr(left, None);
                self.check_expr(right, ctx)
            }
            AmpAmp => {
                let lt = self.check_expr(left, None);
                self.check_testable(left, lt, TruthinessContext::LogicalAndLeft);
                // the right operand is evaluated only when the left is truthy, so
                // narrow it by the left's truthiness.
                let rt = self.narrowed(|this| {
                    this.narrow_by_condition(left, true);
                    this.check_expr(right, ctx)
                });
                let lreg = self.types.regular(lt);
                let r = self.types.regular(rt);
                // tsc: the definitely-falsy slice of the left (of the RIGHT's
                // widened type in non-strict mode) unioned with the right.
                // tsc's `hasTypeFacts(left, Truthy)` short-circuit is
                // deliberately omitted: it only distinguishes definitely-falsy
                // lefts, where tsc's self-referential initializers resolve to
                // `any` by circularity (witness.ts) — machinery tsrs lacks.
                let src = if self.options.strict_null_checks() {
                    lreg
                } else {
                    self.types.widen_literal(r)
                };
                let falsy_left = self.definitely_falsy_part(src);
                self.types.union(vec![falsy_left, r])
            }
            BarBar => {
                let lt = self.check_expr(left, None);
                self.check_testable(left, lt, TruthinessContext::LogicalOrLeft);
                // the right operand is evaluated only when the left is falsy, so
                // narrow it by the left's falsiness (e.g. `x == null || x.foo`
                // narrows `x` to non-null in `x.foo`).
                let rt = self.narrowed(|this| {
                    this.narrow_by_condition(left, false);
                    this.check_expr(right, ctx)
                });
                let lreg = self.types.regular(lt);
                let r = self.types.regular(rt);
                // tsc: a left that can never be falsy short-circuits to
                // itself; otherwise remove the definitely-falsy constituents
                // and (strict) the nullable ones, union with the right.
                if self.type_facts(lreg) & facts::FALSY == 0 {
                    lt
                } else {
                    let kept = self.facts_filter(lreg, facts::TRUTHY, false);
                    let nn = if self.options.strict_null_checks() {
                        self.facts_filter(kept, facts::NE_UNDEFINED_OR_NULL, true)
                    } else {
                        kept
                    };
                    self.types.union(vec![nn, r])
                }
            }
            QuestionQuestion => {
                let lt = self.check_expr(left, None);
                let lreg = self.types.regular(lt);
                let lnn = self.non_nullable(lreg);
                if lnn == lreg
                    && !self.types.is_any_or_error(lreg)
                    && !matches!(self.types.kind(lreg), TypeKind::Unknown)
                {
                    self.error_at(
                        left.span(),
                        &gen::Right_operand_of_is_unreachable_because_the_left_operand_is_never_nullish,
                        &[],
                    );
                } else if !self.types.is_any_or_error(lreg)
                    && self.types.union_members(lreg).iter().all(|&m| {
                        matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined)
                    })
                {
                    self.error_at(left.span(), &gen::This_expression_is_always_nullish, &[]);
                }
                let rt = self.check_expr(right, ctx);
                let r = self.types.regular(rt);
                // tsc: a left that can never be nullish short-circuits to
                // itself; otherwise NonNullable(left) | right, where the
                // NonNullable adjustment only applies under strictNullChecks
                if self.type_facts(lreg) & facts::EQ_UNDEFINED_OR_NULL == 0 {
                    lt
                } else if self.options.strict_null_checks() {
                    let nn = self.facts_filter(lreg, facts::NE_UNDEFINED_OR_NULL, true);
                    self.types.union(vec![nn, r])
                } else {
                    self.types.union(vec![lreg, r])
                }
            }
            EqEq | NotEq | EqEqEq | NotEqEq => {
                let lt = self.check_expr(left, None);
                let rt = self.check_expr(right, None);
                let obj_lit = |e: &Expr| matches!(e, Expr::Object { .. } | Expr::Array { .. });
                if obj_lit(left) && obj_lit(right) {
                    let result = if matches!(op, EqEq | EqEqEq) {
                        "false"
                    } else {
                        "true"
                    };
                    self.error_at(
                        *span,
                        &gen::This_condition_will_always_return_0_since_JavaScript_compares_objects_by_reference_not_value,
                        &[result.to_string()],
                    );
                } else {
                    self.check_comparability(lt, rt, *span, left, right);
                }
                self.types.boolean
            }
            Lt | Gt | LtEq | GtEq => {
                let lt = self.check_expr(left, None);
                let rt = self.check_expr(right, None);
                let l_unusable = self.report_direct_unusable_value(left);
                let r_unusable = self.report_direct_unusable_value(right);
                if l_unusable || r_unusable {
                    return self.types.boolean;
                }
                // tsc checkNonNullType on each operand (18048/18047 instead
                // of 2365 for nullish operands; the stripped types drive the
                // applicability check)
                let lt = self.check_operand_non_nullish(lt, left);
                let rt = self.check_operand_non_nullish(rt, right);
                let l_ok = self.is_comparison_operand(lt);
                let r_ok = self.is_comparison_operand(rt);
                if !(l_ok && r_ok && self.comparison_compatible(lt, rt)) {
                    let ld = self.display_type_widened(lt);
                    let rd = self.display_type_widened(rt);
                    self.error_at(
                        *span,
                        &gen::Operator_0_cannot_be_applied_to_types_1_and_2,
                        &[op.text().to_string(), ld, rd],
                    );
                }
                self.types.boolean
            }
            In => {
                // TS 6.0 checks `in` operands via plain assignability:
                // left -> string | number | symbol, right -> object
                let lt = self.check_expr(left, None);
                let rt = self.check_expr(right, None);
                let lt_r = self.types.regular(lt);
                let rt_r = self.types.regular(rt);
                let left_unusable = self.report_direct_unusable_value(left);
                let right_unusable = self.report_direct_unusable_value(right);
                let key_union = {
                    let s = self.types.string;
                    let n = self.types.number;
                    let sym = self.types.es_symbol;
                    self.types.union(vec![s, n, sym])
                };
                if !left_unusable && !self.types.is_any_or_error(lt_r) {
                    self.check_assignable(lt_r, key_union, left.span(), None, None);
                }
                if !right_unusable && !self.types.is_any_or_error(rt_r) {
                    let obj = self.types.non_primitive;
                    self.check_assignable(rt_r, obj, right.span(), None, None);
                }
                self.types.boolean
            }
            Instanceof => {
                let lt = self.check_expr(left, None);
                let rt = self.check_expr(right, None);
                let l = self.types.regular(lt);
                let l_ok = matches!(
                    self.types.kind(l),
                    TypeKind::Any
                        | TypeKind::Error
                        | TypeKind::Unknown
                        | TypeKind::NonPrimitive
                        | TypeKind::TypeParam(_)
                        | TypeKind::Union(_)
                ) || self.is_object_like(l);
                if !l_ok {
                    self.error_at(
                        left.span(),
                        &gen::The_left_hand_side_of_an_instanceof_expression_must_be_of_type_any_an_object_type_or_a_type_parameter,
                        &[],
                    );
                }
                let r = self.types.regular(rt);
                let r_callable = match self.types.kind(r) {
                    TypeKind::Any
                    | TypeKind::Error
                    | TypeKind::ClassStatics(_)
                    | TypeKind::MappedClassStatics(_, _) => true,
                    _ => {
                        !self.call_signatures_of(r).is_empty()
                            || !self.ctor_signatures_of(r).is_empty()
                    }
                };
                if !r_callable {
                    self.error_at(
                        right.span(),
                        &gen::The_right_hand_side_of_an_instanceof_expression_must_be_either_of_type_any_a_class_function_or_other_type_assignable_to_the_Function_interface_type_or_an_object_type_with_a_Symbol_hasInstance_method,
                        &[],
                    );
                }
                self.types.boolean
            }
            Add => {
                let lt = self.check_expr(left, None);
                let rt = self.check_expr(right, None);
                let (mut lt, mut rt) = (lt, rt);
                if self.options.strict_null_checks()
                    && !self.plus_string_like(lt)
                    && !self.plus_string_like(rt)
                    && !self.plus_any_like(lt)
                    && !self.plus_any_like(rt)
                {
                    let l_unusable = self.report_direct_unusable_value(left);
                    let r_unusable = self.report_direct_unusable_value(right);
                    if l_unusable || r_unusable {
                        return self.types.any;
                    }
                    // strict-only checkNonNullType for `+` (oracle: the
                    // non-strict `undefined + 1` keeps the 2365), and only
                    // for operands actually CONTAINING null/undefined — the
                    // unknown-operand 18046 stays off for `+` (tsrs inference
                    // gaps would surface it where tsc infers a real type)
                    if self.contains_nullish_member(lt) {
                        lt = self.check_operand_non_nullish(lt, left);
                    }
                    if self.contains_nullish_member(rt) {
                        rt = self.check_operand_non_nullish(rt, right);
                    }
                }
                self.check_plus(lt, rt, *span, op_span)
            }
            Sub | Mul | Div | Mod | Exp | Shl | Shr | UShr | BitAnd | BitOr | BitXor => {
                if matches!(op, Exp) {
                    self.check_exponentiation_left_operand(left);
                }
                let lt = self.check_expr(left, None);
                let rt = self.check_expr(right, None);
                // boolean & / | suggest && / || (2447)
                let boolish = |c: &Self, t: TypeId| {
                    matches!(c.types.kind(t), TypeKind::BoolLit(_))
                        || c.types
                            .union_members(t)
                            .iter()
                            .all(|&m| matches!(c.types.kind(m), TypeKind::BoolLit(_)))
                };
                if matches!(op, BitAnd | BitOr) && boolish(self, lt) && boolish(self, rt) {
                    let (sym, sug) = if matches!(op, BitAnd) {
                        ("&", "&&")
                    } else {
                        ("|", "||")
                    };
                    self.error_at(
                        *span,
                        &gen::The_0_operator_is_not_allowed_for_boolean_types_Consider_using_1_instead,
                        &[sym.to_string(), sug.to_string()],
                    );
                    return self.types.number;
                }
                let l_unusable = self.report_direct_unusable_value(left);
                let r_unusable = self.report_direct_unusable_value(right);
                // tsc checkNonNullType on each operand (18048/18047 before
                // the arithmetic-operand applicability errors; a direct
                // null/undefined literal keeps its 18050 report above, and
                // the error result is arithmetic-ok so nothing double-fires)
                let lt = if l_unusable {
                    lt
                } else {
                    self.check_operand_non_nullish(lt, left)
                };
                let rt = if r_unusable {
                    rt
                } else {
                    self.check_operand_non_nullish(rt, right)
                };
                let l_ok = self.is_arithmetic_operand(lt);
                let r_ok = self.is_arithmetic_operand(rt);
                if !l_unusable && !l_ok {
                    self.error_at(
                        left.span(),
                        &gen::The_left_hand_side_of_an_arithmetic_operation_must_be_of_type_any_number_bigint_or_an_enum_type,
                        &[],
                    );
                }
                if !r_unusable && !r_ok {
                    self.error_at(
                        right.span(),
                        &gen::The_right_hand_side_of_an_arithmetic_operation_must_be_of_type_any_number_bigint_or_an_enum_type,
                        &[],
                    );
                }
                if matches!(
                    self.types.kind(lt),
                    TypeKind::Bigint | TypeKind::BigIntLit(_)
                ) {
                    self.types.bigint
                } else {
                    self.types.number
                }
            }
        }
    }

    fn check_exponentiation_left_operand(&mut self, left: &'a Expr) {
        match left {
            Expr::Unary { op, span, .. } => {
                self.error_at(
                    *span,
                    &gen::An_unary_expression_with_the_0_operator_is_not_allowed_in_the_left_hand_side_of_an_exponentiation_expression_Consider_enclosing_the_expression_in_parentheses,
                    &[unary_operator_text(*op).to_string()],
                );
            }
            Expr::Await { span, .. } => {
                self.error_at(
                    *span,
                    &gen::An_unary_expression_with_the_0_operator_is_not_allowed_in_the_left_hand_side_of_an_exponentiation_expression_Consider_enclosing_the_expression_in_parentheses,
                    &["await".to_string()],
                );
            }
            Expr::Assertion { kind, span, .. }
                if matches!(kind, AssertionKind::Angle | AssertionKind::ConstAssert) =>
            {
                self.error_at(
                    *span,
                    &gen::A_type_assertion_expression_is_not_allowed_in_the_left_hand_side_of_an_exponentiation_expression_Consider_enclosing_the_expression_in_parentheses,
                    &[],
                );
            }
            _ => {}
        }
    }

    fn display_type_widened(&mut self, t: TypeId) -> String {
        let r = self.types.regular(t);
        let w = self.types.widen_literal(r);
        self.display_type(w)
    }

    /// type of an assignment target (no narrowing — declared types)
    pub(crate) fn check_target_type(&mut self, target: &'a Expr) -> TypeId {
        match target {
            Expr::Ident(id) => {
                if let Some(sym) = self.lookup_value(self.current_scope, &id.name) {
                    // declared (un-narrowed) type for assignment targets
                    self.type_of_symbol(sym)
                } else {
                    // unresolved: normal expression checking (2304, arguments…)
                    self.check_expr(target, None)
                }
            }
            _ => {
                // write position: tsc checks the assignment against the
                // DECLARED type of the target REFERENCE (AssignmentKind.
                // Definite). Destructuring patterns and parenthesized ident
                // targets blanket-mark every leaf inside (`for ({x, y} of
                // …)`, `(async) of …`); prop/elem targets mark only their
                // own node, so RECEIVER reads still narrow (`control[key] =
                // value` under an `if (control !== undefined)` guard).
                let mut stripped: &Expr = target;
                while let Expr::Paren { inner, .. } = stripped {
                    stripped = inner;
                }
                match stripped {
                    Expr::Object { .. } | Expr::Array { .. } | Expr::Ident(_) => {
                        self.cflags.pattern_target += 1;
                        let t = self.check_expr(target, None);
                        self.cflags.pattern_target -= 1;
                        t
                    }
                    _ => {
                        let saved = self.cflags.assign_target;
                        self.cflags.assign_target =
                            crate::checker::exprs::node_key_expr(stripped);
                        let t = self.check_expr(target, None);
                        self.cflags.assign_target = saved;
                        t
                    }
                }
            }
        }
    }

    fn check_plus(&mut self, lt: TypeId, rt: TypeId, span: Span, _op_span: &Span) -> TypeId {
        let str_like = |c: &Self, t: TypeId| match c.types.kind(t) {
            TypeKind::String | TypeKind::StrLit(_) => true,
            TypeKind::Intersection(ms) => ms
                .iter()
                .any(|&m| matches!(c.types.kind(m), TypeKind::String | TypeKind::StrLit(_))),
            _ => false,
        };
        let num_like = |c: &mut Self, t: TypeId| c.is_arithmetic_operand(t);
        let any_like =
            |c: &Self, t: TypeId| matches!(c.types.kind(t), TypeKind::Any | TypeKind::Error);
        if str_like(self, lt) || str_like(self, rt) {
            return self.types.string;
        }
        if any_like(self, lt) || any_like(self, rt) {
            return self.types.any;
        }
        if num_like(self, lt) && num_like(self, rt) {
            if matches!(
                self.types.kind(lt),
                TypeKind::Bigint | TypeKind::BigIntLit(_)
            ) {
                return self.types.bigint;
            }
            return self.types.number;
        }
        let ld = self.display_type_widened(lt);
        let rd = self.display_type_widened(rt);
        self.error_at(
            span,
            &gen::Operator_0_cannot_be_applied_to_types_1_and_2,
            &["+".to_string(), ld, rd],
        );
        self.types.any
    }

    fn is_comparison_operand(&mut self, t: TypeId) -> bool {
        if let TypeKind::Intersection(ms) = self.types.kind(t) {
            let ms = ms.clone();
            return ms.iter().any(|&m| self.is_comparison_operand(m));
        }
        matches!(
            self.types.kind(t),
            TypeKind::Any
                | TypeKind::Error
                | TypeKind::Number
                | TypeKind::NumLit(_)
                | TypeKind::String
                | TypeKind::StrLit(_)
                | TypeKind::Bigint
                | TypeKind::BigIntLit(_)
        )
    }

    fn cmp_numeric(&mut self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Any
            | TypeKind::Error
            | TypeKind::Number
            | TypeKind::NumLit(_)
            | TypeKind::Bigint
            | TypeKind::BigIntLit(_) => true,
            TypeKind::Intersection(ms) => {
                let ms = ms.clone();
                ms.iter().any(|&m| self.cmp_numeric(m))
            }
            _ => false,
        }
    }

    fn cmp_stringy(&mut self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Any | TypeKind::Error | TypeKind::String | TypeKind::StrLit(_) => true,
            TypeKind::Intersection(ms) => {
                let ms = ms.clone();
                ms.iter().any(|&m| self.cmp_stringy(m))
            }
            _ => false,
        }
    }

    fn comparison_compatible(&mut self, lt: TypeId, rt: TypeId) -> bool {
        (self.cmp_numeric(lt) && self.cmp_numeric(rt))
            || (self.cmp_stringy(lt) && self.cmp_stringy(rt))
    }

    /// 2367 comparison-overlap check
    fn check_comparability(
        &mut self,
        lt: TypeId,
        rt: TypeId,
        span: Span,
        _l: &'a Expr,
        _r: &'a Expr,
    ) {
        let lw = self.types.regular(lt);
        let rw = self.types.regular(rt);
        if self.types.is_any_or_error(lw) || self.types.is_any_or_error(rw) {
            return;
        }
        // tsc's comparable relation always admits null/undefined operands
        // (`s != null` is legal null-guarding even when s can't be nullish)
        if matches!(self.types.kind(lw), TypeKind::Null | TypeKind::Undefined)
            || matches!(self.types.kind(rw), TypeKind::Null | TypeKind::Undefined)
        {
            return;
        }
        if self.is_assignable_to(lw, rw) || self.is_assignable_to(rw, lw) {
            return;
        }
        // literal vs base of other side (e.g. "a" === someString); when BOTH
        // sides are unit-only, units must overlap (1|2 vs 3 errors)
        let unit_only = |c: &mut Self, t: TypeId| {
            c.types
                .union_members(t)
                .iter()
                .all(|&m| c.is_literal_type_pub(m))
        };
        let both_units = unit_only(self, lw) && unit_only(self, rw);
        if !both_units {
            let lwide = self.types.widen_literal(lw);
            let rwide = self.types.widen_literal(rw);
            if self.is_assignable_to(lwide, rwide) || self.is_assignable_to(rwide, lwide) {
                return;
            }
        }
        let (ld, rd) = if both_units {
            (self.display_type(lw), self.display_type(rw))
        } else {
            (self.display_type_widened(lt), self.display_type_widened(rt))
        };
        self.error_at(
            span,
            &gen::This_comparison_appears_to_be_unintentional_because_the_types_0_and_1_have_no_overlap,
            &[ld, rd],
        );
    }

    /// truthiness-context checks: 1345 (void), 2774 (function values), 2801
    /// (Promise), 2872/2873 syntactically-constant truthiness.
    pub(crate) fn check_testable(&mut self, cond: &'a Expr, t: TypeId, ctx: TruthinessContext) {
        let syntactic_truthiness = self.syntactic_truthiness(cond);
        if let Some((truthy, span)) = syntactic_truthiness {
            let msg = if truthy {
                &gen::This_kind_of_expression_is_always_truthy
            } else {
                &gen::This_kind_of_expression_is_always_falsy
            };
            self.error_at(span, msg, &[]);
        }
        let r = self.types.regular(t);
        if matches!(self.types.kind(r), TypeKind::Void) {
            self.error_at(
                cond.span(),
                &gen::An_expression_of_type_void_cannot_be_tested_for_truthiness,
                &[],
            );
            return;
        }
        if self.options.strict_null_checks()
            && ctx.allows_function_condition()
            && syntactic_truthiness.is_none()
            && self.is_always_defined_callable_condition(t)
            && !self.is_optional_property_condition(cond)
        {
            self.error_at(
                cond.span(),
                &gen::This_condition_will_always_return_true_since_this_function_is_always_defined_Did_you_mean_to_call_it_instead,
                &[],
            );
        }
        if ctx.allows_promise_condition() {
            if let TypeKind::Ref(sym, _) = self.types.kind(r) {
                if self.symbol(*sym).name == "Promise" {
                    let d = self.display_type(r);
                    self.error_at(
                        cond.span(),
                        &gen::This_condition_will_always_return_true_since_this_0_is_always_defined,
                        &[d],
                    );
                }
            }
        }
    }

    fn is_optional_property_condition(&mut self, cond: &'a Expr) -> bool {
        let mut cond = cond;
        while let Expr::Paren { inner, .. } = cond {
            cond = inner;
        }
        let Expr::PropAccess {
            obj,
            question_dot,
            name,
            ..
        } = cond
        else {
            return false;
        };
        if *question_dot {
            return true;
        }
        let Some(obj_t) = self.condition_object_type(obj) else {
            return false;
        };
        self.prop_info_of_type(obj_t, &name.name)
            .is_some_and(|p| p.optional)
    }

    fn condition_object_type(&mut self, obj: &'a Expr) -> Option<TypeId> {
        match obj {
            Expr::Paren { inner, .. } => self.condition_object_type(inner),
            Expr::Ident(id) => self
                .lookup_value(self.current_scope, &id.name)
                .map(|sym| self.type_of_symbol(sym)),
            Expr::This { .. } => Some(self.check_expr(obj, None)),
            Expr::Super { .. } => self.current_super_type(),
            _ => self
                .caches
                .expr_type_cache
                .get(&(obj as *const Expr as usize))
                .copied(),
        }
    }

    fn is_always_defined_callable_condition(&mut self, t: TypeId) -> bool {
        let r = self.types.regular(t);
        match self.types.kind(r).clone() {
            TypeKind::Any
            | TypeKind::Error
            | TypeKind::Unknown
            | TypeKind::Never
            | TypeKind::Undefined
            | TypeKind::Null
            | TypeKind::Void => false,
            TypeKind::Union(members) => {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|&m| self.is_always_defined_callable_condition(m))
            }
            _ => !self.call_signatures_of(r).is_empty(),
        }
    }

    fn syntactic_truthiness(&self, cond: &'a Expr) -> Option<(bool, Span)> {
        self.syntactic_truthiness_inner(cond, None)
    }

    fn syntactic_truthiness_inner(
        &self,
        cond: &'a Expr,
        diagnostic_span: Option<Span>,
    ) -> Option<(bool, Span)> {
        let span_for = |span: Span| diagnostic_span.unwrap_or(span);
        match cond {
            Expr::Paren { inner, span } => {
                self.syntactic_truthiness_inner(inner, Some(span_for(*span)))
            }
            Expr::Assertion { expr, span, .. } | Expr::NonNull { expr, span } => {
                self.syntactic_truthiness_inner(expr, Some(span_for(*span)))
            }
            Expr::NullLit { span } => Some((false, span_for(*span))),
            Expr::Ident(id) if id.name == "undefined" => Some((false, span_for(id.span))),
            Expr::NumLit { value, span, .. } if *value != 0.0 && *value != 1.0 => {
                Some((true, span_for(*span)))
            }
            Expr::BigIntLit { span, .. } => Some((true, span_for(*span))),
            Expr::StrLit { value, span } => Some((!value.is_empty(), span_for(*span))),
            Expr::RegexLit { span, .. } | Expr::Array { span, .. } | Expr::Object { span, .. } => {
                Some((true, span_for(*span)))
            }
            Expr::Unary {
                op: UnaryOp::Void,
                span,
                ..
            } => Some((false, span_for(*span))),
            Expr::Arrow(f) => Some((true, span_for(f.span))),
            Expr::FunctionExpr(f) => {
                let span = f.name.as_ref().map_or(f.span, |name| name.span());
                Some((true, span_for(span)))
            }
            Expr::ClassExpr(c) => Some((true, span_for(c.span))),
            Expr::Template { parts, span } if parts.len() == 1 => match &parts[0] {
                crate::ast::TemplatePart::Str(text) => Some((!text.is_empty(), span_for(*span))),
                _ => None,
            },
            _ => None,
        }
    }

    /// tsc getTypeFactsWorker (oracle typescript.js:74338) projected onto the
    /// bits in [`facts`]: which truthiness/nullish-equality facts a type can
    /// satisfy. Type parameters take their base constraint's facts (ALL when
    /// unconstrained) — tsc never collapses them; unions OR their members'
    /// facts; intersections AND theirs (skipping object operands when a
    /// primitive operand is present). Non-strict mode adds the
    /// implicit-nullability facts (EQ_* and FALSY) to every concrete kind,
    /// which is why non-strict falsy narrowing filters nothing.
    pub(crate) fn type_facts(&mut self, t: TypeId) -> u32 {
        self.type_facts_depth(t, 0)
    }

    fn type_facts_depth(&mut self, t: TypeId, depth: u32) -> u32 {
        use facts::*;
        if depth > 10 {
            return ALL;
        }
        let strict = self.options.strict_null_checks();
        // concrete non-nullish kinds: `x === undefined/null` can only hold
        // through the implicit null/undefined of non-strict mode
        let concrete = |truthy: bool, falsy: bool| -> u32 {
            let mut m = NE_UNDEFINED | NE_NULL | NE_UNDEFINED_OR_NULL;
            if !strict {
                m |= EQ_UNDEFINED | EQ_NULL | EQ_UNDEFINED_OR_NULL;
            }
            if truthy {
                m |= TRUTHY;
            }
            if falsy || !strict {
                m |= FALSY;
            }
            m
        };
        match self.types.kind(t).clone() {
            TypeKind::Any | TypeKind::Unknown | TypeKind::Error => ALL,
            TypeKind::TypeParam(sym) => match self.constraint_of_type_param(sym) {
                Some(c) => self.type_facts_depth(c, depth + 1),
                None => ALL,
            },
            TypeKind::IndexedAccess(..)
            | TypeKind::DeferredCond(..)
            | TypeKind::DeferredMapped(..) => ALL,
            // keyof T's base constraint is `string | number | symbol`
            TypeKind::Keyof(_) => concrete(true, true),
            TypeKind::String | TypeKind::Number | TypeKind::Bigint => concrete(true, true),
            TypeKind::StrLit(s) => {
                let empty = s.is_empty();
                concrete(!empty, empty)
            }
            // tsc gives template-literal types NonEmptyString facts
            TypeKind::TemplateLit(_) => concrete(true, false),
            TypeKind::NumLit(bits) => {
                let zero = f64::from_bits(bits) == 0.0;
                concrete(!zero, zero)
            }
            TypeKind::BigIntLit(v) => {
                let zero = is_zero_bigint(&v);
                concrete(!zero, zero)
            }
            TypeKind::BoolLit(b) => concrete(b, !b),
            TypeKind::EsSymbol => concrete(true, false),
            TypeKind::Undefined | TypeKind::Void => {
                EQ_UNDEFINED | EQ_UNDEFINED_OR_NULL | NE_NULL | FALSY
            }
            TypeKind::Null => EQ_NULL | EQ_UNDEFINED_OR_NULL | NE_UNDEFINED | FALSY,
            TypeKind::Never => 0,
            TypeKind::NonPrimitive => concrete(true, false),
            // tsc folds enums into NumberFacts; member values are not consulted
            TypeKind::EnumType(_) | TypeKind::EnumMember(_) => concrete(true, true),
            TypeKind::Union(ms) => ms
                .into_iter()
                .fold(0, |acc, m| acc | self.type_facts_depth(m, depth + 1)),
            TypeKind::Intersection(ms) => {
                let has_prim = ms.iter().any(|&m| {
                    matches!(
                        self.types.kind(m),
                        TypeKind::String
                            | TypeKind::StrLit(_)
                            | TypeKind::TemplateLit(_)
                            | TypeKind::Number
                            | TypeKind::NumLit(_)
                            | TypeKind::Bigint
                            | TypeKind::BigIntLit(_)
                            | TypeKind::BoolLit(_)
                            | TypeKind::EsSymbol
                            | TypeKind::Void
                            | TypeKind::Undefined
                            | TypeKind::Null
                            | TypeKind::EnumType(_)
                            | TypeKind::EnumMember(_)
                    )
                });
                let mut acc = ALL;
                for m in ms {
                    if has_prim
                        && matches!(
                            self.types.kind(m),
                            TypeKind::Anon(_)
                                | TypeKind::DeferredObj(_)
                                | TypeKind::Iface(_)
                                | TypeKind::Ref(..)
                                | TypeKind::Tuple(_)
                                | TypeKind::ReadonlyArray(_)
                                | TypeKind::ReadonlyTuple(_)
                                | TypeKind::ClassStatics(_)
                                | TypeKind::MappedClassStatics(..)
                                | TypeKind::MappedIface(..)
                        )
                    {
                        continue;
                    }
                    acc &= self.type_facts_depth(m, depth + 1);
                }
                acc
            }
            // object family: only the memberless anonymous `{}` keeps every
            // fact under strict (tsc EmptyObjectStrictFacts) — it admits any
            // non-nullish value, falsy ones included
            TypeKind::Anon(_) | TypeKind::DeferredObj(_) => {
                let empty = self
                    .shape_of_type(t)
                    .is_some_and(|s| {
                        let sh = self.types.shape(s);
                        sh.props.is_empty()
                            && sh.call_sigs.is_empty()
                            && sh.ctor_sigs.is_empty()
                            && sh.index_infos.is_empty()
                    });
                if empty {
                    if strict {
                        ALL & !(EQ_UNDEFINED | EQ_NULL | EQ_UNDEFINED_OR_NULL)
                    } else {
                        ALL
                    }
                } else {
                    concrete(true, false)
                }
            }
            // Iface/Ref/tuples/arrays/statics/namespace & enum objects
            _ => concrete(true, false),
        }
    }

    /// tsc getTypeWithFacts + the getAdjustedTypeWithFacts extras, shared by
    /// the truthiness and nullish-equality narrowers: keep the constituents
    /// whose facts include `include`. Strict `unknown` decomposes into
    /// `{} | null | undefined` first (tsc unknownUnionType) and recombines
    /// when the filter keeps all of it; `adjust_nn` applies the strict
    /// NonNullable<> wrap to survivors that kept an EQUndefinedOrNull fact
    /// (unconstrained / nullable-constrained type params — tsc maps these
    /// through getGlobalNonNullableTypeInstantiation for Truthy and
    /// NEUndefinedOrNull).
    pub(crate) fn facts_filter(&mut self, t: TypeId, include: u32, adjust_nn: bool) -> TypeId {
        let strict = self.options.strict_null_checks();
        let mut decomposed = false;
        let members = if strict && matches!(self.types.kind(t), TypeKind::Unknown) {
            decomposed = true;
            let empty = self.empty_object_type();
            vec![empty, self.types.null, self.types.undefined]
        } else {
            self.types.union_members(t)
        };
        let total = members.len();
        let mut kept = Vec::new();
        let mut wrapped = false;
        for m in members {
            let f = self.type_facts(m);
            if f & include == 0 {
                continue;
            }
            if adjust_nn && strict && f & facts::EQ_UNDEFINED_OR_NULL != 0 {
                // `any` absorbs the intersection and stays `any`; type params
                // become NonNullable<T>
                let nn = self.symbolic_non_nullable(m);
                wrapped = wrapped || nn != m;
                kept.push(nn);
            } else {
                kept.push(m);
            }
        }
        if decomposed && kept.len() == total && !wrapped {
            return t; // recombineUnknownType
        }
        self.types.union(kept)
    }

    /// tsc getTypeWithFacts(t, TypeFacts.Falsy): keep the constituents whose
    /// facts include Falsy. `any` / `unknown` / type params pass whole, and
    /// in non-strict mode every kind passes through its implicit
    /// nullability — this never manufactures `never` out of them, unlike the
    /// pre-CFG shape of this helper.
    pub fn falsy_part(&mut self, t: TypeId) -> TypeId {
        self.facts_filter(t, facts::FALSY, false)
    }

    /// tsc getAdjustedTypeWithFacts(t, TypeFacts.Truthy): filter to the
    /// truthiness-capable constituents (`boolean` splits to `true`, strict
    /// `unknown` becomes `{}`), then under strictNullChecks wrap survivors
    /// that could still be undefined/null in NonNullable<>.
    pub fn truthy_part(&mut self, t: TypeId) -> TypeId {
        self.facts_filter(t, facts::TRUTHY, true)
    }

    /// tsc extractDefinitelyFalsyTypes (oracle 72477): the definitely-falsy
    /// slice of each constituent — whole primitives contribute their falsy
    /// literal ("" / 0 / 0n), `any`/`unknown`/nullish kinds pass whole,
    /// everything else drops. This is the `&&` result-type helper; the
    /// narrowing filter is `falsy_part`.
    pub(crate) fn definitely_falsy_part(&mut self, t: TypeId) -> TypeId {
        let members = self.types.union_members(t);
        let mut kept = Vec::new();
        for m in members {
            let part = match self.types.kind(m).clone() {
                TypeKind::String => Some(self.types.string_lit("")),
                TypeKind::Number => Some(self.types.number_lit(0.0)),
                TypeKind::Bigint => Some(self.types.intern_kind(TypeKind::BigIntLit("0n".into()))),
                TypeKind::Any
                | TypeKind::Unknown
                | TypeKind::Error
                | TypeKind::Undefined
                | TypeKind::Null
                | TypeKind::Void => Some(m),
                TypeKind::BoolLit(false) => Some(m),
                TypeKind::StrLit(s) if s.is_empty() => Some(m),
                TypeKind::NumLit(bits) if f64::from_bits(bits) == 0.0 => Some(m),
                TypeKind::BigIntLit(v) if is_zero_bigint(&v) => Some(m),
                _ => None,
            };
            if let Some(p) = part {
                kept.push(p);
            }
        }
        self.types.union(kept)
    }
}

/// tsc TypeFacts bits consumed by the narrowing helpers (values mirror
/// oracle typescript.js TypeFacts; only the bits the checker consults are
/// modeled).
pub(crate) mod facts {
    pub const EQ_UNDEFINED: u32 = 1 << 16;
    pub const EQ_NULL: u32 = 1 << 17;
    pub const EQ_UNDEFINED_OR_NULL: u32 = 1 << 18;
    pub const NE_UNDEFINED: u32 = 1 << 19;
    pub const NE_NULL: u32 = 1 << 20;
    pub const NE_UNDEFINED_OR_NULL: u32 = 1 << 21;
    pub const TRUTHY: u32 = 1 << 22;
    pub const FALSY: u32 = 1 << 23;
    pub const ALL: u32 = EQ_UNDEFINED
        | EQ_NULL
        | EQ_UNDEFINED_OR_NULL
        | NE_UNDEFINED
        | NE_NULL
        | NE_UNDEFINED_OR_NULL
        | TRUTHY
        | FALSY;
}

/// bigint literal text (`-0n`, `0x0n`-normalized digits) denoting zero
fn is_zero_bigint(v: &str) -> bool {
    let s = v.strip_suffix('n').unwrap_or(v);
    let s = s.strip_prefix('-').unwrap_or(s);
    !s.is_empty() && s.chars().all(|c| c == '0')
}

fn unary_operator_text(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Plus => "+",
        UnaryOp::Minus => "-",
        UnaryOp::Bang => "!",
        UnaryOp::Tilde => "~",
        UnaryOp::Typeof => "typeof",
        UnaryOp::Void => "void",
        UnaryOp::Delete => "delete",
    }
}
