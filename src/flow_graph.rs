//! Tier-2 control-flow graph builder (Stage 0).
//!
//! Runs after `bind()`, before the `BindResult` is frozen in an `Arc`,
//! populating `bind.flow_nodes` + `bind.flow_node`. A syntax-only second pass
//! mirroring the binder's evaluation-order walk (kept out of binder.rs to
//! isolate the new logic; the extra traversal is cheap). During Stage 0 the
//! graph is BUILT but NOT consumed by any diagnostic — `get_flow_type_of_reference`
//! reads it only under the `TSRS_FLOW_VERIFY` dark-launch flag — so program
//! output is unchanged regardless of contents.
//!
//! Antecedents point backward toward the function/program `Start`. Reference
//! expressions are keyed to the flow node in effect at them: identifiers by
//! `node_key` of the inner `Ident` (that is what the checker's read seam has
//! in hand), `this` / property accesses by `node_key` of the `Expr`. Nodes
//! that carry expressions also carry the `ScopeId` in effect (from the
//! binder's `node_scope`), so the resolver can re-resolve names in them from
//! any calling context. Coverage is filled incrementally; unhandled
//! constructs thread flow linearly, which is SAFE: a reference then resolves
//! against a wider antecedent (never a wrong narrowing).

use crate::ast::*;
use crate::binder::{BindResult, FlowNode, FlowNodeId, ScopeId};
use crate::text::SourceText;
use std::collections::HashMap;

pub fn build<'a>(bind: &mut BindResult<'a>, files: &'a [(String, SourceText, SourceFileAst)]) {
    let mut b = FlowBuilder {
        nodes: Vec::new(),
        map: HashMap::new(),
        brk: Vec::new(),
        cont: Vec::new(),
        scopes: &bind.node_scope,
        scope: bind.global_scope,
    };
    let start = b.new_node(FlowNode::Start);
    for (i, (_name, _text, ast)) in files.iter().enumerate() {
        b.scope = bind
            .module_scope
            .get(&i)
            .copied()
            .unwrap_or(bind.global_scope);
        b.build_stmts(&ast.stmts, start);
    }
    let FlowBuilder { nodes, map, .. } = b;
    bind.flow_nodes = nodes;
    bind.flow_node = map;
}

struct FlowBuilder<'a, 'b> {
    nodes: Vec<FlowNode<'a>>,
    map: HashMap<usize, FlowNodeId>,
    /// break-target labels (a `Branch` per enclosing loop/switch), inner last.
    brk: Vec<FlowNodeId>,
    /// continue-target labels, inner last.
    cont: Vec<FlowNodeId>,
    /// the binder's container-node → scope map
    scopes: &'b HashMap<usize, ScopeId>,
    /// scope in effect at the point being built
    scope: ScopeId,
}

