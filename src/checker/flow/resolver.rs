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
    Unknown,
}

/// A borrow-free copy of one flow node's payload (the `&'a` fields are `Copy`,
/// so extracting them releases the borrow of `bind.flow_nodes`).
enum Step<'a> {
    Start,
    Branch(Vec<FlowNodeId>),
    Cond(&'a Expr, bool, ScopeId, FlowNodeId),
    Assign(&'a Expr, &'a Expr, ScopeId, FlowNodeId),
    Init(&'a VarDeclarator, ScopeId, FlowNodeId),
    Pass(FlowNodeId),
}

impl<'a> Checker<'a> {
    /// The declared (un-narrowed) type of a narrowable reference: the root
    /// symbol's type walked along the property path. This is
    /// `current_type_of_key` minus the `fact_for` lookups.
    pub(crate) fn declared_type_of_ref(&mut self, key: &RefKey) -> Option<TypeId> {
        let mut t = self.type_of_symbol(key.0);
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
        let r = self.flow_type_at(key, flow, 0);
        self.fresolve.in_progress.clear();
        match r {
            FlowRes::Ty(t) => Some(t),
            _ => None,
        }
    }

    fn flow_type_at(&mut self, key: &RefKey, flow: FlowNodeId, depth: u32) -> FlowRes {
        if depth > 200 {
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
            // context-dependent; not memoizable
            FlowRes::Cycle => {}
        }
        r
    }

    fn flow_step(&mut self, key: &RefKey, flow: FlowNodeId, depth: u32) -> FlowRes {
        let step = match &self.bind.flow_nodes[flow.0 as usize] {
            FlowNode::Start => Step::Start,
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
            // Switch clauses aren't built yet; asserting calls not modeled yet.
            FlowNode::Switch { ante, .. } | FlowNode::Call { ante, .. } => Step::Pass(*ante),
        };
        match step {
            Step::Start => match self.declared_type_of_ref(key) {
                Some(t) => FlowRes::Ty(t),
                None => FlowRes::Unknown,
            },
            Step::Pass(ante) => self.flow_type_at(key, ante, depth + 1),
            Step::Branch(antes) => {
                let mut tys: Vec<TypeId> = Vec::new();
                let mut any_cycle = false;
                for a in antes {
                    match self.flow_type_at(key, a, depth + 1) {
                        FlowRes::Ty(t) => {
                            if !tys.contains(&t) {
                                tys.push(t);
                            }
                        }
                        FlowRes::Cycle => any_cycle = true,
                        FlowRes::Unknown => return FlowRes::Unknown,
                    }
                }
                match tys.len() {
                    0 if any_cycle => FlowRes::Cycle,
                    // a join no edge reaches (e.g. the post-label of
                    // `while (true) {}`): nothing to say
                    0 => FlowRes::Unknown,
                    1 => FlowRes::Ty(tys[0]),
                    _ => FlowRes::Ty(self.types.union(tys)),
                }
            }
            Step::Cond(cond, sense, scope, ante) => {
                let t_in = match self.flow_type_at(key, ante, depth + 1) {
                    FlowRes::Ty(t) => t,
                    other => return other,
                };
                // Reuse the existing condition narrower: seed a scratch fact
                // frame with the antecedent type, run it, read the result
                // back, pop. Names in `cond` resolve in ITS scope; definite-
                // assignment recording stays off; any diagnostics a lazy
                // resolution emits along the way are rolled back.
                let saved_scope = self.current_scope;
                let saved_da = self.flow.record_da_facts;
                self.current_scope = scope;
                self.flow.record_da_facts = false;
                let dlen = self.diags.len();
                let out = self.narrowed(|c| {
                    c.set_fact(key.clone(), t_in);
                    c.narrow_by_condition(cond, sense);
                    c.fact_for(key).unwrap_or(t_in)
                });
                self.diags.truncate(dlen);
                self.flow.record_da_facts = saved_da;
                self.current_scope = saved_scope;
                FlowRes::Ty(out)
            }
            Step::Assign(target, expr, scope, ante) => {
                match self.ref_key_in_scope(target, scope) {
                    Some(tk) if tk == *key => self.assigned_type(key, expr, scope, ante, depth),
                    // fact parity: an assignment through the same root
                    // invalidates every fact rooted at it
                    Some(tk) if tk.0 == key.0 => match self.declared_type_of_ref(key) {
                        Some(t) => FlowRes::Ty(t),
                        None => FlowRes::Unknown,
                    },
                    Some(_) => self.flow_type_at(key, ante, depth + 1),
                    None => match target {
                        // destructuring assignment: conservatively treat as
                        // clobbering `key` (Stage-0 TODO: match the pattern's
                        // actual roots like the fact stack does)
                        Expr::Array { .. } | Expr::Object { .. } => {
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
                // initializer's value fits.
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
            // compound arithmetic assigns and `x++`/`x--`: the fact stack
            // only invalidates
            _ => FlowRes::Ty(declared),
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
        let Some(&fnode) = self.bind.flow_node.get(&nk) else {
            FLOW_VERIFY_NO_NODE.fetch_add(1, Ordering::Relaxed);
            return;
        };
        let dlen = self.diags.len();
        let resolved = self.get_flow_type_of_reference(key, fnode);
        let baseline = match fact {
            Some(t) => Some(t),
            None => self.declared_type_of_ref(key),
        };
        self.diags.truncate(dlen);
        let Some(baseline) = baseline else { return };
        match resolved {
            Some(t) if t == baseline => {
                FLOW_VERIFY_MATCH.fetch_add(1, Ordering::Relaxed);
            }
            Some(t) => {
                FLOW_VERIFY_MISMATCH.fetch_add(1, Ordering::Relaxed);
                if verbose() {
                    let file = self.files[self.current_file].0.clone();
                    let name = self.symbol(key.0).name.clone();
                    let path = if key.1.is_empty() {
                        String::new()
                    } else {
                        format!(".{}", key.1.join("."))
                    };
                    let f = self.display_type(baseline);
                    let r = self.display_type(t);
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
