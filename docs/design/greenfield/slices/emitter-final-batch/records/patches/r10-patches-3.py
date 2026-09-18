#!/usr/bin/env python3
"""r10 patches, part 3: EF7-CJS-EXPORT-INLINE (one expression statement per exported variable statement, generated
names excluded from the hoisted export list), EF7-ASYNC-ACCESSOR-BODY (ES2017), EF7-ASYNC-SHORTHAND-ASSIGNMENT (ES2017),
EF7-ASYNC-HELPER-CHECKS (checker), EF7-NOCHECK-ROUTE (emitter option gate), EF7-SYSTEM-IMPORT-EQUALS-EXPORTS,
EF7-SYSTEM-NAMESPACE-ALIAS, EF7-SYSTEM-IMPORT-HELPERS follow-up (namespace binding identity + setter parameter)."""
import sys, os
root = sys.argv[1] if len(sys.argv) > 1 else "."

def patch(path, pairs):
    p = os.path.join(root, path)
    s = open(p).read()
    for entry in pairs:
        old, new = entry[0], entry[1]
        expected = entry[2] if len(entry) > 2 else 1
        if s.count(old) == 0 and s.count(new) >= 1:
            continue
        n = s.count(old)
        assert n == expected, (path, n, expected, old[:120])
        s = s.replace(old, new)
    open(p, "w").write(s)
    print("patched", path)

# ---------------- EF7-CJS-EXPORT-INLINE (builtins.rs, CommonJS visitor) ----------------
patch("crates/emitter/src/builtins.rs", [
# generated identifiers never enter the hoisted export list (collectExportedVariableInfo: !isGeneratedIdentifier)
('''                            for leaf in
                                binding_name_leaves(arena, source, variable.name, declaration)?
                            {
                                let local = identifier_text_owned(arena, leaf.name)?;
                                if !unique_exports.insert(local.clone().into()) {
                                    continue;
                                }''',
'''                            for leaf in
                                binding_name_leaves(arena, source, variable.name, declaration)?
                            {
                                // collectExportedVariableInfo: `!isGeneratedIdentifier(decl.name)`
                                // — a flattened pattern's temp declaration
                                // (`exports._c = _a = []`) is not a hoisted export.
                                if arena
                                    .metadata(leaf.name)
                                    .and_then(crate::EmitMetadata::generated_binding_id)
                                    .is_some()
                                {
                                    continue;
                                }
                                let local = identifier_text_owned(arena, leaf.name)?;
                                if !unique_exports.insert(local.clone().into()) {
                                    continue;
                                }'''),
# the loop state
('''        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
        let mut retained = Vec::new();
        let mut trailing = Vec::new();
        for declaration in declarations {
            let NodeData::VariableDeclaration(mut variable) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                retained.push(self.visit(declaration.node())?);
                continue;
            };''',
'''        data.modifiers = self.remove_export_modifiers(data.modifiers)?;
        let mut retained = Vec::new();
        let mut trailing = Vec::new();
        // visitVariableStatement: every exported initializer joins ONE
        // expression statement (`inlineExpressions(expressions)`), ranged to
        // the variable statement; alias publications follow it
        // (EF7-CJS-EXPORT-INLINE).
        let mut exported_expressions: Vec<TransformNode> = Vec::new();
        let mut exported_range_name: Option<Option<TransformNode>> = None;
        let mut remove_comments_on_expressions = false;
        for declaration in declarations {
            let NodeData::VariableDeclaration(mut variable) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                retained.push(self.visit(declaration.node())?);
                continue;
            };'''),
# binding pattern arm
('''                    let expression = self.flatten_module_destructuring_declaration(
                        pattern,
                        self.node(initializer),
                        declaration,
                    )?;
                    let statement = self.create_expression_statement(expression)?;
                    self.set_original_and_range(statement, original)?;
                    trailing.push(statement);
                }
                continue;
            }''',
'''                    let expression = self.flatten_module_destructuring_declaration(
                        pattern,
                        self.node(initializer),
                        declaration,
                    )?;
                    exported_expressions.push(expression);
                    exported_range_name.get_or_insert(None);
                }
                continue;
            }'''),
# function-initializer arm
('''                let name = publication_name.expect("a direct export-object plan owns a name");
                let target = self.create_export_access_from_name(name)?;
                let value = self.create_identifier(&plan.local_name)?;
                let assignment = self.create_assignment(target, value)?;
                let statement = self.create_expression_statement(assignment)?;
                self.set_original_and_range(statement, original)?;
                self.set_direct_export_statement_range(statement, name, original)?;
                self.context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .add_flags(EmitFlags::NO_COMMENTS);
                trailing.push(statement);
                for export in plan.alias_targets() {''',
'''                let name = publication_name.expect("a direct export-object plan owns a name");
                let target = self.create_export_access_from_name(name)?;
                let value = self.create_identifier(&plan.local_name)?;
                let assignment = self.create_assignment(target, value)?;
                exported_expressions.push(assignment);
                exported_range_name.get_or_insert(Some(name));
                remove_comments_on_expressions = true;
                for export in plan.alias_targets() {'''),
# ordinary initializer arm
('''                    let name = publication_name.expect("a direct export-object plan owns a name");
                    let target = self.create_export_access_from_name(name)?;
                    let assignment = self.create_assignment(target, initializer)?;
                    let statement = self.create_expression_statement(assignment)?;
                    self.set_original_and_range(statement, original)?;
                    self.set_direct_export_statement_range(statement, name, original)?;
                    trailing.push(statement);
                    for export in plan.alias_targets() {''',
'''                    let name = publication_name.expect("a direct export-object plan owns a name");
                    let target = self.create_export_access_from_name(name)?;
                    let assignment = self.create_assignment(target, initializer)?;
                    exported_expressions.push(assignment);
                    exported_range_name.get_or_insert(Some(name));
                    for export in plan.alias_targets() {'''),
# the combined statement
('''        let mut result = Vec::new();
        if !retained.is_empty() {
            let list = data
                .declaration_list
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableStatement,
                    field: "declaration_list",
                })?;''',
'''        if !exported_expressions.is_empty() {
            let mut expressions = exported_expressions.into_iter();
            let mut expression = expressions.next().expect("non-empty");
            for right in expressions {
                expression = self.create_binary(expression, SyntaxKind::CommaToken, right)?;
            }
            let statement = self.create_expression_statement(expression)?;
            self.set_original_and_range(statement, original)?;
            if let Some(Some(name)) = exported_range_name {
                self.set_direct_export_statement_range(statement, name, original)?;
            }
            if remove_comments_on_expressions {
                self.context
                    .arena_mut()?
                    .metadata_mut(statement)
                    .add_flags(EmitFlags::NO_COMMENTS);
            }
            trailing.insert(0, statement);
        }
        let mut result = Vec::new();
        if !retained.is_empty() {
            let list = data
                .declaration_list
                .and_then(|id| self.context.arena().node_ref(self.source, id))
                .ok_or(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableStatement,
                    field: "declaration_list",
                })?;'''),
])

