//! The documentation a hover shows (tsgo `getDocumentationForSymbol` and the
//! comment writers of ls/hover.go, `getJSDocOrTag` of ls/jsdoc.go): a
//! declaration's JSDoc comment, then its tags, with `{@link}`s as markdown
//! links to the linked declarations.

use tsc_binder::node_util;
use tsc_checker::state::{CheckResult, CheckerState};
use tsc_syntax::{JSDocComment, NodeData, NodeId, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags, SymbolFlags, SymbolId};

use crate::request::Request;
use crate::tree::{for_each_property, Property};
use crate::utilities::{token_start, Touching};

/// One part of a JSDoc comment (tsgo's comment list): text, or a link.
enum CommentPart<'a> {
    Text(&'a str),
    Link {
        kind: SyntaxKind,
        name: Option<NodeId>,
        text: &'a str,
    },
}

/// A JSDoc tag's tag name.
pub(crate) fn tag_name(state: &CheckerState<'_>, tag: NodeId) -> Option<NodeId> {
    let mut name = None;
    let mut first = true;
    for_each_property(state.binder.node_record(tag), |property| {
        if first {
            first = false;
            if let Property::Node(node) = property {
                name = node;
            }
        }
    });
    name
}

/// A JSDoc comment's or tag's comment.
fn comment_of<'a>(state: &CheckerState<'a>, owner: NodeId) -> Option<&'a JSDocComment> {
    let node = state.binder.node_record(owner);
    if let NodeData::JSDoc(js_doc) = &node.data {
        return js_doc.comment.as_ref();
    }
    let mut comment = None;
    for_each_property(node, |property| {
        if let Property::Comment(value) = property {
            comment = value.as_ref();
        }
    });
    comment
}

/// The parts of a comment (tsgo `Comments()`).
fn comment_parts<'a>(
    state: &CheckerState<'a>,
    comment: Option<&'a JSDocComment>,
) -> Vec<CommentPart<'a>> {
    match comment {
        None => Vec::new(),
        Some(JSDocComment::Text { text, .. }) => vec![CommentPart::Text(text)],
        Some(JSDocComment::Nodes(list)) => state
            .binder
            .node_array(*list)
            .nodes
            .iter()
            .filter_map(|&part| {
                let record = state.binder.node_record(part);
                match &record.data {
                    NodeData::JSDocText(text) => Some(CommentPart::Text(&text.text)),
                    NodeData::JSDocLink(link) => Some(CommentPart::Link {
                        kind: record.kind,
                        name: link.name,
                        text: &link.text,
                    }),
                    NodeData::JSDocLinkCode(link) => Some(CommentPart::Link {
                        kind: record.kind,
                        name: link.name,
                        text: &link.text,
                    }),
                    NodeData::JSDocLinkPlain(link) => Some(CommentPart::Link {
                        kind: record.kind,
                        name: link.name,
                        text: &link.text,
                    }),
                    _ => None,
                }
            })
            .collect(),
    }
}

/// tsgo `scanner.GetTextOfJSDocComment`: a comment's parts as text, a link
/// as `{@link name text}`.
fn text_of_comment(state: &CheckerState<'_>, comment: Option<&JSDocComment>) -> String {
    let mut text = String::new();
    for part in comment_parts(state, comment) {
        match part {
            CommentPart::Text(part) => text.push_str(part),
            CommentPart::Link {
                kind,
                name,
                text: link_text,
            } => {
                let tag = match kind {
                    SyntaxKind::JSDocLinkCode => "linkcode",
                    SyntaxKind::JSDocLinkPlain => "linkplain",
                    _ => "link",
                };
                text.push_str("{@");
                text.push_str(tag);
                text.push(' ');
                if let Some(name) = name {
                    text.push_str(&source_text_of(state, name));
                }
                text.push_str(link_text);
                text.push('}');
            }
        }
    }
    text
}

/// tsgo `scanner.GetTextOfNode`: a node's source text without its leading
/// trivia.
fn source_text_of(state: &CheckerState<'_>, node: NodeId) -> String {
    let source = state.binder.source_of_node(node);
    let start = token_start(source, node);
    let end = state.binder.node_record(node).end as usize;
    source.text()[start.min(end)..end].to_owned()
}

