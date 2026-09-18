#!/usr/bin/env python3
"""r10 patches, part 2: EF7-EXPORTED-REST-HOIST (es2018), EF7-SYSTEM-IMPORT-HELPERS (system + shared collector),
EF7-CJS-FLATTENED-EXPORT-NAME (CommonJS publication name / synthetic range)."""
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
        assert n == expected, (path, n, expected, old[:100])
        s = s.replace(old, new)
    open(p, "w").write(s)
    print("patched", path)

# ---------------- EF7-EXPORTED-REST-HOIST (es2018.rs) ----------------
patch("crates/emitter/src/builtins/es2018.rs", [
('''    async_generator_super_captures: Vec<Option<AsyncGeneratorSuperCapture>>,
    iteration_depth: usize,
}
''',
'''    async_generator_super_captures: Vec<Option<AsyncGeneratorSuperCapture>>,
    iteration_depth: usize,
    /// tsc's `exportedVariableStatement`: set while visiting an exported
    /// variable statement so its object-rest declarations hoist their temps
    /// (`hoistTempVariables`) instead of declaring them
    /// (EF7-EXPORTED-REST-HOIST).
    exported_variable_statement: bool,
    hoist_destructuring_temps: bool,
}
'''),
('''            async_generator_super_captures: Vec::new(),
            iteration_depth: 0,
        })''',
'''            async_generator_super_captures: Vec::new(),
            iteration_depth: 0,
            exported_variable_statement: false,
            hoist_destructuring_temps: false,
        })'''),
('''            NodeData::VariableDeclarationList(data) => {
                Some(self.visit_variable_declaration_list(original, data)?)
            }''',
'''            NodeData::VariableStatement(data)
                if super::has_modifier(
                    self.context.arena(),
                    self.source,
                    data.modifiers,
                    SyntaxKind::ExportKeyword,
                )? =>
            {
                // visitVariableStatement: `exportedVariableStatement = true`
                // while the statement's own declarations are visited.
                let saved = std::mem::replace(&mut self.exported_variable_statement, true);
                let visited = self.update_generic(original, NodeData::VariableStatement(data));
                self.exported_variable_statement = saved;
                Some(visited?)
            }
            NodeData::VariableDeclarationList(data) => {
                Some(self.visit_variable_declaration_list(original, data)?)
            }'''),
('''        let declarations = self.array_nodes(data.declarations)?;
        let mut lowered = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let NodeData::VariableDeclaration(declaration_data) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclarationList,
                    field: "declaration",
                });
            };''',
'''        // visitVariableDeclaration: the statement's `exportedVariableStatement`
        // applies to its own declarations and is cleared for nested visits.
        let exported = std::mem::replace(&mut self.exported_variable_statement, false);
        let declarations = self.array_nodes(data.declarations)?;
        let mut lowered = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            let NodeData::VariableDeclaration(declaration_data) =
                self.context.arena().node(declaration)?.data.clone()
            else {
                return Err(TransformError::RequiredChildRemoved {
                    parent: SyntaxKind::VariableDeclarationList,
                    field: "declaration",
                });
            };'''),
('''            if self.pattern_contains_object_rest(name)? {
                lowered.extend(self.flatten_destructuring_binding(
                    declaration,
                    declaration_data,
                    None,
                    false,
                    HelperRequestMode::Immediate,
                )?);
            } else if let Some(visited) = self.visit(declaration.node())? {
                lowered.push(self.node(visited));
            }
        }''',
'''            if self.pattern_contains_object_rest(name)? {
                let saved = std::mem::replace(&mut self.hoist_destructuring_temps, exported);
                let flattened = self.flatten_destructuring_binding(
                    declaration,
                    declaration_data,
                    None,
                    false,
                    HelperRequestMode::Immediate,
                );
                self.hoist_destructuring_temps = saved;
                lowered.extend(flattened?);
            } else if let Some(visited) = self.visit(declaration.node())? {
                lowered.push(self.node(visited));
            }
        }
        self.exported_variable_statement = exported;'''),
('''        let mut plan = match helper_request_mode {
            HelperRequestMode::Immediate => DestructuringPlan::new(DestructuringMode::Binding),
            HelperRequestMode::AfterFunctionBody => DestructuringPlan::parameter_binding(),
        };''',
'''        let mut plan = match helper_request_mode {
            HelperRequestMode::Immediate => DestructuringPlan::new(DestructuringMode::Binding),
            HelperRequestMode::AfterFunctionBody => DestructuringPlan::parameter_binding(),
        };
        plan.hoist_temp_variables = self.hoist_destructuring_temps;'''),
('''    has_transformed_prior_array_element: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelperRequestMode {''',
'''    has_transformed_prior_array_element: bool,
    /// `flattenDestructuringBinding(…, hoistTempVariables)`: temps are
    /// hoisted and their assignments become pending expressions inlined
    /// ahead of the next binding's value (`{ x } = (_a = value, _a)`).
    hoist_temp_variables: bool,
    pending_expressions: Vec<TransformNode>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HelperRequestMode {'''),
('''            has_transformed_prior_array_element: false,
        }''',
'''            has_transformed_prior_array_element: false,
            hoist_temp_variables: false,
            pending_expressions: Vec::new(),
        }''', 2),
('''    fn ensure_destructuring_identifier(
        &mut self,
        plan: &mut DestructuringPlan,
        value: TransformNode,
        reuse_identifier: bool,
        original: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if reuse_identifier && self.context.arena().node(value)?.kind == SyntaxKind::Identifier {
            return Ok(value);
        }
        let binding = self.allocate_destructuring_temp(plan.mode)?;
        let target = self.create_generated_identifier(&binding)?;
        let read = self.create_generated_identifier(&binding)?;
        plan.push(target, value, original);
        Ok(read)
    }''',
'''    /// `emitBindingOrAssignment` for bindings: pending hoisted-temp
    /// assignments are inlined ahead of the value (`(_a = v, _a)`).
    fn plan_push(
        &mut self,
        plan: &mut DestructuringPlan,
        target: TransformNode,
        value: TransformNode,
        original: Option<TransformNode>,
    ) -> Result<(), TransformError> {
        let value = if plan.pending_expressions.is_empty() {
            value
        } else {
            let mut expressions = std::mem::take(&mut plan.pending_expressions);
            expressions.push(value);
            self.inline_expressions(expressions)?
        };
        plan.push(target, value, original);
        Ok(())
    }

    fn ensure_destructuring_identifier(
        &mut self,
        plan: &mut DestructuringPlan,
        value: TransformNode,
        reuse_identifier: bool,
        original: Option<TransformNode>,
    ) -> Result<TransformNode, TransformError> {
        if reuse_identifier && self.context.arena().node(value)?.kind == SyntaxKind::Identifier {
            return Ok(value);
        }
        if plan.hoist_temp_variables {
            // ensureIdentifier with `hoistTempVariables`: the temp is hoisted
            // and `temp = value` is emitted as a pending expression.
            let binding = self.allocate_destructuring_temp(DestructuringMode::Assignment)?;
            let target = self.create_generated_identifier(&binding)?;
            let read = self.create_generated_identifier(&binding)?;
            let assignment = self.create_assignment(target, value)?;
            if let Some(original) = original {
                self.context.factory()?.set_text_range(assignment, original)?;
            }
            plan.pending_expressions.push(assignment);
            return Ok(read);
        }
        let binding = self.allocate_destructuring_temp(plan.mode)?;
        let target = self.create_generated_identifier(&binding)?;
        let read = self.create_generated_identifier(&binding)?;
        self.plan_push(plan, target, value, original)?;
        Ok(read)
    }'''),
('''        // makeObjectAssignmentPattern creates a fresh object literal. Its
        // chunk no longer spans the original rest element or closing brace.
        plan.push(pattern, value, original);
        Ok(())
    }''',
'''        // makeObjectAssignmentPattern creates a fresh object literal. Its
        // chunk no longer spans the original rest element or closing brace.
        self.plan_push(plan, pattern, value, original)
    }'''),
('''        self.set_original_and_range(retained_pattern, pattern)?;
        plan.push(retained_pattern, value, original);''',
'''        self.set_original_and_range(retained_pattern, pattern)?;
        self.plan_push(plan, retained_pattern, value, original)?;'''),
('''                        })?
                    }
                };
                plan.push(target, value, original);
                Ok(())
            }''',
'''                        })?
                    }
                };
                self.plan_push(plan, target, value, original)
            }'''),
('''    fn materialize_binding_plan(
        &mut self,
        plan: DestructuringPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        debug_assert_eq!(plan.mode, DestructuringMode::Binding);
        plan.steps''',
'''    fn materialize_binding_plan(
        &mut self,
        mut plan: DestructuringPlan,
    ) -> Result<Vec<TransformNode>, TransformError> {
        debug_assert_eq!(plan.mode, DestructuringMode::Binding);
        if !plan.pending_expressions.is_empty() {
            // flattenDestructuringBinding tail (`hoistTempVariables`): leftover
            // pending expressions bind a fresh non-hoisted temp.
            let binding = self.allocate_destructuring_temp(DestructuringMode::Binding)?;
            let target = self.create_generated_identifier(&binding)?;
            let expressions = std::mem::take(&mut plan.pending_expressions);
            let value = self.inline_expressions(expressions)?;
            plan.push(target, value, None);
        }
        plan.steps'''),
])

