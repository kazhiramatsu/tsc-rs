//! The JavaScript parts of tsgo's declaration transform
//! (transformers/declarations/transform.go).
//!
//! tsgo's parser rewrites JSDoc into ordinary syntax of the nodes it
//! annotates, and its declaration transform reads that syntax. tsc-rs keeps
//! the parse tree as written; the binder records the same decisions in
//! `tsc_binder::jsdoc_hosted`, which this module and the shared transform
//! read instead.

use rustc_hash::FxHashSet;
use tsc_binder::{get_assignment_declaration_kind, node_util, AssignmentDeclarationKind};
use tsc_diagnostics::JsString;
use tsc_syntax::{JSDocComment, NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, TransformError, TransformNode,
    TransformNodeArray, TransformSourceId, TransformationContext,
};

use super::diagnostics::DiagnosticContext;
use super::state::VisitResult;
use super::subtree::preserve_js_doc;
use super::tracker::{materialize_effects, TrackerAnchor};
use super::DeclarationTransformer;

/// tsgo ast.IsInJSFile: the file is JavaScript.
pub(crate) fn is_javascript_file(source: &SourceFile) -> bool {
    tsc_types::NodeFlags::from_bits(source.arena.node(source.root).flags)
        .intersects(tsc_types::NodeFlags::JAVA_SCRIPT_FILE)
}

/// tsgo thisPropertyAssignmentKey (transform.go:46-61): a member's name text,
/// or the declaration itself when its name has no static text.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ThisPropertyKey {
    Name {
        text: JsString,
        is_static: bool,
        is_private: bool,
    },
    Node {
        node: NodeId,
        is_static: bool,
        is_private: bool,
    },
}

/// A `this.x = …` assignment found in a class member, in tsgo's visiting
/// order.
struct ThisPropertyAssignment {
    node: NodeId,
    name: NodeId,
    is_static: bool,
}

impl DeclarationTransformer<'_> {
    /// tsgo collectThisPropertyAssignments and visitThisPropertyAssignments
    /// (transform.go:2084-2192): a property declaration for each member a
    /// JavaScript class gets only from `this.x = …` assignments in its
    /// constructor, methods, accessors, property initializers and static
    /// blocks. Only `static` is kept of the member's modifiers.
    pub(crate) fn collect_this_property_assignments(
        &mut self,
        cx: &mut TransformationContext,
        class: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let (assignments, mut seen, has_base_types) = {
            let source = cx.arena().source(class.source())?.syntax();
            let members = class_members(source, class.node());
            let mut seen = FxHashSet::default();
            for &member in &members {
                if let Some(name) = node_util::name_field_of(source, member) {
                    let is_static = is_static(source, member);
                    seen.insert(this_property_key(source, Some(name), member, is_static));
                }
            }
            let mut assignments = Vec::new();
            for &member in &members {
                for child in children(source, member) {
                    find_this_property_assignments(source, class.node(), child, &mut assignments);
                }
            }
            (assignments, seen, has_base_types(source, class.node()))
        };
        let mut properties = Vec::new();
        for assignment in assignments {
            let node = TransformNode::new(class.source(), assignment.node);
            let (key, is_dynamic, name_text, is_identifier_name) = {
                let source = cx.arena().source(class.source())?.syntax();
                (
                    this_property_key(
                        source,
                        Some(assignment.name),
                        assignment.node,
                        assignment.is_static,
                    ),
                    node_util::is_dynamic_name(source, assignment.name),
                    property_name_text(source, assignment.name),
                    source.arena.node(assignment.name).kind == SyntaxKind::Identifier,
                )
            };
            let base = self
                .resolver
                .get_referenced_member_value_declaration(self.required_resolver_node(cx, node)?)?;
            if base.is_none() || seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            if has_base_types {
                // The member may override one of a base type, which the
                // assignment's declaration does not show; isolated
                // declarations cannot tell, and a member the base type
                // already provides is left out.
                if self.options.isolated_declarations == Some(true) {
                    self.tracker
                        .report_isolated_inference(TrackerAnchor::Transform(class));
                    let effects = self.tracker.take_pending_effects();
                    materialize_effects(cx, self.host, effects)?;
                }
                if self
                    .resolver
                    .is_this_property_assignment_declaration_redundant(
                        self.required_resolver_node(cx, node)?,
                    )?
                {
                    continue;
                }
            }
            // A dynamic name is an element access, which is never a simple
            // inlineable expression, so tsgo emits no member for it
            // (transform.go:2131-2137).
            if is_dynamic {
                continue;
            }
            if name_text
                .as_ref()
                .is_some_and(|text| *text == "constructor")
            {
                continue;
            }
            let modifiers = if assignment.is_static {
                cx.factory()?
                    .create_modifiers_from_modifier_flags(class.source(), ModifierFlags::STATIC)?
            } else {
                None
            };
            let name = match name_text {
                Some(text)
                    if is_identifier_name
                        && !tsc_syntax::is_identifier_text(&text.to_string_lossy()) =>
                {
                    cx.factory()?
                        .create_string_literal(class.source(), text, false)?
                }
                _ => TransformNode::new(class.source(), assignment.name),
            };
            let r#type = self.ensure_type(cx, node, false)?;
            let property = cx.factory()?.create_property_declaration(
                class.source(),
                modifiers,
                name,
                None,
                r#type,
                None,
            )?;
            let statement = {
                let source = cx.arena().source(class.source())?.syntax();
                node_util::parent_of(source, assignment.node).filter(|&parent| {
                    source.arena.node(parent).kind == SyntaxKind::ExpressionStatement
                })
            };
            let property = match statement {
                Some(statement) => {
                    preserve_js_doc(cx, property, TransformNode::new(class.source(), statement))?
                }
                None => property,
            };
            properties.push(property);
        }
        Ok(properties)
    }
}