# ---------------- EF7-ASYNC-ACCESSOR-BODY + EF7-ASYNC-SHORTHAND-ASSIGNMENT (es2017.rs) ----------------
patch("crates/emitter/src/builtins/es2017.rs", [
('''        let is_async = self.modifiers_contain_async(modifiers)?;
        let (previous_scope, scope) = self
            .generated_bindings
            .enter(GeneratedBindingOwner::FunctionBody);''',
'''        // visitor: Constructor / GetAccessor / SetAccessor take `visitDefault`
        // (visitEachChild drops the async modifier; the body is not an async
        // function body) — EF7-ASYNC-ACCESSOR-BODY.
        let is_async = self.modifiers_contain_async(modifiers)?
            && !matches!(
                kind,
                SyntaxKind::Constructor | SyntaxKind::GetAccessor | SyntaxKind::SetAccessor
            );
        let (previous_scope, scope) = self
            .generated_bindings
            .enter(GeneratedBindingOwner::FunctionBody);'''),
('''                    let target = self.binding_name_to_assignment_target(target)?;
                    let property_name = element_data
                        .property_name
                        .map(|name| self.node(name))
                        .unwrap_or(target);
                    let initializer = if let Some(initializer) = element_data.initializer {
                        let initializer = self.visit_required(
                            Some(initializer),
                            SyntaxKind::BindingElement,
                            "initializer",
                        )?;
                        self.create_binary(target, SyntaxKind::EqualsToken, initializer)?
                    } else {
                        target
                    };
                    properties.push(self.create_property_assignment(property_name, initializer)?);''',
'''                    let target = self.binding_name_to_assignment_target(target)?;
                    let initializer = match element_data.initializer {
                        Some(initializer) => Some(self.visit_required(
                            Some(initializer),
                            SyntaxKind::BindingElement,
                            "initializer",
                        )?),
                        None => None,
                    };
                    let property = match element_data.property_name.map(|name| self.node(name)) {
                        Some(property_name) => {
                            let value = match initializer {
                                Some(initializer) => self.create_binary(
                                    target,
                                    SyntaxKind::EqualsToken,
                                    initializer,
                                )?,
                                None => target,
                            };
                            self.create_property_assignment(property_name, value)?
                        }
                        // convertToObjectAssignmentElement 20743-20744: no
                        // property name → shorthand `{ x }` / `{ x = init }`
                        // (EF7-ASYNC-SHORTHAND-ASSIGNMENT).
                        None => self.create_shorthand_property_assignment(target, initializer)?,
                    };
                    properties.push(property);'''),
('''    fn create_property_assignment(
        &mut self,
        name: TransformNode,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[name, initializer])?;''',
'''    fn create_shorthand_property_assignment(
        &mut self,
        name: TransformNode,
        object_assignment_initializer: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        let mut children = vec![name];
        children.extend(object_assignment_initializer);
        let flags = self.child_flags(&children)? | TransformFlags::CONTAINS_ES_2015;
        self.context.factory()?.create_node(
            self.source,
            NodeData::ShorthandPropertyAssignment(
                tsc_syntax::nodes::ShorthandPropertyAssignmentData {
                    name: Some(name.node()),
                    equals_token: None,
                    object_assignment_initializer: object_assignment_initializer
                        .map(TransformNode::node),
                    modifiers: None,
                    question_token: None,
                    exclamation_token: None,
                },
            ),
            flags,
        )
    }

    fn create_property_assignment(
        &mut self,
        name: TransformNode,
        initializer: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        let flags = self.child_flags(&[name, initializer])?;'''),
])