# ---------------- shared helpers-import collector (builtins.rs) ----------------
p = os.path.join(root, "crates/emitter/src/builtins.rs")
s = open(p).read()
start_marker = "    fn collect_external_helpers_import(\n        &self,\n        context: &mut TransformationContext,\n        source: TransformSourceId,\n        info: &mut CommonJsModuleInfo,\n    ) -> Result<(), TransformError> {\n"
end_marker = "        Ok(())\n    }\n}\n\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\nenum ImportHelperKind {"
if "fn collect_external_helpers_import_declaration(" not in s:
    assert s.count(start_marker) == 1 and s.count(end_marker) == 1
    a = s.index(start_marker); b = s.index(end_marker)
    body = s[a + len(start_marker): b]  # method body without the final Ok(())
    assert "self.es_module_interop" in body and "self.module_kind" in body and "self.host" in body
    free_body = body.replace("self.es_module_interop", "es_module_interop").replace("self.module_kind", "module_kind").replace("self.host", "host")
    assert "self." not in free_body, [l for l in free_body.splitlines() if "self." in l]
    wrapper = start_marker + "        collect_external_helpers_import_declaration(\n            context,\n            source,\n            info,\n            self.module_kind,\n            self.es_module_interop,\n            self.host,\n        )\n    }\n}\n\n"
    free_fn = ("/// The `import tslib_1 = require(\"tslib\")` declaration shared by the\n"
               "/// CommonJS-family and System transforms (createExternalHelpersImportDeclarationIfNeeded\n"
               "/// through collectExternalModuleInfo, _tsc.js:92875; EF7-SYSTEM-IMPORT-HELPERS).\n"
               "fn collect_external_helpers_import_declaration(\n    context: &mut TransformationContext,\n    source: TransformSourceId,\n    info: &mut CommonJsModuleInfo,\n    module_kind: i32,\n    es_module_interop: bool,\n    host: Option<&dyn EmitHost>,\n) -> Result<(), TransformError> {\n"
               + free_body + "        Ok(())\n}\n\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\nenum ImportHelperKind {")
    s = s[:a] + wrapper + free_fn + s[b + len(end_marker):]
    open(p, "w").write(s)
    print("patched crates/emitter/src/builtins.rs (collector)")

