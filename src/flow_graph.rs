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
use std::collections::{HashMap, HashSet};

pub fn build<'a>(bind: &mut BindResult<'a>, files: &'a [(String, SourceText, SourceFileAst)]) {
    let mut b = FlowBuilder {
        nodes: Vec::new(),
        map: HashMap::new(),
        end_flow: HashMap::new(),
        stmt_flow: HashMap::new(),
        fallthrough_flow: HashMap::new(),
        fn_returns: HashMap::new(),
        clause_fallthrough: HashMap::new(),
        stmt_position_calls: HashSet::new(),
        fn_keys: Vec::new(),
        brk: Vec::new(),
        cont: Vec::new(),
        ret: None,
        exc: Vec::new(),
        labels: Vec::new(),
        pending_labels: Vec::new(),
        scopes: &bind.node_scope,
        scope: bind.global_scope,
    };
    let start = b.start(None, None);
    for (i, (_name, _text, ast)) in files.iter().enumerate() {
        b.scope = bind
            .module_scope
            .get(&i)
            .copied()
            .unwrap_or(bind.global_scope);
        b.build_stmts(&ast.stmts, start);
    }
    let FlowBuilder {
        nodes,
        map,
        end_flow,
        stmt_flow,
        fallthrough_flow,
        fn_returns,
        clause_fallthrough,
        stmt_position_calls,
        ..
    } = b;
    bind.flow_nodes = nodes;
    bind.flow_node = map;
    bind.fn_end_flow = end_flow;
    bind.stmt_flow = stmt_flow;
    bind.fn_fallthrough_flow = fallthrough_flow;
    bind.fn_returns = fn_returns;
    bind.clause_fallthrough = clause_fallthrough;
    bind.stmt_position_calls = stmt_position_calls;
}

struct FlowBuilder<'a, 'b> {
    nodes: Vec<FlowNode<'a>>,
    map: HashMap<usize, FlowNodeId>,
    /// function-like node ptr -> END flow (returns + fall-through joined)
    end_flow: HashMap<usize, FlowNodeId>,
    /// statement node ptr -> entry flow (tsc statement flowNode)
    stmt_flow: HashMap<usize, FlowNodeId>,
    /// function-like node ptr -> body fall-through out (pre-end-join)
    fallthrough_flow: HashMap<usize, FlowNodeId>,
    /// function-like node ptr -> each return statement's input flow
    fn_returns: HashMap<usize, Vec<FlowNodeId>>,
    /// switch-case node ptr -> clause body out-flow
    clause_fallthrough: HashMap<usize, FlowNodeId>,
    /// call expressions in statement position (never-call reachability)
    stmt_position_calls: HashSet<usize>,
    /// node keys of enclosing function-likes, inner last (for fn_returns)
    fn_keys: Vec<usize>,
    /// break-target labels (a `Branch` per enclosing loop/switch), inner last.
    brk: Vec<FlowNodeId>,
    /// continue-target labels, inner last.
    cont: Vec<FlowNodeId>,
    /// IIFE return-target (tsc `currentReturnTarget`): `return`s add their
    /// flow as antecedents, and the label becomes the post-call flow.
    ret: Option<FlowNodeId>,
    /// exception labels of enclosing `try` blocks, inner last (tsc
    /// `currentExceptionTarget`): every Assign/Init/Call inside the block
    /// edges into the label, so `catch` joins over all of them.
    exc: Vec<FlowNodeId>,
    /// active statement labels, inner last: (name, break target, continue
    /// target). The continue slot is filled by the loop arm the label
    /// directly wraps (tsc activeLabelList).
    labels: Vec<(String, FlowNodeId, Option<FlowNodeId>)>,
    /// indices into `labels` whose continue slot the NEXT directly-built
    /// loop should claim (`a: b: for (...)` — both labels continue to the
    /// for). Taken (and thereby cleared) at the top of every build_stmt.
    pending_labels: Vec<usize>,
    /// the binder's container-node → scope map
    scopes: &'b HashMap<usize, ScopeId>,
    /// scope in effect at the point being built
    scope: ScopeId,
}

