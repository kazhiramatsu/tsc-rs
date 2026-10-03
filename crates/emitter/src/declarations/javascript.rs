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
use tsc_syntax::{NodeArrayId, NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::ModifierFlags;

use crate::{TransformError, TransformNode, TransformNodeArray, TransformationContext};

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
