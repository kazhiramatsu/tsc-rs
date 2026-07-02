//! Definite-assignment analysis (TS2454): a forward pass over a function body
//! (or the top level) tracking which annotated, initializer-less `let`/`var`
//! bindings are provably assigned on the active path. Split out of `stmts.rs`.

use crate::ast::*;
use crate::binder::{ScopeId, SymbolId};
use crate::checker::Checker;
use std::collections::HashSet;

#[derive(Clone, PartialEq)]
enum DaConst {
    Bool(bool),
    Str(String),
    Num(f64),
    Null,
    Undefined,
}

impl<'a> Checker<'a> {
    // ── definite-assignment analysis (TS2454) ──────────────────────────────
    // A forward pass over a function body (or the top level) tracking which
    // annotated, initializer-less `let`/`var` bindings are provably assigned on
    // the active path. Deliberately conservative: branch joins intersect, but
    // loops / try / switch keep every assignment made inside them (so the result
    // never *over*-reports on a path where the variable might in fact be set).

    pub fn analyze_definite_assignment(&mut self, stmts: &'a [Stmt], scope: ScopeId) {
        let saved_tracked = std::mem::take(&mut self.da.da_tracked);
        let saved_assigned = std::mem::take(&mut self.da.da_assigned);
        let saved_proven = std::mem::take(&mut self.da.da_proven);
        self.da_stmts(stmts, scope);
        self.da.da_tracked = saved_tracked;
        self.da.da_assigned = saved_assigned;
        self.da.da_proven = saved_proven;
    }

    fn da_var(&mut self, v: &'a VarStmt, scope: ScopeId) {
        // `declare let x: T` is ambient — tsc treats it as already-initialized,
        // so it must never enter the tracked set or 2454 fires spuriously
        // (the AMBIENT symbol flag is the source of truth; we check the
        // VarStmt's modifier directly here because no symbol may exist yet for
        // some patterns).
        if has_modifier(&v.modifiers, ModifierKind::Declare) {
            for d in &v.decls {
                if let Some(init) = &d.init {
                    self.da_expr(init, scope);
                }
                // mark each bound name as assigned so later reads do not fire
                if let Binding::Ident(_) = &d.name {
                    if let Some(sym) = self.bind.decl_symbol.get(&node_key(d)).copied() {
                        self.da.da_assigned.insert(sym);
                    }
                }
            }
            return;
        }
        for d in &v.decls {
            if let Some(init) = &d.init {
                self.da_expr(init, scope);
            }
            if let Binding::Ident(id) = &d.name {
                if let Some(sym) = self.bind.decl_symbol.get(&node_key(d)).copied() {
                    if d.init.is_some() {
                        self.da.da_assigned.insert(sym);
                    } else if matches!(v.kind, VarKind::Let | VarKind::Var)
                        && d.ty.is_some()
                        && !d.exclam
                    {
                        // track only when `undefined` is not a legal value of the
                        // declared type (else reading before assignment is harmless).
                        let dt =
                            self.resolve_type_cached(d.ty.as_ref().unwrap(), self.current_scope);
                        let undef = self.types.undefined;
                        if !self.types.is_any_or_error(dt) && !self.is_assignable_to(undef, dt) {
                            self.da.da_tracked.insert(sym, id.name.clone());
                            self.da.da_assigned.remove(&sym);
                        }
                    }
                }
            }
        }
    }

    /// process a statement list in order; returns true if control definitely
    /// leaves the list (return/throw/break/continue) before the end.
    fn da_stmts(&mut self, stmts: &'a [Stmt], scope: ScopeId) -> bool {
        for s in stmts {
            if self.da_stmt(s, scope) {
                return true;
            }
        }
        false
    }

    fn da_block_scope(&self, stmt: &'a Stmt) -> ScopeId {
        self.bind
            .node_scope
            .get(&node_key(stmt))
            .copied()
            .unwrap_or(self.current_scope)
    }

