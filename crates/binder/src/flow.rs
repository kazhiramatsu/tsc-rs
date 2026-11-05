//! Flow-node arena + the constructor family (m2-binder-steps.md stage
//! 3.3 scaffolding; stage 3.5 adds the condition/mutation/call
//! constructors and the narrowing predicates that gate them).
//!
//! tsc FlowNode (core-interfaces §5): `antecedent` holds one node for
//! plain kinds and an array for labels. Here a node is a 16-byte record and
//! the arrays are spans of one pool per file (see [`FlowNode`] and
//! [`FlowArena::freeze`]).
//! NOTE (source fact, 43077-43149): createFlowNode does NOT mark its
//! antecedent Referenced — each caller (addAntecedent,
//! createFlowCondition/Mutation/Call/SwitchClause) does so explicitly.

use tsc_syntax::NodeId;
use tsc_types::FlowFlags;

tsc_types::id_type!(FlowId);

/// What a plain flow node carries (tsc FlowNode.node): an AST node for most
/// kinds, `{switchStatement, clauseStart, clauseEnd}` for SwitchClause.
/// Labels and reduce labels are made by their own constructors.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum FlowPayload {
    #[default]
    None,
    Node(NodeId),
    SwitchClause {
        switch_statement: NodeId,
        clause_start: u32,
        clause_end: u32,
    },
}

/// A flow node's payload as the checker reads it: [`FlowPayload`]'s kinds,
/// and a label's or a reduce label's antecedent list (tsc FlowLabel and
/// ReduceLabel's `{target, antecedents}`, createReduceLabel 43095).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FlowData<'a> {
    None,
    Node(NodeId),
    SwitchClause {
        switch_statement: NodeId,
        clause_start: u32,
        clause_end: u32,
    },
    ReduceLabel {
        target: FlowId,
        antecedents: &'a [FlowId],
    },
    Label {
        antecedents: &'a [FlowId],
    },
}

/// tsc FlowNode in 16 bytes. `antecedent` is the plain kinds' single
/// antecedent (also a switch clause's and a reduce label's); `a` and `c`
/// hold the rest by kind:
///
/// | kind | `a` | `c` |
/// |---|---|---|
/// | plain | the payload node (0 = none) | 0 |
/// | SwitchClause | the switch statement | the list holding `clause_start, clause_end` |
/// | BranchLabel / LoopLabel | the antecedent list | its length (while binding: the list's last cell) |
/// | ReduceLabel | the target flow | the antecedent list |
///
/// A list is a chain of cells (its first cell) while the file is bound and
/// a pool offset once the arena is frozen (see [`FlowArena::freeze`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlowNode {
    pub flags: FlowFlags,
    a: u32,
    antecedent: Option<FlowId>,
    c: u32,
}

#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<FlowNode>() == 16);

impl FlowNode {
    /// tsc FlowNode.antecedent of the plain kinds (none for a label, the
    /// start and the unreachable node).
    #[inline(always)]
    pub fn antecedent(&self) -> Option<FlowId> {
        self.antecedent
    }

    /// The AST node a plain flow node carries (tsc FlowNode.node of an
    /// assignment, condition, call or array mutation; a start node's
    /// function): none for the other kinds. The checker's flow walk reads
    /// it at every step, so it is one flags test and a word.
    #[inline(always)]
    pub fn payload_node(&self) -> Option<NodeId> {
        if self.flags.intersects(
            FlowFlags::BRANCH_LABEL
                | FlowFlags::LOOP_LABEL
                | FlowFlags::REDUCE_LABEL
                | FlowFlags::SWITCH_CLAUSE,
        ) || self.a == 0
        {
            None
        } else {
            Some(NodeId::new(self.a - 1))
        }
    }

    #[inline(always)]
    fn is_label(&self) -> bool {
        self.flags
            .intersects(FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL)
    }
}

/// A label's antecedents while its file is bound: a cell of a linked list in
/// per-thread scratch (tsgo's FlowList), so that no label owns a list
/// allocation. `next` is the next cell's index + 1 (0 ends the list).
#[derive(Clone, Copy, Debug)]
struct ListCell {
    item: FlowId,
    next: u32,
}

