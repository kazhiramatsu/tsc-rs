//! CommonJS exports and imports as tsgo's declaration transform writes them.
//!
//! tsgo does not reparse CommonJS. Before the statements are visited, its
//! declaration transform turns `module.exports = …` into an export
//! assignment, and `exports.x = …`, `module.exports.x = …` and
//! `Object.defineProperty(exports, "x", …)` into exports, which are written
//! before the statements (visitCJSExportAssignments, visitNestedExpression,
//! appendCjsExports, transform.go:327-339, 2680-2723). A variable
//! initialized by `require` becomes an import (transform.go:875-906).

use rustc_hash::FxHashSet;
use tsc_syntax::{NodeData, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::{
    GeneratedIdentifierFlags, TransformError, TransformNode, TransformNodeArray, TransformSourceId,
    TransformationContext,
};

use super::diagnostics::DiagnosticContext;
use super::state::VisitResult;
use super::subtree::preserve_js_doc;
use super::tracker::TrackerAnchor;
use super::DeclarationTransformer;

/// tsgo's CommonJS state of the file being transformed (transform.go:86-98).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CommonJsState {
    /// The file has a CommonJS module indicator.
    pub(crate) is_common_js: bool,
    /// What the last `module.exports = …` became (cjsExportAssignment).
    pub(crate) export_assignment: Vec<TransformNode>,
    /// The name of an exported expression (cjsExportAssignmentName), which
    /// becomes the namespace of the file's other exports.
    pub(crate) export_assignment_name: Option<TransformNode>,
    /// The file's `exports.x = …` exports (cjsExportMembers).
    pub(crate) export_members: Vec<TransformNode>,
    /// The export names already written (witnessedCjsExports).
    pub(crate) witnessed: FxHashSet<String>,
}

/// The name of a CommonJS export as tsgo's
/// getNameExpressionPreferringIdentifier leaves it (transform.go:2641-2663).
struct CjsName {
    node: TransformNode,
    text: Option<String>,
    is_identifier: bool,
    is_string_literal: bool,
    /// The parse-tree node an identifier name resolves at.
    location: TransformNode,
}

