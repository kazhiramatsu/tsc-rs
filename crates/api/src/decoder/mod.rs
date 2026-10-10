//! tsgo's AST decoder (`internal/api/encoder/decoder.go`, protocol 9): the
//! binary nodes `printNode` receives, back into a syntax tree.
//!
//! tsgo builds its nodes bottom-up with its factory: positions as they are
//! (the client's UTF-16 offsets, used as tsgo uses them), its node flags,
//! and no parents. tsc-rs builds the same nodes in tsc's shapes, reading the
//! encoder's views the other way: a type heritage element's TypeReference
//! is an ExpressionWithTypeArguments, the implicit `export` of a nested
//! namespace is the NestedNamespace flag, a childless BindingElement is an
//! array binding hole. The facts tsgo's printer reads from a literal go
//! where tsc-rs's printer reads them. A node of no source file keeps its
//! positions, but they index no text (tsgo's printer has no source file
//! for it).

mod generated;

pub(crate) use generated::port_kind;

use tsc_syntax::nodes::{
    BigIntLiteralData, ExpressionWithTypeArgumentsData, IdentifierData, JSDocComment,
    JSDocLinkCodeData, JSDocLinkData, JSDocLinkPlainData, JSDocTextData, JsxTextData,
    NoSubstitutionTemplateLiteralData, NumericLiteralData, PrivateIdentifierData,
    PropertyAccessExpressionData, RegularExpressionLiteralData, SourceFileData, StringLiteralData,
    TemplateHeadData, TemplateMiddleData, TemplateTailData,
};
use tsc_syntax::{
    ExternalModuleIndicatorOptions, LanguageVariant, NodeArena, NodeArrayId, NodeData, NodeId,
    SourceFile, SyntaxKind,
};
use tsc_types::{EscapedName, JsString, NodeFlags, TokenFlags};

use crate::encoder::{
    kind_name, ScriptKind, HEADER_OFFSET_EXTENDED_DATA, HEADER_OFFSET_METADATA,
    HEADER_OFFSET_NODES, HEADER_OFFSET_PARSE_OPTIONS, HEADER_OFFSET_STRING_DATA,
    HEADER_OFFSET_STRING_OFFSETS, HEADER_SIZE, NODE_DATA_CHILD_MASK, NODE_DATA_STRING_INDEX_MASK,
    NODE_DATA_TYPE_EXTENDED_DATA, NODE_DATA_TYPE_MASK, NODE_DATA_TYPE_STRING, NODE_FLAGS,
    NODE_OFFSET_DATA, NODE_OFFSET_END, NODE_OFFSET_FLAGS, NODE_OFFSET_KIND, NODE_OFFSET_NEXT,
    NODE_OFFSET_PARENT, NODE_OFFSET_POS, NODE_SIZE, PROTOCOL_VERSION, SINGLE_QUOTE,
    SYNTAX_KIND_NODE_LIST, TSGO_NESTED_NAMESPACE, UNTERMINATED,
};

/// A decoded tree, in tsc-rs's nodes.
pub struct DecodedTree {
    /// The decoded source file, or, for a node of none, an empty file
    /// holding the node.
    pub source_file: SourceFile,
    pub root: NodeId,
    /// Whether the root is the source file.
    pub is_source_file: bool,
    /// The decoded source file's script kind (`Unknown` for a node of
    /// none).
    pub script_kind: ScriptKind,
    /// The string literals tsgo flags `SingleQuote` (tsc-rs's printer reads
    /// a literal's quote from its emit properties).
    pub single_quoted: Vec<NodeId>,
}

/// tsgo `DecodeNodes`.
pub fn decode_nodes(data: &[u8]) -> Result<DecodedTree, String> {
    Decoder::new(data)?.decode()
}

/// tsgo `astDecoder`.
struct Decoder<'a> {
    raw: &'a [u8],
    string_offsets: usize,
    string_data: usize,
    extended_data: usize,
    nodes_offset: usize,
    node_count: usize,
    /// Whether the nodes keep their positions: only a source file's do.
    positioned: bool,
    /// The source file's text, whose byte offsets the positions are taken
    /// as.
    text: &'a [u8],
    arena: NodeArena,
    nodes: Vec<Option<NodeId>>,
    lists: Vec<Option<NodeArrayId>>,
    /// The encoded end of each node, for its list's trailing comma.
    ends: Vec<u32>,
    single_quoted: Vec<NodeId>,
    source_file: Option<SourceFileFacts>,
}