# ---------------- EF7-CJS-FLATTENED-EXPORT-NAME (builtins.rs) ----------------
patch("crates/emitter/src/builtins.rs", [
('''            let publication_name = match &self.context.arena().node(original_declaration)?.data {
                NodeData::VariableDeclaration(data) => data
                    .name
                    .and_then(|id| {
                        self.context
                            .arena()
                            .node_ref(original_declaration.source(), id)
                    })
                    .or(local_name),
                _ => local_name,
            };''',
'''            let publication_name = match &self.context.arena().node(original_declaration)?.data {
                NodeData::VariableDeclaration(data) => data
                    .name
                    .and_then(|id| {
                        self.context
                            .arena()
                            .node_ref(original_declaration.source(), id)
                    })
                    // A flattened binding pattern's temp declaration keeps
                    // its generated name (`exports._c = _a = []`,
                    // transformInitializedVariable reads `node.name`);
                    // only an identifier original donates its spelling.
                    .filter(|name| {
                        self.context
                            .arena()
                            .node(*name)
                            .is_ok_and(|node| node.kind == SyntaxKind::Identifier)
                    })
                    .or(local_name),
                _ => local_name,
            };'''),
('''            SourceRange::from_raw(name.pos, variable_statement_record.end, source.positions())
                .map(|range| SourceMapRange::new(variable_statement.source(), range))
                .map_err(|error| TransformError::InvalidSourceRange {
                    node: variable_statement,
                    error,
                })?''',
'''            // A generated name (a flattened binding pattern's temp) has no
            // position; the statement then keeps the variable statement's
            // own start.
            let start = if name.pos == u32::MAX {
                variable_statement_record.pos
            } else {
                name.pos
            };
            SourceRange::from_raw(start, variable_statement_record.end, source.positions())
                .map(|range| SourceMapRange::new(variable_statement.source(), range))
                .map_err(|error| TransformError::InvalidSourceRange {
                    node: variable_statement,
                    error,
                })?'''),
])

