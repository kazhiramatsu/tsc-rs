//! Control-flow analysis: definite assignment (more to follow:
//! reachability, narrowing, flow state).

pub mod definite_assignment;
pub mod narrowing;
pub mod reachability;

use crate::binder::SymbolId;
use crate::checker::{Checker, RefKey};
use crate::types::TypeId;
use std::collections::{HashMap, HashSet};

impl<'a> Checker<'a> {
    // ── narrowing facts (populated in stmts/exprs; full engine in P8) ──────

    pub fn fact_for(&self, key: &RefKey) -> Option<TypeId> {
        for frame in self.flow.facts.iter().rev() {
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

    pub fn da_fact_for(&self, key: &RefKey) -> bool {
        self.flow
            .da_facts
            .iter()
            .rev()
            .any(|frame| frame.contains(key))
    }

    pub fn set_fact_for_definite_assignment(&mut self, key: &RefKey) {
        if !self.flow.record_da_facts {
            return;
        }
        debug_assert!(
            !self.flow.da_facts.is_empty(),
            "da facts base frame drained"
        );
        if let Some(frame) = self.flow.da_facts.last_mut() {
            frame.insert(key.clone());
        }
    }

    pub fn invalidate_fact_root(&mut self, root: SymbolId) {
        for frame in self.flow.facts.iter_mut() {
            frame.retain(|k, _| k.0 != root);
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
        self.flow.da_facts.push(HashSet::new());
        let r = f(self);
        self.flow.facts.pop();
        self.flow.da_facts.pop();
        r
    }
}
