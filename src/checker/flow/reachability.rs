//! Reachability / termination analysis: type-aware "does this body always
//! exit" checks (driving return-value diagnostics) and the purely syntactic
//! `*_definitely_terminates` helpers. Split out of `exprs.rs`.

use crate::ast::*;
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// tsc getAwaitedTypeNoAlias for the return-path tree, without error
    /// reporting: `None` plays errorType's role (the caller exempts) — a
    /// thenable whose promised type cannot be extracted is not analyzable.
    pub(crate) fn awaited_for_return_paths(&mut self, t: TypeId, depth: u32) -> Option<TypeId> {
        if depth > 8 {
            return None;
        }
        match self.types.kind(t) {
            TypeKind::Any | TypeKind::Error => return Some(t),
            TypeKind::Union(ms) => {
                let ms = ms.clone();
                let mut out = Vec::new();
                for m in ms {
                    out.push(self.awaited_for_return_paths(m, depth + 1)?);
                }
                return Some(self.types.union(out));
            }
            _ => {}
        }
        if let Some(p) = self.promised_type_of(t) {
            if p == t {
                return None;
            }
            return self.awaited_for_return_paths(p, depth + 1);
        }
        if self.is_thenable_type(t) {
            return None;
        }
        Some(t)
    }

    /// tsc getPromisedTypeOfPromise (no error reporting, no `this`-parameter
    /// filtering): a global `Promise<T>` reference yields T directly; anything
    /// else goes through the structural `then` unwrap — the first parameter
    /// type of the non-nullish `onfulfilled` callback.
    fn promised_type_of(&mut self, t: TypeId) -> Option<TypeId> {
        if let TypeKind::Ref(sym, args) = self.types.kind(t) {
            if !args.is_empty() && self.symbol(*sym).name == "Promise" {
                return Some(args[0]);
            }
        }
        let then_fn = self.prop_of_type(t, "then")?;
        if matches!(self.types.kind(then_fn), TypeKind::Any) {
            return None;
        }
        let sigs = self.call_signatures_of(then_fn);
        if sigs.is_empty() {
            return None;
        }
        let firsts: Vec<TypeId> = sigs.iter().map(|&s| self.first_param_type(s)).collect();
        let cb_union = self.types.union(firsts);
        let onfulfilled = self.facts_filter(
            cb_union,
            crate::checker::operators::facts::NE_UNDEFINED_OR_NULL,
            false,
        );
        if matches!(self.types.kind(onfulfilled), TypeKind::Any) {
            return None;
        }
        let cb_sigs = self.call_signatures_of(onfulfilled);
        if cb_sigs.is_empty() {
            return None;
        }
        let vals: Vec<TypeId> = cb_sigs.iter().map(|&s| self.first_param_type(s)).collect();
        Some(self.types.union(vals))
    }

    /// tsc isThenableType: a callable non-nullish `then` property.
    fn is_thenable_type(&mut self, t: TypeId) -> bool {
        let Some(then_fn) = self.prop_of_type(t, "then") else {
            return false;
        };
        let nn = self.facts_filter(
            then_fn,
            crate::checker::operators::facts::NE_UNDEFINED_OR_NULL,
            false,
        );
        !self.call_signatures_of(nn).is_empty()
    }

    /// tsc getTypeOfFirstParameterOfSignature: `never` when parameterless.
    fn first_param_type(&self, s: crate::types::SigId) -> TypeId {
        self.types
            .sig(s)
            .params
            .first()
            .map(|p| p.ty)
            .unwrap_or(self.types.never)
    }

    /// `maybeTypeOfKind(type, Void)`: a `void` anywhere in a union or
    /// intersection exempts the function from return-path analysis.
    fn maybe_void_type(&self, t: TypeId) -> bool {
        match self.types.kind(t) {
            TypeKind::Void => true,
            TypeKind::Union(ms) | TypeKind::Intersection(ms) => {
                ms.iter().any(|&m| self.maybe_void_type(m))
            }
            _ => false,
        }
    }

    /// tsc checkAllCodePathsInNonVoidFunctionReturnOrThrow: the ordered
    /// decision tree over the CFG. `declared` is the (async-unwrapped) return
    /// annotation type; `None` runs the noImplicitReturns-only path.
    pub(crate) fn check_return_paths(
        &mut self,
        f: &'a FunctionLike,
        declared: Option<TypeId>,
        b: &'a Block,
    ) {
        // tsc has no checkAllCodePaths call site for constructors or setters.
        if matches!(f.kind, FuncKind::Constructor | FuncKind::Setter) {
            return;
        }
        // Generators satisfy their declared `Generator`/`Iterator` type through
        // `yield`; the iteration-return unwrap is not implemented.
        if f.is_generator {
            return;
        }
        // A `void` member anywhere exempts; `any`/`undefined` only at top
        // level (`number | undefined` is NOT exempt and reports 2355/7030).
        if let Some(ty) = declared {
            if self.maybe_void_type(ty)
                || matches!(
                    self.types.kind(ty),
                    TypeKind::Any | TypeKind::Undefined | TypeKind::Error
                )
            {
                return;
            }
        }
        // tsc functionHasImplicitReturn: the body's fall-through end must be
        // reachable — lazily, so a tail never-call exempts the function.
        let fk = crate::ast::node_key(f);
        let Some(&end) = self.bind.fn_fallthrough_flow.get(&fk) else {
            return;
        };
        if !self.is_reachable_flow(end) {
            return;
        }
        // tsc HasExplicitReturn: any syntactic `return` bound in the container
        // (value-less counts; its own reachability does not).
        let has_explicit_return = self.bind.fn_returns.get(&fk).map_or(false, |v| !v.is_empty());
        let span = f
            .return_type
            .as_ref()
            .map(|rt| rt.span())
            .or_else(|| f.name.as_ref().map(|n| n.span()))
            .unwrap_or(f.span);
        match declared {
            Some(ty) if matches!(self.types.kind(ty), TypeKind::Never) => {
                self.error_at(
                    span,
                    &gen::A_function_returning_never_cannot_have_a_reachable_end_point,
                    &[],
                );
            }
            Some(_) if !has_explicit_return => {
                self.error_at(
                    span,
                    &gen::A_function_whose_declared_type_is_neither_undefined_void_nor_any_must_return_a_value,
                    &[],
                );
            }
            Some(ty)
                if self.options.strict_null_checks() && {
                    let undef = self.types.undefined;
                    !self.is_assignable_to(undef, ty)
                } =>
            {
                self.error_at(
                    span,
                    &gen::Function_lacks_ending_return_statement_and_return_type_does_not_include_undefined,
                    &[],
                );
            }
            _ if self.options.no_implicit_returns => {
                if declared.is_none() {
                    // Unannotated: no returns at all is fine; tsc also exempts
                    // when the inferred return type is undefined/void/any —
                    // value-less-returns-only approximates that here.
                    if !has_explicit_return || !contains_return_with_expr(&b.stmts) {
                        return;
                    }
                }
                self.error_at(span, &gen::Not_all_code_paths_return_a_value, &[]);
            }
            _ => {}
        }
    }
}

