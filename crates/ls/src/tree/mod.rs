//! tsgo's syntax tree as tsc-rs's presents it: tsc-rs keeps tsc's shapes
//! where tsgo's parser changed them, and a [`View`] stands for each node or
//! NodeList of tsgo's tree, in tsgo's visitor order (the API's encoder, the
//! language service's token navigation). Kinds and node flags are tsgo's
//! numbers ([`generated`], `scripts/generate_api_encoder.py`).

mod generated;

use tsc_binder::BindData;
use tsc_syntax::{
    JSDocComment, Node, NodeArena, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind,
};
use tsc_types::NodeFlags;

pub use generated::*;

/// A node record's data: its children, a string or extended data (the
/// API's encoding, whose per-kind choice the generated tables make).
pub const NODE_DATA_TYPE_CHILDREN: u32 = 0;
pub const NODE_DATA_TYPE_STRING: u32 = 1 << 30;
pub const NODE_DATA_TYPE_EXTENDED_DATA: u32 = 2 << 30;
pub const NODE_DATA_TYPE_MASK: u32 = 0xc000_0000;
pub const NODE_DATA_CHILD_MASK: u32 = 0x0000_00ff;
pub const NODE_DATA_STRING_INDEX_MASK: u32 = 0x00ff_ffff;

/// The kind of a NodeList record.
pub const SYNTAX_KIND_NODE_LIST: u32 = u32::MAX;

/// A child property of a node, in tsgo's visitor order (`generated.rs`).
pub enum Property<'a> {
    Node(Option<NodeId>),
    /// A NodeList, encoded when present, even empty.
    List(Option<NodeArrayId>),
    /// tsgo's ModifierList, encoded only when it has a modifier.
    Modifiers(Option<NodeArrayId>),
    /// A plain slice: each element is a direct child.
    Slice(Option<NodeArrayId>),
    /// A JSDoc tag's comment: tsgo's NodeList of JSDocText and JSDocLink nodes.
    Comment(&'a Option<JSDocComment>),
    /// The implicit `export` modifier list of a nested namespace declaration.
    ImplicitModifiers,
    /// The elements of a type heritage clause (tsgo's TypeReferences).
    HeritageTypes(Option<NodeArrayId>),
}

/// A node or NodeList of tsgo's tree, as tsc-rs's tree presents it. tsgo's
/// parser builds some shapes tsc's does not; these views stand for them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    Node(NodeId),
    /// tsgo's TypeReference of an implemented or (interface) extended type,
    /// for tsc's ExpressionWithTypeArguments (tsgo parseTypeHeritageClauseElement).
    HeritageTypeReference(NodeId),
    /// tsgo's QualifiedName of a property access in such a type
    /// (tsgo convertEntityNameExpressionToEntityName).
    QualifiedName(NodeId),
    /// The implicit `export` modifier of a nested namespace declaration at
    /// a position (tsgo parseModuleOrNamespaceDeclaration; tsc flags the
    /// declaration NestedNamespace instead).
    ImplicitExport(u32),
    /// A JSDoc comment, starting where tsgo's does: at its host's position,
    /// or at the end of the host's previous JSDoc comment.
    JSDoc(NodeId, u32),
    List(NodeArrayId, Elements),
    /// The implicit modifier list of a nested namespace declaration.
    ImplicitModifiers(u32),
    /// tsgo's comment list of a JSDoc comment or tag (a JSDoc comment's is
    /// there even when empty).
    Comment(NodeId),
    /// tsgo's JSDocText of a comment tsc keeps as a string.
    CommentText(NodeId),
    /// The empty JSDocText tsgo ends a JSDoc comment's list with when only
    /// white space follows its last link.
    TrailingCommentText(NodeId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Elements {
    Plain,
    HeritageTypes,
    /// tsgo's `@template` type parameter list, whose range it never sets.
    TemplateTypeParameters,
}

impl View {
    /// The tsc-rs node an index of this view resolves to.
    pub fn node(self) -> Option<NodeId> {
        match self {
            View::Node(id)
            | View::HeritageTypeReference(id)
            | View::QualifiedName(id)
            | View::JSDoc(id, _) => Some(id),
            _ => None,
        }
    }
}

/// The file being encoded and the facts its views need.
pub struct Tree<'a> {
    pub file: &'a SourceFile,
    pub arena: &'a NodeArena,
    pub javascript: bool,
    pub bind_data: Option<&'a BindData>,
    /// Whether the nodes' JSDoc comments are children (tsgo visits them
    /// only with the source file).
    pub js_doc: bool,
}