/// tsgo `getEntityNameString`.
fn entity_name_string(state: &CheckerState<'_>, name: NodeId) -> String {
    let mut text = String::new();
    write_entity_name_parts(state, &mut text, name);
    text
}

fn write_entity_name_parts(state: &CheckerState<'_>, text: &mut String, node: NodeId) {
    let source = state.binder.source_of_node(node);
    match &state.binder.node_record(node).data {
        NodeData::Identifier(_) => {
            if let Some(name) = node_util::id_text(source, node) {
                text.push_str(name);
            }
        }
        NodeData::QualifiedName(name) => {
            if let Some(left) = name.left {
                write_entity_name_parts(state, text, left);
            }
            text.push('.');
            if let Some(right) = name.right {
                write_entity_name_parts(state, text, right);
            }
        }
        // tsgo parses a link's `A#b` as a qualified name.
        NodeData::JSDocMemberName(name) => {
            if let Some(left) = name.left {
                write_entity_name_parts(state, text, left);
            }
            text.push('.');
            if let Some(right) = name.right {
                write_entity_name_parts(state, text, right);
            }
        }
        NodeData::PropertyAccessExpression(access) => {
            if let Some(expression) = access.expression {
                write_entity_name_parts(state, text, expression);
            }
            text.push('.');
            if let Some(name) = access.name {
                write_entity_name_parts(state, text, name);
            }
        }
        NodeData::ParenthesizedExpression(data) => {
            if let Some(expression) = data.expression {
                write_entity_name_parts(state, text, expression);
            }
        }
        NodeData::ExpressionWithTypeArguments(data) => {
            if let Some(expression) = data.expression {
                write_entity_name_parts(state, text, expression);
            }
        }
        NodeData::JSDocNameReference(reference) => {
            if let Some(name) = reference.name {
                write_entity_name_parts(state, text, name);
            }
        }
        _ => {}
    }
}

/// tsgo `writeQuotedString`.
fn write_quoted_string(text: &mut String, value: &str, quote: bool) {
    if quote && !value.contains('`') {
        text.push('`');
        text.push_str(value);
        text.push('`');
    } else {
        text.push_str(value);
    }
}

/// tsgo `writeMarkdownLink`.
fn write_markdown_link(text: &mut String, link_text: &str, uri: &str, quote: bool) {
    text.push('[');
    write_quoted_string(text, link_text, quote);
    text.push_str("](");
    text.push_str(uri);
    text.push(')');
}

/// tsgo `trimCommentPrefix`.
fn trim_comment_prefix(text: &str) -> &str {
    let text = text.trim_start_matches(' ');
    text.strip_prefix('|')
        .unwrap_or(text)
        .trim_start_matches(' ')
}

/// tsgo `writeOptionalEntityName`.
fn write_optional_entity_name(state: &CheckerState<'_>, text: &mut String, name: Option<NodeId>) {
    if let Some(name) = name {
        text.push(' ');
        write_quoted_string(text, &entity_name_string(state, name), true);
    }
}

impl Request<'_, '_> {
    /// tsgo `getDocumentationForSymbol`: the call signature's, the roots',
    /// the declaration's, then the alias target's documentation, the first
    /// there is.
    pub(crate) fn documentation_for_symbol(
        &mut self,
        symbol: Option<SymbolId>,
        node: Touching,
        declaration: Option<NodeId>,
        markdown: bool,
        comment_only: bool,
    ) -> CheckResult<String> {
        let documentation =
            self.documentation_from_signature(symbol, node, markdown, comment_only)?;
        if !documentation.is_empty() {
            return Ok(documentation);
        }
        let documentation =
            self.documentation_from_root_symbols(symbol, node, markdown, comment_only)?;
        if !documentation.is_empty() {
            return Ok(documentation);
        }
        let documentation =
            self.documentation_from_declaration(symbol, declaration, node, markdown, comment_only)?;
        if !documentation.is_empty() {
            return Ok(documentation);
        }
        self.documentation_from_alias(symbol, node, markdown, comment_only)
    }