thread_local! {
    /// The cells of the binds on this thread, reused by the next file.
    static FLOW_CELLS: std::cell::Cell<Option<Vec<ListCell>>> = const { std::cell::Cell::new(None) };
}

/// One file's flow graph. While the file is bound, a label's list is a chain
/// of cells in per-thread scratch (`add_antecedent` appends,
/// `set_label_antecedents` replaces); [`Self::freeze`], at the end of the
/// bind, writes the lists once into one exact pool, in node order, and
/// gives the cells back. A frozen arena does not grow.
#[derive(Clone, Debug, Default)]
pub struct FlowArena {
    nodes: Vec<FlowNode>,
    cells: Vec<ListCell>,
    pool: Box<[FlowId]>,
    frozen: bool,
}

impl FlowArena {
    /// An empty arena for a bind, its cells in this thread's scratch.
    pub fn for_bind() -> Self {
        Self {
            cells: FLOW_CELLS.with(std::cell::Cell::take).unwrap_or_default(),
            ..Self::default()
        }
    }

    /// Flow node count and the bytes the arena and its antecedent lists
    /// own on the heap, for memory accounting.
    pub fn memory_usage(&self) -> (usize, usize, usize) {
        (
            self.nodes.len(),
            self.nodes.capacity() * std::mem::size_of::<FlowNode>(),
            self.pool.len() * std::mem::size_of::<FlowId>()
                + self.cells.capacity() * std::mem::size_of::<ListCell>(),
        )
    }

    /// tsc-port: createFlowNode @6.0.3
    /// tsc-hash: 50f4e5850330909e853e82825ef01ab9cb1bcd9bdd13b31469b77b8127094539
    /// tsc-span: _tsc.js:42404-42406
    pub fn create_flow_node(
        &mut self,
        flags: FlowFlags,
        payload: FlowPayload,
        antecedent: Option<FlowId>,
    ) -> FlowId {
        let (a, c) = match payload {
            FlowPayload::None => (0, 0),
            FlowPayload::Node(node) => (node.index() + 1, 0),
            FlowPayload::SwitchClause {
                switch_statement,
                clause_start,
                clause_end,
            } => (
                switch_statement.index() + 1,
                self.chain(&[FlowId::new(clause_start), FlowId::new(clause_end)])
                    .0,
            ),
        };
        self.push(FlowNode {
            flags,
            a,
            antecedent,
            c,
        })
    }