impl<'a> Tree<'a> {
    pub fn new(file: &'a SourceFile, bind_data: Option<&'a BindData>) -> Self {
        let root_flags = NodeFlags::from_bits(file.arena.node(file.root).flags);
        Self {
            file,
            arena: &file.arena,
            javascript: root_flags.contains(NodeFlags::JAVA_SCRIPT_FILE),
            bind_data,
            js_doc: true,
        }
    }

    /// The tree of nodes encoded without their source file.
    pub fn detached(file: &'a SourceFile) -> Self {
        Self {
            js_doc: false,
            ..Self::new(file, None)
        }
    }

    pub fn node(&self, id: NodeId) -> &'a Node {
        self.arena.node(id)
    }

    /// `node`'s child properties as tsgo's tree has them.
    pub fn properties(&self, node: &'a Node, mut f: impl FnMut(Property<'a>)) {
        // A JSDoc namespace name keeps tsgo's NestedNamespace flag instead.
        let flags = NodeFlags::from_bits(node.flags);
        let nested_namespace = node.kind == SyntaxKind::ModuleDeclaration
            && flags.contains(NodeFlags::NESTED_NAMESPACE)
            && !flags.contains(NodeFlags::JS_DOC);
        // tsgo writes a JSDoc link's `A#b` as a QualifiedName.
        if let NodeData::JSDocMemberName(member) = &node.data {
            f(Property::Node(member.left));
            f(Property::Node(member.right));
            return;
        }
        let type_heritage = self.is_type_heritage_clause(node);
        for_each_property(node, |property| match property {
            Property::Modifiers(list)
                if nested_namespace
                    && list.is_none_or(|list| self.arena.node_array(list).nodes.is_empty()) =>
            {
                f(Property::ImplicitModifiers)
            }
            Property::List(list) if type_heritage => f(Property::HeritageTypes(list)),
            property => f(property),
        });
    }

    /// tsgo isTypeHeritageClause: an interface's `extends` or a class's
    /// `implements`.
    pub fn is_type_heritage_clause(&self, node: &Node) -> bool {
        let NodeData::HeritageClause(clause) = &node.data else {
            return false;
        };
        let Some(parent) = node.parent else {
            return false;
        };
        match self.node(parent).kind {
            SyntaxKind::InterfaceDeclaration => clause.token == SyntaxKind::ExtendsKeyword,
            SyntaxKind::ClassDeclaration | SyntaxKind::ClassExpression => {
                clause.token == SyntaxKind::ImplementsKeyword
            }
            _ => false,
        }
    }

    /// tsgo isValidHeritageTypeReferenceExpression.
    pub fn is_valid_heritage_type_reference_expression(&self, id: NodeId) -> bool {
        let node = self.node(id);
        match &node.data {
            NodeData::Identifier(_) => node.pos != node.end,
            NodeData::PropertyAccessExpression(access) => {
                !NodeFlags::from_bits(node.flags).contains(NodeFlags::OPTIONAL_CHAIN)
                    && access
                        .name
                        .is_some_and(|name| self.node(name).pos != self.node(name).end)
                    && access.expression.is_some_and(|expression| {
                        self.is_valid_heritage_type_reference_expression(expression)
                    })
            }
            _ => false,
        }
    }

    /// The view of a heritage clause element.
    pub fn heritage_element(&self, id: NodeId) -> View {
        match &self.node(id).data {
            NodeData::ExpressionWithTypeArguments(element)
                if element.expression.is_some_and(|expression| {
                    self.is_valid_heritage_type_reference_expression(expression)
                }) =>
            {
                View::HeritageTypeReference(id)
            }
            _ => View::Node(id),
        }
    }

    /// tsgo `Node.Parent` of a node view: the parents of a heritage type
    /// reference's expression are tsgo's TypeReference and QualifiedNames,
    /// and a JSDoc comment's is its host.
    pub fn parent(&self, view: View) -> Option<View> {
        let id = match view {
            View::Node(id) | View::QualifiedName(id) | View::JSDoc(id, _) => id,
            View::HeritageTypeReference(id) => return self.node(id).parent.map(View::Node),
            // A comment list's parent is its JSDoc comment or tag.
            View::Comment(owner) => return Some(self.view_of(owner)),
            View::CommentText(owner) | View::TrailingCommentText(owner) => {
                return Some(View::Comment(owner))
            }
            _ => return None,
        };
        let parent = self.node(id).parent?;
        Some(self.view_of(parent))
    }

    /// The view tsgo's tree has for the node `id`.
    pub fn view_of(&self, id: NodeId) -> View {
        let node = self.node(id);
        if node.kind == SyntaxKind::JSDoc {
            if let Some(host) = node.parent {
                let mut js_docs = Vec::new();
                self.js_doc_views(host, &mut js_docs);
                if let Some(&view) = js_docs
                    .iter()
                    .find(|view| matches!(view, View::JSDoc(js_doc, _) if *js_doc == id))
                {
                    return view;
                }
            }
        }
        if self.is_heritage_type_reference(id) {
            return View::HeritageTypeReference(id);
        }
        // A property access in such a reference's expression.
        let mut current = id;
        while let NodeData::PropertyAccessExpression(_) = &self.node(current).data {
            let Some(parent) = self.node(current).parent else {
                break;
            };
            match &self.node(parent).data {
                NodeData::PropertyAccessExpression(access)
                    if access.expression == Some(current) =>
                {
                    current = parent;
                }
                NodeData::ExpressionWithTypeArguments(element)
                    if element.expression == Some(current)
                        && self.is_heritage_type_reference(parent) =>
                {
                    return View::QualifiedName(id);
                }
                _ => break,
            }
        }
        View::Node(id)
    }

    /// Whether the node `id` is an element of a type heritage clause that
    /// tsgo parses as a TypeReference.
    fn is_heritage_type_reference(&self, id: NodeId) -> bool {
        let node = self.node(id);
        node.kind == SyntaxKind::ExpressionWithTypeArguments
            && node
                .parent
                .is_some_and(|clause| self.is_type_heritage_clause(self.node(clause)))
            && matches!(self.heritage_element(id), View::HeritageTypeReference(_))
    }

    /// tsgo convertEntityNameExpressionToEntityName.
    pub fn entity_name(&self, id: NodeId) -> View {
        match self.node(id).kind {
            SyntaxKind::PropertyAccessExpression => View::QualifiedName(id),
            _ => View::Node(id),
        }
    }

    /// Whether tsgo attaches `node`'s JSDoc comments: it never attaches
    /// them to a type parameter.
    pub fn js_doc(&self, node: &Node) -> Option<NodeArrayId> {
        node.js_doc
            .filter(|_| node.kind != SyntaxKind::TypeParameter)
            .filter(|&js_doc| !self.arena.node_array(js_doc).nodes.is_empty())
    }

    /// The comment of a JSDoc comment or tag.
    pub fn comment(&self, owner: NodeId) -> Option<&'a JSDocComment> {
        let mut comment = None;
        let node = self.node(owner);
        if let NodeData::JSDoc(js_doc) = &node.data {
            return js_doc.comment.as_ref();
        }
        for_each_property(node, |property| {
            if let Property::Comment(value) = property {
                comment = value.as_ref();
            }
        });
        comment
    }

    /// The range of the empty JSDocText tsgo ends a JSDoc comment's list
    /// with: tsgo keeps the white space after the last link as a part (its
    /// text trimmed away), unless a tag follows and drops it first.
    pub fn trailing_comment_text(&self, owner: NodeId) -> Option<(u32, u32)> {
        let NodeData::JSDoc(js_doc) = &self.node(owner).data else {
            return None;
        };
        let Some(JSDocComment::Nodes(list)) = &js_doc.comment else {
            return None;
        };
        if js_doc.tags.is_some() {
            return None;
        }
        let array = self.arena.node_array(*list);
        let last = self.node(*array.nodes.last()?);
        (last.kind != SyntaxKind::JSDocText && last.end < array.end)
            .then_some((last.end, array.end))
    }

    /// The flags of tsgo's JSDocText in the comment of `owner`: its parse
    /// context.
    pub fn comment_flags(&self, owner: NodeId) -> u32 {
        mapped_node_flags(self.node(owner).flags) & (TSGO_CONTEXT_FLAGS | TSGO_JSDOC)
    }

    /// The context flags tsgo's nodes of the JSDoc comment `id` lack: in a
    /// TypeScript file tsgo parses a host's JSDoc comments when they are
    /// first read, with a new parser and none of the host's context (unless
    /// they have a `@see` or `@link` tag, which it parses with the host).
    /// tsc-rs parses them with the host, as tsc does.
    pub fn lazy_js_doc_context(&self, id: NodeId) -> u32 {
        if self.javascript {
            return 0;
        }
        let node = self.node(id);
        let eager = node
            .parent
            .and_then(|host| self.node(host).js_doc)
            .is_some_and(|js_doc| {
                self.js_doc_texts_have_tag(js_doc, &["see", "link", "linkcode", "linkplain"])
            });
        if eager {
            0
        } else {
            mapped_node_flags(node.flags) & TSGO_CONTEXT_FLAGS
        }
    }

    /// Where tsgo's comment list of a JSDoc comment ends: at its first tag,
    /// or before the closing `*/`.
    pub fn js_doc_comments_end(&self, id: NodeId) -> u32 {
        let node = self.node(id);
        let NodeData::JSDoc(js_doc) = &node.data else {
            unreachable!("a JSDoc comment")
        };
        js_doc
            .tags
            .and_then(|tags| self.arena.node_array(tags).nodes.first().copied())
            .map_or(node.end.saturating_sub(2), |tag| self.node(tag).pos)
    }

    /// tsgo `Node.JSDoc`: the JSDoc comments of the node `id`, each starting
    /// at its host's position or at the end of the previous one.
    pub fn js_doc_views(&self, id: NodeId, out: &mut Vec<View>) {
        if !self.js_doc {
            return;
        }
        let node = self.node(id);
        if let Some(js_doc) = self.js_doc(node) {
            let mut pos = node.pos;
            for &comment in self.arena.node_array(js_doc).nodes {
                out.push(View::JSDoc(comment, pos));
                pos = self.node(comment).end;
            }
        }
    }

    /// tsgo's kind number of `view` ([`SYNTAX_KIND_NODE_LIST`] for a
    /// NodeList).
    pub fn kind_number(&self, view: View) -> u32 {
        self.kind(view).map_or(SYNTAX_KIND_NODE_LIST, |kind| {
            tsgo_kind(kind).unwrap_or_else(|| panic!("{kind:?} has no TypeScript 7.1 kind"))
        })
    }

    /// tsgo's kind of a node view; `None` for a NodeList.
    pub fn kind(&self, view: View) -> Option<SyntaxKind> {
        match view {
            View::Node(id) => Some(encoded_syntax_kind(self.arena, self.node(id))),
            View::HeritageTypeReference(_) => Some(SyntaxKind::TypeReference),
            View::QualifiedName(_) => Some(SyntaxKind::QualifiedName),
            View::ImplicitExport(_) => Some(SyntaxKind::ExportKeyword),
            View::JSDoc(..) => Some(SyntaxKind::JSDoc),
            View::CommentText(_) | View::TrailingCommentText(_) => Some(SyntaxKind::JSDocText),
            View::List(..) | View::ImplicitModifiers(_) | View::Comment(_) => None,
        }
    }

    /// The position and end tsgo's node or NodeList of `view` has (the ones
    /// [`Encoder::record`] writes, in bytes).
    pub fn range(&self, view: View) -> (u32, u32) {
        match view {
            View::Node(id) | View::HeritageTypeReference(id) | View::QualifiedName(id) => {
                let node = self.node(id);
                (node.pos, node.end)
            }
            View::ImplicitExport(pos) | View::ImplicitModifiers(pos) => (pos, pos),
            View::JSDoc(id, pos) => (pos, self.node(id).end),
            View::List(_, Elements::TemplateTypeParameters) => (0, 0),
            View::List(list, _) => {
                let array = self.arena.node_array(list);
                (array.pos, array.end)
            }
            View::Comment(owner) => match self.comment(owner) {
                Some(JSDocComment::Text { pos, end, .. }) => (*pos, *end),
                Some(JSDocComment::Nodes(list)) => {
                    let array = self.arena.node_array(*list);
                    (array.pos, array.end)
                }
                None => (self.node(owner).pos, self.js_doc_comments_end(owner)),
            },
            View::CommentText(owner) => match self.comment(owner) {
                Some(JSDocComment::Text { pos, text_end, .. }) => (*pos, *text_end),
                _ => unreachable!("a text comment"),
            },
            View::TrailingCommentText(owner) => {
                self.trailing_comment_text(owner).expect("a trailing text")
            }
        }
    }

    /// The children of `view`, in tsgo's visitor order.
    pub fn children(&self, view: View, out: &mut Vec<View>) {
        match view {
            View::Node(id) => {
                let node = self.node(id);
                self.properties(node, |property| match property {
                    Property::Node(Some(child)) => out.push(View::Node(child)),
                    Property::Modifiers(Some(list))
                        if !self.arena.node_array(list).nodes.is_empty() =>
                    {
                        out.push(View::List(list, Elements::Plain));
                    }
                    Property::Slice(Some(list)) => {
                        out.extend(
                            self.arena
                                .node_array(list)
                                .nodes
                                .iter()
                                .map(|&id| View::Node(id)),
                        );
                    }
                    Property::List(Some(list)) if node.kind == SyntaxKind::JSDocTemplateTag => {
                        out.push(View::List(list, Elements::TemplateTypeParameters));
                    }
                    Property::List(Some(list)) => out.push(View::List(list, Elements::Plain)),
                    Property::Comment(Some(_)) => out.push(View::Comment(id)),
                    Property::ImplicitModifiers => out.push(View::ImplicitModifiers(node.pos)),
                    Property::HeritageTypes(Some(list)) => {
                        out.push(View::List(list, Elements::HeritageTypes))
                    }
                    _ => {}
                });
                self.js_doc_views(id, out);
            }
            View::HeritageTypeReference(id) => {
                let NodeData::ExpressionWithTypeArguments(element) = &self.node(id).data else {
                    unreachable!("a heritage element")
                };
                out.push(
                    self.entity_name(element.expression.expect("a valid heritage expression")),
                );
                if let Some(arguments) = element.type_arguments {
                    out.push(View::List(arguments, Elements::Plain));
                }
            }
            View::QualifiedName(id) => {
                let NodeData::PropertyAccessExpression(access) = &self.node(id).data else {
                    unreachable!("a property access")
                };
                out.push(self.entity_name(access.expression.expect("a valid heritage expression")));
                out.push(View::Node(
                    access.name.expect("a valid heritage expression"),
                ));
            }
            View::JSDoc(id, _) => {
                out.push(View::Comment(id));
                if let NodeData::JSDoc(js_doc) = &self.node(id).data {
                    if let Some(tags) = js_doc.tags {
                        out.push(View::List(tags, Elements::Plain));
                    }
                }
            }
            View::Comment(owner) => match self.comment(owner) {
                Some(JSDocComment::Text { .. }) => out.push(View::CommentText(owner)),
                Some(JSDocComment::Nodes(list)) => {
                    out.extend(
                        self.arena
                            .node_array(*list)
                            .nodes
                            .iter()
                            .map(|&id| View::Node(id)),
                    );
                    if self.trailing_comment_text(owner).is_some() {
                        out.push(View::TrailingCommentText(owner));
                    }
                }
                None => {}
            },
            View::List(list, elements) => {
                for &element in self.arena.node_array(list).nodes {
                    out.push(match elements {
                        Elements::HeritageTypes => self.heritage_element(element),
                        Elements::Plain | Elements::TemplateTypeParameters => View::Node(element),
                    });
                }
            }
            View::ImplicitModifiers(pos) => out.push(View::ImplicitExport(pos)),
            View::ImplicitExport(_) | View::CommentText(_) | View::TrailingCommentText(_) => {}
        }
    }
}

