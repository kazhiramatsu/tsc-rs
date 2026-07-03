//! Control-flow analysis: definite assignment (more to follow:
//! reachability, narrowing, flow state).

pub mod narrowing;
pub mod reachability;
pub mod resolver;

use crate::ast::*;
use crate::binder::ScopeId;
use crate::checker::{Checker, RefKey};
use crate::types::TypeId;
use std::collections::HashMap;

impl<'a> Checker<'a> {
    // ── narrowing facts (populated in stmts/exprs; full engine in P8) ──────

    pub fn fact_for(&self, key: &RefKey) -> Option<TypeId> {
        // hermetic scaffolds: inside a resolver scaffold only the frames the
        // scaffold itself pushed are visible — the read site's lexical
        // residue would describe a different program point
        let base = if self.fresolve.quiet > 0 {
            self.fresolve.scaffold_base.last().copied().unwrap_or(0)
        } else {
            0
        };
        for frame in self.flow.facts[base..].iter().rev() {
            if let Some(&t) = frame.get(key) {
                return Some(t);
            }
        }
        None
    }

    pub fn set_fact(&mut self, key: RefKey, t: TypeId) {
        // The base (module-level) frame is pushed once at construction and every
        // narrowing scope balances via `narrowed`, so a frame is always present.
        debug_assert!(!self.flow.facts.is_empty(), "facts base frame drained");
        if let Some(frame) = self.flow.facts.last_mut() {
            frame.insert(key, t);
        }
    }

    /// Run `f` inside a fresh narrowing frame, guaranteeing the frame is removed
    /// afterwards however `f` returns. Fact-stack balance is therefore
    /// structural — there is deliberately no public push/pop pair to hand-balance
    /// (an earlier hand-balanced `||` path over-popped and drained the base
    /// frame, panicking `set_fact`). All control-flow narrowing goes through
    /// here.
    pub fn narrowed<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.flow.facts.push(HashMap::new());
        let r = f(self);
        self.flow.facts.pop();
        r
    }

    /// Resolve the type annotations of uninitialized `let`/`var` declarators
    /// ahead of statement checking, in source order. The retired forward
    /// definite-assignment pass did this as a side effect of computing its
    /// candidate set; keeping the eager resolution preserves diagnostic
    /// order (TS2314 on `var g: G;` and friends) and keeps those
    /// diagnostics from being first-emitted inside a rolled-back narrowing
    /// scaffold, which would swallow them. Nested function/class bodies
    /// prime themselves when their bodies are checked.
    pub fn prime_declarator_annotations(&mut self, stmts: &'a [Stmt], scope: ScopeId) {
        for stmt in stmts {
            self.prime_stmt(stmt, scope);
        }
    }

    fn prime_stmt(&mut self, stmt: &'a Stmt, scope: ScopeId) {
        let block_scope = |c: &Self| {
            c.bind
                .node_scope
                .get(&node_key(stmt))
                .copied()
                .unwrap_or(scope)
        };
        match stmt {
            Stmt::Var(v) => self.prime_var(v, scope),
            Stmt::Block(b) => {
                let s = block_scope(self);
                self.prime_declarator_annotations(&b.stmts, s);
            }
            Stmt::If { then, els, .. } => {
                self.prime_stmt(then, scope);
                if let Some(e) = els {
                    self.prime_stmt(e, scope);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                self.prime_stmt(body, scope);
            }
            Stmt::For { init, body, .. } => {
                let s = block_scope(self);
                if let Some(init) = init {
                    if let ForInit::Var(v) = &**init {
                        self.prime_var(v, s);
                    }
                }
                self.prime_stmt(body, s);
            }
            Stmt::ForIn { body, .. } | Stmt::ForOf { body, .. } => {
                let s = block_scope(self);
                self.prime_stmt(body, s);
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                self.prime_declarator_annotations(&block.stmts, scope);
                if let Some(cc) = catch {
                    let cs = self
                        .bind
                        .node_scope
                        .get(&node_key(&cc.block))
                        .copied()
                        .unwrap_or(scope);
                    self.prime_declarator_annotations(&cc.block.stmts, cs);
                }
                if let Some(fin) = finally {
                    self.prime_declarator_annotations(&fin.stmts, scope);
                }
            }
            Stmt::Switch { cases, .. } => {
                let s = block_scope(self);
                for c in cases {
                    self.prime_declarator_annotations(&c.stmts, s);
                }
            }
            Stmt::Labeled { stmt: inner, .. } | Stmt::With { body: inner, .. } => {
                self.prime_stmt(inner, scope);
            }
            // nested function/class scopes prime their own bodies
            _ => {}
        }
    }

    fn prime_var(&mut self, v: &'a VarStmt, scope: ScopeId) {
        if has_modifier(&v.modifiers, ModifierKind::Declare) {
            return;
        }
        for d in &v.decls {
            if matches!(v.kind, VarKind::Let | VarKind::Var)
                && d.init.is_none()
                && !d.exclam
            {
                if let Some(ty) = &d.ty {
                    let dt = self.resolve_type_cached(ty, scope);
                    // the retired pass also probed assignability, which
                    // resolves the annotation's SHAPE eagerly — inline
                    // object-type annotations emit their member diagnostics
                    // (TS2314 on `{ x: C }`) here, in source order
                    if !self.types.is_any_or_error(dt) {
                        let undef = self.types.undefined;
                        let _ = self.is_assignable_to(undef, dt);
                    }
                }
            }
        }
    }
}