/// The types of a JavaScript class's `@implements` tags, which tsgo's
/// reparser adds to its `implements` clause (reparser.go:568-589).
pub(crate) fn hosted_implements(
    cx: &TransformationContext,
    class: TransformNode,
) -> Result<Vec<TransformNode>, TransformError> {
    let source = cx.arena().source(class.source())?.syntax();
    if !is_javascript_file(source) {
        return Ok(Vec::new());
    }
    Ok(tsc_binder::jsdoc_hosted(source)
        .implements_tags_of(class.node())
        .iter()
        .filter_map(|&tag| match &source.arena.node(tag).data {
            NodeData::JSDocImplementsTag(data) => data.class,
            _ => None,
        })
        .map(|name| TransformNode::new(class.source(), name))
        .collect())
}

/// The type arguments of the `@augments` tag whose type arguments tsgo's
/// reparser gives a JavaScript class's `extends` element
/// (reparser.go:590-607).
pub(crate) fn hosted_augments_type_arguments(
    cx: &TransformationContext,
    element: TransformNode,
) -> Result<Option<NodeArrayId>, TransformError> {
    let source = cx.arena().source(element.source())?.syntax();
    if !is_javascript_file(source) {
        return Ok(None);
    }
    let Some(tag) = tsc_binder::jsdoc_hosted(source).augments_tag_of(element.node()) else {
        return Ok(None);
    };
    let NodeData::JSDocAugmentsTag(data) = &source.arena.node(tag).data else {
        return Ok(None);
    };
    Ok(data
        .class
        .and_then(|class| match &source.arena.node(class).data {
            NodeData::ExpressionWithTypeArguments(class) => class.type_arguments,
            _ => None,
        }))
}

/// The `@template` tags whose type parameters tsgo's reparser gives a
/// JavaScript function, method or class that has none written
/// (reparser.go:440-457, gatherTypeParameters 297-344).
pub(crate) fn hosted_template_tags(
    cx: &TransformationContext,
    owner: TransformNode,
) -> Result<Vec<NodeId>, TransformError> {
    let source = cx.arena().source(owner.source())?.syntax();
    if !is_javascript_file(source) {
        return Ok(Vec::new());
    }
    Ok(tsc_binder::jsdoc_hosted(source)
        .type_parameters_of(owner.node())
        .map(|parameters| parameters.tags.clone())
        .unwrap_or_default())
}

