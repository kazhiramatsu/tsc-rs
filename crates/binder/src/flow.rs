//! Flow-node arena + the constructor family (m2-binder-steps.md stage
//! 3.3 scaffolding; stage 3.5 adds the condition/mutation/call
//! constructors and the narrowing predicates that gate them).
//!
//! tsc FlowNode (core-interfaces §5): `antecedent` holds one node for
//! plain kinds and an array for labels; both live in one Vec here.
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
/// | BranchLabel / LoopLabel | the antecedent list | its length |
/// | ReduceLabel | the target flow | the antecedent list |
///
/// A list is a staging index while the file is bound and a pool offset
/// once the arena is frozen (see [`FlowArena::freeze`]).
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
    pub fn antecedent(&self) -> Option<FlowId> {
        self.antecedent
    }

    fn is_label(&self) -> bool {
        self.flags
            .intersects(FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL)
    }
}

/// One file's flow graph. While the file is bound, the label lists are
/// staged one per list (`add_antecedent` appends, `set_label_antecedents`
/// replaces); [`Self::freeze`] writes them once into one exact pool, in
/// creation order, and drops the staging.
#[derive(Clone, Debug, Default)]
pub struct FlowArena {
    nodes: Vec<FlowNode>,
    staging: Vec<Vec<FlowId>>,
    pool: Box<[FlowId]>,
    frozen: bool,
}

impl FlowArena {
    /// Flow node count and the bytes the arena and its antecedent lists
    /// own on the heap, for memory accounting.
    pub fn memory_usage(&self) -> (usize, usize, usize) {
        let staged = self.staging.iter().map(Vec::capacity).sum::<usize>();
        (
            self.nodes.len(),
            self.nodes.capacity() * std::mem::size_of::<FlowNode>(),
            (staged + self.pool.len()) * std::mem::size_of::<FlowId>(),
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
                self.stage(vec![FlowId::new(clause_start), FlowId::new(clause_end)]),
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

    fn stage(&mut self, list: Vec<FlowId>) -> u32 {
        let index = self.staging.len() as u32;
        self.staging.push(list);
        index
    }

    pub fn flow(&self, id: FlowId) -> &FlowNode {
        &self.nodes[id.index() as usize]
    }

    fn flow_mut(&mut self, id: FlowId) -> &mut FlowNode {
        &mut self.nodes[id.index() as usize]
    }

    /// A staged or frozen list.
    fn list(&self, at: u32, len: Option<u32>) -> &[FlowId] {
        if self.frozen {
            let start = at as usize;
            match len {
                Some(len) => &self.pool[start..start + len as usize],
                // A reduce label's list carries its length first.
                None => {
                    let len = self.pool[start].index() as usize;
                    &self.pool[start + 1..start + 1 + len]
                }
            }
        } else {
            &self.staging[at as usize]
        }
    }

    /// The antecedents in tsc's order: a label's list, or the plain kinds'
    /// single antecedent (none for the start and unreachable nodes).
    pub fn antecedents(&self, id: FlowId) -> &[FlowId] {
        let node = self.flow(id);
        if node.is_label() {
            self.list(node.a, Some(node.c))
        } else {
            node.antecedent.as_slice()
        }
    }

    /// tsc FlowNode.node, with a label's or a reduce label's list.
    pub fn payload(&self, id: FlowId) -> FlowData<'_> {
        let node = self.flow(id);
        if node.is_label() {
            FlowData::Label {
                antecedents: self.list(node.a, Some(node.c)),
            }
        } else if node.flags.intersects(FlowFlags::REDUCE_LABEL) {
            FlowData::ReduceLabel {
                target: FlowId::new(node.a),
                antecedents: self.list(node.c, None),
            }
        } else if node.flags.intersects(FlowFlags::SWITCH_CLAUSE) {
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

    /// Write the staged lists once into one exact pool (labels: start and
    /// length; switch clauses: the clause pair; reduce labels: the length,
    /// then the list) and drop the staging. A bound file's arena does not
    /// grow again.
    pub fn freeze(&mut self) {
        if self.frozen {
            return;
        }
        let staged = std::mem::take(&mut self.staging);
        let reduce_labels = self
            .nodes
            .iter()
            .filter(|node| node.flags.intersects(FlowFlags::REDUCE_LABEL))
            .count();
        let mut pool =
            Vec::with_capacity(staged.iter().map(Vec::len).sum::<usize>() + reduce_labels);
        for node in &mut self.nodes {
            if node
                .flags
                .intersects(FlowFlags::BRANCH_LABEL | FlowFlags::LOOP_LABEL)
            {
                let list = &staged[node.a as usize];
                node.a = pool.len() as u32;
                node.c = list.len() as u32;
                pool.extend_from_slice(list);
            } else if node.flags.intersects(FlowFlags::REDUCE_LABEL) {
                let list = &staged[node.c as usize];
                node.c = pool.len() as u32;
                pool.push(FlowId::new(list.len() as u32));
                pool.extend_from_slice(list);
            } else if node.flags.intersects(FlowFlags::SWITCH_CLAUSE) {
                let list = &staged[node.c as usize];
                node.c = pool.len() as u32;
                pool.extend_from_slice(list);
            }
        }
        self.nodes.shrink_to_fit();
        self.pool = pool.into_boxed_slice();
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
        let list = self.stage(Vec::new());
        self.push(FlowNode {
            flags,
            a: list,
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
        let list = self.stage(antecedents);
        self.push(FlowNode {
            flags: FlowFlags::REDUCE_LABEL,
            a: target.index(),
            antecedent: Some(antecedent),
            c: list,
        })
    }

    fn label_antecedents_mut(&mut self, label: FlowId) -> &mut Vec<FlowId> {
        let node = *self.flow(label);
        assert!(
            node.is_label() && !self.frozen,
            "only branch and loop labels collect antecedents, while binding"
        );
        &mut self.staging[node.a as usize]
    }

    /// Replace a label's antecedent list (the finally label of a try
    /// statement takes the normal, exception and return exits at once).
    pub fn set_label_antecedents(&mut self, label: FlowId, antecedents: Vec<FlowId>) {
        *self.label_antecedents_mut(label) = antecedents;
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
        if self.antecedents(label).contains(&antecedent) {
            return;
        }
        self.label_antecedents_mut(label).push(antecedent);
        self.set_flow_node_referenced(antecedent);
    }

    /// tsc finishFlowLabel (43141): no antecedents ⇒ unreachable, one
    /// ⇒ COLLAPSES to it, several ⇒ the label itself.
    pub fn finish_flow_label(&mut self, flow: FlowId, unreachable: FlowId) -> FlowId {
        let antecedents = self.antecedents(flow);
        match antecedents.len() {
            0 => unreachable,
            1 => antecedents[0],
            _ => flow,
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