/// What tsgo's decoder gives the source file node it creates.
struct SourceFileFacts {
    file_name: String,
    text: String,
    language_variant: LanguageVariant,
    script_kind: ScriptKind,
    external_module_indicator_options: ExternalModuleIndicatorOptions,
}

impl<'a> Decoder<'a> {
    /// tsgo `newASTDecoder`.
    fn new(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < HEADER_SIZE {
            return Err(format!("data too short for header: {} bytes", data.len()));
        }
        let version = data[HEADER_OFFSET_METADATA + 3];
        if version != PROTOCOL_VERSION {
            return Err(format!(
                "unsupported protocol version {version} (expected {PROTOCOL_VERSION})"
            ));
        }
        let string_offsets = read_le32(data, HEADER_OFFSET_STRING_OFFSETS);
        let string_data = read_le32(data, HEADER_OFFSET_STRING_DATA);
        let extended_data = read_le32(data, HEADER_OFFSET_EXTENDED_DATA);
        let nodes_offset = read_le32(data, HEADER_OFFSET_NODES);
        let length = u32::try_from(data.len()).unwrap_or(u32::MAX);
        if string_offsets > length
            || string_data > length
            || extended_data > length
            || nodes_offset > length
        {
            return Err(format!(
                "invalid AST header offsets: offsets exceed data length ({length})"
            ));
        }
        if !(string_offsets <= string_data
            && string_data <= extended_data
            && extended_data <= nodes_offset)
        {
            return Err(format!(
                "invalid AST header offsets: expected strTable <= strData <= extData <= nodeOff (got {string_offsets}, {string_data}, {extended_data}, {nodes_offset})"
            ));
        }
        let node_count = (data.len() - nodes_offset as usize) / NODE_SIZE;
        Ok(Self {
            raw: data,
            string_offsets: string_offsets as usize,
            string_data: string_data as usize,
            extended_data: extended_data as usize,
            nodes_offset: nodes_offset as usize,
            node_count,
            positioned: false,
            text: &[],
            arena: NodeArena::new(),
            nodes: vec![None; node_count],
            lists: vec![None; node_count],
            ends: vec![0; node_count],
            single_quoted: Vec::new(),
            source_file: None,
        })
    }

    /// A field of node record `index`.
    fn field(&self, index: usize, offset: usize) -> u32 {
        read_le32(self.raw, self.nodes_offset + index * NODE_SIZE + offset)
    }