    fn da_stmt(&mut self, stmt: &'a Stmt, scope: ScopeId) -> bool {
        match stmt {
            Stmt::Var(v) => {
                self.da_var(v, scope);
                false
            }
            Stmt::Expr { expr, .. } => {
                self.da_expr(expr, scope);
                false
            }
            Stmt::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.da_expr(e, scope);
                }
                true
            }
            Stmt::Throw { expr, .. } => {
                self.da_expr(expr, scope);
                true
            }
            Stmt::Break { .. } | Stmt::Continue { .. } => true,
            Stmt::Block(b) => {
                let bscope = self.da_block_scope(stmt);
                self.da_stmts(&b.stmts, bscope)
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                self.da_expr(cond, scope);
                let entry = self.da.da_assigned.clone();
                let then_facts = self.da_condition_facts(cond, true, scope);
                let then_term = self.da_with_proven(then_facts, |this| this.da_stmt(then, scope));
                let then_assigned = std::mem::replace(&mut self.da.da_assigned, entry.clone());
                let (else_assigned, else_term) = if let Some(els) = els {
                    let else_facts = self.da_condition_facts(cond, false, scope);
                    let t = self.da_with_proven(else_facts, |this| this.da_stmt(els, scope));
                    (self.da.da_assigned.clone(), t)
                } else {
                    (entry.clone(), false)
                };
                // join: a binding is assigned afterwards only if every branch that
                // falls through assigns it.
                self.da.da_assigned = match (then_term, else_term) {
                    (true, true) => entry,
                    (true, false) => else_assigned,
                    (false, true) => then_assigned,
                    (false, false) => then_assigned
                        .intersection(&else_assigned)
                        .copied()
                        .collect(),
                };
                then_term && else_term
            }
            Stmt::While { cond, body, .. } => {
                self.da_expr(cond, scope);
                let facts = self.da_condition_facts(cond, true, scope);
                self.da_with_proven(facts, |this| this.da_loop_body(body, scope));
                false
            }
            Stmt::DoWhile { body, cond, .. } => {
                // runs at least once, so body assignments persist
                self.da_stmt(body, scope);
                self.da_expr(cond, scope);
                false
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                let fscope = self.da_block_scope(stmt);
                if let Some(init) = init {
                    match &**init {
                        ForInit::Var(v) => self.da_var(v, fscope),
                        ForInit::Expr(e) => self.da_expr(e, fscope),
                    }
                }
                if let Some(c) = cond {
                    self.da_expr(c, fscope);
                }
                let facts = cond
                    .as_ref()
                    .map(|c| self.da_condition_facts(c, true, fscope))
                    .unwrap_or_default();
                self.da_with_proven(facts, |this| this.da_loop_body(body, fscope));
                if let Some(i) = incr {
                    self.da_expr(i, fscope);
                }
                false
            }
            Stmt::ForIn { expr, body, .. } | Stmt::ForOf { expr, body, .. } => {
                let fscope = self.da_block_scope(stmt);
                self.da_expr(expr, fscope);
                self.da_loop_body(body, fscope);
                false
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                // a throw may abort `block` midway, so its assignments are not
                // guaranteed; union all branches (never over-report).
                self.da_stmts(&block.stmts, scope);
                if let Some(cc) = catch {
                    let cscope = self
                        .bind
                        .node_scope
                        .get(&node_key(&cc.block))
                        .copied()
                        .unwrap_or(scope);
                    if let Some(param) = &cc.param {
                        self.da_assign_binding(&param.name, cscope);
                    }
                    self.da_stmts(&cc.block.stmts, cscope);
                }
                if let Some(fin) = finally {
                    self.da_stmts(&fin.stmts, scope);
                }
                false
            }
            Stmt::Switch { expr, cases, .. } => {
                self.da_expr(expr, scope);
                let sscope = self.da_block_scope(stmt);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.da_expr(t, sscope);
                    }
                    self.da_stmts(&c.stmts, sscope);
                }
                false
            }
            Stmt::Labeled { stmt: inner, .. } => self.da_stmt(inner, scope),
            Stmt::With { body, .. } => self.da_stmt(body, scope),
            // declarations that introduce a new function/class scope are analyzed
            // independently when their own bodies are checked.
            _ => false,
        }
    }

    /// a loop body may execute zero times, but to stay FP-safe we keep whatever
    /// the body assigns (treating it as possibly-assigned afterwards).
    fn da_loop_body(&mut self, body: &'a Stmt, scope: ScopeId) {
        self.da_stmt(body, scope);
    }

    fn da_expr(&mut self, e: &'a Expr, scope: ScopeId) {
        match e {
            Expr::Ident(id) => self.da_read(id, scope),
            Expr::Binary {
                op: BinOp::AmpAmp,
                left,
                right,
                ..
            } => {
                self.da_expr(left, scope);
                if self.da_bool_const(left) != Some(false) {
                    let facts = self.da_condition_facts(left, true, scope);
                    self.da_with_proven(facts, |this| this.da_expr(right, scope));
                }
            }
            Expr::Binary {
                op: BinOp::BarBar,
                left,
                right,
                ..
            } => {
                self.da_expr(left, scope);
                if self.da_bool_const(left) != Some(true) {
                    let facts = self.da_condition_facts(left, false, scope);
                    self.da_with_proven(facts, |this| this.da_expr(right, scope));
                }
            }
            Expr::Binary {
                op, left, right, ..
            } if op.is_assignment() => {
                if matches!(op, BinOp::Assign) {
                    self.da_expr(right, scope);
                    self.da_assign_target(left, scope);
                } else {
                    // Logical assignments (`??=`, `||=`, `&&=`) are treated as
                    // assignments for definite-assignment — tsc does not flag the
                    // implicit read of the target — whereas an arithmetic compound
                    // (`+=`, `-=`, …) does read its target first.
                    let is_logical = matches!(
                        op,
                        BinOp::QuestionQuestionAssign | BinOp::BarBarAssign | BinOp::AmpAmpAssign
                    );
                    if is_logical {
                        self.da_expr(right, scope);
                        self.da_assign_target(left, scope);
                    } else {
                        self.da_expr(left, scope);
                        self.da_expr(right, scope);
                        if let Expr::Ident(id) = &**left {
                            if let Some(sym) = self.lookup_value(scope, &id.name) {
                                self.da.da_assigned.insert(sym);
                            }
                        }
                    }
                }
            }
            Expr::Binary { left, right, .. } => {
                self.da_expr(left, scope);
                self.da_expr(right, scope);
            }
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                self.da_expr(cond, scope);
                if let Some(value) = self.da_bool_const(cond) {
                    if value {
                        self.da_expr(when_true, scope);
                    } else {
                        self.da_expr(when_false, scope);
                    }
                    return;
                }
                let true_facts = self.da_condition_facts(cond, true, scope);
                self.da_with_proven(true_facts, |this| this.da_expr(when_true, scope));
                let false_facts = self.da_condition_facts(cond, false, scope);
                self.da_with_proven(false_facts, |this| this.da_expr(when_false, scope));
            }
            Expr::Call { callee, args, .. } => {
                self.da_expr(callee, scope);
                for a in args {
                    self.da_expr(a, scope);
                }
            }
            Expr::New { callee, args, .. } => {
                self.da_expr(callee, scope);
                if let Some(args) = args {
                    for a in args {
                        self.da_expr(a, scope);
                    }
                }
            }
            Expr::PropAccess { obj, .. } => self.da_expr(obj, scope),
            Expr::ElemAccess { obj, index, .. } => {
                self.da_expr(obj, scope);
                self.da_expr(index, scope);
            }
            Expr::Unary { operand, .. } | Expr::Update { operand, .. } => {
                self.da_expr(operand, scope)
            }
            Expr::Paren { inner, .. } => self.da_expr(inner, scope),
            Expr::Assertion { expr, .. }
            | Expr::NonNull { expr, .. }
            | Expr::Await { expr, .. }
            | Expr::Spread { expr, .. } => self.da_expr(expr, scope),
            Expr::Yield { expr, .. } => {
                if let Some(e) = expr {
                    self.da_expr(e, scope);
                }
            }
            Expr::Array { elements, .. } => {
                for el in elements {
                    self.da_expr(el, scope);
                }
            }
            Expr::Object { props, .. } => {
                for p in props {
                    match p {
                        ObjectProp::Property { value, .. } => self.da_expr(value, scope),
                        ObjectProp::Spread { expr, .. } => self.da_expr(expr, scope),
                        ObjectProp::Shorthand { name, .. } => self.da_read(name, scope),
                        _ => {}
                    }
                }
            }
            Expr::Template { parts, .. } => {
                for part in parts {
                    if let TemplatePart::Expr(e) = part {
                        self.da_expr(e, scope);
                    }
                }
            }
            Expr::ImportCall { args, .. } => {
                for a in args {
                    self.da_expr(a, scope);
                }
            }
            // nested functions/classes capture variables and run later: their
            // reads are not part of this body's flow.
            _ => {}
        }
    }

    /// Mark the targets of an assignment as definitely-assigned for the
    /// definite-assignment flow. A plain identifier target is recorded; a
    /// destructuring pattern (`[a, b] = …`, `({x} = …)`) recurses so each leaf
    /// target is recorded rather than mis-read as a use; a property or element
    /// target reads its object/index expressions but assigns no tracked local.
    fn da_assign_target(&mut self, target: &'a Expr, scope: ScopeId) {
        match target {
            Expr::Ident(id) => {
                if let Some(sym) = self.lookup_value(scope, &id.name) {
                    self.da.da_assigned.insert(sym);
                }
            }
            Expr::Paren { inner, .. } => self.da_assign_target(inner, scope),
            Expr::Array { elements, .. } => {
                for el in elements {
                    let mut tgt = el;
                    if let Expr::Binary {
                        op: BinOp::Assign,
                        left,
                        right,
                        ..
                    } = tgt
                    {
                        self.da_expr(right, scope);
                        tgt = left;
                    }
                    if let Expr::Spread { expr, .. } = tgt {
                        tgt = expr;
                    }
                    if matches!(tgt, Expr::Missing { .. }) {
                        continue;
                    }
                    self.da_assign_target(tgt, scope);
                }
            }
            Expr::Object { props, .. } => {
                for p in props {
                    match p {
                        ObjectProp::Shorthand { name, .. } => {
                            if let Some(sym) = self.lookup_value(scope, &name.name) {
                                self.da.da_assigned.insert(sym);
                            }
                        }
                        ObjectProp::Property { value, .. } => {
                            let mut tgt = value;
                            if let Expr::Binary {
                                op: BinOp::Assign,
                                left,
                                right,
                                ..
                            } = tgt
                            {
                                self.da_expr(right, scope);
                                tgt = left;
                            }
                            self.da_assign_target(tgt, scope);
                        }
                        ObjectProp::Spread { expr, .. } => self.da_assign_target(expr, scope),
                        ObjectProp::Method(_) => {}
                    }
                }
            }
            other => self.da_expr(other, scope),
        }
    }

    fn da_assign_binding(&mut self, binding: &'a Binding, scope: ScopeId) {
        match binding {
            Binding::Ident(id) => {
                if let Some(sym) = self.lookup_value(scope, &id.name) {
                    self.da.da_assigned.insert(sym);
                }
            }
            Binding::Object(p) => {
                for prop in &p.props {
                    self.da_assign_binding(&prop.binding, scope);
                    if let Some(default) = &prop.default {
                        self.da_expr(default, scope);
                    }
                }
                if let Some(rest) = &p.rest {
                    self.da_assign_binding(rest, scope);
                }
            }
            Binding::Array(p) => {
                for elem in p.elements.iter().flatten() {
                    self.da_assign_binding(&elem.binding, scope);
                    if let Some(default) = &elem.default {
                        self.da_expr(default, scope);
                    }
                }
            }
        }
    }

    fn da_union(mut left: HashSet<SymbolId>, right: HashSet<SymbolId>) -> HashSet<SymbolId> {
        left.extend(right);
        left
    }

    fn da_intersection(left: HashSet<SymbolId>, right: HashSet<SymbolId>) -> HashSet<SymbolId> {
        left.intersection(&right).copied().collect()
    }

    fn da_with_proven<R>(&mut self, facts: HashSet<SymbolId>, f: impl FnOnce(&mut Self) -> R) -> R {
        if facts.is_empty() {
            return f(self);
        }
        self.da.da_proven.push(facts);
        let r = f(self);
        self.da.da_proven.pop();
        r
    }

    fn da_is_assigned_or_proven(&self, sym: SymbolId) -> bool {
        self.da.da_assigned.contains(&sym)
            || self
                .da
                .da_proven
                .iter()
                .rev()
                .any(|frame| frame.contains(&sym))
    }

    fn da_root_symbol(&self, e: &'a Expr, scope: ScopeId) -> Option<SymbolId> {
        match e {
            Expr::Ident(id) => self.lookup_value(scope, &id.name),
            Expr::Paren { inner, .. } => self.da_root_symbol(inner, scope),
            Expr::PropAccess { obj, .. } | Expr::ElemAccess { obj, .. } => {
                self.da_root_symbol(obj, scope)
            }
            _ => None,
        }
    }

    fn da_typeof_comparison(
        &self,
        left: &'a Expr,
        right: &'a Expr,
        scope: ScopeId,
    ) -> Option<SymbolId> {
        match (left, right) {
            (
                Expr::Unary {
                    op: UnaryOp::Typeof,
                    operand,
                    ..
                },
                Expr::StrLit { .. },
            ) => self.da_root_symbol(operand, scope),
            (
                Expr::StrLit { .. },
                Expr::Unary {
                    op: UnaryOp::Typeof,
                    operand,
                    ..
                },
            ) => self.da_root_symbol(operand, scope),
            _ => None,
        }
    }

    fn da_predicate_call_fact(
        &mut self,
        callee: &'a Expr,
        args: &'a [Expr],
        scope: ScopeId,
    ) -> Option<SymbolId> {
        let Expr::Ident(id) = callee else {
            return None;
        };
        let sym = self.lookup_value(scope, &id.name)?;
        let sym = if self.symbol(sym).flags & crate::binder::flags::ALIAS != 0 {
            self.resolve_alias_chain(sym)
        } else {
            sym
        };
        let f = self.symbol(sym).decls.iter().find_map(|d| match d {
            crate::binder::Decl::Func(f) => Some(*f),
            _ => None,
        })?;
        let Some(TypeNode::Predicate {
            param_name,
            asserts: false,
            ty: Some(_),
            ..
        }) = f.return_type.as_ref()
        else {
            return None;
        };
        if param_name.name == "this" {
            return None;
        }
        let idx = f
            .params
            .iter()
            .position(|p| p.name.as_ident().is_some_and(|i| i.name == param_name.name))?;
        self.da_root_symbol(args.get(idx)?, scope)
    }

    fn da_const_value(&self, e: &'a Expr) -> Option<DaConst> {
        match e {
            Expr::Paren { inner, .. } => self.da_const_value(inner),
            Expr::BoolLit { value, .. } => Some(DaConst::Bool(*value)),
            Expr::StrLit { value, .. } => Some(DaConst::Str(value.to_str_lossy().into_owned())),
            Expr::NumLit { value, .. } => Some(DaConst::Num(*value)),
            Expr::NullLit { .. } => Some(DaConst::Null),
            Expr::Ident(id) if id.name == "undefined" => Some(DaConst::Undefined),
            Expr::Unary {
                op: UnaryOp::Bang,
                operand,
                ..
            } => self.da_bool_const(operand).map(|v| DaConst::Bool(!v)),
            Expr::Unary {
                op: UnaryOp::Typeof,
                operand,
                ..
            } => {
                let s = match self.da_const_value(operand)? {
                    DaConst::Bool(_) => "boolean",
                    DaConst::Str(_) => "string",
                    DaConst::Num(_) => "number",
                    DaConst::Null => "object",
                    DaConst::Undefined => "undefined",
                };
                Some(DaConst::Str(s.to_string()))
            }
            Expr::Binary {
                op, left, right, ..
            } => match op {
                BinOp::AmpAmp => match self.da_bool_const(left) {
                    Some(false) => Some(DaConst::Bool(false)),
                    Some(true) => self.da_bool_const(right).map(DaConst::Bool),
                    None => None,
                },
                BinOp::BarBar => match self.da_bool_const(left) {
                    Some(true) => Some(DaConst::Bool(true)),
                    Some(false) => self.da_bool_const(right).map(DaConst::Bool),
                    None => None,
                },
                BinOp::EqEq | BinOp::EqEqEq | BinOp::NotEq | BinOp::NotEqEq => {
                    let loose = matches!(op, BinOp::EqEq | BinOp::NotEq);
                    let mut eq = self.da_const_eq(
                        self.da_const_value(left)?,
                        self.da_const_value(right)?,
                        loose,
                    );
                    if matches!(op, BinOp::NotEq | BinOp::NotEqEq) {
                        eq = !eq;
                    }
                    Some(DaConst::Bool(eq))
                }
                BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                    let (Some(DaConst::Num(l)), Some(DaConst::Num(r))) =
                        (self.da_const_value(left), self.da_const_value(right))
                    else {
                        return None;
                    };
                    let v = match op {
                        BinOp::Lt => l < r,
                        BinOp::Gt => l > r,
                        BinOp::LtEq => l <= r,
                        BinOp::GtEq => l >= r,
                        _ => unreachable!(),
                    };
                    Some(DaConst::Bool(v))
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn da_const_eq(&self, left: DaConst, right: DaConst, loose: bool) -> bool {
        if loose
            && matches!(
                (&left, &right),
                (DaConst::Null, DaConst::Undefined) | (DaConst::Undefined, DaConst::Null)
            )
        {
            return true;
        }
        left == right
    }

    fn da_bool_const(&self, e: &'a Expr) -> Option<bool> {
        match self.da_const_value(e)? {
            DaConst::Bool(v) => Some(v),
            _ => None,
        }
    }

    fn da_condition_facts(
        &mut self,
        e: &'a Expr,
        sense: bool,
        scope: ScopeId,
    ) -> HashSet<SymbolId> {
        match e {
            Expr::Paren { inner, .. } => self.da_condition_facts(inner, sense, scope),
            Expr::Unary {
                op: UnaryOp::Bang,
                operand,
                ..
            } => self.da_condition_facts(operand, !sense, scope),
            Expr::Call { callee, args, .. } if sense => self
                .da_predicate_call_fact(callee, args, scope)
                .into_iter()
                .collect(),
            Expr::Binary {
                op: BinOp::AmpAmp,
                left,
                right,
                ..
            } if sense => {
                let left_true = self.da_condition_facts(left, true, scope);
                let right_true = self.da_condition_facts(right, true, scope);
                Self::da_union(left_true, right_true)
            }
            Expr::Binary {
                op: BinOp::AmpAmp,
                left,
                right,
                ..
            } => {
                let left_false = self.da_condition_facts(left, false, scope);
                let left_true = self.da_condition_facts(left, true, scope);
                let right_false = self.da_condition_facts(right, false, scope);
                Self::da_intersection(left_false, Self::da_union(left_true, right_false))
            }
            Expr::Binary {
                op: BinOp::BarBar,
                left,
                right,
                ..
            } if sense => {
                let left_true = self.da_condition_facts(left, true, scope);
                let left_false = self.da_condition_facts(left, false, scope);
                let right_true = self.da_condition_facts(right, true, scope);
                Self::da_intersection(left_true, Self::da_union(left_false, right_true))
            }
            Expr::Binary {
                op: BinOp::BarBar,
                left,
                right,
                ..
            } => {
                let left_false = self.da_condition_facts(left, false, scope);
                let right_false = self.da_condition_facts(right, false, scope);
                Self::da_union(left_false, right_false)
            }
            Expr::Binary {
                op, left, right, ..
            } if matches!(
                op,
                BinOp::EqEq | BinOp::EqEqEq | BinOp::NotEq | BinOp::NotEqEq
            ) =>
            {
                let eq_sense = if matches!(op, BinOp::EqEq | BinOp::EqEqEq) {
                    sense
                } else {
                    !sense
                };
                if let Some(sym) = self.da_typeof_comparison(left, right, scope) {
                    return if eq_sense {
                        HashSet::from([sym])
                    } else {
                        HashSet::new()
                    };
                }
                let mut facts = HashSet::new();
                if let Some(sym) = self.da_root_symbol(left, scope) {
                    facts.insert(sym);
                }
                if let Some(sym) = self.da_root_symbol(right, scope) {
                    facts.insert(sym);
                }
                facts
            }
            Expr::Binary {
                op: BinOp::Instanceof,
                left,
                ..
            } if sense => self.da_root_symbol(left, scope).into_iter().collect(),
            Expr::Binary {
                op: BinOp::In,
                right,
                ..
            } if sense => self.da_root_symbol(right, scope).into_iter().collect(),
            _ if sense => self.da_root_symbol(e, scope).into_iter().collect(),
            _ => HashSet::new(),
        }
    }

    fn da_read(&mut self, id: &'a Ident, scope: ScopeId) {
        if self.da.da_tracked.is_empty() {
            return;
        }
        if let Some(sym) = self.lookup_value(scope, &id.name) {
            if self.da.da_tracked.contains_key(&sym) && !self.da_is_assigned_or_proven(sym) {
                let name = self.da.da_tracked.get(&sym).cloned().unwrap_or_default();
                self.report_used_before_assigned(id.span, name);
            }
        }
    }
}