/// tsgo's kind of `node`: tsgo writes a hole of an array binding pattern as
/// a BindingElement without children (tsc: an OmittedExpression) and a
/// JSDoc link's `A#b` as a QualifiedName (tsc: a JSDocMemberName).
pub fn encoded_kind(arena: &NodeArena, node: &Node) -> u32 {
    let kind = encoded_syntax_kind(arena, node);
    tsgo_kind(kind).unwrap_or_else(|| panic!("{kind:?} has no TypeScript 7.1 kind"))
}

/// [`encoded_kind`] before the kind's number.
pub fn encoded_syntax_kind(arena: &NodeArena, node: &Node) -> SyntaxKind {
    if node.kind == SyntaxKind::OmittedExpression
        && node
            .parent
            .is_some_and(|parent| arena.node(parent).kind == SyntaxKind::ArrayBindingPattern)
    {
        SyntaxKind::BindingElement
    } else if node.kind == SyntaxKind::JSDocMemberName {
        SyntaxKind::QualifiedName
    } else {
        node.kind
    }
}

/// tsgo `getChildrenPropertyMask`: bit `i` is set when the `i`th child
/// property is present.
pub fn children_property_mask(tree: &Tree<'_>, node: &Node) -> u32 {
    let mut mask = 0;
    let mut bit = 0;
    tree.properties(node, |property| {
        let present = match property {
            Property::Node(child) => child.is_some(),
            Property::List(list) | Property::HeritageTypes(list) => list.is_some(),
            Property::Modifiers(list) | Property::Slice(list) => {
                list.is_some_and(|list| !tree.arena.node_array(list).nodes.is_empty())
            }
            Property::Comment(comment) => comment.is_some(),
            Property::ImplicitModifiers => true,
        };
        mask |= u32::from(present) << bit;
        bit += 1;
    });
    mask & NODE_DATA_CHILD_MASK
}

