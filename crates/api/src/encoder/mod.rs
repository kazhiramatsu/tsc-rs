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

mod string_table;

use std::collections::HashMap;
use std::sync::OnceLock;

use tsc_binder::BindData;
use tsc_syntax::{JSDocComment, Node, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::JsString;

use string_table::StringTable;
pub(crate) use tsc_ls::tree::*;

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

pub use tsc_ls::tree::{
    kind_name, NODE_DATA_CHILD_MASK, NODE_DATA_STRING_INDEX_MASK, NODE_DATA_TYPE_CHILDREN,
    NODE_DATA_TYPE_EXTENDED_DATA, NODE_DATA_TYPE_MASK, NODE_DATA_TYPE_STRING,
    SYNTAX_KIND_NODE_LIST,
};

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

/// tsgo `SetSourceFileID`: the source file's node id, which the client checks
/// remote references against.
pub fn set_source_file_id(data: &mut [u8], id: u64) {
    data[HEADER_OFFSET_SOURCE_FILE_ID..HEADER_OFFSET_SOURCE_FILE_ID + 8]
        .copy_from_slice(&id.to_le_bytes());
}

/// tsgo `SetSourceFileLease`.
pub fn set_source_file_lease(data: &mut [u8], lease: u64) {
    data[HEADER_OFFSET_SOURCE_FILE_LEASE..HEADER_OFFSET_SOURCE_FILE_LEASE + 8]
        .copy_from_slice(&lease.to_le_bytes());
}

/// An absent structured-data entry or string index.
pub const NO_STRUCTURED_DATA: u32 = u32::MAX;

pub use tsc_syntax::ScriptKind;

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
    /// What a content mapper's output carries (tsgo
    /// `ContentMapperSourceFileInfo`), its positions in UTF-16 units.
    pub content_mapped: Option<&'a tsc_program::content_mapper::source::ContentMappedSource>,
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
    encode_tree(file, file.root, Some(facts), None)
}

/// tsgo `EncodeNode(node, nil)`, as its API encodes a node builder's
/// result: `node` (of `file`) and its descendants without their JSDoc, with
/// no file text in the string data, the positions as they are (UTF-8) and
/// no hash or parse options in the header. `single_quote` tells whether a
/// synthesized string literal is single quoted.
pub fn encode_node(
    file: &SourceFile,
    node: NodeId,
    single_quote: &dyn Fn(NodeId) -> bool,
) -> (Vec<u8>, NodeIndexTable) {
    encode_tree(file, node, None, Some(single_quote))
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
    /// The node being recorded.
    current: Option<NodeId>,
    /// Whether a synthesized string literal is single quoted.
    single_quote: Option<&'a dyn Fn(NodeId) -> bool>,
}

fn encode_tree<'a>(
    file: &'a SourceFile,
    root: NodeId,
    facts: Option<&SourceFileFacts<'_>>,
    single_quote: Option<&'a dyn Fn(NodeId) -> bool>,
) -> (Vec<u8>, NodeIndexTable) {
    let is_source_file = file.arena.node(root).kind == SyntaxKind::SourceFile && facts.is_some();
    let bind_data = facts.and_then(|facts| facts.bind_data);
    let tree = || match facts {
        Some(_) => Tree::new(file, bind_data),
        None => Tree::detached(file),
    };
    let mut encoder = Encoder {
        file,
        tree: tree(),
        positions: PositionMap::new(if facts.is_some() { file.text() } else { "" }),
        strings: StringTable::new(if is_source_file {
            file.text().as_bytes()
        } else {
            &[]
        }),
        nodes: Vec::with_capacity((file.node_count() + 2) * NODE_SIZE),
        extended_data: Vec::new(),
        structured_data: Vec::new(),
        current: None,
        single_quote,
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
    let tree = tree();
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
            hash = match facts.content_mapped {
                Some(info) => content_mapped_hash(info),
                None => xxhash_rust::xxh3::xxh3_128(file.text().as_bytes()),
            };
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
                self.current = Some(id);
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
                // Positions are signed there: a synthesized list (-1) has one
                // only as the deep clone of a list that had one (whose last
                // element tsgo moves to -2).
                let trailing = array.nodes.last().is_some_and(|&last| {
                    if end == u32::MAX {
                        array.has_trailing_comma
                    } else {
                        (self.tree.node(last).end as i32) < end as i32
                    }
                });
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
        if node.pos as usize > text.len() {
            // A synthesized literal: the flags its factory gave it.
            let single_quote = node.kind == SyntaxKind::StringLiteral
                && self
                    .single_quote
                    .zip(self.current)
                    .is_some_and(|(single_quote, id)| single_quote(id));
            return if single_quote { SINGLE_QUOTE } else { 0 };
        }
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
        // tsgo: a content mapper's original text, when it is not the text.
        let original_text = match facts.content_mapped {
            Some(info) if info.original_text.as_bytes() != file.text().as_bytes() => {
                self.strings.add(info.original_text.as_bytes(), 0, 0, 0)
            }
            _ => text,
        };
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
        let mapped = facts.content_mapped;
        let span_map = mapped
            .and_then(|info| info.span_map.as_ref())
            .map_or(NO_STRUCTURED_DATA, |span_map| {
                encode_span_map(span_map, &mut self.structured_data)
            });
        let supplemental_file_names = encode_string_array(
            mapped
                .map_or(&[][..], |info| &info.supplemental[..])
                .iter()
                .map(|name| name.as_bytes()),
            &mut self.structured_data,
        );
        let canonical_file_name = mapped
            .and_then(|info| info.canonical.as_ref())
            .map_or(NO_STRUCTURED_DATA, |name| {
                self.strings.add(name.as_bytes(), 0, 0, 0)
            });
        let content_mapper = mapped
            .filter(|info| !info.content_mapper.is_empty())
            .map_or(NO_STRUCTURED_DATA, |info| {
                self.strings.add(info.content_mapper.as_bytes(), 0, 0, 0)
            });
        let virtual_file_name = mapped
            .filter(|info| !info.virtual_file_name.is_empty())
            .map_or(NO_STRUCTURED_DATA, |info| {
                self.strings.add(info.virtual_file_name.as_bytes(), 0, 0, 0)
            });
        let diagnostic_directives = encode_diagnostic_directives(
            mapped.map_or(&[][..], |info| &info.diagnostic_directives[..]),
            &mut self.structured_data,
        );
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
            original_text,
            span_map,
            supplemental_file_names,
            canonical_file_name,
            content_mapper,
            virtual_file_name,
            diagnostic_directives,
        ]
    }
}

