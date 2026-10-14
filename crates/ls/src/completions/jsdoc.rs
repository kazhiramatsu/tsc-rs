//! tsgo's JSDoc completions: the tag names and the parameters a JSDoc
//! comment of a function may name (`completions.go`), and the doc comment
//! template after `/**` (`jsdoc_snippet.go`).

use tsc_binder::node_util;
use tsc_checker::state::CheckResult;
use tsc_lsproto::{
    CompletionItem, CompletionItemKind, InsertReplaceEdit, InsertTextFormat, TextEdit,
    TextEditOrInsertReplaceEdit,
};
use tsc_syntax::{NodeArrayId, NodeData, NodeId, ParseOptions, SourceFile, SyntaxKind};
use tsc_types::{NodeFlags, TypeFlags};

use crate::astnav::{Found, Navigator, Token};
use crate::lsutil::new_line_kind;
use crate::tree::View;
use crate::utilities::{is_function_like_kind, parameters_in, parameters_of, touching, Touching};

use super::context::Completions;
use super::{default_commit_characters, items, sort_text, ClientSupport, CompletionList};
use crate::format::is_white_space_single_line;

/// tsgo `jsDocTagNames`.
const JS_DOC_TAG_NAMES: [&str; 84] = [
    "abstract",
    "access",
    "alias",
    "argument",
    "async",
    "augments",
    "author",
    "borrows",
    "callback",
    "class",
    "classdesc",
    "constant",
    "constructor",
    "constructs",
    "copyright",
    "default",
    "deprecated",
    "description",
    "emits",
    "enum",
    "event",
    "example",
    "exports",
    "extends",
    "external",
    "field",
    "file",
    "fileoverview",
    "fires",
    "function",
    "generator",
    "global",
    "hideconstructor",
    "host",
    "ignore",
    "implements",
    "import",
    "inheritdoc",
    "inner",
    "instance",
    "interface",
    "kind",
    "lends",
    "license",
    "link",
    "linkcode",
    "linkplain",
    "listens",
    "member",
    "memberof",
    "method",
    "mixes",
    "module",
    "name",
    "namespace",
    "overload",
    "override",
    "package",
    "param",
    "private",
    "prop",
    "property",
    "protected",
    "public",
    "readonly",
    "requires",
    "returns",
    "satisfies",
    "see",
    "since",
    "static",
    "summary",
    "template",
    "this",
    "throws",
    "todo",
    "tutorial",
    "type",
    "typedef",
    "var",
    "variation",
    "version",
    "virtual",
    "yields",
];

/// tsgo `nodebuilder.FlagsUseSingleQuotesForStringLiteralType`.
const USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE: u32 = 1 << 28;

/// A JSDoc completion item: a label, its kind and the location priority.
fn jsdoc_item(label: String, kind: CompletionItemKind) -> CompletionItem {
    CompletionItem {
        label,
        kind: Some(kind),
        sort_text: Some(sort_text::LOCATION_PRIORITY.to_owned()),
        ..CompletionItem::default()
    }
}