pub(crate) fn contains_return_with_expr(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_contains_return_with_expr)
}

fn stmt_contains_return_with_expr(s: &Stmt) -> bool {
    match s {
        Stmt::Return { expr, .. } => expr.is_some(),
        Stmt::Block(b) => contains_return_with_expr(&b.stmts),
        Stmt::If { then, els, .. } => {
            stmt_contains_return_with_expr(then)
                || els
                    .as_ref()
                    .map_or(false, |e| stmt_contains_return_with_expr(e))
        }
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForIn { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::Labeled { stmt: body, .. } => stmt_contains_return_with_expr(body),
        Stmt::Try {
            block,
            catch,
            finally,
            ..
        } => {
            contains_return_with_expr(&block.stmts)
                || catch
                    .as_ref()
                    .map_or(false, |c| contains_return_with_expr(&c.block.stmts))
                || finally
                    .as_ref()
                    .map_or(false, |f| contains_return_with_expr(&f.stmts))
        }
        Stmt::Switch { cases, .. } => cases.iter().any(|c| contains_return_with_expr(&c.stmts)),
        _ => false,
    }
}

/// definite termination: every code path ends in return/throw
pub fn body_definitely_terminates(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_definitely_terminates)
}

pub fn stmt_definitely_terminates(s: &Stmt) -> bool {
    match s {
        Stmt::Return { .. } | Stmt::Throw { .. } => true,
        Stmt::Block(b) => body_definitely_terminates(&b.stmts),
        Stmt::If {
            then,
            els: Some(els),
            ..
        } => stmt_definitely_terminates(then) && stmt_definitely_terminates(els),
        Stmt::Switch { cases, .. } => {
            !cases.is_empty()
                && cases.iter().any(|c| c.test.is_none())
                && cases.iter().all(|c| body_definitely_terminates(&c.stmts))
        }
        Stmt::Try { block, finally, .. } => {
            body_definitely_terminates(&block.stmts)
                || finally
                    .as_ref()
                    .map_or(false, |f| body_definitely_terminates(&f.stmts))
        }
        Stmt::While { cond, .. } => matches!(cond, Expr::BoolLit { value: true, .. }),
        Stmt::DoWhile { body, cond, .. } => {
            // the body always executes at least once
            stmt_definitely_terminates(body) || matches!(cond, Expr::BoolLit { value: true, .. })
        }
        _ => false,
    }
}
