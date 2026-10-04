//! Expando declarations as tsgo's declaration transform writes them
//! (transformers/declarations/transform.go:2700-2970).
//!
//! Before the statements are visited, the transform walks the whole file for
//! `F.x = …` assignments that declare a property of a function or, in
//! JavaScript, of a class. Its host is rewritten as a function declaration
//! (or transformed as usual), and the properties become the members of a
//! namespace of the same name written after it.

use rustc_hash::FxHashMap;
use tsc_binder::{get_assignment_declaration_kind, AssignmentDeclarationKind};
use tsc_syntax::{NodeData, NodeId, SourceFile, SyntaxKind};
use tsc_types::{ModifierFlags, NodeFlags};

use crate::{
    EmitInternalNodeBuilderFlags, EmitNodeBuilderFlags, TransformError, TransformNode,
    TransformSourceId, TransformationContext,
};

use super::diagnostics::{can_produce_diagnostics, DiagnosticContext, DiagnosticContextPlan};
use super::state::VisitResult;
use super::tracker::materialize_effects;
use super::DeclarationTransformer;

/// tsgo's expandoHosts, expandoMembers and deferredExpandoAssignments, keyed
/// by the host's statement (getExpandoHostId, transform.go:2861-2865): the
/// variable statement of a variable, else the declaration.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExpandoState {
    /// What replaces a host's statement.
    hosts: FxHashMap<NodeId, VisitResult>,
    /// The members of a host's namespace.
    members: FxHashMap<NodeId, Vec<TransformNode>>,
    /// The assignments collected while their host was not visible; a host
    /// painted visible later takes them (transform.go:2770-2777).
    deferred: FxHashMap<NodeId, Vec<NodeId>>,
}

type SavedDiagnosticContext = (DiagnosticContext, DiagnosticContextPlan);

/// The assignments that declare properties, in tsgo's visiting order
/// (visitNestedExpression, transform.go:2700-2723): a node before its
/// children.
fn property_assignments(source: &SourceFile) -> Vec<NodeId> {
    let mut found = Vec::new();
    let mut stack = vec![source.root];
    while let Some(node) = stack.pop() {
        let record = source.arena.node(node);
        if record.kind == SyntaxKind::BinaryExpression
            && get_assignment_declaration_kind(source, node) == AssignmentDeclarationKind::Property
        {
            found.push(node);
        }
        let start = stack.len();
        tsc_syntax::for_each_child(&source.arena, record, |child| {
            stack.push(child);
            false
        });
        stack[start..].reverse();
    }
    found
}

/// tsgo ast.IsNonContextualKeyword(scanner.StringToToken(text)).
fn is_non_contextual_keyword(text: &str) -> bool {
    tsc_syntax::identifier_to_keyword_kind(text).is_some_and(|kind| {
        kind.value() >= SyntaxKind::FirstKeyword.value()
            && kind.value() < SyntaxKind::FirstContextualKeyword.value()
    })
}