# ---------------- EF7-ASYNC-HELPER-CHECKS (checker) ----------------
patch("crates/checker/src/modules.rs", [(
'''pub(crate) const EMIT_HELPER_DECORATE: u32 = 1 << 3;''',
'''pub(crate) const EMIT_HELPER_DECORATE: u32 = 1 << 3;
pub(crate) const EMIT_HELPER_AWAITER: u32 = 1 << 6;
pub(crate) const EMIT_HELPER_GENERATOR: u32 = 1 << 7;
/// `AsyncGeneratorIncludes = Await | AsyncGenerator`.
pub(crate) const EMIT_HELPER_ASYNC_GENERATOR_INCLUDES: u32 = (1 << 11) | (1 << 12);''')])
patch("crates/checker/src/functions.rs", [(
'''            self.check_grammar_function_like_declaration(node)?;
        }
        let (parameters, type_node) = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => (data.parameters, data.r#type),''',
'''            self.check_grammar_function_like_declaration(node)?;
        }
        // 81290-81304: importHelpers requests for the async / generator
        // lowerings (EF7-ASYNC-HELPER-CHECKS).
        let function_flags = self.get_function_flags(node);
        if function_flags & FUNCTION_FLAGS_INVALID == 0 {
            let async_generator = FUNCTION_FLAGS_ASYNC | FUNCTION_FLAGS_GENERATOR;
            let language_version = self.options.emit_script_target();
            if function_flags & async_generator == async_generator
                && language_version < tsc_types::ScriptTarget::ES2018
            {
                self.check_external_emit_helpers(
                    node,
                    crate::modules::EMIT_HELPER_ASYNC_GENERATOR_INCLUDES,
                )?;
            }
            if function_flags & async_generator == FUNCTION_FLAGS_ASYNC
                && language_version < tsc_types::ScriptTarget::ES2017
            {
                self.check_external_emit_helpers(node, crate::modules::EMIT_HELPER_AWAITER)?;
            }
            if function_flags & async_generator != 0
                && language_version < tsc_types::ScriptTarget::ES2015
            {
                self.check_external_emit_helpers(node, crate::modules::EMIT_HELPER_GENERATOR)?;
            }
        }
        let (parameters, type_node) = match self.data_of(node) {
            NodeData::FunctionDeclaration(data) => (data.parameters, data.r#type),''')])

# ---------------- EF7-NOCHECK-ROUTE (emitter option gate) ----------------
patch("crates/emitter/src/execute.rs", [(
'''        (
            operation == EmitOperation::Files
                && options.no_check == Some(true)
                && !route.admits_no_check(),
            "noCheck",
        ),
''',
'''        // `noCheck` has no ordinary-emit refusal left: `skipTypeChecking`
        // already reads the option for every file, and the emit resolver
        // resolves lazily (tsc emits the program unchecked;
        // EF7-NOCHECK-ROUTE).
'''),
('''fn validate_emit_options(
    options: &CompilerOptions,
    operation: EmitOperation,
    route: EmitRouteKind,
) -> Result<(), EmitFailure> {''',
'''fn validate_emit_options(
    options: &CompilerOptions,
    operation: EmitOperation,
    _route: EmitRouteKind,
) -> Result<(), EmitFailure> {'''),
])