/// Static truth of a condition built from literal `true`/`false` through
/// `&&`/`||`/`!`. tsc reaches the same conclusions structurally: its binder
/// recurses bindCondition through logical operators and each literal leaf's
/// contradicting edge is unreachable (`true || false ? a : b` has an
/// unreachable false arm). Parens are deliberately NOT stripped — tsc's
/// createFlowCondition checks the raw keyword, so `(true)` stays dynamic.
fn const_bool_cond(e: &Expr) -> Option<bool> {
    match e {
        Expr::BoolLit { value, .. } => Some(*value),
        Expr::Binary {
            op: BinOp::AmpAmp,
            left,
            right,
            ..
        } => match const_bool_cond(left)? {
            false => Some(false),
            true => const_bool_cond(right),
        },
        Expr::Binary {
            op: BinOp::BarBar,
            left,
            right,
            ..
        } => match const_bool_cond(left)? {
            true => Some(true),
            false => const_bool_cond(right),
        },
        Expr::Unary {
            op: UnaryOp::Bang,
            operand,
            ..
        } => const_bool_cond(operand).map(|b| !b),
        _ => None,
    }
}

impl<'a> FlowBuilder<'a, '_> {
    /// Mark call expressions in STATEMENT position (tsc
    /// maybeBindExpressionFlowIfCall: expression statements and comma
    /// operands; parens deliberately NOT stripped): only these participate
    /// in never-returning-call reachability.
    fn mark_stmt_position_calls(&mut self, e: &'a Expr) {
        match e {
            Expr::Call { .. } => {
                self.stmt_position_calls.insert(node_key(e));
            }
            Expr::Binary {
                op: BinOp::Comma,
                left,
                right,
                ..
            } => {
                self.mark_stmt_position_calls(left);
                self.mark_stmt_position_calls(right);
            }
            _ => {}
        }
    }

    fn new_node(&mut self, n: FlowNode<'a>) -> FlowNodeId {
        let id = FlowNodeId(self.nodes.len() as u32);
        self.nodes.push(n);
        id
    }

    fn branch(&mut self, antes: Vec<FlowNodeId>) -> FlowNodeId {
        self.new_node(FlowNode::Branch(antes))
    }

    /// A fresh `Start` — a function-body / module / namespace entry. `outer`
    /// (function-expression-like containers only) lets the resolver resume a
    /// const reference's walk in the enclosing flow; `cspan` is the
    /// container's span for the definite-assignment outer-variable rule.
    fn start(&mut self, outer: Option<(FlowNodeId, Span)>, cspan: Option<Span>) -> FlowNodeId {
        self.new_node(FlowNode::Start { outer, cspan })
    }

    /// Append an effect node (Assign/Init/Call) and, inside a `try` block,
    /// edge it into the enclosing exception label (tsc adds every mutation /
    /// call flow node as an antecedent of `currentExceptionTarget`).
    fn effect(&mut self, n: FlowNode<'a>) -> FlowNodeId {
        let id = self.new_node(n);
        if let Some(&e) = self.exc.last() {
            self.add_ante(e, id);
        }
        id
    }

    /// The dead flow after a return/throw/break/continue (joins skip it).
    fn unreachable(&mut self) -> FlowNodeId {
        self.new_node(FlowNode::Unreachable)
    }

    /// Append an antecedent to an existing `Branch`/loop label.
    fn add_ante(&mut self, label: FlowNodeId, ante: FlowNodeId) {
        if let FlowNode::Branch(antes) = &mut self.nodes[label.0 as usize] {
            antes.push(ante);
        }
    }

    fn cond(&mut self, cond: &'a Expr, sense: bool, ante: FlowNodeId) -> FlowNodeId {
        // tsc createFlowCondition: a literal true/false condition makes the
        // contradicting edge unreachable — everywhere conditions make flow
        // (if / loops / ternaries / && / ||)
        if const_bool_cond(cond) == Some(!sense) {
            return self.unreachable();
        }
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
                flow = self.effect(FlowNode::Init {
                    decl: d,
                    scope: self.scope,
                    ante: after,
                });
            }
        }
        flow
    }

    fn build_stmt(&mut self, stmt: &'a Stmt, flow: FlowNodeId) -> FlowNodeId {
        // the flow in effect ON ENTRY (tsc assigns node.flowNode before
        // binding the statement) — reachability queries key here
        self.stmt_flow.insert(node_key(stmt), flow);
        // labels apply only to the DIRECTLY labeled statement: take them
        // here so any non-loop arm implicitly drops the continue claims
        let pending = std::mem::take(&mut self.pending_labels);
        match stmt {
            Stmt::Var(v) => self.build_var(v, flow),
            Stmt::Expr { expr, .. } => {
                self.mark_stmt_position_calls(expr);
                self.build_expr(expr, flow)
            }
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
                for &i in &pending {
                    self.labels[i].2 = Some(loop_label);
                }
                let ac = self.build_expr(cond, loop_label);
                let body_in = self.cond(cond, true, ac);
                let post = self.branch(vec![]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, body_in);
                self.cont.pop();
                self.brk.pop();
                self.add_ante(loop_label, body_out);
                // the exit edge applies `Cond(cond, false)` (tsc's
                // condition-false antecedent on the post-loop label);
                // `while (true)` gets an unreachable exit from cond() —
                // only breaks reach the post label
                let exit = self.cond(cond, false, ac);
                self.add_ante(post, exit);
                post
            }
            Stmt::DoWhile { body, cond, .. } => {
                let loop_label = self.branch(vec![flow]);
                for &i in &pending {
                    self.labels[i].2 = Some(loop_label);
                }
                let post = self.branch(vec![]);
                self.brk.push(post);
                self.cont.push(loop_label);
                let body_out = self.build_stmt(body, loop_label);
                self.cont.pop();
                self.brk.pop();
                let ac = self.build_expr(cond, body_out);
                // back edge re-enters under cond-true, exit leaves under
                // cond-false (tsc's do-while antecedents); cond() makes the
                // contradicted edge of a literal condition unreachable
                let back = self.cond(cond, true, ac);
                self.add_ante(loop_label, back);
                let exit = self.cond(cond, false, ac);
                self.add_ante(post, exit);
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
                for &i in &pending {
                    self.labels[i].2 = Some(loop_label);
                }
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
                // the exit edge applies `Cond(cond, false)` (tsc's
                // condition-false antecedent on the post-loop label); a
                // missing condition has NO reachable exit — only breaks
                // reach the post label (cond() handles literal true)
                let exit = match cond {
                    Some(c) => self.cond(c, false, ac),
                    None => self.unreachable(),
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
                for &i in &pending {
                    self.labels[i].2 = Some(loop_label);
                }
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
                    // `for (x of e)` assigning to an existing reference: an
                    // Assign node per iteration. The assigning expression is
                    // the target itself (not a `Binary`), which the resolver
                    // resolves to the declared type — matching the fact
                    // stack, which only invalidates here.
                    ForInit::Expr(e) => {
                        let a = self.build_expr(e, loop_label);
                        self.effect(FlowNode::Assign {
                            target: e,
                            expr: e,
                            scope: self.scope,
                            ante: a,
                        })
                    }
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
                if let Some(&fk) = self.fn_keys.last() {
                    // the INPUT flow: "has an explicit return" asks whether
                    // any return statement is structurally reachable
                    self.fn_returns.entry(fk).or_default().push(flow);
                }
                let mut f = flow;
                if let Some(e) = expr {
                    f = self.build_expr(e, f);
                }
                // inside an IIFE the return flow joins the post-call label
                if let Some(t) = self.ret {
                    self.add_ante(t, f);
                }
                self.unreachable()
            }
            Stmt::Throw { expr, .. } => {
                self.build_expr(expr, flow);
                self.unreachable()
            }
            Stmt::Break { label: None, .. } => {
                if let Some(&t) = self.brk.last() {
                    self.add_ante(t, flow);
                }
                self.unreachable()
            }
            Stmt::Continue { label: None, .. } => {
                if let Some(&t) = self.cont.last() {
                    self.add_ante(t, flow);
                }
                self.unreachable()
            }
            Stmt::Break { label: Some(l), .. } => {
                if let Some(&(_, post, _)) =
                    self.labels.iter().rev().find(|(n, ..)| *n == l.name)
                {
                    self.add_ante(post, flow);
                }
                self.unreachable()
            }
            Stmt::Continue { label: Some(l), .. } => {
                // continue targets the labeled LOOP's label; a label on a
                // non-loop has no continue slot (the checker reports it)
                if let Some(&(_, _, Some(ll))) =
                    self.labels.iter().rev().find(|(n, ..)| *n == l.name)
                {
                    self.add_ante(ll, flow);
                }
                self.unreachable()
            }
            Stmt::Labeled { label, stmt: inner, .. } => {
                // register the label: breaks join a fresh post label; the
                // directly-wrapped loop claims the continue slot via
                // `pending_labels` (nested `a: b: for` — both continue to
                // the for)
                let post = self.branch(vec![]);
                self.labels.push((label.name.clone(), post, None));
                let idx = self.labels.len() - 1;
                let mut p = pending;
                p.push(idx);
                self.pending_labels = p;
                let out = self.build_stmt(inner, flow);
                self.pending_labels.clear();
                self.labels.truncate(idx);
                self.add_ante(post, out);
                post
            }
            Stmt::With { obj, body, .. } => {
                let a = self.build_expr(obj, flow);
                self.build_stmt(body, a)
            }
            Stmt::Switch { expr, cases, .. } => {
                let saved = self.enter(node_key(stmt));
                let a = self.build_expr(expr, flow);
                let post = self.branch(vec![]);
                self.brk.push(post);
                let mut fallthrough: Option<FlowNodeId> = None;
                let mut has_default = false;
                for (i, c) in cases.iter().enumerate() {
                    if let Some(t) = &c.test {
                        self.build_expr(t, a);
                    } else {
                        has_default = true;
                    }
                    let clause_node = self.new_node(FlowNode::Switch {
                        disc: expr,
                        cases,
                        clause: i as u32,
                        stmt_key: node_key(stmt),
                        scope: self.scope,
                        ante: a,
                    });
                    // a clause is entered either by matching or by falling
                    // through from the previous clause's body
                    let clause_in = match fallthrough {
                        Some(f) => self.branch(vec![clause_node, f]),
                        None => clause_node,
                    };
                    let clause_out = self.build_stmts(&c.stmts, clause_in);
                    self.clause_fallthrough.insert(node_key(c), clause_out);
                    fallthrough = Some(clause_out);
                }
                if let Some(f) = fallthrough {
                    self.add_ante(post, f);
                }
                if !has_default {
                    // no clause matched: flow falls past the switch narrowed
                    // by the negation of every label
                    let none = self.new_node(FlowNode::Switch {
                        disc: expr,
                        cases,
                        clause: cases.len() as u32,
                        stmt_key: node_key(stmt),
                        scope: self.scope,
                        ante: a,
                    });
                    self.add_ante(post, none);
                }
                self.brk.pop();
                self.scope = saved;
                post
            }
            // tsc's bindTryStatement minus the ReduceLabel re-threading: the
            // catch block enters at "any exception point in the try" (its
            // entry plus after every Assign/Init/Call inside, collected via
            // the `exc` label); `finally` sees the normal exits — widened by
            // the raw exception paths when there is no catch — and the
            // statement exits at the last join. Wider than tsc only in the
            // finally re-threading, which is the safe direction.
            Stmt::Try {
                block,
                catch,
                finally,
                ..
            } => {
                let exception = self.branch(vec![flow]);
                self.exc.push(exception);
                let saved = self.enter(node_key(block));
                let try_out = self.build_stmts(&block.stmts, flow);
                self.scope = saved;
                self.exc.pop();
                let mut normal = vec![try_out];
                if let Some(c) = catch {
                    let saved = self.enter(node_key(c));
                    let catch_out = self.build_stmts(&c.block.stmts, exception);
                    self.scope = saved;
                    normal.push(catch_out);
                }
                let join = self.branch(normal);
                match finally {
                    Some(fin) => {
                        let fin_in = if catch.is_some() {
                            join
                        } else {
                            self.branch(vec![join, exception])
                        };
                        let saved = self.enter(node_key(fin));
                        let out = self.build_stmts(&fin.stmts, fin_in);
                        self.scope = saved;
                        out
                    }
                    None => join,
                }
            }
            // A namespace body is its own control-flow container (tsc
            // ModuleBlock): statements start from a fresh `Start`.
            Stmt::Namespace(n) => {
                let saved = self.enter(node_key(&**n));
                let start = self.start(None, Some(n.span));
                self.build_stmts(&n.body, start);
                self.scope = saved;
                flow
            }
            // Declarations start their own function sub-graphs but do not advance
            // the enclosing flow.
            Stmt::Func(f) => {
                self.build_function(f, None);
                flow
            }
            Stmt::Class(c) => {
                // the heritage expression evaluates in the enclosing flow
                if let Some(h) = &c.extends {
                    self.build_expr(&h.expr, flow);
                }
                self.build_class(c, None);
                flow
            }
            Stmt::ExportDefault { expr, .. } | Stmt::ExportAssign { expr, .. } => {
                self.build_expr(expr, flow)
            }
            // Empty / Missing / Interface / TypeAlias / Enum / Import*
            // / ExportNamed: no intraprocedural flow.
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
                self.effect(FlowNode::Assign {
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
                    self.effect(FlowNode::Assign {
                        target: left,
                        expr,
                        scope: self.scope,
                        ante: ar,
                    })
                } else if matches!(
                    op,
                    BinOp::AmpAmp | BinOp::BarBar | BinOp::QuestionQuestion
                ) {
                    // the expression's out-flow joins the skip edge (RHS not
                    // evaluated) with the RHS out-flow, so downstream reads
                    // see narrowings/assignments from both paths (tsc
                    // bindBinaryExpressionFlow). `??` evaluates its RHS on
                    // NULLISH left — a dedicated Nullish edge, since a
                    // truthiness Cond would drop "" / 0 from the skip edge.
                    let al = self.build_expr(left, flow);
                    let r_in = match op {
                        BinOp::AmpAmp => self.cond(left, true, al),
                        BinOp::BarBar => self.cond(left, false, al),
                        _ => self.new_node(FlowNode::Nullish {
                            expr: left,
                            sense: false,
                            scope: self.scope,
                            ante: al,
                        }),
                    };
                    let r_out = self.build_expr(right, r_in);
                    let skip = match op {
                        BinOp::AmpAmp => self.cond(left, false, al),
                        BinOp::BarBar => self.cond(left, true, al),
                        _ => self.new_node(FlowNode::Nullish {
                            expr: left,
                            sense: true,
                            scope: self.scope,
                            ante: al,
                        }),
                    };
                    self.branch(vec![skip, r_out])
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
                let t_out = self.build_expr(when_true, t_in);
                let f_in = self.cond(cond, false, ac);
                let f_out = self.build_expr(when_false, f_in);
                self.branch(vec![t_out, f_out])
            }
            Expr::Call { callee, args, .. } => {
                // IIFE (tsc bindCallExpressionFlow): a non-async, non-
                // generator function-expression/arrow callee is part of the
                // enclosing control flow — arguments evaluate first, then the
                // body threads from the call site, and `return`s join a label
                // that becomes the post-call flow.
                if let Some(f) = iife_callee(callee) {
                    let mut fl = flow;
                    for a in args {
                        fl = self.build_expr(a, fl);
                    }
                    let post = self.build_iife(f, fl);
                    return self.effect(FlowNode::Call {
                        call: expr,
                        scope: self.scope,
                        ante: post,
                    });
                }
                let mut f = self.build_expr(callee, flow);
                for a in args {
                    f = self.build_expr(a, f);
                }
                self.effect(FlowNode::Call {
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
            // nested functions/classes: their own sub-graphs. The enclosing
            // flow is recorded on the `Start` so const references can walk out.
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                self.build_function(f, Some(flow));
                flow
            }
            Expr::ClassExpr(c) => {
                if let Some(h) = &c.extends {
                    self.build_expr(&h.expr, flow);
                }
                self.build_class(c, Some(flow));
                flow
            }
            Expr::Array { elements, .. } => {
                let mut f = flow;
                for e in elements {
                    f = self.build_expr(e, f);
                }
                f
            }
            Expr::Object { props, .. } => {
                let mut f = flow;
                for p in props {
                    match p {
                        ObjectProp::Property { name, value, .. } => {
                            if let PropName::Computed { expr: k, .. } = name {
                                f = self.build_expr(k, f);
                            }
                            f = self.build_expr(value, f);
                        }
                        // `{ x }` reads `x` — same seam as a bare identifier
                        ObjectProp::Shorthand { name, .. } => {
                            self.map.insert(node_key(name), f);
                        }
                        ObjectProp::Method(m) => {
                            self.build_function(m, Some(f));
                        }
                        ObjectProp::Spread { expr: e, .. } => {
                            f = self.build_expr(e, f);
                        }
                    }
                }
                f
            }
            Expr::Template { parts, .. } => {
                let mut f = flow;
                for p in parts {
                    if let TemplatePart::Expr(e) = p {
                        f = self.build_expr(e, f);
                    }
                }
                f
            }
            Expr::Yield { expr: Some(e), .. } => self.build_expr(e, flow),
            Expr::ImportCall { args, .. } => {
                let mut f = flow;
                for a in args {
                    f = self.build_expr(a, f);
                }
                f
            }
            Expr::JsxElement(j) => self.build_jsx(j, flow),
            // literals / import.meta / missing: no references inside.
            _ => flow,
        }
    }

    /// Build a function body from a fresh `Start`. `outer` — the enclosing
    /// flow at the function's position — is set for function expressions,
    /// arrows and object-literal/class-expression methods (tsc's
    /// `FlowStart.node` containers); `None` for declarations, constructors
    /// and property initializers. Loop/return/exception targets do not cross
    /// function boundaries (tsc bindContainer resets them).
    fn build_function(&mut self, f: &'a FunctionLike, outer: Option<FlowNodeId>) {
        let saved = self.enter(node_key(f));
        let saved_brk = std::mem::take(&mut self.brk);
        let saved_cont = std::mem::take(&mut self.cont);
        let saved_exc = std::mem::take(&mut self.exc);
        let saved_labels = std::mem::take(&mut self.labels);
        let saved_pending = std::mem::take(&mut self.pending_labels);
        // every `return` joins the END label with the body's fall-through
        // (tsc's returnFlowNode) — the constructor-end query point for
        // strictPropertyInitialization. The fall-through alone is recorded
        // separately (tsc endFlowNode): its reachability is "has an
        // implicit return".
        let end = self.branch(vec![]);
        let saved_ret = self.ret.replace(end);
        self.fn_keys.push(node_key(f));
        let start = self.start(outer.map(|o| (o, f.span)), Some(f.span));
        let fl = self.build_params(f, start);
        match &f.body {
            Some(FuncBody::Block(b)) => {
                let out = self.build_stmts(&b.stmts, fl);
                self.fallthrough_flow.insert(node_key(f), out);
                self.add_ante(end, out);
            }
            Some(FuncBody::Expr(e)) => {
                let out = self.build_expr(e, fl);
                self.fallthrough_flow.insert(node_key(f), out);
                self.add_ante(end, out);
            }
            None => {}
        }
        self.fn_keys.pop();
        self.end_flow.insert(node_key(f), end);
        self.ret = saved_ret;
        self.pending_labels = saved_pending;
        self.labels = saved_labels;
        self.exc = saved_exc;
        self.cont = saved_cont;
        self.brk = saved_brk;
        self.scope = saved;
    }

    /// An immediately-invoked function expression: the body threads from the
    /// call-site flow (no fresh `Start`), and every `return` joins the label
    /// returned here, which becomes the post-call flow (tsc bindContainer's
    /// IIFE special case + `currentReturnTarget`).
    fn build_iife(&mut self, f: &'a FunctionLike, flow: FlowNodeId) -> FlowNodeId {
        let saved = self.enter(node_key(f));
        let saved_brk = std::mem::take(&mut self.brk);
        let saved_cont = std::mem::take(&mut self.cont);
        let saved_exc = std::mem::take(&mut self.exc);
        let saved_labels = std::mem::take(&mut self.labels);
        let saved_pending = std::mem::take(&mut self.pending_labels);
        let ret = self.branch(vec![]);
        let saved_ret = self.ret.replace(ret);
        self.fn_keys.push(node_key(f));
        let fl = self.build_params(f, flow);
        let out = match &f.body {
            Some(FuncBody::Block(b)) => self.build_stmts(&b.stmts, fl),
            Some(FuncBody::Expr(e)) => self.build_expr(e, fl),
            None => fl,
        };
        self.fn_keys.pop();
        self.fallthrough_flow.insert(node_key(f), out);
        self.add_ante(ret, out);
        self.ret = saved_ret;
        self.pending_labels = saved_pending;
        self.labels = saved_labels;
        self.exc = saved_exc;
        self.cont = saved_cont;
        self.brk = saved_brk;
        self.scope = saved;
        ret
    }

    /// Walk parameter default-value expressions so their references get flow
    /// nodes (they evaluate at function entry, in order).
    fn build_params(&mut self, f: &'a FunctionLike, mut flow: FlowNodeId) -> FlowNodeId {
        for p in &f.params {
            if let Some(d) = &p.initializer {
                flow = self.build_expr(d, flow);
            }
        }
        flow
    }

    /// `outer` is the enclosing flow at a class *expression*'s position —
    /// tsc treats only object-literal / class-expression methods (not class-
    /// declaration members) as const-walk-out containers.
    fn build_class(&mut self, c: &'a ClassDecl, outer: Option<FlowNodeId>) {
        let saved = self.enter(node_key(c));
        for m in &c.members {
            match m {
                ClassMember::Method(f) => self.build_function(f, outer),
                ClassMember::Constructor(f) => self.build_function(f, None),
                ClassMember::StaticBlock(b) => {
                    let start = self.start(None, Some(b.span));
                    let sv = self.enter(node_key(b));
                    self.build_stmts(&b.stmts, start);
                    self.scope = sv;
                }
                ClassMember::Property(p) => {
                    if let Some(init) = &p.init {
                        let start = self.start(None, Some(init.span()));
                        self.build_expr(init, start);
                    }
                }
                ClassMember::Index(_) => {}
            }
        }
        self.scope = saved;
    }

    fn build_jsx(&mut self, j: &'a JsxElement, mut flow: FlowNodeId) -> FlowNodeId {
        for a in &j.attrs {
            if let Some(v) = &a.value {
                flow = self.build_expr(v, flow);
            }
        }
        for c in &j.children {
            match c {
                JsxChild::Element(e) => flow = self.build_jsx(e, flow),
                JsxChild::Expr(e) => flow = self.build_expr(e, flow),
                JsxChild::Text => {}
            }
        }
        flow
    }
}

/// The function of an IIFE: the call's target, through parentheses, when it
/// is a non-async, non-generator function expression or arrow (tsc
/// `getImmediatelyInvokedFunctionExpression` + the binder's async/generator
/// exclusion).
fn iife_callee(callee: &Expr) -> Option<&FunctionLike> {
    let mut e = callee;
    loop {
        match e {
            Expr::Paren { inner, .. } => e = inner,
            Expr::Arrow(f) | Expr::FunctionExpr(f) => {
                let f: &FunctionLike = f;
                return (!has_modifier(&f.modifiers, ModifierKind::Async) && !f.is_generator)
                    .then_some(f);
            }
            _ => return None,
        }
    }
}