/// tsgo `getNodeCommonData`: bits 24-29 of the node data.
pub fn common_data(node: &Node) -> u32 {
    let flag = |value: bool| u32::from(value) << 24;
    match &node.data {
        NodeData::Block(_)
        | NodeData::ArrayLiteralExpression(_)
        | NodeData::ObjectLiteralExpression(_) => flag(node.multi_line() == Some(true)),
        NodeData::HeritageClause(d) => flag(d.token == SyntaxKind::ImplementsKeyword),
        NodeData::ExportAssignment(d) => flag(d.is_export_equals == Some(true)),
        NodeData::ExportSpecifier(d) => flag(d.is_type_only),
        NodeData::PrefixUnaryExpression(d) => {
            let operator = match d.operator {
                SyntaxKind::MinusToken => 1,
                SyntaxKind::TildeToken => 2,
                SyntaxKind::ExclamationToken => 3,
                SyntaxKind::PlusPlusToken => 4,
                SyntaxKind::MinusMinusToken => 5,
                _ => 0,
            };
            operator << 24
        }
        NodeData::PostfixUnaryExpression(d) => flag(d.operator == SyntaxKind::MinusMinusToken),
        NodeData::MetaProperty(d) => flag(d.keyword_token == SyntaxKind::NewKeyword),
        NodeData::TypeOperator(d) => {
            let operator = match d.operator {
                SyntaxKind::ReadonlyKeyword => 1,
                SyntaxKind::UniqueKeyword => 2,
                _ => 0,
            };
            operator << 24
        }
        NodeData::ImportAttributes(d) => {
            flag(d.multi_line == Some(true)) | u32::from(d.token == SyntaxKind::AssertKeyword) << 25
        }
        NodeData::JsxText(d) => flag(d.contains_only_trivia_white_spaces),
        // tsgo's keyword: `namespace` for a namespace and for a JSDoc
        // typedef's or callback's namespace name (tsc: no Namespace flag).
        NodeData::ModuleDeclaration(_) => {
            let flags = NodeFlags::from_bits(node.flags);
            flag(flags.contains(NodeFlags::NAMESPACE) || flags.contains(NodeFlags::JS_DOC))
        }
        NodeData::ImportEqualsDeclaration(d) => flag(d.is_type_only),
        NodeData::ExportDeclaration(d) => flag(d.is_type_only),
        NodeData::ImportType(d) => flag(d.is_type_of),
        NodeData::ImportClause(d) => {
            let phase = if d.is_type_only {
                1
            } else if d.phase_modifier == Some(SyntaxKind::DeferKeyword) {
                2
            } else if d.phase_modifier == Some(SyntaxKind::SourceKeyword) {
                3
            } else {
                0
            };
            phase << 24
        }
        NodeData::ImportSpecifier(d) => flag(d.is_type_only),
        NodeData::JSDocTypeLiteral(d) => flag(d.is_array_type),
        NodeData::JSDocParameterTag(d) => flag(d.is_bracketed) | u32::from(d.is_name_first) << 25,
        NodeData::JSDocPropertyTag(d) => flag(d.is_bracketed) | u32::from(d.is_name_first) << 25,
        _ => 0,
    }
}

