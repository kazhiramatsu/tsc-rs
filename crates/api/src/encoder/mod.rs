//! The binary source-file format of TypeScript 7.1's API (tsgo
//! `internal/api/encoder`, protocol 9), written from tsc-rs's syntax tree.
//!
//! An encoding is a 64-byte header, the string offsets, the string data
//! (the file text, then the strings that are not slices of it), the
//! extended node data, the structured (msgpack) data and the nodes, 28
//! bytes each: kind, pos, end, next sibling, parent, data and flags, in
//! tsgo's kind numbers, UTF-16 positions and node flags. The nodes are in
//! tsgo's visitor order: a NodeList is a node of kind `0xffff_ffff` whose
//! elements are its children, and a node's JSDoc comments follow its
//! children. tsgo's `encoder.go` documents each section.
//!
//! The tree is tsc-rs's, which keeps tsc's shapes where tsgo's parser
//! changed them. The encoder writes tsgo's shapes from it (`View`): a type
//! heritage element as a TypeReference of an entity name, a nested
//! namespace's implicit `export`, a JSDoc comment's text as a list of
//! JSDocText, an array binding hole as a BindingElement, `A#b` in a JSDoc
//! link as a QualifiedName, JSDoc comments at tsgo's positions and with
//! the context tsgo's lazy JSDoc parse gives a TypeScript file, and tsgo's
//! node and literal token flags. What it cannot derive stays tsc's: the
//! declarations and types tsgo's parser reparses from JSDoc tags in a
//! JavaScript file, and the flags and positions of some parse-error
//! recoveries (docs/design/greenfield/slices/ts71-suites/README.md, P4-7d).

mod generated;
mod string_table;

use std::collections::HashMap;
use std::sync::OnceLock;

use tsc_binder::BindData;
use tsc_syntax::{
    JSDocComment, Node, NodeArena, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind,
};
use tsc_types::{JsString, NodeFlags};

pub(crate) use generated::*;
use string_table::StringTable;

pub const PROTOCOL_VERSION: u8 = 9;

pub const NODE_OFFSET_KIND: usize = 0;
pub const NODE_OFFSET_POS: usize = 4;
pub const NODE_OFFSET_END: usize = 8;
pub const NODE_OFFSET_NEXT: usize = 12;
pub const NODE_OFFSET_PARENT: usize = 16;
pub const NODE_OFFSET_DATA: usize = 20;
pub const NODE_OFFSET_FLAGS: usize = 24;
/// The bytes of one node record.
pub const NODE_SIZE: usize = 28;

pub const NODE_DATA_TYPE_CHILDREN: u32 = 0;
pub const NODE_DATA_TYPE_STRING: u32 = 1 << 30;
pub const NODE_DATA_TYPE_EXTENDED_DATA: u32 = 2 << 30;
pub const NODE_DATA_TYPE_MASK: u32 = 0xc000_0000;
pub const NODE_DATA_CHILD_MASK: u32 = 0x0000_00ff;
pub const NODE_DATA_STRING_INDEX_MASK: u32 = 0x00ff_ffff;

/// The kind of a NodeList record.
pub const SYNTAX_KIND_NODE_LIST: u32 = u32::MAX;

pub const HEADER_OFFSET_METADATA: usize = 0;
pub const HEADER_OFFSET_HASH_LO0: usize = 4;
pub const HEADER_OFFSET_HASH_LO1: usize = 8;
pub const HEADER_OFFSET_HASH_HI0: usize = 12;
pub const HEADER_OFFSET_HASH_HI1: usize = 16;
pub const HEADER_OFFSET_PARSE_OPTIONS: usize = 20;
pub const HEADER_OFFSET_STRING_OFFSETS: usize = 24;
pub const HEADER_OFFSET_STRING_DATA: usize = 28;
pub const HEADER_OFFSET_EXTENDED_DATA: usize = 32;
pub const HEADER_OFFSET_STRUCTURED_DATA: usize = 36;
pub const HEADER_OFFSET_NODES: usize = 40;
pub const HEADER_OFFSET_SOURCE_FILE_ID: usize = 44;
pub const HEADER_OFFSET_SOURCE_FILE_LEASE: usize = 52;
pub const HEADER_OFFSET_BINDER_DATA: usize = 60;
pub const HEADER_SIZE: usize = 64;

/// An absent structured-data entry or string index.
pub const NO_STRUCTURED_DATA: u32 = u32::MAX;

/// tsgo `core.ScriptKind`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScriptKind {
    #[default]
    Unknown = 0,
    Js = 1,
    Jsx = 2,
    Ts = 3,
    Tsx = 4,
    Json = 6,
}

impl ScriptKind {
    /// The kind of tsgo's `core.ScriptKind` number among those a client may
    /// create a source file of (JS, JSX, TS, TSX, JSON).
    pub fn from_number(number: u32) -> Option<Self> {
        match number {
            1 => Some(Self::Js),
            2 => Some(Self::Jsx),
            3 => Some(Self::Ts),
            4 => Some(Self::Tsx),
            6 => Some(Self::Json),
            _ => None,
        }
    }

    /// tsgo `core.GetScriptKindFromFileName`.
    pub fn from_file_name(file_name: &str) -> Self {
        let Some(dot) = file_name.rfind('.') else {
            return Self::Unknown;
        };
        match file_name[dot..].to_ascii_lowercase().as_str() {
            ".js" | ".cjs" | ".mjs" => Self::Js,
            ".jsx" => Self::Jsx,
            ".ts" | ".cts" | ".mts" => Self::Ts,
            ".tsx" => Self::Tsx,
            ".json" => Self::Json,
            _ => Self::Unknown,
        }
    }
}