    /// tsgo `documentationFromSignature`: a call or construct signature's
    /// declaration's documentation, for the call the node names.
    fn documentation_from_signature(
        &mut self,
        symbol: Option<SymbolId>,
        node: Touching,
        markdown: bool,
        comment_only: bool,
    ) -> CheckResult<String> {
        let Some(call) = self.call_or_new_expression(node) else {
            return Ok(String::new());
        };
        let signature =
            self.state
                .get_resolved_signature(call, None, tsc_types::CheckMode::NORMAL)?;
        let Some(declaration) = self.state.signature_of(signature).declaration else {
            return Ok(String::new());
        };
        if matches!(
            self.node_kind(declaration),
            SyntaxKind::CallSignature | SyntaxKind::ConstructSignature
        ) {
            return self.documentation_from_declaration(
                symbol,
                Some(declaration),
                node,
                markdown,
                comment_only,
            );
        }
        Ok(String::new())
    }

    /// tsgo `documentationFromAlias`: an alias target's (or its export
    /// symbol's) declaration's documentation.
    fn documentation_from_alias(
        &mut self,
        symbol: Option<SymbolId>,
        node: Touching,
        markdown: bool,
        comment_only: bool,
    ) -> CheckResult<String> {
        let Some(symbol) = symbol else {
            return Ok(String::new());
        };
        if !self
            .state
            .binder
            .symbol(symbol)
            .flags
            .intersects(SymbolFlags::ALIAS)
        {
            return Ok(String::new());
        }
        let aliased = self.state.get_aliased_symbol(symbol)?;
        if aliased == self.state.unknown_symbol {
            return Ok(String::new());
        }
        let mut candidates = vec![aliased];
        if let Some(export) = self.state.binder.symbol(aliased).export_symbol {
            candidates.push(export);
        }
        for candidate in candidates {
            let declaration = self
                .state
                .binder
                .symbol(candidate)
                .value_declaration
                .or_else(|| self.declarations_of(candidate).first().copied());
            let Some(declaration) = declaration else {
                continue;
            };
            let documentation = self.documentation_from_declaration(
                Some(candidate),
                Some(declaration),
                node,
                markdown,
                comment_only,
            )?;
            if !documentation.is_empty() {
                return Ok(documentation);
            }
        }
        Ok(String::new())
    }

    /// tsgo `documentationFromRootSymbols`: each root's declarations'
    /// documentation, once, for a symbol with several roots.
    fn documentation_from_root_symbols(
        &mut self,
        symbol: Option<SymbolId>,
        node: Touching,
        markdown: bool,
        comment_only: bool,
    ) -> CheckResult<String> {
        let Some(symbol) = symbol else {
            return Ok(String::new());
        };
        let roots = self.state.get_root_symbols(symbol)?;
        if roots.len() <= 1 {
            return Ok(String::new());
        }
        let mut docs: Vec<String> = Vec::new();
        for root in roots {
            let mut declarations = self.declarations_of(root);
            if declarations.is_empty() {
                if let Some(declaration) = self.state.binder.symbol(root).value_declaration {
                    declarations.push(declaration);
                }
            }
            for declaration in declarations {
                let documentation = self.documentation_from_declaration(
                    Some(root),
                    Some(declaration),
                    node,
                    markdown,
                    comment_only,
                )?;
                if !documentation.is_empty() && !docs.contains(&documentation) {
                    docs.push(documentation);
                }
            }
        }
        Ok(docs.join("\n"))
    }