impl Completions<'_, '_, '_> {
    /// tsgo `jsDocCompletionInfo`.
    fn jsdoc_completion_info(&self, mut items: Vec<CompletionItem>) -> CompletionList {
        let position = self
            .request
            .lsp_range(self.source, self.position as usize..self.position as usize)
            .start;
        let item_defaults = items::set_item_defaults(
            ClientSupport(self.request.capabilities),
            position,
            &mut items,
            Some(default_commit_characters(false)),
            None,
        );
        CompletionList {
            is_incomplete: false,
            item_defaults,
            items,
        }
    }

    /// tsgo's `completionDataJSDocTagName` (`tag_name_only`) and
    /// `completionDataJSDocTag` answers: the tag names, with or without
    /// `@`, then the `@param` tags of the parameters not annotated yet.
    pub(super) fn jsdoc_tag_completions(
        &mut self,
        tag_name_only: bool,
    ) -> CheckResult<CompletionList> {
        let mut items = JS_DOC_TAG_NAMES
            .iter()
            .map(|&name| {
                let label = if tag_name_only {
                    name.to_owned()
                } else {
                    format!("@{name}")
                };
                jsdoc_item(label, CompletionItemKind::KEYWORD)
            })
            .collect::<Vec<_>>();
        items.extend(self.jsdoc_parameter_completions(tag_name_only)?);
        Ok(self.jsdoc_completion_info(items))
    }

    /// tsgo `getJSDocParameterCompletions`: an `@param` tag for each of the
    /// function's parameters after those the tags before the position
    /// annotate (tsgo has no snippets here).
    fn jsdoc_parameter_completions(
        &mut self,
        tag_name_only: bool,
    ) -> CheckResult<Vec<CompletionItem>> {
        let Touching::Node(current) = self.token_at(self.position) else {
            return Ok(Vec::new());
        };
        let js_doc = match self.node_kind(current) {
            SyntaxKind::JSDoc => current,
            kind if is_jsdoc_tag_kind(kind) => match self.parent_of(current) {
                Some(parent) => parent,
                None => return Ok(Vec::new()),
            },
            _ => return Ok(Vec::new()),
        };
        let NodeData::JSDoc(data) = self.data(js_doc) else {
            return Ok(Vec::new());
        };
        let Some(function) = self
            .parent_of(js_doc)
            .filter(|&function| is_function_like_kind(self.node_kind(function)))
        else {
            return Ok(Vec::new());
        };
        let is_js = self.is_source_file_js();
        let tags = self.node_list(data.tags).to_vec();
        let param_tag_count = tags
            .iter()
            .filter(|&&tag| {
                self.node_kind(tag) == SyntaxKind::JSDocParameterTag
                    && self.node_start(tag) < self.position
                    && self
                        .name_of(tag)
                        .is_some_and(|name| self.node_kind(name) == SyntaxKind::Identifier)
            })
            .count();
        let new_line = new_line_kind(self.request.state.options).text();
        let mut items = Vec::new();
        for (index, parameter) in parameters_of(self.request.state, function)
            .into_iter()
            .enumerate()
        {
            if index < param_tag_count {
                // The parameter has its tag.
                continue;
            }
            let NodeData::Parameter(data) = self.data(parameter) else {
                continue;
            };
            let (name, initializer, dot_dot_dot) = (
                data.name,
                data.initializer,
                data.dot_dot_dot_token.is_some(),
            );
            let Some(name) = name else {
                continue;
            };
            let display_text = if self.node_kind(name) == SyntaxKind::Identifier {
                let parameter_name = self
                    .identifier_text(Touching::Node(name))
                    .unwrap_or_default();
                self.jsdoc_param_annotation(parameter_name, initializer, dot_dot_dot, is_js, false)?
            } else if index == param_tag_count {
                // A destructured parameter, by its position.
                let path = format!("param{index}");
                let tags = self.jsdoc_param_tags_for_destructuring(
                    &path,
                    name,
                    initializer,
                    dot_dot_dot,
                    is_js,
                )?;
                tags.join(&format!("{new_line}* "))
            } else {
                continue;
            };
            let label = if tag_name_only {
                display_text
                    .strip_prefix('@')
                    .unwrap_or(&display_text)
                    .to_owned()
            } else {
                display_text
            };
            items.push(jsdoc_item(label, CompletionItemKind::VARIABLE));
        }
        Ok(items)
    }

    /// tsgo `getJSDocParamAnnotation` without snippets: `@param` with the
    /// name (bracketed with its default) and, in JavaScript, the type.
    fn jsdoc_param_annotation(
        &mut self,
        parameter_name: &str,
        initializer: Option<NodeId>,
        dot_dot_dot: bool,
        is_js: bool,
        is_object: bool,
    ) -> CheckResult<String> {
        let parameter_name = match initializer {
            Some(initializer) => {
                self.jsdoc_param_name_with_initializer(parameter_name, initializer)
            }
            None => parameter_name.to_owned(),
        };
        if !is_js {
            return Ok(format!("@param {parameter_name} "));
        }
        let mut ty = "*".to_owned();
        if is_object {
            ty = "object".to_owned();
        } else if let Some(initializer) = initializer {
            let host = self
                .parent_of(initializer)
                .expect("an initializer's declaration");
            let inferred = self.request.state.get_type_at_location(host)?;
            if !self
                .request
                .state
                .tables
                .flags_of(inferred)
                .intersects(TypeFlags::ANY | TypeFlags::VOID)
            {
                let flags = if self.quote_preference() == crate::lsutil::QuotePreference::SINGLE {
                    USE_SINGLE_QUOTES_FOR_STRING_LITERAL_TYPE
                } else {
                    0
                };
                let enclosing = self.find_ancestor(Some(initializer), |this, node| {
                    is_function_like_kind(this.node_kind(node))
                });
                if let Ok(text) = self
                    .request
                    .state
                    .type_to_string_with_flags(inferred, enclosing, flags, None)
                {
                    ty = text;
                }
            }
        }
        let dot_dot_dot = if !is_object && dot_dot_dot { "..." } else { "" };
        Ok(format!("@param {{{dot_dot_dot}{ty}}} {parameter_name} "))
    }

    /// tsgo `getJSDocParamNameWithInitializer`: `[name=default]`, or
    /// `[name]` for a long or multiline default.
    fn jsdoc_param_name_with_initializer(
        &self,
        parameter_name: &str,
        initializer: NodeId,
    ) -> String {
        let source = self.source_of(initializer);
        let start = crate::utilities::token_start(source, initializer);
        let text = source.text()[start..self.record(initializer).end as usize].trim();
        if text.contains('\n') || text.len() > 80 {
            return format!("[{parameter_name}]");
        }
        format!("[{parameter_name}={text}]")
    }

    /// tsgo `generateJSDocParamTagsForDestructuring`.
    fn jsdoc_param_tags_for_destructuring(
        &mut self,
        path: &str,
        pattern: NodeId,
        initializer: Option<NodeId>,
        dot_dot_dot: bool,
        is_js: bool,
    ) -> CheckResult<Vec<String>> {
        if !is_js {
            return Ok(vec![self.jsdoc_param_annotation(
                path,
                initializer,
                dot_dot_dot,
                is_js,
                false,
            )?]);
        }
        self.jsdoc_param_pattern_worker(path, pattern, initializer, dot_dot_dot, is_js)
    }

    /// tsgo `jsDocParamPatternWorker`: an object pattern's `@param
    /// {object}` and a tag for each element, else one tag.
    fn jsdoc_param_pattern_worker(
        &mut self,
        path: &str,
        pattern: NodeId,
        initializer: Option<NodeId>,
        dot_dot_dot: bool,
        is_js: bool,
    ) -> CheckResult<Vec<String>> {
        if let (NodeData::ObjectBindingPattern(data), false) = (self.data(pattern), dot_dot_dot) {
            let root = self.jsdoc_param_annotation(path, initializer, dot_dot_dot, is_js, true)?;
            let mut child_tags = Vec::new();
            for element in self.node_list(data.elements).to_vec() {
                let element_tags = self.jsdoc_param_element_worker(path, element, is_js)?;
                if element_tags.is_empty() {
                    child_tags.clear();
                    break;
                }
                child_tags.extend(element_tags);
            }
            if !child_tags.is_empty() {
                let mut tags = vec![root];
                tags.extend(child_tags);
                return Ok(tags);
            }
        }
        Ok(vec![self.jsdoc_param_annotation(
            path,
            initializer,
            dot_dot_dot,
            is_js,
            false,
        )?])
    }

    /// tsgo `jsDocParamElementWorker`: the tags of an object binding
    /// element (an array pattern is not annotated deeply).
    fn jsdoc_param_element_worker(
        &mut self,
        path: &str,
        element: NodeId,
        is_js: bool,
    ) -> CheckResult<Vec<String>> {
        let NodeData::BindingElement(data) = self.data(element) else {
            return Ok(Vec::new());
        };
        let (name, property_name, initializer, dot_dot_dot) = (
            data.name,
            data.property_name,
            data.initializer,
            data.dot_dot_dot_token.is_some(),
        );
        let Some(name) = name else {
            return Ok(Vec::new());
        };
        if self.node_kind(name) == SyntaxKind::Identifier {
            let property = match property_name {
                Some(property_name) => self.text_of_property_name(property_name),
                None => self
                    .identifier_text(Touching::Node(name))
                    .unwrap_or_default()
                    .to_owned(),
            };
            if property.is_empty() {
                return Ok(Vec::new());
            }
            let parameter_name = format!("{path}.{property}");
            return Ok(vec![self.jsdoc_param_annotation(
                &parameter_name,
                initializer,
                dot_dot_dot,
                is_js,
                false,
            )?]);
        }
        if let Some(property_name) = property_name {
            let property = self.text_of_property_name(property_name);
            if property.is_empty() {
                return Ok(Vec::new());
            }
            return self.jsdoc_param_pattern_worker(
                &format!("{path}.{property}"),
                name,
                initializer,
                dot_dot_dot,
                is_js,
            );
        }
        Ok(Vec::new())
    }

    /// tsgo `ast.TryGetTextOfPropertyName`: an identifier's, a string or
    /// numeric literal's text, empty for another name.
    fn text_of_property_name(&self, name: NodeId) -> String {
        match self.data(name) {
            NodeData::StringLiteral(data) => data.text.to_string_lossy().into_owned(),
            NodeData::NoSubstitutionTemplateLiteral(data) => {
                data.text.to_string_lossy().into_owned()
            }
            NodeData::NumericLiteral(data) => data.text.to_string(),
            _ => self
                .identifier_text(Touching::Node(name))
                .unwrap_or_default()
                .to_owned(),
        }
    }

    /// tsgo `getJSDocParameterNameCompletions`: the function's parameter
    /// names that no other tag has, starting with the name written.
    pub(super) fn jsdoc_parameter_name_completions(&self, tag: NodeId) -> CompletionList {
        let name = self.name_of(tag);
        if name.is_some_and(|name| self.node_kind(name) != SyntaxKind::Identifier) {
            return self.jsdoc_completion_info(Vec::new());
        }
        let name_thus_far = name
            .and_then(|name| self.identifier_text(Touching::Node(name)))
            .unwrap_or_default()
            .to_owned();
        let js_doc = self.parent_of(tag).expect("a tag's JSDoc comment");
        let function = self
            .parent_of(js_doc)
            .filter(|&function| is_function_like_kind(self.node_kind(function)));
        let (Some(function), NodeData::JSDoc(data)) = (function, self.data(js_doc)) else {
            return self.jsdoc_completion_info(Vec::new());
        };
        let tags = self.node_list(data.tags).to_vec();
        let mut items = Vec::new();
        for parameter in parameters_of(self.request.state, function) {
            let Some(parameter_name) = self
                .name_of(parameter)
                .filter(|&name| self.node_kind(name) == SyntaxKind::Identifier)
                .and_then(|name| self.identifier_text(Touching::Node(name)))
            else {
                continue;
            };
            let tagged = tags.iter().any(|&other| {
                other != tag
                    && self.node_kind(other) == SyntaxKind::JSDocParameterTag
                    && self
                        .name_of(other)
                        .filter(|&name| self.node_kind(name) == SyntaxKind::Identifier)
                        .and_then(|name| self.identifier_text(Touching::Node(name)))
                        == Some(parameter_name)
            });
            if tagged || !name_thus_far.is_empty() && !parameter_name.starts_with(&name_thus_far) {
                continue;
            }
            items.push(jsdoc_item(
                parameter_name.to_owned(),
                CompletionItemKind::VARIABLE,
            ));
        }
        self.jsdoc_completion_info(items)
    }

    /// tsgo `getJSDocSnippetCompletion`: the doc comment template after
    /// `/**`.
    pub(super) fn jsdoc_snippet_completion(&self) -> Option<CompletionList> {
        let preferences = self.request.preferences;
        if preferences.complete_js_docs == Some(false) {
            return None;
        }
        if !self.is_potentially_valid_jsdoc_snippet_completion_position() {
            return None;
        }
        let configured = &preferences
            .format_code_settings
            .editor_settings
            .new_line_character;
        let new_line = if configured.is_empty() {
            "\n"
        } else {
            configured.as_str()
        };
        let template = TemplateFile::new(self.source).template_at(
            self.position,
            preferences.generate_return_in_doc_template == Some(true),
            new_line,
        )?;
        let support = ClientSupport(self.request.capabilities);
        let (insert_text, insert_text_format) = if support.item_snippet() {
            (
                template_to_snippet(&template, new_line),
                Some(InsertTextFormat::SNIPPET),
            )
        } else {
            (template, None)
        };
        let text_edit = self.jsdoc_snippet_completion_range(insert_text);
        let detail = tsc_diagnostics::gen::JSDoc_comment
            .template_in(self.request.catalog)
            .to_owned();
        let item = CompletionItem {
            label: "/** */".to_owned(),
            kind: Some(CompletionItemKind::TEXT),
            detail: Some(detail),
            sort_text: Some("\0".to_owned()),
            insert_text_format,
            text_edit: Some(text_edit),
            commit_characters: support.item_commit_characters().then(Vec::new),
            ..CompletionItem::default()
        };
        Some(CompletionList {
            is_incomplete: false,
            item_defaults: None,
            items: vec![item],
        })
    }

    /// tsgo `isPotentiallyValidJSDocSnippetCompletionPosition`.
    fn is_potentially_valid_jsdoc_snippet_completion_position(&self) -> bool {
        let text = self.text();
        let line_start = self.line_start_of(self.position) as usize;
        let prefix = &text[line_start..self.position as usize];
        if !is_jsdoc_snippet_prefix(prefix) {
            return false;
        }
        let line_end = self.line_end_of(self.position) as usize;
        is_jsdoc_snippet_suffix(&text[self.position as usize..line_end])
    }

    /// tsgo `getJSDocSnippetCompletionRange`: over the `/**` (or `/`)
    /// before the position and the `*/` after it.
    fn jsdoc_snippet_completion_range(&self, new_text: String) -> TextEditOrInsertReplaceEdit {
        let text = self.text();
        let position = self.position as usize;
        let line_start = self.line_start_of(self.position) as usize;
        let start = jsdoc_snippet_prefix_start(&text[line_start..position])
            .map_or(position, |prefix_start| line_start + prefix_start);
        let line_end = self.line_end_of(self.position) as usize;
        let end = position
            + jsdoc_snippet_suffix_end(&text[position..line_end]).map_or(0, |(end, _)| end);
        let range = self.request.lsp_range(self.source, start..end);
        if ClientSupport(self.request.capabilities).item_insert_replace() {
            return TextEditOrInsertReplaceEdit::InsertReplaceEdit(InsertReplaceEdit {
                new_text,
                insert: range.clone(),
                replace: range,
            });
        }
        TextEditOrInsertReplaceEdit::TextEdit(TextEdit { new_text, range })
    }
}

