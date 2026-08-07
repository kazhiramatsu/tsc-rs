use crate::for_each_child::{for_each_child, NodeLookup};
use crate::nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId};
use crate::relocate::{collect_node_data_ids, relocate_node_data, remap_node_data_ids};
use crate::SyntaxKind;
use tsc_types::{IdentityError, IdentityLease, IdentityRange, IdentitySpace, NodeFlags};

#[derive(Clone, Debug, Default)]
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
    node_lease: Option<IdentityLease>,
    array_lease: Option<IdentityLease>,
}

impl PartialEq for NodeArena {
    fn eq(&self, other: &Self) -> bool {
        // A lease is an ownership capability, not observable AST content.
        self.nodes == other.nodes
            && self.arrays == other.arrays
            && self.node_base == other.node_base
            && self.array_base == other.array_base
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SyntaxIdentityRelocation {
    old_nodes: IdentityRange,
    new_nodes: IdentityRange,
    old_arrays: IdentityRange,
    new_arrays: IdentityRange,
}

/// Reusable dense scratch state for immutable old-subtree copies.
///
/// Old arena IDs are contiguous, so hash tables per list element are both
/// unnecessary and disproportionately expensive for large files containing
/// thousands of small declarations. Generation-stamped dense maps preserve
/// the same complete schema-generated relocation while allocating scratch
/// storage only once per incremental parse.
#[derive(Debug)]
pub(crate) struct SubtreeCopier {
    old_node_base: u32,
    old_array_base: u32,
    generation: u32,
    node_marks: Vec<u32>,
    array_marks: Vec<u32>,
    node_map: Vec<NodeId>,
    array_map: Vec<NodeArrayId>,
    pending_nodes: Vec<NodeId>,
    pending_arrays: Vec<NodeArrayId>,
    old_nodes: Vec<NodeId>,
    old_arrays: Vec<NodeArrayId>,
    node_lineage: Vec<(NodeId, NodeId)>,
    new_arrays: Vec<NodeArrayId>,
}

impl SubtreeCopier {
    pub(crate) fn new(old: &NodeArena) -> Self {
        Self {
            old_node_base: old.node_base(),
            old_array_base: old.array_base(),
            generation: 0,
            node_marks: vec![0; old.nodes.len()],
            array_marks: vec![0; old.arrays.len()],
            node_map: vec![NodeId(0); old.nodes.len()],
            array_map: vec![NodeArrayId(0); old.arrays.len()],
            pending_nodes: Vec::new(),
            pending_arrays: Vec::new(),
            old_nodes: Vec::new(),
            old_arrays: Vec::new(),
            node_lineage: Vec::new(),
            new_arrays: Vec::new(),
        }
    }

    pub(crate) fn node_lineage(&self) -> &[(NodeId, NodeId)] {
        &self.node_lineage
    }

    pub(crate) fn new_arrays(&self) -> &[NodeArrayId] {
        &self.new_arrays
    }

    fn node_index(&self, id: NodeId) -> usize {
        let index =
            id.0.checked_sub(self.old_node_base)
                .expect("copied NodeId is below the old arena base") as usize;
        assert!(
            index < self.node_marks.len(),
            "copied NodeId is outside the old arena"
        );
        index
    }

    fn array_index(&self, id: NodeArrayId) -> usize {
        let index =
            id.0.checked_sub(self.old_array_base)
                .expect("copied NodeArrayId is below the old arena base") as usize;
        assert!(
            index < self.array_marks.len(),
            "copied NodeArrayId is outside the old arena"
        );
        index
    }

    fn begin_copy(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if self.generation == 0 {
            self.node_marks.fill(0);
            self.array_marks.fill(0);
            self.generation = 1;
        }
        self.pending_nodes.clear();
        self.pending_arrays.clear();
        self.old_nodes.clear();
        self.old_arrays.clear();
        self.node_lineage.clear();
        self.new_arrays.clear();
    }

    pub(crate) fn copy_subtree_from(
        &mut self,
        destination: &mut NodeArena,
        old: &NodeArena,
        root: NodeId,
        position_delta: i64,
    ) -> NodeId {
        assert_eq!(old.node_base(), self.old_node_base);
        assert_eq!(old.array_base(), self.old_array_base);
        assert_eq!(old.nodes.len(), self.node_marks.len());
        assert_eq!(old.arrays.len(), self.array_marks.len());
        self.begin_copy();
        self.pending_nodes.push(root);

        while !self.pending_nodes.is_empty() || !self.pending_arrays.is_empty() {
            if let Some(id) = self.pending_nodes.pop() {
                let index = self.node_index(id);
                if self.node_marks[index] == self.generation {
                    continue;
                }
                self.node_marks[index] = self.generation;
                let node = old.node(id);
                self.old_nodes.push(id);
                collect_node_data_ids(
                    &node.data,
                    &mut self.pending_nodes,
                    &mut self.pending_arrays,
                );
                if let Some(js_doc) = node.js_doc {
                    self.pending_arrays.push(js_doc);
                }
                continue;
            }

            let id = self
                .pending_arrays
                .pop()
                .expect("a non-empty identity work list has an element");
            let index = self.array_index(id);
            if self.array_marks[index] == self.generation {
                continue;
            }
            self.array_marks[index] = self.generation;
            let array = old.node_array(id);
            self.old_arrays.push(id);
            self.pending_nodes.extend(array.nodes.iter().copied());
        }

        for old_id in &self.old_nodes {
            let old_node = old.node(*old_id);
            let id = destination.push_node(
                old_node.kind,
                old_node.data.clone(),
                shifted_position(old_node.pos, position_delta) as usize,
                shifted_position(old_node.end, position_delta) as usize,
                NodeFlags::from_bits(old_node.flags),
            );
            let copied = destination.node_mut(id);
            copied.numeric_literal_flags = old_node.numeric_literal_flags;
            copied.multi_line = old_node.multi_line;
            let index = self.node_index(*old_id);
            self.node_map[index] = id;
            self.node_lineage.push((*old_id, id));
        }

        for old_id in &self.old_arrays {
            let old_array = old.node_array(*old_id);
            let id = destination.alloc_array(
                old_array.nodes.clone(),
                shifted_position(old_array.pos, position_delta) as usize,
                shifted_position(old_array.end, position_delta) as usize,
                old_array.has_trailing_comma,
            );
            let destination_index = destination.array_index(id);
            destination.arrays[destination_index].is_missing_list = old_array.is_missing_list;
            let old_index = self.array_index(*old_id);
            self.array_map[old_index] = id;
            self.new_arrays.push(id);
        }

        for (old_id, new_id) in &self.node_lineage {
            let old_node = old.node(*old_id);
            let node = destination.node_mut(*new_id);
            remap_node_data_ids(
                &mut node.data,
                |id| {
                    let index = (id.0 - self.old_node_base) as usize;
                    debug_assert_eq!(self.node_marks[index], self.generation);
                    self.node_map[index]
                },
                |id| {
                    let index = (id.0 - self.old_array_base) as usize;
                    debug_assert_eq!(self.array_marks[index], self.generation);
                    self.array_map[index]
                },
            );
            node.parent = None;
            node.js_doc = old_node.js_doc.map(|id| {
                let index = self.array_index(id);
                debug_assert_eq!(self.array_marks[index], self.generation);
                self.array_map[index]
            });
        }

        for old_id in &self.old_arrays {
            let old_index = self.array_index(*old_id);
            let new_id = self.array_map[old_index];
            let destination_index = destination.array_index(new_id);
            for node in &mut destination.arrays[destination_index].nodes {
                let index = (node.0 - self.old_node_base) as usize;
                debug_assert_eq!(self.node_marks[index], self.generation);
                *node = self.node_map[index];
            }
        }

        let root_index = self.node_index(root);
        self.node_map[root_index]
    }
}

impl SyntaxIdentityRelocation {
    pub(crate) fn node(&self, id: &mut NodeId) -> Result<(), IdentityError> {
        relocate_raw(
            &mut id.0,
            IdentitySpace::Node,
            self.old_nodes,
            self.new_nodes,
        )
    }

    pub(crate) fn node_array(&self, id: &mut NodeArrayId) -> Result<(), IdentityError> {
        relocate_raw(
            &mut id.0,
            IdentitySpace::NodeArray,
            self.old_arrays,
            self.new_arrays,
        )
    }
}

fn relocate_raw(
    value: &mut u32,
    space: IdentitySpace,
    old: IdentityRange,
    new: IdentityRange,
) -> Result<(), IdentityError> {
    if old.len() != new.len() {
        return Err(IdentityError::InvalidLease {
            space,
            detail: "relocation ranges have different lengths",
        });
    }
    let offset = value
        .checked_sub(old.start())
        .filter(|offset| *offset < old.len())
        .ok_or(IdentityError::InvalidLease {
            space,
            detail: "relocated id is outside its source arena",
        })?;
    *value = new
        .start()
        .checked_add(offset)
        .ok_or(IdentityError::InvalidLease {
            space,
            detail: "relocated id overflowed",
        })?;
    Ok(())
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

    pub fn node_identity_lease(&self) -> Option<&IdentityLease> {
        self.node_lease.as_ref()
    }

    pub fn array_identity_lease(&self) -> Option<&IdentityLease> {
        self.array_lease.as_ref()
    }

    pub fn has_identity_leases(&self) -> bool {
        self.node_lease.is_some() && self.array_lease.is_some()
    }

    /// One past the last allocated NodeId — the next file's node base.
    pub fn node_end(&self) -> u32 {
        self.node_base
            .checked_add(u32::try_from(self.nodes.len()).expect("node arena length exceeds u32"))
            .expect("node arena id space exhausted")
    }

    /// One past the last allocated NodeArrayId.
    pub fn array_end(&self) -> u32 {
        self.array_base
            .checked_add(
                u32::try_from(self.arrays.len()).expect("node-array arena length exceeds u32"),
            )
            .expect("node-array arena id space exhausted")
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
        let offset = u32::try_from(self.arrays.len()).expect("node-array arena length exceeds u32");
        let id = NodeArrayId(
            self.array_base
                .checked_add(offset)
                .expect("node-array identity space exhausted"),
        );
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

    /// tsc factory-created arrays that are not parsed list ranges carry
    /// `pos = end = -1`. NodeArray uses unsigned parser offsets, so the
    /// all-ones value is the arena representation of that synthetic span.
    pub fn alloc_synthetic_array(&mut self, nodes: Vec<NodeId>) -> NodeArrayId {
        self.alloc_array(nodes, u32::MAX as usize, u32::MAX as usize, false)
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

    pub fn set_js_doc(&mut self, host: NodeId, js_doc: NodeArrayId) {
        self.node_mut(host).js_doc = Some(js_doc);
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

    pub(crate) fn identity_relocation(
        &self,
        node_lease: &IdentityLease,
        array_lease: &IdentityLease,
    ) -> Result<SyntaxIdentityRelocation, IdentityError> {
        if self.node_lease.is_some() || self.array_lease.is_some() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Node,
                detail: "syntax arena is already identity-owned",
            });
        }
        validate_lease(
            node_lease,
            IdentitySpace::Node,
            self.node_base,
            self.node_end(),
            false,
        )?;
        validate_lease(
            array_lease,
            IdentitySpace::NodeArray,
            self.array_base,
            self.array_end(),
            false,
        )?;
        if !node_lease.same_domain(array_lease) {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Node,
                detail: "node and node-array leases belong to different domains",
            });
        }
        Ok(SyntaxIdentityRelocation {
            old_nodes: IdentityRange::new(self.node_base, self.node_end()),
            new_nodes: node_lease.range(),
            old_arrays: IdentityRange::new(self.array_base, self.array_end()),
            new_arrays: array_lease.range(),
        })
    }

