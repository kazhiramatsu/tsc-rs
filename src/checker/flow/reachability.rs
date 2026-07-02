//! Reachability / termination analysis: type-aware "does this body always
//! exit" checks (driving return-value diagnostics) and the purely syntactic
//! `*_definitely_terminates` helpers. Split out of `exprs.rs`.

use crate::ast::*;
use crate::checker::Checker;
use crate::diagnostics::gen;
use crate::types::{TypeId, TypeKind};

impl<'a> Checker<'a> {
    /// type-aware termination check: like `stmt_definitely_terminates`, but a
    /// `switch` with no `default` still terminates when its case labels cover
    /// every member of the (finite) discriminant type.
    pub(crate) fn body_terminates(&mut self, stmts: &'a [Stmt]) -> bool {
        stmts.iter().any(|s| self.stmt_terminates(s))
    }

    fn stmt_terminates(&mut self, s: &'a Stmt) -> bool {
        match s {
            Stmt::Return { .. } | Stmt::Throw { .. } => true,
            Stmt::Block(b) => self.body_terminates(&b.stmts),
            Stmt::If {
                then,
                els: Some(els),
                ..
            } => self.stmt_terminates(then) && self.stmt_terminates(els),
            Stmt::Switch { cases, .. } => {
                !cases.is_empty()
                    && (cases.iter().any(|c| c.test.is_none())
                        || self
                            .flow
                            .exhaustive_switches
                            .contains(&crate::ast::node_key(s)))
                    && cases.iter().all(|c| self.body_terminates(&c.stmts))
            }
            Stmt::Try { block, finally, .. } => {
                self.body_terminates(&block.stmts)
                    || finally
                        .as_ref()
                        .map_or(false, |fb| self.body_terminates(&fb.stmts))
            }
            Stmt::While { cond, .. } => matches!(cond, Expr::BoolLit { value: true, .. }),
            Stmt::DoWhile { body, cond, .. } => {
                self.stmt_terminates(body) || matches!(cond, Expr::BoolLit { value: true, .. })
            }
            _ => false,
        }
    }

    pub(crate) fn check_return_paths(
        &mut self,
        f: &'a FunctionLike,
        declared: TypeId,
        b: &'a Block,
    ) {
        // Generators satisfy their declared `Generator`/`Iterator` type through
        // `yield`, not `return`, so the "must return a value" analysis does not
        // apply. (Async functions are handled separately by the caller.)
        if f.is_generator {
            return;
        }
        if matches!(
            self.types.kind(declared),
            TypeKind::Void | TypeKind::Any | TypeKind::Undefined | TypeKind::Error
        ) {
            return;
        }
        if let TypeKind::Union(ms) = self.types.kind(declared) {
            if ms
                .iter()
                .any(|&m| matches!(self.types.kind(m), TypeKind::Undefined | TypeKind::Void))
            {
                return;
            }
        }
        let has_return_with_expr = contains_return_with_expr(&b.stmts);
        let span = f.return_type.as_ref().map(|rt| rt.span()).unwrap_or(f.span);
        if matches!(self.types.kind(declared), TypeKind::Never) {
            if !self.body_terminates(&b.stmts) {
                self.error_at(
                    span,
                    &gen::A_function_returning_never_cannot_have_a_reachable_end_point,
                    &[],
                );
            }
            return;
        }
        if !has_return_with_expr {
            // no returns at all
            if !self.body_terminates(&b.stmts) {
                self.error_at(
                    span,
                    &gen::A_function_whose_declared_type_is_neither_undefined_void_nor_any_must_return_a_value,
                    &[],
                );
            }
        } else if !self.body_terminates(&b.stmts) {
            self.error_at(
                span,
                &gen::Function_lacks_ending_return_statement_and_return_type_does_not_include_undefined,
                &[],
            );
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