/// One parse of the file as the doc comment template reads it: the
/// program's, or tsgo's reparse with the comment closed.
struct TemplateFile<'a> {
    source: &'a SourceFile,
    navigator: Navigator<'a>,
    line_starts: Vec<usize>,
}

impl<'a> TemplateFile<'a> {
    fn new(source: &'a SourceFile) -> Self {
        Self {
            source,
            navigator: Navigator::new(source),
            line_starts: tsc_emitter::source_mapper::compute_ecma_line_starts(source.text()),
        }
    }

    fn text(&self) -> &'a str {
        self.source.text()
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

    fn nodes(&self, list: Option<NodeArrayId>) -> &'a [NodeId] {
        list.map_or(&[], |list| self.source.arena.node_array(list).nodes)
    }

    fn identifier_text(&self, node: Option<NodeId>) -> Option<&'a str> {
        node.filter(|&node| self.kind(node) == SyntaxKind::Identifier)
            .and_then(|node| node_util::id_text(self.source, node))
    }

    /// tsgo `GetTokenAtPosition`.
    fn token_at(&self, position: u32) -> Touching {
        touching(self.navigator.token_at(position)).unwrap_or(Touching::Node(self.source.root))
    }

    /// tsgo `GetStartOfNode(node, file, false)`.
    fn start(&self, token: Touching) -> u32 {
        let found = match token {
            Touching::Node(node) => Found::Node(View::Node(node)),
            Touching::Token {
                kind,
                pos,
                end,
                parent,
            } => Found::Token(Token {
                kind,
                pos,
                end,
                parent: View::Node(parent),
            }),
        };
        self.navigator.start_of_node(found, false)
    }

    /// The line of a position.
    fn line_of(&self, position: usize) -> usize {
        match self.line_starts.binary_search(&position) {
            Ok(line) => line,
            Err(insertion) => insertion.saturating_sub(1),
        }
    }

    /// tsgo `format.GetLineStartPositionForPosition`.
    fn line_start(&self, position: usize) -> usize {
        self.line_starts[self.line_of(position)]
    }

    /// tsgo `getLineEndOfPosition`.
    fn line_end(&self, position: usize) -> usize {
        let line = self.line_of(position);
        let last = if line + 1 >= self.line_starts.len() {
            self.source.arena.node(self.source.root).end as usize
        } else {
            self.line_starts[line + 1] - 1
        };
        let bytes = self.text().as_bytes();
        if last > 0 && last < bytes.len() && bytes[last] == b'\n' && bytes[last - 1] == b'\r' {
            return last - 1;
        }
        last
    }

    /// tsgo `getDocCommentTemplateAtPosition`: the comment of the
    /// declaration after the position, with its parameters' (and maybe its
    /// return's) tags; an unclosed comment that has text is closed and the
    /// file parsed again.
    fn template_at(&self, position: u32, generate_return: bool, new_line: &str) -> Option<String> {
        let mut token = self.token_at(position);
        let mut existing_doc_comment = Some(match token {
            Touching::Node(node) => node,
            Touching::Token { parent, .. } => parent,
        });
        while let Some(current) = existing_doc_comment {
            if self.kind(current) == SyntaxKind::JSDoc {
                break;
            }
            existing_doc_comment = self.parent(current);
        }
        let comment_end = self.doc_comment_end_at_position(position as usize);
        let is_in_empty_doc_comment = existing_doc_comment.is_some() || comment_end.is_some();
        let non_empty =
            existing_doc_comment.is_some_and(|comment| self.is_non_empty_jsdoc(comment));
        if non_empty && comment_end.is_some_and(|(_, has_closing)| !has_closing) {
            let text = self.text();
            let at = position as usize;
            let reparse_text = format!("{} */{}", &text[..at], &text[at..]);
            let reparsed = tsc_syntax::parse_source_file(
                self.source.file_name.clone(),
                reparse_text,
                ParseOptions {
                    script_kind: self.source.script_kind,
                    js_doc_parsing_mode: self.source.js_doc_parsing_mode,
                    ..ParseOptions::default()
                },
                None,
            );
            return TemplateFile::new(&reparsed).template_at(position, generate_return, new_line);
        }
        if non_empty {
            return None;
        }
        if existing_doc_comment.is_none() {
            if let Some((end, _)) = comment_end {
                token = self.token_at(skip_whitespace(self.text(), end) as u32);
            }
        }
        let token_start = self.start(token);
        if !is_in_empty_doc_comment && token_start < position {
            return None;
        }
        let owner = self.comment_owner_info(token, generate_return)?;
        let last_js_doc = self.last_js_doc(owner.comment_owner);
        if self.start(Touching::Node(owner.comment_owner)) < position
            || last_js_doc.is_some()
                && existing_doc_comment.is_some()
                && last_js_doc != existing_doc_comment
        {
            return None;
        }
        let indentation = self.indentation_string_at_position(position as usize);
        let mut tags = self.parameter_doc_comments(&owner.parameters, indentation, new_line);
        if owner.has_return {
            tags.push_str(&format!("{indentation} * @returns{new_line}"));
        }
        if !tags.is_empty() && !self.has_jsdoc_tags(owner.comment_owner) {
            let preamble = format!("/**{new_line}{indentation} * ");
            let end_line = if token_start == position {
                format!("{new_line}{indentation}")
            } else {
                String::new()
            };
            return Some(format!(
                "{preamble}{new_line}{tags}{indentation} */{end_line}"
            ));
        }
        Some("/** */".to_owned())
    }

    /// The last JSDoc comment of a node (tsgo `node.JSDoc(file)`'s last).
    fn last_js_doc(&self, node: NodeId) -> Option<NodeId> {
        self.source
            .arena
            .node(node)
            .js_doc
            .and_then(|docs| self.source.arena.node_array(docs).nodes.last().copied())
    }

    /// tsgo `getDocCommentEndAtPosition`: after a `/**` before the
    /// position, the end of the `*/` after it (or the position) and whether
    /// there is one.
    fn doc_comment_end_at_position(&self, position: usize) -> Option<(usize, bool)> {
        let text = self.text();
        let line_start = self.line_start(position);
        let line_end = self.line_end(position);
        if !trim_right_single_line_whitespace(&text[line_start..position]).ends_with("/**") {
            return None;
        }
        Some(match jsdoc_snippet_suffix_end(&text[position..line_end]) {
            Some((end, has_closing)) => (position + end, has_closing),
            None => (position, false),
        })
    }

    /// tsgo `isNonEmptyJSDoc`.
    fn is_non_empty_jsdoc(&self, js_doc: NodeId) -> bool {
        let NodeData::JSDoc(data) = self.data(js_doc) else {
            return false;
        };
        data.comment.is_some() || !self.nodes(data.tags).is_empty()
    }

    /// tsgo `hasJSDocTags`: whether the last JSDoc comment has tags.
    fn has_jsdoc_tags(&self, node: NodeId) -> bool {
        self.last_js_doc(node).is_some_and(|last| {
            matches!(self.data(last), NodeData::JSDoc(data) if !self.nodes(data.tags).is_empty())
        })
    }

    /// tsgo `getIndentationStringAtPosition`.
    fn indentation_string_at_position(&self, position: usize) -> &'a str {
        let text = self.text();
        let rest = &text[self.line_start(position)..position];
        let end = rest
            .char_indices()
            .find(|&(_, ch)| !is_white_space_single_line(ch))
            .map_or(rest.len(), |(index, _)| index);
        &rest[..end]
    }

    /// tsgo `parameterDocComments`.
    fn parameter_doc_comments(
        &self,
        parameters: &[NodeId],
        indentation: &str,
        new_line: &str,
    ) -> String {
        let is_js = NodeFlags::from_bits(self.source.arena.node(self.source.root).flags)
            .contains(NodeFlags::JAVA_SCRIPT_FILE);
        let mut text = String::new();
        for (index, &parameter) in parameters.iter().enumerate() {
            let NodeData::Parameter(data) = self.data(parameter) else {
                continue;
            };
            let name = self
                .identifier_text(data.name)
                .map_or_else(|| format!("param{index}"), str::to_owned);
            let parameter_type = match (is_js, data.dot_dot_dot_token.is_some()) {
                (false, _) => "",
                (true, true) => "{...any} ",
                (true, false) => "{any} ",
            };
            text.push_str(&format!(
                "{indentation} * @param {parameter_type}{name}{new_line}"
            ));
        }
        text
    }

    /// tsgo `getCommentOwnerInfo`: the declaration a doc comment at the
    /// token documents.
    fn comment_owner_info(
        &self,
        token: Touching,
        generate_return: bool,
    ) -> Option<CommentOwnerInfo> {
        let mut node = Some(match token {
            Touching::Node(node) => node,
            Touching::Token { parent, .. } => parent,
        });
        while let Some(current) = node {
            let (info, quit) = self.comment_owner_info_worker(Some(current), generate_return);
            if info.is_some() || quit {
                return info;
            }
            node = self.parent(current);
        }
        None
    }

    /// The owner info of a function-like host.
    fn function_owner(
        &self,
        owner: NodeId,
        host: NodeId,
        generate_return: bool,
    ) -> CommentOwnerInfo {
        CommentOwnerInfo {
            comment_owner: owner,
            parameters: parameters_in(self.source, host),
            has_return: self.has_return(host, generate_return),
        }
    }

    /// tsgo `getCommentOwnerInfoWorker`: the owner info, and whether the
    /// search stops.
    fn comment_owner_info_worker(
        &self,
        owner: Option<NodeId>,
        generate_return: bool,
    ) -> (Option<CommentOwnerInfo>, bool) {
        let Some(owner) = owner else {
            return (None, false);
        };
        let plain = CommentOwnerInfo {
            comment_owner: owner,
            parameters: Vec::new(),
            has_return: false,
        };
        match self.data(owner) {
            NodeData::FunctionDeclaration(_)
            | NodeData::FunctionExpression(_)
            | NodeData::MethodDeclaration(_)
            | NodeData::Constructor(_)
            | NodeData::MethodSignature(_)
            | NodeData::ArrowFunction(_) => (
                Some(self.function_owner(owner, owner, generate_return)),
                false,
            ),
            NodeData::PropertyAssignment(data) => {
                self.comment_owner_info_worker(data.initializer, generate_return)
            }
            NodeData::ClassDeclaration(_)
            | NodeData::InterfaceDeclaration(_)
            | NodeData::EnumDeclaration(_)
            | NodeData::EnumMember(_)
            | NodeData::TypeAliasDeclaration(_) => (Some(plain), false),
            NodeData::PropertySignature(data) => match data.r#type {
                Some(type_node) if self.kind(type_node) == SyntaxKind::FunctionType => (
                    Some(self.function_owner(owner, type_node, generate_return)),
                    false,
                ),
                _ => (Some(plain), false),
            },
            NodeData::VariableStatement(data) => {
                let declarations = match data.declaration_list.map(|list| self.data(list)) {
                    Some(NodeData::VariableDeclarationList(list)) => self.nodes(list.declarations),
                    _ => &[],
                };
                if let [declaration] = declarations {
                    let initializer = match self.data(*declaration) {
                        NodeData::VariableDeclaration(declaration) => declaration.initializer,
                        _ => None,
                    };
                    if let Some(host) = initializer
                        .and_then(|initializer| self.right_hand_side_of_assignment(initializer))
                    {
                        return (
                            Some(self.function_owner(owner, host, generate_return)),
                            false,
                        );
                    }
                }
                (Some(plain), false)
            }
            NodeData::SourceFile(_) => (None, true),
            NodeData::ModuleDeclaration(_) => {
                if self
                    .parent(owner)
                    .is_some_and(|parent| self.kind(parent) == SyntaxKind::ModuleDeclaration)
                {
                    return (None, false);
                }
                (Some(plain), false)
            }
            NodeData::ExpressionStatement(data) => {
                self.comment_owner_info_worker(data.expression, generate_return)
            }
            NodeData::BinaryExpression(data) => {
                if tsc_binder::get_assignment_declaration_kind(self.source, owner)
                    == tsc_binder::AssignmentDeclarationKind::None
                {
                    return (None, true);
                }
                match data.right {
                    Some(right) if is_function_like_kind(self.kind(right)) => (
                        Some(self.function_owner(owner, right, generate_return)),
                        false,
                    ),
                    _ => (Some(plain), false),
                }
            }
            NodeData::PropertyDeclaration(data) => match data.initializer {
                Some(initializer)
                    if matches!(
                        self.kind(initializer),
                        SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
                    ) =>
                {
                    (
                        Some(self.function_owner(owner, initializer, generate_return)),
                        false,
                    )
                }
                _ => (None, false),
            },
            _ => (None, false),
        }
    }

    /// tsgo `hasReturn`: under the preference, a function type, an arrow
    /// function with an expression body, or a body with a `return`.
    fn has_return(&self, node: NodeId, generate_return: bool) -> bool {
        if !generate_return {
            return false;
        }
        let body = match self.data(node) {
            NodeData::FunctionType(_) => return true,
            NodeData::ArrowFunction(data) => {
                if data
                    .body
                    .is_some_and(|body| self.kind(body) != SyntaxKind::Block)
                {
                    return true;
                }
                data.body
            }
            NodeData::FunctionDeclaration(data) => data.body,
            NodeData::FunctionExpression(data) => data.body,
            NodeData::MethodDeclaration(data) => data.body,
            NodeData::Constructor(data) => data.body,
            NodeData::GetAccessor(data) => data.body,
            NodeData::SetAccessor(data) => data.body,
            _ => None,
        };
        body.is_some_and(|body| {
            self.kind(body) == SyntaxKind::Block && self.has_return_statement(body)
        })
    }

    /// tsgo `ast.ForEachReturnStatement` finding one: a `return` of the
    /// statements, not of a nested function.
    fn has_return_statement(&self, node: NodeId) -> bool {
        node_util::for_each_return_statement(self.source, node, &mut |_| true)
    }

    /// tsgo `getRightHandSideOfAssignment`: a function, or a class's
    /// constructor.
    fn right_hand_side_of_assignment(&self, mut node: NodeId) -> Option<NodeId> {
        while let NodeData::ParenthesizedExpression(data) = self.data(node) {
            node = data.expression?;
        }
        match self.data(node) {
            NodeData::FunctionExpression(_) | NodeData::ArrowFunction(_) => Some(node),
            NodeData::ClassExpression(data) => self
                .nodes(data.members)
                .iter()
                .copied()
                .find(|&member| self.kind(member) == SyntaxKind::Constructor),
            _ => None,
        }
    }
}

