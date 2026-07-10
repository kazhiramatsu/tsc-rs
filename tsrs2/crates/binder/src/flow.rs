//! Flow-node arena + the constructor family (m2-binder-steps.md stage
//! 3.3 scaffolding; stage 3.5 adds the condition/mutation/call
//! constructors and the narrowing predicates that gate them).
//!
//! tsc FlowNode (core-interfaces §5): `antecedent` holds one node for
//! plain kinds and an array for labels; both live in one Vec here.
//! NOTE (source fact, 43077-43149): createFlowNode does NOT mark its
//! antecedent Referenced — each caller (addAntecedent,
//! createFlowCondition/Mutation/Call/SwitchClause) does so explicitly.

use tsrs2_syntax::NodeId;
use tsrs2_types::FlowFlags;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FlowId(pub u32);

/// tsc stores the payload in FlowNode.node: an AST node for most
/// kinds, `{switchStatement, clauseStart, clauseEnd}` for SwitchClause
/// and `{target, antecedents}` for ReduceLabel.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum FlowPayload {
    #[default]
    None,
    Node(NodeId),
    SwitchClause {
        switch_statement: NodeId,
        clause_start: u32,
        clause_end: u32,
    },
    ReduceLabel {
        target: FlowId,
        antecedents: Vec<FlowId>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FlowNode {
    pub flags: FlowFlags,
    pub payload: FlowPayload,
    pub antecedent: Vec<FlowId>,
}

#[derive(Debug, Default)]
pub struct FlowArena {
    nodes: Vec<FlowNode>,
}

impl FlowArena {
    /// tsc-port: createFlowNode @6.0.3
    /// tsc-hash: 50f4e5850330909e853e82825ef01ab9cb1bcd9bdd13b31469b77b8127094539
    /// tsc-span: _tsc.js:42404-42406
    pub fn create_flow_node(
        &mut self,
        flags: FlowFlags,
        payload: FlowPayload,
        antecedent: Option<FlowId>,
    ) -> FlowId {
        let id = FlowId(self.nodes.len() as u32);
        self.nodes.push(FlowNode {
            flags,
            payload,
            antecedent: antecedent.into_iter().collect(),
        });
        id
    }

    pub fn flow(&self, id: FlowId) -> &FlowNode {
        &self.nodes[id.0 as usize]
    }

    pub fn flow_mut(&mut self, id: FlowId) -> &mut FlowNode {
        &mut self.nodes[id.0 as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// tsc createBranchLabel (43077).
    pub fn create_branch_label(&mut self) -> FlowId {
        self.create_flow_node(FlowFlags::BRANCH_LABEL, FlowPayload::None, None)
    }

    /// tsc createLoopLabel (43086).
    pub fn create_loop_label(&mut self) -> FlowId {
        self.create_flow_node(FlowFlags::LOOP_LABEL, FlowPayload::None, None)
    }

    /// tsc createReduceLabel (43095).
    pub fn create_reduce_label(
        &mut self,
        target: FlowId,
        antecedents: Vec<FlowId>,
        antecedent: FlowId,
    ) -> FlowId {
        self.create_flow_node(
            FlowFlags::REDUCE_LABEL,
            FlowPayload::ReduceLabel {
                target,
                antecedents,
            },
            Some(antecedent),
        )
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
        if self.flow(antecedent).flags.intersects(FlowFlags::UNREACHABLE) {
            return;
        }
        if self.flow(label).antecedent.contains(&antecedent) {
            return;
        }
        self.flow_mut(label).antecedent.push(antecedent);
        self.set_flow_node_referenced(antecedent);
    }

    /// tsc finishFlowLabel (43141): no antecedents ⇒ unreachable, one
    /// ⇒ COLLAPSES to it, several ⇒ the label itself.
    pub fn finish_flow_label(&mut self, flow: FlowId, unreachable: FlowId) -> FlowId {
        let antecedents = &self.flow(flow).antecedent;
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
    pub name: String,
    pub break_target: FlowId,
    pub continue_target: Option<FlowId>,
    pub referenced: bool,
}

use crate::declare::Binder;
use crate::node_util::{
    is_expression_of_optional_chain_root, is_narrowing_expression, is_nullish_coalesce,
    kind_of, parent_of,
};
use tsrs2_syntax::SyntaxKind;

impl<'a> Binder<'a> {
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
mod tests {
    use super::*;

    #[test]
    fn label_lifecycle_collapse_and_share() {
        let mut arena = FlowArena::default();
        let unreachable =
            arena.create_flow_node(FlowFlags::UNREACHABLE, FlowPayload::None, None);
        let start = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);

        // No antecedents ⇒ unreachable.
        let empty = arena.create_branch_label();
        assert_eq!(arena.finish_flow_label(empty, unreachable), unreachable);

        // One antecedent ⇒ collapses to it.
        let single = arena.create_branch_label();
        arena.add_antecedent(single, start);
        assert_eq!(arena.finish_flow_label(single, unreachable), start);
        assert!(arena.flow(start).flags.intersects(FlowFlags::REFERENCED));
        assert!(!arena.flow(start).flags.intersects(FlowFlags::SHARED));

        // Unreachable antecedents and duplicates are dropped.
        let multi = arena.create_branch_label();
        arena.add_antecedent(multi, unreachable);
        assert!(arena.flow(multi).antecedent.is_empty());
        arena.add_antecedent(multi, start);
        arena.add_antecedent(multi, start);
        assert_eq!(arena.flow(multi).antecedent.len(), 1);
        // Second REFERENCE marks Shared.
        assert!(arena.flow(start).flags.intersects(FlowFlags::SHARED));

        let other = arena.create_flow_node(FlowFlags::START, FlowPayload::None, None);
        arena.add_antecedent(multi, other);
        assert_eq!(arena.finish_flow_label(multi, unreachable), multi);
    }
}