    pub(crate) fn apply_identity_relocation(
        &mut self,
        relocation: SyntaxIdentityRelocation,
        node_lease: IdentityLease,
        array_lease: IdentityLease,
    ) -> Result<(), IdentityError> {
        for node in &mut self.nodes {
            if let Some(parent) = &mut node.parent {
                relocation.node(parent)?;
            }
            if let Some(js_doc) = &mut node.js_doc {
                relocation.node_array(js_doc)?;
            }
            relocate_node_data(&mut node.data, &relocation)?;
        }
        for array in &mut self.arrays {
            for node in &mut array.nodes {
                relocation.node(node)?;
            }
        }
        self.node_base = relocation.new_nodes.start();
        self.array_base = relocation.new_arrays.start();
        self.node_lease = Some(node_lease);
        self.array_lease = Some(array_lease);
        Ok(())
    }

    pub(crate) fn attach_identity_leases(
        &mut self,
        node_lease: IdentityLease,
        array_lease: IdentityLease,
    ) -> Result<(), IdentityError> {
        if self.node_lease.is_some() || self.array_lease.is_some() {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Node,
                detail: "syntax arena is already identity-owned",
            });
        }
        validate_lease(
            &node_lease,
            IdentitySpace::Node,
            self.node_base,
            self.node_end(),
            true,
        )?;
        validate_lease(
            &array_lease,
            IdentitySpace::NodeArray,
            self.array_base,
            self.array_end(),
            true,
        )?;
        if !node_lease.same_domain(&array_lease) {
            return Err(IdentityError::InvalidLease {
                space: IdentitySpace::Node,
                detail: "node and node-array leases belong to different domains",
            });
        }
        self.node_lease = Some(node_lease);
        self.array_lease = Some(array_lease);
        Ok(())
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
        let offset = u32::try_from(self.nodes.len()).expect("node arena length exceeds u32");
        let id = NodeId(
            self.node_base
                .checked_add(offset)
                .expect("node identity space exhausted"),
        );
        self.nodes.push(Node {
            kind,
            flags: flags.bits(),
            numeric_literal_flags: 0,
            multi_line: None,
            pos: pos as u32,
            end: end as u32,
            parent: None,
            js_doc: None,
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
                    let children = self.children_including_js_doc(id);
                    for child in children.into_iter().rev() {
                        stack.push((child, Some(id), Phase::Enter));
                    }
                }
                Phase::Exit => {
                    let mut contains_error = error_flags[index];
                    let flags = NodeFlags::from_bits(self.nodes[index].flags);
                    if !flags.contains(NodeFlags::JS_DOC) {
                        // tsc's lazy aggregateChildData follows public
                        // forEachChild, which excludes node.jsDoc. JSDoc
                        // parents are fixed up recursively, but their parse
                        // errors are not aggregated into the attached host
                        // (or eagerly through the JSDoc subtree).
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

    /// Parent finalization includes the internal Node.jsDoc attachment,
    /// while public for_each_child deliberately does not. This mirrors
    /// tsc setParentRecursive/bindJSDoc and keeps ordinary syntax walks
    /// from visiting documentation twice.
    fn children_including_js_doc(&self, id: NodeId) -> Vec<NodeId> {
        let mut children = self.children(id);
        if let Some(js_doc) = self.node(id).js_doc {
            children.extend(self.node_array(js_doc).nodes.iter().copied());
        }
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

fn shifted_position(position: u32, delta: i64) -> u32 {
    if position == u32::MAX {
        return position;
    }
    let shifted = i64::from(position)
        .checked_add(delta)
        .expect("incremental subtree position overflow");
    u32::try_from(shifted).expect("validated incremental edit keeps positions in the u32 domain")
}

fn validate_lease(
    lease: &IdentityLease,
    space: IdentitySpace,
    old_start: u32,
    old_end: u32,
    require_same_base: bool,
) -> Result<(), IdentityError> {
    if lease.space() != space {
        return Err(IdentityError::InvalidLease {
            space,
            detail: "lease has the wrong identity space",
        });
    }
    if lease.range().len() != old_end - old_start {
        return Err(IdentityError::InvalidLease {
            space,
            detail: "lease length differs from the arena allocation count",
        });
    }
    if require_same_base && lease.range().start() != old_start {
        return Err(IdentityError::InvalidLease {
            space,
            detail: "direct-construction lease base differs from the arena base",
        });
    }
    Ok(())
}

impl NodeLookup for NodeArena {
    fn node(&self, id: NodeId) -> &Node {
        self.node(id)
    }

    fn node_array(&self, id: NodeArrayId) -> &NodeArray {
        self.node_array(id)
    }
}

#[cfg(test)]
#[path = "../tests/unit/arena/tests.rs"]
mod tests;