/// What tsgo's `SourceFile` records beside its tree: the encoding writes
/// them into the source file's extended and structured data.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceFileFacts<'a> {
    /// tsgo `SourceFile.Path()`; the file name when `None`.
    pub path: Option<&'a str>,
    pub script_kind: ScriptKind,
    /// The program's module specifiers of the file (tsgo `Imports()`).
    pub imports: &'a [NodeId],
    /// The program's module augmentation names (tsgo `ModuleAugmentations`).
    pub module_augmentations: &'a [NodeId],
    /// tsgo `AmbientModuleNames`.
    pub ambient_module_names: &'a [JsString],
    /// The file's binding, when it is bound: tsgo's binder sets node flags
    /// in place (reachability, `this` use, export context, async
    /// functions), so a bound file's encoding has them.
    pub bind_data: Option<&'a BindData>,
    /// A file without tsgo's content hash (`SourceFile.Hash`, which the
    /// parse cache sets): a config file's source, whose header hash is zero.
    pub unhashed: bool,
}

/// tsgo `NodeIndexTable`: the node of each encoded index (`None` for index
/// 0 and for NodeLists).
#[derive(Debug, Default)]
pub struct NodeIndexTable {
    nodes: Vec<Option<NodeId>>,
    /// tsgo's kind of each index ([`SYNTAX_KIND_NODE_LIST`] for a NodeList).
    kinds: Vec<u32>,
    indices: OnceLock<HashMap<NodeId, u32>>,
}

impl NodeIndexTable {
    pub fn nodes(&self) -> &[Option<NodeId>] {
        &self.nodes
    }

    /// tsgo's kind of the node at `index`.
    pub fn kind(&self, index: u32) -> u32 {
        self.kinds.get(index as usize).copied().unwrap_or(0)
    }

    /// The encoded index of `node`, 0 when it is not in the table.
    pub fn get_index(&self, node: NodeId) -> u32 {
        self.indices
            .get_or_init(|| {
                self.nodes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, node)| Some(((*node)?, index as u32)))
                    .collect()
            })
            .get(&node)
            .copied()
            .unwrap_or(0)
    }
}

/// tsgo `EncodeSourceFile`.
pub fn encode_source_file(
    file: &SourceFile,
    facts: &SourceFileFacts<'_>,
) -> (Vec<u8>, NodeIndexTable) {
    encode_tree(file, file.root, Some(facts))
}

/// tsgo `EncodeNode`: `node` (of `file`) and its descendants, with no file
/// text in the string data and no hash or parse options in the header.
pub fn encode_node(file: &SourceFile, node: NodeId) -> (Vec<u8>, NodeIndexTable) {
    encode_tree(file, node, None)
}

/// tsgo `BuildNodeIndexTable`: the indices `encode_source_file` gives the
/// file's nodes, without encoding them.
pub fn build_node_index_table(file: &SourceFile) -> NodeIndexTable {
    let mut nodes = vec![None];
    let mut kinds = vec![0];
    let tree = Tree::new(file, None);
    walk(&tree, View::Node(file.root), |event| {
        if let WalkEvent::Record { view, .. } = event {
            nodes.push(view.node());
            kinds.push(tree.kind_number(view));
        }
    });
    NodeIndexTable {
        nodes,
        kinds,
        indices: OnceLock::new(),
    }
}

/// tsgo `ast.Kind.String()` of an encoded kind; `NodeList` for a list.
pub fn kind_name(kind: u32) -> &'static str {
    if kind == SYNTAX_KIND_NODE_LIST {
        return "NodeList";
    }
    KIND_NAMES.get(kind as usize).copied().unwrap_or("Kind(?)")
}

/// A child property of a node, in tsgo's visitor order (`generated.rs`).
pub(crate) enum Property<'a> {
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
pub(crate) enum View {
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
pub(crate) enum Elements {
    Plain,
    HeritageTypes,
    /// tsgo's `@template` type parameter list, whose range it never sets.
    TemplateTypeParameters,
}

impl View {
    /// The tsc-rs node an index of this view resolves to.
    pub(crate) fn node(self) -> Option<NodeId> {
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
pub(crate) struct Tree<'a> {
    file: &'a SourceFile,
    arena: &'a NodeArena,
    javascript: bool,
    bind_data: Option<&'a BindData>,
}

impl<'a> Tree<'a> {
    pub(crate) fn new(file: &'a SourceFile, bind_data: Option<&'a BindData>) -> Self {
        let root_flags = NodeFlags::from_bits(file.arena.node(file.root).flags);
        Self {
            file,
            arena: &file.arena,
            javascript: root_flags.contains(NodeFlags::JAVA_SCRIPT_FILE),
            bind_data,
        }
    }

    pub(crate) fn node(&self, id: NodeId) -> &'a Node {
        self.arena.node(id)
    }

