//! Tier-2 flow-graph type resolver (Stage 0 dark launch).
//!
//! `get_flow_type_of_reference` answers "what is the type of reference `key`
//! at flow node `flow`" by walking the bind-time flow graph backward toward
//! `Start`, reusing `narrow_by_condition` as the condition narrower (seeded
//! into a scratch fact frame, read back, popped). It is the future
//! replacement for the lexical fact stack (`fact_for`); during Stage 0 it is
//! only exercised under `TSRS_FLOW_VERIFY`, which computes BOTH results at
//! the fact-stack read seams and tallies agreement — program output is
//! unchanged.
//!
//! The walk mirrors the fact stack's semantics on purpose (assignment
//! narrowing per `operators.rs`, declarator narrowing per `stmts.rs`,
//! root-invalidation on same-root assignment) so that dark-launch mismatches
//! isolate real flow-graph wins/bugs instead of known modeling differences.

use crate::ast::*;
use crate::binder::{flags, FlowNode, FlowNodeId, ScopeId, SymbolId};
use crate::checker::exprs::node_key_expr;
use crate::checker::{Checker, RefKey};
use crate::types::{TypeId, TypeKind};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Dark-launch tallies (process-wide; printed by `main` when
/// `TSRS_FLOW_VERIFY` is set).
pub static FLOW_VERIFY_MATCH: AtomicUsize = AtomicUsize::new(0);
/// TypeIds differ but the displayed types are identical (interning
/// duplicates, e.g. `NonNullable<T>` built at two sites) — agreement.
pub static FLOW_VERIFY_DISPLAY_MATCH: AtomicUsize = AtomicUsize::new(0);
pub static FLOW_VERIFY_MISMATCH: AtomicUsize = AtomicUsize::new(0);
pub static FLOW_VERIFY_UNRESOLVED: AtomicUsize = AtomicUsize::new(0);
pub static FLOW_VERIFY_NO_NODE: AtomicUsize = AtomicUsize::new(0);

fn verbose() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("TSRS_FLOW_VERIFY").is_ok_and(|v| v == "v" || v == "verbose")
    })
}

/// Outcome of resolving one (ref, flow) pair. `Cycle` marks a loop back-edge
/// hit while its own resolution is in progress — a `Branch` skips such
/// antecedents (tsc's "incomplete" result); anything else propagates it.
#[derive(Clone, Copy, PartialEq, Debug)]
enum FlowRes {
    Ty(TypeId),
    Cycle,
    /// no control path reaches this point (post-return/throw/break/continue);
    /// joins skip it just like `Cycle`
    Dead,
    Unknown,
}

