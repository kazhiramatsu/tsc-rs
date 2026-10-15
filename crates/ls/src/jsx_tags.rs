//! tsgo `ls/linkedediting.go` and `ls/autoinsert.go`: the JSX tag names
//! edited together (`textDocument/linkedEditingRange`) and the closing tag
//! inserted after a `>` (`textDocument/_vs_onAutoInsert`).

use tsc_lsproto::{
    InsertTextFormat, LinkedEditingRangeParams, LinkedEditingRangeResponse, LinkedEditingRanges,
    LinkedEditingRangesOrNull, Range, TextEdit, VSOnAutoInsertParams, VSOnAutoInsertResponse,
    VSOnAutoInsertResponseItem, VSOnAutoInsertResponseItemOrNull,
};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::NodeFlags;

use crate::astnav::{Found, Navigator};
use crate::language_service::{LanguageService, LsError};
use crate::tree::View;
use crate::utilities::script_of;

/// tsgo `jsxTagWordPattern`: more than valid tag names, for a tag name being
/// typed.
const JSX_TAG_WORD_PATTERN: &str = "[a-zA-Z0-9:\\-\\._$]*";

impl LanguageService<'_> {
    /// tsgo `ProvideLinkedEditingRange`: a fragment's positions after `<`
    /// and `</` (at one of them), or an element's two tag names (in one of
    /// them, when both are well formed and alike).
    pub fn provide_linked_editing_range(
        &mut self,
        params: &LinkedEditingRangeParams,
    ) -> Result<LinkedEditingRangeResponse, LsError> {
        let file = self.file(&params.text_document.uri);
        let converters = self.converters;
        let ranges = self
            .with_request(file, &params.position, false, |request, position| {
                let source = request.state.binder.source(file);
                let script = script_of(source);
                let jsx = JsxTags::new(source);
                Ok(jsx
                    .linked_editing_ranges(position as usize)
                    .map(|(open, close)| {
                        [open, close]
                            .map(|(start, end)| converters.to_lsp_range(&script, start..end))
                    }))
            })?
            .flatten();
        Ok(match ranges {
            Some([open, close]) => {
                LinkedEditingRangesOrNull::LinkedEditingRanges(LinkedEditingRanges {
                    ranges: vec![open, close],
                    word_pattern: Some(JSX_TAG_WORD_PATTERN.to_owned()),
                })
            }
            None => LinkedEditingRangesOrNull::Null,
        })
    }

    /// tsgo `ProvideOnAutoInsert`: after a `>` that ends an unclosed JSX
    /// element's (fragment's) opening tag, its closing tag as a snippet.
    pub fn provide_on_auto_insert(
        &mut self,
        params: &VSOnAutoInsertParams,
    ) -> Result<VSOnAutoInsertResponse, LsError> {
        if self.user_preferences_ref().auto_closing_tags == Some(false) || params._vs_ch != ">" {
            return Ok(VSOnAutoInsertResponseItemOrNull::Null);
        }
        let file = self.file(&params._vs_text_document.uri);
        let closing_text = self
            .with_request(file, &params._vs_position, false, |request, position| {
                let source = request.state.binder.source(file);
                Ok(JsxTags::new(source).closing_text(position as usize))
            })?
            .flatten();
        let Some(closing_text) = closing_text else {
            return Ok(VSOnAutoInsertResponseItemOrNull::Null);
        };
        Ok(
            VSOnAutoInsertResponseItemOrNull::VSOnAutoInsertResponseItem(
                VSOnAutoInsertResponseItem {
                    _vs_text_edit_format: InsertTextFormat::SNIPPET,
                    _vs_text_edit: TextEdit {
                        range: Range {
                            start: params._vs_position.clone(),
                            end: params._vs_position.clone(),
                        },
                        // A tag name can hold `$`, which a snippet would read as
                        // a placeholder.
                        new_text: format!(
                            "$0{}",
                            crate::completions::escape_snippet_text(&closing_text)
                        ),
                    },
                },
            ),
        )
    }
}

/// One file's JSX tags.
struct JsxTags<'a> {
    source: &'a SourceFile,
    navigator: Navigator<'a>,
}

impl<'a> JsxTags<'a> {
    fn new(source: &'a SourceFile) -> Self {
        Self {
            source,
            navigator: Navigator::new(source),
        }
    }