    fn push(&mut self, node: FlowNode) -> FlowId {
        debug_assert!(!self.frozen, "a frozen flow arena does not grow");
        let id = FlowId::new(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    /// A chain of cells holding `items`: (head, tail), each a cell index + 1
    /// (0, 0 for none).
    fn chain(&mut self, items: &[FlowId]) -> (u32, u32) {
        debug_assert!(!self.frozen, "a frozen flow arena does not grow");
        let mut head = 0;
        let mut tail = 0;
        for &item in items {
            let cell = self.cells.len() as u32 + 1;
            self.cells.push(ListCell { item, next: 0 });
            if tail == 0 {
                head = cell;
            } else {
                self.cells[tail as usize - 1].next = cell;
            }
            tail = cell;
        }
        (head, tail)
    }

    /// The items of the chain starting at `head`.
    fn walk(&self, head: u32) -> impl Iterator<Item = FlowId> + '_ {
        let mut cell = head;
        std::iter::from_fn(move || {
            if cell == 0 {
                return None;
            }
            let ListCell { item, next } = self.cells[cell as usize - 1];
            cell = next;
            Some(item)
        })
    }

    #[inline(always)]
    pub fn flow(&self, id: FlowId) -> &FlowNode {
        &self.nodes[id.index() as usize]
    }

    fn flow_mut(&mut self, id: FlowId) -> &mut FlowNode {
        &mut self.nodes[id.index() as usize]
    }

    /// A frozen list: `len` items from `at`, or (`None`) a reduce label's
    /// list, which carries its length first.
    #[inline(always)]
    fn list(&self, at: u32, len: Option<u32>) -> &[FlowId] {
        let start = at as usize;
        match len {
            Some(len) => &self.pool[start..start + len as usize],
            None => {
                let len = self.pool[start].index() as usize;
                &self.pool[start + 1..start + 1 + len]
            }
        }
    }

    /// The antecedents in tsc's order: a label's list, or the plain kinds'
    /// single antecedent (none for the start and unreachable nodes). While
    /// the file is bound a label's list is a chain: read it with
    /// [`Self::label_items`].
    #[inline(always)]
    pub fn antecedents(&self, id: FlowId) -> &[FlowId] {
        let node = self.flow(id);
        if node.is_label() {
            debug_assert!(
                self.frozen,
                "a label's list is read with label_items while binding"
            );
            self.list(node.a, Some(node.c))
        } else {
            node.antecedent.as_slice()
        }
    }

    /// A label's antecedents, as a list.
    pub fn label_items(&self, label: FlowId) -> Vec<FlowId> {
        debug_assert!(self.flow(label).is_label(), "label_items reads a label");
        if self.frozen {
            return self.antecedents(label).to_vec();
        }
        self.walk(self.flow(label).a).collect()
    }

    /// Whether a label has no antecedents.
    pub fn label_is_empty(&self, label: FlowId) -> bool {
        debug_assert!(self.flow(label).is_label(), "label_is_empty reads a label");
        if self.frozen {
            return self.antecedents(label).is_empty();
        }
        self.flow(label).a == 0
    }

    /// tsc FlowNode.node, with a label's or a reduce label's list (of a
    /// frozen arena).
    pub fn payload(&self, id: FlowId) -> FlowData<'_> {
        let node = self.flow(id);
        if node.is_label() {
            debug_assert!(
                self.frozen,
                "a label's list is read once the arena is frozen"
            );
            FlowData::Label {
                antecedents: self.list(node.a, Some(node.c)),
            }
        } else if node.flags.intersects(FlowFlags::REDUCE_LABEL) {
            debug_assert!(
                self.frozen,
                "a reduce label's list is read once the arena is frozen"
            );
            FlowData::ReduceLabel {
                target: FlowId::new(node.a),
                antecedents: self.list(node.c, None),
            }
        } else if node.flags.intersects(FlowFlags::SWITCH_CLAUSE) {
            debug_assert!(
                self.frozen,
                "a switch clause's pair is read once the arena is frozen"
            );
            let clauses = self.list(node.c, Some(2));
            FlowData::SwitchClause {
                switch_statement: NodeId::new(node.a - 1),
                clause_start: clauses[0].index(),
                clause_end: clauses[1].index(),
            }
        } else if node.a == 0 {
            FlowData::None
        } else {
            FlowData::Node(NodeId::new(node.a - 1))
        }
    }

    /// Write the lists once into one exact pool, in node order (labels:
    /// start and length; switch clauses: the clause pair; reduce labels: the
    /// length, then the list), shrink the nodes to their count and give the
    /// cells back to the thread. A bound file's arena does not grow again.
    pub fn freeze(&mut self) {
        if self.frozen {
            return;
        }
        let mut pool = Vec::new();
        let mut nodes = std::mem::take(&mut self.nodes);
        for node in &mut nodes {
            if node.is_label() {
                let start = pool.len() as u32;
                pool.extend(self.walk(node.a));
                node.a = start;
                node.c = pool.len() as u32 - start;
            } else if node.flags.intersects(FlowFlags::REDUCE_LABEL) {
                let start = pool.len();
                pool.push(FlowId::new(0));
                pool.extend(self.walk(node.c));
                pool[start] = FlowId::new((pool.len() - start - 1) as u32);
                node.c = start as u32;
            } else if node.flags.intersects(FlowFlags::SWITCH_CLAUSE) {
                let start = pool.len() as u32;
                pool.extend(self.walk(node.c));
                node.c = start;
            }
        }
        nodes.shrink_to_fit();
        self.nodes = nodes;
        self.pool = pool.into_boxed_slice();
        let mut cells = std::mem::take(&mut self.cells);
        cells.clear();
        FLOW_CELLS.with(|slot| slot.set(Some(cells)));
        self.frozen = true;
    }