    /// tsgo `getDocumentationFromDeclaration`: the comment of the
    /// declaration's JSDoc (or its matching tag), then the tags.
    pub(crate) fn documentation_from_declaration(
        &mut self,
        _symbol: Option<SymbolId>,
        declaration: Option<NodeId>,
        _location: Touching,
        markdown: bool,
        comment_only: bool,
    ) -> CheckResult<String> {
        let Some(declaration) = declaration else {
            return Ok(String::new());
        };
        let mut text = String::new();
        let mut seen = Vec::new();
        let Some(js_doc) = self.js_doc_or_tag(declaration, &mut seen)? else {
            return Ok(text);
        };
        if self.contains_typedef_tag(js_doc) && !self.is_typedef_declaration(declaration) {
            return Ok(text);
        }
        let comment = comment_of(self.state, js_doc);
        self.write_comments(&mut text, comment, markdown)?;
        if self.node_kind(js_doc) != SyntaxKind::JSDoc || comment_only {
            return Ok(text);
        }
        let NodeData::JSDoc(data) = &self.state.binder.node_record(js_doc).data else {
            return Ok(text);
        };
        let tags = data
            .tags
            .map(|tags| self.state.binder.node_array(tags).nodes.to_vec())
            .unwrap_or_default();
        for tag in tags {
            let kind = self.node_kind(tag);
            if matches!(
                kind,
                SyntaxKind::JSDocTypeTag
                    | SyntaxKind::JSDocTypedefTag
                    | SyntaxKind::JSDocCallbackTag
            ) {
                continue;
            }
            let name = tag_name(self.state, tag)
                .and_then(|name| {
                    node_util::id_text(self.state.binder.source_of_node(name), name)
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            text.push_str("\n\n");
            if markdown {
                text.push_str("*@");
                text.push_str(&name);
                text.push('*');
            } else {
                text.push('@');
                text.push_str(&name);
            }
            match &self.state.binder.node_record(tag).data {
                NodeData::JSDocParameterTag(data) => {
                    write_optional_entity_name(self.state, &mut text, data.name)
                }
                NodeData::JSDocPropertyTag(data) => {
                    write_optional_entity_name(self.state, &mut text, data.name)
                }
                NodeData::JSDocAugmentsTag(data) => {
                    write_optional_entity_name(self.state, &mut text, data.class)
                }
                NodeData::JSDocTemplateTag(data) => {
                    let parameters = data
                        .type_parameters
                        .map(|parameters| self.state.binder.node_array(parameters).nodes.to_vec())
                        .unwrap_or_default();
                    for (index, parameter) in parameters.into_iter().enumerate() {
                        if index != 0 {
                            text.push(',');
                        }
                        write_optional_entity_name(self.state, &mut text, self.name_of(parameter));
                    }
                }
                _ => {}
            }
            let tag_comment = comment_of(self.state, tag);
            let parts = comment_parts(self.state, tag_comment);
            match &self.state.binder.node_record(tag).data {
                NodeData::JSDocTag(_) if name == "example" => {
                    let mut comment = text_of_comment(self.state, tag_comment);
                    if let Some(rest) = comment.strip_prefix("<caption>") {
                        if let Some(caption_end) = rest.find("</caption>") {
                            if caption_end > 0 || comment.starts_with("<caption></caption>") {
                                text.push_str(" — ");
                                text.push_str(&rest[..caption_end]);
                                let mut after = rest[caption_end + "</caption>".len()..].to_owned();
                                // Leading blank lines go.
                                loop {
                                    let first = after.trim_start_matches([' ', '\t']);
                                    let second = first.trim_start_matches(['\r', '\n']);
                                    if first.len() == second.len() {
                                        break;
                                    }
                                    after = second.to_owned();
                                }
                                comment = after;
                            }
                        }
                    }
                    text.push('\n');
                    if comment.len() > 6
                        && comment.starts_with("```")
                        && comment.ends_with("```")
                        && comment.contains('\n')
                    {
                        text.push_str(&comment);
                        text.push('\n');
                    } else {
                        crate::hover::write_code(&mut text, "tsx", &comment);
                    }
                }
                NodeData::JSDocSeeTag(data) if data.name.is_some() => {
                    text.push_str(" — ");
                    let reference = data.name.expect("tested");
                    let name = match &self.state.binder.node_record(reference).data {
                        NodeData::JSDocNameReference(reference) => reference.name,
                        _ => Some(reference),
                    };
                    if let Some(name) = name {
                        self.write_name_link(&mut text, name, "", false, markdown)?;
                    }
                    if !parts.is_empty() {
                        text.push(' ');
                        self.write_comments(&mut text, tag_comment, markdown)?;
                    }
                }
                NodeData::JSDocThrowsTag(data) if data.type_expression.is_some() => {
                    text.push_str(" — ");
                    text.push_str(&source_text_of(
                        self.state,
                        data.type_expression.expect("tested"),
                    ));
                    if !parts.is_empty() {
                        text.push(' ');
                        self.write_comments(&mut text, tag_comment, markdown)?;
                    }
                }
                _ if !parts.is_empty() => {
                    text.push(' ');
                    let dashed = matches!(parts.first(), Some(CommentPart::Text(first)) if first.starts_with('-'));
                    if !dashed {
                        text.push_str("— ");
                    }
                    self.write_comments(&mut text, tag_comment, markdown)?;
                }
                _ => {}
            }
        }
        Ok(text)
    }

    /// tsgo `writeComments`.
    fn write_comments(
        &mut self,
        text: &mut String,
        comment: Option<&JSDocComment>,
        markdown: bool,
    ) -> CheckResult<()> {
        let parts = comment_parts(self.state, comment);
        for part in parts {
            match part {
                CommentPart::Text(part) => text.push_str(part),
                CommentPart::Link {
                    kind,
                    name,
                    text: link_text,
                } => {
                    let link_text = link_text.to_owned();
                    self.write_js_doc_link(
                        text,
                        name,
                        &link_text,
                        kind == SyntaxKind::JSDocLinkCode,
                        markdown,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// tsgo `writeJSDocLink`.
    fn write_js_doc_link(
        &mut self,
        text: &mut String,
        name: Option<NodeId>,
        link_text: &str,
        quote: bool,
        markdown: bool,
    ) -> CheckResult<()> {
        let trimmed = link_text.trim_matches(' ');
        let Some(name) = name else {
            write_quoted_string(text, trimmed, quote && markdown);
            return Ok(());
        };
        let source = self.state.binder.source_of_node(name);
        let name_text = node_util::id_text(source, name)
            .filter(|_| self.node_kind(name) == SyntaxKind::Identifier);
        if let Some(scheme) = name_text.filter(|scheme| matches!(*scheme, "http" | "https")) {
            if trimmed.starts_with("://") {
                let full = format!("{scheme}{trimmed}");
                let (uri, mut shown) = match full.find([' ', '|']) {
                    Some(position) => {
                        let shown = trim_comment_prefix(&full[position..]).to_owned();
                        (full[..position].to_owned(), shown)
                    }
                    None => (full.clone(), full.clone()),
                };
                if shown.is_empty() {
                    shown = uri.clone();
                }
                if markdown {
                    write_markdown_link(text, &shown, &uri, quote);
                } else {
                    write_quoted_string(text, &shown, false);
                    if shown != uri {
                        text.push_str(" (");
                        text.push_str(&uri);
                        text.push(')');
                    }
                }
                return Ok(());
            }
        }
        self.write_name_link(text, name, trimmed, quote, markdown)
    }

    /// tsgo `writeNameLink`: a link to the name's first declaration.
    fn write_name_link(
        &mut self,
        text: &mut String,
        name: NodeId,
        link_text: &str,
        quote: bool,
        markdown: bool,
    ) -> CheckResult<()> {
        let declarations = self.declarations_from_location(Touching::Node(name))?;
        if let Some(&declaration) = declarations.first() {
            let source = self.state.binder.source_of_node(declaration);
            let target =
                node_util::get_name_of_declaration(source, declaration).unwrap_or(declaration);
            let range = self.range_from_node(target);
            let location = self
                .source_maps
                .location(self.state.binder.source_of_node(target), range);
            let prefix = if link_text.starts_with("()") { 2 } else { 0 };
            let mut shown = trim_comment_prefix(&link_text[prefix..]).to_owned();
            if shown.is_empty() {
                shown = format!(
                    "{}{}",
                    entity_name_string(self.state, name),
                    &link_text[..prefix]
                );
            }
            if markdown {
                let uri = format!(
                    "{}#{},{}-{},{}",
                    location.uri.as_str(),
                    location.range.start.line + 1,
                    location.range.start.character + 1,
                    location.range.end.line + 1,
                    location.range.end.character + 1
                );
                write_markdown_link(text, &shown, &uri, quote);
            } else {
                write_quoted_string(text, &shown, false);
            }
            return Ok(());
        }
        let separator = if link_text.is_empty() { "" } else { " " };
        write_quoted_string(
            text,
            &format!(
                "{}{separator}{link_text}",
                entity_name_string(self.state, name)
            ),
            quote && markdown,
        );
        Ok(())
    }

    /// tsgo `getJSDocOrTag`: the node's JSDoc, a parameter's or type
    /// parameter's matching tag, or the JSDoc of what the node takes it
    /// from (the variable statement, the overload, the base member).
    fn js_doc_or_tag(
        &mut self,
        node: NodeId,
        seen: &mut Vec<SymbolId>,
    ) -> CheckResult<Option<NodeId>> {
        if let Some(js_doc) = self.last_js_doc(node) {
            return Ok(Some(js_doc));
        }
        let record = self.state.binder.node_record(node);
        let parent = record.parent;
        match &record.data {
            NodeData::Parameter(parameter) => {
                let Some(name) = parameter.name else {
                    return Ok(None);
                };
                if matches!(
                    self.node_kind(name),
                    SyntaxKind::ObjectBindingPattern | SyntaxKind::ArrayBindingPattern
                ) {
                    return self.js_doc_parameter_tag_by_position(node);
                }
                let name_text = self.identifier_text(name);
                return self.matching_js_doc_tag(
                    parent,
                    &name_text,
                    SyntaxKind::JSDocParameterTag,
                    seen,
                );
            }
            NodeData::TypeParameter(parameter) => {
                let name_text = parameter
                    .name
                    .map(|name| self.identifier_text(name))
                    .unwrap_or_default();
                return self.matching_js_doc_tag(
                    parent,
                    &name_text,
                    SyntaxKind::JSDocTemplateTag,
                    seen,
                );
            }
            NodeData::VariableDeclaration(_) => {
                if let Some(list) = parent {
                    if let NodeData::VariableDeclarationList(data) =
                        &self.state.binder.node_record(list).data
                    {
                        let first = data.declarations.and_then(|list| {
                            self.state.binder.node_array(list).nodes.first().copied()
                        });
                        if first == Some(node) {
                            let statement = self.state.binder.node_record(list).parent;
                            if let Some(statement) = statement {
                                return self.js_doc_or_tag(statement, seen);
                            }
                            return Ok(None);
                        }
                    }
                }
            }
            NodeData::FunctionExpression(_)
            | NodeData::ArrowFunction(_)
            | NodeData::ClassExpression(_) => {
                if let Some(parent) = parent {
                    let initializer = match &self.state.binder.node_record(parent).data {
                        NodeData::VariableDeclaration(data) => data.initializer,
                        NodeData::PropertyDeclaration(data) => data.initializer,
                        NodeData::PropertyAssignment(data) => data.initializer,
                        _ => None,
                    };
                    if initializer == Some(node) {
                        return self.js_doc_or_tag(parent, seen);
                    }
                }
            }
            NodeData::BindingElement(element) => {
                let pattern = parent
                    .filter(|&pattern| self.node_kind(pattern) == SyntaxKind::ObjectBindingPattern);
                if let Some(pattern) = pattern {
                    let name = element.property_name.or(element.name);
                    if let Some(name) =
                        name.filter(|&name| self.node_kind(name) == SyntaxKind::Identifier)
                    {
                        let object_type = self.state.get_type_at_location(pattern)?;
                        let escaped = self.state.binder.node_record(name);
                        let _ = escaped;
                        if let Some(property_name) = self.state.try_get_text_of_property_name(name)
                        {
                            if let Some(property) = self
                                .state
                                .get_property_of_type_full(object_type, property_name)?
                            {
                                for declaration in self.declarations_of(property) {
                                    if let Some(js_doc) = self.last_js_doc(declaration) {
                                        return Ok(Some(js_doc));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        let Some(symbol) = self.state.binder.node_symbol(node) else {
            return Ok(None);
        };
        let Some(parent) = parent else {
            return Ok(None);
        };
        let kind = self.node_kind(node);
        if matches!(
            kind,
            SyntaxKind::FunctionDeclaration
                | SyntaxKind::MethodDeclaration
                | SyntaxKind::MethodSignature
                | SyntaxKind::Constructor
                | SyntaxKind::ConstructSignature
        ) {
            let first_signature = self
                .declarations_of(symbol)
                .into_iter()
                .find(|&declaration| {
                    crate::utilities::is_function_like_kind(self.node_kind(declaration))
                });
            if let Some(first) = first_signature.filter(|&first| first != node) {
                if let Some(js_doc) = self.js_doc_or_tag(first, seen)? {
                    return Ok(Some(js_doc));
                }
            }
        }
        if matches!(
            self.node_kind(parent),
            SyntaxKind::ClassDeclaration
                | SyntaxKind::ClassExpression
                | SyntaxKind::InterfaceDeclaration
        ) {
            let source = self.state.binder.source_of_node(node);
            let is_static = node_util::has_syntactic_modifier(source, node, ModifierFlags::STATIC);
            let Some(class_symbol) = self.state.binder.node_symbol(parent) else {
                return Ok(None);
            };
            let class_type = self.state.get_declared_type_of_symbol(class_symbol)?;
            let name = self.state.binder.symbol(symbol).escaped_name;
            let bases = if is_static {
                let constructor = self
                    .state
                    .get_base_constructor_type_of_class_public(class_type)?;
                vec![self.state.get_apparent_type(constructor)?]
            } else {
                self.state.get_base_types(class_type)?
            };
            for base in bases {
                if let Some(property) = self.state.get_property_of_type_full(base, name)? {
                    if let Some(declaration) = self.state.binder.symbol(property).value_declaration
                    {
                        if !seen.contains(&property) {
                            seen.push(property);
                            if let Some(js_doc) = self.js_doc_or_tag(declaration, seen)? {
                                return Ok(Some(js_doc));
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }

    /// tsgo `getMatchingJSDocTag`: the tag of `kind` naming `name` in the
    /// node's JSDoc.
    fn matching_js_doc_tag(
        &mut self,
        node: Option<NodeId>,
        name: &str,
        kind: SyntaxKind,
        seen: &mut Vec<SymbolId>,
    ) -> CheckResult<Option<NodeId>> {
        let Some(node) = node else {
            return Ok(None);
        };
        let Some(js_doc) = self.js_doc_or_tag(node, seen)? else {
            return Ok(None);
        };
        let NodeData::JSDoc(data) = &self.state.binder.node_record(js_doc).data else {
            return Ok(None);
        };
        let tags = data
            .tags
            .map(|tags| self.state.binder.node_array(tags).nodes.to_vec())
            .unwrap_or_default();
        for tag in tags {
            if self.node_kind(tag) != kind {
                continue;
            }
            let matches = match &self.state.binder.node_record(tag).data {
                NodeData::JSDocParameterTag(data) => data
                    .name
                    .is_some_and(|tag_name| self.is_identifier_named(tag_name, name)),
                NodeData::JSDocTemplateTag(data) => data
                    .type_parameters
                    .map(|parameters| self.state.binder.node_array(parameters).nodes.to_vec())
                    .unwrap_or_default()
                    .into_iter()
                    .any(|parameter| {
                        self.name_of(parameter).is_some_and(|parameter_name| {
                            self.is_identifier_named(parameter_name, name)
                        })
                    }),
                _ => false,
            };
            if matches {
                return Ok(Some(tag));
            }
        }
        Ok(None)
    }

    /// tsgo `getJSDocParameterTagByPosition`: the `@param` tag at a binding
    /// pattern parameter's position.
    fn js_doc_parameter_tag_by_position(
        &mut self,
        parameter: NodeId,
    ) -> CheckResult<Option<NodeId>> {
        let Some(owner) = self.state.binder.node_record(parameter).parent else {
            return Ok(None);
        };
        let parameters = crate::utilities::parameters_of(self.state, owner);
        let Some(index) = parameters
            .iter()
            .position(|&candidate| candidate == parameter)
        else {
            return Ok(None);
        };
        let mut seen = Vec::new();
        let Some(js_doc) = self.js_doc_or_tag(owner, &mut seen)? else {
            return Ok(None);
        };
        let NodeData::JSDoc(data) = &self.state.binder.node_record(js_doc).data else {
            return Ok(None);
        };
        let tags = data
            .tags
            .map(|tags| self.state.binder.node_array(tags).nodes.to_vec())
            .unwrap_or_default();
        Ok(tags
            .into_iter()
            .filter(|&tag| self.node_kind(tag) == SyntaxKind::JSDocParameterTag)
            .nth(index))
    }

    /// tsgo `getJSDoc`: the node's last JSDoc comment. tsgo reparses a
    /// typedef or callback tag with a type into a type alias that carries the
    /// tag's whole comment; tsc-rs's declaration is the tag, inside it.
    fn last_js_doc(&self, node: NodeId) -> Option<NodeId> {
        let record = self.state.binder.node_record(node);
        match &record.data {
            NodeData::JSDocTypedefTag(tag) if tag.type_expression.is_some() => {
                return record
                    .parent
                    .filter(|&parent| self.node_kind(parent) == SyntaxKind::JSDoc);
            }
            NodeData::JSDocCallbackTag(tag) if tag.type_expression.is_some() => {
                return record
                    .parent
                    .filter(|&parent| self.node_kind(parent) == SyntaxKind::JSDoc);
            }
            // tsgo reparses a typedef's `@property` tag into a property
            // signature with a JSDoc of the tag's comment.
            NodeData::JSDocPropertyTag(tag)
                if tag.comment.is_some()
                    && record.parent.is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::JSDocTypeLiteral
                    }) =>
            {
                return Some(node);
            }
            // tsgo reparses a JSDoc signature's (`@callback`, `@overload`)
            // `@param` and `@this` tags into parameters with a JSDoc of the
            // tag's comment.
            NodeData::JSDocParameterTag(tag)
                if tag.comment.is_some()
                    && record.parent.is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::JSDocSignature
                    }) =>
            {
                return Some(node);
            }
            NodeData::JSDocThisTag(tag)
                if tag.comment.is_some()
                    && record.parent.is_some_and(|parent| {
                        self.node_kind(parent) == SyntaxKind::JSDocSignature
                    }) =>
            {
                return Some(node);
            }
            _ => {}
        }
        if record.kind == SyntaxKind::TypeParameter {
            return None;
        }
        let list = record.js_doc?;
        self.state.binder.node_array(list).nodes.last().copied()
    }

    /// tsgo `containsTypedefTag`.
    fn contains_typedef_tag(&self, js_doc: NodeId) -> bool {
        let NodeData::JSDoc(data) = &self.state.binder.node_record(js_doc).data else {
            return false;
        };
        data.tags.is_some_and(|tags| {
            self.state.binder.node_array(tags).nodes.iter().any(|&tag| {
                matches!(
                    self.node_kind(tag),
                    SyntaxKind::JSDocTypedefTag | SyntaxKind::JSDocCallbackTag
                )
            })
        })
    }

    /// Whether the declaration is a typedef's (tsgo's reparsed type alias).
    fn is_typedef_declaration(&self, declaration: NodeId) -> bool {
        matches!(
            self.node_kind(declaration),
            SyntaxKind::JSDocTypedefTag | SyntaxKind::JSDocCallbackTag
        ) || NodeFlags::from_bits(self.state.binder.node_record(declaration).flags)
            .contains(NodeFlags::JS_DOC)
    }

    fn identifier_text(&self, node: NodeId) -> String {
        node_util::id_text(self.state.binder.source_of_node(node), node)
            .unwrap_or_default()
            .to_owned()
    }

    fn is_identifier_named(&self, node: NodeId, name: &str) -> bool {
        self.node_kind(node) == SyntaxKind::Identifier && self.identifier_text(node) == name
    }
}
