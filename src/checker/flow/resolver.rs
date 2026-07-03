//! The flow-graph type resolver — since Stage 4, THE narrowing engine.
//!
//! `get_flow_type_of_reference` answers "what is the type of reference `key`
//! at flow node `flow`" by walking the bind-time flow graph backward toward
//! `Start`, reusing `narrow_by_condition` as the condition narrower (seeded
//! into a scratch fact frame, read back, popped — the fact stack's only
//! remaining role). The read seams (`check_ident` / `check_prop_access`),
//! definite assignment (2454/2564/2565), auto-variable CFA (7005/7034),
//! reachability (7027/2355/2366/2534/7029/7030) and `this`/typeof-this
//! narrowing all run on these walks; a `None` answer means the read keeps
//! the declared / checker-computed type, as tsc does.

use crate::ast::*;
use crate::binder::{flags, FlowNode, FlowNodeId, ScopeId, SymbolId};
use crate::checker::exprs::node_key_expr;
use crate::checker::{Checker, RefKey};
use crate::types::{TypeId, TypeKind};

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
        let mut t = if let Some(owner) = self.this_param_owner(key.0) {
            // a `this`-rooted key (seeded 2564/2565 queries, typeof-this
            // type queries): the declaring class's instance type (the
            // symbol itself is a type parameter with no value declaration)
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

    /// AUTO-query entry (tsc autoType CFA): an unannotated, noImplicitAny
    /// let/var whose initializer is absent or nullish reads its control-flow
    /// type — seeded with the declaration's initial nullish type. A foreign
    /// container `Start` yields the auto MARKER; a result that IS the marker
    /// means every path reached such an entry (tsc flowType === autoType) ⇒
    /// the caller reports TS7005/7034 and converts to `any`. Returns
    /// `(type, is_marker)`.
    pub(crate) fn get_flow_type_of_reference_auto(
        &mut self,
        key: &RefKey,
        flow: FlowNodeId,
        initial: TypeId,
        decl_span: Span,
    ) -> Option<(TypeId, bool)> {
        let marker = match self.fresolve.auto_marker {
            Some(m) => m,
            None => {
                let m = self.types.alloc(TypeKind::Any);
                self.fresolve.auto_marker = Some(m);
                m
            }
        };
        self.fresolve.memo.clear();
        self.fresolve.initial = Some((key.clone(), initial, decl_span, false));
        self.fresolve.auto = Some(key.clone());
        let r = self.flow_type_at(key, flow, 0);
        self.fresolve.initial = None;
        self.fresolve.auto = None;
        self.fresolve.in_progress.clear();
        match r {
            FlowRes::Ty(t) if t == marker => Some((self.types.any, true)),
            FlowRes::Ty(t) => Some((self.drop_nullish_when_mixed(t), false)),
            _ => None,
        }
    }

    /// Non-strict CFA joins drop nullish members once a non-nullish type has
    /// been assigned (oracle: `let x; if (c) x = 1; x` is `number`, but the
    /// exactly-initial read stays `undefined`). Strict keeps everything.
    fn drop_nullish_when_mixed(&mut self, t: TypeId) -> TypeId {
        if self.options.strict_null_checks() {
            return t;
        }
        let members = self.types.union_members(t);
        if members.len() < 2 {
            return t;
        }
        let live: Vec<TypeId> = members
            .iter()
            .copied()
            .filter(|&m| !matches!(self.types.kind(m), TypeKind::Null | TypeKind::Undefined))
            .collect();
        if live.is_empty() || live.len() == members.len() {
            return t;
        }
        self.types.union(live)
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

    /// Reachability of a flow node — the LAZY walk (tsc
    /// isReachableFlowNode): never-returning statement-position calls,
    /// `assert(false)`, and the exhaustive-switch no-match clause terminate
    /// flow. Drives TS7027 for plain statements, TS7029, and
    /// has-implicit-return.
    pub(crate) fn is_reachable_flow(&mut self, flow: FlowNodeId) -> bool {
        let mut visiting = std::collections::HashSet::new();
        self.reachable_walk(flow, true, &mut visiting)
    }

    /// Reachability of a flow node — the STRUCTURAL walk (tsc's binder
    /// view, type-blind): Call and Switch are transparent. Drives
    /// has-explicit-return and TS7027 for class/enum/namespace
    /// declarations (which carry no flowNode in tsc — the coarse bit).
    pub(crate) fn is_structurally_reachable(&mut self, flow: FlowNodeId) -> bool {
        let mut visiting = std::collections::HashSet::new();
        self.reachable_walk(flow, false, &mut visiting)
    }

    /// Branch = OR over antecedents with cycle-as-false: loop labels are
    /// `Branch([entry, back-edges…])` and a back edge can only reach the
    /// label through the label itself, so this equals tsc's LoopLabel
    /// "antecedent[0]" rule. Branch results are memoized per mode (least
    /// fixpoint — cycle-false contributions are deterministic).
    fn reachable_walk(
        &mut self,
        mut flow: FlowNodeId,
        lazy: bool,
        visiting: &mut std::collections::HashSet<FlowNodeId>,
    ) -> bool {
        loop {
            enum R<'x> {
                Done(bool),
                Next(FlowNodeId),
                Branch(Vec<FlowNodeId>),
                Call(&'x Expr, FlowNodeId),
                Switch(bool, FlowNodeId),
            }
            let step = match &self.bind.flow_nodes[flow.0 as usize] {
                FlowNode::Unreachable => R::Done(false),
                FlowNode::Start { .. } => R::Done(true),
                FlowNode::Cond { ante, .. }
                | FlowNode::Assign { ante, .. }
                | FlowNode::Init { ante, .. }
                | FlowNode::Nullish { ante, .. } => R::Next(*ante),
                FlowNode::Call { call, ante, .. } => R::Call(call, *ante),
                FlowNode::Switch {
                    cases,
                    clause,
                    stmt_key,
                    ante,
                    ..
                } => R::Switch(
                    *clause as usize == cases.len()
                        && self.flow.exhaustive_switches.contains(stmt_key),
                    *ante,
                ),
                FlowNode::Branch(antes) => R::Branch(antes.clone()),
            };
            match step {
                R::Done(r) => return r,
                R::Next(a) => flow = a,
                R::Call(call, a) => {
                    if lazy && self.call_terminates_flow(call) {
                        return false;
                    }
                    flow = a;
                }
                R::Switch(exhaustive_no_match, a) => {
                    if lazy && exhaustive_no_match {
                        return false;
                    }
                    flow = a;
                }
                R::Branch(antes) => {
                    let memo = if lazy {
                        &self.fresolve.reach_lazy
                    } else {
                        &self.fresolve.reach_structural
                    };
                    if let Some(&r) = memo.get(&flow) {
                        return r;
                    }
                    if !visiting.insert(flow) {
                        return false; // cycle: only reachable through itself
                    }
                    let mut r = false;
                    for a in antes {
                        if self.reachable_walk(a, lazy, visiting) {
                            r = true;
                            break;
                        }
                    }
                    visiting.remove(&flow);
                    if lazy {
                        self.fresolve.reach_lazy.insert(flow, r);
                    } else {
                        self.fresolve.reach_structural.insert(flow, r);
                    }
                    return r;
                }
            }
        }
    }

    /// Does this call terminate flow (tsc getEffectsSignature +
    /// hasTypePredicateOrNeverReturnType)? Four gates keep it faithful:
    /// statement position only; dotted-name callee typed through EXPLICIT
    /// annotations (inferred `never` does not count); a single non-generic
    /// call signature; and either the return ANNOTATION resolved to
    /// `never` or a bare `asserts x` applied to a literally-false argument.
    fn call_terminates_flow(&mut self, call: &Expr) -> bool {
        let Expr::Call { callee, args, .. } = call else {
            return false;
        };
        if !self
            .bind
            .stmt_position_calls
            .contains(&node_key_expr(call))
        {
            return false;
        }
        let Some(ct) = self.explicit_type_of_dotted_name(callee) else {
            return false;
        };
        let sigs = self.call_signatures_of(ct);
        if sigs.len() != 1 {
            return false;
        }
        let sig = self.types.sig(sigs[0]).clone();
        if !sig.type_params.is_empty() {
            return false;
        }
        if let Some(p) = &sig.predicate {
            if p.asserts && p.ty.is_none() && p.param >= 0 {
                if let Some(arg) = args.get(p.param as usize) {
                    if is_false_expression(arg) {
                        return true;
                    }
                }
            }
        }
        sig.ret_annotation_never
    }

    /// tsc getTypeOfDottedName / getExplicitTypeOfSymbol: the callee's type
    /// through EXPLICIT annotations only. Function/method/class symbols are
    /// always explicit; variables, properties and parameters count only
    /// when their declaration carries a type annotation. `this` is
    /// deferred (FN-side).
    fn explicit_type_of_dotted_name(&mut self, e: &Expr) -> Option<TypeId> {
        match e {
            Expr::Paren { inner, .. } => self.explicit_type_of_dotted_name(inner),
            Expr::Ident(id) => {
                let sym = self.lookup_value(self.current_scope, &id.name)?;
                let sym = self.resolve_alias_chain(sym);
                let s = self.symbol(sym);
                let explicit = s.flags & (flags::FUNCTION | flags::CLASS | flags::NAMESPACE) != 0
                    || s.decls.iter().any(|d| match d {
                        crate::binder::Decl::Var(v, _) => v.ty.is_some(),
                        crate::binder::Decl::Param(p) => p.ty.is_some(),
                        _ => false,
                    });
                if !explicit {
                    return None;
                }
                Some(self.type_of_symbol(sym))
            }
            Expr::PropAccess {
                obj,
                name,
                question_dot: false,
                ..
            } => {
                let ot = self.explicit_type_of_dotted_name(obj)?;
                self.prop_of_type(ot, &name.name)
            }
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
                        // AUTO query at a FOREIGN container's entry: a
                        // capture read cannot see the declaring container's
                        // assignments — tsc autoType (⇒ TS7005/7034 when it
                        // survives to the result)
                        if self.fresolve.auto.is_some() {
                            if let Some(m) = self.fresolve.auto_marker {
                                return FlowRes::Ty(m);
                            }
                        }
                    }
                }
                match self.declared_type_of_ref(key) {
                    Some(t) => FlowRes::Ty(t),
                    None => FlowRes::Unknown,
                }
            }
            // tsc getTypeAtFlowNode's unreachable terminus yields the
            // DECLARED type, unnarrowed (a read whose own flow sits in dead
            // code). Never-call deadness is different: it is the propagated
            // FlowRes::Dead from the Call arm (tsc unreachableNeverType,
            // dissolving at joins / unwrapping to declared at the read).
            Step::Dead => match self.declared_type_of_ref(key) {
                Some(t) => FlowRes::Ty(t),
                None => FlowRes::Unknown,
            },
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
                // tsc: auto is INFECTIOUS at joins — one path reaching a
                // foreign container's entry unassigned makes the whole join
                // auto (`let x; () => { if (c) x = 1; x }` reads any + 7005)
                if let (Some(ak), Some(m)) = (&self.fresolve.auto, self.fresolve.auto_marker) {
                    if ak == key && tys.contains(&m) {
                        return FlowRes::Ty(m);
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
                self.fresolve.scaffold_base.push(self.flow.facts.len());
                self.fresolve.quiet += 1;
                let dlen = self.diags.len();
                let out = self.narrowed(|c| {
                    c.set_fact(key.clone(), t_in);
                    c.narrow_by_condition(cond, sense);
                    c.fact_for(key).unwrap_or(t_in)
                });
                self.diags.truncate(dlen);
                self.fresolve.quiet -= 1;
                self.fresolve.scaffold_base.pop();
                self.current_scope = saved_scope;
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
                self.fresolve.scaffold_base.push(self.flow.facts.len());
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
                self.fresolve.scaffold_base.pop();
                self.current_scope = saved_scope;
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
                // a never-returning statement-position call terminates flow
                // (tsc getTypeAtFlowCall → unreachable): downstream reads
                // join over the other paths only
                if self.call_terminates_flow(call) {
                    return FlowRes::Dead;
                }
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
                self.fresolve.scaffold_base.push(self.flow.facts.len());
                self.fresolve.quiet += 1;
                let dlen = self.diags.len();
                let out = self.narrowed(|c| {
                    c.set_fact(key.clone(), t_in);
                    c.apply_assertion_narrowing(callee, args);
                    c.fact_for(key).unwrap_or(t_in)
                });
                self.diags.truncate(dlen);
                self.fresolve.quiet -= 1;
                self.fresolve.scaffold_base.pop();
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
                    // tsc getTypeAtFlowAssignment: a MATCHING assignment on an
                    // unreachable path contributes unreachableNeverType —
                    // dropped at joins, declared at the read (`if (false)
                    // { x = 1 } x` must not see `1`)
                    Some(tk) if tk == *key => {
                        if !self.is_reachable_flow(flow) {
                            return FlowRes::Dead;
                        }
                        self.assigned_type(key, expr, scope, ante, depth)
                    }
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
                // dead initialization — same tsc reachability guard as the
                // Assign arm (an Init is a FlowAssignment in tsc)
                if !self.is_reachable_flow(flow) {
                    return FlowRes::Dead;
                }
                // AUTO query (tsc getInitialType): the nullish initializer's
                // type IS the flow type at the declaration — the declared
                // `any` would swallow the CFA (`let x = null; if (c) x = 1;
                // x` reads `number | null` under strict)
                if matches!(&self.fresolve.auto, Some(ak) if ak == key) {
                    return FlowRes::Ty(match &decl.init {
                        Some(Expr::NullLit { .. }) => self.types.null,
                        _ => self.types.undefined,
                    });
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
                        let it = match self.flow_expr_type(init, scope, ante, depth, Some(declared)) {
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
                let rt = match self.flow_expr_type(right, scope, ante, depth, Some(declared)) {
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
                let rt = match self.flow_expr_type(right, scope, ante, depth, Some(declared)) {
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
    /// itself for bare identifiers (which the cache deliberately excludes),
    /// and an exploratory contextual check for expressions the real pass has
    /// not reached yet. `ctx` is the contextual type the real pass would use
    /// (the assignment target's declared type).
    fn flow_expr_type(
        &mut self,
        e: &'a Expr,
        scope: ScopeId,
        flow: FlowNodeId,
        depth: u32,
        ctx: Option<TypeId>,
    ) -> FlowRes {
        match e {
            Expr::Paren { inner, .. } => self.flow_expr_type(inner, scope, flow, depth, ctx),
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
                // not yet checked — a loop back-edge assignment whose
                // statement FOLLOWS the read lexically (`let y = f(x);
                // x = y + 1;` inside a while). tsc checks the RHS on
                // demand (getTypeAtFlowAssignment → checkExpression);
                // mirror it exploratorily: quiet rolls diagnostics back
                // and keeps caches/report-once guards unconsumed. Inner
                // reads of the walk's own key hit the in_progress guard
                // and fall back to declared, so self-referential RHS
                // (`x = x + 1`) terminates.
                None => {
                    let saved_scope = self.current_scope;
                    self.current_scope = scope;
                    self.fresolve.scaffold_base.push(self.flow.facts.len());
                self.fresolve.quiet += 1;
                    let dlen = self.diags.len();
                    let t = self.check_expr(e, ctx);
                    self.diags.truncate(dlen);
                    self.fresolve.quiet -= 1;
                self.fresolve.scaffold_base.pop();
                    self.current_scope = saved_scope;
                    FlowRes::Ty(t)
                }
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
    /// AUTO read (tsc autoType): an unannotated, noImplicitAny, non-ambient,
    /// non-exported let/var with an absent or nullish initializer reads its
    /// control-flow type instead of the declared `any`. A marker result
    /// (every path reached a foreign container's entry) reports TS7005 at
    /// the read, records the declaration for TS7034, and yields `any`.
    /// None ⇒ the normal read path proceeds (declared `any`).
    pub(crate) fn auto_check_ident_read(&mut self, id: &Ident, sym: SymbolId) -> Option<TypeId> {
        if !self.options.no_implicit_any() {
            return None;
        }
        // re-entrancy guard only: initializer inference (`const y = x`)
        // resolves `y` lazily and reads `x` under resolving — the flow graph
        // is bind-time and position-correct, so the walk is valid there
        // (tsc flow-analyzes exactly these reads).
        if !self.fresolve.in_progress.is_empty() {
            return None;
        }
        let sflags = self.symbol(sym).flags;
        if sflags & flags::ALIAS != 0 {
            return None;
        }
        if sflags & (flags::FUNCTION_SCOPED_VARIABLE | flags::BLOCK_SCOPED_VARIABLE) == 0 {
            return None;
        }
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
        if !matches!(kind, VarKind::Let | VarKind::Var) || d.exclam || d.ty.is_some() {
            return None;
        }
        if !matches!(d.name, Binding::Ident(_)) {
            return None;
        }
        let decl_key = node_key(d);
        if self.bind.decl_ambient.contains(&decl_key)
            || self.bind.decl_exported.contains(&decl_key)
            || self.bind.decl_loop_head.contains(&decl_key)
        {
            return None;
        }
        // reads POSITIONED before the declarator keep the declared `any`:
        // hoisted `var` pre-reads and TDZ `let` reads stay on the legacy
        // path (a param-initializer scoping divergence resolves `class
        // extends C` to a body `var C` — typing that read `undefined`
        // manufactured a 2507 tsc doesn't have; tsc CFA's `x; var x = 1`
        // → `undefined` is a documented deferral here)
        if id.span.start < d.span.start {
            return None;
        }
        let initial = match &d.init {
            None => self.types.undefined,
            Some(Expr::Ident(i)) if i.name == "undefined" => self.types.undefined,
            Some(Expr::NullLit { .. }) => self.types.null,
            Some(_) => return None,
        };
        let key = RefKey(sym, Vec::new());
        let fnode = *self.bind.flow_node.get(&node_key(id))?;
        let (t, is_marker) = self.get_flow_type_of_reference_auto(&key, fnode, initial, d.span)?;
        if is_marker {
            if self.fresolve.quiet == 0 {
                let name = self.symbol(sym).name.clone();
                self.error_at(
                    id.span,
                    &crate::diagnostics::gen::Variable_0_implicitly_has_an_1_type,
                    &[name, "any".to_string()],
                );
                self.flow.auto_fired.insert(sym, (cur_file, d.name.span()));
            }
            return Some(self.types.any);
        }
        Some(t)
    }

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

    /// `typeof this` in a TYPE position: run the walk for the class's
    /// `this` parameter at the query's flow node (mapped by the builder for
    /// local declarator annotations). `None` = unmapped position or no
    /// narrowing in effect — the caller keeps the bare polymorphic this.
    pub(crate) fn flow_type_of_this_query(
        &mut self,
        nk: usize,
        this_param: SymbolId,
    ) -> Option<TypeId> {
        if !self.fresolve.in_progress.is_empty() {
            return None;
        }
        let fnode = *self.bind.flow_node.get(&nk)?;
        let key = RefKey(this_param, Vec::new());
        let r = self.get_flow_type_of_reference(&key, fnode)?;
        if Some(r) == self.declared_type_of_ref(&key) {
            return None;
        }
        Some(r)
    }

    /// The seeded-walk entry with the read-seam guard triple.
    pub(crate) fn flow_type_of_da_read(
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
        if !self.res.resolving.is_empty() {
            return None;
        }
        let fnode = *self.bind.flow_node.get(&nk)?;
        self.get_flow_type_of_reference_seeded(key, fnode, initial, decl_span, never_initialized)
    }

    /// The read seam: the resolver's answer for reference `key` at AST node
    /// `nk`; `None` means the read keeps the declared / checker-computed
    /// type (type positions have no flow node, re-entrant reads bail on
    /// in_progress). Reads under LAZY SYMBOL RESOLUTION (`res.resolving`)
    /// DO walk: initializer inference (`const y = x` resolving `y`) reads
    /// `x` at its own position, and the bind-time graph is position-correct
    /// no matter when resolution happens. Assignment-TARGET prop reads are
    /// skipped by the seam itself (`cflags.assign_target`).
    pub(crate) fn flow_type_of_read(&mut self, nk: usize, key: &RefKey) -> Option<TypeId> {
        if !self.fresolve.in_progress.is_empty() {
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

}

/// tsc isFalseExpression: literally-false conditions — the `false` keyword,
/// `&&` with either side false, `||` with both sides false; parens skipped
/// (unlike flow-edge conditions, which keep tsc's raw-keyword rule).
fn is_false_expression(e: &Expr) -> bool {
    match e {
        Expr::BoolLit { value: false, .. } => true,
        Expr::Paren { inner, .. } => is_false_expression(inner),
        Expr::Binary {
            op: BinOp::AmpAmp,
            left,
            right,
            ..
        } => is_false_expression(left) || is_false_expression(right),
        Expr::Binary {
            op: BinOp::BarBar,
            left,
            right,
            ..
        } => is_false_expression(left) && is_false_expression(right),
        _ => false,
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
