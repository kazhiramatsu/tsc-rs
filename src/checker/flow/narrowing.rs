//! Type narrowing (control-flow analysis): truthiness / typeof / equality /
//! assertion / switch-discriminant narrowing, and the reference-key resolution
//! they key on. Split out of `stmts.rs`.

use crate::ast::*;
use crate::binder::flags;
use crate::checker::{Checker, RefKey};
use crate::types::{TypeId, TypeKind};
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    // ── narrowing ───────────────────────────────────────────────────────────

    pub fn ref_key_of_pub(&mut self, e: &Expr) -> Option<RefKey> {
        match e {
            Expr::Ident(id) => {
                let sym = self.lookup_value(self.current_scope, &id.name)?;
                Some(RefKey(sym, Vec::new()))
            }
            Expr::This { .. } => {
                let owner = *self
                    .stacks
                    .class_stack
                    .last()
                    .or_else(|| self.stacks.this_type_stack.last())?;
                let sym = self.this_param_of(owner);
                Some(RefKey(sym, Vec::new()))
            }
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let mut k = self.ref_key_of_pub(obj)?;
                k.1.push(name.name.clone());
                Some(k)
            }
            Expr::Paren { inner, .. } => self.ref_key_of_pub(inner),
            _ => None,
        }
    }

    /// like ref_key_of_pub but follows `?.` links too (for narrowing guards)
    fn ref_key_optional(&mut self, e: &Expr) -> Option<RefKey> {
        match e {
            Expr::Ident(id) => {
                let sym = self.lookup_value(self.current_scope, &id.name)?;
                Some(RefKey(sym, Vec::new()))
            }
            Expr::This { .. } => {
                let owner = *self
                    .stacks
                    .class_stack
                    .last()
                    .or_else(|| self.stacks.this_type_stack.last())?;
                let sym = self.this_param_of(owner);
                Some(RefKey(sym, Vec::new()))
            }
            Expr::PropAccess { obj, name, .. } => {
                let mut k = self.ref_key_optional(obj)?;
                k.1.push(name.name.clone());
                Some(k)
            }
            Expr::Paren { inner, .. } => self.ref_key_optional(inner),
            _ => None,
        }
    }

    /// current (possibly narrowed) type of a narrowable reference
    fn current_type_of_key(&mut self, e: &Expr, key: &RefKey) -> Option<TypeId> {
        if let Some(t) = self.fact_for(key) {
            return Some(t);
        }
        // declared type along the path
        let mut t = self.type_of_symbol(key.0);
        let root_key = RefKey(key.0, Vec::new());
        if let Some(rt) = self.fact_for(&root_key) {
            t = rt;
        }
        for part in &key.1 {
            t = self.prop_of_type(t, part)?;
        }
        let _ = e;
        Some(t)
    }

    pub fn apply_truthiness_narrowing(&mut self, e: &'a Expr, sense: bool) {
        self.narrow_by_condition(e, sense);
    }

    pub fn narrow_by_condition_for_flow(&mut self, cond: &'a Expr, sense: bool) {
        let prev = self.flow.record_da_facts;
        self.flow.record_da_facts = true;
        self.narrow_by_condition(cond, sense);
        self.flow.record_da_facts = prev;
    }

    fn collect_narrowing_facts(
        &mut self,
        f: impl FnOnce(&mut Self),
    ) -> (HashMap<RefKey, TypeId>, HashSet<RefKey>) {
        self.flow.facts.push(HashMap::new());
        self.flow.da_facts.push(HashSet::new());
        f(self);
        let facts = self.flow.facts.pop().unwrap_or_default();
        let da_facts = self.flow.da_facts.pop().unwrap_or_default();
        (facts, da_facts)
    }

    fn merge_narrowing_alternatives(
        &mut self,
        left: (HashMap<RefKey, TypeId>, HashSet<RefKey>),
        right: (HashMap<RefKey, TypeId>, HashSet<RefKey>),
    ) {
        let (left_facts, left_da_facts) = left;
        let (right_facts, right_da_facts) = right;
        for (key, left_ty) in left_facts {
            let Some(&right_ty) = right_facts.get(&key) else {
                continue;
            };
            let merged = if left_ty == right_ty {
                left_ty
            } else {
                self.types.union(vec![left_ty, right_ty])
            };
            if left_da_facts.contains(&key) && right_da_facts.contains(&key) {
                self.set_fact_for_definite_assignment(&key);
            }
            self.set_fact(key, merged);
        }
    }

    /// Resolve a call's user-defined type predicate (`x is T`,
    /// `this is T`, `asserts x [is T]`) to the expression whose flow type is
    /// narrowed and the predicate info.
    fn call_predicate(
        &mut self,
        callee: &'a Expr,
        args: &'a [Expr],
    ) -> Option<(&'a Expr, crate::types::PredInfo)> {
        let ct = match callee {
            Expr::Ident(id) => {
                let sym = self.lookup_value(self.current_scope, &id.name)?;
                let sym = self.resolve_alias_chain(sym);
                self.type_of_symbol(sym)
            }
            // a method/computed callee such as `Array.isArray` carries its
            // predicate on the resolved member type.
            Expr::PropAccess { .. } | Expr::ElemAccess { .. } => self.check_expr(callee, None),
            _ => return None,
        };
        let sigs = self.call_signatures_of(ct);
        let sid = *sigs.first()?;
        let sig = self.types.sig(sid).clone();
        let mut pred = sig.predicate?;
        // A generic guard's predicate (`isArr<T>(x: T | T[]): x is T[]`) carries
        // an abstract type; instantiate it with the type arguments inferred from
        // the call so the narrowing sees the concrete type (`x is number[]`).
        if !sig.type_params.is_empty() {
            if let Some(pty) = pred.ty {
                let mapper = self.infer_type_arguments(&sig, args, None);
                pred.ty = Some(self.instantiate_type(pty, &mapper));
            }
        }
        let target = match pred.param {
            -1 => match callee {
                Expr::PropAccess { obj, .. } | Expr::ElemAccess { obj, .. } => &**obj,
                _ => return None,
            },
            n if n >= 0 => args.get(n as usize)?,
            _ => return None,
        };
        Some((target, pred))
    }

    /// Narrow `cur` by a type predicate: to `pty` (true sense) or removing the
    /// members assignable to `pty` (false sense).
    fn narrow_to_pred(&mut self, cur: TypeId, pty: TypeId, sense: bool) -> TypeId {
        if sense {
            if matches!(self.types.kind(cur), TypeKind::Unknown | TypeKind::Any) {
                return pty;
            }
            let members = self.types.union_members(cur);
            let kept: Vec<TypeId> = members
                .into_iter()
                .filter(|&m| self.is_assignable_to(m, pty))
                .collect();
            if kept.is_empty() {
                pty
            } else {
                self.types.union(kept)
            }
        } else {
            let members = self.types.union_members(cur);
            let kept: Vec<TypeId> = members
                .into_iter()
                .filter(|&m| !self.is_assignable_to(m, pty))
                .collect();
            if kept.is_empty() {
                self.types.never
            } else {
                self.types.union(kept)
            }
        }
    }

    /// Apply the flow effect of an assertion call (`asserts x is T` narrows x to
    /// T; `asserts x` narrows x to truthy) so it persists after the call.
    pub fn apply_assertion_narrowing(&mut self, callee: &'a Expr, args: &'a [Expr]) {
        if let Some((arg, pred)) = self.call_predicate(callee, args) {
            if pred.asserts {
                if let Some(key) = self.ref_key_of_pub(arg) {
                    if let Some(cur) = self.current_type_of_key(arg, &key) {
                        let narrowed = match pred.ty {
                            Some(pty) => self.narrow_to_pred(cur, pty, true),
                            None => {
                                let ff = self.types.false_t;
                                let members = self.types.union_members(cur);
                                let kept: Vec<TypeId> = members
                                    .into_iter()
                                    .filter(|&m| {
                                        !matches!(
                                            self.types.kind(m),
                                            TypeKind::Null | TypeKind::Undefined | TypeKind::Void
                                        ) && m != ff
                                    })
                                    .collect();
                                if kept.is_empty() {
                                    cur
                                } else {
                                    self.types.union(kept)
                                }
                            }
                        };
                        self.set_fact(key, narrowed);
                    }
                }
            }
        }
    }

    /// Whether an expression is a boolean-producing narrowing form that, when
    /// stored in a `const`, can drive aliased-condition narrowing.
    pub(crate) fn is_guard_like_expr(e: &Expr) -> bool {
        match e {
            Expr::Paren { inner, .. } => Self::is_guard_like_expr(inner),
            Expr::Unary {
                op: UnaryOp::Bang,
                operand,
                ..
            } => Self::is_guard_like_expr(operand),
            Expr::Call { .. } => true,
            Expr::Binary { op, .. } => matches!(
                op,
                BinOp::EqEq
                    | BinOp::NotEq
                    | BinOp::EqEqEq
                    | BinOp::NotEqEq
                    | BinOp::Lt
                    | BinOp::Gt
                    | BinOp::LtEq
                    | BinOp::GtEq
                    | BinOp::In
                    | BinOp::Instanceof
                    | BinOp::AmpAmp
                    | BinOp::BarBar
                    | BinOp::QuestionQuestion
            ),
            _ => false,
        }
    }

    pub fn narrow_by_condition(&mut self, cond: &'a Expr, sense: bool) {
        match cond {
            Expr::Paren { inner, .. } => self.narrow_by_condition(inner, sense),
            Expr::Unary {
                op: UnaryOp::Bang,
                operand,
                ..
            } => self.narrow_by_condition(operand, !sense),
            // user-defined type guard: `if (isT(x)) { ... }`
            Expr::Call { callee, args, .. } => {
                if let Some((arg, pred)) = self.call_predicate(callee, args) {
                    if !pred.asserts {
                        if let Some(pty) = pred.ty {
                            if let Some(key) = self.ref_key_of_pub(arg) {
                                if let Some(cur) = self.current_type_of_key(arg, &key) {
                                    let narrowed = self.narrow_to_pred(cur, pty, sense);
                                    if sense {
                                        self.set_fact_for_definite_assignment(&key);
                                    }
                                    self.set_fact(key, narrowed);
                                }
                            }
                        }
                    }
                }
            }
            Expr::Binary {
                op: BinOp::AmpAmp,
                left,
                right,
                ..
            } if sense => {
                self.narrow_by_condition(left, true);
                self.narrow_by_condition(right, true);
            }
            Expr::Binary {
                op: BinOp::AmpAmp,
                left,
                right,
                ..
            } => {
                if same_guard_expr(left, right) {
                    return;
                }
                let left_false = self.collect_narrowing_facts(|this| {
                    this.narrow_by_condition(left, false);
                });
                let right_false = self.collect_narrowing_facts(|this| {
                    this.narrow_by_condition(left, true);
                    this.narrow_by_condition(right, false);
                });
                self.merge_narrowing_alternatives(left_false, right_false);
            }
            Expr::Binary {
                op: BinOp::BarBar,
                left,
                right,
                ..
            } if sense => {
                let left_true = self.collect_narrowing_facts(|this| {
                    this.narrow_by_condition(left, true);
                });
                let right_true = self.collect_narrowing_facts(|this| {
                    this.narrow_by_condition(left, false);
                    this.narrow_by_condition(right, true);
                });
                self.merge_narrowing_alternatives(left_true, right_true);
            }
            Expr::Binary {
                op: BinOp::BarBar,
                left,
                right,
                ..
            } if !sense => {
                if same_guard_expr(left, right) {
                    return;
                }
                self.narrow_by_condition(left, false);
                self.narrow_by_condition(right, false);
            }
            Expr::Binary {
                op, left, right, ..
            } if matches!(
                op,
                BinOp::EqEqEq | BinOp::NotEqEq | BinOp::EqEq | BinOp::NotEq
            ) =>
            {
                let eq_sense = if matches!(op, BinOp::EqEqEq | BinOp::EqEq) {
                    sense
                } else {
                    !sense
                };
                let loose = matches!(op, BinOp::EqEq | BinOp::NotEq);
                // typeof x === "..."
                if let Some((target, lit)) = typeof_comparison(left, right) {
                    self.narrow_by_typeof(target, &lit, eq_sense);
                    return;
                }
                // x === null/undefined/literal
                if let Some((target, value)) = literal_comparison(left, right) {
                    self.narrow_by_equality(target, value, eq_sense, loose);
                    return;
                }
            }
            Expr::Binary {
                op: BinOp::Instanceof,
                left,
                right,
                ..
            } => {
                if let Some(key) = self.ref_key_of_pub(left) {
                    if let Expr::Ident(rid) = &**right {
                        if let Some(rsym) = self.lookup_value(self.current_scope, &rid.name) {
                            let rsym = self.resolve_alias_chain(rsym);
                            let inst = if self.symbol(rsym).flags & flags::CLASS != 0 {
                                Some(self.types.intern_kind(TypeKind::Iface(rsym)))
                            } else {
                                // a non-class constructor value (e.g. the global
                                // `Error`): narrow to the type it constructs.
                                let ctor_t = self.type_of_symbol(rsym);
                                self.instance_type_from_constructor(ctor_t)
                            };
                            if let Some(inst) = inst {
                                let Some(cur) = self.current_type_of_key(left, &key) else {
                                    return;
                                };
                                let narrowed = if sense {
                                    let members = self.types.union_members(cur);
                                    let kept: Vec<TypeId> = members
                                        .into_iter()
                                        .filter(|&m| self.is_assignable_to(m, inst))
                                        .collect();
                                    if kept.is_empty() {
                                        inst
                                    } else {
                                        self.types.union(kept)
                                    }
                                } else {
                                    self.types.filter_union(cur, |_tt, m| {
                                        // keep members NOT assignable to inst
                                        m != inst
                                    })
                                };
                                if sense {
                                    self.set_fact_for_definite_assignment(&key);
                                }
                                self.set_fact(key, narrowed);
                            }
                        }
                    }
                }
            }
            Expr::Binary {
                op: BinOp::In,
                left,
                right,
                ..
            } => {
                if let (Expr::StrLit { value, .. }, Some(key)) =
                    (&**left, self.ref_key_of_pub(right))
                {
                    let Some(cur) = self.current_type_of_key(right, &key) else {
                        return;
                    };
                    let members = self.types.union_members(cur);
                    let kept: Vec<TypeId> = members
                        .into_iter()
                        .filter(|&m| {
                            let has = self
                                .prop_of_type(m, value.to_str_lossy().as_ref())
                                .is_some();
                            if sense {
                                has
                            } else {
                                !has
                            }
                        })
                        .collect();
                    let narrowed = self.types.union(kept);
                    if sense {
                        self.set_fact_for_definite_assignment(&key);
                    }
                    self.set_fact(key, narrowed);
                }
            }
            // truthiness on a reference (incl. optional chains: `if (o?.a)`)
            _ => {
                // `const c = <type-guard>; if (c) { ... }`: narrow the references
                // the aliased expression would, then fall through to truthiness.
                if let Expr::Ident(_) = cond {
                    if let Some(k) = self.ref_key_of_pub(cond) {
                        if k.1.is_empty() {
                            if let Some(alias) = self.cond_aliases.get(&k.0).copied() {
                                self.narrow_by_condition(alias, sense);
                            }
                        }
                    }
                }
                let key = self.ref_key_of_pub(cond).or_else(|| {
                    if sense {
                        self.ref_key_optional(cond)
                    } else {
                        None
                    }
                });
                if let Some(key) = key {
                    if sense {
                        // every prefix of the path is non-nullish and truthy at the leaf
                        for plen in 0..key.1.len() {
                            let prefix = RefKey(key.0, key.1[..plen].to_vec());
                            if let Some(cur) = self.current_type_of_key(cond, &prefix) {
                                let nn = self.non_nullable(cur);
                                self.set_fact_for_definite_assignment(&prefix);
                                self.set_fact(prefix, nn);
                            }
                        }
                    }
                    let Some(cur) = self.current_type_of_key(cond, &key) else {
                        return;
                    };
                    let narrowed = if sense {
                        self.truthy_part(cur)
                    } else {
                        self.falsy_part(cur)
                    };
                    if sense {
                        self.set_fact_for_definite_assignment(&key);
                    }
                    self.set_fact(key, narrowed);
                }
            }
        }
    }

    fn narrow_by_typeof(&mut self, target: &'a Expr, lit: &str, sense: bool) {
        let Some(key) = self.ref_key_of_pub(target) else {
            return;
        };
        let Some(cur) = self.current_type_of_key(target, &key) else {
            return;
        };
        let narrowed = if sense {
            self.typeof_filter(cur, lit)
        } else {
            self.typeof_filter_negative(cur, lit)
        };
        if sense {
            self.set_fact_for_definite_assignment(&key);
        }
        self.set_fact(key, narrowed);
    }

    fn typeof_matches(&mut self, m: TypeId, lit: &str) -> bool {
        match (self.types.kind(m).clone(), lit) {
            (TypeKind::String | TypeKind::StrLit(_), "string") => true,
            (TypeKind::Number | TypeKind::NumLit(_), "number") => true,
            (TypeKind::Bigint | TypeKind::BigIntLit(_), "bigint") => true,
            (TypeKind::BoolLit(_), "boolean") => true,
            (TypeKind::EsSymbol, "symbol") => true,
            (TypeKind::Undefined | TypeKind::Void, "undefined") => true,
            (TypeKind::Null, "object") => true,
            (TypeKind::NonPrimitive, "object") => true,
            (TypeKind::Anon(sid), l) => {
                let sh = self.types.shape(sid).clone();
                let is_fn = !sh.call_sigs.is_empty() || !sh.ctor_sigs.is_empty();
                (is_fn && l == "function") || (!is_fn && l == "object")
            }
            (
                TypeKind::Iface(_)
                | TypeKind::Ref(..)
                | TypeKind::Tuple(_)
                | TypeKind::ReadonlyArray(_),
                "object",
            ) => true,
            (TypeKind::ClassStatics(_) | TypeKind::MappedClassStatics(_, _), "function") => true,
            _ => false,
        }
    }

    fn typeof_filter(&mut self, t: TypeId, lit: &str) -> TypeId {
        // unknown / any: produce the primitive directly
        if matches!(self.types.kind(t), TypeKind::Unknown | TypeKind::Any) {
            return match lit {
                "string" => self.types.string,
                "number" => self.types.number,
                "bigint" => self.types.bigint,
                "boolean" => self.types.boolean,
                "symbol" => self.types.es_symbol,
                "undefined" => self.types.undefined,
                "function" => self
                    .global_type_symbol("Function")
                    .map(|s| self.types.intern_kind(TypeKind::Iface(s)))
                    .unwrap_or(t),
                "object" => {
                    let np = self.types.non_primitive;
                    let nl = self.types.null;
                    self.types.union(vec![np, nl])
                }
                _ => t,
            };
        }
        let members = self.types.union_members(t);
        let kept: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| self.typeof_matches(m, lit))
            .collect();
        self.types.union(kept)
    }

    fn typeof_filter_negative(&mut self, t: TypeId, lit: &str) -> TypeId {
        if matches!(self.types.kind(t), TypeKind::Unknown | TypeKind::Any) {
            return t;
        }
        let members = self.types.union_members(t);
        let kept: Vec<TypeId> = members
            .into_iter()
            .filter(|&m| !self.typeof_matches(m, lit))
            .collect();
        self.types.union(kept)
    }

    fn narrow_by_equality(
        &mut self,
        target: &'a Expr,
        value: NarrowValue,
        sense: bool,
        loose: bool,
    ) {
        // discriminant: x.prop === "lit"
        let Some(key) = self.ref_key_of_pub(target) else {
            return;
        };
        // For property paths, narrow the ROOT by discriminant when possible
        if !key.1.is_empty() {
            if let NarrowValue::Str(ref s) = value {
                let root = RefKey(key.0, key.1[..key.1.len() - 1].to_vec());
                let prop = key.1.last().unwrap().clone();
                let root_expr_t = if root.1.is_empty() {
                    self.fact_for(&root)
                        .or_else(|| Some(self.type_of_symbol(root.0)))
                } else {
                    None
                };
                if let Some(rt) = root_expr_t {
                    if let TypeKind::Union(_) = self.types.kind(rt) {
                        let lit_t = self.types.string_lit(s);
                        let members = self.types.union_members(rt);
                        let kept: Vec<TypeId> = members
                            .into_iter()
                            .filter(|&m| {
                                let pt = self.prop_of_type(m, &prop);
                                match pt {
                                    Some(pt) => {
                                        let includes = self.is_assignable_to(lit_t, pt);
                                        if sense {
                                            includes
                                        } else {
                                            !(pt == lit_t)
                                        }
                                    }
                                    None => !sense,
                                }
                            })
                            .collect();
                        let narrowed = self.types.union(kept);
                        self.set_fact(root, narrowed);
                        return;
                    }
                }
            }
        }
        let Some(cur) = self.current_type_of_key(target, &key) else {
            return;
        };
        let narrowed = match (&value, sense, loose) {
            (NarrowValue::Null, true, false) => self
                .types
                .filter_union(cur, |tt, m| matches!(tt.kind(m), TypeKind::Null)),
            (NarrowValue::Null, false, false) => self
                .types
                .filter_union(cur, |tt, m| !matches!(tt.kind(m), TypeKind::Null)),
            (NarrowValue::Null, true, true) | (NarrowValue::Undefined, true, true) => {
                self.types.filter_union(cur, |tt, m| {
                    matches!(tt.kind(m), TypeKind::Null | TypeKind::Undefined)
                })
            }
            (NarrowValue::Null, false, true) | (NarrowValue::Undefined, false, true) => {
                self.types.filter_union(cur, |tt, m| {
                    !matches!(tt.kind(m), TypeKind::Null | TypeKind::Undefined)
                })
            }
            (NarrowValue::Undefined, true, false) => self
                .types
                .filter_union(cur, |tt, m| matches!(tt.kind(m), TypeKind::Undefined)),
            (NarrowValue::Undefined, false, false) => self
                .types
                .filter_union(cur, |tt, m| !matches!(tt.kind(m), TypeKind::Undefined)),
            (NarrowValue::Str(s), true, _) => {
                let lit = self.types.string_lit(s);
                let members = self.types.union_members(cur);
                if members.contains(&lit) {
                    lit
                } else if matches!(self.types.kind(cur), TypeKind::String) {
                    lit
                } else {
                    // keep members that could equal the literal (e.g. `string`
                    // stays, other primitives drop): `string | number === "s"`
                    // narrows to string, like tsc.
                    let kept: Vec<TypeId> = members
                        .iter()
                        .copied()
                        .filter(|&m| self.is_assignable_to(lit, m) || self.is_assignable_to(m, lit))
                        .collect();
                    if kept.is_empty() {
                        cur
                    } else {
                        self.types.union(kept)
                    }
                }
            }
            (NarrowValue::Str(s), false, _) => {
                let lit = self.types.string_lit(s);
                // remove every constituent that is the literal *or a subtype of
                // it* (`'right' & { right: 'right' }` is `<: 'right'`), so an
                // exhausted `else` collapses to `never`.
                let members = self.types.union_members(cur);
                let mut kept: Vec<TypeId> = Vec::new();
                for m in members {
                    if !self.is_assignable_to(m, lit) {
                        kept.push(m);
                    }
                }
                if kept.is_empty() {
                    self.types.never
                } else {
                    self.types.union(kept)
                }
            }
            (NarrowValue::Num(v), true, _) => {
                let lit = self.types.number_lit(*v);
                let members = self.types.union_members(cur);
                if members.contains(&lit) {
                    lit
                } else if matches!(self.types.kind(cur), TypeKind::Number) {
                    lit
                } else {
                    let kept: Vec<TypeId> = members
                        .iter()
                        .copied()
                        .filter(|&m| self.is_assignable_to(lit, m) || self.is_assignable_to(m, lit))
                        .collect();
                    if kept.is_empty() {
                        cur
                    } else {
                        self.types.union(kept)
                    }
                }
            }
            (NarrowValue::Num(v), false, _) => {
                let lit = self.types.number_lit(*v);
                let members = self.types.union_members(cur);
                let mut kept: Vec<TypeId> = Vec::new();
                for m in members {
                    if !self.is_assignable_to(m, lit) {
                        kept.push(m);
                    }
                }
                if kept.is_empty() {
                    self.types.never
                } else {
                    self.types.union(kept)
                }
            }
            (NarrowValue::Bool(b), true, _) => {
                let lit = if *b {
                    self.types.true_t
                } else {
                    self.types.false_t
                };
                let members = self.types.union_members(cur);
                if members.contains(&lit) {
                    lit
                } else {
                    cur
                }
            }
            (NarrowValue::Bool(b), false, _) => {
                let lit = if *b {
                    self.types.true_t
                } else {
                    self.types.false_t
                };
                self.types.filter_union(cur, |_tt, m| m != lit)
            }
        };
        self.set_fact(key, narrowed);
    }

    pub(crate) fn narrow_switch_case(&mut self, expr: &'a Expr, test: &'a Expr) {
        let value = match test {
            Expr::StrLit { value, .. } => NarrowValue::Str(value.to_str_lossy().into_owned()),
            Expr::NumLit { value, .. } => NarrowValue::Num(*value),
            Expr::BoolLit { value, .. } => NarrowValue::Bool(*value),
            Expr::NullLit { .. } => NarrowValue::Null,
            Expr::Ident(id) if id.name == "undefined" => NarrowValue::Undefined,
            _ => return,
        };
        self.narrow_by_equality(expr, value, true, false);
    }

    /// remove a case label's value from the discriminant (for the default clause)
    pub(crate) fn narrow_switch_case_negative(&mut self, expr: &'a Expr, test: &'a Expr) {
        let value = match test {
            Expr::StrLit { value, .. } => NarrowValue::Str(value.to_str_lossy().into_owned()),
            Expr::NumLit { value, .. } => NarrowValue::Num(*value),
            Expr::BoolLit { value, .. } => NarrowValue::Bool(*value),
            Expr::NullLit { .. } => NarrowValue::Null,
            Expr::Ident(id) if id.name == "undefined" => NarrowValue::Undefined,
            _ => return,
        };
        self.narrow_by_equality(expr, value, false, false);
    }
}