# ---------------- EF7-SYSTEM-IMPORT-HELPERS (system.rs) ----------------
patch("crates/emitter/src/builtins/system.rs", [
('''use crate::{
    factory::EmitHelperName, EmitExportContainerMode, EmitHint, EmitHost, EmitResolver,''',
'''use crate::{
    factory::EmitHelperName, EmitExportContainerMode, EmitFlags, EmitHint, EmitHost, EmitResolver,'''),
('''    Box::new(SystemModuleTransformer {
        resolver,
        host,
        always_strict: options.always_strict_effective(),
    })
}

struct SystemModuleTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    host: Option<&'resolver dyn EmitHost>,
    always_strict: bool,
}''',
'''    Box::new(SystemModuleTransformer {
        resolver,
        host,
        always_strict: options.always_strict_effective(),
        import_helpers: options.import_helpers == Some(true),
        es_module_interop: options.es_module_interop == Some(true),
        current_source: None,
    })
}

struct SystemModuleTransformer<'resolver> {
    resolver: &'resolver dyn EmitResolver,
    host: Option<&'resolver dyn EmitHost>,
    always_strict: bool,
    import_helpers: bool,
    es_module_interop: bool,
    current_source: Option<TransformSourceId>,
}'''),
('''        let common = CommonJsModuleInfo::collect(
            context.arena(),
            source,
            root,
            self.resolver,
            super::MODULE_SYSTEM,
        )?;
        let info = SystemModuleInfo::collect(''',
'''        self.current_source = Some(source);
        let mut common = CommonJsModuleInfo::collect(
            context.arena(),
            source,
            root,
            self.resolver,
            super::MODULE_SYSTEM,
        )?;
        // createExternalHelpersImportDeclarationIfNeeded through
        // collectExternalModuleInfo (92875): the tslib import leads the
        // dependency groups (EF7-SYSTEM-IMPORT-HELPERS).
        if self.import_helpers && is_external {
            super::collect_external_helpers_import_declaration(
                context,
                source,
                &mut common,
                super::MODULE_SYSTEM,
                self.es_module_interop,
                self.host,
            )?;
        }
        let info = SystemModuleInfo::collect('''),
('''    fn substitute_node(
        &mut self,
        _context: &mut TransformationContext,
        _hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // The Rust transform performs the upstream substitutions while its
        // arena is mutable. The hook is still installed so hook composition
        // and activity remain observable.
        Ok(node)
    }''',
'''    fn substitute_node(
        &mut self,
        context: &mut TransformationContext,
        hint: EmitHint,
        node: TransformNode,
    ) -> Result<TransformNode, TransformError> {
        // The Rust transform performs the upstream substitutions while its
        // arena is mutable. Helper-name qualification (`tslib_1.__extends`)
        // belongs to the print-time hook, exactly as in the CommonJS
        // transform (substituteExpressionIdentifier, EmitFlags.HelperName;
        // EF7-SYSTEM-IMPORT-HELPERS).
        if hint != EmitHint::Expression
            || context.arena().node(node)?.kind != SyntaxKind::Identifier
            || !context
                .arena()
                .metadata(node)
                .is_some_and(|metadata| metadata.flags().contains(EmitFlags::HELPER_NAME))
        {
            return Ok(node);
        }
        let Some(source) = self.current_source else {
            return Ok(node);
        };
        let Some(namespace) = super::get_external_helpers_module_name(context.arena(), source)?
        else {
            return Ok(node);
        };
        let final_name = context
            .arena()
            .metadata(namespace)
            .and_then(crate::EmitMetadata::generated_binding_id)
            .and_then(|binding| context.generated_binding_name(binding))
            .map(str::to_owned);
        if let Some(final_name) = final_name {
            context
                .arena_mut()?
                .set_generated_identifier_text(namespace, &final_name)?;
        }
        context
            .substitution_factory()?
            .create_property_access_expression(source, namespace, node)
    }'''),
('''        Ok(Self {
            common,
            dependency_groups,
            non_function_exported_names,
        })''',
'''        if let Some(declaration) = common.external_helpers_import_declaration {
            // `externalImports.unshift(externalHelpersImportDeclaration)`:
            // the helpers module is the first System dependency.
            dependency_groups.insert(
                0,
                SystemDependencyGroup {
                    module_specifier: JsString::from("tslib"),
                    entries: vec![declaration.node()],
                },
            );
        }
        Ok(Self {
            common,
            dependency_groups,
            non_function_exported_names,
        })'''),
('''        // equals lowering, even though setter entries are grouped separately.
        for statement in statements {''',
'''        // equals lowering, even though setter entries are grouped separately.
        if let Some(declaration) = self.info.common.external_helpers_import_declaration {
            // The synthesized `import tslib_1 = require("tslib")` is not a
            // source statement; its local is hoisted first (the helpers
            // import leads `externalImports`).
            if let NodeData::ImportEqualsDeclaration(data) =
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
        for statement in statements {'''),
])
print("r10 part 2 applied")
