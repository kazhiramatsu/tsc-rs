use crate::for_each_child::{for_each_child, NodeLookup};
use crate::nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId};
use crate::SyntaxKind;
use tsrs2_types::NodeFlags;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeArena {
    nodes: Vec<Node>,
    arrays: Vec<NodeArray>,
    /// Program-wide id bases (M4 5.0): tsc nodes are heap objects with
    /// program-unique identity; per-file arenas get the same property
    /// by allocating NodeId/NodeArrayId from a per-file base so a
    /// multi-file checker never sees two nodes share an id. Single-file
    /// paths (relpin, ast-diff, tests) keep base 0 and are unchanged.
    node_base: u32,
    array_base: u32,
}

impl NodeArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_bases(node_base: u32, array_base: u32) -> Self {
        Self {
            node_base,
            array_base,
            ..Self::default()
        }
    }

    pub fn node_base(&self) -> u32 {
        self.node_base
    }

    pub fn array_base(&self) -> u32 {
        self.array_base
    }

    /// One past the last allocated NodeId — the next file's node base.
    pub fn node_end(&self) -> u32 {
        self.node_base + self.nodes.len() as u32
    }

    /// One past the last allocated NodeArrayId.
    pub fn array_end(&self) -> u32 {
        self.array_base + self.arrays.len() as u32
    }

    pub fn contains_node(&self, id: NodeId) -> bool {
        id.0 >= self.node_base && id.0 < self.node_end()
    }

    /// All NodeIds of this arena, in allocation order.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        (self.node_base..self.node_end()).map(NodeId)
    }

    pub fn alloc_node(
        &mut self,
        data: NodeData,
        pos: usize,
        end: usize,
        flags: NodeFlags,
    ) -> NodeId {
        let kind = data
            .kind()
            .expect("NodeData::Token must be allocated with alloc_token");
        self.push_node(kind, data, pos, end, flags)
    }

    pub fn alloc_token(
        &mut self,
        kind: SyntaxKind,
        pos: usize,
        end: usize,
        flags: NodeFlags,
    ) -> NodeId {
        self.push_node(kind, NodeData::Token, pos, end, flags)
    }

    pub fn alloc_missing(&mut self, kind: SyntaxKind, pos: usize) -> NodeId {
        self.push_node(kind, NodeData::missing(kind), pos, pos, NodeFlags::NONE)
    }

    pub fn alloc_array(
        &mut self,
        nodes: Vec<NodeId>,
        pos: usize,
        end: usize,
        has_trailing_comma: bool,
    ) -> NodeArrayId {
        let id = NodeArrayId(self.array_base + self.arrays.len() as u32);
        self.arrays.push(NodeArray {
            nodes,
            pos: pos as u32,
            end: end as u32,
            has_trailing_comma,
            is_missing_list: false,
        });
        id
    }

    pub fn empty_array(&mut self, pos: usize) -> NodeArrayId {
        self.alloc_array(Vec::new(), pos, pos, false)
    }

    /// tsc createMissingList: an empty list tagged so isMissingList checks
    /// (typeHasArrowFunctionBlockingParseError) can distinguish it from `()`.
    pub fn missing_array(&mut self, pos: usize) -> NodeArrayId {
        let id = self.empty_array(pos);
        let index = self.array_index(id);
        self.arrays[index].is_missing_list = true;
        id
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[self.node_index(id)]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        let index = self.node_index(id);
        &mut self.nodes[index]
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    pub fn node_array(&self, id: NodeArrayId) -> &NodeArray {
        &self.arrays[self.array_index(id)]
    }

    pub fn node_arrays(&self) -> &[NodeArray] {
        &self.arrays
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn finalize_tree(&mut self, root: NodeId) {
        let mut seen = vec![false; self.nodes.len()];
        self.finalize_node(root, None, &mut seen);
    }

    fn push_node(
        &mut self,
        kind: SyntaxKind,
        data: NodeData,
        pos: usize,
        end: usize,
        flags: NodeFlags,
    ) -> NodeId {
        let id = NodeId(self.node_base + self.nodes.len() as u32);
        self.nodes.push(Node {
            kind,
            flags: flags.bits(),
            pos: pos as u32,
            end: end as u32,
            parent: None,
            data,
        });
        id
    }

    /// Explicit two-phase stack: deep trees (left-leaning binary
    /// chains) overflow a recursive walk.
    fn finalize_node(&mut self, root: NodeId, parent: Option<NodeId>, seen: &mut [bool]) -> bool {
        enum Phase {
            Enter,
            Exit,
        }
        let mut error_flags = vec![false; self.nodes.len()];
        let mut stack = vec![(root, parent, Phase::Enter)];
        while let Some((id, parent, phase)) = stack.pop() {
            let index = self.node_index(id);
            match phase {
                Phase::Enter => {
                    assert!(!seen[index], "node has more than one parent: {id:?}");
                    seen[index] = true;
                    self.nodes[index].parent = parent;
                    error_flags[index] = NodeFlags::from_bits(self.nodes[index].flags)
                        .contains(NodeFlags::THIS_NODE_HAS_ERROR);
                    stack.push((id, parent, Phase::Exit));
                    let children = self.children(id);
                    for child in children.into_iter().rev() {
                        stack.push((child, Some(id), Phase::Enter));
                    }
                }
                Phase::Exit => {
                    let mut contains_error = error_flags[index];
                    for child in self.children(id) {
                        if error_flags[self.node_index(child)] {
                            contains_error = true;
                        }
                    }
                    if contains_error {
                        self.nodes[index].flags |=
                            NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR.bits();
                        error_flags[index] = true;
                    }
                }
            }
        }
        error_flags[self.node_index(root)]
    }

    fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut children = Vec::new();
        for_each_child(self, self.node(id), |child| {
            children.push(child);
            false
        });
        children
    }

    fn node_index(&self, id: NodeId) -> usize {
        assert!(
            id.0 >= self.node_base,
            "NodeId below arena base: {id:?} (base {})",
            self.node_base
        );
        let index = (id.0 - self.node_base) as usize;
        assert!(index < self.nodes.len(), "invalid NodeId: {id:?}");
        index
    }

    fn array_index(&self, id: NodeArrayId) -> usize {
        assert!(
            id.0 >= self.array_base,
            "NodeArrayId below arena base: {id:?} (base {})",
            self.array_base
        );
        let index = (id.0 - self.array_base) as usize;
        assert!(index < self.arrays.len(), "invalid NodeArrayId: {id:?}");
        index
    }
}

impl NodeLookup for NodeArena {
    fn node_array(&self, id: NodeArrayId) -> &NodeArray {
        self.node_array(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{SourceFileData, StringLiteralData};

    #[test]
    fn finalizes_parent_links_and_error_aggregation() {
        let mut arena = NodeArena::new();
        let stmt = arena.alloc_node(
            NodeData::StringLiteral(StringLiteralData {
                text: "x".to_owned(),
            }),
            0,
            1,
            NodeFlags::THIS_NODE_HAS_ERROR,
        );
        let statements = arena.alloc_array(vec![stmt], 0, 1, false);
        let eof = arena.alloc_token(SyntaxKind::EndOfFileToken, 1, 1, NodeFlags::NONE);
        let root = arena.alloc_node(
            NodeData::SourceFile(SourceFileData {
                statements: Some(statements),
                end_of_file_token: Some(eof),
            }),
            0,
            1,
            NodeFlags::NONE,
        );

        arena.finalize_tree(root);

        assert_eq!(arena.node(stmt).parent, Some(root));
        assert_eq!(arena.node(eof).parent, Some(root));
        assert!(NodeFlags::from_bits(arena.node(root).flags)
            .contains(NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR));
    }
}