    /// A frozen copy, for a bind result taken while the worker lives on.
    pub fn frozen(&self) -> Self {
        let mut copy = self.clone();
        copy.freeze();
        copy
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn create_label(&mut self, flags: FlowFlags) -> FlowId {
        self.push(FlowNode {
            flags,
            a: 0,
            antecedent: None,
            c: 0,
        })
    }

    /// tsc createBranchLabel (43077).
    pub fn create_branch_label(&mut self) -> FlowId {
        self.create_label(FlowFlags::BRANCH_LABEL)
    }

    /// tsc createLoopLabel (43086).
    pub fn create_loop_label(&mut self) -> FlowId {
        self.create_label(FlowFlags::LOOP_LABEL)
    }

    /// tsc createReduceLabel (43095).
    pub fn create_reduce_label(
        &mut self,
        target: FlowId,
        antecedents: Vec<FlowId>,
        antecedent: FlowId,
    ) -> FlowId {
        let (head, _) = self.chain(&antecedents);
        self.push(FlowNode {
            flags: FlowFlags::REDUCE_LABEL,
            a: target.index(),
            antecedent: Some(antecedent),
            c: head,
        })
    }

    /// Replace a label's antecedent list (the finally label of a try
    /// statement takes the normal, exception and return exits at once).
    pub fn set_label_antecedents(&mut self, label: FlowId, antecedents: Vec<FlowId>) {
        assert!(self.flow(label).is_label() && !self.frozen);
        let (head, tail) = self.chain(&antecedents);
        let node = self.flow_mut(label);
        node.a = head;
        node.c = tail;
    }

    /// tsc setFlowNodeReferenced (43098): the second reference marks
    /// Shared (the checker's shared-flow cache keys on it).
    pub fn set_flow_node_referenced(&mut self, flow: FlowId) {
        let node = self.flow_mut(flow);
        node.flags |= if node.flags.intersects(FlowFlags::REFERENCED) {
            FlowFlags::SHARED
        } else {
            FlowFlags::REFERENCED
        };
    }

    /// tsc addAntecedent (43101): unreachable antecedents and
    /// duplicates are dropped.
    pub fn add_antecedent(&mut self, label: FlowId, antecedent: FlowId) {
        if self
            .flow(antecedent)
            .flags
            .intersects(FlowFlags::UNREACHABLE)
        {
            return;
        }
        let node = *self.flow(label);
        assert!(
            node.is_label() && !self.frozen,
            "only branch and loop labels collect antecedents, while binding"
        );
        if self.walk(node.a).any(|item| item == antecedent) {
            return;
        }
        let cell = self.cells.len() as u32 + 1;
        self.cells.push(ListCell {
            item: antecedent,
            next: 0,
        });
        if node.c == 0 {
            self.flow_mut(label).a = cell;
        } else {
            self.cells[node.c as usize - 1].next = cell;
        }
        self.flow_mut(label).c = cell;
        self.set_flow_node_referenced(antecedent);
    }

    /// tsc finishFlowLabel (43141): no antecedents ⇒ unreachable, one
    /// ⇒ COLLAPSES to it, several ⇒ the label itself.
    pub fn finish_flow_label(&mut self, flow: FlowId, unreachable: FlowId) -> FlowId {
        let node = *self.flow(flow);
        debug_assert!(
            node.is_label() && !self.frozen,
            "finish_flow_label finishes a label while binding"
        );
        if node.a == 0 {
            unreachable
        } else if node.a == node.c {
            self.cells[node.a as usize - 1].item
        } else {
            flow
        }
    }
}

/// tsc activeLabelList entry (a linked list in tsc; a stack here).
#[derive(Clone, Debug)]
pub struct ActiveLabel {
    pub name: tsc_types::EscapedName,
    pub break_target: FlowId,
    pub continue_target: Option<FlowId>,
    pub referenced: bool,
}

use crate::declare::BinderWorker;
use crate::node_util::{
    is_expression_of_optional_chain_root, is_narrowing_expression, is_nullish_coalesce, kind_of,
    parent_of,
};
use tsc_syntax::SyntaxKind;

impl<'a> BinderWorker<'a> {
    /// tsc-port: createFlowCondition @6.0.3
    /// tsc-hash: 9e1f79f023aa72c19d5493060927a917d1231495eaa8d121d5453129722fe801
    /// tsc-span: _tsc.js:43107-43122
    ///
    /// The narrowing predicates GATE creation: a non-narrowing
    /// expression returns the antecedent unchanged.
    pub(crate) fn create_flow_condition(
        &mut self,
        flags: FlowFlags,
        antecedent: FlowId,
        expression: Option<NodeId>,
    ) -> FlowId {
        if self
            .flow
            .flow(antecedent)
            .flags
            .intersects(FlowFlags::UNREACHABLE)
        {
            return antecedent;
        }
        let Some(expression) = expression else {
            return if flags.intersects(FlowFlags::TRUE_CONDITION) {
                antecedent
            } else {
                self.unreachable_flow
            };
        };
        let kind = kind_of(self.source, expression);
        if (kind == SyntaxKind::TrueKeyword && flags.intersects(FlowFlags::FALSE_CONDITION)
            || kind == SyntaxKind::FalseKeyword && flags.intersects(FlowFlags::TRUE_CONDITION))
            && !is_expression_of_optional_chain_root(self.source, expression)
            && !parent_of(self.source, expression)
                .is_some_and(|parent| is_nullish_coalesce(self.source, parent))
        {
            return self.unreachable_flow;
        }
        if !is_narrowing_expression(self.source, expression) {
            return antecedent;
        }
        self.flow.set_flow_node_referenced(antecedent);
        self.flow
            .create_flow_node(flags, FlowPayload::Node(expression), Some(antecedent))
    }