/// tsc NodeFlags (tsc-rs's) and tsgo's numbers of the same flags. tsgo
/// renumbered them, dropped tsc's module-keyword flags (`Namespace` and
/// `GlobalAugmentation` are its ModuleDeclaration keyword, a nested
/// declaration has an implicit `export` instead of `NestedNamespace`) and
/// checker caches, never sets `ThisNodeOrAnySubNodesHasError` (tsc's lazily
/// aggregated parse-error bit), and added `HasJSDoc`.
pub const NODE_FLAGS: [(i32, u32); 24] = [
    (NodeFlags::LET.bits(), 1 << 0),
    (NodeFlags::CONST.bits(), 1 << 1),
    (NodeFlags::USING.bits(), 1 << 2),
    (NodeFlags::SYNTHESIZED.bits(), 1 << 4),
    (NodeFlags::OPTIONAL_CHAIN.bits(), 1 << 5),
    (NodeFlags::EXPORT_CONTEXT.bits(), 1 << 6),
    // Also IdentifierHasExtendedUnicodeEscape, on both sides.
    (NodeFlags::CONTAINS_THIS.bits(), 1 << 7),
    (NodeFlags::HAS_IMPLICIT_RETURN.bits(), 1 << 8),
    (NodeFlags::HAS_EXPLICIT_RETURN.bits(), 1 << 9),
    (NodeFlags::DISALLOW_IN_CONTEXT.bits(), 1 << 10),
    (NodeFlags::YIELD_CONTEXT.bits(), 1 << 11),
    (NodeFlags::DECORATOR_CONTEXT.bits(), 1 << 12),
    (NodeFlags::AWAIT_CONTEXT.bits(), 1 << 13),
    (
        NodeFlags::DISALLOW_CONDITIONAL_TYPES_CONTEXT.bits(),
        1 << 14,
    ),
    (NodeFlags::THIS_NODE_HAS_ERROR.bits(), 1 << 15),
    (NodeFlags::JAVA_SCRIPT_FILE.bits(), 1 << 16),
    // Also IdentifierIsInJSDocNamespace, on both sides.
    (NodeFlags::HAS_ASYNC_FUNCTIONS.bits(), 1 << 18),
    (
        NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT.bits(),
        TSGO_POSSIBLY_CONTAINS_DYNAMIC_IMPORT,
    ),
    (NodeFlags::POSSIBLY_CONTAINS_IMPORT_META.bits(), 1 << 20),
    (NodeFlags::JS_DOC.bits(), 1 << 22),
    (NodeFlags::AMBIENT.bits(), 1 << 23),
    (NodeFlags::IN_WITH_STATEMENT.bits(), 1 << 24),
    (NodeFlags::JSON_FILE.bits(), 1 << 25),
    (NodeFlags::UNREACHABLE.bits(), 1 << 27),
];