pub enum NarrowValue {
    Null,
    Undefined,
    Str(String),
    Num(f64),
    Bool(bool),
}

fn typeof_comparison<'b>(left: &'b Expr, right: &'b Expr) -> Option<(&'b Expr, String)> {
    let (t, lit) = match (left, right) {
        (
            Expr::Unary {
                op: UnaryOp::Typeof,
                operand,
                ..
            },
            Expr::StrLit { value, .. },
        ) => (operand, value),
        (
            Expr::StrLit { value, .. },
            Expr::Unary {
                op: UnaryOp::Typeof,
                operand,
                ..
            },
        ) => (operand, value),
        _ => return None,
    };
    Some((t, lit.to_str_lossy().into_owned()))
}

fn same_guard_expr(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Paren { inner: l, .. }, r) => same_guard_expr(l, r),
        (l, Expr::Paren { inner: r, .. }) => same_guard_expr(l, r),
        (
            Expr::Binary {
                op: lop,
                left: ll,
                right: lr,
                ..
            },
            Expr::Binary {
                op: rop,
                left: rl,
                right: rr,
                ..
            },
        ) if lop == rop => {
            if let (Some((lt, llit)), Some((rt, rlit))) =
                (typeof_comparison(ll, lr), typeof_comparison(rl, rr))
            {
                return llit == rlit && same_ref_expr(lt, rt);
            }
            false
        }
        _ => same_ref_expr(left, right),
    }
}