    /// tsc-port: createFlowSwitchClause @6.0.3
    /// tsc-hash: 6fd653d39493395e9a40c41109ccf400a6844a09bf0b5789926e479187c6dba3
    /// tsc-span: _tsc.js:43123-43126
    pub(crate) fn create_flow_switch_clause(
        &mut self,
        antecedent: FlowId,
        switch_statement: NodeId,
        clause_start: u32,
        clause_end: u32,
    ) -> FlowId {
        self.flow.set_flow_node_referenced(antecedent);
        self.flow.create_flow_node(
            FlowFlags::SWITCH_CLAUSE,
            FlowPayload::SwitchClause {
                switch_statement,
                clause_start,
                clause_end,
            },
            Some(antecedent),
        )
    }

    /// tsc-port: createFlowMutation @6.0.3
    /// tsc-hash: 3ae00b4e65acf59b075ff96fcd003cc8c49808b5880a35a57e6734c2a00b2d84
    /// tsc-span: _tsc.js:43127-43135
    pub(crate) fn create_flow_mutation(
        &mut self,
        flags: FlowFlags,
        antecedent: FlowId,
        node: NodeId,
    ) -> FlowId {
        self.flow.set_flow_node_referenced(antecedent);
        self.has_flow_effects = true;
        let result = self
            .flow
            .create_flow_node(flags, FlowPayload::Node(node), Some(antecedent));
        if let Some(exception_target) = self.current_exception_target {
            self.flow.add_antecedent(exception_target, result);
        }
        result
    }

    /// tsc-port: createFlowCall @6.0.3
    /// tsc-hash: b6b6851fb5f624c20cecf4921ffa197df0a5a7692615145543e6eef61966337b
    /// tsc-span: _tsc.js:43136-43140
    pub(crate) fn create_flow_call(&mut self, antecedent: FlowId, node: NodeId) -> FlowId {
        self.flow.set_flow_node_referenced(antecedent);
        self.has_flow_effects = true;
        self.flow
            .create_flow_node(FlowFlags::CALL, FlowPayload::Node(node), Some(antecedent))
    }
}

#[cfg(test)]
#[path = "../tests/unit/flow/tests.rs"]
mod tests;