    /// tsgo `getString`: string `index`'s bytes.
    fn string(&self, index: u32) -> Result<&'a [u8], String> {
        let base = self.string_offsets + index as usize * 4;
        let start = read_le32(self.raw, base) as usize;
        let end = read_le32(self.raw, base + 4) as usize;
        self.raw
            .get(self.string_data..)
            .and_then(|strings| strings.get(start..end))
            .ok_or_else(|| format!("string {index} is outside the string data"))
    }

    /// The `index`th word of a node's extended data.
    fn extended(&self, data: u32, index: usize) -> u32 {
        read_le32(
            self.raw,
            self.extended_data + (data & NODE_DATA_STRING_INDEX_MASK) as usize + index * 4,
        )
    }

    /// tsgo `collectChildren`: the records of node `index`'s children.
    fn children(&self, index: usize) -> Vec<usize> {
        let mut children = Vec::new();
        let first = index + 1;
        if first >= self.node_count || self.field(first, NODE_OFFSET_PARENT) as usize != index {
            return children;
        }
        children.push(first);
        let mut next = self.field(first, NODE_OFFSET_NEXT) as usize;
        while next != 0 && next < self.node_count {
            children.push(next);
            next = self.field(next, NODE_OFFSET_NEXT) as usize;
        }
        children
    }

    /// tsgo `decode`: the nodes from the last record to the first, so that
    /// children exist before their parents.
    fn decode(mut self) -> Result<DecodedTree, String> {
        if self.node_count < 2 {
            return Err("no nodes to decode".to_owned());
        }
        self.positioned = port_kind(self.field(1, NODE_OFFSET_KIND))
            == Some(SyntaxKind::SourceFile)
            && self.field(1, NODE_OFFSET_DATA) & NODE_DATA_TYPE_MASK
                == NODE_DATA_TYPE_EXTENDED_DATA;
        if self.positioned {
            let data = self.field(1, NODE_OFFSET_DATA);
            self.text = self.string(self.extended(data, 0))?;
        }
        for index in (1..self.node_count).rev() {
            let kind = self.field(index, NODE_OFFSET_KIND);
            let pos = self.field(index, NODE_OFFSET_POS);
            let end = self.field(index, NODE_OFFSET_END);
            self.ends[index] = end;
            let children = self.children(index);
            if kind == SYNTAX_KIND_NODE_LIST {
                let elements = children
                    .iter()
                    .filter_map(|&child| self.nodes[child].map(|node| (node, self.ends[child])))
                    .collect::<Vec<_>>();
                // tsgo `NodeList.HasTrailingComma`.
                let trailing_comma = elements.last().is_some_and(|&(_, last)| last < end);
                let elements = elements.iter().map(|&(node, _)| node).collect::<Vec<_>>();
                let (pos, end) = self.range(pos, end);
                self.lists[index] =
                    Some(self.arena.alloc_array(&elements, pos, end, trailing_comma));
                continue;
            }
            let node = self
                .node(index, kind, pos, end, &children)
                .map_err(|error| format!("at node {index} (kind {}): {error}", kind_name(kind)))?;
            self.nodes[index] = Some(node);
        }
        let root = self.nodes[1].ok_or_else(|| "no nodes to decode".to_owned())?;
        let facts = self.source_file.take();
        let is_source_file =
            facts.is_some() && self.arena.node(root).kind == SyntaxKind::SourceFile;
        let script_kind = facts
            .as_ref()
            .filter(|_| is_source_file)
            .map_or(ScriptKind::Unknown, |facts| facts.script_kind);
        let source_file = match facts.filter(|_| is_source_file) {
            Some(facts) => {
                let is_declaration_file =
                    NodeFlags::from_bits(self.arena.node(root).flags).contains(NodeFlags::AMBIENT);
                SourceFile::from_tree(
                    facts.file_name,
                    Some(facts.text),
                    self.arena,
                    root,
                    facts.language_variant,
                    is_declaration_file,
                    facts.external_module_indicator_options,
                )
            }
            None => SourceFile::from_tree(
                "",
                None,
                self.arena,
                root,
                LanguageVariant::Standard,
                false,
                ExternalModuleIndicatorOptions::default(),
            ),
        };
        Ok(DecodedTree {
            source_file,
            root,
            is_source_file,
            script_kind,
            single_quoted: self.single_quoted,
        })
    }

    /// A node's range. tsgo takes the client's (UTF-16) positions as byte
    /// offsets of the file's text: one inside a character is that
    /// character's, and one past the text (a client drops a byte order mark
    /// from the text but not from the positions) is the text's end. A node
    /// of no source file keeps its positions, which index no text.
    fn range(&self, pos: u32, end: u32) -> (usize, usize) {
        if self.positioned {
            (self.text_offset(pos), self.text_offset(end))
        } else {
            (pos as usize, end as usize)
        }
    }

    fn text_offset(&self, position: u32) -> usize {
        if position == u32::MAX {
            return u32::MAX as usize;
        }
        let mut offset = (position as usize).min(self.text.len());
        while offset > 0 && offset < self.text.len() && self.text[offset] & 0xC0 == 0x80 {
            offset -= 1;
        }
        offset
    }

    /// tsgo `createNode`.
    fn node(
        &mut self,
        index: usize,
        kind: u32,
        pos: u32,
        end: u32,
        children: &[usize],
    ) -> Result<NodeId, String> {
        let syntax_kind =
            port_kind(kind).ok_or_else(|| format!("tsc-rs has no {} node", kind_name(kind)))?;
        let data = self.field(index, NODE_OFFSET_DATA);
        let common = (data >> 24) & 0x3f;
        let flags = port_node_flags(syntax_kind, self.field(index, NODE_OFFSET_FLAGS));
        let (pos, end) = self.range(pos, end);
        match data & NODE_DATA_TYPE_MASK {
            NODE_DATA_TYPE_STRING => {
                let node_data = self.string_node(syntax_kind, kind, data, common)?;
                Ok(self.arena.alloc_node(node_data, pos, end, flags))
            }
            NODE_DATA_TYPE_EXTENDED_DATA => {
                self.extended_node(syntax_kind, kind, data, children, pos, end, flags)
            }
            _ => Ok(self.children_node(syntax_kind, data, common, children, pos, end, flags)),
        }
    }

    /// tsgo `createStringNode`.
    fn string_node(
        &self,
        syntax_kind: SyntaxKind,
        kind: u32,
        data: u32,
        common: u32,
    ) -> Result<NodeData, String> {
        let text = self.string(data & NODE_DATA_STRING_INDEX_MASK)?;
        Ok(match syntax_kind {
            SyntaxKind::Identifier => NodeData::Identifier(IdentifierData {
                escaped_text: EscapedName::escape(js_string(text).as_js()),
            }),
            SyntaxKind::PrivateIdentifier => NodeData::PrivateIdentifier(PrivateIdentifierData {
                escaped_text: EscapedName::escape(js_string(text).as_js()),
            }),
            SyntaxKind::JsxText => NodeData::JsxText(Box::new(JsxTextData {
                text: string(text),
                contains_only_trivia_white_spaces: common & 1 != 0,
            })),
            SyntaxKind::JSDocText => NodeData::JSDocText(JSDocTextData { text: string(text) }),
            SyntaxKind::JSDocLink => NodeData::JSDocLink(Box::new(JSDocLinkData {
                name: None,
                text: string(text),
            })),
            SyntaxKind::JSDocLinkPlain => NodeData::JSDocLinkPlain(Box::new(JSDocLinkPlainData {
                name: None,
                text: string(text),
            })),
            SyntaxKind::JSDocLinkCode => NodeData::JSDocLinkCode(Box::new(JSDocLinkCodeData {
                name: None,
                text: string(text),
            })),
            _ => return Err(format!("unknown string node kind {}", kind_name(kind))),
        })
    }

    /// tsgo `createExtendedNode`.
    #[allow(clippy::too_many_arguments)]
    fn extended_node(
        &mut self,
        syntax_kind: SyntaxKind,
        kind: u32,
        data: u32,
        children: &[usize],
        pos: usize,
        end: usize,
        flags: NodeFlags,
    ) -> Result<NodeId, String> {
        let text = self.string(self.extended(data, 0))?;
        match syntax_kind {
            SyntaxKind::StringLiteral => {
                let token_flags = self.extended(data, 1);
                let node_data = NodeData::StringLiteral(StringLiteralData {
                    text: js_string(text),
                });
                let node = self.arena.alloc_node(node_data, pos, end, flags);
                self.arena
                    .node_mut(node)
                    .set_has_extended_unicode_escape(Some(
                        token_flags & TokenFlags::EXTENDED_UNICODE_ESCAPE.bits() as u32 != 0,
                    ));
                if token_flags & SINGLE_QUOTE != 0 {
                    self.single_quoted.push(node);
                }
                Ok(node)
            }
            SyntaxKind::NumericLiteral => {
                let token_flags = self.extended(data, 1);
                let node_data = NodeData::NumericLiteral(NumericLiteralData { text: string(text) });
                let node = self.arena.alloc_node(node_data, pos, end, flags);
                self.arena.node_mut(node).set_numeric_literal_flags(
                    (token_flags & TokenFlags::NUMERIC_LITERAL_FLAGS.bits() as u32) as u16,
                );
                Ok(node)
            }
            SyntaxKind::BigIntLiteral => {
                let node_data = NodeData::BigIntLiteral(BigIntLiteralData { text: string(text) });
                Ok(self.arena.alloc_node(node_data, pos, end, flags))
            }
            SyntaxKind::RegularExpressionLiteral => {
                let token_flags = self.extended(data, 1);
                let node_data = NodeData::RegularExpressionLiteral(RegularExpressionLiteralData {
                    text: string(text),
                });
                let node = self.arena.alloc_node(node_data, pos, end, flags);
                self.arena
                    .node_mut(node)
                    .set_is_unterminated(Some(token_flags & UNTERMINATED != 0));
                Ok(node)
            }
            SyntaxKind::NoSubstitutionTemplateLiteral => {
                let token_flags = self.extended(data, 1);
                // tsgo's factory gives the literal no raw text: its printer
                // writes the raw text only for an empty literal.
                let text = js_string(text);
                let raw_text = text.is_empty().then(String::new);
                let node_data = NodeData::NoSubstitutionTemplateLiteral(Box::new(
                    NoSubstitutionTemplateLiteralData { text, raw_text },
                ));
                let node = self.arena.alloc_node(node_data, pos, end, flags);
                self.arena
                    .node_mut(node)
                    .set_template_flags(template_flags(token_flags));
                Ok(node)
            }
            SyntaxKind::TemplateHead | SyntaxKind::TemplateMiddle | SyntaxKind::TemplateTail => {
                let raw = self.string(self.extended(data, 1))?;
                let token_flags = self.extended(data, 2);
                let text = js_string(text);
                // tsgo getLiteralText writes the raw text when there is
                // one or the text is empty, and the escaped text otherwise.
                let raw_text = (!raw.is_empty() || text.is_empty()).then(|| string(raw));
                let node_data = match syntax_kind {
                    SyntaxKind::TemplateHead => {
                        NodeData::TemplateHead(Box::new(TemplateHeadData { text, raw_text }))
                    }
                    SyntaxKind::TemplateMiddle => {
                        NodeData::TemplateMiddle(Box::new(TemplateMiddleData { text, raw_text }))
                    }
                    _ => NodeData::TemplateTail(Box::new(TemplateTailData { text, raw_text })),
                };
                let node = self.arena.alloc_node(node_data, pos, end, flags);
                self.arena
                    .node_mut(node)
                    .set_template_flags(template_flags(token_flags));
                Ok(node)
            }
            SyntaxKind::SourceFile => self.source_file_node(data, text, children, pos, end, flags),
            _ => Err(format!(
                "unknown extended data node kind {}",
                kind_name(kind)
            )),
        }
    }

    /// tsgo `decodeExtendedData_SourceFile`.
    fn source_file_node(
        &mut self,
        data: u32,
        text: &[u8],
        children: &[usize],
        pos: usize,
        end: usize,
        flags: NodeFlags,
    ) -> Result<NodeId, String> {
        let file_name = string(self.string(self.extended(data, 1))?);
        let language_variant = if self.extended(data, 3) == 1 {
            LanguageVariant::Jsx
        } else {
            LanguageVariant::Standard
        };
        let rooted = tsc_program::path_root_parts(tsc_types::JsStr::from_str(&file_name)).is_some();
        if !rooted
            || tsc_program::normalize_path(tsc_types::JsStr::from_str(&file_name)).as_str()
                != Some(file_name.as_str())
        {
            return Err(format!("invalid source file name {file_name:?}"));
        }
        let parse_options = read_le32(self.raw, HEADER_OFFSET_PARSE_OPTIONS);
        // The statements are the NodeList child, then the EndOfFile token.
        let mut statements = None;
        let mut end_of_file_token = None;
        for &child in children {
            if self.field(child, NODE_OFFSET_KIND) == SYNTAX_KIND_NODE_LIST {
                statements = self.lists[child];
            } else if let Some(node) = self.nodes[child] {
                if self.arena.node(node).kind == SyntaxKind::EndOfFileToken {
                    end_of_file_token = Some(node);
                }
            }
        }
        let end_of_file_token = end_of_file_token.unwrap_or_else(|| {
            self.arena.alloc_token(
                SyntaxKind::EndOfFileToken,
                u32::MAX as usize,
                u32::MAX as usize,
                NodeFlags::NONE,
            )
        });
        let node_data = NodeData::SourceFile(SourceFileData {
            statements,
            end_of_file_token: Some(end_of_file_token),
        });
        self.source_file = Some(SourceFileFacts {
            file_name,
            text: string(text),
            language_variant,
            script_kind: ScriptKind::from_number(self.extended(data, 4)).unwrap_or_default(),
            external_module_indicator_options: ExternalModuleIndicatorOptions {
                jsx: parse_options & 1 != 0,
                force: parse_options & 2 != 0,
            },
        });
        Ok(self.arena.alloc_node(node_data, pos, end, flags))
    }

    /// tsgo `createChildrenNode`, in tsc's shapes.
    #[allow(clippy::too_many_arguments)]
    fn children_node(
        &mut self,
        syntax_kind: SyntaxKind,
        data: u32,
        common: u32,
        children: &[usize],
        pos: usize,
        end: usize,
        mut flags: NodeFlags,
    ) -> NodeId {
        let mut node_data = NodeData::missing(syntax_kind);
        if matches!(node_data, NodeData::Token) {
            // A token, a keyword expression or a keyword type.
            return self.arena.alloc_token(syntax_kind, pos, end, flags);
        }
        let mut cursor = Children {
            mask: data & NODE_DATA_CHILD_MASK,
            children,
            next: 0,
            nodes: &self.nodes,
            lists: &self.lists,
            arena: &mut self.arena,
        };
        generated::decode_properties(&mut node_data, &mut cursor);
        // tsgo writes an array binding hole as a BindingElement without
        // children (tsc: an OmittedExpression).
        if matches!(&node_data, NodeData::BindingElement(element) if element.name.is_none()) {
            node_data = NodeData::missing(SyntaxKind::OmittedExpression);
        }
        let mut multi_line = None;
        match &mut node_data {
            NodeData::Block(_)
            | NodeData::ArrayLiteralExpression(_)
            | NodeData::ObjectLiteralExpression(_) => multi_line = Some(common & 1 != 0),
            NodeData::HeritageClause(clause) => {
                clause.token = if common & 1 != 0 {
                    SyntaxKind::ImplementsKeyword
                } else {
                    SyntaxKind::ExtendsKeyword
                };
                if let Some(types) = clause.types {
                    let elements = self.arena.node_array(types).nodes.to_vec();
                    for element in elements {
                        self.heritage_type(element);
                    }
                }
            }
            NodeData::ExportAssignment(assignment) => {
                assignment.is_export_equals = Some(common & 1 != 0);
            }
            NodeData::ExportSpecifier(specifier) => specifier.is_type_only = common & 1 != 0,
            NodeData::PrefixUnaryExpression(expression) => {
                expression.operator = match common {
                    1 => SyntaxKind::MinusToken,
                    2 => SyntaxKind::TildeToken,
                    3 => SyntaxKind::ExclamationToken,
                    4 => SyntaxKind::PlusPlusToken,
                    5 => SyntaxKind::MinusMinusToken,
                    _ => SyntaxKind::PlusToken,
                };
            }
            NodeData::PostfixUnaryExpression(expression) => {
                expression.operator = if common & 1 != 0 {
                    SyntaxKind::MinusMinusToken
                } else {
                    SyntaxKind::PlusPlusToken
                };
            }
            NodeData::MetaProperty(property) => {
                property.keyword_token = if common & 1 != 0 {
                    SyntaxKind::NewKeyword
                } else {
                    SyntaxKind::ImportKeyword
                };
            }
            NodeData::TypeOperator(operator) => {
                operator.operator = match common {
                    1 => SyntaxKind::ReadonlyKeyword,
                    2 => SyntaxKind::UniqueKeyword,
                    _ => SyntaxKind::KeyOfKeyword,
                };
            }
            NodeData::ImportAttributes(attributes) => {
                attributes.multi_line = Some(common & 1 != 0);
                attributes.token = if common & 2 != 0 {
                    SyntaxKind::AssertKeyword
                } else {
                    SyntaxKind::WithKeyword
                };
            }
            // tsgo's keyword is `namespace` or `module`: `declare global`
            // decodes as a `module` declaration named `global`.
            NodeData::ModuleDeclaration(declaration) => {
                if common & 1 != 0 && !flags.contains(NodeFlags::JS_DOC) {
                    flags |= NodeFlags::NAMESPACE;
                }
                if let Some(body) = declaration.body {
                    self.nested_namespace(body);
                }
            }
            NodeData::ImportEqualsDeclaration(declaration) => {
                declaration.is_type_only = common & 1 != 0;
            }
            NodeData::ExportDeclaration(declaration) => declaration.is_type_only = common & 1 != 0,
            NodeData::ImportType(import) => import.is_type_of = common & 1 != 0,
            NodeData::ImportClause(clause) => match common {
                1 => clause.is_type_only = true,
                2 => clause.phase_modifier = Some(SyntaxKind::DeferKeyword),
                _ => {}
            },
            NodeData::ImportSpecifier(specifier) => specifier.is_type_only = common & 1 != 0,
            NodeData::JSDocTypeLiteral(literal) => literal.is_array_type = common & 1 != 0,
            NodeData::JSDocParameterTag(tag) => {
                tag.is_bracketed = common & 1 != 0;
                tag.is_name_first = common & 2 != 0;
            }
            NodeData::JSDocPropertyTag(tag) => {
                tag.is_bracketed = common & 1 != 0;
                tag.is_name_first = common & 2 != 0;
            }
            _ => {}
        }
        let node = self.arena.alloc_node(node_data, pos, end, flags);
        if multi_line.is_some() {
            self.arena.node_mut(node).set_multi_line(multi_line);
        }
        node
    }

    /// tsgo writes a type heritage element (an interface's `extends`, a
    /// class's `implements`) as a TypeReference of an entity name; tsc's
    /// is an ExpressionWithTypeArguments of an entity name expression.
    fn heritage_type(&mut self, element: NodeId) {
        let NodeData::TypeReference(reference) = &self.arena.node(element).data else {
            return;
        };
        let (type_name, type_arguments) = (reference.type_name, reference.type_arguments);
        let expression = type_name.map(|name| self.entity_name_expression(name));
        let node = self.arena.node_mut(element);
        node.kind = SyntaxKind::ExpressionWithTypeArguments;
        node.data = NodeData::ExpressionWithTypeArguments(ExpressionWithTypeArgumentsData {
            type_arguments,
            expression,
        });
    }

    /// tsc's entity name expression of an entity name: a QualifiedName is
    /// a PropertyAccessExpression.
    fn entity_name_expression(&mut self, name: NodeId) -> NodeId {
        if let NodeData::QualifiedName(qualified) = &self.arena.node(name).data {
            let (left, right) = (qualified.left, qualified.right);
            let expression = left.map(|left| self.entity_name_expression(left));
            let node = self.arena.node_mut(name);
            node.kind = SyntaxKind::PropertyAccessExpression;
            node.data = NodeData::PropertyAccessExpression(PropertyAccessExpressionData {
                name: right,
                expression,
                question_dot_token: None,
            });
        }
        name
    }

    /// tsgo gives the declaration of a dotted namespace name's later part
    /// an implicit `export` modifier; tsc flags it NestedNamespace instead.
    fn nested_namespace(&mut self, body: NodeId) {
        let node = self.arena.node_mut(body);
        if let NodeData::ModuleDeclaration(declaration) = &mut node.data {
            declaration.modifiers = None;
            node.flags |= NodeFlags::NESTED_NAMESPACE.bits();
        }
    }
}

