//! Tier-2 control-flow graph builder (Stage 0).
//!
//! Runs after `bind()`, before the `BindResult` is frozen in an `Arc`,
//! populating `bind.flow_nodes` + `bind.flow_node`. A syntax-only second pass
//! mirroring the binder's evaluation-order walk (kept out of binder.rs to
//! isolate the new logic; the extra traversal is cheap). During Stage 0 the
//! graph is BUILT but NOT consumed by any diagnostic — `get_flow_type_of_reference`
//! reads it in Stage 1 — so program output is unchanged regardless of contents.
//!
//! Antecedents point backward toward the function/program `Start`. Reference
//! expressions (Ident / PropAccess / This) are keyed by `node_key` to the flow
//! node in effect at them. Coverage is filled incrementally; unhandled
//! constructs thread flow linearly, which is SAFE: a reference then resolves
//! against a wider antecedent (never a wrong narrowing).

use crate::ast::*;
use crate::binder::{BindResult, FlowNode, FlowNodeId};
use crate::text::SourceText;
use std::collections::HashMap;

pub fn build<'a>(bind: &mut BindResult<'a>, files: &'a [(String, SourceText, SourceFileAst)]) {
    let mut b = FlowBuilder {
        nodes: Vec::new(),
        map: HashMap::new(),
        brk: Vec::new(),
        cont: Vec::new(),
    };
    let start = b.new_node(FlowNode::Start);
    for (_name, _text, ast) in files {
        b.build_stmts(&ast.stmts, start);
    }
    bind.flow_nodes = b.nodes;
    bind.flow_node = b.map;
}

struct FlowBuilder<'a> {
    nodes: Vec<FlowNode<'a>>,
    map: HashMap<usize, FlowNodeId>,
    /// break-target labels (a `Branch` per enclosing loop/switch), inner last.
    brk: Vec<FlowNodeId>,
    /// continue-target labels, inner last.
    cont: Vec<FlowNodeId>,
}

impl<'a> FlowBuilder<'a> {
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
        self.new_node(FlowNode::Cond { cond, sense, ante })
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
                flow = if let Binding::Ident(_) = &d.name {
                    self.new_node(FlowNode::Assign {
                        node: node_key(d),
                        ante: after,
                    })
                } else {
                    after
                };
            }
        }
        flow
    }

    fn build_stmt(&mut self, stmt: &'a Stmt, flow: FlowNodeId) -> FlowNodeId {
        match stmt {
            Stmt::Var(v) => self.build_var(v, flow),
            Stmt::Expr { expr, .. } => self.build_expr(expr, flow),
            Stmt::Block(b) => self.build_stmts(&b.stmts, flow),
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
                post
            }
            Stmt::ForIn {
                left, expr, body, ..
            }
            | Stmt::ForOf {
                left, expr, body, ..
            } => {
                let ae = self.build_expr(expr, flow);
                let f2 = match &**left {
                    ForInit::Var(v) => self.build_var(v, ae),
                    ForInit::Expr(e) => self.build_expr(e, ae),
                };
                let loop_label = self.branch(vec![f2]);
                let post = self.branch(vec![loop_label]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, loop_label);
                self.cont.pop();
                self.brk.pop();
                self.add_ante(loop_label, body_out);
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
                let a = self.build_expr(expr, flow);
                for c in cases {
                    if let Some(t) = &c.test {
                        self.build_expr(t, a);
                    }
                    self.build_stmts(&c.stmts, a);
                }
                a
            }
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                let mut f = self.build_stmts(&block.stmts, flow);
                if let Some(c) = catch {
                    f = self.build_stmts(&c.block.stmts, flow);
                }
                if let Some(fin) = finally {
                    f = self.build_stmts(&fin.stmts, f);
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
            Expr::Ident(_) | Expr::This { .. } => {
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
                    node: node_key(expr),
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
                        node: node_key(expr),
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
                    node: node_key(expr),
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
    }

    fn build_class(&mut self, c: &'a ClassDecl) {
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
    }
}