impl DeclarationTransformer<'_> {
    /// tsgo transformSourceFile's `expressionVisitor` pass
    /// (transform.go:351), run before the statements are visited.
    pub(crate) fn collect_expandos(
        &mut self,
        cx: &mut TransformationContext,
        root: TransformNode,
    ) -> Result<(), TransformError> {
        let assignments = property_assignments(cx.arena().source(root.source())?.syntax());
        for assignment in assignments {
            let assignment = TransformNode::new(root.source(), assignment);
            self.transform_expando_assignment(cx, assignment)?;
        }
        Ok(())
    }

    /// tsgo transformExpandoAssignment (transform.go:2725-2859).
    fn transform_expando_assignment(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
    ) -> Result<(), TransformError> {
        if !self
            .resolver
            .is_assignment_declaration(self.required_resolver_node(cx, node)?)?
        {
            return Ok(());
        }
        let source = node.source();
        let (left, right) = match &cx.arena().node(node)?.data {
            NodeData::BinaryExpression(data) => (data.left, data.right),
            _ => return Ok(()),
        };
        let (Some(left), Some(right)) = (left, right) else {
            return Ok(());
        };
        let (left, right) = (
            TransformNode::new(source, left),
            TransformNode::new(source, right),
        );
        let mut namespace = left;
        loop {
            let expression = match &cx.arena().node(namespace)?.data {
                NodeData::PropertyAccessExpression(data) => data.expression,
                NodeData::ElementAccessExpression(data) => data.expression,
                _ => break,
            };
            let Some(expression) = expression else {
                return Ok(());
            };
            namespace = TransformNode::new(source, expression);
        }
        let namespace_name = match &cx.arena().node(namespace)?.data {
            NodeData::Identifier(data) => data.text().to_owned(),
            _ => return Ok(()),
        };
        let Some(declaration) = self
            .resolver
            .get_referenced_value_declaration(self.required_resolver_node(cx, namespace)?)?
        else {
            return Ok(());
        };
        // The binder only binds an assignment to a host of the same file.
        let Some(declaration) = cx
            .arena()
            .parse_tree_transform_node(declaration)?
            .filter(|declaration| declaration.source() == source)
        else {
            return Ok(());
        };
        if self.should_strip_internal(cx, Some(declaration))? {
            return Ok(());
        }
        let declaration_kind = cx.arena().node(declaration)?.kind;
        match &cx.arena().node(declaration)?.data {
            NodeData::VariableDeclaration(data) => {
                if data.r#type.is_some()
                    || super::javascript::hosted_type(cx, declaration)?.is_some()
                {
                    return Ok(());
                }
                // A variable that is not a function gets its members in its
                // type.
                let is_function = data.initializer.is_some_and(|initializer| {
                    cx.arena()
                        .node(TransformNode::new(source, initializer))
                        .is_ok_and(|initializer| super::ensure::is_function_like(initializer.kind))
                });
                if !is_function {
                    return Ok(());
                }
            }
            NodeData::FunctionDeclaration(_) => {
                if super::javascript::hosted_full_signature(cx, declaration)?.is_some() {
                    return Ok(());
                }
            }
            _ => {}
        }
        let property = match &cx.arena().node(left)?.data {
            NodeData::ElementAccessExpression(_) => self
                .resolver
                .get_element_access_expression_name(self.required_resolver_node(cx, left)?)?
                .unwrap_or_default(),
            NodeData::PropertyAccessExpression(data) => {
                match data.name.and_then(|name| cx.arena().node_ref(source, name)) {
                    Some(name) => match &cx.arena().node(name)?.data {
                        NodeData::Identifier(name) => name.text().to_owned(),
                        _ => String::new(),
                    },
                    None => String::new(),
                }
            }
            _ => String::new(),
        };
        if property.is_empty() || !tsc_syntax::is_identifier_text(&property) {
            return Ok(());
        }
        let host = self.expando_host_statement(cx, declaration)?;
        if self.is_declaration_and_not_visible(cx, declaration)? {
            // Painting a visible declaration's type may still mark the host
            // visible; its assignments wait for that.
            self.state_mut()?
                .expandos
                .deferred
                .entry(host.node())
                .or_default()
                .push(node.node());
            return Ok(());
        }
        if declaration_kind == SyntaxKind::FunctionDeclaration
            && !super::statements::should_emit_function_properties(self, cx, declaration)?
        {
            return Ok(());
        }
        self.transform_expando_host(cx, &namespace_name, declaration, host)?;

        let enclosing = self.state()?.enclosing_declaration.ok_or_else(|| {
            Self::contract("an expando assignment needs an enclosing declaration")
        })?;
        // The property's own name, unless that would resolve to something
        // else or is a keyword.
        let use_property = !self
            .resolver
            .is_name_resolvable(self.required_resolver_node(cx, enclosing)?, &property)?
            && !is_non_contextual_keyword(&property);
        let saved = self.enter_diagnostic_context(cx, node)?;
        let result = self.add_expando_member(cx, node, right, &property, use_property, host);
        if let Some(saved) = saved {
            self.tracker.restore_diagnostic_context(saved);
        }
        result
    }

    /// The members an assignment adds to its host's namespace
    /// (transform.go:2794-2858): `export { right as name }` for an
    /// identifier, else `var name: T`.
    fn add_expando_member(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
        right: TransformNode,
        property: &str,
        use_property: bool,
        host: TransformNode,
    ) -> Result<(), TransformError> {
        let source = node.source();
        if cx.arena().node(right)?.kind == SyntaxKind::Identifier {
            // tsgo transformBinaryExpressionToExportDeclaration
            // (transform.go:1324-1341).
            let enclosing = self.state()?.enclosing_declaration.ok_or_else(|| {
                Self::contract("an expando assignment needs an enclosing declaration")
            })?;
            self.check_entity_name_visibility(cx, right, enclosing)?;
            let same_name = matches!(
                &cx.arena().node(right)?.data,
                NodeData::Identifier(data) if data.text() == property
            );
            let mut factory = cx.factory()?;
            let name = factory.create_identifier(source, property)?;
            let property_name = (!same_name).then_some(right);
            let specifier = factory.create_export_specifier(source, false, property_name, name)?;
            let declaration = named_exports_declaration(&mut factory, source, specifier)?;
            self.expando_members_mut(host)?.push(declaration);
            return Ok(());
        }

        let has_export_declaration = self.has_expando_export_declaration(cx, host)?;
        let r#type = self.expando_member_type(cx, node, use_property.then_some(property))?;
        let mut factory = cx.factory()?;
        let local_name = if use_property {
            factory.create_identifier(source, property)?
        } else {
            factory.get_generated_name_for_non_member_node(node)?
        };
        let modifiers = if has_export_declaration {
            factory.create_modifiers_from_modifier_flags(source, ModifierFlags::EXPORT)?
        } else {
            None
        };
        let declaration =
            factory.create_variable_declaration(source, local_name, None, r#type, None)?;
        let declarations = factory.create_node_array(source, vec![declaration])?;
        let list =
            factory.create_variable_declaration_list(source, declarations, NodeFlags::NONE)?;
        let mut statements = vec![factory.create_variable_statement(source, modifiers, list)?];
        if !use_property {
            let name = factory.create_identifier(source, property)?;
            let specifier =
                factory.create_export_specifier(source, false, Some(local_name), name)?;
            statements.push(named_exports_declaration(&mut factory, source, specifier)?);
        }
        if statements.len() > 1 && !has_export_declaration {
            // The earlier members stay exported once the namespace has an
            // export declaration.
            let earlier = std::mem::take(self.expando_members_mut(host)?);
            let mut exported = Vec::with_capacity(earlier.len());
            for member in earlier {
                let flags = super::statements::modifier_flags(cx, member)? | ModifierFlags::EXPORT;
                let mut factory = cx.factory()?;
                let modifiers = factory.create_modifiers_from_modifier_flags(source, flags)?;
                exported.push(factory.replace_modifiers(member, modifiers)?);
            }
            *self.expando_members_mut(host)? = exported;
        }
        self.expando_members_mut(host)?.extend(statements);
        Ok(())
    }

    fn expando_members_mut(
        &mut self,
        host: TransformNode,
    ) -> Result<&mut Vec<TransformNode>, TransformError> {
        Ok(self
            .state_mut()?
            .expandos
            .members
            .entry(host.node())
            .or_default())
    }

    fn has_expando_export_declaration(
        &self,
        cx: &TransformationContext,
        host: TransformNode,
    ) -> Result<bool, TransformError> {
        let Some(members) = self.state()?.expandos.members.get(&host.node()) else {
            return Ok(false);
        };
        for &member in members {
            if cx.arena().node(member)?.kind == SyntaxKind::ExportDeclaration {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// tsgo ensureType for the assignment, in the namespace tsgo synthesizes
    /// whose only local is the member (transform.go:2810-2836): a JSDoc
    /// `@type` reused through the node builder, else the property's
    /// serialized type.
    fn expando_member_type(
        &mut self,
        cx: &mut TransformationContext,
        node: TransformNode,
        local_name: Option<&str>,
    ) -> Result<Option<TransformNode>, TransformError> {
        if self.has_effective_modifier(cx, node, ModifierFlags::PRIVATE)? {
            return Ok(None);
        }
        if let Some(explicit) = super::javascript::hosted_type(cx, node)? {
            if let Some(reused) = self.try_js_type_node_to_type_node(cx, explicit)? {
                return Ok(Some(reused));
            }
        }
        let old_error_name = self.tracker.error_name_node.take();
        let saved = match self.enter_diagnostic_context(cx, node) {
            Ok(saved) => saved,
            Err(error) => {
                self.tracker.error_name_node = old_error_name;
                return Err(error);
            }
        };
        let result = (|| {
            let declaration = self.required_resolver_node(cx, node)?;
            let enclosing = self.current_enclosing_resolver_node(cx)?;
            let target = self.state()?.current_source_file;
            let result = self.resolver.create_type_of_expando_member(
                cx.arena_mut()?,
                target,
                declaration,
                local_name.unwrap_or_default(),
                enclosing,
                EmitNodeBuilderFlags::DECLARATION_EMIT,
                EmitInternalNodeBuilderFlags::DECLARATION_EMIT,
                &mut self.tracker,
            );
            let effects = self.tracker.take_pending_effects();
            materialize_effects(cx, self.host, effects)?;
            match result.map_err(TransformError::from)? {
                Some(r#type) => Ok(Some(r#type)),
                None => cx
                    .factory()?
                    .create_keyword_type_node(node.source(), SyntaxKind::AnyKeyword)
                    .map(Some),
            }
        })();
        self.tracker.error_name_node = old_error_name;
        if let Some(saved) = saved {
            self.tracker.restore_diagnostic_context(saved);
        }
        result
    }

    /// tsgo setupDiagnosticContext (transform.go:551-572) for a node that
    /// can produce diagnostics; the caller restores the returned context.
    fn enter_diagnostic_context(
        &mut self,
        cx: &TransformationContext,
        node: TransformNode,
    ) -> Result<Option<SavedDiagnosticContext>, TransformError> {
        if self.tracker.suppress_new_diagnostic_contexts
            || !can_produce_diagnostics(cx.arena().node(node)?.kind)
        {
            return Ok(None);
        }
        self.tracker
            .replace_diagnostic_context(cx.arena(), DiagnosticContext::ForNode(node))
            .map(Some)
    }

    /// tsgo getExpandoHostId's root (transform.go:2861-2865).
    fn expando_host_statement(
        &self,
        cx: &TransformationContext,
        declaration: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        if cx.arena().node(declaration)?.kind != SyntaxKind::VariableDeclaration {
            return Ok(declaration);
        }
        self.parent(cx, declaration)?
            .map(|list| self.parent(cx, list))
            .transpose()?
            .flatten()
            .ok_or_else(|| Self::contract("an expando variable needs a statement"))
    }

    /// tsgo transformExpandoHost (transform.go:2867-2922).
    fn transform_expando_host(
        &mut self,
        cx: &mut TransformationContext,
        name: &str,
        declaration: TransformNode,
        host: TransformNode,
    ) -> Result<(), TransformError> {
        if self.state()?.expandos.hosts.contains_key(&host.node()) {
            return Ok(());
        }
        let saved_needs_declare = std::mem::replace(&mut self.state_mut()?.needs_declare, true);
        let flags = self.ensure_modifier_flags(cx, host);
        self.state_mut()?.needs_declare = saved_needs_declare;
        let mut flags = flags?;
        let default_export =
            flags.contains(ModifierFlags::EXPORT) && flags.contains(ModifierFlags::DEFAULT);
        if default_export {
            flags = ModifierFlags::from_bits(
                (flags.bits() | ModifierFlags::AMBIENT.bits())
                    & !(ModifierFlags::DEFAULT.bits() | ModifierFlags::EXPORT.bits()),
            );
        }
        let saved = self.enter_diagnostic_context(cx, declaration)?;
        let function = self.expando_host_function(cx, name, declaration, flags);
        if let Some(saved) = saved {
            self.tracker.restore_diagnostic_context(saved);
        }
        let Some(function) = function? else {
            let result = super::statements::transform_top_level_declaration(self, cx, declaration)?;
            self.state_mut()?.expandos.hosts.insert(host.node(), result);
            return Ok(());
        };
        if self.options.isolated_declarations == Some(true) {
            self.report_expando_function_errors(cx, declaration)?;
        }
        let mut replacement = vec![function];
        if default_export {
            if super::statements::is_source_file_parent(cx, declaration)? {
                self.state_mut()?.result_has_external_module_indicator = true;
            }
            self.state_mut()?.result_has_scope_marker = true;
            let mut factory = cx.factory()?;
            let name = factory.create_identifier(declaration.source(), name)?;
            replacement.push(factory.create_export_assignment(
                declaration.source(),
                None,
                false,
                name,
            )?);
        }
        self.state_mut()?
            .expandos
            .hosts
            .insert(host.node(), VisitResult::Nodes(replacement));
        if self
            .state()?
            .late_statement_replacement
            .contains_key(&host.node())
        {
            let block = self.create_full_expando_block(cx, host)?;
            self.state_mut()?
                .late_statement_replacement
                .insert(host.node(), block);
        }
        Ok(())
    }

    /// The function declaration that replaces a function host, or a
    /// variable initialized to a function expression or arrow function;
    /// `None` for any other host.
    fn expando_host_function(
        &mut self,
        cx: &mut TransformationContext,
        name: &str,
        declaration: TransformNode,
        flags: ModifierFlags,
    ) -> Result<Option<TransformNode>, TransformError> {
        let source = declaration.source();
        let function = match &cx.arena().node(declaration)?.data {
            NodeData::FunctionDeclaration(_) => declaration,
            NodeData::VariableDeclaration(data) => {
                let Some(initializer) = data
                    .initializer
                    .map(|initializer| TransformNode::new(source, initializer))
                else {
                    return Ok(None);
                };
                if !matches!(
                    cx.arena().node(initializer)?.kind,
                    SyntaxKind::FunctionExpression | SyntaxKind::ArrowFunction
                ) {
                    return Ok(None);
                }
                initializer
            }
            _ => return Ok(None),
        };
        let (type_parameters, parameters, asterisk) = match &cx.arena().node(function)?.data {
            NodeData::FunctionDeclaration(data) => {
                (data.type_parameters, data.parameters, data.asterisk_token)
            }
            NodeData::FunctionExpression(data) => {
                (data.type_parameters, data.parameters, data.asterisk_token)
            }
            NodeData::ArrowFunction(data) => (data.type_parameters, data.parameters, None),
            _ => return Ok(None),
        };
        let asterisk = asterisk.and_then(|token| cx.arena().node_ref(source, token));
        let modifiers = cx
            .factory()?
            .create_modifiers_from_modifier_flags(source, flags)?;
        let type_parameters = self.ensure_type_params(cx, function, type_parameters)?;
        let parameters = self.update_params_list(
            cx,
            function,
            parameters,
            ModifierFlags::from_bits(ModifierFlags::ALL.bits() ^ ModifierFlags::PUBLIC.bits()),
        )?;
        let return_type = self.ensure_type(cx, function, false)?;
        let mut factory = cx.factory()?;
        if function == declaration {
            let name = match &factory.arena().node(declaration)?.data {
                NodeData::FunctionDeclaration(data) => data.name,
                _ => None,
            }
            .and_then(|name| factory.arena().node_ref(source, name));
            factory
                .update_function_declaration(
                    declaration,
                    modifiers,
                    asterisk,
                    name,
                    type_parameters,
                    parameters,
                    return_type,
                    None,
                )
                .map(Some)
        } else {
            let name = factory.create_identifier(source, name)?;
            factory
                .create_function_declaration(
                    source,
                    modifiers,
                    asterisk,
                    Some(name),
                    type_parameters,
                    parameters,
                    return_type,
                    None,
                )
                .map(Some)
        }
    }

    /// tsgo's reportExpandoFunctionErrors (transform.go:117-131): an error
    /// at each property assigned to the function.
    pub(crate) fn report_expando_function_errors(
        &mut self,
        cx: &mut TransformationContext,
        declaration: TransformNode,
    ) -> Result<(), TransformError> {
        let properties = self
            .resolver
            .get_properties_of_container_function(self.required_resolver_node(cx, declaration)?)?;
        self.tracker.report_expando_function_errors(&properties);
        let effects = self.tracker.take_pending_effects();
        materialize_effects(cx, self.host, effects)
    }

    /// Whether a statement is an expando host or has deferred assignments
    /// (transform.go:1742-1748).
    pub(crate) fn is_expando_host(&self, host: NodeId) -> Result<bool, TransformError> {
        let expandos = &self.state()?.expandos;
        Ok(expandos.hosts.contains_key(&host) || expandos.deferred.contains_key(&host))
    }

    /// tsgo createFullExpandoBlock (transform.go:2924-2970): the host's
    /// declarations, then a namespace of its members named and modified like
    /// the first named declaration.
    pub(crate) fn create_full_expando_block(
        &mut self,
        cx: &mut TransformationContext,
        host: TransformNode,
    ) -> Result<VisitResult, TransformError> {
        // The assignments deferred while the host was not visible; any that
        // are still not visible are deferred again.
        if let Some(deferred) = self.state_mut()?.expandos.deferred.remove(&host.node()) {
            for assignment in deferred {
                let assignment = TransformNode::new(host.source(), assignment);
                self.transform_expando_assignment(cx, assignment)?;
            }
        }
        let result = self
            .state()?
            .expandos
            .hosts
            .get(&host.node())
            .cloned()
            .unwrap_or(VisitResult::None);
        let Some(members) = self.state()?.expandos.members.get(&host.node()).cloned() else {
            return Ok(result);
        };
        let nodes = match result {
            VisitResult::None => return Ok(VisitResult::None),
            VisitResult::Node(node) => vec![node],
            VisitResult::Nodes(nodes) => nodes,
        };
        let mut named = None;
        for &node in &nodes {
            let (name, modifiers) = match &cx.arena().node(node)?.data {
                NodeData::FunctionDeclaration(data) => (data.name, data.modifiers),
                NodeData::ClassDeclaration(data) => (data.name, data.modifiers),
                NodeData::InterfaceDeclaration(data) => (data.name, data.modifiers),
                NodeData::TypeAliasDeclaration(data) => (data.name, data.modifiers),
                NodeData::EnumDeclaration(data) => (data.name, data.modifiers),
                NodeData::ModuleDeclaration(data) => (data.name, data.modifiers),
                _ => (None, None),
            };
            if let Some(name) = name.and_then(|name| cx.arena().node_ref(node.source(), name)) {
                let modifiers =
                    modifiers.and_then(|list| cx.arena().node_array_ref(node.source(), list));
                named = Some((name, modifiers));
                break;
            }
        }
        let Some((name, modifiers)) = named else {
            return Ok(VisitResult::Nodes(nodes));
        };
        let source = host.source();
        let mut factory = cx.factory()?;
        // tsgo's Clone keeps the original's text range, so the namespace's
        // name maps to the host's name in a declaration map.
        let name = clone_keeping_range(&mut factory, name)?;
        let modifiers = match modifiers {
            Some(list) => {
                let (originals, pos, end) = {
                    let array = factory.arena().node_array(list)?;
                    (array.nodes.to_vec(), array.pos, array.end)
                };
                let mut cloned = Vec::with_capacity(originals.len());
                for modifier in originals {
                    let modifier = TransformNode::new(list.source(), modifier);
                    cloned.push(clone_keeping_range(&mut factory, modifier)?);
                }
                let cloned = factory.create_node_array(source, cloned)?;
                factory.set_node_array_text_range(cloned, pos, end)?;
                Some(cloned)
            }
            None => None,
        };
        let statements = factory.create_node_array(source, members)?;
        let block = factory.create_module_block(source, statements)?;
        let namespace = factory.create_module_declaration(
            source,
            modifiers,
            name,
            None,
            Some(block),
            NodeFlags::NAMESPACE,
        )?;
        let mut nodes = nodes;
        nodes.push(namespace);
        Ok(VisitResult::Nodes(nodes))
    }
}

/// tsgo Node.Clone: a copy with the original's text range (ast.go:103-120).
fn clone_keeping_range(
    factory: &mut crate::NodeFactory<'_>,
    original: TransformNode,
) -> Result<TransformNode, TransformError> {
    let clone = factory.clone_node(original)?;
    factory.set_text_range(clone, original)
}

/// `export { specifier };`
fn named_exports_declaration(
    factory: &mut crate::NodeFactory<'_>,
    source: TransformSourceId,
    specifier: TransformNode,
) -> Result<TransformNode, TransformError> {
    let specifiers = factory.create_node_array(source, vec![specifier])?;
    let exports = factory.create_named_exports(source, specifiers)?;
    factory.create_export_declaration(source, None, false, Some(exports), None, None)
}
