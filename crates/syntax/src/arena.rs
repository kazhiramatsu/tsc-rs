use crate::for_each_child::{for_each_child, NodeLookup};
use crate::nodes::{Node, NodeArray, NodeArrayId, NodeData, NodeId};
use crate::SyntaxKind;
use tsc_types::NodeFlags;

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

impl NodeLookup for NodeArena {
    fn node(&self, id: NodeId) -> &Node {
        self.node(id)
    }

    fn node_array(&self, id: NodeArrayId) -> &NodeArray {
        self.node_array(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::{
        IdentifierData, JSDocComment, JSDocParameterTagData, JSDocTextData,
        JSDocTypeExpressionData, JSDocTypeLiteralData, JSDocTypedefTagData, SourceFileData,
        StringLiteralData,
    };

    #[test]
    fn finalizes_parent_links_and_error_aggregation() {
        let mut arena = NodeArena::new();
        let stmt = arena.alloc_node(
            NodeData::StringLiteral(StringLiteralData {
                text: "x".to_owned(),
                has_extended_unicode_escape: None,
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

    #[test]
    fn jsdoc_child_order_and_comment_union_follow_tsc() {
        let mut arena = NodeArena::new();
        let identifier = |arena: &mut NodeArena, text: &str, pos: usize| {
            arena.alloc_node(
                NodeData::Identifier(IdentifierData {
                    escaped_text: text.to_owned(),
                    text: text.to_owned(),
                }),
                pos,
                pos + text.len(),
                NodeFlags::JS_DOC,
            )
        };
        let tag_name = identifier(&mut arena, "param", 1);
        let name = identifier(&mut arena, "value", 7);
        let type_node = identifier(&mut arena, "T", 14);
        let type_expression = arena.alloc_node(
            NodeData::JSDocTypeExpression(JSDocTypeExpressionData {
                r#type: Some(type_node),
            }),
            13,
            16,
            NodeFlags::JS_DOC,
        );
        let comment_text = arena.alloc_node(
            NodeData::JSDocText(JSDocTextData {
                text: "description".to_owned(),
            }),
            17,
            28,
            NodeFlags::JS_DOC,
        );
        let comments = arena.alloc_array(vec![comment_text], 17, 28, false);

        for (is_name_first, expected) in [
            (true, vec![tag_name, name, type_expression, comment_text]),
            (false, vec![tag_name, type_expression, name, comment_text]),
        ] {
            let parameter = arena.alloc_node(
                NodeData::JSDocParameterTag(JSDocParameterTagData {
                    tag_name: Some(tag_name),
                    comment: Some(JSDocComment::Nodes(comments)),
                    name: Some(name),
                    type_expression: Some(type_expression),
                    is_name_first,
                    is_bracketed: false,
                }),
                0,
                28,
                NodeFlags::JS_DOC,
            );
            let mut actual = Vec::new();
            for_each_child(&arena, arena.node(parameter), |child| {
                actual.push(child);
                false
            });
            assert_eq!(actual, expected);
        }

        let full_name = identifier(&mut arena, "Alias", 29);
        let typedef = arena.alloc_node(
            NodeData::JSDocTypedefTag(JSDocTypedefTagData {
                tag_name: Some(tag_name),
                comment: Some(JSDocComment::Text("plain".to_owned())),
                name: Some(full_name),
                full_name: Some(full_name),
                type_expression: Some(type_expression),
            }),
            29,
            40,
            NodeFlags::JS_DOC,
        );
        let mut actual = Vec::new();
        for_each_child(&arena, arena.node(typedef), |child| {
            actual.push(child);
            false
        });
        assert_eq!(actual, [tag_name, type_expression, full_name]);

        let property_tags = arena.empty_array(41);
        let type_literal = arena.alloc_node(
            NodeData::JSDocTypeLiteral(JSDocTypeLiteralData {
                js_doc_property_tags: Some(property_tags),
                is_array_type: false,
            }),
            41,
            41,
            NodeFlags::JS_DOC,
        );
        let typedef = arena.alloc_node(
            NodeData::JSDocTypedefTag(JSDocTypedefTagData {
                tag_name: Some(tag_name),
                comment: None,
                name: Some(full_name),
                full_name: Some(full_name),
                type_expression: Some(type_literal),
            }),
            41,
            42,
            NodeFlags::JS_DOC,
        );
        let mut actual = Vec::new();
        for_each_child(&arena, arena.node(typedef), |child| {
            actual.push(child);
            false
        });
        assert_eq!(actual, [tag_name, full_name, type_literal]);
    }

    #[test]
    fn synthetic_node_array_preserves_tsc_negative_span() {
        let mut arena = NodeArena::new();
        let array = arena.alloc_synthetic_array(Vec::new());
        assert_eq!(arena.node_array(array).pos, u32::MAX);
        assert_eq!(arena.node_array(array).end, u32::MAX);
        assert!(!arena.node_array(array).has_trailing_comma);
    }
}