# ---------------- System follow-ups (system.rs) ----------------
patch("crates/emitter/src/builtins/system.rs", [
# import-equals exports at the statement position
('''            NodeData::ImportEqualsDeclaration(_) | NodeData::ExportDeclaration(_) => Ok(Vec::new()),''',
'''            NodeData::ImportEqualsDeclaration(data) => {
                // appendExportsOfImportEqualsDeclaration (111684-111689):
                // `export { n2 }` / `export { n2 as n3 }` of an external
                // import-equals publish at its statement position
                // (EF7-SYSTEM-IMPORT-EQUALS-EXPORTS).
                let mut statements = Vec::new();
                if self.info.common.export_equals.is_none() {
                    if let Some(name) = data
                        .name
                        .and_then(|id| self.context.arena().node_ref(self.source, id))
                        .and_then(|name| identifier_text_owned(self.context.arena(), name).ok())
                    {
                        let exports = self
                            .info
                            .common
                            .export_specifiers_by_local
                            .get(name.as_bytes())
                            .cloned()
                            .unwrap_or_default();
                        for export in exports {
                            let value = self.create_identifier(&name)?;
                            let call = self.create_export_call_with_name(&export, value)?;
                            statements.push(self.create_expression_statement(call)?);
                        }
                    }
                }
                Ok(statements)
            }
            NodeData::ExportDeclaration(_) => Ok(Vec::new()),'''),
# no namespace alias: tsc leaves `import d, * as ns` unbound in System output
('''                    if let Some(alias) = self
                        .info
                        .common
                        .imports
                        .get(&key)
                        .and_then(|plan| plan.namespace_alias.as_deref().map(str::to_owned))
                    {
                        self.push_hoisted_name(&alias);
                    }
                }''',
'''                    // getLocalNameForExternalImport: a default import beside
                    // a namespace import hoists only the module's generated
                    // name; tsc leaves the namespace binding unbound
                    // (EF7-SYSTEM-NAMESPACE-ALIAS).
                }'''),
('''                                if let Some(namespace_alias) = plan.namespace_alias.as_deref() {
                                    let target = self.create_identifier(namespace_alias)?;
                                    let value = self.create_identifier(runtime_name)?;
                                    let assignment = self.create_assignment(target, value)?;
                                    statements.push(self.create_expression_statement(assignment)?);
                                }
''',
''''''),
# the helpers namespace keeps its generated identity; the setter parameter derives from its final text
('''            if let NodeData::ImportEqualsDeclaration(data) =
                &self.context.arena().node(declaration)?.data
            {
                if let Some(name) = data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                    .and_then(|name| identifier_text_owned(self.context.arena(), name).ok())
                {
                    self.push_hoisted_name(&name);
                }
            }
        }
        for statement in statements {''',
'''            if let NodeData::ImportEqualsDeclaration(data) =
                &self.context.arena().node(declaration)?.data.clone()
            {
                if let Some(name_node) = data
                    .name
                    .and_then(|id| self.context.arena().node_ref(self.source, id))
                {
                    let text = identifier_text_owned(self.context.arena(), name_node)?;
                    // The hoisted `var` and the setter assignment print the
                    // same generated identity as the helper qualifier, so the
                    // finalize walk spells all three alike (`tslib_1`).
                    if let Some(binding) = self.generated_binding_of_identifier(name_node) {
                        self.generated_bindings.insert(text.clone(), binding);
                    }
                    self.push_hoisted_name(&text);
                }
            }
        }
        for statement in statements {'''),
('''                    NodeData::ImportEqualsDeclaration(data) => {
                        local_name = data
                            .name
                            .and_then(|id| self.context.arena().node_ref(self.source, id))
                            .and_then(|name| {
                                identifier_text_owned(self.context.arena(), name).ok()
                            });
                        break;
                    }''',
'''                    NodeData::ImportEqualsDeclaration(data) => {
                        local_name = data
                            .name
                            .and_then(|id| self.context.arena().node_ref(self.source, id))
                            .and_then(|name| {
                                identifier_text_owned(self.context.arena(), name).ok()
                            });
                        // createSettersArray: `getGeneratedNameForNode(localName)`
                        // derives the parameter from the generated name's FINAL
                        // text (`tslib_1` → `tslib_1_1`).
                        if self.info.common.external_helpers_import_declaration == Some(entry) {
                            local_name = local_name.map(|name| format!("{name}_1"));
                        }
                        break;
                    }'''),
])
print("r10 part 3 applied")