/// The children of a node record in tsgo's visitor order (`childIterator`):
/// each property whose mask bit is set takes the next child.
pub(crate) struct Children<'a> {
    mask: u32,
    children: &'a [usize],
    next: usize,
    nodes: &'a [Option<NodeId>],
    lists: &'a [Option<NodeArrayId>],
    arena: &'a mut NodeArena,
}

impl Children<'_> {
    /// tsgo `nextIf`.
    fn next_if(&mut self, bit: u32) -> Option<usize> {
        if self.mask & (1 << bit) == 0 {
            return None;
        }
        let child = self.children.get(self.next).copied();
        self.next += 1;
        child
    }

    /// The node of property `bit` (tsgo `nodeAt`: none for a list).
    pub(crate) fn node(&mut self, bit: u32) -> Option<NodeId> {
        self.next_if(bit).and_then(|child| self.nodes[child])
    }

    /// The token of property `bit` and its kind.
    pub(crate) fn token(&mut self, bit: u32) -> Option<(NodeId, SyntaxKind)> {
        self.node(bit)
            .map(|node| (node, self.arena.node(node).kind))
    }

    /// The NodeList of property `bit` (tsgo `nodeListAt`: none for a node).
    pub(crate) fn list(&mut self, bit: u32) -> Option<NodeArrayId> {
        self.next_if(bit).and_then(|child| self.lists[child])
    }

    /// The comment of a JSDoc node: tsgo's NodeList of JSDocText and links.
    pub(crate) fn comment(&mut self, bit: u32) -> Option<JSDocComment> {
        self.list(bit).map(JSDocComment::Nodes)
    }

    /// A plain slice of property `bit` (tsgo's JSDoc property tags): every
    /// remaining child is an element, in a list with no range.
    pub(crate) fn slice(&mut self, bit: u32) -> Option<NodeArrayId> {
        if self.mask & (1 << bit) == 0 {
            return None;
        }
        let elements = self.children[self.next.min(self.children.len())..]
            .iter()
            .filter_map(|&child| self.nodes[child])
            .collect::<Vec<_>>();
        self.next = self.children.len();
        Some(
            self.arena
                .alloc_array(&elements, u32::MAX as usize, u32::MAX as usize, false),
        )
    }

    /// A property tsc-rs's tree has no place for.
    pub(crate) fn skip(&mut self, bit: u32) {
        self.next_if(bit);
    }
}