/// tsc NodeFlags (tsc-rs's) to tsgo's ([`NODE_FLAGS`]).
pub fn mapped_node_flags(flags: i32) -> u32 {
    NODE_FLAGS
        .iter()
        .filter(|(from, _)| flags & from != 0)
        .fold(0, |out, (_, to)| out | to)
}

pub const TSGO_REPARSED: u32 = 1 << 3;
/// tsgo NodeFlagsNestedNamespace (OptionalChain's bit).
pub const TSGO_NESTED_NAMESPACE: u32 = 1 << 5;
/// tsgo NodeFlagsContextFlags.
pub const TSGO_CONTEXT_FLAGS: u32 =
    (1 << 10) | (1 << 11) | (1 << 12) | (1 << 13) | (1 << 14) | (1 << 16) | (1 << 23) | (1 << 24);
pub const TSGO_JSDOC: u32 = 1 << 22;
pub const TSGO_POSSIBLY_CONTAINS_DYNAMIC_IMPORT: u32 = 1 << 19;
pub const TSGO_HAS_JSDOC: u32 = 1 << 21;
pub const TSGO_POSSIBLY_CONTAINS_DEPRECATED_TAG: u32 = 1 << 26;

/// tsgo scanner `hasJSDocTag` over every `@` of a JSDoc comment's text.
pub fn js_doc_text_has_tag(text: &str, tags: &[&str]) -> bool {
    text.match_indices('@').any(|(at, _)| {
        let rest = &text[at + 1..];
        tags.iter().any(|tag| {
            rest.strip_prefix(tag).is_some_and(|after| {
                after
                    .bytes()
                    .next()
                    .is_none_or(|ch| matches!(ch, b' ' | b'\t' | b'\n' | b'\r' | b'}' | b'*'))
            })
        })
    })
}