impl<'a> FlowBuilder<'a, '_> {
    fn new_node(&mut self, n: FlowNode<'a>) -> FlowNodeId {
        let id = FlowNodeId(self.nodes.len() as u32);
        self.nodes.push(n);
        id
    }

    fn branch(&mut self, antes: Vec<FlowNodeId>) -> FlowNodeId {
        self.new_node(FlowNode::Branch(antes))
    }

    /// A fresh `Start` — a function-body entry, or the "unreachable" flow after a
    /// return/throw/break/continue.
    fn start(&mut self) -> FlowNodeId {
        self.new_node(FlowNode::Start)
    }

    /// Append an antecedent to an existing `Branch`/loop label.
    fn add_ante(&mut self, label: FlowNodeId, ante: FlowNodeId) {
        if let FlowNode::Branch(antes) = &mut self.nodes[label.0 as usize] {
            antes.push(ante);
        }
    }

    fn cond(&mut self, cond: &'a Expr, sense: bool, ante: FlowNodeId) -> FlowNodeId {
        self.new_node(FlowNode::Cond {
            cond,
            sense,
            scope: self.scope,
            ante,
        })
    }

    /// Enter the scope the binder recorded for container node `key` (if any),
    /// returning the previous scope for the caller to restore.
    fn enter(&mut self, key: usize) -> ScopeId {
        let saved = self.scope;
        if let Some(&s) = self.scopes.get(&key) {
            self.scope = s;
        }
        saved
    }

    fn build_stmts(&mut self, stmts: &'a [Stmt], mut flow: FlowNodeId) -> FlowNodeId {
        for s in stmts {
            flow = self.build_stmt(s, flow);
        }
        flow
    }

    fn build_var(&mut self, v: &'a VarStmt, mut flow: FlowNodeId) -> FlowNodeId {
        for d in &v.decls {
            if let Some(init) = &d.init {
                let after = self.build_expr(init, flow);
                flow = self.new_node(FlowNode::Init {
                    decl: d,
                    scope: self.scope,
                    ante: after,
                });
            }
        }
        flow
    }

    fn build_stmt(&mut self, stmt: &'a Stmt, flow: FlowNodeId) -> FlowNodeId {
        match stmt {
            Stmt::Var(v) => self.build_var(v, flow),
            Stmt::Expr { expr, .. } => self.build_expr(expr, flow),
            Stmt::Block(b) => {
                let saved = self.enter(node_key(b));
                let out = self.build_stmts(&b.stmts, flow);
                self.scope = saved;
                out
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                let ac = self.build_expr(cond, flow);
                let then_in = self.cond(cond, true, ac);
                let then_out = self.build_stmt(then, then_in);
                let else_in = self.cond(cond, false, ac);
                let else_out = match els {
                    Some(e) => self.build_stmt(e, else_in),
                    None => else_in,
                };
                self.branch(vec![then_out, else_out])
            }
            Stmt::While { cond, body, .. } => {
                let loop_label = self.branch(vec![flow]);
                let ac = self.build_expr(cond, loop_label);
                let body_in = self.cond(cond, true, ac);
                let post = self.branch(vec![]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, body_in);
                self.cont.pop();
                self.brk.pop();
                self.add_ante(loop_label, body_out);
                let exit = self.cond(cond, false, loop_label);
                self.add_ante(post, exit);
                post
            }
            Stmt::DoWhile { body, cond, .. } => {
                let loop_label = self.branch(vec![flow]);
                let post = self.branch(vec![]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, loop_label);
                self.cont.pop();
                self.brk.pop();
                let ac = self.build_expr(cond, body_out);
                self.add_ante(loop_label, ac);
                self.add_ante(post, ac);
                post
            }
            Stmt::For {
                init,
                cond,
                incr,
                body,
                ..
            } => {
                let saved = self.enter(node_key(stmt));
                let mut flow = flow;
                if let Some(init) = init {
                    flow = match &**init {
                        ForInit::Var(v) => self.build_var(v, flow),
                        ForInit::Expr(e) => self.build_expr(e, flow),
                    };
                }
                let loop_label = self.branch(vec![flow]);
                let ac = match cond {
                    Some(c) => self.build_expr(c, loop_label),
                    None => loop_label,
                };
                let body_in = match cond {
                    Some(c) => self.cond(c, true, ac),
                    None => ac,
                };
                let post = self.branch(vec![]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, body_in);
                self.cont.pop();
                self.brk.pop();
                let ai = match incr {
                    Some(e) => self.build_expr(e, body_out),
                    None => body_out,
                };
                self.add_ante(loop_label, ai);
                let exit = match cond {
                    Some(c) => self.cond(c, false, loop_label),
                    None => loop_label,
                };
                self.add_ante(post, exit);
                self.scope = saved;
                post
            }
            Stmt::ForIn {
                left, expr, body, ..
            }
            | Stmt::ForOf {
                left, expr, body, ..
            } => {
                let saved = self.enter(node_key(stmt));
                let ae = self.build_expr(expr, flow);
                let loop_label = self.branch(vec![ae]);
                // The loop variable is (re)bound each iteration, INSIDE the
                // loop: loop_label → Init/Assign → body.
                let body_in = match &**left {
                    ForInit::Var(v) => {
                        let mut f = loop_label;
                        for d in &v.decls {
                            f = self.new_node(FlowNode::Init {
                                decl: d,
                                scope: self.scope,
                                ante: f,
                            });
                        }
                        f
                    }
                    // `for (x of e)` assigning to an existing reference:
                    // threaded linearly for now (Stage-0 TODO: an Assign node
                    // needs an assigned-type source, which here is the
                    // enclosing statement, not an expression).
                    ForInit::Expr(e) => self.build_expr(e, loop_label),
                };
                let post = self.branch(vec![loop_label]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, body_in);
                self.cont.pop();
                self.brk.pop();
                self.add_ante(loop_label, body_out);
                self.scope = saved;
                post
            }
            Stmt::Return { expr, .. } => {
                if let Some(e) = expr {
                    self.build_expr(e, flow);
                }
                self.start()
            }
            Stmt::Throw { expr, .. } => {
                self.build_expr(expr, flow);
                self.start()
            }
            Stmt::Break { label: None, .. } => {
                if let Some(&t) = self.brk.last() {
                    self.add_ante(t, flow);
                }
                self.start()
            }
            Stmt::Continue { label: None, .. } => {
                if let Some(&t) = self.cont.last() {
                    self.add_ante(t, flow);
                }
                self.start()
            }
            Stmt::Labeled { stmt, .. } => self.build_stmt(stmt, flow),
            Stmt::With { obj, body, .. } => {
                let a = self.build_expr(obj, flow);
                self.build_stmt(body, a)
            }
            // Switch / Try (and labeled break/continue targets): threaded
            // linearly for now (Stage-0 TODO). Still walk sub-expressions so
            // references inside get a flow node (their declared type — safe).
            Stmt::Switch { expr, cases, .. } => {
                let saved = self.enter(node_key(stmt));
                let a = self.build_expr(expr, flow);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.build_expr(t, a);
                    }
                    self.build_stmts(&c.stmts, a);
                }
                self.scope = saved;
                a
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                let saved = self.enter(node_key(block));
                let mut f = self.build_stmts(&block.stmts, flow);
                self.scope = saved;
                if let Some(c) = catch {
                    let saved = self.enter(node_key(c));
                    f = self.build_stmts(&c.block.stmts, flow);
                    self.scope = saved;
                }
                if let Some(fin) = finally {
                    let saved = self.enter(node_key(fin));
                    f = self.build_stmts(&fin.stmts, f);
                    self.scope = saved;
                }
                f
            }
            // Declarations start their own function sub-graphs but do not advance
            // the enclosing flow.
            Stmt::Func(f) => {
                self.build_function(f);
                flow
            }
            Stmt::Class(c) => {
                self.build_class(c);
                flow
            }
            Stmt::ExportDefault { expr, .. } | Stmt::ExportAssign { expr, .. } => {
                self.build_expr(expr, flow)
            }
            // Empty / Missing / Interface / TypeAlias / Enum / Namespace / Import*
            // / ExportNamed / labeled break-continue: no intraprocedural flow.
            _ => flow,
        }
    }

    fn build_expr(&mut self, expr: &'a Expr, flow: FlowNodeId) -> FlowNodeId {
        match expr {
            Expr::Ident(id) => {
                self.map.insert(node_key(id), flow);
                flow
            }
            Expr::This { .. } => {
                self.map.insert(node_key(expr), flow);
                flow
            }
            Expr::PropAccess { obj, .. } => {
                let a = self.build_expr(obj, flow);
                self.map.insert(node_key(expr), a);
                a
            }
            Expr::ElemAccess { obj, index, .. } => {
                let a = self.build_expr(obj, flow);
                self.build_expr(index, a)
            }
            Expr::Paren { inner, .. } => self.build_expr(inner, flow),
            Expr::NonNull { expr: e, .. }
            | Expr::Assertion { expr: e, .. }
            | Expr::Await { expr: e, .. }
            | Expr::Spread { expr: e, .. } => self.build_expr(e, flow),
            Expr::Unary { operand, .. } => self.build_expr(operand, flow),
            Expr::Update { operand, .. } => {
                let a = self.build_expr(operand, flow);
                self.new_node(FlowNode::Assign {
                    target: operand,
                    expr,
                    scope: self.scope,
                    ante: a,
                })
            }
            Expr::Binary {
                op, left, right, ..
            } => {
                if op.is_assignment() {
                    let ar = self.build_expr(right, flow);
                    self.build_expr(left, ar);
                    self.new_node(FlowNode::Assign {
                        target: left,
                        expr,
                        scope: self.scope,
                        ante: ar,
                    })
                } else if matches!(op, BinOp::AmpAmp) {
                    let al = self.build_expr(left, flow);
                    let r_in = self.cond(left, true, al);
                    self.build_expr(right, r_in);
                    al
                } else if matches!(op, BinOp::BarBar | BinOp::QuestionQuestion) {
                    let al = self.build_expr(left, flow);
                    let r_in = self.cond(left, false, al);
                    self.build_expr(right, r_in);
                    al
                } else {
                    let al = self.build_expr(left, flow);
                    self.build_expr(right, al)
                }
            }
            Expr::Cond {
                cond,
                when_true,
                when_false,
                ..
            } => {
                let ac = self.build_expr(cond, flow);
                let t_in = self.cond(cond, true, ac);
                self.build_expr(when_true, t_in);
                let f_in = self.cond(cond, false, ac);
                self.build_expr(when_false, f_in);
                ac
            }
            Expr::Call { callee, args, .. } => {
                let mut f = self.build_expr(callee, flow);
                for a in args {
                    f = self.build_expr(a, f);
                }
                self.new_node(FlowNode::Call {
                    call: expr,
                    scope: self.scope,
                    ante: f,
                })
            }
            Expr::New {
                callee,
                args: Some(args),
                ..
            } => {
                let mut f = self.build_expr(callee, flow);
                for a in args {
                    f = self.build_expr(a, f);
                }
                f
            }
            Expr::New { callee, .. } => self.build_expr(callee, flow),
            // nested functions/classes: their own sub-graphs
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                self.build_function(f);
                flow
            }
            Expr::ClassExpr(c) => {
                self.build_class(c);
                flow
            }
            // literals / templates / arrays / objects / jsx / yield / import:
            // not walked for references in Stage 0 (safe — unconsumed).
            _ => flow,
        }
    }

    fn build_function(&mut self, f: &'a FunctionLike) {
        let saved = self.enter(node_key(f));
        let start = self.start();
        match &f.body {
            Some(FuncBody::Block(b)) => {
                self.build_stmts(&b.stmts, start);
            }
            Some(FuncBody::Expr(e)) => {
                self.build_expr(e, start);
            }
            None => {}
        }
        self.scope = saved;
    }

    fn build_class(&mut self, c: &'a ClassDecl) {
        let saved = self.enter(node_key(c));
        for m in &c.members {
            match m {
                ClassMember::Method(f) => self.build_function(f),
                ClassMember::Property(p) => {
                    if let Some(init) = &p.init {
                        let start = self.start();
                        self.build_expr(init, start);
                    }
                }
                _ => {}
            }
        }
        self.scope = saved;
    }
}