/// tsgo's node flags to tsc's ([`NODE_FLAGS`] read the other way). On a
/// module declaration, tsgo's OptionalChain bit is NestedNamespace.
fn port_node_flags(kind: SyntaxKind, flags: u32) -> NodeFlags {
    let mut bits = NODE_FLAGS
        .iter()
        .filter(|(_, to)| flags & to != 0)
        .fold(0, |out, (from, _)| out | from);
    if kind == SyntaxKind::ModuleDeclaration && flags & TSGO_NESTED_NAMESPACE != 0 {
        bits = (bits & !NodeFlags::OPTIONAL_CHAIN.bits()) | NodeFlags::NESTED_NAMESPACE.bits();
    }
    NodeFlags::from_bits(bits)
}

/// tsc's templateFlags of tsgo's token flags.
fn template_flags(token_flags: u32) -> u16 {
    (token_flags & TokenFlags::TEMPLATE_LITERAL_LIKE_FLAGS.bits() as u32) as u16
}

/// tsgo `readLE32`: zero outside the data.
fn read_le32(data: &[u8], offset: usize) -> u32 {
    data.get(offset..offset + 4).map_or(0, |bytes| {
        u32::from_le_bytes(bytes.try_into().expect("four bytes"))
    })
}

/// A Rust string of tsgo's string bytes (UTF-8).
fn string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A JavaScript string of tsgo's string bytes: UTF-8, with a lone
/// surrogate in its three-byte form.
fn js_string(bytes: &[u8]) -> JsString {
    let mut out = JsString::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                out.push_str(text);
                break;
            }
            Err(error) => {
                let (valid, after) = rest.split_at(error.valid_up_to());
                out.push_str(std::str::from_utf8(valid).expect("the valid prefix"));
                if let [0xED, second @ 0xA0..=0xBF, third @ 0x80..=0xBF, ..] = *after {
                    out.push_code_unit(
                        0xD000 | (u16::from(second & 0x3F) << 6) | u16::from(third & 0x3F),
                    );
                    rest = &after[3..];
                } else {
                    out.push('\u{FFFD}');
                    rest = &after[error.error_len().unwrap_or(after.len())..];
                }
            }
        }
    }
    out
}