impl DeclarationTransformer<'_> {
    /// The type parameters tsgo's gatherTypeParameters builds from
    /// `@template` tags (reparser.go:297-344), visited as the declaration
    /// transform visits written ones: a tag's constraint belongs to its first
    /// type parameter.
    pub(crate) fn visit_hosted_type_parameters(
        &mut self,
        cx: &mut TransformationContext,
        owner: TransformNode,
        tags: &[NodeId],
    ) -> Result<TransformNodeArray, TransformError> {
        let mut parameters = Vec::new();
        for &tag in tags {
            let (constraint, type_parameters) = {
                let source = cx.arena().source(owner.source())?.syntax();
                let NodeData::JSDocTemplateTag(data) = &source.arena.node(tag).data else {
                    continue;
                };
                let constraint = data.constraint.and_then(|expression| {
                    match &source.arena.node(expression).data {
                        NodeData::JSDocTypeExpression(expression) => expression.r#type,
                        _ => None,
                    }
                });
                let type_parameters = data
                    .type_parameters
                    .map(|list| source.arena.node_array(list).nodes.to_vec())
                    .unwrap_or_default();
                (constraint, type_parameters)
            };
            for (index, parameter) in type_parameters.into_iter().enumerate() {
                let parameter = TransformNode::new(owner.source(), parameter);
                let visited = match constraint {
                    Some(constraint) if index == 0 => {
                        let NodeData::TypeParameter(data) =
                            cx.arena().node(parameter)?.data.clone()
                        else {
                            continue;
                        };
                        let Some(name) = data.name else {
                            continue;
                        };
                        // The reparsed type parameter is visited like a
                        // written one, in its own diagnostic context.
                        let saved = if self.tracker.suppress_new_diagnostic_contexts {
                            None
                        } else {
                            Some(self.tracker.replace_diagnostic_context(
                                cx.arena(),
                                DiagnosticContext::ForNode(parameter),
                            )?)
                        };
                        let visited = (|| {
                            let constraint = self.visit_hosted_type(
                                cx,
                                TransformNode::new(owner.source(), constraint),
                            )?;
                            let default = match data.r#default {
                                Some(default) => self.visit_hosted_type(
                                    cx,
                                    TransformNode::new(owner.source(), default),
                                )?,
                                None => None,
                            };
                            Ok::<_, TransformError>((constraint, default))
                        })();
                        if let Some(saved) = saved {
                            self.tracker.restore_diagnostic_context(saved);
                        }
                        let (constraint, default) = visited?;
                        let modifiers = data
                            .modifiers
                            .and_then(|list| cx.arena().node_array_ref(owner.source(), list));
                        Some(cx.factory()?.create_type_parameter_declaration(
                            owner.source(),
                            modifiers,
                            TransformNode::new(owner.source(), name),
                            constraint,
                            default,
                        )?)
                    }
                    _ => match self.visit_declaration_subtree(cx, parameter)? {
                        VisitResult::Node(node) => Some(node),
                        _ => None,
                    },
                };
                parameters.extend(visited);
            }
        }
        cx.factory()?.create_node_array(owner.source(), parameters)
    }

    fn visit_hosted_type(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        match self.visit_declaration_subtree(cx, node)? {
            VisitResult::Node(node) => Ok(Some(node)),
            _ => Ok(None),
        }
    }
}