/// tsgo TokenFlags (tsc's bits 0-15 are unchanged).
pub(crate) const UNTERMINATED: u32 = 1 << 2;
pub(crate) const SINGLE_QUOTE: u32 = 1 << 16;
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
pub(crate) const NUMERIC_LITERAL_FLAGS: u32 =
    (1 << 4) | (1 << 5) | (1 << 13) | (1 << 6) | (1 << 7) | (1 << 8) | (1 << 9) | (1 << 14);
/// tsgo TokenFlagsTemplateLiteralLikeFlags.
pub(crate) const TEMPLATE_LITERAL_LIKE_FLAGS: u32 =
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

/// tsgo's hash of a content mapper's output (the content-mapped parse
/// cache's key, `contentMappedParseCacheKey`): of the original text's hash
/// and the transform identity's, each high half first, with the API's
/// diagnostic locale (the default, written as nothing). A failure stub has
/// none (tsgo parses it outside the cache).
fn content_mapped_hash(info: &tsc_program::content_mapper::source::ContentMappedSource) -> u128 {
    if info.is_failure_stub() {
        return 0;
    }
    let raw = xxhash_rust::xxh3::xxh3_128(info.original_text.as_bytes());
    let identity = xxhash_rust::xxh3::xxh3_128(info.transform_identity.as_bytes());
    let mut buffer = [0; 32];
    for (index, half) in [raw >> 64, raw, identity >> 64, identity]
        .into_iter()
        .enumerate()
    {
        buffer[index * 8..index * 8 + 8].copy_from_slice(&(half as u64).to_le_bytes());
    }
    xxhash_rust::xxh3::xxh3_128(&buffer)
}

/// tsgo `encodeSpanMap`: `[virtualStart, virtualLength, originalStart,
/// originalLength, kind]` tuples, with the features when they are not all
/// of them (tsc-rs keeps the positions in UTF-16).
fn encode_span_map(span_map: &tsc_diagnostics::spanmap::SpanMap, out: &mut Vec<u8>) -> u32 {
    let segments = span_map.segments();
    let offset = out.len() as u32;
    msgpack_array_header(out, segments.len());
    for segment in segments {
        let all = segment.features == tsc_diagnostics::spanmap::FEATURES_ALL;
        msgpack_array_header(out, if all { 5 } else { 6 });
        msgpack_uint(out, segment.virtual_start as u32);
        msgpack_uint(out, (segment.virtual_end - segment.virtual_start) as u32);
        msgpack_uint(out, segment.original_start as u32);
        msgpack_uint(out, (segment.original_end - segment.original_start) as u32);
        msgpack_uint(out, segment.kind as u32);
        if !all {
            msgpack_uint(out, segment.features as u32);
        }
    }
    offset
}

/// tsgo `encodeDiagnosticDirectives`: `[originalStart, originalLength,
/// virtualStart, virtualLength, policy, unusedCode]` tuples.
fn encode_diagnostic_directives(
    directives: &[tsc_diagnostics::spanmap::MappedDiagnosticDirective],
    out: &mut Vec<u8>,
) -> u32 {
    if directives.is_empty() {
        return NO_STRUCTURED_DATA;
    }
    let offset = out.len() as u32;
    msgpack_array_header(out, directives.len());
    for directive in directives {
        let (original_start, original_end) = directive.original_range;
        let (virtual_start, virtual_end) = directive.virtual_range;
        msgpack_array_header(out, 6);
        msgpack_uint(out, original_start as u32);
        msgpack_uint(out, (original_end - original_start) as u32);
        msgpack_uint(out, virtual_start as u32);
        msgpack_uint(out, (virtual_end - virtual_start) as u32);
        msgpack_uint(
            out,
            match directive.policy {
                tsc_diagnostics::spanmap::DirectivePolicy::Ignore => 0,
                tsc_diagnostics::spanmap::DirectivePolicy::Expect => 1,
            },
        );
        msgpack_uint(out, directive.unused_code as u32);
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