impl Tree<'_> {
    /// tsgo's flags of a tsc-rs node. A TypeScript file's host has
    /// `PossiblyContainsDeprecatedTag` when one of its JSDoc comments has a
    /// `@deprecated` tag by tsgo's scanner text check (tsgo parses those
    /// comments lazily); a JavaScript file's when a parsed tag is one.
    pub fn node_flags(&self, id: NodeId, node: &Node) -> u32 {
        let bound = self
            .bind_data
            .map_or(node.flags, |data| data.flags_of(id, self.arena).bits());
        let mut flags = mapped_node_flags(bound);
        let tsc = NodeFlags::from_bits(node.flags);
        if tsc.contains(NodeFlags::NESTED_NAMESPACE) && tsc.contains(NodeFlags::JS_DOC) {
            flags |= TSGO_NESTED_NAMESPACE;
        }
        if let Some(js_doc) = self.js_doc(node) {
            flags |= TSGO_HAS_JSDOC;
            let deprecated = if self.javascript {
                NodeFlags::from_bits(node.flags).contains(NodeFlags::DEPRECATED)
            } else {
                self.js_doc_texts_have_tag(js_doc, &["deprecated"])
            };
            if deprecated {
                flags |= TSGO_POSSIBLY_CONTAINS_DEPRECATED_TAG;
            }
        }
        flags
    }

    pub fn js_doc_texts_have_tag(&self, js_doc: NodeArrayId, tags: &[&str]) -> bool {
        let text = self.file.text();
        self.arena.node_array(js_doc).nodes.iter().any(|&comment| {
            let comment = self.node(comment);
            text.get(comment.pos as usize..comment.end as usize)
                .is_some_and(|comment| js_doc_text_has_tag(comment, tags))
        })
    }

    /// tsgo's `PossiblyContainsDynamicImport` of the file: set by an import
    /// call or an import type its parser parses eagerly. A TypeScript
    /// file's JSDoc comments are parsed when first read, unless the host's
    /// comments have a `@see` or `@link` tag, so those parse after the file
    /// is complete and leave the flag alone.
    pub fn possibly_contains_dynamic_import(&self) -> bool {
        let root = self.node(self.file.root);
        if !NodeFlags::from_bits(root.flags).contains(NodeFlags::POSSIBLY_CONTAINS_DYNAMIC_IMPORT) {
            return false;
        }
        self.arena.nodes().iter().any(|node| {
            let dynamic = match &node.data {
                // An `import` keyword expression: tsgo flags `import` before
                // `(` or `<` (a call or an instantiation).
                NodeData::Token => node.kind == SyntaxKind::ImportKeyword,
                NodeData::ImportType(_) => true,
                // `import.defer(...)` or `import.source(...)`.
                NodeData::CallExpression(call) => call.expression.is_some_and(|callee| {
                    tsc_syntax::utilities::is_import_phase_meta_property(self.arena, callee)
                }),
                _ => false,
            };
            dynamic
                && (self.javascript
                    || !NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC)
                    || {
                        let mut current = node.parent;
                        let host = loop {
                            let Some(id) = current else { break None };
                            let ancestor = self.node(id);
                            if ancestor.kind == SyntaxKind::JSDoc {
                                break ancestor.parent;
                            }
                            current = ancestor.parent;
                        };
                        host.and_then(|host| self.node(host).js_doc)
                            .is_some_and(|js_doc| {
                                self.js_doc_texts_have_tag(
                                    js_doc,
                                    &["see", "link", "linkcode", "linkplain"],
                                )
                            })
                    })
        })
    }
}

/// tsgo `ast.Kind.String()` of an encoded kind; `NodeList` for a list.
pub fn kind_name(kind: u32) -> &'static str {
    if kind == SYNTAX_KIND_NODE_LIST {
        return "NodeList";
    }
    KIND_NAMES.get(kind as usize).copied().unwrap_or("Kind(?)")
}