fn same_ref_expr(left: &Expr, right: &Expr) -> bool {
    match (left, right) {
        (Expr::Paren { inner: l, .. }, r) => same_ref_expr(l, r),
        (l, Expr::Paren { inner: r, .. }) => same_ref_expr(l, r),
        (Expr::Ident(l), Expr::Ident(r)) => l.name == r.name,
        (Expr::This { .. }, Expr::This { .. }) => true,
        (
            Expr::PropAccess {
                obj: lo, name: ln, ..
            },
            Expr::PropAccess {
                obj: ro, name: rn, ..
            },
        ) => ln.name == rn.name && same_ref_expr(lo, ro),
        _ => false,
    }
}

fn literal_comparison<'b>(left: &'b Expr, right: &'b Expr) -> Option<(&'b Expr, NarrowValue)> {
    fn value_of(e: &Expr) -> Option<NarrowValue> {
        match e {
            Expr::NullLit { .. } => Some(NarrowValue::Null),
            Expr::Ident(id) if id.name == "undefined" => Some(NarrowValue::Undefined),
            Expr::StrLit { value, .. } => Some(NarrowValue::Str(value.to_str_lossy().into_owned())),
            Expr::NumLit { value, .. } => Some(NarrowValue::Num(*value)),
            Expr::BoolLit { value, .. } => Some(NarrowValue::Bool(*value)),
            _ => None,
        }
    }
    if let Some(v) = value_of(right) {
        return Some((left, v));
    }
    if let Some(v) = value_of(left) {
        return Some((right, v));
    }
    None
}