    /// `node`'s child properties as tsgo's tree has them.
    fn properties(&self, node: &'a Node, mut f: impl FnMut(Property<'a>)) {
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
    fn is_type_heritage_clause(&self, node: &Node) -> bool {
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
    fn is_valid_heritage_type_reference_expression(&self, id: NodeId) -> bool {
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
    fn heritage_element(&self, id: NodeId) -> View {
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

    /// tsgo convertEntityNameExpressionToEntityName.
    fn entity_name(&self, id: NodeId) -> View {
        match self.node(id).kind {
            SyntaxKind::PropertyAccessExpression => View::QualifiedName(id),
            _ => View::Node(id),
        }
    }

    /// Whether tsgo attaches `node`'s JSDoc comments: it never attaches
    /// them to a type parameter.
    pub(crate) fn js_doc(&self, node: &Node) -> Option<NodeArrayId> {
        node.js_doc
            .filter(|_| node.kind != SyntaxKind::TypeParameter)
            .filter(|&js_doc| !self.arena.node_array(js_doc).nodes.is_empty())
    }

    /// The comment of a JSDoc comment or tag.
    fn comment(&self, owner: NodeId) -> Option<&'a JSDocComment> {
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
    fn trailing_comment_text(&self, owner: NodeId) -> Option<(u32, u32)> {
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
    fn comment_flags(&self, owner: NodeId) -> u32 {
        mapped_node_flags(self.node(owner).flags) & (TSGO_CONTEXT_FLAGS | TSGO_JSDOC)
    }

    /// The context flags tsgo's nodes of the JSDoc comment `id` lack: in a
    /// TypeScript file tsgo parses a host's JSDoc comments when they are
    /// first read, with a new parser and none of the host's context (unless
    /// they have a `@see` or `@link` tag, which it parses with the host).
    /// tsc-rs parses them with the host, as tsc does.
    fn lazy_js_doc_context(&self, id: NodeId) -> u32 {
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
    fn js_doc_comments_end(&self, id: NodeId) -> u32 {
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
    pub(crate) fn js_doc_views(&self, id: NodeId, out: &mut Vec<View>) {
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
    fn kind_number(&self, view: View) -> u32 {
        self.kind(view).map_or(SYNTAX_KIND_NODE_LIST, |kind| {
            tsgo_kind(kind).unwrap_or_else(|| panic!("{kind:?} has no TypeScript 7.1 kind"))
        })
    }

    /// tsgo's kind of a node view; `None` for a NodeList.
    pub(crate) fn kind(&self, view: View) -> Option<SyntaxKind> {
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
    pub(crate) fn range(&self, view: View) -> (u32, u32) {
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
    pub(crate) fn children(&self, view: View, out: &mut Vec<View>) {
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

/// tsgo's `PossiblyContainsDynamicImport` of `file` (see
/// `Tree::possibly_contains_dynamic_import`).
pub(crate) fn tsgo_possibly_contains_dynamic_import(file: &SourceFile) -> bool {
    Tree::new(file, None).possibly_contains_dynamic_import()
}

enum WalkEvent {
    /// `strip`: the flags tsgo's parse of this node did not have (see
    /// `Tree::lazy_js_doc_context`).
    Record { view: View, parent: u32, strip: u32 },
    /// `index` is the next sibling of `previous`.
    Next { previous: u32, index: u32 },
}

enum Step {
    Enter(View, u32),
    /// The end of the children of `index`; the walk returns to `parent`.
    Exit {
        index: u32,
        parent: u32,
    },
}

/// tsgo `encodeTree`'s visitor: the root is index 1, every node and list
/// is numbered in visitor order, and each one's children follow it.
/// Iterative, so a deep tree needs no deep stack.
fn walk(tree: &Tree<'_>, root: View, mut on: impl FnMut(WalkEvent)) {
    let mut count = 1;
    on(WalkEvent::Record {
        view: root,
        parent: 0,
        strip: 0,
    });
    let mut previous = 0;
    let mut parent = 1;
    let mut stack = Vec::new();
    let mut children = Vec::new();
    tree.children(root, &mut children);
    stack.extend(children.drain(..).rev().map(|view| Step::Enter(view, 0)));
    while let Some(step) = stack.pop() {
        match step {
            Step::Exit {
                index,
                parent: saved,
            } => {
                previous = index;
                parent = saved;
            }
            Step::Enter(view, inherited) => {
                count += 1;
                if previous != 0 {
                    on(WalkEvent::Next {
                        previous,
                        index: count,
                    });
                }
                let strip = match view {
                    View::JSDoc(id, _) => tree.lazy_js_doc_context(id),
                    _ => inherited,
                };
                on(WalkEvent::Record {
                    view,
                    parent,
                    strip,
                });
                stack.push(Step::Exit {
                    index: count,
                    parent,
                });
                tree.children(view, &mut children);
                stack.extend(
                    children
                        .drain(..)
                        .rev()
                        .map(|view| Step::Enter(view, strip)),
                );
                previous = 0;
                parent = count;
            }
        }
    }
}

/// tsgo's kind of `node`: tsgo writes a hole of an array binding pattern as
/// a BindingElement without children (tsc: an OmittedExpression) and a
/// JSDoc link's `A#b` as a QualifiedName (tsc: a JSDocMemberName).
fn encoded_kind(arena: &NodeArena, node: &Node) -> u32 {
    let kind = encoded_syntax_kind(arena, node);
    tsgo_kind(kind).unwrap_or_else(|| panic!("{kind:?} has no TypeScript 7.1 kind"))
}

/// [`encoded_kind`] before the kind's number.
fn encoded_syntax_kind(arena: &NodeArena, node: &Node) -> SyntaxKind {
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
fn children_property_mask(tree: &Tree<'_>, node: &Node) -> u32 {
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
fn common_data(node: &Node) -> u32 {
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

/// tsc NodeFlags (tsc-rs's) to tsgo's: tsgo renumbered them, dropped tsc's
/// module-keyword flags (`Namespace` and `GlobalAugmentation` are its
/// ModuleDeclaration keyword, a nested declaration has an implicit `export`
/// instead of `NestedNamespace`) and checker caches, never sets
/// `ThisNodeOrAnySubNodesHasError` (tsc's lazily aggregated parse-error
/// bit), and added `HasJSDoc`.
fn mapped_node_flags(flags: i32) -> u32 {
    const MAP: [(i32, u32); 24] = [
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
    MAP.iter()
        .filter(|(from, _)| flags & from != 0)
        .fold(0, |out, (_, to)| out | to)
}

const TSGO_REPARSED: u32 = 1 << 3;
/// tsgo NodeFlagsNestedNamespace (OptionalChain's bit).
const TSGO_NESTED_NAMESPACE: u32 = 1 << 5;
/// tsgo NodeFlagsContextFlags.
const TSGO_CONTEXT_FLAGS: u32 =
    (1 << 10) | (1 << 11) | (1 << 12) | (1 << 13) | (1 << 14) | (1 << 16) | (1 << 23) | (1 << 24);
const TSGO_JSDOC: u32 = 1 << 22;
const TSGO_POSSIBLY_CONTAINS_DYNAMIC_IMPORT: u32 = 1 << 19;
const TSGO_HAS_JSDOC: u32 = 1 << 21;
const TSGO_POSSIBLY_CONTAINS_DEPRECATED_TAG: u32 = 1 << 26;

/// tsgo scanner `hasJSDocTag` over every `@` of a JSDoc comment's text.
fn js_doc_text_has_tag(text: &str, tags: &[&str]) -> bool {
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
    fn node_flags(&self, id: NodeId, node: &Node) -> u32 {
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

    fn js_doc_texts_have_tag(&self, js_doc: NodeArrayId, tags: &[&str]) -> bool {
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
    fn possibly_contains_dynamic_import(&self) -> bool {
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
                // `import.defer(...)`.
                NodeData::CallExpression(call) => call.expression.is_some_and(|callee| {
                    matches!(&self.node(callee).data, NodeData::MetaProperty(meta)
                        if meta.keyword_token == SyntaxKind::ImportKeyword
                            && meta.name.is_some_and(|name| matches!(&self.node(name).data,
                                NodeData::Identifier(identifier) if identifier.escaped_text == "defer")))
                }),
                _ => false,
            };
            dynamic && (self.javascript || !NodeFlags::from_bits(node.flags).contains(NodeFlags::JS_DOC) || {
                let mut current = node.parent;
                let host = loop {
                    let Some(id) = current else { break None };
                    let ancestor = self.node(id);
                    if ancestor.kind == SyntaxKind::JSDoc {
                        break ancestor.parent;
                    }
                    current = ancestor.parent;
                };
                host.and_then(|host| self.node(host).js_doc).is_some_and(|js_doc| {
                    self.js_doc_texts_have_tag(js_doc, &["see", "link", "linkcode", "linkplain"])
                })
            })
        })
    }
}

/// tsgo `ast.PositionMap`: UTF-8 and UTF-16 offsets of a text.
pub(crate) struct PositionMap {
    /// The UTF-8 offset after each non-ASCII character and the cumulative
    /// UTF-8 minus UTF-16 length there.
    entries: Vec<(u32, u32)>,
}

impl PositionMap {
    pub(crate) fn new(text: &str) -> Self {
        let mut entries = Vec::new();
        let mut delta = 0;
        for (offset, ch) in text.char_indices() {
            if !ch.is_ascii() {
                delta += (ch.len_utf8() - ch.len_utf16()) as u32;
                entries.push(((offset + ch.len_utf8()) as u32, delta));
            }
        }
        Self { entries }
    }

    /// tsgo `UTF8ToUTF16`.
    fn utf16(&self, offset: u32) -> u32 {
        let after = self
            .entries
            .partition_point(|&(position, _)| position <= offset);
        match after {
            0 => offset,
            _ => offset - self.entries[after - 1].1,
        }
    }

    /// tsgo `UTF16ToUTF8`: an offset inside a character or past the end
    /// maps by the delta before it.
    pub(crate) fn utf8(&self, offset: u32) -> u32 {
        let after = self
            .entries
            .partition_point(|&(position, delta)| position - delta <= offset);
        match after {
            0 => offset,
            _ => offset + self.entries[after - 1].1,
        }
    }
}

struct Encoder<'a> {
    file: &'a SourceFile,
    tree: Tree<'a>,
    positions: PositionMap,
    strings: StringTable<'a>,
    nodes: Vec<u8>,
    extended_data: Vec<u8>,
    structured_data: Vec<u8>,
}

fn encode_tree(
    file: &SourceFile,
    root: NodeId,
    facts: Option<&SourceFileFacts<'_>>,
) -> (Vec<u8>, NodeIndexTable) {
    let is_source_file = file.arena.node(root).kind == SyntaxKind::SourceFile && facts.is_some();
    let bind_data = facts.and_then(|facts| facts.bind_data);
    let mut encoder = Encoder {
        file,
        tree: Tree::new(file, bind_data),
        positions: PositionMap::new(file.text()),
        strings: StringTable::new(if is_source_file {
            file.text().as_bytes()
        } else {
            &[]
        }),
        nodes: Vec::with_capacity((file.node_count() + 2) * NODE_SIZE),
        extended_data: Vec::new(),
        structured_data: Vec::new(),
    };
    // Index 0 is the nil node.
    encoder.nodes.resize(NODE_SIZE, 0);
    let mut table = vec![None];
    let mut kinds = vec![0];
    let mut tracked: HashMap<NodeId, u32> = HashMap::new();
    if let Some(facts) = facts {
        for &id in facts.imports.iter().chain(facts.module_augmentations) {
            tracked.insert(id, 0);
        }
        if let Some(indicator) = file.external_module_indicator.filter(|&id| id != root) {
            tracked.insert(indicator, 0);
        }
    }
    let source_file_facts = is_source_file.then_some(facts).flatten();
    let tree = Tree::new(file, bind_data);
    walk(&tree, View::Node(root), |event| match event {
        WalkEvent::Record {
            view,
            parent,
            strip,
        } => {
            table.push(view.node());
            kinds.push(tree.kind_number(view));
            if let View::Node(id) = view {
                if let Some(slot) = tracked.get_mut(&id) {
                    *slot = table.len() as u32 - 1;
                }
            }
            let mut record = encoder.record(view, parent, source_file_facts);
            record[6] &= !strip;
            encoder.push_record(record);
        }
        WalkEvent::Next { previous, index } => {
            let at = previous as usize * NODE_SIZE + NODE_OFFSET_NEXT;
            encoder.nodes[at..at + 4].copy_from_slice(&index.to_le_bytes());
        }
    });

    let mut hash = 0u128;
    let mut parse_options = 0;
    if let Some(facts) = source_file_facts {
        if !facts.unhashed {
            hash = xxhash_rust::xxh3::xxh3_128(file.text().as_bytes());
        }
        let indicator = file.external_module_indicator_options;
        parse_options = u32::from(indicator.jsx) | u32::from(indicator.force) << 1;
        // The root's data is the offset of the source file's extended data.
        let root_data =
            &encoder.nodes[NODE_SIZE + NODE_OFFSET_DATA..NODE_SIZE + NODE_OFFSET_DATA + 4];
        let source_file_data = (u32::from_le_bytes(root_data.try_into().unwrap())
            & NODE_DATA_STRING_INDEX_MASK) as usize;
        let index_of = |id: &NodeId| tracked.get(id).copied().unwrap_or(0);
        let imports = encode_uint_array(
            facts.imports.iter().map(index_of),
            &mut encoder.structured_data,
        );
        let augmentations = encode_uint_array(
            facts.module_augmentations.iter().map(index_of),
            &mut encoder.structured_data,
        );
        let ambient = encode_string_array(
            facts.ambient_module_names.iter().map(JsString::as_bytes),
            &mut encoder.structured_data,
        );
        let indicator = match file.external_module_indicator {
            None => 0,
            Some(id) if id == root => 1,
            Some(id) => index_of(&id),
        };
        for (offset, value) in [
            (32, imports),
            (36, augmentations),
            (40, ambient),
            (44, indicator),
        ] {
            let at = source_file_data + offset;
            encoder.extended_data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    let offset_string_offsets = HEADER_SIZE;
    let offset_string_data = offset_string_offsets + encoder.strings.offsets_len() * 4;
    let offset_extended_data = offset_string_data + encoder.strings.string_length();
    let offset_structured_data = offset_extended_data + encoder.extended_data.len();
    let offset_nodes = offset_structured_data + encoder.structured_data.len();
    let header = [
        u32::from(PROTOCOL_VERSION) << 24,
        hash as u32,
        (hash >> 32) as u32,
        (hash >> 64) as u32,
        (hash >> 96) as u32,
        parse_options,
        offset_string_offsets as u32,
        offset_string_data as u32,
        offset_extended_data as u32,
        offset_structured_data as u32,
        offset_nodes as u32,
        0,
        0,
        0,
        0,
        0,
    ];
    let mut out = Vec::with_capacity(offset_nodes + encoder.nodes.len());
    for value in header {
        out.extend_from_slice(&value.to_le_bytes());
    }
    encoder.strings.encode_into(&mut out);
    out.extend_from_slice(&encoder.extended_data);
    out.extend_from_slice(&encoder.structured_data);
    out.extend_from_slice(&encoder.nodes);
    (
        out,
        NodeIndexTable {
            nodes: table,
            kinds,
            indices: OnceLock::new(),
        },
    )
}

impl Encoder<'_> {
    fn push_record(&mut self, fields: [u32; 7]) {
        for value in fields {
            self.nodes.extend_from_slice(&value.to_le_bytes());
        }
    }

    /// The record of `view`: kind, pos, end, next (patched later), parent,
    /// data and flags.
    fn record(&mut self, view: View, parent: u32, facts: Option<&SourceFileFacts<'_>>) -> [u32; 7] {
        let utf16 = |encoder: &Self, pos: u32| encoder.positions.utf16(pos);
        match view {
            View::Node(id) => {
                let node = self.tree.node(id);
                let kind = encoded_kind(self.tree.arena, node);
                let data = self.node_data(node, kind, facts);
                let mut flags = self.tree.node_flags(id, node);
                if id == self.file.root {
                    flags &= !TSGO_POSSIBLY_CONTAINS_DYNAMIC_IMPORT;
                    if self.tree.possibly_contains_dynamic_import() {
                        flags |= TSGO_POSSIBLY_CONTAINS_DYNAMIC_IMPORT;
                    }
                }
                [
                    kind,
                    utf16(self, node.pos),
                    utf16(self, node.end),
                    0,
                    parent,
                    data,
                    flags,
                ]
            }
            View::HeritageTypeReference(id) => {
                let node = self.tree.node(id);
                let NodeData::ExpressionWithTypeArguments(element) = &node.data else {
                    unreachable!("a heritage element")
                };
                let mask = 1 | u32::from(element.type_arguments.is_some()) << 1;
                let kind = tsgo_kind(SyntaxKind::TypeReference).unwrap();
                let flags = mapped_node_flags(node.flags);
                [
                    kind,
                    utf16(self, node.pos),
                    utf16(self, node.end),
                    0,
                    parent,
                    mask,
                    flags,
                ]
            }
            View::QualifiedName(id) => {
                let node = self.tree.node(id);
                let kind = tsgo_kind(SyntaxKind::QualifiedName).unwrap();
                let flags = mapped_node_flags(node.flags);
                [
                    kind,
                    utf16(self, node.pos),
                    utf16(self, node.end),
                    0,
                    parent,
                    0b11,
                    flags,
                ]
            }
            View::ImplicitExport(pos) => {
                let kind = tsgo_kind(SyntaxKind::ExportKeyword).unwrap();
                [
                    kind,
                    utf16(self, pos),
                    utf16(self, pos),
                    0,
                    parent,
                    0,
                    TSGO_REPARSED,
                ]
            }
            View::JSDoc(id, pos) => {
                let node = self.tree.node(id);
                let NodeData::JSDoc(js_doc) = &node.data else {
                    unreachable!("a JSDoc comment")
                };
                // The comment list is always there.
                let mask = 1 | u32::from(js_doc.tags.is_some()) << 1;
                let kind = tsgo_kind(SyntaxKind::JSDoc).unwrap();
                let flags = self.tree.node_flags(id, node);
                [
                    kind,
                    utf16(self, pos),
                    utf16(self, node.end),
                    0,
                    parent,
                    mask,
                    flags,
                ]
            }
            View::CommentText(owner) => {
                let Some(JSDocComment::Text {
                    text,
                    pos,
                    text_end,
                    ..
                }) = self.tree.comment(owner)
                else {
                    unreachable!("a text comment")
                };
                let kind = tsgo_kind(SyntaxKind::JSDocText).unwrap();
                let string =
                    self.strings
                        .add(text.as_bytes(), kind, *pos as usize, *text_end as usize);
                let flags = self.tree.comment_flags(owner);
                [
                    kind,
                    utf16(self, *pos),
                    utf16(self, *text_end),
                    0,
                    parent,
                    NODE_DATA_TYPE_STRING | string,
                    flags,
                ]
            }
            View::TrailingCommentText(owner) => {
                let (pos, end) = self
                    .tree
                    .trailing_comment_text(owner)
                    .expect("a trailing text");
                let kind = tsgo_kind(SyntaxKind::JSDocText).unwrap();
                let string = self.strings.add(b"", kind, pos as usize, end as usize);
                let flags = self.tree.comment_flags(owner);
                [
                    kind,
                    utf16(self, pos),
                    utf16(self, end),
                    0,
                    parent,
                    NODE_DATA_TYPE_STRING | string,
                    flags,
                ]
            }
            View::List(list, elements) => {
                let array = self.tree.arena.node_array(list);
                // tsgo NodeList.HasTrailingComma: the list ends after its last element.
                let (pos, end) = match elements {
                    Elements::TemplateTypeParameters => (0, 0),
                    _ => (array.pos, array.end),
                };
                let trailing = array
                    .nodes
                    .last()
                    .is_some_and(|&last| self.tree.node(last).end < end);
                [
                    SYNTAX_KIND_NODE_LIST,
                    utf16(self, pos),
                    utf16(self, end),
                    0,
                    parent,
                    array.nodes.len() as u32,
                    u32::from(trailing),
                ]
            }
            View::ImplicitModifiers(pos) => [
                SYNTAX_KIND_NODE_LIST,
                utf16(self, pos),
                utf16(self, pos),
                0,
                parent,
                1,
                0,
            ],
            View::Comment(owner) => {
                let (pos, end, count, last_end) = match self.tree.comment(owner) {
                    Some(JSDocComment::Text {
                        pos, end, text_end, ..
                    }) => (*pos, *end, 1, Some(*text_end)),
                    Some(JSDocComment::Nodes(list)) => {
                        let array = self.tree.arena.node_array(*list);
                        let trailing = self.tree.trailing_comment_text(owner);
                        let last_end = trailing
                            .map(|(_, end)| end)
                            .or_else(|| array.nodes.last().map(|&last| self.tree.node(last).end));
                        let count = array.nodes.len() as u32 + u32::from(trailing.is_some());
                        (array.pos, array.end, count, last_end)
                    }
                    // Only a JSDoc comment has a list without a comment.
                    None => (
                        self.tree.node(owner).pos,
                        self.tree.js_doc_comments_end(owner),
                        0,
                        None,
                    ),
                };
                let trailing = last_end.is_some_and(|last| last < end);
                [
                    SYNTAX_KIND_NODE_LIST,
                    utf16(self, pos),
                    utf16(self, end),
                    0,
                    parent,
                    count,
                    u32::from(trailing),
                ]
            }
        }
    }

    /// tsgo `getNodeData`.
    fn node_data(&mut self, node: &Node, kind: u32, facts: Option<&SourceFileFacts<'_>>) -> u32 {
        let data_type = node_data_type(kind);
        let common = common_data(node);
        match data_type {
            NODE_DATA_TYPE_STRING => data_type | common | self.node_string(node, kind),
            NODE_DATA_TYPE_EXTENDED_DATA => {
                data_type | common | self.extended_data(node, kind, facts)
            }
            _ => data_type | common | children_property_mask(&self.tree, node),
        }
    }

    /// tsgo `recordNodeStrings`.
    fn node_string(&mut self, node: &Node, kind: u32) -> u32 {
        let text: &[u8] = match &node.data {
            NodeData::Identifier(d) => d.escaped_text.unescape().as_bytes(),
            NodeData::PrivateIdentifier(d) => d.escaped_text.unescape().as_bytes(),
            NodeData::JsxText(d) => d.text.as_bytes(),
            NodeData::JSDocText(d) => d.text.as_bytes(),
            NodeData::JSDocLink(d) => d.text.as_bytes(),
            NodeData::JSDocLinkPlain(d) => d.text.as_bytes(),
            NodeData::JSDocLinkCode(d) => d.text.as_bytes(),
            _ => panic!("{:?} has no string data", node.kind),
        };
        self.strings
            .add(text, kind, node.pos as usize, node.end as usize)
    }

    /// tsgo `recordExtendedData`: the node's byte offset in the extended data.
    fn extended_data(
        &mut self,
        node: &Node,
        kind: u32,
        facts: Option<&SourceFileFacts<'_>>,
    ) -> u32 {
        let offset = self.extended_data.len() as u32;
        let (pos, end) = (node.pos as usize, node.end as usize);
        match &node.data {
            NodeData::StringLiteral(d) => {
                let flags = self.literal_flags(node, STRING_LITERAL_FLAGS);
                let text = self.strings.add(d.text.as_bytes(), kind, pos, end);
                self.push_extended(&[text, flags]);
            }
            NodeData::NumericLiteral(d) => {
                let text = self.strings.add(d.text.as_bytes(), kind, pos, end);
                self.push_extended(&[text, u32::from(node.numeric_literal_flags())]);
            }
            NodeData::BigIntLiteral(d) => {
                let flags = self.literal_flags(node, NUMERIC_LITERAL_FLAGS);
                let text = self.strings.add(d.text.as_bytes(), kind, pos, end);
                self.push_extended(&[text, flags]);
            }
            NodeData::RegularExpressionLiteral(d) => {
                let flags = u32::from(node.is_unterminated() == Some(true)) * UNTERMINATED;
                let text = self.strings.add(d.text.as_bytes(), kind, pos, end);
                self.push_extended(&[text, flags]);
            }
            NodeData::NoSubstitutionTemplateLiteral(d) => {
                let flags = self.literal_flags(node, TEMPLATE_LITERAL_LIKE_FLAGS);
                let text = self.strings.add(d.text.as_bytes(), kind, pos, end);
                self.push_extended(&[text, flags]);
            }
            NodeData::TemplateHead(d) => {
                self.template(node, kind, d.text.as_bytes(), d.raw_text.as_deref())
            }
            NodeData::TemplateMiddle(d) => {
                self.template(node, kind, d.text.as_bytes(), d.raw_text.as_deref())
            }
            NodeData::TemplateTail(d) => {
                self.template(node, kind, d.text.as_bytes(), d.raw_text.as_deref())
            }
            NodeData::SourceFile(_) => {
                let words = self.source_file_data(facts.copied().unwrap_or_default());
                self.push_extended(&words);
            }
            _ => panic!("{:?} has no extended data", node.kind),
        }
        offset
    }

    fn push_extended(&mut self, words: &[u32]) {
        for word in words {
            self.extended_data.extend_from_slice(&word.to_le_bytes());
        }
    }

    fn template(&mut self, node: &Node, kind: u32, text: &[u8], raw_text: Option<&str>) {
        let (pos, end) = (node.pos as usize, node.end as usize);
        let flags = self.literal_flags(node, TEMPLATE_LITERAL_LIKE_FLAGS);
        let text = self.strings.add(text, kind, pos, end);
        let raw_text = self
            .strings
            .add(raw_text.unwrap_or_default().as_bytes(), kind, pos, end);
        self.push_extended(&[text, raw_text, flags]);
    }

    /// The scanner's token flags of the literal `node` within `mask` (tsgo
    /// keeps them on its literal nodes), with tsgo's `SingleQuote`.
    fn literal_flags(&self, node: &Node, mask: u32) -> u32 {
        let text = self.file.text();
        let start = tsc_syntax::skip_trivia(text, node.pos as usize);
        if start >= text.len() || node.end as usize > text.len() {
            return 0;
        }
        let jsx_attribute_value = node
            .parent
            .is_some_and(|parent| self.file.arena.node(parent).kind == SyntaxKind::JsxAttribute);
        let mut flags =
            tsc_syntax::literal_token_flags(text, start, node.kind, jsx_attribute_value) & mask;
        if node.kind == SyntaxKind::StringLiteral && text.as_bytes()[start] == b'\'' {
            flags |= SINGLE_QUOTE;
        }
        // tsgo flags a `\u` escape before reading its digits, so an invalid
        // one is a UnicodeEscape too (tsc: only a valid one).
        if !jsx_attribute_value
            && mask & UNICODE_ESCAPE != 0
            && has_unicode_escape(&text[start..node.end as usize])
        {
            flags |= UNICODE_ESCAPE;
        }
        flags
    }

    /// tsgo `recordExtendedData_SourceFile`; the imports, module
    /// augmentations, ambient module names and external module indicator
    /// are written after the walk.
    fn source_file_data(&mut self, facts: SourceFileFacts<'_>) -> [u32; 19] {
        let file = self.file;
        let root = file.arena.node(file.root);
        let text = self.strings.add_file_text(root.pos, root.end);
        let file_name = file.file_name.as_bytes();
        let file_name_index = self.strings.add(file_name, 0, 0, 0);
        let path = facts.path.map_or(file_name, str::as_bytes);
        let path_index = self.strings.add(path, 0, 0, 0);
        let referenced_files = encode_file_references(
            file.referenced_files
                .iter()
                .map(|reference| (reference, 0, reference.preserve)),
            &mut self.structured_data,
        );
        let type_reference_directives =
            encode_type_reference_directives(file, &mut self.structured_data);
        let lib_reference_directives = encode_file_references(
            file.lib_reference_directives
                .iter()
                .map(|reference| (reference, 0, reference.preserve)),
            &mut self.structured_data,
        );
        let language_variant = u32::from(file.language_variant == tsc_syntax::LanguageVariant::Jsx);
        [
            text,
            file_name_index,
            path_index,
            language_variant,
            facts.script_kind as u32,
            referenced_files,
            type_reference_directives,
            lib_reference_directives,
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
            0,
            // originalText: the text (no content mapper).
            text,
            // spanMap, supplementalSourceFileNames, canonicalSourceFileName,
            // contentMapper, virtualFileName, diagnosticDirectives.
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
            NO_STRUCTURED_DATA,
        ]
    }
}

/// tsgo TokenFlags (tsc's bits 0-15 are unchanged).
const UNTERMINATED: u32 = 1 << 2;
const SINGLE_QUOTE: u32 = 1 << 16;
const UNICODE_ESCAPE: u32 = 1 << 10;

/// Whether a literal's source has a `\u` escape not of the `\u{…}` form.
fn has_unicode_escape(raw: &str) -> bool {
    let mut bytes = raw.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' && bytes.next() == Some(b'u') && bytes.clone().next() != Some(b'{') {
            return true;
        }
    }
    false
}

/// tsgo TokenFlagsStringLiteralFlags without SingleQuote (added from the quote).
const STRING_LITERAL_FLAGS: u32 = UNTERMINATED | (1 << 12) | (1 << 10) | (1 << 3) | (1 << 11);
/// tsgo TokenFlagsNumericLiteralFlags.
const NUMERIC_LITERAL_FLAGS: u32 =
    (1 << 4) | (1 << 5) | (1 << 13) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 14);
/// tsgo TokenFlagsTemplateLiteralLikeFlags.
const TEMPLATE_LITERAL_LIKE_FLAGS: u32 =
    UNTERMINATED | (1 << 12) | (1 << 10) | (1 << 3) | (1 << 11);

/// tsgo `encodeFileReferences`: `[pos, end, fileName, resolutionMode, preserve]`
/// tuples (tsc-rs keeps the references' positions in UTF-16).
fn encode_file_references<'a>(
    references: impl ExactSizeIterator<Item = (&'a tsc_syntax::FileReference, u32, bool)>,
    out: &mut Vec<u8>,
) -> u32 {
    if references.len() == 0 {
        return NO_STRUCTURED_DATA;
    }
    let offset = out.len() as u32;
    msgpack_array_header(out, references.len());
    for (reference, resolution_mode, preserve) in references {
        msgpack_array_header(out, 5);
        msgpack_uint(out, reference.pos);
        msgpack_uint(out, reference.end);
        msgpack_string(out, reference.file_name.as_bytes());
        msgpack_uint(out, resolution_mode);
        msgpack_bool(out, preserve);
    }
    offset
}

fn encode_type_reference_directives(file: &SourceFile, out: &mut Vec<u8>) -> u32 {
    let directives = &file.type_reference_directives;
    if directives.is_empty() {
        return NO_STRUCTURED_DATA;
    }
    let offset = out.len() as u32;
    msgpack_array_header(out, directives.len());
    for directive in directives {
        // tsgo core.ResolutionMode: CommonJS 1, ESNext 99.
        let resolution_mode = match directive.resolution_mode {
            None => 0,
            Some(tsc_syntax::TypeReferenceDirectiveResolutionMode::Require) => 1,
            Some(tsc_syntax::TypeReferenceDirectiveResolutionMode::Import) => 99,
        };
        msgpack_array_header(out, 5);
        msgpack_uint(out, directive.pos);
        msgpack_uint(out, directive.end);
        msgpack_string(out, directive.file_name.as_bytes());
        msgpack_uint(out, resolution_mode);
        msgpack_bool(out, directive.preserve);
    }
    offset
}

/// tsgo `encodeNodeIndexArray` / `encodeModuleAugmentations`.
fn encode_uint_array(values: impl ExactSizeIterator<Item = u32>, out: &mut Vec<u8>) -> u32 {
    if values.len() == 0 {
        return NO_STRUCTURED_DATA;
    }
    let offset = out.len() as u32;
    msgpack_array_header(out, values.len());
    for value in values {
        msgpack_uint(out, value);
    }
    offset
}

/// tsgo `encodeStringArray`.
fn encode_string_array<'a>(
    values: impl ExactSizeIterator<Item = &'a [u8]>,
    out: &mut Vec<u8>,
) -> u32 {
    if values.len() == 0 {
        return NO_STRUCTURED_DATA;
    }
    let offset = out.len() as u32;
    msgpack_array_header(out, values.len());
    for value in values {
        msgpack_string(out, value);
    }
    offset
}

fn msgpack_array_header(out: &mut Vec<u8>, length: usize) {
    if length <= 0x0f {
        out.push(0x90 | length as u8);
    } else if length <= 0xffff {
        out.push(0xdc);
        out.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        out.push(0xdd);
        out.extend_from_slice(&(length as u32).to_be_bytes());
    }
}

fn msgpack_uint(out: &mut Vec<u8>, value: u32) {
    if value <= 0x7f {
        out.push(value as u8);
    } else if value <= 0xff {
        out.extend_from_slice(&[0xcc, value as u8]);
    } else if value <= 0xffff {
        out.push(0xcd);
        out.extend_from_slice(&(value as u16).to_be_bytes());
    } else {
        out.push(0xce);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

fn msgpack_string(out: &mut Vec<u8>, value: &[u8]) {
    let length = value.len();
    if length <= 0x1f {
        out.push(0xa0 | length as u8);
    } else if length <= 0xff {
        out.extend_from_slice(&[0xd9, length as u8]);
    } else if length <= 0xffff {
        out.push(0xda);
        out.extend_from_slice(&(length as u16).to_be_bytes());
    } else {
        out.push(0xdb);
        out.extend_from_slice(&(length as u32).to_be_bytes());
    }
    out.extend_from_slice(value);
}

fn msgpack_bool(out: &mut Vec<u8>, value: bool) {
    out.push(if value { 0xc3 } else { 0xc2 });
}

/// tsgo `encoder_test.go` formatEncodedSourceFile, the format of the API
/// baselines: one line per node record after the nil node, indented by
/// depth, with an identifier's or a string node's text, the UTF-16 range,
/// the index and the low byte of the next sibling's index.
pub fn format_encoded_source_file(encoded: &[u8]) -> String {
    let read = |offset: usize| u32::from_le_bytes(encoded[offset..offset + 4].try_into().unwrap());
    let offset_nodes = read(HEADER_OFFSET_NODES) as usize;
    let offset_string_offsets = read(HEADER_OFFSET_STRING_OFFSETS) as usize;
    let offset_strings = read(HEADER_OFFSET_STRING_DATA) as usize;
    let depth = |mut parent: u32| {
        let mut depth = 0;
        while parent != 0 {
            depth += 1;
            parent = read(offset_nodes + parent as usize * NODE_SIZE + NODE_OFFSET_PARENT);
        }
        depth
    };
    let mut out = String::new();
    let mut index = 1;
    let mut record = offset_nodes + NODE_SIZE;
    while record < encoded.len() {
        let kind = read(record + NODE_OFFSET_KIND);
        let parent = read(record + NODE_OFFSET_PARENT);
        out.push_str(&"  ".repeat(depth(parent)));
        out.push_str(kind_name(kind));
        let data = read(record + NODE_OFFSET_DATA);
        if kind == KIND_IDENTIFIER || data & NODE_DATA_TYPE_MASK == NODE_DATA_TYPE_STRING {
            let string = (data & NODE_DATA_STRING_INDEX_MASK) as usize;
            let start = read(offset_string_offsets + string * 4) as usize;
            let end = read(offset_string_offsets + string * 4 + 4) as usize;
            let text = &encoded[offset_strings + start..offset_strings + end];
            out.push_str(&format!(" \"{}\"", String::from_utf8_lossy(text)));
        }
        out.push_str(&format!(
            " [{}, {}), i={index}, next={}\n",
            read(record + NODE_OFFSET_POS),
            read(record + NODE_OFFSET_END),
            encoded[record + NODE_OFFSET_NEXT]
        ));
        index += 1;
        record += NODE_SIZE;
    }
    out
}