/// A borrow-free copy of one flow node's payload (the `&'a` fields are `Copy`,
/// so extracting them releases the borrow of `bind.flow_nodes`).
enum Step<'a> {
    Start(Option<(FlowNodeId, Span)>, Option<Span>),
    Dead,
    Branch(Vec<FlowNodeId>),
    Cond(&'a Expr, bool, ScopeId, FlowNodeId),
    Assign(&'a Expr, &'a Expr, ScopeId, FlowNodeId),
    Init(&'a VarDeclarator, ScopeId, FlowNodeId),
    Switch(&'a Expr, &'a [SwitchCase], u32, usize, ScopeId, FlowNodeId),
    Call(&'a Expr, ScopeId, FlowNodeId),
    Nullish(&'a Expr, bool, ScopeId, FlowNodeId),
}

impl<'a> Checker<'a> {
    /// The declared (un-narrowed) type of a narrowable reference: the root
    /// symbol's type walked along the property path. This is
    /// `current_type_of_key` minus the `fact_for` lookups.
    pub(crate) fn declared_type_of_ref(&mut self, key: &RefKey) -> Option<TypeId> {
        let mut t = if Some(key.0) == self.fresolve.this_sym {
            // the synthetic `this` root of a seeded 2564/2565 query: the
            // declaring class's instance type (the symbol itself is a type
            // parameter with no value declaration)
            let owner = self.this_param_owner(key.0)?;
            self.types.intern_kind(TypeKind::Iface(owner))
        } else {
            self.type_of_symbol(key.0)
        };
        for part in &key.1 {
            t = self.prop_of_type(t, part)?;
        }
        Some(t)
    }

    /// Resolve `key`'s flow type at `flow`. `None` = the graph/types cannot
    /// answer (caller falls back to the declared type).
    pub(crate) fn get_flow_type_of_reference(
        &mut self,
        key: &RefKey,
        flow: FlowNodeId,
    ) -> Option<TypeId> {
        self.fresolve.memo.clear();
        self.fresolve.initial = None;
        let r = self.flow_type_at(key, flow, 0);
        self.fresolve.in_progress.clear();
        match r {
            FlowRes::Ty(t) => Some(t),
            _ => None,
        }
    }

    /// Resolve `key`'s flow type at `flow` with a definite-assignment seed
    /// (tsc getFlowTypeOfReference's `initialType` parameter): paths that
    /// reach the declaring container's `Start` without an assignment
    /// contribute `initial` instead of the declared type (see
    /// `FlowResolve::initial` for the outer-variable rule). The memo is
    /// per-query, so the seeded result never leaks into unseeded queries.
    pub(crate) fn get_flow_type_of_reference_seeded(
        &mut self,
        key: &RefKey,
        flow: FlowNodeId,
        initial: TypeId,
        decl_span: Span,
        never_initialized: bool,
    ) -> Option<TypeId> {
        self.fresolve.memo.clear();
        self.fresolve.initial = Some((key.clone(), initial, decl_span, never_initialized));
        let r = self.flow_type_at(key, flow, 0);
        self.fresolve.initial = None;
        self.fresolve.in_progress.clear();
        match r {
            FlowRes::Ty(t) => Some(t),
            _ => None,
        }
    }

    /// Does the (possibly union) type have an `undefined` constituent?
    /// Structural scan — `void` deliberately does NOT count (tsc
    /// containsUndefinedType checks the Undefined flag; `let x: T | void`
    /// stays a definite-assignment candidate).
    pub(crate) fn contains_undefined_member(&self, t: TypeId) -> bool {
        self.types
            .union_members(t)
            .iter()
            .any(|&m| matches!(self.types.kind(m), TypeKind::Undefined))
    }

    fn flow_type_at(&mut self, key: &RefKey, flow: FlowNodeId, depth: u32) -> FlowRes {
        // stack guard only — memoization already bounds total work; frames
        // here are small (the narrowing scaffolds run after the recursive
        // call returns). 2000 matches tsc's flow-analysis budget, so long
        // straight-line bodies resolve instead of silently going Unknown.
        if depth > 2000 {
            return FlowRes::Unknown;
        }
        let mk = (key.clone(), flow);
        if let Some(m) = self.fresolve.memo.get(&mk) {
            return match m {
                Some(t) => FlowRes::Ty(*t),
                None => FlowRes::Unknown,
            };
        }
        if !self.fresolve.in_progress.insert(mk.clone()) {
            return FlowRes::Cycle;
        }
        let r = self.flow_step(key, flow, depth);
        self.fresolve.in_progress.remove(&mk);
        match r {
            FlowRes::Ty(t) => {
                self.fresolve.memo.insert(mk, Some(t));
            }
            FlowRes::Unknown => {
                self.fresolve.memo.insert(mk, None);
            }
            // Cycle is context-dependent (not memoizable); Dead is terminal
            // and cheap to recompute
            FlowRes::Cycle | FlowRes::Dead => {}
        }
        r
    }

    fn flow_step(&mut self, key: &RefKey, flow: FlowNodeId, depth: u32) -> FlowRes {
        let step = match &self.bind.flow_nodes[flow.0 as usize] {
            FlowNode::Start { outer, cspan } => Step::Start(*outer, *cspan),
            FlowNode::Unreachable => Step::Dead,
            FlowNode::Branch(antes) => Step::Branch(antes.clone()),
            FlowNode::Cond {
                cond,
                sense,
                scope,
                ante,
            } => Step::Cond(cond, *sense, *scope, *ante),
            FlowNode::Assign {
                target,
                expr,
                scope,
                ante,
            } => Step::Assign(target, expr, *scope, *ante),
            FlowNode::Init { decl, scope, ante } => Step::Init(decl, *scope, *ante),
            FlowNode::Switch {
                disc,
                cases,
                clause,
                stmt_key,
                scope,
                ante,
            } => Step::Switch(disc, cases, *clause, *stmt_key, *scope, *ante),
            FlowNode::Call { call, scope, ante } => Step::Call(call, *scope, *ante),
            FlowNode::Nullish {
                expr,
                sense,
                scope,
                ante,
            } => Step::Nullish(expr, *sense, *scope, *ante),
        };
        match step {
            Step::Start(outer, cspan) => {
                // tsc extends a bare const reference's flow analysis past
                // function-expression/arrow/method containers it is captured
                // by (checkIdentifier's flowContainer loop): resume the walk
                // in the enclosing flow instead of stopping at the entry.
                if let Some((oflow, fspan)) = outer {
                    if self.const_ref_escapes(key, fspan) {
                        return self.flow_type_at(key, oflow, depth + 1);
                    }
                }
                // seeded (definite-assignment) query: the queried reference
                // reads its initial type at its DECLARING container's entry
                // (tsc getTypeAtFlowNode's Start arm consuming initialType).
                // At a foreign container's entry the variable is outer —
                // tsc assumes it initialized (declared type) — unless it is
                // never-initialized, which tsc checks even across closures.
                if let Some((ik, it, dspan, never_init)) = &self.fresolve.initial {
                    if ik == key {
                        let declares_here = cspan
                            .is_none_or(|cs| cs.start <= dspan.start && dspan.end <= cs.end);
                        if declares_here || *never_init {
                            return FlowRes::Ty(*it);
                        }
                    }
                }
                match self.declared_type_of_ref(key) {
                    Some(t) => FlowRes::Ty(t),
                    None => FlowRes::Unknown,
                }
            }
            Step::Dead => FlowRes::Dead,
            Step::Branch(antes) => {
                let declared = self.declared_type_of_ref(key);
                // tsc takes the declared-type subsumption shortcut only when
                // declaredType === initialType: a seeded (definite-
                // assignment) walk must keep the unassigned edges alive, or
                // `if (c) x = 1; x;` would resolve to the assigned edge's
                // declared type and swallow the seed.
                let subsume = match &self.fresolve.initial {
                    Some((ik, it, ..)) if ik == key => Some(*it) == declared,
                    _ => true,
                };
                let mut tys: Vec<TypeId> = Vec::new();
                let mut any_cycle = false;
                let mut any_dead = false;
                for a in antes {
                    match self.flow_type_at(key, a, depth + 1) {
                        FlowRes::Ty(t) => {
                            // tsc getTypeAtFlowBranchLabel/LoopLabel: an
                            // antecedent that already equals the declared
                            // type subsumes every other path (all flow types
                            // are its subtypes), so stop here — this is also
                            // what keeps `A | C`-style unions collapsed to
                            // `A` after an `if (isC(a)) {...}` join
                            if subsume && Some(t) == declared {
                                return FlowRes::Ty(t);
                            }
                            if !tys.contains(&t) {
                                tys.push(t);
                            }
                        }
                        FlowRes::Cycle => any_cycle = true,
                        FlowRes::Dead => any_dead = true,
                        FlowRes::Unknown => return FlowRes::Unknown,
                    }
                }
                match tys.len() {
                    0 if any_cycle => FlowRes::Cycle,
                    0 if any_dead => FlowRes::Dead,
                    // a join no edge reaches (e.g. the post-label of
                    // `while (true) {}` with no breaks): nothing to say
                    0 => FlowRes::Unknown,
                    1 => FlowRes::Ty(tys[0]),
                    _ => {
                        let u = self.types.union(tys);
                        // tsc getUnionOrEvolvingArrayType recombines the
                        // decomposed unknown (`{} | null | undefined`) back
                        // to `unknown` at joins
                        FlowRes::Ty(self.recombine_unknown(u, declared))
                    }
                }
            }
            Step::Cond(cond, sense, scope, ante) => {
                let t_in = match self.flow_type_at(key, ante, depth + 1) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                // (The former `any`/type-param guard on negative edges is
                // gone: the fact helpers now implement tsc getTypeWithFacts,
                // which keeps both whole on falsy edges by itself.)
                // Reuse the existing condition narrower: seed a scratch fact
                // frame with the antecedent type, run it, read the result
                // back, pop. Names in `cond` resolve in ITS scope; definite-
                // assignment recording stays off; any diagnostics a lazy
                // resolution emits along the way are rolled back.
                let saved_scope = self.current_scope;
                self.current_scope = scope;
                self.fresolve.quiet += 1;
                let dlen = self.diags.len();
                let out = self.narrowed(|c| {
                    c.set_fact(key.clone(), t_in);
                    c.narrow_by_condition(cond, sense);
                    c.fact_for(key).unwrap_or(t_in)
                });
                self.diags.truncate(dlen);
                self.fresolve.quiet -= 1;
                self.current_scope = saved_scope;
                if verbose() {
                    let name = self.symbol(key.0).name.clone();
                    eprintln!(
                        "FLOW_VERIFY debug: Cond sense={} at {} key={}{} {} -> {}",
                        sense,
                        cond.span().start,
                        name,
                        if key.1.is_empty() {
                            String::new()
                        } else {
                            format!(".{}", key.1.join("."))
                        },
                        self.display_type(t_in),
                        self.display_type(out)
                    );
                }
                FlowRes::Ty(out)
            }
            Step::Switch(disc, cases, clause, stmt_key, scope, ante) => {
                // the implicit no-match path past an EXHAUSTIVE switch
                // contributes nothing to joins (tsc getTypeAtFlowBranchLabel
                // defers it as `bypassFlow` and drops it when exhaustive);
                // `never` reproduces that exactly — unions absorb it, and
                // when every clause returned it is the join's only
                // antecedent, matching tsc's empty-union = never (the
                // assertNever idiom). The set is populated when the switch
                // statement itself is checked — the same source order the
                // fact path relied on.
                if clause as usize == cases.len()
                    && self.flow.exhaustive_switches.contains(&stmt_key)
                {
                    return FlowRes::Ty(self.types.never);
                }
                let t_in = match self.flow_type_at(key, ante, depth + 1) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                // Same scaffold as `Cond`, reusing the fact-stack's switch
                // narrowers: a matched clause narrows `disc === label`; a
                // default clause (or the implicit no-match path) narrows by
                // the negation of every label.
                let saved_scope = self.current_scope;
                self.current_scope = scope;
                self.fresolve.quiet += 1;
                let dlen = self.diags.len();
                let out = self.narrowed(|c| {
                    c.set_fact(key.clone(), t_in);
                    match cases.get(clause as usize).and_then(|cl| cl.test.as_ref()) {
                        Some(test) => c.narrow_switch_case(disc, test),
                        None => {
                            for cl in cases {
                                if let Some(test) = &cl.test {
                                    c.narrow_switch_case_negative(disc, test);
                                }
                            }
                        }
                    }
                    c.fact_for(key).unwrap_or(t_in)
                });
                self.diags.truncate(dlen);
                self.fresolve.quiet -= 1;
                self.current_scope = saved_scope;
                if verbose() && out == self.types.never && t_in != self.types.never {
                    eprintln!(
                        "FLOW_VERIFY debug: Switch clause={} narrowed {} -> never",
                        clause,
                        self.display_type(t_in)
                    );
                }
                FlowRes::Ty(out)
            }
            Step::Nullish(expr, sense, scope, ante) => {
                // `a ?? b` (tsc narrowTypeByOptionality): the non-nullish
                // skip edge keeps the NEUndefinedOrNull facts (with the
                // strict NonNullable<> adjustment), the nullish RHS edge the
                // EQUndefinedOrNull ones. Only a leaf reference match narrows.
                let t_in = match self.flow_type_at(key, ante, depth + 1) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                if self.ref_key_in_scope(expr, scope) != Some(key.clone()) {
                    return FlowRes::Ty(t_in);
                }
                let out = if sense {
                    self.facts_filter(
                        t_in,
                        crate::checker::operators::facts::NE_UNDEFINED_OR_NULL,
                        true,
                    )
                } else {
                    self.facts_filter(
                        t_in,
                        crate::checker::operators::facts::EQ_UNDEFINED_OR_NULL,
                        false,
                    )
                };
                FlowRes::Ty(out)
            }
            Step::Call(call, scope, ante) => {
                // Mirror the fact path, which applies `apply_assertion_narrowing`
                // after checking EVERY call (`check_expr`'s Call arm): seed the
                // antecedent type, re-run it, read the fact back. Calls whose
                // callee cannot carry a predicate pass through untouched.
                let Expr::Call { callee, args, .. } = call else {
                    return self.flow_type_at(key, ante, depth + 1);
                };
                if !callee_may_assert(callee) {
                    return self.flow_type_at(key, ante, depth + 1);
                }
                let t_in = match self.flow_type_at(key, ante, depth + 1) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                let saved_scope = self.current_scope;
                self.current_scope = scope;
                self.fresolve.quiet += 1;
                let dlen = self.diags.len();
                let out = self.narrowed(|c| {
                    c.set_fact(key.clone(), t_in);
                    c.apply_assertion_narrowing(callee, args);
                    c.fact_for(key).unwrap_or(t_in)
                });
                self.diags.truncate(dlen);
                self.fresolve.quiet -= 1;
                self.current_scope = saved_scope;
                FlowRes::Ty(out)
            }
            Step::Assign(target, expr, scope, ante) => {
                // only variables (var/let/const/params/catch) are flow-
                // narrowed by assignments; a bogus assignment to an enum
                // object / namespace / class (`E = null`) is an error and
                // must not make downstream reads of `E` see `null`. The
                // synthetic `this` root of a seeded 2564/2565 query is
                // narrowable by construction.
                if Some(key.0) != self.fresolve.this_sym
                    && self.symbol(key.0).flags
                        & (flags::FUNCTION_SCOPED_VARIABLE | flags::BLOCK_SCOPED_VARIABLE)
                        == 0
                {
                    return self.flow_type_at(key, ante, depth + 1);
                }
                match self.ref_key_in_scope(target, scope) {
                    Some(tk) if tk == *key => self.assigned_type(key, expr, scope, ante, depth),
                    // fact parity: an assignment through the same root
                    // invalidates every fact rooted at it — EXCEPT in a
                    // seeded (definite-assignment) walk: `b.func1 = …` does
                    // not assign `b`, so the walk must continue toward the
                    // entry (tsc keeps flowing past non-matching targets)
                    Some(tk) if tk.0 == key.0 => {
                        if matches!(&self.fresolve.initial, Some((ik, ..)) if ik == key) {
                            return self.flow_type_at(key, ante, depth + 1);
                        }
                        match self.declared_type_of_ref(key) {
                            Some(t) => FlowRes::Ty(t),
                            None => FlowRes::Unknown,
                        }
                    }
                    Some(_) => self.flow_type_at(key, ante, depth + 1),
                    None => match target {
                        // destructuring assignment: clobber only when one of
                        // the pattern's targets is rooted at `key`'s symbol
                        // (`check_reference_for_assignment` invalidates per
                        // element root)
                        Expr::Array { .. } | Expr::Object { .. }
                            if self.pattern_clobbers(target, scope, key.0) =>
                        {
                            match self.declared_type_of_ref(key) {
                                Some(t) => FlowRes::Ty(t),
                                None => FlowRes::Unknown,
                            }
                        }
                        // elem access / this-based targets: the fact stack
                        // never keys these, so they don't clear narrowings
                        _ => self.flow_type_at(key, ante, depth + 1),
                    },
                }
            }
            Step::Init(decl, scope, ante) => {
                if !self.decl_declares(decl, key.0) {
                    return self.flow_type_at(key, ante, depth + 1);
                }
                let Some(declared) = self.declared_type_of_ref(key) else {
                    return FlowRes::Unknown;
                };
                if !key.1.is_empty() {
                    return FlowRes::Ty(declared);
                }
                // fact parity (stmts.rs declarator narrowing): a union-typed
                // `let/const x: A | B = init` narrows to the members the
                // initializer's value fits. ANNOTATED declarators only — an
                // inferred union (`const f = c ? g1 : g2`) stays whole (tsc
                // getAssignmentReducedType applies to declared unions).
                if decl.ty.is_none() {
                    return FlowRes::Ty(declared);
                }
                if let (Binding::Ident(_), Some(init)) = (&decl.name, &decl.init) {
                    if matches!(self.types.kind(declared), TypeKind::Union(_)) {
                        let it = match self.flow_expr_type(init, scope, ante, depth) {
                            FlowRes::Ty(t) => t,
                            _ => return FlowRes::Ty(declared),
                        };
                        let r = self.types.regular(it);
                        let is_const = self.symbol(key.0).flags & flags::CONST_VARIABLE != 0;
                        let val = if is_const {
                            r
                        } else {
                            self.types.widen_literal(r)
                        };
                        let nullish =
                            matches!(self.types.kind(val), TypeKind::Null | TypeKind::Undefined);
                        if !nullish && !self.types.is_error(val) {
                            let members = self.types.union_members(declared);
                            let kept: Vec<TypeId> = members
                                .into_iter()
                                .filter(|&m| self.is_assignable_to(val, m))
                                .collect();
                            if !kept.is_empty() {
                                return FlowRes::Ty(self.types.union(kept));
                            }
                        }
                    }
                }
                FlowRes::Ty(declared)
            }
        }
    }

    /// Post-assignment type of `key` when `expr` assigns exactly to it —
    /// mirrors the fact-stack updates in `operators.rs`.
    fn assigned_type(
        &mut self,
        key: &RefKey,
        expr: &'a Expr,
        scope: ScopeId,
        ante: FlowNodeId,
        depth: u32,
    ) -> FlowRes {
        let Some(declared) = self.declared_type_of_ref(key) else {
            return FlowRes::Unknown;
        };
        match expr {
            Expr::Binary {
                op: BinOp::Assign,
                right,
                ..
            } => {
                let rt = match self.flow_expr_type(right, scope, ante, depth) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                // an error-typed RHS (e.g. a failed overload inside a loop)
                // must not become the narrowed type — the fact path never
                // narrows to error because its live re-check differs
                if self.types.is_error(rt) {
                    return FlowRes::Ty(declared);
                }
                let narrowed = self.types.regular(rt);
                let widened = self.types.widen_literal(narrowed);
                if !self.is_global_object_type(declared)
                    && self.is_assignable_to(widened, declared)
                {
                    FlowRes::Ty(widened)
                } else {
                    FlowRes::Ty(declared)
                }
            }
            Expr::Binary {
                op:
                    op @ (BinOp::AmpAmpAssign | BinOp::BarBarAssign | BinOp::QuestionQuestionAssign),
                right,
                ..
            } => {
                let rt = match self.flow_expr_type(right, scope, ante, depth) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                if self.types.is_error(rt) {
                    return FlowRes::Ty(declared);
                }
                let cur = self.types.regular(declared);
                let kept = match op {
                    BinOp::QuestionQuestionAssign => self.non_nullable(cur),
                    BinOp::BarBarAssign => self.truthy_part(cur),
                    _ => self.falsy_part(cur),
                };
                let r0 = self.types.regular(rt);
                let r = self.types.widen_literal(r0);
                let narrowed = self.types.union(vec![kept, r]);
                if self.is_assignable_to(narrowed, declared) {
                    FlowRes::Ty(narrowed)
                } else {
                    FlowRes::Ty(declared)
                }
            }
            // `x++` / `x--` / compound assigns: tsc passes the ANTECEDENT
            // flow type through, widened to its literal base
            // (getTypeAtFlowAssignment's AssignmentKind::Compound arm) —
            // `any++` stays `any`, and an `i++` loop back edge resolves as
            // Cycle so the loop join keeps the entry edge's type instead of
            // collapsing to the declared `any`. (The fact stack only
            // invalidates here — reads seeing the declared type right after
            // a compound is its early-invalidation behavior.)
            _ => match self.flow_type_at(key, ante, depth + 1) {
                FlowRes::Ty(t) => {
                    let r = self.types.regular(t);
                    FlowRes::Ty(self.types.widen_literal(r))
                }
                other => other,
            },
        }
    }

    /// Type of an already-checked expression, for the resolver: the
    /// expression-type cache for everything the checker caches, the flow walk
    /// itself for bare identifiers (which the cache deliberately excludes).
    fn flow_expr_type(
        &mut self,
        e: &'a Expr,
        scope: ScopeId,
        flow: FlowNodeId,
        depth: u32,
    ) -> FlowRes {
        match e {
            Expr::Paren { inner, .. } => self.flow_expr_type(inner, scope, flow, depth),
            // `undefined` never resolves to a symbol (check_ident special-
            // cases it); an Unknown here would abort whole walks over
            // `x = undefined` back edges
            Expr::Ident(id) if id.name == "undefined" => FlowRes::Ty(self.types.undefined),
            Expr::Ident(_) => match self.ref_key_in_scope(e, scope) {
                Some(k) => self.flow_type_at(&k, flow, depth + 1),
                None => FlowRes::Unknown,
            },
            Expr::This { .. } => FlowRes::Unknown,
            _ => match self.caches.expr_type_cache.get(&node_key_expr(e)).copied() {
                Some(t) => FlowRes::Ty(t),
                None => FlowRes::Unknown,
            },
        }
    }

    /// `ref_key_of` with an explicit scope (the flow node's), so upstream
    /// expressions resolve their names where they appear, not where the
    /// resolver was invoked. `this` is not keyed (matches `ref_key_of`).
    pub(crate) fn ref_key_in_scope(&self, e: &Expr, scope: ScopeId) -> Option<RefKey> {
        match e {
            // `this` is keyed only during a this-seeded query (2564/2565);
            // Stage-1 narrowing reads never set this_sym
            Expr::This { .. } => self.fresolve.this_sym.map(|s| RefKey(s, Vec::new())),
            Expr::Ident(id) => self
                .lookup_value(scope, &id.name)
                .map(|s| RefKey(s, Vec::new())),
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let mut k = self.ref_key_in_scope(obj, scope)?;
                k.1.push(name.name.clone());
                Some(k)
            }
            Expr::Paren { inner, .. } => self.ref_key_in_scope(inner, scope),
            _ => None,
        }
    }

    /// tsc keeps narrowing for a bare `const` reference captured by a nested
    /// function expression (checkIdentifier extends `flowContainer` outward
    /// while the symbol is a const declared outside it): true when the walk
    /// should continue in the enclosing flow instead of stopping at `Start`.
    fn const_ref_escapes(&self, key: &RefKey, fspan: Span) -> bool {
        if !key.1.is_empty() {
            return false;
        }
        let sym = self.symbol(key.0);
        if sym.flags & flags::CONST_VARIABLE == 0 {
            return false;
        }
        // declared in another file ⇒ certainly outside this function; same
        // file ⇒ outside when no declaration lies within the span
        sym.file != self.current_file
            || !sym.decls.iter().any(|d| match d {
                crate::binder::Decl::Var(v, _) => {
                    fspan.start <= v.span.start && v.span.end <= fspan.end
                }
                crate::binder::Decl::Param(p) => {
                    fspan.start <= p.span.start && p.span.end <= fspan.end
                }
                _ => false,
            })
    }

    /// Does a destructuring-assignment pattern (`[a, b] = …` / `({x} = …)`)
    /// assign through `root`? Mirrors `check_reference_for_assignment`,
    /// which invalidates facts per element-target root.
    fn pattern_clobbers(&self, e: &Expr, scope: ScopeId, root: SymbolId) -> bool {
        match e {
            Expr::Array { elements, .. } => elements.iter().any(|el| {
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
                self.pattern_target_clobbers(tgt, scope, root)
            }),
            Expr::Object { props, .. } => props.iter().any(|p| match p {
                ObjectProp::Shorthand { name, .. } => {
                    self.lookup_value(scope, &name.name) == Some(root)
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
                    self.pattern_target_clobbers(tgt, scope, root)
                }
                ObjectProp::Spread { expr, .. } => {
                    self.pattern_target_clobbers(expr, scope, root)
                }
                ObjectProp::Method(_) => false,
            }),
            _ => false,
        }
    }

    fn pattern_target_clobbers(&self, tgt: &Expr, scope: ScopeId, root: SymbolId) -> bool {
        match tgt {
            Expr::Array { .. } | Expr::Object { .. } => self.pattern_clobbers(tgt, scope, root),
            _ => self
                .ref_key_in_scope(tgt, scope)
                .is_some_and(|k| k.0 == root),
        }
    }

    /// Does this declarator bind `sym`? Top-level ident declarators are keyed
    /// in `decl_symbol` by the declarator node, pattern idents by the ident
    /// node.
    fn decl_declares(&self, decl: &VarDeclarator, sym: SymbolId) -> bool {
        self.bind.decl_symbol.get(&node_key(decl)) == Some(&sym)
            || self.binding_declares(&decl.name, sym)
    }

    fn binding_declares(&self, b: &Binding, sym: SymbolId) -> bool {
        match b {
            Binding::Ident(id) => self.bind.decl_symbol.get(&node_key(id)) == Some(&sym),
            Binding::Object(p) => {
                p.props
                    .iter()
                    .any(|pr| self.binding_declares(&pr.binding, sym))
                    || p.rest
                        .as_deref()
                        .is_some_and(|r| self.binding_declares(r, sym))
            }
            Binding::Array(a) => a
                .elements
                .iter()
                .flatten()
                .any(|el| self.binding_declares(&el.binding, sym)),
        }
    }

    /// tsc recombineUnknownType at flow joins: a union that reassembles the
    /// decomposed strict `unknown` (`{} | null | undefined`, the empty
    /// object anonymous and memberless) collapses back to the declared
    /// `unknown`. Guarded on the declared type so an annotated
    /// `{} | null | undefined` keeps its shape.
    fn recombine_unknown(&mut self, u: TypeId, declared: Option<TypeId>) -> TypeId {
        if declared.is_none_or(|d| !matches!(self.types.kind(d), TypeKind::Unknown)) {
            return u;
        }
        let members = self.types.union_members(u);
        if members.len() == 3
            && members
                .iter()
                .any(|&m| matches!(self.types.kind(m), TypeKind::Null))
            && members
                .iter()
                .any(|&m| matches!(self.types.kind(m), TypeKind::Undefined))
            && members.iter().any(|&m| self.is_empty_object_type(m))
        {
            return self.types.unknown;
        }
        u
    }

    /// Stage-2 definite assignment at an identifier read (tsc
    /// checkIdentifier's 2454 path). For a candidate — strictNullChecks on;
    /// a `let`/`var` declarator without `!`; not ambient/alias/param; the
    /// declared type neither any/unknown/void nor undefined-including — run
    /// the flow walk seeded with `declared | undefined` (tsc initialType =
    /// getOptionalType). A surviving `undefined` means some path reaches the
    /// container entry unassigned: report 2454 and yield the DECLARED type
    /// (tsc returns `type` after the error). `None` = not a candidate or
    /// the walk cannot answer — the caller falls through to the normal
    /// (unseeded) read path.
    pub(crate) fn da_check_ident_read(&mut self, id: &Ident, sym: SymbolId) -> Option<TypeId> {
        if !self.options.strict_null_checks() {
            return None;
        }
        let sflags = self.symbol(sym).flags;
        if sflags & flags::ALIAS != 0 {
            return None;
        }
        if sflags & (flags::FUNCTION_SCOPED_VARIABLE | flags::BLOCK_SCOPED_VARIABLE) == 0 {
            return None;
        }
        // declaration shape: a let/var declarator IN THIS FILE (a merged
        // lib global like `var Symbol: SymbolConstructor` picks the local
        // declarator; a purely cross-file variable is outer by definition
        // and assumed initialized). Parameters and catch variables never
        // match (tsc isParameter; catch clauses admit only any/unknown).
        let cur_file = self.current_file;
        let decl = self.symbol(sym).decls.iter().find_map(|d| match d {
            crate::binder::Decl::Var(v, k)
                if self.bind.decl_file.get(&node_key(*v)) == Some(&cur_file) =>
            {
                Some((*v, *k))
            }
            _ => None,
        });
        let Some((d, kind)) = decl else {
            return None;
        };
        let (has_init, has_exclam, has_ty, decl_key, decl_span) =
            (d.init.is_some(), d.exclam, d.ty.is_some(), node_key(d), d.span);
        // `declare let/var` — the DECLARATOR's ambient context, not the
        // symbol flag (a lib-merged global like `Symbol` carries AMBIENT
        // from the lib while its local declarator is checkable)
        if self.bind.decl_ambient.contains(&decl_key) {
            return None;
        }
        if !matches!(kind, VarKind::Let | VarKind::Var) || has_exclam {
            return None;
        }
        // `x!` (direct NonNull parent) asserts initialization — parens
        // (`(x)!`) deliberately break the exemption, as in tsc
        if self.cflags.nonnull_ident == node_key(id) {
            return None;
        }
        // declared-type gates (tsc assumeInitialized's type disjuncts).
        // Unannotated `let x;` is plain `any` here, mirroring tsc's autoType
        // branch never producing 2454. The gate consults the SAME-FILE
        // declarator's own annotation when present — a lib-merged global
        // (`var Symbol: any` over the lib's `declare var Symbol:
        // SymbolConstructor`) types reads by the first declaration, but the
        // local `any` annotation still exempts it, matching the oracle.
        let declared = self.type_of_symbol(sym);
        // the annotation re-resolve only matters for lib-merged globals
        // (symbol.file = the lib), where the merged type hides a local
        // `any`; single-file symbols would only risk re-resolution noise
        // (recursive aliases resolve to error mid-read)
        let gate_ty = match &d.ty {
            Some(ann) if self.symbol(sym).file != self.current_file => {
                let scope = self
                    .bind
                    .decl_scope
                    .get(&decl_key)
                    .copied()
                    .unwrap_or(self.current_scope);
                self.resolve_type_cached(ann, scope)
            }
            _ => declared,
        };
        for t in [declared, gate_ty] {
            if matches!(
                self.types.kind(t),
                TypeKind::Any | TypeKind::Unknown | TypeKind::Error | TypeKind::Void
            ) || self.contains_undefined_member(t)
            {
                if verbose() {
                    let name = self.symbol(sym).name.clone();
                    let shown = self.display_type(t);
                    eprintln!("DA_GATE {}@{} bail type={}", name, id.span.start, shown);
                }
                return None;
            }
        }
        // outer-variable rule (tsc isOuterVariable && !isNeverInitialized):
        // the walk consumes the seed only at the DECLARING container's
        // entry — a read whose walk stops at a foreign (nested) container
        // entry gets the declared type there, i.e. is assumed initialized.
        // Never-initialized variables — annotated, never-assigned `let`s
        // (a for-in/of head cannot carry an annotation, so it never
        // counts) — consume the seed at ANY entry: tsc checks those even
        // across closures. Module-level declarations keep the pre-CFG
        // conservative skip for the never-initialized escalation (any
        // top-level statement may assign before the function runs).
        let decl_container = self
            .bind
            .decl_container
            .get(&decl_key)
            .copied()
            .unwrap_or(0);
        let never_initialized = matches!(kind, VarKind::Let)
            && !has_init
            && has_ty
            && decl_container != 0
            && !self.symuse.assigned_symbols.contains(&sym);
        let undef = self.types.undefined;
        let initial = self.types.union(vec![declared, undef]);
        let key = RefKey(sym, Vec::new());
        let t =
            self.flow_type_of_da_read(node_key(id), &key, initial, decl_span, never_initialized);
        if verbose() {
            let shown = match t {
                Some(t) => self.display_type(t),
                None => "None".into(),
            };
            let name = self.symbol(sym).name.clone();
            eprintln!("DA_CHECK {}@{} -> {}", name, id.span.start, shown);
        }
        let t = t?;
        if self.contains_undefined_member(t) {
            let name = self.symbol(sym).name.clone();
            self.report_used_before_assigned(id.span, name);
            return Some(declared);
        }
        Some(t)
    }

    /// `da_check_ident_read` for a compound-assignment target (`x += 1`
    /// reads x first — tsc AssignmentKind.Compound proceeds through
    /// checkIdentifier). Bare and paren-wrapped identifier targets bypass
    /// the read seam (`check_target_type`), so the compound arm calls this
    /// directly; only the report side effect matters.
    pub(crate) fn da_check_compound_target(&mut self, target: &Expr) {
        let mut e = target;
        while let Expr::Paren { inner, .. } = e {
            e = inner;
        }
        if let Expr::Ident(id) = e {
            if let Some(sym) = self.lookup_value(self.current_scope, &id.name) {
                let _ = self.da_check_ident_read(id, sym);
            }
        }
    }

    /// TS2564: is `this.<prop>` definitely assigned at the constructor's
    /// END flow (every `return` joined with the fall-through)? Mirrors tsc
    /// isPropertyInitializedInConstructor: seed `propType | undefined`, key
    /// `this` for the query's duration, and read whether `undefined`
    /// survived. `None` = the graph cannot answer (no end flow recorded) —
    /// callers treat that as not-assigned, like the old syntactic scan.
    pub(crate) fn prop_assigned_in_ctor_flow(
        &mut self,
        class_sym: SymbolId,
        ctor_key: usize,
        prop: &str,
        prop_ty: TypeId,
        prop_span: Span,
    ) -> Option<bool> {
        let end = *self.bind.fn_end_flow.get(&ctor_key)?;
        let this_sym = self.this_param_of(class_sym);
        let key = RefKey(this_sym, vec![prop.to_string()]);
        let undef = self.types.undefined;
        let initial = self.types.union(vec![prop_ty, undef]);
        self.fresolve.this_sym = Some(this_sym);
        // never_initialized=true: the property "declares" outside the
        // constructor's span, so the seed must be consumable at the ctor's
        // own entry
        let t = self.get_flow_type_of_reference_seeded(&key, end, initial, prop_span, true);
        self.fresolve.this_sym = None;
        t.map(|t| !self.contains_undefined_member(t))
    }

    /// The seeded-walk entry with the read-seam guard triple.
    fn flow_type_of_da_read(
        &mut self,
        nk: usize,
        key: &RefKey,
        initial: TypeId,
        decl_span: Span,
        never_initialized: bool,
    ) -> Option<TypeId> {
        if !self.fresolve.in_progress.is_empty() {
            return None;
        }
        if !self.res.resolving.is_empty() || self.fresolve.suppress > 0 {
            return None;
        }
        let fnode = *self.bind.flow_node.get(&nk)?;
        self.get_flow_type_of_reference_seeded(key, fnode, initial, decl_span, never_initialized)
    }

    /// Stage-1 read seam: the resolver's answer for reference `key` at AST
    /// node `nk`, now used IN PLACE of the lexical fact when it can be
    /// computed. `None` sends the caller down the legacy fact path: type
    /// positions have no flow node, dead-code walks return no answer, and
    /// out-of-lexical-context reads (lazy symbol resolution, TS2403's
    /// re-derivation, resolver re-entrancy) must not consult a flow node
    /// that describes a different program point.
    pub(crate) fn flow_type_of_read(&mut self, nk: usize, key: &RefKey) -> Option<TypeId> {
        if !self.fresolve.in_progress.is_empty() {
            return None;
        }
        if !self.res.resolving.is_empty() || self.fresolve.suppress > 0 {
            return None;
        }
        // tsc runs flow analysis only for variable-like references
        // (checkIdentifier bails for function/class/enum/namespace symbols)
        // — the exhaustive-switch no-match `never` would otherwise leak
        // into every callee read in post-switch code
        if self.symbol(key.0).flags
            & (flags::FUNCTION_SCOPED_VARIABLE | flags::BLOCK_SCOPED_VARIABLE)
            == 0
        {
            return None;
        }
        let fnode = *self.bind.flow_node.get(&nk)?;
        self.get_flow_type_of_reference(key, fnode)
    }

    /// Dark-launch verification at a fact-stack read seam: compute the
    /// resolver's answer for the same reference and tally agreement against
    /// the fact-based result. Never changes checker output (diagnostics are
    /// rolled back; the caller still returns the fact-based type).
    pub(crate) fn flow_verify_read(
        &mut self,
        nk: usize,
        key: &RefKey,
        fact: Option<TypeId>,
        span: Span,
    ) {
        // resolver → type_of_symbol → lazy initializer check can re-enter a
        // read seam; verifying that inner read would clobber the outer walk
        if !self.fresolve.in_progress.is_empty() {
            return;
        }
        // lazy symbol-type resolution and explicit out-of-context re-checks
        // (TS2403 re-deriving a merged `var`'s first declarator under a later
        // declarator's facts) re-check nodes OUT of their lexical context —
        // the fact and the flow node would describe different program
        // points, so don't compare
        if !self.res.resolving.is_empty() || self.fresolve.suppress > 0 {
            return;
        }
        let Some(&fnode) = self.bind.flow_node.get(&nk) else {
            FLOW_VERIFY_NO_NODE.fetch_add(1, Ordering::Relaxed);
            return;
        };
        self.fresolve.quiet += 1;
        let dlen = self.diags.len();
        let resolved = self.get_flow_type_of_reference(key, fnode);
        let baseline = match fact {
            Some(t) => Some(t),
            None => self.declared_type_of_ref(key),
        };
        self.diags.truncate(dlen);
        self.fresolve.quiet -= 1;
        let Some(baseline) = baseline else { return };
        match resolved {
            Some(t) if t == baseline => {
                FLOW_VERIFY_MATCH.fetch_add(1, Ordering::Relaxed);
            }
            Some(t) => {
                // normalize before comparing: `T & {}` (NonNullable) interns a
                // fresh empty-object shape per construction site, so the same
                // type built twice gets two TypeIds — display equality treats
                // those as agreement (tallied separately)
                let f = self.display_type(baseline);
                let r = self.display_type(t);
                if f == r {
                    FLOW_VERIFY_DISPLAY_MATCH.fetch_add(1, Ordering::Relaxed);
                    return;
                }
                FLOW_VERIFY_MISMATCH.fetch_add(1, Ordering::Relaxed);
                if verbose() {
                    let file = self.files[self.current_file].0.clone();
                    let name = self.symbol(key.0).name.clone();
                    let path = if key.1.is_empty() {
                        String::new()
                    } else {
                        format!(".{}", key.1.join("."))
                    };
                    eprintln!(
                        "FLOW_VERIFY mismatch {}:{} {}{}: fact={} resolver={}",
                        file, span.start, name, path, f, r
                    );
                }
            }
            None => {
                FLOW_VERIFY_UNRESOLVED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Cheap syntactic filter for `Step::Call`: `call_predicate` resolves a
/// predicate only for bare ident / member callees (no paren-stripping), so
/// anything else is pass-through without the narrowing scaffold.
fn callee_may_assert(callee: &Expr) -> bool {
    matches!(
        callee,
        Expr::Ident(_) | Expr::PropAccess { .. } | Expr::ElemAccess { .. }
    )
}