fn node_text(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<String>, TransformError> {
    Ok(match &cx.arena().node(node)?.data {
        NodeData::Identifier(data) => Some(data.text().to_owned()),
        NodeData::StringLiteral(data) => data.text.as_str().map(str::to_owned),
        NodeData::NoSubstitutionTemplateLiteral(data) => data.text.as_str().map(str::to_owned),
        NodeData::NumericLiteral(data) => Some(data.text.clone()),
        _ => None,
    })
}

/// tsgo ast.IsRequireCall(node, true): `require("…")`.
fn is_require_call(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::CallExpression(data) = &cx.arena().node(node)?.data else {
        return Ok(false);
    };
    let Some(callee) = data
        .expression
        .and_then(|callee| cx.arena().node_ref(node.source(), callee))
    else {
        return Ok(false);
    };
    if !matches!(&cx.arena().node(callee)?.data, NodeData::Identifier(name) if name.text() == "require")
    {
        return Ok(false);
    }
    let arguments = match data
        .arguments
        .and_then(|arguments| cx.arena().node_array_ref(node.source(), arguments))
    {
        Some(arguments) => cx.arena().node_array(arguments)?.nodes.to_vec(),
        None => Vec::new(),
    };
    let [argument] = arguments.as_slice() else {
        return Ok(false);
    };
    Ok(matches!(
        cx.arena()
            .node(TransformNode::new(node.source(), *argument))?
            .kind,
        SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
    ))
}

/// tsgo ast.IsVariableDeclarationInitializedToRequire for a variable
/// declaration (ast/utilities.go:2874-2879).
pub(crate) fn is_variable_declaration_initialized_to_require(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<bool, TransformError> {
    let NodeData::VariableDeclaration(data) = &cx.arena().node(node)?.data else {
        return Ok(false);
    };
    match data
        .initializer
        .and_then(|initializer| cx.arena().node_ref(node.source(), initializer))
    {
        Some(initializer) => is_require_call(cx, initializer),
        None => Ok(false),
    }
}

impl DeclarationTransformer<'_> {
    /// tsgo visitCJSExportAssignments for one `module.exports = …`
    /// (transform.go:2680-2698): the export assignment it writes replaces any
    /// earlier one.
    pub(crate) fn collect_module_exports_assignment(
        &mut self,
        cx: &mut TransformationContext,
        binary: TransformNode,
    ) -> Result<(), TransformError> {
        let Some(input) = self.parent(cx, binary)? else {
            return Ok(());
        };
        let right = match &cx.arena().node(binary)?.data {
            NodeData::BinaryExpression(data) => data.right,
            _ => None,
        }
        .and_then(|right| cx.arena().node_ref(binary.source(), right));
        let Some(right) = right else {
            return Ok(());
        };
        let saved = self.enter_node_diagnostic_context(cx, binary)?;
        let result = self.transform_export_assignment(cx, input, binary, right, true);
        if let Some(saved) = saved {
            self.tracker.restore_diagnostic_context(saved);
        }
        let nodes = visit_result_nodes(result?);
        let state = self.state_mut()?;
        state.common_js.export_assignment = nodes;
        state.result_has_scope_marker = true;
        state.result_has_external_module_indicator = true;
        Ok(())
    }

    /// tsgo visitNestedExpression's CommonJS arms (transform.go:2706-2719):
    /// `exports.x = …`, `module.exports.x = …` or
    /// `Object.defineProperty(exports, "x", …)`.
    pub(crate) fn collect_common_js_export(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        let name_expression = match &cx.arena().node(node)?.data {
            NodeData::BinaryExpression(data) => {
                let left = data
                    .left
                    .and_then(|left| cx.arena().node_ref(node.source(), left));
                match left {
                    Some(left) => element_or_property_access_name(cx, left)?,
                    None => None,
                }
            }
            NodeData::CallExpression(data) => data
                .arguments
                .and_then(|arguments| cx.arena().node_array_ref(node.source(), arguments))
                .map(|arguments| {
                    cx.arena()
                        .node_array(arguments)
                        .map(|array| array.nodes.get(1).copied())
                })
                .transpose()?
                .flatten()
                .map(|argument| TransformNode::new(node.source(), argument)),
            _ => None,
        };
        let Some(name_expression) = name_expression else {
            return Ok(());
        };
        let name = self.name_expression_preferring_identifier(cx, name_expression)?;
        let saved = self.enter_node_diagnostic_context(cx, node)?;
        let result = self.transform_common_js_export(cx, node, &name);
        if let Some(saved) = saved {
            self.tracker.restore_diagnostic_context(saved);
        }
        if let Some(nodes) = result? {
            self.state_mut()?.common_js.export_members.extend(nodes);
        }
        Ok(())
    }

    /// tsgo setupDiagnosticContext for a node that can produce diagnostics.
    fn enter_node_diagnostic_context(
        &mut self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<
        Option<(DiagnosticContext, super::diagnostics::DiagnosticContextPlan)>,
        TransformError,
    > {
        if self.tracker.suppress_new_diagnostic_contexts
            || !super::diagnostics::can_produce_diagnostics(cx.arena().node(node)?.kind)
        {
            return Ok(None);
        }
        self.tracker
            .replace_diagnostic_context(cx.arena(), DiagnosticContext::ForNode(node))
            .map(Some)
    }

    /// tsgo getNameExpressionPreferringIdentifier (transform.go:2641-2663):
    /// a numeric name is a string; a string that is an identifier other than
    /// a keyword (or `default`) is that identifier.
    fn name_expression_preferring_identifier(
        &mut self,
        cx: &mut TransformationContext,
        expression: TransformNode,
    ) -> Result<CjsName, TransformError> {
        let source = expression.source();
        let kind = cx.arena().node(expression)?.kind;
        let text = node_text(cx, expression)?;
        let (node, is_string_literal) = if kind == SyntaxKind::NumericLiteral {
            let literal = cx.factory()?.create_string_literal(
                source,
                text.clone().unwrap_or_default(),
                false,
            )?;
            (literal, true)
        } else {
            (
                expression,
                matches!(
                    kind,
                    SyntaxKind::StringLiteral | SyntaxKind::NoSubstitutionTemplateLiteral
                ),
            )
        };
        if is_string_literal {
            if let Some(text) = text
                .as_deref()
                .filter(|text| tsc_syntax::is_identifier_text(text))
            {
                let keyword = tsc_syntax::identifier_to_keyword_kind(text);
                if keyword.is_none() || keyword == Some(SyntaxKind::DefaultKeyword) {
                    let identifier = cx.factory()?.create_identifier(source, text)?;
                    return Ok(CjsName {
                        node: identifier,
                        text: Some(text.to_owned()),
                        is_identifier: true,
                        is_string_literal: false,
                        location: expression,
                    });
                }
            }
        }
        Ok(CjsName {
            node,
            text,
            is_identifier: kind == SyntaxKind::Identifier,
            is_string_literal,
            location: expression,
        })
    }

    /// tsgo transformCommonJSExport (transform.go:1343-1349).
    fn transform_common_js_export(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        name: &CjsName,
    ) -> Result<Option<Vec<TransformNode>>, TransformError> {
        let Some(nodes) = self.transform_common_js_export_worker(cx, input, name)? else {
            return Ok(None);
        };
        self.wrap_in_cjs_export_namespace(cx, input.source(), nodes)
            .map(Some)
    }

    /// tsgo transformCommonJSExportWorker (transform.go:1351-1534).
    fn transform_common_js_export_worker(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        name: &CjsName,
    ) -> Result<Option<Vec<TransformNode>>, TransformError> {
        let source = input.source();
        let name_text = if name.is_identifier || name.is_string_literal {
            name.text.clone().unwrap_or_default()
        } else {
            String::new()
        };
        {
            let witnessed = &mut self.state_mut()?.common_js.witnessed;
            if !name_text.is_empty() && witnessed.contains(&name_text) {
                return Ok(None);
            }
            witnessed.insert(name_text.clone());
        }
        self.state_mut()?.result_has_external_module_indicator = true;
        self.state_mut()?.result_has_scope_marker = true;

        // Only a top-level `exports.x = y` becomes `export { y as x }`.
        let top_level_statement = match self.parent(cx, input)? {
            Some(parent) if cx.arena().node(parent)?.kind == SyntaxKind::ExpressionStatement => {
                self.parent(cx, parent)?
                    .map(|grandparent| {
                        cx.arena()
                            .node(grandparent)
                            .map(|node| node.kind == SyntaxKind::SourceFile)
                    })
                    .transpose()?
                    .unwrap_or(false)
            }
            _ => false,
        };
        if top_level_statement
            && self
                .resolver
                .is_common_js_alias_export(self.required_resolver_node(cx, input)?)?
        {
            let right = binary_right(cx, input)?
                .ok_or_else(|| Self::contract("an alias export has no right side"))?;
            let export = self.binary_expression_to_export_declaration(cx, right, name.node)?;
            return Ok(Some(vec![export]));
        }

        if let Some(class) = binary_right(cx, input)?
            .map(|right| super::export_assignment::unwrap_parentheses(cx, right))
            .transpose()?
            .filter(|right| {
                cx.arena()
                    .node(*right)
                    .is_ok_and(|node| node.kind == SyntaxKind::ClassExpression)
            })
        {
            return self
                .common_js_class_export(cx, input, class, name)
                .map(Some);
        }

        if name.is_identifier {
            if name.text.as_deref() == Some("default") {
                // const _default: Type; export default _default;
                let new_id = cx.factory()?.create_unique_name(
                    source,
                    "_default",
                    GeneratedIdentifierFlags::OPTIMISTIC,
                )?;
                let statement = self.typed_export_variable(cx, input, new_id, true)?;
                let mut factory = cx.factory()?;
                let assignment = factory.create_export_assignment(source, None, false, new_id)?;
                let assignment = cx.arena_mut()?.remove_all_comments(assignment);
                let statement = preserve_js_doc(cx, statement, input)?;
                return Ok(Some(vec![statement, assignment]));
            }
            let referenced = self.resolver.get_referenced_value_declaration_of_name(
                self.required_resolver_node(cx, name.location)?,
                name.text.as_deref().unwrap_or_default(),
            )?;
            let input_resolver_node = self.required_resolver_node(cx, input)?;
            if referenced.is_none_or(|referenced| referenced == input_resolver_node) {
                // export var name: Type
                let saved_fallback = self
                    .tracker
                    .error_fallback_node
                    .replace(TrackerAnchor::Transform(input));
                let r#type = self.ensure_type(cx, input, false);
                self.tracker.error_fallback_node = saved_fallback;
                let r#type = r#type?;
                let needs_declare = self.state()?.needs_declare;
                let mut factory = cx.factory()?;
                let flags = if needs_declare {
                    ModifierFlags::from_bits(
                        ModifierFlags::EXPORT.bits() | ModifierFlags::AMBIENT.bits(),
                    )
                } else {
                    ModifierFlags::EXPORT
                };
                let modifiers = factory.create_modifiers_from_modifier_flags(source, flags)?;
                let declaration =
                    factory.create_variable_declaration(source, name.node, None, r#type, None)?;
                let declarations = factory.create_node_array(source, vec![declaration])?;
                let list = factory.create_variable_declaration_list(
                    source,
                    declarations,
                    NodeFlags::NONE,
                )?;
                return Ok(Some(vec![
                    factory.create_variable_statement(source, modifiers, list)?
                ]));
            }
        }

        // const _exported: Type; export {_exported as "name"};
        let new_id = cx.factory()?.create_unique_name(
            source,
            "_exported",
            GeneratedIdentifierFlags::OPTIMISTIC,
        )?;
        let statement = self.typed_export_variable(cx, input, new_id, true)?;
        let mut factory = cx.factory()?;
        let specifier = factory.create_export_specifier(source, false, Some(new_id), name.node)?;
        let specifiers = factory.create_node_array(source, vec![specifier])?;
        let exports = factory.create_named_exports(source, specifiers)?;
        let declaration =
            factory.create_export_declaration(source, None, false, Some(exports), None, None)?;
        let declaration = cx.arena_mut()?.remove_all_comments(declaration);
        let statement = preserve_js_doc(cx, statement, input)?;
        Ok(Some(vec![statement, declaration]))
    }

    /// `const name: Type;` for an export's assignment, with tsgo's default
    /// export diagnostic and error fallback (transform.go:1475-1494).
    fn typed_export_variable(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        name: TransformNode,
        default_export_diagnostic: bool,
    ) -> Result<TransformNode, TransformError> {
        let source = input.source();
        let saved_diagnostic =
            if default_export_diagnostic {
                Some(self.tracker.replace_diagnostic_context(
                    cx.arena(),
                    DiagnosticContext::DefaultExport(input),
                )?)
            } else {
                None
            };
        let saved_fallback = self
            .tracker
            .error_fallback_node
            .replace(TrackerAnchor::Transform(input));
        let r#type = self.ensure_type(cx, input, false);
        self.tracker.error_fallback_node = saved_fallback;
        if let Some(saved) = saved_diagnostic {
            self.tracker.restore_diagnostic_context(saved);
        }
        let r#type = r#type?;
        let needs_declare = self.state()?.needs_declare;
        let mut factory = cx.factory()?;
        let modifiers = if needs_declare {
            factory.create_modifiers_from_modifier_flags(source, ModifierFlags::AMBIENT)?
        } else {
            None
        };
        let modifiers = match modifiers {
            Some(modifiers) => modifiers,
            None => factory.create_node_array(source, Vec::new())?,
        };
        let declaration = factory.create_variable_declaration(source, name, None, r#type, None)?;
        let declarations = factory.create_node_array(source, vec![declaration])?;
        let list =
            factory.create_variable_declaration_list(source, declarations, NodeFlags::CONST)?;
        factory.create_variable_statement(source, Some(modifiers), list)
    }

    /// tsgo transformBinaryExpressionToExportDeclaration
    /// (transform.go:1324-1341): `export { right as name }`, or
    /// `export { name }` when both are the same identifier.
    pub(crate) fn binary_expression_to_export_declaration(
        &mut self,
        cx: &mut TransformationContext,
        right: TransformNode,
        name: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let enclosing = self.state()?.enclosing_declaration.ok_or_else(|| {
            Self::contract("an export of an identifier needs an enclosing declaration")
        })?;
        self.check_entity_name_visibility(cx, right, enclosing)?;
        let same_name = match (&cx.arena().node(right)?.data, &cx.arena().node(name)?.data) {
            (NodeData::Identifier(right), NodeData::Identifier(name)) => {
                right.text() == name.text()
            }
            _ => false,
        };
        let source = name.source();
        let mut factory = cx.factory()?;
        let property_name = (!same_name).then_some(right);
        let specifier = factory.create_export_specifier(source, false, property_name, name)?;
        let specifiers = factory.create_node_array(source, vec![specifier])?;
        let exports = factory.create_named_exports(source, specifiers)?;
        factory.create_export_declaration(source, None, false, Some(exports), None, None)
    }

    /// The class expression arms of transformCommonJSExportWorker
    /// (transform.go:1371-1458).
    fn common_js_class_export(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
        class: TransformNode,
        name: &CjsName,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let source = input.source();
        let class_name_text = match &cx.arena().node(class)?.data {
            NodeData::ClassExpression(data) => data
                .name
                .and_then(|name| cx.arena().node_ref(class.source(), name))
                .map(|name| node_text(cx, name))
                .transpose()?
                .flatten()
                .filter(|text| !text.is_empty()),
            _ => None,
        };
        let needs_declare = self.state()?.needs_declare;
        if let Some(class_name_text) = class_name_text {
            // Watch whether the class's own symbol names one of its
            // members' types.
            let watched = self
                .resolver
                .get_tracker_symbol_of_node(self.required_resolver_node(cx, class)?)?;
            let previous_watched =
                std::mem::replace(&mut self.tracker.watched_class_symbol, watched);
            let previous_tracked = std::mem::replace(&mut self.tracker.class_symbol_tracked, false);
            let result = (|| {
                let class_name = cx
                    .factory()?
                    .create_identifier(source, class_name_text.clone())?;
                let modifiers = cx
                    .factory()?
                    .create_modifiers_from_modifier_flags(source, ModifierFlags::EXPORT)?
                    .ok_or_else(|| Self::contract("export modifiers are missing"))?;
                let declaration = self
                    .transform_class_expression_to_declaration(cx, class, class_name, modifiers)?;
                let declaration = preserve_js_doc(cx, declaration, input)?;
                let names_differ =
                    !name.is_identifier || name.text.as_deref() != Some(class_name_text.as_str());
                let needs_isolation = names_differ || self.tracker.class_symbol_tracked;
                if needs_isolation {
                    let mut factory = cx.factory()?;
                    let namespace_name = factory.create_unique_name(
                        source,
                        "_ns",
                        GeneratedIdentifierFlags::OPTIMISTIC,
                    )?;
                    let namespace_modifiers = if needs_declare {
                        factory
                            .create_modifiers_from_modifier_flags(source, ModifierFlags::AMBIENT)?
                    } else {
                        None
                    };
                    let namespace_modifiers = match namespace_modifiers {
                        Some(modifiers) => modifiers,
                        None => factory.create_node_array(source, Vec::new())?,
                    };
                    let statements = factory.create_node_array(source, vec![declaration])?;
                    let block = factory.create_module_block(source, statements)?;
                    let namespace = factory.create_module_declaration(
                        source,
                        Some(namespace_modifiers),
                        namespace_name,
                        None,
                        Some(block),
                        NodeFlags::NAMESPACE,
                    )?;
                    let alias_base = match name.text.as_deref() {
                        Some(text)
                            if name.is_identifier
                                && tsc_syntax::is_identifier_text(&format!("_{text}")) =>
                        {
                            format!("_{text}")
                        }
                        _ => "_exported".to_owned(),
                    };
                    let alias = factory.create_unique_name(
                        source,
                        alias_base,
                        GeneratedIdentifierFlags::OPTIMISTIC,
                    )?;
                    let qualified =
                        factory.create_qualified_name(source, namespace_name, class_name)?;
                    let import = factory
                        .create_import_equals_declaration(source, None, false, alias, qualified)?;
                    let specifier =
                        factory.create_export_specifier(source, false, Some(alias), name.node)?;
                    let specifiers = factory.create_node_array(source, vec![specifier])?;
                    let exports = factory.create_named_exports(source, specifiers)?;
                    let export = factory.create_export_declaration(
                        source,
                        None,
                        false,
                        Some(exports),
                        None,
                        None,
                    )?;
                    let export = cx.arena_mut()?.remove_all_comments(export);
                    return Ok(vec![namespace, import, export]);
                }
                // No isolation: `export declare class Name`.
                let flags = if needs_declare {
                    ModifierFlags::from_bits(
                        ModifierFlags::EXPORT.bits() | ModifierFlags::AMBIENT.bits(),
                    )
                } else {
                    ModifierFlags::EXPORT
                };
                let mut factory = cx.factory()?;
                let modifiers = factory.create_modifiers_from_modifier_flags(source, flags)?;
                Ok(vec![factory.replace_modifiers(declaration, modifiers)?])
            })();
            self.tracker.watched_class_symbol = previous_watched;
            self.tracker.class_symbol_tracked = previous_tracked;
            return result;
        }
        let flags = if needs_declare {
            ModifierFlags::from_bits(ModifierFlags::EXPORT.bits() | ModifierFlags::AMBIENT.bits())
        } else {
            ModifierFlags::EXPORT
        };
        let modifiers = cx
            .factory()?
            .create_modifiers_from_modifier_flags(source, flags)?
            .ok_or_else(|| Self::contract("export modifiers are missing"))?;
        let class_name = if name.is_identifier {
            name.node
        } else {
            cx.factory()?.create_unique_name(
                source,
                "_class",
                GeneratedIdentifierFlags::OPTIMISTIC,
            )?
        };
        let declaration =
            self.transform_class_expression_to_declaration(cx, class, class_name, modifiers)?;
        let declaration = preserve_js_doc(cx, declaration, input)?;
        if !name.is_identifier {
            let mut factory = cx.factory()?;
            let specifier =
                factory.create_export_specifier(source, false, Some(class_name), name.node)?;
            let specifiers = factory.create_node_array(source, vec![specifier])?;
            let exports = factory.create_named_exports(source, specifiers)?;
            let export = factory.create_export_declaration(
                source,
                None,
                false,
                Some(exports),
                None,
                None,
            )?;
            let export = cx.arena_mut()?.remove_all_comments(export);
            return Ok(vec![declaration, export]);
        }
        Ok(vec![declaration])
    }

    /// tsgo wrapInCJSExportNamespace (transform.go:1536-1562): with an
    /// exported expression's name, the exports become the members of a
    /// namespace of that name, without `declare`.
    fn wrap_in_cjs_export_namespace(
        &mut self,
        cx: &mut TransformationContext,
        source: TransformSourceId,
        content: Vec<TransformNode>,
    ) -> Result<Vec<TransformNode>, TransformError> {
        let Some(namespace_name) = self.state()?.common_js.export_assignment_name else {
            return Ok(content);
        };
        let mut members = Vec::with_capacity(content.len());
        for member in content {
            members.push(strip_declare_modifiers(cx, member)?);
        }
        let needs_declare = self.state()?.needs_declare;
        let mut factory = cx.factory()?;
        let modifiers = if needs_declare {
            factory.create_modifiers_from_modifier_flags(source, ModifierFlags::AMBIENT)?
        } else {
            None
        };
        let modifiers = match modifiers {
            Some(modifiers) => modifiers,
            None => factory.create_node_array(source, Vec::new())?,
        };
        let statements = factory.create_node_array(source, members)?;
        let block = factory.create_module_block(source, statements)?;
        Ok(vec![factory.create_module_declaration(
            source,
            Some(modifiers),
            namespace_name,
            None,
            Some(block),
            NodeFlags::NAMESPACE,
        )?])
    }

    /// tsgo transformSourceFile (transform.go:356-363): each declaration of a
    /// JavaScript module's `export=` when there is more than one.
    pub(crate) fn report_multiple_module_exports(
        &mut self,
        cx: &mut TransformationContext,
        root: TransformNode,
    ) -> Result<(), TransformError> {
        let declarations = self
            .resolver
            .get_export_equals_declarations(self.required_resolver_node(cx, root)?)?;
        if declarations.len() <= 1 {
            return Ok(());
        }
        for declaration in declarations {
            let Some(declaration) = cx.arena().parse_tree_transform_node(declaration)? else {
                continue;
            };
            self.tracker.report_diagnostic_at(
                TrackerAnchor::Transform(declaration),
                &tsc_diagnostics::gen::Multiple_module_exports_assignments_cannot_be_serialized_for_declaration_emit,
            );
        }
        let effects = self.tracker.take_pending_effects();
        super::tracker::materialize_effects(cx, self.host, effects)
    }

    /// tsgo transformCjsRequireVariableDeclaration (transform.go:875-906):
    /// `const x = require("m")` becomes `import x = require("m")`, and
    /// `const {x, y: z} = require("m")` becomes `import {x, y as z} from "m"`.
    pub(crate) fn transform_cjs_require_variable_declaration(
        &mut self,
        cx: &mut TransformationContext,
        input: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        let source = input.source();
        let (name, initializer) = match &cx.arena().node(input)?.data {
            NodeData::VariableDeclaration(data) => (data.name, data.initializer),
            _ => (None, None),
        };
        let name = name.and_then(|name| cx.arena().node_ref(source, name));
        let initializer =
            initializer.and_then(|initializer| cx.arena().node_ref(source, initializer));
        let (Some(name), Some(initializer)) = (name, initializer) else {
            return Ok(VisitResult::None);
        };
        let argument = match &cx.arena().node(initializer)?.data {
            NodeData::CallExpression(data) => data
                .arguments
                .and_then(|arguments| cx.arena().node_array_ref(source, arguments))
                .map(|arguments| {
                    cx.arena()
                        .node_array(arguments)
                        .map(|array| array.nodes.first().copied())
                })
                .transpose()?
                .flatten(),
            _ => None,
        };
        let Some(argument) = argument else {
            return Ok(VisitResult::None);
        };
        let specifier = super::statements::rewrite_module_specifier(
            self,
            cx,
            input,
            Some(TransformNode::new(source, argument)),
        )?
        .ok_or_else(|| Self::contract("a require call has no specifier"))?;
        match &cx.arena().node(name)?.data {
            NodeData::Identifier(_) => {
                let mut factory = cx.factory()?;
                let reference = factory.create_external_module_reference(source, specifier)?;
                let import = factory
                    .create_import_equals_declaration(source, None, false, name, reference)?;
                Ok(VisitResult::Node(import))
            }
            NodeData::ObjectBindingPattern(data) => {
                let elements = match data
                    .elements
                    .and_then(|elements| cx.arena().node_array_ref(source, elements))
                {
                    Some(elements) => cx.arena().node_array(elements)?.nodes.to_vec(),
                    None => Vec::new(),
                };
                let mut specifiers = Vec::new();
                for element in elements {
                    let element = TransformNode::new(source, element);
                    let NodeData::BindingElement(element_data) = &cx.arena().node(element)?.data
                    else {
                        continue;
                    };
                    let element_name = element_data
                        .name
                        .and_then(|name| cx.arena().node_ref(source, name));
                    let property_name = element_data
                        .property_name
                        .and_then(|name| cx.arena().node_ref(source, name));
                    let Some(element_name) = element_name.filter(|name| {
                        cx.arena()
                            .node(*name)
                            .is_ok_and(|name| name.kind == SyntaxKind::Identifier)
                    }) else {
                        continue;
                    };
                    specifiers.push(cx.factory()?.create_import_specifier(
                        source,
                        false,
                        property_name,
                        element_name,
                    )?);
                }
                let mut factory = cx.factory()?;
                let specifiers = factory.create_node_array(source, specifiers)?;
                let named = factory.create_named_imports(source, specifiers)?;
                let clause = factory.create_import_clause(source, None, None, Some(named))?;
                let import = factory.create_import_declaration(
                    source,
                    None,
                    Some(clause),
                    specifier,
                    None,
                )?;
                Ok(VisitResult::Node(import))
            }
            _ => Ok(VisitResult::None),
        }
    }
}

/// tsgo ast.GetElementOrPropertyAccessName (ast/utilities.go).
fn element_or_property_access_name(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    Ok(match &cx.arena().node(node)?.data {
        NodeData::PropertyAccessExpression(data) => data
            .name
            .and_then(|name| cx.arena().node_ref(node.source(), name))
            .filter(|name| {
                cx.arena()
                    .node(*name)
                    .is_ok_and(|name| name.kind == SyntaxKind::Identifier)
            }),
        NodeData::ElementAccessExpression(data) => {
            match data
                .argument_expression
                .and_then(|argument| cx.arena().node_ref(node.source(), argument))
            {
                Some(argument) => {
                    let argument = super::export_assignment::unwrap_parentheses(cx, argument)?;
                    matches!(
                        cx.arena().node(argument)?.kind,
                        SyntaxKind::StringLiteral
                            | SyntaxKind::NoSubstitutionTemplateLiteral
                            | SyntaxKind::NumericLiteral
                    )
                    .then_some(argument)
                }
                None => None,
            }
        }
        _ => None,
    })
}

fn binary_right(
    cx: &TransformationContext,
    node: TransformNode,
) -> Result<Option<TransformNode>, TransformError> {
    Ok(match &cx.arena().node(node)?.data {
        NodeData::BinaryExpression(data) => data
            .right
            .and_then(|right| cx.arena().node_ref(node.source(), right)),
        _ => None,
    })
}

/// tsgo stripDeclareModifiers (transform.go:2665-2678).
fn strip_declare_modifiers(
    cx: &mut TransformationContext,
    node: TransformNode,
) -> Result<TransformNode, TransformError> {
    let flags = super::statements::modifier_flags(cx, node)?;
    if !flags.contains(ModifierFlags::AMBIENT) {
        return Ok(node);
    }
    let modifiers = super::statements::modifiers_of(cx, node)?;
    let Some(modifiers) = modifiers else {
        return Ok(node);
    };
    let kept = cx
        .arena()
        .node_array(modifiers)?
        .nodes
        .iter()
        .copied()
        .map(|modifier| TransformNode::new(modifiers.source(), modifier))
        .filter(|modifier| {
            cx.arena()
                .node(*modifier)
                .is_ok_and(|modifier| modifier.kind != SyntaxKind::DeclareKeyword)
        })
        .collect::<Vec<_>>();
    let mut factory = cx.factory()?;
    let kept: TransformNodeArray = factory.create_node_array(modifiers.source(), kept)?;
    factory.replace_modifiers(node, Some(kept))
}

fn visit_result_nodes(result: VisitResult) -> Vec<TransformNode> {
    match result {
        VisitResult::None => Vec::new(),
        VisitResult::Node(node) => vec![node],
        VisitResult::Nodes(nodes) => nodes,
    }
}