    fn kind(&self, node: NodeId) -> SyntaxKind {
        self.source.arena.node(node).kind
    }

    fn data(&self, node: NodeId) -> &'a NodeData {
        &self.source.arena.node(node).data
    }

    fn parent(&self, node: NodeId) -> Option<NodeId> {
        self.source.arena.node(node).parent
    }

    fn has_flag(&self, node: NodeId, flag: NodeFlags) -> bool {
        NodeFlags::from_bits(self.source.arena.node(node).flags).contains(flag)
    }

    /// tsgo `GetStartOfNode(node, file, false)`.
    fn start(&self, node: NodeId) -> usize {
        self.navigator
            .start_of_node(Found::Node(View::Node(node)), false) as usize
    }

    fn end(&self, node: NodeId) -> usize {
        self.source.arena.node(node).end as usize
    }

    /// The ranges of tsgo `ProvideLinkedEditingRange` (empty ones for a
    /// fragment).
    fn linked_editing_ranges(&self, position: usize) -> Option<((usize, usize), (usize, usize))> {
        let token = self.navigator.preceding_token(position as u32)?;
        let parent = match self.navigator.parent(token)? {
            Found::Node(view) => view.node()?,
            Found::Token(_) => return None,
        };
        if self.kind(parent) == SyntaxKind::SourceFile {
            return None;
        }
        if let Some(grandparent) = self.parent(parent) {
            if let NodeData::JsxFragment(fragment) = self.data(grandparent) {
                let open = fragment.opening_fragment?;
                let close = fragment.closing_fragment?;
                if self.has_flag(open, NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
                    || self.has_flag(close, NodeFlags::THIS_NODE_OR_ANY_SUB_NODES_HAS_ERROR)
                {
                    return None;
                }
                let open_pos = self.start(open) + "<".len();
                let close_pos = self.start(close) + "</".len();
                // Only right after the opening bracket: `<| ></| >`.
                if position != open_pos && position != close_pos {
                    return None;
                }
                return Some(((open_pos, open_pos), (close_pos, close_pos)));
            }
        }
        // The cursor is in an element's tag.
        let mut tag = Some(parent);
        while let Some(node) = tag {
            if matches!(
                self.kind(node),
                SyntaxKind::JsxOpeningElement | SyntaxKind::JsxClosingElement
            ) {
                break;
            }
            tag = self.parent(node);
        }
        let element = self.parent(tag?)?;
        let NodeData::JsxElement(element) = self.data(element) else {
            return None;
        };
        let open_tag = element.opening_element?;
        let close_tag = element.closing_element?;
        let open_name = self.tag_name(open_tag)?;
        let close_name = self.tag_name(close_tag)?;
        let (open_start, open_end) = (self.start(open_name), self.end(open_name));
        let (close_start, close_end) = (self.start(close_name), self.end(close_name));
        // Not for tags that are not well formed.
        if open_start == self.start(open_tag)
            || close_start == self.start(close_tag)
            || open_end == self.end(open_tag)
            || close_end == self.end(close_tag)
        {
            return None;
        }
        // Only within a tag name.
        if !((open_start..=open_end).contains(&position)
            || (close_start..=close_end).contains(&position))
        {
            return None;
        }
        // Only for tags with the same text.
        if self.text_of_node(open_name) != self.text_of_node(close_name) {
            return None;
        }
        Some(((open_start, open_end), (close_start, close_end)))
    }

    /// tsgo `ProvideOnAutoInsert`'s closing text: `</tag>` for an unclosed
    /// element whose opening tag (or text after it) is before the position,
    /// `</>` for such a fragment.
    fn closing_text(&self, position: usize) -> Option<String> {
        let token = self.navigator.preceding_token(position as u32)?;
        let token_kind = self.navigator.kind(token);
        let parent = match self.navigator.parent(token)? {
            Found::Node(view) => view.node(),
            Found::Token(_) => None,
        };
        let parent_kind = parent.map(|parent| self.kind(parent));
        let element = if token_kind == SyntaxKind::GreaterThanToken
            && parent_kind == Some(SyntaxKind::JsxOpeningElement)
        {
            parent.and_then(|parent| self.parent(parent))
        } else if token_kind == SyntaxKind::JsxText && parent_kind == Some(SyntaxKind::JsxElement) {
            parent
        } else {
            None
        };
        if let Some(element) = element.filter(|&element| self.is_unclosed_tag(element)) {
            let NodeData::JsxElement(data) = self.data(element) else {
                return None;
            };
            let tag_name = self.tag_name(data.opening_element?)?;
            // Not the opening tag's verbatim text (tsgo's divergence from
            // Strada).
            return Some(format!("</{}>", self.entity_name_to_string(tag_name)));
        }
        let fragment = if token_kind == SyntaxKind::GreaterThanToken
            && parent_kind == Some(SyntaxKind::JsxOpeningFragment)
        {
            parent.and_then(|parent| self.parent(parent))
        } else if token_kind == SyntaxKind::JsxText && parent_kind == Some(SyntaxKind::JsxFragment)
        {
            parent
        } else {
            None
        };
        fragment
            .filter(|&fragment| self.is_unclosed_fragment(fragment))
            .map(|_| "</>".to_owned())
    }

    /// tsgo `isUnclosedTag`: the closing tag names another element, or the
    /// parent element of the same name is unclosed.
    fn is_unclosed_tag(&self, element: NodeId) -> bool {
        let NodeData::JsxElement(data) = self.data(element) else {
            return false;
        };
        let opening_name = data
            .opening_element
            .and_then(|opening| self.tag_name(opening));
        let closing_name = data
            .closing_element
            .and_then(|closing| self.tag_name(closing));
        if !tsc_syntax::utilities::tag_names_are_equivalent(
            &self.source.arena,
            opening_name,
            closing_name,
        ) {
            return true;
        }
        match self
            .parent(element)
            .map(|parent| (parent, self.data(parent)))
        {
            Some((parent, NodeData::JsxElement(parent_data))) => {
                let parent_name = parent_data
                    .opening_element
                    .and_then(|opening| self.tag_name(opening));
                tsc_syntax::utilities::tag_names_are_equivalent(
                    &self.source.arena,
                    opening_name,
                    parent_name,
                ) && self.is_unclosed_tag(parent)
            }
            _ => false,
        }
    }

    /// tsgo `isUnclosedFragment`: its closing fragment has an error, or its
    /// parent fragment is unclosed.
    fn is_unclosed_fragment(&self, fragment: NodeId) -> bool {
        let NodeData::JsxFragment(data) = self.data(fragment) else {
            return false;
        };
        if data
            .closing_fragment
            .is_some_and(|closing| self.has_flag(closing, NodeFlags::THIS_NODE_HAS_ERROR))
        {
            return true;
        }
        self.parent(fragment).is_some_and(|parent| {
            self.kind(parent) == SyntaxKind::JsxFragment && self.is_unclosed_fragment(parent)
        })
    }

    fn tag_name(&self, tag: NodeId) -> Option<NodeId> {
        match self.data(tag) {
            NodeData::JsxOpeningElement(data) => data.tag_name,
            NodeData::JsxClosingElement(data) => data.tag_name,
            NodeData::JsxSelfClosingElement(data) => data.tag_name,
            _ => None,
        }
    }

    /// tsgo `scanner.GetTextOfNode` (a missing node's is empty).
    fn text_of_node(&self, node: NodeId) -> &'a str {
        let record = self.source.arena.node(node);
        if record.pos == record.end {
            return "";
        }
        &self.source.text()[self.start(node)..record.end as usize]
    }

    /// tsgo `ast.EntityNameToString(name, scanner.GetTextOfNode)`.
    fn entity_name_to_string(&self, name: NodeId) -> String {
        match self.data(name) {
            NodeData::QualifiedName(data) => format!(
                "{}.{}",
                data.left
                    .map_or(String::new(), |left| self.entity_name_to_string(left)),
                data.right
                    .map_or(String::new(), |right| self.entity_name_to_string(right))
            ),
            NodeData::PropertyAccessExpression(data) => format!(
                "{}.{}",
                data.expression.map_or(String::new(), |expression| self
                    .entity_name_to_string(expression)),
                data.name
                    .map_or(String::new(), |name| self.entity_name_to_string(name))
            ),
            NodeData::JsxNamespacedName(data) => format!(
                "{}:{}",
                data.namespace.map_or(String::new(), |namespace| self
                    .entity_name_to_string(namespace)),
                data.name
                    .map_or(String::new(), |name| self.entity_name_to_string(name))
            ),
            _ => self.text_of_node(name).to_owned(),
        }
    }
}