/// tsgo `commentOwnerInfo`.
struct CommentOwnerInfo {
    comment_owner: NodeId,
    parameters: Vec<NodeId>,
    has_return: bool,
}

/// tsgo `ast.IsJSDocTag`.
fn is_jsdoc_tag_kind(kind: SyntaxKind) -> bool {
    (SyntaxKind::FirstJSDocTagNode..=SyntaxKind::LastJSDocTagNode).contains(&kind)
}

/// tsgo `GetNewLineCharacter` of the `newLine` option.
/// tsgo `templateToSnippet`.
fn template_to_snippet(template: &str, new_line: &str) -> String {
    if template == "/** */" {
        return format!("/**{new_line} * $0{new_line} */");
    }
    let mut snippet_index = 1;
    let template = super::entries::escape_snippet_text(template);
    let template = strip_jsdoc_template_indentation(&template, new_line);
    transform_jsdoc_template_lines(&template, new_line, &mut snippet_index)
}

/// tsgo `stripJSDocTemplateIndentation`.
fn strip_jsdoc_template_indentation(template: &str, new_line: &str) -> String {
    template
        .split(new_line)
        .map(|line| {
            let trimmed = line.trim_start_matches([' ', '\t']);
            if trimmed.starts_with('/') {
                trimmed.to_owned()
            } else if trimmed.starts_with('*') {
                format!(" {trimmed}")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(new_line)
}

/// tsgo `transformJSDocTemplateLines`.
fn transform_jsdoc_template_lines(
    template: &str,
    new_line: &str,
    snippet_index: &mut usize,
) -> String {
    let lines = template.split(new_line).collect::<Vec<_>>();
    let mut result = Vec::with_capacity(lines.len());
    for (index, &line) in lines.iter().enumerate() {
        if index > 0 && lines[index - 1].starts_with("/**") && line_has_only_jsdoc_asterisk(line) {
            result.push(format!("{line}$0"));
            continue;
        }
        if let Some(transformed) = transform_jsdoc_param_line(line, snippet_index) {
            result.push(transformed);
            continue;
        }
        if let Some(transformed) = transform_jsdoc_returns_line(line, snippet_index) {
            result.push(transformed);
            continue;
        }
        result.push(line.to_owned());
    }
    result.join(new_line)
}

/// tsgo `lineHasOnlyJSDocAsterisk`.
fn line_has_only_jsdoc_asterisk(line: &str) -> bool {
    let line = line.trim_start_matches([' ', '\t']);
    line.strip_prefix('*').is_some_and(is_only_spaces_or_tabs)
}

/// tsgo `transformJSDocParamLine`.
fn transform_jsdoc_param_line(line: &str, snippet_index: &mut usize) -> Option<String> {
    let (prefix, rest) = match line.strip_prefix(' ') {
        Some(rest) => (" ", rest),
        None => ("", line),
    };
    let rest = rest.strip_prefix("* @param")?;
    if !starts_with_single_line_whitespace(rest) {
        return None;
    }
    let mut rest = rest.trim_start_matches([' ', '\t']);
    let mut type_text = String::new();
    if rest.starts_with('{') {
        let close_brace = rest.find('}')?;
        type_text = format!(" {}", &rest[..=close_brace]);
        rest = &rest[close_brace + 1..];
        if !starts_with_single_line_whitespace(rest) {
            return None;
        }
        rest = rest.trim_start_matches([' ', '\t']);
    }
    let (parameter_name, rest) = scan_non_whitespace(rest)?;
    if !is_only_spaces_or_tabs(rest) {
        return None;
    }
    let mut out = format!("{prefix}* @param ");
    if type_text == " {any}" || type_text == " {*}" {
        out.push_str(&format!("{{${{{snippet_index}:*}}}} "));
        *snippet_index += 1;
    } else if !type_text.is_empty() {
        out.push_str(&format!("{type_text} "));
    }
    out.push_str(&format!("{parameter_name} ${{{snippet_index}}}"));
    *snippet_index += 1;
    Some(out)
}

/// tsgo `transformJSDocReturnsLine`.
fn transform_jsdoc_returns_line(line: &str, snippet_index: &mut usize) -> Option<String> {
    let (prefix, rest) = match line.strip_prefix(' ') {
        Some(rest) => (" ", rest),
        None => ("", line),
    };
    let after = rest.strip_prefix("* @returns")?;
    if !is_only_spaces_or_tabs(after) {
        return None;
    }
    let text = format!("{prefix}* @returns ${{{snippet_index}}}");
    *snippet_index += 1;
    Some(text)
}

/// tsgo `scanNonWhitespace`.
fn scan_non_whitespace(text: &str) -> Option<(&str, &str)> {
    if text.is_empty() {
        return None;
    }
    match text
        .char_indices()
        .find(|&(_, ch)| tsc_syntax::is_whitespace_like(ch))
    {
        Some((0, _)) => None,
        Some((index, _)) => Some((&text[..index], &text[index..])),
        None => Some((text, "")),
    }
}

/// tsgo `isJSDocSnippetPrefix`: `/**` at the end, or a line of `/` and
/// asterisks.
fn is_jsdoc_snippet_prefix(prefix: &str) -> bool {
    let trimmed = trim_right_single_line_whitespace(prefix);
    if trimmed.ends_with("/**") {
        return true;
    }
    let start = skip_single_line_whitespace(prefix, 0);
    let bytes = trimmed.as_bytes();
    if start >= bytes.len() || bytes[start] != b'/' {
        return false;
    }
    if start + 3 > bytes.len() {
        return false;
    }
    bytes[start + 1..].iter().all(|&byte| byte == b'*') && bytes.len() - start >= 3
}

/// tsgo `getJSDocSnippetPrefixStart`.
fn jsdoc_snippet_prefix_start(prefix: &str) -> Option<usize> {
    let trimmed = trim_right_single_line_whitespace(prefix);
    let bytes = trimmed.as_bytes();
    let mut index = bytes.len();
    while index > 0 && bytes[index - 1] == b'*' {
        let i = index - 1;
        if i > 0 && bytes[i - 1] == b'/' {
            return Some(i - 1);
        }
        index -= 1;
    }
    trimmed.ends_with('/').then(|| bytes.len() - 1)
}

/// tsgo `isJSDocSnippetSuffix`: nothing, or asterisks and `/`.
fn is_jsdoc_snippet_suffix(suffix: &str) -> bool {
    let trimmed =
        trim_right_single_line_whitespace(&suffix[skip_single_line_whitespace(suffix, 0)..]);
    if trimmed.is_empty() {
        return true;
    }
    if !trimmed.ends_with('/') {
        return false;
    }
    trimmed.as_bytes()[..trimmed.len() - 1]
        .iter()
        .all(|&byte| byte == b'*')
}

/// tsgo `getJSDocSnippetSuffixEnd`: the end of a `*/` after whitespace
/// and asterisks, and that there is one.
fn jsdoc_snippet_suffix_end(suffix: &str) -> Option<(usize, bool)> {
    let bytes = suffix.as_bytes();
    let mut pos = skip_single_line_whitespace(suffix, 0);
    while pos < bytes.len() && bytes[pos] == b'*' {
        pos += 1;
    }
    (pos < bytes.len() && bytes[pos] == b'/').then_some((pos + 1, true))
}

/// tsgo `trimRightSingleLineWhitespace`.
fn trim_right_single_line_whitespace(text: &str) -> &str {
    text.trim_end_matches(is_white_space_single_line)
}

/// tsgo `skipSingleLineWhitespace`.
fn skip_single_line_whitespace(text: &str, pos: usize) -> usize {
    text[pos..]
        .char_indices()
        .find(|&(_, ch)| !is_white_space_single_line(ch))
        .map_or(text.len(), |(index, _)| pos + index)
}

/// tsgo `skipWhitespace`.
fn skip_whitespace(text: &str, position: usize) -> usize {
    text[position..]
        .char_indices()
        .find(|&(_, ch)| !tsc_syntax::is_whitespace_like(ch))
        .map_or(text.len(), |(index, _)| position + index)
}

/// tsgo `startsWithSingleLineWhitespace`.
fn starts_with_single_line_whitespace(text: &str) -> bool {
    text.chars().next().is_some_and(is_white_space_single_line)
}

/// tsgo `isOnlySpacesOrTabs`.
fn is_only_spaces_or_tabs(text: &str) -> bool {
    text.bytes().all(|byte| byte == b' ' || byte == b'\t')
}