/// The type tsgo's reparser gives a JavaScript declaration from JSDoc
/// (`@type`, `@param`, `@returns`): its `Type()`.
pub(crate) fn hosted_type(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    let source = cx.arena().source(node.source())?.syntax();
    if !is_javascript_file(source) {
        return Ok(None);
    }
    Ok(tsc_binder::jsdoc_hosted(source)
        .type_of(node.node())
        .map(|r#type| TransformNode::new(node.source(), r#type)))
}

/// A `@typedef`, `@callback` or `@import` tag that tsgo's reparser turns into
/// a statement (reparser.go:74-138), with the JSDoc comment that holds it.
#[derive(Clone, Copy)]
struct ReparsedTag {
    tag: NodeId,
    comment: NodeId,
}

/// The tags whose statements tsgo's parser puts before a top-level
/// statement, in its order (parser.go:614-643): a node's JSDoc is reparsed
/// when the node is finished, after its children, and the statement list of a
/// block keeps the statements of the tags inside it.
fn reparsed_statement_tags(source: &SourceFile, statement: NodeId) -> Vec<ReparsedTag> {
    let mut tags = Vec::new();
    collect_reparsed_tags(source, statement, &mut tags);
    tags
}

fn collect_reparsed_tags(source: &SourceFile, node: NodeId, tags: &mut Vec<ReparsedTag>) {
    if !matches!(
        source.arena.node(node).kind,
        SyntaxKind::Block | SyntaxKind::ModuleBlock
    ) {
        for child in children(source, node) {
            collect_reparsed_tags(source, child, tags);
        }
    }
    push_reparsed_tags(source, node, tags);
}

fn push_reparsed_tags(source: &SourceFile, host: NodeId, tags: &mut Vec<ReparsedTag>) {
    let Some(comments) = source.arena.node(host).js_doc else {
        return;
    };
    for &comment in source.arena.node_array(comments).nodes {
        let NodeData::JSDoc(data) = &source.arena.node(comment).data else {
            continue;
        };
        let Some(list) = data.tags else {
            continue;
        };
        for &tag in source.arena.node_array(list).nodes {
            let reparsed = match &source.arena.node(tag).data {
                NodeData::JSDocTypedefTag(data) => data.type_expression.is_some(),
                NodeData::JSDocCallbackTag(data) => data.type_expression.is_some(),
                NodeData::JSDocImportTag(data) => data.import_clause.is_some(),
                _ => false,
            };
            if reparsed {
                tags.push(ReparsedTag { tag, comment });
            }
        }
    }
}

fn template_tags_of_comment(source: &SourceFile, comment: NodeId) -> Vec<NodeId> {
    let NodeData::JSDoc(data) = &source.arena.node(comment).data else {
        return Vec::new();
    };
    data.tags
        .map(|list| source.arena.node_array(list).nodes.to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|&tag| source.arena.node(tag).kind == SyntaxKind::JSDocTemplateTag)
        .collect()
}

/// tsgo scanner.GetTextOfJSDocComment (scanner/utilities.go:106-120).
fn text_of_jsdoc_comment(source: &SourceFile, comment: Option<&JSDocComment>) -> String {
    let mut text = String::new();
    match comment {
        None => {}
        Some(JSDocComment::Text(comment)) => text.push_str(comment),
        Some(JSDocComment::Nodes(nodes)) => {
            for &node in source.arena.node_array(*nodes).nodes {
                let record = source.arena.node(node);
                match &record.data {
                    NodeData::JSDocText(data) => text.push_str(&data.text),
                    NodeData::JSDocLink(_)
                    | NodeData::JSDocLinkCode(_)
                    | NodeData::JSDocLinkPlain(_) => text.push_str(
                        source
                            .text()
                            .get(record.pos as usize..record.end as usize)
                            .unwrap_or_default(),
                    ),
                    _ => {}
                }
            }
        }
    }
    text.truncate(text.trim_end().len());
    text
}

/// tsgo reparseJSDocSignature's parameter name (reparser.go:194-216): a name
/// that is not an identifier has each invalid character replaced by `_`.
fn sanitized_parameter_name(text: &str, index: usize) -> Option<String> {
    if tsc_syntax::is_identifier_text(text) {
        return None;
    }
    let mut result = String::new();
    for (position, character) in text.chars().enumerate() {
        let valid = if position == 0 {
            tsc_syntax::is_identifier_text(&character.to_string())
        } else {
            tsc_syntax::is_identifier_text(&format!("_{character}"))
        };
        result.push(if valid { character } else { '_' });
    }
    if result.is_empty() {
        result = format!("_{index}");
    }
    Some(result)
}

impl DeclarationTransformer<'_> {
    /// tsgo TryJSTypeNodeToTypeNode for the declaration being transformed.
    pub(crate) fn try_js_type_node_to_type_node(
        &mut self,
        cx: &mut TransformationContext,
        type_node: TransformNode,
    ) -> Result<Option<TransformNode>, TransformError> {
        let resolver_node = self.required_resolver_node(cx, type_node)?;
        let enclosing = self.current_enclosing_resolver_node(cx)?;
        let target = self.state()?.current_source_file;
        let result = self.resolver.try_js_type_node_to_type_node(
            cx.arena_mut()?,
            target,
            resolver_node,
            enclosing,
            EmitNodeBuilderFlags::DECLARATION_EMIT,
            EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
            &mut self.tracker,
        );
        let effects = self.tracker.take_pending_effects();
        materialize_effects(cx, self.host, effects)?;
        result.map_err(TransformError::from)
    }

    /// The statements tsgo's parser inserts before a top-level statement of a
    /// JavaScript file: the type aliases of the `@typedef` and `@callback`
    /// tags and the imports of the `@import` tags in its JSDoc and in the
    /// JSDoc of the nodes inside it that are not in a block.
    pub(crate) fn reparsed_statements_before(
        &mut self,
        cx: &mut TransformationContext,
        statement: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let tags = {
            let source = cx.arena().source(statement.source())?.syntax();
            if !is_javascript_file(source) {
                return Ok(Vec::new());
            }
            reparsed_statement_tags(source, statement.node())
        };
        self.reparsed_statements(cx, statement.source(), tags)
    }

    /// The statements of the tags in the JSDoc of the end-of-file token,
    /// which tsgo's parser appends to the statements (parser.go:445-448).
    pub(crate) fn reparsed_statements_at_end(
        &mut self,
        cx: &mut TransformationContext,
        root: TransformNode,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let tags = {
            let source = cx.arena().source(root.source())?.syntax();
            if !is_javascript_file(source) {
                return Ok(Vec::new());
            }
            let mut tags = Vec::new();
            if let NodeData::SourceFile(data) = &source.arena.node(root.node()).data {
                if let Some(end) = data.end_of_file_token {
                    push_reparsed_tags(source, end, &mut tags);
                }
            }
            tags
        };
        self.reparsed_statements(cx, root.source(), tags)
    }

    fn reparsed_statements(
        &mut self,
        cx: &mut TransformationContext,
        source: TransformSourceId,
        tags: Vec<ReparsedTag>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        // tsgo IsImplicitlyExportedJSDocDeclaration (ast/utilities.go:4226-4236).
        let root = TransformNode::new(source, cx.arena().source(source)?.syntax().root);
        let is_module = self
            .resolver
            .is_external_or_common_js_module(self.required_resolver_node(cx, root)?)?;
        let mut statements = Vec::new();
        for tag in tags {
            let tag_node = TransformNode::new(source, tag.tag);
            if cx.arena().node(tag_node)?.kind == SyntaxKind::JSDocImportTag {
                // The import is a late-painted statement: its bindings are
                // kept once declarations refer to them, so the tag stands
                // in the list until the late pass replaces it
                // (transformAndReplaceLatePaintedStatements).
                let result =
                    super::statements::transform_top_level_declaration(self, cx, tag_node)?;
                self.state_mut()?
                    .late_statement_replacement
                    .insert(tag.tag, result);
                statements.push(tag_node);
            } else {
                statements.push(self.reparsed_type_alias(cx, source, tag, is_module)?);
            }
        }
        Ok(statements)
    }

    /// The declaration of a `@typedef` or `@callback` tag as tsgo's reparser
    /// builds it (reparser.go:76-123) and its declaration transform prints
    /// it: `export type` in a module, wrapped in a namespace for a dotted
    /// name, whose nested namespaces lose `export` and whose type alias keeps
    /// it (transform.go:1858-1866, stripExportModifiers 1906-1924).
    fn reparsed_type_alias(
        &mut self,
        cx: &mut TransformationContext,
        source: TransformSourceId,
        tag: ReparsedTag,
        is_module: bool,
    ) -> Result<TransformNode, TransformError> {
        let tag_node = TransformNode::new(source, tag.tag);
        let (namespaces, name, type_expression, templates) = {
            let syntax = cx.arena().source(source)?.syntax();
            let (full_name, name, type_expression) = match &syntax.arena.node(tag.tag).data {
                NodeData::JSDocTypedefTag(data) => {
                    (data.full_name, data.name, data.type_expression)
                }
                NodeData::JSDocCallbackTag(data) => {
                    (data.full_name, data.name, data.type_expression)
                }
                _ => return Err(Self::contract("reparsed alias tag is not a typedef")),
            };
            let mut namespaces = Vec::new();
            let mut current = full_name;
            while let Some(node) = current {
                let NodeData::ModuleDeclaration(module) = &syntax.arena.node(node).data else {
                    break;
                };
                namespaces.extend(module.name);
                current = module.body;
            }
            (
                namespaces,
                name,
                type_expression,
                template_tags_of_comment(syntax, tag.comment),
            )
        };
        let name = name.ok_or_else(|| Self::contract("reparsed alias has no name"))?;
        let type_expression = TransformNode::new(
            source,
            type_expression.ok_or_else(|| Self::contract("reparsed alias has no type"))?,
        );
        let previous_enclosing =
            std::mem::replace(&mut self.state_mut()?.enclosing_declaration, Some(tag_node));
        let saved = if self.tracker.suppress_new_diagnostic_contexts {
            None
        } else {
            Some(
                self.tracker
                    .replace_diagnostic_context(cx.arena(), DiagnosticContext::ForNode(tag_node))?,
            )
        };
        let parts = (|| {
            let type_parameters = if templates.is_empty() {
                None
            } else {
                Some(self.visit_hosted_type_parameters(cx, tag_node, &templates)?)
            };
            let r#type = self.reparsed_alias_type(cx, type_expression)?;
            Ok::<_, TransformError>((type_parameters, r#type))
        })();
        if let Some(saved) = saved {
            self.tracker.restore_diagnostic_context(saved);
        }
        self.state_mut()?.enclosing_declaration = previous_enclosing;
        let (type_parameters, r#type) = parts?;
        let mut factory = cx.factory()?;
        let modifiers = if is_module || !namespaces.is_empty() {
            factory.create_modifiers_from_modifier_flags(source, ModifierFlags::EXPORT)?
        } else {
            None
        };
        let mut statement = factory.create_type_alias_declaration(
            source,
            modifiers,
            TransformNode::new(source, name),
            type_parameters,
            r#type,
        )?;
        for (index, &namespace) in namespaces.iter().enumerate().rev() {
            let flags = if index == 0 {
                let mut flags = ModifierFlags::AMBIENT;
                if is_module {
                    flags |= ModifierFlags::EXPORT;
                }
                flags
            } else {
                ModifierFlags::NONE
            };
            let modifiers = factory.create_modifiers_from_modifier_flags(source, flags)?;
            let statements = factory.create_node_array(source, vec![statement])?;
            let block = factory.create_module_block(source, statements)?;
            statement = factory.create_module_declaration(
                source,
                modifiers,
                TransformNode::new(source, namespace),
                None,
                Some(block),
                tsc_types::NodeFlags::NAMESPACE,
            )?;
        }
        Ok(statement)
    }

    /// The type of a reparsed type alias: a written type visited by the
    /// declaration transform, the type literal of `@property` tags, or the
    /// function type of a `@callback`.
    fn reparsed_alias_type(
        &mut self,
        cx: &mut TransformationContext,
        type_expression: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = type_expression.source();
        match cx.arena().node(type_expression)?.data.clone() {
            NodeData::JSDocTypeExpression(data) => {
                let r#type = data
                    .r#type
                    .ok_or_else(|| Self::contract("JSDoc type expression has no type"))?;
                match self.visit_declaration_subtree(cx, TransformNode::new(source, r#type))? {
                    VisitResult::Node(node) => Ok(node),
                    _ => cx
                        .factory()?
                        .create_keyword_type_node(source, SyntaxKind::AnyKeyword),
                }
            }
            NodeData::JSDocTypeLiteral(_) => self.reparsed_type_literal(cx, type_expression),
            NodeData::JSDocSignature(_) => self.reparsed_signature_type(cx, type_expression),
            _ => Err(Self::contract("reparsed alias type is not a JSDoc type")),
        }
    }

    /// tsgo reparseJSDocTypeLiteral (reparser.go:244-283) as the declaration
    /// transform prints it: each property's type through ensureType, and the
    /// comment of its tag kept (preservePartialJsDoc, transform.go:1614-1628).
    fn reparsed_type_literal(
        &mut self,
        cx: &mut TransformationContext,
        literal: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = literal.source();
        let (properties, is_array_type) = {
            let syntax = cx.arena().source(source)?.syntax();
            let NodeData::JSDocTypeLiteral(data) = &syntax.arena.node(literal.node()).data else {
                return Err(Self::contract(
                    "reparsed type literal is not a JSDoc type literal",
                ));
            };
            let mut properties = Vec::new();
            for &tag in data
                .js_doc_property_tags
                .map(|tags| syntax.arena.node_array(tags).nodes.to_vec())
                .unwrap_or_default()
                .iter()
            {
                let (name, type_expression, is_bracketed, comment) =
                    match &syntax.arena.node(tag).data {
                        NodeData::JSDocPropertyTag(data) => (
                            data.name,
                            data.type_expression,
                            data.is_bracketed,
                            data.comment.clone(),
                        ),
                        NodeData::JSDocParameterTag(data) => (
                            data.name,
                            data.type_expression,
                            data.is_bracketed,
                            data.comment.clone(),
                        ),
                        _ => continue,
                    };
                let Some(name) = name else {
                    continue;
                };
                let name = match &syntax.arena.node(name).data {
                    NodeData::QualifiedName(qualified) => qualified.right.unwrap_or(name),
                    _ => name,
                };
                let invalid_name = match &syntax.arena.node(name).data {
                    NodeData::Identifier(data) if !tsc_syntax::is_identifier_text(data.text()) => {
                        Some(data.text().to_owned())
                    }
                    _ => None,
                };
                let r#type = type_expression.and_then(|expression| {
                    match &syntax.arena.node(expression).data {
                        NodeData::JSDocTypeExpression(data) => data.r#type,
                        _ => Some(expression),
                    }
                });
                let optional = is_bracketed
                    || r#type.is_some_and(|r#type| {
                        syntax.arena.node(r#type).kind == SyntaxKind::JSDocOptionalType
                    });
                properties.push((
                    tag,
                    name,
                    invalid_name,
                    r#type,
                    optional,
                    text_of_jsdoc_comment(syntax, comment.as_ref()),
                ));
            }
            (properties, data.is_array_type)
        };
        let mut members = Vec::new();
        for (tag, name, invalid_name, r#type, optional, comment) in properties {
            let name = match invalid_name {
                Some(text) => cx.factory()?.create_string_literal(source, text, false)?,
                None => TransformNode::new(source, name),
            };
            let question = if optional {
                Some(cx.factory()?.create_token(
                    source,
                    SyntaxKind::QuestionToken,
                    crate::TransformFlags::NONE,
                )?)
            } else {
                None
            };
            let r#type = self.ensure_reparsed_type(
                cx,
                TransformNode::new(source, tag),
                r#type.map(|r#type| TransformNode::new(source, r#type)),
            )?;
            let property = cx
                .factory()?
                .create_property_signature(source, None, name, question, r#type)?;
            if !comment.is_empty() {
                cx.arena_mut()?.metadata_mut(property).add_leading_comment(
                    crate::SyntheticComment::new(
                        crate::SyntheticCommentKind::MultiLine,
                        format!("*\n * {}\n ", comment.replace('\n', "\n * ")),
                        false,
                        true,
                    ),
                );
            }
            members.push(property);
        }
        let mut factory = cx.factory()?;
        let members = factory.create_node_array(source, members)?;
        let literal = factory.create_type_literal_node(source, members)?;
        if is_array_type {
            factory.create_array_type_node(source, literal)
        } else {
            Ok(literal)
        }
    }

    /// tsgo reparseJSDocSignature for a `@callback` (reparser.go:146-242) as
    /// the declaration transform prints the function type.
    fn reparsed_signature_type(
        &mut self,
        cx: &mut TransformationContext,
        signature: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let source = signature.source();
        let (parameters, return_type) = {
            let syntax = cx.arena().source(source)?.syntax();
            let NodeData::JSDocSignature(data) = &syntax.arena.node(signature.node()).data else {
                return Err(Self::contract(
                    "reparsed signature is not a JSDoc signature",
                ));
            };
            let mut parameters = Vec::new();
            for (index, &tag) in data
                .parameters
                .map(|list| syntax.arena.node_array(list).nodes.to_vec())
                .unwrap_or_default()
                .iter()
                .enumerate()
            {
                let NodeData::JSDocParameterTag(parameter) = &syntax.arena.node(tag).data else {
                    continue;
                };
                let Some(name) = parameter.name else {
                    continue;
                };
                // A `@param x.y` describes a property of `x`.
                if syntax.arena.node(name).kind == SyntaxKind::QualifiedName {
                    continue;
                }
                let renamed = match &syntax.arena.node(name).data {
                    NodeData::Identifier(data) => sanitized_parameter_name(data.text(), index),
                    _ => None,
                };
                let mut r#type = parameter.type_expression.and_then(|expression| {
                    match &syntax.arena.node(expression).data {
                        NodeData::JSDocTypeExpression(data) => data.r#type,
                        _ => Some(expression),
                    }
                });
                let optional = parameter.is_bracketed
                    || r#type.is_some_and(|r#type| {
                        syntax.arena.node(r#type).kind == SyntaxKind::JSDocOptionalType
                    });
                let mut rest = false;
                if let Some(NodeData::JSDocVariadicType(variadic)) =
                    r#type.map(|r#type| &syntax.arena.node(r#type).data)
                {
                    rest = true;
                    r#type = variadic.r#type;
                }
                parameters.push((tag, name, renamed, r#type, optional, rest));
            }
            let return_type = data
                .r#type
                .and_then(|tag| match &syntax.arena.node(tag).data {
                    NodeData::JSDocReturnTag(data) => data.type_expression.and_then(|expression| {
                        match &syntax.arena.node(expression).data {
                            NodeData::JSDocTypeExpression(data) => data.r#type,
                            _ => None,
                        }
                    }),
                    _ => None,
                });
            (parameters, return_type)
        };
        let mut declarations = Vec::new();
        for (tag, name, renamed, r#type, optional, rest) in parameters {
            let r#type = self.ensure_reparsed_type(
                cx,
                TransformNode::new(source, tag),
                r#type.map(|r#type| TransformNode::new(source, r#type)),
            )?;
            let mut factory = cx.factory()?;
            let name = match renamed {
                Some(text) => factory.create_identifier(source, text)?,
                None => TransformNode::new(source, name),
            };
            let dot = if rest {
                Some(factory.create_token(
                    source,
                    SyntaxKind::DotDotDotToken,
                    crate::TransformFlags::NONE,
                )?)
            } else {
                None
            };
            let question = if optional {
                Some(factory.create_token(
                    source,
                    SyntaxKind::QuestionToken,
                    crate::TransformFlags::NONE,
                )?)
            } else {
                None
            };
            declarations.push(
                factory.create_parameter_declaration(
                    source, None, dot, name, question, r#type, None,
                )?,
            );
        }
        let return_type = match return_type {
            Some(r#type) => {
                self.try_js_type_node_to_type_node(cx, TransformNode::new(source, r#type))?
            }
            None => None,
        };
        let mut factory = cx.factory()?;
        let return_type = match return_type {
            Some(r#type) => r#type,
            None => factory.create_keyword_type_node(source, SyntaxKind::AnyKeyword)?,
        };
        let parameters = factory.create_node_array(source, declarations)?;
        factory.create_function_type_node(source, None, parameters, return_type)
    }

    /// tsgo ensureType of a declaration its reparser builds from a JSDoc tag
    /// (transform.go:1650-1666): the tag's type through the node builder,
    /// else the type of the tag's symbol.
    fn ensure_reparsed_type(
        &mut self,
        cx: &mut TransformationContext,
        tag: TransformNode,
        r#type: Option<TransformNode>,
    ) -> Result<Option<TransformNode>, TransformError> {
        if let Some(r#type) = r#type {
            if let Some(reused) = self.try_js_type_node_to_type_node(cx, r#type)? {
                return Ok(Some(reused));
            }
        }
        self.ensure_type(cx, tag, false)
    }
}

/// tsgo visitThisPropertyAssignments' walk (transform.go:2084-2151): the
/// node's `this` must be the class's, so the walk stops at a function or
/// other member that binds its own `this`.
fn find_this_property_assignments(
    source: &SourceFile,
    class: NodeId,
    node: NodeId,
    found: &mut Vec<ThisPropertyAssignment>,
) {
    let Some(container) = node_util::get_this_container(source, node, false) else {
        return;
    };
    let Some(target) = node_util::parent_of(source, container) else {
        return;
    };
    if target != class {
        return;
    }
    if get_assignment_declaration_kind(source, node) == AssignmentDeclarationKind::ThisProperty {
        if let Some(name) = this_property_assignment_name(source, node) {
            found.push(ThisPropertyAssignment {
                node,
                name,
                is_static: node_util::has_syntactic_modifier(
                    source,
                    container,
                    ModifierFlags::STATIC,
                ) || source.arena.node(container).kind
                    == SyntaxKind::ClassStaticBlockDeclaration,
            });
        }
    }
    for child in children(source, node) {
        find_this_property_assignments(source, class, child, found);
    }
}

/// tsgo GetNameOfDeclaration of a `this.x = …` assignment
/// (GetNonAssignedNameOfDeclaration, ast/utilities.go:1467-1477): the
/// access's identifier name or literal argument, else the access itself.
fn this_property_assignment_name(source: &SourceFile, node: NodeId) -> Option<NodeId> {
    let NodeData::BinaryExpression(data) = &source.arena.node(node).data else {
        return None;
    };
    let left = data.left?;
    let name = match &source.arena.node(left).data {
        NodeData::PropertyAccessExpression(access) => access
            .name
            .filter(|&name| source.arena.node(name).kind == SyntaxKind::Identifier),
        NodeData::ElementAccessExpression(access) => access
            .argument_expression
            .map(|argument| node_util::skip_parentheses_pub(source, argument))
            .filter(|&argument| node_util::is_string_or_numeric_literal_like(source, argument)),
        _ => None,
    };
    Some(name.unwrap_or(left))
}

fn this_property_key(
    source: &SourceFile,
    name: Option<NodeId>,
    node: NodeId,
    is_static: bool,
) -> ThisPropertyKey {
    let is_private =
        name.is_some_and(|name| source.arena.node(name).kind == SyntaxKind::PrivateIdentifier);
    if let Some(text) = name
        .filter(|&name| !node_util::is_dynamic_name(source, name))
        .and_then(|name| property_name_text(source, name))
    {
        return ThisPropertyKey::Name {
            text,
            is_static,
            is_private,
        };
    }
    ThisPropertyKey::Node {
        node,
        is_static,
        is_private,
    }
}

/// tsgo TryGetTextOfPropertyName (ast/utilities.go:2182-2195).
fn property_name_text(source: &SourceFile, name: NodeId) -> Option<JsString> {
    match &source.arena.node(name).data {
        NodeData::ComputedPropertyName(data) => data
            .expression
            .filter(|&expression| node_util::is_string_or_numeric_literal_like(source, expression))
            .and_then(|expression| node_util::literal_text_of(source, expression))
            .map(JsString::from),
        NodeData::JsxNamespacedName(data) => {
            let namespace = node_util::literal_text_of(source, data.namespace?)?;
            let name = node_util::literal_text_of(source, data.name?)?;
            Some(JsString::from(format!(
                "{}:{}",
                namespace.to_string_lossy(),
                name.to_string_lossy()
            )))
        }
        _ => node_util::literal_text_of(source, name).map(JsString::from),
    }
}

/// tsgo ast.IsStatic (ast/utilities.go:1048-1051).
fn is_static(source: &SourceFile, member: NodeId) -> bool {
    node_util::has_syntactic_modifier(source, member, ModifierFlags::STATIC)
        || source.arena.node(member).kind == SyntaxKind::ClassStaticBlockDeclaration
}

/// Whether a class has a heritage clause other than `extends null`,
/// counting the `implements` clause tsgo's reparser adds from `@implements`
/// tags (transform.go:2114, isClassExtendingNull 2153-2168).
fn has_base_types(source: &SourceFile, class: NodeId) -> bool {
    let NodeData::ClassDeclaration(data) = &source.arena.node(class).data else {
        return false;
    };
    let clauses = data
        .heritage_clauses
        .map(|clauses| source.arena.node_array(clauses).nodes.to_vec())
        .unwrap_or_default();
    if clauses.is_empty()
        && tsc_binder::jsdoc_hosted(source)
            .implements_tags_of(class)
            .is_empty()
    {
        return false;
    }
    let extends_null = clauses.iter().any(|&clause| {
        let NodeData::HeritageClause(clause) = &source.arena.node(clause).data else {
            return false;
        };
        if clause.token != SyntaxKind::ExtendsKeyword {
            return false;
        }
        let types = clause
            .types
            .map(|types| source.arena.node_array(types).nodes.to_vec())
            .unwrap_or_default();
        types.len() == 1
            && matches!(
                &source.arena.node(types[0]).data,
                NodeData::ExpressionWithTypeArguments(element)
                    if element.expression.is_some_and(|expression| {
                        source.arena.node(expression).kind == SyntaxKind::NullKeyword
                    })
            )
    });
    !extends_null
}

fn class_members(source: &SourceFile, class: NodeId) -> Vec<NodeId> {
    match &source.arena.node(class).data {
        NodeData::ClassDeclaration(data) => data
            .members
            .map(|members| source.arena.node_array(members).nodes.to_vec())
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn children(source: &SourceFile, node: NodeId) -> Vec<NodeId> {
    let mut children = Vec::new();
    tsc_syntax::for_each_child(&source.arena, source.arena.node(node), |child| {
        children.push(child);
        false
    });
    children
}
